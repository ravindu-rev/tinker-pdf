//! Apple Advanced Typography: the tables a face built for Apple's layout
//! carries its shaping in.
//!
//! `docs/design/shaping.md`, *AAT shaping*, reads them clause by clause from
//! Apple's *TrueType Reference Manual* and from nothing else (ruling 21): its
//! *AAT special tables* chapter for the two structures every AAT table is made
//! of, and the chapters of the tables themselves. This module holds the
//! shared machinery:
//!
//! - [`Lookup`], the lookup table in each of the six formats the manual
//!   defines, a glyph in and a value out;
//! - [`StateTable`], the extended state table — its class lookup, its state
//!   array and its entry table — and [`StateTable::run`], the one driver
//!   every machine runs on, a subtable's [`Actions`] acting at each
//!   transition, under the ceiling [`crate::Limits::max_transitions`] puts on
//!   a machine that never advances.
//!
//! and the tables:
//!
//! - [`Morx`], the extended glyph metamorphosis table: its chains, their
//!   feature entries and sub-feature flags, each subtable's coverage, and the
//!   subtables the chain runs — noncontextual, rearrangement, contextual,
//!   ligature, on a component stack [`COMPONENT_STACK`] deep, and insertion,
//!   each inserted glyph marked with how ([`crate::Insertion`]);
//!   [`Selection`] is what a caller's OpenType features ask of it;
//! - [`Kerx`], the extended kerning table: its subtables' coverage, and the
//!   pair formats 0, 2 and 6 run over a run in layout order, along the line
//!   or across it;
//! - [`Feat`], the feature name table, which says which features' settings
//!   exclude one another.
//!
//! Everything is integer, as the rest of the crate is (`lib.rs`), and every
//! read is a bounds check, so a truncated table or an offset past its end
//! answers nothing and never panics (ruling 1). What a table asks for that
//! this crate refuses is a [`crate::Warning`] naming where in the table it
//! was asked ([`Site`]), recorded once a run (ruling 10).

mod contextual;
mod feat;
mod insertion;
mod kerx;
mod ligature;
mod lookup;
mod morx;
mod rearrangement;
mod state;

pub use feat::{Feat, FeatureName, FEAT};
pub use kerx::{
    Kerx, KerxSubtable, CROSS_STREAM, KERX, KERX_VARIATION, KERX_VERTICAL, PROCESS_DIRECTION,
};
pub use ligature::COMPONENT_STACK;
pub use lookup::{Lookup, LookupError, LookupFormat};
pub use morx::{
    aat_setting, AatSetting, Chain, FeatureEntry, Morx, Selection, Subtable, ANY_ORIENTATION,
    CONTEXTUAL, DESCENDING, INSERTION, LIGATURE, LOGICAL, MORX, NONCONTEXTUAL, REARRANGEMENT,
    VERTICAL,
};
pub use state::{
    Actions, Cursor, Entry, StateError, StateTable, Traversal, DELETED, DELETED_GLYPH,
    DONT_ADVANCE, END_OF_LINE, END_OF_TEXT, OUT_OF_BOUNDS,
};

use crate::common::Tag;

/// Where in an AAT table something was refused: the table, by its tag, and
/// the chain and the subtable, each counted from zero in table order.
///
/// Ruling 10's provenance for a table this crate reads by position rather
/// than by name: "subtable 3 of chain 0 of `morx`" is the sentence a face's
/// author can find. A table without chains, as `kerx` is, is chain 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Site {
    /// The table, as the face's table directory spells it.
    pub table: Tag,
    /// The chain, counting from zero.
    pub chain: u32,
    /// The subtable within its chain, counting from zero.
    pub subtable: u32,
}
