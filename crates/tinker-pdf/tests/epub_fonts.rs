//! A book's own faces reach its pages, one character at a time (gap 31,
//! milestone 9).
//!
//! Milestone 8 set every book in Times, Helvetica or Courier and said so. This
//! is where `@font-face` becomes a font program in the synthesised document and
//! `css-fonts-4` §5.3's **per-character** matching becomes something a content
//! stream can be asked about.
//!
//! # The headline is a count, and it is a count on purpose
//!
//! A PDF string is bytes in **one** font. So a run whose characters need three
//! faces is three `BT … ET` text objects at three origins, each starting where
//! the one before it left off — and
//! [`a_run_needing_three_faces_becomes_three_text_objects`] asserts the number
//! of objects rather than what the page looks like, because *a single face with
//! two notdefs also looks like something*. The control beside it,
//! [`one_face_that_covers_the_whole_run_is_one_text_object`], is the same
//! paragraph through a face that covers all of it; a build that emitted one
//! object per character would pass the first test and fail that one, and a
//! build that resolved the family list once per run would fail the first and
//! pass the second.
//!
//! # Every fixture face is synthesised, and it states its own coverage
//!
//! `tests/epub_support/typeface.rs` builds them, and records why they are
//! synthesised rather than read from the system: the test is then identical on
//! every platform and the repository carries no font anybody has to licence.
//! What is new here is that a fixture face has a **`cmap`**, so it can be asked
//! which characters it has — and [`the_three_fixture_faces_cover_a_third_of_the_run_each`]
//! asserts that premise out loud, because a three-face test whose faces overlap
//! is a test of something else and the overlap is invisible in the assertion it
//! would break.

mod epub_support;

use std::sync::{Arc, Mutex};

use epub_support::typeface::{covering, origin_of, text_objects, Face};
use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::epub::obfuscation::{adobe_key, deobfuscate, idpf_key, KeyDefect};
use tinker_pdf::epub::ocf::{Ocf, ADOBE_OBFUSCATION, IDPF_OBFUSCATION};
use tinker_pdf::epub::typeface::{load, FaceDefect};
use tinker_pdf::epub::Limits;
use tinker_pdf::{ArchiveWarning, Document, FontProvider, FontRequest, OpenOptions, RenderOptions};
use tinker_pdf_zip::Archive;

// ---- fixtures ---------------------------------------------------------------

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

/// The identifier every fixture book publishes, unless it says otherwise.
///
/// A UUID, because Adobe's obfuscation has no key for anything else and a
/// fixture that used an ISBN would make half of §4.4 untestable.
const IDENTIFIER: &str = "urn:uuid:1f0c2c1e-0000-4000-8000-00000000000a";

/// SHA-1 of [`IDENTIFIER`], written down rather than computed.
///
/// **Every fixture below obfuscates its font with a key and then asks this
/// build to undo it, and a wrong key cancels out exactly.** The injection
/// matrix found that: swapping the nibbles of Adobe's key survived every
/// assertion in this file, because the fixture and the reader derived it the
/// same wrong way. So the two keys are stated here and the two functions are
/// checked against them, which is the only form of the assertion a shared
/// derivation cannot satisfy.
///
/// From `sha1sum` over the forty-five bytes of the identifier above:
/// `0abddf6a7de5ce6839752120081560e5add45ade`.
const IDPF_KEY: [u8; 20] = [
    0x0A, 0xBD, 0xDF, 0x6A, 0x7D, 0xE5, 0xCE, 0x68, 0x39, 0x75, 0x21, 0x20, 0x08, 0x15, 0x60, 0xE5,
    0xAD, 0xD4, 0x5A, 0xDE,
];

/// The sixteen bytes of [`IDENTIFIER`]'s UUID, in the order they are written.
///
/// Read straight off the identifier: `1f0c2c1e-0000-4000-8000-00000000000a`
/// with the hyphens dropped is thirty-two hexadecimal digits, and each pair is
/// one byte, high nibble first.
const ADOBE_KEY: [u8; 16] = [
    0x1F, 0x0C, 0x2C, 0x1E, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A,
];

/// A package document naming one content document, one identifier, and
/// whatever extra manifest items a fixture needs.
fn package(identifier: &str, extra_items: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">{}</dc:identifier>"#,
            r#"<dc:title>A Book With Faces</dc:title>"#,
            r#"<dc:language>en</dc:language>"#,
            r#"<dc:creator>The tinker-pdf authors</dc:creator>"#,
            r#"</metadata>"#,
            r#"<manifest>"#,
            r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"{}"#,
            r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
        ),
        identifier, extra_items
    )
}

/// A content document whose one paragraph is `body`, set by `style`.
fn chapter(style: &str, body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
            r#"<style>{}</style></head><body><p>{}</p></body></html>"#
        ),
        style, body
    )
}

/// A whole container: the reserved files, the package, the chapter, and
/// whatever resources a fixture puts beside them.
fn book(package: &str, chapter: &str, resources: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    for (name, bytes) in resources {
        entries.push(OcfEntry::deflated(name, bytes));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// One `@font-face` rule.
fn font_face(family: &str, src: &str) -> String {
    format!("@font-face {{ font-family: \"{family}\"; src: {src}; }}")
}

/// The first page's content stream, decoded, through this repository's own
/// reader.
fn page_content(doc: &Document) -> String {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// Every `FontFace` warning a book's report carries.
fn face_warnings(doc: &Document) -> Vec<(String, FaceDefect, usize)> {
    doc.archive()
        .expect("a book carries a report")
        .warnings()
        .iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::FontFace {
                family,
                defect,
                rules,
            } => Some((family.clone(), defect.clone(), *rules)),
            _ => None,
        })
        .collect()
}

// ---- the fixture faces themselves --------------------------------------------

/// **A fixture face covers what it says it covers, and reads back through the
/// engine's own `Sfnt`.**
///
/// The premise of every test below, asserted rather than assumed. Four claims,
/// because a face that got any one of them wrong would make a later assertion
/// pass for the wrong reason:
///
/// - a covered character has a **non-zero** glyph, in the order the builder
///   promises, so a test can name the glyph it expects;
/// - an uncovered character has **none**, which is a `cmap` saying *no* rather
///   than a `cmap` that was never consulted;
/// - the advance is the one the fixture stated, out of `hmtx`;
/// - and the ascent and descent are the ones the fixture stated, out of
///   `hhea` — at the offsets the specification puts them at, which is where
///   this milestone found a bug.
#[test]
fn a_fixture_face_covers_what_it_says_and_reads_back_through_sfnt() {
    let face = Face::new("Alpha", "ACE").with_advance(640);
    let program = face.build();
    let sfnt = tinker_pdf_font::Sfnt::parse(&program).expect("a fixture face is an sfnt");

    assert_eq!(sfnt.units_per_em, 1000);
    for (ch, expected) in [('A', 1u16), ('C', 2), ('E', 3)] {
        assert_eq!(
            sfnt.glyph_for_char(ch),
            Some(expected),
            "{ch:?} is covered and is glyph {expected}"
        );
        assert_eq!(face.glyph_of(ch), Some(expected), "the builder agrees");
        assert_eq!(sfnt.advance(expected), Some(640));
    }
    for ch in ['B', 'D', 'Z', ' ', '\u{65e5}'] {
        assert!(
            matches!(sfnt.glyph_for_char(ch), None | Some(0)),
            "{ch:?} is not covered and must not resolve to a glyph"
        );
    }
}

/// **The three faces cover a third of the run each, and no character twice.**
///
/// The premise [`a_run_needing_three_faces_becomes_three_text_objects`] rests
/// on. Two faces that overlapped would make the object count depend on which
/// family `choose` happened to reach first, and nothing in the count assertion
/// would say so.
#[test]
fn the_three_fixture_faces_cover_a_third_of_the_run_each() {
    let alpha = Face::new("Alpha", "ABC");
    let beta = Face::new("Beta", "DEF");
    let gamma = Face::new("Gamma", "GHI");
    for ch in THREE_FACE_TEXT.chars() {
        let covering: Vec<&str> = [("Alpha", &alpha), ("Beta", &beta), ("Gamma", &gamma)]
            .iter()
            .filter(|(_, face)| face.glyph_of(ch).is_some())
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            covering.len(),
            1,
            "{ch:?} is covered by {covering:?} and must be covered by exactly one"
        );
    }
}

// ---- the headline: three faces, three text objects ---------------------------

/// The one run every three-face fixture sets: three characters per face, and no
/// space, because a space no fixture face covers would be a fourth object drawn
/// in Times and would make the count say something else.
const THREE_FACE_TEXT: &str = "ABCDEFGHI";

/// A book whose one paragraph needs `families` in order, with `faces` embedded.
fn three_face_book(faces: &[(&str, Face)], families: &str, text: &str) -> Vec<u8> {
    let mut style = String::new();
    let mut resources: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut items = String::new();
    for (path, face) in faces {
        style.push_str(&font_face(
            &face.family,
            &format!("url({path}) format(\"truetype\")"),
        ));
        resources.push((*path, face.build()));
        items.push_str(&format!(
            r#"<item id="f{}" href="{path}" media-type="font/ttf"/>"#,
            resources.len()
        ));
    }
    style.push_str(&format!("p {{ font-family: {families}; }}"));
    // The container paths are the chapter's own directory, which is where a
    // relative `url()` in a `<style>` element resolves to.
    let named: Vec<(String, Vec<u8>)> = resources
        .iter()
        .map(|(path, bytes)| (format!("EPUB/{path}"), bytes.clone()))
        .collect();
    let borrowed: Vec<(&str, Vec<u8>)> = named
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.clone()))
        .collect();
    book(
        &package(IDENTIFIER, &items),
        &chapter(&style, text),
        &borrowed,
    )
}

/// **A run needing three faces becomes three PDF text objects**, naming three
/// different font resources, at origins that continue from one another.
///
/// Row 9's own criterion, and the difference between `css-fonts-4` §5.3's
/// per-character matching and one face with holes in it. Four claims, and each
/// of them fails on its own:
///
/// 1. **Three objects.** A build that resolved `font-family` once per run
///    writes one, with six notdefs in it.
/// 2. **Three different resources**, in the declaration order the faces were
///    loaded in — so the segments are not three copies of the same font.
/// 3. **The glyphs are the `cmap`'s**, and each face numbers its own from 1: a
///    build that passed the character through as a code would write `0041` for
///    `A` rather than `0001`.
/// 4. **The origins continue**, each by exactly the advance the previous face's
///    own `hmtx` states, and all three share a baseline. The three faces are
///    given three different advances precisely so this can fail: a build that
///    measured every character with the first face's metrics puts objects two
///    and three in the wrong place and draws a perfectly plausible page.
#[test]
fn a_run_needing_three_faces_becomes_three_text_objects() {
    let bytes = three_face_book(
        &[
            (
                "fonts/alpha.ttf",
                Face::new("Alpha", "ABC").with_advance(500),
            ),
            ("fonts/beta.ttf", Face::new("Beta", "DEF").with_advance(750)),
            (
                "fonts/gamma.ttf",
                Face::new("Gamma", "GHI").with_advance(250),
            ),
        ],
        r#""Alpha", "Beta", "Gamma""#,
        THREE_FACE_TEXT,
    );
    let doc = Document::open(bytes).expect("a book");
    assert_eq!(
        face_warnings(&doc),
        Vec::new(),
        "every face was supposed to load"
    );

    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(
        objects.len(),
        3,
        "a run needing three faces is three text objects, not {}: {content}",
        objects.len()
    );

    let resources: Vec<&str> = objects.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        resources,
        ["Bf0", "Bf1", "Bf2"],
        "the three segments name the three faces in declaration order: {content}"
    );

    // Each face numbers its own glyphs from 1, in the sorted order of what it
    // covers, so `ABC`, `DEF` and `GHI` are `0001 0002 0003` three times over.
    for (_, object) in &objects {
        assert!(
            object.contains("<000100020003>"),
            "a segment does not draw its face's own first three glyphs: {object}"
        );
    }

    // `Bf0 12 Tf …`: the resource, then the size the whole object is set at.
    let size: f64 = objects[0]
        .1
        .split_whitespace()
        .nth(1)
        .expect("a size before Tf")
        .parse()
        .expect("a numeric size");
    let origins: Vec<(f64, f64)> = objects.iter().map(|(_, o)| origin_of(o)).collect();
    // Three characters at half an em, then three at three-quarters: the second
    // object starts 1.5 ems along and the third a further 2.25.
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(origins[1].0 - origins[0].0, 3.0 * 0.500 * size),
        "the second object does not start where Alpha's own advances left off: {origins:?}"
    );
    assert!(
        close(origins[2].0 - origins[1].0, 3.0 * 0.750 * size),
        "the third object does not start where Beta's own advances left off: {origins:?}"
    );
    assert!(
        close(origins[0].1, origins[1].1) && close(origins[1].1, origins[2].1),
        "the three objects are one line and must share a baseline: {origins:?}"
    );

    // And the text still extracts as one word, which is what says the three
    // objects are a **run** and not three runs.
    assert!(
        doc.page(0)
            .expect("a page")
            .text()
            .plain_text()
            .contains(THREE_FACE_TEXT),
        "the three segments do not extract as the paragraph"
    );
}

/// **The same paragraph through one face that covers all of it is one text
/// object.**
///
/// The control, and it is what makes the count above a measurement rather than
/// a coincidence: a build that started a new text object per character passes
/// [`a_run_needing_three_faces_becomes_three_text_objects`] and fails this.
#[test]
fn one_face_that_covers_the_whole_run_is_one_text_object() {
    let bytes = three_face_book(
        &[(
            "fonts/whole.ttf",
            Face::new("Whole", THREE_FACE_TEXT).with_advance(500),
        )],
        r#""Whole""#,
        THREE_FACE_TEXT,
    );
    let doc = Document::open(bytes).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(
        objects.len(),
        1,
        "one face covering the whole run is one text object: {content}"
    );
    assert_eq!(objects[0].0, "Bf0");
    assert!(
        objects[0]
            .1
            .contains("<000100020003000400050006000700080009>"),
        "one object draws all nine glyphs: {}",
        objects[0].1
    );
}

/// **A face that covers none of the run draws none of it**, and the family list
/// moves past it rather than stopping.
///
/// §5's algorithm walks the list *in the author's order* and a family whose
/// faces exist but cover nothing is not a match **for this character**. A build
/// that treated "the family exists" as the answer would draw the whole
/// paragraph in `Empty` and produce nine notdefs — which is one text object,
/// exactly like the control above, and is why the resource is asserted beside
/// the count.
#[test]
fn a_declared_family_that_covers_nothing_is_stepped_over() {
    let bytes = three_face_book(
        &[
            ("fonts/empty.ttf", Face::new("Empty", "xyz")),
            (
                "fonts/whole.ttf",
                Face::new("Whole", THREE_FACE_TEXT).with_advance(500),
            ),
        ],
        r#""Empty", "Whole""#,
        THREE_FACE_TEXT,
    );
    let doc = Document::open(bytes).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(objects.len(), 1, "{content}");
    assert_eq!(
        objects[0].0, "Bf1",
        "the run was drawn in the family that covers nothing: {content}"
    );
}

/// **An embedded face's baseline is placed by its own `hhea`, at the offsets
/// `hhea` puts them.**
///
/// `hhea`'s `ascender` is at byte 4 and its `descender` at byte 6; bytes 0 to 3
/// are the table's **version**, which is `0x00010000` in every font there has
/// ever been. A build that read the two fields from the front of the table gets
/// an ascent of **one unit** and a descent of zero for every face alike, and it
/// looks like a working build: the paragraph still has its text and every line
/// is simply set a little high.
///
/// What is measured is the **baseline**, not the line height, and the
/// difference is CSS 2.1 §10.8: `line-height` is `normal` here and so is a
/// multiple of the font size whichever face is used, and what the face decides
/// is where the baseline sits inside that box — `ascent + (line-height −
/// ascent − descent) / 2`, which is the half-leading rule. Subtracting two of
/// those cancels the `normal` constant, so the assertion below is arithmetic
/// this test states rather than a number copied out of a run.
///
/// **Three faces, and two independent differences.** One face alone cannot
/// separate the file's numbers from a plausible constant; two faces with both
/// fields changed cannot separate the ascent from the descent, and a build that
/// read the ascent correctly and the descent from the wrong place would pass
/// that. So the second face changes only the ascender and the third only the
/// descender.
#[test]
fn an_embedded_faces_baseline_is_placed_by_its_own_hhea() {
    let baseline = |ascender: i16, descender: i16| {
        let bytes = three_face_book(
            &[(
                "fonts/one.ttf",
                Face::new("One", "ABCDE")
                    .with_advance(500)
                    .with_vertical(ascender, descender),
            )],
            r#""One""#,
            "ABCDE",
        );
        let doc = Document::open(bytes).expect("a book");
        let content = page_content(&doc);
        let objects = text_objects(&content);
        assert_eq!(objects.len(), 1, "one face, one object: {content}");
        // `Bf0 12 Tf …`: the size the object is set at, in points.
        let size: f64 = objects[0]
            .1
            .split_whitespace()
            .nth(1)
            .expect("a size before Tf")
            .parse()
            .expect("a numeric size");
        (origin_of(&objects[0].1).1, size)
    };

    let (plain, size) = baseline(800, -200);
    let (taller, _) = baseline(1600, -200);
    let (deeper, _) = baseline(800, -600);

    // Half-leading: half of what the ascender grew by moves the baseline down
    // the page, which is a **smaller** PDF `y`.
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(plain - taller, size * 0.800 / 2.0),
        "the ascender did not move the baseline by half of what it grew by: \
         {plain} against {taller}"
    );
    // And half of what the descender grew by moves it back up, which is the
    // half the ascent alone cannot say anything about.
    assert!(
        close(plain - deeper, -size * 0.400 / 2.0),
        "the descender did not move the baseline: {plain} against {deeper}"
    );
    // Neither difference is zero, which is what a build reading the version
    // field produces: one ascent and one descent for every face there is.
    assert!(
        !close(plain, taller) && !close(plain, deeper),
        "three faces with three different `hhea` tables set one baseline"
    );
}

/// **A face the book never names is still tried, once the author's list is
/// exhausted.**
///
/// `css-fonts-4` §5.3's system fallback, for a reading system whose "system" is
/// the book: after the `font-family` list runs out a browser looks through the
/// faces it has, and the faces this build has are the standard 14 and whatever
/// the book embedded. A book that ships a CJK face under a family its `body`
/// rule never mentions is the case — real producers put the face in a
/// stylesheet that a chapter's own rule then forgets to reference — and without
/// this step its Japanese is a row of notdefs.
///
/// The paragraph asks for `serif` and nothing else, so nothing in the author's
/// list can reach `Cjk`. Two claims: the character is drawn in the embedded
/// face, and it is drawn as **that face's own glyph** rather than as a code in
/// an overflow font.
#[test]
fn a_face_the_family_list_never_mentions_is_still_the_system_fallback() {
    let bytes = three_face_book(
        &[(
            "fonts/cjk.ttf",
            Face::new("Cjk", "\u{65e5}\u{672c}").with_advance(1000),
        )],
        "serif",
        "\u{65e5}\u{672c}",
    );
    let doc = Document::open(bytes).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(objects.len(), 1, "{content}");
    assert_eq!(
        objects[0].0, "Bf0",
        "the book's own face was never tried: {content}"
    );
    assert!(
        objects[0].1.contains("<00010002>"),
        "the two kanji are not the face's own glyphs: {}",
        objects[0].1
    );
    // And nothing is reported missing, because nothing is.
    assert!(
        !doc.archive()
            .expect("a report")
            .warnings()
            .iter()
            .any(|w| matches!(w, ArchiveWarning::UncoveredCharacters { .. })),
        "a character the book's own face covers was reported as a notdef"
    );
}

/// **An unrecognised `format()` keyword does not refuse the file.**
///
/// §4.3 makes `format()` advisory, and the other half of *"a `format("woff2")`
/// is refused without reading the entry"* is that a sheet which wrote a vendor
/// keyword over a perfectly ordinary OpenType still gets its font. A build that
/// refused every hint it did not know would lose those books, and the two
/// halves are one `match` arm apart — which is why they are two tests.
#[test]
fn an_unrecognised_format_hint_does_not_refuse_a_perfectly_good_file() {
    let style = format!(
        "{}p {{ font-family: \"vend\"; }}",
        font_face("vend", r#"url(fonts/a.ttf) format("supertype-2")"#)
    );
    let doc = Document::open(book(
        &package(
            IDENTIFIER,
            r#"<item id="f1" href="fonts/a.ttf" media-type="font/ttf"/>"#,
        ),
        &chapter(&style, "ABC"),
        &[("EPUB/fonts/a.ttf", covering("vend", "ABC"))],
    ))
    .expect("a book");

    assert_eq!(
        face_warnings(&doc),
        Vec::new(),
        "a hint this build has never heard of refused a font the bytes accept"
    );
    let content = page_content(&doc);
    assert_eq!(
        text_objects(&content)
            .first()
            .map(|(name, _)| name.clone())
            .unwrap_or_default(),
        "Bf0",
        "the face is not on the page: {content}"
    );
}

/// **A face declared in every chapter is loaded once, and every rule that asked
/// for it is counted.**
///
/// Two consequences, and a test for one of them is not a test — which is how
/// this milestone found that the build had only one of them.
///
/// A book's chapters share one stylesheet, and there are two ways to share it.
/// A `<link>`ed sheet has an **address of its own**, so thirteen chapters
/// produce thirteen *equal* `@font-face` rules and `synthesise` collapses them
/// before anything is opened. A `<style>` element has no address, so its rules
/// carry the **content document's** — a different string per chapter — and no
/// comparison of rules can ever collapse those. The build claimed to
/// deduplicate and did not, for every book that puts its `@font-face` in a
/// `<style>` block, which is what the second half asserts here.
///
/// - **The face is loaded once**, against the resolved container path, so a
///   thirteen-chapter book with a two-megabyte face inflates and parses it once
///   rather than thirteen times.
/// - **The rules are still counted.** Two chapters that both asked for a WOFF2
///   is two rules, and `rules` says two: collapsing the report as well would
///   tell a producer to change one file when there are two.
#[test]
fn a_face_declared_in_every_chapter_is_loaded_once_and_every_rule_is_counted() {
    let style = format!(
        "{}p {{ font-family: \"shared\", serif; }}",
        font_face("shared", r#"url(fonts/a.woff2) format("woff2")"#)
    );
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">{}</dc:identifier>"#,
            r#"<dc:title>A Book Of Two Chapters</dc:title>"#,
            r#"<dc:language>en</dc:language>"#,
            r#"<dc:creator>The tinker-pdf authors</dc:creator></metadata>"#,
            r#"<manifest>"#,
            r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="f1" href="fonts/a.woff2" media-type="font/woff2"/>"#,
            r#"</manifest>"#,
            r#"<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#
        ),
        IDENTIFIER
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter(&style, "One.").as_bytes()),
        OcfEntry::deflated("EPUB/ch2.xhtml", chapter(&style, "Two.").as_bytes()),
        OcfEntry::deflated("EPUB/fonts/a.woff2", b"wOF2 and nothing this build reads"),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    let doc = Document::open(ocf_zip(&entries, &directory)).expect("a book");
    assert_eq!(doc.page_count(), 2, "both chapters are in the spine");

    let warnings = face_warnings(&doc);
    assert_eq!(
        warnings,
        vec![
            ("shared".to_owned(), FaceDefect::NoUsableSource, 2),
            (
                "shared".to_owned(),
                FaceDefect::UnsupportedFormat("woff2".to_owned()),
                2
            ),
        ],
        "two chapters asked and the report does not say two"
    );

    // And the other half, where the rule succeeds: two rules for one file are
    // one face. Asserted through `typeface::load` because a face set is not on
    // the report — the document is the same either way, and what a duplicate
    // costs is the inflation and the parse.
    let program = covering("shared", "ABC");
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/fonts/a.ttf", &program),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    let bytes = ocf_zip(&entries, &directory);
    let limits = Limits::DEFAULT;
    let archive =
        Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT).expect("a fixture container");
    let mut ocf = Ocf::open(archive, &limits);

    // The same rule as two chapters' `<style>` elements produce it: one family,
    // one url, two bases.
    let rule = |base: &str| tinker_pdf_css::font_face::FontFace {
        family: "shared".to_owned(),
        sources: vec![tinker_pdf_css::font_face::FontSource::Url {
            url: "fonts/a.ttf".to_owned(),
            format: None,
        }],
        weight: (400, 400),
        style: tinker_pdf_css::property::FontStyle::Normal,
        base: Some(base.to_owned()),
    };
    let rules = vec![rule("EPUB/ch1.xhtml"), rule("EPUB/ch2.xhtml")];
    assert_ne!(
        rules[0], rules[1],
        "the two rules are not equal, and cannot be"
    );

    let set = load(
        &mut ocf,
        &rules,
        Some(IDENTIFIER),
        &tinker_pdf::epub::ocf::Encryption::default(),
        &limits,
    );
    assert_eq!(
        set.faces().len(),
        1,
        "two rules resolving to one container entry loaded it twice"
    );
    assert_eq!(set.faces()[0].resource, b"Bf0".to_vec());
    assert!(set.defects().is_empty(), "{:?}", set.defects());
}

// ---- a character no face covers ------------------------------------------------

/// **A character no available face covers produces a named warning rather than
/// a blank.**
///
/// Row 9's criterion, and milestone 8's own `Still owed`. The character is on
/// the page as a notdef and in `Page::text()` as itself, which is a *different*
/// fact from a character that got no code at all — so the two are two warnings
/// and this asserts both halves: the count is right, and the text is still
/// there.
#[test]
fn a_character_no_face_covers_is_named_rather_than_left_blank() {
    // Four kanji and a Latin word, and one of the kanji **twice**: the Latin is
    // in `WinAnsiEncoding` and the kanji are in no face this build has. The
    // repeat is what makes the number a count of blank places on the page
    // rather than of distinct characters — which is what a host asking *"how
    // much of this book is missing"* wants, and the two differ by exactly one
    // here.
    let bytes = three_face_book(&[], "serif", "ok\u{65e5}\u{672c}\u{8a9e}\u{65e5}");
    let doc = Document::open(bytes).expect("a book");
    let warnings = doc.archive().expect("a report").warnings().to_vec();
    assert!(
        warnings.contains(&ArchiveWarning::UncoveredCharacters { characters: 4 }),
        "four blank places, three distinct characters, and the count is of \
         places: {warnings:?}"
    );
    // And nothing was **lost**: an unrepresented character is the other
    // warning, and this book has none.
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, ArchiveWarning::UnrepresentedCharacters { .. })),
        "a character that got a code was reported as one that did not: {warnings:?}"
    );
    assert!(
        doc.page(0)
            .expect("a page")
            .text()
            .plain_text()
            .contains("\u{65e5}\u{672c}\u{8a9e}"),
        "an uncovered character is missing from the picture and present in the text"
    );
}

/// **A book that brings its own face for those characters has neither the
/// warning nor the 224-code ceiling.**
///
/// Milestone 8's census listed *"a notdef glyph unwarned and more than 224
/// out-of-encoding characters per face lost"* as this milestone's to fix, and
/// this is the second half: an embedded face draws through a **composite**
/// font, where the two-byte code is the glyph identifier, so `/Differences`'s
/// 224 codes are not in the question at all. Three hundred distinct characters,
/// which is past the ceiling by seventy-six.
#[test]
fn an_embedded_face_removes_the_overflow_ceiling_and_the_warning() {
    let text: String = (0..300u32)
        .filter_map(|at| char::from_u32(0x4E00 + at))
        .collect();

    // The control: the same text with no face for it at all.
    let bare = Document::open(three_face_book(&[], "serif", &text)).expect("a book");
    let warnings = bare.archive().expect("a report").warnings().to_vec();
    assert!(
        warnings.contains(&ArchiveWarning::UnrepresentedCharacters { characters: 76 }),
        "the control does not lose the excess past 224: {warnings:?}"
    );
    assert!(
        warnings.contains(&ArchiveWarning::UncoveredCharacters { characters: 224 }),
        "the control does not draw 224 notdefs: {warnings:?}"
    );

    // And the same book with a face that covers every one of them.
    let bytes = three_face_book(
        &[("fonts/cjk.ttf", Face::new("Cjk", &text).with_advance(1000))],
        r#""Cjk", serif"#,
        &text,
    );
    let doc = Document::open(bytes).expect("a book");
    let warnings = doc.archive().expect("a report").warnings().to_vec();
    assert!(
        !warnings.iter().any(|w| matches!(
            w,
            ArchiveWarning::UnrepresentedCharacters { .. }
                | ArchiveWarning::UncoveredCharacters { .. }
        )),
        "a book that brought its own face still reports missing characters: {warnings:?}"
    );
    // And **every one** of the three hundred extracts, which is the assertion
    // the ceiling used to make impossible: the composite font's `/ToUnicode`
    // maps each glyph back to the character it was drawn for. The line breaks
    // `Page::text()` puts between lines are removed, because where the lines
    // fell is a different claim.
    let extracted = doc
        .page(0)
        .expect("a page")
        .text()
        .plain_text()
        .replace('\n', "");
    assert!(
        extracted.contains(&text),
        "a book set in its own face lost {} of its characters",
        text.chars().count() - extracted.chars().count()
    );
}

// ---- WOFF and WOFF2, refused by name -------------------------------------------

/// **WOFF and WOFF2 are refused by name, by the hint and by the bytes, and the
/// two are different findings.**
///
/// Four cases, and no two of them are the same code path:
///
/// - a `format("woff")` and a `format("woff2")` are refused **without reading
///   the entry**, which is what §4.3 says a hint is for;
/// - and a file whose sheet gave no hint at all is refused on its **signature**,
///   which is the commoner book: a producer that omits `format()` is more
///   common than one that lies in it.
///
/// A build with only the hint check passes on the first two and sets the last
/// two in Times in silence; a build with only the sniff downloads and inflates
/// a file it already knew it could not use, and names the wrong defect.
#[test]
fn woff_and_woff2_are_refused_by_name_on_the_hint_and_on_the_bytes() {
    let sfnt = covering("Real", "ABC");
    let mut woff = sfnt.clone();
    woff[0..4].copy_from_slice(b"wOFF");
    let mut woff2 = sfnt.clone();
    woff2[0..4].copy_from_slice(b"wOF2");

    let cases: [(&str, &str, Vec<u8>, FaceDefect); 4] = [
        (
            "HintedWoff",
            r#"url(fonts/a.woff) format("woff")"#,
            sfnt.clone(),
            FaceDefect::UnsupportedFormat("woff".to_owned()),
        ),
        (
            "HintedWoff2",
            r#"url(fonts/a.woff) format("woff2")"#,
            sfnt.clone(),
            FaceDefect::UnsupportedFormat("woff2".to_owned()),
        ),
        (
            "SniffedWoff",
            "url(fonts/a.woff)",
            woff,
            FaceDefect::PackedContainer("woff"),
        ),
        (
            "SniffedWoff2",
            "url(fonts/a.woff)",
            woff2,
            FaceDefect::PackedContainer("woff2"),
        ),
    ];

    for (family, src, bytes, expected) in cases {
        let style = format!(
            "{}p {{ font-family: \"{family}\", serif; }}",
            font_face(&family.to_ascii_lowercase(), src)
        );
        let doc = Document::open(book(
            &package(
                IDENTIFIER,
                r#"<item id="f1" href="fonts/a.woff" media-type="font/woff"/>"#,
            ),
            &chapter(&style, "Words."),
            &[("EPUB/fonts/a.woff", bytes)],
        ))
        .expect("a book");
        let warnings = face_warnings(&doc);
        assert!(
            warnings.iter().any(|(_, defect, _)| *defect == expected),
            "{family}: expected {expected:?}, got {warnings:?}"
        );
        // And the rule as a whole is reported too, because *"this entry failed"*
        // and *"the family has no file"* are two facts a producer acts on
        // differently.
        assert!(
            warnings
                .iter()
                .any(|(_, defect, _)| *defect == FaceDefect::NoUsableSource),
            "{family}: the family was left without a stated reason: {warnings:?}"
        );
        // The book still sets, in the standard 14, which is the failure mode
        // the warning exists to make visible rather than to prevent.
        assert!(page_content(&doc).contains("BT /Bk"));
    }
}

/// **The `src` list is walked to the end, and the entry that failed is still
/// named.**
///
/// `url(x.woff2) format("woff2"), url(x.ttf) format("truetype")` is what a
/// modern producer writes, with the entry this build cannot use **first**. Two
/// claims: the second entry produces the face, and the first entry's defect is
/// reported beside it rather than swallowed by the success — a host that wants
/// to stop shipping a file nothing reads needs to be told about it even when
/// the book worked.
#[test]
fn a_src_list_is_walked_past_a_refused_entry_and_the_refusal_is_still_named() {
    let style = format!(
        "{}p {{ font-family: \"pref\"; }}",
        font_face(
            "pref",
            r#"url(fonts/a.woff2) format("woff2"), url(fonts/a.ttf) format("truetype")"#
        )
    );
    let doc = Document::open(book(
        &package(
            IDENTIFIER,
            concat!(
                r#"<item id="f1" href="fonts/a.woff2" media-type="font/woff2"/>"#,
                r#"<item id="f2" href="fonts/a.ttf" media-type="font/ttf"/>"#
            ),
        ),
        &chapter(&style, "ABC"),
        &[
            ("EPUB/fonts/a.woff2", b"wOF2 not a font at all".to_vec()),
            ("EPUB/fonts/a.ttf", covering("pref", "ABC")),
        ],
    ))
    .expect("a book");

    let warnings = face_warnings(&doc);
    assert_eq!(
        warnings,
        vec![(
            "pref".to_owned(),
            FaceDefect::UnsupportedFormat("woff2".to_owned()),
            1
        )],
        "the refused entry is named and the rule is not reported as a whole failure"
    );
    // And the face that did load is the one on the page.
    let content = page_content(&doc);
    assert_eq!(
        text_objects(&content)
            .first()
            .map(|(name, _)| name.clone())
            .unwrap_or_default(),
        "Bf0",
        "the second src entry did not become the face: {content}"
    );
}

/// **`local()` is a defect of its own**, because a reading system with no
/// installed faces is a different thing from one whose `url()` is missing.
#[test]
fn a_local_source_is_refused_under_its_own_name() {
    let style = format!(
        "{}p {{ font-family: \"loc\", serif; }}",
        font_face("loc", r#"local("Palatino")"#)
    );
    let doc = Document::open(book(
        &package(IDENTIFIER, ""),
        &chapter(&style, "Words."),
        &[],
    ))
    .expect("a book");
    let defects: Vec<FaceDefect> = face_warnings(&doc).into_iter().map(|(_, d, _)| d).collect();
    assert!(
        defects.contains(&FaceDefect::LocalUnavailable),
        "local() was reported as something else: {defects:?}"
    );
    assert!(
        defects.contains(&FaceDefect::NoUsableSource),
        "the family was left with no stated reason: {defects:?}"
    );
}

// ---- the two obfuscations, on the de-obfuscated bytes ----------------------------

/// The XOR one of the two algorithms applies, done to a pristine font so a
/// fixture can put an obfuscated one in a container.
///
/// Written out here rather than called through `deobfuscate`, so the fixture
/// and the code under test are not the same arithmetic: the obfuscation is its
/// own inverse, and a test that obfuscated with the function it then inverts
/// would pass for any key at all.
fn obfuscate(key: &[u8], length: usize, bytes: &mut [u8]) {
    for (at, byte) in bytes.iter_mut().enumerate().take(length) {
        *byte ^= key[at % key.len()];
    }
}

/// An `encryption.xml` naming one algorithm over one resource.
fn encryption_xml(algorithm: &str, uri: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" "#,
            r#"xmlns:enc="http://www.w3.org/2001/04/xmlenc#">"#,
            r#"<enc:EncryptedData><enc:EncryptionMethod Algorithm="{}"/>"#,
            r#"<enc:CipherData><enc:CipherReference URI="{}"/></enc:CipherData>"#,
            r#"</enc:EncryptedData></encryption>"#
        ),
        algorithm, uri
    )
}

/// The face `typeface::load` produces for one obfuscated font, or the defects
/// it produced instead.
///
/// Loads through the real path — container, `encryption.xml`, the package's
/// identifier — and hands back the **program**, so the assertion can be made on
/// the de-obfuscated bytes rather than on a page that drew. Gap 30's milestone
/// 7 spent itself on exactly that distinction: a wrong key still produces a
/// font a reader will parse, because the header a parser looks at often
/// survives the XOR, and the page then draws *something*.
fn loaded_program(
    algorithm: &str,
    identifier: &str,
    key: &[u8],
    length: usize,
    original: &[u8],
) -> Result<Vec<u8>, Vec<FaceDefect>> {
    let mut scrambled = original.to_vec();
    obfuscate(key, length, &mut scrambled);
    assert_ne!(
        scrambled.as_slice(),
        original,
        "the fixture did not obfuscate anything"
    );

    let style = format!(
        "{}p {{ font-family: \"obf\"; }}",
        font_face("obf", "url(fonts/obf.ttf)")
    );
    let bytes = {
        let mut entries = vec![
            OcfEntry::stored("mimetype", b"application/epub+zip"),
            OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            OcfEntry::deflated(
                "META-INF/encryption.xml",
                encryption_xml(algorithm, "EPUB/fonts/obf.ttf").as_bytes(),
            ),
            OcfEntry::deflated(
                "EPUB/content.opf",
                package(
                    identifier,
                    r#"<item id="f1" href="fonts/obf.ttf" media-type="font/ttf"/>"#,
                )
                .as_bytes(),
            ),
            OcfEntry::deflated("EPUB/ch1.xhtml", chapter(&style, "ABC").as_bytes()),
        ];
        entries.push(OcfEntry::stored("EPUB/fonts/obf.ttf", &scrambled));
        let directory: Vec<usize> = (0..entries.len()).collect();
        ocf_zip(&entries, &directory)
    };

    let limits = Limits::DEFAULT;
    let archive =
        Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT).expect("a fixture container");
    let mut ocf = Ocf::open(archive, &limits);
    let encryption = ocf.encryption().expect("obfuscation only").clone();
    assert_eq!(
        encryption.entries().len(),
        1,
        "the fixture's encryption.xml did not name the font"
    );

    let rules = vec![tinker_pdf_css::font_face::FontFace {
        family: "obf".to_owned(),
        sources: vec![tinker_pdf_css::font_face::FontSource::Url {
            url: "fonts/obf.ttf".to_owned(),
            format: None,
        }],
        weight: (400, 400),
        style: tinker_pdf_css::property::FontStyle::Normal,
        base: Some("EPUB/ch1.xhtml".to_owned()),
    }];
    let set = load(&mut ocf, &rules, Some(identifier), &encryption, &limits);
    match set.faces().first() {
        Some(face) => Ok(face.program.clone()),
        None => Err(set.defects().iter().map(|(_, d)| d.clone()).collect()),
    }
}

/// **IDPF's obfuscation is undone with the SHA-1 of the identifier over 1 040
/// bytes**, and the de-obfuscated bytes are the font's own.
///
/// Asserted on the bytes: every byte, and not a table tag or a header that
/// might have survived a wrong key. And the length is asserted from the other
/// side too — a fixture that obfuscated 1 024 bytes the IDPF way leaves sixteen
/// XORed with nothing, which the equality catches.
#[test]
fn the_idpf_obfuscation_is_undone_over_its_own_thousand_and_forty_bytes() {
    let original = covering("obf", "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    assert!(
        original.len() > 1040,
        "a fixture face shorter than the obfuscated run proves nothing about the length"
    );
    let key = idpf_key(IDENTIFIER).expect("a key");
    assert_eq!(
        key, IDPF_KEY,
        "§4.4.3's key is the SHA-1 of the identifier, and this is that digest"
    );
    let program = loaded_program(IDPF_OBFUSCATION, IDENTIFIER, &IDPF_KEY, 1040, &original)
        .expect("the face loads");
    assert_eq!(
        program, original,
        "the de-obfuscated bytes are not the font's"
    );

    // And the wrong length is a wrong answer: Adobe's 1 024 leaves sixteen
    // bytes of the file XORed with a key that was never applied to them.
    let wrong = loaded_program(IDPF_OBFUSCATION, IDENTIFIER, &IDPF_KEY, 1024, &original);
    assert_ne!(
        wrong.ok().as_deref(),
        Some(original.as_slice()),
        "1 024 bytes and 1 040 bytes cannot both be right"
    );
}

/// **Adobe's obfuscation is undone with the identifier's sixteen UUID bytes
/// over 1 024**, and the same equality holds.
#[test]
fn the_adobe_obfuscation_is_undone_over_its_own_thousand_and_twenty_four_bytes() {
    let original = covering("obf", "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    let key = adobe_key(IDENTIFIER).expect("a key");
    assert_eq!(
        key, ADOBE_KEY,
        "Adobe's key is the identifier's own sixteen bytes, high nibble first"
    );
    let program = loaded_program(ADOBE_OBFUSCATION, IDENTIFIER, &ADOBE_KEY, 1024, &original)
        .expect("the face loads");
    assert_eq!(
        program, original,
        "the de-obfuscated bytes are not the font's"
    );

    // The two algorithms are not interchangeable: the same file obfuscated
    // Adobe's way and read IDPF's way is a font full of noise, and this build
    // says so rather than embedding it.
    let crossed = loaded_program(IDPF_OBFUSCATION, IDENTIFIER, &ADOBE_KEY, 1024, &original);
    assert_ne!(
        crossed.ok().as_deref(),
        Some(original.as_slice()),
        "one algorithm undid the other's key"
    );
}

/// **§4.4.3's whitespace-stripping, proved by an identifier that has some.**
///
/// A pretty-printed package document puts a newline and an indent inside
/// `<dc:identifier>`, and an XML parser hands the text back with them. Row 9
/// asks for this to be proved rather than asserted, and there are two halves:
///
/// - the key is the **same** as the key for the same identifier written
///   without whitespace, so a book obfuscated by a producer that did not indent
///   still opens on a reading system that did;
/// - and the whitespace is removed from **inside** as well as from the ends,
///   which trimming alone would not do — an identifier wrapped across two lines
///   has a newline in the middle of it.
#[test]
fn the_idpf_key_strips_the_whitespace_section_4_4_3_says_to_strip() {
    let tight = "urn:uuid:1f0c2c1e-0000-4000-8000-00000000000a";
    let expected = idpf_key(tight).expect("a key");

    // Whitespace at the ends, which trimming would also fix.
    let padded = format!("\n      {tight}\n   ");
    assert_eq!(
        idpf_key(&padded),
        Ok(expected),
        "the ends were not stripped"
    );

    // And whitespace in the **middle**, which trimming would not.
    let wrapped = "urn:uuid:1f0c2c1e-0000-4000-\n      8000-00000000000a";
    assert_eq!(
        idpf_key(wrapped),
        Ok(expected),
        "an identifier wrapped across two lines got a different key"
    );
    // All four of §4.4.3's characters, and not merely the two a producer's
    // formatter happens to emit.
    let all_four = "urn:uuid:1f0c2c1e-0000-4000\t-8000-\r\n 00000000000a";
    assert_eq!(idpf_key(all_four), Ok(expected));

    // The stripping is not "delete every non-hexadecimal character": an
    // identifier that differs from this one by a real character has a different
    // key, which is what says the filter is whitespace and not noise.
    let different = "urn:uuid:1f0c2c1e-0000-4000-8000-00000000000b";
    assert_ne!(idpf_key(different), Ok(expected));

    // An identifier that is **only** whitespace has no key at all, because the
    // SHA-1 of the empty string is a perfectly good digest and a perfectly
    // wrong key.
    assert_eq!(idpf_key("  \t\r\n "), Err(KeyDefect::NoIdentifier));
}

/// **A whitespace-padded identifier de-obfuscates a real font in a real
/// container.**
///
/// The unit test above is the arithmetic; this is the same claim through the
/// path a book takes, with the whitespace where a pretty-printer puts it. The
/// two are separate because the stripping could be correct in `idpf_key` and
/// never reached — an earlier build could trim the identifier at the parser and
/// leave `idpf_key`'s filter dead.
#[test]
fn a_pretty_printed_identifier_still_opens_its_own_obfuscated_font() {
    let original = covering("obf", "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    let padded = format!("\n        {IDENTIFIER}\n    ");
    let program = loaded_program(IDPF_OBFUSCATION, &padded, &IDPF_KEY, 1040, &original)
        .expect("the face loads");
    assert_eq!(
        program, original,
        "an indented dc:identifier produced a different key"
    );
}

/// **An identifier Adobe's algorithm has no key for is refused under its own
/// name.**
///
/// A book obfuscated the IDPF way works with any identifier a publisher chose
/// and the same book obfuscated Adobe's way does not, so a reading system that
/// reported *"the key failed"* for both would send a producer looking in the
/// wrong place.
#[test]
fn an_identifier_that_is_not_a_uuid_has_no_adobe_key_and_says_so() {
    assert_eq!(idpf_key("urn:isbn:9780000000001").map(|k| k.len()), Ok(20));
    assert_eq!(
        adobe_key("urn:isbn:9780000000001"),
        Err(KeyDefect::IdentifierIsNotAUuid)
    );
    assert_eq!(adobe_key(""), Err(KeyDefect::NoIdentifier));
    // Thirty-one hexadecimal digits is not a UUID either, and a build that
    // took whatever it found would key on fifteen bytes and a zero.
    assert_eq!(
        adobe_key("urn:uuid:1f0c2c1e-0000-4000-8000-0000000000a"),
        Err(KeyDefect::IdentifierIsNotAUuid)
    );

    // And through the container: the face does not load, and the reason names
    // the identifier rather than the font.
    let original = covering("obf", "ABC");
    let defects = loaded_program(
        ADOBE_OBFUSCATION,
        "urn:isbn:9780000000001",
        // Some key: which one cannot matter, because the reader never derives
        // one at all for an identifier with no UUID in it.
        &[0x5Au8; 16],
        1024,
        &original,
    )
    .expect_err("a book with no Adobe key has no face");
    assert!(
        defects.contains(&FaceDefect::KeyUnavailable(KeyDefect::IdentifierIsNotAUuid)),
        "the missing key was reported as something else: {defects:?}"
    );
}

/// **A file shorter than the obfuscated run is XORed as far as it goes.**
///
/// §4.4.4's own wording, and the two failures it rules out are opposite: a
/// build that refused a short file would refuse every small font, and one that
/// read past the end would panic on it.
#[test]
fn a_font_shorter_than_the_obfuscated_run_is_covered_to_its_end() {
    let mut short = vec![0xA5u8; 300];
    let original = short.clone();
    obfuscate(&IDPF_KEY, 1040, &mut short);
    assert_ne!(short, original);
    deobfuscate(
        tinker_pdf::epub::ocf::Obfuscation::Idpf,
        IDENTIFIER,
        &mut short,
    )
    .expect("a key");
    assert_eq!(short, original, "a short file was not covered to its end");
}

// ---- the provider question -------------------------------------------------------

/// A provider that records every family it is asked for and answers with one
/// face.
struct Recording {
    asked: Mutex<Vec<String>>,
    face: Arc<Vec<u8>>,
}

impl FontProvider for Recording {
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        self.asked
            .lock()
            .expect("no other thread panicked")
            .push(request.base_font.clone());
        Some(self.face.clone())
    }
}

/// **`FontProvider` is asked per family, and the three generic families arrive
/// under three different names.**
///
/// Row 9 asks for *"`FontProvider`'s per-family fallback question answered —
/// the trait extended, or the reason it is not recorded"*. **The trait is not
/// extended, and this is the evidence rather than the assertion.**
/// [`FontRequest`] already carries `base_font`, and this build's synthesis
/// writes a *distinct* `/BaseFont` per generic family — `Times-Roman`,
/// `Helvetica`, `Courier` — so a host with a serif face and a sans face is
/// already able to answer for each. Extending the trait would add a second way
/// to say the same thing and a second way for the two to disagree.
///
/// What a provider **cannot** do is change the pagination, and that is the
/// subject of the test below rather than a limitation of the trait: the
/// document was synthesised at `open`, which is what
/// `ArchiveWarning::FontsAttachedAfterPagination` says out loud.
#[test]
fn a_provider_is_asked_per_family_and_the_three_generics_arrive_by_name() {
    let provider = Arc::new(Recording {
        asked: Mutex::new(Vec::new()),
        face: Arc::new(epub_support::typeface::boxy_font()),
    });
    let style = concat!(
        ".s { font-family: serif; }",
        ".n { font-family: sans-serif; }",
        ".m { font-family: monospace; }"
    );
    // The fixture wraps its body in one `<p>`, so this closes it, writes the
    // three paragraphs the test is about, and opens an empty one to be closed.
    let body = r#"</p><p class="s">Serif.</p><p class="n">Sans.</p><p class="m">Mono.</p><p>"#;
    let doc = Document::open_with(
        book(&package(IDENTIFIER, ""), &chapter(style, body), &[]),
        &OpenOptions::default().with_fonts(provider.clone()),
    )
    .expect("a book");
    let _ = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    let mut asked = provider
        .asked
        .lock()
        .expect("no other thread panicked")
        .clone();
    asked.sort_unstable();
    asked.dedup();
    assert_eq!(
        asked,
        ["Courier", "Helvetica", "Times-Roman"],
        "a provider was not asked for the three generic families by name"
    );
}

/// **The page count does not depend on whether a provider is attached**, and
/// neither does a single byte of the page.
///
/// Milestone 4's whole argument for `OpenOptions::fonts` rests on this: the
/// generic families are measured at the standard 14's **published** advances,
/// which `tinker-pdf-font` holds because a PDF may omit `/Widths` for them, so
/// nothing in the layout asks whether a host supplied a face. If this fails,
/// the pagination a caller gets is a function of their machine's font
/// configuration and the page count in the report means nothing.
///
/// Asserted over the whole committed corpus rather than over one fixture,
/// because a book of one paragraph would agree by having nothing to disagree
/// about.
#[test]
fn the_page_count_does_not_depend_on_whether_a_provider_is_attached() {
    let corpus = [
        "calibre-book-cover.epub",
        "pandoc-book-cover.epub",
        "pandoc-plates.epub",
    ];
    let provider = Arc::new(tinker_pdf::SimpleFontProvider::new(
        epub_support::typeface::boxy_font(),
    ));
    for name in corpus {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("epub")
            .join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {name}: {e}"));

        let bare = Document::open(bytes.clone()).expect("a book");
        let with = Document::open_with(bytes, &OpenOptions::default().with_fonts(provider.clone()))
            .expect("a book");

        assert_eq!(
            bare.page_count(),
            with.page_count(),
            "{name}: the page count moved when a provider was attached"
        );
        assert_eq!(
            page_content(&bare),
            page_content(&with),
            "{name}: the first page's content stream moved when a provider was attached"
        );
    }
}

/// **The three generic families measure at their own published advances**, and
/// the three numbers are different.
///
/// The reason the test above can hold: `Times-Roman`'s `a` is 444 thousandths,
/// `Helvetica`'s is 556 and `Courier`'s is 600 because every Courier glyph is.
/// A build that gave all three the same number would paginate consistently and
/// wrongly, and `the_page_count_does_not_depend_on_whether_a_provider_is_attached`
/// would still pass.
#[test]
fn the_three_generic_families_measure_at_their_own_published_advances() {
    use tinker_pdf::epub::paint::BookMetrics;
    use tinker_pdf_css::property::{FontFamily, FontStyle};
    use tinker_pdf_layout::metrics::{FontRequest, Metrics};

    let advance = |family: FontFamily| {
        let families = [family];
        let font = FontRequest {
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            size: 1000.0,
        };
        BookMetrics::STANDARD.advance('a', &font)
    };
    assert!((advance(FontFamily::Serif) - 444.0).abs() < 1e-9);
    assert!((advance(FontFamily::SansSerif) - 556.0).abs() < 1e-9);
    assert!((advance(FontFamily::Monospace) - 600.0).abs() < 1e-9);

    // And the line heights are the three families' own AFM ascenders and
    // descenders, which is the other half of a pagination: a build with one
    // advance table and one line height would set every book at Times' leading.
    let vertical = |family: FontFamily| {
        let families = [family];
        let font = FontRequest {
            families: &families,
            weight: 400,
            style: FontStyle::Normal,
            size: 1000.0,
        };
        let v = BookMetrics::STANDARD.vertical(&font);
        (v.ascent, v.descent)
    };
    assert_eq!(vertical(FontFamily::Serif), (683.0, 217.0));
    assert_eq!(vertical(FontFamily::SansSerif), (718.0, 207.0));
    assert_eq!(vertical(FontFamily::Monospace), (629.0, 157.0));
}

// ---- the font census, per book ---------------------------------------------------

/// The committed corpus, in the order `tests/epub/FONTS.tsv` records it.
const COMMITTED: [&str; 6] = [
    "calibre-book-cover.epub",
    "calibre-book-nocover.epub",
    "pandoc-book-cover.epub",
    "pandoc-book-epub2.epub",
    "pandoc-book-nocover.epub",
    "pandoc-plates.epub",
];

fn corpus_book(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {name}: {e}"))
}

/// **The font census, per book, against the record.**
///
/// Milestone 8's census had a hole in it that its own `Still owed` named: *"a
/// notdef glyph unwarned and more than 224 out-of-encoding characters per face
/// lost"*. The first half is the `uncovered` column here — every character a
/// book draws that no available face has a glyph for — and it was **zero
/// before this milestone because nothing counted it**, not because no book had
/// any.
///
/// Written down in `tests/epub/FONTS.tsv` rather than listed here, for
/// `CENSUS.tsv`'s reason: a milestone that gives this build a CJK face, or that
/// stops warning, has to re-measure rather than argue.
///
/// Three claims beside the equality, and each rules out a different way of
/// satisfying the file trivially:
///
/// - **the numbers are not all zero**, so a build that reported nothing could
///   not pass by writing zeroes into the record;
/// - **`pandoc-plates.epub` is zero and the others are not**, which is what
///   says the figure is a property of the book rather than a constant — it is
///   the one committed book with no Japanese line in it;
/// - and **nothing is unrepresented**, which is the other half of the pair: a
///   character drawn as a notdef is in `Page::text()` and a character with no
///   code is not, and a build that folded the two together would report one
///   number for both.
#[test]
fn the_font_census_is_the_one_the_record_states() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join("FONTS.tsv");
    let recorded: Vec<String> = std::fs::read_to_string(&path)
        .expect("tests/epub/FONTS.tsv")
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();

    let mut measured = Vec::new();
    let mut uncovered_total = 0usize;
    for name in COMMITTED {
        let doc = Document::open(corpus_book(name)).expect(name);
        let warnings = doc.archive().expect("a report").warnings().to_vec();
        let count = |pick: fn(&ArchiveWarning) -> Option<usize>| -> usize {
            warnings.iter().filter_map(pick).sum()
        };
        let uncovered = count(|w| match w {
            ArchiveWarning::UncoveredCharacters { characters } => Some(*characters),
            _ => None,
        });
        let unrepresented = count(|w| match w {
            ArchiveWarning::UnrepresentedCharacters { characters } => Some(*characters),
            _ => None,
        });
        let defects: usize = face_warnings(&doc).iter().map(|(_, _, rules)| rules).sum();
        println!(
            "  {name:28} {uncovered:4} notdefs {unrepresented:4} lost {defects:3} face defects"
        );
        uncovered_total += uncovered;
        measured.push(format!("{name}\t{uncovered}\t{unrepresented}\t{defects}"));

        assert_eq!(
            unrepresented, 0,
            "{name}: a character got no code at all, which no committed book does"
        );
    }
    assert_eq!(measured, recorded, "tests/epub/FONTS.tsv is out of date");
    assert!(
        uncovered_total > 0,
        "no committed book draws a notdef, which five of the six do"
    );
}

/// **The one book with no Japanese line in it reports nothing**, and the others
/// report their own count.
///
/// The census above is an equality against a file and would pass if every
/// figure in it were the same number. This is what says they are not: a warning
/// every book trips is not a warning, which is the pair `with_fonts` was held
/// to in milestone 4 and the same pair here.
#[test]
fn the_notdef_count_is_a_property_of_the_book_and_not_a_constant() {
    let uncovered = |name: &str| -> usize {
        Document::open(corpus_book(name))
            .expect(name)
            .archive()
            .expect("a report")
            .warnings()
            .iter()
            .filter_map(|w| match w {
                ArchiveWarning::UncoveredCharacters { characters } => Some(*characters),
                _ => None,
            })
            .sum()
    };
    assert_eq!(
        uncovered("pandoc-plates.epub"),
        0,
        "the book with no line of Japanese reported one"
    );
    assert!(uncovered("calibre-book-cover.epub") > 0);
    assert_ne!(
        uncovered("calibre-book-cover.epub"),
        uncovered("pandoc-book-cover.epub"),
        "two producers' books of the same text report the same number, which \
         would make this a constant rather than a measurement"
    );
}
