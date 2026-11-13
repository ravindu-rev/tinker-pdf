//! Page analysis: what a page draws, read from its drawing rather than from
//! its pixels — [`Page::analysis`] and [`DisplayList::analysis`].
//!
//! Design: `docs/design/page-analysis.md`. Feature documentation:
//! `docs/features/rendering.md`, "Page analysis".
//!
//! # One device, the renderer's answers
//!
//! [`Analyser`] is a [`Device`] that paints nothing. It is fed a display
//! list's events by [`replay`], or a page by [`interpret`] where the list is
//! not retained, and it answers the interpreter's three questions through the
//! renderer's own [`Admission`]: the recording was made with those answers, so
//! a replay never disagrees with it, and an interpretation runs the brackets a
//! render runs. It resolves glyph outlines, shadings and form scopes through
//! the same resources a render reads, so a glyph's box is its face's outline,
//! and a glyph no face draws is no mark, as it is no ink.
//!
//! # Where a paint can land
//!
//! It keeps, as rectangles in the page frame — default user space, in points —
//! what the renderer keeps to decide where a paint lands: the clip's box, and
//! the soft mask's. `q`, a form, a transparency group and a mask group save
//! both and their ends restore them, as `Renderer` saves and restores its clip
//! and its mask; a group's mask bounds what the group paints from outside it,
//! since 11.6.6 applies it to the group's result; a mask group draws under no
//! clip and no mask, and nothing in it is a mark.
//!
//! # What it reads
//!
//! The union of every mark's box, met with those rectangles and the crop box;
//! whether the page uses transparency in `pdfa/colour.rs`'s five ways; and
//! which paints are made with overprint on, from the keys K-01 stores in the
//! state — each read at the paints. The design doc says what a mark is and why each box is
//! taken as it is; the comments here cite where each rule comes from.

use std::sync::Arc;

use tinker_pdf_content::{
    interpret, replay, BlendMode, Device, Glyph, GraphicsState, Group, ImageRef,
    LineCap as ContentCap, LineJoin as ContentJoin, MarkedProps, MaskGroup, Matrix, PathSegment,
    TextRenderMode,
};
use tinker_pdf_cos::pages as cos_pages;
use tinker_pdf_font::{Outline, Segment};
use tinker_pdf_raster::canvas::Color;
use tinker_pdf_raster::fill::FillScratch;
use tinker_pdf_raster::geom::Path;
use tinker_pdf_raster::image::Transform;
use tinker_pdf_raster::stroke::{stroke_mapped_with, stroke_with, LineCap, LineJoin, StrokeStyle};
use tinker_pdf_render::{user_space_pen, Admission, GlyphSource};

use crate::annots::{self, AppearanceDevice, Kept};
use crate::pdfa::colour::{is_transparency_group, ALPHA_TOLERANCE};
use crate::resources::PageResources;
use crate::{Bitmap, DisplayList, Page, PixelRegion, RenderOptions};

/// How finely a stroke's outline is flattened before its box is read, in
/// points: the box falls short of a curved outline by at most this.
const STROKE_TOLERANCE: f64 = 0.01;

/// What a page draws, read from its drawing: [`Page::analysis`].
///
/// `#[non_exhaustive]`, so that an answer a later step adds is an addition:
/// read the fields by name.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PageAnalysis {
    /// The bounding box of every mark a render of the page paints, or `None`
    /// for a page that paints nothing on itself.
    pub marks: Option<Marks>,
    /// Whether the page uses transparency, in the five ways ISO 19005-1 6.4's
    /// reading in this engine names, read at what the page paints: a
    /// transparency group, a soft mask in force at a paint, an `/SMask` on an
    /// XObject the page draws, a blend mode other than Normal at a paint, or
    /// a constant alpha below one at a paint.
    pub transparency: bool,
    /// Which paints are made with overprint on, read from the overprint keys
    /// in force at each (8.6.7).
    pub overprint: Overprint,
}

/// Which of a page's paints are made with overprint on (8.6.7), and under
/// which mode: what the graphics state says at each paint, never what a
/// device would do with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Overprint {
    /// A non-stroking paint — a fill, a filled glyph, an image or `sh` — was
    /// made with `/op` in force, which is `/OP`'s value where a state writes
    /// no `/op` (Table 58).
    pub fills: bool,
    /// A stroke or a stroked glyph was made with `/OP` in force.
    pub strokes: bool,
    /// One of those paints was made under the non-zero overprint mode,
    /// `/OPM 1` (8.6.7.4), under which a DeviceCMYK component of zero leaves
    /// the ink beneath it: the mode as the paints settled it, not as a
    /// dictionary wrote it.
    pub nonzero_mode: bool,
}

impl Overprint {
    /// Whether any paint was made with overprint on.
    #[must_use]
    pub fn used(self) -> bool {
        self.fills || self.strokes
    }
}

/// The box a page's marks fill, in points and in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marks {
    /// `(x0, y0, x1, y1)` in default user space, `x0 <= x1` and `y0 <= y1`,
    /// inside the crop box: the numbers a `/CropBox` is written in.
    pub points: (f64, f64, f64, f64),
    /// The same box in the bitmap [`Page::render`] returns for the options
    /// the analysis was asked with: carried by the page's view transform at
    /// the scale the render applies, rounded outward to whole pixels, at
    /// least one pixel a side, and inside [`Page::pixel_size`]. Counted from
    /// the bitmap's top-left corner, as [`PixelRegion`] always is.
    pub pixels: PixelRegion,
}

impl Page {
    /// What this page draws, read from its drawing without rendering it
    /// (`docs/design/page-analysis.md`): the bounding box of its marks,
    /// whether it uses transparency and which of its paints overprint.
    ///
    /// The analysis of the page's display list for the usage `options`
    /// names ([`Page::display_list_for`], [`DisplayList::analysis`]), which
    /// this records and drops: a caller who also renders the page asks the
    /// list it renders from instead, and pays for one interpretation.
    ///
    /// Seven of `options` are read, because a render reads them: `usage`,
    /// whose optional-content states decide what is hidden; `content`,
    /// `annotations`, `widgets` and `other_annotations`, which parts of the
    /// page are part of what it draws; `magnification`, where an annotation
    /// `/NoZoom` keeps its size draws; and `scale`, with the clamp
    /// [`Page::pixel_size`] applies, for the box in pixels. The region, the
    /// format and the rest change no answer.
    #[must_use]
    pub fn analysis(&self, options: &RenderOptions) -> PageAnalysis {
        self.display_list_for(options.usage).analysis(options)
    }
}

/// [`DisplayList::analysis`]: the list's events replayed into one
/// [`Analyser`], or, for a list that is not retained, the page interpreted
/// into it, its annotations drawn through it as a render draws them.
///
/// A list carries the optional-content states of the usage it was recorded
/// for, so another usage is answered by the page's list for that usage, as a
/// render of another usage is drawn the direct way.
pub(crate) fn analyse(list: &DisplayList, options: &RenderOptions) -> PageAnalysis {
    if options.usage != list.usage() {
        return list
            .page()
            .display_list_for(options.usage)
            .analysis(options);
    }
    let page = list.page();
    let resources = list.resources();
    let mut analyser = Analyser::new(resources, page.crop_box());
    let placement = list.placement(options.magnification);
    let kinds = annots::Kinds::of(options);
    // A list whose parts' answers depend on a part these options leave out
    // is answered by interpreting the page, as a render of them draws it.
    if list.is_retained() && list.parts_replay_alike(options) {
        if options.content {
            replay(list.content(), &mut analyser);
        }
        for annotation in list.annotation_layers() {
            if !kinds.draws(annotation.widget) {
                continue;
            }
            match &annotation.kept {
                Kept::Calls { scope, events } => {
                    analyser.push_scope(resources.bind(scope));
                    replay(events, &mut analyser);
                    analyser.pop_scope();
                }
                Kept::Placed(appearance) => annots::draw_prepared(
                    &page.doc,
                    appearance,
                    page.fonts.as_ref(),
                    &mut analyser,
                    resources.operation(),
                    placement,
                ),
            }
        }
    } else {
        if options.content {
            let content = cos_pages::content_bytes(&page.doc, &page.inner);
            interpret(&content, Matrix::IDENTITY, &mut analyser, resources);
        }
        if options.annotations {
            annots::draw(
                &page.doc,
                &page.inner,
                page.fonts.as_ref(),
                &mut analyser,
                resources.operation(),
                placement,
                kinds,
            );
        }
    }
    analyser.finish(page, options)
}

// ---- rectangles ---------------------------------------------------------------

/// A closed rectangle of the page frame, `x0 <= x1` and `y0 <= y1`. One of no
/// area is a line or a point, which a stroke of zero width marks.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Rect {
    /// The overlap of two, or `None` where they do not meet.
    fn meet(self, other: Rect) -> Option<Rect> {
        let r = Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        };
        (r.x0 <= r.x1 && r.y0 <= r.y1).then_some(r)
    }

    /// The smallest rectangle holding both.
    fn union(self, other: Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    fn has_area(self) -> bool {
        self.x1 > self.x0 && self.y1 > self.y0
    }

    /// The box `map` carries this rectangle's four corners to.
    fn mapped(self, map: &Matrix) -> Option<Rect> {
        let mut extent = Extent::default();
        for (x, y) in [
            (self.x0, self.y0),
            (self.x1, self.y0),
            (self.x0, self.y1),
            (self.x1, self.y1),
        ] {
            let (x, y) = map.apply(x, y);
            extent.add(x, y);
        }
        extent.rect()
    }
}

/// A growing box over points, the non-finite ones left out.
#[derive(Clone, Copy, Debug, Default)]
struct Extent {
    rect: Option<Rect>,
}

impl Extent {
    fn add(&mut self, x: f64, y: f64) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        let point = Rect {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        };
        self.rect = Some(self.rect.map_or(point, |r| r.union(point)));
    }

    fn rect(self) -> Option<Rect> {
        self.rect
    }
}

/// Where a paint may land: everywhere, inside a rectangle, or nowhere.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Area {
    Everywhere,
    Within(Rect),
    Nowhere,
}

impl Area {
    fn meet(self, other: Area) -> Area {
        match (self, other) {
            (Area::Nowhere, _) | (_, Area::Nowhere) => Area::Nowhere,
            (Area::Everywhere, area) | (area, Area::Everywhere) => area,
            (Area::Within(a), Area::Within(b)) => a.meet(b).map_or(Area::Nowhere, Area::Within),
        }
    }

    /// The part of `rect` inside this area.
    fn clip(self, rect: Rect) -> Option<Rect> {
        match self {
            Area::Everywhere => Some(rect),
            Area::Within(area) => area.meet(rect),
            Area::Nowhere => None,
        }
    }

    /// A clip's area: its path's box, or nowhere for a path of no area, which
    /// covers no pixel and so clips everything away.
    fn of_clip(rect: Option<Rect>) -> Area {
        match rect {
            Some(rect) if rect.has_area() => Area::Within(rect),
            _ => Area::Nowhere,
        }
    }
}

// ---- exact boxes of paths --------------------------------------------------------

/// A cubic Bézier's value at `t`.
fn cubic_at(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let mt = 1.0 - t;
    mt * mt * mt * p0 + 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t * p3
}

/// The parameters in `(0, 1)` where a cubic's coordinate turns: the roots of
/// its derivative, `a t² + b t + c` over three, by the form that does not
/// cancel.
fn cubic_turns(p0: f64, p1: f64, p2: f64, p3: f64) -> [Option<f64>; 2] {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let inside = |t: f64| (t > 0.0 && t < 1.0).then_some(t);
    if a == 0.0 {
        if b == 0.0 {
            return [None, None];
        }
        return [inside(-c / b), None];
    }
    let disc = b * b - 4.0 * a * c;
    if disc.is_nan() || disc < 0.0 {
        return [None, None];
    }
    let s = disc.sqrt();
    let q = if b >= 0.0 {
        -0.5 * (b + s)
    } else {
        -0.5 * (b - s)
    };
    let second = if q == 0.0 { None } else { inside(c / q) };
    [inside(q / a), second]
}

/// The parameter in `(0, 1)` where a quadratic's coordinate turns.
fn quad_turn(p0: f64, p1: f64, p2: f64) -> Option<f64> {
    let d = p0 - 2.0 * p1 + p2;
    if d == 0.0 {
        return None;
    }
    let t = (p0 - p1) / d;
    (t > 0.0 && t < 1.0).then_some(t)
}

/// Grows `extent` by a cubic from `p0` — its end and its turning points, not
/// its control points.
fn add_cubic(extent: &mut Extent, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
    extent.add(p3.0, p3.1);
    for t in cubic_turns(p0.0, p1.0, p2.0, p3.0)
        .into_iter()
        .chain(cubic_turns(p0.1, p1.1, p2.1, p3.1))
        .flatten()
    {
        extent.add(
            cubic_at(p0.0, p1.0, p2.0, p3.0, t),
            cubic_at(p0.1, p1.1, p2.1, p3.1, t),
        );
    }
}

/// Grows `extent` by a quadratic from `p0`, as [`add_cubic`] does a cubic.
fn add_quad(extent: &mut Extent, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) {
    extent.add(p2.0, p2.1);
    let at = |a: f64, b: f64, c: f64, t: f64| {
        let mt = 1.0 - t;
        mt * mt * a + 2.0 * mt * t * b + t * t * c
    };
    for t in [quad_turn(p0.0, p1.0, p2.0), quad_turn(p0.1, p1.1, p2.1)]
        .into_iter()
        .flatten()
    {
        extent.add(at(p0.0, p1.0, p2.0, t), at(p0.1, p1.1, p2.1, t));
    }
}

/// A device path's exact bounds: every point it passes through, a curve's
/// turning points among them, and none of its control points.
fn path_box(path: &[PathSegment]) -> Option<Rect> {
    let mut extent = Extent::default();
    let mut current: Option<(f64, f64)> = None;
    let mut start: Option<(f64, f64)> = None;
    for segment in path {
        match *segment {
            PathSegment::MoveTo { x, y } => {
                extent.add(x, y);
                current = Some((x, y));
                start = current;
            }
            PathSegment::LineTo { x, y } => {
                extent.add(x, y);
                current = Some((x, y));
            }
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                match current {
                    Some(p0) => add_cubic(&mut extent, p0, (x1, y1), (x2, y2), (x3, y3)),
                    // A curve with no point before it starts nowhere a reader
                    // can say; its own points are all there is.
                    None => {
                        extent.add(x1, y1);
                        extent.add(x2, y2);
                        extent.add(x3, y3);
                    }
                }
                current = Some((x3, y3));
            }
            PathSegment::Close => current = start,
        }
    }
    extent.rect()
}

/// A glyph's outline, through its transform, as exact bounds.
fn outline_box(outline: &Outline, map: &Matrix) -> Option<Rect> {
    let mut extent = Extent::default();
    let mut current: Option<(f64, f64)> = None;
    let mut start: Option<(f64, f64)> = None;
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                let p = map.apply(x, y);
                extent.add(p.0, p.1);
                current = Some(p);
                start = current;
            }
            Segment::LineTo { x, y } => {
                let p = map.apply(x, y);
                extent.add(p.0, p.1);
                current = Some(p);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                let c = map.apply(cx, cy);
                let p = map.apply(x, y);
                match current {
                    Some(p0) => add_quad(&mut extent, p0, c, p),
                    None => {
                        extent.add(c.0, c.1);
                        extent.add(p.0, p.1);
                    }
                }
                current = Some(p);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                let c1 = map.apply(c1x, c1y);
                let c2 = map.apply(c2x, c2y);
                let p = map.apply(x, y);
                match current {
                    Some(p0) => add_cubic(&mut extent, p0, c1, c2, p),
                    None => {
                        extent.add(c1.0, c1.1);
                        extent.add(c2.0, c2.1);
                        extent.add(p.0, p.1);
                    }
                }
                current = Some(p);
            }
            Segment::Close => current = start,
        }
    }
    extent.rect()
}

/// A device path as the rasterizer's, in the page frame, for the stroker.
fn raster_path(path: &[PathSegment]) -> Path {
    let mut out = Path::new();
    for segment in path {
        match *segment {
            PathSegment::MoveTo { x, y } => out.move_to(x, y),
            PathSegment::LineTo { x, y } => out.line_to(x, y),
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => out.curve_to(x1, y1, x2, y2, x3, y3),
            PathSegment::Close => out.close(),
        }
    }
    out
}

/// A glyph's outline as the rasterizer's path, through its transform.
fn outline_path(outline: &Outline, map: &Matrix) -> Path {
    let mut out = Path::new();
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                let (x, y) = map.apply(x, y);
                out.move_to(x, y);
            }
            Segment::LineTo { x, y } => {
                let (x, y) = map.apply(x, y);
                out.line_to(x, y);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                let (cx, cy) = map.apply(cx, cy);
                let (x, y) = map.apply(x, y);
                out.quad_to(cx, cy, x, y);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                let (c1x, c1y) = map.apply(c1x, c1y);
                let (c2x, c2y) = map.apply(c2x, c2y);
                let (x, y) = map.apply(x, y);
                out.curve_to(c1x, c1y, c2x, c2y, x, y);
            }
            Segment::Close => out.close(),
        }
    }
    out
}

/// The same six numbers in the rasterizer's spelling.
fn transform(m: &Matrix) -> Transform {
    Transform {
        a: m.a,
        b: m.b,
        c: m.c,
        d: m.d,
        e: m.e,
        f: m.f,
    }
}

/// `map`'s inverse, or `None` where it is not finite: `Renderer::pen`'s.
fn inverse(map: &Transform) -> Option<Transform> {
    let det = map.a * map.d - map.b * map.c;
    if !det.is_finite() || det == 0.0 {
        return None;
    }
    let inverse = Transform {
        a: map.d / det,
        b: -map.b / det,
        c: -map.c / det,
        d: map.a / det,
        e: (map.c * map.f - map.d * map.e) / det,
        f: (map.b * map.e - map.a * map.f) / det,
    };
    [
        inverse.a, inverse.b, inverse.c, inverse.d, inverse.e, inverse.f,
    ]
    .iter()
    .all(|v| v.is_finite())
    .then_some(inverse)
}

/// A path's stroke outline under `style` and the state's transform, as its
/// box: the outline `Renderer::pen` writes, with the page frame for the
/// device and no thinnest-line floor. A pen of no width marks the path's own
/// box, because a render draws it as the thinnest line it can (8.4.3.2).
fn stroke_box(
    scratch: &mut FillScratch,
    path: &Path,
    segments_box: Option<Rect>,
    style: StrokeStyle,
    state: &GraphicsState,
) -> Option<Rect> {
    let m = state.ctm;
    let width = style.width * m.expansion();
    if width.is_nan() || width <= 0.0 {
        return segments_box;
    }
    let mut routed = false;
    if user_space_pen(&m).is_some() {
        let map = transform(&m);
        if let Some(back) = inverse(&map) {
            let user = path.mapped(&back);
            stroke_mapped_with(scratch, &user, &style, &map, 0.0, STROKE_TOLERANCE, None);
            routed = true;
        }
    }
    if !routed {
        let scale = m.expansion();
        let style = StrokeStyle {
            width: style.width * scale,
            dashes: style.dashes.iter().map(|d| d * scale).collect(),
            dash_phase: style.dash_phase * scale,
            ..style
        };
        stroke_with(scratch, path, &style, STROKE_TOLERANCE, None);
    }
    let (x0, y0, x1, y1) = scratch.outline_bounds()?;
    [x0, y0, x1, y1]
        .iter()
        .all(|v| v.is_finite())
        .then_some(Rect { x0, y0, x1, y1 })
}

/// A stroke's pen as the rasterizer takes it, from the state (8.4.3).
fn stroke_style(state: &GraphicsState) -> StrokeStyle {
    StrokeStyle {
        width: state.line_width,
        cap: match state.line_cap {
            ContentCap::Butt => LineCap::Butt,
            ContentCap::Round => LineCap::Round,
            ContentCap::Square => LineCap::Square,
        },
        join: match state.line_join {
            ContentJoin::Miter => LineJoin::Miter,
            ContentJoin::Round => LineJoin::Round,
            ContentJoin::Bevel => LineJoin::Bevel,
        },
        miter_limit: state.miter_limit,
        dashes: state.dashes.clone(),
        dash_phase: state.dash_phase,
    }
}

// ---- the device ---------------------------------------------------------------

/// Which resources names resolve in: the list's, or a form's or an
/// appearance's own.
enum Scope<'r> {
    Borrowed(&'r PageResources),
    Owned(Arc<PageResources>),
}

impl core::ops::Deref for Scope<'_> {
    type Target = PageResources;

    fn deref(&self) -> &PageResources {
        match self {
            Scope::Borrowed(resources) => resources,
            Scope::Owned(resources) => resources,
        }
    }
}

/// The clip's and the soft mask's areas, as `q` saves them.
#[derive(Clone, Copy, Debug)]
struct Held {
    clip: Area,
    soft: Option<Area>,
}

/// An accepted transparency group: what its `Do` displaced.
#[derive(Clone, Copy, Debug)]
struct GroupFrame {
    held: Held,
    saved: usize,
}

/// An accepted soft mask's group: what its `gs` displaced, and what the mask
/// will bound once it is made.
#[derive(Clone, Copy, Debug)]
struct MaskFrame {
    held: Held,
    saved: usize,
    /// `/G`'s box, or `None` where it has none.
    bbox: Option<Rect>,
    /// Whether the mask reads more than zero outside `/G`'s box.
    outside: bool,
}

/// What one paint is, for the rules that read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    /// A fill, a filled glyph, an image or `sh`: the non-stroking paints.
    Fill,
    /// A stroke or a stroked glyph.
    Stroke,
}

/// The analysis device. See the module documentation.
pub(crate) struct Analyser<'r> {
    scope: Scope<'r>,
    /// Scopes [`AppearanceDevice::push_scope`] displaced, innermost last.
    pushed: Vec<Scope<'r>>,
    /// The scope each open form displaced, `None` for a form with none of
    /// its own.
    forms: Vec<Option<Scope<'r>>>,
    admission: Admission,
    /// The crop box: what a render's canvas shows.
    page: Rect,
    clip: Area,
    /// The soft mask in force: `None` for none, `Some(Everywhere)` for one
    /// that reads more than zero outside its group's box.
    soft: Option<Area>,
    saved: Vec<Held>,
    groups: Vec<GroupFrame>,
    masks: Vec<MaskFrame>,
    /// The boxes of a text object's clipping glyphs so far.
    text_clip: Option<Rect>,
    /// Whether a glyph of this text object selected a clipping mode, whether
    /// or not it had an outline (9.3.6): the renderer's own flag.
    text_clip_requested: bool,
    scratch: FillScratch,
    marks: Option<Rect>,
    transparency: bool,
    overprint: Overprint,
}

impl<'r> Analyser<'r> {
    pub(crate) fn new(resources: &'r PageResources, crop: (f64, f64, f64, f64)) -> Analyser<'r> {
        let (x0, y0, x1, y1) = crop;
        Analyser {
            scope: Scope::Borrowed(resources),
            pushed: Vec::new(),
            forms: Vec::new(),
            admission: Admission::new(),
            page: Rect {
                x0: x0.min(x1),
                y0: y0.min(y1),
                x1: x0.max(x1),
                y1: y0.max(y1),
            },
            clip: Area::Everywhere,
            soft: None,
            saved: Vec::new(),
            groups: Vec::new(),
            masks: Vec::new(),
            text_clip: None,
            text_clip_requested: false,
            scratch: FillScratch::new(),
            marks: None,
            transparency: false,
            overprint: Overprint::default(),
        }
    }

    /// Whether a paint now is a paint on the page: outside hidden content
    /// (8.11.3.2), which a render runs and does not paint, and outside a mask
    /// group, which is a picture of a mask.
    fn painting(&self) -> bool {
        !self.admission.hidden() && self.masks.is_empty()
    }

    /// Whether a soft mask shapes a paint now: one in force, or one on the
    /// result of a group that encloses it (11.6.6).
    fn masked(&self) -> bool {
        self.soft.is_some() || self.groups.iter().any(|frame| frame.held.soft.is_some())
    }

    /// Where a paint can land: the crop box, the clip, the soft mask, and the
    /// masks on the results of the groups around it.
    fn limit(&self) -> Area {
        let mut area = Area::Within(self.page).meet(self.clip);
        for soft in self
            .groups
            .iter()
            .filter_map(|frame| frame.held.soft)
            .chain(self.soft)
        {
            area = area.meet(soft);
        }
        area
    }

    /// What one paint says about the page: its alpha, its blend mode and the
    /// mask over it, read against `pdfa/colour.rs`'s reading of ISO 19005-1
    /// 6.4; and the overprint key for its kind, with the mode, from the
    /// parameters K-01 stores in its state.
    fn read_paint(&mut self, ink: Ink, state: &GraphicsState) {
        // 8.6.7: `/OP` for stroking, `/op` for every other painting operation,
        // `/op` taking `/OP`'s value where a dictionary writes none (Table 58),
        // which is how the stored parameters already hold it.
        let stored = state.stored_parameters();
        let overprints = match ink {
            Ink::Fill => stored.fill_overprint,
            Ink::Stroke => stored.stroke_overprint,
        };
        if overprints {
            match ink {
                Ink::Fill => self.overprint.fills = true,
                Ink::Stroke => self.overprint.strokes = true,
            }
            // 8.6.7.4 defines 0 and 1; a mode written otherwise is not 0.
            if stored.overprint_mode != 0 {
                self.overprint.nonzero_mode = true;
            }
        }
        let alpha = match ink {
            Ink::Fill => state.fill_alpha,
            Ink::Stroke => state.stroke_alpha,
        };
        // 11.3.5: `/Compatible` is read as Normal where `/BM` is parsed.
        if alpha < 1.0 - ALPHA_TOLERANCE || state.blend != BlendMode::Normal || self.masked() {
            self.transparency = true;
        }
    }

    /// A mark with box `rect`, met with where a paint can land. `area` says
    /// whether it must cover an area to be one: every mark but a stroke of
    /// no width.
    fn mark(&mut self, rect: Rect, area: bool) {
        let Some(landed) = self.limit().clip(rect) else {
            return;
        };
        if area && !landed.has_area() {
            return;
        }
        self.marks = Some(self.marks.map_or(landed, |marks| marks.union(landed)));
    }

    /// A stroke's mark: its outline's box, an area unless its pen has none.
    fn mark_stroke(
        &mut self,
        path: &Path,
        own: Option<Rect>,
        style: StrokeStyle,
        state: &GraphicsState,
    ) {
        if let Some(rect) = stroke_box(&mut self.scratch, path, own, style, state) {
            let area = rect.has_area();
            self.mark(rect, area);
        }
    }

    /// The answers, and the box in pixels at `options`' scale.
    fn finish(mut self, page: &Page, options: &RenderOptions) -> PageAnalysis {
        // A page's own transparency group (11.4.7) is nothing the content
        // draws, so it is read off the page dictionary, by `pdfa/colour.rs`'s
        // predicate.
        if let Ok(object) = page.doc.get(page.inner.reference) {
            if object
                .as_dict()
                .is_some_and(|dict| is_transparency_group(&page.doc, dict))
            {
                self.transparency = true;
            }
        }
        PageAnalysis {
            marks: self.marks.map(|rect| Marks {
                points: (rect.x0, rect.y0, rect.x1, rect.y1),
                pixels: pixel_box(page, options, rect),
            }),
            transparency: self.transparency,
            overprint: self.overprint,
        }
    }
}

/// `rect` in the pixels of the bitmap `Page::render` returns for `options`:
/// carried by the transform the renderer draws the page through, rounded
/// outward, at least a pixel a side and inside the page's pixels.
fn pixel_box(page: &Page, options: &RenderOptions, rect: Rect) -> PixelRegion {
    let (width, height) = page.size();
    let (_, applied) = page.scales(options);
    let (full_width, full_height) = tinker_pdf_render::page_pixels(width, height, applied);
    let view = tinker_pdf_render::page_view_transform(page.crop_box(), page.rotation(), applied);
    let Some(device) = rect.mapped(&view) else {
        return PixelRegion::new(0, 0, 0, 0);
    };
    let side = |lo: f64, hi: f64, full: u32| -> (u32, u32) {
        let full_f = f64::from(full);
        let start = lo.floor().clamp(0.0, full_f);
        let end = hi.ceil().clamp(0.0, full_f);
        // `as` from a finite float in `0..=full` is exact.
        let (mut start, mut end) = (start as u32, end as u32);
        if end <= start {
            if start >= full {
                start = full.saturating_sub(1);
            }
            end = (start + 1).min(full);
        }
        (start, end - start)
    };
    let (x, w) = side(device.x0, device.x1, full_width);
    let (y, h) = side(device.y0, device.y1, full_height);
    PixelRegion::new(x, y, w, h)
}

impl Device for Analyser<'_> {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        // The renderer's order exactly: a glyph in a hidden layer neither
        // paints nor adds to the text clip.
        if self.admission.hidden() {
            return;
        }
        let mode = state.text.render_mode;
        if matches!(mode, TextRenderMode::Invisible) || (!mode.paints() && !mode.clips()) {
            return;
        }
        if mode.clips() {
            self.text_clip_requested = true;
        }
        let Some(outline) = self.scope.outline(glyph.font_id, glyph.code) else {
            return;
        };
        if outline.is_empty() || !glyph.transform.is_finite() {
            return;
        }
        let own = outline_box(&outline, &glyph.transform);
        let painting = self.painting();
        if mode.fills() && painting {
            self.read_paint(Ink::Fill, state);
            if let Some(rect) = own {
                self.mark(rect, true);
            }
        }
        if mode.strokes() && painting {
            self.read_paint(Ink::Stroke, state);
            // 9.3.6 strokes a glyph as a path, with the line width in user
            // space and the pen's other parameters at their defaults, as the
            // renderer strokes one.
            let style = StrokeStyle {
                width: state.line_width,
                ..StrokeStyle::default()
            };
            let path = outline_path(&outline, &glyph.transform);
            self.mark_stroke(&path, own, style, state);
        }
        if mode.clips() {
            if let Some(rect) = own {
                self.text_clip = Some(self.text_clip.map_or(rect, |clip| clip.union(rect)));
            }
        }
    }

    fn end_text(&mut self) {
        // 9.3.6: the clipping glyphs of a text object clip together at `ET`,
        // and a text object that asked to clip and drew nothing clips
        // everything away.
        let requested = std::mem::take(&mut self.text_clip_requested);
        match self.text_clip.take() {
            Some(rect) => self.clip = self.clip.meet(Area::of_clip(Some(rect))),
            None if requested => self.clip = Area::Nowhere,
            None => {}
        }
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, _even_odd: bool) {
        if !self.painting() || path.is_empty() {
            return;
        }
        self.read_paint(Ink::Fill, state);
        if let Some(rect) = path_box(path) {
            self.mark(rect, true);
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if !self.painting() || path.is_empty() {
            return;
        }
        self.read_paint(Ink::Stroke, state);
        let own = path_box(path);
        let built = raster_path(path);
        self.mark_stroke(&built, own, stroke_style(state), state);
    }

    fn clip_path(&mut self, path: &[PathSegment], _state: &GraphicsState, _even_odd: bool) {
        // A clip applies in hidden content as anywhere else: only painting
        // stops there.
        if path.is_empty() {
            return;
        }
        self.clip = self.clip.meet(Area::of_clip(path_box(path)));
    }

    fn save_state(&mut self) {
        self.saved.push(Held {
            clip: self.clip,
            soft: self.soft,
        });
    }

    fn restore_state(&mut self) {
        if let Some(held) = self.saved.pop() {
            self.clip = held.clip;
            self.soft = held.soft;
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        if !self.painting() {
            return;
        }
        self.read_paint(Ink::Fill, state);
        // `pdfa/colour.rs`'s third way: an XObject the page draws with an
        // `/SMask` key. An inline image has no dictionary that could hold one.
        if !image.inline && self.scope.xobject_has_soft_mask(&image.name) {
            self.transparency = true;
        }
        // 8.9.5.2: the unit square of the current transform, which an image
        // that would not decode is drawn over as its placeholder too.
        if !state.ctm.is_finite() {
            return;
        }
        let unit = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
        };
        if let Some(rect) = unit.mapped(&state.ctm) {
            self.mark(rect, true);
        }
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        if !self.painting() {
            return;
        }
        let Ok(Some(shading)) = self.scope.shading(name) else {
            // Not painted: nothing a render draws, and nothing it reads.
            return;
        };
        self.read_paint(Ink::Fill, state);
        // 8.7.4.2: `sh` paints the current clip, and the page where there is
        // none. A mesh paints its own geometry inside that.
        let rect = match shading.mesh() {
            Some(mesh) => {
                let mut extent = Extent::default();
                for &(x, y) in &mesh.positions {
                    let (x, y) = state.ctm.apply(x, y);
                    extent.add(x, y);
                }
                for patch in &mesh.patches {
                    for &(x, y) in patch.points.iter().flatten() {
                        let (x, y) = state.ctm.apply(x, y);
                        extent.add(x, y);
                    }
                }
                match extent.rect() {
                    Some(rect) => rect,
                    None => return,
                }
            }
            None => self.page,
        };
        self.mark(rect, true);
    }

    fn begin_marked_content(
        &mut self,
        _tag: &[u8],
        visible: bool,
        _hidden_layer: Option<&str>,
        _props: Option<&MarkedProps>,
    ) {
        self.admission.begin_marked_content(visible);
    }

    fn end_marked_content(&mut self) {
        self.admission.end_marked_content();
    }

    fn begin_form(&mut self, _id: u64, name: &[u8]) -> bool {
        // An `/SMask` on a form is `pdfa/colour.rs`'s too, read as on an image.
        if self.painting() && self.scope.xobject_has_soft_mask(name) {
            self.transparency = true;
        }
        self.saved.push(Held {
            clip: self.clip,
            soft: self.soft,
        });
        // 8.10.1: a form's own `/Resources`, asked as the renderer asks.
        let nested = self.scope.form_scope(name);
        let displaced = nested.map(|scope| std::mem::replace(&mut self.scope, Scope::Owned(scope)));
        self.forms.push(displaced);
        true
    }

    fn end_form(&mut self, _id: u64) {
        if let Some(Some(outer)) = self.forms.pop() {
            self.scope = outer;
        }
        if let Some(held) = self.saved.pop() {
            self.clip = held.clip;
            self.soft = held.soft;
        }
    }

    fn begin_group(&mut self, _group: Group, state: &GraphicsState) -> bool {
        // A group in hidden content paints nothing and is declined, as the
        // renderer declines it; any other was asked for, whatever a render's
        // budgets answer.
        if self.admission.hidden() {
            return false;
        }
        if self.masks.is_empty() {
            self.transparency = true;
            // The group's own alpha and blend mode apply to its result
            // (11.6.6), which is a paint like any other.
            self.read_paint(Ink::Fill, state);
        }
        if !self.admission.begin_group() {
            return false;
        }
        self.groups.push(GroupFrame {
            held: Held {
                clip: self.clip,
                soft: self.soft.take(),
            },
            saved: self.saved.len(),
        });
        true
    }

    fn end_group(&mut self) {
        let Some(frame) = self.groups.pop() else {
            return;
        };
        self.admission.end_group();
        self.saved.truncate(frame.saved);
        self.clip = frame.held.clip;
        self.soft = frame.held.soft;
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        _state: &GraphicsState,
    ) -> bool {
        if !self.admission.begin_soft_mask() {
            return false;
        }
        // What the mask reads outside `/G`'s box: `/BC`'s luminosity through
        // `/TR` for a luminosity mask, zero for an alpha one — the renderer's
        // `outside`, by the same arithmetic.
        let outside = if mask.luminosity {
            let backdrop = mask.backdrop.unwrap_or(tinker_pdf_content::Rgb::BLACK);
            let raw = Color::rgb(backdrop.r, backdrop.g, backdrop.b).luminosity();
            mask.transfer
                .as_ref()
                .map_or(raw, |lut| lut.get(usize::from(raw)).copied().unwrap_or(raw))
        } else {
            0
        };
        self.masks.push(MaskFrame {
            held: Held {
                clip: self.clip,
                soft: self.soft,
            },
            saved: self.saved.len(),
            bbox: path_box(bbox),
            outside: outside != 0,
        });
        // A mask group draws under no clip and no mask of its own.
        self.clip = Area::Everywhere;
        self.soft = None;
        true
    }

    fn end_soft_mask(&mut self) {
        let Some(frame) = self.masks.pop() else {
            return;
        };
        self.admission.end_soft_mask();
        self.saved.truncate(frame.saved);
        self.clip = frame.held.clip;
        // The mask the `gs` replaced is gone, not restored (11.6.5.1): the new
        // one bounds what follows by its group's box where it reads zero
        // outside it, and bounds nothing where it does not.
        self.soft = Some(match (frame.outside, frame.bbox) {
            (false, Some(bbox)) => Area::Within(bbox),
            _ => Area::Everywhere,
        });
    }

    fn clear_soft_mask(&mut self) {
        self.soft = None;
    }

    fn group_budget_spent(&mut self) {
        // The interpreter passes over groups and masks past its budget: the
        // page asked for them, which is transparency the paints cannot show.
        self.admission.note_budget_spent();
        self.transparency = true;
    }
}

// ---- colour or grey ------------------------------------------------------------

/// The thresholds of the grey test (`docs/design/page-analysis.md`, "Colour
/// or grey"): [`Bitmap::greyness`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GreyThresholds {
    /// The widest a pixel's components may spread — the largest of its red,
    /// green and blue less the smallest, in levels of 255 — and the pixel be
    /// grey. 8 by default: past the few levels a grey picture coded as colour
    /// wanders by, and short of the palest tint a page draws on purpose.
    pub tolerance: u8,
    /// How many coloured pixels a page may hold and be grey. None by default.
    pub allowed: u64,
}

impl Default for GreyThresholds {
    fn default() -> Self {
        GreyThresholds {
            tolerance: 8,
            allowed: 0,
        }
    }
}

/// What the grey test read of a render: [`Bitmap::greyness`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Greyness {
    /// Whether the page is grey: its coloured pixels number at most
    /// [`GreyThresholds::allowed`].
    pub grey: bool,
    /// The pixels whose components spread more than
    /// [`GreyThresholds::tolerance`].
    pub coloured: u64,
    /// The pixels read: the bitmap's width times its height.
    pub pixels: u64,
    /// The widest spread of any pixel that holds something, so a caller sees
    /// how near the line a page fell.
    pub widest: u8,
}

/// `round(c * 255 / a)`, clamped to 255 and zero where the alpha is:
/// `Bitmap::straightened`'s arithmetic, for one component.
fn straighten(c: u8, a: u8) -> u8 {
    let a = u32::from(a);
    (u32::from(c) * 255 + a / 2)
        .checked_div(a)
        .map_or(0, |v| v.min(255)) as u8
}

impl Bitmap {
    /// Whether this render needs colour (`docs/design/page-analysis.md`):
    /// each pixel read as the colour [`Bitmap::to_png`] writes for it —
    /// straightened when premultiplied, ink through 8.6.4.4's relation, Lab
    /// back to sRGB, grey replicated — and coloured when its components
    /// spread more than `thresholds.tolerance`; the page is grey when its
    /// coloured pixels number at most `thresholds.allowed`. A pixel whose
    /// alpha is zero holds nothing and is not coloured.
    ///
    /// `None` exactly when the bitmap is not a picture, `to_png`'s
    /// condition. Rows are read over `stride`, and no row's padding is.
    #[must_use]
    pub fn greyness(&self, thresholds: &GreyThresholds) -> Option<Greyness> {
        if !self.layout().is_picture(self.data.len()) {
            return None;
        }
        let format = self.format;
        let components = format.components();
        let alpha_at = format.has_alpha().then(|| components - 1);
        let premultiplied = self.premultiplied && format.has_alpha();
        let row = self.width as usize * components;
        let (mut coloured, mut widest) = (0u64, 0u8);
        for y in 0..self.height as usize {
            let start = y * self.stride;
            let Some(pixels) = self.data.get(start..start + row) else {
                continue;
            };
            for pixel in pixels.chunks_exact(components) {
                let alpha = alpha_at.map_or(255, |at| pixel.get(at).copied().unwrap_or(255));
                if alpha == 0 {
                    continue;
                }
                let mut straight = [0u8; 5];
                for (slot, &c) in straight.iter_mut().zip(pixel) {
                    *slot = c;
                }
                if premultiplied {
                    for c in straight.iter_mut().take(components - 1) {
                        *c = straighten(*c, alpha);
                    }
                }
                let Some(colour) =
                    tinker_pdf_raster::toolbox::decode(format, straight.get(..components)?)
                else {
                    continue;
                };
                let high = colour.r.max(colour.g).max(colour.b);
                let low = colour.r.min(colour.g).min(colour.b);
                let spread = high - low;
                widest = widest.max(spread);
                if spread > thresholds.tolerance {
                    coloured += 1;
                }
            }
        }
        Some(Greyness {
            grey: coloured <= thresholds.allowed,
            coloured,
            pixels: u64::from(self.width) * u64::from(self.height),
            widest,
        })
    }
}

impl Page {
    /// Whether this page needs colour: [`Page::render`] at `options` — the
    /// caller's scale, since pixels decide — read by [`Bitmap::greyness`].
    /// What a render decides, the test inherits: a hidden layer's colour is
    /// not drawn and a colour an opaque grey paints over is not seen.
    #[must_use]
    pub fn greyness(
        &self,
        options: &RenderOptions,
        thresholds: &GreyThresholds,
    ) -> Option<Greyness> {
        self.render(options).greyness(thresholds)
    }
}

impl DisplayList {
    /// [`Page::greyness`], from this list's render.
    #[must_use]
    pub fn greyness(
        &self,
        options: &RenderOptions,
        thresholds: &GreyThresholds,
    ) -> Option<Greyness> {
        self.render(options).greyness(thresholds)
    }
}

impl AppearanceDevice for Analyser<'_> {
    fn push_scope(&mut self, scope: Arc<PageResources>) {
        let outer = std::mem::replace(&mut self.scope, Scope::Owned(scope));
        self.pushed.push(outer);
    }

    fn pop_scope(&mut self) {
        if let Some(outer) = self.pushed.pop() {
            self.scope = outer;
        }
    }
}

#[cfg(test)]
mod tests;
