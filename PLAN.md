# yeti-gx-tray — plan

A small Rust program that puts the Logitech Yeti GX microphone's **hardware gain** (and mute /
Smart Audio Lock state) in the KDE Plasma system tray, without G HUB.

## Why

The Yeti GX's real preamp gain is set by its physical knob (or G HUB, which has no Linux build).
That gain is invisible to every Linux audio tool: ALSA only exposes a separate 0–12 digital trim
(`Mic Capture Volume`, −12..0 dB). On 2026-11-02 low mic levels on `cepheus` turned out to be the
knob, so I want the value visible at a glance.

## Target machine (verified 2026-11-02 on cepheus)

- CachyOS, Plasma 6.8.2 (Wayland), PipeWire 1.6.10.
- Toolchain present: `cargo`/`rustc` (pacman), no hidapi needed.
- Mic: USB `046d:0afc`, firmware 2.00 (per upstream notes). Interfaces: audio control,
  audio streaming, and one vendor HID interface (usage page `0xFF43`) carrying **HID++ 2.0**.
- Its hidraw node was `/dev/hidraw6`, tagged `seat` + `uaccess` by an existing udev rule, so the
  logged-in user already has rw access. **No udev rule or sudo is needed.** The node number is not
  stable; find it at runtime.
- Live readback that worked (read-only requests): feature `0x8370` at index **13**,
  mute=1 (unmuted), gain=**100**, Smart Audio Lock=0.

## Protocol (HID++ 2.0 over hidraw)

Sources: <https://github.com/hicham-bouchikhi/yeti-gx-linux> (CLI, verified against fw 2.00)
and <https://github.com/paul-utley/yeti-gx-linux-ui> (GUI fork, documents notifications).

Reports: `0x10` short = 7 bytes, `0x11` long = 20 bytes.

```
byte 0  report id     0x10 / 0x11
byte 1  device index  0xFF
byte 2  feature index discovered at runtime (13 observed)
byte 3  (function << 4) | sw_id   — sw_id is ours to choose, 1..15; device echoes it
byte 4+ params
```

**Find the device:** scan `/sys/class/hidraw/hidraw*/device/uevent` for
`HID_ID=0003:0000046D:00000AFC`; open `/dev/<name>` read/write with plain `std::fs::File`.

**Discover the feature index** (do not hardcode 13): root feature getFeature(0x8370)

```
send 10 ff 00 0c 83 70 00   ->  reply byte 4 = index of 0x8370
```

**Read a field** (function 2): `10 ff <idx> 2<sw> 00 <field> 00` → long reply
`11 ff <idx> 2<sw> 00 <field> <type> <value> …`, value at **byte 7**.

**Write a field** (function 3): `10 ff <idx> 3<sw> 00 <field> <value>` (short write is enough;
the long-report "token" G HUB sends is not validated).

| Field | Access | Values |
|---|---|---|
| `0x00` mute | read/write | `0` muted, `1` unmuted |
| `0x01` gain | read/write | `0`–`100` |
| `0x02` Smart Audio Lock | read | `0` off, `1` on |
| `0x05` Smart Audio Lock | write | `0x06` off, `0x07` on |

**Errors:** `10 ff ff <feat> <func> <code> 00`, HID++ 2.0 codes: 0x01 unknown, 0x02 invalid
argument, 0x03 out of range, 0x04 hardware error, 0x05 Logitech internal, 0x06 invalid feature
index, 0x07 invalid function, 0x08 busy, 0x09 unsupported. (The upstream README's list, copied
here originally, mislabelled 0x04–0x07.)

**Device-initiated notifications:** `11 ff <idx> 20 00 <type> <value>` (sw_id 0).
`type 0x00` mute, `0x01` gain (knob turned), `0x05` Smart Audio Lock.
The two upstream READMEs disagree: one documents only the mute notification and says software
writes produce none; the other documents all three and says writes do notify. **Unverified on
cepheus.** Design so it works either way (see below).

hidraw delivers every input report to every open file descriptor, so other programs holding the
node do not steal replies — but you must match replies by feature index + function + your sw_id
and ignore everything else.

## What to build

A single binary crate in this folder, `yeti-gx-tray`.

1. **Device layer** — pure functions to encode requests and decode replies/notifications/errors
   (unit-tested), plus a small `Device` that finds, opens, discovers the feature index, reads
   `{mute, gain, smartlock}`, and writes the three writable fields. Only ever send the requests in
   the table above; never probe other features or fields.
2. **Tray** — StatusNotifierItem via the `ksni` crate (check its current API; it has a blocking
   facade and an async one). No Qt.
   - Icon: the gain number drawn onto the icon pixmap (Plasma's tray shows icons only, no text
     label). Distinct look when muted (e.g. red / muted mic) and when the mic is disconnected.
     Keep dependencies light — a tiny embedded bitmap digit font is fine.
   - Tooltip/title: `Yeti GX — gain 100, unmuted, Smart Lock off`.
   - Left click: toggle mute. Scroll: gain ±5 (clamped 0–100).
   - Menu: Smart Audio Lock checkbox, Quit.
3. **State updates** — one reader thread doing blocking reads on the hidraw node, applying
   notifications as they arrive, plus a re-read of all three fields every ~2 s and right after any
   write. That covers both readings of the notification behaviour.
4. **Hotplug** — on read/write error (e.g. `ENODEV`) show the disconnected icon and rescan for the
   device every ~2 s; resume automatically on replug (feature index may change — rediscover).
5. **`--listen` flag** — print every raw report from the device with a timestamp and decoded
   meaning, for debugging. Use it to settle the notification question: ask the user to turn the
   knob, press mute, and toggle Smart Lock while it runs, then record the result in the README.
6. **Packaging** — `README.md` (what it is, build, run, install, protocol summary with links to
   the two upstream repos) and a `yeti-gx-tray.desktop` autostart file in the repo.

## Acceptance

- [ ] `cargo build --release` and `cargo clippy -- -D warnings` clean; `cargo test` passes
      (encode/decode tests for reads, writes, notifications, error replies, wrong-sw_id filtering).
- [ ] Running `target/release/yeti-gx-tray` shows a tray icon in Plasma with the current gain.
- [ ] Turning the knob updates the icon within ~2 s (instantly if notifications fire).
- [ ] Click toggles hardware mute (mic LED goes red/white); scroll changes gain; menu toggles
      Smart Audio Lock; all three reflected on the icon after readback.
- [ ] Unplug shows the disconnected state; replug recovers without restarting the program.
- [ ] `--listen` result for knob/mute/Smart Lock notifications recorded in the README.
- [ ] Idle CPU ~0 (no busy loop); RSS a few MB.

## Constraints

- Git-init this folder; Conventional Commits (`feat: …`, `fix: …`).
- Do not install anything into `$HOME` (`~/.cargo/bin`, `~/.config/autostart`) and do not add
  udev rules or use sudo — document the install commands in the README; the user installs.
- Do not touch `~/dev/personal-knowledge` or `~/dev/homelab`.
- Physical interaction (turning the knob, pressing mute, replugging) needs the user — ask them.
