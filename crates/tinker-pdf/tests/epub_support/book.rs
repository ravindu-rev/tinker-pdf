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
            r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"{items}"#,
            r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
        ),
        title = title,
        items = items
    );
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
            r#"<style>{rules}"#,
            r#" body {{ margin: 0 }}"#,
            r#" p {{ margin: 0; font-family: {families}; font-size: {size}px; }}</style>"#,
            r#"</head><body><p>{body}</p></body></html>"#
        ),
        rules = rules,
        families = families.join(", "),
        size = size_px,
        body = body
    );
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let paths: Vec<String> = (0..faces.len())
        .map(|at| format!("EPUB/fonts/face{at}.ttf"))
        .collect();
    for (path, (_, program)) in paths.iter().zip(faces.iter()) {
        entries.push(OcfEntry::deflated(path, program));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}
