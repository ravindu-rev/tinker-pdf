//! The document a book is synthesised into, held to ISO 32000.
//!
//! This replaces `epub_qpdf.rs`, which asked qpdf whether the synthesised
//! document was a valid PDF, what its pages said, and what its content streams
//! held once decoded. Ruling 13 retires that; the strict validator answers the
//! structural half and raw reads answer the rest.
//!
//! **What is lost is worth naming.** qpdf was a reader nobody here wrote, and
//! the pages here are grey rectangles, so its acceptance was most of what it
//! could offer. What it also did — decoding a content stream and handing back
//! the operators — this file still does, through this crate's own filters
//! rather than through somebody else's. That is a real reduction in
//! independence and `docs/verification.md` states it.
//!
//! What survives is the part that catches things: every page is read from the
//! catalog's own `/Kids` and every value out of a dictionary as the file spells
//! it, so a `/MediaBox` the tolerant reader would have invented cannot pass.

mod epub_support;
mod validated_support;

use epub_support::typeface::Face;
use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{CosDocument, Defect, Document, Name, OpenOptions, WriteOptions};
use validated_support::{category, flat, numbers, pages, value};

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

const PACKAGE_OPF: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:2b6c0a10-0000-4000-8000-00000000000b</dc:identifier>"#,
    r#"<dc:title>A Book Read By Somebody Else</dc:title>"#,
    r#"<dc:language>en</dc:language>"#,
    r#"<dc:creator>The tinker-pdf authors</dc:creator></metadata>"#,
    r#"<manifest>"#,
    r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"<item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"<item id="gone" href="text/missing.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest>"#,
    r#"<spine><itemref idref="c1"/><itemref idref="gone"/><itemref idref="c2"/></spine>"#,
    r#"</package>"#
);

fn chapter(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><p>{body}</p></body></html>"#
    )
}

/// A book with a resolving spine item, one that does not, and a third that
/// does — so the document read here is the one a partial build produces rather
/// than the one it would like to.
fn book() -> Vec<u8> {
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE_OPF.as_bytes()),
        OcfEntry::deflated("EPUB/text/ch1.xhtml", chapter("One.").as_bytes()),
        OcfEntry::deflated("EPUB/text/ch2.xhtml", chapter("Two.").as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// A book whose one paragraph needs three embedded faces, each covering three
/// of its nine characters and none covering another's.
fn three_face_book() -> Vec<u8> {
    let faces = [
        ("alpha", "ABC", 500u16),
        ("beta", "DEF", 750),
        ("gamma", "GHI", 250),
    ];
    let mut style = String::new();
    let mut items = String::new();
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
    ];
    let mut programs: Vec<Vec<u8>> = Vec::new();
    for (family, covers, advance) in faces {
        style.push_str(&format!(
            "@font-face {{ font-family: \"{family}\"; src: url(fonts/{family}.ttf) format(\"truetype\"); }}"
        ));
        items.push_str(&format!(
            r#"<item id="{family}" href="fonts/{family}.ttf" media-type="font/ttf"/>"#
        ));
        programs.push(Face::new(family, covers).with_advance(advance).build());
    }
    style.push_str("p { font-family: \"alpha\", \"beta\", \"gamma\"; }");

    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:2b6c0a10-0000-4000-8000-00000000000c</dc:identifier>"#,
            r#"<dc:title>A Book Of Three Faces</dc:title>"#,
            r#"<dc:language>en</dc:language>"#,
            r#"<dc:creator>The tinker-pdf authors</dc:creator></metadata>"#,
            r#"<manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>{}</manifest>"#,
            r#"<spine><itemref idref="c1"/></spine></package>"#
        ),
        items
    );
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title>"#,
            r#"<style>{}</style></head><body><p>ABCDEFGHI</p></body></html>"#
        ),
        style
    );
    entries.push(OcfEntry::deflated("EPUB/content.opf", package.as_bytes()));
    entries.push(OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()));
    for ((family, _, _), program) in faces.iter().zip(programs.iter()) {
        entries.push(OcfEntry::deflated(
            &format!("EPUB/fonts/{family}.ttf"),
            program,
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

#[track_caller]
fn valid(label: &str, bytes: &[u8]) -> CosDocument {
    let doc = CosDocument::open(bytes.to_vec()).expect("the document opens");
    let defects = tinker_pdf_cos::validate(&doc);
    assert!(
        defects.is_empty(),
        "{label}: {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
    doc
}

/// Every page's content stream, decoded, in page order.
fn contents(doc: &CosDocument) -> Vec<String> {
    pages(doc)
        .into_iter()
        .map(|(reference, page)| {
            let stream = page
                .get_ref(Name::CONTENTS)
                .unwrap_or_else(|| panic!("page {reference} has no /Contents stream"));
            let bytes = doc
                .stream_decoded(stream)
                .unwrap_or_else(|e| panic!("page {reference}'s content stream: {e}"));
            String::from_utf8_lossy(&bytes)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

// ---- the structure ----------------------------------------------------------

#[test]
fn a_synthesised_book_validates() {
    let doc = Document::open(book()).expect("a book");
    assert_eq!(doc.page_count(), 3);
    valid("the synthesised document", doc.cos().bytes());
}

#[test]
fn a_saved_synthesised_book_validates() {
    let doc = Document::open(book()).expect("a book");
    for (label, options) in [
        ("saved", WriteOptions::default()),
        (
            "linearized",
            WriteOptions {
                linearize: true,
                ..WriteOptions::default()
            },
        ),
    ] {
        valid(label, &doc.editor().save(&options));
    }
}

// ---- what the structure says ------------------------------------------------

/// Every page of the spine is there, at the box the caller stated.
///
/// A page count and a page box are two claims: a build that wrote the right
/// number of pages at the wrong size satisfies the first and not the second.
#[test]
fn the_spines_pages_are_at_the_box_the_caller_stated() {
    let doc = Document::open_with(book(), &OpenOptions::at_page(300.0, 500.0)).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let doc = valid("the saved book", &pdf);

    let pages = pages(&doc);
    assert_eq!(pages.len(), 3, "three pages");
    for (reference, page) in &pages {
        assert_eq!(
            numbers(&doc, page, b"MediaBox"),
            Some(vec![0.0, 0.0, 300.0, 500.0]),
            "page {reference} is not the box the caller stated"
        );
        assert!(
            category(&doc, page, b"XObject").is_empty(),
            "a book of placeholders carries no image: {}",
            flat(&doc, &value(&doc, page, b"Resources"))
        );
    }
}

/// The operators tell a placeholder page from a page that reads.
///
/// The fixture is three pages and **two kinds of page**: the middle one is a
/// spine item that does not resolve and is the neutral grey, and the two either
/// side are chapters that laid out and carry text. A build whose placeholder
/// was white would write `1 g`; one that filled part of the page would write a
/// smaller rectangle; and one that greyed *every* page would fail on the first
/// and third. Only reading the operators tells those apart.
#[test]
fn a_placeholder_page_and_a_page_that_reads_are_different_operators() {
    let doc = Document::open(book()).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let doc = valid("the saved book", &pdf);

    let contents = contents(&doc);
    assert_eq!(contents.len(), 3, "three pages, three content streams");

    // The middle page: the spine item that does not resolve. The grey is
    // compared as a number rather than as the string it happens to be printed
    // as, because how many digits a writer emits is a formatting choice and
    // 191/255 is the claim.
    let words: Vec<&str> = contents[1].split(' ').collect();
    let grey: f64 = words
        .iter()
        .position(|word| *word == "g")
        .and_then(|at| words.get(at.wrapping_sub(1)))
        .and_then(|word| word.parse().ok())
        .unwrap_or_else(|| panic!("no grey level in: {}", contents[1]));
    assert!(
        (grey - 191.0 / 255.0).abs() < 1e-9,
        "page content is not the neutral placeholder grey: {grey}"
    );
    assert!(
        contents[1].contains("0 0 432 648 re"),
        "the placeholder does not cover the page box: {}",
        contents[1]
    );
    assert!(
        !contents[1].contains("Tj"),
        "the placeholder page draws text: {}",
        contents[1]
    );

    for (at, body) in [(0usize, "One."), (2, "Two.")] {
        assert!(
            contents[at].contains(&format!("({body}) Tj")),
            "page {at} does not say {body:?}: {}",
            contents[at]
        );
        assert!(
            contents[at].contains("BT /Bk0"),
            "page {at} does not set the serif face this build registered: {}",
            contents[at]
        );
        assert!(
            !contents[at].contains(" g "),
            "page {at} is a chapter and is painted as a placeholder: {}",
            contents[at]
        );
    }
}

/// The book's own metadata reaches the document's `/Info`.
///
/// §5.5.3.1's three required Dublin Core elements are the criterion; a
/// `dc:title` that is parsed and thrown away is the failure the whole feature
/// is organised around. Read out of the trailer's `/Info` dictionary rather
/// than through this engine's metadata surface, which would accept a form
/// nothing else does.
#[test]
fn the_books_title_reaches_the_documents_info() {
    let doc = Document::open(book()).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let doc = valid("the saved book", &pdf);

    let info = doc
        .trailer()
        .get_ref(Name::INFO)
        .expect("the trailer names an /Info");
    let info = doc.get(info).expect("it resolves");
    let info = info.as_dict().expect("an /Info dictionary");

    let entry = |key: &[u8]| -> String {
        info.get_string(doc.intern(key))
            .map(|s| String::from_utf8_lossy(&s.bytes).into_owned())
            .unwrap_or_default()
    };
    assert!(
        entry(b"Title").contains("A Book Read By Somebody Else"),
        "the book's title did not reach /Title: {}",
        entry(b"Title")
    );
    assert!(
        entry(b"Author").contains("The tinker-pdf authors"),
        "the book's creator did not reach /Author: {}",
        entry(b"Author")
    );
}

/// Three text objects, three font resources, and three `/W` arrays carrying
/// the three faces' own `hmtx` advances.
///
/// A build that wrote one face's metrics into all three passes every
/// count-based test there is, which is why the widths are read per resource
/// rather than in aggregate.
#[test]
fn a_books_three_faces_keep_their_own_widths() {
    let doc = Document::open(three_face_book()).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let doc = valid("the saved book", &pdf);

    let stream = contents(&doc).first().cloned().expect("one page");
    assert_eq!(
        stream.matches("BT /").count(),
        3,
        "three text objects: {stream}"
    );
    for resource in ["BT /Bf0 ", "BT /Bf1 ", "BT /Bf2 "] {
        assert!(stream.contains(resource), "{resource} is not in {stream}");
    }

    let (_, page) = pages(&doc).into_iter().next().expect("one page");
    for (name, advance) in [("Bf0", 500.0), ("Bf1", 750.0), ("Bf2", 250.0)] {
        let (_, font) = validated_support::resource(&doc, &page, b"Font", name.as_bytes())
            .unwrap_or_else(|| panic!("{name} is not in the page's resources"));
        let font = font.as_dict().expect("a font dictionary").clone();
        assert_eq!(
            validated_support::name(&doc, &font, b"Subtype").as_deref(),
            Some(&b"Type0"[..]),
            "{name} is not a composite font"
        );
        assert_eq!(
            validated_support::name(&doc, &font, b"Encoding").as_deref(),
            Some(&b"Identity-H"[..])
        );

        let descendants = value(&doc, &font, b"DescendantFonts");
        let descendants = descendants.as_array().expect("9.7.1's one descendant");
        let descendant = doc.resolve(&descendants[0]);
        let descendant = descendant.as_dict().expect("a CID font").clone();

        // 9.7.4.3: `c [w1 w2 ...]`, and every width in it is this face's own
        // advance rather than the first face's.
        let widths = value(&doc, &descendant, b"W");
        let widths = widths.as_array().expect("a /W array").to_vec();
        let mut seen = 0usize;
        for entry in &widths {
            let resolved = doc.resolve(entry);
            let Some(run) = resolved.as_array() else {
                continue;
            };
            for width in run {
                assert_eq!(
                    width.as_number(),
                    Some(advance),
                    "{name}'s /W carries a width that is not its own face's"
                );
                seen += 1;
            }
        }
        assert!(seen > 0, "{name}'s /W states no widths at all");
    }
}
