//! JPEG streams that reach every path of the decoder, for the tests that
//! hold a fast path to its direct form (`docs/design/jpeg-speed.md`, "The
//! invariant").
//!
//! The headers are written here; the entropy-coded data is random bytes,
//! which the decoder reads leniently into coefficients of every magnitude.
//! Nothing here is a claim about what a stream should decode to: the tests
//! that use these compare two decodes of the same bytes.

use super::encode::{
    jpeg_encode, HuffSpec, JpegOptions, JpegQuantisation, JpegSampling, JpegSource,
    JpegSourceColour, ANNEX_K3_DC_LUMA, ANNEX_K4_DC_CHROMA, ANNEX_K5_AC_LUMA, ANNEX_K6_AC_CHROMA,
};

/// A xorshift generator: the same numbers on every target.
pub(super) struct Noise(pub(super) u64);

impl Noise {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `n`, which is not zero.
    pub(super) fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// One of `items`.
    pub(super) fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// How a frame's components are scanned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scans {
    /// One scan of every component, interleaved when there are several.
    Interleaved,
    /// One scan for each component, alone.
    EachAlone,
    /// The components' first two in one scan, the rest in another.
    Split,
    /// A progressive frame's scans: the DC interleaved, then each component's
    /// AC bands alone, then the refinements.
    Progressive,
}

/// What a generated stream is.
#[derive(Clone, Debug)]
pub(super) struct Recipe {
    /// The SOF marker's code: 0xC0, 0xC1, 0xC2, 0xC9 or 0xCA.
    pub(super) frame: u8,
    /// 8 or 12.
    pub(super) precision: u8,
    pub(super) width: u16,
    pub(super) height: u16,
    /// Each component's `H` and `V`.
    pub(super) sampling: Vec<(u8, u8)>,
    /// An Adobe `APP14` marker's transform.
    pub(super) adobe: Option<u8>,
    /// `DRI`'s interval, or zero for none.
    pub(super) restart: u16,
    pub(super) scans: Scans,
    /// Huffman tables drawn at random rather than Annex K's.
    pub(super) random_tables: bool,
    /// Where the stream is cut, as a fraction in 1/256 of its length, or
    /// `None` for the whole stream.
    pub(super) cut: Option<u8>,
}

impl Recipe {
    /// Whether every component's sampling divides the largest.
    pub(super) fn divides(&self) -> bool {
        let h_max = self.sampling.iter().map(|s| s.0).max().unwrap_or(1);
        let v_max = self.sampling.iter().map(|s| s.1).max().unwrap_or(1);
        self.sampling
            .iter()
            .all(|&(h, v)| h_max % h == 0 && v_max % v == 0)
    }

    /// The largest `V`.
    pub(super) fn v_max(&self) -> usize {
        usize::from(self.sampling.iter().map(|s| s.1).max().unwrap_or(1))
    }

    /// Whether the frame is sequential.
    pub(super) fn sequential(&self) -> bool {
        matches!(self.frame, 0xC0 | 0xC1 | 0xC9)
    }

    /// Whether the frame is arithmetic-coded.
    fn arithmetic(&self) -> bool {
        matches!(self.frame, 0xC9 | 0xCA)
    }
}

/// The sampling sets a recipe draws from: one to four components, every
/// ratio T.81 allows an encoder to write, and ratios that do not divide.
const SAMPLINGS: &[&[(u8, u8)]] = &[
    &[(1, 1)],
    &[(2, 2)],
    &[(1, 2)],
    &[(3, 1)],
    &[(1, 1), (1, 1)],
    &[(1, 1), (1, 1), (1, 1)],
    &[(2, 1), (1, 1), (1, 1)],
    &[(2, 2), (1, 1), (1, 1)],
    &[(4, 1), (1, 1), (1, 1)],
    &[(1, 2), (1, 1), (1, 1)],
    &[(1, 4), (1, 1), (1, 1)],
    &[(3, 1), (2, 1), (1, 1)],
    &[(2, 2), (2, 1), (1, 2)],
    &[(3, 3), (1, 1), (2, 2)],
    &[(4, 4), (2, 2), (1, 1)],
    &[(1, 1), (1, 1), (1, 1), (1, 1)],
    &[(2, 2), (1, 1), (1, 1), (2, 2)],
    &[(2, 1), (1, 1), (1, 1), (1, 1)],
];

/// A recipe drawn from `noise`.
pub(super) fn random_recipe(noise: &mut Noise) -> Recipe {
    let frame = noise.pick(&[0xC0u8, 0xC1, 0xC2, 0xC9, 0xCA]);
    let precision = if frame == 0xC0 || noise.below(3) != 0 {
        8
    } else {
        12
    };
    let sampling = noise.pick(SAMPLINGS).to_vec();
    let size = |noise: &mut Noise| {
        if noise.below(6) == 0 {
            40 + noise.below(90) as u16
        } else {
            1 + noise.below(40) as u16
        }
    };
    let adobe = match sampling.len() {
        3 | 4 => noise.pick(&[None, None, Some(0u8), Some(1), Some(2)]),
        _ => None,
    };
    let sequential = matches!(frame, 0xC0 | 0xC1 | 0xC9);
    let scans = if !sequential {
        Scans::Progressive
    } else if sampling.len() == 1 {
        Scans::Interleaved
    } else {
        noise.pick(&[
            Scans::Interleaved,
            Scans::Interleaved,
            Scans::EachAlone,
            Scans::Split,
        ])
    };
    Recipe {
        frame,
        precision,
        width: size(noise),
        height: size(noise),
        sampling,
        adobe,
        restart: if noise.below(3) == 0 {
            1 + noise.below(5) as u16
        } else {
            0
        },
        scans,
        random_tables: noise.below(3) == 0,
        cut: (noise.below(4) == 0).then(|| noise.next() as u8),
    }
}

/// Appends a marker segment.
fn segment(out: &mut Vec<u8>, code: u8, body: &[u8]) {
    out.extend_from_slice(&[0xFF, code]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

/// A `DHT` body for one table: its class and slot, its counts and values.
fn table_body(class: u8, slot: u8, bits: &[u8; 16], values: &[u8]) -> Vec<u8> {
    let mut body = vec![(class << 4) | slot];
    body.extend_from_slice(bits);
    body.extend_from_slice(values);
    body
}

/// A complete code of random lengths up to sixteen, its values below
/// `alphabet`.
fn random_code(noise: &mut Noise, alphabet: u64) -> ([u8; 16], Vec<u8>) {
    let longest = 1 + noise.below(16) as usize;
    let mut bits = [0u8; 16];
    let mut free = 2u64;
    for length in 1..=longest {
        let count = if length == longest {
            free
        } else {
            let least = free.saturating_sub(127);
            least + noise.below(free - least)
        };
        bits[length - 1] = count as u8;
        free = 2 * (free - count.min(free));
    }
    let total: usize = bits.iter().map(|&b| usize::from(b)).sum();
    let values = (0..total).map(|_| noise.below(alphabet) as u8).collect();
    (bits, values)
}

/// Random entropy-coded bytes, `X'FF'` stuffed, with restart markers dropped
/// in when the frame has an interval.
fn entropy(noise: &mut Noise, length: usize, restart: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(length + length / 8);
    let mut marker = 0u8;
    for _ in 0..length {
        if restart > 0 && noise.below(24) == 0 {
            out.extend_from_slice(&[0xFF, 0xD0 + marker]);
            marker = (marker + 1) % 8;
        }
        let byte = noise.next() as u8;
        out.push(byte);
        if byte == 0xFF {
            out.push(0x00);
        }
    }
    out
}

/// The stream `recipe` describes, its entropy-coded data from `noise`.
pub(super) fn stream(recipe: &Recipe, noise: &mut Noise) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    let n = recipe.sampling.len();

    for slot in 0..4u8 {
        let largest = noise.pick(&[4u64, 32, 255, 1024]);
        if recipe.precision == 12 && noise.below(2) == 0 {
            let mut body = vec![0x10 | slot];
            for _ in 0..64 {
                let q = 1 + noise.below(largest) as u16;
                body.extend_from_slice(&q.to_be_bytes());
            }
            segment(&mut out, 0xDB, &body);
        } else {
            let mut body = vec![slot];
            body.extend((0..64).map(|_| 1 + noise.below(largest.min(255)) as u8));
            segment(&mut out, 0xDB, &body);
        }
    }
    if let Some(transform) = recipe.adobe {
        segment(
            &mut out,
            0xEE,
            &[b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, transform],
        );
    }
    if recipe.restart > 0 {
        segment(&mut out, 0xDD, &recipe.restart.to_be_bytes());
    }

    let mut sof = vec![recipe.precision];
    sof.extend_from_slice(&recipe.height.to_be_bytes());
    sof.extend_from_slice(&recipe.width.to_be_bytes());
    sof.push(n as u8);
    for (index, &(h, v)) in recipe.sampling.iter().enumerate() {
        sof.extend_from_slice(&[index as u8 + 1, (h << 4) | v, (index as u8).min(3)]);
    }
    segment(&mut out, recipe.frame, &sof);

    if recipe.arithmetic() {
        if noise.below(2) == 0 {
            // B.2.4.3: a DC table's L and U, an AC table's Kx.
            let (l, u) = (noise.below(3) as u8, 1 + noise.below(4) as u8);
            let kx = 1 + noise.below(63) as u8;
            segment(&mut out, 0xCC, &[0x00, (u << 4) | l, 0x10, kx]);
        }
    } else {
        let specs: [(&HuffSpec, u8, u8, u64); 4] = [
            (&ANNEX_K3_DC_LUMA, 0, 0, 16),
            (&ANNEX_K4_DC_CHROMA, 0, 1, 16),
            (&ANNEX_K5_AC_LUMA, 1, 0, 256),
            (&ANNEX_K6_AC_CHROMA, 1, 1, 256),
        ];
        for (spec, class, slot, alphabet) in specs {
            let body = if recipe.random_tables {
                let (bits, values) = random_code(noise, alphabet);
                table_body(class, slot, &bits, &values)
            } else {
                table_body(class, slot, &spec.bits, spec.values)
            };
            segment(&mut out, 0xC4, &body);
        }
    }

    let h_max = usize::from(recipe.sampling.iter().map(|s| s.0).max().unwrap_or(1));
    let v_max = recipe.v_max();
    let mcus = usize::from(recipe.width).div_ceil(8 * h_max)
        * usize::from(recipe.height).div_ceil(8 * v_max);
    let blocks = |component: usize| {
        let (h, v) = recipe.sampling[component];
        mcus * usize::from(h) * usize::from(v)
    };
    let bytes_per_block = 2 + noise.below(18) as usize;
    let mut scan = |out: &mut Vec<u8>, parts: &[usize], band: (u8, u8), approximation: u8| {
        let mut body = vec![parts.len() as u8];
        for &part in parts {
            let tables = (part.min(1) as u8) << 4 | part.min(1) as u8;
            body.extend_from_slice(&[part as u8 + 1, tables]);
        }
        body.extend_from_slice(&[band.0, band.1, approximation]);
        segment(out, 0xDA, &body);
        let blocks: usize = parts.iter().map(|&part| blocks(part)).sum();
        let data = entropy(noise, blocks * bytes_per_block, recipe.restart);
        out.extend_from_slice(&data);
    };

    let all: Vec<usize> = (0..n).collect();
    match recipe.scans {
        Scans::Interleaved => scan(&mut out, &all, (0, 63), 0),
        Scans::EachAlone => {
            for part in 0..n {
                scan(&mut out, &[part], (0, 63), 0);
            }
        }
        Scans::Split => {
            scan(&mut out, &all[..2.min(n)], (0, 63), 0);
            if n > 2 {
                scan(&mut out, &all[2..], (0, 63), 0);
            }
        }
        Scans::Progressive => {
            scan(&mut out, &all, (0, 0), 0x01);
            for part in 0..n {
                scan(&mut out, &[part], (1, 5), 0x01);
                scan(&mut out, &[part], (6, 63), 0x01);
            }
            scan(&mut out, &all, (0, 0), 0x10);
            for part in 0..n {
                scan(&mut out, &[part], (1, 63), 0x10);
            }
        }
    }
    out.extend_from_slice(&[0xFF, 0xD9]);

    if let Some(cut) = recipe.cut {
        let keep = out.len() * usize::from(cut) / 256;
        out.truncate(keep.max(2));
    }
    out
}

/// A raster for the encoder: a ramp with a pattern on it, in `colour`.
fn raster(width: u32, height: u32, colour: JpegSourceColour) -> Vec<u8> {
    let components = usize::from(colour.components());
    (0..width as usize * height as usize * components)
        .map(|i| {
            let pixel = i / components;
            let (x, y) = (pixel % width as usize, pixel / width as usize);
            ((x * 7 + y * 3 + (i % components) * 61 + (x ^ y) * 5) % 256) as u8
        })
        .collect()
}

/// Streams the tree's own encoder writes: baseline, Gray and YCbCr, 4:4:4
/// and 4:2:0, odd sizes, with and without restart intervals.
pub(super) fn encoded() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for (width, height) in [(1, 1), (8, 8), (13, 7), (17, 33), (64, 48)] {
        for (colour, sampling) in [
            (JpegSourceColour::Gray, JpegSampling::FourFourFour),
            (JpegSourceColour::Rgb, JpegSampling::FourFourFour),
            (JpegSourceColour::Rgb, JpegSampling::FourTwoZero),
        ] {
            for restart_interval in [0u16, 3] {
                let samples = raster(width, height, colour);
                let source = JpegSource {
                    width,
                    height,
                    colour,
                    stride: width as usize * usize::from(colour.components()),
                    data: &samples,
                };
                let options = JpegOptions {
                    quantisation: JpegQuantisation::AnnexKHalved,
                    sampling,
                    restart_interval,
                };
                if let Ok(bytes) = jpeg_encode(&source, &options) {
                    out.push((
                        format!(
                            "encoded {width}x{height} {colour:?} {sampling:?} {restart_interval}"
                        ),
                        bytes,
                    ));
                }
            }
        }
    }
    out
}

/// The committed fuzz seeds, `fuzz/corpus/jpeg/`.
pub(super) fn seeds() -> Vec<(String, Vec<u8>)> {
    [
        (
            "arithmetic-progressive",
            &include_bytes!("../../../../fuzz/corpus/jpeg/arithmetic-progressive.jpg")[..],
        ),
        (
            "arithmetic-sequential",
            &include_bytes!("../../../../fuzz/corpus/jpeg/arithmetic-sequential.jpg")[..],
        ),
        (
            "progressive-refined",
            &include_bytes!("../../../../fuzz/corpus/jpeg/progressive-refined.jpg")[..],
        ),
        (
            "progressive",
            &include_bytes!("../../../../fuzz/corpus/jpeg/progressive.jpg")[..],
        ),
        (
            "sequential",
            &include_bytes!("../../../../fuzz/corpus/jpeg/sequential.jpg")[..],
        ),
        (
            "tiny-gray",
            &include_bytes!("../../../../fuzz/corpus/jpeg/tiny-gray.jpg")[..],
        ),
    ]
    .into_iter()
    .map(|(name, bytes)| (format!("seed {name}"), bytes.to_vec()))
    .collect()
}

/// Streams whose end is not their scan's: a segment after the scan that the
/// end of a decode reads — a `DQT`, an Adobe `APP14`, a second scan or a
/// second frame header — or a frame `finish` refuses.
pub(super) fn tails(noise: &mut Noise) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for case in 0..24u64 {
        let mut recipe = random_recipe(noise);
        recipe.frame = noise.pick(&[0xC0u8, 0xC1, 0xC9]);
        if recipe.frame == 0xC0 {
            recipe.precision = 8;
        }
        recipe.scans = Scans::Interleaved;
        recipe.cut = None;
        let mut bytes = stream(&recipe, noise);
        // Drop the EOI and put a segment where it was.
        bytes.truncate(bytes.len() - 2);
        let name = match case % 6 {
            0 => {
                let mut body = vec![0x00];
                body.extend((0..64).map(|k| 1 + (k * 7 % 200) as u8));
                segment(&mut bytes, 0xDB, &body);
                "a DQT after the scan"
            }
            1 => {
                segment(
                    &mut bytes,
                    0xEE,
                    &[b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, 2],
                );
                "an Adobe APP14 after the scan"
            }
            2 => {
                let tables = if recipe.arithmetic() { 0x00 } else { 0x11 };
                segment(&mut bytes, 0xDA, &[1, 1, tables, 0, 63, 0]);
                let data = entropy(noise, 64, 0);
                bytes.extend_from_slice(&data);
                "a second scan of a component"
            }
            3 => {
                let mut sof = vec![8, 0, 9, 0, 11, 1, 1, 0x11, 0];
                sof[0] = recipe.precision;
                segment(&mut bytes, recipe.frame, &sof);
                "a second frame header"
            }
            4 => {
                segment(&mut bytes, 0xC3, &[8, 0, 8, 0, 8, 1, 1, 0x11, 0]);
                "a lossless frame header after the scan"
            }
            _ => {
                segment(&mut bytes, 0xFE, b"a comment, which changes nothing");
                segment(
                    &mut bytes,
                    0xC4,
                    &table_body(
                        0,
                        3,
                        &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                        &[0],
                    ),
                );
                "a comment and a DHT after the scan"
            }
        };
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        out.push((format!("{name}, {recipe:?}"), bytes));
    }
    out
}
