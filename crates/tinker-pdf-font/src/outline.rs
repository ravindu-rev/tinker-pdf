//! Glyph outlines.
//!
//! The shape a glyph describes, in font units, as moves, lines and curves. A
//! consumer scales by `units_per_em` and draws; nothing here knows about
//! pixels.

/// One instruction of a glyph's contour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Begin a contour.
    MoveTo {
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A straight line.
    LineTo {
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A quadratic Bézier, as TrueType describes curves.
    QuadTo {
        /// Control point.
        cx: f64,
        /// Control point.
        cy: f64,
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A cubic Bézier, as CFF and Type 1 describe curves.
    CurveTo {
        /// First control point.
        c1x: f64,
        /// First control point.
        c1y: f64,
        /// Second control point.
        c2x: f64,
        /// Second control point.
        c2y: f64,
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// Close the current contour.
    Close,
}

/// A glyph's shape.
#[derive(Clone, Debug, Default)]
pub struct Outline {
    /// The segments, in order.
    pub segments: Vec<Segment>,
}

impl Outline {
    /// Whether the outline draws nothing — a space, or a missing glyph.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Appends a segment, bounded so a hostile font cannot exhaust memory.
    pub fn push(&mut self, segment: Segment) {
        if self.segments.len() < MAX_SEGMENTS {
            self.segments.push(segment);
        }
    }

    /// The bounding box in font units, as `(x0, y0, x1, y1)`.
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut bounds: Option<(f64, f64, f64, f64)> = None;
        let mut add = |x: f64, y: f64| {
            if !x.is_finite() || !y.is_finite() {
                return;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for segment in &self.segments {
            match *segment {
                Segment::MoveTo { x, y } | Segment::LineTo { x, y } => add(x, y),
                Segment::QuadTo { cx, cy, x, y } => {
                    add(cx, cy);
                    add(x, y);
                }
                Segment::CurveTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => {
                    add(c1x, c1y);
                    add(c2x, c2y);
                    add(x, y);
                }
                Segment::Close => {}
            }
        }
        bounds
    }
}

/// A glyph may not describe more segments than this.
const MAX_SEGMENTS: usize = 1 << 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_include_control_points() {
        let mut outline = Outline::default();
        outline.push(Segment::MoveTo { x: 0.0, y: 0.0 });
        outline.push(Segment::QuadTo {
            cx: 50.0,
            cy: 100.0,
            x: 100.0,
            y: 0.0,
        });
        assert_eq!(outline.bounds(), Some((0.0, 0.0, 100.0, 100.0)));
    }

    #[test]
    fn an_empty_outline_has_no_bounds() {
        assert!(Outline::default().is_empty());
        assert_eq!(Outline::default().bounds(), None);
    }

    #[test]
    fn segments_are_bounded() {
        let mut outline = Outline::default();
        for _ in 0..(MAX_SEGMENTS + 1000) {
            outline.push(Segment::LineTo { x: 1.0, y: 1.0 });
        }
        assert_eq!(outline.segments.len(), MAX_SEGMENTS);
    }
}
