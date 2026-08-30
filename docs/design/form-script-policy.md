# An execution policy for a form's scripts

A form carries four kinds of field script and two kinds of document script, and until this
work three of the six were surfaced as source and run nowhere. **All six now have a policy,
and it is a type.** `ScriptPolicy` names the trigger classes a host allows; `recalculate`,
`formatted_value`, and the new `keystroke` and `validate` entry points take one; a denied
trigger is a refusal that names what it refused rather than a pass that quietly did nothing.
The document-level scripts of 7.7.4 are read into a name table of function definitions, which
is what makes an ordinary generated form computable at all — before it, the first call to a
form's own helper raised `ScriptError::UnknownName` and refused the whole pass. And the one
rule this feature had been holding up by arrangement rather than by construction — a format
action's display string must never become `/V` (12.7.3.3) — is now a type no write door
accepts, proved by compiling the mistake and asserting `error[E0308]`.

Everything here is in [features/forms.md](../features/forms.md), which is the record. This
document is the reasoning: the alternatives that were rejected, what the counted injections
measured, and the two places where a guard turned out to hold nothing and is written down
saying so.

## Scope

- A policy type over the six trigger classes of 12.6.3 tables 198 and 200 and 7.7.4, threaded
  through every entry point that runs a script.
- Document-level scripts parsed into callable helpers, which means function **definition** and
  **call** added to the subset, bounded by the caps that were already there.
- Keystroke and validate as entry points a host calls with an event it built, answering a
  verdict rather than writing anything.
- One script-source budget per read of a document, rather than one per walk.
- `DisplayString`, and the compile-refusal proof under it.

## Non-goals

- **Running a catalog action.** `WC`, `WS`, `DS`, `WP` and `DP` are will-close, will-save,
  did-save, will-print and did-print. Every one needs an event a document reader has no notion
  of, and none of them is document-open — which is the one `/Names /JavaScript` covers. The
  trigger class exists in the policy because it names a real class a host has to be able to
  deny; nothing in this build consumes it. Decision 7 below is about saying so rather than
  letting it be assumed.
- **A general ECMAScript.** The subset stays a subset. `eval`, `try`, `switch`, `for...in`,
  `typeof`, `delete`, `with`, `class`, `let`/`const`, `import`/`export`, regular expressions,
  object literals and prototypes are refused exactly as they were, and a helper's body is
  parsed by the same productions a calculate action's is, so nothing can arrive through one
  that could not arrive through the other.
- **Automatic recalculation.** When a calculation runs is a host's policy, so `recalculate()`
  stays an explicit call. That was true before this work and is not changed by it.

## Decisions

**1. Deny by default, except `Calculate` and `Format` — and that default is the shipped
behaviour written down, not a judgement about safety.**

The alternative was to deny all six and make every caller opt in. It was rejected because
`recalculate()` and `formatted_value()` have run calculate and format actions since the
interpreter landed: a type that silently turned an existing capability off would be a breaking
change wearing a safety argument, and a caller who upgraded would find their totals stopped
computing with a refusal they never asked for. The other four are denied because nothing ran
them before the type existed, and a default that starts running document program text on the
strength of a new struct is exactly the change nobody reviews.

The exit criterion followed from this directly: the whole of `form_calculations.rs` had to pass
with **no assertion edited**, and it does.

**2. A denied trigger is a named refusal, never a silent skip — and it is raised only against a
script that is there.**

`CalcError::Refused { trigger, subject }` carries the class and the field. The alternative — a
denied trigger simply producing an empty result — was rejected under ruling 10: a pass that
quietly ran nothing is indistinguishable from a pass over a form that carries no scripts, and
those are different documents. The second half matters as much as the first. A form with no
calculate action recalculates to the same empty answer under every policy, because refusing an
absent script would collapse "this host does not run calculations" into "this document has
none", which is the confusion the refusal was added to prevent.

**3. `/Names /JavaScript` yields a name table of function definitions and nothing else, and
denying `Document` gives an empty table rather than a refusal.**

Two decisions that only make sense together.

The first: anything at document scope that is not a `function` declaration is
`ScriptError::NotADefinition`, named and refusing the pass. Skipping it was the alternative and
is worse than it looks — a skipped `var` builds a name table silently missing whatever the
statement would have defined, and a calculation running against a half-built table of helpers
is the form that lies this whole subset is written against. `var doc = this;` at document scope
is common, and under an allowed `Document` trigger it refuses. That is deliberate.

The second: denying the trigger must **not** be `CalcError::Refused`. A document-level script
is not something the pass is asked to run; it is a resource the pass may consult. Refusing
would make every form that carries a `/Names /JavaScript` block uncomputable under the default
policy, which is most of them. So the table is empty and a call to a helper is the
`UnknownName` it always was — which is also precisely the behaviour that existed before this
milestone, and is asserted as such.

Definition is document scope's production alone. `function` stays reserved everywhere a field
script is parsed, so a calculate action gained the ability to **call** a helper and never to
declare one. One place declares, and it is the one the policy gates.

**4. Recursion is refused by name, not bounded by a counter.**

`ScriptError::Recursion`, raised when a helper already on the call stack is called again,
directly or through another. A depth cap was the alternative and was rejected for the same
reason the cascade rule rejected bounded re-entry: it makes the answer depend on a number
nobody can predict from the file, so the same document computes one total at depth 8 and a
different one at depth 16. With recursion refused, a call chain is at most as long as the table
has distinct functions, which `MAX_SCRIPT_FUNCTIONS` caps at 256; the evaluator's own stack is
bounded by the same `MAX_SCRIPT_DEPTH` the parser counts with, because this recursion is one
the parser cannot see.

**5. The format string cannot reach `/V`, and a type says so rather than an arrangement of
code.**

Before this, 12.7.3.3 held because `formatted_value` happened not to call the editor. The first
caller to write `editor.set_field_value(name, formatted)` would have produced a `/V` of
"GBP 1,234.00" and nothing in the workspace would have objected. `DisplayString` has no
`Deref`, no `Into<String>` and no constructor outside `calc.rs`, and all four write doors take
`&str`.

The alternative that was considered and rejected is a type with **no** way out at all. A caller
has to be able to draw the characters, so `DisplayString::text` exists and
`set_field_value(name, formatted.text())` compiles. That is not a hole in the guarantee; it is
the guarantee's shape. No type stops somebody who means it, and what this one removes is the
mistake nobody makes on purpose — `.text()` is a spelling a reviewer can see. The doc says so
in those words rather than claiming more than the type does.

The proof compiles a **caller**, not the crate, because the defect lives at a call site. That
is the one structural difference from the css and layout compile-refusal proofs, which inject a
non-exhaustive `match` *inside* the crate they compile.

**6. A validate action that refuses a computed value aborts the whole pass; a validation the
policy declined to run is named in the result.**

12.7.2 puts a computed value through the field's own validate action before it is committed,
and the pass does that at the last moment nothing has been written. A refusal is
`CalcError::Invalid`, naming the field and the value, and the pass writes nothing at all. The
alternative — refusing only that field and writing the rest — is the outcome this module exists
to prevent: a form whose validation rejects one total and whose other nine were written anyway
is a document that disagrees with itself.

Under a policy that denies validation the pass still runs, and `Recalculation::refused` names
every field it wrote **without** checking. This is ruling 10 applied to a check rather than to a
repair, and it is what stops the `Validate` bit being a switch with nothing behind it: a pass
that silently skipped a form's own validation reads exactly like a pass over a form that has
none, and the difference is whether the numbers now in the document were ever looked at.

Event scripts run against the read-only staged host a format action gets, so one that tries to
write a field is `ScriptError::FieldRefused`. The `!writable` path in `StagedHost::set_field` is
**used**, not changed.

**7. A trigger with no consumer is written down and asserted, not quietly carried.**

`Trigger::Catalog` gates nothing in this build. The honest options were to leave it out of the
enum or to keep it and say so. It is kept — it names a real trigger class a host has to be able
to deny, `ScriptSummary` already counts catalog actions, and the C ABI projection is 1:1 —
and `form_document_scripts.rs::allowing_the_catalog_trigger_changes_no_answer` asserts that
allowing it changes no answer anywhere. A guard that holds nothing is a measurement here rather
than an assumption a future reader has to make.

## Design

Six types, and none of them knows what a PDF is except the two in `calc.rs` that have to.

| Type | Where | What it is |
| --- | --- | --- |
| `Trigger` | `script.rs` | the six classes: `Calculate`, `Format`, `Keystroke`, `Validate`, `Document`, `Catalog` |
| `ScriptPolicy` | `script.rs` | which of them a host allows. `default()`, `nothing()`, `everything()`, `allow`, `deny`, `allows` |
| `ScriptScope` | `script.rs` | the document's helpers, as a name table `define` fills from one source at a time |
| `Event` | `script.rs` | what the `event` object holds at the start of a run: `value`, `change`, the selection, `willCommit` |
| `Keystroke` | `calc.rs` | the PDF-facing half of an event — what a host offers. The field's own value is not in it, because the editor knows it |
| `EventVerdict` | `calc.rs` | `Accepted(text)` or `Refused`. A refusal is the action working |
| `DisplayString` | `calc.rs` | what a format action produced, in a type no write door takes |
| `ScriptBudget` | `form.rs` | the 4 MiB of surfaced source, as a value the caller carries |

`script::run` keeps its signature; `run_in` adds a scope and `run_event` adds a whole event,
with `run` and `run_in` defined in terms of the one below them so there is a single interpreter
entry rather than three.

**One budget, not three.** `MAX_SCRIPT_TOTAL` was handed out afresh by each of the three walks
that surface a document's scripts — the field tree's `/AA`, `/Names /JavaScript` and the
catalog's `/AA` — so a file that filled all three surfaced twelve mebibytes against a cap that
says four. `ScriptBudget` is that total as a value: `fields_within`,
`document_scripts_within` and `catalog_scripts_within` spend one between them, in that fixed
order because which scripts come back as source and which as `Script::Oversize` depends on it
and determinism is a contract (ruling 4). A budget living inside `CosDocument` was rejected: it
would make reading a document mutate it, and the same document read twice answer differently.

## Milestones

| # | What | Exit criterion | Size |
| --- | --- | --- | --- |
| 1 | `ScriptPolicy`, `Trigger`, threaded into `recalculate` and `formatted_value` | **Done.** `form_calculations.rs` passes with no assertion edited; the six defaults flipped one at a time and counted in its header | M |
| 2 | One `ScriptBudget` across the three walks | **Done.** `form_script_budget.rs` crowds all three surfaces and asserts four mebibytes are spent once between them | S |
| 3 | Document-level scripts as a name table, under policy | **Done.** `form_document_scripts.rs` asserts both directions in one test — a form computes through its own helpers, and fails with `UnknownName` without them | L |
| 4 | Keystroke and validate as entry points | **Done.** `form_events.rs`; `Recalculation` gains `refused`; `StagedHost`'s `!writable` path unchanged | M |
| 5 | `DisplayString` and the compile-refusal proof | **Done.** `display_string_does_not_reach_v.rs`, four doors injected separately, `error[E0308]` each, with a pristine control | M |
| 6 | Facade and C ABI projection (ruling 11) | **Done.** `tpdf_editor_recalculate`, `_formatted_value`, `_keystroke`, `_validate` and the `tpdf_recalculation_*` accessors; `cargo xtask bindings-parity` green | M |

## What the counted injections measured

Each of the six policy defaults was flipped on its own and the three suites run against it.
These are measurements, not expectations, and they are repeated in each suite's own header.

| Default flipped | `form_calculations` | `form_document_scripts` | `form_events` |
| --- | --- | --- | --- |
| `calculate` allowed → denied | 19 of 29 | 10 of 13 | 4 of 13 |
| `format` allowed → denied | 3 of 29 | 1 of 13 | 0 of 13 |
| `document` denied → allowed | 0 of 29 | 3 of 13 | 0 of 13 |
| `keystroke` denied → allowed | 0 of 29 | 0 of 13 | 1 of 13 |
| `validate` denied → allowed | 0 of 29 | 0 of 13 | 2 of 13 |
| `catalog` denied → allowed | 0 of 29 | 0 of 13 | 0 of 13 |

Three things this says that a pass/fail summary would not.

**The `catalog` row is zero everywhere, and always will be.** That is decision 7, measured.

**The `keystroke` and `validate` rows are 1 and 2, not more.** Most of `form_events.rs` passes
an explicit policy, so a change to the *default* cannot reach it — which is the right shape for
a suite about entry points a host calls deliberately, and the two tests that do move are the
ones about the default itself. A reader who expected larger numbers there would be expecting
the wrong thing, so the number is explained where it is recorded rather than left to look thin.

**At milestone 1 the last four rows were all zero**, because nothing yet ran those triggers.
The table was written down at zero anyway, and milestones 3 and 4 are what moved two of them.
A guard that fires nothing is worth recording precisely because the next reader would otherwise
assume it fires something.

## What this changed that was not in its brief

**11.8.5.** Two strings compare code unit by code unit and only otherwise by number; this
engine compared every relational operator by number. It was found by writing the commonest
keystroke script there is — `event.change >= '0' && event.change <= '9'` — under which `'!'` is
`NaN`, every comparison against it is false, and a digits-only field silently accepts
everything. Fixed, with the ordering caveat stated in the source: comparison is by code point
rather than by UTF-16 code unit, and the two differ only between an astral character and
U+E000..U+FFFF.

**A catalog cannot hold three mebibytes.** Milestone 2's brief asked for a fixture with three
mebibytes of source in each of the three places. 12.6.3 table 200 defines exactly five triggers
and `MAX_SCRIPT_LEN` caps each at 64 KiB, so the catalog's ceiling is 320 KiB whatever a file
does — it was never the surface that could triple the total on its own. The fixture puts the
pressure where a real file can put it and lets the catalog read last with nothing left, which
is a sharper assertion than a catalog that fits.

## Risks

**A form that this build now runs more of.** Allowing `Document` runs program text the previous
build only surfaced. That is the point, and it is why the default denies it: the decision to
run a document's own functions belongs to a host that knows where the document came from. The
bounds are the ones that were already there — depth, steps and size, shared across the pass —
and the fuzz target drives the new entry points under a deliberately small budget so a path
that forgot to charge a step fails the target rather than merely running slowly.

**`NotADefinition` is strict, and real files will meet it.** A `/Names /JavaScript` block
holding `var doc = this;` refuses the pass under an allowed `Document` trigger. If the corpus
says that shape is common enough to matter, the answer is not to skip the statement silently —
decision 3 is about exactly that — but to consider a narrower production for the few statement
forms that are provably inert. That would be a new decision, taken against measured evidence
(ruling 3), not a loosening of this one.

**`Recalculation::refused` can be ignored.** A caller that drops the value writes computed
numbers no validation ever looked at, and nothing forces them to read it. This is the same
shape as `Recalculation::skipped` and `FillReport`, and the answer is the same one ruling 10
gives: the engine's job is that the fact is available and named, not that the caller acts on
it. The C ABI carries it as `tpdf_recalculation_refused_count` for that reason.
