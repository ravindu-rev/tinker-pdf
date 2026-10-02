//! WebP — RFC 9649 — as a *container* decoder.
//!
//! No PDF stream is ever a WebP file, so this is not a `/Filter`. It is here
//! for `gif.rs`'s reason: the tier-4 archive row names WebP as a format a
//! comic page arrived in and became a placeholder for, and EPUB 3.3 §3.2 makes
//! it a core media type an `<img>` may name.
//!
//! # What is read
//!
//! RFC 9649 §2's RIFF container in all three layouts — simple lossy (`VP8 `),
//! simple lossless (`VP8L`), and extended (`VP8X`) with its `ALPH`, `ANIM`
//! and `ANMF` chunks — §3's lossless bitstream, in `webp/vp8l.rs`, and the
//! lossy one, RFC 6386's VP8 key frame, in `webp/vp8.rs`, with the `ALPH`
//! chunk beside it.
//!
//! # The decisions a WebP forces, each taken once
//!
//! - **The first frame of an animation is the picture**, for `gif.rs`'s
//!   reason, with [`Warning::WebpFramesIgnored`] when there are more. It is
//!   placed at its own offset on the `VP8X` canvas, and the canvas around it
//!   is transparent: §2.7.1.1 makes the `ANIM` background colour "a hint"
//!   that viewers "are not required to use", and a hint is not a picture.
//! - **An image whose every alpha is 255 comes back RGB**, not RGBA. VP8L
//!   carries an alpha channel always and its `alpha_is_used` bit "SHOULD NOT
//!   impact decoding" (§3.4), so opacity is read from the samples rather than
//!   the flag — and an opaque picture needs no soft mask.
//! - **A lossy picture is libwebp's picture.** RFC 6386 ends at the Y, U and
//!   V planes; how they become RGB is a choice, and the choice taken is the
//!   one every browser's WebP decoder makes: libwebp's "fancy" chroma
//!   upsampling and its fixed-point BT.601 conversion, both integer
//!   arithmetic, in `vp8::to_argb`.
//! - **An `ALPH` chunk that will not decode leaves the picture opaque**, with
//!   [`Warning::WebpAlphaDropped`]: the colour is intact and is still the
//!   picture, which is ruling 2's trade. libwebp refuses the whole file.
//! - **Damage inside the pixel data leaves a partial image**, with
//!   [`Warning::TruncatedInput`] or [`Warning::WebpCorruptData`]: the rest of
//!   a lossless picture is transparent black, which is what an ARGB word of
//!   zero is, and the macroblocks a lossy one never reached are opaque black.
//!   Damage in anything that says how to read the pixels (a header, a
//!   transform, a prefix code) is a [`WebpError`], because nothing after it
//!   can be read.

use crate::raster::ImagePixels;
use crate::{Limits, Warning, Warnings};

mod vp8;
mod vp8l;

// --- the budget ---------------------------------------------------------

/// Samples in the **output** raster — `width x height x 4`, the RGBA every
/// WebP decodes to before an opaque one is narrowed — checked with a saturating
/// multiply before any buffer exists.
///
/// | | Samples |
/// | --- | --- |
/// | The most any fixture in this crate spends | 61 440 — Pillow's mixed 160 x 96 |
/// | A comic page: 2000 x 3000 | 24 000 000 |
/// | **This cap** | **67 108 864** |
///
/// `MAX_PNG_SAMPLES`'s `1 << 26`, for its arithmetic. A VP8L or VP8 header's
/// 14-bit dimensions can ask for 16 384 x 16 384 x 4 — 2^30 samples from five
/// bytes — and a `VP8X` canvas's 24-bit ones for 2^50, so the cap is what
/// stands between any of them and an allocation. The decoders' working
/// buffers are one `u32` a pixel, the same bytes, and a lossy frame's planes
/// half as much again over its macroblock grid.
///
/// **Reachable**: `an_image_past_the_sample_cap_is_refused_before_it_allocates`
/// builds both headers.
pub const MAX_WEBP_SAMPLES: u64 = 1 << 26;

// --- the refusals -------------------------------------------------------

/// Why a WebP was refused outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebpError {
    /// Not `RIFF` .... `WEBP`.
    NotWebp,
    /// A chunk, a header or a prefix code is cut off before it ends.
    Truncated,
    /// The container holds no `VP8 `, `VP8L` or `ANMF` chunk to decode.
    NoImage,
    /// A lossless bitstream that breaks a rule of RFC 9649 §3, named.
    Lossless(&'static str),
    /// A lossy bitstream that breaks a rule of RFC 6386, or is a frame a
    /// still image is not (an inter frame, a hidden one), named.
    Lossy(&'static str),
    /// A zero dimension in a VP8 header (the other headers store a
    /// dimension less one, and cannot say zero).
    BadDimensions { width: u32, height: u32 },
    /// [`MAX_WEBP_SAMPLES`] would be spent. Refused before any buffer exists.
    TooManySamples { samples: u64, max: u64 },
    /// The raster would be larger than the caller's own
    /// [`Limits::max_output`].
    ExceedsOutputLimit { bytes: u64, limit: usize },
}

impl core::fmt::Display for WebpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotWebp => f.write_str("not a WebP: no RIFF/WEBP header"),
            Self::Truncated => f.write_str("the WebP is cut off"),
            Self::NoImage => f.write_str("a WebP with no image chunk"),
            Self::Lossless(why) => write!(f, "WebP lossless bitstream: {why}"),
            Self::Lossy(why) => write!(f, "WebP lossy bitstream: {why}"),
            Self::BadDimensions { width, height } => {
                write!(f, "WebP dimensions {width} x {height}")
            }
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples, ceiling is {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes of raster, caller's ceiling is {limit}")
            }
        }
    }
}

impl std::error::Error for WebpError {}

// --- what comes out -----------------------------------------------------

/// A decoded WebP (its first frame, for an animation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebpImage {
    pub width: u32,
    pub height: u32,
    /// RGB when every sample is opaque, RGBA otherwise; never indexed.
    pub pixels: ImagePixels,
    /// False when the pixel data ended or broke before the image did.
    pub complete: bool,
    /// Typed leniency records (ruling 10), deduplicated.
    pub warnings: Vec<Warning>,
}

/// One RIFF chunk: its FourCC and its payload.
#[derive(Clone, Copy)]
struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
}

/// §2.3: chunks end to end, each padded to an even length.
fn chunks<'a>(mut body: &'a [u8], w: &mut Warnings) -> Vec<Chunk<'a>> {
    let mut out = Vec::new();
    while body.len() >= 8 {
        let kind = [body[0], body[1], body[2], body[3]];
        let size = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        let rest = &body[8..];
        let data = match rest.get(..size) {
            Some(d) => d,
            None => {
                // A chunk that says it is longer than the file: the bytes that
                // are there, and a record that they were not all there.
                w.push(Warning::TruncatedInput);
                rest
            }
        };
        out.push(Chunk { kind, data });
        let step = size.saturating_add(size & 1);
        body = rest.get(step..).unwrap_or(&[]);
    }
    out
}

fn u24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(u32::from(s[0]) | (u32::from(s[1]) << 8) | (u32::from(s[2]) << 16))
}

/// The cap and the caller's ceiling, charged on a picture about to be made.
fn charge(width: u64, height: u64, limits: &Limits) -> Result<(), WebpError> {
    let samples = width.saturating_mul(height).saturating_mul(4);
    if samples > MAX_WEBP_SAMPLES {
        return Err(WebpError::TooManySamples {
            samples,
            max: MAX_WEBP_SAMPLES,
        });
    }
    if samples > limits.max_output as u64 {
        return Err(WebpError::ExceedsOutputLimit {
            bytes: samples,
            limit: limits.max_output,
        });
    }
    Ok(())
}

/// A decoded frame: ARGB words, row-major.
struct Frame {
    width: usize,
    height: usize,
    argb: Vec<u32>,
    complete: bool,
}

// --- the entry point ----------------------------------------------------

/// The container, the first frame's bitstream, its alpha, and the canvas.
///
/// # Errors
/// Any [`WebpError`].
pub fn webp_decode(bytes: &[u8], limits: &Limits) -> Result<WebpImage, WebpError> {
    if !(bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice())) {
        return Err(WebpError::NotWebp);
    }
    let mut w = Warnings::default();
    let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    // §2.3: the RIFF size counts from after itself, so the file is it plus 8.
    let end = declared.saturating_add(8).min(bytes.len());
    if declared.saturating_add(8) > bytes.len() {
        w.push(Warning::TruncatedInput);
    }
    let body = bytes.get(12..end).unwrap_or(&[]);
    let list = chunks(body, &mut w);
    let first = list.first().ok_or(WebpError::NoImage)?;

    let Frame {
        width,
        height,
        argb,
        complete,
    } = match &first.kind {
        b"VP8L" | b"VP8 " => decode_frame(None, *first, limits, &mut w)?,
        b"VP8X" => extended(first.data, &list[1..], limits, &mut w)?,
        _ => return Err(WebpError::NoImage),
    };

    let opaque = argb.iter().all(|&p| p >> 24 == 0xff);
    let pixels = if opaque {
        ImagePixels::Rgb(
            argb.iter()
                .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
                .collect(),
        )
    } else {
        ImagePixels::Rgba(
            argb.iter()
                .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8])
                .collect(),
        )
    };
    Ok(WebpImage {
        width: width as u32,
        height: height as u32,
        pixels,
        complete,
        warnings: w.into_vec(),
    })
}

/// §2.7: a `VP8X` file — a still image with its optional `ALPH`, or an
/// animation whose first `ANMF` frame is drawn on the canvas.
fn extended(
    header: &[u8],
    rest: &[Chunk<'_>],
    limits: &Limits,
    w: &mut Warnings,
) -> Result<Frame, WebpError> {
    let canvas_w = u64::from(u24(header, 4).ok_or(WebpError::Truncated)?) + 1;
    let canvas_h = u64::from(u24(header, 7).ok_or(WebpError::Truncated)?) + 1;

    let frames: Vec<&Chunk<'_>> = rest.iter().filter(|c| &c.kind == b"ANMF").collect();
    if let Some(anmf) = frames.first() {
        if frames.len() > 1 {
            w.push(Warning::WebpFramesIgnored);
        }
        charge(canvas_w, canvas_h, limits)?;
        let (cw, ch) = (canvas_w as usize, canvas_h as usize);
        let d = anmf.data;
        let x = u24(d, 0).ok_or(WebpError::Truncated)? as usize * 2;
        let y = u24(d, 3).ok_or(WebpError::Truncated)? as usize * 2;
        let inner = chunks(d.get(16..).unwrap_or(&[]), w);
        let (alpha, bitstream) = frame_chunks(&inner)?;
        let frame = decode_frame(alpha, bitstream, limits, w)?;
        // §2.7.2: the first frame over a cleared canvas — which blending or
        // not leaves as the frame's own pixels — clipped to the canvas.
        let mut canvas = vec![0u32; cw * ch];
        for fy in 0..frame.height {
            for fx in 0..frame.width {
                let (cx, cy) = (x + fx, y + fy);
                if cx < cw && cy < ch {
                    if let (Some(slot), Some(&p)) = (
                        canvas.get_mut(cy * cw + cx),
                        frame.argb.get(fy * frame.width + fx),
                    ) {
                        *slot = p;
                    }
                }
            }
        }
        return Ok(Frame {
            width: cw,
            height: ch,
            argb: canvas,
            complete: frame.complete,
        });
    }
    let (alpha, bitstream) = frame_chunks(rest)?;
    decode_frame(alpha, bitstream, limits, w)
}

/// The optional `ALPH` chunk and the bitstream chunk of one frame.
fn frame_chunks<'a>(list: &[Chunk<'a>]) -> Result<(Option<&'a [u8]>, Chunk<'a>), WebpError> {
    let alpha = list.iter().find(|c| &c.kind == b"ALPH").map(|c| c.data);
    let bitstream = list
        .iter()
        .find(|c| &c.kind == b"VP8 " || &c.kind == b"VP8L")
        .copied()
        .ok_or(WebpError::NoImage)?;
    Ok((alpha, bitstream))
}

/// One frame's bitstream, and its `ALPH` when it is lossy.
fn decode_frame(
    alpha: Option<&[u8]>,
    bitstream: Chunk<'_>,
    limits: &Limits,
    w: &mut Warnings,
) -> Result<Frame, WebpError> {
    let charge = |wd: usize, ht: usize| charge(wd as u64, ht as u64, limits);
    if &bitstream.kind == b"VP8L" {
        // §2.7.1.2: "A frame containing a 'VP8L' Chunk SHOULD NOT contain"
        // an `ALPH` — its alpha is its own — so one that does is ignored.
        let (width, height, argb, complete) = vp8l::decode(bitstream.data, w, charge)?;
        return Ok(Frame {
            width,
            height,
            argb,
            complete,
        });
    }
    let picture = vp8::decode(bitstream.data, w, charge)?;
    let plane = alpha.and_then(|data| {
        let plane = alpha_plane(data, picture.width, picture.height, w);
        if plane.is_none() {
            w.push(Warning::WebpAlphaDropped);
        }
        plane
    });
    Ok(Frame {
        width: picture.width,
        height: picture.height,
        argb: vp8::to_argb(&picture, plane.as_deref()),
        complete: picture.complete,
    })
}

/// §2.7.1.2: an `ALPH` chunk's alpha plane, `width x height` bytes.
///
/// `None` when it will not decode — a compression or pre-processing method
/// past those §2.7.1.2 defines, reserved bits set, a raw plane that is short,
/// or a lossless stream refused — which the caller turns into an opaque
/// picture and [`Warning::WebpAlphaDropped`]. libwebp refuses the same
/// headers.
fn alpha_plane(data: &[u8], width: usize, height: usize, w: &mut Warnings) -> Option<Vec<u8>> {
    let header = *data.first()?;
    let compression = header & 0x03;
    let filtering = (header >> 2) & 0x03;
    let preprocessing = (header >> 4) & 0x03;
    // Pre-processing 1 says the encoder reduced the levels; it changes
    // nothing a decoder does, but a value past it is not one §2.7.1.2 has.
    if preprocessing > 1 || header >> 6 != 0 {
        return None;
    }
    let stream = data.get(1..)?;
    let mut plane = match compression {
        0 => {
            let raw = stream.get(..width * height)?;
            raw.to_vec()
        }
        1 => {
            let mut inner = Warnings::default();
            let (argb, complete) = vp8l::image_stream(stream, width, height, &mut inner).ok()?;
            if !complete {
                w.push(Warning::TruncatedInput);
            }
            // The alpha values are the green channel (§2.7.1.2).
            argb.iter().map(|&p| (p >> 8) as u8).collect()
        }
        _ => return None,
    };
    unfilter(&mut plane, width, height, filtering);
    Some(plane)
}

/// §2.7.1.2's three alpha filters, undone in place.
fn unfilter(plane: &mut [u8], width: usize, height: usize, method: u8) {
    if method == 0 {
        return;
    }
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let predictor = if x == 0 && y == 0 {
                0
            } else if y == 0 {
                // The top row is predicted from the left, whatever the method.
                i32::from(plane[i - 1])
            } else if x == 0 {
                // The left column from above, whatever the method.
                i32::from(plane[i - width])
            } else {
                let (a, b, c) = (
                    i32::from(plane[i - 1]),
                    i32::from(plane[i - width]),
                    i32::from(plane[i - width - 1]),
                );
                match method {
                    1 => a,
                    2 => b,
                    _ => (a + b - c).clamp(0, 255),
                }
            };
            if let Some(v) = plane.get_mut(i) {
                *v = (i32::from(*v) + predictor) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests;
