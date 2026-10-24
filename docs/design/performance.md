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

Owed. MS-04.1 writes it: the thirteen groups with every operation of tier P's
table, its criterion id, throughput unit, input and fixture source; the CLI
operations; `baseline.json`'s lists; criterion's sampling per group; the 20–300
ms window; and the operations each capability row adds. MS-04.9 adds the
window's report.

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
