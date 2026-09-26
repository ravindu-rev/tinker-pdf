//! T.800 A.6.6's progression order change, adjudicated against Annex J.10.
//!
//! # What the standard publishes for POC, and what it does not
//!
//! **It publishes no codestream that carries one.** Every annex was searched.
//! `0xFF5F` appears exactly twice in the 231-page Recommendation — in Table
//! A.2's marker list and in Table A.32's parameter list — and the only
//! codestream T.800 prints anywhere is J.10's, whose Scod, SGcod and marker
//! sequence carry no POC. Outside Annexes A and B the string "POC" does not
//! occur at all: not in Annex J's fifteen subclauses, not in K's bibliography.
//! So the roadmap's claim held here, as it held for ROI, and it was tested
//! rather than believed.
//!
//! **What it does publish is a POC's parameter values, twice, and they are
//! not test data.** Table A.45 constrains Profile-0 — "If the POC marker is
//! present, the POC marker shall have RSPOC0 = 0 and CSPOC0 = 0" — and Table
//! A.46 gives, for the 4K digital cinema profile, "exactly two progressions":
//! `RSpoc = 0, CSpoc = 0, LYEpoc = 1, REpoc = NL, CEpoc = 3, Ppoc = 4` and
//! `RSpoc = NL, CSpoc = 0, LYEpoc = 1, REpoc = NL+1, CEpoc = 3, Ppoc = 4`.
//! Those are field values for a profile, with no image, no bytes and no
//! decoded result, so they can check a transcription's plausibility and
//! nothing else. [`table_a46s_published_progressions_survive_a_round_trip`]
//! uses them for exactly that and claims no more.
//!
//! # What can adjudicate a progression change, and why J.10 can
//!
//! A progression order change acts on packet *order* rather than on
//! coefficients, so J.10.4's published intermediate coefficients — the lever
//! that made Annex H adjudicable — are no use here. What is needed instead is
//! a published **packet boundary**, and J.10 has one:
//!
//! - Table J.20 lists the first packet header's three bytes, `0xC7`, `0xD4`,
//!   `0x0C`, and J.10.3 concludes "Decoding the first packet header requires
//!   3 bytes and indicates that 6 bytes of arithmetic coded compressed data
//!   are used for the only code-block in this packet."
//! - J.10.4: "The bytes provided to the arithmetic coder are those beginning
//!   at offset 0125", printed as `01 8F0D C875 5D`.
//! - Table J.21 lists the second packet header's four bytes, `0xC0`, `0x7C`,
//!   `0x21`, `0x80`.
//! - J.10.4 again: "The compressed data for the only code-block in the second
//!   packet, representing the vertical high pass horizontal lowpass sub-band
//!   begins at offset 0137 octal", printed as `0F B176`.
//!
//! So J.10's sixteen bytes of tile data are two packets of published extent,
//! nine bytes then seven, and **the two packets can be swapped**.
//!
//! **The brief this lane was given guessed that they could not**, and the
//! guess is worth recording because it is nearly right. J.10 is one layer,
//! one component and one precinct, and along those three axes there is
//! nothing to reorder. It is **not** single-resolution: J.10.1's COD declares
//! one decomposition level, so B.12 gives it two resolution levels and
//! therefore exactly two packets — and (B-21)'s resolution bound is enough to
//! express a progression change over them. One axis with two values is all a
//! reordering needs.
//!
//! The reordered codestream is conforming. Its first volume has `RSpoc = 1`,
//! which Table A.45 forbids for Profile-0; J.10's `Rsiz` is `0x0000`, which
//! Table A.10 reads as "Capabilities specified in this Recommendation |
//! International Standard only", so no profile restriction reaches it. Table
//! A.45's own NOTE 3 says why that profile has the rule and why a general
//! decoder does not: "Some compliant decoders might decode only packets
//! associated with the first progression."
//!
//! # The chain, link by link (ruling 13)
//!
//! 1. **Bytes to packets — adjudicated.** The fixtures are J.10's own
//!    hundred bytes with a POC marker segment inserted in the main header (or
//!    in the tile-part header) and the two published packets placed in the
//!    order the POC describes. Nothing inside either packet is touched, and
//!    where each one starts and ends is J.10.3's and J.10.4's rather than
//!    this file's.
//! 2. **POC bytes to progression order volumes — adjudicated.** Figure A.15
//!    fixes the field order inside a progression and Table A.32 fixes each
//!    field's width and range; both are transcribed below and
//!    [`the_poc_segment_matches_figure_a15s_field_offsets`] asserts every
//!    field sits where the syntax requires **before** any decode runs.
//! 3. **Volumes to a packet sequence — adjudicated.** (B-21) bounds the three
//!    loops, and B.12.2 fixes the order between volumes: "All the packets
//!    included in the entire progression order volume are found in order in
//!    the codestream before the next progression order change takes effect."
//! 4. **Sequence to samples — adjudicated.** The answer demanded is J.10.5's
//!    nine published samples, the same nine the untouched codestream
//!    produces.
//!
//! **Where self-consistency starts, stated plainly**: one `jpx_decode`
//! produces both the base decode and every POC decode, so a defect that moved
//! J.10's own samples would fail [`the_base_decodes_to_the_samples_j10_publishes`]
//! first. What is left is the narrow possibility of a defect that leaves the
//! base decode exact and happens to move a reordered decode onto the same
//! nine bytes — which, since the reordered stream feeds the two packets to
//! different resolution levels, would mean two different sub-bands
//! reconstructing one image.
//! [`skipping_the_poc_decodes_a_different_picture_and_says_nothing`] pins that
//! the reordering is doing real work: the same reordered bytes with the POC
//! removed decode, cleanly and silently, to nine different samples.
//!
//! # Provenance
//!
//! ITU-T Rec. T.800 (11/2015) = ISO/IEC 15444-1:2016, SHA-256
//! `b1ca01eecd3fe13ad58ea2e253c19274eb091e4a3e240b489941c664e96869e0`,
//! 4 789 495 bytes, byte-identical to the copy `jpx_annex_j.rs` records as
//! fetched from the ITU on 13 September 2026 and `jpx_annex_h.rs` cites
//! again. This repository's own `tpdf info` reads it as 231 pages titled
//! "ITU-T Rec. T.800 (11/2015) Information technology – JPEG 2000 image
//! coding system: Core coding system"; its pages are headed ISO/IEC
//! 15444-1:2016 (E). The Recommendation is published free of charge.
//!
//! Read with this repository's own `tpdf text`, and **every clause quoted
//! here was read a second time from `tpdf render` of its page with a Times
//! face supplied**, because this document's text layer drops its mathematics:
//! equation (A-6) extracts as `72`/`92` with its two branches interleaved,
//! and (B-21) extracts as a run of bare identifiers with the inequalities
//! gone. Pages 39, 48, 52, 80 and 82 of the PDF — printed pages 31, 40, 44,
//! 72 and 74 — were read as images, and page 52 is why: the text layer
//! interleaves Table A.46's five profile columns, so which profile owns which
//! POC parameter set is not recoverable from it.

use tinker_pdf_filters::{jpx_decode, Limits};

// --- T.800 J.10's codestream ----------------------------------------------

/// T.800 J.10's codestream, transcribed field by field from J.10.1 and
/// J.10.2's annotated listings rather than from the raw hex dump.
///
/// A third independent transcription of the same hundred bytes: `jpx_annex_j.rs`
/// and `jpx_annex_h.rs` carry the other two, and three test binaries cannot
/// check each other, so each is held to J.10's own octal offsets and to
/// J.10.5's published samples on its own.
const ANNEX_J10: &[u8] = &[
    // 00000  SOC
    0xFF, 0x4F, //
    // 00002  SIZ
    0xFF, 0x51, //
    // 00004  Lsiz = 41
    0x00, 0x29, //
    // 00006  Rsiz = 0: Table A.10's "Capabilities specified in this
    //        Recommendation | International Standard only" — no profile
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
    // 00062  SPqcd: exponents 8, 9, 9, 10 for LL, HL, LH, HH
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
    // 00077  Number of decomposition levels = 1, so two resolution levels
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
    // 00122  First packet: Table J.20's three header bytes, then J.10.4's six
    //        body bytes at octal 0125
    0xC7, 0xD4, 0x0C, //
    0x01, 0x8F, 0x0D, 0xC8, 0x75, 0x5D, //
    // 00133  Second packet: Table J.21's four header bytes, then J.10.4's
    //        three body bytes at octal 0137
    0xC0, 0x7C, 0x21, 0x80, //
    0x0F, 0xB1, 0x76, //
    // 00142  EOC
    0xFF, 0xD9,
];

/// T.800 J.10.5: "After the inverse 5-3 reversible filter and level shifting,
/// the component samples in decimal are: 101, 103, 104, 105, 96, 97, 96, 102,
/// 109".
const J10_SAMPLES: [u8; 9] = [101, 103, 104, 105, 96, 97, 96, 102, 109];

/// An offset J.10 writes in octal, as a byte index.
fn octal(at: &str) -> usize {
    usize::from_str_radix(at, 8).expect("J.10 writes its offsets in octal")
}

/// The transcription is checked against J.10's own octal offsets before it is
/// trusted to check anything else.
#[test]
fn the_transcription_matches_j10s_annotated_offsets() {
    let named: &[(&str, &str, &[u8])] = &[
        ("00000", "SOC", &[0xFF, 0x4F]),
        ("00004", "Lsiz", &[0x00, 0x29]),
        ("00006", "Rsiz", &[0x00, 0x00]),
        ("00050", "Csiz", &[0x00, 0x01]),
        ("00055", "QCD", &[0xFF, 0x5C]),
        ("00066", "COD", &[0xFF, 0x52]),
        ("00073", "the progression order", &[0x00]),
        ("00074", "the layer count", &[0x00, 0x01]),
        ("00077", "the decomposition level count", &[0x01]),
        ("00104", "SOT", &[0xFF, 0x90]),
        ("00112", "Psot", &[0x00, 0x00, 0x00, 0x1E]),
        ("00120", "SOD", &[0xFF, 0x93]),
        ("00142", "EOC", &[0xFF, 0xD9]),
    ];
    for (at, field, bytes) in named {
        let i = octal(at);
        assert_eq!(
            &ANNEX_J10[i..i + bytes.len()],
            *bytes,
            "{field} should sit at octal {at} (byte {i})"
        );
    }
    assert_eq!(ANNEX_J10.len(), octal("00142") + 2);
}

// --- J.10's two packets, at the boundaries J.10.3 and J.10.4 publish ------

/// Octal 00104, where J.10.2 says the tile-part begins: "The first and only
/// tile-part header begins at byte 0104 octal with the SOT marker".
const TILE_PART: &str = "00104";

/// Octal 00122, where J.10.2 says the compressed data begins: "The next 16
/// bytes are compressed data (30-byte length – 14 bytes of marker segments)".
const DATA: &str = "00122";

/// The first packet: Table J.20's three header bytes and the six body bytes
/// J.10.4 prints at octal 0125, so nine bytes running 00122..00133.
fn first_packet() -> &'static [u8] {
    &ANNEX_J10[octal(DATA)..octal("00133")]
}

/// The second packet: Table J.21's four header bytes and the three body bytes
/// J.10.4 prints at octal 0137, so seven bytes running 00133..00142.
fn second_packet() -> &'static [u8] {
    &ANNEX_J10[octal("00133")..octal("00142")]
}

/// The split into two packets is J.10's, not this file's.
///
/// J.10.3 ends "Thus the next packet header begins at offset 0134", which is
/// one past where its own arithmetic lands: three header bytes from 0122 and
/// six body bytes from 0125 end at 0133, and Table J.21 immediately below
/// prints `0xC0` as the second header's first byte — the byte at 0133 and not
/// the byte at 0134. `jpx_annex_j.rs` records the same slip. The offsets
/// J.10.4 states for the two *bodies*, 0125 and 0137, are consistent with
/// 0133 and are what this test uses.
#[test]
fn the_two_packets_sit_where_j10_prints_them() {
    assert_eq!(
        first_packet(),
        &[0xC7, 0xD4, 0x0C, 0x01, 0x8F, 0x0D, 0xC8, 0x75, 0x5D],
        "Table J.20's three header bytes and J.10.4's six body bytes at 0125"
    );
    assert_eq!(
        second_packet(),
        &[0xC0, 0x7C, 0x21, 0x80, 0x0F, 0xB1, 0x76],
        "Table J.21's four header bytes and J.10.4's three body bytes at 0137"
    );
    // J.10.2: "The next 16 bytes are compressed data".
    assert_eq!(first_packet().len() + second_packet().len(), 16);
}

/// The base fixture is the standard's, and decodes to the standard's answer.
///
/// Link 4 of the chain in the module note, run on the untouched bytes. Every
/// fixture below is these bytes rearranged, so a transcription slip fails
/// here first.
#[test]
fn the_base_decodes_to_the_samples_j10_publishes() {
    let mut warnings = Vec::new();
    let image = jpx_decode(ANNEX_J10, &Limits::new(1 << 20), &mut warnings)
        .expect("T.800 J.10's own codestream decodes");
    assert_eq!(image.samples, &J10_SAMPLES, "T.800 J.10.5");
    assert!(warnings.is_empty(), "{warnings:?}");
}

// --- A.6.6's POC marker segment, transcribed ------------------------------

/// One progression of a POC marker segment, as Figure A.15 orders its fields.
///
/// **Figure A.15's order is `RSpoc`, `CSpoc`, `LYEpoc`, `REpoc`, `CEpoc`,
/// `Ppoc`** — read from the figure itself rather than from the parameter
/// list beneath it, which explains `LYEpoc` before `REpoc` but is prose
/// rather than syntax. Table A.32's row order agrees with the figure.
///
/// The widths are Table A.32's: `RSpoc` 8 bits, `CSpoc` 8 or 16 depending on
/// `Csiz`, `LYEpoc` 16, `REpoc` 8, `CEpoc` 8 or 16, `Ppoc` 8. J.10's `Csiz`
/// is 1, so the two component indices are the narrow form and one progression
/// is seven bytes — which is equation (A-6)'s `2 + 7 ·
/// number_progression_order_change` with the `Lpoc` field's own two taken out.
#[derive(Clone, Copy)]
struct Progression {
    /// `RSpoc`, "Resolution level index (inclusive) for the start of a
    /// progression".
    rs: u8,
    /// `CSpoc`, "Component index (inclusive) for the start of a progression".
    cs: u8,
    /// `LYEpoc`, "Layer index (exclusive) for the end of a progression".
    lye: u16,
    /// `REpoc`, "Resolution level index (exclusive) for the end of a
    /// progression".
    re: u8,
    /// `CEpoc`, "Component index (exclusive) for the end of a progression".
    ce: u8,
    /// `Ppoc`, a Table A.16 progression order — the same eight bits `SGcod`
    /// carries, 0 being layer-resolution level-component-position.
    ppoc: u8,
}

impl Progression {
    fn bytes(self) -> Vec<u8> {
        let mut out = vec![self.rs, self.cs];
        out.extend_from_slice(&self.lye.to_be_bytes());
        out.extend_from_slice(&[self.re, self.ce, self.ppoc]);
        out
    }
}

/// A whole POC marker segment: `0xFF5F`, `Lpoc`, then the progressions.
///
/// `Lpoc` is computed from (A-6) rather than from the buffer length, so a
/// disagreement between the equation and the bytes would show up here.
fn poc(progressions: &[Progression]) -> Vec<u8> {
    let mut out = vec![0xFF, 0x5F];
    let lpoc = 2 + 7 * progressions.len();
    out.extend_from_slice(
        &u16::try_from(lpoc)
            .expect("Table A.32: Lpoc is 9 to 65 535")
            .to_be_bytes(),
    );
    for p in progressions {
        out.extend_from_slice(&p.bytes());
    }
    out
}

/// Every field of a POC segment sits where Figure A.15 puts it, before any
/// decode has been asked to read one.
///
/// This is the same discipline `jpx_annex_j.rs` applies to J.10's own fields:
/// the syntax is checked against the clause by arithmetic on offsets, so a
/// fixture that happens to decode cannot cover for a field written in the
/// wrong place.
#[test]
fn the_poc_segment_matches_figure_a15s_field_offsets() {
    let segment = poc(&[
        Progression {
            rs: 1,
            cs: 0,
            lye: 1,
            re: 2,
            ce: 1,
            ppoc: 0,
        },
        Progression {
            rs: 0,
            cs: 0,
            lye: 1,
            re: 1,
            ce: 1,
            ppoc: 0,
        },
    ]);

    // Table A.32: POC is 16 bits, 0xFF5F.
    assert_eq!(&segment[0..2], &[0xFF, 0x5F]);
    // Equation (A-6): Lpoc = 2 + 7 · number_progression_order_change for
    // Csiz < 257, so 16 for two progressions — and Table A.32's "9 to 65 535"
    // has 9 as the one-progression case.
    assert_eq!(&segment[2..4], &16u16.to_be_bytes());
    assert_eq!(segment.len(), 2 + 16, "Lpoc excludes the marker itself");

    // Figure A.15, first progression: RSpoc, CSpoc, LYEpoc, REpoc, CEpoc,
    // Ppoc at 4, 5, 6..8, 8, 9, 10.
    assert_eq!(segment[4], 1, "RSpoc0");
    assert_eq!(segment[5], 0, "CSpoc0");
    assert_eq!(&segment[6..8], &1u16.to_be_bytes(), "LYEpoc0");
    assert_eq!(segment[8], 2, "REpoc0");
    assert_eq!(segment[9], 1, "CEpoc0");
    assert_eq!(segment[10], 0, "Ppoc0");
    // The second progression begins seven bytes later, which is the same
    // seven (A-6) multiplies by.
    assert_eq!(segment[11], 0, "RSpoc1");
    assert_eq!(segment[12], 0, "CSpoc1");
    assert_eq!(&segment[13..15], &1u16.to_be_bytes(), "LYEpoc1");
    assert_eq!(segment[15], 1, "REpoc1");
    assert_eq!(segment[16], 1, "CEpoc1");
    assert_eq!(segment[17], 0, "Ppoc1");
}

// --- the fixtures ---------------------------------------------------------

/// J.10's codestream with `main` appended to its main header, `tile` appended
/// to its tile-part header, and `data` as the tile's packets.
///
/// `Psot` is rewritten from the finished tile-part, which is A.4.2's own
/// definition — "the length, in bytes, from the beginning of the first byte
/// of this SOT marker segment of the tile-part to the end of the data of that
/// tile-part" — so a fixture that changes the tile-part's size stays
/// self-describing. Every fixture here leaves it at J.10's 30, because the
/// sixteen bytes of packets are rearranged rather than resized.
fn rebuilt(main: &[u8], tile: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = ANNEX_J10[..octal(TILE_PART)].to_vec();
    out.extend_from_slice(main);
    let mut part = ANNEX_J10[octal(TILE_PART)..octal("00120")].to_vec();
    part.extend_from_slice(tile);
    part.extend_from_slice(&[0xFF, 0x93]); // SOD
    part.extend_from_slice(data);
    let psot = u32::try_from(part.len()).expect("a tile-part inside 4 GiB");
    part[6..10].copy_from_slice(&psot.to_be_bytes());
    out.extend_from_slice(&part);
    out.extend_from_slice(&[0xFF, 0xD9]); // EOC
    out
}

/// The two progressions that put resolution level 1 before resolution level
/// 0, and the sixteen published bytes in that order.
///
/// (B-21) over J.10's geometry: the first volume is `0 <= i < 1`,
/// `1 <= r < 2`, `0 <= l < 1`, which is the second packet alone; the second
/// volume is `0 <= i < 1`, `0 <= r < 1`, `0 <= l < 1`, which is the first.
/// B.12.2 fixes the order between them: "All the packets included in the
/// entire progression order volume are found in order in the codestream
/// before the next progression order change takes effect."
fn reversed_progressions() -> [Progression; 2] {
    [
        Progression {
            rs: 1,
            cs: 0,
            lye: 1,
            re: 2,
            ce: 1,
            ppoc: 0,
        },
        Progression {
            rs: 0,
            cs: 0,
            lye: 1,
            re: 1,
            ce: 1,
            ppoc: 0,
        },
    ]
}

fn reversed_data() -> Vec<u8> {
    let mut data = second_packet().to_vec();
    data.extend_from_slice(first_packet());
    data
}

fn decodes_to_j10(bytes: &[u8], what: &str) {
    let mut warnings = Vec::new();
    let image = jpx_decode(bytes, &Limits::new(1 << 20), &mut warnings)
        .unwrap_or_else(|e| panic!("{what} should decode: {e:?}"));
    assert_eq!(image.width, 1, "J.10.1: 1 sample horizontally");
    assert_eq!(image.height, 9, "J.10.1: 9 samples vertically");
    assert_eq!(image.samples, &J10_SAMPLES, "T.800 J.10.5, via {what}");
    assert!(warnings.is_empty(), "{what}: {warnings:?}");
}

/// **The adjudicated one.** J.10's two published packets, swapped, with a
/// main-header POC describing the swap — decoding to J.10.5's nine published
/// samples.
///
/// Every link is the standard's: the bytes are J.10's, the boundary between
/// them is J.10.3's and J.10.4's, the two progressions are (B-21) applied to
/// A.6.6's fields, and the answer demanded is J.10.5's.
///
/// B.12.3 is what makes a main-header POC bind this tile: "If the POC marker
/// segment is found in the main header, it overrides the progression found in
/// the COD for all tiles."
#[test]
fn a_main_header_poc_reorders_j10s_two_packets() {
    let bytes = rebuilt(&poc(&reversed_progressions()), &[], &reversed_data());
    decodes_to_j10(&bytes, "a main-header POC reversing the resolution levels");
}

/// The same reordering signalled in the tile-part header instead.
///
/// A.6.6: "If a POC is used to describe the progression of a particular tile,
/// a POC marker segment must appear in the first tile-part header of that
/// tile." J.10 has one tile with one tile-part, so that header is this one.
#[test]
fn a_tile_part_poc_reorders_them_too() {
    let bytes = rebuilt(&[], &poc(&reversed_progressions()), &reversed_data());
    decodes_to_j10(&bytes, "a tile-part POC reversing the resolution levels");
}

/// **A.6.6's precedence, with the two POCs disagreeing on purpose.**
///
/// > Tile-part POC > Main POC > Tile-part COD > Main COD
/// >
/// > where the "greater than" sign > means that the greater overrides the
/// > lesser marker segment.
///
/// B.12.3 says the same from the other end: with a tile-part POC, "The COD
/// progression order **and the main header POC marker segment (if there is
/// one) are overridden**".
///
/// The main header here describes J.10's own order and the tile-part header
/// describes the reversal, and the bytes are reversed. A decoder that took
/// the main header's would read the second packet's header as the first's and
/// could not produce these nine samples; a decoder that merged the two would
/// have four volumes and too many packets for sixteen bytes.
#[test]
fn a_tile_part_poc_overrides_a_main_header_poc_that_disagrees() {
    let main_says_j10s_own_order = [Progression {
        rs: 0,
        cs: 0,
        lye: 1,
        re: 2,
        ce: 1,
        ppoc: 0,
    }];
    let bytes = rebuilt(
        &poc(&main_says_j10s_own_order),
        &poc(&reversed_progressions()),
        &reversed_data(),
    );
    decodes_to_j10(&bytes, "a tile-part POC overriding a main-header POC");
}

/// A POC that restates J.10's own order changes nothing.
///
/// One volume covering everything is B.12.2's description of the default —
/// "The progression loops of B.12.1 all go from zero to the maximum value" —
/// so this is the codestream saying explicitly what it already said, and the
/// bytes are J.10's in J.10's order.
#[test]
fn a_poc_restating_the_default_order_changes_nothing() {
    let whole = [Progression {
        rs: 0,
        cs: 0,
        lye: 1,
        re: 2,
        ce: 1,
        ppoc: 0,
    }];
    let mut data = first_packet().to_vec();
    data.extend_from_slice(second_packet());
    decodes_to_j10(
        &rebuilt(&poc(&whole), &[], &data),
        "a POC restating B.12.1's own loops",
    );
}

/// The two resolution levels split across two volumes, in J.10's own order.
///
/// This is the shape Table A.46 requires of the 4K digital cinema profile —
/// one volume for the low resolution levels and one for the top — and with
/// two resolution levels it is `0 <= r < 1` then `1 <= r < 2`. The packets
/// are in J.10's published order because that is what the split describes.
#[test]
fn two_volumes_in_the_published_order_keep_the_published_bytes() {
    let split = [
        Progression {
            rs: 0,
            cs: 0,
            lye: 1,
            re: 1,
            ce: 1,
            ppoc: 0,
        },
        Progression {
            rs: 1,
            cs: 0,
            lye: 1,
            re: 2,
            ce: 1,
            ppoc: 0,
        },
    ];
    let mut data = first_packet().to_vec();
    data.extend_from_slice(second_packet());
    decodes_to_j10(&rebuilt(&poc(&split), &[], &data), "two ascending volumes");
}

/// **A.6.6's "not included again" rule, with a volume that repeats one.**
///
/// > LYEpoc: Layer index (exclusive) for the end of a progression. The layer
/// > index always starts at zero for every progression. **Packets that have
/// > already been included in the codestream are not included again.**
///
/// B.12.2 states the consequence: "No packet is ever repeated in the
/// codestream. Therefore, the layer always starts with the next one for a
/// given tile-component, resolution level and precinct."
///
/// The second volume here covers everything the first one did. A decoder that
/// took the volumes literally would expect four packets from sixteen bytes
/// that hold two, and would refuse the tile on the packet-length check rather
/// than produce these nine samples — so this fixture cannot pass by accident.
#[test]
fn a_volume_repeating_an_earlier_one_emits_nothing_new() {
    let whole = Progression {
        rs: 0,
        cs: 0,
        lye: 1,
        re: 2,
        ce: 1,
        ppoc: 0,
    };
    let mut data = first_packet().to_vec();
    data.extend_from_slice(second_packet());
    decodes_to_j10(
        &rebuilt(&poc(&[whole, whole]), &[], &data),
        "a repeated progression order volume",
    );
}

/// **A volume may name more than the tile has**, and the surplus describes no
/// packets.
///
/// B.12.3: "the POC marker segments may describe more progression order
/// volumes than exist in the codestream". Table A.32's ceilings are constants
/// — `REpoc` runs to 33, `CEpoc` to 255 — rather than this image's geometry,
/// so an encoder asking for "to the end" writes the constant. Here one volume
/// asks for thirty-three resolution levels and 255 components of a codestream
/// with two and one.
#[test]
fn a_volume_wider_than_the_tile_describes_no_extra_packets() {
    let everything = [Progression {
        rs: 0,
        cs: 0,
        lye: 1,
        re: 33,
        ce: 255,
        ppoc: 0,
    }];
    let mut data = first_packet().to_vec();
    data.extend_from_slice(second_packet());
    decodes_to_j10(
        &rebuilt(&poc(&everything), &[], &data),
        "a volume at Table A.32's ceilings",
    );
}

/// **The cost of skipping a POC, measured rather than asserted.**
///
/// The same sixteen published bytes in the same swapped order, with the POC
/// segment removed, **decode cleanly** — no refusal, no warning, nine
/// eight-bit samples — to a *different* picture:
///
/// ```text
/// J.10.5's samples   101  103  104  105   96   97   96  102  109
/// with the POC gone  128  130  132  139  128  124  143   97  153
/// ```
///
/// This tree has said for a year that a skipped POC "changes the packet order
/// mid-stream and mis-parses every packet after it", and that the result is
/// "a soft, plausible image" rather than an error. Those were arguments from
/// the shape of tier-2. This is the thing itself, on the standard's own
/// bytes: a nine-sample greyscale ramp that no check in this decoder objects
/// to, off by up to 44 levels out of 255, with the same warnings a correct
/// decode leaves — none.
///
/// It is also what makes the six fixtures above mean something. They read the
/// same sixteen bytes; if the POC were ignored they would all land here.
#[test]
fn skipping_the_poc_decodes_a_different_picture_and_says_nothing() {
    let mut warnings = Vec::new();
    let image = jpx_decode(
        &rebuilt(&[], &[], &reversed_data()),
        &Limits::new(1 << 20),
        &mut warnings,
    )
    .expect("the mis-ordered stream decodes, which is the whole problem");
    assert_eq!(
        image.samples,
        &[128, 130, 132, 139, 128, 124, 143, 97, 153],
        "the picture a decoder that ignored A.6.6 would draw"
    );
    assert_ne!(image.samples, &J10_SAMPLES, "T.800 J.10.5");
    assert!(
        warnings.is_empty(),
        "nothing objected, which is the point: {warnings:?}"
    );
}

// --- Table A.46's published progressions ----------------------------------

/// Table A.46's 4K digital cinema profile POC, through the transcription.
///
/// **This adjudicates a transcription and nothing else**, and saying so is
/// the point of putting it in its own section. Table A.46 gives field values
/// for a profile — no image, no bytes, no decoded result — so the most it can
/// do is confirm that the six fields this file writes are the six the table
/// names, in Figure A.15's order and at Table A.32's widths.
///
/// > There shall be exactly one POC marker segment in the main header. Other
/// > POC marker segments are disallowed. The POC marker segment shall specify
/// > exactly two progressions which have the following parameters:
/// > First progression: RSpoc = 0, CSpoc = 0, LYEpoc = 1, REpoc = NL,
/// > CEpoc = 3, Ppoc = 4
/// > Second progression: RSpoc = NL, CSpoc = 0, LYEpoc = 1, REpoc = NL + 1,
/// > CEpoc = 3, Ppoc = 4
///
/// `Ppoc = 4` is Table A.16's component-position-resolution level-layer
/// progression. `NL` is the decomposition level count, taken as 5 here purely
/// to have a number; the table states the parameters symbolically.
#[test]
fn table_a46s_published_progressions_survive_a_round_trip() {
    const NL: u8 = 5;
    let segment = poc(&[
        Progression {
            rs: 0,
            cs: 0,
            lye: 1,
            re: NL,
            ce: 3,
            ppoc: 4,
        },
        Progression {
            rs: NL,
            cs: 0,
            lye: 1,
            re: NL + 1,
            ce: 3,
            ppoc: 4,
        },
    ]);
    assert_eq!(
        segment,
        vec![
            0xFF,
            0x5F, // POC
            0x00,
            0x10, // Lpoc = 2 + 7 · 2
            0x00,
            0x00,
            0x00,
            0x01,
            NL,
            0x03,
            0x04, // first progression
            NL,
            0x00,
            0x00,
            0x01,
            NL + 1,
            0x03,
            0x04, // second progression
        ]
    );
    // Table A.45's Profile-0 parsability rule, which Table A.46's profiles do
    // not inherit: "If the POC marker is present, the POC marker shall have
    // RSPOC0 = 0 and CSPOC0 = 0." This one satisfies it; the reordering
    // fixtures above deliberately do not, and J.10's Rsiz of 0 is why they
    // may not (Table A.10).
    assert_eq!((segment[4], segment[5]), (0, 0), "RSPOC0 and CSPOC0");
}
