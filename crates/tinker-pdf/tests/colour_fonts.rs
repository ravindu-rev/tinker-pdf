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
/// `palette`, under `builder`; what the face warned, and the document.
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
    builder.add_page(200.0, 100.0, |page| page.raw(&content));
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
