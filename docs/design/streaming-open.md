# Streaming open

When this is done, a caller holding a byte *source* rather than a byte
*buffer* — a memory map, a file handle, an HTTP server answering range
requests (RFC 9110) — will open a document and render page one before most
of the file has been read, with a byte counter proving it: for a linearized
file (ISO 32000-1 Annex F) not one read touches the tail. The
roadmap named this gap and its unusual advantage (the item has since closed,
and the hint-table gap below is what the [roadmap](../ROADMAP.md) still
carries): the
reader's spec-complete counterpart is already in-tree, because the writer
produces Annex F output whose hint tables are read back bit-for-bit by this
repository's own tests. The reader is also what closes an old gap in that
claim: hint tables were once bit-packed structures this repository wrote and
nothing here read, wrong in five ways with every test green — a production
reader over *linearized files this project did not write* is the first
in-tree check that could have caught it. What is missing is
a source abstraction, an open path that discovers structure incrementally,
and honesty — typed, per [rulings](../rulings.md) 2 and 10 — about the
operations that still need every byte.

## Scope

- A `ByteSource` trait in `tinker-pdf-cos` (new `source.rs`, re-exported by
  the facade per ruling 11): length plus ranged reads, synchronous, with a
  typed miss. The engine defines the trait; hosts implement transport, the
  [`FontProvider`](../features/opening.md) pattern from
  `crates/tinker-pdf/src/fonts.rs` applied one layer down.
- `SliceSource`: today's `Arc<[u8]>` contract as the degenerate source, so
  `Document::open(bytes)` keeps its exact signature and behaviour
  (`tinker_parity.rs` compares it, ruling 12).
- Incremental discovery: header window, `startxref` tail window, xref chain
  walked section-by-section — never the whole file for an undamaged one.
- The linearized fast path: read the hint tables with
  `validate::hints`, the production decoder that already exists, so page one
  costs only head-of-file reads. **Amended August 2026** — see milestone 3.
- The lazy store re-keyed on ranges: `CosDocument`'s loads fetch bounded
  windows instead of indexing one whole buffer.
- Honest degradation: every whole-file dependency (repair rescan, container
  formats, incremental save) declares itself with a typed warning before
  fetching everything.
- A wasm strategy that works on `wasm32-unknown-unknown` without an async
  runtime.

## Non-goals

- **No transport in the engine.** No HTTP client, no file I/O, no mmap call
  anywhere in the workspace — hosts own transport, exactly as they own
  fonts. Zero third-party logic crates stays true; nothing new is linked.
- **No async.** Argued below; the trait is synchronous forever.
- **No streaming write.** `DocumentEditor`'s incremental save requires the
  original bytes verbatim as its prefix (`CosDocument::bytes` says why:
  signatures); a save on a streamed document fetches all, and says so.
- **No prefetch heuristics.** Speculative read-ahead is a host policy; the
  engine asks for exactly the ranges it needs and the counter stays honest.
- **No progressive rendering below page granularity.**
- **No container streaming.** A ZIP's central directory is at its end and
  cbz/xps/epub synthesis re-writes the whole document; `Document::open`'s
  container path declares whole-file and fetches all.

## Design

### One trait, three implementations

```rust
pub trait ByteSource: Send + Sync {
    fn len(&self) -> u64;
    /// The bytes of `range`, or a typed miss naming the range needed.
    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss>;
}
```

`SourceMiss` carries the range; it is a typed refusal in the sense of
rulings 2 and 10, not an error string. Implementations: `SliceSource`
(wraps the `Arc<[u8]>` every caller passes today; never misses),
caller-owned mmap (native facade helper; never misses), caller-owned HTTP
range client (host code in `bindings/`; misses until the host fetches).
`doc.rs`'s opening comment — no file handles and no I/O traits, because
wasm has neither files nor mmap — survives intact: this is not an I/O
trait. It performs no transport, blocks on nothing, and on wasm is backed
by memory the host has already fed.

### Discovery without the whole file

`CosDocument::open` today runs on `&buffer` end to end. The streaming open
reorders nothing and re-windows everything, reusing the exact functions:

1. **Head window.** One read of the first 4 KiB covers `xref::header_shift`
   (`limits::MAX_HEADER_SCAN`, 7.5.2) and the linearization sniff below.
2. **Tail window.** `xref::startxref` already probes `STARTXREF_SCAN`
   (1 KiB) then `STARTXREF_SCAN_MAX` (64 KiB) from the end — two ranged
   reads, unchanged in shape.
3. **Chain walk.** `xref::build` follows `/Prev` (7.5.4, 7.5.8; bounded by
   `limits::MAX_XREF_CHAIN`) fetching each section as a window: a classic
   table by chunk-doubling until `trailer` parses (7.5.5), an xref stream
   by parsing its dictionary then fetching its extent. Revisions (7.5.6)
   merge exactly as now.

The eager `validate()` pass — every type-1 offset probed against its
`N G obj` header — is the one open-time step that touches everywhere. On a
streamed source it is deferred: `parse_at` already re-checks every header
at load, so a lying entry is still caught at first use. The cost is a
semantic one, handled under determinism below.

### The linearized fast path

If the head window contains a `/Linearized` parameter dictionary whose `/L`
equals `source.len()` — Annex F's own rule for detecting that a linearized
file was incrementally updated and must be read as ordinary — the fast path
engages: parse the first-page xref table and trailer sitting in the head,
fetch the hint stream at `/H` (offset, length), and hand it to
`linearize::hints`. That module is the test reader promoted, not new logic:
the page-offset table (F.4.1) yields each page's object run and byte span,
the shared-object table the groups pages share. Page one's objects all lie
below `/E`, so rendering it is head reads only; `/Encrypt`, when present,
is object 3 in the head by this writer's own layout, so authentication
needs no tail either. The main table at `/T` is fetched only when a read
leaves page one. Hints are attacker-controlled bytes: they are an
accelerator, never an authority — every object still passes `parse_at`'s
header check, and a hint that lies falls back to the generic path with a
typed warning, never a panic (ruling 1).

### What changes in the store

`SlotStore`, `ResolveCtx` and the compare-and-swap publish discipline in
`store.rs` — and `ObjStmCache` in `objstm.rs` — are untouched — the store is already lazy; what
changes is what it is lazy *over*. `CosDocument.buffer: Arc<[u8]>` becomes
a `Backing`: whole buffer (today's contract, zero new cost) or a chunk
cache over a `ByteSource` — fixed-granularity aligned chunks, each fetched
once and kept as `Arc<[u8]>`, so repeated small reads coalesce and the byte
counter measures policy, not luck. Every internal `&self.buffer[..]` read
goes through a window accessor; the parse layer (`parse_indirect_at`,
`lexer.rs`, `streams::resolve_extent`) keeps taking `&[u8]` — bytes in,
values out, per ruling 8. A miss never publishes: no `Object::Null` enters
the store because a range was absent, or a retry would deterministically
read a wrong value the first arrival baked in.

### What stays whole-file, and says so

- **Repair.** `ScanIndex::build` is one forward pass over everything; that
  is its point. A streamed document that needs ladder level 2 or 3 fetches
  the whole source first and emits a new typed `WarningKind` variant
  (`WholeFileFetched`, provenance per ruling 10) alongside the existing
  `DocumentRescanned`.
- **Eager validation.** `ladder_level()` on a streamed document reflects
  the bytes read so far; a new completion call fetches and validates all,
  giving the eager answer. Both are documented observables, not moods.
- **`bytes()`**, incremental save, signatures, and a `WriteOptions::linearize`
  re-save: whole-file by contract, declared the same way.
- **Containers** (`cbz::container` sniff): whole-file, declared.

### wasm: the trait is synchronous, and why

An async trait was considered and rejected. It would need a runtime the
workspace forbids, would color every function from `load` to `Device`, and
would make output timing-dependent, which ruling 4 exists to prevent. The
sync-plus-typed-miss shape gives wasm the same engine: the JS host
(`bindings/js`) implements `ByteSource` over memory it has fetched; a read
the host has not fed yet returns `SourceMiss`, which propagates up — never
converted into a value — and surfaces at the facade as a typed "need these
ranges" result. The host awaits its own `fetch`, feeds the chunk cache,
and calls again; because parsing is pure and the store caches, the retry
repeats no completed work. Native hosts back the source with mmap or a
blocking file read and simply never miss. One engine, two host loops.

### Determinism: arrival is not an input

Ruling 4's contract extends, not bends: the same bytes and the same query
sequence produce the same values, warnings, and fingerprints regardless of
chunking, ordering, or how many misses occurred on the way. The existing
warning contract already allows warnings to *arrive* lazily after open
(`CosDocument::warnings` documents this); what streaming adds is proven by
running the 15 committed fingerprints in
`crates/tinker-pdf/tests/determinism.rs` over a `ShreddedSource` that
splits every read adversarially, and asserting bit-identical output
against `SliceSource`.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|---|---|---|
| 1 | `ByteSource`, `SliceSource`, `CountingSource`, `ShreddedSource`; `Backing` behind `CosDocument` | Full workspace suite green with `SliceSource`; `determinism.rs` fingerprints byte-identical over `ShreddedSource`; no public-signature change flagged by `tinker_parity.rs` | M |
| 2 | Incremental tail-first discovery (generic path) | New `streaming_open.rs` test: a multi-megabyte committed fixture opens and reads one mid-file object with `CountingSource` total under a committed byte budget (ratchet-style number, `--check`ed like `corpus/ratchet.json`) | M |
| 3 | One hint-table reader: `validate::hints`, re-pointed and widened | Existing round-trip tests re-pointed at the production module and the test-only reader in `linearize.rs` deleted; the reader parses the hint tables of every already-linearized file in the fetched qpdf corpus — files this project did not write — and the count parsed is asserted so a shrinking set cannot read as a passing one | S |
| 4 | Linearized fast path + `Document::open_streaming` + page-one render | Test renders page one of (a) a writer-linearized fixture and (b) a linearized file from the fetched qpdf corpus, asserting via `CountingSource` that **zero read ranges intersect the tail** past `/E` and total bytes stay under budget; `/L` mismatch provably falls back to the generic path | L |
| 5 | Honest degradation + wasm host loop | Damaged fixture on a streamed source reaches `LadderLevel::Rescan` with `WholeFileFetched` warned; `hostile_input.rs` sweep runs over `ShreddedSource` with zero panics; `bindings/js` demo feeds ranges and draws page one, checked in the existing wasm CI job shape | M |

## Amendment, August 2026: milestone 3 is one reader, not a promotion

The row above said the hint-table reader was test-only, living in
`linearize.rs`'s test module, and had to be promoted into a production
`linearize::hints`. That was stale when it was written. A production decoder
already existed at `crates/tinker-pdf-cos/src/validate/hints.rs` — bounds
checked before each field is consumed, `width > 32` refused, entry counts
guarded — already consumed from `validate.rs` and already run against
qpdf-corpus files this project did not write. Promoting a third reader into
existence would have left the tree with three readers for one format.

So milestone 3 is reuse and re-point. `validate::hints` becomes crate-visible,
the test-only reader in `linearize.rs` is deleted, and the round-trip tests
that drove it drive the production decoder instead. Making one reader carry
both jobs is what surfaced the disagreement between them: Table F.4 item 5,
the fractional-position numerator, is written once per shared object
*reference* and the production decoder read one per *page*. Both readings
agree on every linearized file in the qpdf corpus, because all of them state
item 12 as zero bits — which is exactly why a second reader could disagree
for as long as it existed without a file noticing, and why the discriminating
case is now a hand-packed table in the decoder's own tests.

## As built, August 2026

All five milestones landed. What differs from the design above is written here
rather than quietly, because a design doc that describes a plan the code did
not follow is worse than no design doc.

### The fast path reads `/O` and the first-page section, not the hint tables

The design said page one would be found through the page-offset hint table.
It is found through Annex F part 3 -- the first-page cross-reference section,
which sits in the head and is parsed by the same walker every other section
goes through -- and through `/O`, which names the first page's object number
outright (F.2.2 item 3).

`/O` turned out not to be an optimisation but a requirement, and the corpus is
what said so. **A linearized file is free to leave the page tree root in the
tail, and qpdf's linearizer does**: `lin1.pdf`'s first-page section covers
objects 60 to 78, its catalogue is 61, and its page tree root is not in the
head at all. A page-one render that insisted on walking the tree fetched the
end of every such file to draw the front of it. So `pages::at(doc, 0)` builds
the page from `/O` with nothing inherited -- by the same function the tree walk
uses -- and declines, letting the walk run, when the page does not carry its
own `/MediaBox` and `/Resources`. A page laid out at US Letter because its
ancestor was not fetched is the wrong page, not a cheaper one.

The hint tables are read and checked by `validate::hints` and by its corpus
sweep, and they are **not** on the open path. Nothing in the head-only open
consults them, which is a gap rather than a decision: they would be what finds
pages *other* than page one without the main table, and that is not built.

### Measured

| What | Fixture | Bytes |
|---|---|---|
| Open, generic path | generated, 120 pages, 4,891,065 bytes | 13,753 |
| Open plus one mid-file object | the same | 67,001 |
| Page-one render, linearized | generated, 60 pages, 1,631,095 bytes, `/E` 28,224 | 29,696 |

43 of the qpdf corpus's 45 already-linearized files open from their heads and
render page one; `badlin1.pdf` is the only one whose render reaches past `/E`,
and it is qpdf's deliberately damaged linearization fixture, whose declared
`/E` is not where its first page ends.

### Three things that had to shrink

Every one of them was a window bigger than the head of a small file, and none
of them showed on a fixture this repository wrote.

- The `%%EOF` search that ends a revision. A linearized file's only `%%EOF` is
  at the very back, so scanning forward from a first-page section at the front
  read everything. The head path is told its revision is the document.
- The first-page section window, from 8 KiB to 1 KiB, and the object window,
  from 4 KiB to 1 KiB. On a 13 KB file whose `/E` is 1,827 both pulled chunks
  of the tail to parse the front.
- A read that begins inside the first page is clamped to `/E`. An object that
  ends there needs no byte past it, but a window sized by a guess asks for
  more, and on a fixed granularity that guess costs a whole chunk of tail. Only
  a parse that comes up short is allowed through, so the ceiling changes which
  bytes are fetched first and never which are readable.

### What a streamed document does differently, and says so

- `ladder_level()` is **provisional** until `complete_validation()`, which
  fetches the source and runs the eager offset probe the open deferred.
- The repair index is built at the first read that finds an entry lying, rather
  than at open. The *values* are the same on both paths -- that is asserted --
  and the fetch is declared with `WarningKind::WholeFileFetched`.
- `Revision::byte_range.end` is the document's end rather than the byte past
  `%%EOF` when the marker is more than 8 KiB past the section, which on the
  head path it always is.
- `xref()` fetches the main table, because asking for the table is asking about
  every object.

## Dependencies

- The Annex F writer and its tests — `linearize.rs`,
  `tests/linearized.rs` — all landed; milestone 3 is a deletion and a
  re-point, not an implementation.
- The determinism suite and its fingerprints
  ([verification.md](../verification.md)) as the arrival-independence bar.
- The qpdf corpus (637 files, fetched via `xtask corpus-fetch`) as the
  source of linearized files this project did not write — inputs, which
  ruling 13 keeps — for milestones 3 and 4.
- No new crates and no external programs (ruling 13).

## Risks

| Risk | Mitigation |
|---|---|
| A miss half-poisons state (a `Null` cached because bytes were absent) | Invariant stated and tested: `SourceMiss` propagates, never converts to a value; the store publishes only completed parses; a fault-injection test drops every Nth read and asserts the retry converges to the `SliceSource` answer |
| Hint tables lie (attacker-controlled) and misdirect reads | Hints accelerate, never decide: `parse_at`'s header check stays mandatory; a failed check falls back to the generic path with a typed warning; all digit/width fields bounds-checked as `limits.rs` does elsewhere |
| Deferred validation makes `ladder_level()` access-pattern-dependent | The provisional/completed split is explicit API, both documented; the completion call restores the eager answer, and a test asserts provisional-then-complete equals whole-buffer open |
| Byte budgets rot as the open path evolves | Budgets are committed numbers checked in CI like `corpus/ratchet.json`, moved only by a reviewed `--record` |
| Chunk granularity choices leak into output | Chunking affects only which ranges are fetched, never values; the `ShreddedSource` fingerprint run is the regression trap |
| wasm retry loops livelock on a host that never feeds the range | The miss names its exact range; the facade result is a finite set of needs per call, and a test drives the loop to completion in a bounded number of host round trips |
