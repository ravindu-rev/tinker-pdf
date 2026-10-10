//! Ruling 14: extracted text is reported in **logical** order.
//!
//! A page draws its glyphs in *visual* order — left to right along the
//! baseline — and a right-to-left script is read the other way. So the
//! characters `tinker-pdf-content`'s `TextDevice` collects in content-stream
//! order are, for a producer that draws visually, an Arabic or Hebrew line
//! read backwards; and for one that draws each glyph in reading order with the
//! pen moving left, they are already right. [`into_logical_order`] answers
//! both the same way, because it never trusts the stream's order for a line
//! that holds a right-to-left character — including for where the line ends:
//!
//! 0. **A line the stream's order cut is one line again.** A right-to-left
//!    word drawn in reading order as several text objects, inside a
//!    left-to-right line, reaches `TextDevice` as pieces on one baseline,
//!    since a line resumes after an `ET` only where it stopped; consecutive
//!    lines that hold a right-to-left character and meet end to end on one
//!    baseline are joined ([`rejoin_split_lines`]).
//! 1. **Marks are kept with their base.** A character whose first code point
//!    is a nonspacing mark (`Bidi_Class` `NSM`) belongs to the base glyph it
//!    sits on — the neighbour, in content order, whose extent along the
//!    baseline holds the mark's centre, or failing that the nearer one. Which
//!    side of its base a producer wrote a mark on is a producer's habit, and
//!    the pairing has to survive both habits.
//! 2. **The line is put in visual order**, by where each base glyph starts
//!    along the baseline. A stable sort, so glyphs a producer overprinted at
//!    one position keep the order it wrote them in.
//! 3. **The visual line is read back through UAX #9**, by
//!    [`tinker_pdf_shape::bidi::logical_order`]: the order whose text, drawn
//!    by the algorithm, is the line as it stands. Not L2 applied to levels
//!    resolved over the drawn line — those are other levels, because W2, W5
//!    and W7 look backwards for a strong character and N1 at both neighbours,
//!    and a `%` drawn beside Arabic digits would join them. Every order is
//!    checked forwards before it is used.
//! 4. **The paragraph direction is read off the drawn line**
//!    ([`tinker_pdf_shape::bidi::drawn_direction`]): P2's first strong
//!    character is the leftmost strong one for a left-to-right paragraph and
//!    the rightmost for a right-to-left one, so a line whose two ends agree is
//!    that direction, and a line whose ends disagree goes by its majority.
//!    Classes are `Bidi_Class`, so N'Ko and Adlam read right to left like
//!    Hebrew. **One tie-break** (the ruling as amended 10 October 2026): a
//!    line with no left-to-right strong character whose leftmost unit is a
//!    strong right-to-left character and whose rightmost is punctuation
//!    (`CS`, `ON`, `ES` or `ET`, but not an opening bracket or quotation
//!    mark; whitespace and invisible format characters passed over) reads
//!    as a left-to-right paragraph **if a left-to-right paragraph draws
//!    it** — a Hebrew or Arabic word quoted in left-to-right
//!    text with its comma after it, `חו,` drawn `וח,`, which both paragraphs
//!    draw alike and which read right to left came back `,חו`. Where no
//!    left-to-right reading draws the line, it stays right to left.
//!    [`TextLine::rtl`] is set to the direction read, for every line this
//!    touches.
//!
//! A line with no right-to-left character is left exactly as it was
//! collected, byte for byte: no sort, no reorder. That is what keeps every
//! left-to-right page this engine has ever extracted unchanged by the ruling,
//! and it is asserted rather than assumed.
//!
//! # What it does not undo
//!
//! - **Mirroring (rule L4).** A right-to-left `(` is drawn with the glyph of
//!   `)`, and whether a producer's `/ToUnicode` names the character it stands
//!   for or the glyph's own shape is not recoverable from the line. This
//!   engine's own writers name the character, so nothing is swapped.
//! - **Texts UAX #9 draws alike.** The algorithm is not one-to-one: in a
//!   right-to-left paragraph `שלום 2026 now` and `שלום now 2026` are one
//!   picture, and this reads it as the second. `logical_order` states which
//!   one it returns; every answer draws the line as drawn.
//! - **Bracket pairs.** Rule N0 pairs brackets in the logical text, and a
//!   right-to-left run draws them mirrored, so a line holding a bracket pair
//!   can read back as another text the same picture could be:
//!   `BidiCharacterTest.txt` has 669 such cases of 91 616, and no others.
//! - **The paragraph.** A line is resolved on its own. A line of a
//!   right-to-left paragraph that begins and ends with Latin reads as a
//!   left-to-right one, and its runs come back each in the right order but
//!   placed as a left-to-right paragraph would place them. And a line of a
//!   right-to-left paragraph that holds nothing left to right and *opens*
//!   with a mark of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is not `Ps` or
//!   `Pi` — a dash (`— שלום` drawn `םולש —`), a bullet, `*`, `#`, `%`, an
//!   ellipsis, or a straight or closing-form quotation mark used to open — is
//!   the tie-break's shape, a left-to-right paragraph draws it too, and it
//!   reads with the mark at its end, `שלום —`: the price of the tie-break,
//!   chosen because a quoted word's trailing punctuation is far commoner
//!   (`text_logical_order.rs` pins it by name). An opening bracket or
//!   quotation mark does not pay where `/ToUnicode` names the typed character,
//!   `(١) بند`, `“שלום` (where it names the mirrored glyph, `»` for `«`, the
//!   line ends in a closing mark and pays like a dash); nor does a mark a
//!   European number follows, `(1) פריט`, `— 2026 שלום`, `• 5 תפוחים`,
//!   since no left-to-right paragraph draws one next to the mark.
//!   Arabic-Indic digits (`AN`) do not spare it, `— ١ بند` reading
//!   `١ بند —`, nor does a number further on, `— שלום 5 חו`.
//! - **Vertical lines**, which UAX #9 does not describe.
//!
//! [`crate::Page::text_with`] with [`TextOptions::content_order`] is the
//! opt-out: the characters in the order the content stream showed them, as
//! `TextDevice` collected them.

use tinker_pdf_content::text::RESUME_TIE;
use tinker_pdf_content::{Quad, TextChar, TextLine, TextPage, WritingMode};
use tinker_pdf_shape::bidi::{drawn_direction, logical_order, BaseDirection};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

/// How [`crate::Page::text_with`] orders a page's lines.
///
/// `Default` is [`crate::Page::text`]'s behaviour, to the byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextOptions {
    /// Report every line's characters in the order the content stream showed
    /// them, rather than in logical order (ruling 14).
    ///
    /// For a caller that needs the stream's own order — a redaction tool
    /// matching operators, a diff of two producers' output — and for nothing
    /// that reads the text as text.
    pub content_order: bool,
}

/// Puts every line of `page` into logical order, and returns how many lines it
/// changed.
pub(crate) fn into_logical_order(page: &mut TextPage) -> usize {
    let mut changed = 0usize;
    for block in &mut page.blocks {
        changed += rejoin_split_lines(&mut block.lines);
        for line in &mut block.lines {
            if logical_line(line) {
                changed += 1;
            }
        }
    }
    changed
}

/// Joins consecutive lines of one block that the content stream's order split
/// and that are one line on the page: on one baseline, end to end, with a
/// right-to-left character in either. Returns how many joins it made.
///
/// # Why a line drawn in reading order can arrive in pieces
///
/// `TextDevice` resumes a line after an `ET` only where the last glyph
/// stopped, give or take half an em — which is what keeps two cells of a table
/// row two lines. A producer that draws a right-to-left word in reading order
/// moves its pen left, and when that word is several text objects inside a
/// left-to-right line — `a ب<span>ح</span>م b`, each span its own object — the
/// object drawn first is the rightmost: `a ` ends at one place and `ب` starts
/// two letters further on, so the line was cut there, and again after `م`,
/// which ends at the word's left edge while ` b` starts at its right. Three
/// lines on one baseline came back, and the reading order below, which never
/// trusts the stream's order, was handed three lines to order instead of one.
/// Ruling 14 says the content stream's order decides nothing; this is that
/// sentence applied to where a line ends as well as to how one is read.
///
/// So two lines in a row are one when their baselines agree to half an em and
/// their extents along it meet to within the same half em the device's own
/// rule allows. A line with no right-to-left character beside a line with
/// none is never joined — a left-to-right page is collected exactly as it
/// was — and neither is a vertical one.
///
/// # And two lines on top of each other are two drawings
///
/// **Meeting is not overlapping.** The pieces of a split line lie end to end,
/// touching or a kerned hair into each other; text overdrawn in a second text
/// object — a fake bold, a hand-made shadow, a word stroked twice — lies over
/// the first copy. Joined, the two copies became one line whose characters
/// [`logical_line`] interleaved: `אבג` drawn twice half a point apart read
/// back `אאבבגג`, where every reader before this join read two lines of
/// `אבג` (review of lane 8C). So two lines overlapping by more than half the
/// shorter one's extent are not joined. A zero-width mark in an object of its
/// own overlaps nothing and still joins its base's line.
///
/// # And a join costs the piece it joins
///
/// What a join asks of the line so far — where it lies along its baseline,
/// which way that baseline runs, whether it holds a right-to-left character,
/// its text — is kept in a [`Reach`] and grown by each piece, never read
/// again from the line's first character. Read again, a line rejoined from
/// `n` pieces cost `n²`: a right-to-left word drawn as one text object per
/// letter or two is all a page needs to be that line, and this runs on every
/// PDF's default extraction (review of lane 8C).
fn rejoin_split_lines(lines: &mut Vec<TextLine>) -> usize {
    let mut joined = 0usize;
    let mut out: Vec<TextLine> = Vec::with_capacity(lines.len());
    // What `continues_on_page` asks of `out.last()`, kept as it grows.
    let mut reach: Option<Reach> = None;
    for line in std::mem::take(lines) {
        let rtl = holds_rtl(&line.chars);
        if let (Some(previous), Some(reached)) = (out.last_mut(), reach.as_mut()) {
            if let Some(along) = continues_on_page(previous, reached, &line, rtl) {
                // A joined line's text is its characters': made from them
                // once, at its first join, and grown by each piece's after.
                if !reached.joined {
                    previous.text.clear();
                    push_text(&mut previous.text, &previous.chars);
                    reached.joined = true;
                }
                let first_new = previous.chars.len();
                push_text(&mut previous.text, &line.chars);
                previous.chars.extend(line.chars);
                previous.size = previous.size.max(line.size);
                previous.quad = enclose(previous.quad, line.quad);
                reached.grow(&previous.chars, first_new, along, rtl);
                joined += 1;
                continue;
            }
        }
        reach = Some(Reach::of(&line.chars, rtl));
        out.push(line);
    }
    *lines = out;
    joined
}

/// What [`continues_on_page`] asks of the line a piece might join, kept as
/// pieces join it — so that a join looks at the piece and not at the line
/// so far. See [`rejoin_split_lines`].
struct Reach {
    /// The line's [`baseline`].
    axis: (f64, f64),
    /// Whether a character of the line gave `axis` — until one does, it is
    /// the page's `x` axis standing in, and a piece joined later may give
    /// the line its first, as `baseline` over the joined characters would.
    settled: bool,
    /// Where the line's characters start and end along `axis`.
    span: (f64, f64),
    /// Whether any of them reads right to left ([`holds_rtl`]).
    rtl: bool,
    /// Whether a piece has been joined to the line yet.
    joined: bool,
}

impl Reach {
    /// A line's reach, from all of its characters, of which `rtl` says
    /// whether any reads right to left.
    fn of(chars: &[TextChar], rtl: bool) -> Reach {
        let found = found_baseline(chars);
        let axis = found.unwrap_or((1.0, 0.0));
        Reach {
            axis,
            settled: found.is_some(),
            span: span(chars, axis),
            rtl,
            joined: false,
        }
    }

    /// The reach of the line once the characters from `first_new` on —
    /// a piece whose extent along `axis` is `along`, and of which `rtl` says
    /// whether it reads right to left — have joined it.
    fn grow(&mut self, chars: &[TextChar], first_new: usize, along: (f64, f64), rtl: bool) {
        self.rtl |= rtl;
        self.span = (self.span.0.min(along.0), self.span.1.max(along.1));
        if !self.settled {
            if let Some(axis) = found_baseline(chars.get(first_new..).unwrap_or_default()) {
                // The line's first baseline, from this piece: the extent is
                // measured again along it — once for the line, since a
                // settled axis never moves.
                self.axis = axis;
                self.settled = true;
                self.span = span(chars, axis);
            }
        }
    }
}

/// Whether `next` is the rest of the line `previous` is on, and if it is,
/// `next`'s extent along that line's baseline. `reach` is `previous`'s
/// [`Reach`]; `next_rtl` says whether `next` holds a right-to-left
/// character. See [`rejoin_split_lines`].
fn continues_on_page(
    previous: &TextLine,
    reach: &Reach,
    next: &TextLine,
    next_rtl: bool,
) -> Option<(f64, f64)> {
    if previous.wmode == WritingMode::Vertical || next.wmode == WritingMode::Vertical {
        return None;
    }
    if !reach.rtl && !next_rtl {
        return None;
    }
    let (Some(first), Some(other)) = (previous.chars.first(), next.chars.first()) else {
        return None;
    };
    let axis = reach.axis;
    let theirs = baseline(&next.chars);
    if axis.0 * theirs.0 + axis.1 * theirs.1 < 0.999 {
        return None;
    }
    // Half an em inclusive, as `TextDevice` resumes a line: a gap of exactly
    // half an em is within it, whatever its last place (`RESUME_TIE`).
    let slack = previous.size.max(next.size).max(1.0) * (0.5 + RESUME_TIE);
    // Across the baseline: the two first glyphs' origins, on the normal.
    let normal = (-axis.1, axis.0);
    let across =
        (other.origin.0 - first.origin.0) * normal.0 + (other.origin.1 - first.origin.1) * normal.1;
    if !across.is_finite() || across.abs() > slack {
        return None;
    }
    // Along it: the gap between the two extents, zero where they overlap.
    let (lo1, hi1) = reach.span;
    let (lo2, hi2) = span(&next.chars, axis);
    let gap = (lo2 - hi1).max(lo1 - hi2).max(0.0);
    let overlap = (hi1.min(hi2) - lo1.max(lo2)).max(0.0);
    let shorter = (hi1 - lo1).min(hi2 - lo2);
    (gap.is_finite() && gap <= slack && overlap <= shorter * 0.5).then_some((lo2, hi2))
}

/// Whether any of `chars` holds a right-to-left character.
fn holds_rtl(chars: &[TextChar]) -> bool {
    chars.iter().any(|c| c.text.chars().any(right_to_left))
}

/// Adds the text of `chars`, in their order, to `text`.
fn push_text(text: &mut String, chars: &[TextChar]) {
    for c in chars {
        look();
        text.push_str(&c.text);
    }
}

/// Where `chars` start and end along `axis`: the outermost of their
/// [`extent`]s.
fn span(chars: &[TextChar], axis: (f64, f64)) -> (f64, f64) {
    chars
        .iter()
        .map(|c| extent(c, axis))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
            (lo.min(a), hi.max(b))
        })
}

/// The smallest upright box around two quads.
fn enclose(a: Quad, b: Quad) -> Quad {
    let (ax0, ay0, ax1, ay1) = a.bounds();
    let (bx0, by0, bx1, by1) = b.bounds();
    let (x0, y0, x1, y1) = (ax0.min(bx0), ay0.min(by0), ax1.max(bx1), ay1.max(by1));
    Quad {
        ul: (x0, y1),
        ur: (x1, y1),
        ll: (x0, y0),
        lr: (x1, y0),
    }
}

/// Whether a character reads right to left, or may open a right-to-left
/// level, by its `Bidi_Class`.
///
/// The trigger for touching a line at all. `AN` is not in it: an Arabic-Indic
/// number in a left-to-right line resolves to level 2, which L2 reverses twice
/// and so leaves where it was.
fn right_to_left(c: char) -> bool {
    look();
    matches!(
        bidi_class(c),
        BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
    )
}

/// Whether a character is a nonspacing mark, which rides on a base glyph.
fn is_mark(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| bidi_class(c) == BidiClass::NSM)
}

/// The unit vector along the line's baseline, from the first character whose
/// quad has one; the page's own `x` axis when none does.
fn baseline(chars: &[TextChar]) -> (f64, f64) {
    found_baseline(chars).unwrap_or((1.0, 0.0))
}

/// The unit vector along the baseline of the first of `chars` whose quad has
/// one, if any does.
fn found_baseline(chars: &[TextChar]) -> Option<(f64, f64)> {
    chars.iter().find_map(|c| {
        let (dx, dy) = (c.quad.lr.0 - c.quad.ll.0, c.quad.lr.1 - c.quad.ll.1);
        let length = (dx * dx + dy * dy).sqrt();
        (length.is_finite() && length > 1e-9).then(|| (dx / length, dy / length))
    })
}

/// Where a character's box starts and ends along `axis`.
fn extent(c: &TextChar, (ux, uy): (f64, f64)) -> (f64, f64) {
    look();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (x, y) in [c.quad.ll, c.quad.lr, c.quad.ul, c.quad.ur] {
        let s = x * ux + y * uy;
        lo = lo.min(s);
        hi = hi.max(s);
    }
    (lo, hi)
}

#[cfg(test)]
thread_local! {
    /// [`look`]'s count, on this thread.
    static LOOKS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// One look at one character: its extent along a baseline ([`extent`]), its
/// `Bidi_Class` ([`right_to_left`]) or its text added to a line's
/// ([`push_text`]) — the three things a join asks of every character of a
/// line, and so the three a crafted page could make it ask over and over.
/// Counted only under `cfg(test)`, where the tests below hold a page's total
/// to a small multiple of its characters, so that a lookup over the whole
/// line so far put back **fails** rather than runs slowly — which
/// `cargo test`, having no timeout, would not notice. Everywhere else it is
/// nothing.
#[inline]
fn look() {
    #[cfg(test)]
    LOOKS.with(|looks| looks.set(looks.get().saturating_add(1)));
}

/// How far `at` lies outside `(lo, hi)`; zero inside it.
fn outside(at: f64, (lo, hi): (f64, f64)) -> f64 {
    if at < lo {
        lo - at
    } else if at > hi {
        at - hi
    } else {
        0.0
    }
}

/// Puts one line into logical order. Returns whether anything moved.
fn logical_line(line: &mut TextLine) -> bool {
    if line.wmode == WritingMode::Vertical {
        return false;
    }
    if !line.chars.iter().any(|c| c.text.chars().any(right_to_left)) {
        return false;
    }
    let count = line.chars.len();
    let axis = baseline(&line.chars);
    let extents: Vec<(f64, f64)> = line.chars.iter().map(|c| extent(c, axis)).collect();
    let marks: Vec<bool> = line.chars.iter().map(|c| is_mark(&c.text)).collect();

    // The nearest base on each side of every position, in two linear passes —
    // a line of nothing but marks is hostile input, and a search per mark
    // would make it quadratic.
    let mut before: Vec<Option<usize>> = vec![None; count];
    let mut last = None;
    for at in 0..count {
        before[at] = last;
        if !marks[at] {
            last = Some(at);
        }
    }
    let mut after: Vec<Option<usize>> = vec![None; count];
    let mut next = None;
    for at in (0..count).rev() {
        after[at] = next;
        if !marks[at] {
            next = Some(at);
        }
    }

    // Step 1: each mark's base. A mark with no base at all stands alone.
    let mut owner: Vec<usize> = (0..count).collect();
    for at in 0..count {
        if !marks[at] {
            continue;
        }
        let (lo, hi) = extents[at];
        let centre = (lo + hi) / 2.0;
        owner[at] = match (before[at], after[at]) {
            (Some(p), Some(q)) => {
                if outside(centre, extents[q]) < outside(centre, extents[p]) {
                    q
                } else {
                    p
                }
            }
            (Some(p), None) => p,
            (None, Some(q)) => q,
            (None, None) => at,
        };
    }
    let mut cluster_of: Vec<Option<usize>> = vec![None; count];
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for at in 0..count {
        if owner[at] == at {
            cluster_of[at] = Some(clusters.len());
            clusters.push(vec![at]);
        }
    }
    for at in 0..count {
        if owner[at] == at {
            continue;
        }
        if let Some(cluster) = cluster_of[owner[at]].and_then(|c| clusters.get_mut(c)) {
            cluster.push(at);
        }
    }

    // Step 2: visual order, by where each base starts. `sort_by` is stable.
    clusters.sort_by(|a, b| {
        let start = |cluster: &Vec<usize>| cluster.first().map_or(0.0, |at| extents[*at].0);
        start(a).total_cmp(&start(b))
    });

    // Steps 3 and 4: the paragraph's direction, and the order the visual line
    // is read in.
    let texts: Vec<String> = clusters
        .iter()
        .map(|cluster| {
            cluster
                .iter()
                .map(|at| line.chars[*at].text.as_str())
                .collect()
        })
        .collect();
    let units: Vec<&str> = texts.iter().map(String::as_str).collect();
    let direction = drawn_direction(&units);
    line.rtl = direction == BaseDirection::RightToLeft;
    let order = logical_order(&units, direction);

    let sequence: Vec<usize> = order
        .iter()
        .filter_map(|k| clusters.get(*k))
        .flat_map(|cluster| cluster.iter().copied())
        .collect();
    if sequence.len() != count || sequence.iter().enumerate().all(|(i, at)| i == *at) {
        return false;
    }
    let mut taken: Vec<Option<TextChar>> = std::mem::take(&mut line.chars)
        .into_iter()
        .map(Some)
        .collect();
    line.chars = sequence
        .iter()
        .filter_map(|at| taken.get_mut(*at).and_then(Option::take))
        .collect();
    line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A character `width` wide whose box starts at `x` on the baseline.
    fn ch(text: &str, x: f64, width: f64) -> TextChar {
        TextChar {
            text: text.to_string(),
            quad: Quad {
                ll: (x, -2.0),
                lr: (x + width, -2.0),
                ul: (x, 8.0),
                ur: (x + width, 8.0),
            },
            size: 10.0,
            origin: (x, 0.0),
            mcid: None,
            stream: 0,
            font: None,
        }
    }

    fn line(chars: Vec<TextChar>, rtl: bool) -> TextLine {
        let text = chars.iter().map(|c| c.text.as_str()).collect();
        TextLine {
            quad: chars[0].quad,
            chars,
            text,
            wmode: WritingMode::Horizontal,
            rtl,
            size: 10.0,
        }
    }

    /// Hebrew alef, bet, gimel.
    const ALEF: &str = "\u{5D0}";
    const BET: &str = "\u{5D1}";
    const GIMEL: &str = "\u{5D2}";
    /// Hebrew point qamats, a nonspacing mark.
    const QAMATS: &str = "\u{5B8}";

    #[test]
    fn a_visual_hebrew_line_reads_forwards() {
        // Drawn left to right as the eye sees it: gimel, bet, alef.
        let mut l = line(
            vec![ch(GIMEL, 0.0, 5.0), ch(BET, 5.0, 5.0), ch(ALEF, 10.0, 5.0)],
            true,
        );
        assert!(logical_line(&mut l));
        assert_eq!(l.text, format!("{ALEF}{BET}{GIMEL}"));
    }

    #[test]
    fn a_line_drawn_in_reading_order_is_already_right() {
        // Alef drawn first at the right, the pen moving left.
        let mut l = line(
            vec![ch(ALEF, 10.0, 5.0), ch(BET, 5.0, 5.0), ch(GIMEL, 0.0, 5.0)],
            true,
        );
        assert!(!logical_line(&mut l), "nothing should have moved");
        assert_eq!(l.text, format!("{ALEF}{BET}{GIMEL}"));
    }

    #[test]
    fn a_mark_stays_with_its_base_whichever_side_it_was_written() {
        // Qamats sits on bet, in the middle of the box bet occupies. A
        // producer that writes the mark after its base, and one that writes
        // it before, describe the same page.
        let after = vec![
            ch(GIMEL, 0.0, 5.0),
            ch(BET, 5.0, 5.0),
            ch(QAMATS, 7.0, 0.0),
            ch(ALEF, 10.0, 5.0),
        ];
        let before = vec![
            ch(GIMEL, 0.0, 5.0),
            ch(QAMATS, 7.0, 0.0),
            ch(BET, 5.0, 5.0),
            ch(ALEF, 10.0, 5.0),
        ];
        for chars in [after, before] {
            let mut l = line(chars, true);
            logical_line(&mut l);
            assert_eq!(l.text, format!("{ALEF}{BET}{QAMATS}{GIMEL}"));
        }
    }

    #[test]
    fn digits_inside_a_right_to_left_line_keep_their_own_order() {
        // Visual: "12 בא" — the number reads left to right, the letters
        // right to left, and the line is right to left.
        let mut l = line(
            vec![
                ch("1", 0.0, 5.0),
                ch("2", 5.0, 5.0),
                ch(" ", 10.0, 5.0),
                ch(BET, 15.0, 5.0),
                ch(ALEF, 20.0, 5.0),
            ],
            true,
        );
        logical_line(&mut l);
        assert_eq!(l.text, format!("{ALEF}{BET} 12"));
    }

    #[test]
    fn a_left_to_right_line_is_not_touched() {
        // Out of order on purpose: a line with no right-to-left character is
        // left as collected, even where its geometry disagrees with its order.
        let mut l = line(vec![ch("b", 5.0, 5.0), ch("a", 0.0, 5.0)], false);
        assert!(!logical_line(&mut l));
        assert_eq!(l.text, "ba");
    }

    /// The direction comes from the drawn line, not from the flag it arrived
    /// with: `TextDevice`'s count of letters by block took this line as right
    /// to left, and it is read as left to right and says so.
    #[test]
    fn the_direction_is_read_off_the_line_and_reported() {
        let mut l = line(
            vec![
                ch("a", 0.0, 5.0),
                ch(" ", 5.0, 5.0),
                ch(GIMEL, 10.0, 5.0),
                ch(BET, 15.0, 5.0),
                ch(ALEF, 20.0, 5.0),
                ch(" ", 25.0, 5.0),
                ch("b", 30.0, 5.0),
            ],
            true,
        );
        assert!(logical_line(&mut l));
        assert_eq!(l.text, format!("a {ALEF}{BET}{GIMEL} b"));
        assert!(!l.rtl, "the line was read left to right");
    }

    /// **The comma tie-break** (ruling 14, amended 10 October 2026): a word
    /// and the comma after it, alone on a line, drawn by a left-to-right
    /// paragraph — the comma at the right — read as that paragraph, and the
    /// line says so. The same word with its comma drawn at the left, as a
    /// right-to-left paragraph draws a trailing one, is still right to left.
    #[test]
    fn a_word_and_its_comma_alone_read_as_their_paragraph_drew_them() {
        let mut quoted = line(
            vec![
                ch(GIMEL, 0.0, 5.0),
                ch(BET, 5.0, 5.0),
                ch(ALEF, 10.0, 5.0),
                ch(",", 15.0, 3.0),
            ],
            true,
        );
        assert!(logical_line(&mut quoted));
        assert_eq!(quoted.text, format!("{ALEF}{BET}{GIMEL},"));
        assert!(!quoted.rtl, "the line was read left to right");

        let mut own = line(
            vec![
                ch(",", 0.0, 3.0),
                ch(GIMEL, 3.0, 5.0),
                ch(BET, 8.0, 5.0),
                ch(ALEF, 13.0, 5.0),
            ],
            false,
        );
        assert!(logical_line(&mut own));
        assert_eq!(own.text, format!("{ALEF}{BET}{GIMEL},"));
        assert!(own.rtl, "the line was read right to left");
    }

    /// A percentage after Arabic digits, the `%` drawn on the number's left.
    #[test]
    fn a_percentage_reads_back_as_typed() {
        let typed = "\u{646}\u{633} 50%";
        let drawn: Vec<TextChar> = ["%", "5", "0", " ", "\u{633}", "\u{646}"]
            .iter()
            .enumerate()
            .map(|(i, t)| ch(t, i as f64 * 5.0, 5.0))
            .collect();
        let mut l = line(drawn, false);
        logical_line(&mut l);
        assert_eq!(l.text, typed);
        assert!(l.rtl);
    }

    #[test]
    fn a_line_of_nothing_but_marks_is_linear_and_whole() {
        let chars: Vec<TextChar> = (0..20_000)
            .map(|i| ch(QAMATS, f64::from(i), 0.0))
            .chain(std::iter::once(ch(ALEF, 0.0, 5.0)))
            .collect();
        let mut l = line(chars, true);
        logical_line(&mut l);
        assert_eq!(l.chars.len(), 20_001, "no character may be lost");
    }

    /// `a ב<span>ג</span>א b`-shaped, as an EPUB draws it: each span its own
    /// text object, in reading order, so the word's objects are drawn right to
    /// left and the device cut the line at both ends of the word.
    fn split_in_three() -> Vec<TextLine> {
        // Each line's quad encloses its characters, as the device's do.
        let whole = |chars: Vec<TextChar>, rtl: bool| {
            let mut l = line(chars, rtl);
            l.quad = l.chars.iter().fold(l.quad, |q, c| enclose(q, c.quad));
            l
        };
        vec![
            whole(vec![ch("a", 0.0, 5.0), ch(" ", 5.0, 5.0)], false),
            whole(
                vec![
                    ch(ALEF, 20.0, 5.0),
                    ch(BET, 15.0, 5.0),
                    ch(GIMEL, 10.0, 5.0),
                ],
                true,
            ),
            whole(vec![ch(" ", 25.0, 5.0), ch("b", 30.0, 5.0)], false),
        ]
    }

    #[test]
    fn a_line_the_stream_split_on_one_baseline_is_read_as_one() {
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines: split_in_three(),
            quad: ch("a", 0.0, 35.0).quad,
        });
        into_logical_order(&mut page);
        let lines = &page.blocks[0].lines;
        assert_eq!(lines.len(), 1, "the three pieces are one line");
        assert_eq!(lines[0].text, format!("a {ALEF}{BET}{GIMEL} b"));
        let (x0, _, x1, _) = lines[0].quad.bounds();
        assert_eq!((x0, x1), (0.0, 35.0), "the joined line encloses all three");
    }

    #[test]
    fn lines_apart_on_one_baseline_stay_apart() {
        // Two table cells: the second starts a whole em past the first.
        let mut lines = vec![
            line(vec![ch(ALEF, 0.0, 5.0)], true),
            line(vec![ch(BET, 15.0, 5.0)], true),
        ];
        assert_eq!(rejoin_split_lines(&mut lines), 0);
        assert_eq!(lines.len(), 2);
    }

    /// **Two pieces exactly half an em apart are one line, whatever the last
    /// place of the gap** (`RESUME_TIE`), as `TextDevice` resumes a line —
    /// and a gap a ten-thousandth of an em wider is not. The first piece
    /// ends at 5 and half an em at ten points is 5, so the second drawn at
    /// 10 is half an em on; drawn at the next `f64` past 10 its gap is
    /// 5.000000000000002, past half an em by the last place alone.
    #[test]
    fn lines_half_an_em_apart_are_rejoined_and_wider_ones_are_not() {
        let apart = |at: f64| {
            let mut lines = vec![
                line(vec![ch(ALEF, 0.0, 5.0)], true),
                line(vec![ch(BET, at, 5.0)], true),
            ];
            rejoin_split_lines(&mut lines);
            lines.len()
        };
        assert_eq!(apart(10.0), 1);
        let next = f64::from_bits(10.0_f64.to_bits() + 1);
        assert!(
            next - 5.0 > 5.0,
            "the gap is past half an em by its last place"
        );
        assert_eq!(apart(next), 1, "the last place cut the line");
        assert_eq!(apart(10.001), 2, "a gap past half an em rejoined the line");
    }

    #[test]
    fn lines_on_two_baselines_stay_apart() {
        let mut lower = ch(BET, 5.0, 5.0);
        lower.origin.1 = -12.0;
        let mut lines = vec![
            line(vec![ch(ALEF, 0.0, 5.0)], true),
            line(vec![lower], true),
        ];
        assert_eq!(rejoin_split_lines(&mut lines), 0);
    }

    /// **A line drawn twice is two lines, not one line of each letter
    /// twice** (review of lane 8C): `אבג` in reading order, and the same again
    /// half a point to the right in a second text object — a fake bold. Each
    /// copy reads back whole.
    #[test]
    fn a_line_drawn_over_itself_is_not_joined_to_its_copy() {
        let copy = |dx: f64| {
            line(
                vec![
                    ch(ALEF, 20.0 + dx, 5.0),
                    ch(BET, 15.0 + dx, 5.0),
                    ch(GIMEL, 10.0 + dx, 5.0),
                ],
                true,
            )
        };
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines: vec![copy(0.0), copy(0.5)],
            quad: ch(ALEF, 10.0, 15.5).quad,
        });
        into_logical_order(&mut page);
        let texts: Vec<&str> = page.blocks[0]
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect();
        let word = format!("{ALEF}{BET}{GIMEL}");
        assert_eq!(texts, [word.as_str(), word.as_str()]);
    }

    /// A mark in a text object of its own, with no width, lies inside its
    /// base's extent and overlaps nothing: it is still the base's line.
    #[test]
    fn a_zero_width_mark_drawn_apart_still_joins_its_line() {
        let mut lines = vec![
            line(vec![ch(BET, 15.0, 5.0), ch(ALEF, 10.0, 5.0)], true),
            line(vec![ch(QAMATS, 17.0, 0.0)], true),
        ];
        assert_eq!(rejoin_split_lines(&mut lines), 1);
        assert_eq!(lines.len(), 1);
    }

    /// **A line rejoined from many pieces costs its characters, not their
    /// square** (review of lane 8C). A right-to-left word drawn as one text
    /// object per two letters, in reading order, after a long left-to-right
    /// run: every piece meets the line so far end to end and is joined to
    /// it. Each join used to rebuild the line's text from every character
    /// joined so far, measure its extent over all of them and look for a
    /// right-to-left character from its start — past the whole left-to-right
    /// run — so the page cost the square of its pieces, on every PDF's
    /// default extraction.
    ///
    /// Held by count, not by a clock: [`look`] counts every character's
    /// extent, `Bidi_Class` and text looked at, under `cfg(test)`, and a page
    /// of `C` characters is held to `8 C`, 49 152 here, where a join that
    /// looks at the line so far took 12 595 201 (its text rebuilt from every
    /// character at every join, uncounted then, besides). And the line is
    /// whole: one line, every character in it once, read in logical order.
    #[test]
    fn a_line_rejoined_from_many_pieces_costs_its_characters() {
        const RUN: usize = 2048;
        const PIECES: usize = 2048;
        let start = -5.0 * RUN as f64;
        let mut lines = vec![line(
            (0..RUN)
                .map(|i| ch("a", start + 5.0 * i as f64, 5.0))
                .collect(),
            false,
        )];
        // Piece `k` spans `[10k, 10k + 10]`: alef drawn first on the right,
        // the pen moving left to bet. The next piece starts where it ends.
        lines.extend((0..PIECES).map(|k| {
            let x = 10.0 * k as f64;
            line(vec![ch(ALEF, x + 5.0, 5.0), ch(BET, x, 5.0)], true)
        }));
        let characters = RUN + 2 * PIECES;
        let mut page = TextPage::default();
        page.blocks.push(tinker_pdf_content::TextBlock {
            lines,
            quad: ch("a", start, 5.0 * characters as f64).quad,
        });

        LOOKS.with(|looks| looks.set(0));
        into_logical_order(&mut page);
        let looks = LOOKS.with(core::cell::Cell::get);

        let bound = 8 * characters;
        assert!(
            looks <= bound,
            "{looks} characters looked at to rejoin and read a page of {characters}; \
             a join that costs its piece looks at no more than {bound}"
        );
        let lines = &page.blocks[0].lines;
        assert_eq!(lines.len(), 1, "every piece is the one line");
        assert_eq!(lines[0].chars.len(), characters, "no character lost");
        let word = format!("{ALEF}{BET}").repeat(PIECES);
        assert_eq!(lines[0].text, format!("{word}{}", "a".repeat(RUN)));
        assert_eq!(
            lines[0].text,
            lines[0]
                .chars
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>(),
            "the text is its characters'"
        );
    }

    /// The join as it was before [`Reach`]: everything about the line so far
    /// read again from its characters at every piece. Quadratic, and kept
    /// only to say what the kept reach must answer.
    fn rejoin_by_rereading(lines: &mut Vec<TextLine>) -> usize {
        fn continues(previous: &TextLine, next: &TextLine) -> bool {
            if previous.wmode == WritingMode::Vertical || next.wmode == WritingMode::Vertical {
                return false;
            }
            if !holds_rtl(&previous.chars) && !holds_rtl(&next.chars) {
                return false;
            }
            let (Some(first), Some(other)) = (previous.chars.first(), next.chars.first()) else {
                return false;
            };
            let axis = baseline(&previous.chars);
            let theirs = baseline(&next.chars);
            if axis.0 * theirs.0 + axis.1 * theirs.1 < 0.999 {
                return false;
            }
            let slack = previous.size.max(next.size).max(1.0) * (0.5 + RESUME_TIE);
            let normal = (-axis.1, axis.0);
            let across = (other.origin.0 - first.origin.0) * normal.0
                + (other.origin.1 - first.origin.1) * normal.1;
            if !across.is_finite() || across.abs() > slack {
                return false;
            }
            let (lo1, hi1) = span(&previous.chars, axis);
            let (lo2, hi2) = span(&next.chars, axis);
            let gap = (lo2 - hi1).max(lo1 - hi2).max(0.0);
            let overlap = (hi1.min(hi2) - lo1.max(lo2)).max(0.0);
            let shorter = (hi1 - lo1).min(hi2 - lo2);
            gap.is_finite() && gap <= slack && overlap <= shorter * 0.5
        }
        let mut joined = 0usize;
        let mut out: Vec<TextLine> = Vec::new();
        for line in std::mem::take(lines) {
            match out.last_mut() {
                Some(previous) if continues(previous, &line) => {
                    previous.chars.extend(line.chars);
                    previous.text = previous.chars.iter().map(|c| c.text.as_str()).collect();
                    previous.size = previous.size.max(line.size);
                    previous.quad = enclose(previous.quad, line.quad);
                    joined += 1;
                }
                _ => out.push(line),
            }
        }
        *lines = out;
        joined
    }

    /// **The kept reach answers what reading the line again answered**, to
    /// the bit, on 2 000 made-up blocks of pieces that meet, miss, overlap,
    /// sit a hair or a line apart, run along baselines a degree, two and
    /// thirty off the page's, and start with characters whose quads have no
    /// baseline at all — so that a line's axis is first the page's and then
    /// its first real piece's, and a piece a degree off is measured along
    /// whichever the line has. The lines are compared by their `Debug`
    /// form, every coordinate in it exact.
    #[test]
    fn the_kept_reach_joins_exactly_what_rereading_the_line_joined() {
        // (cos, sin) of 0, 1, 2 and 30 degrees: within the join's 0.999 of
        // each other but for the last.
        const AXES: [(f64, f64); 4] = [
            (1.0, 0.0),
            (0.999_847_695_156_391_3, 0.017_452_406_437_283_51),
            (0.999_390_827_019_095_8, 0.034_899_496_702_500_97),
            (0.866_025_403_784_438_6, 0.5),
        ];
        const TEXTS: [&str; 5] = ["a", ALEF, BET, QAMATS, " "];
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = |below: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % below
        };
        let mut joins = 0usize;
        for _ in 0..2000 {
            let mut lines = Vec::new();
            let mut along = 0.0f64;
            for _ in 0..1 + next(10) {
                let (ux, uy) = AXES[next(4) as usize];
                let rise = [0.0, 0.3, 20.0][next(3) as usize];
                // Where this piece starts against where the last one ended:
                // overlapping, touching, a hair apart, or an em.
                along += [-4.0, -1.0, 0.0, 0.5, 2.0, 12.0][next(6) as usize];
                let mut chars = Vec::new();
                for _ in 0..1 + next(4) {
                    let width = [0.0, 5.0, 5.0, 3.0][next(4) as usize];
                    let at =
                        |s: f64, up: f64| (s * ux - (rise + up) * uy, s * uy + (rise + up) * ux);
                    chars.push(TextChar {
                        text: TEXTS[next(5) as usize].to_string(),
                        quad: Quad {
                            ll: at(along, -2.0),
                            lr: at(along + width, -2.0),
                            ul: at(along, 8.0),
                            ur: at(along + width, 8.0),
                        },
                        size: [10.0, 12.0][next(2) as usize],
                        origin: at(along, 0.0),
                        mcid: None,
                        stream: 0,
                        font: None,
                    });
                    along += width;
                }
                let mut piece = line(chars, false);
                piece.size = piece.chars.iter().map(|c| c.size).fold(0.0, f64::max);
                piece.quad = piece
                    .chars
                    .iter()
                    .fold(piece.quad, |q, c| enclose(q, c.quad));
                lines.push(piece);
            }
            let mut kept = lines.clone();
            let mut reread = lines;
            let joined = rejoin_split_lines(&mut kept);
            assert_eq!(joined, rejoin_by_rereading(&mut reread));
            assert_eq!(format!("{kept:?}"), format!("{reread:?}"));
            joins += joined;
        }
        assert!(
            joins > 1000,
            "only {joins} joins: the blocks test too little"
        );
    }

    #[test]
    fn left_to_right_lines_are_never_joined() {
        // The same shape as the split word, with no right-to-left character:
        // the device's lines stand exactly as it collected them.
        let mut lines = vec![
            line(vec![ch("a", 0.0, 5.0)], false),
            line(vec![ch("c", 10.0, 5.0)], false),
            line(vec![ch("b", 5.0, 5.0)], false),
        ];
        assert_eq!(rejoin_split_lines(&mut lines), 0);
        assert_eq!(lines.len(), 3);
    }
}
