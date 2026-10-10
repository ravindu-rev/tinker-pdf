//! A resource name the builder writes into a content stream is the name its
//! `/Resources` holds, whatever bytes the caller chose.
//!
//! 7.3.5 makes a delimiter, white space, `#` and every byte outside `!`..`~` a
//! `#xx` escape inside a name. The dictionary side of the builder always went
//! through the writer's escaper; the content-stream side wrote the bytes raw,
//! so `page.form(b"Fm B")` wrote `/Fm B Do` — the name `/Fm` and a stray
//! operand — and the page drew nothing while its `/Resources` held the form
//! under the whole name.
//!
//! Every operator that takes a resource name gets its own test here, and each
//! asserts the same three things about one awkward name: the stream carries
//! the escaped token, the page's resource dictionary carries the name's own
//! bytes, and the page **draws** exactly what the same document draws under a
//! plain name. The last is the one that matters: a stream and a dictionary
//! that are each escaped by a different rule would pass the first two.

use tinker_pdf::{DeviceSpace, Object};
use tinker_pdf::{
    Document, DocumentBuilder, ExtGState, FormXObject, Function, Glyph, PlacedGlyph, Shading,
    TilingPattern, TilingType,
};

mod pdfa_support;
mod render_support;
use render_support::{curvy_font, ink, render, same_picture};

/// The awkward name: a space, a `#`, a `/` and a byte past 0x7F.
const ODD: &[u8] = b"R a#/\xE9";
/// How 7.3.5 spells it.
const ODD_TOKEN: &str = "/R#20a#23#2F#E9";
/// A name that needs nothing escaped, for the picture to be compared against.
const PLAIN: &[u8] = b"R0";

/// The first page's content stream, decoded.
fn content(bytes: &[u8]) -> String {
    let doc = Document::open(bytes.to_vec()).expect("the document opens");
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// Whether the first page's `/Resources /<category>` has an entry under
/// exactly these bytes.
fn resource_named(bytes: &[u8], category: &[u8], name: &[u8]) -> bool {
    let doc = Document::open(bytes.to_vec()).expect("the document opens");
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    let Some(resources) = page.resources.as_ref() else {
        return false;
    };
    let table = cos.resolve_key(resources, cos.intern(category));
    matches!(
        table.as_dict().and_then(|d| d.get(cos.intern(name))),
        Some(Object::Ref(_) | Object::Dict(_) | Object::Array(_))
    )
}

/// Builds one document under [`ODD`] and one under [`PLAIN`] and holds the
/// first to the three claims in the module header.
///
/// `least` is the ink floor: a pair of pages that both draw nothing are the
/// same picture, which is exactly the failure this file exists to catch.
#[track_caller]
fn draws_under_either_name(
    category: &[u8],
    least: usize,
    build: impl Fn(&[u8]) -> Vec<u8>,
    what: &str,
) {
    let odd = build(ODD);
    let plain = build(PLAIN);

    let stream = content(&odd);
    assert!(
        stream.contains(ODD_TOKEN),
        "{what}: the stream names the resource with 7.3.5's escapes: {stream}"
    );
    assert!(
        resource_named(&odd, category, ODD),
        "{what}: /Resources holds the resource under the name's own bytes"
    );

    let drawn = render(odd);
    let painted = ink(&drawn);
    assert!(
        painted >= least,
        "{what}: the page painted {painted} pixels, fewer than {least}"
    );
    same_picture(drawn, render(plain), what);
}

// ---- Tf ------------------------------------------------------------------

#[test]
fn a_font_name_is_escaped_in_text() {
    draws_under_either_name(
        b"Font",
        40,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_embedded_font(name, b"Curvy", &curvy_font()));
            builder.add_page(60.0, 60.0, |page| {
                page.text(name, 20.0, 5.0, 20.0, "ABC");
            });
            builder.finish()
        },
        "Tf, through text",
    );
}

#[test]
fn a_font_name_is_escaped_in_encoded_text() {
    draws_under_either_name(
        b"Font",
        40,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_embedded_font(name, b"Curvy", &curvy_font()));
            builder.add_page(60.0, 60.0, |page| {
                page.encoded_text(name, 20.0, 5.0, 20.0, (0.0, 0.0), b"ABC", "ABC");
            });
            builder.finish()
        },
        "Tf, through encoded_text",
    );
}

#[test]
fn a_font_name_is_escaped_in_glyphs() {
    draws_under_either_name(
        b"Font",
        40,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_cid_font(name, b"Curvy", &curvy_font()));
            builder.add_page(60.0, 60.0, |page| {
                assert!(page.glyphs(
                    name,
                    20.0,
                    5.0,
                    20.0,
                    &[Glyph { id: 1, text: "A" }, Glyph { id: 2, text: "B" }],
                ));
            });
            builder.finish()
        },
        "Tf, through glyphs",
    );
}

/// `glyph_run` writes into a stream the caller owns, so it reaches a page
/// through `raw` — and escapes by the same rule, or the run names a font the
/// page's dictionary spells differently.
#[test]
fn a_font_name_is_escaped_in_a_glyph_run() {
    draws_under_either_name(
        b"Font",
        40,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_cid_font(name, b"Curvy", &curvy_font()));
            let mut run = Vec::new();
            assert!(builder.glyph_run(
                &mut run,
                name,
                20.0,
                [1.0, 0.0, 0.0, 1.0, 5.0, 20.0],
                &[
                    PlacedGlyph {
                        glyph: Glyph { id: 1, text: "A" },
                        x: 0.0,
                        rise: 0.0,
                    },
                    PlacedGlyph {
                        glyph: Glyph { id: 2, text: "B" },
                        x: 0.7,
                        rise: 0.0,
                    },
                ],
            ));
            builder.add_page(60.0, 60.0, |page| page.raw(&run));
            builder.finish()
        },
        "Tf, through glyph_run",
    );
}

// ---- Do ------------------------------------------------------------------

#[test]
fn an_image_name_is_escaped_in_do() {
    draws_under_either_name(
        b"XObject",
        400,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_image(
                name,
                &tinker_pdf::ImageData::Gray8 {
                    width: 2,
                    height: 2,
                    data: &[0, 80, 160, 40],
                },
            ));
            builder.add_page(60.0, 60.0, |page| page.image(name, 10.0, 10.0, 40.0, 40.0));
            builder.finish()
        },
        "Do, through image",
    );
}

#[test]
fn a_form_name_is_escaped_in_do() {
    draws_under_either_name(
        b"XObject",
        400,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_form(
                name,
                &FormXObject {
                    bbox: [0.0, 0.0, 60.0, 60.0],
                    matrix: None,
                    group: None,
                    content: b"0 0 1 rg 10 10 40 40 re f",
                },
            ));
            builder.add_page(60.0, 60.0, |page| assert!(page.form(name)));
            builder.finish()
        },
        "Do, through form",
    );
}

// ---- gs ------------------------------------------------------------------

#[test]
fn a_graphics_state_name_is_escaped_in_gs() {
    draws_under_either_name(
        b"ExtGState",
        400,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_ext_gstate(
                name,
                &ExtGState {
                    fill_alpha: Some(0.25),
                    ..ExtGState::default()
                },
            ));
            builder.add_page(60.0, 60.0, |page| {
                page.set_fill_rgb(0.0, 0.0, 0.0);
                assert!(page.set_ext_gstate(name));
                page.raw(b"10 10 40 40 re f");
            });
            builder.finish()
        },
        "gs",
    );
}

// ---- sh ------------------------------------------------------------------

#[test]
fn a_shading_name_is_escaped_in_sh() {
    draws_under_either_name(
        b"Shading",
        400,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_shading(
                name,
                &Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [10.0, 0.0, 50.0, 0.0],
                    function: Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![1.0, 0.0, 0.0],
                        c1: vec![0.0, 0.0, 1.0],
                        n: 1.0,
                    },
                    extend: (false, false),
                },
            ));
            builder.add_page(60.0, 60.0, |page| {
                page.raw(b"10 10 40 40 re W n");
                assert!(page.shading(name));
            });
            builder.finish()
        },
        "sh",
    );
}

// ---- scn and SCN, through a pattern ---------------------------------------

fn tiling() -> TilingPattern<'static> {
    TilingPattern {
        bbox: [0.0, 0.0, 10.0, 10.0],
        x_step: 10.0,
        y_step: 10.0,
        matrix: None,
        tiling_type: TilingType::ConstantSpacing,
        content: b"1 0 0 rg 0 0 5 5 re f",
    }
}

#[test]
fn a_pattern_name_is_escaped_in_scn() {
    draws_under_either_name(
        b"Pattern",
        100,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_tiling_pattern(name, &tiling()));
            builder.add_page(60.0, 60.0, |page| {
                assert!(page.set_fill_pattern(name));
                page.raw(b"10 10 40 40 re f");
            });
            builder.finish()
        },
        "scn, through a pattern",
    );
}

#[test]
fn a_pattern_name_is_escaped_in_stroking_scn() {
    draws_under_either_name(
        b"Pattern",
        100,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_tiling_pattern(name, &tiling()));
            builder.add_page(60.0, 60.0, |page| {
                assert!(page.set_stroke_pattern(name));
                page.raw(b"10 w 10 30 m 50 30 l S");
            });
            builder.finish()
        },
        "SCN, through a pattern",
    );
}

// ---- cs and CS, through /ICCBased ------------------------------------------

#[test]
fn a_colour_space_name_is_escaped_in_cs() {
    draws_under_either_name(
        b"ColorSpace",
        400,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_icc_color_space(name, &pdfa_support::srgb_like(), 3));
            builder.add_page(60.0, 60.0, |page| {
                assert!(page.set_fill_icc(name, &[1.0, 0.0, 0.0]));
                page.raw(b"10 10 40 40 re f");
            });
            builder.finish()
        },
        "cs, through an ICC space",
    );
}

#[test]
fn a_colour_space_name_is_escaped_in_stroking_cs() {
    draws_under_either_name(
        b"ColorSpace",
        100,
        |name| {
            let mut builder = DocumentBuilder::new();
            assert!(builder.add_icc_color_space(name, &pdfa_support::srgb_like(), 3));
            builder.add_page(60.0, 60.0, |page| {
                assert!(page.set_stroke_icc(name, &[1.0, 0.0, 0.0]));
                page.raw(b"10 w 10 30 m 50 30 l S");
            });
            builder.finish()
        },
        "CS, through an ICC space",
    );
}
