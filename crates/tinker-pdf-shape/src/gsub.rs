//! `GSUB`: the eight kinds of substitution, and what each does to the buffer.
//!
//! Type 7 is not here, because it is not a substitution: an extension
//! subtable is a 32-bit offset to one of the other seven, and
//! [`crate::apply::Runner`] follows it before anything in this file is
//! reached. Types 5 and 6 are not here either, for the opposite reason — a
//! contextual substitution is a rule that runs *other* lookups, and every
//! line of it is shared word for word with `GPOS` types 7 and 8, so it lives
//! in `apply.rs` and is called from here.
//!
//! What is left is the five that actually change glyphs, and the one that
//! runs backwards.
//!
//! # The cluster is what makes a ligature reversible
//!
//! Every substitution here keeps the cluster of what it replaced: a ligature
//! takes the smallest of its components', a multiple substitution gives all
//! its outputs the input's. That is not bookkeeping for its own sake — it is
//! the whole of what makes milestone 7's `/ToUnicode` possible, because after
//! `f` and `i` become one glyph the only remaining evidence that two
//! characters went in is the cluster they share.

use crate::apply::{apply_chain_context, apply_context, Applied, Runner, Skipper};
use crate::buffer::Buffer;
use crate::common::Coverage;
use crate::gdef::GlyphClass;
use crate::read::Bytes;

/// Applies one `GSUB` subtable at one position.
///
/// `kind` has already had any extension indirection resolved, so type 7 never
/// reaches here.
pub(crate) fn apply<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    kind: u16,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    depth: u32,
) -> Applied {
    match kind {
        1 => single(runner, buffer, data, at),
        2 => multiple(runner, buffer, data, at),
        3 => alternate(runner, buffer, data, at),
        4 => ligature(runner, buffer, data, skipper, at),
        5 => apply_context(runner, buffer, data, skipper, at, depth),
        6 => apply_chain_context(runner, buffer, data, skipper, at, depth),
        8 => reverse_chain(runner, buffer, data, skipper, at),
        _ => Applied::Unsupported,
    }
}

/// Type 1: one glyph becomes one other glyph.
///
/// Two formats, and the difference is only how the output is written: format
/// 1 adds a constant to the glyph index, format 2 lists the replacements.
/// The constant is added *modulo 65 536* — the specification says the result
/// "must be a valid glyph index", which is the font's obligation, and a
/// reader that saturated instead would silently substitute glyph 65 535 for
/// every overflow.
fn single(runner: &Runner<'_, '_>, buffer: &mut Buffer, data: Bytes<'_>, at: usize) -> Applied {
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let replacement = match data.u16(0) {
        Some(1) => data.i16(4).map(|delta| {
            #[allow(clippy::cast_sign_loss)]
            glyph.wrapping_add(delta as u16)
        }),
        Some(2) => {
            let count = usize::from(data.u16(4).unwrap_or(0));
            if usize::from(index) >= count {
                None
            } else {
                data.u16_at(6, usize::from(index))
            }
        }
        _ => None,
    };
    let Some(replacement) = replacement else {
        return Applied::No;
    };
    buffer.set_glyph(at, replacement);
    runner.reclassify(buffer, at, GlyphClass::Base);
    Applied::Yes(at.saturating_add(1))
}

/// Type 2: one glyph becomes several — or none.
///
/// A `Sequence` of length zero deletes the glyph, which the specification
/// permits in as many words, and it is the reason this returns `at` rather
/// than `at + 1` in that case: there is no glyph at `at + 1` any more, and a
/// cursor moved past the deletion would step over the glyph that took its
/// place.
fn multiple<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let count = usize::from(data.u16(4).unwrap_or(0));
    if usize::from(index) >= count {
        return Applied::No;
    }
    let Some(sequence) = data.offset16_at(6, usize::from(index)) else {
        return Applied::No;
    };
    let glyphs = usize::from(sequence.u16(0).unwrap_or(0));
    if !runner.may_grow(buffer, glyphs) {
        return Applied::No;
    }
    let mut replacements = Vec::with_capacity(glyphs);
    for n in 0..glyphs {
        let Some(glyph) = sequence.u16_at(2, n) else {
            return Applied::No;
        };
        replacements.push(glyph);
    }
    buffer.replace(at, &replacements);
    for n in 0..replacements.len() {
        runner.reclassify(buffer, at.saturating_add(n), GlyphClass::Base);
    }
    Applied::Yes(at.saturating_add(replacements.len()))
}

/// Type 3: one glyph becomes one of several, and this crate takes the first.
///
/// **A decision, not an omission.** An alternate set is what `aalt` and
/// `salt` offer a user interface: "this glyph, but the second swash". Which
/// alternate a run wants is a *feature parameter* — a number the caller
/// supplies beside the feature tag — and nothing in this milestone's API
/// carries one. Taking the first is the specification's own default for a
/// feature turned on with no index, so a face's `aalt` behaves as though it
/// were on rather than as though it were absent; a later milestone that grows
/// an alternate index changes this function and nothing else.
fn alternate(runner: &Runner<'_, '_>, buffer: &mut Buffer, data: Bytes<'_>, at: usize) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let count = usize::from(data.u16(4).unwrap_or(0));
    if usize::from(index) >= count {
        return Applied::No;
    }
    let Some(set) = data.offset16_at(6, usize::from(index)) else {
        return Applied::No;
    };
    if set.u16(0).unwrap_or(0) == 0 {
        return Applied::No;
    }
    let Some(replacement) = set.u16_at(2, 0) else {
        return Applied::No;
    };
    buffer.set_glyph(at, replacement);
    runner.reclassify(buffer, at, GlyphClass::Base);
    Applied::Yes(at.saturating_add(1))
}

/// Type 4: several glyphs become one.
///
/// # Where the marks go
///
/// A ligature lookup nearly always ignores marks, so the glyphs it matches
/// are not adjacent: `lam`, `alef` with a fatha between them is three glyphs
/// and a two-component rule. The components are removed and **the marks stay
/// where they were**, which is the easy half.
///
/// The hard half is that each surviving mark now hangs off a glyph that no
/// longer exists, and `GPOS` mark-to-ligature attachment will shortly need to
/// know *which component* it belonged to — the fatha above belongs over the
/// lam, not over the alef, and the two anchors are in different places. So
/// each ligature gets a serial number and each mark inside it is stamped with
/// the component it followed, counting from one. Marks that were never inside
/// the ligature keep a serial of zero, and mark-to-ligature attachment reads
/// that as "the last component", which is the specification's own fallback.
///
/// Components that are themselves ligatures make the numbering
/// non-obvious — a two-component ligature of a three-component ligature and a
/// letter has four components as far as its marks are concerned — which is
/// what the component arithmetic below is for.
fn ligature<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let count = usize::from(data.u16(4).unwrap_or(0));
    if usize::from(index) >= count {
        return Applied::No;
    }
    let Some(set) = data.offset16_at(6, usize::from(index)) else {
        return Applied::No;
    };
    let ligatures = usize::from(set.u16(0).unwrap_or(0));
    for n in 0..ligatures {
        let Some(entry) = set.offset16_at(2, n) else {
            continue;
        };
        let Some(replacement) = entry.u16(0) else {
            continue;
        };
        let components = usize::from(entry.u16(2).unwrap_or(0));
        if components == 0 {
            continue;
        }
        // The component array holds glyphs 2..n; the first is the coverage.
        let matched = runner.match_input(buffer, skipper, at, components, |step, found| {
            step == 0 || entry.u16_at(4, step - 1) == Some(found)
        });
        let Some(matched) = matched else {
            continue;
        };
        ligate(runner, buffer, &matched.positions, replacement);
        // Every component but the first has been removed, so what was one
        // past the match is now that much closer.
        let removed = matched.positions.len().saturating_sub(1);
        return Applied::Yes(matched.end.saturating_sub(removed).max(at));
    }
    Applied::No
}

/// Replaces the matched components with the ligature glyph and renumbers the
/// marks between them.
fn ligate(runner: &mut Runner<'_, '_>, buffer: &mut Buffer, positions: &[usize], glyph: u16) {
    let Some(first) = positions.first().copied() else {
        return;
    };
    // A ligature made entirely of marks is itself a mark — a shadda over a
    // fatha is one mark, not a base — and it gets no serial number, because
    // nothing attaches to a component of it.
    let all_marks = positions
        .iter()
        .all(|at| buffer.props(*at).class == GlyphClass::Mark);
    let total: u16 = positions
        .iter()
        .map(|at| buffer.props(*at).num_comps)
        .fold(0u16, u16::saturating_add);
    let lig_id = if all_marks { 0 } else { buffer.next_lig_id() };

    let mut last_num_comps = buffer.props(first).num_comps;
    let mut last_lig_id = buffer.props(first).lig_id;
    let mut so_far = last_num_comps;

    buffer.set_glyph(first, glyph);
    runner.reclassify(
        buffer,
        first,
        if all_marks {
            GlyphClass::Mark
        } else {
            GlyphClass::Ligature
        },
    );
    if let Some(props) = buffer.props_mut(first) {
        props.lig_id = lig_id;
        props.lig_comp = 0;
        props.num_comps = total;
    }

    for step in 1..positions.len() {
        let (Some(previous), Some(current)) = (
            positions.get(step - 1).copied(),
            positions.get(step).copied(),
        ) else {
            break;
        };
        if !all_marks {
            for between in previous.saturating_add(1)..current {
                let comp = component_of(buffer, between, so_far, last_num_comps);
                if let Some(props) = buffer.props_mut(between) {
                    props.lig_id = lig_id;
                    props.lig_comp = comp;
                }
            }
        }
        last_lig_id = buffer.props(current).lig_id;
        last_num_comps = buffer.props(current).num_comps;
        so_far = so_far.saturating_add(last_num_comps);
    }

    // Marks that trailed the *last* component belonged to it, and it may have
    // been a ligature with its own numbering. Without this, a ligature of a
    // ligature loses the marks hanging off its tail.
    if !all_marks && last_lig_id != 0 {
        let mut after = positions.last().copied().unwrap_or(first).saturating_add(1);
        while after < buffer.len() {
            let props = buffer.props(after);
            if props.lig_id != last_lig_id || props.lig_comp == 0 {
                break;
            }
            let comp = component_of(buffer, after, so_far, last_num_comps);
            if let Some(props) = buffer.props_mut(after) {
                props.lig_id = lig_id;
                props.lig_comp = comp;
            }
            after = after.saturating_add(1);
        }
    }

    for at in positions.iter().skip(1).rev() {
        buffer.remove(*at);
    }
}

/// Which component of the new ligature the glyph at `at` belongs to.
///
/// A mark that was not already part of a ligature is component
/// `so_far` — the number of components consumed before the one it follows.
/// A mark that *was* part of one keeps its position within that component's
/// span, which is what the clamp preserves: component 2 of a three-component
/// inner ligature stays the second of the three, wherever the three now sit.
fn component_of(buffer: &Buffer, at: usize, so_far: u16, last_num_comps: u16) -> u16 {
    let mut this = buffer.props(at).lig_comp;
    if this == 0 {
        this = last_num_comps;
    }
    so_far
        .saturating_sub(last_num_comps)
        .saturating_add(this.min(last_num_comps))
}

/// Type 8: one glyph becomes another, decided by what follows it, applied
/// from the end of the run backwards.
///
/// The direction is the whole point and the specification is explicit about
/// it: the lookahead of a reverse chaining rule is text this lookup has *not
/// yet* rewritten, so a forward pass would test its conditions against glyphs
/// it was about to replace. Urdu's Nastaliq faces are built on this — the
/// form a letter takes depends on the shape of the word ahead of it, resolved
/// right to left — and a shaper that ran this lookup forwards produces a
/// plausible, wrong word.
///
/// It is also the only lookup that may not run other lookups, which is why
/// nothing here recurses.
fn reverse_chain<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let Some(backtrack) = data.u16(4).map(usize::from) else {
        return Applied::No;
    };
    let backtrack_at = 6usize;
    let Some(lookahead_count_at) = backtrack_at.checked_add(backtrack.saturating_mul(2)) else {
        return Applied::No;
    };
    let Some(lookahead) = data.u16(lookahead_count_at).map(usize::from) else {
        return Applied::No;
    };
    let lookahead_at = lookahead_count_at.saturating_add(2);
    let Some(glyphs_count_at) = lookahead_at.checked_add(lookahead.saturating_mul(2)) else {
        return Applied::No;
    };
    let Some(replacements) = data.u16(glyphs_count_at).map(usize::from) else {
        return Applied::No;
    };
    let replacements_at = glyphs_count_at.saturating_add(2);
    if usize::from(index) >= replacements {
        return Applied::No;
    }
    let covers = |base: usize, step: usize, glyph: u16| {
        data.offset16_at(base, step)
            .map(Coverage::new)
            .is_some_and(|coverage| coverage.covers(glyph))
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
        .match_lookahead(
            buffer,
            skipper,
            at.saturating_add(1),
            lookahead,
            |step, found| covers(lookahead_at, step, found),
        )
        .is_none()
    {
        return Applied::No;
    }
    let Some(replacement) = data.u16_at(replacements_at, usize::from(index)) else {
        return Applied::No;
    };
    buffer.set_glyph(at, replacement);
    runner.reclassify(buffer, at, GlyphClass::Base);
    Applied::Yes(at)
}
