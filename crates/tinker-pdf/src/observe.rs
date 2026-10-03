//! What the geometric inferences read off a page: the text device's page,
//! and the ink drawn beside it, read once.
//!
//! [`crate::reading_order`] and [`crate::tables`] both want the [`TextPage`]
//! that [`crate::Page::text`] builds — one assembler, or an inferred view
//! drifts from search and selection — and the tables want what a text device
//! throws away: the rules a table is drawn with and the fills that shade its
//! cells. Interpreting the page twice, once for each, is what the designs
//! costed before a replay existed.
//!
//! What runs instead is **one interpretation into a tee**: [`Observer`] is a
//! [`Device`] that hands the four calls [`TextDevice`] implements —
//! `begin_marked_content`, `end_marked_content`, `show_glyph` and `end_text` —
//! to a `TextDevice` unchanged, and keeps paths and clips for itself. It
//! answers the interpreter's three questions (`begin_form`, `begin_group`,
//! `begin_soft_mask`) with `TextDevice`'s own answers — every form, no group,
//! no mask — so the interpreter does exactly what it does for
//! [`crate::Page::text`] and the page it builds is that page, character for
//! character. `crates/tinker-pdf/tests/reading_order.rs` holds the equality
//! over every committed fixture rather than leaving it to this argument.
//!
//! # The tree hidden
//!
//! [`Observed::read`] with `keep_artifacts` set is the page **as an untagged
//! reader would see it**: an `/Artifact` scope (14.8.2.2) is handed to the text
//! device under another tag, so its text is extracted rather than dropped.
//! An artifact is part of a file's tagging — the producer saying "this running
//! head is not content" — and an inference measured with the tree hidden has
//! to find the running head without being told, which it cannot do if the
//! head was never extracted. Rules are kept whatever scope draws them: a
//! producer that marks a table's borders `/Artifact` has still drawn them.
//!
//! # Rules
//!
//! A *rule* ([`TableRule`]) is ink a table could be drawn with: a straight
//! segment of a stroked path, axis-aligned to within [`RULE_SLANT`] over its
//! length and no wider than [`RULE_MAX_WIDTH`], or a filled rectangle no
//! thicker than that in one dimension. Every other filled rectangle is a
//! [`Fill`], which is what a shaded header row is. Curves are neither. The
//! thresholds are design/table-reconstruction.md's and are in points, because
//! a rule's weight is a typesetter's choice made in points rather than in ems
//! of the text it rules.
//!
//! The recorder keeps no accumulated clip, and neither does the interpreter's
//! device seam: `q`, `Q`, a form's bracket and each `W` arrive as calls. So
//! the observer accumulates the clip itself, as a rectangle while every clip
//! path is one, and refuses a rule drawn under anything else — counted, and
//! named by [`crate::TableWarning::ClipNotRectangular`] — rather than reading
//! a rule through a clip it cannot intersect. A rule in a hidden layer
//! (8.11.3.2) is not on the page and is not kept.

use tinker_pdf_content::{
    interpret, Device, Glyph, GraphicsState, MarkedProps, Matrix, PathSegment, TextDevice,
    TextPage, TextWarning,
};
use tinker_pdf_cos::pages as cos_pages;

use crate::tables::{TableRule, MAX_TABLE_RULES};
use crate::{resources, text_order, Page};

/// The widest a stroke, or the thinnest dimension of a filled rectangle, may
/// be and still be a rule, in points (design/table-reconstruction.md).
pub(crate) const RULE_MAX_WIDTH: f64 = 2.0;

/// The shortest straight segment kept as a rule at all, in points.
///
/// Table reconstruction applies its own minimum in ems of the page's text;
/// this one only keeps a page of dots and dashes from filling the budget.
const RULE_MIN_LENGTH: f64 = 2.0;

/// How far a segment may lean and still be axis-aligned, in points over its
/// whole length (design/table-reconstruction.md).
const RULE_SLANT: f64 = 0.5;

/// How deep the observer's own clip stack goes.
///
/// The interpreter refuses `q` past 64 and forms past `MAX_FORM_DEPTH`, so a
/// stack this deep is never reached by a real page; it only keeps a bracket
/// imbalance from growing it.
const MAX_SAVED_CLIPS: usize = 1 << 10;

/// A filled rectangle too large to be a rule — a cell's shading, a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fill {
    /// `(x0, y0, x1, y1)`, ordered.
    pub rect: (f64, f64, f64, f64),
    /// Whether it painted something other than white.
    pub inked: bool,
}

/// What one interpretation of a page left behind.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observed {
    /// The page's text, in logical order (ruling 14) — [`Page::text`]'s,
    /// unless artifacts were kept.
    pub text: TextPage,
    /// The rules, at most [`MAX_TABLE_RULES`] of them; none when the page drew
    /// more.
    pub rules: Vec<TableRule>,
    /// How many rules the page drew, counted past the cap.
    pub rules_drawn: usize,
    /// Rules dropped because the clip in force was not a rectangle.
    pub rules_unclipped: usize,
    /// Filled rectangles that are not rules, at most [`MAX_TABLE_RULES`] —
    /// the cap on what a page's ink may cost, read the same way.
    pub fills: Vec<Fill>,
}

impl Observed {
    /// Interprets `page` once.
    ///
    /// `keep_artifacts` reads `/Artifact` scopes as content; see the module
    /// documentation.
    pub(crate) fn read(page: &Page, keep_artifacts: bool) -> Observed {
        let content = cos_pages::content_bytes(&page.doc, &page.inner);
        // `Page::text`'s own resources: no glyph outlines are needed, so no
        // provider is consulted.
        let resources = resources::PageResources::new(&page.doc, &page.inner, None);
        let mut observer = Observer::new(keep_artifacts);
        interpret(&content, Matrix::IDENTITY, &mut observer, &resources);
        for name in resources.missing_fonts() {
            observer.text.warn(TextWarning::UnknownFont { name });
        }
        let Observer {
            text,
            rules,
            rules_drawn,
            rules_unclipped,
            fills,
            ..
        } = observer;
        let mut text = text.finish();
        text_order::into_logical_order(&mut text);
        // Past the cap a page's rules are not read at all, rather than read
        // up to an arbitrary first part of the content stream.
        let rules = if rules_drawn > MAX_TABLE_RULES {
            Vec::new()
        } else {
            rules
        };
        Observed {
            text,
            rules,
            rules_drawn,
            rules_unclipped,
            fills,
        }
    }
}

/// The clip in force, as far as a rule cares.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Clip {
    /// Nothing clips.
    Open,
    /// A rectangle, `(x0, y0, x1, y1)`.
    Rect(f64, f64, f64, f64),
    /// Something that is not a rectangle, so whether a rule is visible under
    /// it is not a question this answers.
    Shaped,
}

impl Clip {
    fn and(self, other: Clip) -> Clip {
        match (self, other) {
            (Clip::Shaped, _) | (_, Clip::Shaped) => Clip::Shaped,
            (Clip::Open, other) | (other, Clip::Open) => other,
            (Clip::Rect(a0, b0, a1, b1), Clip::Rect(c0, d0, c1, d1)) => {
                Clip::Rect(a0.max(c0), b0.max(d0), a1.min(c1), b1.min(d1))
            }
        }
    }
}

/// The tee: a [`TextDevice`] and the ink beside it.
struct Observer {
    text: TextDevice,
    keep_artifacts: bool,
    clip: Clip,
    saved: Vec<Clip>,
    /// Each open form's entry: how deep `saved` was and the clip in force.
    forms: Vec<(usize, Clip)>,
    /// Forms opened past [`MAX_SAVED_CLIPS`], whose ends restore nothing.
    forms_unrecorded: usize,
    /// Each open marked-content scope's own visibility (8.11.3.2), so an `EMC`
    /// knows whether the scope it closes was one hiding things.
    scopes: Vec<bool>,
    /// How many open scopes hide what they hold.
    hidden: usize,
    rules: Vec<TableRule>,
    rules_drawn: usize,
    rules_unclipped: usize,
    fills: Vec<Fill>,
}

impl Observer {
    fn new(keep_artifacts: bool) -> Observer {
        Observer {
            text: TextDevice::new(),
            keep_artifacts,
            clip: Clip::Open,
            saved: Vec::new(),
            forms: Vec::new(),
            forms_unrecorded: 0,
            scopes: Vec::new(),
            hidden: 0,
            rules: Vec::new(),
            rules_drawn: 0,
            rules_unclipped: 0,
            fills: Vec::new(),
        }
    }

    fn push_clip(&mut self) {
        if self.saved.len() < MAX_SAVED_CLIPS {
            self.saved.push(self.clip);
        }
    }

    fn pop_clip(&mut self) {
        if let Some(clip) = self.saved.pop() {
            self.clip = clip;
        }
    }

    /// Keeps `rule` if the clip lets any of it through, cut to the clip.
    fn rule(&mut self, mut rule: TableRule) {
        if !(rule.at.is_finite() && rule.from.is_finite() && rule.to.is_finite()) {
            return;
        }
        if rule.to - rule.from < RULE_MIN_LENGTH {
            return;
        }
        match self.clip {
            Clip::Open => {}
            Clip::Shaped => {
                self.rules_unclipped = self.rules_unclipped.saturating_add(1);
                return;
            }
            Clip::Rect(x0, y0, x1, y1) => {
                let (across_lo, across_hi, along_lo, along_hi) = if rule.horizontal {
                    (y0, y1, x0, x1)
                } else {
                    (x0, x1, y0, y1)
                };
                if rule.at < across_lo || rule.at > across_hi {
                    return;
                }
                rule.from = rule.from.max(along_lo);
                rule.to = rule.to.min(along_hi);
                if rule.to - rule.from < RULE_MIN_LENGTH {
                    return;
                }
            }
        }
        self.rules_drawn = self.rules_drawn.saturating_add(1);
        if self.rules.len() < MAX_TABLE_RULES {
            self.rules.push(rule);
        }
    }

    /// A straight segment from `a` to `b`, kept when it is axis-aligned.
    fn segment(&mut self, a: (f64, f64), b: (f64, f64), width: f64) {
        let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
        if dy <= RULE_SLANT && dx > dy {
            self.rule(TableRule {
                horizontal: true,
                at: (a.1 + b.1) / 2.0,
                from: a.0.min(b.0),
                to: a.0.max(b.0),
                width,
            });
        } else if dx <= RULE_SLANT && dy > dx {
            self.rule(TableRule {
                horizontal: false,
                at: (a.0 + b.0) / 2.0,
                from: a.1.min(b.1),
                to: a.1.max(b.1),
                width,
            });
        }
    }
}

/// The subpaths of `path` that are axis-aligned rectangles, as
/// `(x0, y0, x1, y1)`, and whether every subpath was one.
fn rectangles(path: &[PathSegment]) -> (Vec<(f64, f64, f64, f64)>, bool) {
    let mut out = Vec::new();
    let mut all = true;
    for subpath in subpaths(path) {
        match rectangle(subpath) {
            Some(rect) => out.push(rect),
            None => all = false,
        }
    }
    (out, all)
}

/// `path` cut at every `MoveTo`.
fn subpaths(path: &[PathSegment]) -> Vec<&[PathSegment]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (at, segment) in path.iter().enumerate() {
        if at > start && matches!(segment, PathSegment::MoveTo { .. }) {
            if let Some(piece) = path.get(start..at) {
                out.push(piece);
            }
            start = at;
        }
    }
    if let Some(piece) = path.get(start..) {
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// One subpath as an axis-aligned rectangle: a `MoveTo`, three or four
/// `LineTo`s that each move along one axis, and an optional `Close`, the
/// corners closing back on the start.
fn rectangle(subpath: &[PathSegment]) -> Option<(f64, f64, f64, f64)> {
    let mut points = Vec::with_capacity(5);
    for segment in subpath {
        match *segment {
            PathSegment::MoveTo { x, y } if points.is_empty() => points.push((x, y)),
            PathSegment::LineTo { x, y } if !points.is_empty() && points.len() < 5 => {
                points.push((x, y));
            }
            PathSegment::Close => {}
            _ => return None,
        }
    }
    let first = *points.first()?;
    if points.len() == 5 && points.last().copied() == Some(first) {
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }
    // Every edge moves along exactly one axis, and the edges alternate.
    let mut horizontal_edges = 0;
    for at in 0..4 {
        let a = *points.get(at)?;
        let b = *points.get((at + 1) % 4)?;
        let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
        if dy <= 1e-6 && dx > 1e-6 {
            horizontal_edges += 1;
        } else if !(dx <= 1e-6 && dy > 1e-6) {
            return None;
        }
    }
    if horizontal_edges != 2 {
        return None;
    }
    let xs = points.iter().map(|p| p.0);
    let ys = points.iter().map(|p| p.1);
    let x0 = xs.clone().fold(f64::INFINITY, f64::min);
    let x1 = xs.fold(f64::NEG_INFINITY, f64::max);
    let y0 = ys.clone().fold(f64::INFINITY, f64::min);
    let y1 = ys.fold(f64::NEG_INFINITY, f64::max);
    [x0, y0, x1, y1]
        .iter()
        .all(|v| v.is_finite())
        .then_some((x0, y0, x1, y1))
}

/// Whether the fill colour is white, which a fill of it does not ink.
fn white(state: &GraphicsState) -> bool {
    let c = state.fill_color;
    c.r == 255 && c.g == 255 && c.b == 255
}

impl Device for Observer {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.text.show_glyph(glyph, state);
    }

    fn end_text(&mut self) {
        self.text.end_text();
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        self.scopes.push(visible);
        if !visible {
            self.hidden += 1;
        }
        // The text device drops what an `/Artifact` scope draws (14.8.2.2);
        // any other tag it reads as content. See the module documentation.
        let tag: &[u8] = if self.keep_artifacts && tag == b"Artifact" {
            b"Artifact (read as content)"
        } else {
            tag
        };
        self.text
            .begin_marked_content(tag, visible, hidden_layer, props);
    }

    fn end_marked_content(&mut self) {
        if self.scopes.pop() == Some(false) {
            self.hidden = self.hidden.saturating_sub(1);
        }
        self.text.end_marked_content();
    }

    fn save_state(&mut self) {
        self.push_clip();
    }

    fn restore_state(&mut self) {
        self.pop_clip();
    }

    // 8.10.2: a form runs with the surrounding state saved, and the
    // interpreter's own comment says `begin_form` saves the clip and
    // `end_form` restores it. Entering every form is the trait's default and
    // the text device's answer.
    //
    // The interpreter runs the form on a `q` stack of its own and drops what
    // the form left open with no `restore_state`, so the clip and the depth
    // of the saved stack are kept here and both put back: popping one entry
    // would restore the form's own last `q` and leave its `/BBox` clipping
    // the rest of the page.
    fn begin_form(&mut self, _id: u64, _name: &[u8]) -> bool {
        if self.forms.len() < MAX_SAVED_CLIPS {
            self.forms.push((self.saved.len(), self.clip));
        } else {
            self.forms_unrecorded = self.forms_unrecorded.saturating_add(1);
        }
        true
    }

    fn end_form(&mut self, _id: u64) {
        if self.forms_unrecorded > 0 {
            self.forms_unrecorded -= 1;
        } else if let Some((depth, clip)) = self.forms.pop() {
            self.saved.truncate(depth);
            self.clip = clip;
        }
    }

    fn clip_path(&mut self, path: &[PathSegment], _state: &GraphicsState, _even_odd: bool) {
        let (rects, all) = rectangles(path);
        let clip = match (rects.as_slice(), all) {
            ([(x0, y0, x1, y1)], true) => Clip::Rect(*x0, *y0, *x1, *y1),
            _ => Clip::Shaped,
        };
        self.clip = self.clip.and(clip);
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, _even_odd: bool) {
        if self.hidden > 0 {
            return;
        }
        let (rects, _) = rectangles(path);
        let inked = !white(state);
        for (x0, y0, x1, y1) in rects {
            let (w, h) = (x1 - x0, y1 - y0);
            if h <= RULE_MAX_WIDTH && w > h {
                self.rule(TableRule {
                    horizontal: true,
                    at: (y0 + y1) / 2.0,
                    from: x0,
                    to: x1,
                    width: h,
                });
            } else if w <= RULE_MAX_WIDTH && h > w {
                self.rule(TableRule {
                    horizontal: false,
                    at: (x0 + x1) / 2.0,
                    from: y0,
                    to: y1,
                    width: w,
                });
            } else if self.fills.len() < MAX_TABLE_RULES {
                self.fills.push(Fill {
                    rect: (x0, y0, x1, y1),
                    inked,
                });
            }
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.hidden > 0 {
            return;
        }
        let width = state.line_width.abs() * state.ctm.expansion();
        if !width.is_finite() || width > RULE_MAX_WIDTH {
            return;
        }
        let mut start: Option<(f64, f64)> = None;
        let mut pen: Option<(f64, f64)> = None;
        for segment in path {
            match *segment {
                PathSegment::MoveTo { x, y } => {
                    start = Some((x, y));
                    pen = Some((x, y));
                }
                PathSegment::LineTo { x, y } => {
                    if let Some(from) = pen {
                        self.segment(from, (x, y), width);
                    }
                    pen = Some((x, y));
                }
                PathSegment::CurveTo { x3, y3, .. } => pen = Some((x3, y3)),
                PathSegment::Close => {
                    if let (Some(from), Some(to)) = (pen, start) {
                        self.segment(from, to, width);
                    }
                    pen = start;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<PathSegment> {
        vec![
            PathSegment::MoveTo { x: x0, y: y0 },
            PathSegment::LineTo { x: x1, y: y0 },
            PathSegment::LineTo { x: x1, y: y1 },
            PathSegment::LineTo { x: x0, y: y1 },
            PathSegment::Close,
        ]
    }

    #[test]
    fn a_rectangle_is_recognised_and_a_triangle_is_not() {
        assert_eq!(
            rectangle(&rect_path(1.0, 2.0, 5.0, 9.0)),
            Some((1.0, 2.0, 5.0, 9.0))
        );
        let triangle = [
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::LineTo { x: 4.0, y: 0.0 },
            PathSegment::LineTo { x: 0.0, y: 4.0 },
            PathSegment::Close,
        ];
        assert_eq!(rectangle(&triangle), None);
    }

    #[test]
    fn a_thin_fill_is_a_rule_and_a_thick_one_is_a_fill() {
        let mut observer = Observer::new(false);
        let state = GraphicsState::new(Matrix::IDENTITY);
        observer.fill_path(&rect_path(10.0, 10.0, 110.0, 10.5), &state, false);
        observer.fill_path(&rect_path(10.0, 20.0, 110.0, 40.0), &state, false);
        assert_eq!(observer.rules.len(), 1);
        assert!(observer.rules.iter().all(|r| r.horizontal));
        assert_eq!(observer.fills.len(), 1);
        assert!(observer.fills[0].inked, "black is ink");
    }

    #[test]
    fn a_rule_in_a_hidden_layer_is_not_on_the_page() {
        let mut observer = Observer::new(false);
        let state = GraphicsState::new(Matrix::IDENTITY);
        observer.begin_marked_content(b"OC", false, Some("draft"), None);
        observer.fill_path(&rect_path(10.0, 10.0, 110.0, 10.5), &state, false);
        observer.end_marked_content();
        observer.fill_path(&rect_path(10.0, 20.0, 110.0, 20.5), &state, false);
        assert_eq!(observer.rules.len(), 1);
        assert_eq!(observer.rules[0].at, 20.25);
    }
}
