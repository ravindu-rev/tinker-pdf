//! The glyph insertion subtable, `morx` type 5.
//!
//! Apple's *TrueType Reference Manual*, *The 'morx' table*, "Glyph Insertion
//! Subtable": it "permits extra glyphs (which have no corresponding
//! characters in the backing store) to be inserted into a layout shape". The
//! subtable is an extended state table and a `UInt32` `insertionActionOffset`,
//! "Byte offset from stateHeader to the start of the insertion glyph table";
//! its entries are eight bytes, a new state, flags, a `currentInsertIndex`
//! and a `markedInsertIndex`, each a "Zero-based index into the insertion
//! glyph table", `0xFFFF` for none. The flags:
//!
//! | Mask | Name | Meaning |
//! | --- | --- | --- |
//! | `0x8000` | `setMark` | "mark the current glyph" |
//! | `0x4000` | `dontAdvance` | "don't update the glyph index before going to the new state" |
//! | `0x2000` | `currentIsKashidaLike` | the current glyph's list inserted kashida-like, split-vowel-like where clear |
//! | `0x1000` | `markedIsKashidaLike` | the same of the marked glyph's list |
//! | `0x0800` | `currentInsertBefore` | "insertions are to be made to the left of the current glyph", to its right where clear |
//! | `0x0400` | `markedInsertBefore` | the same of the marked glyph |
//! | `0x03E0` | `currentInsertCount` | how many glyphs, at most 31 |
//! | `0x001F` | `markedInsertCount` | how many glyphs, at most 31 |
//!
//! "Left" and "right" are positions in layout order, whichever way the
//! machine walks (`docs/design/shaping.md`, *`morx`*).
//!
//! # Positions, as the test sections pin them
//!
//! The chapter states the machine over "the glyph index", and the
//! text-rendering-tests sections settle what an index means once glyphs are
//! inserted around it:
//!
//! - **The mark is a position.** Glyphs inserted before the marked glyph do
//!   not move the mark: it names whatever now stands where the marked glyph
//!   stood (`MORX-30`, whose second case inserts after the first of the
//!   glyphs an earlier insertion put before the marked one).
//! - **`setMark` on an inserting transition marks the position the current
//!   glyph held when the transition began**, before its insertions moved it
//!   (`MORX-31`).
//! - **Where no glyph has been marked, the mark is the first glyph the
//!   machine met** (`MORX-32`).
//! - **The current glyph moves with what is inserted before it**, and without
//!   `dontAdvance` the machine goes on past it and past the glyphs inserted
//!   after it, so an insertion is not read again (`MORX-31`, `MORX-33`).
//! - **With `dontAdvance` the machine stays at its position**, which a
//!   marked insertion before it moves and the current glyph's own insertion
//!   does not: it meets the current glyph again where that glyph's list went
//!   after it (`MORX-35`), and "the first one inserted" where the list went
//!   before it, the chapter's own sentence.
//!
//! In one transition the marked glyph's list is inserted first and the
//! current glyph's second, as the contextual subtable substitutes the marked
//! glyph before the current one; a walk backwards goes on past the glyphs
//! the current glyph's list put on its left, the side the walk goes on to.
//!
//! # Clusters, and what the flags keep
//!
//! An inserted glyph takes the cluster of the glyph it is inserted at. A
//! split-vowel-like list, the chapter's glyphs that "do not select with that
//! glyph, but rather with some other glyph", is "associated with the same
//! character offset" as the other glyph of its action — the current glyph for
//! a list inserted at the mark, the marked one for a list inserted at the
//! current glyph — so the clusters from the inserted glyphs to that glyph are
//! merged, as a rearranged span's are, and stay monotonic. Each inserted
//! glyph carries how it was inserted ([`crate::buffer::Insertion`]), which
//! changes no output here.
//!
//! # Bounds
//!
//! An insertion that would take the run past [`Limits::max_glyphs`] is
//! refused, as a `GSUB` multiple substitution's is, and named
//! ([`Warning::InsertionBudgetExceeded`]); a list the insertion glyph table
//! does not hold is named as malformed. Either way the transition's other
//! insertion and the machine go on, and a machine that inserts for ever
//! stops at [`Limits::max_transitions`] (`MORX-36`).

use super::state::{Cursor, Entry, StateTable, Traversal};
use super::Site;
use crate::buffer::{Buffer, Insertion};
use crate::limits::Limits;
use crate::read::Bytes;
use crate::Warning;

/// The bytes of an insertion entry: new state, flags and the two indices.
pub(crate) const ENTRY_SIZE: usize = 8;
/// The flag marking the current glyph.
const SET_MARK: u16 = 0x8000;
/// The flag inserting the current glyph's list kashida-like.
const CURRENT_IS_KASHIDA_LIKE: u16 = 0x2000;
/// The flag inserting the marked glyph's list kashida-like.
const MARKED_IS_KASHIDA_LIKE: u16 = 0x1000;
/// The flag inserting the current glyph's list on its left.
const CURRENT_INSERT_BEFORE: u16 = 0x0800;
/// The flag inserting the marked glyph's list on its left.
const MARKED_INSERT_BEFORE: u16 = 0x0400;
/// The current glyph's count, bits 5 to 9.
const CURRENT_INSERT_COUNT: u16 = 0x03E0;
/// The marked glyph's count, bits 0 to 4.
const MARKED_INSERT_COUNT: u16 = 0x001F;
/// The index naming no list.
const NONE: u16 = 0xFFFF;
/// Where the `insertionActionOffset` sits, after the `STXHeader`.
const INSERTION_ACTION: usize = 16;

/// Runs an insertion machine over `buffer`, walking it `traversal`'s way;
/// `body` is the subtable's, the state table's header at its start. An
/// insertion refused is recorded in `warnings` and the machine goes on.
///
/// # Errors
///
/// [`Warning::MalformedAatSubtable`] where the subtable holds no
/// `insertionActionOffset`, or one past its end, and the driver's: the
/// machine stopped at its ceiling, or at a state, entry or class its tables
/// do not hold, the run as it left it.
pub(crate) fn run(
    table: &StateTable<'_>,
    body: &[u8],
    buffer: &mut Buffer,
    traversal: Traversal,
    limits: &Limits,
    site: Site,
    warnings: &mut Vec<Warning>,
) -> Result<(), Warning> {
    let data = Bytes::new(body);
    let lists = data
        .u32(INSERTION_ACTION)
        .and_then(|offset| usize::try_from(offset).ok())
        .and_then(|offset| data.at(offset))
        .ok_or(Warning::MalformedAatSubtable { site })?;
    let inserter = Inserter {
        lists,
        site,
        max_glyphs: limits.max_glyphs,
    };
    let mut mark: Option<usize> = traversal
        .first(buffer.len())
        .filter(|at| *at < buffer.len());
    let mut act =
        |buffer: &mut Buffer, entry: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
            let Some(slot) = cursor.glyph else {
                // An empty run: no glyph to insert at, and none to mark.
                return Ok(());
            };
            let flags = entry.flags;
            let mut moved = Moved {
                slot,
                ..Moved::default()
            };
            // The marked glyph's list, inserted beside the glyph the mark
            // names and associated with the current one.
            let count = usize::from(flags & MARKED_INSERT_COUNT);
            let index = entry.field(1).unwrap_or(NONE);
            if count > 0 && index != NONE {
                if let Some(at) = mark {
                    let request = Request {
                        index,
                        count,
                        beside: at,
                        before: flags & MARKED_INSERT_BEFORE != 0,
                        kind: kind(flags & MARKED_IS_KASHIDA_LIKE != 0),
                    };
                    inserter.insert(buffer, &request, &mut moved, Other::Current, warnings);
                }
            }
            // The current glyph's, beside it and associated with the mark.
            let count = usize::from((flags & CURRENT_INSERT_COUNT) >> 5);
            let index = entry.field(0).unwrap_or(NONE);
            if count > 0 && index != NONE {
                let request = Request {
                    index,
                    count,
                    beside: moved.current(),
                    before: flags & CURRENT_INSERT_BEFORE != 0,
                    kind: kind(flags & CURRENT_IS_KASHIDA_LIKE != 0),
                };
                let other = mark.map_or(Other::None, Other::Mark);
                inserter.insert(buffer, &request, &mut moved, other, warnings);
            }
            if flags & SET_MARK != 0 {
                mark = Some(slot);
            }
            cursor.glyph = Some(if entry.dont_advance() {
                moved.position()
            } else {
                match traversal {
                    // The driver steps past what this leaves the cursor on.
                    Traversal::Forward => moved.last_after(),
                    Traversal::Backward => moved.position(),
                }
            });
            Ok(())
        };
    table.run(buffer, traversal, limits, site, &mut act)
}

/// The kind of insertion a flag's state asks for.
const fn kind(kashida_like: bool) -> Insertion {
    if kashida_like {
        Insertion::KashidaLike
    } else {
        Insertion::SplitVowelLike
    }
}

/// What one transition's insertions did around the current glyph, in
/// positions of the run as it now stands.
#[derive(Clone, Copy, Debug, Default)]
struct Moved {
    /// Where the current glyph stood when the transition began.
    slot: usize,
    /// Glyphs inserted before that position, which moved it.
    shift: usize,
    /// Glyphs inserted at it, between it and the current glyph.
    at_slot: usize,
    /// Glyphs inserted right after the current glyph.
    after: usize,
}

impl Moved {
    /// The machine's position: where the current glyph stood, moved by what
    /// was inserted before it and not by the current glyph's own list.
    const fn position(&self) -> usize {
        self.slot + self.shift
    }

    /// Where the current glyph now stands.
    const fn current(&self) -> usize {
        self.position() + self.at_slot
    }

    /// The last glyph inserted right after the current glyph, or the current
    /// glyph itself: what a forward walk steps on from.
    const fn last_after(&self) -> usize {
        self.current() + self.after
    }

    /// Accounts for `count` glyphs inserted at position `at`.
    fn record(&mut self, at: usize, count: usize) {
        if at < self.position() {
            self.shift += count;
        } else if at <= self.current() {
            self.at_slot += count;
        } else if at <= self.last_after() + 1 {
            self.after += count;
        }
    }
}

/// The glyph a split-vowel-like list is associated with, besides the glyph
/// it is inserted at.
#[derive(Clone, Copy, Debug)]
enum Other {
    /// The current glyph, wherever the insertion leaves it.
    Current,
    /// The glyph at the mark's position.
    Mark(usize),
    /// None: an empty run has no mark.
    None,
}

/// One list to insert.
struct Request {
    /// Its first glyph's index in the insertion glyph table.
    index: u16,
    /// How many glyphs.
    count: usize,
    /// The glyph it is inserted beside.
    beside: usize,
    /// Whether it goes on that glyph's left.
    before: bool,
    /// How it is inserted.
    kind: Insertion,
}

/// A subtable's insertion glyph table, and the ceiling growth is held to.
struct Inserter<'a> {
    lists: Bytes<'a>,
    site: Site,
    max_glyphs: usize,
}

impl Inserter<'_> {
    /// Inserts `request`'s list beside its glyph, each taking that glyph's
    /// cluster, and accounts for it in `moved`; a split-vowel-like list has
    /// its clusters merged with `other`'s. A list the table does not hold, or
    /// one past the glyph ceiling, is named and not inserted.
    fn insert(
        &self,
        buffer: &mut Buffer,
        request: &Request,
        moved: &mut Moved,
        other: Other,
        warnings: &mut Vec<Warning>,
    ) {
        let site = self.site;
        let Some(cluster) = buffer.glyph(request.beside).map(|glyph| glyph.cluster) else {
            return;
        };
        let mut glyphs: Vec<u16> = Vec::with_capacity(request.count);
        for n in 0..request.count {
            let Some(glyph) = usize::from(request.index)
                .checked_add(n)
                .and_then(|at| self.lists.u16_at(0, at))
            else {
                warnings.push(Warning::MalformedAatSubtable { site });
                return;
            };
            glyphs.push(glyph);
        }
        if buffer.len().saturating_add(glyphs.len()) > self.max_glyphs {
            warnings.push(Warning::InsertionBudgetExceeded { site });
            return;
        }
        let at = if request.before {
            request.beside
        } else {
            request.beside.saturating_add(1)
        };
        buffer.insert(at, &glyphs, cluster, request.kind);
        moved.record(at, glyphs.len());
        if request.kind == Insertion::SplitVowelLike {
            let partner = match other {
                Other::Current => Some(moved.current()),
                Other::Mark(mark) => Some(mark),
                Other::None => None,
            };
            if let Some(partner) = partner.filter(|partner| *partner < buffer.len()) {
                let last = at.saturating_add(glyphs.len()).saturating_sub(1);
                let low = at.min(partner);
                let high = last.max(partner);
                buffer.merge_clusters(low..high.saturating_add(1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Moved;

    /// What an insertion moves, by where it lands: before the position, at
    /// it, right after the current glyph, or further on.
    #[test]
    fn an_insertion_moves_what_it_lands_before() {
        let mut moved = Moved {
            slot: 5,
            ..Moved::default()
        };
        assert_eq!(
            (moved.position(), moved.current(), moved.last_after()),
            (5, 5, 5)
        );
        // At the mark, three glyphs before the position.
        moved.record(2, 3);
        assert_eq!(
            (moved.position(), moved.current(), moved.last_after()),
            (8, 8, 8)
        );
        // The current glyph's own list on its left.
        moved.record(8, 2);
        assert_eq!(
            (moved.position(), moved.current(), moved.last_after()),
            (8, 10, 10)
        );
        // On its right, and then at the end of what went there.
        moved.record(11, 4);
        moved.record(15, 1);
        assert_eq!(
            (moved.position(), moved.current(), moved.last_after()),
            (8, 10, 15)
        );
        // Further on, which moves nothing the machine stands on.
        moved.record(30, 6);
        assert_eq!(
            (moved.position(), moved.current(), moved.last_after()),
            (8, 10, 15)
        );
    }
}
