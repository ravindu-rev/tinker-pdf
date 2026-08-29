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
use crate::form::{self, Script, ScriptBudget};
use crate::limits;
use crate::script::{self, Budget, Host, ScriptError, ScriptPolicy, ScriptScope, Trigger};

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
    /// A field's own `/AA /V` validate action refused the value the pass
    /// computed for it (12.6.4.16 table 196).
    ///
    /// Refuses the **whole** pass, not the field. A form whose validation
    /// rejects one total and whose other nine totals were written anyway is a
    /// document that disagrees with itself, which is the outcome this
    /// module's all-or-nothing contract exists to prevent — and a validate
    /// action saying no is the form working, so the answer is a refusal with
    /// the value in it rather than a repair.
    Invalid {
        /// The field whose action refused.
        field: String,
        /// The value it refused.
        value: String,
    },
    /// A document-level script (7.7.4) would not read as a name table of
    /// function definitions. Names the tree key it came under and why.
    ///
    /// Refuses the pass, for the reason an unrunnable field script does: a
    /// calculation running against a half-built table of helpers is a form
    /// whose totals disagree with its own definitions.
    DocumentScript {
        /// The `/Names /JavaScript` key.
        name: String,
        /// Why it would not read.
        reason: ScriptError,
    },
    /// The [`ScriptPolicy`] this pass ran under does not allow that trigger
    /// class, and there was a script of that class to run.
    ///
    /// A refusal rather than a skip, for the reason ruling 10 exists: a pass
    /// that quietly ran nothing is indistinguishable from a form that carries
    /// no scripts, and a caller cannot tell "this document does not compute"
    /// from "this host did not let it".
    Refused {
        /// Which class was refused.
        trigger: Trigger,
        /// What carried it: a field's fully qualified name (12.7.3.2), or a
        /// document-level script's name-tree key.
        subject: String,
    },
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
            CalcError::Refused { trigger, subject } => {
                write!(f, "{subject}: the policy does not run {trigger} scripts")
            }
            CalcError::DocumentScript { name, reason } => {
                write!(f, "document-level script {name}: {reason}")
            }
            CalcError::Invalid { field, value } => {
                write!(f, "{field}: the form refused the value {value}")
            }
        }
    }
}

/// A keystroke offered to a field's `/AA /K` action (12.6.4.16 table 196).
///
/// The value the event runs against is the field's own and is not here: the
/// editor knows it, and asking a caller for it twice invites two answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keystroke {
    /// `event.change` — the text being inserted. Empty for a deletion.
    pub change: String,
    /// `event.selStart` and `event.selEnd`, in that order.
    pub selection: (i64, i64),
    /// `event.willCommit` — whether this is the commit at the end of typing
    /// rather than one keystroke inside it.
    pub will_commit: bool,
}

/// What a keystroke or validate action decided.
///
/// A refusal is the action **working**, not an error: 12.6.4.16 table 196's
/// `event.rc` is how a form says "not that value", and a form that rejects a
/// date in the wrong century is doing its job. Errors are for scripts that
/// could not run at all, and they are [`CalcError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventVerdict {
    /// The action accepted. For a keystroke this is `event.change` as the
    /// script left it — a keystroke action may rewrite what is being typed —
    /// and for a validate it is `event.value`.
    Accepted(String),
    /// The action set `event.rc = false`. The value is refused whole.
    Refused,
}

impl EventVerdict {
    /// Whether the action accepted.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(self, EventVerdict::Accepted(_))
    }

    /// The accepted text, or `None` for a refusal.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            EventVerdict::Accepted(text) => Some(text),
            EventVerdict::Refused => None,
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
    /// Fields whose computed value was written **without** their own `/AA /V`
    /// validate action being consulted, because the policy denies
    /// [`Trigger::Validate`].
    ///
    /// Ruling 10, applied to a check rather than to a repair: a pass that
    /// silently skipped a form's own validation would be indistinguishable
    /// from a pass over a form that has none, and the difference is whether
    /// the numbers now in the document were ever checked. Empty under a
    /// policy that allows validation — where a refusal aborts the pass
    /// instead ([`CalcError::Invalid`]) — and empty for a form whose
    /// calculated fields carry no validate action.
    pub refused: Vec<String>,
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

/// The helpers a pass may call, read out of `/Names /JavaScript` (7.7.4).
///
/// **This is the door a real calculating form was failing at.** A generated
/// form keeps its arithmetic in document-level functions and its `/AA /C`
/// scripts call them, so the interpreter met the first call, raised
/// [`ScriptError::UnknownName`] and refused the whole pass — a correctly
/// authored file this build could not compute.
///
/// Denying [`Trigger::Document`] gives an empty table rather than a refusal,
/// and that distinction is deliberate. A document-level script is not
/// something the pass is *asked* to run; it is a resource the pass may
/// consult. Refusing here would make every form that carries a
/// `/Names /JavaScript` block uncomputable under the default policy, which is
/// most of them — so under the default the table is empty and a call to a
/// helper is the `UnknownName` it always was.
///
/// The budget is the field walk's own, continued: one read of a document,
/// one [`ScriptBudget`].
fn helpers(
    editor: &DocumentEditor,
    policy: ScriptPolicy,
    budget: &mut ScriptBudget,
) -> Result<ScriptScope, CalcError> {
    let mut scope = ScriptScope::empty();
    if !policy.allows(Trigger::Document) {
        return Ok(scope);
    }
    for entry in form::document_scripts_within(editor.document(), budget) {
        let source = match &entry.script {
            Script::Source(text) => text.as_str(),
            // A script too big to read is a table this build will not build
            // part of.
            Script::Oversize(_) => {
                return Err(CalcError::DocumentScript {
                    name: entry.name.clone(),
                    reason: ScriptError::TooLong,
                })
            }
        };
        scope
            .define(source)
            .map_err(|reason| CalcError::DocumentScript {
                name: entry.name.clone(),
                reason,
            })?;
    }
    Ok(scope)
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

/// Runs the form's calculate actions and applies what they produced, under
/// the default [`ScriptPolicy`].
///
/// # Errors
///
/// Every [`CalcError`] means nothing was written. See the module docs for why
/// one unrunnable script refuses the whole pass rather than the field it
/// belongs to.
pub fn recalculate(editor: &mut DocumentEditor) -> Result<Recalculation, CalcError> {
    recalculate_under(editor, ScriptPolicy::default())
}

/// The same pass, under a policy the host chose.
///
/// The policy is consulted per trigger class and only where there is a script
/// of that class to run: a form with no calculate action answers
/// [`Recalculation::default`] under every policy, because refusing an absent
/// script would make "this host does not run calculations" and "this document
/// has none" the same answer, which is the confusion the refusal exists to
/// prevent.
///
/// # Errors
///
/// [`CalcError::Refused`] when the policy denies a trigger class this form
/// carries, and every other [`CalcError`] for the reasons
/// [`recalculate`] gives. All of them mean nothing was written.
pub fn recalculate_under(
    editor: &mut DocumentEditor,
    policy: ScriptPolicy,
) -> Result<Recalculation, CalcError> {
    // One read of the document: the field tree's `/AA` and, if the policy
    // allows it, `/Names /JavaScript` share this budget rather than starting
    // from the total apiece.
    let mut bytes = ScriptBudget::new();
    let fields = editor.fields_within(&mut bytes);
    let order = form::calculation_order(editor.document());
    let sequence = sequence(&fields, &order);
    if sequence.len() > limits::MAX_CALC_FIELDS {
        return Err(CalcError::TooManyFields(sequence.len()));
    }
    if sequence.is_empty() {
        return Ok(Recalculation::default());
    }
    // The policy is asked once the form is known to carry a calculate action,
    // and named against the first field that has one — a refusal with no
    // subject is a refusal nobody can act on (ruling 10).
    if !policy.allows(Trigger::Calculate) {
        let subject = sequence
            .first()
            .and_then(|at| fields.get(*at))
            .map_or_else(String::new, |field| field.name.clone());
        return Err(CalcError::Refused {
            trigger: Trigger::Calculate,
            subject,
        });
    }

    let scope = helpers(editor, policy, &mut bytes)?;

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
        let outcome = script::run_in(
            source,
            &field.name,
            &current,
            &mut host,
            &mut script_budget,
            &scope,
        )
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

    // 12.7.2: a computed value goes through the field's own validate action
    // before it is committed, and this is the last moment nothing has been
    // written. A refusal here aborts the whole pass rather than the field,
    // because a form whose validation rejects one total and whose other nine
    // were written anyway is a document that disagrees with itself.
    let refused = validate_computed(&fields, &changed, &scope, policy, &mut budget, &mut host)?;

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
        refused,
    })
}

/// Runs each changed field's `/AA /V` action against the value the pass
/// computed for it, and answers the ones that were never consulted.
///
/// Two outcomes and no third: the action accepts, or the pass is over. What
/// comes back is the list of fields whose validation the **policy** declined
/// to run — written unchecked, and said so rather than left to be assumed
/// (ruling 10).
fn validate_computed(
    fields: &[form::Field],
    changed: &[(String, String)],
    scope: &ScriptScope,
    policy: ScriptPolicy,
    budget: &mut Budget,
    host: &mut StagedHost<'_>,
) -> Result<Vec<String>, CalcError> {
    // The pass is done computing, so the staging map is sealed for the rest
    // of it: a validate action that tries to write a field takes the same
    // `!writable` path a format action does and stops with
    // `ScriptError::FieldRefused`. It still *reads* the staged values, which
    // is the whole reason this runs against the pass's own host rather than a
    // fresh one — a total is validated against the inputs that produced it.
    host.writable = false;
    let mut refused = Vec::new();
    for (name, value) in changed {
        let Some(field) = fields.iter().find(|f| &f.name == name) else {
            continue;
        };
        let source = match field.scripts.validate.as_ref() {
            Some(Script::Source(text)) => text.as_str(),
            Some(Script::Oversize(_)) => {
                return Err(CalcError::Script {
                    field: name.clone(),
                    reason: ScriptError::TooLong,
                })
            }
            None => continue,
        };
        if !policy.allows(Trigger::Validate) {
            refused.push(name.clone());
            continue;
        }

        let left = limits::MAX_CALC_STEPS.saturating_sub(budget.used());
        let mut script_budget = Budget::new(limits::MAX_SCRIPT_STEPS.min(left));
        let event = script::Event {
            value: value.clone(),
            ..script::Event::default()
        };
        let outcome = script::run_event(source, name, &event, host, &mut script_budget, scope)
            .map_err(|reason| CalcError::Script {
                field: name.clone(),
                reason,
            })?;
        budget
            .charge(script_budget.used())
            .map_err(|reason| CalcError::Script {
                field: name.clone(),
                reason,
            })?;
        if !outcome.accepted {
            return Err(CalcError::Invalid {
                field: name.clone(),
                value: value.clone(),
            });
        }
    }
    Ok(refused)
}

// ---------------------------------------------------------------------------
// The two events that need one
// ---------------------------------------------------------------------------

/// Offers a keystroke to a field's `/AA /K` action (12.6.4.16 table 196).
///
/// Keystroke and validate could never run implicitly, and that is why they sat
/// surfaced-and-never-run for so long: both need an **event** — what is being
/// typed, where, and whether this is the commit — and a reader has no typing
/// to report. So they are entry points a host calls with an event it built,
/// and nothing in a recalculation reaches the keystroke one at all.
///
/// `Ok(EventVerdict::Accepted)` with the change unchanged is the answer for a field
/// that carries no keystroke action, because there is nothing to consult and
/// the keystroke stands.
///
/// # Errors
///
/// [`CalcError::NoSuchField`], [`CalcError::Refused`] when the policy denies
/// [`Trigger::Keystroke`] and the field carries one, and
/// [`CalcError::Script`] when the action would not run. A script that sets
/// `event.rc = false` is not an error — that is [`EventVerdict::Refused`].
pub fn keystroke(
    editor: &DocumentEditor,
    name: &str,
    event: &Keystroke,
    policy: ScriptPolicy,
) -> Result<EventVerdict, CalcError> {
    event_action(editor, name, Trigger::Keystroke, event.clone(), policy)
}

/// Offers a committed value to a field's `/AA /V` validate action
/// (12.6.4.16 table 196).
///
/// `Ok(EventVerdict::Accepted(value))` for a field that carries no validate action:
/// a form that does not check a value has accepted it.
///
/// # Errors
///
/// The same set [`keystroke`] returns, with [`Trigger::Validate`] in the
/// refusal.
pub fn validate(
    editor: &DocumentEditor,
    name: &str,
    value: &str,
    policy: ScriptPolicy,
) -> Result<EventVerdict, CalcError> {
    let event = Keystroke {
        change: value.to_string(),
        ..Keystroke::default()
    };
    event_action(editor, name, Trigger::Validate, event, policy)
}

/// The one implementation behind both events.
///
/// The host is **read-only** — `StagedHost::new(&fields, false)`, the same
/// door a format action gets — so an event script that tries to write a field
/// is [`ScriptError::FieldRefused`]. A keystroke that changes data as a side
/// effect of being typed is a defect wherever it appears, and this module's
/// only way into the document is the all-or-nothing apply a recalculation
/// makes.
fn event_action(
    editor: &DocumentEditor,
    name: &str,
    trigger: Trigger,
    event: Keystroke,
    policy: ScriptPolicy,
) -> Result<EventVerdict, CalcError> {
    let mut bytes = ScriptBudget::new();
    let fields = editor.fields_within(&mut bytes);
    let Some(field) = fields.iter().find(|f| f.name == name) else {
        return Err(CalcError::NoSuchField);
    };
    let current = field.value.as_text();
    let carried = match trigger {
        Trigger::Keystroke => field.scripts.keystroke.as_ref(),
        _ => field.scripts.validate.as_ref(),
    };
    let source = match carried {
        Some(Script::Source(text)) => text.as_str(),
        Some(Script::Oversize(_)) => {
            return Err(CalcError::Script {
                field: field.name.clone(),
                reason: ScriptError::TooLong,
            })
        }
        // Nothing to consult: a keystroke stands as it was offered, and a
        // value nothing checks has been accepted.
        None => return Ok(EventVerdict::Accepted(event.change)),
    };
    if !policy.allows(trigger) {
        return Err(CalcError::Refused {
            trigger,
            subject: field.name.clone(),
        });
    }
    let scope = helpers(editor, policy, &mut bytes)?;

    // 12.6.4.16 table 196: a validate action's `event.value` is the value
    // being committed, and a keystroke's is the field as it stands with
    // `event.change` holding what is being typed into it.
    let raw = match trigger {
        Trigger::Keystroke => script::Event {
            value: current,
            change: event.change.clone(),
            selection: event.selection,
            will_commit: event.will_commit,
        },
        _ => script::Event {
            value: event.change.clone(),
            ..script::Event::default()
        },
    };

    let mut host = StagedHost::new(&fields, false);
    let mut budget = Budget::new(limits::MAX_SCRIPT_STEPS);
    let outcome = script::run_event(source, &field.name, &raw, &mut host, &mut budget, &scope)
        .map_err(|reason| CalcError::Script {
            field: field.name.clone(),
            reason,
        })?;
    if !outcome.accepted {
        return Ok(EventVerdict::Refused);
    }
    Ok(EventVerdict::Accepted(match trigger {
        // A keystroke action may rewrite what is being typed, which is how
        // every "digits only" field in the wild works.
        Trigger::Keystroke => outcome.change.unwrap_or(event.change),
        _ => outcome.value.unwrap_or(event.change),
    }))
}

/// The text a format action produced, in a type no write door will take.
///
/// 12.7.3.3 keeps a field's value and its appearance apart, and until this
/// type existed that was held up by [`formatted_value`] simply not calling
/// the editor — a property of how the code happened to be arranged rather
/// than a rule anything enforced. The first caller to write
/// `editor.set_field_value(name, formatted)` would have produced a form whose
/// `/V` reads "GBP 1,234.00" where a consumer expects 1234, and nothing would
/// have objected.
///
/// So it is a newtype with **no `Deref`, no `Into<String>` and no constructor
/// outside this module**: every door into the document —
/// `DocumentEditor::set_field_value`, `fill_field`, `set_field_values`,
/// `set_calculated_values` — takes `&str`, and one of these cannot be spelled
/// as one. `crates/tinker-pdf-cos/tests/display_string_does_not_reach_v.rs`
/// compiles that mistake and asserts `error[E0308]`.
///
/// [`DisplayString::text`] is the way out, and it is deliberately a *spelling*
/// rather than a coercion. A caller who genuinely wants the characters — to
/// draw them, to log them — writes `.text()`, and a caller who writes
/// `.text()` into `/V` has made a decision a reviewer can see. What the type
/// removes is the mistake nobody makes on purpose.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DisplayString(String);

impl DisplayString {
    /// The characters, for showing.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for DisplayString {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The text a field's format action would display, without changing anything.
///
/// 12.7.3.3 keeps a field's value and its appearance apart, and so does the
/// return type: `/V` holds the number and the format action produces what a
/// viewer shows, in a [`DisplayString`] no write door accepts.
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
pub fn formatted_value(
    editor: &DocumentEditor,
    name: &str,
) -> Result<Option<DisplayString>, CalcError> {
    formatted_value_under(editor, name, ScriptPolicy::default())
}

/// The same display string, under a policy the host chose.
///
/// As in [`recalculate_under`], the policy is asked only where there is a
/// format action to refuse: a field carrying none answers `Ok(None)` under
/// every policy.
///
/// # Errors
///
/// [`CalcError::Refused`] when the policy denies [`Trigger::Format`] and the
/// field carries one, plus everything [`formatted_value`] can return.
pub fn formatted_value_under(
    editor: &DocumentEditor,
    name: &str,
    policy: ScriptPolicy,
) -> Result<Option<DisplayString>, CalcError> {
    let mut bytes = ScriptBudget::new();
    let fields = editor.fields_within(&mut bytes);
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
    if !policy.allows(Trigger::Format) {
        return Err(CalcError::Refused {
            trigger: Trigger::Format,
            subject: field.name.clone(),
        });
    }

    // A format action calls the same document-level helpers a calculate
    // action does, and gates on the same trigger.
    let scope = helpers(editor, policy, &mut bytes)?;

    let mut host = StagedHost::new(&fields, false);
    let mut budget = Budget::new(limits::MAX_SCRIPT_STEPS);
    let current = field.value.as_text();
    let outcome = script::run_in(
        source,
        &field.name,
        &current,
        &mut host,
        &mut budget,
        &scope,
    )
    .map_err(|reason| CalcError::Script {
        field: field.name.clone(),
        reason,
    })?;
    // The one constructor, and it is here rather than on the type: a
    // `DisplayString` a caller could build is a `DisplayString` that means
    // nothing about where the text came from.
    Ok(outcome.value.map(DisplayString))
}
