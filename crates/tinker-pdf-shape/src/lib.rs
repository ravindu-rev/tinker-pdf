//! OpenType Layout: face bytes and glyph indices in, substituted glyphs and
//! integer positions out.
//!
//! Design documentation: `docs/design/shaping.md`. This is its **first
//! milestone**: the `GDEF`, `GSUB` and `GPOS` tables of ISO/IEC 14496-22, and
//! the machinery that executes a lookup list against a [`Buffer`]. It is not
//! yet a shaper — nothing here maps text to glyphs, itemizes a paragraph or
//! knows what script anything is in. A caller supplies the glyphs and the
//! feature tags; milestones 2 to 5 grow the code that decides both.
//!
//! **The eleventh leaf.** Ruling 8's August 2026 amendment makes the test of
//! a leaf the definition rather than the list: *a leaf is any crate that takes
//! bytes and plain parameters and returns bytes and values, whatever the list
//! says.* Nothing below knows what a PDF, a page, a content stream or a CSS
//! property is. What arrives is a face's table bytes and some glyph indices;
//! what leaves is glyph indices and six integers each.
//!
//! # Integer, and denied at compile time
//!
//! `#![deny(clippy::float_arithmetic)]` is at the top of this file because
//! `docs/design/shaping.md` puts it there, and the reason is ruling 4. Every
//! number in this crate is a font design unit — the `FWORD`s the tables carry,
//! widened to `i32` — so a run shaped on linux, windows, macos and wasm is the
//! same run. Scaling to points is `units * size / upem`, which happens in the
//! consumer: one multiply and one divide, both correctly rounded by IEEE 754
//! and therefore identical everywhere, which is the side of ruling 4's line
//! the rule explicitly allows.
//!
//! The lint is not decoration. A shaper that accumulated advances in `f32`
//! would agree with itself on every machine anybody tested it on and diverge
//! on one nobody did, and the divergence would be a line of Arabic that
//! wrapped one word earlier.
//!
//! # Never panicking is the load-bearing property
//!
//! A `GSUB` table is a graph of 16-bit offsets, read from a file this engine
//! did not write. Offsets point anywhere, counts lie, lookups name each other
//! in cycles, and one substitution can turn five glyphs into five thousand.
//! Ruling 1 says none of that may panic, and three things discharge it:
//! [`read::Bytes`] makes every read a bounds check, [`limits::Limits`] bounds
//! the three ways execution can run away, and `fuzz/fuzz_targets/shape.rs`
//! drives arbitrary bytes as a face against arbitrary glyph runs.
//!
//! What a malformed table produces instead is a [`Warning`] naming the lookup
//! it came from — ruling 10's shape — and a buffer left as it stood.
//!
//! # Worked example
//!
//! ```no_run
//! use tinker_pdf_font::Sfnt;
//! use tinker_pdf_shape::{Buffer, Layout, Limits, MarkWidths, Tag};
//!
//! # fn shape(face_bytes: &[u8]) -> Option<()> {
//! let face = Sfnt::parse(face_bytes)?;
//! let layout = Layout::parse(&face);
//!
//! let mut buffer = Buffer::from_glyphs(&[24, 25, 26]);
//! for at in 0..buffer.len() {
//!     let glyph = buffer.glyph(at)?.glyph;
//!     let advance = i32::from(face.advance(glyph).unwrap_or(0));
//!     buffer.glyph_mut(at)?.x_advance = advance;
//! }
//!
//! let limits = Limits::for_glyphs(buffer.len());
//! if let Some(gsub) = layout.gsub() {
//!     let lookups = gsub.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"liga")]);
//!     layout.substitute(&mut buffer, &lookups, limits);
//! }
//! if let Some(gpos) = layout.gpos() {
//!     let lookups = gpos.lookups_for(Tag::new(b"latn"), None, &[Tag::new(b"kern")]);
//!     layout.position(&mut buffer, &lookups, limits, MarkWidths::default());
//! }
//! # Some(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::float_arithmetic)]

mod apply;
pub mod buffer;
pub mod common;
pub mod gdef;
mod gpos;
mod gsub;
pub mod limits;
pub mod read;

pub use buffer::{Buffer, Direction, ShapedGlyph};
pub use common::{
    ClassDef, Condition, ConditionSet, Coverage, Device, Feature, FeatureList,
    FeatureSubstitutions, FeatureVariations, LangSys, Lookup, LookupList, Script, ScriptList, Tag,
};
pub use gdef::{AttachList, Caret, Gdef, GlyphClass, LigCaretList};
pub use limits::Limits;

use apply::Runner;
use read::Bytes;
use tinker_pdf_font::Sfnt;

/// Which of the two layout tables something came from.
///
/// Carried by every [`Warning`], because "lookup 12 was malformed" is not an
/// actionable sentence in a face that has a lookup 12 in each table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Table {
    /// `GSUB`, the substitution table.
    Gsub,
    /// `GPOS`, the positioning table.
    Gpos,
}

impl core::fmt::Display for Table {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Table::Gsub => "GSUB",
            Table::Gpos => "GPOS",
        })
    }
}

/// Something a face asked for that this crate refused to do.
///
/// Ruling 10: every leniency action names the object it touched, so that "it
/// shaped" and "it shaped cleanly" are different sentences. A warning here is
/// never fatal — the run continues with the offending lookup skipped, per
/// ruling 2 — and each distinct one is recorded once however often it fires,
/// because a hostile face can make one refusal happen a hundred thousand
/// times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// A lookup of a type this crate does not implement.
    ///
    /// Every type ISO/IEC 14496-22 defines is implemented, so in practice
    /// this is a face using a type from a later revision, or a byte of
    /// garbage where a type should be.
    UnknownLookupType {
        /// Which table the lookup came from.
        table: Table,
        /// Its index in that table's lookup list.
        lookup: u16,
        /// The type it claimed to be.
        kind: u16,
    },
    /// A subtable whose header could not be read.
    MalformedSubtable {
        /// Which table the lookup came from.
        table: Table,
        /// Its index in that table's lookup list.
        lookup: u16,
        /// Which of the lookup's subtables.
        subtable: u16,
    },
    /// An extension subtable pointed at another extension subtable, which
    /// 14496-22 forbids in as many words.
    ///
    /// Followed anyway, under [`Limits::max_extension_depth`], because a rule
    /// a conforming font obeys is not a defence against one that does not.
    ExtensionChained {
        /// Which table the lookup came from.
        table: Table,
        /// Its index in that table's lookup list.
        lookup: u16,
    },
    /// Extension indirections exceeded [`Limits::max_extension_depth`], which
    /// is what a subtable pointing at itself produces.
    ExtensionTooDeep {
        /// Which table the lookup came from.
        table: Table,
        /// Its index in that table's lookup list.
        lookup: u16,
    },
    /// A contextual lookup named a lookup that named a lookup, past
    /// [`Limits::max_nesting_depth`].
    NestingTooDeep {
        /// Which table the lookup came from.
        table: Table,
        /// The lookup that was not run.
        lookup: u16,
    },
    /// A rule named a lookup index the lookup list does not have.
    MissingLookup {
        /// Which table the reference came from.
        table: Table,
        /// The index named.
        lookup: u16,
    },
    /// A rule's input sequence was longer than
    /// [`Limits::max_context_length`], so it was refused rather than
    /// truncated — a truncated match is a different rule silently applied.
    ContextTooLong {
        /// Which table the rule came from.
        table: Table,
    },
    /// [`Limits::max_operations`] was reached; the rest of the lookup list
    /// did not run.
    OperationBudgetExceeded {
        /// Which table was running.
        table: Table,
    },
    /// [`Limits::max_glyphs`] would have been exceeded by a substitution, so
    /// the substitution did not happen.
    GlyphBudgetExceeded {
        /// Which table was running.
        table: Table,
    },
}

/// What positioning does with the advance a mark glyph arrived with.
///
/// # Why this is a parameter rather than a rule
///
/// A face has to write *something* in `hmtx` for every glyph, and for a
/// combining accent it usually writes the same advance as everything else —
/// the aots fixtures are an honest example, with one horizontal metric of
/// 1500 units covering all hundred glyphs. A mark drawn with that advance
/// pushes the pen along and the word comes apart, so a shaper zeroes it.
///
/// It is a parameter and not a rule because it is not the same answer for
/// every script. A combining mark in Latin, Arabic and Hebrew is
/// non-spacing; a Hangul jamo classified as a mark is not; and an Indic
/// shaper handles the question inside its own reordering rather than here.
/// `docs/design/shaping.md` puts the per-script answer in milestones 2, 4 and
/// 5, so this milestone exposes the mechanism and takes the caller's word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkWidths {
    /// Zero the advance of every glyph `GDEF` calls a mark, before
    /// attachment is resolved. What a default, Arabic or Hebrew shaper wants.
    #[default]
    ZeroByGdef,
    /// Leave every advance exactly as the caller supplied it.
    AsSupplied,
}

/// A `GSUB` or `GPOS` table.
///
/// The two have the same header — three offsets and, from version 1.1, a
/// fourth — and differ only in what a lookup means, so one type reads both
/// and [`Table`] says which one this is.
#[derive(Clone, Copy, Debug)]
pub struct LayoutTable<'a> {
    data: Bytes<'a>,
    which: Table,
}

impl<'a> LayoutTable<'a> {
    /// Reads a layout table.
    ///
    /// `None` for a major version other than 1, which is the only one that
    /// exists; a face claiming version 2 is claiming a format nobody has
    /// written down, and reading it as though it were version 1 would be
    /// guessing.
    #[must_use]
    pub fn parse(bytes: &'a [u8], which: Table) -> Option<Self> {
        let data = Bytes::new(bytes);
        if data.u16(0)? != 1 {
            return None;
        }
        Some(Self { data, which })
    }

    /// Which of the two tables this is.
    #[must_use]
    pub const fn table(&self) -> Table {
        self.which
    }

    /// The table's minor version: 0, or 1 where it carries feature
    /// variations.
    #[must_use]
    pub fn minor_version(&self) -> u16 {
        self.data.u16(2).unwrap_or(0)
    }

    /// The scripts the face declares.
    #[must_use]
    pub fn scripts(&self) -> ScriptList<'a> {
        ScriptList::new(self.data.offset16(4).unwrap_or(Bytes::new(&[])))
    }

    /// The features the face carries.
    #[must_use]
    pub fn features(&self) -> FeatureList<'a> {
        FeatureList::new(self.data.offset16(6).unwrap_or(Bytes::new(&[])))
    }

    /// The lookups the face carries.
    #[must_use]
    pub fn lookups(&self) -> LookupList<'a> {
        LookupList::new(self.data.offset16(8).unwrap_or(Bytes::new(&[])))
    }

    /// The feature variations table, in a variable face at version 1.1.
    ///
    /// Surfaced, not applied; see [`FeatureVariations`] for why.
    #[must_use]
    pub fn feature_variations(&self) -> Option<FeatureVariations<'a>> {
        if self.minor_version() < 1 {
            return None;
        }
        self.data.offset32(10).map(FeatureVariations::new)
    }

    /// Which lookups to run for a script, a language and a set of features.
    ///
    /// # Sorted, and deduplicated, and both matter
    ///
    /// The specification says lookups are applied "in the order they appear
    /// in the lookup list", not in the order the features naming them appear
    /// — so this returns indices in ascending order whatever order the
    /// feature tags arrived in. And a lookup named by two of the requested
    /// features runs **once**: `liga` and `clig` routinely share one, and
    /// running it twice would ligate the ligature.
    ///
    /// The language system's *required* feature is always included, whether
    /// or not the caller asked for it, because that is what "required" means:
    /// a face that puts its Arabic joining behind a required feature is
    /// entitled to have it run.
    ///
    /// A script the face does not declare falls back to `DFLT`, and a
    /// language it does not declare falls back to the script's default
    /// language system. Both fallbacks are the specification's own and not
    /// leniencies: a face lists a script or a language only where it needs
    /// different behavior there.
    #[must_use]
    pub fn lookups_for(&self, script: Tag, language: Option<Tag>, features: &[Tag]) -> Vec<u16> {
        let scripts = self.scripts();
        let Some(script) = scripts
            .find(script)
            .or_else(|| scripts.find(Tag::DEFAULT_SCRIPT))
        else {
            return Vec::new();
        };
        let Some(lang) = script.lang_sys(language) else {
            return Vec::new();
        };
        let list = self.features();
        let mut wanted: Vec<u16> = Vec::new();
        if let Some(required) = lang.required_feature() {
            wanted.push(required);
        }
        for n in 0..lang.len() {
            let Some(index) = lang.feature(n) else {
                continue;
            };
            let Some(tag) = list.tag(usize::from(index)) else {
                continue;
            };
            if features.contains(&tag) {
                wanted.push(index);
            }
        }
        let mut lookups: Vec<u16> = Vec::new();
        for index in wanted {
            let Some(feature) = list.get(usize::from(index)) else {
                continue;
            };
            for n in 0..feature.len() {
                if let Some(lookup) = feature.lookup(n) {
                    lookups.push(lookup);
                }
            }
        }
        lookups.sort_unstable();
        lookups.dedup();
        lookups
    }
}

/// A face's OpenType Layout tables, together.
///
/// The three are held as one because they are read as one: a lookup flag in
/// `GSUB` is meaningless without `GDEF`'s glyph classes, and a mark
/// attachment in `GPOS` is meaningless without the ligature components
/// `GSUB` recorded.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout<'a> {
    gdef: Option<Gdef<'a>>,
    gsub: Option<LayoutTable<'a>>,
    gpos: Option<LayoutTable<'a>>,
}

/// `GDEF`, as the table directory spells it.
const GDEF: u32 = u32::from_be_bytes(*b"GDEF");
/// `GSUB`.
const GSUB: u32 = u32::from_be_bytes(*b"GSUB");
/// `GPOS`.
const GPOS: u32 = u32::from_be_bytes(*b"GPOS");

impl<'a> Layout<'a> {
    /// Reads whichever of the three tables a face carries.
    ///
    /// This is the only place `tinker-pdf-font` is used, and it is the whole
    /// reason for the dependency: `Sfnt` already disbelieves every offset and
    /// length in a table directory, and a second reader of the same twelve
    /// bytes in this workspace would be a second place for the same bug to
    /// live. A face with none of the three yields an empty [`Layout`] rather
    /// than an error — most faces in circulation have none, and "this face
    /// has no layout tables" is a fact about the face, not a failure.
    #[must_use]
    pub fn parse(sfnt: &Sfnt<'a>) -> Self {
        Self {
            gdef: sfnt.table(GDEF).and_then(Gdef::parse),
            gsub: sfnt
                .table(GSUB)
                .and_then(|bytes| LayoutTable::parse(bytes, Table::Gsub)),
            gpos: sfnt
                .table(GPOS)
                .and_then(|bytes| LayoutTable::parse(bytes, Table::Gpos)),
        }
    }

    /// The same, from table bytes a caller already has.
    ///
    /// For a consumer whose tables did not arrive in an sfnt — a `WOFF`
    /// decompressor, a test that assembles one table by hand, or the fuzz
    /// target, which drives each table independently so that a malformed
    /// `GSUB` is reached without a valid directory in front of it.
    #[must_use]
    pub fn from_tables(
        gdef: Option<&'a [u8]>,
        gsub: Option<&'a [u8]>,
        gpos: Option<&'a [u8]>,
    ) -> Self {
        Self {
            gdef: gdef.and_then(Gdef::parse),
            gsub: gsub.and_then(|bytes| LayoutTable::parse(bytes, Table::Gsub)),
            gpos: gpos.and_then(|bytes| LayoutTable::parse(bytes, Table::Gpos)),
        }
    }

    /// The face's `GDEF`, if it has one this crate could read.
    #[must_use]
    pub const fn gdef(&self) -> Option<&Gdef<'a>> {
        self.gdef.as_ref()
    }

    /// The face's `GSUB`.
    #[must_use]
    pub const fn gsub(&self) -> Option<&LayoutTable<'a>> {
        self.gsub.as_ref()
    }

    /// The face's `GPOS`.
    #[must_use]
    pub const fn gpos(&self) -> Option<&LayoutTable<'a>> {
        self.gpos.as_ref()
    }

    /// Runs `lookups` from `GSUB` over the buffer.
    ///
    /// The indices come from [`LayoutTable::lookups_for`] and are applied in
    /// the order given; each is run over the whole buffer before the next
    /// starts, which is the specification's model and not an implementation
    /// choice — a face's second lookup is written expecting the first to have
    /// finished.
    pub fn substitute(&self, buffer: &mut Buffer, lookups: &[u16], limits: Limits) -> Vec<Warning> {
        let Some(gsub) = self.gsub else {
            return Vec::new();
        };
        let mut runner = Runner::new(Table::Gsub, gsub.lookups(), self.gdef.as_ref(), limits);
        runner.run(buffer, lookups);
        runner.warnings
    }

    /// Runs `lookups` from `GPOS` over the buffer, then finishes positioning.
    ///
    /// # The finish is two passes, in this order, and the order is load-bearing
    ///
    /// Attachment is resolved first: mark and cursive lookups record
    /// *relationships* rather than offsets, because the glyph a mark hangs on
    /// may still be moved by a lookup that has not run, and resolving turns
    /// each relationship into a number by walking the advances between the
    /// two glyphs. **Only then** does `marks` decide whether a glyph `GDEF`
    /// calls a mark keeps the advance its `hmtx` gave it.
    ///
    /// Zeroing first would break the second mark of every stack. Two accents
    /// over one base are two attachments to the same glyph, and the second
    /// one's correction is the advance of the base *plus the advance of the
    /// first accent* — because that is how far the pen had travelled by the
    /// time it was drawn. Zero the first accent's advance before the walk and
    /// the second accent lands exactly on top of it.
    ///
    /// A caller that ran the lookups and neither pass would get marks at the
    /// origin, which is why this is one function rather than three.
    pub fn position(
        &self,
        buffer: &mut Buffer,
        lookups: &[u16],
        limits: Limits,
        marks: MarkWidths,
    ) -> Vec<Warning> {
        let Some(gpos) = self.gpos else {
            return Vec::new();
        };
        let mut runner = Runner::new(Table::Gpos, gpos.lookups(), self.gdef.as_ref(), limits);
        runner.run(buffer, lookups);
        buffer.propagate_attachments();
        if marks == MarkWidths::ZeroByGdef {
            buffer.zero_mark_advances();
        }
        runner.warnings
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, Table, Warning};

    #[test]
    fn a_face_with_no_layout_tables_is_empty_rather_than_an_error() {
        let layout = Layout::from_tables(None, None, None);
        assert!(layout.gdef().is_none());
        assert!(layout.gsub().is_none());
        assert!(layout.gpos().is_none());
        let mut buffer = crate::Buffer::from_glyphs(&[1, 2, 3]);
        assert!(layout
            .substitute(&mut buffer, &[0], crate::Limits::DEFAULT)
            .is_empty());
        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn a_version_this_crate_does_not_know_is_refused_rather_than_guessed() {
        let table = [0x00, 0x02, 0x00, 0x00, 0, 0, 0, 0, 0, 0];
        let layout = Layout::from_tables(None, Some(&table), None);
        assert!(layout.gsub().is_none());
    }

    #[test]
    fn a_warning_names_its_table() {
        let warning = Warning::MissingLookup {
            table: Table::Gpos,
            lookup: 7,
        };
        assert_eq!(Table::Gpos.to_string(), "GPOS");
        assert_eq!(
            warning,
            Warning::MissingLookup {
                table: Table::Gpos,
                lookup: 7
            }
        );
        assert_ne!(
            warning,
            Warning::MissingLookup {
                table: Table::Gsub,
                lookup: 7
            }
        );
    }
}
