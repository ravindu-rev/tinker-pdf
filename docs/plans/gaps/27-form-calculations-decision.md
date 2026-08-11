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
