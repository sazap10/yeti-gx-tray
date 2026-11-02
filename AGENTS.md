# AGENTS.md

KDE Plasma tray icon (StatusNotifierItem via `ksni`) for the Logitech Yeti GX mic's hardware
gain, mute and Smart Audio Lock, spoken over HID++ 2.0 on its hidraw node. Single binary crate,
Rust edition 2024, no Qt, no hidapi.

## Commands

```sh
mise run check   # clippy -D warnings + tests + release build: run before every commit
mise run run     # build and run the tray
mise run listen  # --listen: print raw HID++ reports with decoded meaning
```

Plain `cargo` works too: `cargo clippy --all-targets -- -D warnings`, `cargo test`,
`cargo build --release`.

## Layout

| File | Role |
|---|---|
| `src/hidpp.rs` | Pure encode/decode of HID++ frames; no I/O. Most protocol tests live here. |
| `src/device.rs` | Finds the hidraw node in sysfs, opens it, runs the `poll(2)` reader thread. |
| `src/worker.rs` | The one thread that owns the device: request/reply, notifications, refresh, commands, hotplug. |
| `src/main.rs` | CLI parsing, the `ksni::Tray` impl (click, scroll, menu), `--listen`, theme watcher thread. |
| `src/icon.rs` | Software-rendered ARGB32 pixmaps at 16/22/24/32/48/64 px with a 3x5 bitmap font. |
| `src/theme.rs` | Digit colour from `[Colors:Window] ForegroundNormal` in `kdeglobals`; `--fg` parsing. |
| `src/mic.rs` | `Mic` state and the tooltip summary. |

Docs: `PLAN.md` (original spec and acceptance), `README.md` (users), `PRODUCT.md` (principles),
`DESIGN.md` + `.impeccable/design.json` (icon visual system).

## Hard rules

- **Only send the requests documented in `PLAN.md`**: root getFeature(0x8370), read fields
  0x00/0x01/0x02, write fields 0x00/0x01/0x05. Never probe other features or fields; this is
  undocumented hardware.
- **Never touch the real device to test a change** (no writes to `/dev/hidraw*`, no tray clicks
  or scrolls on a running instance) without asking: writes change the user's live mic gain and
  mute. Read-only runs (`--listen`, launching the tray and watching stderr) are fine. Knob
  turns, hardware buttons and replugging need the user; ask.
- **Install nothing.** No writes to `~/.cargo/bin`, `~/.local/bin`, `~/.config/autostart`, no
  udev rules, no sudo. `mise run install` exists for the user to run.
- Conventional Commits (`feat:`, `fix(worker):`, …), one logical change per commit.

## Protocol invariants

- Feature index is discovered at runtime on every connect (13 on firmware 2.00); never hardcode.
- Reads use sw id `0x0C`, writes `0x0D`, so a late write ack or error can never answer a read.
  Replies are matched by feature index + field + sw id; error frames count only when their
  feature byte is ours (`s.index`, which is 0 during discovery).
- hidraw delivers every input report to every open fd: other clients (upstream `yeti-ctl`
  also uses sw id 12, `--listen`, a second tray) see our replies and we see theirs. Foreign
  frames must be ignored, never fatal.
- Mute field: `0` = muted, `1` = unmuted. Smart Lock reads field 0x02 as 0/1 but writes field
  0x05 as `0x06` off / `0x07` on. The mic reverts a Smart Lock change made while muted, so the
  menu disables it then and the worker drops a queued one.
- Software writes produce no notification; the worker re-reads after every write and every
  2 s. Hardware knob/button changes do notify (`11 ff <idx> 20 00 <type> <value>`).

## Worker invariants

- Only `Fail::Gone` ends a session; `Fail::Rejected` fails the request in flight and is logged.
- Tray callbacks only enqueue `Cmd`s; the D-Bus thread never blocks on the device.
- Commands are applied to the cached `Mic` only after a refresh that completed with no
  notification during it (`stale == false`); otherwise they stay queued. Never re-read in a
  tight loop on rejected reads: the periodic 2 s refresh is the retry.
- Every connect bumps `epoch`; events from an older reader are dropped.
- No busy loops: idle CPU must stay ~0 (reader wakes 1/s, theme watcher stats every 2 s).

## Icon rules (see `DESIGN.md`)

- Every state distinguishable without colour: bare digits (rest), solid red tile with
  knocked-out digits (muted), two dimmed dashes (disconnected), bar under the digits (Smart Lock).
- Mute Red `#d03230` is used for muted only. Resting ink is the panel text colour, never
  hard-coded.
- From 22 px up no stroke is thinner than 2 px; digit height is the same for 1–3 digits; the
  Smart Lock bar row is always reserved so digits never move.
- Ink drawn over the muted tile must stay inside its rounded corners.
- Pixmap bytes are ARGB32 in network byte order (A, R, G, B).

## Docs to keep in sync

User-visible behaviour (gestures, menu, CLI flags, install steps, limitations) is described in
`README.md` and the `USAGE` string in `src/main.rs`; icon geometry and colours in `DESIGN.md`
and `.impeccable/design.json`. Update them in the same commit as the code.
