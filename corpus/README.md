# Test corpora

External corpora are FETCHED, never committed. [`corpora.lock`](corpora.lock)
pins each one by upstream commit *and* by archive checksum; CI restores from a
cache keyed on that file. Nothing from any corpus enters git — size aside,
redistribution rights are per-file murky in every real-world PDF collection,
and a pin plus a checksum reproduces the set without this project becoming a
distributor. The reasoning is
[`docs/plans/14-testing-and-corpora.md`](../docs/plans/14-testing-and-corpora.md).

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
