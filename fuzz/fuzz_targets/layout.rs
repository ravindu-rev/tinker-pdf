//! Layout: the box model, margin collapsing, line breaking and fragmentation.
//!
//! The repository's **twenty-fourth** target, and the first whose input is not
//! bytes. Gap 31 argued for two crates rather than one partly on this: `css` is
//! a **parser** — untrusted bytes, a byte corpus, a target in the shape `xml`
//! and `zip_archive` established — and `layout` is an **algorithm**, whose
//! inputs are already-validated structures and whose failure mode is unbounded
//! work rather than a panic on a malformed field. *"A merged crate gets one
//! target that can only exercise the first, and the second is where the
//! quadratic blowups live."*
//!
//! So the body is a **structured generator**: the bytes name a tree of boxes,
//! their styles and their text, and the layout engine is driven over it. A
//! target that handed these bytes to a parser would spend its whole session
//! being refused at the door.
//!
//! The control byte picks the **bounds** rather than the input, for gap 18a
//! milestone 8's reason and `css`'s: a target whose limits are all at their
//! shipped defaults never explores a refusal, because a two-hundred-thousand
//! box cap cannot fire inside one iteration.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **Text is conserved.** Every non-whitespace character of the tree appears
//!   on some page, exactly once, in document order — with the text under a
//!   `display: none` excluded, which is the one legitimate way to lose some.
//!   This is gap 31's fourth honesty device and the invariant that survives
//!   every level of CSS partiality: a float that pushed content off the page
//!   bottom, a fragmentation bug that repeated a paragraph, a spine item that
//!   was skipped — every one of them renders beautifully and every one of them
//!   fails here.
//! - **Every budget holds rather than being exceeded and reported.** Boxes,
//!   layout operations and break evaluations, each against the cap the control
//!   byte chose rather than against the shipped one.
//! - **Every number that reaches a page is finite.** A `NaN` width propagates
//!   silently through every comparison it touches and comes out as a page of
//!   nothing.
//! - **Pagination terminates and makes progress.** A page cap that fires is a
//!   refusal; a fragmenter that cut at a position it had already passed would
//!   hang, and libFuzzer's `-timeout` is what reports that.
//! - **Layout is deterministic**, which is ruling 4 asserted on an algorithm
//!   rather than on a rendered page — and it is not free here, because the
//!   warning set is built in a hash map before it is sorted.
//! - **UAX #14's own shape.** Break opportunities are strictly increasing, land
//!   on character boundaries, and end at the end of the text.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{
    BorderStyle, BoxSizing, Clear, Display, Float, LengthPercentage, LineBreakStrictness,
    LineHeight, ListStyleType, MarginValue, OverflowWrap, PageBreak, PageBreakInside, Sides, Size,
    TextAlign, Visibility, WhiteSpace, WordBreak,
};
use tinker_pdf_layout::metrics::FixedPitch;
use tinker_pdf_layout::uax14::{opportunities, Tailoring};
use tinker_pdf_layout::{layout_with, BoxNode, Budget, Content, Limits, Options};

/// The alphabet the generator writes text out of.
///
/// Not `from_utf8_lossy` over the input, and the difference is the point: a
/// mutator that had to discover valid UTF-8 by chance would spend its session
/// on the decoder. These are the characters the line breaker's rules are
/// **about** — an ideograph, a small kana whose class the tailoring decides, a
/// no-break space, a word joiner, a zero-width space, a joiner, a soft hyphen,
/// an opening bracket that must not end a line and a full-width one that may.
const ALPHABET: [char; 24] = [
    'a', 'b', ' ', ' ', '\n', '\t', '-', '.', '0', '9', '(', ')', '\u{6771}', '\u{4eac}',
    '\u{3041}', '\u{3001}', '\u{ff08}', '\u{a0}', '\u{2060}', '\u{200b}', '\u{200d}', '\u{2010}',
    '\u{05d0}', '\u{1f469}',
];

/// The bytes, one at a time, wrapping when they run out.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn next(&mut self) -> u8 {
        if self.data.is_empty() {
            return 0;
        }
        let byte = self.data[self.at % self.data.len()];
        self.at += 1;
        byte
    }

    fn spent(&self) -> bool {
        self.at >= self.data.len()
    }
}

/// A style built out of four bytes.
fn style(bytes: &mut Bytes<'_>, block: bool) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    let a = bytes.next();
    let b = bytes.next();
    let c = bytes.next();
    let d = bytes.next();
    style.font_size = 4.0 + f64::from(a & 15);
    style.display = if block {
        match a >> 6 {
            0 => Display::Block,
            1 => Display::ListItem,
            2 => Display::InlineBlock,
            _ => Display::Block,
        }
    } else {
        match a >> 6 {
            0 => Display::None,
            _ => Display::Inline,
        }
    };
    let margin = |value: u8| {
        MarginValue::Length(LengthPercentage::Px(f64::from(value as i8) / 4.0))
    };
    style.margin = Sides {
        top: margin(b),
        right: margin(c),
        bottom: margin(c),
        left: margin(b),
    };
    if b & 1 != 0 {
        style.margin.left = MarginValue::Auto;
        style.margin.right = MarginValue::Auto;
    }
    style.padding = Sides::all(LengthPercentage::Px(f64::from(c & 7)));
    style.border_width = Sides::all(f64::from(d & 3));
    style.border_style = Sides::all(if d & 4 != 0 {
        BorderStyle::Solid
    } else {
        BorderStyle::None
    });
    style.box_sizing = if d & 8 != 0 {
        BoxSizing::BorderBox
    } else {
        BoxSizing::ContentBox
    };
    style.width = if b & 2 != 0 {
        Size::Length(LengthPercentage::Percent(f64::from(c) / 2.0))
    } else {
        Size::Auto
    };
    style.height = if b & 4 != 0 {
        Size::Length(LengthPercentage::Px(f64::from(c)))
    } else {
        Size::Auto
    };
    style.line_height = match d >> 6 {
        0 => LineHeight::Normal,
        1 => LineHeight::Number(1.0 + f64::from(c & 3)),
        _ => LineHeight::Px(f64::from(c & 63)),
    };
    style.text_align = match c >> 6 {
        0 => TextAlign::Left,
        1 => TextAlign::Right,
        2 => TextAlign::Center,
        _ => TextAlign::Justify,
    };
    style.text_indent = LengthPercentage::Px(f64::from(d as i8));
    style.white_space = match (b >> 3) & 7 {
        0 | 1 | 2 => WhiteSpace::Normal,
        3 => WhiteSpace::NoWrap,
        4 => WhiteSpace::Pre,
        5 => WhiteSpace::PreWrap,
        _ => WhiteSpace::PreLine,
    };
    style.page_break_before = match (d >> 4) & 3 {
        0 | 1 => PageBreak::Auto,
        2 => PageBreak::Always,
        _ => PageBreak::Avoid,
    };
    style.page_break_after = match (c >> 4) & 3 {
        0 | 1 => PageBreak::Auto,
        2 => PageBreak::Always,
        _ => PageBreak::Avoid,
    };
    style.page_break_inside = if a & 16 != 0 {
        PageBreakInside::Avoid
    } else {
        PageBreakInside::Auto
    };
    style.orphans = 1 + u16::from(a & 3);
    style.widows = 1 + u16::from(b & 3);
    style.overflow_wrap = match (a >> 2) & 3 {
        0 | 1 => OverflowWrap::Normal,
        2 => OverflowWrap::BreakWord,
        _ => OverflowWrap::Anywhere,
    };
    style.line_break = match (b >> 6) & 3 {
        0 => LineBreakStrictness::Auto,
        1 => LineBreakStrictness::Loose,
        2 => LineBreakStrictness::Strict,
        _ => LineBreakStrictness::Anywhere,
    };
    style.word_break = match (c >> 2) & 3 {
        0 | 1 => WordBreak::Normal,
        2 => WordBreak::BreakAll,
        _ => WordBreak::KeepAll,
    };
    style.list_style_type = match (d >> 2) & 3 {
        0 => ListStyleType::Disc,
        1 => ListStyleType::Decimal,
        2 => ListStyleType::LowerRoman,
        _ => ListStyleType::None,
    };
    style.visibility = if a & 32 != 0 {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    style.float = match (d >> 4) & 3 {
        1 => Float::Left,
        2 => Float::Right,
        _ => Float::None,
    };
    style.clear = match (c >> 4) & 3 {
        1 => Clear::Left,
        2 => Clear::Both,
        _ => Clear::None,
    };
    style
}

/// A subtree. The generator's own depth is capped well below the layout crate's
/// so that a hostile input exercises the crate's cap rather than this file's
/// stack.
fn node(bytes: &mut Bytes<'_>, depth: usize) -> BoxNode {
    let shape = bytes.next();
    let block = depth == 0 || shape & 1 != 0;
    let style = style(bytes, block);
    if depth >= 12 || shape & 2 != 0 || bytes.spent() {
        let count = usize::from(shape >> 3) + 1;
        let mut text = String::new();
        for _ in 0..count {
            text.push(ALPHABET[usize::from(bytes.next()) % ALPHABET.len()]);
        }
        return BoxNode::text(style, text);
    }
    let count = usize::from((shape >> 2) & 7) + 1;
    let mut children = Vec::new();
    for _ in 0..count {
        children.push(node(bytes, depth + 1));
        if bytes.spent() {
            break;
        }
    }
    BoxNode::element(style, children)
}

/// The text a correct layout must produce: everything but what a
/// `display: none` removed, with the white space taken out.
///
/// A second walk of the tree rather than a call to `BoxNode::source_text`, and
/// deliberately so: the crate's own function includes the hidden subtrees,
/// because the *facade* compares against the spine and needs it to. Here the
/// comparison is against what layout produced, so the one legitimate way to
/// lose text has to be modelled — and modelling it in six lines beside the
/// assertion is what keeps the assertion an equality.
fn expected(node: &BoxNode, out: &mut String) {
    if node.style.display == Display::None {
        return;
    }
    match &node.content {
        Content::Text(text) => out.extend(text.chars().filter(|c| !c.is_whitespace())),
        Content::Children(children) => {
            for child in children {
                expected(child, out);
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that every cap is crossable inside one iteration, and varied
    // enough that both sides of each are reachable from one corpus.
    let (max_depth, max_boxes) = match knobs & 3 {
        0 => (2, 4),
        1 => (8, 32),
        2 => (64, 4_096),
        _ => (256, 262_144),
    };
    let max_break_work = match (knobs >> 2) & 3 {
        0 => 8,
        1 => 128,
        2 => 8_192,
        _ => 4_000_000,
    };
    let max_pages = match (knobs >> 4) & 3 {
        0 => 1,
        1 => 4,
        2 => 256,
        _ => 65_536,
    };
    // Milestone 10's third work total. The small values are the interesting
    // ones: a book of floats past this refuses, and the assertion that matters
    // is that it refuses **by name** rather than by running for ever.
    let max_layout_work = match (knobs >> 8) & 3 {
        0 => 0,
        1 => 64,
        2 => 4_096,
        _ => 4_000_000,
    };
    let (width, height) = match (knobs >> 6) & 3 {
        // A page one point wide is where `overflow-wrap`, the last-resort
        // character break and the "nothing fits" warning all live.
        0 => (1.0, 1.0),
        1 => (12.0, 20.0),
        2 => (200.0, 60.0),
        _ => (432.0, 648.0),
    };
    let limits = Limits {
        max_depth,
        max_boxes,
        max_break_work,
        max_layout_work,
        max_pages,
    };

    let mut bytes = Bytes { data: body, at: 0 };
    let tree = node(&mut bytes, 0);
    let options = Options::new(width, height);
    let metrics = FixedPitch::COURIER;

    let mut budget = Budget::new(&limits);
    let Ok(laid) = layout_with(&tree, &metrics, &options, &limits, &mut budget) else {
        return;
    };

    assert!(budget.boxes() <= limits.max_boxes, "the box total was exceeded rather than refused");
    assert!(
        budget.breaks() <= limits.max_break_work,
        "the break total was exceeded rather than refused"
    );
    assert!(
        budget.layout() <= limits.max_layout_work,
        "the float total was exceeded rather than refused"
    );
    assert!(
        laid.pages.len() <= limits.max_pages,
        "the page cap was exceeded rather than refused"
    );
    assert!(!laid.pages.is_empty(), "a book laid out into no pages at all");

    // Gap 31's fourth honesty device, on trees nobody wrote.
    let mut wanted = String::new();
    expected(&tree, &mut wanted);
    let got: String = laid.text().chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        got == wanted,
        "text conservation failed: {} characters in, {} out",
        wanted.chars().count(),
        got.chars().count()
    );

    for page in &laid.pages {
        for run in &page.runs {
            assert!(run.x.is_finite(), "a run's x is not finite");
            assert!(run.y.is_finite(), "a run's y is not finite");
            assert!(run.width.is_finite(), "a run's width is not finite");
            assert!(run.font_size.is_finite() && run.font_size > 0.0);
        }
        for fragment in &page.boxes {
            assert!(fragment.x.is_finite() && fragment.y.is_finite());
            assert!(fragment.width.is_finite() && fragment.width >= 0.0);
            assert!(fragment.height.is_finite() && fragment.height >= 0.0);
        }
    }

    // Ruling 10's shape: one entry per warning, with a count.
    for (index, (warning, count)) in laid.warnings.iter().enumerate() {
        assert!(*count > 0, "a warning was recorded zero times");
        assert!(
            !laid.warnings[..index]
                .iter()
                .any(|(earlier, _)| earlier == warning),
            "a warning was recorded twice instead of counted"
        );
    }

    // Ruling 4, on the algorithm. The warning list is built in a hash map and
    // sorted afterwards, so this is a real assertion rather than a tautology.
    let mut again_budget = Budget::new(&limits);
    let again = layout_with(&tree, &metrics, &options, &limits, &mut again_budget)
        .expect("the same tree refused on a second run");
    assert!(again.warnings == laid.warnings, "layout is not deterministic");
    assert_eq!(again.pages.len(), laid.pages.len());
    assert_eq!(again.text(), laid.text());

    // UAX #14's own shape, over the text this tree happens to hold.
    let source = tree.source_text();
    for tailoring in [
        Tailoring::default(),
        Tailoring::UAX14,
        Tailoring {
            strictness: LineBreakStrictness::Anywhere,
            word_break: WordBreak::Normal,
        },
    ] {
        let at = opportunities(&source, tailoring);
        let mut previous = 0usize;
        for opportunity in &at {
            assert!(opportunity.at > previous, "break opportunities are not increasing");
            assert!(
                source.is_char_boundary(opportunity.at),
                "a break opportunity is inside a character"
            );
            previous = opportunity.at;
        }
        if !source.is_empty() {
            assert_eq!(
                at.last().map(|o| o.at),
                Some(source.len()),
                "the end of the text is not a break opportunity"
            );
        }
    }
});
