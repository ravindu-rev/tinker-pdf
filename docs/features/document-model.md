# Document model

Everything a document says about itself before a content stream is opened:
identity (`/Info`, the format version), shape (the page tree and its
geometry), navigation (outline, destinations, page labels, links, actions)
and payload (attachments, XMP). Every structure here is a tree walk over an
untrusted graph, so every walk carries the same equipment — a visited set, a
depth and entry cap, and a typed warning naming what was tolerated
([ruling 10](../rulings.md)).

## What it does

**Metadata.** All nine `/Info` fields (14.3.3), looked up in the trailer
(7.5.5 Table 15) and, for producers that put it there instead, the catalog.
Absent-not-empty is a contract: a key the document never wrote is `None`, a
key holding `()` is `Some("")`, and whitespace survives untrimmed — a viewer
shows "(untitled)" for one and a blank field for the other, and nothing
downstream can recover the difference once collapsed. `/Trapped` (7.7.2,
Table 349) carries the same distinction one level in: it is a name, not a
string, read as `Trapped::True`/`False`/`Unknown`, where `Some(Unknown)` is
the document answering — including with a name outside the three — and
`None` is the document silent. Text strings decode per 7.9.2.2: UTF-16BE
behind `FE FF`, PDF 2.0's UTF-8 behind `EF BB BF`, PDFDocEncoding (Annex D)
otherwise, with damage becoming U+FFFD rather than an error. Dates parse
leniently per 7.9.4 through `Metadata::created()`/`modified()`, with the raw
strings kept beside them.

**Version.** The later of the header's (7.5.2) and the catalog's `/Version`
(7.7.2), because that entry exists to let an incremental update *raise* the
version without touching header bytes a signature covers — a stale
`/Version /1.4` cannot demote a 1.7 file. The two are compared as the `M.N`
number pair 7.5.2 spells, never as text, so 1.10 outranks 1.9; a version
that does not parse is absent, not zero. A document stating no readable
version anywhere reports the 1.7 baseline and says so with
`WarningKind::HeaderMissing`, so the guess is on the record. Reporting a
version is not enforcing one: no feature is gated on it.

**Page tree.** One walk (7.7.3) builds the whole index: pages in document
order, each with the four inheritable attributes of 7.7.3.4 — `/Resources`,
`/MediaBox`, `/CropBox`, `/Rotate` — already resolved, whether written
inline or by reference. A node without `/Kids` is a leaf whatever its
`/Type` claims, because plenty of files claim nothing. Geometry is
normalised: corners ordered, `/CropBox` clipped to `/MediaBox` and equal to
it when absent (7.7.3.3), `/Rotate` reduced to {0, 90, 180, 270} with
off-quarter values rounding to the nearest turn, and `Page::size()`
reporting the displayed size with the quarter-turn axis swap applied.
`/Count` is a claim like any other: a document that lies about it gets
counted by walking instead of believed.

**Name and number trees.** One module (7.9.6, 7.9.7) serves `/Dests`,
`/EmbeddedFiles` and `/PageLabels`. Keys are byte strings matched literally,
never text-decoded first. Full enumeration walks every leaf and sorts
afterwards, because enough producers break the sorted-keys promise;
targeted lookup (`name_tree_lookup`) descends by `/Limits`, skipping
subtrees whose declared range excludes the key — and a node whose `/Limits`
are missing or malformed is descended into anyway, since a damaged index is
no evidence the entry is gone.

**Destinations.** `Destination` is a three-variant enum — `Explicit` (a
page and a `DestKind` view), `Named` (bytes to look up in the document's own
tables) and `Uri` — and the three are never conflated, in either direction
([ruling 6](../rulings.md)). Reading never collapses a URI into a name;
writing round-trips each variant to its own syntax (12.3.2.2 Table 151 for
arrays, 12.6.4.7 for URI actions), and the reader and writer for each form
live in one file so they cannot drift. All eight view kinds of Table 151
are read, with `null` components kept as `None` — "leave the current value" —
and a zoom of 0 folded onto `None` because 12.3.2.2 gives both spellings
one meaning. Named destinations resolve through the `/Names` → `/Dests`
name tree (12.3.2.3) and the legacy catalog `/Dests` dictionary, accepting
the entry as a bare array or a dictionary carrying it under `/D`.

**Outline.** The `/First`/`/Next` walk of 12.3.3, cycle-guarded on both
axes, titles decoded as text strings, the open state from the sign of
`/Count`, and each entry's target taken from `/Dest` or its `/A` action
with `/Dest` winning where both exist. An absent `/Outlines` is an empty
outline, which is an ordinary answer and not an error.

**Actions and links.** `/GoTo`, `/GoToR`, `/URI`, `/Named` and `/Launch`
(12.6.4) are read as data; anything else is preserved as `Action::Other`
with its `/S` subtype rather than discarded. `Page::links()` reads
`/Subtype /Link` annotations only (12.5.6.5), in `/Annots` order, each with
its ordered `/Rect` and resolved target — and a link carrying neither
`/Dest` nor a usable `/A` is kept with `target: None`, because a reader
drawing link borders still has to draw that one.

**Attachments and XMP.** `/Names` → `/EmbeddedFiles` (7.11.4) lists every
attached file with its `/UF`-preferred filename (7.11.3), description,
declared size and the stream reference — the bytes deliberately not read,
so listing costs less than extracting. The catalog's `/Metadata` stream
(14.3.2) comes back as decoded raw bytes: XMP is RDF/XML, and a caller that
wants it parsed already has a reader.

**Page labels.** The `/PageLabels` number tree (12.4.2) yields one label
per page: all five styles of Table 159 plus the bare prefix, with the
letter styles repeating — 27 is "AA", not spreadsheet base-26 — and roman
numerals capped so a hostile `/St` cannot emit a page of M's.

## API

Everything is on the facade `Document` and `Page`: `metadata()`,
`pdf_version()`, `outline()`, `page_labels()`, `attachments()`,
`xmp_metadata()`, `page_count()`, `pages()`, `page(index)`, `layers()`,
`fonts()`, and `Page::media_box()`, `crop_box()`, `rotation()`, `size()`,
`links()`, `annotations()`. The types they hand back — `Metadata`, `Trapped`,
`OutlineItem`, `Destination`, `DestKind`, `Action`, `Link`, `Attachment`,
`OptionalGroup`, `Annotation`, `AnnotationKind`, `AnnotationFlags` — are
re-exported from the same crate. The writing side takes the same vocabulary: `Target` wraps a page
plus `DestKind` or a URI for `PageBuilder::link` and `OutlineEntry`, so a
write followed by a read is an equality, not a translation.

```rust
let doc = tinker_pdf::Document::open(bytes)?;
let meta = doc.metadata();
println!("{} — {}", doc.pdf_version(),
         meta.title.as_deref().unwrap_or("(untitled)"));
for (depth, item) in tinker_pdf::OutlineItem::flatten(&doc.outline()) {
    println!("{:indent$}{}", "", item.title, indent = depth as usize * 2);
}
```

### Layers, as built

`Document::layers()` is the read half of optional content (8.11). It returns
the catalog's `/OCProperties /OCGs` in the catalog's own order — which is the
order a producer's layer panel shows, not object-number order — each entry
carrying the group's reference, its `/Name` decoded as a text string, and
whether the default configuration `/D` shows it. `/BaseState` then `/ON` then
`/OFF`, exactly 8.11.4.3 Table 101's order.

The visibility comes **from the reader the renderer uses**, not from a second
read of the dictionary: both call the one `OptionalContent::bind`, so the list
and the painted page cannot disagree about the same file. A group with no
`/Name` is listed with an empty one rather than dropped, because a caller
toggling layers still has to see it. Most documents declare no optional
content and get an empty list, which is an ordinary answer.

Writing groups — `DocumentBuilder::add_layer`, and an editor that toggles a
default configuration — is the other half of that roadmap row and is not in
this build.

### Annotations, as built

`Page::annotations()` returns one `Annotation` per entry of the page's
`/Annots`, in the array's own order (12.5.2). It is **total by construction up
to ruling 1's bound of 4 096 entries per page**: a `/Subtype` no edition of ISO
32000 defines comes back as `AnnotationKind::Other` carrying the name the file
used, a dictionary with no `/Subtype` as `Unnamed`, and an entry that is not a
dictionary at all as `Unreadable`. Nothing below the bound is dropped, which is
how ruling 10's "name what you touched" is satisfied for a list: a caller
auditing a file counts what this build does not model instead of comparing
lengths to find out what went missing.

Past 4 096 the list is shortened and **nothing says so**. That is forced
rather than chosen: the only place a truncation could be reported is
`Document::warnings()`, and appending to it from a read would make the
warnings depend on whether anyone had called `annotations()` first — the same
argument that keeps `fonts()` off `cos::font::read`. The bound is pinned by
`a_hostile_annots_array_is_capped`, which exists because raising the constant
to a hundred thousand previously failed nothing in the crate; the corpus's
largest page carries 122, three orders of magnitude below it, so no real file
is affected. Reporting the truncation is a roadmap row, not a claim made here.

`AnnotationKind` covers ISO 32000-1 Table 169's twenty-six subtypes and ISO
32000-2's two, and the table is transcribed a second time in the test beside
it and compared. Each entry carries Table 164's common entries and Table 170's
markup ones: `/Rect` normalised so `x0 <= x1` (7.9.5), `/Contents`, `/T`, `/M`
both as the file's own text *and* as a parsed 7.9.4 date, `/F` as a raw
`AnnotationFlags` with Table 165's ten bit accessors, `/Popup`, `/Parent`, and
whether `/AP` carries an `/N`. A pop-up's `/Contents`, `/T` and `/M` come from
its `/Parent` (12.5.6.14 Table 183) — and only that way: a markup annotation's
text is never read through its own `/Popup`, which the clause does not licence
and which would report a note's text as whatever its window happened to carry.

`Page::links()` is unchanged and stays the narrower navigation view over the
same array: `/Link` annotations with their destinations **resolved**, which is
a question about targets rather than about annotations (ruling 6). What is
*not* here is per-subtype geometry — `/QuadPoints`, `/InkList`, `/Vertices`,
`/L` and their relatives — which is one payload per 12.5.6 family and has a
roadmap row of its own.

**Measured over the corpus.** `crates/tinker-pdf/tests/annotation_census.rs`,
`#[ignore]`d and run with `-- --ignored --nocapture`, over the 1 012 fetched
files whose raw bytes name `/Annots` (15 September 2026; all 1 012 opened,
982 carried an annotation): **14 996 annotations read, 14 986 covered and 10
refused**, across 28 distinct covered subtypes — every subtype the model
defines appears in the corpus — and 6 refused names:

| Refused subtype | Count | First seen in |
| --- | --- | --- |
| `FREETEXT` | 5 | `verapdf/PDF_UA-1/7.18 Annotations/7.18.1 General/7.18.1-t01-pass-c.pdf` |
| `APEX:Zone` | 1 | `qpdf/examples/qtest/mod-info/files/source2.pdf` |
| `SomePrivateCustomAnnotationType` | 1 | `verapdf/Isartor test files/PDFA-1b/6.5 Annotations/6.5.2 Annotation types/isartor-6-5-2-t01-fail-c.pdf` |
| `line` | 1 | `verapdf/PDF_A-4/6.3 Annotations/6.3.1 Annotation types/veraPDF test suite 6-3-1-t01-fail-g.pdf` |
| (no `/Subtype`) | 1 | `pdfjs/test/pdfs/issue7446.pdf` |
| (not a dictionary) | 1 | `pdfjs/test/pdfs/annotation-text-without-popup.pdf` |

That is 0.067% of the corpus refused, and the two case-shifted names —
`FREETEXT` and `line` — are why 7.3.5's rule that a name's identity is its
bytes is applied rather than a case-insensitive match: both are veraPDF
fixtures that exist *because* the spelling is wrong, and folding the case
would hide the defect they were written to expose.

The largest `/Annots` on one page is 122, in
`pdfjs/test/pdfs/prefilled_f1040.pdf` — three orders of magnitude under the
4 096 cap. Of the 281 pop-ups, 276 have a `/Parent` and 140 report text
through it. Every one of the 1 658 `/M` entries found parses as a 7.9.4 date,
and 5 129 annotations carry a normal appearance.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| A `/Kids` graph that revisits a node | `WarningKind::PageTreeCycle` | 7.7.3.2 makes the tree a tree; following a repeat duplicates pages forever | [ruling 10](../rulings.md) |
| A page tree past the depth or page cap | `WarningKind::PageTreeTruncated` | bounded truncation beats an unbounded walk over hostile input | [ruling 1](../rulings.md) |
| No usable `/MediaBox` on the whole path | `WarningKind::MediaBoxMissing` | 7.7.3.3 requires one; US Letter is guessed and the guess recorded | [ruling 10](../rulings.md) |
| A `/Count` that disagrees with the walk | `WarningKind::PageCountMismatch` | the count is a claim; the walk is the fact | [ruling 10](../rulings.md) |
| An outline `/First`/`/Next` loop, or one past the caps | `WarningKind::OutlineCycle`, `OutlineTruncated` | a looping sibling chain never ends on its own | [ruling 1](../rulings.md) |
| A name/number tree cycle, cap breach, or odd-length leaf | `WarningKind::TreeCycle`, `TreeTruncated`, `TreeOddEntries` | the last key of an odd `/Names` array has no value | [ruling 10](../rulings.md) |
| No readable version in header or catalog | `WarningKind::HeaderMissing` | the 1.7 baseline is reported and the guess stays on the record | [ruling 10](../rulings.md) |
| `/Launch` and every other action | `Action::Launch`, `Action::Other` | reported, never executed — running a program because a document asked is not a service | [architecture](../architecture.md) |
| Writing a view with a NaN coordinate, a negative zoom or an empty `/FitR` | `DestKind::is_writable` → `false` | `NaN` is not a PDF number, and an empty rectangle asks for infinite magnification | [ruling 6](../rulings.md) |
| Writing a URI that is empty, non-ASCII or control-bearing | `is_writable_uri` → `false`; `PageBuilder::link` returns `false` | 12.6.4.7 makes `/URI` 7-bit ASCII; percent-encoding is the caller's, since only the caller knows the bytes' encoding | [ruling 6](../rulings.md) |

## Verified

As of 15 September 2026, in the workspace suite of 4 779 passing tests
(0 failed, 58 ignored, 218 suites, Windows x86_64, measured on this
branch — other lanes are moving the total in parallel):

- `crates/tinker-pdf/src/layers.rs` and `src/annotations.rs` — 6 and 13 unit
  tests beside the code: `/BaseState` inverted by `/ON`, a nameless group
  still listed, the listing reading the renderer's own bound configuration;
  Table 169 transcribed a second time and compared against the enum, nothing
  in `/Annots` dropped, 12.5.6.14 in both directions, a pop-up parent cycle
  that terminates, Table 165 bit by bit, a reversed `/Rect` ordered, a
  `/M` that is not a date carried as text, and a 4 097-entry `/Annots` array
  cut to ruling 1's bound — the last written after an injection found the cap
  guarded by nothing at all.
- `crates/tinker-pdf/tests/facade_read.rs` — 7 tests over one fixture, run
  from **outside** the crate the way ruling 11 makes the contract: each is
  named for the defect it re-creates rather than for the feature, and one of
  them asserts that listing fonts, layers and annotations leaves
  `Document::warnings()` where it was.
- `crates/tinker-pdf/tests/annotation_census.rs` — the two `#[ignore]`d corpus
  censuses above, printing `RAN`/`SKIPPED` so a missing corpus cannot read as
  a pass, with floors at the counts recorded here.
- `crates/tinker-pdf/tests/tinker_parity.rs` — the ported parity tests
  ([ruling 12](../rulings.md)): `pdf_version()` returns exactly `"PDF 1.7"`, the three-level
  outline nests with zero-based page indices, and a document without an
  outline returns an empty one.
- `crates/tinker-pdf/tests/writer_navigation.rs` — [ruling 6](../rulings.md)
  round-trips:
  an internal link reads back `Explicit`, an external one reads back a URI
  action, an unwritable target is refused leaving nothing behind, and an
  outline round-trips its shape, destinations and open flags.
- `crates/tinker-pdf/tests/page_geometry.rs` — rendered assertions that
  `/Rotate` moves ink to the right corner at every quarter turn and a
  shifted `/CropBox` origin lands content where it should.
- `crates/tinker-pdf/tests/hostile_input.rs` — mutation rounds that call
  `metadata()`, `pdf_version()`, `outline()` and `page_labels()` on every
  damaged document that still opens.
- `crates/tinker-pdf-cos/tests/semantics.rs` and `semantics_extras.rs` —
  fixture-based assertions on geometry, version, metadata, outlines and
  explicit-not-named destinations, plus attachments, XMP bytes, and
  `/Limits` descent including a node whose limits lie.
- Unit tests beside the code in `pages.rs`, `outline.rs`, `dest.rs`,
  `trees.rs` and `text_string.rs`: version comparison as numbers, blank
  against absent for every `/Info` field, `/Trapped`'s three names, corner
  ordering, rotation normalisation, the 27 → "AA" letter repetition, and
  the URI/named-destination distinction pinned as a type inequality.
- The `cos_document` fuzz target — one of the 24 — walks the page tree and
  reads content bytes after every successful open, so a document that opens
  and then panics on use counts as a crash. The corpus run backs it at
  scale: 5 525 files, 5 516 rendered every page, 0 crashes (September 2026).

The document byte-hashes in [determinism](determinism.md) pin the writing
half: a synthesised document's outline, links and page tree are part of the
bytes those hashes freeze. The verification approach as a whole is
[../verification.md](../verification.md).
