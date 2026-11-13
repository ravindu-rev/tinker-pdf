//! What painting a shading costs, counted rather than timed (P-14).
//!
//! A shading is evaluated once a pixel, so anything its sweep allocates for a
//! pixel is allocated a million times over a page. `painting_a_shading_*` and
//! the tests beside it render each page at two sizes, the second four times the
//! pixels of the first, and hold the allocations of the two renders equal: a
//! sweep that allocated anything per pixel would allocate thousands more at the
//! second. Every route a shading is painted by is among them — `sh`, a shading
//! pattern's fill and a mesh's colour pass — and so is every function type and
//! every colour space that evaluates a function of its own, since each
//! evaluates through its own code.
//!
//! No tally sees an allocation, so this binary installs MS-01's A10 counter,
//! the guard suite's counting allocator (`guard_support::Counting`), which
//! counts each allocation against the thread that made it: each test renders
//! on its own thread, serially, as `RenderOptions::default()` asks.
//! `#![forbid(unsafe_code)]` binds the library this tests and is untouched; the
//! allocator forwards every call to `System` and reads no document byte.
//!
//! Each page is rendered once at a third size before anything is counted, so a
//! table the first render of a process builds is not counted against the
//! smaller page.

mod guard_support;
mod render_support;

use guard_support::{allocations_of, object, pdf, stream, Counting};
use tinker_pdf::{Document, RenderOptions};

#[global_allocator]
static COUNTING: Counting = Counting;

/// The page's side, in points and so in pixels at 72 dpi: the warm-up, then
/// the two that are counted.
const WARM: f64 = 50.0;
const SIDES: [f64; 2] = [100.0, 200.0];

/// A page `side` points square, its resources `resources`, its content
/// `content`, and `objects` after the first four, numbered from 5.
fn page(side: f64, resources: &str, content: &str, objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut all = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {side} {side}] /Resources {resources} \
             /Contents 4 0 R >>"
        )),
        stream("", content.as_bytes()),
    ];
    all.extend(objects);
    pdf(&all)
}

/// A page painted by `sh` with the shading at object 5, whose other objects
/// follow it.
fn flooded(side: f64, objects: Vec<Vec<u8>>) -> Vec<u8> {
    page(side, "<< /Shading << /Sh0 5 0 R >> >>", "/Sh0 sh", objects)
}

/// A page whose one path, the whole page, is filled with the shading pattern
/// at object 5, the shading it names after it.
fn patterned(side: f64, objects: Vec<Vec<u8>>) -> Vec<u8> {
    page(
        side,
        "<< /Pattern << /P0 5 0 R >> >>",
        &format!("/Pattern cs /P0 scn 0 0 {side} {side} re f"),
        objects,
    )
}

/// A type 2 function, `c0` to `c1` along `t^n`.
fn exponential(c0: &str, c1: &str, n: f64) -> Vec<u8> {
    object(&format!(
        "<< /FunctionType 2 /Domain [0 1] /C0 [{c0}] /C1 [{c1}] /N {n} >>"
    ))
}

/// What one render of `bytes` allocates on this thread, the document opened
/// and its page found outside the count.
fn render_allocations(bytes: Vec<u8>) -> u64 {
    let document = Document::open(bytes).expect("the document opens");
    let page = document.page(0).expect("a page");
    allocations_of(|| drop(page.render(&RenderOptions::default())))
}

/// Holds `page`'s render to the same allocations at both of [`SIDES`], after
/// one render at [`WARM`], and says what it measured.
#[track_caller]
fn flat(name: &str, page: fn(f64) -> Vec<u8>) {
    let warm = render_allocations(page(WARM));
    assert!(
        warm > 0,
        "{name}: a render that allocates nothing measured nothing"
    );
    let [small, large] = SIDES.map(|side| render_allocations(page(side)));
    assert_eq!(
        small, large,
        "{name}: {small} allocations at {} points square and {large} at {}, four times \
         the pixels: the sweep allocates per pixel",
        SIDES[0], SIDES[1]
    );
    println!("{name}: {small} allocations at both sizes");
}

// ---- the pages ---------------------------------------------------------------

/// An axial shading over the whole page in DeviceRGB, from a type 2 function:
/// the bench's full-page shading.
fn axial(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 {side} {side}] \
                 /Function 6 0 R /Extend [true true] >>"
            )),
            exponential("0 0.1 0.6", "1 0.9 0.2", 1.0),
        ],
    )
}

/// A radial shading over the whole page, from a type 3 function stitching two
/// type 2 functions whose exponents are not 1, so each pixel takes a `pow`.
fn radial(side: f64) -> Vec<u8> {
    let centre = side / 2.0;
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [{centre} {centre} {} \
                 {centre} {centre} {centre}] /Function 6 0 R /Extend [true true] >>",
                side / 10.0
            )),
            object(
                "<< /FunctionType 3 /Domain [0 1] /Functions [7 0 R 8 0 R] /Bounds [0.4] \
                 /Encode [0 1 0 1] >>",
            ),
            exponential("1 1 1", "0.9 0.2 0.1", 2.2),
            exponential("0.9 0.2 0.1", "0 0 0.3", 0.5),
        ],
    )
}

/// An axial shading in DeviceGray from a one-input type 0 table.
fn sampled(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 2 /ColorSpace /DeviceGray /Coords [0 {side} {side} 0] \
                 /Function 6 0 R /Extend [true true] >>"
            )),
            stream(
                "/FunctionType 0 /Domain [0 1] /Range [0 1] /Size [5] /BitsPerSample 8",
                &[0, 60, 130, 200, 255],
            ),
        ],
    )
}

/// An axial shading from a type 4 program with a procedure and an `if`, whose
/// operand stack is the evaluation's own memory.
fn calculator(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 {side} 0] \
                 /Function 6 0 R /Extend [true true] >>"
            )),
            stream(
                "/FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1]",
                b"{ dup 0.5 gt { 1 exch sub } if dup 0.5 mul exch dup }",
            ),
        ],
    )
}

/// An axial shading in a `/Separation` space, whose tint transform is
/// evaluated at every pixel too.
fn spot(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 2 /ColorSpace [/Separation /Spot /DeviceCMYK 7 0 R] \
                 /Coords [0 0 0 {side}] /Function 6 0 R /Extend [true true] >>"
            )),
            exponential("0", "1", 1.0),
            exponential("0 0 0 0", "0.1 0.7 0.2 0.05", 1.0),
        ],
    )
}

/// A radial shading in a two-ink `/DeviceN` space, its `/Function` an array of
/// two type 2 functions, one an ink, and its tint transform a type 4 program
/// that copies and rolls its stack.
fn two_inks(side: f64) -> Vec<u8> {
    let centre = side / 2.0;
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 3 /ColorSpace [/DeviceN [/A /B] /DeviceRGB 8 0 R] \
                 /Coords [{centre} {centre} 0 {centre} {centre} {side}] \
                 /Function [6 0 R 7 0 R] /Extend [true true] >>"
            )),
            exponential("0", "1", 1.0),
            exponential("1", "0.25", 1.8),
            stream(
                "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1]",
                b"{ 2 copy add 0.5 mul 3 1 roll }",
            ),
        ],
    )
}

/// A function-based shading over the whole page from a two-input type 0
/// table, read across both inputs.
fn function_based(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            object(&format!(
                "<< /ShadingType 1 /ColorSpace /DeviceGray /Domain [0 {side} 0 {side}] \
                 /Function 6 0 R >>"
            )),
            stream(
                &format!(
                    "/FunctionType 0 /Domain [0 {side} 0 {side}] /Range [0 1] /Size [2 2] \
                     /BitsPerSample 8"
                ),
                &[0, 255, 128, 64],
            ),
        ],
    )
}

/// The radial shading again, painted as a shading pattern filling a path.
fn radial_pattern(side: f64) -> Vec<u8> {
    let centre = side / 2.0;
    patterned(
        side,
        vec![
            object("<< /Type /Pattern /PatternType 2 /Shading 6 0 R >>"),
            object(&format!(
                "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [{centre} {centre} 0 \
                 {centre} {centre} {centre}] /Function 7 0 R /Extend [true true] >>"
            )),
            exponential("1 1 0.8", "0.1 0.3 0.6", 1.0),
        ],
    )
}

/// The axial shading in its `/Separation` space, painted as a shading
/// pattern.
fn spot_pattern(side: f64) -> Vec<u8> {
    patterned(
        side,
        vec![
            object("<< /Type /Pattern /PatternType 2 /Shading 6 0 R >>"),
            object(&format!(
                "<< /ShadingType 2 /ColorSpace [/Separation /Spot /DeviceCMYK 8 0 R] \
                 /Coords [0 0 {side} {side}] /Function 7 0 R /Extend [true true] >>"
            )),
            exponential("0", "1", 1.0),
            exponential("0 0 0 0", "0.1 0.7 0.2 0.05", 2.2),
        ],
    )
}

/// The two triangles that cover a page `side` points square, as a type 4
/// free-form mesh: a flag byte, two 16-bit coordinates across the whole
/// `/Decode` range, and `colour`'s bytes for each vertex.
fn triangles(colour: impl Fn(usize) -> Vec<u8>) -> Vec<u8> {
    let corners = [
        (0u16, 0u16),
        (u16::MAX, 0),
        (u16::MAX, u16::MAX),
        (0, 0),
        (u16::MAX, u16::MAX),
        (0, u16::MAX),
    ];
    let mut data = Vec::new();
    for (vertex, (x, y)) in corners.iter().enumerate() {
        data.push(0);
        data.extend_from_slice(&x.to_be_bytes());
        data.extend_from_slice(&y.to_be_bytes());
        data.extend(colour(vertex));
    }
    data
}

/// A mesh over the whole page whose vertices carry one parametric value into
/// a type 2 function, which its colour pass evaluates at every pixel.
fn mesh_function(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            stream(
                &format!(
                    "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 \
                     /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 {side} 0 {side} 0 1] \
                     /Function 6 0 R"
                ),
                &triangles(|vertex| vec![[0, 128, 255, 0, 255, 160][vertex]]),
            ),
            exponential("0 0.1 0.6", "1 0.9 0.2", 2.2),
        ],
    )
}

/// The same mesh carrying two inks of a `/DeviceN` space at every vertex, no
/// function between: the colour pass converts through the tint transform.
fn mesh_inks(side: f64) -> Vec<u8> {
    flooded(
        side,
        vec![
            stream(
                &format!(
                    "/ShadingType 4 /ColorSpace [/DeviceN [/A /B] /DeviceRGB 6 0 R] \
                     /BitsPerCoordinate 16 /BitsPerComponent 8 /BitsPerFlag 8 \
                     /Decode [0 {side} 0 {side} 0 1 0 1]"
                ),
                &triangles(|vertex| {
                    vec![[0, 255, 128, 0, 128, 64][vertex], 255 - 40 * vertex as u8]
                }),
            ),
            stream(
                "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1]",
                b"{ 2 copy add 0.5 mul 3 1 roll }",
            ),
        ],
    )
}

// ---- the tests ---------------------------------------------------------------

/// **P-14.1's exit.** A full-page axial and a full-page radial shading, each
/// painted by `sh`, allocate the same at a page four times the pixels.
#[test]
fn painting_a_shading_allocates_nothing_per_pixel() {
    flat("an axial shading", axial);
    flat("a radial shading", radial);
}

/// The same for every function type a shading carries — a table of one input
/// and of two, a calculator, a stitch of exponentials — and for the spaces
/// whose tint transform runs at every pixel.
#[test]
fn every_function_type_and_tinted_space_allocates_nothing_per_pixel() {
    flat("a type 0 table", sampled);
    flat("a type 4 program", calculator);
    flat("a /Separation space", spot);
    flat("a two-ink /DeviceN space from a function array", two_inks);
    flat(
        "a function-based shading of a two-input table",
        function_based,
    );
}

/// The same for a shading painted as a shading pattern's fill, which sweeps
/// the path's rectangle rather than the page.
#[test]
fn a_shading_pattern_allocates_nothing_per_pixel() {
    flat("a radial shading pattern", radial_pattern);
    flat(
        "an axial shading pattern in a /Separation space",
        spot_pattern,
    );
}

/// The same for a mesh's colour pass, which converts each pixel's
/// interpolated inputs: through a function, and through a tint transform.
#[test]
fn a_mesh_s_colour_pass_allocates_nothing_per_pixel() {
    flat("a mesh through a function", mesh_function);
    flat("a mesh of two inks", mesh_inks);
}
