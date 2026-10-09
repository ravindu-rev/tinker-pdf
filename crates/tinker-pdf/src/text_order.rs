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
//!    Hebrew. [`TextLine::rtl`] is set to the direction read, for every line
//!    this touches.
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
//!   placed as a left-to-right paragraph would place them.
//! - **Vertical lines**, which UAX #9 does not describe.
//!
//! [`crate::Page::text_with`] with [`TextOptions::content_order`] is the
//! opt-out: the characters in the order the content stream showed them, as
//! `TextDevice` collected them.

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
/// their extents along it meet or overlap to within the same half em the
/// device's own rule allows. A line with no right-to-left character beside a
/// line with none is never joined — a left-to-right page is collected exactly
/// as it was — and neither is a vertical one.
fn rejoin_split_lines(lines: &mut Vec<TextLine>) -> usize {
    let mut joined = 0usize;
    let mut out: Vec<TextLine> = Vec::with_capacity(lines.len());
    for line in std::mem::take(lines) {
        match out.last_mut() {
            Some(previous) if continues_on_page(previous, &line) => {
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

/// Whether `next` is the rest of the line `previous` is on. See
/// [`rejoin_split_lines`].
fn continues_on_page(previous: &TextLine, next: &TextLine) -> bool {
    if previous.wmode == WritingMode::Vertical || next.wmode == WritingMode::Vertical {
        return false;
    }
    let holds_rtl = |line: &TextLine| line.chars.iter().any(|c| c.text.chars().any(right_to_left));
    if !holds_rtl(previous) && !holds_rtl(next) {
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
    let slack = previous.size.max(next.size).max(1.0) * 0.5;
    // Across the baseline: the two first glyphs' origins, on the normal.
    let normal = (-axis.1, axis.0);
    let across =
        (other.origin.0 - first.origin.0) * normal.0 + (other.origin.1 - first.origin.1) * normal.1;
    if !across.is_finite() || across.abs() > slack {
        return false;
    }
    // Along it: the gap between the two extents, zero where they overlap.
    let span = |line: &TextLine| {
        line.chars
            .iter()
            .map(|c| extent(c, axis))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
                (lo.min(a), hi.max(b))
            })
    };
    let (lo1, hi1) = span(previous);
    let (lo2, hi2) = span(next);
    let gap = (lo2 - hi1).max(lo1 - hi2).max(0.0);
    gap.is_finite() && gap <= slack
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
    for c in chars {
        let (dx, dy) = (c.quad.lr.0 - c.quad.ll.0, c.quad.lr.1 - c.quad.ll.1);
        let length = (dx * dx + dy * dy).sqrt();
        if length.is_finite() && length > 1e-9 {
            return (dx / length, dy / length);
        }
    }
    (1.0, 0.0)
}

/// Where a character's box starts and ends along `axis`.
fn extent(c: &TextChar, (ux, uy): (f64, f64)) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (x, y) in [c.quad.ll, c.quad.lr, c.quad.ul, c.quad.ur] {
        let s = x * ux + y * uy;
        lo = lo.min(s);
        hi = hi.max(s);
    }
    (lo, hi)
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
