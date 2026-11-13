//! One reviewed golden per rendered surface the engine synthesises
//! (ROADMAP row K-08): appearances it draws for annotations and signatures,
//! pages it makes from other formats, the SVG it writes.
//!
//! `render_goldens.rs` holds one golden per operator family; this file holds
//! the surfaces by the same mechanism, `golden_support`'s — the header each
//! golden carries, its refusal of a field absent, blank or white space, the
//! byte-for-byte comparison and the writer — with a directory and ceilings of
//! its own. A surface is a whole synthesised page rather than one operator,
//! so its golden is larger than an operator's, and the operator families'
//! ceilings do not move for it. Every surface with text sets it in the test's
//! own face (`curvy_font`), never a face a build resolves by name, so one
//! golden serves the default and `bundled-fonts` builds and both run this file.
//!
//! A raster golden is a PPM with its header in `#` comment lines; an SVG
//! golden is the writer's output with its header in an XML comment after the
//! XML declaration, compared past that comment byte for byte.

mod written_support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tinker_pdf::ocr::{add_text_layer, import};
use tinker_pdf::{
    Bitmap, Date, DigestAlgorithm, Document, DocumentBuilder, ImageData, OpenOptions, Rect,
    RenderOptions, RenderWarning, SignRefused, SignatureAppearance, SignatureImage, Signer,
    SigningRequest, SigningTarget, SimpleFontProvider, SvgOptions, SvgWarning, WordBoxOptions,
    WriteMode, WriteOptions,
};
use tinker_pdf_cos::{CosDocument, Dict, DocumentEditor, FormXObject, ObjRef, StampPlacement};

mod cbz_support;
mod golden_support;
mod render_support;
use cbz_support::{rgb_png, zip, Damage, ZipFile};
use golden_support::{
    assert_small_enough, disagreement, goldens_in, ppm_mismatch, ppm_samples, read_header,
    read_svg_header, reviewer_and_date, svg_body, svg_mismatch, to_ppm, to_svg_golden, Golden,
    Header, HeaderError, UNREVIEWED_MARK,
};
use render_support::curvy_font;

/// The most bytes one surface golden may be: a page of about 145 points
/// square at 72 dpi. A reviewer reads the whole picture against its clause.
const PER_SURFACE: u64 = 64 * 1024;

/// The most bytes the surface goldens may be altogether.
const ALL_SURFACES: u64 = 512 * 1024;

/// A surface and the clause its golden draws.
struct Surface {
    /// The family, which is the golden's file name.
    name: &'static str,
    /// The clause the golden is read against.
    clause: &'static str,
    /// What the golden holds, made by this build.
    golden: Kind,
}

/// What a surface's golden holds.
enum Kind {
    /// A rendered page, held as a PPM.
    Raster {
        /// The surface, drawn by this build.
        draw: fn() -> Bitmap,
        /// The fewest non-background pixels it may paint: a blank golden
        /// matches a blank render forever.
        least_ink: usize,
    },
    /// The SVG a writer produces, held past its header comment.
    Svg {
        /// The surface, written by this build.
        write: fn() -> String,
    },
}

/// The surfaces, in the order their goldens are listed and reviewed.
///
/// Order-sensitive on purpose, as `render_goldens.rs`'s families are: a
/// surface dropped from the list is the failure this exists to make visible.
const SURFACES: &[Surface] = &[
    Surface {
        name: "annotation-appearances",
        clause: "ISO 32000-1 12.5.5, 12.5.6",
        golden: Kind::Raster {
            draw: annotation_appearances,
            // About half of what the page paints, 6 895 pixels.
            least_ink: 3400,
        },
    },
    Surface {
        name: "visible-signature",
        clause: "ISO 32000-1 12.7.4.5, 12.5.5",
        golden: Kind::Raster {
            draw: visible_signature,
            // About half of what the page paints, 2 867 pixels.
            least_ink: 1400,
        },
    },
    Surface {
        name: "epub-page",
        clause: "EPUB 3.3 3.2, 6.1, 6.3.1",
        golden: Kind::Raster {
            draw: epub_page,
            // About half of what the page paints, 1 306 pixels.
            least_ink: 650,
        },
    },
    Surface {
        name: "html-page",
        clause: "HTML 15.3.3, 15.3.4, 15.3.6, 15.3.8",
        golden: Kind::Raster {
            draw: html_page,
            // About half of what the page paints, 1 124 pixels.
            least_ink: 560,
        },
    },
    Surface {
        name: "markdown-page",
        clause: "CommonMark 0.31.2 4.2, 4.8, 5.2, 5.3, 6.2",
        golden: Kind::Raster {
            draw: markdown_page,
            // About half of what the page paints, 810 pixels.
            least_ink: 400,
        },
    },
    Surface {
        name: "fb2-page",
        clause: "FictionBook 2.1 section, title, epigraph, p, emphasis",
        golden: Kind::Raster {
            draw: fb2_page,
            // About half of what the page paints, 609 pixels.
            least_ink: 300,
        },
    },
    Surface {
        name: "svg-page",
        clause: "SVG 1.1 8.3, 9.2, 9.3, 10.4, 11.3, 11.4, 13.2.2",
        golden: Kind::Raster {
            draw: svg_page,
            // About half of what the page paints, 3 441 pixels.
            least_ink: 1700,
        },
    },
    Surface {
        name: "xps-page",
        clause: "ECMA-388 11.1, 12.1, 13.1, 13.2, 13.5",
        golden: Kind::Raster {
            draw: xps_page,
            // About half of what the page paints, 5 178 pixels.
            least_ink: 2580,
        },
    },
    Surface {
        name: "to-svg",
        clause: "SVG 1.1 5.2, 5.7, 8.3, 11.3, 11.4, 14.3, 14.5",
        golden: Kind::Svg { write: to_svg },
    },
    Surface {
        name: "ocr-layer-alignment",
        clause: "ISO 32000-2 9.3.4, 9.3.6, 9.3.7, 9.4.4; hOCR 1.2 4.2",
        golden: Kind::Raster {
            draw: ocr_layer_alignment,
            // Not measured: the floor stands under what the page's
            // construction draws at the least, nine stroked word boxes of at
            // least 32 points of perimeter each, their nine baselines and the
            // scan's 32 glyphs.
            least_ink: 400,
        },
    },
];

/// The surfaces no person has read yet: exactly those whose golden's header
/// reads `reviewer: unreviewed`. A surface leaves this list when a person has
/// read its golden against its clause and put their name in it (H-73).
const UNREVIEWED: &[&str] = &[
    "annotation-appearances",
    "visible-signature",
    "epub-page",
    "html-page",
    "markdown-page",
    "fb2-page",
    "svg-page",
    "xps-page",
    "to-svg",
    "ocr-layer-alignment",
];

fn surfaces_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("surfaces")
}

fn golden_path(surface: &Surface) -> PathBuf {
    let extension = match surface.golden {
        Kind::Raster { .. } => "ppm",
        Kind::Svg { .. } => "svg",
    };
    surfaces_dir().join(format!("{}.{extension}", surface.name))
}

/// Pixels that are not the white the page started as.
fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count()
}

// ---- the surfaces -----------------------------------------------------------

/// The annotation page's size in points, and so in pixels at 72 dpi.
const ANNOTATION_PAGE: (f64, f64) = (140.0, 120.0);

/// The subtypes appearance synthesis draws from a design this repository
/// states, each as `DocumentEditor::add_annotation` is handed it with no
/// `/AP`: a line with an ending at each point, a polyline with its own two,
/// the three shapes, an ink path, the four text markups and a free text box,
/// laid out apart so that each is read on its own.
const ANNOTATIONS: [&str; 11] = [
    "<< /Type /Annot /Subtype /Line /Rect [0 0 140 120] /L [10 105 60 105] \
     /BS << /W 2 >> /C [0.8 0.1 0.1] /IC [0.1 0.3 0.9] /LE [/ClosedArrow /Square] >>",
    "<< /Type /Annot /Subtype /PolyLine /Rect [0 0 140 120] \
     /Vertices [75 95 95 112 115 95 132 110] /BS << /W 1.5 >> /C [0.1 0.5 0.2] \
     /IC [0.95 0.8 0.3] /LE [/Circle /OpenArrow] >>",
    "<< /Type /Annot /Subtype /Square /Rect [8 58 38 84] /BS << /W 2 >> \
     /C [0.8 0.1 0.1] /IC [0.95 0.8 0.3] >>",
    "<< /Type /Annot /Subtype /Circle /Rect [44 58 74 84] /BS << /W 2 >> \
     /C [0.1 0.3 0.9] /IC [0.7 0.9 0.7] >>",
    "<< /Type /Annot /Subtype /Polygon /Rect [0 0 140 120] \
     /Vertices [80 58 108 58 94 84] /BS << /W 1.5 >> /C [0.5 0.1 0.6] /IC [0.9 0.8 0.95] >>",
    "<< /Type /Annot /Subtype /Ink /Rect [0 0 140 120] /BS << /W 2 >> /C [0.2 0.2 0.2] \
     /InkList [[114 60 120 82 126 62 132 84]] >>",
    "<< /Type /Annot /Subtype /Highlight /Rect [0 0 140 120] /C [1 0.85 0.2] \
     /QuadPoints [8 50 38 50 8 38 38 38] >>",
    "<< /Type /Annot /Subtype /Underline /Rect [0 0 140 120] /C [0.1 0.3 0.9] \
     /QuadPoints [42 50 72 50 42 38 72 38] >>",
    "<< /Type /Annot /Subtype /StrikeOut /Rect [0 0 140 120] /C [0.8 0.1 0.1] \
     /QuadPoints [76 50 106 50 76 38 106 38] >>",
    "<< /Type /Annot /Subtype /Squiggly /Rect [0 0 140 120] /C [0.1 0.5 0.2] \
     /QuadPoints [110 50 134 50 110 38 134 38] >>",
    "<< /Type /Annot /Subtype /FreeText /Rect [8 4 132 32] /C [0.9 0.95 1] \
     /BS << /W 1 >> /DA (/Cv 9 Tf 0.1 0.1 0.4 rg) /Contents (Free text in the curvy face) >>",
];

/// `curvy_font` as a simple TrueType font, its descriptor and its program,
/// objects 4, 5 and 6: the face the form's `/DR` names `/Cv`, which the free
/// text annotation's `/DA` selects, so its text is drawn from an embedded
/// program in every build.
fn curvy_objects() -> Vec<Vec<u8>> {
    let program = curvy_font();
    let widths: Vec<&str> = (32..=126)
        .map(|code| if code == 32 { "320" } else { "640" })
        .collect();
    let font = format!(
        "<< /Type /Font /Subtype /TrueType /BaseFont /Curvy /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{}] /FontDescriptor 5 0 R >>",
        widths.join(" ")
    );
    let descriptor = "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 \
                      /FontBBox [-500 -300 1500 1000] /ItalicAngle 0 /Ascent 750 \
                      /Descent -250 /CapHeight 700 /StemV 80 /FontFile2 6 0 R >>";
    let mut file = format!("<< /Length {0} /Length1 {0} >>\nstream\n", program.len()).into_bytes();
    file.extend_from_slice(&program);
    file.extend_from_slice(b"\nendstream");
    vec![font.into_bytes(), descriptor.as_bytes().to_vec(), file]
}

/// A one-page file: the catalog, whose form's `/DR` holds the face, the
/// page, the face's three objects, and each of [`ANNOTATIONS`] as an object
/// of its own from object 7, on no page's `/Annots`.
fn annotation_fixture() -> Vec<u8> {
    let (width, height) = ANNOTATION_PAGE;
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R \
          /AcroForm << /Fields [] /DR << /Font << /Cv 4 0 R >> >> >> >>"
            .to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << >> >>"
        )
        .into_bytes(),
    ];
    bodies.extend(curvy_objects());
    bodies.extend(ANNOTATIONS.iter().map(|a| a.as_bytes().to_vec()));
    pdf_file(&bodies)
}

/// A file of `bodies` as objects 1 onwards, object 1 its catalog, with a
/// cross-reference table.
fn pdf_file(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    let size = bodies.len() + 1;
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// A stream object of `data` under the entries `dict`.
fn stream_object(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 12.5.5 and 12.5.6: every annotation of [`ANNOTATIONS`] added through the
/// editor, which synthesises its appearance, the file saved and opened again
/// and its page rendered with its annotations.
fn annotation_appearances() -> Bitmap {
    let doc = Arc::new(CosDocument::open(annotation_fixture()).expect("the fixture opens"));
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let ap = doc.intern(b"AP");
    for num in (7u32..).take(ANNOTATIONS.len()) {
        let dict = doc
            .get(ObjRef { num, gen: 0 })
            .ok()
            .and_then(|object| object.as_dict().cloned())
            .expect("each annotation is a dictionary");
        assert!(!dict.contains_key(ap), "the fixture carries no /AP");
        editor
            .add_annotation(0, dict)
            .expect("the annotation is added");
    }
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    written_support::rewritten("annotation appearances", doc.bytes(), &saved);
    let document = converted(
        "annotation appearances",
        Document::open(saved).expect("the saved file opens"),
    );
    let page = document.page(0).expect("the page");
    let annotations = page.annotations();
    assert_eq!(
        annotations.len(),
        ANNOTATIONS.len(),
        "every annotation is on the page"
    );
    assert!(
        annotations.iter().all(|a| a.has_appearance),
        "and each was given an appearance"
    );
    page.render(&RenderOptions::default())
}

/// The signature page's size in points.
const SIGNATURE_PAGE: (f64, f64) = (160.0, 70.0);

/// Where the seal goes on it.
const SEAL: Rect = Rect {
    x0: 8.0,
    y0: 8.0,
    x1: 152.0,
    y1: 62.0,
};

/// A signer whose signature is no signature: the seal is what this golden
/// draws, and it is the same whatever `/Contents` holds.
struct Unsigned;

impl Signer for Unsigned {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        Ok(Vec::new())
    }
}

/// A 24 by 16 greyscale picture: a dark disc on a mid-grey ground.
fn seal_picture() -> SignatureImage {
    let (width, height) = (24u32, 16u32);
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (f64::from(x) - 11.5, f64::from(y) - 7.5);
            data.push(if dx * dx + dy * dy < 36.0 { 20 } else { 140 });
        }
    }
    SignatureImage::Gray8 {
        width,
        height,
        data,
    }
}

/// 12.7.4.5 with 12.5.5: a new visible signature field's seal — a picture,
/// and the request's who, when, why and where — on a blank page, rendered
/// with its widget.
///
/// The seal names its text's font Helvetica, a standard 14 name a build with
/// no faces cannot draw and a `bundled-fonts` build draws in a face of its
/// own; the document is opened with the test's own face as its provider,
/// which every build asks before any face it bundles, so both builds draw the
/// seal's text from one program.
fn visible_signature() -> Bitmap {
    let mut builder = tinker_pdf::DocumentBuilder::new();
    builder.add_page(SIGNATURE_PAGE.0, SIGNATURE_PAGE.1, |_| {});
    let blank = converted(
        "visible signature",
        Document::open(built("visible signature", builder)).expect("the blank page opens"),
    );

    let signer = Unsigned;
    let mut request = SigningRequest::new(
        SigningTarget::NewVisibleField {
            name: "Seal".to_owned(),
            page: 0,
            rect: SEAL,
            appearance: SignatureAppearance::with_image(seal_picture()),
        },
        &signer,
    );
    request.reserve = 16;
    request.name = Some("Ada Lovelace".to_owned());
    request.reason = Some("I approve this page".to_owned());
    request.location = Some("London".to_owned());
    request.signed_at = Some(Date {
        year: 2026,
        month: 9,
        day: 26,
        hour: 10,
        minute: 30,
        second: 0,
        utc_offset_minutes: Some(60),
    });
    let signed = blank
        .editor()
        .save_signed(
            &WriteOptions {
                mode: WriteMode::Incremental,
                ..WriteOptions::default()
            },
            &request,
        )
        .expect("the seal is drawn and the file signed");
    written_support::rewritten("visible signature", blank.cos().bytes(), &signed);

    let face = Arc::new(SimpleFontProvider::new(curvy_font()));
    converted(
        "visible signature",
        Document::open(signed).expect("the signed file opens"),
    )
    .with_fonts(face)
    .page(0)
    .expect("the page")
    .render(&RenderOptions::default())
}

// ---- pages made from other formats --------------------------------------------

/// The page box the four reflowable samples are laid out into, in points. The
/// reading system's margin of 36 points leaves the text a square of 72.
const BOOK_PAGE: (f64, f64) = (144.0, 144.0);

/// The base size, in points, the reflowable samples are set at, so that what
/// each one shows fits its square.
const BOOK_FONT_SIZE: f64 = 7.0;

/// How a sample is opened: in the book box at the base size, which only a
/// reflowable one is laid out into, and — for a sample whose text names a face
/// it does not embed — with the test's own face as the provider, which every
/// build asks before any face it bundles. The sample text is ASCII, which the
/// standard faces' built-in metrics set alike in every build, so only the
/// glyphs are the provider's.
fn book_options(provider: bool) -> OpenOptions {
    let mut options = OpenOptions::at_page(BOOK_PAGE.0, BOOK_PAGE.1);
    options.font_size = BOOK_FONT_SIZE;
    if provider {
        options = options.with_fonts(Arc::new(SimpleFontProvider::new(curvy_font())));
    }
    options
}

/// The one page of a synthesised document, rendered, after checking that the
/// sample is what it is meant to be: a document of its format that fits one
/// page, with nothing in it a reader is told was dropped.
fn only_page(name: &str, document: &Document) -> Bitmap {
    assert_eq!(document.page_count(), 1, "{name}: the sample fits one page");
    let report = document
        .archive()
        .expect("a synthesised document has a report");
    assert!(
        report.warnings().is_empty(),
        "{name}: the sample is not clean: {:?}",
        report.warnings()
    );
    document
        .page(0)
        .expect("the page")
        .render(&RenderOptions::default())
}

/// OCF 3.3's container document, naming the one package.
const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/package.opf" "#,
    r#"media-type="application/oebps-package+xml"/></rootfiles></container>"#
);

/// A book of one chapter whose every character is set in its own embedded
/// face: the package, a navigation document, the chapter and `curvy_font`
/// as `font/ttf`, a core media type (3.2), named by the chapter's
/// `@font-face`.
fn epub_sample() -> Vec<u8> {
    let package = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"#,
        r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:identifier id="id">urn:uuid:5a1d0c62-8d0e-4c1b-9f4a-0b7c3e2d1a90</dc:identifier>"#,
        r#"<dc:title>A Sample Book</dc:title><dc:language>en</dc:language>"#,
        r#"<meta property="dcterms:modified">2026-11-13T00:00:00Z</meta></metadata>"#,
        r#"<manifest>"#,
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
        r#"<item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<item id="face" href="fonts/curvy.ttf" media-type="font/ttf"/>"#,
        r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
    );
    let nav = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"#,
        r#"<head><title>Contents</title></head><body><nav epub:type="toc"><ol>"#,
        r#"<li><a href="chapter.xhtml">Chapter One</a></li></ol></nav></body></html>"#
    );
    let chapter = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en" lang="en"><head>"#,
        r#"<title>Chapter One</title><style>"#,
        r#"@font-face { font-family: "Curvy"; src: url(fonts/curvy.ttf) format("truetype"); } "#,
        r#"body { margin: 0; font-family: "Curvy"; } "#,
        r#"h1 { font-size: 1.6em; margin: 0 0 0.5em 0; color: #1f3a93; } "#,
        r#"p { margin: 0; text-indent: 1em; text-align: justify; }"#,
        r#"</style></head><body><h1>Chapter 1</h1>"#,
        r#"<p>The road ran on through the hills, and the rain came down.</p>"#,
        r#"<p>Then it stopped.</p></body></html>"#
    );
    let files = [
        ZipFile::stored("mimetype", b"application/epub+zip"),
        ZipFile::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        ZipFile::deflated("EPUB/package.opf", package.as_bytes()),
        ZipFile::deflated("EPUB/nav.xhtml", nav.as_bytes()),
        ZipFile::deflated("EPUB/chapter.xhtml", chapter.as_bytes()),
        ZipFile::deflated("EPUB/fonts/curvy.ttf", &curvy_font()),
    ];
    zip(&files, Damage::None)
}

/// EPUB 3.3 6.1 and 6.3.1: the chapter, an XHTML content document, laid out
/// with its style sheet into the book box, its text in the face the book
/// embeds (3.2) and no other.
fn epub_page() -> Bitmap {
    let document = converted(
        "epub page",
        Document::open_with(epub_sample(), &book_options(false)).expect("the book opens"),
    );
    only_page("epub-page", &document)
}

/// HTML 15.3: a loose HTML file of a heading, a paragraph with bold and
/// italic words and a row of bordered table cells, laid out with the
/// user-agent sheet HTML's rendering section states and a sheet of its own.
fn html_page() -> Bitmap {
    let html = concat!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><title>A Sample Page</title><style>",
        "h1 { font-size: 1.5em; color: #8a1c1c } ",
        "td { border: 1px solid #4a4a4a; background: #e8eef8 }",
        "</style></head><body><h1>A heading</h1>",
        "<p>Some <b>bold</b> and <i>italic</i> words.</p>",
        "<table><tr><td>one</td><td>two</td><td>three</td></tr></table>",
        "</body></html>\n"
    );
    let document = converted(
        "html page",
        Document::open_with(html.as_bytes().to_vec(), &book_options(true)).expect("the page opens"),
    );
    only_page("html-page", &document)
}

/// CommonMark 0.31.2: an ATX heading (4.2), a paragraph (4.8) with emphasis
/// and strong emphasis (6.2), and an ordered list (5.3) of two items (5.2),
/// laid out as the XHTML the reader translates them to.
fn markdown_page() -> Bitmap {
    let text = "## A heading\n\nSome *emphasis* and **strong** words.\n\n1. One\n2. Two\n";
    let document = converted(
        "markdown page",
        Document::open_markdown(text.as_bytes().to_vec(), &book_options(true))
            .expect("the text opens"),
    );
    only_page("markdown-page", &document)
}

/// FictionBook 2.1: a section with its title, an epigraph and a paragraph
/// with an emphasis, set by the reading system's sheet for the format.
fn fb2_page() -> Bitmap {
    let fb2 = concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
        "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">",
        "<description><title-info><genre>prose</genre>",
        "<author><first-name>Anna</first-name><last-name>Writer</last-name></author>",
        "<book-title>A Sample</book-title><lang>en</lang></title-info></description>",
        "<body><section><title><p>One</p></title>",
        "<epigraph><p>All roads are long.</p></epigraph>",
        "<p>It began with <emphasis>one</emphasis> step.</p>",
        "</section></body></FictionBook>\n"
    );
    let document = converted(
        "fb2 page",
        Document::open_with(fb2.as_bytes().to_vec(), &book_options(true)).expect("the book opens"),
    );
    only_page("fb2-page", &document)
}

/// The SVG sample's page, in points: the document's own `width` and `height`
/// of 160 by 128 CSS pixels, which a standalone SVG's page is, its `viewBox`
/// making one user unit a point.
const SVG_PAGE: (f64, f64) = (120.0, 96.0);

/// SVG 1.1: a rounded `rect` (9.2) filled with a linear gradient (13.2.2), a
/// `circle` (9.3) filled and stroked (11.3, 11.4), a dashed cubic `path`
/// (8.3) and a line of `text` (10.4), on a page the document's own size.
fn svg_page() -> Bitmap {
    let svg = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="128" viewBox="0 0 120 96">"#,
        r#"<defs><linearGradient id="ramp" x1="0" y1="0" x2="1" y2="0">"#,
        r##"<stop offset="0" stop-color="#1f77b4"/><stop offset="1" stop-color="#ff7f0e"/>"##,
        r#"</linearGradient></defs>"#,
        r#"<rect x="6" y="6" width="54" height="34" rx="6" fill="url(#ramp)"/>"#,
        r##"<circle cx="88" cy="24" r="16" fill="#ffd54f" stroke="#6d4c41" stroke-width="3"/>"##,
        r##"<path d="M 8 88 C 30 54 62 104 112 64" fill="none" stroke="#2e7d32" "##,
        r#"stroke-width="2.5" stroke-dasharray="6 3"/>"#,
        r##"<text x="8" y="60" font-family="serif" font-size="12" fill="#222222">SVG text</text>"##,
        r#"</svg>"#
    );
    let document = converted(
        "svg page",
        Document::open_with(svg.as_bytes().to_vec(), &book_options(true))
            .expect("the picture opens"),
    );
    let page = document.page(0).expect("page one");
    assert_eq!(page.size(), SVG_PAGE, "the page is the document's own size");
    only_page("svg-page", &document)
}

/// ECMA-388's 1.0 namespace, which every fixed-payload part here is in.
const XPS_NS: &str = "http://schemas.microsoft.com/xps/2005/06";

/// A package of one fixed page whose text is set in its own font part: a
/// `Path` filled with a solid colour and stroked (11.1, 13.1), one filled with
/// a linear gradient (13.5), one filled with a picture (13.2) and a `Glyphs`
/// run (12.1) in `curvy_font`, a plain OpenType part (9.1.7).
fn xps_sample() -> Vec<u8> {
    let picture: Vec<u8> = (0..8u32)
        .flat_map(|y| {
            (0..8u32).flat_map(move |x| {
                if (x + y) % 2 == 0 {
                    [40u8, 40, 160]
                } else {
                    [240u8, 220, 120]
                }
            })
        })
        .collect();
    let page = format!(
        concat!(
            r#"<FixedPage xmlns="{ns}" Width="160" Height="128" xml:lang="en-US">"#,
            r##"<Path Data="M 8,8 L 72,8 L 72,56 L 8,56 Z" Fill="#FF3366CC" "##,
            r##"Stroke="#FF1A1A1A" StrokeThickness="3"/>"##,
            r#"<Path Data="M 84,8 L 152,8 L 152,56 L 84,56 Z"><Path.Fill>"#,
            r#"<LinearGradientBrush MappingMode="Absolute" StartPoint="84,8" EndPoint="152,56">"#,
            r#"<LinearGradientBrush.GradientStops>"#,
            r##"<GradientStop Color="#FFFFC107" Offset="0"/><GradientStop Color="#FF2E7D32" Offset="1"/>"##,
            r#"</LinearGradientBrush.GradientStops></LinearGradientBrush></Path.Fill></Path>"#,
            r#"<Path Data="M 8,68 L 48,68 L 48,108 L 8,108 Z"><Path.Fill>"#,
            r#"<ImageBrush ImageSource="/Resources/picture.png" Viewbox="0,0,8,8" "#,
            r#"ViewboxUnits="Absolute" Viewport="8,68,40,40" ViewportUnits="Absolute"/>"#,
            r#"</Path.Fill></Path>"#,
            r##"<Glyphs OriginX="60" OriginY="98" FontRenderingEmSize="18" "##,
            r##"FontUri="/Resources/curvy.ttf" UnicodeString="XPS text" Fill="#FF202020"/>"##,
            r#"</FixedPage>"#
        ),
        ns = XPS_NS
    );
    let types = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml"/>"#,
        r#"<Default Extension="fdoc" ContentType="application/vnd.ms-package.xps-fixeddocument+xml"/>"#,
        r#"<Default Extension="fpage" ContentType="application/vnd.ms-package.xps-fixedpage+xml"/>"#,
        r#"<Default Extension="png" ContentType="image/png"/>"#,
        r#"<Default Extension="ttf" ContentType="application/vnd.ms-opentype"/>"#,
        r#"</Types>"#
    );
    let rels = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Type="{ns}/fixedrepresentation" Target="/Sequence.fdseq" Id="R0"/>"#,
            r#"</Relationships>"#
        ),
        ns = XPS_NS
    );
    let page_rels = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Type="http://schemas.microsoft.com/xps/2005/06/required-resource" "#,
        r#"Target="/Resources/curvy.ttf" Id="R1"/>"#,
        r#"<Relationship Type="http://schemas.microsoft.com/xps/2005/06/required-resource" "#,
        r#"Target="/Resources/picture.png" Id="R2"/>"#,
        r#"</Relationships>"#
    );
    let sequence = format!(
        r#"<FixedDocumentSequence xmlns="{XPS_NS}"><DocumentReference Source="/Document.fdoc"/></FixedDocumentSequence>"#
    );
    let document = format!(
        r#"<FixedDocument xmlns="{XPS_NS}"><PageContent Source="/Pages/1.fpage"/></FixedDocument>"#
    );
    let files = [
        ZipFile::deflated("_rels/.rels", rels.as_bytes()),
        ZipFile::deflated("Sequence.fdseq", sequence.as_bytes()),
        ZipFile::deflated("Document.fdoc", document.as_bytes()),
        ZipFile::deflated("Pages/1.fpage", page.as_bytes()),
        ZipFile::deflated("Pages/_rels/1.fpage.rels", page_rels.as_bytes()),
        ZipFile::deflated("Resources/curvy.ttf", &curvy_font()),
        ZipFile::deflated("Resources/picture.png", &rgb_png(8, 8, &picture)),
        ZipFile::deflated("[Content_Types].xml", types.as_bytes()),
    ];
    zip(&files, Damage::None)
}

/// ECMA-388: the package's one fixed page, its text drawn from the package's
/// own font part.
fn xps_page() -> Bitmap {
    let document = converted(
        "xps page",
        Document::open(xps_sample()).expect("the package opens"),
    );
    only_page("xps-page", &document)
}

// ---- the SVG the writer produces ----------------------------------------------

/// The `to_svg` page's size in points.
const TO_SVG_PAGE: (u32, u32) = (120, 90);

/// A one-page file of everything `Page::to_svg` maps: a filled path and a
/// stroked curve, a line of text in `curvy_font` embedded as objects 4 to 6,
/// an image (object 8), a fill under a clip, a transparency group drawn at
/// half opacity (object 9) and a fill under a luminosity soft mask whose
/// group is object 10.
fn to_svg_fixture() -> Vec<u8> {
    let (width, height) = TO_SVG_PAGE;
    let content = "0.15 0.35 0.75 rg 8 46 40 34 re f\n\
                   0.8 0.2 0.1 RG 2 w 1 J 8 8 m 30 44 60 0 112 28 c S\n\
                   BT /Cv 16 Tf 0.1 0.1 0.1 rg 56 66 Td (Svg) Tj ET\n\
                   q 22 0 0 22 92 58 cm /Im Do Q\n\
                   q 56 30 26 26 re W n 0.2 0.6 0.3 rg 50 24 40 40 re f Q\n\
                   q /Half gs /Grp Do Q\n\
                   q /Soft gs 0.95 0.6 0.1 rg 92 30 20 20 re f Q";
    // Four by four pixels of four colours, a quadrant each.
    let samples: Vec<u8> = (0..4u32)
        .flat_map(|y| {
            (0..4u32).flat_map(move |x| match (x < 2, y < 2) {
                (true, true) => [200u8, 30, 30],
                (false, true) => [30, 160, 60],
                (true, false) => [40, 60, 200],
                (false, false) => [240, 200, 40],
            })
        })
        .collect();
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents 7 0 R \
             /Resources << /Font << /Cv 4 0 R >> /XObject << /Im 8 0 R /Grp 9 0 R >> \
             /ExtGState << /Half << /ca 0.5 /CA 0.5 >> \
             /Soft << /SMask << /S /Luminosity /G 10 0 R >> >> >> >> >>"
        )
        .into_bytes(),
    ];
    bodies.extend(curvy_objects());
    bodies.push(stream_object("", content.as_bytes()));
    bodies.push(stream_object(
        "/Type /XObject /Subtype /Image /Width 4 /Height 4 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8",
        &samples,
    ));
    bodies.push(stream_object(
        "/Type /XObject /Subtype /Form /BBox [0 0 120 90] /Group << /S /Transparency >> \
         /Resources << >>",
        b"0.55 0.1 0.6 rg 60 4 44 20 re f",
    ));
    bodies.push(stream_object(
        "/Type /XObject /Subtype /Form /BBox [0 0 120 90] \
         /Group << /S /Transparency /CS /DeviceGray >> /Resources << >>",
        b"0.5 g 0 0 120 90 re f",
    ));
    pdf_file(&bodies)
}

/// SVG 1.1 as `Page::to_svg` writes a page onto it: paths and the text's
/// glyph outlines as path data (8.3) with their fill and stroke properties
/// (11.3, 11.4), the image as an `image` (5.7), the clip as a `clipPath`
/// (14.3) and the group as a `g` (5.2) at the group's opacity (14.5). The
/// soft mask is the one thing the page asks for that the writer declines, so
/// what it masks is written unmasked and named, and nothing else is.
fn to_svg() -> String {
    let document = converted(
        "to svg",
        Document::open(to_svg_fixture()).expect("the fixture opens"),
    );
    let svg = document
        .page(0)
        .expect("the page")
        .to_svg(&SvgOptions::default());
    assert_eq!(
        svg.warnings,
        vec![SvgWarning::SoftMaskRefused {
            group: Some(ObjRef { num: 10, gen: 0 }),
        }],
        "the soft mask is the one thing the writer declines"
    );
    svg.markup
}

// ---- an OCR text layer, drawn so that it shows ------------------------------

/// The scanned page's size in points.
const OCR_PAGE: (f64, f64) = (160.0, 120.0);

/// The scan's resolution, twice the page's: a recogniser's boxes are in the
/// scan's pixels, and the layer scales them onto the page.
const SCAN_DPI: f64 = 144.0;

/// What the scan shows: each line's left edge, baseline and size in points,
/// and its text, set in the test's own face.
const SCANNED: [(f64, f64, f64, &str); 3] = [
    (12.0, 92.0, 14.0, "Scanned text"),
    (12.0, 64.0, 10.0, "set in a face"),
    (12.0, 40.0, 10.0, "and read back"),
];

/// A scanned page with the text layer OC-01's import seam writes over it,
/// drawn so that it shows: each word's box as text extraction reads it off
/// the layer, stroked in red, and its baseline, a fifth of the box up, in
/// blue — so a reviewer sees the layer sit on the words of the scan.
///
/// The scan is a page of text in `curvy_font` rendered at [`SCAN_DPI`] and
/// placed as a grey image; the recogniser's file is hOCR stating each word's
/// box in the scan's pixels, rounded outward to whole ones, as FT-24's word
/// boxes find them on the page the scan was made from; the layer is written
/// with [`add_text_layer`] onto the whole page, and the boxes drawn are read
/// back from the layer alone, the scan having no text.
fn ocr_layer_alignment() -> Bitmap {
    // The words, in the test's own face.
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    builder.add_page(OCR_PAGE.0, OCR_PAGE.1, |page| {
        for (x, y, size, text) in SCANNED {
            page.text(b"F0", size, x, y, text);
        }
    });
    let source = Document::open(builder.finish()).expect("the page of text opens");
    let printed = source.page(0).expect("its page");

    // The scan: that page at twice the resolution, in grey.
    let scan = printed.render(&RenderOptions::at_dpi(SCAN_DPI));
    let grey: Vec<u8> = scan
        .data
        .chunks_exact(scan.components())
        .map(|pixel| {
            let sum = u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]);
            (sum / 3) as u8
        })
        .collect();

    // The recogniser's file: each word's box in the scan's pixels.
    let mut body = String::new();
    let found = printed.word_boxes(&WordBoxOptions::at_dpi(SCAN_DPI));
    let mut words = 0usize;
    for line in found
        .blocks
        .iter()
        .flat_map(|b| &b.paragraphs)
        .flat_map(|p| &p.lines)
    {
        body.push_str("<span class=\"ocr_line\">");
        for word in &line.words {
            let (x0, y0, x1, y1) = word.quad.bounds();
            body.push_str(&format!(
                "<span class=\"ocrx_word\" title=\"bbox {} {} {} {}\">{}</span> ",
                x0.floor(),
                y0.floor(),
                x1.ceil(),
                y1.ceil(),
                word.text
            ));
            words += 1;
        }
        body.push_str("</span>\n");
    }
    assert_eq!(words, 9, "the scan's nine words are found");
    let hocr = format!(
        "<!DOCTYPE html>\n<html><head><title>scan</title></head><body>\
         <div class=\"ocr_page\" title=\"bbox 0 0 {} {}; scan_res 144 144\">\n{body}</div>\
         </body></html>\n",
        scan.width, scan.height
    );
    let imported = import(hocr.as_bytes())
        .expect("the file imports")
        .pages
        .remove(0);

    // The scanned page: the scan filling a page with no text of its own.
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Scan",
        &ImageData::Gray8 {
            width: scan.width,
            height: scan.height,
            data: &grey,
        },
    ));
    builder.add_page(OCR_PAGE.0, OCR_PAGE.1, |page| {
        page.image(b"Scan", 0.0, 0.0, OCR_PAGE.0, OCR_PAGE.1);
    });
    let scanned = Document::open(builder.finish()).expect("the scanned page opens");

    // The layer, written over it.
    let mut editor = scanned.editor();
    let report = add_text_layer(&mut editor, 0, &imported, None).expect("the layer is written");
    assert_eq!(report.drawn, words, "every word is drawn");
    let layered = Document::open(editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }))
    .expect("the layered page opens");

    // The layer made visible: each word's box as extraction reads it, and its
    // baseline.
    let read = layered
        .page(0)
        .expect("the page")
        .word_boxes(&WordBoxOptions::default());
    let mut boxes = String::from("0.85 0.1 0.1 RG 0.6 w\n");
    let mut baselines = String::from("0.1 0.3 0.9 RG 0.6 w\n");
    for word in read
        .blocks
        .iter()
        .flat_map(|b| &b.paragraphs)
        .flat_map(|p| &p.lines)
        .flat_map(|l| &l.words)
    {
        // The frame at 72 dpi is the page in points, `y` down.
        let (left, top, right, bottom) = word.quad.bounds();
        let (low, high) = (OCR_PAGE.1 - bottom, OCR_PAGE.1 - top);
        boxes.push_str(&format!(
            "{left:.3} {low:.3} {:.3} {:.3} re S\n",
            right - left,
            high - low
        ));
        let baseline = low + (high - low) / 5.0;
        baselines.push_str(&format!(
            "{left:.3} {baseline:.3} m {right:.3} {baseline:.3} l S\n"
        ));
    }
    let overlay = format!("{boxes}{baselines}");
    let mut editor = layered.editor();
    let form = editor
        .add_form(
            &FormXObject {
                bbox: [0.0, 0.0, OCR_PAGE.0, OCR_PAGE.1],
                matrix: None,
                group: None,
                content: overlay.as_bytes(),
            },
            Dict::new(),
        )
        .expect("the overlay's form");
    editor
        .stamp(0, form, StampPlacement::Over)
        .expect("the overlay is stamped");
    Document::open(editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }))
    .expect("the drawn page opens")
    .page(0)
    .expect("the page")
    .render(&RenderOptions::default())
}

// ---- the checks -------------------------------------------------------------

/// Every surface golden is what this build makes, byte for byte: a raster
/// above the surface's ink floor and with every glyph it names drawn, an SVG
/// past its header comment.
#[test]
fn every_surface_golden_is_what_this_build_draws() {
    let mut wrong = Vec::new();
    for surface in SURFACES {
        match surface.golden {
            Kind::Raster { draw, least_ink } => {
                let bitmap = draw();
                let drawn = ink(&bitmap);
                assert!(
                    drawn >= least_ink,
                    "{}: the page painted {drawn} pixels, fewer than the {least_ink} it \
                     is supposed to -- a blank golden matches a blank render forever",
                    surface.name
                );
                assert!(
                    !bitmap.warnings.contains(&RenderWarning::UnreadableFont),
                    "{}: a font this build cannot draw, so its glyphs are missing",
                    surface.name
                );
                let committed = std::fs::read(golden_path(surface)).expect("the golden reads");
                wrong.extend(ppm_mismatch(surface.name, &committed, &bitmap));
            }
            Kind::Svg { write } => {
                let committed =
                    std::fs::read_to_string(golden_path(surface)).expect("the golden reads");
                wrong.extend(svg_mismatch(surface.name, &committed, &write()));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", disagreement(&wrong));
}

/// One byte changed in a surface golden is a golden this build does not make:
/// every byte is compared, and no tolerance stands between a golden and the
/// picture or the markup.
#[test]
fn one_changed_byte_in_a_surface_golden_fails_it() {
    for surface in SURFACES {
        let mut committed = std::fs::read(golden_path(surface)).expect("the golden reads");
        let still_matched = match surface.golden {
            Kind::Raster { draw, .. } => {
                // The middle pixel's first component, one level away.
                let samples = ppm_samples(&committed).len();
                let at = committed.len() - samples + samples / 6 * 3;
                committed[at] ^= 1;
                ppm_mismatch(surface.name, &committed, &draw()).is_none()
            }
            Kind::Svg { write } => {
                // The first letter or digit past the middle of the markup
                // after the header comment, one bit away: an ASCII byte stays
                // one, so the golden is still UTF-8 and only that byte moves.
                let text = String::from_utf8(committed.clone()).expect("the golden is UTF-8");
                let body = text.find("-->\n").expect("a header comment") + "-->\n".len();
                let at = (body + (text.len() - body) / 2..text.len())
                    .find(|&at| committed[at].is_ascii_alphanumeric())
                    .expect("a letter or digit past the middle");
                committed[at] ^= 1;
                let changed = String::from_utf8(committed).expect("still UTF-8");
                svg_mismatch(surface.name, &changed, &write()).is_none()
            }
        };
        assert!(
            !still_matched,
            "{}: a golden one byte off still matched",
            surface.name
        );
    }
}

/// Every surface has a golden whose header names it and its clause, the
/// surfaces left unreviewed are exactly those `UNREVIEWED` lists, and the
/// directory holds nothing the list does not name.
#[test]
fn every_surface_family_has_a_golden() {
    let mut unreviewed = Vec::new();
    for surface in SURFACES {
        let path = golden_path(surface);
        assert!(
            path.exists(),
            "{} has no golden; run the writer below with --ignored",
            surface.name
        );
        let bytes = std::fs::read(&path).expect("the golden reads");
        let header = match surface.golden {
            Kind::Raster { .. } => read_header(&bytes),
            Kind::Svg { .. } => read_svg_header(&String::from_utf8_lossy(&bytes)),
        }
        .unwrap_or_else(|e| panic!("{}: its header is refused: {e:?}", surface.name));
        assert_eq!(header.family, surface.name, "the header names its family");
        assert_eq!(header.clause, surface.clause, "the header names its clause");
        if header.reviewer == UNREVIEWED_MARK {
            unreviewed.push(surface.name);
        }
    }
    let mut listed = UNREVIEWED.to_vec();
    listed.sort_unstable();
    unreviewed.sort_unstable();
    assert_eq!(
        listed, unreviewed,
        "UNREVIEWED is the surfaces whose goldens read `reviewer: unreviewed`"
    );

    // A golden left behind by a renamed surface reads exactly like a
    // reviewed one.
    let mut wanted: Vec<String> = SURFACES.iter().map(|s| s.name.to_string()).collect();
    wanted.sort();
    assert_eq!(
        goldens_in(&surfaces_dir(), &["ppm", "svg"]),
        wanted,
        "the surface goldens on disk are the surfaces listed"
    );
}

/// A header with a field absent, blank or white space is refused, in the PPM
/// form and in the SVG comment form alike, and the undamaged twin of each is
/// read; a golden made from the writer's output gives that output back past
/// its header comment, and one changed byte is a difference.
#[test]
fn a_surface_golden_without_a_reviewer_is_refused() {
    let ppm = |reviewer: &str, reviewed: &str| {
        format!(
            "P6\n# tinker-pdf reviewed golden\n# family: signature\n# clause: 12.7.4.5\n\
             {reviewer}{reviewed}# built-by: surface_goldens.rs::signature\n1 1\n255\n\0\0\0"
        )
        .into_bytes()
    };
    let read = read_header(&ppm("# reviewer: A Person\n", "# reviewed: 2026-11-13\n"));
    assert_eq!(
        read,
        Ok(Header {
            family: "signature".into(),
            clause: "12.7.4.5".into(),
            reviewer: "A Person".into(),
            reviewed: "2026-11-13".into(),
            built_by: "surface_goldens.rs::signature".into(),
        }),
        "the undamaged twin is read"
    );
    let reviewed = "# reviewed: 2026-11-13\n";
    assert_eq!(
        read_header(&ppm("", reviewed)),
        Err(HeaderError::Missing("reviewer"))
    );
    assert_eq!(
        read_header(&ppm("# reviewer:\n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_header(&ppm("# reviewer: \t  \n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_header(&ppm("# reviewer: A Person\n", "# reviewed:  \n")),
        Err(HeaderError::Empty("reviewed")),
        "a later field is read as closely as the first"
    );

    let svg = |reviewer: &str, reviewed: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!--\ntinker-pdf reviewed golden\n\
             family: to-svg\nclause: SVG 1.1 8.3\n{reviewer}{reviewed}\
             built-by: surface_goldens.rs::to-svg\n-->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n"
        )
    };
    assert!(
        read_svg_header(&svg("reviewer: A Person\n", "reviewed: 2026-11-13\n")).is_ok(),
        "the undamaged twin is read"
    );
    let reviewed = "reviewed: 2026-11-13\n";
    assert_eq!(
        read_svg_header(&svg("", reviewed)),
        Err(HeaderError::Missing("reviewer"))
    );
    assert_eq!(
        read_svg_header(&svg("reviewer:\n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_svg_header(&svg("reviewer: \t  \n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_svg_header(&svg("reviewer: A Person\n", "reviewed:  \n")),
        Err(HeaderError::Empty("reviewed"))
    );
    assert_eq!(
        read_svg_header("<?xml version=\"1.0\"?>\n<svg/>\n"),
        Err(HeaderError::NotAnSvgGolden),
        "an SVG with no header comment is no golden"
    );
    assert_eq!(read_header(b"P5\n# family: x\n"), Err(HeaderError::NotAPpm));

    // The two forms, written and read back.
    let golden = Golden {
        family: "to-svg",
        clause: "SVG 1.1 8.3",
        built_by: "surface_goldens.rs::to-svg",
    };
    let output = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg width=\"1pt\"/>\n";
    let committed = to_svg_golden(output, golden, UNREVIEWED_MARK, UNREVIEWED_MARK);
    assert_eq!(svg_body(&committed).as_deref(), Some(output));
    let header = read_svg_header(&committed).expect("a written header reads");
    assert_eq!(
        (header.family.as_str(), header.reviewer.as_str()),
        ("to-svg", UNREVIEWED_MARK)
    );
    assert_eq!(svg_mismatch("to-svg", &committed, output), None);
    assert!(svg_mismatch("to-svg", &committed, &output.replace("1pt", "2pt")).is_some());

    let bitmap = Bitmap {
        width: 1,
        height: 1,
        format: tinker_pdf::PixelFormat::Rgb8,
        stride: 3,
        data: vec![10, 20, 30],
        warnings: Vec::new(),
        premultiplied: false,
        stats: None,
    };
    let committed = to_ppm(&bitmap, golden, UNREVIEWED_MARK, UNREVIEWED_MARK);
    assert!(read_header(&committed).is_ok(), "a written header reads");
    assert_eq!(ppm_mismatch("to-svg", &committed, &bitmap), None);
    let mut changed = bitmap.clone();
    changed.data[1] = 21;
    assert!(ppm_mismatch("to-svg", &committed, &changed).is_some());
}

/// A surface golden stays small enough that reviewing one is a real act, under
/// ceilings of the surfaces' own.
#[test]
fn a_surface_golden_stays_small_enough_to_review() {
    let goldens: Vec<(String, PathBuf)> = SURFACES
        .iter()
        .map(|surface| (surface.name.to_string(), golden_path(surface)))
        .collect();
    assert_small_enough(&goldens, PER_SURFACE, ALL_SURFACES);
}

/// Writes the surface goldens. Run with `--ignored` when a picture is meant to
/// change, and have the result reviewed before it is committed.
#[test]
#[ignore = "writes the committed goldens"]
fn write_the_surface_goldens() {
    let (reviewer, reviewed) = reviewer_and_date();
    std::fs::create_dir_all(surfaces_dir()).expect("the surfaces directory");
    for surface in SURFACES {
        let built_by = format!("surface_goldens.rs::{}", surface.name);
        let golden = Golden {
            family: surface.name,
            clause: surface.clause,
            built_by: &built_by,
        };
        match surface.golden {
            Kind::Raster { draw, least_ink } => {
                let bitmap = draw();
                let drawn = ink(&bitmap);
                assert!(
                    drawn >= least_ink,
                    "{}: the page painted {drawn} pixels, fewer than the {least_ink} it \
                     is supposed to -- a blank golden is not written",
                    surface.name
                );
                let bytes = to_ppm(&bitmap, golden, &reviewer, &reviewed);
                std::fs::write(golden_path(surface), &bytes).expect("the golden writes");
                println!(
                    "  wrote {} ({} bytes, {drawn} pixels of ink)",
                    surface.name,
                    bytes.len()
                );
            }
            Kind::Svg { write } => {
                let text = to_svg_golden(&write(), golden, &reviewer, &reviewed);
                std::fs::write(golden_path(surface), &text).expect("the golden writes");
                println!("  wrote {} ({} bytes)", surface.name, text.len());
            }
        }
    }
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}

/// `document`, and what `Document::open` synthesised for it held to the
/// validate-on-save helper. A PDF is opened, not written, and is handed back
/// as it is.
#[track_caller]
fn converted(label: &str, document: tinker_pdf::Document) -> tinker_pdf::Document {
    if document.archive().is_some() {
        written_support::synthesised(label, document)
    } else {
        document
    }
}
