//! Features and languages: `Feat`, `Sill`, and the values a run's rules read.
//!
//! A Graphite feature is a switch the face defines and its rules test: an id
//! — four characters, or a number where the top byte is 0 — and the values
//! it may take, one of them its default. `Sill` gives each language the face
//! lists its own settings. What a run's rules read is one value a feature,
//! made from the defaults, the language's settings and the caller's, in that
//! order (`docs/design/shaping.md`, *Features*, *Languages* and *Feature
//! values, from the shaper's settings*).
//!
//! # The versions read
//!
//! `Feat` 1 is the TrueType `feat` table's layout, which the GTF names as
//! `Feat`'s basis: twelve bytes a feature, a 16-bit feature type first.
//! `Feat` 2 widens the id to 32 bits and inserts a reserved `USHORT`,
//! sixteen bytes a feature. Both begin with a header of their version, the
//! feature count and six reserved bytes, and both point each feature at its
//! settings by an offset from the table's start: a `SHORT` value and a name
//! index a setting. `Sill` 1 is its header, one entry a language and one
//! more, and the settings they point at.

use crate::common::Tag;
use crate::read::Bytes;

use super::{Refusal, FEAT, SILL};

/// One feature of a `Feat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feature {
    /// Its id: four characters, or a number where the top byte is 0. A
    /// `Feat` 1 id is 16 bits, widened.
    pub id: u32,
    /// How many settings it declares.
    pub settings: u16,
    /// Its flags. Bit 0 marks an alias of the feature before it, the GDL's
    /// hidden id; 0x8000 and 0x4000 are the TrueType chapter's exclusive and
    /// default-index bits.
    pub flags: u16,
    /// The `name` table index of its label.
    pub label: u16,
    /// Where its settings start, from the table's start.
    offset: u32,
}

impl Feature {
    /// Whether the feature is an alias of the one before it.
    #[must_use]
    pub const fn is_alias(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// One setting of a [`Feature`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setting {
    /// The value it sets.
    pub value: i16,
    /// The `name` table index of its label.
    pub label: u16,
}

/// A face's `Feat`.
#[derive(Clone, Copy, Debug)]
pub struct Features<'a> {
    data: Bytes<'a>,
    wide: bool,
    count: u16,
}

impl<'a> Features<'a> {
    /// Reads a `Feat`.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownVersion`] for a major version other than 1 and 2,
    /// and [`Refusal::TableMalformed`] for a table too short for its header
    /// or for the features its count states.
    pub fn parse(feat: &'a [u8]) -> Result<Self, Refusal> {
        let data = Bytes::new(feat);
        let malformed = Refusal::TableMalformed { table: FEAT };
        let version = data.u32(0).ok_or(malformed)?;
        let wide = match version >> 16 {
            1 => false,
            2 => true,
            _ => {
                return Err(Refusal::UnknownVersion {
                    table: FEAT,
                    version,
                })
            }
        };
        let count = data.u16(4).ok_or(malformed)?;
        let size = if wide { 16 } else { 12 };
        let end = 12usize
            .checked_add(usize::from(count) * size)
            .ok_or(malformed)?;
        if end > data.len() {
            return Err(malformed);
        }
        Ok(Self { data, wide, count })
    }

    /// How many features the table declares.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// Whether it declares none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Feature number `index`, in the table's order — the order `PushFeat`
    /// numbers features in.
    #[must_use]
    pub fn feature(&self, index: usize) -> Option<Feature> {
        if index >= self.len() {
            return None;
        }
        if self.wide {
            let at = 12 + index * 16;
            Some(Feature {
                id: self.data.u32(at)?,
                settings: self.data.u16(at + 4)?,
                offset: self.data.u32(at + 8)?,
                flags: self.data.u16(at + 12)?,
                label: self.data.u16(at + 14)?,
            })
        } else {
            let at = 12 + index * 12;
            Some(Feature {
                id: u32::from(self.data.u16(at)?),
                settings: self.data.u16(at + 2)?,
                offset: self.data.u32(at + 4)?,
                flags: self.data.u16(at + 8)?,
                label: self.data.u16(at + 10)?,
            })
        }
    }

    /// The index of the first feature whose id is `id`.
    #[must_use]
    pub fn index_of(&self, id: u32) -> Option<usize> {
        (0..self.len()).find(|index| self.feature(*index).is_some_and(|f| f.id == id))
    }

    /// Setting `n` of feature `index`, where the table holds it.
    #[must_use]
    pub fn setting(&self, index: usize, n: usize) -> Option<Setting> {
        let feature = self.feature(index)?;
        if n >= usize::from(feature.settings) {
            return None;
        }
        let at = usize::try_from(feature.offset)
            .ok()?
            .checked_add(n.checked_mul(4)?)?;
        Some(Setting {
            value: self.data.i16(at)?,
            label: self.data.u16(at + 2)?,
        })
    }

    /// Feature `index`'s settings, as far as the table holds them.
    #[must_use]
    pub fn settings(&self, index: usize) -> Vec<Setting> {
        let count = self.feature(index).map_or(0, |f| usize::from(f.settings));
        (0..count).map_while(|n| self.setting(index, n)).collect()
    }

    /// Feature `index`'s default value: its setting at index 0, or, where
    /// its flags set both 0x8000 and 0x4000, the setting their low byte
    /// names — the TrueType `feat` chapter's rule — and 0 for a feature
    /// with no settings, the GDL's boolean, or a setting the table does not
    /// hold.
    #[must_use]
    pub fn default_value(&self, index: usize) -> i16 {
        let Some(feature) = self.feature(index) else {
            return 0;
        };
        let at = if feature.flags & 0xC000 == 0xC000 {
            usize::from(feature.flags & 0x00FF)
        } else {
            0
        };
        self.setting(index, at).map_or(0, |s| s.value)
    }

    /// Whether feature `index` declares `value`: one of its settings sets
    /// it, or it has no settings and `value` is the GDL boolean's 0 or 1.
    #[must_use]
    pub fn declares(&self, index: usize, value: i16) -> bool {
        let Some(feature) = self.feature(index) else {
            return false;
        };
        if feature.settings == 0 {
            return value == 0 || value == 1;
        }
        self.settings(index).iter().any(|s| s.value == value)
    }

    /// The largest value feature `index` declares, which `SetFeat` clips to:
    /// its settings' greatest, or 1 for a feature with none.
    #[must_use]
    pub fn max_value(&self, index: usize) -> i16 {
        let settings = self.settings(index);
        settings.iter().map(|s| s.value).max().unwrap_or(1)
    }
}

/// A face's `Sill`.
#[derive(Clone, Copy, Debug)]
pub struct Languages<'a> {
    data: Bytes<'a>,
    count: u16,
}

impl<'a> Languages<'a> {
    /// Reads a `Sill`.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownVersion`] for a major version other than 1, and
    /// [`Refusal::TableMalformed`] for a table too short for its header or
    /// for the entries its count states — the GTF's one more included.
    pub fn parse(sill: &'a [u8]) -> Result<Self, Refusal> {
        let data = Bytes::new(sill);
        let malformed = Refusal::TableMalformed { table: SILL };
        let version = data.u32(0).ok_or(malformed)?;
        if version >> 16 != 1 {
            return Err(Refusal::UnknownVersion {
                table: SILL,
                version,
            });
        }
        let count = data.u16(4).ok_or(malformed)?;
        let end = 12usize
            .checked_add((usize::from(count) + 1) * 8)
            .ok_or(malformed)?;
        if end > data.len() {
            return Err(malformed);
        }
        Ok(Self { data, count })
    }

    /// How many languages the table lists, its last bogus entry not counted.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// Whether it lists none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Language `index`'s code, four bytes padded with NULs.
    #[must_use]
    pub fn code(&self, index: usize) -> Option<[u8; 4]> {
        if index >= self.len() {
            return None;
        }
        let at = 12 + index * 8;
        Some([
            self.data.u8(at)?,
            self.data.u8(at + 1)?,
            self.data.u8(at + 2)?,
            self.data.u8(at + 3)?,
        ])
    }

    /// The first language whose code is `code`.
    #[must_use]
    pub fn find(&self, code: [u8; 4]) -> Option<usize> {
        (0..self.len()).find(|index| self.code(*index) == Some(code))
    }

    /// Language `index`'s settings, each a feature id and a value, as far as
    /// the table holds them.
    #[must_use]
    pub fn settings(&self, index: usize) -> Vec<(u32, i16)> {
        if index >= self.len() {
            return Vec::new();
        }
        let at = 12 + index * 8;
        let (Some(count), Some(offset)) = (self.data.u16(at + 4), self.data.u16(at + 6)) else {
            return Vec::new();
        };
        (0..usize::from(count))
            .map_while(|n| {
                let at = usize::from(offset) + n * 8;
                Some((self.data.u32(at)?, self.data.i16(at + 4)?))
            })
            .collect()
    }

    /// The code `Shaper::with_language`'s tag asks `Sill` for: the tag's
    /// letters lowercased and its trailing spaces read as NULs, so `urd ` and
    /// `URD ` both ask for `urd`, and `en  ` for the GTF's bytes 101, 110, 0
    /// and 0.
    #[must_use]
    pub fn code_for(tag: Tag) -> [u8; 4] {
        let mut code = tag.to_bytes();
        let mut trailing = true;
        for byte in code.iter_mut().rev() {
            if trailing && (*byte == b' ' || *byte == 0) {
                *byte = 0;
            } else {
                trailing = false;
                *byte = byte.to_ascii_lowercase();
            }
        }
        code
    }
}

/// The value of each of a face's features for a run: one `i16` a feature,
/// in `Feat`'s order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureValues {
    values: Vec<i16>,
}

impl FeatureValues {
    /// Every feature at its default.
    #[must_use]
    pub fn defaults(features: &Features<'_>) -> FeatureValues {
        FeatureValues {
            values: (0..features.len())
                .map(|index| features.default_value(index))
                .collect(),
        }
    }

    /// The values a run takes, each over the last: the defaults; the
    /// language's `Sill` settings, where `language`'s code is one `Sill`
    /// lists; each feature whose id a tag of `turned_on` spells, set to 1;
    /// and `settings`, each tag the id it spells, the last setting of a tag
    /// winning.
    ///
    /// A tag or a `Sill` setting naming no feature the face declares, and a
    /// value of the caller's the feature does not declare, is ignored and
    /// named, each once. A `Sill` value is the face's own statement and is
    /// taken as written.
    #[must_use]
    pub fn resolve(
        features: &Features<'_>,
        languages: Option<&Languages<'_>>,
        language: Option<Tag>,
        turned_on: &[Tag],
        settings: &[(Tag, u32)],
    ) -> (FeatureValues, Vec<Refusal>) {
        let mut values = FeatureValues::defaults(features);
        let mut refusals: Vec<Refusal> = Vec::new();
        let refuse = |refusal: Refusal, refusals: &mut Vec<Refusal>| {
            if !refusals.contains(&refusal) {
                refusals.push(refusal);
            }
        };

        if let (Some(languages), Some(language)) = (languages, language) {
            if let Some(index) = languages.find(Languages::code_for(language)) {
                for (id, value) in languages.settings(index) {
                    match features.index_of(id) {
                        Some(feature) => values.set(feature, value),
                        None => refuse(Refusal::UndeclaredFeature { feature: id }, &mut refusals),
                    }
                }
            }
        }

        let mut caller = |tag: Tag, value: u32, refusals: &mut Vec<Refusal>| {
            let id = tag.0;
            let Some(feature) = features.index_of(id) else {
                refuse(Refusal::UndeclaredFeature { feature: id }, refusals);
                return;
            };
            match i16::try_from(value) {
                Ok(value) if features.declares(feature, value) => values.set(feature, value),
                _ => refuse(Refusal::UndeclaredSetting { feature: id, value }, refusals),
            }
        };
        for tag in turned_on {
            caller(*tag, 1, &mut refusals);
        }
        // The last setting of each tag, in the order the tags first appear.
        let mut seen: Vec<Tag> = Vec::new();
        for (tag, _) in settings {
            if seen.contains(tag) {
                continue;
            }
            seen.push(*tag);
            let last = settings
                .iter()
                .rev()
                .find(|(t, _)| t == tag)
                .map_or(0, |(_, value)| *value);
            caller(*tag, last, &mut refusals);
        }
        (values, refusals)
    }

    /// Feature `index`'s value.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<i16> {
        self.values.get(index).copied()
    }

    /// The value of the feature whose id is `id`.
    #[must_use]
    pub fn by_id(&self, features: &Features<'_>, id: u32) -> Option<i16> {
        self.get(features.index_of(id)?)
    }

    /// Every value, in `Feat`'s order.
    #[must_use]
    pub fn as_slice(&self) -> &[i16] {
        &self.values
    }

    /// How many features there are values for.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Feature `index` set to `value`.
    fn set(&mut self, index: usize, value: i16) {
        if let Some(slot) = self.values.get_mut(index) {
            *slot = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureValues, Features, Languages, Setting};
    use crate::common::Tag;
    use crate::graphite::{Refusal, FEAT, SILL};

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A `Feat` 1, the TrueType layout, of one feature of two settings.
    fn feat_one() -> Vec<u8> {
        let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
        out.extend(u16s(&[1, 0, 0, 0]));
        // id 7, two settings at 24, flags 0x8000, label 256.
        out.extend(u16s(&[7, 2]));
        out.extend(24u32.to_be_bytes());
        out.extend(u16s(&[0x8000, 256]));
        out.extend(u16s(&[5, 257, 6, 258]));
        out
    }

    #[test]
    fn a_feat_of_version_one_is_the_truetype_layout() {
        let feat = feat_one();
        let features = Features::parse(&feat).expect("readable");
        assert_eq!(features.len(), 1);
        let feature = features.feature(0).expect("a feature");
        assert_eq!((feature.id, feature.settings, feature.label), (7, 2, 256));
        assert!(!feature.is_alias());
        assert_eq!(
            features.settings(0),
            vec![
                Setting {
                    value: 5,
                    label: 257
                },
                Setting {
                    value: 6,
                    label: 258
                }
            ]
        );
        assert_eq!(features.default_value(0), 5);
        assert!(features.declares(0, 6) && !features.declares(0, 7));
        assert_eq!(features.max_value(0), 6);
        assert_eq!(features.index_of(7), Some(0));
        assert_eq!(features.index_of(8), None);
    }

    #[test]
    fn a_feat_it_cannot_read_is_refused() {
        let mut feat = feat_one();
        feat[1] = 3;
        assert_eq!(
            Features::parse(&feat).map(|_| ()),
            Err(Refusal::UnknownVersion {
                table: FEAT,
                version: 0x0003_0000
            })
        );
        // A count of features past the table's end.
        let mut feat = feat_one();
        feat[5] = 3;
        assert_eq!(
            Features::parse(&feat).map(|_| ()),
            Err(Refusal::TableMalformed { table: FEAT })
        );
        assert_eq!(
            Languages::parse(&[0, 2, 0, 0, 0, 0]).map(|_| ()),
            Err(Refusal::UnknownVersion {
                table: SILL,
                version: 0x0002_0000
            })
        );
        assert_eq!(
            Languages::parse(&[0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]).map(|_| ()),
            Err(Refusal::TableMalformed { table: SILL })
        );
    }

    #[test]
    fn settings_past_the_table_are_not_read() {
        // The feature claims three settings; the table holds two.
        let mut feat = feat_one();
        feat[15] = 3;
        let features = Features::parse(&feat).expect("readable");
        assert_eq!(features.settings(0).len(), 2);
        assert_eq!(features.setting(0, 2), None);
    }

    #[test]
    fn a_tag_asks_for_its_code_lowercased_and_padded_with_nuls() {
        assert_eq!(Languages::code_for(Tag::new(b"en  ")), [101, 110, 0, 0]);
        assert_eq!(Languages::code_for(Tag::new(b"URD ")), *b"urd\0");
        assert_eq!(Languages::code_for(Tag::new(b"urd\0")), *b"urd\0");
        assert_eq!(Languages::code_for(Tag::new(b"KSH ")), *b"ksh\0");
        // A space inside the code is kept: only trailing ones are padding.
        assert_eq!(Languages::code_for(Tag::new(b"a b ")), *b"a b\0");
    }

    #[test]
    fn caller_settings_repeat_and_the_last_wins() {
        let feat = feat_one();
        let features = Features::parse(&feat).expect("readable");
        let tag = Tag(7);
        let (values, refusals) =
            FeatureValues::resolve(&features, None, None, &[], &[(tag, 6), (tag, 5), (tag, 6)]);
        assert_eq!(values.as_slice(), &[6]);
        assert!(refusals.is_empty());
        // An undeclared value repeated is named once.
        let (values, refusals) = FeatureValues::resolve(&features, None, None, &[tag], &[(tag, 9)]);
        assert_eq!(values.as_slice(), &[5]);
        assert_eq!(
            refusals,
            vec![
                Refusal::UndeclaredSetting {
                    feature: 7,
                    value: 1
                },
                Refusal::UndeclaredSetting {
                    feature: 7,
                    value: 9
                }
            ]
        );
    }
}
