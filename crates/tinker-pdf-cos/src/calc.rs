//! Running a form's calculations (gap 27, option A).
//!
//! [`crate::script`] is the interpreter and knows nothing about PDF; this
//! module is the other half — which scripts run, in what order, what they may
//! see, and how their answers reach the document.
//!
//! # A partial calculation is impossible here, twice over
//!
//! The failure this whole feature is judged by is a form that sets three
//! fields, fails on the fourth, and is saved looking filled and wrong. Two
//! independent things prevent it, and either alone would be enough:
//!
//! 1. **Scripts never touch the document.** Every write a script makes —
//!    `event.value`, or `getField("x").value = ...` — lands in a staging map
//!    held by [`StagedHost`]. Later scripts read that map, which is what makes
//!    `/CO` order mean something, but the document is untouched for the whole
//!    pass. A script that fails halfway had nothing to undo.
//! 2. **The apply is one all-or-nothing call.** What the pass computed goes in
//!    through `DocumentEditor::set_calculated_values`, which is
//!    `DocumentEditor::transaction` underneath: the first field the fill layer
//!    refuses rolls back every field before it and names which one it was.
//!
//! **Any script that cannot be run refuses the whole pass.** Running the nine
//! scripts that parse and skipping the tenth would produce a document whose
//! totals disagree with its inputs, which is precisely the outcome gap 27
//! calls the worst a form has — so a form that this build cannot compute is
//! left exactly as it was saved, and the caller is told which field and why.
//!
//! # The cascade rule
//!
//! `/CO` gives an order, and a script can write a field whose own calculate
//! action would fire. The rule here, chosen and enforced:
//!
//! **One pass, and every calculate action runs at most once.** The sequence is
//! `/CO` order first (12.7.2 table 218), then any remaining field carrying
//! `/AA /C` in document order, so a form whose producer left `/CO` out still
//! computes. A write is visible to every *later* script in the pass — that is
//! what makes the order mean something — but it never re-triggers a script
//! that has already run, and never re-orders the pass.
//!
//! A cycle therefore terminates by construction rather than by a counter: A
//! writes B, B writes A, and the pass ends after two scripts with a
//! deterministic result. Bounded re-entry was the alternative and was
//! rejected — it needs a depth cap, a depth cap on a branching cascade is not
//! a work cap, and it makes the answer depend on a number nobody can predict
//! from the file. What re-entry would have hidden is reported instead:
//! [`Recalculation::cascades_cut`] names every field a later script wrote
//! after that field's own calculation had already run, which is the one case
//! where this rule and a full fixed-point disagree.

use std::collections::{BTreeMap, HashSet};

use crate::edit::{DocumentEditor, FillRejection, SkippedWidget};
use crate::fill;
use crate::form::{self, Script};
use crate::limits;
use crate::script::{self, Budget, Host, ScriptError};

/// Why a recalculation produced nothing.
///
/// Every variant means **nothing was written**, which is the same contract
/// `FillRejection` gives one field down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcError {
    /// A script would not run. Names the field whose action it was
    /// (12.7.3.2's fully qualified name) and what went wrong.
    Script {
        /// The field carrying the action.
        field: String,
        /// Why the interpreter refused it.
        reason: ScriptError,
    },
    /// The computed values were refused by the fill layer, which named the
    /// first field to refuse and why.
    Rejected(FillRejection),
    /// More fields carry a calculate action than
    /// [`limits::MAX_CALC_FIELDS`]. Refused rather than truncated, for the
    /// same reason a failing script refuses the pass.
    TooManyFields(usize),
    /// [`formatted_value`] was asked about a field the form does not have.
    NoSuchField,
}

impl core::fmt::Display for CalcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CalcError::Script { field, reason } => write!(f, "{field}: {reason}"),
            CalcError::Rejected(rejection) => write!(f, "{rejection}"),
            CalcError::TooManyFields(count) => {
                write!(f, "{count} fields carry a calculate action")
            }
            CalcError::NoSuchField => f.write_str("no such field"),
        }
    }
}

/// What a recalculation did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recalculation {
    /// The fields whose `/V` changed, with their new values, in the order the
    /// pass computed them. A field whose calculation produced the value it
    /// already had is not here and was not rewritten — there is no appearance
    /// to regenerate for a value that did not move.
    pub changed: Vec<(String, String)>,
    /// Widgets left showing what they were showing, because 12.5.2's required
    /// `/Rect` is missing from them (ruling 2 degrades, ruling 10 names).
    pub skipped: Vec<SkippedWidget>,
    /// Fields a later script wrote *after* their own calculation had already
    /// run in this pass. Their stored value is the later write, not their own
    /// calculation — which is the one case where one pass and a full
    /// fixed-point disagree, so it is reported rather than iterated on.
    pub cascades_cut: Vec<String>,
}

/// The values a pass may see and the ones it has computed.
///
/// Nothing here reaches the document. `set_field` runs the same
/// [`fill::accepts_value`] the apply will run, so a value the fill layer would
/// refuse is refused *while the script is running* — which turns a rejection
/// that would have arrived after ten more scripts into one that names the
/// script that produced it.
struct StagedHost<'f> {
    fields: BTreeMap<&'f str, &'f form::Field>,
    staged: BTreeMap<String, String>,
    /// Staging order, so that the applied order is the computed order rather
    /// than alphabetical. Determinism is a contract (ruling 4) and a map's
    /// iteration order is not the order anything happened in.
    order: Vec<String>,
    /// False for a format action, which must not change data.
    writable: bool,
}

impl<'f> StagedHost<'f> {
    fn new(fields: &'f [form::Field], writable: bool) -> StagedHost<'f> {
        let mut by_name = BTreeMap::new();
        for field in fields {
            // Same-name fields are one logical field (12.7.3.2); the first
            // wins, which is the one the fill layer will also find.
            by_name.entry(field.name.as_str()).or_insert(field);
        }
        StagedHost {
            fields: by_name,
            staged: BTreeMap::new(),
            order: Vec::new(),
            writable,
        }
    }

    fn updates(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for name in &self.order {
            let Some(value) = self.staged.get(name) else {
                continue;
            };
            let was = self.fields.get(name.as_str()).map(|f| f.value.as_text());
            if was.as_deref() == Some(value.as_str()) {
                continue;
            }
            out.push((name.clone(), value.clone()));
        }
        out
    }
}

impl Host for StagedHost<'_> {
    fn field(&self, name: &str) -> Option<String> {
        if let Some(value) = self.staged.get(name) {
            return Some(value.clone());
        }
        self.fields.get(name).map(|field| field.value.as_text())
    }

    fn set_field(&mut self, name: &str, value: &str) -> bool {
        if !self.writable {
            return false;
        }
        let Some(field) = self.fields.get(name) else {
            return false;
        };
        if !fill::accepts_value(field, value) {
            return false;
        }
        if self
            .staged
            .insert(name.to_string(), value.to_string())
            .is_none()
        {
            self.order.push(name.to_string());
        }
        true
    }
}

/// The fields whose calculate action runs, in the order it runs them.
fn sequence(fields: &[form::Field], order: &[crate::object::ObjRef]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for reference in order {
        let Some(at) = fields.iter().position(|f| f.reference == *reference) else {
            continue;
        };
        if fields[at].scripts.calculate.is_some() && seen.insert(at) {
            out.push(at);
        }
    }
    // A producer that omits /CO still has a calculating form, and document
    // order is the only other order the file offers.
    for (at, field) in fields.iter().enumerate() {
        if field.scripts.calculate.is_some() && seen.insert(at) {
            out.push(at);
        }
    }
    out
}

/// Runs the form's calculate actions and applies what they produced.
///
/// # Errors
///
/// Every [`CalcError`] means nothing was written. See the module docs for why
/// one unrunnable script refuses the whole pass rather than the field it
/// belongs to.
pub fn recalculate(editor: &mut DocumentEditor) -> Result<Recalculation, CalcError> {
    let fields = editor.fields();
    let order = form::calculation_order(editor.document());
    let sequence = sequence(&fields, &order);
    if sequence.len() > limits::MAX_CALC_FIELDS {
        return Err(CalcError::TooManyFields(sequence.len()));
    }
    if sequence.is_empty() {
        return Ok(Recalculation::default());
    }

    let mut host = StagedHost::new(&fields, true);
    // One budget for the whole pass: MAX_SCRIPT_STEPS bounds a script, and
    // the document chooses how many scripts there are.
    let mut budget = Budget::new(limits::MAX_CALC_STEPS);
    let mut ran: HashSet<&str> = HashSet::new();
    let mut cascades_cut: Vec<String> = Vec::new();

    for at in sequence {
        let field = &fields[at];
        let source = match field.scripts.calculate.as_ref() {
            Some(Script::Source(text)) => text.as_str(),
            // A script too big to read is a script this build will not run a
            // prefix of.
            Some(Script::Oversize(_)) => {
                return Err(CalcError::Script {
                    field: field.name.clone(),
                    reason: ScriptError::TooLong,
                })
            }
            None => continue,
        };

        let current = host.field(&field.name).unwrap_or_default();
        // Two caps, and both bind: the smaller of what one script may spend
        // and what the pass has left. A pass with nothing left hands out a
        // budget of zero, whose first step fails.
        let left = limits::MAX_CALC_STEPS.saturating_sub(budget.used());
        let mut script_budget = Budget::new(limits::MAX_SCRIPT_STEPS.min(left));
        let outcome = script::run(source, &field.name, &current, &mut host, &mut script_budget)
            .map_err(|reason| CalcError::Script {
                field: field.name.clone(),
                reason,
            })?;
        // A thousand scripts each inside their own cap are still a thousand
        // scripts, so what one spent counts against the pass.
        budget
            .charge(script_budget.used())
            .map_err(|reason| CalcError::Script {
                field: field.name.clone(),
                reason,
            })?;

        if let Some(value) = outcome.value {
            if !host.set_field(&field.name, &value) {
                return Err(CalcError::Script {
                    field: field.name.clone(),
                    reason: ScriptError::FieldRefused,
                });
            }
        }
        ran.insert(field.name.as_str());
        for written in &outcome.wrote {
            if written != &field.name
                && ran.contains(written.as_str())
                && !cascades_cut.contains(written)
            {
                cascades_cut.push(written.clone());
            }
        }
    }

    let changed = host.updates();
    if changed.is_empty() {
        return Ok(Recalculation {
            cascades_cut,
            ..Recalculation::default()
        });
    }

    let pairs: Vec<(&str, &str)> = changed
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let skipped = editor
        .set_calculated_values(&pairs)
        .map_err(CalcError::Rejected)?;

    Ok(Recalculation {
        changed,
        skipped,
        cascades_cut,
    })
}

/// The text a field's format action would display, without changing anything.
///
/// 12.7.3.3 keeps a field's value and its appearance apart, and so does this:
/// `/V` holds the number and the format action produces what a viewer shows.
/// Writing the formatted text into `/V` would give a form whose export reads
/// "GBP 1,234.00" where a consumer expects 1234, which is the same class of
/// damage as a stale total.
///
/// `Ok(None)` means the field carries no format action, which is most fields.
///
/// The host is read-only: a format action that tries to write a field is
/// refused with [`ScriptError::FieldRefused`], because a format event changing
/// data is a defect wherever it appears.
///
/// # Errors
///
/// [`CalcError::NoSuchField`] when the form has no such field, and
/// [`CalcError::Script`] when the action would not run.
pub fn formatted_value(editor: &DocumentEditor, name: &str) -> Result<Option<String>, CalcError> {
    let fields = editor.fields();
    let Some(field) = fields.iter().find(|f| f.name == name) else {
        return Err(CalcError::NoSuchField);
    };
    let source = match field.scripts.format.as_ref() {
        Some(Script::Source(text)) => text.as_str(),
        Some(Script::Oversize(_)) => {
            return Err(CalcError::Script {
                field: field.name.clone(),
                reason: ScriptError::TooLong,
            })
        }
        None => return Ok(None),
    };

    let mut host = StagedHost::new(&fields, false);
    let mut budget = Budget::new(limits::MAX_SCRIPT_STEPS);
    let current = field.value.as_text();
    let outcome =
        script::run(source, &field.name, &current, &mut host, &mut budget).map_err(|reason| {
            CalcError::Script {
                field: field.name.clone(),
                reason,
            }
        })?;
    Ok(outcome.value)
}
