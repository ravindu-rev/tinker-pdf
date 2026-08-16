//! Milestone 1: the container (Annex I) and the codestream headers (Annex A).

use super::writer::{boxed, segment, tile_part, Spec, SIGNATURE};
use crate::jpx::codestream::{marker, refuse_marker, Progression, QuantStyle};
use crate::jpx::{boxes, codestream, jpx_decode, JpxColour, Refusal};
use crate::{Capability, FilterError, Limits, Warning};

/// The bytes of a codestream, parsed, or the refusal it earned.
fn parse(bytes: &[u8]) -> Result<codestream::Codestream<'_>, Refusal> {
    codestream::parse(bytes)
}

/// A minimal 4 x 4 greyscale stream with one empty tile-part.
/// A codestream this build parses and still cannot finish.
///
/// It has to be *chosen* rather than assumed, and the choice moves as the
/// decoder grows. Until milestone 4 the minimal single-component fixture was
/// the example, because nothing turned coefficients into samples; now that
/// one decodes end to end and this is three components, which needs the
/// colour pipeline that is milestone 6.
///
/// The pattern is worth naming: a test asserting "not built yet" has a
/// half-life, and the honest maintenance is to move it to whatever is
/// genuinely next rather than to keep it passing.
fn unfinishable() -> Vec<u8> {
    let spec = Spec {
        components: vec![(8, false, 1, 1); 3],
        ..Spec::default()
    };
    spec.codestream(&[(0, &EMPTY_PACKETS_3C)])
}

/// Two packets per resolution per component: three components, two
/// resolutions, one layer.
const EMPTY_PACKETS_3C: [u8; 6] = [0x00; 6];

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

/// `pclr`, `cmap` and `cdef` are recorded rather than applied. Recording is
/// what lets milestone 6 refuse a palette it cannot map instead of rendering
/// the indices as grey.
#[test]
fn palette_and_channel_boxes_are_recorded_not_ignored() {
    let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]);
    jp2h.extend_from_slice(&boxed(*b"pclr", &[0, 2, 1, 7, 0, 255]));
    jp2h.extend_from_slice(&boxed(*b"cmap", &[0, 0, 1, 0]));
    jp2h.extend_from_slice(&boxed(*b"cdef", &[0, 1, 0, 0, 0, 0]));
    jp2h.extend_from_slice(&boxed(*b"res ", &[]));
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    file.extend_from_slice(&boxed(*b"jp2c", &minimal()));
    let header = boxes::parse(&file).expect("valid").header.expect("jp2h");
    assert!(header.palette && header.component_map && header.channel_definition);
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
/// Not "most", and not "the ones a file is likely to hold". A skipped RGN
/// draws a bright rectangle where the region of interest was; a skipped POC
/// changes the packet order mid-stream and mis-parses every packet after it.
/// Both produce a picture. This test walks the whole table and demands that
/// each marker lands in exactly one of the two buckets — which is checkable
/// only because [`Refusal`] carries the name rather than collapsing to a
/// warning.
#[test]
fn every_table_a2_marker_is_parsed_or_named() {
    /// T.800 Table A.2, transcribed. Twenty markers.
    const TABLE_A2: [(u16, &str, bool); 20] = [
        (marker::SOC, "SOC", true),
        (marker::SOT, "SOT", true),
        (marker::SOD, "SOD", true),
        (marker::EOC, "EOC", true),
        (marker::SIZ, "SIZ", true),
        (marker::COD, "COD", true),
        (marker::COC, "COC", true),
        (marker::RGN, "RGN", false),
        (marker::QCD, "QCD", true),
        (marker::QCC, "QCC", true),
        (marker::POC, "POC", false),
        (marker::TLM, "TLM", true),
        (marker::PLM, "PLM", true),
        (marker::PLT, "PLT", true),
        (marker::PPM, "PPM", false),
        (marker::PPT, "PPT", false),
        (marker::SOP, "SOP", true),
        (marker::EPH, "EPH", true),
        (marker::CRG, "CRG", false),
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
    for (code, name, _) in TABLE_A2 {
        // Every marker that may appear in a main header, put in one. The four
        // delimiters and the two in-packet markers are covered by their own
        // tests; what this asks of them is that reaching one here is still a
        // refusal that names it rather than a skip.
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&segment(code, &[0, 0]));
        bytes.extend_from_slice(&tile_part(0, 0, 1, &[], &[]));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        let got = parse(&bytes);
        match got {
            // Parsed: TLM, PLM, PLT and COM carry no coding information, so a
            // main header holding one still parses.
            Ok(_) => assert!(
                matches!(code, marker::TLM | marker::PLM | marker::PLT | marker::COM),
                "{name} parsed and should not have"
            ),
            Err(Refusal::Marker(text)) => assert!(
                text.starts_with(name),
                "{name} was refused as {text:?}, which does not name it"
            ),
            // The delimiters are structural where this test puts them: a
            // second SIZ, an SOD with no SOT, an SOC in the middle. Refused,
            // and never *skipped*.
            Err(Refusal::Structure(_) | Refusal::Truncated(_) | Refusal::Feature(_)) => {}
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

/// The five Table A.2 markers this build refuses, each in a main header or a
/// tile-part header, each naming itself.
#[test]
fn the_five_refused_markers_name_themselves() {
    let spec = Spec::default();
    for (code, name) in [
        (marker::RGN, "RGN"),
        (marker::POC, "POC"),
        (marker::PPM, "PPM"),
        (marker::CRG, "CRG"),
    ] {
        let mut bytes = spec.main_header();
        bytes.extend_from_slice(&segment(code, &[0, 0, 0]));
        bytes.extend_from_slice(&marker::EOC.to_be_bytes());
        match parse(&bytes) {
            Err(Refusal::Marker(text)) => assert!(text.starts_with(name), "{text:?}"),
            other => panic!("{name} produced {other:?}"),
        }
    }
    // PPT lives in a tile-part header rather than the main one.
    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 1, &segment(marker::PPT, &[0]), &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    match parse(&bytes) {
        Err(Refusal::Marker(text)) => assert!(text.starts_with("PPT"), "{text:?}"),
        other => panic!("PPT produced {other:?}"),
    }
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
    for bit in [0x01u8, 0x02, 0x04, 0x08, 0x10] {
        let spec = Spec {
            cb_style: bit,
            ..Spec::default()
        };
        assert!(
            matches!(parse(&spec.codestream(&[])), Err(Refusal::Feature(_))),
            "code-block style bit {bit:#04X} was not refused"
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

/// A.4.2's rules about tile-parts, each of which is on the refusal list
/// because reassembling them in stream order regardless produces a picture.
#[test]
fn tile_parts_out_of_order_or_short_of_their_count_are_refused() {
    let spec = Spec::default();

    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 1, 2, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("tile-parts out of order"))
    );

    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(0, 0, 3, &[], &[]));
    bytes.extend_from_slice(&tile_part(0, 1, 3, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("a tile whose parts do not cover it"))
    );

    let mut bytes = spec.main_header();
    bytes.extend_from_slice(&tile_part(1, 0, 1, &[], &[]));
    bytes.extend_from_slice(&marker::EOC.to_be_bytes());
    assert_eq!(
        parse(&bytes),
        Err(Refusal::Structure("an SOT naming a tile outside the grid"))
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

/// The whole degradation contract in one test: every refusal is the named
/// capability, and every one leaves exactly one warning saying which class it
/// was. The caller draws the placeholder (ruling 2) and reports the reason
/// (ruling 10).
#[test]
fn the_public_entry_point_refuses_with_the_capability_and_one_warning() {
    let cases: [(&str, Vec<u8>, Warning); 5] = [
        (
            "a marker T.800 defines and this build does not",
            {
                let spec = Spec::default();
                let mut b = spec.main_header();
                b.extend_from_slice(&segment(marker::RGN, &[0, 0, 0]));
                b.extend_from_slice(&marker::EOC.to_be_bytes());
                b
            },
            Warning::JpxMarkerUnsupported,
        ),
        (
            "a marker Table A.2 does not define",
            {
                let spec = Spec::default();
                let mut b = spec.main_header();
                b.extend_from_slice(&segment(0xFF74, &[0]));
                b.extend_from_slice(&marker::EOC.to_be_bytes());
                b
            },
            Warning::JpxMarkerUnknown,
        ),
        (
            "a zero tile size",
            Spec {
                xtsiz: 0,
                ..Spec::default()
            }
            .codestream(&[]),
            Warning::JpxStructureInvalid,
        ),
        (
            "precision above sixteen bits",
            Spec {
                components: vec![(24, false, 1, 1)],
                ..Spec::default()
            }
            .codestream(&[]),
            Warning::JpxPrecisionUnsupported,
        ),
        (
            "a codestream this build parses and cannot yet finish",
            unfinishable(),
            Warning::JpxStageNotBuilt,
        ),
    ];
    for (what, bytes, want) in cases {
        let mut warnings = Vec::new();
        let got = jpx_decode(&bytes, &Limits::new(1 << 20), &mut warnings);
        assert_eq!(
            got,
            Err(FilterError::Unsupported(Capability::Jpx)),
            "{what} did not refuse with the capability"
        );
        assert_eq!(warnings, vec![want], "{what}");
    }
}

/// Ruling 10: the warning set is closed and each variant is recorded at most
/// once per decode, so a stream of a million bad markers cannot turn leniency
/// into an allocation attack.
#[test]
fn a_decode_leaves_at_most_one_warning() {
    let mut warnings = Vec::new();
    let _ = jpx_decode(&unfinishable(), &Limits::new(1 << 20), &mut warnings);
    let _ = jpx_decode(&unfinishable(), &Limits::new(1 << 20), &mut warnings);
    assert_eq!(warnings, vec![Warning::JpxStageNotBuilt]);
}
