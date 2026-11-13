//! Running a description's passes over a slot stream.
//!
//! `docs/design/shaping.md`, *Passes running* and *Bidi*, is the reading
//! this follows. Passes run in the description's order over the whole run,
//! each over the stream the last left; a reversed pass runs over the stream
//! turned end to end, which is turned back after it; a pass constraint that
//! is false skips the pass. At each scan position the first candidate whose
//! constraint holds for every slot of its match fires; its action leaves the
//! slot pointer where its `Next`s put it, and the scan position becomes that
//! pointer plus the offset it returns. Where no rule fires the position
//! advances by one.
//!
//! # The loop ceiling
//!
//! The GDL's safety net, held to the pass's own counts: once `maxRuleLoop`
//! rules have fired without the scan position passing the furthest it has
//! reached, it is advanced past it before another rule is tried; and a rule
//! that would move it more than `maxBackup` slots behind that furthest
//! leaves it where it stood. Every position is therefore passed in a bounded
//! number of firings, and every instruction they run is charged against the
//! run's [`Budget`] besides.

use core::ops::Range;

use super::machine::{self, Block, Bounds, Budget, Frame, Program, Site};
use super::{Pass, Refusal, Slots};
use crate::limits::Limits;

/// Runs the substitution stage of the stream's description: its line-break
/// and substitution passes, from the first to the first justification pass,
/// with the bidi pass placed before the pass it names, or at the stage's end
/// where that is a justification or positioning pass.
///
/// This crate resolves UAX #9 over the paragraph before a run is shaped, so
/// every slot of a run is at the run's level and the bidi pass reorders
/// nothing: what it does is mirror, where the description has one and its
/// `Silf` is 4 or later, each slot of a run at an odd level
/// (`right_to_left`) taking its glyph's mirrored glyph from the mirroring
/// attribute.
///
/// Returns what the passes refused, each once, in the order met.
pub fn substitute(
    slots: &mut Slots<'_>,
    right_to_left: bool,
    limits: &Limits,
    budget: &mut Budget,
) -> Vec<Refusal> {
    let description = slots.face().description;
    let end = usize::from(description.justification.min(description.positioning));
    let bidi = usize::from(description.bidi);
    let mirrors = description.bidi != 255 && description.table_version() >= 4 && right_to_left;
    let mut refusals = Vec::new();
    if mirrors && bidi < end {
        run_passes(slots, 0..bidi, limits, budget, &mut refusals);
        mirror(slots, &mut refusals);
        run_passes(slots, bidi..end, limits, budget, &mut refusals);
    } else {
        run_passes(slots, 0..end, limits, budget, &mut refusals);
        if mirrors {
            mirror(slots, &mut refusals);
        }
    }
    refusals
}

/// Runs the positioning stage of the stream's description: its passes from
/// the first positioning pass on, each making only one-to-one substitutions.
/// What they set is resolved into positions by [`super::position::place`].
///
/// Returns what the passes refused, each once, in the order met.
pub fn position(slots: &mut Slots<'_>, limits: &Limits, budget: &mut Budget) -> Vec<Refusal> {
    let description = slots.face().description;
    let start = usize::from(description.positioning);
    let mut refusals = Vec::new();
    run_passes(
        slots,
        start..usize::from(description.passes),
        limits,
        budget,
        &mut refusals,
    );
    refusals
}

/// Each slot's glyph, mirrored: the glyph the description's mirroring
/// attribute names, where it names one.
fn mirror(slots: &mut Slots<'_>, refusals: &mut Vec<Refusal>) {
    let description = slots.face().description;
    let attribute = u16::from(description.attr_mirroring);
    for at in 0..slots.len() {
        let Some(glyph) = slots.glyph_at(at) else {
            continue;
        };
        let mirrored = match slots.face().attributes {
            Some(attributes) => attributes.lookup(glyph, attribute),
            None => Ok(0),
        };
        match mirrored {
            Ok(mirrored) if mirrored > 0 => {
                if let Ok(mirrored) = u16::try_from(mirrored) {
                    slots.set_glyph(at, mirrored);
                }
            }
            Ok(_) => {}
            Err(refusal) => refuse(refusals, refusal),
        }
    }
}

/// Runs the passes of `range` of the stream's description, in order, each
/// over the stream the last left.
pub fn run_passes(
    slots: &mut Slots<'_>,
    range: Range<usize>,
    limits: &Limits,
    budget: &mut Budget,
    refusals: &mut Vec<Refusal>,
) {
    let description = slots.face().description;
    let count = usize::from(description.passes);
    for index in range.start..range.end.min(count) {
        if budget.is_spent() {
            break;
        }
        match description.pass(index, limits) {
            Ok(pass) => run_pass(slots, &pass, index, limits, budget, refusals),
            Err(refusal) => refuse(refusals, refusal),
        }
    }
}

/// Adds `refusal` to `refusals` where it is not there already.
pub(crate) fn refuse(refusals: &mut Vec<Refusal>, refusal: Refusal) {
    if !refusals.contains(&refusal) {
        refusals.push(refusal);
    }
}

/// A rule's code checked once a pass, its refusal kept where it failed.
type Checked = Option<Option<Program>>;

/// The program of `code` at `site`, checked the first time it is asked for;
/// `None` where it fails its check, which is refused once.
fn program<'c>(
    cache: &'c mut [Checked],
    rule: u16,
    code: Option<&[u8]>,
    site: Site,
    bounds: &Bounds,
    limits: &Limits,
    refusals: &mut Vec<Refusal>,
) -> Option<&'c Program> {
    let entry = cache.get_mut(usize::from(rule))?;
    if entry.is_none() {
        let checked = match code {
            Some(code) => machine::check(code, site, bounds, limits),
            None => Err(Refusal::PassMalformed { pass: site.pass }),
        };
        *entry = Some(match checked {
            Ok(program) => Some(program),
            Err(refusal) => {
                refuse(refusals, refusal);
                None
            }
        });
    }
    entry.as_ref().and_then(Option::as_ref)
}

/// Runs one pass over the stream.
fn run_pass(
    slots: &mut Slots<'_>,
    pass: &Pass<'_>,
    index: usize,
    limits: &Limits,
    budget: &mut Budget,
    refusals: &mut Vec<Refusal>,
) {
    let description = slots.face().description;
    let features = slots.face().features.map_or(0, |features| features.len());
    let bounds = Bounds::for_pass(description, features, index);
    let number = pass.number();
    if let Some(code) = pass.pass_constraint() {
        let site = Site {
            pass: number,
            block: Block::PassConstraint,
        };
        let frame = Frame {
            position: 0,
            under_test: None,
        };
        let holds = machine::check(code, site, &bounds, limits)
            .and_then(|program| machine::holds(&program, &*slots, frame, budget));
        match holds {
            Ok(true) => {}
            Ok(false) => return,
            Err(refusal) => {
                refuse(refusals, refusal);
                return;
            }
        }
    }
    if !pass.has_machine() {
        return;
    }
    let reversed = pass.is_reversed();
    if reversed {
        slots.reverse();
    }
    let rules = usize::from(pass.rules);
    let mut constraints: Vec<Checked> = vec![None; rules];
    let mut actions: Vec<Checked> = vec![None; rules];
    let max_backup = i64::from(pass.max_backup);
    let max_rule_loop = u32::from(pass.max_rule_loop);
    let mut position = 0usize;
    let mut furthest = 0usize;
    let mut loops = 0u32;
    while position < slots.len() && !budget.is_spent() {
        if position > furthest {
            furthest = position;
            loops = 0;
        }
        let matched = pass.candidates_in(|at| slots.glyph_at(at), position, limits);
        for refusal in matched.refusals {
            refuse(refusals, refusal);
        }
        let mut fired = None;
        for candidate in &matched.candidates {
            let rule = candidate.rule;
            let site = Site {
                pass: number,
                block: Block::Constraint(rule),
            };
            let Some(constraint) = program(
                &mut constraints,
                rule,
                pass.constraint(rule),
                site,
                &bounds,
                limits,
                refusals,
            ) else {
                continue;
            };
            // Once for each slot of its match, from its first pre-context
            // slot to the last slot the machine read.
            let first = position.saturating_sub(usize::from(candidate.pre_context));
            let mut holds = true;
            for under_test in first..candidate.end {
                let frame = Frame {
                    position,
                    under_test: Some(under_test),
                };
                match machine::holds(constraint, &*slots, frame, budget) {
                    Ok(true) => {}
                    Ok(false) => {
                        holds = false;
                        break;
                    }
                    Err(refusal) => {
                        refuse(refusals, refusal);
                        holds = false;
                        break;
                    }
                }
            }
            if !holds {
                continue;
            }
            let site = Site {
                pass: number,
                block: Block::Action(rule),
            };
            let Some(action) = program(
                &mut actions,
                rule,
                pass.action(rule),
                site,
                &bounds,
                limits,
                refusals,
            ) else {
                continue;
            };
            fired = Some(slots.act(action, position, budget, limits));
            break;
        }
        position = match fired {
            None => position + 1,
            Some(Err(refusal)) => {
                refuse(refusals, refusal);
                position + 1
            }
            Some(Ok(acted)) => {
                let len = i64::try_from(slots.len()).unwrap_or(i64::MAX);
                let target = acted
                    .pointer
                    .saturating_add(i64::from(acted.offset))
                    .clamp(0, len);
                let floor = i64::try_from(furthest).unwrap_or(i64::MAX) - max_backup;
                // Backed up past `maxBackup`: left where it stood.
                let next = if target < floor {
                    position
                } else {
                    usize::try_from(target).unwrap_or(0)
                };
                if next > furthest {
                    loops = 0;
                    next
                } else {
                    loops += 1;
                    if loops >= max_rule_loop {
                        loops = 0;
                        furthest + 1
                    } else {
                        next
                    }
                }
            }
        };
    }
    if reversed {
        slots.reverse();
    }
}
