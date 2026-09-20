//! T.800 Annex H's region of interest, adjudicated against Annex J.10.
//!
//! # What the standard publishes for ROI, and what it does not
//!
//! **It publishes no test data at all.** Every annex was searched. Annex H —
//! "Coding of images with regions of interest", the normative one — is prose
//! and seven equations: H.1 is the decoder, H.2 is the encoder, H.3 is the
//! mask, and Figures H.1 and H.2 draw which wavelet coefficients a mask has
//! to contain rather than any coefficient's value. There is no worked
//! example, no sample, no byte. K.4, "Region of interest coding", is five
//! bibliography entries. J.9 mentions ROIs only as a way of *interpreting*
//! multiple components in SAR data. `0xFF5E` appears exactly twice in the
//! whole 231-page Recommendation — in Table A.2 and in Table A.24 — so no
//! codestream it prints carries an RGN marker segment.
//!
//! So the roadmap's claim was right for this capability, and it was tested
//! rather than believed.
//!
//! # What it does publish, and why that is nearly as good
//!
//! J.10, "An example of decoding showing intermediate steps", publishes a
//! complete 100-byte codestream **and its intermediate coefficients**:
//! J.10.4 states the five low-pass coefficients as "-26, -22, -30, -32, -19"
//! and the four vertical-high-pass ones as "1, 5, 1, 0", and J.10.5 states
//! the nine samples they reconstruct to. H.1 is a rewrite of exactly those
//! coefficients, so the standard's own published numbers are the input to the
//! clause under test — which is what makes a hand-authored ROI codestream
//! adjudicable at all.
//!
//! # The chain, and where self-consistency starts
//!
//! Ruling 13 wants this said plainly, link by link.
//!
//! 1. **Bytes to coefficients — adjudicated.** The fixtures below are J.10's
//!    own codestream with two edits: four `SPqcd` exponents lowered, and an
//!    RGN marker segment inserted. Neither edit reaches tier-1 or tier-2: the
//!    zero-bit-plane count and the pass count come from the packet header,
//!    and `Mb` (E-2) is consumed only by E-1. So the coefficients tier-1
//!    decodes are still J.10.4's published ones. *This is reasoning, not a
//!    citation* — and it is checked rather than asserted, because
//!    [`the_base_decodes_to_the_samples_j10_publishes`] decodes the
//!    untouched bytes and [`an_roi_that_restores_j10s_coefficients_restores_its_samples`]
//!    decodes the edited ones to the same nine samples. If an edit had moved
//!    a magnitude, the second could not match the first.
//! 2. **Coefficients to realigned coefficients — adjudicated.** H.1's three
//!    branches, applied to J.10.4's numbers, with the arithmetic written out
//!    per coefficient on each constant below.
//! 3. **Realigned coefficients to samples — adjudicated, via a calibrated
//!    second transcription.** [`inverse_5_3_column`] is F-3, F-4, F-5, F-6
//!    and G-2 transcribed here, sharing no code with the decoder.
//!    [`the_transcribed_inverse_5_3_reproduces_j10`] runs it on J.10.4's
//!    published coefficients and demands J.10.5's published samples **before**
//!    it is used to predict anything, so the standard adjudicates the
//!    predictor as well as the prediction.
//!
//! **Self-consistency starts, and ends, at one place**: the same `jpx_decode`
//! produces both the base decode and the ROI decodes. A defect that moved
//! J.10's own samples would fail link 1 outright, so what is left is the
//! narrow possibility of a defect that leaves J.10 exact and happens to move
//! an ROI decode onto a value `inverse_5_3_column` independently predicts.
//! Nothing in the decode is shared with the predictor, so that would have to
//! be a coincidence rather than a common cause.
//!
//! # Provenance
//!
//! ITU-T Rec. T.800 (11/2015) = ISO/IEC 15444-1:2016, SHA-256
//! `b1ca01eecd3fe13ad58ea2e253c19274eb091e4a3e240b489941c664e96869e0`,
//! 4 789 495 bytes, **byte-identical to the copy `jpx_annex_j.rs` records
//! as fetched from the ITU on 13 September 2026**. This repository's own
//! `tpdf info` reads it as 231 pages titled "ITU-T Rec. T.800 (11/2015)
//! Information technology – JPEG 2000 image coding system: Core coding
//! system", and its pages are headed ISO/IEC 15444-1:2016 (E). The
//! Recommendation is published free of charge.
//!
//! *The URL is recorded with the status it actually returned, in both
//! directions.* On 20 September 2026 a plain `curl` of the ITU's
//! `https://www.itu.int/rec/dologin_pub.asp?lang=e&id=T-REC-T.800-201511-S!!PDF-E&type=items`
//! answered **HTTP 500** three times running — the same thing this tree's
//! ROADMAP records for T.83's `dologin_pub` — so what adjudicates the
//! citations below is the hash above and not a fetch anyone can repeat
//! today. It is worth trying again: the W3C copy of T.81 was written down
//! here as returning 403 and answered 200 when someone retried it.
//!
//! Read with this repository's own `tpdf text`, and every clause quoted here
//! was read a second time from `tpdf render` of the page, because the text
//! layer drops this document's mathematics: the `>=` in H.1's steps 3 and 4
//! and the whole of (E-1), (H-1) and (H-2) extract as rubble.

use tinker_pdf_filters::{jpx_decode, Limits};

// --- T.800 J.10's codestream ----------------------------------------------

/// T.800 J.10's codestream, transcribed field by field from J.10.1 and
/// J.10.2's annotated listings rather than from the raw hex dump.
///
/// This is a second, independent transcription of the same 100 bytes
/// `jpx_annex_j.rs` carries; the two cannot check each other across test
/// binaries, so each is checked against J.10's own octal offsets and against
/// J.10.5's published samples on its own.
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

/// T.800 J.10.4: "Thus the decoded coefficients are: -26, -22, -30, -32,
/// -19" — the horizontal-low-pass vertical-low-pass sub-band, five of them
/// because the image is 1 x 9 with one decomposition level.
const J10_LL: [i32; 5] = [-26, -22, -30, -32, -19];

/// T.800 J.10.4: "The decoded vertical high pass horizontal low pass
/// coefficients are: 1, 5, 1, 0".
const J10_LH: [i32; 4] = [1, 5, 1, 0];

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
///
/// J.10 prints an octal offset against every field it names, so a
/// mis-transcribed byte count moves a named field off its stated offset.
#[test]
fn the_transcription_matches_j10s_annotated_offsets() {
    let named: &[(&str, &str, &[u8])] = &[
        ("00000", "SOC", &[0xFF, 0x4F]),
        ("00002", "SIZ", &[0xFF, 0x51]),
        ("00004", "Lsiz", &[0x00, 0x29]),
        ("00050", "Csiz", &[0x00, 0x01]),
        ("00055", "QCD", &[0xFF, 0x5C]),
        ("00061", "Sqcd", &[0x40]),
        ("00062", "SPqcd", &[0x40, 0x48, 0x48, 0x50]),
        ("00066", "COD", &[0xFF, 0x52]),
        ("00104", "SOT", &[0xFF, 0x90]),
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
    assert_eq!(
        ANNEX_J10.len(),
        octal("00142") + 2,
        "the codestream ends with the EOC at octal 00142"
    );
}

/// The base fixture is the standard's, and decodes to the standard's answer.
///
/// Link 1 of the chain in the module note. Everything below edits these
/// bytes, so a transcription slip has to fail here first.
#[test]
fn the_base_decodes_to_the_samples_j10_publishes() {
    let mut warnings = Vec::new();
    let image = jpx_decode(ANNEX_J10, &Limits::new(1 << 20), &mut warnings)
        .expect("T.800 J.10's own codestream decodes");
    assert_eq!(image.samples, &J10_SAMPLES, "T.800 J.10.5");
    assert!(warnings.is_empty(), "{warnings:?}");
}

// --- Annex F, transcribed a second time -----------------------------------

/// **T.800 F.3.3, F.3.6, F.3.7, F.3.8.1 and G.1.2 for a 1 x 9
/// tile-component**, transcribed here and sharing no code with the decoder.
///
/// The image is one sample wide, so the horizontal pass is **F.3.6's**
/// degenerate case — "For signals of length one (i.e., i0 = i1 - 1), the
/// 1D_SR procedure sets the value of X(i0) to Y(i0) if i0 is an even
/// integer" — and `i0` is 0, so it is the identity and only the vertical pass
/// does anything. That leaves one column, `i0 = 0` and `i1 = 9`:
///
/// *That clause number read F.3.7 until it was checked a second time, and
/// F.3.7 is the next procedure along.* F.3.6 is "The 1D_SR procedure", which
/// is where the length-one rule is; F.3.7 is "The 1D_EXTR procedure", which
/// is where (F-3) and (F-4) below are. Both are cited here because this
/// function does both jobs, and citing one for the other is precisely the
/// slip a second pass exists to catch.
///
/// - **F.3.3's interleave**: the low band on the even indices, the high band
///   on the odd ones.
/// - **(F-3) and (F-4)**: `Yext(i) = Y(PSEO(i, i0, i1))` with
///   `PSEO(i, i0, i1) = i0 + min(mod(i - i0, 2(i1 - i0 - 1)),
///   2(i1 - i0 - 1) - mod(i - i0, 2(i1 - i0 - 1)))`.
/// - **(F-5)**: `X(2n) = Yext(2n) - floor((Yext(2n-1) + Yext(2n+1) + 2) / 4)`
///   for `floor(i0/2) <= n < floor(i1/2) + 1`.
/// - **(F-6)**: `X(2n+1) = Yext(2n+1) + floor((X(2n) + X(2n+2)) / 2)` for
///   `floor(i0/2) <= n < floor(i1/2)`, using the values (F-5) just wrote.
/// - **G.1.2** (G-2): `+2^(Ssiz-1)` for an unsigned component. The clamp
///   below is **not** from the equation: G.1.2's NOTE says reconstructed
///   samples "may exceed the dynamic range of the original samples", that
///   "there is no normative procedure for this overflow or underflow
///   situation", and that clipping into range "is a typical solution". It is
///   named as the NOTE's solution rather than as the standard's arithmetic,
///   and J.10's own samples (96 to 109) are nowhere near it.
///
/// The floors are `div_euclid`, not `/`: T.800's `floor` rounds towards
/// negative infinity and Rust's `/` rounds towards zero, and every value in
/// this fixture is negative, so the two differ on the very first coefficient.
fn inverse_5_3_column(ll: [i32; 5], lh: [i32; 4]) -> [u8; 9] {
    // F.3.3's 2D_INTERLEAVE, for one column.
    let mut y = [0i32; 9];
    for (n, v) in ll.iter().enumerate() {
        y[2 * n] = *v;
    }
    for (n, v) in lh.iter().enumerate() {
        y[2 * n + 1] = *v;
    }

    // (F-3) and (F-4). `i0` is 0, so `PSEO`'s leading `i0 +` and its
    // `i - i0` are both the identity here and the expression below is what
    // is left of the equation.
    const I0: i64 = 0;
    const I1: i64 = 9;
    let ext = |i: i64| -> i32 {
        let period = 2 * (I1 - I0 - 1);
        let m = (i - I0).rem_euclid(period);
        y[usize::try_from(I0 + m.min(period - m)).expect("PSEO lands inside [i0, i1)")]
    };

    let mut x = [0i32; 9];
    // (F-5): n from floor(0/2) = 0 to floor(9/2) + 1 = 5, exclusive.
    for n in 0..5i64 {
        let even = usize::try_from(2 * n).expect("in range");
        x[even] = ext(2 * n) - (ext(2 * n - 1) + ext(2 * n + 1) + 2).div_euclid(4);
    }
    // (F-6): n from 0 to floor(9/2) = 4, exclusive.
    for n in 0..4i64 {
        let odd = usize::try_from(2 * n + 1).expect("in range");
        let even = usize::try_from(2 * n).expect("in range");
        x[odd] = ext(2 * n + 1) + (x[even] + x[even + 2]).div_euclid(2);
    }

    // G.1.2 (G-2), plus its NOTE's clip into the component's range.
    let mut out = [0u8; 9];
    for (dst, src) in out.iter_mut().zip(x) {
        *dst = u8::try_from((src + 128).clamp(0, 255)).expect("clamped to a byte");
    }
    out
}

/// **The predictor is adjudicated before it predicts anything.**
///
/// J.10.4 publishes the coefficients and J.10.5 publishes the samples they
/// reconstruct to, so the standard states both ends of `inverse_5_3_column`'s
/// one job. Four JBIG2 Annex B tables in this repository were transcribed
/// wrongly and passed every check available to them; this is the check that
/// was not available there.
#[test]
fn the_transcribed_inverse_5_3_reproduces_j10() {
    assert_eq!(
        inverse_5_3_column(J10_LL, J10_LH),
        J10_SAMPLES,
        "F-5 and F-6 as transcribed here do not reproduce J.10.5 from J.10.4, \
         so nothing below may be believed"
    );
}

// --- Annex H, over J.10's own coefficients --------------------------------

/// An RGN marker segment: `RGN`, `Lrgn`, `Crgn`, `Srgn`, `SPrgn` in Figure
/// A.12's order, at Table A.24's widths.
///
/// `Lrgn` is 5, which is Table A.24's "5 to 6" at its low end: two for the
/// length field, one for `Crgn` because `Csiz` is 1 and so below 257, one for
/// `Srgn` and one for `SPrgn`. `Srgn` is 0, Table A.25's only defined ROI
/// style, "Implicit ROI (maximum shift)". `SPrgn` is Table A.26's implicit
/// ROI shift, which H.1 calls `s`.
fn rgn_segment(component: u8, shift: u8) -> [u8; 7] {
    [0xFF, 0x5E, 0x00, 0x05, component, 0x00, shift]
}

/// The Maxshift shift both fixtures carry.
const SHIFT: u8 = 3;

/// J.10's codestream with `SPqcd` exponents lowered and an RGN inserted.
///
/// `drop` is subtracted from each of the four exponents named in the
/// assertion below, and the RGN segment goes in front of the SOT at octal
/// 00104 — A.6.3's "Main ... header of a given tile" — unless
/// `in_tile_part_header` moves it after the SOT segment instead, which is
/// A.6.3's other permitted placement.
///
/// **Lowering an exponent is how Maxshift headroom is written down.** `Mb` is
/// `G + eps_b - 1` (E-2) and the code-block's coded plane count comes from
/// the packet header, so an exponent `k` lower leaves `k` planes below E-1's
/// radix point — exactly the `s` planes H-3's `M'b = Mb + s` puts there.
///
/// **`Psot` moves only for the tile-part placement, and that is A.4.2's
/// arithmetic rather than a convenience.** "Psot: Length, in bytes, from the
/// beginning of the first byte of this SOT marker segment of the tile-part to
/// the end of the data of that tile-part", so a marker segment *in front of*
/// the SOT is outside it and a marker segment *inside the header* is not. Get
/// this wrong and the tile-part's data is read seven bytes short, which this
/// decoder catches as a packet that does not end where the next begins —
/// which is how the error was found.
fn maxshift_fixture(drop: [u8; 4], shift: u8, in_tile_part_header: bool) -> Vec<u8> {
    let mut out = ANNEX_J10.to_vec();

    let spqcd = octal("00062");
    assert_eq!(
        &out[spqcd..spqcd + 4],
        &[0x40, 0x48, 0x48, 0x50],
        "SPqcd sits at octal 00062: exponents 8, 9, 9, 10 for LL, HL, LH, HH"
    );
    for (i, k) in drop.iter().enumerate() {
        // A.6.4 Table A.29: with no quantization the byte is the exponent in
        // its top five bits, so `>> 3` reads it and `<< 3` writes it.
        let was = out[spqcd + i] >> 3;
        out[spqcd + i] = (was - k) << 3;
    }

    // A.6.3 permits the segment in the main header or in the first tile-part
    // header of a tile. J.10's one tile-part starts at octal 00104 and its
    // header ends at the SOD at octal 00120.
    let segment = rgn_segment(0, shift);
    let (at, marker) = if in_tile_part_header {
        // A.4.2: Psot spans the tile-part from its own SOT, so a segment
        // added inside the header lengthens it.
        let psot = octal("00112");
        assert_eq!(
            &out[psot..psot + 4],
            &[0x00, 0x00, 0x00, 0x1E],
            "Psot sits at octal 00112 and J.10.2 reads it as 30 bytes"
        );
        let grown = 30 + u32::try_from(segment.len()).expect("a seven-byte segment");
        out[psot..psot + 4].copy_from_slice(&grown.to_be_bytes());
        (octal("00120"), [0xFF, 0x93])
    } else {
        (octal("00104"), [0xFF, 0x90])
    };
    assert_eq!(&out[at..at + 2], &marker, "the RGN goes in front of this");
    out.splice(at..at, segment);
    out
}

/// **An ROI that puts every coefficient back where J.10 published it must
/// decode to the samples J.10 published.**
///
/// This is the fixture whose expected answer is entirely the standard's, and
/// it exercises both of H.1's live branches at once.
///
/// The LL band keeps J.10's exponent, so `Mb = 2 + 8 - 1 = 9` against a
/// code-block of 3 zero bit-planes and 6 coded planes (J.10.3: 16 passes is
/// `1 + 3 x 5`). That is 9 planes for a 9-plane budget, so the lowest coded
/// bit sits on `2^0`, `Nb(u, v) = Mb`, and every non-zero magnitude has a bit
/// among the first `Mb` MSBs. **H.1 step 3**: `Nb(u, v)` becomes `Mb`, which
/// truncates a value that is already an integer. `-26, -22, -30, -32, -19`
/// survive unchanged.
///
/// The LH band's exponent is lowered by three, so `Mb = 2 + 6 - 1 = 7`
/// against 7 zero bit-planes and 3 coded planes (J.10.3: 7 passes is
/// `1 + 3 x 2`) — a 10-plane code-block in a 7-plane budget, leaving three
/// planes of fractional weight. Every magnitude there is 5 or less, so
/// `|q| < 2^3 / 2^3 = 1` and all of the first `Mb` MSBs are zero. **H.1 step
/// 4**: H-1 shifts the remaining MSBs `s = 3` places and H-2 sets
/// `Nb(u, v) = max(0, Nb(u, v) - 3)`, which multiplies by `2^3` and lands
/// `1, 5, 1, 0` back on `2^0`.
///
/// So H.1 hands the inverse wavelet exactly J.10.4's nine coefficients, and
/// J.10.5 says what they reconstruct to.
#[test]
fn an_roi_that_restores_j10s_coefficients_restores_its_samples() {
    // LL keeps its exponent; HL, LH and HH lose three. Only LH has samples —
    // the image is one sample wide, so HL and HH are empty — and they move
    // together because a single component's bands share one `s`.
    let bytes = maxshift_fixture([0, 3, 3, 3], SHIFT, false);
    let mut warnings = Vec::new();
    let image = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings)
        .expect("J.10's codestream with an RGN decodes");
    assert_eq!(
        image.samples, &J10_SAMPLES,
        "H.1 step 4 should have put the LH band back on 2^0, which is where \
         J.10.4 published it"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// The same, with the RGN in the tile-part header instead of the main one.
///
/// A.6.3: "Usage: Main and first tile-part header of a given tile." J.10 has
/// one tile with one tile-part, so the two placements describe the same ROI
/// and must decode the same way. A decoder that read the main header's RGN
/// and not the tile-part's would pass the test above and fail this one.
#[test]
fn an_rgn_in_the_tile_part_header_decodes_the_same_way() {
    let bytes = maxshift_fixture([0, 3, 3, 3], SHIFT, true);
    let mut warnings = Vec::new();
    let image = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings)
        .expect("J.10's codestream with a tile-part RGN decodes");
    assert_eq!(image.samples, &J10_SAMPLES);
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// T.800 H.1 applied to J.10.4's five low-pass coefficients with `s = 3`,
/// against a band whose exponent is three lower than J.10's.
///
/// `Mb = 2 + 5 - 1 = 6` and the code-block carries 9 planes, so the lowest
/// coded bit sits on `2^-3` and `Nb(u, v) = Mb + 3`. Every magnitude is at
/// least 8, so `|q| >= 1` and **H.1 step 3** fires: `Nb(u, v)` becomes `Mb`,
/// which drops the three fractional bits. That is `floor(|q| / 8)` with the
/// sign kept, one coefficient at a time:
///
/// - `-26` -> `-floor(26/8)` = `-3`
/// - `-22` -> `-floor(22/8)` = `-2`
/// - `-30` -> `-floor(30/8)` = `-3`
/// - `-32` -> `-floor(32/8)` = `-4`
/// - `-19` -> `-floor(19/8)` = `-2`
const MAXSHIFT_LL: [i32; 5] = [-3, -2, -3, -4, -2];

/// T.800 H.1 applied to J.10.4's four vertical-high-pass coefficients with
/// `s = 3`, against a band whose exponent is three lower than J.10's.
///
/// `Mb = 2 + 6 - 1 = 7` and the code-block carries 10 planes, so again
/// `Nb(u, v) = Mb + 3`. Every magnitude is 5 or less, so `|q| = m / 8 < 1`,
/// all of the first `Mb` MSBs are zero, and **H.1 step 4** fires: H-1 shifts
/// by `s = 3` places, which is `x 2^3`, and `m / 8 x 8` is `m`. `1, 5, 1, 0`
/// are unchanged — which is the whole point of Maxshift, and the reason a
/// decoder cannot tell ROI from background by anything but magnitude.
const MAXSHIFT_LH: [i32; 4] = [1, 5, 1, 0];

/// **The ROI branch, decoded: H.1 step 3 divides an ROI coefficient down.**
///
/// This is the fixture that a decoder ignoring the RGN cannot pass. With the
/// LL band's exponent lowered too, its five coefficients cross into step 3's
/// truncation and come back at an eighth of J.10's — so the picture flattens
/// towards mid-grey, which is exactly what a Maxshift decoder does to a
/// region the encoder had scaled up. Leave the marker unapplied and the
/// samples are J.10's `101, 103, 104, 105, 96, 97, 96, 102, 109` instead.
///
/// The expected samples are not written out here. They come from
/// [`inverse_5_3_column`], which T.800 J.10 adjudicates in
/// [`the_transcribed_inverse_5_3_reproduces_j10`] before this test runs,
/// applied to the coefficients H.1's own arithmetic gives on
/// [`MAXSHIFT_LL`] and [`MAXSHIFT_LH`].
#[test]
fn h1s_roi_branch_scales_j10s_low_pass_band_down() {
    let bytes = maxshift_fixture([3, 3, 3, 3], SHIFT, false);
    let mut warnings = Vec::new();
    let image = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings)
        .expect("J.10's codestream with a Maxshift RGN decodes");

    let want = inverse_5_3_column(MAXSHIFT_LL, MAXSHIFT_LH);
    assert_eq!(
        image.samples, &want,
        "H.1 step 3 should have truncated the low-pass band to its integer \
         part; getting J.10's own samples back means the RGN was parsed and \
         not applied"
    );
    assert_ne!(
        want, J10_SAMPLES,
        "a fixture whose answer is J.10's own cannot show that H.1 ran"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// **A shift of zero is the degenerate ROI and must move nothing.**
///
/// H.1 with `s = 0`: step 3 truncates a value already on `2^0`, and step 4's
/// H-1 shifts by no places at all. A decoder that treated the RGN's presence
/// rather than its value as the trigger would fail here.
#[test]
fn a_zero_shift_leaves_j10_exactly_as_it_was() {
    let bytes = maxshift_fixture([0, 0, 0, 0], 0, false);
    let mut warnings = Vec::new();
    let image = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings)
        .expect("an RGN with a zero shift decodes");
    assert_eq!(image.samples, &J10_SAMPLES);
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A reserved `Srgn` is refused rather than decoded, on a codestream that
/// would otherwise decode perfectly.
///
/// Table A.25 defines style 0 and says "All other values reserved". The
/// fixture is J.10's own bytes, so what fails is the style and nothing else —
/// which is the shape of claim a refusal test needs to make.
#[test]
fn a_reserved_srgn_style_refuses_j10s_own_codestream() {
    let mut bytes = ANNEX_J10.to_vec();
    let sot = octal("00104");
    let mut segment = rgn_segment(0, SHIFT);
    segment[5] = 1; // Srgn = 1, which Table A.25 reserves.
    bytes.splice(sot..sot, segment);

    let mut warnings = Vec::new();
    assert!(
        jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings).is_err(),
        "a reserved ROI style must refuse rather than be read as Maxshift"
    );
}
