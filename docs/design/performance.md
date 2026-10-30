# Performance

The design holds an operation's speed two ways. A change that makes it do more
work than its input calls for fails `cargo test` on linux, windows, macos and
`wasm32-wasip1`, through counts read at the facade, which is where ruling 11
says a guard has to stand; one that makes it slower than the planning baseline
fails a paired job beside it.

This document is the design the speed rows share. Its first sections are
common to all of them — how a target gates, and which instrument holds each
kind of gate — and they are also MS-01's own design: the `Work` tally, the
doors that hand it out, the guard suite that reads it and the four legs that
run it. After them, each row with an instrument of its own has a section:
MS-02's paired job, MS-04's benchmark set and MS-07's corpus time ratchet.

## Scope

- **How a target gates**: tier P's six gate kinds — counted, factor,
  calibrated, relation, held and peak band — and the instrument that holds
  each (`docs/roadmap/P.md:22-61`), for every row of tier 0 and tier P that
  names a target.
- **The `Work` tally** (MS-01): plain counters one operation carries from its
  door to its end, the tally's home in the crate graph, and how it rides each
  context an operation runs in — the page walk, the filter chain, a page's
  resource scopes, the renderer and the editor.
- **The facade's stats doors**: render, text, save, merge, page lookup, image
  extraction (the decode door) and validation, each counting nothing unless
  asked.
- **The guard suite**, `crates/tinker-pdf/tests/guards.rs`: its generators,
  the A1–A10 roster naming each guard's test, its file and its owning row, the
  pinned counts that show the tally the same on every target, and its rule
  against clocks.
- **Allocations per page** for A10, counted by a test binary's allocator
  (MS-01.7) rather than by the tally.
- **The four legs** that run the suite: the three native `test` legs and
  `wasm-determinism`.
- **A section for each row with an instrument of its own**: MS-02, MS-04
  and MS-07.

## Non-goals

- **A clock in a test.** A count is a property of the binary and its input,
  which is why it can gate on a shared runner and under wasmtime. Time is
  gated by MS-02's paired job and checked absolutely by MS-03's pinned runner,
  never by `cargo test` (`docs/verification.md`, "Clocks: outside the suite,
  and not nowhere").
- **A tally that refuses work.** Refusing is what the budgets do —
  `ScriptBudget` (`crates/tinker-pdf-cos/src/form.rs:120`), the JBIG2 decoder's
  own `Work` (`crates/tinker-pdf-filters/src/jbig2.rs:1739`, private to that
  decoder and nothing to do with this one) and the caps in cos's `limits`. The
  tally only adds, and nothing but a door reads it.
- **`unsafe`.** None in the tally, the doors or the guards. The one
  `unsafe impl` in this design is MS-01.7's counting allocator, in a test
  binary, forwarding every call to `System` as `property_list_work.rs:27-31`
  and `svg_dash_memory.rs` already do; no engine crate is touched by it.
- **A count on `Document`, in a global or in a thread-local.** `Document` is
  `Send + Sync` and one is read by many threads at once
  (`docs/architecture.md`, Concurrency; `tinker_parity.rs:323`), so a count
  kept on it would mix two threads' lookups. The library owns no global
  mutable state beyond once-cells (`docs/architecture.md:304-306`), and a
  context is carried down the path that uses it rather than kept per thread,
  for the reason `ResolveCtx` gives (`crates/tinker-pdf-cos/src/store.rs:118-122`).
- **A counter in a leaf.** No leaf gains a `Work` or an edge (ruling 8;
  `tinker-pdf-filters` depends on nothing). A leaf with a count to give
  returns it as a value in its own vocabulary, as `Jbig2SymbolExtent::work`
  already does (`crates/tinker-pdf-filters/src/jbig2.rs:149-158`), and the
  facade adds it to the operation's tally.
- **The C ABI and the bindings.** MS-01 projects none of its doors; each is a
  'not projected' row in `docs/features/bindings.md` (ruling 11).
- **The `cfg(test)` step counters** some crates' unit tests read. They stay
  until a row that reads one
  moves it onto the tally — K-05 for the HTML tree builder, whose crate is a
  leaf and so reports its steps as a value — and no test loses the count it
  has in the meantime.
- **Wall-clock targets.** Factors, calibrations, relations and holds are
  MS-02's; peak memory is MS-05's; absolute figures are MS-03's.

## Design

### How a target gates, and which instrument holds it

Tier P states each target twice: as a figure measured on the planning machine
at `61adcde`, and in its **Gate** column as a form hosted CI checks and fails
on (`docs/roadmap/P.md:22-61`). The form is what gates. The figure is what
MS-03's pinned runner checks once it exists (H-110), a second and tighter check
that never decides by itself whether a target is met. Every "Today" figure is
re-derived by MS-04's benchmark set before it is quoted as current, and a
target gates as tier P states it until H-77 re-states it, never because of the
planning-time comparison that suggested the number, which is not re-run
(H-112).

| Gate kind | What it compares | Instrument | Where its numbers live |
| --- | --- | --- | --- |
| **counted** | a count of work against a bound, or one count at two sizes | MS-01's guard suite, in `cargo test` on the four targets | the bound in the guard; the guard's entry in the roster in `guards.rs`; the pinned counts in K-07's determinism roster |
| **factor** | the head's time over `61adcde`'s, paired and interleaved on one runner, against 1 ÷ the operation's committed floor | MS-02's paired job, over MS-04's benchmark set | the floors, placed by MS-02.1; a P row raises its operations' floors in the commit that closes it, and only H-78 lowers one |
| **calibrated** | an operation the baseline cannot run at all, as a fraction of a calibration operation the same job runs — `61adcde`'s text of 100 dense pages, 751 ms on the planning machine | MS-02's paired job | beside the floors |
| **relation** | two operations of one build, in one run, with no baseline | MS-02's paired job, on the head | beside the floors |
| **held** | a factor of 1: the operation may not lose time against the baseline | MS-02's paired job | every floor starts at 1 |
| **peak band** (MS-05) | an operation's peak resident set against a committed band | MS-05's Linux-only test, one first-party probe per case | MS-05's bands |

Two consequences shape everything below.

**A count and a time answer different questions, and a row owes both where
its Gate column names both.** A factor says an operation got faster on one
machine. A guard says it does no more work than its input calls for on every
machine, which is the property a regression breaks first and a wall clock on a
shared runner sees last. That is why each P row lands with its counted guard.

**A count gates where a time cannot.** A hosted runner's benchmarks swing
widely between runs of one revision (`docs/verification.md`, "Clocks: outside
the suite, and not nowhere"), and under wasmtime there is no benchmark job at
all; a count of the same work is the same number on all four targets.

### What a count is

A count is a number of steps of one named kind that one operation took: a
`/Pages` node read, a filter chain run, a font dictionary resolved, a cache
asked. A guard is a relation over counts — a bound (`page(i)` reads at most
depth × fan-out nodes) or one count at two sizes (the work of 4 000 pages at
most 4.4 times the work of 1 000). Neither depends on the machine, the build
profile or the target, and the guard suite shows that by pinning counts on all
four.

### The tally, and the value a door hands out

**Shape.** Two types, both in `crates/tinker-pdf-cos/src/work.rs`.

`Work` is the tally one operation carries. Its counters are `u64`s behind one
`Mutex`, beside a `BTreeMap` from stream reference to the decodes that stream
had, and every method that counts takes `&self` and adds with saturation.
`&self`, because the contexts it rides are reached through shared references:
the interpreter and the renderer hold a `&PageResources` and reach it through
`FontSource` and `GlyphSource`, whose methods take `&self`. A lock, because
those contexts are shared between threads: a `DisplayList` is `Send + Sync`
and keeps its `PageResources` (`crates/tinker-pdf/tests/display_list.rs:146-147`,
`crates/tinker-pdf/src/display.rs:92`), so a tally a scope holds must be
`Sync`. A lock that will not open drops the count rather than panicking, as the
caches beside it do (`crates/tinker-pdf/src/resources.rs:626-641`); it can be
poisoned only by a panic while it is held, and it is held for one addition or
one insertion. The map is a `BTreeMap` for ruling 4's reason: no count may
depend on hash order.

`Stats` is what a door hands out: `Copy`, `Default`, `PartialEq`, `Eq` and
`Debug`, `#[non_exhaustive]`, every field a public `u64`. Non-exhaustive so that
the counters later rows add — P-01's index entries copied, P-03's
deduplication decodes, P-06's masks rasterised, P-09's inverse-transform
output, P-10's levels decoded, P-04's scratch, P-15's threads — are additions
rather than breaks. `Stats::work()` is the total A2 compares. `Stats` adds with
`+`: counts sum, so a caller's loop of doors adds up to the loop's totals, and a
maximum stays the largest any one door saw. The facade re-exports it as
`tinker_pdf::Stats` from `crates/tinker-pdf/src/stats.rs`, which also holds the
doors' glue — re-exported under ruling 11, as `TextPage` and `DocumentEditor`
are.

Nothing reads a `Work` but a door turning it into `Stats`. That is the whole of
why a counted operation's output cannot differ from an uncounted one's, and
`stats_do_not_change_a_pixel` (`tests/stats.rs`) and MS-01.3's byte-identity
checks hold it.

**Home.** `tinker-pdf-cos`, because it is the lowest crate with an operation
context and three of the contexts the tally rides are in it: the page walk
(`crates/tinker-pdf-cos/src/pages.rs:279-351`), the two entry points that run
a stream's filter chain (`crates/tinker-pdf-cos/src/streams.rs:267`, `:290`)
with the `Resolve` views over them (`crates/tinker-pdf-cos/src/resolve.rs:27-46`),
and `DocumentEditor`, which a merge, a rotation, a split and a redaction all
run through before any save door is reached
(`crates/tinker-pdf-cos/src/edit/mod.rs:73`). Every other counting context is
above it. The graph rules out each alternative:

- **Not lower.** Every crate under cos is a leaf, and a leaf's API carries no
  PDF vocabulary (ruling 8); a tally of `/Pages` nodes and filter chains is
  nothing else. Nor can the tally sit in a crate under all the others:
  `tinker-pdf-css` must keep the empty dependency row that lets
  `unimplemented_property_does_not_build.rs` compile the crate's real source
  with a bare `rustc` (`xtask/src/main.rs:321-328`, `:626`).
- **Not in the facade.** The editor is cos's, and it has to carry the tally
  across a merge's imports before the facade's save door sees anything.
- **Not a new crate.** That is a node and its edges in `ALLOWED` and an
  amendment to `docs/architecture.md`, for a type whose every user already
  depends on cos.
- **Not on `Document`**, for the non-goal's reason.

`tinker-pdf-render` has no edge to cos (`ALLOWED`'s render row is content,
raster, font and colour), so the renderer keeps its two counts — glyphs shown,
outlines filled — in a plain struct of its own, as it already keeps
`missing_fonts: u32` (`crates/tinker-pdf-render/src/lib.rs:790`), and hands it
back with what `finish` returns (`:1124`); the door adds it to the render's
`Work`. A leaf reports the same way, as a value.

So cos's `Stats` names counts that layers above cos make: glyphs shown, cache
hits. That is the cost of one ledger an operation can carry through every
context instead of two, and it is taken knowingly. Each layer at or above cos
writes its own entries; cos itself counts only its walk, its filter chain and
its editor; each counter's doc comment names the layer that counts it, and a
counter joins `Stats` in the commit of the row that counts it.

**The counters**, each with the step that adds it, or *built* where `Stats`
already carries it.

| Counter | One unit is | Counted in | Step |
| --- | --- | --- | --- |
| `glyphs_shown` | a glyph the interpreter showed the renderer (`Device::show_glyph`, `crates/tinker-pdf-render/src/lib.rs:2847`) | the renderer | built |
| `outlines_filled` | a glyph outline the renderer built into a path and painted | the renderer | built |
| `font_dictionaries_resolved` | a font dictionary read: each font `cos_font::from_resources` returns when a scope is built (`crates/tinker-pdf/src/resources.rs:585`), and each `fonts::font_dict` lookup (`crates/tinker-pdf/src/fonts.rs:215`), which `type3_glyph` makes for every glyph (`resources.rs:1428`) | the facade's scopes | built |
| `images_decoded` | an image decoder run: an image XObject's cache miss, and every inline image, which is never cached (`resources.rs:222-224`) | the facade's scopes | built |
| a `_hits` and a `_misses` for each of the five caches `Caches` holds — font programs, images, ICC transforms, glyph outlines, form scopes (`resources.rs:63-101`) | a lookup that found an entry, a cached `None` included, or one that did not | the facade's scopes | built |
| `page_walks` | a walk of the page tree from its root | the facade's call sites, then `collect_upto` itself from MS-01.4 | MS-01.3 |
| `page_nodes_visited` | a `/Pages` tree node a lookup or a walk read, leaves included, and the one object the linearized or hinted first-page path reads (`pages.rs:459`, `:477`) | cos's page walk | MS-01.4 |
| `streams_decoded` | a run of a stream's filter chain, whatever it returned | cos's two filter-chain entry points and every `Resolve` view | MS-01.5 |
| `most_decodes_of_one_stream` | the most runs any one stream had in the operation — a maximum, the quantity A5 holds to one | the same | MS-01.5 |
| `bytes_decoded` | a byte a filter chain returned — a quantity | the same | MS-01.5 |

`work()` sums every counter that counts a step taken, and leaves out maxima and
quantities — `most_decodes_of_one_stream`, `bytes_decoded`, and every byte or
pixel figure a later row adds — because a byte total grows with the page and
can hide a step count that grows with the document. A page loop whose `page(i)`
reads `i + 1` nodes, over pages that decode 20 kB each, is 4.29 times as
much work at 4 000 pages as at 1 000 when its bytes are summed in, inside A2's
4.4, and 15.99 times without them. A row that adds a counter says which kind it
is.

### How the tally rides each operation context

An operation that was asked carries exactly one `Work`, from the door that
creates it to the door that reads it. A context that outlives a call holds it
as `Option<Arc<Work>>`; a call that only passes through takes `Option<&Work>`.
`None` is the state of every operation nobody asked about, and in it nothing
counts: no lock is taken, nothing is allocated, and the only cost is the
`Option` each counting site tests. It is the shape the budgets already have — a
value the operation owns, handed down the path that spends it
(`fields_within(doc, budget: &mut ScriptBudget)`,
`crates/tinker-pdf-cos/src/form.rs:322`) — except that a budget is spent and
can refuse, and a tally is only added to.

| Context | How it carries the tally | Step |
| --- | --- | --- |
| A page lookup: `pages::at` and the walk under it (`pages.rs:488-499`) | a `Work` on the door's stack, passed as `Option<&Work>` down `at`, `collect_upto` and `walk`, and to the linearized and hinted paths | MS-01.4 |
| A render: `Page::render` (`crates/tinker-pdf/src/lib.rs:2247`), `Page::render_form` and `Page::render_annotation` (`crates/tinker-pdf/src/render_part.rs:175`, `:230`), `DisplayList::render` (`display.rs:181`) | the render's `PageResources` holds it — built with it by `Page::render_layer` (`lib.rs:2269-2280`), handed it by `for_one_render` for a display list (`resources.rs:546`); `annots::draw` takes it for the appearances it draws; the page's content decode, made before the scope exists (`lib.rs:2248`), takes it too | built; MS-01.5 for decodes |
| A scope a scope makes: a pattern cell's and a form's own in a render (`resources.rs:1858-1868`, `:872-890`); and `own_scope`'s — a pattern cell's, a mask group's or a form's in the save's font walk, which only `subset.rs` calls (`resources.rs:750-764`) | inherits its maker's | built; MS-01.3 for `own_scope`, which no render reaches |
| A scope a cache keeps: a form's, in `form_scopes` (`resources.rs:856-869`) | kept without one, and handed out bound to the asker's, its caches shared so a hit stays a hit — a kept scope outlives the render that built it, and two renders of one display list may hold it at once | built |
| The renderer | its own plain counts, handed back by `finish_counted`, `finish`'s sibling, and added by the door | built |
| Text and image extraction | as a render: the content decode and the `PageResources` the door builds (`lib.rs:2533-2540`, `crates/tinker-pdf/src/images.rs:328-339`) | MS-01.3 |
| Validation | `pdfa::validate`, `pdfua::validate` and `pdfx::validate` take it, so their page walks (`crates/tinker-pdf/src/pdfa/content.rs:90`) and their decodes count | MS-01.3 |
| An editor | `DocumentEditor` holds it from `count_work` until it is dropped. Its own page and object operations count into it in cos — `page_refs`'s walk (`crates/tinker-pdf-cos/src/edit/page_ops.rs:16-24`), `import_page`'s `pages::collect(source)` (`:185`), its overlay's decodes (`edit/overlay.rs:48`) — and the facade's passes over it count into it too: a save's image and font passes (`crates/tinker-pdf/src/write.rs:337`; `subset.rs:553` builds a scope a page), a redaction. A rolled-back transaction keeps its counts: the work was done | MS-01.3, and MS-01.4 for its walks |
| A save | the editor's tally if it is counting, and one of the save's own if not; the serializer counts into whichever the save carries | MS-01.3, and MS-01.5 for the writer's decodes |
| A stream decode | `CosDocument::stream_decoded` and `stream_image_input` gain siblings that take `Option<&Work>`, and `Resolve` a counting view — a `Resolve` over any `Resolve` and a `&Work`, whose `stream_decoded` runs the inner one's and counts it — so code already written against the trait counts when handed the view, as a font's CMap reads are (`font::from_resources_in`, `crates/tinker-pdf-cos/src/font.rs:1110`; `read_cmap`, `:601-602`). The uncounted entry points delegate with `None`, so no caller outside an operation changes | MS-01.5 |

**Threads.** One `Work` per operation, never per document. Counts sum, so an
operation's own workers (P-15) add to one total in any order. Hits and misses
are the exception: two threads that miss one key before either fills it both
count a miss. Every guard and every pin reads a serial run, A9 compares pixels
and warnings rather than counts, and P-15.6's single-flight fills are what make
a shared cache's counts deterministic under threads — its own test counts one
decode across four renders.

### The doors

Two rules decide each door's shape, and the tree decides which applies.

**The ask.** A door whose options value every caller builds from a base takes
a flag on it, off by default: `RenderOptions::stats` and `SaveOptions::stats`.
Every `RenderOptions` literal in the tree but its own `Default`
(`lib.rs:705-718`) names a base, and so does every `SaveOptions` literal but
`tools/tpdf/src/writing.rs:564`, which gains one in MS-01.3. A door with no
options value, or one whose callers write it out whole, gets a counted sibling
instead. `TextOptions` is written out whole by its callers, so a field on it
breaks every one of them, which is the constraint FT-17.1 is already designing
around (`text_order.rs:128`).
`Document::page` and `Page::images` take no options at all, and the validators
take only the rule groups to run, a value each verdict reports back as what
ran and no place for a flag.

**The answer.** A flag's answer rides on the result as `stats: Option<Stats>`,
`None` unless asked. A counted sibling returns `(answer, Stats)`, because its
answer may be nothing at all — `page(i)` past the last page still walked the
tree — or a list, or a verdict whose fields other code compares.

| Door | Asked by | Answered by | Step |
| --- | --- | --- | --- |
| render: `Page::render`, `render_form`, `render_annotation`, `DisplayList::render` | `RenderOptions::stats: bool` | `Bitmap::stats: Option<Stats>` | built |
| text | `Page::text_counted(&self, &TextOptions)` | `(TextPage, Stats)` | MS-01.3 |
| save: `write::save` | `SaveOptions::stats: bool` | `Saved::stats: Option<Stats>` | MS-01.3 |
| merge: pages imported, then saved | `DocumentEditor::count_work(&mut self)` before the first import, and `SaveOptions::stats` | `Saved::stats`: everything the editor counted since `count_work`, the save's own work included | MS-01.3 |
| image extraction, the decode door | `Page::images_counted(&self)` | `(Vec<PageImage>, Stats)` | MS-01.3 |
| validation | `Document::validate_pdfa_counted(&self, PdfACoverage)`, and `validate_pdfua_counted` and `validate_pdfx_counted` alike | `(verdict, Stats)` | MS-01.3 |
| page lookup, the document door | `Document::page_counted(&self, index: u32)` | `(Option<Page>, Stats)` | MS-01.4 |

`Bitmap::stats` is a public field rather than a method: a method needs a field
the caller cannot see, and one private field would stop every hand-built
`Bitmap` compiling, where a public one asks each of them for `stats: None`;
nothing in the C ABI or the bindings builds one. `Saved::stats` joins `Saved`'s
other public fields; its one literal is the facade's own (`write.rs:368`).

The other doors that walk or decode — `Document::pages`,
`Document::page_count` and the rest — gain counted siblings in the rows that
first need them, by these two rules, and not before.

### The guard suite

`crates/tinker-pdf/tests/guards.rs` (MS-01.6) holds four things.

**Generators.** A balanced 10 000-page tree of stated depth and fan-out for A1
— ten thousand is 10⁴, so fan-out 10 over four levels is the shape under which
the per-lookup bound is small enough to mean something, and the generator
states its own shape and asserts it; documents of 1 000 and 4 000 pages for
each A2 operation; a 5 600-glyph page of about 80 distinct glyphs for A4; and
four documents of 100 pages to merge for A6. Each is written in memory, object by
object, by the generator itself, so a tree's shape is the generator's statement
rather than the writer's choice. Pages carry the least content that exercises
their leg, with their faces embedded — `tests/render_support`'s
`curvy_font` (`crates/tinker-pdf/tests/render_support/mod.rs:434`) — so the
default build and `bundled-fonts` count the same work. Nothing is read from
disk, because wasmtime gives the `wasm32-wasip1` leg no filesystem
(`.github/workflows/ci.yml:410-415`), and nothing is fetched.

**The roster.** A `const` table with one entry per guard test: its guard, its
test's name, the file it is in, the row that owns it, and whether it is landed
or owed. `every_guard_is_landed_or_owed` reads each file the roster names
through `include_str!`, since the wasm leg has no filesystem to read them from,
and fails when a landed entry is not an un-ignored `#[test]` there or an owed
one already is. An owning row lands its entry in the commit that fixes the
cause, flipping it from owed to landed. A guard that holds at the tip — A2's
subset, validation and split legs (`crates/tinker-pdf/src/subset.rs:552`,
`crates/tinker-pdf/src/pdfa/content.rs:90`) and A10's flat entries — lands
green. An entry a later row adds already landed, as P-18.5 adds A2's book, is
never owed.

**Pinned counts.** `the_tally_counts_the_same_on_every_target` holds the
`Stats` of a render, a text extraction, a save and a page walk over fixed
fixtures to committed values, and K-07's determinism roster makes every leg
grep it. The guards each compare counts on one target; the pins are what show
the counts every guard reads do not depend on which target read them. They are
goldens, so MS-01.6 pins the counters whose value is the fixture's rather than
the algorithm's — the glyphs a page shows, the bytes of streams each decoded
once, the nodes of a one-page tree — and says which it pinned and why. A pin a
fix has to move is a finding the owner rules on, as a moved fingerprint is.

**No clock.** `the_guard_suite_reads_no_clock` reads the suite's sources —
`guards.rs`, `guard_allocations.rs`, `guard_support/mod.rs` and every file the
roster names — and fails on `Instant::now` or `SystemTime::now`, as
`bounds_ledger.rs` holds the sources its ledger lists
(`crates/tinker-pdf/tests/bounds_ledger.rs:3229-3255`).

**A2's bound** is 1.1 times the ratio of the two sizes: 4.4 for 4 000 pages
over 1 000, as the exit states it, and 11 for P-18.5's 1 000-chapter book over
100. A2 compares `work()`, and a failing leg prints every counter's ratio beside
it, so the counter that grew is named by the failure rather than found after
it. A2 sees only what is counted: a superlinear loop that counts nothing passes
it, which is why P-01.1 records, for each leg, the site that is superlinear at
the tip and the counter that sees it.

**A10** counts allocations, which no tally sees without instrumenting every
allocation, so MS-01.7 counts them with a counting `#[global_allocator]` put
once in `crates/tinker-pdf/tests/guard_support/mod.rs`. A process-wide count
sees every thread's allocations, so either each counted test has a binary of
its own — the rule `property_list_work.rs:30-31` keeps — or the allocator
counts per thread. MS-01.7 settles which, and where A10's entries live follows
from it.

### The four targets

MS-01.8 runs the suite where ruling 4's targets already run.

- **Each native `test` leg** — `ubuntu-latest`, `windows-latest` and
  `macos-14` (`.github/workflows/ci.yml:23-28`) — runs
  `cargo test -p tinker-pdf --test guards` and `--test guard_allocations`, in
  the default build and with `--features bundled-fonts`, each through `tee`
  under `shell: bash` and `set -o pipefail`, and greps its log for a non-zero
  `test result: ok`, for `every_guard_is_landed_or_owed ... ok` and
  `the_tally_counts_the_same_on_every_target ... ok`, and for every landed
  roster entry by name: the determinism step's shape (`ci.yml:53-58`). A green
  tick from a filter that matched nothing, a `cfg` that dropped the target or
  an `#[ignore]` added one afternoon is the hazard "a check that can be absent
  is red, not green" names (`docs/verification.md`), and the greps are
  what close it.
- **`wasm-determinism`** runs the same two files on `wasm32-wasip1` under
  wasmtime, in the default build as its other steps do (`ci.yml:375-459`),
  with the same greps.
- **A guard in a file of its own** — A7's two, in `images.rs` and
  `jpx_images.rs` — joins every leg's commands as well as its greps, in its
  row's commit; a grep for a test its leg never ran fails, which is right, but
  only the command makes the leg pass.
- **The pinned counts** join K-07's determinism roster
  (`xtask/src/determinism.rs`, which K-07.1 creates), so `cargo xtask check`
  fails a leg that stops grepping them.

There are no threads on `wasm32-wasip1`: P-15's pool leaves the share of a
worker that will not start to the caller (P-15.2), so A9's legs there cut and
stitch on the caller and still compare against the serial render (P-15.9). The
1 000-, 4 000- and 10 000-page generators run in debug on every leg and under
wasmtime. A guard too slow for a leg is never ignored or shrunk below its page
counts, and what keeping it costs goes to the owner.

### The roster

Every entry MS-01.6 writes starts owed but A2's book, which P-18.5 adds
landed; A10's entries wait for MS-01.7. A guard lands in the step named, in
`crates/tinker-pdf/tests/guards.rs` unless another file is named.

| Guard | Clause, as the row's exit states it | Test | Owner, and the step it lands in |
| --- | --- | --- | --- |
| A1 | `page(i)` visits ≤ depth × fan-out nodes | `a1_page_lookup_reads_at_most_depth_times_fan_out_nodes` | P-01, P-01.2 |
| A1 | every page of a generated 10 000-page document ≤ 3 × 10 000 in total | `a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits` | P-01, P-01.3 |
| A2 | text: work(4 000 pages) ≤ 4.4 × work(1 000) | `a2_text_is_linear_in_pages` | P-01, P-01.3 |
| A2 | render-all at 36 dpi | `a2_render_all_at_36_dpi_is_linear_in_pages` | P-01, P-01.3 |
| A2 | rewrite | `a2_rewrite_is_linear_in_pages` | P-01, the step P-01.1 assigns |
| A2 | rotate | `a2_rotate_is_linear_in_pages` | P-01, P-01.4 |
| A2 | merge | `a2_merge_is_linear_in_pages` | P-01, P-01.4 |
| A2 | split | `a2_split_is_linear_in_pages` | P-01, P-01.4, holding at the tip |
| A2 | subset | `a2_subset_is_linear_in_pages` | P-01, P-01.3, holding at the tip |
| A2 | redaction | `a2_redaction_is_linear_in_pages` | P-01, P-01.4 |
| A2 | validation | `a2_validation_is_linear_in_pages` | P-01, P-01.3, holding at the tip |
| A2 | a 1 000-chapter book against 100 chapters, beyond the exit's nine | `a2_a_thousand_chapter_book_costs_at_most_eleven_times_a_hundred` | P-18, P-18.5, entered landed |
| A3 | font dictionaries resolved per page ≤ distinct `/Font` entries used | `a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses` | P-02, P-02.3 |
| A4 | glyph masks rasterised ≤ distinct (font, glyph, scale, phase) keys, hit ratio ≥ 98 % on a 5 600-glyph page | `a4_glyph_masks_rasterised_are_at_most_their_distinct_keys` | P-06, P-06.3 |
| A5 | inflate calls per stream ≤ 1 per operation: a save | `a5_a_save_decodes_each_stream_at_most_once` | P-03, P-03.2 |
| A5 | a render | `a5_a_render_decodes_each_stream_at_most_once` | P-03, P-03.2 |
| A5 | a text extraction | `a5_text_decodes_each_stream_at_most_once` | P-03, P-03.2 |
| A5 | image extraction, the decode door | `a5_image_extraction_decodes_each_stream_at_most_once` | P-03, P-03.2 |
| A5 | a merge | `a5_a_merge_decodes_each_stream_at_most_once` | P-03, P-03.4 |
| A6 | merge deduplication decompresses nothing unless encoded length and encoded hash collide | `a6_merge_deduplication_decodes_only_on_a_collision` | P-03, P-03.6, as H-137 is decided |
| A7 | IDCT output ≤ 4 × drawn pixels for a quarter-scale draw | `a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel`, in `crates/tinker-pdf/tests/images.rs` | P-09, P-09.8 |
| A7 | JPX levels decoded ≤ levels needed | `a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer`, in `crates/tinker-pdf/tests/jpx_images.rs` | P-10, P-10.8 |
| A8 | whole-image scratch on the PNG path ≤ 1 × the image | `a8_the_png_path_holds_at_most_one_image_of_scratch` | P-04, P-04.5 |
| A9 | page-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads | `a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads` | P-15, P-15.3 |
| A9 | band-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads | `a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads` | P-15, P-15.7 |
| A10 | allocations O(1) per page: page lookup | `a10_page_lookup_allocates_a_flat_amount_per_page` | as MS-01.7 settles: MS-01 in MS-01.7 if flat, else the row its cause is owed to |
| A10 | text | `a10_text_allocates_a_flat_amount_per_page` | as MS-01.7 settles |
| A10 | render-all at 36 dpi | `a10_render_all_at_36_dpi_allocates_a_flat_amount_per_page` | as MS-01.7 settles |

The names A7 and A9 carry, and A2's book, are the ones their rows' steps
already give; the rest are given here. A10's file is `guard_allocations.rs` or a
binary per entry, as MS-01.7 settles the counting allocator.

## Risks

| Risk | Mitigation |
| --- | --- |
| A count drifts from the work it names: a site moves, a path is added that does not count, a refactor counts twice | Every count is checked against a figure the test derives from its fixture |
| Counting changes what an operation does | Nothing reads a `Work` but a door; a kept scope handed out bound shares its caches, so a hit stays a hit; `stats_do_not_change_a_pixel` (pages and tiles) and MS-01.3's byte-identity checks hold it |
| Off is not free | An uncounted operation carries `None`: no lock, no allocation, one tested `Option` a site; MS-02's floors of 1 hold every operation to the baseline once that job exists |
| Two threads' counts mix | One `Work` an operation, never on `Document`, in a global or in a thread-local; an operation's own workers add to one total in any order |
| Hits and misses depend on interleaving when threads share a cache | Guards and pins read serial runs; A9 compares pixels and warnings, not counts; P-15.6's single-flight fills make a shared cache's counts deterministic and its test reads them |
| A count differs by target | `u64` counters with saturating adds, a `BTreeMap` for the per-stream decodes, no count of a hash-ordered walk; the pins are read on all four legs |
| The pins are goldens and the P rows exist to cut work | MS-01.6 pins counts that are the fixture's rather than the algorithm's, and says which; a pin a fix has to move is a finding the owner rules on, raised by the row that moves it |
| A2 reads only what is counted | A superlinear loop that counts nothing passes A2; P-01.1 records each leg's superlinear site at the tip and the counter that sees it, and a site outside P-01 is a finding, not absorbed |
| A byte total hides a step count | `work()` leaves out maxima and quantities, and a failing leg prints every counter's ratio |
| A guard is absent and reads as green | The roster checks each landed name is an un-ignored `#[test]` in its file, and every leg greps every landed name (`docs/verification.md`) |
| A guard is too slow for a leg, in debug or under wasmtime | Pages carry the least content that exercises their leg; a slow guard is never ignored or shrunk below its page counts, and what keeping it costs goes to the owner |
| A process-wide allocation count sees another test's allocations | One counted test a binary, or a per-thread count in `guard_support`; MS-01.7 settles which |
| A hand-built `Bitmap` stops compiling | The field is public, so a literal needs `stats: None`; nothing in the C ABI or the bindings builds one |
| `Stats` names counts made above cos | Each counter's doc comment names the layer that counts it, and cos counts only its walk, its filter chain and its editor; the alternative was two ledgers an operation, and an editor able to carry only one of them |

## MS-02 — A paired base-versus-head wall-clock gate on every pull request

In this design, a change that makes an operation of MS-04's set slower
fails the job that judges it, on whatever runner the job lands on, with no
time recorded for that runner to compare against. Base, head and the
planning baseline `61adcde` are built in one job on one runner, every
operation runs ten interleaved rounds on each, and each pair is judged on the
median of its ten paired ratios and a sign test. The same rounds hold the
head to the floors, calibrated targets and relations of tier P's Gate column
against `61adcde` (`docs/roadmap/P.md:29-55`).

### Scope

- **The paired protocol**: the three builds, a round and its interleaving,
  criterion's sampling in the rounds, and where each round's output is kept.
- **The inputs** both sides of a pair are timed on.
- **The verdict** on base and head: the median paired ratio against 1 + the
  operation's band, and the 8-of-10 sign test.
- **The gates against `61adcde`**: floors, held operations, operations paired
  with another, calibrated targets and relations.
- **The bands**, and **where the numbers live**: the gates and the A/A bands
  in `baseline.json`, with the runs they were read from.
- **The head's bench files on `61adcde`**: the `cfg`, the copy and the
  `tpdf` each build runs.
- **The triggers**, and **the 15-minute budget**: what each part costs,
  priced here on tier P's figures to show whether it can hold, and in MS-02.5
  on MS-04.10's measured set.

### Non-goals

- **An absolute figure.** A pair says that one build is slower than another
  on the runner both ran on, never that an operation takes more than so many
  milliseconds; that is MS-03's pinned runner (H-110). `bench.yml`'s weekly
  run and its bands stay MS-04's.
- **A second timing.** Every figure is criterion's, read from the files it
  wrote; nothing re-times anything, and no clock enters `cargo test`.
- **Memory, counts and wasm**, which are MS-05's, MS-01's and MS-10's.
- **A price.** What gives when the budget cannot hold is H-162's; a floor
  below 1, a band wider than the A/A rule and any loosened gate are H-78's; a
  target is H-77's and an input H-163's. This section gives the figures each
  is decided on and decides none of them.
- **The planning comparison.** H-112's figures stay outside the repository;
  nothing here reads or writes them.

### Design

#### Three builds on one runner

The job checks out three revisions side by side — the head, the base and
`61adcde` — and builds each in release in a target directory of its own: the
bench binaries of MS-04's thirteen group files, and `tpdf`, which the
CLI-marked operations run as a child resolved beside the bench binary
(MS-04, "The CLI operations"). The base and the head build as committed.
`61adcde` builds with the head's `crates/tinker-pdf/benches/` and the head's
`[[bench]]` entries in place of its own, under the `cfg` below, beside its
own `tpdf`. Every timed process of all three then runs on the one runner, so
each ratio is of two builds on one host minutes apart. The fifteen dispatches
each ratio is of two builds on one host minutes apart. `ubuntu-latest` is a
fleet of unlike hosts (`docs/verification.md`, "The named machine is
`ubuntu-latest`"); a ratio taken on one host divides that out.

thirteen, because `cargo bench -p tinker-pdf` also builds the library's own
unit tests as a bench target (its output lists `benches src/lib.rs`), which
no round runs; `tpdf` then builds in the same target directory, where it
reuses the engine the bench build compiled and compiles itself alone. The
job runs the binaries itself, never through `cargo bench`, whose start it
would otherwise pay once a process: `--no-run --message-format=json` names
each binary, and each runs with `--bench`, without which a criterion binary
runs its test mode (`criterion-0.5.1/src/lib.rs:963-967`, the version
`Cargo.lock:118-121` pins).

#### A round

A round times every operation once on each build, one operation a process:
the group's binary with `--bench --exact <id>`, criterion's whole-id match
(`criterion-0.5.1/src/lib.rs:1004-1006`, `:1139-1146`). The three builds'
processes for one operation run back to back, so the two sides of a pair lie
one operation's run apart rather than a round's. The build that goes first
rotates — head, base, `61adcde` in round 1; base, `61adcde`, head in round
2; and so on — so that none is always first, and the operations run in the
head's order, group by group as its files declare them. Each build's ids
come from its binaries' `--list` (`lib.rs:969-982`), which runs no routine
(`benchmark_group.rs:331-335`); an id one build lists and another does not
is timed where it is listed and reported, not judged (MS-02.2). Ten rounds
of 138 operations on three builds are 4 140 processes.

Each process writes into `CRITERION_HOME`, criterion's first choice of
output directory (`lib.rs:133-145`), set to `rounds/<build>/round-NN/`, so
each round of each build keeps criterion's `sample.json`, `estimates.json`
and `benchmark.json` under `<group>/<name>/new/`
(`analysis/mod.rs:159-181`, `:240-248`): the files `bench-check --paired`
reads and the job uploads. A directory a round also means criterion never
finds a saved `base` beside the operation, so it never runs a comparison of
its own (`analysis/mod.rs:183-188`). The job never passes
`--discard-baseline`, under which criterion writes none of these files
(`lib.rs:1150-1154`).

#### The rounds' sampling

| Setting | In the rounds | How it is set | Why |
| --- | --- | --- | --- |
| Samples | 10 | `--sample-size 10` | criterion's least (`criterion-0.5.1/src/lib.rs:1096`, `benchmark_group.rs:98`); the command line overrides each file's `criterion_group!` `config` (MS-04, "Sampling, and the 30 minutes") |
| Warm-up | one iteration | `--warm-up-time 0.001` | the warm-up loop stops at the first call whose time passes it (`routine.rs:257-282`) |
| Measurement | one iteration a sample | `--measurement-time 0.001` | flat sampling takes ⌈measurement time ÷ (samples × the warm-up's mean)⌉ iterations a sample (`lib.rs:1430-1454`), which is 1 for any operation over 0.1 ms |
| Mode | flat | by each group itself | no flag sets it (MS-04); linear sampling would take 55 iterations where flat takes 10 |
| Resamples | 1 000 | `--nresamples 1000` | the verdict reads the mean, which criterion takes from the sample before it resamples (`analysis/mod.rs:313`, `:323`); its default of 100 000 (`lib.rs:430`) is most of what a process costs beyond its iterations |
| Plots | none | `--noplot` | criterion 0.5.1 draws its report through gnuplot whenever one is on the path, whatever its features (`lib.rs:106-122`, `:421`, `:1029-1031`; `report.rs:328-330`), and the job's figures must not depend on the runner's image |

An operation of t therefore costs 11 t a build a round, one warm-up iteration
and ten samples of one, and 330 t over the job: H-162's "criterion's floor of
ten samples an operation a round", and the least criterion allows.

#### The inputs both sides are timed on

A paired ratio compares two times of one input, and two engines generating
"one" input write two. Every generator not pinned by formula writes through
the tree's own builder, encoders and editor (MS-04, "The inputs"), and
`scan()` through its renderer, so a head that changes any of them would be
timed on other bytes than its base, and `61adcde` on other bytes again. So the
job sets `TINKER_BENCH_FIXTURES` for
every process, and with it set `support/inputs.rs` reads every input —
generated, or committed and read by path — from that directory, writing it
there first if it is absent. Round 1 runs the head first, so the first
process to need an input is the head's: all three builds are timed on the
head's bytes, and the generators run once a job rather than once a process.
An input a base or `61adcde` process cannot read leaves that operation's
rounds missing on that side. MS-04 left this to MS-02 (its "Files" and
"Risks"); it widens MS-04's directory from the CLI operations' files to
every input, and MS-02.4 writes it.

A process that times one operation must build nothing for the rest of its
group, so an operation's input and any state it starts from are built the
first time criterion calls its routine and kept for the rest of the process
(MS-02.4 again).

#### The verdict

An operation's figure for a round is that round's `mean.point_estimate`
over the `calls` its side's committed list gives it (MS-04, "`baseline.json`'s
lists"): a call's time, so a change to `calls` moves neither side.
`sample.json` is read for its ten samples and their iteration counts, and a
round sampled otherwise is refused. A round's paired ratio is the head's
figure over the base's, and with b the operation's band:

- the **median** of the ten ratios is the mean of the fifth and the sixth;
- a round is **slower** when its ratio is above 1;
- the operation **fails** when its median is above 1 + b and at least 8 of
  its 10 rounds are slower.

Each half covers what the other cannot. On identical code the sign test
alone fires on 56 of the 1 024 equally likely patterns of ten rounds (45, 10
and 1 for eight, nine and ten slower), 5.5 % an operation, and if the 138
were independent the chance that none fired would be 0.945 to the 138th
power, 0.04 %: it is the band that keeps an A/A run green. The band alone
would fail an operation whose rounds split, some far above 1 and some below
it; the sign test asks that a slowdown show in eight rounds of ten. An
operation on one side only is reported, not judged, and a round missing on
either side is refused (MS-02.2).

#### The gates against `61adcde`

The head's rounds are paired a second time, with `61adcde`'s, and each
operation is held to what tier P's Gate column gives it
(`docs/roadmap/P.md:29-55`):

- **a floor** f, 1 until the P row that owns the operation raises it: the
  head-over-`61adcde` ratio against 1 ÷ f. **Held** is a floor of 1 that no
  row raises.
- **paired with another operation**, where `61adcde` cannot express the
  head's: that build runs, under the head's id, the operation the Gate column
  names — the full-size decode for a quarter-scale one (MS-04, "What later
  rows add") — and the ratio is held to 1 ÷ the factor as a floor is.
- **calibrated**, where `61adcde` lacks the capability altogether: the head's
  figure over `61adcde`'s `calibration/text of 100 dense pages` in the same
  round, against the target over 751 ms (`P.md:43-47`), so 0.5 ms reads as at
  most 1/1 502.
- **a relation**: two operations of the head's group in the same round, the
  ratio of their per-call figures against the bound, in its direction.

Tier P writes the factor as the median against 1 ÷ the floor
(`P.md:32-35`) and the others as bounds. Read that way, a floor of 1 cannot
be met by a head that runs an operation at `61adcde`'s speed: with the two
speeds equal, the median of ten ratios lands above 1 in about half of all
runs, by symmetry, so on a day when most of the set runs where `61adcde` left
it, each of those operations fails about one run in two and the job
cannot be read green. This design holds each gate to the regression pair's rule
instead: the median beyond the bound by more than the operation's band, with
at least eight of the ten rounds beyond the bound, a relation taking the
wider of its two operations' bands. That loosens tier P's text by the band,
and loosening a gate is H-78's, as H-77's default says; MS-02.7 asks for it
with the bands H-109 approves, and until it is decided the job prints both
readings.

An operation whose gate names a baseline side that `61adcde`'s rounds lack
is refused, not reported: the gate data says it can be judged, which MS-02.4's
`every_operation_has_a_baseline_side_or_a_calibration` holds, so its absence
is a fault of the mapping, and a check that can be absent is red
(`docs/verification.md`). A head slower than `61adcde` beyond its band
on the day the floors are set is a finding against that operation's row; a
floor below 1 is H-78's to grant, and no step lowers one.

#### The bands

An A/A dispatch's **paired spread** for an operation is how far its median
paired ratio lies from 1, in per cent: the statistic the verdict reads,
taken on two builds of one revision. The operation's band is the worst
spread of fifteen such dispatches, half again as wide, rounded up to the next
5 % and never under 5 % — `bench.yml`'s rule, for the reason
`docs/verification.md` gives under "The named machine is `ubuntu-latest`": a
spread over fifteen runs is a lower bound that has not converged. A band set so
clears the medians of its own fifteen runs by arithmetic, which is no evidence
that the band is wide enough; only a run no band was fitted to is. An A/A
dispatch needs no `61adcde` rounds, since a band is read from base and head
alone. H-109 authorises the dispatches and approves the bands; a band wider
than the rule gives is H-78's.

#### Where the numbers live

| What | Where | Written by | Moved by |
| --- | --- | --- | --- |
| Each operation's floor, 1 to start, or held | its entry in `baseline.json`'s group list, as `gate`, beside its unit (MS-04, "`baseline.json`'s lists") | MS-02.3, from tier P's Gate column | raised by a P row in its closing commit; lowered only by H-78 |
| The operation `61adcde` runs in another's place | the same `gate`, naming it; the bench files' `cfg` arm runs it | MS-02.3 and MS-02.4 | the row that adds the operation |
| Calibrated targets | the same `gate`, in milliseconds; the calibration operation and its 751 ms named once, ahead of the groups | MS-02.3 | re-stated by H-77; loosened by H-78 |
| Relations | the same `gate`: the other operation of the group, the bound and its direction | MS-02.3 | as calibrated targets |
| A/A bands | each operation's entry: its spread and its band; the fifteen run ids once, beside the calibration | MS-02.7 | approved by H-109; widened by H-78 |

```json
"calibration": { "operation": "calibration/text of 100 dense pages",
                 "planning_ms": 751 },
"paired_runs": { "runner": "ubuntu-latest", "runs": [ ... ] },
"groups": [
  {
    "group": "open",
    "operations": [
      { "name": "every page's size, 10000 pages", "unit": "pages",
        "per_iteration": 10000, "calls": 1,
        "gate": { "floor": 1,
                  "relations": [ { "to": "every page's size, 3000 pages",
                                   "at_most": 3.6 } ] },
        "paired": { "spread_percent": 3.4, "band_percent": 10 } }
    ]
  }
],
```

The block shows the shape, not measured figures. The reader cuts entries
at `"machine":` and `"id":` (`xtask/src/bench.rs:243`, `:272`), which no key
here contains. `cargo xtask baseline` refuses an operation with no gate and a
relation naming an unknown operation (MS-02.3), and a band without its runs
and spread (MS-02.7). H-112's planning comparison is in none of these
places.

#### The head's bench files on `61adcde`

- **The `cfg`**, `tinker_planning_baseline`, is set for that build alone,
  through `RUSTFLAGS`, which then drops the workflow's `-D warnings`
  (`.github/workflows/ci.yml:20`) as the fuzz steps' `RUSTFLAGS: ''` does
  (`:212`, `:234`): `61adcde` is frozen, and a lint a later stable adds must
  not fail it. Its `tpdf` builds in the same target directory under the same
  flags, so the engine compiles once for both. Under the `cfg` each bench
  file's operation bodies call `61adcde`'s facade; the generators need no
  arm, calling only what that revision exposes (MS-04, "Files").
- **The declaration.** The head declares the `cfg` to `check-cfg` in
  `crates/tinker-pdf/Cargo.toml`, or `unexpected_cfgs` fails its clippy
  under `-D warnings`. No `Cargo.toml` in the tree has a `[lints]` table or
  declares a `cfg`.
- **The copy.** The head's `crates/tinker-pdf/benches/` replaces
  `61adcde`'s whole, `engine.rs` and `baseline.json` with it, and the head's
  `[[bench]]` entries replace that revision's one
  (`61adcde:crates/tinker-pdf/Cargo.toml:114-116`). No other code of the
  head's enters that checkout, so every line the baseline's rounds time
  outside the bench files is `61adcde`'s.
- **Its `tpdf`.** Every command line MS-04 gives its CLI operations runs as
  written on `61adcde`'s `tpdf`, which already takes `--json`, `--dpi`,
  `--jobs`, `--font-policy` and `--entropy`
  (`61adcde:tools/tpdf/src/main.rs:36-53`) and opens a book through the
  container sniffing in `Document::open_with`
  (`61adcde:crates/tinker-pdf/src/lib.rs:1395-1408`).

#### The triggers

- **Every pull request**, H-111's default: head the merge commit the event
  checks out, base that commit's first parent, the target branch's tip it was
  made on, so the pair is the pull request's change and nothing else.
- **Every push to `develop`**: every change goes to `develop` with no pull
  request, so this is the trigger that reaches one. Base is the tip the
  push replaced (`github.event.before`) and head the push's tip, so a push of
  several commits is judged as one change.
- **A dispatch for A/A runs**, base and head both the dispatched revision,
  checked out and built twice exactly as a real pair's are, so a band
  measures everything two builds differ by except the change. `ci.yml` has
  no `workflow_dispatch` (`.github/workflows/ci.yml:8-16`), and one added
  there dispatches every other job in it as well unless each is told not to
  run on it; a workflow of its own needs no such line.
- **Not** on `main`, whose pushes are the owner's merges of commits already
  judged on `develop`, and not on a schedule: `bench.yml`'s weekly run
  is unchanged.

No run cancels another: each push's run is the one that judges it, as
`ci.yml` says of its own (`:12-13`). Without committed bands the job
prints every verdict and fails on none; with them it fails as they say. A
base or `61adcde` that cannot be checked out or built fails the job, since a
pair that cannot be judged is red, not green (`docs/verification.md`).

#### The 15 minutes

The row gives the job 15 minutes for three release builds and thirty runs of
the set. Its parts, what each costs, and where MS-02.5 reads each from:

| Part | Cost | By tier P's figures | MS-02.5 reads it from |
| --- | --- | --- | --- |
| Three checkouts, the toolchain and the cache | once | — | the job's log |
| Three release builds of the thirteen bench binaries and `tpdf` | 3 × b, each bench binary a link under the release profile's thin LTO and single codegen unit (`Cargo.toml:86-88`) | — | the job's log |
| 10 rounds × 3 builds × 138 operations, 11 iterations each | 330 × Σt | Σt, the time of one iteration of every operation, 207.7 s: 1 142 min | MS-04.10's figures |
| Setup inside an iteration, untimed | 330 × s | s = 652 ms, the 72-dpi first render of `a second render of the JPEG page at 300 dpi` (`iter_batched`): 3.6 min | MS-04.10's figures |
| 4 140 processes: start, criterion's analysis, its files | 4 140 × p | — | the job's log |
| Two calls of `bench-check --paired`, and the upload | once | — | the job's log |

Σt takes MS-04's figures and the estimates its section makes: tier P's
Today at `61adcde` with each operation's `calls`; an in-process twin at its
CLI figure; every page's size over 10 000 pages at MS-04's 28 500 ms; an
operation tier P states only against another at what that statement gives
it — the analysis group at its relations' bounds over a 751 ms text, the
stroked curves at their page's figure, the second JPEG render at the
first's, the four bands at their relation's bound — and the standard-14 text
and the annotated page at the dense text's and the text page's; the layout
group's unmeasured operations at three times their targets; any other
operation with a target and no Today at its target; a range at its top; and
an operation with neither at 25 ms, the size MS-04's `calls` give one. The
base and the head are taken at `61adcde`'s figures, since no P row that moves
one has landed.

| Group | Operations | Σt | Its rounds in the job |
| --- | ---: | ---: | ---: |
| text | 12 | 79.9 s | 439 min |
| render | 33 | 39.5 s | 217 min |
| open | 6 | 31.3 s | 172 min |
| layout | 39 | 17.6 s | 97 min |
| write | 16 | 15.3 s | 84 min |
| analysis | 8 | 11.6 s | 64 min |
| decode | 9 | 10.4 s | 57 min |
| encode | 4 | 1.0 s | 5.3 min |
| calibration | 1 | 0.8 s | 4.1 min |
| fonts | 7 | 0.2 s | 1.0 min |
| sign | 3 | 0.2 s | 0.8 min |
| the set | 138 | 207.7 s | 1 142 min |

**The budget cannot hold, and the figures say by how much.**

- **As the row is written**, the rounds alone need 19.0 hours against 15
  minutes, 76 times over before a build. Twelve operations would each take
  longer than the whole job by themselves — `rewrite 3000 pages` at 5.48 s
  needs 330 × 5.48 s, 30.1 minutes — and the three that read 10 000 pages
  are 100.5 s of Σt's 207.7.
- **At any figures inside MS-04's window**, 138 operations of 20 ms or more
  cost at least 138 × 330 × 20 ms, 15.2 minutes of rounds, so no answer to
  H-163 that keeps the set inside the window fits three builds into 15
  minutes.
- **The planning baseline's third**, 110 × Σt, is 6.3 hours and never
  shrinks: a P row makes the head faster, and `61adcde` stays what it was.

H-162 decides what gives. Each of its options alone, at the same figures:

| What gives | Rounds | Against the budget |
| --- | ---: | --- |
| The number of rounds: one | 33 × Σt, 1.9 h | and a sign test of 8 in 10 needs ten |
| The planning baseline's half, to a job of its own | 220 × Σt, 12.7 h, in each of two jobs | the baseline's job runs the head beside it, since a pair is one runner's |
| The operations the pull-request job runs: the 82 inside 20–300 ms | 330 × 8.3 s, 45.7 min | three times the whole 15 minutes, before a build |
| The 15 minutes | 19.0 h | past six hours, a job's default limit (`.github/workflows/corpus.yml:24`) and the most a hosted job may run |

None fits alone. A shape beyond the four — a matrix of jobs, each judging a
share of the operations with all three builds on its own runner, which keeps
each pair on one runner but not the set in one job — would need about 115
jobs a run at these figures. Which combination, if any, is H-162's.
MS-02.5 re-prices every line and every option on MS-04.10's measured
figures before H-162 is decided, never on tier P's column, and MS-02.6
builds what H-162 decides.

### Risks

| Risk | Mitigation |
| --- | --- |
| The job cannot finish in 15 minutes | The budget above gives the figures — 19.0 hours of rounds at tier P's, at least 15.2 minutes at any figures inside the window — and H-162 decides on MS-02.5's prices from MS-04.10's figures |
| A pair times two inputs, because two engines wrote them | Every input through `TINKER_BENCH_FIXTURES`, written by the head first and read by all three builds |
| `ubuntu-latest` is a fleet, and two runs land on unlike hosts | Both sides of every pair run in one job on one runner, and the fleet's split divides out of a ratio |
| The host drifts while a round runs | An operation's three processes run back to back, so a pair spans one operation's run, and the first build rotates by round |
| The sign test fires on noise | It fires on 5.5 % of operations on identical code, so an operation fails only with its median past the band as well |
| A floor of 1 fails about half the runs of a head that matches `61adcde` | The gates take the band and the sign test, H-78's to approve; until then the job prints both readings |
| A head is slower than `61adcde` on the day the floors are set | A finding against that operation's row; the floor stays 1 unless H-78 grants less |
| An operation the base or `61adcde` cannot express | One the base lacks is reported, not judged; one `61adcde` lacks runs the operation its Gate column pairs it with, or is calibrated (MS-02.4) |
| `61adcde` cannot read an input the head writes | Its rounds of that operation are missing, and an operation whose gate names a baseline side it lacks is refused, so the job fails on MS-02.4's mapping rather than passing the operation unjudged |
| The calibration moves within a round | Each calibrated ratio is taken against the same round's calibration, and the band absorbs what moves between the two |
| A lint a later stable adds fails the frozen baseline | Its build drops `-D warnings`, as the fuzz steps do (`ci.yml:212`, `:234`) |
| A round writes nothing and reads as a pass | A directory a round and never `--discard-baseline`; the guard counts every operation in all ten rounds of every build (MS-02.6) |
| A base or `61adcde` that will not build lets the job pass on the head alone | The job fails: a pair that cannot be judged is red (`docs/verification.md`) |
| Criterion's own cost swamps the rounds | `--nresamples 1000` and `--noplot`, which take out most of what a process costs beyond its iterations |

## MS-04 — The benchmark set, replacing the seven

In this design, `cargo bench` times every operation of tier P's target
table that the tree can run, and the row's own, in thirteen groups of one file
each: 138 operations over inputs the benches generate or the tree holds, each
held by `bench.yml` to a band measured on `ubuntu-latest`. MS-02's paired job,
MS-05's probe, MS-09's thread group, MS-10's wasm ratio and every P row's
floor are built on this set, and each later capability row adds its operation
to it in the commit that adds the capability.

### Scope

- **The thirteen groups**: open, text, analysis, render, encode, decode,
  write, sign, layout, fonts, scripts, OCR and calibration, one bench file
  each over a shared support module; scripts and OCR arrive with the rows that
  add their operations.
- **Every operation** of tier P's table (`docs/roadmap/P.md:63-135`) and of
  the row's Item whose capability exists: its criterion id, its unit and what
  one iteration covers, its input beside the input the table states, and where
  that input comes from.
- **The inputs the tree cannot write** — a progressive JPEG and two JPEG 2000
  codestreams at 2480×3508, and a face covering Devanagari — and the one
  encoder the benches own, LZW.
- **The CLI-marked operations**, timed by running this revision's `tpdf`.
- **`baseline.json`'s per-group lists**, criterion's sampling per group,
  `bench.yml`'s jobs and how a run fits its 30 minutes, the 20–300 ms window
  and how it is checked, where the two level-6 byte counts live, how the seven
  retire, and the operations each later row adds.

### Non-goals

- **A gate on a push or a pull request.** `bench.yml` keeps its dispatch and
  its weekly cron (`bench.yml:24-40`); MS-02's paired job is the gate.
- **A changed input.** Every operation runs on the input tier P's table
  states. Which gives for one that cannot fit 20–300 ms at that input is the
  owner's (H-163); until it is decided the operation keeps its input and is
  named outside the window.
- **Floors, calibrations and relations.** What an operation is lives here;
  how it gates is MS-02.3's to commit beside it, and the thread floors are
  MS-09.3's.
- **Memory and wasm.** MS-05's probe, `tests/peak_memory.rs`, builds its
  cases from these inputs and keeps its own bands. MS-10.1 builds this set for
  `wasm32-wasip1` and runs it under a wasm runtime, less the operations it
  lists as unable to run there — a child, a thread — so every file here
  compiles for that target and reads its committed inputs by path, which the
  runtime's directory grant reaches.
- **A number an outside program measures.** The level-6 byte counts are
  brought in once (H-112), and no outside program runs while the set does
  (ruling 13): a fixture an outside program made once is bytes with its
  provenance.
- **A capability the tree lacks.** Its row adds its operation, in the commit
  that adds the capability (the table under "What later rows add").

### Design

#### Files

```text
crates/tinker-pdf/benches/
  support/mod.rs     the harness: each group's sampling, the iteration shapes,
                     the fixture directory, what an operation declares
  support/inputs.rs  every generator, and the two level-6 byte counts
  support/cli.rs     the one spawner: this revision's tpdf, as a child
  open.rs  text.rs  analysis.rs  render.rs  encode.rs  decode.rs  write.rs
  sign.rs  layout.rs  fonts.rs  scripts.rs  ocr.rs  calibration.rs
  data/              the inputs the tree cannot write, and README.md
  baseline.json
```

One `[[bench]]` per group file, `harness = false`. `inputs.rs`
holds no criterion type, no clock and no spawn, so a test reaches it by path
alone — MS-05's probe, `tests/peak_memory.rs`, for its cases and P-04's
`deflate_size.rs` for the level-6 counts (P-04.1's exit) — without compiling
the harness or the child. `cli.rs` is the only file here that calls `Command::new`, so it is the
one `SPAWNERS` row `cargo xtask oracles` needs (`xtask/src/main.rs:984-1048`,
`:1076-1079`). Every input is generated in memory except what `data/` and
the tree already hold, and a CLI operation's files are written once a process
into `CARGO_TARGET_TMPDIR`'s `bench-fixtures/`, a directory cargo gives every
bench target, or into the one `TINKER_BENCH_FIXTURES` names: that is how a
paired job hands two builds the same bytes (MS-02, "The inputs both sides
are timed on").

The generators call only what `61adcde` already exposes — the builder calls
used below (`archival`, `tagged`, `output_intent`, `add_embedded_font`,
`add_base_font`, `add_image`, `add_icc_color_space`, `add_tiling_pattern`,
`set_info`, `raw`), the encoders (`jpeg_encode`, `ccitt_g4_encode`,
`jbig2_generic_encode`, `png_encode`, `deflate`, `zlib_compress`),
`BilevelCodec::Jbig2Generic`, `add_annotation`, `annot::square`, `FromHtml`
and `Document::open_markdown` are all public at that revision — so MS-02.4's
mapping onto the planning baseline covers operation bodies only.
`PageBuilder::text_pieces` is not in `61adcde`, which is why producer-style
lines are
written through `raw`.

#### An operation: its id, its unit and its iteration

**The id** is criterion's, `<group>/<name>`: the group file opens a
`benchmark_group` named for its group and names each operation in it.
`bench.rs` keys every estimate by the directory criterion wrote it under
(`xtask/src/bench.rs:359-385`), and criterion 0.5.1 makes each part of that
path filename-safe — it replaces `?`, `"`, `/`, `\`, `*`, `<`, `>`, `:`, `|`
and `^` with `_` and cuts the part at 64 bytes, and on Windows, where no job
here runs, it lowercases it — so a name that breaks either rule is recorded
under another key, reported "new, with no recorded figure" and never held to
its band. Every name is therefore ASCII, at most 64 bytes, free of those ten
characters (PDF-A, not PDF/A; 9-7 is spelled irreversible), and writes its
numbers unspaced ("3000 pages"). A name says what the input is and never how
often it is repeated.

**The unit** is what an iteration processes: pages, pixels (printed as
Mpixel/s), bytes (MB/s, a megabyte being 10⁶ bytes), outlines (tier P states
P-26's CFF target in outlines a second), or none, for an operation whose input
is none of these — a private-key operation, a shaped paragraph — which
reports milliseconds alone. Criterion keeps a group's throughput from one
operation to the next and has no call that clears it, so a group declares its
unitless operations first: the AES-256 open opens the open group. A render's pixels are its bitmap's; a decode's
are the image's; the ICC conversion's are the image's, because tier P states
that target in the image's Mpixel/s.

**The iteration** is one of four shapes, and the shape is part of what the
operation measures.

- **From bytes.** An iteration opens the document from its bytes, handed as
  an `Arc<[u8]>` so no copy is timed (`Document::open`,
  `crates/tinker-pdf/src/lib.rs:1369`), and does the operation. A render opens
  its document every time, so that a cache a later row keeps on the document —
  P-16's — cannot turn a first render into a warm one unnoticed. An open is
  lazy, and small beside 20 ms.
- **Over prebuilt state**, built once before the loop when the operation's
  name says it starts from it: prebuilt text, a recorded display list, a
  rendered bitmap, a decoder's input. The second render and the re-rendered
  annotation build theirs per iteration, untimed, through criterion's
  `iter_batched`.
- **Repeated.** An operation whose head figure would sit under 20 ms, today or
  at its target, runs `calls` times an iteration, the same calls on both sides
  of a pair, so a paired ratio does not move; `calls` is chosen below and
  confirmed at MS-04.10. A relation compares per-call figures, an iteration's
  mean over its `calls`, so each side of one keeps its own.
- **A child.** A CLI operation runs `tpdf` once a call (below).

#### The inputs

All but the committed ones are written by `support/inputs.rs`, by formula
where the bytes are pinned (seeded integer arithmetic, no float, so every
target computes the same bytes) and through the tree's own writers elsewhere.

| Input | What it is | How it is written | Tier P's input |
| --- | --- | --- | --- |
| `dense(n, seed)` | n A4 pages (595 × 842 pt), each about 5 600 characters of seeded words in 70 lines, drawn from about 80 distinct glyphs with a few accented letters | Liberation Serif Regular (`crates/tinker-pdf-font/data/liberation/`) embedded whole — `add_embedded_font` (`crates/tinker-pdf-cos/src/build.rs:5694`) with `set_subset_fonts(false)` (`:6795`), so subset-on-save has a program to cut | "100 dense pages", "the text page" (`dense(1, 0)`), "20 pages"; 5 600 characters a page is P-02's ≥ 5.6 M characters/s at 1 000 pages/s |
| `producer(n)` | the dense pages' text in standard-14 Helvetica, unembedded, one `TJ` a line with kerning numbers | `add_base_font` (`build.rs:5561`) and `PageBuilder::raw` (`:4125`) | the Item's "100 producer-style standard-14 pages" |
| `small(n)` | n pages of one Helvetica line each, the line `three_pages()` writes | the builder's flat page tree, one `/Kids` of every page (`build.rs:8508-8516`) | "small pages", "10 000-page document", "rewrite 100 / 1 000 / 3 000 pages" |
| `three_pages()` | the document `open/open a 3-page document` opens | `support/inputs.rs` | — |
| `report(n)` | n A4 pages of two columns of body text in the embedded face, a running head and foot, a page number and one ruled table each; untagged | the builder | "100 text pages", "100 pages" of the analysis rows |
| `claiming(n)` | `report(n)` tagged (`PageBuilder::tagged`, `build.rs:2980`) under `DocumentBuilder::archival` (`:9047`), claiming PDF/A-2b, PDF/UA-1 and PDF/X-1a:2003 (`set_info`'s `GTS_PDFXVersion`, `:7744`; an output intent, `:3306`, carrying `fuzz/corpus/icc_profile/cmyk-lut.icc`), so each validator runs its rules rather than reporting no claim | the builder | "100 pages" of the validation row |
| `photo(w, h, seed)` | RGB samples by formula: smooth fields, hard edges and a few levels of seeded noise | `inputs.rs` | the photographic content of every image input |
| `page_like()` | 1240×1755 RGB (6 528 600 bytes) of text-like marks on white, about a tenth of the pixels inked, by formula — never a render, so it cannot move with the renderer | `inputs.rs`; its length and SHA-256 pinned beside the level-6 counts | "page-like data" |
| `photo_like()` | `photo(1240, 1755, 0)`, 6 528 600 bytes, pinned the same way | `inputs.rs` | "photo-like data" |
| `noise()` | 6 528 600 seeded bytes whose deflate stream is between 90 % and 100 % of their length, so inflate decodes coded literals, not stored blocks (asserted) | `inputs.rs` | "incompressible data" |
| `lzw(bytes)` | 7.4.4's LZW, 9- to 12-bit codes, early change 1, a clear code at 4 096 | an encoder in `inputs.rs`, about sixty lines, round-tripped through `lzw_decode` (`crates/tinker-pdf-filters/src/lib.rs:738`) whenever it runs | LZW-coded page-like data; the tree's only LZW writer packs literals and compresses nothing (`crates/tinker-pdf-filters/src/tiff/tests.rs:376`) |
| `baseline_jpeg()` | `photo(2480, 3508, 0)` as a baseline JPEG, Annex K's tables | `jpeg_encode` (`crates/tinker-pdf-filters/src/jpeg/encode.rs:819`) | "JPEG decode 2480×3508, baseline" |
| `scan()` | `dense(1, 0)`'s page rendered at 300 dpi and thresholded to one bit | the renderer | "G4 / JBIG2 scan page" |
| `image_file()` | as many A4 pages as reach 23 000 000 bytes, each covered by its own image, `photo(2480, 3508, page)` DCT-coded, so no two pages share an image a writer could deduplicate | `jpeg_encode`, `add_image` (`build.rs:7206`) | "a 23 MB file", "a 23 MB image file" |
| `aes_file()` | `image_file()` saved at R6 with both passwords empty and fixed entropy, the file `tpdf encrypt` writes with neither password | `WriteOptions::encryption` (`crates/tinker-pdf-cos/src/write.rs:30-55`, `:124`) | the Item's "an AES-256 file" |
| `jpeg_page()`, `flate_page()`, `cmyk_page()` | an A4 page covered by one 2480×3508 image: `baseline_jpeg()`; `photo(2480, 3508, 1)` through `zlib_compress` (`deflate.rs:218`); `photo` converted to CMYK by formula and Flate-coded under `/ICCBased` with `cmyk-lut.icc` (`add_icc_color_space`, `build.rs:6356`) | `add_image` | "full-page JPEG", "Flate RGB image page", "an ICC-based CMYK image"; the CMYK image is not DCT-coded, so its operation times the conversion and no JPEG decode |
| `jpx_page()` | an A4 page covered by `data/`'s irreversible codestream | `ImageFilter::Jpx` (`build.rs:438`) | "full-page JPEG 2000" |
| `g4_page()`, `jbig2_page()` | an A4 page covered by `scan()`: G4-coded (`ccitt_g4_encode`, `ccitt/encode.rs:386`, as `ImageFilter::CcittFax`); and the same page saved through the image pass's `BilevelCodec::Jbig2Generic` (`crates/tinker-pdf/src/recode.rs:188-197`), the builder having no JBIG2 filter | the builder, then `write::save` | "G4 / JBIG2 scan page" |
| `paths(5000)`, `curves(2000)` | 3 000 filled polygons and closed curves at fractional coordinates, as `triangles()` draws them, and 2 000 stroked cubic curves, 0.5 to 4 pt wide, each of the nine cap-and-join pairs in turn, every fifth dashed; `curves` is those 2 000 alone | `raw` | "5 000 paths", "that page's 2 000 stroked curves alone" |
| `triangles()`, `axial()` | the render group's 300 triangles and full-page axial shading | `support/inputs.rs` | "300 triangles", "full-page axial shading" |
| `tiling(fills)` | one tiling pattern (`add_tiling_pattern`, `build.rs:6276`) filling an A4 page in one rectangle, or in 50 strips that together cover what the one covers | the builder | "a page filled by one tiling pattern 50 times / once" |
| `annotated(base)` | `base` with one 100×100 pt square annotation on its first page, appearance included (`DocumentEditor::add_annotation`, `crates/tinker-pdf-cos/src/edit/annotations.rs:214`; `annot::square`, `edit/annot.rs:84`) | the editor | "a changed 100×100 pt annotation", "a 1 000-page file", "a 10-page one" |
| `book(chapters)` | an EPUB of a heading and one paragraph of about 600 characters a chapter, each chapter a page at the default box, Liberation Serif embedded through `@font-face` in the one stylesheet every chapter links so its glyphs are drawn in both builds | `tests/epub_support`'s OCF writer (`crates/tinker-pdf/tests/epub_support/mod.rs:543`), by path | "a generated 1 000-chapter book", "100 chapters" |
| `html_report()`, `markdown()` | 20 pages each at the default page box. The report is 20 sections, each a heading, two paragraphs and a ruled table of six rows on a page of its own; the Markdown is 20 sections, each a heading, three paragraphs and a list of five items, in one flow, CommonMark having no table | `FromHtml` (`crates/tinker-pdf/src/html.rs:181`); CommonMark for `Document::open_markdown` (`crates/tinker-pdf/src/lib.rs:1472`) | "a generated 20-page HTML report / 20-page Markdown document" |
| `message()` | 10 000 000 seeded bytes, and their RSA-2048 PKCS #1 v1.5 signature made in the bench with `pss-vect.txt`'s Example 10 key (`crates/tinker-pdf-crypto/tests/data/pkcs1/pss-vect.txt`) through `Modulus::pow` (`crates/tinker-pdf-crypto/src/bignum.rs:468`) | `inputs.rs` | "over 10 MB" |
| committed | the nine books (`crates/tinker-pdf/tests/epub/`); the mesh and transparency fixtures (`fuzz/corpus/render_page/`); every `.svg` in `crates/tinker-pdf-svg/tests/fixtures/` (17), every `.xps` and `.oxps` under `crates/tinker-pdf/tests/` (19) and every `.cbz` in `crates/tinker-pdf/tests/cbz/` (9); the three Liberation Regular faces; the four bare CFF programs in `fuzz/corpus/cff/` and the `CFF ` tables of the 189 committed OpenType files (185 in `crates/tinker-pdf-shape/data/aots/fonts/`, three text-rendering-tests faces and `fuzz/corpus/sfnt/opentype-cff.otf`); `TestShapeAran.ttf`; `data/`'s three codestreams; the vendored Devanagari face | read by path | as named |

#### The operations

Each group's table gives the operation's id, its unit and what one
iteration covers, its input, tier P's figure and gate, where it stands
against the window, and the step that adds it. Tier P's figures are the
planning machine's at `61adcde` (`docs/roadmap/P.md:22-28`), in milliseconds
unless marked; "twin" is an in-process operation the table marks only for the
CLI, which sits at floor 1 (MS-02.3's note).

**Window**, from tier P's figures: *in* — inside 20–300 ms today and at its
target; *×n* — `calls` n, which keeps it inside both; *above* — over 300 ms
today, inside at its target; *above, and at target* — over 300 ms even when
its row lands; *crosses* — over 300 ms today and under 20 ms at its target,
which no single `calls` can hold; *unmeasured* — tier P has no figure for it
today, so its `calls` comes from its target where it has one and from
MS-04.10's measured figure where it has none, and its iteration is written "a
call". `calls` n is the least that puts n × the target at 25 ms or more, kept
only where n × today's figure stays at or under 300 ms.

**open**

| Id | Unit · an iteration | Input | Tier P: today → target; gate | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `open/open a 3-page document` | bytes · the file a call | `three_pages()` | —; floor 1 | unmeasured | built |
| `open/open and count the 23 MB image file` | bytes · the file a call | `image_file()` | twin; floor 1 | unmeasured | built |
| `open/open the AES-256 file` | none | `aes_file()`, authenticated with the empty password (`lib.rs:1722`) and counted | the Item's; floor 1 | unmeasured | built |
| `open/every page's size, 3000 pages` | pages · 3 000 | `small(3000)`, `page(i)` then `Page::size` (`lib.rs:1773`, `:2198`) for every i | 2 569 → 3; 856× | crosses | built |
| `open/every page's size, 10000 pages` | pages · 10 000 | `small(10000)` | Θ(n²), about 28 500 from the 3 000-page figure → 10; relation ≤ 3.6 × 3 000 | crosses | built |
| `open/open and count the 23 MB image file, CLI` | bytes · 5 files | `tpdf info` | 36 → 6; 6× | ×5 | built |

**text**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `text/text of 100 dense pages` | pages · 100 | `dense(100, 0)`, every page's `text().plain_text()` by index, as `tpdf text` reads them (`tools/tpdf/src/main.rs:973-991`) | 751 → 100; 7.51× | above | built |
| `text/text of 100 standard-14 pages` | pages · 100 | `producer(100)` | the Item's; floor 1 | above, expected | built |
| `text/text of 1000 small pages` | pages · 1 000 | `small(1000)` | twin (CLI 350); floor 1 | crosses | built |
| `text/text of 10000 small pages` | pages · 10 000 | `small(10000)` | twin (CLI 36 000); floor 1 | above | built |
| `text/structured JSON of 100 pages` | pages · 100 | `dense(100, 0)` through `TextWriter`'s JSON, as `tpdf text --json` (`main.rs:962-972`) | twin (CLI 1 926); floor 1 | above | built |
| `text/search a phrase over 100 pages from scratch` | pages · 100 | `dense(100, 0)`, `TextPage::search` (`crates/tinker-pdf-content/src/text.rs:265`) | 722 → 110; 6.56× | above | built |
| `text/search 100 pages' prebuilt text` | pages · 500 | the 100 `TextPage`s, built once | 36 → 6; 6× | ×5 | built |
| `text/search 100 pages' prebuilt text, whole words, unaccented` | pages · 300 | the same, `search_with` whole-word and diacritic-insensitive (`search.rs:43-50`, `:108`) | 77 → 12; 6.41× | ×3 | built |
| `text/text of 100 dense pages, CLI` | pages · 100 | `tpdf text` | 678 → 130; 5.21× | above | built |
| `text/text of 1000 small pages, CLI` | pages · 1 000 | `tpdf text` | 350 → 15; 23.3× | crosses | built |
| `text/text of 10000 small pages, CLI` | pages · 10 000 | `tpdf text` | 36 000 → 150; relation ≤ 11 × 1 000 | above | built |
| `text/structured JSON of 100 pages, CLI` | pages · 100 | `tpdf text --json` | 1 926 → 200; 9.63× | above | built |

**analysis** — every relation's "text of the same pages" is the group's own
text of the document the operation reads, so all six stay inside the group.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `analysis/text of the 100 report pages` | pages · 100 | `report(100)` | the relations' denominator; floor 1 | above, expected | built |
| `analysis/redact a rectangle on each of 100 pages` | pages · 100 | `report(100)`, `redact::apply` on every page of one editor (`crates/tinker-pdf/src/redact.rs:1106`), unsaved: saving is the write group's | not measured; ≤ 1.5 × text | above, expected | built |
| `analysis/reading order inferred over 100 pages` | pages · 100 | `report(100)`, `Document::inferred_orders` (`crates/tinker-pdf/src/reading_order.rs:484`); the document is untagged, so the default options infer (`:173-176`) | not measured; ≤ 1.5 × text | above, expected | built |
| `analysis/tables inferred over 100 pages` | pages · 100 | `report(100)`, `Page::tables(TableSource::Inferred)` (`crates/tinker-pdf/src/tables.rs:448`) | not measured; ≤ 1.5 × text | above, expected | built |
| `analysis/text of the 100 claiming pages` | pages · 100 | `claiming(100)` | the validators' denominator; floor 1 | above, expected | built |
| `analysis/PDF-A validation of 100 pages` | pages · 100 | `claiming(100)`, `validate_pdfa` (`lib.rs:1990`) | not measured; ≤ 3 × text | above, expected | built |
| `analysis/PDF-UA validation of 100 pages` | pages · 100 | `validate_pdfua` (`:2020`) | not measured; ≤ 3 × text | above, expected | built |
| `analysis/PDF-X validation of 100 pages` | pages · 100 | `validate_pdfx` (`:2043`) | not measured; ≤ 3 × text | above, expected | built |

**render** — a page render is `Page::render` (`lib.rs:2247`) at
`RenderOptions::at_dpi` (`:724`); "the text page" is `dense(1, 0)`'s page.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `render/the text page at 72 dpi` | pixels · 5 × 595×842 | `dense(1, 0)` | 54 → 6; 9× | ×5 | built |
| `render/the text page at 150 dpi` | pixels · 2 × 1240×1755 | `dense(1, 0)` | 88 → 15; 5.86× | ×2 | built |
| `render/the text page at 300 dpi` | pixels · 2480×3509 | `dense(1, 0)` | 171 → 35; 4.88× | in | built |
| `render/record the text page's display list` | pages · 21 | `Page::display_list` (`crates/tinker-pdf/src/display.rs:129`) | 6.3 → 1.2; 5.25× | ×21 | built |
| `render/5000 paths at 150 dpi` | pixels · 1240×1755 | `paths(5000)` | 1 326 → 450; 2.94× | above, and at target | built |
| `render/5000 paths at 300 dpi` | pixels · 2480×3509 | `paths(5000)` | 3 434 → 1 100; 3.12× | above, and at target | built |
| `render/the 2000 stroked curves alone at 150 dpi` | pixels · 1240×1755 | `curves(2000)` | not measured apart; as the page; 2.94×, paired with the baseline's same curves | above, expected | built |
| `render/300 triangles at 150 dpi` | pixels · 2 × 1240×1755 | `triangles()` | 44 → 20; 2.2× | ×2 | MS-04.10 renames the seven's |
| `render/a full-page axial shading at 150 dpi` | pixels · 1240×1755 | `axial()` | 124–170 → 25; 4.96× | in | MS-04.10 renames the seven's |
| `render/a full-page axial shading at 300 dpi` | pixels · 2480×3509 | `axial()` | 674 → 90; 7.48× | above | built |
| `render/the mesh shadings fixture at 150 dpi` | pixels · 7 bitmaps | `fuzz/corpus/render_page/mesh-shadings.pdf` | 18.1 → 4; 4.52× | ×7 | built |
| `render/the mesh shadings fixture at 300 dpi` | pixels · 3 bitmaps | the same | 43.1 → 10; 4.31× | ×3 | built |
| `render/the transparency fixture at 150 dpi` | pixels · 9 bitmaps | `fuzz/corpus/render_page/transparency.pdf` | 8.4 → 3; 2.8× | ×9 | built |
| `render/a page tiled by one pattern fill at 150 dpi` | pixels · 1240×1755 a call | `tiling(1)` | not measured; relation: 50 fills ≤ 1.2 × one; counted | unmeasured | built |
| `render/a page tiled by 50 pattern fills at 150 dpi` | pixels · 1240×1755 a call | `tiling(50)` | the same relation | unmeasured | built |
| `render/the 300-dpi text page in 4 bands on 4 threads` | pixels · 3 × 2480×3509 | `dense(1, 0)`, four `RenderOptions::region` bands (`lib.rs:584`), one scoped thread each | relation ≥ 3 × the 1-thread speed (`the text page at 300 dpi`) | ×3 | built |
| `render/100 text pages at 150 dpi on 1 thread` | pages · 100 | `dense(100, 0)` | 11.4 → 60 pages/s (8 772 → 1 667); 5.26× | above, and at target | built |
| `render/100 text pages at 150 dpi on 4 threads` | pages · 100 | the same, on `std::thread::scope` over owned pages, as `crates/tinker-pdf/examples/parallel.rs:87` does | 43 → 200 pages/s (2 326 → 500); 4.65×; relation ≥ 3.33 × 1 thread | above, and at target | built |
| `render/the annotated text page at 150 dpi` | pixels · 2 × 1240×1755 | `annotated(dense(1, 0))` | the next one's full-page render; floor 1 | ×2 | built |
| `render/re-render a changed 100x100 pt annotation` | pixels · 3 × 1240×1755 | the same, its appearance replaced, saved and reopened untimed; today the whole page re-renders | relation ≤ 10 % of the full-page render | ×3 today; under 20 at its bound | built |
| `render/replay the Flate RGB image page at 150 dpi` | pixels · 1240×1755 | `flate_page()`'s display list, recorded once; its renders share the decoded image (`resources.rs:532-546`) | 350 → 40; 8.75× | above | built |
| `render/an ICC-based CMYK image to sRGB at 300 dpi` | pixels · 2480×3508, the image's | `cmyk_page()`, drawn at the image's own resolution so each sample is converted once | not measured → ≥ 100 Mpixel/s (≤ 87); calibrated | unmeasured | built |
| `render/the JPEG page at 72 dpi` | pixels · 595×842 | `jpeg_page()` | 652 → 35; 18.6× | above | built |
| `render/the JPEG page at 300 dpi` | pixels · 2480×3509 | `jpeg_page()` | 1 585 → 150; 10.5× | above | built |
| `render/a second render of the JPEG page at 300 dpi` | pixels · 2480×3509 | `jpeg_page()`, after the same `Page` rendered at 72 dpi, untimed | not measured; relation ≤ 30 % of the first (`the JPEG page at 300 dpi`) | above, expected | built |
| `render/the JPEG 2000 page at 72 dpi` | pixels · 595×842 | `jpx_page()` | 2 753 → 250; 11× | above | built |
| `render/the G4 scan page at 150 dpi` | pixels · 1240×1755 | `g4_page()` | 641 → 50; 12.8× | above | built |
| `render/the JBIG2 scan page at 150 dpi` | pixels · 1240×1755 | `jbig2_page()` | 875 → 80; 10.9× | above | built |
| `render/the Flate RGB image page at 72 dpi` | pixels · 595×842 | `flate_page()` | 924 → 230; 4.01× | above | built |
| `render/the text page to PNG at 150 dpi, CLI` | pixels · 1240×1755 | `tpdf render` | 402 → 70; 5.74× | above | built |
| `render/the text page to PNG at 300 dpi, CLI` | pixels · 2480×3509 | `tpdf render` | 1 180 → 200; 5.9× | above | built |
| `render/20 pages to PNG at 150 dpi, 1 job, CLI` | pages · 20 | `tpdf render --jobs 1` | 6 847 → 1 300; 5.26× | above, and at target | built |
| `render/20 pages to PNG at 150 dpi, 4 jobs, CLI` | pages · 20 | `tpdf render --jobs 4` | 1 743 → 400; 4.35× | above, and at target | built |

**encode**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `encode/PNG of the 1240x1755 text page` | pixels · 1240×1755 | the text page's 150-dpi bitmap, rendered once, `Bitmap::to_png` (`lib.rs:832`) | 269 → 40; 6.72× | in | built |
| `encode/PNG of the near-blank page` | pixels · 2 × 1240×1755 | a 150-dpi bitmap of a page holding one line | 145 → 15; 9.66× | ×2, at 290 today | built |
| `encode/deflate page-like data` | bytes · 6 528 600 | `page_like()`, `deflate` (`deflate.rs:200`) | 40 → 120 MB/s (163 → 54); 3×; size ≤ 1.02 × level 6's, exact | in | built |
| `encode/deflate photo-like data` | bytes · 6 528 600 | `photo_like()` | 28 → 40 MB/s (233 → 163); 1.4×; size likewise | in | built |

**decode** — each through its `tinker-pdf-filters` entry point
(`crates/tinker-pdf-filters/src/lib.rs:70-93`, `:721`, `:738`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `decode/inflate page-like data` | bytes out · 2 × 6 528 600 | `deflate(page_like())` | 279 → 500 MB/s (23.4 → 13.1 a call); 1.79× | ×2 | built |
| `decode/inflate near-incompressible data` | bytes out · 6 528 600 | `deflate(noise())` | 141 → 150 MB/s (46.3 → 43.5); 1.06× | in | built |
| `decode/LZW decode page-like data` | bytes out · 6 528 600 | `lzw(page_like())` | not measured → ≥ 200 MB/s (≤ 32.6); calibrated | unmeasured | built |
| `decode/baseline JPEG 2480x3508` | pixels · 2480×3508 | `baseline_jpeg()` | 446 → 60; 7.43× | above | built |
| `decode/progressive JPEG 2480x3508` | pixels · 2480×3508 | `data/progressive-2480x3508.jpg` | 458 → 120; 3.81× | above | built |
| `decode/irreversible JPEG 2000 2480x3508` | pixels · 2480×3508 | `data/irreversible-2480x3508.jp2` | 2 566 → 1 000; 2.56× | above, and at target | built |
| `decode/reversible JPEG 2000 2480x3508` | pixels · 2480×3508 | `data/reversible-2480x3508.jp2` | 6 516 → 2 700; 2.41× | above, and at target | built |
| `decode/decode the JBIG2 generic page` | pixels · `scan()`'s | `jbig2_generic_encode(scan())` (`jbig2/encode.rs:251`) | 255 → 60; 4.25× | in | built |
| `decode/decode the CCITT G4 page` | pixels · `scan()`'s | `ccitt_g4_encode(scan())` | 46 → 25; 1.84× | in | built |

**write** — a rewrite is the editor's save under `WriteMode::Rewrite`, as
`write/rewrite a document`'s is (`benches/write.rs`); merge and subset-on-save go through
`write::save` (`crates/tinker-pdf/src/write.rs:337`) as `tpdf merge` does
(`tools/tpdf/src/writing.rs:132-165`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `write/rewrite 100 pages` | pages · 100 a call | `small(100)` | twin; floor 1 | unmeasured | built |
| `write/rewrite 1000 pages` | pages · 1 000 | `small(1000)` | twin (CLI 668); floor 1 | above | built |
| `write/rewrite 3000 pages` | pages · 3 000 | `small(3000)` | 5 480 → 40; 137× | above | built |
| `write/rewrite 100 pages into object streams` | pages · 10 × 100 | `small(100)`, `object_streams` (`crates/tinker-pdf-cos/src/write.rs:120`) | 2.6; held | ×10 | built |
| `write/rewrite 100 pages linearized` | pages · 7 × 100 | `small(100)`, `linearize` (`:112`) | 3.7; held | ×7 | built |
| `write/subset-on-save of 100 pages` | pages · 100 | `dense(100, 0)`, `SaveOptions::default()`: a rewrite, fonts subset (`crates/tinker-pdf/src/write.rs:144-157`) | twin (in-process 1 305); floor 1 | above | built |
| `write/merge 4 x 100 pages` | pages · 400 | `dense(100, 0..3)`: pages imported with `import_page`, saved with stream deduplication and `FontPolicy::Keep` | 2 855 → 25; 114× | above | built |
| `write/AES-256-CBC decrypt 23 MB` | bytes · the file | `image_file()`'s bytes, encrypted once, `cbc_decrypt_with_iv_prefix` (`crates/tinker-pdf-crypto/src/aes.rs:238`) | 32 → 150 MB/s (about 719 → 153); 4.68× | above | built |
| `write/AES-256-CBC encrypt 23 MB` | bytes · the file | the same plain bytes, `cbc_encrypt_with_iv_prefix` (`:289`) | 57 → 80 MB/s (about 404 → 288); 1.4× | above | built |
| `write/edit an annotation, save, reopen, 1000 pages` | pages · 13 × 1 000 | `annotated(small(1000))`: the appearance replaced (`DocumentEditor::put_stream`, `edit/mod.rs:395`), saved under `WriteMode::Incremental`, the bytes reopened | not measured → ≤ 2 plus output bytes; relation ≤ 2 × the 10-page one | unmeasured, ×13 | built |
| `write/edit an annotation, save, reopen, 10 pages` | pages · 10 a call | `annotated(small(10))` | the relation's other side | unmeasured | built |
| `write/rewrite 100 pages, CLI` | pages · 4 × 100 | `tpdf merge`, one input | 14.8 → 8; 1.85× | ×4 | built |
| `write/rewrite 1000 pages, CLI` | pages · 1 000 | `tpdf merge`, one input | 668 → 60; 11.1× | above | built |
| `write/subset-on-save of 100 pages, CLI` | pages · 100 | `tpdf merge --font-policy subset` | 1 208 → 120; 10× | above | built |
| `write/merge 4 x 100 pages, CLI` | pages · 400 | `tpdf merge`, four inputs | 1 008 → 40; 25.2× | above | built |
| `write/encrypt the 23 MB image file, CLI` | bytes · the file | `tpdf encrypt` | 770 → 150; 5.13× | above | built |

The merge keeps fonts because its target leaves no room for a subset pass —
40 ms for 400 pages where subset-on-save's is 120 ms for 100 — and
subset-on-save measures that pass on its own. The AES pair is tier P's "decrypt
/ encrypt", whose table states no size; 23 MB is the Item's, so a smaller input
there would re-state no tier P figure, only the row's text, which is the
owner's under H-163 all the same.

**sign**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `sign/RSA-2048 private-key operation` | none · 13 calls | Example 10's modulus and private exponent, `Modulus::pow` | 6.0 → 2; 3×, paired with the baseline's `pow` | ×13 | built |
| `sign/verify an RSA-2048 signature over 10 MB` | bytes · 10 000 000 | `message()`, digest and public operation (`RsaPublicKey`, `crates/tinker-pdf-crypto/src/rsa.rs:256`) | about 38 → 30; 1.26× | in | built |
| `sign/SHA-256 of 10 MB` | bytes · 10 000 000 | `message()`, `sha256` (`crates/tinker-pdf-crypto/src/sha2.rs:473`) | 262 → 350 MB/s (38.2 → 28.6); 1.33× | in | built |

**layout** — a book opens at the default box, 432 × 648 pt (`OpenOptions`,
`crates/tinker-pdf/src/lib.rs:1128-1154`), which is the seven's box too; a
container's synthesised PDF is `doc.cos().bytes()`
(`crates/tinker-pdf-cos/src/doc.rs:1029`), opened again from those bytes.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `layout/open and paginate <book> at 432x648 pt`, one for each of the nine | pages · 4 × the book's | each committed book | 17–22 → 8; 2.12× | ×4 | built |
| `layout/render every page of <book> at 72 dpi`, nine | pages · the book's | the same | twin (CLI 84–130); floor 1 | in | built |
| `layout/open and paginate the 1000-chapter book` | pages · its count | `book(1000)` | not measured → ≥ 1 000 pages/s; calibrated; relation ≤ 11 × 100 chapters | above, and at target | built |
| `layout/render the 1000-chapter book at 72 dpi` | pages · its count | `book(1000)` | not measured → ≥ 300 pages/s; calibrated; the same relation | above, and at target | built |
| `layout/open and paginate the 100-chapter book` | pages · its count a call | `book(100)` | the relation's other side | unmeasured | built |
| `layout/render the 100-chapter book at 72 dpi` | pages · its count | `book(100)` | the relation's other side; 333 at 300 pages/s | above, and at target | built |
| `layout/the 20-page HTML report to PDF` | pages · 20 | `html_report()`, laid out and finished | not measured → ≤ 60; calibrated | unmeasured | built |
| `layout/the 20-page Markdown document to PDF` | pages · 20 | `markdown()` | not measured → ≤ 50; calibrated | unmeasured | built |
| `layout/open each SVG fixture, render page 1` | pages · 17 a call | every committed `.svg`, its first page at 72 dpi | not measured; relation ≤ 1.5 × its synthesised PDF | unmeasured | built |
| `layout/render each SVG fixture's synthesised PDF` | pages · 17 a call | each one's synthesised PDF, opened from bytes | the relation's other side | unmeasured | built |
| `layout/open each XPS fixture, render page 1` | pages · 19 a call | every committed `.xps` and `.oxps` | the same relation | unmeasured | built |
| `layout/render each XPS fixture's synthesised PDF` | pages · 19 a call | the same | its other side | unmeasured | built |
| `layout/open each CBZ fixture, render page 1` | pages · 9 a call | every committed `.cbz` | the same relation | unmeasured | built |
| `layout/render each CBZ fixture's synthesised PDF` | pages · 9 a call | the same | its other side | unmeasured | built |
| `layout/render every page of <book> at 72 dpi, CLI`, nine | pages · the book's | `tpdf render` | 84–130 → 40; 2.1× | in | built |

The nine books are `calibre-book-cover`, `calibre-book-nocover`,
`calibre-embedded-font`, `kcc-fixed-layout`, `pandoc-book-cover`,
`pandoc-book-epub2`, `pandoc-book-nocover`, `pandoc-embedded-font` and
`pandoc-plates`, each `.epub`, and the id carries the file name: the longest,
`render every page of calibre-embedded-font.epub at 72 dpi, CLI`, is 62
bytes. The containers are timed a format at a time, every fixture of the
format in one iteration, so the relation holds for each format's set: one
operation a fixture would be 90 operations and about seven minutes of the
layout job. A fixture the facade refused would be named in its generator's
list of those left out and never timed as a failure. The facade opens all 45,
each renders its first page from the container and from its synthesised PDF,
and the group's setup asserts the three counts.

**fonts** — `Sfnt::parse` and `glyf::outline`
(`crates/tinker-pdf-font/src/sfnt.rs:46`, `glyf.rs:67`), `Cff::parse` and
`Cff::outline` (`cff.rs:218`, `:414`), and a fresh `Shaper` a call as every
caller makes one (`crates/tinker-pdf-shape/src/shape.rs:270`, `:426`;
`crates/tinker-pdf/src/shaping.rs:198`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `fonts/outlines of Liberation Serif Regular` | outlines · 12 × 2 602 | the face, parsed and every glyph outlined | 2.1; held | ×12 | built |
| `fonts/outlines of Liberation Sans Regular` | outlines · 9 × 2 620 | the same | 3.0; held | ×9 | built |
| `fonts/outlines of Liberation Mono Regular` | outlines · 14 × 2 423 | the same | 1.8; held | ×14 | built |
| `fonts/outlines of every committed CFF program` | outlines · 2 × every program's | the 193 programs | not measured → ≥ 1 M outlines a second; calibrated | unmeasured, ×2 | built |
| `fonts/shape 1000 characters of Latin` | none · 125 calls | Liberation Serif Regular | not measured → ≤ 0.2; calibrated | unmeasured, ×125 | built |
| `fonts/shape 1000 characters of Arabic` | none · 50 calls | `TestShapeAran.ttf` | not measured → ≤ 0.5; calibrated | unmeasured, ×50 | built |
| `fonts/shape 1000 characters of Devanagari` | none · 42 calls | the vendored face | not measured → ≤ 0.6; calibrated | unmeasured, ×42 | built |

The 192 programs that parse as plain CFF hold 18 529 glyphs, and every one
outlines; the 193rd, `fuzz/corpus/sfnt/opentype-cff.otf`, is a fuzz seed
whose table the parser refuses, so it costs its refusal and adds no outline,
and the group's setup asserts all three counts. At a million outlines a
second the 18 529 take under 19 ms, so `calls` 2.

**scripts** and **OCR** have no operation of MS-04's own; their rows create
their files (below). **calibration** holds one:

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `calibration/text of 100 dense pages` | pages · 100 | `dense(100, 0)`, the call `text/text of 100 dense pages` makes | 751 on the planning machine (H-112); the operation every calibrated target is a fraction of | above until P-02 | built |

Its file is frozen: it is the same input and call as the text group's, kept
apart so that nothing a later row adds to the text group can move it, and a
change to it re-states every calibrated target, which is the owner's (H-77).

**For MS-04.9's report**, then, tier P's figures put 30 operations above
300 ms today and inside at their targets, and three more are expected there:
`text of 100 standard-14 pages`,
`the 2000 stroked curves alone at 150 dpi` and
`a second render of the JPEG page at 300 dpi`.
Eleven stay above even at their targets. Four cross the whole window: both
`every page's size` operations and both
`text of 1000 small pages`.
The eight analysis operations are expected above by their relations' bounds
over a 751 ms text. That is 56 of the 138 expected outside at tier P's
inputs. Of the rest, 58 sit inside today and at their targets by tier P's
figures, 30 of them by their `calls`; 23 have no figure today, nine sized by
their targets and 14 waiting for MS-04.10's figures; and
`re-render a changed 100x100 pt annotation`
is inside today and falls under 20 ms at its bound. The planning machine is
not the runner, so an operation near 300 ms may fall inside; MS-04.9's dispatch is the list H-163 is decided on.

#### Fixtures the tree cannot write

The tree writes baseline JPEG, G4, JBIG2 generic regions, PNG and deflate. It
writes no progressive JPEG
(`crates/tinker-pdf-filters/src/jpeg/encode.rs:12-16`), no JPEG 2000 image
(`crates/tinker-pdf-filters/src/jpx/tests/writer.rs` writes test headers) and
no LZW that compresses, and it commits no face covering Devanagari. The two
image kinds come as ruling 13 admits a third party's bytes: a picture this
repository authored, turned into bytes once by an outside program, committed
with its provenance and read like any fixture — no test runs the program, and
every check on the result is first-party. It is the device
`crates/tinker-pdf-filters/tests/images/README.md` already uses.

- **The progressive JPEG** is `baseline_jpeg()`'s coefficients re-scanned
  progressively by an outside program's lossless transcode, so a test holds
  its decode to `baseline_jpeg()`'s decode sample for sample: the two differ
  in their scan schedule and nothing else, which is what the
  baseline/progressive pair is for. `baseline_jpeg()` asserts the SHA-256 the
  README records as the transcode's source, so a change to the encoder fails
  there first, a finding rather than a quiet mismatch.
- **The two JPEG 2000 codestreams** are `photo(2480, 3508, 0)`, written once
  as a PPM, encoded by an outside JPEG 2000 encoder as JP2 files: one tile,
  five decomposition levels (P-10 discards two), 64 × 64 code-blocks, one
  layer, LRCP; the irreversible one at one bit a pixel, the reversible one
  lossless — so a test holds the reversible decode to `photo(2480, 3508, 0)`
  exactly, the generator's input being the expected output, and the
  formula's digest is pinned beside the README's record of it.
- **The Devanagari face** is vendored data rather than a fixture: a face
  under OFL-1.1, which the allowlist admits (H-116's default), in
  `crates/tinker-pdf-shape/data/devanagari/` with its `LICENSE`, declared in
  `THIRDPARTY.md`'s table (`:24-38`) with its upstream, release and date.
  There `cargo xtask vendor` finds it (`xtask/src/main.rs:1222-1242`),
  `.gitattributes` already marks it (`:97`), and it sits beside the shape
  crate's other script faces; Noto Sans Devanagari is the candidate, as the
  Kannada and Balinese faces there are Noto's.
- **LZW** is the exception: an encoder is sixty lines, so `inputs.rs` owns
  one and round-trips it, and nothing is committed.

`crates/tinker-pdf/benches/data/README.md` records, for each committed file,
its bytes and SHA-256, how it was coded and the SHA-256 of the input it was
made from, and never the program that made it; the file names say what each
is (`progressive-2480x3508.jpg`, `irreversible-2480x3508.jp2`,
`reversible-2480x3508.jp2`), and `.jpg` and `.jp2` are already binary
(`.gitattributes:13-14`). The lossless codestream is the largest, at eleven
megabytes far larger than any other committed JPEG 2000 file, and the README
states each one's size.

#### The CLI operations

Tier P marks thirteen operations CLI, and their figures include the process;
`tpdf` has no library half (`tools/tpdf/src` holds `main.rs`, `writing.rs` and
`images.rs`), so each is timed by running it. `support/cli.rs` resolves `tpdf`
beside the bench binary's profile directory — a bench binary lives in
`target/<profile>/deps/`, so `tpdf` is `target/<profile>/tpdf` — the layout
`xtask/src/corpus.rs:717-730` relies on for its own child, and refuses with
the command that builds it when it is not there. Each call is one child, its
stdout and stderr written to files in the operation's scratch directory, its
exit status checked every time: a failed child panics the bench, which the
group's guard then counts short, and is never timed. Test mode reads the same
files back, so the run that is timed and the run that is checked are the same
run.

| Operation | Command | Calls |
| --- | --- | --- |
| open and count the 23 MB image file | `tpdf info image.pdf` (`tools/tpdf/src/main.rs:903-957`; P-05's Today is this command) | 5 |
| text of 100 dense pages / 1000 / 10000 small pages | `tpdf text dense-100.pdf`, `small-1000.pdf`, `small-10000.pdf` | 1 |
| structured JSON of 100 pages | `tpdf text dense-100.pdf --json` | 1 |
| the text page to PNG at 150 / 300 dpi | `tpdf render dense-1.pdf --out DIR --dpi 150` / `--dpi 300` | 1 |
| 20 pages to PNG at 150 dpi, 1 / 4 jobs | `tpdf render dense-20.pdf --out DIR --dpi 150 --jobs 1` / `--jobs 4` | 1 |
| rewrite 100 / 1000 pages | `tpdf merge small-100.pdf --out OUT.pdf --font-policy keep` / `small-1000.pdf` | 4 / 1 |
| subset-on-save of 100 pages | `tpdf merge dense-100.pdf --out OUT.pdf --font-policy subset` | 1 |
| merge 4 x 100 pages | `tpdf merge dense-100-0.pdf dense-100-1.pdf dense-100-2.pdf dense-100-3.pdf --out OUT.pdf --font-policy keep` | 1 |
| encrypt the 23 MB image file | `tpdf encrypt image.pdf --out OUT.pdf --entropy ENTROPY --font-policy keep` | 1 |
| render every page of a book at 72 dpi | `tpdf render BOOK.epub --out DIR --dpi 72` | 1 |

`tpdf` has no rewrite command. `merge` given one input writes that input again
under `rewrite()` with stream deduplication and nothing else
(`tools/tpdf/src/writing.rs:132-165`, `:547-552`), where `rotate --by 0` reads
and writes every page through `rotate_page` first, a walk of the page tree
each (`writing.rs:261-269`,
`crates/tinker-pdf-cos/src/edit/page_ops.rs:71-83`), and `sanitise` takes
something out; `61adcde`'s `merge` is the same code, so the planning baseline
can run the same command line. Every writing command takes `--font-policy` and
subsets by default (`tools/tpdf/src/main.rs:48`, `:102-104`), so each names
its policy: `keep` everywhere but subset-on-save, so that each figure times
one pass. `--entropy` names a file of 48 fixed bytes, because a figure must
not depend on `/dev/urandom`. The child is this repository's binary, built
from the same checkout in the same job, so the helper's `SPAWNERS` row says it
hosts this engine and adjudicates nothing.

#### `baseline.json`'s lists

Ahead of `"machines"`, one list a group, in the order its file declares
them:

```json
"groups": [
  {
    "group": "render",
    "operations": [
      { "name": "the text page at 72 dpi", "unit": "pixels",
        "per_iteration": 2504950, "calls": 5 },
      { "name": "the text page at 300 dpi", "unit": "pixels",
        "per_iteration": 8702320, "calls": 1 }
    ]
  }
],
```

`per_iteration` is an integer in the unit's base — pages, pixels, bytes,
outlines — covering every call, so throughput is `per_iteration` over the
mean and a call is the mean over `calls`; the bench file declares the same
figure through criterion's `Throughput`, criterion records it in each
operation's `benchmark.json`, and `bench-check` refuses a run whose recorded
figure is not the committed one, so a generator and its list cannot drift
apart unseen on the runner. An operation named outside the window by the
owner's word carries `"window": "outside: H-163, <date>"`, which MS-04.9's
check reads. A machine entry carries `"unmeasured"`, the ids it has no figure
for, ahead of its `"operations"`, so a hole is named and never silent. The
reader cuts the file at `"machines"` first, so no list entry can fall into a
machine's chunk. MS-02.3 adds each operation's
gate beside its unit, in the same list.

`cargo xtask baseline` then holds four things to each other: each
`[[bench]]`, its group file's names, its list, and `bench.yml`'s count for the
group; and it refuses a name criterion would rewrite, a unit outside the
five, a `per_iteration` or `calls` under one, and a machine entry naming an
operation as both measured and unmeasured.

#### Sampling, and the 30 minutes

| Groups | Samples | Warm-up | Measurement | Mode |
| --- | ---: | ---: | ---: | --- |
| open, text, analysis, render, encode, decode, write, layout, OCR, calibration | 10 | 1 s | 3 s | flat |
| sign, fonts, scripts | 30 | 1 s | 3 s | flat |

The window puts every operation at 20 ms or more, where criterion's linear
sampling buys nothing and costs up to 55 iterations at ten samples; flat
sampling takes each sample as ⌈3 s ÷ (samples × t)⌉ iterations, so an
operation of t costs its warm-up — at least one iteration — and then about the
larger of 3 s and samples × t: about 4.7 s inside the window, and 11 × t above
it, ten samples being the least criterion 0.5.1 takes (its `sample_size`
asserts it). Thirty samples cost the sign, fonts and scripts groups nothing:
their operations are sized near 25 ms by their targets, and at 100 ms or less
thirty samples fit the 3 s that ten would take. Each file sets its sample size
and times in `criterion_group!`'s `config`, which criterion's command line
overrides, never on a `BenchmarkGroup`, whose settings would beat the command
line, so MS-02's rounds can sample differently without editing a file; flat
sampling, which criterion sets only on a group and which has no command-line
flag, is the one setting each group makes itself.

At tier P's stated inputs the set cannot run in one 30-minute job. By tier
P's figures, under this sampling, its 138 operations measure for about 43
minutes before anything is built, and the three that read 10 000 pages are
18 of those minutes. So `bench.yml` becomes one job a group, a matrix whose
every entry names its group and its operation count, each job with its own
30 minutes; a run's wall time is its slowest job. Every relation tier P
draws between two operations has both in one group — the analysis group
carries its own text for that reason — so one job measures both on one host,
where across two jobs a ratio would measure the fleet (MS-10.1's note).

| Job | Operations | Measurement, by tier P's figures | Most of it |
| --- | ---: | ---: | --- |
| text | 12 | 14.8 min | text of 10 000 small pages, in-process and CLI, 6.6 min each |
| render | 33 | 8.0 min | 100 pages on one thread, 1.6 min; 20 pages to PNG on one job, 1.3 min |
| open | 6 | 6.0 min | every page's size over 10 000 pages, 5.2 min |
| layout | 39 | 5.4 min | the 1 000-chapter book |
| write | 16 | 3.3 min | rewrite of 3 000 pages, 1.0 min |
| decode | 9 | 2.3 min | the reversible JPEG 2000, 1.2 min |
| analysis | 8 | 2.2 min | the three validators |
| fonts | 7 | 0.5 min | — |
| encode | 4 | 0.3 min | — |
| sign | 3 | 0.2 min | — |
| calibration | 1 | 0.2 min | — |

The layout job's not-measured operations are taken at three times their
targets — the 1 000-chapter book at 3 s to paginate and 10 s to render — and
the analysis operations at their relations' bounds. Every job measures for
under half its limit, which leaves at least 15 minutes for its checkout, its
build and its generators, and a group whose job nears its limit splits by a criterion filter into
two entries with a count each, never by a changed input. Every job builds
its group's bench in release, as `cargo bench` does, and the five with CLI
operations build `tpdf` in release first. MS-02's paired job, three builds
and ten rounds of this set in 15 minutes, is priced in MS-02's section and
H-162's to decide.

#### The window

MS-04.9's `bench-check --sizes` reads each operation's mean from the run's
estimates — an iteration's, so `calls` included — and fails one outside
20–300 ms on the head, unless its list names it outside by the owner's word,
which it reports instead. It runs in each job after the bench, because the
window is a property of the runner the job landed on and never of
`cargo test`. One dispatch on `develop` in report mode prints every operation
outside with its mean; H-163 is
decided on that list, not on tier P's column. Until it is, every operation keeps
tier P's input, a fast one runs its `calls`, and MS-04.10 names the slow ones
outside as H-163 decides.

#### The two level-6 byte counts

Tier P holds the deflate ratio level to 1.02 times the size level 6's
parameters give each of the two inputs, exactly, and says that size is
recorded once, when MS-04 commits it, as a planning number
(`docs/roadmap/P.md:96-97`, `:137-142`; H-112). They live in
`benches/support/inputs.rs`, as two constants beside `page_like()` and
`photo_like()`, each with the date it was brought in and the length and
SHA-256 of the bytes it was taken on, which each generator asserts of its own
output. Not in `baseline.json`, which holds a machine's times and is read as
such: a byte count is neither. Beside the generators, because a count is a
fact about those bytes alone, and a generator changed under it would leave it
describing other bytes — so the digest fails the generator first, and a moved
digest is a finding, never re-pinned. MS-04.10 commits them, dated, as the
owner brings them in under H-112; P-04.4's `deflate_size.rs` reads them by
path. Nothing in the tree runs level 6: no first-party encoder has it, and an
outside one may not decide a build (ruling 13).

#### How the seven retire

Seven operations in the group files carry committed figures and bands, and the
set grows beside them. MS-04.10 measures the whole set, and the seven's figures
leave with the seven: three are tier P's own operations on the same input and
continue under tier P's names, one continues as itself, and three are succeeded
by the same call on tier P's larger input. None retires without a successor, so
H-78 is asked nothing for them.

| Operation | From MS-04.10 | Why |
| --- | --- | --- |
| `open/open a 3-page document` | itself, with `calls` | the set's only in-process open of a small file; P-21's held operation is the CLI's |
| `render/render text at 150 dpi` | `render/the text page at 150 dpi` | its page draws no glyph; the successor's does |
| `text/extract a page of text` | `text/text of 100 standard-14 pages` | the same call over standard-14 Helvetica, 100 pages for one |
| `render/render a full-page axial shading at 150 dpi` | `render/a full-page axial shading at 150 dpi` | the same generator, `axial()` |
| `render/fill 300 anti-aliased paths at 150 dpi` | `render/300 triangles at 150 dpi` | the same 300 triangles, `triangles()` |
| `write/rewrite a document` | `write/rewrite 100 pages` | the same call, 100 pages for three |
| `layout/paginate a book at 432x648 pt` | `layout/open and paginate pandoc-book-cover.epub at 432x648 pt` | the same book, box and call |

#### What later rows add

A row whose capability does not exist yet adds its operation in the commit
that adds the capability, under an id these rules allow, in the group its
operation belongs to, listed unmeasured in the machine entry until the next
fifteen-run measurement (H-109), and sized by `calls` or named outside under
H-163. Tier P's table and the Item name these:

| Row, step | Operation | Group | Tier P's gate |
| --- | --- | --- | --- |
| FJ-01, FJ-01.41 | `scripts/compile a 64 KiB document-level script` | scripts, creating `scripts.rs` | calibrated, ≤ 5 ms |
| FJ-16, FJ-16.1 | `scripts/a 500-field calculation cascade`, `scripts/500 field appearances regenerated` | scripts | calibrated, ≤ 20 and ≤ 25 ms |
| OC-17, OC-17.2 | `ocr/OCR a 300-dpi Latin page on one thread`, `ocr/OCR a 300-dpi Chinese page on one thread` | OCR, creating `ocr.rs` | calibrated, ≤ 1.5 and ≤ 4 s: above the window at their targets |
| SG-02, SG-02.7 | `sign/ECDSA P-256 signature`, `sign/ECDSA P-384 signature`, `sign/ECDSA P-521 signature`, `sign/ECDSA Brainpool P-256 signature` | sign | calibrated, ≤ 0.5, 1.2, 2.5 and 1 ms; `calls` 50, 21, 10 and 25 |
| P-09, P-09.9 | `decode/JPEG decode at quarter scale` | decode | 22.3×, paired with the baseline's full-size decode; `calls` 2 |
| P-10, P-10.4 | `decode/irreversible JPEG 2000, two levels discarded`, `decode/reversible JPEG 2000, two levels discarded`, and a window decode of each | decode | 25.6× and 36.2×, paired with the baseline's full decode; the windows recorded ungated (H-77) |
| P-10, P-10.12 | `decode/irreversible JPEG 2000 2480x3508 on 4 threads` | decode | 7.33×, paired with the baseline's one-thread decode |
| P-04's fast level, P-04.3 | `encode/deflate page-like data, fast level`, `encode/deflate photo-like data, fast level` | encode | 7.5× and 5.27×, at floor 1 until P-04.6 |
| MS-08, MS-08.4 | `open/first page of the 60-page linearized fixture over a slow link` | open | counted; the time follows from MS-08.3's counts, never slept |
| P-21, P-21.1 | `open/open a small file, CLI`; `decode/extract a Flate image with predictors`; `write/no-op incremental save of 100 pages` | open, decode, write | held |
| MS-09, MS-09.2 | the `threads` group: 100 text pages at 150 dpi on 1, 2, 4 and 8 threads through the CLI's pool, and the 300-dpi page on one thread and in four bands | `threads`, a fourteenth file | relations inside one run (MS-09.3) |

Tier P's "Time to first page over a source with 50 ms latency at 10 Mbit/s,
the 60-page linearized fixture" cannot be a name — it holds a `/` and runs
past 64 bytes — so MS-08.4's operation takes the id above. MS-10's two rows
are relations over this set, not operations of their own. The other rows'
speed rows come in by the same rule, each in the group its operation belongs
to.

### Risks

| Risk | Mitigation |
| --- | --- |
| A name criterion rewrites is recorded under another key, reported new and never held to its band | Every name ASCII, at most 64 bytes, free of criterion's ten replaced characters; `cargo xtask baseline` refuses one that is not |
| A cache a later row adds turns a cold operation warm | An iteration opens its document from bytes; only an operation whose name says so starts from prebuilt state |
| Two engines generating one input write different bytes, so a pair times two inputs | MS-02's paired job routes every input through the directory `TINKER_BENCH_FIXTURES` names, written by the head first |
| An input an engine encoder writes moves when the encoder changes (P-04.7's deflate, the JPEG encoder) | The row that changes the encoder names the operations it moved in its commit; the two inputs the level-6 counts describe are formulas pinned by length and digest, so they never move |
| The run does not fit 30 minutes | One job a group, each measuring under half its limit by tier P's figures; a group nearing its limit splits by a filter, never by its input |
| Tier P's figures are the planning machine's, so the budget and the window's list are predictions | MS-04.9's dispatch measures the list H-163 is decided on, and an operation outside that it missed goes to H-163 before MS-04.10 commits any figure |
| A repetition count and its committed `per_iteration` drift apart | Criterion's recorded `Throughput` is checked against the list on every run |
| A CLI operation times a failed child, or a stale `tpdf` | The exit status is checked on every call and a failure panics the bench; `tpdf` is resolved beside the bench binary and built from the same checkout in the same job |
| The committed codestreams grow the repository and come from a program this project does not own | Three files with their bytes, digests, coding and sources recorded; their decodes are checked first-party, the reversible one against the generator's input and the progressive one against the baseline decode |
| A format's aggregate hides one container fixture's regression | Every fixture is opened and rendered in the operation and asserted in test mode; a figure for each fixture would take ninety operations, which a row that needs one adds |
| The calibration operation moves | Its file is frozen; a change re-states every calibrated target, which is the owner's (H-77) |
| A row's speed-up takes an operation under 20 ms | The row raises its `calls` in the commit that raises its floor, with the figure re-measured under H-109, or names it for H-163; the four that cross the window are listed above |
| `ubuntu-latest` is a fleet, and one dispatch's jobs land on different hosts | Each band is per operation over fifteen dispatches, as the committed ones are (`docs/verification.md`, "The named machine is `ubuntu-latest`"); every job logs its CPU model and count, as `bench.yml` does |

## MS-07 — Corpus throughput and latency ratchet

In this design, each of the corpus job's three bars holds every corpus to
five time figures at the run's dpi — pages rendered a second, the mean
open-and-count latency, and the 50th, 95th and 99th percentile of a page's
render time — each inside a band set by the five runs the bar records, and
`corpus.yml` fails a figure past its band. Every run prints each corpus's ten
slowest files with their `/Producer`, and each bar records them. The figures
come from the runs the bars already make, four children at a time on the
runner, so the job's time does not grow.

### Scope

- **What the child times**: one span for opening a document and counting its
  pages, and one for each page's render at the run's dpi, both apart from the
  strict and metamorphic passes, printed in microseconds in record format 7.
- **The five figures** a corpus is held to, and how each is computed from
  those spans.
- **Where the times come from**: the shared run, how its concurrent children
  bear on each figure, and what a pass of its own would cost the 120-minute
  job.
- **The keys** the report and the three bars carry, and what `--check`,
  `--record` and `ratchet::parse` do with them.
- **The band rule** over five runs, and what it does not catch.
- **The ten slowest files** of each corpus, by `/Producer`.

### Non-goals

- **A clock in a test.** The record's, the report's and the bar's tests feed records and bars
  to the parsers and read no clock; the child and the runner keep the clocks,
  as this document's first non-goal and `docs/verification.md`'s "Clocks:
  outside the suite, and not nowhere" ask.
- **Timing the rest of a child.** The disk read, the process, the page
  lookup, the structure join, the strict rewrite and the metamorphic renders
  stay outside every span, and `ms` and `millis` span them. The page lookup is A1's and A2's to count (the roster, above) and
  MS-04's `open/every page's size` operations' to time.
- **A gate on a file.** The ten slowest are recorded and printed, never
  compared, and no file has a band of its own.
- **A figure that cancels the runner.** A calibrated figure divides by an
  operation the same job runs; the corpus job runs none, so these figures are
  absolute and their bands absorb the fleet (the band rule, below).
- **Other platforms.** `corpus.yml` runs on `ubuntu-latest` alone
  (`.github/workflows/corpus.yml:23`).
- **Absolute targets.** A figure here is held to its own five runs, never to a
  tier P target; absolute figures are MS-03's.

### Design

#### What the child times

Two spans, each read from `std::time::Instant` and printed in whole
microseconds.

- **Open-and-count**: `Document::open` on the bytes the file's read returned
  (`tools/tpdf/src/main.rs:860`), the face provider attached (`:861-864`), the
  authentication with the empty password or the sidecar's (`:866-872`), and
  the first `page_count` (`:2629`), which walks the page tree
  (`crates/tinker-pdf/src/lib.rs:1754-1756`). The read itself
  (`tools/tpdf/src/main.rs:859`) is outside, since it measures the runner's
  disk; so are `ladder_level` and the `/Producer` read between the open and
  the count (`:2606`, `:2618`), and every line the record prints. A file that
  does not open has no span: its failure is the pass rate's, and the time it
  spent failing stays in `millis`.
- **A page**: `page.render` at the run's dpi (`:2663-2666`, `:2685`), once for
  every page the loop reaches (`:2670`). The lookup before it,
  `doc.page(index)` (`:2671`), is outside: the walk to a page gathers
  every leaf up to it (MS-01.4, above), so a document's lookups grow with the
  square of its pages, and tier P's figures put every page's size over 10 000
  generated pages at about 28.5 s (`open/every page's size, 10000 pages`, in
  MS-04's table above); a span that held the lookup would make verapdf's
  figures its 10 000-page fixture's lookups. The structure join on a tagged file
  (`:2678`) and the progress line (`:2684`) are outside too.

Neither span reaches the strict pass or the metamorphic pass (`:2727-2729`),
which rewrite the document and render page one again (`:2129`, `:2159`,
`:2163`, `:2173`). A child the runner kills writes no complete record, and
the runner keeps nothing of one but its producer
(`xtask/src/runner.rs:459-485`), so a file that timed out or stalled has no
times; it is the pass rate's regression, and the producer table names it
(`xtask/src/report.rs:638`). Each child renders on one thread: the engine
spawns none outside its tests (the only `thread::spawn` under `crates/*/src`
are in `tinker-pdf-cos`'s test modules, `name.rs:305` and `store.rs:234`), and
P-15's pool, when it lands, runs serially unless a caller asks for threads
(`docs/roadmap/P.md:604`).

#### The figures

Per corpus, over the files whose records carry the spans:

| Figure | What it is | Kept as |
| --- | --- | --- |
| pages/s at the run's dpi | the pages timed over their summed render time | `pages`, `render_micros` |
| open-and-count latency | the mean over the files that opened: their summed spans over their count | `files`, `open_micros` |
| p50, p95 and p99 page render time | over the corpus's pages, not its files, by nearest rank: of n page times, the ⌈p × n ÷ 100⌉-th smallest | `p50_micros`, `p95_micros`, `p99_micros` |

Every figure is a time once pages/s is read as its reciprocal, so every band
reads one way: the run's figure may be at most (100 + band) % of the bar's.
The two means stay a count and a sum, and every comparison is a
cross-multiplication in `u128`, as every other axis in `ratchet.rs` is
(`xtask/src/ratchet.rs:1-26`, `:200-207`). Every key is a whole number, which
`json.rs` holds exactly below 2⁵³ (`xtask/src/json.rs:42-46`, `:105-107`), and the
largest, a corpus's summed render time in microseconds, sits far below it.

**Over pages, not files**, because a page is what a reader waits on; it makes
a corpus's percentiles its long documents': most of verapdf's pages are one
10 000-page implementation-limit fixture's, so that file is most of the
population verapdf's percentiles are taken over. **The latency is a mean**
rather than a percentile because it moves when any file's open moves — the
common path a thousand times over, or one repair — where a percentile is
blind to everything on one side of it; the report keeps each file's span,
which is where the tail is read.

#### Four children at once, and what a pass of its own would cost

`corpus-run` runs one child a logical CPU unless told otherwise
(`xtask/src/corpus.rs:192`, `:628-656`), and `corpus.yml` never tells it
(`.github/workflows/corpus.yml:152-154`, `:190-193`, `:220-223`): four, on the
runner's four logical CPUs. A span is wall time, so a page's includes what its
three neighbours cost it — shared caches, memory bandwidth, and a core where two
logical CPUs share one — and seldom a wait for a CPU: four single-threaded
children do not outnumber the runner's logical CPUs, and the runner's own four
worker threads sleep between polls (`xtask/src/runner.rs:451`). That cost has
two parts. One is systematic, every child somewhat slower than it would run
alone, and a bar measured at the same job count carries it inside its figures;
the bar holds that constant by refusing times from a run at another job count.
The other varies with which files share the machine with which, and the five
runs' spread measures it. It sits under what the fleet does across hosts
(`docs/verification.md`, "The named machine is `ubuntu-latest`"), which every band has to absorb
anyway.

**So the times come from the shared run**: each bar's own `corpus-run`, at the
job count it runs at. Its cost is two clock reads and one record line a page,
and the job's time does not grow. A pass of its own, one child at a time over
open and render alone, would remove the neighbours and nothing else, and would
run each bar's files end to end rather than four at a time inside the job's
120 minutes; it does not buy enough of the smaller term to be worth that.

#### The record, the report and the bar

**The record** is version 7. After `pages`, a file that
opened prints `time open <micros>`: `Document::open` on bytes already read,
the faces, the authentication and the first `page_count`. The read, which
times the runner's disk rather than the engine, is outside it, and so are the
ladder, the producer and the lines printed between them
(`tools/tpdf/src/main.rs:2613-2623`, `:2671-2675`). After each page's render
it prints `time page <n> <micros>`, the render alone, n counted from 1 as the
progress line counts (`:2729-2738`). The page's lookup is outside that span:
the walk to page n gathers every leaf before it
(`crates/tinker-pdf-cos/src/pages.rs:486-498`), so over a long document the
lookups grow with the square of its pages. `time` is its own key; `page` is
the progress line's, which the runner never reads (`:2725-2728`).
Version 7 rather than a quiet new key, for `peak`'s reason
(`xtask/src/runner.rs:604-617`): the time figures are made from the spans, so
a version-6 child read as version 7 would be a corpus whose files timed
nothing rather than a `tpdf` nobody rebuilt, and the handshake exists to name
the second (`xtask/src/corpus.rs:749-794`). The runner keeps a file's open
span as an `Option` and its page spans as a list of page and microseconds
(`xtask/src/runner.rs:131-151`, `:222-229`), and drops a line that does not
parse, or that names a page outside 1 to `pages`, rather than reading it as
zero (`:690-703`, `:818-821`).

**The report** carries, per file, `open_micros`, `render_micros` (its pages'
sum) and `slowest_page` (`{ "page", "micros" }`), each absent where there was
nothing to time; per corpus, in its `summary` (`xtask/src/report.rs:406-450`),
`time` — `files`, `open_micros`, `pages`, `render_micros`, `p50_micros`,
`p95_micros` and `p99_micros` — and `slowest`, the ten slowest; `jobs` in its
`settings` (`:237-250`, `:395-404`); and a `host` beside them,
`/proc/cpuinfo`'s first model name and the logical CPU count, absent off
Linux, so that a figure that moved with the host is attributed rather than
banded, which is why `bench.yml` logs its host
(`.github/workflows/bench.yml:62-70`). Its summary lines
(`xtask/src/report.rs:555-619`) carry a `time` line per corpus — pages/s at the
dpi, the latency and the three percentiles — and one for the run,
`time   N pages timed   M files opened`, which each bar's step in
`corpus.yml` greps as it greps `strict` (`.github/workflows/corpus.yml:172`).

**The bar** carries `time_runs` at its top and `time` and `slowest` in each
corpus, and each run's per-corpus figures go into it whole:

```text
"time_runs": {
  "dpi": …, "jobs": …, "revision": "…", "recorded": "…",
  "dispatches": [ { "id": "…", "host": "…" }, … five in all … ]
},
"corpora": [
  { "name": "pdfjs", …,
    "time": {
      "total": …,
      "runs": [ { "files": …, "open_micros": …, "pages": …,
                  "render_micros": …, "p50_micros": …,
                  "p95_micros": …, "p99_micros": … }, … five in all … ]
    },
    "slowest": [ { "path": "…", "producer": "…", "pages": …,
                   "micros": … }, … up to ten … ] },
  …
]
```

`time.runs` holds the five runs' `summary.time` objects in the order
`dispatches` names them, and `time.total` the corpus's file count when they
ran. No band is a key: `ratchet.rs` derives each from the five runs by the
rule below, so none is typed by hand.

- **`--check`** holds each figure to its band, and one figure past its band
  fails the run alone. It refuses the times of a run at another dpi or job
  count than `time_runs`', and those of a corpus whose file count is not
  `time.total`, as it refuses a run under another `--fonts`
  (`xtask/src/ratchet.rs:251-258`): each is a different measurement. A run
  that timed fewer files or pages in a corpus than the fewest of the five
  fails, as a peak over fewer children does (`:567-576`). A figure faster
  than the five's fastest is a note, not an improvement, because one run
  measures no band. A bar with no `time_runs` is an improvement, new with no
  recorded band, and `SCHEMA` stays 2, as it does for `peak`
  (`xtask/src/report.rs:23`, `xtask/src/ratchet.rs:82-90`).
- **`--record`** writes every other axis from its run
  (`xtask/src/corpus.rs:508-524`), and the run's ten slowest; it carries
  `time_runs` and each corpus's `time` over from the committed bar unchanged,
  and never writes a time figure from one run. A figure from one run would
  ratchet the bar onto whichever host that run landed on, and the next run on
  the fleet's other group would fail on the machine rather than on the code.
- **`ratchet::parse`** refuses a time block of other than five runs, or whose
  runs do not match `dispatches`; a run with more files than `total`; and a
  zero figure over files or pages that were timed, a band nothing can sit
  under, as it refuses a peak of zero bytes (`xtask/src/ratchet.rs:756-761`).

#### The band rule over five runs

The five runs are five dispatches of `corpus.yml` at one revision of
`develop`; a run at another revision is not one of them. They need not be
green: every bar's step runs whatever the one before it returned
(`.github/workflows/corpus.yml:65-71`), and the report is kept whatever the
outcome (`:232-244`). For each corpus and each figure:

- **The figure is the fastest of the five**, for the reason the bench's is the
  fastest of fifteen: the comparison is one-sided, so the bar worth holding is
  the best the runner has done (`docs/verification.md`, "The named machine is `ubuntu-latest`").
- **The band is the spread fifteen runs would show, set as the bench sets one
  from fifteen.** The five runs' spread — slowest over fastest, less one — is
  grown by 1.53, which is what the bench's widest spread grew by from five
  runs to fifteen, then made half again as wide and rounded up to the next
  5 %, the bench's own rule: 2.295 times the spread,
  rounded up.
- **And never under 20 %**, the swing the bench's noisiest operation showed
  within one of the fleet's two groups, so a figure whose five
  runs all landed on one group is not held tighter than that group has been
  seen to move.

So a spread of 3 % is banded at 20 %, one of 10 % at 25 %, and one of 36 % —
what the bench's fifteen runs put between their fastest and slowest scores,
1.00 and 1.36 — at 85 %. Every recorded run holds the band it sets: its
figure is at most the slowest, and the band is above the spread. `ratchet.rs`
derives each band in integers, as it compares.

**What this does not catch.** A regression inside its band: on a figure
banded at 85 %, a page render half again slower passes in silence, and a
several-fold regression would not. This is
the corpus's ratchet against the catastrophic, on pages real producers wrote;
the finer instruments are MS-02's paired job over MS-04's set and MS-01's
counts, which the fleet does not move.

#### The ten slowest

Per corpus, the ten files whose spans sum the most — open-and-count and every
page — slowest first, ties by path, each with its corpus, its path, its pages
timed and its sum, under its `/Producer` as the record states it
(`xtask/src/runner.rs:159-168`; `(none stated)` is a producer,
`tools/tpdf/src/main.rs:2618-2627`). The table is `producer_table`'s shape,
with its escaping of a producer's pipe (`xtask/src/report.rs:638`, `:680`),
printed after it on every run (`xtask/src/corpus.rs:491-495`), in each
corpus's report summary, and in the bar as `slowest`; nothing compares it. A
file with no spans is not ranked, so a file the runner killed is in the
producer table instead. Ranking by a file's sum favours long documents; the
report's `slowest_page` names the page that cost a file most.

### Risks

| Risk | Mitigation |
| --- | --- |
| A child's neighbours move its spans | Four single-threaded children on four logical CPUs; the job count is in `time_runs` and a run at another is refused; the five runs' spread measures the rest |
| `ubuntu-latest` is a fleet, and five runs may land on one group of it | The fastest of five is the figure, and the band is 2.295 times the spread and never under 20 %; each run's host is recorded beside its id |
| Five runs read a spread below what fifteen would | The 1.53 the bench measured for tripling its sample; a band that fails on unchanged code is a finding, re-measured over five more runs and never widened by hand |
| A run that times fewer files or pages reads as a faster one | Files and pages timed are held to the fewest of the five; a killed child's spans are dropped, and its timeout is already the pass rate's regression |
| One run's figure ratchets the bar onto a fast host | Only five runs set figures; `--record` carries them and writes none; a faster figure is a note |
| A re-pinned corpus, a new runner size or another dpi changes what is measured | Each is refused by name — `time.total`, `time_runs.jobs`, `time_runs.dpi` — until five runs re-measure, so a re-pin costs five runs before the job is green again |
| A P row's speed-up leaves its corpora's bands slack | The bar is a backstop against the catastrophic; re-measuring is five more runs, never `--record`; MS-02's floors are what a speed-up raises |
| The clocks cost the child time | Two clock reads and one line a page |
| A probe that starts using threads changes what neighbours cost | The engine spawns none, and P-15's pool will run serially unless asked; a row that gives the probe threads re-measures the time block |
| A corpus's percentiles are one long document's | Said above for verapdf; the report keeps each file's sum and slowest page, so a moved percentile is followed to its file |
| A zero figure bands nothing | `parse` refuses one over files or pages that were timed |
