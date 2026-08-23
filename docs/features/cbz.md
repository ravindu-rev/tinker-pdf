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
it) are recognised at fixed positions too, and refused by name: "this is a CBR
and I do not read CBR" is a different sentence from "this is not a PDF". A ZIP
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
that are not images at all — `ComicInfo.xml`, `Thumbs.db`, `__MACOSX/`,
directory records (a stored path ending in `/`, APPNOTE 4.4.17.1) — are
neither pages nor warnings.

**Bounds on the synthesis itself.** At most `MAX_CBZ_PAGES` (4 096) pages, and
at most `MAX_SYNTHESISED_PDF` (512 MiB) of document, charged *before* each
page is built on what that page has undertaken to contribute — because the
archive's own caps bound the image data and nothing else bounds the object
graph.

**The honest caveat.** Every committed fixture is hand-built from APPNOTE's
field layouts; no `.cbz` written by a real archiver has ever been opened by
this engine. That is recorded as owed in the [roadmap](../ROADMAP.md).

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
  `warnings()` (every `ArchiveWarning`, in the order it happened, ruling 10)
  and `synthesised_bytes()`, published so the byte bound is measurable rather
  than asserted about in prose.
- `cbz::container(bytes)` — the fixed-position sniff, returning `Container`.
- `cbz::image_format(bytes)` — magic-byte classification, returning
  `ImageFormat`.
- `cbz::synthesise(container, bytes, &cbz::Limits)` — archive bytes to
  `(Vec<u8>, ArchiveReport)`, public so a host can convert a comic to a PDF
  for its own sake and so tests can reach the bounds without building half a
  gigabyte. `cbz::open_archive` and `cbz::pages_from_archive` are the two
  halves `Document::open` actually uses, split so the sniff and the read share
  one `Archive`.
- `tinker_pdf_zip::Archive` — `open`, `entries()`, `read(index)` (checked;
  stored entries are handed back borrowed, copied nowhere), `route()`,
  `warnings()` and `inflated()`.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| CBR, CB7, CBT | `ArchiveRefusal::NotAZip` | three more decompressors, two of them encumbered, and none of them a page | [roadmap](../ROADMAP.md) |
| Archive damaged past recovery | `ArchiveRefusal::Damaged` | structure present, nothing recoverable from either route | — |
| Every page entry encrypted | `ArchiveRefusal::Encrypted` | ZipCrypto and the AES extensions are named non-goals; nothing is left to page | — |
| Spanned / multi-disk archive | `ArchiveRefusal::MultiDisk` | the fragment that happens to be here is not the archive | — |
| Zip64 value past the file | `ArchiveRefusal::Zip64OutOfBounds` | a declared size or offset the archive cannot contain | — |
| Valid archive, no image entries | `ArchiveRefusal::NoImages` | a zero-page open is a failure dressed as a success | — |
| Past a bound | `ArchiveRefusal::TooLarge` | `MAX_CBZ_PAGES`, `MAX_SYNTHESISED_PDF`, or one of the archive reader's own | — |
| One encrypted or checksum-failed entry | `PageDefect::EntryRefused(ZipEntryError)` | placeholder page; the page count and every number after it are unchanged | — |
| Compression method other than stored/deflated | `ZipEntryError::UnsupportedMethod(u16)` | shrink, implode, bzip2, LZMA, Zstandard — named by code so a refusal says which | — |
| GIF, WebP, BMP, TIFF, AVIF, JPEG 2000 entries | `PageDefect::UnsupportedFormat(ImageFormat)` | recognised and named; a placeholder page rather than a dropped one | — |
| A JPEG or PNG that will not decode | `PageDefect::Undecodable` | an unreadable header, a colour type outside the table, a raster past the ceiling | — |
| `ComicInfo.xml` | *(none — skipped by design)* | metadata is neither a page nor a warning; reading it is a named non-goal | [roadmap](../ROADMAP.md) |

## Verified

- `crates/tinker-pdf/tests/cbz.rs` — 33 tests over what the synthesis alone
  owns: the fixed-position sniff (a PDF carrying `PK\x03\x04` in a stream
  stays a PDF), natural order, pass-through, placeholders, and the bounds —
  `a_page_count_past_the_cap_is_refused_by_name` builds a real 4 097-entry
  archive rather than lowering the constant, and
  `the_synthesised_document_fits_inside_what_was_charged_for_it` measures the
  page-overhead charge against documents of 1 to 200 pages. Pictures are
  asserted against literal expected pixels, not only against another render.
- `crates/tinker-pdf/tests/cbz_validated.rs` — the synthesised document and
  the same document saved back are both held to the strict validator, and the
  pages are read out of the catalog's own `/Kids` rather than through the
  tolerant page walk: the `/MediaBox` from each page object and the dimensions
  from each image XObject's own `/Width`, which are two independent claims. It
  replaces the qpdf oracle retired with ruling 9, and **what left with that
  oracle is that a reader nobody here wrote accepts the file**
  ([verification](../verification.md)).
- `crates/tinker-pdf-zip/src/tests.rs` — 40 tests over both routes of the
  archive reader; `crates/tinker-pdf/src/cbz/tests.rs` — 17 unit tests over
  ordering and classification.
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
  `fuzz_targets/png.rs` covers the decoder the non-pass-through routes take.
  Two of the 24 targets.
- The whole workspace: `cargo test --workspace` is 2 940 passed, 0 failed,
  8 ignored (Windows x86_64, as of August 2026).
