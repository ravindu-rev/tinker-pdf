# An interleaved XPS package, derived

ECMA-376 Part 2 7.2.4 lets an OPC producer write a part as a run of **pieces** —
`<item>/[0].piece`, `<item>/[1].piece`, …, `<item>/[n].last.piece` — so it can
stream several parts at once and interleave them in the archive. No producer
on hand writes one: WPF, the XPS object model and Ghostscript all write whole
parts, and so does every package in [`../xps`](../xps/README.md). So this
package is **derived** rather than produced, and it lives here rather than
there because that directory's first claim is that nothing in this repository
wrote a byte of anything in it.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `wpf-image-and-text-pieces.xps` | 82 987 | `4cdf71cf1a2170035a1880ee28686d481d465be2029381d665a88580d790ed4b` |

**What it is.** `../xps/wpf-image-and-text.xps` — WPF's one-page package of a
PNG behind an `ImageBrush` and a `<Glyphs>` run in an obfuscated font — with six
of its eight items cut into equal-sized pieces and the pieces of different items
written round-robin, so the archive interleaves six parts at once:
`[Content_Types].xml` in three, `_rels/.rels` in two, the page in four, the
page's relationships part in two, the PNG in two and the 189 252-byte ODTTF font
in five. Every piece keeps its source item's compression — the PNG's and
`_rels/.rels`'s stored, the rest deflated — and `FixedDocumentSequence.fdseq`
and `FixedDocument.fdoc` stay whole. **Every byte of every part is WPF's**; only
the container is this repository's.

**How it was obtained**, on Linux x86_64 with CPython 3.11.15's `zipfile`, on
26 September 2026:

```
cd crates/tinker-pdf/tests/xps_interleaved && python3 make-interleaved.py
```

`make-interleaved.py` stamps every item with one fixed time, so a rerun under
the same CPython writes the same bytes; the hash above was measured twice.

**How it is checked.** `xps_conservation.rs` sweeps it with the thirteen real
packages — its row in `../xps/CONSERVATION.tsv` is its source package's row, 3
facts of 3 — and
`an_interleaved_package_states_the_census_of_the_one_it_was_cut_from` holds the
two packages to one census and one rendered page, byte for byte. The harness
joins the pieces by its own reading of 7.2.4 rather than by the reader's.
`xps_opc.rs`'s `hostile_bytes_through_an_interleaved_package_never_panic`
damages it across its length.

**What it does not buy** is a second implementation's idea of interleaving: the
piece names and the round-robin order are this repository's reading of 7.2.4,
and a package from a producer that interleaves would be the file that closes
that.
