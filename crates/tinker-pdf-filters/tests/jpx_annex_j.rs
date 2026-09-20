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

// --- J.10 with its packet headers packed (A.7.4, A.7.5) -----------------
//
// # Why this is evidence and not self-agreement
//
// The obvious way to test packed packet headers is to write a codestream
// that carries one picture twice, once with ordinary headers and once
// packed, and assert the two decode alike. That proves nothing: both halves
// would be ours, and an encoder we wrote agreeing with a decoder we wrote is
// a statement about this repository rather than about T.800.
//
// J.10 makes a real one available, and the reason is easy to miss. It is not
// only that the codestream and its samples are published — it is that **the
// boundary between each packet's header and its body is published too**, in
// prose and in two tables, byte for byte:
//
//   - Table J.20 lists the first packet header's three bytes, `0xC7`, `0xD4`
//     and `0x0C`, and J.10.3 says "Decoding the first packet header requires
//     3 bytes and indicates that 6 bytes of arithmetic coded compressed data
//     are used for the only code-block in this packet."
//   - J.10.4: "The bytes provided to the arithmetic coder are those
//     beginning at offset 0125", and prints them: `01 8F0D C875 5D`.
//   - Table J.21 lists the second packet header's four bytes, `0xC0`,
//     `0x7C`, `0x21` and `0x80`.
//   - J.10.4 again: "The compressed data for the only code-block in the
//     second packet, representing the vertical high pass horizontal lowpass
//     sub-band begins at offset 0137 octal", and prints `0F B176`.
//
// So the split below is the standard's, not this decoder's. A.7.4 then says
// exactly what a packed header stream contains — "The contents are exactly
// the packet header which would have been distributed in the bit stream as
// described in B.10" — which makes the relocation a mechanical rearrangement
// of published bytes rather than an encoding decision. The expected output
// is still J.10.5's nine published samples.
//
// **Where the standard's evidence stops and ours begins.** Everything inside
// the two streams is J.10's. What this file authors is the *container*: the
// PPM and PPT marker segments around the packed bytes, and the two `Psot`
// values the move changes. Those are transcribed from Tables A.38 and A.39
// and asserted field by field, at their offsets, before any decode runs —
// the same discipline as
// [`the_transcription_matches_the_annotated_offsets`] above, and for the
// same reason.

/// Octal 00122, where J.10.2 says the tile-part's 16 bytes of compressed
/// data begin: "The next 16 bytes are compressed data (30-byte length – 14
/// bytes of marker segments)."
const J10_DATA_AT: &str = "00122";

/// J.10.3 and J.10.4's division of those 16 bytes, each span at the octal
/// offset the standard prints against it.
///
/// `(octal offset, what J.10 calls it, its published bytes)`.
const J10_SPANS: [(&str, &str, &[u8]); 4] = [
    // Table J.20's "Codestream bytes" column, in its own order.
    ("00122", "first packet header", &[0xC7, 0xD4, 0x0C]),
    // J.10.4: "0000125  01 8F0D C875 5D".
    (
        "00125",
        "first packet body",
        &[0x01, 0x8F, 0x0D, 0xC8, 0x75, 0x5D],
    ),
    // Table J.21's "Codestream bytes" column.
    ("00133", "second packet header", &[0xC0, 0x7C, 0x21, 0x80]),
    // J.10.4: "0000137 0F B176".
    ("00137", "second packet body", &[0x0F, 0xB1, 0x76]),
];

fn octal(s: &str) -> usize {
    usize::from_str_radix(s, 8).expect("J.10 writes its offsets in octal")
}

/// The four spans sit where J.10 says, hold the bytes J.10 prints, and tile
/// the tile-part's compressed data exactly.
///
/// This runs before either packed fixture is built, because it is the whole
/// basis for them: if the header/body boundary were this decoder's opinion
/// rather than the standard's, moving the headers would be an experiment on
/// ourselves.
///
/// **It also records an erratum, because the transcription found one.**
/// J.10.3 ends "Thus the next packet header begins at offset 0134." The
/// first header is three bytes at 0122 and the first body is six bytes at
/// 0125, so the next header begins at 0133 — and Table J.21, immediately
/// below that sentence, lists `0xC0` as its first byte, which is the byte at
/// 0133 and not the byte at 0134. J.10.4's "begins at offset 0137" for the
/// second body agrees with 0133 and not with 0134. Three of the standard's
/// four statements about this boundary are consistent; the prose sentence is
/// one past them, and the assertions below are on the three.
#[test]
fn j10_publishes_where_each_packet_header_ends() {
    let mut at = octal(J10_DATA_AT);
    assert_eq!(at, 82, "octal 00122");
    for (offset, name, bytes) in J10_SPANS {
        assert_eq!(octal(offset), at, "{name} follows the span before it");
        assert_eq!(
            ANNEX_J10.get(at..at + bytes.len()),
            Some(bytes),
            "{name} at octal {offset} is not what J.10 prints"
        );
        at += bytes.len();
    }
    // J.10.2: the compressed data is 16 bytes, and the EOC follows it.
    assert_eq!(at - octal(J10_DATA_AT), 16, "J.10.2: 16 bytes of data");
    assert_eq!(at, octal("00142"), "the EOC sits at octal 00142");

    // The erratum, pinned so that a later reader does not re-derive it.
    // Table J.21's first byte is at 0133; J.10.3's prose says 0134.
    assert_eq!(octal("00133"), octal("00122") + 3 + 6);
    assert_ne!(octal("00134"), octal("00122") + 3 + 6);
}

/// The two packet headers, concatenated: J.10's `Ippm` and `Ippt` contents.
fn j10_packed_headers() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(J10_SPANS[0].2);
    out.extend_from_slice(J10_SPANS[2].2);
    out
}

/// The two packet bodies, concatenated: what is left in the bit stream once
/// the headers have moved.
fn j10_packed_bodies() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(J10_SPANS[1].2);
    out.extend_from_slice(J10_SPANS[3].2);
    out
}

/// Octal 00104, J.10.1's "the main header … ends before byte 0104".
const J10_SOT_AT: &str = "00104";
/// Octal 00120, J.10.2's SOD.
const J10_SOD_AT: &str = "00120";

/// J.10's codestream with its packet headers moved into a PPM marker segment
/// in the main header (A.7.4).
///
/// Everything but the PPM segment and `Psot` is J.10's own bytes in J.10's
/// own order.
fn j10_with_ppm() -> Vec<u8> {
    assemble_ppm(&j10_packed_headers(), &j10_packed_bodies())
}

/// A.7.4's container around any header stream and any body stream, so that
/// [`a_packed_stream_split_one_byte_wrong_is_refused`] can build the same
/// codestream with the seam in the wrong place.
fn assemble_ppm(headers: &[u8], bodies: &[u8]) -> Vec<u8> {
    // Table A.38: PPM, Lppm, Zppm, then the (Nppm, Ippm) series.
    let mut ppm = vec![0xFF, 0x60];
    let lppm = 2 + 1 + 4 + headers.len();
    ppm.extend_from_slice(
        &u16::try_from(lppm)
            .expect("a 7-byte header stream")
            .to_be_bytes(),
    );
    ppm.push(0); // Zppm: the only segment, so index 0.
                 // Nppm_0: "Number of bytes of Ippm information for the ith tile-part in
                 // the order found in the codestream." J.10 has one tile-part.
    ppm.extend_from_slice(&u32::try_from(headers.len()).expect("fits").to_be_bytes());
    ppm.extend_from_slice(headers);

    let mut out = ANNEX_J10[..octal(J10_SOT_AT)].to_vec();
    out.extend_from_slice(&ppm);
    // A.4.2's SOT segment, unchanged but for Psot, which counts from the
    // first byte of SOT to the last byte of the tile-part's data.
    let mut sot = ANNEX_J10[octal(J10_SOT_AT)..octal(J10_SOD_AT)].to_vec();
    let psot = sot.len() + 2 + bodies.len();
    sot[6..10].copy_from_slice(&u32::try_from(psot).expect("fits").to_be_bytes());
    out.extend_from_slice(&sot);
    out.extend_from_slice(&[0xFF, 0x93]); // SOD
    out.extend_from_slice(bodies);
    out.extend_from_slice(&[0xFF, 0xD9]); // EOC
    out
}

/// J.10's codestream with its packet headers moved into a PPT marker segment
/// in the one tile-part header (A.7.5).
fn j10_with_ppt() -> Vec<u8> {
    assemble_ppt(&j10_packed_headers(), &j10_packed_bodies())
}

/// A.7.5's container, as [`assemble_ppm`] is A.7.4's.
fn assemble_ppt(headers: &[u8], bodies: &[u8]) -> Vec<u8> {
    // Table A.39: PPT, Lppt, Zppt, then Ippt. There is no Nppt — a PPT
    // segment describes one tile's packets and carries no per-tile-part
    // lengths, which is the one structural difference between the two.
    let mut ppt = vec![0xFF, 0x61];
    let lppt = 2 + 1 + headers.len();
    ppt.extend_from_slice(&u16::try_from(lppt).expect("fits").to_be_bytes());
    ppt.push(0); // Zppt.
    ppt.extend_from_slice(headers);

    let mut out = ANNEX_J10[..octal(J10_SOT_AT)].to_vec();
    let mut sot = ANNEX_J10[octal(J10_SOT_AT)..octal(J10_SOD_AT)].to_vec();
    let psot = sot.len() + ppt.len() + 2 + bodies.len();
    sot[6..10].copy_from_slice(&u32::try_from(psot).expect("fits").to_be_bytes());
    out.extend_from_slice(&sot);
    out.extend_from_slice(&ppt);
    out.extend_from_slice(&[0xFF, 0x93]); // SOD
    out.extend_from_slice(bodies);
    out.extend_from_slice(&[0xFF, 0xD9]); // EOC
    out
}

/// Every field of the two containers sits where Tables A.38 and A.39 say,
/// and this runs before either is decoded.
#[test]
fn the_packed_containers_match_tables_a38_and_a39() {
    let ppm = j10_with_ppm();
    let sot = octal(J10_SOT_AT);
    // Table A.38's rows, in order, with their sizes in bits: PPM 16, Lppm
    // 16, Zppm 8, Nppm_i 32, Ippm_ij variable.
    assert_eq!(&ppm[sot..sot + 2], &[0xFF, 0x60], "PPM is 0xFF60");
    let lppm = u16::from_be_bytes([ppm[sot + 2], ppm[sot + 3]]);
    assert_eq!(
        lppm, 14,
        "Lppm counts Lppm, Zppm, Nppm and Ippm: 2 + 1 + 4 + 7"
    );
    assert!(
        (7..=65_535).contains(&u32::from(lppm)),
        "Table A.38 gives Lppm as 7 to 65 535"
    );
    assert_eq!(ppm[sot + 4], 0, "Zppm");
    assert_eq!(
        u32::from_be_bytes([ppm[sot + 5], ppm[sot + 6], ppm[sot + 7], ppm[sot + 8]]),
        7,
        "Nppm_0 is the one tile-part's header byte count"
    );
    assert_eq!(&ppm[sot + 9..sot + 16], &j10_packed_headers()[..], "Ippm");
    // The tile-part follows, and its Psot reaches the EOC.
    assert_eq!(&ppm[sot + 16..sot + 18], &[0xFF, 0x90], "SOT after the PPM");
    let psot = u32::from_be_bytes([ppm[sot + 22], ppm[sot + 23], ppm[sot + 24], ppm[sot + 25]]);
    assert_eq!(psot, 23, "Psot: 12 SOT + 2 SOD + 9 body bytes");
    assert_eq!(
        &ppm[sot + 16 + psot as usize..],
        &[0xFF, 0xD9],
        "A.4.2: Psot reaches the byte after the tile-part, which is the EOC"
    );

    let ppt = j10_with_ppt();
    // Table A.39: PPT 16, Lppt 16, Zppt 8, Ippt_i variable. The PPT segment
    // begins where J.10's SOD used to, since the SOT segment before it is
    // unchanged.
    let at = octal(J10_SOD_AT);
    assert_eq!(&ppt[at..at + 2], &[0xFF, 0x61], "PPT is 0xFF61");
    let lppt = u16::from_be_bytes([ppt[at + 2], ppt[at + 3]]);
    assert_eq!(lppt, 10, "Lppt counts Lppt, Zppt and Ippt: 2 + 1 + 7");
    assert!(
        (4..=65_535).contains(&u32::from(lppt)),
        "Table A.39 gives Lppt as 4 to 65 535"
    );
    assert_eq!(ppt[at + 4], 0, "Zppt");
    assert_eq!(&ppt[at + 5..at + 12], &j10_packed_headers()[..], "Ippt");
    assert_eq!(&ppt[at + 12..at + 14], &[0xFF, 0x93], "SOD after the PPT");
    let psot = u32::from_be_bytes([ppt[74], ppt[75], ppt[76], ppt[77]]);
    assert_eq!(psot, 35, "Psot: 12 SOT + 12 PPT + 2 SOD + 9 body bytes");
    assert_eq!(
        &ppt[octal(J10_SOT_AT) + psot as usize..],
        &[0xFF, 0xD9],
        "A.4.2: Psot reaches the EOC"
    );

    // Neither container moved a byte of J.10's data: the two streams
    // together are still the 16 bytes at octal 00122, rearranged.
    let mut rejoined = j10_packed_headers();
    rejoined.extend_from_slice(&j10_packed_bodies());
    assert_eq!(rejoined.len(), 16, "J.10.2: 16 bytes of compressed data");
}

/// **The published samples adjudicate the packed path.**
///
/// The header bytes are J.10's, their boundaries are J.10's, the body bytes
/// are J.10's and the nine expected samples are J.10.5's. Only the PPM
/// container is this repository's, and A.7.4 fixes its contents: "The
/// contents are exactly the packet header which would have been distributed
/// in the bit stream as described in B.10."
///
/// A decoder that read the packed stream as a bit stream, or that took a
/// header from the wrong one of the two, does not reach these nine numbers:
/// it fails the exact-consumption check on one stream or the other first,
/// and if it somehow passed both it would still have to land on J.10.5.
#[test]
fn annex_j10_packed_into_ppm_decodes_to_the_published_samples() {
    let mut warnings = Vec::new();
    let image = jpx_decode(&j10_with_ppm(), &Limits::new(1 << 20), &mut warnings)
        .expect("J.10's own bytes, with A.7.4's relocation applied");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(image.width, 1);
    assert_eq!(image.height, 9);
    assert_eq!(
        image.samples, &ANNEX_J10_SAMPLES,
        "T.800 J.10.5's nine samples, from a PPM codestream"
    );
}

/// The same, through A.7.5's tile-part header instead of A.7.4's main
/// header.
///
/// PPM and PPT are one mechanism in two places, and this is the half the
/// main header cannot reach: the stream is per tile, it is concatenated
/// across that tile's tile-parts, and it carries no `Nppm` series at all.
#[test]
fn annex_j10_packed_into_ppt_decodes_to_the_published_samples() {
    let mut warnings = Vec::new();
    let image = jpx_decode(&j10_with_ppt(), &Limits::new(1 << 20), &mut warnings)
        .expect("J.10's own bytes, with A.7.5's relocation applied");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        image.samples, &ANNEX_J10_SAMPLES,
        "T.800 J.10.5's nine samples, from a PPT codestream"
    );
}

/// All three of B.10's arrangements of J.10's picture decode to one answer.
///
/// B.10: "The packet headers appear in the codestream immediately preceding
/// the packet data, unless one of the PPM or PPT marker segments has been
/// used." Three places, one picture — and unlike a fixture written twice by
/// this repository, the three share every byte that carries image data.
#[test]
fn the_three_places_b10_allows_a_packet_header_decode_alike() {
    let limits = Limits::new(1 << 20);
    let mut warnings = Vec::new();
    let plain = jpx_decode(ANNEX_J10, &limits, &mut warnings).expect("J.10");
    let ppm = jpx_decode(&j10_with_ppm(), &limits, &mut warnings).expect("J.10 with PPM");
    let ppt = jpx_decode(&j10_with_ppt(), &limits, &mut warnings).expect("J.10 with PPT");
    assert_eq!(plain, ppm);
    assert_eq!(plain, ppt);
    assert_eq!(plain.samples, &ANNEX_J10_SAMPLES);
}

/// **The fixture bites.** A packed stream split one byte away from where
/// J.10 publishes the boundary is refused rather than decoded.
///
/// Without this, the two tests above would be consistent with a decoder that
/// ignored the PPM segment entirely and got lucky. They are not: moving one
/// byte across the seam in either direction — which is the mildest possible
/// mis-transcription of J.10.3 — leaves a codestream this build will not
/// decode, in both containers, in both directions.
///
/// It is also the check that would have caught J.10.3's own `0134`: a
/// transcription that believed the prose over Table J.21 puts the boundary
/// one byte late, which is exactly the `+1` case below.
#[test]
fn a_packed_stream_split_one_byte_wrong_is_refused() {
    for shift in [-1i32, 1] {
        for packed in ["PPM", "PPT"] {
            let all = {
                let mut v = j10_packed_headers();
                v.extend_from_slice(&j10_packed_bodies());
                v
            };
            let cut = usize::try_from(i32::try_from(j10_packed_headers().len()).unwrap() + shift)
                .expect("the seam stays inside the data");
            let (headers, bodies) = all.split_at(cut);
            let bytes = if packed == "PPM" {
                assemble_ppm(headers, bodies)
            } else {
                assemble_ppt(headers, bodies)
            };
            let mut warnings = Vec::new();
            assert!(
                jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings).is_err(),
                "{packed} with the seam moved by {shift} decoded, so the \
                 published boundary is not what is being checked"
            );
        }
    }
}

/// J.10's SOT segment, with `Psot`, `TPsot` and `TNsot` set.
///
/// Every other field is J.10's own: `Lsot` is 10 and `Isot` is tile 0.
fn j10_sot(psot: u32, tpsot: u8, tnsot: u8) -> Vec<u8> {
    let mut sot = ANNEX_J10[octal(J10_SOT_AT)..octal(J10_SOD_AT)].to_vec();
    sot[6..10].copy_from_slice(&psot.to_be_bytes());
    sot[10] = tpsot;
    sot[11] = tnsot;
    sot
}

/// A.7.4's series, split across two PPM segments that appear in the file in
/// *decreasing* `Zppm`, still decodes to J.10.5's samples.
///
/// Two clauses at once, on published bytes:
///
///   - "The sequence of (Nppm_i, Ippm_ij) parameters from this marker
///     segment is concatenated, in order of increasing Zppm" — so file
///     order is not the join order, and here the two are opposite.
///   - "the series of Ippm parameters described by the Nppm does not have to
///     be complete in a given marker segment. Therefore, it is possible that
///     the next PPM marker segment will not have an Nppm parameter after
///     Zppm, but the continuation of the Ippm series from the last PPM
///     marker segment" — the run is declared as 7 bytes in the segment that
///     carries only the first 3 of them.
///
/// The cut falls at 3, which is *inside* the `Nppm` run and exactly at the
/// boundary between J.10's two published packet headers. A parser that
/// joined in file order would hand the first packet `0xC0 0x7C 0x21`, and a
/// parser that read each segment alone would read `0xC0 0x7C 0x21 0x80` as a
/// 32-bit length. Neither reaches J.10.5.
#[test]
fn a_ppm_series_joins_by_zppm_across_segments_before_it_is_read() {
    let headers = j10_packed_headers();
    let bodies = j10_packed_bodies();
    let (first, rest) = headers.split_at(3);

    // Table A.38: Zppm, then the (Nppm, Ippm) series. Segment 0 declares the
    // whole 7-byte run and carries 3 bytes of it; segment 1 carries the
    // other 4 and no Nppm at all.
    let mut zero = vec![0u8]; // Zppm = 0
    zero.extend_from_slice(&7u32.to_be_bytes());
    zero.extend_from_slice(first);
    let mut one = vec![1u8]; // Zppm = 1
    one.extend_from_slice(rest);

    let mut out = ANNEX_J10[..octal(J10_SOT_AT)].to_vec();
    for tail in [&one, &zero] {
        out.extend_from_slice(&[0xFF, 0x60]);
        out.extend_from_slice(&u16::try_from(tail.len() + 2).expect("fits").to_be_bytes());
        out.extend_from_slice(tail);
    }
    // Both segments clear Table A.38's minimum Lppm of 7: 10 and 7.
    assert_eq!(zero.len() + 2, 10);
    assert_eq!(one.len() + 2, 7);

    out.extend_from_slice(&j10_sot(
        u32::try_from(12 + 2 + bodies.len()).expect("fits"),
        0,
        1,
    ));
    out.extend_from_slice(&[0xFF, 0x93]);
    out.extend_from_slice(&bodies);
    out.extend_from_slice(&[0xFF, 0xD9]);

    let mut warnings = Vec::new();
    let image = jpx_decode(&out, &Limits::new(1 << 20), &mut warnings)
        .expect("two PPM segments out of file order");
    assert_eq!(image.samples, &ANNEX_J10_SAMPLES);

    // The join order is not the file order, and the two differ: joining as
    // written would put J.10's second header first.
    let mut file_order = rest.to_vec();
    file_order.extend_from_slice(first);
    assert_ne!(file_order, headers);
}

/// A.7.4 and A.7.5: "Every marker segment in this series shall end with a
/// completed packet header."
///
/// Two PPT segments carrying the same seven bytes. Cut at 3 — the boundary
/// J.10.3 publishes between the two packet headers — the codestream decodes
/// to J.10.5's samples. Cut at 2, one byte inside the first header, it is
/// refused. **Nothing but the seam differs**: the `Ippt` bytes concatenate
/// to the same seven either way, so a decoder that ignored the seam would
/// decode both, and a decoder that got the seam wrong would decode neither.
#[test]
fn a_packed_segment_that_ends_inside_a_published_packet_header_is_refused() {
    let headers = j10_packed_headers();
    let bodies = j10_packed_bodies();

    let build = |cut: usize| {
        let (first, rest) = headers.split_at(cut);
        let mut header = Vec::new();
        for (z, part) in [(0u8, first), (1u8, rest)] {
            header.extend_from_slice(&[0xFF, 0x61]);
            let lppt = u16::try_from(part.len() + 3).expect("fits");
            // Table A.39: Lppt is 4 to 65 535, and both cuts clear it.
            assert!(lppt >= 4, "Lppt {lppt} is below Table A.39's minimum");
            header.extend_from_slice(&lppt.to_be_bytes());
            header.push(z);
            header.extend_from_slice(part);
        }
        let mut out = ANNEX_J10[..octal(J10_SOT_AT)].to_vec();
        let psot = u32::try_from(12 + header.len() + 2 + bodies.len()).expect("fits");
        out.extend_from_slice(&j10_sot(psot, 0, 1));
        out.extend_from_slice(&header);
        out.extend_from_slice(&[0xFF, 0x93]);
        out.extend_from_slice(&bodies);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    };

    let limits = Limits::new(1 << 20);
    let mut warnings = Vec::new();
    let good = jpx_decode(&build(3), &limits, &mut warnings)
        .expect("a seam at J.10's published header boundary");
    assert_eq!(good.samples, &ANNEX_J10_SAMPLES);
    assert!(
        jpx_decode(&build(2), &limits, &mut warnings).is_err(),
        "a PPT segment ending one byte inside a packet header was accepted"
    );
}

/// A.7.4's `Nppm` series across two tile-parts, on published bytes.
///
/// J.10's tile has two packets, so B.11's "divisions between tile-parts must
/// occur at packet boundaries" allows one body in each part. "The kth entry
/// in the resulting list contains the number of bytes and packet headers for
/// the kth tile-part appearing in the codestream", so the runs are 3 and 4 —
/// the sizes J.10.3 publishes for the two headers.
///
/// Swapping them to 4 and 3 keeps every byte of the codestream the same and
/// moves only the seam, one byte into the second header. It is refused.
#[test]
fn the_nppm_runs_of_two_tile_parts_are_the_published_header_lengths() {
    let headers = j10_packed_headers();
    let first_body = J10_SPANS[1].2;
    let second_body = J10_SPANS[3].2;

    let build = |runs: [usize; 2]| {
        let mut tail = vec![0u8]; // Zppm
        let mut at = 0;
        for n in runs {
            tail.extend_from_slice(&u32::try_from(n).expect("fits").to_be_bytes());
            tail.extend_from_slice(&headers[at..at + n]);
            at += n;
        }
        let mut out = ANNEX_J10[..octal(J10_SOT_AT)].to_vec();
        out.extend_from_slice(&[0xFF, 0x60]);
        out.extend_from_slice(&u16::try_from(tail.len() + 2).expect("fits").to_be_bytes());
        out.extend_from_slice(&tail);
        for (index, body) in [(0u8, first_body), (1u8, second_body)] {
            let psot = u32::try_from(12 + 2 + body.len()).expect("fits");
            out.extend_from_slice(&j10_sot(psot, index, 2));
            out.extend_from_slice(&[0xFF, 0x93]);
            out.extend_from_slice(body);
        }
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    };

    let limits = Limits::new(1 << 20);
    let mut warnings = Vec::new();
    let good = jpx_decode(&build([3, 4]), &limits, &mut warnings)
        .expect("J.10's two packets, one per tile-part, headers in a PPM");
    assert_eq!(good.samples, &ANNEX_J10_SAMPLES);
    assert!(
        jpx_decode(&build([4, 3]), &limits, &mut warnings).is_err(),
        "an Nppm run ending one byte inside a packet header was accepted"
    );
}

/// A.7.5's segments are joined "in the order of increasing Zppt", not in the
/// order they appear in the tile-part header.
///
/// The mirror of [`a_ppm_series_joins_by_zppm_across_segments_before_it_is_read`]
/// for the other marker, and it needs J.10's bytes for the same reason: two
/// segments of identical bytes are the same in either order, so only a split
/// whose halves differ can fail. J.10's two packet headers differ in every
/// byte.
#[test]
fn ppt_segments_join_by_zppt_rather_than_by_file_order() {
    let headers = j10_packed_headers();
    let bodies = j10_packed_bodies();
    let (first, rest) = headers.split_at(3);

    // Zppt = 1 written first, Zppt = 0 second.
    let mut header = Vec::new();
    for (z, part) in [(1u8, rest), (0u8, first)] {
        header.extend_from_slice(&[0xFF, 0x61]);
        let lppt = u16::try_from(part.len() + 3).expect("fits");
        assert!(lppt >= 4, "Table A.39 gives Lppt as 4 to 65 535");
        header.extend_from_slice(&lppt.to_be_bytes());
        header.push(z);
        header.extend_from_slice(part);
    }

    let mut out = ANNEX_J10[..octal(J10_SOT_AT)].to_vec();
    let psot = u32::try_from(12 + header.len() + 2 + bodies.len()).expect("fits");
    out.extend_from_slice(&j10_sot(psot, 0, 1));
    out.extend_from_slice(&header);
    out.extend_from_slice(&[0xFF, 0x93]);
    out.extend_from_slice(&bodies);
    out.extend_from_slice(&[0xFF, 0xD9]);

    let mut warnings = Vec::new();
    let image = jpx_decode(&out, &Limits::new(1 << 20), &mut warnings)
        .expect("two PPT segments written in decreasing Zppt");
    assert_eq!(image.samples, &ANNEX_J10_SAMPLES);

    // File order and Zppt order genuinely differ here, so a decoder that
    // joined as written would hand the first packet J.10's second header.
    let mut file_order = rest.to_vec();
    file_order.extend_from_slice(first);
    assert_ne!(file_order, headers);
}
