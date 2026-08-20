# Third-party material

tinker-pdf's own code is **MIT OR Apache-2.0**, and every line of PDF logic in
it is hand-rolled (CONTRIBUTING rule 1). Nothing here is a code dependency.

What this file records is the other thing a repository can carry: **data**.
Encoding tables, glyph-name lists and character-collection mappings are
published facts about file formats, and re-deriving them would produce the same
numbers with more mistakes. They are vendored verbatim, with the licence that
came with them, and compiled into static tables by a build script — so the raw
files never reach a released binary, and the compiled numbers ride under the
upstream licence's redistribution terms.

The distinction matters to the licence gate. `cargo deny check licenses` reads
the *crate* graph and has nothing to say about a directory of text files, so a
BSD-3-Clause asset inside an MIT OR Apache-2.0 crate is invisible to it.
`cargo xtask vendor` is the other half: every vendored tree must appear below,
must carry its own licence file, and must declare an SPDX identifier that
`deny.toml` already allows. A data licence the project could not ship therefore
fails the same allowlist a crate licence would.

## Vendored data

| Path | Upstream | SPDX |
| --- | --- | --- |
| `crates/tinker-pdf-font/data/cmap-resources` | [adobe-type-tools/cmap-resources](https://github.com/adobe-type-tools/cmap-resources) at `f5cf3bc` (2023-11-15) | `BSD-3-Clause` |
| `crates/tinker-pdf-layout/data/ucd` | [The Unicode Character Database](https://www.unicode.org/Public/17.0.0/ucd/), version 17.0.0 (2025-07-29) | `Unicode-3.0` |

### `crates/tinker-pdf-font/data/cmap-resources`

Adobe's published CMap resources: the code-to-CID mappings for the
Adobe-Japan1, Adobe-GB1, Adobe-CNS1, Adobe-Korea1, Adobe-KR, Adobe-Manga1 and
Adobe-Identity character collections, plus the deprecated Adobe-Japan2, which
`UniHojo-*` and `Hojo-*` still name in files in circulation. 9.7.5.2 calls
these the predefined CMaps and gives no table of their contents, so this
directory is the normative statement of what `90ms-RKSJ-H` means.

Only the `CMap/` directories are vendored, along with `LICENSE.md` and
`VERSIONS.txt` for provenance. The collections' `cid2code.txt` files and the
JIS mapping tables map CIDs to *character sets* rather than to codes; nothing
in this engine reads them.

Upstream's own licence text is kept beside the data at
`crates/tinker-pdf-font/data/cmap-resources/LICENSE.md`, and every vendored
file additionally repeats it in its `%%Copyright` header. It is reproduced here
in full because BSD-3-Clause requires a binary redistribution to carry it, and
a compiled table is a binary redistribution:

```
Copyright 1990-2023 Adobe. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.

Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

Neither the name of Adobe nor the names of its contributors may be
used to endorse or promote products derived from this software without
specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### `crates/tinker-pdf-layout/data/ucd`

The Unicode Character Database, at the five files [UAX #14](https://www.unicode.org/reports/tr14/)'s
line breaking algorithm needs. Gap 31's plan calls the alternative by name — an
ASCII heuristic that breaks at spaces *"works on Project Gutenberg's entire
catalogue, and is catastrophically wrong on CJK"* — and CONTRIBUTING rule 1 has
no exception for a line breaker, so the third route is the one taken: published
facts about text, vendored verbatim and compiled into static tables by
`build.rs`, exactly as Adobe's CMap registry is one crate over.

| File | What it is |
| --- | --- |
| `LineBreak.txt` | The `Line_Break` property, which **is** the algorithm |
| `EastAsianWidth.txt` | UAX #11, needed by LB19a and LB30 rather than by measurement: `a(` is one word and `a（` is two |
| `extracted/DerivedGeneralCategory.txt`, here as `DerivedGeneralCategory.txt` | `Mn`/`Mc` for LB1's `SA` resolution, `Cn` for LB30b's unassigned pictographs, and `Pi`/`Pf` for LB15a and LB15b |
| `emoji/emoji-data.txt`, here as `emoji-data.txt` | `Extended_Pictographic`, LB30b's other half |
| `auxiliary/LineBreakTest.txt`, here as `LineBreakTest.txt` | **The conformance oracle.** 19 338 cases, run by `tests/uax14_conformance.rs` against the same entry point a book goes through |

The fifth is not compiled into anything and is the one worth defending. A line
breaker's own author can only write the tests that author thought of, and gap
31's whole subject is a build that is plausible and wrong; this file was written
by the people who wrote the algorithm, and it is the only assertion available
that a space-scanner cannot satisfy. It is a test input rather than a
redistributed table, and it is here rather than fetched because gap 20's finding
holds a third time: **a skipped oracle exits 0 and reads exactly like a pass.**

`LICENSE.txt` is upstream's own, kept beside the data. The Unicode License v3 is
`Unicode-3.0` in SPDX terms, which `deny.toml`'s allowlist **already permitted**
before this tree arrived — checked rather than assumed, and it is the single
fact that made UAX #14 buildable here rather than blocked. Its permission
notice must appear with any redistribution of the data files, which is what
`LICENSE.txt` beside them is for:

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2026 Unicode, Inc.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

## Test fixtures

`testdata/` holds PDFs generated by MuPDF, copied from Tinker; see
[`testdata/README.md`](testdata/README.md). They are inputs to tests and are
not redistributed in any built artefact.

## What is deliberately not here

No bundled typefaces. `crates/tinker-pdf/tests/substitute_fonts.rs` states the
position: the engine carries no font anyone has to licence, and a host that
wants one supplies it through `FontProvider`. If that changes —
[plan 05](docs/plans/05-fonts.md) M10 reserves a place for the Liberation
family under OFL-1.1 — the faces, the licence text and the `deny.toml`
allowlist entry all arrive in the same commit, and this section says so instead.
