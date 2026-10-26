# Comic archives that real archivers wrote

Every hand-built fixture in `tests/cbz_support/mod.rs` is laid out from
APPNOTE 6.3.10's field layouts by this repository. The archives here are the
other half: the device `tests/xps/` uses for XPS and `tests/epub/` for EPUB,
**author the pictures here, pack them with somebody else's tool, commit the
result.** What comes back is a real implementation's idea of a ZIP over
content nobody else owns.

## What produced them

The archives come from several independent implementations, plus one ZIP
this repository writes around another's frames. The Windows rows were made
on the machine described below; the Linux rows say so. Every archive holds
the same five pages but `python-jpx.cbz`, which holds a picture T.800
publishes instead.

| Producer | Files | What it is |
| --- | --- | --- |
| **7-Zip 26.02** (x64) | `7z-*.cbz`, `7z-lzma2.cb7`, `7z-nonsolid.cb7`, `7z-dictreset.cb7`, `7z-tar.cbt` | ip7z/7zip, LGPL-2.1-or-later with an unRAR restriction. Driven from `make-corpus.ps1`. Writes ZIP at two compression levels, 7z in three shapes and tar. |
| **WinRAR 7.20** (x64, trial) | `winrar.cbz`, `winrar-rar5.cbr` | RARLAB, proprietary. `WinRAR.exe a -afzip` writes the ZIP; `Rar.exe` writes the RAR. The trial adds no comment or watermark to an archive — the nag is on the console. |
| **.NET `System.IO.Compression`** | `pwsh.cbz` | Through PowerShell's `Compress-Archive`. Microsoft's ZIP writer, and the one most Windows software reaches for. |
| **CPython 3.12 `zipfile`** | `python.cbz` | The standard library's writer, and the one most tooling that touches comics is written against. |
| **CPython 3.11.15 `zipfile`**, `ZIP_LZMA`, over liblzma 5.4.5 (Ubuntu `5.6.1+really5.4.5-1ubuntu0.2`) | `python-lzma.cbz` | The same writer asked for APPNOTE method 14 — the one writer on hand that emits it — by `make-lzma.py`, on Linux x86_64. See *The LZMA ZIP* below. |
| **CPython 3.11.15 `zipfile`**, `ZIP_STORED` | `python-jpx.cbz` | T.800 Annex J.10's codestream as two JPEG 2000 pages, by `make-jpx.py`, on Linux x86_64. See *The JPEG 2000 ZIP* below. |
| **CPython 3.11.15 `zipfile`**, `ZIP_BZIP2`, over libbzip2 1.0.8 (Ubuntu `1.0.8-5.1build0.1`) | `python-bzip2.cbz` | The same writer asked for APPNOTE method 12, by `make-bzip2.py`, on Linux x86_64. |
| **7-Zip 26.02** (x64) for Linux, `7zz` | `7zz-bcj2.cb7` | The same program as the Windows row, its Linux build from the ip7z/7zip release (`7z2602-linux-x64.tar.xz`, SHA-256 `41aaba7b…c28c03e`), by `make-bcj2.sh`, on Linux x86_64: the one writer of BCJ2. |
| **python-zstandard 0.25.0** (libzstd 1.5.7) on CPython 3.11.15, in a ZIP `make-zstd.py` writes | `python-zstd.cbz` | APPNOTE method 93, which no ZIP writer on hand makes (CPython's `zipfile` writes it only from 3.14): libzstd's frames — the reference encoder, level 3 with content checksums — inside local headers and a central directory `make-zstd.py` writes from APPNOTE 4.3, on Linux x86_64. BSD-3-Clause. |
| **py7zr 1.1.3** on CPython 3.11.15, over liblzma 5.4.5, libbzip2 1.0.8 and pyppmd 1.3.1 | `py7zr-bcj.cb7`, `py7zr-bzip2.cb7`, `py7zr-ppmd.cb7` | The second 7z writer, a separate implementation of the container, by `make-py7zr.py`, on Linux x86_64. LGPL-2.1-or-later. See *The py7zr `.cb7`s* below. |

**RAR 4 cannot be produced with these tools.** RAR 7.20's `rar.exe` has no
`-ma` switch at all: `-ma4` answers `ERROR: Unknown option: ma4` and its help
lists no format-version switch. This release writes RAR 5 and nothing else,
so there is no RAR 4 fixture here, and RAR 4 is a hard limit
([NG-01](../../../../docs/roadmap/NG.md)).

Machine: Windows 11 Pro, x64.

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

`sha256` is the first sixteen hex digits, enough to tell a file from a
regeneration of it.

| File | Bytes | Producer | What it demonstrates | sha256 |
| --- | ---: | --- | --- | --- |
| `7z-deflate.cbz` | 19 043 | 7-Zip `-tzip -mx9` | The common case: **three entries stored and two deflated in one archive** | `bd724ce883078ede` |
| `7z-store.cbz` | 19 159 | 7-Zip `-tzip -mx0` | Every entry stored, which is the route `Archive::read` hands back **borrowed** rather than copying | `8ba93056684bb219` |
| `winrar.cbz` | 19 039 | WinRAR `-afzip` | A second ZIP writer that makes the same per-entry store/deflate choices as 7-Zip and lays its central directory out differently | `1db56f83e8cac68e` |
| `pwsh.cbz` | 18 879 | .NET `System.IO.Compression` | **Deflates every entry, including the three it makes larger** | `944a422eb61cd0fa` |
| `python.cbz` | 18 879 | CPython 3.12 `zipfile` | A fourth implementation doing the same, so the behaviour is not one library's quirk | `48bd76da17bc6a61` |
| `python-jpx.cbz` | 495 | CPython 3.11 `zipfile`, stored | **Two JPEG 2000 pages**, T.800 J.10's 100-byte codestream bare (`page1.j2k`) and in Annex I's JP2 boxes (`page2.jp2`): the entries the comic path places as `/JPXDecode` | `322249c87ea1a56b` |
| `python-lzma.cbz` | 18 939 | CPython 3.11 `zipfile`, `ZIP_LZMA` | **Every entry ZIP method 14**, with APPNOTE 5.8.8's header and an end-of-stream marker (general-purpose bit 1) | `22543533a5094f1b` |
| `python-bzip2.cbz` | 20 742 | CPython 3.11 `zipfile`, `ZIP_BZIP2` | **Every entry ZIP method 12**, a whole bzip2 stream per entry with no framing round it | `3aecc1a5538058d6` |
| `python-zstd.cbz` | 18 944 | libzstd's frames, in a ZIP `make-zstd.py` writes | **Every entry ZIP method 93**, one Zstandard frame per entry with its content checksum and no framing round it | `3f0e2edee0f60d50` |
| `7z-lzma2.cb7` | 17 663 | 7-Zip `-t7z -m0=LZMA2` | A CB7 holding the same five pages, in one solid LZMA2 block under an **LZMA-compressed header** | `f211476cb9b199d9` |
| `7z-nonsolid.cb7` | 18 608 | 7-Zip `-t7z -m0=LZMA2 -ms=off` | The same five pages in **five folders**, one per page: the folder walk runs past folder 0 and sets a coder up five times | `6a10817c1df4905f` |
| `7z-dictreset.cb7` | 18 073 | 7-Zip `-t7z -m0=LZMA2:d8k:c8k` | The same five pages in one folder of **three LZMA2 chunks**, each opening with a dictionary reset — two of them mid-stream | `7e9caaa4ff5fc706` |
| `py7zr-bcj.cb7` | 17 643 | py7zr, `[FILTER_X86, FILTER_LZMA2]` | The same five pages from **a second 7z writer**, BCJ (`03030103`) fed by LZMA2 in one solid folder, the two coders listed in the opposite order from 7-Zip's | `e4042a54e44e9a66` |
| `py7zr-bzip2.cb7` | 18 443 | py7zr, `[FILTER_BZIP2]` | The same five pages as 7z coder `040202`, one bzip2 block in one solid folder | `40fc68e031f52f68` |
| `py7zr-ppmd.cb7` | 17 776 | py7zr, `[FILTER_PPMD]` order 6, 16 MiB | The same five pages as 7z coder `030401`, PPMd var.H through pyppmd — 7-Zip's own encoder | `e2447d6ec2f19f0a` |
| `7zz-bcj2.cb7` | 17 677 | 7-Zip (Linux) `-m0=BCJ2 -m1=LZMA:d20 -m2=LZMA:d20 -m3=LZMA:d20 -mb0:1 -mb0s1:2 -mb0s2:3` | The same five pages as a **BCJ2 folder**: four coders and four pack streams meeting in one output, BCJ2 listed last | `8acbf5deac63ace7` |
| `7z-tar.cbt` | 23 552 | 7-Zip `-ttar` | A CBT holding the same five pages, in GNU's tar dialect | `97911001905ea8b5` |
| `winrar-rar5.cbr` | 18 860 | `Rar.exe` (RAR 5) | A CBR holding the same five pages: **four stored, one compressed with method 3**, plus a `QO` service record. The compressed entry is a placeholder page | `b8f7d4de4b0933a1` |

The non-ZIPs hold the *same five pages* as the ZIPs, so the pictures a decoder
of them produces have something in the tree to be compared against, put there
by a different program. The `.cbt` and the `.cb7`s join the ZIPs in
`cbz_real.rs`'s cross-producer identity, with no oracle anywhere, and each
`.cb7`'s own recorded CRC-32 is what adjudicates a hand-rolled LZMA decoder.

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
11, 2, 3, with every page present and nothing anywhere saying so. That is the
defect natural order exists to prevent.

## What these files show, that the hand-built fixtures do not

1. **A real archive mixes compression methods per entry.** 7-Zip and WinRAR
   both store `page1.png`, `page10.png` and `page11.png` and deflate
   `page2.png` and `page3.jpg`, deciding per entry whether deflate is worth
   anything. Every fixture in `cbz_support` uses one method for the whole
   archive, because `ZipFile::stored` and `ZipFile::deflated` are chosen per
   file by a test that is making a point about one of them, so none of them
   has the shape every desktop archiver emits.

2. **Two of the writers deflate entries to *more* bytes than they started
   with.** `.NET` and CPython both compress unconditionally, and on three of
   the five pages the deflated entry is larger than the original — 4 526 bytes
   for a 4 521-byte PNG. `compressed_size > uncompressed_size` is therefore
   *ordinary*, not a corruption signal, and a reader that treated it as one
   would refuse most comics Windows software produces. This reader does not,
   and `pwsh.cbz` and `python.cbz` hold it to that.

3. **None of the five page ZIPs has a data descriptor.** All five take the
   central-directory route with general-purpose bit 3 clear on every entry.
   The streamed-entry path — the one APPNOTE 4.3.9 exists for, where the local
   header holds zeros by design and `Archive::route` falls back to the
   local-header scan — is exercised **only by fixtures this repository
   wrote**; a producer that streams (a web server zipping on the fly, most
   likely) would be the file that closes it.

4. **`Compress-Archive` refuses a `.cbz` destination**, with
   `NotSupportedArchiveFileExtension`: it writes `.zip` or nothing.
   `make-corpus.ps1` renames afterwards, which changes no bytes — and that is
   the point worth keeping, because this reader decides what a file is from the
   bytes at offset zero and never from its name.

5. **A real RAR mixes stored and compressed entries too.** `winrar-rar5.cbr`
   holds four stored PNGs, **`page3.jpg` compressed with method 3**, and a
   `QO` quick-open index service record, which is listed and is not a page.
   The 169-byte JPEG *does* get smaller, because a JPEG of a flat synthetic
   test image is not the incompressible thing a photograph is. RAR's
   compression is a hard limit ([NG-01](../../../../docs/roadmap/NG.md)), so a
   `.cbr` is four fifths of a comic here: the compressed entry is a
   placeholder named by its method, with its own recorded CRC-32 and the same
   picture in the ZIPs beside it.

6. **No real archiver trips a single `ZipWarning`.** All five page ZIPs take
   `Route::CentralDirectory` with an empty warning list. The leniency ladder in
   `tinker-pdf-zip` is built for damaged and unusual archives, and this corpus
   says plainly that it is *not* what ordinary tools produce — so the ladder's
   value rests on the hand-built fixtures, which stay exactly where they are.
   The two corpora answer different questions and neither replaces the other.

## The 7-Zip `.cb7`s

`7z-lzma2.cb7` is what a desktop archiver writes by default, and the default
is the **simplest** thing the format allows: one folder, holding one LZMA2
chunk. Two more archives from the same producer reach the loops it does not,
`decode_folder`'s walk over folders and `decode_lzma2`'s loop over chunks:

- **`7z-nonsolid.cb7`** (`-ms=off`) is five folders, one per page. The walk runs
  past folder 0, a coder is set up five times over five different pack offsets,
  and `Archive::read`'s folder cache is asked for a folder it does not hold.
- **`7z-dictreset.cb7`** (`-m0=LZMA2:d8k:c8k`) is one folder of three LZMA2
  chunks, each opening with a dictionary reset — two of them mid-stream, at
  output offsets 8 192 and 16 384, which fall *inside* `page10.png` and
  *inside* `page2.png` rather than on any page boundary.

**The flags are checked against what comes out.**
`the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`, in
`tinker-pdf-archive`'s `sevenz/tests.rs`, opens both files with this
repository's own reader and asserts the folder count and the chunk count, so a
regeneration that quietly lost either shape fails a test rather than leaving a
name to do the arguing. A dictionary size does not split a solid block; the
LZMA2 *block* size (`c`) does, and `d8k` is set beside it only so the
dictionary cannot outlive the block it belongs to.

All three are one implementation asked for three shapes, which buys coverage
of this decoder's loops and nothing against a shared misreading of the
format. The second writer is py7zr, below, and BCJ2's one writer is 7-Zip's
Linux build (`7zz-bcj2.cb7`).

## The py7zr `.cb7`s

py7zr is a pure-Python 7z writer — its own header writer and folder layout,
sharing no code with 7-Zip's container — that hands the coding to libraries:
liblzma for LZMA, LZMA2 and the x86 filter. So a `.cb7` from it is a second
writer's container over a second implementation of BCJ (xz's rather than
7-Zip's). Made on Linux x86_64 with py7zr 1.1.3 on CPython 3.11.15 over
liblzma 5.4.5:

```
pip install --user py7zr
cd crates/tinker-pdf/tests/cbz && python3 make-py7zr.py
```

py7zr stamps entries with the current time; `make-py7zr.py` pins its clock to
one instant, so a rerun writes the same bytes — `py7zr-bcj.cb7` is SHA-256
`e4042a54e44e9a66d7ea7507082deb1585969df633fe1e4df4edb7c07dadd174`. It packs
in `make-corpus.ps1`'s order.

**It differs from 7-Zip where it matters**, which is what a second writer is
for: py7zr lists a BCJ folder's coders as LZMA2 then BCJ, bound
`(in 1, out 0)`, where 7-Zip lists the filter first. A reader that walked
coders in list order would run the filter before the decompressor and hand
back noise; the bind pairs are what say the order, and
`tinker-pdf-archive`'s `the_bcj_fixture_is_a_filter_chain_that_rewrote_operands`
pins py7zr's layout.

Five pictures carry almost nothing for BCJ to convert — one `E8` in these five
pages has an operand whose top byte is `00` or `FF` — so this archive is the
second writer's *container*, and the filter itself is held elsewhere:
`tinker-pdf-archive/tests/coders/py7zr-bcj.7z` is the same writer over
`x86.bin`, a file shaped for the filter, and `tests/coders.rs` holds it to that
file byte for byte.

## The LZMA ZIP

`source/` holds the five files that went into `python-lzma.cbz`, and LZMA is
lossless. So `cbz_real.rs`'s
`a_real_archiver_s_lzma_entries_are_the_files_that_went_in` holds every decoded
entry to its source file byte for byte, which is the generator's *input* as the
expected answer rather than any decoder's output, and the archive's own CRC-32
stands behind that inside `tinker-pdf-zip`.

Made on Linux x86_64 with CPython 3.11.15 over liblzma 5.4.5:

```
cd crates/tinker-pdf/tests/cbz && python3 make-lzma.py
```

`make-lzma.py` gives every entry one fixed timestamp, so unlike the Windows
archives a rerun under the same two versions writes the **same bytes** —
SHA-256 `22543533a5094f1beea83fbfe354ec03783dc08ec3e3d67a6b8854868a8768b5`.
It packs in `make-corpus.ps1`'s order, not reading order.

**It is deliberately not in `INVENTORY.tsv`.** That table is .NET's reading of
the ZIPs, and .NET exposes no method code: `inventory.ps1` infers `stored` or
`deflate` from whether two lengths are equal, and would write `deflate` for all
five method-14 entries — a row this reader would then correctly disagree with,
for a reason that is .NET's and not the archive's. `inventory.ps1` skips the
file by name and says why, so a regeneration does not quietly put that claim
in the table; the entries' CRC-32 is what adjudicates the decoder instead.

## The JPEG 2000 ZIP

`python-jpx.cbz` does not hold the five pages, and cannot usefully: nothing
here writes a JPEG 2000 file, and a page this repository encoded would test
its decoder against its own encoder. What it holds instead is the one JPEG 2000
file whose decoded picture a standard publishes — T.800 Annex J.10's 100-byte
codestream, whose samples J.10.5 prints as 101, 103, 104, 105, 96, 97, 96, 102
and 109 — twice: bare, as `page1.j2k`, and inside T.800 Annex I's JP2 boxes
(signature, `ftyp`, a `jp2h` of `ihdr` and a greyscale `colr`, and `jp2c`), as
`page2.jp2`. The boxes are this repository's, written from Annex I; the
codestream is the standard's, read by `make-jpx.py` out of
`tinker-pdf-filters/tests/jpx_annex_j.rs` rather than transcribed a second
time. Both entries are stored.

```
cd crates/tinker-pdf/tests/cbz && python3 make-jpx.py
```

One fixed timestamp again, so a rerun writes the same bytes — SHA-256
`322249c87ea1a56baf6e1c0423906ca20a9c84e65346417e314510240275784e`.
`cbz_real.rs`'s
`a_jpeg_2000_page_is_placed_as_jpxdecode_and_draws_the_samples_t800_publishes`
holds both pages to J.10.5's samples after a render, and
`a_jpeg_2000_entry_whose_header_is_refused_is_a_placeholder_naming_it` damages
the JP2's codestream in two places and asserts a named placeholder. It is not
in `INVENTORY.tsv` either: that table describes the archives of the five pages.

## The scripts

`make-corpus.ps1` writes the Windows archives and prints a hash for each;
`inventory.ps1` regenerates `INVENTORY.tsv` from the five ZIPs through .NET's
reader; `make-lzma.py`, `make-jpx.py`, `make-bzip2.py` and `make-zstd.py`
write one ZIP each, `make-py7zr.py` the py7zr `.cb7`s, and `make-bcj2.sh` the
BCJ2 one. None runs in CI — they are how these files are made.

```
cargo test -p tinker-pdf --test cbz_real -- --ignored write_the_source_pages
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\make-corpus.ps1
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\inventory.ps1
```

`make-corpus.ps1 -Only <names>` writes a subset, and it is not a convenience:
a whole run stamps every archive with the source files' current modification
times, so regenerating the directory to add one fixture would change the bytes
and the hash of every file already in it. For example:

```
pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\make-corpus.ps1 -Only 7z-nonsolid.cb7,7z-dictreset.cb7
```

**pwsh 7, not Windows PowerShell 5.1**, for two reasons that both bite:
`ZipArchiveEntry.Crc32` exists only from .NET 7, so 5.1 leaves that column
empty rather than failing, and 5.1 writes a byte-order mark this file must
not have.

The archives `make-corpus.ps1` writes are **not** reproducible byte for byte:
every ZIP writer stamps each entry with the source file's modification time,
and the four record it to different precisions. The committed archives are
the record; the scripts are how they are made, and the table above carries a
hash for each. `.gitattributes` marks all four extensions binary, because a
normalised line ending inside a **stored** entry breaks its CRC-32, and there
is no way back.

## How they are checked

`crates/tinker-pdf/tests/cbz_real.rs`:

- `the_inventory_matches_the_archives` recomputes every row of `INVENTORY.tsv`
  through `tinker-pdf-zip` on every `cargo test`, so .NET's reader and this one
  must agree about every entry. The two reach their answers differently:
  .NET exposes no method code and its column is inferred from whether the two
  lengths are equal, where this reader has the central directory's own field.
- `every_real_archive_opens_and_pages_in_natural_order` asserts the page order
  and that every page is its entry's own picture rather than a placeholder.
- `five_zip_writers_produce_the_same_five_pictures` is the relation worth
  having: five implementations that share no code, disagree about what to
  store and what to deflate, and lay their directories out differently, must
  still give this reader the same five pictures at the same five sizes.
  Nothing outside this repository renders any of it. It runs over the five
  ZIPs and every archive `READ_CONTAINERS` names — the `.cbt`, the `.cb7`s,
  `python-lzma.cbz`, `python-bzip2.cbz` and `python-zstd.cbz` — rather than
  giving each a check of its own, because what is worth asserting about a
  `.cbt` or a `.cb7` is not that it opens, it is that it opens as *the same
  pictures* a `.cbz` of the same pages does.
- `the_tar_a_real_archiver_wrote_pages_in_natural_order` and
  `the_7z_a_real_archiver_wrote_pages_in_natural_order` add the order
  assertion for the two containers with no central directory to re-order
  things behind; the `.cb7` one runs over every `.cb7`.
- `the_rar_a_real_archiver_wrote_pages_what_it_stored_and_names_what_it_did_not`
  holds `winrar-rar5.cbr` to four pictures and one placeholder **named by
  method number**, which keeps the limit visible.

`crates/tinker-pdf-archive/src/sevenz/tests.rs`:

- `the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`
  asserts the folder count and the LZMA2 chunk count of the three 7-Zip
  `.cb7`s against the committed bytes, so `7z-nonsolid.cb7` cannot stop being
  multi-folder and `7z-dictreset.cb7` cannot stop having dictionary resets
  without a test going red.
