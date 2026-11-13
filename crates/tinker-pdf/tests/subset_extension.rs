//! FT-11.9's subset extension through the facade (`docs/design/subsetting.md`,
//! *Extension*): an embedded subset gains glyphs of the face it was cut from,
//! in place, at their own identifiers, under a new subset tag — what an
//! editor adding text over a subset font needs.
//!
//! The TrueType face is the vendored Liberation Serif Regular, whose glyphs,
//! composites and tables are the face's own and not this repository's; the
//! CFF face is a Type 1 program built byte by byte (`type1_support`)
//! converted by `tinker_pdf_font::type1_to_cff`, and the Type 1 face that
//! program itself, so every glyph either holds is known in advance. Each
//! extended program is read back from a saved document and held glyph by
//! glyph to the face.

mod render_support;
mod type1_support;
mod written_support;

use std::collections::BTreeSet;

use tinker_pdf::subset::{extend, ExtendError, Extension};
use tinker_pdf::write::{save, FontPolicy, SaveOptions};
use tinker_pdf::Document;
use tinker_pdf_cos::{subset_tag, CosDocument, Dict, ObjRef, Object, XrefEntry};
use tinker_pdf_font::{subset_cff, subset_type1, type1_to_cff, Cff, ExtendRefusal, Sfnt, Type1};

use type1_support::{n, rect, CLOSEPATH, ENDCHAR, HLINETO, HSBW, RMOVETO, SEAC, VLINETO};

// ---------------------------------------------------------------------------
// Faces.
// ---------------------------------------------------------------------------

/// A vendored Liberation face.
fn liberation(file: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf-font/data/liberation")
        .join(file);
    std::fs::read(&path).expect("the vendored Liberation face is readable")
}

fn serif() -> Vec<u8> {
    liberation("LiberationSerif-Regular.ttf")
}

/// The glyphs `text` draws in a `glyf` face, through its `cmap`.
fn glyphs_of(face: &[u8], text: &str) -> BTreeSet<u16> {
    tinker_pdf_font::glyphs_for(face, text)
}

/// `glyphs`, and every component a composite among them is built from,
/// followed through the face.
fn closed(face: &[u8], glyphs: &BTreeSet<u16>) -> BTreeSet<u16> {
    let sfnt = Sfnt::parse(face).expect("the face parses");
    let mut out = glyphs.clone();
    let mut pending: Vec<u16> = glyphs.iter().copied().collect();
    while let Some(glyph) = pending.pop() {
        for component in tinker_pdf_font::subset::components(&sfnt, glyph) {
            if out.insert(component) {
                pending.push(component);
            }
        }
    }
    out
}

/// A Type 1 face of `.notdef`, `A` to `E` and `Aacute`, its boxes `size`
/// apart, encoded at 65 to 69 and 201.
fn type1_face(size: i32) -> type1_support::Program {
    let glyphs = vec![
        (".notdef", [n(&[0, 250]), vec![HSBW, ENDCHAR]].concat()),
        ("A", rect(600, 50, 0, size, 500)),
        ("B", rect(600, 60, 0, size + 50, 450)),
        ("C", rect(600, 70, 0, size + 100, 400)),
        ("D", rect(600, 80, 0, size + 150, 350)),
        ("E", rect(600, 90, 0, size + 200, 300)),
        (
            "acute",
            [
                n(&[0, 300]),
                vec![HSBW],
                n(&[100, 750]),
                vec![RMOVETO],
                n(&[100]),
                vec![HLINETO],
                n(&[100]),
                vec![VLINETO],
                n(&[-100]),
                vec![HLINETO, CLOSEPATH, ENDCHAR],
            ]
            .concat(),
        ),
        (
            "Aacute",
            [
                n(&[0, 600]),
                vec![HSBW],
                n(&[0, 100, 0, 65, 194]),
                SEAC.to_vec(),
            ]
            .concat(),
        ),
    ];
    type1_support::program(
        &glyphs,
        &[],
        &[
            (65, "A"),
            (66, "B"),
            (67, "C"),
            (68, "D"),
            (69, "E"),
            (194, "acute"),
            (201, "Aacute"),
        ],
    )
}

// ---------------------------------------------------------------------------
// Documents.
// ---------------------------------------------------------------------------

/// How a page embeds its program: the font's `/Subtype` and `/BaseFont`, the
/// descriptor's font-file key and whatever else it carries, and the stream's
/// own entries.
struct Embedding<'a> {
    subtype: &'a str,
    base_font: &'a str,
    key: &'a str,
    descriptor: &'a str,
    entries: String,
}

/// A page drawing `text` in `F0`, a font embedding `program` as `embedding`
/// says; and, where `second` is true, a second font `F1` over the same
/// descriptor drawing the same text.
fn page(embedding: &Embedding<'_>, program: &[u8], text: &str, second: bool) -> Vec<u8> {
    let Embedding {
        subtype,
        base_font,
        key,
        descriptor,
        entries,
    } = embedding;
    let widths = vec!["600"; 256].join(" ");
    let font = |at: u32| {
        format!(
            "<< /Type /Font /Subtype /{subtype} /BaseFont /{base_font} /FirstChar 0 \
             /LastChar 255 /Widths [{widths}] /FontDescriptor {at} 0 R >>"
        )
        .into_bytes()
    };
    let content = if second {
        format!("BT /F0 30 Tf 10 60 Td ({text}) Tj ET BT /F1 30 Tf 10 20 Td ({text}) Tj ET")
    } else {
        format!("BT /F0 30 Tf 10 40 Td ({text}) Tj ET")
    };
    let fonts = if second {
        "/F0 5 0 R /F1 8 0 R"
    } else {
        "/F0 5 0 R"
    };
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] \
             /Resources << /Font << {fonts} >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        render_support::raw_stream("", content.as_bytes()),
        font(6),
        format!(
            "<< /Type /FontDescriptor /FontName /{base_font} /Flags 32 \
             /FontBBox [-100 -250 1200 950] /ItalicAngle 0 /Ascent 900 /Descent -250 \
             /CapHeight 700 /StemV 80 /{key} 7 0 R{descriptor} >>"
        )
        .into_bytes(),
        render_support::raw_stream(entries, program),
    ];
    if second {
        objects.push(font(6));
    }
    render_support::raw_pdf(&objects)
}

/// A TrueType embedding of `program`, under `/FontFile2`.
fn truetype(program: &[u8]) -> Embedding<'static> {
    Embedding {
        subtype: "TrueType",
        base_font: "ABCDEF+LiberationSerif",
        key: "FontFile2",
        descriptor: "",
        entries: format!("/Length1 {}", program.len()),
    }
}

/// A TrueType page over `program`.
fn truetype_page(program: &[u8], text: &str) -> Vec<u8> {
    page(&truetype(program), program, text, false)
}

/// A CFF page over `program`, `/FontFile3 /Subtype /Type1C` under `/Type1`.
fn cff_page(program: &[u8], text: &str) -> Vec<u8> {
    let embedding = Embedding {
        subtype: "Type1",
        base_font: "ABCDEF+Fixture",
        key: "FontFile3",
        descriptor: "",
        entries: "/Subtype /Type1C".to_owned(),
    };
    page(&embedding, program, text, false)
}

/// A Type 1 page over `cut`, `/FontFile` with its three lengths, its
/// descriptor carrying `descriptor` besides.
fn type1_page(cut: &tinker_pdf_font::Type1Subset, descriptor: &str, text: &str) -> Vec<u8> {
    let [one, two, three] = cut.lengths;
    let embedding = Embedding {
        subtype: "Type1",
        base_font: "ABCDEF+Fixture",
        key: "FontFile",
        descriptor,
        entries: format!("/Length1 {one} /Length2 {two} /Length3 {three}"),
    };
    page(&embedding, &cut.program, text, false)
}

/// The font dictionary `/F0` names: object 5 in every page here.
const F0: ObjRef = ObjRef::new(5, 0);

/// Extends the page's `/F0` and saves it whole, so a save keeps the glyphs
/// nothing draws yet.
fn extended(
    page: &[u8],
    face: &[u8],
    glyphs: &BTreeSet<u16>,
) -> (Result<Extension, ExtendError>, Vec<u8>) {
    let doc = Document::open(page.to_vec()).expect("the page opens");
    let mut editor = doc.editor();
    let result = extend(&mut editor, F0, face, glyphs);
    let saved = save(
        &mut editor,
        &SaveOptions {
            fonts: FontPolicy::Keep,
            ..SaveOptions::default()
        },
    );
    written_support::rewritten("an extended subset", page, &saved.bytes);
    (result, saved.bytes)
}

/// The one embedded program of a document, decoded, with its stream's and
/// its descriptor's dictionaries.
fn the_program(bytes: &[u8]) -> (Vec<u8>, Dict, Dict, CosDocument) {
    let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
    let mut found = Vec::new();
    for (number, entry) in doc.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let Ok(object) = doc.get(ObjRef::new(number, 0)) else {
            continue;
        };
        let Object::Dict(descriptor) = object.as_ref() else {
            continue;
        };
        for key in [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"] {
            if let Some(program) = descriptor.get_ref(doc.intern(key)) {
                let stream = doc.get(program).expect("the program");
                let dict = stream.as_dict().expect("a stream").clone();
                let decoded = doc.stream_decoded(program).expect("it decodes");
                found.push((decoded, dict, descriptor.clone()));
            }
        }
    }
    assert_eq!(found.len(), 1, "one program");
    let (decoded, dict, descriptor) = found.remove(0);
    (decoded, dict, descriptor, doc)
}

/// A dictionary's name entry, as bytes.
fn name_of(dict: &Dict, doc: &CosDocument, key: &[u8]) -> Option<Vec<u8>> {
    dict.get_name(doc.intern(key))
        .and_then(|name| doc.name_bytes(name))
        .map(|bytes| bytes.to_vec())
}

/// The `/BaseFont` of the font dictionary at `at`.
fn base_font(bytes: &[u8], at: ObjRef) -> Vec<u8> {
    let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
    let object = doc.get(at).expect("the font");
    let dict = object.as_dict().expect("a dictionary");
    name_of(dict, &doc, b"BaseFont").expect("a /BaseFont")
}

/// A glyph's `glyf` entry, through `loca` in the form `head` states.
fn glyph_bytes<'a>(sfnt: &Sfnt<'a>, glyph: u16) -> &'a [u8] {
    let table = |tag: &[u8; 4]| sfnt.table(u32::from_be_bytes(*tag)).expect("the table");
    let (head, loca, glyf) = (table(b"head"), table(b"loca"), table(b"glyf"));
    let long = i16::from_be_bytes([head[50], head[51]]) != 0;
    let at = usize::from(glyph);
    let (start, end) = if long {
        let offset = |i: usize| {
            u32::from_be_bytes([
                loca[i * 4],
                loca[i * 4 + 1],
                loca[i * 4 + 2],
                loca[i * 4 + 3],
            ]) as usize
        };
        (offset(at), offset(at + 1))
    } else {
        let offset = |i: usize| usize::from(u16::from_be_bytes([loca[i * 2], loca[i * 2 + 1]])) * 2;
        (offset(at), offset(at + 1))
    };
    &glyf[start..end]
}

/// The glyph indices whose `glyf` entry is not empty.
fn held(program: &[u8]) -> BTreeSet<u16> {
    let sfnt = Sfnt::parse(program).expect("an sfnt");
    (0..sfnt.glyph_count().expect("a glyph count"))
        .filter(|glyph| !glyph_bytes(&sfnt, *glyph).is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// The exit's tests.
// ---------------------------------------------------------------------------

/// **A TrueType subset gains exactly the new glyphs**: after the extension it
/// holds the glyphs it held, the ones asked for and the components of the
/// composite among them, each the face's glyph at its own index, and nothing
/// else; every one outlines as in the face.
#[test]
fn an_extended_subset_gains_exactly_the_new_glyphs() {
    let face = serif();
    let old = glyphs_of(&face, "Hel");
    let program = tinker_pdf_font::subset(&face, &old).expect("the face subsets");
    let new = glyphs_of(&face, "o\u{e9}");
    let (result, bytes) = extended(&truetype_page(&program, "Hel"), &face, &new);
    let extension = result.expect("the subset extends");

    let (after, dict, _, doc) = the_program(&bytes);
    assert_eq!(extension.after, after.len());
    assert_eq!(extension.before, program.len());
    assert!(extension.after > extension.before);
    assert_eq!(
        dict.get_int(doc.intern(b"Length1")),
        Some(after.len() as i64)
    );

    let mut expected = closed(&face, &old.union(&new).copied().collect());
    expected.insert(0);
    let blank: BTreeSet<u16> = {
        let sfnt = Sfnt::parse(&face).expect("the face");
        expected
            .iter()
            .copied()
            .filter(|glyph| glyph_bytes(&sfnt, *glyph).is_empty())
            .collect()
    };
    let drawn: BTreeSet<u16> = expected.difference(&blank).copied().collect();
    assert_eq!(held(&after), drawn, "the old, the new and the components");
    let (source, cut) = (
        Sfnt::parse(&face).expect("the face"),
        Sfnt::parse(&after).expect("the extension"),
    );
    for glyph in &expected {
        assert_eq!(
            tinker_pdf_font::glyf::outline(&cut, *glyph).map(|o| o.segments),
            tinker_pdf_font::glyf::outline(&source, *glyph).map(|o| o.segments),
            "glyph {glyph}"
        );
    }
}

/// The same for a CFF subset: the old glyphs and the new draw and advance as
/// in the face, every other glyph is empty.
#[test]
fn an_extended_cff_subset_gains_exactly_the_new_glyphs() {
    let converted = type1_to_cff(&type1_face(300).bytes).expect("the face converts");
    let face = converted.program;
    let gid = |name: &str| {
        converted
            .names
            .iter()
            .position(|n| n == name.as_bytes())
            .map(|at| at as u16)
            .expect("in the face")
    };
    let old: BTreeSet<u16> = [gid("A"), gid("B")].into_iter().collect();
    let program = subset_cff(&face, &old).expect("the face subsets");
    let new: BTreeSet<u16> = [gid("D")].into_iter().collect();
    let (result, bytes) = extended(&cff_page(&program, "AB"), &face, &new);
    result.expect("the subset extends");

    let (after, dict, _, doc) = the_program(&bytes);
    assert_eq!(name_of(&dict, &doc, b"Subtype"), Some(b"Type1C".to_vec()));
    let (source, cut) = (
        Cff::parse(&face).expect("the face"),
        Cff::parse(&after).expect("the extension"),
    );
    assert_eq!(cut.glyph_count(), source.glyph_count());
    for (at, name) in converted.names.iter().enumerate() {
        let glyph = at as u16;
        let name = String::from_utf8_lossy(name);
        let drawn = cut.outline(glyph).expect("it answers");
        if ["A", "B", "D"].contains(&name.as_ref()) {
            assert_eq!(
                drawn.segments,
                source.outline(glyph).expect("the face").segments,
                "{name}"
            );
            assert_eq!(cut.advance(glyph), source.advance(glyph), "{name}");
        } else {
            assert!(drawn.is_empty(), "{name} was not asked for");
        }
    }
}

/// A Type 1 subset gains the charstrings asked for: its stream states the new
/// program's three lengths, its descriptor's `/CharSet` lists the new names,
/// and every kept glyph draws as in the face.
#[test]
fn an_extended_type1_subset_gains_the_new_charstrings() {
    let face = type1_face(300);
    let source = Type1::parse(&face.bytes).expect("the face parses");
    let index = |name: &[u8]| source.glyph_for_name(name).expect("in the face");
    let cut =
        subset_type1(&face.bytes, &[index(b"A")].into_iter().collect()).expect("the face subsets");
    // A `/CharSet` listing the subset's glyphs, which the extension rewrites.
    let original = type1_page(&cut, " /CharSet (/A)", "A");
    let (result, bytes) = extended(&original, &face.bytes, &[index(b"C")].into_iter().collect());
    result.expect("the subset extends");

    let (after, dict, descriptor, doc) = the_program(&bytes);
    let length = |key: &[u8]| {
        usize::try_from(dict.get_int(doc.intern(key)).expect("stated")).expect("positive")
    };
    assert_eq!(
        length(b"Length1") + length(b"Length2") + length(b"Length3"),
        after.len()
    );
    assert_eq!(
        descriptor
            .get_string(doc.intern(b"CharSet"))
            .map(|s| s.bytes.clone()),
        Some(b"/A/C".to_vec())
    );
    let extension = Type1::parse(&after).expect("the extension parses");
    assert_eq!(extension.glyph_count(), 3, ".notdef, A and C");
    for name in [b".notdef".as_slice(), b"A", b"C"] {
        let (a, b) = (extension.glyph_for_name(name).expect("kept"), index(name));
        assert_eq!(
            extension.outline(a).map(|o| o.segments),
            source.outline(b).map(|o| o.segments)
        );
        assert_eq!(extension.advance(a), source.advance(b));
    }
}

/// **A face that is not the source is refused**, and the document is left as
/// it was: a TrueType subset of the serif face extended from the sans, a CFF
/// one from a CFF of other boxes, a Type 1 one from a Type 1 face of other
/// boxes.
#[test]
fn a_face_that_is_not_the_source_is_refused() {
    let face = serif();
    let program = tinker_pdf_font::subset(&face, &glyphs_of(&face, "Hel")).expect("it subsets");
    let original = truetype_page(&program, "Hel");
    let sans = liberation("LiberationSans-Regular.ttf");
    let (result, bytes) = extended(&original, &sans, &glyphs_of(&sans, "o"));
    assert_eq!(
        result,
        Err(ExtendError::Refused(ExtendRefusal::NotTheSource))
    );
    assert_eq!(the_program(&bytes).0, program, "the program is as it was");
    assert_eq!(base_font(&bytes, F0), b"ABCDEF+LiberationSerif");

    let converted = type1_to_cff(&type1_face(300).bytes).expect("it converts");
    let cut = subset_cff(&converted.program, &[1u16].into_iter().collect()).expect("it subsets");
    let other = type1_to_cff(&type1_face(320).bytes).expect("it converts");
    let (result, _) = extended(
        &cff_page(&cut, "A"),
        &other.program,
        &[2u16].into_iter().collect(),
    );
    assert_eq!(
        result,
        Err(ExtendError::Refused(ExtendRefusal::NotTheSource))
    );

    let face = type1_face(300);
    let cut = subset_type1(&face.bytes, &[1u16].into_iter().collect()).expect("it subsets");
    let original = type1_page(&cut, "", "A");
    let (result, _) = extended(
        &original,
        &type1_face(320).bytes,
        &[3u16].into_iter().collect(),
    );
    assert_eq!(
        result,
        Err(ExtendError::Refused(ExtendRefusal::NotTheSource))
    );
}

/// **The extended program is renamed**: a new subset tag, the one the
/// engine's subsets are named with, computed over the new program, on the
/// font's `/BaseFont` and its descriptor's `/FontName`.
#[test]
fn the_extended_program_is_renamed() {
    let face = serif();
    let program = tinker_pdf_font::subset(&face, &glyphs_of(&face, "Hel")).expect("it subsets");
    let (result, bytes) = extended(
        &truetype_page(&program, "Hel"),
        &face,
        &glyphs_of(&face, "o"),
    );
    let extension = result.expect("it extends");
    let (after, _, descriptor, doc) = the_program(&bytes);

    let mut expected = subset_tag(&after);
    expected.extend_from_slice(b"LiberationSerif");
    assert_ne!(expected, b"ABCDEF+LiberationSerif", "the tag changed");
    assert_eq!(base_font(&bytes, F0), expected);
    assert_eq!(
        name_of(&descriptor, &doc, b"FontName"),
        Some(expected.clone())
    );
    assert_eq!(extension.base_font.as_bytes(), expected.as_slice());
}

/// **The saved document passes the strict validator**, and draws what it
/// drew: the glyphs it drew are the ones it had.
#[test]
fn the_saved_document_passes_the_strict_validator() {
    let face = serif();
    let program = tinker_pdf_font::subset(&face, &glyphs_of(&face, "Hel")).expect("it subsets");
    let original = truetype_page(&program, "Hel");
    let (result, bytes) = extended(&original, &face, &glyphs_of(&face, "o\u{e9}"));
    result.expect("it extends");
    let doc = Document::open(bytes.clone()).expect("it opens");
    assert_eq!(doc.validate(), Vec::new());
    let draw = |bytes: &[u8]| {
        Document::open(bytes.to_vec())
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render(&tinker_pdf::RenderOptions::default())
            .data
    };
    assert_eq!(draw(&bytes), draw(&original));
}

// ---------------------------------------------------------------------------
// The rest of the call.
// ---------------------------------------------------------------------------

/// A default save cuts the program to what the pages draw: the glyphs added
/// and drawn by nothing yet leave again, which is why a caller adds their
/// text first.
#[test]
fn a_default_save_cuts_what_nothing_draws() {
    let face = serif();
    let old = glyphs_of(&face, "Hel");
    let program = tinker_pdf_font::subset(&face, &old).expect("it subsets");
    let original = truetype_page(&program, "Hel");
    let doc = Document::open(original.clone()).expect("it opens");
    let mut editor = doc.editor();
    extend(&mut editor, F0, &face, &glyphs_of(&face, "o")).expect("it extends");
    let saved = save(&mut editor, &SaveOptions::default());
    written_support::rewritten("a default save after an extension", &original, &saved.bytes);
    let (after, _, _, _) = the_program(&saved.bytes);
    let o = glyphs_of(&face, "o");
    assert!(held(&after).is_disjoint(&o), "nothing draws o");
}

/// Two font dictionaries over one program both draw with it, and both take
/// the new name.
#[test]
fn every_font_naming_the_program_is_renamed() {
    let face = serif();
    let program = tinker_pdf_font::subset(&face, &glyphs_of(&face, "Hel")).expect("it subsets");
    let original = page(&truetype(&program), &program, "Hel", true);
    let (result, bytes) = extended(&original, &face, &glyphs_of(&face, "o"));
    let extension = result.expect("it extends");
    for font in [F0, ObjRef::new(8, 0)] {
        assert_eq!(base_font(&bytes, font), extension.base_font.as_bytes());
    }
}

/// An object that is no font embedding a program, and a glyph past the
/// face's last, write nothing.
#[test]
fn what_cannot_be_extended_is_refused_and_writes_nothing() {
    let face = serif();
    let program = tinker_pdf_font::subset(&face, &glyphs_of(&face, "Hel")).expect("it subsets");
    let original = truetype_page(&program, "Hel");

    let doc = Document::open(original.clone()).expect("it opens");
    let mut editor = doc.editor();
    for not_a_font in [ObjRef::new(3, 0), ObjRef::new(40, 0)] {
        assert_eq!(
            extend(&mut editor, not_a_font, &face, &glyphs_of(&face, "o")),
            Err(ExtendError::NoProgram)
        );
    }

    let past: BTreeSet<u16> = [u16::MAX].into_iter().collect();
    let (result, bytes) = extended(&original, &face, &past);
    assert_eq!(
        result,
        Err(ExtendError::Refused(ExtendRefusal::NoSuchGlyph))
    );
    assert_eq!(the_program(&bytes).0, program);

    // A CFF face for a TrueType subset is no format the call extends.
    let converted = type1_to_cff(&type1_face(300).bytes).expect("it converts");
    let (result, _) = extended(&original, &converted.program, &BTreeSet::new());
    assert_eq!(result, Err(ExtendError::Refused(ExtendRefusal::Format)));
}
