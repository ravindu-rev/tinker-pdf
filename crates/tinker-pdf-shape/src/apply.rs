//! Running a lookup list: what a lookup skips, what it matches, and what
//! happens when one lookup names another.
//!
//! `GSUB` and `GPOS` differ in what a subtable *does* and agree completely on
//! everything around it — which glyphs a lookup can see, how a contextual rule
//! matches, and what a nested lookup means. So all of that is here, once, and
//! `gsub.rs` and `gpos.rs` hold only the eight and nine kinds of subtable.
//!
//! # The three things a hostile font can do here, and what stops each
//!
//! **Name itself.** A contextual lookup names other lookups by index, and
//! index *n* may name *n*. [`Limits::max_nesting_depth`] bounds that, and it
//! is the only bound that can: there is nothing in the table to detect, since
//! a lookup naming itself at a different position is a legitimate and common
//! construction.
//!
//! **Point at itself.** An extension subtable is a 32-bit offset to another
//! subtable, and ISO/IEC 14496-22 forbids it pointing at another extension.
//! That rule is checked *and* [`Limits::max_extension_depth`] is enforced,
//! because a rule a conforming font obeys is not a defence against one that
//! does not.
//!
//! **Grow.** Multiple substitution replaces one glyph with up to 65 535, and
//! a feature may run it repeatedly. [`Limits::max_glyphs`] bounds the buffer
//! and [`Limits::max_operations`] bounds the subtables searched, and the two
//! are separate because a font can exhaust either without touching the other.
//! The unit is the *subtable* rather than the attempt for the reason
//! [`Runner::apply_at`] gives: `subTableCount` is the font's to choose, so a
//! ceiling counting attempts leaves it choosing what each attempt costs.
//!
//! # Skipping is not a filter on the output
//!
//! A lookup flag says which glyphs this lookup *cannot see*: a rule with
//! `IGNORE_MARKS` matching `f`, `i` matches across a combining accent between
//! them, and the accent stays exactly where it was. Getting this wrong is the
//! difference between a shaper that sets Arabic and one that mostly does:
//! every joining rule in every Arabic face is written assuming the vowel
//! signs are invisible to it.
//!
//! # Passing what no subtable covers
//!
//! A runner handed a [`LookupIndex`] for a lookup does not probe a position
//! whose glyph none of the lookup's subtables could apply at: it charges the
//! operation budget what the probe would have cost and moves on. Everything
//! else — the order lookups run in, the positions visited, every warning —
//! is the probing runner's, because the index is built only where skipping a
//! probe can change nothing but the time it takes (`docs/design/shaping.md`,
//! *Lookup plans and coverage indexes*).

use std::sync::Arc;

use crate::buffer::Buffer;
use crate::common::{
    ClassDef, Coverage, GlyphSet, Lookup, LookupList, IGNORE_BASE_GLYPHS, IGNORE_LIGATURES,
    IGNORE_MARKS,
};
use crate::gdef::{Gdef, GlyphClass};
use crate::limits::Limits;
use crate::read::Bytes;
use crate::{Table, Warning};

/// The three `IGNORE_*` bits together, for the two searches that have to
/// override them.
const IGNORE_FLAGS: u16 = IGNORE_BASE_GLYPHS | IGNORE_LIGATURES | IGNORE_MARKS;

/// One run of one lookup list over one buffer.
///
/// Holds the budgets, because they are spent across lookups rather than
/// within one, and the warnings, because ruling 10 wants the object each
/// refusal touched and the object is a lookup index.
pub(crate) struct Runner<'a, 'g> {
    pub(crate) table: Table,
    pub(crate) lookups: LookupList<'a>,
    pub(crate) gdef: Option<&'g Gdef<'a>>,
    pub(crate) limits: Limits,
    pub(crate) warnings: Vec<Warning>,
    ops: u32,
    /// The feature mask of the lookup currently running. See
    /// [`Runner::skipper`].
    mask: u32,
    /// The coverage index of each lookup [`Runner::run`] is handed, by its
    /// place in that list; empty, or `None` at a place, where there is none.
    covered: &'g [Option<Arc<LookupIndex>>],
    /// Whether a `GSUB` rule's input is kept inside the cluster it starts
    /// in: the stage's own answer, from the feature group it runs
    /// (`docs/design/shaping.md`, *What a cluster confines*). See
    /// [`Runner::per_syllable`].
    confined: bool,
}

/// What a lookup's subtables could apply at, so a position none of them
/// could is passed without resolving or probing one.
///
/// Built once per face and lookup ([`crate::Plans`]) from each subtable's
/// [`Gate`]: the coverage it reads first at a position, as its kind reads it.
/// A lookup some subtable of which warns whatever the glyph has none.
#[derive(Debug)]
pub(crate) struct LookupIndex {
    /// The glyphs some subtable could apply at.
    any: GlyphSet,
    /// Each subtable's own, where the lookup has more than one; empty where
    /// it has one, whose set is `any`.
    each: Vec<GlyphSet>,
    /// The lookup's `subTableCount`, which a passed position is charged.
    subtables: usize,
    /// The most extension indirections one subtable takes to resolve:
    /// `Runner::resolve` warns past [`Limits::max_extension_depth`].
    extension_hops: u32,
    /// The longest input a format 3 rule matches: `Runner::match_input`
    /// warns past [`Limits::max_context_length`] wherever it starts.
    longest_input: usize,
}

/// A fixed charge for each subtable an index records, beside its bitmap, so
/// a lookup of 65 535 empty subtables is not free to index.
const SUBTABLE_COST: usize = 32;

impl LookupIndex {
    /// The index of `lookup`, read from table `table`, its coverage bytes
    /// and everything it keeps charged to `budget`; `None` where some
    /// subtable's probe warns whatever the glyph, or the budget runs out.
    pub(crate) fn build(table: Table, lookup: &Lookup<'_>, budget: &mut usize) -> Option<Self> {
        let subtables = lookup.len();
        *budget = budget.checked_sub(subtables.checked_mul(SUBTABLE_COST)?)?;
        let mut each: Vec<GlyphSet> = Vec::new();
        let mut every: Vec<(u16, u16)> = Vec::new();
        let mut extension_hops = 0u32;
        let mut longest_input = 0usize;
        for sub in 0..subtables {
            let traced = trace(table, lookup, sub)?;
            extension_hops = extension_hops.max(traced.hops);
            let mut ranges: Vec<(u16, u16)> = Vec::new();
            if let Some((kind, data)) = traced.resolved {
                match gate(table, kind, data) {
                    Gate::Everywhere => return None,
                    Gate::Never => {}
                    Gate::Coverage(coverage, input) => {
                        longest_input = longest_input.max(input);
                        if let Some(coverage) = coverage {
                            coverage.ranges(&mut ranges, budget)?;
                        }
                    }
                }
            }
            every.extend_from_slice(&ranges);
            if subtables > 1 {
                let set = GlyphSet::from_ranges(&mut ranges);
                *budget = budget.checked_sub(set.bytes())?;
                each.push(set);
            }
        }
        let any = if subtables > 1 {
            GlyphSet::union(&each)
        } else {
            GlyphSet::from_ranges(&mut every)
        };
        *budget = budget.checked_sub(any.bytes())?;
        Some(Self {
            any,
            each,
            subtables,
            extension_hops,
            longest_input,
        })
    }

    /// Whether this index answers for a run under `limits`: it is set aside
    /// where the limits would make a probe warn whatever the glyph.
    pub(crate) fn answers_under(&self, limits: &Limits) -> bool {
        self.extension_hops <= limits.max_extension_depth
            && self.longest_input <= limits.max_context_length
    }

    /// Whether subtable `sub` could apply at `glyph`.
    fn subtable_holds(&self, sub: usize, glyph: u16) -> bool {
        if self.each.is_empty() {
            return self.any.holds(glyph);
        }
        self.each.get(sub).is_some_and(|set| set.holds(glyph))
    }

    /// How many subtables could apply at `glyph`.
    #[cfg(test)]
    pub(crate) fn holding(&self, glyph: u16) -> usize {
        (0..self.subtables)
            .filter(|sub| self.subtable_holds(*sub, glyph))
            .count()
    }
}

/// What a resolved subtable needs of the glyph where it is tried before it
/// can apply or warn.
///
/// Read off the subtable as `gsub.rs`, `gpos.rs` and the contextual matchers
/// below read it first, before anything that could apply or warn, so that a
/// glyph its coverage does not hold is one where the probe returns
/// [`Applied::No`] and says nothing.
pub(crate) enum Gate<'a> {
    /// The subtable applies, or warns, only at a glyph this coverage holds;
    /// with the input count a format 3 rule matches, which warns past
    /// [`Limits::max_context_length`] wherever it is tried. No coverage is a
    /// subtable that holds no glyph but may still warn.
    Coverage(Option<Coverage<'a>>, usize),
    /// It never applies and says nothing, whatever the glyph.
    Never,
    /// A probe warns whatever the glyph: a type this crate does not read.
    Everywhere,
}

/// The [`Gate`] of a subtable of resolved `kind` in `table`.
pub(crate) fn gate(table: Table, kind: u16, data: Bytes<'_>) -> Gate<'_> {
    // The coverage at offset 2, read first by every simple subtable.
    let first = Gate::Coverage(data.offset16(2).map(Coverage::new), 0);
    match (table, kind) {
        (Table::Gsub, 1) | (Table::Gpos, 1 | 2) => first,
        // The same, after a format check that refuses anything but format 1.
        (Table::Gsub, 2 | 3 | 4 | 8) | (Table::Gpos, 3..=6) => {
            if data.u16(0) == Some(1) {
                first
            } else {
                Gate::Never
            }
        }
        (Table::Gsub, 5) | (Table::Gpos, 7) => context_gate(data),
        (Table::Gsub, 6) | (Table::Gpos, 8) => chain_gate(data),
        _ => Gate::Everywhere,
    }
}

/// [`apply_context`]'s first reads.
fn context_gate(data: Bytes<'_>) -> Gate<'_> {
    match data.u16(0) {
        Some(1 | 2) => Gate::Coverage(data.offset16(2).map(Coverage::new), 0),
        Some(3) => {
            // `context_format3`: the counts, then `match_input` over the
            // whole input from the first coverage.
            let (Some(glyphs), Some(_)) = (data.u16(2), data.u16(4)) else {
                return Gate::Never;
            };
            if glyphs == 0 {
                return Gate::Never;
            }
            Gate::Coverage(
                data.offset16_at(6, 0).map(Coverage::new),
                usize::from(glyphs),
            )
        }
        _ => Gate::Never,
    }
}

/// [`apply_chain_context`]'s first reads.
fn chain_gate(data: Bytes<'_>) -> Gate<'_> {
    match data.u16(0) {
        Some(1 | 2) => Gate::Coverage(data.offset16(2).map(Coverage::new), 0),
        Some(3) => {
            // `chain_format3`: every count it reads before `match_input`,
            // the same way, so a table it refuses is one this refuses.
            let gate = || -> Option<Gate<'_>> {
                let backtrack = usize::from(data.u16(2)?);
                let input_count_at = 4usize.checked_add(backtrack.saturating_mul(2))?;
                let input = usize::from(data.u16(input_count_at)?);
                let input_at = input_count_at + 2;
                let lookahead_count_at = input_at.checked_add(input.saturating_mul(2))?;
                let lookahead = usize::from(data.u16(lookahead_count_at)?);
                let lookahead_at = lookahead_count_at + 2;
                let records_count_at = lookahead_at.checked_add(lookahead.saturating_mul(2))?;
                data.u16(records_count_at)?;
                if input == 0 {
                    return None;
                }
                Some(Gate::Coverage(
                    data.offset16_at(input_at, 0).map(Coverage::new),
                    input,
                ))
            };
            gate().unwrap_or(Gate::Never)
        }
        _ => Gate::Never,
    }
}

/// Where [`Runner::resolve`] would take a subtable, and how far.
struct Traced<'a> {
    /// The extension indirections it entered, each checked against
    /// [`Limits::max_extension_depth`] as it is entered.
    hops: u32,
    /// The type and subtable it resolves to, or `None` for one it cannot
    /// read, which never applies and says nothing.
    resolved: Option<(u16, Bytes<'a>)>,
}

/// [`Runner::resolve`], warning nothing: `None` where it would warn
/// whatever the limits — an extension of a format other than 1, or one
/// naming the extension type — since a probe of that subtable always speaks.
fn trace<'a>(table: Table, lookup: &Lookup<'a>, sub: usize) -> Option<Traced<'a>> {
    let extension = match table {
        Table::Gsub => 7,
        Table::Gpos => 9,
    };
    let mut kind = lookup.kind();
    let Some(mut data) = lookup.subtable(sub) else {
        return Some(Traced {
            hops: 0,
            resolved: None,
        });
    };
    let mut hops = 0u32;
    while kind == extension {
        hops += 1;
        let unread = Some(Traced {
            hops,
            resolved: None,
        });
        match data.u16(0) {
            Some(1) => {}
            Some(_) => return None,
            None => return unread,
        }
        let Some(inner) = data.u16(2) else {
            return unread;
        };
        if inner == extension {
            return None;
        }
        let Some(next) = data.offset32(4) else {
            return unread;
        };
        data = next;
        kind = inner;
    }
    Some(Traced {
        hops,
        resolved: Some((kind, data)),
    })
}

/// Which glyphs a lookup can see.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Skipper<'a> {
    flags: u16,
    mark_set: Option<Coverage<'a>>,
    /// Which glyphs the *feature* this lookup came from may touch.
    ///
    /// A separate question from the lookup flags, and asked at a different
    /// place: the flags say what this lookup cannot see, the mask says which
    /// positions the feature was turned on at. `!0` — every bit — is a lookup
    /// no feature restricted, which is every lookup in a run that set no
    /// masks.
    ///
    /// It is checked when matching a rule's **input** and not its backtrack
    /// or lookahead. That asymmetry is the specification's: a chaining rule's
    /// context is *"the glyphs around this one"*, whatever features they were
    /// turned on for, while its input is the sequence the feature is being
    /// applied to. [`Skipper::over_context`] is the second reading.
    mask: u32,
    /// The Brahmic cluster this application started in, or `None` where the
    /// run has no clusters.
    ///
    /// A `GSUB` rule may not match outside the syllable it started in; see
    /// [`crate::buffer::Buffer::set_syllable`] for why, and why `GPOS` is not
    /// restricted the same way. Like [`Skipper::mask`] it is checked on a
    /// rule's input and lifted over its context, for the same reason.
    syllable: Option<u16>,
}

impl<'a> Skipper<'a> {
    /// The lookup flags this filter was built from.
    ///
    /// Cursive attachment is the one place a lookup flag is read for
    /// something other than skipping — `RIGHT_TO_LEFT` decides which end of a
    /// join stays on the baseline — so the bits are available here rather
    /// than threading the whole lookup down four call levels for one
    /// comparison.
    pub(crate) const fn flags(&self) -> u16 {
        self.flags
    }

    /// The same filter with the three class-ignore bits cleared, keeping the
    /// mark attachment class and mark filtering set.
    ///
    /// Mark-to-mark attachment needs this: it searches backwards for the mark
    /// it hangs on, so a lookup that ignores marks in its *input* must still
    /// be able to find one as its *anchor*. The filtering set is kept because
    /// that is what a face uses to say which marks stack on which.
    fn without_class_filters(self) -> Self {
        Self {
            flags: self.flags & !IGNORE_FLAGS,
            ..self
        }
    }

    /// The same filter with the feature mask lifted, for matching the
    /// backtrack and lookahead of a chaining rule.
    ///
    /// See [`Skipper::mask`] for why the two halves of a rule are matched
    /// under different rules. The lookup flags are kept exactly as they are:
    /// a rule that ignores marks ignores them in its context too.
    const fn over_context(self) -> Self {
        Self {
            mask: !0,
            syllable: None,
            ..self
        }
    }

    /// A filter that sees everything but marks.
    ///
    /// Mark-to-base and mark-to-ligature both search backwards for "the glyph
    /// this mark sits on", and the specification's answer is the nearest
    /// preceding non-mark — not the nearest glyph the lookup's own flags
    /// admit. A lookup that did not override this would attach a mark to
    /// another mark whenever it happened to ignore base glyphs.
    fn marks_only_ignored() -> Self {
        Self {
            flags: IGNORE_MARKS,
            mark_set: None,
            mask: !0,
            syllable: None,
        }
    }

    /// The same filter, applying inside one Brahmic cluster.
    const fn in_syllable(self, syllable: u16) -> Self {
        Self {
            syllable: if syllable == 0 { None } else { Some(syllable) },
            ..self
        }
    }

    /// Whether the feature this lookup came from was turned on at `at`.
    ///
    /// Separate from [`Skipper::skips`] because it is asked in one more place
    /// than skipping is: a lookup starts at a position only if the feature
    /// reaches it, and a rule matches an input glyph only if the feature
    /// reaches that too, but a glyph the feature does not reach is **not**
    /// stepped over — it ends the match, exactly as a glyph of the wrong index
    /// would.
    fn reaches(&self, buffer: &Buffer, at: usize) -> bool {
        let props = buffer.props(at);
        if props.mask & self.mask == 0 {
            return false;
        }
        self.syllable
            .is_none_or(|syllable| props.syllable == syllable)
    }

    /// Why the glyph at `at` is passed over, for `crate::trace`.
    #[cfg(test)]
    fn passed_because(&self, buffer: &Buffer, at: usize) -> &'static str {
        if self.skips(buffer, at) {
            "the lookup's flags skip it"
        } else if buffer.props(at).mask & self.mask == 0 {
            "the feature's mask does not reach it"
        } else {
            "in another cluster"
        }
    }

    /// Which half of [`Skipper::reaches`] refused the glyph at `at`, for
    /// `crate::trace`.
    #[cfg(test)]
    fn unreached(&self, buffer: &Buffer, at: usize) -> crate::trace::Cause {
        if buffer.props(at).mask & self.mask == 0 {
            crate::trace::Cause::Mask
        } else {
            crate::trace::Cause::Syllable
        }
    }

    /// Whether this lookup steps over the glyph at `at`.
    fn skips(&self, buffer: &Buffer, at: usize) -> bool {
        let props = buffer.props(at);
        match props.class {
            GlyphClass::Base => self.flags & IGNORE_BASE_GLYPHS != 0,
            GlyphClass::Ligature => self.flags & IGNORE_LIGATURES != 0,
            GlyphClass::Mark => {
                if self.flags & IGNORE_MARKS != 0 {
                    return true;
                }
                // The high byte is a mark attachment class, and a non-zero
                // one means "only marks of this class are visible". Both this
                // and the filtering set below apply to marks alone, which is
                // why they sit inside this arm rather than above the match.
                let wanted = self.flags >> 8;
                if wanted != 0 && props.mark_attach != wanted {
                    return true;
                }
                if let Some(set) = self.mark_set {
                    let Some(glyph) = buffer.glyph_id(at) else {
                        return true;
                    };
                    if !set.covers(glyph) {
                        return true;
                    }
                }
                false
            }
            GlyphClass::Unclassified | GlyphClass::Component => false,
        }
    }
}

/// Where a matched input sequence landed.
pub(crate) struct Matched {
    /// The buffer position of each matched glyph, the first included.
    pub(crate) positions: Vec<usize>,
    /// One past the last matched glyph: where a lookup resumes.
    pub(crate) end: usize,
}

impl<'a, 'g> Runner<'a, 'g> {
    pub(crate) fn new(
        table: Table,
        lookups: LookupList<'a>,
        gdef: Option<&'g Gdef<'a>>,
        limits: Limits,
    ) -> Self {
        Self {
            table,
            lookups,
            gdef,
            limits,
            warnings: Vec::new(),
            ops: 0,
            mask: !0,
            covered: &[],
            confined: true,
        }
    }

    /// The same runner, passing over what `covered` says the lookups of
    /// [`Runner::run`]'s list cannot apply at: one entry per lookup, in that
    /// list's order.
    pub(crate) fn with_index(mut self, covered: &'g [Option<Arc<LookupIndex>>]) -> Self {
        self.covered = covered;
        self
    }

    /// The same runner, with a `GSUB` rule's input kept inside its cluster
    /// or let reach across: `true`, the default, keeps the syllables a
    /// buffer carries; `false` is a stage the specification applies to the
    /// whole run.
    pub(crate) const fn with_confinement(mut self, confined: bool) -> Self {
        self.confined = confined;
        self
    }

    /// Records a refusal, once.
    ///
    /// Deduplicated rather than counted, and deliberately: a hostile face can
    /// make the same refusal fire a hundred thousand times, and a warning
    /// list that grew with it would be the denial of service the budgets were
    /// added to prevent. The list stays short enough that the linear scan is
    /// cheaper than a set.
    pub(crate) fn warn(&mut self, warning: Warning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// Fills in every glyph's GDEF class, before any lookup runs.
    ///
    /// Done once per run rather than per lookup because it is a table read per
    /// glyph, and kept up to date by [`Runner::reclassify`] as substitutions
    /// change what the glyphs are.
    ///
    /// A face with no `GlyphClassDef` is left alone rather than reset to
    /// unclassified, and that is the whole subtlety here: positioning runs
    /// after substitution, and a ligature that `GSUB` declared for itself
    /// because the face said nothing must still be a ligature when `GPOS`
    /// looks at it.
    pub(crate) fn classify(&self, buffer: &mut Buffer) {
        let Some(gdef) = self.gdef else {
            return;
        };
        let has_classes = gdef.has_glyph_classes();
        for at in 0..buffer.len() {
            let Some(glyph) = buffer.glyph_id(at) else {
                continue;
            };
            let class = gdef.glyph_class(glyph);
            let attach = gdef.mark_attach_class(glyph);
            if let Some(props) = buffer.props_mut(at) {
                if has_classes {
                    props.class = class;
                }
                props.mark_attach = attach;
            }
        }
    }

    /// Updates one glyph's class after a substitution replaced it.
    ///
    /// `guess` is what the substitution itself implies — a ligature
    /// substitution produces a ligature — and it is used **only** where the
    /// face has no `GlyphClassDef`. Where it has one, the table wins even if
    /// it disagrees, because the face's own statement about its glyphs is the
    /// one its other lookups were written against.
    pub(crate) fn reclassify(&self, buffer: &mut Buffer, at: usize, guess: GlyphClass) {
        let Some(glyph) = buffer.glyph_id(at) else {
            return;
        };
        let has_classes = self.gdef.is_some_and(Gdef::has_glyph_classes);
        let class = if has_classes {
            self.gdef
                .map_or(GlyphClass::Unclassified, |gdef| gdef.glyph_class(glyph))
        } else {
            guess
        };
        let attach = self.gdef.map_or(0, |gdef| gdef.mark_attach_class(glyph));
        if let Some(props) = buffer.props_mut(at) {
            props.class = class;
            props.mark_attach = attach;
        }
    }

    /// The cluster a lookup starting at `at` is confined to, or zero for none.
    ///
    /// `GPOS` is never confined, and nor is a `GSUB` stage that applies its
    /// features to the whole run; [`crate::buffer::Buffer::set_syllable`]
    /// says why the two differ from the rest.
    fn per_syllable(&self, buffer: &Buffer, at: usize) -> u16 {
        #[cfg(test)]
        if crate::trace::unconfined() {
            return 0;
        }
        match self.table {
            Table::Gsub if self.confined => buffer.props(at).syllable,
            Table::Gsub | Table::Gpos => 0,
        }
    }

    /// The filter lookup `index` sees the buffer through.
    ///
    /// The feature mask comes off the runner rather than being passed in,
    /// because a contextual lookup that names another one hands the named
    /// lookup the *calling feature's* mask: the nested lookup is part of the
    /// same feature's work, and giving it a mask of its own would mean a
    /// chaining rule inside `init` could reach a glyph `init` was not turned
    /// on at.
    fn skipper(&self, lookup: &Lookup<'a>) -> Skipper<'a> {
        let mark_set = lookup
            .mark_filtering_set()
            .and_then(|set| self.gdef?.mark_glyph_set(usize::from(set)));
        Skipper {
            flags: lookup.flags(),
            mark_set,
            mask: self.mask,
            // Filled in per starting position by `run_forward`, because a
            // syllable is a property of where a lookup is applied and not of
            // the lookup.
            syllable: None,
        }
    }

    /// The real type of a subtable, following any extension indirection.
    ///
    /// Returns the resolved type and the subtable it actually points at, so
    /// every caller above this one is written as though extensions did not
    /// exist — which is the whole purpose of the extension mechanism and the
    /// only way to keep type 7 from appearing in eight other match arms.
    fn resolve(&mut self, lookup: &Lookup<'a>, index: usize, at: u16) -> Option<(u16, Bytes<'a>)> {
        let extension = match self.table {
            Table::Gsub => 7,
            Table::Gpos => 9,
        };
        let mut kind = lookup.kind();
        let mut data = lookup.subtable(index)?;
        let mut hops = 0u32;
        while kind == extension {
            if hops >= self.limits.max_extension_depth {
                self.warn(Warning::ExtensionTooDeep {
                    table: self.table,
                    lookup: at,
                });
                return None;
            }
            if data.u16(0)? != 1 {
                self.warn(Warning::MalformedSubtable {
                    table: self.table,
                    lookup: at,
                    subtable: u16::try_from(index).unwrap_or(u16::MAX),
                });
                return None;
            }
            let inner = data.u16(2)?;
            if inner == extension {
                // 14496-22: "the extensionLookupType field must not be set to
                // the extension lookup type itself". A font that does it
                // anyway is followed under the cap rather than trusted, so
                // the refusal is a warning and not a silent stop.
                self.warn(Warning::ExtensionChained {
                    table: self.table,
                    lookup: at,
                });
            }
            data = data.offset32(4)?;
            kind = inner;
            hops += 1;
        }
        Some((kind, data))
    }

    /// The type a lookup's subtables have, once extensions are followed.
    ///
    /// Read from the first subtable, because a lookup's subtables all share
    /// its type and an extension lookup states the real one in each of them.
    fn resolved_kind(&mut self, lookup: &Lookup<'a>, at: u16) -> u16 {
        self.resolve(lookup, 0, at)
            .map_or(lookup.kind(), |(k, _)| k)
    }

    /// Runs every lookup in `lookups`, in the order given, each restricted to
    /// the glyphs its own mask reaches.
    pub(crate) fn run(&mut self, buffer: &mut Buffer, lookups: &[(u16, u32)]) {
        self.classify(buffer);
        let covered = self.covered;
        for (place, (index, mask)) in lookups.iter().enumerate() {
            // An index this run's limits would make a probe warn past is set
            // aside, and the lookup probed as though it had none.
            let held = covered
                .get(place)
                .and_then(Option::as_deref)
                .filter(|held| held.answers_under(&self.limits));
            #[cfg(test)]
            let before = crate::trace::recording().then(|| buffer.clone());
            self.run_lookup(buffer, *index, *mask, held);
            #[cfg(test)]
            if let Some(before) = before {
                crate::trace::lookup_ran(self.table, *index, *mask, &before, buffer);
            }
        }
    }

    fn run_lookup(
        &mut self,
        buffer: &mut Buffer,
        index: u16,
        mask: u32,
        held: Option<&LookupIndex>,
    ) {
        let Some(lookup) = self.lookups.get(usize::from(index)) else {
            self.warn(Warning::MissingLookup {
                table: self.table,
                lookup: index,
            });
            return;
        };
        self.mask = mask;
        let skipper = self.skipper(&lookup);
        let kind = self.resolved_kind(&lookup, index);
        // Reverse chaining single substitution is the one lookup the
        // specification requires be applied from the end of the run: its
        // lookahead is the text it has not rewritten yet, so a forward pass
        // would match against glyphs it was about to replace.
        if self.table == Table::Gsub && kind == 8 {
            self.run_backward(buffer, &lookup, index, &skipper, held);
        } else {
            self.run_forward(buffer, &lookup, index, &skipper, held);
        }
    }

    /// Whether the index says no subtable could apply at `at`, having
    /// charged the budget what probing there would have cost: one operation
    /// per subtable and at least one, as [`Runner::apply_at`] charges a
    /// probe none of whose subtables applies.
    fn passes(
        &mut self,
        buffer: &Buffer,
        at: usize,
        index: u16,
        held: Option<&LookupIndex>,
    ) -> bool {
        let Some(held) = held else {
            return false;
        };
        let glyph = buffer.glyph_id(at);
        if glyph.is_some_and(|glyph| held.any.holds(glyph)) {
            return false;
        }
        self.charge(held.subtables.max(1));
        #[cfg(test)]
        probes::record(probes::Probe {
            table: self.table,
            lookup: index,
            glyph: glyph.unwrap_or(0),
            resolved: 0,
            probed: 0,
            holding: Some(0),
        });
        #[cfg(not(test))]
        let _ = index;
        true
    }

    fn run_forward(
        &mut self,
        buffer: &mut Buffer,
        lookup: &Lookup<'a>,
        index: u16,
        skipper: &Skipper<'a>,
        held: Option<&LookupIndex>,
    ) {
        let mut at = 0usize;
        while at < buffer.len() {
            if skipper.skips(buffer, at) || !skipper.reaches(buffer, at) {
                #[cfg(test)]
                crate::trace::passed_over(at, skipper.passed_because(buffer, at));
                at += 1;
                continue;
            }
            if self.passes(buffer, at, index, held) {
                at += 1;
                continue;
            }
            // The rule matches inside the cluster it started in. In a run with
            // no clusters — every Latin, Arabic and Han run — this is `None`
            // and restricts nothing.
            let syllable = self.per_syllable(buffer, at);
            let skipper = &skipper.in_syllable(syllable);
            let before = buffer.len();
            #[cfg(test)]
            let glyph = buffer.glyph_id(at).unwrap_or(0);
            let applied = self.apply_held(buffer, lookup, index, skipper, at, 0, held);
            #[cfg(test)]
            crate::trace::attempted(at, glyph, syllable, applied.is_some());
            match applied {
                // A subtable that neither consumed a glyph nor shortened the
                // buffer would leave the cursor where it was, and the same
                // subtable would match again for ever. The operation budget
                // would eventually stop it; this stops it immediately, and
                // without spending a caller's whole allowance on one glyph.
                Some(next) if next > at || buffer.len() < before => at = next.max(at),
                _ => at += 1,
            }
        }
    }

    fn run_backward(
        &mut self,
        buffer: &mut Buffer,
        lookup: &Lookup<'a>,
        index: u16,
        skipper: &Skipper<'a>,
        held: Option<&LookupIndex>,
    ) {
        let mut at = buffer.len();
        while at > 0 {
            at -= 1;
            if skipper.skips(buffer, at) || !skipper.reaches(buffer, at) {
                #[cfg(test)]
                crate::trace::passed_over(at, skipper.passed_because(buffer, at));
                continue;
            }
            if self.passes(buffer, at, index, held) {
                continue;
            }
            let syllable = self.per_syllable(buffer, at);
            let skipper = &skipper.in_syllable(syllable);
            #[cfg(test)]
            let glyph = buffer.glyph_id(at).unwrap_or(0);
            let applied = self.apply_held(buffer, lookup, index, skipper, at, 0, held);
            #[cfg(test)]
            crate::trace::attempted(at, glyph, syllable, applied.is_some());
            #[cfg(not(test))]
            let _ = applied;
        }
    }

    /// Charges one operation against [`Limits::max_operations`], and says
    /// whether there was one left to charge.
    ///
    /// The refusal is recorded once per table by [`Runner::warn`], so a
    /// ceiling reached a hundred thousand times is still one warning.
    fn spend(&mut self) -> bool {
        self.ops = self.ops.saturating_add(1);
        if self.ops > self.limits.max_operations {
            self.warn(Warning::OperationBudgetExceeded { table: self.table });
            return false;
        }
        true
    }

    /// Charges `count` operations at once: what `count` calls of
    /// [`Runner::spend`] leave behind, once the first refusal is reached —
    /// the count past the ceiling and its one warning.
    fn charge(&mut self, count: usize) {
        self.ops = self
            .ops
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
        if self.ops > self.limits.max_operations {
            self.warn(Warning::OperationBudgetExceeded { table: self.table });
        }
    }

    /// Tries every subtable of one lookup at one position.
    ///
    /// The first that applies wins and the rest are not consulted, which is
    /// the specification's rule and not an optimisation: subtables of one
    /// lookup are alternatives, and a face relies on the earlier ones
    /// shadowing the later.
    ///
    /// # What the operation budget counts, and why it is the subtable
    ///
    /// A lookup's `subTableCount` is a 16-bit field the *font* writes, and
    /// every subtable in it is resolved through any extension indirection and
    /// searched until one applies. So charging once for the attempt and then
    /// looping over the subtables would leave [`Limits::max_operations`]
    /// bounding how many times a lookup was *tried* while the font chose what
    /// each try cost — which is not a bound at all.
    ///
    /// `fuzz/corpus/shape/lookup-list-subtable-storm` is what that reads like
    /// from the outside: 1 158 bytes, three glyphs, no `GSUB` or `GPOS` in the
    /// face at all, and seconds of work out of a ceiling of half a million
    /// that was never reached. The unit is therefore the subtable, with the
    /// attempt's own charge paying for the first of them — so a lookup with
    /// one subtable, which is nearly every lookup in every shipped face,
    /// costs exactly what it has always cost and no well-formed face's
    /// accounting moves.
    pub(crate) fn apply_at(
        &mut self,
        buffer: &mut Buffer,
        lookup: &Lookup<'a>,
        index: u16,
        skipper: &Skipper<'a>,
        at: usize,
        depth: u32,
    ) -> Option<usize> {
        self.apply_held(buffer, lookup, index, skipper, at, depth, None)
    }

    /// [`Runner::apply_at`], resolving and probing only the subtables `held`
    /// says could apply at the glyph here, with the same charge before each
    /// subtable either way.
    ///
    /// A subtable that does not apply leaves the buffer as it was, so the
    /// glyph asked about is the same one for every subtable of the lookup.
    #[allow(clippy::too_many_arguments)]
    fn apply_held(
        &mut self,
        buffer: &mut Buffer,
        lookup: &Lookup<'a>,
        index: u16,
        skipper: &Skipper<'a>,
        at: usize,
        depth: u32,
        held: Option<&LookupIndex>,
    ) -> Option<usize> {
        let glyph = buffer.glyph_id(at);
        // What this attempt resolves and probes, recorded when it returns,
        // for the tests that hold a probe to the index.
        #[cfg(test)]
        let mut recorder = probes::Recorder {
            probe: probes::Probe {
                table: self.table,
                lookup: index,
                glyph: glyph.unwrap_or(0),
                resolved: 0,
                probed: 0,
                holding: held.zip(glyph).map(|(held, glyph)| held.holding(glyph)),
            },
            depth,
        };
        if !self.spend() {
            return None;
        }
        for sub in 0..lookup.len() {
            if sub > 0 && !self.spend() {
                return None;
            }
            if let (Some(held), Some(glyph)) = (held, glyph) {
                if !held.subtable_holds(sub, glyph) {
                    continue;
                }
            }
            #[cfg(test)]
            {
                recorder.probe.resolved += 1;
            }
            let Some((kind, data)) = self.resolve(lookup, sub, index) else {
                continue;
            };
            #[cfg(test)]
            {
                recorder.probe.probed += 1;
            }
            let applied = match self.table {
                Table::Gsub => crate::gsub::apply(self, buffer, kind, data, skipper, at, depth),
                Table::Gpos => crate::gpos::apply(self, buffer, kind, data, skipper, at, depth),
            };
            match applied {
                Applied::Yes(next) => return Some(next),
                Applied::No => {}
                Applied::Unsupported => {
                    self.warn(Warning::UnknownLookupType {
                        table: self.table,
                        lookup: index,
                        kind,
                    });
                }
            }
        }
        None
    }

    /// Runs lookup `index` at one position, from inside a contextual rule.
    pub(crate) fn recurse(
        &mut self,
        buffer: &mut Buffer,
        index: u16,
        at: usize,
        depth: u32,
    ) -> bool {
        if depth >= self.limits.max_nesting_depth {
            self.warn(Warning::NestingTooDeep {
                table: self.table,
                lookup: index,
            });
            return false;
        }
        let Some(lookup) = self.lookups.get(usize::from(index)) else {
            self.warn(Warning::MissingLookup {
                table: self.table,
                lookup: index,
            });
            return false;
        };
        let skipper = self.skipper(&lookup);
        self.apply_at(buffer, &lookup, index, &skipper, at, depth + 1)
            .is_some()
    }

    /// Whether the buffer may still grow by `by` glyphs.
    pub(crate) fn may_grow(&mut self, buffer: &Buffer, by: usize) -> bool {
        if buffer.len().saturating_add(by) > self.limits.max_glyphs {
            self.warn(Warning::GlyphBudgetExceeded { table: self.table });
            return false;
        }
        true
    }

    // --- matching ---------------------------------------------------------

    /// The positions of `count` glyphs starting at `at`, each satisfying
    /// `wanted`, stepping over what the lookup cannot see.
    ///
    /// `wanted(n, glyph)` is asked about the *n*th glyph of the sequence,
    /// counting the one at `at` as zero — which is what lets one matcher
    /// serve a ligature's component list, a contextual rule's glyph list and
    /// the same rule's class list without any of them knowing about the
    /// others.
    pub(crate) fn match_input(
        &mut self,
        buffer: &Buffer,
        skipper: &Skipper<'a>,
        at: usize,
        count: usize,
        wanted: impl Fn(usize, u16) -> bool,
    ) -> Option<Matched> {
        #[cfg(test)]
        use crate::trace::{Cause, Part};
        if count > self.limits.max_context_length {
            self.warn(Warning::ContextTooLong { table: self.table });
            #[cfg(test)]
            crate::trace::missed(Part::Input, 0, Some(at), None, Cause::TooLong);
            return None;
        }
        let mut positions = Vec::with_capacity(count);
        let mut cursor = at;
        for step in 0..count {
            if step > 0 {
                let Some(next) = self.step_forward(buffer, skipper, cursor) else {
                    #[cfg(test)]
                    crate::trace::missed(Part::Input, step, None, None, Cause::End);
                    return None;
                };
                cursor = next;
            }
            let glyph = buffer.glyph_id(cursor)?;
            if !wanted(step, glyph) {
                #[cfg(test)]
                crate::trace::missed(Part::Input, step, Some(cursor), Some(glyph), Cause::Glyph);
                return None;
            }
            if !skipper.reaches(buffer, cursor) {
                #[cfg(test)]
                crate::trace::missed(
                    Part::Input,
                    step,
                    Some(cursor),
                    Some(glyph),
                    skipper.unreached(buffer, cursor),
                );
                return None;
            }
            positions.push(cursor);
        }
        let end = positions.last().map_or(at, |last| last.saturating_add(1));
        Some(Matched { positions, end })
    }

    /// The positions of `count` glyphs *before* `at`, nearest first.
    pub(crate) fn match_backtrack(
        &mut self,
        buffer: &Buffer,
        skipper: &Skipper<'a>,
        at: usize,
        count: usize,
        wanted: impl Fn(usize, u16) -> bool,
    ) -> Option<Vec<usize>> {
        if count == 0 {
            return Some(Vec::new());
        }
        if count > self.limits.max_context_length {
            self.warn(Warning::ContextTooLong { table: self.table });
            return None;
        }
        // A rule's context is the glyphs around its input, whatever features
        // they were turned on for. See `Skipper::mask`.
        let skipper = &skipper.over_context();
        let mut positions = Vec::with_capacity(count);
        let mut cursor = at;
        for step in 0..count {
            let Some(next) = self.step_backward(buffer, skipper, cursor) else {
                #[cfg(test)]
                crate::trace::missed(
                    crate::trace::Part::Backtrack,
                    step,
                    None,
                    None,
                    crate::trace::Cause::End,
                );
                return None;
            };
            cursor = next;
            let glyph = buffer.glyph_id(cursor)?;
            if !wanted(step, glyph) {
                #[cfg(test)]
                crate::trace::missed(
                    crate::trace::Part::Backtrack,
                    step,
                    Some(cursor),
                    Some(glyph),
                    crate::trace::Cause::Glyph,
                );
                return None;
            }
            positions.push(cursor);
        }
        Some(positions)
    }

    /// The positions of `count` glyphs from `from` onwards, in order.
    pub(crate) fn match_lookahead(
        &mut self,
        buffer: &Buffer,
        skipper: &Skipper<'a>,
        from: usize,
        count: usize,
        wanted: impl Fn(usize, u16) -> bool,
    ) -> Option<Vec<usize>> {
        if count == 0 {
            return Some(Vec::new());
        }
        if count > self.limits.max_context_length {
            self.warn(Warning::ContextTooLong { table: self.table });
            return None;
        }
        // As in `match_backtrack`: the context is matched with the feature
        // mask lifted, and only the input sequence is restricted by it.
        let skipper = &skipper.over_context();
        let mut positions = Vec::with_capacity(count);
        // `from` is one past the last matched glyph, so the first lookahead
        // candidate is `from` itself — but it may be a glyph this lookup
        // steps over, which is why the search starts one behind it and moves
        // forward rather than reading `from` directly.
        let mut cursor = from.checked_sub(1)?;
        for step in 0..count {
            let Some(next) = self.step_forward(buffer, skipper, cursor) else {
                #[cfg(test)]
                crate::trace::missed(
                    crate::trace::Part::Lookahead,
                    step,
                    None,
                    None,
                    crate::trace::Cause::End,
                );
                return None;
            };
            cursor = next;
            let glyph = buffer.glyph_id(cursor)?;
            if !wanted(step, glyph) {
                #[cfg(test)]
                crate::trace::missed(
                    crate::trace::Part::Lookahead,
                    step,
                    Some(cursor),
                    Some(glyph),
                    crate::trace::Cause::Glyph,
                );
                return None;
            }
            positions.push(cursor);
        }
        Some(positions)
    }

    /// The next position after `at` this lookup can see.
    pub(crate) fn step_forward(
        &mut self,
        buffer: &Buffer,
        skipper: &Skipper<'a>,
        at: usize,
    ) -> Option<usize> {
        let mut cursor = at.checked_add(1)?;
        while cursor < buffer.len() {
            if !skipper.skips(buffer, cursor) {
                return Some(cursor);
            }
            cursor = cursor.checked_add(1)?;
        }
        None
    }

    /// The previous position before `at` this lookup can see.
    pub(crate) fn step_backward(
        &mut self,
        buffer: &Buffer,
        skipper: &Skipper<'a>,
        at: usize,
    ) -> Option<usize> {
        let mut cursor = at;
        while cursor > 0 {
            cursor -= 1;
            if !skipper.skips(buffer, cursor) {
                return Some(cursor);
            }
        }
        None
    }

    /// The nearest preceding glyph that is not a mark.
    pub(crate) fn previous_base(&mut self, buffer: &Buffer, at: usize) -> Option<usize> {
        let skipper = Skipper::marks_only_ignored();
        self.step_backward(buffer, &skipper, at)
    }

    /// The nearest preceding glyph, marks included, under this lookup's
    /// mark filters.
    pub(crate) fn previous_mark(
        &mut self,
        buffer: &Buffer,
        skipper: &Skipper<'a>,
        at: usize,
    ) -> Option<usize> {
        let skipper = skipper.without_class_filters();
        self.step_backward(buffer, &skipper, at)
    }

    // --- contextual lookups ----------------------------------------------

    /// Applies a contextual rule's nested lookups, keeping the matched
    /// positions correct as the buffer changes underneath them.
    ///
    /// # Why this is not simply a loop
    ///
    /// A contextual rule is a list of `(sequenceIndex, lookupListIndex)`
    /// pairs: "at the third glyph of what you just matched, run lookup 9". If
    /// lookup 9 is a ligature, the buffer is now shorter and every position
    /// after the third is wrong. If it is a multiple substitution, the buffer
    /// is longer and the sequence has *gained* members that the rule's later
    /// pairs may name.
    ///
    /// So each nested application is followed by a repair: the positions
    /// after the one that changed move by the difference, new positions are
    /// invented for glyphs a substitution added, and positions for glyphs it
    /// removed are dropped. Nothing here can be skipped — a rule whose second
    /// lookup ligates and whose third positions the result is the ordinary
    /// shape of an Arabic feature file, not an edge case.
    ///
    /// Returns where the lookup resumes: one past the matched sequence, moved
    /// by whatever the nested lookups did to it.
    pub(crate) fn apply_records(
        &mut self,
        buffer: &mut Buffer,
        matched: &Matched,
        records: impl Iterator<Item = (u16, u16)>,
        depth: u32,
    ) -> usize {
        let mut positions = matched.positions.clone();
        let mut end = matched.end;
        for (sequence_index, lookup_index) in records {
            let index = usize::from(sequence_index);
            let Some(at) = positions.get(index).copied() else {
                continue;
            };
            if at >= buffer.len() {
                continue;
            }
            let before = buffer.len();
            if !self.recurse(buffer, lookup_index, at, depth) {
                #[cfg(test)]
                crate::trace::nested_missed(lookup_index, at);
                continue;
            }
            let after = buffer.len();
            if after == before {
                continue;
            }
            if after > before {
                let added = after - before;
                if positions.len().saturating_add(added) > self.limits.max_context_length {
                    self.warn(Warning::ContextTooLong { table: self.table });
                    break;
                }
                end = end.saturating_add(added);
                for position in positions.iter_mut().skip(index + 1) {
                    *position = position.saturating_add(added);
                }
                let inserted: Vec<usize> = (1..=added).map(|n| at.saturating_add(n)).collect();
                let after_index = index + 1;
                positions.splice(after_index..after_index, inserted);
            } else {
                let removed = (before - after).min(positions.len().saturating_sub(index + 1));
                end = end.saturating_sub(before - after).max(at);
                positions.drain(index + 1..index + 1 + removed);
                for position in positions.iter_mut().skip(index + 1) {
                    *position = position.saturating_sub(before - after);
                }
            }
        }
        end.min(buffer.len())
    }

    /// A contextual or chaining-contextual subtable's `SequenceLookupRecord`
    /// array: `count` pairs of `uint16` starting at `offset`.
    pub(crate) fn records(
        data: Bytes<'a>,
        offset: usize,
        count: usize,
    ) -> impl Iterator<Item = (u16, u16)> + 'a {
        (0..count).filter_map(move |n| {
            let at = offset.checked_add(n.checked_mul(4)?)?;
            Some((data.u16(at)?, data.u16(at.checked_add(2)?)?))
        })
    }
}

/// What one subtable did.
pub(crate) enum Applied {
    /// It applied; the lookup resumes at this position.
    Yes(usize),
    /// It did not match here.
    No,
    /// The subtable is of a type this crate does not implement, which is a
    /// different fact from "did not match" and is reported as one.
    Unsupported,
}

/// The shared shape of contextual lookups: `GSUB` type 5 and `GPOS` type 7.
///
/// Three formats, differing only in how the input sequence is written — as
/// glyph indices, as classes, or as coverage tables — and identical in
/// everything they do afterwards. `gsub.rs` and `gpos.rs` both call straight
/// into this.
pub(crate) fn apply_context<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    depth: u32,
) -> Applied {
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    match data.u16(0) {
        Some(1) => context_format1(runner, buffer, data, skipper, at, glyph, depth),
        Some(2) => context_format2(runner, buffer, data, skipper, at, glyph, depth),
        Some(3) => context_format3(runner, buffer, data, skipper, at, depth),
        _ => Applied::No,
    }
}

/// Format 1: rule sets keyed by the first glyph, each rule a list of glyphs.
fn context_format1<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    glyph: u16,
    depth: u32,
) -> Applied {
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let Some(set) = rule_set(data, 4, index) else {
        return Applied::No;
    };
    let count = usize::from(set.u16(0).unwrap_or(0));
    for n in 0..count {
        let Some(rule) = set.offset16_at(2, n) else {
            continue;
        };
        let Some(glyphs) = rule.u16(0).map(usize::from) else {
            continue;
        };
        let Some(records) = rule.u16(2).map(usize::from) else {
            continue;
        };
        if glyphs == 0 {
            continue;
        }
        // `inputSequence` holds glyphs 1..n: the first is the coverage.
        let matched = runner.match_input(buffer, skipper, at, glyphs, |step, found| {
            step == 0 || rule.u16_at(4, step - 1) == Some(found)
        });
        if let Some(matched) = matched {
            let offset = 4usize.saturating_add(glyphs.saturating_sub(1).saturating_mul(2));
            let records = Runner::records(rule, offset, records);
            return Applied::Yes(runner.apply_records(buffer, &matched, records, depth));
        }
    }
    Applied::No
}

/// Format 2: rule sets keyed by the first glyph's *class*.
fn context_format2<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    glyph: u16,
    depth: u32,
) -> Applied {
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    if !coverage.covers(glyph) {
        return Applied::No;
    }
    let Some(classes) = data.offset16(4).map(ClassDef::new) else {
        return Applied::No;
    };
    let class = classes.class_of(glyph);
    let Some(set) = rule_set(data, 6, class) else {
        return Applied::No;
    };
    let count = usize::from(set.u16(0).unwrap_or(0));
    for n in 0..count {
        let Some(rule) = set.offset16_at(2, n) else {
            continue;
        };
        let Some(glyphs) = rule.u16(0).map(usize::from) else {
            continue;
        };
        let Some(records) = rule.u16(2).map(usize::from) else {
            continue;
        };
        if glyphs == 0 {
            continue;
        }
        let matched = runner.match_input(buffer, skipper, at, glyphs, |step, found| {
            step == 0 || rule.u16_at(4, step - 1) == Some(classes.class_of(found))
        });
        if let Some(matched) = matched {
            let offset = 4usize.saturating_add(glyphs.saturating_sub(1).saturating_mul(2));
            let records = Runner::records(rule, offset, records);
            return Applied::Yes(runner.apply_records(buffer, &matched, records, depth));
        }
    }
    Applied::No
}

/// Format 3: one rule, written as a coverage table per input position.
fn context_format3<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    depth: u32,
) -> Applied {
    let Some(glyphs) = data.u16(2).map(usize::from) else {
        return Applied::No;
    };
    let Some(records) = data.u16(4).map(usize::from) else {
        return Applied::No;
    };
    if glyphs == 0 {
        return Applied::No;
    }
    let matched = runner.match_input(buffer, skipper, at, glyphs, |step, found| {
        data.offset16_at(6, step)
            .map(Coverage::new)
            .is_some_and(|coverage| coverage.covers(found))
    });
    let Some(matched) = matched else {
        return Applied::No;
    };
    let offset = 6usize.saturating_add(glyphs.saturating_mul(2));
    let records = Runner::records(data, offset, records);
    Applied::Yes(runner.apply_records(buffer, &matched, records, depth))
}

/// The shared shape of chaining contextual lookups: `GSUB` type 6 and `GPOS`
/// type 8.
pub(crate) fn apply_chain_context<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    depth: u32,
) -> Applied {
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    match data.u16(0) {
        Some(1) => chain_format1(runner, buffer, data, skipper, at, glyph, depth),
        Some(2) => chain_format2(runner, buffer, data, skipper, at, glyph, depth),
        Some(3) => chain_format3(runner, buffer, data, skipper, at, depth),
        _ => Applied::No,
    }
}

/// One chaining rule's three sequences, read from a rule table whose
/// backtrack, input and lookahead counts are written inline one after
/// another.
///
/// The layout is the reason this is a function rather than three field reads:
/// every count is followed immediately by its own array, so the offset of the
/// input array depends on the backtrack count, and the offset of the
/// lookahead count depends on both. A reader that hard-coded any of them
/// would work on every font whose backtrack happened to be the length it
/// assumed.
struct ChainRule<'a> {
    data: Bytes<'a>,
    backtrack: usize,
    backtrack_at: usize,
    input: usize,
    input_at: usize,
    lookahead: usize,
    lookahead_at: usize,
    records: usize,
    records_at: usize,
}

impl<'a> ChainRule<'a> {
    fn parse(data: Bytes<'a>) -> Option<Self> {
        let backtrack = usize::from(data.u16(0)?);
        let backtrack_at = 2usize;
        let input_count_at = backtrack_at.checked_add(backtrack.checked_mul(2)?)?;
        let input = usize::from(data.u16(input_count_at)?);
        let input_at = input_count_at.checked_add(2)?;
        // The input array holds glyphs 1..n: the first is the coverage table.
        let lookahead_count_at = input_at.checked_add(input.checked_sub(1)?.checked_mul(2)?)?;
        let lookahead = usize::from(data.u16(lookahead_count_at)?);
        let lookahead_at = lookahead_count_at.checked_add(2)?;
        let records_count_at = lookahead_at.checked_add(lookahead.checked_mul(2)?)?;
        let records = usize::from(data.u16(records_count_at)?);
        let records_at = records_count_at.checked_add(2)?;
        Some(Self {
            data,
            backtrack,
            backtrack_at,
            input,
            input_at,
            lookahead,
            lookahead_at,
            records,
            records_at,
        })
    }
}

/// Format 1: rule sets keyed by the first input glyph, everything by glyph
/// index.
fn chain_format1<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    glyph: u16,
    depth: u32,
) -> Applied {
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let Some(set) = rule_set(data, 4, index) else {
        return Applied::No;
    };
    let count = usize::from(set.u16(0).unwrap_or(0));
    for n in 0..count {
        let Some(rule) = set.offset16_at(2, n).and_then(ChainRule::parse) else {
            continue;
        };
        let matched = chain_match(
            runner,
            buffer,
            skipper,
            at,
            &rule,
            (
                |step, found| rule.data.u16_at(rule.backtrack_at, step) == Some(found),
                |step, found| step == 0 || rule.data.u16_at(rule.input_at, step - 1) == Some(found),
                |step, found| rule.data.u16_at(rule.lookahead_at, step) == Some(found),
            ),
        );
        if let Some(matched) = matched {
            let records = Runner::records(rule.data, rule.records_at, rule.records);
            return Applied::Yes(runner.apply_records(buffer, &matched, records, depth));
        }
    }
    Applied::No
}

/// Format 2: the same, with three class definitions instead of glyph indices.
fn chain_format2<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    glyph: u16,
    depth: u32,
) -> Applied {
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    if !coverage.covers(glyph) {
        return Applied::No;
    }
    // A missing class definition is not a malformed table: the specification
    // lets a face omit any of the three, and an omitted one puts every glyph
    // in class 0. `ClassDef::class_of` already answers zero for a glyph it
    // does not know, so the fallback is the same code path.
    let backtrack_classes = data.offset16(4).map(ClassDef::new);
    let Some(input_classes) = data.offset16(6).map(ClassDef::new) else {
        return Applied::No;
    };
    let lookahead_classes = data.offset16(8).map(ClassDef::new);
    let class_of = |classes: Option<ClassDef<'a>>, glyph: u16| {
        classes.map_or(0, |classes| classes.class_of(glyph))
    };
    let Some(set) = rule_set(data, 10, input_classes.class_of(glyph)) else {
        return Applied::No;
    };
    let count = usize::from(set.u16(0).unwrap_or(0));
    for n in 0..count {
        let Some(rule) = set.offset16_at(2, n).and_then(ChainRule::parse) else {
            continue;
        };
        let matched = chain_match(
            runner,
            buffer,
            skipper,
            at,
            &rule,
            (
                |step, found| {
                    rule.data.u16_at(rule.backtrack_at, step)
                        == Some(class_of(backtrack_classes, found))
                },
                |step, found| {
                    step == 0
                        || rule.data.u16_at(rule.input_at, step - 1)
                            == Some(input_classes.class_of(found))
                },
                |step, found| {
                    rule.data.u16_at(rule.lookahead_at, step)
                        == Some(class_of(lookahead_classes, found))
                },
            ),
        );
        if let Some(matched) = matched {
            let records = Runner::records(rule.data, rule.records_at, rule.records);
            return Applied::Yes(runner.apply_records(buffer, &matched, records, depth));
        }
    }
    Applied::No
}

/// Format 3: one rule, three arrays of coverage tables.
fn chain_format3<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    depth: u32,
) -> Applied {
    let Some(backtrack) = data.u16(2).map(usize::from) else {
        return Applied::No;
    };
    let backtrack_at = 4usize;
    let Some(input_count_at) = backtrack_at.checked_add(backtrack.saturating_mul(2)) else {
        return Applied::No;
    };
    let Some(input) = data.u16(input_count_at).map(usize::from) else {
        return Applied::No;
    };
    let input_at = input_count_at + 2;
    let Some(lookahead_count_at) = input_at.checked_add(input.saturating_mul(2)) else {
        return Applied::No;
    };
    let Some(lookahead) = data.u16(lookahead_count_at).map(usize::from) else {
        return Applied::No;
    };
    let lookahead_at = lookahead_count_at + 2;
    let Some(records_count_at) = lookahead_at.checked_add(lookahead.saturating_mul(2)) else {
        return Applied::No;
    };
    let Some(records) = data.u16(records_count_at).map(usize::from) else {
        return Applied::No;
    };
    let records_at = records_count_at + 2;
    if input == 0 {
        return Applied::No;
    }
    let covers = |base: usize, step: usize, glyph: u16| {
        data.offset16_at(base, step)
            .map(Coverage::new)
            .is_some_and(|coverage| coverage.covers(glyph))
    };

    let Some(matched) = runner.match_input(buffer, skipper, at, input, |step, found| {
        covers(input_at, step, found)
    }) else {
        return Applied::No;
    };
    if runner
        .match_backtrack(buffer, skipper, at, backtrack, |step, found| {
            covers(backtrack_at, step, found)
        })
        .is_none()
    {
        return Applied::No;
    }
    if runner
        .match_lookahead(buffer, skipper, matched.end, lookahead, |step, found| {
            covers(lookahead_at, step, found)
        })
        .is_none()
    {
        return Applied::No;
    }
    let records = Runner::records(data, records_at, records);
    Applied::Yes(runner.apply_records(buffer, &matched, records, depth))
}

/// Matches a chaining rule's input first, then its context.
///
/// Input before context on purpose: the input is the part that is *always*
/// present and is what a rule is keyed by, so failing it is the cheapest way
/// to reject the overwhelming majority of positions. The backtrack and
/// lookahead each cost a walk over glyphs the lookup may not even be able to
/// see.
fn chain_match<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &Buffer,
    skipper: &Skipper<'a>,
    at: usize,
    rule: &ChainRule<'a>,
    predicates: (
        impl Fn(usize, u16) -> bool,
        impl Fn(usize, u16) -> bool,
        impl Fn(usize, u16) -> bool,
    ),
) -> Option<Matched> {
    let (backtrack, input, lookahead) = predicates;
    if rule.input == 0 {
        return None;
    }
    let matched = runner.match_input(buffer, skipper, at, rule.input, input)?;
    runner.match_backtrack(buffer, skipper, at, rule.backtrack, backtrack)?;
    runner.match_lookahead(buffer, skipper, matched.end, rule.lookahead, lookahead)?;
    Some(matched)
}

/// Rule set `index` of an array of `Offset16` at `at`, preceded by its count.
fn rule_set(data: Bytes<'_>, at: usize, index: u16) -> Option<Bytes<'_>> {
    let count = usize::from(data.u16(at)?);
    let index = usize::from(index);
    if index >= count {
        return None;
    }
    data.offset16_at(at.checked_add(2)?, index)
}

/// What the tests read of the runner's probing: an entry for each lookup
/// tried at a position from the top of a run, kept per thread so tests run
/// side by side see their own.
#[cfg(test)]
pub(crate) mod probes {
    use std::cell::RefCell;

    use crate::Table;

    /// One lookup tried, or passed, at one position.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Probe {
        pub(crate) table: Table,
        pub(crate) lookup: u16,
        pub(crate) glyph: u16,
        /// Subtables the attempt resolved.
        pub(crate) resolved: usize,
        /// Subtables it probed: resolved and handed the position, which
        /// reads their coverage first.
        pub(crate) probed: usize,
        /// For a lookup with an index, how many of its subtables' sets hold
        /// the glyph; `None` for a lookup probed without one.
        pub(crate) holding: Option<usize>,
    }

    thread_local! {
        static LOG: RefCell<Vec<Probe>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn record(probe: Probe) {
        LOG.with(|log| log.borrow_mut().push(probe));
    }

    /// Every entry recorded on this thread since the last call.
    pub(crate) fn take() -> Vec<Probe> {
        LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
    }

    /// Records its probe when the attempt returns, by whichever return, if
    /// the attempt was a run's own rather than a nested lookup's.
    pub(crate) struct Recorder {
        pub(crate) probe: Probe,
        pub(crate) depth: u32,
    }

    impl Drop for Recorder {
        fn drop(&mut self) {
            if self.depth == 0 {
                record(self.probe);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tinker_pdf_font::Sfnt;

    use super::{probes, LookupIndex, Runner};
    use crate::bidi::BaseDirection;
    use crate::buffer::{Buffer, ShapedGlyph};
    use crate::common::{LookupList, Tag};
    use crate::limits::Limits;
    use crate::read::Bytes;
    use crate::shape::ShapedRun;
    use crate::{Plans, Shaper, Table, Warning};

    // --- a small table assembler -------------------------------------------

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A header with `slots[i]` patched to the offset of `parts[i]`, the
    /// parts appended after it in order.
    fn assemble(mut head: Vec<u8>, slots: &[usize], parts: &[Vec<u8>]) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        for (slot, part) in slots.iter().zip(parts) {
            let offset = u16::try_from(head.len() + body.len()).expect("a table under 64 KB");
            head[*slot..*slot + 2].copy_from_slice(&offset.to_be_bytes());
            body.extend_from_slice(part);
        }
        head.extend(body);
        head
    }

    /// Coverage format 1: the glyphs, in the order given.
    fn coverage(glyphs: &[u16]) -> Vec<u8> {
        let mut out = u16s(&[1, glyphs.len() as u16]);
        out.extend(u16s(glyphs));
        out
    }

    /// Coverage format 2: `(first, last, startCoverageIndex)`, in the order
    /// given, sorted or not.
    fn coverage_ranges(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut out = u16s(&[2, ranges.len() as u16]);
        for (first, last, index) in ranges {
            out.extend(u16s(&[*first, *last, *index]));
        }
        out
    }

    /// ClassDef format 2: `(first, last, class)` ranges.
    fn class_def(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut out = u16s(&[2, ranges.len() as u16]);
        for (first, last, class) in ranges {
            out.extend(u16s(&[*first, *last, *class]));
        }
        out
    }

    fn lookup(kind: u16, subtables: &[Vec<u8>]) -> Vec<u8> {
        let mut head = u16s(&[kind, 0, subtables.len() as u16]);
        head.extend(vec![0u8; subtables.len() * 2]);
        let slots: Vec<usize> = (0..subtables.len()).map(|i| 6 + i * 2).collect();
        assemble(head, &slots, subtables)
    }

    fn lookup_list(lookups: &[Vec<u8>]) -> Vec<u8> {
        let mut head = u16s(&[lookups.len() as u16]);
        head.extend(vec![0u8; lookups.len() * 2]);
        let slots: Vec<usize> = (0..lookups.len()).map(|i| 2 + i * 2).collect();
        assemble(head, &slots, lookups)
    }

    /// `SingleSubstFormat2` over `glyphs`, each replaced by its `to`.
    fn single2(glyphs: &[u16], to: &[u16]) -> Vec<u8> {
        let mut head = u16s(&[2, 0, to.len() as u16]);
        head.extend(u16s(to));
        assemble(head, &[2], &[coverage(glyphs)])
    }

    /// `SingleSubstFormat1`: `delta` added to every glyph `coverage` holds.
    fn single1(coverage: Vec<u8>, delta: i16) -> Vec<u8> {
        assemble(u16s(&[1, 0, delta as u16]), &[2], &[coverage])
    }

    /// `MultipleSubstFormat1`, or a format no reader takes.
    fn multiple(format: u16, from: u16, to: &[u16]) -> Vec<u8> {
        let mut sequence = u16s(&[to.len() as u16]);
        sequence.extend(u16s(to));
        assemble(
            u16s(&[format, 0, 1, 0]),
            &[2, 6],
            &[coverage(&[from]), sequence],
        )
    }

    fn alternate(from: u16, alternates: &[u16]) -> Vec<u8> {
        let mut set = u16s(&[alternates.len() as u16]);
        set.extend(u16s(alternates));
        assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[from]), set])
    }

    fn ligature(first: u16, rest: &[u16], out: u16) -> Vec<u8> {
        let mut entry = u16s(&[out, rest.len() as u16 + 1]);
        entry.extend(u16s(rest));
        let set = assemble(u16s(&[1, 0]), &[2], &[entry]);
        assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[first]), set])
    }

    /// A format 3 context rule: a coverage per input glyph.
    fn context3(positions: &[Vec<u16>], records: &[(u16, u16)]) -> Vec<u8> {
        let mut head = u16s(&[3, positions.len() as u16, records.len() as u16]);
        head.extend(vec![0u8; positions.len() * 2]);
        for (sequence, lookup) in records {
            head.extend(u16s(&[*sequence, *lookup]));
        }
        let slots: Vec<usize> = (0..positions.len()).map(|i| 6 + i * 2).collect();
        let parts: Vec<Vec<u8>> = positions.iter().map(|glyphs| coverage(glyphs)).collect();
        assemble(head, &slots, &parts)
    }

    /// A format 3 chained context rule.
    fn chain3(
        backtrack: &[Vec<u16>],
        input: &[Vec<u16>],
        lookahead: &[Vec<u16>],
        records: &[(u16, u16)],
    ) -> Vec<u8> {
        let mut head = u16s(&[3]);
        let mut slots = Vec::new();
        let mut parts = Vec::new();
        for sequence in [backtrack, input, lookahead] {
            head.extend(u16s(&[sequence.len() as u16]));
            for glyphs in sequence {
                slots.push(head.len());
                head.extend(u16s(&[0]));
                parts.push(coverage(glyphs));
            }
        }
        head.extend(u16s(&[records.len() as u16]));
        for (sequence, lookup) in records {
            head.extend(u16s(&[*sequence, *lookup]));
        }
        assemble(head, &slots, &parts)
    }

    /// `ReverseChainSingleSubstFormat1` with no backtrack.
    fn reverse(map: &[(u16, u16)], lookahead: &[Vec<u16>]) -> Vec<u8> {
        let glyphs: Vec<u16> = map.iter().map(|(from, _)| *from).collect();
        let subs: Vec<u16> = map.iter().map(|(_, to)| *to).collect();
        let mut head = u16s(&[1, 0, 0, lookahead.len() as u16]);
        head.extend(vec![0u8; lookahead.len() * 2]);
        head.extend(u16s(&[subs.len() as u16]));
        head.extend(u16s(&subs));
        let mut slots = vec![2usize];
        slots.extend((0..lookahead.len()).map(|i| 8 + i * 2));
        let mut parts = vec![coverage(&glyphs)];
        parts.extend(lookahead.iter().map(|glyphs| coverage(glyphs)));
        assemble(head, &slots, &parts)
    }

    /// An extension subtable of `format` declaring `kind`, pointing at
    /// `inner`.
    fn extension(format: u16, kind: u16, inner: Vec<u8>) -> Vec<u8> {
        let mut out = u16s(&[format, kind]);
        out.extend(8u32.to_be_bytes());
        out.extend(inner);
        out
    }

    /// An `Anchor` of format 1.
    fn anchor(x: i16, y: i16) -> Vec<u8> {
        u16s(&[1, x as u16, y as u16])
    }

    // --- the tables ---------------------------------------------------------

    /// A `GSUB` lookup list reaching every kind, every format the gate reads
    /// and every way a probe can warn whatever the glyph, over glyphs 0 to 9
    /// and 65 535.
    fn gsub_list() -> Vec<u8> {
        let mut unreadable = u16s(&[1, 0, 1]);
        unreadable.extend(u16s(&[0xFFF0]));
        lookup_list(&[
            // 0: single, format 2.
            lookup(1, &[single2(&[1, 2], &[3, 4])]),
            // 1: single, format 1, over ranges that are unsorted and overlap,
            // so the search finds less than they list.
            lookup(
                1,
                &[single1(
                    coverage_ranges(&[(5, 6, 0), (2, 3, 2), (5, 5, 1)]),
                    10,
                )],
            ),
            // 2 and 3: multiple, and a format nothing reads.
            lookup(2, &[multiple(1, 3, &[7, 8])]),
            lookup(2, &[multiple(2, 4, &[7, 8])]),
            // 4: alternate.
            lookup(3, &[alternate(4, &[9, 6])]),
            // 5: ligature of two.
            lookup(4, &[ligature(7, &[8], 9)]),
            // 6: context, format 3, two inputs, running lookup 0.
            lookup(5, &[context3(&[vec![1, 3], vec![2, 4]], &[(0, 0)])]),
            // 7: chained context, format 3, running lookup 4.
            lookup(6, &[chain3(&[vec![3]], &[vec![4]], &[vec![5]], &[(0, 4)])]),
            // 8: reverse chaining.
            lookup(8, &[reverse(&[(6, 1)], &[vec![2]])]),
            // 9: one extension hop to a single substitution.
            lookup(7, &[extension(1, 1, single2(&[9], &[5]))]),
            // 10: an extension naming the extension type.
            lookup(7, &[extension(1, 7, extension(1, 1, single2(&[9], &[5])))]),
            // 11: an extension of a format other than 1.
            lookup(7, &[extension(2, 1, single2(&[9], &[5]))]),
            // 12: a type GSUB does not have.
            lookup(9, &[single2(&[1], &[2])]),
            // 13: three subtables over glyph 5: a ligature it may fail, then
            // two single substitutions.
            lookup(
                4,
                &[
                    ligature(5, &[6], 0),
                    single2(&[5], &[2]),
                    single2(&[5, 65535], &[1, 0]),
                ],
            ),
            // 14: no subtables at all.
            lookup(1, &[]),
            // 15: a subtable offset past the end of the table.
            unreadable,
            // 16: context, format 3, three inputs.
            lookup(
                5,
                &[context3(&[vec![1, 2], vec![3, 4], vec![5]], &[(1, 0)])],
            ),
            // 17: the two ends of the glyph space.
            lookup(1, &[single2(&[0, 65535], &[1, 2])]),
            // 18: three subtables, the middle one of a format the gate
            // refuses, each over its own glyph.
            lookup(
                2,
                &[
                    multiple(1, 1, &[2, 2]),
                    multiple(2, 2, &[1]),
                    multiple(1, 3, &[]),
                ],
            ),
        ])
    }

    /// A `GPOS` lookup list reaching every positioning kind the gate
    /// distinguishes.
    fn gpos_list() -> Vec<u8> {
        let mark_array = assemble(u16s(&[1, 0, 0]), &[4], &[anchor(10, 20)]);
        let base_array = assemble(u16s(&[2, 0, 0]), &[2, 4], &[anchor(5, 50), anchor(6, 60)]);
        let mark_to_base = assemble(
            u16s(&[1, 0, 0, 1, 0, 0]),
            &[2, 4, 8, 10],
            &[coverage(&[2]), coverage(&[1, 3]), mark_array, base_array],
        );
        let cursive = assemble(
            u16s(&[1, 0, 2, 0, 0, 0, 0]),
            &[2, 6, 8, 10, 12],
            &[
                coverage(&[8, 9]),
                anchor(0, 0),
                anchor(400, 30),
                anchor(0, 10),
                anchor(300, 0),
            ],
        );
        let pair1 = assemble(
            u16s(&[1, 0, 4, 0, 1, 0]),
            &[2, 10],
            &[coverage(&[3]), u16s(&[1, 4, (-20i16) as u16])],
        );
        let pair2 = assemble(
            u16s(&[2, 0, 4, 0, 0, 0, 2, 2, 0, 0, 0, (-30i16) as u16]),
            &[2, 8, 10],
            &[
                coverage(&[5, 6]),
                class_def(&[(5, 6, 1)]),
                class_def(&[(7, 7, 1)]),
            ],
        );
        let single = assemble(u16s(&[1, 0, 4, 50]), &[2], &[coverage(&[1, 2])]);
        lookup_list(&[
            lookup(1, &[single.clone()]),
            lookup(2, &[pair1, pair2]),
            lookup(3, &[cursive]),
            lookup(4, &[mark_to_base]),
            lookup(7, &[context3(&[vec![1], vec![2]], &[(1, 0)])]),
            lookup(8, &[chain3(&[vec![1]], &[vec![2]], &[], &[(0, 0)])]),
            lookup(9, &[extension(1, 1, single)]),
            // A type GPOS does not have.
            lookup(10, &[u16s(&[1, 0])]),
            // Cursive of a format nothing reads.
            lookup(3, &[assemble(u16s(&[2, 0, 0]), &[2], &[coverage(&[8])])]),
        ])
    }

    /// The index of each lookup in `list`.
    fn indexes(table: Table, list: &[u8]) -> Vec<Option<Arc<LookupIndex>>> {
        let lookups = LookupList::new(Bytes::new(list));
        let mut budget = Limits::DEFAULT.max_index_bytes;
        (0..lookups.len())
            .map(|at| {
                lookups
                    .get(at)
                    .and_then(|lookup| LookupIndex::build(table, &lookup, &mut budget))
                    .map(Arc::new)
            })
            .collect()
    }

    /// What one run of every lookup of `list` leaves: the glyphs and their
    /// positions, the warnings, and the operations charged.
    fn run(
        table: Table,
        list: &[u8],
        covered: &[Option<Arc<LookupIndex>>],
        glyphs: &[u16],
        limits: Limits,
    ) -> (Vec<ShapedGlyph>, Vec<Warning>, u32) {
        let lookups: Vec<(u16, u32)> = (0..LookupList::new(Bytes::new(list)).len())
            .map(|at| (at as u16, !0))
            .collect();
        let mut buffer = Buffer::from_glyphs(glyphs);
        for at in 0..buffer.len() {
            if let Some(glyph) = buffer.glyph_mut(at) {
                glyph.x_advance = 100;
            }
        }
        let mut runner =
            Runner::new(table, LookupList::new(Bytes::new(list)), None, limits).with_index(covered);
        runner.run(&mut buffer, &lookups);
        if table == Table::Gpos {
            buffer.propagate_attachments();
        }
        (buffer.glyphs().to_vec(), runner.warnings, runner.ops)
    }

    /// The limits each comparison runs under: the defaults, and each ceiling
    /// a probe can warn past set where some lookup reaches it.
    fn limit_sets() -> Vec<Limits> {
        let mut out = vec![Limits::DEFAULT];
        for depth in [0, 1] {
            out.push(Limits {
                max_extension_depth: depth,
                ..Limits::DEFAULT
            });
        }
        for length in [1, 2] {
            out.push(Limits {
                max_context_length: length,
                ..Limits::DEFAULT
            });
        }
        for operations in [0, 5, 40, 120] {
            out.push(Limits {
                max_operations: operations,
                ..Limits::DEFAULT
            });
        }
        out.push(Limits {
            max_nesting_depth: 0,
            ..Limits::DEFAULT
        });
        out.push(Limits {
            max_glyphs: 3,
            ..Limits::DEFAULT
        });
        out
    }

    /// Buffers over the tables' glyphs, drawn by a fixed sequence.
    fn buffers() -> Vec<Vec<u16>> {
        let alphabet = [0u16, 1, 2, 3, 4, 5, 6, 7, 8, 9, 65535];
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut out: Vec<Vec<u16>> = vec![Vec::new()];
        out.extend(alphabet.iter().map(|glyph| vec![*glyph]));
        for length in 2..=10 {
            for _ in 0..24 {
                let glyphs = (0..length)
                    .map(|_| {
                        state = state
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1_442_695_040_888_963_407);
                        alphabet[((state >> 33) as usize) % alphabet.len()]
                    })
                    .collect();
                out.push(glyphs);
            }
        }
        out
    }

    /// The runner with an index answers as the runner without one, glyph for
    /// glyph, position for position and warning for warning, and charges the
    /// same operations — past the ceiling, both past it.
    fn assert_index_changes_nothing(table: Table, list: &[u8]) {
        let covered = indexes(table, list);
        for limits in limit_sets() {
            for glyphs in buffers() {
                let probed = run(table, list, &[], &glyphs, limits);
                let indexed = run(table, list, &covered, &glyphs, limits);
                assert_eq!(probed.0, indexed.0, "{table} {glyphs:?} under {limits:?}");
                assert_eq!(probed.1, indexed.1, "{table} {glyphs:?} under {limits:?}");
                if probed.2 <= limits.max_operations || indexed.2 <= limits.max_operations {
                    assert_eq!(probed.2, indexed.2, "{table} {glyphs:?} under {limits:?}");
                }
            }
        }
    }

    #[test]
    fn an_index_changes_no_substitution() {
        let list = gsub_list();
        assert_index_changes_nothing(Table::Gsub, &list);
    }

    #[test]
    fn an_index_changes_no_position() {
        let list = gpos_list();
        assert_index_changes_nothing(Table::Gpos, &list);
    }

    /// Which lookups get an index, what each holds, and what sets one aside.
    #[test]
    fn the_index_is_each_subtables_first_coverage() {
        let list = gsub_list();
        let covered = indexes(Table::Gsub, &list);
        let index = |at: usize| covered[at].as_deref().expect("an index");
        let holds = |at: usize| -> Vec<u16> {
            (0..=u16::MAX)
                .filter(|glyph| index(at).any.holds(*glyph))
                .collect()
        };
        assert_eq!(holds(0), [1, 2]);
        // Everything the ranges list, though the search finds less.
        assert_eq!(holds(1), [2, 3, 5, 6]);
        assert_eq!(holds(2), [3]);
        assert_eq!(holds(3), Vec::<u16>::new(), "a format nothing reads");
        assert_eq!(holds(4), [4]);
        assert_eq!(holds(5), [7]);
        assert_eq!(holds(6), [1, 3], "a format 3 rule's first input");
        assert_eq!(holds(7), [4], "a chained rule's first input");
        assert_eq!(holds(8), [6]);
        assert_eq!(holds(9), [9], "through the extension");
        assert!(
            covered[10].is_none(),
            "an extension naming the extension type"
        );
        assert!(covered[11].is_none(), "an extension of another format");
        assert!(covered[12].is_none(), "a type GSUB does not have");
        assert_eq!(holds(13), [5, 65535]);
        assert_eq!(index(13).each.len(), 3);
        assert_eq!(index(13).holding(5), 3);
        assert_eq!(index(13).holding(65535), 1);
        assert_eq!(holds(14), Vec::<u16>::new(), "no subtables");
        assert_eq!(holds(15), Vec::<u16>::new(), "an unreadable subtable");
        assert_eq!(holds(17), [0, 65535]);
        assert_eq!(holds(18), [1, 3]);
        assert_eq!(index(18).holding(2), 0, "the refused format holds nothing");

        // The two limit-dependent warnings set an index aside, and only
        // under the limits that would fire them.
        assert_eq!(index(9).extension_hops, 1);
        assert!(index(9).answers_under(&Limits::DEFAULT));
        let shallow = Limits {
            max_extension_depth: 0,
            ..Limits::DEFAULT
        };
        assert!(!index(9).answers_under(&shallow));
        assert!(index(0).answers_under(&shallow));
        assert_eq!(index(16).longest_input, 3);
        let short = Limits {
            max_context_length: 2,
            ..Limits::DEFAULT
        };
        assert!(!index(16).answers_under(&short));
        assert!(index(6).answers_under(&short));

        let positioning = gpos_list();
        let covered = indexes(Table::Gpos, &positioning);
        let holds = |at: usize| -> Vec<u16> {
            let index = covered[at].as_deref().expect("an index");
            (0..=u16::MAX)
                .filter(|glyph| index.any.holds(*glyph))
                .collect()
        };
        assert_eq!(holds(0), [1, 2]);
        assert_eq!(holds(1), [3, 5, 6], "both pair formats' first coverage");
        assert_eq!(holds(2), [8, 9]);
        assert_eq!(holds(3), [2], "the mark coverage, not the base's");
        assert_eq!(holds(4), [1]);
        assert_eq!(holds(5), [2], "a chained rule's input, not its backtrack");
        assert_eq!(holds(6), [1, 2], "through the extension");
        assert!(covered[7].is_none(), "a type GPOS does not have");
        assert_eq!(holds(8), Vec::<u16>::new(), "a format nothing reads");
    }

    /// The bound: an index costs what it reads and keeps, and one the budget
    /// cannot pay for is not built.
    #[test]
    fn an_index_past_its_budget_is_not_built() {
        let list = gsub_list();
        let lookups = LookupList::new(Bytes::new(&list));
        let one = lookups.get(13).expect("lookup 13");
        let mut ample = usize::MAX;
        assert!(LookupIndex::build(Table::Gsub, &one, &mut ample).is_some());
        let cost = usize::MAX - ample;
        assert!(cost > 0);
        for short in [0, cost / 2, cost - 1] {
            let mut budget = short;
            assert!(
                LookupIndex::build(Table::Gsub, &one, &mut budget).is_none(),
                "{short} of {cost} bytes"
            );
        }
        let mut exact = cost;
        assert!(LookupIndex::build(Table::Gsub, &one, &mut exact).is_some());
        assert_eq!(exact, 0);
    }

    // --- shaping a paragraph ----------------------------------------------

    /// 1 000 characters of `words`, a space between each, drawn by a fixed
    /// sequence from `seed`.
    fn paragraph(words: &[&str], seed: u64) -> String {
        let mut state = seed;
        let mut text = String::new();
        while text.chars().count() < 1000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(words[((state >> 33) as usize) % words.len()]);
        }
        text.chars().take(1000).collect()
    }

    const LATIN: [&str; 24] = [
        "the",
        "of",
        "and",
        "a",
        "to",
        "in",
        "is",
        "that",
        "office",
        "affine",
        "flight",
        "waffle",
        "AVATAR",
        "Toyota",
        "caf\u{e9}",
        "na\u{ef}ve",
        "fa\u{e7}ade",
        "\u{fc}ber",
        "1984",
        "(see",
        "it).",
        "Wave",
        "LT",
        "yoyo",
    ];

    /// Urdu in the seventeen letters `TestShapeAran.ttf` maps.
    const URDU: [&str; 16] = [
        "\u{646}\u{633}\u{62a}\u{639}\u{644}\u{6cc}\u{642}",
        "\u{62e}\u{637}",
        "\u{641}\u{646}",
        "\u{637}\u{648}\u{637}\u{6cc}",
        "\u{62a}\u{62e}\u{62a}\u{6cc}",
        "\u{646}\u{642}\u{644}",
        "\u{642}\u{633}\u{637}",
        "\u{6a9}\u{644}",
        "\u{67e}\u{62a}\u{627}",
        "\u{679}\u{6a9}\u{679}",
        "\u{688}\u{627}\u{6a9}",
        "\u{633}\u{627}\u{644}",
        "\u{646}\u{6cc}\u{627}",
        "\u{642}\u{627}\u{646}\u{648}\u{646}",
        "\u{627}\u{646}\u{633}\u{627}\u{646}",
        "\u{644}\u{6cc}\u{6a9}\u{646}",
    ];

    /// Kannada, with conjuncts, a reph and pre-base and two-part vowels.
    const KANNADA: [&str; 15] = [
        "\u{c95}\u{ca8}\u{ccd}\u{ca8}\u{ca1}",
        "\u{cad}\u{cbe}\u{cb7}\u{cc6}",
        "\u{cae}\u{ca8}\u{cc6}",
        "\u{cb6}\u{cbe}\u{cb2}\u{cc6}",
        "\u{caa}\u{cc1}\u{cb8}\u{ccd}\u{ca4}\u{c95}",
        "\u{ca8}\u{cc0}\u{cb0}\u{cc1}",
        "\u{c95}\u{cb0}\u{ccd}\u{ca8}\u{cbe}\u{c9f}\u{c95}",
        "\u{cae}\u{cbe}\u{cb0}\u{ccd}\u{c9a}\u{ccd}",
        "\u{cac}\u{cc6}\u{c82}\u{c97}\u{cb3}\u{cc2}\u{cb0}\u{cc1}",
        "\u{c95}\u{ccd}\u{cb7}\u{cae}\u{cc6}",
        "\u{cb5}\u{cbf}\u{ca6}\u{ccd}\u{caf}\u{cc6}",
        "\u{cb9}\u{cac}\u{ccd}\u{cac}",
        "\u{c8a}\u{cb0}\u{cc1}",
        "\u{cb8}\u{ccd}\u{ca8}\u{cc7}\u{cb9}",
        "\u{ca6}\u{cc7}\u{cb5}\u{cb0}\u{cc1}",
    ];

    /// The three faces and their paragraphs, or `None` where this checkout
    /// does not have the font crate's Liberation faces beside this crate.
    fn faces() -> Option<Vec<(&'static str, Vec<u8>, String)>> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |path: &str| std::fs::read(root.join(path)).ok();
        Some(vec![
            (
                "Liberation Serif",
                read("../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf")?,
                paragraph(&LATIN, 1),
            ),
            (
                "TestShapeAran",
                read("data/text-rendering-tests/fonts/TestShapeAran.ttf")?,
                paragraph(&URDU, 2),
            ),
            (
                "Noto Sans Kannada",
                read("data/text-rendering-tests/fonts/NotoSansKannada-Regular.ttf")?,
                paragraph(&KANNADA, 3),
            ),
        ])
    }

    /// Everything a shaped paragraph is: each run's glyphs and warnings.
    fn output(runs: &[ShapedRun]) -> Vec<(Vec<ShapedGlyph>, Vec<Warning>)> {
        runs.iter()
            .map(|run| (run.glyphs().to_vec(), run.warnings().to_vec()))
            .collect()
    }

    /// P-26.3: through a kept `Plans`, a position no subtable of a lookup
    /// covers is passed without a subtable resolved or probed, and the
    /// paragraph shapes as it does with none.
    #[test]
    fn a_glyph_no_subtable_covers_is_never_probed() {
        let Some(faces) = faces() else {
            println!("shape-index-probes: SKIPPED (no Liberation faces beside this crate)");
            return;
        };
        for (name, bytes, text) in &faces {
            assert_eq!(text.chars().count(), 1000, "{name}'s paragraph");
            let face = Sfnt::parse(bytes).unwrap_or_else(|| panic!("{name} parses"));
            let plain = Shaper::new(&face).shape_text(text, BaseDirection::Auto).1;
            let _ = probes::take();
            let plans = Plans::new();
            let kept = Shaper::new(&face)
                .with_plans(&plans)
                .shape_text(text, BaseDirection::Auto)
                .1;
            let log = probes::take();
            assert_eq!(
                output(&plain),
                output(&kept),
                "{name}: the index moved the output"
            );

            let indexed = plans.indexes();
            assert!(!indexed.is_empty(), "{name}: no lookup was planned");
            assert!(
                indexed.iter().all(|(_, has)| *has),
                "{name}: a planned lookup has no index: {indexed:?}"
            );
            let (mut passed, mut tried) = (0usize, 0usize);
            for probe in &log {
                let holding = probe
                    .holding
                    .unwrap_or_else(|| panic!("{name}: probed without an index: {probe:?}"));
                assert!(
                    probe.resolved <= holding && probe.probed <= holding,
                    "{name}: more subtables resolved or probed than hold the glyph: {probe:?}"
                );
                if holding == 0 {
                    passed += 1;
                } else {
                    tried += 1;
                }
            }
            assert!(passed > 0, "{name}: the index passed no position");
            assert!(tried > 0, "{name}: no lookup was tried at all");
            println!("RAN shape-index-probes: {name}, {passed} passed, {tried} tried");
        }
    }

    /// P-26.3: one `Plans` builds each plan and index once, however many runs
    /// follow, and another feature set builds plans of its own and indexes no
    /// lookup twice.
    #[test]
    fn a_plan_is_built_once_per_face_and_feature_set() {
        let Some(faces) = faces() else {
            println!("shape-plans-once: SKIPPED (no Liberation faces beside this crate)");
            return;
        };
        for (name, bytes, text) in &faces {
            let face = Sfnt::parse(bytes).unwrap_or_else(|| panic!("{name} parses"));
            let plans = Plans::new();
            let shaper = Shaper::new(&face).with_plans(&plans);
            let first = output(&shaper.shape_text(text, BaseDirection::Auto).1);
            let (built, indexed) = (plans.plans_built(), plans.indexes_built());
            assert!(built > 0 && indexed > 0, "{name}: nothing was built");
            for run in 1..100 {
                let again = output(&shaper.shape_text(text, BaseDirection::Auto).1);
                assert_eq!(again, first, "{name}: run {run} shaped otherwise");
            }
            assert_eq!(plans.plans_built(), built, "{name}: a plan was built again");
            assert_eq!(
                plans.indexes_built(),
                indexed,
                "{name}: an index was built again"
            );

            let off = [(Tag::new(b"kern"), 0)];
            let other = Shaper::new(&face).with_plans(&plans).with_settings(&off);
            let plain = Shaper::new(&face).with_settings(&off);
            assert_eq!(
                output(&other.shape_text(text, BaseDirection::Auto).1),
                output(&plain.shape_text(text, BaseDirection::Auto).1),
                "{name}: the second feature set shaped otherwise"
            );
            assert!(
                plans.plans_built() > built,
                "{name}: no plan for the second set"
            );
            assert_eq!(
                plans.indexes_built(),
                indexed,
                "{name}: the second set indexed a lookup again"
            );
            println!("RAN shape-plans-once: {name}, {built} plans, {indexed} indexes");
        }
    }
}
