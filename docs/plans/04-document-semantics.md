# Phase 04 — Document semantics

When this phase is done, a `Document` answers every non-content question Tinker asks:
its shape (page count, per-page geometry), its identity (`/Info`, format version), its
navigation (outline, destinations, page labels, links), and its payload (attachments,
XMP). It is the semantic layer between the COS object graph
([01-cos-and-object-model](01-cos-and-object-model.md)) and the content interpreter
([06-content-and-text](06-content-and-text.md)), and it is shaped by one recurring
observation: everything here is a tree walk over an untrusted graph. So every feature
gets the same treatment — a cycle-guarded walk, a typed leniency policy with
provenance (ruling 10 in [99-consistency](99-consistency.md)), and a subprocess oracle
differential to prove the answers are right at corpus scale, not just on fixtures.

## Scope

- `/Info` dictionary (Table 317): all nine fields Tinker reads — Title, Author,
  Subject, Keywords, Creator, Producer, CreationDate, ModDate, Trapped — with
  absent-not-empty semantics. Text-string decoding per 7.9.2.2 (UTF-16BE,
  PDFDocEncoding, and the PDF 2.0 UTF-8 BOM as a read-side delta), date parsing per
  7.9.4 as a helper, raw strings preserved.
- Format version: header comment (7.5.2) overridden by catalog `/Version` (7.7.2)
  when greater; rendered exactly as `"PDF 1.7"` for Tinker parity.
- Page tree (7.7.3): `/Kids` walk with cycle guards, attribute inheritance of
  `/Resources`, `/MediaBox`, `/CropBox`, `/Rotate` (7.7.3.4), page count without
  loading all pages, O(depth) random access by descending intermediate `/Count`.
- Per-page geometry: normalized boxes, effective crop = CropBox ∩ MediaBox, rotation
  normalized to {0, 90, 180, 270}.
- Name trees (7.9.6) and number trees (7.9.7) as one generic, reusable module.
- Destinations (12.3.2): explicit arrays (XYZ/Fit/FitH/FitV/FitR/FitB/FitBH/FitBV),
  named destinations via the `/Names` → `/Dests` name tree and the old-style catalog
  `/Dests` dictionary (PDF 1.1), resolution to zero-based page indices.
- Outline (12.3.3): `/First`/`/Next` walk with cycle guards, titles as text strings,
  `/Dest` and `/A` on items, open state from the `/Count` sign.
- Actions (12.6.4): GoTo, GoToR, URI, Named, Launch — reported as data, never
  executed.
- Links: page `/Annots` entries with `/Subtype /Link` (12.5.6.5) — rect plus resolved
  target. No other annotation subtype is touched.
- Page labels: `/PageLabels` number tree (12.4.2) → label string per page index, all
  five numbering styles plus prefix.
- Embedded files: `/Names` → `/EmbeddedFiles` name tree, file specification
  dictionaries (7.11.3), embedded stream data and `/Params` (7.11.4).
- Document XMP: catalog `/Metadata` stream (14.3.2) as decoded raw bytes, passthrough.

## Non-goals

- **Structured text, search, tagged reading order** — the text device in
  [06-content-and-text](06-content-and-text.md). This phase stops at the page's
  resource dictionary and `/Contents` reference; it never opens a content stream.
- **Rendering anything** — link rects and annotation appearances are drawn by
  [08-rendering-device](08-rendering-device.md).
- **Writing or round-tripping** — [09-writing](09-writing.md) and
  [12-creation](12-creation.md) serialize this model; ruling 6 binds them to the same
  destination enum so nothing read here is lossy on the way back out.
- **Annotations beyond `/Link`** — reading markup annotations, and `FileAttachment`
  annotations as an attachment source, belong to [10-editing](10-editing.md).
- **AcroForm, the JavaScript name tree, widget annotations** —
  [11-forms](11-forms.md).
- **XMP parsing or Info↔XMP reconciliation** — the engine passes bytes through;
  which of the two wins is application policy, and Tinker reads `/Info`.
- **Executing actions** — never, in any phase. `Launch` and `URI` are data about the
  document, and treating them as anything else is a vulnerability, not a feature.

## Design

### Where the code lives

In the `tinker-pdf` facade crate, as the `doc` module tree. `Document`, `Page`,
`OutlineItem` and `Destination` *are* the public surface (ruling 11), and no other
crate consumes them: [06-content-and-text](06-content-and-text.md) receives resolved
streams and resource dictionaries at the COS level, and the writer serializes COS. A
dedicated `tinker-pdf-doc` crate would have exactly one consumer, which is a crate
boundary with cost and no benefit. This phase is what turns the facade from a
re-export shell into a library.

### Text strings first

Every feature here decodes text strings (7.9.2.2), so the helper comes first:
`FE FF` BOM → UTF-16BE (lone surrogates become U+FFFD with a warning); `EF BB BF` →
UTF-8, accepted on read as a PDF 2.0 delta and logged to `pdf20-deltas.md`; otherwise
PDFDocEncoding via a fixed 256-entry table transcribed from Annex D and tested
entry-by-entry against it. Undefined code points map to U+FFFD, never to an error — a
mangled title must not sink a document.

### `/Info` and version

```rust
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>, // raw; parse_date (7.9.4) is a helper
    pub modification_date: Option<String>,
    pub trapped: Option<Trapped>,      // True | False | Unknown — a name, not a string
}
```

Absent-not-empty is a contract: a key missing from `/Info` (or `/Info` missing
entirely) is `None`; a writer that emitted an empty string gets `Some("")`. Tinker's
UI distinguishes "no title" from "blank title", and collapsing the two on read makes
the distinction unrecoverable. Dates stay raw strings because that is what the parity
tests compare; the lenient date parser (missing apostrophes, `Z` offsets — both
common in the wild) is offered alongside, not imposed.

`/Trapped` carries the same distinction one level in. `None` is the key being absent;
`Some(Unknown)` is the document answering `/Unknown` — Table 349's own default, and
also what a name outside the three reads as, because a file that said something we did
not recognise is not a file that said nothing. A `/Trapped` holding a value that is not
a name at all is absent, which is what the string fields do with a name.

Version: the header found by the COS opener (junk prefixes already handled by repair
in [01-cos-and-object-model](01-cos-and-object-model.md)), overridden by the
catalog's `/Version` when that names a *later* version (7.7.2) — the two compared as
the `M.N` number pair rather than as text, and an unparseable version on either side
treated as absent rather than as zero. Rendered as `"PDF 1.7"` — the exact string
`open_documents.rs` asserts. If repair recovered a document whose header is
unreadable, the version reports the 1.7 baseline with a provenance warning
(`HeaderMissing`): guessing low misleads more than stating the baseline, and the
warning keeps "we guessed" on the record. Because the baseline always applies, the
version is a `String` and not an `Option<String>`: after this rule there is no such
thing as a document with no version to report.

Reporting a version is not enforcing one. No feature anywhere in the engine is gated
on it — leniency means reading what the file contains regardless of what it claims to
be.

*Amended, August 2026.* Three names in the sketch above had drifted from the shipped
code and are corrected here rather than in the code: the struct is `Metadata`, not
`Info` — `Info` is the PDF dictionary it reads, and naming the model after the
dictionary made two different things one word — its date field is
`modification_date`, and the date helper is the free function `parse_date`. The
version rules gained the sentences the code needed and this file only implied:
numeric comparison, unparseable-is-absent, and the `String` return that the baseline
makes total. Found while closing [gaps 21 and 22](gaps/README.md), whose `As built`
sections carry the detail.

### Page tree

The walk is iterative with an explicit stack and a visited set of object ids —
a `/Kids` cycle is an infinite loop in a naive reader, and the fuzzer will find one
even if the corpus doesn't. Inheritance (7.7.3.4) carries exactly the four
inheritable attributes down the descent: `/Resources`, `/MediaBox`, `/CropBox`,
`/Rotate`. Node classification is lenient: `/Kids` present means interior node,
otherwise leaf, regardless of what `/Type` claims — missing `/Type` is one of the
most common real-world defects.

`page_count()` reads the root `/Count` without touching a single page, clamped to
the xref entry count so a hostile value cannot drive allocation. `page(i)` descends
by intermediate `/Count`s in O(depth). The first inconsistency — a leaf at the wrong
index, a `/Count` that disagrees with its kids — triggers exactly one full
enumeration that builds the page map (`Vec<ObjId>`) and its reverse
(`HashMap<ObjId, u32>`, which destination resolution needs anyway), emits a warning,
and wins over `/Count` from then on. Trust the fast path, verify on contact,
repair once.

```rust
impl Document {
    pub fn page_count(&self) -> u32;
    pub fn page(&self, index: u32) -> Result<Page, Error>;
}

impl Page {
    pub fn media_box(&self) -> Rect;      // normalized corners
    pub fn crop_box(&self) -> Rect;       // already ∩ media
    pub fn rotation(&self) -> Rotation;   // 0 | 90 | 180 | 270
    pub fn geometry(&self) -> PageGeometry; // what Tinker's page_geometry maps from
    pub fn resources(&self) -> &Dict;     // inherited-resolved; handed to phase 06
    pub fn links(&self) -> Result<Vec<Link>, Error>;
}
```

Geometry policy, in order: normalize every box (writers emit corners in any order);
missing `/CropBox` → MediaBox; crop ∩ media empty or degenerate → MediaBox; missing
`/MediaBox` on the whole inheritance path → US Letter 612×792 with a warning (the
de-facto industry default). `/Rotate` is reduced modulo 360 into {0, 90, 180, 270};
a value that is not a multiple of 90 snaps to the nearest one with a warning rather
than failing — the spec forbids it, the wild contains it. `PageGeometry` reports the
effective display size with the 90/270 width/height swap applied, which is the number
`page_geometry_covers_every_page` asserts against.

### Name and number trees

One generic module, because `/Dests`, `/EmbeddedFiles` and `/PageLabels` are the
same structure with different leaves, and [09-writing](09-writing.md) and
[11-forms](11-forms.md) will need it again:

```rust
pub struct NameTree<'d> { /* root + resolver */ }

impl<'d> NameTree<'d> {
    pub fn get(&self, key: &[u8]) -> Option<Object>;
    pub fn iter(&self) -> impl Iterator<Item = (Vec<u8>, Object)>; // cycle-guarded
}

pub struct NumberTree<'d> { /* identical shape, i64 keys */ }
```

Keys are byte strings compared in lexicographic byte order — not UTF-8, and the API
does not pretend otherwise. Lookup descends by `/Limits`; the first sign that
`/Limits` lies or `/Names` is unsorted (generators that sort case-insensitively
produce exactly this) degrades that lookup to an exhaustive scan with a warning, so a
lying index can slow us down but never hide a key. Odd-length `/Names` arrays drop
the trailing key with a warning. `/Kids` cycles are guarded like the page tree.

### Destinations

The model is ruling 6, stated here once and binding the writer phases too:

```rust
pub enum Destination {
    Explicit { page: PageTarget, kind: DestKind },
    Named(String),
    Uri(String),
}

pub enum PageTarget { Ref(ObjId), Index(u32) }   // GoToR addresses by integer

pub enum DestKind {
    Xyz { left: Option<f32>, top: Option<f32>, zoom: Option<f32> },
    Fit,
    FitH { top: Option<f32> },
    FitV { left: Option<f32> },
    FitR { rect: Rect },
    FitB,
    FitBH { top: Option<f32> },
    FitBV { left: Option<f32> },
}
```

The three variants are never conflated. This is the design answer to MuPDF
limitation #6, where writing a URI outline entry silently produced a percent-encoded
*named* destination that resolved to nothing on read-back. Here a URI stays `Uri`, a
name stays `Named`, and [09-writing](09-writing.md) / [12-creation](12-creation.md)
round-trip each variant to its own syntax. `Option` on the XYZ components is
load-bearing: `null` means "keep the current view", and collapsing it to `0.0` is a
write-side corruption we refuse to set up.

Named resolution tries the `/Names` → `/Dests` name tree, then the old-style catalog
`/Dests` dictionary — the spec assigns strings to the former and names to the latter,
but we try both for either key shape because real files mix them. A named value may
be the destination array directly or a dictionary carrying it under `/D`. Keys that
are not valid UTF-8 (legal, never yet seen from a mainstream generator) resolve
through a byte-preserving side table so the lossy `String` in the public model can
never break lookup.

```rust
impl Document {
    /// Named → Explicit lookup, page ref → zero-based index via the reverse
    /// page map. None for dangling targets — the caller keeps the item.
    pub fn resolve_destination(&self, dest: &Destination) -> Option<ResolvedDest>;
}

pub struct ResolvedDest { pub page_index: u32, pub kind: DestKind }
```

A dangling destination resolves to `None` with a warning; the outline item or link
that carried it is kept. A bookmark with a dead target still belongs in the sidebar —
Tinker's `page: Option<u32>` DTO says the same thing.

### Outline, actions, links

```rust
pub struct OutlineItem {
    pub title: String,
    pub target: Option<Target>,
    pub open: bool,               // sign of /Count (12.3.3)
    pub children: Vec<OutlineItem>,
}

pub enum Target {
    Dest(Destination),  // /Dest, /A GoTo (same semantics per 12.3.2), /A URI → Uri
    Action(Action),     // everything else: reported, never executed
}

pub enum Action {
    GoToR { file: FileSpec, dest: Option<Destination> },
    Named(String),                 // /NextPage, /PrevPage, …
    Launch { target: Option<String> },
    Unsupported { subtype: Name }, // JavaScript, SubmitForm… — phase 11 territory
}

pub struct Link { pub rect: Rect, pub target: Target }
```

`/Dest` and a GoTo action collapse into `Target::Dest` because the spec defines them
as equivalent; URI actions become `Destination::Uri` because that is what they are.
`Unsupported` exists so an unrecognized subtype is visible rather than silently
`None` — "reported, never executed" applies uniformly, and for `Launch` doubly so:
its target is a string we hand to the caller, and nothing in this engine will ever
spawn it. Action `/Next` chains are not followed; the first action is modeled and the
presence of a chain is warned about, since chains only matter to a JavaScript-capable
viewer and [11-forms](11-forms.md) owns that question.

The outline walk guards both axes — child cycles via `/First` and sibling cycles via
`/Next` — with one visited set, truncating at the first revisit with a warning. An
absent `/Outlines` is an empty `Vec`, not an error: `outline.rs` is explicit that
"most documents have no outline" is an ordinary answer.

`Page::links()` filters `/Annots` to `/Subtype /Link` only, normalizes each `/Rect`,
and builds the same `Target`. Everything else in `/Annots` is invisible to this
phase, which is the scope fence that keeps annotations out of it.

### Page labels

`/PageLabels` is a number tree keyed by page index; each value is
`{ /S style, /P prefix, /St start }` (12.4.2). The tree is flattened once into
sorted runs; `label(i)` binary-searches for the greatest key ≤ i and formats
`prefix + numeral(style, st + i − key)`. Styles: `D` decimal; `R`/`r` upper/lower
roman; `A`/`a` the letter-repetition style — A…Z, then AA…ZZ, then AAA (27 is "AA",
not spreadsheet base-26, a classic implementation bug our unit vectors pin). No `/S`
means the prefix alone. Leniency: a missing run at key 0 gets an implicit decimal run
with a warning (matching viewer behavior); `/St` < 1 becomes 1 with a warning.

```rust
impl Document {
    pub fn page_labels(&self) -> Option<PageLabels>; // None = feature absent
}

impl PageLabels {
    pub fn label(&self, page_index: u32) -> String;  // "iv", "A-3", "7"
}
```

`Option` at the top because absence is meaningful: a viewer falls back to plain page
numbers, and synthesizing labels for a document that has none would erase that
signal. Tinker's plans/01 M7 wants labels in the page indicator and its MuPDF
binding never exposed them — this is one of the concrete "the rewrite pays for
itself" items, so it is in the Checkpoint A surface, not deferred.

### Embedded files and XMP

The `/Names` → `/EmbeddedFiles` name tree yields file specifications (7.11.3):
display name prefers `/UF` over `/F` (both kept raw), `/Desc` carried, data from the
`/EF` stream decoded lazily through [02-filters](02-filters.md), `/Params` giving
size, dates, and an MD5 `/CheckSum` that we verify with
[03-encryption](03-encryption.md)'s MD5 — a mismatch is a warning, not an error,
because the payload may still be exactly what the user needs to recover.

XMP is the catalog `/Metadata` stream returned as decoded raw bytes. No XML parser,
deliberately: Tinker needs passthrough, [09-writing](09-writing.md) copies bytes
verbatim, and an XMP data model is a large dependency-shaped hole with no consumer.

### Error and leniency policy

Nothing in this phase can panic (ruling 1) and almost nothing here is fatal: fatality
was spent in phase 01 when repair either produced an object graph or didn't. The
ladder for every feature is — feature absent → ordinary empty answer; feature broken
→ best-effort answer plus a typed warning naming the object (ruling 10); feature
hostile (cycles, bomb counts) → bounded truncation plus a warning. The differential
oracles exist precisely because lenient readers fail silently: a flattened outline or
an off-by-one label looks plausible until compared against `mutool show` and
`qpdf --json` output at corpus scale (oracles as subprocesses only, ruling 9).

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Text strings, `/Info`, version, page tree, geometry | Ports of `opens_a_pdf_and_reports_its_shape` and `page_geometry_covers_every_page` pass on `simple-text.pdf` (count 3, `"PDF 1.7"`, A4 within a point, indices in order). PDFDocEncoding table matches Annex D entry-by-entry. Inheritance fixture (boxes/rotation defined only at the root) reads correctly at the leaf. `fuzz_page_tree` runs cyclic `/Kids` and `/Count` = 2³¹ inputs with bounded time and memory, zero panics. | S |
| 2 | Name/number trees, named destinations, page labels | Tree lookups find every key the exhaustive scan finds on fixtures with lying `/Limits`, unsorted `/Names`, and odd-length arrays. Label vectors for D/R/r/A/a including the 27→"AA" repetition case and `/St` offsets. Labels agree with `qpdf --json` page labels across the corpus. Name-tree and old-style `/Dests` fixtures resolve to identical page indices. | S |
| 3 | Outline, destination model, actions, links | Port of `outline.rs` passes: 3-level nesting on `outline-3level.pdf`, zero-based indices, empty outline as ordinary answer. Outline title/page pairs agree with `mutool show <f> outline` across the corpus; every disagreement is triaged to a filed bug (ours or theirs). A URI outline entry reads back as `Destination::Uri` — the limitation-#6 pin, read side. Link fixture yields correct rects and page indices for GoTo, URI, and Named targets. | S |
| 4 | Embedded files, XMP passthrough, parity gate | Attachment names, sizes, and checksums match `qpdf --json` across the corpus; extracted bytes hash-equal to the files embedded at fixture build. XMP bytes byte-identical to the source stream. Full ports of `open_documents.rs` + `outline.rs` green on all four fixtures (`simple-text`, `encrypted-aes256`, `permissions-noprint`, `outline-3level`), landing in `tinker_parity.rs` per ruling 12. `oracle-diff` outline job wired into nightly CI. | S |

## Dependencies

Needs [01-cos-and-object-model](01-cos-and-object-model.md) (object graph, resolver,
repair, the warning sink), [02-filters](02-filters.md) wave 1 (stream decode for XMP
and attachment data), and [03-encryption](03-encryption.md) (two of the four parity
fixtures are encrypted; string and stream decryption must already be transparent at
the object layer).

Unblocks [06-content-and-text](06-content-and-text.md) — the interpreter starts from
`Page::resources()` and the `/Contents` chain resolved here — and with it
Checkpoint A ([PLAN.md](../PLAN.md)). Gives [09-writing](09-writing.md) its
round-trip targets (destination enum, outline model, labels, filespecs) and
[15-tinker-integration](15-tinker-integration.md) a surface that maps 1:1 onto
Tinker's `DocumentInfo`, `PageGeometry`, and outline DTOs.

## Risks

| Risk | Mitigation |
| --- | --- |
| Page-tree pathology (kid cycles, hostile `/Count`, deep nesting) turns open into a hang or OOM | Visited sets on every walk, `/Count` clamped to xref size, iterative traversal with heap stacks, dedicated `fuzz_page_tree` target; a fuzz crash is a release blocker (ruling 1) |
| Lying `/Limits` or unsorted `/Names` makes binary descent silently miss keys — named dests and attachments vanish without an error | First inconsistency degrades that lookup to exhaustive scan with a warning; corpus differential vs `qpdf --json` catches residual misses at scale |
| Destination corner forms (dict-with-`/D`, integer page targets, dangling refs, mixed old/new `/Dests`) resolve to wrong pages and poison outline parity | Corner-case fixture set built up front; `mutool show outline` differential across the corpus with triage-every-disagreement discipline, not a tolerance percentage that hides a class of bug |
| Text-string decode errors (PDFDocEncoding table slips, surrogate handling) corrupt titles, labels and filenames everywhere at once | The table is tested against Annex D entry-by-entry; UTF-16 fixtures include lone surrogates; the helper is written once, in one place, before anything consumes it |
| Scope creep: this phase touches `/Annots` and filespecs, both doors into annotations and forms | Hard fence: `/Subtype /Link` only, `/EmbeddedFiles` tree only; everything else named in Non-goals with its owning phase, and [99-consistency](99-consistency.md) rulings outrank enthusiasm |
| "Version string parity" is ambiguous on repaired or version-less files | The policy is written down here (catalog override when later; 1.7 baseline + warning when unreadable) and the parity test pins the four fixtures, so the ambiguity has one recorded answer instead of drift |

---

Rulings 1, 6, 10 and 12 in [99-consistency](99-consistency.md) bind this phase; the
master map is [PLAN.md](../PLAN.md).
