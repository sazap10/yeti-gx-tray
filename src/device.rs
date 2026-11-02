//! hidraw node discovery, open, and a blocking reader thread.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;

/// Everything the worker thread waits on, so one blocking `recv` covers all sources.
pub enum Event {
    /// A raw input report from the device opened as generation `epoch`.
    Report { epoch: u64, data: Vec<u8> },
    /// The reader for generation `epoch` hit EOF or an error (unplug).
    ReadFailed { epoch: u64 },
    /// A user action from the tray.
    Cmd(Cmd),
}

pub enum Cmd {
    ToggleMute,
    /// Gain steps of 5, positive = up.
    AdjustGain(i32),
    SetSmartLock(bool),
}

const HID_ID: &str = "HID_ID=0003:0000046D:00000AFC";

/// True if a hidraw `uevent` file describes the Yeti GX (USB 046d:0afc).
pub fn uevent_matches(uevent: &str) -> bool {
    uevent.lines().any(|l| l.trim().eq_ignore_ascii_case(HID_ID))
}

/// Scan `/sys/class/hidraw` for the mic. The node number changes between plugs.
pub fn find() -> io::Result<Option<PathBuf>> {
    let mut names: Vec<String> = fs::read_dir("/sys/class/hidraw")?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let Ok(uevent) = fs::read_to_string(format!("/sys/class/hidraw/{name}/device/uevent"))
        else {
            continue;
        };
        if uevent_matches(&uevent) {
            return Ok(Some(PathBuf::from("/dev").join(name)));
        }
    }
    Ok(None)
}

pub struct Device {
    file: Arc<File>,
    stop: Arc<AtomicBool>,
    pub path: PathBuf,
}

impl Device {
    /// Open read/write and start a reader thread that forwards every input report to `tx`
    /// tagged with `epoch`. The thread exits on error, on `Device` drop, or if `tx` is closed.
    pub fn open(path: PathBuf, tx: Sender<Event>, epoch: u64) -> io::Result<Self> {
        let file = Arc::new(OpenOptions::new().read(true).write(true).open(&path)?);
        let stop = Arc::new(AtomicBool::new(false));
        let (f, s) = (file.clone(), stop.clone());
        thread::Builder::new().name("hidraw-reader".into()).spawn(move || reader(&f, &s, &tx, epoch))?;
        Ok(Self { file, stop, path })
    }

    pub fn send(&self, report: &[u8]) -> io::Result<()> {
        (&*self.file).write_all(report)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Wake-up interval for noticing `stop`; a poll timeout, not a busy loop.
const POLL_MS: i32 = 1000;

fn reader(file: &File, stop: &AtomicBool, tx: &Sender<Event>, epoch: u64) {
    let fd = file.as_raw_fd();
    let mut buf = [0u8; 64];
    let failed = |tx: &Sender<Event>| {
        let _ = tx.send(Event::ReadFailed { epoch });
    };
    while !stop.load(Ordering::Relaxed) {
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: `p` is a valid pollfd for the duration of the call; nfds is 1.
        let rc = unsafe { libc::poll(&mut p, 1, POLL_MS) };
        if rc < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return failed(tx);
        }
        if rc == 0 {
            continue;
        }
        if p.revents & libc::POLLIN != 0 {
            match (&*file).read(&mut buf) {
                Ok(0) => return failed(tx),
                Ok(n) => {
                    if tx.send(Event::Report { epoch, data: buf[..n].to_vec() }).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return failed(tx),
            }
        } else if p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return failed(tx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_yeti_gx_uevent() {
        let u = "DRIVER=hid-generic\nHID_ID=0003:0000046D:00000AFC\nHID_NAME=Logitech Yeti GX\n";
        assert!(uevent_matches(u));
        assert!(uevent_matches("HID_ID=0003:0000046d:00000afc"));
    }

    #[test]
    fn rejects_other_devices() {
        assert!(!uevent_matches("HID_ID=0003:0000046D:0000C52B\n"));
        assert!(!uevent_matches("HID_ID=0005:0000046D:00000AFC\n"));
        assert!(!uevent_matches("HID_NAME=HID_ID=0003:0000046D:00000AFC\n"));
        assert!(!uevent_matches(""));
    }
}
