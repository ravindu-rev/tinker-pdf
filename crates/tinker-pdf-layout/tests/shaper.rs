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
use tinker_pdf_css::property::{Display, TextAlign, TextJustify, Visibility};
use tinker_pdf_layout::metrics::{
    FixedPitch, FontRequest, Metrics, PlacedGlyph, ShapedText, Shaper, ShapingContext, Vertical,
    CONTEXT_BYTES,
};
use tinker_pdf_layout::{layout, BoxNode, CellSpan, Content, Justified, Limits, Options, TextRun};

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
        variations: &[],
        optical_sizing: tinker_pdf_css::property::FontOpticalSizing::Auto,
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

// ---- a justified line offers its slack to the shaper -----------------------

/// A provider whose every glyph is half an em wide and which, asked to
/// justify, takes `answer` of what a run holding `marker` is offered,
/// recording every offer. Its `Metrics::advance` and `measure` panic, so a
/// line justified through it is a line no run of which was measured twice.
struct Justifies {
    marker: char,
    answer: fn(f64) -> f64,
    offers: RefCell<Vec<(String, f64)>>,
}

impl Justifies {
    fn new(marker: char, answer: fn(f64) -> f64) -> Self {
        Justifies {
            marker,
            answer,
            offers: RefCell::new(Vec::new()),
        }
    }
}

impl Shaper for Justifies {
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

    fn justify(
        &self,
        text: &str,
        _font: &FontRequest<'_>,
        _rtl: bool,
        _context: &ShapingContext<'_>,
        width: f64,
    ) -> f64 {
        self.offers.borrow_mut().push((text.to_string(), width));
        if text.contains(self.marker) {
            (self.answer)(width)
        } else {
            0.0
        }
    }
}

impl Metrics for Justifies {
    fn advance(&self, ch: char, _font: &FontRequest<'_>) -> f64 {
        panic!("{ch:?} was measured a character at a time as well as through the shaper")
    }

    fn measure(&self, text: &str, _font: &FontRequest<'_>) -> f64 {
        panic!("{text:?} was measured through Metrics as well as through the shaper")
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

/// A justified paragraph of inline runs at ten points, each with its own
/// `text-justify`. At five points a character and 110 points a line, the
/// first line of [`justified_parts`] holds `aaaa aaaa b#bb bbbb` — 95 points
/// and 15 of slack — and the rest is the last line, which is not justified.
fn justified_tree(parts: &[(&str, TextJustify)]) -> BoxNode {
    let mut style = ComputedStyle::initial();
    style.display = Display::Block;
    style.text_align = TextAlign::Justify;
    let children = parts
        .iter()
        .map(|(text, justify)| {
            let mut inline = ComputedStyle::initial();
            inline.font_size = 10.0;
            inline.text_justify = *justify;
            BoxNode {
                style: inline,
                content: Content::Text((*text).into()),
                anchor: None,
                span: CellSpan::ONE,
                marker: None,
            }
        })
        .collect();
    BoxNode {
        style,
        content: Content::Children(children),
        anchor: None,
        span: CellSpan::ONE,
        marker: None,
    }
}

/// The three runs, the middle one's `text-justify` given.
fn justified_parts(middle: TextJustify) -> [(&'static str, TextJustify); 3] {
    [
        ("aaaa aaaa ", TextJustify::Auto),
        ("b#bb bbbb ", middle),
        ("cccc cccc cccc", TextJustify::Auto),
    ]
}

/// The first line's runs: the runs on the first page whose baseline is the
/// first run's.
fn first_line(laid: &tinker_pdf_layout::Layout) -> Vec<TextRun> {
    let runs = &laid.pages[0].runs;
    let baseline = runs[0].y;
    runs.iter()
        .filter(|run| (run.y - baseline).abs() < 1e-9)
        .take_while(|run| !run.text.starts_with('c'))
        .cloned()
        .collect()
}

fn lay<M: Metrics>(tree: &BoxNode, provider: &M) -> tinker_pdf_layout::Layout {
    layout(
        tree,
        provider,
        &Options::new(110.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("the paragraph lays out")
}

/// **A justified line offers its slack to the shaper first**
/// (`docs/design/shaping.md`, *Justification*): the run under
/// `inter-character` is offered the line's fifteen points of slack, its
/// shaper takes half, the run's width takes what was taken and records the
/// offer, and the rest is spread over the line's three spaces as before. The
/// last line is offered nothing.
#[test]
fn a_justified_line_offers_its_slack_to_the_shaper_first() {
    let provider = Justifies::new('#', |width| width / 2.0);
    let laid = lay(
        &justified_tree(&justified_parts(TextJustify::InterCharacter)),
        &provider,
    );
    assert_eq!(laid.text(), "aaaa aaaa b#bb bbbb cccc cccc cccc");
    assert_eq!(
        *provider.offers.borrow(),
        vec![("b#bb bbbb".to_string(), 15.0)],
        "one offer: the inter-character run, with the whole slack"
    );
    let line = first_line(&laid);
    assert_eq!(line.len(), 2);
    let (a, b) = (&line[0], &line[1]);
    assert_eq!(a.justified, None, "an auto run is not offered");
    assert_eq!(
        b.justified,
        Some(Justified {
            offered: 15.0,
            taken: 7.5
        })
    );
    // 7.5 points over three spaces: two in the first run, one in the second.
    assert!((a.word_spacing - 2.5).abs() < 1e-9, "{}", a.word_spacing);
    assert!((b.word_spacing - 2.5).abs() < 1e-9, "{}", b.word_spacing);
    assert!((a.width - 55.0).abs() < 1e-9, "{}", a.width);
    assert!((b.width - (45.0 + 7.5 + 2.5)).abs() < 1e-9, "{}", b.width);
    assert!(
        (a.width + b.width - 110.0).abs() < 1e-9,
        "the line is filled"
    );
    assert!((b.x - (a.x + a.width)).abs() < 1e-9);
}

/// **A shaper that declines leaves the line justified by its spaces**: the
/// default answer, and an answer of nothing, set the line exactly as the
/// same paragraph under `auto` sets it.
#[test]
fn a_shaper_that_declines_leaves_the_line_justified_by_its_spaces() {
    let auto = lay(
        &justified_tree(&justified_parts(TextJustify::Auto)),
        &Justifies::new('#', |width| width),
    );
    let declining = Justifies::new('#', |_| 0.0);
    let declined = lay(
        &justified_tree(&justified_parts(TextJustify::InterCharacter)),
        &declining,
    );
    assert_eq!(declining.offers.borrow().len(), 1, "it was asked");
    assert_eq!(declined.pages[0].runs, auto.pages[0].runs);
    // `OnlyShapes` never heard of justification: the trait's default.
    let silent = lay(
        &justified_tree(&justified_parts(TextJustify::InterCharacter)),
        &OnlyShapes::default(),
    );
    let auto_silent = lay(
        &justified_tree(&justified_parts(TextJustify::Auto)),
        &OnlyShapes::default(),
    );
    assert_eq!(silent.pages[0].runs, auto_silent.pages[0].runs);
    assert!(silent.pages[0]
        .runs
        .iter()
        .all(|run| run.justified.is_none()));
    // And a provider that is no shaper justifies over the spaces too.
    let fixed = layout(
        &justified_tree(&justified_parts(TextJustify::InterCharacter)),
        &FixedPitch::COURIER,
        &Options::new(110.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect("lays out");
    assert!(fixed.pages[0]
        .runs
        .iter()
        .all(|run| run.justified.is_none()));
}

/// **A justified run is measured by one path**: the provider's `advance`
/// and `measure` panic, and a shaper that takes the whole slack still sets
/// the line — filled by the shaper alone, its spaces given nothing.
#[test]
fn a_justified_run_is_measured_by_one_path() {
    let provider = Justifies::new('#', |width| width);
    let laid = lay(
        &justified_tree(&justified_parts(TextJustify::InterCharacter)),
        &provider,
    );
    let line = first_line(&laid);
    assert_eq!(
        line[1].justified,
        Some(Justified {
            offered: 15.0,
            taken: 15.0
        })
    );
    assert!(line.iter().all(|run| run.word_spacing.abs() < 1e-12));
    assert!((line[0].width + line[1].width - 110.0).abs() < 1e-9);
}

/// The runs of a line are offered **from its logical end**, each what the
/// runs after it left: two `inter-character` runs whose shaper takes half of
/// each offer.
#[test]
fn a_lines_runs_are_offered_from_its_end() {
    let provider = Justifies::new('#', |width| width / 2.0);
    let laid = lay(
        &justified_tree(&[
            ("a#aa aaaa ", TextJustify::InterCharacter),
            ("b#bb bbbb ", TextJustify::InterCharacter),
            ("cccc cccc cccc", TextJustify::Auto),
        ]),
        &provider,
    );
    assert_eq!(
        *provider.offers.borrow(),
        vec![
            ("b#bb bbbb".to_string(), 15.0),
            ("a#aa aaaa ".to_string(), 7.5)
        ]
    );
    let line = first_line(&laid);
    assert_eq!(
        line[0].justified,
        Some(Justified {
            offered: 7.5,
            taken: 3.75
        })
    );
    assert_eq!(
        line[1].justified,
        Some(Justified {
            offered: 15.0,
            taken: 7.5
        })
    );
    assert!((line[0].width + line[1].width - 110.0).abs() < 1e-9);
}

/// An answer outside the offer is held to it: more than offered is the
/// offer, and a negative or non-finite answer is nothing.
#[test]
fn an_answer_outside_the_offer_is_held_to_it() {
    let greedy = Justifies::new('#', |width| width * 3.0);
    let laid = lay(
        &justified_tree(&justified_parts(TextJustify::InterCharacter)),
        &greedy,
    );
    assert_eq!(
        first_line(&laid)[1].justified,
        Some(Justified {
            offered: 15.0,
            taken: 15.0
        })
    );
    let auto = lay(
        &justified_tree(&justified_parts(TextJustify::Auto)),
        &Justifies::new('#', |width| width),
    );
    let answers: [fn(f64) -> f64; 3] = [|_| -4.0, |_| f64::NAN, |_| f64::INFINITY];
    for answer in answers {
        let provider = Justifies::new('#', answer);
        let laid = lay(
            &justified_tree(&justified_parts(TextJustify::InterCharacter)),
            &provider,
        );
        assert_eq!(laid.pages[0].runs, auto.pages[0].runs);
    }
}

/// **`text-justify: none` offers no opportunity**: its run's spaces take
/// none of the slack, which the other runs' spaces take; and a line none of
/// whose runs offers one is set at its start, its slack left at its end.
#[test]
fn text_justify_none_takes_none_of_the_slack() {
    let provider = Justifies::new('#', |width| width);
    let laid = lay(
        &justified_tree(&[
            ("aaaa aaaa ", TextJustify::None),
            ("b#bb bbbb ", TextJustify::InterWord),
            ("cccc cccc cccc", TextJustify::Auto),
        ]),
        &provider,
    );
    assert!(
        provider.offers.borrow().is_empty(),
        "inter-word is never offered"
    );
    let line = first_line(&laid);
    assert!(line[0].word_spacing.abs() < 1e-12, "none: no extra");
    // The second run's one space takes all fifteen points.
    assert!((line[1].word_spacing - 15.0).abs() < 1e-9);
    assert!((line[0].width + line[1].width - 110.0).abs() < 1e-9);

    let laid = lay(
        &justified_tree(&[
            ("aaaa aaaa ", TextJustify::None),
            ("b#bb bbbb ", TextJustify::None),
            ("cccc cccc cccc", TextJustify::None),
        ]),
        &provider,
    );
    let line = first_line(&laid);
    assert!(line.iter().all(|run| run.word_spacing.abs() < 1e-12));
    assert!(line[0].x.abs() < 1e-9, "set at its start");
    assert!((line[0].width + line[1].width - 95.0).abs() < 1e-9);
}
