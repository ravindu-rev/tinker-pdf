//! The pages `render_regions.rs` tiles and `guards.rs`'s A9 draws in bands:
//! one set of fixtures, so the guard that holds a band to the page and the
//! guard that holds a tile to it draw the same documents.
//!
//! `render_regions.rs`'s header says why these pages: text, a shading, an
//! image and strokes, turned and cropped; canvases that stand somewhere in the
//! page; the stroker's whole pen, one-bit images, and JPEGs and JPEG 2000
//! codestreams decoded at the size they are drawn; and beside them the stroked frame whose corners sit on a
//! sub-scanline, the three-image page of ruling 5's named exception and the
//! page whose only ink is two annotations.
//!
//! # Why the allow
//!
//! Each binary compiles its own copy of this module and uses a different
//! subset of it.

#![allow(
    dead_code,
    reason = "shared by two test binaries; each uses a different subset"
)]

/// The validate-on-save helper's facade reading, for the pages this module
/// builds and the ones it shapes; a test that takes this module may take the
/// reading itself too.
#[allow(clippy::duplicate_mod)]
#[path = "../written_support/mod.rs"]
mod written_support;

use tinker_pdf::{
    DeviceSpace, Document, DocumentBuilder, Function, ImageData, Shading, WriteOptions,
};

use super::render_support::{
    bilevel_page, curvy_font, jpeg_minified_page, jpx_minified_page, pen_page,
};

/// The fixture page, in points. Not round, and coprime with every tile size
/// below, so no lattice in this file ever divides it evenly.
pub const PAGE_W: f64 = 91.0;
/// Taller than it is wide, so a quarter turn is visible in the bitmap's shape.
pub const PAGE_H: f64 = 131.0;

/// How a fixture's page is shaped: its `/Rotate`, and its `/CropBox` when it
/// has one that is not the media box.
#[derive(Clone, Copy)]
pub struct Shape {
    pub rotation: i64,
    pub crop: Option<(f64, f64, f64, f64)>,
}

/// The crop box every cropped fixture uses: offset from the media box on both
/// axes and ending inside it, so the page's own translation is non-zero in `x`
/// and in `y` before the region's is composed onto it.
pub const CROP: (f64, f64, f64, f64) = (11.0, 17.0, 79.0, 109.0);

impl Shape {
    pub const PLAIN: Shape = Shape {
        rotation: 0,
        crop: None,
    };

    pub const fn turned(rotation: i64) -> Shape {
        Shape {
            rotation,
            crop: None,
        }
    }

    pub const fn cropped(rotation: i64) -> Shape {
        Shape {
            rotation,
            crop: Some(CROP),
        }
    }
}

/// Applies a [`Shape`] to a built document, through the editor.
///
/// Through the editor and not by splicing bytes into the page dictionary,
/// which is what this did first and is worth recording: inserting
/// `/Rotate 90` after `/Type /Page` moves every byte after it, so every offset
/// in the cross-reference table is wrong by the length of the insertion. A page
/// of shapes survives that on the repair path; a page whose font program is
/// reached through the xref does not, and the `text` fixture came back
/// **blank** — and a blank page tiles perfectly. The ink floor in
/// [`with_page`] is what caught it.
pub fn shaped(bytes: Vec<u8>, shape: Shape) -> Vec<u8> {
    if shape.rotation == 0 && shape.crop.is_none() {
        return bytes;
    }
    let document = Document::open(bytes).expect("the fixture opens");
    let mut editor = document.editor();
    if shape.rotation != 0 {
        assert!(editor.rotate_page(0, shape.rotation), "the page rotates");
    }
    if let Some((x0, y0, x1, y1)) = shape.crop {
        assert!(editor.set_crop_box(0, x0, y0, x1, y1), "the crop box sets");
    }
    let out = editor.save(&WriteOptions::default());
    written_support::rewritten("shaped", document.cos().bytes(), &out);
    out
}

/// Seven lines of the six-shape synthetic face, at two sizes and two sub-pixel
/// phases.
///
/// The face is `render_support::curvy_font` — curves, diagonals and a thin
/// apex, chosen so that partial coverage is everywhere rather than at four
/// axis-aligned edges. Subsetting is off so that nothing between these bytes
/// and the pixels is a subsetter.
pub fn text_page(shape: Shape) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(
        builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()),
        "the synthetic face parses as a TrueType program"
    );
    builder.add_page(PAGE_W, PAGE_H, |page| {
        for line in 0..6 {
            let y = PAGE_H - 18.0 - f64::from(line) * 19.0;
            // The half point on alternate lines puts the same outlines on a
            // different sub-pixel phase, which is where a seam would show.
            let x = if line % 2 == 0 { 5.0 } else { 5.5 };
            page.text(b"F0", 15.0, x, y, "tiled#page");
        }
    });
    shaped(built("text page", builder), shape)
}

/// The page flooded by one diagonal axial shading (8.7.4.5.3).
///
/// Diagonal rather than along an axis: a gradient whose parameter is a function
/// of one coordinate agrees with itself under a shift in the other, so an axis
/// -aligned ramp is the one gradient a broken horizontal translation cannot
/// disturb.
pub fn shading_page(shape: Shape) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [0.0, 0.0, PAGE_W, PAGE_H],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![0.95, 0.15, 0.05],
                c1: vec![0.05, 0.25, 0.95],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(PAGE_W, PAGE_H, |page| {
        page.raw(format!("q 0 0 {PAGE_W} {PAGE_H} re W n").as_bytes());
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    shaped(built("shading page", builder), shape)
}

/// A 5x7 colour image stretched over the page at a ratio that is not a whole
/// number, so every device pixel is a sample between sample points.
pub fn image_page(shape: Shape) -> Vec<u8> {
    let (w, h) = (5u32, 7u32);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            // A pattern with no axis of symmetry, so a mirrored or transposed
            // region is not the same picture.
            data.push((x * 47 + y * 11) as u8);
            data.push((x * 13 + y * 61) as u8);
            data.push(255 - (x * 29 + y * 23) as u8);
        }
    }
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: w,
            height: h,
            data: &data,
        }
    ));
    builder.add_page(PAGE_W, PAGE_H, |page| {
        page.image(b"Im0", 3.0, 4.0, PAGE_W - 7.0, PAGE_H - 9.0);
    });
    shaped(built("image page", builder), shape)
}

/// Curves and strokes at an angle: the stroker's joins and the flattening
/// tolerance, which are a fourth path and the one that puts long anti-aliased
/// edges across the middle of a page rather than around glyph-sized shapes.
pub fn strokes_page(shape: Shape) -> Vec<u8> {
    let mut content = String::new();
    content.push_str("0.15 0.35 0.85 RG 2.7 w 1 J 1 j\n");
    content.push_str("4 6 m 33 118 62 12 87 96 c S\n");
    content.push_str("0.85 0.2 0.15 rg\n");
    content.push_str("9 14 m 57 101 l 81 21 l f\n");
    content.push_str("0 0 0 RG 0.6 w\n");
    for step in 0..11 {
        let y = 8.0 + f64::from(step) * 11.3;
        content.push_str(&format!(
            "3 {y:.3} m {:.3} {:.3} l S\n",
            PAGE_W - 3.0,
            y + 4.7
        ));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_page(PAGE_W, PAGE_H, |page| page.raw(content.as_bytes()));
    shaped(built("strokes page", builder), shape)
}

/// A bare one-page document around `content`, with `resources` and any further
/// objects numbered from 5, sized to [`PAGE_W`] by [`PAGE_H`].
///
/// Hand-written for the fixtures below because the builder writes no group, no
/// soft mask, no tiling pattern and no mesh.
pub fn raw_page(content: &str, resources: &str, objects: &[String]) -> Vec<u8> {
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}]\n\
   /Resources {resources} /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    );
    for (index, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 5));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 5
    ));
    out.into_bytes()
}

/// A stream object with `body` as its data.
pub fn stream(dict: &str, body: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    )
}

/// Transparency: a non-isolated group under a blend mode, an isolated knockout
/// group, and a luminosity soft mask drawn from a shading (11.4, 11.6.5).
///
/// **Each of the three is a canvas that stands somewhere in the page** — a
/// group's buffer is its clip's bounding box, a mask's is its `/BBox` — and
/// until the canvas carried its origin every one of them was drawn in a frame
/// of its own, the page's transform less the whole pixels to its corner. So
/// they are exactly where a tile and a page could round apart for a second
/// reason, and none of the four fixtures above has one. The mask's box hangs
/// past the page's top edge, and a tile below it asks about a mask whose group
/// it never sees.
pub fn transparency_page(shape: Shape) -> Vec<u8> {
    let content = "0.2 0.7 0.4 rg 0 0 91 40 re f\n\
                   q /GA gs /Grp Do Q\n\
                   q /GB gs /Knock Do Q\n\
                   q /GM gs 0.9 0.2 0.1 rg 0 58 91 66 re f Q";
    let grouped = "/GH gs 0 0 0.9 rg 6.3 4.1 34 34 re f 0.9 0.6 0 rg 20.2 14.7 34 34 re f";
    let knocked = "/GH gs 0.1 0.1 0.1 rg 33.4 44.2 34 34 re f 49.1 54.6 34 34 re f";
    let mask = "q 3 58 85 90 re W n /Sh0 sh Q";
    let resources = "<< /XObject << /Grp 5 0 R /Knock 6 0 R >> /Shading << /Sh0 9 0 R >>\n\
         /ExtGState << /GA << /ca 0.6 /BM /Multiply >>\n\
                       /GB << /ca 1 /BM /Normal /SMask /None >>\n\
                       /GH << /ca 0.5 >>\n\
                       /GM << /SMask << /S /Luminosity /G 7 0 R /TR 8 0 R >> >> >> >>";
    let bytes = raw_page(
        content,
        resources,
        &[
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 58 56] \
                 /Group << /S /Transparency /I false /K false >>",
                grouped,
            ),
            stream(
                "/Type /XObject /Subtype /Form /BBox [30 40 88 100] \
                 /Group << /S /Transparency /I true /K true >>",
                knocked,
            ),
            stream(
                "/Type /XObject /Subtype /Form /BBox [3 58 88 148] \
                 /Group << /S /Transparency /CS /DeviceGray >>",
                mask,
            ),
            "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >>".to_string(),
            "<< /ShadingType 2 /ColorSpace /DeviceGray /Coords [5 60 86 140]\n\
             /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>\n\
             /Extend [true true] >>"
                .to_string(),
        ],
    );
    shaped(bytes, shape)
}

/// A tiling pattern (8.7.3.2): a rotated lattice whose cell is rasterised once
/// into a buffer of its own and composited at whole-pixel offsets.
///
/// The cell's buffer is a canvas that stands at a place in the page, like a
/// group's, and the offsets are rounded from the lattice arithmetic; both have
/// to come out the same in a tile as in the page.
pub fn tiling_page(shape: Shape) -> Vec<u8> {
    let content = "/Pattern cs /P0 scn 4 6 m 86 9 l 80 124 l 9 110 l h f";
    let cell = "0.9 0.2 0.1 rg -2 -2 8 12 re f 0 0.5 0.2 rg 4 4 8 8 re f";
    let bytes = raw_page(
        content,
        "<< /Pattern << /P0 5 0 R >> >>",
        &[stream(
            "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
             /XStep 11 /YStep 8 /Matrix [0.9659 0.2588 -0.2588 0.9659 3 -5] \
             /Resources << >>",
            cell,
        )],
    );
    shaped(bytes, shape)
}

/// A free-form Gouraud mesh (8.7.4.5.5), two triangles sharing an edge, which
/// the rasterizer draws into one buffer over its own device rectangle and then
/// spreads colour into the anti-aliased fringe of.
pub fn mesh_page(shape: Shape) -> Vec<u8> {
    // flag, x, y, r, g, b — a byte each, decoded onto the page and 0..1.
    let vertex = |flag: u8, x: f64, y: f64, rgb: [u8; 3]| {
        let byte = |v: f64, extent: f64| (v / extent * 255.0).round() as u8;
        format!(
            "{flag:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            byte(x, PAGE_W),
            byte(y, PAGE_H),
            rgb[0],
            rgb[1],
            rgb[2]
        )
    };
    let data = [
        vertex(0, 4.0, 5.0, [230, 20, 20]),
        vertex(0, 86.0, 21.0, [20, 200, 40]),
        vertex(0, 37.0, 119.0, [20, 40, 230]),
        vertex(1, 88.0, 127.0, [240, 220, 30]),
    ]
    .concat()
        + ">";
    let bytes = raw_page(
        "/Sh0 sh",
        "<< /Shading << /Sh0 5 0 R >> >>",
        &[stream(
            &format!(
                "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
                 /BitsPerComponent 8 /BitsPerFlag 8 \
                 /Decode [0 {PAGE_W} 0 {PAGE_H} 0 1 0 1 0 1] /Filter /ASCIIHexDecode"
            ),
            &data,
        )],
    );
    shaped(bytes, shape)
}

/// Every fixture: a builder, the page dictionary entry it is built with, a
/// name, and the least ink it must draw.
///
/// The ink floor is not decoration. `determinism.rs` enrolled a text fixture
/// whose face was missing, hashed a blank page, and passed on every target for
/// months; a tiling guard has the same failure mode and it is worse here,
/// because a blank page tiles perfectly. Every fixture is held to a floor
/// before it is allowed to be evidence.
pub struct Fixture {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub least_ink: usize,
}

pub fn fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut add = |name: &'static str, bytes: Vec<u8>, least_ink: usize| {
        out.push(Fixture {
            name,
            bytes,
            least_ink,
        });
    };

    add("text", text_page(Shape::PLAIN), 1_000);
    add("shading", shading_page(Shape::PLAIN), 11_000);
    add("image", image_page(Shape::PLAIN), 9_000);
    add("strokes", strokes_page(Shape::PLAIN), 2_500);
    // A quarter turn: the bitmap is 131x91 and the region indexes *that*.
    add("text rotated 90", text_page(Shape::turned(90)), 1_000);
    add("image rotated 90", image_page(Shape::turned(90)), 9_000);
    add(
        "strokes rotated 270",
        strokes_page(Shape::turned(270)),
        2_000,
    );
    // A crop box offset from the media box on both axes.
    add("text cropped", text_page(Shape::cropped(0)), 400);
    add("shading cropped", shading_page(Shape::cropped(0)), 6_000);
    // Both at once, which is where the two translations and the rotation have
    // to compose in the one right order.
    add(
        "strokes cropped and rotated",
        strokes_page(Shape::cropped(90)),
        700,
    );
    // Canvases that stand somewhere in the page: group and mask buffers, a
    // tiling cell, a mesh's own buffer. Turned and cropped once each, so their
    // corners are carried through the page's own translation as well.
    add("transparency", transparency_page(Shape::PLAIN), 8_000);
    add(
        "transparency cropped and rotated",
        transparency_page(Shape::cropped(90)),
        4_500,
    );
    add("tiling", tiling_page(Shape::PLAIN), 6_500);
    add("tiling rotated 270", tiling_page(Shape::turned(270)), 6_500);
    add("mesh", mesh_page(Shape::PLAIN), 6_000);
    add("mesh cropped", mesh_page(Shape::cropped(0)), 4_300);
    // The `strokes` fingerprint's page in `determinism.rs`: every cap and
    // join, a miter past its limit, dashes at two phases along a curve and an
    // even-odd star, the outlines the stroker overlaps by construction.
    add("pen", pen_page(), 1_500);
    // One-bit images five ways at four sizes: the page `determinism.rs`
    // fingerprints as `bilevel`, built by the same function. A page of its own
    // size, 93 by 165, which no tile size here divides either.
    add("bilevel", bilevel_page(), 2_800);
    // JPEGs drawn at a half, a quarter and an eighth: the page `determinism.rs`
    // fingerprints as `jpeg_minified`, 119 by 157, which no tile size here
    // divides.
    add("jpeg minified", jpeg_minified_page(), 2_000);
    // JPEG 2000 codestreams drawn with one level dropped and two: the page
    // `determinism.rs` fingerprints as `jpx_minified`, 96 by 80, which no tile
    // size here divides.
    add("jpx minified", jpx_minified_page(), 550);
    out
}

/// A 150 x 100 page whose content strokes one rectangle four points wide, its
/// miter corners on the filler's sixteenth-of-a-pixel grid.
///
/// `render_parts.rs` found this with an annotation's appearance, whose
/// `/BBox` is fitted onto a `/Rect` at `(10, 10)`, and said the same frame
/// in a page's own content stream shows it; this is that page.
pub fn framed_page() -> Vec<u8> {
    let content = "q 1 0 0 1 10 10 cm 1 0 0 RG 4 w 3 3 54 34 re S Q";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 150 100] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// Three one-sample black images on a 100 x 100 page. The first and the third
/// abut along `x = 40.5` across the page's top forty rows; the third overlaps
/// the second in rows 60 to 69, which is the bottom half of the page.
pub fn image_run_page() -> Vec<u8> {
    let content = "q 40.5 0 0 40 0 60 cm /I Do Q \
                   q 40 0 0 40 40.5 0 cm /I Do Q \
                   q 40 0 0 70 40.5 30 cm /I Do Q";
    let image = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 \
                 /Filter /ASCIIHexDecode /Length 3 >>\nstream\n00>\nendstream";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /XObject << /I 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n{image}\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// A page whose only ink comes from an annotation appearance stream (12.5.5).
///
/// Hand-written rather than built, because the builder writes no `/Annots` and
/// `annotation_appearances.rs` writes its fixtures the same way for the same
/// reason. The appearance is a curve and a diagonal so that its edges are
/// anti-aliased, and its `/BBox` is fitted onto a `/Rect` of a different size
/// so the annotation's own transform is not the identity.
pub fn annotated_page() -> Vec<u8> {
    let stream = "0.1 0.2 0.8 rg 0 0 m 30 44 l 60 0 l f \
                  0 0 0 RG 2 w 3 40 m 30 6 57 46 57 8 c S";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}]\n\
   /Annots [5 0 R 7 0 R] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Square /Rect [7 11 84 79]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 44]\n\
   /Length {len} >>\nstream\n{stream}\nendstream\nendobj\n\
7 0 obj\n<< /Type /Annot /Subtype /Square /Rect [21 86 70 124]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n",
        len = stream.len()
    )
    .into_bytes()
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}
