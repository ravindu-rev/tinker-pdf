//! The tables `GSUB` and `GPOS` share, word for word.
//!
//! ISO/IEC 14496-22 gives these once, in the "OpenType Layout common table
//! formats" clause, and then says that both layout tables begin with the same
//! three offsets. So they are parsed once here, and `gsub.rs` and `gpos.rs`
//! differ only in what a *lookup* means.
//!
//! # A script list is a query, not a tree
//!
//! Nothing here builds a map at parse. A [`ScriptList`] is the bytes; asking
//! it for `latn` is a linear scan of a list that a real font keeps under
//! twenty entries long, and the alternative — a `HashMap` built at parse time
//! — would cost an allocation per face for a question most callers ask twice.
//! The same reasoning runs all the way down: [`Coverage`] binary-searches the
//! font's own sorted array rather than inflating it into a set, which is what
//! makes a face with a thousand lookups cost a thousand bounds checks instead
//! of a thousand allocations.
//!
//! The one question asked more than twice is whether a planned lookup's
//! subtables cover the glyph at a position: a paragraph asks it per glyph per
//! lookup. Its answer is kept, but not here and not at parse: a
//! [`crate::Plans`] a caller keeps beside a face builds a [`GlyphSet`] per
//! subtable from [`Coverage::ranges`] the first time a plan names the lookup,
//! and a lookup nobody plans is never read that way.
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
    /// A lookup over its own bytes, for one that lives outside a
    /// `LookupList`: a `JSTF` table's `JstfMax` lookups.
    pub(crate) const fn new(data: Bytes<'a>) -> Self {
        Self { data }
    }

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

    /// Appends to `out` every glyph this table could answer for, as
    /// inclusive ranges: each glyph a format 1 array lists and each range a
    /// format 2 table gives, read as far as the table's bytes go.
    ///
    /// A superset of what [`Coverage::index_of`] finds, whatever the table
    /// says: a binary search finds only what is listed, and a listing that
    /// is unsorted, overlapping or wrapping past index 65 535 only makes the
    /// search find less. A range whose end is below its start lists nothing.
    ///
    /// The entries read are charged to `budget` first, two bytes a glyph and
    /// six a range, and `None` — with nothing appended — is a table whose
    /// entries cost more than is left. A format this crate does not read
    /// answers for no glyph and costs nothing.
    pub(crate) fn ranges(&self, out: &mut Vec<(u16, u16)>, budget: &mut usize) -> Option<()> {
        let (size, start) = match self.data.u16(0) {
            Some(1) => (2usize, 4usize),
            Some(2) => (6, 4),
            _ => return Some(()),
        };
        let Some(count) = self.data.u16(2).map(usize::from) else {
            return Some(());
        };
        let readable = self.data.len().saturating_sub(start) / size;
        let entries = count.min(readable);
        let cost = entries.checked_mul(size)?;
        *budget = budget.checked_sub(cost)?;
        out.reserve(entries);
        for entry in 0..entries {
            let at = start.checked_add(entry.checked_mul(size)?)?;
            let range = if size == 2 {
                let glyph = self.data.u16(at)?;
                (glyph, glyph)
            } else {
                (self.data.u16(at)?, self.data.u16(at.checked_add(2)?)?)
            };
            if range.0 <= range.1 {
                out.push(range);
            }
        }
        Some(())
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

/// A set of glyph indices, as a bitmap over the 64-glyph blocks from its
/// lowest glyph's to its highest's.
///
/// What a coverage index is made of (`docs/design/shaping.md`, *Lookup plans
/// and coverage indexes*): asking about a glyph is a subtraction, a load and
/// a shift, where asking the coverage table it came from is a binary search
/// of bounds-checked reads. Blocks are aligned to multiples of 64, so two
/// sets combine word for word, and a set spanning the whole 16-bit glyph
/// space is 1 024 words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GlyphSet {
    /// The block of the set's first word: its lowest glyph divided by 64.
    base: u16,
    words: Vec<u64>,
}

impl GlyphSet {
    /// The set of every glyph `ranges` covers, each `(first, last)`
    /// inclusive.
    ///
    /// The ranges are sorted and merged first unless they already ascend
    /// without overlapping, which is what a well-formed coverage gives and
    /// what keeps the fill linear: a hostile table of overlapping ranges
    /// would otherwise set the same words once per range.
    pub(crate) fn from_ranges(ranges: &mut Vec<(u16, u16)>) -> GlyphSet {
        let ascending = ranges
            .windows(2)
            .all(|pair| matches!(pair, [one, two] if one.1 < two.0));
        if !ascending {
            ranges.sort_unstable();
            let mut merged: Vec<(u16, u16)> = Vec::with_capacity(ranges.len());
            for &(first, last) in ranges.iter() {
                match merged.last_mut() {
                    Some(top) if u32::from(first) <= u32::from(top.1) + 1 => {
                        top.1 = top.1.max(last);
                    }
                    _ => merged.push((first, last)),
                }
            }
            *ranges = merged;
        }
        let (Some(&(low, _)), Some(&(_, high))) = (ranges.first(), ranges.last()) else {
            return GlyphSet::default();
        };
        let base = low >> 6;
        let mut words = vec![0u64; usize::from((high >> 6) - base) + 1];
        for &(first, last) in ranges.iter() {
            let (first, last) = (usize::from(first), usize::from(last));
            for block in (first >> 6)..=(last >> 6) {
                let low_bit = if block == first >> 6 { first & 63 } else { 0 };
                let high_bit = if block == last >> 6 { last & 63 } else { 63 };
                // Bits `low_bit..=high_bit`: between one and sixty-four of
                // them, so neither shift reaches 64.
                let mask = (!0u64 >> (63 - (high_bit - low_bit))) << low_bit;
                if let Some(word) = block
                    .checked_sub(usize::from(base))
                    .and_then(|slot| words.get_mut(slot))
                {
                    *word |= mask;
                }
            }
        }
        GlyphSet { base, words }
    }

    /// The union of `sets`.
    pub(crate) fn union(sets: &[GlyphSet]) -> GlyphSet {
        let mut held = sets.iter().filter(|set| !set.words.is_empty());
        let Some(first) = held.next() else {
            return GlyphSet::default();
        };
        let mut base = first.base;
        let mut end = first.end();
        for set in held {
            base = base.min(set.base);
            end = end.max(set.end());
        }
        let mut words = vec![0u64; end.saturating_sub(usize::from(base))];
        for set in sets {
            let from = usize::from(set.base).saturating_sub(usize::from(base));
            for (slot, word) in words.iter_mut().skip(from).zip(&set.words) {
                *slot |= word;
            }
        }
        GlyphSet { base, words }
    }

    /// One past the set's last block.
    fn end(&self) -> usize {
        usize::from(self.base) + self.words.len()
    }

    /// Whether `glyph` is in the set.
    pub(crate) fn holds(&self, glyph: u16) -> bool {
        usize::from(glyph >> 6)
            .checked_sub(usize::from(self.base))
            .and_then(|slot| self.words.get(slot))
            .is_some_and(|word| (word >> (glyph & 63)) & 1 != 0)
    }

    /// The bytes the set's bitmap holds.
    pub(crate) fn bytes(&self) -> usize {
        self.words.len().saturating_mul(8)
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
    use super::{ClassDef, Coverage, Device, GlyphSet, Tag};
    use crate::read::Bytes;

    /// A bitmap holds exactly the glyphs its ranges cover — sorted or not,
    /// overlapping or not, across a block's edge and at both ends of the
    /// glyph space.
    #[test]
    fn a_glyph_set_holds_exactly_its_ranges() {
        let cases: [&[(u16, u16)]; 10] = [
            &[],
            &[(5, 5)],
            &[(0, 0), (65535, 65535)],
            &[(10, 70), (60, 200), (3, 4)],
            &[(1, 1), (2, 2), (3, 3)],
            &[(64, 127)],
            &[(63, 64)],
            &[(0, 65535)],
            &[(9, 9), (9, 9), (8, 10)],
            &[(65000, 65535), (0, 63), (1000, 1000)],
        ];
        for ranges in cases {
            let set = GlyphSet::from_ranges(&mut ranges.to_vec());
            for glyph in 0..=u16::MAX {
                let expected = ranges
                    .iter()
                    .any(|(first, last)| (*first..=*last).contains(&glyph));
                assert_eq!(set.holds(glyph), expected, "{ranges:?} at {glyph}");
            }
            assert!(set.bytes() <= 1024 * 8, "{ranges:?}: {} bytes", set.bytes());
        }
        assert_eq!(GlyphSet::from_ranges(&mut Vec::new()).bytes(), 0);
        assert_eq!(
            GlyphSet::from_ranges(&mut vec![(70, 80)]).bytes(),
            8,
            "one block"
        );
    }

    #[test]
    fn a_union_holds_what_any_of_its_sets_holds() {
        let a = GlyphSet::from_ranges(&mut vec![(3, 9), (200, 210)]);
        let b = GlyphSet::from_ranges(&mut vec![(100, 130), (65000, 65535)]);
        let c = GlyphSet::from_ranges(&mut vec![(5, 6)]);
        let empty = GlyphSet::default();
        let union = GlyphSet::union(&[a.clone(), empty.clone(), b.clone(), c.clone()]);
        for glyph in 0..=u16::MAX {
            let expected = a.holds(glyph) || b.holds(glyph) || c.holds(glyph);
            assert_eq!(union.holds(glyph), expected, "{glyph}");
        }
        assert_eq!(GlyphSet::union(&[]), GlyphSet::default());
        assert_eq!(
            GlyphSet::union(&[empty.clone(), empty]),
            GlyphSet::default()
        );
        assert_eq!(GlyphSet::union(&[c.clone()]), c);
    }

    /// `Coverage::ranges` lists every glyph the search can find, whatever
    /// shape the table is in, and charges two bytes a glyph and six a range
    /// as far as the table's bytes go.
    #[test]
    fn a_coverage_lists_everything_its_search_finds() {
        let tables: [&[u8]; 9] = [
            &[0, 1, 0, 3, 0, 5, 0, 9, 0, 20],
            // Unsorted: the search finds less than is listed.
            &[0, 1, 0, 3, 0, 20, 0, 5, 0, 9],
            &[0, 2, 0, 1, 0, 10, 0, 13, 0, 7],
            // A start index that wraps past 65 535.
            &[0, 2, 0, 1, 0, 10, 0, 13, 0xFF, 0xFE],
            // Ranges out of order.
            &[0, 2, 0, 2, 0, 50, 0, 60, 0, 0, 0, 10, 0, 20, 0, 11],
            // A range that ends below its start lists nothing.
            &[0, 2, 0, 1, 0, 13, 0, 10, 0, 0],
            // A count past the table's bytes.
            &[0, 1, 0, 9, 0, 5],
            // A format nothing reads.
            &[0, 3, 0, 1, 0, 5],
            // No count at all.
            &[0, 1],
        ];
        for bytes in tables {
            let coverage = Coverage::new(Bytes::new(bytes));
            let mut ranges = Vec::new();
            let mut budget = usize::MAX;
            assert_eq!(coverage.ranges(&mut ranges, &mut budget), Some(()));
            let set = GlyphSet::from_ranges(&mut ranges);
            for glyph in 0..=u16::MAX {
                if coverage.index_of(glyph).is_some() {
                    assert!(
                        set.holds(glyph),
                        "{bytes:?} finds {glyph}, the ranges miss it"
                    );
                }
            }
        }
        let reversed: [u8; 10] = [0, 2, 0, 1, 0, 13, 0, 10, 0, 0];
        let mut ranges = Vec::new();
        let mut budget = usize::MAX;
        let _ = Coverage::new(Bytes::new(&reversed)).ranges(&mut ranges, &mut budget);
        assert!(ranges.is_empty(), "a reversed range is no range");

        let unsorted: [u8; 16] = [0, 2, 0, 2, 0, 50, 0, 60, 0, 0, 0, 10, 0, 20, 0, 11];
        let coverage = Coverage::new(Bytes::new(&unsorted));
        let (mut ranges, mut budget) = (Vec::new(), 12usize);
        assert_eq!(coverage.ranges(&mut ranges, &mut budget), Some(()));
        assert_eq!((budget, ranges), (0, vec![(50, 60), (10, 20)]));
        let (mut ranges, mut budget) = (Vec::new(), 11usize);
        assert_eq!(coverage.ranges(&mut ranges, &mut budget), None);
        assert_eq!(
            (budget, ranges),
            (11, Vec::new()),
            "refused, and nothing read or charged"
        );

        let truncated: [u8; 6] = [0, 1, 0, 9, 0, 5];
        let (mut ranges, mut budget) = (Vec::new(), 2usize);
        assert_eq!(
            Coverage::new(Bytes::new(&truncated)).ranges(&mut ranges, &mut budget),
            Some(())
        );
        assert_eq!(
            (budget, ranges),
            (0, vec![(5, 5)]),
            "as far as the bytes go"
        );
    }

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
