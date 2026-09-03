# Test corpora

External corpora are FETCHED, never committed. [`corpora.lock`](corpora.lock)
pins each one by upstream commit *and* by archive checksum; CI restores from a
cache keyed on that file. Nothing from any corpus enters git — size aside,
redistribution rights are per-file murky in every real-world PDF collection,
and a pin plus a checksum reproduces the set without this project becoming a
distributor. The reasoning is
[`docs/verification.md`](../docs/verification.md).

## The metamorphic relations are declined on a wall clock, so the bar moves with load

`corpus-run --check` can report what reads exactly like an engine regression
when nothing about the engine has changed: fewer files passed, and `crop`,
`dpi` and `rotate` *asked of* a smaller denominator, which the comparator
refuses on purpose because declining the hard files is not a better rate.

**The cause is `META_BUDGET_MS` in `tools/tpdf`, not the per-file timeout.** A
file that has already spent more than 3 100 ms of wall clock opening and
rendering has all three relations skipped, for a good reason written down at
the site: the relations re-render the first page and two of them save and
reopen the document, and on a slow file that extra work used to run the
runner's own timeout out and turn three pdf.js files that had always passed
into timeouts. Declining is the right answer. What makes it awkward is that
3 100 ms is *wall clock*, so how many files cross it depends on what else the
machine is doing.

Measured on one commit, 31 August 2026: under load, `qpdf` 606/637 with two
timed out; idle at `--timeout 120`, every bar held and `verapdf` read 2907/2907
rather than 2906. Measured again on 4 September with the machine idle and the
same flag: **zero files timed out anywhere, every outcome count equal to or
better than the record, and the relation denominators still down** — five in
`pdfjs`, two in `qpdf`. That last run is what rules the timeout out as the
explanation and points at the budget instead.

Two consequences worth keeping:

- **A shortfall accounted for entirely by declined relations is not evidence
  about the engine.** Re-run idle before believing it. The comparator says as
  much in its own output when timeouts explain the gap; when they do not, this
  is the next thing to check.
- **A better number measured at a non-default timeout is not recorded.** The
  2907 above is reachable only with headroom the default run does not have, so
  recording it would set a bar an ordinary `--check` cannot clear.

An earlier version of this note blamed the per-file timeout alone. That was
wrong, and the run with zero timeouts is what disproved it.

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
