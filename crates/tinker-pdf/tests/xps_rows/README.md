# XPS packages for the rows that left the refusal table, derived

Each `XpsElementDefect` row that closed after tier 4 owes a fixture in the
conservation sweep, and no producer on hand writes any of the features those
rows are about — WPF, the XPS object model and Ghostscript write none of
12.1.5's simulations, no n-channel `ContextColor`, no `ContextColor` gradient
stop and no per-stop alpha. So these packages are **derived** rather than
produced, and they live here rather than in [`../xps`](../xps/README.md)
because that directory's first claim is that nothing in this repository wrote
a byte of anything in it.

Every package is `../xps/wpf-image-and-text.xps` — WPF's one-page package with
an obfuscated ODTTF font — with its **fixed page replaced**. The font, the
picture, the relationships, the content types and the containers are WPF's
bytes; the page markup, and where a row needs one a profile part, are this
repository's.

| File | Bytes | SHA-256 | Row |
| --- | ---: | --- | --- |
| `wpf-style-simulations.xps` | 78 764 | `3a31d48215aea021cb0a5853533656cf0f9821ddaa494bf84d279df6d310426a` | 12.1.5's `StyleSimulations` |

**`wpf-style-simulations.xps`** sets WPF's own run — `"Page one"`,
`Indices=",53"`, in the package's font — four times at a 48-unit em, a hundred
units apart: as designed, `BoldSimulation`, `ItalicSimulation` and
`BoldItalicSimulation`.

**How they were obtained**, on Linux x86_64 with CPython 3.11.15's `zipfile`,
on 3 October 2026:

```
cd crates/tinker-pdf/tests/xps_rows && python3 make-rows.py
```

`make-rows.py` stamps every item with one fixed time, so a rerun under the
same CPython writes the same bytes; each hash above was measured twice.

**How they are checked.** `xps_conservation.rs` sweeps them with the thirteen
real packages, and each has its row in `../xps/CONSERVATION.tsv`. The census
reads what each row is about out of the markup with its own scanners — for the
simulations, `StyleSimulations` and 12.1.5's S5.6 offset written out from the
clause — and out of the document from the content stream: Table 106's
fill-and-stroke mode at a line width of 2% of the em, and a text matrix whose
second axis leans 20°. `xps_glyphs.rs` renders the simulation fixture and
measures the ink.

**What they do not buy** is a producer's idea of these features: the markup is
this repository's reading of ECMA-388, and a package from a producer that
writes them would be the file that closes that.
