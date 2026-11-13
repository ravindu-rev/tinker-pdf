//! `JSTF`: a face's justification suggestions, read.
//!
//! ISO/IEC 14496-22's justification table, as OpenType 1.9.1 publishes its
//! `JSTF` chapter: scripts, each with its extender glyphs — the kashidas of
//! Arabic — and its language systems, each a list of **priorities**, the
//! first the least obtrusive suggestion. A priority is ten offsets: for
//! shrinkage and for extension, the `GSUB` and the `GPOS` lookups to enable
//! and to disable, as indices into those tables' `LookupList`s, and a
//! `JstfMax` of lookups of the `GPOS` form whose values are the most a glyph
//! may move. `docs/design/shaping.md`, *Justification*, says how a justify
//! call applies them; this module only reads them.
//!
//! Every structure is a view over the table's bytes, as the rest of the
//! crate's tables are (`read::Bytes`): nothing is copied, every offset is
//! measured from the table that holds it, a NULL offset is an absent table,
//! and an offset past the end, a count past its bytes or a major version
//! other than 1 answers `None` rather than reading anything (ruling 1).

use crate::common::{Lookup, Tag};
use crate::read::Bytes;
use crate::Table;

/// Shrinking a line or extending it: which half of a priority is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Making the run shorter.
    Shrinkage,
    /// Making the run longer.
    Extension,
}

/// Whether a modification list's lookups are switched on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    /// The lookups are applied, where the plan does not already.
    Enable,
    /// The lookups are taken out of the plan.
    Disable,
}

/// A face's `JSTF` table.
#[derive(Clone, Copy, Debug)]
pub struct Jstf<'a> {
    data: Bytes<'a>,
}

impl<'a> Jstf<'a> {
    /// The table, if its header is whole and its major version is 1, the
    /// only one the chapter defines.
    #[must_use]
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let data = Bytes::new(bytes);
        let major = data.u16(0)?;
        data.u16(2)?;
        data.u16(4)?;
        (major == 1).then_some(Self { data })
    }

    /// The minor version.
    #[must_use]
    pub fn minor_version(&self) -> u16 {
        self.data.u16(2).unwrap_or(0)
    }

    /// How many script records the table declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(4).unwrap_or(0))
    }

    /// Whether it declares none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tag of script record `index`.
    #[must_use]
    pub fn tag(&self, index: usize) -> Option<Tag> {
        if index >= self.len() {
            return None;
        }
        let at = 6usize.checked_add(index.checked_mul(6)?)?;
        Some(Tag(self.data.u32(at)?))
    }

    /// Script record `index`'s table.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<JstfScript<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 6usize.checked_add(index.checked_mul(6)?)?;
        Some(JstfScript {
            data: self.data.offset16(at.checked_add(4)?)?,
        })
    }

    /// The script with this tag: the first record carrying it.
    #[must_use]
    pub fn find(&self, tag: Tag) -> Option<JstfScript<'a>> {
        (0..self.len())
            .find(|index| self.tag(*index) == Some(tag))
            .and_then(|index| self.get(index))
    }
}

/// One script's justification data: its extender glyphs and its language
/// systems.
#[derive(Clone, Copy, Debug)]
pub struct JstfScript<'a> {
    data: Bytes<'a>,
}

impl<'a> JstfScript<'a> {
    /// The extender glyphs a client may insert to lengthen a line — the
    /// kashidas, where the script has them. `None` for a NULL offset.
    #[must_use]
    pub fn extender_glyphs(&self) -> Option<ExtenderGlyphs<'a>> {
        let data = self.data.offset16(0)?;
        data.u16(0)?;
        Some(ExtenderGlyphs { data })
    }

    /// The language system that applies where no record names the text's
    /// language. `None` for a NULL offset.
    #[must_use]
    pub fn default_lang_sys(&self) -> Option<JstfLangSys<'a>> {
        let data = self.data.offset16(2)?;
        data.u16(0)?;
        Some(JstfLangSys { data })
    }

    /// How many language system records the script declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(4).unwrap_or(0))
    }

    /// Whether it declares none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tag of language system record `index`.
    #[must_use]
    pub fn tag(&self, index: usize) -> Option<Tag> {
        if index >= self.len() {
            return None;
        }
        let at = 6usize.checked_add(index.checked_mul(6)?)?;
        Some(Tag(self.data.u32(at)?))
    }

    /// Language system record `index`'s table.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<JstfLangSys<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 6usize.checked_add(index.checked_mul(6)?)?;
        let data = self.data.offset16(at.checked_add(4)?)?;
        data.u16(0)?;
        Some(JstfLangSys { data })
    }

    /// The language system for a language: its record's where the script
    /// has one for the tag, the default where it has none or no tag is
    /// given.
    #[must_use]
    pub fn lang_sys(&self, tag: Option<Tag>) -> Option<JstfLangSys<'a>> {
        if let Some(tag) = tag {
            if let Some(index) = (0..self.len()).find(|index| self.tag(*index) == Some(tag)) {
                return self.get(index);
            }
        }
        self.default_lang_sys()
    }
}

/// A script's extender glyphs, in the order the table lists them —
/// increasing glyph id, the chapter says, which is not relied on.
#[derive(Clone, Copy, Debug)]
pub struct ExtenderGlyphs<'a> {
    data: Bytes<'a>,
}

impl ExtenderGlyphs<'_> {
    /// How many glyphs the table declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether it declares none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Glyph `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<u16> {
        if index >= self.len() {
            return None;
        }
        self.data.u16_at(2, index)
    }
}

/// One language system's justification suggestions, in priority order.
#[derive(Clone, Copy, Debug)]
pub struct JstfLangSys<'a> {
    data: Bytes<'a>,
}

impl<'a> JstfLangSys<'a> {
    /// How many priorities the language system declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether it declares none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Priority `index`, the first the least obtrusive. `None` for a NULL
    /// offset or one past the table.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<JstfPriority<'a>> {
        if index >= self.len() {
            return None;
        }
        let data = self.data.offset16_at(2, index)?;
        // The ten offsets, so that every accessor below reads inside it.
        data.u16(18)?;
        Some(JstfPriority { data })
    }
}

/// One priority: for shrinkage and for extension, the lookups to enable and
/// to disable in each of `GSUB` and `GPOS`, and the `JstfMax` lookups.
#[derive(Clone, Copy, Debug)]
pub struct JstfPriority<'a> {
    data: Bytes<'a>,
}

impl<'a> JstfPriority<'a> {
    /// Where a mode's five offsets begin: `gsubShrinkageEnableOffset` for
    /// shrinkage, `gsubExtensionEnableOffset` for extension.
    const fn base(mode: Mode) -> usize {
        match mode {
            Mode::Shrinkage => 0,
            Mode::Extension => 10,
        }
    }

    /// The modification list for one mode, table and change: the chapter's
    /// `gsubShrinkageEnable` … `gposExtensionDisable`. `None` for a NULL
    /// offset, one past the table, or a list whose count is not there.
    #[must_use]
    pub fn mod_list(&self, mode: Mode, table: Table, change: Change) -> Option<JstfModList<'a>> {
        let within = match (table, change) {
            (Table::Gsub, Change::Enable) => 0,
            (Table::Gsub, Change::Disable) => 2,
            (Table::Gpos, Change::Enable) => 4,
            (Table::Gpos, Change::Disable) => 6,
        };
        let data = self.data.offset16(Self::base(mode) + within)?;
        data.u16(0)?;
        Some(JstfModList { data })
    }

    /// The `JstfMax` for one mode: `shrinkageJstfMax` or `extensionJstfMax`.
    /// `None` for a NULL offset, one past the table, or a table whose count
    /// is not there.
    #[must_use]
    pub fn max(&self, mode: Mode) -> Option<JstfMax<'a>> {
        let data = self.data.offset16(Self::base(mode) + 8)?;
        data.u16(0)?;
        Some(JstfMax { data })
    }
}

/// A list of lookup indices into the `GSUB` or the `GPOS` `LookupList`.
#[derive(Clone, Copy, Debug)]
pub struct JstfModList<'a> {
    data: Bytes<'a>,
}

impl JstfModList<'_> {
    /// How many lookups the list declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether it declares none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lookup index `index`.
    #[must_use]
    pub fn lookup(&self, index: usize) -> Option<u16> {
        if index >= self.len() {
            return None;
        }
        self.data.u16_at(2, index)
    }
}

/// A `JstfMax`: lookups of the `GPOS` form, in the `JSTF` table itself,
/// whose values are the most a glyph may be moved.
#[derive(Clone, Copy, Debug)]
pub struct JstfMax<'a> {
    data: Bytes<'a>,
}

impl<'a> JstfMax<'a> {
    /// How many lookups the table declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether it declares none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lookup `index`, measured from the `JstfMax` table; `None` for a NULL
    /// offset, one past the table, or a lookup whose header is not there.
    #[must_use]
    pub fn lookup(&self, index: usize) -> Option<Lookup<'a>> {
        if index >= self.len() {
            return None;
        }
        let data = self.data.offset16_at(2, index)?;
        // `lookupType`, `lookupFlag` and `subTableCount`.
        data.u16(4)?;
        Some(Lookup::new(data))
    }
}
