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
  *No longer a non-goal, 9 October 2026 (ruling 3 as amended)*: a streaming writer is
  [ROADMAP](../ROADMAP.md) row ED-38.
- **No prefetch heuristics.** Speculative read-ahead is a host policy; the
  engine asks for exactly the ranges it needs and the counter stays honest.
  *Narrowed, 9 October 2026*: offering a linearized file's hint tables to the
  host as prefetch ranges is [ROADMAP](../ROADMAP.md) row FI-32; the policy
  stays the host's.
- **No progressive rendering below page granularity.** *No longer a
  non-goal, 9 October 2026 (ruling 3 as amended)*: [ROADMAP](../ROADMAP.md) row RC-15.
- **No container streaming.** A ZIP's central directory is at its end and
  cbz/xps/epub synthesis re-writes the whole document; `Document::open`'s
  container path declares whole-file and fetches all.
  *No longer a non-goal, 9 October 2026 (ruling 3 as amended)*: [ROADMAP](../ROADMAP.md) row FI-32.

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
is numbered inside the head group by this writer's own layout — directly
after the parameter dictionary — so the front section places it and
authentication needs no tail either. The main table at `/T` is fetched only when a read
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
outright (F.3.3, Table F.1).

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

### Page *N* is found through the hint tables, and only their ranges are believed

*September 2026.* The paragraph that stood here said the hint tables were read
by `validate::hints` and its corpus sweep and were **not** on the open path,
which made every page but the first cost the main cross-reference table. They
are on it now.

What the tables supply is a **byte range per page** and nothing else. Table F.4
item 2 places a page by accumulating the lengths of the pages before it and
says a reader "shall skip over the primary hint stream, wherever it is
located"; F.4's own rule is that every position a hint table states is written
as though that stream were not in the file, so one at or past its offset gets
its length added back. Table F.4 item 1 makes the page's own page object the
first object of its run. So `CosDocument::hinted_page_object` computes the
range, fetches it, and reads the `N G obj` headers *inside* it — the same
statement the repair scanner believes over a whole document at ladder level 3
— and records where each object was found. Every one of those offsets is
checked again by `parse_at` when the object loads.

That is what "hints accelerate, never decide" comes to in code, and it is
stronger than a check after the fact: **almost no value comes from a hint at
all.** A table that names the wrong bytes costs a window nobody uses. Three
things are checked anyway, and the third is the one that matters —

- Table F.3 item 2, the first page's location, must be the offset the
  first-page section gives for the object `/O` names, which is the other route
  Table F.4 item 2 offers. Compared through `xref::offset_candidates` rather
  than by equality, because both numbers are offsets *the file* stated and
  7.5.2 measures those from `%PDF-`;
- the leading object of a page's run must be a page leaf carrying its own
  `/MediaBox` and `/Resources`, by the same rule `/O` is held to;
- **each run must begin strictly after the one before it.** This is the only
  hint here that can change an *answer* rather than a cost, and a zero in the
  counted injection campaign is what found it — the question "what would make
  the leaf test catch something" had an answer nobody wanted: a table stating
  page one's length as
  zero puts page two's run at page one's page object — a page leaf with its
  own `/MediaBox` and `/Resources`, so every other check passes it — and
  `page(1)` comes back holding page one's content under index 1. Silently,
  cheaply, and wrong.

A file that fails any of them gets `WarningKind::LinearizedHintsUnusable` or
`LinearizedPageHintRejected` and the page tree walk, which fetches the main
table and is never wrong, only slower.

The first form of that third check also asked for a page after the first to
begin at or past `/E`, and it **refused eight corpus files that are perfectly
well linearized.** Table F.3 item 4 measures a page's length from its page
object to the last byte of the last object it *uses*, while `/E` ends part 6,
and F.3.5 puts things in part 6 the first page does not use — an outline
hierarchy, when the catalogue asks for one. The two are not the same number,
and a check built on their being equal measured this reader's assumption
rather than the file.

Object *numbers* are deliberately not derived. F.3.1 numbers the remaining
pages' objects from 1 and the first page's after them, and Table F.4 item 1
repeats the rule — but a reader that derived numbers would be believing a hint,
and this one reads them off the file's own headers instead. That was not a
theoretical preference while this project's own linearizer numbered the two
groups the other way round; it numbers them F.3.1's way as of 20 September
2026, and the reader is unchanged, because it never asked. What does derive
the numbers, and says so, is
`crates/tinker-pdf-cos/tests/linearized_numbering.rs` and
`validate/hints.rs`'s `annex_f_numbering` — a test rather than a read path,
which is the whole distinction.

The shared object hint table comes along for the same ride. Table F.6 item 1
places part 8's groups by accumulating from Table F.5 item 2, and Table F.4
item 4 says which of them a page names, so a font or a colour space several
pages share is fetched with the page that needs it rather than through the
table at `/T`.

### Measured

| What | Fixture | Bytes |
|---|---|---|
| Open, generic path | generated, 120 pages, 4,891,065 bytes | 13,753 |
| Open plus one mid-file object | the same | 67,001 |
| Page-one render, linearized | generated, 60 pages, 1,631,075 bytes, `/E` 28,222 | 29,696 |
| Page-31 render, same document untouched | the same | 37,888 |
| Page 31 after page one, marginal | the same | 32,768 |

43 of the qpdf corpus's 45 already-linearized files open from their heads and
render page one; `badlin1.pdf` is the only one whose render reaches past `/E`,
and it is qpdf's deliberately damaged linearization fixture, whose declared
`/E` is not where its first page ends.

**Page one is compared now rather than counted**, which `/O` had never been
held to: 41 of the 43 draw the page the page tree walk draws, and the two that
do not are two the walk cannot answer for at all. Both are sealed under a
password the sweep does not have, and the difference is which route survives
that — `/O` names the object outright and a page dictionary is structure rather
than content, so the head path builds a page and draws whatever its content
stream decrypts to, which is nothing; the walk cannot start, because the
catalogue is inside an object stream and an object stream is a stream. Named in
`PAGE_ONE_ONLY_FROM_THE_HEAD` rather than excluded, because it is a real
difference between two routes to the same page.

**29 of those 43 have a second page to ask for, and every one of them draws the
page the main table draws** — compared bitmap against bitmap, because a cheap
render of the wrong page passes every byte budget there is. **20 of the 29
reach it without the main table at all.** The nine that do not are two families
and no others, named in `streaming_open.rs`: five are encrypted under a
password the sweep does not have, so their hint stream is ciphertext and
refusing to read it is the right answer, and four are qpdf's damaged
linearization fixtures, three of which `validate::hints` already refuses by
name. A sealed document that is then given its password reads its tables at
once — `install_security` drops the refusal with everything else that was read
as plaintext out of ciphertext.

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
  every object. So does `page_count()`, for the same reason: 7.7.3.2's `/Count`
  is a claim like any other and the walk that checks it is a walk over every
  page. `/N` in the parameter dictionary would answer it from the head, and is
  not used for it — a page count taken from a hint would be a number this
  reader had checked nothing about.
- `main_table_fetched()` reports whether that has happened, which is the
  observable the byte budgets for page *N* are written against.

### Bytes before `%PDF-`, and what is still owed

7.5.2 lets a file begin with something else, and then every offset it states is
measured from the header rather than from byte zero. Table F.1 says `/L` is
"the length of the entire file", so such a file states a length that *includes*
the prefix while `/H`, `/O`, `/E`, `/T` and every position in the hint tables do
not. The hint path holds both readings: positions it derives are shifted, and
the two cross-checks above compare through `xref::offset_candidates` rather than
by equality, so a file that agrees with itself is not called a file that does
not.

What is **not** fixed is underneath it. The first-page cross-reference section
states its entries from the header too, and the pass that reconciles those with
the file's own `N G obj` headers is the eager `validate()` a streamed open
defers — so on such a file the objects that section places are found by the
repair scanner instead, which fetches everything and warns `WholeFileFetched`.
Page *N* is drawn from its head regardless, because its objects come from the
hint tables. `a_linearized_file_behind_leading_junk_still_opens_from_its_head`
pins both halves, and the fetched corpus says nothing either way: its one file
with leading junk states `/L` *without* the junk, which Table F.1 makes a
mismatch and therefore an ordinary PDF.

## Time to first page

*25 October 2026, for [ROADMAP](../ROADMAP.md) row MS-08.* The budgets above
say how many bytes page one costs. They do not say how long it takes over a
link where every request waits for its round trip, and on such a link it is
requests rather than bytes that cost: at 50 ms and 10 Mbit/s one round trip is
worth 62,500 bytes of transfer, and the whole of page one of the linearized
fixture, 29,696 bytes, is under 24 ms of it. MS-08 counts requests, bounds them
for page one with and without linearization, and turns the counts into a time.
This section fixes what is counted before anything is built on the count.

### Scope

- **What a request is:** one call to `ByteSource::read` that reaches the source
  a document was opened over, which is what `CountingSource::reads()` counts
  (`crates/tinker-pdf-cos/src/source.rs:190-194`).
- **One read per run of missing chunks** (MS-08.2): `ChunkCache::window`
  fetches each run of the window's uncached chunks with one read, where today
  it reads them a chunk at a time.
- **The linearized bound:** page one of the 60-page linearized fixture
  (`a_linearized_document`, `crates/tinker-pdf/tests/streaming_open.rs:239-272`)
  in at most 29,696 bytes and 6 requests, the main cross-reference table never
  fetched.
- **The unlinearized bound:** the same pages saved without linearization, page
  one in at most 12 requests and in no more bytes than three byte ranges -- its
  tail, its cross-reference section, page one's objects -- plus 4,096 a
  request, the ranges derived from a buffered open of the same bytes.
- **A bench row** (MS-08.4) whose time is the measured work plus a link time
  computed from those counts.
- **The container sniff:** whether it stays a request of its own. It does.

### Non-goals

- **No prefetch, still.** The non-goal above stands. A coalesced read is
  bounded by the window it serves, so it fetches exactly the chunks today's
  chunk-at-a-time loop fetches, in fewer requests; nothing is read because it
  might be wanted next.
- **No new granularity, and no byte budget moves.** `CHUNK_SIZE` stays 4,096
  (`source.rs:288-296`), so every byte figure above stands. MS-08 moves request
  counts and no byte count.
- **Nothing sleeps.** A slept latency would time the runner's timer and its
  scheduler, and the row asks for a time its counts determine.
- **Not the host loop's rounds.** A host that misses and feeds, as the wasm
  loop does, pays a round trip per round rather than per read, and
  `a_host_that_feeds_what_it_was_asked_for_converges_in_bounded_rounds`
  (`crates/tinker-pdf-cos/tests/streaming.rs:325-356`) bounds its rounds at 16.
  That bound stays where it is. A coalesced miss names its whole run, so the
  loop may converge in fewer rounds; nothing here asserts that it does.
- **No concurrency in the model.** The trait is synchronous
  (`source.rs:13-23`): a read is answered before the next is asked, so the
  model adds every latency. A host that overlaps its requests would beat the
  model, and is not credited for it.
- **Page one of generated fixtures.** Page *N* keeps its byte budgets and gains
  no request bound, and the qpdf corpus sweeps keep their assertions and gain no
  count.

### Design

#### What a request is

`CountingSource` is the outermost source. The guard opens over
`CountingSource::new(SliceSource::new(bytes))`, as
`a_linearized_file_renders_page_one_without_touching_its_tail` already does
(`streaming_open.rs:337-338`), so every read anything in the engine makes is one
the counter sees. Two readers make them, and only two: the chunk cache, whose
one read is in `ChunkCache::chunk` (`source.rs:424`), and the facade's `filled`
(`crates/tinker-pdf/src/lib.rs:1313-1326`), through which the facade sniffs for
a container (`lib.rs:1529`). A file whose first kilobyte holds no `%PDF-` adds
the standalone sniff's wider read (`lib.rs:1538-1541`); neither fixture does.

`SliceSource` answers every range whole (`source.rs:144-146`), so over it one
read is one range a host would ask its transport for -- an HTTP range request,
a positioned read. That is the request MS-08 counts, and three things follow
from it.

- A source that answers short is asked again by the loops that read it
  (`source.rs:423-433`, `lib.rs:1317-1324`), and every ask is a request. That
  count is honest about such a source and is not the guard's: over
  `ShreddedSource` a chunk is 4,096 requests by design.
- A read that misses is not recorded. `CountingSource::read` returns the miss
  before it records anything (`source.rs:246-255`); a miss is the host loop's
  business, and the guard's source never misses.
- `bytes_read()` stays gross (`source.rs:196-209`): a byte fetched twice is
  charged twice. On these fixtures that happens once, to the sniff's kilobyte,
  which chunk 0 fetches again.

#### One read per run of missing chunks

`ChunkCache::window` (`source.rs:446-463`) takes the chunks from `first` to
`last` (`:452-453`) and asks `ChunkCache::chunk` (`:409-444`) for each in turn,
and `chunk` fetches a missing one with a read of its own, so a window over *k*
missing chunks is *k* requests. MS-08.2 makes it one request a run:

1. the window's chunks that are not yet cached are grouped into maximal runs of
   consecutive indices, between `first` and `last` and never outside them;
2. each run is one read, from its first chunk's start to its last chunk's end,
   clamped to the document, looping on a short answer as `chunk` does;
3. what arrives is cut at chunk boundaries and published a whole chunk at a
   time, so a miss part-way through a run publishes the whole chunks before it,
   as today's loop does, and never a part-filled one -- the invariant
   `source.rs:316-319` already states for one chunk.

A cached chunk inside a window splits it into two runs and two reads; a window
with nothing missing is no read. **A run never reaches past its window**, and
that is what keeps the no-prefetch non-goal true: the chunks a coalesced read
fetches are the chunks today's loop fetches, so the bytes and every value are
unchanged and only the number of requests moves. A window that asks for more
than its object needs -- a section's 8 KiB first window
(`crates/tinker-pdf-cos/src/limits.rs:148-155`), an object's 1 KiB
(`limits.rs:112-125`) -- costs the chunks it costs today. Shrinking a window is
a different change, and not this row's.

#### The container sniff stays a request of its own

The facade reads the first 1,024 bytes (`CONTAINER_SNIFF`, `lib.rs:200-206`)
straight from the source before it knows what it is opening (`lib.rs:1529`): a
container or a standalone file, either of which is then fetched whole
(`lib.rs:1542-1549`), or a PDF. Only a PDF gets a chunk cache, built inside
`CosDocument::open_source` (`crates/tinker-pdf-cos/src/doc.rs:387-389`, called
at `lib.rs:1551`), and its head window then fetches chunk 0 (`doc.rs:399`) --
the sniff's kilobyte again. The sniff stays a request, and both bounds count
it: the linearized page's 29,696 bytes are 7 x 4,096 + 1,024, and its 6
requests include the sniff.

What it costs is one round trip and 1,024 bytes on every streamed PDF open: on
the bench's link 50 ms, and after MS-08.2 one of the linearized page's three
requests. Serving chunk 0 from the sniff -- the facade reading a whole chunk and
handing it to the open -- would take that page from three requests to two, and
every budget measured from an open down by 1,024 bytes. That is a facade change
that moves committed budgets, which this row was cut to leave alone, so it is
named here rather than taken.

#### The linearized bound

By the code's arithmetic page one costs eight requests today: the sniff, then
chunks 0 to 6 a read each. The head window is chunk 0, and the page's content
stream is read by one window over its declared length
(`crates/tinker-pdf-cos/src/streams.rs:457-509`, sized at `:464-468`), which
ends in chunk 6, the chunk `/E` at 28,222 falls in. After MS-08.2 that window's
six missing chunks are one read, and the page costs three: the sniff, the head,
the content. A probe counted the eight when this section was written, and a
prototype of the coalescing, applied and then removed, counted the three and
read the same 29,696 bytes.

The guard holds the page to the row's figures: at most 29,696 bytes and 6
requests, `main_table_fetched()` false (`doc.rs:670-672`), and no read touching
the table at `/T`.

#### The unlinearized bound, and its three terms

The twin is `a_linearized_document`'s pages saved with `linearize: false` and
`object_streams: false`, so the two files differ only in layout. Its bound is

```text
reads() <= 12
bytes_read() <= |T ∪ X ∪ P| + 4,096 x reads()
```

where T, X and P are byte ranges the test derives from a buffered open of the
same bytes (`CosDocument::open`, `doc.rs:370-372`). They are never typed in, so
a writer change that moves the twin's layout moves its bound with it.

- **T, the `startxref` probe's tail window**: the window `xref::startxref`
  (`crates/tinker-pdf-cos/src/xref.rs:242-273`) finds the keyword in -- the last
  `STARTXREF_SCAN` bytes, 1,024 (`limits.rs:105-106`), or, when the keyword is
  not in them, the last `STARTXREF_SCAN_MAX`, 64 KiB (`limits.rs:108-110`). The
  test finds the keyword in the buffered bytes and takes the window that holds
  it.
- **X, the cross-reference section `startxref` names, through its trailer**:
  from `CosDocument::last_startxref()` (`doc.rs:894-898`), where the `xref`
  keyword is, to the byte after the trailer dictionary's `>>`. That is where
  `parse_object_at` (`crates/tinker-pdf-cos/src/parse.rs:48-60`) stops on the
  dictionary after the `trailer` keyword, the parse the walker itself makes
  (`xref.rs:535-541`). The `startxref` line and the `%%EOF` after it are T's.
  One section, because the twin is one revision.
- **P, page one's objects**, each from its `N G obj` header, at the offset the
  buffered table states, to the end of its `endobj`:
  - the catalog, the trailer's `/Root`;
  - the page tree nodes walked: from the catalog's `/Pages` down to page one,
    the nodes `pages::collect_upto` loads before its limit stops the walk
    (`crates/tinker-pdf-cos/src/pages.rs:279-351`, the limit at `:314-316`);
  - the leaf, page one's page object;
  - its contents: every stream `/Contents` names, and the array too when it is
    an indirect object of its own (`pages.rs:506-522`);
  - its resources: the `/Resources` dictionary when it is an indirect object,
    the leaf's own or one a walked node passes down (`Inherited::extend`
    resolves it, `pages.rs:228-253`), and every indirect object reachable from
    it -- more than a render may read, which loosens the bound and never
    tightens it.

  An object that is not a stream ends where `parse_indirect_at` stops, past
  `endobj` (`parse.rs:113-117`). A stream's parse stops at the end of its
  declared data (`parse.rs:31-39`), and its term runs on through `endstream` to
  the `endobj` after it.

The terms are taken as a union, so a byte two of them hold counts once: on the
twin the tail window begins inside the table. **The allowance is a chunk a
request.** Every read fetches whole aligned chunks, so each carries the bytes
between a term's edge and its chunk's, and the sniff carries the kilobyte that
chunk 0 holds again. It is a claim about the twin's layout rather than a
theorem -- a run whose first and last chunks a term only grazes could carry
nearly two chunks -- and the guard is what checks it.

Measured when this section was written, by a probe the guard replaces: the twin
is 1,630,484 bytes. T is its last 1,024; X is 2,578 bytes from 1,627,881; P is
the catalog (45 bytes), one page tree node of 60 kids (460), the leaf (97) --
its resources an empty dictionary written inline, so no object -- and one
content stream (27,022). The union is 30,227, the 999 bytes the tail window
shares with the table counted once. Page one costs 12 requests and 42,260 bytes
today: the sniff, chunk 0, the two chunks the tail window straddles, and the
content stream's eight chunks a read each. The section and the three small
objects lie in chunks already held. With the prototype it costs 5 requests and
the same bytes -- the sniff, the head, the tail run, the content stream's first
kilobyte (its object window, `doc.rs:1619-1632`, alone in chunk 1) and its
data -- inside a bound of 30,227 + 5 x 4,096 = 50,707. No read carries more than
3,494 bytes outside the terms.

#### The guard

**A ceiling alone does not bite here.** Twelve is what the twin costs on the
code before MS-08.2, to the request, so a test that held it to twelve and
nothing else would pass there, and would stay green with MS-08.2 reverted. So
each of MS-08.3's tests holds `reads()` to the count it measured as well as to
the row's ceiling. The count is committed beside the ceiling the way
`LINEARIZED_PAGE_ONE_BUDGET` commits a measured byte count
(`streaming_open.rs:323-331`), and moves, like it, only by a reviewed change.
The ceilings are the row's promise; the committed counts are what catch a
request added under them, and what make both tests fail against the code before
MS-08.2.

Each prints a `RAN` line with its counts and runs in MS-01's guard suite on
linux, windows, macos and `wasm32-wasip1`. The fixtures draw paths and no text,
so one build.

#### The link time is computed, never slept

`CountingSource::link_time(latency, bits_per_second)` (MS-08.4) is
`reads()` times the latency plus `bytes_read()` times 8 over the bits a second:
a latency a request, because the trait is synchronous and each read is a round
trip that ends before the next begins, and the bandwidth a byte delivered,
gross, as `bytes_read()` counts it. At 10 Mbit/s a byte is exactly 800 ns, so
the bench's figure needs no rounding: the linearized page's ceiling, 6 requests
and 29,696 bytes, is 323.7568 ms at 50 ms a request.

The bench row is tier P's "Time to first page over a source with 50 ms latency
at 10 Mbit/s, the 60-page linearized fixture" (`docs/roadmap/P.md:128`), in
MS-04's open group. It is timed through criterion's custom timing: each
iteration opens the fixture over a fresh `CountingSource`, renders page one,
and reports the time that took plus the link time of that iteration's counts.
The counts do not vary, so the link part is a constant and the row's spread is
the work's.

### Milestones

| Step | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|---|---|---|
| MS-08.2 | One read per run of missing chunks in `ChunkCache::window` | `source.rs` unit tests (new) `a_window_over_missing_chunks_is_one_read` (failing first on today's one read per chunk), `a_cached_chunk_splits_a_window_into_two_reads`, `a_short_reading_source_still_fills_a_coalesced_run` and `a_miss_inside_a_run_publishes_only_whole_chunks`; `streaming_determinism.rs`, `crates/tinker-pdf-cos/tests/streaming.rs` and every byte budget in `streaming_open.rs` pass unchanged | S |
| MS-08.3 | The counted guard, in MS-01's guard suite on four targets | `streaming_open.rs` (new) `a_linearized_first_page_costs_at_most_six_requests_and_never_its_main_table` (at most 29,696 bytes and 6 reads, `main_table_fetched()` false, no read touching the table at `/T`) and `the_same_pages_unlinearized_cost_their_tail_table_and_page_one_in_at_most_twelve_requests` (at most 12 reads; bytes within the three terms plus 4,096 a read), each also held to the read count it measured, committed beside its ceiling, print `RAN` lines, and fail against the code before MS-08.2, where the linearized page costs eight reads and the twin twelve | S |
| MS-08.4 | The link model and the bench row | `source.rs` `the_link_model_charges_latency_per_read_and_bandwidth_per_byte` (new) holds the model to hand-computed figures; `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree` holds the bench file, the workflow's guard count and `baseline.json` to one list with the operation in it; the Measured table above carries the row's figure and the twin's link time from MS-08.3's counts | M |

MS-08.2 follows P-05.3, which changes the same chunk cache; MS-08.3 needs
MS-01.8's guard suite on four targets; MS-08.4 needs MS-04.3's open group.

### Risks

| Risk | Mitigation |
|---|---|
| A miss part-way through a coalesced read publishes a part-filled chunk, and the miss's answer is read back for good | Whole chunks only, the invariant one chunk already keeps; `a_miss_inside_a_run_publishes_only_whole_chunks`, and a counted injection that publishes the part-filled chunk |
| Coalescing grows into read-ahead, and the counter stops measuring what the engine needs | A run is bounded by its window's own chunks, and every byte budget in `streaming_open.rs` holding unchanged is MS-08.2's exit, so a read past a window moves a byte count and fails |
| A ceiling with room under it hides an added request | The measured counts are committed beside the ceilings, and MS-08.2 reverted is a counted injection that must fire both tests |
| The twin's bound is typed in and rots with the writer | Derived from a buffered open of the same bytes on every run |
| The allowance and the committed counts are facts about the fixtures' layout, so a writer change can fail the guard with no engine change | The terms are recomputed on every run, and a failure names each read with the bytes it carried outside them, so a layout change can be told from an engine one; the counts then move by a reviewed change |
| The row's time is mostly link time -- 50 ms a request against milliseconds of work -- so a slower open or render barely moves it | The counts are held exactly in `cargo test`, and the link part of the figure is a constant those counts give, so the work is the figure less it and reads on its own |
| The model flatters a real link: no slow start, no per-request overhead, no server time | The model is stated beside its figure; it compares layouts and changes with each other, and predicts no network |

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
