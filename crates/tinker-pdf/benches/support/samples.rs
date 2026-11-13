//! The benchmark set's raw byte inputs: seeded samples by formula, and the
//! level-6 byte counts tier P holds the encode group's two deflate inputs to
//! (ruling 17), each beside its generator.
//!
//! `inputs.rs` takes these and re-exports them, so a group reads them where it
//! reads every other input. They are apart because none of them is a
//! document: a test that reads a count or a sample takes this file alone, and
//! compiles none of the generators that write one.

use tinker_pdf_crypto::sha2::sha256;

/// The integer stream every seeded input draws from: SplitMix64, so an input
/// is the same bytes on every target.
pub struct Seeded(u64);

impl Seeded {
    /// The stream for `seed`.
    pub fn new(seed: u64) -> Seeded {
        Seeded(seed)
    }

    /// The next 64 bits.
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `bound`, which is at least one.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// `width` by `height` RGB samples by formula: smooth fields, hard-edged
/// shapes and a few levels of seeded noise, the photographic content of
/// every image input.
pub fn photo(width: u32, height: u32, seed: u64) -> Vec<u8> {
    let mut noise = Seeded::new(seed);
    let (w, h) = (u64::from(width), u64::from(height));
    // Eight rectangles and discs, placed and coloured by the seed.
    let shapes: Vec<(u64, u64, u64, [u8; 3], bool)> = (0..8)
        .map(|_| {
            let cx = noise.next() % w;
            let cy = noise.next() % h;
            let r = w / 16 + noise.next() % (w / 5);
            let colour = [noise.next() as u8, noise.next() as u8, noise.next() as u8];
            (cx, cy, r, colour, noise.next() % 2 == 0)
        })
        .collect();
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut pixel = [
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                ((x + y) * 255 / (w + h)) as u8,
            ];
            for (cx, cy, r, colour, disc) in &shapes {
                let (dx, dy) = (x.abs_diff(*cx), y.abs_diff(*cy));
                let inside = if *disc {
                    dx * dx + dy * dy <= r * r
                } else {
                    dx <= *r && dy <= *r / 2
                };
                if inside {
                    pixel = *colour;
                }
            }
            let grain = noise.next();
            for (at, sample) in pixel.iter_mut().enumerate() {
                let level = ((grain >> (8 * at)) & 7) as u8;
                *sample = sample.saturating_add(level).saturating_sub(4);
            }
            out.extend_from_slice(&pixel);
        }
    }
    out
}

/// The bytes of [`page_like`] and [`photo_like`]: 1240 by 1755 RGB.
pub const PAGE_BYTES: usize = 1240 * 1755 * 3;

/// A level-6 byte count, tier P's planning number for one of the encode
/// group's two deflate inputs (ruling 17): the ratio level's deflate of that
/// input is held to at most 1.02 times it. It carries the length and SHA-256
/// of the bytes it was taken on.
///
/// Each count was computed once by a reference deflate outside the engine,
/// over the bytes its generator returns: a raw RFC 1951 stream, as
/// `tinker_pdf_filters::deflate` writes, at level 6's parameters — a 32 KiB
/// window, memory level 8 and the default strategy. It is never measured
/// again against a build and nothing in the tree runs level 6: a count states
/// a target and judges no output. A count describes its bytes alone, so each
/// generator asserts them of its own output and a generator changed under
/// its count fails there first; a moved digest is a finding, never re-pinned.
pub struct LevelSix {
    /// How many bytes a level-6 deflate gives on the input.
    pub count: usize,
    /// The input's length.
    pub length: usize,
    /// The input's SHA-256, in lower-case hexadecimal.
    pub sha256: &'static str,
}

impl LevelSix {
    /// Whether `bytes` are the input this count was taken on.
    pub fn describes(&self, bytes: &[u8]) -> bool {
        bytes.len() == self.length && hex(&sha256(bytes)) == self.sha256
    }
}

/// [`page_like`]'s level-6 count.
pub const PAGE_LIKE_LEVEL_SIX: LevelSix = LevelSix {
    count: 88_264,
    length: PAGE_BYTES,
    sha256: "64165ddc859a4924f3f18fb4c8a6ca237c56580bf90a9aa6a48c0b24ad3e590c",
};

/// [`photo_like`]'s level-6 count.
pub const PHOTO_LIKE_LEVEL_SIX: LevelSix = LevelSix {
    count: 4_759_080,
    length: PAGE_BYTES,
    sha256: "6f4014bf78787788f40d868b63e1fb05d2ee79b1884b39ca788addda04549d08",
};

/// 1240 by 1755 RGB of text-like marks on white, about a tenth of the
/// pixels inked: lines of glyph-sized boxes in words, each box drawn in a
/// stroke pattern, by formula — never a render, so it cannot move with the
/// renderer. The bytes [`PAGE_LIKE_LEVEL_SIX`] was taken on, asserted.
pub fn page_like() -> Vec<u8> {
    let (width, height) = (1240usize, 1755usize);
    let mut out = vec![255u8; PAGE_BYTES];
    let mut seed = Seeded::new(11);
    let mut top = 60;
    while top + 24 < height - 60 {
        let mut x = 80;
        while x + 12 < width - 80 {
            for _ in 0..2 + seed.below(8) {
                let (glyph, tall) = (5 + seed.below(6), 10 + seed.below(6));
                for y in top + 16 - tall..top + 16 {
                    for column in x..(x + glyph).min(width - 80) {
                        if (column + 2 * y) % 4 == 0 {
                            let at = (y * width + column) * 3;
                            out[at..at + 3].copy_from_slice(&[24, 24, 32]);
                        }
                    }
                }
                x += glyph + 2;
            }
            x += 8;
        }
        top += 24;
    }
    assert!(
        PAGE_LIKE_LEVEL_SIX.describes(&out),
        "page_like() is the bytes its level-6 count was taken on"
    );
    out
}

/// [`photo`] at 1240 by 1755, seed 0: the photo-like data the encode and
/// decode groups compress. The bytes [`PHOTO_LIKE_LEVEL_SIX`] was taken on,
/// asserted.
pub fn photo_like() -> Vec<u8> {
    let out = photo(1240, 1755, 0);
    assert!(
        PHOTO_LIKE_LEVEL_SIX.describes(&out),
        "photo_like() is the bytes its level-6 count was taken on"
    );
    out
}

/// `bytes` as lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
