//! Dirty-region re-render (`Appended::patch`, `docs/design/performance.md`,
//! P-17): an annotation edit appended to a document, and the page re-rendered
//! by the rectangle the edit changed over the bitmap of the page before it,
//! held to the bitmap a full render of the page after the edit gives — pixels,
//! warnings and shape — on pages turned every way and cropped, at scales from
//! 0.5x to 4x, over the image-run page ruling 5 names, and for every reason a
//! page is rendered whole instead. What a patch renders is counted on the
//! render's own stats (`Stats::pixels_rendered`).

mod written_support;

use tinker_pdf::write::{append, AppendOptions, Appended};
use tinker_pdf::{
    Bitmap, Dict, Document, DocumentEditor, ObjRef, Object, PatchFallback, PixelFormat,
    PixelRegion, RenderOptions, RenderWarning,
};
use tinker_pdf_cos::{annot, Name, Rect, StreamData};
use written_support::rewritten;

/// The page's first annotation, a red square, and its appearance.
const A: ObjRef = ObjRef { num: 5, gen: 0 };
const A_LOOK: ObjRef = ObjRef { num: 6, gen: 0 };
/// The page's second annotation, a green one with a diamond, and its
/// appearance.
const B: ObjRef = ObjRef { num: 7, gen: 0 };
/// The page itself, its tree, its content and the image its content draws.
const PAGE: ObjRef = ObjRef { num: 3, gen: 0 };
const TREE: ObjRef = ObjRef { num: 2, gen: 0 };
const CONTENT: ObjRef = ObjRef { num: 4, gen: 0 };
const IMAGE: ObjRef = ObjRef { num: 9, gen: 0 };
/// The optional content group the layered page's content is drawn in.
const LAYER: ObjRef = ObjRef { num: 10, gen: 0 };

/// `A`'s rectangle.
const A_RECT: [f64; 4] = [40.0, 30.0, 100.0, 80.0];

/// The scales every edit is patched at.
const SCALES: [f64; 5] = [0.5, 1.0, 1.5, 2.0, 4.0];

/// A PDF of `objects`, numbered from 1 in order, with a cross-reference table
/// so that it opens without repair.
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

/// A stream object: `dict`'s entries, its `/Length`, and `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn object(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// How a fixture's page is turned and cropped.
#[derive(Clone, Copy, Debug)]
struct Layout {
    rotate: u16,
    crop: Option<[f64; 4]>,
}

/// The layouts every edit is patched on: upright, a quarter turn, the other
/// quarter turn cropped, a half turn cropped, and upright cropped, each crop
/// box offset from the media box on both axes.
const LAYOUTS: [Layout; 5] = [
    Layout {
        rotate: 0,
        crop: None,
    },
    Layout {
        rotate: 90,
        crop: None,
    },
    Layout {
        rotate: 270,
        crop: Some([15.0, 10.0, 185.0, 140.0]),
    },
    Layout {
        rotate: 180,
        crop: Some([5.0, 20.0, 190.0, 150.0]),
    },
    Layout {
        rotate: 0,
        crop: Some([25.0, 15.0, 195.0, 145.0]),
    },
];

/// The page's content: a wash, a box, two diagonals that cross both
/// annotations, and a 2 x 2 image. `layered` draws the box and the diagonals
/// in the optional content group `/L1`.
fn content(layered: bool) -> String {
    let (open, close) = if layered {
        ("/OC /L1 BDC\n", "EMC\n")
    } else {
        ("", "")
    };
    format!(
        "0.95 0.9 0.6 rg 10 10 180 130 re f\n{open}\
         0.3 0.5 0.9 rg 60 50 80 40 re f\n\
         0 0 0 RG 1.5 w 0 0 m 200 150 l S 0 150 m 200 0 l S\n{close}\
         q 30 0 0 20 150 20 cm /Im Do Q\n"
    )
}

/// A 200 x 150 pt page laid out as `layout`, its content [`content`], with two
/// square annotations over it, `A` and `B`, each with an appearance of its
/// own. Objects: 1 the catalog, 2 the page tree, 3 the page, 4 its content,
/// 5 and 6 `A` and its appearance, 7 and 8 `B` and its, 9 the image, and 10
/// the layer, which only the layered page's catalog and content name.
fn annotated(layout: Layout, layered: bool) -> Vec<u8> {
    let crop = layout
        .crop
        .map(|[x0, y0, x1, y1]| format!("/CropBox [{x0} {y0} {x1} {y1}] "))
        .unwrap_or_default();
    // An upright page states no `/Rotate`, so that one its tree states is
    // the one it has (7.7.3.4).
    let rotate = match layout.rotate {
        0 => String::new(),
        turn => format!("/Rotate {turn}"),
    };
    let (catalog, properties) = if layered {
        (
            "<< /Type /Catalog /Pages 2 0 R \
             /OCProperties << /OCGs [10 0 R] /D << /ON [10 0 R] >> >> >>",
            "/Properties << /L1 10 0 R >> ",
        )
    } else {
        ("<< /Type /Catalog /Pages 2 0 R >>", "")
    };
    let [x0, y0, x1, y1] = A_RECT;
    pdf(&[
        object(catalog),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 150] {crop}{rotate}\n\
             /Resources << /XObject << /Im 9 0 R >> {properties}>> /Contents 4 0 R\n\
             /Annots [5 0 R 7 0 R] >>"
        )),
        stream("", content(layered).as_bytes()),
        object(&format!(
            "<< /Type /Annot /Subtype /Square /Rect [{x0} {y0} {x1} {y1}] /F 4 /AP << /N 6 0 R >> >>"
        )),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 50]",
            b"0.8 0.1 0.1 rg 0 0 60 50 re f 0 0 0 RG 2 w 4 4 52 42 re S",
        ),
        object(
            "<< /Type /Annot /Subtype /Square /Rect [110 60 170 120] /F 4 /AP << /N 8 0 R >> >>",
        ),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 60]",
            b"0.1 0.6 0.2 rg 0 0 60 60 re f 0 0 1 RG 3 w 30 5 m 55 30 l 30 55 l 5 30 l h S",
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode",
            b"FF000000FF000000FFFFFFFF>",
        ),
        object("<< /Type /OCG /Name (L1) >>"),
    ])
}

/// The three images of ruling 5's named exception on a 100 x 100 page — the
/// first and the third abut along `x = 40.5` across the top forty rows, and
/// the third overlaps the second in rows 60 to 69 — with a square annotation
/// over the top half, `/Rect [30 60 50 90]`. Objects: 1 the catalog, 2 the
/// page tree, 3 the page, 4 its content, 5 the image, 6 the annotation and 7
/// its appearance.
fn image_run_page() -> Vec<u8> {
    pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
             /Resources << /XObject << /I 5 0 R >> >> /Contents 4 0 R /Annots [6 0 R] >>",
        ),
        stream(
            "",
            b"q 40.5 0 0 40 0 60 cm /I Do Q q 40 0 0 40 40.5 0 cm /I Do Q \
              q 40 0 0 70 40.5 30 cm /I Do Q",
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode",
            b"00>",
        ),
        object("<< /Type /Annot /Subtype /Square /Rect [30 60 50 90] /F 4 /AP << /N 7 0 R >> >>"),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 20 30]",
            b"0.9 0.2 0.2 rg 0 0 20 30 re f",
        ),
    ])
}

/// The options at `scale`, with what the patch needs and nothing else.
fn at(scale: f64) -> RenderOptions {
    RenderOptions {
        scale,
        ..RenderOptions::default()
    }
}

/// Whether two bitmaps are one picture: shape, pixels, warnings and
/// premultiplication.
#[track_caller]
fn same_bitmap(got: &Bitmap, want: &Bitmap, what: &str) {
    assert_eq!(
        (got.width, got.height, got.stride, got.format),
        (want.width, want.height, want.stride, want.format),
        "{what}: the bitmap's shape"
    );
    assert!(got.data == want.data, "{what}: the pixels");
    assert_eq!(got.warnings, want.warnings, "{what}: the warnings");
    assert_eq!(
        got.premultiplied, want.premultiplied,
        "{what}: premultiplied"
    );
}

/// `rect` as a PDF array.
fn rect_array(rect: [f64; 4]) -> Object {
    Object::Array(rect.iter().map(|v| Object::Real(*v)).collect())
}

/// The dictionary `editor` holds for `r`.
#[track_caller]
fn dict_of(editor: &DocumentEditor, r: ObjRef) -> Dict {
    match editor.get(r) {
        Some(Object::Dict(dict)) => dict,
        Some(Object::Stream(stream)) => stream.dict,
        other => panic!("{r:?} is a dictionary: {other:?}"),
    }
}

/// `annotation`'s `/Rect` set to `rect`.
fn set_rect(editor: &mut DocumentEditor, annotation: ObjRef, rect: [f64; 4]) {
    let mut dict = dict_of(editor, annotation);
    dict.insert(editor.intern(b"Rect"), rect_array(rect));
    editor.put(annotation, Object::Dict(dict));
}

/// The page's `/Annots` set to `annots`.
fn set_annots(editor: &mut DocumentEditor, annots: &[ObjRef]) {
    let mut dict = dict_of(editor, PAGE);
    dict.insert(
        editor.intern(b"Annots"),
        Object::Array(annots.iter().map(|r| Object::Ref(*r)).collect()),
    );
    editor.put(PAGE, Object::Dict(dict));
}

/// The form stream at `r` given `data` in place of its own, its dictionary
/// kept but for its `/Length`.
fn set_look(editor: &mut DocumentEditor, r: ObjRef, data: &[u8]) {
    let mut dict = Dict::new();
    for (key, value) in dict_of(editor, r).iter() {
        if *key != Name::LENGTH {
            dict.insert(*key, value.clone());
        }
    }
    editor.put_stream(
        r,
        StreamData {
            dict,
            data: data.to_vec(),
        },
    );
}

/// A square annotation at `rect` whose appearance `look` draws over a box of
/// `bbox` — none when `bbox` is `None` — added to the page last, the
/// appearance an object of its own.
fn add_with_look(
    editor: &mut DocumentEditor,
    rect: [f64; 4],
    bbox: Option<[f64; 4]>,
    look: &[u8],
) -> ObjRef {
    let form = editor.allocate();
    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(editor.intern(b"XObject")));
    dict.insert(Name::SUBTYPE, Object::Name(editor.intern(b"Form")));
    if let Some(bbox) = bbox {
        dict.insert(editor.intern(b"BBox"), rect_array(bbox));
    }
    editor.put_stream(
        form,
        StreamData {
            dict,
            data: look.to_vec(),
        },
    );
    let mut ap = Dict::new();
    ap.insert(Name::N, Object::Ref(form));
    let mut annotation = Dict::new();
    annotation.insert(Name::TYPE, Object::Name(editor.intern(b"Annot")));
    annotation.insert(Name::SUBTYPE, Object::Name(editor.intern(b"Square")));
    annotation.insert(editor.intern(b"Rect"), rect_array(rect));
    annotation.insert(editor.intern(b"F"), Object::Int(4));
    annotation.insert(editor.intern(b"AP"), Object::Dict(ap));
    editor
        .add_annotation(0, annotation)
        .expect("the page is there")
}

/// One annotation edit, made through `editor`.
#[derive(Clone, Copy, Debug)]
enum Edit {
    /// `A` moved 37 pt right and 13 pt down, onto `B`.
    Move,
    /// `A` made 40 pt wider and 25 pt taller, its appearance fitted onto it.
    Resize,
    /// `A`'s appearance replaced by a blue one.
    Recolour,
    /// `A` taken off the page.
    Remove,
    /// A square added, its appearance synthesized.
    Add,
    /// A square added whose appearance draws an image its resources do not
    /// name, which the render draws a placeholder for and warns about.
    AddWarning,
}

const EDITS: [Edit; 6] = [
    Edit::Move,
    Edit::Resize,
    Edit::Recolour,
    Edit::Remove,
    Edit::Add,
    Edit::AddWarning,
];

fn make(editor: &mut DocumentEditor, doc: &Document, edit: Edit) {
    let [x0, y0, x1, y1] = A_RECT;
    match edit {
        Edit::Move => set_rect(editor, A, [x0 + 37.0, y0 - 13.0, x1 + 37.0, y1 - 13.0]),
        Edit::Resize => set_rect(editor, A, [x0, y0, x1 + 40.0, y1 + 25.0]),
        Edit::Recolour => set_look(editor, A_LOOK, b"0.1 0.2 0.9 rg 0 0 60 50 re f"),
        Edit::Remove => set_annots(editor, &[B]),
        Edit::Add => {
            let square = annot::square(
                doc.cos(),
                Rect {
                    x0: 20.0,
                    y0: 90.0,
                    x1: 70.0,
                    y1: 140.0,
                },
                annot::Color {
                    r: 0.5,
                    g: 0.0,
                    b: 0.5,
                },
                3.0,
            );
            editor.add_annotation(0, square).expect("the page is there");
        }
        Edit::AddWarning => {
            add_with_look(
                editor,
                [120.0, 15.0, 160.0, 55.0],
                Some([0.0, 0.0, 40.0, 40.0]),
                b"0 0 0 rg 2 2 36 36 re f q 20 0 0 20 10 10 cm /Missing Do Q",
            );
        }
    }
}

/// `edit` made to the document over `original` and appended, the joined
/// bytes held as an update of the file.
fn edited(original: &[u8], edit: impl FnOnce(&mut DocumentEditor, &Document)) -> Appended {
    let doc = Document::open(original.to_vec()).expect("the fixture opens");
    let mut editor = doc.editor();
    edit(&mut editor, &doc);
    let appended = append(&doc, &editor, &AppendOptions::default()).expect("it appends");
    rewritten(
        "an annotation edit, appended",
        original.to_vec(),
        [original, &appended.update[..]].concat(),
    );
    appended
}

/// Patches page one of `appended` at `options` over the render of the page
/// before the edit, and holds the result to the full render after it: the
/// patch, and what the full render was.
#[track_caller]
fn patched(
    original: &[u8],
    appended: &Appended,
    options: &RenderOptions,
    what: &str,
) -> (tinker_pdf::Patch, Bitmap) {
    let before = Document::open(original.to_vec()).expect("the fixture opens");
    let mut previous = before.page(0).expect("page one").render(options);
    let patch = appended
        .patch(0, &mut previous, options)
        .expect("the reopened document has page one");
    let full = appended.document.page(0).expect("page one").render(options);
    same_bitmap(&previous, &full, what);
    (patch, full)
}

/// **A patched render is the full render after every annotation edit.**
/// Moving, resizing, recolouring, removing and adding an annotation — one
/// whose appearance warns among them — on a page upright, turned a quarter
/// either way and a half turn, cropped and not, at 0.5x to 4x: the page
/// before the edit with the rectangle the edit changed rendered over it is
/// the full render of the page after it, pixel for pixel and warning for
/// warning, each by a rectangle smaller than the page. Over ruling 5's
/// image-run page, where the rectangle cuts the run the page ends, the
/// rectangle grows until it holds where the third image meets the run, and
/// the patch is the full render there too.
#[test]
fn a_patched_render_is_the_full_render_after_every_annotation_edit() {
    for layout in LAYOUTS {
        let original = annotated(layout, false);
        let base = Document::open(original.clone()).expect("the fixture opens");
        for edit in EDITS {
            let appended = edited(&original, |editor, doc| make(editor, doc, edit));
            for scale in SCALES {
                let what = format!("{edit:?} on {layout:?} at {scale}x");
                let options = at(scale);
                let before = base.page(0).expect("page one").render(&options);
                assert!(before.warnings.is_empty(), "{what}: a clean page");
                let (patch, full) = patched(&original, &appended, &options, &what);
                assert_eq!(patch.fallback, None, "{what}: patched");
                let region = patch.region.expect("the edit changed pixels");
                assert!(
                    u64::from(region.width) * u64::from(region.height)
                        < u64::from(full.width) * u64::from(full.height),
                    "{what}: {region:?} is less than the page"
                );
                assert!(before.data != full.data, "{what}: the edit shows");
                assert_eq!(
                    full.warnings.is_empty(),
                    !matches!(edit, Edit::AddWarning),
                    "{what}: {:?}",
                    full.warnings
                );
            }
        }
    }

    let original = image_run_page();
    let appended = edited(&original, |editor, _| {
        set_look(editor, ObjRef::new(7, 0), b"0.2 0.2 0.9 rg 0 0 20 30 re f");
    });
    for scale in SCALES {
        let what = format!("the image-run page at {scale}x");
        let (patch, full) = patched(&original, &appended, &at(scale), &what);
        assert_eq!(patch.fallback, None, "{what}: patched");
        let region = patch.region.expect("the edit changed pixels");
        // The annotation stands in the top half; where the third image meets
        // the run of the first two runs down to row 70 of the page's hundred.
        let row = |points: f64| (points * scale) as u32;
        assert!(
            region.y == 0 && region.y + region.height >= row(70.0),
            "{what}: {region:?} holds where the third image meets the run"
        );
        assert!(full.width > 0);
    }
}

/// **A patch renders only its rectangle**, counted on the render's stats:
/// recolouring `A` on the upright page at 1x renders `A`'s `/Rect`, rounded
/// outward and widened by a pixel, and nothing else — where the full render
/// draws the whole page — and over the image-run page the patch renders the
/// annotation's rectangle once, then the rectangle grown to where the third
/// image meets the run, and counts both. A patch that changes no pixel renders
/// none, and a page rendered whole counts the whole page.
#[test]
fn a_patch_renders_only_its_rectangle() {
    let counting = RenderOptions {
        stats: true,
        ..RenderOptions::default()
    };
    let pixels = |bitmap: &Bitmap| {
        bitmap
            .stats
            .expect("the render was asked to count")
            .pixels_rendered
    };

    let original = annotated(LAYOUTS[0], false);
    let appended = edited(&original, |editor, _| {
        set_look(editor, A_LOOK, b"0.1 0.2 0.9 rg 0 0 60 50 re f");
    });
    let before = Document::open(original.clone()).expect("it opens");
    let mut previous = before.page(0).expect("page one").render(&counting);
    assert_eq!(pixels(&previous), 200 * 150, "the whole page");
    let patch = appended
        .patch(0, &mut previous, &counting)
        .expect("page one");
    // `A`, 40 to 100 across and 30 to 80 up a 150 pt page: pixels 40 to 100
    // and 70 to 120 down, a pixel more each way.
    assert_eq!(patch.region, Some(PixelRegion::new(39, 69, 62, 52)));
    assert_eq!(patch.fallback, None);
    assert_eq!(pixels(&previous), 62 * 52, "the rectangle and nothing else");
    let full = appended
        .document
        .page(0)
        .expect("page one")
        .render(&counting);
    assert_eq!(pixels(&full), 200 * 150);
    same_bitmap(&previous, &full, "the counted patch");

    // The image-run page: the annotation at 30 to 50 across and 60 to 90 up
    // a 100 pt page is pixels 29 to 51 and 9 to 41 down; the third image
    // meets the run of the first two over pixels 40 to 81 and 0 to 70, which
    // the rectangle then grows to hold.
    let original = image_run_page();
    let appended = edited(&original, |editor, _| {
        set_look(editor, ObjRef::new(7, 0), b"0.2 0.2 0.9 rg 0 0 20 30 re f");
    });
    let before = Document::open(original.clone()).expect("it opens");
    let mut previous = before.page(0).expect("page one").render(&counting);
    let patch = appended
        .patch(0, &mut previous, &counting)
        .expect("page one");
    assert_eq!(patch.region, Some(PixelRegion::new(29, 0, 52, 70)));
    assert_eq!(
        pixels(&previous),
        22 * 32 + 52 * 70,
        "the rectangle, and the rectangle grown"
    );

    // An edit this page does not draw: nothing rendered, nothing counted.
    let original = annotated(LAYOUTS[0], false);
    let appended = edited(&original, |editor, _| editor.set_info(b"Title", "edited"));
    let before = Document::open(original.clone()).expect("it opens");
    let mut previous = before.page(0).expect("page one").render(&counting);
    let patch = appended
        .patch(0, &mut previous, &counting)
        .expect("page one");
    assert_eq!(patch.region, None);
    assert_eq!(patch.fallback, None);
    assert_eq!(pixels(&previous), 0, "no render");

    // A page rendered whole counts the whole page.
    let appended = edited(&original, |editor, _| {
        assert!(editor.rotate_page(0, 90));
    });
    let mut previous = before.page(0).expect("page one").render(&counting);
    let patch = appended
        .patch(0, &mut previous, &counting)
        .expect("page one");
    assert_eq!(patch.fallback, Some(PatchFallback::ContentChanged));
    assert_eq!(pixels(&previous), 150 * 200, "the turned page, whole");
}

/// One edit of a table of them.
type EditFn = Box<dyn Fn(&mut DocumentEditor, &Document)>;

/// **An edit that changes no pixel of a page leaves its bitmap as it was**:
/// an edit of the document's information, an annotation added hidden, and an
/// annotation edit on a render that draws no annotations each patch no
/// rectangle, and the bitmap is the full render after the edit.
#[test]
fn an_edit_that_changes_no_pixel_patches_nothing() {
    let original = annotated(LAYOUTS[1], false);
    let edits: [(&str, EditFn, RenderOptions); 3] = [
        (
            "the document's information",
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                editor.set_info(b"Title", "edited");
            }),
            at(1.5),
        ),
        (
            "a hidden annotation",
            Box::new(|editor: &mut DocumentEditor, doc: &Document| {
                let mut square = annot::square(
                    doc.cos(),
                    Rect {
                        x0: 20.0,
                        y0: 20.0,
                        x1: 80.0,
                        y1: 80.0,
                    },
                    annot::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                    },
                    2.0,
                );
                square.insert(editor.intern(b"F"), Object::Int(2 | 4));
                editor.add_annotation(0, square).expect("the page is there");
            }),
            at(2.0),
        ),
        (
            "an annotation moved, annotations not drawn",
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                set_rect(editor, A, [0.0, 0.0, 60.0, 50.0]);
            }),
            RenderOptions {
                annotations: false,
                ..at(1.0)
            },
        ),
    ];
    for (what, edit, options) in edits {
        let appended = edited(&original, |editor, doc| edit(editor, doc));
        let (patch, _) = patched(&original, &appended, &options, what);
        assert_eq!(patch.region, None, "{what}: no pixel changed");
        assert_eq!(patch.fallback, None, "{what}");
    }
}

/// **Every page the patch cannot take is rendered whole, and says why**, the
/// bitmap the full render after the edit all the same: a page inserted before
/// it (`OtherPage`), the page's content, an object its resources reach, its
/// crop box, its turn, a turn it inherits from its tree, and the layer its
/// content is drawn in (`ContentChanged`), two annotations left alone that
/// swap places (`Reordered`), an annotation added with no `/BBox`
/// (`Unbounded`), and bitmaps that are not the page's render — another scale,
/// another format, a region asked for, premultiplied otherwise, a stride too
/// narrow, too few bytes (`Mismatch`) — or that carry a warning
/// (`PreviousWarned`). Swapping two annotations of which one changed reorders
/// nothing left alone, and is patched. A page past the reopened document's
/// last is no patch, and the bitmap is untouched.
#[test]
fn every_page_the_patch_cannot_take_is_rendered_whole_and_says_why() {
    let original = annotated(LAYOUTS[0], false);
    let layered = annotated(LAYOUTS[2], true);
    let options = at(1.0);
    let cases: [(&str, &[u8], EditFn, PatchFallback); 9] = [
        (
            "a page inserted before",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                editor.insert_page(0, 200.0, 150.0).expect("inserted");
            }),
            PatchFallback::OtherPage,
        ),
        (
            "the content",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                editor.put_stream(
                    CONTENT,
                    StreamData {
                        dict: Dict::new(),
                        data: b"0 0 1 rg 0 0 50 50 re f".to_vec(),
                    },
                );
            }),
            PatchFallback::ContentChanged,
        ),
        (
            "the image the content draws",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                // Its samples written out plain: twelve zero bytes, all black.
                let mut dict = Dict::new();
                for (key, value) in dict_of(editor, IMAGE).iter() {
                    if *key != Name::FILTER && *key != Name::LENGTH {
                        dict.insert(*key, value.clone());
                    }
                }
                editor.put_stream(
                    IMAGE,
                    StreamData {
                        dict,
                        data: vec![0; 12],
                    },
                );
            }),
            PatchFallback::ContentChanged,
        ),
        (
            "the crop box",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                assert!(editor.set_crop_box(0, 10.0, 10.0, 190.0, 140.0));
            }),
            PatchFallback::ContentChanged,
        ),
        (
            "the turn",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                assert!(editor.rotate_page(0, 180));
            }),
            PatchFallback::ContentChanged,
        ),
        (
            "a turn the tree hands down",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                // The upright page states no `/Rotate` of its own, so it takes
                // its tree's; the page's own dictionary is left as it was.
                let mut tree = dict_of(editor, TREE);
                tree.insert(editor.intern(b"Rotate"), Object::Int(90));
                editor.put(TREE, Object::Dict(tree));
            }),
            PatchFallback::ContentChanged,
        ),
        (
            "the layer the content is drawn in",
            layered.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                assert!(editor.set_layer_visible(LAYER, false));
            }),
            PatchFallback::ContentChanged,
        ),
        (
            "two annotations swapped",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                set_annots(editor, &[B, A]);
            }),
            PatchFallback::Reordered,
        ),
        (
            "an annotation with no /BBox",
            original.as_slice(),
            Box::new(|editor: &mut DocumentEditor, _: &Document| {
                add_with_look(
                    editor,
                    [20.0, 90.0, 70.0, 140.0],
                    None,
                    b"0 0 1 rg 20 90 50 50 re f",
                );
            }),
            PatchFallback::Unbounded,
        ),
    ];
    for (what, original, edit, why) in cases {
        let appended = edited(original, |editor, doc| edit(editor, doc));
        let (patch, _) = patched(original, &appended, &options, what);
        assert_eq!(patch.fallback, Some(why), "{what}");
        assert_eq!(patch.region, None, "{what}: rendered whole");
    }

    // A swap that moves only an annotation the edit changed reorders nothing
    // the edit left alone.
    let appended = edited(&original, |editor, _| {
        set_annots(editor, &[B, A]);
        set_look(editor, A_LOOK, b"0.1 0.2 0.9 rg 0 0 60 50 re f");
    });
    let (patch, _) = patched(&original, &appended, &options, "a swap with a change");
    assert_eq!(patch.fallback, None);
    assert!(patch.region.is_some());

    // The page after an inserted one is no page the document had there.
    let appended = edited(&original, |editor, _| {
        editor.insert_page(0, 200.0, 150.0).expect("inserted");
    });
    let before = Document::open(original.clone()).expect("it opens");
    let mut previous = before.page(0).expect("page one").render(&options);
    let patch = appended
        .patch(1, &mut previous, &options)
        .expect("page two");
    assert_eq!(patch.fallback, Some(PatchFallback::OtherPage));
    let moved = appended
        .document
        .page(1)
        .expect("page two")
        .render(&options);
    same_bitmap(&previous, &moved, "the page moved to page two, whole");
    let mut untouched = before.page(0).expect("page one").render(&options);
    let kept = untouched.clone();
    assert!(appended.patch(2, &mut untouched, &options).is_none());
    same_bitmap(&untouched, &kept, "no page three, no patch");

    // Bitmaps that are not the page's render at these options.
    let appended = edited(&original, |editor, _| {
        set_look(editor, A_LOOK, b"0.1 0.2 0.9 rg 0 0 60 50 re f");
    });
    let page = before.page(0).expect("page one");
    let render = |options: &RenderOptions| page.render(options);
    let narrow = {
        let mut bitmap = render(&options);
        bitmap.stride -= 1;
        bitmap
    };
    let short = {
        let mut bitmap = render(&options);
        bitmap.data.truncate(bitmap.data.len() - 1);
        bitmap
    };
    let flagged = {
        let mut bitmap = render(&options);
        bitmap.premultiplied = true;
        bitmap
    };
    let region = RenderOptions {
        region: Some(PixelRegion::new(0, 0, 50, 50)),
        ..options.clone()
    };
    let mismatched: [(&str, Bitmap, RenderOptions); 6] = [
        ("another scale", render(&at(2.0)), options.clone()),
        (
            "another format",
            render(&RenderOptions {
                format: PixelFormat::Rgba8,
                ..options.clone()
            }),
            options.clone(),
        ),
        ("a region asked for", render(&region), region.clone()),
        ("premultiplied otherwise", flagged, options.clone()),
        ("a stride too narrow", narrow, options.clone()),
        ("too few bytes", short, options.clone()),
    ];
    for (what, mut previous, asked) in mismatched {
        let patch = appended.patch(0, &mut previous, &asked).expect("page one");
        assert_eq!(patch.fallback, Some(PatchFallback::Mismatch), "{what}");
        let full = appended.document.page(0).expect("page one").render(&asked);
        same_bitmap(&previous, &full, what);
    }
    let mut warned = render(&options);
    warned.warnings.push(RenderWarning::Cancelled);
    let patch = appended.patch(0, &mut warned, &options).expect("page one");
    assert_eq!(patch.fallback, Some(PatchFallback::PreviousWarned));
    let full = appended
        .document
        .page(0)
        .expect("page one")
        .render(&options);
    same_bitmap(&warned, &full, "a bitmap with a warning");
    assert!(
        warned.warnings.is_empty(),
        "the full render's, which has none"
    );
}
