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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tinker_pdf::{
    Bitmap, Date, DigestAlgorithm, Document, Rect, RenderOptions, RenderWarning, SignRefused,
    SignatureAppearance, SignatureImage, Signer, SigningRequest, SigningTarget, SimpleFontProvider,
    WriteMode, WriteOptions,
};
use tinker_pdf_cos::{CosDocument, DocumentEditor, ObjRef};

mod golden_support;
mod render_support;
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
    /// The clause the picture is read against.
    clause: &'static str,
    /// The surface, drawn by this build.
    draw: fn() -> Bitmap,
    /// The fewest non-background pixels it may paint: a blank golden matches
    /// a blank render forever.
    least_ink: usize,
}

/// The surfaces, in the order their goldens are listed and reviewed.
///
/// Order-sensitive on purpose, as `render_goldens.rs`'s families are: a
/// surface dropped from the list is the failure this exists to make visible.
const SURFACES: &[Surface] = &[
    Surface {
        name: "annotation-appearances",
        clause: "ISO 32000-1 12.5.5, 12.5.6",
        draw: annotation_appearances,
        // About half of what the page paints, 6 895 pixels.
        least_ink: 3400,
    },
    Surface {
        name: "visible-signature",
        clause: "ISO 32000-1 12.7.4.5, 12.5.5",
        draw: visible_signature,
        // About half of what the page paints, 2 867 pixels.
        least_ink: 1400,
    },
];

/// The surfaces no person has read yet: exactly those whose golden's header
/// reads `reviewer: unreviewed`. A surface leaves this list when a person has
/// read its golden against its clause and put their name in it (H-73).
const UNREVIEWED: &[&str] = &["annotation-appearances", "visible-signature"];

fn surfaces_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("surfaces")
}

fn golden_path(surface: &Surface) -> PathBuf {
    surfaces_dir().join(format!("{}.ppm", surface.name))
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
    let document = Document::open(saved).expect("the saved file opens");
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
    let blank = Document::open(builder.finish()).expect("the blank page opens");

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

    let face = Arc::new(SimpleFontProvider::new(curvy_font()));
    Document::open(signed)
        .expect("the signed file opens")
        .with_fonts(face)
        .page(0)
        .expect("the page")
        .render(&RenderOptions::default())
}

// ---- the checks -------------------------------------------------------------

/// Every surface golden is what this build draws, byte for byte, above the
/// surface's ink floor and with every glyph it names drawn.
#[test]
fn every_surface_golden_is_what_this_build_draws() {
    let mut wrong = Vec::new();
    for surface in SURFACES {
        let bitmap = (surface.draw)();
        let drawn = ink(&bitmap);
        assert!(
            drawn >= surface.least_ink,
            "{}: the page painted {drawn} pixels, fewer than the {} it is \
             supposed to -- a blank golden matches a blank render forever",
            surface.name,
            surface.least_ink
        );
        assert!(
            !bitmap.warnings.contains(&RenderWarning::UnreadableFont),
            "{}: a font this build cannot draw, so its glyphs are missing",
            surface.name
        );
        let committed = std::fs::read(golden_path(surface)).expect("the golden reads");
        wrong.extend(ppm_mismatch(surface.name, &committed, &bitmap));
    }
    assert!(wrong.is_empty(), "{}", disagreement(&wrong));
}

/// One pixel changed in a surface golden is a golden this build does not
/// draw: every byte is compared, and no tolerance stands between a golden
/// and the picture.
#[test]
fn one_pixel_changed_in_a_surface_golden_fails_it() {
    for surface in SURFACES {
        let bitmap = (surface.draw)();
        let mut committed = std::fs::read(golden_path(surface)).expect("the golden reads");
        // The middle pixel's first component, one level away.
        let samples = ppm_samples(&committed).len();
        let at = committed.len() - samples + samples / 6 * 3;
        committed[at] ^= 1;
        assert!(
            ppm_mismatch(surface.name, &committed, &bitmap).is_some(),
            "{}: a golden one pixel off still matched",
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
        let header = read_header(&bytes)
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
        let bitmap = (surface.draw)();
        let drawn = ink(&bitmap);
        assert!(
            drawn >= surface.least_ink,
            "{}: the page painted {drawn} pixels, fewer than the {} it is              supposed to -- a blank golden is not written",
            surface.name,
            surface.least_ink
        );
        let built_by = format!("surface_goldens.rs::{}", surface.name);
        let golden = Golden {
            family: surface.name,
            clause: surface.clause,
            built_by: &built_by,
        };
        let bytes = to_ppm(&bitmap, golden, &reviewer, &reviewed);
        std::fs::write(golden_path(surface), &bytes).expect("the golden writes");
        println!(
            "  wrote {} ({} bytes, {drawn} pixels of ink)",
            surface.name,
            bytes.len()
        );
    }
}
