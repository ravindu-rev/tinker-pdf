//! **A page written as SVG, read back through this repository's own SVG
//! reader.**
//!
//! `Page::to_svg` is held to `tinker-pdf-svg` — the reader the EPUB path draws
//! a book's pictures with — rather than to any outside program (ruling 13):
//! what the writer says, the reader must hear, for everything the two have in
//! common. And the thing heard is compared with **what the page states**,
//! worked out here from the content stream's own numbers — a rectangle's
//! corners, a glyph's control points from the font program, an image's
//! samples, a gradient's two colours — never with a second rendering of the
//! same output.
//!
//! The reader reports its scene in user units at SVG 1.1 §7.10's ninety to the
//! inch, and the writer's root is sized in points so the picture keeps the
//! page's physical size, so every coordinate comes back multiplied by the
//! reader's `pt` factor, 90/72. That
//! factor is read off the scene (`size` against the written width) and
//! checked to be exactly that, once, rather than assumed.

use tinker_pdf::{
    Bitmap, Document, DocumentBuilder, Rasterised, RenderWarning, Svg, SvgOptions, SvgWarning,
};
use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{Limits, Node, Paint, Scene};

mod render_support;
use render_support::curvy_font;

/// A one-page document of `width` x `height` points around `content`, with
/// `resources` and any objects numbered from 5.
fn pdf(content: &str, width: u32, height: u32, resources: &str, objects: &[String]) -> Vec<u8> {
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
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
fn stream(dict: &str, body: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    )
}

/// Writes page 0 of `bytes` as SVG.
fn svg_of(bytes: Vec<u8>) -> Svg {
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    page.to_svg(&SvgOptions::default())
}

/// Reads an SVG back, and the factor its coordinates come back multiplied by.
fn read_back(svg: &Svg) -> (Scene, f64) {
    let scene = tinker_pdf_svg::read(svg.markup.as_bytes(), None, &Limits::DEFAULT)
        .expect("the writer's output reads back");
    let k = scene.size.0 / svg.width;
    assert!(
        (k - 90.0 / 72.0).abs() < 1e-9,
        "SVG 1.1 §7.10: 1in = 90px, so a pt is 90/72 of a user unit: {k}"
    );
    assert!(
        (scene.size.1 / svg.height - k).abs() < 1e-9,
        "and the same on both axes"
    );
    (scene, k)
}

/// Every point of an outline, divided back into points.
fn points(segments: &[Segment], k: f64) -> Vec<Vec<[f64; 2]>> {
    let at = |p: [f64; 2]| [p[0] / k, p[1] / k];
    segments
        .iter()
        .map(|segment| match *segment {
            Segment::Move(p) | Segment::Line(p) => vec![at(p)],
            Segment::Cubic(a, b, c) => vec![at(a), at(b), at(c)],
            Segment::Close => Vec::new(),
            _ => panic!("a segment kind this test does not know"),
        })
        .collect()
}

/// The kinds of an outline's segments, as letters.
fn kinds(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| match segment {
            Segment::Move(_) => 'M',
            Segment::Line(_) => 'L',
            Segment::Cubic(..) => 'C',
            Segment::Close => 'Z',
            _ => '?',
        })
        .collect()
}

/// Asserts two point lists agree to the writer's precision: four decimal
/// places, so half of 1e-4 of rounding, and a little for the reader's
/// arithmetic.
#[track_caller]
fn close(actual: &[Vec<[f64; 2]>], expected: &[Vec<[f64; 2]>], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: segment count");
    for (index, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a.len(), e.len(), "{what}: segment {index}'s points");
        for (p, q) in a.iter().zip(e) {
            assert!(
                (p[0] - q[0]).abs() < 1e-4 && (p[1] - q[1]).abs() < 1e-4,
                "{what}: segment {index} is at {p:?}, the page says {q:?}"
            );
        }
    }
}

/// The paths in a scene, in order.
fn paths(scene: &Scene) -> Vec<&Node> {
    scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Path { .. }))
        .collect()
}

fn solid(paint: &Paint) -> [f64; 3] {
    match paint {
        Paint::Solid(colour) => colour.rgb,
        other => panic!("a flat colour, not {other:?}"),
    }
}

/// **Fills and strokes**: a rectangle, an even-odd shape at half alpha, a
/// curve, and a dashed stroke with round caps and bevel joins — each read
/// back with the corners, colour, rule, opacity and pen the page states.
#[test]
fn fills_and_strokes_read_back_as_the_page_states_them() {
    let content = "1 0 0 rg 10 20 30 40 re f\n\
                   /GS0 gs 0 0 1 rg 50 10 m 90 10 l 90 50 l 50 50 l h 60 20 m 80 20 l 80 40 l 60 40 l h f*\n\
                   /GS1 gs 0 0.4 0 rg 100 10 m 110 60 130 60 140 10 c f\n\
                   0.2 0.2 0.2 RG 3 w 1 J 2 j 5 M [4 2] 1 d 20 90 m 180 90 l 180 70 l S";
    let resources = "<< /ExtGState << /GS0 << /ca 0.5 >> /GS1 << /ca 1 >> >> >>";
    let svg = svg_of(pdf(content, 200, 100, resources, &[]));
    assert!(
        svg.warnings.is_empty(),
        "nothing refused: {:?}",
        svg.warnings
    );
    let (scene, k) = read_back(&svg);
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 4, "two fills, a curve and a stroke");

    // y runs down from the top of a 100 pt page: y' = 100 - y.
    let Node::Path {
        outline,
        fill,
        rule,
        fill_opacity,
        stroke,
        clip,
    } = nodes[0]
    else {
        unreachable!()
    };
    assert_eq!(kinds(&outline.segments), "MLLLZ");
    close(
        &points(&outline.segments, k),
        &[
            vec![[10.0, 80.0]],
            vec![[40.0, 80.0]],
            vec![[40.0, 40.0]],
            vec![[10.0, 40.0]],
            vec![],
        ],
        "the red rectangle",
    );
    assert_eq!(solid(fill), [1.0, 0.0, 0.0]);
    assert_eq!(*rule, tinker_pdf_svg::FillRule::NonZero);
    assert_eq!(*fill_opacity, 1.0);
    assert!(stroke.is_none() && clip.is_none());

    let Node::Path {
        outline,
        fill,
        rule,
        fill_opacity,
        ..
    } = nodes[1]
    else {
        unreachable!()
    };
    assert_eq!(kinds(&outline.segments), "MLLLZMLLLZ");
    assert_eq!(solid(fill), [0.0, 0.0, 1.0]);
    assert_eq!(*rule, tinker_pdf_svg::FillRule::EvenOdd);
    assert_eq!(*fill_opacity, 0.5);

    let Node::Path { outline, fill, .. } = nodes[2] else {
        unreachable!()
    };
    assert_eq!(kinds(&outline.segments), "MC");
    close(
        &points(&outline.segments, k),
        &[
            vec![[100.0, 90.0]],
            vec![[110.0, 40.0], [130.0, 40.0], [140.0, 90.0]],
        ],
        "the curve",
    );
    // 0.4 of 255 is 102 exactly, and the writer states bytes.
    assert_eq!(solid(fill), [0.0, 102.0 / 255.0, 0.0]);

    let Node::Path {
        outline,
        fill,
        stroke,
        ..
    } = nodes[3]
    else {
        unreachable!()
    };
    assert_eq!(*fill, Paint::None);
    close(
        &points(&outline.segments, k),
        &[vec![[20.0, 10.0]], vec![[180.0, 10.0]], vec![[180.0, 30.0]]],
        "the stroked polyline",
    );
    let stroke = stroke.as_ref().expect("a stroke");
    assert_eq!(solid(&stroke.paint), [51.0 / 255.0; 3]);
    assert_eq!(stroke.width, 3.0, "a user-space width, the page's own");
    assert_eq!(stroke.cap, tinker_pdf_svg::LineCap::Round);
    assert_eq!(stroke.join, tinker_pdf_svg::LineJoin::Bevel);
    assert_eq!(stroke.miter_limit, 5.0);
    assert_eq!(stroke.dashes, vec![4.0, 2.0]);
    assert_eq!(stroke.dash_offset, 1.0);
}

/// **A stroke under a transform that scales** is written with its width, its
/// dashes and its phase scaled by the transform's expansion into page space,
/// `sqrt(|det|)` — the number the renderer strokes with. The fixture above
/// strokes under the identity, where a width that was never scaled and one
/// that was are the same number.
///
/// `2 0 0 8 0 0 cm` has a determinant of 16, so an expansion of exactly 4:
/// `1 w` is 4 points, `[2 1] 0.5 d` is `[8 4]` from 2. And the renderer agrees
/// in pixels: the undashed line's column is four rows of ink at scale 1.
#[test]
fn a_stroke_under_a_scaling_transform_is_scaled_as_the_renderer_scales_it() {
    let content = "q 2 0 0 8 0 0 cm 0 0 1 RG 1 w [2 1] 0.5 d 5 1 m 50 1 l S Q\n\
                   q 2 0 0 8 0 0 cm 0 0 0 RG 1 w 5 5 m 50 5 l S Q";
    let bytes = pdf(content, 200, 100, "<< >>", &[]);
    let svg = svg_of(bytes.clone());
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    let (scene, k) = read_back(&svg);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 2, "two strokes");

    let Node::Path {
        outline, stroke, ..
    } = nodes[0]
    else {
        unreachable!()
    };
    close(
        &points(&outline.segments, k),
        &[vec![[10.0, 92.0]], vec![[100.0, 92.0]]],
        "the dashed line, its points through the CTM",
    );
    let stroke = stroke.as_ref().expect("a stroke");
    assert_eq!(stroke.width, 4.0, "1 w under an expansion of 4");
    assert_eq!(stroke.dashes, vec![8.0, 4.0], "[2 1] under the same");
    assert_eq!(stroke.dash_offset, 2.0, "and the phase");

    let Node::Path { stroke, .. } = nodes[1] else {
        unreachable!()
    };
    let width = stroke.as_ref().expect("a stroke").width;
    assert_eq!(width, 4.0);
    assert!(
        stroke.as_ref().is_some_and(|s| s.dashes.is_empty()),
        "the second line is solid"
    );

    // The renderer's own pen, counted down column 50 across the solid line:
    // page y 40 is pixel row 60, and the pen covers 58 to 62. Rows 40 to 79
    // only, because the dashed line's ink is in this column too, at row 92.
    let document = Document::open(bytes).expect("it opens");
    let bitmap = document
        .page(0)
        .expect("a page")
        .render(&tinker_pdf::RenderOptions::default());
    let components = bitmap.components();
    let inked = (40..80)
        .filter(|&y| {
            let at = y * bitmap.stride + 50 * components;
            bitmap.data.get(at).is_some_and(|&v| v < 128)
        })
        .count();
    assert_eq!(
        inked as f64, width,
        "the SVG's width is the renderer's, in rows of ink"
    );
}

/// **A clip** is a `<clipPath>` the element names, and the reader hands back
/// the clip's own rectangle beside the fill it clips.
#[test]
fn a_clip_reads_back_beside_what_it_clips() {
    let content = "q 20 30 60 40 re W n 0 0 1 rg 0 0 200 100 re f Q 1 0 0 rg 150 10 20 20 re f";
    let svg = svg_of(pdf(content, 200, 100, "<< >>", &[]));
    let (scene, k) = read_back(&svg);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 2);
    let Node::Path { clip, .. } = nodes[0] else {
        unreachable!()
    };
    let clip = clip.as_ref().expect("the first fill is clipped");
    assert_eq!(clip.rule, tinker_pdf_svg::FillRule::NonZero);
    close(
        &points(&clip.outline.segments, k),
        &[
            vec![[20.0, 70.0]],
            vec![[80.0, 70.0]],
            vec![[80.0, 30.0]],
            vec![[20.0, 30.0]],
            vec![],
        ],
        "the clip",
    );
    let Node::Path { clip, .. } = nodes[1] else {
        unreachable!()
    };
    assert!(clip.is_none(), "`Q` put the clip back");
}

/// **A clip inside a clip** is written as SVG 1.1 §14.3.5's intersection —
/// the inner `<clipPath>` names the outer with its own `clip-path` — and the
/// reader, which follows one reference per element and not that chain, hands
/// back the inner rectangle. Pinned on both halves, so a reader that learns
/// the chain fails here and the row can be told.
#[test]
fn a_clip_inside_a_clip_names_its_parent() {
    let content = "q 10 10 100 50 re W n 50 0 100 100 re W n 1 0 0 rg 0 0 200 100 re f Q";
    let svg = svg_of(pdf(content, 200, 100, "<< >>", &[]));
    assert!(
        svg.markup.contains(
            "<clipPath id=\"c2\" clipPathUnits=\"userSpaceOnUse\" clip-path=\"url(#c1)\">"
        ),
        "the inner clip names the outer: {}",
        svg.markup
    );
    assert!(
        svg.markup.contains("clip-path=\"url(#c2)\"/>"),
        "the fill names the inner"
    );
    let (scene, k) = read_back(&svg);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 1);
    let Node::Path { clip, .. } = nodes[0] else {
        unreachable!()
    };
    let clip = clip.as_ref().expect("clipped");
    close(
        &points(&clip.outline.segments, k),
        &[
            vec![[50.0, 100.0]],
            vec![[150.0, 100.0]],
            vec![[150.0, 0.0]],
            vec![[50.0, 0.0]],
            vec![],
        ],
        "the reader's one clip is the inner one",
    );
}

/// **A clipped image** sits in a `<g>` that names the clip, because a clip
/// named by the `<image>` would be read through the image's own `transform`
/// (§14.3.5) and land wherever the unit square sends it. The image's
/// placement still reads back exactly, and so — since the reader made a
/// group's clip a node of its own — does the clip, in page space.
#[test]
fn a_clipped_image_is_clipped_in_page_space() {
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /ASCIIHexDecode",
        "00ff>",
    );
    let svg = svg_of(pdf(
        "q 30 20 20 40 re W n 50 0 0 30 20 10 cm /Im0 Do Q",
        200,
        100,
        "<< /XObject << /Im0 5 0 R >> >>",
        &[image],
    ));
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    let at = svg.markup.find("<image").expect("an image");
    let element = &svg.markup[at..at + svg.markup[at..].find("/>").expect("closed")];
    assert!(
        !element.contains("clip-path"),
        "not on the image: {element}"
    );
    assert!(
        svg.markup[..at].ends_with("<g clip-path=\"url(#c1)\">"),
        "on a group around it"
    );
    let (scene, k) = read_back(&svg);
    let Some(Node::Group {
        nodes,
        clip: Some(clip),
        ..
    }) = scene.nodes.first()
    else {
        panic!("a clipped group: {:?}", scene.nodes)
    };
    // `30 20 20 40 re` on a page 100 high is x 30 to 50 and, flipped, y 40 to
    // 80 in the SVG's downward space.
    let corners: Vec<[f64; 2]> = points(&clip.outline.segments, k)
        .into_iter()
        .flatten()
        .collect();
    let low = |axis: usize| corners.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
    let high = |axis: usize| corners.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
    close(
        &[vec![[low(0), low(1)]], vec![[high(0), high(1)]]],
        &[vec![[30.0, 40.0]], vec![[50.0, 80.0]]],
        "the clip, in page space",
    );
    let Some(Node::Image { matrix, .. }) = nodes.first() else {
        panic!("an image inside it: {nodes:?}")
    };
    let place = |x: f64, y: f64| {
        [
            (matrix[0] * x + matrix[2] * y + matrix[4]) / k,
            (matrix[1] * x + matrix[3] * y + matrix[5]) / k,
        ]
    };
    close(
        &[vec![place(0.0, 0.0)], vec![place(1.0, 1.0)]],
        &[vec![[20.0, 60.0]], vec![[70.0, 90.0]]],
        "the image's corners",
    );
}

/// **An image this build cannot decode** is the renderer's grey placeholder
/// over its unit square, and the renderer's own warning names the codec.
#[test]
fn an_undecodable_image_is_the_renderer_s_placeholder() {
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /JPXDecode",
        "\u{0}\u{0}\u{0}\u{0}",
    );
    let svg = svg_of(pdf(
        "q 40 0 0 20 10 30 cm /Im0 Do Q",
        200,
        100,
        "<< /XObject << /Im0 5 0 R >> >>",
        &[image],
    ));
    assert!(
        svg.warnings.iter().any(|w| matches!(
            w,
            SvgWarning::Render(RenderWarning::UnsupportedImage { .. })
        )),
        "{:?}",
        svg.warnings
    );
    let (scene, k) = read_back(&svg);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 1, "the placeholder");
    let Node::Path { outline, fill, .. } = nodes[0] else {
        unreachable!()
    };
    assert_eq!(solid(fill), [191.0 / 255.0; 3], "0xBF, the renderer's grey");
    close(
        &points(&outline.segments, k),
        &[
            vec![[10.0, 70.0]],
            vec![[50.0, 70.0]],
            vec![[50.0, 50.0]],
            vec![[10.0, 50.0]],
            vec![],
        ],
        "the unit square, placed",
    );
}

/// **Text is written as the glyphs' outlines**: every control point of every
/// glyph, read back, is where the font program and the text matrix put it —
/// computed here from `glyf` with `tinker-pdf-font`, the em scale, the font
/// size and each glyph's advance, with a quadratic raised to its one cubic.
#[test]
fn text_is_written_as_the_glyphs_outlines() {
    let face = curvy_font();
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &face));
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 20.0, 10.0, 30.0, "ab c")
    });
    let svg = svg_of(builder.finish());
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    let (scene, k) = read_back(&svg);

    let sfnt = tinker_pdf_font::Sfnt::parse(&face).expect("the face parses");
    let em = f64::from(sfnt.units_per_em);
    let mut pen = 10.0;
    let mut expected = Vec::new();
    for c in "ab c".chars() {
        let glyph = sfnt.glyph_for_char(c).expect("mapped");
        let advance = f64::from(sfnt.advance(glyph).expect("an advance")) / em * 20.0;
        let at = |x: f64, y: f64| [pen + x / em * 20.0, 100.0 - (30.0 + y / em * 20.0)];
        if let Some(outline) = tinker_pdf_font::glyf::outline(&sfnt, glyph) {
            if !outline.is_empty() {
                let mut out = Vec::new();
                let (mut last, mut start) = ((0.0, 0.0), (0.0, 0.0));
                for segment in &outline.segments {
                    use tinker_pdf_font::Segment as F;
                    match *segment {
                        F::MoveTo { x, y } => {
                            (last, start) = ((x, y), (x, y));
                            out.push(vec![at(x, y)]);
                        }
                        F::LineTo { x, y } => {
                            last = (x, y);
                            out.push(vec![at(x, y)]);
                        }
                        F::QuadTo { cx, cy, x, y } => {
                            let c1 = (
                                last.0 + (cx - last.0) * 2.0 / 3.0,
                                last.1 + (cy - last.1) * 2.0 / 3.0,
                            );
                            let c2 = (x + (cx - x) * 2.0 / 3.0, y + (cy - y) * 2.0 / 3.0);
                            last = (x, y);
                            out.push(vec![at(c1.0, c1.1), at(c2.0, c2.1), at(x, y)]);
                        }
                        F::CurveTo {
                            c1x,
                            c1y,
                            c2x,
                            c2y,
                            x,
                            y,
                        } => {
                            last = (x, y);
                            out.push(vec![at(c1x, c1y), at(c2x, c2y), at(x, y)]);
                        }
                        F::Close => {
                            last = start;
                            out.push(Vec::new());
                        }
                    }
                }
                expected.push(out);
            }
        }
        pen += advance;
    }
    assert_eq!(expected.len(), 3, "three glyphs draw; the space does not");

    let nodes = paths(&scene);
    assert_eq!(nodes.len(), expected.len(), "one path per glyph");
    for (index, (node, want)) in nodes.iter().zip(&expected).enumerate() {
        let Node::Path { outline, fill, .. } = node else {
            unreachable!()
        };
        assert_eq!(solid(fill), [0.0, 0.0, 0.0]);
        close(
            &points(&outline.segments, k),
            want,
            &format!("glyph {index}"),
        );
    }
}

/// Minimal base64 (RFC 4648 §4) for reading a `data:` URI back.
fn unbase64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
        }
    };
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let n = chunk
            .iter()
            .take(4 - pad)
            .fold(0u32, |n, &c| (n << 6) | value(c))
            << (6 * pad);
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    out
}

/// The picture a scene's image node carries.
fn picture(href: &str) -> Bitmap {
    let data = href
        .strip_prefix("data:image/png;base64,")
        .expect("a PNG data URI");
    Bitmap::from_png(&unbase64(data)).expect("the PNG decodes")
}

/// **An image** is its samples, losslessly: the PNG in the `data:` URI
/// decodes to exactly the RGB the page's image holds, and its unit square
/// lands where the image's `cm` puts it.
#[test]
fn an_image_is_its_own_samples_where_the_page_places_it() {
    let (w, h) = (5u32, 3u32);
    let samples: Vec<u8> = (0..w * h * 3).map(|i| (i * 37 % 251) as u8).collect();
    let hex: String = samples
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        + ">";
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode"
        ),
        &hex,
    );
    let svg = svg_of(pdf(
        "q 50 0 0 30 20 10 cm /Im0 Do Q",
        200,
        100,
        "<< /XObject << /Im0 5 0 R >> >>",
        &[image],
    ));
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    let (scene, k) = read_back(&svg);
    let Some(Node::Image {
        href, rect, matrix, ..
    }) = scene.nodes.first()
    else {
        panic!("an image: {:?}", scene.nodes)
    };
    let bitmap = picture(href);
    assert_eq!((bitmap.width, bitmap.height), (w, h));
    let rgb: Vec<u8> = bitmap
        .data
        .chunks_exact(4)
        .flat_map(|p| p[..3].to_vec())
        .collect();
    assert_eq!(rgb, samples, "the samples, exactly");
    assert!(bitmap.data.chunks_exact(4).all(|p| p[3] == 255));
    assert_eq!(*rect, [0.0, 0.0, 1.0, 1.0]);
    // The unit square's top edge (SVG y = 0) is PDF's y = 1: (20, 40) on the
    // page, which is y' = 60; its bottom is y' = 90.
    let place = |x: f64, y: f64| {
        [
            (matrix[0] * x + matrix[2] * y + matrix[4]) / k,
            (matrix[1] * x + matrix[3] * y + matrix[5]) / k,
        ]
    };
    close(
        &[vec![place(0.0, 0.0)], vec![place(1.0, 1.0)]],
        &[vec![[20.0, 60.0]], vec![[70.0, 90.0]]],
        "the image's corners",
    );
}

/// **An exact shading is a gradient**: an axial RGB ramp with `N` 1 and both
/// ends extended reads back as a linear gradient with the ramp's two colours
/// at 0 and 1 and its axis where the CTM puts it; a radial one whose first
/// circle is a point inside the second, as a radial gradient.
#[test]
fn an_exact_shading_reads_back_as_a_gradient() {
    let axial = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 0 110 0] \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
                 /Extend [true true] >>";
    let radial = "<< /ShadingType 3 /ColorSpace /DeviceGray /Coords [5 5 0 0 0 40] \
                  /Function << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >> \
                  /Extend [true true] >>";
    let svg = svg_of(pdf(
        "q 0 0 100 50 re W n /A sh Q q 1 0 0 1 150 50 cm /R sh Q",
        200,
        100,
        "<< /Shading << /A 5 0 R /R 6 0 R >> >>",
        &[axial.to_string(), radial.to_string()],
    ));
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    let (scene, k) = read_back(&svg);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 2);

    let Node::Path { fill, clip, .. } = nodes[0] else {
        unreachable!()
    };
    assert!(clip.is_some(), "`sh` paints the clip");
    let Paint::Linear {
        from,
        to,
        matrix,
        stops,
    } = fill
    else {
        panic!("a linear gradient, not {fill:?}")
    };
    let map = |p: [f64; 2]| {
        [
            (matrix[0] * p[0] + matrix[2] * p[1] + matrix[4]) / k,
            (matrix[1] * p[0] + matrix[3] * p[1] + matrix[5]) / k,
        ]
    };
    close(
        &[vec![map(*from)], vec![map(*to)]],
        &[vec![[10.0, 100.0]], vec![[110.0, 100.0]]],
        "the axis",
    );
    let colours: Vec<(f64, [f64; 3])> = stops.iter().map(|s| (s.offset, s.colour.rgb)).collect();
    assert_eq!(
        colours,
        vec![(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])]
    );

    let Node::Path { fill, .. } = nodes[1] else {
        unreachable!()
    };
    let Paint::Radial {
        centre,
        radius,
        focus,
        matrix,
        stops,
    } = fill
    else {
        panic!("a radial gradient, not {fill:?}")
    };
    let map = |p: [f64; 2]| {
        [
            (matrix[0] * p[0] + matrix[2] * p[1] + matrix[4]) / k,
            (matrix[1] * p[0] + matrix[3] * p[1] + matrix[5]) / k,
        ]
    };
    close(
        &[vec![map(*centre)], vec![map(*focus)]],
        &[vec![[150.0, 50.0]], vec![[155.0, 45.0]]],
        "the circle's centre and the focus",
    );
    assert_eq!(*radius, 40.0);
    let colours: Vec<(f64, [f64; 3])> = stops.iter().map(|s| (s.offset, s.colour.rgb)).collect();
    assert_eq!(
        colours,
        vec![(0.0, [1.0, 1.0, 1.0]), (1.0, [0.0, 0.0, 0.0])]
    );
}

/// **What SVG cannot say is named, never dropped, and never written as an
/// element the reader refuses**: a soft mask, a blend mode, a knockout group,
/// a mesh and a tiling pattern each produce their warning, the two that are
/// drawable are drawn as embedded pictures, and the markup holds no
/// `<mask>`, `<pattern>` or `<filter>` — so the reader reports none of its
/// own refusals.
#[test]
fn what_svg_cannot_say_is_named() {
    let mesh = stream(
        "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 /BitsPerComponent 8 \
         /BitsPerFlag 8 /Decode [0 200 0 100 0 1 0 1 0 1] /Filter /ASCIIHexDecode",
        "000a0aff0000 00640aff00ff 00326400ff00>",
    );
    let cell = stream(
        "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 \
         /Resources << >>",
        "0 0 1 rg 0 0 5 5 re f",
    );
    let mask_group = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 100] /Group << /S /Transparency /CS /DeviceGray >>",
        "0.5 g 0 0 200 100 re f",
    );
    let knockout = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 100] /Group << /S /Transparency /K true >>",
        "0 1 0 rg 1 1 5 5 re f",
    );
    let content = "/M sh\n\
                   /Pattern cs /P scn 120 10 40 40 re f\n\
                   q /Soft gs 1 0 0 rg 10 60 30 30 re f Q\n\
                   q /Mul gs 0 0 1 rg 50 60 30 30 re f Q\n\
                   /K Do";
    let resources = "<< /Shading << /M 5 0 R >> /Pattern << /P 6 0 R >> \
         /XObject << /K 8 0 R >> \
         /ExtGState << /Soft << /SMask << /S /Luminosity /G 7 0 R >> >> /Mul << /BM /Multiply >> >> >>";
    let svg = svg_of(pdf(
        content,
        200,
        100,
        resources,
        &[mesh, cell, mask_group, knockout],
    ));
    // Each names what it touched (ruling 10): the shading and the pattern by
    // resource name, the mask by its `/G` group's reference — object 7 — and
    // the knockout group by the form's resource name. The blend mode names
    // the mode, because a device is handed the state a `gs` made and never
    // the `gs`.
    for expected in [
        SvgWarning::Rasterised {
            what: Rasterised::Shading,
            name: "M".to_string(),
        },
        SvgWarning::Rasterised {
            what: Rasterised::TilingPattern,
            name: "P".to_string(),
        },
        SvgWarning::SoftMaskRefused {
            group: Some(tinker_pdf::ObjRef { num: 7, gen: 0 }),
        },
        SvgWarning::BlendModeRefused { mode: "Multiply" },
        SvgWarning::KnockoutRefused {
            form: "K".to_string(),
        },
    ] {
        assert!(
            svg.warnings.contains(&expected),
            "{expected:?} is named: {:?}",
            svg.warnings
        );
    }
    for element in ["<mask", "<pattern", "<filter", "<feBlend"] {
        assert!(!svg.markup.contains(element), "no {element} is written");
    }
    let (scene, _) = read_back(&svg);
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let images: Vec<&Node> = scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Image { .. }))
        .collect();
    assert_eq!(
        images.len(),
        2,
        "the mesh and the tiling pattern, as pixels"
    );
    // The mesh was drawn, not left transparent: its picture has ink.
    let Node::Image { href, .. } = images[0] else {
        unreachable!()
    };
    let mesh = picture(href);
    assert!(
        mesh.data.chunks_exact(4).filter(|p| p[3] == 255).count() > 100,
        "the rasterised mesh is opaque where it paints"
    );
}

/// A page's SVG is the same bytes every time, and the same whether the page
/// is written directly or from a display list recorded earlier.
#[test]
fn the_output_is_deterministic_and_the_list_writes_the_same() {
    let content = "1 0 0 rg 10 20 30 40 re f 0.3 w 0 0 1 RG 5 5 m 150 95 l S";
    let document = Document::open(pdf(content, 200, 100, "<< >>", &[])).expect("opens");
    let page = document.page(0).expect("a page");
    let direct = page.to_svg(&SvgOptions::default());
    assert_eq!(direct, page.to_svg(&SvgOptions::default()));
    assert_eq!(direct, page.display_list().to_svg(&SvgOptions::default()));
    assert!(direct
        .markup
        .starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg "));
    assert!(direct
        .markup
        .contains("width=\"200pt\" height=\"100pt\" viewBox=\"0 0 200 100\""));
}

/// **A point that is not finite is dropped, as the renderer drops it** — the
/// rasterizer's path builder discards a verb with a non-finite point and
/// keeps the rest — rather than written as `0`, which made it a spurious
/// corner at the origin. So a clip whose every point overflows installs no
/// clip, as on a render, instead of a clip of no area that hides everything;
/// and a fill with one overflowing vertex is the fill without it.
#[test]
fn a_point_that_is_not_finite_is_dropped_as_the_renderer_drops_it() {
    let content = "q 10 0 0 10 0 0 cm 1e308 1e308 m 1e308 0 l 0 1e308 l W n \
                   1 0 0 rg 0 0 5 5 re f Q\n\
                   q 10 0 0 10 0 0 cm 0 0 1 rg 6 1 m 1e308 1 l 8 1 l 8 3 l h f Q";
    let bytes = pdf(content, 100, 100, "<< >>", &[]);
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&tinker_pdf::RenderOptions::default());
    let red = bitmap
        .data
        .chunks_exact(3)
        .filter(|p| *p == [255, 0, 0])
        .count();
    assert_eq!(
        red, 2_500,
        "the renderer installs no clip: the square paints"
    );

    let svg = page.to_svg(&SvgOptions::default());
    assert!(
        !svg.markup.contains("<clipPath"),
        "no clip, as on the render: {}",
        svg.markup
    );
    let (scene, k) = read_back(&svg);
    let nodes = paths(&scene);
    assert_eq!(nodes.len(), 2);
    let Node::Path { clip, fill, .. } = nodes[0] else {
        unreachable!()
    };
    assert!(clip.is_none(), "the red square is unclipped");
    assert_eq!(solid(fill), [1.0, 0.0, 0.0]);
    let Node::Path { outline, .. } = nodes[1] else {
        unreachable!()
    };
    // (60, 10), then the overflowing vertex dropped, then (80, 10), (80, 30)
    // and back: y' = 100 - y.
    assert_eq!(kinds(&outline.segments), "MLLZ");
    close(
        &points(&outline.segments, k),
        &[
            vec![[60.0, 90.0]],
            vec![[80.0, 90.0]],
            vec![[80.0, 70.0]],
            vec![],
        ],
        "the blue triangle, without a corner at the origin",
    );
}

/// Deterministic noise, `count` bytes of it, as the hex an `ASCIIHexDecode`
/// stream carries — samples no PNG filter can predict, so a picture of them
/// is as large as its pixels.
fn noise_hex(count: usize, seed: u64) -> String {
    let mut state = seed | 1;
    let mut out = String::with_capacity(count * 2 + 1);
    for _ in 0..count {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push_str(&format!("{:02x}", state as u8));
    }
    out.push('>');
    out
}

/// An RGB image XObject of `side` x `side` noisy samples.
fn noisy_image(side: u32, seed: u64) -> String {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {side} /Height {side} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode"
        ),
        &noise_hex((side * side * 3) as usize, seed),
    )
}

/// **A picture drawn again is written once.** The review's case, scaled down:
/// one 64 x 64 image of noise drawn four hundred times wrote four hundred
/// copies of its PNG, so the markup grew with the operator count times the
/// image. Now it is one `<image>` in `<defs>` and four hundred `<use>`s, each
/// read back where its `cm` puts it with the image's own samples.
#[test]
fn a_picture_drawn_again_is_written_once() {
    let mut content = String::new();
    for index in 0..400u32 {
        let (x, y) = (index % 20 * 10, index / 20 * 5);
        content.push_str(&format!("q 8 0 0 4 {x} {y} cm /Im0 Do Q\n"));
    }
    let svg = svg_of(pdf(
        &content,
        200,
        100,
        "<< /XObject << /Im0 5 0 R >> >>",
        &[noisy_image(64, 7)],
    ));
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    assert_eq!(
        svg.markup.matches("data:image/png").count(),
        1,
        "one copy of the picture"
    );
    assert_eq!(svg.markup.matches("<use ").count(), 400, "one use per draw");
    // One 64 x 64 RGBA PNG of noise is 16 640 bytes before base64, so four
    // hundred of them were 8.9 MB; one and four hundred references are not
    // a tenth of one of those megabytes.
    assert!(
        svg.markup.len() < 100_000,
        "{} bytes of markup",
        svg.markup.len()
    );

    let (scene, k) = read_back(&svg);
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let images: Vec<&Node> = scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Image { .. }))
        .collect();
    assert_eq!(images.len(), 400, "every draw reads back as the image");
    for (index, node) in images.iter().enumerate() {
        let Node::Image { matrix, href, .. } = node else {
            unreachable!()
        };
        let (x, y) = ((index % 20 * 10) as f64, (index / 20 * 5) as f64);
        let place = |u: f64, v: f64| {
            [
                (matrix[0] * u + matrix[2] * v + matrix[4]) / k,
                (matrix[1] * u + matrix[3] * v + matrix[5]) / k,
            ]
        };
        // The unit square's top edge is PDF's y + 4, which is y' = 96 - y.
        close(
            &[vec![place(0.0, 0.0)], vec![place(1.0, 1.0)]],
            &[vec![[x, 96.0 - y]], vec![[x + 8.0, 100.0 - y]]],
            &format!("draw {index}'s corners"),
        );
        if index == 0 {
            let bitmap = picture(href);
            assert_eq!((bitmap.width, bitmap.height), (64, 64));
            let hex = noise_hex(64 * 64 * 3, 7);
            let rgb: String = bitmap
                .data
                .chunks_exact(4)
                .flat_map(|p| p[..3].to_vec())
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(rgb + ">", hex, "the samples, exactly");
        }
    }
}

/// **Small pictures are written in place, and so is any picture once the
/// reader's budget of references is spent**, so a file this writes is never
/// one `tinker-pdf-svg` refuses for its `<use>`s: a 2 x 2 image drawn three
/// times is three `<image>`s, and a 32 x 32 one drawn 4 100 times is 4 096
/// references and four copies.
#[test]
fn references_stop_where_the_reader_s_budget_does() {
    let tiny = stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /ASCIIHexDecode",
        "00ff00ff>",
    );
    let svg = svg_of(pdf(
        "q 5 0 0 5 0 0 cm /T Do Q q 5 0 0 5 10 0 cm /T Do Q q 5 0 0 5 20 0 cm /T Do Q",
        100,
        100,
        "<< /XObject << /T 5 0 R >> >>",
        &[tiny],
    ));
    assert_eq!(svg.markup.matches("data:image/png").count(), 3);
    assert_eq!(svg.markup.matches("<use ").count(), 0);

    let budget = tinker_pdf_svg::Limits::DEFAULT.max_uses;
    let mut content = String::new();
    for index in 0..budget + 4 {
        content.push_str(&format!("q 1 0 0 1 {} 0 cm /N Do Q\n", index % 90));
    }
    let svg = svg_of(pdf(
        &content,
        100,
        100,
        "<< /XObject << /N 5 0 R >> >>",
        &[noisy_image(32, 11)],
    ));
    assert_eq!(svg.markup.matches("<use ").count(), budget);
    assert_eq!(
        svg.markup.matches("data:image/png").count(),
        1 + 4,
        "the definition, and the four draws past the budget in place"
    );
    let (scene, _) = read_back(&svg);
    assert_eq!(
        scene
            .nodes
            .iter()
            .filter(|node| matches!(node, Node::Image { .. }))
            .count(),
        budget + 4,
        "and the reader takes every one"
    );
}

/// **A rasterised paint drawn again is written once**: a radial shading no
/// gradient can state (`/Extend [false false]`), painted by `sh` three times
/// under the same clip, is one raster and three references to it.
#[test]
fn a_raster_drawn_again_is_written_once() {
    let radial = "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [50 50 0 50 50 50] \
                  /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
                  /Extend [false false] >>";
    let svg = svg_of(pdf(
        "/S sh /S sh /S sh",
        100,
        100,
        "<< /Shading << /S 5 0 R >> >>",
        &[radial.to_string()],
    ));
    assert_eq!(
        svg.warnings,
        vec![SvgWarning::Rasterised {
            what: Rasterised::Shading,
            name: "S".to_string(),
        }]
    );
    assert_eq!(svg.markup.matches("data:image/png").count(), 1);
    assert_eq!(svg.markup.matches("<use ").count(), 3);
    let (scene, k) = read_back(&svg);
    let images: Vec<&Node> = scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Image { .. }))
        .collect();
    assert_eq!(images.len(), 3);
    // Each covers the page: the raster's rectangle is the clip's, which is
    // the page's.
    for node in images {
        let Node::Image { matrix, .. } = node else {
            unreachable!()
        };
        let far = [
            (matrix[0] + matrix[2] + matrix[4]) / k,
            (matrix[1] + matrix[3] + matrix[5]) / k,
        ];
        close(
            &[vec![[matrix[4] / k, matrix[5] / k]], vec![far]],
            &[vec![[0.0, 0.0]], vec![[100.0, 100.0]]],
            "the raster's corners",
        );
    }
}

/// **The markup has a budget, and a page past it is cut short and says so.**
/// Three thousand filled squares under a budget of 4 KiB: what fits is
/// written, the rest is not, the warning names the budget, and the document
/// is still one the reader takes whole. The replay stops where the budget
/// did — the image name the page ends on, which names nothing, is never
/// reached. The default budget is the cap, and a caller can lower it but not
/// raise it.
#[test]
fn markup_past_its_budget_is_cut_short_and_says_so() {
    assert_eq!(SvgOptions::default().max_bytes, tinker_pdf::MAX_SVG_BYTES);
    let mut content = String::new();
    for index in 0..3_000u32 {
        content.push_str(&format!(
            "0 0 1 rg {} {} 1 1 re f\n",
            index % 100,
            index / 100
        ));
    }
    content.push_str("/Nope Do\n");
    let document = Document::open(pdf(&content, 100, 100, "<< >>", &[])).expect("it opens");
    let page = document.page(0).expect("a page");
    let whole = page.to_svg(&SvgOptions::default());
    assert_eq!(
        whole.warnings,
        vec![SvgWarning::Render(RenderWarning::UnsupportedImage {
            codec: "Nope".to_string()
        })],
        "the whole page reaches its last operator"
    );
    assert_eq!(whole.markup.matches("<path ").count(), 3_000 + 1);

    let mut options = SvgOptions::default();
    options.max_bytes = 4_096;
    let cut = page.to_svg(&options);
    assert_eq!(
        cut.warnings,
        vec![SvgWarning::Truncated { limit: 4_096 }],
        "named once, with the budget, and nothing after it was replayed"
    );
    let drawn = cut.markup.matches("<path ").count();
    assert!(
        drawn > 10 && drawn < 3_000,
        "what fitted was written: {drawn} squares"
    );
    // Not a byte short of the budget's worth either: one more square would
    // not have fitted. Every square here is one `<path>` of the same length
    // give or take a digit, so the cut is within one of them of the budget.
    assert!(
        cut.markup.len() + 80 > 4_096,
        "the budget was spent, not abandoned early: {} bytes",
        cut.markup.len()
    );
    // The budget counts the elements; the root, `<defs>`' tags and the
    // closing tags are the few hundred bytes outside it.
    assert!(
        cut.markup.len() <= 4_096 + 400,
        "{} bytes",
        cut.markup.len()
    );
    assert!(
        whole
            .markup
            .starts_with(&cut.markup[..cut.markup.len() - "</svg>\n".len()]),
        "and what was written is the start of the whole document"
    );
    let (scene, _) = read_back(&cut);
    assert_eq!(paths(&scene).len(), drawn);

    // A budget above the cap is read as the cap.
    let mut options = SvgOptions::default();
    options.max_bytes = usize::MAX;
    assert_eq!(page.to_svg(&options), whole);
}

/// **The writer never panics on a hostile page.** Deterministically mutated
/// versions of every page above — bytes flipped, runs deleted, numbers
/// replaced with enormous and non-finite ones — written as SVG; each result
/// must be a document the reader accepts or the writer's own output, and no
/// call may panic (ruling 1). The `render_page` fuzz target writes every page
/// it reaches as SVG too, so the same property runs there without a seed
/// budget.
#[test]
fn a_hostile_page_never_panics_the_writer() {
    let pages: Vec<Vec<u8>> = vec![
        pdf(
            "1 0 0 rg 10 20 30 40 re f q 5 5 50 50 re W n 0 0 1 RG 2 w [3 1] 0 d 0 0 m 99 99 l S Q",
            200,
            100,
            "<< >>",
            &[],
        ),
        pdf(
            "q 1e300 0 0 1e300 0 0 cm /A sh Q /Pattern cs /P scn 0 0 10 10 re f",
            50,
            50,
            "<< /Shading << /A 5 0 R >> /Pattern << /P 6 0 R >> >>",
            &[
                "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 0 0] \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>"
                    .to_string(),
                stream(
                    "/PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 0 0] /XStep 0 /YStep 0",
                    "0 0 1 1 re f",
                ),
            ],
        ),
    ];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut written = 0usize;
    for page in &pages {
        for _ in 0..60 {
            let mut bytes = page.clone();
            for _ in 0..1 + next() % 6 {
                let at = (next() as usize) % bytes.len().max(1);
                match next() % 4 {
                    0 => bytes[at] ^= 1 << (next() % 8),
                    1 => {
                        let end = (at + 1 + next() as usize % 16).min(bytes.len());
                        bytes.drain(at..end);
                    }
                    2 => bytes
                        .splice(at..at, b" 1e308 ".iter().copied())
                        .for_each(drop),
                    _ => bytes
                        .splice(at..at, b" -0 NaN ".iter().copied())
                        .for_each(drop),
                }
                if bytes.is_empty() {
                    bytes.push(b' ');
                }
            }
            let Ok(document) = Document::open(bytes) else {
                continue;
            };
            let Some(page) = document.page(0) else {
                continue;
            };
            let svg = page.to_svg(&SvgOptions::default());
            written += 1;
            // Whatever the page said, the output is a document the reader can
            // take or refuse by name — never one that trips it.
            let _ = tinker_pdf_svg::read(svg.markup.as_bytes(), None, &Limits::DEFAULT);
        }
    }
    assert!(
        written > 30,
        "the campaign reached the writer {written} times"
    );
}
