# Tinker integration: three decisions and a blocker nobody planned

Plan 15 describes the swap in full and calls it "a mechanical phase by design;
every judgment call was moved earlier". Three judgment calls were moved *here*
and none has been made. There is also a parity blocker that appears in no plan
at all. This document is where they get settled. (S — the decisions)

## Where integration actually stands

Plan 15's precondition is Checkpoint B: parity tests green, corpus render
parity ratcheted at 95% or better, write round-trips validated.

**The corpus has never been run** ([23](23-corpus-runner.md)), so the
precondition is not close to met — not because the engine is far off, but
because nobody has measured. Integration cannot start on evidence that does
not exist.

## The blocker in no plan

`docs/plans/16-build-sequence.md` records it and no phase plan does:

> Tinker hands the engine a `FontProvider` or every non-embedding document
> renders textless.

The engine bundles no font faces and reads no font directories — deliberately,
so that it has no OS dependencies and builds identically on wasm. A document
that does not embed its fonts therefore renders with no text unless the host
supplies them. Tinker is the host.

This sits between phase 05 and phase 15 and belongs to neither. It is a
render-parity blocker on the widest possible class of real files, and it needs
to be done *before* any golden comparison, or every comparison is measuring
the absence of text.

**Decision needed:** where Tinker's faces come from — the system font
directories, a bundled set, or both with a fallback order.

## Decision 1: EPUB, XPS and CBZ

MuPDF opens these; Tinker exposes them as `Doc::Other`. tinker-pdf is a PDF
engine and always will be.

**A. Drop them.** Tinker becomes PDF-only. Simplest, and removes the last
reason to keep any MuPDF code in the tree — which is the whole point of the
exercise, since MuPDF is the only AGPL dependency and the documented iOS App
Store blocker.

**B. Keep MuPDF for those formats only.** The AGPL dependency survives, the
iOS blocker survives, the vendored MSVC patch survives. Everything plan 15's
deletion checklist removes stays.

**C. An external tool.** Convert on the way in, out of process. No engine
dependency, and a conversion step the user sees.

**Recommendation: A**, unless there is usage data saying otherwise. B forfeits
the licensing outcome that motivated the entire engine, for three formats
Tinker's own plans treat as incidental. If those formats matter, C keeps the
outcome and costs a conversion step.

**This is a product decision, not an engineering one.** It needs whatever
usage data exists.

## Decision 2: form calculations

Covered in [27](27-form-calculations-decision.md); the recommendation is to
surface `/AA` as data and not build an interpreter. It appears here because
plan 15 lists it as one of the three, and closing it closes part of this.

## Decision 3: Tinker's licence

Once MuPDF is gone, Tinker's AGPL obligation goes with it. Whether Tinker
*stays* AGPL is entirely the owner's call — the engine is MIT OR Apache-2.0
and imposes nothing.

Worth noting only that the iOS App Store blocker documented in
`docs/mupdf-limitations.md` was about the *dependency*, not about Tinker's own
licence. Removing the dependency removes the blocker regardless of what Tinker
chooses for itself.

## What is stale in plan 15

Not wrong — unverified, which is different, and worth flagging before someone
follows it as though it were current.

**The deletion checklist's paths were verified against Tinker's tree at
planning time**, which was the original scaffolding commit. Tinker has been
frozen since, but the freeze exempts dependency and security bumps, so at
least some drift is guaranteed. Every path wants re-checking before it is used
as a checklist.

**Plan 15 has never been amended**, unlike plans 00, 02 and 99, which carry
dated in-place amendments. It has no `As built` and nothing from the August
2026 audit — including the `FontProvider` blocker above.

## Scope

- Record the three decisions, with dates, in plan 15's "Owner decisions"
  section.
- Add a dated amendment to plan 15 recording the `FontProvider` blocker and
  the staleness of the deletion checklist.
- Re-verify the deletion checklist against Tinker's current tree.
- Plan the `FontProvider` work — which is Tinker-side and therefore outside
  this repository, but the engine-side seam is already there and tested.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | The three decisions recorded in plan 15 | Each has an answer and a date, not a list of options | S |
| 2 | Plan 15 amended with the `FontProvider` blocker and the checklist's status | The amendment is dated and in the in-place style plans 00, 02 and 99 use | S |
| 3 | Deletion checklist re-verified | Every path in it exists in Tinker's current tree, or is corrected | S |
| 4 | `FontProvider` wired through Tinker's seam | A non-embedding document renders with text — the parity blocker cleared **before** any golden comparison | M |

## Dependencies

**Needs first:** [23](23-corpus-runner.md) for the precondition, and
[27](27-form-calculations-decision.md) for decision 2.

**Unblocks:** phase 15, and with it the removal of the last AGPL code from
Tinker's shipped tree.

## Risks

| Risk | Mitigation |
| --- | --- |
| Integration starts before the corpus says the engine is at parity, and regressions land on users | The precondition is stated in plan 15 and restated here; it is a gate, not a guideline |
| The deletion checklist is followed as written and misses paths that moved | Milestone 3, before the checklist is used |
| Goldens are re-baselined before `FontProvider` lands, so every one bakes in missing text | Milestone 4 is explicitly before any golden comparison |
| Decision 1 is deferred and MuPDF stays for three formats, forfeiting the licensing outcome | Named as a product decision needing usage data, so it can be answered rather than avoided |
