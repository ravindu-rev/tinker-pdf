//! The analysis device held to its own definitions: the box of the marks
//! recomputed from a display list's events by a walk of this file's own, over
//! pages that draw every family of operator; and the geometry it is built
//! from, each against a box worked out by hand.

use tinker_pdf_content::{Event, PathSegment, TextRenderMode};
use tinker_pdf_font::Segment;
use tinker_pdf_render::GlyphSource;

use super::*;
use crate::Document;

// ---- the geometry ---------------------------------------------------------------

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

/// `(0, 0)` to `(10, 0)` through control points ten up: `y = 30 t (1 - t)`,
/// whose top is 7.5 at `t = 0.5`, short of the control points' 10.
#[test]
fn a_curves_box_is_its_extremes_not_its_control_points() {
    let path = [
        PathSegment::MoveTo { x: 0.0, y: 0.0 },
        PathSegment::CurveTo {
            x1: 0.0,
            y1: 10.0,
            x2: 10.0,
            y2: 10.0,
            x3: 10.0,
            y3: 0.0,
        },
    ];
    assert_eq!(path_box(&path), Some(rect(0.0, 0.0, 10.0, 7.5)));
    // An S whose two turns are both inside: `x = 3t(1-t)^2*4 - 3t^2(1-t)*4`
    // runs from 0 out to `±4 sqrt(3) / 9` and back.
    let s = [
        PathSegment::MoveTo { x: 0.0, y: 0.0 },
        PathSegment::CurveTo {
            x1: 4.0,
            y1: 1.0,
            x2: -4.0,
            y2: 2.0,
            x3: 0.0,
            y3: 3.0,
        },
    ];
    let b = path_box(&s).expect("a box");
    let reach = 4.0 * 3f64.sqrt() / 9.0;
    assert!(
        (b.x1 - reach).abs() < 1e-12 && (b.x0 + reach).abs() < 1e-12,
        "{b:?}"
    );
    assert_eq!((b.y0, b.y1), (0.0, 3.0));
    // A straight path's box is its points', and a closed one returns to its
    // start without a point of its own.
    let square = [
        PathSegment::MoveTo { x: 1.0, y: 2.0 },
        PathSegment::LineTo { x: 5.0, y: 2.0 },
        PathSegment::LineTo { x: 5.0, y: 9.0 },
        PathSegment::Close,
    ];
    assert_eq!(path_box(&square), Some(rect(1.0, 2.0, 5.0, 9.0)));
    assert_eq!(path_box(&[]), None);
    // A non-finite point is left out rather than poisoning the box.
    let poisoned = [
        PathSegment::MoveTo { x: 1.0, y: 1.0 },
        PathSegment::LineTo {
            x: f64::NAN,
            y: 4.0,
        },
        PathSegment::LineTo { x: 3.0, y: 2.0 },
    ];
    assert_eq!(path_box(&poisoned), Some(rect(1.0, 1.0, 3.0, 2.0)));
}

/// A quadratic from `(0, 0)` through `(5, 10)` to `(10, 0)` tops out at 5.
#[test]
fn a_glyphs_box_is_its_outlines_extremes_through_its_transform() {
    let outline = Outline {
        segments: vec![
            Segment::MoveTo { x: 0.0, y: 0.0 },
            Segment::QuadTo {
                cx: 5.0,
                cy: 10.0,
                x: 10.0,
                y: 0.0,
            },
            Segment::Close,
        ],
    };
    assert_eq!(
        outline_box(&outline, &Matrix::IDENTITY),
        Some(rect(0.0, 0.0, 10.0, 5.0))
    );
    // Scaled by 2 and moved to (100, 50).
    let map = Matrix {
        a: 2.0,
        b: 0.0,
        c: 0.0,
        d: 2.0,
        e: 100.0,
        f: 50.0,
    };
    assert_eq!(
        outline_box(&outline, &map),
        Some(rect(100.0, 50.0, 120.0, 60.0))
    );
}

#[test]
fn areas_meet_as_clips_do() {
    let a = Area::Within(rect(0.0, 0.0, 10.0, 10.0));
    let b = Area::Within(rect(5.0, 5.0, 20.0, 20.0));
    assert_eq!(a.meet(b), Area::Within(rect(5.0, 5.0, 10.0, 10.0)));
    assert_eq!(a.meet(Area::Everywhere), a);
    assert_eq!(Area::Everywhere.meet(Area::Nowhere), Area::Nowhere);
    assert_eq!(
        a.meet(Area::Within(rect(20.0, 20.0, 30.0, 30.0))),
        Area::Nowhere
    );
    // A clip of no area clips everything away; a box with area is itself.
    assert_eq!(
        Area::of_clip(Some(rect(0.0, 0.0, 10.0, 0.0))),
        Area::Nowhere
    );
    assert_eq!(Area::of_clip(None), Area::Nowhere);
    assert_eq!(
        Area::of_clip(Some(rect(0.0, 0.0, 1.0, 1.0))),
        Area::Within(rect(0.0, 0.0, 1.0, 1.0))
    );
    // A line meets a box it lies on as a line, which a hairline marks.
    assert_eq!(
        a.clip(rect(2.0, 4.0, 8.0, 4.0)),
        Some(rect(2.0, 4.0, 8.0, 4.0))
    );
    assert_eq!(Area::Nowhere.clip(rect(0.0, 0.0, 1.0, 1.0)), None);
}

/// A state with `width` and `cap`, and the identity or a doubling transform.
fn pen(width: f64, cap: ContentCap, doubled: bool) -> GraphicsState {
    GraphicsState {
        line_width: width,
        line_cap: cap,
        miter_limit: 10.0,
        ctm: if doubled {
            Matrix::scale(2.0, 2.0)
        } else {
            Matrix::IDENTITY
        },
        ..GraphicsState::default()
    }
}

#[test]
fn a_strokes_box_is_its_outline_under_its_pen() {
    let line = [
        PathSegment::MoveTo { x: 20.0, y: 50.0 },
        PathSegment::LineTo { x: 120.0, y: 50.0 },
    ];
    let mut scratch = FillScratch::new();
    let boxed = |scratch: &mut FillScratch, state: &GraphicsState| {
        stroke_box(
            scratch,
            &raster_path(&line),
            path_box(&line),
            stroke_style(state),
            state,
        )
    };
    let near = |got: Option<Rect>, want: Rect, what: &str| {
        let got = got.unwrap_or_else(|| panic!("{what}: no box"));
        for (g, w) in [
            (got.x0, want.x0),
            (got.y0, want.y0),
            (got.x1, want.x1),
            (got.y1, want.y1),
        ] {
            assert!((g - w).abs() < 1e-6, "{what}: {got:?} against {want:?}");
        }
    };
    // Butt caps end at the path's ends; square caps reach half the width
    // past them (8.4.3.3); round caps as far, as a curve.
    near(
        boxed(&mut scratch, &pen(8.0, ContentCap::Butt, false)),
        rect(20.0, 46.0, 120.0, 54.0),
        "butt",
    );
    near(
        boxed(&mut scratch, &pen(8.0, ContentCap::Square, false)),
        rect(16.0, 46.0, 124.0, 54.0),
        "square",
    );
    let round = boxed(&mut scratch, &pen(8.0, ContentCap::Round, false)).expect("a box");
    assert!(
        (round.x0 - 16.0).abs() < 0.02 && (round.x1 - 124.0).abs() < 0.02,
        "round: {round:?}"
    );
    // The width is in user space: doubled by the transform, the line keeps
    // its device path and its pen is twice as wide.
    near(
        boxed(&mut scratch, &pen(4.0, ContentCap::Butt, true)),
        rect(20.0, 46.0, 120.0, 54.0),
        "doubled",
    );
    // A pen of no width marks its path, as a render's thinnest line does.
    near(
        boxed(&mut scratch, &pen(0.0, ContentCap::Butt, false)),
        rect(20.0, 50.0, 120.0, 50.0),
        "hairline",
    );
}

// ---- the documents ------------------------------------------------------------

/// A complete PDF of `objects`, numbered from 1, the first the catalog.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
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

fn object(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A face this repository carries in both builds, embedded as a program so
/// the glyphs have outlines whichever build reads them.
fn face_program() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tinker-pdf-font/data/liberation/LiberationSans-Regular.ttf"
    ))
    .expect("the vendored face is in the tree")
}

/// The pages the walk below is held to, one family of operators or more to a
/// page, on 200-point square pages sharing one resource dictionary.
const PAGES: [&str; 8] = [
    // Paths: fills of a rectangle and of a curve, two strokes, and a fill
    // half off the page.
    "10 10 30 20 re f \
     q 0 0 1 RG 6 w 20 100 m 120 100 l S Q \
     q 4 w 150 20 m 150 80 l S Q \
     50 150 m 50 166.5685 63.4315 180 80 180 c 96.5685 180 110 166.5685 110 150 c h f \
     190 190 30 30 re f",
    // Clips: a fill clipped to a square, and one clipped away.
    "q 20 20 60 60 re W n 0 0 200 200 re f Q \
     q 100 100 10 10 re W n 150 150 20 20 re f Q",
    // Text: glyphs filled, then glyphs that only clip and a fill they clip.
    "BT /F1 40 Tf 20 100 Td (Hello) Tj ET \
     q BT 7 Tr /F1 30 Tf 20 40 Td (Clip) Tj ET 0 0 200 200 re f Q",
    // Images: an XObject turned by `cm`, and an inline image.
    "q 30 40 -40 30 100 10 cm /Im1 Do Q \
     q 20 0 0 20 150 150 cm BI /W 2 /H 2 /CS /RGB /BPC 8 /F /AHx ID 000000FFFFFFFFFFFF000000> EI Q",
    // A shading painted over a clip.
    "q 30 30 50 50 re W n /Sh sh Q",
    // Forms: one clipped to its box, and a transparency group.
    "q 1 0 0 1 20 20 cm /Fm1 Do Q /Grp Do",
    // A soft mask: what its group draws is no mark, and what is painted under
    // it is bounded by its group's box until `/SMask /None`.
    "q /GSm gs 0 0 200 200 re f /GSnone gs 10 10 5 5 re f Q",
    // Optional content: a hidden layer's fill and image, and a visible
    // layer's fill.
    "/OC /Off BDC 0 0 200 200 re f /Im1 Do EMC /OC /On BDC 40 40 10 10 re f EMC",
];

/// The document holding [`PAGES`].
fn document() -> Vec<u8> {
    let first_page = 14;
    let kids: Vec<String> = (0..PAGES.len())
        .map(|i| format!("{} 0 R", first_page + 2 * i))
        .collect();
    let widths = " 600".repeat(95);
    let mut objects = vec![
        object(
            "<< /Type /Catalog /Pages 2 0 R \
             /OCProperties << /OCGs [8 0 R 9 0 R] /D << /ON [9 0 R] /OFF [8 0 R] >> >> >>",
        ),
        object(&format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            PAGES.len(),
            kids.join(" ")
        )),
        object(
            "<< /Font << /F1 4 0 R >> \
             /XObject << /Im1 7 0 R /Fm1 10 0 R /Grp 11 0 R >> \
             /Shading << /Sh 13 0 R >> \
             /ExtGState << /GSm << /SMask << /S /Luminosity /G 12 0 R >> >> \
                           /GSnone << /SMask /None >> >> \
             /Properties << /Off 8 0 R /On 9 0 R >> >>",
        ),
        object(&format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans \
             /FirstChar 32 /LastChar 126 /Widths [{widths}] /FontDescriptor 5 0 R >>"
        )),
        object(
            "<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 \
             /FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 900 /Descent -200 \
             /CapHeight 700 /StemV 80 /FontFile2 6 0 R >>",
        ),
        stream("", &face_program()),
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8",
            &[0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0],
        ),
        object("<< /Type /OCG /Name (Off) >>"),
        object("<< /Type /OCG /Name (On) >>"),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 50 50] /Resources 3 0 R",
            b"0 0 100 100 re f",
        ),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Group << /S /Transparency >>",
            b"10 10 20 20 re f",
        ),
        stream(
            "/Type /XObject /Subtype /Form /BBox [100 100 160 160] \
             /Group << /S /Transparency /CS /DeviceGray >>",
            b"1 g 100 100 60 60 re f",
        ),
        object(
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 200 0] \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
             /Extend [true true] >>",
        ),
    ];
    for (i, content) in PAGES.iter().enumerate() {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources 3 0 R \
             /Contents {} 0 R >>",
            first_page + 1 + 2 * i
        )));
        objects.push(stream("", content.as_bytes()));
    }
    pdf(&objects)
}

// ---- the walk -------------------------------------------------------------------

/// How finely the walk samples a curve: fine enough that its box is within a
/// thousandth of a point of the curve's.
const SAMPLES: usize = 512;

/// Every point a font segment list passes through, sampled, through `map`.
fn sampled_outline(segments: &[Segment], map: &Matrix) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let mut current = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    for segment in segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                current = (x, y);
                start = current;
                points.push(current);
            }
            Segment::LineTo { x, y } => {
                current = (x, y);
                points.push(current);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                for i in 1..=SAMPLES {
                    let t = i as f64 / SAMPLES as f64;
                    let mt = 1.0 - t;
                    points.push((
                        mt * mt * current.0 + 2.0 * mt * t * cx + t * t * x,
                        mt * mt * current.1 + 2.0 * mt * t * cy + t * t * y,
                    ));
                }
                current = (x, y);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                for i in 1..=SAMPLES {
                    let t = i as f64 / SAMPLES as f64;
                    let mt = 1.0 - t;
                    let f = |p0: f64, p1: f64, p2: f64, p3: f64| {
                        mt * mt * mt * p0
                            + 3.0 * mt * mt * t * p1
                            + 3.0 * mt * t * t * p2
                            + t * t * t * p3
                    };
                    points.push((f(current.0, c1x, c2x, x), f(current.1, c1y, c2y, y)));
                }
                current = (x, y);
            }
            Segment::Close => current = start,
        }
    }
    points.into_iter().map(|(x, y)| map.apply(x, y)).collect()
}

/// A device path's points, its curves sampled.
fn sampled_path(path: &[PathSegment]) -> Vec<(f64, f64)> {
    let segments: Vec<Segment> = path
        .iter()
        .map(|segment| match *segment {
            PathSegment::MoveTo { x, y } => Segment::MoveTo { x, y },
            PathSegment::LineTo { x, y } => Segment::LineTo { x, y },
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => Segment::CurveTo {
                c1x: x1,
                c1y: y1,
                c2x: x2,
                c2y: y2,
                x: x3,
                y: y3,
            },
            PathSegment::Close => Segment::Close,
        })
        .collect();
    sampled_outline(&segments, &Matrix::IDENTITY)
}

fn bounds(points: &[(f64, f64)]) -> Option<Rect> {
    let (first, rest) = points.split_first()?;
    Some(
        rest.iter()
            .fold(rect(first.0, first.1, first.0, first.1), |r, &(x, y)| {
                rect(r.x0.min(x), r.y0.min(y), r.x1.max(x), r.y1.max(y))
            }),
    )
}

/// Where the walk lets a paint land: `None` for everywhere.
fn within(limit: Option<Rect>, mark: Rect) -> Option<Rect> {
    match limit {
        None => Some(mark),
        Some(limit) => limit.meet(mark),
    }
}

/// The box of a recording's marks, by this walk's own rules — the ones the
/// pages above keep to: clips and soft masks bounded by their paths' boxes,
/// strokes of horizontal and vertical lines with butt caps, glyphs and curves
/// sampled, images their unit squares, `sh` its clip.
fn recomputed(list: &DisplayList) -> Option<Rect> {
    let page = rect(0.0, 0.0, 200.0, 200.0);
    let resources = list.resources();
    let mut hidden: Vec<bool> = Vec::new();
    let mut clip: Option<Rect> = Some(page);
    let mut soft: Option<Rect> = None;
    let mut stack: Vec<(Option<Rect>, Option<Rect>)> = Vec::new();
    let mut masks: Vec<(Option<Rect>, Option<Rect>, Option<Rect>)> = Vec::new();
    let mut text_clip: Option<Rect> = None;
    let mut marks: Option<Rect> = None;
    let add = |marks: &mut Option<Rect>, limit: Option<Rect>, mark: Option<Rect>, area: bool| {
        let Some(landed) = mark.and_then(|mark| within(limit, mark)) else {
            return;
        };
        if area && !landed.has_area() {
            return;
        }
        *marks = Some(marks.map_or(landed, |m| m.union(landed)));
    };
    let meet = |a: Option<Rect>, b: Option<Rect>| match (a, b) {
        (Some(a), Some(b)) => Some(a.meet(b).unwrap_or(rect(0.0, 0.0, 0.0, 0.0))),
        (a, None) => a,
        (None, b) => b,
    };
    for event in list.content() {
        let painting = !hidden.iter().any(|&h| h) && masks.is_empty();
        let limit = meet(clip, soft);
        match event {
            Event::BeginMarkedContent(scope) => hidden.push(!scope.visible),
            Event::EndMarkedContent => {
                hidden.pop();
            }
            Event::SaveState
            | Event::BeginForm { entered: true, .. }
            | Event::BeginGroup { entered: true, .. } => {
                stack.push((clip, soft));
            }
            Event::RestoreState | Event::EndForm { .. } | Event::EndGroup => {
                if let Some((c, s)) = stack.pop() {
                    clip = c;
                    soft = s;
                }
            }
            Event::BeginSoftMask {
                bbox,
                entered: true,
                ..
            } => {
                masks.push((clip, soft, bounds(&sampled_path(bbox))));
                clip = Some(page);
                soft = None;
            }
            Event::EndSoftMask => {
                if let Some((c, _, bbox)) = masks.pop() {
                    clip = c;
                    soft = bbox;
                }
            }
            Event::ClearSoftMask => soft = None,
            Event::ClipPath { path, .. } => {
                let b = bounds(&sampled_path(path)).filter(|b| b.has_area());
                clip = Some(match (clip, b) {
                    (Some(c), Some(b)) => c.meet(b).unwrap_or(rect(0.0, 0.0, 0.0, 0.0)),
                    _ => rect(0.0, 0.0, 0.0, 0.0),
                });
            }
            Event::FillPath { path, .. } if painting => {
                add(&mut marks, limit, bounds(&sampled_path(path)), true);
            }
            Event::StrokePath { path, state } if painting => {
                let width = state
                    .as_deref()
                    .map_or(1.0, |s| s.line_width * s.ctm.expansion());
                let half = width / 2.0;
                let b = bounds(&sampled_path(path)).map(|b| {
                    if b.x0 == b.x1 {
                        rect(b.x0 - half, b.y0, b.x1 + half, b.y1)
                    } else {
                        rect(b.x0, b.y0 - half, b.x1, b.y1 + half)
                    }
                });
                add(&mut marks, limit, b, true);
            }
            Event::ShowGlyph { glyph, state } if !hidden.iter().any(|&h| h) => {
                let mode = state
                    .as_deref()
                    .map_or(TextRenderMode::Fill, |s| s.text.render_mode);
                let Some(outline) = resources.outline(glyph.font_id, glyph.code) else {
                    continue;
                };
                let b = bounds(&sampled_outline(&outline.segments, &glyph.transform));
                if mode.clips() {
                    if let Some(b) = b {
                        text_clip = Some(text_clip.map_or(b, |t| t.union(b)));
                    }
                }
                if mode.fills() && painting {
                    add(&mut marks, limit, b, true);
                }
            }
            Event::EndText => {
                if let Some(t) = text_clip.take() {
                    clip = meet(clip, Some(t));
                }
            }
            Event::DrawImage { state, .. } if painting => {
                let ctm = state.as_deref().map_or(Matrix::IDENTITY, |s| s.ctm);
                let corners =
                    [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(x, y)| ctm.apply(x, y));
                add(&mut marks, limit, bounds(&corners), true);
            }
            Event::DrawShading { .. } if painting => {
                add(&mut marks, limit, Some(page), true);
            }
            _ => {}
        }
    }
    marks
}

/// **The box is the union of the recorded events.** Every page's analysis is
/// held to the walk above, which reads the same display list's events by
/// rules of its own, and to the box worked out by hand for the pages whose
/// marks are rectangles.
#[test]
fn the_box_is_the_union_of_the_recorded_events() {
    let document = Document::open(document()).expect("the document opens");
    assert_eq!(document.page_count() as usize, PAGES.len());
    let options = RenderOptions::default();
    let by_hand: [Option<(f64, f64, f64, f64)>; 8] = [
        None,
        Some((20.0, 20.0, 80.0, 80.0)),
        None,
        Some((60.0, 10.0, 170.0, 170.0)),
        Some((30.0, 30.0, 80.0, 80.0)),
        Some((10.0, 10.0, 70.0, 70.0)),
        Some((10.0, 10.0, 160.0, 160.0)),
        Some((40.0, 40.0, 50.0, 50.0)),
    ];
    for (index, want) in by_hand.iter().enumerate() {
        let page = document.page(index as u32).expect("a page");
        let list = page.display_list();
        assert!(list.is_retained(), "page {index} is retained");
        let analysed = list.analysis(&options);
        let walked = recomputed(&list);
        let got = analysed.marks.map(|m| m.points);
        match (got, walked) {
            (Some(got), Some(walked)) => {
                for (g, w) in [
                    (got.0, walked.x0),
                    (got.1, walked.y0),
                    (got.2, walked.x1),
                    (got.3, walked.y1),
                ] {
                    assert!(
                        (g - w).abs() < 1e-3,
                        "page {index}: analysed {got:?}, walked {walked:?}"
                    );
                }
            }
            (None, None) => {}
            other => panic!("page {index}: analysed and walked disagree: {other:?}"),
        }
        if let Some(want) = want {
            assert_eq!(got, Some(*want), "page {index}, worked out by hand");
        }
        // The page's own door is the list's.
        assert_eq!(page.analysis(&options), analysed, "page {index}");
    }

    // The paths page: the rectangle, the two strokes, the curve's arch and
    // the corner the page keeps of the fill that runs off it.
    let paths = document.page(0).expect("a page").analysis(&options);
    let (x0, y0, x1, y1) = paths.marks.expect("it paints").points;
    assert_eq!((x0, y0, x1, y1), (10.0, 10.0, 200.0, 200.0));
    // The text page draws, and its glyphs clip its second fill to them.
    let text = document.page(2).expect("a page").analysis(&options);
    assert!(text.marks.is_some(), "the glyphs are marks");
}

/// A list that is not retained is analysed by interpreting the page into the
/// same device, and answers alike: the walk is the same whichever feeds it.
#[test]
fn an_interpreted_page_answers_as_its_replay() {
    let document = Document::open(document()).expect("the document opens");
    let options = RenderOptions::default();
    for index in 0..PAGES.len() {
        let page = document.page(index as u32).expect("a page");
        let list = page.display_list();
        let replayed = list.analysis(&options);
        let resources = list.resources();
        let mut analyser = Analyser::new(resources, page.crop_box());
        let content = cos_pages::content_bytes(&page.doc, &page.inner);
        interpret(&content, Matrix::IDENTITY, &mut analyser, resources);
        assert_eq!(analyser.finish(&page, &options), replayed, "page {index}");
    }
}
