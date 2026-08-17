//! What the routing decision is held to.
//!
//! # The shape of test this file has to be
//!
//! Gap 18 milestone 7 found a precision shift that survived four boundary tests
//! because the comparative ones rendered two dictionaries through the same
//! defect. The comparison that matters here — pass-through against decode — is
//! two genuinely different paths and lives in
//! `crates/tinker-pdf/tests/png_passthrough.rs`, where a real render can be
//! compared pixel for pixel. What lives *here* is the half that comparison
//! cannot see: which route a file took, and what numbers went into its
//! dictionary. A build that decoded every file would render every page
//! correctly and be wrong about the only thing this module is for.
//!
//! Every expected value below is written out from ISO/IEC 15948 and ISO 32000-1
//! rather than read back from what the code emitted.

use super::*;
use crate::object::Object;
use crate::{CosDocument, Dict, DocumentBuilder, ObjRef};
use tinker_pdf_filters::Crc32;

const CAP: Limits = Limits::new(1 << 22);

// --- the builder --------------------------------------------------------

/// 5.3: length, type, data, CRC over the type and the data.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(kind);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
    out
}

/// 11.2.2's thirteen bytes.
fn ihdr(width: u32, height: u32, depth: u8, colour: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&width.to_be_bytes());
    d.extend_from_slice(&height.to_be_bytes());
    d.extend_from_slice(&[depth, colour, 0, 0, interlace]);
    chunk(b"IHDR", &d)
}

/// Rows of packed scanline bytes, each behind filter tag 0, zlib-compressed.
fn idat(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut raw = Vec::new();
    for r in rows {
        raw.push(0u8);
        raw.extend_from_slice(r);
    }
    chunk(b"IDAT", &zlib_compress(&raw))
}

fn file(header: Vec<u8>, middle: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::from(tinker_pdf_filters::PNG_SIGNATURE);
    out.extend_from_slice(&header);
    for c in middle {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// A one-pixel-per-row image of the given shape, with no transparency.
fn plain(width: u32, height: u32, depth: u8, colour: u8, interlace: u8) -> Vec<u8> {
    let channels = match colour {
        2 => 3u32,
        4 => 2,
        6 => 4,
        _ => 1,
    };
    let row_bytes =
        (u64::from(width) * u64::from(channels) * u64::from(depth)).div_ceil(8) as usize;
    let rows: Vec<Vec<u8>> = (0..height).map(|_| vec![0x11u8; row_bytes]).collect();

    let mut middle = Vec::new();
    if colour == 3 {
        // 11.2.3: a palette the depth can index. Two entries is legal at every
        // indexed depth, which keeps this builder honest at depth 1.
        middle.push(chunk(b"PLTE", &[0, 0, 0, 255, 255, 255]));
    }
    // The interlaced fixtures carry the same bytes rather than Adam7's seven
    // reduced images: the route is decided from IHDR's interlace byte alone,
    // and a decode that runs short still returns an image (ruling 2). The
    // fixture that holds interlace to a *picture* is `adam7_8x8.png`, in
    // `png.rs`'s own tests.
    middle.push(idat(&rows));
    file(ihdr(width, height, depth, colour, interlace), &middle)
}

fn route_of(bytes: &[u8]) -> PngRoute {
    png_image(bytes, &CAP)
        .expect("the fixture is a PNG")
        .route()
}

// --- gap 29's routing table, transcribed --------------------------------

/// The design table, entry by entry, over every legal colour type.
///
/// Written as the table rather than as a predicate: "everything with alpha
/// decodes" is a plausible reading that gets the `tRNS` rows wrong in both
/// directions, and a predicate carrying that mistake reads exactly like a
/// correct one — which is `png.rs`'s reason for transcribing Table 11.1 the
/// same way.
#[test]
fn every_colour_type_takes_the_route_the_design_table_says() {
    // (colour type, bit depth, interlaced, expected route)
    const TABLE: [(u8, u8, u8, PngRoute); 20] = [
        (0, 1, 0, PngRoute::PassedThrough),
        (0, 2, 0, PngRoute::PassedThrough),
        (0, 4, 0, PngRoute::PassedThrough),
        (0, 8, 0, PngRoute::PassedThrough),
        (0, 16, 0, PngRoute::PassedThrough),
        (2, 8, 0, PngRoute::PassedThrough),
        (2, 16, 0, PngRoute::PassedThrough),
        (3, 1, 0, PngRoute::PassedThrough),
        (3, 2, 0, PngRoute::PassedThrough),
        (3, 4, 0, PngRoute::PassedThrough),
        (3, 8, 0, PngRoute::PassedThrough),
        // The alpha is interleaved into the scanlines and PDF has no
        // /DecodeParms for that, whatever the depth.
        (4, 8, 0, PngRoute::Decoded),
        (4, 16, 0, PngRoute::Decoded),
        (6, 8, 0, PngRoute::Decoded),
        (6, 16, 0, PngRoute::Decoded),
        // Interlace overrides everything: seven reduced images are not one
        // raster, and /Predictor describes one raster.
        (0, 8, 1, PngRoute::Decoded),
        (2, 8, 1, PngRoute::Decoded),
        (3, 8, 1, PngRoute::Decoded),
        (4, 8, 1, PngRoute::Decoded),
        (6, 8, 1, PngRoute::Decoded),
    ];

    for (colour, depth, interlace, want) in TABLE {
        let bytes = plain(8, 8, depth, colour, interlace);
        assert_eq!(
            route_of(&bytes),
            want,
            "colour type {colour} depth {depth} interlace {interlace}"
        );
    }
}

/// The pass-through is not merely *chosen*: the bytes are the file's own.
#[test]
fn a_passed_through_idat_is_the_files_own_bytes() {
    let bytes = plain(8, 4, 8, 2, 0);
    let scan = png_scan(&bytes).expect("it scans");
    let prepared = png_image(&bytes, &CAP).expect("it prepares");

    assert_eq!(prepared.route(), PngRoute::PassedThrough);
    assert_eq!(
        prepared.data, scan.idat,
        "the concatenated IDAT reaches the page unmodified"
    );
    assert!(
        bytes
            .windows(prepared.data.len())
            .any(|w| w == prepared.data),
        "and those bytes are still a subsequence of the file"
    );
}

/// 7.4.4.4 Table 10, against IHDR's own numbers rather than against whatever
/// the code produced.
///
/// `/Columns` is the one that fails invisibly: a wrong value unfilters every
/// row at the wrong stride and produces a scrambled picture rather than an
/// absent one, which reads as a decoder bug and gets found late.
#[test]
fn the_predictor_parameters_are_ihdrs_own_numbers() {
    // (colour type, depth, width, expected /Colors, expected /BitsPerComponent)
    const CASES: [(u8, u8, u32, u32, u32); 6] = [
        (0, 1, 17, 1, 1),
        (0, 8, 5, 1, 8),
        (0, 16, 5, 1, 16),
        (2, 8, 5, 3, 8),
        (2, 16, 5, 3, 16),
        // Indexed: one index a sample, whatever the palette holds.
        (3, 4, 9, 1, 4),
    ];

    for (colour, depth, width, colors, bits) in CASES {
        let prepared = png_image(&plain(width, 3, depth, colour, 0), &CAP).expect("prepares");
        assert_eq!(
            prepared.filter,
            Some(ImageFilter::FlatePngPredictor {
                colors,
                bits_per_component: bits,
                columns: width,
            }),
            "colour type {colour} depth {depth}"
        );
    }
}

// --- 11.3.2.1 into 8.9.6.4 ----------------------------------------------

/// A greyscale `tRNS` is one fully transparent level, which is one range.
#[test]
fn a_grey_transparency_key_becomes_a_one_value_range() {
    let mut middle = vec![chunk(b"tRNS", &[0x00, 0x11])];
    middle.push(idat(&[vec![0x11, 0x22]]));
    let bytes = file(ihdr(2, 1, 8, 0, 0), &middle);

    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.route(), PngRoute::PassedThrough);
    assert_eq!(
        prepared.color_key_mask,
        vec![(0x11, 0x11)],
        "8.9.6.4: [min max] over the one component"
    );
}

/// A truecolour `tRNS` is one triple, so three ranges in component order.
#[test]
fn a_truecolour_transparency_key_becomes_three_ranges_in_order() {
    let mut middle = vec![chunk(b"tRNS", &[0, 0x10, 0, 0x20, 0, 0x30])];
    middle.push(idat(&[vec![1, 2, 3]]));
    let bytes = file(ihdr(1, 1, 8, 2, 0), &middle);

    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.route(), PngRoute::PassedThrough);
    assert_eq!(
        prepared.color_key_mask,
        vec![(0x10, 0x10), (0x20, 0x20), (0x30, 0x30)],
        "red, green, blue — a transposed pair here is a mask on the wrong colour"
    );
}

/// 11.3.2.1 stores the key in two bytes at the image's own depth, so a value
/// the depth cannot hold matches no sample at all.
///
/// The wrong answer is a `/Mask [255 255]` on a 2-bit image: 8.9.6.4 requires
/// each integer to be at most `2^BitsPerComponent - 1`, and a reader that
/// clamped it would mask value 3 — every white pixel in the picture.
#[test]
fn a_key_the_depth_cannot_hold_produces_no_mask_rather_than_an_illegal_one() {
    let mut middle = vec![chunk(b"tRNS", &[0x00, 0xFF])];
    middle.push(idat(&[vec![0b1100_0000]]));
    let bytes = file(ihdr(1, 1, 2, 0, 0), &middle);

    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.route(), PngRoute::PassedThrough);
    assert!(
        prepared.color_key_mask.is_empty(),
        "no key, rather than one out of range: {:?}",
        prepared.color_key_mask
    );
}

/// A palette `tRNS`, in each of the four shapes it can take.
#[test]
fn a_palette_transparency_is_a_range_only_when_it_is_one() {
    // Four entries, so a tRNS list of up to four alphas.
    let palette = chunk(b"PLTE", &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    let with = |alphas: &[u8]| -> PngImageData {
        let middle = vec![
            palette.clone(),
            chunk(b"tRNS", alphas),
            idat(&[vec![0b0001_1011]]),
        ];
        png_image(&file(ihdr(4, 1, 2, 3, 0), &middle), &CAP).expect("prepares")
    };

    // One transparent index: a range of length one.
    let one = with(&[0, 255, 255, 255]);
    assert_eq!(one.route(), PngRoute::PassedThrough);
    assert_eq!(one.color_key_mask, vec![(0, 0)]);

    // A contiguous run of two, which is what 8.9.6.4 expresses and what gap
    // 29's table calls "one fully transparent index" the general case of.
    let run = with(&[255, 0, 0, 255]);
    assert_eq!(run.route(), PngRoute::PassedThrough);
    assert_eq!(run.color_key_mask, vec![(1, 2)]);

    // Two runs. One inclusive range cannot name both, and naming the span
    // between them would make index 1 transparent when the file says it is not.
    assert_eq!(with(&[0, 255, 0, 255]).route(), PngRoute::Decoded);

    // Partial alpha: a range test is not a lookup.
    assert_eq!(with(&[128, 255, 255, 255]).route(), PngRoute::Decoded);

    // Every entry opaque is a file that asked for nothing.
    let none = with(&[255, 255, 255, 255]);
    assert_eq!(none.route(), PngRoute::PassedThrough);
    assert!(none.color_key_mask.is_empty());
}

/// Entries past the end of a `tRNS` list are opaque (11.3.2.1), so a run cannot
/// continue past it — a decoder that treated the missing tail as transparent
/// would mask most of the palette.
#[test]
fn transparency_stops_where_the_trns_list_does() {
    let palette = chunk(b"PLTE", &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    let middle = vec![
        palette,
        // Only the first two entries are named at all.
        chunk(b"tRNS", &[255, 0]),
        idat(&[vec![0b0001_1011]]),
    ];
    let prepared = png_image(&file(ihdr(4, 1, 2, 3, 0), &middle), &CAP).expect("prepares");
    assert_eq!(
        prepared.color_key_mask,
        vec![(1, 1)],
        "index 1, and only it"
    );
}

// --- the decoded route --------------------------------------------------

/// Colour type 6 splits into RGB samples and a `/DeviceGray` `/SMask`.
#[test]
fn an_alpha_channel_becomes_a_soft_mask_of_its_own() {
    // Two pixels: opaque red, half-transparent green.
    let rows = vec![vec![255, 0, 0, 255, 0, 255, 0, 128]];
    let bytes = file(ihdr(2, 1, 8, 6, 0), &[idat(&rows)]);

    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.route(), PngRoute::Decoded);

    let mask = prepared.soft_mask.as_ref().expect("a soft mask");
    assert_eq!((mask.width, mask.height), (2, 1));
    assert_eq!(mask.bits_per_component, 8);
    assert_eq!(
        inflate(&mask.data),
        vec![255, 128],
        "the alpha channel, in pixel order"
    );
    assert_eq!(
        inflate(&prepared.data),
        vec![255, 0, 0, 0, 255, 0],
        "and the colour with the alpha taken out from between the pixels"
    );
}

/// Sixteen-bit alpha keeps both of its bytes, in PNG's network order.
///
/// Splitting on a one-byte stride would interleave the halves of every sample
/// into the mask and produce a plausible-looking gradient out of nothing.
#[test]
fn a_sixteen_bit_alpha_channel_splits_on_the_right_stride() {
    // Colour type 4 at depth 16: two components, four bytes a pixel. Two
    // pixels — grey 0x1122 over alpha 0x3344, then 0x5566 over 0x7788 — and
    // every one of the eight bytes differs, so a stride mistake cannot land on
    // the right answer by accident.
    let row = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let bytes = file(ihdr(2, 1, 16, 4, 0), &[idat(&[row])]);

    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.bits_per_component, 16);
    assert_eq!(inflate(&prepared.data), vec![0x11, 0x22, 0x55, 0x66]);
    let mask = prepared.soft_mask.as_ref().expect("a soft mask");
    assert_eq!(inflate(&mask.data), vec![0x33, 0x44, 0x77, 0x88]);
}

/// The decoded route deflates its own bytes, because nothing downstream will.
///
/// `WriteOptions::default()` has `compress: false` and `DocumentBuilder::finish`
/// uses the default, so a raster handed over uncompressed lands in the file at
/// full size — which is the *w x h x 3* a page that gap 29 exists to avoid,
/// moved from memory into the document. The gap is only visible with a fixture
/// large enough for the difference to be an order of magnitude.
#[test]
fn a_decoded_png_is_re_deflated_here_because_the_writer_will_not() {
    // 64 x 64 RGBA in flat 8 x 8 blocks: 16 384 bytes of raster, 12 288 of
    // colour, and the kind of repetition a page of line art actually has.
    let rows: Vec<Vec<u8>> = (0..64u32)
        .map(|y| {
            (0..64u32)
                .flat_map(|x| [(x / 8 * 32) as u8, (y / 8 * 32) as u8, 0x40, 0xC0])
                .collect()
        })
        .collect();
    let bytes = file(ihdr(64, 64, 8, 6, 0), &[idat(&rows)]);

    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.route(), PngRoute::Decoded);
    assert!(
        prepared.data.len() < 64 * 64 * 3 / 4,
        "the colour samples are deflated: {} bytes for {}",
        prepared.data.len(),
        64 * 64 * 3
    );

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &prepared.image()));
    builder.add_page(64.0, 64.0, |page| page.image(b"Im0", 0.0, 0.0, 64.0, 64.0));
    let written = builder.finish();
    assert!(
        written.len() < 64 * 64 * 4 / 2,
        "and the document is not the raster: {} bytes",
        written.len()
    );

    // And it still reads back as the samples the decoder produced.
    let doc = CosDocument::open(written).expect("it opens");
    let (_, reference) = image_dict(&doc).expect("an image XObject");
    assert_eq!(
        doc.stream_decoded(reference).map(|d| d.len()).ok(),
        Some(64 * 64 * 3)
    );
}

/// A greyscale image with no alpha keeps its single channel and gains no mask.
#[test]
fn a_decoded_image_without_alpha_gets_no_soft_mask() {
    let bytes = plain(4, 4, 8, 0, 1);
    let prepared = png_image(&bytes, &CAP).expect("prepares");
    assert_eq!(prepared.route(), PngRoute::Decoded);
    assert!(prepared.soft_mask.is_none());
    assert_eq!(prepared.filter, Some(ImageFilter::Flate));
}

fn inflate(data: &[u8]) -> Vec<u8> {
    tinker_pdf_filters::flate_decode(data, &Limits::new(1 << 20), None)
        .expect("the re-deflated stream inflates")
        .data
}

// --- the two fields milestone 5 will read -------------------------------

/// `complete` and `warnings` have no consumer in this milestone, so they are
/// tested as though they had one — milestone 1's posture towards `end`, and for
/// the same reason: a seam checked only by its future caller is gap 18
/// milestone 6's plane count, which was correct only for the inputs its own
/// encoder could produce.
///
/// What a caller will do with them is decide whether a page is honest, so the
/// distinction that has to hold is between damage that costs *pixels* and damage
/// that costs none.
#[test]
fn a_pass_through_reports_what_the_walk_found() {
    let good = plain(4, 4, 8, 2, 0);
    let sound = png_image(&good, &CAP).expect("prepares");
    assert!(sound.complete());
    assert!(
        sound.warnings().is_empty(),
        "a well-formed file needs no excuses: {:?}",
        sound.warnings()
    );

    // A file whose IEND was cut off. Nothing here inflates, so what is known is
    // that the file did not end where PNG says a file ends.
    let mut cut = good.clone();
    cut.truncate(cut.len() - 12);
    let short = png_image(&cut, &CAP).expect("still prepares");
    assert_eq!(short.route(), PngRoute::PassedThrough);
    assert!(short.warnings().contains(&FilterWarning::EarlyEod));
    assert!(!short.complete(), "and it says the file stopped early");
    assert_eq!(
        short.data,
        png_scan(&good).expect("scans").idat,
        "while the IDAT it did carry is still the file's own bytes"
    );

    // An ancillary chunk with a broken CRC is dropped and reported, and costs
    // no pixels at all — 5.4's whole point, and the reason `complete` is not
    // simply "the warning list is empty".
    let mut gama = chunk(b"gAMA", &[0, 1, 0x86, 0xA0]);
    let last = gama.len() - 1;
    gama[last] ^= 0xFF;
    let bytes = file(
        ihdr(2, 1, 8, 2, 0),
        &[gama, idat(&[vec![1, 2, 3, 4, 5, 6]])],
    );
    let dropped = png_image(&bytes, &CAP).expect("prepares");
    assert!(dropped
        .warnings()
        .contains(&FilterWarning::PngChunkCrcMismatch));
    assert!(
        dropped.complete(),
        "a dropped gAMA costs no pixels, so the picture is whole"
    );
}

/// The decoded route reports its own raster's completeness rather than the
/// walk's, because that is the number that says whether pixels are missing.
#[test]
fn a_decoded_image_reports_its_own_raster() {
    // Colour type 6, so this decodes; the IDAT is one row short of the four the
    // header declares, which is damage that costs pixels.
    let rows: Vec<Vec<u8>> = (0..3).map(|_| vec![9u8; 4 * 4]).collect();
    let bytes = file(ihdr(4, 4, 8, 6, 0), &[idat(&rows)]);
    let prepared = png_image(&bytes, &CAP).expect("prepares");

    assert_eq!(prepared.route(), PngRoute::Decoded);
    assert!(
        !prepared.complete(),
        "three rows where four were declared is not complete"
    );
    // And the image is still produced: ruling 2 degrades rather than refuses.
    assert!(prepared.soft_mask.is_some());
}

// --- what reaches the dictionary ----------------------------------------

/// The whole of milestone 4's dictionary criterion, read back out of a written
/// file rather than out of the builder's own structures.
#[test]
fn a_passed_through_png_reaches_the_page_as_flate_with_predictor_fifteen() {
    let png = plain(6, 3, 8, 2, 0);
    let prepared = png_image(&png, &CAP).expect("prepares");

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &prepared.image()));
    builder.add_page(6.0, 3.0, |page| page.image(b"Im0", 0.0, 0.0, 6.0, 3.0));
    let bytes = builder.finish();

    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    let (dict, reference) = image_dict(&doc).expect("an image XObject");

    let int = |key: &[u8]| dict.get_int(doc.intern(key));
    assert_eq!(int(b"Width"), Some(6));
    assert_eq!(int(b"Height"), Some(3));
    assert_eq!(int(b"BitsPerComponent"), Some(8));
    assert_eq!(
        name_of(&doc, &dict, b"ColorSpace").as_deref(),
        Some(&b"DeviceRGB"[..])
    );
    assert_eq!(
        name_of(&doc, &dict, b"Filter").as_deref(),
        Some(&b"FlateDecode"[..])
    );

    let parms = dict
        .get(doc.intern(b"DecodeParms"))
        .and_then(Object::as_dict)
        .expect("/DecodeParms");
    // 7.4.4.4 Table 10, key by key.
    assert_eq!(parms.get_int(doc.intern(b"Predictor")), Some(15));
    assert_eq!(parms.get_int(doc.intern(b"Colors")), Some(3));
    assert_eq!(parms.get_int(doc.intern(b"BitsPerComponent")), Some(8));
    assert_eq!(parms.get_int(doc.intern(b"Columns")), Some(6));

    // And the bytes in the file are the file's own IDAT, not a re-encoding.
    let scan = png_scan(&png).expect("scans");
    assert!(
        bytes.windows(scan.idat.len()).any(|w| w == scan.idat),
        "the IDAT survives the writer verbatim"
    );
    // Which the reader then undoes back to samples.
    let samples = doc.stream_decoded(reference).expect("it decodes");
    assert_eq!(samples.len(), 6 * 3 * 3);
}

/// An indexed PNG becomes `[/Indexed /DeviceRGB hival lookup]` with `/hival`
/// derived from the palette rather than declared beside it.
#[test]
fn an_indexed_png_carries_its_palette_as_the_colour_space() {
    let middle = vec![
        chunk(b"PLTE", &[10, 20, 30, 40, 50, 60, 70, 80, 90]),
        idat(&[vec![0b0001_1000]]),
    ];
    let png = file(ihdr(4, 1, 2, 3, 0), &middle);
    let prepared = png_image(&png, &CAP).expect("prepares");

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &prepared.image()));
    builder.add_page(4.0, 1.0, |page| page.image(b"Im0", 0.0, 0.0, 4.0, 1.0));
    let doc = CosDocument::open(builder.finish()).expect("it opens");
    let (dict, _) = image_dict(&doc).expect("an image XObject");

    let space = dict
        .get(doc.intern(b"ColorSpace"))
        .and_then(Object::as_array)
        .expect("an array");
    assert_eq!(space.len(), 4);
    assert_eq!(
        space
            .first()
            .and_then(Object::as_name)
            .and_then(|n| doc.name_bytes(n))
            .as_deref()
            .map(<[u8]>::to_vec),
        Some(b"Indexed".to_vec())
    );
    assert_eq!(
        space
            .get(1)
            .and_then(Object::as_name)
            .and_then(|n| doc.name_bytes(n))
            .as_deref()
            .map(<[u8]>::to_vec),
        Some(b"DeviceRGB".to_vec())
    );
    // Three entries, so 8.6.6.3's /hival is two — never the entry count.
    assert_eq!(space.get(2).and_then(Object::as_int), Some(2));
    assert_eq!(
        space
            .get(3)
            .and_then(Object::as_string)
            .map(|s| s.bytes.clone()),
        Some(vec![10, 20, 30, 40, 50, 60, 70, 80, 90])
    );
    // /Colors is 1 for an indexed image: a scanline holds indices.
    let parms = dict
        .get(doc.intern(b"DecodeParms"))
        .and_then(Object::as_dict)
        .expect("/DecodeParms");
    assert_eq!(parms.get_int(doc.intern(b"Colors")), Some(1));
    assert_eq!(parms.get_int(doc.intern(b"BitsPerComponent")), Some(2));
    assert_eq!(parms.get_int(doc.intern(b"Columns")), Some(4));
}

/// A colour key reaches the dictionary as 8.9.6.4's flat array of 2n integers.
#[test]
fn a_colour_key_reaches_the_dictionary_as_a_flat_array() {
    let middle = vec![
        chunk(b"tRNS", &[0, 0x10, 0, 0x20, 0, 0x30]),
        idat(&[vec![1, 2, 3]]),
    ];
    let png = file(ihdr(1, 1, 8, 2, 0), &middle);
    let prepared = png_image(&png, &CAP).expect("prepares");

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &prepared.image()));
    builder.add_page(1.0, 1.0, |page| page.image(b"Im0", 0.0, 0.0, 1.0, 1.0));
    let doc = CosDocument::open(builder.finish()).expect("it opens");
    let (dict, _) = image_dict(&doc).expect("an image XObject");

    let mask = dict
        .get(doc.intern(b"Mask"))
        .and_then(Object::as_array)
        .expect("/Mask");
    let values: Vec<i64> = mask.iter().filter_map(Object::as_int).collect();
    assert_eq!(
        values,
        vec![0x10, 0x10, 0x20, 0x20, 0x30, 0x30],
        "[min1 max1 min2 max2 min3 max3], not pairs in some other order"
    );
}

/// The soft mask is a second image XObject, referenced rather than inlined.
#[test]
fn a_soft_mask_is_written_as_its_own_xobject() {
    let rows = vec![vec![9, 9, 9, 255, 8, 8, 8, 0]];
    let png = file(ihdr(2, 1, 8, 6, 0), &[idat(&rows)]);
    let prepared = png_image(&png, &CAP).expect("prepares");

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &prepared.image()));
    builder.add_page(2.0, 1.0, |page| page.image(b"Im0", 0.0, 0.0, 2.0, 1.0));
    let doc = CosDocument::open(builder.finish()).expect("it opens");
    let (dict, _) = image_dict(&doc).expect("an image XObject");

    let smask = dict.get_ref(doc.intern(b"SMask")).expect("/SMask");
    let mask = doc.get(smask).ok().and_then(|o| o.as_dict().cloned());
    let mask = mask.expect("the mask object is a dictionary");
    assert_eq!(mask.get_int(doc.intern(b"Width")), Some(2));
    assert_eq!(mask.get_int(doc.intern(b"Height")), Some(1));
    assert_eq!(
        name_of(&doc, &mask, b"ColorSpace").as_deref(),
        Some(&b"DeviceGray"[..]),
        "11.6.5.3: a soft mask is DeviceGray or it is not a mask"
    );
    assert_eq!(
        doc.stream_decoded(smask).ok(),
        Some(vec![255, 0]),
        "and its samples are the alpha channel"
    );
}

// --- helpers ------------------------------------------------------------

/// The first `/Subtype /Image` object in a written document, with its number.
fn image_dict(doc: &CosDocument) -> Option<(Dict, ObjRef)> {
    let subtype = doc.intern(b"Subtype");
    let smask = doc.intern(b"SMask");
    let mut found: Option<(Dict, ObjRef)> = None;
    for num in 1..40u32 {
        let r = ObjRef::new(num, 0);
        let Ok(object) = doc.get(r) else { continue };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let is_image = dict
            .get(subtype)
            .and_then(Object::as_name)
            .and_then(|n| doc.name_bytes(n))
            .is_some_and(|b| b.as_ref() == b"Image");
        if !is_image {
            continue;
        }
        // Prefer the one that owns a mask, so a fixture with two images does
        // not hand back the mask itself.
        if dict.get(smask).is_some() {
            return Some((dict.clone(), r));
        }
        if found.is_none() {
            found = Some((dict.clone(), r));
        }
    }
    found
}

fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    dict.get(doc.intern(key))
        .and_then(Object::as_name)
        .and_then(|n| doc.name_bytes(n))
        .map(|b| b.as_ref().to_vec())
}
