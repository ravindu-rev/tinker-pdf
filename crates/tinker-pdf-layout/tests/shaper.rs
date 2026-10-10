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

use std::cell::{Cell, RefCell};

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{Display, Visibility};
use tinker_pdf_layout::metrics::{
    FixedPitch, FontRequest, Metrics, PlacedGlyph, ShapedText, Shaper, ShapingContext, Vertical,
    CONTEXT_BYTES,
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
            marker: None,
        }]),
        anchor: None,
        span: CellSpan::ONE,
        marker: None,
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
        kerning: tinker_pdf_css::property::FontKerning::Auto,
        features: &[],
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

// ---- a run measured in its context ------------------------------------------

/// A provider that kerns one pair: every glyph half an em, and an `A` a fifth
/// of an em narrower when the text after it — its own or its context's —
/// starts with `V`. The smallest thing whose answer depends on a neighbour.
struct Kerns;

impl Kerns {
    fn kerned(text: &str, after: Option<&str>, size: f64) -> ShapedText {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut glyphs = Vec::new();
        let mut advance = 0.0;
        for (k, (at, ch)) in chars.iter().enumerate() {
            let next = chars
                .get(k + 1)
                .map(|(_, c)| *c)
                .or_else(|| after.and_then(|a| a.chars().next()));
            let width = if *ch == 'A' && next == Some('V') {
                size * 0.3
            } else {
                size * 0.5
            };
            glyphs.push(PlacedGlyph {
                glyph: 1,
                cluster: u32::try_from(*at).unwrap_or(0),
                x_advance: width,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
            });
            advance += width;
        }
        ShapedText {
            glyphs,
            advance,
            rtl: false,
        }
    }
}

impl Shaper for Kerns {
    fn shape(&self, text: &str, font: &FontRequest<'_>, _rtl: bool) -> ShapedText {
        Kerns::kerned(text, None, font.size)
    }

    fn shape_in(
        &self,
        text: &str,
        font: &FontRequest<'_>,
        _rtl: bool,
        context: &ShapingContext<'_>,
    ) -> ShapedText {
        Kerns::kerned(text, context.after.map(|n| n.text), font.size)
    }
}

impl Metrics for Kerns {
    fn advance(&self, ch: char, _font: &FontRequest<'_>) -> f64 {
        panic!("{ch:?} was measured a character at a time")
    }

    fn vertical(&self, font: &FontRequest<'_>) -> Vertical {
        Vertical {
            ascent: font.size * 0.8,
            descent: font.size * 0.2,
        }
    }

    fn shaper(&self) -> Option<&dyn Shaper> {
        Some(self)
    }
}

/// `A` and then an inline box holding `V`, at 10 px: two pieces, so two runs.
fn kerned_pair(visible: bool) -> BoxNode {
    let mut block = ComputedStyle::initial();
    block.display = Display::Block;
    let mut text = ComputedStyle::initial();
    text.font_size = 10.0;
    let mut span = text.clone();
    span.display = Display::Inline;
    if !visible {
        span.visibility = Visibility::Hidden;
    }
    let leaf = |style: &ComputedStyle, s: &str| BoxNode {
        style: style.clone(),
        content: Content::Text(s.into()),
        anchor: None,
        span: CellSpan::ONE,
        marker: None,
    };
    BoxNode {
        style: block,
        content: Content::Children(vec![
            leaf(&text, "A"),
            BoxNode {
                style: span.clone(),
                content: Content::Children(vec![leaf(&span, "V")]),
                anchor: None,
                span: CellSpan::ONE,
                marker: None,
            },
        ]),
        anchor: None,
        span: CellSpan::ONE,
        marker: None,
    }
}

/// **A run is measured with its neighbour beside it**, through
/// `Shaper::shape_in`: `A` is three points wide because the next run starts
/// with `V`, and the `V` run starts there.
#[test]
fn a_run_is_measured_in_the_context_of_its_neighbour() {
    let laid = layout(
        &kerned_pair(true),
        &Kerns,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("two runs lay out");
    let runs = &laid.pages[0].runs;
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!((runs[0].width - 3.0).abs() < 1e-9, "{runs:?}");
    assert!(
        (runs[1].x - (runs[0].x + 3.0)).abs() < 1e-9,
        "the V run was not placed after the kerned A: {runs:?}"
    );
}

/// **A neighbour that is not painted is no context**: the painter shapes a
/// run against what it draws, so text laid out and not drawn kerns nothing.
#[test]
fn hidden_text_is_no_one_s_context() {
    let laid = layout(
        &kerned_pair(false),
        &Kerns,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("two runs lay out");
    let runs = &laid.pages[0].runs;
    assert!((runs[0].width - 5.0).abs() < 1e-9, "{runs:?}");
}

// ---- how much of a neighbour a shaper is handed -------------------------------

/// A provider that keeps count of the context it is handed: the longest
/// neighbour either side, how many slices had none before them, how many
/// `before` neighbours filled the window, and how many neighbours were not
/// the near end of the text beside the slice. Counts rather than copies,
/// since the defect this watches for hands over tens of kilobytes a call.
#[derive(Default)]
struct Records {
    /// The paragraph, so a short `before` can be checked to be its start.
    paragraph: String,
    calls: Cell<usize>,
    longest: Cell<usize>,
    alone: Cell<usize>,
    filled: Cell<usize>,
    far: Cell<usize>,
}

impl Shaper for Records {
    fn shape(&self, text: &str, font: &FontRequest<'_>, rtl: bool) -> ShapedText {
        let glyphs: Vec<PlacedGlyph> = text
            .char_indices()
            .map(|(at, _)| PlacedGlyph {
                glyph: 1,
                cluster: u32::try_from(at).unwrap_or(0),
                x_advance: font.size / 2.0,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
            })
            .collect();
        let advance = glyphs.len() as f64 * font.size / 2.0;
        ShapedText {
            glyphs,
            advance,
            rtl,
        }
    }

    fn shape_in(
        &self,
        text: &str,
        font: &FontRequest<'_>,
        rtl: bool,
        context: &ShapingContext<'_>,
    ) -> ShapedText {
        self.calls.set(self.calls.get() + 1);
        for n in [context.before, context.after].into_iter().flatten() {
            self.longest.set(self.longest.get().max(n.text.len()));
        }
        if context.before.is_none() {
            self.alone.set(self.alone.get() + 1);
        }
        if let Some(before) = context.before {
            if before.text.len() + 3 >= CONTEXT_BYTES {
                self.filled.set(self.filled.get() + 1);
            } else if !self.paragraph.starts_with(before.text) {
                // Shorter than the window, so it is all of the line before
                // the slice, and this line starts the paragraph.
                self.far.set(self.far.get() + 1);
            }
            if !self.touch(before.text.chars().next_back(), text.chars().next()) {
                self.far.set(self.far.get() + 1);
            }
        }
        if let Some(after) = context.after {
            if !self.touch(text.chars().next_back(), after.text.chars().next()) {
                self.far.set(self.far.get() + 1);
            }
        }
        self.shape(text, font, rtl)
    }
}

impl Records {
    /// Whether `left` then `right` is a pair the paragraph holds — which a
    /// neighbour's far end and the slice are not: the line's first 64 bytes
    /// end in `a`, and no `a` is followed by an `a` or a space.
    fn touch(&self, left: Option<char>, right: Option<char>) -> bool {
        let (Some(left), Some(right)) = (left, right) else {
            return false;
        };
        self.paragraph.contains(&format!("{left}{right}"))
    }
}

impl Metrics for Records {
    fn advance(&self, ch: char, _font: &FontRequest<'_>) -> f64 {
        panic!("{ch:?} was measured a character at a time")
    }

    fn vertical(&self, font: &FontRequest<'_>) -> Vertical {
        Vertical {
            ascent: font.size * 0.8,
            descent: font.size * 0.2,
        }
    }

    fn shaper(&self) -> Option<&dyn Shaper> {
        Some(self)
    }
}

/// **A shaper is handed the near end of a neighbour, never all of it**
/// (review of lane 8C).
///
/// Before a slice its neighbour is everything on the line so far, and a
/// slice is measured at every break opportunity. Handed over whole, that
/// made the cost of filling a line rest on every provider reading only the
/// near end of it, and the EPUB provider once counted it instead —
/// `O(characters²)` a line, and a paragraph at a tiny `font-size` is one
/// line. So `flow.rs` hands over at most [`CONTEXT_BYTES`], cut back to a
/// character boundary, and no provider can.
///
/// Held by what the provider is handed, not by a clock: 10 000 words of
/// `aé€ ` — one, two, three and one bytes, so a cut that is not at a
/// character boundary is met — set on one line at a hundredth of a point.
/// Every neighbour is at most the window; every slice but the line's first
/// has one before it; a `before` is the window filled, or the whole start
/// of the line; and each is the near end — its character
/// next to the slice and the slice's own next to it are a pair the paragraph
/// holds.
#[test]
fn a_shaper_is_handed_the_near_end_of_a_neighbour_and_never_all_of_it() {
    let paragraph = "a\u{e9}\u{20ac} ".repeat(10_000);
    let mut node = tree(&paragraph);
    if let Content::Children(children) = &mut node.content {
        children[0].style.font_size = 0.01;
    }
    let provider = Records {
        paragraph: paragraph.clone(),
        ..Records::default()
    };
    let laid = layout(
        &node,
        &provider,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("one long line lays out");
    let runs = &laid.pages[0].runs;
    assert_eq!(runs.len(), 1, "the paragraph is not one line");
    // Its last space hangs at the line's end, and is in no run.
    assert!(
        runs[0].text == paragraph.trim_end(),
        "the line holds {} bytes of the paragraph's {}",
        runs[0].text.len(),
        paragraph.len()
    );
    assert!(provider.calls.get() > 10_000, "{}", provider.calls.get());
    // The line's first slice, and the line's run measured whole once the
    // line is set, have nothing before them; every other slice has the
    // line so far, and a cut inside a character would have left it none.
    assert!(
        provider.alone.get() <= 2,
        "{} of {} slices were measured with nothing before them",
        provider.alone.get(),
        provider.calls.get()
    );
    assert!(
        provider.longest.get() <= CONTEXT_BYTES,
        "a neighbour of {} bytes was handed over",
        provider.longest.get()
    );
    assert!(
        provider.filled.get() > 9_000,
        "only {} of {} neighbours before a slice filled the window",
        provider.filled.get(),
        provider.calls.get()
    );
    assert_eq!(provider.far.get(), 0, "a neighbour was not its near end");
}
