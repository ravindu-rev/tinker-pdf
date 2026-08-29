//! One path owns a run, asserted rather than agreed to.
//!
//! `docs/design/shaping.md`'s risk table names this as one of the item's five
//! risks — *"two measurement paths disagree — the exact failure `metrics.rs`
//! warns about, now with three paths"* — and its mitigation as *"one path owns
//! a run, asserted by test (milestone 6)"*. This is that test.
//!
//! # Why an assertion and not a convention
//!
//! Before shaping there was one way to measure a run and the rule was free. A
//! shaper adds a second, and the two **do** disagree: a ligature is narrower
//! than the glyphs it replaced and a joined Arabic word is narrower still. A
//! line broken against one measurement and drawn with the other breaks in the
//! wrong place, and it does so *plausibly* — the text is all there, the page
//! is full, and one word is on the wrong line.
//!
//! So the provider below is built to make the mistake fatal rather than
//! subtle: its `Metrics::advance` and `Metrics::measure` **panic**. A
//! pagination that completes is a pagination in which no run was measured
//! twice, and a `flow.rs` that grew a second `metrics.measure` call would fail
//! here rather than in a book.

use std::cell::RefCell;

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::Display;
use tinker_pdf_layout::metrics::{
    FixedPitch, FontRequest, Metrics, PlacedGlyph, ShapedText, Shaper, Vertical,
};
use tinker_pdf_layout::{layout, BoxNode, CellSpan, Content, Limits, Options};

/// A provider that can only shape, and treats being asked for a character's
/// advance as the defect it would be.
#[derive(Default)]
struct OnlyShapes {
    /// Every run this provider was asked to shape, in order, so the test can
    /// say what was measured rather than only that something was.
    runs: RefCell<Vec<String>>,
}

/// Every glyph half an em wide, except that `fi` is one em rather than two —
/// a ligature, in effect, and the smallest thing that makes the two paths
/// disagree by a number a test can name.
impl Shaper for OnlyShapes {
    fn shape(&self, text: &str, font: &FontRequest<'_>, rtl: bool) -> ShapedText {
        self.runs.borrow_mut().push(text.to_string());
        let mut glyphs = Vec::new();
        let mut advance = 0.0;
        let mut rest = text;
        let mut at = 0usize;
        while !rest.is_empty() {
            let (width, len) = if rest.starts_with("fi") {
                (font.size, 2)
            } else {
                (
                    font.size / 2.0,
                    rest.chars().next().map_or(1, char::len_utf8),
                )
            };
            glyphs.push(PlacedGlyph {
                glyph: 1,
                cluster: u32::try_from(at).unwrap_or(0),
                x_advance: width,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
            });
            advance += width;
            at += len;
            rest = &rest[len..];
        }
        ShapedText {
            glyphs,
            advance,
            rtl,
        }
    }
}

impl Metrics for OnlyShapes {
    fn advance(&self, ch: char, _font: &FontRequest<'_>) -> f64 {
        panic!("a run was measured through Metrics::advance ({ch:?}) as well as through the shaper")
    }

    fn measure(&self, text: &str, _font: &FontRequest<'_>) -> f64 {
        panic!(
            "a run was measured through Metrics::measure ({text:?}) as well as through the shaper"
        )
    }

    fn vertical(&self, font: &FontRequest<'_>) -> Vertical {
        // Not a run measurement, and deliberately still answered: a line's
        // height is a property of the face and not of the text on it, so
        // nothing about it is duplicated by the shaper.
        Vertical {
            ascent: font.size * 0.8,
            descent: font.size * 0.2,
        }
    }

    fn shaper(&self) -> Option<&dyn Shaper> {
        Some(self)
    }
}

fn tree(text: &str) -> BoxNode {
    let mut style = ComputedStyle::initial();
    style.display = Display::Block;
    let mut inline = ComputedStyle::initial();
    inline.font_size = 10.0;
    BoxNode {
        style,
        content: Content::Children(vec![BoxNode {
            style: inline,
            content: Content::Text(text.into()),
            anchor: None,
            span: CellSpan::ONE,
        }]),
        anchor: None,
        span: CellSpan::ONE,
    }
}

#[test]
fn a_run_the_shaper_owns_is_never_also_measured_by_metrics() {
    let provider = OnlyShapes::default();
    let laid = layout(
        &tree("the fifth finger of the fist"),
        &provider,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("a paragraph of Latin lays out");
    assert_eq!(laid.text(), "the fifth finger of the fist");
    assert!(
        !provider.runs.borrow().is_empty(),
        "nothing was shaped, so the panicking measure was never in the way"
    );
}

/// The other half of the same rule: a provider that is **not** a shaper is
/// untouched, and every existing caller is one.
#[test]
fn a_provider_with_no_shaper_still_measures_a_character_at_a_time() {
    assert!(
        FixedPitch::COURIER.shaper().is_none(),
        "the default `shaper` answer changed, and every provider that never \
         heard of shaping would now be asked to do it"
    );
    let laid = layout(
        &tree("the fifth finger"),
        &FixedPitch::COURIER,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("a paragraph of Latin lays out");
    assert_eq!(laid.text(), "the fifth finger");
}

/// And the reason the rule is worth a test at all: the two paths give
/// different answers for the same text, so which one measured a line is
/// visible in where it broke.
#[test]
fn the_two_paths_disagree_by_a_number_this_test_can_name() {
    let font = FontRequest {
        families: &[],
        weight: 400,
        style: tinker_pdf_css::property::FontStyle::Normal,
        size: 10.0,
    };
    let shaper = OnlyShapes::default();
    // `fi` ligates to one em; five characters that do not would be 25 points.
    let shaped = Shaper::shape(&shaper, "fifth", &font, false).advance;
    let unshaped = FixedPitch::COURIER.measure("fifth", &font);
    assert!((shaped - 25.0).abs() < 1e-9, "{shaped}");
    assert!((unshaped - 30.0).abs() < 1e-9, "{unshaped}");
    assert_ne!(
        shaped, unshaped,
        "if the two paths agreed there would be nothing for the rule to protect"
    );
}
