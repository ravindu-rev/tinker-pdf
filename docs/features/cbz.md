# CBZ

A `.cbz` — a ZIP of page images — opens as an ordinary `Document`. There is no
comic-shaped branch below the facade: at open the archive is synthesised into a
real PDF, serialised by this repository's own writer and parsed straight back
by the same parser every real PDF goes through, so `Document::cos()` still
returns a borrow and every existing capability — cancellation, typed warnings,
`RenderOptions::at_dpi`, bounded painting — arrives for nothing rather than
being wired a second time. The archive itself is read by `tinker-pdf-zip`, a
leaf crate that knows APPNOTE 6.3.10 and nothing else (ruling 8,
[rulings](../rulings.md)).

## What it does

**The sniff.** `PK\x03\x04` is tested at offset zero and nowhere else, so a
PDF that happens to carry those four bytes inside a stream — an attachment, a
compressed object, a font program — stays an ordinary PDF. RAR
(`Rar!\x1A\x07`), 7z and tar (`ustar` at offset 257, where POSIX 1003.1 puts
it) are recognised at fixed positions too. **A `.cbt` is read**; a `.cbr` and a
`.cb7` are still refused by name, and by name matters: "this is a CBR and I do
not read CBR" is a different sentence from "this is not a PDF". A ZIP
is one signature over several formats, so the archive is opened **once** and
asked what it is: ECMA-388 E.3's three-step test routes an XPS package first
([xps](xps.md)), OCF's `META-INF/container.xml` routes an EPUB second
([epub](epub.md)), and the comic path is the fallthrough — which is what E.3's
own text asks for. See [opening](opening.md) for the routing as a whole.

**The archive reader.** `tinker-pdf-zip` reads the central directory, and when
there is no end-of-central-directory record — or its offsets do not land on
`PK\x01\x02` — it recovers the entry list by scanning for local file headers,
the posture the COS layer already takes for a damaged cross-reference table.
`Archive::route` says which rung was used. On disagreement the central
directory wins: an entry written with general-purpose bit 3 set has zeros in
its local header *by design* (APPNOTE 4.3.9), so following the local header
would refuse every streamed entry in the world. Deflated entries are raw
DEFLATE by definition (APPNOTE 4.4.5, RFC 1951). Names decode as UTF-8 under
bit 11 and as CP437 otherwise (APPNOTE D.1). **Every entry is CRC-32 checked
before its bytes are returned, and an entry that cannot be checked is
refused** — the design copies image bytes into the PDF untouched, so the
archive's checksum is the only integrity evidence those bytes will ever have.
Everything is bounded: entries per archive, bytes per entry, an inflation
total per archive that is charged on what an entry was *permitted* to produce
and never refunded, and name length. Everything tolerated is recorded as a
typed `ZipWarning`, at most once per archive (ruling 10).

**A `.cbt` is a tar of page images**, read by `tinker-pdf-archive` — a second
leaf beside `tinker-pdf-zip` rather than three modules inside it, because that
crate's `Archive::read` hands a stored entry back *borrowed* and a 7z solid
block cannot ([architecture](../architecture.md),
[design/comic-archives.md](../design/comic-archives.md)). Everything above this
paragraph and below it is the same for a `.cbt` as for a `.cbz`: the same
natural order, the same magic-byte classification, the same pass-through, the
same placeholder pages, the same `ComicInfo.xml`. The tar reader takes
POSIX 1003.1 `ustar`, GNU's `L` long-name pseudo-entry and PAX's `x`/`g`
extended headers; the original v7 format is refused because it carries no magic
and a reader with no signature to check accepts anything. Sparse files and
multi-volume continuations are listed, so the page count stays honest, and
refused at read. **Every tar entry is a contiguous byte range of the input**,
so its `read` returns a plain borrow rather than a `Cow` — the strongest form
of the no-copy property the whole design rests on.

**Pass-through is the design.** A non-interlaced PNG of colour type 0, 2 or 3
passes through verbatim: its IDAT *is* a `/FlateDecode` stream with
`/DecodeParms << /Predictor 15 … >>`, byte for byte, because PDF's predictor
is PNG 9.2's row-filter specification adopted wholesale — so the common case
copies bytes, reads a thirteen-byte header and never builds a raster. A `tRNS`
naming one fully transparent colour, or one contiguous run of palette
indices, becomes 8.9.6.4's colour-key `/Mask` and still passes through. A
JPEG is placed exactly as the archive holds it. Adam7 interlace (PNG 7.2),
grey+alpha and RGBA (colour types 4 and 6) and partial-alpha `tRNS` cannot be
expressed that way, so they take the decoder and are split into samples and an
`/SMask`. The consequence is the cost model: a 200-page archive costs a
multiple of its own size to open, not *w × h × 3* per page — about 3.6 GB for
200 pages at 2000 × 3000, had every page been decoded.

**And a TIFF is four more of the same argument.** Four of TIFF 6.0's codings
already have a `/Filter` name, so a single-strip file of any of them is placed
rather than decoded: compressions 2, 3 and 4 become `/CCITTFaxDecode` with
Table 11 filled in from the directory's own `T4Options` and geometry, 5 becomes
`/LZWDecode` (§13's LZW is 7.4.4's, `/EarlyChange 1` included), 7 becomes
`/DCTDecode` with `JPEGTables` spliced in front, and 8 and 32946 become
`/FlateDecode`, with `/Predictor 2` where the file used one — which is the same
Table 10 predictor PDF calls "TIFF horizontal differencing" because that is
where it came from. A scanned comic is a G4 fax, and a G4 fax reaches the page
as its own bytes.

What sends a TIFF to the decoder is a shorter list than it looks:
`PhotometricInterpretation` 0 outside the fax codings (it needs `/Decode [1 0]`,
which the writer's compressed-image path does not carry), `PlanarConfiguration`
2, `FillOrder` 2, `ExtraSamples` (PDF wants a separate `/SMask`), tiles (an
edge tile is stored padded out to the tile size), 16-bit samples in an `II`
file (8.9.5.2's are big-endian), PackBits (7.4.5 reads the byte 128 as
end-of-data where TIFF 6.0 §9 reads it as a no-op, so a PackBits strip is not a
`/RunLengthDecode` stream), LZW in the pre-1993 bit order, and **more than one
strip**. That last one is decided per filter rather than waved at, and all four
filters say no for four different reasons: a second CCITT strip would be coded
against the first's last row, a second LZW strip is past an EndOfInformation
code, a second zlib header would be read as compressed data, and two JPEG
datastreams end to end are not a JPEG. It costs less than it sounds, because
`RowsPerStrip` defaults to 2^32-1 (TIFF 6.0 p.39) and a writer that does not
set it has written exactly one strip.

One thing the TIFF pass-through gives up is written down rather than absorbed.
A PNG carries a CRC-32 on every chunk, so the pass-through can check the bytes
it copies; **a TIFF carries no checksum at all**, so `complete()` on the placed
route is a claim about the file's structure and not about its bytes. The
archive's own CRC-32 over the whole entry is what stands behind them, which is
the same guarantee a placed JPEG has.

**Order and geometry.** Pages come in natural order over the full stored path
— `page2` before `page10` — in byte arithmetic with no locale anywhere, so the
order is the same on every target (ruling 4). Lexicographic order fails
invisibly: `1.jpg` through `12.jpg` reads 1, 10, 11, 12, 2 … with every page
present and the comic unreadable. Each page is the image's own pixel size, and
the image fills it exactly — one image pixel is one PDF point, because
8.9.5.2's unit square is scaled by the page's own dimensions.

**A broken entry keeps its page number.** An entry that is recognisably an
image and cannot become a real page — an unsupported format, a refused entry,
an undecodable file — becomes a placeholder page of its neighbours' size,
filled with the same neutral grey the renderer paints over an image it cannot
decode, with `ArchiveWarning::PlaceholderPage` naming why. Dropping it would
renumber every page after it and the story would jump with nothing anywhere
saying so. Classification is by magic bytes, never by extension — a `.jpg`
that is a PNG is routine — with one narrow exception: an entry whose bytes
cannot be read at all (encrypted, checksum failed) is judged by its name,
because a spurious placeholder is visible where a missing page is not. Entries
that are not images at all — `Thumbs.db`, `__MACOSX/`, directory records (a
stored path ending in `/`, APPNOTE 4.4.17.1) — are neither pages nor warnings.

**`ComicInfo.xml` is a third thing an entry can be.** Not a page, not ignored:
it is read with `tinker-pdf-xml` and becomes the synthesised document's `/Info`
dictionary. Six of ComicRack's fifty-odd elements are mapped, because six is
what §14.3.3 has anywhere sensible to put them — `Title` to `/Title`, `Series`
and `Number` to `/Keywords` and, **when there is no `Title`**, to `/Title` as
`Series #Number` (most issues carry no title of their own, and the series and
the number are what the book is called); `Writer` and `Penciller` to `/Author`,
joined, one name when they are one person; `Summary` to `/Subject`. `Publisher`
is deliberately not `/Creator` or `/Producer`: those two name the application
that wrote the file, and this engine is that application. It is matched by its
whole stored path, case-insensitively — a nested copy describes something that
is not this document. `MAX_COMIC_INFO_BYTES` (64 KiB) decides whether it is
parsed at all, because a `/Info` dictionary is not a page and nothing else in
this path would have bounded it. Everything that can go wrong degrades: the
document is every page the archive holds either way, with
`ArchiveWarning::ComicInfo` naming why the metadata did not arrive.
`ArchiveReport::comic_info()` is what was read — `Some` and empty for a
`<ComicInfo/>` that names nothing, `None` for an archive that carries no such
file, which is the distinction a warning would otherwise have had to make.

**Bounds on the synthesis itself.** At most `MAX_CBZ_PAGES` (4 096) pages, and
at most `MAX_SYNTHESISED_PDF` (512 MiB) of document, charged *before* each
page is built on what that page has undertaken to contribute — because the
archive's own caps bound the image data and nothing else bounds the object
graph.

**Archives real archivers wrote.** Gap 29 closed owing this, and it is owed no
longer: `crates/tinker-pdf/tests/cbz/` holds five `.cbz` from four independent
ZIP implementations — 7-Zip 26.02 at two compression levels, WinRAR 7.20, .NET's
`System.IO.Compression` and CPython 3.12's `zipfile` — over pages this
repository wrote from the PNG and JPEG specifications. The hand-built fixtures
stay exactly where they are: the two corpora answer different questions, and
the census in that directory's README says which. **Real archivers tripped no
warning at all** and took the central-directory route every time, so the
leniency ladder's value still rests entirely on the fixtures built to exercise
it — including the streamed-entry path of APPNOTE 4.3.9, which **no real
producer here emits** and which is stated as a remaining gap rather than left
to be inferred.

Two things the first real archives found. **A real archive mixes methods per
entry** — 7-Zip and WinRAR both stored three pages and deflated two, deciding
per entry — where every hand-built fixture used one method throughout. And
**.NET and CPython deflate unconditionally, producing three entries larger than
their originals**, so `compressed_size > uncompressed_size` is ordinary rather
than a corruption signal; a reader that treated it as one would refuse most
comics Windows software makes.

## API

`Document::open` (or `open_with`) does everything: it sniffs, routes,
synthesises and parses, and a container that cannot become a document returns
`OpenError::UnsupportedArchive(ArchiveRefusal)`. A synthesised document then
reports its provenance through `Document::archive()`, which is `None` for an
ordinary PDF — that is how a caller tells the two apart.

```rust
use tinker_pdf::{Document, RenderOptions};

let doc = Document::open(std::fs::read("issue-01.cbz")?)?;
let report = doc.archive().expect("a synthesised document carries a report");
for origin in report.pages() {
    // origin.name is the archive entry's stored path;
    // origin.defect is None when the page is the entry's own picture.
}
let bitmap = doc.page(0).expect("a page").render(&RenderOptions::default());
```

- `ArchiveReport` — `pages()` (one `PageOrigin` per page, in page order),
  `warnings()` (every `ArchiveWarning`, in the order it happened, ruling 10),
  `synthesised_bytes()`, published so the byte bound is measurable rather
  than asserted about in prose, and `comic_info()`.
- `cbz::container(bytes)` — the fixed-position sniff, returning `Container`.
- `cbz::image_format(bytes)` — magic-byte classification, returning
  `ImageFormat`.
- `cbz::comic_info::parse(bytes, &xml_limits)` — `ComicInfo.xml` to a
  `ComicInfo`, whose `info_entries()` is the `/Info` mapping itself rather than
  a description of it, and `cbz::comic_info::is_comic_info(name)`, which is the
  only place a comic entry's *name* decides that it is metadata.
- `cbz::synthesise(container, bytes, &cbz::Limits)` — archive bytes to
  `(Vec<u8>, ArchiveReport)`, public so a host can convert a comic to a PDF
  for its own sake and so tests can reach the bounds without building half a
  gigabyte. `cbz::open_archive` and `cbz::pages_from_archive` are the two
  halves `Document::open` actually uses, split so the sniff and the read share
  one `Archive`.
- `tinker_pdf_zip::Archive` — `open`, `entries()`, `read(index)` (checked;
  stored entries are handed back borrowed, copied nowhere), `route()`,
  `warnings()` and `inflated()`.
- `cbz::open_tar` and `cbz::pages_from_tar`, the same two halves for a `.cbt`,
  over `tinker_pdf_archive::tar::Archive` — `open`, `entries()`,
  `read(index)` (a plain borrow), `warnings()`.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| CBR, CB7 | `ArchiveRefusal::NotAZip` | two more decompressors and neither of them a page *yet* — staged in [design/comic-archives.md](../design/comic-archives.md) | [roadmap](../ROADMAP.md) |
| A sparse or multi-volume tar entry | `PageDefect::TarEntryRefused(TarEntryError)` | placeholder page; a reader that ignored the flag hands back bytes in the wrong places, which is worse than a page that failed | — |
| A tar with no `ustar` magic | `ArchiveRefusal::NotAZip` | the magic is the only signature tar has, so a reader that did not require it accepts anything | — |
| Archive damaged past recovery | `ArchiveRefusal::Damaged` | structure present, nothing recoverable from either route | — |
| Every page entry encrypted | `ArchiveRefusal::Encrypted` | ZipCrypto and the AES extensions are named non-goals; nothing is left to page | — |
| Spanned / multi-disk archive | `ArchiveRefusal::MultiDisk` | the fragment that happens to be here is not the archive | — |
| Zip64 value past the file | `ArchiveRefusal::Zip64OutOfBounds` | a declared size or offset the archive cannot contain | — |
| Valid archive, no image entries | `ArchiveRefusal::NoImages` | a zero-page open is a failure dressed as a success | — |
| Past a bound | `ArchiveRefusal::TooLarge` | `MAX_CBZ_PAGES`, `MAX_SYNTHESISED_PDF`, or one of the archive reader's own | — |
| One encrypted or checksum-failed entry | `PageDefect::EntryRefused(ZipEntryError)` | placeholder page; the page count and every number after it are unchanged | — |
| Compression method other than stored/deflated | `ZipEntryError::UnsupportedMethod(u16)` | shrink, implode, bzip2, LZMA, Zstandard — named by code so a refusal says which | — |
| GIF, WebP, BMP, AVIF, JPEG 2000 entries | `PageDefect::UnsupportedFormat(ImageFormat)` | recognised and named; a placeholder page rather than a dropped one | — |
| A JPEG, PNG or TIFF that will not decode | `PageDefect::Undecodable` | an unreadable header, a colour type outside the table, a `Compression` or `PhotometricInterpretation` refused by name, a raster past the ceiling | [filters](filters.md) |
| A `ComicInfo.xml` that will not read | `ArchiveWarning::ComicInfo(ComicInfoDefect)` | past 64 KiB, an entry the archive refused, markup that is not well formed, or a root that is not `ComicInfo`; the pages are unaffected | — |

## Verified

- `crates/tinker-pdf/tests/cbz.rs` — 38 tests over what the synthesis alone
  owns: the fixed-position sniff (a PDF carrying `PK\x03\x04` in a stream
  stays a PDF), natural order, pass-through, placeholders, and the bounds —
  `a_page_count_past_the_cap_is_refused_by_name` builds a real 4 097-entry
  archive rather than lowering the constant, and
  `the_synthesised_document_fits_inside_what_was_charged_for_it` measures the
  page-overhead charge against documents of 1 to 200 pages. Pictures are
  asserted against literal expected pixels, not only against another render.
  Five of the 38 are the TIFF route, and the one worth naming is
  `a_placed_tiff_renders_the_same_as_a_decoded_one`: the same picture as one
  G4 strip and as two, which is one `/CCITTFaxDecode` stream carrying the
  file's own bytes against one `/FlateDecode` stream of eight-bit samples —
  two dictionaries, two filters, two bit depths, and **0 pixels different**.
  Both sides are held to the literal pattern as well, because two identical
  blank pages compare equal.
- `crates/tinker-pdf/tests/cbz_validated.rs` — the synthesised document and
  the same document saved back are both held to the strict validator, and the
  pages are read out of the catalog's own `/Kids` rather than through the
  tolerant page walk: the `/MediaBox` from each page object and the dimensions
  from each image XObject's own `/Width`, which are two independent claims. It
  replaces the qpdf oracle retired with ruling 9, and **what left with that
  oracle is that a reader nobody here wrote accepts the file**
  ([verification](../verification.md)).
- `crates/tinker-pdf/tests/cbz_real.rs` — the five archives real archivers
  wrote, described in `tests/cbz/README.md`. Every row of `INVENTORY.tsv` is
  recomputed through `tinker-pdf-zip` on every `cargo test`, so .NET's reader
  and this one must agree about all 25 entries — and they reach the answer
  differently, since .NET exposes no method code and infers it from the two
  lengths where this reader has the central directory's own field. The
  assertion worth having is `five_zip_writers_produce_the_same_five_pictures`:
  five implementations sharing no code, disagreeing about what to store and
  what to deflate, must give this reader the same five pictures at the same
  sizes. It is a relation between two reads rather than an oracle — nothing
  outside this repository renders any of it ([verification](../verification.md)).
  The `.cb7`, `.cbt` and `.cbr` beside them hold the *same five pages* and are
  held to `ArchiveRefusal::NotAZip`, so a decoder that arrives later has
  something to be compared against that was put there before it existed.
- `crates/tinker-pdf-zip/src/tests.rs` — 40 tests over both routes of the
  archive reader; `crates/tinker-pdf/src/cbz/tests.rs` — 28 unit tests over
  ordering, classification and the `ComicInfo.xml` mapping, which is asserted
  as the table it is rather than as whatever the code emitted.
  `the_metadata_entry_is_read_and_is_still_not_a_page` makes the three claims
  separately, because the middle one is the one that would have gone quietly:
  the *name* answers the metadata question, the *extension* still answers false
  to the image question, and the bytes are not an image either.
- [Determinism](determinism.md) — the `cbz` render fingerprint (one of the
  15) opens a four-page mixed archive whose stored, lexicographic and natural
  orders all disagree about which entry is page 0, so a regression to either
  wrong order changes the picture *and* its dimensions; and one of the 3
  document byte-hashes covers the synthesised bytes whole, including the
  writer's deflate encoder and the pages no fingerprint renders.
- Fuzzing ([verification](../verification.md)) — `fuzz_targets/zip_archive.rs`
  drives both parsers over the same bytes and asserts, beyond "no panic", that
  a successful read produced exactly the declared length, spent no more than
  the archive's total, and that every entry is either checksummed or refused;
  `fuzz_targets/png.rs` covers the decoder the non-pass-through routes take,
  and `fuzz_targets/tiff.rs` the one a scanned comic reaches. Three of the 32
  targets.
- The whole workspace: `cargo test --workspace` is 3 024 passed, 0 failed,
  9 ignored across 123 suites (Windows x86_64, as of August 2026).
