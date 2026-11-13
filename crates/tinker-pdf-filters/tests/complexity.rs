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

use tinker_pdf_filters::{
    ascii85_decode, ascii85_decode_counted, ascii_hex_decode, ascii_hex_decode_counted, bmp_decode,
    bmp_decode_counted, brotli_decode, brotli_decode_counted, ccitt_decode, ccitt_decode_counted,
    ccitt_g4_encode, flate_decode, flate_decode_counted, gif_decode, gif_decode_counted,
    jbig2_decode, jbig2_decode_measured, jbig2_generic_region_segment, lzw_decode,
    lzw_decode_counted, png_decode, png_decode_counted, png_encode, predictor_decode,
    predictor_decode_counted, run_length_decode, run_length_decode_counted, tiff_decode,
    tiff_decode_counted, zlib_compress, CcittParams, CcittSource, Jbig2GenericSource, Jbig2Params,
    Limits, PngColour, PngSource, PredictorParams,
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
