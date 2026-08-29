//! A one-chapter book around an embedded face, for the tests that need one.
//!
//! `epub_fonts.rs` grew this shape first and keeps its own copy, because what
//! it builds is a book with *several* faces and a family list to walk. What
//! milestone 6 of `docs/design/shaping.md` needs is the other thing: one face,
//! one paragraph, and the same bytes from two test binaries — `epub_shaped.rs`
//! asserts what the page draws, and `epub_reftest.rs` lays two spellings of it
//! side by side. A builder in each would be two fixtures that could drift, and
//! a reftest whose two sides came from two builders proves nothing.

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
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000000b</dc:identifier>"#,
            r#"<dc:title>{family}</dc:title>"#,
            r#"<dc:language>ar</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="f1" href="fonts/face.ttf" media-type="font/ttf"/>"#,
            r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
        ),
        family = family
    );
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
            r#"<style>@font-face {{ font-family: "{family}";"#,
            r#" src: url(fonts/face.ttf) format("truetype"); }}"#,
            r#" body {{ margin: 0 }}"#,
            r#" p {{ margin: 0; font-family: "{family}"; font-size: {size}px; }}</style>"#,
            r#"</head><body><p>{body}</p></body></html>"#
        ),
        family = family,
        size = size_px,
        body = body
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
        OcfEntry::deflated("EPUB/fonts/face.ttf", program),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}
