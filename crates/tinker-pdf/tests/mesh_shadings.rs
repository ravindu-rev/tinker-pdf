//! Mesh shadings paint (8.7.4.5.5 to 8.7.4.5.8).
//!
//! Types 4 to 7 were a capability gate until August 2026: `read_shading`
//! answered types 1, 2 and 3 and returned the type number for the rest, which
//! surfaced as a named `UnsupportedShading` warning and a blank area. That was
//! correct degradation, and it is what these tests replace.
//!
//! # What is easy to get wrong here, and therefore what is asserted
//!
//! Almost every defect in a mesh renders *plausibly*. A stream cut up at the
//! wrong bit width gives vertices — in the wrong places, which reads as a
//! transform bug. An ignored edge flag gives triangles — every other one
//! wrong, which reads as a torn mesh. A `/Decode` range read backwards gives a
//! mesh — mirrored. Colour converted at the vertices and interpolated in RGB
//! gives a gradient — with the wrong middle, which nothing that tests the ends
//! can see. So the assertions here are about middles, about continuity across
//! a shared edge, and about two spellings of the same shading agreeing.

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning};

/// Packs values of a fixed bit width, most significant bit first — the layout
/// 8.7.4.5.5 gives the vertex stream.
struct Packer {
    data: Vec<u8>,
    bit: u32,
}

impl Packer {
    fn new() -> Packer {
        Packer {
            data: Vec::new(),
            bit: 0,
        }
    }

    fn push(&mut self, value: u64, bits: u32) -> &mut Packer {
        for index in (0..bits).rev() {
            if self.bit == 0 {
                self.data.push(0);
            }
            if let Some(byte) = self.data.last_mut() {
                *byte |= (((value >> index) & 1) as u8) << (7 - self.bit);
            }
            self.bit = (self.bit + 1) % 8;
        }
        self
    }

    /// 8.7.4.5.5 pads each vertex of a type 4 stream to a byte; 8.7.4.5.7 pads
    /// each patch of a type 6 or 7 one.
    fn align(&mut self) -> &mut Packer {
        self.bit = 0;
        self
    }

    fn hex(&self) -> String {
        let mut out = String::with_capacity(self.data.len() * 2 + 1);
        for byte in &self.data {
            out.push_str(&format!("{byte:02x}"));
        }
        out.push('>');
        out
    }
}

/// A 60 x 60 page whose only shading resource is the dictionary given, with
/// the packed stream as its body.
fn mesh_page(keys: &str, stream: &Packer, content: &str) -> Bitmap {
    let hex = stream.hex();
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /Shading << /Sh0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< {keys} /Filter /ASCIIHexDecode /Length {} >>\nstream\n{hex}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1,
        hex.len() + 1,
    );
    render(bytes.into_bytes())
}

fn render(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

/// Whether a render gave up on a shading, and on which type.
fn unsupported(bitmap: &Bitmap) -> Option<i64> {
    bitmap.warnings.iter().find_map(|w| match w {
        RenderWarning::UnsupportedShading { kind } => Some(*kind),
        _ => None,
    })
}

/// The eight keys every fixture below shares: one grey component per vertex,
/// eight bits everywhere, and page coordinates straight through `/Decode`.
const GREY_KEYS: &str = "/ColorSpace /DeviceGray /BitsPerCoordinate 8 \
     /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 60 0 60 0 1]";

/// A square split along its diagonal into two type 4 triangles, dark on the
/// left and light on the right.
fn split_square() -> Packer {
    let mut stream = Packer::new();
    // (x, y, grey) at 8 bits, where 60 units is the full `/Decode` range and a
    // raw 255 is 60 points.
    let corners = [
        (0u64, 0u64, 0u64),
        (255, 0, 255),
        (255, 255, 255),
        (0, 255, 0),
    ];
    for index in [0usize, 1, 2, 0, 2, 3] {
        let (x, y, grey) = corners[index];
        stream
            .push(0, 8)
            .push(x, 8)
            .push(y, 8)
            .push(grey, 8)
            .align();
    }
    stream
}

/// A mesh paints, rather than warning and leaving the area blank.
#[test]
fn a_free_form_triangle_mesh_paints_a_gradient() {
    let bitmap = mesh_page(
        &format!("/ShadingType 4 {GREY_KEYS}"),
        &split_square(),
        "/Sh0 sh",
    );

    assert_eq!(unsupported(&bitmap), None, "type 4 is painted now");

    // The page is 60 x 60 at scale 1, and the mesh covers all of it. Grey runs
    // left to right, so a column decides the colour and a row does not.
    let left = pixel(&bitmap, 2, 30).0;
    let right = pixel(&bitmap, 57, 30).0;
    assert!(left < 20, "the dark edge is {left}");
    assert!(right > 235, "the light edge is {right}");

    // The middle is the value that says it interpolated rather than filled.
    let middle = pixel(&bitmap, 30, 30).0;
    assert!(
        (118..=138).contains(&middle),
        "halfway across should be mid grey, got {middle}"
    );

    // And it is flat down a column, which a mesh torn along the diagonal is
    // not.
    for y in 5..55 {
        let value = pixel(&bitmap, 30, y).0;
        assert!(
            (i32::from(value) - i32::from(middle)).abs() <= 3,
            "column 30 is not flat: {value} at row {y} against {middle}"
        );
    }
}

/// **The seam.** Two triangles sharing an edge must not leave a line where
/// they meet.
///
/// The failure this guards is not subtle once seen and is very hard to
/// attribute: filling each triangle separately gives about half coverage from
/// each along the shared edge, source-over composites the two to about three
/// quarters, and the mesh acquires a lattice of pale lines. Here the page
/// starts white, so a seam shows as a pixel *lighter* than both of its
/// horizontal neighbours — which is what is checked, along every row, rather
/// than at one sampled point.
#[test]
fn two_triangles_sharing_an_edge_leave_no_seam() {
    // Flat mid-grey, so the only thing that can vary along a row is coverage.
    let mut stream = Packer::new();
    let corners = [(0u64, 0u64), (255, 0), (255, 255), (0, 255)];
    for index in [0usize, 1, 2, 0, 2, 3] {
        let (x, y) = corners[index];
        stream.push(0, 8).push(x, 8).push(y, 8).push(128, 8).align();
    }

    let bitmap = mesh_page(&format!("/ShadingType 4 {GREY_KEYS}"), &stream, "/Sh0 sh");
    let flat = pixel(&bitmap, 30, 30).0;
    assert!((123..=133).contains(&flat), "the mesh is mid grey: {flat}");

    // The diagonal runs from (0, 0) to (60, 60) in page space, which is
    // top-left to bottom-right after the flip. Every interior pixel is the
    // same grey, and in particular no pixel is lighter than its neighbours.
    let mut lightest = 0u8;
    for y in 2..58 {
        for x in 2..58 {
            let value = pixel(&bitmap, x, y).0;
            lightest = lightest.max(value);
            assert!(
                (i32::from(value) - i32::from(flat)).abs() <= 2,
                "({x}, {y}) is {value}, not the mesh's {flat}: that is a seam"
            );
        }
    }
    assert!(
        lightest <= flat + 2,
        "some pixel is lighter than the mesh at {lightest}"
    );
}

/// Type 5 needs no flags and takes its shape from `/VerticesPerRow`
/// (8.7.4.5.6).
#[test]
fn a_lattice_mesh_paints() {
    // A 3 x 3 lattice over the page, dark at the top and light at the bottom.
    let mut stream = Packer::new();
    for row in 0..3u64 {
        for column in 0..3u64 {
            let grey = row * 127;
            stream
                .push(column * 127, 8)
                .push(row * 127, 8)
                .push(grey.min(255), 8);
        }
    }

    let bitmap = mesh_page(
        &format!("/ShadingType 5 /VerticesPerRow 3 {GREY_KEYS}"),
        &stream,
        "/Sh0 sh",
    );
    assert_eq!(unsupported(&bitmap), None, "type 5 is painted");

    // Row 0 of the lattice is at y = 0 in page space, which is the *bottom* of
    // the bitmap.
    let bottom = pixel(&bitmap, 30, 57).0;
    let top = pixel(&bitmap, 30, 2).0;
    assert!(bottom < 20, "the dark row is {bottom}");
    assert!(top > 230, "the light row is {top}");
    let middle = pixel(&bitmap, 30, 30).0;
    assert!(
        (110..=145).contains(&middle),
        "the middle row is {middle}, not halfway"
    );

    // And flat across, which a lattice whose rows were cut up wrongly is not.
    for x in 3..57 {
        let value = pixel(&bitmap, x, 30).0;
        assert!(
            (i32::from(value) - i32::from(middle)).abs() <= 3,
            "the middle row is not flat: {value} at column {x}"
        );
    }
}

/// A `/Function` mesh and a direct-component mesh describing the same colours
/// render to the same bytes.
///
/// The two forms are deliberately chosen so that "the same colours" is exact
/// rather than approximate: the function is `C0 = [0]`, `C1 = [1]`, `N = 1`,
/// which evaluates to its input unchanged, and the space is `/DeviceGray`, so
/// the interpolated parametric value and the interpolated component are the
/// same number reached by the same arithmetic. Anything less than byte
/// equality here would be the two paths disagreeing.
#[test]
fn the_function_form_and_the_component_form_agree() {
    let component_form = mesh_page(
        &format!("/ShadingType 4 {GREY_KEYS}"),
        &split_square(),
        "/Sh0 sh",
    );
    let function_form = mesh_page(
        &format!(
            "/ShadingType 4 {GREY_KEYS} \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>"
        ),
        &split_square(),
        "/Sh0 sh",
    );

    assert_eq!(unsupported(&function_form), None);
    for y in 0..component_form.height {
        for x in 0..component_form.width {
            assert_eq!(
                pixel(&component_form, x, y),
                pixel(&function_form, x, y),
                "the two forms disagree at ({x}, {y})"
            );
        }
    }
}

/// **Colour is interpolated in the shading's own colour space, then
/// converted.**
///
/// The space here is a `/Separation` whose tint transform is cubic, so the
/// difference between the two orders is enormous rather than subtle. A vertex
/// at tint 0 is white and one at tint 1 is black; halfway between them the
/// tint is 0.5, and `0.5^3` is 0.125, so the correct midpoint is 87 per cent
/// of the way back to white — around 221.
///
/// Converting at the vertices and interpolating the *results* would give 125,
/// the midpoint of white and black. Both are plausible gradients and only one
/// is the file's.
#[test]
fn colour_is_interpolated_in_the_shadings_own_space() {
    // The same square as everywhere else, so the tint is linear in x: 0 down
    // the left edge and 1 down the right.
    let bitmap = mesh_page(
        "/ShadingType 4 \
         /ColorSpace [/Separation /Ink /DeviceGray \
           << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 3 >>] \
         /BitsPerCoordinate 8 /BitsPerComponent 8 /BitsPerFlag 8 \
         /Decode [0 60 0 60 0 1]",
        &split_square(),
        "/Sh0 sh",
    );

    // The ends, which both orders agree about.
    assert!(pixel(&bitmap, 2, 30).0 > 250, "tint 0 is white");
    assert!(pixel(&bitmap, 57, 30).0 < 45, "tint 1 is nearly black");

    // The middle, which they do not. At x = 30.5 the tint is 0.508; cubed
    // that is 0.131, so the ink covers an eighth and the grey is about 221.
    let middle = pixel(&bitmap, 30, 30).0;
    assert!(
        (208..=234).contains(&middle),
        "halfway along the tint ramp should be about 221 -- the tint \
         interpolated in the shading's own space and converted after -- but \
         it is {middle}. About 125 means the vertices were converted to RGB \
         first and the results interpolated, which is a different picture in \
         every non-linear space"
    );
}

/// A mesh inside a `PatternType 2` fills a path, which is the route an
/// Illustrator gradient mesh actually arrives by.
#[test]
fn a_mesh_shading_pattern_fills_a_path() {
    let hex = split_square().hex();
    let content = "/Pattern cs /P0 scn 10 10 40 40 re f";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /Pattern << /P0 6 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /ShadingType 4 {GREY_KEYS} /Filter /ASCIIHexDecode\n\
   /Length {} >>\nstream\n{hex}\nendstream\nendobj\n\
6 0 obj\n<< /PatternType 2 /Matrix [1 0 0 1 0 0] /Shading 5 0 R >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1,
        hex.len() + 1,
    );
    let bitmap = render(bytes.into_bytes());

    assert!(
        !bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::UnsupportedPattern { .. })),
        "a mesh pattern is paintable now: {:?}",
        bitmap.warnings
    );

    // Inside the rectangle the mesh shows; outside it the page is untouched.
    assert!(pixel(&bitmap, 15, 30).0 < 90, "the dark end, inside");
    assert!(pixel(&bitmap, 45, 30).0 > 170, "the light end, inside");
    assert_eq!(pixel(&bitmap, 5, 30), (255, 255, 255), "outside the shape");
    assert_eq!(pixel(&bitmap, 55, 30), (255, 255, 255), "and the far side");
}

/// A mesh this build cannot read at all still degrades the way it always did:
/// nothing painted, and a warning naming the type (ruling 2, ruling 10).
#[test]
fn an_unreadable_mesh_warns_with_its_type() {
    // Seven bits per coordinate is not one of the widths Table 79 allows, and
    // the widths decide how the whole stream is cut up -- so there is no
    // reading of these bytes that is even approximately right.
    let bitmap = mesh_page(
        "/ShadingType 6 /ColorSpace /DeviceGray /BitsPerCoordinate 7 \
         /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 60 0 60 0 1]",
        &split_square(),
        "/Sh0 sh",
    );
    assert_eq!(unsupported(&bitmap), Some(6), "the type is named");
    assert_eq!(pixel(&bitmap, 30, 30), (255, 255, 255), "nothing painted");

    // And a mesh written as a direct dictionary, which carries no stream and
    // therefore no vertices at all.
    let content = "/Sh0 sh";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /Shading << /Sh0 << /ShadingType 4 {GREY_KEYS} >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1
    );
    let direct = render(bytes.into_bytes());
    assert_eq!(direct.warnings.len(), 1, "one warning, not a cascade");
    assert_eq!(unsupported(&direct), Some(4), "a mesh with no stream");
}

/// The edge flag decides which two vertices a continuing triangle keeps
/// (8.7.4.5.5). Ignoring it still produces a mesh — a torn one.
#[test]
fn the_edge_flag_decides_where_the_next_triangle_goes() {
    // A strip: three vertices with flag 0, then one with flag 1 that must
    // continue from the *second* and *third* of them. If the flag were
    // ignored and every vertex started a new triangle, or if flag 1 were read
    // as flag 2, the fourth vertex would join a different pair and the strip
    // would fold back on itself.
    let mut stream = Packer::new();
    for (flag, x, y, grey) in [
        (0u64, 0u64, 0u64, 0u64),
        (0, 0, 255, 0),
        (0, 128, 0, 255),
        (1, 255, 255, 255),
    ] {
        stream
            .push(flag, 8)
            .push(x, 8)
            .push(y, 8)
            .push(grey, 8)
            .align();
    }

    let bitmap = mesh_page(&format!("/ShadingType 4 {GREY_KEYS}"), &stream, "/Sh0 sh");
    assert_eq!(unsupported(&bitmap), None);

    // The second triangle is (0, 60) - (30, 0) - (60, 60) in page space, so
    // the top-right region of the *bitmap* — near (50, 12) — is inside it.
    let inside = pixel(&bitmap, 50, 14);
    assert!(
        inside.0 > 180,
        "the continued triangle should cover (50, 14) at its light end, got {inside:?}"
    );
    // And the far corner is outside both triangles, so the page shows through.
    // Both are narrow at page y = 5: triangle one reaches x = 27.5 and
    // triangle two spans 27.5 to 32.5, so nothing covers column 58 there.
    assert_eq!(pixel(&bitmap, 58, 55), (255, 255, 255), "past the strip");
}

// ---------------------------------------------------------------------------
// Coons and tensor patches (8.7.4.5.7, 8.7.4.5.8).
// ---------------------------------------------------------------------------

/// 60 units of page, as the raw 8-bit value `/Decode [0 60 ...]` maps it to.
fn raw(units: f64) -> u64 {
    (units / 60.0 * 255.0).round() as u64
}

/// One patch: its flag, the points it carries in the spiral order 8.7.4.5.7
/// numbers them, and the corner colours it carries.
///
/// A flag of 0 carries all twelve points (sixteen for a tensor patch) and four
/// colours; a flag of 1, 2 or 3 carries eight (or twelve) and two, because the
/// shared edge comes from the previous patch (Table 85).
fn patch(stream: &mut Packer, flag: u64, points: &[(f64, f64)], colors: &[f64]) {
    stream.push(flag, 8);
    for (x, y) in points {
        stream.push(raw(*x), 8).push(raw(*y), 8);
    }
    for grey in colors {
        stream.push((grey * 255.0).round() as u64, 8);
    }
    stream.align();
}

/// The twelve boundary points of an axis-aligned patch with straight edges,
/// evenly spaced, in the spiral order.
///
/// `u` runs left to right and `v` bottom to top, so corner `c1` is at
/// `(x0, y0)`, `c2` at `(x0, y1)`, `c3` at `(x1, y1)` and `c4` at `(x1, y0)`.
fn flat_patch_points(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    let x = |t: f64| x0 + (x1 - x0) * t;
    let y = |t: f64| y0 + (y1 - y0) * t;
    let third = 1.0 / 3.0;
    vec![
        (x(0.0), y(0.0)),         // p1  = p00
        (x(0.0), y(third)),       // p2  = p01
        (x(0.0), y(2.0 * third)), // p3  = p02
        (x(0.0), y(1.0)),         // p4  = p03
        (x(third), y(1.0)),       // p5  = p13
        (x(2.0 * third), y(1.0)), // p6  = p23
        (x(1.0), y(1.0)),         // p7  = p33
        (x(1.0), y(2.0 * third)), // p8  = p32
        (x(1.0), y(third)),       // p9  = p31
        (x(1.0), y(0.0)),         // p10 = p30
        (x(2.0 * third), y(0.0)), // p11 = p20
        (x(third), y(0.0)),       // p12 = p10
    ]
}

const PATCH_KEYS: &str = "/ColorSpace /DeviceGray /BitsPerCoordinate 8 \
     /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 60 0 60 0 1]";

/// **Milestone 4's exit criterion.** A flat Coons patch renders as the
/// equivalent two triangles do.
///
/// The corner colours are chosen so that the two really are equivalent:
/// 8.7.4.5.7 interpolates a patch's four corners *bilinearly* over the unit
/// square, and bilinear is only linear when opposite corners agree along one
/// axis. Here `c1 = c2 = 0` and `c3 = c4 = 1`, so the colour depends on `u`
/// alone and the patch is exactly the ramp the two triangles paint.
///
/// The two routes could hardly be less alike: one is two triangles straight
/// from the stream, the other is a bicubic surface subdivided into a grid of
/// several hundred. Getting the same pixels out of both is what says the
/// subdivision, the Coons interior formula and the corner interpolation all
/// agree with the triangle path they are supposed to be a front end on.
#[test]
fn a_flat_coons_patch_renders_like_the_equivalent_two_triangles() {
    let mut stream = Packer::new();
    patch(
        &mut stream,
        0,
        &flat_patch_points(0.0, 0.0, 60.0, 60.0),
        &[0.0, 0.0, 1.0, 1.0],
    );

    let patch_form = mesh_page(&format!("/ShadingType 6 {PATCH_KEYS}"), &stream, "/Sh0 sh");
    let triangle_form = mesh_page(
        &format!("/ShadingType 4 {GREY_KEYS}"),
        &split_square(),
        "/Sh0 sh",
    );
    assert_eq!(unsupported(&patch_form), None, "type 6 is painted now");

    let mut worst = 0i32;
    for y in 1..59 {
        for x in 1..59 {
            let difference =
                i32::from(pixel(&patch_form, x, y).0) - i32::from(pixel(&triangle_form, x, y).0);
            worst = worst.max(difference.abs());
        }
    }
    assert!(
        worst <= 2,
        "a flat Coons patch and the two triangles it is equal to differ by \
         {worst} levels somewhere inside"
    );
}

/// A patch's edges are cubics, not chords: control points off the chord bulge
/// the boundary, and the bulge is painted.
#[test]
fn a_patch_boundary_is_a_curve_rather_than_a_chord() {
    let mut points = flat_patch_points(10.0, 25.0, 50.0, 55.0);
    // p11 and p12 are the two interior control points of the v = 0 edge, the
    // one running from (10, 25) to (50, 25). Dragging both down to y = 0 bows
    // that edge well below the corners: the cubic's midpoint lands at
    // (25 + 25) / 8 = 6.25.
    points[10] = (points[10].0, 0.0);
    points[11] = (points[11].0, 0.0);

    let mut stream = Packer::new();
    patch(&mut stream, 0, &points, &[0.3, 0.3, 0.3, 0.3]);
    let bitmap = mesh_page(&format!("/ShadingType 6 {PATCH_KEYS}"), &stream, "/Sh0 sh");

    // Page y = 12 is fifteen points below the corners, and only the bulge
    // reaches it. Bitmap rows count from the top, so that is row 48.
    let inside = pixel(&bitmap, 30, 48).0;
    assert!(
        inside < 120,
        "the middle of the bowed edge should be painted, got {inside}"
    );
    // The corners of the same band are not bowed, so the page shows there.
    assert_eq!(pixel(&bitmap, 12, 48), (255, 255, 255), "beside the bulge");
    assert_eq!(
        pixel(&bitmap, 48, 48),
        (255, 255, 255),
        "and the other side"
    );
}

/// A tensor patch's four internal control points are read and used
/// (8.7.4.5.8), rather than being recomputed from the boundary as a Coons
/// patch's are.
///
/// The two types share every byte of their boundary, so a build that ignored
/// the extra four points would paint a type 7 patch as a type 6 one and no
/// test of the outline could tell. The colours are what shows it: dragging the
/// interior points does not move the boundary at all, it moves where inside
/// the patch each parameter lands, so the colour at the centre shifts.
#[test]
fn a_tensor_patch_uses_its_internal_control_points() {
    let boundary = flat_patch_points(0.0, 0.0, 60.0, 60.0);
    let colors = [0.0, 0.0, 1.0, 1.0];

    // The Coons reading of the same boundary: the four internal points that
    // 8.7.4.5.7's formula implies, which for a flat patch is the flat grid.
    let mut flat = Packer::new();
    let mut points = boundary.clone();
    for (u, v) in [(1.0, 1.0), (1.0, 2.0), (2.0, 2.0), (2.0, 1.0)] {
        points.push((u * 20.0, v * 20.0));
    }
    patch(&mut flat, 0, &points, &colors);

    // The same boundary with the interior dragged hard towards one corner.
    let mut pulled = Packer::new();
    let mut points = boundary;
    for (u, v) in [(0.0, 0.0), (0.0, 60.0), (12.0, 60.0), (12.0, 0.0)] {
        points.push((u, v));
    }
    patch(&mut pulled, 0, &points, &colors);

    let flat = mesh_page(&format!("/ShadingType 7 {PATCH_KEYS}"), &flat, "/Sh0 sh");
    let pulled = mesh_page(&format!("/ShadingType 7 {PATCH_KEYS}"), &pulled, "/Sh0 sh");
    assert_eq!(unsupported(&flat), None, "type 7 is painted");
    assert_eq!(unsupported(&pulled), None);

    let (a, b) = (pixel(&flat, 30, 30).0, pixel(&pulled, 30, 30).0);
    assert!(
        i32::from(a).abs_diff(i32::from(b)) > 25,
        "the internal control points should move the centre: {a} against {b}"
    );
}

/// Patches continue across an edge flag exactly as triangles do, and the join
/// leaves no seam (8.7.4.5.7, Table 85).
///
/// Flag 2 makes the new patch's first edge the previous patch's `p7 p8 p9 p10`
/// — its `u = 1` side — and its first two colours the previous patch's `c3`
/// and `c4`. Reading the wrong four points still produces a patch, hinged
/// somewhere else, which is why the assertion is that the ramp is monotone
/// across the join rather than merely that something painted.
#[test]
fn patches_continue_across_an_edge_flag() {
    let mut stream = Packer::new();
    patch(
        &mut stream,
        0,
        &flat_patch_points(0.0, 0.0, 30.0, 60.0),
        &[0.0, 0.0, 0.5, 0.5],
    );
    // The continuation carries eight points: its own p5 to p12. The shared
    // edge arrives from the previous patch, and it arrives *reversed* — the
    // new p1 is the old p7, at (30, 60) — so this patch's v axis runs the
    // other way and its far edge is written top to bottom.
    patch(
        &mut stream,
        2,
        &[
            (40.0, 0.0),
            (50.0, 0.0),
            (60.0, 0.0),
            (60.0, 20.0),
            (60.0, 40.0),
            (60.0, 60.0),
            (50.0, 60.0),
            (40.0, 60.0),
        ],
        &[1.0, 1.0],
    );

    let bitmap = mesh_page(&format!("/ShadingType 6 {PATCH_KEYS}"), &stream, "/Sh0 sh");
    assert_eq!(unsupported(&bitmap), None);

    // One ramp across the whole page, with the join at x = 30.
    assert!(pixel(&bitmap, 2, 30).0 < 20, "the dark end");
    assert!(pixel(&bitmap, 57, 30).0 > 235, "the light end");
    let mut previous = 0u8;
    for x in 2..58 {
        let value = pixel(&bitmap, x, 30).0;
        assert!(
            value + 2 >= previous,
            "the ramp falls back at column {x}: {value} after {previous}"
        );
        previous = value;
    }

    // And nothing at the join is lighter than the page's own ramp, which is
    // what a seam between two patches would be.
    for y in 3..57 {
        for x in 28..33 {
            let value = pixel(&bitmap, x, y).0;
            assert!(
                (100..=160).contains(&value),
                "({x}, {y}) is {value}, which is a seam at the patch join"
            );
        }
    }
}

/// A page mixing triangles and patches paints both.
///
/// This is the half-implementation the plan calls worse than none: a gradient
/// mesh exports as patches while its background exports as triangles, so a
/// build with types 4 and 5 and not 6 and 7 renders half such a page and warns
/// about the other half, which reads as a corrupt file rather than as a
/// partial capability.
#[test]
fn a_page_mixing_triangles_and_patches_paints_both() {
    let triangles = split_square().hex();
    let mut stream = Packer::new();
    patch(
        &mut stream,
        0,
        &flat_patch_points(20.0, 20.0, 40.0, 40.0),
        &[1.0, 1.0, 1.0, 1.0],
    );
    let patches = stream.hex();
    let content = "/Sh0 sh q 20 20 20 20 re W n /Sh1 sh Q";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << /Shading << /Sh0 5 0 R /Sh1 6 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /ShadingType 4 {GREY_KEYS} /Filter /ASCIIHexDecode\n\
   /Length {} >>\nstream\n{triangles}\nendstream\nendobj\n\
6 0 obj\n<< /ShadingType 6 {PATCH_KEYS} /Filter /ASCIIHexDecode\n\
   /Length {} >>\nstream\n{patches}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1,
        triangles.len() + 1,
        patches.len() + 1,
    );
    let bitmap = render(bytes.into_bytes());

    assert!(
        bitmap.warnings.is_empty(),
        "neither half is missing: {:?}",
        bitmap.warnings
    );
    // The triangle mesh shows outside the patch's box.
    assert!(pixel(&bitmap, 5, 30).0 < 30, "the triangle background");
    // The white patch covers the middle, over a background that is mid grey
    // there.
    assert!(pixel(&bitmap, 30, 30).0 > 250, "the patch on top of it");
}

/// `/Decode` says what the packed integers mean, and reversing a range
/// reverses the axis. A reader that ignores it draws a mesh in the wrong
/// place, which reads as a transform bug rather than as a decoding one.
#[test]
fn the_decode_ranges_place_the_mesh() {
    let forward = mesh_page(
        &format!("/ShadingType 4 {GREY_KEYS}"),
        &split_square(),
        "/Sh0 sh",
    );
    let reversed = mesh_page(
        "/ShadingType 4 /ColorSpace /DeviceGray /BitsPerCoordinate 8 \
         /BitsPerComponent 8 /BitsPerFlag 8 /Decode [60 0 0 60 0 1]",
        &split_square(),
        "/Sh0 sh",
    );

    // The same mesh, mirrored: what was dark on the left is dark on the right.
    assert!(pixel(&forward, 2, 30).0 < 20);
    assert!(pixel(&reversed, 2, 30).0 > 235, "the x range is reversed");
    assert!(pixel(&reversed, 57, 30).0 < 20);
}
