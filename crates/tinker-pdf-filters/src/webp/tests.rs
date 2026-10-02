//! WebP containers built chunk by chunk from RFC 9649 §2, and lossless
//! bitstreams built bit by bit from §3, with the decoder held to them.
//!
//! The Pillow and imagecodecs files in `tests/images/webp/` carry the exit
//! criterion — authored pixels a real encoder compressed, held exactly for a
//! lossless file and at a stated distance for a lossy one, whose VP8 planes
//! are held to published test vectors instead. These build what an encoder
//! was not asked for: a back-reference before the first pixel, a frame offset on its
//! canvas, a padded chunk, every `ALPH` filter and coding, an `ALPH` that
//! will not decode, and one file per [`WebpError`]. The VP8 bitstream itself
//! is `webp/vp8/tests.rs`'s.

use super::*;

const CAP: Limits = Limits::new(1 << 24);

/// Bits least significant first, the order §3 packs everything in.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    at: usize,
}

impl BitWriter {
    fn put(&mut self, value: u32, n: u32) {
        for i in 0..n {
            if self.at % 8 == 0 {
                self.out.push(0);
            }
            if (value >> i) & 1 == 1 {
                *self.out.last_mut().expect("a byte was pushed") |= 1 << (self.at % 8);
            }
            self.at += 1;
        }
    }

    /// §3.7.2.1.1's simple code of one or two symbols, each below 256.
    fn simple(&mut self, symbols: &[u8]) {
        self.put(1, 1);
        self.put(symbols.len() as u32 - 1, 1);
        self.put(1, 1); // is_first_8bits
        self.put(u32::from(symbols[0]), 8);
        if let Some(&second) = symbols.get(1) {
            self.put(u32::from(second), 8);
        }
    }

    /// §3.7.2.1.2's normal code in which every symbol of `alphabet` has
    /// length 0 or 1: a code-length code of the two lengths 0 and 1, each one
    /// bit long, then one bit a symbol.
    fn one_bit_code(&mut self, alphabet: usize, ones: &[usize]) {
        self.put(0, 1); // normal
        self.put(0, 4); // four code-length code lengths: 17, 18, 0, 1
        for len in [0, 0, 1, 1] {
            self.put(len, 3);
        }
        self.put(0, 1); // no max_symbol
        for symbol in 0..alphabet {
            self.put(u32::from(ones.contains(&symbol)), 1);
        }
    }
}

/// A VP8L header (§3.4): the signature, the dimensions less one, the alpha
/// hint and version 0.
fn header(bits: &mut BitWriter, width: u32, height: u32) {
    bits.put(0x2f, 8);
    bits.put(width - 1, 14);
    bits.put(height - 1, 14);
    bits.put(1, 1);
    bits.put(0, 3);
}

/// A `width x 1` image of literals whose green is 0 or 255, one bit a pixel,
/// red 10, blue 30 and alpha `alpha` throughout.
fn two_greens(width: u32, alpha: u8, pixels: &[bool]) -> Vec<u8> {
    let mut bits = BitWriter::default();
    header(&mut bits, width, 1);
    bits.put(0, 1); // no transform
    bits.put(0, 1); // no colour cache
    bits.put(0, 1); // no meta prefix codes
    bits.simple(&[0, 255]); // green
    bits.simple(&[10]); // red
    bits.simple(&[30]); // blue
    bits.simple(&[alpha]); // alpha
    bits.simple(&[0]); // distance
    for &p in pixels {
        bits.put(u32::from(p), 1);
    }
    bits.out
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    out
}

/// A `VP8X` header chunk: flags, then the canvas less one in 24 bits each.
fn vp8x(flags: u8, width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![flags, 0, 0, 0];
    data.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    data.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    chunk(b"VP8X", &data)
}

/// An `ANMF` frame at `(x, y)` — each even — holding `inner`.
fn anmf(x: u32, y: u32, width: u32, height: u32, inner: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(x / 2).to_le_bytes()[..3]);
    data.extend_from_slice(&(y / 2).to_le_bytes()[..3]);
    data.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    data.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    data.extend_from_slice(&[0, 0, 0, 0]); // duration, flags
    data.extend_from_slice(inner);
    chunk(b"ANMF", &data)
}

fn rgb(pixels: &ImagePixels) -> &[u8] {
    match pixels {
        ImagePixels::Rgb(v) => v,
        other => panic!("expected RGB, got {other:?}"),
    }
}

fn rgba(pixels: &ImagePixels) -> &[u8] {
    match pixels {
        ImagePixels::Rgba(v) => v,
        other => panic!("expected RGBA, got {other:?}"),
    }
}

// ---- the lossless bitstream -------------------------------------------------

#[test]
fn a_hand_built_lossless_stream_decodes_pixel_for_pixel() {
    let file = riff(&[chunk(b"VP8L", &two_greens(3, 255, &[false, true, false]))]);
    let img = webp_decode(&file, &CAP).expect("decodes");
    assert_eq!((img.width, img.height), (3, 1));
    assert!(img.complete);
    assert!(img.warnings.is_empty(), "{:?}", img.warnings);
    // Every alpha is 255, so the picture comes back RGB.
    assert_eq!(rgb(&img.pixels), &[10, 0, 30, 10, 255, 30, 10, 0, 30]);
}

#[test]
fn an_alpha_below_opaque_anywhere_keeps_the_alpha_channel() {
    let file = riff(&[chunk(b"VP8L", &two_greens(2, 128, &[true, false]))]);
    let img = webp_decode(&file, &CAP).expect("decodes");
    assert_eq!(rgba(&img.pixels), &[10, 255, 30, 128, 10, 0, 30, 128]);
}

/// A green alphabet with a length code (256) and a distance of one pixel to
/// the left (§3.6.2.2.1's code 2): the first pixel a literal, the rest copies.
fn copies(width: u32, first_is_copy: bool) -> Vec<u8> {
    let mut bits = BitWriter::default();
    header(&mut bits, width, 1);
    bits.put(0, 3); // no transform, no cache, no meta codes
    bits.one_bit_code(256 + 24, &[7, 256]); // green: 7 is 0, length 1 is 1
    bits.simple(&[1]);
    bits.simple(&[2]);
    bits.simple(&[255]);
    bits.simple(&[1]); // distance prefix 1: distance code 2, one to the left
    if first_is_copy {
        bits.put(1, 1);
    } else {
        bits.put(0, 1);
        for _ in 1..width {
            bits.put(1, 1);
        }
    }
    bits.out
}

#[test]
fn a_back_reference_copies_the_pixel_it_names() {
    let file = riff(&[chunk(b"VP8L", &copies(4, false))]);
    let img = webp_decode(&file, &CAP).expect("decodes");
    assert!(img.complete, "{:?}", img.warnings);
    assert_eq!(rgb(&img.pixels), [1, 7, 2].repeat(4).as_slice());
}

#[test]
fn a_back_reference_before_the_first_pixel_stops_the_image() {
    let file = riff(&[chunk(b"VP8L", &copies(4, true))]);
    let img = webp_decode(&file, &CAP).expect("a partial image, not an error");
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::WebpCorruptData));
    // Nothing was decoded, so the picture is transparent black: the rest of
    // an ARGB image the decoder did not reach.
    assert!(rgba(&img.pixels).iter().all(|&v| v == 0));
}

#[test]
fn a_stream_cut_short_is_a_partial_image() {
    let mut stream = two_greens(40, 255, &[true; 40]);
    stream.truncate(stream.len() - 2);
    let file = riff(&[chunk(b"VP8L", &stream)]);
    let img = webp_decode(&file, &CAP).expect("a partial image");
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::TruncatedInput));
}

#[test]
fn the_subtract_green_transform_is_undone() {
    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    bits.put(1, 1); // a transform
    bits.put(2, 2); // subtract green
    bits.put(0, 1); // no more
    bits.put(0, 2); // no cache, no meta codes
    bits.simple(&[100]); // green
    bits.simple(&[200]); // red, less green
    bits.simple(&[250]); // blue, less green
    bits.simple(&[255]);
    bits.simple(&[0]);
    let img = webp_decode(&riff(&[chunk(b"VP8L", &bits.out)]), &CAP).expect("decodes");
    // (200 + 100) mod 256 = 44, (250 + 100) mod 256 = 94.
    assert_eq!(rgb(&img.pixels), &[44, 100, 94]);
}

// ---- the container -----------------------------------------------------------

#[test]
fn an_odd_chunk_is_padded_and_the_next_one_found() {
    let file = riff(&[
        vp8x(0, 3, 1),
        chunk(b"EXIF", &[1, 2, 3]),
        chunk(b"VP8L", &two_greens(3, 255, &[true, true, false])),
    ]);
    let img = webp_decode(&file, &CAP).expect("decodes");
    assert_eq!(rgb(&img.pixels), &[10, 255, 30, 10, 255, 30, 10, 0, 30]);
}

#[test]
fn a_first_frame_is_placed_at_its_offset_on_a_transparent_canvas() {
    let frame = chunk(b"VP8L", &two_greens(1, 255, &[true]));
    let second = chunk(b"VP8L", &two_greens(1, 255, &[false]));
    let file = riff(&[
        vp8x(0x02, 4, 1),
        chunk(b"ANIM", &[0xff; 6]),
        anmf(2, 0, 1, 1, &frame),
        anmf(0, 0, 1, 1, &second),
    ]);
    let img = webp_decode(&file, &CAP).expect("decodes");
    assert_eq!((img.width, img.height), (4, 1));
    assert!(img.warnings.contains(&Warning::WebpFramesIgnored));
    // The ANIM background (opaque white here) is a hint, not the picture.
    assert_eq!(
        rgba(&img.pixels),
        &[0, 0, 0, 0, 0, 0, 0, 0, 10, 255, 30, 255, 0, 0, 0, 0]
    );
}

#[test]
fn a_riff_size_past_the_file_is_read_as_far_as_it_goes() {
    let mut file = riff(&[chunk(b"VP8L", &two_greens(2, 255, &[true, false]))]);
    file[4..8].copy_from_slice(&1000u32.to_le_bytes());
    let img = webp_decode(&file, &CAP).expect("decodes");
    assert!(img.complete);
    assert!(img.warnings.contains(&Warning::TruncatedInput));
}

// ---- every refusal -------------------------------------------------------------

#[test]
fn every_webp_error_is_reached() {
    let refused = |file: &[u8], want: WebpError| {
        assert_eq!(webp_decode(file, &CAP), Err(want));
    };
    refused(b"RIFF\0\0\0\0WEBQ", WebpError::NotWebp);
    refused(b"GIF89a", WebpError::NotWebp);
    refused(&riff(&[]), WebpError::NoImage);
    refused(&riff(&[chunk(b"EXIF", &[0; 4])]), WebpError::NoImage);
    refused(&riff(&[vp8x(0, 2, 2)]), WebpError::NoImage);
    refused(&riff(&[chunk(b"VP8X", &[0; 5])]), WebpError::Truncated);
    refused(&riff(&[chunk(b"VP8L", &[0x2f, 0])]), WebpError::Truncated);
    // A frame tag of zeros is a key frame its encoder said not to show.
    refused(
        &riff(&[chunk(b"VP8 ", &[0; 10])]),
        WebpError::Lossy("a key frame marked not to be shown"),
    );

    let mut stream = two_greens(1, 255, &[true]);
    stream[0] = 0x2e;
    refused(
        &riff(&[chunk(b"VP8L", &stream)]),
        WebpError::Lossless("no 0x2f signature"),
    );

    let mut bits = BitWriter::default();
    bits.put(0x2f, 8);
    bits.put(0, 28);
    bits.put(0, 1);
    bits.put(1, 3);
    refused(
        &riff(&[chunk(b"VP8L", &bits.out)]),
        WebpError::Lossless("a VP8L version other than 0"),
    );

    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    for _ in 0..2 {
        bits.put(1, 1);
        bits.put(2, 2);
    }
    refused(
        &riff(&[chunk(b"VP8L", &bits.out)]),
        WebpError::Lossless("a transform used twice"),
    );

    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    bits.put(0, 1);
    bits.put(1, 1);
    bits.put(12, 4);
    refused(
        &riff(&[chunk(b"VP8L", &bits.out)]),
        WebpError::Lossless("a colour cache size outside 1 to 11 bits"),
    );

    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    bits.put(0, 3);
    bits.put(0, 1); // a normal code
    bits.put(0, 4);
    for len in [0, 0, 1, 0] {
        bits.put(len, 3); // one code-length symbol — a single leaf, legal
    }
    bits.put(1, 1); // max_symbol present
    bits.put(3, 3); // eight bits of it
    bits.put(255, 8); // 257 reads, each a zero length: no symbol is used
    refused(
        &riff(&[chunk(b"VP8L", &bits.out)]),
        WebpError::Lossless("a prefix code with no symbols"),
    );

    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    bits.put(0, 3);
    bits.put(0, 1);
    bits.put(0, 4);
    for len in [0, 0, 1, 1] {
        bits.put(len, 3);
    }
    bits.put(1, 1);
    bits.put(4, 3); // ten bits
    bits.put(1000, 10); // 1002 > 280
    refused(
        &riff(&[chunk(b"VP8L", &bits.out)]),
        WebpError::Lossless("max_symbol past the alphabet"),
    );
}

#[test]
fn a_prefix_code_that_breaks_the_tree_rule_is_refused_by_name() {
    // Three one-bit code-length codes: over-subscribed.
    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    bits.put(0, 3);
    bits.put(0, 1);
    bits.put(0, 4);
    for len in [1, 1, 1, 0] {
        bits.put(len, 3);
    }
    assert_eq!(
        webp_decode(&riff(&[chunk(b"VP8L", &bits.out)]), &CAP),
        Err(WebpError::Lossless("an over-subscribed prefix code"))
    );
    // Two lengths of 2: incomplete.
    let mut bits = BitWriter::default();
    header(&mut bits, 1, 1);
    bits.put(0, 3);
    bits.put(0, 1);
    bits.put(0, 4);
    for len in [2, 2, 0, 0] {
        bits.put(len, 3);
    }
    assert_eq!(
        webp_decode(&riff(&[chunk(b"VP8L", &bits.out)]), &CAP),
        Err(WebpError::Lossless("an incomplete prefix code"))
    );
}

/// The sample cap, by both routes that could ask past it: five bytes of VP8L
/// header, and a `VP8X` canvas for an animation. Neither allocates.
#[test]
fn an_image_past_the_sample_cap_is_refused_before_it_allocates() {
    let mut bits = BitWriter::default();
    header(&mut bits, 16_384, 16_384);
    let file = riff(&[chunk(b"VP8L", &bits.out)]);
    let generous = Limits::new(usize::MAX);
    assert_eq!(
        webp_decode(&file, &generous),
        Err(WebpError::TooManySamples {
            samples: 16_384 * 16_384 * 4,
            max: MAX_WEBP_SAMPLES,
        })
    );

    let frame = chunk(b"VP8L", &two_greens(1, 255, &[true]));
    let file = riff(&[vp8x(0x02, 1 << 24, 1 << 24), anmf(0, 0, 1, 1, &frame)]);
    assert_eq!(
        webp_decode(&file, &generous),
        Err(WebpError::TooManySamples {
            samples: 1 << 50,
            max: MAX_WEBP_SAMPLES,
        })
    );

    // At the cap exactly, the caller's own ceiling is what refuses it.
    let mut bits = BitWriter::default();
    header(&mut bits, 4096, 4096);
    let file = riff(&[chunk(b"VP8L", &bits.out)]);
    assert_eq!(
        webp_decode(&file, &CAP),
        Err(WebpError::ExceedsOutputLimit {
            bytes: MAX_WEBP_SAMPLES,
            limit: 1 << 24,
        })
    );
    // One row more and it is the cap.
    let mut bits = BitWriter::default();
    header(&mut bits, 4096, 4097);
    let file = riff(&[chunk(b"VP8L", &bits.out)]);
    assert!(matches!(
        webp_decode(&file, &generous),
        Err(WebpError::TooManySamples { .. })
    ));
}

#[test]
fn every_error_and_warning_names_itself() {
    for e in [
        WebpError::NotWebp,
        WebpError::Truncated,
        WebpError::NoImage,
        WebpError::Lossless("x"),
        WebpError::Lossy("x"),
        WebpError::BadDimensions {
            width: 0,
            height: 1,
        },
        WebpError::TooManySamples { samples: 1, max: 0 },
        WebpError::ExceedsOutputLimit { bytes: 1, limit: 0 },
    ] {
        assert!(!e.to_string().is_empty());
    }
    for warning in [
        Warning::WebpCorruptData,
        Warning::WebpFramesIgnored,
        Warning::WebpAlphaDropped,
    ] {
        assert!(warning.as_str().starts_with("webp-"));
        assert!(warning.to_string().starts_with("WebP"));
    }
}

// ---- ALPH beside a lossy frame ----------------------------------------------------

/// The `VP8 ` payload of Pillow's 61 x 45 lossy file, to put an `ALPH`
/// beside.
fn lossy_frame() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/images/webp/pillow-lossy-rgb-61x45.webp"
    );
    let file = std::fs::read(path).expect("the committed lossy fixture");
    assert_eq!(&file[12..16], b"VP8 ");
    let size = u32::from_le_bytes([file[16], file[17], file[18], file[19]]) as usize;
    file[20..20 + size].to_vec()
}

const LW: usize = 61;
const LH: usize = 45;

/// The alpha the tests below encode: every value, in a pattern no filter
/// predicts exactly.
fn alpha_at(x: usize, y: usize) -> u8 {
    ((x * 37 + y * 11 + x * y) % 256) as u8
}

/// §2.7.1.2's forward filters, applied to `alpha_at` — the encoder's half,
/// so that the decoder's half is held to the generator's input.
fn filtered(method: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(LW * LH);
    for y in 0..LH {
        for x in 0..LW {
            let predictor = if x == 0 && y == 0 {
                0
            } else if y == 0 {
                i32::from(alpha_at(x - 1, 0))
            } else if x == 0 {
                i32::from(alpha_at(0, y - 1))
            } else {
                let (a, b, c) = (
                    i32::from(alpha_at(x - 1, y)),
                    i32::from(alpha_at(x, y - 1)),
                    i32::from(alpha_at(x - 1, y - 1)),
                );
                match method {
                    1 => a,
                    2 => b,
                    _ => (a + b - c).clamp(0, 255),
                }
            };
            let predictor = if method == 0 { 0 } else { predictor };
            out.push((i32::from(alpha_at(x, y)) - predictor) as u8);
        }
    }
    out
}

fn with_alpha(alph: &[u8]) -> Vec<u8> {
    riff(&[
        vp8x(0x10, LW as u32, LH as u32),
        chunk(b"ALPH", alph),
        chunk(b"VP8 ", &lossy_frame()),
    ])
}

#[test]
fn every_alpha_filter_is_undone() {
    let opaque = webp_decode(&riff(&[chunk(b"VP8 ", &lossy_frame())]), &CAP).expect("decodes");
    let colour = rgb(&opaque.pixels).to_vec();
    for method in 0..4u8 {
        let mut alph = vec![method << 2];
        alph.extend_from_slice(&filtered(method));
        let img = webp_decode(&with_alpha(&alph), &CAP).expect("decodes");
        assert!(
            img.warnings.is_empty(),
            "method {method}: {:?}",
            img.warnings
        );
        let px = rgba(&img.pixels);
        for y in 0..LH {
            for x in 0..LW {
                let at = (y * LW + x) * 4;
                assert_eq!(px[at + 3], alpha_at(x, y), "method {method} ({x}, {y})");
                // The colour is the frame's, whatever its alpha.
                assert_eq!(&px[at..at + 3], &colour[(y * LW + x) * 3..][..3]);
            }
        }
    }
}

/// Compression 1: a VP8L image stream with no header, its alpha the green
/// of each pixel.
#[test]
fn a_lossless_alpha_stream_is_its_green_channel() {
    let mut bits = BitWriter::default();
    bits.put(0, 3); // no transform, no cache, no meta codes
    bits.simple(&[7]); // green: the alpha
    bits.simple(&[200]);
    bits.simple(&[201]);
    bits.simple(&[202]);
    bits.simple(&[0]);
    let mut alph = vec![1u8];
    alph.extend_from_slice(&bits.out);
    let img = webp_decode(&with_alpha(&alph), &CAP).expect("decodes");
    assert!(rgba(&img.pixels).chunks_exact(4).all(|p| p[3] == 7));
}

/// An `ALPH` that will not decode leaves the colour, opaque, and says so —
/// libwebp refuses the whole file instead.
#[test]
fn an_alpha_that_will_not_decode_leaves_the_picture_opaque() {
    let short = vec![0u8; 10];
    let mut bad_stream = vec![1u8];
    bad_stream.extend_from_slice(&[0xff; 4]);
    for (label, alph) in [
        ("compression 2", vec![2u8]),
        ("pre-processing 2", vec![2 << 4]),
        ("a reserved bit", vec![1 << 6]),
        ("a raw plane cut short", short),
        ("a lossless stream refused", bad_stream),
        ("no header at all", Vec::new()),
    ] {
        let img = webp_decode(&with_alpha(&alph), &CAP).expect("still a picture");
        assert!(matches!(img.pixels, ImagePixels::Rgb(_)), "{label}");
        assert!(img.warnings.contains(&Warning::WebpAlphaDropped), "{label}");
    }
}

/// A VP8 header can say zero, which the other headers cannot.
#[test]
fn a_lossy_frame_of_no_width_is_refused_by_name() {
    let mut frame = lossy_frame();
    frame[6] = 0;
    frame[7] = 0;
    assert_eq!(
        webp_decode(&riff(&[chunk(b"VP8 ", &frame)]), &CAP),
        Err(WebpError::BadDimensions {
            width: 0,
            height: LH as u32,
        })
    );
    // And fourteen bits of each is past the cap, asked before allocating.
    frame[6..10].copy_from_slice(&[0xff, 0x3f, 0xff, 0x3f]);
    assert!(matches!(
        webp_decode(&riff(&[chunk(b"VP8 ", &frame)]), &Limits::new(usize::MAX)),
        Err(WebpError::TooManySamples { .. })
    ));
}
