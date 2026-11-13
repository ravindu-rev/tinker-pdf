//! Size-scaling complexity tests for the codecs
//! (`docs/design/resource-exhaustion.md`, "The size-scaling law").
//!
//! One test a codec. Each shape is an input this file writes at a size n and
//! at four times n — through the crate's own encoders where it has one, by
//! hand from the format's clauses where it has none — decoded through the
//! codec's counted door, which returns the steps the decode took: the
//! symbols, codes and fields it read, the rows it unfiltered, the reference
//! changes it examined and the bytes it returned. The law is the design's:
//! the work at 4 n is at most 1.1 × 4 times the work at n. A quadratic shape
//! costs sixteen times as much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: what comes
//! back is what was written, through the counted and the uncounted doors
//! alike. No clock is read here.
//!
//! The transform codecs' shapes need coders the crate has no encoder for,
//! and each says where its bytes come from: the baseline JPEG is
//! `jpeg_encode`'s; the progressive one is written here from T.81 G.1's scan
//! syntax under Huffman tables of a code or two; the arithmetic one repeats
//! one restart interval `qm.rs`'s test-only QM encoder wrote, committed in
//! `complexity/arithmetic.rs` and held to that encoder by `jpeg.rs`'s unit
//! test; the JPEG 2000 codestream's tiles are written here from T.800 Annex
//! A's markers and B.10's packet headers, around a code-block the crate's MQ
//! encoder codes from Annex D's contexts; both WebP bitstreams are written
//! here from RFC 9649 §3 and RFC 6386; and JPEG XR, which this repository
//! cannot encode, is read from the committed lossless pictures whose areas
//! are four times one another.

#[path = "complexity/arithmetic.rs"]
mod arithmetic;

use tinker_pdf_filters::{
    ascii85_decode, ascii85_decode_counted, ascii_hex_decode, ascii_hex_decode_counted, bmp_decode,
    bmp_decode_counted, brotli_decode, brotli_decode_counted, ccitt_decode, ccitt_decode_counted,
    ccitt_g4_encode, flate_decode, flate_decode_counted, gif_decode, gif_decode_counted,
    jbig2_decode, jbig2_decode_measured, jbig2_generic_region_segment, jpeg_decode,
    jpeg_decode_counted, jpeg_encode, jpx_decode, jpx_decode_counted, jxr_decode,
    jxr_decode_counted, lzw_decode, lzw_decode_counted, netpbm_decode, netpbm_decode_counted,
    png_decode, png_decode_counted, png_encode, predictor_decode, predictor_decode_counted,
    run_length_decode, run_length_decode_counted, tiff_decode, tiff_decode_counted, webp_decode,
    webp_decode_counted, zlib_compress, CcittParams, CcittSource, ImagePixels, Jbig2GenericSource,
    Jbig2Params, JpegColor, JpegOptions, JpegQuantisation, JpegSampling, JpegSource,
    JpegSourceColour, Limits, MqEncoder, PngColour, PngSource, PredictorParams,
};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// A ceiling no shape here reaches.
const ROOM: Limits = Limits::new(1 << 26);

/// The law, with both figures in the failure.
fn holds_the_law(shape: &str, n: u64, kn: u64) {
    println!("  {shape:28} work at n {n:>9}  at 4n {kn:>9}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}"
    );
}

/// `len` bytes of a text that compresses but not to nothing: words of a
/// small alphabet from a fixed linear congruential sequence.
fn text(len: usize) -> Vec<u8> {
    let mut state = 0x2545_F491u32;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let word = (state >> 16) % 7 + 1;
        for k in 0..word {
            out.push(b'a' + ((state >> (k * 3)) % 26) as u8);
        }
        out.push(b' ');
    }
    out.truncate(len);
    out
}

// ---------------------------------------------------------------------------
// Inflate.
// ---------------------------------------------------------------------------

/// `data` in stored DEFLATE blocks of `block` bytes, wrapped in zlib.
fn stored(data: &[u8], block: usize) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(block).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(index + 1 == chunks.len()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn inflated(stream: &[u8], want: &[u8]) -> u64 {
    let (decoded, steps) = flate_decode_counted(stream, &ROOM, None);
    let decoded = decoded.expect("the stream inflates");
    assert!(decoded.complete);
    assert_eq!(
        decoded.data, want,
        "the stream inflates to what was written"
    );
    assert_eq!(
        flate_decode(stream, &ROOM, None),
        Ok(decoded),
        "alike uncounted"
    );
    steps
}

#[test]
fn inflate_costs_work_linear_in_its_blocks_and_symbols() {
    let compressed = |n: usize| {
        let data = text(n);
        inflated(&zlib_compress(&data), &data)
    };
    holds_the_law(
        "inflate, compressed",
        compressed(16 << 10),
        compressed(64 << 10),
    );
    let blocks = |n: usize| {
        let data = text(n * 512);
        inflated(&stored(&data, 512), &data)
    };
    holds_the_law("inflate, stored blocks", blocks(32), blocks(128));
}

// ---------------------------------------------------------------------------
// LZW.
// ---------------------------------------------------------------------------

/// `data` as 7.4.4.2's codes, every byte a literal, nine bits each, with a
/// clear code every 200 codes so the width never grows.
fn lzw_literals(data: &[u8]) -> Vec<u8> {
    let mut codes = vec![256u16];
    for (index, &byte) in data.iter().enumerate() {
        if index > 0 && index % 200 == 0 {
            codes.push(256);
        }
        codes.push(u16::from(byte));
    }
    codes.push(257);
    let mut out = Vec::new();
    let (mut acc, mut used) = (0u32, 0u32);
    for code in codes {
        acc = (acc << 9) | u32::from(code);
        used += 9;
        while used >= 8 {
            used -= 8;
            out.push((acc >> used) as u8);
        }
    }
    if used > 0 {
        out.push((acc << (8 - used)) as u8);
    }
    out
}

#[test]
fn lzw_costs_work_linear_in_its_codes() {
    let at = |n: usize| {
        let data = text(n);
        let stream = lzw_literals(&data);
        let (decoded, steps) = lzw_decode_counted(&stream, &ROOM, true, None);
        let decoded = decoded.expect("the stream decodes");
        assert_eq!(decoded.data, data, "every literal comes back");
        assert_eq!(lzw_decode(&stream, &ROOM, true, None), Ok(decoded));
        steps
    };
    holds_the_law("LZW", at(8 << 10), at(32 << 10));
}

// ---------------------------------------------------------------------------
// ASCII85, ASCIIHex and RunLength.
// ---------------------------------------------------------------------------

#[test]
fn ascii_hex_costs_work_linear_in_its_digits() {
    let at = |n: usize| {
        let data = text(n);
        let mut stream: Vec<u8> = data
            .iter()
            .flat_map(|b| format!("{b:02X}").into_bytes())
            .collect();
        stream.push(b'>');
        let (decoded, steps) = ascii_hex_decode_counted(&stream, &ROOM);
        assert_eq!(decoded.data, data);
        assert_eq!(ascii_hex_decode(&stream, &ROOM), decoded, "alike uncounted");
        steps
    };
    holds_the_law("ASCIIHex", at(8 << 10), at(32 << 10));
}

/// `data` in 7.4.3's base-85, four bytes to five characters.
fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for group in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..group.len()].copy_from_slice(group);
        let mut value = u32::from_be_bytes(word);
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..group.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

#[test]
fn ascii85_costs_work_linear_in_its_groups() {
    let at = |n: usize| {
        let data = text(n);
        let stream = ascii85(&data);
        let (decoded, steps) = ascii85_decode_counted(&stream, &ROOM);
        assert_eq!(decoded.data, data);
        assert_eq!(ascii85_decode(&stream, &ROOM), decoded, "alike uncounted");
        steps
    };
    holds_the_law("ASCII85", at(8 << 10), at(32 << 10));
}

#[test]
fn run_length_costs_work_linear_in_its_runs() {
    let at = |n: usize| {
        let data = text(n * 128);
        let mut stream = Vec::new();
        for (index, run) in data.chunks(128).enumerate() {
            if index % 2 == 0 {
                stream.push((run.len() - 1) as u8);
                stream.extend_from_slice(run);
            } else {
                // A repeat of the run's first byte, the other kind of run.
                stream.push((257 - run.len()) as u8);
                stream.push(run[0]);
            }
        }
        stream.push(128);
        let want: Vec<u8> = data
            .chunks(128)
            .enumerate()
            .flat_map(|(index, run)| {
                if index % 2 == 0 {
                    run.to_vec()
                } else {
                    vec![run[0]; run.len()]
                }
            })
            .collect();
        let (decoded, steps) = run_length_decode_counted(&stream, &ROOM);
        assert_eq!(decoded.data, want);
        assert_eq!(
            run_length_decode(&stream, &ROOM),
            decoded,
            "alike uncounted"
        );
        steps
    };
    holds_the_law("RunLength", at(64), at(256));
}

// ---------------------------------------------------------------------------
// The predictors.
// ---------------------------------------------------------------------------

#[test]
fn the_predictors_cost_work_linear_in_their_rows() {
    let columns = 64u32;
    let png = |rows: usize| {
        let params = PredictorParams {
            predictor: 15,
            colors: 3,
            bits_per_component: 8,
            columns,
        };
        let row_bytes = 3 * columns as usize;
        let mut stream = Vec::new();
        for row in 0..rows {
            // Every filter type in turn, over a row of the text.
            stream.push((row % 5) as u8);
            stream.extend_from_slice(&text(row_bytes));
        }
        let (decoded, steps) = predictor_decode_counted(&stream, &params, &ROOM);
        let decoded = decoded.expect("the rows unfilter");
        assert_eq!(decoded.data.len(), rows * row_bytes);
        assert_eq!(predictor_decode(&stream, &params, &ROOM), Ok(decoded));
        steps
    };
    holds_the_law("PNG predictors", png(100), png(400));
    let tiff = |rows: usize| {
        let params = PredictorParams {
            predictor: 2,
            colors: 3,
            bits_per_component: 8,
            columns,
        };
        let stream = text(rows * 3 * columns as usize);
        let (decoded, steps) = predictor_decode_counted(&stream, &params, &ROOM);
        let decoded = decoded.expect("the rows unpredict");
        assert_eq!(decoded.data.len(), stream.len());
        assert_eq!(predictor_decode(&stream, &params, &ROOM), Ok(decoded));
        steps
    };
    holds_the_law("TIFF predictor 2", tiff(100), tiff(400));
}

// ---------------------------------------------------------------------------
// Brotli.
// ---------------------------------------------------------------------------

/// An LSB-first bit writer.
struct Bits {
    out: Vec<u8>,
    used: u32,
}

impl Bits {
    fn put(&mut self, value: u32, width: u32) {
        for bit in 0..width {
            if self.used == 0 {
                self.out.push(0);
            }
            if (value >> bit) & 1 == 1 {
                if let Some(last) = self.out.last_mut() {
                    *last |= 1 << self.used;
                }
            }
            self.used = (self.used + 1) % 8;
        }
    }

    fn align(&mut self) {
        while self.used != 0 {
            self.put(0, 1);
        }
    }
}

/// `data` as a brotli stream of uncompressed meta-blocks of `block` bytes
/// (RFC 7932 §9.2), under a window of 2¹⁶ − 16, closed by an empty last one.
fn brotli_stored(data: &[u8], block: usize) -> Vec<u8> {
    let mut bits = Bits {
        out: Vec::new(),
        used: 0,
    };
    bits.put(0, 1); // WBITS 16
    for chunk in data.chunks(block) {
        bits.put(0, 1); // ISLAST
        bits.put(0, 2); // MNIBBLES: four
        bits.put(chunk.len() as u32 - 1, 16);
        bits.put(1, 1); // ISUNCOMPRESSED
        bits.align();
        bits.out.extend_from_slice(chunk);
    }
    bits.put(1, 1); // ISLAST
    bits.put(1, 1); // ISLASTEMPTY
    bits.align();
    bits.out
}

#[test]
fn brotli_costs_work_linear_in_its_meta_blocks() {
    let at = |n: usize| {
        let data = text(n * 256);
        let stream = brotli_stored(&data, 256);
        let (decoded, steps) = brotli_decode_counted(&stream, &ROOM);
        let decoded = decoded.expect("the stream decodes");
        assert_eq!(decoded, data);
        assert_eq!(
            brotli_decode(&stream, &ROOM),
            Ok(decoded),
            "alike uncounted"
        );
        steps
    };
    holds_the_law("brotli", at(32), at(128));
}

// ---------------------------------------------------------------------------
// CCITT.
// ---------------------------------------------------------------------------

/// A G4 page of `rows` rows of `columns` pixels, from `pixel(x, y)`, one for
/// black, and what decoding it gives back.
fn g4(columns: u32, rows: u32, pixel: impl Fn(u32, u32) -> bool) -> (Vec<u8>, Vec<u8>) {
    let stride = columns.div_ceil(8) as usize;
    let mut raster = vec![0xFFu8; stride * rows as usize];
    for y in 0..rows {
        for x in 0..columns {
            if pixel(x, y) {
                // 0 is black, 7.4.6 Table 11's default.
                raster[y as usize * stride + x as usize / 8] &= !(0x80 >> (x % 8));
            }
        }
    }
    let coded = ccitt_g4_encode(&CcittSource {
        columns,
        rows,
        black_is_1: false,
        stride,
        end_of_block: true,
        data: &raster,
    })
    .expect("a G4 page");
    (coded, raster)
}

fn g4_params(columns: u32, rows: u32) -> CcittParams {
    CcittParams {
        k: -1,
        columns,
        rows,
        black_is_1: false,
        byte_align: false,
        end_of_line: false,
        end_of_block: true,
    }
}

fn ccitt_steps(coded: &[u8], raster: &[u8], params: &CcittParams) -> u64 {
    let ((packed, _), steps) = ccitt_decode_counted(coded, params, 1 << 26);
    assert_eq!(packed, raster, "the page comes back");
    assert_eq!(
        ccitt_decode(coded, params, 1 << 26).0,
        packed,
        "alike uncounted"
    );
    steps
}

/// A G3 page of `rows` all-white rows of eight pixels, each row one
/// terminating code for a white run of eight (T.4 Table 2: `10011`).
fn g3_white(rows: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut used) = (0u32, 0u32);
    for _ in 0..rows {
        acc = (acc << 5) | 0b10011;
        used += 5;
        while used >= 8 {
            used -= 8;
            out.push((acc >> used) as u8);
        }
    }
    if used > 0 {
        out.push((acc << (8 - used)) as u8);
    }
    out
}

#[test]
fn ccitt_costs_work_linear_in_its_rows_and_its_changes() {
    // A page whose rows each carry the same diagonal hatching.
    let rows = |n: u32| {
        let (coded, raster) = g4(64, n, |x, y| (x + y) % 8 < 2);
        ccitt_steps(&coded, &raster, &g4_params(64, n))
    };
    holds_the_law("CCITT G4 rows", rows(100), rows(400));

    // One row of `n` black pixels a pixel apart, below a row of the same, so
    // every code is read against a reference line as long as the row.
    let changes = |n: u32| {
        let (coded, raster) = g4(2 * n, 2, |x, _| x % 2 == 0);
        ccitt_steps(&coded, &raster, &g4_params(2 * n, 2))
    };
    holds_the_law("CCITT G4 changes a row", changes(500), changes(2_000));

    let g3 = |n: u32| {
        let params = CcittParams {
            k: 0,
            columns: 8,
            rows: n,
            black_is_1: false,
            byte_align: false,
            end_of_line: false,
            end_of_block: false,
        };
        ccitt_steps(&g3_white(n), &vec![0xFF; n as usize], &params)
    };
    holds_the_law("CCITT G3 rows", g3(100), g3(400));
}

// ---------------------------------------------------------------------------
// JBIG2.
// ---------------------------------------------------------------------------

/// A generic region of `width` by `height` pixels, a pattern of `pixel(x, y)`
/// with one for black, in the embedded organisation (Annex D.3): a page
/// information segment, the region and an end of page.
fn jbig2(width: u32, height: u32, pixel: impl Fn(u32, u32) -> bool) -> (Vec<u8>, Vec<u8>) {
    let stride = width.div_ceil(8) as usize;
    let mut raster = vec![0u8; stride * height as usize];
    for y in 0..height {
        for x in 0..width {
            if pixel(x, y) {
                raster[y as usize * stride + x as usize / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    let region = jbig2_generic_region_segment(
        &Jbig2GenericSource {
            width,
            height,
            template: 0,
            tpgdon: true,
            at: [(3, -1), (-3, -1), (2, -2), (-2, -2)],
            stride,
            data: &raster,
        },
        0,
        0,
        0,
    )
    .expect("a generic region");
    let segment = |number: u32, kind: u8, data: &[u8]| -> Vec<u8> {
        let mut out = number.to_be_bytes().to_vec();
        out.push(kind & 0x3F);
        out.push(0);
        out.push(1);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    };
    let mut page = width.to_be_bytes().to_vec();
    page.extend_from_slice(&height.to_be_bytes());
    page.extend_from_slice(&[0; 8]);
    page.push(0x01);
    page.extend_from_slice(&[0, 0]);
    let mut out = segment(0, 48, &page);
    out.extend_from_slice(&segment(1, 39, &region));
    out.extend_from_slice(&segment(2, 49, &[]));
    (out, raster)
}

#[test]
fn jbig2_costs_work_linear_in_its_rows() {
    let at = |height: u32| {
        let (stream, raster) = jbig2(64, height, |x, y| (x * 7 + y * 3) % 11 < 4);
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height,
            work_ceiling: None,
        };
        let (page, extent) = jbig2_decode_measured(&stream, &params, 1 << 26, &mut Vec::new());
        let page = page.expect("the region decodes");
        assert_eq!(page, raster, "the region comes back");
        assert_eq!(
            jbig2_decode(&stream, &params, 1 << 26, &mut Vec::new()),
            Ok(page),
            "alike unmeasured"
        );
        extent.work
    };
    holds_the_law("JBIG2 generic region", at(100), at(400));
}

// ---------------------------------------------------------------------------
// PNG.
// ---------------------------------------------------------------------------

#[test]
fn png_costs_work_linear_in_its_rows() {
    let at = |height: u32| {
        let width = 64u32;
        let pixels = text((width * height * 3) as usize);
        let file = png_encode(&PngSource {
            width,
            height,
            colour: PngColour::Rgb,
            stride: (width * 3) as usize,
            data: &pixels,
        })
        .expect("a PNG");
        let (image, steps) = png_decode_counted(&file, &ROOM);
        let image = image.expect("the file decodes");
        assert_eq!(image.data, pixels, "every pixel comes back");
        assert_eq!(png_decode(&file, &ROOM).map(|i| i.data), Ok(image.data));
        steps
    };
    holds_the_law("PNG", at(100), at(400));
}

// ---------------------------------------------------------------------------
// TIFF.
// ---------------------------------------------------------------------------

/// An uncompressed little-endian TIFF of eight-bit grey, `width` by
/// `strips` rows, one row a strip.
fn tiff(width: u32, strips: u32, pixels: &[u8]) -> Vec<u8> {
    let entries: u16 = 9;
    let ifd = 8u32;
    let ifd_len = 2 + 12 * u32::from(entries) + 4;
    let offsets_at = ifd + ifd_len;
    let counts_at = offsets_at + 4 * strips;
    let data_at = counts_at + 4 * strips;
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&ifd.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    };
    // SHORT is 3, LONG is 4.
    entry(256, 4, 1, width);
    entry(257, 4, 1, strips);
    entry(258, 3, 1, 8);
    entry(259, 3, 1, 1);
    entry(262, 3, 1, 1);
    entry(273, 4, strips, offsets_at);
    entry(277, 3, 1, 1);
    entry(278, 4, 1, 1);
    entry(279, 4, strips, counts_at);
    out.extend_from_slice(&0u32.to_le_bytes());
    for strip in 0..strips {
        out.extend_from_slice(&(data_at + strip * width).to_le_bytes());
    }
    for _ in 0..strips {
        out.extend_from_slice(&width.to_le_bytes());
    }
    out.extend_from_slice(pixels);
    out
}

#[test]
fn tiff_costs_work_linear_in_its_strips() {
    let at = |strips: u32| {
        let pixels = text((64 * strips) as usize);
        let file = tiff(64, strips, &pixels);
        let (image, steps) = tiff_decode_counted(&file, &ROOM);
        let image = image.expect("the file decodes");
        assert_eq!(image.data, pixels, "every strip comes back");
        assert_eq!(tiff_decode(&file, &ROOM).map(|i| i.data), Ok(image.data));
        steps
    };
    holds_the_law("TIFF", at(100), at(400));
}

// ---------------------------------------------------------------------------
// GIF.
// ---------------------------------------------------------------------------

/// A GIF89a of `width` by `height` pixels of a 128-colour table, `indices`,
/// every pixel an LZW literal of eight bits with a clear code every hundred.
fn gif(width: u16, height: u16, indices: &[u8]) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    // A global table of 2⁷ entries.
    out.extend_from_slice(&[0x80 | 0x06, 0, 0]);
    for i in 0..128u32 {
        out.extend_from_slice(&[(i * 2) as u8, i as u8, (255 - i) as u8]);
    }
    out.push(0x2C);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.push(0);
    out.push(7); // the minimum code size: clear is 128, end 129
    let mut codes = vec![128u16];
    for (index, &pixel) in indices.iter().enumerate() {
        if index > 0 && index % 100 == 0 {
            codes.push(128);
        }
        codes.push(u16::from(pixel));
    }
    codes.push(129);
    let mut bits = Bits {
        out: Vec::new(),
        used: 0,
    };
    for code in codes {
        bits.put(u32::from(code), 8);
    }
    for block in bits.out.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(0);
    out.push(0x3B);
    out
}

#[test]
fn gif_costs_work_linear_in_its_rows() {
    let at = |height: u16| {
        let indices: Vec<u8> = text(64 * height as usize).iter().map(|b| b % 128).collect();
        let file = gif(64, height, &indices);
        let (image, steps) = gif_decode_counted(&file, &ROOM);
        let image = image.expect("the file decodes");
        let tinker_pdf_filters::ImagePixels::Indexed { indices: got, .. } = &image.pixels else {
            panic!("a GIF without transparency decodes to indices");
        };
        assert_eq!(got, &indices, "every pixel comes back");
        assert_eq!(gif_decode(&file, &ROOM).map(|i| i.pixels), Ok(image.pixels));
        steps
    };
    holds_the_law("GIF", at(100), at(400));
}

// ---------------------------------------------------------------------------
// BMP.
// ---------------------------------------------------------------------------

/// A 24-bit bottom-up BMP of `width` by `height` pixels, rows as given
/// bottom row first, each row a multiple of four bytes.
fn bmp(width: u32, height: u32, rows: &[u8]) -> Vec<u8> {
    let data_at = 14 + 40;
    let mut out = b"BM".to_vec();
    out.extend_from_slice(&((data_at + rows.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(data_at as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(rows);
    out
}

#[test]
fn bmp_costs_work_linear_in_its_rows() {
    let at = |height: u32| {
        let rows = text((64 * 3 * height) as usize);
        let file = bmp(64, height, &rows);
        let (image, steps) = bmp_decode_counted(&file, &ROOM);
        let image = image.expect("the file decodes");
        assert_eq!(image.height, height);
        // The bottom row of the file is the last row of the picture, and its
        // first pixel's blue, green and red are read back as red, green, blue.
        let data = image.pixels.data();
        let last = (height as usize - 1) * 64 * 3;
        assert_eq!(&data[last..last + 3], &[rows[2], rows[1], rows[0]]);
        assert_eq!(bmp_decode(&file, &ROOM).map(|i| i.pixels), Ok(image.pixels));
        steps
    };
    holds_the_law("BMP", at(100), at(400));
}

// ---------------------------------------------------------------------------
// Netpbm.
// ---------------------------------------------------------------------------

/// A raw PPM 64 pixels wide, and a plain PGM of the same samples' first
/// third, each written from its format description: a header, then the
/// raster.
fn ppm(height: u32, raster: &[u8]) -> Vec<u8> {
    let mut out = format!("P6\n64 {height}\n255\n").into_bytes();
    out.extend_from_slice(raster);
    out
}

fn plain_pgm(height: u32, samples: &[u8]) -> Vec<u8> {
    let mut out = format!("P2\n64 {height}\n255\n").into_bytes();
    for (i, sample) in samples.iter().enumerate() {
        out.extend_from_slice(sample.to_string().as_bytes());
        out.push(if i % 16 == 15 { b'\n' } else { b' ' });
    }
    out
}

#[test]
fn netpbm_costs_work_linear_in_its_rows() {
    let raw = |height: u32| {
        let raster = text((64 * 3 * height) as usize);
        let file = ppm(height, &raster);
        let (image, steps) = netpbm_decode_counted(&file, &ROOM);
        let image = image.expect("the file decodes");
        assert_eq!(image.height, height);
        // Under a maxval of 255 the samples come back as they were written.
        assert_eq!(image.data, raster);
        assert_eq!(netpbm_decode(&file, &ROOM), Ok(image));
        steps
    };
    holds_the_law("Netpbm raw", raw(100), raw(400));

    let plain = |height: u32| {
        let samples = text((64 * height) as usize);
        let file = plain_pgm(height, &samples);
        let (image, steps) = netpbm_decode_counted(&file, &ROOM);
        let image = image.expect("the file decodes");
        assert_eq!(image.data, samples);
        assert_eq!(netpbm_decode(&file, &ROOM), Ok(image));
        steps
    };
    holds_the_law("Netpbm plain", plain(100), plain(400));
}

// ---------------------------------------------------------------------------
// JPEG: baseline, progressive and arithmetic.
// ---------------------------------------------------------------------------

/// A marker segment: `X'FF'`, `code`, a length that counts itself, `body`.
fn segment(out: &mut Vec<u8>, code: u8, body: &[u8]) {
    out.extend_from_slice(&[0xFF, code]);
    let length = u16::try_from(body.len() + 2).expect("a short segment");
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
}

/// An MSB-first writer of entropy-coded data: a zero stuffed after every
/// `X'FF'` (T.81 F.1.2.3) and the last byte padded with ones (F.1.2.3's
/// "1-bits" before a marker).
struct ScanBits {
    out: Vec<u8>,
    acc: u32,
    used: u32,
}

impl ScanBits {
    fn new() -> ScanBits {
        ScanBits {
            out: Vec::new(),
            acc: 0,
            used: 0,
        }
    }

    fn put(&mut self, value: u32, width: u32) {
        for bit in (0..width).rev() {
            self.acc = (self.acc << 1) | ((value >> bit) & 1);
            self.used += 1;
            if self.used == 8 {
                let byte = self.acc as u8;
                self.out.push(byte);
                if byte == 0xFF {
                    self.out.push(0x00);
                }
                self.acc = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        while self.used != 0 {
            self.put(1, 1);
        }
        self.out
    }
}

/// What a JPEG decodes to through the counted door, held to the uncounted
/// one, and the steps it took.
fn jpeg_decoded(file: &[u8]) -> (tinker_pdf_filters::JpegImage, u64) {
    let (image, steps) = jpeg_decode_counted(file, 1 << 26);
    let image = image.expect("the frame decodes");
    let plain = jpeg_decode(file, 1 << 26).expect("alike uncounted");
    assert_eq!(
        (plain.width, plain.height, &plain.data, &plain.warnings),
        (image.width, image.height, &image.data, &image.warnings),
        "alike uncounted"
    );
    assert_eq!(
        image.warnings,
        Vec::new(),
        "a clean frame warns about nothing"
    );
    (image, steps)
}

/// `n` MCUs of a baseline 4:2:0 frame side by side, each the same 16 × 16
/// tile of a pattern, from `jpeg_encode`.
fn baseline(n: u32) -> Vec<u8> {
    let (width, height) = (16 * n, 16u32);
    let mut raster = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x % 16, y);
            raster.extend_from_slice(&[(u * 16) as u8, (v * 16) as u8, ((u + v) * 8) as u8]);
        }
    }
    jpeg_encode(
        &JpegSource {
            width,
            height,
            colour: JpegSourceColour::Rgb,
            stride: (width * 3) as usize,
            data: &raster,
        },
        &JpegOptions {
            quantisation: JpegQuantisation::AnnexK,
            sampling: JpegSampling::FourTwoZero,
            restart_interval: 0,
        },
    )
    .expect("a baseline frame")
}

/// A progressive grey frame of `n` blocks in a row, every block the same flat
/// one, in G.1.1.1.1's four scans: the DC coefficients at `Al = 1`, their
/// last bit, the AC band at `Al = 1`, and its refinement, each band ending
/// at once.
///
/// The tables are a code or two each, which C.2 permits: DC category 0 is
/// `00` and category 3 is `01`; the AC end of band is `0`. The first block's
/// DC difference is 4 at `Al = 1`, so every block's coefficient is 8, and the
/// quantiser of 16 makes it 128 — sixteen levels above mid-grey.
fn progressive(n: u16) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    let mut dqt = vec![0x00];
    dqt.extend_from_slice(&[16; 64]);
    segment(&mut out, 0xDB, &dqt);
    let mut sof = vec![8, 0, 8];
    sof.extend_from_slice(&(8 * n).to_be_bytes());
    sof.extend_from_slice(&[1, 1, 0x11, 0]);
    segment(&mut out, 0xC2, &sof);
    let mut dc = vec![0x00, 0, 2];
    dc.extend_from_slice(&[0; 14]);
    dc.extend_from_slice(&[0, 3]);
    segment(&mut out, 0xC4, &dc);
    let mut ac = vec![0x10, 1];
    ac.extend_from_slice(&[0; 15]);
    ac.push(0x00);
    segment(&mut out, 0xC4, &ac);

    let mut scan = |ss: u8, se: u8, a: u8, code: &dyn Fn(&mut ScanBits, u16)| {
        segment(&mut out, 0xDA, &[1, 1, 0x00, ss, se, a]);
        let mut bits = ScanBits::new();
        for block in 0..n {
            code(&mut bits, block);
        }
        out.extend_from_slice(&bits.finish());
    };
    // DC, first: category 3 and the value 4 for the first block, a
    // difference of zero for each after it.
    scan(0, 0, 0x01, &|bits, block| {
        if block == 0 {
            bits.put(0b01, 2);
            bits.put(0b100, 3);
        } else {
            bits.put(0b00, 2);
        }
    });
    // DC, refined: the bit below, zero.
    scan(0, 0, 0x10, &|bits, _| bits.put(0, 1));
    // The AC band, first and refined: an end of band each block.
    scan(1, 63, 0x01, &|bits, _| bits.put(0, 1));
    scan(1, 63, 0x10, &|bits, _| bits.put(0, 1));
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// An arithmetic grey frame of `n` blocks in a row with a restart interval
/// of one MCU, every interval the committed flat block, between `RST0` to
/// `RST7` in turn (T.81 B.2.1).
fn arithmetic_frame(n: u16) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    let mut dqt = vec![0x00];
    dqt.extend_from_slice(&[1; 64]);
    segment(&mut out, 0xDB, &dqt);
    segment(&mut out, 0xDD, &1u16.to_be_bytes());
    let mut sof = vec![8, 0, 8];
    sof.extend_from_slice(&(8 * n).to_be_bytes());
    sof.extend_from_slice(&[1, 1, 0x11, 0]);
    segment(&mut out, 0xC9, &sof);
    segment(&mut out, 0xDA, &[1, 1, 0x00, 0, 63, 0]);
    for block in 0..n {
        if block > 0 {
            out.extend_from_slice(&[0xFF, 0xD0 + ((block - 1) % 8) as u8]);
        }
        out.extend_from_slice(arithmetic::FLAT_INTERVAL);
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[test]
fn jpeg_costs_work_linear_in_its_mcus_and_restart_intervals() {
    let baseline_at = |n: u32| {
        let (image, steps) = jpeg_decoded(&baseline(n));
        assert_eq!((image.width, image.height), (16 * n, 16));
        assert_eq!(image.color, JpegColor::Rgb);
        // Every MCU coded the same coefficients, so every MCU is the first.
        let row = (16 * n * 3) as usize;
        for y in 0..16usize {
            let line = &image.data[y * row..(y + 1) * row];
            for (x, pixel) in line.chunks_exact(3).enumerate() {
                let first = &line[(x % 16) * 3..(x % 16) * 3 + 3];
                assert_eq!(pixel, first, "pixel ({x}, {y}) is not its MCU's");
            }
        }
        steps
    };
    holds_the_law("JPEG baseline MCUs", baseline_at(64), baseline_at(256));

    let progressive_at = |n: u16| {
        let (image, steps) = jpeg_decoded(&progressive(n));
        assert_eq!((image.width, image.height), (8 * u32::from(n), 8));
        let level = image.data[0];
        assert_ne!(level, 128, "the four scans placed no coefficient");
        assert!(
            image.data.iter().all(|&s| s == level),
            "every block is the one flat block"
        );
        steps
    };
    holds_the_law(
        "JPEG progressive MCUs",
        progressive_at(64),
        progressive_at(256),
    );

    let arithmetic_at = |n: u16| {
        let (image, steps) = jpeg_decoded(&arithmetic_frame(n));
        assert_eq!((image.width, image.height), (8 * u32::from(n), 8));
        assert!(
            image.data.iter().all(|&s| s == 128),
            "a zero DC difference after every restart is mid-grey"
        );
        steps
    };
    holds_the_law(
        "JPEG arithmetic intervals",
        arithmetic_at(64),
        arithmetic_at(256),
    );
}

// ---------------------------------------------------------------------------
// JPEG 2000.
// ---------------------------------------------------------------------------

/// A packet header's bits, most significant first, packed into bytes: the
/// byte after an `X'FF'` carries seven, its first a stuffed zero (T.800
/// B.10.1), and the last is filled out with zeros.
fn packet_header(bits: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bits.len() {
        let width = if out.last() == Some(&0xFF) { 7 } else { 8 };
        let mut byte = 0u8;
        for k in 0..width {
            byte = (byte << 1) | bits.get(at + k).copied().unwrap_or(0);
        }
        at += width;
        out.push(byte);
    }
    out
}

/// The data of one tile of [`jpx_tiles`]: its two packets.
///
/// The tile is 2 × 2 samples under one level of the reversible 5/3 (T.800
/// Annex F), so its LL, HL, LH and HH bands are one coefficient each. The LL
/// band's coefficient is 1 and the others are 0, which F.3.7's synthesis
/// turns into four samples of 1 and G.1's level shift into four of 129.
///
/// The LL code-block is coded in one cleanup pass (D.2.4) on its only
/// bit-plane: the coefficient becomes significant in the all-insignificant
/// zero-coding context, context 0 of Table D.1, which Table D.7 starts at
/// state 4, and its sign is coded 0 in Table D.3's context 9, whose XOR bit
/// is 0. The crate's MQ encoder codes the two decisions; T.88's flush ends
/// with the `X'FF' X'AC'` JBIG2's segments close with, which T.800 C.2.9 does
/// not write, so it is taken off.
///
/// The first packet's header (B.10) says: not empty; the block included in
/// layer 0, one bit of its tag tree; eight missing bit-planes, eight zeros
/// and a one, since QCD's exponent of 8 and two guard bits give the LL band
/// `Mb = 9` (E-2) and the coefficient is one plane deep; one coding pass,
/// `0` (Table B.4); no change to `Lblock`, `0`; and the segment's length in
/// `Lblock = 3` bits (B.10.7.1). The second packet, the high-pass bands', is
/// empty: one zero bit.
fn jpx_tile_data() -> Vec<u8> {
    let mut mq = MqEncoder::new(19);
    mq.set_state(0, 4, 0);
    mq.set_state(17, 3, 0);
    mq.set_state(18, 46, 0);
    mq.encode_at(0, 1);
    mq.encode_at(9, 0);
    let mut segment = mq.flush();
    assert!(
        segment.ends_with(&[0xFF, 0xAC]),
        "T.88's flush ends every segment it writes this way"
    );
    segment.truncate(segment.len() - 2);
    let length = u8::try_from(segment.len()).expect("a short segment");
    assert!(length <= 7, "the length fits Lblock's three bits");

    let mut bits = vec![1, 1];
    bits.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 1]);
    bits.extend_from_slice(&[0, 0]);
    bits.extend((0..3).rev().map(|k| (length >> k) & 1));
    let mut data = packet_header(&bits);
    data.extend_from_slice(&segment);
    data.push(0x00);
    data
}

/// A bare JPEG 2000 codestream of `cols` by `rows` tiles of 2 × 2 samples,
/// one component of eight bits, every tile [`jpx_tile_data`]: SIZ, COD and
/// QCD as T.800 A.5 and A.6 lay them out, then one tile-part a tile.
fn jpx_tiles(cols: u32, rows: u32) -> Vec<u8> {
    let mut out = vec![0xFF, 0x4F];
    // SIZ: Lsiz = 41 for one component, Rsiz = 0, the image and tile grids,
    // then Ssiz = 7 (eight bits, unsigned) with no subsampling.
    out.extend_from_slice(&[0xFF, 0x51, 0x00, 41, 0x00, 0x00]);
    for value in [2 * cols, 2 * rows, 0, 0, 2, 2, 0, 0] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&[0x00, 0x01, 0x07, 0x01, 0x01]);
    // COD: no precincts, SOP or EPH; LRCP; one layer; no component
    // transform; one decomposition level; 64 × 64 code-blocks; no style
    // bits; the 5/3.
    out.extend_from_slice(&[
        0xFF, 0x52, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x04, 0x00, 0x01,
    ]);
    // QCD: two guard bits, no quantisation, exponents 8, 9, 9, 10.
    out.extend_from_slice(&[0xFF, 0x5C, 0x00, 0x07, 0x40, 0x40, 0x48, 0x48, 0x50]);
    let data = jpx_tile_data();
    let psot = u32::try_from(12 + 2 + data.len()).expect("a short tile-part");
    for tile in 0..cols * rows {
        out.extend_from_slice(&[0xFF, 0x90, 0x00, 0x0A]);
        out.extend_from_slice(&u16::try_from(tile).expect("a tile index").to_be_bytes());
        out.extend_from_slice(&psot.to_be_bytes());
        out.extend_from_slice(&[0x00, 0x01, 0xFF, 0x93]);
        out.extend_from_slice(&data);
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[test]
fn jpx_costs_work_linear_in_its_tiles() {
    let at = |cols: u32, rows: u32| {
        let stream = jpx_tiles(cols, rows);
        let mut warnings = Vec::new();
        let (image, steps) = jpx_decode_counted(&stream, &ROOM, &mut warnings);
        let image = image.expect("the codestream decodes");
        assert_eq!(
            warnings,
            Vec::new(),
            "a clean codestream warns about nothing"
        );
        assert_eq!((image.width, image.height), (2 * cols, 2 * rows));
        assert_eq!((image.components, image.precision), (1, 8));
        assert!(!image.truncated);
        assert!(
            image.samples.iter().all(|&s| s == 129),
            "every tile is its LL coefficient of 1, level-shifted"
        );
        let mut again = Vec::new();
        assert_eq!(
            jpx_decode(&stream, &ROOM, &mut again),
            Ok(image),
            "alike uncounted"
        );
        assert_eq!(again, warnings);
        steps
    };
    holds_the_law("JPX tiles", at(10, 10), at(20, 20));
}

// ---------------------------------------------------------------------------
// WebP: lossless and lossy.
// ---------------------------------------------------------------------------

/// A RIFF container of one `kind` chunk (RFC 9649 §2.3, §2.5, §2.6).
fn riff(kind: &[u8; 4], chunk: &[u8]) -> Vec<u8> {
    let pad = chunk.len() % 2;
    let mut out = b"RIFF".to_vec();
    let size = u32::try_from(4 + 8 + chunk.len() + pad).expect("a short file");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(kind);
    let length = u32::try_from(chunk.len()).expect("a short chunk");
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(chunk);
    if pad == 1 {
        out.push(0);
    }
    out
}

/// The red, the blue and the two greens of [`webp_lossless`]'s pixels.
const WEBP_RED: u8 = 200;
const WEBP_BLUE: u8 = 40;
const WEBP_GREENS: [u8; 2] = [10, 240];

/// A lossless WebP of `width` by `height` pixels, each one bit of the image
/// data: RFC 9649 §3.4's header, no transform, no colour cache and one
/// prefix code group (§3.7.2) of five simple codes (§3.7.2.1.1) — green of
/// two symbols, each a code of one bit, the smaller `0`; red, blue and alpha
/// of one symbol each, which take no bits; distance of one. A pixel `i` is
/// green `WEBP_GREENS[green(i)]`.
fn webp_lossless(width: u32, height: u32, green: impl Fn(u32) -> usize) -> Vec<u8> {
    let mut bits = Bits {
        out: vec![0x2f],
        used: 0,
    };
    bits.put(width - 1, 14);
    bits.put(height - 1, 14);
    bits.put(0, 1); // alpha_is_used
    bits.put(0, 3); // version
    bits.put(0, 1); // no transform
    bits.put(0, 1); // no colour cache
    bits.put(0, 1); // no meta prefix codes
                    // Green: simple, two symbols, the first of eight bits.
    bits.put(1, 1);
    bits.put(1, 1);
    bits.put(1, 1);
    bits.put(u32::from(WEBP_GREENS[0]), 8);
    bits.put(u32::from(WEBP_GREENS[1]), 8);
    // Red, blue and alpha: simple, one symbol of eight bits.
    for value in [WEBP_RED, WEBP_BLUE, 255] {
        bits.put(1, 1);
        bits.put(0, 1);
        bits.put(1, 1);
        bits.put(u32::from(value), 8);
    }
    // Distance: simple, one symbol of one bit.
    bits.put(1, 1);
    bits.put(0, 1);
    bits.put(0, 1);
    bits.put(0, 1);
    for i in 0..width * height {
        bits.put(green(i) as u32, 1);
    }
    riff(b"VP8L", &bits.out)
}

/// A lossy WebP of `cols` by `rows` macroblocks whose every boolean decodes
/// as zero: both partitions are zero bytes, so the decoder's value register
/// stays below every split (RFC 6386 §7.3) and each boolean is `false`
/// whatever its probability.
///
/// Read that way the frame header (§9.2 to §9.11) asks for no segmentation,
/// a loop filter of level 0, one token partition, quantiser index 0, no
/// coefficient probability updates and no skip flags; every macroblock is
/// `B_PRED` with `B_DC_PRED` subblocks and `DC_PRED` chroma (§11.2 to §11.4,
/// the first branch of each tree); and every block's first token is
/// `DCT_EOB_TOKEN` (§13.2). The partitions are long enough that no boolean
/// reads past them: a macroblock spends a few bits of the first and under a
/// byte of the second, and gets a byte and four.
fn webp_lossy(cols: u32, rows: u32) -> Vec<u8> {
    let macroblocks = (cols * rows) as usize;
    let first = 64 + macroblocks;
    let tag = (1u32 << 4) | (u32::try_from(first).expect("a short partition") << 5);
    let mut frame = tag.to_le_bytes()[..3].to_vec();
    frame.extend_from_slice(&[0x9d, 0x01, 0x2a]);
    frame.extend_from_slice(&(u16::try_from(16 * cols).expect("a width")).to_le_bytes());
    frame.extend_from_slice(&(u16::try_from(16 * rows).expect("a height")).to_le_bytes());
    frame.resize(frame.len() + first + 4 * macroblocks, 0);
    riff(b"VP8 ", &frame)
}

/// What a WebP decodes to through the counted door, held to the uncounted
/// one, and the steps it took.
fn webp_decoded(file: &[u8]) -> (tinker_pdf_filters::WebpImage, u64) {
    let (image, steps) = webp_decode_counted(file, &ROOM);
    let image = image.expect("the file decodes");
    assert!(image.complete, "the picture decoded whole");
    assert_eq!(
        image.warnings,
        Vec::new(),
        "a clean file warns about nothing"
    );
    assert_eq!(
        webp_decode(file, &ROOM),
        Ok(image.clone()),
        "alike uncounted"
    );
    (image, steps)
}

#[test]
fn webp_costs_work_linear_in_its_pixels_and_macroblocks() {
    let green = |i: u32| ((i * 7 + i / 64) % 3 == 0) as usize;
    let lossless = |rows: u32| {
        let (image, steps) = webp_decoded(&webp_lossless(64, rows, green));
        assert_eq!((image.width, image.height), (64, rows));
        let ImagePixels::Rgb(rgb) = &image.pixels else {
            panic!("an opaque picture decodes to RGB");
        };
        for (i, pixel) in rgb.chunks_exact(3).enumerate() {
            let want = [WEBP_RED, WEBP_GREENS[green(i as u32)], WEBP_BLUE];
            assert_eq!(pixel, want, "pixel {i} comes back");
        }
        steps
    };
    holds_the_law("WebP lossless pixels", lossless(100), lossless(400));

    let lossy = |side: u32| {
        let (image, steps) = webp_decoded(&webp_lossy(side, side));
        assert_eq!((image.width, image.height), (16 * side, 16 * side));
        let ImagePixels::Rgb(rgb) = &image.pixels else {
            panic!("an opaque picture decodes to RGB");
        };
        // Every macroblock of a row of them is predicted alike from what is
        // above and to its left, so every row of pixels is one colour.
        for (y, row) in rgb.chunks_exact((16 * side * 3) as usize).enumerate() {
            let first = &row[..3];
            assert!(
                row.chunks_exact(3).all(|pixel| pixel == first),
                "row {y} is not one colour"
            );
        }
        steps
    };
    holds_the_law("WebP lossy macroblocks", lossy(8), lossy(16));
}

// ---------------------------------------------------------------------------
// JPEG XR.
// ---------------------------------------------------------------------------

/// The committed lossless pictures whose areas are four times one another,
/// each pair one pixel format and one codestream layout: 48 × 32 in one tile
/// and 96 × 64 in four (`tests/jxr/manifest.txt`). What each picture holds is
/// `jxr_fixtures.rs`'s to check, bit for bit; here each decodes whole, at its
/// size, alike through both doors.
const JXR_PAIRS: [(&str, &[u8], &[u8]); 3] = [
    (
        "JPEG XR RGB, spatial",
        include_bytes!("jxr/rgb24.jxr"),
        include_bytes!("jxr/tiled.jxr"),
    ),
    (
        "JPEG XR grey, spatial",
        include_bytes!("jxr/gray8.jxr"),
        include_bytes!("jxr/tiled_gray.jxr"),
    ),
    (
        "JPEG XR RGB, frequency",
        include_bytes!("jxr/frequency.jxr"),
        include_bytes!("jxr/frequency_tiled.jxr"),
    ),
];

#[test]
fn jpeg_xr_costs_work_linear_in_its_macroblocks_and_samples() {
    let at = |file: &[u8], size: (u32, u32)| {
        let (image, steps) = jxr_decode_counted(file, &ROOM);
        let image = image.expect("the picture decodes");
        assert!(image.complete, "the picture decoded whole");
        assert_eq!((image.width, image.height), size);
        assert_eq!(jxr_decode(file, &ROOM), Ok(image), "alike uncounted");
        steps
    };
    for (shape, small, large) in JXR_PAIRS {
        holds_the_law(shape, at(small, (48, 32)), at(large, (96, 64)));
    }
}

// ---------------------------------------------------------------------------
// The transform codecs' doors on damage.
// ---------------------------------------------------------------------------

/// Every way a shape above can be cut short: each prefix of a step through
/// it, and the whole with its last bytes lost.
fn cuts(file: &[u8]) -> Vec<&[u8]> {
    let step = (file.len() / 64).max(1);
    (0..file.len())
        .step_by(step)
        .map(|end| &file[..end])
        .chain([&file[..file.len().saturating_sub(1)]])
        .collect()
}

/// **A counted door answers what its uncounted sibling answers on damage
/// too**: a refusal is the same refusal, and a picture drawn around damage
/// the same picture, through each of the four.
#[test]
fn every_transform_codec_door_answers_as_its_uncounted_sibling_on_damage() {
    for file in [baseline(4), progressive(4), arithmetic_frame(4)] {
        for cut in cuts(&file) {
            let (counted, _) = jpeg_decode_counted(cut, 1 << 26);
            let plain = jpeg_decode(cut, 1 << 26);
            assert_eq!(
                counted.map(|i| (i.width, i.height, i.data, i.warnings)),
                plain.map(|i| (i.width, i.height, i.data, i.warnings)),
                "a JPEG cut at {}",
                cut.len()
            );
        }
    }
    let stream = jpx_tiles(3, 2);
    for cut in cuts(&stream) {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let (counted, _) = jpx_decode_counted(cut, &ROOM, &mut a);
        assert_eq!(
            counted,
            jpx_decode(cut, &ROOM, &mut b),
            "a codestream cut at {}",
            cut.len()
        );
        assert_eq!(a, b, "the same warnings");
    }
    for file in [webp_lossless(16, 4, |i| (i % 2) as usize), webp_lossy(2, 2)] {
        for cut in cuts(&file) {
            let (counted, _) = webp_decode_counted(cut, &ROOM);
            assert_eq!(
                counted,
                webp_decode(cut, &ROOM),
                "a WebP cut at {}",
                cut.len()
            );
        }
    }
    for (_, small, _) in JXR_PAIRS {
        for cut in cuts(small) {
            let (counted, _) = jxr_decode_counted(cut, &ROOM);
            assert_eq!(
                counted,
                jxr_decode(cut, &ROOM),
                "a JPEG XR file cut at {}",
                cut.len()
            );
        }
    }
}
