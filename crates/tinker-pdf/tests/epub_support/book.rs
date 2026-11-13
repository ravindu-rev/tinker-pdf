//! A one-chapter book around embedded faces, for the tests that need one.
//!
//! One paragraph, one chapter, and the same bytes from two test binaries —
//! `epub_shaped.rs` asserts what the page draws, and `epub_reftest.rs` lays two
//! spellings of it side by side. A builder in each would be two fixtures that
//! could drift, and a reftest whose two sides came from two builders proves
//! nothing.
//!
//! [`one_face_book`] is a caller of [`faces_book`] rather than a second
//! builder, for the same reason: a book with one face and a book with two have
//! to declare their `@font-face` rules and their family list identically, or a
//! test that compares a page from each is comparing the builders.
//!
//! `epub_fonts.rs` still keeps its own, because its books carry a *generic*
//! family at the end of the list (`"Alpha", "Beta", serif`) and asserting what
//! falls through to the standard 14 is what that file is about.
//!
//! [`generated_book`] is the other shape: a book of as many chapters as asked,
//! each a page, for the tests that compare one book at two lengths —
//! `producer_work.rs`'s census and `guards.rs`'s A2 book.

use super::{ocf_zip, OcfEntry};

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

/// A book of one chapter whose one paragraph is set in `family`, with
/// `program` embedded under that family.
///
/// `body` is markup rather than text, so a caller can wrap it in the inline
/// boxes a reftest pair is made of.
#[must_use]
pub fn one_face_book(family: &str, program: &[u8], size_px: u32, body: &str) -> Vec<u8> {
    faces_book(&[(family, program)], size_px, body)
}

/// A book of one chapter whose one paragraph lists **every** family in order,
/// with each face embedded under its own.
///
/// [`one_face_book`] is a caller of this with one face, so the two cannot
/// drift: a fixture whose one-face and two-face books declared their
/// `@font-face` rules differently would be two fixtures, and a test that
/// compared a page from each would be comparing the builders.
///
/// The family list is the declaration order, which is what makes
/// `css-fonts-4` §5.3's per-character walk observable: a character the first
/// family does not cover falls to the second, and the run becomes two
/// segments in two faces.
#[must_use]
pub fn faces_book(faces: &[(&str, &[u8])], size_px: u32, body: &str) -> Vec<u8> {
    faces_book_with(faces, size_px, "", body)
}

/// [`faces_book`], with `attributes` written into the paragraph's start tag
/// — ` dir="rtl"`, ` style="text-align: end"` — for the tests about what a
/// paragraph's own attributes do to its lines. An empty `attributes` is
/// [`faces_book`]'s bytes exactly.
#[must_use]
pub fn faces_book_with(
    faces: &[(&str, &[u8])],
    size_px: u32,
    attributes: &str,
    body: &str,
) -> Vec<u8> {
    chapters_book(faces, size_px, attributes, &[body])
}

/// [`faces_book_with`], with one chapter for each of `bodies`, in spine
/// order, every chapter one paragraph set in the faces' family list. One
/// chapter is [`faces_book_with`]'s bytes exactly, since that is a caller of
/// this.
#[must_use]
pub fn chapters_book(
    faces: &[(&str, &[u8])],
    size_px: u32,
    attributes: &str,
    bodies: &[&str],
) -> Vec<u8> {
    let mut chapter_items = String::new();
    let mut spine = String::new();
    for at in 1..=bodies.len() {
        chapter_items.push_str(&format!(
            r#"<item id="c{at}" href="ch{at}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{at}"/>"#));
    }
    let mut items = String::new();
    let mut rules = String::new();
    let mut families: Vec<String> = Vec::with_capacity(faces.len());
    for (at, (family, _)) in faces.iter().enumerate() {
        items.push_str(&format!(
            r#"<item id="f{at}" href="fonts/face{at}.ttf" media-type="font/ttf"/>"#
        ));
        rules.push_str(&format!(
            "@font-face {{ font-family: \"{family}\"; \
             src: url(fonts/face{at}.ttf) format(\"truetype\"); }} "
        ));
        families.push(format!("\"{family}\""));
    }
    let title = faces.first().map_or("A Book", |(family, _)| *family);
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000000b</dc:identifier>"#,
            r#"<dc:title>{title}</dc:title>"#,
            r#"<dc:language>ar</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"{chapter_items}"#,
            r#"{items}"#,
            r#"</manifest><spine>{spine}</spine></package>"#
        ),
        title = title,
        chapter_items = chapter_items,
        items = items,
        spine = spine
    );
    let chapters: Vec<(String, String)> = bodies
        .iter()
        .enumerate()
        .map(|(at, body)| {
            let chapter = format!(
                concat!(
                    r#"<?xml version="1.0" encoding="utf-8"?>"#,
                    r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
                    r#"<style>{rules}"#,
                    r#" body {{ margin: 0 }}"#,
                    r#" p {{ margin: 0; font-family: {families}; font-size: {size}px; }}</style>"#,
                    r#"</head><body><p{attributes}>{body}</p></body></html>"#
                ),
                rules = rules,
                families = families.join(", "),
                size = size_px,
                attributes = attributes,
                body = body
            );
            (format!("EPUB/ch{}.xhtml", at + 1), chapter)
        })
        .collect();
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
    ];
    for (path, chapter) in &chapters {
        entries.push(OcfEntry::deflated(path, chapter.as_bytes()));
    }
    let paths: Vec<String> = (0..faces.len())
        .map(|at| format!("EPUB/fonts/face{at}.ttf"))
        .collect();
    for (path, (_, program)) in paths.iter().zip(faces.iter()) {
        entries.push(OcfEntry::deflated(path, program));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// A book of one chapter whose `<style>` is `style` and whose `<body>` is
/// `body`, in `language`, set in whatever the user-agent sheet and `style`
/// say — no embedded face.
///
/// For the tests that need a page *laid out* a particular way — two columns,
/// a ruled table — rather than set in a particular face.
#[must_use]
pub fn styled_book(language: &str, style: &str, body: &str) -> Vec<u8> {
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000000c</dc:identifier>"#,
            r#"<dc:title>A Styled Book</dc:title>"#,
            r#"<dc:language>{language}</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
        ),
        language = language
    );
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="{language}"><head>"#,
            r#"<title>A Chapter</title><style>{style}</style>"#,
            r#"</head><body>{body}</body></html>"#
        ),
        language = language,
        style = style,
        body = body
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// The words [`prose`] draws from: ASCII, so every one is in the standard 14's
/// encoding and in any face a test supplies.
const WORDS: [&str; 24] = [
    "the", "quiet", "river", "turned", "toward", "evening", "and", "every", "lamp", "along", "its",
    "bank", "was", "lit", "by", "hands", "that", "knew", "each", "name", "of", "stone", "light",
    "water",
];

/// About `length` characters of [`WORDS`] in sentences, from a linear
/// congruential sequence started at `seed`: the same text for the same two
/// numbers on every target, and a different text for each seed.
#[must_use]
pub fn prose(seed: u64, length: usize) -> String {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move |bound: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) % bound as u64) as usize
    };
    let mut text = String::new();
    let mut start = true;
    while text.len() < length {
        if !text.is_empty() {
            text.push(' ');
        }
        let word = WORDS[next(WORDS.len())];
        if start {
            text.push_str(&word[..1].to_uppercase());
            text.push_str(&word[1..]);
            start = false;
        } else {
            text.push_str(word);
        }
        if next(10) == 0 {
            text.push('.');
            start = true;
        }
    }
    text.push('.');
    text
}

/// A book of `chapters` chapters, in the shape MS-04's layout group generates
/// (`benches/support/inputs.rs`'s `book`): each chapter one XHTML file holding
/// a heading and one paragraph of about 600 characters of [`prose`], each
/// linking the book's one stylesheet, so at the default 432 × 648 pt box every
/// chapter starts a page and fits on it and the book is `chapters` pages long.
///
/// No face is embedded, so the book's bytes are the same in both builds and
/// its lines break by the standard 14's metrics; a caller that wants its
/// glyphs drawn alike in both supplies a face when it opens the book.
#[must_use]
pub fn generated_book(chapters: usize) -> Vec<u8> {
    let mut manifest = String::new();
    let mut spine = String::new();
    for chapter in 1..=chapters {
        manifest.push_str(&format!(
            r#"<item id="c{chapter}" href="c{chapter}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{chapter}"/>"#));
    }
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000000c0</dc:identifier>"#,
            r#"<dc:title>A Book of {chapters} Chapters</dc:title>"#,
            r#"<dc:language>en</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"<item id="css" href="book.css" media-type="text/css"/>"#,
            r#"{manifest}</manifest><spine>{spine}</spine></package>"#
        ),
        chapters = chapters,
        manifest = manifest,
        spine = spine
    );
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated(
            "EPUB/book.css",
            b"h2 { margin: 0 0 6pt 0 } p { text-indent: 1em; margin: 0 }",
        ),
    ];
    for chapter in 1..=chapters {
        let body = prose(chapter as u64, 600);
        let xhtml = format!(
            concat!(
                r#"<?xml version="1.0" encoding="utf-8"?>"#,
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en"><head>"#,
                r#"<title>Chapter {chapter}</title>"#,
                r#"<link rel="stylesheet" type="text/css" href="book.css"/>"#,
                r#"</head><body><h2>Chapter {chapter}</h2><p>{body}</p></body></html>"#
            ),
            chapter = chapter,
            body = body
        );
        entries.push(OcfEntry::deflated(
            &format!("EPUB/c{chapter}.xhtml"),
            xhtml.as_bytes(),
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}
