# Tinker integration: three decisions and a blocker nobody planned

Plan 15 describes the swap in full and calls it "a mechanical phase by design;
every judgment call was moved earlier". Three judgment calls were moved *here*
and none has been made. There is also a parity blocker that appears in no plan
at all. This document is where they get settled. (S — the decisions)

## Where integration actually stands

Plan 15's precondition is Checkpoint B: parity tests green, corpus render
parity ratcheted at 95% or better, write round-trips validated.

~~**The corpus has never been run** ([23](23-corpus-runner.md)), so the
precondition is not close to met~~ — not because the engine is far off, but
because nobody has measured. Integration cannot start on evidence that does
not exist.

*Amended 16 August 2026: it has been run — 4 525 files, 4 484 rendering every
page, zero crashes. That closes the half that could not be measured and leaves
render **parity** open, because gap 23 counts bitmaps that came back rather
than bitmaps that are right. See the `As built`.*

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

*Answered 16 August 2026: **both, bundled first, system second**, with the
order and the face set written into plan 15 as milestone 15.1a. The engine
side was already done and is re-measured in the `As built`.*

## Decision 1: EPUB, XPS and CBZ

MuPDF opens these; Tinker exposes them as `Doc::Other`. ~~tinker-pdf is a PDF
engine and always will be.~~ *Amended 16 August 2026: no longer true, and it
was the premise this section's three options were drawn from. The owner chose
**D**, below, which is a fourth option this document did not contain and which
this sentence is what excluded. Amended here, and in every other place it was
stated, per CONTRIBUTING rule 4.*

**A. Drop them.** Tinker becomes PDF-only. Simplest, and removes the last
reason to keep any MuPDF code in the tree — which is the whole point of the
exercise, since MuPDF is the only AGPL dependency and the documented iOS App
Store blocker.

**B. Keep MuPDF for those formats only.** The AGPL dependency survives, the
iOS blocker survives, the vendored MSVC patch survives. Everything plan 15's
deletion checklist removes stays.

**C. An external tool.** Convert on the way in, out of process. No engine
dependency, and a conversion step the user sees.

**D. Build them natively, in tinker-pdf.** *Added 16 August 2026, because it
is what was chosen and none of A, B or C describes it.* The three formats
become capabilities of this engine. Nothing is dropped, nothing is converted
out of process, and no MuPDF survives — so the licensing outcome is A's,
reached without losing the formats.

It is a fourth option rather than a variant of "a later dedicated module"
(which plan 15's version of this decision does list) because it is not
deferral: it is a commitment to three new engine capabilities, one of which is
larger than everything this engine has built so far.

**Recommendation: A**, unless there is usage data saying otherwise. B forfeits
the licensing outcome that motivated the entire engine, for three formats
Tinker's own plans treat as incidental. If those formats matter, C keeps the
outcome and costs a conversion step.

**This is a product decision, not an engineering one.** It needs whatever
usage data exists.

*Answered 16 August 2026: **D**, against this recommendation. The reasoning,
the sizes and what it costs are in the `As built` below.*

## Decision 2: form calculations

Covered in [27](27-form-calculations-decision.md); the recommendation is to
surface `/AA` as data and not build an interpreter. It appears here because
plan 15 lists it as one of the three, and closing it closes part of this.

*Answered 16 August 2026: **option A**, a hand-rolled ECMAScript subset,
against gap 27's recommendation of B — and already built, `07dd4b0`..`7c7b52d`.
B's reader landed first and independently, so A sits on top of it. See the
`As built`.*

## Decision 3: Tinker's licence

Once MuPDF is gone, Tinker's AGPL obligation goes with it. Whether Tinker
*stays* AGPL is entirely the owner's call — the engine is MIT OR Apache-2.0
and imposes nothing.

Worth noting only that the iOS App Store blocker documented in
`docs/mupdf-limitations.md` was about the *dependency*, not about Tinker's own
licence. Removing the dependency removes the blocker regardless of what Tinker
chooses for itself.

*Answered 16 August 2026: **permissive — MIT OR Apache-2.0, matching the
engine.** The caveat the owner should see beside it: relicensing existing
Tinker code needs its contributors' agreement, which is a step outside this
repository. See the `As built`.*

## What is stale in plan 15

Not wrong — unverified, which is different, and worth flagging before someone
follows it as though it were current.

**The deletion checklist's paths were verified against Tinker's tree at
planning time**, which was the original scaffolding commit. Tinker has been
frozen since, but the freeze exempts dependency and security bumps, so at
least some drift is guaranteed. Every path wants re-checking before it is used
as a checklist.

~~**Plan 15 has never been amended**~~, unlike plans 00, 02 and 99, which carry
dated in-place amendments. It has no `As built` and nothing from the August
2026 audit — including the `FontProvider` blocker above.

*Amended 16 August 2026: this was true when written and false by the time it
landed — `e0fc873` wrote this document and plan 15's first amendment in one
commit, so the sentence was falsified by its own commit. Plan 15 now carries a
second dated amendment as well, plus the checklist verification recorded in
place.*

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

## As built — August 2026

All four milestones are documentation, and milestone 4 is documentation on
purpose: its code lives in the Tinker repository, which this task does not
own. **No commit was made in Tinker, and nothing in its tree was modified.**

### The three decisions, as recorded

Each is written into plan 15's `Owner decisions` section, in place of the
options it used to list, with a date beside it. They are summarised here
because a reader arriving at this document deserves the answer without a
second hop.

**1. EPUB, XPS and CBZ — built natively into tinker-pdf. Option D, which this
document does not contain.** Not A, B or C, and recording it as a bent version
of one of the three would have misrepresented it — so the option is added
above with its own heading rather than the recommendation being quietly
rewritten.

It reaches the licensing outcome A was recommended *for*: MuPDF leaves the
shipped tree entirely, taking the AGPL dependency, the vendored MSVC patch and
the iOS App Store blocker with it. What it costs is this repository's standing
identity. "tinker-pdf is a PDF engine and always will be" appears in this
document and is assumed throughout the plans; every place it is stated has
been amended deliberately, per CONTRIBUTING rule 4, rather than left to be
noticed later — the list is at the foot of this section.

Sized after the costs were put to the owner explicitly. **CBZ is S**: a ZIP of
images, and the engine's own inflate and JPEG decoder do most of it. **XPS is
L**: an OPC ZIP, a hand-rolled XML parser, and fixed-page markup that maps
closely onto the path, glyph and brush calls the `Device` seam already has.
**EPUB is XL+**: XHTML plus a CSS cascade, a box model, line breaking,
pagination and font fallback — a layout engine rather than a renderer, and on
its own larger than the entire twenty-eight-plan gap programme just completed.
CONTRIBUTING rule 1 forbids third-party crates, so the ZIP reader, the XML
parser and every line of the CSS live in this tree.

**Three new gap plans will be written for them — CBZ, XPS and EPUB — after
this gap closes.** That is where the work is planned; it is not in this
document and it is not in plan 15.

**2. Form calculations — option A, a hand-rolled ECMAScript subset, and it is
already built.** Settled by [27](27-form-calculations-decision.md), closed in
`07dd4b0`..`7c7b52d`, against that document's own recommendation of B. The
premise that changed is B's deciding argument: a half-implementation is worse
than none *unless the writes are transactional*, and that clause names the
constraint as "the part most likely to be skipped". PRE-E built it first —
`DocumentEditor::transaction` — so the argument was answered by work that had
already landed rather than overruled. **Option B's reader landed first and
independently green, in its own commit**, so A sits on top of it and removing
the interpreter would leave a working reader rather than a hole. Plan 11's
open JavaScript question closes with it.

**3. Tinker's licence — permissive: MIT OR Apache-2.0, matching the engine.**
Two things the owner should see beside that. **Relicensing existing Tinker
code needs its contributors' agreement**, which is a step outside this
repository — the decision is recorded here, the consent is collected there.
And, as this document already notes, the iOS App Store blocker in
`docs/mupdf-limitations.md` was about the *dependency* rather than about
Tinker's own licence, so it clears when MuPDF leaves whatever Tinker chooses
for itself.

### What Checkpoint B's precondition now stands at

This document opens by saying the corpus has never been run and the
precondition "is not close to met". [23](23-corpus-runner.md) has since run
it: 4 525 files, 4 484 rendering every page, 40 failures, one timeout, **zero
crashes**. That closes the half of the precondition that could not be measured
at all and leaves the other half untouched, and the distinction matters enough
to state twice:

- gap 23 defines `rendered` as "returned a bitmap without crashing or timing
  out", so **99.1 % is a crash-and-hang number**, not a parity number;
- Checkpoint B asks for render **parity** ratcheted at 95 %, which is a claim
  about whether the bitmap is the *right* bitmap, and gap 23 performs no
  oracle comparison at all — that is `oracle-diff`'s separate question;
- the second axis, 1 092 files (24 %) rendering with something reported, was
  measured with **no faces**, so it is dominated by documents that draw no
  text. `ratchet.json` records `"fonts": "none"` and `corpus-run` refuses to
  compare a `--fonts` run against a bar recorded without them.

So: from "cannot start against evidence that does not exist" to "half
measured, and the missing half is an oracle pass and a `--fonts` bar, in that
order".

### The deletion checklist, re-verified

Against Tinker at `f33ce8a`, on 16 August 2026. **Twenty-two named items, all
twenty-two still present at the paths stated, nothing moved.** The drift this
document predicted did not happen where it was expected: the two commits since
the scaffolding commit adopted this engine as a submodule and advanced its
pointer, and touched no part of the MuPDF surface.

The value was in what the checklist omits. Five traces would survive it being
followed exactly as written, and they are recorded inside the checklist in
plan 15 rather than here, because a checklist is read where it is used. The
one with a shipped consequence: `release.yml`'s source-archive job asserts
that `third_party/mupdf-msvc` is "a plain directory rather than a submodule,
so the vendored MuPDF patch travels with the archive" — and Tinker now has a
real submodule at `engine/tinker-pdf`, which `git archive` does not follow. As
soon as the swap lands, the released source archive ships a workspace that
cannot build. The same submodule falsifies milestone 15.3's `rg -i mupdf` exit
criterion, which is scoped in plan 15's table rather than left as something
that can never go green.

### Milestone 4: the seam is done, the host side is planned

The engine side was already built and is re-verified here rather than assumed.
`Document::with_fonts(Arc<dyn FontProvider>)` is the seam — `set_fonts` across
the C ABI, Python, JS/wasm and .NET — `FontRequest` carries the
subset-stripped `/BaseFont` and five flags read from `/Flags`, from the name
and from `/StemV`, and `SimpleFontProvider` is a working implementation.
`crates/tinker-pdf/tests/substitute_fonts.rs`'s seven tests pass, and the
proof was repeated on a MuPDF-generated fixture rather than only on the
synthetic face they build: **`testdata/simple-text.pdf` renders 0 inked pixels
with `UnreadableFont` on all three pages and no provider, and 4 192 on page
one at 150 dpi with three real system faces behind `tpdf --fonts`, with the
warning gone.**

The decision this document says is needed — where Tinker's faces come from —
is answered in plan 15 under *Where Tinker's faces come from*, with milestone
15.1a: **both, bundled first and system directories second.** The deciding
argument is not coverage but goldens. Tinker's own `plans/01` names bundled
fonts as its mitigation for cross-platform golden flake, and plan 15's
milestone 15.2 re-baselines every visual golden once under human review; doing
that against faces that differ per machine makes the review meaningless. It is
15.1a rather than 15.0 because there is no `Document` in Tinker to call
`with_fonts` on until the swap has put one there — what the work must precede
is 15.2, so that is where the gate is written.

### Where the PDF-only identity was stated, and how it was amended

Four places, all amended in this gap's final commit:

- **this document**, the sentence that excluded option D — struck, with the
  amendment naming what it excluded;
- **`docs/plans/15-tinker-integration.md`**, "drop (viewer becomes PDF-only,
  honest)" — replaced by the answer;
- **`docs/STATUS.md`**, "This engine reads PDF and will not read those" —
  replaced by the decision and its sizes;
- **`README.md`**, whose opening line is the identity most readers form —
  which keeps "PDF engine" as what it *is*, and gains a sentence saying what
  it is becoming, because a README that quietly grew three formats would be
  the same failure in the other direction.

`docs/PLAN.md`'s locked-decisions list gains the scope decision beside the
licence and the spec baseline, which is where a reader looks for what this
project has committed to.
