//! VP8L, WebP's lossless bitstream — RFC 9649 §3, "Specification for WebP
//! Lossless Bitstream".
//!
//! Section numbers below are RFC 9649's. The decoder is the RFC's own
//! structure: an optional chain of four transforms (§3.5), then an image of
//! prefix-coded literals, LZ77 back-references and colour-cache hits (§3.6),
//! under prefix codes that may change block by block (§3.7). Where the RFC
//! leaves a corner open this module says which way it went and why; there
//! are three, and each is marked **decision** where it is taken.

use super::WebpError;
use crate::{Warning, Warnings};

/// §3.7.2.1.2's order for the code-length code's own lengths.
const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// §3.6.2.2.1, Figure 20: distance codes 1 to 120 as (x, y) offsets.
#[rustfmt::skip]
const DISTANCE_MAP: [(i8, i8); 120] = [
    (0, 1), (1, 0), (1, 1), (-1, 1), (0, 2), (2, 0), (1, 2),
    (-1, 2), (2, 1), (-2, 1), (2, 2), (-2, 2), (0, 3), (3, 0),
    (1, 3), (-1, 3), (3, 1), (-3, 1), (2, 3), (-2, 3), (3, 2),
    (-3, 2), (0, 4), (4, 0), (1, 4), (-1, 4), (4, 1), (-4, 1),
    (3, 3), (-3, 3), (2, 4), (-2, 4), (4, 2), (-4, 2), (0, 5),
    (3, 4), (-3, 4), (4, 3), (-4, 3), (5, 0), (1, 5), (-1, 5),
    (5, 1), (-5, 1), (2, 5), (-2, 5), (5, 2), (-5, 2), (4, 4),
    (-4, 4), (3, 5), (-3, 5), (5, 3), (-5, 3), (0, 6), (6, 0),
    (1, 6), (-1, 6), (6, 1), (-6, 1), (2, 6), (-2, 6), (6, 2),
    (-6, 2), (4, 5), (-4, 5), (5, 4), (-5, 4), (3, 6), (-3, 6),
    (6, 3), (-6, 3), (0, 7), (7, 0), (1, 7), (-1, 7), (5, 5),
    (-5, 5), (7, 1), (-7, 1), (4, 6), (-4, 6), (6, 4), (-6, 4),
    (2, 7), (-2, 7), (7, 2), (-7, 2), (3, 7), (-3, 7), (7, 3),
    (-7, 3), (5, 6), (-5, 6), (6, 5), (-6, 5), (8, 0), (4, 7),
    (-4, 7), (7, 4), (-7, 4), (8, 1), (8, 2), (6, 6), (-6, 6),
    (8, 3), (5, 7), (-5, 7), (7, 5), (-7, 5), (8, 4), (6, 7),
    (-6, 7), (7, 6), (-7, 6), (8, 5), (7, 7), (-7, 7), (8, 6),
    (8, 7),
];

/// §3.6.2.3: the colour cache hash multiplier.
const CACHE_MULTIPLIER: u32 = 0x1e35_a7bd;

/// A reader of bits least significant first, the order §3 packs everything
/// in. Reading past the end yields zeros and remembers that it did, so a
/// truncated stream is found once, where it is decided what that costs.
pub(crate) struct Bits<'a> {
    data: &'a [u8],
    at: u64,
    overrun: bool,
}

impl<'a> Bits<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            overrun: false,
        }
    }

    /// `n` bits, at most 32.
    pub(crate) fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let byte = (self.at / 8) as usize;
        let shift = (self.at % 8) as u32;
        let mut window = 0u64;
        for k in 0..8usize {
            let b = self.data.get(byte + k).copied().unwrap_or(0);
            window |= u64::from(b) << (8 * k);
        }
        let end = self.at + u64::from(n);
        if end > (self.data.len() as u64) * 8 {
            self.overrun = true;
        }
        self.at = end;
        ((window >> shift) & ((1u64 << n) - 1)) as u32
    }

    /// One bit: [`Bits::read`]`(1)` without the window, for the prefix-code
    /// walk that reads a bit at a time.
    fn bit(&mut self) -> u32 {
        let byte = self.data.get((self.at / 8) as usize).copied();
        let shift = (self.at % 8) as u32;
        self.at += 1;
        match byte {
            Some(b) => u32::from(b >> shift) & 1,
            None => {
                self.overrun = true;
                0
            }
        }
    }

    pub(crate) fn overrun(&self) -> bool {
        self.overrun
    }
}

/// One canonical prefix code (§3.7), decoded a bit at a time the way
/// DEFLATE's are: a code's first bit is its most significant.
struct Code {
    /// Codes of each length, 1 to 15.
    counts: [u16; 16],
    /// Symbols ordered by code length, then by value.
    symbols: Vec<u16>,
    /// §3.7.2.1: a tree of one leaf, which consumes no bits at all.
    single: Option<u16>,
}

impl Code {
    /// Builds a code from its lengths, refusing a tree that is not complete —
    /// §3.7.2.1: "The described tree must be a complete binary tree" — with
    /// the one exception it names, a single leaf.
    fn build(lengths: &[u8]) -> Result<Self, WebpError> {
        let mut counts = [0u16; 16];
        let mut used = 0usize;
        let mut last = 0u16;
        for (symbol, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let slot = counts
                    .get_mut(usize::from(len))
                    .ok_or(WebpError::Lossless("a code length past 15"))?;
                *slot += 1;
                used += 1;
                last = symbol as u16;
            }
        }
        if used == 0 {
            return Err(WebpError::Lossless("a prefix code with no symbols"));
        }
        if used == 1 {
            return Ok(Self {
                counts,
                symbols: Vec::new(),
                single: Some(last),
            });
        }
        let mut left = 1i64;
        for &count in counts.iter().skip(1) {
            left = left * 2 - i64::from(count);
            if left < 0 {
                return Err(WebpError::Lossless("an over-subscribed prefix code"));
            }
        }
        if left != 0 {
            return Err(WebpError::Lossless("an incomplete prefix code"));
        }
        let mut offsets = [0usize; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + usize::from(counts[len]);
        }
        let mut symbols = vec![0u16; used];
        for (symbol, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let slot = &mut offsets[usize::from(len)];
                if let Some(s) = symbols.get_mut(*slot) {
                    *s = symbol as u16;
                }
                *slot += 1;
            }
        }
        Ok(Self {
            counts,
            symbols,
            single: None,
        })
    }

    /// One symbol. `None` when the bits match no code, which a complete code
    /// makes impossible except past the end of the data.
    fn decode(&self, bits: &mut Bits<'_>) -> Option<u16> {
        if let Some(s) = self.single {
            return Some(s);
        }
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.bit() as i32;
            let count = i32::from(self.counts[len]);
            if code - first < count {
                return self.symbols.get((index + code - first) as usize).copied();
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

/// §3.7.2.1: one prefix code over an alphabet of `size` symbols.
fn read_code(bits: &mut Bits<'_>, size: usize) -> Result<Code, WebpError> {
    let mut lengths = vec![0u8; size];
    if bits.read(1) == 1 {
        // §3.7.2.1.1, the simple code length code.
        let two = bits.read(1) == 1;
        let first_wide = bits.read(1) == 1;
        let first = bits.read(if first_wide { 8 } else { 1 }) as usize;
        *lengths.get_mut(first).ok_or(WebpError::Lossless(
            "a simple code's symbol past its alphabet",
        ))? = 1;
        if two {
            let second = bits.read(8) as usize;
            *lengths.get_mut(second).ok_or(WebpError::Lossless(
                "a simple code's symbol past its alphabet",
            ))? = 1;
        }
    } else {
        // §3.7.2.1.2, the normal code length code.
        let mut cl_lengths = [0u8; 19];
        let n = 4 + bits.read(4) as usize;
        for &slot in CODE_LENGTH_ORDER.iter().take(n) {
            cl_lengths[slot] = bits.read(3) as u8;
        }
        let cl_code = Code::build(&cl_lengths)?;
        // **Decision**: "read up to max_symbol code lengths" counts reads of
        // the code-length code — a repeat is one read however many lengths it
        // writes — which is how libwebp, the format's reference encoder,
        // writes the field.
        let mut budget = if bits.read(1) == 1 {
            let length_bits = 2 + 2 * bits.read(3);
            let max = 2 + bits.read(length_bits) as usize;
            if max > size {
                return Err(WebpError::Lossless("max_symbol past the alphabet"));
            }
            max
        } else {
            size
        };
        let mut symbol = 0usize;
        let mut previous = 8u8;
        while symbol < size {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let len = cl_code
                .decode(bits)
                .ok_or(WebpError::Lossless("a code length that decodes to nothing"))?;
            if len < 16 {
                lengths[symbol] = len as u8;
                symbol += 1;
                if len != 0 {
                    previous = len as u8;
                }
            } else {
                let (extra, offset, value) = match len {
                    16 => (2, 3, previous),
                    17 => (3, 3, 0),
                    _ => (7, 11, 0),
                };
                let repeat = offset + bits.read(extra) as usize;
                if symbol + repeat > size {
                    return Err(WebpError::Lossless(
                        "a repeated code length past the alphabet",
                    ));
                }
                for l in lengths.iter_mut().skip(symbol).take(repeat) {
                    *l = value;
                }
                symbol += repeat;
            }
            if bits.overrun() {
                return Err(WebpError::Truncated);
            }
        }
    }
    if bits.overrun() {
        return Err(WebpError::Truncated);
    }
    Code::build(&lengths)
}

/// §3.7.2: five codes — green-length-cache, red, blue, alpha, distance.
struct Group {
    codes: [Code; 5],
}

fn read_group(bits: &mut Bits<'_>, cache_size: usize) -> Result<Group, WebpError> {
    let green = read_code(bits, 256 + 24 + cache_size)?;
    let red = read_code(bits, 256)?;
    let blue = read_code(bits, 256)?;
    let alpha = read_code(bits, 256)?;
    let distance = read_code(bits, 40)?;
    Ok(Group {
        codes: [green, red, blue, alpha, distance],
    })
}

fn div_round_up(n: usize, bits: u32) -> usize {
    (n + (1usize << bits) - 1) >> bits
}

/// §3.6.2.2's value from a prefix code and its extra bits.
fn prefix_value(code: u32, bits: &mut Bits<'_>) -> usize {
    if code < 4 {
        return code as usize + 1;
    }
    let extra = (code - 2) >> 1;
    let offset = (2 + (code as usize & 1)) << extra;
    offset + bits.read(extra) as usize + 1
}

/// §3.6.2.2.1: a distance code to a scan-line distance.
fn plane_distance(code: usize, width: usize) -> usize {
    if code > 120 {
        return code - 120;
    }
    let (xi, yi) = DISTANCE_MAP
        .get(code.wrapping_sub(1))
        .copied()
        .unwrap_or((0, 1));
    let dist = i64::from(xi) + i64::from(yi) * width as i64;
    dist.max(1) as usize
}

/// What an entropy-coded image decode produced.
struct Decoded {
    pixels: Vec<u32>,
    /// False when the data ran out, or a code or a copy was impossible, and
    /// the rest of the image is black.
    complete: bool,
}

/// §3.6 and §3.7.2.3: an entropy-coded image of `width x height`.
///
/// `level0` is the ARGB image itself, the only role meta prefix codes may be
/// used in (§3.7.2.2); the sub-resolution images have one prefix code group.
fn entropy_image(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    level0: bool,
    w: &mut Warnings,
) -> Result<Decoded, WebpError> {
    // §3.6.2.3: the colour cache.
    let cache_bits = if bits.read(1) == 1 {
        let b = bits.read(4);
        if !(1..=11).contains(&b) {
            return Err(WebpError::Lossless(
                "a colour cache size outside 1 to 11 bits",
            ));
        }
        b
    } else {
        0
    };
    let cache_size = if cache_bits > 0 {
        1usize << cache_bits
    } else {
        0
    };

    // §3.7.2.2: meta prefix codes, the ARGB image only.
    let mut meta: Option<(u32, usize, Vec<u32>)> = None;
    let mut groups_needed = 1usize;
    if level0 && bits.read(1) == 1 {
        let prefix_bits = bits.read(3) + 2;
        let mw = div_round_up(width, prefix_bits);
        let mh = div_round_up(height, prefix_bits);
        let image = entropy_image(bits, mw, mh, false, w)?;
        if !image.complete {
            return Err(WebpError::Truncated);
        }
        groups_needed = image
            .pixels
            .iter()
            .map(|&p| ((p >> 8) & 0xffff) as usize + 1)
            .max()
            .unwrap_or(1);
        meta = Some((prefix_bits, mw, image.pixels));
    }
    // Read one group at a time, so a claim of 65 536 groups costs the input
    // the groups actually occupy rather than an allocation up front.
    let mut groups = Vec::new();
    for _ in 0..groups_needed {
        groups.push(read_group(bits, cache_size)?);
    }

    let total = width.saturating_mul(height);
    let mut pixels = vec![0u32; total];
    let mut cache = vec![0u32; cache_size];
    let mut complete = true;
    let mut pos = 0usize;
    let cache_shift = 32 - cache_bits;
    let insert = |cache: &mut [u32], argb: u32| {
        if let Some(slot) =
            cache.get_mut((argb.wrapping_mul(CACHE_MULTIPLIER) >> cache_shift) as usize)
        {
            *slot = argb;
        }
    };

    while pos < total {
        let (x, y) = (pos % width.max(1), pos / width.max(1));
        let group = match &meta {
            Some((pb, mw, image)) => {
                let index = (y >> pb) * mw + (x >> pb);
                let code = image
                    .get(index)
                    .map_or(0, |&p| ((p >> 8) & 0xffff) as usize);
                groups.get(code)
            }
            None => groups.first(),
        };
        let Some(group) = group else {
            complete = false;
            break;
        };
        let Some(green) = group.codes[0].decode(bits) else {
            complete = false;
            break;
        };
        let green = u32::from(green);
        if green < 256 {
            let r = group.codes[1].decode(bits);
            let b = group.codes[2].decode(bits);
            let a = group.codes[3].decode(bits);
            let (Some(r), Some(b), Some(a)) = (r, b, a) else {
                complete = false;
                break;
            };
            let argb = (u32::from(a) << 24) | (u32::from(r) << 16) | (green << 8) | u32::from(b);
            if bits.overrun() {
                complete = false;
                break;
            }
            pixels[pos] = argb;
            if cache_size > 0 {
                insert(&mut cache, argb);
            }
            pos += 1;
        } else if green < 256 + 24 {
            let length = prefix_value(green - 256, bits);
            let Some(dist_code) = group.codes[4].decode(bits) else {
                complete = false;
                break;
            };
            let dist_code = prefix_value(u32::from(dist_code), bits);
            let dist = plane_distance(dist_code, width);
            // **Decision**: a copy reaching before the first pixel or past
            // the last is well-formed and impossible; §3.6.2.2 does not say
            // what it means, so the image ends here, marked, rather than
            // being clamped into a picture nobody encoded.
            if bits.overrun() || dist > pos || pos + length > total {
                if !bits.overrun() {
                    w.push(Warning::WebpCorruptData);
                }
                complete = false;
                break;
            }
            for k in 0..length {
                let argb = pixels[pos + k - dist];
                pixels[pos + k] = argb;
                if cache_size > 0 {
                    insert(&mut cache, argb);
                }
            }
            pos += length;
        } else {
            let key = (green - 256 - 24) as usize;
            let Some(&argb) = cache.get(key) else {
                w.push(Warning::WebpCorruptData);
                complete = false;
                break;
            };
            if bits.overrun() {
                complete = false;
                break;
            }
            pixels[pos] = argb;
            insert(&mut cache, argb);
            pos += 1;
        }
    }
    if bits.overrun() {
        w.push(Warning::TruncatedInput);
    }
    Ok(Decoded { pixels, complete })
}

/// One transform as read, with the width it was read at (§3.5).
enum Transform {
    Predictor {
        bits: u32,
        width: usize,
        image: Vec<u32>,
    },
    Colour {
        bits: u32,
        width: usize,
        image: Vec<u32>,
    },
    SubtractGreen,
    ColourIndexing {
        bits: u32,
        width: usize,
        palette: Vec<u32>,
    },
}

/// A whole VP8L bitstream, header first (§3.4): the signature, the size,
/// and the image stream.
pub(crate) fn decode(
    data: &[u8],
    w: &mut Warnings,
    check: impl FnOnce(usize, usize) -> Result<(), WebpError>,
) -> Result<(usize, usize, Vec<u32>, bool), WebpError> {
    if data.first() != Some(&0x2f) {
        return Err(WebpError::Lossless("no 0x2f signature"));
    }
    let mut bits = Bits::new(data.get(1..).unwrap_or(&[]));
    let width = bits.read(14) as usize + 1;
    let height = bits.read(14) as usize + 1;
    let _alpha_is_used = bits.read(1);
    let version = bits.read(3);
    if bits.overrun() {
        return Err(WebpError::Truncated);
    }
    if version != 0 {
        return Err(WebpError::Lossless("a VP8L version other than 0"));
    }
    check(width, height)?;
    let (pixels, complete) = stream(&mut bits, width, height, w)?;
    Ok((width, height, pixels, complete))
}

/// A whole VP8L image stream of `width x height` with no header — which is
/// how an `ALPH` chunk carries one (§2.7.1.2), its size being the frame's.
pub(crate) fn image_stream(
    data: &[u8],
    width: usize,
    height: usize,
    w: &mut Warnings,
) -> Result<(Vec<u32>, bool), WebpError> {
    let mut bits = Bits::new(data);
    stream(&mut bits, width, height, w)
}

/// A whole image stream of `width x height`, after the header: the
/// transforms, the entropy-coded image, and the transforms undone.
fn stream(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    w: &mut Warnings,
) -> Result<(Vec<u32>, bool), WebpError> {
    let mut transforms: Vec<Transform> = Vec::new();
    let mut seen = [false; 4];
    let mut xsize = width;
    while bits.read(1) == 1 {
        let kind = bits.read(2) as usize;
        if seen[kind] {
            return Err(WebpError::Lossless("a transform used twice"));
        }
        seen[kind] = true;
        match kind {
            0 | 1 => {
                let b = bits.read(3) + 2;
                let image = entropy_image(
                    bits,
                    div_round_up(xsize, b),
                    div_round_up(height, b),
                    false,
                    w,
                )?;
                if !image.complete {
                    return Err(WebpError::Truncated);
                }
                transforms.push(if kind == 0 {
                    Transform::Predictor {
                        bits: b,
                        width: xsize,
                        image: image.pixels,
                    }
                } else {
                    Transform::Colour {
                        bits: b,
                        width: xsize,
                        image: image.pixels,
                    }
                });
            }
            2 => transforms.push(Transform::SubtractGreen),
            _ => {
                let colours = bits.read(8) as usize + 1;
                let image = entropy_image(bits, colours, 1, false, w)?;
                if !image.complete {
                    return Err(WebpError::Truncated);
                }
                // §3.5.4: the table is subtraction-coded, channel by channel.
                let mut palette = image.pixels;
                for i in 1..palette.len() {
                    palette[i] = add_pixels(palette[i], palette[i - 1]);
                }
                let b = match colours {
                    1..=2 => 3,
                    3..=4 => 2,
                    5..=16 => 1,
                    _ => 0,
                };
                transforms.push(Transform::ColourIndexing {
                    bits: b,
                    width: xsize,
                    palette,
                });
                xsize = div_round_up(xsize, b);
            }
        }
        if bits.overrun() {
            return Err(WebpError::Truncated);
        }
    }

    let image = entropy_image(bits, xsize, height, true, w)?;
    let mut pixels = image.pixels;
    for transform in transforms.iter().rev() {
        pixels = match transform {
            Transform::Predictor { bits, width, image } => {
                predict(&mut pixels, *width, height, *bits, image);
                pixels
            }
            Transform::Colour { bits, width, image } => {
                colour_transform(&mut pixels, *width, *bits, image);
                pixels
            }
            Transform::SubtractGreen => {
                for p in &mut pixels {
                    let g = (*p >> 8) & 0xff;
                    let r = (((*p >> 16) & 0xff) + g) & 0xff;
                    let b = ((*p & 0xff) + g) & 0xff;
                    *p = (*p & 0xff00_ff00) | (r << 16) | b;
                }
                pixels
            }
            Transform::ColourIndexing {
                bits,
                width,
                palette,
            } => unindex(&pixels, *width, height, *bits, palette),
        };
    }
    Ok((pixels, image.complete))
}

/// Channel-wise sum modulo 256.
fn add_pixels(a: u32, b: u32) -> u32 {
    let ag = (a & 0xff00_ff00).wrapping_add(b & 0xff00_ff00) & 0xff00_ff00;
    let rb = (a & 0x00ff_00ff).wrapping_add(b & 0x00ff_00ff) & 0x00ff_00ff;
    ag | rb
}

fn channel(p: u32, shift: u32) -> i32 {
    ((p >> shift) & 0xff) as i32
}

fn map_channels(f: impl Fn(u32) -> u32) -> u32 {
    (f(24) << 24) | (f(16) << 16) | (f(8) << 8) | f(0)
}

fn average2(a: u32, b: u32) -> u32 {
    map_channels(|s| (((a >> s) & 0xff) + ((b >> s) & 0xff)) / 2)
}

/// §3.5.1's Select.
fn select(l: u32, t: u32, tl: u32) -> u32 {
    let mut pl = 0;
    let mut pt = 0;
    for s in [24, 16, 8, 0] {
        let p = channel(l, s) + channel(t, s) - channel(tl, s);
        pl += (p - channel(l, s)).abs();
        pt += (p - channel(t, s)).abs();
    }
    if pl < pt {
        l
    } else {
        t
    }
}

fn clamp8(v: i32) -> u32 {
    v.clamp(0, 255) as u32
}

/// §3.5.1: the fourteen predictors, and — **decision** — 14 and 15, which
/// the RFC leaves undefined, predicting as mode 0 (opaque black), which is
/// what the reference decoder does with them.
fn predictor(mode: u32, l: u32, t: u32, tr: u32, tl: u32) -> u32 {
    match mode {
        1 => l,
        2 => t,
        3 => tr,
        4 => tl,
        5 => average2(average2(l, tr), t),
        6 => average2(l, tl),
        7 => average2(l, t),
        8 => average2(tl, t),
        9 => average2(t, tr),
        10 => average2(average2(l, tl), average2(t, tr)),
        11 => select(l, t, tl),
        12 => map_channels(|s| clamp8(channel(l, s) + channel(t, s) - channel(tl, s))),
        13 => {
            let avg = average2(l, t);
            map_channels(|s| {
                let a = channel(avg, s);
                clamp8(a + (a - channel(tl, s)) / 2)
            })
        }
        _ => 0xff00_0000,
    }
}

/// §3.5.1, undone in scan order over the image being rebuilt in place.
///
/// The neighbours are indexed directly: each is at a smaller index than the
/// pixel being rebuilt, which `get` has just found inside the buffer.
fn predict(pixels: &mut [u32], width: usize, height: usize, bits: u32, image: &[u32]) {
    let blocks = div_round_up(width, bits);
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let Some(&residual) = pixels.get(i) else {
                return;
            };
            let predicted = if y == 0 {
                if x == 0 {
                    0xff00_0000
                } else {
                    pixels[i - 1]
                }
            } else if x == 0 {
                pixels[i - width]
            } else {
                let mode = image
                    .get((y >> bits) * blocks + (x >> bits))
                    .map_or(0, |&p| (p >> 8) & 0xf);
                let l = pixels[i - 1];
                let t = pixels[i - width];
                let tl = pixels[i - width - 1];
                // The rightmost column's TR is the leftmost pixel of its own
                // row, which is where scan order puts the pixel after TL's.
                let tr = pixels[i - width + 1];
                predictor(mode, l, t, tr, tl)
            };
            pixels[i] = add_pixels(residual, predicted);
        }
    }
}

fn delta(t: u32, c: u32) -> i32 {
    (i32::from(t as u8 as i8) * i32::from(c as u8 as i8)) >> 5
}

/// §3.5.2's inverse colour transform.
fn colour_transform(pixels: &mut [u32], width: usize, bits: u32, image: &[u32]) {
    let blocks = div_round_up(width, bits);
    for (i, p) in pixels.iter_mut().enumerate() {
        let (x, y) = (i % width.max(1), i / width.max(1));
        let element = image
            .get((y >> bits) * blocks + (x >> bits))
            .copied()
            .unwrap_or(0);
        let green_to_red = element & 0xff;
        let green_to_blue = (element >> 8) & 0xff;
        let red_to_blue = (element >> 16) & 0xff;
        let green = (*p >> 8) & 0xff;
        let red = ((((*p >> 16) & 0xff) as i32 + delta(green_to_red, green)) & 0xff) as u32;
        let blue = (((*p & 0xff) as i32 + delta(green_to_blue, green) + delta(red_to_blue, red))
            & 0xff) as u32;
        *p = (*p & 0xff00_ff00) | (red << 16) | blue;
    }
}

/// §3.5.4's inverse: unbundle and look up. An index past the table is
/// transparent black, as the RFC says.
fn unindex(packed: &[u32], width: usize, height: usize, bits: u32, palette: &[u32]) -> Vec<u32> {
    let packed_width = div_round_up(width, bits);
    let per = 1usize << bits;
    let depth = 8 >> bits;
    let mask = (1u32 << depth) - 1;
    let mut out = vec![0u32; width * height];
    for y in 0..height {
        for x in 0..width {
            let p = packed.get(y * packed_width + x / per).copied().unwrap_or(0);
            let green = (p >> 8) & 0xff;
            let index = if bits == 0 {
                green
            } else {
                (green >> ((x % per) as u32 * depth)) & mask
            };
            out[y * width + x] = palette.get(index as usize).copied().unwrap_or(0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_code_decodes_canonically_and_refuses_bad_trees() {
        // Lengths 1, 2, 3, 3: codes 0, 10, 110, 111, first bit most
        // significant, read least significant bit of each byte first.
        let code = Code::build(&[1, 2, 3, 3]).expect("complete");
        // Bits in reading order: 1,1,0 (symbol 2), 0 (symbol 0), 1,1,1 (3).
        let data = [0b0111_0011u8];
        let mut bits = Bits::new(&data);
        assert_eq!(code.decode(&mut bits), Some(2));
        assert_eq!(code.decode(&mut bits), Some(0));
        assert_eq!(code.decode(&mut bits), Some(3));
        assert!(Code::build(&[1, 1, 1]).is_err(), "over-subscribed");
        assert!(Code::build(&[1, 2]).is_err(), "incomplete");
        assert!(Code::build(&[0, 0]).is_err(), "empty");
        assert_eq!(Code::build(&[0, 3, 0]).expect("one leaf").single, Some(1));
    }

    #[test]
    fn the_distance_map_reaches_the_neighbourhood_the_rfc_describes() {
        // Code 1 is the pixel above; code 2 the one to the left; code 3 the
        // top-left — RFC 9649 §3.6.2.2.1's own examples.
        assert_eq!(plane_distance(1, 100), 100);
        assert_eq!(plane_distance(2, 100), 1);
        assert_eq!(plane_distance(3, 100), 101);
        assert_eq!(plane_distance(121, 100), 1);
        // A neighbourhood reaching past a narrow image's own row is at least 1.
        assert_eq!(plane_distance(4, 1), 1);
    }
}
