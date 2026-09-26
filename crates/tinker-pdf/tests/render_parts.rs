//! `Page::render_form` and `Page::render_annotation`: one part of a page,
//! rendered on its own through the page's own pipeline.
//!
//! # What is asserted, and why equality is the whole of it
//!
//! Both entry points are the page's render with less in it — the same scale,
//! the same view transform, the same `Renderer` — so the claim they make is
//! exact: **a part rendered alone is byte-equal to its rectangle of a page
//! that draws that part there and nothing else**. Equality with no tolerance is
//! the only assertion that tells a second implementation from the first: a
//! separate drawing path that got a form's `/Matrix` right and its `/BBox` clip
//! a pixel wide would be *close*, and close is what every plausible defect here
//! looks like.
//!
//! Each part is compared twice, because the two comparisons catch different
//! defects. Against `Page::render` with `region` set to the same rectangle the
//! two frames are the same frame, so the bytes must be; against a crop of the
//! whole page render the frames differ by a whole-pixel translation, which
//! ruling 5 already requires to be exact for everything drawn here (no
//! shading — the one sampler ruling 5 names an exception for).
//!
//! Every fixture also draws something the part does not, away from it, so a
//! part render that drew the whole page fails on the pixels that are not the
//! part's; and each part carries an ink floor, so one that drew nothing fails
//! too.

mod render_support;

use render_support::curvy_font;
use tinker_pdf::{
    Bitmap, Document, DocumentBuilder, FormXObject, ImageData, NotDrawn, PixelFormat, PixelRegion,
    RenderOptions, RenderPartError,
};

/// Non-white pixels.
fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p.iter().take(3).any(|v| *v != 255))
        .count()
}

/// The rectangle `region` of `whole`, as its own bytes.
fn crop(whole: &Bitmap, region: PixelRegion) -> Vec<u8> {
    let n = whole.components();
    let mut out = Vec::new();
    for row in region.y..region.y + region.height {
        let at = row as usize * whole.stride + region.x as usize * n;
        out.extend_from_slice(&whole.data[at..at + region.width as usize * n]);
    }
    out
}

/// The region a part rendered into, recovered from the part's own size and a
/// region the caller knows it must be — the test states the rectangle it
/// expects rather than trusting the one it was handed.
fn same(part: &Bitmap, reference: &Bitmap, what: &str) {
    assert_eq!(
        (part.width, part.height, part.format),
        (reference.width, reference.height, reference.format),
        "{what}: a different shape"
    );
    if part.data != reference.data {
        let differing = part
            .data
            .iter()
            .zip(&reference.data)
            .filter(|(a, b)| a != b)
            .count();
        panic!("{what}: {differing} bytes differ from the page's own render");
    }
}

// ---- forms ------------------------------------------------------------------

/// A page of 120 x 90 points drawing two forms at the identity, an image, and
/// a rectangle of its own away from both:
///
/// - `/Fm0`, `/BBox [10 12 70 60]`: text in an embedded face, a diagonal fill
///   running past the box so the `/BBox` clip has something to cut, and a
///   stroked curve;
/// - `/Fm B` — a name with a space in it, which the content stream spells
///   `/Fm#20B` — whose `/Matrix` scales it by 2.5 and moves it to
///   (80.3, 50.3), so its box on the page is not its `/BBox` and none of its
///   edges is on a whole pixel;
/// - `/Im0`, an image: an XObject and not a form.
fn forms_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    let samples = [200u8, 30, 30].repeat(4);
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &samples,
        }
    ));
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [10.0, 12.0, 70.0, 60.0],
            matrix: None,
            group: None,
            content: b"BT /F0 11 Tf 14 44 Td (fox 01) Tj ET\n\
                       1 0 0 rg 5 15 m 80 58 l 60 15 l h f\n\
                       0 0 1 RG 1.7 w 12 20 m 30 70 50 0 68 40 c S",
        }
    ));
    assert!(builder.add_form(
        b"Fm B",
        &FormXObject {
            bbox: [0.0, 0.0, 10.0, 8.0],
            matrix: Some([2.5, 0.0, 0.0, 2.5, 80.3, 50.3]),
            group: None,
            content: b"0 0.5 0 rg 0 0 m 10 1 l 4 8 l h f",
        }
    ));
    builder.add_page(120.0, 90.0, |page| {
        assert!(page.form(b"Fm0"));
        // Spelled out rather than through `page.form(b"Fm B")`: the builder
        // writes a resource name into the content stream as its raw bytes, so
        // that call would write `/Fm B Do` — the name `/Fm`, an operator `B`
        // and a `Do` — and the page would never draw the form. The resource
        // dictionary's key is written escaped; the operand is not.
        page.raw(b"/Fm#20B Do");
        page.raw(b"0.3 0.3 0.3 rg 78 5 30 20 re f");
    });
    builder.finish()
}

/// Where each form lands on the page, in pixels at `scale`: `/BBox` through
/// `/Matrix`, rounded outward, with y counted down from the top of a
/// 90-point page. Computed here from the fixture's own numbers, not from the
/// code under test.
fn form_region(name: &[u8], scale: f64) -> PixelRegion {
    let (x0, y0, x1, y1) = match name {
        b"Fm0" => (10.0, 12.0, 70.0, 60.0),
        // [0 0 10 8] scaled by 2.5 and moved to (80.3, 50.3): no edge on a
        // whole pixel, so rounding outward and rounding inward disagree.
        _ => (80.3, 50.3, 105.3, 70.3),
    };
    let left = (x0 * scale).floor() as u32;
    let right = (x1 * scale).ceil() as u32;
    let top = ((90.0 - y1) * scale).floor() as u32;
    let bottom = ((90.0 - y0) * scale).ceil() as u32;
    PixelRegion::new(left, top, right - left, bottom - top)
}

/// **The exit criterion, for forms.** Each form, at three scales, is
/// byte-equal to its rectangle of the page — the page rendered with that
/// rectangle as its region, and the whole page cropped to it — and the
/// rectangle is the one the fixture's own `/BBox` and `/Matrix` say.
#[test]
fn a_form_alone_is_byte_equal_to_its_rectangle_of_the_page() {
    let doc = Document::open(forms_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    for scale in [1.0, 2.0, 1.5] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        let whole = page.render(&options);
        for name in [b"Fm0".as_slice(), b"Fm B"] {
            let what = format!("{} at {scale}x", String::from_utf8_lossy(name));
            let part = page.render_form(name, &options).expect("a form");
            let region = form_region(name, scale);
            assert_eq!(
                (part.width, part.height),
                (region.width, region.height),
                "{what}: the form's box on the page"
            );
            assert!(
                ink(&part) as f64 >= 60.0 * scale * scale,
                "{what}: drew {} pixels",
                ink(&part)
            );
            assert!(part.warnings.is_empty(), "{what}: {:?}", part.warnings);

            let framed = page.render(&RenderOptions {
                region: Some(region),
                ..options.clone()
            });
            same(
                &part,
                &framed,
                &format!("{what}, against the page's region"),
            );
            assert_eq!(
                part.data,
                crop(&whole, region),
                "{what}, against the whole page cropped"
            );
        }
    }
}

/// A form alone draws only the form: the page's own rectangle, which lies
/// inside neither form's box, is white in a render of either one even when the
/// caller's region is the whole page — which is what distinguishes "the form"
/// from "the page, cropped".
#[test]
fn a_form_alone_draws_nothing_but_the_form() {
    let doc = Document::open(forms_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let full = PixelRegion::new(0, 0, 120, 90);
    let options = RenderOptions {
        region: Some(full),
        ..RenderOptions::default()
    };
    let page_ink = ink(&page.render(&options));
    let fm0 = page.render_form(b"Fm0", &options).expect("a form");
    let fmb = page.render_form(b"Fm B", &options).expect("a form");
    assert_eq!(
        (fm0.width, fm0.height),
        (120, 90),
        "the caller's region wins"
    );
    // The page's rectangle: x 78..108, y 5..25 in page space, rows 65..85.
    let grey = crop(&fm0, PixelRegion::new(80, 67, 26, 16));
    assert!(
        grey.iter().all(|v| *v == 255),
        "Fm0 drew the page's rectangle"
    );
    assert!(
        ink(&fm0) + ink(&fmb) < page_ink,
        "the two forms are less than the page"
    );
}

/// A form the page's content stream names with an escape is found by the name
/// the escape stands for, and a caller's bytes that need escaping are escaped
/// on the way back into a content stream: `Fm B` is drawn, `Fm#20B` is not a
/// name the page has.
#[test]
fn a_name_is_the_decoded_name() {
    let doc = Document::open(forms_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions::default();
    assert!(page.render_form(b"Fm B", &options).is_ok());
    assert_eq!(
        page.render_form(b"Fm#20B", &options).err(),
        Some(RenderPartError::NoSuchXObject {
            name: "Fm#20B".to_string()
        })
    );
}

/// The refusals, each named: a name the page does not have, an image, and
/// names that are not names at all.
#[test]
fn a_form_that_is_not_there_or_not_a_form_is_refused_by_name() {
    let doc = Document::open(forms_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions::default();
    assert_eq!(
        page.render_form(b"Fm9", &options).err(),
        Some(RenderPartError::NoSuchXObject {
            name: "Fm9".to_string()
        })
    );
    assert_eq!(
        page.render_form(b"Im0", &options).err(),
        Some(RenderPartError::NotAForm {
            name: "Im0".to_string(),
            subtype: Some("Image".to_string())
        })
    );
    for hostile in [&b""[..], b"/", b"#", b"a b/c)(<>", &[0, 255, 10, 13]] {
        assert!(
            matches!(
                page.render_form(hostile, &options),
                Err(RenderPartError::NoSuchXObject { .. })
            ),
            "{hostile:?}"
        );
    }
}

/// A form with no `/Subtype`, and a form that is a dictionary where a stream
/// belongs, so that there is no content to decode at all.
#[test]
fn a_broken_form_is_refused_rather_than_drawn_blank() {
    let page = |xobject: &str| -> Vec<u8> {
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 50 50]\n\
   /Resources << /XObject << /X 4 0 R >> >> >>\nendobj\n\
4 0 obj\n{xobject}\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n"
        )
        .into_bytes()
    };
    let options = RenderOptions::default();
    let render = |bytes: Vec<u8>| {
        Document::open(bytes)
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render_form(b"X", &options)
            .err()
    };
    assert_eq!(
        render(page(
            "<< /Type /XObject /BBox [0 0 5 5] /Length 9 >>\nstream\n0 0 5 5 re\nendstream"
        )),
        Some(RenderPartError::NotAForm {
            name: "X".to_string(),
            subtype: None
        })
    );
    assert_eq!(
        render(page("<< /Subtype /Form >>")),
        Some(RenderPartError::UnreadableForm {
            name: "X".to_string()
        }),
        "a dictionary where a stream belongs"
    );
}

// ---- annotations ------------------------------------------------------------

/// An appearance stream: a red frame four points wide and a blue cross,
/// drawn in a `/BBox` of `[0 0 60 40]`.
///
/// The frame's corners are placed **off** the filler's sixteenth-of-a-pixel
/// grid on purpose, and the reason is a defect this file found and does not
/// fix. A stroked rectangle whose miter corners land exactly on a sub-scanline
/// — `3 3 54 34 re` at width 4, say — renders differently in a tile than in the
/// page under it, at 1x: four pixels, by 15 levels, one sixteenth of a pixel's
/// coverage each. The corner the stroker computes sits an ulp above the
/// sub-scanline in one frame and an ulp below it in the other, and `fill`
/// takes an edge's first sub-scanline as `ceil(y × 16)` with nothing to absorb
/// the ulp. It is ruling 5's named exception reached through a stroke instead
/// of a shading, it is independent of anything rendered here — a page with the
/// same frame in its own content stream shows it, with no annotation in sight
/// — and fixing it moves committed fingerprints, so it is reported instead.
/// The same-frame comparison below does not depend on it either way.
const FRAME: &str = "1 0 0 RG 4 w 3.3 2.7 53.4 34.6 re S 0 0 1 RG 1.5 w 8 8 m 52 32 l S";

/// A page of 150 x 100 points whose content draws one grey rectangle, with an
/// `/Annots` array of eight entries:
///
/// 0. a square at `/Rect [10 10 70 50]` whose appearance's box matches it;
/// 1. the same appearance fitted onto `/Rect [80 50 145 98]` at another size,
///    as a *direct* dictionary rather than a reference — whose `reference` is
///    `None`, which is why annotations are addressed by index;
/// 2. a hidden one (`/F 2`);
/// 3. a pop-up;
/// 4. one with no appearance;
/// 5. an integer, not a dictionary;
/// 6. one whose `/AS` names a state its appearance dictionary lacks;
/// 7. one whose `/Rect` is not four numbers.
fn annotations_page() -> Vec<u8> {
    let annots = "[5 0 R\n\
         << /Type /Annot /Subtype /Square /Rect [80 50 145 98] /AP << /N 6 0 R >> >>\n\
         7 0 R 8 0 R 9 0 R 42 10 0 R 11 0 R]";
    let objects = [
        (5, "<< /Type /Annot /Subtype /Square /Rect [10 10 70 50] /AP << /N 6 0 R >> >>".to_string()),
        (
            6,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 60 40] /Length {} >>\nstream\n{FRAME}\nendstream",
                FRAME.len() + 1
            ),
        ),
        (7, "<< /Type /Annot /Subtype /Square /F 2 /Rect [2 90 8 98] /AP << /N 6 0 R >> >>".to_string()),
        (8, "<< /Type /Annot /Subtype /Popup /Rect [2 90 8 98] /AP << /N 6 0 R >> >>".to_string()),
        (9, "<< /Type /Annot /Subtype /Text /Rect [2 90 8 98] >>".to_string()),
        (
            10,
            "<< /Type /Annot /Subtype /Widget /Rect [2 90 8 98] /AS /On /AP << /N << /Off 6 0 R >> >> >>"
                .to_string(),
        ),
        (11, "<< /Type /Annot /Subtype /Square /Rect [1 2 3] /AP << /N 6 0 R >> >>".to_string()),
    ];
    let content = "0.4 0.4 0.4 rg 100 5 40 30 re f";
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 150 100]\n\
   /Annots {annots} /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len() + 1
    );
    for (number, body) in objects {
        out.push_str(&format!("{number} 0 obj\n{body}\nendobj\n"));
    }
    out.push_str("trailer\n<< /Size 12 /Root 1 0 R >>\n%%EOF\n");
    out.into_bytes()
}

fn annotation_region(rect: (f64, f64, f64, f64), scale: f64) -> PixelRegion {
    let (x0, y0, x1, y1) = rect;
    let left = (x0 * scale).floor() as u32;
    let right = (x1 * scale).ceil() as u32;
    let top = ((100.0 - y1) * scale).floor() as u32;
    let bottom = ((100.0 - y0) * scale).ceil() as u32;
    PixelRegion::new(left, top, right - left, bottom - top)
}

/// **The exit criterion, for annotations.** Each annotation that draws, alone,
/// is byte-equal to its `/Rect` of the page rendered with annotations on —
/// both the page rendered with that rectangle as its region and the whole
/// page cropped — at three scales, the direct-dictionary one included.
#[test]
fn an_annotation_alone_is_byte_equal_to_its_rectangle_of_the_page() {
    let doc = Document::open(annotations_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    assert_eq!(page.annotations().len(), 8, "the index space is /Annots");
    assert_eq!(page.annotations()[1].reference, None, "entry 1 is direct");
    for scale in [1.0, 2.0, 1.5] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        let whole = page.render(&options);
        for (index, rect) in [
            (0, (10.0, 10.0, 70.0, 50.0)),
            (1, (80.0, 50.0, 145.0, 98.0)),
        ] {
            let what = format!("annotation {index} at {scale}x");
            let part = page.render_annotation(index, &options).expect("it draws");
            let region = annotation_region(rect, scale);
            assert_eq!(
                (part.width, part.height),
                (region.width, region.height),
                "{what}: the annotation's /Rect on the page"
            );
            assert!(
                ink(&part) as f64 >= 300.0 * scale * scale,
                "{what}: drew {} pixels",
                ink(&part)
            );
            let framed = page.render(&RenderOptions {
                region: Some(region),
                ..options.clone()
            });
            same(
                &part,
                &framed,
                &format!("{what}, against the page's region"),
            );
            assert_eq!(
                part.data,
                crop(&whole, region),
                "{what}, against the whole page cropped"
            );
        }
    }
}

/// `options.annotations` is not consulted: asking for one annotation is asking
/// for it drawn, and a caller who renders pages without them still gets it.
#[test]
fn an_annotation_is_drawn_whatever_the_page_switch_says() {
    let doc = Document::open(annotations_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let on = page
        .render_annotation(0, &RenderOptions::default())
        .expect("it draws");
    let off = page
        .render_annotation(
            0,
            &RenderOptions {
                annotations: false,
                ..RenderOptions::default()
            },
        )
        .expect("it draws");
    assert_eq!(on.data, off.data);
}

/// Every entry that exists and draws nothing is named, and an index past the
/// list is told how long the list is.
#[test]
fn an_annotation_that_draws_nothing_says_why() {
    let doc = Document::open(annotations_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions::default();
    let why = |index| match page.render_annotation(index, &options) {
        Err(RenderPartError::AnnotationNotDrawn { index: at, why }) => {
            assert_eq!(at, index);
            Some(why)
        }
        _ => None,
    };
    assert_eq!(why(2), Some(NotDrawn::Hidden));
    assert_eq!(why(3), Some(NotDrawn::Popup));
    assert_eq!(why(4), Some(NotDrawn::NoAppearance));
    assert_eq!(why(5), Some(NotDrawn::NotADictionary));
    assert_eq!(
        why(6),
        Some(NotDrawn::NoAppearance),
        "/AS names a state it lacks"
    );
    assert_eq!(why(7), Some(NotDrawn::NoRect));
    for index in [8, 9, usize::MAX] {
        assert_eq!(
            page.render_annotation(index, &options).err(),
            Some(RenderPartError::NoSuchAnnotation { index, count: 8 })
        );
    }
}

/// A page with no `/Annots`, and one whose `/Annots` is not an array, has no
/// annotation at any index — and says it has none.
#[test]
fn a_page_without_annotations_has_none_at_any_index() {
    for annots in ["", "/Annots 7", "/Annots << >>"] {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 50 50] {annots} >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        assert_eq!(
            page.render_annotation(0, &RenderOptions::default()).err(),
            Some(RenderPartError::NoSuchAnnotation { index: 0, count: 0 }),
            "{annots:?}"
        );
    }
}

/// Every option means for a part what it means for a page: a format, a
/// hard-edged render and a region all apply, through the same pipeline.
#[test]
fn a_part_takes_every_option_a_page_does() {
    let doc = Document::open(forms_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions {
        format: PixelFormat::GrayA8,
        antialias: false,
        ..RenderOptions::default()
    };
    let part = page.render_form(b"Fm0", &options).expect("a form");
    assert_eq!(part.format, PixelFormat::GrayA8);
    let framed = page.render(&RenderOptions {
        region: Some(form_region(b"Fm0", 1.0)),
        ..options.clone()
    });
    same(&part, &framed, "Fm0 in grey with hard edges");
}

/// A small fixed-seed generator, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// **Hostile documents never panic either entry point.** Both fixtures, each
/// with bytes flipped, overwritten and cut short, opened if they still open,
/// and every annotation index up to two past the end and every form name
/// either fixture uses asked for. Whatever comes back is a bitmap or a named
/// refusal; the campaign also counts that both were reached.
#[test]
fn mutated_documents_never_panic_a_part_render() {
    let mut rng = Rng(0x0BAD_5EED_F0F0_1234);
    let (mut drawn, mut refused) = (0usize, 0usize);
    for seed in [forms_page(), annotations_page()] {
        for _ in 0..150 {
            let mut bytes = seed.clone();
            for _ in 0..=(rng.next() % 6) {
                let at = (rng.next() % bytes.len() as u64) as usize;
                match rng.next() % 3 {
                    0 => bytes[at] ^= 1 << (rng.next() % 8),
                    1 => bytes[at] = b"/<>[]()0 9R"[(rng.next() % 11) as usize],
                    _ => bytes.truncate(at.max(1)),
                }
            }
            let Ok(doc) = Document::open(bytes) else {
                continue;
            };
            let Some(page) = doc.page(0) else {
                continue;
            };
            let options = RenderOptions::default();
            for index in 0..10 {
                match page.render_annotation(index, &options) {
                    Ok(_) => drawn += 1,
                    Err(_) => refused += 1,
                }
            }
            for name in [b"Fm0".as_slice(), b"Fm B", b"Im0", b"X"] {
                match page.render_form(name, &options) {
                    Ok(_) => drawn += 1,
                    Err(_) => refused += 1,
                }
            }
        }
    }
    assert!(
        drawn > 100 && refused > 100,
        "the campaign must reach both answers: {drawn} drawn, {refused} refused"
    );
}
