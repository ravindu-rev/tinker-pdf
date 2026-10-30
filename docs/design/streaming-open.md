# Streaming open

A caller holding a byte *source* rather than a byte *buffer* — a file read at
offsets, an HTTP server answering range requests (RFC 9110) — opens a
document and renders page one before most of the file has been read, with a
byte counter proving it: for a linearized file (ISO 32000-1 Annex F) not one
read touches the tail. The reader's spec-complete counterpart is in-tree: the
writer produces Annex F output whose hint tables `validate::hints` reads back
bit for bit, and the same reader is held to linearized files this project did
not write. The design is a source abstraction, an open path that discovers
structure incrementally, and honesty — typed, per [rulings](../rulings.md) 2
and 10 — about the operations that still need every byte.

## Scope

- A `ByteSource` trait in `tinker-pdf-cos` (`source.rs`, re-exported by
  the facade per ruling 11): length plus ranged reads, synchronous, with a
  typed miss. The engine defines the trait; hosts implement transport, the
  [`FontProvider`](../features/opening.md) pattern from
  `crates/tinker-pdf/src/fonts.rs` applied one layer down.
- `SliceSource`: the `Arc<[u8]>` contract as the degenerate source, so
  `Document::open(bytes)` keeps its exact signature and behaviour
  (`tinker_parity.rs` compares it, ruling 12).
- Incremental discovery: header window, `startxref` tail window, xref chain
  walked section-by-section — never the whole file for an undamaged one.
- The linearized fast path: page one from `/O` and the first-page
  cross-reference section, every later page through the hint tables
  `validate::hints` reads, so page one costs only head-of-file reads.
- The lazy store keyed on ranges: `CosDocument`'s loads fetch bounded
  windows instead of indexing one whole buffer.
- Honest degradation: a whole-file dependency (repair rescan, container
  formats, incremental save) is visible — a repair warns `WholeFileFetched`,
  and `whole_file_fetched()` reports every whole fetch.
- A wasm strategy that works on `wasm32-unknown-unknown` without an async
  runtime.

## Non-goals

- **No transport in the engine crates.** No HTTP client, no file I/O, no mmap
  call in any engine crate — hosts own transport, exactly as they own
  fonts. Zero third-party logic crates stays true; nothing new is linked.
  `tools/tpdf` is a workspace member that reads files, and a file-backed
  source belongs there and in the Python and .NET bindings, never in an
  engine crate (*A file-backed source in the hosts that own files*).
- **No async.** Argued below; the trait is synchronous.
- **No streaming write.** `DocumentEditor`'s incremental save requires the
  original bytes verbatim as its prefix (`CosDocument::bytes` says why:
  signatures); a save on a streamed document fetches all. A streaming writer
  is [ROADMAP](../ROADMAP.md) row ED-38.
- **No prefetch heuristics.** Speculative read-ahead is a host policy; the
  engine asks for exactly the ranges it needs and the counter stays honest.
  Offering a linearized file's hint tables to the host as prefetch ranges is
  [ROADMAP](../ROADMAP.md) row FI-32; the policy stays the host's.
- **No progressive rendering below page granularity** here; it is
  [ROADMAP](../ROADMAP.md) row RC-15.
- **No container streaming.** A ZIP's central directory is at its end and
  cbz/xps/epub synthesis re-writes the whole document; `Document::open`'s
  container path declares whole-file and fetches all. Container streaming is
  [ROADMAP](../ROADMAP.md) row FI-32.

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
(wraps an `Arc<[u8]>`; never misses), a file read at offsets in each host
that owns one (it can miss, since a read that fails is a `SourceMiss`; a
mapping needs `unsafe` and cannot survive a truncated file — see *A
file-backed source in the hosts that own files*), and a host's range client,
such as the JavaScript binding's `PdfSource`, which misses until the host
fetches. `doc.rs`'s opening comment — no file handles and no I/O traits,
because wasm has neither files nor mmap — holds: this is not an I/O
trait. It performs no transport, blocks on nothing, and on wasm is backed
by memory the host has already fed.

### Discovery without the whole file

`CosDocument::open` runs on `&buffer` end to end. The streaming open
reorders nothing and windows everything, reusing the same functions:

1. **Head window.** One read of the first 4 KiB covers `xref::header_shift`
   (`limits::MAX_HEADER_SCAN`, 7.5.2) and the linearization sniff below.
2. **Tail window.** `xref::startxref` probes `STARTXREF_SCAN`
   (1 KiB) then `STARTXREF_SCAN_MAX` (64 KiB) from the end — two ranged
   reads.
3. **Chain walk.** `xref::build` follows `/Prev` (7.5.4, 7.5.8; bounded by
   `limits::MAX_XREF_CHAIN`) fetching each section as a window: a classic
   table by chunk-doubling until `trailer` parses (7.5.5), an xref stream
   by parsing its dictionary then fetching its extent. Revisions (7.5.6)
   merge exactly as in a buffered open.

The eager `validate()` pass — every type-1 offset probed against its
`N G obj` header — is the one open-time step that touches everywhere. On a
streamed source it is deferred: `parse_at` re-checks every header
at load, so a lying entry is still caught at first use. The cost is a
semantic one, handled under determinism below.

### The linearized fast path

If the head window contains a `/Linearized` parameter dictionary whose `/L`
equals `source.len()` — Annex F's own rule for detecting that a linearized
file was incrementally updated and must be read as ordinary — the fast path
engages. Page one is found through Annex F part 3, the first-page
cross-reference section, which sits in the head and is parsed by the same
walker every other section goes through, and through `/O`, which names the
first page's object number outright (F.3.3, Table F.1). Page one's objects all
lie below `/E`, so rendering it is head reads only; `/Encrypt`, when present,
is numbered inside the head group by this writer's own layout — directly
after the parameter dictionary — so the front section places it and
authentication needs no tail either. The main table at `/T` is fetched only
when a read leaves the head and the hint tables cannot place it. Hints are
attacker-controlled bytes: they are an accelerator, never an authority —
every object still passes `parse_at`'s header check, and a hint that lies
falls back to the generic path with a typed warning, never a panic
(ruling 1).

`/O` is a requirement, not an optimisation. **A linearized file is free to
leave the page tree root in the tail, and qpdf's linearizer does**:
`lin1.pdf`'s first-page section covers objects 60 to 78, its catalogue is 61,
and its page tree root is not in the head at all. A page-one render that
insisted on walking the tree would fetch the end of every such file to draw
the front of it. So `pages::at(doc, 0)` builds the page from `/O` with nothing
inherited — by the same function the tree walk uses — and declines, letting
the walk run, when the page does not carry its own `/MediaBox` and
`/Resources`. A page laid out at US Letter because its ancestor was not
fetched is the wrong page, not a cheaper one.

There is one hint-table reader, `crates/tinker-pdf-cos/src/validate/hints.rs`
— bounds checked before each field is consumed, `width > 32` refused, entry
counts guarded — and it serves the open path, `validate.rs` and the writer's
round-trip tests in `linearize.rs`. Table F.4 item 5, the fractional-position
numerator, is read once per shared object *reference*. Every linearized file
in the qpdf corpus states item 12 as zero bits, so the corpus cannot tell
that reading from one per *page*; the discriminating case is a hand-packed
table in the decoder's own tests.

### Page *N* is found through the hint tables, and only their ranges are believed

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
  hint here that can change an *answer* rather than a cost: a table stating
  page one's length as
  zero puts page two's run at page one's page object — a page leaf with its
  own `/MediaBox` and `/Resources`, so every other check passes it — and
  `page(1)` would come back holding page one's content under index 1.
  Silently, cheaply, and wrong.

A file that fails any of them gets `WarningKind::LinearizedHintsUnusable` or
`LinearizedPageHintRejected` and the page tree walk, which fetches the main
table and is never wrong, only slower.

The third check does not ask a page after the first to begin at or past `/E`.
Table F.3 item 4 measures a page's length from its page object to the last
byte of the last object it *uses*, while `/E` ends part 6, and F.3.5 puts
things in part 6 the first page does not use — an outline hierarchy, when the
catalogue asks for one. The two are not the same number, and a check built on
their being equal would measure this reader's assumption rather than the file;
it would refuse well-linearized files.

Object *numbers* are deliberately not derived. F.3.1 numbers the remaining
pages' objects from 1 and the first page's after them, and Table F.4 item 1
repeats the rule — but a reader that derived numbers would be believing a hint,
and this one reads them off the file's own headers instead. What does derive
the numbers, and says so, is
`crates/tinker-pdf-cos/tests/linearized_numbering.rs` and
`validate/hints.rs`'s `annex_f_numbering` — a test rather than a read path,
which is the whole distinction.

The shared object hint table comes along for the same ride. Table F.6 item 1
places part 8's groups by accumulating from Table F.5 item 2, and Table F.4
item 4 says which of them a page names, so a font or a colour space several
pages share is fetched with the page that needs it rather than through the
table at `/T`.

### Against linearized files this project did not write

`crates/tinker-pdf/tests/streaming_open.rs` holds the generic open, an open
plus one mid-file object, page one of a generated 60-page linearized file and
page 31 of it — alone and after page one — each to a committed byte budget
(`OPEN_BUDGET`, `OPEN_AND_ONE_OBJECT_BUDGET`, `LINEARIZED_PAGE_ONE_BUDGET`,
`LINEARIZED_PAGE_N_BUDGET`, `LINEARIZED_PAGE_N_AFTER_ONE_BUDGET`).

Over the qpdf corpus's already-linearized files, the sweep holds how many open
from their heads (`HEAD_OPENED_CORPUS_FILES`; a file stating parameters this
reader will not act on falls back rather than guess), and each renders page
one without reaching past `/E` but those `REACHES_PAST_E` names: `badlin1.pdf`,
qpdf's deliberately damaged linearization fixture, whose declared `/E` is not
where its first page ends. Page one is **compared** with
the page the page tree walk draws, not counted. The files where the two differ
are named in `PAGE_ONE_ONLY_FROM_THE_HEAD`, and they are files the walk cannot
answer for at all: both are sealed under a password the sweep does not have,
and the difference is which route survives that — `/O` names the object
outright and a page dictionary is structure rather than content, so the head
path builds a page and draws whatever its content stream decrypts to, which is
nothing; the walk cannot start, because the catalogue is inside an object
stream and an object stream is a stream. They are named rather than excluded,
because it is a real difference between two routes to the same page.

**Every file with a second page to ask for draws the page the main table
draws** — compared bitmap against bitmap, because a cheap render of the wrong
page passes every byte budget there is. The files that reach it only through
the main table are named in `PAGE_TWO_NEEDS_THE_MAIN_TABLE`, and are two
families: files encrypted under a password the sweep does not have, so their
hint stream is ciphertext and refusing to read it is the right answer, and
qpdf's damaged linearization fixtures, some of which `validate::hints`
refuses by name. A sealed document that is then given its password reads its
tables at once — `install_security` drops the refusal with everything else
that was read as plaintext out of ciphertext.

### Windows sized to the head

A window bigger than the head of a small file pulls the tail to parse the
front, so three are kept to the head:

- The `%%EOF` search that ends a revision is not run on the head path. A
  linearized file's only `%%EOF` is at the very back, so scanning forward from
  a first-page section at the front would read everything. The head path is
  told its revision is the document.
- The first-page section window is `HEAD_SECTION_WINDOW` and the object
  window `OBJECT_WINDOW`, 1 KiB each.
- A read that begins inside the first page is clamped to `/E`. An object that
  ends there needs no byte past it, but a window sized by a guess asks for
  more, and on a fixed granularity that guess costs a whole chunk of tail. Only
  a parse that comes up short is allowed through, so the ceiling changes which
  bytes are fetched first and never which are readable.

### The store

`SlotStore`, `ResolveCtx` and the compare-and-swap publish discipline in
`store.rs` — and `ObjStmCache` in `objstm.rs` — are the buffered store's; what
differs is what the store is lazy *over*. `CosDocument`'s buffer is a
`Backing`: a whole buffer (zero new cost) or a chunk
cache over a `ByteSource` — fixed-granularity aligned chunks, each fetched
once and kept as `Arc<[u8]>`, so repeated small reads coalesce and the byte
counter measures policy, not luck. Every internal read
goes through a window accessor; the parse layer (`parse_indirect_at`,
`lexer.rs`, `streams::resolve_extent`) keeps taking `&[u8]` — bytes in,
values out, per ruling 8. A miss never publishes: no `Object::Null` enters
the store because a range was absent, or a retry would deterministically
read a wrong value the first arrival baked in.

### What stays whole-file, and says so

- **Repair.** `ScanIndex::build` is one forward pass over everything; that
  is its point. A streamed document that needs ladder level 2 or 3 fetches
  the whole source first and warns `WholeFileFetched` (provenance per
  ruling 10) alongside `DocumentRescanned`.
- **Eager validation.** `ladder_level()` on a streamed document reflects
  the bytes read so far; `complete_validation()` gives the eager answer,
  probing each header through a window it keeps none of rather than fetching
  the file (*The windowed verdict and the gate*, below). Both are documented
  observables, not moods.
- **`bytes()`**, incremental save, signatures, and a `WriteOptions::linearize`
  re-save: whole-file by contract. `whole_file_fetched()` reports it; it is
  not warned (see the `CosDocument::bytes()` risk under *A file-backed source
  in the hosts that own files*).
- **Containers** (`cbz::container` sniff): whole-file, declared through
  `Document::whole_file_fetched`.

### What a streamed document does differently, and says so

- `ladder_level()` is **provisional** until `complete_validation()`, which
  runs the eager offset probe the open deferred over the merged table, through
  windows it keeps none of, and warns nothing.
- The repair index is built at the first read the table cannot answer, and
  only when a buffered open of the same bytes would have one (*An entry the
  table cannot place*, below); the fetch is declared with
  `WarningKind::WholeFileFetched`, and a lying entry reads the same value on
  both paths (`a_lying_offset_is_repaired_on_both_paths`). The one ask the
  gate leaves as it was, a type-1 entry of a file with a header shift, is in
  *The windowed verdict and the gate*.
- `Revision::byte_range.end` is the document's end rather than the byte past
  `%%EOF` when the marker is more than `REVISION_END_SCAN` (8 KiB) past the
  section, which on the head path it always is.
- `xref()` fetches the main table, because asking for the table is asking about
  every object. So does `page_count()`, for the same reason: 7.7.3.2's `/Count`
  is a claim like any other and the walk that checks it is a walk over every
  page. `/N` in the parameter dictionary would answer it from the head, and is
  not used for it — a page count taken from a hint would be a number this
  reader had checked nothing about.
- `main_table_fetched()` reports whether that has happened, which is the
  observable the byte budgets for page *N* are written against.

### Bytes before `%PDF-`

7.5.2 lets a file begin with something else, and then every offset it states is
measured from the header rather than from byte zero. Table F.1 says `/L` is
"the length of the entire file", so such a file states a length that *includes*
the prefix while `/H`, `/O`, `/E`, `/T` and every position in the hint tables do
not. The hint path holds both readings: positions it derives are shifted, and
the two cross-checks above compare through `xref::offset_candidates` rather than
by equality, so a file that agrees with itself is not called a file that does
not.

What the hint path does not cover is underneath it. The first-page
cross-reference section states its entries from the header too, and the pass
that reconciles those with the file's own `N G obj` headers is the eager
`validate()` a streamed open defers — so on such a file the objects that
section places are found by the repair scanner instead, which fetches
everything and warns `WholeFileFetched`. Page *N* is drawn from its head
regardless, because its objects come from the hint tables.
`a_linearized_file_behind_leading_junk_still_opens_from_its_head` pins both
halves.

### wasm: the trait is synchronous, and why

An async trait is rejected. It would need a runtime the
workspace forbids, would color every function from `load` to `Device`, and
would make output timing-dependent, which ruling 4 exists to prevent. The
sync-plus-typed-miss shape gives wasm the same engine: the JS host
(`bindings/js`) implements `ByteSource` over memory it has fetched; a read
the host has not fed yet returns `SourceMiss`, which propagates up — never
converted into a value — and surfaces at the facade as a typed "need these
ranges" result. The host awaits its own `fetch`, feeds the chunk cache,
and calls again; because parsing is pure and the store caches, the retry
repeats no completed work. A native host reads its file at offsets; a read
that fails is a miss it has nowhere to fetch from, so it fails the file rather
than calling again (*A miss, to a host that cannot fetch*, below). One engine,
two host loops.

### Determinism: arrival is not an input

Ruling 4's contract extends, not bends: the same bytes and the same query
sequence produce the same values, warnings, and fingerprints regardless of
chunking, ordering, or how many misses occurred on the way. The warning
contract allows warnings to *arrive* lazily after open
(`CosDocument::warnings` documents this).
`crates/tinker-pdf/tests/streaming_determinism.rs` holds the rest: every
fixture renders from a buffer, from a `SliceSource` and from a
`ShreddedSource` that splits every read adversarially, and each streamed
render must equal the buffered one.

## Milestones

1. `ByteSource`, `SliceSource`, `CountingSource`, `ShreddedSource`; `Backing`
   behind `CosDocument`.
2. Incremental tail-first discovery (the generic path), held to committed byte
   budgets in `streaming_open.rs`.
3. One hint-table reader, `validate::hints`, serving the open path, the
   validator and the writer's round-trip tests.
4. The linearized fast path, `Document::open_streaming` and the page-one
   render.
5. Honest degradation and the wasm host loop.

## Time to first page

*For [ROADMAP](../ROADMAP.md) row MS-08.* The byte budgets say how many bytes
page one costs. They do not say how long it takes over a link where every
request waits for its round trip, and on such a link it is requests rather
than bytes that cost: at 50 ms and 10 Mbit/s one round trip is worth 62,500
bytes of transfer, more than the whole of page one of the linearized fixture.
MS-08 counts requests, bounds them for page one with and without
linearization, and turns the counts into a time. This section fixes what is
counted.

### Scope

- **What a request is:** one call to `ByteSource::read` that reaches the source
  a document was opened over, which is what `CountingSource::reads()` counts.
- **One read per run of missing chunks** (MS-08.2): `ChunkCache::window`
  fetching each run of the window's uncached chunks with one read, rather
  than one read per chunk.
- **The linearized bound:** page one of the 60-page linearized fixture
  (`a_linearized_document`, `crates/tinker-pdf/tests/streaming_open.rs`)
  in at most `LINEARIZED_PAGE_ONE_BUDGET` bytes and 6 requests, the main
  cross-reference table never fetched.
- **The unlinearized bound:** the same pages saved without linearization, page
  one in at most 12 requests and in no more bytes than three byte ranges -- its
  tail, its cross-reference section, page one's objects -- plus 4,096 a
  request, the ranges derived from a buffered open of the same bytes.
- **A bench row** (MS-08.4) whose time is the measured work plus a link time
  computed from those counts.
- **The container sniff:** whether it stays a request of its own. It does.

### Non-goals

- **No prefetch.** The non-goal above stands. A coalesced read is
  bounded by the window it serves, so it fetches exactly the chunks a
  chunk-at-a-time loop fetches, in fewer requests; nothing is read because it
  might be wanted next.
- **No new granularity, and no byte budget moves.** `CHUNK_SIZE` stays 4,096,
  so every byte budget stands. MS-08 moves request counts and no byte count.
- **Nothing sleeps.** A slept latency would time the runner's timer and its
  scheduler, and the row asks for a time its counts determine.
- **Not the host loop's rounds.** A host that misses and feeds, as the wasm
  loop does, pays a round trip per round rather than per read, and
  `a_host_that_feeds_what_it_was_asked_for_converges_in_bounded_rounds`
  (`crates/tinker-pdf-cos/tests/streaming.rs`) bounds its rounds at 16.
  That bound stays where it is. A coalesced miss names its whole run, so the
  loop may converge in fewer rounds; nothing here asserts that it does.
- **No concurrency in the model.** The trait is synchronous: a read is
  answered before the next is asked, so the model adds every latency. A host
  that overlaps its requests would beat the model, and is not credited for it.
- **Page one of generated fixtures.** Page *N* keeps its byte budgets and gains
  no request bound, and the qpdf corpus sweeps keep their assertions and gain no
  count.

### Design

#### What a request is

`CountingSource` is the outermost source. The guard opens over
`CountingSource::new(SliceSource::new(bytes))`, as
`a_linearized_file_renders_page_one_without_touching_its_tail` does, so every
read anything in the engine makes is one the counter sees. Two readers make
them, and only two: the chunk cache, whose one read is in `ChunkCache::gather`
— which `ChunkCache::chunk` calls, and so does `Backing::read_transient`, the
windowed verdict's read, which keeps nothing — and the facade's `filled`,
through which the facade sniffs for a container. A file whose first kilobyte
holds no `%PDF-` adds the standalone sniff's wider read; neither fixture does.
A page render that asks for no verdict and reads no entry its table cannot
place makes no `read_transient` read.

`SliceSource` answers every range whole, so over it one read is one range a
host would ask its transport for -- an HTTP range request, a positioned read.
That is the request MS-08 counts, and three things follow from it.

- A source that answers short is asked again by the loops that read it
  (`ChunkCache::gather`, `filled`), and every ask is a request. That count is
  honest about such a source and is not the guard's: over `ShreddedSource` a
  chunk is 4,096 requests by design.
- A read that misses is not recorded. `CountingSource::read` returns the miss
  before it records anything; a miss is the host loop's business, and the
  guard's source never misses.
- `bytes_read()` is gross: a byte fetched twice is charged twice. On these
  fixtures that happens once, to the sniff's kilobyte, which chunk 0 fetches
  again.

#### One read per run of missing chunks

`ChunkCache::window` takes the chunks from `first` to `last` and asks
`ChunkCache::chunk` for each in turn, and `chunk` fetches a missing one with a
read of its own, so a window over *k* missing chunks is *k* requests. MS-08.2
makes it one request a run:

1. the window's chunks that are not yet cached are grouped into maximal runs of
   consecutive indices, between `first` and `last` and never outside them;
2. each run is one read, from its first chunk's start to its last chunk's end,
   clamped to the document, looping on a short answer as `chunk` does;
3. what arrives is cut at chunk boundaries and published a whole chunk at a
   time, so a miss part-way through a run publishes the whole chunks before it,
   as the chunk-at-a-time loop does, and never a part-filled one -- the
   invariant `chunk` keeps for one chunk ("nothing is cached until it is
   complete").

A cached chunk inside a window splits it into two runs and two reads; a window
with nothing missing is no read. **A run never reaches past its window**, and
that is what keeps the no-prefetch non-goal true: the chunks a coalesced read
fetches are the chunks a chunk-at-a-time loop fetches, so the bytes and every
value are unchanged and only the number of requests moves. A window that asks
for more than its object needs -- a section's first window
(`XREF_SECTION_WINDOW`, 8 KiB), an object's (`OBJECT_WINDOW`, 1 KiB) -- costs
the chunks it costs. Shrinking a window is a different change, and not this
row's.

#### The container sniff stays a request of its own

The facade reads the first 1,024 bytes (`CONTAINER_SNIFF`) straight from the
source before it knows what it is opening: a container or a standalone file,
either of which is then fetched whole, or a PDF. Only a PDF gets a chunk
cache, built inside `CosDocument::open_source`, and its head window then
fetches chunk 0 -- the sniff's kilobyte again. The sniff stays a request, and
both bounds count it: the linearized page's `LINEARIZED_PAGE_ONE_BUDGET` bytes
are 7 x 4,096 + 1,024, and its 6 requests include the sniff.

What it costs is one round trip and 1,024 bytes on every streamed PDF open: on
the bench's link 50 ms, and with MS-08.2 one of the linearized page's three
requests. Serving chunk 0 from the sniff -- the facade reading a whole chunk and
handing it to the open -- would take that page from three requests to two, and
every budget measured from an open down by 1,024 bytes. That is a facade change
that moves committed budgets, which this row leaves alone, so it is named here
rather than taken.

#### The linearized bound

The head window is chunk 0, and the page's content stream is read by one
window over its declared length (`crates/tinker-pdf-cos/src/streams.rs`),
which ends in the chunk `/E` falls in. Read a chunk at a time, that window is
a request per missing chunk; with MS-08.2 its missing chunks are one read, and
the page costs three: the sniff, the head, the content.

The guard holds the page to the row's figures: at most
`LINEARIZED_PAGE_ONE_BUDGET` bytes and 6 requests, `main_table_fetched()`
false, and no read touching the table at `/T`.

#### The unlinearized bound, and its three terms

The twin is `a_linearized_document`'s pages saved with `linearize: false` and
`object_streams: false`, so the two files differ only in layout. Its bound is

```text
reads() <= 12
bytes_read() <= |T ∪ X ∪ P| + 4,096 x reads()
```

where T, X and P are byte ranges the test derives from a buffered open of the
same bytes (`CosDocument::open`). They are never typed in, so
a writer change that moves the twin's layout moves its bound with it.

- **T, the `startxref` probe's tail window**: the window `xref::startxref`
  finds the keyword in -- the last `STARTXREF_SCAN` bytes, 1,024, or, when the
  keyword is not in them, the last `STARTXREF_SCAN_MAX`, 64 KiB. The test finds
  the keyword in the buffered bytes and takes the window that holds it.
- **X, the cross-reference section `startxref` names, through its trailer**:
  from `CosDocument::last_startxref()`, where the `xref` keyword is, to the
  byte after the trailer dictionary's `>>`. That is where `parse_object_at`
  (`crates/tinker-pdf-cos/src/parse.rs`) stops on the dictionary after the
  `trailer` keyword, the parse the walker itself makes. The `startxref` line
  and the `%%EOF` after it are T's. One section, because the twin is one
  revision.
- **P, page one's objects**, each from its `N G obj` header, at the offset the
  buffered table states, to the end of its `endobj`:
  - the catalog, the trailer's `/Root`;
  - the page tree nodes walked: from the catalog's `/Pages` down to page one,
    the nodes `pages::collect_upto` loads before its limit stops the walk;
  - the leaf, page one's page object;
  - its contents: every stream `/Contents` names, and the array too when it is
    an indirect object of its own;
  - its resources: the `/Resources` dictionary when it is an indirect object,
    the leaf's own or one a walked node passes down (`Inherited::extend`
    resolves it, `pages.rs`), and every indirect object reachable from
    it -- more than a render may read, which loosens the bound and never
    tightens it.

  An object that is not a stream ends where `parse_indirect_at` stops, past
  `endobj`. A stream's parse stops at the end of its declared data, and its
  term runs on through `endstream` to the `endobj` after it.

The terms are taken as a union, so a byte two of them hold counts once: on the
twin the tail window begins inside the table. **The allowance is a chunk a
request.** Every read fetches whole aligned chunks, so each carries the bytes
between a term's edge and its chunk's, and the sniff carries the kilobyte that
chunk 0 holds again. It is a claim about the twin's layout rather than a
theorem -- a run whose first and last chunks a term only grazes could carry
nearly two chunks -- and the guard is what checks it.

#### The guard

**A ceiling alone does not bite here.** The twin's ceiling is what it costs
read a chunk at a time, so a test that held it to the ceiling and nothing else
would stay green with MS-08.2 reverted. So each of MS-08.3's tests holds
`reads()` to the count it measured as well as to the row's ceiling. The count
is committed beside the ceiling the way `LINEARIZED_PAGE_ONE_BUDGET` commits a
measured byte count. The ceilings are the row's promise; the committed counts
are what catch a request added under them.

Each prints a `RAN` line with its counts and runs in MS-01's guard suite on
linux, windows, macos and `wasm32-wasip1`. The fixtures draw paths and no text,
so one build.

#### The link time is computed, never slept

`CountingSource::link_time(latency, bits_per_second)` (MS-08.4) is
`reads()` times the latency plus `bytes_read()` times 8 over the bits a second:
a latency a request, because the trait is synchronous and each read is a round
trip that ends before the next begins, and the bandwidth a byte delivered,
gross, as `bytes_read()` counts it. At 10 Mbit/s a byte is exactly 800 ns, so
the bench's figure needs no rounding.

The bench row is tier P's "Time to first page over a source with 50 ms latency
at 10 Mbit/s, the 60-page linearized fixture" (`docs/roadmap/P.md`), in
MS-04's open group. It is timed through criterion's custom timing: each
iteration opens the fixture over a fresh `CountingSource`, renders page one,
and reports the time that took plus the link time of that iteration's counts.
The counts do not vary, so the link part is a constant and the row's spread is
the work's.

### Milestones

1. MS-08.2: one read per run of missing chunks in `ChunkCache::window`, with
   `streaming_determinism.rs`, `crates/tinker-pdf-cos/tests/streaming.rs` and
   every byte budget in `streaming_open.rs` unchanged.
2. MS-08.3: the counted guard in `streaming_open.rs` — the linearized first
   page and the unlinearized twin, each held to its ceiling and its committed
   count — in MS-01's guard suite on four targets.
3. MS-08.4: the link model and the bench row.

### Risks

| Risk | Mitigation |
|---|---|
| A miss part-way through a coalesced read publishes a part-filled chunk, and the miss's answer is read back for good | Whole chunks only, the invariant one chunk already keeps, and a test of a miss inside a run |
| Coalescing grows into read-ahead, and the counter stops measuring what the engine needs | A run is bounded by its window's own chunks, and every byte budget in `streaming_open.rs` holding unchanged is MS-08.2's exit, so a read past a window moves a byte count and fails |
| A ceiling with room under it hides an added request | The measured counts are committed beside the ceilings |
| The twin's bound is typed in and rots with the writer | Derived from a buffered open of the same bytes on every run |
| The allowance and the committed counts are facts about the fixtures' layout, so a writer change can fail the guard with no engine change | The terms are recomputed on every run, and a failure names each read with the bytes it carried outside them, so a layout change can be told from an engine one |
| The row's time is mostly link time -- 50 ms a request against milliseconds of work -- so a slower open or render barely moves it | The counts are held exactly in `cargo test`, and the link part of the figure is a constant those counts give, so the work is the figure less it and reads on its own |
| The model flatters a real link: no slow start, no per-request overhead, no server time | The model is stated beside its figure; it compares layouts and changes with each other, and predicts no network |

## A file-backed source in the hosts that own files

*For [ROADMAP](../ROADMAP.md) row P-05.* Every native host
that opens a path reads the whole file and then hands the facade a copy of it.
`tpdf` reads a `Vec` that `Document::open` copies into an `Arc<[u8]>`
(`tools/tpdf/src/main.rs`, `crates/tinker-pdf/src/lib.rs`); Python
reads one, copies it, and the facade copies the copy
(`bindings/python/src/lib.rs`); .NET reads a managed array that
the C ABI copies twice more (`bindings/dotnet/TinkerPdf.cs`,
`crates/tinker-pdf-ffi/src/lib.rs`). The row asks for 6 ms and 10 MB. This
section fixes where the source that gets there lives, what it may do, and how
every host keeps the answers a buffered open gives.

### Scope

- **Where the source lives:** in the hosts that own files and in no engine
  crate: `tools/tpdf/src/source.rs` (P-05.4), the Python binding's own copy
  of it (P-05.7), and a `FileStream` behind the C ABI's vtable in .NET (P-05.8).
- **How it reads:** at offsets, through std's per-platform `FileExt` traits for
  unix and windows; no mapping and no `unsafe`.
- **Its length,** fixed at open. A read that fails, or ends short of that
  length, is a `SourceMiss`, which a host with nowhere to fetch from turns into
  a failed file rather than a null.
- **Which CLI commands read on demand,** and which read the file whole, once,
  in place.
- **One rule for every path open:** keep the streamed document only when it
  answers as a buffered open of the same bytes does, and otherwise read the file
  whole and open that. It is how `info` and `check` keep the buffered open's
  ladder line and warnings, and it settles what a binding's path open reports in
  `warnings()`.
- **One copy:** a whole-file fetch holds the file once (P-05.3); a slice handed
  to the facade costs one copy and an `Arc<[u8]>` none (P-05.6).
- **What the engine provides** (P-05.2): a verdict reached through windows it
  keeps none of; a streamed document that reads an entry its table cannot place
  as a buffered one does; and the predicate the rule asks.
- **The measures:** how MS-04's 23 MB open and MS-05's open case reach the
  path, so P-05.9's 6x floor and 10 MB band measure it.

### Non-goals

- **No mapping.** std maps nothing, so a mapping is `unsafe` wherever it is
  made, and a mapped file that another process truncates raises `SIGBUS` at the
  next touch, which no Rust code turns into a `SourceMiss`: ruling 1 would end
  at the first truncated file. A positioned read of the same file comes back
  short, and short is a miss.
- **No file I/O in the engine crates.** `tools/tpdf` is a workspace member
  that reads files; the engine gains no source and no trait method.
- **No prefetch.** The source answers the ranges it is asked for, whole,
  and reads nothing because it might be wanted next.
- **`probe` keeps its record.** It opens buffered, because the corpus
  ratchet reads what it prints (`xtask/src/runner.rs`). Reading the file once
  instead of copying it lowers only its `peak` and `ms` lines, measurements the
  ratchet records as an improvement and never as a regression
  (`xtask/src/ratchet.rs`).
- **No ladder in a binding's own surface.** Python decides in Rust; .NET asks
  one C ABI function P-05.8 adds, which answers yes or no. Neither binding gains
  a ladder accessor, and the C ABI has none.
- **Not the C ABI callback's own copy.** `CallbackSource` reads into a `Vec` and
  copies that into an `Arc` (`crates/tinker-pdf-ffi/src/lib.rs`), so
  each answer through a vtable costs its length twice for a moment. .NET reaches
  it only for the documents it keeps streamed, which read whole only when a
  caller asks for a whole-file operation; no band measures that, and P-05
  leaves it.
- **Not JavaScript, Go or Java.** JavaScript keeps `PdfSource`, the host-fed
  source wasm needs; Go and Java open through
  `tpdf_document_open_streaming` with sources of their own and keep the
  streamed contract they have.
- **Unix and Windows only.** Those are the source's two arms and what CI builds
  `tpdf` on (`.github/workflows/ci.yml`); a target with neither fails to
  compile rather than reading through an arm no run exercises.

### Design

#### Where the source lives

The engine keeps its one trait (`crates/tinker-pdf-cos/src/source.rs`)
and implements it for memory alone. A host that owns a file implements it once
more:

- **`tools/tpdf/src/source.rs`** (P-05.4) holds the file source, the whole-file
  read and the open every reading command calls. It names nothing but `std` and
  `tinker_pdf`, so P-05.5's probe child includes it by path, as MS-05 reaches
  `benches/support`, and runs the CLI's own open rather than a copy of it.
- **The Python binding** (P-05.7) carries its own copy of that shape, because
  it builds outside the workspace and cannot depend on a binary's module.
- **.NET** (P-05.8) reads a `FileStream` at offsets behind
  `tpdf_document_open_streaming`'s vtable, its context a `GCHandle` the
  vtable's `free` releases, as Go's binding holds a `cgo.Handle`.

#### Positioned reads, and a length fixed at open

- **The length** is the open handle's `metadata().len()`, read once.
  `ByteSource::len` is fixed for the life of a source, and the C ABI caches its
  host's for the same reason. Every read is clamped to it, so
  bytes appended after open are never read: the document is the file as long as
  it was when it opened, which is what every offset in it, `/L` among them, is
  measured against.
- **A read** allocates exactly its clamped range as a zero-filled `Arc<[u8]>`,
  collected from `iter::repeat_n(0u8, n)`, which builds the `Arc` in one
  allocation, and fills it in place through `Arc::get_mut`, which a freshly
  collected `Arc` always grants. No `Vec` is copied into an `Arc`, and nothing
  is `unsafe`.
- **On unix** the fill is `std::os::unix::fs::FileExt::read_exact_at`, `pread`
  in a loop that retries an interrupted call; **on windows** it is a loop of
  `std::os::windows::fs::FileExt::seek_read`, which moves the handle's cursor
  but takes its offset from every call. Nothing reads the cursor, so concurrent
  reads share nothing, and the engine does read from several threads at once
  (`ByteSource: Send + Sync`; `render --jobs`).
- **The answer is the whole range or a miss.** The trait allows a short
  answer; a file has no reason to give one, and a whole answer keeps one read
  one request, which is what `CountingSource` counts and what P-05.4's read
  budget is written in.
- **A read that fails, or ends inside the fixed length** (the file shrank under
  the reader, the disk failed) is `SourceMiss::at` its range. The engine's own
  loops take an empty answer inside the length for a miss rather than an end
  of file (`ChunkCache::gather`).
- **The whole-file read** is the same read over `0..len`: one buffer of the
  file's length, filled in place, handed to `Document::open` as an
  `Arc<[u8]>`, which the facade keeps without a copy.

#### A miss, to a host that cannot fetch

The engine never turns a miss into a value. A load that missed reads as null for
that call and is not published, so the next call asks again, and an open that
missed is `OpenError::SourceUnavailable`. That is right for a host
that goes and fetches the range, and wrong for one with nowhere to fetch it
from: a CLI that printed a page count after a failed read would print the count
of a page tree that was partly null.

So the file source records its first failure, the range and the error, and
`tpdf` asks it before printing or writing anything it read through the source:
`info`'s and `check`'s lines for a file, each page for the commands that go page
by page. A recorded failure fails that file, `reading {path}: {error} at bytes
{start}..{end}`, and `run` goes on to the next, as it does for one it
cannot open. In a binding a failed open raises as an unreadable file raises.
After open the binding keeps the engine's contract, in which the call that
needed the bytes reads them as absent and the next asks again, as every
streamed host does.

#### One rule for every path open

A streamed document can answer differently from a buffered open of the same
bytes, and *What a streamed document does differently* lists how: its ladder is
provisional, it repairs at first read and fetches the whole file to do it, and
Annex F's fast path builds from the head what a buffered open builds from the
page tree. P-05 removes none of that. It keeps every path open away from it:

1. Open through the file source (`Document::open_streaming`).
2. Straight after the open, before anything else is read, ask
   `CosDocument::answers_as_buffered`, through `Document::cos`.
3. If it says no, drop the document and read the file whole. If it says yes,
   answer from the streamed document.

`answers_as_buffered` is true for a document opened from a buffer and, for a
streamed one, exactly when three things hold. Each is a way the two can part
that nothing at open would otherwise show.

- **It did not open on Annex F's head-only path** (`first_page_end()` is
  `None`). That path builds page one from `/O`, and later pages from the hint
  tables, out of the page's own dictionary (`pages.rs`), and declines only for
  a missing `/MediaBox` or `/Resources`: two of the four attributes 7.7.3.4
  lets a page inherit. A page whose `/Rotate` or `/CropBox` comes from its
  `/Pages` node is therefore built without it: a buffered open's page one, and
  the streamed document's own tree walk (`Document::pages`), carry the
  ancestor's rotation and crop box, and the head path's does not, and neither
  open warns. So a path open reads a linearized file whole (see Risks).
- **It has warned nothing.** What a streamed open warns at open is the header
  scan's and the cross-reference walk's, which a buffered open warns the same
  from the same functions, and the source's own. Five kinds are raised only
  where a streamed document does what a buffered one does not, by Annex F's
  fast path or by a whole-file fetch: `LinearizedLengthMismatch` and
  `LinearizedParametersUnusable` at open, the two hint-table kinds for a page
  past the first, and `WholeFileFetched`. Rather than classify them, the rule
  reads any document that has warned anything whole. The one other warning at
  open that would matter, `HeaderNotAtStart` (`xref.rs`), goes the same way: a
  buffered open rewrites every shifted entry, where a streamed one reads each
  at the offset its table states and so repairs it at first read through the
  whole file, which
  `a_linearized_file_behind_leading_junk_still_opens_from_its_head` pins for a
  linearized head.
- **Its completed verdict is `Trust`.** `complete_validation` decides the
  ladder a buffered open would have, through windows it keeps none of, over
  the merged table. `Patch` or `Rescan` means a buffered open repairs at open,
  and the answers must come from that open's table and warnings.

No host classifies a warning kind, and none has to: the predicate lives in the
engine beside the mechanisms it sums up, so a change to one of them changes it
too. `streaming.rs`'s comparison helper calls `WholeFileFetched` the one warning
a streamed document has that a buffered one cannot
(`crates/tinker-pdf-cos/tests/streaming.rs`), which is true of its fixtures,
none of them linearized, and not of the five.

**Why the streamed branch answers as a buffer does.** On that branch the open
warned nothing, took the generic path, and decided the buffered open's `Trust`.
A buffered open of the same bytes then warns nothing at open either: its header
scan and its walk are the same functions over the same bytes, and its eager
validation found no failure, which is what `Trust` means. With no header shift
the only offset it tries for an entry is the stated one, so the only thing its
validation rewrites is a generation the object's own header states, and both
documents read every object from the same offset through the same parse, which
reads the same over a window as over the whole buffer
(`crates/tinker-pdf/tests/streaming_determinism.rs` holds the two to the same
pixels). What a command reads afterwards therefore warns the same and reads the
same values on both, because the gate below makes an entry the table cannot
place read as the buffered open reads it. On the other branch the answers come
from a buffered open, so they are a buffered open's by construction.

**What reads on demand is therefore a file that opens cleanly and is not
linearized**, the row's file among them. Every other file is read whole, once.

#### An entry the table cannot place

`load_uncached` can ask the repair scanner for any object its table cannot
place: a type-1 entry that does not parse as the object it names, and also an
entry that is free or absent. `repair_index` builds that scanner for a streamed
document by fetching the whole file, where a buffered document has one only
when its open had to repair. Ungated, a reference to a free or absent object on
a streamed document whose verdict is `Trust` would cost the whole file, and a
free object whose old body is still in the file would read back as that body,
where a buffered open reads it as `Null` and warns nothing. 7.3.10 makes both
null.

So the ask is gated. A streamed document asks the scanner for an object its
table cannot place only when a buffered open of the same bytes would have one,
which is when its verdict, from `complete_validation` and memoized, is not
`Trust`. That holds for a type-1 entry too: one whose header is right but whose
object does not parse passes the eager probe, and a buffered open with no other
failure reads it as null, with no scanner to ask. On a document nothing has
asked for the verdict yet, the first such read completes it, through windows.
The streamed branch of the rule is then the buffered open's, value for value.

#### Which commands read on demand

| Command | Opens | Why |
|---|---|---|
| `info`; `check` with no verdict flag | on demand, by the rule | they print the ladder and the warnings, which the rule keeps a buffered open's |
| `text`, `render`, `fields`, `fonts`, `images`, `outline` | on demand, by the rule | they print values only (`tools/tpdf/src/main.rs`, `tools/tpdf/src/images.rs`) |
| `check --strict`, `--pdfa`, `--pdfua`, `--pdfx` | whole | the strict and PDF/A passes read the file's bytes whole through `CosDocument::bytes()` (`crates/tinker-pdf-cos/src/validate.rs`, `crates/tinker-pdf/src/pdfa/syntax.rs`); the strict pass reports the ladder as it stands and counts every warning kind but two as a defect; and `check` opens a file once for every flag |
| `objects` | whole | it prints the table's offsets and generations, which a buffered open's validation rewrites from each object's own header and a streamed open never rewrites |
| `probe` | whole | its record is the corpus ratchet's input |
| `merge`, `split`, `rotate`, `encrypt`, `decrypt`, `attach`, `stamp`, `sanitise` | whole | every write is a rewrite (`tools/tpdf/src/writing.rs`) that reads every object it keeps, which on demand is the whole file in pieces |

*Whole* is the in-place read above in every row, and it is also the read the
rule's reopen uses: one copy, never a `Vec` copied into an `Arc`. `info` prints
`complete_validation`'s verdict as its ladder line, which on the streamed branch
is `Trust`, as `ladder_level()` is.

#### The two files the exit names

| | A file whose validation needs repair | A file whose streamed open raises the source's own warnings |
|---|---|---|
| Its shape | a type-1 entry pointing into another object (`a_damaged_document`, `crates/tinker-pdf-cos/tests/streaming.rs`) | a linearized file with bytes after it (`a_length_that_is_not_the_file_falls_back_to_the_generic_path`, `streaming_open.rs`), or a parameter dictionary without `/E`, `/T` or `/O` |
| A buffered open | ladder `Patch`; `ObjectHeaderMismatch` and `ObjectRepaired` at open | ladder `Trust`; no warning |
| A streamed open | provisional `Trust`, verdict `Patch`; nothing warned until a read reaches the entry, then `ObjectHeaderMismatch`, `WholeFileFetched`, `ObjectRepaired` | `LinearizedLengthMismatch`, or `LinearizedParametersUnusable`, at open; verdict `Trust` |
| The rule | the verdict is not `Trust`: whole | it warned: whole |
| `tpdf info` | the buffered open's lines: `ladder      Patch`, then `warnings    2` over `ObjectHeaderMismatch` and `ObjectRepaired` | the buffered open's lines, for that fixture `ladder      Trust` and no `warnings` line |
| `tpdf check` | `ok    {path}  {n} pages, Patch, 2 warnings`, and counted among the files with warnings | `ok    {path}  {n} pages, Trust, 0 warnings`, and not counted |
| A binding's path open | `warnings()` is `ObjectHeaderMismatch`, `ObjectRepaired`, as the byte open's; not streamed | `warnings()` is empty, as the byte open's; not streamed |

The third kind the step names, `WholeFileFetched`, is never all a streamed open
raises at open: there it comes with `DocumentRescanned`, when the tables cannot
be used, which is the left-hand column at `Rescan`. After open only
`fetch_whole` raises it, and its callers are the two repairs at open and
`repair_index`; `complete_validation` does not call it. On a document the rule
keeps, none of them runs: the verdict is `Trust`, the scanner's ask is gated,
and validation keeps the file out of its reads. The one shape the gate leaves
ungated, a type-1 entry of a file with a header shift, never meets a kept
document, which warned nothing and so warned no `HeaderNotAtStart`. So a
binding's `warnings()` on a document it kept streamed gains only what that
document's reads tolerate, which a buffered document's same reads tolerate the
same way.

#### One copy, and what a slice costs

- **A whole-file fetch holds the file once** (P-05.3). `materialise` keeps
  every chunk beside a joined copy, and the join is a `Vec` copied into an
  `Arc`; P-05.3 makes it ask the source for the whole range, keep an answer of
  the whole length as the document's buffer, gather a short one into a buffer
  of the document's length filled in place, and empty the chunk map. The file
  source keeps no bytes of its own, so the process holds the file once. A
  `SliceSource` keeps its own, so a streamed open over one holds them twice
  after a whole-file fetch; no host that owns a file uses one.
- **A slice handed to the facade costs one copy, and an `Arc<[u8]>` none.**
  `Document::open` takes `impl Into<Arc<[u8]>>` and converts once: a slice is
  copied into a new `Arc`, so is a `Vec`, both existing for the moment of the
  copy, and an `Arc` is kept as it is. The surfaces that take bytes copy a
  borrowed slice into a `Vec` first, which the facade then copies again
  (`crates/tinker-pdf-ffi/src/lib.rs`, `bindings/python/src/lib.rs`,
  `bindings/js/src/lib.rs`); P-05.6 builds the `Arc` straight from the
  slice.
- **Every CLI command holds one copy fewer:** on demand it holds the chunks its
  reads touched, and whole it holds the file once, where a read-then-copy
  holds it twice for the length of the copy.

#### How the 6x floor and the 10 MB band reach the path

- **MS-04's operation** is tier P's "Open and count a 23 MB file, CLI"
  (`docs/roadmap/P.md`), which MS-04.8 runs as the release `tpdf` of the same
  revision, a child per iteration, over a file the generator wrote once. Its
  command line is MS-04.1's to write, and this section fixes it: `tpdf info
  FILE` over an unlinearized file, so that P-05.4 opens it on demand and the
  rule keeps it streamed. Its time is then the process, the open, the windowed
  verdict, the page tree's walk for the count and page one: the work P-05.9's
  6x floor holds against the planning baseline. A linearized file, `probe` or a
  verdict flag would time a whole-file read, and floor an operation P-05 does
  not change. The open group's in-process "open and count the 23 MB image
  file" opens bytes already in memory, so it does not reach this path, and it
  is not the gated row.
- **MS-05's open case** re-executes its own test binary, so it cannot time
  `tpdf`; P-05.5 has its child do what `info` does to the file instead. The
  parent writes the file to disk, so the child holds no generated copy; the
  child includes `tools/tpdf/src/source.rs` by path, calls the CLI's own open,
  which applies the rule, counts the pages, and asserts that what it counted is
  streamed, which shows the rule kept it. The peak it reports is then the
  child's own resident floor and the chunks those reads kept, which P-05.9
  bands at 10 MB in both builds. The test binary's own floor is MS-05.3's to
  record.

#### The windowed verdict and the gate

- **The probe.** `complete_validation` pairs every candidate offset of every
  type-1 entry of the merged table with its entry, through the same
  `xref::offset_candidates` the buffered `validate` tries, the shifted offset
  first, sorts the pairs by offset and reads them in runs. A run is one read,
  from its first offset to `HEADER_PROBE_WINDOW` (32 bytes) past its last, and
  holds every offset whose probe ends within `VALIDATION_WINDOW` (64 KiB) of its
  first (`limits.rs`). Each header is lexed in the window by the three tokens
  `object_header_at` reads, through `xref::object_header_within`, which also
  says where the lexing stopped. A lexing that ran into the window's end short
  of the document's, behind a comment or white space or in a token that runs
  on, doubles the window until it stops inside, and the bytes a grown window
  holds serve the runs it reaches, so no byte is read twice. The reads go
  straight to the source through `Backing::read_transient` and are dropped with
  the run, so the chunk cache holds what it held, nothing is fetched whole and
  the pass warns nothing. An entry passes when some candidate's header names
  it, which is when the buffered pass finds one, and the level follows from the
  failures by the buffered pass's rule.
- **Kept once known.** The verdict is memoized on the document. A window the
  source could not supply ends the pass with nothing kept: `complete_validation`
  returns the provisional level, and the next call asks again.
- **The gate.** `load_uncached` asks the scanner only when
  `scanner_may_answer` agrees: always on a buffered document, whose
  `repair_index` answers from what its open built, and on a streamed one when
  the verdict is not `Trust`, worked out at that read if nothing has asked yet.
  A verdict cut short by a miss is a miss for the load, which is then not
  published.
- **The one ask left ungated.** The gate keeps one ask as it was: a type-1
  entry of a file whose header is not at byte 0. The walker keeps every entry
  as the file states it, from the header (it shifts only where a section
  starts), and only the buffered validation moves an entry to its shifted
  offset; a streamed document reads each where it is stated, misses its header,
  and finds the object through the scanner (*Bytes before `%PDF-`*, above). A
  shifted file decides `Trust`, so gating that ask on the verdict would read
  every object of such a file as null, and
  `a_linearized_file_behind_leading_junk_still_opens_from_its_head` would fail.
  A shifted file's free and absent entries are gated like any other's, and the
  rule reads a shifted file whole anyway, since it warned `HeaderNotAtStart`.
- **The predicate.** `CosDocument::answers_as_buffered` asks the three
  conditions in order of cost: `first_page_end()` is `None`, nothing is warned,
  and the verdict is `Trust`. A verdict the source could not supply answers
  false.
- **The tests**, in `crates/tinker-pdf-cos/src/doc.rs`:
  `windowed_validation_decides_what_the_buffered_open_decided`,
  `validation_keeps_no_byte_it_read`,
  `a_free_or_absent_entry_reads_as_the_buffered_open_reads_it` and
  `a_document_answers_as_buffered_only_when_nothing_could_part_them`.

### Milestones

1. P-05.2: a windowed verdict over the merged table that keeps nothing,
   memoized; the repair scanner asked only when that verdict is not `Trust`;
   `CosDocument::answers_as_buffered`.
2. P-05.3: `materialise` holds the file once.
3. P-05.4: the file source in `tools/tpdf`; the reading commands on demand by
   the rule; the rest read once, in place.
4. P-05.5: MS-05's open case through the CLI's own open.
5. P-05.6: one copy of a slice in the C ABI, Python and JavaScript, and none of
   an `Arc`.
6. P-05.7: Python's path open by the rule, and `is_streamed`.
7. P-05.8: `tpdf_document_answers_as_buffered`; .NET's path open by the rule,
   and `IsStreamed`.
8. P-05.9: the 6x floor and the 10 MB band held.

### Risks

| Risk | Mitigation |
|---|---|
| Annex F's head path builds a page without its ancestors' `/Rotate` and `/CropBox`, silently, for every streamed host, wasm's page one among them | Not this row's to fix, and named for a row of its own; P-05 keeps its hosts off that path by the rule's first condition, and P-05.4's equality over a linearized fixture whose page one inherits `/Rotate` fails if the condition goes |
| Linearized files are read whole by every path open, so the row's gain stops at the files that are not | A whole read is what such a file costs a buffered open; fixing the head path drops the condition |
| A clean file is read whole because its open raised some harmless warning | Whole is a buffered open's cost, read once; the predicate stays three observables rather than a list of kinds that would rot as kinds are added |
| The streamed branch parts from a buffered open in a way none of the three conditions sees | The equality is asserted, not argued: P-05.4 over a damaged, a shifted-header, a linearized-then-updated and a linearized fixture, P-05.7 and P-05.8 over the two files above, and P-05.2's predicate test, one case per condition |
| A local read fails after open in a binding, and a call reads that range as absent | The source never answers short of a failure, so absent means an I/O error, and the next call reads again; the CLI, which can, fails the file instead |
| The file changes under the reader | Appended bytes are never read, since the length is fixed; a truncation is a miss; a rewrite in place reads whatever is there at each read, as a buffered read of a file being rewritten does at its one read, and every parse of it is ruling 1's untrusted input |
| The windowed verdict costs a read per run of nearby object headers, so a file of many small objects pays close to one read of itself at open | Kept by nothing, against a whole-file read kept for the life of the document; the 23 MB file has a handful of objects, and a figure that moves names the change |
| The test binary's own resident floor, which nothing here has measured, leaves the 10 MB band less room than `tpdf`'s floor does | MS-05.3 records the case before P-05.9 holds it; a floor over the band is not a reason to widen the band |
| `CosDocument::bytes()` materialises a streamed document through `Backing::whole` rather than `fetch_whole`, so a save, a signature's byte range or a verdict over one pulls the whole file without the `WholeFileFetched` that a repair warns | Not this row's: P-05's hosts read whole for every verdict flag and every writing command, so none of them meets it; named for the row that owns ruling 10's declarations |
| The repair scanner's index takes time quadratic in a file's objects: `ScanIndex::build` looks for the next `trailer` from every object it finds, which on a file with one trailer at its end is a search to the end each time | Not this row's, and the gate makes it rarer: a streamed document builds the index only when a buffered open of the same bytes would, and the rule reads every such file whole, where it pays what a buffered open pays. Named for the row that owns the scanner |

## Dependencies

- The Annex F writer and its tests — `linearize.rs`, `tests/linearized.rs`.
- The determinism suite ([verification.md](../verification.md)) as the
  arrival-independence bar.
- The qpdf corpus (fetched by `xtask corpus-fetch`) as the source of
  linearized files this project did not write — inputs, which ruling 13
  keeps.
- No new crates and no external programs (ruling 13).

## Risks

| Risk | Mitigation |
|---|---|
| A miss half-poisons state (a `Null` cached because bytes were absent) | Invariant stated and tested: `SourceMiss` propagates, never converts to a value; the store publishes only completed parses (`a_miss_never_publishes_a_null_the_host_could_still_fix`, `crates/tinker-pdf-cos/tests/streaming.rs`) |
| Hint tables lie (attacker-controlled) and misdirect reads | Hints accelerate, never decide: `parse_at`'s header check stays mandatory; a failed check falls back to the generic path with a typed warning; all digit/width fields bounds-checked as `limits.rs` does elsewhere |
| Deferred validation makes `ladder_level()` access-pattern-dependent | The provisional/completed split is explicit API, both documented; the completion call restores the eager answer, and a test asserts provisional-then-complete equals whole-buffer open (`completing_validation_restores_the_verdict_a_buffer_would_have_given`) |
| Byte budgets rot as the open path evolves | Budgets are committed constants in `streaming_open.rs`, checked in CI |
| Chunk granularity choices leak into output | Chunking affects only which ranges are fetched, never values; the `ShreddedSource` run in `streaming_determinism.rs` is the regression trap |
| wasm retry loops livelock on a host that never feeds the range | The miss names its exact range; the facade result is a finite set of needs per call, and a test drives the loop to completion in a bounded number of host round trips |
