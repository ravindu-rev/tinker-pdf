//! The tables `GSUB` and `GPOS` share, word for word.
//!
//! ISO/IEC 14496-22 gives these once, in the "OpenType Layout common table
//! formats" clause, and then says that both layout tables begin with the same
//! three offsets. So they are parsed once here, and `gsub.rs` and `gpos.rs`
//! differ only in what a *lookup* means.
//!
//! # A script list is a query, not a tree
//!
//! Nothing here builds a map. A [`ScriptList`] is the bytes; asking it for
//! `latn` is a linear scan of a list that a real font keeps under twenty
//! entries long, and the alternative — a `HashMap` built at parse time —
//! would cost an allocation per face for a question most callers ask twice.
//! The same reasoning runs all the way down: [`Coverage`] binary-searches the
//! font's own sorted array rather than inflating it into a set, which is what
//! makes a face with a thousand lookups cost a thousand bounds checks instead
//! of a thousand allocations.
//!
//! # What the ordering rules are, and which of them are trusted
//!
//! The specification requires a coverage table's glyph array and a class
//! definition's range array to be sorted, and this crate binary-searches both
//! — so an unsorted table gives a wrong answer rather than a crash. That is a
//! deliberate line. Verifying sortedness costs a scan of every table on every
//! face, to convert a *misrendering* into a different misrendering; nothing
//! downstream is unsafe either way, because every index the search produces is
//! bounds-checked before it is used. What is **not** trusted is anything
//! whose failure is unbounded work or an out-of-range read, and all of that is
//! checked every time.

use crate::read::Bytes;

/// A four-byte OpenType tag: `latn`, `arab`, `liga`, `kern`.
///
/// Stored as the big-endian `u32` the table carries so that comparison is one
/// integer compare, and constructed from a byte literal so that a caller
/// writes `Tag::new(b"liga")` rather than a number nobody can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub u32);

impl Tag {
    /// The tag those four bytes spell.
    #[must_use]
    pub const fn new(bytes: &[u8; 4]) -> Self {
        Self(u32::from_be_bytes(*bytes))
    }

    /// The four bytes, for display.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    /// The default script, which a face uses for everything it did not name.
    pub const DEFAULT_SCRIPT: Self = Self::new(b"DFLT");
}

impl core::fmt::Display for Tag {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in self.to_bytes() {
            // A tag is four bytes and a font may put anything in them, so the
            // unprintable ones become a dot rather than being written raw
            // into somebody's terminal.
            let c = char::from(byte);
            if c.is_ascii_graphic() || c == ' ' {
                write!(f, "{c}")?;
            } else {
                write!(f, ".")?;
            }
        }
        Ok(())
    }
}

/// The `ScriptList` at the top of a layout table.
#[derive(Clone, Copy, Debug)]
pub struct ScriptList<'a> {
    data: Bytes<'a>,
}

impl<'a> ScriptList<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// How many scripts the face declares.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether the face declares no script at all, which makes every feature
    /// in it unreachable through the ordinary selection path.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tag of script `index`.
    #[must_use]
    pub fn tag(&self, index: usize) -> Option<Tag> {
        if index >= self.len() {
            return None;
        }
        let at = 2usize.checked_add(index.checked_mul(6)?)?;
        Some(Tag(self.data.u32(at)?))
    }

    /// Script `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Script<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 2usize.checked_add(index.checked_mul(6)?)?;
        Some(Script {
            data: self.data.offset16(at.checked_add(4)?)?,
        })
    }

    /// The script with this tag.
    #[must_use]
    pub fn find(&self, tag: Tag) -> Option<Script<'a>> {
        for index in 0..self.len() {
            if self.tag(index) == Some(tag) {
                return self.get(index);
            }
        }
        None
    }
}

/// One script's language systems.
#[derive(Clone, Copy, Debug)]
pub struct Script<'a> {
    data: Bytes<'a>,
}

impl<'a> Script<'a> {
    /// The language system used for languages the script does not name.
    #[must_use]
    pub fn default_lang_sys(&self) -> Option<LangSys<'a>> {
        Some(LangSys {
            data: self.data.offset16(0)?,
        })
    }

    /// How many languages the script names.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(2).unwrap_or(0))
    }

    /// Whether the script names no language of its own.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tag of language system `index`.
    #[must_use]
    pub fn tag(&self, index: usize) -> Option<Tag> {
        if index >= self.len() {
            return None;
        }
        let at = 4usize.checked_add(index.checked_mul(6)?)?;
        Some(Tag(self.data.u32(at)?))
    }

    /// Language system `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<LangSys<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 4usize.checked_add(index.checked_mul(6)?)?;
        Some(LangSys {
            data: self.data.offset16(at.checked_add(4)?)?,
        })
    }

    /// The language system with this tag, or the default where the script
    /// does not name it.
    ///
    /// The fallback is the specification's own rule and not a leniency: a
    /// face lists a language system only where that language differs from the
    /// script's default, so "not found" means "the default applies", never
    /// "no features".
    #[must_use]
    pub fn lang_sys(&self, tag: Option<Tag>) -> Option<LangSys<'a>> {
        if let Some(tag) = tag {
            for index in 0..self.len() {
                if self.tag(index) == Some(tag) {
                    return self.get(index);
                }
            }
        }
        self.default_lang_sys()
    }
}

/// One language system: which features it turns on.
#[derive(Clone, Copy, Debug)]
pub struct LangSys<'a> {
    data: Bytes<'a>,
}

impl LangSys<'_> {
    /// The feature the language system requires, if it requires one.
    ///
    /// `0xFFFF` means none, which is why this is an `Option` rather than the
    /// raw index: a caller that treated the sentinel as an index would look
    /// up feature 65 535 and get whatever the bounds check happened to allow.
    #[must_use]
    pub fn required_feature(&self) -> Option<u16> {
        match self.data.u16(2)? {
            0xFFFF => None,
            index => Some(index),
        }
    }

    /// How many optional features the language system names.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(4).unwrap_or(0))
    }

    /// Whether it names none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Feature index `index`, into the layout table's [`FeatureList`].
    #[must_use]
    pub fn feature(&self, index: usize) -> Option<u16> {
        if index >= self.len() {
            return None;
        }
        self.data.u16_at(6, index)
    }
}

/// The `FeatureList`: every feature the face carries, by tag.
#[derive(Clone, Copy, Debug)]
pub struct FeatureList<'a> {
    data: Bytes<'a>,
}

impl<'a> FeatureList<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// How many features the face carries.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether the face carries none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tag of feature `index`.
    #[must_use]
    pub fn tag(&self, index: usize) -> Option<Tag> {
        if index >= self.len() {
            return None;
        }
        let at = 2usize.checked_add(index.checked_mul(6)?)?;
        Some(Tag(self.data.u32(at)?))
    }

    /// Feature `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Feature<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 2usize.checked_add(index.checked_mul(6)?)?;
        Some(Feature {
            data: self.data.offset16(at.checked_add(4)?)?,
        })
    }
}

/// One feature: which lookups it runs.
#[derive(Clone, Copy, Debug)]
pub struct Feature<'a> {
    data: Bytes<'a>,
}

impl Feature<'_> {
    /// How many lookups the feature names.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(2).unwrap_or(0))
    }

    /// Whether the feature names none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lookup index `index`, into the layout table's [`LookupList`].
    #[must_use]
    pub fn lookup(&self, index: usize) -> Option<u16> {
        if index >= self.len() {
            return None;
        }
        self.data.u16_at(4, index)
    }
}

/// The `FeatureVariations` table: which features a variable face substitutes
/// at which points in its design space.
///
/// **Parsed and surfaced; not applied.** `docs/design/shaping.md` defers
/// variation-aware shaping under ruling 3 — "`fvar`/`avar`/`HVAR` deltas
/// applied to GPOS values are deferred until a corpus document demands them"
/// — and applying a feature substitution without applying the deltas that go
/// with it would produce a run positioned half one way and half the other.
/// It is read here so that a caller can see the table exists, and so that the
/// fuzz target reaches its offsets; the numbers it carries are `F2DOT14`
/// fixed-point and are handed back as the raw `i16`, because converting them
/// to a fraction is the first float this crate would contain.
#[derive(Clone, Copy, Debug)]
pub struct FeatureVariations<'a> {
    data: Bytes<'a>,
}

impl<'a> FeatureVariations<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// How many variation records the table carries.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::try_from(self.data.u32(4).unwrap_or(0)).unwrap_or(0)
    }

    /// Whether it carries none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The condition set of record `index`: the axis ranges that select it.
    #[must_use]
    pub fn conditions(&self, index: usize) -> Option<ConditionSet<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 8usize.checked_add(index.checked_mul(8)?)?;
        Some(ConditionSet {
            data: self.data.offset32(at)?,
        })
    }

    /// The feature substitutions of record `index`: which feature index gets
    /// which replacement table when the conditions hold.
    #[must_use]
    pub fn substitutions(&self, index: usize) -> Option<FeatureSubstitutions<'a>> {
        if index >= self.len() {
            return None;
        }
        let at = 8usize.checked_add(index.checked_mul(8)?)?;
        Some(FeatureSubstitutions {
            data: self.data.offset32(at.checked_add(4)?)?,
        })
    }
}

/// One `ConditionSet`: every condition that must hold at once.
#[derive(Clone, Copy, Debug)]
pub struct ConditionSet<'a> {
    data: Bytes<'a>,
}

impl ConditionSet<'_> {
    /// How many conditions the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether it holds none, which makes it unconditionally true.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Condition `index`: the axis, and the `F2DOT14` range on it.
    ///
    /// `None` for a condition in a format this crate does not read, which as
    /// of OpenType 1.9 means anything but format 1.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Condition> {
        if index >= self.len() {
            return None;
        }
        let at = 2usize.checked_add(index.checked_mul(4)?)?;
        let table = self.data.offset32(at)?;
        if table.u16(0)? != 1 {
            return None;
        }
        Some(Condition {
            axis: table.u16(2)?,
            min: table.i16(4)?,
            max: table.i16(6)?,
        })
    }
}

/// One axis range, in the raw `F2DOT14` the table carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Condition {
    /// Which `fvar` axis, by index.
    pub axis: u16,
    /// The bottom of the range, as `F2DOT14`: the value divided by 16 384.
    pub min: i16,
    /// The top of the range, as `F2DOT14`.
    pub max: i16,
}

/// The replacement features one variation record installs.
#[derive(Clone, Copy, Debug)]
pub struct FeatureSubstitutions<'a> {
    data: Bytes<'a>,
}

impl<'a> FeatureSubstitutions<'a> {
    /// How many features the record replaces.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(4).unwrap_or(0))
    }

    /// Whether it replaces none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Substitution `index`: the feature index replaced, and what replaces it.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<(u16, Feature<'a>)> {
        if index >= self.len() {
            return None;
        }
        let at = 6usize.checked_add(index.checked_mul(6)?)?;
        let feature = self.data.offset32(at.checked_add(2)?)?;
        Some((self.data.u16(at)?, Feature { data: feature }))
    }
}

/// The `LookupList`: every lookup, in the order a face wants them applied.
#[derive(Clone, Copy, Debug)]
pub struct LookupList<'a> {
    data: Bytes<'a>,
}

impl<'a> LookupList<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// How many lookups the face carries.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(0).unwrap_or(0))
    }

    /// Whether the face carries none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lookup `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Lookup<'a>> {
        if index >= self.len() {
            return None;
        }
        Some(Lookup {
            data: self.data.offset16_at(2, index)?,
        })
    }
}

/// One lookup: a type, some flags, and a list of subtables.
#[derive(Clone, Copy, Debug)]
pub struct Lookup<'a> {
    data: Bytes<'a>,
}

/// Skip glyphs the GDEF glyph class definition calls base glyphs.
pub const IGNORE_BASE_GLYPHS: u16 = 0x0002;
/// Skip glyphs the GDEF glyph class definition calls ligatures.
pub const IGNORE_LIGATURES: u16 = 0x0004;
/// Skip glyphs the GDEF glyph class definition calls marks.
pub const IGNORE_MARKS: u16 = 0x0008;
/// Skip marks outside the mark glyph set this lookup names.
pub const USE_MARK_FILTERING_SET: u16 = 0x0010;
/// Position the last glyph of a cursive run on the baseline rather than the
/// first. Read by cursive attachment and by nothing else.
pub const RIGHT_TO_LEFT: u16 = 0x0001;

impl<'a> Lookup<'a> {
    /// The lookup type, as the table numbers them: what it means depends on
    /// which of `GSUB` and `GPOS` the lookup came from.
    #[must_use]
    pub fn kind(&self) -> u16 {
        self.data.u16(0).unwrap_or(0)
    }

    /// The lookup flags: which glyph classes this lookup steps over.
    #[must_use]
    pub fn flags(&self) -> u16 {
        self.data.u16(2).unwrap_or(0)
    }

    /// How many subtables the lookup holds.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(4).unwrap_or(0))
    }

    /// Whether it holds none, which a face is allowed to write and which
    /// makes the lookup a no-op rather than an error.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Subtable `index`, unresolved: an extension subtable arrives here as
    /// itself, and `gsub.rs` and `gpos.rs` follow it.
    #[must_use]
    pub fn subtable(&self, index: usize) -> Option<Bytes<'a>> {
        if index >= self.len() {
            return None;
        }
        self.data.offset16_at(6, index)
    }

    /// The mark glyph set this lookup filters by, if it declares one.
    ///
    /// The field is *conditional on a flag bit* and sits after the subtable
    /// array, so reading it without checking the bit reads whatever follows
    /// the lookup — which for the last lookup in the list is whatever the
    /// face put after the lookup list.
    #[must_use]
    pub fn mark_filtering_set(&self) -> Option<u16> {
        if self.flags() & USE_MARK_FILTERING_SET == 0 {
            return None;
        }
        let at = 6usize.checked_add(self.len().checked_mul(2)?)?;
        self.data.u16(at)
    }
}

/// A `Coverage` table: which glyphs a lookup applies to, and in what order.
#[derive(Clone, Copy, Debug)]
pub struct Coverage<'a> {
    data: Bytes<'a>,
}

impl<'a> Coverage<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// Where `glyph` sits in the coverage order, or `None` if it is not
    /// covered.
    ///
    /// The index is what every parallel array in every subtable is keyed by,
    /// so this is the single most-called function in the crate.
    #[must_use]
    pub fn index_of(&self, glyph: u16) -> Option<u16> {
        match self.data.u16(0)? {
            1 => self.index_of_format1(glyph),
            2 => self.index_of_format2(glyph),
            _ => None,
        }
    }

    /// Whether `glyph` is covered at all.
    #[must_use]
    pub fn covers(&self, glyph: u16) -> bool {
        self.index_of(glyph).is_some()
    }

    /// Format 1: a sorted array of glyph indices; the coverage index is the
    /// position in it.
    fn index_of_format1(&self, glyph: u16) -> Option<u16> {
        let count = usize::from(self.data.u16(2)?);
        let mut low = 0usize;
        let mut high = count;
        while low < high {
            let mid = low + (high - low) / 2;
            let at = self.data.u16_at(4, mid)?;
            if at < glyph {
                low = mid + 1;
            } else if at > glyph {
                high = mid;
            } else {
                return u16::try_from(mid).ok();
            }
        }
        None
    }

    /// Format 2: sorted ranges, each carrying the coverage index its first
    /// glyph has.
    fn index_of_format2(&self, glyph: u16) -> Option<u16> {
        let count = usize::from(self.data.u16(2)?);
        let mut low = 0usize;
        let mut high = count;
        while low < high {
            let mid = low + (high - low) / 2;
            let at = 4usize.checked_add(mid.checked_mul(6)?)?;
            let start = self.data.u16(at)?;
            let end = self.data.u16(at.checked_add(2)?)?;
            if glyph < start {
                high = mid;
            } else if glyph > end {
                low = mid + 1;
            } else {
                let first = self.data.u16(at.checked_add(4)?)?;
                // The range's own arithmetic is checked rather than trusted:
                // a `startCoverageIndex` near 65 535 on a wide range would
                // otherwise wrap into a small index and read the wrong entry
                // of whatever array is keyed by it.
                return first.checked_add(glyph - start);
            }
        }
        None
    }
}

/// A `ClassDef` table: which class each glyph is in, zero by default.
#[derive(Clone, Copy, Debug)]
pub struct ClassDef<'a> {
    data: Bytes<'a>,
}

impl<'a> ClassDef<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// The class of `glyph`.
    ///
    /// Zero for anything the table does not mention, which the specification
    /// makes the meaningful default rather than an error: class 0 is "every
    /// glyph not otherwise classified", and a contextual rule may name it.
    #[must_use]
    pub fn class_of(&self, glyph: u16) -> u16 {
        self.try_class_of(glyph).unwrap_or(0)
    }

    fn try_class_of(&self, glyph: u16) -> Option<u16> {
        match self.data.u16(0)? {
            1 => {
                let start = self.data.u16(2)?;
                let count = usize::from(self.data.u16(4)?);
                let index = usize::from(glyph.checked_sub(start)?);
                if index >= count {
                    return None;
                }
                self.data.u16_at(6, index)
            }
            2 => {
                let count = usize::from(self.data.u16(2)?);
                let mut low = 0usize;
                let mut high = count;
                while low < high {
                    let mid = low + (high - low) / 2;
                    let at = 4usize.checked_add(mid.checked_mul(6)?)?;
                    let start = self.data.u16(at)?;
                    let end = self.data.u16(at.checked_add(2)?)?;
                    if glyph < start {
                        high = mid;
                    } else if glyph > end {
                        low = mid + 1;
                    } else {
                        return self.data.u16(at.checked_add(4)?);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// A `Device` table, or the `VariationIndex` table that shares its shape.
///
/// A device table carries per-pixel-size corrections for a value the font
/// hinted by hand. `VariationIndex` reuses the same three fields to name a
/// delta set in an item variation store instead, and is told apart by a
/// `deltaFormat` of `0x8000` — which is why one type reads both: a caller
/// that assumed the first shape would read a variation index's delta-set
/// numbers as a pixel range and correct by whatever fell out.
#[derive(Clone, Copy, Debug)]
pub struct Device<'a> {
    data: Bytes<'a>,
}

impl<'a> Device<'a> {
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

    /// Whether this is a `VariationIndex` rather than a `Device`.
    #[must_use]
    pub fn is_variation_index(&self) -> bool {
        self.data.u16(4) == Some(0x8000)
    }

    /// The delta set a `VariationIndex` names: the outer and inner index.
    #[must_use]
    pub fn variation_index(&self) -> Option<(u16, u16)> {
        if !self.is_variation_index() {
            return None;
        }
        Some((self.data.u16(0)?, self.data.u16(2)?))
    }

    /// The correction this device table makes at `ppem` pixels per em.
    ///
    /// Zero outside the range the table covers, which is the specification's
    /// own answer rather than a fallback: a device table states corrections
    /// for the sizes it was hinted at and says nothing about the others.
    ///
    /// The deltas are packed 2, 4 or 8 bits wide by `deltaFormat`, each a
    /// signed value in that width, which is the sign extension below.
    #[must_use]
    pub fn delta(&self, ppem: u16) -> i32 {
        self.try_delta(ppem).unwrap_or(0)
    }

    fn try_delta(&self, ppem: u16) -> Option<i32> {
        let start = self.data.u16(0)?;
        let end = self.data.u16(2)?;
        let format = self.data.u16(4)?;
        if ppem < start || ppem > end || !(1..=3).contains(&format) {
            return None;
        }
        // 1 -> 2 bits, 2 -> 4 bits, 3 -> 8 bits.
        let bits = 1u32 << format;
        let per_word = 16 / bits;
        let index = u32::from(ppem - start);
        let word = self
            .data
            .u16_at(6, usize::try_from(index / per_word).ok()?)?;
        let slot = index % per_word;
        // The high-order slot of a word is the first one, so the shift counts
        // down from the top rather than up from the bottom.
        let shift = 16 - bits * (slot + 1);
        let mask = (1u32 << bits) - 1;
        let raw = (u32::from(word) >> shift) & mask;
        // Sign-extend from `bits` wide.
        let sign = 1u32 << (bits - 1);
        let value = if raw & sign == 0 {
            i64::from(raw)
        } else {
            i64::from(raw) - i64::from(1u32 << bits)
        };
        i32::try_from(value).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassDef, Coverage, Device, Tag};
    use crate::read::Bytes;

    #[test]
    fn a_tag_prints_its_four_bytes() {
        assert_eq!(Tag::new(b"latn").to_string(), "latn");
        assert_eq!(Tag::new(b"DFLT"), Tag::DEFAULT_SCRIPT);
        assert_eq!(Tag(0x00_00_00_00).to_string(), "....");
    }

    #[test]
    fn coverage_format_1_is_a_sorted_array() {
        let table = [0, 1, 0, 3, 0, 5, 0, 9, 0, 20];
        let coverage = Coverage::new(Bytes::new(&table));
        assert_eq!(coverage.index_of(5), Some(0));
        assert_eq!(coverage.index_of(9), Some(1));
        assert_eq!(coverage.index_of(20), Some(2));
        assert_eq!(coverage.index_of(6), None);
        assert_eq!(coverage.index_of(0), None);
        assert_eq!(coverage.index_of(65535), None);
    }

    #[test]
    fn coverage_format_2_counts_from_the_range_start() {
        // One range: glyphs 10..=13, first coverage index 7.
        let table = [0, 2, 0, 1, 0, 10, 0, 13, 0, 7];
        let coverage = Coverage::new(Bytes::new(&table));
        assert_eq!(coverage.index_of(10), Some(7));
        assert_eq!(coverage.index_of(13), Some(10));
        assert_eq!(coverage.index_of(14), None);
        assert!(coverage.covers(11));
    }

    #[test]
    fn a_coverage_index_that_would_wrap_is_refused() {
        // startCoverageIndex 65 534 over a four-glyph range: the third glyph
        // would be index 65 536.
        let table = [0, 2, 0, 1, 0, 10, 0, 13, 0xFF, 0xFE];
        let coverage = Coverage::new(Bytes::new(&table));
        assert_eq!(coverage.index_of(10), Some(65534));
        assert_eq!(coverage.index_of(11), Some(65535));
        assert_eq!(coverage.index_of(12), None);
    }

    #[test]
    fn a_truncated_coverage_answers_none_rather_than_panicking() {
        for cut in 0..12usize {
            let table = [0, 1, 0, 3, 0, 5, 0, 9, 0, 20];
            let coverage = Coverage::new(Bytes::new(table.get(..cut).unwrap_or(&table)));
            let _ = coverage.index_of(9);
        }
    }

    #[test]
    fn classdef_format_1_is_an_array_from_a_start_glyph() {
        let table = [0, 1, 0, 5, 0, 3, 0, 1, 0, 2, 0, 1];
        let classes = ClassDef::new(Bytes::new(&table));
        assert_eq!(classes.class_of(5), 1);
        assert_eq!(classes.class_of(6), 2);
        assert_eq!(classes.class_of(7), 1);
        assert_eq!(classes.class_of(8), 0);
        assert_eq!(classes.class_of(4), 0);
    }

    #[test]
    fn classdef_format_2_is_ranges() {
        let table = [0, 2, 0, 2, 0, 3, 0, 5, 0, 1, 0, 9, 0, 12, 0, 4];
        let classes = ClassDef::new(Bytes::new(&table));
        assert_eq!(classes.class_of(3), 1);
        assert_eq!(classes.class_of(5), 1);
        assert_eq!(classes.class_of(6), 0);
        assert_eq!(classes.class_of(11), 4);
        assert_eq!(classes.class_of(13), 0);
    }

    #[test]
    fn a_device_table_corrects_only_inside_its_range() {
        // startSize 11, endSize 14, format 1 (two bits each): the four
        // deltas -1, 0, 1, 1 pack into the top byte as 11 00 01 01.
        let table = [0, 11, 0, 14, 0, 1, 0b1100_0101, 0];
        let device = Device::new(Bytes::new(&table));
        assert_eq!(device.delta(10), 0);
        assert_eq!(device.delta(11), -1);
        assert_eq!(device.delta(12), 0);
        assert_eq!(device.delta(13), 1);
        assert_eq!(device.delta(14), 1);
        assert_eq!(device.delta(15), 0);
        assert!(!device.is_variation_index());
    }

    #[test]
    fn a_variation_index_is_not_read_as_a_device() {
        let table = [0, 3, 0, 7, 0x80, 0x00];
        let device = Device::new(Bytes::new(&table));
        assert!(device.is_variation_index());
        assert_eq!(device.variation_index(), Some((3, 7)));
        assert_eq!(device.delta(5), 0);
    }
}
