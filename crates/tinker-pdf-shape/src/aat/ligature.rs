//! The ligature subtable, `morx` type 2.
//!
//! Apple's *TrueType Reference Manual*, *The 'morx' table*, "Ligature
//! subtables": an extended state table and three `UInt32` offsets "from
//! stateHeader", to the ligature action table, the component table and the
//! ligature list. Its entries are six bytes, a new state, flags and a
//! `ligActionIndex`; `setComponent` (`0x8000`) pushes the current glyph "onto
//! the component stack for eventual processing", and `performAction`
//! (`0x2000`) uses the index "to process a ligature group", the choice of
//! where to start in the action table "made by index, not offset".
//!
//! A ligature action is a `UInt32`: `last` (`0x80000000`), "the last action
//! in the list. This also implies storage"; `store` (`0x40000000`), "Store the
//! ligature at the current cumulated index in the ligature table in place of
//! the marked (i.e. currently-popped) glyph"; and a 30-bit `offset`
//! "sign-extended to 32-bits and added to the glyph ID, resulting in an index
//! into the component table". Each action pops one component, "in reverse
//! order", and the component table's `UInt16` values, "once looked up as
//! described above, are added together to obtain an index (not an offset!)
//! into ... the actual ligature list", an array of glyphs.
//!
//! What `morx` inherits from the `mort` chapter's statement of the same
//! machine (`docs/design/shaping.md`, *`morx`*): "If the store and last flags
//! in the entry are clear, the component glyph is replaced by the deleted
//! glyph index; if the store or last flag is set, the accumulated offset is
//! used to fetch the glyph index for the ligature, which replaces the
//! component glyph"; "The ligature component stack is 16 deep"; and "When the
//! entire ligature action list has been processed, the position of each
//! ligature glyph in the glyph array is pushed onto the component stack again
//! so that the ligatures can be used as components of other ligatures".
//!
//! The readings the design settles where the chapters say no more:
//!
//! - A push onto a full stack is refused and named
//!   ([`Warning::ComponentStackFull`]), the glyph not pushed.
//! - An action popping an empty stack ends its list there, what it formed
//!   standing, and is named ([`Warning::ComponentStackEmpty`]); so does an
//!   action, a component or a ligature the subtable does not hold
//!   ([`Warning::MalformedAatSubtable`]), the glyph it would have replaced
//!   standing.
//! - The sum starts again after each ligature an action list stores, so each
//!   ligature of a list that forms several is indexed by its own components,
//!   which is this engine's reading where the chapters say only that
//!   "multiple ligatures may be formed by setting the store flag on
//!   intermediate entries".
//! - The ligatures are pushed again in the order their components were, the
//!   one stored deepest first, so the stack keeps the order the walk met its
//!   glyphs in.
//! - At the end of text the current glyph is the last one the machine met,
//!   as it is for every machine on the shared driver.
//!
//! **Clusters.** A ligature takes the first cluster of its components — the
//! smallest, the one `shaping::cluster_texts` rebuilds `/ToUnicode` from —
//! and where it is stored away from the component that holds it, the
//! clusters from the one to the other are merged as [`Buffer`]'s reordering
//! merges them, so they stay monotonic. The components it consumed become
//! deleted glyphs and leave the run when the chain ends.

use super::state::{Cursor, Entry, StateTable, Traversal, DELETED};
use super::Site;
use crate::buffer::Buffer;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::Warning;

/// The bytes of a ligature entry: new state, flags and the action index.
pub(crate) const ENTRY_SIZE: usize = 6;
/// The flag pushing the current glyph on the component stack.
const SET_COMPONENT: u16 = 0x8000;
/// The flag running the action list the entry's index starts.
const PERFORM_ACTION: u16 = 0x2000;
/// The action flag ending the list, and storing the ligature.
const LAST: u32 = 0x8000_0000;
/// The action flag storing the ligature in place of the popped glyph.
const STORE: u32 = 0x4000_0000;
/// The action's 30-bit offset.
const OFFSET: u32 = 0x3FFF_FFFF;
/// The sign bit of the 30-bit offset.
const OFFSET_SIGN: u32 = 0x2000_0000;
/// The component stack's depth, the `mort` chapter's "16 deep".
pub const COMPONENT_STACK: usize = 16;
/// Where the ligature action table's offset sits, after the `STXHeader`.
const LIG_ACTIONS: usize = 16;
/// Where the component table's offset sits.
const COMPONENTS: usize = 20;
/// Where the ligature list's offset sits.
const LIGATURES: usize = 24;

/// Runs a ligature machine over `buffer`, walking it `traversal`'s way;
/// `body` is the subtable's, the state table's header at its start. A push
/// or an action the subtable cannot make is recorded in `warnings` and the
/// machine goes on.
///
/// # Errors
///
/// [`Warning::MalformedAatSubtable`] where the subtable holds no offset to
/// its action table, component table or ligature list, or one past its end,
/// and the driver's: the machine stopped at its ceiling, or at a state, entry
/// or class its tables do not hold, the run as it left it.
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
    let section = |at: usize| -> Result<Bytes<'_>, Warning> {
        data.u32(at)
            .and_then(|offset| usize::try_from(offset).ok())
            .and_then(|offset| data.at(offset))
            .ok_or(Warning::MalformedAatSubtable { site })
    };
    let tables = Tables {
        actions: section(LIG_ACTIONS)?,
        components: section(COMPONENTS)?,
        ligatures: section(LIGATURES)?,
        site,
    };
    let mut stack: Vec<usize> = Vec::with_capacity(COMPONENT_STACK);
    let mut act =
        |buffer: &mut Buffer, entry: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
            if entry.flags & SET_COMPONENT != 0 {
                if let Some(at) = cursor.glyph {
                    if stack.len() >= COMPONENT_STACK {
                        warnings.push(Warning::ComponentStackFull { site });
                    } else {
                        stack.push(at);
                    }
                }
            }
            if entry.flags & PERFORM_ACTION != 0 {
                let first = entry.field(0).unwrap_or(0);
                tables.perform(buffer, &mut stack, first, warnings);
            }
            Ok(())
        };
    table.run(buffer, traversal, limits, site, &mut act)
}

/// A ligature subtable's three tables, each to the end of the subtable.
struct Tables<'a> {
    actions: Bytes<'a>,
    components: Bytes<'a>,
    ligatures: Bytes<'a>,
    site: Site,
}

impl Tables<'_> {
    /// Runs the action list starting at action `first`, popping a component
    /// an action; see the module documentation.
    ///
    /// The list is bounded by the stack: each action pops one of at most
    /// [`COMPONENT_STACK`] components, and the one after the last pops an
    /// empty stack, which ends it.
    fn perform(
        &self,
        buffer: &mut Buffer,
        stack: &mut Vec<usize>,
        first: u16,
        warnings: &mut Vec<Warning>,
    ) {
        let site = self.site;
        // The positions of the ligatures stored, in the order stored, and
        // the components popped since the last of them.
        let mut formed: Vec<usize> = Vec::new();
        let mut group: Vec<usize> = Vec::new();
        let mut sum = 0u64;
        let mut index = usize::from(first);
        loop {
            let Some(action) = index.checked_mul(4).and_then(|at| self.actions.u32(at)) else {
                warnings.push(Warning::MalformedAatSubtable { site });
                break;
            };
            let Some(at) = stack.pop() else {
                warnings.push(Warning::ComponentStackEmpty { site });
                break;
            };
            let Some(glyph) = buffer.glyph_id(at) else {
                warnings.push(Warning::MalformedAatSubtable { site });
                break;
            };
            let Some(value) = self.component(glyph, action) else {
                warnings.push(Warning::MalformedAatSubtable { site });
                break;
            };
            sum = sum.saturating_add(u64::from(value));
            group.push(at);
            if action & (LAST | STORE) != 0 {
                let Some(ligature) = self.ligature(sum) else {
                    warnings.push(Warning::MalformedAatSubtable { site });
                    break;
                };
                buffer.set_glyph(at, ligature);
                take_first_cluster(buffer, at, &group);
                formed.push(at);
                group.clear();
                sum = 0;
            } else {
                buffer.set_glyph(at, DELETED);
            }
            if action & LAST != 0 {
                break;
            }
            index = match index.checked_add(1) {
                Some(next) => next,
                None => break,
            };
        }
        // Pushed again, the deepest first, so that the stack keeps the order
        // the walk met the glyphs in; it has room, every one of them having
        // been popped from it.
        for at in formed.into_iter().rev() {
            if stack.len() < COMPONENT_STACK {
                stack.push(at);
            }
        }
    }

    /// The component table's value for `glyph` under `action`'s offset: the
    /// glyph index plus the sign-extended 30 bits, an index into the table.
    fn component(&self, glyph: u16, action: u32) -> Option<u16> {
        let raw = action & OFFSET;
        let offset = if raw & OFFSET_SIGN != 0 {
            i64::from(raw) - i64::from(OFFSET) - 1
        } else {
            i64::from(raw)
        };
        let index = usize::try_from(i64::from(glyph).checked_add(offset)?).ok()?;
        self.components.u16_at(0, index)
    }

    /// The ligature list's glyph at `index`.
    fn ligature(&self, index: u64) -> Option<u16> {
        self.ligatures.u16_at(0, usize::try_from(index).ok()?)
    }
}

/// Gives the ligature stored at `at` the first cluster of `components`, the
/// smallest, merging the clusters between `at` and the component that held
/// it so that they stay monotonic.
fn take_first_cluster(buffer: &mut Buffer, at: usize, components: &[usize]) {
    let first = components
        .iter()
        .filter_map(|component| {
            buffer
                .glyph(*component)
                .map(|glyph| (glyph.cluster, *component))
        })
        .min();
    if let Some((_, holder)) = first {
        buffer.merge_clusters(at.min(holder)..at.max(holder).saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::{Tables, LAST, STORE};
    use crate::aat::Site;
    use crate::common::Tag;
    use crate::read::Bytes;

    fn site() -> Site {
        Site {
            table: Tag::new(b"morx"),
            chain: 0,
            subtable: 0,
        }
    }

    /// The 30-bit offset is sign-extended: the manual's own three actions,
    /// −25, −19 and −14, index the components their comments say.
    #[test]
    fn the_offset_is_thirty_bits_sign_extended() {
        let components: Vec<u8> = (0u16..40).flat_map(|v| (v * 10).to_be_bytes()).collect();
        let tables = Tables {
            actions: Bytes::new(&[]),
            components: Bytes::new(&components),
            ligatures: Bytes::new(&[]),
            site: site(),
        };
        assert_eq!(tables.component(25, 0x3FFF_FFE7), Some(0), "f, -25");
        assert_eq!(tables.component(28, 0x3FFF_FFE7), Some(30), "i, -25");
        assert_eq!(tables.component(23, 0x3FFF_FFED), Some(40), "d, -19");
        assert_eq!(tables.component(20, 0xBFFF_FFF2), Some(60), "a, -14");
        // The flag bits play no part in the offset.
        assert_eq!(tables.component(5, LAST | STORE | 2), Some(70));
        assert_eq!(tables.component(5, 2), Some(70));
        // An index below zero, or past the table, answers nothing.
        assert_eq!(tables.component(3, 0x3FFF_FFFB), None, "3 - 5");
        assert_eq!(tables.component(39, 1), None, "past the table");
        assert_eq!(tables.component(u16::MAX, 0x1FFF_FFFF), None);
    }
}
