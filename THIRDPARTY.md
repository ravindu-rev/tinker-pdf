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
| `crates/tinker-pdf-font/data/liberation` | [liberationfonts/liberation-fonts](https://github.com/liberationfonts/liberation-fonts), release `2.1.5` (2021-10-01) | `OFL-1.1` |
| `crates/tinker-pdf-shape/data/aots` | [adobe-type-tools/aots](https://github.com/adobe-type-tools/aots) at `d256691` (2025-11-29), fonts via [harfbuzz/harfbuzz](https://github.com/harfbuzz/harfbuzz) `test/shape/data/aots/fonts` at `e0d7060` (2021-08-12) | `Apache-2.0` |
| `crates/tinker-pdf-shape/data/ucd` | [The Unicode Character Database](https://www.unicode.org/Public/17.0.0/ucd/), version 17.0.0 (2025-07-29) | `Unicode-3.0` |
| `crates/tinker-pdf-shape/data/text-rendering-tests` | [unicode-org/text-rendering-tests](https://github.com/unicode-org/text-rendering-tests) at `26cfb96` (2026-08-24) | `Unicode-3.0` |

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

### `crates/tinker-pdf-font/data/liberation`

The Liberation family: twelve TrueType faces, four each of Sans, Serif and
Mono, metric-compatible with Arial, Times New Roman and Courier New — which
are in turn the faces every reader substitutes for Helvetica, Times and
Courier. That is twelve of the standard 14 fonts 9.6.2.2 requires a reader to
have; the other two are Symbol and ZapfDingbats, and see below.

They are here because of a measurement rather than an argument.
`corpus/ratchet-fonts.json` is the same 4 525 corpus files rendered with a
face supplied, against `corpus/ratchet.json` without one, and **65 % of all
reported degradation was the absence of a face** — 973 files down to 343,
and in qpdf's corpus 530 down to 110. A conforming file that names Helvetica
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

### `crates/tinker-pdf-shape/data/aots`

Adobe's **annotated OpenType specification** test suite: the specification with
a test case attached to almost every clause of `GSUB`, `GPOS` and `GDEF` — a
tiny font exercising one lookup type in one format, a sequence of glyph indices
to feed it, and the glyphs or positions a correct implementation must produce.
`docs/design/shaping.md` names it as one of the two conformance bars for
`tinker-pdf-shape`, and ruling 13 is what makes it admissible: the expected
output is **inside the fixture**, written by the people who wrote the
specification, so nothing outside this repository is being asked whether the
answer is right.

| File | What it is |
| --- | --- |
| `cases.txt` | 275 cases distilled from `src/opentype.xml`: the `inputs`, `outputs`, `xdeltas` and `ydeltas` of every `<aots:gsub-test>`, `<aots:gpos-test>` and `<aots:context-test>`, carried over verbatim |
| `fonts/*.otf` | 185 compiled test faces, one or more per case |
| `LICENSE.md` | Upstream's own, kept beside the data |

**The fonts came the long way round, and it is worth being plain about it.**
aots ships its faces as XML and compiles them with a Java toolchain — Saxon,
plus a compiler generated from that XML by an XSLT stylesheet — which was not
available where this was vendored, so they could not be built from source here.
The `.otf` files are the compiled aots fixtures as redistributed by the
HarfBuzz project, under the same Apache-2.0 licence they carry upstream. They
are *bytes*, which ruling 13 keeps admissible with provenance recorded; **no
expected output was taken from HarfBuzz**, and every number `tests/aots.rs`
asserts against comes from Adobe's XML. The risk that leaves — that a font
differs from what aots's own compiler would emit — is named in that test file
rather than absorbed, and it is bounded by the two halves having independent
origins: a mismatch shows up as a failure rather than as agreement.

`cases.txt` is the committed output of a script run once against
`src/opentype.xml`, which ruling 13 calls a dated measurement rather than a
check. The XML itself is 2.9 MB of specification prose around the numbers and
is not vendored; the distillation keeps the numbers and the case identifiers.

Apache-2.0 was already on `deny.toml`'s allowlist, so nothing there moved. Its
notice, from `LICENSE.md` beside the data:

```text
Copyright 2000-2016 Adobe Systems Incorporated. All Rights Reserved.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use these files except in compliance with the License.
You may obtain a copy of the License at

 http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

These are test inputs and reach no built artefact. They **do** ship with the
published crate: `exclude = ["data/"]` was in the manifest until the release
pipeline's `a_crate_that_vendors_data_publishes_it` refused it, and the manifest
now records at length why — the corpus is the evidence the crate is right, and a
`cargo test` run by whoever downloaded it should reach the same cases it reaches
here.

### `crates/tinker-pdf-shape/data/ucd`

The Unicode Character Database again, at the files [UAX #9](https://www.unicode.org/reports/tr9/)'s
bidirectional algorithm and [UAX #24](https://www.unicode.org/reports/tr24/)'s
script property need. It is a **second** tree rather than a share of
`tinker-pdf-layout`'s because ruling 8 keeps the two crates leaves: neither
depends on the other, and a build script cannot read across a crate boundary.
The cost of the duplication is version skew, which
`tests/ucd_version.rs` asserts away by comparing the two trees' headers — the
mitigation `docs/design/shaping.md`'s risk table names.

| File | What it is |
| --- | --- |
| `extracted/DerivedBidiClass.txt`, here as `DerivedBidiClass.txt` | The `Bidi_Class` property, which **is** UAX #9. Its `@missing` lines are applied, unlike `LineBreak.txt`'s block defaults one crate over; `build.rs` says why the two differ |
| `BidiBrackets.txt` | `Bidi_Paired_Bracket` and `Bidi_Paired_Bracket_Type`, which rule N0 is written in terms of |
| `BidiMirroring.txt` | `Bidi_Mirroring_Glyph`, rule L4 |
| `Scripts.txt` | UAX #24's `Script`, which itemization splits a paragraph on |
| `extracted/DerivedJoiningType.txt`, here as `DerivedJoiningType.txt` | `Joining_Type`, which the Arabic cursive-joining state machine is written in. The derived file rather than `ArabicShaping.txt` because it lists the 386 `Transparent` ranges outright instead of leaving them to be re-derived from `General_Category`; `build.rs` says so at length |
| `PropertyValueAliases.txt` | Each script's ISO 15924 code, which an OpenType script tag is derived from, and the long-to-short `Bidi_Class` names the `@missing` lines use |
| `BidiTest.txt` | **A conformance oracle.** Every combination of `Bidi_Class` values up to length four: 490 846 data lines, 770 241 resolutions |
| `BidiCharacterTest.txt` | **The other one.** 91 707 cases of real code points, and the only one of the two that reaches bracket pairs |

The last two are 15 MB between them and are not compiled into anything. They
are here for the reason `LineBreakTest.txt` is here one crate over, and gap
20's finding holds a fifth time: **a skipped oracle exits 0 and reads exactly
like a pass.** They compress to a small fraction of that in a published crate,
which is what `crates.io`'s ceiling is measured against.

`LICENSE.txt` is upstream's own, byte for byte the same file
`crates/tinker-pdf-layout/data/ucd` carries — asserted, so that one tree cannot
end up describing terms the other was not given under. The Unicode License v3
is `Unicode-3.0` in SPDX terms, which `deny.toml` already permits; its text is
reproduced above.

### `crates/tinker-pdf-shape/data/text-rendering-tests`

Unicode's own **text-rendering-tests**: real fonts, real strings, and the
glyphs and positions a correct implementation must produce, written into each
fixture as SVG. `docs/design/shaping.md` names it as the second of the two
conformance bars for `tinker-pdf-shape`, beside aots, and ruling 13 is what
makes it admissible — the expected output is inside the fixture, put there by
the people who publish the tests.

| File | What it is |
| --- | --- |
| `testcases/CMAP-{1,2,3,4}.html`, `GSUB-{1,2,3}.html`, `GPOS-{1,2,3,4,5}.html` | The twelve sections milestone 2 is graded on, **verbatim** — parsed by `tests/text_rendering.rs` rather than distilled first, so nothing stands between what upstream wrote and what the test asserts |
| `testcases/SHARAN-1.html` | Milestone 4's whole bar: Nasta‘līq, the one Arabic-script section the corpus has |
| `testcases/SHBALI-{1,2,3}.html`, `SHKNDA-{1,2,3}.html`, `SHLANA-{1,…,10}.html` | Milestone 5's: Balinese, Kannada and Tai Tham — the corpus's only Indic and Southeast Asian sections |
| `fonts/*.ttf`, `fonts/*.otf` | The sixteen faces those sections name |
| `LICENSE` | Upstream's own |

Only those twenty-nine sections are vendored. The AAT (`MORX`, `MORT`),
variable (`GVAR`, `CVAR`, `AVAR`, `HVAR`) and outline (`CFF`, `GLYF`, `SFNT`)
sections are not: `morx` is a stated non-goal, the variable ones are deferred
under ruling 3, and the outline ones test a rasterizer this crate is not.

The seventeen shaping sections and their six faces arrived with milestones 4
and 5, fetched at the same pinned commit as the first twelve so that one
`ft:render` string cannot be describing a face from a different revision. What
each of them adjudicates — and, for the ones this repository declines, why —
is in `crates/tinker-pdf-shape/tests/text_rendering.rs`, which is the file that
has to stay honest about them; this table only says where the bytes came from.

Upstream's README states that *"the contents of this repository are governed by
the Unicode Terms of Use and are released under LICENSE"*, and that LICENSE is
the Unicode License v3, the same `Unicode-3.0` the UCD carries and `deny.toml`
already allows. Its text is reproduced above.

## Test fixtures

`testdata/` holds PDFs written by mutool, copied from Tinker; see
[`testdata/README.md`](testdata/README.md). They are inputs to tests and are
not redistributed in any built artefact.

`crates/tinker-pdf-crypto/tests/data/cavp/` holds NIST CAVP known-answer
vectors for RSA and ECDSA signature verification, from the DSS test-vector
archives at csrc.nist.gov. NIST publications are works of the United States
Government and carry no copyright, so there is no licence to reproduce. Each
file's own header comment records the archive it came from, the date it was
fetched, the SHA-256 of that archive, and — for the two ECDSA files — which
curve groups were dropped and why. They are `cargo test` inputs, compiled in
only under `#[cfg(test)]`, and are not in any built artefact.

These sit under `tests/data/` rather than `crates/<crate>/data/` because they
are neither vendored *into* the engine nor redistributed by it; `cargo xtask
vendor`'s allowlist governs the latter, and this is the former.

## What is deliberately not here

No typefaces beyond the twelve above. Symbol and ZapfDingbats — the two of the
standard 14 that are not text faces — have no Liberation equivalent, and
substituting a text face for a symbolic font draws confidently wrong glyphs,
which `FontProvider::substitute` names as the reason declining is a legitimate
answer. Nothing reads a font directory either: that is an operating-system
dependency, and `wasm32-unknown-unknown` has no filesystem at all. A host with
faces of its own still supplies them through `FontProvider`, which remains the
seam whether the feature is on or off.
