//! The rearrangement subtable, `morx` type 0.
//!
//! Apple's *TrueType Reference Manual*, *The 'morx' table*, "Indic
//! Rearrangement Subtable": "the whole subtable is just a state table", its
//! entries a new state and flags, four bytes, using "no per-glyph tables".
//! `markFirst` (`0x8000`) makes "the current glyph the first glyph to be
//! rearranged", `markLast` (`0x2000`) "the last glyph to be rearranged", and
//! the low four bits are the verb, applied to the glyphs from the first to the
//! last, "The first and last glyph values are set before the rearrangement
//! specified by the value of the verb field is applied". The verb table:
//!
//! | Verb | | Verb | | Verb | | Verb | |
//! | --- | --- | --- | --- | --- | --- | --- | --- |
//! | 0 | no change | 4 | `ABx ⇒ xAB` | 8 | `AxCD ⇒ CDxA` | 12 | `ABxCD ⇒ CDxAB` |
//! | 1 | `Ax ⇒ xA` | 5 | `ABx ⇒ xBA` | 9 | `AxCD ⇒ DCxA` | 13 | `ABxCD ⇒ CDxBA` |
//! | 2 | `xD ⇒ Dx` | 6 | `xCD ⇒ CDx` | 10 | `ABxD ⇒ DxAB` | 14 | `ABxCD ⇒ DCxAB` |
//! | 3 | `AxD ⇒ DxA` | 7 | `xCD ⇒ DCx` | 11 | `ABxD ⇒ DxBA` | 15 | `ABxCD ⇒ DCxBA` |
//!
//! "The uppercase letters 'A', 'B', 'C', and 'D' represent individual glyphs,
//! and 'x' represents an arbitrary sequence of glyphs", the empty one
//! included. `docs/design/shaping.md`, *`morx`*, settles what the chapter
//! leaves open, each reading held by a text-rendering-tests section:
//!
//! - The marks are positions in layout order, whichever way the machine
//!   walks, and the span runs from the leftmost of the two to the rightmost.
//! - Where no glyph was marked first, the span starts at the first glyph the
//!   machine met (`MORX-7`); where none was marked last, there is no span and
//!   a verb moves nothing (`MORX-3`).
//! - At the end of text the current glyph is the last one met, so `markLast`
//!   there extends the span to it (`MORX-11`), and the verb acts once
//!   (`MORX-10`, `MORX-17`).
//! - A span with fewer glyphs than its verb names is left as it is
//!   (`MORX-5`), and the marks stay where they were after a verb, so a later
//!   verb acts on the same positions (`MORX-6`).
//!
//! A rearranged span's clusters are merged as [`Buffer`]'s reordering merges
//! them, every glyph of it taking the smallest, so they stay monotonic; a verb
//! that leaves every glyph where it was merges nothing.

use super::state::{Cursor, Entry, StateTable, Traversal};
use super::Site;
use crate::buffer::Buffer;
use crate::limits::Limits;
use crate::Warning;

/// The bytes of a rearrangement entry: its new state and its flags.
pub(crate) const ENTRY_SIZE: usize = 4;
/// The flag making the current glyph the first of the span.
const MARK_FIRST: u16 = 0x8000;
/// The flag making the current glyph the last of the span.
const MARK_LAST: u16 = 0x2000;
/// The verb, the flags' low four bits.
const VERB: u16 = 0x000F;

/// Runs a rearrangement machine over `buffer`, walking it `traversal`'s way;
/// see the module documentation.
///
/// # Errors
///
/// The driver's: the machine stopped at its ceiling, or at a state, entry or
/// class its tables do not hold, the run as it left it.
pub(crate) fn run(
    table: &StateTable<'_>,
    buffer: &mut Buffer,
    traversal: Traversal,
    limits: &Limits,
    site: Site,
) -> Result<(), Warning> {
    let mut span = Span {
        start: traversal
            .first(buffer.len())
            .filter(|at| *at < buffer.len()),
        first: None,
        last: None,
    };
    let mut act =
        |buffer: &mut Buffer, entry: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
            span.act(buffer, entry.flags, cursor.glyph);
            Ok(())
        };
    table.run(buffer, traversal, limits, site, &mut act)
}

/// The marks of a rearrangement machine.
struct Span {
    /// The first glyph the machine met, which the span starts at where no
    /// glyph was marked first.
    start: Option<usize>,
    /// The glyph `markFirst` last named.
    first: Option<usize>,
    /// The glyph `markLast` last named.
    last: Option<usize>,
}

impl Span {
    /// One transition: the marks, then the verb.
    fn act(&mut self, buffer: &mut Buffer, flags: u16, current: Option<usize>) {
        if flags & MARK_FIRST != 0 {
            self.first = current;
        }
        if flags & MARK_LAST != 0 {
            self.last = current;
        }
        let verb = flags & VERB;
        if verb == 0 {
            return;
        }
        let (Some(first), Some(last)) = (self.first.or(self.start), self.last) else {
            return;
        };
        let (low, high) = (first.min(last), first.max(last));
        if high >= buffer.len() {
            return;
        }
        let Some(order) = verb_order(verb, high - low + 1) else {
            return;
        };
        if order.iter().enumerate().all(|(at, from)| at == *from) {
            return;
        }
        buffer.reorder(low..high + 1, &order);
    }
}

/// Where each glyph of a span of `len` glyphs comes from after `verb`: the
/// `n`th glyph of the result is glyph `order[n]` of the span. `None` for a
/// span shorter than the glyphs its verb names, and for verb 0.
///
/// Each verb moves the first one or two glyphs (`A`, `AB`) and the last one
/// or two (`D`, `CD`) past what lies between them (`x`), reversing a pair
/// where the table writes it reversed.
pub(crate) fn verb_order(verb: u16, len: usize) -> Option<Vec<usize>> {
    // (glyphs from the start, their pair reversed, glyphs from the end,
    // their pair reversed), by the chapter's table.
    let (front, front_reversed, back, back_reversed) = match verb {
        1 => (1, false, 0, false),
        2 => (0, false, 1, false),
        3 => (1, false, 1, false),
        4 => (2, false, 0, false),
        5 => (2, true, 0, false),
        6 => (0, false, 2, false),
        7 => (0, false, 2, true),
        8 => (1, false, 2, false),
        9 => (1, false, 2, true),
        10 => (2, false, 1, false),
        11 => (2, true, 1, false),
        12 => (2, false, 2, false),
        13 => (2, true, 2, false),
        14 => (2, false, 2, true),
        15 => (2, true, 2, true),
        _ => return None,
    };
    if len < front + back {
        return None;
    }
    let mut head: Vec<usize> = (0..front).collect();
    if front_reversed {
        head.reverse();
    }
    let mut tail: Vec<usize> = (len - back..len).collect();
    if back_reversed {
        tail.reverse();
    }
    let mut order = tail;
    order.extend(front..len - back);
    order.extend(head);
    Some(order)
}

#[cfg(test)]
mod tests {
    use super::verb_order;

    /// Each verb over the five glyphs `A B x C D`, by the chapter's table.
    #[test]
    fn each_verb_orders_five_glyphs_as_the_table_writes() {
        let expected: [&[usize]; 16] = [
            &[0, 1, 2, 3, 4],
            &[1, 2, 3, 4, 0],
            &[4, 0, 1, 2, 3],
            &[4, 1, 2, 3, 0],
            &[2, 3, 4, 0, 1],
            &[2, 3, 4, 1, 0],
            &[3, 4, 0, 1, 2],
            &[4, 3, 0, 1, 2],
            &[3, 4, 1, 2, 0],
            &[4, 3, 1, 2, 0],
            &[4, 2, 3, 0, 1],
            &[4, 2, 3, 1, 0],
            &[3, 4, 2, 0, 1],
            &[3, 4, 2, 1, 0],
            &[4, 3, 2, 0, 1],
            &[4, 3, 2, 1, 0],
        ];
        assert_eq!(verb_order(0, 5), None);
        for (verb, order) in (1u16..).zip(expected.iter().skip(1)) {
            assert_eq!(verb_order(verb, 5).as_deref(), Some(*order), "verb {verb}");
        }
        for verb in 16..=40u16 {
            assert_eq!(verb_order(verb, 5), None);
        }
    }

    /// A span too short for its verb is no order; one just long enough
    /// moves its named glyphs past an empty `x`.
    #[test]
    fn a_span_too_short_for_its_verb_has_no_order() {
        for (verb, needs) in [
            (1u16, 1usize),
            (2, 1),
            (3, 2),
            (4, 2),
            (5, 2),
            (6, 2),
            (7, 2),
            (8, 3),
            (9, 3),
            (10, 3),
            (11, 3),
            (12, 4),
            (13, 4),
            (14, 4),
            (15, 4),
        ] {
            for len in 0..needs {
                assert_eq!(verb_order(verb, len), None, "verb {verb} over {len}");
            }
            assert!(
                verb_order(verb, needs).is_some(),
                "verb {verb} over {needs}"
            );
        }
        assert_eq!(verb_order(12, 4).as_deref(), Some(&[2, 3, 0, 1][..]));
        assert_eq!(verb_order(3, 2).as_deref(), Some(&[1, 0][..]));
    }
}
