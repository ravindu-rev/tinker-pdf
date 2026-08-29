# Comic archives that real archivers wrote

Gap 29 closed having **never opened a `.cbz` a real archiver produced**. Every
fixture it had was built from APPNOTE 6.3.10's field layouts by
`tests/cbz_support/mod.rs`, three of its milestones recorded the debt as owed,
and the sixth had to write it into the gap's closing statement as a limitation
of the whole gap:

> The first real archive this meets may find something, and nothing here would
> have.

[`docs/features/cbz.md`](../../../../docs/features/cbz.md) repeated it as an
honest caveat — *"Every committed fixture is hand-built from APPNOTE's field
layouts; no `.cbz` written by a real archiver has ever been opened by this
engine"* — and [`docs/ROADMAP.md`](../../../../docs/ROADMAP.md) carried it into
tier 4. This directory is that debt paid, and the sentence above turned out to
be right: the first real archives found five things, listed at the bottom.

The device is the one gap 30 used for XPS and gap 31 for EPUB, and it is
deliberate rather than convenient: **author the pictures here, pack them with
somebody else's tool, commit the result.** What comes back is a real
implementation's idea of a ZIP over content nobody else owns.

## What produced them

Eight archives, five of them ZIPs, from four independent implementations on the
machine described below.

| Producer | Files | What it is |
| --- | --- | --- |
| **7-Zip 26.02** (x64) | `7z-*.cbz`, `7z-lzma2.cb7`, `7z-tar.cbt` | ip7z/7zip, LGPL-2.1-or-later with an unRAR restriction. Driven from `make-corpus.ps1`. Writes ZIP at two compression levels, 7z and tar. |
| **WinRAR 7.20** (x64, trial) | `winrar.cbz`, `winrar-rar5.cbr` | RARLAB, proprietary. `WinRAR.exe a -afzip` writes the ZIP; `Rar.exe` writes the RAR. The trial adds no comment or watermark to an archive — the nag is on the console. |
| **.NET `System.IO.Compression`** | `pwsh.cbz` | Through PowerShell's `Compress-Archive`. Microsoft's ZIP writer, and the one most Windows software reaches for. |
| **CPython 3.12 `zipfile`** | `python.cbz` | The standard library's writer, and the one most tooling that touches comics is written against. |

**RAR 4 cannot be produced on this machine, and that is recorded rather than
worked around.** RAR 7.20's `rar.exe` has no `-ma` switch at all: `-ma4`
answers `ERROR: Unknown option: ma4` and its help lists no format-version
switch. This release writes RAR 5 and nothing else. So a RAR 4 decoder, if one
is ever written, has **no first-party fixture here to be held to** — which
under ruling 13 means it would be decoded-but-unadjudicated and would have to
say so by name, exactly as `docs/features/fonts.md` lists shaped-but-unverified
scripts. That is a constraint on the roadmap item, discovered by trying, and it
belongs in this file rather than in a surprise later.

Machine: Windows 11 Pro, x64. Produced 29 August 2026.

## Whether they may be committed

Yes, and by the same reading the two sibling corpora already record rather than
a fresh judgement.

`tests/epub/README.md` establishes the row — *"A real producer's output on our
own text — Ours — **Yes**"* — and argues that a converter's copyright does not
reach the document it converts, applying it to pandoc (GPL-2.0-or-later) and
calibre (GPL-3.0-only). `fuzz/README.md` applies the same reading to
`opj_compress`, and `tests/xps/README.md` to Windows' XPS serialisers. An
archiver is a weaker case than any of those: it does not transform the content
at all, it only frames it. 7-Zip's LGPL, WinRAR's proprietary licence and
Microsoft's do not reach a ZIP of our pictures, nothing of any of them is
vendored, linked or redistributed here, and none is a dependency of anything in
the workspace.

**The pictures are ours without qualification.** They are written by
`cbz_real.rs`'s `write_the_source_pages`, through `cbz_support::rgb_png` and
`cbz_support::grey_jpeg` — the PNG and JPEG specifications transcribed in this
repository — rather than through any imaging library. `source/` holds them and
they are committed too, so what went into each archive is checkable without
rerunning a producer.

## What is committed

183 776 bytes. `sha256` is the first sixteen hex digits, enough to tell a file
from a regeneration of it.

| File | Bytes | Producer | What it demonstrates | sha256 |
| --- | ---: | --- | --- | --- |
| `7z-deflate.cbz` | 19 043 | 7-Zip `-tzip -mx9` | The common case, and the first finding below: **three entries stored and two deflated in one archive** | `bd724ce883078ede` |
| `7z-store.cbz` | 19 159 | 7-Zip `-tzip -mx0` | Every entry stored, which is the route `Archive::read` hands back **borrowed** rather than copying | `8ba93056684bb219` |
| `winrar.cbz` | 19 039 | WinRAR `-afzip` | A second ZIP writer that makes the same per-entry store/deflate choices as 7-Zip and lays its central directory out differently | `1db56f83e8cac68e` |
| `pwsh.cbz` | 18 879 | .NET `System.IO.Compression` | The second finding: **deflates every entry, including the three it makes larger** | `944a422eb61cd0fa` |
| `python.cbz` | 18 879 | CPython 3.12 `zipfile` | A fourth implementation doing the same, so the finding is not one library's quirk | `48bd76da17bc6a61` |
| `7z-lzma2.cb7` | 17 663 | 7-Zip `-t7z -m0=LZMA2` | A CB7 holding the same five pages. Refused by name today | `f211476cb9b199d9` |
| `7z-tar.cbt` | 23 552 | 7-Zip `-ttar` | A CBT holding the same five pages. Refused by name today | `97911001905ea8b5` |
| `winrar-rar5.cbr` | 18 860 | `Rar.exe` (RAR 5) | A CBR holding the same five pages. Refused by name today | `b8f7d4de4b0933a1` |

**The three that are refused are committed anyway, and on purpose.** They hold
the *same five pages* as the five ZIPs. So the day a decoder for one of them
exists, the pictures it produces have something already in the tree to be
compared against — put there by a different program, before the decoder was
written. That is gap 30's structural lesson applied one format further out: the
fixture arrives before the reader, not after it.

## The pages

Five, and every one a different size, because a sequence of sizes is what names
an ordering.

| Page | Size | Bytes | What it is |
| --- | --- | ---: | --- |
| `page1.png` | 60 × 80 | 4 521 | Truecolour PNG, every pixel different from every other |
| `page2.png` | 64 × 88 | 5 184 | The same, and the one entry 7-Zip and WinRAR both chose to deflate |
| `page3.jpg` | 48 × 96 | 169 | A DC-only baseline JPEG — one flat grey, which is what makes it worth having: the JPEG route copies bytes and never builds a raster, and a picture with no detail still proves it ran |
| `page10.png` | 56 × 72 | 4 154 | |
| `page11.png` | 72 × 64 | 4 455 | |

`make-corpus.ps1` packs them in the order `page1, page10, page11, page2,
page3` — **deliberately not** the reading order. An implementation that trusted
the archive's own order, or sorted lexicographically, pages the comic 1, 10,
11, 2, 3, with every page present and nothing anywhere saying so. That is gap
29's own example of the defect natural order exists to prevent, and now it is
measured against archives this repository did not write.

## What these files showed, that the hand-built fixtures never did

Five things. The first two change what a reader must tolerate; the third is a
gap in coverage this corpus **does not** close and the doc says so.

1. **A real archive mixes compression methods per entry.** 7-Zip and WinRAR
   both stored `page1.png`, `page10.png` and `page11.png` and deflated
   `page2.png` and `page3.jpg`, deciding per entry whether deflate was worth
   anything. Every fixture in `cbz_support` uses one method for the whole
   archive, because `ZipFile::stored` and `ZipFile::deflated` are chosen per
   file by a test that is making a point about one of them. Nothing was wrong
   with that; it simply never produced the shape every desktop archiver emits.

2. **Two of the four writers deflate entries to *more* bytes than they started
   with.** `.NET` and CPython both compress unconditionally, and on three of
   the five pages the deflated entry is larger than the original — 4 526 bytes
   for a 4 521-byte PNG. `compressed_size > uncompressed_size` is therefore
   *ordinary*, not a corruption signal, and a reader that treated it as one
   would refuse most comics Windows software produces. This reader does not,
   and now there is a file proving it rather than an argument.

3. **Nobody wrote a data descriptor.** All five ZIPs took the central-directory
   route with general-purpose bit 3 clear on every entry. The streamed-entry
   path — the one APPNOTE 4.3.9 exists for, where the local header holds zeros
   by design and `Archive::route` falls back to the local-header scan — is
   still exercised **only by fixtures this repository wrote**. That is a real
   remaining gap and it is stated here rather than left to be inferred from a
   green run; a producer that streams (a web server zipping on the fly, most
   likely) would be the file that closes it.

4. **`Compress-Archive` refuses a `.cbz` destination**, with
   `NotSupportedArchiveFileExtension`: it writes `.zip` or nothing.
   `make-corpus.ps1` renames afterwards, which changes no bytes — and that is
   the point worth keeping, because this reader decides what a file is from the
   bytes at offset zero and never from its name.

5. **No real archiver tripped a single `ZipWarning`.** All five took
   `Route::CentralDirectory` with an empty warning list. The leniency ladder in
   `tinker-pdf-zip` is built for damaged and unusual archives, and this corpus
   says plainly that it is *not* what ordinary tools produce — so the ladder's
   value rests on the hand-built fixtures, which stay exactly where they are.
   The two corpora answer different questions and neither replaces the other.

## The scripts

`make-corpus.ps1` writes the eight archives and prints a hash for each;
`inventory.ps1` regenerates `INVENTORY.tsv` from them through .NET's reader.
Neither runs in CI — they are how these files were obtained.

```
cargo test -p tinker-pdf --test cbz_real -- --ignored write_the_source_pages
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\make-corpus.ps1
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\inventory.ps1
```

**pwsh 7, not Windows PowerShell 5.1**, for two reasons that both bite:
`ZipArchiveEntry.Crc32` arrived in .NET 7, so 5.1 leaves that column empty
rather than failing, and 5.1 writes a byte-order mark this file must not have.

The corpus is **not** reproducible byte for byte: every ZIP writer stamps each
entry with the source file's modification time, and the four record it to
different precisions. The committed archives are the record; the scripts are
how they were obtained, and the table above carries a hash for each.
`.gitattributes` marks all four extensions binary for gap 30's reason — a
normalised line ending inside a **stored** entry breaks its CRC-32, and there
is no way back.

## How they are checked

`crates/tinker-pdf/tests/cbz_real.rs`:

- `the_inventory_matches_the_archives` recomputes every row of `INVENTORY.tsv`
  through `tinker-pdf-zip` on every `cargo test`, so .NET's reader and this one
  must agree about all 25 entries. The two reach their answers differently:
  .NET exposes no method code and its column is inferred from whether the two
  lengths are equal, where this reader has the central directory's own field.
- `every_real_archive_opens_and_pages_in_natural_order` asserts the page order
  and that every page is its entry's own picture rather than a placeholder.
- `five_zip_writers_produce_the_same_five_pictures` is the relation worth
  having: five implementations that share no code, disagree about what to
  store and what to deflate, and lay their directories out differently, must
  still give this reader the same five pictures at the same five sizes.
  Nothing outside this repository renders any of it.
- `the_containers_this_build_does_not_read_are_refused_by_name` holds the
  `.cb7`, `.cbt` and `.cbr` to `ArchiveRefusal::NotAZip` and to the
  fixed-position sniff, so "this is a CBR and I do not read CBR" stays a
  different sentence from "this is not a PDF".
