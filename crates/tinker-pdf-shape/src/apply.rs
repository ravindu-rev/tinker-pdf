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
//! and [`Limits::max_operations`] bounds the attempts, and the two are
//! separate because a font can exhaust either without touching the other.
//!
//! # Skipping is not a filter on the output
//!
//! A lookup flag says which glyphs this lookup *cannot see*: a rule with
//! `IGNORE_MARKS` matching `f`, `i` matches across a combining accent between
//! them, and the accent stays exactly where it was. Getting this wrong is the
//! difference between a shaper that sets Arabic and one that mostly does:
//! every joining rule in every Arabic face is written assuming the vowel
//! signs are invisible to it.

use crate::buffer::Buffer;
use crate::common::{
    ClassDef, Coverage, Lookup, LookupList, IGNORE_BASE_GLYPHS, IGNORE_LIGATURES, IGNORE_MARKS,
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
}

/// Which glyphs a lookup can see.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Skipper<'a> {
    flags: u16,
    mark_set: Option<Coverage<'a>>,
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
        }
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

    /// The filter lookup `index` sees the buffer through.
    fn skipper(&self, lookup: &Lookup<'a>) -> Skipper<'a> {
        let mark_set = lookup
            .mark_filtering_set()
            .and_then(|set| self.gdef?.mark_glyph_set(usize::from(set)));
        Skipper {
            flags: lookup.flags(),
            mark_set,
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

    /// Runs every lookup in `indices`, in the order given.
    pub(crate) fn run(&mut self, buffer: &mut Buffer, indices: &[u16]) {
        self.classify(buffer);
        for index in indices {
            self.run_lookup(buffer, *index);
        }
    }

    fn run_lookup(&mut self, buffer: &mut Buffer, index: u16) {
        let Some(lookup) = self.lookups.get(usize::from(index)) else {
            self.warn(Warning::MissingLookup {
                table: self.table,
                lookup: index,
            });
            return;
        };
        let skipper = self.skipper(&lookup);
        let kind = self.resolved_kind(&lookup, index);
        // Reverse chaining single substitution is the one lookup the
        // specification requires be applied from the end of the run: its
        // lookahead is the text it has not rewritten yet, so a forward pass
        // would match against glyphs it was about to replace.
        if self.table == Table::Gsub && kind == 8 {
            self.run_backward(buffer, &lookup, index, &skipper);
        } else {
            self.run_forward(buffer, &lookup, index, &skipper);
        }
    }

    fn run_forward(
        &mut self,
        buffer: &mut Buffer,
        lookup: &Lookup<'a>,
        index: u16,
        skipper: &Skipper<'a>,
    ) {
        let mut at = 0usize;
        while at < buffer.len() {
            if skipper.skips(buffer, at) {
                at += 1;
                continue;
            }
            let before = buffer.len();
            match self.apply_at(buffer, lookup, index, skipper, at, 0) {
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
    ) {
        let mut at = buffer.len();
        while at > 0 {
            at -= 1;
            if skipper.skips(buffer, at) {
                continue;
            }
            let _ = self.apply_at(buffer, lookup, index, skipper, at, 0);
        }
    }

    /// Tries every subtable of one lookup at one position.
    ///
    /// The first that applies wins and the rest are not consulted, which is
    /// the specification's rule and not an optimisation: subtables of one
    /// lookup are alternatives, and a face relies on the earlier ones
    /// shadowing the later.
    pub(crate) fn apply_at(
        &mut self,
        buffer: &mut Buffer,
        lookup: &Lookup<'a>,
        index: u16,
        skipper: &Skipper<'a>,
        at: usize,
        depth: u32,
    ) -> Option<usize> {
        self.ops = self.ops.saturating_add(1);
        if self.ops > self.limits.max_operations {
            self.warn(Warning::OperationBudgetExceeded { table: self.table });
            return None;
        }
        for sub in 0..lookup.len() {
            let Some((kind, data)) = self.resolve(lookup, sub, index) else {
                continue;
            };
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
        if count > self.limits.max_context_length {
            self.warn(Warning::ContextTooLong { table: self.table });
            return None;
        }
        let mut positions = Vec::with_capacity(count);
        let mut cursor = at;
        for step in 0..count {
            if step > 0 {
                cursor = self.step_forward(buffer, skipper, cursor)?;
            }
            let glyph = buffer.glyph_id(cursor)?;
            if !wanted(step, glyph) {
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
        let mut positions = Vec::with_capacity(count);
        let mut cursor = at;
        for step in 0..count {
            cursor = self.step_backward(buffer, skipper, cursor)?;
            let glyph = buffer.glyph_id(cursor)?;
            if !wanted(step, glyph) {
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
        let mut positions = Vec::with_capacity(count);
        // `from` is one past the last matched glyph, so the first lookahead
        // candidate is `from` itself — but it may be a glyph this lookup
        // steps over, which is why the search starts one behind it and moves
        // forward rather than reading `from` directly.
        let mut cursor = from.checked_sub(1)?;
        for step in 0..count {
            cursor = self.step_forward(buffer, skipper, cursor)?;
            let glyph = buffer.glyph_id(cursor)?;
            if !wanted(step, glyph) {
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
