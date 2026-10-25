//! Every input the benchmark set times, and nothing that times it.
//!
//! This file holds no criterion type, no clock and no spawn, so a test can
//! reach a generator by path (`#[path = "../benches/support/inputs.rs"]`)
//! without compiling the harness. Every input is written here, in memory, or
//! read by path from what the tree already commits; a run needs no corpus.
//!
//! The four below are the seven operations' inputs, moved out of the single
//! `benches/engine.rs` they were written in with their bodies unchanged, so
//! each of the seven still times the bytes it timed there.

use tinker_pdf::{DeviceSpace, DocumentBuilder, Function, Shading};

/// Three A4 pages of forty lines each in standard-14 Helvetica, unembedded.
///
/// The seven's text page, and the document their open and rewrite take. No
/// glyph of it is drawn in the default build: Helvetica names no program, so
/// the renderer reports `UnreadableFont` and paints nothing (see
/// [`triangles`]).
pub fn three_pages() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for page in 0..3 {
        builder.add_page(595.0, 842.0, |p| {
            for line in 0..40 {
                p.text(
                    b"F0",
                    11.0,
                    72.0,
                    780.0 - f64::from(line) * 16.0,
                    &format!("Page {page}, line {line}: the quick brown fox jumps over it."),
                );
            }
        });
    }
    builder.finish()
}

/// One page carrying an axial shading over its whole area.
pub fn axial() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    let shading = Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: [0.0, 0.0, 595.0, 842.0],
        function: Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0, 0.1, 0.6],
            c1: vec![1.0, 0.9, 0.2],
            n: 1.0,
        },
        extend: (true, true),
    };
    builder.add_shading(b"S0", &shading);
    builder.add_page(595.0, 842.0, |page| {
        page.shading(b"S0");
    });
    builder.finish()
}

/// Three hundred filled paths with edges on no axis, over a whole page.
///
/// **The only one of the seven that spends its time in the scanline
/// rasteriser, and it was added because the roadmap's vectorisation row could
/// not be measured without one.** That row's exit criterion said "a measured
/// speedup on the six benchmarks", and of the six: four never enter
/// `tinker-pdf-raster` at all, the shading is its own sampler, and *"render
/// text at 150 dpi" draws no glyphs* — its fixture names base-14 Helvetica,
/// embeds no program, `bundled-fonts` is off by default, so the renderer
/// pushes `UnreadableFont` and paints nothing. A speedup in the span filler
/// would have moved none of them.
///
/// Triangles rather than rectangles, and at fractional coordinates, because
/// the loops in question are the ones that accumulate *partial* coverage: an
/// axis-aligned rectangle on integers is the one shape that costs a span
/// filler nothing to anti-alias.
pub fn triangles() -> Vec<u8> {
    let mut content = String::with_capacity(24 * 1024);
    for index in 0..300 {
        let step = f64::from(index);
        let x = 20.0 + (step * 37.0) % 520.0;
        let y = 20.0 + (step * 61.0) % 760.0;
        let size = 18.0 + (step * 7.0) % 44.0;
        let shade = 0.15 + (step % 17.0) / 20.0;
        content.push_str(&format!(
            "{shade:.3} {:.3} {:.3} rg {x:.2} {y:.2} m {:.2} {:.2} l {:.2} {:.2} l h f\n",
            1.0 - shade,
            (step % 11.0) / 11.0,
            x + size * 0.93,
            y + size * 0.37,
            x + size * 0.41,
            y + size,
        ));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_page(595.0, 842.0, |page| page.raw(content.as_bytes()));
    builder.finish()
}

/// One of the books committed under `crates/tinker-pdf/tests/epub/`, by its
/// file name.
pub fn committed_book(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/epub/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed book {path}: {error}"))
}
