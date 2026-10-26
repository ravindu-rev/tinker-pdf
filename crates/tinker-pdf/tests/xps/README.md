# Real XPS packages

Nothing in this repository wrote a byte of any `.xps` or `.oxps` here. That is
the whole point of the directory. (This repository *did* write the five PDFs
under `source/`, which is a different claim and the one the Ghostscript
packages rest on: our content, their container. See "What the Ghostscript
packages were made from".)

## What produced them

Three serialisers from two vendors, all on the machine described below, none
needing a printer nor elevation.

| Producer | Files | What it is |
| --- | --- | --- |
| `System.Windows.Xps.Packaging.XpsDocument` | `wpf-*.xps` | WPF's `ReachFramework`, .NET Framework 4.8.9337.0. Microsoft's own XPS serialiser, driven from `make-corpus.ps1`. Writes the **XPS 1.0** dialect, `http://schemas.microsoft.com/xps/2005/06`. |
| The XPS Document API's object model | `xpsom-*.oxps` | The "XPS Object Factory" coclass `{E974D26D-3D9B-4D47-88CC-3872F2DC3585}`, served by `XpsServices.dll` 10.0.26100.8972, driven from `to-openxps.ps1`. Reads one of the `.xps` files above and writes it back through `IXpsOMPackage1::WriteToFile1` with `XPS_DOCUMENT_TYPE_OPENXPS`. Writes the **OpenXPS** dialect, `http://schemas.openxps.org/oxps/v1.0`. |
| Ghostscript's `xpswrite` device | `gs-*.xps` | GPL Ghostscript 10.07.1, AGPL-3.0-or-later, from Artifex's own `gs10071w64.exe` release — unpacked with 7-Zip rather than installed, because the NSIS installer wants elevation. Driven from `make-gs-corpus.ps1`, converting PDFs **this repository wrote**. Writes the **XPS 1.0** dialect, and writes it very differently: see the corpus table and the list below. |

**Ghostscript's serialiser shares no code, no vendor and no lineage with the
two Microsoft ones**: it is written in C by Artifex, and it decomposes rather
than serialises. Many of the points in the list below hold only because it
contradicts something the Microsoft packages make look like a rule.

**The "Microsoft XPS Document Writer" printer cannot be used here.** It is
supplied by the `Printing-XPSServices-Features` optional feature;
`Get-WindowsOptionalFeature`, `Enable-WindowsOptionalFeature` and
`dism /online /get-featureinfo` each answer *"The requested operation
requires elevation"* (DISM error 740), `mxdwdrv.dll` is absent from
`System32`, no XPS printer is installed, and `Add-PrinterDriver "Microsoft XPS
Document Writer v4"` answers *"The specified driver does not exist in the
driver store."* Elevation is not obtainable non-interactively. The object model
above is what stands in for it: it is a different Microsoft component but it is
Microsoft's OpenXPS writer, so the dialect the printer exists to supply is
represented. What is **not** established is that the printer's byte-level output
matches the object model's.

Machine: Windows 11 Pro 25H2, build 10.0.26200.9168, x64.

## What the Ghostscript packages were made from

The `wpf-` and `xpsom-` packages have their content authored in
`make-corpus.ps1`, in PowerShell, against WPF's own object model. The `gs-`
packages cannot work that way, because `xpswrite` takes a document rather
than a scene graph — so their content is authored where every other document
this project writes is authored: in **`DocumentBuilder`**, from
`crates/tinker-pdf/tests/xps_corpus_source.rs`, an `#[ignore]`d test that writes
five one-page PDFs into `source/`.

That is the pattern the fuzz seed corpora follow — written by an `#[ignore]`d
test in the crate that owns the fixtures, so the seeds and the fixtures cannot
drift — and it is here for the same reason: the writer that produces a fixture
and the writer the suite tests have to be **one writer**. A second,
non-`#[ignore]`d test in the same file compares the committed `source/*.pdf`
against what `DocumentBuilder` produces, so the drift fails a normal
`cargo test` rather than waiting to be noticed.

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
`crates/tinker-pdf-font/data/liberation` vendors — OFL-1.1, on `deny.toml`'s
allowlist and in `THIRDPARTY.md`. The licence question is the one Cascadia Mono
answers below and the answer is the same clause: *"The requirement for fonts to
remain under this license does not apply to any document created using the Font
Software."* A document set in it is not a derivative of it.

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
line and wonder, because AGPL-3.0-or-later is the strongest copyleft anywhere
near this repository.

**Nothing of Ghostscript is vendored, linked or redistributed.** No source, no
binary, no header, no `Resource/` file and no font of its is in this tree. It is
not a dependency of any crate in the workspace, it is not on `deny.toml`'s
allowlist because it is not a dependency at all, and it is not fetched, built or
executed by any test or CI job — ruling 13 would not permit one, and
`cargo xtask oracles` fails the build if a test tries. `make-gs-corpus.ps1` is
**how the five files are made**, run by hand, exactly as
`tests/epub/README.md` says of its own `make-corpus.ps1`.

**A converter's copyright does not reach the document it converts.** That is not
a reading invented for this row; it is the reading this repository applies
elsewhere, and each case is a stronger one than the last:

- `fuzz/README.md` and **`opj_compress`** — the JPX seed codestreams, committed
  under exactly this argument;
- `tests/epub/README.md` and **pandoc**, GPL-2.0-or-later, and **calibre**,
  GPL-3.0-only — committed EPUBs, where that file says *"none of the three
  licences touches the output"*.

AGPLv3's addition to GPLv3 is §13, the network-interaction clause, which
obliges an *operator of a modified version* to offer its source to users
interacting with it remotely. Ghostscript here is neither modified nor operated
over a network; it is run by hand, locally, over five PDFs. And Artifex's own
position on this is not ambiguous: the AGPL covers the program, and a document
Ghostscript produces from your input is yours. The five packages below are our
content, in a container format ECMA-388 specifies, laid out by a program whose
licence governs the program.

If anything in this repository were to *run* Ghostscript, rather than a person
running it by hand, that would be a different question with a different
answer, and ruling 13 answers it first.

## The corpus

`sha256` is the first sixteen hex digits, enough to tell a file from a
regeneration of it.

| File | Bytes | sha256 | What it is for |
| --- | --- | --- | --- |
| `wpf-image-and-text.xps` | 79 153 | `c657e9a0a206bc34` | One 816 × 1056 fixed page: a 32 × 32 PNG behind an `ImageBrush` reached through `{StaticResource}`, and a `<Glyphs>` run in an obfuscated Cascadia Mono carrying `Indices=",53"`. The **raster** package and the **ODTTF** package. |
| `wpf-shapes-only.xps` | 1 743 | `b83a8a33b0771b76` | Three filled `<Path>` elements and **no image part anywhere**. |
| `wpf-three-pages.xps` | 2 433 | `835234c011c2ab09` | The **multi-page** package: three `<PageContent>` elements in one `FixedDocument`. |
| `wpf-gradients.xps` | 1 992 | `b0743efa69a95820` | `LinearGradientBrush` with three stops and `RadialGradientBrush` with two, both `MappingMode="Absolute"`. |
| `wpf-tiled-brush.xps` | 2 959 | `a73207f0b25e6b52` | The **tiling brush** package: `TileMode="Tile"` and `TileMode="FlipXY"` over one shared PNG, plus an `Opacity="0.5"`. |
| `wpf-jpeg-image.xps` | 3 744 | `fc1197f036c9eb3f` | A JPEG image part, so the corpus is not all PNG. |
| `xpsom-image-and-text.oxps` | 78 988 | `885456f000c27653` | `wpf-image-and-text.xps` as **OpenXPS**. Same PNG and same ODTTF, byte for byte and under the same part names. |
| `xpsom-gradients.oxps` | 1 856 | `84748205dd6972aa` | `wpf-gradients.xps` as **OpenXPS**. |
| `gs-paths.xps` | 3 091 | `efed72d2a4f939e3` | **The most valuable file in this directory for its size.** Six filled `<Path>` elements carrying `V` and `H` abbreviated commands, which **no Microsoft package in this corpus uses at all**, and *both* fill rules — an explicit `F 1` beside a default even-odd. 1 074 bytes of markup. |
| `gs-gradients.xps` | 67 773 | `234291f36fa4a299` | An axial and a radial shading, **flattened**: 1 114 one-unit `<Path>` boxes in 261 distinct colours, no gradient brush anywhere. Its one fixed page is 65 756 bytes, fifty times the largest Microsoft markup part in the corpus. |
| `gs-images.xps` | 10 613 | `5dbc7c85d50bd571` | A **TIFF** image part and an **ICC profile** part — two things nothing else here has — reached through `{ColorConvertedBitmap …}` inside a `<Path.Fill>`, with whitespace on both sides of twelve `=` signs and tabs between elements. See below. |
| `gs-embedded-font.xps` | 37 001 | `bf6c5863ecaa4c62` | Three lines set in an **embedded** Liberation Serif, arriving as 589 paths and **no font part at all**. |
| `gs-rasterised-text.xps` | 44 499 | `8d2051dbb6dcafff` | The same three lines with nothing embedded, arriving as 714 paths. The pair is the measurement: embedding a face changes the outlines and not the outcome. |

**A real XPS is not the size of the picture on it**, and the two vendors'
producers are expensive in opposite directions — one carries a whole variable
font to set eight characters, the other carries no font at all and pays a path
per pixel.

## The scripts

`make-corpus.ps1` writes the `wpf-*.xps` files and calls `to-openxps.ps1` for
the `.oxps` ones. `make-gs-corpus.ps1` runs Ghostscript over the five PDFs in
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

**None of these is something CI runs.** They are how the files are made.
Ruling 13 would not allow a test to spawn any of them, and `cargo xtask
oracles` fails the build if one tries; `tests/epub/README.md` says the same of
its own `make-corpus.ps1`, for the same reason.

**Re-running the Microsoft ones does not reproduce their bytes.** Both
serialisers mint a fresh GUID for every resource part and a fresh `Id` for every
relationship, so a second run produces different part names and a different
file. That is why `.gitattributes` marks the packages binary and why the table
above carries hashes.

**Re-running the Ghostscript one does**, so the SHA-256 in the table is a
*check* rather than a record. Three things have to be true at once for that,
and all three are:

- **the source is reproducible.** `DocumentBuilder` is deterministic (ruling 4,
  applied to the writer rather than to the rasterizer), and the test that
  compares `source/*.pdf` against a fresh build says so on every `cargo test`;
- **the container is reproducible.** Ghostscript stamps one fixed timestamp on
  every ZIP entry of every package it writes, stores rather than deflates, and
  derives its one relationship `Id` from the content rather than from a GUID;
- **the markup is reproducible.** Nothing in a `gs-` package carries a date, a
  build string or a generated name.

A corpus one can regenerate is a corpus one can *audit*: a reader who doubts
these five files can rebuild them from `source/` and compare hashes, without
trusting this document. The Microsoft packages, like the EPUB corpus, cannot be
regenerated byte for byte.

`INVENTORY.tsv` names every item of every package, its media type as OPC
7.2.3.5 resolves it, its ZIP compression method and both its sizes.
`tests/xps.rs`'s `inventory_matches_the_packages` recomputes the same table
through `tinker-pdf-zip` on every `cargo test` and compares — so the inventory
cannot drift from the files, and two independent ZIP readers, .NET's and ours,
have to agree about every row. Four of those rows — two `image/tiff` and two
`application/vnd.ms-color.iccprofile`, all four in `gs-images.xps` — are the
corpus's only parts of either kind.

`tinker-pdf-zip` reads every package by the central-directory route with no
warnings and no leniency of any kind, and the test asserts both, because "it
worked" is not a measurement.

## What this build does with the Ghostscript packages

`tests/xps/CONSERVATION.tsv` carries a row for every package, and
`xps_conservation.rs`'s sweep holds every one of them to `conserved == facts`:
what the markup states is what the synthesised document has, in order, at the
place 18.1 puts it. The Ghostscript rows hold far more facts than any Microsoft
row, so the census's own comparator is exercised at a scale small packages
never reach.

`gs-images.xps`'s two pictures are TIFF parts addressed through a
`{ColorConvertedBitmap …}` wrapper naming an ICC profile. The picture is drawn,
and the profile goes into the document as the `/ICCBased` space its samples are
values in. `the_package_that_used_to_be_excluded_now_conserves_with_its_profiles`
asserts both: the pictures are drawn, and there are two `/ICCBased` spaces in the
document, one per picture, because Ghostscript writes a profile part per image.
A wrapper this build does not know is refused, as
`XpsElementDefect::ImageProfileUnsupported`, because its references cannot be
told apart.

## What these files show, that ECMA-388 does not say

Each is a thing a fixture written from the standard would get wrong, and
several hold only because a second vendor's serialiser contradicts what the
first vendor's files make look like a rule.

- **`[Content_Types].xml` is in no fixed position.** OPC 7.3.7 leaves it
  unconstrained, both Microsoft serialisers put it **last**, and Ghostscript
  puts it **second** — right after `FixedDocumentSequence.fdseq` and before the
  fixed document it types. `the_content_types_item_is_in_no_fixed_position`
  asserts the clause: exactly one such item per package, and its index is not
  a constant across the corpus.
- **A UTF-8 BOM is a WPF habit, not an XPS one.** Every part WPF writes has one.
  **The object model writes none, on any part.** So BOM detection cannot be
  required of a part; a reader that demanded one would refuse every OpenXPS file
  Windows writes.
- **A fixed page part may have no XML declaration at all.** WPF's `.fpage`,
  `.fdoc` and `.fdseq` begin directly with their root element. The object model
  writes `<?xml version="1.0"?>` — *no encoding* — on `.fpage` and
  `<?xml version="1.0" encoding="UTF-8"?>` on the rest, upper case, where WPF
  writes `encoding="utf-8"` lower case in `[Content_Types].xml`. Four spellings
  of the prolog across the Microsoft packages.
- **A comment sits inside element content**, not in the prolog:
  `<!-- Generated by: Microsoft XPS Object Model, Version: 1.0, Build:
  10.0.26100.8972 -->`, between `<FixedPage>` and `<FixedPage.Resources>`. A
  parser that skips comments only before the root element fails on the first
  real OpenXPS file.
- **`ImageSource` and `FontUri` are absolute in XPS 1.0 and relative in
  OpenXPS.** `/Resources/….png` in the `wpf-` packages against
  `../../../Resources/….png` in the `xpsom-` ones, for the same part. So
  relative-reference resolution is needed on *markup attributes* and not only
  on relationship targets, and it resolves against the fixed page part's name.
- **Relationship targets are relative in both dialects** —
  `Target="../../../Resources/….png"` — beside the absolute `ImageSource` in
  the same XPS 1.0 file. Both forms in one package.
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
  package at all.** No `Data` or `Clip` attribute in any of them carries one.
  Ghostscript writes them on every path it emits, which is thousands, so a
  parser that never implemented either would pass every Microsoft package and
  fail here.
- **Both fill rules arrive, and only one of them is spelt.** `gs-paths.xps`
  carries `Data="F 1 M 346.668,133.332 …"` for a nonzero fill and
  `Data=" M 53.332,346.668 …"` — no prefix at all, so 11.2.3's default
  even-odd — for the annulus beside it. The two shapes are otherwise identical
  and differ only in whether the middle is painted. 11.2.3's
  `"F" wsp* ("0"|"1")` allows the space, so a scanner that strips `F0` and `F1`
  and not `F 1` is half an implementation; no Microsoft package writes an `F`,
  so only this file shows it.
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
  Cascadia Mono's 371 352. It is 75 850 bytes deflated. Comfortably under
  `MAX_ZIP_ENTRY_BYTES` (128 MiB), and its shape is the point: the font part of
  a real XPS is not proportional to the text on the page.
- **A dialect conversion does not re-obfuscate the font.** The ODTTF part keeps
  its name, its GUID, its content type and all 189 252 of its bytes across the
  two `image-and-text` packages, so the de-obfuscation key is the same in both.
- **A producer may deflate nothing at all.** Every entry Ghostscript writes is
  **stored**: compressed size equals uncompressed size, on markup, on a TIFF
  and on an ICC profile alike. WPF deflates everything but `_rels/.rels`; the
  object model deflates that too. Three producers, three policies, and OPC
  7.3.6 permits all three — so "stored" and "deflate" are not a property of the
  format or of the part, and `INVENTORY.tsv` has to carry the method per item
  because nothing else predicts it.
- **A package may have a fixed timestamp**, and this one does: one timestamp on
  every entry of every `gs-*.xps`, which is neither the run's date nor the
  source document's nor Ghostscript's own release date. It is why these five
  files regenerate byte for byte.
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
  WPF writes `Target="../../../Resources/….png"` in the same place. So
  relative-reference resolution is needed for one producer and the absolute form
  for another, in the same file position, in the same dialect.
- **`[Content_Types].xml` declares media types for parts the package does not
  contain.** Ghostscript writes the identical 753-byte content-types item into
  every package it produces, and it names `image/tiff`, `image/png`,
  `application/vnd.ms-opentype` and `application/vnd.ms-color.iccprofile`
  whether or not one such part exists — `gs-paths.xps` holds five markup parts
  and nothing else, and still declares all four. A reader that inferred a
  package's contents from its `<Default>` list would be wrong about four things
  in the smallest file here. The producer says up front that it may emit a
  TIFF, and in `gs-images.xps` it does.
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
  point is not "it substitutes a font"; it is that **this producer has no
  glyph path at all**. A consequence worth naming: the `.ttf` default in its own
  content-types item is never used, so the **unobfuscated** font route through
  `xps/font.rs` has no real file behind it — `xps_glyphs.rs` reaches it with a
  synthetic package and nothing else does.
- **A gradient may not survive being written.** `gs-gradients.xps` states an
  axial and a radial shading as **1 114 one-unit filled boxes in 261 distinct
  colours**, with no `LinearGradientBrush` or `RadialGradientBrush` anywhere.
  The cost is quadratic in the painted area and it is not a small constant: at
  360 × 90 pt and 160 × 160 pt the same two shadings make a package of several
  megabytes from a 1.3 KB source PDF, so the committed file paints them at
  80 × 40 pt and 16 × 16 pt.
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
  comparison against a literal is a comparison that works until it does not.

## The ODTTF key order, checked against a real file

9.1.7.3 [M2.53]'s permutation is *"B37, B36, B35, B34, B33, B32, B31, B30, B20,
B21, B10, B11, B00, B01, B02, B03"*, which is entirely unmemorable. Read
against a part name written
`B03B02B01B00-B11B10-B21B20-B30B31-B32B33B34B35B36B37`, it is **exactly the
sixteen bytes of the hex string reversed**. Writing it out as the B-names
rather than as a reversal invites transposing two pairs, which produces a font
whose first eight bytes are right, whose table tags are right, and whose
`searchRange`, `entrySelector` and `rangeShift` are garbage — a font that still
parses far enough to look plausible.

For `Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF` in
`wpf-image-and-text.xps`:

```text
first 16 obfuscated  01 f4 52 a0 77 de 33 a0 a5 4c e8 5b eb 62 15 1e
key                  01 f5 52 a0 77 c6 32 a0 a5 48 e8 db af 31 5c 59
first 16 clear       00 01 00 00 00 18 01 00 00 04 00 80 44 53 49 47
```

Which reads as sfnt version `0x00010000`, twenty-four tables, `searchRange` 256,
`entrySelector` 4, `rangeShift` 128, and the first table tag `DSIG` — 9 116
bytes of it, followed by `GDEF`, `GPOS` and `GSUB`. The bytes are identical in
`xpsom-image-and-text.oxps`, under the same part name. `xps_glyphs.rs` asserts
the de-obfuscated bytes against this reference rather than that a page drew.
