//! The feature name table, `feat`: which of a face's AAT features take
//! settings that exclude one another.
//!
//! Apple's *TrueType Reference Manual*, *The 'feat' table*, "Feature Name
//! Table Format": a `fixed32` version, 1.0, a `uint16` feature count, six
//! reserved bytes, then the feature name array, "sorted by feature type", of
//! twelve bytes a feature — its type, its setting count, the offset of its
//! setting name array "from the beginning of this table", its `featureFlags`
//! and a name index. Of the flags, `0x8000` marks the feature's settings
//! "mutually exclusive", and `0x4000` says the low byte (`0x00FF`, the
//! chapter's mask; its prose says bits 0 to 15 and 0 to 16 in two places)
//! holds the index of its default setting, "the setting with an index of 0"
//! being the default otherwise. A setting is a `uint16` and a name index.
//!
//! The `morx` chain reads exclusivity from it and nothing else
//! (`docs/design/shaping.md`, *`feat`*); the rest is read for what a caller
//! asks of it. A table that cannot be read, a version other than 1, or a
//! feature the table does not list answers nothing, and the chain falls back
//! on what the Font Feature Registry marks exclusive.

use crate::common::Tag;
use crate::read::Bytes;

/// `feat`, as the face's table directory spells it.
pub const FEAT: Tag = Tag::new(b"feat");

/// The bytes of one feature name record.
const RECORD: usize = 12;
/// Where the feature name array starts.
const NAMES: usize = 12;
/// The `featureFlags` bit marking a feature's settings exclusive.
const EXCLUSIVE: u16 = 0x8000;
/// The `featureFlags` bit saying the low byte names the default setting.
const DEFAULT_GIVEN: u16 = 0x4000;
/// The low byte of `featureFlags`: the default setting's index.
const DEFAULT_INDEX: u16 = 0x00FF;

/// A `feat` table.
#[derive(Clone, Copy, Debug)]
pub struct Feat<'a> {
    data: Bytes<'a>,
    count: u16,
}

impl<'a> Feat<'a> {
    /// Reads a `feat` header; `None` for a major version other than 1 or a
    /// header that cannot be read.
    #[must_use]
    pub fn parse(bytes: &'a [u8]) -> Option<Feat<'a>> {
        let data = Bytes::new(bytes);
        if data.u16(0)? != 1 {
            return None;
        }
        let count = data.u16(4)?;
        Some(Feat { data, count })
    }

    /// `featureNameCount`: how many features the table lists.
    #[must_use]
    pub const fn feature_count(&self) -> u16 {
        self.count
    }

    /// The `n`th feature name record, in table order, or `None` past what
    /// the table holds.
    #[must_use]
    pub fn feature_at(&self, n: u16) -> Option<FeatureName<'a>> {
        if n >= self.count {
            return None;
        }
        let at = NAMES.checked_add(usize::from(n).checked_mul(RECORD)?)?;
        let record = self.data.at(at)?;
        let settings = usize::try_from(record.u32(4)?).ok()?;
        Some(FeatureName {
            feature: record.u16(0)?,
            setting_count: record.u16(2)?,
            flags: record.u16(8)?,
            name_index: record.i16(10)?,
            settings: self.data.at(settings).unwrap_or(Bytes::new(&[])),
        })
    }

    /// The record of feature type `feature`, found by bisecting the array the
    /// chapter keeps sorted by type, as far as the table holds it; `None`
    /// where the table does not list it.
    #[must_use]
    pub fn feature(&self, feature: u16) -> Option<FeatureName<'a>> {
        let held = self.data.len().saturating_sub(NAMES) / RECORD;
        let held = u16::try_from(held).unwrap_or(u16::MAX);
        let (mut low, mut high) = (0u16, self.count.min(held));
        while low < high {
            let middle = low + (high - low) / 2;
            let found = self.feature_at(middle)?;
            match found.feature.cmp(&feature) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => return Some(found),
            }
        }
        None
    }

    /// Whether the table marks feature type `feature`'s settings exclusive;
    /// `None` where it does not list the feature.
    #[must_use]
    pub fn exclusive(&self, feature: u16) -> Option<bool> {
        self.feature(feature).map(|name| name.exclusive())
    }
}

/// One feature of a `feat` table.
#[derive(Clone, Copy, Debug)]
pub struct FeatureName<'a> {
    /// The feature type.
    pub feature: u16,
    /// `nSettings`: how many settings its setting name array holds.
    pub setting_count: u16,
    /// Its `featureFlags`.
    pub flags: u16,
    /// The `name` table index of its name.
    pub name_index: i16,
    /// Its setting name array, to the end of the table.
    settings: Bytes<'a>,
}

impl FeatureName<'_> {
    /// Whether its settings exclude one another.
    #[must_use]
    pub const fn exclusive(&self) -> bool {
        self.flags & EXCLUSIVE != 0
    }

    /// The index of its default setting in its setting name array: the low
    /// byte of its flags where `0x4000` says so, the first otherwise.
    #[must_use]
    pub const fn default_index(&self) -> u16 {
        if self.flags & DEFAULT_GIVEN != 0 {
            self.flags & DEFAULT_INDEX
        } else {
            0
        }
    }

    /// The `n`th setting and its name index, or `None` past `nSettings` or
    /// what the table holds.
    #[must_use]
    pub fn setting(&self, n: u16) -> Option<(u16, i16)> {
        if n >= self.setting_count {
            return None;
        }
        let at = usize::from(n).checked_mul(4)?;
        Some((
            self.settings.u16(at)?,
            self.settings.i16(at.checked_add(2)?)?,
        ))
    }

    /// Its default setting: the setting at [`FeatureName::default_index`].
    #[must_use]
    pub fn default_setting(&self) -> Option<u16> {
        self.setting(self.default_index())
            .map(|(setting, _)| setting)
    }
}

#[cfg(test)]
mod tests {
    use super::Feat;

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn a_version_other_than_one_or_a_cut_header_is_no_table() {
        assert!(Feat::parse(&u16s(&[2, 0, 0, 0, 0, 0])).is_none());
        assert!(Feat::parse(&u16s(&[1, 0])).is_none());
        assert!(Feat::parse(&u16s(&[1, 0, 0, 0, 0, 0])).is_some());
    }

    /// A count past the records the table holds answers nothing past them,
    /// and a feature the array does not list is not found.
    #[test]
    fn a_count_past_the_table_answers_nothing_past_it() {
        let mut table = u16s(&[1, 0, 9, 0, 0, 0]);
        // One feature, type 6, exclusive, its settings at offset 24.
        table.extend(u16s(&[6, 1, 0, 24, 0x8000, 300]));
        table.extend(u16s(&[1, 301]));
        let feat = Feat::parse(&table).expect("readable");
        assert_eq!(feat.feature_count(), 9);
        assert!(feat.feature_at(1).is_none());
        assert_eq!(feat.exclusive(6), Some(true));
        assert_eq!(feat.exclusive(7), None);
        let name = feat.feature(6).expect("listed");
        assert_eq!(name.setting(0), Some((1, 301)));
        assert_eq!(name.setting(1), None, "one setting counted");
        assert_eq!(name.default_setting(), Some(1));
    }
}
