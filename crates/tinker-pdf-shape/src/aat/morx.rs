//! The extended glyph metamorphosis table, `morx`: a face's substitutions as
//! AAT carries them.
//!
//! Apple's *TrueType Reference Manual*, *The 'morx' table*
//! (`docs/design/shaping.md`, *AAT shaping*, *`morx`*).
//!
//! # The chain
//!
//! "Table Format": a version, "either 2 or 3", an unused word and a chain
//! count, then the chains. A chain is `defaultFlags`, `chainLength` ("Total
//! byte count, including this header"), `nFeatureEntries` and `nSubtables`,
//! its feature entries — a feature type and setting and the flags it enables
//! and the complement of those it disables, twelve bytes each — and its
//! subtables, each a `length` ("Total subtable length, including this
//! header"), a `coverage` and `subFeatureFlags` before its body. Version 3's
//! subtable glyph coverage arrays after a chain's last subtable are, in the
//! chapter's words, an optimisation by which "the subtable may be skipped",
//! and are not read, so a wrong one costs no glyph.
//!
//! "Feature Table": "The result is initialized to the chain's defaultFlags.
//! The entries in the feature table are processed in order. If the setting is
//! in the requested list, the result is ANDed with the entry's disableFlags
//! and ORed with the entry's enableFlags", and "Entries that are located
//! later in the feature table take precedence over earlier entries"
//! ([`Chain::flags`]). A subtable runs where the result and its
//! `subFeatureFlags` share a bit: "If any bits are common and are non-zero,
//! the table is enabled."
//!
//! # Coverage
//!
//! "Metamorphosis Subtables": `0x80000000` set, "this subtable will only be
//! applied to vertical text", clear, to horizontal text; `0x20000000`, "applied
//! to both horizontal and vertical text"; and the low byte the subtable's
//! type. An upright run is vertical text, a line's run and a sideways one
//! horizontal. "Normally, when a 'morx' table is processed, the Unicode bidi
//! algorithm has already been run and the glyph stream is in layout order",
//! so [`Morx::run`] reverses a right-to-left run into layout order for the
//! chain and back after it; bits 30 and 28 then choose the walk: layout order
//! with both clear, reverse layout order with 30, logical order with 28 —
//! layout order for a left-to-right run, its reverse for a right-to-left one
//! — and reverse logical order with both ([`Subtable::traversal`]).
//!
//! # What the chain asks for, from a caller's OpenType tags
//!
//! [`Selection::from_opentype`] turns what `Shaper::with_features` and
//! `Shaper::with_settings` say into the feature settings the chain is asked
//! for, by the design's table ([`aat_setting`]), read by meaning from Apple's
//! *Font Feature Registry* and the OpenType tag registry.
//!
//! # The subtables
//!
//! The noncontextual subtable, type 4, is one lookup table: "a lookup table
//! that maps a single glyph index into another glyph index", a value of 0
//! meaning "do nothing" ("Lookup tables in a 'morx'"). Types 0, 1, 2 and 5 are
//! state machines on the shared driver ([`super::StateTable::run`]), and the
//! rearrangement subtable, type 0, runs (`super::rearrangement`); a machine
//! runs only over a run holding a glyph its class table names (`machine`).
//! Type 3 is reserved and a type above 5 unknown, and each such subtable, and
//! each of a type this crate does not run yet, is skipped and named. A glyph
//! a subtable makes `0xFFFF` is deleted: the later subtables see it as the
//! deleted glyph, class 2, and it leaves the run when the chain ends.

use super::feat::Feat;
use super::lookup::{Lookup, LookupError};
use super::rearrangement;
use super::state::{StateTable, DELETED};
use super::Site;
use crate::buffer::{Buffer, Direction};
use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::Warning;

/// `morx`, as the face's table directory spells it.
pub const MORX: Tag = Tag::new(b"morx");

/// The coverage bit of a subtable for vertical text alone.
pub const VERTICAL: u32 = 0x8000_0000;
/// The coverage bit of a subtable walked in descending order.
pub const DESCENDING: u32 = 0x4000_0000;
/// The coverage bit of a subtable for both orientations.
pub const ANY_ORIENTATION: u32 = 0x2000_0000;
/// The coverage bit of a subtable walked in logical order.
pub const LOGICAL: u32 = 0x1000_0000;
/// The coverage byte holding the subtable's type.
const KIND: u32 = 0x0000_00FF;

/// Subtable type 0, rearrangement.
pub const REARRANGEMENT: u8 = 0;
/// Subtable type 1, contextual glyph substitution.
pub const CONTEXTUAL: u8 = 1;
/// Subtable type 2, ligature.
pub const LIGATURE: u8 = 2;
/// Subtable type 4, noncontextual glyph substitution.
pub const NONCONTEXTUAL: u8 = 4;
/// Subtable type 5, glyph insertion.
pub const INSERTION: u8 = 5;

/// The bytes of a chain's header.
const CHAIN_HEADER: usize = 16;
/// The bytes of one feature entry.
const FEATURE_ENTRY: usize = 12;
/// The bytes of a subtable's header.
const SUBTABLE_HEADER: usize = 12;

/// A `morx` table.
#[derive(Clone, Copy, Debug)]
pub struct Morx<'a> {
    data: Bytes<'a>,
    version: u16,
    chains: u32,
}

impl<'a> Morx<'a> {
    /// Reads a `morx` header: `None` for a version other than 2 or 3 or a
    /// header that cannot be read.
    #[must_use]
    pub fn parse(bytes: &'a [u8]) -> Option<Morx<'a>> {
        let data = Bytes::new(bytes);
        let version = data.u16(0)?;
        if version != 2 && version != 3 {
            return None;
        }
        let chains = data.u32(4)?;
        Some(Morx {
            data,
            version,
            chains,
        })
    }

    /// The table's version, 2 or 3.
    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    /// `nChains`, as the table states it.
    #[must_use]
    pub const fn chain_count(&self) -> u32 {
        self.chains
    }

    /// The chains, in table order, each with its feature entries and its
    /// subtables, read as far as the table holds them and `limits` allows,
    /// and what was refused on the way.
    ///
    /// A chain and each of its subtables cost one of
    /// [`Limits::max_subtables`], past which nothing more is read
    /// ([`Warning::SubtableBudgetExceeded`]); a chain's feature entries past
    /// [`Limits::max_feature_entries`] are not read
    /// ([`Warning::FeatureEntryBudgetExceeded`]). A chain whose header or
    /// feature entries cannot be read, or whose length runs past the table,
    /// ends the walk ([`Warning::MalformedChain`]), there being no length to
    /// find the next by; a subtable whose length runs past its chain ends its
    /// chain's subtables ([`Warning::MalformedAatSubtable`]).
    #[must_use]
    pub fn chains(&self, limits: &Limits) -> (Vec<Chain<'a>>, Vec<Warning>) {
        let mut chains: Vec<Chain<'a>> = Vec::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut spent = 0u32;
        let mut at = 8usize;
        for index in 0..self.chains {
            if spent >= limits.max_subtables {
                warnings.push(Warning::SubtableBudgetExceeded { table: MORX });
                break;
            }
            spent = spent.saturating_add(1);
            let Some(chain) = self.chain(index, at, limits, &mut spent, &mut warnings) else {
                warnings.push(Warning::MalformedChain {
                    table: MORX,
                    chain: index,
                });
                break;
            };
            at = match at.checked_add(chain.length) {
                Some(next) => next,
                None => break,
            };
            let stop = spent > limits.max_subtables;
            chains.push(chain);
            if stop {
                break;
            }
        }
        (chains, dedup(warnings))
    }

    /// The chain whose header is at `at`, its subtables read while `spent`
    /// stays within the ceiling; `None` where its header or its feature
    /// entries cannot be read.
    fn chain(
        &self,
        index: u32,
        at: usize,
        limits: &Limits,
        spent: &mut u32,
        warnings: &mut Vec<Warning>,
    ) -> Option<Chain<'a>> {
        let header = self.data.at(at)?;
        let default_flags = header.u32(0)?;
        let length = usize::try_from(header.u32(4)?).ok()?;
        let feature_count = header.u32(8)?;
        let subtable_count = header.u32(12)?;
        if length < CHAIN_HEADER {
            return None;
        }
        let bytes = Bytes::new(header.as_slice().get(..length)?);

        let readable = feature_count.min(limits.max_feature_entries);
        if readable < feature_count {
            warnings.push(Warning::FeatureEntryBudgetExceeded {
                table: MORX,
                chain: index,
            });
        }
        let mut features = Vec::new();
        for n in 0..readable {
            let entry =
                CHAIN_HEADER.checked_add(usize::try_from(n).ok()?.checked_mul(FEATURE_ENTRY)?)?;
            features.push(FeatureEntry {
                feature: bytes.u16(entry)?,
                setting: bytes.u16(entry.checked_add(2)?)?,
                enable: bytes.u32(entry.checked_add(4)?)?,
                disable: bytes.u32(entry.checked_add(8)?)?,
            });
        }

        let mut subtables = Vec::new();
        let mut sub = CHAIN_HEADER.checked_add(
            usize::try_from(feature_count)
                .ok()?
                .checked_mul(FEATURE_ENTRY)?,
        )?;
        for n in 0..subtable_count {
            if *spent >= limits.max_subtables {
                warnings.push(Warning::SubtableBudgetExceeded { table: MORX });
                *spent = limits.max_subtables.saturating_add(1);
                break;
            }
            *spent = spent.saturating_add(1);
            let site = Site {
                table: MORX,
                chain: index,
                subtable: n,
            };
            let Some((size, coverage, flags, body)) = subtable_at(bytes, sub) else {
                warnings.push(Warning::MalformedAatSubtable { site });
                break;
            };
            subtables.push(Subtable {
                index: n,
                coverage,
                sub_feature_flags: flags,
                body,
            });
            sub = match sub.checked_add(size) {
                Some(next) => next,
                None => break,
            };
        }
        Some(Chain {
            index,
            length,
            default_flags,
            features,
            subtables,
        })
    }

    /// Runs the table's chains over `buffer`, as `selection` asks, and says
    /// what it refused.
    ///
    /// The run's direction is the buffer's: a right-to-left run is reversed
    /// into layout order for the chain and back after it, and a top-to-bottom
    /// one is vertical text. Each chain's subtables run in order where the
    /// chain's flags select them and their coverage takes the run's
    /// orientation, each walking the run the way its coverage says; glyphs a
    /// subtable deleted leave the run when the chains end. Each warning is
    /// recorded once.
    pub fn run(&self, buffer: &mut Buffer, selection: &Selection, limits: &Limits) -> Vec<Warning> {
        let direction = buffer.direction();
        let rtl = direction == Direction::RightToLeft;
        let vertical = direction.is_vertical();
        let (chains, mut warnings) = self.chains(limits);
        if rtl {
            buffer.reverse();
        }
        for chain in &chains {
            let flags = chain.flags(selection);
            for subtable in &chain.subtables {
                if subtable.sub_feature_flags & flags == 0 || !subtable.applies(vertical) {
                    continue;
                }
                let site = Site {
                    table: MORX,
                    chain: chain.index,
                    subtable: subtable.index,
                };
                let traversal = subtable.traversal(rtl);
                let outcome = match subtable.kind() {
                    NONCONTEXTUAL => {
                        noncontextual(subtable.body, buffer, site, &mut warnings);
                        Ok(())
                    }
                    REARRANGEMENT => match machine(
                        subtable.body,
                        rearrangement::ENTRY_SIZE,
                        buffer,
                        site,
                        &mut warnings,
                    ) {
                        Some(table) => rearrangement::run(&table, buffer, traversal, limits, site),
                        None => Ok(()),
                    },
                    kind => Err(Warning::SubtableTypeNotRun { site, kind }),
                };
                if let Err(warning) = outcome {
                    warnings.push(warning);
                }
            }
        }
        buffer.remove_every(DELETED);
        if rtl {
            buffer.reverse();
        }
        dedup(warnings)
    }
}

/// The state machine of a subtable whose entries are `entry_size` bytes,
/// where it is to run over `buffer`: `None` where its header cannot be read,
/// which is named, and where the run holds no glyph its class table gives a
/// class other than out of bounds.
///
/// The second is this engine's reading. The chapter lets "the subtable ...
/// be skipped" where "none of the glyphs in the run is used by the subtable"
/// ("Subtable Glyph Coverage Tables"), and where a table states no coverage —
/// version 2 states none — the glyphs a machine uses are the ones its class
/// table names. A machine skipped takes no end-of-text transition either:
/// text-rendering-tests `MORX-20` holds it, its `EE` and `E` keeping their
/// glyphs though the machine's end-of-text entry substitutes both the marked
/// and the current glyph.
fn machine<'a>(
    body: &'a [u8],
    entry_size: usize,
    buffer: &Buffer,
    site: Site,
    warnings: &mut Vec<Warning>,
) -> Option<StateTable<'a>> {
    match StateTable::parse(body, entry_size) {
        Ok(table) => table.reaches(buffer).then_some(table),
        Err(error) => {
            warnings.push(error.warning(site));
            None
        }
    }
}

/// The subtable whose header is at `at` in its chain's bytes: its length,
/// coverage and sub-feature flags, and its body; `None` where its header
/// cannot be read, it is shorter than its header, or its length runs past
/// the chain.
fn subtable_at(chain: Bytes<'_>, at: usize) -> Option<(usize, u32, u32, &[u8])> {
    let size = usize::try_from(chain.u32(at)?).ok()?;
    let coverage = chain.u32(at.checked_add(4)?)?;
    let flags = chain.u32(at.checked_add(8)?)?;
    if size < SUBTABLE_HEADER {
        return None;
    }
    let body = chain
        .as_slice()
        .get(at.checked_add(SUBTABLE_HEADER)?..at.checked_add(size)?)?;
    Some((size, coverage, flags, body))
}

/// Each warning once, in the order first recorded.
fn dedup(warnings: Vec<Warning>) -> Vec<Warning> {
    let mut out: Vec<Warning> = Vec::with_capacity(warnings.len());
    for warning in warnings {
        if !out.contains(&warning) {
            out.push(warning);
        }
    }
    out
}

/// One chain of a `morx` table.
#[derive(Clone, Debug)]
pub struct Chain<'a> {
    index: u32,
    length: usize,
    default_flags: u32,
    features: Vec<FeatureEntry>,
    subtables: Vec<Subtable<'a>>,
}

impl<'a> Chain<'a> {
    /// Which chain of its table this is, counting from zero.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// `defaultFlags`: "The default specification for subtables".
    #[must_use]
    pub const fn default_flags(&self) -> u32 {
        self.default_flags
    }

    /// The feature entries read, in table order.
    #[must_use]
    pub fn features(&self) -> &[FeatureEntry] {
        &self.features
    }

    /// The subtables read, in table order.
    #[must_use]
    pub fn subtables(&self) -> &[Subtable<'a>] {
        &self.subtables
    }

    /// The sub-feature flags `selection` gives this chain: from its
    /// `defaultFlags`, or from none where the caller named its own
    /// features, each feature entry in table order whose feature and setting
    /// are requested setting `flags = (flags & disable) | enable`.
    #[must_use]
    pub fn flags(&self, selection: &Selection) -> u32 {
        let mut flags = if selection.start_at_defaults {
            self.default_flags
        } else {
            0
        };
        for entry in &self.features {
            if selection.requests(entry.feature, entry.setting) {
                flags = (flags & entry.disable) | entry.enable;
            }
        }
        flags
    }
}

/// One feature entry of a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureEntry {
    /// `featureType`.
    pub feature: u16,
    /// `featureSetting`, "aka selector".
    pub setting: u16,
    /// `enableFlags`: the sub-feature flags the setting turns on.
    pub enable: u32,
    /// `disableFlags`: the complement of the flags it turns off.
    pub disable: u32,
}

/// One subtable of a chain.
#[derive(Clone, Copy, Debug)]
pub struct Subtable<'a> {
    index: u32,
    /// `coverage`: its orientation and walk bits, and its type.
    pub coverage: u32,
    /// `subFeatureFlags`: the flags that select it.
    pub sub_feature_flags: u32,
    body: &'a [u8],
}

impl<'a> Subtable<'a> {
    /// Which subtable of its chain this is, counting from zero.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Its type, the low byte of its coverage.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        // The mask keeps the low byte, which fits.
        #[allow(clippy::cast_possible_truncation)]
        let kind = (self.coverage & KIND) as u8;
        kind
    }

    /// Its body: the bytes after its header, to its length.
    #[must_use]
    pub const fn body(&self) -> &'a [u8] {
        self.body
    }

    /// Whether it runs over a run of this orientation: vertical text for a
    /// subtable with `0x80000000` set, horizontal for one with it clear, and
    /// both for one with `0x20000000` set.
    #[must_use]
    pub const fn applies(&self, vertical: bool) -> bool {
        self.coverage & ANY_ORIENTATION != 0 || (self.coverage & VERTICAL != 0) == vertical
    }

    /// The way it walks a run held in layout order: backward where bit 30
    /// asks for the reverse of the order bit 28 chooses, layout order or, for
    /// a right-to-left run, logical order, its reverse.
    #[must_use]
    pub const fn traversal(&self, rtl: bool) -> super::Traversal {
        let reverse = self.coverage & DESCENDING != 0;
        let logical_reversed = self.coverage & LOGICAL != 0 && rtl;
        if reverse != logical_reversed {
            super::Traversal::Backward
        } else {
            super::Traversal::Forward
        }
    }
}

/// Runs a noncontextual subtable over `buffer`: each glyph its lookup table
/// covers with a value other than 0 becomes that value.
fn noncontextual(body: &[u8], buffer: &mut Buffer, site: Site, warnings: &mut Vec<Warning>) {
    let lookup = match Lookup::parse(body) {
        Ok(lookup) => lookup,
        Err(LookupError::Format(format)) => {
            warnings.push(Warning::UnknownLookupFormat { site, format });
            return;
        }
        Err(LookupError::Malformed) => {
            warnings.push(Warning::MalformedAatSubtable { site });
            return;
        }
    };
    for at in 0..buffer.len() {
        let Some(glyph) = buffer.glyph(at).map(|g| g.glyph) else {
            continue;
        };
        if glyph == DELETED {
            continue;
        }
        match lookup.value(glyph) {
            None | Some(0) => {}
            Some(value) => match u16::try_from(value) {
                Ok(replacement) => buffer.set_glyph(at, replacement),
                // A glyph index is sixteen bits, and a value past them
                // names none.
                Err(_) => warnings.push(Warning::MalformedAatSubtable { site }),
            },
        }
    }
}

/// What a run asks of a `morx` chain: where its sub-feature flags start, and
/// the feature settings it requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Whether each chain's flags start at its `defaultFlags` — the face's
    /// own statement of its defaults — or at none, for a caller who named
    /// its own features.
    pub start_at_defaults: bool,
    /// The `(feature type, setting)` pairs requested, each once.
    pub settings: Vec<(u16, u16)>,
}

impl Selection {
    /// The face's defaults and no request beside them: what a run whose
    /// caller asks for nothing gets.
    #[must_use]
    pub const fn defaults() -> Selection {
        Selection {
            start_at_defaults: true,
            settings: Vec::new(),
        }
    }

    /// Whether `(feature, setting)` is requested.
    #[must_use]
    pub fn requests(&self, feature: u16, setting: u16) -> bool {
        self.settings.contains(&(feature, setting))
    }

    /// The selection a caller's OpenType features make
    /// (`docs/design/shaping.md`, *Requests from a caller*), and the tags it
    /// asked for that no AAT feature answers.
    ///
    /// `named` is what `Shaper::with_features` names, `None` where it was not
    /// called: then the flags start at each chain's defaults; otherwise they
    /// start at none and each named tag asks for its feature's on-setting,
    /// so naming nothing runs no subtable. `settings` is what
    /// `Shaper::with_settings` sets over that, the last setting of a tag
    /// winning: a tag set above 0 asks for its on-setting, and one set to 0
    /// the off-setting of a non-exclusive feature — the on-setting plus one,
    /// the registry's pairing — and nothing of an exclusive one, whose
    /// chain's defaults then stand. A feature is exclusive where the face's
    /// `feat` says so, or, where it has none or does not list the feature,
    /// where the registry marks it so, and an exclusive feature keeps only
    /// the last setting asked of it. The tags are taken in the order first
    /// named. `kern` is no `morx` feature and asks for nothing here; every
    /// other tag without a row in [`aat_setting`]'s table asks for nothing
    /// and is named, once ([`Warning::FeatureNotInAat`]).
    #[must_use]
    pub fn from_opentype(
        named: Option<&[Tag]>,
        settings: &[(Tag, u32)],
        feat: Option<&Feat<'_>>,
    ) -> (Selection, Vec<Warning>) {
        let mut order: Vec<Tag> = Vec::new();
        let mut values: Vec<(Tag, u32)> = Vec::new();
        let mut set = |tag: Tag, value: u32, order: &mut Vec<Tag>| {
            if !order.contains(&tag) {
                order.push(tag);
            }
            match values.iter_mut().find(|(t, _)| *t == tag) {
                Some(slot) => slot.1 = value,
                None => values.push((tag, value)),
            }
        };
        for tag in named.unwrap_or(&[]) {
            set(*tag, 1, &mut order);
        }
        for (tag, value) in settings {
            set(*tag, *value, &mut order);
        }

        let mut requested: Vec<(u16, u16)> = Vec::new();
        let mut warnings: Vec<Warning> = Vec::new();
        for tag in order {
            let value = values
                .iter()
                .find(|(t, _)| *t == tag)
                .map_or(0, |(_, value)| *value);
            if tag == KERN {
                continue;
            }
            let Some(row) = aat_setting(tag) else {
                warnings.push(Warning::FeatureNotInAat { tag });
                continue;
            };
            let exclusive = feat
                .and_then(|feat| feat.exclusive(row.feature))
                .unwrap_or(row.exclusive);
            let asked = if value > 0 {
                if exclusive {
                    requested.retain(|(feature, _)| *feature != row.feature);
                }
                (row.feature, row.selector)
            } else if exclusive {
                continue;
            } else {
                (row.feature, row.selector.saturating_add(1))
            };
            if !requested.contains(&asked) {
                requested.push(asked);
            }
        }
        (
            Selection {
                start_at_defaults: named.is_none(),
                settings: requested,
            },
            dedup(warnings),
        )
    }
}

/// `kern`, which turns `kerx` off rather than asking anything of `morx`.
const KERN: Tag = Tag::new(b"kern");

/// The AAT feature setting an OpenType tag asks for: one row of
/// `docs/design/shaping.md`'s table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AatSetting {
    /// The feature type.
    pub feature: u16,
    /// The on-setting the tag asks for.
    pub selector: u16,
    /// Whether the *Font Feature Registry* marks the feature's settings
    /// exclusive.
    pub exclusive: bool,
}

/// The design's table: each OpenType tag and the AAT feature type,
/// on-setting and registry exclusivity it maps to.
const MAPPING: &[(Tag, u16, u16, bool)] = &[
    // Ligatures (1).
    (Tag::new(b"rlig"), 1, 0, false),
    (Tag::new(b"liga"), 1, 2, false),
    (Tag::new(b"dlig"), 1, 4, false),
    (Tag::new(b"clig"), 1, 18, false),
    (Tag::new(b"hlig"), 1, 20, false),
    // Contextual Alternates (36).
    (Tag::new(b"calt"), 36, 0, false),
    (Tag::new(b"swsh"), 36, 2, false),
    (Tag::new(b"cswh"), 36, 4, false),
    // Lower Case (37) and Upper Case (38).
    (Tag::new(b"smcp"), 37, 1, true),
    (Tag::new(b"pcap"), 37, 2, true),
    (Tag::new(b"c2sc"), 38, 1, true),
    (Tag::new(b"c2pc"), 38, 2, true),
    // Number Case (21) and Number Spacing (6).
    (Tag::new(b"onum"), 21, 0, true),
    (Tag::new(b"lnum"), 21, 1, true),
    (Tag::new(b"tnum"), 6, 0, true),
    (Tag::new(b"pnum"), 6, 1, true),
    // Fractions (11) and Vertical Position (10).
    (Tag::new(b"afrc"), 11, 1, true),
    (Tag::new(b"frac"), 11, 2, true),
    (Tag::new(b"sups"), 10, 1, true),
    (Tag::new(b"subs"), 10, 2, true),
    (Tag::new(b"ordn"), 10, 3, true),
    (Tag::new(b"sinf"), 10, 4, true),
    // Typographic Extras (14): `kSlashedZeroOnSelector`, the row the registry
    // titles "Unslashed Zero".
    (Tag::new(b"zero"), 14, 4, false),
    // Mathematical Extras (15).
    (Tag::new(b"mgrk"), 15, 10, false),
    // Case Sensitive Layout (33).
    (Tag::new(b"case"), 33, 0, false),
    (Tag::new(b"cpsp"), 33, 2, false),
    // Stylistic Alternatives (35), "the AAT equivalent of the OpenType 'ss01'
    // through 'ss20' features": the nth set at 2n.
    (Tag::new(b"ss01"), 35, 2, false),
    (Tag::new(b"ss02"), 35, 4, false),
    (Tag::new(b"ss03"), 35, 6, false),
    (Tag::new(b"ss04"), 35, 8, false),
    (Tag::new(b"ss05"), 35, 10, false),
    (Tag::new(b"ss06"), 35, 12, false),
    (Tag::new(b"ss07"), 35, 14, false),
    (Tag::new(b"ss08"), 35, 16, false),
    (Tag::new(b"ss09"), 35, 18, false),
    (Tag::new(b"ss10"), 35, 20, false),
    (Tag::new(b"ss11"), 35, 22, false),
    (Tag::new(b"ss12"), 35, 24, false),
    (Tag::new(b"ss13"), 35, 26, false),
    (Tag::new(b"ss14"), 35, 28, false),
    (Tag::new(b"ss15"), 35, 30, false),
    (Tag::new(b"ss16"), 35, 32, false),
    (Tag::new(b"ss17"), 35, 34, false),
    (Tag::new(b"ss18"), 35, 36, false),
    (Tag::new(b"ss19"), 35, 38, false),
    (Tag::new(b"ss20"), 35, 40, false),
    // Character Alternatives (17) and Style Options (19).
    (Tag::new(b"salt"), 17, 1, true),
    (Tag::new(b"titl"), 19, 4, true),
    // Text Spacing (22).
    (Tag::new(b"pwid"), 22, 0, true),
    (Tag::new(b"fwid"), 22, 1, true),
    (Tag::new(b"hwid"), 22, 2, true),
    (Tag::new(b"twid"), 22, 3, true),
    (Tag::new(b"qwid"), 22, 4, true),
    // Character Shape (20), whose registry entry names `hojo`.
    (Tag::new(b"trad"), 20, 0, true),
    (Tag::new(b"smpl"), 20, 1, true),
    (Tag::new(b"jp78"), 20, 2, true),
    (Tag::new(b"jp83"), 20, 3, true),
    (Tag::new(b"jp90"), 20, 4, true),
    (Tag::new(b"expt"), 20, 10, true),
    (Tag::new(b"jp04"), 20, 11, true),
    (Tag::new(b"hojo"), 20, 12, true),
    (Tag::new(b"nlck"), 20, 13, true),
    (Tag::new(b"tnam"), 20, 14, true),
    // Ruby Kana (28), Italic CJK Roman (32) and Alternate Kana (34).
    (Tag::new(b"ruby"), 28, 2, false),
    (Tag::new(b"ital"), 32, 2, false),
    (Tag::new(b"hkna"), 34, 0, false),
    (Tag::new(b"vkna"), 34, 2, false),
    // Vertical Substitution (4).
    (Tag::new(b"vert"), 4, 0, false),
    (Tag::new(b"vrt2"), 4, 0, false),
];

/// The AAT feature setting `tag` asks for, by the design's table; `None`
/// for a tag with no row, `kern` among them.
#[must_use]
pub fn aat_setting(tag: Tag) -> Option<AatSetting> {
    MAPPING
        .iter()
        .find(|(of, _, _, _)| *of == tag)
        .map(|(_, feature, selector, exclusive)| AatSetting {
            feature: *feature,
            selector: *selector,
            exclusive: *exclusive,
        })
}

#[cfg(test)]
mod tests {
    use super::{aat_setting, Subtable, MAPPING};
    use crate::aat::Traversal;
    use crate::common::Tag;

    /// No tag has two rows, every tag is four printable bytes, and every
    /// non-exclusive setting is even, the registry pairing it with the odd
    /// setting after it.
    #[test]
    fn the_mapping_has_one_row_a_tag_and_pairs_its_non_exclusive_settings() {
        for (at, (tag, feature, selector, exclusive)) in MAPPING.iter().enumerate() {
            assert!(
                MAPPING.iter().skip(at + 1).all(|(other, ..)| other != tag),
                "{tag} has two rows"
            );
            assert!(tag.to_bytes().iter().all(u8::is_ascii_alphanumeric));
            if !exclusive {
                assert_eq!(selector % 2, 0, "{tag}: feature {feature}");
            }
            assert_eq!(
                aat_setting(*tag).map(|row| (row.feature, row.selector)),
                Some((*feature, *selector))
            );
        }
        assert_eq!(MAPPING.len(), 69);
        assert!(aat_setting(Tag::new(b"kern")).is_none());
        assert!(aat_setting(Tag::new(b"mark")).is_none());
    }

    /// The four walks bits 30 and 28 choose, over a left-to-right run and a
    /// right-to-left one held in layout order.
    #[test]
    fn bits_thirty_and_twenty_eight_choose_the_walk() {
        let walk = |coverage: u32, rtl: bool| {
            Subtable {
                index: 0,
                coverage,
                sub_feature_flags: 1,
                body: &[],
            }
            .traversal(rtl)
        };
        let (forward, backward) = (Traversal::Forward, Traversal::Backward);
        // Layout order, reverse layout order, logical order, reverse logical.
        assert_eq!((walk(0, false), walk(0, true)), (forward, forward));
        assert_eq!(
            (walk(0x4000_0000, false), walk(0x4000_0000, true)),
            (backward, backward)
        );
        assert_eq!(
            (walk(0x1000_0000, false), walk(0x1000_0000, true)),
            (forward, backward)
        );
        assert_eq!(
            (walk(0x5000_0000, false), walk(0x5000_0000, true)),
            (backward, forward)
        );
    }
}
