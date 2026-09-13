//! T.800 Annex J.10's published decoding example, decoded and checked.
//!
//! # Why this file exists, and what it replaces
//!
//! `jpx_reference.rs`'s module note says, in these words, that **"T.800
//! publishes no datastream annex — there is no equivalent of T.88's Annex
//! H.1"**. That is false, and it had been believed long enough to shape the
//! whole JPEG 2000 verification story: because no published datastream was
//! thought to exist, the decoder's strongest gate became a set of committed
//! third-party reference decodes, which ruling 13 admits **as a dated
//! measurement and never as a check that re-runs**.
//!
//! T.800 J.10, "An example of decoding showing intermediate steps", is exactly
//! that missing artefact. It gives a complete 100-byte codestream in hex, an
//! annotated field-by-field walk through its main header, tile-part header and
//! packet headers, and — in J.10.5 — the nine decoded component samples in
//! decimal. Every byte below and every expected value is transcribed from it.
//!
//! So this is the JPX equivalent of what Annex H.1 is for JBIG2: a check that
//! **can** be re-run, that shares no code with this decoder, and that is a
//! published statement by the people who wrote the format rather than a
//! recording of what some other program did once.
//!
//! # What one comparison pins
//!
//! The example is deliberately tiny — one component, 1 sample wide and 9
//! tall — and that is the point: it is small enough to be transcribed
//! correctly and complete enough that nothing can be got wrong quietly. The
//! nine samples at the end depend on the SIZ geometry, the QCD's guard bits
//! and exponents, the COD's single decomposition level and 5/3 reversible
//! filter, tier-2's packet header arithmetic, tier-1's context numbering and
//! MQ decoding, dequantisation, the inverse 5/3 and the DC level shift. A
//! defect in any of them moves at least one of the nine.
//!
//! # Provenance
//!
//! ITU-T Rec. T.800 (11/2015), Annex J.10, fetched from the ITU on 13
//! September 2026 and read with this repository's own `tpdf text`. The
//! Recommendation is published free of charge. Offsets in J.10 are octal and
//! values hexadecimal; the byte table below keeps J.10's own octal offsets in
//! its comments so a reader can check any line against the standard without
//! converting anything.

use tinker_pdf_filters::{jpx_decode, Limits};

/// T.800 J.10's codestream, transcribed field by field from J.10.1 and
/// J.10.2's annotated listings rather than from the raw hex dump.
///
/// The annotated listings name every field at its offset, so transcribing
/// from them is self-checking in a way the dump is not:
/// [`the_transcription_matches_the_annotated_offsets`] re-derives J.10's
/// octal offsets from this array and asserts each named field sits where the
/// standard says it does.
const ANNEX_J10: &[u8] = &[
    // 00000  SOC
    0xFF, 0x4F, //
    // 00002  SIZ
    0xFF, 0x51, //
    // 00004  Lsiz = 41
    0x00, 0x29, //
    // 00006  Rsiz
    0x00, 0x00, //
    // 00010  Xsiz = 1
    0x00, 0x00, 0x00, 0x01, //
    // 00014  Ysiz = 9
    0x00, 0x00, 0x00, 0x09, //
    // 00020  XOsiz
    0x00, 0x00, 0x00, 0x00, //
    // 00024  YOsiz
    0x00, 0x00, 0x00, 0x00, //
    // 00030  XTsiz = 1
    0x00, 0x00, 0x00, 0x01, //
    // 00034  YTsiz = 9
    0x00, 0x00, 0x00, 0x09, //
    // 00040  XTOsiz
    0x00, 0x00, 0x00, 0x00, //
    // 00044  YTOsiz
    0x00, 0x00, 0x00, 0x00, //
    // 00050  Csiz = 1
    0x00, 0x01, //
    // 00052  Ssiz = 8 bits unsigned, 00053 XRsiz = 1, 00054 YRsiz = 1
    0x07, 0x01, 0x01, //
    // 00055  QCD
    0xFF, 0x5C, //
    // 00057  Lqcd = 7
    0x00, 0x07, //
    // 00061  Sqcd: 2 guard bits, no quantization
    0x40, //
    // 00062  SPqcd: exponents 8, 9, 9, 10
    0x40, 0x48, 0x48, 0x50, //
    // 00066  COD
    0xFF, 0x52, //
    // 00070  Lcod = 12
    0x00, 0x0C, //
    // 00072  Scod: PPx = PPy = 15, no SOP, no EPH
    0x00, //
    // 00073  Progression order: layer-resolution-component-position
    0x00, //
    // 00074  Number of layers = 1
    0x00, 0x01, //
    // 00076  Multiple component transform: none
    0x00, //
    // 00077  Number of decomposition levels = 1
    0x01, //
    // 00100  Code-block width exponent offset = 4
    0x04, //
    // 00101  Code-block height exponent offset = 4
    0x04, //
    // 00102  Style of the code-block coding passes: none of Table A.19's bits
    0x00, //
    // 00103  Transform: 5/3 reversible
    0x01, //
    // 00104  SOT
    0xFF, 0x90, //
    // 00106  Lsot = 10
    0x00, 0x0A, //
    // 00110  Isot = tile 0
    0x00, 0x00, //
    // 00112  Psot = 30
    0x00, 0x00, 0x00, 0x1E, //
    // 00116  TPsot = 0
    0x00, //
    // 00117  TNsot = 1
    0x01, //
    // 00120  SOD
    0xFF, 0x93, //
    // 00122  Compressed data: packet header and packet body, 16 bytes
    0xC7, 0xD4, 0x0C, 0x01, 0x8F, 0x0D, 0xC8, 0x75, //
    0x5D, 0xC0, 0x7C, 0x21, 0x80, 0x0F, 0xB1, 0x76, //
    // 00142  EOC
    0xFF, 0xD9,
];

/// T.800 J.10.5: "After the inverse 5-3 reversible filter and level shifting,
/// the component samples in decimal are: 101, 103, 104, 105, 96, 97, 96, 102,
/// 109".
const ANNEX_J10_SAMPLES: [u8; 9] = [101, 103, 104, 105, 96, 97, 96, 102, 109];

/// The transcription is checked against J.10's own octal offsets before it is
/// trusted to check anything else.
///
/// Four JBIG2 Annex B tables were transcribed wrongly in this repository, and
/// the one that survived every check did so because the only invariant
/// available was necessary rather than sufficient. Here a stronger one is
/// available for free: J.10 prints an octal offset against every field it
/// names, so a mis-transcribed byte count moves a named field off its stated
/// offset. This test is that check, and it runs before the decode does.
#[test]
fn the_transcription_matches_the_annotated_offsets() {
    // (octal offset as written in J.10, the field's name, its bytes)
    let named: &[(&str, &str, &[u8])] = &[
        ("00000", "SOC", &[0xFF, 0x4F]),
        ("00002", "SIZ", &[0xFF, 0x51]),
        ("00004", "Lsiz", &[0x00, 0x29]),
        ("00050", "Csiz", &[0x00, 0x01]),
        ("00055", "QCD", &[0xFF, 0x5C]),
        ("00066", "COD", &[0xFF, 0x52]),
        ("00104", "SOT", &[0xFF, 0x90]),
        ("00120", "SOD", &[0xFF, 0x93]),
        ("00142", "EOC", &[0xFF, 0xD9]),
    ];
    for (octal, field, bytes) in named {
        let at = usize::from_str_radix(octal, 8).expect("J.10 writes its offsets in octal");
        assert_eq!(
            &ANNEX_J10[at..at + bytes.len()],
            *bytes,
            "{field} should sit at octal {octal} (byte {at})"
        );
    }

    // J.10.2: "The length of the tile-part is 30 bytes. Thus the next
    // tile-part or the end of the codestream is at 0104 + 036 = 0142."
    let sot = usize::from_str_radix("00104", 8).expect("octal");
    let eoc = usize::from_str_radix("00142", 8).expect("octal");
    assert_eq!(eoc - sot, 30, "Psot: the tile-part is 30 bytes");
    assert_eq!(
        ANNEX_J10.len(),
        eoc + 2,
        "the codestream ends with the EOC at octal 00142"
    );
}

/// The nine samples T.800 J.10.5 publishes, decoded by this build.
///
/// This is the one check in the JPEG 2000 decoder that is neither a round trip
/// through code this repository wrote nor a recording of what another program
/// once produced. The standard states the answer; this asserts it.
#[test]
fn annex_j10_decodes_to_the_samples_the_standard_publishes() {
    let mut warnings = Vec::new();
    let image = jpx_decode(ANNEX_J10, &Limits::new(1 << 20), &mut warnings)
        .expect("T.800 J.10's own codestream decodes");

    assert_eq!(image.width, 1, "J.10.1: 1 sample horizontally");
    assert_eq!(image.height, 9, "J.10.1: 9 samples vertically");
    assert_eq!(image.components, 1, "J.10.1: one component");
    assert_eq!(image.precision, 8, "J.10.1: 8 bits/sample unsigned");
    assert!(!image.truncated, "the codestream is whole");
    assert_eq!(
        image.samples, &ANNEX_J10_SAMPLES,
        "T.800 J.10.5 publishes these nine samples"
    );
}
