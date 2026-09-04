# Test corpora

External corpora are FETCHED, never committed. [`corpora.lock`](corpora.lock)
pins each one by upstream commit *and* by archive checksum; CI restores from a
cache keyed on that file. Nothing from any corpus enters git — size aside,
redistribution rights are per-file murky in every real-world PDF collection,
and a pin plus a checksum reproduces the set without this project becoming a
distributor. The reasoning is
[`docs/verification.md`](../docs/verification.md).

## The metamorphic relations were declined on a wall clock, and are not any more

*Resolved 4-5 September 2026. Kept because the reasoning is worth having and
because the next reader deserves the measurement rather than the conclusion.*

`corpus-run --check` used to report what read exactly like an engine regression
when nothing about the engine had changed: `crop`, `dpi` and `rotate` *asked of*
a smaller denominator, which the comparator refuses on purpose, because
declining the hard files is not a better rate. The nightly failed on it every
night for a week.

**The cause was `META_BUDGET_MS` in `tools/tpdf`, not the per-file timeout.** A
file that had already spent more than 3 100 ms opening and rendering had all
three relations skipped, for a reason written down at the site: the relations
re-render the first page and two of them save and reopen the document, and on a
slow file that extra work used to run the runner's own timeout out. Declining
was a defensible answer. What made it untenable is that 3 100 ms is *wall
clock*, so how many files crossed it depended on what else the machine was
doing — and `compared` is a ratcheted denominator.

**Nothing deterministic could replace it, and that was measured.** The `cost`
line reports the three properties that ought to bound the work — bytes, objects
and first-page pixels — and they do not: `qpdf/numeric-and-string-2.pdf` is
16 KB with 22 objects and was declined at 6.3 s, while its sibling
`numeric-and-string-1.pdf`, 18 KB and 15 objects, was admitted at 8.9 s. Cost
does not predict time here, so a cost gate would have been the same lottery with
a straighter face.

**So the gate is gone and the timeout is sixty seconds.** The comment being
deleted had considered exactly that and rejected it, on the grounds that a
longer timeout changes what the pass rate means. It does — and the change was
measured before it was taken, over all 4 525 files at 72 dpi:

- the whole corpus runs in **95 seconds**;
- **nothing times out**, where the twenty-second run had two pdf.js files
  flipping between passed, timed out and stalled from one run to the next;
- every one of the twelve corpus-and-relation counts is **up or equal** —
  pdf.js `rotate` 838 compared to 840, `dpi` 944 to 948, qpdf `crop` 471 to 472;
- veraPDF reads **2907/2907**, because the ten-thousand-page implementation-limit
  fixture finishes for the first time.

What still declines a relation is a property of the document — no pages,
encrypted, opened with a warning, needed the leniency ladder, a page too large
to render twice, a page too small to crop. `compared` is a function of the
corpus now and not of the machine.

The note this replaces went through two wrong explanations before the right
one: it blamed the per-file timeout alone, was corrected to blame the budget,
and is now settled by deleting the budget. Both earlier versions are in git.

## Licences

Generated from the lock by `cargo run -p xtask -- corpus-licences`, and checked
against this file by `--check` in CI, so a corpus cannot be added without its
terms reaching the file a person reads.

| Corpus | What it exercises | Upstream licence | Redistributed here? |
| --- | --- | --- | --- |
| `pdfjs` | decades of real-world breakage, reported by users of a browser's viewer | Apache-2.0 (the project); the fixtures are third-party and mixed, and upstream itself links rather than stores many of them | **no** — fetched, never committed |
| `verapdf` | atomic spec-conformance cases for PDF/A, PDF/UA, ISO 32000-1 and ISO 32000-2 | CC-BY-4.0 | **no** — fetched, never committed |
| `qpdf` | cross-reference, object-stream, linearization and encryption torture | Apache-2.0 | **no** — fetched, never committed |
| `pdfa-examples` | PDF 2.0 features shown deliberately: UTF-8 strings, page-level output intents, incremental saves | CC-BY-SA-4.0 | **no** — fetched, never committed |
