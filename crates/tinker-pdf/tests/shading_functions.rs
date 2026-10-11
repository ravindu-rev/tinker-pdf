//! A shading's one `/Function`, when it is a **stream**.
//!
//! 7.10.2's sampled function and 7.10.5's calculator are streams, and a
//! shading names its function by reference. This reader used to resolve the
//! `/Function` entry before handing it on, and the parser for those two types
//! reaches the stream through its reference — so it found none, read nothing,
//! and the shading painted as `Function::Identity`: the parameter itself, as a
//! grey ramp, with no warning. A function stated *inline* as a dictionary (type
//! 2 or 3) and one inside an array were unaffected, which is every shading the
//! suite had.
//!
//! Every expected colour here is the function's own value at the sampled
//! parameter, worked out beside the assertion.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | the single `/Function` is resolved before it is parsed (the old reader) | 2 |
//! | a two-input sampled table is read along its first input alone (the old reader) | 1 |
//! | the table's last input varies fastest | 1 |

use tinker_pdf::{Document, RenderOptions};

/// A one-page document, 100 by 100 points, whose page shades its whole box
/// with `/Sh0` — object 5 — whose function is object 6, written as `function`.
fn pdf(shading: &str, function: &[u8]) -> Vec<u8> {
    let content = "/Sh0 sh";
    let objects: [Vec<u8>; 6] = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
          /Resources << /Shading << /Sh0 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        shading.as_bytes().to_vec(),
        function.to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
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

/// A stream object of `dict` entries around `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// The colour at a fraction of the page's width, on its middle row.
fn colour_at(bytes: Vec<u8>, x: f64) -> [u8; 3] {
    let doc = Document::open(bytes).expect("it opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let width = bitmap.width as usize;
    let row = bitmap.height as usize / 2;
    let column = ((width as f64 * x) as usize).min(width - 1);
    let at = (row * width + column) * components;
    [bitmap.data[at], bitmap.data[at + 1], bitmap.data[at + 2]]
}

fn near(got: [u8; 3], want: [u8; 3], what: &str) {
    assert!(
        got.iter()
            .zip(want)
            .all(|(g, w)| (i32::from(*g) - i32::from(w)).abs() <= 3),
        "{what}: {got:?}, not {want:?}"
    );
}

const AXIAL: &str = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0] \
                     /Extend [true true] /Function 6 0 R >>";

/// 7.10.5's calculator: `{ dup 0 exch 1 exch sub }` takes t to (t, 0, 1 − t),
/// so a quarter of the way across is (0.25, 0, 0.75) — `(64, 0, 191)` — and
/// not the grey `(64, 64, 64)` the identity paints.
#[test]
fn a_calculator_function_shades_its_own_colours() {
    let function = stream(
        "/FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1]",
        b"{ dup 0 exch 1 exch sub }",
    );
    near(
        colour_at(pdf(AXIAL, &function), 0.25),
        [64, 0, 191],
        "a quarter across",
    );
}

/// **A two-input sampled function is read across both inputs.** A type 1
/// shading hands its function `(x, y)`, and 2 x 2 samples — red at (0, 0),
/// green at (1, 0), blue at (0, 1), white at (1, 1), the first input varying
/// fastest as 7.10.2 lays a table out — blend bilinearly: on the middle row a
/// quarter across is `(128, 64, 128)`. A reader that read along `x` alone
/// paints the bottom edge's `(191, 64, 0)` at every height.
#[test]
fn a_two_input_sampled_function_shades_across_both_inputs() {
    let function = stream(
        "/FunctionType 0 /Domain [0 100 0 100] /Range [0 1 0 1 0 1] /Size [2 2] /BitsPerSample 8",
        &[0xFF, 0, 0, 0, 0xFF, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF],
    );
    // Over the page's own 100 x 100 rather than through a `/Matrix`, which
    // this reader's type 1 shading does not read.
    let shading = "<< /ShadingType 1 /ColorSpace /DeviceRGB /Domain [0 100 0 100] \
                   /Function 6 0 R >>";
    near(
        colour_at(pdf(shading, &function), 0.25),
        [128, 64, 128],
        "a quarter across, halfway up",
    );
}

/// 7.10.2's sampled function, two samples of eight-bit RGB — red, then blue —
/// interpolated: a quarter of the way across is three quarters red and a
/// quarter blue, `(191, 0, 64)`.
#[test]
fn a_sampled_function_shades_its_own_colours() {
    let function = stream(
        "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8",
        &[0xFF, 0, 0, 0, 0, 0xFF],
    );
    near(
        colour_at(pdf(AXIAL, &function), 0.25),
        [191, 0, 64],
        "a quarter across",
    );
}
