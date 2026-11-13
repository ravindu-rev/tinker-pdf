//! Graphite: a face whose layout is a Graphite description.
//!
//! `docs/design/shaping.md`, *Graphite shaping*, is the reading of the
//! Graphite Table Format (the GTF) this follows, and `THIRDPARTY.md` names
//! every document read. The tables are SIL's: `Silf` for the rules, `Glat`
//! and `Gloc` for the glyph attributes the rules read, `Feat` for the
//! features a caller sets and `Sill` for each language's settings.
//!
//! What is here so far is the reading of the tables — a face's view
//! ([`Tables`]), with its compressed tables decoded once ([`decompress`]);
//! its glyph attributes ([`Attributes`]); and its features and languages
//! ([`Features`], [`Languages`]), and the value each feature takes for a run
//! ([`FeatureValues`], which `Shaper::graphite_features` makes from a
//! shaper's settings, features and language); its rules ([`Silf`]): each
//! description's header, pseudo-glyph map and class map, and each pass's
//! machine finding the rules that match at a scan position ([`Pass`]); and
//! the stack machine their code runs on ([`machine`]): every block checked
//! before it runs, and a constraint's computing instructions run against a
//! stream ([`Reads`]) to decide whether its rule fires; and the slot stream
//! ([`Slots`]) a rule's action rewrites, putting glyphs, inserting,
//! deleting, copying and associating slots and setting their attributes. A face whose only
//! layout tables are these is still shaped as though it carried none, and
//! every run over it says so (`crate::Warning::TableNotRun`).
//!
//! # Every count is a bound, and every bound refuses by name
//!
//! A description is read from a file this engine did not write: offsets point
//! anywhere and counts lie. Every read goes through [`crate::read::Bytes`],
//! every size a table states is held to a ceiling in [`crate::Limits`] before
//! anything is spent on it, and what is refused is a [`Refusal`] naming the
//! table, glyph or number it touched (rulings 1 and 10).

use std::borrow::Cow;

use tinker_pdf_font::Sfnt;

use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;

pub mod feat;
pub mod glat;
mod lz4;
pub mod machine;
pub mod silf;
pub mod slot;

pub use feat::{Feature, FeatureValues, Features, Languages, Setting};
pub use glat::{Attributes, GlatVersion, Octabox, Subbox};
pub use machine::{
    Block, Bounds, Budget, Fault, Frame, Metric, Op, Program, Reads, Site, SlotAttribute,
};
pub use silf::{
    Candidate, ClassKind, ClassMap, Description, JustificationLevel, Matched, Pass, Silf,
};
pub use slot::{Acted, Slot, Slots, Source};

/// `Silf`, a Graphite description's rules.
pub const SILF: Tag = Tag::new(b"Silf");
/// `Glat`, its glyph attributes.
pub const GLAT: Tag = Tag::new(b"Glat");
/// `Gloc`, where each glyph's attributes start in `Glat`.
pub const GLOC: Tag = Tag::new(b"Gloc");
/// `Feat`, its features.
pub const FEAT: Tag = Tag::new(b"Feat");
/// `Sill`, its languages' feature settings.
pub const SILL: Tag = Tag::new(b"Sill");

/// Something a Graphite table asked for that this crate refused to do.
///
/// Each names what it touched — the table, the glyph — because "a Graphite
/// table was malformed" is not a sentence anybody can act on (ruling 10).
/// Copyable and comparable, so a list of them is kept free of repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A table's version is one the design does not read
    /// (`docs/design/shaping.md`, *Graphite scope*).
    UnknownVersion {
        /// The table.
        table: Tag,
        /// The version it states, as its 32-bit field.
        version: u32,
    },
    /// A table is compressed by a scheme other than the GTF's 0 and 1.
    UnknownCompression {
        /// The table.
        table: Tag,
        /// The scheme its header's top five bits name.
        scheme: u8,
    },
    /// A compressed table states a decoded size past
    /// [`Limits::max_table_bytes`]; nothing of it was decoded.
    TableTooLarge {
        /// The table.
        table: Tag,
        /// The size it states, in bytes.
        size: u32,
    },
    /// A compressed table's block is one the LZ4 Block Format calls
    /// corrupted, ends inside a sequence, or decodes to a size other than
    /// the one it states, or what it decodes to is not the table again —
    /// its version and a scheme of 0.
    CompressionMalformed {
        /// The table.
        table: Tag,
    },
    /// A table too short to hold its own header, or whose header contradicts
    /// itself.
    TableMalformed {
        /// The table.
        table: Tag,
    },
    /// A glyph's attributes cannot be read: the glyph is past `Gloc`'s
    /// offsets, its block ends before it starts or past `Glat`'s end, or a
    /// run reaches past the block.
    GlyphUnreadable {
        /// The glyph.
        glyph: u16,
    },
    /// A glyph's block holds more than [`Limits::max_attribute_runs`] runs.
    TooManyAttributeRuns {
        /// The glyph.
        glyph: u16,
    },
    /// A caller's setting, a caller's turned-on feature or a language's
    /// `Sill` setting names a feature the face's `Feat` does not declare; it
    /// was ignored.
    UndeclaredFeature {
        /// The id named: four characters, or a number.
        feature: u32,
    },
    /// A caller's value for a feature is not one of the values the feature
    /// declares; it was ignored, and the feature kept the value it had.
    UndeclaredSetting {
        /// The feature's id.
        feature: u32,
        /// The value asked for.
        value: u32,
    },
    /// A description states more passes than [`Limits::max_passes`]; the
    /// passes past the ceiling do not run.
    TooManyPasses {
        /// The passes it states.
        passes: u8,
    },
    /// A pass that cannot be read whole: its block past the description, its
    /// arrays past its block, a range naming a column it has not, or counts
    /// the GTF's order of states cannot hold. It does not run.
    PassMalformed {
        /// The pass.
        pass: u8,
    },
    /// A pass's machine took, or would take, a transition its table cannot
    /// give — to a state past its states, or from a row past its table. The
    /// walk ended there, and the rules it had found stand.
    TransitionPastTable {
        /// The pass.
        pass: u8,
        /// The state the transition was taken from.
        state: u16,
    },
    /// A pass's rule map names a rule past the pass's rule count; it was not
    /// a candidate.
    RuleOutOfRange {
        /// The pass.
        pass: u8,
        /// The rule named.
        rule: u16,
    },
    /// A pass's machine would read more slots than
    /// [`Limits::max_context_length`] at one position; the walk ended there.
    RuleContextTooLong {
        /// The pass.
        pass: u8,
    },
    /// A position of a pass gathered more rules than
    /// [`Limits::max_rule_candidates`]; the rest were not tried.
    TooManyCandidates {
        /// The pass.
        pass: u8,
    },
    /// A block of a pass's code refused: by its check, so that it never
    /// runs and its rule never fires, or where it ran and could not go on —
    /// a division the machine cannot do, a slot offset outside the stream —
    /// so that it ended there (`machine::check`, `machine::Fault`).
    CodeRefused {
        /// The pass.
        pass: u8,
        /// Which of its blocks.
        block: machine::Block,
        /// The byte offset of the instruction refused in the block.
        at: u16,
        /// Why.
        fault: machine::Fault,
    },
    /// The run spent [`Limits::max_instructions`]; the rest of its code did
    /// not run.
    TooManyInstructions,
    /// Code read a slot's position, or a glyph metric over an attachment
    /// cluster, which positioning resolves; the code ended there.
    PositionNotResolved,
    /// Code read a glyph's box, and the face gives none: no `glyf`, or a
    /// glyph past `loca`.
    GlyphBoundsUnavailable {
        /// The glyph, the real one a pseudo-glyph stands for.
        glyph: u16,
    },
}

/// The table `table` as it reads uncompressed: the bytes themselves where it
/// is stored plain, and the bytes its LZ4 block decodes to where it is
/// compressed.
///
/// Only `Silf` from version 5 and `Glat` from version 3 can be compressed;
/// any other table, and either of these at an earlier version, is its bytes
/// as they stand. A compressed table is its version, a word whose top five
/// bits are the scheme and whose low twenty-seven are the decoded size, and
/// the scheme's data; scheme 0 is the table as it stands. Scheme 1's block
/// must decode to exactly the size stated, at most
/// [`Limits::max_table_bytes`], and to the table again: the same version,
/// and a scheme of 0 in the same word.
///
/// # Errors
///
/// [`Refusal::TableMalformed`] for a table too short for the header its
/// version gives it; [`Refusal::UnknownCompression`] for a scheme other than
/// 0 and 1; [`Refusal::TableTooLarge`] for a stated size past the ceiling;
/// and [`Refusal::CompressionMalformed`] for a block that does not decode to
/// the table.
pub fn decompress<'a>(
    table: Tag,
    bytes: &'a [u8],
    limits: &Limits,
) -> Result<Cow<'a, [u8]>, Refusal> {
    let data = Bytes::new(bytes);
    let version = data.u32(0).ok_or(Refusal::TableMalformed { table })?;
    let compressible =
        (table == SILF && version >> 16 >= 5) || (table == GLAT && version >> 16 >= 3);
    if !compressible {
        return Ok(Cow::Borrowed(bytes));
    }
    let word = data.u32(4).ok_or(Refusal::TableMalformed { table })?;
    let scheme = scheme_of(word);
    match scheme {
        0 => Ok(Cow::Borrowed(bytes)),
        1 => {
            let size = word & 0x07FF_FFFF;
            let wanted = usize::try_from(size).unwrap_or(usize::MAX);
            if wanted > limits.max_table_bytes {
                return Err(Refusal::TableTooLarge { table, size });
            }
            let block = bytes.get(8..).ok_or(Refusal::TableMalformed { table })?;
            let decoded =
                lz4::decode(block, wanted).ok_or(Refusal::CompressionMalformed { table })?;
            // "The uncompressed form is the complete table including the
            // version number but with the scheme always set to 0."
            let again = Bytes::new(&decoded);
            if again.u32(0) != Some(version) || again.u32(4).map(scheme_of) != Some(0) {
                return Err(Refusal::CompressionMalformed { table });
            }
            Ok(Cow::Owned(decoded))
        }
        other => Err(Refusal::UnknownCompression {
            table,
            scheme: other,
        }),
    }
}

/// A compression header word's scheme: its top five bits.
fn scheme_of(word: u32) -> u8 {
    // Five bits: at most 31, inside `u8` by construction.
    u8::try_from(word >> 27).unwrap_or(u8::MAX)
}

/// A face's Graphite tables, each decoded once where it is compressed.
///
/// Made from a face that carries a `Silf`; a face without one has no
/// description, whatever else it carries. Each table is held as it reads
/// uncompressed ([`decompress`]); one that could not be decoded is held as
/// absent, and its refusal kept ([`Tables::refusals`]), so that a face whose
/// `Glat` is broken still has a `Feat` to answer.
#[derive(Clone, Debug)]
pub struct Tables<'a> {
    silf: Cow<'a, [u8]>,
    glat: Option<Cow<'a, [u8]>>,
    gloc: Option<&'a [u8]>,
    feat: Option<&'a [u8]>,
    sill: Option<&'a [u8]>,
    refusals: Vec<Refusal>,
}

impl<'a> Tables<'a> {
    /// The Graphite tables of `face`, or `None` where it carries no `Silf`.
    #[must_use]
    pub fn parse(face: &Sfnt<'a>, limits: &Limits) -> Option<Tables<'a>> {
        let table = |tag: Tag| face.table(tag.0);
        Tables::from_tables(
            table(SILF),
            table(GLAT),
            table(GLOC),
            table(FEAT),
            table(SILL),
            limits,
        )
    }

    /// The same, from table bytes a caller already has: a test that writes
    /// one table by hand, or the fuzz target, which drives each table without
    /// a directory in front of it.
    #[must_use]
    pub fn from_tables(
        silf: Option<&'a [u8]>,
        glat: Option<&'a [u8]>,
        gloc: Option<&'a [u8]>,
        feat: Option<&'a [u8]>,
        sill: Option<&'a [u8]>,
        limits: &Limits,
    ) -> Option<Tables<'a>> {
        let mut refusals = Vec::new();
        let mut decoded = |table: Tag, bytes: Option<&'a [u8]>| -> Option<Cow<'a, [u8]>> {
            match decompress(table, bytes?, limits) {
                Ok(bytes) => Some(bytes),
                Err(refusal) => {
                    refusals.push(refusal);
                    None
                }
            }
        };
        let silf = decoded(SILF, silf);
        let glat = decoded(GLAT, glat);
        // A `Silf` that could not be decoded still makes the face a Graphite
        // description: it is held empty, and its refusal says why.
        let silf = match (silf, refusals.iter().any(is_silf)) {
            (Some(silf), _) => silf,
            (None, true) => Cow::Borrowed(&[][..]),
            (None, false) => return None,
        };
        Some(Tables {
            silf,
            glat,
            gloc,
            feat,
            sill,
            refusals,
        })
    }

    /// `Silf` as it reads uncompressed; empty where it could not be decoded.
    #[must_use]
    pub fn silf(&self) -> &[u8] {
        &self.silf
    }

    /// `Glat` as it reads uncompressed, where the face carries one that could
    /// be decoded.
    #[must_use]
    pub fn glat(&self) -> Option<&[u8]> {
        self.glat.as_deref()
    }

    /// `Gloc`, where the face carries one.
    #[must_use]
    pub const fn gloc(&self) -> Option<&'a [u8]> {
        self.gloc
    }

    /// `Feat`, where the face carries one.
    #[must_use]
    pub const fn feat(&self) -> Option<&'a [u8]> {
        self.feat
    }

    /// `Sill`, where the face carries one.
    #[must_use]
    pub const fn sill(&self) -> Option<&'a [u8]> {
        self.sill
    }

    /// The refusals decoding the tables met, each once, in the order met.
    #[must_use]
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }

    /// The face's features, read from its `Feat`; `None` where it carries
    /// none.
    ///
    /// # Errors
    ///
    /// The refusal [`Features::parse`] gives for a table it cannot read.
    pub fn features(&self) -> Option<Result<Features<'a>, Refusal>> {
        self.feat.map(Features::parse)
    }

    /// The face's languages, read from its `Sill`; `None` where it carries
    /// none.
    ///
    /// # Errors
    ///
    /// The refusal [`Languages::parse`] gives for a table it cannot read.
    pub fn languages(&self) -> Option<Result<Languages<'a>, Refusal>> {
        self.sill.map(Languages::parse)
    }

    /// The face's glyph attributes, read from its `Gloc` and `Glat`.
    ///
    /// `None` where the face carries neither or one could not be decoded.
    ///
    /// # Errors
    ///
    /// The refusal [`Attributes::parse`] gives for tables it cannot read.
    pub fn attributes(&self, limits: &Limits) -> Option<Result<Attributes<'_>, Refusal>> {
        let gloc = self.gloc?;
        let glat = self.glat()?;
        Some(Attributes::parse(gloc, glat, limits))
    }
}

/// Whether `refusal` names `Silf`.
fn is_silf(refusal: &Refusal) -> bool {
    matches!(
        refusal,
        Refusal::UnknownCompression { table, .. }
            | Refusal::TableTooLarge { table, .. }
            | Refusal::CompressionMalformed { table }
            | Refusal::TableMalformed { table }
            | Refusal::UnknownVersion { table, .. }
            if *table == SILF
    )
}

#[cfg(test)]
mod tests {
    use super::{decompress, Refusal, Tables, GLAT, GLOC, SILF};
    use crate::limits::Limits;
    use std::borrow::Cow;

    /// A table compressed by scheme 1: its version, the scheme and size
    /// word, and `block`.
    fn compressed(version: u32, size: u32, block: &[u8]) -> Vec<u8> {
        let mut out = version.to_be_bytes().to_vec();
        out.extend(((1u32 << 27) | size).to_be_bytes());
        out.extend_from_slice(block);
        out
    }

    /// An LZ4 block of literals alone: the token's count and the bytes.
    fn literals(bytes: &[u8]) -> Vec<u8> {
        assert!(bytes.len() < 15, "a short block");
        let mut out = vec![u8::try_from(bytes.len()).expect("short") << 4];
        out.extend_from_slice(bytes);
        out
    }

    /// The table a test decodes: `Glat` 3, its word scheme 0 and octaboxes
    /// clear, and four bytes of entries.
    const PLAIN_GLAT: [u8; 12] = [0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2];

    #[test]
    fn a_plain_or_early_table_is_its_bytes() {
        let limits = Limits::DEFAULT;
        // Before version 3 a `Glat` has no compression word to read.
        let early = [0, 1, 0, 0, 7, 9];
        assert!(matches!(
            decompress(GLAT, &early, &limits),
            Ok(Cow::Borrowed(b)) if b == early
        ));
        // A version-3 table of scheme 0 is as it stands.
        assert!(matches!(
            decompress(GLAT, &PLAIN_GLAT, &limits),
            Ok(Cow::Borrowed(b)) if b == PLAIN_GLAT
        ));
        // `Gloc` is never compressed, whatever its second word says.
        let gloc = [0, 1, 0, 0, 0x08, 0, 0, 0];
        assert!(matches!(
            decompress(GLOC, &gloc, &limits),
            Ok(Cow::Borrowed(_))
        ));
        // A `Silf` before version 5 likewise.
        let silf = [0, 4, 0, 0, 0x08, 0, 0, 0];
        assert!(matches!(
            decompress(SILF, &silf, &limits),
            Ok(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn a_compressed_table_decodes_to_the_table_again() {
        let limits = Limits::DEFAULT;
        let table = compressed(0x0003_0000, 12, &literals(&PLAIN_GLAT));
        let decoded = decompress(GLAT, &table, &limits).expect("it decodes");
        assert!(matches!(decoded, Cow::Owned(_)));
        assert_eq!(&*decoded, &PLAIN_GLAT[..]);
    }

    #[test]
    fn a_compressed_table_that_is_not_its_table_again_is_refused() {
        let limits = Limits::DEFAULT;
        // The block decodes to another version.
        let mut other = PLAIN_GLAT;
        other[1] = 4;
        let table = compressed(0x0003_0000, 12, &literals(&other));
        assert_eq!(
            decompress(GLAT, &table, &limits),
            Err(Refusal::CompressionMalformed { table: GLAT })
        );
        // The block decodes to a table that says it is compressed still.
        let mut still = PLAIN_GLAT;
        still[4] = 0x08;
        let table = compressed(0x0003_0000, 12, &literals(&still));
        assert_eq!(
            decompress(GLAT, &table, &limits),
            Err(Refusal::CompressionMalformed { table: GLAT })
        );
        // The block decodes short of the size stated.
        let table = compressed(0x0003_0000, 13, &literals(&PLAIN_GLAT));
        assert_eq!(
            decompress(GLAT, &table, &limits),
            Err(Refusal::CompressionMalformed { table: GLAT })
        );
    }

    #[test]
    fn a_stated_size_past_the_ceiling_is_refused_before_decoding() {
        let tight = Limits {
            max_table_bytes: 11,
            ..Limits::DEFAULT
        };
        let table = compressed(0x0003_0000, 12, &literals(&PLAIN_GLAT));
        assert_eq!(
            decompress(GLAT, &table, &tight),
            Err(Refusal::TableTooLarge {
                table: GLAT,
                size: 12
            })
        );
        // The largest size the word can state is refused under the default.
        let table = compressed(0x0005_0000, 0x07FF_FFFF, &[0]);
        assert_eq!(
            decompress(SILF, &table, &Limits::DEFAULT),
            Err(Refusal::TableTooLarge {
                table: SILF,
                size: 0x07FF_FFFF
            })
        );
    }

    #[test]
    fn an_unknown_scheme_and_a_short_header_are_refused() {
        let limits = Limits::DEFAULT;
        let mut table = PLAIN_GLAT.to_vec();
        table[4] = 2 << 3;
        assert_eq!(
            decompress(GLAT, &table, &limits),
            Err(Refusal::UnknownCompression {
                table: GLAT,
                scheme: 2
            })
        );
        assert_eq!(
            decompress(GLAT, &[0, 3, 0, 0, 0], &limits),
            Err(Refusal::TableMalformed { table: GLAT })
        );
        assert_eq!(
            decompress(SILF, &[0, 5], &limits),
            Err(Refusal::TableMalformed { table: SILF })
        );
    }

    #[test]
    fn a_face_with_no_silf_has_no_description() {
        let limits = Limits::DEFAULT;
        assert!(
            Tables::from_tables(None, Some(&PLAIN_GLAT[..]), None, None, None, &limits).is_none()
        );
        // A `Silf` that cannot be decoded still makes a description, held
        // empty, and says why.
        let broken = compressed(0x0005_0000, 64, &[0xF0]);
        let tables = Tables::from_tables(Some(&broken[..]), None, None, None, None, &limits)
            .expect("a description");
        assert!(tables.silf().is_empty());
        assert_eq!(
            tables.refusals(),
            &[Refusal::CompressionMalformed { table: SILF }]
        );
        // A `Glat` that cannot be decoded is held absent, and the rest stand.
        let silf = [0, 4, 0, 0, 0, 4, 0, 2, 0, 0, 0, 0];
        let glat = compressed(0x0003_0000, 1 << 26, &[0]);
        let tables =
            Tables::from_tables(Some(&silf[..]), Some(&glat[..]), None, None, None, &limits)
                .expect("a description");
        assert_eq!(tables.silf(), &silf[..]);
        assert!(tables.glat().is_none());
        assert_eq!(
            tables.refusals(),
            &[Refusal::TableTooLarge {
                table: GLAT,
                size: 1 << 26
            }]
        );
    }
}
