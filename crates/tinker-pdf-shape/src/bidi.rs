//! UAX #9, the Unicode bidirectional algorithm.
//!
//! Text arrives in *logical* order — the order somebody typed it — and has to
//! be drawn in *visual* order, which for a paragraph mixing Hebrew and English
//! is neither left to right nor right to left but both at once. This module
//! resolves an embedding **level** for every character of a paragraph, and
//! exposes the reordering as something the caller applies **per line**.
//!
//! # Why the split, and why it is not an implementation detail
//!
//! Levels are a property of the paragraph. Reordering is a property of the
//! *line*, because two of UAX #9's rules — L1's reset of trailing whitespace
//! and L2's reversal — are defined on a line and give different answers
//! depending on where it ends. Only the caller knows that: line breaking
//! happens in `tinker-pdf-layout`, over the logical text, after shaping.
//!
//! So [`Paragraph::new`] does P2 through I2 once, and [`Paragraph::line`]
//! does L1 and L2 for one range, as many times as the caller has lines. A
//! shaper that reordered the whole paragraph and then broke it would put the
//! trailing space of every right-to-left line on the wrong side.
//!
//! # X9's removed characters are kept in place
//!
//! Rule X9 removes the embedding and override characters and everything of
//! class `BN`. UAX #9 offers two ways to honour it — actually delete them, or
//! retain them and make every later rule skip them — and this takes the
//! second, because deleting renumbers every character after the deletion and
//! this crate's whole contract is that a cluster is an offset into the text
//! the caller supplied. [`Paragraph::is_removed`] says which those are;
//! [`Paragraph::line`] leaves them out of the visual order, which is what X9
//! asks for.
//!
//! # What bounds this against a hostile paragraph
//!
//! Ruling 1: the text is attacker-controlled. Every pass here is linear in the
//! paragraph, the directional status stack is capped at
//! [`Level::MAX_DEPTH`] + 2 entries by rule X2 itself, and BD16's bracket
//! stack is capped at sixty-three by the specification, which says in as many
//! words to stop looking for pairs rather than to grow. Nothing recurses.

use core::ops::Range;

use crate::buffer::Direction;
use crate::unicode::{self, BidiClass, BracketKind};

/// An embedding level: how deeply nested, and which way, a character reads.
///
/// Even is left to right and odd is right to left, which is not a convention
/// this crate chose — it is how UAX #9 states every rule from I1 onwards, and
/// the parity *is* the direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(u8);

impl Level {
    /// The deepest level UAX #9 permits, from rule X1's `max_depth`.
    ///
    /// **125.** An embedding or isolate that would exceed it overflows: the
    /// character is counted and ignored rather than honoured, which is what
    /// keeps a paragraph of 100 000 nested isolates linear instead of
    /// unbounded.
    pub const MAX_DEPTH: u8 = 125;

    /// Left to right, the outermost level of a left-to-right paragraph.
    pub const LTR: Self = Self(0);
    /// Right to left, the outermost level of a right-to-left paragraph.
    pub const RTL: Self = Self(1);

    /// The level as a number.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.0
    }

    /// The level a number names, where UAX #9 can resolve one: up to
    /// [`Level::MAX_DEPTH`] and the one above it that rule I2 can raise a
    /// character to. A caller that resolved a line once and kept each run's
    /// [`Level::number`] reorders the runs with [`reorder`] through this.
    #[must_use]
    pub const fn from_number(number: u8) -> Option<Self> {
        if number <= Self::MAX_DEPTH + 1 {
            Some(Self(number))
        } else {
            None
        }
    }

    /// Whether text at this level reads right to left.
    #[must_use]
    pub const fn is_rtl(self) -> bool {
        self.0 % 2 == 1
    }

    /// Which way text at this level reads.
    #[must_use]
    pub const fn direction(self) -> Direction {
        if self.is_rtl() {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        }
    }

    /// The strong class this level's direction corresponds to: `L` or `R`.
    const fn strong(self) -> BidiClass {
        if self.is_rtl() {
            BidiClass::R
        } else {
            BidiClass::L
        }
    }

    /// The next level above this one that reads right to left (X2, X4, X5a).
    const fn next_odd(self) -> u8 {
        (self.0 + 1) | 1
    }

    /// The next level above this one that reads left to right (X3, X5, X5b).
    const fn next_even(self) -> u8 {
        (self.0 + 2) & !1
    }
}

/// What paragraph direction the caller wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaseDirection {
    /// Rules P2 and P3: the direction of the first strong character, and left
    /// to right if there is none. What a plain text file gets, and what
    /// `dir="auto"` means.
    #[default]
    Auto,
    /// Left to right, whatever the text says.
    LeftToRight,
    /// Right to left, whatever the text says.
    RightToLeft,
}

/// One paragraph, resolved through rule I2.
///
/// Built once per paragraph; [`Paragraph::line`] is then called once per line
/// the caller's line breaker produced.
#[derive(Clone, Debug)]
pub struct Paragraph {
    base: Level,
    /// The `Bidi_Class` each character arrived with, before any rule ran. L1
    /// is defined on these rather than on the resolved ones, and says so.
    original: Vec<BidiClass>,
    /// The byte offset of each character in the text this was built from.
    offsets: Vec<u32>,
    levels: Vec<Level>,
    removed: Vec<bool>,
}

/// One line of a paragraph, after L1 and L2.
#[derive(Clone, Debug)]
pub struct Line {
    start: usize,
    levels: Vec<Level>,
    order: Vec<usize>,
}

impl Line {
    /// The level of each character of the line, after L1's whitespace reset.
    ///
    /// Indexed from the start of the line. A character X9 removed carries the
    /// level it had before L1 and is absent from [`Line::visual_order`];
    /// UAX #9 assigns it none, so nothing should read this for one.
    #[must_use]
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// The characters of the line in the order they are drawn, left to right,
    /// as indices into the *paragraph*.
    ///
    /// Characters X9 removed are not in it.
    #[must_use]
    pub fn visual_order(&self) -> &[usize] {
        &self.order
    }

    /// Where in the paragraph this line began.
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }
}

impl Paragraph {
    /// Resolves one paragraph, P2 through I2.
    ///
    /// `text` is one paragraph: rule P1's split at a paragraph separator is
    /// the caller's, because a caller that already knows where its paragraphs
    /// are should not have them re-found, and one that does not can split on
    /// [`BidiClass::B`] itself.
    #[must_use]
    pub fn new(text: &str, direction: BaseDirection) -> Self {
        let mut chars = Vec::new();
        let mut original = Vec::new();
        let mut offsets = Vec::new();
        for (at, c) in text.char_indices() {
            chars.push(c);
            original.push(unicode::bidi_class(c));
            offsets.push(u32::try_from(at).unwrap_or(u32::MAX));
        }
        let pdi = matching_pdis(&original);
        let base = match direction {
            BaseDirection::LeftToRight => Level::LTR,
            BaseDirection::RightToLeft => Level::RTL,
            BaseDirection::Auto => first_strong(&original, &pdi, 0, original.len()),
        };
        let removed: Vec<bool> = original.iter().map(|c| c.is_removed_by_x9()).collect();
        let (mut levels, mut classes) = explicit(&original, &pdi, base);
        for sequence in isolating_run_sequences(&original, &removed, &levels, &pdi, base) {
            resolve(&mut classes, &original, &chars, &sequence);
            implicit(&mut levels, &classes, &sequence);
        }
        Self {
            base,
            original,
            offsets,
            levels,
            removed,
        }
    }

    /// The paragraph's own embedding level, from P2 and P3 or from the caller.
    #[must_use]
    pub fn base_level(&self) -> Level {
        self.base
    }

    /// How many characters the paragraph holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Whether the paragraph is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Every character's level, after I2 and **before** L1.
    ///
    /// L1 depends on where the line ends, so it is applied by
    /// [`Paragraph::line`] and not here. A caller that reads these directly is
    /// reading the paragraph's structure rather than a line's layout.
    #[must_use]
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// The byte offset of each character in the text.
    #[must_use]
    pub fn offsets(&self) -> &[u32] {
        &self.offsets
    }

    /// Whether rule X9 removed this character.
    ///
    /// True for the four embedding and override initiators, for `PDF`, and
    /// for everything of class `BN`. UAX #9 assigns none of them a level, and
    /// none of them appears in a line's visual order.
    #[must_use]
    pub fn is_removed(&self, at: usize) -> bool {
        self.removed.get(at).copied().unwrap_or(false)
    }

    /// L1 and L2 for one line, as a half-open range of character indices.
    ///
    /// This is the per-line half of the algorithm and the reason the two are
    /// separate: the caller breaks lines over the logical text and then calls
    /// this once per line. A range outside the paragraph is clamped rather
    /// than refused, per ruling 2 — a caller that miscounted gets the text it
    /// asked about and not a panic.
    #[must_use]
    pub fn line(&self, line: Range<usize>) -> Line {
        let start = line.start.min(self.levels.len());
        let end = line.end.clamp(start, self.levels.len());
        let mut levels: Vec<Level> = self.levels[start..end].to_vec();

        // L1: the resets are stated on the *original* classes, and the rule
        // says so in as many words — a whitespace run that W and N turned into
        // something else is still whitespace at the end of a line.
        let mut trailing = true;
        for at in (start..end).rev() {
            let class = self.original[at];
            match class {
                BidiClass::S | BidiClass::B => {
                    levels[at - start] = self.base;
                    trailing = true;
                }
                BidiClass::WS
                | BidiClass::FSI
                | BidiClass::LRI
                | BidiClass::RLI
                | BidiClass::PDI => {
                    if trailing {
                        levels[at - start] = self.base;
                    }
                }
                // An X9-removed character neither ends a trailing run nor
                // starts one: it is not there. Leaving it opaque here would
                // strand the whitespace in front of an `RLE` at the end of a
                // line at its resolved level, which is the one place this
                // choice is visible.
                _ if self.removed[at] => {
                    levels[at - start] = self.base;
                }
                _ => trailing = false,
            }
        }

        // L2, over the characters X9 kept.
        let kept: Vec<usize> = (start..end).filter(|at| !self.removed[*at]).collect();
        let kept_levels: Vec<Level> = kept.iter().map(|at| levels[*at - start]).collect();
        let order = reorder(&kept_levels)
            .into_iter()
            .map(|at| kept[at])
            .collect();

        Line {
            start,
            levels,
            order,
        }
    }
}

/// Rule L2, as a pure function of one line's levels.
///
/// *"From the highest level found in the text to the lowest odd level on each
/// line, including intermediate levels not actually present in the text,
/// reverse any contiguous sequence of characters that are at that level or
/// higher."* The result is the positions of `levels`, in the order they are
/// drawn from left to right.
///
/// Exported because `docs/design/shaping.md` says it should be: the caller
/// owns line breaking, so the caller owns this, and a shaper that kept it
/// private would have to be told where the lines were.
#[must_use]
pub fn reorder(levels: &[Level]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(highest) = levels.iter().map(|l| l.0).max() else {
        return order;
    };
    // The lowest odd level, which is where the reversals stop. A line entirely
    // at even levels has none and is drawn as it stands.
    let lowest_odd = levels
        .iter()
        .map(|l| l.0)
        .filter(|l| l % 2 == 1)
        .min()
        .unwrap_or(highest.saturating_add(1));
    let mut level = highest;
    while level >= lowest_odd && level > 0 {
        let mut at = 0usize;
        while at < levels.len() {
            if levels[at].0 < level {
                at += 1;
                continue;
            }
            let start = at;
            while at < levels.len() && levels[at].0 >= level {
                at += 1;
            }
            order[start..at].reverse();
            // The reversal is over *positions*, so the levels are read from
            // the original array and the order array is what moves.
        }
        level -= 1;
    }
    order
}

/// P2 through L2 for one line handed over as **units** — strings that must
/// stay whole, such as the characters one glyph stands for — with every unit
/// placed, including one made only of characters rule X9 removes.
///
/// The result is the positions of `units` in the order UAX #9 draws them, left
/// to right. That is the same permutation [`Paragraph::line`]'s
/// [`Line::visual_order`] gives for one-character units, less nothing: a unit
/// X9 removed takes the level of the unit before it — the paragraph's own if
/// it is first — which is what UAX #9 §5.2 says an implementation that
/// *retains* the removed characters gives them, and which moves no other unit.
/// A unit's level is its first kept character's.
///
/// This is the **drawing** direction, logical to visual. Reading a drawn line
/// back is [`logical_order`], which calls this to check every answer it gives.
///
/// `crates/tinker-pdf-shape/tests/bidi_conformance.rs` runs the whole of
/// `BidiCharacterTest.txt` and `BidiTest.txt` through **this** function.
#[must_use]
pub fn order_units(units: &[&str], direction: BaseDirection) -> Vec<usize> {
    reorder(&unit_levels(units, direction))
}

/// The level of each unit after L1, as [`order_units`] places them.
fn unit_levels(units: &[&str], direction: BaseDirection) -> Vec<Level> {
    let text: String = units.concat();
    let paragraph = Paragraph::new(&text, direction);
    let line = paragraph.line(0..paragraph.len());
    let resolved = line.levels();
    let mut levels = Vec::with_capacity(units.len());
    let mut previous = paragraph.base_level();
    let mut at = 0usize;
    for unit in units {
        let count = unit.chars().count();
        let end = at.saturating_add(count);
        let own = (at..end)
            .find(|index| !paragraph.is_removed(*index))
            .and_then(|index| resolved.get(index).copied());
        let level = own.unwrap_or(previous);
        levels.push(level);
        previous = level;
        at = end;
    }
    levels
}

/// How many times [`logical_order`] re-resolves a candidate from one starting
/// point before trying the next.
///
/// Each round is one linear resolution of the line, so this is what keeps
/// reading a hostile line a constant multiple of drawing it. Over the whole of
/// `BidiCharacterTest.txt` two rounds from each start already find every
/// order that more rounds find.
const LOGICAL_ORDER_ROUNDS: usize = 4;

/// The inverse of [`order_units`]: given one line's units **as drawn**, left
/// to right, the order they are read in — the positions of `visual`, the
/// first read first.
///
/// # Why this is a search and not a reversal
///
/// L2 is a sequence of reversals, and with every unit's level known it undoes
/// itself. But the levels are resolved over the *logical* line, and rules W2,
/// W5 and W7 look backwards for a strong character while N1 reads both
/// neighbours, so levels resolved over the line as drawn are other levels.
/// `نسبة 50%` resolves its `%` as `ON` (W2 made the digits `AN`) and is
/// drawn `%50` to the left of the word; resolve that drawn line as it stands
/// and the `%` is an `ET` touching an `EN`, so it joins the number and the
/// line reads back as `نسبة %50`.
///
/// So every answer is **checked forwards**: an order is returned only if
/// [`order_units`] draws the units read in that order exactly as `visual`
/// stands, unit for unit. The search is a fixed point on the levels: take a
/// level for each drawn unit, undo L2 with them, resolve what that reads as
/// logical text, carry those levels back to the drawn positions, and go round
/// again until an order checks. It starts from two places, up to four rounds
/// each: the levels of the line resolved as drawn, and the levels of the line
/// resolved backwards (its units right to left), carried back. A
/// right-to-left paragraph starts from the first and a left-to-right one from
/// the second; that order is not derived but measured, because it is the one
/// that brings the most of Unicode's own file back to the text it was typed
/// as (`bidi_conformance.rs` pins the count).
///
/// # What no reader can recover
///
/// UAX #9 is not one-to-one. In a right-to-left paragraph `שלום 2026 now`
/// and `שלום now 2026` are drawn identically — W7 makes digits after a Latin
/// word left to right, so `now 2026` is one run either way — and nothing on
/// the page says which was typed; this returns `שלום now 2026`, the reading
/// that keeps a Latin name and its number together (`iPhone 15`). Where
/// several orders draw the line, the first the search reaches is returned,
/// which in `BidiCharacterTest.txt` is the file's own text except in cases
/// that each hold a paired bracket, whose pairing (rule N0) is read off the
/// logical text and is mirrored on the page. Where the search reaches no order
/// that checks — a line no logical text draws, such as one a producer laid out
/// by a rule of its own — the first candidate is returned, so every unit is
/// still placed exactly once.
///
/// `direction` is the paragraph's; [`BaseDirection::Auto`] is
/// [`drawn_direction`].
///
/// `crates/tinker-pdf-shape/tests/bidi_conformance.rs` feeds every visual
/// order `BidiCharacterTest.txt` states back through this function.
#[must_use]
pub fn logical_order(visual: &[&str], direction: BaseDirection) -> Vec<usize> {
    match direction {
        BaseDirection::Auto => drawn_reading(visual).order,
        given => match read_back(visual, given) {
            Ok(order) | Err(order) => order,
        },
    }
}

/// How a drawn line is read: the paragraph direction [`drawn_direction`]
/// reads it in, the order [`logical_order`] reads it in for that direction,
/// and whether the bracket tie-break chose the direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawnReading {
    /// [`drawn_direction`] of the line; never [`BaseDirection::Auto`].
    pub direction: BaseDirection,
    /// [`logical_order`] of the line for [`Self::direction`]: the drawn
    /// positions in the order they are read.
    pub order: Vec<usize>,
    /// Whether [`drawn_direction`]'s bracket tie-break turned the line from
    /// the direction its strong characters and the comma tie-break gave it.
    pub turned_by_brackets: bool,
}

/// [`drawn_direction`] and [`logical_order`] in one: the direction a drawn
/// line is read in and the order, found together.
///
/// A line [`drawn_direction`] has to read to choose its direction — the
/// comma tie-break's shape, and a line holding a closing bracket left of an
/// opening one — is read in the direction chosen once, and that reading is
/// the order returned. Asked of [`drawn_direction`] and then of
/// [`logical_order`], such a line was searched in the direction chosen twice:
/// three searches on every line of the bracket tie-break's shape where two
/// do (review of the K-11 fix).
#[must_use]
pub fn drawn_reading(visual: &[&str]) -> DrawnReading {
    let (direction, order, turned_by_brackets) = decide(visual);
    let order = order.unwrap_or_else(|| match read_back(visual, direction) {
        Ok(order) | Err(order) => order,
    });
    DrawnReading {
        direction,
        order,
        turned_by_brackets,
    }
}

/// [`logical_order`]'s search, for a paragraph direction already known:
/// `Ok` with the first order that checks — drawn by [`order_units`] under
/// `direction`, it is `visual` unit for unit — or `Err` with the first
/// candidate, every unit placed once, where none does.
///
/// [`drawn_direction`]'s comma tie-break asks the same question of a
/// left-to-right reading, and takes it only on `Ok`.
fn read_back(visual: &[&str], direction: BaseDirection) -> Result<Vec<usize>, Vec<usize>> {
    #[cfg(test)]
    tests::SEARCHES.with(|searches| searches.set(searches.get() + 1));
    let count = visual.len();
    let as_drawn = unit_levels(visual, direction);
    let backwards: Vec<&str> = visual.iter().rev().copied().collect();
    let mut from_the_right = unit_levels(&backwards, direction);
    from_the_right.reverse();
    let starts = if direction == BaseDirection::RightToLeft {
        [as_drawn, from_the_right]
    } else {
        [from_the_right, as_drawn]
    };
    let mut first: Option<Vec<usize>> = None;
    for start in starts {
        let mut levels = start;
        for _ in 0..LOGICAL_ORDER_ROUNDS {
            // With every unit's level known L2 undoes itself: `order` lists
            // the drawn positions in the order they are read.
            let order = reorder(&levels);
            let read: Vec<&str> = order
                .iter()
                .filter_map(|at| visual.get(*at).copied())
                .collect();
            let resolved = unit_levels(&read, direction);
            let drawn = reorder(&resolved);
            if drawn.len() == count
                && drawn
                    .iter()
                    .zip(visual)
                    .all(|(at, unit)| read.get(*at) == Some(unit))
            {
                return Ok(order);
            }
            let mut carried = levels.clone();
            for (position, at) in order.iter().enumerate() {
                if let (Some(slot), Some(level)) = (carried.get_mut(*at), resolved.get(position)) {
                    *slot = *level;
                }
            }
            first.get_or_insert(order);
            if carried == levels {
                break;
            }
            levels = carried;
        }
    }
    Err(first.unwrap_or_else(|| (0..count).collect()))
}

/// P2 for a line given in **visual** order: which way its paragraph runs.
///
/// P2 takes the first strong character in logical order. A left-to-right
/// paragraph draws that one leftmost of its strong characters, and a
/// right-to-left paragraph rightmost, so a line whose leftmost and rightmost
/// strong characters agree is that direction — an English line holding a long
/// Arabic word stays left to right, and a line of a script whose letters are
/// `R`, N'Ko or Adlam as much as Hebrew, reads right to left. Where they
/// disagree either paragraph could have drawn the line, and the majority of
/// its strong characters decides, left to right on a tie or with none.
///
/// Never [`BaseDirection::Auto`]. Classes are the characters' own
/// `Bidi_Class`, so no script is missed by a table of blocks.
///
/// # The comma tie-break
///
/// A line whose strong characters are all right to left has two ends that
/// agree, and is right to left — except where it is drawn as a right-to-left
/// word with punctuation after it **and a left-to-right paragraph draws it**.
/// The shape: its leftmost unit a strong `R` or `AL` character and its
/// rightmost **closing punctuation**, a unit whose class is `CS`, `ON`, `ES`
/// or `ET` (not a number, `EN` or `AN`, and not a mark, `NSM`) and whose
/// `General_Category` is not `Ps` or `Pi`: a mark that trails a
/// left-to-right reading closes, so an opening bracket (every
/// `Bidi_Paired_Bracket_Type` `Open` one is `Ps`) or an opening quotation
/// mark at the right is a right-to-left line's first character. A unit is
/// classed by its first character that draws something, and units that draw
/// nothing are passed over at either end, as rule L1 passes them over at a
/// line's end: whitespace (`WS`, `S`, `B`, and the two no-break spaces, which
/// are `White_Space` but `CS` because they separate digits), the characters
/// X9 removes and the isolate formatting characters. The check:
/// [`logical_order`]'s search, run for a left-to-right paragraph, reaches an
/// order that [`order_units`] draws as the line stands. Where it reaches
/// none, the line is right to left as before. That is one more search, run
/// only on a line of the tie-break's shape.
///
/// Where both paragraphs draw the line — a left-to-right one draws a quoted
/// Hebrew word and its comma `וח,`, and a right-to-left one draws a line
/// that *opens* with the comma the same — P2 cannot be read off it, and
/// this takes the commoner of the two: a right-to-left word quoted in a
/// left-to-right text, followed by its comma or full stop. [`logical_order`]
/// reads the line either way as an order that draws it; only which order is
/// returned depends on this. Ruling 14 (`docs/rulings.md`), amended
/// 10 October 2026 by the owner, records the choice.
///
/// # What it costs
///
/// A lone line of a right-to-left paragraph, holding nothing left to right,
/// that opens with a mark of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is not
/// `Ps` or `Pi` reads with that mark at its end: a dash (`— שלום`, drawn
/// `םולש —`, reads `שלום —`), a bullet, `*`, `+`, `#`, `%`, an ellipsis, or a
/// straight or closing-form quotation mark used to open (`"`, `'`, `”`, `»`;
/// `"` and `'` are `Po`, closing as often as they open). An opening bracket
/// or quotation mark does not pay where `/ToUnicode` names the typed
/// character — `(١) بند`, `“שלום`, `«مرحبا` read as typed — but where it
/// names the mirrored glyph (`»` for `«`) the line ends in a closing mark and
/// pays like a dash. Nor does a
/// line whose mark a European number follows: `— 2026 שלום`, `• 5 תפוחים`
/// and `(1) פריט` are drawn with the number next to the mark, `םולש 2026 —`,
/// and no left-to-right paragraph draws that, because there a European number
/// with no right-to-left letter before it is left to right (W7) and one with
/// a letter before it is drawn left of that letter. Arabic-Indic digits are
/// `AN`, which W7 does not touch, so `— ١ بند` still pays and reads
/// `١ بند —`; so does a line whose number is not next to the mark,
/// `— שלום 5 חו`.
///
/// # A bracket pair drawn facing outward
///
/// The ends, the majority and the comma tie-break each read P2 off the
/// line's strong characters, and where a line's two readings both draw it
/// they can pick the one that reads its brackets inside out. A line of a
/// right-to-left paragraph narrowed so that it opens with
/// `(histone deacetylase). وقد أصبحنا نعرف الآن` holds eighteen Latin
/// letters and seventeen Arabic ones, its ends disagree, and the majority
/// read it left to right, `وقد أصبحنا نعرف الآن .)histone deacetylase(`:
/// a left-to-right paragraph draws that text as the line stands too.
///
/// What tells the two apart is how the pair is drawn. A bracket pair at a
/// right-to-left level, where `/ToUnicode` names the character typed — as
/// this engine's writers do, and as ruling 14's reading, which swaps no
/// character, already takes it — stands on the page with its closing
/// bracket on the **left** of its opening one, each hollow turned away from
/// what it encloses: `)histone deacetylase(`. Read in the right direction,
/// the two pair as typed (BD16, over the text read); in the wrong one they
/// do not. No other way of drawing a **pair** puts it so: at a left-to-right
/// level a pair faces inward, and a producer whose `/ToUnicode` names the
/// mirrored glyph names a right-to-left pair facing inward too. But two
/// brackets that are no pair stand so all the time in left-to-right text — a
/// list marker before a parenthesis, `Step 1) open the file (שלום)`, a
/// smiley, `:)`, or a parenthesis that opens at the end of a line and closes
/// on the next — and read from the right they pair. So the other direction
/// is taken only **where its reading draws the line, pairs more brackets in
/// all than the reading in the direction chosen above, and pairs more of
/// them facing outward and hugging what they enclose** — no whitespace just
/// inside either bracket, as a bracket is set against its own text
/// ([`paired_brackets`]). A pair the line's brackets make only when read the
/// wrong way round has the spaces that stood outside them inside it,
/// `see( وقد )b` from `b) وقد (see`. A pair drawn facing inward is evidence
/// of nothing — `(HDAC)` facing inward in a line with right-to-left text at
/// both ends is what a left-to-right paragraph draws and what a right-to-left
/// one whose `/ToUnicode` names the mirrored glyph draws, and nothing on the
/// page says which — so it never turns a line; it is counted with the rest
/// only so that a reading cannot win by pairing a `)(` that faces outward
/// and breaking the two pairs either side of it, `()()` read from the right.
/// Ruling 14, amended 10 October 2026 a second time, records this as its
/// bracket tie-break, which the owner accepted on 11 October 2026. It is asked
/// only of a line that draws a closing bracket left of an opening one
/// ([`faces_outward`]), and costs that line one search in each direction;
/// [`drawn_reading`] returns the one chosen, so reading the line then costs
/// nothing more.
#[must_use]
pub fn drawn_direction(visual: &[&str]) -> BaseDirection {
    decide(visual).0
}

/// [`drawn_direction`]'s decision: the direction; the order the line is read
/// in for it, where deciding had to read the line in that direction; and
/// whether the bracket tie-break turned it.
fn decide(visual: &[&str]) -> (BaseDirection, Option<Vec<usize>>, bool) {
    let (chosen, found) = first_strong_drawn(visual);
    if !faces_outward(visual) {
        return (chosen, found, false);
    }
    let other = match chosen {
        BaseDirection::RightToLeft => BaseDirection::LeftToRight,
        _ => BaseDirection::RightToLeft,
    };
    let Ok(theirs) = read_back(visual, other) else {
        return (chosen, found, false);
    };
    let mine = found.unwrap_or_else(|| match read_back(visual, chosen) {
        Ok(order) | Err(order) => order,
    });
    let (mut my_outward, mut my_pairs) = paired_brackets(visual, &mine, None);
    // A pair opening the line in the direction chosen may have taken its
    // level from the line before, and is no evidence for the other one
    // where the line's letters say the direction chosen ([`OpenedBefore`]).
    let before = OpenedBefore::of(visual, chosen, my_outward > 0);
    if before.is_some() {
        (my_outward, my_pairs) = paired_brackets(visual, &mine, before);
    }
    let (their_outward, their_pairs) = paired_brackets(visual, &theirs, before);
    if their_outward > my_outward && their_pairs > my_pairs {
        (other, Some(theirs), true)
    } else {
        (chosen, Some(mine), false)
    }
}

/// Whether a drawn line holds a closing bracket left of an opening one of
/// its kind: the only shape in which [`paired_brackets`] can count a pair
/// facing outward, so the only line [`drawn_direction`] reads in both
/// directions for its brackets and the only one its bracket tie-break can
/// turn ([`DrawnReading::turned_by_brackets`]). Linear in the line: it asks
/// nothing of a unit but its first character.
#[must_use]
pub fn faces_outward(visual: &[&str]) -> bool {
    // The opening brackets each closing one met so far, left to right, by the
    // character an opening bracket of its kind would pair with: an opening
    // bracket right of one of them is the shape. Bounded by the brackets'
    // kinds, a handful.
    let mut closed: Vec<char> = Vec::new();
    for unit in visual {
        let Some((paired, kind)) = unit.chars().next().and_then(unicode::bracket) else {
            continue;
        };
        match kind {
            BracketKind::Close => {
                let opener = canonical(paired);
                if !closed.contains(&opener) {
                    closed.push(opener);
                }
            }
            BracketKind::Open => {
                if let Some(c) = unit.chars().next() {
                    if closed.contains(&canonical(c)) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// The bracket pairs the units of `visual`, read in `order`, make as BD16
/// pairs them — over the text read, so in logical order: how many of them
/// are drawn **with the opening bracket right of the closing one**, a pair
/// at a right-to-left level with each hollow turned away from what it
/// encloses, **and hug what they enclose** ([`drawn_direction`]); and how
/// many pairs there are in all.
///
/// A pair hugs what it encloses where the unit read just inside each of its
/// brackets is not whitespace ([`is_blank`]) — `(HDAC)`, and `()`, which
/// encloses nothing — as a bracket is set against its own text. Two brackets
/// that are not a pair as typed but stand facing each other, `1)` and the
/// `(` of a later `(see`, each have their space on the side away from the
/// other: read the wrong way round they pair, and the spaces fall inside,
/// `see( … )1`.
///
/// The pairs in all, because a reading can make a pair facing outward only
/// by breaking two facing inward: `()()` drawn as typed, read from the right,
/// pairs the `)(` between the two pairs and loses both. And a reading that
/// pairs as many brackets as the other, but differently, has nothing to say
/// for it: `Step 1) open the file (שלום) and 2) close it.` pairs `(שלום)`
/// read as typed and `) open the file (` read from the right, one each.
///
/// A unit is a bracket by its first character, the one its marks ride on.
/// BD16's stack of 63 is kept: past it, nothing more is paired. Positions
/// `order` does not hold are skipped, as [`logical_order`] never returns.
fn paired_brackets(
    visual: &[&str],
    order: &[usize],
    before: Option<OpenedBefore>,
) -> (usize, usize) {
    /// BD16's stack, as [`brackets`] keeps it.
    const STACK: usize = 63;
    // The units in the order read, so that what stands just inside a bracket
    // is found by its place in the reading.
    let read: Vec<&str> = order
        .iter()
        .filter_map(|at| visual.get(*at).copied())
        .collect();
    let blank_at =
        |place: Option<usize>| place.and_then(|p| read.get(p)).is_some_and(|u| is_blank(u));
    // Each opening bracket waiting for its closing one: the character it
    // pairs with, where it is drawn, and where it is read.
    let mut stack: Vec<(char, usize, usize)> = Vec::new();
    let mut outward = 0usize;
    let mut pairs = 0usize;
    for (place, at) in order.iter().enumerate() {
        let Some(c) = visual.get(*at).and_then(|unit| unit.chars().next()) else {
            continue;
        };
        let Some((paired, kind)) = unicode::bracket(c) else {
            continue;
        };
        match kind {
            BracketKind::Open => {
                if stack.len() >= STACK {
                    break;
                }
                stack.push((canonical(paired), *at, place));
            }
            BracketKind::Close => {
                let folded = canonical(c);
                if let Some(found) = stack.iter().rposition(|(want, _, _)| *want == folded) {
                    let from_before = stack.get(found).is_some_and(|(_, open_at, open_place)| {
                        before.is_some_and(|before| {
                            before.opens(*open_at, *at)
                                && !read
                                    .get(open_place + 1..place)
                                    .unwrap_or_default()
                                    .iter()
                                    .flat_map(|unit| unit.chars())
                                    .any(|c| before.reads_its_way(c))
                        })
                    });
                    if from_before {
                        stack.truncate(found);
                        continue;
                    }
                    pairs += 1;
                    if let Some((_, open_at, open_place)) = stack.get(found) {
                        // Of an empty pair, what stands just inside each
                        // bracket is the other bracket.
                        let hugs =
                            !blank_at(open_place.checked_add(1)) && !blank_at(place.checked_sub(1));
                        if open_at > at && hugs {
                            outward += 1;
                        }
                    }
                    stack.truncate(found);
                }
            }
        }
    }
    (outward, pairs)
}

/// **A pair that may have taken its level from the line before** — the
/// bracket tie-break's evidence it does not count ([`drawn_direction`]).
///
/// N0 resolves a pair enclosing only letters of the paragraph's other
/// direction by the strong character before its opening bracket, and for a
/// pair that opens a line that character is on the line above. So a
/// left-to-right paragraph's line opening with `(הָאָרֶץ)`, after a line that
/// ended in Hebrew, draws the pair facing outward, `)ץֶרָאָה(`, at its left
/// end, exactly as a right-to-left paragraph draws a pair that ends it — and
/// read from the right, the line pairs it. The review of the K-11 fix found
/// an English paragraph quoting Hebrew in brackets, justified, whose third
/// line was turned so, though it held fifty Latin letters to fifteen Hebrew
/// ones, and read backwards (4 books in a default build, 1 with
/// `bundled-fonts`): its reading from the right paired that pair and the
/// `(מִלְחָמָה)` later on the line, the reading as chosen only the second.
///
/// Such a pair is passed over by both readings — matched, as BD16 matches
/// it, and counted neither among the pairs nor among those facing outward —
/// where it holds a bracket at the end of the line a reading in the
/// direction chosen starts from (the first unit that draws something, from
/// the left for a left-to-right reading, from the right for a right-to-left
/// one) and encloses no strong character of that direction; and only where
/// the line's letters say the direction chosen: where they outnumber the
/// other's and the reading in that direction pairs a bracket facing outward
/// of its own, or where they are twice the other's and eight more
/// ([`OpenedBefore::WIDE_MARGIN`]) — the same paragraph's last line,
/// `(מלחמה) and more English text,.`, its pair the only one, with 22 Latin
/// letters to 5 Hebrew ones. So the tie-break reads every line of
/// `BidiCharacterTest.txt` — whole paragraphs, where nothing comes from a
/// line before, and none of whose turned lines holds more than six letters
/// against the turn — and the reviews' 2 235 wrapped and 13 300 sentence
/// lines as it did without this: passed over wherever it opens the line and
/// the letters outnumber the other's, it read 1 341 of the file's lines
/// worse; wherever the reading as chosen also pairs one facing outward, 8,
/// each with an unpaired bracket.
#[derive(Clone, Copy, Debug)]
struct OpenedBefore {
    /// Where the unit at the start of the line, for the direction chosen, is
    /// drawn.
    edge: usize,
    /// Whether the direction chosen is left to right.
    ltr: bool,
}

impl OpenedBefore {
    /// How many more letters of the direction chosen than of the other a
    /// line holds, at twice as many or more, for them to say that direction
    /// whatever its reading pairs ([`OpenedBefore`]).
    const WIDE_MARGIN: usize = 8;

    /// The line's [`OpenedBefore`] for `chosen`, where `own_outward` says
    /// whether its reading in that direction pairs a bracket facing outward
    /// itself — or `None` where its letters do not say `chosen`: as many of
    /// the other direction or more, or, where the reading pairs none facing
    /// outward, fewer than twice as many and eight more.
    fn of(visual: &[&str], chosen: BaseDirection, own_outward: bool) -> Option<OpenedBefore> {
        let ltr = chosen != BaseDirection::RightToLeft;
        let draws = |at: &usize| visual.get(*at).and_then(|unit| first_drawn(unit)).is_some();
        let edge = if ltr {
            (0..visual.len()).find(draws)
        } else {
            (0..visual.len()).rev().find(draws)
        }?;
        let found = OpenedBefore { edge, ltr };
        let (mut chosen_letters, mut other_letters) = (0usize, 0usize);
        for c in visual.iter().flat_map(|unit| unit.chars()) {
            if found.reads_its_way(c) {
                chosen_letters += 1;
            } else if matches!(
                unicode::bidi_class(c),
                BidiClass::L | BidiClass::R | BidiClass::AL
            ) {
                other_letters += 1;
            }
        }
        let more = chosen_letters > other_letters;
        let wide = chosen_letters >= 2 * other_letters
            && chosen_letters >= other_letters + OpenedBefore::WIDE_MARGIN;
        ((own_outward && more) || wide).then_some(found)
    }

    /// Whether a pair drawn with its brackets at `open` and `close` has one
    /// at the line's start.
    fn opens(self, open: usize, close: usize) -> bool {
        open == self.edge || close == self.edge
    }

    /// Whether `c` is a strong character of the direction chosen.
    fn reads_its_way(self, c: char) -> bool {
        match unicode::bidi_class(c) {
            BidiClass::L => self.ltr,
            BidiClass::R | BidiClass::AL => !self.ltr,
            _ => false,
        }
    }
}

/// Whether a unit is whitespace — every character of it `WS`, `S` or `B`, or
/// one of the [`BLANK_SEPARATORS`] — which [`paired_brackets`] asks of what
/// stands just inside a bracket.
fn is_blank(unit: &str) -> bool {
    !unit.is_empty()
        && unit.chars().all(|c| {
            BLANK_SEPARATORS.contains(&c)
                || matches!(
                    unicode::bidi_class(c),
                    BidiClass::WS | BidiClass::S | BidiClass::B
                )
        })
}

/// [`drawn_direction`] before its bracket tie-break: P2 read off the line's
/// strong characters — its two ends, its majority where they disagree — and
/// the comma tie-break, with the left-to-right reading the comma tie-break
/// found where it took one.
fn first_strong_drawn(visual: &[&str]) -> (BaseDirection, Option<Vec<usize>>) {
    let strong = |c: char| match unicode::bidi_class(c) {
        BidiClass::L => Some(false),
        BidiClass::R | BidiClass::AL => Some(true),
        _ => None,
    };
    let mut leftmost = None;
    let mut rightmost = None;
    let (mut rtl, mut ltr) = (0usize, 0usize);
    for c in visual.iter().flat_map(|unit| unit.chars()) {
        if let Some(is_rtl) = strong(c) {
            leftmost.get_or_insert(is_rtl);
            rightmost = Some(is_rtl);
            if is_rtl {
                rtl += 1;
            } else {
                ltr += 1;
            }
        }
    }
    match (leftmost, rightmost) {
        (Some(false), Some(false)) => (BaseDirection::LeftToRight, None),
        (Some(true), Some(true)) => {
            if ltr == 0 && trailing_punctuation(visual) {
                if let Ok(order) = read_back(visual, BaseDirection::LeftToRight) {
                    return (BaseDirection::LeftToRight, Some(order));
                }
            }
            (BaseDirection::RightToLeft, None)
        }
        _ if rtl > ltr => (BaseDirection::RightToLeft, None),
        _ => (BaseDirection::LeftToRight, None),
    }
}

/// The comma tie-break's shape ([`drawn_direction`]): whether the leftmost
/// unit of a drawn line that draws something is a strong right-to-left
/// character and the rightmost is punctuation that opens nothing.
fn trailing_punctuation(visual: &[&str]) -> bool {
    let mut drawn = visual.iter().filter_map(|unit| first_drawn(unit));
    let leftmost = drawn.next();
    let rightmost = drawn.next_back();
    matches!(
        leftmost.map(unicode::bidi_class),
        Some(BidiClass::R | BidiClass::AL)
    ) && rightmost.is_some_and(|c| {
        matches!(
            unicode::bidi_class(c),
            BidiClass::CS | BidiClass::ON | BidiClass::ES | BidiClass::ET
        ) && !unicode::opens(c)
    })
}

/// The `White_Space` characters whose `Bidi_Class` is not `WS`, `S` or `B`:
/// U+00A0 NO-BREAK SPACE and U+202F NARROW NO-BREAK SPACE, both `CS`
/// because they separate digits.
///
/// Written out because `White_Space` lives in `PropList.txt`, which the
/// vendored UCD does not carry, and every other property this module reads
/// is the vendored one; the standard library's `char::is_whitespace` is the
/// same property at the toolchain's Unicode version, and a test holds the
/// two to each other.
const BLANK_SEPARATORS: [char; 2] = ['\u{A0}', '\u{202F}'];

/// A unit's first character that draws something, or `None` for a unit that
/// draws nothing: whitespace (`WS`, `S`, `B`, or [`BLANK_SEPARATORS`]), a
/// character X9 removes, or an isolate formatting character — what L1 resets
/// with whitespace at a line's end.
fn first_drawn(unit: &str) -> Option<char> {
    unit.chars().find(|c| {
        let class = unicode::bidi_class(*c);
        let blank = BLANK_SEPARATORS.contains(c)
            || matches!(
                class,
                BidiClass::WS | BidiClass::S | BidiClass::B | BidiClass::PDI
            )
            || class.is_removed_by_x9()
            || class.is_isolate_initiator();
        !blank
    })
}

// --- P2, P3 --------------------------------------------------------------

/// Rules P2 and P3: the level of the first strong character, or zero.
///
/// The scan skips anything between an isolate initiator and its matching PDI,
/// which is what makes `FSI` work at all — an isolate's contents must not
/// decide the direction of the paragraph containing it.
fn first_strong(classes: &[BidiClass], pdi: &[Option<usize>], from: usize, to: usize) -> Level {
    let mut at = from;
    while at < to {
        match classes[at] {
            BidiClass::L => return Level::LTR,
            BidiClass::R | BidiClass::AL => return Level::RTL,
            class if class.is_isolate_initiator() => {
                at = pdi[at].unwrap_or(to);
                continue;
            }
            // A PDI with no initiator of its own ends the scan for an
            // enclosing FSI, and is an ordinary neutral for a paragraph.
            BidiClass::PDI => {}
            _ => {}
        }
        at += 1;
    }
    Level::LTR
}

/// BD9: which PDI, if any, matches each isolate initiator.
///
/// One forward pass with a stack rather than a scan per initiator, because a
/// paragraph of nested isolates is otherwise quadratic and ruling 1 says the
/// text is hostile.
fn matching_pdis(classes: &[BidiClass]) -> Vec<Option<usize>> {
    let mut out = vec![None; classes.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (at, class) in classes.iter().enumerate() {
        if class.is_isolate_initiator() {
            stack.push(at);
        } else if *class == BidiClass::PDI {
            if let Some(initiator) = stack.pop() {
                out[initiator] = Some(at);
            }
        }
    }
    out
}

/// BD9's other direction: which initiator, if any, each PDI matches.
fn matching_initiators(classes: &[BidiClass], pdi: &[Option<usize>]) -> Vec<Option<usize>> {
    let mut out = vec![None; classes.len()];
    for (initiator, matched) in pdi.iter().enumerate() {
        if let Some(at) = matched {
            out[*at] = Some(initiator);
        }
    }
    out
}

// --- X1 to X8 ------------------------------------------------------------

/// One entry of X1's directional status stack.
#[derive(Clone, Copy)]
struct Status {
    level: Level,
    /// X6's override: `None` is neutral, otherwise every character in scope
    /// is reclassified as this.
    override_to: Option<BidiClass>,
    isolate: bool,
}

/// Rules X1 through X8: the explicit levels, and the override reclassification.
///
/// Returns a level per character and the class list X6 rewrote.
fn explicit(
    original: &[BidiClass],
    pdi: &[Option<usize>],
    base: Level,
) -> (Vec<Level>, Vec<BidiClass>) {
    let mut levels = vec![base; original.len()];
    let mut classes = original.to_vec();
    let mut stack: Vec<Status> = Vec::with_capacity(usize::from(Level::MAX_DEPTH) + 2);
    stack.push(Status {
        level: base,
        override_to: None,
        isolate: false,
    });
    let mut overflow_isolates = 0usize;
    let mut overflow_embeddings = 0usize;
    let mut valid_isolates = 0usize;

    for at in 0..original.len() {
        let class = original[at];
        // The current top of the stack, which every arm needs and which the
        // isolate arms need *before* they push.
        let top = *stack.last().unwrap_or(&Status {
            level: base,
            override_to: None,
            isolate: false,
        });
        match class {
            // X2 to X5: the embeddings and overrides. The character itself is
            // removed by X9, so the level it carries is never read; it is set
            // to the level in force before the push, which is what the
            // specification's own "retaining" guidance describes.
            BidiClass::RLE | BidiClass::LRE | BidiClass::RLO | BidiClass::LRO => {
                levels[at] = top.level;
                let wanted = if matches!(class, BidiClass::RLE | BidiClass::RLO) {
                    top.level.next_odd()
                } else {
                    top.level.next_even()
                };
                let override_to = match class {
                    BidiClass::RLO => Some(BidiClass::R),
                    BidiClass::LRO => Some(BidiClass::L),
                    _ => None,
                };
                if wanted <= Level::MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0
                {
                    stack.push(Status {
                        level: Level(wanted),
                        override_to,
                        isolate: false,
                    });
                } else if overflow_isolates == 0 {
                    overflow_embeddings += 1;
                }
            }
            // X5a to X5c: the isolates. Unlike an embedding, an isolate
            // initiator is *not* removed by X9 — it takes a level and takes
            // part in the neutral rules — so its level is the one in force
            // before the push.
            BidiClass::RLI | BidiClass::LRI | BidiClass::FSI => {
                levels[at] = top.level;
                if let Some(to) = top.override_to {
                    classes[at] = to;
                }
                // X5c: an FSI reads as an RLI or an LRI depending on the first
                // strong character *inside* it.
                let rtl = match class {
                    BidiClass::RLI => true,
                    BidiClass::LRI => false,
                    _ => {
                        let end = pdi[at].unwrap_or(original.len());
                        first_strong(original, pdi, at + 1, end).is_rtl()
                    }
                };
                let wanted = if rtl {
                    top.level.next_odd()
                } else {
                    top.level.next_even()
                };
                if wanted <= Level::MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0
                {
                    valid_isolates += 1;
                    stack.push(Status {
                        level: Level(wanted),
                        override_to: None,
                        isolate: true,
                    });
                } else {
                    overflow_isolates += 1;
                }
            }
            // X6a.
            BidiClass::PDI => {
                if overflow_isolates > 0 {
                    overflow_isolates -= 1;
                } else if valid_isolates > 0 {
                    overflow_embeddings = 0;
                    while stack.last().is_some_and(|s| !s.isolate) && stack.len() > 1 {
                        stack.pop();
                    }
                    if stack.len() > 1 {
                        stack.pop();
                    }
                    valid_isolates -= 1;
                }
                let top = *stack.last().unwrap_or(&top);
                levels[at] = top.level;
                if let Some(to) = top.override_to {
                    classes[at] = to;
                }
            }
            // X7.
            BidiClass::PDF => {
                levels[at] = top.level;
                if overflow_isolates > 0 {
                    // A PDF inside an overflowed isolate does nothing at all.
                } else if overflow_embeddings > 0 {
                    overflow_embeddings -= 1;
                } else if !top.isolate && stack.len() > 1 {
                    stack.pop();
                    levels[at] = stack.last().map_or(base, |s| s.level);
                }
            }
            // X8: a paragraph separator is always at the paragraph level.
            BidiClass::B => {
                levels[at] = base;
            }
            // X6.
            _ => {
                levels[at] = top.level;
                if let Some(to) = top.override_to {
                    classes[at] = to;
                }
            }
        }
    }
    (levels, classes)
}

// --- X10 -----------------------------------------------------------------

/// One isolating run sequence: the positions it covers, and its boundaries.
struct Sequence {
    positions: Vec<usize>,
    level: Level,
    sos: BidiClass,
    eos: BidiClass,
}

/// Rule X10: the isolating run sequences, with their `sos` and `eos`.
///
/// A level run is a maximal stretch of characters X9 kept that share a level.
/// An isolating run sequence is a level run, plus — whenever its last
/// character is an isolate initiator with a matching PDI — the level run that
/// PDI starts, and so on. That is what makes the text inside an isolate
/// invisible to the neutral rules outside it, which is the whole point of an
/// isolate.
fn isolating_run_sequences(
    original: &[BidiClass],
    removed: &[bool],
    levels: &[Level],
    pdi: &[Option<usize>],
    base: Level,
) -> Vec<Sequence> {
    let kept: Vec<usize> = (0..original.len()).filter(|at| !removed[*at]).collect();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for at in kept {
        match runs.last_mut() {
            Some(run) if levels[*run.last().unwrap_or(&at)] == levels[at] => run.push(at),
            _ => runs.push(vec![at]),
        }
    }
    let mut run_of: Vec<Option<usize>> = vec![None; original.len()];
    for (index, run) in runs.iter().enumerate() {
        for at in run {
            run_of[*at] = Some(index);
        }
    }
    let initiator_of = matching_initiators(original, pdi);

    let mut out = Vec::new();
    let mut used = vec![false; runs.len()];
    for index in 0..runs.len() {
        if used[index] {
            continue;
        }
        let Some(&first) = runs[index].first() else {
            continue;
        };
        // A run beginning with a PDI that closes an isolate is the
        // continuation of somebody else's sequence, not the start of one.
        if original[first] == BidiClass::PDI && initiator_of[first].is_some() {
            continue;
        }
        let mut positions = Vec::new();
        let mut current = index;
        loop {
            used[current] = true;
            positions.extend_from_slice(&runs[current]);
            let Some(&last) = runs[current].last() else {
                break;
            };
            if !original[last].is_isolate_initiator() {
                break;
            }
            let Some(matched) = pdi[last] else {
                break;
            };
            let Some(next) = run_of[matched] else {
                break;
            };
            if used[next] {
                break;
            }
            current = next;
        }

        let level = positions.first().map_or(base, |at| levels[*at]);
        // sos: the higher of this sequence's level and the level of the
        // character before it, or the paragraph level if there is none.
        let before = positions
            .first()
            .and_then(|first| (0..*first).rev().find(|at| !removed[*at]))
            .map_or(base, |at| levels[at]);
        let sos = Level(level.0.max(before.0)).strong();
        // eos: the same, forward — except that an isolate initiator with no
        // matching PDI ends its sequence at the paragraph boundary, whatever
        // follows it in the text.
        let last = positions.last().copied();
        let dangling =
            last.is_some_and(|at| original[at].is_isolate_initiator() && pdi[at].is_none());
        let after = if dangling {
            base
        } else {
            last.and_then(|last| ((last + 1)..original.len()).find(|at| !removed[*at]))
                .map_or(base, |at| levels[at])
        };
        let eos = Level(level.0.max(after.0)).strong();

        out.push(Sequence {
            positions,
            level,
            sos,
            eos,
        });
    }
    out
}

// --- W1 to W7, N0 to N2 ---------------------------------------------------

/// Whether a class is one of N1's "NI" — a neutral or an isolate formatting
/// character.
fn is_neutral(class: BidiClass) -> bool {
    matches!(
        class,
        BidiClass::B
            | BidiClass::S
            | BidiClass::WS
            | BidiClass::ON
            | BidiClass::FSI
            | BidiClass::LRI
            | BidiClass::RLI
            | BidiClass::PDI
    )
}

/// N1's reading of a class as a direction: numbers count as right to left.
fn as_strong(class: BidiClass) -> Option<BidiClass> {
    match class {
        BidiClass::L => Some(BidiClass::L),
        BidiClass::R | BidiClass::EN | BidiClass::AN => Some(BidiClass::R),
        _ => None,
    }
}

/// Rules W1 through N2, over one isolating run sequence.
fn resolve(classes: &mut [BidiClass], original: &[BidiClass], chars: &[char], sequence: &Sequence) {
    let positions = &sequence.positions;
    let mut work: Vec<BidiClass> = positions.iter().map(|at| classes[*at]).collect();
    let (sos, eos) = (sequence.sos, sequence.eos);

    // W1: a non-spacing mark takes the type of what it follows, and `ON` after
    // an isolate initiator or a PDI — a mark must not reach across an
    // isolate's boundary for its class.
    let mut previous = sos;
    for class in &mut work {
        if *class == BidiClass::NSM {
            *class = if previous.is_isolate_initiator() || previous == BidiClass::PDI {
                BidiClass::ON
            } else {
                previous
            };
        }
        previous = *class;
    }

    // W2: a European number after an Arabic letter is an Arabic number.
    let mut strong = sos;
    for class in &mut work {
        match *class {
            BidiClass::L | BidiClass::R | BidiClass::AL => strong = *class,
            BidiClass::EN if strong == BidiClass::AL => *class = BidiClass::AN,
            _ => {}
        }
    }

    // W3: and the Arabic letter itself is now simply right to left.
    for class in &mut work {
        if *class == BidiClass::AL {
            *class = BidiClass::R;
        }
    }

    // W4: a single separator between two numbers of the same kind joins them.
    for at in 1..work.len().saturating_sub(1) {
        let (before, here, after) = (work[at - 1], work[at], work[at + 1]);
        if here == BidiClass::ES && before == BidiClass::EN && after == BidiClass::EN {
            work[at] = BidiClass::EN;
        } else if here == BidiClass::CS
            && before == after
            && matches!(before, BidiClass::EN | BidiClass::AN)
        {
            work[at] = before;
        }
    }

    // W5: a run of European terminators next to a European number joins it.
    let mut at = 0usize;
    while at < work.len() {
        if work[at] != BidiClass::ET {
            at += 1;
            continue;
        }
        let start = at;
        while at < work.len() && work[at] == BidiClass::ET {
            at += 1;
        }
        let before = start.checked_sub(1).map(|b| work[b]);
        let after = work.get(at).copied();
        if before == Some(BidiClass::EN) || after == Some(BidiClass::EN) {
            for class in &mut work[start..at] {
                *class = BidiClass::EN;
            }
        }
    }

    // W6: every separator and terminator that survived is a neutral.
    for class in &mut work {
        if matches!(*class, BidiClass::ET | BidiClass::ES | BidiClass::CS) {
            *class = BidiClass::ON;
        }
    }

    // W7: a European number in left-to-right context is left to right.
    let mut strong = sos;
    for class in &mut work {
        match *class {
            BidiClass::L | BidiClass::R => strong = *class,
            BidiClass::EN if strong == BidiClass::L => *class = BidiClass::L,
            _ => {}
        }
    }

    brackets(&mut work, original, chars, positions, sequence.level, sos);

    // N1: a run of neutrals between two sides that agree takes their
    // direction. `EN` and `AN` count as right to left here, which is why the
    // comparison goes through `as_strong` rather than over the classes.
    let mut at = 0usize;
    while at < work.len() {
        if !is_neutral(work[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < work.len() && is_neutral(work[at]) {
            at += 1;
        }
        let before = start
            .checked_sub(1)
            .and_then(|b| as_strong(work[b]))
            .unwrap_or(sos);
        let after = work.get(at).copied().and_then(as_strong).unwrap_or(eos);
        // N2: and one that does not agree takes the embedding direction.
        let resolved = if before == after {
            before
        } else {
            sequence.level.strong()
        };
        for class in &mut work[start..at] {
            *class = resolved;
        }
    }

    for (at, class) in positions.iter().zip(work) {
        classes[*at] = class;
    }
}

/// BD16 and N0: bracket pairs.
///
/// # The canonical equivalence, and where it comes from
///
/// BD16 matches an opening bracket against a closing one *"including canonical
/// equivalence"*, and there are exactly two characters this affects:
/// U+2329 LEFT-POINTING ANGLE BRACKET is canonically equivalent to U+3008, and
/// U+232A to U+3009. `BidiBrackets.txt`'s own header names those four code
/// points as the case. Without the fold, a paragraph that opens with U+2329
/// and closes with U+3009 finds no pair and resolves the contents as bare
/// neutrals — which `BidiCharacterTest.txt` has cases for, and which is how
/// this was checked rather than assumed.
fn brackets(
    work: &mut [BidiClass],
    original: &[BidiClass],
    chars: &[char],
    positions: &[usize],
    level: Level,
    sos: BidiClass,
) {
    /// BD16: *"if an attempt is made to push onto the stack an element that
    /// would make its size exceed 63 elements, stop processing BD16 for the
    /// remainder of the isolating run sequence"*.
    const STACK: usize = 63;

    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (at, class) in work.iter().enumerate() {
        if *class != BidiClass::ON {
            continue;
        }
        let Some(c) = positions.get(at).and_then(|p| chars.get(*p)).copied() else {
            continue;
        };
        let Some((paired, kind)) = unicode::bracket(c) else {
            continue;
        };
        match kind {
            BracketKind::Open => {
                if stack.len() >= STACK {
                    break;
                }
                stack.push((canonical(paired), at));
            }
            BracketKind::Close => {
                let folded = canonical(c);
                if let Some(found) = stack.iter().rposition(|(want, _)| *want == folded) {
                    pairs.push((stack[found].1, at));
                    stack.truncate(found);
                }
            }
        }
    }
    pairs.sort_unstable();

    let e = level.strong();
    let o = if e == BidiClass::L {
        BidiClass::R
    } else {
        BidiClass::L
    };
    for (open, close) in pairs {
        let mut found_e = false;
        let mut found_o = false;
        for class in &work[open + 1..close] {
            match as_strong(*class) {
                Some(strong) if strong == e => found_e = true,
                Some(_) => found_o = true,
                None => {}
            }
        }
        let resolved = if found_e {
            Some(e)
        } else if found_o {
            // The opposite direction is only taken where the context before
            // the pair is also opposite; otherwise the pair takes the
            // embedding direction and the text inside it is left to N1.
            let context = work[..open]
                .iter()
                .rev()
                .find_map(|class| as_strong(*class))
                .unwrap_or(sos);
            Some(if context == o { o } else { e })
        } else {
            None
        };
        let Some(resolved) = resolved else {
            continue;
        };
        work[open] = resolved;
        work[close] = resolved;
        // N0's closing note: a combining mark on a bracket follows the
        // bracket, and "was an NSM" is asked of the *original* class, because
        // W1 has already overwritten the working one.
        for bracket in [open, close] {
            let mut at = bracket.saturating_add(1);
            while positions.get(at).map(|p| original[*p]) == Some(BidiClass::NSM) {
                work[at] = resolved;
                at = at.saturating_add(1);
            }
        }
    }
}

/// The two canonical equivalences BD16's bracket matching has to fold.
const fn canonical(c: char) -> char {
    match c {
        '\u{3008}' => '\u{2329}',
        '\u{3009}' => '\u{232A}',
        other => other,
    }
}

/// Rules I1 and I2: the levels the resolved classes imply.
fn implicit(levels: &mut [Level], classes: &[BidiClass], sequence: &Sequence) {
    for at in &sequence.positions {
        let level = levels[*at];
        let bump = if level.is_rtl() {
            // I2: at an odd level, everything left-to-right-ish goes up one.
            match classes[*at] {
                BidiClass::L | BidiClass::EN | BidiClass::AN => 1,
                _ => 0,
            }
        } else {
            // I1: at an even level, R goes up one and the numbers go up two.
            match classes[*at] {
                BidiClass::R => 1,
                BidiClass::EN | BidiClass::AN => 2,
                _ => 0,
            }
        };
        levels[*at] = Level(level.0.saturating_add(bump));
    }
}

/// Rule L4: the character this one is *drawn* as in a right-to-left context.
///
/// `None` where nothing changes — at an even level, or for a character with no
/// `Bidi_Mirroring_Glyph`. A `(` inside a Hebrew phrase is drawn as a `)`
/// because the reader's eye moves the other way; the *text* still holds a `(`,
/// which is why this returns a character to draw rather than editing anything.
///
/// UAX #9 says in as many words that the mirroring may instead be done by the
/// font, through `GSUB`'s `rtlm` feature, and that a renderer must not do both.
/// This crate's default shaper does not request `rtlm`, so the answer here is
/// the whole answer; a caller that turns `rtlm` on must stop calling this.
///
/// The conformance files do not reach this rule — `BidiCharacterTest.txt`
/// states in its own header that *"rules L3 and L4 are also out of scope"* —
/// so what stands behind it is the vendored `BidiMirroring.txt` and the unit
/// tests below, and that is weaker evidence than the rest of this module has.
#[must_use]
pub fn mirror(c: char, level: Level) -> Option<char> {
    if !level.is_rtl() {
        return None;
    }
    unicode::mirrored(c)
}

#[cfg(test)]
mod tests {
    use super::{
        drawn_direction, drawn_reading, logical_order, mirror, order_units, read_back, reorder,
        trailing_punctuation, BaseDirection, Level, Paragraph, BLANK_SEPARATORS,
    };
    use crate::unicode::{bidi_class, BidiClass};
    use std::cell::Cell;

    std::thread_local! {
        /// How many times [`read_back`] searched on this thread.
        pub(super) static SEARCHES: Cell<usize> = const { Cell::new(0) };
    }

    /// How many searches `read` makes.
    fn searches<T>(read: impl FnOnce() -> T) -> (T, usize) {
        let before = SEARCHES.with(Cell::get);
        let value = read();
        (value, SEARCHES.with(Cell::get) - before)
    }

    fn levels(text: &str, direction: BaseDirection) -> Vec<u8> {
        let paragraph = Paragraph::new(text, direction);
        let line = paragraph.line(0..paragraph.len());
        line.levels().iter().map(|l| l.number()).collect()
    }

    fn order(text: &str, direction: BaseDirection) -> Vec<usize> {
        let paragraph = Paragraph::new(text, direction);
        paragraph.line(0..paragraph.len()).visual_order().to_vec()
    }

    #[test]
    fn a_left_to_right_paragraph_is_left_alone() {
        assert_eq!(levels("abc", BaseDirection::Auto), vec![0, 0, 0]);
        assert_eq!(order("abc", BaseDirection::Auto), vec![0, 1, 2]);
    }

    /// P2 and P3: the first strong character decides, and Hebrew is strong.
    #[test]
    fn the_first_strong_character_sets_the_paragraph_level() {
        let hebrew = "\u{05D0}\u{05D1}";
        assert_eq!(
            Paragraph::new(hebrew, BaseDirection::Auto).base_level(),
            Level::RTL
        );
        assert_eq!(
            Paragraph::new("a\u{05D0}", BaseDirection::Auto).base_level(),
            Level::LTR
        );
        // Nothing strong at all, so left to right.
        assert_eq!(
            Paragraph::new("123 ", BaseDirection::Auto).base_level(),
            Level::LTR
        );
    }

    #[test]
    fn a_hebrew_word_reverses() {
        let hebrew = "\u{05D0}\u{05D1}\u{05D2}";
        assert_eq!(order(hebrew, BaseDirection::Auto), vec![2, 1, 0]);
    }

    /// L1: whitespace at the end of a right-to-left line falls back to the
    /// paragraph level, so it is drawn on the left and not stranded inside the
    /// word. This is the rule that makes reordering a per-*line* operation.
    #[test]
    fn trailing_whitespace_falls_back_to_the_paragraph_level() {
        let text = "\u{05D0}\u{05D1} ";
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        assert_eq!(paragraph.base_level(), Level::RTL);
        // Before L1 the space is inside the run.
        assert_eq!(
            paragraph
                .levels()
                .iter()
                .map(|l| l.number())
                .collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
        // After L1, over the whole paragraph as one line, it is not.
        assert_eq!(levels(text, BaseDirection::Auto), vec![1, 1, 1]);

        // And in a left-to-right paragraph the same reset moves it the other
        // way: the trailing space of a Hebrew phrase goes back to level 0.
        let mixed = "a \u{05D0}\u{05D1} ";
        assert_eq!(levels(mixed, BaseDirection::Auto), vec![0, 0, 1, 1, 0]);
    }

    /// Breaking the same paragraph in two places gives two different answers
    /// for the same characters, which is why `line` exists at all.
    #[test]
    fn where_the_line_ends_changes_the_answer() {
        let text = "\u{05D0} \u{05D1}";
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        // The whole thing on one line: the space is between two Hebrew
        // letters and stays at level 1.
        assert_eq!(
            paragraph
                .line(0..3)
                .levels()
                .iter()
                .map(|l| l.number())
                .collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
        // Broken after the space, the space is now trailing and L1 resets it.
        let first = paragraph.line(0..2);
        assert_eq!(
            first
                .levels()
                .iter()
                .map(|l| l.number())
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        assert_eq!(first.visual_order(), &[1, 0]);
    }

    /// X9's removed characters keep their place in the paragraph and stay out
    /// of the visual order.
    #[test]
    fn an_embedding_character_is_removed_from_the_order_and_not_from_the_text() {
        // RLE, an Arabic letter, PDF.
        let text = "a\u{202B}\u{05D0}\u{202C}b";
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        assert!(!paragraph.is_removed(0));
        assert!(paragraph.is_removed(1));
        assert!(!paragraph.is_removed(2));
        assert!(paragraph.is_removed(3));
        assert!(!paragraph.is_removed(4));
        assert_eq!(paragraph.line(0..5).visual_order(), &[0, 2, 4]);
    }

    /// Byte offsets survive, because a cluster is one and milestone 7 rebuilds
    /// `/ToUnicode` out of them.
    #[test]
    fn offsets_are_bytes_and_not_characters() {
        let paragraph = Paragraph::new("a\u{05D0}b", BaseDirection::Auto);
        assert_eq!(paragraph.offsets(), &[0, 1, 3]);
    }

    /// L2 on its own, as the pure function the design doc says the caller gets.
    #[test]
    fn reorder_reverses_from_the_highest_level_down() {
        let l = |n: u8| Level(n);
        // One right-to-left stretch inside a left-to-right line.
        assert_eq!(
            reorder(&[l(0), l(1), l(1), l(0)]),
            vec![0, 2, 1, 3],
            "the level 1 run reverses and nothing else moves"
        );
        // A number inside Hebrew: level 2 inside level 1 reverses twice.
        assert_eq!(reorder(&[l(1), l(2), l(2), l(1)]), vec![3, 1, 2, 0]);
        // A line with no odd level at all is drawn as it stands.
        assert_eq!(reorder(&[l(0), l(0), l(2)]), vec![0, 1, 2]);
        assert!(reorder(&[]).is_empty());
    }

    #[test]
    fn a_line_outside_the_paragraph_is_clamped_rather_than_panicking() {
        let paragraph = Paragraph::new("ab", BaseDirection::Auto);
        assert_eq!(paragraph.line(0..99).visual_order(), &[0, 1]);
        // A start past the end and an end before the start both clamp to
        // nothing rather than panicking, which is ruling 2's shape.
        #[allow(clippy::reversed_empty_ranges)]
        let backwards = paragraph.line(5..1);
        assert_eq!(backwards.visual_order(), &[] as &[usize]);
    }

    #[test]
    fn mirroring_happens_at_odd_levels_and_nowhere_else() {
        assert_eq!(mirror('(', Level::RTL), Some(')'));
        assert_eq!(mirror('(', Level::LTR), None);
        assert_eq!(mirror('a', Level::RTL), None);
    }

    /// Rule X1's `max_depth`. A paragraph of nested isolates past 125 levels
    /// overflows rather than growing, which is what keeps the cost linear.
    #[test]
    fn nesting_past_the_maximum_depth_overflows_rather_than_growing() {
        let mut text = String::new();
        for _ in 0..200 {
            text.push('\u{2067}'); // RLI
        }
        text.push('a');
        let paragraph = Paragraph::new(&text, BaseDirection::LeftToRight);
        let highest = paragraph
            .levels()
            .iter()
            .map(|l| l.number())
            .max()
            .unwrap_or(0);
        // 126, not 125: rule X1 caps the *explicit* depth at 125, and I1 may
        // then raise a right-to-left character inside it by one more. UAX #9
        // BD2 states the resolved maximum as one above the explicit one, which
        // is why this is not off by one.
        assert!(
            highest <= Level::MAX_DEPTH + 1,
            "an isolate reached level {highest}"
        );
    }

    /// `text` as one-character units, drawn by [`order_units`] and read back
    /// by [`logical_order`]: what the line looks like, and what it reads as.
    fn drawn_and_read(text: &str, direction: BaseDirection) -> (String, String) {
        let units: Vec<String> = text.chars().map(String::from).collect();
        let logical: Vec<&str> = units.iter().map(String::as_str).collect();
        let visual: Vec<&str> = order_units(&logical, direction)
            .into_iter()
            .map(|at| logical[at])
            .collect();
        let read: String = logical_order(&visual, direction)
            .into_iter()
            .map(|at| visual[at])
            .collect();
        (visual.concat(), read)
    }

    /// **The review's percentage.** W2 makes the digits after an Arabic
    /// letter `AN`, so the `%` is `ON` and is drawn on the number's left. Read
    /// as drawn, that `%` is an `ET` beside an `EN` and joins the number; the
    /// forward check is what refuses `نسبة %50`.
    #[test]
    fn an_arabic_percentage_reads_back_as_typed() {
        let typed = "\u{646}\u{633}\u{628}\u{629} 50%";
        let (drawn, read) = drawn_and_read(typed, BaseDirection::RightToLeft);
        assert_eq!(drawn, "%50 \u{629}\u{628}\u{633}\u{646}");
        assert_eq!(read, typed);
    }

    /// Unicode's own counter-examples to "L2 undoes itself", from the review:
    /// a Hebrew letter before a hyphenated range, and an Arabic letter before
    /// a fraction.
    #[test]
    fn numbers_with_separators_read_back_as_typed() {
        for (typed, direction) in [
            ("\u{5D0} 1-2", BaseDirection::LeftToRight),
            ("\u{62A}1/2", BaseDirection::LeftToRight),
            ("\u{5D0} 1-2", BaseDirection::RightToLeft),
        ] {
            let (_, read) = drawn_and_read(typed, direction);
            assert_eq!(read, typed, "{direction:?}");
        }
    }

    /// **What no reader can recover.** Two texts drawn identically: the answer
    /// is one of them, it draws the line, and which one is the stated choice.
    #[test]
    fn two_texts_drawn_alike_read_as_the_stated_one() {
        let shalom = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
        let number_first = format!("{shalom} 2026 now");
        let word_first = format!("{shalom} now 2026");
        let (a, read_a) = drawn_and_read(&number_first, BaseDirection::RightToLeft);
        let (b, read_b) = drawn_and_read(&word_first, BaseDirection::RightToLeft);
        assert_eq!(a, b, "the two are not drawn alike");
        assert_eq!(read_a, word_first);
        assert_eq!(read_b, word_first);
    }

    /// Every unit is placed once, even for input no logical text draws.
    #[test]
    fn a_line_nothing_draws_still_places_every_unit() {
        // A lone PDF, an RLE and a tab around digits: drawn by no rule.
        let units = ["\u{202C}", "1", "\u{202B}", "\t", "\u{661}", "(", ")"];
        for direction in [
            BaseDirection::LeftToRight,
            BaseDirection::RightToLeft,
            BaseDirection::Auto,
        ] {
            let mut order = logical_order(&units, direction);
            order.sort_unstable();
            assert_eq!(order, (0..units.len()).collect::<Vec<_>>());
        }
        assert!(logical_order(&[], BaseDirection::Auto).is_empty());
    }

    /// P2 read off a drawn line: the two ends, then the majority.
    #[test]
    fn the_drawn_direction_is_read_from_both_ends() {
        let dir = |text: &str| {
            let units: Vec<String> = text.chars().map(String::from).collect();
            let borrowed: Vec<&str> = units.iter().map(String::as_str).collect();
            drawn_direction(&borrowed)
        };
        // An English line holding a longer Arabic word.
        assert_eq!(
            dir("a \u{645}\u{62D}\u{628}\u{645}\u{62D}\u{628} b"),
            BaseDirection::LeftToRight
        );
        // N'Ko, whose letters are R and sit in no Hebrew or Arabic block.
        assert_eq!(dir(".\u{7CC} \u{7CB}\u{7CA}"), BaseDirection::RightToLeft);
        // Adlam.
        assert_eq!(dir("\u{1E922}\u{1E923}"), BaseDirection::RightToLeft);
        // The ends disagree: the majority.
        assert_eq!(dir("ab \u{5D2}\u{5D1}\u{5D0}"), BaseDirection::RightToLeft);
        assert_eq!(dir("abc \u{5D1}\u{5D0}"), BaseDirection::LeftToRight);
        // No strong character at all.
        assert_eq!(dir("12 %"), BaseDirection::LeftToRight);
    }

    /// `units` read in `order`.
    fn read_in(units: &[&str], order: &[usize]) -> String {
        order
            .iter()
            .filter_map(|at| units.get(*at).copied())
            .collect()
    }

    /// `text`, one unit per character, as `direction`'s paragraph draws it.
    fn drawn(text: &str, direction: BaseDirection) -> String {
        drawn_and_read(text, direction).0
    }

    /// **The comma tie-break** (ruling 14, amended 10 October 2026): a
    /// right-to-left word and the punctuation after it, alone on a line of a
    /// left-to-right paragraph, read with the punctuation trailing.
    ///
    /// The line holds no left-to-right character, so its two ends agree and
    /// were read as right to left, which put the comma first: Moby-Dick's
    /// etymology cell `חו,` read back `,חו`. A right-to-left paragraph that
    /// opens with the comma draws the same line, so both readings draw it —
    /// asserted for each, because the tie-break chooses between two answers
    /// the forward check accepts and must never pick one it refuses.
    #[test]
    fn a_right_to_left_word_and_its_punctuation_alone_read_left_to_right() {
        for typed in [
            // Moby-Dick's Hebrew, and an Arabic word; CS.
            "\u{5D7}\u{5D5},",
            "\u{62D}\u{648}\u{62A},",
            "\u{5E9}\u{5DC}\u{5D5}\u{5DD}.",
            "\u{62D}\u{648}\u{62A}\u{60C}",
            // ON, ES, ET.
            "\u{5D7}\u{5D5}?",
            "\u{5D7}\u{5D5})",
            "\u{5D7}\u{5D5}-",
            "\u{5D7}\u{5D5}%",
            // Two words, and two marks after them.
            "\u{5D7}\u{5D5} \u{5E9}\u{5DC}\u{5D5}\u{5DD}.\u{201D}",
        ] {
            let line = drawn(typed, BaseDirection::LeftToRight);
            let units: Vec<String> = line.chars().map(String::from).collect();
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            assert_eq!(
                drawn_direction(&units),
                BaseDirection::LeftToRight,
                "{typed:?} drawn {line:?}"
            );
            assert_eq!(
                read_in(&units, &logical_order(&units, BaseDirection::Auto)),
                typed
            );
            // Both readings draw the line as it stands.
            for direction in [BaseDirection::LeftToRight, BaseDirection::RightToLeft] {
                let read = read_in(&units, &logical_order(&units, direction));
                assert_eq!(drawn(&read, direction), line, "{typed:?} {direction:?}");
            }
        }
        // What draws nothing is passed over at either end: a space, a
        // no-break space (`CS`, as a digit separator), a joiner (`BN`).
        for line in [
            "\u{5D5}\u{5D7}, ",
            "\u{5D5}\u{5D7},\u{A0}",
            "\u{5D5}\u{5D7},\u{200D}",
            " \u{5D5}\u{5D7},",
        ] {
            let units: Vec<String> = line.chars().map(String::from).collect();
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            assert_eq!(
                drawn_direction(&units),
                BaseDirection::LeftToRight,
                "{line:?}"
            );
        }
    }

    /// **And every other line keeps the rule it had.** A right-to-left line
    /// ending in a full stop draws it leftmost; a line holding any `L`
    /// character is out of the tie-break's reach, whatever its ends —
    /// including one whose strong characters are right to left at both ends
    /// and Latin only between them, which the ends alone would have handed
    /// to the tie-break; a number, a mark or a blank at the right is not
    /// punctuation, and punctuation at the left is not a strong right-to-left
    /// character.
    #[test]
    fn the_tie_break_reaches_no_other_line() {
        let dir = |line: &str| {
            let units: Vec<String> = line.chars().map(String::from).collect();
            let borrowed: Vec<&str> = units.iter().map(String::as_str).collect();
            drawn_direction(&borrowed)
        };
        let shalom = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
        // As a right-to-left paragraph types and draws it.
        for typed in [
            format!("{shalom}."),
            format!("\u{2014} a {shalom}"),
            format!("\u{2014} {shalom} a \u{5D7}\u{5D5}"),
            format!("{shalom} 12"),
            format!("({shalom}),"),
        ] {
            let line = drawn(&typed, BaseDirection::RightToLeft);
            assert_eq!(dir(&line), BaseDirection::RightToLeft, "{typed:?} {line:?}");
        }
        // A right-to-left word and its punctuation with a Latin letter on the
        // line, at an end of its strong characters or between them: the ends,
        // then the majority, as before the tie-break.
        assert_eq!(dir("\u{5D5}\u{5D7} a ,"), BaseDirection::RightToLeft);
        assert_eq!(
            dir("\u{5D5}\u{5D7} a \u{5DD}\u{5D5}\u{5DC}\u{5E9},"),
            BaseDirection::RightToLeft
        );
        // At the right a European and an Arabic digit, a mark and a no-break
        // space; at the left an opening quote.
        for line in [
            "\u{5D5}\u{5D7} 5",
            "\u{5D5}\u{5D7} \u{661}",
            "\u{5D5}\u{5D7}\u{5B8}",
            "\u{5D5}\u{5D7}\u{A0}",
            "\u{201C}\u{5D5}\u{5D7},",
        ] {
            assert_eq!(dir(line), BaseDirection::RightToLeft, "{line:?}");
        }
    }

    /// `text` as one-character units.
    fn chars(text: &str) -> Vec<String> {
        text.chars().map(String::from).collect()
    }

    /// **The tie-break takes left to right only where a left-to-right
    /// paragraph draws the line** (review of the amendment). A right-to-left
    /// paragraph's line that opens with a mark and a European number is drawn
    /// with the number next to the mark, `טירפ )1(`: the tie-break's shape,
    /// which no left-to-right paragraph draws. Taken left to right regardless,
    /// [`logical_order`] found no order that checks and returned its first
    /// candidate, `פריט )1(`, which draws another line. Such a line is right to
    /// left again and reads as typed. The last is Persian digits, which are
    /// `EN` too.
    #[test]
    fn a_mark_and_a_number_opening_a_right_to_left_line_read_as_typed() {
        let shalom = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
        for typed in [
            "(1) \u{5E4}\u{5E8}\u{5D9}\u{5D8}".to_owned(),
            format!("\u{2014} 2026 {shalom}"),
            "\u{2022} 5 \u{5EA}\u{5E4}\u{5D5}\u{5D7}\u{5D9}\u{5DD}".to_owned(),
            format!("- 5 {shalom}"),
            "\u{2014} \u{6F1} \u{628}\u{646}\u{62F}".to_owned(),
        ] {
            let line = drawn(&typed, BaseDirection::RightToLeft);
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            assert!(
                read_back(&units, BaseDirection::LeftToRight).is_err(),
                "a left-to-right paragraph draws {line:?}"
            );
            assert_eq!(
                drawn_direction(&units),
                BaseDirection::RightToLeft,
                "{typed:?} drawn {line:?}"
            );
            let read = read_in(&units, &logical_order(&units, BaseDirection::Auto));
            assert_eq!(read, typed, "drawn {line:?}");
        }
    }

    /// **An opening bracket or quotation mark at the right is not trailing
    /// punctuation** (review of the amendment). In a left-to-right reading a
    /// mark after the word closes, so a mark that opens, drawn at the right,
    /// is the first character of a right-to-left line. `(١) بند` is drawn
    /// `دنب )١(`, and a left-to-right reading draws it too, `١) بند(`, which
    /// the tie-break took; `“שלום`, drawn `םולש“`, read `שלום“`. `Ps` and
    /// `Pi` from the vendored `UnicodeData.txt` keep them right to left.
    #[test]
    fn an_opening_bracket_or_quotation_mark_at_the_right_does_not_trail() {
        let shalom = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
        for typed in [
            "(\u{661}) \u{628}\u{646}\u{62F}".to_owned(),
            format!("\u{201C}{shalom}"),
            format!("\u{201E}{shalom}"),
            "\u{AB}\u{645}\u{631}\u{62D}\u{628}\u{627}".to_owned(),
            format!("({shalom}"),
        ] {
            let line = drawn(&typed, BaseDirection::RightToLeft);
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            assert!(!trailing_punctuation(&units), "{typed:?} drawn {line:?}");
            assert_eq!(
                drawn_direction(&units),
                BaseDirection::RightToLeft,
                "{typed:?} drawn {line:?}"
            );
            let read = read_in(&units, &logical_order(&units, BaseDirection::Auto));
            assert_eq!(read, typed, "drawn {line:?}");
        }
        // A closing one still trails a quoted word, as a comma does.
        let units = ["\u{5D5}", "\u{5D7}", "\u{201D}"];
        assert_eq!(drawn_direction(&units), BaseDirection::LeftToRight);
    }

    /// [`BLANK_SEPARATORS`] is every `White_Space` character whose class is
    /// not `WS`, `S` or `B`: the standard library's `char::is_whitespace`
    /// is that property, and this crate's `Bidi_Class` the vendored one.
    #[test]
    fn the_blank_separators_are_the_white_space_that_bidi_class_misses() {
        let missed: Vec<char> = (0..=0x0010_FFFF_u32)
            .filter_map(char::from_u32)
            .filter(|c| {
                c.is_whitespace()
                    && !matches!(bidi_class(*c), BidiClass::WS | BidiClass::S | BidiClass::B)
            })
            .collect();
        assert_eq!(missed, BLANK_SEPARATORS);
    }

    /// **The price, named** (ruling 14's amendment): a lone line of a
    /// right-to-left paragraph that opens with punctuation — a dialogue
    /// dash — is drawn as the tie-break's shape and reads with the dash at
    /// its end. Both readings draw it; this is the one the owner chose. So do
    /// three more shapes: Arabic-Indic digits, `AN`, which a left-to-right
    /// paragraph draws next to the mark too; a number that is not next to the
    /// mark; and a straight quotation mark, which closes as often as it opens.
    #[test]
    fn a_right_to_left_line_opening_with_a_dash_reads_it_trailing() {
        let shalom = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
        let band = "\u{628}\u{646}\u{62F}";
        for (typed, read) in [
            (format!("\u{2014} {shalom}"), format!("{shalom} \u{2014}")),
            (
                format!("\u{2014} \u{661} {band}"),
                format!("\u{661} {band} \u{2014}"),
            ),
            (
                format!("\u{2014} {shalom} 5 \u{5D7}\u{5D5}"),
                format!("{shalom} 5 \u{5D7}\u{5D5} \u{2014}"),
            ),
            (format!("\"{shalom}"), format!("{shalom}\"")),
        ] {
            let line = drawn(&typed, BaseDirection::RightToLeft);
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            assert_eq!(
                drawn_direction(&units),
                BaseDirection::LeftToRight,
                "{typed:?}"
            );
            assert_eq!(
                read_in(&units, &logical_order(&units, BaseDirection::Auto)),
                read
            );
            assert_eq!(drawn(&read, BaseDirection::LeftToRight), line, "{typed:?}");
            assert_eq!(
                read_in(&units, &logical_order(&units, BaseDirection::RightToLeft)),
                typed,
                "a reader told the paragraph's direction still reads it as typed"
            );
        }
    }

    /// **A pair drawn facing outward reads in the direction that pairs it**
    /// (ruling 14's bracket tie-break, amended 10 October 2026; ROADMAP K-11).
    ///
    /// A right-to-left paragraph narrowed so that a line opens with a
    /// bracketed Latin run: `(histone deacetylase). وقد أصبحنا`, eighteen
    /// Latin letters and eleven Arabic ones. Its ends disagree and the
    /// majority is left to right, and a left-to-right paragraph draws
    /// `وقد أصبحنا .)histone deacetylase(` as that line stands, so the forward
    /// check could not refuse it. The pair is drawn facing outward, which
    /// only a pair at a right-to-left level is, and the right-to-left reading
    /// pairs it.
    #[test]
    fn a_pair_drawn_facing_outward_reads_in_the_direction_that_pairs_it() {
        let typed = "(histone deacetylase). \u{648}\u{642}\u{62F} \u{623}\u{635}\u{628}\u{62D}\u{646}\u{627}";
        let line = drawn(typed, BaseDirection::RightToLeft);
        assert!(line.contains(")histone deacetylase("), "{line:?}");
        let units = chars(&line);
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        let other = read_in(&units, &logical_order(&units, BaseDirection::LeftToRight));
        assert_eq!(
            drawn(&other, BaseDirection::LeftToRight),
            line,
            "a left-to-right paragraph draws the line too"
        );
        assert_eq!(drawn_direction(&units), BaseDirection::RightToLeft);
        assert_eq!(
            read_in(&units, &logical_order(&units, BaseDirection::Auto)),
            typed
        );
    }

    /// **A pair drawn facing inward decides nothing.** With no `dir`, a line
    /// with Arabic at both ends and a Latin run in brackets between is a
    /// left-to-right paragraph's, and draws the pair facing inward, `(HDAC)`;
    /// a right-to-left paragraph whose `/ToUnicode` names the mirrored glyph
    /// draws the same picture, and both readings draw the line. The pair is
    /// not counted: the line keeps the direction its ends give it, right to
    /// left — two texts UAX #9 draws alike, which no reader tells apart.
    #[test]
    fn a_pair_drawn_facing_inward_decides_nothing() {
        let typed = "\u{639}\u{62F}\u{629} (HDAC). \u{648}\u{642}\u{62F}";
        let line = drawn(typed, BaseDirection::LeftToRight);
        assert!(line.contains("(HDAC)"), "{line:?}");
        let units = chars(&line);
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        let other = read_in(&units, &logical_order(&units, BaseDirection::RightToLeft));
        assert_eq!(drawn(&other, BaseDirection::RightToLeft), line);
        assert_eq!(drawn_direction(&units), BaseDirection::RightToLeft);
        assert_eq!(
            read_in(&units, &logical_order(&units, BaseDirection::Auto)),
            other
        );
    }

    /// **Brackets that are no pair as typed keep the direction the rules
    /// before the tie-break give the line** — the review of the K-11 fix.
    ///
    /// Each of these left-to-right lines draws a closing bracket left of an
    /// opening one, and a right-to-left paragraph draws the same picture for
    /// another text, in which the two pair facing outward. As first written
    /// the tie-break took that reading wherever it paired more brackets
    /// facing outward and no fewer in all, and read every one of them
    /// backwards: `.and 2) close it )שלום( open the file )Step 1`. A list
    /// marker, a smiley and a parenthesis wrapped onto the next line are
    /// commoner in running text than any line `BidiCharacterTest.txt` holds,
    /// whose lines are each a whole paragraph. The first pairs `(שלום)` read
    /// as typed and `) open the file (` read from the right — one each, so
    /// nothing is said for the other; the rest pair more read from the right,
    /// but with the spaces that stood outside the two brackets inside the
    /// pair, `see( מלחמה )b` — no bracket set against its own text.
    #[test]
    fn brackets_that_are_no_pair_as_typed_keep_the_lines_direction() {
        for typed in [
            "Step 1) open the file (\u{5E9}\u{5DC}\u{5D5}\u{5DD}) and 2) close it.",
            "Great :) \u{5E9}\u{5DC}\u{5D5}\u{5DD} (yes) \u{5E2}\u{5D5}\u{5DC}\u{5DD}.",
            "Peace (\u{639}\u{648}\u{627}\u{645}\u{644} \u{639}\u{62F}\u{629}) and war (\u{648}\u{642}\u{62F}",
            "a) \u{5E9}\u{5DC}\u{5D5}\u{5DD} b) \u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4} (see",
            "x) \u{5E9}\u{5DC}\u{5D5}\u{5DD} (y",
            "The term ( \u{5E9}\u{5DC}\u{5D5}\u{5DD} ) and (",
        ] {
            let line = drawn(typed, BaseDirection::LeftToRight);
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            assert_eq!(
                drawn_direction(&units),
                BaseDirection::LeftToRight,
                "{typed:?}"
            );
            assert_eq!(
                read_in(&units, &logical_order(&units, BaseDirection::Auto)),
                typed
            );
        }
    }

    /// **A reading that pairs no more brackets than the other turns nothing,
    /// whatever faces outward.** `(a)א(` in a left-to-right paragraph pairs
    /// `(a)`; a right-to-left paragraph draws the same picture for `(א)a(`,
    /// whose one pair faces outward and is set against its letter. One pair
    /// each: the ends and the majority (a tie, so left to right) keep the
    /// line. `BidiCharacterTest.txt` has 1 250 lines of this kind, typed with
    /// brackets that do not all pair, that a tie-break asking for no fewer
    /// pairs in all read worse, whether or not it asked the pairs to hug.
    #[test]
    fn a_reading_pairing_no_more_brackets_turns_nothing() {
        let typed = "(a)\u{5D0}(";
        let line = drawn(typed, BaseDirection::LeftToRight);
        let units = chars(&line);
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        let other = read_in(&units, &logical_order(&units, BaseDirection::RightToLeft));
        assert_eq!(
            other, "(\u{5D0})a(",
            "the other reading pairs one, facing outward"
        );
        assert_eq!(drawn(&other, BaseDirection::RightToLeft), line);
        assert_eq!(drawn_direction(&units), BaseDirection::LeftToRight);
        assert_eq!(
            read_in(&units, &logical_order(&units, BaseDirection::Auto)),
            typed
        );
    }

    /// **The price: brackets typed closing before opening, each set against
    /// the text between them, read as the other direction pairs them.**
    /// `x)שלום(y` in a left-to-right paragraph is drawn `x)םולש(y`, and a
    /// right-to-left paragraph draws `y(שלום)x` the same, its pair facing
    /// outward and hugging the word. That reading pairs a bracket and the
    /// typed text pairs none, so it is taken. `BidiCharacterTest.txt` has no
    /// line that reads worse for it, nor has the review's probe of wrapped
    /// sentences; a text typed so is a fragment, not prose.
    #[test]
    fn brackets_typed_closing_first_against_a_word_read_as_the_other_direction_pairs_them() {
        let typed = "x)\u{5E9}\u{5DC}\u{5D5}\u{5DD}(y";
        let line = drawn(typed, BaseDirection::LeftToRight);
        let units = chars(&line);
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        assert_eq!(drawn_direction(&units), BaseDirection::RightToLeft);
        let read = read_in(&units, &logical_order(&units, BaseDirection::Auto));
        assert_eq!(read, "y(\u{5E9}\u{5DC}\u{5D5}\u{5DD})x");
        assert_eq!(drawn(&read, BaseDirection::RightToLeft), line);
    }

    /// **Two pairs side by side are not broken for the bracket between
    /// them.** `()()` drawn left to right holds a `)` left of a `(`; read from
    /// the right it pairs that one, facing outward, and loses the two pairs it
    /// was typed with. A reading that pairs fewer brackets in all does not
    /// win: 1 376 of `BidiCharacterTest.txt`'s lines, every bracket of them
    /// paired as typed, read worse before that was asked.
    #[test]
    fn two_pairs_side_by_side_are_not_broken_for_the_bracket_between_them() {
        for typed in ["()()", "a(b)\u{5D0}(c)"] {
            let line = drawn(typed, BaseDirection::LeftToRight);
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            let chosen = drawn_direction(&units);
            assert_eq!(
                read_in(&units, &logical_order(&units, chosen)),
                typed,
                "{typed:?} drawn {line:?}, read {chosen:?}"
            );
        }
    }

    /// **A pair that may have taken its level from the line before turns no
    /// line** ([`OpenedBefore`]; the third review of the K-11 fix).
    ///
    /// A justified English paragraph quoting Hebrew in brackets, `… Words
    /// שלום עולם (מלחמה) and more English text, …`, wraps so that a line
    /// opens with `(הארץ)`: N0 resolved that pair by the Hebrew word that
    /// ended the line above, so it is drawn facing outward at the line's left
    /// end, `)ץראה(`. Read from the right the line pairs it and `(מלחמה)`; as
    /// the strong letters choose, left to right, only `(מלחמה)`. The bracket
    /// tie-break took the reading from the right, which reads the English
    /// backwards. That pair opens the line a left-to-right reading starts
    /// from and encloses no Latin letter, so it is no evidence: the reading as
    /// chosen pairs a bracket facing outward of its own and the line's Latin
    /// letters outnumber its Hebrew ones, and the line keeps its direction.
    /// The paragraph's last line, `(מלחמה) and more English text,.`, pairs
    /// nothing else; its Latin letters are more than three times its Hebrew
    /// ones and eight more, and it keeps its direction too.
    #[test]
    fn a_pair_the_line_above_resolved_turns_no_line() {
        let ltr = BaseDirection::LeftToRight;
        let rtl = BaseDirection::RightToLeft;
        let rest = "and more English text, Words \u{5E9}\u{5DC}\u{5D5}\u{5DD} \
                    \u{5E2}\u{5D5}\u{5DC}\u{5DD} (\u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4}) \
                    and more English text,";
        let opened = ")\u{5E5}\u{5E8}\u{5D0}\u{5D4}(";
        let last = ")\u{5D4}\u{5DE}\u{5D7}\u{5DC}\u{5DE}( and more English text,.";
        for (line, pair) in [
            (
                format!("{opened} {}", drawn(rest, ltr)),
                "(\u{5D4}\u{5D0}\u{5E8}\u{5E5})",
            ),
            (last.to_owned(), "(\u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4})"),
        ] {
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            let reading = read_in(&units, &logical_order(&units, rtl));
            assert!(
                reading.contains(pair),
                "read from the right, the line pairs the pair that opens it: {reading:?}"
            );
            assert_eq!(drawn(&reading, rtl), line, "and both directions draw it");
            assert_eq!(drawn_direction(&units), ltr, "{line:?}");
            let read = read_in(&units, &logical_order(&units, BaseDirection::Auto));
            assert!(read.contains("and more English text,"), "{read:?}");
        }
        // Where the line pairs no other bracket facing outward and its
        // letters are not so many more, the pair still counts — a line of
        // `BidiCharacterTest.txt`, a whole right-to-left paragraph, ends so.
        let units = chars(")\u{5D0}( ab");
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        assert_eq!(drawn_direction(&units), rtl);
    }

    /// **[`drawn_reading`] is [`drawn_direction`] and [`logical_order`] in
    /// one, and reads a line no more often than choosing its direction
    /// does** — the review of the K-11 fix. A line is searched once or, where
    /// it draws a closing bracket left of an opening one, once in each
    /// direction, and the order returned is the one deciding found; asked of
    /// [`drawn_direction`] and then of [`logical_order`] for the direction
    /// chosen, such a line was searched three times. `turned_by_brackets` is
    /// set on the lines the bracket tie-break turns and on no other.
    #[test]
    fn a_line_is_read_once_in_the_direction_chosen() {
        let k11 = "(histone deacetylase). \u{648}\u{642}\u{62F} \u{623}\u{635}\u{628}\u{62D}\u{646}\u{627}";
        let rtl = BaseDirection::RightToLeft;
        let ltr = BaseDirection::LeftToRight;
        for (typed, direction, turned, outward) in [
            (k11, rtl, true, true),
            ("x)\u{5E9}\u{5DC}\u{5D5}\u{5DD}(y", ltr, true, true),
            ("(a)\u{5D0}(", ltr, false, true),
            (
                "Great :) \u{5E9}\u{5DC}\u{5D5}\u{5DD} (yes) \u{5E2}.",
                ltr,
                false,
                true,
            ),
            (
                "\u{639}\u{62F}\u{629} (HDAC). \u{648}\u{642}\u{62F}",
                ltr,
                false,
                false,
            ),
            ("\u{5D7}\u{5D5},", ltr, false, false),
            ("\u{5E9}\u{5DC}\u{5D5}\u{5DD} abc", rtl, false, false),
            ("abc", ltr, false, false),
        ] {
            let line = drawn(typed, direction);
            let units = chars(&line);
            let units: Vec<&str> = units.iter().map(String::as_str).collect();
            let (reading, searched) = searches(|| drawn_reading(&units));
            let chosen = drawn_direction(&units);
            assert_eq!(reading.direction, chosen, "{typed:?}");
            assert_eq!(reading.order, logical_order(&units, chosen), "{typed:?}");
            assert_eq!(reading.turned_by_brackets, turned, "{typed:?}");
            assert_eq!(searched, if outward { 2 } else { 1 }, "{typed:?}");
            if outward {
                let (_, apart) = searches(|| logical_order(&units, drawn_direction(&units)));
                assert_eq!(apart, 3, "{typed:?}");
            }
        }
    }
}
