//! A soft mask's group runs in **its own `/Resources`**, as a form does.
//!
//! 11.6.5.2 makes an `/SMask`'s `/G` a transparency group XObject, so its
//! names resolve in its own resources (8.10.1), and in the scope that set
//! the `gs` where it brings none. A group run where the `gs` was draws
//! without a font, an image, a graphics state or a colour space that only
//! its own resources hold.
//!
//! Each page fills itself black under `/K`, a luminosity mask: the black is
//! kept where the group paints white and dropped where it paints nothing, so
//! what the group drew is read off the page. Every page is drawn three ways —
//! rendered, from its display list, and from its command stream — and each
//! way draws the same pixels.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | the interpreter and the renderer run the group where the `gs` was | 4 |
//! | the command stream records no mask scope | 4 |

use tinker_pdf::{Bitmap, CommandField, CommandKind, CommandStream, Document, RenderOptions};

/// Liberation Serif, the face the font kind's `H` is embedded in.
const SERIF: &[u8] =
    include_bytes!("../../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");

/// One resource kind a group can name, and the content that draws with it.
struct Kind {
    what: &'static str,
    /// The group's content, naming the resource.
    content: &'static str,
    /// The resource that draws white where the group paints: the group's
    /// own, or the page's for a group with none.
    drawing: Resource,
    /// A second resource of the same name that draws otherwise.
    other: Resource,
}

/// Resource entries: `/ExtGState` entries, which the page's must merge with
/// its own `/K`, and every other category's.
#[derive(Clone, Copy)]
struct Resource {
    states: &'static str,
    rest: &'static str,
}

const NONE: Resource = Resource {
    states: "",
    rest: "",
};

/// The four kinds, each drawing white with [`Kind::drawing`]: an embedded
/// face's `H`, where [`Kind::other`] is a Type 3 font whose `H` draws nothing; a
/// white image over the page, where the other is black; a white fill at
/// alpha 0.5, where the other's alpha is 0.25; and a white fill through an
/// `/Indexed` space, where the other's index 0 is grey.
fn kinds() -> [Kind; 4] {
    [
        Kind {
            what: "font",
            content: "1 g BT /FG 90 Tf 5 15 Td (H) Tj ET",
            drawing: Resource {
                states: "",
                rest: "/Font << /FG 7 0 R >>",
            },
            other: Resource {
                states: "",
                rest: "/Font << /FG 8 0 R >>",
            },
        },
        Kind {
            what: "image",
            content: "q 100 0 0 100 0 0 cm /IG Do Q",
            drawing: Resource {
                states: "",
                rest: "/XObject << /IG 10 0 R >>",
            },
            other: Resource {
                states: "",
                rest: "/XObject << /IG 11 0 R >>",
            },
        },
        Kind {
            what: "graphics state",
            content: "/WG gs 1 g 0 0 100 100 re f",
            drawing: Resource {
                states: "/WG << /ca 0.5 >>",
                rest: "",
            },
            other: Resource {
                states: "/WG << /ca 0.25 >>",
                rest: "",
            },
        },
        Kind {
            what: "colour space",
            content: "/CG cs 0 sc 0 0 100 100 re f",
            drawing: Resource {
                states: "",
                rest: "/ColorSpace << /CG [/Indexed /DeviceGray 1 <FF00>] >>",
            },
            other: Resource {
                states: "",
                rest: "/ColorSpace << /CG [/Indexed /DeviceGray 1 <8000>] >>",
            },
        },
    ]
}

/// A 100-point page filling itself black under `/K`, a luminosity mask
/// whose group (object 6) runs `content` with `own` as its `/Resources`, or
/// none; `page` is what the page's resources hold beside `/K`. Objects 7 to
/// 11 are what a resource names: Liberation Serif, embedded (its descriptor
/// and program 12 and 13), a Type 3 font whose `H` draws nothing (its
/// procedure 9), and a white and a black one-pixel image.
fn pdf(page: Resource, own: Option<Resource>, content: &str) -> Vec<u8> {
    let resources = |r: Resource| {
        if r.states.is_empty() {
            format!("<< {} >>", r.rest)
        } else {
            format!("<< /ExtGState << {} >> {} >>", r.states, r.rest)
        }
    };
    let group = match own {
        Some(own) => format!("/Resources {}", resources(own)),
        None => String::new(),
    };
    let fill = "/K gs 0 0 0 rg 0 0 100 100 re f";
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
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
             /Resources << /ExtGState << /K 5 0 R {} >> {} >> /Contents 4 0 R >>",
            page.states, page.rest
        )
        .into_bytes(),
        stream(String::new(), fill.as_bytes()),
        b"<< /Type /ExtGState /SMask << /Type /Mask /S /Luminosity /G 6 0 R >> >>".to_vec(),
        stream(
            format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100] \
                 /Group << /S /Transparency /CS /DeviceRGB >> {group}"
            ),
            content.as_bytes(),
        ),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 72           /LastChar 72 /Widths [722] /FontDescriptor 12 0 R /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
          /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /H 9 0 R >> \
          /Encoding << /Type /Encoding /Differences [72 /H] >> /FirstChar 72 \
          /LastChar 72 /Widths [1000] >>"
            .to_vec(),
        stream(String::new(), b"1000 0 0 0 1000 1000 d1"),
        image(255),
        image(0),
        b"<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32           /FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200           /CapHeight 700 /StemV 80 /FontFile2 13 0 R >>"
            .to_vec(),
        stream(format!("/Length1 {}", SERIF.len()), SERIF),
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

/// The red channel at the page's centre: 255 where the mask dropped the
/// black, 0 where it kept it, `255 × (1 − m)` between.
fn centre(bitmap: &Bitmap) -> u8 {
    let width = bitmap.width as usize;
    let row = bitmap.height as usize / 2;
    bitmap.data[(row * width + width / 2) * bitmap.components()]
}

/// How many pixels are darker than half: where the black was kept.
fn dark(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks(bitmap.components())
        .filter(|pixel| pixel[0] < 128)
        .count()
}

/// **A mask group draws what its own resources name**: each kind, held only
/// in the group's `/Resources`, draws — the embedded face's `H` keeps the black
/// where it paints, the white image keeps it all, the 0.5 alpha keeps half
/// and the `/Indexed` white keeps it all — where a group that paints nothing
/// keeps none.
#[test]
fn a_mask_group_draws_what_its_own_resources_name() {
    let nothing = render(pdf(NONE, None, ""), "a group painting nothing");
    assert_eq!(dark(&nothing), 0, "a group painting nothing keeps no black");
    for kind in kinds() {
        let what = kind.what;
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        match what {
            "font" => assert!(dark(&own) > 100, "{what}: the H keeps the black"),
            "graphics state" => assert!(
                (125..=130).contains(&centre(&own)),
                "{what}: alpha 0.5 keeps half: {}",
                centre(&own)
            ),
            _ => assert!(centre(&own) < 8, "{what}: white keeps all the black"),
        }
    }
}

/// **A mask group with no resources of its own draws in the scope that set
/// the `gs`**: each kind, held only in the page's resources, draws what the
/// group holding it in its own draws.
#[test]
fn a_mask_group_with_none_draws_in_the_scope_that_set_it() {
    for kind in kinds() {
        let what = kind.what;
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        let setting = render(pdf(kind.drawing, None, kind.content), what);
        assert!(
            setting.data == own.data,
            "{what}: the page's resources draw what the group's own do"
        );
    }
}

/// **A name in both resolves to the group's own**: with the page's
/// resources holding the same name for another resource, the group draws
/// its own — what it draws with the page holding nothing — and not the
/// page's, which a group with no resources draws.
#[test]
fn a_name_in_both_resolves_to_the_groups_own() {
    for kind in kinds() {
        let what = kind.what;
        let both = render(pdf(kind.other, Some(kind.drawing), kind.content), what);
        let own = render(pdf(NONE, Some(kind.drawing), kind.content), what);
        let page = render(pdf(kind.other, None, kind.content), what);
        assert!(both.data == own.data, "{what}: the group's own draws");
        assert!(both.data != page.data, "{what}: and not the page's");
    }
}

/// **The command stream records the mask's scope** (minor 5): a group with
/// resources of its own is a `Scope` record under the page's, by no form name
/// and by the group's stream, its appended field; a group with none is none.
#[test]
fn the_command_stream_records_the_masks_scope() {
    assert_eq!(CommandKind::Scope.fields()[3], "mask");
    let scope = CommandKind::Scope.number();
    for kind in kinds() {
        let what = kind.what;
        for (own, held) in [(Some(kind.drawing), true), (None, false)] {
            let page = if own.is_some() { NONE } else { kind.drawing };
            let document = Document::open(pdf(page, own, kind.content)).expect("it opens");
            let bytes = document.page(0).expect("a page").command_stream();
            let stream = CommandStream::parse(&bytes).expect("the stream reads");
            let scopes: Vec<&[CommandField]> = stream
                .records()
                .iter()
                .filter(|record| record.kind == scope)
                .map(|record| record.fields.as_slice())
                .collect();
            let want: Vec<&[CommandField]> = if held {
                vec![&[
                    CommandField::Unsigned(1),
                    CommandField::Unsigned(0),
                    CommandField::Nothing,
                    CommandField::Unsigned(6 << 16),
                ]]
            } else {
                Vec::new()
            };
            assert_eq!(scopes, want, "{what}: own resources {held}");
        }
    }
}
