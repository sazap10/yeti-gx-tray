//! Tiny software renderer for the tray pixmap: the gain number in a 3x5 bitmap font.
//!
//! Three looks, each distinguishable without colour:
//! - **Unmuted** (the resting state): bare digits in the panel's text colour, no tile.
//! - **Muted**: a solid red rounded tile with the digits knocked out of it. A filled block
//!   versus bare digits is the cue; the digits stay fully legible.
//! - **Disconnected**: two dimmed dashes.
//!
//! Smart Audio Lock adds a bar under the digits. The bar's row is reserved in every state so
//! the digits never move when the lock toggles.

use crate::mic::Mic;

/// Pixmap sizes offered to the tray host; it picks the closest.
pub const SIZES: [u32; 6] = [16, 22, 24, 32, 48, 64];

/// Tile fill. Off-white text on it is 4.6:1; the tile is 3.0:1 against Breeze Dark and 4.4:1
/// against Breeze Light panels.
const RED: [u8; 3] = [0xd0, 0x32, 0x30];
/// Knockout ink on the red tile (red-tinted off-white, not pure white).
const KNOCKOUT: [u8; 3] = [0xfd, 0xf3, 0xf1];
/// Dashes for the disconnected state are the text colour at this opacity.
const DIM_ALPHA: u8 = 0xBF;

/// 3x5 glyphs, one `u8` per row, low 3 bits left-to-right = MSB..LSB.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];
const DASH: [u8; 5] = [0b000, 0b000, 0b111, 0b000, 0b000];

/// Pixel geometry for `n` glyphs on a `size` square.
struct Layout {
    x0: i32,
    y0: i32,
    /// Horizontal / vertical size of one font cell.
    sx: i32,
    sy: i32,
    /// Gap between glyphs.
    gap: i32,
    /// Gap between digits and the Smart Lock bar, and the bar's thickness.
    bar_gap: i32,
    bar: i32,
    /// Total glyph row width.
    width: i32,
}

impl Layout {
    fn new(size: u32, n: usize) -> Self {
        let size = size as i32;
        let n = n as i32;
        let margin = (size / 32).max(1);
        let gap = (size / 22).max(1);
        let bar_gap = gap;
        let bar = (size / 11).max(2);
        // Tallest cell that fits digits + gap + bar; then the widest cell that fits n glyphs,
        // never wider than tall; then re-cap the height so digits aren't more than 2:1.
        let sy0 = ((size - 2 * margin - bar_gap - bar) / 5).max(1);
        let fit = ((size - 2 * margin - (n - 1) * gap) / (3 * n)).max(1);
        let sx = fit.min(sy0);
        let sy = sy0.min(2 * sx);
        let width = n * 3 * sx + (n - 1) * gap;
        let digits = 5 * sy;
        let block = digits + bar_gap + bar;
        // Centre the digits on the canvas (the common, lock-off state then looks balanced)
        // when the lock bar still fits in the bottom margin; else centre digits+bar as one
        // block. Either way the digits never move when the lock toggles.
        let centred = (size - digits) / 2;
        let y0 = if centred + block <= size - margin { centred } else { (size - block) / 2 };
        Self {
            x0: (size - width) / 2,
            y0,
            sx,
            sy,
            gap,
            bar_gap,
            bar,
            width,
        }
    }
}

struct Canvas {
    size: u32,
    /// ARGB32, network byte order: bytes are A, R, G, B.
    data: Vec<u8>,
    /// Paint only where something is already drawn, never more opaque than it: ink drawn
    /// over the muted tile stays inside its rounded corners.
    clip: bool,
}

impl Canvas {
    fn new(size: u32) -> Self {
        Self { size, data: vec![0; (size * size * 4) as usize], clip: false }
    }

    fn put(&mut self, x: i32, y: i32, rgb: [u8; 3], alpha: u8) {
        if x < 0 || y < 0 || x >= self.size as i32 || y >= self.size as i32 {
            return;
        }
        let i = ((y as u32 * self.size + x as u32) * 4) as usize;
        let alpha = if self.clip { alpha.min(self.data[i]) } else { alpha };
        self.data[i..i + 4].copy_from_slice(&[alpha, rgb[0], rgb[1], rgb[2]]);
    }

    fn rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, rgb: [u8; 3], alpha: u8) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                self.put(x, y, rgb, alpha);
            }
        }
    }

    /// Full-canvas rounded square with 4x4-supersampled edges.
    fn tile(&mut self, rgb: [u8; 3]) {
        let s = self.size as f32;
        let r = s * 0.22;
        for y in 0..self.size {
            for x in 0..self.size {
                let mut inside = 0u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = x as f32 + (sx as f32 + 0.5) / 4.0;
                        let py = y as f32 + (sy as f32 + 0.5) / 4.0;
                        let (cx, cy) = (px.clamp(r, s - r), py.clamp(r, s - r));
                        if (px - cx).powi(2) + (py - cy).powi(2) <= r * r {
                            inside += 1;
                        }
                    }
                }
                if inside > 0 {
                    self.put(x as i32, y as i32, rgb, (inside * 255 / 16) as u8);
                }
            }
        }
    }

    fn glyphs(&mut self, lay: &Layout, glyphs: &[[u8; 5]], rgb: [u8; 3], alpha: u8) {
        for (gi, g) in glyphs.iter().enumerate() {
            let gx = lay.x0 + gi as i32 * (3 * lay.sx + lay.gap);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..3 {
                    if bits >> (2 - col) & 1 == 1 {
                        let (x, y) = (gx + col * lay.sx, lay.y0 + row as i32 * lay.sy);
                        self.rect(x, y, lay.sx, lay.sy, rgb, alpha);
                    }
                }
            }
        }
    }

    fn lock_bar(&mut self, lay: &Layout, rgb: [u8; 3]) {
        let y = lay.y0 + 5 * lay.sy + lay.bar_gap;
        self.rect(lay.x0, y, lay.width, lay.bar, rgb, 0xff);
    }
}

/// Render the icon as ARGB32 (network byte order) bytes, `size * size * 4` long.
/// `fg` is the panel's text colour, used for the resting and disconnected looks.
pub fn render(size: u32, mic: Option<&Mic>, fg: [u8; 3]) -> Vec<u8> {
    let mut c = Canvas::new(size);
    match mic {
        None => {
            let lay = Layout::new(size, 2);
            c.glyphs(&lay, &[DASH, DASH], fg, DIM_ALPHA);
        }
        Some(m) => {
            let glyphs: Vec<[u8; 5]> = m
                .gain
                .min(100)
                .to_string()
                .bytes()
                .map(|b| DIGITS[(b - b'0') as usize])
                .collect();
            let lay = Layout::new(size, glyphs.len());
            let ink = if m.muted {
                c.tile(RED);
                c.clip = true;
                KNOCKOUT
            } else {
                fg
            };
            c.glyphs(&lay, &glyphs, ink, 0xff);
            if m.smart_lock {
                c.lock_bar(&lay, ink);
            }
        }
    }
    c.data
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: [u8; 3] = [0x4c, 0x4f, 0x69];
    const ON: Mic = Mic { muted: false, gain: 100, smart_lock: false };

    fn alpha(data: &[u8], size: u32, x: u32, y: u32) -> u8 {
        data[((y * size + x) * 4) as usize]
    }

    fn rgb(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * size + x) * 4) as usize;
        [data[i + 1], data[i + 2], data[i + 3]]
    }

    fn opaque_px(data: &[u8], size: u32) -> usize {
        (0..size * size).filter(|i| alpha(data, size, i % size, i / size) == 0xff).count()
    }

    #[test]
    fn buffer_is_argb_sized_for_every_size() {
        for s in SIZES {
            assert_eq!(render(s, Some(&ON), FG).len(), (s * s * 4) as usize);
            assert_eq!(render(s, None, FG).len(), (s * s * 4) as usize);
        }
    }

    #[test]
    fn digit_strokes_are_at_least_two_pixels_from_22px_up() {
        // The audit finding: "100" was drawn with 1 px strokes at 22 px. Find the shortest
        // horizontal run of ink in the middle row of the glyphs.
        for s in [22u32, 24, 32, 48, 64] {
            let d = render(s, Some(&ON), FG);
            let lay = Layout::new(s, 3);
            assert!(lay.sx >= 2, "size {s}: sx={}", lay.sx);
            let y = (lay.y0 + lay.sy * 2) as u32; // row 2 of the 3x5 font: full bars
            let mut min_run = u32::MAX;
            let mut run = 0;
            for x in 0..s {
                if alpha(&d, s, x, y) == 0xff {
                    run += 1;
                } else if run > 0 {
                    min_run = min_run.min(run);
                    run = 0;
                }
            }
            assert!(min_run >= 2, "size {s}: shortest stroke run {min_run}px");
        }
    }

    #[test]
    fn content_stays_inside_the_canvas_for_every_width() {
        for s in SIZES {
            for gain in [0u8, 7, 8, 10, 55, 99, 100] {
                let m = Mic { gain, smart_lock: true, ..ON };
                let d = render(s, Some(&m), FG);
                // A bare-digit icon keeps the outer 1 px ring empty.
                for i in 0..s {
                    for (x, y) in [(i, 0), (i, s - 1), (0, i), (s - 1, i)] {
                        assert_eq!(alpha(&d, s, x, y), 0, "size {s} gain {gain} at ({x},{y})");
                    }
                }
            }
        }
    }

    #[test]
    fn resting_icon_is_bare_digits_in_the_text_colour() {
        let d = render(22, Some(&ON), FG);
        assert_eq!(alpha(&d, 22, 0, 0), 0);
        assert_eq!(alpha(&d, 22, 11, 1), 0, "no tile behind the digits");
        let lay = Layout::new(22, 3);
        // Bottom row of the first glyph ("1") is a full 3-cell bar.
        let (x, y) = (lay.x0 as u32, (lay.y0 + 4 * lay.sy) as u32);
        assert_eq!(alpha(&d, 22, x, y), 0xff);
        assert_eq!(rgb(&d, 22, x, y), FG);
    }

    #[test]
    fn muted_is_a_solid_block_not_just_a_colour_change() {
        for s in SIZES {
            let muted = render(s, Some(&Mic { muted: true, ..ON }), FG);
            let rest = render(s, Some(&ON), FG);
            // Mid-edge pixels of the tile are opaque red; at rest they are empty.
            let mid = s / 2;
            assert_eq!(alpha(&muted, s, mid, 0), 0xff);
            assert_eq!(rgb(&muted, s, mid, 0), RED);
            assert_eq!(alpha(&rest, s, mid, 0), 0);
            // Edge midpoints on all four sides: a block, not just bare ink.
            for (x, y) in [(mid, s - 1), (0, mid), (s - 1, mid)] {
                assert_eq!(alpha(&muted, s, x, y), 0xff, "size {s} at ({x},{y})");
                assert_eq!(alpha(&rest, s, x, y), 0, "size {s} at ({x},{y})");
            }
        }
    }

    #[test]
    fn muted_digits_are_knocked_out_and_not_struck_through() {
        let s = 22;
        let muted = render(s, Some(&Mic { muted: true, ..ON }), FG);
        let rest = render(s, Some(&ON), FG);
        // Every digit pixel at rest is knockout ink when muted; nothing else draws over them.
        for y in 0..s {
            for x in 0..s {
                if alpha(&rest, s, x, y) == 0xff {
                    assert_eq!(rgb(&muted, s, x, y), KNOCKOUT, "({x},{y})");
                }
            }
        }
        // And the knockout ink pixels are exactly the digit pixels.
        let ink = |d: &[u8]| {
            (0..s * s).filter(|i| rgb(d, s, i % s, i / s) == KNOCKOUT && alpha(d, s, i % s, i / s) == 0xff).count()
        };
        assert_eq!(ink(&muted), opaque_px(&rest, s));
    }

    #[test]
    fn muted_ink_never_leaves_the_rounded_tile() {
        // Knockout digits and the lock bar recolour the tile; they must not square off its
        // corners or leave specks outside it (48 px is 22 px on a 2x display).
        for s in SIZES {
            let mut tile = Canvas::new(s);
            tile.tile(RED);
            for gain in 0..=100u8 {
                for smart_lock in [false, true] {
                    let d = render(s, Some(&Mic { muted: true, gain, smart_lock }), FG);
                    for i in 0..s * s {
                        let (x, y) = (i % s, i / s);
                        assert_eq!(
                            alpha(&d, s, x, y),
                            alpha(&tile.data, s, x, y),
                            "size {s} gain {gain} lock {smart_lock} at ({x},{y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn smart_lock_adds_a_bar_and_never_moves_the_digits() {
        for s in SIZES {
            let off = render(s, Some(&ON), FG);
            let on = render(s, Some(&Mic { smart_lock: true, ..ON }), FG);
            let lay = Layout::new(s, 3);
            let bar_y = (lay.y0 + 5 * lay.sy + lay.bar_gap) as u32;
            let mid = s / 2;
            assert_eq!(alpha(&off, s, mid, bar_y), 0, "size {s}");
            assert_eq!(alpha(&on, s, mid, bar_y), 0xff, "size {s}");
            // Everything above the bar is identical.
            let cut = (bar_y * s * 4) as usize;
            assert_eq!(off[..cut], on[..cut], "size {s}");
        }
    }

    #[test]
    fn disconnected_is_two_dimmed_dashes_with_no_tile() {
        let d = render(22, None, FG);
        assert_eq!(alpha(&d, 22, 0, 0), 0);
        assert_eq!(alpha(&d, 22, 11, 0), 0);
        let lay = Layout::new(22, 2);
        let y = (lay.y0 + 2 * lay.sy) as u32;
        assert_eq!(alpha(&d, 22, lay.x0 as u32, y), DIM_ALPHA);
        assert_eq!(rgb(&d, 22, lay.x0 as u32, y), FG);
    }

    #[test]
    fn every_gain_and_state_renders_differently() {
        let g = |gain, muted| render(22, Some(&Mic { gain, muted, smart_lock: false }), FG);
        let mut seen = std::collections::HashSet::new();
        for gain in 0..=100u8 {
            assert!(seen.insert(g(gain, false)), "gain {gain} duplicates another");
        }
        assert_ne!(g(50, false), g(50, true));
        assert_ne!(g(50, true), render(22, None, FG));
    }

    #[test]
    fn out_of_range_gain_does_not_panic() {
        render(22, Some(&Mic { gain: 255, ..ON }), FG);
    }

    #[test]
    fn digits_are_vertically_centred_wherever_the_bar_still_fits() {
        for s in [22u32, 24, 32] {
            let lay = Layout::new(s, 3);
            let (top, bottom) = (lay.y0, s as i32 - (lay.y0 + 5 * lay.sy));
            assert!((top - bottom).abs() <= 1, "size {s}: {top} above, {bottom} below");
        }
    }

    #[test]
    fn dashes_hold_3_to_1_contrast_on_light_and_dark_panels() {
        fn lum(c: [f64; 3]) -> f64 {
            let l = |v: f64| {
                let v = v / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
        }
        let a = f64::from(DIM_ALPHA) / 255.0;
        // (text colour, panel colour): Catppuccin Latte as on the author's panel, Breeze Dark.
        for (fg, bg) in [([76.0, 79.0, 105.0], [230.0, 233.0, 239.0]), ([239.0, 240.0, 241.0], [35.0, 38.0, 41.0])] {
            let ink = [0, 1, 2].map(|i| fg[i] * a + bg[i] * (1.0 - a));
            let (hi, lo) = (lum(ink).max(lum(bg)), lum(ink).min(lum(bg)));
            assert!((hi + 0.05) / (lo + 0.05) >= 3.0, "fg {fg:?} on {bg:?}");
        }
    }
}
