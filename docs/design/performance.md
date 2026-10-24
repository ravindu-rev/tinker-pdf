# Performance

When this is done, a change that makes an operation of this engine do more
work than its input calls for fails `cargo test` on linux, windows, macos and
`wasm32-wasip1`, and one that makes an operation slower than the planning
baseline fails a paired job beside it. Today neither can. `bench.yml` runs on
dispatch and on a weekly schedule that measures the default branch, and it
gates nothing (`.github/workflows/bench.yml:24-40`). The three tests tier 0
names as its counted-work tests hold by finishing rather than by counting, so
a quadratic in any of them is a slow test and not a failing one. What does
count is seven `cfg(test)` thread-locals in six crates, each readable only by
its own crate's unit tests, and none of it through the facade, which is where
ruling 11 says a guard has to stand.

This document is the design the speed rows share. Its first five sections are
common to all of them — how a target gates, and which instrument holds each
kind of gate — and they are also MS-01's own design: the `Work` tally, the
doors that hand it out, the guard suite that reads it and the four legs that
run it. After them, each other row the roadmap points here writes a section of
its own, in the order the last part of this document gives.

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
- **A section for each row that adds one**, in a stated order.

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
- **The seven `cfg(test)` counters.** They stay until a row that reads one
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
target gates because the owner set it (H-77), never because of the
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
shared runner sees last. That is why each P row lands with its counted guard,
failing against the code before its fix (`docs/roadmap/P.md:19-20`).

**A count gates where a time cannot.** A hosted runner's worst benchmark swung
88.36 % across fifteen runs of one revision (`docs/verification.md:1796`), and
under wasmtime there is no benchmark job at all; a count of the same work is
the same number on all four targets.

### What a count is, and what the tree counts today

A count is a number of steps of one named kind that one operation took: a
`/Pages` node read, a filter chain run, a font dictionary resolved, a cache
asked. A guard is a relation over counts — a bound (`page(i)` reads at most
depth × fan-out nodes) or one count at two sizes (the work of 4 000 pages at
most 4.4 times the work of 1 000). Neither depends on the machine, the build
profile or the target, and the guard suite shows that by pinning counts on all
four.

The tree has nothing of the kind. The three tests tier 0's row names hold by
finishing: `a_hostile_run_of_regional_indicators_is_linear` builds 200 000
regional indicators and checks the boundaries it gets back
(`crates/tinker-pdf-content/src/words.rs:697-701`),
`moving_many_children_is_linear` parses 60 000 children and checks the tree
(`crates/tinker-pdf-xml/tests/html.rs:308`), and
`a_line_of_nothing_but_marks_is_linear_and_whole` orders 20 001 characters and
checks none is lost (`crates/tinker-pdf/src/text_order.rs:1048-1056`). The
counting that exists is under `cfg(test)`: seven thread-local step counters in
six crates (`crates/tinker-pdf-xml/src/html/mod.rs:326-330`,
`crates/tinker-pdf/src/epub/paint.rs:6364`,
`crates/tinker-pdf/src/text_order.rs:677-684`,
`crates/tinker-pdf-cos/src/build.rs:2339-2342`,
`crates/tinker-pdf-layout/src/flow.rs:6855-6858`,
`crates/tinker-pdf-shape/src/bidi.rs:1815`,
`crates/tinker-pdf-filters/src/jbig2.rs:5869`). The HTML tree builder's
`assert_linear` is what one of them can do inside its crate
(`crates/tinker-pdf-xml/src/html/tree.rs:3305-3319`); an integration test can
read none of them.

### The tally, and the value a door hands out

**Shape.** Two types, both in a new `crates/tinker-pdf-cos/src/work.rs`.

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
MS-01.2's `stats_do_not_change_a_pixel` and MS-01.3's byte-identity checks hold
it.

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

**The counters MS-01 lands.**

| Counter | One unit is | Counted in | Step |
| --- | --- | --- | --- |
| `glyphs_shown` | a glyph the interpreter showed the renderer (`Device::show_glyph`, `crates/tinker-pdf-render/src/lib.rs:2847`) | the renderer | MS-01.2 |
| `outlines_filled` | a glyph outline the renderer built into a path and painted | the renderer | MS-01.2 |
| `font_dictionaries_resolved` | a font dictionary read: each font `cos_font::from_resources` returns when a scope is built (`crates/tinker-pdf/src/resources.rs:585`), and each `fonts::font_dict` lookup (`crates/tinker-pdf/src/fonts.rs:215`), which `type3_glyph` makes for every glyph (`resources.rs:1428`) | the facade's scopes | MS-01.2 |
| `images_decoded` | an image decoder run: an image XObject's cache miss, and every inline image, which is never cached (`resources.rs:222-224`) | the facade's scopes | MS-01.2 |
| a `_hits` and a `_misses` for each of the five caches `Caches` holds — font programs, images, ICC transforms, glyph outlines, form scopes (`resources.rs:63-101`) | a lookup that found an entry, a cached `None` included, or one that did not | the facade's scopes | MS-01.2 |
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
| A render: `Page::render` (`crates/tinker-pdf/src/lib.rs:2247`), `Page::render_form` and `Page::render_annotation` (`crates/tinker-pdf/src/render_part.rs:175`, `:230`), `DisplayList::render` (`display.rs:181`) | the render's `PageResources` holds it — built with it by `Page::render_layer` (`lib.rs:2269-2280`), handed it by `for_one_render` for a display list (`resources.rs:546`); `annots::draw` takes it for the appearances it draws; the page's content decode, made before the scope exists (`lib.rs:2248`), takes it too | MS-01.2, and MS-01.5 for decodes |
| A scope a scope makes: a pattern cell's, a mask group's or a form's own (`resources.rs:750-764`, `:1858-1868`) | inherits its maker's | MS-01.2 |
| A scope a cache keeps: a form's, in `form_scopes` (`resources.rs:856-869`) | kept without one, and handed out bound to the asker's, its caches shared so a hit stays a hit — a kept scope outlives the render that built it, and two renders of one display list may hold it at once | MS-01.2 |
| The renderer | its own plain counts, handed back with `finish` and added by the door | MS-01.2 |
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
Of the 85 `RenderOptions` literals in the tree, every one but its own
`Default` (`lib.rs:705-718`) names a base, and 12 of the 13 `SaveOptions`
literals do; the thirteenth, `tools/tpdf/src/writing.rs:564`, gains one in
MS-01.3. A door with no options value, or one whose callers write it out whole,
gets a counted sibling instead. `TextOptions` is written out whole 113 times —
31 under `crates/tinker-pdf/tests`, 82 in the probe kit's sources under
`tools/probes/rtl` — so a field on it breaks every one of them, which is the
constraint FT-17.1 is already designing around (`text_order.rs:128`).
`Document::page` and `Page::images` take no options at all, and the validators
take only the rule groups to run, a value each verdict reports back as what
ran and no place for a flag.

**The answer.** A flag's answer rides on the result as `stats: Option<Stats>`,
`None` unless asked. A counted sibling returns `(answer, Stats)`, because its
answer may be nothing at all — `page(i)` past the last page still walked the
tree — or a list, or a verdict whose fields other code compares.

| Door | Asked by | Answered by | Step |
| --- | --- | --- | --- |
| render: `Page::render`, `render_form`, `render_annotation`, `DisplayList::render` | `RenderOptions::stats: bool` | `Bitmap::stats: Option<Stats>` | MS-01.2 |
| text | `Page::text_counted(&self, &TextOptions)` | `(TextPage, Stats)` | MS-01.3 |
| save: `write::save` | `SaveOptions::stats: bool` | `Saved::stats: Option<Stats>` | MS-01.3 |
| merge: pages imported, then saved | `DocumentEditor::count_work(&mut self)` before the first import, and `SaveOptions::stats` | `Saved::stats`: everything the editor counted since `count_work`, the save's own work included | MS-01.3 |
| image extraction, the decode door | `Page::images_counted(&self)` | `(Vec<PageImage>, Stats)` | MS-01.3 |
| validation | `Document::validate_pdfa_counted(&self, PdfACoverage)`, and `validate_pdfua_counted` and `validate_pdfx_counted` alike | `(verdict, Stats)` | MS-01.3 |
| page lookup, the document door | `Document::page_counted(&self, index: u32)` | `(Option<Page>, Stats)` | MS-01.4 |

`Bitmap::stats` is a public field, not the method the row writes as
`Bitmap::stats()`. A method needs a field the caller cannot see, and one
private field would stop every hand-built `Bitmap` compiling, where a public
one asks each of them for `stats: None`. In the tree that is eight literals
outside the facade's own source — five in `crates/tinker-pdf/tests/png_output.rs`,
two in `png_input.rs`, one in `tools/pdfcmp/src/main.rs` — and three inside it
(`lib.rs:933`, `:2424`, `crates/tinker-pdf/src/png_read.rs:199`); nothing in
the C ABI or the bindings builds one. `premultiplied` arrived the same way and
touched the same eight (d47b2629). `Saved::stats` joins `Saved`'s other public
fields; its one literal is the facade's own (`write.rs:368`).

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
cause, flipping it from owed to landed, and the commit names the run in which
the guard failed against the code before the fix. A guard that holds at the tip
— A2's subset, validation and split legs (`crates/tinker-pdf/src/subset.rs:552`,
`crates/tinker-pdf/src/pdfa/content.rs:90`) and A10's flat entries — lands with
a counted injection instead, which MS-01.9 reads as its failing run; that is
the cut's reading of the exit's words, for the owner to correct. An entry a
later row adds already landed, as P-18.5 adds A2's book, is never owed and adds
nothing to MS-01.9's waits.

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
  is red, not green" names (`docs/verification.md:1567`), and the greps are
  what close it.
- **`wasm-determinism`** runs the same two files on `wasm32-wasip1` under
  wasmtime, in the default build as its four steps do today
  (`ci.yml:375-459`), with the same greps.
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
wasmtime. MS-01.8 reads each leg's wall time from its run; a guard too slow for
a leg is never ignored or shrunk below the exit's page counts, and what keeping
it costs goes to the owner. `ci.yml` has fired on every push to `develop` since
79506900 (`ci.yml:14-15`), so MS-01.8's note that it fires only on `main` and
pull requests until MS-11 adds `develop` is out of date.

## Milestones

Each step is one workflow run, and its exit is quoted as the step states it in
`docs/roadmap/MS.md`. Each build step times its paths with stats off against
the tip, in release and in debug, before it writes its tests, and stops if off
is not free.

| Step | Deliverable | Exit, as the step states it | Waits on |
| --- | --- | --- | --- |
| MS-01.2 | `Work` and `Stats`, and the render door: `RenderOptions::stats` and `Bitmap::stats` over the render's scopes and the renderer | `tests/stats.rs` (new) `render_stats_are_none_unless_asked`, `render_stats_count_what_a_generated_page_draws` (a page of n glyphs in k fonts with one image drawn twice reports n glyphs shown, its font resolutions as today's code makes them, one decode and one cache hit) and `stats_do_not_change_a_pixel` (stats on and off, page and tiles, byte-identical); a doubled increment injected at each site fires. Default and `bundled-fonts` builds. | MS-01.1 |
| MS-01.3 | The text, save, merge, image-extraction and validation doors | `tests/stats.rs` `text_stats_count_font_resolutions`, `save_stats_are_none_unless_asked`, `a_merge_reads_its_stats_from_the_save` (4 × 10 generated pages imported and saved) `image_stats_count_the_decodes_made` and `validation_stats_count_its_page_walks`: each count is what today's code makes, derived in the test; stats on leaves every output byte-identical; a doubled increment injected at each new site fires. Both builds. | MS-01.2, K-02.3 |
| MS-01.4 | Page lookup counted, and the document door `Document::page_counted` | `tests/stats.rs` `page_lookup_counts_the_nodes_it_visits`: on generated balanced trees of known depth and fan-out, `page(i)` reports the nodes today's walk visits (`collect_upto` gathers index + 1 leaves, derived in the test), a streamed linearized first page reports its head-only path, and an injected double count fires; `streaming_open.rs` passes unchanged. | MS-01.3 |
| MS-01.5 | Decoding counted, per stream and per operation, at every door | `tests/stats.rs` `decodes_are_counted_per_stream_and_operation`: a generated page whose form is shown three times reports the decodes today's code makes, and the inflated total equals the decoded lengths; a subset-on-save of a generated document reports each font program's decodes per page as today; an uncounted decode path injected fires. Both builds. | MS-01.4 |
| MS-01.6 | The guard suite: generators, the roster below with every entry owed, the pinned counts, no clock | `guards.rs` (new) `every_guard_is_landed_or_owed` (each roster entry is a `#[test]` in the test file it names, not ignored, or owed to a named row; it fails when a landed name is missing or an owed one is already there), `the_tally_counts_the_same_on_every_target` (pinned `Stats` of a render, a text, a save and a page walk), `the_guard_suite_reads_no_clock`, and each generator asserting its own shape. Both builds. | MS-01.5 |
| MS-01.7 | Allocations per page today, and A10's entries settled from them | `guard_allocations.rs` (new) `allocations_per_page_by_operation` prints each operation's allocations per page at both sizes, the table is committed in `docs/design/performance.md`, and A10's roster entries follow it: an operation flat at both sizes lands its guard green, firing when an allocation that grows with the page index is injected; each other is owed to the row the doc names for its cause. | MS-01.6 |
| MS-01.8 | The suite on the four targets, and the pins in K-07's roster | The next CI run on `develop` is green on linux, windows, macos and `wasm32-wasip1`, each leg's log showing `every_guard_is_landed_or_owed` and `the_tally_counts_the_same_on_every_target` passed and `cargo xtask check` listing the pinned counts in the roster; a guard a P row lands later joins the greps in that row's commit. | MS-01.7, MS-11, K-07.1 |
| MS-01.9 | The suite read to its end once every owning row has landed its guards | A `CI` run on `develop` green on linux, windows and macos in both builds and on `wasm32-wasip1`, each leg's log showing every A1–A10 guard passed and the roster owing none, and, read from `git log`, each guard that fails at the tip landed with the run in which it failed against the code before its fix named in its commit, and each that holds at the tip (A2's subset, validation and split legs, A10's flat entries) with its counted injection. | MS-01.8, P-01, P-02, P-03, P-04, P-06, P-09, P-10, P-15 |

**What each step needs beyond its text**, from the tree as this design reads
it. Each step's own review corrects its text against this when it is taken.

- **MS-01.2** also creates `crates/tinker-pdf-cos/src/work.rs` and its line in
  cos's `lib.rs`, the tally's home; threads the render's tally through
  `render_part.rs`'s two doors and `annots::draw`; and gives the eleven `Bitmap`
  literals `stats: None` (`png_output.rs`, `png_input.rs`,
  `tools/pdfcmp/src/main.rs`, `png_read.rs`, `lib.rs`). Its text's
  `Bitmap::stats()` is the field `Bitmap::stats`.
- **MS-01.3**'s text door is `Page::text_counted`, not `TextOptions::stats`, so
  `TextPage` gains no slot and neither `text_order.rs` nor
  `tinker-pdf-content` changes for it. It adds `DocumentEditor::count_work`
  (`crates/tinker-pdf-cos/src/edit/mod.rs`), gives
  `tools/tpdf/src/writing.rs:564`'s `SaveOptions` a base, and binds the scopes
  the save's font pass builds (`subset.rs:553`, `:560`).
- **MS-01.4** counts the editor's walks as well as the facade's
  (`edit/page_ops.rs:16-24`, `:185`), and moves MS-01.3's `page_walks` count
  from the facade's call sites into `collect_upto`, so a walk counts once
  whoever starts it.
- **MS-01.5** reaches the decodes under a scope's font reading through the
  counting view (`font.rs:601-602`), the page's content decode through
  `content_bytes`'s counted sibling (`pages.rs:526-537`), and the editor's own
  decodes through its overlay (`edit/overlay.rs:48`).
- **MS-01.8** edits K-07.1's `xtask/src/determinism.rs` rather than creating
  it, and its note about `ci.yml`'s triggers is out of date (the four targets,
  above).

### The roster

Every entry MS-01.6 writes starts owed, because every A1–A9 clause fails
against today's code or, where it holds, lands with its row's counted
injection; A10's entries wait for MS-01.7, and A2's book is P-18.5's to add,
landed. A guard lands in the step named, in `crates/tinker-pdf/tests/guards.rs`
unless another file is named.

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

### Allocations per page, today

Owed: MS-01.7 commits the table here — allocations per page for page lookup,
text and render-all at 36 dpi, over every page of the generated 1 000- and
4 000-page documents, in both builds — with, for each operation that is not
flat, the row its cause belongs to.

## Risks

| Risk | Mitigation |
| --- | --- |
| A count drifts from the work it names: a site moves, a path is added that does not count, a refactor counts twice | Every count MS-01.2 to MS-01.5 lands is checked against a figure the test derives from its fixture, and a doubled increment injected at each site fires; MS-01.5 injects an uncounted decode path and its test fires |
| Counting changes what an operation does | Nothing reads a `Work` but a door; a kept scope handed out bound shares its caches, so a hit stays a hit; `stats_do_not_change_a_pixel` (pages and tiles) and MS-01.3's byte-identity checks hold it |
| Off is not free | An uncounted operation carries `None`: no lock, no allocation, one tested `Option` a site; each build step times its paths with stats off against the tip in release and in debug, and MS-02's floors of 1 hold every operation to the baseline once that job exists |
| Two threads' counts mix | One `Work` an operation, never on `Document`, in a global or in a thread-local; an operation's own workers add to one total in any order |
| Hits and misses depend on interleaving when threads share a cache | Guards and pins read serial runs; A9 compares pixels and warnings, not counts; P-15.6's single-flight fills make a shared cache's counts deterministic and its test reads them |
| A count differs by target | `u64` counters with saturating adds, a `BTreeMap` for the per-stream decodes, no count of a hash-ordered walk; the pins are read on all four legs |
| The pins are goldens and the P rows exist to cut work | MS-01.6 pins counts that are the fixture's rather than the algorithm's, and says which; a pin a fix has to move is a finding the owner rules on, raised by the row that moves it |
| A2 reads only what is counted | A superlinear loop that counts nothing passes A2; P-01.1 records each leg's superlinear site at the tip and the counter that sees it, and a site outside P-01 is a finding, not absorbed |
| A byte total hides a step count | `work()` leaves out maxima and quantities, and a failing leg prints every counter's ratio |
| A guard is absent and reads as green | The roster checks each landed name is an un-ignored `#[test]` in its file, and every leg greps every landed name (`docs/verification.md:1567`) |
| A guard is too slow for a leg, in debug or under wasmtime | Pages carry the least content that exercises their leg; MS-01.8 reads each leg's wall time; a slow guard is never ignored or shrunk below the exit's page counts, and what keeping it costs goes to the owner |
| A process-wide allocation count sees another test's allocations | One counted test a binary, or a per-thread count in `guard_support`; MS-01.7 settles which |
| A hand-built `Bitmap` stops compiling | The field is public, so a literal needs `stats: None`; the tree's eight outside the facade change in MS-01.2, as they did for `premultiplied` (d47b2629); nothing in the C ABI or the bindings builds one |
| `Stats` names counts made above cos | Each counter's doc comment names the layer that counts it, and cos counts only its walk, its filter chain and its editor; the alternative was two ledgers an operation, and an editor able to carry only one of them |

## The rows' sections

Each row below writes its own section, under its own heading, with its own
scope, non-goals, design, milestones with exit tests, and risks. The order is
the tiers' and then the rows' own; a section is written by the step named and
holds one paragraph saying what is owed until it is.

## MS-02 — A paired base-versus-head wall-clock gate on every pull request

Owed. MS-02.1 writes it: the paired protocol, with base, head and `61adcde`
built in one job and the suite run interleaved ten rounds; the verdict; where
floors, calibrated targets, relations and A/A bands are committed; how the
head's bench files build against `61adcde`; the triggers; and the 15-minute
budget. MS-02.5 prices that budget on MS-04.10's measured set.

## MS-04 — The benchmark set, replacing the seven

When this is done, `cargo bench` times every operation of tier P's target
table that the tree can run, and the row's own, in thirteen groups of one file
each: 138 operations over inputs the benches generate or the tree holds, each
held by `bench.yml` to a band measured on `ubuntu-latest`. MS-02's paired job,
MS-05's probe, MS-09's thread group, MS-10's wasm ratio and every P row's
floor are built on this set, and each later capability row adds its operation
to it in the commit that adds the capability.

Today it times seven operations, all in `crates/tinker-pdf/benches/engine.rs`
(`:135-188`), one `[[bench]]` (`crates/tinker-pdf/Cargo.toml:111-116`), over
inputs of one to three pages and one committed book read from disk
(`engine.rs:122-128`). `bench.yml` counts seven criterion lines
(`.github/workflows/bench.yml:92`) and `cargo xtask baseline` holds the file,
that count and `baseline.json` to one list (`xtask/src/bench.rs:495-587`). Its
"render text at 150 dpi" draws no glyph, by its file's own account
(`engine.rs:85-93`); nothing decodes an image, encodes a PNG, walks more than
three pages, merges, encrypts, subsets, signs or runs a thread.

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
- **Memory and wasm.** MS-05's probe builds its cases from these inputs
  (MS-05.1) and keeps its own bands. MS-10.1 builds this set for
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

One `[[bench]]` per group file, `harness = false`, as today's is. `inputs.rs`
holds no criterion type, no clock and no spawn, so a test reaches it by path
alone — MS-05's probe for its cases (MS-05.1) and P-04's `deflate_size.rs` for
the level-6 counts (P-04.1's exit) — without compiling the harness or the
child. `cli.rs` is the only file here that calls `Command::new`, so it is the
one `SPAWNERS` row `cargo xtask oracles` needs (`xtask/src/main.rs:984-1048`,
`:1076-1079`). Every input is generated in memory except what `data/` and
the tree already hold, and a CLI operation's files are written once a process
into `CARGO_TARGET_TMPDIR`'s `bench-fixtures/`, a directory cargo gives every
bench target, or into the one `TINKER_BENCH_FIXTURES` names: that is how a
paired job can hand two builds the same bytes, if MS-02.1 so decides.

The generators call only what `61adcde` already exposes — the builder calls
used below (`archival`, `tagged`, `output_intent`, `add_embedded_font`,
`add_base_font`, `add_image`, `add_icc_color_space`, `add_tiling_pattern`,
`set_info`, `raw`), the encoders (`jpeg_encode`, `ccitt_g4_encode`,
`jbig2_generic_encode`, `png_encode`, `deflate`, `zlib_compress`),
`BilevelCodec::Jbig2Generic`, `add_annotation`, `annot::square`, `FromHtml`
and `Document::open_markdown` are all public at that revision — so MS-02.4's
mapping onto the planning baseline covers operation bodies only.
`PageBuilder::text_pieces` came later, which is why producer-style lines are
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
reports milliseconds alone. A render's pixels are its bitmap's; a decode's
are the image's; the ICC conversion's are the image's, because tier P states
that target in the image's Mpixel/s.

**The iteration** is one of four shapes, and the shape is part of what the
operation measures.

- **From bytes.** An iteration opens the document from its bytes, handed as
  an `Arc<[u8]>` so no copy is timed (`Document::open`,
  `crates/tinker-pdf/src/lib.rs:1369`), and does the operation. A render opens
  its document every time, so that a cache a later row keeps on the document —
  P-16's — cannot turn a first render into a warm one unnoticed. An open is
  lazy, and small beside 20 ms: the seven's 3-page open is 0.0145 ms on
  `ubuntu-latest`.
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
| `small(n)` | n pages of one Helvetica line each, the line `engine.rs:54` writes | the builder's flat page tree, one `/Kids` of every page (`build.rs:8508-8516`) | "small pages", "10 000-page document", "rewrite 100 / 1 000 / 3 000 pages" |
| `three_pages()` | the seven's 3-page document | `engine.rs:43-60`, moved | — |
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
| `jpeg_page()`, `flate_page()`, `cmyk_page()` | an A4 page covered by one 2480×3508 image: `baseline_jpeg()`; `photo(2480, 3508, 1)` through `zlib_compress` (`deflate.rs:218`); `photo` converted to CMYK by formula and Flate-coded under `/ICCBased` with `cmyk-lut.icc` (`add_icc_color_space`, `build.rs:6356`) | `add_image` | "full-page JPEG", "Flate RGB image page", "an ICC-based CMYK image"; the CMYK image is not DCT-coded, since `jpeg_image` never reads `/ColorSpace` (`crates/tinker-pdf/src/resources.rs:253-269`) |
| `jpx_page()` | an A4 page covered by `data/`'s irreversible codestream | `ImageFilter::Jpx` (`build.rs:438`) | "full-page JPEG 2000" |
| `g4_page()`, `jbig2_page()` | an A4 page covered by `scan()`: G4-coded (`ccitt_g4_encode`, `ccitt/encode.rs:386`, as `ImageFilter::CcittFax`); and the same page saved through the image pass's `BilevelCodec::Jbig2Generic` (`crates/tinker-pdf/src/recode.rs:188-197`), the builder having no JBIG2 filter | the builder, then `write::save` | "G4 / JBIG2 scan page" |
| `paths(5000)`, `curves(2000)` | 3 000 filled polygons and closed curves at fractional coordinates, as `paths_document` draws them (`engine.rs:99-120`), and 2 000 stroked cubic curves, 0.5 to 4 pt wide, each of the nine cap-and-join pairs in turn, every fifth dashed; `curves` is those 2 000 alone | `raw` | "5 000 paths", "that page's 2 000 stroked curves alone" |
| `triangles()`, `axial()` | the seven's 300 triangles and full-page axial shading | `engine.rs:99-120`, `:63-81`, moved | "300 triangles", "full-page axial shading" |
| `tiling(fills)` | one tiling pattern (`add_tiling_pattern`, `build.rs:6276`) filling an A4 page in one rectangle, or in 50 strips that together cover what the one covers | the builder | "a page filled by one tiling pattern 50 times / once" |
| `annotated(base)` | `base` with one 100×100 pt square annotation on its first page, appearance included (`DocumentEditor::add_annotation`, `crates/tinker-pdf-cos/src/edit/annotations.rs:214`; `annot::square`, `edit/annot.rs:84`) | the editor | "a changed 100×100 pt annotation", "a 1 000-page file", "a 10-page one" |
| `book(chapters)` | an EPUB of one paragraph a chapter, Liberation Serif embedded through `@font-face` so its glyphs are drawn in both builds | `tests/epub_support`'s OCF writer (`crates/tinker-pdf/tests/epub_support/mod.rs:543`), by path | "a generated 1 000-chapter book", "100 chapters" |
| `html_report()`, `markdown()` | 20 pages each at the default page box: headings, paragraphs and tables | `FromHtml` (`crates/tinker-pdf/src/html.rs:181`); CommonMark for `Document::open_markdown` (`crates/tinker-pdf/src/lib.rs:1472`) | "a generated 20-page HTML report / 20-page Markdown document" |
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
| `open/open a 3-page document` | bytes · the file a call | `three_pages()` | — (0.0145 on `ubuntu-latest`); floor 1 | unmeasured | MS-04.2 |
| `open/open and count the 23 MB image file` | bytes · the file a call | `image_file()` | twin; floor 1 | unmeasured | MS-04.3 |
| `open/open the AES-256 file` | none | `aes_file()`, authenticated with the empty password (`lib.rs:1722`) and counted | the Item's; floor 1 | unmeasured | MS-04.3 |
| `open/every page's size, 3000 pages` | pages · 3 000 | `small(3000)`, `page(i)` then `Page::size` (`lib.rs:1773`, `:2198`) for every i | 2 569 → 3; 856× | crosses | MS-04.3 |
| `open/every page's size, 10000 pages` | pages · 10 000 | `small(10000)` | Θ(n²), about 28 500 from the 3 000-page figure → 10; relation ≤ 3.6 × 3 000 | crosses | MS-04.3 |
| `open/open and count the 23 MB image file, CLI` | bytes · 5 files | `tpdf info` | 36 → 6; 6× | ×5 | MS-04.8 |

**text**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `text/text of 100 dense pages` | pages · 100 | `dense(100, 0)`, every page's `text().plain_text()` by index, as `tpdf text` reads them (`tools/tpdf/src/main.rs:973-991`) | 751 → 100; 7.51× | above | MS-04.3 |
| `text/text of 100 standard-14 pages` | pages · 100 | `producer(100)` | the Item's; floor 1 | above, expected | MS-04.3 |
| `text/text of 1000 small pages` | pages · 1 000 | `small(1000)` | twin (CLI 350); floor 1 | crosses | MS-04.3 |
| `text/text of 10000 small pages` | pages · 10 000 | `small(10000)` | twin (CLI 36 000); floor 1 | above | MS-04.3 |
| `text/structured JSON of 100 pages` | pages · 100 | `dense(100, 0)` through `TextWriter`'s JSON, as `tpdf text --json` (`main.rs:962-972`) | twin (CLI 1 926); floor 1 | above | MS-04.3 |
| `text/search a phrase over 100 pages from scratch` | pages · 100 | `dense(100, 0)`, `TextPage::search` (`crates/tinker-pdf-content/src/text.rs:265`) | 722 → 110; 6.56× | above | MS-04.3 |
| `text/search 100 pages' prebuilt text` | pages · 500 | the 100 `TextPage`s, built once | 36 → 6; 6× | ×5 | MS-04.3 |
| `text/search 100 pages' prebuilt text, whole words, unaccented` | pages · 300 | the same, `search_with` whole-word and diacritic-insensitive (`search.rs:43-50`, `:108`) | 77 → 12; 6.41× | ×3 | MS-04.3 |
| `text/text of 100 dense pages, CLI` | pages · 100 | `tpdf text` | 678 → 130; 5.21× | above | MS-04.8 |
| `text/text of 1000 small pages, CLI` | pages · 1 000 | `tpdf text` | 350 → 15; 23.3× | crosses | MS-04.8 |
| `text/text of 10000 small pages, CLI` | pages · 10 000 | `tpdf text` | 36 000 → 150; relation ≤ 11 × 1 000 | above | MS-04.8 |
| `text/structured JSON of 100 pages, CLI` | pages · 100 | `tpdf text --json` | 1 926 → 200; 9.63× | above | MS-04.8 |

**analysis** — every relation's "text of the same pages" is the group's own
text of the document the operation reads, so all six stay inside the group.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `analysis/text of the 100 report pages` | pages · 100 | `report(100)` | the relations' denominator; floor 1 | above, expected | MS-04.4 |
| `analysis/redact a rectangle on each of 100 pages` | pages · 100 | `report(100)`, `redact::apply` on every page of one editor (`crates/tinker-pdf/src/redact.rs:1106`), unsaved: saving is the write group's | not measured; ≤ 1.5 × text | above, expected | MS-04.4 |
| `analysis/reading order inferred over 100 pages` | pages · 100 | `report(100)`, `Document::inferred_orders` (`crates/tinker-pdf/src/reading_order.rs:484`); the document is untagged, so the default options infer (`:173-176`) | not measured; ≤ 1.5 × text | above, expected | MS-04.4 |
| `analysis/tables inferred over 100 pages` | pages · 100 | `report(100)`, `Page::tables(TableSource::Inferred)` (`crates/tinker-pdf/src/tables.rs:448`) | not measured; ≤ 1.5 × text | above, expected | MS-04.4 |
| `analysis/text of the 100 claiming pages` | pages · 100 | `claiming(100)` | the validators' denominator; floor 1 | above, expected | MS-04.4 |
| `analysis/PDF-A validation of 100 pages` | pages · 100 | `claiming(100)`, `validate_pdfa` (`lib.rs:1990`) | not measured; ≤ 3 × text | above, expected | MS-04.4 |
| `analysis/PDF-UA validation of 100 pages` | pages · 100 | `validate_pdfua` (`:2020`) | not measured; ≤ 3 × text | above, expected | MS-04.4 |
| `analysis/PDF-X validation of 100 pages` | pages · 100 | `validate_pdfx` (`:2043`) | not measured; ≤ 3 × text | above, expected | MS-04.4 |

**render** — a page render is `Page::render` (`lib.rs:2247`) at
`RenderOptions::at_dpi` (`:724`); "the text page" is `dense(1, 0)`'s page.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `render/the text page at 72 dpi` | pixels · 5 × 595×842 | `dense(1, 0)` | 54 → 6; 9× | ×5 | MS-04.5 |
| `render/the text page at 150 dpi` | pixels · 2 × 1240×1755 | `dense(1, 0)` | 88 → 15; 5.86× | ×2 | MS-04.5 |
| `render/the text page at 300 dpi` | pixels · 2480×3509 | `dense(1, 0)` | 171 → 35; 4.88× | in | MS-04.5 |
| `render/record the text page's display list` | pages · 21 | `Page::display_list` (`crates/tinker-pdf/src/display.rs:129`) | 6.3 → 1.2; 5.25× | ×21 | MS-04.5 |
| `render/5000 paths at 150 dpi` | pixels · 1240×1755 | `paths(5000)` | 1 326 → 450; 2.94× | above, and at target | MS-04.5 |
| `render/5000 paths at 300 dpi` | pixels · 2480×3509 | `paths(5000)` | 3 434 → 1 100; 3.12× | above, and at target | MS-04.5 |
| `render/the 2000 stroked curves alone at 150 dpi` | pixels · 1240×1755 | `curves(2000)` | not measured apart; as the page; 2.94×, paired with the baseline's same curves | above, expected | MS-04.5 |
| `render/300 triangles at 150 dpi` | pixels · 2 × 1240×1755 | `triangles()` | 44 → 20; 2.2× | ×2 | MS-04.10 renames the seven's |
| `render/a full-page axial shading at 150 dpi` | pixels · 1240×1755 | `axial()` | 124–170 → 25; 4.96× | in | MS-04.10 renames the seven's |
| `render/a full-page axial shading at 300 dpi` | pixels · 2480×3509 | `axial()` | 674 → 90; 7.48× | above | MS-04.5 |
| `render/the mesh shadings fixture at 150 dpi` | pixels · 7 bitmaps | `fuzz/corpus/render_page/mesh-shadings.pdf` | 18.1 → 4; 4.52× | ×7 | MS-04.5 |
| `render/the mesh shadings fixture at 300 dpi` | pixels · 3 bitmaps | the same | 43.1 → 10; 4.31× | ×3 | MS-04.5 |
| `render/the transparency fixture at 150 dpi` | pixels · 9 bitmaps | `fuzz/corpus/render_page/transparency.pdf` | 8.4 → 3; 2.8× | ×9 | MS-04.5 |
| `render/a page tiled by one pattern fill at 150 dpi` | pixels · 1240×1755 a call | `tiling(1)` | not measured; relation: 50 fills ≤ 1.2 × one; counted | unmeasured | MS-04.5 |
| `render/a page tiled by 50 pattern fills at 150 dpi` | pixels · 1240×1755 a call | `tiling(50)` | the same relation | unmeasured | MS-04.5 |
| `render/the 300-dpi text page in 4 bands on 4 threads` | pixels · 3 × 2480×3509 | `dense(1, 0)`, four `RenderOptions::region` bands (`lib.rs:584`), one scoped thread each | relation ≥ 3 × the 1-thread speed (`the text page at 300 dpi`) | ×3 | MS-04.5 |
| `render/100 text pages at 150 dpi on 1 thread` | pages · 100 | `dense(100, 0)` | 11.4 → 60 pages/s (8 772 → 1 667); 5.26× | above, and at target | MS-04.5 |
| `render/100 text pages at 150 dpi on 4 threads` | pages · 100 | the same, on `std::thread::scope` over owned pages, as `crates/tinker-pdf/examples/parallel.rs:87` does | 43 → 200 pages/s (2 326 → 500); 4.65×; relation ≥ 3.33 × 1 thread | above, and at target | MS-04.5 |
| `render/the annotated text page at 150 dpi` | pixels · 2 × 1240×1755 | `annotated(dense(1, 0))` | the next one's full-page render; floor 1 | ×2 | MS-04.5 |
| `render/re-render a changed 100x100 pt annotation` | pixels · 3 × 1240×1755 | the same, its appearance replaced, saved and reopened untimed; today the whole page re-renders | relation ≤ 10 % of the full-page render | ×3 today; under 20 at its bound | MS-04.5 |
| `render/replay the Flate RGB image page at 150 dpi` | pixels · 1240×1755 | `flate_page()`'s display list, recorded once; its renders share the decoded image (`resources.rs:532-546`) | 350 → 40; 8.75× | above | MS-04.6 |
| `render/an ICC-based CMYK image to sRGB at 300 dpi` | pixels · 2480×3508, the image's | `cmyk_page()`, drawn at the image's own resolution so each sample is converted once | not measured → ≥ 100 Mpixel/s (≤ 87); calibrated | unmeasured | MS-04.6 |
| `render/the JPEG page at 72 dpi` | pixels · 595×842 | `jpeg_page()` | 652 → 35; 18.6× | above | MS-04.6 |
| `render/the JPEG page at 300 dpi` | pixels · 2480×3509 | `jpeg_page()` | 1 585 → 150; 10.5× | above | MS-04.6 |
| `render/a second render of the JPEG page at 300 dpi` | pixels · 2480×3509 | `jpeg_page()`, after the same `Page` rendered at 72 dpi, untimed | not measured; relation ≤ 30 % of the first (`the JPEG page at 300 dpi`) | above, expected | MS-04.6 |
| `render/the JPEG 2000 page at 72 dpi` | pixels · 595×842 | `jpx_page()` | 2 753 → 250; 11× | above | MS-04.6 |
| `render/the G4 scan page at 150 dpi` | pixels · 1240×1755 | `g4_page()` | 641 → 50; 12.8× | above | MS-04.6 |
| `render/the JBIG2 scan page at 150 dpi` | pixels · 1240×1755 | `jbig2_page()` | 875 → 80; 10.9× | above | MS-04.6 |
| `render/the Flate RGB image page at 72 dpi` | pixels · 595×842 | `flate_page()` | 924 → 230; 4.01× | above | MS-04.6 |
| `render/the text page to PNG at 150 dpi, CLI` | pixels · 1240×1755 | `tpdf render` | 402 → 70; 5.74× | above | MS-04.8 |
| `render/the text page to PNG at 300 dpi, CLI` | pixels · 2480×3509 | `tpdf render` | 1 180 → 200; 5.9× | above | MS-04.8 |
| `render/20 pages to PNG at 150 dpi, 1 job, CLI` | pages · 20 | `tpdf render --jobs 1` | 6 847 → 1 300; 5.26× | above, and at target | MS-04.8 |
| `render/20 pages to PNG at 150 dpi, 4 jobs, CLI` | pages · 20 | `tpdf render --jobs 4` | 1 743 → 400; 4.35× | above, and at target | MS-04.8 |

**encode**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `encode/PNG of the 1240x1755 text page` | pixels · 1240×1755 | the text page's 150-dpi bitmap, rendered once, `Bitmap::to_png` (`lib.rs:832`) | 269 → 40; 6.72× | in | MS-04.6 |
| `encode/PNG of the near-blank page` | pixels · 2 × 1240×1755 | a 150-dpi bitmap of a page holding one line | 145 → 15; 9.66× | ×2, at 290 today | MS-04.6 |
| `encode/deflate page-like data` | bytes · 6 528 600 | `page_like()`, `deflate` (`deflate.rs:200`) | 40 → 120 MB/s (163 → 54); 3×; size ≤ 1.02 × level 6's, exact | in | MS-04.6 |
| `encode/deflate photo-like data` | bytes · 6 528 600 | `photo_like()` | 28 → 40 MB/s (233 → 163); 1.4×; size likewise | in | MS-04.6 |

**decode** — each through its `tinker-pdf-filters` entry point
(`crates/tinker-pdf-filters/src/lib.rs:70-93`, `:721`, `:738`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `decode/inflate page-like data` | bytes out · 2 × 6 528 600 | `deflate(page_like())` | 279 → 500 MB/s (23.4 → 13.1 a call); 1.79× | ×2 | MS-04.6 |
| `decode/inflate near-incompressible data` | bytes out · 6 528 600 | `deflate(noise())` | 141 → 150 MB/s (46.3 → 43.5); 1.06× | in | MS-04.6 |
| `decode/LZW decode page-like data` | bytes out · 6 528 600 | `lzw(page_like())` | not measured → ≥ 200 MB/s (≤ 32.6); calibrated | unmeasured | MS-04.6 |
| `decode/baseline JPEG 2480x3508` | pixels · 2480×3508 | `baseline_jpeg()` | 446 → 60; 7.43× | above | MS-04.6 |
| `decode/progressive JPEG 2480x3508` | pixels · 2480×3508 | `data/progressive-2480x3508.jpg` | 458 → 120; 3.81× | above | MS-04.6 |
| `decode/irreversible JPEG 2000 2480x3508` | pixels · 2480×3508 | `data/irreversible-2480x3508.jp2` | 2 566 → 1 000; 2.56× | above, and at target | MS-04.6 |
| `decode/reversible JPEG 2000 2480x3508` | pixels · 2480×3508 | `data/reversible-2480x3508.jp2` | 6 516 → 2 700; 2.41× | above, and at target | MS-04.6 |
| `decode/decode the JBIG2 generic page` | pixels · `scan()`'s | `jbig2_generic_encode(scan())` (`jbig2/encode.rs:251`) | 255 → 60; 4.25× | in | MS-04.6 |
| `decode/decode the CCITT G4 page` | pixels · `scan()`'s | `ccitt_g4_encode(scan())` | 46 → 25; 1.84× | in | MS-04.6 |

**write** — a rewrite is the editor's save under `WriteMode::Rewrite`, as
the seven's is (`engine.rs:168-175`); merge and subset-on-save go through
`write::save` (`crates/tinker-pdf/src/write.rs:337`) as `tpdf merge` does
(`tools/tpdf/src/writing.rs:132-165`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `write/rewrite 100 pages` | pages · 100 a call | `small(100)` | twin; floor 1 | unmeasured | MS-04.4 |
| `write/rewrite 1000 pages` | pages · 1 000 | `small(1000)` | twin (CLI 668); floor 1 | above | MS-04.4 |
| `write/rewrite 3000 pages` | pages · 3 000 | `small(3000)` | 5 480 → 40; 137× | above | MS-04.4 |
| `write/rewrite 100 pages into object streams` | pages · 10 × 100 | `small(100)`, `object_streams` (`crates/tinker-pdf-cos/src/write.rs:120`) | 2.6; held | ×10 | MS-04.4 |
| `write/rewrite 100 pages linearized` | pages · 7 × 100 | `small(100)`, `linearize` (`:112`) | 3.7; held | ×7 | MS-04.4 |
| `write/subset-on-save of 100 pages` | pages · 100 | `dense(100, 0)`, `SaveOptions::default()`: a rewrite, fonts subset (`crates/tinker-pdf/src/write.rs:144-157`) | twin (in-process 1 305); floor 1 | above | MS-04.4 |
| `write/merge 4 x 100 pages` | pages · 400 | `dense(100, 0..3)`: pages imported with `import_page`, saved with stream deduplication and `FontPolicy::Keep` | 2 855 → 25; 114× | above | MS-04.4 |
| `write/AES-256-CBC decrypt 23 MB` | bytes · the file | `image_file()`'s bytes, encrypted once, `cbc_decrypt_with_iv_prefix` (`crates/tinker-pdf-crypto/src/aes.rs:238`) | 32 → 150 MB/s (about 719 → 153); 4.68× | above | MS-04.4 |
| `write/AES-256-CBC encrypt 23 MB` | bytes · the file | the same plain bytes, `cbc_encrypt_with_iv_prefix` (`:289`) | 57 → 80 MB/s (about 404 → 288); 1.4× | above | MS-04.4 |
| `write/edit an annotation, save, reopen, 1000 pages` | pages · 13 × 1 000 | `annotated(small(1000))`: the appearance replaced (`DocumentEditor::put_stream`, `edit/mod.rs:395`), saved under `WriteMode::Incremental`, the bytes reopened | not measured → ≤ 2 plus output bytes; relation ≤ 2 × the 10-page one | unmeasured, ×13 | MS-04.4 |
| `write/edit an annotation, save, reopen, 10 pages` | pages · 10 a call | `annotated(small(10))` | the relation's other side | unmeasured | MS-04.4 |
| `write/rewrite 100 pages, CLI` | pages · 4 × 100 | `tpdf merge`, one input | 14.8 → 8; 1.85× | ×4 | MS-04.8 |
| `write/rewrite 1000 pages, CLI` | pages · 1 000 | `tpdf merge`, one input | 668 → 60; 11.1× | above | MS-04.8 |
| `write/subset-on-save of 100 pages, CLI` | pages · 100 | `tpdf merge --font-policy subset` | 1 208 → 120; 10× | above | MS-04.8 |
| `write/merge 4 x 100 pages, CLI` | pages · 400 | `tpdf merge`, four inputs | 1 008 → 40; 25.2× | above | MS-04.8 |
| `write/encrypt the 23 MB image file, CLI` | bytes · the file | `tpdf encrypt` | 770 → 150; 5.13× | above | MS-04.8 |

The merge keeps fonts because its target leaves no room for a subset pass —
40 ms for 400 pages where subset-on-save's is 120 ms for 100 — and
subset-on-save measures that pass on its own. The AES pair is tier P's "decrypt
/ encrypt", whose table states no size; 23 MB is the Item's, so a smaller input
there would re-state no tier P figure, only the row's text, which is the
owner's under H-163 all the same.

**sign**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `sign/RSA-2048 private-key operation` | none · 13 calls | Example 10's modulus and private exponent, `Modulus::pow` | 6.0 → 2; 3×, paired with the baseline's `pow` | ×13 | MS-04.7 |
| `sign/verify an RSA-2048 signature over 10 MB` | bytes · 10 000 000 | `message()`, digest and public operation (`RsaPublicKey`, `crates/tinker-pdf-crypto/src/rsa.rs:256`) | about 38 → 30; 1.26× | in | MS-04.7 |
| `sign/SHA-256 of 10 MB` | bytes · 10 000 000 | `message()`, `sha256` (`crates/tinker-pdf-crypto/src/sha2.rs:473`) | 262 → 350 MB/s (38.2 → 28.6); 1.33× | in | MS-04.7 |

**layout** — a book opens at the default box, 432 × 648 pt (`OpenOptions`,
`crates/tinker-pdf/src/lib.rs:1128-1154`), which is the seven's box too; a
container's synthesised PDF is `doc.cos().bytes()`
(`crates/tinker-pdf-cos/src/doc.rs:867`), opened again from those bytes.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `layout/open and paginate <book> at 432x648 pt`, one for each of the nine | pages · 4 × the book's | each committed book | 17–22 → 8; 2.12× | ×4 | MS-04.7 |
| `layout/render every page of <book> at 72 dpi`, nine | pages · the book's | the same | twin (CLI 84–130); floor 1 | in | MS-04.7 |
| `layout/open and paginate the 1000-chapter book` | pages · its count | `book(1000)` | not measured → ≥ 1 000 pages/s; calibrated; relation ≤ 11 × 100 chapters | above, and at target | MS-04.7 |
| `layout/render the 1000-chapter book at 72 dpi` | pages · its count | `book(1000)` | not measured → ≥ 300 pages/s; calibrated; the same relation | above, and at target | MS-04.7 |
| `layout/open and paginate the 100-chapter book` | pages · its count a call | `book(100)` | the relation's other side | unmeasured | MS-04.7 |
| `layout/render the 100-chapter book at 72 dpi` | pages · its count | `book(100)` | the relation's other side; 333 at 300 pages/s | above, and at target | MS-04.7 |
| `layout/the 20-page HTML report to PDF` | pages · 20 | `html_report()`, laid out and finished | not measured → ≤ 60; calibrated | unmeasured | MS-04.7 |
| `layout/the 20-page Markdown document to PDF` | pages · 20 | `markdown()` | not measured → ≤ 50; calibrated | unmeasured | MS-04.7 |
| `layout/open each SVG fixture, render page 1` | pages · 17 a call | every committed `.svg`, its first page at 72 dpi | not measured; relation ≤ 1.5 × its synthesised PDF | unmeasured | MS-04.7 |
| `layout/render each SVG fixture's synthesised PDF` | pages · 17 a call | each one's synthesised PDF, opened from bytes | the relation's other side | unmeasured | MS-04.7 |
| `layout/open each XPS fixture, render page 1` | pages · 19 a call | every committed `.xps` and `.oxps` | the same relation | unmeasured | MS-04.7 |
| `layout/render each XPS fixture's synthesised PDF` | pages · 19 a call | the same | its other side | unmeasured | MS-04.7 |
| `layout/open each CBZ fixture, render page 1` | pages · 9 a call | every committed `.cbz` | the same relation | unmeasured | MS-04.7 |
| `layout/render each CBZ fixture's synthesised PDF` | pages · 9 a call | the same | its other side | unmeasured | MS-04.7 |
| `layout/render every page of <book> at 72 dpi, CLI`, nine | pages · the book's | `tpdf render` | 84–130 → 40; 2.1× | in | MS-04.8 |

The nine books are `calibre-book-cover`, `calibre-book-nocover`,
`calibre-embedded-font`, `kcc-fixed-layout`, `pandoc-book-cover`,
`pandoc-book-epub2`, `pandoc-book-nocover`, `pandoc-embedded-font` and
`pandoc-plates`, each `.epub`, and the id carries the file name: the longest,
`render every page of calibre-embedded-font.epub at 72 dpi, CLI`, is 62
bytes. The containers are timed a format at a time, every fixture of the
format in one iteration, so the relation holds for each format's set: one
operation a fixture would be 90 operations and about seven minutes of the
layout job. A fixture the facade refuses is named by the generator and left
out, never timed as a failure.

**fonts** — `Sfnt::parse` and `glyf::outline`
(`crates/tinker-pdf-font/src/sfnt.rs:46`, `glyf.rs:67`), `Cff::parse` and
`Cff::outline` (`cff.rs:218`, `:414`), and a fresh `Shaper` a call as every
caller makes one (`crates/tinker-pdf-shape/src/shape.rs:270`, `:426`;
`crates/tinker-pdf/src/shaping.rs:198`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `fonts/outlines of Liberation Serif Regular` | outlines · 12 × 2 602 | the face, parsed and every glyph outlined | 2.1; held | ×12 | MS-04.7 |
| `fonts/outlines of Liberation Sans Regular` | outlines · 9 × 2 620 | the same | 3.0; held | ×9 | MS-04.7 |
| `fonts/outlines of Liberation Mono Regular` | outlines · 14 × 2 423 | the same | 1.8; held | ×14 | MS-04.7 |
| `fonts/outlines of every committed CFF program` | outlines · 2 × every program's | the 193 programs | not measured → ≥ 1 M outlines a second; calibrated | unmeasured, ×2 | MS-04.7 |
| `fonts/shape 1000 characters of Latin` | none · 125 calls | Liberation Serif Regular | not measured → ≤ 0.2; calibrated | unmeasured, ×125 | MS-04.7 |
| `fonts/shape 1000 characters of Arabic` | none · 50 calls | `TestShapeAran.ttf` | not measured → ≤ 0.5; calibrated | unmeasured, ×50 | MS-04.7 |
| `fonts/shape 1000 characters of Devanagari` | none · 42 calls | the vendored face | not measured → ≤ 0.6; calibrated | unmeasured, ×42 | MS-04.7 |

A walk of the 193 programs' `CharStrings` indexes counts 18 529 glyphs in the
192 that parse as plain CFF; the 193rd, `fuzz/corpus/sfnt/opentype-cff.otf`,
is a fuzz seed MS-04.7 counts through the parser. At a million a second that
is under 19 ms, so `calls` 2.

**scripts** and **OCR** have no operation of MS-04's own; their rows create
their files (below). **calibration** holds one:

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `calibration/text of 100 dense pages` | pages · 100 | `dense(100, 0)`, the call `text/text of 100 dense pages` makes | 751 on the planning machine (H-112); the operation every calibrated target is a fraction of | above until P-02 | MS-04.3 |

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
not the runner: the three of the seven that are tier P's own operations ran
0.56 to 1.00 times tier P's figures on `ubuntu-latest`, by the fastest of
their fifteen runs (300 triangles 31.7 ms against 44, the axial shading
123.7 against 124–170, a book 12.4 against 17–22), so an operation near
300 ms may fall inside; MS-04.9's dispatch is the list H-163 is decided on.

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
its bytes and SHA-256, the producer and its version, the command, the date,
and the SHA-256 of the input it was made from; the file names say what each
is (`progressive-2480x3508.jpg`, `irreversible-2480x3508.jp2`,
`reversible-2480x3508.jp2`), and `.jpg` and `.jp2` are already binary
(`.gitattributes:13-14`). The largest committed JPEG 2000 file today is 2 741
bytes (`fuzz/corpus/jpx/rgb-rct`) and the largest progressive JPEG 178
(`fuzz/corpus/jpeg/progressive-refined.jpg`); these are megabytes each, the
lossless one most, and MS-04.6 states each one's size.

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
hosts this engine and adjudicates nothing (MS-04.8).

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
check reads. A machine entry gains `"unmeasured"`, the ids it has no figure
for, ahead of its `"operations"`, so a hole is named and never silent:
today's check refuses an entry short of any operation
(`xtask/src/bench.rs:569-577`). Today's reader cuts entries at `"machine":`
and `"id":` (`:243`, `:272`), so MS-04.2's cuts the file at `"machines"`
first and no list entry can fall into a machine's chunk — the trap
`BASELINE_PATH`'s comment describes (`:65-73`). MS-02.3 adds each operation's
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
flag, is the one setting each group makes itself. The seven's `shading` group,
with its 20 samples and 10 s (`engine.rs:154-160`), goes with them.

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
build and its generators; a job's build is read from MS-04.2's first matrix
run, and a group whose job nears its limit splits by a criterion filter into
two entries with a count each, never by a changed input. Every job builds
its group's bench in release, as `cargo bench` does, and the five with CLI
operations build `tpdf` in release first. MS-02's paired job, three builds
and ten rounds of this set in 15 minutes, is MS-02.1's to price and H-162's
to decide.

#### The window

MS-04.9's `bench-check --sizes` reads each operation's mean from the run's
estimates — an iteration's, so `calls` included — and fails one outside
20–300 ms on the head, unless its list names it outside by the owner's word,
which it reports instead. It runs in each job after the bench, because the
window is a property of the runner the job landed on and never of
`cargo test`. One dispatch on `develop` in report mode prints every operation
outside with its mean, which MS-04.9 commits below with its run id; H-163 is
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

MS-04.2 moves the seven into their groups' files with their figures re-keyed
under group ids and unchanged; they keep their bands while the set grows
beside them, and take their groups' sampling: ten samples for criterion's
hundred widen a mean's spread, which their bands, 35 to 135 %, were set
above, and a run that fails one on the change is a finding MS-04.2 reads,
never a band to widen. MS-04.10 measures the whole set, and the seven's figures
leave with the seven: three are tier P's own operations on the same input and
continue under tier P's names, one continues as itself, and three are
succeeded by the same call on tier P's larger input. None retires without a
successor, so H-78 is asked nothing for them.

| The seven, as MS-04.2 re-keys them | On `ubuntu-latest` | From MS-04.10 | Why |
| --- | ---: | --- | --- |
| `open/open a 3-page document` | 0.0145 ms | itself, with `calls` | the set's only in-process open of a small file; P-21's held operation is the CLI's |
| `render/render text at 150 dpi` | 5.341 ms | `render/the text page at 150 dpi` | its page draws no glyph (`engine.rs:85-93`); the successor's does |
| `text/extract a page of text` | 0.728 ms | `text/text of 100 standard-14 pages` | the same call over standard-14 Helvetica, 100 pages for one |
| `render/render a full-page axial shading at 150 dpi`, the seven's `shading/…` | 123.662 ms | `render/a full-page axial shading at 150 dpi` | the same generator (`engine.rs:63-81`) |
| `render/fill 300 anti-aliased paths at 150 dpi` | 31.739 ms | `render/300 triangles at 150 dpi` | the same 300 triangles (`engine.rs:99-120`) |
| `write/rewrite a document` | 0.052 ms | `write/rewrite 100 pages` | the same call (`engine.rs:168-175`), 100 pages for three |
| `layout/paginate a book at 432x648 pt` | 12.365 ms | `layout/open and paginate pandoc-book-cover.epub at 432x648 pt` | the same book, box and call (`engine.rs:122-128`, `:180-188`) |

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
speed rows — 115 steps of other rows wait on MS-04 or one of its steps
(`python3 -I tools/roadmap/brief.py MS-04 --all` lists them) — come in by the
same rule, each in the group its operation belongs to.

### Milestones

Each step is one workflow run, and its exit is quoted as the step states it
in `docs/roadmap/MS.md`.

| Step | Deliverable | Exit, as the step states it | Waits on |
| --- | --- | --- | --- |
| MS-04.2 | The per-group harness: the seven in their group files over `support`, a `[[bench]]` each, `baseline.json`'s lists with units and named-unmeasured entries, and `cargo xtask baseline` holding files, lists and `bench.yml`'s per-group counts to each other | `xtask/src/bench.rs` unit tests (new) `a_group_file_and_its_list_that_disagree_are_refused`, `a_group_guard_counting_short_is_refused`, `an_operation_neither_measured_nor_named_unmeasured_is_refused` and `the_seven_keep_their_figures_under_their_group_ids`; `the_committed_baseline_agrees_with_the_tree` passes; `cargo test -p tinker-pdf --benches` runs each of the seven once. | MS-04.1 |
| MS-04.3 | The open, text and calibration groups, in-process | `cargo test -p tinker-pdf --benches` runs each operation once, each generator asserting what its operation needs (the file at least 23 MB, the page counts, the face embedded and its text extracted, the AES-256 file authenticating, each search finding its hits); `the_committed_baseline_agrees_with_the_tree` holds the three groups. Default and `bundled-fonts` builds. | MS-04.2 |
| MS-04.4 | The write and analysis groups, in-process | `cargo test -p tinker-pdf --benches` runs each once, each generator asserting its shape (each saved file reopens with its page count, the decrypted bytes are the plain ones, the redaction removed the text under it, each validator returns a verdict); `the_committed_baseline_agrees_with_the_tree` holds both groups. Both builds. | MS-04.3 |
| MS-04.5 | The render group's vectors and text pages, threads and bands | `cargo test -p tinker-pdf --benches` runs each once; the text page's generator asserts its render reports no `UnreadableFont` and inks glyph pixels, and the four bands reassemble to the whole page byte for byte (ruling 5); `the_committed_baseline_agrees_with_the_tree` holds the group. Default and `bundled-fonts` builds. | MS-04.4 |
| MS-04.6 | The encode and decode groups and the image pages, with `data/` and the LZW encoder | `cargo test -p tinker-pdf --benches` runs each once, each decode asserting its samples' size and, where the tree writes the format (baseline JPEG, G4, JBIG2, PNG, deflate), a round trip, and each page asserting its image drew; `the_committed_baseline_agrees_with_the_tree` holds the groups. | MS-04.5 |
| MS-04.7 | The sign, layout and fonts groups, with the Devanagari face | `cargo test -p tinker-pdf --benches` runs each once and each asserts its input (the signature verifies, every book under `crates/tinker-pdf/tests/epub/` paginates and renders every page, the generated books have their chapters, each face yields its outlines, each shaped paragraph has its glyphs and no `.notdef`); `the_committed_baseline_agrees_with_the_tree` holds the three groups and `cargo xtask vendor` the Devanagari face's declaration. Default and `bundled-fonts` builds. | MS-04.6 |
| MS-04.8 | The CLI-marked operations through `support/cli.rs`, with its `SPAWNERS` row | `cargo test -p tinker-pdf --benches`, with `tpdf` built, runs each once and each asserts what its command wrote (the PNGs' count and size, the text's pages, each saved file reopening with its page count, the encrypted file authenticating); `cargo xtask oracles` passes with the helper's `SPAWNERS` row and fails without it; `the_committed_baseline_agrees_with_the_tree` holds the operations in their groups. Default and `bundled-fonts` builds of `tpdf`. | MS-04.7 |
| MS-04.9 | Throughput from the committed unit, `--sizes`, and the report-mode dispatch | `xtask/src/bench.rs` unit tests (new) `throughput_is_read_from_the_committed_unit`, `an_operation_under_twenty_ms_fails_the_size_check`, `an_operation_over_three_hundred_ms_fails_it` `a_baseline_side_may_run_longer` and `an_operation_named_outside_the_window_is_reported_not_failed`; USAGE names `--sizes`; the report-mode run's list is committed in MS-04's section of `docs/design/performance.md` with its run id. | MS-04.8, H-109 |
| MS-04.10 | The set measured by the 15-run rule; the seven's figures retired; the level-6 counts committed | `cargo xtask baseline` holds the bench files, the per-group guard counts and `baseline.json` to one list per group with no operation unmeasured; `every_committed_band_fires_at_its_own_edge_and_not_before` pushes on every band; all fifteen runs pass the entry they set and the sizes check; the two level-6 byte counts are committed where MS-04.1 puts them, dated. | MS-04.9, H-109, H-112, H-163 |

**What each step needs beyond its text**, from the tree as this design reads
it. Each step's own run corrects its text against this when it is taken.

- **MS-04.2** creates `support/inputs.rs` beside `support/mod.rs` and moves
  the seven's generators into it; makes `bench.yml` the matrix, its guard
  reading each entry's count and each job uploading `bench-<group>`, with
  `guard_count` (`xtask/src/bench.rs:478-482`) reading the matrix; changes
  `BENCH_SOURCE` (`:78`) into the group files and the `baseline` task's usage
  (`xtask/src/main.rs:27-29`); and refuses a name criterion would rewrite,
  under a test of its own,
  `an_operation_name_criterion_would_rewrite_is_refused`, since today a long
  name would be reported new rather than refused. The seven's ids are
  `open/open a 3-page document`,
  `render/render text at 150 dpi`,
  `text/extract a page of text`,
  `render/render a full-page axial shading at 150 dpi` (the `shading/` prefix
  gone),
  `render/fill 300 anti-aliased paths at 150 dpi`,
  `write/rewrite a document` and
  `layout/paginate a book at 432x648 pt`.
- **MS-04.3** opens the in-process documents from an `Arc<[u8]>`, reads every
  page's size by index as `tpdf` would, and freezes `calibration.rs`.
- **MS-04.4** writes `report` and `claiming`, and splits `claiming` into one
  document a claim, each with its text, if the builder will not write three
  claims into one; its merge keeps fonts, as above.
- **MS-04.5** moves "a second render at another dpi" to MS-04.6, which adds
  the JPEG page it renders: on a text page a second render has little to
  reuse, since P-06's glyph masks are keyed by their scale (the roster's
  A4), so P-16's 30 % could never be met there.
- **MS-04.6** adds `benches/data/` with its README, the LZW encoder, the
  pinned `page_like` and `photo_like` (the level-6 constants left for
  MS-04.10), and the second render. Its note that a committed fixture carries
  "never its producer's name" is read as its file name: ruling 13 records a
  third party's bytes with the tool, its version and the command, as
  `crates/tinker-pdf-filters/tests/images/README.md` and
  `crates/tinker-pdf-font/tests/woff/PROVENANCE.tsv` do, and the rulings
  override. `.jpg` and `.jp2` need no new `.gitattributes` line.
- **MS-04.7** vendors the face into `crates/tinker-pdf-shape/data/devanagari/`,
  which `.gitattributes` already covers, and embeds Liberation Serif in the
  generated books so their glyphs are drawn in the default build.
- **MS-04.8** puts the helper in `support/cli.rs`, the file its `SPAWNERS`
  row names, and builds `tpdf` in release in the five jobs that run it.
- **MS-04.9** reads `calls` and `per_iteration` as well as the unit, prints a
  call's figure beside an iteration's, accepts outlines as a unit beside
  pages, Mpixel and MB (P-26's target is in outlines a second), refuses a run
  whose criterion `Throughput` disagrees with the list, and runs `--sizes` in
  every job.
- **MS-04.10** may change an operation's `calls` on its measured figure —
  never its input — and commits the two level-6 constants in
  `benches/support/inputs.rs`.

The steps' `cargo test -p tinker-pdf --benches` runs each operation once,
unoptimised, at tier P's inputs: the three that read 10 000 pages and the two
JPEG 2000 decodes take minutes each in that profile, and the command stays as
the exits state it, its wall time reported with each step.

### The window, as the runner measured it

Owed. MS-04.9 commits here the list its report-mode dispatch on `develop`
printed — each operation outside 20–300 ms with its mean — and the run's id.

### Risks

| Risk | Mitigation |
| --- | --- |
| A name criterion rewrites is recorded under another key, reported new and never held to its band | Every name ASCII, at most 64 bytes, free of criterion's ten replaced characters; `cargo xtask baseline` refuses one that is not (MS-04.2) |
| A cache a later row adds turns a cold operation warm | An iteration opens its document from bytes; only an operation whose name says so starts from prebuilt state |
| Two engines generating one input write different bytes, so a pair times two inputs | The fixture directory `TINKER_BENCH_FIXTURES` names lets a paired job hand both builds the head's bytes; whether it does is MS-02.1's |
| An input an engine encoder writes moves when the encoder changes (P-04.7's deflate, the JPEG encoder) | The row that changes the encoder names the operations it moved in its commit; the two inputs the level-6 counts describe are formulas pinned by length and digest, so they never move |
| The run does not fit 30 minutes | One job a group, each measuring under half its limit by tier P's figures; a group nearing its limit splits by a filter, never by its input |
| Tier P's figures are the planning machine's, so the budget and the window's list are predictions | MS-04.9's dispatch measures the list H-163 is decided on, and an operation outside that it missed goes to H-163 before MS-04.10 commits any figure |
| A repetition count and its committed `per_iteration` drift apart | Criterion's recorded `Throughput` is checked against the list on every run |
| A CLI operation times a failed child, or a stale `tpdf` | The exit status is checked on every call and a failure panics the bench; `tpdf` is resolved beside the bench binary and built from the same checkout in the same job |
| The committed codestreams grow the repository and come from a program this project does not own | Three files with their bytes, digests and commands recorded; their decodes are checked first-party, the reversible one against the generator's input and the progressive one against the baseline decode |
| A format's aggregate hides one container fixture's regression | Every fixture is opened and rendered in the operation and asserted in test mode; a figure for each fixture would take ninety operations, which a row that needs one adds |
| The calibration operation moves | Its file is frozen; a change re-states every calibrated target, which is the owner's (H-77) |
| A row's speed-up takes an operation under 20 ms | The row raises its `calls` in the commit that raises its floor, with the figure re-measured under H-109, or names it for H-163; the four that cross the window are listed above |
| `ubuntu-latest` is a fleet, and one dispatch's jobs land on different hosts | Each band is per operation over fifteen dispatches, as today's are (`docs/verification.md:1906-1915`); every job logs its CPU model and count as `bench.yml:70` does |

## MS-07 — Corpus throughput and latency ratchet

Owed. MS-07.1 writes it: what the corpus child times, the per-corpus figures
and their band rule over five runs, the `ratchet.json` keys, and the
ten-slowest listing by `/Producer`.

## P-01 — Page index

Owed. P-01.1 writes it: the lookup order, the descent and what it checks, the
flat index and its life beside the object store, the editor's indexed lookups,
and A1's and A2's counted quantities in this tally.

## P-02 — Resolved-resource cache

Owed. P-02.1 writes it: what one scope resolves once and under which key, what
a hit reports again, A3's counted quantity, and each relation's path.

## P-03 — A decoded-stream cache per operation, and deduplication on encoded bytes

Owed. P-03.1 writes it: the operation-scoped decoded-stream cache, the
deduplication bucket key and its confirmation, both outcomes of H-137, A5's and
A6's counted quantities and MS-05's merge band. P-03.5 adds the measurements
H-137 is priced on.

## P-04 — PNG and deflate

Owed. P-04.1 writes it: the encoder's two levels, the match finder, the
streaming interface, the PNG writer's one pass, and A8's scratch bound.

## P-06 — Glyph coverage cache

Owed. P-06.1 writes it: the glyph-mask key, which glyphs take the cache, the
blit, the byte bound and its eviction order, and A4's counts on the 5 600-glyph
page.

## P-16 — Caches with byte budgets and a caller memory budget

Owed. P-16.1 writes it: one document-scoped cache of typed entries, each
charged its bytes; `CacheLimits` and where a caller states it; the eviction
order; reduced-resolution decode in place of abort; and how MS-05 reads
resident memory against a stated budget.

## P-17 — Incremental reparse and dirty-region re-render

Owed. P-17.1 writes it: the update section written without the original's
bytes, a reopen that keeps the base's work, the facade's edit, save and reopen
call, and the changed rectangle an annotation edit gives.

## P-24 — Search over prebuilt text

Owed. P-24.2 writes it: where a `TextPage` keeps its folded forms and their
offset tables, when a form is built, the boundary cache and the matcher.

## P-25 — Transparency groups and soft masks

Owed. P-25.1 writes it: a group's backdrop and a knockout group's initial state
without the copies kept beside its buffer, a soft mask's outside value, what
the guard counts as a group-buffer pixel, and the opaque fast path's reach into
group buffers.
