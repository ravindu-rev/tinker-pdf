//! FictionBook 2 onto the HTML path (tier 5's FB2 row): `Document::open` of an
//! `.fb2`, and of the `.fb2.zip` it is usually shipped as.
//!
//! The fixture is written here, from FictionBook 2.1's schema, with every
//! element the translation maps — a book title and an author, a cover, two
//! bodies, nested sections with titles, an epigraph, a poem, a cite, a table,
//! a picture, a note reference and the note it names — and a PNG carried in a
//! `<binary>`. The claims are the module's: an FB2 is laid out **exactly** as
//! the XHTML it translates to, its description is its document information
//! and not its text, its pictures are its own binaries, a note link lands on
//! the note, and what does not translate is named.

mod cbz_support;
mod render_support;

use std::sync::Arc;

use cbz_support::{rgb_png, zip, Damage, ZipFile};
use render_support::{curvy_font, ink};
use tinker_pdf::cbz::ImageDefect;
use tinker_pdf::standalone::TranslationDefect;
use tinker_pdf::{
    ArchiveWarning, Bitmap, Document, DocumentBuilder, FromHtml, PageBox, RenderOptions,
    SimpleFontProvider, Standalone,
};

fn render(document: &Document, page: u32) -> Bitmap {
    document
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// `document` with a face to draw its text in.
///
/// A document that embeds no face draws **none** of its text without one — it
/// extracts perfectly and renders `UnreadableFont` and a blank page — so two
/// pages compared without it are two blank pages, equal whatever was laid out
/// on them. The face is `render_support`'s synthetic one, attached after
/// pagination: the line breaks are the ones `open` made from the built-in
/// metrics, on both sides of a comparison, and only the glyphs are its.
fn drawn(document: Document) -> Document {
    document.with_fonts(Arc::new(SimpleFontProvider::new(curvy_font())))
}

/// Pixels that are not white: what says a comparison compared something.
const LEAST_INK: usize = 200;

fn warnings(document: &Document) -> Vec<ArchiveWarning> {
    document
        .archive()
        .expect("a synthesised document has a report")
        .warnings()
        .to_vec()
}

fn text(document: &Document) -> String {
    (0..document.page_count())
        .map(|at| document.page(at).expect("a page").text().plain_text())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// RFC 4648 §4, so the fixture can carry a picture.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for (at, chunk) in bytes.chunks(3).enumerate() {
        // FB2 producers break a binary into lines, and the decoder skips them.
        if at > 0 && at % 19 == 0 {
            out.push('\n');
        }
        let group = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((group >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The blue the fixture's picture is, every pixel of it.
const BLUE: [u8; 3] = [0, 0, 230];

fn picture() -> Vec<u8> {
    rgb_png(24, 24, &BLUE.repeat(24 * 24))
}

/// A whole FB2 around `bodies`, with `binaries` after them.
fn book(bodies: &str, binaries: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
            "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" ",
            "xmlns:l=\"http://www.w3.org/1999/xlink\">\n",
            "<description><title-info><genre>prose</genre>",
            "<author><first-name>Anna</first-name><middle-name>K.</middle-name>",
            "<last-name>Writer</last-name></author>",
            "<author><nickname>Second Hand</nickname></author>",
            "<book-title>The   Long Way</book-title>",
            "<annotation><p>An annotation that is metadata.</p></annotation>",
            "<lang>en</lang></title-info>",
            "<document-info><author><nickname>maker</nickname></author></document-info>",
            "</description>\n{bodies}\n{binaries}</FictionBook>\n"
        ),
        bodies = bodies,
        binaries = binaries
    )
}

fn full_book() -> String {
    let words = "the road went on and on through the hills ".repeat(30);
    let bodies = format!(
        concat!(
            "<body><title><p>The Long Way</p><p>A Novel</p></title>",
            "<epigraph><p>All roads are long.</p><text-author>Somebody</text-author></epigraph>",
            "<section id=\"one\"><title><p>Chapter One</p></title>",
            "<p>It began with <emphasis>one</emphasis> step and <strong>two</strong> more",
            "<a l:href=\"#n1\" type=\"note\">[1]</a>.</p>",
            "<empty-line/><p>{words}</p>",
            "<image l:href=\"#pic.png\"/>",
            "<subtitle>* * *</subtitle>",
            "<poem><title><p>A Song</p></title><stanza><v>first line</v><v>second line</v></stanza></poem>",
            "<cite><p>a quotation</p><text-author>Quoted</text-author></cite>",
            "<table><tr><th>left</th><th>right</th></tr><tr><td>a</td><td>b</td></tr></table>",
            "<section id=\"one-a\"><title><p>Part One A</p></title><p>nested text</p></section>",
            "</section></body>",
            "<body name=\"notes\"><title><p>Notes</p></title>",
            "<section id=\"n1\"><title><p>1</p></title><p>The note itself.</p></section></body>"
        ),
        words = words
    );
    let binaries = format!(
        "<binary id=\"pic.png\" content-type=\"image/png\">{}</binary>",
        base64(&picture())
    );
    book(&bodies, &binaries)
}

/// **An FB2 is sniffed by its root and opens as its book**: every word of the
/// bodies on its pages, the description's title and first author as the
/// document's information, and nothing of the description, the binaries or
/// the stylesheet set as text.
#[test]
fn an_fb2_opens_as_its_book() {
    let source = full_book();
    assert_eq!(
        tinker_pdf::standalone::sniff(source.as_bytes()),
        Some(Standalone::Fb2)
    );
    let document = Document::open(source.into_bytes()).expect("the book opens");
    let words = text(&document);
    for expected in [
        "The Long Way",
        "A Novel",
        "All roads are long.",
        "Chapter One",
        "It began with one step and two more[1].",
        "first line",
        "second line",
        "a quotation",
        "left",
        "right",
        "Part One A",
        "nested text",
        "The note itself.",
    ] {
        assert!(words.contains(expected), "{expected:?} is not in {words:?}");
    }
    for absent in [
        "An annotation that is metadata.",
        "prose",
        "maker",
        "Second Hand",
        "iVBOR",
    ] {
        assert!(!words.contains(absent), "{absent:?} reached the page");
    }
    let metadata = document.metadata();
    assert_eq!(metadata.title.as_deref(), Some("The Long Way"));
    assert_eq!(metadata.author.as_deref(), Some("Anna K. Writer"));
    assert!(warnings(&document).is_empty(), "{:?}", warnings(&document));

    // The format's sheet is what sets it as a book: a title is centred, and an
    // epigraph is set from the middle of the measure. On a 432-point page with
    // a 36-point margin either starts well right of the margin, where a
    // paragraph starts at it.
    let left_of = |needle: &str| left_edge(&document, needle);
    assert!(left_of("Chapter One") > 150.0, "the title is not centred");
    assert!(
        left_of("All roads") > 150.0,
        "the epigraph is not set to the right"
    );
    assert!(
        left_of("It began") < 80.0,
        "a paragraph starts at the margin"
    );
}

/// Where the first line holding `needle` starts.
fn left_edge(document: &Document, needle: &str) -> f64 {
    (0..document.page_count())
        .flat_map(|at| {
            document
                .page(at)
                .expect("a page")
                .text()
                .lines()
                .iter()
                .filter(|line| line.text.contains(needle))
                .map(|line| line.quad.ll.0)
                .collect::<Vec<_>>()
        })
        .next()
        .unwrap_or_else(|| panic!("{needle:?} is on no line"))
}

/// **A book's own `<stylesheet>` wins over the format's**, and is not text: a
/// title the book sets flush left starts at the margin.
#[test]
fn a_books_own_stylesheet_wins_over_the_formats() {
    let source = full_book().replace(
        "<description>",
        "<stylesheet type=\"text/css\">.title { text-align: left }</stylesheet><description>",
    );
    let document = Document::open(source.into_bytes()).expect("opens");
    assert!(warnings(&document).is_empty(), "{:?}", warnings(&document));
    assert!(
        left_edge(&document, "Chapter One") < 80.0,
        "the book's sheet did not win"
    );
    assert!(!text(&document).contains("text-align"));
}

/// **An FB2 is the XHTML it translates to, pixel for pixel**: the document
/// `Document::open` makes against `from_html` handed the translation and the
/// format's sheet, at the same box. A book with no pictures, so the one thing
/// the two do not share — the provider of the `<binary>` elements — is not on
/// the page.
#[test]
fn an_fb2_is_the_xhtml_it_translates_to() {
    let source = book(
        "<body><title><p>Title</p></title><section><title><p>One</p></title>\
         <p>Some <emphasis>words</emphasis>.</p><poem><stanza><v>a verse</v></stanza></poem>\
         </section></body>",
        "",
    );
    let opened = drawn(Document::open(source.clone().into_bytes()).expect("opens"));
    let xhtml = tinker_pdf::fb2::to_xhtml(source.as_bytes()).expect("translates");
    let (built, _) = DocumentBuilder::from_html(
        xhtml,
        tinker_pdf::fb2::STYLESHEET,
        PageBox::new(432.0, 648.0),
    )
    .expect("lays out");
    let made = drawn(Document::open(built.finish()).expect("opens"));
    assert_eq!(opened.page_count(), made.page_count());
    for at in 0..opened.page_count() {
        let (a, b) = (render(&opened, at), render(&made, at));
        assert!(ink(&a) >= LEAST_INK, "page {at} drew no text to compare");
        assert!(a.data == b.data, "page {at} differs");
    }
}

/// **A picture is the book's own `<binary>`**: the cover and the block image
/// are each drawn, in the picture's colour, with nothing reported — the cover
/// on a book whose body has no picture, and the block image in one with no
/// cover, so neither can stand in for the other.
#[test]
fn a_picture_is_drawn_from_the_books_own_binary() {
    let blue = |bitmap: &Bitmap| {
        bitmap
            .data
            .chunks(bitmap.components())
            .any(|p| p.get(..3) == Some(&BLUE[..]))
    };
    let binary = format!(
        "<binary id=\"pic.png\" content-type=\"image/png\">{}</binary>",
        base64(&picture())
    );
    let covered = book(
        "<body><section><p>No picture in the body.</p></section></body>",
        &binary,
    )
    .replace(
        "<lang>en</lang>",
        "<coverpage><image l:href=\"#pic.png\"/></coverpage><lang>en</lang>",
    );
    let document = Document::open(covered.into_bytes()).expect("opens");
    assert!(warnings(&document).is_empty(), "{:?}", warnings(&document));
    assert!(
        blue(&render(&document, 0)),
        "the cover is not on the first page"
    );

    let document = Document::open(full_book().into_bytes()).expect("opens");
    assert!(warnings(&document).is_empty(), "{:?}", warnings(&document));
    assert!(
        (0..document.page_count()).any(|at| blue(&render(&document, at))),
        "the block picture is on no page"
    );
}

/// **A note reference is a link to the page the note is on.**
#[test]
fn a_note_reference_lands_on_its_note() {
    use tinker_pdf_cos::dest::{Action, Destination};

    let document = Document::open(full_book().into_bytes()).expect("opens");
    let mut targets = Vec::new();
    for at in 0..document.page_count() {
        for link in document.page(at).expect("a page").links() {
            if let Some(Action::GoTo(Destination::Explicit { page_index, .. })) = link.target {
                targets.push(page_index);
            }
        }
    }
    let note_page = (0..document.page_count())
        .find(|&at| {
            document
                .page(at)
                .expect("a page")
                .text()
                .plain_text()
                .contains("The note itself.")
        })
        .expect("the note is on a page");
    assert_eq!(targets, [Some(note_page)], "the one note link");
}

/// **What does not translate is named**: a binary whose base64 will not
/// decode, and the picture that then has nothing to draw; an element the
/// schema does not define, and one in another namespace that borrows an FB2
/// name, each of whose text is kept.
#[test]
fn what_does_not_translate_is_named() {
    let source = book(
        "<body><section><p>before <unheard-of>kept text</unheard-of> \
         <x:emphasis xmlns:x=\"urn:example:other\">foreign</x:emphasis> after</p>\
         <image l:href=\"#broken\"/></section></body>",
        "<binary id=\"broken\" content-type=\"image/png\">not base64 at all!</binary>",
    );
    let document = Document::open(source.into_bytes()).expect("opens");
    let found = warnings(&document);
    assert!(
        found.contains(&ArchiveWarning::Translation {
            item: String::new(),
            defect: TranslationDefect::UnknownElement,
            count: 2
        }),
        "{found:?}"
    );
    assert!(
        found.contains(&ArchiveWarning::Translation {
            item: String::new(),
            defect: TranslationDefect::BinaryUnreadable,
            count: 1
        }),
        "{found:?}"
    );
    assert!(
        found.contains(&ArchiveWarning::ImageNotDrawn {
            item: String::new(),
            defect: ImageDefect::Unresolved,
            images: 1
        }),
        "{found:?}"
    );
    assert!(text(&document).contains("before kept text foreign after"));
}

/// A Russian book, declared in `encoding`, written here as text.
fn cyrillic_book(encoding: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"{encoding}\"?>\n",
            "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">",
            "<description><title-info><author><first-name>Анна</first-name>",
            "<last-name>Писатель</last-name></author>",
            "<book-title>Долгая дорога</book-title><lang>ru</lang></title-info>",
            "</description><body><section><title><p>Глава первая</p></title>",
            "<p>Привет, мир. Ёлка и ёж ЖДУТ у ДОРОГИ.</p></section></body></FictionBook>"
        ),
        encoding = encoding
    )
}

/// `text` in windows-1251, by its code chart: А to я are 0xC0 to 0xFF in
/// order, Ё is 0xA8 and ё 0xB8.
fn windows_1251(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            c if c.is_ascii() => c as u8,
            'Ё' => 0xA8,
            'ё' => 0xB8,
            'А'..='я' => (c as u32 - 'А' as u32 + 0xC0) as u8,
            other => panic!("{other} is not in this fixture's repertoire"),
        })
        .collect()
}

/// `text` in KOI8-R, by its code chart: the lower case in the order of the
/// Latin letters they transliterate, from 0xC0, and the upper case 0x20 above.
fn koi8_r(text: &str) -> Vec<u8> {
    const ORDER: &str = "юабцдефгхийклмнопярстужвьызшэщчъ";
    text.chars()
        .map(|c| match c {
            c if c.is_ascii() => c as u8,
            'ё' => 0xA3,
            'Ё' => 0xB3,
            c => {
                let lower = c.to_lowercase().next().unwrap_or(c);
                let at = ORDER
                    .chars()
                    .position(|o| o == lower)
                    .unwrap_or_else(|| panic!("{c} is not in this fixture's repertoire"));
                (if lower == c { 0xC0 } else { 0xE0 }) + at as u8
            }
        })
        .collect()
}

/// **An FB2 in `windows-1251` or `koi8-r` is the book its UTF-8 twin is**
/// (tier 5's FB2 row): the same words on the same pages, the same title and
/// author, and nothing tolerated — where before the row it was an empty page.
/// The bytes are encoded here from each code chart, by hand, so the decoder
/// is held to the charts and not to itself.
#[test]
fn an_fb2_in_an_eight_bit_encoding_is_the_book_its_utf8_twin_is() {
    let twin = Document::open(cyrillic_book("utf-8").into_bytes()).expect("opens");
    let words = text(&twin);
    let drawn_twin = drawn(twin);
    assert!(
        words.contains("Привет, мир. Ёлка и ёж ЖДУТ у ДОРОГИ."),
        "{words}"
    );
    for (label, bytes) in [
        ("windows-1251", windows_1251(&cyrillic_book("windows-1251"))),
        ("cp1251", windows_1251(&cyrillic_book("cp1251"))),
        ("koi8-r", koi8_r(&cyrillic_book("koi8-r"))),
        ("KOI8-R", koi8_r(&cyrillic_book("KOI8-R"))),
    ] {
        assert_eq!(
            tinker_pdf::standalone::sniff(&bytes),
            Some(Standalone::Fb2),
            "{label}"
        );
        let document = drawn(Document::open(bytes).expect("opens"));
        assert_eq!(text(&document), words, "{label}");
        // The same as the twin's — a face covering no Cyrillic is named the
        // same way for both — and nothing about markup or translation.
        assert_eq!(warnings(&document), warnings(&drawn_twin), "{label}");
        assert!(
            !warnings(&document).iter().any(|w| matches!(
                w,
                ArchiveWarning::Markup { .. } | ArchiveWarning::Translation { .. }
            )),
            "{label}: {:?}",
            warnings(&document)
        );
        let metadata = document.metadata();
        assert_eq!(metadata.title.as_deref(), Some("Долгая дорога"), "{label}");
        assert_eq!(metadata.author.as_deref(), Some("Анна Писатель"), "{label}");
        let (page, twin_page) = (render(&document, 0), render(&drawn_twin, 0));
        assert!(
            page.data == twin_page.data,
            "{label}: the page is not its twin's"
        );
        assert!(ink(&page) > LEAST_INK, "{label}: nothing was drawn");
    }
}

/// **A byte the declared table leaves unmapped is U+FFFD and counted**, and a
/// **multi-byte encoding** — what is left of the row — still opens as an empty
/// page that says why rather than as text in the wrong letters.
#[test]
fn what_an_eight_bit_book_cannot_say_is_named() {
    // windows-1253 leaves 0xAA unmapped; the rest of the book is ASCII.
    let mut greek = full_book()
        .replace("encoding=\"utf-8\"", "encoding=\"windows-1253\"")
        .into_bytes();
    let at = greek
        .windows(9)
        .position(|w| w == b"nested te")
        .expect("the fixture has nested text");
    greek[at] = 0xAA;
    let document = Document::open(greek).expect("opens");
    assert!(
        warnings(&document).contains(&ArchiveWarning::Translation {
            item: String::new(),
            defect: TranslationDefect::UnmappedByte,
            count: 1
        }),
        "{:?}",
        warnings(&document)
    );
    assert!(text(&document).contains("\u{FFFD}ested text"));

    let source = full_book().replace("encoding=\"utf-8\"", "encoding=\"Shift_JIS\"");
    let document = Document::open(source.into_bytes()).expect("opens");
    assert_eq!(document.page_count(), 1);
    assert!(warnings(&document).contains(&ArchiveWarning::Markup {
        item: String::new(),
        defect: tinker_pdf::epub::xhtml::MarkupDefect::Truncated
    }));
    assert!(text(&document).is_empty());
}

/// **A book cut part way is read as far as it goes**, every element it had
/// opened closed, and the stop named.
#[test]
fn a_book_cut_part_way_is_read_as_far_as_it_goes() {
    let source = full_book();
    let cut = source
        .find("<poem>")
        .map(|at| &source[..at])
        .expect("the fixture has a poem");
    let xhtml = tinker_pdf::fb2::to_xhtml(cut.as_bytes()).expect("it begins");
    let dom = tinker_pdf::epub::read::markup(xhtml.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT);
    assert!(
        dom.defects.is_empty(),
        "the translation of a cut book is not XML: {:?}",
        dom.defects
    );
    let document = Document::open(cut.as_bytes().to_vec()).expect("opens");
    assert!(text(&document).contains("Chapter One"));
    assert!(warnings(&document).contains(&ArchiveWarning::Markup {
        item: String::new(),
        defect: tinker_pdf::epub::xhtml::MarkupDefect::Truncated
    }));
}

/// **An `.fb2.zip` opens as the book it holds**, decided by the bytes of its
/// one file — and a ZIP whose one file is a picture is still a comic.
#[test]
fn an_fb2_in_a_zip_of_one_file_is_the_book() {
    let source = full_book();
    let packed = zip(
        &[ZipFile::deflated("book.fb2", source.as_bytes())],
        Damage::None,
    );
    let unpacked = Document::open(source.into_bytes()).expect("opens");
    let document = Document::open(packed).expect("the archive opens");
    assert_eq!(document.page_count(), unpacked.page_count());
    assert_eq!(text(&document), text(&unpacked));
    assert_eq!(
        document.metadata().author.as_deref(),
        Some("Anna K. Writer")
    );

    let comic = zip(&[ZipFile::stored("book.fb2", &picture())], Damage::None);
    let opened = Document::open(comic).expect("the comic opens");
    assert_eq!(opened.page(0).expect("a page").size(), (24.0, 24.0));
}
