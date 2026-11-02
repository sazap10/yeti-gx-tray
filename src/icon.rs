//! Tiny software renderer: a rounded square with the gain number in a 3x5 bitmap font.
//!
//! Looks: teal = unmuted, red with a white slash = muted, grey with "--" = disconnected.
//! A yellow corner dot marks Smart Audio Lock being on.

use crate::mic::Mic;

/// Pixmap sizes offered to the tray host; it picks the closest.
pub const SIZES: [u32; 4] = [22, 32, 48, 64];

const TEAL: [u8; 3] = [0x00, 0x7d, 0x7a];
const RED: [u8; 3] = [0xc6, 0x28, 0x28];
const GREY: [u8; 3] = [0x5f, 0x63, 0x68];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
const AMBER: [u8; 3] = [0xff, 0xc1, 0x07];

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

struct Canvas {
    size: u32,
    /// ARGB32, network byte order: bytes are A, R, G, B.
    data: Vec<u8>,
}

impl Canvas {
    fn new(size: u32) -> Self {
        Self { size, data: vec![0; (size * size * 4) as usize] }
    }

    fn put(&mut self, x: i32, y: i32, rgb: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.size as i32 || y >= self.size as i32 {
            return;
        }
        let i = ((y as u32 * self.size + x as u32) * 4) as usize;
        self.data[i..i + 4].copy_from_slice(&[0xff, rgb[0], rgb[1], rgb[2]]);
    }

    fn rounded_square(&mut self, rgb: [u8; 3]) {
        let n = self.size as i32;
        let r = (n / 5).max(2);
        for y in 0..n {
            for x in 0..n {
                // Distance from the nearest corner centre, only where we're in a corner box.
                let cx = if x < r { r - 1 } else if x >= n - r { n - r } else { x };
                let cy = if y < r { r - 1 } else if y >= n - r { n - r } else { y };
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy < r * r {
                    self.put(x, y, rgb);
                }
            }
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, rgb: [u8; 3]) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                self.put(x, y, rgb);
            }
        }
    }

    fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), thick: i32, rgb: [u8; 3]) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for s in 0..=steps {
            let x = x0 + (x1 - x0) * s / steps;
            let y = y0 + (y1 - y0) * s / steps;
            self.rect(x - thick / 2, y - thick / 2, thick, thick, rgb);
        }
    }

    /// Draw glyphs centred, as tall as fits and as wide as the count allows.
    fn glyphs(&mut self, glyphs: &[[u8; 5]], rgb: [u8; 3]) {
        let n = glyphs.len() as i32;
        let size = self.size as i32;
        let margin = (size / 8).max(2);
        let avail = size - 2 * margin;
        let sy = (avail / 5).max(1);
        // n glyphs of 3 cells plus n-1 gaps of 1 cell = 4n-1 cells.
        let sx = (avail / (4 * n - 1)).min(sy).max(1);
        let total_w = (4 * n - 1) * sx;
        let x_start = (size - total_w) / 2;
        let y_start = (size - 5 * sy) / 2;
        for (gi, g) in glyphs.iter().enumerate() {
            let gx = x_start + gi as i32 * 4 * sx;
            for (row, bits) in g.iter().enumerate() {
                for col in 0..3 {
                    if bits >> (2 - col) & 1 == 1 {
                        self.rect(gx + col * sx, y_start + row as i32 * sy, sx, sy, rgb);
                    }
                }
            }
        }
    }
}

/// Render the icon as ARGB32 (network byte order) bytes, `size * size * 4` long.
pub fn render(size: u32, mic: Option<&Mic>) -> Vec<u8> {
    let mut c = Canvas::new(size);
    let n = size as i32;
    match mic {
        None => {
            c.rounded_square(GREY);
            c.glyphs(&[DASH, DASH], WHITE);
        }
        Some(m) => {
            c.rounded_square(if m.muted { RED } else { TEAL });
            let text = m.gain.min(100).to_string();
            let glyphs: Vec<[u8; 5]> =
                text.bytes().map(|b| DIGITS[(b - b'0') as usize]).collect();
            c.glyphs(&glyphs, WHITE);
            if m.muted {
                let m = n / 5;
                c.line((m, n - m), (n - m, m), (n / 12).max(2), WHITE);
            }
            if m.smart_lock {
                let d = (n / 6).max(3);
                c.rect(n - d - n / 14, n / 14, d, d, AMBER);
            }
        }
    }
    c.data
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Mic = Mic { muted: false, gain: 100, smart_lock: false };

    fn pixel(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        data[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn buffer_is_argb_sized_for_every_size() {
        for s in SIZES {
            assert_eq!(render(s, Some(&ON)).len(), (s * s * 4) as usize);
            assert_eq!(render(s, None).len(), (s * s * 4) as usize);
        }
    }

    #[test]
    fn corners_are_transparent_and_centre_is_opaque() {
        let d = render(32, Some(&ON));
        assert_eq!(pixel(&d, 32, 0, 0)[0], 0);
        assert_eq!(pixel(&d, 32, 16, 1)[0], 0xff);
    }

    #[test]
    fn the_three_looks_and_each_gain_differ() {
        let ok = render(32, Some(&ON));
        let muted = render(32, Some(&Mic { muted: true, ..ON }));
        let gone = render(32, None);
        assert_ne!(ok, muted);
        assert_ne!(ok, gone);
        assert_ne!(muted, gone);
        // Background colours: teal vs red vs grey, sampled in an unused corner of the square.
        assert_eq!(pixel(&ok, 32, 8, 3), [0xff, TEAL[0], TEAL[1], TEAL[2]]);
        assert_eq!(pixel(&muted, 32, 8, 3), [0xff, RED[0], RED[1], RED[2]]);
        assert_eq!(pixel(&gone, 32, 8, 3), [0xff, GREY[0], GREY[1], GREY[2]]);
        let g = |gain| render(32, Some(&Mic { gain, ..ON }));
        assert_ne!(g(7), g(8));
        assert_ne!(g(0), g(100));
        assert_ne!(g(5), g(55));
    }

    #[test]
    fn smart_lock_draws_a_marker() {
        let on = render(32, Some(&Mic { smart_lock: true, ..ON }));
        assert_ne!(on, render(32, Some(&ON)));
    }

    #[test]
    fn out_of_range_gain_does_not_panic() {
        render(22, Some(&Mic { gain: 255, ..ON }));
    }
}
