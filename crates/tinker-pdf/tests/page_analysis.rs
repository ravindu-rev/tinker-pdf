//! Page analysis (`docs/design/page-analysis.md`), each answer held to a
//! fixture whose answer is known: built here, object by object, so that the
//! box, the transparency and the overprint a page carries are written in the
//! test beside it.
//!
//! The glyph fixtures draw the synthetic face `render_support` builds, which
//! the document embeds, so they hold in the default build and with
//! `bundled-fonts` alike.

mod render_support;

use tinker_pdf::{Document, Overprint, PageAnalysis, PixelRegion, RenderOptions};

// ---- building pages -------------------------------------------------------------

fn object(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A complete PDF of `objects`, numbered from 1.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// One page and what it names. Objects: 1 the catalog, 2 the page tree, 3
/// the page, 4 its content, and `extra` from 5.
struct Page<'a> {
    /// Entries the catalog carries beside its `/Pages`.
    catalog: &'a str,
    /// Entries the page carries beside its `/MediaBox`, `/Resources` and
    /// `/Contents`: a `/Rotate`, a `/Group`.
    page: &'a str,
    /// The page's `/MediaBox`.
    media: &'a str,
    /// The page's resource dictionary's entries.
    resources: &'a str,
    content: &'a str,
    extra: Vec<Vec<u8>>,
}

impl Default for Page<'_> {
    fn default() -> Self {
        Page {
            catalog: "",
            page: "",
            media: "[0 0 200 100]",
            resources: "",
            content: "",
            extra: Vec::new(),
        }
    }
}

impl Page<'_> {
    fn bytes(&self) -> Vec<u8> {
        let mut objects = vec![
            object(&format!(
                "<< /Type /Catalog /Pages 2 0 R {} >>",
                self.catalog
            )),
            object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            object(&format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox {} /Resources << {} >> \
                 /Contents 4 0 R {} >>",
                self.media, self.resources, self.page
            )),
            stream("", self.content.as_bytes()),
        ];
        objects.extend(self.extra.iter().cloned());
        pdf(&objects)
    }

    fn analysis(&self, options: &RenderOptions) -> PageAnalysis {
        let document = Document::open(self.bytes()).expect("the page opens");
        let page = document.page(0).expect("its page");
        let analysis = page.analysis(options);
        // One answer whichever door is asked.
        assert_eq!(
            page.display_list().analysis(options),
            analysis,
            "the display list answers as the page"
        );
        analysis
    }
}

/// The synthetic face as three objects from `at`: the font, its descriptor
/// and its program. Code 0x21 is glyph 1, the chevron, whose outline runs
/// from 20 to 540 across and 0 to 700 up in units of a thousandth of an em,
/// every extreme an on-curve point.
fn face(at: usize) -> Vec<Vec<u8>> {
    vec![
        object(&format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Curvy /FirstChar 32 /LastChar 126\n\
             /Widths [320{}] /Encoding /WinAnsiEncoding /FontDescriptor {} 0 R >>",
            " 640".repeat(94),
            at + 1
        )),
        object(&format!(
            "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 /FontBBox [0 0 1000 1000]\n\
             /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 \
             /FontFile2 {} 0 R >>",
            at + 2
        )),
        stream("", &render_support::curvy_font()),
    ]
}

/// A two-by-two RGB image, with `entries` beside its own.
fn image(entries: &str) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 {entries}"
        ),
        &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
    )
}

/// An axial shading across the page, red to blue.
const AXIAL: &str = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
    /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
    /Extend [true true] >>";

/// Two layers, `/Off` hidden and `/On` shown, as objects 5 and 6.
const LAYERS_CATALOG: &str =
    "/OCProperties << /OCGs [5 0 R 6 0 R] /D << /ON [6 0 R] /OFF [5 0 R] >> >>";
const LAYERS_RESOURCES: &str = "/Properties << /Off 5 0 R /On 6 0 R >>";

fn layers() -> Vec<Vec<u8>> {
    vec![
        object("<< /Type /OCG /Name (Off) >>"),
        object("<< /Type /OCG /Name (On) >>"),
    ]
}

// ---- the box and transparency ---------------------------------------------------

/// The box of a page's marks, in points and in pixels.
fn marks(page: &Page<'_>, options: &RenderOptions) -> Option<((f64, f64, f64, f64), PixelRegion)> {
    page.analysis(options)
        .marks
        .map(|marks| (marks.points, marks.pixels))
}

fn near(got: (f64, f64, f64, f64), want: (f64, f64, f64, f64), what: &str) {
    for (g, w) in [
        (got.0, want.0),
        (got.1, want.1),
        (got.2, want.2),
        (got.3, want.3),
    ] {
        assert!((g - w).abs() < 1e-9, "{what}: {got:?} against {want:?}");
    }
}

/// Every mark's known box on a 200 x 100 page, in points and in pixels at
/// 72 dpi, where a point is a pixel and the pixel rows count down from the
/// top: `y` points up is row `100 - y`.
#[test]
fn each_answer_is_the_fixtures_known_one() {
    let at_72 = RenderOptions::default();

    // A fill: its rectangle.
    let fill = Page {
        content: "10 20 30 40 re f",
        ..Page::default()
    };
    let (points, pixels) = marks(&fill, &at_72).expect("a fill is a mark");
    near(points, (10.0, 20.0, 40.0, 60.0), "fill");
    assert_eq!(pixels, PixelRegion::new(10, 40, 30, 40), "fill");
    // At 144 dpi every pixel figure doubles; turned a quarter clockwise, the
    // page's bottom-left is the bitmap's top-left, so `x` across is `y` up.
    let at_144 = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    let (_, doubled) = marks(&fill, &at_144).expect("a fill is a mark");
    assert_eq!(doubled, PixelRegion::new(20, 80, 60, 80), "fill at 144 dpi");
    let turned = Page {
        page: "/Rotate 90",
        ..fill
    };
    let (points, pixels) = marks(&turned, &at_72).expect("a fill is a mark");
    near(points, (10.0, 20.0, 40.0, 60.0), "turned");
    assert_eq!(pixels, PixelRegion::new(20, 10, 40, 30), "turned");

    // A wide stroke: butt caps end at the path's ends, and the pen reaches
    // half its width either side.
    let stroke = Page {
        content: "8 w 20 50 m 120 50 l S",
        ..Page::default()
    };
    let (points, pixels) = marks(&stroke, &at_72).expect("a stroke is a mark");
    near(points, (20.0, 46.0, 120.0, 54.0), "stroke");
    assert_eq!(pixels, PixelRegion::new(20, 46, 100, 8), "stroke");

    // A glyph: the chevron at 100 points from (50.5, 10.5) spans 2 to 54 of
    // the em across and 0 to 70 up — edges half a pixel inside whole ones, so
    // rounding outward takes the pixel each one is in.
    let glyph = Page {
        resources: "/Font << /F1 5 0 R >>",
        content: "BT /F1 100 Tf 50.5 10.5 Td (!) Tj ET",
        extra: face(5),
        ..Page::default()
    };
    let (points, pixels) = marks(&glyph, &at_72).expect("a glyph is a mark");
    near(points, (52.5, 10.5, 104.5, 80.5), "glyph");
    assert_eq!(pixels, PixelRegion::new(52, 19, 53, 71), "glyph");
    // Mode 3 paints nothing and mode 7 only clips (9.3.6).
    for mode in [3, 7] {
        let content = format!("BT {mode} Tr /F1 100 Tf 50.5 10.5 Td (!) Tj ET");
        let unpainted = Page {
            resources: "/Font << /F1 5 0 R >>",
            content: &content,
            extra: face(5),
            ..Page::default()
        };
        assert_eq!(marks(&unpainted, &at_72), None, "render mode {mode}");
    }

    // A rotated image: its unit square under `30 40 -40 30 100 10 cm`, a
    // turn and a scale of five with every corner on a whole point.
    let rotated = Page {
        resources: "/XObject << /Im 5 0 R >>",
        content: "q 30 40 -40 30 100 10 cm /Im Do Q",
        extra: vec![image("")],
        ..Page::default()
    };
    let (points, pixels) = marks(&rotated, &at_72).expect("an image is a mark");
    near(points, (60.0, 10.0, 130.0, 80.0), "rotated image");
    assert_eq!(pixels, PixelRegion::new(60, 20, 70, 70), "rotated image");

    // `sh` paints its clip (8.7.4.2), and the page where there is none.
    let shading = Page {
        resources: &format!("/Shading << /Sh {AXIAL} >>"),
        content: "q 20 20 60 30 re W n /Sh sh Q",
        ..Page::default()
    };
    let (points, pixels) = marks(&shading, &at_72).expect("a shading is a mark");
    near(points, (20.0, 20.0, 80.0, 50.0), "clipped sh");
    assert_eq!(pixels, PixelRegion::new(20, 50, 60, 30), "clipped sh");
    let flood = Page {
        resources: &format!("/Shading << /Sh {AXIAL} >>"),
        content: "/Sh sh",
        ..Page::default()
    };
    let (points, pixels) = marks(&flood, &at_72).expect("a shading is a mark");
    near(points, (0.0, 0.0, 200.0, 100.0), "unclipped sh");
    assert_eq!(pixels, PixelRegion::new(0, 0, 200, 100), "unclipped sh");

    // A mark in hidden content is no mark; the visible one beside it is.
    let hidden = Page {
        catalog: LAYERS_CATALOG,
        resources: LAYERS_RESOURCES,
        content: "/OC /Off BDC 0 0 200 100 re f EMC /OC /On BDC 10 10 20 20 re f EMC",
        extra: layers(),
        ..Page::default()
    };
    let (points, _) = marks(&hidden, &at_72).expect("the visible fill");
    near(points, (10.0, 10.0, 30.0, 30.0), "hidden");
    let only_hidden = Page {
        catalog: LAYERS_CATALOG,
        resources: LAYERS_RESOURCES,
        content: "/OC /Off BDC 0 0 200 100 re f 8 w 0 0 m 200 100 l S EMC",
        extra: layers(),
        ..Page::default()
    };
    assert_eq!(marks(&only_hidden, &at_72), None, "only hidden content");

    // A mark clipped away is no mark, and one clipped is its part.
    let clipped_away = Page {
        content: "q 0 0 10 10 re W n 50 50 20 20 re f Q",
        ..Page::default()
    };
    assert_eq!(marks(&clipped_away, &at_72), None, "clipped away");
    let clipped = Page {
        content: "q 0 0 30 30 re W n 20 20 40 40 re f Q",
        ..Page::default()
    };
    let (points, _) = marks(&clipped, &at_72).expect("its part in the clip");
    near(points, (20.0, 20.0, 30.0, 30.0), "clipped");

    // A mark off the page is no mark, and one across its edge is its part on
    // the page.
    let off = Page {
        content: "250 50 20 20 re f",
        ..Page::default()
    };
    assert_eq!(marks(&off, &at_72), None, "off the page");
    let across = Page {
        content: "190 90 30 30 re f",
        ..Page::default()
    };
    let (points, pixels) = marks(&across, &at_72).expect("its part on the page");
    near(points, (190.0, 90.0, 200.0, 100.0), "across the edge");
    assert_eq!(pixels, PixelRegion::new(190, 0, 10, 10), "across the edge");

    // A white fill and a fill at no alpha are marks: geometry decides.
    let white = Page {
        resources: "/ExtGState << /None << /ca 0 >> >>",
        content: "1 g 10 10 10 10 re f /None gs 50 50 10 10 re f",
        ..Page::default()
    };
    let (points, _) = marks(&white, &at_72).expect("both are marks");
    near(points, (10.0, 10.0, 60.0, 60.0), "invisible marks");
    // A fill of no area is none, and a hairline is one, at its path.
    let thin = Page {
        content: "10 10 m 50 10 l f 0 w 20 60 m 80 60 l S",
        ..Page::default()
    };
    let (points, pixels) = marks(&thin, &at_72).expect("the hairline");
    near(points, (20.0, 60.0, 80.0, 60.0), "hairline");
    assert_eq!(pixels, PixelRegion::new(20, 40, 60, 1), "hairline");

    // An empty page paints nothing.
    assert_eq!(marks(&Page::default(), &at_72), None, "an empty page");

    transparency_in_each_of_its_five_ways();
}

/// Whether a page uses transparency.
fn uses(page: &Page<'_>) -> bool {
    page.analysis(&RenderOptions::default()).transparency
}

/// Each of the five ways and its twin without it.
fn transparency_in_each_of_its_five_ways() {
    // 1. A transparency group: the page's own, and a form's.
    let grouped = Page {
        page: "/Group << /S /Transparency /CS /DeviceRGB >>",
        content: "10 10 20 20 re f",
        ..Page::default()
    };
    assert!(uses(&grouped), "a page group");
    let plain = Page {
        content: "10 10 20 20 re f",
        ..Page::default()
    };
    assert!(!uses(&plain), "no group");
    let form = |group: &str| {
        stream(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 50 50] {group}"),
            b"10 10 20 20 re f",
        )
    };
    let grouped_form = Page {
        resources: "/XObject << /Fm 5 0 R >>",
        content: "/Fm Do",
        extra: vec![form("/Group << /S /Transparency >>")],
        ..Page::default()
    };
    assert!(uses(&grouped_form), "a form's group");
    let plain_form = Page {
        resources: "/XObject << /Fm 5 0 R >>",
        content: "/Fm Do",
        extra: vec![form("")],
        ..Page::default()
    };
    assert!(!uses(&plain_form), "a form with no group");

    // 2. A soft mask set by `gs`, and a paint under it; `/SMask /None`; and
    // a mask nothing is painted under.
    let mask_group = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 100] \
         /Group << /S /Transparency /CS /DeviceGray >>",
        b"1 g 0 0 100 100 re f",
    );
    let masks = "/ExtGState << /M << /SMask << /S /Luminosity /G 5 0 R >> >> \
                 /U << /SMask /None >> >>";
    let masked = Page {
        resources: masks,
        content: "/M gs 0 0 200 100 re f",
        extra: vec![mask_group.clone()],
        ..Page::default()
    };
    assert!(uses(&masked), "a paint under a soft mask");
    let unmasked = Page {
        resources: masks,
        content: "/U gs 0 0 200 100 re f",
        extra: vec![mask_group.clone()],
        ..Page::default()
    };
    assert!(!uses(&unmasked), "/SMask /None");
    let unpainted = Page {
        resources: masks,
        content: "10 10 20 20 re f /M gs",
        extra: vec![mask_group],
        ..Page::default()
    };
    assert!(!uses(&unpainted), "a mask nothing is painted under");

    // 3. An `/SMask` on an image the page draws, and the same image without.
    let mask_image = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8",
        &[128],
    );
    let soft_image = Page {
        resources: "/XObject << /Im 5 0 R >>",
        content: "q 50 0 0 50 10 10 cm /Im Do Q",
        extra: vec![image("/SMask 6 0 R"), mask_image],
        ..Page::default()
    };
    assert!(uses(&soft_image), "an image with /SMask");
    let hard_image = Page {
        resources: "/XObject << /Im 5 0 R >>",
        content: "q 50 0 0 50 10 10 cm /Im Do Q",
        extra: vec![image("")],
        ..Page::default()
    };
    assert!(!uses(&hard_image), "an image without one");

    // 4. A blend mode other than Normal; Normal and its synonym Compatible.
    let blend = |mode: &str| format!("/ExtGState << /B << /BM /{mode} >> >>");
    for (mode, want) in [("Multiply", true), ("Normal", false), ("Compatible", false)] {
        let resources = blend(mode);
        let page = Page {
            resources: &resources,
            content: "/B gs 10 10 20 20 re f",
            ..Page::default()
        };
        assert_eq!(uses(&page), want, "/BM /{mode}");
    }

    // 5. A constant alpha below one, for the paint's own kind, with the
    // tolerance PDF/A's reading allows.
    let alphas = "/ExtGState << /Half << /ca 0.5 >> /One << /ca 1 >> \
                  /Nearly << /ca 0.9999999 >> /StrokeHalf << /CA 0.5 >> >>";
    for (content, want, what) in [
        ("/Half gs 10 10 20 20 re f", true, "ca 0.5 under a fill"),
        ("/One gs 10 10 20 20 re f", false, "ca 1"),
        (
            "/Nearly gs 10 10 20 20 re f",
            false,
            "ca within the tolerance",
        ),
        (
            "/StrokeHalf gs 2 w 10 10 m 50 10 l S",
            true,
            "CA 0.5 under a stroke",
        ),
        (
            "/StrokeHalf gs 10 10 20 20 re f",
            false,
            "CA under a fill only",
        ),
        ("/Half gs", false, "an alpha nothing is painted under"),
    ] {
        let page = Page {
            resources: alphas,
            content,
            ..Page::default()
        };
        assert_eq!(uses(&page), want, "{what}");
    }

    // Hidden content paints nothing, so what it would have used is unused.
    let hidden = Page {
        catalog: LAYERS_CATALOG,
        resources: "/Properties << /Off 5 0 R /On 6 0 R >> \
                    /ExtGState << /Half << /ca 0.5 >> >>",
        content: "/OC /Off BDC /Half gs 10 10 20 20 re f EMC",
        extra: layers(),
        ..Page::default()
    };
    assert!(!uses(&hidden), "transparency in hidden content");
}

// ---- overprint ---------------------------------------------------------------------

/// The overprint a page's paints are made with.
fn overprint(page: &Page<'_>) -> Overprint {
    page.analysis(&RenderOptions::default()).overprint
}

/// What `fills`, `strokes` and `nonzero_mode` should read, as one triple.
fn read(page: &Page<'_>) -> (bool, bool, bool) {
    let o = overprint(page);
    (o.fills, o.strokes, o.nonzero_mode)
}

/// The graphics states the overprint fixtures set by `gs`: `/OP` alone (which
/// `/op` then takes, Table 58), `/OP` for strokes only, `/op` alone, `/op`
/// under each mode, and nothing.
const STATES: &str = "/ExtGState << \
    /Both << /OP true >> \
    /StrokeOnly << /OP true /op false >> \
    /FillOnly << /op true >> \
    /Mode1 << /op true /OPM 1 >> \
    /Mode0 << /op true /OPM 0 >> \
    /ModeAlone << /OPM 1 >> \
    /Off << /OP false /op false >> >>";

/// **A paint is made with overprint when the key for its kind is in force at
/// it** (8.6.7): `/op` for a fill, a filled glyph, an image and `sh`, `/OP`
/// for a stroke and a stroked glyph; a state set and never painted under says
/// nothing, nor does hidden content or a mask's group; a form's paints are the
/// page's; and the mode is the one the overprinting paint was made under.
#[test]
fn overprint_is_reported_where_a_paint_uses_it() {
    let page = |content: &'static str| Page {
        resources: STATES,
        content,
        ..Page::default()
    };

    // `/OP` on a stroke, and the fill beside it that `/op false` keeps off.
    assert_eq!(
        read(&page(
            "/StrokeOnly gs 4 w 10 10 m 100 10 l S 10 20 30 30 re f"
        )),
        (false, true, false),
        "/OP on a stroke"
    );
    // `/op` on a fill, and the stroke beside it that has no `/OP`.
    assert_eq!(
        read(&page(
            "/FillOnly gs 10 20 30 30 re f 4 w 10 10 m 100 10 l S"
        )),
        (true, false, false),
        "/op on a fill"
    );
    // `/OP` alone sets both, since `/op` takes its value.
    assert_eq!(
        read(&page("/Both gs 10 20 30 30 re f 4 w 10 10 m 100 10 l S")),
        (true, true, false),
        "/OP alone"
    );
    // Set by `gs` and never painted under: before the `gs`, or nothing after.
    assert_eq!(
        read(&page("10 20 30 30 re f 4 w 10 10 m 100 10 l S /Both gs")),
        (false, false, false),
        "set and never painted under"
    );
    assert!(!overprint(&page("/Both gs")).used(), "a gs alone");
    // `Q` puts the state back.
    assert_eq!(
        read(&page("q /Both gs Q 10 20 30 30 re f")),
        (false, false, false),
        "restored by Q"
    );
    // Turned off again.
    assert_eq!(
        read(&page("/Both gs /Off gs 10 20 30 30 re f")),
        (false, false, false),
        "turned off"
    );

    // The mode, as the overprinting paint settled it: 1, 0, and a mode set
    // with no overprint on.
    assert_eq!(
        read(&page("/Mode1 gs 10 20 30 30 re f")),
        (true, false, true),
        "/OPM 1"
    );
    assert_eq!(
        read(&page("/Mode0 gs 10 20 30 30 re f")),
        (true, false, false),
        "/OPM 0"
    );
    assert_eq!(
        read(&page("/ModeAlone gs 10 20 30 30 re f")),
        (false, false, false),
        "/OPM 1 with nothing overprinting"
    );

    // An image and `sh` are non-stroking paints.
    let image_page = Page {
        resources: &format!("{STATES} /XObject << /Im 5 0 R >>"),
        content: "/FillOnly gs q 20 0 0 20 10 10 cm /Im Do Q",
        extra: vec![image("")],
        ..Page::default()
    };
    assert_eq!(read(&image_page), (true, false, false), "an image");
    let shading_page = Page {
        resources: &format!("{STATES} /Shading << /Sh {AXIAL} >>"),
        content: "/FillOnly gs q 0 0 50 50 re W n /Sh sh Q",
        ..Page::default()
    };
    assert_eq!(read(&shading_page), (true, false, false), "sh");

    // A glyph overprints by its mode: filled under `/op`, stroked under
    // `/OP`.
    for (mode, state, want) in [
        (0, "/FillOnly", (true, false, false)),
        (1, "/StrokeOnly", (false, true, false)),
        (1, "/FillOnly", (false, false, false)),
        (2, "/Both", (true, true, false)),
    ] {
        let content = format!("{state} gs BT {mode} Tr /F1 50 Tf 20 20 Td (!) Tj ET");
        let glyph = Page {
            resources: &format!("{STATES} /Font << /F1 5 0 R >>"),
            content: &content,
            extra: face(5),
            ..Page::default()
        };
        assert_eq!(read(&glyph), want, "a glyph in mode {mode} under {state}");
    }

    // Set inside a form: the form's paint is the page's.
    let form = stream(
        &format!("/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << {STATES} >>"),
        b"/FillOnly gs 10 10 20 20 re f",
    );
    let in_form = Page {
        resources: "/XObject << /Fm 5 0 R >>",
        content: "/Fm Do",
        extra: vec![form],
        ..Page::default()
    };
    assert_eq!(read(&in_form), (true, false, false), "set inside a form");

    // Set inside hidden content, and painted there: nothing is painted.
    let hidden = Page {
        catalog: LAYERS_CATALOG,
        resources: &format!("{LAYERS_RESOURCES} {STATES}"),
        content: "/OC /Off BDC /Both gs 10 10 20 20 re f 2 w 0 0 m 50 50 l S EMC",
        extra: layers(),
        ..Page::default()
    };
    assert!(!overprint(&hidden).used(), "inside hidden content");
    // Set inside hidden content and painted after it: the state outlives the
    // scope (8.11.3.2 hides paints, not operators), so the paint after it
    // overprints.
    let after = Page {
        catalog: LAYERS_CATALOG,
        resources: &format!("{LAYERS_RESOURCES} {STATES}"),
        content: "/OC /Off BDC /FillOnly gs EMC 10 10 20 20 re f",
        extra: layers(),
        ..Page::default()
    };
    assert_eq!(
        read(&after),
        (true, false, false),
        "painted after hidden content"
    );

    // A soft mask's group is a picture of a mask, and its paints are none of
    // the page's.
    let mask_group = stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 100] \
             /Group << /S /Transparency /CS /DeviceGray >> /Resources << {STATES} >>"
        ),
        b"/Both gs 1 g 0 0 100 100 re f",
    );
    let masked = Page {
        resources: "/ExtGState << /M << /SMask << /S /Luminosity /G 5 0 R >> >> >>",
        content: "/M gs 0 0 200 100 re f",
        extra: vec![mask_group],
        ..Page::default()
    };
    assert!(!overprint(&masked).used(), "inside a mask's group");
}
