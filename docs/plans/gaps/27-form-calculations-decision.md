# Form calculations: the JavaScript question

A decision, not an implementation. `/AA` action dictionaries carry the scripts
that make a form's totals add up. The engine reads the field tree, fills
fields and regenerates appearances; it does not run the scripts, so a
calculated total shows whatever was last saved. What to do about that has been
open since plan 11 was written. (S — the decision)

## What is wrong

Nothing, exactly. `/AA`, `/CO` and `/Names /JavaScript` are simply not read.
A form fills correctly and its computed fields do not recompute.

## What the rulings already decide

**Ruling 3 rules out a JavaScript crate.** Everything that touches PDF data is
hand-rolled; `boa` and its peers are out, and that is settled rather than open.

So the actual question is narrower than "how should we run JavaScript": it is
whether to *write* an interpreter, surface the scripts as data, or do nothing.

## The options

### A. A hand-rolled ECMAScript subset

Number and string literals, arithmetic, comparison, `if`/`else`, `var`, member
access, function calls, the `event` object (`event.value`, `event.target`),
and the Acrobat helpers: `AFSimple_Calculate`, `AFNumber_Format`,
`AFPercent_Format`, `AFDate_Format`, `AFSpecial_Format`.

Roughly 1,200 lines. Covers the large majority of real `/AA` scripts, because
most of them are one call to `AFSimple_Calculate` with a field list, or a
short arithmetic expression over `getField(...).value`.

Explicitly not supported, and the boundary is the point: no loops beyond a
bounded count, no `eval`, no DOM, no file or network access, no `app.*` beyond
inert stubs. A form script is untrusted input from a document.

### B. Surface the scripts as data

Read `/AA`, `/CO` (the calculation order) and `/Names /JavaScript`, expose
them on the field model, warn that they were not run, and stop. Roughly 150
lines.

A host that wants calculation can run it — Tinker has a JavaScript runtime
available to it in a way the engine does not — and a host that does not gets
an honest report rather than a silently stale total.

### C. Nothing

Current behaviour. The scripts are invisible.

## Recommendation

**B.**

Three reasons, in order of weight.

**It contributes nothing to the parity bar.** Tinker's engine seam is eleven
functions — open, page count, metadata, permissions, geometry, encryption
info, render, text, search, outline. None runs `/AA`. Option A is 1,200 lines
that move the parity needle by zero.

**A half-implementation is worse than none.** A script that sets three fields
and then submits leaves a file that *looks* filled and is wrong. Unless the
writes are transactional — all fields updated or none — a partial calculation
produces a document whose totals disagree with its inputs, which is the worst
possible outcome for a form. Option B cannot produce a wrong answer because it
does not produce an answer.

**The scripts are more useful as data than as behaviour** for the host this
engine exists to serve. Tinker can decide policy — run them, show them, refuse
them — which is a decision an engine should not make on a host's behalf.

Option A stays available. If a corpus or a user says otherwise, B is the
foundation it would be built on rather than work that gets thrown away.

## Where a half-implementation is worse than none

Stated above and worth repeating as the deciding factor: **non-transactional
writes**. A calculation that partially succeeds is a form that lies. Option A,
if ever taken, must apply all field updates atomically or none — which is a
constraint on the *host* interface as much as on the interpreter, and is the
part most likely to be skipped.

## If B is chosen — scope

- Read `/AA` on fields and on the document catalog.
- Read `/CO`, the calculation order array.
- Read `/Names /JavaScript`, the document-level scripts.
- Expose them on `Field` as source text, unexecuted.
- Warn once per document that scripts were present and not run.

## Milestones

For option B.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `/AA`, `/CO` and `/Names /JavaScript` read and exposed | A form with a calculated total reports the script text and the calculation order; a form without reports neither | S |
| 2 | The warning | A document with scripts warns once, naming how many fields carry them | S |
| 3 | Honest reporting | STATUS says "surfaced, not executed", and plan 11's open question is closed with this decision recorded | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** closing plan 11's open question, which is one of the three
decisions [28](28-tinker-integration-decisions.md) is also waiting on.

## Risks

| Risk | Mitigation |
| --- | --- |
| The decision is deferred again and the question stays open indefinitely | This document is the decision; recording B closes it, and A remains available on evidence |
| A host runs the surfaced scripts unsafely | The engine surfaces text and says so; sandboxing is the host's problem and the engine should not imply otherwise |

## As built — option A, against this document's own recommendation (August 2026)

**Both options were built, in that order, and A was chosen.** This document
recommends B. The owner chose A. What changed is not the argument — it is one
of the argument's premises.

### Why the recommendation no longer holds

The deciding paragraph above is *"a half-implementation is worse than none"*,
and its own last clause says why: unless the writes are transactional, a
partial calculation produces a document whose totals disagree with its inputs
— *"which is a constraint on the host interface as much as on the interpreter,
and is the part most likely to be skipped."*

That constraint was not skipped. It was built first, as
[PRE-E](00-execution-order.md), before this gap was picked up (`77d9901`):
`DocumentEditor::transaction` restores the editor's whole mutable state on an
error return, and `set_field_values` is the all-or-nothing multi-field apply
built on it. So the deciding argument against A was answered by work that had
already landed, and the recommendation was written against a repository that
no longer exists.

The other two reasons stand and are not disputed. Option A still contributes
nothing to the eleven-function parity bar, and the scripts are still more
useful to Tinker as data than as behaviour — which is exactly why **B is
present, tested and independently green underneath A**, in its own commit, so
that removing the interpreter leaves a working reader rather than a hole.

### What was built

Two commits, in the order this document's own closing line prescribes:
*"Option A stays available. If a corpus or a user says otherwise, B is the
foundation it would be built on rather than work that gets thrown away."*

**`07dd4b0` — option B's scope.** `/AA` on fields (12.6.3 table 198) in all
four flavours, `/CO` (12.7.2 table 218) in declared order, `/Names
/JavaScript` (7.7.4 table 31) and the catalog's own `/AA` (12.6.3 table 200),
surfaced on `Field::scripts` as source text. `/JS` is read in both of
12.6.4.16's forms, string and stream, because a producer writes the stream as
soon as the script exceeds a line. A script past `MAX_SCRIPT_LEN`, or past the
document's `MAX_SCRIPT_TOTAL`, is reported as `Script::Oversize(len)` rather
than truncated — truncated source means something different from what the file
says, and the difference is silent.

**`acbce89` — the interpreter.** `tinker-pdf-cos::script` is the ECMAScript
subset and is PDF-free: values reach it through a two-method `Host`, so it is
fuzzable without a document and the decision about whether a write is
*allowed* stays next to the transaction primitive.
`tinker-pdf-cos::calc` is the pass — which scripts run, in what order, what
they may see, and how their answers reach the document. Neither module is a
leaf primitive (ruling 8) nor a binding (ruling 11), and a separate crate
would have had to depend on the field model that depends on it; `xtask dag`
is unchanged.

### Milestone 2's warning is a value, not a `Warning`

The one place the delivered work departs from this document's table.
`WarningKind` is a closed set describing repairs the lexer and object parser
performed on bytes, collected while parsing; a script is neither a repair nor
something the parser sees, and walking the field tree on every open would
charge every document for the few that have one. So `form::script_summary`
returns the counts and `ScriptSummary::describe` is the sentence to put in
front of a user. The milestone's intent — a document with scripts can be
reported on, once, with numbers — is met; its mechanism is not.

### The cascade rule, which this document does not name

`/CO` gives an order, and a script can write a field whose own `/AA` would
fire. The rule chosen: **one pass, and every calculate action runs at most
once** — `/CO` order first, then any remaining field carrying `/AA /C` in
document order, so a form whose producer omitted `/CO` still computes. A write
is visible to every later script in the pass; it never re-triggers one that
has run and never re-orders the pass. A cycle therefore terminates by
construction rather than by a counter, and `Recalculation::cascades_cut` names
each field a later script wrote after that field's own calculation had already
run — the one case where one pass and a full fixed-point disagree.

Injection settled the alternative. Following the cascade to a fixed point does
**not** hang, because `MAX_CALC_STEPS` stops it; what it does instead is turn
a form that computes correctly into one that computes nothing, since the
runaway exhausts the pass budget and refuses everything. The cut is about
giving a right answer; the total budget is about giving an answer at all.

### The boundary, and how a hostile script terminates

Ruling 1 governs every line: the source is attacker-controlled text arriving
through the same door a malformed `cmap` does. Three bounds, none of which
substitutes for another.

- **Depth** — `MAX_SCRIPT_DEPTH`, counted at `statement` and at `unary`, which
  is the one production every expression cycle passes through. The evaluator
  walks the tree the parser built, so that single cap bounds its stack too and
  there is no second cap to get wrong.
- **Work** — `MAX_SCRIPT_STEPS` per script and `MAX_CALC_STEPS` per pass. *A
  depth cap is not a work cap when the structure branches*: `while (true) {}`
  is depth one and costs everything. Both budgets bind, because a document
  chooses how many scripts it carries.
- **Size** — source length, token count, string length, array length and
  variable count, because repeated concatenation is cheap in steps and ruinous
  in bytes.

Refused rather than approximated: `eval`, `function`, `try`, `switch`,
`for...in`, regular expressions, objects, prototypes, and every `app.*` beyond
an inert stub. An inert stub's *result* can never become a field value —
`undefined` in a total is the document that lies this whole feature is about.

### Where a half-implementation was prevented, twice

1. Scripts never touch the document. Every write lands in a staging map; the
   pass reads it, so `/CO` order means something, and a script that fails
   halfway had nothing to undo.
2. The apply is one `set_calculated_values`, which is `transaction`
   underneath.

**Any script that cannot be run refuses the whole pass.** Running the nine
that parse and skipping the tenth is the document this gap is written about.

### One rule the fill layer had to grow

12.7.4.1 table 227 makes ReadOnly a constraint on *the user*, and a calculated
total is flagged read-only precisely so that nothing but the document's own
action writes it — refusing there would make every properly authored
calculated form uncomputable. `fill::accepts` splits into the read-only test
and `accepts_value`, which both doors share, so the calculation path has no
second copy of the rules to drift from. `set_calculated_values` is public,
because a host computing the surfaced scripts itself needs exactly that door
and would otherwise clear the flag and put it back.

### What is supported, and what is not

Number and string literals, arithmetic, comparison, the logical and
conditional operators, `if`/`else`, `var`, `while` and C-style `for`, blocks,
`return`, array literals and `new Array(...)`, member access, indexing, calls,
`event.value`/`event.target`, `getField`, and the five helpers:
`AFSimple_Calculate` with SUM, AVG, PRD, MIN and MAX over an array or a
comma-separated string; `AFNumber_Format` with all four separator styles, the
negative styles and currency on either side; `AFPercent_Format`;
`AFDate_Format` over its index table; `AFSpecial_Format` over its four
shapes.

The format helpers deliberately do **not** write `/V`. 12.7.3.3 keeps a
field's value and its appearance apart, and a `/V` of "GBP 1,234.00" is a form
whose export is unusable. `calc::formatted_value` runs `/AA /F` against a
read-only host and hands back the display string.

Keystroke and validate scripts are surfaced and never run, which plan 11 puts
out of scope regardless of how this decision landed. Document-level scripts
are surfaced and never run: arbitrary program text with no field to write is a
different problem from a calculation.

### Two defects found on the way

**`DocumentEditor::fields` read the document underneath the overlay**, so no
edit was ever visible to a later one. Nothing depended on it — `accepts` looks
at the kind and the flags, not the value — but a calculation reads the values
it computes from, and one reading them from under its own writes computes a
total from inputs the file no longer has.

**A `Budget` recorded the step it refused**, so `used` could sit one past
`limit` — a per-script number that could exceed the per-script cap, which
makes the pass total drift by one per overrunning script. Found by the fuzz
target's own invariant assertion on its first seed, in under a second; five
minutes of mutation would not have found it, because nothing crashed.

### Numbers

1 496 tests to 1 570. `form_script` is the seventeenth fuzz target, with
eighteen seeds; after the budget fix, two five-minute sessions on
WSL2/Ubuntu-24.04 ran 2 112 845 and 1 322 809 executions with no crash, at
cov 2 249 edges / ft 10 622. The eleven determinism fingerprints did not move.
Five injected defects: two were caught by exactly one assertion each and one
by none at all, and all three gained a test — see `3724063`.
