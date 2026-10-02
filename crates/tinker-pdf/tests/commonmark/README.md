# CommonMark's examples, fetched rather than committed

`tests/commonmark_spec.rs` holds `tinker_pdf::markdown::to_html` to the 652
examples of CommonMark 0.31.2 (tier 5's Markdown row). Nothing from the
specification is in this directory, and that is the point of it.

| What | Where from | Terms | In git |
| --- | --- | --- | --- |
| `spec.txt`, tag `0.31.2` | `https://raw.githubusercontent.com/commonmark/commonmark-spec/0.31.2/spec.txt` | CC-BY-SA 4.0 (the repository's `LICENSE`: "The CommonMark spec (spec.txt) and DTD (CommonMark.dtd) are Copyright (C) 2014-16 John MacFarlane. Released under the Creative Commons CC-BY-SA 4.0 license") | **no** — fetched, never committed |

- **Fetched 2 October 2026**, 205 025 bytes, SHA-256
  `257c41ad946f7a1414a499aca402a1aa8fdac3678532266611348c1cf54f4b80`. The
  script and the test both check it, so a different revision's examples
  cannot move the floor for a reason that is not the reader.
- **Why not committed.** `deny.toml` admits no copyleft, "not even weak
  copyleft", and lists the licences vendored data is checked against; CC-BY-SA
  is share-alike and is not among them. `epub3-samples` (CC-BY-SA 3.0) and the
  PDF Association's `pdf20examples` (CC-BY-SA 4.0) are fetched for the same
  reason (`tests/epub/fetch-corpus.sh`, `corpus/corpora.lock`).
- **`spec.json` is the same 652.** The published `spec.json` is what the tag's
  own `test/spec_tests.py --dump-tests` writes from `spec.txt` (BSD-3-Clause,
  the same repository). On 2 October 2026 that script was run once — Python
  3.11.2, `python3 test/spec_tests.py --dump-tests --spec spec.txt` with the
  tag's `cmark.py` and `normalize.py` beside it — and a SHA-256 taken over its
  652 objects' `markdown`, `html` and `section`; `commonmark_spec.rs` recomputes
  that fingerprint over the examples it extracts itself and asserts it, so the
  test reads exactly the examples `spec.json` carries. The script's output is a
  dated measurement of what the examples *are*, never of whether an answer is
  right: the answers are the specification's own.
- **Run it:** `sh crates/tinker-pdf/tests/commonmark/fetch-spec.sh`, then
  `TINKER_COMMONMARK_SPEC=<absolute path it prints> cargo test -p tinker-pdf
  --test commonmark_spec -- --nocapture`. `TINKER_COMMONMARK_SPEC_FAILURES=1`
  prints every failing example; `TINKER_COMMONMARK_SPEC_REQUIRED=1` makes a
  skip a failure, which is what the `commonmark-spec` CI job sets.
- **Measured 2 October 2026: 651 of 652**, every section whole but *Entity and
  numeric character references* (16 of 17). Example 25 names five HTML5-only
  entities (`&Dcaron;`, `&HilbertSpace;`, `&DifferentialD;`,
  `&ClockwiseContourIntegral;`, `&ngE;`) outside the XHTML 1.0 sets this
  repository vendors.
