# Real XPS packages

Nothing in this repository wrote a byte of any `.xps` or `.oxps` here. That is
the whole point of the directory, and it is gap 30 milestone 1's deliverable.
(This repository *did* write the five PDFs under `source/`, which is a different
claim and the one the corpus rests on: our content, their container. See
"What the Ghostscript packages were made from".)

Gap 29 closed having **never opened a
`.cbz` produced by a real archiver**. Every fixture in it was hand-built from
APPNOTE 6.3.10's field layouts, three milestones recorded the debt as owed, and
the sixth had to write it into the gap's closing section as a limitation of the
whole gap: *"The first real archive this meets may find something, and nothing
here would have."* Gap 30 does not
repeat that, and the way it does not is structural — obtaining real documents is
its **first** milestone, before the XML parser, before the package layer, before
any reader code at all. Every later milestone's fixtures come from these files.

## What produced them

Three serialisers from two vendors, all on the machine described below, none
needing a printer nor elevation.

| Producer | Files | What it is |
| --- | --- | --- |
| `System.Windows.Xps.Packaging.XpsDocument` | `wpf-*.xps` | WPF's `ReachFramework`, .NET Framework 4.8.9337.0. Microsoft's own XPS serialiser, driven from `make-corpus.ps1`. Writes the **XPS 1.0** dialect, `http://schemas.microsoft.com/xps/2005/06`. |
| The XPS Document API's object model | `xpsom-*.oxps` | The "XPS Object Factory" coclass `{E974D26D-3D9B-4D47-88CC-3872F2DC3585}`, served by `XpsServices.dll` 10.0.26100.8972, driven from `to-openxps.ps1`. Reads one of the `.xps` files above and writes it back through `IXpsOMPackage1::WriteToFile1` with `XPS_DOCUMENT_TYPE_OPENXPS`. Writes the **OpenXPS** dialect, `http://schemas.openxps.org/oxps/v1.0`. |
| Ghostscript's `xpswrite` device | `gs-*.xps` | GPL Ghostscript 10.07.1, AGPL-3.0-or-later, from Artifex's own `gs10071w64.exe` release — unpacked with 7-Zip rather than installed, because the NSIS installer wants elevation this session does not have. Driven from `make-gs-corpus.ps1`, converting PDFs **this repository wrote**. Writes the **XPS 1.0** dialect, and writes it very differently: see the corpus table and the register below. |

**The third producer is the whole of Tier 4's XPS row.** Milestone 1 asked for a
non-Windows producer *"if one can be found"*, found none, and recorded the debt
in this file rather than dropping it. What closes it is not a non-Windows
machine — it is a serialiser sharing no code, no vendor and no lineage with the
two above, and Ghostscript's is exactly that: it is written in C by Artifex, it
decomposes rather than serialises, and **fifteen of the thirty findings** in the
register below could not have been made without it — three of them by
contradicting something the first eight files had made look like a rule.

**The plan's route 2 — the "Microsoft XPS Document Writer" printer — was tried
and could not be used.** It is supplied by the `Printing-XPSServices-Features`
optional feature; `Get-WindowsOptionalFeature`, `Enable-WindowsOptionalFeature`
and `dism /online /get-featureinfo` each answered *"The requested operation
requires elevation"* (DISM error 740), `mxdwdrv.dll` is absent from
`System32`, no XPS printer is installed, and `Add-PrinterDriver "Microsoft XPS
Document Writer v4"` answers *"The specified driver does not exist in the
driver store."* Elevation was not obtainable non-interactively. The object model
above is what stands in for it: it is a different Microsoft component but it is
Microsoft's OpenXPS writer, so the dialect the printer exists to supply is
represented. What is **not** established is that the printer's byte-level output
matches the object model's — see the milestone's `As built`.

Machine: Windows 11 Pro 25H2, build 10.0.26200.9168, x64. The eight Microsoft
packages were produced 18 August 2026; the five Ghostscript ones 29 August 2026.
The second date matters less than any date in this file usually would, because
those five regenerate byte for byte — see "The scripts".

## What the Ghostscript packages were made from

The `wpf-` and `xpsom-` packages have their content authored in
`make-corpus.ps1`, in PowerShell, against WPF's own object model. The `gs-`
packages could not work that way, because `xpswrite` takes a document rather
than a scene graph — so their content is authored where every other document
this project writes is authored: in **`DocumentBuilder`**, from
`crates/tinker-pdf/tests/xps_corpus_source.rs`, an `#[ignore]`d test that writes
five one-page PDFs into `source/`.

That is the pattern `docs/verification.md` records for the six fuzz seed corpora
*"written by an `#[ignore]`d test in the crate that owns the fixtures, so the
seeds and the fixtures cannot drift"*, and it is here for the same reason: the
writer that produces a fixture and the writer the suite tests have to be **one
writer**. A second, non-`#[ignore]`d test in the same file compares
the committed `source/*.pdf` against what `DocumentBuilder` produces today, so
the drift fails a normal `cargo test` rather than waiting to be noticed.

| Source | Bytes | sha256 | Asks `xpswrite` |
| --- | ---: | --- | --- |
| `source/paths.pdf` | 842 | `8144f912af75a271` | whether filled paths keep their colours, and what becomes of an even-odd fill |
| `source/gradients.pdf` | 1 328 | `5ba5f760b6185f18` | whether an axial or a radial shading survives as a gradient brush |
| `source/images.pdf` | 4 177 | `9bcc2399d7c7347a` | whether a Flate image and a DCT image come out as PNG and JPEG |
| `source/embedded-font.pdf` | 32 804 | `803d640a5a5ab56c` | whether an embedded TrueType face reaches the package as a font part |
| `source/rasterised-text.pdf` | 802 | `ffc113dcdb17c5f4` | what a base-14 name, with no program behind it, costs in markup |

The last two set **the same three lines of text, character for character**, and
differ in exactly one thing: whether a font program is embedded. Any difference
between the two packages is that difference's, which is what makes the pair a
measurement rather than two anecdotes.

**The font in them is Liberation Serif**, from the faces
`crates/tinker-pdf-font/data/liberation` already vendors — OFL-1.1, already on
`deny.toml`'s allowlist and already in `THIRDPARTY.md`. The licence question is
the one Cascadia Mono answers below and the answer is the same clause: *"The
requirement for fonts to remain under this license does not apply to any
document created using the Font Software."* A document set in it is not a
derivative of it.

## Whether they may be committed

Yes, and there is an in-tree precedent rather than a judgement call.
`fuzz/README.md` records that the JPX seed corpus holds *"codestreams
`opj_compress` made from **our own** 32 × 32 images"*, under the reading that a
tool's output on our input is ours to commit, while ISO/IEC 15444-4's
conformance codestreams stay out. The content of every package here — four
coloured quadrants under a white diagonal, three rectangles, two gradients,
eight characters of text, and the five PDFs in `source/` — is authored in this
repository. These are our input through their tool, in exactly that place.

**The font is the one part that is not ours, and it was chosen for that
reason.** The text in the Microsoft packages is set in **Cascadia Mono**, which
ships with Windows and is the only font on a stock Windows 11 install whose own
`name` table carries the SIL Open Font License grant: *"Permission is hereby
granted, free of charge, to any person obtaining a copy of the Font Software, to
use, study, copy, merge, embed, modify, redistribute, and sell modified and
unmodified copies"*, and, decisively for a committed document, *"The requirement
for fonts to remain under this license does not apply to any document created
using the Font Software."* Its `fsType` is `Installable`. The Monotype faces
beside it in `C:\Windows\Fonts` carry no such grant and none is used here.

### Ghostscript is AGPLv3, and that changes nothing here

It is worth stating plainly rather than leaving a reader to stop at the licence
line and wonder, because AGPL-3.0-or-later is the strongest copyleft this
repository has ever been near.

**Nothing of Ghostscript is vendored, linked or redistributed.** No source, no
binary, no header, no `Resource/` file and no font of its is in this tree. It is
not a dependency of any crate in the workspace, it is not on `deny.toml`'s
allowlist because it is not a dependency at all, and it is not fetched, built or
executed by any test or CI job — ruling 13 would not permit one, and
`cargo xtask oracles` fails the build if a test tries. `make-gs-corpus.ps1` is
**how the five files were obtained**, run once by hand, exactly as
`tests/epub/README.md` says of its own `make-corpus.ps1`.

**A converter's copyright does not reach the document it converts.** That is not
a reading invented for this row; it is the reading this repository already
applies three times over, and each one is a stronger case than the last:

- `fuzz/README.md` and **`opj_compress`** — the JPX seed codestreams, committed
  under exactly this argument;
- `tests/epub/README.md` and **pandoc**, GPL-2.0-or-later — six committed EPUBs,
  where that file says in its own voice *"Neither licence touches the output"*;
- the same file and **calibre**, GPL-3.0-only, for two of those six.

AGPLv3's addition to GPLv3 is §13, the network-interaction clause, which
obliges an *operator of a modified version* to offer its source to users
interacting with it remotely. Ghostscript here is neither modified nor operated
over a network; it was run once, locally, over five PDFs. And Artifex's own
position on this is not ambiguous: the AGPL covers the program, and a document
Ghostscript produces from your input is yours. The five packages below are our
content, in a container format ECMA-388 specifies, laid out by a program whose
licence governs the program.

Recorded, dated and non-negotiable in one direction: if a future change wants
Ghostscript to be *run* by anything in this repository rather than to have been
run once by a person, that is a different question with a different answer, and
ruling 13 answers it first.

## The corpus

`sha256` is the first sixteen hex digits, enough to tell a file from a
regeneration of it.

| File | Bytes | sha256 | What it is for |
| --- | --- | --- | --- |
| `wpf-image-and-text.xps` | 79 153 | `c657e9a0a206bc34` | One 816 × 1056 fixed page: a 32 × 32 PNG behind an `ImageBrush` reached through `{StaticResource}`, and a `<Glyphs>` run in an obfuscated Cascadia Mono carrying `Indices=",53"`. The **raster** package, the **ODTTF** package, and the one the first pinned failure is measured on. |
| `wpf-shapes-only.xps` | 1 743 | `b83a8a33b0771b76` | Three filled `<Path>` elements and **no image part anywhere**. The one the second pinned failure is measured on. |
| `wpf-three-pages.xps` | 2 433 | `835234c011c2ab09` | The **multi-page** package: three `<PageContent>` elements in one `FixedDocument`. |
| `wpf-gradients.xps` | 1 992 | `b0743efa69a95820` | `LinearGradientBrush` with three stops and `RadialGradientBrush` with two, both `MappingMode="Absolute"`. |
| `wpf-tiled-brush.xps` | 2 959 | `a73207f0b25e6b52` | The **tiling brush** package: `TileMode="Tile"` and `TileMode="FlipXY"` over one shared PNG, plus an `Opacity="0.5"`. |
| `wpf-jpeg-image.xps` | 3 744 | `fc1197f036c9eb3f` | A JPEG image part, so the corpus is not all PNG. |
| `xpsom-image-and-text.oxps` | 78 988 | `885456f000c27653` | `wpf-image-and-text.xps` as **OpenXPS**. Same PNG and same ODTTF, byte for byte and under the same part names. |
| `xpsom-gradients.oxps` | 1 856 | `84748205dd6972aa` | `wpf-gradients.xps` as **OpenXPS**. |
| `gs-paths.xps` | 3 091 | `efed72d2a4f939e3` | **The most valuable file in this directory for its size.** Six filled `<Path>` elements carrying `V` and `H` abbreviated commands, which **no Microsoft package in this corpus uses at all**, and *both* fill rules — an explicit `F 1` beside a default even-odd. 1 074 bytes of markup. |
| `gs-gradients.xps` | 67 773 | `234291f36fa4a299` | An axial and a radial shading, **flattened**: 1 114 one-unit `<Path>` boxes in 261 distinct colours, no gradient brush anywhere. Its one fixed page is 65 756 bytes, fifty times the largest Microsoft markup part in the corpus. |
| `gs-images.xps` | 10 613 | `5dbc7c85d50bd571` | A **TIFF** image part and an **ICC profile** part — two things nothing else here has — reached through `{ColorConvertedBitmap …}` inside a `<Path.Fill>`, with whitespace on both sides of twelve `=` signs and tabs between elements. Refused at the element by this build; see below. |
| `gs-embedded-font.xps` | 37 001 | `bf6c5863ecaa4c62` | Three lines set in an **embedded** Liberation Serif, arriving as 589 paths and **no font part at all**. |
| `gs-rasterised-text.xps` | 44 499 | `8d2051dbb6dcafff` | The same three lines with nothing embedded, arriving as 714 paths. The pair is the measurement: embedding a face changes the outlines and not the outcome. |

335 845 bytes in total — 172 868 from the two Microsoft producers, of which
151 700 is one font part written twice, and 162 977 from Ghostscript, of which
149 273 is three packages of text and gradients turned into filled paths. The
five source PDFs under `source/` are a further 39 953. Both halves of that
arithmetic are the same fact told twice: **a real XPS is not the size of the
picture on it**, and the two producers are expensive in opposite directions —
one carries a whole variable font to set eight characters, the other carries no
font at all and pays a path per pixel.

## The scripts

`make-corpus.ps1` writes the six `.xps` files and calls `to-openxps.ps1` for
the two `.oxps`. `make-gs-corpus.ps1` runs Ghostscript over the five PDFs in
`source/` and prints each output's SHA-256. `inventory.ps1` writes
`INVENTORY.tsv`. None reads the network, and none touches anything outside this
directory.

```powershell
powershell.exe -NoProfile -STA -ExecutionPolicy Bypass `
    -File crates\tinker-pdf\tests\xps\make-corpus.ps1
cargo test -p tinker-pdf --test xps_corpus_source -- --ignored --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass `
    -File crates\tinker-pdf\tests\xps\make-gs-corpus.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass `
    -File crates\tinker-pdf\tests\xps\inventory.ps1
```

Windows PowerShell 5.1 and `-STA` for the first, because WPF will not build a
visual on an MTA thread.

**None of these is something CI runs.** They are how the files were obtained.
Ruling 13 would not allow a test to spawn any of them, and `cargo xtask
oracles` fails the build if one tries; `tests/epub/README.md` says the same of
its own `make-corpus.ps1`, in the same words and for the same reason.

**Re-running the Microsoft ones does not reproduce their bytes.** Both
serialisers mint a fresh GUID for every resource part and a fresh `Id` for every
relationship, so a second run produces different part names and a different
file. That is why `.gitattributes` marks the packages binary and why the table
above carries hashes.

**Re-running the Ghostscript one does.** Every `gs-*.xps` in the table above was
produced twice, on two runs, and the two runs agreed to the byte — the SHA-256
in the table is a *check* rather than a record. Three things have to be true at
once for that, and all three are:

- **the source is reproducible.** `DocumentBuilder` is deterministic (ruling 4,
  applied to the writer rather than to the rasterizer), and the test that
  compares `source/*.pdf` against a fresh build says so on every `cargo test`;
- **the container is reproducible.** Ghostscript stamps `2012-02-16 09:15:00` on
  every ZIP entry of every package it writes, stores rather than deflates, and
  derives its one relationship `Id` from the content rather than from a GUID;
- **the markup is reproducible.** Nothing in a `gs-` package carries a date, a
  build string or a generated name.

**This is the first container fixture in this repository that can be
regenerated byte for byte**, and it is worth stating plainly because the
alternative was previously universal. `tests/epub/README.md` records that its
corpus *"is not regenerable byte for byte"* — both producers mint a fresh UUID
for `dc:identifier` and calibre stamps a `dcterms:modified` — and the eight
Microsoft packages here are in the same position. A corpus one can regenerate is
a corpus one can *audit*: a reader who doubts these five files can rebuild them
from `source/` and compare hashes, without trusting this document.

`INVENTORY.tsv` names every item of every package, its media type as OPC
7.2.3.5 resolves it, its ZIP compression method and both its sizes.
`tests/xps.rs`'s `inventory_matches_the_packages` recomputes the same table
through `tinker-pdf-zip` on every `cargo test` and compares — so the inventory
cannot drift from the files, and two independent ZIP readers, .NET's and ours,
have to agree about all **eighty-two** rows. Four of those rows — two
`image/tiff` and two `application/vnd.ms-color.iccprofile`, all four in
`gs-images.xps` — are the first parts of either kind this repository has
resolved through 7.2.3.5 on a real file.

It is also the first time this repository's own archive reader has been pointed
at an archive it did not write — the exact debt gap 29 closed with. It read all
thirteen by the central-directory route with no warnings and no leniency of any
kind, and the test asserts both, because "it worked" is not a measurement.

## What this build does with the Ghostscript packages, today

Four of the five render with nothing owed, and the fifth is refused by name at
exactly two elements. Both halves are asserted rather than described.

`tests/xps/CONSERVATION.tsv` carries a row for twelve of the thirteen packages,
and `xps_conservation.rs`'s sweep holds every one of them to
`conserved == facts`: what the markup states is what the synthesised document
has, in order, at the place 18.1 puts it. The Ghostscript rows are the first in
that file with more than three facts in them — 1 115, 715 and 590 against a
Microsoft maximum of six — so the census's own comparator is now exercised at a
scale eight small packages could never reach.

**`gs-images.xps` is the thirteenth, and it is named rather than dropped.** Its
two pictures are TIFF parts addressed through a `{ColorConvertedBitmap …}`
wrapper naming an ICC profile, and this build refuses that wrapper
(`XpsElementDefect::ImageProfileUnsupported`) because the syntax has nowhere to
put an sRGB fallback — so ruling 2's grey placeholder is what reaches the page.
The markup states two pictures; the document carries two grey rectangles; both
sides are right, and no census can make them equal. Widening the markup walk
until they agreed would have made the harness agree with the engine about a
picture neither of them drew. So the exclusion is itself a test —
`the_one_package_the_sweep_excludes_diverges_only_by_its_refusal` pins the
divergence to its exact shape and pins the refusal to one deduplicated warning.
**The day this build reads a TIFF, that test fails**, and the package joins the
sweep in the same commit.

That makes `gs-images.xps` the fixture the TIFF row of the same roadmap tier
should start from: a real package, from a real producer, whose image parts are
`image/tiff` and whose `[Content_Types].xml` said they would be before a single
part was read.

## What these files already showed, that ECMA-388 does not say

The plan predicted seven of these from two probe packages. The rest are new, and
each is a thing a fixture written from the standard would have got wrong.
Thirty of them now. **Fifteen arrived with the third producer, and it falsified
three of the fifteen that were already here** — which is the argument for a
second vendor stated as a number rather than as a principle.

- **`[Content_Types].xml` is in no fixed position.** OPC 7.3.7 leaves it
  unconstrained, both Microsoft serialisers put it **last**, and Ghostscript
  puts it **second** — right after `FixedDocumentSequence.fdseq` and before the
  fixed document it types. For eight files "last" looked like a rule; on the
  ninth it was a habit. `the_content_types_item_is_in_no_fixed_position` now
  asserts the clause instead: exactly one such item per package, and its index
  is not a constant across the corpus.
- **A UTF-8 BOM is a WPF habit, not an XPS one.** Every part WPF writes has one.
  **The object model writes none, on any part.** So BOM detection cannot be
  required of a part; a reader that demanded one would refuse every OpenXPS file
  Windows writes.
- **A fixed page part may have no XML declaration at all.** WPF's `.fpage`,
  `.fdoc` and `.fdseq` begin directly with their root element. The object model
  writes `<?xml version="1.0"?>` — *no encoding* — on `.fpage` and
  `<?xml version="1.0" encoding="UTF-8"?>` on the rest, upper case, where WPF
  writes `encoding="utf-8"` lower case in `[Content_Types].xml`. Four spellings
  of the prolog across eight files.
- **A comment sits inside element content**, not in the prolog:
  `<!-- Generated by: Microsoft XPS Object Model, Version: 1.0, Build:
  10.0.26100.8972 -->`, between `<FixedPage>` and `<FixedPage.Resources>`. A
  parser that skips comments only before the root element fails on the first
  real OpenXPS file.
- **`ImageSource` and `FontUri` are absolute in XPS 1.0 and relative in
  OpenXPS.** `/Resources/….png` in the `wpf-` packages against
  `../../../Resources/….png` in the `xpsom-` ones, for the same part. So
  relative-reference resolution is owed on *markup attributes* and not only on
  relationship targets, and it resolves against the fixed page part's name.
- **Relationship targets are relative in both dialects** —
  `Target="../../../Resources/….png"` — beside the absolute `ImageSource` in
  the same XPS 1.0 file. Both forms in one package, as the plan reported.
- **`<Default Extension="ODTTF" …>` is upper case** against a part named
  `….ODTTF`, in **both** dialects. A byte comparison against `odttf` finds
  nothing; OPC 7.2.3.5's case-insensitivity is not decoration.
- **Colours come in three spellings, and the dialect does not decide which.**
  `Fill="#FF000000"` and `Color="#FFDC143C"` from WPF; `Fill="#000000"` and
  `Color="#dc143c"` — six digits, lower case — from the object model; and
  `Fill="#DB143D"` — six digits, **upper** case — from Ghostscript, in the same
  XPS 1.0 dialect as WPF's eight-digit form. Two producers writing one dialect
  disagree about the spelling, so the dialect is not the thing to key on.
- **Abbreviated geometry comes in three spellings too.** `M0,0L200,0 200,200
  0,200Z` from WPF, `M 0,0 L 200,0 200,200 0,200 Z` from the object model, and
  `M 0,0 V 400 H 533.332 V 0 Z` from Ghostscript. All three are 11.2.3 and a
  reader needs all three.
- **`V` and `H` — vertical and horizontal line-to — appear in no Microsoft
  package at all.** Every `Data` and `Clip` attribute in all eight of them was
  checked: not one command among them. Ghostscript writes them on every path it
  emits, which is thousands. A parser that had never implemented either would
  have passed every test in this repository until the ninth file arrived, and
  that is the single most valuable thing the third producer adds.
- **Both fill rules arrive, and only one of them is spelt.** `gs-paths.xps`
  carries `Data="F 1 M 346.668,133.332 …"` for a nonzero fill and
  `Data=" M 53.332,346.668 …"` — no prefix at all, so 11.2.3's default
  even-odd — for the annulus beside it. The two shapes are otherwise identical
  and differ only in whether the middle is painted. **This found a defect in
  this repository's own conservation harness**: its independent markup scanner
  stripped `F0` and `F1` and not `F 1`, so 11.2.3's `"F" wsp* ("0"|"1")` was
  half-implemented, and two of six marks censused as having no bounds at all.
  Nothing in eight Microsoft packages could have shown it, because none of them
  writes an `F`.
- **The markup is not one line.** WPF writes newlines and four-space indentation
  *inside* `FixedPage.Resources`, with no `xml:space`, so inter-element
  whitespace is real and ignorable.
- **`Indices=",53"` — 12.1.3's empty `GlyphIndex`** — survives both serialisers
  unchanged. A parser that requires a digit before the comma fails here.
- **The object model drops what is default.** `TileMode="None"`,
  `SpreadMethod="Pad"` and `ColorInterpolationMode="SRgbLinearInterpolation"`
  are written by WPF and absent from the OpenXPS twin of the same page.
- **`_rels/.rels` is stored in the WPF packages and deflated in the object
  model's**, while every image part is stored and every ODTTF deflated. Both
  ZIP methods appear in one corpus; neither producer is consistent about it.
- **Eight characters of text cost a 189 252-byte font part.** WPF's subsetter
  keeps a variable font's `gvar` table whole — 142 688 of those bytes — out of
  Cascadia Mono's 371 352. It is 75 850 bytes deflated, and the two copies of
  it are 88 % of this corpus. Comfortably under `MAX_ZIP_ENTRY_BYTES` (128
  MiB), but it is a measured figure for the ledger's *"the most any fixture in
  this repository legitimately spends"* column and for the peak-memory
  measurement milestone 9 owes — and its shape is the point: the font part of a
  real XPS is not proportional to the text on the page.
- **A dialect conversion does not re-obfuscate the font.** The ODTTF part keeps
  its name, its GUID, its content type and all 189 252 of its bytes across the
  two `image-and-text` packages, so the de-obfuscation key is the same in both.
- **A producer may deflate nothing at all.** Every one of the thirty entries
  Ghostscript writes is **stored**: compressed size equals uncompressed size, on
  markup, on a TIFF and on an ICC profile alike. WPF deflates everything but
  `_rels/.rels`; the object model deflates that too. Three producers, three
  policies, and OPC 7.3.6 permits all three — so "stored" and "deflate" are not
  a property of the format or of the part, and `INVENTORY.tsv` has to carry the
  method per item because nothing else predicts it.
- **A package may have a fixed timestamp**, and this one does: `2012-02-16
  09:15:00` on every entry of every `gs-*.xps`, which is neither today nor the
  source document's date nor Ghostscript's own release date. It is why these
  five files regenerate byte for byte, and why they are the only fixtures in
  this repository that do.
- **Per-part relationships are written only where a part needs one.**
  `gs-images.xps` has `Documents/1/Pages/_rels/1.fpage.rels` naming four
  required resources; the other four Ghostscript packages have **only**
  `_rels/.rels`, and their spine is resolved entirely through `Source=` markup
  attributes with no relationship of any kind between the sequence, the
  document and the page. WPF writes a page `.rels` whenever a page has a
  resource and never otherwise, which looks the same until you meet a package
  with no resources at all — so a reader that required per-part relationships to
  walk a spine reads four of these five as empty.
- **And when they are written, the targets are absolute.**
  `Target="/Documents/1/Resources/Images/0.tif"` in a page's own `.rels`, where
  WPF writes `Target="../../../Resources/….png"` in the same place. So the
  relative-reference resolution milestone 3 owed is needed for one producer and
  the absolute form for another, in the same file position, in the same dialect.
- **`[Content_Types].xml` declares media types for parts the package does not
  contain.** Ghostscript writes the identical 753-byte content-types item into
  every package it produces, and it names `image/tiff`, `image/png`,
  `application/vnd.ms-opentype` and `application/vnd.ms-color.iccprofile`
  whether or not one such part exists — `gs-paths.xps` holds five markup parts
  and nothing else, and still declares all four. A reader that inferred a
  package's contents from its `<Default>` list would be wrong about four things
  in the smallest file here. It is also **direct evidence for the TIFF row of
  this roadmap tier**: the producer says up front that it may emit one, and in
  `gs-images.xps` it does.
- **Whitespace is allowed on both sides of an `=`, and a real producer uses
  it.** `<Default Extension = "icc" ContentType =
  "application/vnd.ms-color.iccprofile" />` — one attribute pair in a file whose
  every other attribute has no space at all, in every `gs-` package. In
  `gs-images.xps`'s fixed page it is not an outlier but the house style: twelve
  attributes spaced that way, on `ImageSource`, `ViewboxUnits`, `Viewport`,
  `TileMode`, `Matrix`, `Target`, `Id` and `Type`. XML permits it; a hand-rolled
  parser that assumed otherwise would fail here, and this one does not.
- **An attribute value may begin with a space that is part of the value.**
  `Clip=" M 0,0 L 533,0 L 533,400 L 0,400 Z"` and `Data=" M 53.332,346.668 …"`.
  11.2.3's grammar allows leading whitespace before the first command, so the
  value is well formed and a scanner that trimmed nothing reads a command it
  does not recognise as the first character.
- **Text may arrive as no text at all.** Ghostscript writes **zero `<Glyphs>`
  elements** and **zero font parts** across all five packages, embedded face or
  not: three lines become 714 filled `<Path>` elements when the face is
  Helvetica-by-name and 589 when a Liberation Serif is embedded in the source
  PDF, each path one scanline of one glyph — `M 46,77 V 78 H 51 V 77 Z`. The two
  documents set the same three lines and differ only in the embedding, so the
  finding is not "it substitutes a font"; it is that **this producer has no
  glyph path at all**. A consequence worth naming: the `.ttf` default in its own
  content-types item is never used, so the **unobfuscated** font route through
  `xps/font.rs` still has no real file behind it — `xps_glyphs.rs` reaches it
  with a synthetic package and nothing else does.
- **A gradient may not survive being written.** `gs-gradients.xps` states an
  axial and a radial shading as **1 114 one-unit filled boxes in 261 distinct
  colours**, with no `LinearGradientBrush` or `RadialGradientBrush` anywhere.
  The cost is
  quadratic in the painted area and it is not a small constant: the same two
  shadings over the 360 × 90 pt and 160 × 160 pt regions they were first drawn
  at produced a **5.3 MB** package — thirty times the whole corpus that existed
  before it — from a 1.3 KB source PDF. The committed file paints them at 80 × 40
  pt and 16 × 16 pt for that reason and no other.
- **A page dimension may be truncated rather than rounded.** A 400 pt page is
  400 × 96/72 = 533.333 XPS units, and Ghostscript writes `Width="533"` — so
  this reader answers 399.75 pt for a document that went in at 400. That is the
  producer's arithmetic, not this reader's, and it is recorded here so nobody
  goes looking for a rounding bug in `xps.rs`. `Height="400"` for a 300 pt page
  is exact, which is why one dimension shows it and the other does not.
- **A brush may be nested in place rather than named.** WPF states its brushes
  in `<FixedPage.Resources>` and refers to them as `{StaticResource b0}`;
  Ghostscript writes `<Path><Path.Fill><ImageBrush …><ImageBrush.Transform>
  <MatrixTransform …/></ImageBrush.Transform></ImageBrush></Path.Fill></Path>`,
  which is **seven levels** of element nesting where the deepest Microsoft
  package reaches six. Two producers, one dialect, and the property-element form
  is the only one either of them writes for that brush.
- **Inter-element whitespace may be tabs**, and a real fixed page carries
  thirty-two of them. WPF indents with four spaces inside
  `FixedPage.Resources`; Ghostscript indents with tabs inside `Path.Fill`. There
  is no `xml:space` on either, so both are ignorable and both have to be read.
- **`xml:lang` arrives in two cases.** `en-us` from both Microsoft producers,
  `en-US` from Ghostscript, on the `FixedPage` element of every fixed page in
  the corpus. BCP 47 says a language tag's case carries no meaning, so a
  comparison against a literal is a comparison that works until it does not —
  which is what `xml_real_packages.rs` asserted before this producer arrived.

## The ODTTF key order, checked against a real file

9.1.7.3 [M2.53]'s permutation is *"B37, B36, B35, B34, B33, B32, B31, B30, B20,
B21, B10, B11, B00, B01, B02, B03"*, which the plan rightly calls entirely
unmemorable. Read against a part name written
`B03B02B01B00-B11B10-B21B20-B30B31-B32B33B34B35B36B37`, it is **exactly the
sixteen bytes of the hex string reversed** — and writing it out as the B-names
rather than as a reversal got two pairs transposed on the first attempt here,
which produced a font whose first eight bytes were right, whose table tags were
right, and whose `searchRange`, `entrySelector` and `rangeShift` were garbage.
That is the failure mode the plan warns about, reached in ten minutes.

For `Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF` in
`wpf-image-and-text.xps`:

```text
first 16 obfuscated  01 f4 52 a0 77 de 33 a0 a5 4c e8 5b eb 62 15 1e
key                  01 f5 52 a0 77 c6 32 a0 a5 48 e8 db af 31 5c 59
first 16 clear       00 01 00 00 00 18 01 00 00 04 00 80 44 53 49 47
```

Which reads as sfnt version `0x00010000`, twenty-four tables, `searchRange` 256,
`entrySelector` 4, `rangeShift` 128, and the first table tag `DSIG` — 9 116
bytes of it, followed by `GDEF`, `GPOS` and `GSUB`. The transposed key got the
first eight of those bytes right and `searchRange`, `entrySelector` and
`rangeShift` wrong, which is a font that still parses far enough to look
plausible. The bytes are identical in `xpsom-image-and-text.oxps`, under the
same part name. Milestone 7's criterion is to assert the de-obfuscated bytes
rather than that a page drew, and this is the reference it asserts against.
