//! Milestone 1: the container (Annex I) and the codestream headers (Annex A).

use super::writer::{boxed, segment, tile_part, Spec, SIGNATURE};
use crate::jpx::codestream::cb_style;
use crate::jpx::codestream::{marker, refuse_marker, Progression, QuantStyle};
use crate::jpx::{boxes, codestream, jpx_decode, JpxColour, Refusal};
use crate::Limits;

/// The bytes of a codestream, parsed, or the refusal it earned.
fn parse(bytes: &[u8]) -> Result<codestream::Codestream<'_>, Refusal> {
    codestream::parse(bytes)
}

// `unfinishable()` stood here from milestone 1 to milestone 7: a codestream
// this build parsed and still could not finish, whose contents had to be
// *chosen* rather than assumed and moved three times as the decoder grew.
// Until milestone 4 it was the minimal single-component fixture, because
// nothing turned coefficients into samples; milestone 5 moved it to three
// components, which needed the colour pipeline; milestone 6 moved it to three
// components at 8, 8 and 12 bits, the one thing no milestone had specified.
//
// Milestone 8 retired it, because there is no longer a stage for it to name.
// Differing bit depths are still refused -- `super::refusals` reaches them --
// but as a *capability* this build does not have rather than as a decoder
// that is part-built, which is the difference `Refusal::NotBuilt` used to
// carry and no longer needs to. The pattern the fixture existed to
// demonstrate outlives it: an assertion about what is missing has a
// half-life, and the honest maintenance is to move it to whatever is
// genuinely next, right up until nothing is.

/// A minimal 4 x 4 greyscale stream with one empty tile-part.
fn minimal() -> Vec<u8> {
    Spec::default().codestream(&[(0, &EMPTY_PACKETS)])
}

/// Enough packet bytes for tier-2 to read the tile out exactly.
///
/// A tile carrying no data at all is *truncated*, not header-only: B.10 says
/// a layer has one packet per precinct per resolution per component, and the
/// default spec has one of each, so the tile owes one packet however empty
/// the picture is. A single zero bit is that packet -- B.10.3's non-empty
/// flag clear, meaning no code-block is included -- and the rest of the byte
/// is padding.
///
/// Before tier-2 ran, these fixtures reached the decoder's refusal without
/// anyone noticing they were short. That is the small print of wiring a stage
/// in: the fixtures that were sufficient for the stages before it stop being
/// sufficient, and a test that starts failing is the stage doing its job.
const EMPTY_PACKETS: [u8; 2] = [0x00, 0x00];

// --- the container ------------------------------------------------------

/// The exit criterion, first half: a hand-built JP2 reports its dimensions,
/// component count and precision **exactly**.
#[test]
fn a_jp2_reports_its_geometry_exactly() {
    let spec = Spec {
        xsiz: 37,
        ysiz: 61,
        xtsiz: 37,
        ytsiz: 61,
        components: vec![(12, false, 1, 1), (12, false, 2, 2), (12, false, 2, 2)],
        ..Spec::default()
    };
    let file = spec.jp2(&[(0, &[])]);
    let container = boxes::parse(&file).expect("a well-formed JP2");
    let header = container.header.expect("a jp2h header box");
    assert_eq!((header.width, header.height), (37, 61));
    assert_eq!(header.components, 3);
    assert_eq!(header.bpc, Some((12, false)));
    assert_eq!(header.colour, JpxColour::Greyscale);

    let stream = parse(container.codestream).expect("a well-formed codestream");
    assert_eq!((stream.siz.width(), stream.siz.height()), (37, 61));
    assert_eq!(stream.siz.components.len(), 3);
    assert!(stream.siz.components.iter().all(|c| c.precision == 12));
    assert_eq!(
        (stream.siz.components[1].dx, stream.siz.components[1].dy),
        (2, 2)
    );
    // B.2 equation B-12: a component subsampled by two covers half as many
    // columns, rounded up, which for 37 is 19 rather than 18.
    assert_eq!(stream.siz.tile_component_bounds(0, 0), (0, 0, 37, 61));
    assert_eq!(stream.siz.tile_component_bounds(0, 1), (0, 0, 19, 31));
}

/// The exit criterion, second half: a bare J2K codestream with no boxes at
/// all is recognised too. PDF permits both (7.4.9) and OpenJPEG writes this
/// one for a `.j2k` output.
#[test]
fn a_bare_codestream_needs_no_boxes() {
    let bytes = minimal();
    let container = boxes::parse(&bytes).expect("SOC then SIZ is a codestream");
    assert!(container.header.is_none());
    assert_eq!(container.codestream, &bytes[..]);
}

#[test]
fn a_file_that_is_neither_is_refused() {
    for bytes in [
        &b""[..],
        &b"%PDF-1.7"[..],
        &[0xFF, 0x4F, 0xFF, 0x52][..], // SOC but not SIZ
        &SIGNATURE[..],                // a signature box and nothing else
    ] {
        assert!(
            matches!(boxes::parse(bytes), Err(Refusal::Structure(_))),
            "{bytes:?}"
        );
    }
}

/// A box whose length cannot include its own header would leave the walk
/// where it started. Clamping it to zero is an infinite loop, which is the
/// shape ruling 1 exists for, so it is refused instead.
#[test]
fn a_box_shorter_than_its_header_is_refused_not_clamped() {
    for lbox in [2u32, 7] {
        let mut file = SIGNATURE.to_vec();
        file.extend_from_slice(&lbox.to_be_bytes());
        file.extend_from_slice(b"jp2c");
        assert_eq!(
            boxes::parse(&file),
            Err(Refusal::Structure("a JP2 box shorter than its header"))
        );
    }
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&1u32.to_be_bytes());
    file.extend_from_slice(b"jp2c");
    file.extend_from_slice(&8u64.to_be_bytes());
    assert_eq!(
        boxes::parse(&file),
        Err(Refusal::Structure("a JP2 XLBox shorter than its header"))
    );
}

#[test]
fn a_jp2_with_no_codestream_or_no_ihdr_is_refused() {
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 16])));
    assert_eq!(
        boxes::parse(&file),
        Err(Refusal::Structure("jp2h with no ihdr image header box"))
    );

    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"ftyp", b"jp2 "));
    assert_eq!(
        boxes::parse(&file),
        Err(Refusal::Structure("no jp2c contiguous codestream box"))
    );
}

/// I.5.3.1: the compression type is 7 and nothing else. Reading a
/// non-JPEG-2000 JP2 as one is how a decoder ends up parsing a codestream
/// that is not a codestream.
#[test]
fn an_ihdr_that_is_not_jpeg_2000_is_refused() {
    let ihdr = [0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 0, 0, 0];
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &boxed(*b"ihdr", &ihdr)));
    file.extend_from_slice(&boxed(*b"jp2c", &minimal()));
    assert_eq!(
        boxes::parse(&file),
        Err(Refusal::Structure("ihdr compression type is not 7"))
    );
}

/// Ruling 2: an enumerated colour space or a `colr` method this build cannot
/// map is **reported**, not guessed at. A file rendered in a guessed space is
/// a picture in the wrong colours that reads as a colour-management problem.
#[test]
fn colr_maps_what_it_can_and_refuses_what_it_cannot() {
    let cases = [
        (12u32, JpxColour::Cmyk),
        (16, JpxColour::Srgb),
        (17, JpxColour::Greyscale),
        (18, JpxColour::Sycc),
        (24, JpxColour::EYcc),
    ];
    for (enumcs, want) in cases {
        let mut colr = vec![1u8, 0, 0];
        colr.extend_from_slice(&enumcs.to_be_bytes());
        let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]);
        jp2h.extend_from_slice(&boxed(*b"colr", &colr));
        let mut file = SIGNATURE.to_vec();
        file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
        file.extend_from_slice(&boxed(*b"jp2c", &minimal()));
        let header = boxes::parse(&file).expect("valid").header.expect("jp2h");
        assert_eq!(header.colour, want, "EnumCS {enumcs}");
    }

    for colr in [
        vec![1u8, 0, 0, 0, 0, 0, 99], // an EnumCS this build cannot map
        vec![3u8, 0, 0],              // a method Table I.9 does not define
    ] {
        let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]);
        jp2h.extend_from_slice(&boxed(*b"colr", &colr));
        let mut file = SIGNATURE.to_vec();
        file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
        file.extend_from_slice(&boxed(*b"jp2c", &minimal()));
        assert!(
            matches!(boxes::parse(&file), Err(Refusal::Feature(_))),
            "{colr:?}"
        );
    }
}

/// `pclr`, `cmap` and `cdef` are read in full rather than noted as present.
///
/// Milestone 6 applies them, so what this asserts moved with it: it used to
/// be that the three booleans were set, and it is now that the palette's
/// entries, the channel mapping and the channel types all come back with the
/// values the file wrote. What the colour pipeline *does* with them is
/// `tests::colour`'s.
#[test]
fn palette_and_channel_boxes_are_read_not_ignored() {
    let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]);
    // Two entries of one 8-bit channel: 0 and 255.
    jp2h.extend_from_slice(&boxed(*b"pclr", &[0, 2, 1, 7, 0, 255]));
    jp2h.extend_from_slice(&boxed(*b"cmap", &[0, 0, 1, 0]));
    jp2h.extend_from_slice(&boxed(*b"cdef", &[0, 1, 0, 0, 0, 0, 0, 0]));
    jp2h.extend_from_slice(&boxed(*b"res ", &[]));
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    file.extend_from_slice(&boxed(*b"jp2c", &minimal()));
    let header = boxes::parse(&file).expect("valid").header.expect("jp2h");
    let palette = header.palette.expect("pclr");
    assert_eq!(palette.channels, vec![(8, false)]);
    assert_eq!(palette.columns, vec![vec![0, 255]]);
    assert_eq!(header.component_map.len(), 1);
    assert_eq!(header.component_map[0].component, 0);
    assert_eq!(header.component_map[0].column, Some(0));
    assert_eq!(header.channel_definition.len(), 1);
    assert_eq!(header.channel_definition[0].kind, 0);
}

/// `bpcc` carries the per-component precisions when `ihdr` says they differ.
#[test]
fn bpcc_is_read_when_ihdr_says_the_components_differ() {
    let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 3, 255, 7, 0, 0]);
    jp2h.extend_from_slice(&boxed(*b"bpcc", &[7, 7, 0x8B]));
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    file.extend_from_slice(&boxed(*b"jp2c", &minimal()));
    let header = boxes::parse(&file).expect("valid").header.expect("jp2h");
    assert_eq!(header.bpc, None);
    assert_eq!(header.bpcc, vec![(8, false), (8, false), (12, true)]);
}

// --- Table A.2, in full -------------------------------------------------

/// **Every marker in Table A.2 is either parsed or named in a refusal.**
///
/// Not "most", and not "the ones a file is likely to hold". A skipped POC
/// changes the packet order mid-stream and mis-parses every packet after it;
/// a skipped RGN leaves every background coefficient `2^s` too small. Both
/// produce a picture. This test walks the whole table and demands that each
/// marker lands in exactly one of the two buckets — which is checkable only
/// because [`Refusal`] carries the name rather than collapsing to a warning.
#[test]
fn every_table_a2_marker_is_parsed_or_named() {
    /// T.800 Table A.2, transcribed. Twenty markers, and the flag says
    /// whether this build **acts on** the marker or refuses it by name.
    ///
    /// **The flag used to be documentation** — every loop below took it as
    /// `_` — so it could have said anything, and a row could have gone stale
    /// in either direction without a test moving. It is asserted now, in both
    /// directions: a marker flagged acted-on may not come back as
    /// `Refusal::Marker`, and a marker flagged refused must. RGN, PPM and PPT
    /// all moved from `false` to `true` on 20 September 2026 and POC on the
    /// 21st, and a flag nothing reads would have recorded those moves without
    /// checking them.
    ///
    /// **Every row is `true` now**, which is a statement about the decoder
    /// rather than about this table: no Table A.2 marker is refused as a
    /// capability. SOP and EPH are `true` and still refuse where this test
    /// puts them, because A.8 puts both inside the bit stream, tier-2 reads
    /// them there, and a header is the one place they have no meaning.
    const TABLE_A2: [(u16, &str, bool); 20] = [
        (marker::SOC, "SOC", true),
        (marker::SOT, "SOT", true),
        (marker::SOD, "SOD", true),
        (marker::EOC, "EOC", true),
        (marker::SIZ, "SIZ", true),
        (marker::COD, "COD", true),
        (marker::COC, "COC", true),
        (marker::RGN, "RGN", true),
        (marker::QCD, "QCD", true),
        (marker::QCC, "QCC", true),
        (marker::POC, "POC", true),
        (marker::TLM, "TLM", true),
        (marker::PLM, "PLM", true),
        (marker::PLT, "PLT", true),
        (marker::PPM, "PPM", true),
        (marker::PPT, "PPT", true),
        (marker::SOP, "SOP", true),
        (marker::EPH, "EPH", true),
        (marker::CRG, "CRG", true),
        (marker::COM, "COM", true),
    ];

    // No two markers share a code, which is the transcription slip that would
    // make the loop below pass while testing one marker twice.
    let mut codes: Vec<u16> = TABLE_A2.iter().map(|(c, _, _)| *c).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), 20, "Table A.2 has twenty distinct markers");

    // Two independent transcriptions of the same table, cross-checked. The
    // list above is the test's; `in_table_a2` is the decoder's. A slip in one
    // cannot agree with a slip in the other.
    for (code, name, _) in TABLE_A2 {
        assert!(
            codestream::in_table_a2(code),
            "the decoder does not know {name}"
        );
    }
    for code in [0xFF00u16, 0xFF74, 0xFF75, 0xFF78, 0xFF79, 0xFFAA, 0xFF31] {
        assert!(
            !codestream::in_table_a2(code),
            "{code:#06X} is not in Table A.2"
        );
    }

    let spec = Spec::default();
    for (code, name, acted_on) in TABLE_A2 {
        // Every marker that may appear in a main header, put in one. The four
        // delimiters and the two in-packet markers are covered by their own
        // tests; what this asks of them is that reaching one here is still a
        // refusal that names it rather than a skip.
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&segment(code, &[0, 0]));
        bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        let got = parse(&bytes);
        // **The third column is a demand, not a note.** It used to be read
        // as `_` in both loops of this test, so it recorded the decoder's
        // refusal list without checking a word of it — and a row could have
        // gone stale in either direction without a test moving. A marker
        // flagged as one this build does not act on must be refused *by
        // name*, here, in a main header, which is the property the whole
        // refusal list exists to make checkable.
        if !acted_on {
            assert!(
                matches!(got, Err(Refusal::Marker(_))),
                "{name} is flagged as a marker this build does not act on,                  so it must be refused by name; it produced {got:?}"
            );
        }
        match got {
            // Parsed: TLM, PLM, PLT and COM carry no coding information, so a
            // main header holding one still parses.
            Ok(_) => assert!(
                matches!(code, marker::TLM | marker::PLM | marker::PLT | marker::COM),
                "{name} was acted on and should not have been"
            ),
            Err(Refusal::Marker(text)) => {
                assert!(
                    text.starts_with(name),
                    "{name} was refused as {text:?}, which does not name it"
                );
                // SOP and EPH are the two flagged acted-on that still refuse
                // *here*: they belong to tier-2's packet data, and a header
                // is the one place they have no meaning. Every other marker
                // refusing by name is one the flag says is refused.
                assert!(
                    !acted_on || matches!(code, marker::SOP | marker::EPH),
                    "{name} is flagged acted-on in TABLE_A2 and came back as a \
                     marker refusal"
                );
            }
            // The delimiters are structural where this test puts them: a
            // second SIZ, an SOD with no SOT, an SOC in the middle. Refused,
            // and never *skipped*.
            Err(Refusal::Structure(_) | Refusal::Truncated(_) | Refusal::Feature(_)) => assert!(
                acted_on,
                "{name} is flagged refused in TABLE_A2 and did not name \
                 itself — a refusal that does not carry the marker's name is \
                 what this whole test exists to forbid"
            ),
            // The one outcome this whole test exists to forbid: a marker the
            // standard defines being reported as one nothing defines, which
            // is what a decoder that had never transcribed the table would do.
            Err(Refusal::UnknownMarker(c)) => {
                panic!("{name} ({c:#06X}) was reported as a marker Table A.2 does not define")
            }
            Err(other) => panic!("{name} produced {other:?}"),
        }
    }
}

/// A marker code Table A.2 does not define — which is where every ISO/IEC
/// 15444-2 marker lands, Part 2 being a non-goal — is refused as unknown and
/// carries its code.
#[test]
fn a_marker_outside_table_a2_is_refused_by_code() {
    for code in [0xFF74u16, 0xFF75, 0xFF78, 0xFF79, 0xFF00, 0xFFAA] {
        assert_eq!(
            refuse_marker(code),
            Refusal::UnknownMarker(code),
            "{code:#06X}"
        );
    }
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(0xFF74, &[0]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(parse(&bytes), Err(Refusal::UnknownMarker(0xFF74)));
}

/// **No Table A.2 marker is refused as a capability, and the two that name
/// themselves name a *place* instead.**
///
/// This test was `the_one_refused_marker_names_itself` and before that a loop
/// over four, and what it asserts has changed shape rather than shrunk again.
/// The list is empty of capabilities now:
///
/// - CRG went first: A.9.1 says it "has no effect on decoding the
///   codestream", so it is parsed, carried and never applied.
/// - RGN went when T.800 Annex H was implemented, and what took its place is
///   narrower and lives inside the segment rather than at it — Table A.25
///   defines one `Srgn` style and reserves the rest, so a reserved style is
///   refused as a reserved style ([`a_reserved_srgn_style_is_refused_by_name`])
///   and style 0 is decoded.
/// - PPM and PPT went when packed packet headers were implemented; they are
///   parsed here and read by tier-2
///   ([`ppm_relocates_every_tile_part_s_headers`] and its neighbours below).
/// - POC went last, on 21 September 2026: A.6.6's progressions are B.12.2's
///   progression order volumes and tier-2 sequences the packets from them.
///   `crates/tinker-pdf-filters/tests/jpx_poc.rs` holds it to T.800's own
///   bytes, and what is left of it here is a *value* — a `Ppoc` Table A.16
///   does not define, a bound outside Table A.32, a length that is not
///   equation (A-6)'s.
///
/// What still refuses **by name** is SOP and EPH, and for a reason that is
/// about placement rather than capability: A.8 puts both inside the bit
/// stream, tier-2 reads them there, and a header is the one place in a
/// codestream where neither has a meaning.
#[test]
fn the_markers_that_name_themselves_are_the_two_out_of_place_ones() {
    let spec = Spec::default();
    for (code, name) in [(marker::SOP, "SOP"), (marker::EPH, "EPH")] {
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&segment(code, &[0, 0]));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        match parse(&bytes) {
            Err(Refusal::Marker(text)) => {
                assert!(text.starts_with(name), "{text:?}");
                assert!(
                    text.contains("outside tile data"),
                    "{name} should be refused for where it is, not for what it \
                     is: {text:?}"
                );
            }
            other => panic!("{name} produced {other:?}"),
        }
    }

    // And the marker that used to stand here: a POC whose body is the seven
    // bytes (A-6) requires now parses, so the refusal it leaves behind is a
    // value inside the segment rather than the segment.
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 1, 0)])));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    let stream = parse(&bytes).expect("a POC at Table A.32's widths parses");
    assert_eq!(stream.poc.as_ref().map(Vec::len), Some(1));
}

// --- A.6.6's progression order change ------------------------------------

/// One POC progression, in Figure A.15's field order — `RSpoc`, `CSpoc`,
/// `LYEpoc`, `REpoc`, `CEpoc`, `Ppoc`.
///
/// The narrow `CSpoc` and `CEpoc`, because every fixture here has far fewer
/// than 257 components (Table A.32), which is also why one progression is
/// seven bytes rather than nine.
type PocFields = (u8, u8, u16, u8, u8, u8);

/// A POC body: one seven-byte progression per [`PocFields`].
fn poc_body(progressions: &[PocFields]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(rs, cs, lye, re, ce, ppoc) in progressions {
        out.extend_from_slice(&[rs, cs]);
        out.extend_from_slice(&lye.to_be_bytes());
        out.extend_from_slice(&[re, ce, ppoc]);
    }
    out
}

/// A codestream with `main` in its main header and `tile` in its only
/// tile-part header.
fn with_headers(main: &[u8], tile: &[u8]) -> Vec<u8> {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(main);
    bytes.extend_from_slice(&tile_part(0, 0, 1, tile, &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    bytes
}

/// **Equation (A-6) leaves exactly one legal `Lpoc` for a progression
/// count**, so the length is checked for equality rather than sufficiency.
///
/// ```text
/// Lpoc = 2 + 7 * number_progression_order_change   Csiz <  257
/// Lpoc = 2 + 9 * number_progression_order_change   Csiz >= 257
/// ```
///
/// Table A.32 gives `Lpoc` as "9 to 65 535", whose lower bound is the
/// one-progression case — so a zero-progression POC is a segment describing
/// nothing while A.6.6 requires "the progression of every packet in the
/// codestream ... shall be defined in one or more POC marker segments".
#[test]
fn a_poc_length_equation_a6_does_not_allow_is_refused() {
    for body in [Vec::new(), vec![0; 6], vec![0; 8], vec![0; 13], vec![0; 15]] {
        let len = body.len();
        assert_eq!(
            parse(&with_headers(&segment(marker::POC, &body), &[])),
            Err(Refusal::Structure(
                "a POC marker segment whose length is not equation A-6's"
            )),
            "a {len}-byte POC body",
        );
    }
    // Seven and fourteen are one and two progressions.
    for n in [1usize, 2] {
        let body = poc_body(&vec![(0, 0, 1, 2, 1, 0); n]);
        assert_eq!(body.len(), 7 * n);
        let bytes = with_headers(&segment(marker::POC, &body), &[]);
        let stream = parse(&bytes).expect("(A-6)'s own length parses");
        assert_eq!(stream.poc.as_ref().map(Vec::len), Some(n));
    }
}

/// **Every one of Table A.32's ranges, checked rather than assumed.**
///
/// | Parameter | Values |
/// | --- | --- |
/// | `RSpoc` | 0 to 32 |
/// | `LYEpoc` | 1 to 65 535 |
/// | `REpoc` | (`RSpoc` + 1) to 33 |
/// | `CEpoc` | (`CSpoc` + 1) to 255, 0; if Csiz < 257 |
///
/// A volume whose bounds run backwards is not a volume, and clamping one into
/// shape would decode a packet sequence the codestream never described —
/// which is the same failure as skipping the marker, reached from the other
/// side.
#[test]
fn a_poc_outside_table_a32s_ranges_is_refused() {
    let cases: &[(PocFields, &str)] = &[
        (
            (33, 0, 1, 34, 1, 0),
            "a POC RSpoc above Table A.32's thirty-two",
        ),
        (
            (1, 0, 1, 1, 1, 0),
            "a POC REpoc outside Table A.32's (RSpoc + 1) to 33",
        ),
        (
            (0, 0, 1, 0, 1, 0),
            "a POC REpoc outside Table A.32's (RSpoc + 1) to 33",
        ),
        (
            (0, 0, 1, 34, 1, 0),
            "a POC REpoc outside Table A.32's (RSpoc + 1) to 33",
        ),
        ((0, 0, 0, 2, 1, 0), "a POC LYEpoc of zero"),
        // The default spec has one component, so `CSpoc` = 1 names one SIZ
        // never declared. `CEpoc` = 2 keeps Table A.32's (CSpoc + 1) rule
        // satisfied, so this reaches the check it is about.
        (
            (0, 1, 1, 2, 2, 0),
            "a POC CSpoc naming a component SIZ did not",
        ),
    ];
    for &(fields, why) in cases {
        assert_eq!(
            parse(&with_headers(
                &segment(marker::POC, &poc_body(&[fields])),
                &[]
            )),
            Err(Refusal::Structure(why)),
            "{fields:?}",
        );
    }
    // `CEpoc` <= `CSpoc` needs a component to start from, so it gets its own
    // fixture with two of them.
    let spec = Spec {
        components: vec![(8, false, 1, 1), (8, false, 1, 1)],
        ..Spec::default()
    };
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(marker::POC, &poc_body(&[(0, 1, 1, 2, 1, 0)])));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure(
            "a POC CEpoc outside Table A.32's (CSpoc + 1) to its ceiling"
        ))
    );
}

/// Table A.32's `CEpoc` footnote: "(0 is interpreted as 256)".
///
/// Zero is the *largest* legal `CEpoc` and not the smallest, so it is read
/// before the range check rather than after — a decoder that ordered the two
/// the other way would refuse the one value the table adds a note for.
#[test]
fn a_cepoc_of_zero_is_table_a32s_two_hundred_and_fifty_six() {
    let bytes = with_headers(&segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 0, 0)])), &[]);
    let stream = parse(&bytes).expect("CEpoc = 0 is Table A.32's 256");
    let volumes = stream.poc.expect("a main-header POC");
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].component_end, 256);
}

/// `Ppoc` is Table A.16's eight bits, which is the same table `SGcod` uses —
/// so a sixth progression order is refused here exactly as it is in a COD.
#[test]
fn a_ppoc_table_a16_does_not_define_is_refused() {
    for ppoc in [5u8, 6, 255] {
        assert_eq!(
            parse(&with_headers(
                &segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 1, ppoc)])),
                &[]
            )),
            Err(Refusal::Feature(
                "a progression order Table A.16 does not define"
            )),
            "Ppoc = {ppoc}",
        );
    }
    for ppoc in 0..=4u8 {
        assert!(
            parse(&with_headers(
                &segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 1, ppoc)])),
                &[]
            ))
            .is_ok(),
            "Ppoc = {ppoc} is one of Table A.16's five",
        );
    }
}

/// A.6.6: "At most one POC marker segment may appear in any header."
///
/// B.12.3 says it again with the scope spelled out — "There can only be one
/// POC marker segment in a given header (main or tile-part) but that marker
/// segment can describe many progression order changes" — so both headers are
/// checked, and a segment with two progressions in it is the legal way to say
/// what two segments cannot.
#[test]
fn two_poc_segments_in_one_header_are_refused() {
    let one = segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 1, 0)]));
    let mut two = one.clone();
    two.extend_from_slice(&one);
    assert_eq!(
        parse(&with_headers(&two, &[])),
        Err(Refusal::Structure(
            "a second POC marker segment in one header"
        ))
    );
    assert_eq!(
        parse(&with_headers(&[], &two)),
        Err(Refusal::Structure(
            "a second POC marker segment in one header"
        ))
    );
    // One segment describing two progressions is what A.6.6 offers instead.
    let both = segment(
        marker::POC,
        &poc_body(&[(0, 0, 1, 1, 1, 0), (1, 0, 1, 2, 1, 0)]),
    );
    assert_eq!(
        parse(&with_headers(&both, &[]))
            .expect("two progressions in one segment")
            .poc
            .map(|v| v.len()),
        Some(2)
    );
}

/// **A.6.6's precedence, at the parser rather than through a decode.**
///
/// > Tile-part POC > Main POC > Tile-part COD > Main COD
///
/// B.12.3 states the override from the other end: with a tile-part POC, "The
/// COD progression order and the main header POC marker segment (if there is
/// one) are overridden" — so a tile's own volumes *replace* the main
/// header's rather than extending them.
#[test]
fn a_tile_part_poc_replaces_the_main_headers_rather_than_extending_it() {
    let main = segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 1, 0)]));
    let tile = segment(
        marker::POC,
        &poc_body(&[(1, 0, 1, 2, 1, 1), (0, 0, 1, 1, 1, 1)]),
    );

    let main_only_bytes = with_headers(&main, &[]);
    let only_main = parse(&main_only_bytes).expect("a main-header POC");
    let volumes = only_main
        .progression_volumes(0)
        .expect("the main header's reach every tile");
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].resolution_start, 0);
    assert_eq!(volumes[0].order, Progression::Lrcp);

    let both_bytes = with_headers(&main, &tile);
    let both = parse(&both_bytes).expect("both headers may carry one");
    let volumes = both.progression_volumes(0).expect("the tile-part's win");
    assert_eq!(
        volumes.len(),
        2,
        "two, not three: this replaces, not appends"
    );
    assert_eq!(volumes[0].resolution_start, 1);
    assert_eq!(volumes[0].order, Progression::Rlcp);
    // The main header's is still parsed and still there for any other tile.
    assert_eq!(both.poc.as_ref().map(Vec::len), Some(1));
}

/// B.12.3: "If a POC marker segment is used for an individual tile, **there
/// shall be a POC marker in the first tile-part header of that tile**."
///
/// A.6.6 requires the same thing in its own words. A later part carrying the
/// first of a tile's volumes is a codestream whose packets began before
/// anything said in what order, so it is refused rather than read as though
/// the volumes had started at the top.
///
/// **The other placement B.12.3 allows is not refused**, and the difference
/// is the whole reason this is checked after every part has arrived rather
/// than inside the tile-part parser: Figure B.15b draws volumes 1 and 2 in
/// the first tile-part header with volume 3 in the third, and that is a
/// conforming arrangement.
#[test]
fn a_tile_part_poc_needs_one_in_the_tiles_first_tile_part_header() {
    let poc = segment(marker::POC, &poc_body(&[(0, 0, 1, 2, 1, 0)]));
    let spec = Spec::default();

    let mut late_only = spec.main_header();
    late_only.extend_from_slice(&tile_part(0, 0, 2, &[], &EMPTY_PACKETS));
    late_only.extend_from_slice(&tile_part(0, 1, 2, &poc, &[]));
    late_only.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&late_only),
        Err(Refusal::Structure(
            "a tile-part POC with none in the tile's first tile-part header"
        ))
    );

    // Figure B.15b's arrangement: the first part opens the series and a later
    // part continues it. The volumes join in TPsot order.
    let mut both = spec.main_header();
    both.extend_from_slice(&tile_part(
        0,
        0,
        2,
        &segment(marker::POC, &poc_body(&[(0, 0, 1, 1, 1, 0)])),
        &EMPTY_PACKETS,
    ));
    both.extend_from_slice(&tile_part(
        0,
        1,
        2,
        &segment(marker::POC, &poc_body(&[(1, 0, 1, 2, 1, 0)])),
        &[],
    ));
    both.extend_from_slice(&marker::EOC.to_be_bytes());
    let stream = parse(&both).expect("Figure B.15b's placement is conforming");
    let volumes = stream.progression_volumes(0).expect("the tile's own");
    assert_eq!(volumes.len(), 2, "joined across the tile's parts");
    assert_eq!(
        (volumes[0].resolution_start, volumes[1].resolution_start),
        (0, 1),
        "B.12.3: the volumes are described in order"
    );
}

// --- A.6.3's region of interest -----------------------------------------

/// An RGN marker segment's body, Figure A.12's fields in order: `Crgn`,
/// `Srgn`, `SPrgn`, with `Crgn` one byte because every fixture here has far
/// fewer than 257 components (Table A.24).
fn rgn(component: u8, srgn: u8, sprgn: u8) -> Vec<u8> {
    segment(marker::RGN, &[component, srgn, sprgn])
}

/// **Table A.24's field widths, read as the table writes them.**
///
/// The order is Figure A.12's — `RGN`, `Lrgn`, `Crgn`, `Srgn`, `SPrgn` — and
/// getting `Srgn` and `SPrgn` the wrong way round is the transcription slip
/// this guards: both are one byte, so a swap parses cleanly and gives a
/// shift of 0 with a style of 21, or a style of 0 with a shift of nothing.
/// The fixture below uses a style of 0 and a shift of 21 precisely because
/// they are distinguishable.
#[test]
fn an_rgn_segment_is_read_at_table_a24s_field_widths() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&rgn(0, 0, 21));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    let stream = parse(&bytes).expect("an RGN with Table A.25's one style parses");
    assert_eq!(
        stream.roi_for(0, 0).map(|r| r.shift),
        Some(21),
        "SPrgn is Table A.26's implicit ROI shift"
    );
}

/// **A component with no RGN has no region of interest**, so H.1 applies to
/// nothing — which is every component of almost every file.
#[test]
fn a_component_with_no_rgn_has_no_roi() {
    let bytes = minimal();
    let stream = parse(&bytes).expect("the minimal fixture parses");
    assert_eq!(stream.roi_for(0, 0), None);
}

/// **A reserved `Srgn` is refused by name, not stepped over.**
///
/// Table A.25 gives value 0, "Implicit ROI (maximum shift)", and says "All
/// other values reserved". A style this build has never seen describes some
/// other realignment of the coefficients, and running H.1's Maxshift
/// arithmetic over it would put the background at the wrong magnitude and
/// draw a plausible picture. That is the SOF3/SOF5/SOF6/SOF7 failure in this
/// tree's JPEG decoder, where a frame type was skipped rather than refused
/// and a lossless file surfaced as a damaged one.
#[test]
fn a_reserved_srgn_style_is_refused_by_name() {
    let spec = Spec::default();
    for srgn in [1u8, 2, 127, 255] {
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&rgn(0, srgn, 4));
        bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        assert_eq!(
            parse(&bytes),
            Err(Refusal::Feature("an Srgn ROI style Table A.25 reserves")),
            "Srgn = {srgn}"
        );
    }
}

/// **`Lrgn` is checked for equality, not sufficiency.**
///
/// Table A.24 gives "5 to 6", which is two for the length, one or two for
/// `Crgn` by `Csiz`, one for `Srgn` and one for `SPrgn`. For a one-component
/// image that is exactly one legal body length, and a decoder that read the
/// first three bytes out of a longer segment would be inventing a tolerance
/// the table does not give — the mistake `parse_crg` records for CRG.
#[test]
fn an_rgn_length_table_a24_does_not_allow_is_refused() {
    let spec = Spec::default();
    for body in [vec![0u8], vec![0, 0], vec![0, 0, 0, 0], vec![0, 0, 0, 0, 0]] {
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&segment(marker::RGN, &body));
        bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        assert_eq!(
            parse(&bytes),
            Err(Refusal::Structure(
                "an RGN marker segment whose length is not Table A.24's"
            )),
            "a {}-byte RGN body",
            body.len()
        );
    }
}

/// A.6.3: "There may be at most one RGN marker segment for each component in
/// either the main or tile-part headers." Two is a codestream saying two
/// things about one component's ROI, and last-one-wins is a guess that
/// decodes.
#[test]
fn two_rgn_markers_for_one_component_are_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&rgn(0, 0, 3));
    bytes.extend_from_slice(&rgn(0, 0, 4));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("two RGN markers for one component"))
    );

    // And the same inside one tile-part header.
    let mut header = rgn(0, 0, 3);
    header.extend_from_slice(&rgn(0, 0, 4));
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 1, &header, &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("two RGN markers for one component"))
    );
}

/// An RGN naming a component SIZ did not declare is refused rather than
/// widening any array (ruling 1).
#[test]
fn an_rgn_naming_a_component_siz_did_not_is_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&rgn(3, 0, 3));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("RGN names a component SIZ did not"))
    );
}

/// **A.6.3's precedence, which is two deep rather than four.**
///
/// "The RGN marker segment for a particular component which appears in a
/// tile-part header overrides any marker for that component in the main
/// header, for the tile in which it appears." Two tiles, an RGN in the main
/// header and another in tile 1's first tile-part: tile 0 keeps the main
/// header's shift and tile 1 takes its own. A decoder that took the larger,
/// or added them, would be inventing an arithmetic the clause does not have.
#[test]
fn a_tile_part_rgn_overrides_the_main_header_for_that_tile() {
    let spec = Spec {
        xsiz: 8,
        ysiz: 4,
        xtsiz: 4,
        ytsiz: 4,
        ..Spec::default()
    };
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&rgn(0, 0, 3));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&tile_part(1, 0, 1, &rgn(0, 0, 9), &EMPTY_PACKETS));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    let stream = parse(&bytes).expect("two tiles, one with its own RGN");
    assert_eq!(stream.roi_for(0, 0).map(|r| r.shift), Some(3), "tile 0");
    assert_eq!(stream.roi_for(1, 0).map(|r| r.shift), Some(9), "tile 1");
}

/// A.6.3: "If there are multiple tile-parts in a tile, then this marker
/// segment shall be found only in the first tile-part header."
#[test]
fn an_rgn_after_the_first_tile_part_is_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 2, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&tile_part(0, 1, 2, &rgn(0, 0, 3), &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure(
            "a coding style marker in a tile-part after the first"
        ))
    );
}

// --- A.5.1's tile grid --------------------------------------------------

/// **Each of A.5.1's constraints refuses rather than dividing by zero.**
///
/// These are not policy. `XTsiz` is the divisor in equation A-4 and `XOsiz`
/// is a subtrahend in A-5, so a zero tile size is a division by zero and an
/// offset past the size is an underflow — on attacker-controlled 32-bit
/// numbers (ruling 1). The constraint is checked *before* the arithmetic that
/// needs it, which is why `parse_siz` can then divide without a guard.
#[test]
fn every_a51_tile_grid_constraint_refuses() {
    let cases: [(&str, Spec, &str); 7] = [
        (
            "zero XTsiz",
            Spec {
                xtsiz: 0,
                ..Spec::default()
            },
            "a zero tile size",
        ),
        (
            "zero YTsiz",
            Spec {
                ytsiz: 0,
                ..Spec::default()
            },
            "a zero tile size",
        ),
        (
            "XOsiz == Xsiz",
            Spec {
                xosiz: 4,
                ..Spec::default()
            },
            "an image offset at or past its size",
        ),
        (
            "YOsiz past Ysiz",
            Spec {
                yosiz: 9,
                ..Spec::default()
            },
            "an image offset at or past its size",
        ),
        (
            "XTOsiz past XOsiz",
            Spec {
                xosiz: 1,
                xtosiz: 2,
                ..Spec::default()
            },
            "a tile offset past the image offset",
        ),
        (
            "a tile grid that starts after the image",
            Spec {
                xosiz: 3,
                xtosiz: 0,
                xtsiz: 1,
                ..Spec::default()
            },
            "a tile grid that misses the image",
        ),
        (
            "a zero component separation",
            Spec {
                components: vec![(8, false, 0, 1)],
                ..Spec::default()
            },
            "a zero component separation",
        ),
    ];
    for (what, spec, want) in cases {
        let bytes = spec.codestream(&[(0, &[])]);
        assert_eq!(parse(&bytes), Err(Refusal::Structure(want)), "{what}");
    }
}

/// The grid this build *does* accept divides out to the tile count A.5.1's
/// equation A-4 gives, and each tile's bounds are A-5 to A-8's.
#[test]
fn the_tile_grid_matches_equations_a4_to_a8() {
    let spec = Spec {
        xsiz: 10,
        ysiz: 7,
        xosiz: 1,
        yosiz: 1,
        xtsiz: 4,
        ytsiz: 4,
        xtosiz: 0,
        ytosiz: 0,
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let stream = parse(&bytes).expect("a valid grid");
    // ceil((10 - 0) / 4) = 3, ceil((7 - 0) / 4) = 2.
    assert_eq!((stream.siz.tiles_x, stream.siz.tiles_y), (3, 2));
    // Tile 0 starts at the image offset rather than at the grid origin.
    assert_eq!(stream.siz.tile_bounds(0), (1, 1, 4, 4));
    assert_eq!(stream.siz.tile_bounds(2), (8, 1, 10, 4));
    assert_eq!(stream.siz.tile_bounds(5), (8, 4, 10, 7));
}

/// A.5.1's own tile bound, and the fact that it is *not* the work cap.
#[test]
fn the_tile_count_is_bounded_by_a51_and_the_samples_by_the_budget() {
    // 65 536 tiles of one pixel: inside every per-item cap except this one.
    let spec = Spec {
        xsiz: 256,
        ysiz: 256,
        xtsiz: 1,
        ytsiz: 1,
        ..Spec::default()
    };
    assert_eq!(parse(&spec.codestream(&[])), Err(Refusal::Budget("tiles")));

    // And the other way round: a tile count of one, a component count inside
    // its cap, and a sample total that is not. This is the case a per-item
    // cap cannot see (`5adf502`).
    let spec = Spec {
        xsiz: 20_000,
        ysiz: 20_000,
        xtsiz: 20_000,
        ytsiz: 20_000,
        components: vec![(8, false, 1, 1); 4],
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let stream = parse(&bytes).expect("the headers are legal");
    assert_eq!(
        stream.check_budget(&Limits::new(usize::MAX)),
        Err(Refusal::Budget("tile-component samples")),
        "1.6e9 samples inside every per-item cap was not refused"
    );
}

/// The caller's own ceiling is respected rather than overridden by this
/// module's.
#[test]
fn the_callers_output_ceiling_is_checked_too() {
    let spec = Spec {
        xsiz: 512,
        ysiz: 512,
        xtsiz: 512,
        ytsiz: 512,
        components: vec![(8, false, 1, 1); 3],
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let stream = parse(&bytes).expect("valid");
    assert_eq!(stream.check_budget(&Limits::new(1 << 20)), Ok(()));
    assert_eq!(
        stream.check_budget(&Limits::new(1 << 10)),
        Err(Refusal::Budget("the caller's output ceiling"))
    );
}

// --- COD, COC, QCD, QCC -------------------------------------------------

/// A COD's five progression orders, and a sixth value refused rather than
/// defaulted to LRCP. Defaulting is the shape that produces a plausible
/// picture: the packets are all there, just read in the wrong order.
#[test]
fn cod_reads_the_five_progression_orders_and_refuses_a_sixth() {
    let want = [
        Progression::Lrcp,
        Progression::Rlcp,
        Progression::Rpcl,
        Progression::Pcrl,
        Progression::Cprl,
    ];
    for (byte, order) in want.into_iter().enumerate() {
        let spec = Spec {
            progression: byte as u8,
            ..Spec::default()
        };
        let bytes = spec.codestream(&[]);
        assert_eq!(parse(&bytes).expect("valid").cod.progression, order);
    }
    let spec = Spec {
        progression: 5,
        ..Spec::default()
    };
    assert!(matches!(
        parse(&spec.codestream(&[])),
        Err(Refusal::Feature(_))
    ));
}

/// The five Table A.19 code-block style bits this build does not implement,
/// each refused. Segmentation symbols are the sixth and are implemented, so
/// they are not here.
#[test]
fn the_unsupported_code_block_styles_are_refused_one_by_one() {
    // BYPASS and TERMALL, and only those two. Both move where a coding pass's
    // *bytes* start rather than how its decisions are read, so both need a
    // length per pass out of the packet header (B.10.7's multiple codeword
    // segments) rather than anything tier-1 can do alone.
    for bit in [cb_style::BYPASS, cb_style::TERMALL] {
        let spec = Spec {
            cb_style: bit,
            ..Spec::default()
        };
        assert!(
            matches!(parse(&spec.codestream(&[])), Err(Refusal::Feature(_))),
            "code-block style bit {bit:#04X} was not refused"
        );
    }

    // The three that are now decoded are *accepted* here, which is the half of
    // this test that would otherwise quietly stop meaning anything: a build
    // that refused them again would pass a test that only checked refusals.
    for bit in [
        cb_style::RESET,
        cb_style::VERTICALLY_CAUSAL,
        cb_style::PREDICTABLE,
    ] {
        let spec = Spec {
            cb_style: bit,
            ..Spec::default()
        };
        assert!(
            parse(&spec.codestream(&[])).is_ok(),
            "code-block style bit {bit:#04X} is implemented and must be accepted"
        );
    }

    // And a bit the table does not define at all.
    let spec = Spec {
        cb_style: 0x40,
        ..Spec::default()
    };
    assert!(matches!(
        parse(&spec.codestream(&[])),
        Err(Refusal::Feature(_))
    ));
}

/// Table A.18's code-block bounds: exponents 2 to 10 with a sum at most 12.
#[test]
fn code_block_sizes_outside_table_a18_are_refused() {
    for (xcb, ycb) in [(9u8, 0u8), (0, 9), (5, 5), (255, 0)] {
        let spec = Spec {
            xcb,
            ycb,
            ..Spec::default()
        };
        assert!(
            matches!(parse(&spec.codestream(&[])), Err(Refusal::Structure(_))),
            "xcb {xcb} ycb {ycb} was accepted"
        );
    }
    // 64 x 64, the common case, and 4 x 4, the smallest.
    for (xcb, ycb) in [(4u8, 4u8), (0, 0)] {
        let spec = Spec {
            xcb,
            ycb,
            ..Spec::default()
        };
        let bytes = spec.codestream(&[]);
        let stream = parse(&bytes).expect("a legal code-block size");
        assert_eq!(stream.cod.style.cb_width, xcb + 2);
    }
}

/// A.6.1's default precinct is 2^15 on every resolution, which is larger than
/// any legal code-block partition — that is what makes "no precincts
/// signalled" and "one precinct per subband" the same thing.
#[test]
fn precincts_default_to_the_whole_subband_and_are_read_when_signalled() {
    let spec = Spec {
        levels: 2,
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let stream = parse(&bytes).expect("valid");
    assert_eq!(stream.cod.style.precincts, vec![(15, 15); 3]);

    let spec = Spec {
        levels: 2,
        scod: 0x01,
        precincts: vec![0x66, 0x77, 0x88],
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let stream = parse(&bytes).expect("valid");
    assert_eq!(stream.cod.style.precincts, vec![(6, 6), (7, 7), (8, 8)]);

    // A.6.1: a zero exponent is legal only at resolution 0.
    let spec = Spec {
        levels: 1,
        scod: 0x01,
        precincts: vec![0x00, 0x66],
        ..Spec::default()
    };
    assert!(parse(&spec.codestream(&[])).is_ok());
    let spec = Spec {
        levels: 1,
        scod: 0x01,
        precincts: vec![0x66, 0x00],
        ..Spec::default()
    };
    assert!(matches!(
        parse(&spec.codestream(&[])),
        Err(Refusal::Structure(_))
    ));
}

/// Table A.28's three quantisation styles, and the guard bits that ride in
/// the same byte. The guard-bit count is attacker-controlled and is what
/// milestone 4's clamp is computed from, so reading it wrong is not cosmetic.
#[test]
fn qcd_reads_its_style_guard_bits_and_step_sizes() {
    let spec = Spec {
        levels: 1,
        quant_style: 0,
        guard_bits: 2,
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let q = &parse(&bytes).expect("valid").qcd;
    assert_eq!(q.style, QuantStyle::None);
    assert_eq!(q.guard_bits, 2);
    assert_eq!(q.steps.len(), 4);
    assert_eq!(q.steps[0], (8, 0));

    let spec = Spec {
        levels: 1,
        quant_style: 2,
        guard_bits: 7,
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let q = &parse(&bytes).expect("valid").qcd;
    assert_eq!(q.style, QuantStyle::Expounded);
    assert_eq!(q.guard_bits, 7);
    assert_eq!(q.steps, vec![(8, 0); 4]);

    let spec = Spec {
        quant_style: 3,
        ..Spec::default()
    };
    assert!(matches!(
        parse(&spec.codestream(&[])),
        Err(Refusal::Feature(_))
    ));
}

/// **COC and QCC override per component**, which is the exit criterion's own
/// words. A.6.2 and A.6.5 make the override per *component*, and a decoder
/// that applies the main-header default to every component decodes a chroma
/// plane with a luma plane's decomposition depth — which produces a picture.
#[test]
fn coc_and_qcc_override_one_component_and_leave_the_others() {
    let spec = Spec {
        components: vec![(8, false, 1, 1); 3],
        levels: 5,
        ..Spec::default()
    };
    let mut coc = vec![1u8, 0x00]; // component 1, no precincts signalled
    coc.extend_from_slice(&[2, 4, 4, 0, 1]); // two levels rather than five
    let mut qcc = vec![1u8, (3 << 5) | 2];
    for _ in 0..7 {
        qcc.extend_from_slice(&(9u16 << 11).to_be_bytes());
    }

    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(marker::COC, &coc));
    bytes.extend_from_slice(&segment(marker::QCC, &qcc));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());

    let stream = parse(&bytes).expect("valid");
    assert_eq!(stream.cod.style.levels, 5);
    assert_eq!(stream.coc[0], None);
    assert_eq!(
        stream.coc[1]
            .as_ref()
            .expect("a COC for component 1")
            .levels,
        2
    );
    assert_eq!(stream.coc[2], None);
    assert_eq!(stream.qcc[0], None);
    let q = stream.qcc[1].as_ref().expect("a QCC for component 1");
    assert_eq!((q.guard_bits, q.style), (3, QuantStyle::Expounded));
    assert_eq!(stream.qcd.guard_bits, 2);
}

#[test]
fn a_coc_or_qcc_naming_a_component_siz_did_not_is_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(marker::COC, &[9, 0x00, 1, 4, 4, 0, 1]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("COC names a component SIZ did not"))
    );
}

// --- SIZ, SOT and the rest ----------------------------------------------

/// Non-goal: component precision above 16 bits is refused, not truncated.
/// T.800 allows 38; PDF's sample path reads at most 16 and the fixed-point
/// format of milestone 5 is proved for 16.
#[test]
fn precision_above_sixteen_bits_is_refused_by_name() {
    let spec = Spec {
        components: vec![(17, false, 1, 1)],
        ..Spec::default()
    };
    assert_eq!(parse(&spec.codestream(&[])), Err(Refusal::Precision(17)));
    let spec = Spec {
        components: vec![(38, true, 1, 1)],
        ..Spec::default()
    };
    assert_eq!(parse(&spec.codestream(&[])), Err(Refusal::Precision(38)));
    let spec = Spec {
        components: vec![(16, true, 1, 1)],
        ..Spec::default()
    };
    assert!(
        parse(&spec.codestream(&[])).is_ok(),
        "16 bits is the boundary and is in"
    );
}

/// A.5.1: `Lsiz` is `38 + 3 * Csiz`. A `Csiz` that disagrees with the length
/// means the component list and the marker disagree about where it ends, and
/// trusting either one is a guess.
#[test]
fn a_siz_length_that_disagrees_with_csiz_is_refused() {
    let spec = Spec {
        csiz_override: Some(2),
        ..Spec::default()
    };
    assert_eq!(
        parse(&spec.codestream(&[])),
        Err(Refusal::Structure("a SIZ length that does not match Csiz"))
    );
}

/// A.5.1 Table A.10: 0, 1 and 2 restrict Part 1 rather than extending it and
/// decode identically. Everything above is a capability this build cannot
/// claim, which is where Part 2's `Rsiz` values land.
#[test]
fn an_rsiz_beyond_part_1_is_refused() {
    for rsiz in [0u16, 1, 2] {
        let spec = Spec {
            rsiz,
            ..Spec::default()
        };
        assert!(parse(&spec.codestream(&[])).is_ok(), "Rsiz {rsiz}");
    }
    for rsiz in [3u16, 0x8000, 0xFFFF] {
        let spec = Spec {
            rsiz,
            ..Spec::default()
        };
        assert!(
            matches!(parse(&spec.codestream(&[])), Err(Refusal::Feature(_))),
            "Rsiz {rsiz}"
        );
    }
}

/// A.4.2's rules about tile-parts, and **the line between the two failures
/// that live here**.
///
/// A codestream contradicting itself is refused: parts out of order, or an SOT
/// naming a tile outside the grid, would both produce a picture if
/// reassembled in stream order, and the picture would be wrong in a way that
/// looks like compression.
///
/// A codestream that *stops early* is not the same thing. A tile short of its
/// declared parts is a file that ended, which costs pixels rather than
/// meaning — the same bargain `JxrWarning::TileDroppedAsZero` strikes and the
/// one a fax row strikes. It is marked rather than refused, and only a
/// codestream with no whole tile at all is a refusal, because a page of
/// rectangles reported as a successful decode is worse than the placeholder.
#[test]
fn tile_parts_out_of_order_are_refused_and_short_ones_are_marked() {
    let spec = Spec::default();

    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 1, 2, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("tile-parts out of order"))
    );

    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(1, 0, 1, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("an SOT naming a tile outside the grid"))
    );

    // One tile short of its three declared parts, and it is the only tile:
    // nothing arrived whole, so there is no picture to degrade to.
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 3, &[], &[]));
    bytes.extend_from_slice(&tile_part(0, 1, 3, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("a codestream with no complete tile"))
    );

    // Two tiles, one whole and one short. The codestream parses, and the short
    // one is marked for the caller to leave blank.
    // A four-by-four image in two two-by-four tiles.
    let spec = Spec {
        xtsiz: 2,
        ..Spec::default()
    };
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &EMPTY_PACKETS));
    bytes.extend_from_slice(&tile_part(1, 0, 3, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    let stream = parse(&bytes).expect("one whole tile is a picture");
    assert_eq!(
        stream.short_tiles,
        vec![false, true],
        "the second tile declared three parts and one arrived"
    );
}

/// A.6.1: a coding style marker may only appear in a tile's *first* part.
/// Honouring one in a later part would change how the earlier parts of the
/// same tile should have been read, after they were read.
#[test]
fn a_coding_style_marker_in_a_later_tile_part_is_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 2, &[], &[]));
    bytes.extend_from_slice(&tile_part(0, 1, 2, &spec.cod(), &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure(
            "a coding style marker in a tile-part after the first"
        ))
    );
}

/// A.4: once the tile-parts have started, only SOT and EOC remain at that
/// level. A main-header marker there would apply to some tiles and not
/// others, which is a picture rather than an error.
#[test]
fn a_main_header_marker_after_a_tile_part_is_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    bytes.extend_from_slice(&spec.qcd());
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("a main header marker after a tile-part"))
    );
}

#[test]
fn a_codestream_missing_soc_siz_cod_or_qcd_is_refused() {
    let spec = Spec::default();
    assert_eq!(
        parse(&[0xFF, 0x90]),
        Err(Refusal::Structure(
            "a codestream that does not open with SOC"
        ))
    );
    let mut bytes = marker::SOC.to_be_bytes().to_vec();
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("a codestream with no SIZ marker"))
    );

    let mut bytes = marker::SOC.to_be_bytes().to_vec();
    bytes.extend_from_slice(&spec.siz());
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("a codestream with no COD marker"))
    );

    let mut bytes = marker::SOC.to_be_bytes().to_vec();
    bytes.extend_from_slice(&spec.siz());
    bytes.extend_from_slice(&spec.cod());
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("a codestream with no QCD marker"))
    );
}

/// Ruling 1, the general form: no input panics, hangs or reads past its end.
/// Every prefix of a valid file, and every single-byte corruption of one.
#[test]
fn no_prefix_or_corruption_panics() {
    let file = Spec {
        components: vec![(8, false, 1, 1); 3],
        levels: 2,
        ..Spec::default()
    }
    .jp2(&[(0, &[1, 2, 3, 4])]);
    for n in 0..=file.len() {
        let mut warnings = Vec::new();
        let _ = jpx_decode(&file[..n], &Limits::new(1 << 16), &mut warnings);
    }
    for i in 0..file.len() {
        let mut damaged = file.clone();
        damaged[i] ^= 0xFF;
        let mut warnings = Vec::new();
        let _ = jpx_decode(&damaged, &Limits::new(1 << 16), &mut warnings);
    }
}

// --- the public boundary ------------------------------------------------
//
// The whole degradation contract -- every refusal reaching the caller as the
// named capability with exactly one warning saying which class it was -- is
// asserted in `super::refusals`, over the plan's refusal list entry by entry
// rather than over the five cases that lived here while the list was still
// being built. Two tests moved there with milestone 8, and this note is where
// a reader looking for them lands.

/// T.800 A.9.1: a CRG marker segment is parsed, carried and never applied.
///
/// The clause is unusually explicit — "This marker segment has no effect on
/// decoding the codestream" — so honouring it means reading it and leaving
/// every sample alone. Refusing a file for carrying it, which this build did
/// until A.9.1 was read, refused a conforming file for saying something true
/// about itself.
///
/// The pairs are **interleaved** per component, `Xcrg_0, Ycrg_0, Xcrg_1,
/// Ycrg_1`. Figure A.23 is what settles that: the prose says "This value is
/// repeated for every component" separately of `Xcrg_i` and of `Ycrg_i`,
/// which on its own is ambiguous between interleaved and grouped. This test
/// uses two components with four distinct values, so a build that read them
/// grouped would produce `(0x0102, 0x0304)` for the first component instead
/// of `(0x0102, 0x8000)` and fail here rather than silently.
#[test]
fn a_crg_marker_is_parsed_and_carried() {
    let spec = Spec {
        components: vec![(7, false, 1, 1), (7, false, 1, 1)],
        ..Default::default()
    };
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(
        marker::CRG,
        &[
            0x01, 0x02, // Xcrg_0
            0x80, 0x00, // Ycrg_0: half of YRsiz_0
            0x03, 0x04, // Xcrg_1
            0xFF, 0xFF, // Ycrg_1: just before the next sample's grid point
        ],
    ));
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());

    let parsed = parse(&bytes).expect("a CRG marker segment is not a refusal");
    let registration = parsed.registration.expect("the CRG was carried");
    assert_eq!(
        registration,
        vec![
            codestream::Registration {
                x: 0x0102,
                y: 0x8000
            },
            codestream::Registration {
                x: 0x0304,
                y: 0xFFFF
            },
        ]
    );
}

/// A codestream with no CRG carries no registration, which is what says the
/// field above means "the file said so" rather than "the parser defaulted".
#[test]
fn a_codestream_without_crg_carries_no_registration() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(parse(&bytes).expect("parses").registration, None);
}

/// Table A.42 fixes `Lcrg` at `2 + 4 × Csiz`, so a segment of any other length
/// is malformed rather than merely long.
///
/// Both directions, because the tolerant reading is the dangerous one: a
/// decoder that took the first `Csiz` pairs out of a longer segment would be
/// inventing a latitude the table does not give, and one that accepted a short
/// segment would read a registration for a component the file never described.
#[test]
fn a_crg_whose_length_is_not_four_bytes_per_component_is_refused() {
    let spec = Spec {
        components: vec![(7, false, 1, 1), (7, false, 1, 1)],
        ..Default::default()
    };
    for body in [
        &[0x01, 0x02, 0x03, 0x04][..], // one pair short
        &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A][..], // two bytes long
    ] {
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&segment(marker::CRG, body));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        assert!(
            matches!(parse(&bytes), Err(Refusal::Structure(_))),
            "a CRG of {} bytes against 2 components",
            body.len()
        );
    }
}

/// A.9.1: "Only one CRG may be used in the main header."
#[test]
fn a_second_crg_marker_is_refused() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&segment(marker::CRG, &[0x00, 0x00, 0x00, 0x00]));
    bytes.extend_from_slice(&segment(marker::CRG, &[0x00, 0x00, 0x00, 0x00]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(matches!(parse(&bytes), Err(Refusal::Structure(_))));
}

/// A.9.1: "Usage: Main header only." Table A.2 says the same, so a CRG in a
/// tile-part header is a marker out of place rather than one this build
/// refuses by name — a distinction a reader of the warning depends on.
#[test]
fn a_crg_in_a_tile_part_header_is_out_of_place() {
    let spec = Spec::default();
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(
        0,
        0,
        1,
        &segment(marker::CRG, &[0x00, 0x00, 0x00, 0x00]),
        &[],
    ));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert!(
        matches!(parse(&bytes), Err(Refusal::Structure(_))),
        "a CRG in a tile-part header"
    );
}
