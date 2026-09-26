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
| `crates/tinker-pdf-shape/data/aots` | [adobe-type-tools/aots](https://github.com/adobe-type-tools/aots) at `d256691` (2025-11-29), fonts via [harfbuzz/harfbuzz](https://github.com/harfbuzz/harfbuzz) `test/shape/data/aots/fonts` at `e0d7060` (2021-08-12) | `Apache-2.0` |
| `crates/tinker-pdf-shape/data/ucd` | [The Unicode Character Database](https://www.unicode.org/Public/17.0.0/ucd/), version 17.0.0 (2025-07-29) | `Unicode-3.0` |
| `crates/tinker-pdf-shape/data/text-rendering-tests` | [unicode-org/text-rendering-tests](https://github.com/unicode-org/text-rendering-tests) at `26cfb96` (2026-08-24) | `Unicode-3.0` |
| `crates/tinker-pdf-xml/data/xhtml-entities` | XHTML 1.0's three entity sets as XHTML Modularization 1.1 (2010-07-29) publishes them, from [w3c/markup-validator](https://github.com/w3c/markup-validator) `htdocs/sgml-lib/REC-xhtml-modularization-20100729/` at `724a15b` (fetched 2026-09-26) | `W3C` |
| `crates/tinker-pdf-content/data/ucd` | The Unicode Character Database, version 17.0.0, via [unicode-org/unicodetools](https://github.com/unicode-org/unicodetools) `unicodetools/data/ucd/17.0.0` at `0509b4b` (fetched 2026-09-26) | `Unicode-3.0` |

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
mitigation `docs/design/shaping.md`'s risk table names. One file has no header
to compare and is pinned another way; its row below says which.

| File | What it is |
| --- | --- |
| `extracted/DerivedBidiClass.txt`, here as `DerivedBidiClass.txt` | The `Bidi_Class` property, which **is** UAX #9. Its `@missing` lines are applied, unlike `LineBreak.txt`'s block defaults one crate over; `build.rs` says why the two differ |
| `BidiBrackets.txt` | `Bidi_Paired_Bracket` and `Bidi_Paired_Bracket_Type`, which rule N0 is written in terms of |
| `BidiMirroring.txt` | `Bidi_Mirroring_Glyph`, rule L4 |
| `Scripts.txt` | UAX #24's `Script`, which itemization splits a paragraph on |
| `extracted/DerivedJoiningType.txt`, here as `DerivedJoiningType.txt` | `Joining_Type`, which the Arabic cursive-joining state machine is written in. The derived file rather than `ArabicShaping.txt` because it lists the 386 `Transparent` ranges outright instead of leaving them to be re-derived from `General_Category`; `build.rs` says so at length |
| `PropertyValueAliases.txt` | Each script's ISO 15924 code, which an OpenType script tag is derived from, and the long-to-short `Bidi_Class` names the `@missing` lines use |
| `IndicSyllabicCategory.txt`, `IndicPositionalCategory.txt` | What a Brahmic character *is* and which side of its base it is drawn on. The Universal Shaping Engine's cluster model is written in the two together, and neither is enough alone: the pair is what tells a pre-base vowel, which has to be moved in front of its consonant, from an above-base one, which does not |
| `UnicodeData.txt` | `Canonical_Decomposition_Mapping`, field 5, which the cluster model needs before it can reorder: a two-part vowel such as `U+1B40 BALINESE VOWEL SIGN TALING TEDUNG` is drawn on *both* sides of its consonant, and its left half only becomes something that can be moved once the character is `U+1B3E` and `U+1B35`. The UCD publishes no extracted file for it, and this is the one data file it publishes with **no version header** — so `tests/ucd_version.rs` pins it by a repertoire cross-check against `Scripts.txt` rather than by comparing headers, and says which direction of drift that catches |
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

### `crates/tinker-pdf-xml/data/xhtml-entities`

The three character entity sets every XHTML 1.x DTD declares — Latin-1 (96
names), symbols (124) and special (33), **253 names**, each a single code point
— which is what a document whose `<!DOCTYPE` names XHTML 1.0, XHTML 1.1 or
XHTML Basic has in scope as `&nbsp;`, `&mdash;` and the rest. `tinker-pdf-xml`
resolves them under `Doctype::SkipExternalId` when, and only when, the
declaration names one of those DTDs; `build.rs` compiles the three files into a
sorted `(name, char)` array, and nothing under `data/` is opened at run time.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `xhtml-lat1.ent` | 8 758 | `3535a3cf7672ab1a511e4edd094e8e1da8b5874aba8ee8851bd2861d25b0dfd9` |
| `xhtml-symbol.ent` | 12 771 | `5b173003c47aba07879397bccdd23ef240eb7578c6345a84f3453617410b7e7d` |
| `xhtml-special.ent` | 4 259 | `348d006519736b764a86fd24aed49ad35114f030ede0f263d3c4638f04e12107` |
| `LICENSE` | 2 701 | `df7429635bacfb82b3e92c2fff50553949afd97389890d53a67b67cbf9fba68e` |

**Where they came from.** `w3c/markup-validator` is the W3C's own repository
for the Markup Validation Service, and `htdocs/sgml-lib` is the catalogue of
DTDs it validates against; fetched from `raw.githubusercontent.com` at commit
`724a15b0dd40841b03a7a8bc4abde8bdb16c0385` on 26 September 2026. The three
files sit in its `REC-xhtml-modularization-20100729/` directory, which is where
its own `catalog.xml` maps the public identifiers `-//W3C//ENTITIES Latin 1 for
XHTML//EN`, `…Symbols for XHTML//EN` and `…Special for XHTML//EN` — the three
that `xhtml1-strict.dtd`, `xhtml1-transitional.dtd` and `xhtml1-frameset.dtd`
name directly and that XHTML 1.1 and XHTML Basic reach through
`xhtml-framework-1.mod` → `xhtml-charent-1.mod`, each of which was read at the
same commit to check exactly that. The files are byte-for-byte what was
fetched; `.gitattributes` leaves them as text with LF line ends, which is what
they were.

**The licence.** The repository's README states that its contents are under
the *W3C Software License and Notice* at
`http://www.w3.org/Consortium/Legal/2002/copyright-software-20021231`, which is
SPDX `W3C`; the repository carries no licence file of its own, so `LICENSE`
here is SPDX's canonical text of that identifier, from
[spdx/license-list-data](https://github.com/spdx/license-list-data)
`text/W3C.txt` at `31ba1a50e5397e00a304dbadc76531740e89ee48`, verbatim. Each
`.ent` file also carries, inside its own header comment and therefore inside
every copy, ISO 8879's notice for the portions derived from its entity sets —
*"Permission to copy in any form is granted for use with conforming SGML
systems and applications as defined in ISO 8879, provided this notice is
included in all copies"* — which the verbatim copy satisfies. `W3C` was added
to `deny.toml`'s allowlist for this tree; it is a permissive licence the FSF
lists as GPL-compatible and the OSI approves, and it asks for the notice to
travel with the files, which it does.

**Cross-checked against the HTML standard's list, and it disagrees in two
places, both deliberately.** `whatwg/html-build` at
`283a3531a61106d07d9a7d9fb3e6f3b9bfd33d70`, `entities/out/entities.json`
(145 897 bytes, SHA-256
`d741d877ac77c4194c4ad526b5b4a19aef8dfe411ab840a466891cdbb9f362e6`), holds all
253 names. **251 agree** on their code point. The two that do not are `&lang;`
and `&rang;`: XHTML 1.0 declares U+2329 and U+232A (LEFT- and RIGHT-POINTING
ANGLE BRACKET), and the HTML standard maps them to U+27E8 and U+27E9
(MATHEMATICAL LEFT and RIGHT ANGLE BRACKET), having changed them because
U+2329 and U+232A are canonically equivalent to the CJK brackets U+3008 and
U+3009. A document that names an XHTML DTD gets what that DTD declares, so the
table keeps U+2329 and U+232A, and
`every_vendored_name_decodes_to_the_code_point_its_set_declares` pins both.
The cross-check was a script run once over the two files on 26 September 2026,
recorded here as the measurement it was rather than run by any test (ruling
13); the HTML list itself is not vendored — its 2 125 names (2 231 entries with
the semicolon-less legacy spellings) include ones that expand to two code points (`&nGt;` is U+226B U+20D2), which would end
`tinker-pdf-xml`'s invariant that decoded text is never longer than its source,
and it is not what any XHTML DTD declares.

### `crates/tinker-pdf-content/data/ucd`

The Unicode Character Database a third time, at the files
[UAX #29](https://www.unicode.org/reports/tr29/)'s word boundaries need, for
`TextLine::words` and the word boxes it returns, and the two the search
options' diacritic folding needs, for `TextPage::search_with`. A **third** tree
rather than a share of either existing one, for the reason the second exists:
this crate may depend on neither `tinker-pdf-layout` nor `tinker-pdf-shape`
(`cargo xtask dag`), and a build script cannot read across a crate boundary.
`crates/tinker-pdf-shape/tests/ucd_version.rs` holds all three trees to one
Unicode version, one licence text and a fixed list of files each.

unicode.org itself was not reachable from where this was vendored, so the
files were fetched from the Unicode Consortium's own tools repository, which
carries the published release under `unicodetools/data/ucd/<version>/`, pinned
to commit `0509b4b256ff75c65300c8aaecb9e6ec816d9520` so the URL names one set
of bytes:
`https://raw.githubusercontent.com/unicode-org/unicodetools/0509b4b256ff75c65300c8aaecb9e6ec816d9520/unicodetools/data/ucd/17.0.0/<path>`.
Each file's first line states its own version — except `UnicodeData.txt`,
which has no header anywhere — and `emoji-data.txt` and `UnicodeData.txt` are
byte for byte the copies `crates/tinker-pdf-layout/data/ucd` and
`crates/tinker-pdf-shape/data/ucd` already carried; `tests/ucd_version.rs`
asserts the second, since no header can.

| File | Upstream path | SHA-256 | What it is |
| --- | --- | --- | --- |
| `WordBreakProperty.txt` | `auxiliary/WordBreakProperty.txt` | `72274cac1e6b919507db35655c3e175aa27274668a1ece95c28d2069f2ad9852` | The `Word_Break` property, which **is** UAX #29's word algorithm |
| `emoji-data.txt` | `emoji/emoji-data.txt` | `2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b` | `Extended_Pictographic`, which rule WB3c is written in |
| `UnicodeData.txt` | `UnicodeData.txt` | `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` | Field 5's canonical decompositions — published nowhere else — and field 2's `General_Category`, for diacritic-insensitive search |
| `PropList.txt` | `PropList.txt` | `130dcddcaadaf071008bdfce1e7743e04fdfbc910886f017d9f9ac931d8c64dd` | `Diacritic`: a nonspacing mark that is also `Diacritic` is what that search removes, so a Devanagari vowel sign, which is `Mn` and not an accent, stays |
| `WordBreakTest.txt` | `auxiliary/WordBreakTest.txt` | `1de23a75f37904abc7d206239ee8d34f8fdf0fb4ab32a7174dfbabbde25419b2` | **The conformance oracle.** 1 944 cases, every one run by `tests/uax29_conformance.rs` against the function `TextLine::words` calls |
| `LICENSE.txt` | — | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` | The Unicode License v3, byte for byte the file the other two UCD trees carry |

The first four are compiled into static tables by `build.rs` and never opened
at run time; `WordBreakTest.txt` is a test input and is not compiled into
anything. It is here rather than fetched for the reason `LineBreakTest.txt` is:
a skipped oracle exits 0 and reads exactly like a pass. `Unicode-3.0` is
already on `deny.toml`'s allowlist, and its text is reproduced above.

### The predefined XMP schemas' property tables

Transcribed into `crates/tinker-pdf/src/pdfa/xmp_schemas.rs` rather than
vendored as a tree, for the reason the four Brotli tables above are: 443 lines
of `(name, value type)` pairs are short enough to read in a diff, and
`cargo xtask vendor` checks directories under `crates/<crate>/data`, which this
is deliberately not.

**Two sources, because the standards cite two revisions.** ISO 19005-1 cites
the January 2004 revision of Adobe's XMP specification; ISO 19005-2 and
ISO 19005-3 are governed by the September 2005 revision; ISO 19005-4 carries no
predefined-schema requirement at all, so it reads neither table.

| table | document | fetched from | bytes | SHA-256 |
| --- | --- | --- | ---: | --- |
| `PREDEFINED_2004` | *XMP Specification*, Adobe Systems Incorporated, January 2004, 94 pp | `https://printtechnologies.org/standards/files/xmp-specification-jan04.pdf` | 601 059 | `a452d9629814e5dd502dac6d245bb8484a543f5aced178e9405f2e87a41a072d` |
| `PREDEFINED_2005` | *XMP Specification*, Adobe Systems Incorporated, September 2005, 112 pp | `https://printtechnologies.org/standards/files/xmp-specification-sep05.pdf` | 931 213 | `6fd7659bbb8d859aee598b928feb24ec50bb430924fe4038d5ed4f1276988ddc` |

**Nothing is vendored, and what was taken is not either document's text.** Both
PDFs carry "All rights reserved" and neither grants a redistribution licence;
neither file is in this repository and neither is redistributed by anything
this repository builds. What was taken is chapter 4 "XMP Schemas"' tabulated
**facts** — for each property, the namespace URI, the preferred prefix, the
property name and the value type the table's own column prints — read off
pages 37–58 of the January 2004 document and pages 39–70 of the September 2005
one, with the page recorded beside every row and every row read back against
that page a second time before it was written into the source. Eleven schemas
and 169 properties from the first; fourteen and 274 from the second. Each value
type is then narrowed to one of four **forms** an RDF/XML serialisation can be
distinguished into without reading the value — a simple value, an array, a
language alternative, or a structure — which is the only thing the engine
stores and the only thing the rule reads.

Because nothing of either document is reproduced, there is no licence text to
carry here; the rows above record where the facts were read, which is what this
file exists to make checkable. The BSD-3-Clause notice this entry used to carry
belonged to Adobe's `xmp-docs` namespace tables, which were Adobe's *current*
revision and are the tables these two replaced. Nothing from that repository is
in the engine any more, so the notice is gone with it.

The names **are** read as a membership list, and only alongside the exception
that makes one safe. ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 require every
property to belong to a predefined schema *or* be described by an extension
schema, and 6.7.8 is the second half of that sentence: a packet may declare a
schema of its own and carry properties neither revision defines. A membership
rule written before that declaration is read would report every conforming file
that uses one, so neither half landed before the other;
`crates/tinker-pdf/src/pdfa/xmp_extension.rs` reads the declarations and
`crates/tinker-pdf/src/pdfa/xmp_schemas.rs` argues the tables at length. Two
names are read out of these documents and **not** used as membership: the
`pdfaid` identification schema and the `pdfaExtension` vocabularies are ISO
19005's own and no XMP revision prints them, so the rule exempts them by
namespace rather than pretending a table names them.

*Amended 14 September 2026, at the commit that landed the membership half. The
paragraph above said the restraint was what made the tables safe to compile in;
what makes them safe is the exception, and it is now read.*

The evidence that the transcription is right is a count, a difference and a
corpus. `both_revision_tables_are_sorted_and_have_no_duplicate_property` pins
11 schemas and 169 properties against 14 and 274;
`the_two_tables_differ_exactly_where_the_two_specifications_do` pins the three
schemas September 2005 added, the two `xmp` properties it added, the one `exif`
property it dropped, and the single property whose form it changed
(`photoshop:SupplementalCategories`, `Text` on page 47 of January 2004 and
`bag Text` on page 55 of September 2005 — a change the September 2005 document's
own changelog records under April 2005). And the rule runs over all 2 371
annotated PDF/A files the veraPDF corpus carries without moving the
false-positive count by one file: the tables replaced a single table of Adobe's
current revision, on which every shared property kept its form, and the eleven
files the agreement gained are eleven `fail` fixtures whose properties the
cited revisions name and the current tables had dropped.

## Test fixtures

`testdata/` holds PDFs written by mutool, copied from Tinker; see
[`testdata/README.md`](testdata/README.md). They are inputs to tests and are
not redistributed in any built artefact.

`crates/tinker-pdf-crypto/tests/data/cavp/` holds NIST CAVP known-answer
vectors for RSA and ECDSA signature verification and for Triple DES, from the
DSS and block-cipher test-vector archives at csrc.nist.gov. NIST publications
are works of the United States Government and carry no copyright, so there is
no licence to reproduce. Each file's own header comment records the archive it
came from, the date it was fetched, the SHA-256 of that archive, the SHA-256 of
the file inside it, and what was taken — for the two ECDSA files, which curve
groups were dropped and why; for the seven Triple DES files, that nothing was
dropped at all. They are `cargo test` inputs, compiled in only under
`#[cfg(test)]`, and are not in any built artefact.

These sit under `tests/data/` rather than `crates/<crate>/data/` because they
are neither vendored *into* the engine nor redistributed by it; `cargo xtask
vendor`'s allowlist governs the latter, and this is the former.

`crates/tinker-pdf/tests/signature_support/` holds the output of **OpenSSL
3.5.5 (27 Jan 2026)**, run once on this machine and committed — which is the
half of ruling 13 that permits a third-party program to *supply data* and
never to adjudicate one. Three artefacts, and none of them was in this file
before the third arrived:

- `certificates.tsv`, the expected subject, issuer, validity, SPKI digest and
  serial for seventeen certificates the fetched corpora carry, produced
  28 August 2026. Its own header records the commands.
- `pubsec_support/pubsec-rc4-128.pdf` and `content-key.bin` (7.6.5's
  public-key security handler), sealed 28 August 2026; `tests/pubsec.rs`
  states which half of that fixture is interop and which half is one author
  agreeing with themselves.
- `ecdsa-p256.pdf`, `ecdsa-p384.pdf` and the two `-root.der` anchors, built
  14 September 2026 by `ecdsa-fixtures.py` and OpenSSL: two throwaway
  elliptic-curve key pairs, two certificate chains, and two detached CMS
  `SignedData` blobs signed with ECDSA. They exist because **no signature in
  any fetched corpus uses ECDSA**, so the verdict path's P-256 and P-384 arms
  had nothing real to be held to; see
  [`crates/tinker-pdf/tests/signature_support/README.md`](crates/tinker-pdf/tests/signature_support/README.md).

The keys and certificates are generated for the fixtures and belong to nobody;
there is no licence on any of it. Nothing re-runs OpenSSL — `cargo xtask
oracles` refuses a test that tries — and these are `cargo test` inputs that
reach no built artefact.

The seven Triple DES files were added 15 September 2026 with the `des-ede3-cbc`
content cipher, and they are **committed rather than fetched**, deliberately.
This repository commits small vector sets and fetches large corpora, and the
rule that decides between them is that a check which quietly succeeds when its
data is missing is worse than no check. These total 71 KB — smaller than the
ECDSA `SigVer` file already beside them and a fifteenth of the RSA one — so
committing them costs little and makes the gate unconditional: there is no
`SKIPPED` path, because there is nothing to skip. `TCBCvartext`, `TCBCvarkey`,
`TCBCsubtab`, `TCBCpermop` and `TCBCinvperm` come from `KAT_TDES.zip`, and
`TCBCMMT3` and `TCBCMMT2` from `tdesmmt.zip`, both under
`https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/des/`.
The CBC-mode files are taken in preference to the ECB ones because CBC is what
a CMS envelope uses, and every KAT in them is single-block under a zero IV, so
they exercise the bare block cipher as well as the mode. The two `MMT` files
are the multi-block ones, and they are what the KATs cannot be: `TCBCMMT3` is
the only file whose three keys differ, so it is the only one that can see the
order of EDE3's sub-keys, and `TCBCMMT2` is the only published answer keying
option 2 has at all. Both were chosen by a counted-injection campaign rather
than by taste — the sub-key order and the 16-byte key bundle were each caught
by exactly nothing until the file that sees them was committed.

Each of the seven was checked back against its archive when it was committed:
re-fetching both zips, hashing them, and comparing each committed file's body,
after the LF normalisation `.gitattributes` applies to the whole tree, against
the member inside — all seven are byte-for-byte the archive, and the SHA-256s
their headers pin are the SHA-256s the archives yield.

**The tables those vectors adjudicate** are from **FIPS 46-3** (reaffirmed 25
October 1999, withdrawn 19 May 2005), the archived PDF at
`https://csrc.nist.gov/files/pubs/fips/46-3/final/docs/fips46-3.pdf`, SHA-256
`38dc009ca59d391814328fbbf3df0dfe30c69e75dc22b280efd807621e0244b1` — fetched
and hashed again on 15 September 2026, and it is that document. Also a United
States Government work and also uncopyrighted; the document itself is not
committed, only the eight S-boxes and six permutation tables it specifies,
which are the algorithm and not the text.

The vector files sit under `tests/data/` rather than `crates/<crate>/data/`
because they are neither vendored *into* the engine nor redistributed by it;
`cargo xtask vendor`'s allowlist governs the latter, and this is the former.
`crates/tinker-pdf-filters/tests/jxr/*.jxr` are JPEG XR encodings of rasters
this repository authors, produced by the Windows Imaging Component codec
through WPF's `WmpBitmapEncoder` on Windows 11 Pro build 10.0.26200.0, 30
August 2026; see
[`crates/tinker-pdf-filters/tests/jxr/README.md`](crates/tinker-pdf-filters/tests/jxr/README.md).
The *content* of every one of them — a ramp, a gradient, a checkerboard, a
rectangle and a diagonal — is generated by integer arithmetic in
`crates/tinker-pdf-filters/tests/jxr_fixtures.rs`, so these are this
repository's own images through a third party's encoder, which is the same
reading under which `fuzz/corpus/jpx` holds `opj_compress` output of our own
32 x 32 images. ITU-T T.832's conformance bitstreams are **not** here and are
not admissible: they are not freely licensed. They are inputs to tests and are
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
**No ICC profile**, and this is the second decision this file made rather than
recorded. `docs/design/pdfa.md`'s writer profile needs an output intent, an
output intent needs an ICC destination profile, and the obvious convenience is
to vendor sRGB and default to it. The gate at the top of this file decides
otherwise, on the licence and on nothing else:

- **the ICC's own sRGB profiles**, the ones every other producer embeds, carry
  the International Color Consortium's bespoke permission notice. It is
  permissive in substance - copy, distribute, embed, sell, without restriction
  - and it is **not an SPDX-identified licence**. `cargo xtask vendor`
  requires every vendored tree to declare an identifier `deny.toml` already
  allows, and there is no identifier to declare. It fails the gate at the
  first requirement rather than at the allowlist;
- **a third party's CC0 regeneration** would clear the gate on the licence and
  is a different object: one person's rebuild of the numbers, from a personal
  repository rather than the standards body that owns them. Everything else in
  the table above is a *published fact about a file format* - Adobe's own CMap
  resources, the Unicode Character Database, the conformance suites. A colour
  profile is not that. It is a characterisation of a particular device, and
  which device an archival document's colours are *for* is a statement about
  the caller's document.

So `ArchivalProfile::destination_profile` is a mandatory `Vec<u8>` with no
`Option` around it, and the type's own documentation says why. It is the same
answer, for the same kind of reason, that the paragraph above gives about
typefaces: this engine does not decide on a caller's behalf what their file is
made of.
