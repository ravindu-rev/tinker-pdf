//! Colour glyphs written into a PDF (`docs/design/colour-fonts.md`).
//!
//! `DocumentBuilder` writes a colour face as Type 3 fonts whose glyph
//! procedures the caller draws, and a run writer that places them where the
//! caller says and records each glyph's text; `tinker_pdf::colour_glyphs`
//! turns a face's colour tables into those procedures, and
//! `shaping::write_run` writes a run in a colour face through it. Every
//! assertion reads the written file back through this engine — its
//! structure, its text, where its glyphs stand and the pixels they paint —
//! against numbers the test computes from what it wrote: a layer's colour
//! from the palette and simple alpha compositing, a transformed shape's
//! place from the matrix (ruling 13). The faces are synthesised
//! (`epub_support::typeface`), their colour tables written here field by
//! field from the OpenType chapters' layouts.

mod epub_support;
mod pdfa_support;

use epub_support::typeface::{Colour, Face};
use tinker_pdf::colour_glyphs::{ColourFace, ColourWarning};
use tinker_pdf::shaping::{write_run, Run};
use tinker_pdf::{Bitmap, Document, FontKind, RenderOptions};
use tinker_pdf_cos::build::{ColourGlyph, DocumentBuilder, Glyph, PlacedGlyph};
use tinker_pdf_cos::{CosDocument, Dict, ObjRef, Object};
use tinker_pdf_font::{PaletteChoice, Sfnt};
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_shape::Shaper;

/// The face's em: a TrueType face's usual 2048, so each glyph's form maps its
/// units into the font's glyph space by a `/Matrix` that is not the identity.
const EM: u16 = 2048;

/// The render's pixels per point.
const SCALE: f64 = 2.0;

/// `r g b rg x y w h re f`: a rectangle `[x0 y0 x1 y1]` in font units, filled
/// in a colour given in eight-bit components.
fn rectangle(colour: [u8; 3], rect: [f64; 4]) -> Vec<u8> {
    let [r, g, b] = colour.map(|c| f64::from(c) / 255.0);
    format!(
        "{r} {g} {b} rg {} {} {} {} re f\n",
        rect[0],
        rect[1],
        rect[2] - rect[0],
        rect[3] - rect[1]
    )
    .into_bytes()
}

/// One glyph to register: its id, its colour and its rectangle in font units,
/// a full em wide.
struct Rect {
    id: u16,
    colour: [u8; 3],
    rect: [f64; 4],
}

impl Rect {
    fn register(&self, builder: &mut DocumentBuilder, font: &[u8]) {
        let content = rectangle(self.colour, self.rect);
        assert!(
            builder.add_colour_glyph(
                font,
                &ColourGlyph {
                    id: self.id,
                    advance: f64::from(EM),
                    bbox: self.rect,
                    content: &content,
                },
            ),
            "glyph {} registers",
            self.id
        );
    }
}

/// Glyph 1, a red rectangle from a tenth of the em to eight tenths and up
/// seven; glyph 2, a green one inset a fifth and a tenth; glyph 3, a blue
/// square filling the em.
const RECTS: [Rect; 3] = [
    Rect {
        id: 1,
        colour: [255, 0, 0],
        rect: [205.0, 0.0, 1638.0, 1434.0],
    },
    Rect {
        id: 2,
        colour: [0, 255, 0],
        rect: [410.0, 205.0, 1434.0, 1229.0],
    },
    Rect {
        id: 3,
        colour: [0, 0, 255],
        rect: [0.0, 0.0, 2048.0, 2048.0],
    },
];

/// Where a font unit lands on the page: a glyph placed at `x` along a run at
/// `size` points whose matrix puts its origin at `origin`.
fn on_page(origin: (f64, f64), x: f64, size: f64, unit: [f64; 2]) -> (f64, f64) {
    let em = f64::from(EM);
    (
        origin.0 + x + unit[0] / em * size,
        origin.1 + unit[1] / em * size,
    )
}

/// The pixel a page point falls in, on a page `height` points tall.
fn pixel(bitmap: &Bitmap, point: (f64, f64), height: f64) -> [u8; 3] {
    let x = (point.0 * SCALE) as usize;
    let y = ((height - point.1) * SCALE) as usize;
    let at = y * bitmap.stride + x * bitmap.components();
    let p = &bitmap.data[at..at + 3];
    [p[0], p[1], p[2]]
}

/// Whether two colours are within `tolerance` of 255 in every component —
/// FT-06.1's two levels for a fill.
fn close(got: [u8; 3], want: [u8; 3], tolerance: u8) -> bool {
    got.iter()
        .zip(want)
        .all(|(&got, want)| got.abs_diff(want) <= tolerance)
}

/// The run [`three_glyph_document`] writes: glyph 1 standing for `A`, glyph 2
/// for `B` and glyph 3 for the two letters of a ligature, at positions that
/// are not their advances, so a run that let the advances place them would
/// put them elsewhere.
const PLACED: [(u16, &str, f64); 3] = [(1, "A", 0.0), (2, "B", 30.0), (3, "fi", 70.0)];

/// The run's size and its origin on a 200 by 100 page.
const SIZE: f64 = 20.0;
const ORIGIN: (f64, f64) = (30.0, 40.0);

/// One page, one colour face with [`RECTS`]' glyphs, and [`PLACED`] as one run.
fn three_glyph_document(builder: &mut DocumentBuilder) -> Vec<u8> {
    let mut builder = std::mem::take(builder);
    assert!(builder.add_colour_font(b"C0", b"FixtureColour", EM));
    for rect in &RECTS {
        rect.register(&mut builder, b"C0");
    }
    let glyphs: Vec<PlacedGlyph<'_>> = PLACED
        .iter()
        .map(|&(id, text, x)| PlacedGlyph {
            glyph: Glyph { id, text },
            x,
            rise: 0.0,
        })
        .collect();
    let mut content = Vec::new();
    assert!(builder.colour_glyph_run(
        &mut content,
        b"C0",
        SIZE,
        [1.0, 0.0, 0.0, 1.0, ORIGIN.0, ORIGIN.1],
        &glyphs
    ));
    builder.add_page(200.0, 100.0, |page| page.raw(&content));
    builder.finish()
}

/// The characters of a page's text that are not white space, with where each
/// starts.
fn inked_chars(document: &Document) -> Vec<(String, (f64, f64))> {
    let page = document.page(0).expect("a page");
    let text = page.text();
    text.lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| (c.text.clone(), c.origin))
        .collect()
}

/// **FT-06.10's exit.** Rectangles drawn by three glyph procedures, written as
/// one run, read back: each glyph's text extracts at the position the run put
/// it, and its pixels are its colour there, with white between the glyphs.
#[test]
fn a_colour_glyph_run_round_trips_its_text_and_positions() {
    let bytes = three_glyph_document(&mut DocumentBuilder::new());
    let document = Document::open(bytes).expect("the written document opens");

    let chars = inked_chars(&document);
    assert_eq!(chars.len(), 3, "{chars:?}");
    for ((text, origin), &(_, want, x)) in chars.iter().zip(&PLACED) {
        assert_eq!(text, want);
        assert!(
            (origin.0 - (ORIGIN.0 + x)).abs() < 1e-3 && (origin.1 - ORIGIN.1).abs() < 1e-3,
            "{want} starts at {origin:?}, placed at {}",
            ORIGIN.0 + x
        );
    }
    let page = document.page(0).expect("a page");
    let plain: String = page
        .text()
        .plain_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(plain, "ABfi");
    let found = page.text().search("fi");
    assert_eq!(found.len(), 1, "the ligature's text is found: {found:?}");

    let bitmap = page.render(&RenderOptions {
        scale: SCALE,
        ..RenderOptions::default()
    });
    for (rect, &(_, _, x)) in RECTS.iter().zip(&PLACED) {
        let middle = [
            (rect.rect[0] + rect.rect[2]) / 2.0,
            (rect.rect[1] + rect.rect[3]) / 2.0,
        ];
        let got = pixel(&bitmap, on_page(ORIGIN, x, SIZE, middle), 100.0);
        assert!(
            close(got, rect.colour, 2),
            "glyph {} paints {got:?} at its middle, not {:?}",
            rect.id,
            rect.colour
        );
    }
    // Between the first glyph's right edge and the second's left one, and
    // above the third: the page.
    for point in [(52.0, 47.0), (110.0, 75.0), (10.0, 10.0)] {
        let got = pixel(&bitmap, point, 100.0);
        assert!(close(got, [255, 255, 255], 2), "{point:?} is {got:?}");
    }
}

/// An entry of a dictionary, resolved.
fn entry(cos: &CosDocument, dict: &Dict, key: &[u8]) -> Object {
    (*cos.resolve_key(dict, cos.intern(key))).clone()
}

/// The numbers of an array entry.
fn numbers(cos: &CosDocument, dict: &Dict, key: &[u8]) -> Vec<f64> {
    entry(cos, dict, key)
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(Object::as_number)
        .collect()
}

/// A name's bytes.
fn name(cos: &CosDocument, object: &Object) -> Vec<u8> {
    object
        .as_name()
        .and_then(|name| cos.name_bytes(name))
        .map(|bytes| bytes.to_vec())
        .unwrap_or_default()
}

/// The Type 3 fonts a written document carries, by reference, sorted by the
/// resource name they answer to.
fn type3_fonts(bytes: &[u8]) -> Vec<(String, ObjRef)> {
    let document = Document::open(bytes.to_vec()).expect("opens");
    let mut fonts: Vec<(String, ObjRef)> = document
        .fonts()
        .into_iter()
        .filter(|font| font.kind == FontKind::Type3)
        .map(|font| {
            (
                font.resource_names.first().cloned().unwrap_or_default(),
                font.reference.expect("written indirectly"),
            )
        })
        .collect();
    fonts.sort();
    fonts
}

/// What one written Type 3 font says, read field by field.
struct WrittenFont {
    matrix: Vec<f64>,
    first: i64,
    last: i64,
    widths: Vec<f64>,
    /// `/Differences` as codes and glyph names.
    names: Vec<(i64, Vec<u8>)>,
    /// Each `/CharProcs` entry's name and decoded procedure.
    procedures: Vec<(Vec<u8>, Vec<u8>)>,
    /// Each form of `/Resources /XObject`: its name and dictionary.
    forms: Vec<(Vec<u8>, Dict)>,
    font_box: Vec<f64>,
    to_unicode: Option<Vec<u8>>,
    descriptor_flags: Option<i64>,
}

fn written_font(cos: &CosDocument, reference: ObjRef) -> WrittenFont {
    let object = cos.get(reference).expect("the font resolves");
    let dict = object.as_dict().expect("a font dictionary").clone();
    assert_eq!(name(cos, &entry(cos, &dict, b"Subtype")), b"Type3");
    let encoding = entry(cos, &dict, b"Encoding");
    let differences = encoding
        .as_dict()
        .map(|encoding| entry(cos, encoding, b"Differences"))
        .unwrap_or(Object::Null);
    let mut names = Vec::new();
    let mut code = 0i64;
    for item in differences.as_array().unwrap_or_default() {
        match item {
            Object::Int(value) => code = *value,
            other => {
                names.push((code, name(cos, other)));
                code += 1;
            }
        }
    }
    let procedures = entry(cos, &dict, b"CharProcs")
        .as_dict()
        .map(|procs| {
            procs
                .entries()
                .iter()
                .map(|(key, value)| {
                    let bytes = cos.name_bytes(*key).map(|b| b.to_vec()).unwrap_or_default();
                    let stream = value.as_objref().expect("an indirect procedure");
                    (bytes, cos.stream_decoded(stream).expect("decodes"))
                })
                .collect()
        })
        .unwrap_or_default();
    let resources = entry(cos, &dict, b"Resources");
    let forms = resources
        .as_dict()
        .map(|resources| entry(cos, resources, b"XObject"))
        .and_then(|xobjects| xobjects.as_dict().cloned())
        .map(|xobjects| {
            xobjects
                .entries()
                .iter()
                .map(|(key, value)| {
                    let bytes = cos.name_bytes(*key).map(|b| b.to_vec()).unwrap_or_default();
                    let form = cos.resolve(value);
                    let stream = form.as_stream().expect("a form is a stream");
                    (bytes, stream.dict.clone())
                })
                .collect()
        })
        .unwrap_or_default();
    let to_unicode = dict
        .get_ref(cos.intern(b"ToUnicode"))
        .map(|stream| cos.stream_decoded(stream).expect("decodes"));
    let descriptor = entry(cos, &dict, b"FontDescriptor");
    WrittenFont {
        matrix: numbers(cos, &dict, b"FontMatrix"),
        first: dict.get_int(cos.intern(b"FirstChar")).unwrap_or(-1),
        last: dict.get_int(cos.intern(b"LastChar")).unwrap_or(-1),
        widths: numbers(cos, &dict, b"Widths"),
        names,
        procedures,
        forms,
        font_box: numbers(cos, &dict, b"FontBBox"),
        to_unicode,
        descriptor_flags: descriptor
            .as_dict()
            .and_then(|d| d.get_int(cos.intern(b"Flags"))),
    }
}

/// **FT-06.10's exit.** The written colour font is clean to the strict
/// validator, and is the object the design settles: a Type 3 font at a
/// thousandth, one procedure per code its `/Differences` names, each
/// beginning with the width `/Widths` states and drawing its glyph's form, an
/// isolated transparency group under the matrix that takes the face's 2048
/// units into the font's thousand, its text in a one-byte `/ToUnicode`.
#[test]
fn the_written_colour_font_is_structurally_clean() {
    let bytes = three_glyph_document(&mut DocumentBuilder::new());
    let document = Document::open(bytes.clone()).expect("opens");
    assert!(document.validate().is_empty(), "{:?}", document.validate());

    let fonts = type3_fonts(&bytes);
    assert_eq!(fonts.len(), 1, "{fonts:?}");
    assert_eq!(fonts[0].0, "C0");
    let cos = CosDocument::open(bytes).expect("opens");
    let font = written_font(&cos, fonts[0].1);
    assert_eq!(font.matrix, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    assert_eq!((font.first, font.last), (0, 2));
    assert_eq!(font.widths, [1000.0, 1000.0, 1000.0], "a full em each");
    assert_eq!(
        font.names,
        vec![
            (0, b"g1".to_vec()),
            (1, b"g2".to_vec()),
            (2, b"g3".to_vec())
        ]
    );
    assert_eq!(font.procedures.len(), 3);
    for (glyph, procedure) in &font.procedures {
        let text = String::from_utf8_lossy(procedure);
        assert!(
            text.starts_with("1000 0 d0\n"),
            "{glyph:?} begins with its width: {text}"
        );
        assert!(
            text.contains(&format!("/{} Do", String::from_utf8_lossy(glyph))),
            "{glyph:?} draws its form: {text}"
        );
    }
    assert_eq!(font.forms.len(), 3);
    for (glyph, form) in &font.forms {
        let group = entry(&cos, form, b"Group");
        let group = group.as_dict().expect("a transparency group");
        assert_eq!(name(&cos, &entry(&cos, group, b"S")), b"Transparency");
        assert_eq!(entry(&cos, group, b"I").as_bool(), Some(true), "{glyph:?}");
        assert!(group.get(cos.intern(b"K")).is_none(), "not knockout");
        let matrix = numbers(&cos, form, b"Matrix");
        assert_eq!(matrix.len(), 6);
        assert!((matrix[0] - 1000.0 / 2048.0).abs() < 1e-6, "{matrix:?}");
        assert!((matrix[3] - 1000.0 / 2048.0).abs() < 1e-6, "{matrix:?}");
    }
    // The union of the three boxes, in the font's glyph space.
    let scale = 1000.0 / 2048.0;
    let expected = [0.0, 0.0, 2048.0 * scale, 2048.0 * scale];
    for (got, want) in font.font_box.iter().zip(expected) {
        assert!((got - want).abs() < 1e-3, "{:?}", font.font_box);
    }
    let cmap = String::from_utf8(font.to_unicode.expect("a /ToUnicode")).expect("text");
    assert!(cmap.contains("<00> <FF>"), "{cmap}");
    for line in ["<00> <0041>", "<01> <0042>", "<02> <00660069>"] {
        assert!(cmap.contains(line), "{line} in {cmap}");
    }
    assert_eq!(font.descriptor_flags, Some(4), "symbolic");
}

/// Under a profile that admits no transparency (ISO 19005-1 6.4), the
/// glyph's form carries no `/Group`, which draws the same picture wherever
/// the text's alpha is one; nothing is refused.
#[test]
fn under_a_profile_without_transparency_a_glyphs_form_is_plain() {
    let mut builder = DocumentBuilder::archival(tinker_pdf_cos::ArchivalProfile {
        part: tinker_pdf_cos::ArchivalPart::One,
        level: Some(tinker_pdf_cos::ArchivalLevel::B),
        destination_profile: pdfa_support::srgb_like(),
        destination_space: tinker_pdf_cos::DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    });
    let bytes = three_glyph_document(&mut builder);
    let fonts = type3_fonts(&bytes);
    let cos = CosDocument::open(bytes).expect("opens");
    let font = written_font(&cos, fonts[0].1);
    assert_eq!(font.forms.len(), 3);
    for (glyph, form) in &font.forms {
        assert!(
            form.get(cos.intern(b"Group")).is_none(),
            "{glyph:?} has no group"
        );
    }
}

/// The first glyph of [`many_glyph_document`]'s run: glyph `n` stands for
/// one ideograph each, so the extracted text says which glyph drew it.
fn ideograph(n: u16) -> String {
    char::from_u32(0x4E00 + u32::from(n))
        .expect("an ideograph")
        .to_string()
}

/// Three hundred glyphs, each a small square, drawn as one run: more than the
/// 255 codes one Type 3 font gives, so the run spills into a second font.
fn many_glyph_document() -> (Vec<u8>, Vec<u8>) {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_colour_font(b"C0", b"FixtureColour", EM));
    for id in 1..=300u16 {
        let shade = u8::try_from(id % 200).expect("under 200");
        let content = rectangle([shade, 0, 255 - shade], [256.0, 256.0, 768.0, 768.0]);
        assert!(builder.add_colour_glyph(
            b"C0",
            &ColourGlyph {
                id,
                advance: 1024.0,
                bbox: [256.0, 256.0, 768.0, 768.0],
                content: &content,
            },
        ));
    }
    let texts: Vec<String> = (1..=300u16).map(ideograph).collect();
    let glyphs: Vec<PlacedGlyph<'_>> = (1..=300u16)
        .zip(&texts)
        .map(|(id, text)| PlacedGlyph {
            glyph: Glyph { id, text },
            // Half an em at four points apart: each glyph at its advance.
            x: f64::from(id - 1) * 2.0,
            rise: 0.0,
        })
        .collect();
    let mut content = Vec::new();
    assert!(builder.colour_glyph_run(
        &mut content,
        b"C0",
        4.0,
        [1.0, 0.0, 0.0, 1.0, 10.0, 50.0],
        &glyphs
    ));
    builder.add_page(620.0, 100.0, |page| page.raw(&content));
    (builder.finish(), content)
}

/// **FT-06.10's exit.** A run of three hundred glyphs spills from the face's
/// first Type 3 font into a second as its 256th glyph needs a code, selects
/// it with `Tf` there, and still extracts every glyph's text, in order, at
/// its position.
#[test]
fn a_run_longer_than_one_fonts_codes_spills_and_round_trips() {
    let (bytes, content) = many_glyph_document();
    let content = String::from_utf8(content).expect("the run is text");
    assert_eq!(content.matches("/C0 4 Tf").count(), 1, "{content}");
    assert_eq!(content.matches("/C0.1 4 Tf").count(), 1, "{content}");
    // Code 32 is given to no glyph.
    assert!(!content.contains("<20>"), "{content}");

    let fonts = type3_fonts(&bytes);
    assert_eq!(
        fonts
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["C0", "C0.1"]
    );
    let cos = CosDocument::open(bytes.clone()).expect("opens");
    let first = written_font(&cos, fonts[0].1);
    let second = written_font(&cos, fonts[1].1);
    assert_eq!(first.procedures.len(), 255);
    assert_eq!(second.procedures.len(), 45);
    // 0 to 31, then 33 to 255: a width of 0 stands at 32.
    assert_eq!((first.first, first.last), (0, 255));
    assert_eq!(first.widths.len(), 256);
    assert_eq!(first.widths[32], 0.0);
    assert!(first.names.iter().all(|(code, _)| *code != 32));
    // Forty-five codes more: 0 to 31 and 33 to 45.
    assert_eq!((second.first, second.last), (0, 45));
    assert_eq!(second.widths.len(), 46);

    let document = Document::open(bytes).expect("opens");
    assert!(document.validate().is_empty(), "{:?}", document.validate());
    let chars = inked_chars(&document);
    assert_eq!(chars.len(), 300);
    for (n, (text, origin)) in (1..=300u16).zip(&chars) {
        assert_eq!(*text, ideograph(n), "glyph {n}'s text");
        let x = 10.0 + f64::from(n - 1) * 2.0;
        assert!(
            (origin.0 - x).abs() < 1e-3 && (origin.1 - 50.0).abs() < 1e-3,
            "glyph {n} starts at {origin:?}, placed at {x}"
        );
    }
}

/// A glyph that draws nothing — a space — is its width alone: no form, and
/// no mark in the font's box; the glyph after it still lands where placed.
#[test]
fn a_glyph_that_draws_nothing_is_its_width_alone() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_colour_font(b"C0", b"FixtureColour", EM));
    RECTS[0].register(&mut builder, b"C0");
    assert!(builder.add_colour_glyph(
        b"C0",
        &ColourGlyph {
            id: 9,
            advance: 512.0,
            bbox: [0.0; 4],
            content: b"",
        },
    ));
    let glyphs = [
        PlacedGlyph {
            glyph: Glyph { id: 9, text: " " },
            x: 0.0,
            rise: 0.0,
        },
        PlacedGlyph {
            glyph: Glyph { id: 1, text: "A" },
            x: 5.0,
            rise: 0.0,
        },
    ];
    let mut content = Vec::new();
    assert!(builder.colour_glyph_run(
        &mut content,
        b"C0",
        SIZE,
        [1.0, 0.0, 0.0, 1.0, ORIGIN.0, ORIGIN.1],
        &glyphs
    ));
    builder.add_page(200.0, 100.0, |page| page.raw(&content));
    let bytes = builder.finish();

    let fonts = type3_fonts(&bytes);
    let cos = CosDocument::open(bytes.clone()).expect("opens");
    let font = written_font(&cos, fonts[0].1);
    assert_eq!(font.forms.len(), 1, "only the marking glyph has a form");
    let space = font
        .procedures
        .iter()
        .find(|(name, _)| name == b"g9")
        .expect("the space's procedure");
    assert_eq!(String::from_utf8_lossy(&space.1), "250 0 d0\n");
    // The box is the rectangle's alone, in thousandths.
    let scale = 1000.0 / 2048.0;
    for (got, want) in font.font_box.iter().zip(RECTS[0].rect.map(|v| v * scale)) {
        assert!((got - want).abs() < 1e-3, "{:?}", font.font_box);
    }
    let document = Document::open(bytes).expect("opens");
    let chars = inked_chars(&document);
    assert_eq!(chars.len(), 1, "{chars:?}");
    assert!((chars[0].1 .0 - (ORIGIN.0 + 5.0)).abs() < 1e-3, "{chars:?}");
}

/// `/ToUnicode` is one function of the code: a glyph drawn first with no
/// text and then with two different texts stands for the first text it was
/// given, wherever it is drawn.
#[test]
fn the_first_text_a_glyph_is_drawn_with_stands() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_colour_font(b"C0", b"FixtureColour", EM));
    RECTS[2].register(&mut builder, b"C0");
    let mut content = Vec::new();
    for (line, text) in ["", "Q", "R"].into_iter().enumerate() {
        let glyphs = [PlacedGlyph {
            glyph: Glyph { id: 3, text },
            x: 0.0,
            rise: 0.0,
        }];
        let y = 80.0 - 25.0 * line as f64;
        assert!(builder.colour_glyph_run(
            &mut content,
            b"C0",
            SIZE,
            [1.0, 0.0, 0.0, 1.0, 20.0, y],
            &glyphs
        ));
    }
    builder.add_page(200.0, 100.0, |page| page.raw(&content));
    let document = Document::open(builder.finish()).expect("opens");
    let chars = inked_chars(&document);
    assert_eq!(chars.len(), 3, "{chars:?}");
    assert!(chars.iter().all(|(text, _)| text == "Q"), "{chars:?}");
}

/// A glyph off the baseline is drawn there through `Ts`, which the run resets
/// before it ends, and extracts where it was placed.
#[test]
fn a_glyph_off_the_baseline_is_drawn_and_read_there() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_colour_font(b"C0", b"FixtureColour", EM));
    RECTS[2].register(&mut builder, b"C0");
    let glyphs = [
        PlacedGlyph {
            glyph: Glyph { id: 3, text: "x" },
            x: 0.0,
            rise: 0.0,
        },
        PlacedGlyph {
            glyph: Glyph { id: 3, text: "x" },
            x: 30.0,
            rise: 15.0,
        },
    ];
    let mut content = Vec::new();
    assert!(builder.colour_glyph_run(
        &mut content,
        b"C0",
        SIZE,
        [1.0, 0.0, 0.0, 1.0, ORIGIN.0, ORIGIN.1],
        &glyphs
    ));
    let text = String::from_utf8(content.clone()).expect("text");
    assert!(text.contains("15 Ts"), "{text}");
    assert!(text.trim_end().ends_with("0 Ts\nET"), "{text}");
    builder.add_page(200.0, 100.0, |page| page.raw(&content));
    let document = Document::open(builder.finish()).expect("opens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions {
        scale: SCALE,
        ..RenderOptions::default()
    });
    // The raised square's middle, and where it would have been unraised.
    let raised = on_page(ORIGIN, 30.0, SIZE, [1024.0, 1024.0]);
    let raised = (raised.0, raised.1 + 15.0);
    assert!(close(pixel(&bitmap, raised, 100.0), [0, 0, 255], 2));
    let unraised = on_page(ORIGIN, 30.0, SIZE, [1024.0, 256.0]);
    assert!(close(pixel(&bitmap, unraised, 100.0), [255, 255, 255], 2));
}

/// What a run cannot write it refuses, having written nothing; what a face
/// cannot register it refuses, registering nothing.
#[test]
fn a_colour_run_refuses_what_it_cannot_write() {
    let mut builder = DocumentBuilder::new();
    assert!(
        !builder.add_colour_font(b"C0", b"FixtureColour", 0),
        "an em of nothing"
    );
    assert!(
        !builder.add_colour_font(b"", b"FixtureColour", EM),
        "no resource"
    );
    assert!(!builder.add_colour_font(b"C0", b"", EM), "no name");
    assert!(builder.add_colour_font(b"C0", b"FixtureColour", EM));
    assert!(
        !builder.add_colour_font(b"C0", b"Other", EM),
        "a name already a font"
    );
    assert!(builder.add_base_font(b"F1", b"Helvetica"));
    assert!(
        !builder.add_colour_font(b"F1", b"FixtureColour", EM),
        "a name already a font"
    );

    RECTS[0].register(&mut builder, b"C0");
    assert!(builder.has_colour_glyph(b"C0", 1));
    assert!(!builder.has_colour_glyph(b"C0", 2));
    assert!(!builder.has_colour_glyph(b"F1", 1));
    let glyph = |id, content: &'static [u8], bbox, advance| ColourGlyph {
        id,
        advance,
        bbox,
        content,
    };
    let fill: &[u8] = b"0 0 1 1 re f\n";
    let unit = [0.0, 0.0, 1.0, 1.0];
    assert!(
        !builder.add_colour_glyph(b"C0", &glyph(1, fill, unit, 1.0)),
        "twice"
    );
    assert!(
        !builder.add_colour_glyph(b"F1", &glyph(2, fill, unit, 1.0)),
        "no face"
    );
    assert!(
        !builder.add_colour_glyph(b"C0", &glyph(2, fill, [0.0; 4], 1.0)),
        "no area"
    );
    assert!(!builder.add_colour_glyph(b"C0", &glyph(2, fill, [0.0, 0.0, f64::NAN, 1.0], 1.0)));
    assert!(
        !builder.add_colour_glyph(b"C0", &glyph(2, fill, unit, f64::INFINITY)),
        "no advance"
    );
    assert!(
        !builder.has_colour_glyph(b"C0", 2),
        "nothing was registered"
    );

    let placed = |id| PlacedGlyph {
        glyph: Glyph { id, text: "A" },
        x: 0.0,
        rise: 0.0,
    };
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut out = Vec::new();
    assert!(
        !builder.colour_glyph_run(&mut out, b"C0", 10.0, identity, &[placed(1), placed(2)]),
        "glyph 2 has no procedure"
    );
    assert!(
        !builder.colour_glyph_run(&mut out, b"F1", 10.0, identity, &[placed(1)]),
        "no colour face"
    );
    assert!(
        !builder.colour_glyph_run(&mut out, b"C9", 10.0, identity, &[placed(1)]),
        "no font at all"
    );
    assert!(
        !builder.colour_glyph_run(&mut out, b"C0", 10.0, identity, &[]),
        "no glyphs"
    );
    for size in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            !builder.colour_glyph_run(&mut out, b"C0", size, identity, &[placed(1)]),
            "size {size}"
        );
    }
    assert!(!builder.colour_glyph_run(
        &mut out,
        b"C0",
        10.0,
        [1.0, 0.0, 0.0, f64::NAN, 0.0, 0.0],
        &[placed(1)]
    ));
    let mut off = placed(1);
    off.x = f64::NAN;
    assert!(!builder.colour_glyph_run(&mut out, b"C0", 10.0, identity, &[off]));
    off.x = 0.0;
    off.rise = f64::INFINITY;
    assert!(!builder.colour_glyph_run(&mut out, b"C0", 10.0, identity, &[off]));
    assert!(out.is_empty(), "a refused run writes nothing: {out:?}");

    // A refused run gave no code: the next run's first glyph takes code 0.
    assert!(builder.colour_glyph_run(&mut out, b"C0", 10.0, identity, &[placed(1)]));
    let text = String::from_utf8(out).expect("text");
    assert!(text.contains("[<00>] TJ"), "{text}");
}

// ---- COLR version 0, through `shaping::write_run` --------------------------

/// A version 0 `COLR` (*COLR header*, *BaseGlyph and Layer records*): each
/// base glyph with its layers, bottom first, as glyph ids and palette
/// indices, the records in increasing glyph order as the chapter requires.
fn colr_v0(bases: &[(u16, Vec<(u16, u16)>)]) -> Vec<u8> {
    let layer_count: usize = bases.iter().map(|(_, layers)| layers.len()).sum();
    let base_at = 14usize;
    let layers_at = base_at + 6 * bases.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(bases.len() as u16).to_be_bytes());
    out.extend_from_slice(&(base_at as u32).to_be_bytes());
    out.extend_from_slice(&(layers_at as u32).to_be_bytes());
    out.extend_from_slice(&(layer_count as u16).to_be_bytes());
    let mut first = 0u16;
    for (glyph, layers) in bases {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&first.to_be_bytes());
        out.extend_from_slice(&(layers.len() as u16).to_be_bytes());
        first += layers.len() as u16;
    }
    for (_, layers) in bases {
        for (glyph, index) in layers {
            out.extend_from_slice(&glyph.to_be_bytes());
            out.extend_from_slice(&index.to_be_bytes());
        }
    }
    out
}

/// A version 0 `CPAL` (*Palette table header*, *Palette entries and color
/// records*): palettes of the same number of entries, each given as RGBA and
/// written in the table's blue-green-red-alpha order, each palette's records
/// after the one before.
fn cpal(palettes: &[Vec<[u8; 4]>]) -> Vec<u8> {
    let entries = palettes.first().map_or(0, Vec::len);
    let records_at = 12 + 2 * palettes.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(entries as u16).to_be_bytes());
    out.extend_from_slice(&(palettes.len() as u16).to_be_bytes());
    out.extend_from_slice(&((entries * palettes.len()) as u16).to_be_bytes());
    out.extend_from_slice(&(records_at as u32).to_be_bytes());
    for palette in 0..palettes.len() {
        out.extend_from_slice(&((palette * entries) as u16).to_be_bytes());
    }
    for palette in palettes {
        for [r, g, b, a] in palette {
            out.extend_from_slice(&[*b, *g, *r, *a]);
        }
    }
    out
}

/// The palette index that is no index: the text's own colour.
const FOREGROUND: u16 = 0xFFFF;

/// Half an alpha: 128 of 255.
const HALF: u8 = 128;

/// A face covering `A` and `B` at a thousand units, each a full em wide:
/// `A` the colour glyph of three layers — a red em, a half-transparent blue
/// square over its middle, and a bar in the text's colour down the centre —
/// and `B` a plain box, which no table describes. Palette 1 gives the same
/// layers yellow and cyan.
fn layered_face() -> Face {
    let base = Face::new("Fixture Colour", "AB").with_advance(1000);
    let em = base.colour_glyph(0);
    let square = base.colour_glyph(1);
    let bar = base.colour_glyph(2);
    let a = base.glyph_of('A').expect("A is covered");
    base.with_colour(Colour {
        rectangles: vec![
            [0, 0, 1000, 1000],
            [250, 250, 750, 750],
            [400, 100, 600, 900],
        ],
        colr: colr_v0(&[(a, vec![(em, 0), (square, 1), (bar, FOREGROUND)])]),
        cpal: cpal(&[
            vec![[255, 0, 0, 255], [0, 0, 255, HALF]],
            vec![[255, 255, 0, 255], [0, 255, 255, HALF]],
        ]),
    })
}

/// The run [`colour_document`] writes: 50 points to the em, its origin at
/// (20, 20) on a 200 by 100 page, in a text colour of `0 0.5 0`.
const RUN_SIZE: f64 = 50.0;
const RUN_ORIGIN: (f64, f64) = (20.0, 20.0);
const TEXT_GREEN: [u8; 3] = [0, 128, 0];

/// `text` in `face`, written through `shaping::write_run` as a colour run in
/// `palette`, under `builder`, on a page 900 by 100; what the face warned,
/// and the document.
fn colour_document(
    face: &Face,
    text: &str,
    palette: PaletteChoice,
    mut builder: DocumentBuilder,
) -> (Vec<u8>, Vec<ColourWarning>) {
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("the fixture face is a valid sfnt");
    let colour = ColourFace::new(&sfnt, palette);
    assert!(colour.has_colour());
    assert!(colour.register(&mut builder, b"C0", b"FixtureColour"));
    let mut content = b"0 0.5 0 rg\n".to_vec();
    assert!(write_run(
        &mut builder,
        &mut content,
        &Run {
            font: b"C0",
            shaper: Shaper::new(&sfnt),
            size: RUN_SIZE,
            matrix: [1.0, 0.0, 0.0, 1.0, RUN_ORIGIN.0, RUN_ORIGIN.1],
            text,
            direction: BaseDirection::LeftToRight,
            colour: Some(&colour),
        },
    ));
    builder.add_page(900.0, 100.0, |page| page.raw(&content));
    (builder.finish(), colour.warnings())
}

/// A point of the glyph at `pen` ems along the run, in font units.
fn at_units(pen: f64, unit: [f64; 2]) -> (f64, f64) {
    (
        RUN_ORIGIN.0 + (pen + unit[0] / 1000.0) * RUN_SIZE,
        RUN_ORIGIN.1 + unit[1] / 1000.0 * RUN_SIZE,
    )
}

/// `source` at `alpha` over `backdrop` — W3C Compositing Level 1 5.1's simple
/// alpha compositing, which COLR's *Compositing and blending* names — with
/// both opaque underneath, rounded to eight bits.
fn over(source: [u8; 3], alpha: u8, backdrop: [u8; 3]) -> [u8; 3] {
    let a = f64::from(alpha) / 255.0;
    let mut out = [0u8; 3];
    for i in 0..3 {
        let value = f64::from(source[i]) * a + f64::from(backdrop[i]) * (1.0 - a);
        out[i] = value.round() as u8;
    }
    out
}

/// The rendered page and its text.
fn rendered(bytes: Vec<u8>) -> (Bitmap, String) {
    let document = Document::open(bytes).expect("the written document opens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions {
        scale: SCALE,
        ..RenderOptions::default()
    });
    let text = page
        .text()
        .plain_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    (bitmap, text)
}

/// **FT-06.11's exit.** A synthetic face's colour glyph, written through
/// `shaping::write_run` and rendered: in each layer the colour the palette
/// gives, composited over the layers below — the red em, the blue square at
/// half over it, the bar in the text's own green over both — in either
/// palette; the plain glyph beside it in the text's colour; and the text
/// `AB` extracting.
#[test]
fn colr_v0_layers_draw_their_palette_colours() {
    let face = layered_face();
    for (palette, em_colour, square_colour) in [
        (PaletteChoice::Default, [255, 0, 0], [0, 0, 255]),
        (PaletteChoice::Index(1), [255, 255, 0], [0, 255, 255]),
    ] {
        let (bytes, warnings) = colour_document(&face, "AB", palette, DocumentBuilder::new());
        assert!(warnings.is_empty(), "{warnings:?}");
        let (bitmap, text) = rendered(bytes);
        assert_eq!(text, "AB");
        let samples = [
            // The em alone, in a corner.
            (at_units(0.0, [100.0, 100.0]), em_colour),
            (at_units(0.0, [900.0, 900.0]), em_colour),
            // The square over the em, beside the bar.
            (
                at_units(0.0, [300.0, 500.0]),
                over(square_colour, HALF, em_colour),
            ),
            (
                at_units(0.0, [700.0, 300.0]),
                over(square_colour, HALF, em_colour),
            ),
            // The bar over the em, below the square, and over both.
            (at_units(0.0, [500.0, 175.0]), TEXT_GREEN),
            (at_units(0.0, [500.0, 500.0]), TEXT_GREEN),
            // `B`, the plain box from 0 to 700, in the text's colour.
            (at_units(1.0, [350.0, 350.0]), TEXT_GREEN),
            // Above the box and past the run: the page.
            (at_units(1.0, [350.0, 850.0]), [255, 255, 255]),
            (at_units(2.5, [500.0, 500.0]), [255, 255, 255]),
        ];
        for (point, want) in samples {
            let got = pixel(&bitmap, point, 100.0);
            assert!(
                close(got, want, 2),
                "{palette:?}: {point:?} is {got:?}, not {want:?}"
            );
        }
    }
}

/// The foreground layer is the text's colour wherever the text is shown in a
/// colour: the same glyph shown in red has a red bar and the same red em and
/// square.
#[test]
fn a_foreground_layer_takes_whatever_colour_the_text_is_in() {
    let face = layered_face();
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let colour = ColourFace::new(&sfnt, PaletteChoice::Default);
    let mut builder = DocumentBuilder::new();
    assert!(colour.register(&mut builder, b"C0", b"FixtureColour"));
    let mut content = Vec::new();
    for (line, fill) in [(0.0, "0 0 1 rg"), (1.0, "1 0.5 0 rg")] {
        content.extend_from_slice(fill.as_bytes());
        content.push(b'\n');
        assert!(write_run(
            &mut builder,
            &mut content,
            &Run {
                font: b"C0",
                shaper: Shaper::new(&sfnt),
                size: 20.0,
                matrix: [1.0, 0.0, 0.0, 1.0, 20.0 + 40.0 * line, 20.0],
                text: "A",
                direction: BaseDirection::LeftToRight,
                colour: Some(&colour),
            },
        ));
    }
    builder.add_page(200.0, 100.0, |page| page.raw(&content));
    let (bitmap, text) = rendered(builder.finish());
    assert_eq!(text, "AA");
    // The bar's middle in each, and the em's corner, which no text colour
    // reaches.
    for (x, bar) in [(20.0, [0, 0, 255]), (60.0, [255, 128, 0])] {
        let middle = (x + 10.0, 20.0 + 3.5);
        assert!(close(pixel(&bitmap, middle, 100.0), bar, 2), "{x}");
        let corner = (x + 2.0, 20.0 + 2.0);
        assert!(close(pixel(&bitmap, corner, 100.0), [255, 0, 0], 2), "{x}");
    }
}

/// One glyph drawn twice is one procedure and one form, and each layer alpha
/// one `/ExtGState`, however many runs draw it.
#[test]
fn a_colour_glyph_drawn_twice_is_one_form() {
    let face = layered_face();
    let (bytes, _) = colour_document(
        &face,
        "AABA",
        PaletteChoice::Default,
        DocumentBuilder::new(),
    );
    let fonts = type3_fonts(&bytes);
    assert_eq!(fonts.len(), 1);
    let cos = CosDocument::open(bytes).expect("opens");
    let font = written_font(&cos, fonts[0].1);
    assert_eq!(font.procedures.len(), 2, "A and B");
    assert_eq!(font.forms.len(), 2);
    // `A`'s form names its alpha's state once; `B`'s names none.
    let a = font
        .forms
        .iter()
        .find(|(name, _)| name == b"g1")
        .expect("A's form");
    let states = entry(&cos, &a.1, b"Resources");
    let states = states
        .as_dict()
        .map(|resources| entry(&cos, resources, b"ExtGState"))
        .and_then(|states| states.as_dict().cloned())
        .expect("A's form names its states");
    assert!(states
        .entries()
        .iter()
        .any(|(key, _)| cos.name_bytes(*key).as_deref() == Some(&b"C0.ca128"[..])));
}

/// Under a profile that admits no transparency, a glyph whose layers need an
/// alpha below one is drawn as its outline, in the text's colour, and named;
/// the builder refuses nothing, so the document is still the profile's.
#[test]
fn a_translucent_layer_under_a_profile_without_transparency_is_drawn_as_the_outline() {
    let builder = DocumentBuilder::archival(tinker_pdf_cos::ArchivalProfile {
        part: tinker_pdf_cos::ArchivalPart::One,
        level: Some(tinker_pdf_cos::ArchivalLevel::B),
        destination_profile: pdfa_support::srgb_like(),
        destination_space: tinker_pdf_cos::DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    });
    let face = layered_face();
    let (bytes, warnings) = colour_document(&face, "A", PaletteChoice::Default, builder);
    let glyph = face.glyph_of('A').expect("A is covered");
    assert_eq!(warnings, [ColourWarning::TransparencyForbidden { glyph }]);
    let (bitmap, text) = rendered(bytes);
    assert_eq!(text, "A");
    // `A`'s own outline, the box from 0 to 700, in green; the em's corner past
    // it, white.
    assert!(close(
        pixel(&bitmap, at_units(0.0, [350.0, 350.0]), 100.0),
        TEXT_GREEN,
        2
    ));
    assert!(close(
        pixel(&bitmap, at_units(0.0, [900.0, 900.0]), 100.0),
        [255, 255, 255],
        2
    ));
}

/// A face with no colour tables draws every glyph as its outline in the
/// text's colour, and a `COLR` with no `CPAL` is no colour table, as the
/// chapter's *Overview* says.
#[test]
fn a_face_without_a_palette_has_no_colour_glyph() {
    let plain = Face::new("Fixture Plain", "AB").with_advance(1000);
    let program = plain.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    assert!(!ColourFace::new(&sfnt, PaletteChoice::Default).has_colour());

    let mut no_palette = layered_face();
    if let Some(colour) = no_palette.colour.as_mut() {
        colour.cpal = Vec::new();
    }
    let program = no_palette.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    assert!(!ColourFace::new(&sfnt, PaletteChoice::Default).has_colour());
}

// ---- COLR version 1, through `shaping::write_run` --------------------------

/// A `COLR` version 1 paint to write (*Paint tables*): a tree, each node one
/// table, a subtree written twice two tables — but for
/// [`V1::LayersOfOne`], whose layer slots all name one table.
#[derive(Clone, Debug)]
enum V1 {
    /// Format 1: a slice of the `LayerList`, bottom first.
    Layers(Vec<V1>),
    /// Format 1, its `n` slots all naming one table, so the reader reads one
    /// node with `n` parents.
    LayersOfOne(usize, Box<V1>),
    /// Format 2: a palette entry and an alpha.
    Solid(u16, f64),
    /// Format 3, at the default instance.
    VarSolid(u16, f64),
    /// Formats 4 and 5, the second at the default instance: `p0`, `p1` and
    /// `p2`, and the colour line.
    Linear {
        line: Line,
        points: [[i16; 2]; 3],
        variable: bool,
    },
    /// Formats 6 and 7: two circles, centre and radius each.
    Radial {
        line: Line,
        circles: [([i16; 2], u16); 2],
        variable: bool,
    },
    /// Formats 8 and 9: the centre, and the start and end angles in degrees,
    /// counter-clockwise, written biased as the formats store them.
    Sweep {
        line: Line,
        centre: [i16; 2],
        angles: [f64; 2],
        variable: bool,
    },
    /// Format 10: a glyph's outline as the child's clip.
    Glyph(u16, Box<V1>),
    /// Format 11: another base glyph's graph.
    ColrGlyph(u16),
    /// Format 12: an `Affine2x3`, `xx yx xy yy dx dy`.
    Transform([f64; 6], Box<V1>),
    /// Formats 14: a translation in font units.
    Translate(i16, i16, Box<V1>),
    /// Formats 16 and 18: scales along x and y, about a centre for 18.
    Scale(f64, f64, Option<[i16; 2]>, Box<V1>),
    /// Formats 20 and 22.
    ScaleUniform(f64, Option<[i16; 2]>, Box<V1>),
    /// Formats 24 and 26: an angle in units of 180°, counter-clockwise.
    Rotate(f64, Option<[i16; 2]>, Box<V1>),
    /// Formats 28 and 30: the x and y skew angles in units of 180°.
    Skew(f64, f64, Option<[i16; 2]>, Box<V1>),
    /// Format 32: source, `compositeMode`, backdrop.
    Composite(Box<V1>, u8, Box<V1>),
}

/// A colour line to write (*Color references, ColorStop and ColorLine*): its
/// `extend` byte, and its stops in the order the table gives them, each an
/// offset, a palette entry and an alpha.
#[derive(Clone, Debug)]
struct Line {
    extend: u8,
    stops: Vec<(f64, u16, f64)>,
}

/// `EXTEND_PAD`, `EXTEND_REPEAT` and `EXTEND_REFLECT`.
const PAD: u8 = 0;
const REPEAT: u8 = 1;
const REFLECT: u8 = 2;

/// A line of opaque stops.
fn line(extend: u8, stops: &[(f64, u16)]) -> Line {
    Line {
        extend,
        stops: stops.iter().map(|&(at, entry)| (at, entry, 1.0)).collect(),
    }
}

/// An `F2DOT14`.
fn f2dot14(value: f64) -> [u8; 2] {
    ((value * 16384.0).round() as i16).to_be_bytes()
}

/// A `Fixed`.
fn fixed(value: f64) -> [u8; 4] {
    ((value * 65536.0).round() as i32).to_be_bytes()
}

/// The paint tables of a `COLR`, written into one area, and the `LayerList`
/// slots its layer lists filled, each the area offset of a table.
#[derive(Default)]
struct V1Writer {
    area: Vec<u8>,
    layers: Vec<usize>,
}

impl V1Writer {
    /// Puts `value` as an `Offset24` at `at`.
    fn patch(&mut self, at: usize, value: usize) {
        let bytes = u32::try_from(value).expect("an offset").to_be_bytes();
        self.area[at..at + 3].copy_from_slice(&bytes[1..]);
    }

    /// A child offset to fill at `at`, from the table at `table`, with the
    /// child written now, after it.
    fn child(&mut self, table: usize, at: usize, child: &V1) {
        let written = self.write(child);
        self.patch(at, written - table);
    }

    fn centre(&mut self, centre: Option<[i16; 2]>) {
        if let Some([x, y]) = centre {
            self.area.extend_from_slice(&x.to_be_bytes());
            self.area.extend_from_slice(&y.to_be_bytes());
        }
    }

    /// A variable format's `varIndexBase`: no delta set, so the table draws
    /// at its default.
    fn var_index(&mut self, variable: bool) {
        if variable {
            self.area.extend_from_slice(&u32::MAX.to_be_bytes());
        }
    }

    /// The colour line of the gradient table at `table`, written now, after
    /// it, its offset patched into the table's `Offset24` at one: a
    /// `ColorLine`, or a `VarColorLine` whose stops each carry a
    /// `varIndexBase`.
    fn line(&mut self, table: usize, line: &Line, variable: bool) {
        let here = self.area.len();
        self.patch(table + 1, here - table);
        self.area.push(line.extend);
        self.area.extend_from_slice(
            &u16::try_from(line.stops.len())
                .expect("few stops")
                .to_be_bytes(),
        );
        for &(offset, entry, alpha) in &line.stops {
            self.area.extend_from_slice(&f2dot14(offset));
            self.area.extend_from_slice(&entry.to_be_bytes());
            self.area.extend_from_slice(&f2dot14(alpha));
            self.var_index(variable);
        }
    }

    /// Writes `paint` at the area's end, its children after it, each offset
    /// from the table that names it as *Paint tables* has it; returns where it
    /// starts.
    fn write(&mut self, paint: &V1) -> usize {
        let at = self.area.len();
        match paint {
            V1::Layers(children) => {
                let first = self.layers.len();
                self.layers.extend(std::iter::repeat_n(0, children.len()));
                self.area.extend_from_slice(&[1, children.len() as u8]);
                self.area.extend_from_slice(&(first as u32).to_be_bytes());
                for (i, child) in children.iter().enumerate() {
                    let written = self.write(child);
                    self.layers[first + i] = written;
                }
            }
            V1::LayersOfOne(count, child) => {
                let first = self.layers.len();
                self.layers.extend(std::iter::repeat_n(0, *count));
                self.area.extend_from_slice(&[1, *count as u8]);
                self.area.extend_from_slice(&(first as u32).to_be_bytes());
                let written = self.write(child);
                for slot in &mut self.layers[first..first + count] {
                    *slot = written;
                }
            }
            V1::Solid(entry, alpha) | V1::VarSolid(entry, alpha) => {
                let variable = matches!(paint, V1::VarSolid(..));
                self.area.push(if variable { 3 } else { 2 });
                self.area.extend_from_slice(&entry.to_be_bytes());
                self.area.extend_from_slice(&f2dot14(*alpha));
                if variable {
                    self.area.extend_from_slice(&0u32.to_be_bytes());
                }
            }
            V1::Linear {
                line,
                points,
                variable,
            } => {
                // *Formats 4 and 5*: the format, an `Offset24` to the colour
                // line, `x0 y0 x1 y1 x2 y2`, and for 5 a `varIndexBase`.
                self.area
                    .extend_from_slice(&[if *variable { 5 } else { 4 }, 0, 0, 0]);
                for [x, y] in points {
                    self.area.extend_from_slice(&x.to_be_bytes());
                    self.area.extend_from_slice(&y.to_be_bytes());
                }
                self.var_index(*variable);
                self.line(at, line, *variable);
            }
            V1::Radial {
                line,
                circles,
                variable,
            } => {
                // *Formats 6 and 7*: `x0 y0 radius0 x1 y1 radius1`.
                self.area
                    .extend_from_slice(&[if *variable { 7 } else { 6 }, 0, 0, 0]);
                for ([x, y], radius) in circles {
                    self.area.extend_from_slice(&x.to_be_bytes());
                    self.area.extend_from_slice(&y.to_be_bytes());
                    self.area.extend_from_slice(&radius.to_be_bytes());
                }
                self.var_index(*variable);
                self.line(at, line, *variable);
            }
            V1::Sweep {
                line,
                centre,
                angles,
                variable,
            } => {
                // *Formats 8 and 9*: the centre, and `startAngle` and
                // `endAngle` as `F2DOT14`s biased by one, 180° a unit.
                self.area
                    .extend_from_slice(&[if *variable { 9 } else { 8 }, 0, 0, 0]);
                for value in centre {
                    self.area.extend_from_slice(&value.to_be_bytes());
                }
                for degrees in angles {
                    self.area.extend_from_slice(&f2dot14(degrees / 180.0 - 1.0));
                }
                self.var_index(*variable);
                self.line(at, line, *variable);
            }
            V1::Glyph(glyph, child) => {
                self.area.extend_from_slice(&[10, 0, 0, 0]);
                self.area.extend_from_slice(&glyph.to_be_bytes());
                self.child(at, at + 1, child);
            }
            V1::ColrGlyph(glyph) => {
                self.area.push(11);
                self.area.extend_from_slice(&glyph.to_be_bytes());
            }
            V1::Transform(matrix, child) => {
                self.area.extend_from_slice(&[12, 0, 0, 0, 0, 0, 0]);
                let affine = self.area.len();
                for value in matrix {
                    self.area.extend_from_slice(&fixed(*value));
                }
                self.patch(at + 4, affine - at);
                self.child(at, at + 1, child);
            }
            V1::Translate(dx, dy, child) => {
                self.area.extend_from_slice(&[14, 0, 0, 0]);
                self.area.extend_from_slice(&dx.to_be_bytes());
                self.area.extend_from_slice(&dy.to_be_bytes());
                self.child(at, at + 1, child);
            }
            V1::Scale(x, y, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 18 } else { 16 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*x));
                self.area.extend_from_slice(&f2dot14(*y));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::ScaleUniform(scale, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 22 } else { 20 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*scale));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::Rotate(angle, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 26 } else { 24 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*angle));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::Skew(x, y, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 30 } else { 28 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*x));
                self.area.extend_from_slice(&f2dot14(*y));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::Composite(source, mode, backdrop) => {
                self.area.extend_from_slice(&[32, 0, 0, 0, *mode, 0, 0, 0]);
                self.child(at, at + 1, source);
                self.child(at, at + 5, backdrop);
            }
        }
        at
    }
}

/// A version 1 `COLR` (*COLR header*, *BaseGlyphList, LayerList and
/// ClipList*): the 34-byte header, a `BaseGlyphList` of `bases` (in
/// increasing glyph order, as the chapter requires), the `LayerList` their
/// layer lists filled, a format 1 `ClipList` of `clips` where there are any,
/// and then every paint table.
fn colr_v1(bases: &[(u16, V1)], clips: &[(u16, u16, [i16; 4])]) -> Vec<u8> {
    let mut writer = V1Writer::default();
    let roots: Vec<(u16, usize)> = bases
        .iter()
        .map(|(glyph, paint)| (*glyph, writer.write(paint)))
        .collect();
    let base_list_at = 34usize;
    let layer_list_at = base_list_at + 4 + 6 * roots.len();
    let clip_list_at = layer_list_at + 4 + 4 * writer.layers.len();
    let clip_list_len = if clips.is_empty() {
        0
    } else {
        5 + 16 * clips.len()
    };
    let area_at = clip_list_at + clip_list_len;

    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(base_list_at as u32).to_be_bytes());
    out.extend_from_slice(&(layer_list_at as u32).to_be_bytes());
    let clip_offset = if clips.is_empty() { 0 } else { clip_list_at };
    out.extend_from_slice(&(clip_offset as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());

    out.extend_from_slice(&(roots.len() as u32).to_be_bytes());
    for (glyph, root) in &roots {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&((area_at + root - base_list_at) as u32).to_be_bytes());
    }
    out.extend_from_slice(&(writer.layers.len() as u32).to_be_bytes());
    for layer in &writer.layers {
        out.extend_from_slice(&((area_at + layer - layer_list_at) as u32).to_be_bytes());
    }
    if !clips.is_empty() {
        out.push(1);
        out.extend_from_slice(&(clips.len() as u32).to_be_bytes());
        for (i, (start, end, _)) in clips.iter().enumerate() {
            out.extend_from_slice(&start.to_be_bytes());
            out.extend_from_slice(&end.to_be_bytes());
            let box_at = 5 + 7 * clips.len() + 9 * i;
            out.extend_from_slice(&(box_at as u32).to_be_bytes()[1..]);
        }
        for (_, _, rect) in clips {
            out.push(1);
            for value in rect {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    out.extend_from_slice(&writer.area);
    out
}

/// The rectangles a version 1 face's graphs clip to, at
/// `Face::colour_glyph(n)`.
const V1_RECTS: [[i16; 4]; 7] = [
    // 0: the square every glyph below uses.
    [100, 100, 500, 500],
    // 1: a larger square around the em's middle.
    [200, 200, 800, 800],
    // 2: the em.
    [0, 0, 1000, 1000],
    // 3: a bar along the x axis, centred at (500, 0).
    [300, -50, 700, 50],
    // 4: a bar below the em's middle.
    [400, 100, 600, 400],
    // 5 and 6: a composite's backdrop, lower left, and its source, upper
    // right, overlapping in the em's middle.
    [100, 100, 600, 600],
    [400, 400, 900, 900],
];

/// Palette 0's entries: red, blue, green, and four colours no two of which
/// differ by more than three quarters of a channel, which the gradients ramp
/// between so that a ramp moves a few levels a pixel and a seam many.
const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 255, 0];
const WHITE: [u8; 3] = [255, 255, 255];
const ORANGE: [u8; 3] = [230, 120, 40];
const AZURE: [u8; 3] = [40, 110, 210];
const LEAF: [u8; 3] = [60, 200, 90];
const PLUM: [u8; 3] = [200, 60, 170];

/// Palette 0, entry by entry.
const V1_ENTRIES: [[u8; 3]; 7] = [RED, BLUE, GREEN, ORANGE, AZURE, LEAF, PLUM];

fn v1_palette() -> Vec<u8> {
    cpal(&[V1_ENTRIES
        .iter()
        .map(|[r, g, b]| [*r, *g, *b, 255])
        .collect()])
}

/// `source` at `alpha` over `backdrop`, unrounded alpha: simple alpha
/// compositing, rounded to eight bits.
fn over_at(source: [u8; 3], alpha: f64, backdrop: [u8; 3]) -> [u8; 3] {
    let mut out = [0u8; 3];
    for i in 0..3 {
        let value = f64::from(source[i]) * alpha + f64::from(backdrop[i]) * (1.0 - alpha);
        out[i] = value.round() as u8;
    }
    out
}

/// A face covering `letters`, a full em each, whose `COLR` version 1 graphs
/// `graphs` makes from its rectangle glyphs' ids and its letters' glyph ids,
/// with `clips` (by letter) as its `ClipList`.
fn v1_face(
    letters: &str,
    graphs: impl Fn(&dyn Fn(usize) -> u16, &dyn Fn(char) -> u16) -> Vec<(char, V1)>,
    clips: &[(char, [i16; 4])],
) -> Face {
    let base = Face::new("Fixture COLR", letters).with_advance(1000);
    let rect = |n: usize| base.colour_glyph(n);
    let letter = |c: char| base.glyph_of(c).expect("a covered letter");
    let mut bases: Vec<(u16, V1)> = graphs(&rect, &letter)
        .into_iter()
        .map(|(c, paint)| (letter(c), paint))
        .collect();
    bases.sort_by_key(|(glyph, _)| *glyph);
    let clips: Vec<(u16, u16, [i16; 4])> = clips
        .iter()
        .map(|&(c, rect)| (letter(c), letter(c), rect))
        .collect();
    let colr = colr_v1(&bases, &clips);
    base.clone().with_colour(Colour {
        rectangles: V1_RECTS.to_vec(),
        colr,
        cpal: v1_palette(),
    })
}

/// `Glyph(rect, Solid(entry))`, the commonest graph below.
fn filled(rect: u16, entry: u16) -> V1 {
    V1::Glyph(rect, Box::new(V1::Solid(entry, 1.0)))
}

/// **FT-06.12's exit.** One letter a paint format, each glyph's graph
/// written, drawn through `shaping::write_run` and rendered: at a sample
/// point inside the transformed clip, the colour the graph paints there,
/// and at one outside it, the page — every place computed here from the
/// graph's own affine arithmetic, the matrices of *Formats 12 to 31*, and
/// every colour from the palette and simple alpha compositing.
#[test]
fn colr_v1_solids_glyphs_and_transforms_draw_by_the_formulas() {
    let letters = "ABCDEFGHIJKLMNOP";
    let face = v1_face(
        letters,
        |rect, letter| {
            let r0 = rect(0);
            vec![
                // Formats 10 and 2.
                ('A', filled(r0, 0)),
                // Format 1: blue under green at half.
                (
                    'B',
                    V1::Layers(vec![
                        filled(rect(1), 1),
                        V1::Glyph(r0, Box::new(V1::Solid(2, 0.5))),
                    ]),
                ),
                // Format 14.
                ('C', V1::Translate(300, 200, Box::new(filled(r0, 0)))),
                // Formats 16, 18, 20 and 22.
                ('D', V1::Scale(1.5, 0.5, None, Box::new(filled(r0, 0)))),
                (
                    'E',
                    V1::Scale(0.5, 0.5, Some([300, 300]), Box::new(filled(r0, 0))),
                ),
                ('F', V1::ScaleUniform(1.5, None, Box::new(filled(r0, 0)))),
                (
                    'G',
                    V1::ScaleUniform(0.5, Some([100, 100]), Box::new(filled(r0, 0))),
                ),
                // Formats 24 and 26: a quarter of 180° about the origin, and a
                // half about the em's middle.
                ('H', V1::Rotate(0.25, None, Box::new(filled(rect(3), 0)))),
                (
                    'I',
                    V1::Rotate(0.5, Some([500, 500]), Box::new(filled(rect(4), 0))),
                ),
                // Formats 28 and 30: 45° along y about the origin, and 45°
                // along x about (300, 300).
                ('J', V1::Skew(0.0, 0.25, None, Box::new(filled(r0, 0)))),
                (
                    'K',
                    V1::Skew(0.25, 0.0, Some([300, 300]), Box::new(filled(r0, 0))),
                ),
                // Format 12: a quarter turn and a move of an em along x.
                (
                    'L',
                    V1::Transform([0.0, 1.0, -1.0, 0.0, 1000.0, 0.0], Box::new(filled(r0, 0))),
                ),
                // Format 11: `A`'s graph.
                ('M', V1::ColrGlyph(letter('A'))),
                // A clip box over the em's lower left quarter.
                ('N', filled(rect(2), 1)),
                // The text's own colour.
                ('O', V1::Glyph(r0, Box::new(V1::Solid(FOREGROUND, 1.0)))),
                // Format 3, at the default instance.
                ('P', V1::Glyph(r0, Box::new(V1::VarSolid(1, 1.0)))),
            ]
        },
        &[('N', [0, 0, 500, 500])],
    );
    let (bytes, warnings) = colour_document(
        &face,
        letters,
        PaletteChoice::Default,
        DocumentBuilder::new(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let (bitmap, text) = rendered(bytes);
    assert_eq!(text, letters);

    // A point of glyph space through a matrix, for the rotations and skews.
    let through = |matrix: [f64; 6], [x, y]: [f64; 2]| {
        [
            matrix[0] * x + matrix[2] * y + matrix[4],
            matrix[1] * x + matrix[3] * y + matrix[5],
        ]
    };
    let quarter = std::f64::consts::FRAC_PI_4;
    let rotate_45 = [
        quarter.cos(),
        quarter.sin(),
        -quarter.sin(),
        quarter.cos(),
        0.0,
        0.0,
    ];
    let samples: Vec<(char, [f64; 2], [u8; 3])> = vec![
        ('A', [300.0, 300.0], RED),
        ('A', [700.0, 700.0], WHITE),
        // Green at half over blue where both are, blue alone, green alone
        // over the page.
        ('B', [300.0, 300.0], over_at(GREEN, 0.5, BLUE)),
        ('B', [700.0, 700.0], BLUE),
        ('B', [150.0, 150.0], over_at(GREEN, 0.5, WHITE)),
        ('B', [900.0, 900.0], WHITE),
        // The square moved to (400, 300)-(800, 700).
        ('C', [600.0, 500.0], RED),
        ('C', [300.0, 300.0], WHITE),
        // Scaled to (150, 50)-(750, 250).
        ('D', [450.0, 150.0], RED),
        ('D', [300.0, 350.0], WHITE),
        // Halved about its middle: (200, 200)-(400, 400).
        ('E', [300.0, 300.0], RED),
        ('E', [150.0, 150.0], WHITE),
        ('E', [450.0, 450.0], WHITE),
        // Scaled to (150, 150)-(750, 750).
        ('F', [700.0, 700.0], RED),
        ('F', [850.0, 850.0], WHITE),
        // Halved about (100, 100): (100, 100)-(300, 300).
        ('G', [200.0, 200.0], RED),
        ('G', [400.0, 400.0], WHITE),
        // The bar's middle, turned 45° about the origin; and where it was.
        ('H', through(rotate_45, [500.0, 0.0]), RED),
        ('H', [500.0, 30.0], WHITE),
        // Turned a half of 180° about (500, 500): (x, y) to (1000 - y, x).
        ('I', [1000.0 - 250.0, 500.0], RED),
        ('I', [500.0, 250.0], WHITE),
        // y' = x + y: (300, 300) to (300, 600); (300, 200) comes from
        // (300, -100), outside the square.
        ('J', [300.0, 600.0], RED),
        ('J', [300.0, 200.0], WHITE),
        // x' = x - y + 300 about (300, 300): (300, 400) to (200, 400); (600,
        // 300) comes from (600, 300), outside.
        ('K', [200.0, 400.0], RED),
        ('K', [600.0, 300.0], WHITE),
        // x' = 1000 - y, y' = x: (300, 300) to (700, 300).
        ('L', [700.0, 300.0], RED),
        ('L', [300.0, 300.0], WHITE),
        ('M', [300.0, 300.0], RED),
        ('M', [700.0, 700.0], WHITE),
        // The em filled blue, but for the clip box's quarter.
        ('N', [250.0, 250.0], BLUE),
        ('N', [750.0, 750.0], WHITE),
        ('N', [250.0, 750.0], WHITE),
        ('O', [300.0, 300.0], TEXT_GREEN),
        ('P', [300.0, 300.0], BLUE),
        ('P', [700.0, 700.0], WHITE),
    ];
    for (letter, unit, want) in samples {
        let pen = letters.find(letter).expect("a letter") as f64;
        let point = at_units(pen, unit);
        let got = pixel(&bitmap, point, 100.0);
        assert!(
            close(got, want, 2),
            "{letter} at {unit:?} ({point:?}) is {got:?}, not {want:?}"
        );
    }
}

/// **FT-06.12's exit**, as FT-06.14 leaves it: a composite whose mode no
/// construct of ISO 32000-2's transparency expresses is drawn source-over —
/// the backdrop, then the source over it — and named with its mode (ruling
/// 10) until its own step draws it; a source-over composite is that, and
/// named not at all.
#[test]
fn a_composite_is_named_until_it_is_drawn() {
    let face = v1_face(
        "AB",
        |rect, _| {
            let composite = |mode| {
                V1::Composite(
                    Box::new(filled(rect(0), 1)),
                    mode,
                    Box::new(filled(rect(1), 0)),
                )
            };
            // 9, COMPOSITE_SRC_ATOP; 3, COMPOSITE_SRC_OVER.
            vec![('A', composite(9)), ('B', composite(3))]
        },
        &[],
    );
    let (bytes, warnings) =
        colour_document(&face, "AB", PaletteChoice::Default, DocumentBuilder::new());
    let a = face.glyph_of('A').expect("covered");
    assert_eq!(
        warnings,
        [ColourWarning::CompositeDrawnSourceOver {
            glyph: a,
            mode: tinker_pdf_font::colr::CompositeMode::SrcAtop,
        }]
    );
    let (bitmap, _) = rendered(bytes);
    for pen in [0.0, 1.0] {
        for (unit, want) in [
            ([300.0, 300.0], BLUE),
            ([700.0, 700.0], RED),
            ([150.0, 150.0], BLUE),
            ([900.0, 900.0], WHITE),
        ] {
            let got = pixel(&bitmap, at_units(pen, unit), 100.0);
            assert!(close(got, want, 2), "{pen} {unit:?}: {got:?}");
        }
    }
}

// ---- COLR version 1 gradients ---------------------------------------------

/// What an `F2DOT14` the test wrote reads back as: the font holds the value
/// to a 16 384th, and the formulas are evaluated on what it holds.
fn stored(value: f64) -> f64 {
    f64::from(i16::from_be_bytes(f2dot14(value))) / 16384.0
}

/// IEC 61966-2-1's inverse transfer function, as the CPAL chapter's
/// *Interpolation of colors* has a `COLR` gradient apply it.
fn to_linear(encoded: f64) -> f64 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// IEC 61966-2-1's transfer function.
fn to_encoded(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The colour line `line` at position `s` over palette 0, as *Color lines*
/// and the CPAL chapter's *Interpolation of colors* state it: its encoded
/// colour and its alpha, and whether `s` lies beyond the line's stops.
///
/// The stops in offset order, those of one offset in the font's; the extend
/// applied past the first offset and the last; and between two stops the
/// alpha linear and each linear-light component with the alpha
/// premultiplied linear, divided by the alpha and re-encoded.
fn line_at(line: &Line, s: f64) -> ([f64; 3], f64, bool) {
    let mut stops: Vec<(f64, [f64; 3], f64)> = line
        .stops
        .iter()
        .map(|&(at, entry, alpha)| {
            let rgb = V1_ENTRIES[usize::from(entry)].map(|c| f64::from(c) / 255.0);
            (stored(at), rgb, stored(alpha).clamp(0.0, 1.0))
        })
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    let beyond = s < first.0 || s > last.0;
    let period = last.0 - first.0;
    let u = match line.extend {
        PAD if s < first.0 => return (first.1, first.2, beyond),
        PAD if s >= last.0 => return (last.1, last.2, beyond),
        PAD => s,
        REPEAT => first.0 + (s - first.0).rem_euclid(period),
        _ => {
            let x = ((s - first.0) / period).rem_euclid(2.0);
            first.0 + if x > 1.0 { 2.0 - x } else { x } * period
        }
    };
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.0 < b.0 && a.0 <= u && u < b.0 {
            let t = (u - a.0) / (b.0 - a.0);
            let alpha = a.2 + t * (b.2 - a.2);
            let colour = [0, 1, 2].map(|c| {
                let from = to_linear(a.1[c]) * a.2;
                let to = to_linear(b.1[c]) * b.2;
                let premultiplied = from + t * (to - from);
                if alpha > 0.0 {
                    to_encoded(premultiplied / alpha)
                } else {
                    0.0
                }
            });
            return (colour, alpha, beyond);
        }
    }
    (last.1, last.2, beyond)
}

/// A colour and its alpha over the white page, in eight bits.
fn over_white(colour: [f64; 3], alpha: f64) -> [u8; 3] {
    colour.map(|c| ((alpha * c + (1.0 - alpha)) * 255.0).round() as u8)
}

/// One gradient's geometry, as the test writes it and evaluates it.
#[derive(Clone, Debug)]
enum Gradient {
    /// `p0`, `p1`, `p2`.
    Linear([[i16; 2]; 3]),
    /// Two circles.
    Radial([([i16; 2], u16); 2]),
    /// A centre, and the start and end angles in degrees.
    Sweep([i16; 2], [f64; 2]),
}

impl Gradient {
    /// The position on the colour line at `p`, by the chapter's own
    /// definition of each gradient, or `None` where it paints nothing.
    ///
    /// *Linear gradients*: the colour is constant along lines parallel to
    /// `p0p2`, offset 0 at `p0` and 1 at `p1`. *Radial gradients*: the
    /// circles `c(ω) = c0 + ω·(c1 − c0)`, `r(ω) = r0 + ω·(r1 − r0)`, drawn
    /// from the ω nearest +∞ down for every ω whose radius is positive, so a
    /// point takes the largest ω whose circle passes through it. *Sweep
    /// gradients*: the angle in `[0°, 360°)` counter-clockwise from the
    /// positive x axis, offset 0 at the start angle and 1 at the end.
    fn position(&self, p: [f64; 2]) -> Option<f64> {
        let point = |q: [i16; 2]| [f64::from(q[0]), f64::from(q[1])];
        match self {
            Gradient::Linear(points) => {
                let [p0, p1, p2] = points.map(point);
                let normal = [p0[1] - p2[1], p2[0] - p0[0]];
                let along = (p[0] - p0[0]) * normal[0] + (p[1] - p0[1]) * normal[1];
                let unit = (p1[0] - p0[0]) * normal[0] + (p1[1] - p0[1]) * normal[1];
                Some(along / unit)
            }
            Gradient::Radial([(c0, r0), (c1, r1)]) => {
                let (c0, c1) = (point(*c0), point(*c1));
                let (r0, r1) = (f64::from(*r0), f64::from(*r1));
                let (dx, dy, dr) = (c1[0] - c0[0], c1[1] - c0[1], r1 - r0);
                let (px, py) = (p[0] - c0[0], p[1] - c0[1]);
                // |p − c(ω)|² = r(ω)²: a ω² − 2 b ω + c = 0.
                let a = dx * dx + dy * dy - dr * dr;
                let b = px * dx + py * dy + r0 * dr;
                let c = px * px + py * py - r0 * r0;
                let roots: Vec<f64> = if a.abs() < 1e-9 {
                    if b == 0.0 {
                        Vec::new()
                    } else {
                        vec![c / (2.0 * b)]
                    }
                } else {
                    let discriminant = b * b - a * c;
                    if discriminant < 0.0 {
                        Vec::new()
                    } else {
                        let root = discriminant.sqrt();
                        vec![(b + root) / a, (b - root) / a]
                    }
                };
                roots
                    .into_iter()
                    .filter(|omega| r0 + omega * dr > 0.0)
                    .fold(None, |best: Option<f64>, omega| {
                        Some(best.map_or(omega, |b| b.max(omega)))
                    })
            }
            Gradient::Sweep(centre, [start, end]) => {
                let centre = point(*centre);
                let mut angle = (p[1] - centre[1]).atan2(p[0] - centre[0]).to_degrees();
                if angle < 0.0 {
                    angle += 360.0;
                }
                // The angles as the format holds them: biased by one, 180° a
                // unit, to a 16 384th.
                let held = |degrees: f64| (stored(degrees / 180.0 - 1.0) + 1.0) * 180.0;
                Some((angle - held(*start)) / (held(*end) - held(*start)))
            }
        }
    }
}

/// One glyph of a gradient fixture: its letter, its gradient over the em, its
/// colour line, and whether it is written in the variable format.
#[derive(Clone, Debug)]
struct Case {
    letter: char,
    gradient: Gradient,
    line: Line,
    variable: bool,
}

impl Case {
    fn paint(&self) -> V1 {
        let (line, variable) = (self.line.clone(), self.variable);
        match self.gradient.clone() {
            Gradient::Linear(points) => V1::Linear {
                line,
                points,
                variable,
            },
            Gradient::Radial(circles) => V1::Radial {
                line,
                circles,
                variable,
            },
            Gradient::Sweep(centre, angles) => V1::Sweep {
                line,
                centre,
                angles,
                variable,
            },
        }
    }

    /// The colour the formulas give the glyph at `unit` over the white page,
    /// and whether the position there lies beyond the colour line's stops.
    fn expected(&self, unit: [f64; 2]) -> ([u8; 3], bool) {
        match self.gradient.position(unit) {
            None => (WHITE, false),
            Some(s) => {
                let (colour, alpha, beyond) = line_at(&self.line, s);
                (over_white(colour, alpha), beyond)
            }
        }
    }
}

/// The font-unit point at the centre of device pixel `(x, y)`, in the glyph
/// `pen` ems along [`colour_document`]'s run: where the renderer evaluates a
/// shading for that pixel.
fn unit_at(pen: f64, x: usize, y: usize) -> [f64; 2] {
    let page = ((x as f64 + 0.5) / SCALE, 100.0 - (y as f64 + 0.5) / SCALE);
    [
        ((page.0 - RUN_ORIGIN.0) / RUN_SIZE - pen) * 1000.0,
        (page.1 - RUN_ORIGIN.1) / RUN_SIZE * 1000.0,
    ]
}

/// The pixel at `(x, y)`.
fn pixel_at(bitmap: &Bitmap, x: usize, y: usize) -> [u8; 3] {
    let at = y * bitmap.stride + x * bitmap.components();
    let p = &bitmap.data[at..at + 3];
    [p[0], p[1], p[2]]
}

/// Holds each glyph of `cases`, rendered in `bitmap` as [`colour_document`]
/// draws `letters`, to its formulas over a grid of the em's inside, each
/// sample at the centre of a device pixel and compared with the colour the
/// formulas give that centre, within `tolerance`.
///
/// A sample is kept only where the formulas give every pixel within two of
/// it a colour within 40 levels of its own: a seam, a cone's edge or a hard
/// stop there would make the pixel's colour a matter of which side of an edge
/// its centre fell, so it is passed over (`docs/design/colour-fonts.md`, *How
/// tests compare pixels*). Each glyph keeps at least half its samples, and at
/// least `beyond` of them lie past its colour line's stops, where the extend
/// decides.
fn hold_gradients(bitmap: &Bitmap, letters: &str, cases: &[Case], tolerance: u8, beyond: usize) {
    for case in cases {
        let pen = letters.find(case.letter).expect("a letter") as f64;
        let (mut kept, mut past) = (0usize, 0usize);
        let grid = 18usize;
        for gx in 0..grid {
            for gy in 0..grid {
                let unit = [75.0 + 50.0 * gx as f64, 75.0 + 50.0 * gy as f64];
                let point = at_units(pen, unit);
                let (x, y) = (
                    (point.0 * SCALE) as usize,
                    ((100.0 - point.1) * SCALE) as usize,
                );
                let (want, is_beyond) = case.expected(unit_at(pen, x, y));
                let smooth = (-2i64..=2).all(|dx| {
                    (-2i64..=2).all(|dy| {
                        let near = unit_at(pen, (x as i64 + dx) as usize, (y as i64 + dy) as usize);
                        let (there, _) = case.expected(near);
                        close(there, want, 40)
                    })
                });
                if !smooth {
                    continue;
                }
                kept += 1;
                past += usize::from(is_beyond);
                let got = pixel_at(bitmap, x, y);
                assert!(
                    close(got, want, tolerance),
                    "{case:?} at {unit:?}, pixel ({x}, {y}): {got:?}, not {want:?}"
                );
            }
        }
        assert!(
            kept * 2 >= grid * grid,
            "{}: only {kept} samples are away from an edge",
            case.letter
        );
        assert!(
            past >= beyond,
            "{}: only {past} samples lie past the colour line",
            case.letter
        );
    }
}

/// The em, as the outline every gradient below fills.
const EM_RECT: usize = 2;

/// **FT-06.13's exit.** One letter a gradient and an extend mode — linear,
/// radial and sweep, each padded, repeated and reflected — and four more:
/// each variable format at its default instance, a cone whose outside paints
/// nothing, a sweep running clockwise, a line written out of order with a
/// hard stop, and two lines with a translucent stop. Each glyph fills the em
/// with its gradient, written through `shaping::write_run` and rendered, and
/// a grid of samples inside the em, along the colour line and beyond it,
/// carries the colour the test computes from the chapter's formula for that
/// gradient, the CPAL chapter's linear-light interpolation and simple alpha
/// compositing over the page, within FT-06.1's three levels.
#[test]
fn colr_v1_gradients_match_the_specification_formulas() {
    let cases = vec![
        Case {
            letter: 'A',
            gradient: Gradient::Linear([[300, 300], [700, 300], [400, 900]]),
            line: line(PAD, &[(0.0, 3), (1.0, 4)]),
            variable: false,
        },
        Case {
            letter: 'B',
            gradient: Gradient::Linear([[200, 500], [650, 500], [200, 900]]),
            line: line(REPEAT, &[(0.0, 3), (1.0, 5)]),
            variable: false,
        },
        Case {
            letter: 'C',
            gradient: Gradient::Linear([[500, 200], [500, 600], [900, 200]]),
            line: line(REFLECT, &[(0.0, 6), (1.0, 4)]),
            variable: false,
        },
        Case {
            letter: 'D',
            gradient: Gradient::Radial([([500, 500], 100), ([500, 500], 450)]),
            line: line(PAD, &[(0.0, 5), (1.0, 3)]),
            variable: false,
        },
        Case {
            letter: 'E',
            gradient: Gradient::Radial([([420, 500], 0), ([520, 500], 400)]),
            line: line(REPEAT, &[(0.0, 4), (1.0, 6)]),
            variable: false,
        },
        Case {
            letter: 'F',
            gradient: Gradient::Radial([([500, 500], 150), ([500, 500], 450)]),
            line: line(REFLECT, &[(0.0, 3), (1.0, 5)]),
            variable: false,
        },
        Case {
            letter: 'G',
            gradient: Gradient::Sweep([500, 500], [45.0, 225.0]),
            line: line(PAD, &[(0.0, 3), (1.0, 4)]),
            variable: false,
        },
        Case {
            letter: 'H',
            gradient: Gradient::Sweep([500, 500], [0.0, 135.0]),
            line: line(REPEAT, &[(0.0, 4), (1.0, 5)]),
            variable: false,
        },
        Case {
            letter: 'I',
            gradient: Gradient::Sweep([450, 550], [90.0, 270.0]),
            line: line(REFLECT, &[(0.0, 6), (1.0, 3)]),
            variable: false,
        },
        // Format 5, its second stop at half an alpha: a soft mask.
        Case {
            letter: 'J',
            gradient: Gradient::Linear([[150, 500], [850, 500], [150, 900]]),
            line: Line {
                extend: PAD,
                stops: vec![(0.0, 3, 1.0), (1.0, 4, 0.5)],
            },
            variable: true,
        },
        // Format 7: a cone, neither circle holding the other, open to the
        // right; behind its tip and outside its sides nothing is painted.
        Case {
            letter: 'K',
            gradient: Gradient::Radial([([300, 500], 60), ([650, 500], 200)]),
            line: line(PAD, &[(0.0, 5), (1.0, 6)]),
            variable: true,
        },
        // Format 9: the start past the end, so the line runs clockwise.
        Case {
            letter: 'L',
            gradient: Gradient::Sweep([500, 500], [270.0, 90.0]),
            line: line(PAD, &[(0.0, 3), (1.0, 6)]),
            variable: true,
        },
        // Stops written out of offset order, two at one offset: below it the
        // first of the two, at and past it the second.
        Case {
            letter: 'M',
            gradient: Gradient::Linear([[100, 500], [900, 500], [100, 900]]),
            line: line(REPEAT, &[(1.0, 3), (0.0, 4), (0.5, 5), (0.5, 6)]),
            variable: false,
        },
        // A radial line whose first stop is a quarter opaque, reflected.
        Case {
            letter: 'N',
            gradient: Gradient::Radial([([500, 500], 100), ([500, 500], 400)]),
            line: Line {
                extend: REFLECT,
                stops: vec![(0.0, 3, 0.25), (1.0, 6, 1.0)],
            },
            variable: false,
        },
    ];
    let letters: String = cases.iter().map(|case| case.letter).collect();
    let face = v1_face(
        &letters,
        |rect, _| {
            cases
                .iter()
                .map(|case| {
                    (
                        case.letter,
                        V1::Glyph(rect(EM_RECT), Box::new(case.paint())),
                    )
                })
                .collect()
        },
        &[],
    );
    let (bytes, warnings) = colour_document(
        &face,
        &letters,
        PaletteChoice::Default,
        DocumentBuilder::new(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let document = Document::open(bytes.clone()).expect("opens");
    assert!(document.validate().is_empty(), "{:?}", document.validate());
    let (bitmap, text) = rendered(bytes);
    assert_eq!(text, letters);
    hold_gradients(&bitmap, &letters, &cases, 3, 10);
}

/// What the chapter makes of a colour line or a geometry that does not
/// describe a gradation: an ill-formed linear gradient — `p2` on `p0`, `p0p2`
/// along `p0p1` — and an ill-formed radial one — two equal circles, two of
/// no radius — paint nothing and are named; so do a repeating line whose
/// stops share one offset and a repeating sweep over no angle. A padded line
/// whose stops share an offset is the first stop below it and the last at
/// and past it; a padded sweep over no angle is the first stop short of it
/// and the last from it on; a line of one stop is that stop's colour, alpha
/// and all, everywhere; and a line of none is transparent black.
#[test]
fn colour_lines_and_geometries_that_describe_no_gradation_paint_as_the_chapter_says() {
    let ill: Vec<(char, V1)> = vec![
        (
            'A',
            V1::Linear {
                line: line(PAD, &[(0.0, 3), (1.0, 4)]),
                points: [[200, 200], [800, 200], [200, 200]],
                variable: false,
            },
        ),
        (
            'B',
            V1::Linear {
                line: line(PAD, &[(0.0, 3), (1.0, 4)]),
                points: [[200, 200], [800, 200], [500, 200]],
                variable: false,
            },
        ),
        (
            'C',
            V1::Radial {
                line: line(PAD, &[(0.0, 3), (1.0, 4)]),
                circles: [([500, 500], 200), ([500, 500], 200)],
                variable: false,
            },
        ),
        (
            'D',
            V1::Radial {
                line: line(PAD, &[(0.0, 3), (1.0, 4)]),
                circles: [([300, 500], 0), ([700, 500], 0)],
                variable: false,
            },
        ),
        (
            'E',
            V1::Sweep {
                line: line(REPEAT, &[(0.0, 3), (1.0, 4)]),
                centre: [500, 500],
                angles: [90.0, 90.0],
                variable: false,
            },
        ),
        (
            'F',
            V1::Linear {
                line: line(REPEAT, &[(0.5, 3), (0.5, 4)]),
                points: [[200, 500], [800, 500], [200, 900]],
                variable: false,
            },
        ),
    ];
    let padded_hard = Case {
        letter: 'G',
        gradient: Gradient::Linear([[200, 500], [800, 500], [200, 900]]),
        line: line(PAD, &[(0.5, 3), (0.5, 4)]),
        variable: false,
    };
    let padded_sweep = Case {
        letter: 'H',
        gradient: Gradient::Sweep([500, 500], [90.0, 90.0]),
        line: line(PAD, &[(0.0, 5), (1.0, 6)]),
        variable: false,
    };
    let one_stop = V1::Linear {
        line: Line {
            extend: REPEAT,
            stops: vec![(0.3, 5, 0.5)],
        },
        points: [[200, 500], [800, 500], [200, 900]],
        variable: false,
    };
    let no_stop = V1::Radial {
        line: Line {
            extend: PAD,
            stops: Vec::new(),
        },
        circles: [([500, 500], 0), ([500, 500], 400)],
        variable: false,
    };
    let letters = "ABCDEFGHIJ";
    let face = v1_face(
        letters,
        |rect, _| {
            let mut graphs: Vec<(char, V1)> = ill
                .iter()
                .cloned()
                .map(|(letter, paint)| (letter, V1::Glyph(rect(EM_RECT), Box::new(paint))))
                .collect();
            for case in [&padded_hard, &padded_sweep] {
                graphs.push((
                    case.letter,
                    V1::Glyph(rect(EM_RECT), Box::new(case.paint())),
                ));
            }
            graphs.push(('I', V1::Glyph(rect(EM_RECT), Box::new(one_stop.clone()))));
            graphs.push(('J', V1::Glyph(rect(EM_RECT), Box::new(no_stop.clone()))));
            graphs
        },
        &[],
    );
    let (bytes, warnings) = colour_document(
        &face,
        letters,
        PaletteChoice::Default,
        DocumentBuilder::new(),
    );
    let glyph = |c| face.glyph_of(c).expect("covered");
    for (letter, format) in [('A', 4), ('B', 4), ('C', 6), ('D', 6), ('E', 8), ('F', 4)] {
        assert!(
            warnings.contains(&ColourWarning::GradientIllFormed {
                glyph: glyph(letter),
                format,
            }),
            "{letter}: {warnings:?}"
        );
    }
    assert_eq!(warnings.len(), 6, "{warnings:?}");
    let (bitmap, text) = rendered(bytes);
    assert_eq!(text, letters);
    // The ill-formed six and the line of no stop paint nothing at all: not
    // their outline, which the box from 0 to 700 would show, either.
    for letter in ['A', 'B', 'C', 'D', 'E', 'F', 'J'] {
        let pen = letters.find(letter).expect("a letter") as f64;
        for unit in [
            [350.0, 350.0],
            [150.0, 850.0],
            [850.0, 150.0],
            [600.0, 600.0],
        ] {
            let got = pixel(&bitmap, at_units(pen, unit), 100.0);
            assert!(close(got, WHITE, 2), "{letter} at {unit:?}: {got:?}");
        }
    }
    hold_gradients(&bitmap, letters, &[padded_hard, padded_sweep], 3, 10);
    // One stop: its colour at half an alpha over the page, wherever it is.
    let pen = letters.find('I').expect("a letter") as f64;
    let want = over_white(LEAF.map(|c| f64::from(c) / 255.0), stored(0.5));
    for unit in [[150.0, 150.0], [500.0, 500.0], [850.0, 850.0]] {
        let got = pixel(&bitmap, at_units(pen, unit), 100.0);
        assert!(close(got, want, 2), "I at {unit:?}: {got:?}, not {want:?}");
    }
}

/// Under a profile that admits no transparency (ISO 19005-1 6.4), an opaque
/// gradient is drawn — a shading needs no group — through a form with no
/// `/Group`, and one with a translucent stop, which needs a soft mask, is
/// drawn as its outline in the text's colour and named; the builder refuses
/// nothing.
#[test]
fn a_translucent_gradient_under_a_profile_without_transparency_is_drawn_as_the_outline() {
    let opaque = Case {
        letter: 'A',
        gradient: Gradient::Linear([[200, 500], [800, 500], [200, 900]]),
        line: line(PAD, &[(0.0, 3), (1.0, 4)]),
        variable: false,
    };
    let translucent = Line {
        extend: PAD,
        stops: vec![(0.0, 3, 1.0), (1.0, 4, 0.5)],
    };
    let face = v1_face(
        "AB",
        |rect, _| {
            vec![
                ('A', V1::Glyph(rect(EM_RECT), Box::new(opaque.paint()))),
                (
                    'B',
                    V1::Glyph(
                        rect(EM_RECT),
                        Box::new(V1::Linear {
                            line: translucent.clone(),
                            points: [[200, 500], [800, 500], [200, 900]],
                            variable: false,
                        }),
                    ),
                ),
            ]
        },
        &[],
    );
    let builder = DocumentBuilder::archival(tinker_pdf_cos::ArchivalProfile {
        part: tinker_pdf_cos::ArchivalPart::One,
        level: Some(tinker_pdf_cos::ArchivalLevel::B),
        destination_profile: pdfa_support::srgb_like(),
        destination_space: tinker_pdf_cos::DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    });
    let (bytes, warnings) = colour_document(&face, "AB", PaletteChoice::Default, builder);
    let b = face.glyph_of('B').expect("covered");
    assert_eq!(
        warnings,
        [ColourWarning::TransparencyForbidden { glyph: b }]
    );
    let fonts = type3_fonts(&bytes);
    let cos = CosDocument::open(bytes.clone()).expect("opens");
    let font = written_font(&cos, fonts[0].1);
    for (glyph, form) in &font.forms {
        assert!(
            form.get(cos.intern(b"Group")).is_none(),
            "{glyph:?} has no group"
        );
    }
    let (bitmap, _) = rendered(bytes);
    hold_gradients(&bitmap, "AB", &[opaque], 3, 10);
    assert!(close(
        pixel(&bitmap, at_units(1.0, [350.0, 350.0]), 100.0),
        TEXT_GREEN,
        2
    ));
    assert!(close(
        pixel(&bitmap, at_units(1.0, [850.0, 850.0]), 100.0),
        WHITE,
        2
    ));
}

// ---- COLR version 1 composites --------------------------------------------

/// W3C Compositing and Blending Level 1, section 9.1: the two fractions of
/// the Porter-Duff operator composite mode `mode` names — of the source and
/// of the backdrop — given their alphas; `None` for a blend mode.
fn porter_duff(mode: u8, source: f64, backdrop: f64) -> Option<(f64, f64)> {
    Some(match mode {
        // clear, copy, destination
        0 => (0.0, 0.0),
        1 => (1.0, 0.0),
        2 => (0.0, 1.0),
        // source-over, destination-over
        3 => (1.0, 1.0 - source),
        4 => (1.0 - backdrop, 1.0),
        // source-in, destination-in, source-out, destination-out
        5 => (backdrop, 0.0),
        6 => (0.0, source),
        7 => (1.0 - backdrop, 0.0),
        8 => (0.0, 1.0 - source),
        // source-atop, destination-atop, xor
        9 => (backdrop, 1.0 - source),
        10 => (1.0 - backdrop, source),
        11 => (1.0 - backdrop, 1.0 - source),
        // lighter: both whole, the sum clamped.
        12 => (1.0, 1.0),
        _ => return None,
    })
}

/// Section 10.2's luminosity of a colour.
fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// Section 10.2's `ClipColor`.
fn clip_colour(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        out = out.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    out
}

/// Section 10.2's `SetLum`.
fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_colour(c.map(|v| v + d))
}

/// Section 10.2's `Sat`.
fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// Section 10.2's `SetSat`.
fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [min, mid, max] = order;
    let mut out = [0.0; 3];
    if c[max] > c[min] {
        out[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        out[max] = s;
    }
    out
}

/// The blend function `B(Cb, Cs)` of composite mode `mode`, sections 10.1
/// and 10.2, each channel in `[0, 1]`; `None` for a Porter-Duff mode.
fn blend(mode: u8, cb: [f64; 3], cs: [f64; 3]) -> Option<[f64; 3]> {
    let multiply = |b: f64, s: f64| b * s;
    let screen = |b: f64, s: f64| b + s - b * s;
    let hard_light = |b: f64, s: f64| {
        if s <= 0.5 {
            multiply(b, 2.0 * s)
        } else {
            screen(b, 2.0 * s - 1.0)
        }
    };
    let separable = |f: &dyn Fn(f64, f64) -> f64| [0, 1, 2].map(|i| f(cb[i], cs[i]));
    Some(match mode {
        13 => separable(&screen),
        14 => separable(&|b, s| hard_light(s, b)),
        15 => separable(&|b: f64, s: f64| b.min(s)),
        16 => separable(&|b: f64, s: f64| b.max(s)),
        17 => separable(&|b, s| {
            if b == 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }),
        18 => separable(&|b, s| {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }),
        19 => separable(&hard_light),
        20 => separable(&|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        21 => separable(&|b: f64, s: f64| (b - s).abs()),
        22 => separable(&|b, s| b + s - 2.0 * b * s),
        23 => separable(&multiply),
        24 => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        25 => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        26 => set_lum(cs, lum(cb)),
        27 => set_lum(cb, lum(cs)),
        _ => return None,
    })
}

/// The source `(cs, αs)` composited onto the backdrop `(cb, αb)` by `mode`,
/// W3C Compositing Level 1 transcribed: premultiplied, `co` and `αo`.
fn composited(mode: u8, cs: [f64; 3], alpha_s: f64, cb: [f64; 3], alpha_b: f64) -> ([f64; 3], f64) {
    if let Some((fa, fb)) = porter_duff(mode, alpha_s, alpha_b) {
        let colour = [0, 1, 2].map(|i| (alpha_s * fa * cs[i] + alpha_b * fb * cb[i]).min(1.0));
        return (colour, (alpha_s * fa + alpha_b * fb).min(1.0));
    }
    let blended = blend(mode, cb, cs).expect("a blend mode");
    // Section 5.8: the source, blended with the backdrop where it lies on
    // it, composited source-over.
    let colour = [0, 1, 2].map(|i| {
        alpha_s * (1.0 - alpha_b) * cs[i]
            + alpha_b * (1.0 - alpha_s) * cb[i]
            + alpha_s * alpha_b * blended[i]
    });
    (colour, alpha_s + alpha_b * (1.0 - alpha_s))
}

/// Composite modes 9 to 12, which no construct of ISO 32000-2's transparency
/// expresses (`docs/design/colour-fonts.md`, (c)).
const INEXPRESSIBLE: [u8; 4] = [9, 10, 11, 12];

/// **FT-06.14's exit.** Every composite mode COLR version 1 defines, each
/// one glyph: a blue square at half an alpha, the source, over an orange one
/// at three quarters, the backdrop, overlapping in the em's middle. At a
/// point of each, of both and of neither, each mode the transparency model
/// expresses draws what W3C Compositing Level 1's Porter-Duff or blend
/// formula gives, evaluated here over the page's encoded components, which
/// is where the design writes a composite until H-141 (d) rules; each of the
/// four it does not express is drawn source-over and named with its mode.
#[test]
fn colr_v1_composites_match_their_formulas() {
    // The source's and the backdrop's alphas, eight bits each as the writer
    // states them.
    let alpha = |paint: f64| (stored(paint) * 255.0).round() / 255.0;
    let (alpha_s, alpha_b) = (alpha(0.5), alpha(0.75));
    let encoded = |c: [u8; 3]| c.map(|v| f64::from(v) / 255.0);
    let (cs, cb) = (encoded(BLUE), encoded(ORANGE));
    let letters = "ABCDEFGHIJKLMN";
    for half in [0u8, 14] {
        let modes: Vec<u8> = (half..half + 14).collect();
        let face = v1_face(
            letters,
            |rect, _| {
                letters
                    .chars()
                    .zip(&modes)
                    .map(|(letter, &mode)| {
                        let source = V1::Glyph(rect(6), Box::new(V1::Solid(1, 0.5)));
                        let backdrop = V1::Glyph(rect(5), Box::new(V1::Solid(3, 0.75)));
                        (
                            letter,
                            V1::Composite(Box::new(source), mode, Box::new(backdrop)),
                        )
                    })
                    .collect()
            },
            &[],
        );
        let (bytes, warnings) = colour_document(
            &face,
            letters,
            PaletteChoice::Default,
            DocumentBuilder::new(),
        );
        let named: Vec<ColourWarning> = letters
            .chars()
            .zip(&modes)
            .filter(|(_, mode)| INEXPRESSIBLE.contains(mode))
            .map(|(letter, &mode)| ColourWarning::CompositeDrawnSourceOver {
                glyph: face.glyph_of(letter).expect("covered"),
                mode: tinker_pdf_font::colr::CompositeMode::from_raw(mode).expect("a mode"),
            })
            .collect();
        assert_eq!(warnings, named, "modes {half} to {}", half + 13);
        let (bitmap, text) = rendered(bytes);
        assert_eq!(text, letters);
        for (pen, &mode) in modes.iter().enumerate() {
            // Drawn source-over where no construct expresses the mode.
            let drawn = if INEXPRESSIBLE.contains(&mode) {
                3
            } else {
                mode
            };
            for (unit, (s, b)) in [
                ([250.0, 250.0], (0.0, alpha_b)),
                ([750.0, 750.0], (alpha_s, 0.0)),
                ([500.0, 500.0], (alpha_s, alpha_b)),
                ([250.0, 750.0], (0.0, 0.0)),
                ([750.0, 250.0], (0.0, 0.0)),
            ] {
                let (colour, coverage) = composited(drawn, cs, s, cb, b);
                // The result over the white page, premultiplied.
                let want = colour.map(|c| ((c + (1.0 - coverage)) * 255.0).round() as u8);
                let got = pixel(&bitmap, at_units(pen as f64, unit), 100.0);
                assert!(
                    close(got, want, 2),
                    "mode {mode} at {unit:?}: {got:?}, not {want:?}"
                );
            }
        }
    }
}

/// A composite whose construct needs a group, a soft mask or a blend mode —
/// the `IN` and `OUT` modes and the blends — is, under a profile that admits
/// no transparency, drawn as the glyph's outline and named; one whose
/// construct needs none — clear, one paint, one over the other — is drawn as
/// it is, its paints opaque.
#[test]
fn a_composite_needing_transparency_under_a_profile_without_it_is_drawn_as_the_outline() {
    let face = v1_face(
        "ABCD",
        |rect, _| {
            let composite = |mode| {
                V1::Composite(
                    Box::new(V1::Glyph(rect(6), Box::new(V1::Solid(1, 1.0)))),
                    mode,
                    Box::new(V1::Glyph(rect(5), Box::new(V1::Solid(3, 1.0)))),
                )
            };
            // DEST_OVER, SRC, SRC_IN, MULTIPLY.
            vec![
                ('A', composite(4)),
                ('B', composite(1)),
                ('C', composite(5)),
                ('D', composite(23)),
            ]
        },
        &[],
    );
    let builder = DocumentBuilder::archival(tinker_pdf_cos::ArchivalProfile {
        part: tinker_pdf_cos::ArchivalPart::One,
        level: Some(tinker_pdf_cos::ArchivalLevel::B),
        destination_profile: pdfa_support::srgb_like(),
        destination_space: tinker_pdf_cos::DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    });
    let (bytes, warnings) = colour_document(&face, "ABCD", PaletteChoice::Default, builder);
    let glyph = |c| face.glyph_of(c).expect("covered");
    assert_eq!(
        warnings,
        [
            ColourWarning::TransparencyForbidden { glyph: glyph('C') },
            ColourWarning::TransparencyForbidden { glyph: glyph('D') },
        ]
    );
    let (bitmap, _) = rendered(bytes);
    let samples = [
        // DEST_OVER: the backdrop over the source where both are.
        (0.0, [500.0, 500.0], ORANGE),
        (0.0, [750.0, 750.0], BLUE),
        // SRC: the source alone, the backdrop nowhere.
        (1.0, [500.0, 500.0], BLUE),
        (1.0, [250.0, 250.0], WHITE),
        // SRC_IN and MULTIPLY: the outline, the box from 0 to 700.
        (2.0, [350.0, 350.0], TEXT_GREEN),
        (2.0, [850.0, 850.0], WHITE),
        (3.0, [350.0, 350.0], TEXT_GREEN),
    ];
    for (pen, unit, want) in samples {
        let got = pixel(&bitmap, at_units(pen, unit), 100.0);
        assert!(close(got, want, 2), "{pen} {unit:?}: {got:?}, not {want:?}");
    }
}

/// What version 1 cannot draw falls to the glyph's next table, named: a
/// gradient whose colour line names the text's own colour, which a procedure
/// every run shares cannot state, a graph that is not bounded and has no clip
/// box, and a palette entry the palette lacks; a graph bounded only by its
/// clip box draws inside it; and a table refused for a cycle is named, the
/// rest of the graph drawn.
#[test]
fn version_1_graphs_it_cannot_draw_fall_to_the_next_table_and_are_named() {
    let face = v1_face(
        "ABCDE",
        |rect, letter| {
            let foreground = V1::Linear {
                line: line(PAD, &[(0.0, 0), (1.0, FOREGROUND)]),
                points: [[0, 0], [1000, 0], [0, 1000]],
                variable: false,
            };
            vec![
                ('A', V1::Glyph(rect(0), Box::new(foreground))),
                ('B', V1::Solid(0, 1.0)),
                ('C', V1::Solid(0, 1.0)),
                ('D', filled(rect(0), 9)),
                (
                    'E',
                    V1::Layers(vec![filled(rect(0), 1), V1::ColrGlyph(letter('E'))]),
                ),
            ]
        },
        &[('C', [100, 100, 400, 400])],
    );
    let (bytes, warnings) = colour_document(
        &face,
        "ABCDE",
        PaletteChoice::Default,
        DocumentBuilder::new(),
    );
    let glyph = |c| face.glyph_of(c).expect("covered");
    assert!(warnings.contains(&ColourWarning::ForegroundStop { glyph: glyph('A') }));
    assert!(warnings.contains(&ColourWarning::Unbounded { glyph: glyph('B') }));
    assert!(warnings.contains(&ColourWarning::PaletteEntryMissing {
        glyph: glyph('D'),
        entry: 9,
    }));
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            ColourWarning::Read {
                glyph,
                warning: tinker_pdf_font::colr::ColrWarning::PaintRefused(
                    tinker_pdf_font::colr::ColrRefusal::Cycle { .. }
                ),
            } if *glyph == face.glyph_of('E').expect("covered")
        )),
        "{warnings:?}"
    );
    let (bitmap, text) = rendered(bytes);
    assert_eq!(text, "ABCDE");
    let samples = [
        // A, B and D: each its own outline, the box from 0 to 700, in the
        // text's colour.
        (0.0, [350.0, 350.0], TEXT_GREEN),
        (0.0, [850.0, 850.0], WHITE),
        (1.0, [350.0, 350.0], TEXT_GREEN),
        (3.0, [350.0, 350.0], TEXT_GREEN),
        // C: red inside its clip box only.
        (2.0, [250.0, 250.0], RED),
        (2.0, [600.0, 600.0], WHITE),
        // E: its first layer, the cycle drawn as nothing.
        (4.0, [300.0, 300.0], BLUE),
        (4.0, [700.0, 700.0], WHITE),
    ];
    for (pen, unit, want) in samples {
        let got = pixel(&bitmap, at_units(pen, unit), 100.0);
        assert!(close(got, want, 2), "{pen} {unit:?}: {got:?}, not {want:?}");
    }
}

/// `LayersOfOne` nested `depth` times over `leaf`: one table at each level,
/// its two slots naming the level below, so the paths double at each.
fn fan_out(depth: usize, leaf: V1) -> V1 {
    (0..depth).fold(leaf, |below, _| V1::LayersOfOne(2, Box::new(below)))
}

/// A table that many parents name and that draws only inside its own
/// outlines is written once, as a form each parent draws: twenty levels of a
/// layer list naming the level below twice is a million paths and a page of
/// a few kilobytes, painting its square red.
#[test]
fn a_table_shared_by_many_parents_is_written_once() {
    let face = v1_face(
        "A",
        |rect, _| vec![('A', fan_out(20, filled(rect(0), 0)))],
        &[],
    );
    let (bytes, warnings) =
        colour_document(&face, "A", PaletteChoice::Default, DocumentBuilder::new());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(bytes.len() < 1 << 16, "{} bytes", bytes.len());
    let (bitmap, _) = rendered(bytes);
    assert!(close(
        pixel(&bitmap, at_units(0.0, [300.0, 300.0]), 100.0),
        RED,
        2
    ));
    assert!(close(
        pixel(&bitmap, at_units(0.0, [700.0, 700.0]), 100.0),
        WHITE,
        2
    ));
}

/// The same fan-out with a fill below it that no outline of its own bounds —
/// the clip is the glyph's, above the fan — cannot be one form, and its
/// million paths pass `MAX_DRAWN_PAINTS`: the glyph is drawn as its outline
/// and named (ruling 1).
#[test]
fn a_fan_out_past_the_walk_budget_is_named() {
    let face = v1_face(
        "A",
        |rect, _| {
            vec![(
                'A',
                V1::Glyph(rect(0), Box::new(fan_out(20, V1::Solid(0, 1.0)))),
            )]
        },
        &[],
    );
    let (bytes, warnings) =
        colour_document(&face, "A", PaletteChoice::Default, DocumentBuilder::new());
    let a = face.glyph_of('A').expect("covered");
    assert_eq!(warnings, [ColourWarning::GraphTooLarge { glyph: a }]);
    let (bitmap, _) = rendered(bytes);
    assert!(close(
        pixel(&bitmap, at_units(0.0, [350.0, 350.0]), 100.0),
        TEXT_GREEN,
        2
    ));
}
