# Test corpora

External corpora are FETCHED, never committed. [`corpora.lock`](corpora.lock)
pins each one by upstream commit *and* by archive checksum; CI restores from a
cache keyed on that file. Nothing from any corpus enters git — size aside,
redistribution rights are per-file murky in every real-world PDF collection,
and a pin plus a checksum reproduces the set without this project becoming a
distributor. The reasoning is
[`docs/verification.md`](../docs/verification.md).

## The per-file timeout is marginal for three files

`corpus-run`'s default is **20 seconds of wall clock per file**, and three
files in the fetched set sit close enough to it that they flip with machine
load: two in `qpdf` and one in `verapdf`. When they flip, the ratchet reports
what looks like an engine regression — fewer files passed, and the metamorphic
relations "asked of" a smaller denominator, which the comparator refuses on
purpose because declining the hard files is not a better rate.

Measured 31 August 2026, same commit, same corpus: under load, 606/637 `qpdf`
with 2 timed out; idle at `--timeout 120`, every bar held and `verapdf` read
2907/2907 rather than 2906.

**That better number is not recorded, and should not be.** It is reachable only
at a timeout the default run does not use, so recording it would set a bar that
an ordinary `--check` cannot clear. The comparator already says this in its own
output — *"a file near the limit flips when the machine is busy; re-run before
believing this is the engine"* — and that sentence is the right response to a
ratchet failure whose whole shortfall is accounted for by timeouts.

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
