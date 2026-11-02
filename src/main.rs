//! Logitech Yeti GX hardware gain / mute / Smart Audio Lock in the system tray.

mod device;
mod hidpp;
mod icon;
mod mic;
mod theme;
mod worker;

use std::io;
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use ksni::{Category, Orientation, Status, ToolTip};

use device::{Cmd, Device, Event};
use mic::Mic;

const USAGE: &str = "\
yeti-gx-tray: Logitech Yeti GX gain/mute/Smart Lock in the system tray

USAGE:
    yeti-gx-tray              run the tray icon
    yeti-gx-tray --fg #rrggbb run the tray icon with a fixed digit colour
    yeti-gx-tray --listen     print every raw HID++ report from the mic with its meaning
    yeti-gx-tray --help

The digit colour follows the Plasma colour scheme (Colors:Window text) unless --fg is given;
use --fg if your panel colour differs from the scheme's window colour.

Tray: left click = toggle mute, scroll = gain +/-5, menu = Mute / Smart Audio Lock / gain / Quit.";

struct YetiTray {
    mic: Option<Mic>,
    /// Panel text colour from the Plasma colour scheme.
    fg: [u8; 3],
    tx: Sender<Event>,
}

impl YetiTray {
    fn send(&self, cmd: Cmd) {
        // Only fails if the worker thread is gone, in which case the process is exiting.
        let _ = self.tx.send(Event::Cmd(cmd));
    }

    fn pixmaps(&self) -> Vec<ksni::Icon> {
        icon::SIZES
            .iter()
            .map(|&size| ksni::Icon {
                width: size as i32,
                height: size as i32,
                data: icon::render(size, self.mic.as_ref(), self.fg),
            })
            .collect()
    }
}

impl ksni::Tray for YetiTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn category(&self) -> Category {
        Category::Hardware
    }

    fn title(&self) -> String {
        mic::summary(self.mic.as_ref())
    }

    fn status(&self) -> Status {
        // Keeps a muted mic visible in the tray instead of letting Plasma tuck it away.
        if self.mic.is_some_and(|m| m.muted) {
            Status::NeedsAttention
        } else {
            Status::Active
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.pixmaps()
    }

    fn attention_icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.pixmaps()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: mic::summary(self.mic.as_ref()), ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Cmd::ToggleMute);
    }

    fn scroll(&mut self, delta: i32, orientation: Orientation) {
        if orientation == Orientation::Vertical && delta != 0 {
            self.send(Cmd::AdjustGain(delta.signum()));
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let connected = self.mic.is_some();
        let muted = self.mic.is_some_and(|m| m.muted);
        let locked = self.mic.is_some_and(|m| m.smart_lock);
        let step = |label: &str, steps: i32| -> MenuItem<Self> {
            StandardItem {
                label: label.into(),
                enabled: connected,
                activate: Box::new(move |t: &mut Self| t.send(Cmd::AdjustGain(steps))),
                ..Default::default()
            }
            .into()
        };
        vec![
            CheckmarkItem {
                label: "Mute".into(),
                enabled: connected,
                checked: muted,
                activate: Box::new(|t: &mut Self| t.send(Cmd::ToggleMute)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                // The device ignores a Smart Lock change made while muted (it reads back as
                // changed, then reverts on unmute), so don't offer it.
                label: if muted { "Smart Audio Lock (unmute to change)" } else { "Smart Audio Lock" }
                    .into(),
                enabled: connected && !muted,
                checked: locked,
                activate: Box::new(move |t: &mut Self| t.send(Cmd::SetSmartLock(!locked))),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            step("Raise gain (+5)", 1),
            step("Lower gain (-5)", -1),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// `fg_override` pins the icon's ink colour; otherwise it follows the Plasma colour scheme.
fn run_tray(fg_override: Option<[u8; 3]>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let handle = YetiTray {
        mic: None,
        fg: fg_override.unwrap_or_else(theme::foreground),
        tx: tx.clone(),
    }
    // Start even if plasmashell's tray watcher isn't up yet (autostart ordering).
    .assume_sni_available(true)
    .spawn()
    .map_err(|e| format!("cannot register tray icon: {e}"))?;

    // Repaint when the Plasma colour scheme changes. A stat every 2 s; the file is only read
    // after its mtime moves. Pointless (and skipped) when the colour is pinned.
    if fg_override.is_none() {
        let theme_handle = handle.clone();
        thread::Builder::new()
            .name("theme-watch".into())
            .spawn(move || {
                let mut seen = theme::modified();
                loop {
                    thread::sleep(Duration::from_secs(2));
                    let now = theme::modified();
                    if now != seen {
                        seen = now;
                        let fg = theme::foreground();
                        if theme_handle.update(|t| t.fg = fg).is_none() {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }

    let publish = move |mic: Option<Mic>| {
        handle.update(|t| t.mic = mic);
    };
    worker::Worker::new(rx, tx, publish).run();
    Ok(())
}

fn run_listen() -> Result<(), String> {
    let path = device::find()
        .map_err(|e| format!("scanning /sys/class/hidraw: {e}"))?
        .ok_or("no Yeti GX hidraw node found")?;
    let (tx, rx) = mpsc::channel();
    let dev = Device::open(path.clone(), tx, 1).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("listening on {} (Ctrl-C to stop)", path.display());
    // The one documented request needed to learn the feature index, for annotation only.
    dev.send(&hidpp::get_feature(hidpp::SW_READ)).map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let mut index = None;
    loop {
        match rx.recv() {
            Ok(Event::Report { data, .. }) => {
                if let Some(hidpp::Msg::FeatureIndex(i)) = hidpp::decode(&data, hidpp::SW_READ) {
                    index = Some(i);
                }
                println!(
                    "+{:9.3}s  {}  | {}",
                    t0.elapsed().as_secs_f64(),
                    hidpp::hex(&data),
                    hidpp::describe(&data, index)
                );
            }
            Ok(Event::ReadFailed { .. }) => return Err("device read failed (unplugged?)".into()),
            Ok(Event::Cmd(_)) => {}
            Err(_) => return Err(io::Error::from(io::ErrorKind::BrokenPipe).to_string()),
        }
    }
}

enum Mode {
    Tray,
    Listen,
    Help,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<(Mode, Option<[u8; 3]>), String> {
    let (mut mode, mut fg) = (None, None);
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        let mut set_mode = |m: Mode| match mode.replace(m) {
            None => Ok(()),
            Some(_) => Err("only one of --listen / --help may be given".to_string()),
        };
        match a.as_str() {
            "--listen" => set_mode(Mode::Listen)?,
            "-h" | "--help" => set_mode(Mode::Help)?,
            _ => {
                let value = if a == "--fg" {
                    args.next().ok_or("--fg needs a colour, e.g. --fg '#4c4f69'")?
                } else if let Some(v) = a.strip_prefix("--fg=") {
                    v.to_string()
                } else {
                    return Err(format!("unknown argument {a:?}\n\n{USAGE}"));
                };
                let rgb = theme::parse_hex(&value)
                    .ok_or_else(|| format!("--fg: {value:?} is not a #rrggbb colour"))?;
                fg = Some(rgb);
            }
        }
    }
    let mode = mode.unwrap_or(Mode::Tray);
    if fg.is_some() && !matches!(mode, Mode::Tray) {
        return Err("--fg only applies to the tray icon".into());
    }
    Ok((mode, fg))
}

fn main() -> ExitCode {
    let result = parse_args(std::env::args().skip(1)).and_then(|(mode, fg)| match mode {
        Mode::Tray => run_tray(fg),
        Mode::Listen => run_listen(),
        Mode::Help => {
            println!("{USAGE}");
            Ok(())
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yeti-gx-tray: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<(Mode, Option<[u8; 3]>), String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn fg_accepts_both_forms_and_hash_optional() {
        assert_eq!(parse(&["--fg", "#4c4f69"]).unwrap().1, Some([0x4c, 0x4f, 0x69]));
        assert_eq!(parse(&["--fg=4C4F69"]).unwrap().1, Some([0x4c, 0x4f, 0x69]));
        assert!(matches!(parse(&[]).unwrap(), (Mode::Tray, None)));
    }

    #[test]
    fn rejects_bad_or_misplaced_arguments() {
        assert!(parse(&["--fg"]).is_err());
        assert!(parse(&["--fg", "red"]).is_err());
        assert!(parse(&["--fg", "#12345"]).is_err());
        assert!(parse(&["--listen", "--fg", "#ffffff"]).is_err());
        assert!(parse(&["--listen", "--help"]).is_err());
        assert!(parse(&["--nope"]).is_err());
    }
}
