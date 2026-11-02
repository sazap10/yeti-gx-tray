//! The single thread that owns the device: discovery, request/reply, notifications,
//! periodic re-read, command execution and hotplug recovery.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::io;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::device::{self, Cmd, Device, Event};
use crate::hidpp::{self, Msg};
use crate::mic::Mic;

/// Re-read all fields this often even if no notification arrives.
const REFRESH: Duration = Duration::from_secs(2);
/// Rescan for a missing device this often.
const RESCAN: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_secs(1);

const GAIN_STEP: i32 = 5;

enum Fail {
    /// The device is gone or unresponsive; drop the session and rescan.
    Gone(io::Error),
    /// The device answered with a HID++ error. Fails the request in flight; never ends the
    /// session.
    Rejected(u8),
}

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Self {
        Fail::Gone(e)
    }
}

impl Fail {
    fn describe(&self) -> String {
        match self {
            Fail::Gone(e) => e.to_string(),
            Fail::Rejected(c) => format!("device error 0x{c:02x} ({})", hidpp::error_name(*c)),
        }
    }
}

struct Session {
    dev: Device,
    epoch: u64,
    /// Discovered index of feature 0x8370.
    index: u8,
    mic: Mic,
    /// A notification or a write means the cached state may be stale: re-read soon.
    dirty: bool,
    /// The last refresh failed or a notification overtook it, so `mic` must not seed a
    /// command. Only a refresh that completes with no notification during it clears this.
    stale: bool,
    /// Commands received while waiting for a reply.
    backlog: VecDeque<Cmd>,
}

pub struct Worker<P: FnMut(Option<Mic>)> {
    rx: Receiver<Event>,
    tx: Sender<Event>,
    publish: P,
    shown: Option<Option<Mic>>,
    epoch: u64,
}

impl<P: FnMut(Option<Mic>)> Worker<P> {
    /// `tx` must be the sending half of `rx`; it is cloned into each reader thread.
    pub fn new(rx: Receiver<Event>, tx: Sender<Event>, publish: P) -> Self {
        Self { rx, tx, publish, shown: None, epoch: 0 }
    }

    /// Run forever.
    pub fn run(mut self) {
        loop {
            match self.connect() {
                Ok(mut s) => {
                    eprintln!("connected: {}", s.dev.path.display());
                    let why = self.session(&mut s);
                    eprintln!("disconnected: {}", why.describe());
                }
                Err(Fail::Gone(e)) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => eprintln!("connect failed: {}", e.describe()),
            }
            self.show(None);
            self.pause(RESCAN);
        }
    }

    fn show(&mut self, mic: Option<Mic>) {
        if self.shown != Some(mic) {
            self.shown = Some(mic);
            (self.publish)(mic);
        }
    }

    /// Sleep while discarding tray commands (nothing to send them to).
    fn pause(&self, d: Duration) {
        let end = Instant::now() + d;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            let _ = self.rx.recv_timeout(left);
        }
    }

    fn connect(&mut self) -> Result<Session, Fail> {
        let path = device::find()?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Yeti GX hidraw node"))?;
        self.epoch += 1;
        let dev = Device::open(path, self.tx.clone(), self.epoch)?;
        let mut s = Session {
            dev,
            epoch: self.epoch,
            index: 0,
            mic: Mic { muted: false, gain: 0, smart_lock: false },
            dirty: false,
            stale: true,
            backlog: VecDeque::new(),
        };
        let req = hidpp::get_feature(hidpp::SW_READ);
        let reply = self.request(&mut s, &req, |m| matches!(m, Msg::FeatureIndex(_)))?;
        let Msg::FeatureIndex(index) = reply else { unreachable!() };
        if index == 0 {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "feature 0x8370 not present").into());
        }
        s.index = index;
        self.refresh(&mut s)?;
        Ok(s)
    }

    /// Returns why the session ended.
    fn session(&mut self, s: &mut Session) -> Fail {
        match self.serve(s) {
            Ok(never) => match never {},
            Err(e) => e,
        }
    }

    /// Serve the session until the device is gone; only `Fail::Gone` gets out.
    fn serve(&mut self, s: &mut Session) -> Result<Infallible, Fail> {
        let mut last_refresh = Instant::now();
        loop {
            // Drain everything pending so a burst of notifications costs one re-read.
            while let Ok(ev) = self.rx.try_recv() {
                self.absorb(s, ev)?;
            }
            if !s.backlog.is_empty() {
                // Commands work from the cached state. If the knob, a hardware button or our
                // own last write may have moved it, read the truth first so a command never
                // writes back a value the device has already left.
                if s.dirty {
                    last_refresh = Instant::now();
                    self.refresh_logged(s)?;
                }
                // Still stale (a notification landed mid-read, or a read was rejected): keep
                // the commands queued. A notification re-reads on the next pass; a rejection
                // waits for the periodic refresh, which bounds the retry rate.
                if !s.stale {
                    while let Some(c) = s.backlog.pop_front() {
                        self.apply(s, c)?;
                    }
                }
            }
            if s.dirty || last_refresh.elapsed() >= REFRESH {
                last_refresh = Instant::now();
                self.refresh_logged(s)?;
                continue;
            }
            let wait = REFRESH.saturating_sub(last_refresh.elapsed());
            match self.rx.recv_timeout(wait) {
                Ok(ev) => self.absorb(s, ev)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "event channel closed").into());
                }
            }
        }
    }

    /// `handle` with no request in flight: an error frame nobody is waiting for (another
    /// HID++ client sharing our sw id, say) is logged, not fatal.
    fn absorb(&mut self, s: &mut Session, ev: Event) -> Result<(), Fail> {
        match self.handle(s, ev) {
            Err(e @ Fail::Rejected(_)) => {
                eprintln!("unexpected HID++ error: {}", e.describe());
                Ok(())
            }
            r => r.map(drop),
        }
    }

    /// `refresh`, logging a rejected read instead of ending the session.
    fn refresh_logged(&mut self, s: &mut Session) -> Result<(), Fail> {
        match self.refresh(s) {
            Err(e @ Fail::Rejected(_)) => {
                eprintln!("refresh: {}", e.describe());
                Ok(())
            }
            r => r,
        }
    }

    /// Absorb one event. Returns a candidate reply message for `request`.
    fn handle(&mut self, s: &mut Session, ev: Event) -> Result<Option<Msg>, Fail> {
        match ev {
            Event::Cmd(c) => s.backlog.push_back(c),
            Event::ReadFailed { epoch } if epoch == s.epoch => {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "hidraw read failed").into());
            }
            Event::Report { epoch, data } if epoch == s.epoch => {
                // Errors count only for our feature (or the root feature, index 0, while
                // `s.index` is still 0 during discovery); other clients' errors are not ours.
                match hidpp::decode(&data, hidpp::SW_WRITE) {
                    Some(Msg::Error { feature, code }) if feature == s.index => {
                        eprintln!("write rejected: {} (0x{code:02x})", hidpp::error_name(code));
                    }
                    _ => {}
                }
                match hidpp::decode(&data, hidpp::SW_READ) {
                    Some(Msg::Notify { feature, .. }) if feature == s.index => s.dirty = true,
                    Some(Msg::Error { feature, code }) if feature == s.index => {
                        return Err(Fail::Rejected(code));
                    }
                    Some(Msg::Notify { .. } | Msg::Error { .. }) => {}
                    other => return Ok(other),
                }
            }
            // Stale events from a previous generation.
            Event::ReadFailed { .. } | Event::Report { .. } => {}
        }
        Ok(None)
    }

    /// Send `req` and wait for the first reply accepted by `accept`.
    fn request(
        &mut self,
        s: &mut Session,
        req: &[u8],
        accept: impl Fn(&Msg) -> bool,
    ) -> Result<Msg, Fail> {
        s.dev.send(req)?;
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(ev) => {
                    if let Some(m) = self.handle(s, ev)?.filter(&accept) {
                        return Ok(m);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "no reply from device").into());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "event channel closed").into());
                }
            }
        }
    }

    fn read_field(&mut self, s: &mut Session, field: u8) -> Result<u8, Fail> {
        let req = hidpp::read_field(s.index, hidpp::SW_READ, field);
        let index = s.index;
        let m = self.request(s, &req, |m| {
            matches!(m, Msg::Read { feature, field: f, .. } if *feature == index && *f == field)
        })?;
        let Msg::Read { value, .. } = m else { unreachable!() };
        Ok(value)
    }

    fn refresh(&mut self, s: &mut Session) -> Result<(), Fail> {
        s.dirty = false;
        s.stale = true;
        let mute = self.read_field(s, hidpp::FIELD_MUTE)?;
        let gain = self.read_field(s, hidpp::FIELD_GAIN)?;
        let lock = self.read_field(s, hidpp::FIELD_SMART_LOCK_READ)?;
        s.mic = Mic { muted: mute == 0, gain: gain.min(hidpp::GAIN_MAX), smart_lock: lock != 0 };
        self.show(Some(s.mic));
        // A notification during the reads set `dirty` again; then these values may be old.
        s.stale = s.dirty;
        Ok(())
    }

    /// Write, update the cached state optimistically (so rapid scrolls accumulate), then
    /// mark dirty so the next loop pass reads the truth back.
    fn apply(&mut self, s: &mut Session, c: Cmd) -> Result<(), Fail> {
        let (field, value) = match c {
            Cmd::ToggleMute => {
                s.mic.muted = !s.mic.muted;
                (hidpp::FIELD_MUTE, if s.mic.muted { 0 } else { 1 })
            }
            Cmd::AdjustGain(steps) => {
                let g = (i32::from(s.mic.gain) + steps * GAIN_STEP).clamp(0, i32::from(hidpp::GAIN_MAX));
                s.mic.gain = g as u8;
                (hidpp::FIELD_GAIN, s.mic.gain)
            }
            Cmd::SetSmartLock(_) if s.mic.muted => {
                // The menu disables this while muted, but a command can be queued before the
                // menu catches up. The mic would read the change back, then revert it on unmute.
                eprintln!("ignoring Smart Audio Lock change while muted");
                return Ok(());
            }
            Cmd::SetSmartLock(on) => {
                s.mic.smart_lock = on;
                let v = if on { hidpp::SMART_LOCK_WRITE_ON } else { hidpp::SMART_LOCK_WRITE_OFF };
                (hidpp::FIELD_SMART_LOCK_WRITE, v)
            }
        };
        s.dev.send(&hidpp::write_field(s.index, hidpp::SW_WRITE, field, value))?;
        s.dirty = true;
        Ok(())
    }
}
