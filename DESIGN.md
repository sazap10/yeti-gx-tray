---
name: Yeti GX Tray
description: A glanceable gain readout that stays quiet in the tray and turns red only when the mic is muted.
colors:
  mute-red: "#d03230"
  knockout: "#fdf3f1"
  ink: "#4c4f69"
  ink-fallback: "#eff0f1"
typography:
  gain-digits:
    fontFamily: "3x5 bitmap (DIGITS in src/icon.rs)"
    fontSize: "15px"
    lineHeight: 1
    letterSpacing: "1px"
rounded:
  tile: "22%"
spacing:
  margin: "1px"
  glyph-gap: "1px"
  bar-gap: "1px"
  lock-bar: "2px"
components:
  icon-resting:
    textColor: "{colors.ink}"
    typography: "{typography.gain-digits}"
    size: "22px"
  icon-resting-locked:
    textColor: "{colors.ink}"
    typography: "{typography.gain-digits}"
    height: "{spacing.lock-bar}"
    size: "22px"
  icon-muted:
    backgroundColor: "{colors.mute-red}"
    textColor: "{colors.knockout}"
    typography: "{typography.gain-digits}"
    rounded: "{rounded.tile}"
    size: "22px"
  icon-disconnected:
    textColor: "{colors.ink}"
    typography: "{typography.gain-digits}"
    size: "22px"
---

# Design System: Yeti GX Tray

## 1. Overview

**Creative North Star: "The Pilot Light"**

A pilot light is unremarkable until something is wrong. This icon is the same: at rest it is a few bare digits in the panel's own text colour, indistinguishable in spirit from the rest of the tray. The one loud thing in the system, a solid red tile, exists for the one state that needs attention: the mic is muted. Gain and mute carry equal weight, so the digits stay legible in every state and the red never covers them.

The system is glanceable, exact and native. It is read at 22 px beside other tray icons, so every pixel is deliberate: hard pixel edges, a 3x5 bitmap font scaled in whole pixels, and colour pulled from the user's Plasma colour scheme rather than invented. It explicitly rejects **loud, fully saturated colour on inactive/resting states**, and it never lets colour carry a state alone.

**Key Characteristics:**
- Bare digits at rest; a filled block only when muted.
- Ink follows the Plasma colour scheme; the red and its knockout are the only fixed colours.
- Every state is distinguishable without colour: bare digits, solid block, dimmed dashes, bar under the digits.
- Flat, hard-edged, whole-pixel geometry. Anti-aliasing exists only on the tile's rounded corners.

## 2. Colors

A single signal red on a field of theme-matched neutrals; the palette is restrained to the point of being nearly absent.

### Primary
- **Mute Red** (#d03230): the solid tile behind the digits when the mic is muted, and nothing else. Chosen so the knocked-out digits reach 4.6:1 and the tile itself holds 4.1:1 on Catppuccin Latte, 4.4:1 on Breeze Light and 3.0:1 on Breeze Dark.

### Neutral
- **Panel Ink** (#4c4f69, runtime): the digits, the Smart Lock bar and the disconnected dashes. This value is the author's Catppuccin Latte `ForegroundNormal`; at runtime it is read from `[Colors:Window]` in `~/.config/kdeglobals` and re-read within about 2 s of that file changing, or pinned with `--fg`.
- **Fallback Ink** (#eff0f1): Breeze Dark's text colour, used only when the scheme cannot be read. Illegible on a light panel; that is why `--fg` exists.
- **Knockout Rose-White** (#fdf3f1): the digits and lock bar cut out of the red tile. Red-tinted rather than pure white, so it sits inside the tile instead of glowing on it.

### Named Rules
**The Pilot Light Rule.** Mute Red appears only when the mic is muted. It is never used for gain, Smart Lock, errors or decoration. Its rarity is the signal.

**The Panel-Matched Ink Rule.** Resting ink is never hard-coded; it is the panel's own text colour. If the icon looks foreign next to its neighbours, the ink is wrong.

## 3. Typography

**Digit Font:** a 3x5 pixel bitmap for `0` to `9` and a dash, drawn in whole-pixel cells (no font file, no antialiasing)

**Character:** Blunt and mechanical, like a segment display that has been tidied. Legibility at 16 to 24 px is the only goal; there is no display and body pairing.

### Hierarchy
- **Gain digits** (strokes of one cell, cap height 5 cells): the gain number, 0 to 100. One to three digits, always the same height. Cell size by pixmap size (width x height in pixels): 16 px 1x2, 22 px 2x3, 24 px 2x3, 32 px 3x5, 48 px 4x8, 64 px 6x10. One-digit and two-digit gains use wider cells where they fit, never wider than tall, so "8" is not a thinner "100".
- **Dash** (same cells): two dashes for "disconnected", at 75% opacity.

### Named Rules
**The Two-Pixel Rule.** From 22 px up, no stroke is thinner than 2 px. At 16 px three digits cannot fit wider than 1 px strokes; that is the accepted limit.

**The Equal Height Rule.** Digit height does not depend on how many digits there are. "5", "55" and "100" share a baseline and a top.

## 4. Elevation

Flat. There are no shadows, blurs or gradients. Importance is conveyed by a solid block versus bare ink, not by depth. The tile is a single opaque layer; its only soft edge is the four anti-aliased rounded corners (4x4 supersampled).

### Named Rules
**The Solid-Block Rule.** Importance is a filled block, never a shadow, glow or outline. If a new state needs more emphasis than bare digits, it earns a tile; it does not earn an effect.

## 5. Components

The feel is **crisp and native**: it should read like a Breeze symbolic icon that happens to show a number, not like an app badge. All states share one geometry.

Geometry (all sizes): a 1 px outer margin (2 px at 64 px), glyph gap of 1 px (2 px from 48 px), digits centred horizontally. Digits are also centred vertically wherever the Smart Lock bar still fits below them (22, 24 and 32 px); elsewhere (16, 48, 64 px) digits and bar are centred together. The bar's row is reserved in every state, so the digits never move when Smart Audio Lock toggles. Pixmaps are offered at 16, 22, 24, 32, 48 and 64 px; the host picks the closest.

### Resting icon (unmuted)
- **Shape:** no tile; bare digits on a transparent background.
- **Colour:** Panel Ink at full opacity. Contrast 6.6:1 on the author's panel.
- **Smart Audio Lock on:** a bar under the digits, as wide as the digits, 2 px thick at 22 px, same ink, 1 px below the glyphs.

### Muted icon
- **Shape:** a full-canvas rounded square (corner radius 22% of the size, 4.84 px at 22 px), solid Mute Red.
- **Digits:** knocked out in Knockout Rose-White at full height, never struck through or overlapped by a mute mark. The solid block versus bare digits is the non-colour cue.
- **Smart Audio Lock on:** the same bar, in the knockout ink.
- **Tray status:** also reported to Plasma as "needs attention".

### Disconnected icon
- **Shape:** two dashes at the digits' middle row, no tile.
- **Colour:** Panel Ink at 75% opacity, which holds 3:1 on both light and dark panels. The dashes, not the dimming, are the cue.

### Tray menu (Plasma-native)
- **Entries:** Mute (checkable), Smart Audio Lock (checkable), Raise gain (+5), Lower gain (-5), Quit. Native Plasma menu styling; no custom drawing.
- **Smart Audio Lock while muted:** greyed out and labelled "Smart Audio Lock (unmute to change)", because the mic reverts a lock change made while muted.

## 6. Do's and Don'ts

### Do:
- **Do** draw the resting state as bare digits in the panel's text colour (Panel Ink); keep the tile for muted only.
- **Do** convey every state by form as well as colour: bare digits, solid block, dimmed dashes, bar under the digits. Gain and mute carry equal weight.
- **Do** keep strokes at least 2 px from 22 px up and digit height constant for one to three digits.
- **Do** keep the Smart Lock bar row reserved so toggling the lock never moves the digits.
- **Do** verify any new fixed colour at 3:1 against Breeze Light, Breeze Dark and Catppuccin Latte before shipping, and 4.5:1 for text-like ink on a tile.
- **Do** pin the colour with `--fg #rrggbb` when the panel differs from the scheme's window colour.

### Don't:
- **Don't** use loud, fully saturated colour on inactive/resting states. The unmuted icon has no tile and no accent.
- **Don't** use Mute Red for anything but muted.
- **Don't** strike through or overlay the digits to show a state; the gain must stay legible when muted.
- **Don't** rely on colour alone to convey a state.
- **Don't** add shadows, glows, outlines, gradients or animation to the icon.
- **Don't** hard-code the ink colour or assume a dark or light panel.
- **Don't** draw strokes thinner than 2 px at 22 px or above.
