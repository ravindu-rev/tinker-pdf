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
| `crates/tinker-pdf-filters/data/brotli` | [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932.txt), Appendix A, "Static Dictionary Data" (July 2016) | `BSD-3-Clause` |
| `crates/tinker-pdf-layout/data/ucd` | [The Unicode Character Database](https://www.unicode.org/Public/17.0.0/ucd/), version 17.0.0 (2025-07-29) | `Unicode-3.0` |
| `crates/tinker-pdf-font/data/liberation` | [liberationfonts/liberation-fonts](https://github.com/liberationfonts/liberation-fonts), release `2.1.5` (2021-10-01) | `OFL-1.1` |

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

### `crates/tinker-pdf-filters/data/brotli`

RFC 7932 Appendix A's `DICT` array: the 122 784-byte static dictionary every
Brotli stream may reference, which is what makes WOFF2 readable here at all.
§8 is unambiguous that it is not optional — a backward distance that reaches
past the start of the output *is* a dictionary reference, not an error — so a
decoder without these bytes is one that works until it meets a real font.

The bytes are published in the RFC as a hexadecimal dump; `dictionary.bin` is
that dump decoded, which is the form §8's `DOFFSET` arithmetic indexes. Nothing
is paraphrased and nothing is regenerated: the RFC states the length and the
CRC-32 of the array in the sentence that introduces it, and
`the_dictionary_matches_the_published_crc32` checks both against this
repository's own `crc32`, so the specification is what says the copy is right.

```text
122 784 bytes, CRC-32 0x5136cb04
```

**Why this clears the licence gate, checked at the source on 2026-08-30.**
The gate is real: `cargo xtask vendor` requires an SPDX identifier `deny.toml`
already allows, and a dictionary that could not be shipped would have meant
landing WOFF 1.0 and refusing WOFF2 by name. The chain is four links, and each
was read rather than assumed:

1. RFC 7932 is an **IETF-stream** document — its masthead reads "Internet
   Engineering Task Force (IETF)" and its Status section says it "is a product
   of the Internet Engineering Task Force (IETF)" and "has been approved for
   publication by the IESG". This matters because TLP §8.e, §8.f and §8.g
   disapply section 4 entirely for the IAB, Independent Submission and IRTF
   streams. An Independent Submission of the same document would **not** have
   cleared this gate.
2. The Trust Legal Provisions in effect on its publication date are TLP 5.0,
   effective 25 March 2015.
3. TLP §4.a defines Code Components by reference to the Trust's published
   list, and **"tables of values" is an entry on that list** (Code Components
   3.0, 23 April 2009). Appendix A is a named array of 122 784 byte values;
   the RFC calls it "the DICT array".
4. TLP §4.c licenses Code Components under the "Revised BSD License", whose
   text it reproduces and which it states "is intended to be compatible with
   the Revised BSD License template published at
   https://opensource.org/licenses/BSD-3-Clause". Three clauses, so the SPDX
   identifier is `BSD-3-Clause` — which `deny.toml` **already allowed** before
   this tree arrived, for Adobe's cmap-resources. No allowlist edit was needed,
   which was checked rather than assumed.

One wrinkle is recorded rather than smoothed over: RFC 7932's own boilerplate
says "Simplified BSD License", which in SPDX terms would be `BSD-2-Clause`.
That is the Trust's older name for the same clause, not a different licence —
TLP 5.0 renamed it to "Revised BSD License" and the text under both names is
the three-clause one reproduced in §4.c. `BSD-3-Clause` is the stricter of the
two readings and both are on the allowlist, so the identifier here is the one
that is true under either.

`LICENSE.txt` beside the data carries that reasoning and the full licence text,
as TLP §4.e option (1) permits, along with the attribution §4.d requests: this
data was taken from IETF RFC 7932.

Unlike Adobe's CMaps and the UCD, nothing here is compiled by a build script.
A dictionary is already the form the decoder wants, so `include_bytes!` embeds
it as it is — the Liberation faces' case rather than the other two. That puts
120 KiB in every binary that links `tinker-pdf-filters`, unconditionally and
including the wasm one. It is not behind a feature because there is no safe way
to turn it off: a build without it would decode most streams and silently fail
on the ones that reach Appendix A, which is the worst of the three options.

Four smaller tables out of the same RFC are transcribed into
`crates/tinker-pdf-filters/src/brotli.rs` rather than vendored here, because
they are short enough to read in a diff: Appendix B's 121 word
transformations, and §7.1's `Lut0`, `Lut1` and `Lut2`. They ride under the same
licence, and each is checked against **its own published CRC-32** by a unit
test, which is the same evidence the dictionary has.

### `crates/tinker-pdf-font/data/liberation`

The Liberation family: twelve TrueType faces, four each of Sans, Serif and
Mono, metric-compatible with Arial, Times New Roman and Courier New — which
are in turn the faces every reader substitutes for Helvetica, Times and
Courier. That is twelve of the standard 14 fonts 9.6.2.2 requires a reader to
have; the other two are Symbol and ZapfDingbats, and see below.

They are here because of a measurement rather than an argument.
`corpus/ratchet-fonts.json` is the same 4 525 corpus files rendered with a
face supplied, against `corpus/ratchet.json` without one, and **52 % of all
reported degradation was the absence of a face** — 1 045 files down to 506,
and in qpdf's corpus 530 down to 130. A conforming file that names Helvetica
and embeds nothing is one this engine could not draw, which makes the absence
a conformance gap rather than only a policy
([features/fonts.md](docs/features/fonts.md)).

**Off by default.** The `bundled-fonts` feature is opt-in on `tinker-pdf` and
`tinker-pdf-font`, because a host that has faces of its own should not carry
4.2 MB of ours, and because `FontProvider` is still the better answer where
one exists. With the feature off nothing here is compiled in and the engine
behaves exactly as it always has.

Fetched from the upstream release archive
`liberation-fonts-ttf-2.1.5.tar.gz`, whose SHA-256 is

```text
7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0
```

Only the twelve `.ttf` files are vendored, with upstream's `LICENSE` and
`AUTHORS` beside them for provenance. The sources, the build system and the
changelog are not: nothing here builds a font.

Unlike the other two trees, these are **not** compiled into tables by a build
script — a font program is already the form the rasterizer wants, so
`include_bytes!` embeds them as they are. The OFL's redistribution terms
therefore apply to the bytes themselves rather than to a derived table, which
is the simpler case: clause 2 requires the licence to travel with them, and it
does, twice — beside the data and reproduced here in full:

```text
Digitized data copyright (c) 2010 Google Corporation
	with Reserved Font Arimo, Tinos and Cousine.
Copyright (c) 2012 Red Hat, Inc.
	with Reserved Font Name Liberation.

This Font Software is licensed under the SIL Open Font License,
Version 1.1.

This license is copied below, and is also available with a FAQ at:
http://scripts.sil.org/OFL

SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007

PREAMBLE The goals of the Open Font License (OFL) are to stimulate
worldwide development of collaborative font projects, to support the font
creation efforts of academic and linguistic communities, and to provide
a free and open framework in which fonts may be shared and improved in
partnership with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves.
The fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works.  The fonts and derivatives,
however, cannot be released under any other type of license.  The
requirement for fonts to remain under this license does not apply to
any document created using the fonts or their derivatives.

 

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such.
This may include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components
as distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting ? in part or in whole ?
any of the components of the Original Version, by changing formats or
by porting the Font Software to a new environment.

"Author" refers to any designer, engineer, programmer, technical writer
or other person who contributed to the Font Software.


PERMISSION & CONDITIONS

Permission is hereby granted, free of charge, to any person obtaining a
copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,in
   Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
   redistributed and/or sold with any software, provided that each copy
   contains the above copyright notice and this license. These can be
   included either as stand-alone text files, human-readable headers or
   in the appropriate machine-readable metadata fields within text or
   binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
   Name(s) unless explicit written permission is granted by the
   corresponding Copyright Holder. This restriction only applies to the
   primary font name as presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
   Software shall not be used to promote, endorse or advertise any
   Modified Version, except to acknowledge the contribution(s) of the
   Copyright Holder(s) and the Author(s) or with their explicit written
   permission.

5) The Font Software, modified or unmodified, in part or in whole, must
   be distributed entirely under this license, and must not be distributed
   under any other license. The requirement for fonts to remain under
   this license does not apply to any document created using the Font
   Software.


 
TERMINATION
This license becomes null and void if any of the above conditions are not met.

 

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT.  IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER
DEALINGS IN THE FONT SOFTWARE.
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

`testdata/` holds PDFs written by mutool, copied from Tinker; see
[`testdata/README.md`](testdata/README.md). They are inputs to tests and are
not redistributed in any built artefact.

## What is deliberately not here

No typefaces beyond the twelve above. Symbol and ZapfDingbats — the two of the
standard 14 that are not text faces — have no Liberation equivalent, and
substituting a text face for a symbolic font draws confidently wrong glyphs,
which `FontProvider::substitute` names as the reason declining is a legitimate
answer. Nothing reads a font directory either: that is an operating-system
dependency, and `wasm32-unknown-unknown` has no filesystem at all. A host with
faces of its own still supplies them through `FontProvider`, which remains the
seam whether the feature is on or off.
