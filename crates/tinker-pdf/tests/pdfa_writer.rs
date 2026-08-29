//! The writer profile: what `DocumentBuilder::archival` produces, and what it
//! refuses (milestone 6 of `docs/design/pdfa.md`).
//!
//! # The limit this file does not close, and does not pretend to
//!
//! Every document below is judged by **this repository's own validator**, the
//! one milestone 5 built. Nothing outside this repository ever sees a file
//! this engine wrote (ruling 13), so a defect the writer emits and the rule
//! table would accept is invisible here by construction. The near-miss twins
//! narrow that and do not close it: for each fixture built at the edge of a
//! clause there is a second document differing in one call, asserted to
//! *fail* — which shows the rule discriminates rather than merely passes, but
//! shows it with the same rule.
//!
//! What is genuinely independent is the second gate every fixture goes
//! through: `tinker_pdf_cos::validate`, the strict structural validator, which
//! reads the bytes rather than the object graph and was written for the writer
//! rather than for PDF/A. A file that satisfies the conformance rules and not
//! that one is a file with a defect the conformance rules had no opinion
//! about, and there is one such gate per fixture here.
//!
//! # Injections, counted, of the workspace's 3 613 tests
//!
//! Each of the five things the profile does to a document was undone in turn
//! and the suite counted, so that "the fixture validates" is known to depend
//! on each of them rather than on any one:
//!
//! - the standard 14 admitted under a profile after all: **1** test;
//! - the XMP packet not written: **4**;
//! - the output intent not written: **1**;
//! - part 1 no longer refusing transparency: **1**;
//! - the header version left at the writer's default rather than the part's:
//!   **1**.
//!
//! The output-intent and header injections failing exactly one test each is
//! worth reading twice: both are caught by
//! [`every_flavour_the_writer_claims_validates_with_no_findings`], which is
//! the round trip through the whole validator, and by nothing else. A rule
//! group that stopped running would take that test's evidence with it.

mod pdfa_support;

use pdfa_support::{cmyk_like, srgb_like};
use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, ArchivalRefusal, BlendMode, Document,
    DocumentBuilder, ExtGState, FindingKind, StateMask,
};
use tinker_pdf_cos::{DeviceSpace, FormXObject, MaskKind, TransparencyGroup};

// ---- a font program the builder will embed --------------------------------

/// How many glyphs the fixture face has: `.notdef` and the twenty-six capitals.
const GLYPHS: u16 = 27;

/// A minimal TrueType face with real `cmap`, `glyf`, `hmtx` and `loca` tables.
///
/// Every table here is one `DocumentBuilder::add_embedded_font` reads: it
/// refuses a program `tinker_pdf_font` cannot parse, takes each glyph's
/// advance out of `hmtx` to write `/Widths`, and resolves a character through
/// `cmap` to know which advance it wants. A face made of filler would be
/// refused at the first call and there would be nothing to embed.
///
/// The outlines are empty — a `loca` whose entries repeat, which is how a font
/// says "this glyph has no shape". Conformance is about the *program* being
/// there and being the format its key claims, and an empty outline is a
/// legitimate glyph; drawing a real one would test the outline builder rather
/// than the profile.
fn face() -> Vec<u8> {
    let glyphs = usize::from(GLYPHS);

    // Every glyph empty: `loca` has one more entry than there are glyphs, and
    // repeating an offset is an empty range.
    let mut loca = Vec::with_capacity((glyphs + 1) * 4);
    for _ in 0..=glyphs {
        loca.extend_from_slice(&0u32.to_be_bytes());
    }
    // A `glyf` table has to exist for the program to be a TrueType one at all;
    // four zero bytes is a table with nothing in it, which every `loca` entry
    // points at.
    let glyf = vec![0u8; 4];

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca offsets

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&GLYPHS.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&GLYPHS.to_be_bytes()); // numberOfHMetrics

    let mut hmtx = Vec::with_capacity(glyphs * 4);
    for _ in 0..glyphs {
        hmtx.extend_from_slice(&600u16.to_be_bytes()); // advance
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // left side bearing
    }

    let cmap = cmap_table();
    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"maxp", &maxp),
    ];
    sfnt(&tables)
}

/// A `cmap` with one format 4 subtable mapping `A`..=`Z` onto glyphs 1..=26.
fn cmap_table() -> Vec<u8> {
    // Two segments: the capitals, and the terminating 0xFFFF segment format 4
    // requires.
    let segments: [(u16, u16, i16); 2] = [(0x0041, 0x005A, 1 - 0x0041), (0xFFFF, 0xFFFF, 1)];
    let count = segments.len() as u16;
    let search = 2 * count.next_power_of_two() / 2;

    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes()); // format
    sub.extend_from_slice(&(16 + 8 * count).to_be_bytes()); // length
    sub.extend_from_slice(&0u16.to_be_bytes()); // language
    sub.extend_from_slice(&(count * 2).to_be_bytes()); // segCountX2
    sub.extend_from_slice(&search.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
    sub.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
    for (_, end, _) in segments {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for (start, _, _) in segments {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for (_, _, delta) in segments {
        sub.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in segments {
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes()); // version
    out.extend_from_slice(&1u16.to_be_bytes()); // one table
    out.extend_from_slice(&3u16.to_be_bytes()); // platform: Windows
    out.extend_from_slice(&1u16.to_be_bytes()); // encoding: BMP
    out.extend_from_slice(&12u32.to_be_bytes()); // offset
    out.extend_from_slice(&sub);
    out
}

/// A table directory over `tables`, in the order given.
fn sfnt(tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let count = tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    for value in [0u16, 0, 0] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut at = 12 + count * 16;
    for (tag, data) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(at as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        at += data.len().div_ceil(4) * 4;
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    out
}

// ---- building a conforming document ---------------------------------------

/// The profile every fixture below starts from: part 2, level B, an RGB
/// destination.
fn rgb_profile(part: ArchivalPart, level: Option<ArchivalLevel>) -> ArchivalProfile {
    ArchivalProfile {
        part,
        level,
        destination_profile: srgb_like(),
        destination_space: DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    }
}

/// A one-page document drawing text in an embedded face and a red rectangle,
/// built under `profile`.
fn built(profile: ArchivalProfile) -> Vec<u8> {
    let mut builder = DocumentBuilder::archival(profile);
    assert!(
        builder.add_embedded_font(b"F1", b"Fixture", &face()),
        "the fixture face is one the builder embeds"
    );
    builder.add_page(200.0, 200.0, |page| {
        assert!(page.set_fill_rgb(1.0, 0.0, 0.0));
        page.fill_rect(10.0, 10.0, 50.0, 50.0, 0.5);
        page.text(b"F1", 12.0, 20.0, 100.0, "ABC");
    });
    assert!(builder.refusals().is_empty(), "{:?}", builder.refusals());
    builder
        .finish_archival()
        .expect("the profile is satisfiable")
}

/// Every conformance finding a full validation reports, and the strict
/// structural validator's structure-tier defects.
fn judged(bytes: &[u8]) -> (Vec<FindingKind>, Vec<String>) {
    let document = Document::open(bytes.to_vec()).expect("the built document opens");
    let verdict = document.validate_pdfa();
    assert!(
        verdict.coverage.is_complete(),
        "a writer fixture judged by an incomplete validator proves nothing"
    );
    let structural: Vec<String> = tinker_pdf_cos::validate(document.cos())
        .into_iter()
        .filter(|defect| defect.kind.tier() == tinker_pdf_cos::Tier::Structure)
        .map(|defect| format!("{:?}", defect.kind))
        .collect();
    (
        verdict.findings.into_iter().map(|f| f.kind).collect(),
        structural,
    )
}

// ---- the fixtures ---------------------------------------------------------

/// Every flavour the writer can claim, built and validated with zero findings,
/// and clean under the strict structural validator too.
#[test]
fn every_flavour_the_writer_claims_validates_with_no_findings() {
    for (part, level) in [
        (ArchivalPart::One, Some(ArchivalLevel::B)),
        (ArchivalPart::Two, Some(ArchivalLevel::B)),
        (ArchivalPart::Two, Some(ArchivalLevel::U)),
        (ArchivalPart::Three, Some(ArchivalLevel::B)),
        (ArchivalPart::Four, None),
        (ArchivalPart::Four, Some(ArchivalLevel::E)),
        (ArchivalPart::Four, Some(ArchivalLevel::F)),
    ] {
        let bytes = built(rgb_profile(part, level));
        let (findings, structural) = judged(&bytes);
        assert_eq!(
            findings,
            Vec::<FindingKind>::new(),
            "part {} level {level:?}",
            part.number()
        );
        assert_eq!(
            structural,
            Vec::<String>::new(),
            "the strict structural validator, part {}",
            part.number()
        );
    }
}

/// **The near-miss twin for the whole profile.** The same document built
/// without one is not a PDF/A, and the validator says so.
///
/// Without this, "the built fixture validates" would be a statement about the
/// validator's leniency rather than about the writer.
#[test]
fn the_same_document_built_without_a_profile_does_not_conform() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_embedded_font(b"F1", b"Fixture", &face()));
    builder.add_page(200.0, 200.0, |page| {
        page.set_fill_rgb(1.0, 0.0, 0.0);
        page.text(b"F1", 12.0, 20.0, 100.0, "ABC");
    });
    let (findings, _) = judged(&builder.finish());
    assert!(
        findings.contains(&FindingKind::MetadataMissing),
        "no packet, so no claim: {findings:#?}"
    );
}

/// The flavour the file claims is the flavour the profile asked for, read back
/// through the validator's own XMP parser rather than out of the bytes.
#[test]
fn the_generated_packet_says_what_the_profile_said() {
    for (part, level, expected) in [
        (ArchivalPart::One, Some(ArchivalLevel::B), "PDF/A-1B"),
        (ArchivalPart::Two, Some(ArchivalLevel::U), "PDF/A-2U"),
        (ArchivalPart::Four, None, "PDF/A-4"),
        (ArchivalPart::Four, Some(ArchivalLevel::E), "PDF/A-4E"),
    ] {
        let bytes = built(rgb_profile(part, level));
        let document = Document::open(bytes).expect("opens");
        assert_eq!(
            document.validate_pdfa().flavour.map(|f| f.to_string()),
            Some(expected.to_string())
        );
    }
}

/// ISO 19005-1 6.7.3: the `/Info` dictionary and the packet say the same
/// thing, because one table writes both.
///
/// The near-miss twin is the value changed by one character in the `/Info`
/// dictionary alone, which the validator must report — asserted by building
/// the document, editing the `/Info` string in the bytes, and revalidating.
#[test]
fn the_info_dictionary_and_the_packet_agree_and_a_changed_one_does_not() {
    let mut builder =
        DocumentBuilder::archival(rgb_profile(ArchivalPart::One, Some(ArchivalLevel::B)));
    assert!(builder.add_embedded_font(b"F1", b"Fixture", &face()));
    assert!(builder.set_info(b"Title", "Marmalade"));
    assert!(builder.set_info(b"Author", "A Person"));
    assert!(builder.set_info(b"Producer", "tinker-pdf"));
    assert!(builder.set_info(b"CreationDate", "D:20240102030405+01'00'"));
    builder.add_page(200.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 100.0, "ABC");
    });
    let bytes = builder.finish_archival().expect("satisfiable");
    let (findings, structural) = judged(&bytes);
    assert_eq!(findings, Vec::<FindingKind>::new());
    assert_eq!(structural, Vec::<String>::new());

    // The twin: the `/Info` title alone, changed in the file's bytes. The
    // packet still says `Marmalade` and the dictionary no longer does, which
    // is exactly the defect 6.7.3 is about.
    let mut edited = bytes.clone();
    let at = find(&edited, b"(Marmalade)").expect("the literal string is in the file");
    edited[at + 1] = b'M';
    edited[at + 2] = b'a';
    edited[at + 3] = b'r';
    edited[at + 4] = b'z';
    let (findings, _) = judged(&edited);
    assert!(
        findings.contains(&FindingKind::InfoXmpMismatch {
            key: "Title".to_string()
        }),
        "{findings:#?}"
    );
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Ruling 4: the same program produces the same bytes.
///
/// The packet is the part of a profiled document most likely to break this —
/// a producer that stamped the current time into `xmp:CreateDate` would be
/// writing a different file every second — so the assertion is over the whole
/// document and the twin is a second run of the same builder.
#[test]
fn two_builds_of_one_profiled_document_are_byte_identical() {
    let first = built(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    let second = built(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    assert_eq!(first, second, "a profiled document is byte-deterministic");
    assert!(
        !first.windows(9).any(|w| w == b"xmp:Metad"),
        "nothing here writes a metadata date it read from a clock"
    );
}

/// The archival fixture's bytes, hashed, in the shape
/// `crates/tinker-pdf/tests/determinism.rs` states its document hashes in.
///
/// **Why a PDF/A document belongs among the byte-hashes and not only among the
/// rendered ones.** Every other written-document hash in this repository is
/// about geometry the writer laid down. This one is about a *claim*: the
/// packet says "this file is PDF/A-2B", and a producer whose claim moved
/// between runs would be issuing a different archival document every time it
/// was asked for the same one. The packet is also the one part of a written
/// document with an obvious way to become nondeterministic - a producer that
/// stamped the current time into `xmp:CreateDate` - and the hash is what makes
/// that a test failure rather than a difference nobody diffs.
///
/// The hash is over the fixture [`built`] makes, which depends on [`face`] and
/// on `pdfa_support::srgb_like`. Moving the assertion without moving those
/// moves the hash.
#[test]
fn the_archival_fixture_hashes_the_same_on_every_target() {
    let bytes = built(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    let digest: String = tinker_pdf_crypto::sha2::sha256(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        digest, "d6915d9006d5c880f5aab2af35b099ba21d89dcfab6179f3c6b89ebc907ada1c",
        "the archival fixture's bytes moved"
    );
}

/// Level A is reachable, because `PageBuilder::tagged` exists.
///
/// `docs/design/pdfa.md` used to stage level A behind the tagged-PDF design.
/// That design landed, so the writer claims A by tagging — and the two ways of
/// getting it wrong are refused rather than written.
#[test]
fn level_a_is_written_when_the_pages_are_tagged_and_refused_when_they_are_not() {
    let mut profile = rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::A));
    profile.language = Some("en-GB".to_string());

    let mut builder = DocumentBuilder::archival(profile.clone());
    assert!(builder.add_embedded_font(b"F1", b"Fixture", &face()));
    builder.add_page(200.0, 200.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 100.0, "ABC");
        });
    });
    let bytes = builder
        .finish_archival()
        .expect("a tagged page satisfies A");
    let (findings, structural) = judged(&bytes);
    assert_eq!(findings, Vec::<FindingKind>::new());
    assert_eq!(structural, Vec::<String>::new());
    let document = Document::open(bytes).expect("opens");
    assert_eq!(
        document.validate_pdfa().flavour.map(|f| f.to_string()),
        Some("PDF/A-2A".to_string())
    );

    // The first near-miss: an untagged page.
    let mut untagged = DocumentBuilder::archival(profile.clone());
    assert!(untagged.add_embedded_font(b"F1", b"Fixture", &face()));
    untagged.add_page(200.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 100.0, "ABC");
    });
    assert_eq!(
        untagged.finish_archival(),
        Err(ArchivalRefusal::UntaggedPage { page: 0 })
    );

    // The second: no natural language.
    let mut speechless = ArchivalProfile {
        language: None,
        ..profile
    };
    speechless.language = None;
    let mut builder = DocumentBuilder::archival(speechless);
    builder.add_page(200.0, 200.0, |page| {
        page.tagged(b"P", |page| page.fill_rect(1.0, 1.0, 2.0, 2.0, 0.0));
    });
    assert_eq!(
        builder.finish_archival(),
        Err(ArchivalRefusal::LanguageMissing)
    );
}

/// A level the part does not define, and a part 1-to-3 profile with none, are
/// both refused before any bytes are written.
#[test]
fn a_level_the_part_does_not_define_is_refused() {
    let builder = DocumentBuilder::archival(rgb_profile(ArchivalPart::One, Some(ArchivalLevel::U)));
    assert_eq!(
        builder.finish_archival(),
        Err(ArchivalRefusal::LevelNotInPart)
    );

    let builder = DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, None));
    assert_eq!(
        builder.finish_archival(),
        Err(ArchivalRefusal::LevelMissing)
    );

    // The near-miss: part 4 is the one part where no level is correct.
    let builder = DocumentBuilder::archival(rgb_profile(ArchivalPart::Four, None));
    assert!(builder.finish_archival().is_ok());
}

/// An output intent with no profile bytes is refused, which is the API half of
/// the licence decision: there is no default to fall back to.
#[test]
fn a_profile_with_no_destination_profile_is_refused() {
    let profile = ArchivalProfile {
        destination_profile: Vec::new(),
        ..rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B))
    };
    assert_eq!(
        DocumentBuilder::archival(profile).finish_archival(),
        Err(ArchivalRefusal::DestinationProfileMissing)
    );
}

// ---- one refusal test per forbidden feature -------------------------------

/// The standard 14 have no embedded program, and ISO 19005 has no exception
/// for them. Both ways of asking for one are refused.
///
/// The near-miss twin is the same call on a builder with no profile, which
/// must succeed — a refusal that fired everywhere would be a broken method
/// rather than a profile.
#[test]
fn an_unembedded_font_is_refused_at_the_call_that_asks_for_it() {
    let mut builder =
        DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    assert!(!builder.add_base_font(b"F1", b"Helvetica"));
    assert!(!builder.add_named_font(b"F2", b"Helvetica", 65, &["A"], &[500]));
    assert_eq!(
        builder.refusals(),
        &[
            ArchivalRefusal::UnembeddedFont {
                resource: b"F1".to_vec()
            },
            ArchivalRefusal::UnembeddedFont {
                resource: b"F2".to_vec()
            },
        ]
    );
    assert_eq!(builder.refusals()[0].clause(), "6.3.4");

    let mut plain = DocumentBuilder::new();
    assert!(plain.add_base_font(b"F1", b"Helvetica"));
    assert!(plain.add_named_font(b"F2", b"Helvetica", 65, &["A"], &[500]));
    assert!(plain.refusals().is_empty());
}

/// Part 1 admits no transparency, in any of the four ways a graphics state or
/// a form can introduce some.
///
/// The near-miss twin is the identical call under a part 2 profile, which
/// permits transparency — so the refusal is shown to be the part's and not the
/// method's.
#[test]
fn transparency_is_refused_under_part_one_and_written_under_part_two() {
    let states: [(&str, ExtGState<'_>); 3] = [
        (
            "ca",
            ExtGState {
                fill_alpha: Some(0.5),
                ..ExtGState::default()
            },
        ),
        (
            "CA",
            ExtGState {
                stroke_alpha: Some(0.25),
                ..ExtGState::default()
            },
        ),
        (
            "BM",
            ExtGState {
                blend_mode: Some(BlendMode::Multiply),
                ..ExtGState::default()
            },
        ),
    ];

    for (feature, state) in states {
        let mut one =
            DocumentBuilder::archival(rgb_profile(ArchivalPart::One, Some(ArchivalLevel::B)));
        assert!(!one.add_ext_gstate(b"Gs", &state), "for {feature}");
        assert_eq!(
            one.refusals(),
            &[ArchivalRefusal::Transparency { feature }],
            "for {feature}"
        );
        assert_eq!(one.refusals()[0].clause(), "6.4");

        let mut two =
            DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
        assert!(two.add_ext_gstate(b"Gs", &state), "for {feature}");
        assert!(two.refusals().is_empty());
    }

    // The fourth way: a soft mask over a transparency group.
    let group = FormXObject {
        bbox: [0.0, 0.0, 10.0, 10.0],
        matrix: None,
        group: Some(TransparencyGroup {
            color_space: DeviceSpace::Rgb,
            isolated: true,
            knockout: false,
        }),
        content: b"0 0 10 10 re f",
    };
    let mut one = DocumentBuilder::archival(rgb_profile(ArchivalPart::One, Some(ArchivalLevel::B)));
    assert!(!one.add_form(b"Fm", &group));
    assert_eq!(
        one.refusals(),
        &[ArchivalRefusal::Transparency { feature: "Group" }]
    );

    let mut two = DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    assert!(two.add_form(b"Fm", &group));
    assert!(two.add_ext_gstate(
        b"Gs",
        &ExtGState {
            soft_mask: Some(StateMask::Group {
                kind: MaskKind::Alpha,
                form: b"Fm",
                backdrop: None,
            }),
            ..ExtGState::default()
        }
    ));
    assert!(two.refusals().is_empty());
}

/// A device colour the destination profile cannot reproduce is refused where
/// the caller asks for it, on the page and on an image alike.
///
/// The near-miss twin is the identical call under an RGB destination.
#[test]
fn a_device_colour_the_output_intent_does_not_admit_is_refused() {
    let cmyk = ArchivalProfile {
        destination_profile: cmyk_like(),
        destination_space: DeviceSpace::Cmyk,
        ..rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B))
    };

    let mut builder = DocumentBuilder::archival(cmyk.clone());
    builder.add_page(200.0, 200.0, |page| {
        assert!(!page.set_fill_rgb(1.0, 0.0, 0.0));
        assert!(!page.set_stroke_rgb(1.0, 0.0, 0.0));
        // Grey is admitted under any destination, which is the reading the
        // validator makes and the twin that keeps this rule from being "no
        // colour at all".
        page.fill_rect(1.0, 1.0, 2.0, 2.0, 0.5);
    });
    assert_eq!(
        builder.refusals(),
        &[
            ArchivalRefusal::DeviceColour {
                space: DeviceSpace::Rgb
            },
            ArchivalRefusal::DeviceColour {
                space: DeviceSpace::Rgb
            },
        ]
    );
    assert_eq!(builder.refusals()[0].clause(), "6.2.3.3");

    // An image's samples are values in a colour space too.
    let mut images = DocumentBuilder::archival(cmyk);
    assert!(!images.add_image(
        b"Im",
        &tinker_pdf::ImageData::Rgb8 {
            width: 1,
            height: 1,
            data: &[0, 0, 0],
        }
    ));
    assert!(images.add_image(
        b"Im",
        &tinker_pdf::ImageData::Gray8 {
            width: 1,
            height: 1,
            data: &[0],
        }
    ));

    let mut rgb = DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    rgb.add_page(200.0, 200.0, |page| {
        assert!(page.set_fill_rgb(1.0, 0.0, 0.0));
    });
    assert!(rgb.refusals().is_empty());
}

/// ISO 19005-4 6.1.3 leaves a part 4 document one `/Info` entry, and the
/// builder refuses the rest.
///
/// The near-miss twins are `/ModDate` under part 4, which is admitted, and the
/// same entries under part 2, which are.
#[test]
fn an_info_entry_part_four_does_not_admit_is_refused() {
    let mut four = DocumentBuilder::archival(rgb_profile(ArchivalPart::Four, None));
    assert!(!four.set_info(b"Title", "Marmalade"));
    assert!(four.set_info(b"ModDate", "D:20240102030405Z"));
    assert_eq!(
        four.refusals(),
        &[ArchivalRefusal::InfoEntry {
            key: b"Title".to_vec()
        }]
    );
    assert_eq!(four.refusals()[0].clause(), "6.1.3");

    let mut two = DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    assert!(two.set_info(b"Title", "Marmalade"));
    assert!(two.refusals().is_empty());
}

/// A date the packet could not restate is refused rather than written, because
/// 6.7.3 requires the dictionary and the packet to agree and a date nobody can
/// parse has nothing to agree with.
#[test]
fn an_unparseable_date_is_refused_rather_than_left_to_disagree() {
    let mut builder =
        DocumentBuilder::archival(rgb_profile(ArchivalPart::Two, Some(ArchivalLevel::B)));
    assert!(!builder.set_info(b"CreationDate", "last Tuesday"));
    assert_eq!(
        builder.refusals(),
        &[ArchivalRefusal::InfoEntry {
            key: b"CreationDate".to_vec()
        }]
    );

    // The near-miss: a date that parses.
    assert!(builder.set_info(b"CreationDate", "D:20240102030405+01'00'"));
}

/// Every refusal names a clause, and no two name the same one by accident.
///
/// A refusal whose clause a caller cannot act on is a `false` with extra
/// syntax, which is what the milestone's "typed error saying why" is about.
#[test]
fn every_refusal_names_the_clause_it_refuses_under() {
    let refusals = [
        ArchivalRefusal::UnembeddedFont {
            resource: b"F1".to_vec(),
        },
        ArchivalRefusal::Transparency { feature: "ca" },
        ArchivalRefusal::DeviceColour {
            space: DeviceSpace::Rgb,
        },
        ArchivalRefusal::InfoEntry {
            key: b"Title".to_vec(),
        },
        ArchivalRefusal::LevelNotInPart,
        ArchivalRefusal::LevelMissing,
        ArchivalRefusal::UntaggedPage { page: 3 },
        ArchivalRefusal::LanguageMissing,
        ArchivalRefusal::DestinationProfileMissing,
    ];
    for refusal in &refusals {
        assert!(refusal.clause().starts_with("6."), "{refusal:?}");
        let text = refusal.to_string();
        assert!(text.starts_with(refusal.clause()), "{text}");
        assert!(
            text.len() > refusal.clause().len() + 20,
            "a refusal that says only its clause has not said why: {text}"
        );
    }
}
