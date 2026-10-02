//! VP8 held to frames built bool by bool with RFC 6386 §7.3's own encoder,
//! to the WebM project's test vectors, and — for the colour conversion the
//! vectors stop short of — to the arithmetic it claims.
//!
//! # The vectors
//!
//! `webmproject/vp8-test-vectors` is 61 IVF files and, beside each, the MD5
//! of every frame libvpx decodes from it. The repository carries no licence,
//! so nothing of it is committed here: `tests/vp8-vectors/fetch.sh` fetches
//! it into `target/`, pinned to its last commit and to the SHA-256 of every
//! file in `tests/vp8-vectors/SHA256SUMS`, and every **key frame** in it is
//! decoded and hashed:
//!
//! ```text
//! sh crates/tinker-pdf-filters/tests/vp8-vectors/fetch.sh
//! TINKER_VP8_VECTORS=$PWD/target/vp8-test-vectors TINKER_VP8_VECTORS_REQUIRED=1 \
//!     cargo test -p tinker-pdf-filters --lib vp8_test_vectors -- --nocapture
//! ```
//!
//! A key frame decodes on its own — §9.3, §9.6 and §13.5 reset everything a
//! key frame reads — so each is a still picture as a WebP would hold it, and
//! its MD5 is the published answer for it. That makes this the one check of
//! the lossy decoder whose expected answers are published data rather than
//! this repository's own reading of the RFC, and CI's `vp8-vectors` job runs
//! it on every push with `TINKER_VP8_VECTORS_REQUIRED=1` and greps for
//! [`VECTORS_RAN`]. Without the variable the test prints [`VECTORS_SKIPPED`]
//! and passes, as `png_suite.rs` does; with the switch set the absence is a
//! failure, and so is any vector set that is not the pinned commit's 61 files
//! and 182 key frames.
//!
//! # The colour conversion
//!
//! RFC 6386 ends at the Y, U and V planes, and so do the vectors' MD5s.
//! `to_argb` goes on to a picture by two stated rules — BT.601's
//! limited-range matrix, and chroma upsampled by weighting the four nearest
//! samples 9:3:3:1 — and the tests at the end of this file hold it to those
//! rules as written, worked out here in exact arithmetic, and never to another
//! decoder's output (ruling 13).

use super::*;

// --- an MD5 (RFC 1321), so that the vectors' hashes can be compared ----------

const MD5_SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// RFC 1321 §3.4's table, `floor(abs(sin(i + 1)) * 2^32)`, written out.
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

fn md5(data: &[u8]) -> String {
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in message.chunks_exact(64) {
        let m: Vec<u32> = block
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let sum = a.wrapping_add(f).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(sum.rotate_left(MD5_SHIFTS[i]));
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    }
    state
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn the_md5_here_is_rfc_1321s() {
    // RFC 1321 Appendix A.5's test suite.
    assert_eq!(md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(md5(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(md5(b"message digest"), "f96b697d7cb7938d525a2f31aaf161d0");
    assert_eq!(
        md5(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"),
        "57edf4a22be3c955ac49da2e2107b67a"
    );
}

// --- RFC 6386 §7.3's encoder, to build frames with ---------------------------

struct BoolEncoder {
    out: Vec<u8>,
    range: u32,
    bottom: u32,
    bit_count: i32,
}

impl BoolEncoder {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            range: 255,
            bottom: 0,
            bit_count: 24,
        }
    }

    fn add_one(&mut self) {
        for b in self.out.iter_mut().rev() {
            if *b == 255 {
                *b = 0;
            } else {
                *b += 1;
                return;
            }
        }
    }

    fn put(&mut self, prob: u8, value: bool) {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        if value {
            self.bottom = self.bottom.wrapping_add(split);
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            if self.bottom & (1 << 31) != 0 {
                self.add_one();
            }
            self.bottom <<= 1;
            self.bit_count -= 1;
            if self.bit_count == 0 {
                self.out.push((self.bottom >> 24) as u8);
                self.bottom &= (1 << 24) - 1;
                self.bit_count = 8;
            }
        }
    }

    fn bit(&mut self, value: bool) {
        self.put(128, value);
    }

    fn literal(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            self.bit((value >> i) & 1 == 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        let mut c = self.bit_count;
        let mut v = self.bottom;
        if v & (1 << (32 - c)) != 0 {
            self.add_one();
        }
        v <<= c & 7;
        c >>= 3;
        while c > 0 {
            v <<= 8;
            c -= 1;
        }
        for _ in 0..4 {
            self.out.push((v >> 24) as u8);
            v <<= 8;
        }
        self.out
    }
}

#[test]
fn the_decoder_reads_back_what_the_rfcs_encoder_wrote() {
    let mut e = BoolEncoder::new();
    let mut want = Vec::new();
    let mut seed = 12345u32;
    for _ in 0..2000 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let prob = (seed >> 24) as u8 | 1;
        let value = (seed >> 7) & 0xff < u32::from(prob) / 2;
        e.put(prob, value);
        want.push((prob, value));
    }
    let data = e.finish();
    let mut d = BoolDecoder::new(&data);
    for (i, &(prob, value)) in want.iter().enumerate() {
        assert_eq!(d.get(prob), value, "bool {i}");
    }
    assert!(!d.exhausted());
}

/// The frame header fields this file sets; everything else is zero.
struct Spec {
    width: u32,
    height: u32,
    level: u32,
    skip_prob: Option<u8>,
    tag: Option<u32>,
}

impl Spec {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            level: 0,
            skip_prob: Some(1),
            tag: None,
        }
    }
}

/// A key frame: §9's header with no segmentation, no deltas, quantizer 0
/// and one token partition, then `macroblocks` writing each macroblock's
/// header, then `tokens` as the token partition.
fn key_frame(spec: &Spec, macroblocks: impl FnOnce(&mut BoolEncoder), tokens: Vec<u8>) -> Vec<u8> {
    let mut e = BoolEncoder::new();
    e.bit(false); // colour space
    e.bit(false); // clamping
    e.bit(false); // segmentation
    e.bit(false); // normal filter
    e.literal(spec.level, 6);
    e.literal(0, 3); // sharpness
    e.bit(false); // no deltas
    e.literal(0, 2); // one token partition
    e.literal(0, 7); // quantizer index
    for _ in 0..5 {
        e.bit(false); // no quantizer deltas
    }
    e.bit(false); // refresh entropy
    for types in &COEFF_UPDATE_PROBS {
        for bands in types {
            for ctxs in bands {
                for &p in ctxs {
                    e.put(p, false);
                }
            }
        }
    }
    match spec.skip_prob {
        Some(p) => {
            e.bit(true);
            e.literal(u32::from(p), 8);
        }
        None => e.bit(false),
    }
    macroblocks(&mut e);
    let first = e.finish();
    let tag = spec.tag.unwrap_or((first.len() as u32) << 5 | 1 << 4);
    let mut out = tag.to_le_bytes()[..3].to_vec();
    out.extend_from_slice(&[0x9d, 0x01, 0x2a]);
    out.extend_from_slice(&(spec.width as u16).to_le_bytes());
    out.extend_from_slice(&(spec.height as u16).to_le_bytes());
    out.extend_from_slice(&first);
    out.extend_from_slice(&tokens);
    out
}

/// A macroblock's header for DC prediction of luma and chroma, skipped or
/// not: §11.2's trees walked by hand.
fn dc_macroblock(e: &mut BoolEncoder, skip: Option<(u8, bool)>) {
    if let Some((p, skip)) = skip {
        e.put(p, skip);
    }
    // KF_Y_MODE_TREE: not B_PRED (1), then DC_PRED's branch (0, 0).
    e.put(KF_Y_MODE_PROBS[0], true);
    e.put(KF_Y_MODE_PROBS[1], false);
    e.put(KF_Y_MODE_PROBS[2], false);
    // UV_MODE_TREE: DC_PRED (0).
    e.put(KF_UV_MODE_PROBS[0], false);
}

fn decode_ok(data: &[u8]) -> (Picture, Vec<Warning>) {
    let mut w = Warnings::default();
    let picture = decode(data, &mut w, |_, _| Ok(())).expect("decodes");
    (picture, w.into_vec())
}

#[test]
fn a_frame_of_prediction_alone_is_mid_grey() {
    // 20 x 20 is four macroblocks, three of them cropped.
    let spec = Spec::new(20, 20);
    let frame = key_frame(
        &spec,
        |e| {
            for _ in 0..4 {
                dc_macroblock(e, Some((1, true)));
            }
        },
        BoolEncoder::new().finish(),
    );
    let (p, warnings) = decode_ok(&frame);
    assert!(p.complete);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!((p.width, p.height), (20, 20));
    // §12.2: DC with no edge is 128, and every macroblock after the first
    // averages edges that are 128.
    assert!(p.y.iter().all(|&v| v == 128));
    assert_eq!((p.u.len(), p.v.len()), (100, 100));
    assert!(p.u.iter().chain(&p.v).all(|&v| v == 128));
    // And libwebp's conversion makes 128, 128, 128 the grey 130.
    let argb = to_argb(&p, None);
    assert!(argb.iter().all(|&px| px == 0xff82_8282), "{:08x}", argb[0]);
}

#[test]
fn a_token_partition_that_ends_early_leaves_the_rest_black() {
    // Sixteen macroblocks, none skipped, so every block reads its tokens —
    // from a partition of no bytes at all.
    let mut spec = Spec::new(64, 64);
    spec.skip_prob = None;
    let frame = key_frame(
        &spec,
        |e| {
            for _ in 0..16 {
                dc_macroblock(e, None);
            }
        },
        Vec::new(),
    );
    let (p, warnings) = decode_ok(&frame);
    assert!(!p.complete);
    assert!(warnings.contains(&Warning::TruncatedInput));
    // The last macroblock was never reached: Y 0 under neutral chroma, which
    // libwebp's conversion makes black.
    let at = 63 * 64 + 63;
    assert_eq!(p.y[at], 0);
    assert_eq!((p.u[32 * 32 - 1], p.v[32 * 32 - 1]), (128, 128));
    assert_eq!(yuv_to_rgb(0, 128, 128), 0);
}

#[test]
fn every_lossy_refusal_is_reached() {
    let grey = |spec: &Spec| {
        key_frame(
            spec,
            |e| dc_macroblock(e, Some((1, true))),
            BoolEncoder::new().finish(),
        )
    };
    let refused = |data: &[u8]| {
        let mut w = Warnings::default();
        decode(data, &mut w, |_, _| Ok(())).err()
    };
    let ok = grey(&Spec::new(16, 16));
    assert!(refused(&ok).is_none());

    let mut inter = ok.clone();
    inter[0] |= 1;
    assert_eq!(
        refused(&inter),
        Some(WebpError::Lossy(
            "an inter frame, which a still image is not"
        ))
    );
    let mut version = ok.clone();
    version[0] |= 4 << 1;
    assert_eq!(
        refused(&version),
        Some(WebpError::Lossy("a version past 3"))
    );
    let mut hidden = ok.clone();
    hidden[0] &= !(1 << 4);
    assert_eq!(
        refused(&hidden),
        Some(WebpError::Lossy("a key frame marked not to be shown"))
    );
    let mut start = ok.clone();
    start[4] = 0x02;
    assert_eq!(
        refused(&start),
        Some(WebpError::Lossy("no key frame start code"))
    );
    assert_eq!(
        refused(&grey(&Spec::new(0, 16))),
        Some(WebpError::BadDimensions {
            width: 0,
            height: 16
        })
    );
    assert_eq!(refused(&ok[..9]), Some(WebpError::Truncated));
    assert_eq!(refused(&ok[..2]), Some(WebpError::Truncated));
    // The cap is asked before anything is allocated.
    let mut w = Warnings::default();
    let asked = decode(&grey(&Spec::new(16383, 16383)), &mut w, |wd, ht| {
        Err(WebpError::TooManySamples {
            samples: (wd * ht * 4) as u64,
            max: 0,
        })
    });
    assert!(matches!(asked, Err(WebpError::TooManySamples { .. })));
}

/// The scaling hint in a dimension's top two bits says how to display the
/// picture, not how big it is.
#[test]
fn the_scaling_bits_are_not_part_of_the_size() {
    let mut spec = Spec::new(16 | (2 << 14), 16 | (3 << 14));
    spec.level = 10;
    let frame = key_frame(
        &spec,
        |e| dc_macroblock(e, Some((1, true))),
        BoolEncoder::new().finish(),
    );
    let (p, _) = decode_ok(&frame);
    assert_eq!((p.width, p.height), (16, 16));
}

/// The inverse transforms, held to values worked by hand from §14.3 and
/// §14.4: a lone DC goes everywhere as `(dc + 3) >> 3` and `(dc + 4) >> 3`.
#[test]
fn a_lone_dc_spreads_evenly_through_both_transforms() {
    let mut input = [0i16; 16];
    input[0] = 85;
    let mut out = [0i16; 16];
    inverse_wht(&input, &mut out);
    assert!(out.iter().all(|&v| v == (85 + 3) >> 3));

    let mut buf = vec![100u8; 4 * 4];
    let mut coeffs = [0i16; 16];
    coeffs[0] = -60;
    idct_add(&mut buf, 0, 4, &coeffs);
    assert!(buf.iter().all(|&v| v == 100 - 7), "{buf:?}");
}

// --- the vectors -----------------------------------------------------------------

/// The key frames of an IVF file: (frame number among those shown, data).
fn ivf_key_frames(file: &[u8]) -> Vec<(usize, &[u8])> {
    let header = usize::from(u16::from_le_bytes([file[6], file[7]]));
    let mut at = header;
    let mut shown = 0usize;
    let mut out = Vec::new();
    while at + 12 <= file.len() {
        let size =
            u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let Some(frame) = file.get(at + 12..at + 12 + size) else {
            break;
        };
        let tag = u32::from(frame[0]);
        let visible = (tag >> 4) & 1 == 1;
        if tag & 1 == 0 && visible {
            out.push((shown, frame));
        }
        if visible {
            shown += 1;
        }
        at += 12 + size;
    }
    out
}

/// Printed when the vectors were read. CI's `vp8-vectors` job greps for it.
const VECTORS_RAN: &str = "vp8-test-vectors: RAN";

/// Printed when they were not. CI greps for it too, and fails.
const VECTORS_SKIPPED: &str = "vp8-test-vectors: SKIPPED";

/// The pinned commit's vectors, counted when it was pinned: an `.ivf` and an
/// `.ivf.md5` each, and the key frames shown among them.
const PINNED_FILES: usize = 61;
const PINNED_KEY_FRAMES: usize = 182;

/// `TINKER_VP8_VECTORS_REQUIRED` makes the absence of the vectors a failure
/// rather than a skip, and holds what is there to the pinned set.
fn vectors_required() -> bool {
    std::env::var_os("TINKER_VP8_VECTORS_REQUIRED").is_some_and(|value| value != "0")
}

#[test]
fn vp8_test_vectors_key_frames_match_their_md5s() {
    let required = vectors_required();
    let Some(dir) = std::env::var_os("TINKER_VP8_VECTORS") else {
        assert!(
            !required,
            "TINKER_VP8_VECTORS_REQUIRED is set and TINKER_VP8_VECTORS is not: \
             run tests/vp8-vectors/fetch.sh and point it at the result"
        );
        println!("{VECTORS_SKIPPED} (set TINKER_VP8_VECTORS to the fetched vectors)");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    assert!(
        !required || dir.is_absolute(),
        "TINKER_VP8_VECTORS must be absolute: a test runs from its crate directory"
    );
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("TINKER_VP8_VECTORS is a directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ivf"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no .ivf files in {}", dir.display());
    let (mut checked, mut files) = (0usize, 0usize);
    let mut failures = Vec::new();
    for path in &names {
        let file = std::fs::read(path).expect("reads");
        let mut sums = path.clone().into_os_string();
        sums.push(".md5");
        let sums = std::fs::read_to_string(&sums).expect("an .md5 beside each .ivf");
        let sums: Vec<&str> = sums
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        files += 1;
        for (shown, frame) in ivf_key_frames(&file) {
            let mut w = Warnings::default();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match decode(frame, &mut w, |_, _| Ok(())) {
                Ok(p) => {
                    let mut planes = p.y.clone();
                    planes.extend_from_slice(&p.u);
                    planes.extend_from_slice(&p.v);
                    let got = md5(&planes);
                    let want = sums.get(shown).copied().unwrap_or("");
                    if got != want || !p.complete {
                        failures.push(format!("{name} frame {shown}: {got} != {want}"));
                    }
                }
                Err(e) => failures.push(format!("{name} frame {shown}: {e}")),
            }
            checked += 1;
        }
    }
    println!(
        "{VECTORS_RAN} {checked} key frames from {files} files, {} failed ({})",
        failures.len(),
        dir.display()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if required {
        assert_eq!(
            (files, checked),
            (PINNED_FILES, PINNED_KEY_FRAMES),
            "not the pinned vector set: run tests/vp8-vectors/fetch.sh"
        );
    }
}

// --- the colour conversion, which the vectors do not reach ----------------------

/// BT.601's limited-range Y'CbCr to R'G'B' in exact integer arithmetic,
/// rounded half up and clamped to a byte: the Recommendation's `Kr = 0.299`
/// and `Kb = 0.114`, luma scaled from 219 levels to 255 and chroma from 224.
///
/// Every term is an integer over one common denominator, so nothing is
/// rounded before the last step.
fn bt601(y: u8, u: u8, v: u8) -> [i64; 3] {
    // Kr, Kb and Kg in thousandths. R = 255/219 Y + 255/112 (1 - Kr) Cr, and
    // so on, all over the denominator 219 * 112 * 1000 * Kg.
    let (kr, kb, kg) = (299i64, 114i64, 587i64);
    let d = 219 * 112 * 1000 * kg;
    let (y, cb, cr) = (i64::from(y) - 16, i64::from(u) - 128, i64::from(v) - 128);
    let luma = 255 * 112 * 1000 * kg * y;
    let r = luma + 255 * 219 * kg * (1000 - kr) * cr;
    let g = luma - 255 * 219 * ((1000 - kb) * kb * cb + (1000 - kr) * kr * cr);
    let b = luma + 255 * 219 * kg * (1000 - kb) * cb;
    // Half up: floor((2n + d) / 2d), `div_euclid` so the negative side floors.
    [r, g, b].map(|n| (2 * n + d).div_euclid(2 * d).clamp(0, 255))
}

/// `yuv_to_rgb` is BT.601 to within one level on every one of the 2^24
/// inputs, and exactly it on nearly all of them.
///
/// libwebp's 14-bit fixed point is what makes "within one" the claim rather
/// than "equal": each product is truncated before the sum. Measured when this
/// was written, 169 223 of the 50 331 648 channels miss by one and none by
/// more. What this pins is that the matrix is BT.601's, that the range is the
/// limited one and the rounding half up, and that the fixed point never
/// wanders further than that off the exact answer.
#[test]
fn the_colour_conversion_is_bt_601_to_within_one_level() {
    let (mut worst, mut off) = (0i64, 0u64);
    for y in 0..=255u8 {
        for u in 0..=255u8 {
            for v in 0..=255u8 {
                let got = yuv_to_rgb(y, u, v);
                let have = [(got >> 16) & 0xff, (got >> 8) & 0xff, got & 0xff].map(i64::from);
                for (h, w) in have.iter().zip(bt601(y, u, v)) {
                    let miss = (h - w).abs();
                    worst = worst.max(miss);
                    off += u64::from(miss != 0);
                }
            }
        }
    }
    assert!(worst <= 1, "{worst} levels off BT.601");
    // Nearly all: fewer than one channel in two hundred misses at all.
    assert!(
        off < 3 * (1 << 24) / 200,
        "{off} channels off BT.601 by one"
    );
}

/// The upsampler's weights, held to their definition: a pixel between two
/// chroma columns is 9:3:3:1 of the four samples around it, nearest first,
/// and a pixel in an edge column 3:1 of the two beside it. libwebp's integer
/// form rounds twice, so "within one" is the claim.
#[test]
fn the_chroma_upsampler_weighs_nine_three_three_one() {
    let samples: Vec<u8> = (0..=255u8).step_by(15).chain([1, 254]).collect();
    // `got` within one level of `sixteenths / 16`, exactly.
    let within_one =
        |got: u8, sixteenths: u32| (16 * i64::from(got) - i64::from(sixteenths)).abs() < 16;
    for &n0 in &samples {
        for &n1 in &samples {
            for &f0 in &samples {
                for &f1 in &samples {
                    let (left, right) = fancy_pair(n0, n1, f0, f1);
                    let [a, b, c, d] = [n0, n1, f0, f1].map(u32::from);
                    assert!(
                        within_one(left, 9 * a + 3 * b + 3 * c + d),
                        "left of {n0} {n1} over {f0} {f1} is {left}"
                    );
                    assert!(
                        within_one(right, 9 * b + 3 * a + 3 * d + c),
                        "right of {n0} {n1} over {f0} {f1} is {right}"
                    );
                }
            }
        }
        for &f in &samples {
            let got = 4 * i64::from(fancy_edge(n0, f));
            let exact = 3 * i64::from(n0) + i64::from(f);
            assert!((got - exact).abs() <= 2, "edge of {n0} over {f}");
        }
    }
    // And one sample everywhere is that sample everywhere: no weight lost.
    for &c in &samples {
        assert_eq!(fancy_pair(c, c, c, c), (c, c));
        assert_eq!(fancy_edge(c, c), c);
    }
}

/// Which chroma sample is *nearer*, worked out by hand on a 4 x 4 picture
/// whose U plane is 2 x 2: grey luma, neutral V, and U differing by row in one
/// picture and by column in the other.
///
/// 4:2:0 puts each chroma sample at the centre of a 2 x 2 block of luma, so an
/// inner output row or column is three quarters of the way from the chroma
/// row or column across from it to the one whose block it is in. Row 0 and
/// the last row of an even height have one chroma row beside them and take it
/// alone; so do the first and last columns.
#[test]
fn each_pixel_takes_its_chroma_nearest_first() {
    let picture = |u: [u8; 4]| Picture {
        width: 4,
        height: 4,
        y: vec![128; 16],
        u: u.to_vec(),
        v: vec![128; 4],
        complete: true,
    };
    // 64 and 192 alone at the edges, 3:1 and 1:3 of them inside — and the
    // 9:3:3:1 of an inner pixel is the same, since two of its four agree.
    let ramp = [64u8, 96, 160, 192];
    let at = |argb: &[u32], x: usize, row: usize| argb[row * 4 + x];

    // U by row: 64 above, 192 below.
    let argb = to_argb(&picture([64, 64, 192, 192]), None);
    for (row, &u) in ramp.iter().enumerate() {
        for x in 0..4 {
            let want = 0xff00_0000 | yuv_to_rgb(128, u, 128);
            assert_eq!(at(&argb, x, row), want, "({x}, {row}) is U {u}");
        }
    }
    // U by column: 64 left, 192 right.
    let argb = to_argb(&picture([64, 192, 64, 192]), None);
    for (x, &u) in ramp.iter().enumerate() {
        for row in 0..4 {
            let want = 0xff00_0000 | yuv_to_rgb(128, u, 128);
            assert_eq!(at(&argb, x, row), want, "({x}, {row}) is U {u}");
        }
    }
}
