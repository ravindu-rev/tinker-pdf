# Parallelism

The engine spawns no thread today: `Document` is `Send + Sync`,
`Document::page` hands back an owned `Page`, and a caller who wants several
cores writes a pool over page indices itself ([architecture](../architecture.md),
"Concurrency"). Tier P's targets ask for more than a caller's pool can give —
one page drawn in bands on four threads, a save that compresses its streams
side by side, JPEG 2000 code-blocks decoded at once — and ruling 23's answer
to H-16 admits it: the engine's own pool, on the facade, behind
`Parallelism::Serial` by default, with `forbid(unsafe_code)` kept. This design
settles that pool, where each caller hands it work, how every parallel path
stays byte-identical to its serial run, and the amendment it makes to the
architecture's Concurrency section (row P-15).

## Scope

- **`Parallelism`**, `Serial` by default or `Threads(n)`, and the options
  struct of each operation that carries it.
- **The pool**: a PDF-free leaf crate, `tinker-pdf-pool`, on
  `std::thread::scope`, beneath `tinker-pdf-cos` and `tinker-pdf-filters`, so
  the writer and the JPEG 2000 decoder take it as a plain parameter
  (ruling 8).
- **Index order**: every result written to a pre-sized slot of its own and
  joined in index order, so no output depends on the thread count or on which
  thread finished first.
- **Serial on the caller where no thread starts**: wasm32, or a host that
  refuses a thread.
- **The parallel paths of P-15**: the page walk for render and text, the
  subset walk of a save, per-stream compression in the writer, and one page
  rendered in bands from its display list.
- **Single-flight fills** for the caches several threads share, and why
  `OnceLock` is not the answer.
- **Warnings**, which a parallel render reports exactly as its serial run.
- **Counted work**: what MS-01's tally counts of a pool, and how a guard
  proves work left the caller without flaking.
- **Memory**: pages in flight held at the thread count.
- **The amendment** to `docs/architecture.md`'s Concurrency section.

## Non-goals

- **A threaded wasm build.** H-44's answer is `+simd128` with a plain
  fallback and no threaded build; on wasm32 the pool runs everything on the
  caller.
- **`unsafe` and SIMD intrinsics.** H-16 keeps `#![forbid(unsafe_code)]` on
  every engine crate, the pool's included; speed comes from safe,
  auto-vectorisable code and from cores.
- **A persistent runtime.** No global pool, no thread that outlives the call
  that started it, no executor, no `async`: each call starts its workers in a
  scope and joins them before it returns.
- **Work stealing and nested pools.** One level of threads an operation; a
  pooled operation inside a worker runs on that worker.
- **The bindings and the C ABI.** Projecting `Parallelism` onto every
  surface is SU-12's, as each facade item is.
- **RC-16's serial band sink**, which renders a page in bands to a writer to
  bound memory, one band after another. The bands here are for speed and are
  stitched into one bitmap.
- **Deciding image runs in the page frame.** Ruling 5's named exception is
  RC-21's, behind H-15; the band cuts here keep it out of a stitched page
  without changing how a run is decided.
- **A different default.** `Serial` stays the default everywhere, and
  `tpdf render --jobs` keeps its default of 1 (H-42).

## Design

### `Parallelism`

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Parallelism {
    /// Everything on the caller's thread, in order.
    #[default]
    Serial,
    /// The operation's independent parts on this many worker threads the
    /// call starts and joins, the caller waiting; `Threads(0)` is `Serial`.
    Threads(usize),
}
```

It is a plain `Copy` value, so a leaf crate takes it in a parameter struct
with nothing of the facade in it, and the facade re-exports it as
`tinker_pdf::Parallelism` (ruling 11), as it re-exports `Stats`. `Threads(n)`
starts at most `n` workers and never more than the operation has parts, and
at most `MAX_THREADS` (256), so a caller's `usize::MAX` asks the host for a
bounded number of threads.

Each operation that can use threads carries it in the options it already
takes, defaulting to `Serial`:

| Options | Spent on | Step |
| --- | --- | --- |
| `RenderOptions::parallelism` | the pages of `Document::render_pages`; the bands of one page in `DisplayList::render` and `Page::render` | P-15.3, P-15.7 |
| `TextOptions::parallelism` | the pages of `Document::text_pages` | P-15.3 |
| `SaveOptions::parallelism` | the subset walk's pages, and handed to the writer | P-15.4 |
| `tinker_pdf_cos::WriteOptions::parallelism` | the streams the writer compresses | P-15.5 |
| P-10's JPEG 2000 decode request | code-blocks and synthesis | P-10.10, P-10.11 |

`Page::render` with `Threads(n)` records the page's display list for the
call and renders it in bands, which is the same bitmap as its direct render
(`determinism.rs`'s
`a_display_list_replays_every_fingerprinted_page_byte_for_byte` holds the
replay to the direct render) and, for a render that runs to its end, the same
warnings; a cancelled replay reports less of what the interpretation met than
a cancelled direct render, never more (`display.rs`'s module documentation). A page walk spends its threads on pages: each
page inside it renders on its worker, serially, whatever the options say,
because a map called on a worker runs there (below).

### The pool: `tinker-pdf-pool`

A leaf with no internal edge, `#![forbid(unsafe_code)]`, no third-party
crate. Its whole surface:

```rust
pub const MAX_THREADS: usize = 256;

/// What a crate hands its independent work to: the engine's pool, or a
/// test's own threads. `run` calls `job(i)` exactly once for each `i` in
/// `0..len`, on whatever threads it has, and returns when every call has.
pub trait Executor: Sync {
    fn run(&self, len: usize, job: &(dyn Fn(usize) + Sync));
}

impl Executor for Parallelism { /* below */ }

/// `f` over `0..len` on `executor`, each result in its own slot, returned
/// in index order.
pub fn map<T: Send>(executor: &dyn Executor, len: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T>;

/// `work` over `0..len` in waves of the worker count, each wave handed to
/// `deliver` in index order on the caller before the next starts.
pub fn waves<T: Send>(
    parallelism: Parallelism,
    len: usize,
    work: impl Fn(usize) -> T + Sync,
    deliver: impl FnMut(usize, T),
);

/// Whether this thread is one of a pool's workers.
pub fn on_worker() -> bool;
```

**`Parallelism::run`.** `Serial`, a `len` of 0 or 1, or a call on a worker
(`on_worker`) runs every `job` on the calling thread in index order. Otherwise
it starts `min(n, len, MAX_THREADS)` workers through
`std::thread::Builder::spawn_scoped` inside one `std::thread::scope`; each
marks itself in a thread-local, takes the next index from a shared atomic
counter and runs it until the counter passes `len`. The caller waits in the
scope's join. A worker that will not start — `spawn_scoped` returns an error
on wasm32 and on a host at its thread limit — is not a panic: the caller
joins the counter loop itself, so the share the missing worker would have
taken is taken on the caller, and no index is lost.

**`map`** gives each index a slot of its own — a slice of
`Mutex<Option<T>>` pre-sized to `len` — has `job(i)` fill slot `i`, and
collects the slots in index order. A slot an executor left empty, which only a wrong `Executor`
can do, is filled on the caller, so a test's executor that skips an index
cannot panic the crate that trusted it.

**`waves`** is the page walk's schedule. With `w` workers started, worker
`k` runs items `k`, `k + w`, … and every worker waits at a barrier at each
wave's start until the wave before it has been delivered; the caller takes
each finished wave's results in index order and hands them to `deliver`.
So the items alive at once are one wave's — at most `w` — the same on every
run whichever thread finishes first, which is what keeps a page walk's peak
steady (MS-25 found a counter-driven pool's peak moving with its schedule,
and `peak_memory.rs`'s `in_waves` is this schedule). A worker that will not
start leaves its column of the waves to the caller.

`on_worker` is a thread-local flag, not global state: the library still owns
no global mutable state beyond once-cells.

### Where each part runs, and why each output is the serial one

| Operation | Parts | Schedule | Why the output is the serial one |
| --- | --- | --- | --- |
| Page walk, render and text (P-15.3) | pages | `waves` | each page is its own operation, rendered exactly as `Page::render` renders it; results delivered in page order |
| Subset walk (P-15.4) | pages | `map` | each page walks into a `Usage` of its own; the usages fold in page order |
| Stream compression on save (P-15.5) | streams | `map` | each stream deflates alone; objects are serialised and encrypted in object order after |
| Bands of one page (P-15.7) | bands | `map` | each band is the page's own region (ruling 5), cut where no image run can span it |
| JPEG 2000 code-blocks (P-10.10) | code-blocks | `map` | each block decodes alone into its slot; the work budget is charged in index order first |

### The page walk

```rust
impl Document {
    /// Each of `pages` rendered as `options` asks, on the threads its
    /// `parallelism` names, handed to `each` in page order on this thread;
    /// a page the tree will not hand back is `None`.
    pub fn render_pages(
        &self,
        pages: impl IntoIterator<Item = u32>,
        options: &RenderOptions,
        each: impl FnMut(u32, Option<Bitmap>),
    );

    /// Each page's text, as `Page::text_counted` reads it, likewise.
    pub fn text_pages(
        &self,
        pages: impl IntoIterator<Item = u32>,
        options: &TextOptions,
        each: impl FnMut(u32, Option<TextPage>),
    );

    /// The walk both are: `work` runs for each page on a worker, and `each`
    /// takes what it returned, in page order, on this thread.
    pub fn walk_pages<T: Send>(
        &self,
        pages: impl IntoIterator<Item = u32>,
        parallelism: Parallelism,
        work: impl Fn(u32, Option<Page>) -> T + Sync,
        each: impl FnMut(u32, T),
    );
}
```

The walk shares the one `Document` by reference across its workers — `Sync`,
not a clone a worker — and each worker takes its pages through
`Document::page`, so the walk needs no lifetime work. A page is one
operation, as a direct render is: it builds its resources fresh, counts into
its own tally and reports what it met, so a page's bitmap, warnings and text
are the ones `Page::render` and `Page::text_counted` give it serially. Pages
come back in the order `pages` names them, never in completion order.
`tpdf render --jobs N` becomes `walk_pages` with `work` rendering the page and
writing its PNG on the worker, returning the page's report lines, so its
standard output is in page order as today, and the CLI's own scoped pool,
`render_pages` in `tools/tpdf/src/main.rs`, is deleted (P-15.8).

**Memory.** At most one wave of pages is alive: a worker holds the page it is
drawing, and a finished page waits only for the rest of its wave to be
delivered. `render_pages` hands each bitmap to `each` as its wave is
delivered, so twenty pages on four threads hold four bitmaps, not twenty,
which is what MS-05's band for twenty A4 pages on four jobs needs. A caller
that keeps every bitmap `each` hands it pays for them, as it would serially.

### Bands of one page

`DisplayList::render` with `Threads(n)` cuts the page into at most `n`
horizontal bands at rows of the page frame, renders each as a region
(`RenderOptions::region`) on the pool over the list's shared caches, and
writes each band's rows into one bitmap pre-sized to the page. Each band is
the page's own region in its one frame — the canvas stands at the band's
corner (`Canvas::origin`), never a translated frame — so ruling 5 makes every
band byte-equal to its rows of the serial page. A list that is not retained
(`DisplayList::is_retained`) renders as one band, on the caller.

**The cuts keep ruling 5's named exception out of a stitched page.** The one
decision ruling 5 leaves to the canvas is whether an image draw joins the run
of images held back so that abutting ones do not conflate: the renderer ends
the run when the draw *overlaps* what the run holds on the canvas
(`run_would_overlap` in `crates/tinker-pdf-render/src/lib.rs`), and a band's
canvas holds only its rows. A run is consecutive image draws — every device
operation that is not an image draw flushes it (`flush_run`) — so the
exception needs two or more image draws in one such sequence whose overlap
lies outside the band. The planner therefore reads the list before cutting:
for each maximal sequence of image draws with nothing the renderer flushes
on between them — in the content, its forms, groups and soft masks, and the
annotations when the render draws them — the rows its draws cover on the
page at the render's scale, from each draw's transform and the page's
(`image_bounds_in`'s rectangle).
A cut row is never inside the rows of a sequence of two or more draws. The
cuts start at `round(k × height ÷ n)` and each moves to the nearest row no
such sequence covers, between its neighbours; a cut with no such row is
dropped, and the page renders in fewer bands. A sequence wholly inside a band
is decided there exactly as on the page, since every draw of it is on that
band's canvas; in every other band its draws have no rectangle on the canvas
and end no run. A single image is a run of one and constrains nothing, so a
JPEG page cuts where it likes. The planner's rule for what ends a sequence is
the renderer's for what flushes a run — a `q`, a `Q` and a form boundary do
not, every call that draws anything else does — and a test holds the two to
each other one event kind at a time, so the planner cannot drift from the
renderer. A tiling pattern's cell is drawn into a canvas of its own, the
same in a band as on the page, so the runs inside it constrain no cut. A
scanned page assembled from strips is one sequence that covers the page, and
renders as one band until RC-21 decides runs in the page frame; then every
row is a cut and the planner's constraint goes.

The three-image page of
`an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`
is in A9's band leg (P-15.7) for that reason: its sequence covers rows the
even cut would fall in, so the cut moves past it and the stitched page is the
page.

### Single-flight fills

The caches several threads share fill in one shape today: look the key up
under the lock, release it, compute, take the lock again and insert
(`Caches` in `crates/tinker-pdf/src/resources.rs`; `DecodedStreams::read` in
`crates/tinker-pdf-cos/src/resolve.rs`, whose comment says two threads may
each run the decode). The values are deterministic, so two fills race to the
same answer; what they cost is the second decode, and a counted guard that
reads one decode would read two or four depending on the schedule.

`OnceLock` is not the answer, for the reason `crates/tinker-pdf-cos/src/store.rs`
gives at its head: same-thread re-entry deadlocks it, and re-entry is what a
`/Length` that points into its own stream produces. The object store answers
with a compare-and-swap and no waiting at all, which costs a wasted parse; a
decode is too dear to waste, so the caches wait, under rules that cannot
deadlock.

`tinker_pdf_cos::Flights<K, V>`, a map of slots under one mutex and a
condition variable, landed with its first user, the decoded streams a save's
pages share (P-15.4), and used by the facade's caches the renders of one
display list share (P-15.6) — images, font programs, outlines, ICC
transforms and form scopes:

1. A hit returns the published value.
2. A miss on an empty slot claims it for this thread and fills it outside the
   lock; the fill publishes its value and wakes the slot's waiters.
3. A miss on a slot another thread is filling **waits** for that fill —
   unless this thread is itself in the middle of a fill, of any key in any
   map, in which case it computes the value for itself and publishes
   nothing. A thread-local count of the fills a thread has open says which.
4. A same-thread re-entry for a key the thread is filling is rule 3's
   exception: it computes without waiting, as today's uncached read does.
5. A fill that unwinds releases its slot and wakes its waiters, so a waiter
   never waits on a fill that will not finish; the first of them claims the
   slot in turn.

Only a thread with no fill open ever waits, so no thread waits on a thread
that is waiting, and no cycle of waits can form, across any number of maps.
The price of rule 3 is a second computation where fills nest across threads
under contention; the values are the same, so only a count can see it, and a
guard that counts fills — four renders of one list decode a shared JPEG and a
Flate image once each — draws images whose fills open no other.

A fill never reads a render's cancel token: a cancelled render stops between
draws, never inside a fill, so what a fill publishes is what every render
would have computed. A cached image keeps the damage and the repairs its
decode reported (`CachedImage`), and a render that meets it in the cache —
a waiter included — reports them again, so a hit is exactly what a miss
computes, warnings included.

### Warnings equal the serial render's

A page walk's warnings are each page's own, and each page is rendered as it
is serially. A banded render's are the union of its bands', put in the serial
order:

- A render's warnings are deduplicated by value and kept in the order the
  render first met each, then the end-of-render ones (`Renderer::finish`:
  `UnreadableFont`, `GroupBudgetSpent`, `RecursiveContent`,
  `NestedBudgetSpent`, `Cancelled`) and the facade's (`DamagedImage`,
  `RepairedColorSpace`, `PageScaledDown`) in a fixed order
  (`Page::render_layer_with` in `crates/tinker-pdf/src/lib.rs`).
- Every band replays the same list, so the device calls arrive in the same
  order in every band. Each band stamps each warning it records with the
  ordinal of the device call it was first met at.
- The stitch takes every band's warnings, keeps each at the least ordinal any
  band met it at, and orders each part of the serial list by it — the
  warnings the renderer met, then its end-of-render ones in their fixed
  order, then the facade's, `DamagedImage` and `RepairedColorSpace` each by
  ordinal. That is the serial list: the page met each warning first at the
  first call that produced it on its canvas, the bands tile the canvas, and
  so the band that holds that call met it at that ordinal; a band that skips
  a draw skips one off its own canvas.
- The kinds that depend on the content alone, not on what a canvas holds —
  the admission's group budget, recursion, the page's scale — are met alike in
  every band and deduplicate to one. No band is clamped, since each lies inside
  the page, so a band reports no `RegionClamped` of its own; one the caller's
  region earned is the page's, reported once.

### Counted work, and proving work left the caller

MS-01's tally gains two counters, each counted where an item of a pooled
operation runs, into that operation's tally: `pool_items_on_workers`, an
item a worker ran, and `pool_items_on_caller`, one the caller ran. A page of
a walk is an operation of its own, so its bitmap's stats say where it ran; a
band, a stream of a save and a page of a subset walk count into the one
operation they are parts of. Neither depends on the
schedule: with `Threads(n)` and every worker started, every item runs on a
worker and the caller runs none; with `Serial`, or on wasm32-wasip1, every
item is the caller's. So `a_page_walk_on_four_threads_runs_no_page_on_the_caller`
and its band, subset and compression siblings assert those two figures
exactly, on every run. They never assert which worker took how many, which
is the schedule's.

Where a test must show distinct workers, it makes the items prove it rather
than counting a schedule: the items rendezvous at a barrier sized to the
thread count, so they can only finish once that many distinct threads each
hold one, and each records its thread beside the caller's. With `waves`,
worker `k` runs items `k`, `k + w`, …, so per-worker shares are fixed by the
arithmetic and a test may count them. A guard that counts hits and misses
reads a serial run, as every guard does today; the single-flight fills are
what make a shared cache's counts exact under threads.

A10 counts the allocations of the caller's thread. Every operation in its
roster runs `Serial`, its default, so its counts do not move; a pooled
operation is never an A10 case.

### The writer and the subset walk

**The subset walk** (`subset::collect` in `crates/tinker-pdf/src/subset.rs`)
walks each page and its annotation appearances on the pool into a `Usage` of
its own, over the save's one `Operation`, whose decoded streams become
single-flight (P-15.4), and folds the usages in page order: `Usage::refuse`
keeps the first reason and `Usage::direct` keys a program by the first font
that named it, so a fold in completion order would change the report.

**Compression** (`maybe_compress` in `crates/tinker-pdf-cos/src/write.rs`)
runs on the pool for the streams of the objects the writer writes, in object
order into pre-sized slots, a window at a time: the writer compresses a
window of streams, serialises that window's objects in order, and moves on,
so a rewrite's extra memory is one window's compressed bytes. The linearized
layout, which already holds every serialised object to measure its offsets,
compresses them all before laying out. Encryption stays after compression and
in object order, so every IV and every ciphertext is the serial writer's.
`Serial` runs today's path unchanged. The `tinker-pdf-cos` to
`tinker-pdf-pool` edge is declared in `ALLOWED` and `docs/architecture.md`
with it (P-15.5).

### JPEG 2000: P-10's seam

The decoder stays a leaf (ruling 8): beside its request it takes an
`&dyn tinker_pdf_pool::Executor`, which the facade fills with the caller's
`Parallelism` and a test fills with its own scoped threads, and nothing of
the facade crosses. The executor is a parameter of its own rather than a
field of the request, because the request is what the facade's image cache
keys on (P-10.8) and stays a plain, hashable value. The `tinker-pdf-filters`
to `tinker-pdf-pool` edge is declared with it (P-10.10), and the
architecture's note that `filters` depends on nothing then names `pool`,
which does not depend on anything. The decoder charges
its work budget for every block in index order before any block is decoded,
so a refusal names the same block at every thread count, and joins results in
index order ([jpx-speed](jpx-speed.md)).

### wasm

`std::thread::Builder::spawn_scoped` fails on wasm32, so every pooled
operation there runs on the caller, by the same start-failure path a native
host takes at its thread limit, and its output is the serial output. A9's
legs on `wasm32-wasip1` therefore cut and stitch on the caller and compare
against the serial render (P-15.9), and `pool_items_on_caller` holds every
item there.

### The `architecture.md` amendment (H-16, H-08)

P-15.3 replaces the Concurrency section's decision paragraph — "the pools
live outside the facade, and there is no threading API on `tinker-pdf`" —
with this, and keeps the rest of the section and its locked decisions
(ruling 23, H-08):

> **The engine's own pool, serial unless asked.** Every operation that can
> use threads takes a `Parallelism` in its options — a page walk, a render in
> bands, a save's subset walk and stream compression, a JPEG 2000 decode —
> and it is `Serial` by default, so no caller's behaviour changes unless it
> asks. `Threads(n)` runs the operation's independent parts on at most `n`
> threads that the call starts in a scope and joins before it returns: no
> persistent runtime, no global pool, no thread outliving its call. The pool
> is `tinker-pdf-pool`, a PDF-free leaf on `std::thread::scope` under
> `#![forbid(unsafe_code)]` (ruling 23, H-16). Results go to pre-sized slots
> and are joined in index order, so no output depends on the thread count or
> on which thread finished first, and guard A9 holds every page walk and
> every banded render byte-identical to its serial run at 2, 3, 4 and 7
> threads, warnings included. On wasm32 no thread starts and the same work
> runs on the caller, with the same result. The library owns no global
> mutable state beyond once-cells; a worker marks itself in a thread-local,
> so that a pooled operation inside another runs on the worker it is on. An
> embedder with an executor of its own keeps `Serial` and schedules pages
> itself, as before: `Document` stays `Send + Sync`, `Document::page` still
> returns an owned `Page`, and `FontProvider` stays `Send + Sync`.

and rewrites its list of what exercises the claim: `tinker_parity.rs`'s
`one_document_is_read_and_rendered_by_several_threads_at_once` still shares
one `&Document` across a caller's threads; `examples/parallel.rs` renders
every page through `Document::render_pages` with `Threads(n)` and checks each
against the serial page byte for byte; `tpdf render --jobs N` is the facade's
page walk.

## Milestones

| Step | What lands | Exit test |
| --- | --- | --- |
| P-15.2 | `tinker-pdf-pool`: `Parallelism`, `Executor`, `map`, `waves`, `on_worker`; registered as a leaf with no internal edge | `crates/tinker-pdf-pool/tests/pool.rs` `results_come_back_in_index_order_at_every_thread_count` (1, 2, 3, 4 and 7 threads, items made to finish in reverse order) and `four_threads_run_every_item_off_the_caller` (items rendezvous at a barrier of four and record their threads: four distinct, none the caller's); `src/lib.rs` `a_worker_that_will_not_start_leaves_its_share_to_the_caller` (an injected start failure loses no item); `cargo xtask check` green with the crate in `ALLOWED` |
| P-15.3 | `RenderOptions::parallelism`, `TextOptions::parallelism`; `Document::render_pages`, `text_pages` and `walk_pages` on `waves`; the two pool counters; the Concurrency amendment; `examples/parallel.rs` over the walk | `guards.rs` `a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads`, A9's page half flipped, in both builds; `parallelism.rs` `a_page_walk_on_four_threads_runs_no_page_on_the_caller`; `tinker_parity.rs`'s thread test; `cargo run -p tinker-pdf --example parallel` prints its byte-identity line |
| P-15.4 | `tinker_pdf_cos::Flights`; `DecodedStreams` single-flight; `subset::collect` page-parallel, folded in page order; `SaveOptions::parallelism` | `parallelism.rs` `a_subset_walk_on_2_3_4_and_7_threads_cuts_what_the_serial_walk_cuts` and `a_subset_walk_on_four_threads_walks_no_page_on_the_caller`; every `subset.rs` unit test and `the_default_save_cuts_the_programs_down`, in both builds |
| P-15.5 | `WriteOptions::parallelism`; windowed compression on the pool for a rewrite, an incremental update, object streams and the linearized layout; the cos to pool edge | `crates/tinker-pdf-cos/tests/parallel_save.rs` `saves_on_2_3_4_and_7_threads_are_the_serial_bytes`; `write.rs` `compression_on_four_threads_runs_on_no_caller_thread`; `maybe_compress`'s tests and `encrypt_on_save.rs` unchanged |
| P-15.6 | The facade's shared caches on `Flights`: images, font programs, outlines, ICC transforms, form scopes | `parallelism.rs` `four_renders_of_one_display_list_decode_a_shared_image_once` (four threads meet at the miss behind a barrier) and `a_same_thread_reentry_never_waits_on_itself`; `a_display_list_replays_every_fingerprinted_page_byte_for_byte` and `render_regions.rs` unchanged |
| P-15.7 | Bands: the planner's cuts, the stitch into one bitmap, the warning merge by first-met ordinal; `Page::render` with threads through a list | `guards.rs` `a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads`, A9's band half flipped: every `render_regions.rs` fixture, the named exception's three-image page included, and a text, a paths and a JPEG page, at 0.5x to 4x, turned and cropped, byte-identical with equal warnings, in both builds; `parallelism.rs` `a_four_band_render_runs_no_band_on_the_caller`; `render_regions.rs` unchanged |
| P-15.8 | `tpdf render --jobs N` on `walk_pages`; `render_pages` deleted | `tools/tpdf` `render_goes_through_the_facades_page_walk`; `jobs_changes_the_clock_and_not_one_byte_of_the_output`, `jobs_defaults_to_one_and_refuses_a_zero` and `a_page_that_will_not_write_is_counted_and_the_rest_still_render` unchanged; clippy green |
| P-15.9 | Determinism of the walk and the bands in K-07's roster | `determinism.rs` `parallel_and_banded_renders_are_the_fingerprints` in both builds, on Linux, Windows and `wasm32-wasip1`; `cargo xtask check` green; the `GOLDEN` table untouched |
| P-15.10a | The pool's speed rows through the facade's walk and bands, and the CLI's through the rebuilt `tpdf` | `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree` with the operations in their groups |
| P-15.10b | Their figures, in the timing phase | the figures committed and `cargo xtask baseline` holding them |
| P-15.11 | The row's floors and relations; MS-05's twenty pages on four jobs through the facade's walk | a local MS-02 paired run green with them; `peak_memory.rs` holding twenty pages at 150 dpi on four jobs within its band at or under 120 MB on Linux; A9 in `cargo test` on Linux and Windows and under `wasm32-wasip1` |

## Risks

| Risk | Mitigation |
| --- | --- |
| A parallel path's bytes depend on the schedule | Every result is written to its own slot and joined in index order; A9 compares every page walk and every banded render with its serial run at 2, 3, 4 and 7 threads, pixels and warnings, in both builds and on three targets (P-15.9) |
| A band cut splits an image run and the stitched page differs from the page (ruling 5's named exception) | No cut inside the rows of a sequence of two or more image draws; the planner shares the renderer's flush predicate; the exception's own page is in A9's band leg; RC-21 lifts the constraint |
| A page whose image sequence covers it, such as a scan of strips, gains nothing from bands | It renders as one band, correct and no slower than serial; tier P's band relation is held on the text, paths and JPEG pages, and RC-21 decides runs in the page frame |
| Two threads fill one cache and decode twice, so a counted guard flakes | `Flights` makes the fill single-flight; only a thread with no fill open waits, so there is no deadlock, and the guard's images open no nested fill, so its count is exact |
| A wait deadlocks on same-thread re-entry, as `OnceLock` would | A thread with a fill open never waits; re-entry computes, as today's uncached read does (`store.rs`'s reason) |
| A fill that panics leaves its waiters waiting forever | The fill's guard releases the slot and wakes the waiters when it unwinds; ruling 1 keeps fills from panicking at all |
| A banded render's warnings come out in another order, or twice | Stamped with the ordinal of the device call first met at, merged at the least ordinal, deduplicated by value; A9 compares the lists |
| A counted guard that counts per worker flakes with the schedule | Guards count only `pool_items_on_workers` and `pool_items_on_caller`, which the schedule cannot move; distinct workers are proved by a barrier in the items, or read from `waves`' fixed columns |
| A page walk holds every page in flight and breaks MS-05's band | `waves` keeps one wave alive and delivers it before the next starts; the band for twenty pages on four jobs is held in P-15.11 |
| A pooled operation inside a worker starts a pool of its own and oversubscribes | `on_worker` makes a map called on a worker run there; one level of threads an operation |
| The bands of a page all wait on one first decode, the decode serial on its worker | A list's caches outlive the render, so a list rendered again meets the decode in its cache; tier P times bands over a retained list, and a decode asked for on its own uses the pool (P-10.10) |
| A caller asks for more threads than the host allows | At most `MAX_THREADS`, never more than parts; a worker that will not start leaves its share to the caller |
| A10's allocation counts see only the caller's thread | Every A10 operation runs `Serial`; a pooled operation is never an A10 case |
| The writer's windowed compression changes a save's bytes or its encryption | Compression is per stream and order-free; objects are serialised and encrypted in object order after it; `parallel_save.rs` holds every save shape byte-identical, encrypted and not |
| Threads on wasm32 | None start; the start-failure path runs every share on the caller, and A9's wasm leg holds the result to the serial one |
