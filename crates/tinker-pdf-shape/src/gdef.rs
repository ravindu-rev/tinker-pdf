//! `GDEF`: what each glyph *is*, which is what makes a lookup flag mean
//! anything.
//!
//! On its own this table substitutes nothing and positions nothing. What it
//! does is answer one question — is this glyph a base, a ligature, a mark or
//! a component — and every lookup flag in `GSUB` and `GPOS` is a filter over
//! that answer. A face whose `GDEF` says nothing is not a face whose lookups
//! do nothing; it is a face whose `IGNORE_MARKS` skips nothing, so an Arabic
//! ligature rule matches across a vowel sign it was written to step over, and
//! the word is set wrong in a way no offset check would catch.
//!
//! # The four other lists, and what they are for here
//!
//! `AttachList` and `LigCaretList` are read and surfaced without being used
//! by anything in this crate, and that is a decision rather than an omission.
//! Attachment points are for a hinting engine choosing where to put a mark on
//! an unhinted base; caret positions are for a text editor placing a cursor
//! inside a ligature. Both are things a *consumer* of this crate does, and
//! `docs/rulings.md` ruling 8 is why they are exposed rather than dropped: a
//! leaf hands back values and lets the caller decide, and a caller that has to
//! reparse `GDEF` to find a ligature caret is a caller that has two parsers.
//!
//! `MarkAttachClassDef` and `MarkGlyphSets` are the other half of the lookup
//! flag and are read by [`crate::apply`] on every skip decision.

use crate::common::{ClassDef, Coverage, Device};
use crate::read::Bytes;

/// What a glyph is, as `GlyphClassDef` numbers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphClass {
    /// The face says nothing about this glyph — either because it has no
    /// `GlyphClassDef` at all or because this glyph is not in it.
    ///
    /// **Not the same as "base".** A lookup with `IGNORE_BASE_GLYPHS` set
    /// skips class 1 and does not skip this, which is the specification's
    /// reading and the one that keeps an unclassified glyph visible to every
    /// rule rather than invisible to some of them.
    #[default]
    Unclassified,
    /// A glyph that stands on the baseline on its own.
    Base,
    /// A glyph a substitution made out of several others.
    Ligature,
    /// A glyph positioned relative to another one.
    Mark,
    /// A piece of a ligature, present in the face so a caret can be placed
    /// inside one.
    Component,
}

impl GlyphClass {
    /// The class a `GlyphClassDef` value names.
    ///
    /// Anything above 4 is reserved and reads as unclassified rather than as
    /// an error: the specification adds classes over time, and a face using
    /// one this crate has not heard of should have its other lookups work.
    #[must_use]
    pub const fn from_value(value: u16) -> Self {
        match value {
            1 => GlyphClass::Base,
            2 => GlyphClass::Ligature,
            3 => GlyphClass::Mark,
            4 => GlyphClass::Component,
            _ => GlyphClass::Unclassified,
        }
    }

    /// The number the table would carry for this class.
    #[must_use]
    pub const fn value(self) -> u16 {
        match self {
            GlyphClass::Unclassified => 0,
            GlyphClass::Base => 1,
            GlyphClass::Ligature => 2,
            GlyphClass::Mark => 3,
            GlyphClass::Component => 4,
        }
    }
}

/// The `GDEF` table.
#[derive(Clone, Copy, Debug)]
pub struct Gdef<'a> {
    data: Bytes<'a>,
    /// Whether the face carries a `GlyphClassDef` at all.
    ///
    /// Kept as a flag rather than rediscovered, because the *absence* of the
    /// table changes what a substitution does: with no classes to consult, a
    /// ligature substitution has to declare its own output a ligature, and
    /// with classes present the table wins even when it disagrees.
    has_classes: bool,
}

impl<'a> Gdef<'a> {
    /// Reads a `GDEF` table.
    ///
    /// `None` for a major version this crate does not read. The three minor
    /// versions — 1.0, 1.2 and 1.3 — differ only by fields appended at the
    /// end, so one reader covers all three: a 1.0 table simply has no bytes
    /// where 1.2's `markGlyphSetsDefOffset` would be, and the bounds check
    /// says so.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let data = Bytes::new(data);
        if data.u16(0)? != 1 {
            return None;
        }
        Some(Self {
            has_classes: data.offset16(4).is_some(),
            data,
        })
    }

    /// The table's minor version.
    #[must_use]
    pub fn minor_version(&self) -> u16 {
        self.data.u16(2).unwrap_or(0)
    }

    /// Whether the face classifies its glyphs at all.
    #[must_use]
    pub const fn has_glyph_classes(&self) -> bool {
        self.has_classes
    }

    /// The `GlyphClassDef`.
    #[must_use]
    pub fn glyph_classes(&self) -> Option<ClassDef<'a>> {
        self.data.offset16(4).map(ClassDef::new)
    }

    /// What the face says `glyph` is.
    #[must_use]
    pub fn glyph_class(&self, glyph: u16) -> GlyphClass {
        self.glyph_classes()
            .map_or(GlyphClass::Unclassified, |classes| {
                GlyphClass::from_value(classes.class_of(glyph))
            })
    }

    /// The `MarkAttachClassDef`: which attachment class each mark is in.
    #[must_use]
    pub fn mark_attach_classes(&self) -> Option<ClassDef<'a>> {
        self.data.offset16(10).map(ClassDef::new)
    }

    /// Which mark attachment class `glyph` is in, zero for none.
    #[must_use]
    pub fn mark_attach_class(&self, glyph: u16) -> u16 {
        self.mark_attach_classes()
            .map_or(0, |classes| classes.class_of(glyph))
    }

    /// The `AttachList`: which outline points a mark may be attached at.
    #[must_use]
    pub fn attachments(&self) -> Option<AttachList<'a>> {
        self.data.offset16(6).map(|data| AttachList { data })
    }

    /// The `LigCaretList`: where a cursor goes inside a ligature.
    #[must_use]
    pub fn ligature_carets(&self) -> Option<LigCaretList<'a>> {
        self.data.offset16(8).map(|data| LigCaretList { data })
    }

    /// How many mark glyph sets the face declares.
    ///
    /// Zero before version 1.2, which is exactly what the bounds check
    /// produces: the field does not exist, `u16` reads past the end of a 1.0
    /// header, and the count is nothing.
    #[must_use]
    pub fn mark_glyph_set_count(&self) -> usize {
        if self.minor_version() < 2 {
            return 0;
        }
        self.mark_glyph_sets()
            .and_then(|sets| sets.u16(2))
            .map_or(0, usize::from)
    }

    /// Mark glyph set `index`, as a coverage table.
    ///
    /// This is the set a lookup with `USE_MARK_FILTERING_SET` names. Note the
    /// offsets inside are 32-bit where every other offset in the table is 16
    /// — the one place in `GDEF` where reading the wrong width silently
    /// halves every offset in the array.
    #[must_use]
    pub fn mark_glyph_set(&self, index: usize) -> Option<Coverage<'a>> {
        if index >= self.mark_glyph_set_count() {
            return None;
        }
        let sets = self.mark_glyph_sets()?;
        if sets.u16(0)? != 1 {
            return None;
        }
        let at = 4usize.checked_add(index.checked_mul(4)?)?;
        sets.offset32(at).map(Coverage::new)
    }

    fn mark_glyph_sets(&self) -> Option<Bytes<'a>> {
        if self.minor_version() < 2 {
            return None;
        }
        self.data.offset16(12)
    }

    /// The `ItemVariationStore`, present from version 1.3.
    ///
    /// Surfaced and not read, for the reason
    /// [`crate::common::FeatureVariations`] gives: variation-aware shaping is
    /// deferred, and a store applied without the rest of the machinery would
    /// move some numbers and not others.
    #[must_use]
    pub fn item_variation_store(&self) -> Option<Bytes<'a>> {
        if self.minor_version() < 3 {
            return None;
        }
        self.data.offset32(14)
    }
}

/// The `AttachList`: per glyph, the outline points a mark attaches at.
#[derive(Clone, Copy, Debug)]
pub struct AttachList<'a> {
    data: Bytes<'a>,
}

impl<'a> AttachList<'a> {
    /// Which glyphs have attachment points.
    #[must_use]
    pub fn coverage(&self) -> Option<Coverage<'a>> {
        self.data.offset16(0).map(Coverage::new)
    }

    /// How many glyphs the list covers.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(2).unwrap_or(0))
    }

    /// Whether it covers none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The outline point indices for `glyph`, in increasing order.
    #[must_use]
    pub fn points(&self, glyph: u16) -> Vec<u16> {
        self.try_points(glyph).unwrap_or_default()
    }

    fn try_points(&self, glyph: u16) -> Option<Vec<u16>> {
        let index = usize::from(self.coverage()?.index_of(glyph)?);
        if index >= self.len() {
            return None;
        }
        let point = self.data.offset16_at(4, index)?;
        let count = usize::from(point.u16(0)?);
        let mut out = Vec::new();
        for at in 0..count {
            out.push(point.u16_at(2, at)?);
        }
        Some(out)
    }
}

/// The `LigCaretList`: where the divisions inside a ligature fall.
#[derive(Clone, Copy, Debug)]
pub struct LigCaretList<'a> {
    data: Bytes<'a>,
}

impl<'a> LigCaretList<'a> {
    /// Which glyphs have caret positions.
    #[must_use]
    pub fn coverage(&self) -> Option<Coverage<'a>> {
        self.data.offset16(0).map(Coverage::new)
    }

    /// How many ligature glyphs the list covers.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.data.u16(2).unwrap_or(0))
    }

    /// Whether it covers none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The caret positions inside `glyph`.
    ///
    /// A ligature of *n* components has *n − 1* carets, and they arrive in
    /// the three forms the table allows: a coordinate in design units, an
    /// outline point index, or a coordinate with a device correction.
    #[must_use]
    pub fn carets(&self, glyph: u16) -> Vec<Caret<'a>> {
        self.try_carets(glyph).unwrap_or_default()
    }

    fn try_carets(&self, glyph: u16) -> Option<Vec<Caret<'a>>> {
        let index = usize::from(self.coverage()?.index_of(glyph)?);
        if index >= self.len() {
            return None;
        }
        let lig = self.data.offset16_at(4, index)?;
        let count = usize::from(lig.u16(0)?);
        let mut out = Vec::new();
        for at in 0..count {
            let Some(value) = lig.offset16_at(2, at) else {
                continue;
            };
            let Some(format) = value.u16(0) else {
                continue;
            };
            let caret = match format {
                1 => value.i16(2).map(|x| Caret::Coordinate {
                    x: i32::from(x),
                    device: None,
                }),
                2 => value.u16(2).map(|point| Caret::Point { point }),
                3 => value.i16(2).map(|x| Caret::Coordinate {
                    x: i32::from(x),
                    device: value.offset16(4).map(Device::new),
                }),
                _ => None,
            };
            if let Some(caret) = caret {
                out.push(caret);
            }
        }
        Some(out)
    }
}

/// One division inside a ligature.
#[derive(Clone, Copy, Debug)]
pub enum Caret<'a> {
    /// A position in font design units, with an optional per-size correction.
    Coordinate {
        /// The horizontal position, in design units.
        x: i32,
        /// The device table that corrects it, if the face hinted one.
        device: Option<Device<'a>>,
    },
    /// An outline point index, which a consumer resolves against the glyph's
    /// own outline: a caret that follows the shape when the outline is
    /// hinted, rather than one fixed in the design.
    Point {
        /// The index of the point, in the glyph's outline.
        point: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::{Gdef, GlyphClass};

    /// A `GDEF` 1.0 whose `GlyphClassDef` calls glyph 3 a mark and glyph 4 a
    /// ligature, and whose other three offsets are zero.
    fn gdef() -> Vec<u8> {
        let mut table = vec![
            0x00, 0x01, 0x00, 0x00, // version 1.0
            0x00, 0x0C, // glyphClassDef at 12
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // the other three: absent
        ];
        // ClassDef format 2: glyph 3 -> 3, glyph 4 -> 2.
        table.extend_from_slice(&[
            0x00, 0x02, 0x00, 0x02, 0x00, 0x03, 0x00, 0x03, 0x00, 0x03, 0x00, 0x04, 0x00, 0x04,
            0x00, 0x02,
        ]);
        table
    }

    #[test]
    fn classes_come_back_as_the_enum() {
        let bytes = gdef();
        let gdef = Gdef::parse(&bytes).expect("a version 1 table");
        assert!(gdef.has_glyph_classes());
        assert_eq!(gdef.glyph_class(3), GlyphClass::Mark);
        assert_eq!(gdef.glyph_class(4), GlyphClass::Ligature);
        assert_eq!(gdef.glyph_class(5), GlyphClass::Unclassified);
        assert_eq!(gdef.mark_attach_class(3), 0);
        assert!(gdef.attachments().is_none());
        assert!(gdef.ligature_carets().is_none());
        assert_eq!(gdef.mark_glyph_set_count(), 0);
    }

    #[test]
    fn a_reserved_class_is_unclassified_rather_than_an_error() {
        assert_eq!(GlyphClass::from_value(9), GlyphClass::Unclassified);
        assert_eq!(GlyphClass::Component.value(), 4);
    }

    #[test]
    fn truncation_anywhere_answers_rather_than_panicking() {
        let bytes = gdef();
        for cut in 0..bytes.len() {
            let Some(gdef) = Gdef::parse(bytes.get(..cut).unwrap_or(&bytes)) else {
                continue;
            };
            for glyph in 0..8u16 {
                let _ = gdef.glyph_class(glyph);
                let _ = gdef.mark_attach_class(glyph);
            }
            let _ = gdef.mark_glyph_set(0);
            let _ = gdef.item_variation_store();
        }
    }
}
