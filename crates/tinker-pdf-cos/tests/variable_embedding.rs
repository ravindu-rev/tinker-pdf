//! A variable face embedded at an instance (`docs/design/variable-fonts.md`,
//! 'Who asks for an instance').
//!
//! The two faces `tinker-pdf-font` commits under `tests/variable/` are
//! registered through `add_embedded_font_at` and `add_cid_font_at`, and
//! through the two registrations that name no instance, and every document is
//! read back: its `/FontFile2` or `/FontFile3`, each glyph it drew held to the
//! outline the font crate's static instance draws, and `/Widths` or `/W` to
//! that instance's advances. The font crate's `variations.rs` holds the
//! instance to the specification's arithmetic; this file holds the document
//! to the instance.

mod written_support;

use std::collections::BTreeMap;

use tinker_pdf_cos::{CosDocument, Dict, DocumentBuilder, Glyph, Object, PlacedGlyph};
use tinker_pdf_font::instance::{instantiate, DROPPED};
use tinker_pdf_font::variation::{DesignSpace, Instance, UserCoordinate};
use tinker_pdf_font::{glyf, Cff, Cff2, Outline, Sfnt};

const GLYF_FACE: &[u8] = include_bytes!("../../tinker-pdf-font/tests/variable/variable-glyf.ttf");
const CFF2_FACE: &[u8] = include_bytes!("../../tinker-pdf-font/tests/variable/variable-cff2.otf");

/// Both committed faces, by file name.
const FACES: [(&str, &[u8]); 2] = [
    ("variable-glyf.ttf", GLYF_FACE),
    ("variable-cff2.otf", CFF2_FACE),
];

/// The characters both faces map: the space, the full stop, the colon, `H`,
/// `I` and `O`.
const TEXT: &str = " .:HIO";

/// 0x43464620 `CFF `.
const TAG_CFF: u32 = 0x4346_4620;
/// 0x43464632 `CFF2`.
const TAG_CFF2: u32 = 0x4346_4632;
/// 0x70726570 `prep`.
const TAG_PREP: u32 = 0x7072_6570;

fn coordinate(tag: &[u8; 4], value: f64) -> UserCoordinate {
    UserCoordinate::new(*tag, value)
}

fn space_of(face: &[u8]) -> DesignSpace {
    DesignSpace::read(&Sfnt::parse(face).expect("the face parses")).expect("its design space")
}

/// Every named instance of a face, its default, and user coordinates no
/// named instance gives — over every axis either face has, so a tag the
/// CFF2 face has no axis for is passed too and ignored.
fn instances(face: &[u8]) -> Vec<Instance> {
    let mut out: Vec<Instance> = (0..space_of(face).instances().len())
        .map(|i| Instance::Named(u16::try_from(i).expect("few")))
        .collect();
    out.push(Instance::default());
    out.push(Instance::User(vec![
        coordinate(b"wght", 250.0),
        coordinate(b"opsz", 30.0),
    ]));
    out.push(Instance::User(vec![
        coordinate(b"wght", 813.5),
        coordinate(b"wdth", 90.0),
        coordinate(b"slnt", -7.25),
    ]));
    out
}

/// How a font is registered: a simple font drawn by `PageBuilder::text`, or
/// a composite one drawn by `PageBuilder::glyphs`.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Simple,
    Composite,
}

const KINDS: [Kind; 2] = [Kind::Simple, Kind::Composite];

/// A one-page document with the font `register` registers under `F0`: a
/// simple font drawing [`TEXT`], or a composite one drawing every glyph the
/// program has, each standing for the character that maps to it. `reference`
/// is the program the font is expected to embed, whose `cmap` and glyph
/// count decide what is drawn. The document comes back read and held to the
/// validator, with the glyphs it drew.
fn build(
    label: &str,
    kind: Kind,
    subset: bool,
    reference: &[u8],
    register: impl FnOnce(&mut DocumentBuilder) -> bool,
) -> (CosDocument, Vec<u16>) {
    let sfnt = Sfnt::parse(reference).expect("the reference parses");
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(subset);
    assert!(register(&mut builder), "{label}: the font registers");
    let drawn: Vec<u16> = match kind {
        Kind::Simple => TEXT
            .chars()
            .map(|c| sfnt.glyph_for_char(c).expect("the face maps it"))
            .collect(),
        Kind::Composite => (0..sfnt.glyph_count().expect("a glyph count")).collect(),
    };
    let texts: Vec<String> = drawn
        .iter()
        .map(|&glyph| {
            TEXT.chars()
                .find(|&c| sfnt.glyph_for_char(c) == Some(glyph))
                .map(String::from)
                .unwrap_or_default()
        })
        .collect();
    builder.add_page(300.0, 100.0, |page| match kind {
        Kind::Simple => page.text(b"F0", 24.0, 20.0, 40.0, TEXT),
        Kind::Composite => {
            let glyphs: Vec<Glyph<'_>> = drawn
                .iter()
                .zip(&texts)
                .map(|(&id, text)| Glyph {
                    id,
                    text: text.as_str(),
                })
                .collect();
            assert!(
                page.glyphs(b"F0", 24.0, 20.0, 40.0, &glyphs),
                "{label}: the glyphs draw"
            );
        }
    });
    let bytes = builder.finish();
    (written_support::authored(label, &bytes), drawn)
}

/// The font dictionary the first page names `resource`.
fn page_font(doc: &CosDocument, resource: &[u8]) -> Dict {
    let pages = tinker_pdf_cos::pages::collect(doc);
    let page = pages.first().expect("a page");
    let resources = page.resources.as_ref().expect("the page has resources");
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    let fonts = fonts.as_dict().expect("a /Font sub-dictionary");
    doc.resolve_key(fonts, doc.intern(resource))
        .as_dict()
        .cloned()
        .expect("the font resource")
}

/// The descendant of a Type0 font, or the font itself when it is simple.
fn descendant(doc: &CosDocument, font: &Dict) -> Dict {
    let descendants = doc.resolve_key(font, doc.intern(b"DescendantFonts"));
    match descendants.as_array().and_then(<[Object]>::first) {
        Some(first) => doc
            .resolve(first)
            .as_dict()
            .cloned()
            .expect("the descendant font"),
        None => font.clone(),
    }
}

fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    doc.resolve_key(dict, doc.intern(key))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|bytes| bytes.to_vec())
}

/// The program embedded for `resource`, the descriptor key it hangs off, and
/// the stream's own `/Subtype` where it has one.
fn embedded(doc: &CosDocument, resource: &[u8]) -> (Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
    let font = page_font(doc, resource);
    let descendant = descendant(doc, &font);
    let descriptor = doc.resolve_key(&descendant, doc.intern(b"FontDescriptor"));
    let descriptor = descriptor.as_dict().expect("a font descriptor");
    for key in [b"FontFile2".as_slice(), b"FontFile3"] {
        let Some(reference) = descriptor.get_ref(doc.intern(key)) else {
            continue;
        };
        let bytes = doc.stream_decoded(reference).expect("the program decodes");
        let stream = doc.resolve(&Object::Ref(reference));
        let subtype = stream
            .as_dict()
            .and_then(|dict| name_of(doc, dict, b"Subtype"));
        return (bytes, key.to_vec(), subtype);
    }
    panic!("no font program was embedded");
}

/// What a program draws for one glyph: its `CFF ` table's outline where it
/// has one, its `glyf` outline otherwise.
fn outline(program: &[u8], glyph: u16) -> Outline {
    let sfnt = Sfnt::parse(program).expect("an sfnt");
    match sfnt.table(TAG_CFF) {
        Some(cff) => Cff::parse(cff)
            .expect("its CFF parses")
            .outline(glyph)
            .expect("an outline"),
        None => glyf::outline(&sfnt, glyph).expect("an outline"),
    }
}

/// A glyph's advance in thousandths of the em, as a width array states it.
fn width(program: &[u8], glyph: u16) -> f64 {
    let sfnt = Sfnt::parse(program).expect("an sfnt");
    let advance = sfnt.advance(glyph).expect("an advance");
    (f64::from(advance) * 1000.0 / f64::from(sfnt.units_per_em.max(1))).round()
}

/// A simple font's `/Widths`, by code from its `/FirstChar`.
fn simple_widths(doc: &CosDocument, font: &Dict) -> BTreeMap<u8, f64> {
    let first = doc
        .resolve_key(font, doc.intern(b"FirstChar"))
        .as_int()
        .expect("a /FirstChar");
    let widths = doc.resolve_key(font, doc.intern(b"Widths"));
    let widths = widths.as_array().expect("a /Widths array");
    widths
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let code = u8::try_from(first + i64::try_from(i).expect("few")).expect("a code");
            (code, doc.resolve(value).as_number().expect("a width"))
        })
        .collect()
}

/// A CIDFont's `/W`, by code, in either of 9.7.4.3's forms.
fn cid_widths(doc: &CosDocument, cid: &Dict) -> BTreeMap<u16, f64> {
    let w = doc.resolve_key(cid, doc.intern(b"W"));
    let items = w.as_array().expect("a /W array");
    let mut out = BTreeMap::new();
    let mut at = 0;
    while at < items.len() {
        let first = doc.resolve(&items[at]).as_int().expect("a first code");
        let next = doc.resolve(&items[at + 1]);
        match next.as_array() {
            Some(widths) => {
                for (i, value) in widths.iter().enumerate() {
                    let code = first + i64::try_from(i).expect("few");
                    out.insert(
                        u16::try_from(code).expect("a code"),
                        doc.resolve(value).as_number().expect("a width"),
                    );
                }
                at += 2;
            }
            None => {
                let last = next.as_int().expect("a last code");
                let value = doc.resolve(&items[at + 2]).as_number().expect("a width");
                for code in first..=last {
                    out.insert(u16::try_from(code).expect("a code"), value);
                }
                at += 3;
            }
        }
    }
    out
}

/// Holds the widths `F0` states to `reference`'s advances: `/Widths` at each
/// character of [`TEXT`] for a simple font, `/W` at every glyph drawn, keyed
/// by the glyph's index, for a composite one.
#[track_caller]
fn widths_agree(label: &str, doc: &CosDocument, kind: Kind, reference: &[u8], drawn: &[u16]) {
    let font = page_font(doc, b"F0");
    match kind {
        Kind::Simple => {
            let widths = simple_widths(doc, &font);
            for (c, &glyph) in TEXT.chars().zip(drawn) {
                let code = u8::try_from(u32::from(c)).expect("ASCII");
                assert_eq!(
                    widths.get(&code).copied(),
                    Some(width(reference, glyph)),
                    "{label}: the width of {c:?}"
                );
            }
        }
        Kind::Composite => {
            let widths = cid_widths(doc, &descendant(doc, &font));
            assert_eq!(
                widths.keys().copied().collect::<Vec<u16>>(),
                drawn,
                "{label}: each glyph's code is its index"
            );
            for &glyph in drawn {
                assert_eq!(
                    widths.get(&glyph).copied(),
                    Some(width(reference, glyph)),
                    "{label}: the width of glyph {glyph}"
                );
            }
        }
    }
}

/// Holds every glyph drawn to the outline `reference` draws for it.
#[track_caller]
fn outlines_agree(label: &str, program: &[u8], reference: &[u8], drawn: &[u16]) {
    for &glyph in drawn {
        assert_eq!(
            outline(program, glyph).segments,
            outline(reference, glyph).segments,
            "{label}: glyph {glyph}'s outline"
        );
    }
}

/// `add_embedded_font_at` or `add_cid_font_at`, as `kind` says, under `F0`.
fn register_at(
    builder: &mut DocumentBuilder,
    kind: Kind,
    face: &[u8],
    instance: &Instance,
) -> bool {
    match kind {
        Kind::Simple => builder.add_embedded_font_at(b"F0", b"Variable", face, instance),
        Kind::Composite => builder.add_cid_font_at(b"F0", b"Variable", face, instance),
    }
}

/// `add_embedded_font` or `add_cid_font`, as `kind` says, under `F0`.
fn register(builder: &mut DocumentBuilder, kind: Kind, face: &[u8]) -> bool {
    match kind {
        Kind::Simple => builder.add_embedded_font(b"F0", b"Variable", face),
        Kind::Composite => builder.add_cid_font(b"F0", b"Variable", face),
    }
}

#[test]
fn an_instanced_embedding_re_reads_to_the_same_outlines() {
    for (name, face) in FACES {
        for instance in instances(face) {
            let expected = instantiate(face, &instance).expect("the font crate's instance");
            let cff = Sfnt::parse(&expected)
                .expect("the instance parses")
                .table(TAG_CFF)
                .is_some();
            for kind in KINDS {
                for subset in [true, false] {
                    let label = format!("{name} at {instance:?}, {kind:?}, subsetting {subset}");
                    let (doc, drawn) = build(&label, kind, subset, &expected, |builder| {
                        register_at(builder, kind, face, &instance)
                    });
                    let (program, key, subtype) = embedded(&doc, b"F0");
                    if cff {
                        assert_eq!(key, b"FontFile3", "{label}: CFF outlines (9.9)");
                        assert_eq!(subtype.as_deref(), Some(&b"OpenType"[..]), "{label}");
                    } else {
                        assert_eq!(key, b"FontFile2", "{label}: glyf outlines (9.9)");
                    }
                    if !subset {
                        assert!(
                            program == expected,
                            "{label}: the instance is embedded whole"
                        );
                    }
                    outlines_agree(&label, &program, &expected, &drawn);
                    widths_agree(&label, &doc, kind, &expected, &drawn);
                }
            }
        }
    }
}

#[test]
fn the_written_program_carries_no_variation_table() {
    let bold = Instance::User(vec![coordinate(b"wght", 700.0)]);
    for (name, face) in FACES {
        let carried = Sfnt::parse(face).expect("the face parses").tags();
        assert!(
            carried.contains(b"fvar") && carried.contains(b"HVAR"),
            "{name}: the face varies, so the test can fail"
        );
        for instance in [Instance::Named(1), bold.clone()] {
            let expected = instantiate(face, &instance).expect("the font crate's instance");
            for kind in KINDS {
                for subset in [true, false] {
                    let label = format!("{name} at {instance:?}, {kind:?}, subsetting {subset}");
                    let (doc, _) = build(&label, kind, subset, &expected, |builder| {
                        register_at(builder, kind, face, &instance)
                    });
                    let (program, _, _) = embedded(&doc, b"F0");
                    let tags = Sfnt::parse(&program).expect("an sfnt").tags();
                    for tag in DROPPED.iter().chain([b"CFF2"]) {
                        assert!(
                            !tags.contains(tag),
                            "{label}: {} is not embedded",
                            String::from_utf8_lossy(tag)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_cff2_face_is_embedded_not_refused() {
    let sfnt = Sfnt::parse(CFF2_FACE).expect("the face parses");
    let cff2 = Cff2::parse(sfnt.table(TAG_CFF2).expect("a CFF2 table")).expect("CFF2 parses");
    let default = instantiate(CFF2_FACE, &Instance::default()).expect("the default instance");
    for kind in KINDS {
        for subset in [true, false] {
            let label = format!("the CFF2 face, {kind:?}, subsetting {subset}");
            let (doc, drawn) = build(&label, kind, subset, &default, |builder| {
                register(builder, kind, CFF2_FACE)
            });
            let (program, key, subtype) = embedded(&doc, b"F0");
            assert_eq!(key, b"FontFile3", "{label}");
            assert_eq!(subtype.as_deref(), Some(&b"OpenType"[..]), "{label}");
            let tags = Sfnt::parse(&program).expect("an sfnt").tags();
            assert!(tags.contains(b"CFF "), "{label}: a CFF table");
            assert!(!tags.contains(b"CFF2"), "{label}: no CFF2 table");
            if !subset {
                assert!(program == default, "{label}: the default instance, whole");
            }
            // The default instance draws what the face draws at its default,
            // and measures what its `hmtx` says.
            for &glyph in &drawn {
                assert_eq!(
                    outline(&program, glyph).segments,
                    cff2.outline(glyph).expect("the default outline").segments,
                    "{label}: glyph {glyph}"
                );
            }
            widths_agree(&label, &doc, kind, CFF2_FACE, &drawn);
        }
    }
}

#[test]
fn a_glyf_face_given_no_instance_takes_todays_path() {
    let face = Sfnt::parse(GLYF_FACE).expect("the face parses");
    let instance_prep =
        Sfnt::parse(&instantiate(GLYF_FACE, &Instance::default()).expect("the default instance"))
            .expect("the instance parses")
            .table(TAG_PREP)
            .expect("a prep")
            .to_vec();
    assert_ne!(
        face.table(TAG_PREP),
        Some(instance_prep.as_slice()),
        "an instance appends its GETVARIATION IDEF, so the test can fail"
    );
    for kind in KINDS {
        let label = format!("the glyf face whole, {kind:?}");
        let (doc, drawn) = build(&label, kind, false, GLYF_FACE, |builder| {
            register(builder, kind, GLYF_FACE)
        });
        let (program, key, _) = embedded(&doc, b"F0");
        assert_eq!(key, b"FontFile2", "{label}");
        assert!(
            program == GLYF_FACE,
            "{label}: the face itself, its variations with it"
        );
        widths_agree(&label, &doc, kind, GLYF_FACE, &drawn);

        let label = format!("the glyf face subset, {kind:?}");
        let (doc, drawn) = build(&label, kind, true, GLYF_FACE, |builder| {
            register(builder, kind, GLYF_FACE)
        });
        let (program, key, _) = embedded(&doc, b"F0");
        assert_eq!(key, b"FontFile2", "{label}");
        assert_eq!(
            Sfnt::parse(&program).expect("an sfnt").table(TAG_PREP),
            face.table(TAG_PREP),
            "{label}: the face's own prep"
        );
        outlines_agree(&label, &program, GLYF_FACE, &drawn);
        widths_agree(&label, &doc, kind, GLYF_FACE, &drawn);
    }
}

#[test]
fn an_instance_the_face_cannot_give_registers_nothing() {
    let past = |face: &[u8]| {
        Instance::Named(u16::try_from(space_of(face).instances().len()).expect("few"))
    };
    let truncated = &GLYF_FACE[..GLYF_FACE.len() / 2];
    let mut builder = DocumentBuilder::new();
    assert!(
        !builder.add_embedded_font_at(b"F0", b"Variable", GLYF_FACE, &past(GLYF_FACE)),
        "a named instance past the glyf face's"
    );
    assert!(
        !builder.add_cid_font_at(b"F1", b"Variable", GLYF_FACE, &past(GLYF_FACE)),
        "the same, composite"
    );
    assert!(
        !builder.add_embedded_font_at(b"F2", b"Variable", CFF2_FACE, &past(CFF2_FACE)),
        "a named instance past the CFF2 face's"
    );
    assert!(
        !builder.add_cid_font_at(b"F3", b"Variable", CFF2_FACE, &past(CFF2_FACE)),
        "the same, composite"
    );
    assert!(
        !builder.add_embedded_font_at(b"F4", b"Variable", truncated, &Instance::Named(0)),
        "a face cut short"
    );
    assert!(
        !builder.add_cid_font_at(b"F5", b"Variable", b"no font at all", &Instance::default()),
        "bytes that are no font"
    );
    let mut run = Vec::new();
    assert!(
        !builder.glyph_run(
            &mut run,
            b"F1",
            12.0,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            &[PlacedGlyph {
                glyph: Glyph { id: 1, text: "" },
                x: 0.0,
                rise: 0.0,
            }],
        ),
        "no composite font is registered under F1"
    );
    builder.add_page(100.0, 100.0, |_| {});
    let doc = written_support::authored("nothing registered", &builder.finish());
    let pages = tinker_pdf_cos::pages::collect(&doc);
    let page = pages.first().expect("a page");
    if let Some(resources) = page.resources.as_ref() {
        let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
        if let Some(fonts) = fonts.as_dict() {
            for resource in [b"F0", b"F1", b"F2", b"F3", b"F4", b"F5"] {
                assert!(
                    fonts.get(doc.intern(resource)).is_none(),
                    "{} names no font",
                    String::from_utf8_lossy(resource)
                );
            }
        }
    }
}

#[test]
fn two_instances_of_one_face_are_two_programs() {
    let light = Instance::Named(0);
    let heavy = Instance::User(vec![coordinate(b"wght", 900.0)]);
    for (name, face) in FACES {
        let expected_light = instantiate(face, &light).expect("the light instance");
        let expected_heavy = instantiate(face, &heavy).expect("the heavy instance");
        assert_ne!(expected_light, expected_heavy, "{name}: two instances");
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_embedded_font_at(b"F0", b"Variable-Light", face, &light));
        assert!(builder.add_embedded_font_at(b"F1", b"Variable-Heavy", face, &heavy));
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 24.0, 20.0, 60.0, TEXT);
            page.text(b"F1", 24.0, 20.0, 20.0, TEXT);
        });
        let doc = written_support::authored(name, &builder.finish());
        let (light_program, _, _) = embedded(&doc, b"F0");
        let (heavy_program, _, _) = embedded(&doc, b"F1");
        assert!(light_program != heavy_program, "{name}: two programs");
        let sfnt = Sfnt::parse(&expected_light).expect("the instance parses");
        let drawn: Vec<u16> = TEXT
            .chars()
            .map(|c| sfnt.glyph_for_char(c).expect("mapped"))
            .collect();
        outlines_agree(name, &light_program, &expected_light, &drawn);
        outlines_agree(name, &heavy_program, &expected_heavy, &drawn);
    }
}

#[test]
fn a_bare_cff_and_a_static_face_are_their_own_instances() {
    let static_face = instantiate(GLYF_FACE, &Instance::Named(2)).expect("a static face");
    let cff_face = instantiate(CFF2_FACE, &Instance::default()).expect("a CFF face");
    let bare = Sfnt::parse(&cff_face)
        .expect("the CFF face parses")
        .table(TAG_CFF)
        .expect("its CFF")
        .to_vec();
    assert!(
        Sfnt::parse(&bare).is_none() && Cff::parse(&bare).is_some(),
        "a bare CFF program"
    );
    let heavy = Instance::User(vec![coordinate(b"wght", 900.0)]);
    // A static face under either kind; a CID-keyed bare CFF under the
    // composite one, which is where 9.7.4.2 reads its charset. Each draws `H`
    // and `O` by the glyph indices the face it came from maps them to.
    let programs = [
        ("a static face", &static_face, &static_face, Kind::Simple),
        ("a static face", &static_face, &static_face, Kind::Composite),
        ("a bare CFF", &bare, &cff_face, Kind::Composite),
    ];
    for (what, program, mapped_by, kind) in programs {
        let sfnt = Sfnt::parse(mapped_by).expect("the mapping face parses");
        let h = sfnt.glyph_for_char('H').expect("H");
        let o = sfnt.glyph_for_char('O').expect("O");
        let draw = |builder: DocumentBuilder| -> Vec<u8> {
            let mut builder = builder;
            builder.add_page(300.0, 100.0, |page| match kind {
                Kind::Simple => page.text(b"F0", 24.0, 20.0, 40.0, "HIO"),
                Kind::Composite => {
                    assert!(page.glyphs(
                        b"F0",
                        24.0,
                        20.0,
                        40.0,
                        &[Glyph { id: h, text: "H" }, Glyph { id: o, text: "O" }],
                    ));
                }
            });
            builder.finish()
        };
        let mut plain = DocumentBuilder::new();
        plain.set_subset_fonts(false);
        assert!(register(&mut plain, kind, program), "{what}, {kind:?}");
        let plain = draw(plain);
        written_support::authored(what, &plain);
        for instance in [Instance::default(), heavy.clone()] {
            let label = format!("{what} at {instance:?}, {kind:?}");
            let mut builder = DocumentBuilder::new();
            builder.set_subset_fonts(false);
            assert!(
                register_at(&mut builder, kind, program, &instance),
                "{label}"
            );
            let bytes = draw(builder);
            let doc = written_support::authored(&label, &bytes);
            let (embedded_program, _, _) = embedded(&doc, b"F0");
            assert!(embedded_program == *program, "{label}: embedded as it is");
            assert!(
                bytes == plain,
                "{label}: the document registering no instance writes"
            );
        }
        let mut builder = DocumentBuilder::new();
        assert!(
            !register_at(&mut builder, kind, program, &Instance::Named(0)),
            "{what}, {kind:?}: it has no named instance"
        );
    }
}

#[test]
fn a_glyph_run_advances_by_the_instances_widths() {
    let heavy = Instance::User(vec![coordinate(b"wght", 900.0)]);
    for (name, face) in FACES {
        let expected = instantiate(face, &heavy).expect("the heavy instance");
        let sfnt = Sfnt::parse(&expected).expect("the instance parses");
        let original = Sfnt::parse(face).expect("the face parses");
        let glyphs: Vec<u16> = "HIO"
            .chars()
            .map(|c| sfnt.glyph_for_char(c).expect("mapped"))
            .collect();
        assert!(
            glyphs
                .iter()
                .any(|&glyph| sfnt.advance(glyph) != original.advance(glyph)),
            "{name}: the instance moves an advance, so the test can fail"
        );
        let size = 20.0;
        // Each glyph where the instance's `/W` width puts it after the last.
        let mut x = 0.0;
        let placed: Vec<PlacedGlyph<'_>> = glyphs
            .iter()
            .zip(["H", "I", "O"])
            .map(|(&id, text)| {
                let at = x;
                x += width(&expected, id) / 1000.0 * size;
                PlacedGlyph {
                    glyph: Glyph { id, text },
                    x: at,
                    rise: 0.0,
                }
            })
            .collect();
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font_at(b"F0", b"Variable-Heavy", face, &heavy));
        let mut run = Vec::new();
        assert!(builder.glyph_run(
            &mut run,
            b"F0",
            size,
            [1.0, 0.0, 0.0, 1.0, 20.0, 40.0],
            &placed
        ));
        let text = String::from_utf8(run.clone()).expect("ASCII");
        let array = text
            .split('[')
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("a TJ array");
        let codes: String = glyphs
            .iter()
            .map(|glyph| format!("<{glyph:04X}>"))
            .collect();
        assert_eq!(
            array, codes,
            "{name}: no adjustment, the pen moving by the instance's advances"
        );
        builder.add_page(300.0, 100.0, |page| page.raw(&run));
        written_support::authored(name, &builder.finish());
    }
}

#[test]
fn an_instanced_document_is_the_same_bytes_every_time() {
    let instance = Instance::User(vec![coordinate(b"wght", 640.0), coordinate(b"wdth", 80.0)]);
    for (name, face) in FACES {
        let sfnt = Sfnt::parse(face).expect("the face parses");
        let h = sfnt.glyph_for_char('H').expect("H");
        let i = sfnt.glyph_for_char('I').expect("I");
        for kind in KINDS {
            let write = || {
                let mut builder = DocumentBuilder::new();
                assert!(register_at(&mut builder, kind, face, &instance));
                builder.add_page(300.0, 100.0, |page| match kind {
                    Kind::Simple => page.text(b"F0", 24.0, 20.0, 40.0, TEXT),
                    Kind::Composite => {
                        assert!(page.glyphs(
                            b"F0",
                            24.0,
                            20.0,
                            40.0,
                            &[Glyph { id: h, text: "H" }, Glyph { id: i, text: "I" }],
                        ));
                    }
                });
                builder.finish()
            };
            let first = write();
            written_support::authored(name, &first);
            assert!(first == write(), "{name}, {kind:?}: the same bytes twice");
        }
    }
}
