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

Ten archives, five of them ZIPs, from four independent implementations on the
machine described below.

| Producer | Files | What it is |
| --- | --- | --- |
| **7-Zip 26.02** (x64) | `7z-*.cbz`, `7z-lzma2.cb7`, `7z-nonsolid.cb7`, `7z-dictreset.cb7`, `7z-tar.cbt` | ip7z/7zip, LGPL-2.1-or-later with an unRAR restriction. Driven from `make-corpus.ps1`. Writes ZIP at two compression levels, 7z in three shapes and tar. |
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

191 755 bytes, which is the sum of the Bytes column below and nothing else —
the pages in `source/` are another 18 483 and the four text files here are not
counted at all. (The figure that stood here before the two later `.cb7`s landed
was 183 776, which was the sum of nothing: the eight archives it described came
to 155 074. Re-measured rather than carried.) `sha256` is the first sixteen hex
digits, enough to tell a file from a regeneration of it.

| File | Bytes | Producer | What it demonstrates | sha256 |
| --- | ---: | --- | --- | --- |
| `7z-deflate.cbz` | 19 043 | 7-Zip `-tzip -mx9` | The common case, and the first finding below: **three entries stored and two deflated in one archive** | `bd724ce883078ede` |
| `7z-store.cbz` | 19 159 | 7-Zip `-tzip -mx0` | Every entry stored, which is the route `Archive::read` hands back **borrowed** rather than copying | `8ba93056684bb219` |
| `winrar.cbz` | 19 039 | WinRAR `-afzip` | A second ZIP writer that makes the same per-entry store/deflate choices as 7-Zip and lays its central directory out differently | `1db56f83e8cac68e` |
| `pwsh.cbz` | 18 879 | .NET `System.IO.Compression` | The second finding: **deflates every entry, including the three it makes larger** | `944a422eb61cd0fa` |
| `python.cbz` | 18 879 | CPython 3.12 `zipfile` | A fourth implementation doing the same, so the finding is not one library's quirk | `48bd76da17bc6a61` |
| `7z-lzma2.cb7` | 17 663 | 7-Zip `-t7z -m0=LZMA2` | A CB7 holding the same five pages, in one solid LZMA2 block under an **LZMA-compressed header**. Read since tier 4 | `f211476cb9b199d9` |
| `7z-nonsolid.cb7` | 18 608 | 7-Zip `-t7z -m0=LZMA2 -ms=off` | The same five pages in **five folders**, one per page: the folder walk runs past folder 0 and sets a coder up five times | `6a10817c1df4905f` |
| `7z-dictreset.cb7` | 18 073 | 7-Zip `-t7z -m0=LZMA2:d8k:c8k` | The same five pages in one folder of **three LZMA2 chunks**, each opening with a dictionary reset — two of them mid-stream | `7e9caaa4ff5fc706` |
| `7z-tar.cbt` | 23 552 | 7-Zip `-ttar` | A CBT holding the same five pages, in GNU's tar dialect. Read since tier 4 | `97911001905ea8b5` |
| `winrar-rar5.cbr` | 18 860 | `Rar.exe` (RAR 5) | A CBR holding the same five pages, and the fifth finding below: **four stored, one compressed with method 3**, plus a `QO` service record. Container read since tier 4; the compressed entry is a placeholder page | `b8f7d4de4b0933a1` |

**The first three non-ZIPs were committed while all three were still refused,
and on purpose.** They hold the *same five pages* as the five ZIPs, so the day a
decoder for one of them existed, the pictures it produced had something already
in the tree to be compared against — put there by a different program, before
the decoder was written. That is gap 30's structural lesson applied one format
further out: the fixture arrives before the reader, not after it.

It paid off exactly as intended. Tier 4's `tinker-pdf-archive` read all three
containers against these files and nothing else, with no oracle anywhere: the
`.cbt` and `7z-lzma2.cb7` now join the five ZIPs in
`cbz_real.rs`'s cross-producer identity, and the `.cb7`'s own recorded CRC-32
is what adjudicates a hand-rolled LZMA decoder. The `.cbr` is the one that has
not closed, for the reason in finding 5 below.

**`7z-nonsolid.cb7` and `7z-dictreset.cb7` arrived later and for the opposite
reason: the decoder was already here and the fixture that adjudicated it only
ever asked it one question.** See *Three `.cb7`s, and the one thing this
directory still cannot buy* below.

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

Six things. The first two change what a reader must tolerate; the third is a
gap in coverage this corpus **does not** close and the doc says so. (There were
six all along; two of them were both numbered 5.)

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

5. **A real RAR mixes stored and compressed entries too, and the reasoning that
   said it would not was wrong.** The plan for tier 4's RAR lane assumed
   `winrar-rar5.cbr` would store every entry, on the argument that WinRAR
   compresses only what gets smaller and an image never does. Reading the file
   rather than reasoning about it found four stored PNGs, **`page3.jpg`
   compressed with method 3**, and a `QO` quick-open index service record — a
   record type nothing in this corpus had before, which is listed and is not a
   page. The 169-byte JPEG *did* get smaller, because a JPEG of a flat
   synthetic test image is not the incompressible thing a photograph is.

   Both halves matter. It means a `.cbr` is four fifths of a comic until RAR 5's
   compression is written, so the CBR row does not close. And it means that
   decompressor **has a first-party fixture here after all** — 169 bytes with
   its own recorded CRC-32, and the same picture in five ZIPs beside it — where
   the plan had it blocked on a producer this machine cannot run. See
   `docs/design/comic-archives.md`, milestone 5.

6. **No real archiver tripped a single `ZipWarning`.** All five took
   `Route::CentralDirectory` with an empty warning list. The leniency ladder in
   `tinker-pdf-zip` is built for damaged and unusual archives, and this corpus
   says plainly that it is *not* what ordinary tools produce — so the ladder's
   value rests on the hand-built fixtures, which stay exactly where they are.
   The two corpora answer different questions and neither replaces the other.

## Three `.cb7`s, and the one thing this directory still cannot buy

`7z-lzma2.cb7` is the file that adjudicates a hand-rolled LZMA decoder, and for
a while it was the only one. What it is, though, is what a desktop archiver
writes by default — and the default is the **simplest** thing the format allows:
one folder, holding one LZMA2 chunk. So `decode_folder`'s walk over folders and
`decode_lzma2`'s loop over chunks were each entered exactly once by every
archive in this directory, and the second iteration of either was reached by
nothing at all. `docs/design/comic-archives.md` had named that as the residual
risk in as many words.

Two more archives from the same producer close two thirds of it:

- **`7z-nonsolid.cb7`** (`-ms=off`) is five folders, one per page. The walk runs
  past folder 0, a coder is set up five times over five different pack offsets,
  and `Archive::read`'s folder cache is asked for a folder it does not hold.
- **`7z-dictreset.cb7`** (`-m0=LZMA2:d8k:c8k`) is one folder of three LZMA2
  chunks, each opening with a dictionary reset — two of them mid-stream, at
  output offsets 8 192 and 16 384, which fall *inside* `page10.png` and
  *inside* `page2.png` rather than on any page boundary.

**The flags were checked against what came out.**
`the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`, in
`tinker-pdf-archive`'s `sevenz/tests.rs`, opens both files with this
repository's own reader and asserts the folder count and the chunk count, so a
regeneration that quietly lost either shape fails a test rather than leaving a
name to do the arguing. That check earned itself immediately: the obvious flag,
`-m0=LZMA2:d64k`, measured **one** chunk over these five pages. A dictionary
size does not split a solid block; the LZMA2 *block* size (`c`) does, and `d8k`
is set beside it only so the dictionary cannot outlive the block it belongs to.

### What is still missing, and why it is not here

**A second real archiver's 7z.** The row that asked for these two fixtures asked
for a *second archiver*, and this machine cannot supply one. 7-Zip 26.02 is the
same program that wrote `7z-lzma2.cb7`, so all three `.cb7`s are one
implementation asked for three shapes — which buys coverage of this decoder's
loops and buys **nothing** against a shared misreading of the format, because
there is only one writer to misread it. `py7zr` is not installed here and no
other 7z writer is. This is the same shape as the RAR 4 note above: recorded,
with the reason, rather than quietly redefined into something that was
achievable. A `.cb7` from any second writer would close it, and the exit
criterion is already written — the file joins `READ_CONTAINERS` and
`five_zip_writers_produce_the_same_five_pictures` passes over it.

**A BCJ filter chain**, which the same row also named, is deliberately absent
and is not the same kind of gap. `-mf=BCJ` writes coder id `03030103`, which is
outside the allow-list in `sevenz.rs` and would be **refused at open** — so a
BCJ fixture would not test the decoder that exists, it would sit in the tree
waiting for the capability that does not. That belongs with the work that adds
it, in tier 4's archive row, where the same argument already parks PPMd and
bzip2. BCJ2 stays refused by `Error::NotAChain` by design: it takes four input
streams and is not a chain.

## The scripts

`make-corpus.ps1` writes the ten archives and prints a hash for each;
`inventory.ps1` regenerates `INVENTORY.tsv` from the five ZIPs through .NET's
reader. Neither runs in CI — they are how these files were obtained.

```
cargo test -p tinker-pdf --test cbz_real -- --ignored write_the_source_pages
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\make-corpus.ps1
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\inventory.ps1
```

`make-corpus.ps1 -Only <names>` writes a subset, and it is not a convenience:
a whole run stamps every archive with the source files' current modification
times, so regenerating the directory to add one fixture would change the bytes
and the hash of every file already in it. The two later `.cb7`s were obtained
with

```
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\make-corpus.ps1 -Only 7z-nonsolid.cb7,7z-dictreset.cb7
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
  Nothing outside this repository renders any of it. It runs over **nine**
  archives now rather than five — the four non-ZIPs this build reads join it
  rather than getting a check of their own, because what is worth asserting
  about a `.cbt` or a `.cb7` is not that it opens, it is that it opens as *the
  same pictures* a `.cbz` of the same pages does.
- `the_tar_a_real_archiver_wrote_pages_in_natural_order` and
  `the_7z_a_real_archiver_wrote_pages_in_natural_order` add the order
  assertion for the two containers with no central directory to re-order
  things behind; the `.cb7` one runs over all three `.cb7`s.
- `the_rar_a_real_archiver_wrote_pages_what_it_stored_and_names_what_it_did_not`
  is the one that holds `winrar-rar5.cbr` to four pictures and one placeholder
  **named by method number**, which is what stops the CBR row closing quietly.

`crates/tinker-pdf-archive/src/sevenz/tests.rs`:

- `the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`
  asserts the folder count and the LZMA2 chunk count of the three `.cb7`s
  against the committed bytes, so `7z-nonsolid.cb7` cannot stop being
  multi-folder and `7z-dictreset.cb7` cannot stop having dictionary resets
  without a test going red.
