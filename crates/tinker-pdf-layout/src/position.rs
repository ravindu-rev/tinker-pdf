//! The four positions that are not `static`: CSS 2.2 §9.4.3 and §9.6, and
//! `css-position-3` §3.4.
//!
//! Feature documentation: `docs/features/epub.md`.
//!
//! # Two of the four are answers and not approximations
//!
//! `fixed` and `sticky` are usually where a print build shrugs, and neither
//! has to be.
//!
//! **§9.6.1 says what `fixed` means on paper**, in as many words: *"in the
//! case of paged media, fixed boxes are repeated on every page, and are fixed
//! with respect to the page box"*. So a `fixed` box in this build is positioned
//! against the page and drawn on **every** page of the document, which is not a
//! degradation of the screen behaviour — it is the specification's own paged
//! answer, and it is what a running header in a stylesheet was asking for.
//!
//! **`css-position-3` §3.4 says what `sticky` means with no scrollport.** A
//! sticky box is offset by the distance its nearest scrollport has scrolled,
//! clamped to its containing block; a paginated document has no scrollport, so
//! that distance is zero for every box on every page, and §3.4's own words are
//! that it is then *"the same as `relative`"*. Not a fallback: the value of a
//! parameter this medium does not have.
//!
//! # And two of them are the real work
//!
//! **§9.4.3's `relative` is an offset applied after layout that changes
//! nothing else.** That sentence is load-bearing here rather than merely true:
//! [`crate::flow`] is one continuous column whose `y` never goes backwards, and
//! a box moved *up* by `top: -10px` would put an item above the one before it
//! and the page cutter could no longer order the flow. So the offset is carried
//! to **paint** — into each run's `x` and `y`, and into a
//! [`crate::flow::BlockRecord`]'s own `dy` — and the flow keeps the box's
//! original position, which is precisely what §9.4.3 asks for: *"the box is
//! offset ... this does not affect the layout of any other box"*.
//!
//! **§9.6's `absolute` is out of flow**, which this crate already has a shape
//! for: a [`crate::flow::FloatRecord`] is items that are not in the column,
//! carried with the `y` they were placed at. An absolutely positioned box is
//! the same thing with a different placement rule and one difference at
//! pagination, which is why `FloatRecord` grew a flag rather than a twin: a
//! float that does not fit the page it started on may be **pushed** whole to
//! the next one, and an absolutely positioned box may not — pushing it is
//! moving it, and where it is is the whole of what the declaration said.

use tinker_pdf_css::property::{Inset, LengthPercentage, Side, Sides};

/// The rectangle an offset is resolved against.
///
/// §9.3.2's percentages are of the containing block's **width** for `left` and
/// `right` and of its **height** for `top` and `bottom`, so the two measures
/// are separate fields and the height is optional: a block container's height
/// is `auto` until its content is laid out, and §10.5's answer for a percentage
/// against an indefinite size is that it behaves as `auto`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Containing {
    /// Its left content edge, in flow coordinates.
    pub left: f64,
    /// Its top content edge.
    pub top: f64,
    /// Its content width.
    pub width: f64,
    /// Its content height, where it has one.
    pub height: Option<f64>,
}

/// One inset as a used length, or `None` where it is `auto` — or where it is a
/// percentage of a measure that does not exist.
#[must_use]
pub(crate) fn inset_px(inset: Inset, measure: Option<f64>) -> Option<f64> {
    match inset {
        Inset::Auto => None,
        Inset::Length(LengthPercentage::Px(px)) => Some(px),
        Inset::Length(LengthPercentage::Percent(percent)) => {
            measure.map(|size| size * percent / 100.0)
        }
    }
}

/// §9.4.3's offset for a relatively positioned box, as `(dx, dy)`.
///
/// **`left` beats `right` and `top` beats `bottom`**, which is §9.4.3's
/// over-constrained rule for a left-to-right direction: when both of a pair are
/// lengths *"the position is over-constrained, and one of them has to be
/// ignored"*, and the one ignored is `right` (for `direction: ltr`) and
/// `bottom`. With only the second of a pair stated, the box moves the other
/// way: `right: 10px` moves it ten points **left**, which is the sign a build
/// gets wrong once and then everywhere.
#[must_use]
pub(crate) fn relative_offset(inset: &Sides<Inset>, containing: &Containing) -> (f64, f64) {
    let left = inset_px(inset.get(Side::Left), Some(containing.width));
    let right = inset_px(inset.get(Side::Right), Some(containing.width));
    let top = inset_px(inset.get(Side::Top), containing.height);
    let bottom = inset_px(inset.get(Side::Bottom), containing.height);
    let dx = match (left, right) {
        (Some(left), _) => left,
        (None, Some(right)) => -right,
        (None, None) => 0.0,
    };
    let dy = match (top, bottom) {
        (Some(top), _) => top,
        (None, Some(bottom)) => -bottom,
        (None, None) => 0.0,
    };
    (dx, dy)
}

/// §9.6's used left edge for an out-of-flow box.
///
/// Three cases and they are §10.3.7's: `left` places it from the containing
/// block's left edge, `right` from its right edge less the box's own width, and
/// **neither** leaves it at its static position — *"the position the box would
/// have had if it had been `static`"*, which is the case nearly every real
/// stylesheet takes and the one an implementation is most likely to leave out.
#[must_use]
pub(crate) fn used_left(
    inset: &Sides<Inset>,
    containing: &Containing,
    static_left: f64,
    outer_width: f64,
) -> f64 {
    match (
        inset_px(inset.get(Side::Left), Some(containing.width)),
        inset_px(inset.get(Side::Right), Some(containing.width)),
    ) {
        (Some(left), _) => containing.left + left,
        (None, Some(right)) => containing.left + containing.width - right - outer_width,
        (None, None) => static_left,
    }
}

/// The same for the top edge, against the containing block's height.
///
/// The height is `None` for a containing block whose own is `auto`, and then a
/// `bottom` has nothing to measure from: the box stays at its static position
/// rather than being placed against a number this build invented. §10.6.4 makes
/// that the honest answer — the used height it would need does not exist yet.
#[must_use]
pub(crate) fn used_top(
    inset: &Sides<Inset>,
    containing: &Containing,
    static_top: f64,
    outer_height: f64,
) -> f64 {
    match (
        inset_px(inset.get(Side::Top), containing.height),
        inset_px(inset.get(Side::Bottom), containing.height),
        containing.height,
    ) {
        (Some(top), _, _) => containing.top + top,
        (None, Some(bottom), Some(height)) => containing.top + height - bottom - outer_height,
        (None, _, _) => static_top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insets(
        top: Option<f64>,
        right: Option<f64>,
        bottom: Option<f64>,
        left: Option<f64>,
    ) -> Sides<Inset> {
        let one = |value: Option<f64>| match value {
            Some(px) => Inset::Length(LengthPercentage::Px(px)),
            None => Inset::Auto,
        };
        Sides {
            top: one(top),
            right: one(right),
            bottom: one(bottom),
            left: one(left),
        }
    }

    const BOX: Containing = Containing {
        left: 10.0,
        top: 20.0,
        width: 100.0,
        height: Some(200.0),
    };

    /// §9.4.3: `right` alone moves the box **left**.
    #[test]
    fn a_right_offset_moves_the_box_the_other_way() {
        let (dx, dy) = relative_offset(&insets(None, Some(10.0), Some(4.0), None), &BOX);
        assert_eq!((dx, dy), (-10.0, -4.0));
    }

    /// §9.4.3: with both of a pair stated the second is ignored.
    #[test]
    fn left_beats_right_and_top_beats_bottom() {
        let (dx, dy) = relative_offset(&insets(Some(1.0), Some(9.0), Some(9.0), Some(2.0)), &BOX);
        assert_eq!((dx, dy), (2.0, 1.0));
    }

    /// §10.3.7: neither inset leaves the box where `static` would have put it.
    #[test]
    fn no_inset_is_the_static_position() {
        assert_eq!(
            used_left(&insets(None, None, None, None), &BOX, 77.0, 30.0),
            77.0
        );
        assert_eq!(
            used_top(&insets(None, None, None, None), &BOX, 88.0, 30.0),
            88.0
        );
    }

    /// And `right`/`bottom` measure from the far edge, less the box's own size.
    #[test]
    fn the_far_insets_measure_from_the_far_edge() {
        assert_eq!(
            used_left(&insets(None, Some(5.0), None, None), &BOX, 0.0, 30.0),
            75.0
        );
        assert_eq!(
            used_top(&insets(None, None, Some(5.0), None), &BOX, 0.0, 30.0),
            185.0
        );
    }

    /// A `bottom` against a containing block with no height has nothing to
    /// measure from, so the box stays where `static` put it.
    #[test]
    fn a_bottom_against_an_auto_height_is_the_static_position() {
        let auto = Containing {
            height: None,
            ..BOX
        };
        assert_eq!(
            used_top(&insets(None, None, Some(5.0), None), &auto, 42.0, 30.0),
            42.0
        );
    }
}
