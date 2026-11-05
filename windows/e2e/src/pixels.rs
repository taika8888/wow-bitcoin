//! Screenshot analysis with the rule scripts/e2e.swift uses, calibrated in this crate's tests against ticker-paint's
//! own chart renders composited over the stand-in, so the thresholds were checked before a runner saw them.
use std::fmt;

pub const MIN_COLORS: usize = 16;
/// The stand-in game window's fill, red, green, blue: dark, so candles stand out, and odd, so a capture of it is
/// recognizable.
pub const STANDIN_FILL: [u8; 3] = [0x20, 0x30, 0x40];
pub const MIN_TONED: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inspection {
    /// Distinct colors at 4 bits per channel.
    pub colors: usize,
    /// Pixels close to the default up (green) and down (red) candle colors.
    pub up: usize,
    pub down: usize,
}

impl Inspection {
    pub fn shows_candles(&self) -> bool {
        self.colors >= MIN_COLORS && self.up >= MIN_TONED && self.down >= MIN_TONED
    }
}

impl fmt::Display for Inspection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} colors, {} up-colored and {} down-colored pixels",
            self.colors, self.up, self.down
        )
    }
}

/// `bgra` is top-down 32-bit. A screen capture's alpha byte means nothing, so only color is read.
pub fn inspect(bgra: &[u8]) -> Inspection {
    let mut colors = std::collections::HashSet::new();
    let (mut up, mut down) = (0, 0);
    for pixel in bgra.as_chunks::<4>().0 {
        let (b, g, r) = (
            i32::from(pixel[0]),
            i32::from(pixel[1]),
            i32::from(pixel[2]),
        );
        colors.insert((r >> 4) << 8 | (g >> 4) << 4 | b >> 4);
        if g > r + 60 && g > b + 40 {
            up += 1;
        }
        if r > g + 80 && r > b + 80 {
            down += 1;
        }
    }
    Inspection {
        colors: colors.len(),
        up,
        down,
    }
}

/// The pixels of an `x, y, width, height` rect inside a `stride_px`-wide top-down image, clipped to the image.
pub fn crop(bgra: &[u8], stride_px: usize, rect: (usize, usize, usize, usize)) -> Vec<u8> {
    let rows = bgra.len() / 4 / stride_px.max(1);
    let (x, y, width, height) = rect;
    let right = (x + width).min(stride_px);
    (y..(y + height).min(rows))
        .flat_map(|row| {
            let start = (row * stride_px + x.min(right)) * 4;
            bgra[start..(row * stride_px + right) * 4].iter().copied()
        })
        .collect()
}
