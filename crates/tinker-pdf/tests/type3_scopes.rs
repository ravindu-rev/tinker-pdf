//! A Type 3 glyph's procedure runs in **its font's own `/Resources`**.
//!
//! 9.6.5's Table 121 makes a Type 3 font's `/Resources` the named resources
//! of its glyph descriptions, and has a reader look in the resources of the
//! page the font is used on only where the font has none. Whole, as a form's
//! are: a name the font's resources lack resolves nowhere. A procedure run in
//! the scope that shows the glyph draws without a font, an image, a graphics
//! state, a colour space or a pattern that only its font's resources hold.
//!
//! Each page shows one glyph of `/T3` at 100 points over the whole page, its
//! procedure a `d0` glyph that paints with what it names. Table 121 does not
//! tell a font written directly into the page's `/Font` from one it refers to,
//! so every case is drawn with each. Every page is drawn three ways —
//! rendered, from its display list, and from its command stream — and each
//! way draws the same pixels.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | the interpreter and the renderer run the procedure where it is shown | 8 |
//! | the command stream records no Type 3 font's scope | 7 |
//! | a font written directly into the page's `/Font` is given no identity, so keeps the showing scope | 2 |

use tinker_pdf::{
    Bitmap, CommandField, CommandKind, CommandStream, Document, RenderOptions, SvgOptions,
};

/// Liberation Serif, the face the font kind's `H` is embedded in.
const SERIF: &[u8] =
    include_bytes!("../../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");

/// One resource kind a procedure can name, and the content that paints with
/// it, in glyph space, where the page is 1000 units square.
struct Kind {
    what: &'static str,
    /// The procedure's content after its `d0`, naming the resource.
    content: &'static str,
    /// The resource that paints where the procedure paints: the font's own,
    /// or the page's for a font with none.
    drawing: Resource,
    /// A second resource of the same name that paints otherwise.
    other: Resource,
}

/// Entries of one resource category: the category's key and what it holds,
/// merged with the page's own `/Font`, which names `/T3`.
#[derive(Clone, Copy)]
struct Resource {
    key: &'static str,
    entries: &'static str,
}

/// No resource at all.
const NONE: Resource = Resource {
    key: "",
    entries: "",
};

impl Resource {
    /// The resource as a dictionary's entries, beside a `/Font` of `fonts`.
    fn with_fonts(self, fonts: &str) -> String {
        match self.key {
            "Font" => format!("/Font << {fonts} {} >>", self.entries),
            "" if fonts.is_empty() => String::new(),
            "" => format!("/Font << {fonts} >>"),
            key if fonts.is_empty() => format!("/{key} << {} >>", self.entries),
            key => format!("/Font << {fonts} >> /{key} << {} >>", self.entries),
        }
    }
}

/// The five kinds, each painting with [`Kind::drawing`]: an embedded face's
/// `H`, where [`Kind::other`] is a Type 3 font whose `H` draws nothing;
/// a black image over the page, where the other is grey; a black fill at
/// alpha 0.5, where the other's alpha is 0.25; a grey fill through an
/// `/Indexed` space's index 1, where the other's is black, as is the colour
/// a space that is not there leaves; and a black shading pattern, where the
/// other is grey.
fn kinds() -> [Kind; 5] {
    [
        Kind {
            what: "font",
            content: "0 g BT /FT 800 Tf 100 200 Td (H) Tj ET",
            drawing: Resource {
                key: "Font",
                entries: "/FT 7 0 R",
            },
            other: Resource {
                key: "Font",
                entries: "/FT 14 0 R",
            },
        },
        Kind {
            what: "image",
            content: "q 1000 0 0 1000 0 0 cm /IT Do Q",
            drawing: Resource {
                key: "XObject",
                entries: "/IT 10 0 R",
            },
            other: Resource {
                key: "XObject",
                entries: "/IT 11 0 R",
            },
        },
        Kind {
            what: "graphics state",
            content: "/GT gs 0 g 0 0 1000 1000 re f",
            drawing: Resource {
                key: "ExtGState",
                entries: "/GT << /ca 0.5 >>",
            },
            other: Resource {
                key: "ExtGState",
                entries: "/GT << /ca 0.25 >>",
            },
        },
        Kind {
            what: "colour space",
            content: "/CT cs 1 sc 0 0 1000 1000 re f",
            drawing: Resource {
                key: "ColorSpace",
                entries: "/CT [/Indexed /DeviceGray 1 <FF80>]",
            },
            other: Resource {
                key: "ColorSpace",
                entries: "/CT [/Indexed /DeviceGray 1 <FF00>]",
            },
        },
        Kind {
            what: "pattern",
            content: "/Pattern cs /PT scn 0 0 1000 1000 re f",
            drawing: Resource {
                key: "Pattern",
                entries: "/PT 12 0 R",
            },
            other: Resource {
                key: "Pattern",
                entries: "/PT 13 0 R",
            },
        },
    ]
}

/// A 100-point page showing `/T3`'s `A` at 100 points from the origin, its
/// procedure (object 6) `1000 0 d0` and `content`; `own` is what the font's
/// `/Resources` hold, or none, and `page` what the page's hold beside `/T3`.
/// The font is object 5, to which the page's `/Font` refers.
/// Objects 7 to 15 are what a resource names: Liberation Serif, embedded
/// (its descriptor and program 8 and 9), a black and a grey one-pixel image,
/// a black and a grey shading pattern, and a Type 3 font whose `H` draws
/// nothing (its procedure 15).
fn pdf(page: Resource, own: Option<Resource>, content: &str) -> Vec<u8> {
    pdf_with(page, own, content, false)
}

/// [`pdf`], the font written directly into the page's `/Font` where `direct`
/// is set, so that it has no reference, object 5 then an empty dictionary.
fn pdf_with(page: Resource, own: Option<Resource>, content: &str, direct: bool) -> Vec<u8> {
    let resources = own.map_or_else(String::new, |own| {
        format!("/Resources << {} >>", own.with_fonts(""))
    });
    let font = format!(
        "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
         /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /a 6 0 R >> \
         /Encoding << /Type /Encoding /Differences [65 /a] >> /FirstChar 65 \
         /LastChar 65 /Widths [1000] {resources} >>"
    );
    let page = if direct {
        page.with_fonts(&format!("/T3 {font}"))
    } else {
        page.with_fonts("/T3 5 0 R")
    };
    let stream = |dict: String, data: &[u8]| {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    };
    let image = |sample: u8| {
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
             /BitsPerComponent 8"
                .to_string(),
            &[sample],
        )
    };
    let pattern = |grey: &str| {
        format!(
            "<< /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceGray \
             /Coords [0 0 1000 0] /Function << /FunctionType 2 /Domain [0 1] \
             /C0 [{grey}] /C1 [{grey}] /N 1 >> /Extend [true true] >> >>"
        )
        .into_bytes()
    };
    let procedure = format!("1000 0 d0 {content}");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
             /Resources << {page} >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream(String::new(), b"BT /T3 100 Tf 0 0 Td (A) Tj ET"),
        if direct {
            b"<< >>".to_vec()
        } else {
            font.into_bytes()
        },
        stream(String::new(), procedure.as_bytes()),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 72 \
          /LastChar 72 /Widths [722] /FontDescriptor 8 0 R /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 \
          /FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 \
          /CapHeight 700 /StemV 80 /FontFile2 9 0 R >>"
            .to_vec(),
        stream(format!("/Length1 {}", SERIF.len()), SERIF),
        image(0),
        image(128),
        pattern("0"),
        pattern("0.5"),
        b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
          /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /H 15 0 R >> \
          /Encoding << /Type /Encoding /Differences [72 /H] >> /FirstChar 72 \
          /LastChar 72 /Widths [1000] >>"
            .to_vec(),
        stream(String::new(), b"1000 0 0 0 1000 1000 d1"),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
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

/// The page rendered, after holding its display list's render and its
/// command stream's to it, pixel for pixel and warning for warning.
#[track_caller]
fn render(bytes: Vec<u8>, what: &str) -> Bitmap {
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let rendered = page.render(&options);
    let listed = page.display_list().render(&options);
    let stream = CommandStream::parse(&page.command_stream()).expect("the stream reads");
    let streamed = stream.render(&options);
    for (way, drawn) in [("display list", &listed), ("command stream", &streamed)] {
        assert!(
            drawn.data == rendered.data && drawn.warnings == rendered.warnings,
            "{what}: the {way} draws otherwise than the page"
        );
    }
    rendered
}

/// The red channel at the page's centre: 255 where the procedure painted
/// nothing, 0 where it painted black, between where it painted grey.
fn centre(bitmap: &Bitmap) -> u8 {
    let width = bitmap.width as usize;
    let row = bitmap.height as usize / 2;
    bitmap.data[(row * width + width / 2) * bitmap.components()]
}

/// How many pixels are darker than half.
fn dark(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks(bitmap.components())
        .filter(|pixel| pixel[0] < 128)
        .count()
}

/// **A Type 3 glyph draws what its font's own resources name**: each kind,
/// held only in the font's `/Resources`, draws — the embedded face's `H`, the
/// black image over the page, the fill at alpha 0.5, the `/Indexed` grey and
/// the black pattern — where a procedure that paints nothing leaves the page
/// white.
#[test]
fn a_type3_glyph_draws_what_its_fonts_own_resources_name() {
    let nothing = render(pdf(NONE, None, ""), "a procedure painting nothing");
    assert_eq!(
        dark(&nothing),
        0,
        "a procedure painting nothing paints nothing"
    );
    for kind in kinds() {
        let what = kind.what;
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        match what {
            "font" => assert!(dark(&own) > 100, "{what}: the H is drawn"),
            "graphics state" => assert!(
                (125..=130).contains(&centre(&own)),
                "{what}: alpha 0.5 paints half: {}",
                centre(&own)
            ),
            "colour space" => assert!(
                (126..=130).contains(&centre(&own)),
                "{what}: index 1 paints grey: {}",
                centre(&own)
            ),
            _ => assert!(
                centre(&own) < 8,
                "{what}: black is painted: {}",
                centre(&own)
            ),
        }
    }
}

/// **A Type 3 glyph whose font has no resources draws from the scope that
/// shows it**: each kind, held only in the page's resources, draws what the
/// font holding it in its own draws.
#[test]
fn a_type3_glyph_whose_font_has_none_draws_from_the_showing_scope() {
    for kind in kinds() {
        let what = kind.what;
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        let showing = render(pdf(kind.drawing, None, kind.content), what);
        assert!(
            showing.data == own.data,
            "{what}: the page's resources draw what the font's own do"
        );
    }
}

/// **A name in both resolves to the font's own**: with the page's resources
/// holding the same name for another resource, the procedure draws its
/// font's — what it draws with the page holding nothing — and not the
/// page's, which a font with no resources draws.
#[test]
fn a_name_in_both_resolves_to_the_fonts_own() {
    for kind in kinds() {
        let what = kind.what;
        let both = render(pdf(kind.other, Some(kind.drawing), kind.content), what);
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        let page = render(pdf(kind.other, None, kind.content), what);
        assert!(both.data == own.data, "{what}: the font's own draws");
        assert!(both.data != page.data, "{what}: and not the page's");
    }
}

/// **The font's resources are whole, as a form's are**: a font whose
/// `/Resources` lack the name draws what it draws with the name nowhere,
/// though the page holds it — the page's resources are where a procedure's
/// names resolve only when the font has none — and not what the name draws.
#[test]
fn a_name_the_fonts_resources_lack_resolves_nowhere() {
    for kind in kinds() {
        let what = kind.what;
        let lacking = render(pdf(kind.drawing, Some(NONE), kind.content), what);
        let nowhere = render(pdf(NONE, Some(NONE), kind.content), what);
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        assert!(
            lacking.data == nowhere.data,
            "{what}: the page's resources are not looked in"
        );
        assert!(
            lacking.data != own.data,
            "{what}: so the name draws nothing"
        );
    }
}

/// **The SVG writer and the text read the font's scope too**: each page's
/// SVG is the same markup whether the resource is the font's own or the
/// page's for a font with none, and is not the markup of a procedure that
/// paints nothing; and the `H` the procedure shows through its font's own
/// face is the page's text, where a procedure whose font lacks the face
/// shows none and leaves the glyph's own.
#[test]
fn the_svg_writer_and_the_text_read_the_fonts_scope() {
    let svg = |bytes: Vec<u8>| {
        let document = Document::open(bytes).expect("it opens");
        let page = document.page(0).expect("a page");
        page.to_svg(&SvgOptions::default()).markup
    };
    let nothing = svg(pdf(NONE, None, ""));
    for kind in kinds() {
        let what = kind.what;
        let own = svg(pdf(NONE, Some(kind.drawing), kind.content));
        let showing = svg(pdf(kind.drawing, None, kind.content));
        assert_eq!(own, showing, "{what}: one picture in either scope");
        assert_ne!(own, nothing, "{what}: the SVG draws the resource");
    }
    let text = |bytes: Vec<u8>| {
        let document = Document::open(bytes).expect("it opens");
        document.page(0).expect("a page").text().plain_text()
    };
    let font = &kinds()[0];
    let own = text(pdf(NONE, Some(font.drawing), font.content));
    assert!(own.contains('H'), "the procedure's own text: {own:?}");
    let lacking = text(pdf(font.drawing, Some(NONE), font.content));
    assert!(!lacking.contains('H'), "no face, no H: {lacking:?}");
}

/// **The command stream records the font's scope** (minor 6): a procedure
/// whose font brings resources is a `BeginType3Glyph` and its end, by the
/// font's identity — its reference here — and the scope a `Scope` record
/// under the page's, by no form name and by the same identity; a font with
/// none is neither.
#[test]
fn the_command_stream_records_the_fonts_scope() {
    let scope = CommandKind::Scope.number();
    let begin = CommandKind::BeginType3Glyph.number();
    let end = CommandKind::EndType3Glyph.number();
    let font = 5 << 16;
    for kind in kinds() {
        let what = kind.what;
        for (own, held) in [(Some(kind.drawing), true), (None, false)] {
            let page = if own.is_some() { NONE } else { kind.drawing };
            let document = Document::open(pdf(page, own, kind.content)).expect("it opens");
            let bytes = document.page(0).expect("a page").command_stream();
            let stream = CommandStream::parse(&bytes).expect("the stream reads");
            let of = |number: u32| -> Vec<&[CommandField]> {
                stream
                    .records()
                    .iter()
                    .filter(|record| record.kind == number)
                    .map(|record| record.fields.as_slice())
                    .collect()
            };
            let scopes = of(scope);
            let begun = of(begin);
            let ended = of(end);
            if held {
                assert!(
                    scopes.contains(
                        &&[
                            CommandField::Unsigned(1),
                            CommandField::Unsigned(0),
                            CommandField::Nothing,
                            CommandField::Unsigned(font),
                        ][..]
                    ),
                    "{what}: the font's scope, under the page's: {scopes:?}"
                );
                assert_eq!(
                    begun,
                    vec![&[CommandField::Unsigned(font)][..]],
                    "{what}: the bracket's begin, by the font"
                );
                assert_eq!(ended.len(), 1, "{what}: and its end");
            } else {
                assert!(
                    scopes.is_empty() && begun.is_empty() && ended.is_empty(),
                    "{what}: a font with no resources is no scope and no bracket"
                );
            }
        }
    }
}

/// **A font written directly into the page's `/Font` runs its procedures in
/// its own `/Resources` too** (Table 121, which does not tell it from a font
/// the page refers to): each kind, held only in its `/Resources`, draws what
/// the referred font draws with them; with none, it draws from the page's,
/// as before; and a name in both resolves to the font's own, not the page's.
#[test]
fn a_direct_type3_font_runs_its_procedures_in_its_own_resources() {
    let nothing = render(
        pdf_with(NONE, None, "", true),
        "a procedure painting nothing",
    );
    for kind in kinds() {
        let what = kind.what;
        let referred = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        let own = render(pdf_with(NONE, Some(kind.drawing), kind.content, true), what);
        assert!(own.data == referred.data, "{what}: its own resources draw");
        assert!(own.data != nothing.data, "{what}: and draw something");
        let showing = render(pdf_with(kind.drawing, None, kind.content, true), what);
        assert!(
            showing.data == own.data,
            "{what}: a direct font with none draws from the page's"
        );
        let both = render(
            pdf_with(kind.other, Some(kind.drawing), kind.content, true),
            what,
        );
        let page = render(pdf_with(kind.other, None, kind.content, true), what);
        assert!(
            both.data == own.data,
            "{what}: a name in both is the font's"
        );
        assert!(both.data != page.data, "{what}: and not the page's");
    }
}

/// **A direct font's scope is keyed by its name in the page's `/Font`**:
/// with no reference, its `BeginType3Glyph` carries the identity the page's
/// scope gives it — bit 62 set over its name's id there — and the stream's
/// `Scope` record for it, under the page's, carries the same.
#[test]
fn a_direct_type3_fonts_scope_is_keyed_by_its_name() {
    let scope = CommandKind::Scope.number();
    let begin = CommandKind::BeginType3Glyph.number();
    let kind = &kinds()[1];
    let document =
        Document::open(pdf_with(NONE, Some(kind.drawing), kind.content, true)).expect("it opens");
    let bytes = document.page(0).expect("a page").command_stream();
    let stream = CommandStream::parse(&bytes).expect("the stream reads");
    let begun: Vec<&CommandField> = stream
        .records()
        .iter()
        .filter(|record| record.kind == begin)
        .filter_map(|record| record.fields.first())
        .collect();
    let [CommandField::Unsigned(font)] = begun.as_slice() else {
        panic!("one bracket, by an identity: {begun:?}");
    };
    assert_eq!(
        font >> 62,
        1,
        "a name's identity, not a reference: {font:#x}"
    );
    let scopes: Vec<&[CommandField]> = stream
        .records()
        .iter()
        .filter(|record| record.kind == scope)
        .map(|record| record.fields.as_slice())
        .collect();
    assert_eq!(
        scopes,
        vec![
            &[
                CommandField::Unsigned(1),
                CommandField::Unsigned(0),
                CommandField::Nothing,
                CommandField::Unsigned(*font),
            ][..]
        ],
        "the font's scope, under the page's, by the same identity"
    );
}
