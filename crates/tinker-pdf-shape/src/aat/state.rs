//! AAT extended state tables, and the one driver every machine runs on.
//!
//! Apple's *TrueType Reference Manual*, *AAT special tables* chapter, "State
//! Tables" and "Extended State Tables". An extended state table opens with an
//! `STXHeader` — `nClasses`, "the number of 16-bit entry indices in a single
//! line in the state array", and the offsets "from the start of this state
//! table header" of the class table, the state array and the entry table —
//! and its class table is a lookup table "where the looked-up value is a
//! 16-bit class value" ([`crate::aat::Lookup`]). The state array is
//! `nClasses` entry indices a state, "16-bit zero-based indices into the entry
//! table"; an entry is a `uint16` new state — "a zero-based 16-bit index" in
//! `morx` and `kerx`, the `morx` chapter's reminder — and `uint16` flags,
//! then whatever fields the table that embeds it adds, so its size is the
//! embedding table's.
//!
//! Four classes are predefined ("The Class Subtable"): 0, *end of text*; 1,
//! *out of bounds*, which "all glyph indexes" the class table does not cover
//! "will automatically be assigned"; 2, *deleted glyph*, the class of glyph
//! `0xFFFF`; and 3, *end of line*. Two states are: 0, *start of text*, and 1,
//! *start of line*, "One of these two states will be chosen as the initial
//! state (depending on context)". A run is not a line, so the machine starts
//! in state 0 (`docs/design/shaping.md`, *Lookup tables and state tables*).
//!
//! # One driver
//!
//! [`StateTable::run`] is every machine's loop, as the chapter states it:
//! "For each glyph in the glyph array, the finite state machine performs an
//! action appropriate to the glyph's class and state ... After processing all
//! glyphs in the glyph array, the finite state machine performs the action in
//! the current state indicated by a special class, called end of text". From
//! state 0 it reads the class of the glyph at the current position, takes that
//! state's entry for it, hands the entry to the subtable's [`Actions`], moves
//! to the entry's state, and advances unless the entry's `dontAdvance` flag
//! (`0x4000`, the same bit in every `morx` subtable type and every `kerx`
//! state machine) is set; past the last glyph it takes the current state's
//! end-of-text entry once, `dontAdvance` meaning nothing there. The walk is
//! over the run in layout order, forward or backward ([`Traversal`]).
//!
//! A machine whose entries hold their position for ever never reaches the
//! end of its run, so every transition, the end of text's included, is
//! counted against [`Limits::max_transitions`], and past it the machine stops
//! where it stands, the run as it stood, with
//! [`Warning::TransitionBudgetExceeded`]. A state or an entry the table does
//! not hold, or a class with no column in its state array, stops it the same
//! way with [`Warning::MalformedAatSubtable`]: every read is `read::Bytes`',
//! so a truncated table or an offset past its end answers nothing and never
//! panics (ruling 1).

use super::lookup::{Lookup, LookupError};
use super::Site;
use crate::buffer::Buffer;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::Warning;

/// The class of the end of text, taken once past the last glyph.
pub const END_OF_TEXT: u32 = 0;
/// The class of a glyph the class table does not cover.
pub const OUT_OF_BOUNDS: u32 = 1;
/// The class of the deleted glyph, `0xFFFF`.
pub const DELETED_GLYPH: u32 = 2;
/// The class of the end of a line, which no run reaches: a run is not a
/// line.
pub const END_OF_LINE: u32 = 3;
/// The glyph index a deleted glyph carries: "the deleted glyph index,
/// 0xFFFF".
pub const DELETED: u16 = 0xFFFF;
/// The `dontAdvance` flag of an entry.
pub const DONT_ADVANCE: u16 = 0x4000;

/// Why an extended state table could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateError {
    /// Its header, its offsets or its entry size cannot be read as a state
    /// table: a header cut short, an offset past the table, fewer than the
    /// four predefined classes, or an entry smaller than its new state and
    /// flags.
    Malformed,
    /// Its class table is a lookup table of a format the manual does not
    /// define, by its number.
    LookupFormat(u16),
}

impl StateError {
    /// The warning a subtable at `site` records for this error.
    #[must_use]
    pub const fn warning(self, site: Site) -> Warning {
        match self {
            StateError::Malformed => Warning::MalformedAatSubtable { site },
            StateError::LookupFormat(format) => Warning::UnknownLookupFormat { site, format },
        }
    }
}

/// An extended state table, read.
#[derive(Clone, Copy, Debug)]
pub struct StateTable<'a> {
    classes: Lookup<'a>,
    class_count: u32,
    states: Bytes<'a>,
    entries: Bytes<'a>,
    entry_size: usize,
}

impl<'a> StateTable<'a> {
    /// Reads the `STXHeader` at the start of `bytes`, whose entries are
    /// `entry_size` bytes each: four for a rearrangement subtable, eight for
    /// a contextual or an insertion one, six for a ligature one or a `kerx`
    /// contextual kerning one.
    ///
    /// # Errors
    ///
    /// [`StateError::LookupFormat`] where the class table's format is one the
    /// manual does not define, and [`StateError::Malformed`] where anything
    /// else of the header cannot be read: see its variants.
    pub fn parse(bytes: &'a [u8], entry_size: usize) -> Result<StateTable<'a>, StateError> {
        let data = Bytes::new(bytes);
        let class_count = data.u32(0).ok_or(StateError::Malformed)?;
        if class_count < 4 || entry_size < 4 {
            return Err(StateError::Malformed);
        }
        let section = |at: usize| -> Result<Bytes<'a>, StateError> {
            let offset = data.u32(at).ok_or(StateError::Malformed)?;
            let offset = usize::try_from(offset).map_err(|_| StateError::Malformed)?;
            data.at(offset).ok_or(StateError::Malformed)
        };
        let class_table = section(4)?;
        let states = section(8)?;
        let entries = section(12)?;
        let classes = Lookup::parse(class_table.as_slice()).map_err(|error| match error {
            LookupError::Format(format) => StateError::LookupFormat(format),
            LookupError::Malformed => StateError::Malformed,
        })?;
        Ok(StateTable {
            classes,
            class_count,
            states,
            entries,
            entry_size,
        })
    }

    /// `nClasses`: how many classes, and so how many entry indices a state's
    /// line of the state array holds.
    #[must_use]
    pub const fn class_count(&self) -> u32 {
        self.class_count
    }

    /// The bytes of one entry.
    #[must_use]
    pub const fn entry_size(&self) -> usize {
        self.entry_size
    }

    /// The class of `glyph`: [`DELETED_GLYPH`] for `0xFFFF`, the class table's
    /// value where it covers the glyph, and [`OUT_OF_BOUNDS`] where it does
    /// not. A value too large for a class is answered as it is, and has no
    /// column to take.
    #[must_use]
    pub fn class(&self, glyph: u16) -> u32 {
        if glyph == DELETED {
            return DELETED_GLYPH;
        }
        match self.classes.value(glyph) {
            Some(value) => u32::try_from(value).unwrap_or(u32::MAX),
            None => OUT_OF_BOUNDS,
        }
    }

    /// The entry index the state array gives `state` for `class`, or `None`
    /// for a class past `nClasses` or a state line the table does not hold.
    #[must_use]
    pub fn entry_index(&self, state: u16, class: u32) -> Option<u16> {
        if class >= self.class_count {
            return None;
        }
        let line = u64::from(state).checked_mul(u64::from(self.class_count))?;
        let index = line.checked_add(u64::from(class))?.checked_mul(2)?;
        self.states.u16(usize::try_from(index).ok()?)
    }

    /// Entry `index` of the entry table, or `None` where the table does not
    /// hold it whole.
    #[must_use]
    pub fn entry(&self, index: u16) -> Option<Entry<'a>> {
        let start = usize::from(index).checked_mul(self.entry_size)?;
        let end = start.checked_add(self.entry_size)?;
        let whole = self.entries.as_slice().get(start..end)?;
        let bytes = Bytes::new(whole);
        Some(Entry {
            new_state: bytes.u16(0)?,
            flags: bytes.u16(2)?,
            fields: bytes.at(4)?,
        })
    }

    /// The entry `state` takes for `class`: [`StateTable::entry_index`] then
    /// [`StateTable::entry`].
    #[must_use]
    pub fn transition(&self, state: u16, class: u32) -> Option<Entry<'a>> {
        self.entry(self.entry_index(state, class)?)
    }

    /// Whether any glyph of `buffer` takes a class other than out of bounds:
    /// whether the run holds a glyph this machine is written for.
    #[must_use]
    pub fn reaches(&self, buffer: &Buffer) -> bool {
        buffer
            .glyphs()
            .iter()
            .any(|glyph| self.class(glyph.glyph) != OUT_OF_BOUNDS)
    }

    /// Runs the machine over `buffer`, walking it `traversal`'s way, each
    /// transition handed to `actions`; see the module documentation.
    ///
    /// # Errors
    ///
    /// [`Warning::TransitionBudgetExceeded`] where the machine took
    /// `limits.max_transitions` transitions without finishing its run, and
    /// [`Warning::MalformedAatSubtable`] where it needed a state or an entry
    /// the table does not hold, both for `site`; and whatever `actions`
    /// stopped it with. The buffer is left as the machine left it.
    pub fn run<A: Actions + ?Sized>(
        &self,
        buffer: &mut Buffer,
        traversal: Traversal,
        limits: &Limits,
        site: Site,
        actions: &mut A,
    ) -> Result<(), Warning> {
        let mut state = 0u16;
        let mut position = traversal.first(buffer.len());
        let mut taken = 0u32;
        loop {
            if taken >= limits.max_transitions {
                return Err(Warning::TransitionBudgetExceeded { site });
            }
            taken += 1;
            let at = position.filter(|at| *at < buffer.len());
            let class = match at {
                Some(at) => self.class(buffer.glyph(at).map_or(DELETED, |g| g.glyph)),
                None => END_OF_TEXT,
            };
            let entry = self
                .transition(state, class)
                .ok_or(Warning::MalformedAatSubtable { site })?;
            let mut cursor = Cursor {
                glyph: at.or_else(|| traversal.last(buffer.len())),
                end_of_text: at.is_none(),
            };
            actions.act(buffer, &entry, &mut cursor)?;
            state = entry.new_state;
            if at.is_none() {
                return Ok(());
            }
            position = cursor.glyph;
            if !entry.dont_advance() {
                position = position.and_then(|at| traversal.step(at));
            }
        }
    }
}

/// One entry of an entry table: the state it moves to, its flags, and the
/// fields the embedding table adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The state the machine moves to: a line of the state array.
    pub new_state: u16,
    /// The entry's flags, whose meaning is the embedding table's but for
    /// [`DONT_ADVANCE`].
    pub flags: u16,
    /// The fields after the flags.
    fields: Bytes<'a>,
}

impl Entry<'_> {
    /// The `n`th `uint16` field after the flags, counting from zero.
    #[must_use]
    pub fn field(&self, n: usize) -> Option<u16> {
        self.fields.u16_at(0, n)
    }

    /// Whether the machine holds its position after this entry.
    #[must_use]
    pub const fn dont_advance(&self) -> bool {
        self.flags & DONT_ADVANCE != 0
    }
}

/// Which way a machine walks its run, in layout order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Traversal {
    /// From the first glyph to the last.
    #[default]
    Forward,
    /// From the last glyph to the first.
    Backward,
}

impl Traversal {
    /// The first glyph the walk meets in a run of `len` glyphs; a position
    /// past the run where it holds none.
    #[must_use]
    pub const fn first(self, len: usize) -> Option<usize> {
        match self {
            Traversal::Forward => Some(0),
            Traversal::Backward => len.checked_sub(1),
        }
    }

    /// The last glyph the walk meets in a run of `len` glyphs, or `None` in
    /// an empty run.
    #[must_use]
    pub const fn last(self, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        match self {
            Traversal::Forward => len.checked_sub(1),
            Traversal::Backward => Some(0),
        }
    }

    /// The position after `at` in the walk's direction; `None` past the
    /// first glyph of a backward walk.
    #[must_use]
    pub const fn step(self, at: usize) -> Option<usize> {
        match self {
            Traversal::Forward => at.checked_add(1),
            Traversal::Backward => at.checked_sub(1),
        }
    }
}

/// Where a machine stands at one transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// The current glyph: at the end of text, the last glyph the walk met,
    /// which an action there acts on as the current one; `None` in an empty
    /// run. An action that moves the current glyph — an insertion before it
    /// — sets where it now stands, and the walk advances from there.
    pub glyph: Option<usize>,
    /// Whether this is the end-of-text transition.
    pub end_of_text: bool,
}

/// What a subtable does at each transition of its machine.
pub trait Actions {
    /// Acts on one transition: `entry` is the one the machine took and
    /// `cursor` where it stands.
    ///
    /// # Errors
    ///
    /// A warning stops the machine where it stands, and is the subtable's
    /// to record.
    fn act(
        &mut self,
        buffer: &mut Buffer,
        entry: &Entry<'_>,
        cursor: &mut Cursor,
    ) -> Result<(), Warning>;
}

impl<F> Actions for F
where
    F: FnMut(&mut Buffer, &Entry<'_>, &mut Cursor) -> Result<(), Warning>,
{
    fn act(
        &mut self,
        buffer: &mut Buffer,
        entry: &Entry<'_>,
        cursor: &mut Cursor,
    ) -> Result<(), Warning> {
        self(buffer, entry, cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::{StateError, StateTable, Traversal};
    use crate::aat::Site;
    use crate::common::Tag;
    use crate::Warning;

    fn u32s(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn a_header_that_cannot_be_read_is_malformed() {
        // Cut short, fewer than four classes, an offset past the table.
        assert_eq!(
            StateTable::parse(&[0, 0, 0], 4).err(),
            Some(StateError::Malformed)
        );
        let three = u32s(&[3, 16, 16, 16]);
        assert_eq!(
            StateTable::parse(&three, 4).err(),
            Some(StateError::Malformed)
        );
        let past = u32s(&[4, 16, 400, 16]);
        assert_eq!(
            StateTable::parse(&past, 4).err(),
            Some(StateError::Malformed)
        );
        // An entry smaller than its state and flags.
        let mut ok = u32s(&[4, 16, 16, 16]);
        ok.extend([0u8, 8, 0, 0, 0, 0]);
        assert_eq!(StateTable::parse(&ok, 2).err(), Some(StateError::Malformed));
        // A class table of a format the manual does not define.
        let mut odd = u32s(&[4, 16, 16, 16]);
        odd.extend([0u8, 5, 0, 0]);
        assert_eq!(
            StateTable::parse(&odd, 4).err(),
            Some(StateError::LookupFormat(5))
        );
    }

    #[test]
    fn each_error_names_its_site() {
        let site = Site {
            table: Tag::new(b"morx"),
            chain: 1,
            subtable: 2,
        };
        assert_eq!(
            StateError::Malformed.warning(site),
            Warning::MalformedAatSubtable { site }
        );
        assert_eq!(
            StateError::LookupFormat(12).warning(site),
            Warning::UnknownLookupFormat { site, format: 12 }
        );
    }

    #[test]
    fn a_walk_meets_its_run_from_either_end() {
        assert_eq!(Traversal::Forward.first(3), Some(0));
        assert_eq!(Traversal::Backward.first(3), Some(2));
        assert_eq!(Traversal::Backward.first(0), None);
        assert_eq!(Traversal::Forward.last(3), Some(2));
        assert_eq!(Traversal::Backward.last(3), Some(0));
        assert_eq!(Traversal::Forward.last(0), None);
        assert_eq!(Traversal::Backward.last(0), None);
        assert_eq!(Traversal::Forward.step(1), Some(2));
        assert_eq!(Traversal::Backward.step(1), Some(0));
        assert_eq!(Traversal::Backward.step(0), None);
    }
}
