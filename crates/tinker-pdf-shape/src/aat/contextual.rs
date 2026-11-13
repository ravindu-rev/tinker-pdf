//! The contextual glyph substitution subtable, `morx` type 1.
//!
//! Apple's *TrueType Reference Manual*, *The 'morx' table*, "Contextual Glyph
//! Substitution Subtable": "the substitution of other glyphs for both the
//! glyph at the current position in the glyph array and an earlier glyph in
//! the glyph array, called the marked glyph". The subtable is an extended
//! state table and a `uint32` `substitutionTable`, "Byte offset from the
//! beginning of the state subtable to the beginning of the substitution
//! tables"; its entries are eight bytes, a new state, flags, a `markIndex`
//! and a `currentIndex`, each "the substitution table for the marked glyph"
//! or "for the current glyph", `0xFFFF` for none. `setMark` (`0x8000`) makes
//! the current glyph the marked one, "after the substitutions are made".
//!
//! The substitution tables are lookup tables, reached through "an array of
//! UInt32 offsets from the start of the per-glyph lookup table space", and
//! "nowhere is there specified the number of LookupTables". The array ends
//! where the first table it names begins at the latest, so an index whose
//! offset would lie at or past that point is past the array: it substitutes
//! nothing and is named ([`Warning::SubstitutionTableMissing`]), as is one
//! whose offset cannot be read. A value of 0 a table gives is "do nothing",
//! as everywhere in `morx`; a value of `0xFFFF` deletes the glyph.
//!
//! The readings `docs/design/shaping.md` settles, each held by a
//! text-rendering-tests section: where no glyph has been marked, the mark is
//! the first glyph the machine met (`MORX-19`); the marked glyph is
//! substituted before the current one, so both indices on one glyph chain
//! (`MORX-22`); the marked glyph is substituted wherever the machine has
//! moved since marking it (`MORX-25`); and at the end of text the current
//! glyph is the last one the machine met (`MORX-20`).

use super::lookup::{Lookup, LookupError};
use super::state::{Cursor, Entry, StateTable, Traversal, DELETED};
use super::Site;
use crate::buffer::Buffer;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::Warning;

/// The bytes of a contextual entry: new state, flags and the two indices.
pub(crate) const ENTRY_SIZE: usize = 8;
/// The flag making the current glyph the marked one.
const SET_MARK: u16 = 0x8000;
/// The index naming no substitution table.
const NONE: u16 = 0xFFFF;
/// Where the `substitutionTable` offset sits, after the `STXHeader`.
const SUBSTITUTION_TABLE: usize = 16;

/// Runs a contextual substitution machine over `buffer`, walking it
/// `traversal`'s way; `body` is the subtable's, the state table's header at
/// its start. A substitution the tables cannot make is recorded in
/// `warnings` and the machine goes on.
///
/// # Errors
///
/// [`Warning::MalformedAatSubtable`] where the subtable holds no
/// `substitutionTable` offset, and the driver's: the machine stopped at its
/// ceiling, or at a state, entry or class its tables do not hold, the run as
/// it left it.
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
    let offset = data
        .u32(SUBSTITUTION_TABLE)
        .and_then(|offset| usize::try_from(offset).ok())
        .ok_or(Warning::MalformedAatSubtable { site })?;
    let tables = Substitutions {
        space: data.at(offset).unwrap_or(Bytes::new(&[])),
        site,
    };
    let mut mark: Option<usize> = traversal
        .first(buffer.len())
        .filter(|at| *at < buffer.len());
    let mut act =
        |buffer: &mut Buffer, entry: &Entry<'_>, cursor: &mut Cursor| -> Result<(), Warning> {
            let current = cursor.glyph;
            let mark_index = entry.field(0).unwrap_or(NONE);
            let current_index = entry.field(1).unwrap_or(NONE);
            if mark_index != NONE {
                if let Some(at) = mark {
                    tables.substitute(buffer, at, mark_index, warnings);
                }
            }
            if current_index != NONE {
                if let Some(at) = current {
                    tables.substitute(buffer, at, current_index, warnings);
                }
            }
            if entry.flags & SET_MARK != 0 {
                mark = current;
            }
            Ok(())
        };
    table.run(buffer, traversal, limits, site, &mut act)
}

/// A subtable's per-glyph substitution tables.
struct Substitutions<'a> {
    /// The per-glyph lookup table space: the offset array, then the tables.
    space: Bytes<'a>,
    site: Site,
}

impl Substitutions<'_> {
    /// The lookup table `index` names, or why it names none.
    fn table(&self, index: u16) -> Result<Lookup<'_>, Warning> {
        let site = self.site;
        let missing = Warning::SubstitutionTableMissing { site, index };
        // The array ends where its first table begins at the latest.
        let first = self.space.u32(0).ok_or(missing)?;
        let slot = u64::from(index) * 4;
        if slot + 4 > u64::from(first) {
            return Err(missing);
        }
        let at = usize::try_from(slot).map_err(|_| missing)?;
        let offset = self.space.u32(at).ok_or(missing)?;
        let offset = usize::try_from(offset).map_err(|_| missing)?;
        let bytes = self.space.at(offset).ok_or(missing)?;
        Lookup::parse(bytes.as_slice()).map_err(|error| match error {
            LookupError::Format(format) => Warning::UnknownLookupFormat { site, format },
            LookupError::Malformed => Warning::MalformedAatSubtable { site },
        })
    }

    /// Puts the glyph at `at` through table `index`: a value other than 0
    /// replaces it. A deleted glyph is not looked up.
    fn substitute(&self, buffer: &mut Buffer, at: usize, index: u16, warnings: &mut Vec<Warning>) {
        let Some(glyph) = buffer.glyph(at).map(|g| g.glyph) else {
            return;
        };
        if glyph == DELETED {
            return;
        }
        let lookup = match self.table(index) {
            Ok(lookup) => lookup,
            Err(warning) => {
                warnings.push(warning);
                return;
            }
        };
        match lookup.value(glyph) {
            None | Some(0) => {}
            Some(value) => match u16::try_from(value) {
                Ok(replacement) => buffer.set_glyph(at, replacement),
                // A glyph index is sixteen bits, and a value past them names
                // none.
                Err(_) => warnings.push(Warning::MalformedAatSubtable { site: self.site }),
            },
        }
    }
}
