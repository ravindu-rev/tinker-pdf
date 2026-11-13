//! FT-11's subsets on save, through the facade (`docs/design/subsetting.md`):
//! a Type 1 program cut down to the glyphs its pages draw. Each saved document
//! is re-read and drawn page by page against the original, held to the
//! strict validator, and its report read.
//!
//! Every program is built byte by byte in the test through
//! `tinker_pdf_font::type1`'s own cipher (`type1_support`), so which glyphs a
//! page draws, which subroutines they reach and what must leave the program
//! are all known in advance.

mod render_support;
mod type1_support;
mod written_support;

use tinker_pdf::subset::{SubsetReport, UntouchedReason};
use tinker_pdf::write::{save, SaveOptions, SubsetOutcome};
use tinker_pdf::{Document, RenderOptions};
use tinker_pdf_cos::{CosDocument, Dict, ObjRef, Object, XrefEntry};
use tinker_pdf_font::Type1;

use type1_support::{
    n, rect, CALLOTHERSUBR, CALLSUBR, CLOSEPATH, ENDCHAR, HLINETO, HSBW, POP, RETURN, RMOVETO,
    SEAC, VLINETO,
};

// ---------------------------------------------------------------------------
// The fixture.
// ---------------------------------------------------------------------------

/// The flex and hint-replacement subroutines chapter 8.4 fixes, and three
/// that draw parts of glyphs: 5 the left of `A`, 6 the bottom of `B`, and 7,
/// which only `D` calls.
fn subrs() -> Vec<(i64, Vec<u8>)> {
    vec![
        (
            0,
            [
                n(&[3, 0]),
                CALLOTHERSUBR.to_vec(),
                POP.to_vec(),
                POP.to_vec(),
                vec![12, 33, RETURN],
            ]
            .concat(),
        ),
        (
            1,
            [n(&[0, 1]), CALLOTHERSUBR.to_vec(), vec![RETURN]].concat(),
        ),
        (
            2,
            [n(&[0, 2]), CALLOTHERSUBR.to_vec(), vec![RETURN]].concat(),
        ),
        (3, vec![RETURN]),
        (4, [n(&[0, -10]), vec![5, RETURN]].concat()),
        (
            5,
            [
                n(&[300]),
                vec![HLINETO],
                n(&[700]),
                vec![VLINETO],
                n(&[-300]),
                vec![HLINETO],
                n(&[4]),
                vec![CALLSUBR, RETURN],
            ]
            .concat(),
        ),
        (
            6,
            [
                n(&[700]),
                vec![HLINETO],
                n(&[300]),
                vec![VLINETO],
                n(&[-700]),
                vec![HLINETO, RETURN],
            ]
            .concat(),
        ),
        (7, [n(&[400, 400]), vec![5, RETURN]].concat()),
    ]
}

/// A glyph `0 800 hsbw x y rmoveto <subr> callsubr closepath endchar`.
fn calling(x: i32, y: i32, subr: i32) -> Vec<u8> {
    [
        n(&[0, 800]),
        vec![HSBW],
        n(&[x, y]),
        vec![RMOVETO],
        n(&[subr]),
        vec![CALLSUBR, CLOSEPATH, ENDCHAR],
    ]
    .concat()
}

/// The fixture's glyphs, in program order: `.notdef`, `A`, `B`, `C`, `D`,
/// `E`, `F` (in no encoding), `acute` and `Aacute`.
fn glyphs() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (".notdef", [n(&[0, 800]), vec![HSBW, ENDCHAR]].concat()),
        ("A", calling(100, 0, 5)),
        ("B", calling(50, 50, 6)),
        ("C", rect(800, 200, 200, 400, 400)),
        ("D", calling(100, 100, 7)),
        ("E", rect(800, 0, 600, 800, 100)),
        ("F", rect(800, 300, 0, 100, 800)),
        ("acute", rect(300, 100, 750, 150, 150)),
        (
            "Aacute",
            // asb adx ady bchar achar seac: `A` is StandardEncoding 65 and
            // `acute` 194.
            [
                n(&[0, 800]),
                vec![HSBW],
                n(&[0, 200, 50, 65, 194]),
                SEAC.to_vec(),
            ]
            .concat(),
        ),
    ]
}

/// The built-in encoding: A to E at 65 to 69, `acute` at 194 and `Aacute`
/// at 201. `F` is in none.
const ENCODING: &[(u8, &str)] = &[
    (65, "A"),
    (66, "B"),
    (67, "C"),
    (68, "D"),
    (69, "E"),
    (194, "acute"),
    (201, "Aacute"),
];

fn fixture_program() -> type1_support::Program {
    type1_support::program(&glyphs(), &subrs(), ENCODING)
}

/// One font dictionary: its `/Encoding` entry and `/ToUnicode` reference, if
/// any, and the descriptor and program objects it names.
struct Font {
    encoding: &'static str,
    to_unicode: Option<u32>,
    descriptor: u32,
}

/// A document whose pages each show one string in `F0` — and in `F1`, where
/// a second font is given, a second string — over the Type 1 program `program`
/// with the descriptor's `charset` entry, if any.
fn document(
    program: &type1_support::Program,
    fonts: &[Font],
    pages: &[&[(&str, &str)]],
    charset: Option<&str>,
    to_unicode: Option<&str>,
) -> Vec<u8> {
    // 1 catalog, 2 pages, 3 descriptor, 4 program, 5 ToUnicode, then the
    // fonts, then each page and its content.
    let font_at = 6u32;
    let page_at = font_at + fonts.len() as u32;
    let widths = vec!["800"; 201 - 65 + 1].join(" ");
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            pages.len(),
            (0..pages.len())
                .map(|i| format!("{} 0 R", page_at + 2 * i as u32))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes(),
        format!(
            "<< /Type /FontDescriptor /FontName /Fixture /Flags 32 \
             /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 1000 /Descent 0 \
             /CapHeight 700 /StemV 80 /FontFile 4 0 R{} >>",
            charset
                .map(|names| format!(" /CharSet ({names})"))
                .unwrap_or_default()
        )
        .into_bytes(),
        type1_support::font_file(program),
        render_support::raw_stream(
            "",
            format!(
                "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
                 /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
                 1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
                 1 beginbfchar\n{}\nendbfchar\nendcmap\n\
                 CMapName currentdict /CMap defineresource pop\nend\nend\n",
                to_unicode.unwrap_or("<20> <0020>")
            )
            .as_bytes(),
        ),
    ];
    for font in fonts {
        objects.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Fixture /FirstChar 65 \
                 /LastChar 201 /Widths [{widths}] /FontDescriptor {} 0 R{}{} >>",
                font.descriptor,
                font.encoding,
                font.to_unicode
                    .map(|at| format!(" /ToUnicode {at} 0 R"))
                    .unwrap_or_default()
            )
            .into_bytes(),
        );
    }
    let names: Vec<String> = (0..fonts.len())
        .map(|i| format!("/F{i} {} 0 R", font_at + i as u32))
        .collect();
    for (i, shown) in pages.iter().enumerate() {
        let content: String = shown
            .iter()
            .enumerate()
            .map(|(line, (font, text))| {
                format!(
                    "BT /{font} 40 Tf 10 {} Td ({text}) Tj ET\n",
                    80 - 45 * line as i32
                )
            })
            .collect();
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 130] \
                 /Resources << /Font << {} >> >> /Contents {} 0 R >>",
                names.join(" "),
                page_at + 2 * i as u32 + 1
            )
            .into_bytes(),
        );
        objects.push(render_support::raw_stream("", content.as_bytes()));
    }
    render_support::raw_pdf(&objects)
}

/// One font over the fixture with no `/Encoding` and no `/ToUnicode`.
fn plain_font() -> Vec<Font> {
    vec![Font {
        encoding: "",
        to_unicode: None,
        descriptor: 3,
    }]
}

/// Two pages: `AB` on the first, and `C` and `Aacute` (code 201) on the
/// second, so `D`, `E`, `F` and subroutine 7 are nobody's.
fn two_pages() -> Vec<u8> {
    document(
        &fixture_program(),
        &plain_font(),
        &[&[("F0", "AB")], &[("F0", "C\\311")]],
        Some("/A/B/C/D/E/F/acute/Aacute"),
        None,
    )
}

// ---------------------------------------------------------------------------
// Reading what came out.
// ---------------------------------------------------------------------------

/// The document saved by the facade's default save: subset, rewritten.
fn saved(bytes: &[u8]) -> (Vec<u8>, SubsetReport) {
    let doc = Document::open(bytes.to_vec()).expect("the fixture opens");
    let mut editor = doc.editor();
    let saved = save(&mut editor, &SaveOptions::default());
    let report = match saved.fonts {
        SubsetOutcome::Cut(report) => report,
        other => panic!("a default save cuts: {other:?}"),
    };
    written_support::rewritten("a Type 1 subset on save", bytes, &saved.bytes);
    (saved.bytes, report)
}

/// Each page's pixels at 72 dpi.
fn pages_drawn(bytes: &[u8]) -> Vec<Vec<u8>> {
    let doc = Document::open(bytes.to_vec()).expect("it opens");
    (0..doc.page_count())
        .map(|i| {
            let bitmap = doc
                .page(i)
                .expect("a page")
                .render(&RenderOptions::default());
            assert!(
                bitmap.data.iter().any(|v| *v < 250),
                "page {i} draws: {:?}",
                bitmap.warnings
            );
            bitmap.data
        })
        .collect()
}

/// Every embedded program in the document: the stream's dictionary and its
/// decoded bytes, with the descriptor that names it.
fn programs(bytes: &[u8]) -> Vec<(Dict, Vec<u8>, Dict)> {
    let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
    let mut out = Vec::new();
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
        let Some(program) = descriptor.get_ref(doc.intern(b"FontFile")) else {
            continue;
        };
        let stream = doc.get(program).expect("the program");
        let dict = stream.as_dict().expect("a stream").clone();
        let decoded = doc.stream_decoded(program).expect("it decodes");
        out.push((dict, decoded, descriptor.clone()));
    }
    out
}

/// The one Type 1 program in a document, parsed.
fn only_program(bytes: &[u8]) -> (Dict, Vec<u8>, Dict, CosDocument) {
    let mut found = programs(bytes);
    assert_eq!(found.len(), 1, "one program");
    let (dict, decoded, descriptor) = found.remove(0);
    let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
    (dict, decoded, descriptor, doc)
}

/// The glyph names a program carries, in its order.
fn names_in(program: &[u8]) -> Vec<String> {
    let font = Type1::parse(program).expect("a Type 1 program");
    (0..font.glyph_count())
        .map(|i| String::from_utf8_lossy(font.glyph_name(i).expect("in range")).into_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// FT-11.4's exit.
// ---------------------------------------------------------------------------

/// **Each page draws as it did**, pixel for pixel, from a program that no
/// longer carries the glyphs nobody draws.
#[test]
fn a_type1_subset_draws_the_used_glyphs_identically() {
    let original = two_pages();
    let (bytes, _) = saved(&original);
    assert_eq!(pages_drawn(&bytes), pages_drawn(&original));

    let (_, program, _, _) = only_program(&bytes);
    assert_eq!(
        names_in(&program),
        [".notdef", "A", "B", "C", "acute", "Aacute"],
        "the drawn glyphs, `.notdef` and Aacute's components, in program order"
    );
}

/// **The saved document passes the strict validator**, and every profile it
/// claims, as a save of its input.
#[test]
fn the_type1_subset_passes_the_strict_validator() {
    let original = two_pages();
    let (bytes, _) = saved(&original);
    let doc = Document::open(bytes).expect("it opens");
    assert_eq!(doc.validate(), Vec::new());
}

/// **The bytes saved are reported**: the program's size before and after,
/// under its new subset tag.
#[test]
fn the_bytes_saved_are_reported() {
    let original = two_pages();
    let (_, before, _, _) = only_program(&original);
    let (bytes, report) = saved(&original);
    let (_, after, _, _) = only_program(&bytes);

    assert_eq!(report.untouched, Vec::new(), "nothing is left whole");
    assert_eq!(report.subsetted.len(), 1);
    let cut = &report.subsetted[0];
    assert_eq!(cut.before, before.len());
    assert_eq!(cut.after, after.len());
    assert!(cut.after < cut.before, "{} -> {}", cut.before, cut.after);
    assert_eq!(cut.glyphs, 4, "A, B, C and Aacute were shown");
    let (tag, face) = cut.base_font.split_at(7);
    assert_eq!(face, "Fixture");
    assert!(tag.ends_with('+') && tag[..6].bytes().all(|b| b.is_ascii_uppercase()));
    assert_eq!(
        report.bytes_before() - report.bytes_after(),
        before.len() - after.len()
    );
}

// ---------------------------------------------------------------------------
// What else a Type 1 subset writes.
// ---------------------------------------------------------------------------

/// ISO 32000-2 Table 125: `/Length1`, `/Length2` and `/Length3` are the
/// subset's clear text, encrypted portion and trailer, and add up to it.
#[test]
fn the_three_lengths_are_the_subsets_segments() {
    let (bytes, _) = saved(&two_pages());
    let (dict, program, _, doc) = only_program(&bytes);
    let length = |key: &[u8]| {
        usize::try_from(dict.get_int(doc.intern(key)).expect("stated")).expect("positive")
    };
    let (one, two, three) = (length(b"Length1"), length(b"Length2"), length(b"Length3"));
    assert_eq!(one + two + three, program.len());
    assert!(program[..one].ends_with(b"currentfile eexec\n"));
    assert!(program[one + two..].ends_with(b"cleartomark\n"));
}

/// Table 120: a descriptor's `/CharSet` lists the subset's glyphs, `.notdef`
/// left out, in program order; one with none gains none.
#[test]
fn a_charset_lists_the_subsets_glyphs() {
    let (bytes, _) = saved(&two_pages());
    let (_, _, descriptor, doc) = only_program(&bytes);
    assert_eq!(
        descriptor
            .get_string(doc.intern(b"CharSet"))
            .map(|s| s.bytes.clone()),
        Some(b"/A/B/C/acute/Aacute".to_vec())
    );

    let bare = document(
        &fixture_program(),
        &plain_font(),
        &[&[("F0", "AB")]],
        None,
        None,
    );
    let (bytes, _) = saved(&bare);
    let (_, _, descriptor, doc) = only_program(&bytes);
    assert!(!descriptor.contains_key(doc.intern(b"CharSet")));
}

/// A code the document names nothing for, drawn here by its text and by a
/// reader following 9.6.5.2 by the built-in encoding, keeps both glyphs: the
/// `/ToUnicode` says code 66 is the letter A, and the built-in encoding's 66
/// is B.
#[test]
fn the_built_in_encodings_glyph_is_kept_beside_the_texts() {
    let fonts = vec![Font {
        encoding: "",
        to_unicode: Some(5),
        descriptor: 3,
    }];
    let original = document(
        &fixture_program(),
        &fonts,
        &[&[("F0", "B")]],
        None,
        Some("<42> <0041>"),
    );
    let (bytes, report) = saved(&original);
    assert_eq!(report.subsetted.len(), 1, "{report:?}");
    assert_eq!(pages_drawn(&bytes), pages_drawn(&original));
    let (_, program, _, _) = only_program(&bytes);
    let names = names_in(&program);
    for name in ["A", "B"] {
        assert!(names.iter().any(|n| n == name), "{name} kept: {names:?}");
    }
}

/// A code only this renderer's guess reaches — `F` is in no encoding, and
/// the code's text names it — leaves the program whole, as a code mapped by
/// guess does for every format.
#[test]
fn a_type1_font_a_guess_reaches_is_left_whole() {
    let original = document(
        &fixture_program(),
        &plain_font(),
        &[&[("F0", "AF")]],
        None,
        None,
    );
    let (bytes, report) = saved(&original);
    assert_eq!(report.subsetted, Vec::new());
    assert_eq!(report.untouched.len(), 1);
    assert_eq!(report.untouched[0].reason, UntouchedReason::CodeNotMapped);
    let (_, before, _, _) = only_program(&original);
    let (_, after, _, _) = only_program(&bytes);
    assert_eq!(after, before, "written through as it arrived");
}

/// Two font dictionaries over one program are one job: the program keeps the
/// glyphs either draws.
#[test]
fn a_program_two_fonts_share_keeps_both_fonts_glyphs() {
    let fonts = vec![
        Font {
            encoding: "",
            to_unicode: None,
            descriptor: 3,
        },
        Font {
            encoding: " /Encoding << /Differences [65 /E] >>",
            to_unicode: None,
            descriptor: 3,
        },
    ];
    let original = document(
        &fixture_program(),
        &fonts,
        &[&[("F0", "B"), ("F1", "A")]],
        None,
        None,
    );
    let (bytes, report) = saved(&original);
    assert_eq!(report.subsetted.len(), 1, "one job: {report:?}");
    assert_eq!(pages_drawn(&bytes), pages_drawn(&original));
    let (_, program, _, _) = only_program(&bytes);
    assert_eq!(names_in(&program), [".notdef", "B", "E"]);
}

/// A program the subsetter refuses by name is written through as it
/// arrived: here a drawn glyph calls a subroutine the program does not
/// define.
#[test]
fn a_type1_program_the_subsetter_refuses_is_left_whole() {
    let mut glyphs = glyphs();
    glyphs[1].1 = calling(100, 0, 50);
    let program = type1_support::program(&glyphs, &subrs(), ENCODING);
    let original = document(&program, &plain_font(), &[&[("F0", "AB")]], None, None);
    let (bytes, report) = saved(&original);
    assert_eq!(report.subsetted, Vec::new());
    assert_eq!(report.untouched.len(), 1);
    assert_eq!(
        report.untouched[0].reason,
        UntouchedReason::ProgramNotRebuildable
    );
    let (_, before, _, _) = only_program(&original);
    let (_, after, _, _) = only_program(&bytes);
    assert_eq!(after, before);
}
