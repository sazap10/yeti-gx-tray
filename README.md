# yeti-gx-tray

A small Rust program that shows the Logitech Yeti GX microphone's **hardware gain**, mute
and Smart Audio Lock state as a KDE Plasma system-tray icon, without G HUB.

The mic's real preamp gain is invisible to Linux audio tools (ALSA only exposes a separate
0–12 digital trim). It is reachable over Logitech HID++ 2.0 on the mic's vendor hidraw
interface; this tool talks to that directly.

- **Icon**: the gain number, drawn to stay legible at 22 px. Every state is distinguishable
  without colour:
  - unmuted: bare digits in the panel's text colour, no tile (the quiet resting state);
  - muted: a solid red tile with the digits knocked out of it; the tray item's status is also
    set to "needs attention";
  - disconnected: two dimmed dashes.

  A bar under the digits means Smart Audio Lock is on.
- **Tooltip**: `Yeti GX — gain 100, unmuted, Smart Lock off`.
- **Left click**: toggle hardware mute. **Scroll**: gain ±5 (0–100).
- **Menu**: Mute, Smart Audio Lock, Raise gain (+5), Lower gain (-5), Quit. Everything the
  mouse gestures do is also available as a menu entry.
- Recovers automatically after unplug/replug.

No Qt, no hidapi: StatusNotifierItem over D-Bus via [`ksni`](https://crates.io/crates/ksni),
and plain `std::fs::File` on the hidraw node. About 8 MB RSS, 0 CPU when idle.

## Requirements

- Linux with a StatusNotifierItem host (Plasma's tray).
- Read/write access to the mic's `/dev/hidrawN`. On a seat-managed desktop this is already
  granted by the `uaccess` udev tag; no udev rule or sudo is needed (verified on CachyOS /
  Plasma 6.8). The node number changes between plugs; it is found at runtime by matching
  `HID_ID=0003:0000046D:00000AFC` in `/sys/class/hidraw/*/device/uevent`.
- Rust toolchain (`cargo`) to build. [mise](https://mise.jdx.dev) drives the tasks; plain
  `cargo` works too.

## Build and run

```sh
mise run check      # clippy -D warnings, tests, release build
mise run run        # build + run the tray
mise run listen     # build + print raw HID++ reports (see below)
```

Without mise: `cargo build --release && target/release/yeti-gx-tray`.

## Install

Nothing is installed automatically. To install for your user and autostart on login:

```sh
mise run install
# equivalent to:
#   install -Dm755 target/release/yeti-gx-tray ~/.local/bin/yeti-gx-tray
#   install -Dm644 yeti-gx-tray.desktop ~/.config/autostart/yeti-gx-tray.desktop
```

`~/.local/bin` must be on the session `PATH` for the autostart entry's `Exec=yeti-gx-tray`;
otherwise put the absolute path in `Exec=`. To uninstall, delete those two files.

## `--listen`

`yeti-gx-tray --listen` prints every raw report from the mic with a timestamp and decoded
meaning, and sends nothing except the one documented getFeature request needed to learn the
feature index. It can run alongside the tray (hidraw delivers every input report to every
open file descriptor).

### Notification behaviour (verified on cepheus, firmware 2.00)

Observed with `--listen` while using the tray, the knob and the hardware buttons:

| Action | Notification? |
|---|---|
| Turn the knob | **Yes**: `11 ff 0d 20 00 01 <gain>` per step, value = new gain |
| Press the hardware mute button | **Yes**: `... 20 00 00 <0\|1>` (0 = muted, 1 = unmuted) |
| Press the hardware Smart Lock button | **Yes**: `... 20 00 05 <0\|1>` |
| Software write (tray click / scroll / menu) | **No**: only the write reply `... 3d 00 <field> <value>` |

So hardware changes show up on the icon almost instantly, and software writes are picked up
by the re-read the tray does immediately after every write. The 2 s periodic re-read stays as
a safety net.

## Protocol summary

HID++ 2.0 over hidraw, feature `0x8370` (index discovered at runtime; 13 on firmware 2.00).
Short report `0x10` = 7 bytes, long `0x11` = 20 bytes.

```
byte 0  report id (0x10 / 0x11)
byte 1  device index 0xFF
byte 2  feature index
byte 3  (function << 4) | sw_id
byte 4+ params
```

| Request | Bytes |
|---|---|
| getFeature(0x8370) | `10 ff 00 0c 83 70 00` → reply byte 4 = feature index |
| read field | `10 ff <idx> 2<sw> 00 <field> 00` → reply value at byte 7 |
| write field | `10 ff <idx> 3<sw> 00 <field> <value>` |

| Field | Access | Values |
|---|---|---|
| `0x00` mute | r/w | `0` muted, `1` unmuted |
| `0x01` gain | r/w | `0`–`100` |
| `0x02` Smart Audio Lock | r | `0` off, `1` on |
| `0x05` Smart Audio Lock | w | `0x06` off, `0x07` on |

Only these requests are ever sent. Replies are matched by feature index + function + sw id
(reads use sw id `0x0C`, writes `0x0D`); everything else is ignored. Notifications are
`11 ff <idx> 20 00 <type> <value>` with type `0x00` mute, `0x01` gain, `0x05` Smart Lock.
Errors are `10 ff ff <feat> <func|sw> <code> 00`.

Protocol reverse-engineered by:

- <https://github.com/hicham-bouchikhi/yeti-gx-linux>
- <https://github.com/paul-utley/yeti-gx-linux-ui>

## Design

One worker thread owns the device. A reader thread per open device blocks in `poll(2)` on
the hidraw fd (1 s wake-up only to notice shutdown) and forwards reports over a channel; the
worker also receives tray commands on the same channel. Any notification marks the state
dirty and one burst-coalesced re-read of all three fields follows. On read error the icon
turns grey and the worker rescans `/sys/class/hidraw` every 2 s, re-discovering the feature
index on reconnect.

Tray callbacks only enqueue commands, so the D-Bus thread never blocks on the device.

The icon is rendered in software (`src/icon.rs`) at 16, 22, 24, 32, 48 and 64 px with a 3x5
bitmap font; no image or font dependency. A 2 s stat of `kdeglobals` in a separate thread
repaints the icon when the Plasma colour scheme changes.

## Limitations

- Multiple instances would show multiple icons; there is no single-instance guard.
- Scroll direction assumes Plasma reports wheel-up as a positive delta.
- The resting icon uses the `ForegroundNormal` of `[Colors:Window]` in `~/.config/kdeglobals`
  (re-read when that file changes). If the panel uses a different colour set, or the file is
  unreadable, the fallback is Breeze Dark's text colour (`#eff0f1`), which is illegible on a
  light panel.
