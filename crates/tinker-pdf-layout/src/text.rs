//! `css-text-3` §4.1's white-space processing, **in both phases**.
//!
//! # Two phases, two failure modes, and one of them is invisible
//!
//! §4.1.1 is *phase I*: it happens once, over the source, and it collapses runs
//! of white space and transforms segment breaks. §4.1.2 is *phase II*: it
//! happens at line-break time, and it trims what is left at the two ends of a
//! line.
//!
//! Doing neither draws visible gaps between every inline element, which is a
//! mercy: an indented XHTML source has a newline and eight spaces between
//! `</em>` and `<em>`, and a book that renders them renders wrong in a way
//! anybody can see. Doing phase I and not phase II leaves a space hanging at
//! the start of every line after the first, which is a ragged left margin —
//! also visible. Doing phase I **per element** rather than per formatting
//! context is the one that is not visible: `<em>a </em><em> b</em>` keeps two
//! spaces where there should be one, and no reader would ever call it a bug.
//!
//! §4.1.1 is explicit about that case and it is why [`Collapser`] is a stateful
//! object threaded across runs rather than a function called on each: *"any
//! collapsible space immediately following another collapsible space — **even
//! one outside the boundary of the inline containing that space**, provided
//! both spaces are within the same inline formatting context — is collapsed"*.
//!
//! # What a segment break becomes
//!
//! Under `normal` and `nowrap` a newline in the source is a *space*, not a
//! break — which is why an XHTML paragraph written across six source lines sets
//! as one paragraph. Under `pre-line` it is a break and the spaces around it go
//! away. Under `pre` and `pre-wrap` everything is kept, tabs included.

use tinker_pdf_css::property::WhiteSpace;

/// U+200B ZERO WIDTH SPACE, whose adjacency to a segment break removes the
/// break entirely (§4.1.1).
const ZERO_WIDTH_SPACE: char = '\u{200b}';

// `css-text-3` §7.1's `tab-size` is **not** here, and the absence is this
// crate's own rule applied to itself. A `TAB_SIZE` constant with no consumer is
// exactly what `style::consume` exists to make impossible one level up: it
// would read as though preserved tabs advanced to a tab stop, and they do not.
// Under `pre` and `pre-wrap` a tab survives phase I and is then measured as one
// character of the element's font, which is wrong for a table set in a `<pre>`
// and right for everything else a reflowable book does with one. Recorded in
// the crate's `Still owed` rather than approximated by a number nobody reads.

/// Whether a value collapses white space at all.
#[must_use]
pub fn collapses(white_space: WhiteSpace) -> bool {
    matches!(
        white_space,
        WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine
    )
}

/// Whether a value preserves segment breaks as forced line breaks.
#[must_use]
pub fn preserves_breaks(white_space: WhiteSpace) -> bool {
    matches!(
        white_space,
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
    )
}

/// Whether a value allows a line to wrap at a soft break opportunity.
#[must_use]
pub fn wraps(white_space: WhiteSpace) -> bool {
    matches!(
        white_space,
        WhiteSpace::Normal | WhiteSpace::PreWrap | WhiteSpace::PreLine
    )
}

/// Phase I, §4.1.1, carried **across** the runs of one inline formatting
/// context.
///
/// One of these is made per formatting context and [`Collapser::push`] is
/// called once per text run in document order. The state it carries is one
/// bit — whether the last character emitted was a collapsible space — and that
/// bit is the whole of what makes `<em>a </em><em> b</em>` one space rather
/// than two.
#[derive(Clone, Copy, Debug, Default)]
pub struct Collapser {
    /// The last character emitted was a collapsible space, so the next one is
    /// removed.
    pending: bool,
    /// Nothing has been emitted yet, so a leading collapsible space is removed
    /// outright — §4.1.1's *"a sequence of collapsible spaces at the beginning
    /// of a line is removed"* applied at the start of the context, where there
    /// is no line yet to trim.
    started: bool,
}

impl Collapser {
    /// A fresh collapser for one inline formatting context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One text run through phase I.
    pub fn push(&mut self, source: &str, white_space: WhiteSpace) -> String {
        let mut out = String::with_capacity(source.len());
        if !collapses(white_space) {
            // `pre` and `pre-wrap`: everything survives, tabs included. A run
            // that preserves resets the collapser, because a space after
            // preserved text is not following a *collapsible* one.
            for ch in source.chars() {
                out.push(ch);
            }
            self.pending = false;
            self.started |= !source.is_empty();
            return out;
        }
        let preserve_breaks = preserves_breaks(white_space);
        let chars: Vec<char> = source.chars().collect();
        for (index, &ch) in chars.iter().enumerate() {
            match ch {
                // §4.1.1: a carriage return is part of a segment break and a
                // tab is a space. Both are folded here so that everything
                // below has one kind of collapsible space to think about.
                '\r' => {
                    if chars.get(index + 1) == Some(&'\n') {
                        continue;
                    }
                    self.segment_break(&mut out, preserve_breaks, &chars, index);
                }
                '\n' => self.segment_break(&mut out, preserve_breaks, &chars, index),
                ' ' | '\t' => {
                    if self.pending || !self.started {
                        continue;
                    }
                    out.push(' ');
                    self.pending = true;
                }
                other => {
                    out.push(other);
                    self.pending = false;
                    self.started = true;
                }
            }
        }
        out
    }

    /// One segment break, transformed per §4.1.1's segment break
    /// transformation rules.
    fn segment_break(&mut self, out: &mut String, preserve: bool, chars: &[char], index: usize) {
        if preserve {
            // `pre-line`. A collapsible space *before* the break has already
            // been emitted, so it is taken back off: §4.1.1 removes spaces
            // around a preserved break rather than leaving one hanging.
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
            self.pending = false;
            // A space after the break is a leading space on the next line and
            // is removed, which is what `started = false` means here.
            self.started = false;
            return;
        }
        // §4.1.1: *"if the character immediately before or immediately after
        // the segment break is the zero-width space, the segment break is
        // removed entirely"*. It is the one rule here that joins two words
        // rather than separating them, and a build without it turns a
        // deliberately invisible break opportunity into a visible space.
        let before = out.chars().next_back();
        let after = chars.get(index + 1).copied();
        if before == Some(ZERO_WIDTH_SPACE) || after == Some(ZERO_WIDTH_SPACE) {
            return;
        }
        if self.pending || !self.started {
            return;
        }
        out.push(' ');
        self.pending = true;
    }
}

/// Phase II, §4.1.2, at the two ends of one line.
///
/// It is a separate function from [`Collapser`] and takes a `&str` rather than
/// being folded into the collapser, because it happens at a **different time**:
/// phase I runs once over the source and phase II runs once per line, and a
/// build that did phase II's trimming during phase I would remove the space
/// between two words that happened to straddle a line break in the source.
///
/// Returns the byte range of the line's text that is actually set. The
/// trailing space is *hung* rather than deleted — it keeps its position in the
/// conserved character stream and contributes no width — which is why this
/// answers a range rather than a `String`.
#[must_use]
pub fn trim_line(text: &str, white_space: WhiteSpace) -> (usize, usize) {
    if !collapses(white_space) {
        // `pre` and `pre-wrap` preserve both ends. §4.1.2's *"hanging"* of a
        // preserved trailing space is a refinement this build does not make,
        // and the value it applies to is one no reflowable book sets on a
        // paragraph.
        return (0, text.len());
    }
    let start = text.len() - text.trim_start_matches(' ').len();
    let end = text.trim_end_matches(' ').len();
    (start, start.max(end))
}
