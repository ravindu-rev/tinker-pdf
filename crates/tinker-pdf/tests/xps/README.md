# Real XPS packages

Nothing in this repository wrote a byte of any package here. That is the whole
point of the directory, and it is gap 30 milestone 1's deliverable.

[Gap 29](../../../../docs/plans/gaps/29-cbz.md) closed having **never opened a
`.cbz` produced by a real archiver**. Every fixture in it was hand-built from
APPNOTE 6.3.10's field layouts, three milestones recorded the debt as owed, and
the sixth had to write it into the gap's closing section as a limitation of the
whole gap: *"The first real archive this meets may find something, and nothing
here would have."* [Gap 30](../../../../docs/plans/gaps/30-xps.md) does not
repeat that, and the way it does not is structural — obtaining real documents is
its **first** milestone, before the XML parser, before the package layer, before
any reader code at all. Every later milestone's fixtures come from these files.

## What produced them

Two Microsoft serialisers, both on the machine described below, neither needing
a printer nor elevation.

| Producer | Files | What it is |
| --- | --- | --- |
| `System.Windows.Xps.Packaging.XpsDocument` | `wpf-*.xps` | WPF's `ReachFramework`, .NET Framework 4.8.9337.0. Microsoft's own XPS serialiser, driven from `make-corpus.ps1`. Writes the **XPS 1.0** dialect, `http://schemas.microsoft.com/xps/2005/06`. |
| The XPS Document API's object model | `xpsom-*.oxps` | The "XPS Object Factory" coclass `{E974D26D-3D9B-4D47-88CC-3872F2DC3585}`, served by `XpsServices.dll` 10.0.26100.8972, driven from `to-openxps.ps1`. Reads one of the `.xps` files above and writes it back through `IXpsOMPackage1::WriteToFile1` with `XPS_DOCUMENT_TYPE_OPENXPS`. Writes the **OpenXPS** dialect, `http://schemas.openxps.org/oxps/v1.0`. |

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

Machine: Windows 11 Pro 25H2, build 10.0.26200.9168, x64. Produced 18 August
2026.

## Whether they may be committed

Yes, and there is an in-tree precedent rather than a judgement call.
`fuzz/README.md` records that the JPX seed corpus holds *"codestreams
`opj_compress` made from **our own** 32 × 32 images"*, under the reading that a
tool's output on our input is ours to commit, while ISO/IEC 15444-4's
conformance codestreams stay out. The content of every package here — four
coloured quadrants under a white diagonal, three rectangles, two gradients,
eight characters of text — is authored in `make-corpus.ps1`, in this
repository. These are our input through their tool, in exactly that place.

**The font is the one part that is not ours, and it was chosen for that
reason.** The text is set in **Cascadia Mono**, which ships with Windows and is
the only font on a stock Windows 11 install whose own `name` table carries the
SIL Open Font License grant: *"Permission is hereby granted, free of charge, to
any person obtaining a copy of the Font Software, to use, study, copy, merge,
embed, modify, redistribute, and sell modified and unmodified copies"*, and,
decisively for a committed document, *"The requirement for fonts to remain under
this license does not apply to any document created using the Font Software."*
Its `fsType` is `Installable`. The Monotype faces beside it in
`C:\Windows\Fonts` carry no such grant and none is used here.

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

172 868 bytes in total, of which 151 700 is one font part written twice — see
below.

**Nothing here was produced by anything that is not Windows.** Milestone 1's
criterion asks for one *"if one can be found"*, and none was: no LibreOffice,
Ghostscript or Inkscape is installed on this machine, and nothing else on it
emits XPS. Recorded as owed rather than quietly dropped.

## The scripts

`make-corpus.ps1` writes the six `.xps` files and calls `to-openxps.ps1` for
the two `.oxps`. `inventory.ps1` writes `INVENTORY.tsv`. All three read no
network and touch nothing outside this directory.

```powershell
powershell.exe -NoProfile -STA -ExecutionPolicy Bypass `
    -File crates\tinker-pdf\tests\xps\make-corpus.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass `
    -File crates\tinker-pdf\tests\xps\inventory.ps1
```

Windows PowerShell 5.1 and `-STA`, because WPF will not build a visual on an
MTA thread.

**Re-running them does not reproduce these bytes.** Both serialisers mint a
fresh GUID for every resource part and a fresh `Id` for every relationship, so a
second run produces different part names and a different file. The committed
files are the record; the scripts are how they were obtained. That is why
`.gitattributes` marks them binary and why the table above carries hashes.

`INVENTORY.tsv` names every item of every package, its media type as OPC
7.2.3.5 resolves it, its ZIP compression method and both its sizes.
`tests/xps.rs`'s `inventory_matches_the_packages` recomputes the same table
through `tinker-pdf-zip` on every `cargo test` and compares — so the inventory
cannot drift from the files, and two independent ZIP readers, .NET's and ours,
have to agree about all fifty-two rows.

It is also the first time this repository's own archive reader has been pointed
at an archive it did not write — the exact debt gap 29 closed with. It read all
eight by the central-directory route with no warnings and no leniency of any
kind, and the test asserts both, because "it worked" is not a measurement.

## What these files already showed, that ECMA-388 does not say

The plan predicted seven of these from two probe packages. The rest are new, and
each is a thing a fixture written from the standard would have got wrong.

- **`[Content_Types].xml` is the last item of every package, in both dialects.**
  OPC 7.3.7 leaves its position unconstrained. A reader that assumes it is first
  is wrong on the first real file it meets.
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
- **Colours come in two spellings.** `Fill="#FF000000"` and
  `Color="#FFDC143C"` in XPS 1.0; `Fill="#000000"` and `Color="#dc143c"` —
  six digits, lower case — in OpenXPS.
- **Abbreviated geometry comes in two spellings too.** `M0,0L200,0 200,200
  0,200Z` from WPF against `M 0,0 L 200,0 200,200 0,200 Z` from the object
  model. Both are 11.2.3 and a reader needs both.
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
