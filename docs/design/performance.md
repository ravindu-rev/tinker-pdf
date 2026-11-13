# Performance

The design holds an operation's speed two ways. A change that makes it do more
work than its input calls for fails `cargo test` on linux and windows in CI's
tests and on `wasm32-wasip1` locally, through counts read at the facade, which
is where ruling 11 says a guard has to stand; one that makes it slower than
the planning baseline fails a paired run beside it.

This document is the design the speed rows share. Its first sections are
common to all of them — how a target gates, and which instrument holds each
kind of gate — and they are also MS-01's own design: the `Work` tally, the
doors that hand it out, the guard suite that reads it and the three targets
that run it. After them, each row with an instrument of its own has a section:
MS-02's paired run, MS-04's benchmark set and MS-07's corpus time ratchet.

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
  rather than by the tally.
- **The three targets** that run the suite: linux and windows in CI's tests,
  and `wasm32-wasip1` locally.
- **A section for each row with an instrument of its own**: MS-02, MS-04
  and MS-07.

## Non-goals

- **A clock in a test.** A count is a property of the binary and its input,
  which is why it can gate on a shared runner and under wasmtime. Time is
  gated by MS-02's paired run and checked absolutely by MS-03's pinned machine,
  never by `cargo test` (`docs/verification.md`, "Clocks: outside the suite,
  and not nowhere").
- **A tally that refuses work.** Refusing is what the budgets do —
  `ScriptBudget` (`crates/tinker-pdf-cos/src/form.rs:120`), the JBIG2 decoder's
  own `Work` (`crates/tinker-pdf-filters/src/jbig2.rs:1739`, private to that
  decoder and nothing to do with this one) and the caps in cos's `limits`. The
  tally only adds, and nothing but a door reads it.
- **`unsafe`.** None in the tally, the doors or the guards. The one
  `unsafe impl` in this design is the counting allocator in
  `crates/tinker-pdf/tests/guard_support/mod.rs`, which
  `guard_allocations.rs` installs, forwarding every call to `System` as
  `property_list_work.rs:27-31` and `svg_dash_memory.rs` already do; no
  engine crate is touched by it.
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
at `61adcde`, and in its **Gate** column as a form a run on any machine checks
and fails on (`docs/roadmap/P.md:22-61`). The form is what gates. The figure is what
MS-03's pinned machine checks once it exists (H-110), a second and tighter check
that never decides by itself whether a target is met. Every "Today" figure is
re-derived by MS-04's benchmark set before it is quoted as current, and a
target gates as tier P states it until H-77 re-states it, never because of the
planning-time comparison that suggested the number, which is not re-run
(H-112).

| Gate kind | What it compares | Instrument | Where its numbers live |
| --- | --- | --- | --- |
| **counted** | a count of work against a bound, or one count at two sizes | MS-01's guard suite, in `cargo test` on the three targets | the bound in the guard; the guard's entry in the roster in `guards.rs`; the pinned counts in K-07's determinism roster |
| **factor** | the head's time over `61adcde`'s, paired and interleaved on one machine, against 1 ÷ the operation's committed floor | MS-02's paired run, over MS-04's benchmark set | the floors, placed by MS-02.1; a P row raises its operations' floors in the commit that closes it, and only H-78 lowers one |
| **calibrated** | an operation the baseline cannot run at all, as a fraction of a calibration operation the same run times — `61adcde`'s text of 5 dense pages, 37.6 ms on the planning machine (rescaled from 100 pages' 751) | MS-02's paired run | beside the floors |
| **relation** | two operations of one build, in one run, with no baseline | MS-02's paired run, on the head | beside the floors |
| **held** | a factor of 1: the operation may not lose time against the baseline | MS-02's paired run | every floor starts at 1 |
| **peak band** (MS-05) | an operation's peak resident set against a committed band | MS-05's Linux-only test, one first-party probe per case | MS-05's bands |

Two consequences shape everything below.

**A count and a time answer different questions, and a row owes both where
its Gate column names both.** A factor says an operation got faster on one
machine. A guard says it does no more work than its input calls for on every
machine, which is the property a regression breaks first and a wall clock on a
shared runner sees last. That is why each P row lands with its counted guard.

**A count gates where a time cannot.** A hosted runner's benchmarks swing
widely between runs of one revision (`docs/verification.md`, "Clocks: outside
the suite, and not nowhere"), and under wasmtime there is no benchmark run at
all; a count of the same work is the same number on every target.

### What a count is

A count is a number of steps of one named kind that one operation took: a
`/Pages` node read, a filter chain run, a font dictionary resolved, a cache
asked. A guard is a relation over counts — a bound (`page(i)` reads at most
depth × fan-out nodes) or one count at two sizes (the work of 4 000 pages at
most 4.4 times the work of 1 000). Neither depends on the machine, the build
profile or the target, and the guard suite shows that by pinning counts on
every target it runs on.

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
`stats_do_not_change_a_pixel` (`tests/stats.rs`) and the other doors' checks
beside it hold it.

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
| `outlines_filled` | a glyph outline the renderer built into a path and painted: on the path route, and for a glyph mask the render's cache did not hold | the renderer | built |
| `glyph_masks_rasterised` | a filled glyph's mask rasterised: a key the render's cache did not hold — or the document's, under a stated budget — or a font with no identity (P-06, P-16.7) | the renderer | built |
| `glyph_mask_hits` | a filled glyph drawn from a mask the render's cache held, or the document's under a stated budget (P-06, P-16.7) | the renderer | built |
| `font_dictionaries_resolved` | a font dictionary a scope reads: once for each name its content selects a font by — the `/Font` entry, or a reserved name's object, when it is a dictionary — at the first question about the name or about the id another scope over the same dictionary handed out (P-02, "The font table") | the facade's scopes (`PageResources::scope_font`) | built |
| `images_decoded` | an image decoder run: an image XObject's cache miss, and every inline image, which is never cached (`resources.rs:222-224`); in an image extraction, each entry it lists and every inline image | the facade's scopes | built |
| a `_hits` and a `_misses` for each of the five caches `Caches` holds — font programs, images, ICC transforms, glyph outlines, form scopes (`resources.rs:63-101`) | a lookup that found an entry, a cached `None` included, or one that did not | the facade's scopes | built |
| `page_walks` | a walk of the page tree from its root | `collect_upto` itself, for every caller | built |
| `page_nodes_visited` | a `/Pages` tree node a lookup or a walk read, leaves included, and the page object the linearized or hinted path reads with each ancestor the head holds (`page_alone` and `ancestors_in_the_head` in `pages.rs`) | cos's page walk | built |
| `keys_looked_up` | a lookup by its bytes of a name the name table pre-interns (`Name::is_preinterned`): a key read where its constant answers, or a resource name that spells one (P-02) | the facade's scopes (`PageResources::name`) and the counting view (`Counting::intern`) | built |
| `color_spaces_parsed` | a resource name's colour space a scope reads out of `/ColorSpace` and parses, both its readings at once, at the first question about the name (P-02, "Colour spaces, functions and graphics states") | the facade's scopes (`PageResources::kept_space`) | built |
| `functions_read` | a function a scope reads from an object of its own — a shading's, a tint transform, a stitching function's piece, a graphics state's — once an object, and a stitching function once for each depth it is read at | the facade's scopes (`PageResources::kept_function`) | built |
| `graphics_states_read` | a `gs` name's state a scope reads out of `/ExtGState`, its entry a dictionary, at the first `gs` of the name | the facade's scopes (`PageResources::read_ext_g_state`) | built |
| `pages_interpreted` | a page's content interpreted for a reading-order inference, for its own order or for a neighbour's margin bands (P-02, "Batch inference") | the facade's reading-order inference (`reading_order.rs`'s `Batch::observe`) | built |
| `streams_decoded` | a run of a stream's filter chain, whatever it returned | cos's two filter-chain entry points and every `Resolve` view | built |
| `most_decodes_of_one_stream` | the most runs any one stream had in the operation — a maximum, the quantity A5 holds to one | the same | built |
| `most_decoded_stream` | the reference of the stream that had those runs, the lowest of a tie — what names a stream a page keeps decoding | the same | built |
| `bytes_decoded` | a byte a filter chain returned — a quantity | the same | built |
| `png_image_scratch` | a byte of a buffer a PNG write sized from the image rather than from a row or a constant, the most held at once — a maximum, the quantity A8 holds to none | `tinker-pdf-filters`' PNG writer, handed back as a value (`PngHeld`), and the facade's PNG door for a copy it converts | built |
| `png_peak` | a byte a PNG write held, its file included, the most at once — a maximum, A8's peak | the same | built |
| `tokens_lexed` | a token cos's lexer produced, for an object, a section, a trailer, an object stream's prologue or a repair scan's parse — a window read again counts again (`docs/design/resource-exhaustion.md`, "The counters") | cos's parse path, into the `WarningSink` each parse is handed, and out through `CosDocument::open_counted` and `get_counted` | built |
| `xref_entries_read` | a row of a classic table's subsection or of a cross-reference stream, on every section of the `/Prev` chain | the same | built |
| `objects_parsed` | an indirect object parsed, from the file, a window, the repair scan or an object stream | the same | built |
| `bytes_scanned` | a byte the repair scan's keyword searches looked through, a step rather than a quantity | the same, from the searches' own count, which the repair pass's unit test reads too | built |
| `content_tokens_read` | a token the content interpreter took from its tokenizer, nested content included | `tinker-pdf-content`'s interpreter, in its own state, added once a run by `interpret_counted` | built |
| `operators_run` | an operator the content interpreter ran, nested content included | the same | built |
| `shading_functions_evaluated` | a shading's function evaluated for a colour: once a table entry where a sweep reads it through a colour table, once a colour asked of it otherwise | the renderer | built |
| `image_colours_converted` | a colour an image's converter asked of its space's conversion (`ColorSpace::to_rgb`): a table entry filled, or a pixel no copy, table, row path or remembered colour answered (P-22.6, `docs/design/icc.md`'s "The per-image converter") | the facade's sample loops: a stream's, an inline image's, a JPEG's strips and a JPEG 2000 image's samples | built |
| `stream_seal_passes` | a pass of the AES core that sealed a save's streams side by side under the key the save holds for the whole file — a rewrite's R6 key, an update's inherited `/AESV3` one — whatever its width and however many lanes it filled (`docs/design/crypto-throughput.md`) | cos's writer, from the passes `tinker-pdf-crypto`'s side-by-side seal hands back as a value (`SideBySide`) | built |
| `jpeg_held` | a byte a JPEG decode held beside the image it decoded into — coefficients, planes and the strip it handed on — the most at once, a maximum | `tinker-pdf-filters`' strip decode, handed back as a value (`JpegHeld`), and the facade's image path, which writes each strip into the image | built |
| `text_forms_built` | a page's text folded and joined in one of a search's four folds, its offset table with it (P-24) | `tinker-pdf-content`'s search, through `TextPage::search_counted` | built |
| `word_boundaries_computed` | a form's lines' UAX #29 boundaries computed, for a whole-word search (P-24) | the same | built |
| `search_steps` | a step of a search's matcher or mapping: a byte of a line's kept text read, a fall back of its automaton, an entry of its needle's failure table, a word boundary or a glyph a cursor moved past (P-24) | the same | built |
| `group_buffer_pixels` | a pixel of a buffer a transparency group or a soft mask's group allocated for its own pixels while it was open, counted at the allocation — a quantity (P-25) | the renderer | built |
| `group_bound_pixels` | a pixel of the region such a group opened over, once a group — a quantity | the renderer | built |
| `soft_mask_pixels` | a pixel of a soft mask the renderer installed — a quantity | the renderer | built |
| `soft_mask_bound_pixels` | a pixel of the region of the group such a mask was read from, once a mask — a quantity | the renderer | built |
| `drawn_glyphs`, `drawn_fills`, `drawn_strokes`, `drawn_clips`, `drawn_images`, `drawn_shadings`, `drawn_groups`, `drawn_soft_masks`, `hidden_calls`, `decoded_images_drawn`, `decoded_image_samples` | a call the renderer drew, by class; one hidden optional content kept from painting; an image drawn from decoded samples, and its samples — the per-render statistics (`docs/design/page-analysis.md`), which are not steps | the renderer: a page's, its first band's when it is drawn in bands, and never a tiling cell's | built |

`work()` sums every counter that counts a step taken, and leaves out maxima and
quantities — `most_decodes_of_one_stream`, `bytes_decoded`, and every byte or
pixel figure a later row adds — because a byte total grows with the page and
can hide a step count that grows with the document. `bytes_scanned` is in: a
byte the repair scan looked through is a step it took, not output. A page loop whose `page(i)`
reads `i + 1` nodes, over pages that decode 20 kB each, is 4.29 times as
much work at 4 000 pages as at 1 000 when its bytes are summed in, inside A2's
4.4, and 15.99 times without them. A row that adds a counter says which kind it
is. The per-render statistics are neither: each classifies a call whose steps
are counted where they were taken, so `work()` leaves them out too.

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
| A page lookup: `pages::at` and the walk under it (`pages.rs:488-499`) | a `Work` on the door's stack, passed as `Option<&Work>` down `at`, `collect_upto` and `walk`, and to the linearized and hinted paths | built |
| A render: `Page::render` (`crates/tinker-pdf/src/lib.rs:2247`), `Page::render_form` and `Page::render_annotation` (`crates/tinker-pdf/src/render_part.rs:175`, `:230`), `DisplayList::render` (`display.rs:181`) | the render's `PageResources` holds it — built with it by `Page::render_layer` (`lib.rs:2269-2280`), handed it by `for_one_render` for a display list (`resources.rs:546`); `annots::draw` takes it for the appearances it draws; the page's content decode, made before the scope exists (`lib.rs:2248`), takes it too | built |
| A scope a scope makes: a pattern cell's and a form's own in a render (`resources.rs:1858-1868`, `:872-890`); and `own_scope`'s — a pattern cell's, a mask group's or a form's in the save's font walk, which only `subset.rs` calls (`resources.rs:750-764`) | inherits its maker's | built |
| A scope a cache keeps: a form's, in `form_scopes` (`resources.rs:856-869`) | kept without one, and handed out bound to the asker's, its caches shared so a hit stays a hit — a kept scope outlives the render that built it, and two renders of one display list may hold it at once | built |
| The renderer | its own plain counts, handed back by `finish_counted`, `finish`'s sibling, and added by the door | built |
| Text and image extraction | as a render: the content decode and the `PageResources` the door builds (`lib.rs:2533-2540`, `crates/tinker-pdf/src/images.rs:328-339`) | built |
| Validation | `pdfa::validate`, `pdfua::validate` and `pdfx::validate` take it, so their page walks (`crates/tinker-pdf/src/pdfa/content.rs:90`) and their decodes count | built |
| An editor | `DocumentEditor` holds it from `count_work` until it is dropped. Its own page and object operations count into it in cos — `page_refs`'s walk (`crates/tinker-pdf-cos/src/edit/page_ops.rs:16-24`), `import_page`'s `pages::collect(source)` (`:185`), its overlay's decodes (`edit/overlay.rs:48`) — and the facade's passes over it count into it too: a save's image and font passes (`crates/tinker-pdf/src/write.rs:337`; `subset::collect` builds a scope a page), a redaction. A rolled-back transaction keeps its counts: the work was done | built |
| A save | the editor's tally if it is counting, and one of the save's own if not; the serializer counts into whichever the save carries | built |
| A stream decode | `CosDocument::stream_decoded` and `stream_image_input` have siblings that take `Option<&Work>`, `stream_decoded_counted` and `stream_image_input_counted`, and the uncounted ones delegate with `None`, so no caller outside an operation changes; `pages::content_bytes_counted` and `xmp_metadata_counted` are the content's and the catalog packet's. `Counting` is the counting view — a `Resolve` over any `Resolve` and a `&Work`, whose `stream_decoded` runs the inner one's and counts it — so code written against the trait counts when handed the view, as a font's CMap reads are (`font::read_in`; `read_cmap`), through the `Cached` view over it that decodes each once an operation (P-03). An editor counts its own: `stream_bytes` into its tally, `stream_bytes_counted` into the one a pass carries, and the serializer's deduplication into the save's through `save_counted`. A stream the editor holds unfiltered runs no chain and counts nothing; what a document decodes for itself, once — object streams, cross-reference streams, hint tables — is no operation's | built |

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
(`lib.rs:705-718`) names a base, and so does every `SaveOptions` literal. A
door with no options value, or one whose callers write it out whole, gets a
counted sibling instead. `TextOptions` is written out whole by its callers, so a field on it
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
| text | `Page::text_counted(&self, &TextOptions)` | `(TextPage, Stats)` | built |
| save: `write::save` | `SaveOptions::stats: bool` | `Saved::stats: Option<Stats>` | built |
| merge: pages imported, then saved | `DocumentEditor::count_work(&mut self)` before the first import, and `SaveOptions::stats` | `Saved::stats`: everything the editor counted since `count_work`, the save's own work included | built |
| image extraction, the decode door | `Page::images_counted(&self)` | `(Vec<PageImage>, Stats)` | built |
| validation | `Document::validate_pdfa_counted(&self, PdfACoverage)`, and `validate_pdfua_counted` and `validate_pdfx_counted` alike | `(verdict, Stats)` | built |
| page lookup, the document door | `Document::page_counted(&self, index: u32)` | `(Option<Page>, Stats)` | built |
| reading-order inference | `Document::inferred_orders_counted(&self, Range<u32>, &InferenceOptions)` | `(Vec<InferredOrder>, Stats)` | built |
| PNG, the bitmap door | `Bitmap::to_png_counted(&self)` | `(Option<Vec<u8>>, Stats)` | built |

`Bitmap::stats` is a public field rather than a method: a method needs a field
the caller cannot see, and one private field would stop every hand-built
`Bitmap` compiling, where a public one asks each of them for `stats: None`;
nothing in the C ABI or the bindings builds one. `Saved::stats` joins `Saved`'s
other public fields; its one literal is the facade's own (`write.rs:388`).

The other doors that walk or decode — `Document::pages`,
`Document::page_count` and the rest — gain counted siblings in the rows that
first need them, by these two rules, and not before.

### The guard suite

`crates/tinker-pdf/tests/guards.rs` holds four things.

**Generators.** A balanced 10 000-page tree of stated depth and fan-out for A1
— ten thousand is 10⁴, so fan-out 10 over four levels is the shape under which
the per-lookup bound is small enough to mean something, and the generator
states its own shape and asserts it; documents of 1 000 and 4 000 pages for
the A2 legs, one generator for all nine, which `guard_allocations.rs`
reads too from `guard_support/mod.rs` — a line of text a page from a
deflated content stream of its own, under a flat tree, with a packet claiming
PDF/A-2b, which is the least that gives text, a render, subsetting and
redaction glyphs, a rewrite, a rotation, a split and a merge pages, and a
validation a claim to judge; a 5 600-glyph page of 80 distinct glyphs for A4,
whose every glyph origin falls on a whole pixel at 72 dpi, so that its
distinct (glyph, scale, phase) keys at that scale are its 80 glyphs; four
documents of 100 pages to merge for A6, the same generator's, whose faces are
one encoded stream and whose pages' contents all differ, so that a merge's
deduplication meets both halves of its mechanism: the copies of the face the
imports make, one encoding, and encodings that all differ under the dictionary
the pages' contents share with the face; and, for A5, a document of 100 pages
each drawing one embedded face with a `/ToUnicode`, which a save's every page
reads, and a page showing one form three times and one Type 3 glyph fifty
times, whose form names the page's face in its own resources and draws an
image in the ICC profile an image of the page's shares, every stream deflated,
so that a render, a text extraction and an image extraction each meet a
stream that several of their scopes, or several draws, read. Each is written in memory, object
by object, by the generator itself, so a tree's shape is the generator's
statement rather than the writer's choice, and each has a test that holds it
to its shape. Pages carry the least content that exercises their leg, with
their faces embedded — `tests/render_support`'s `curvy_font`
(`crates/tinker-pdf/tests/render_support/mod.rs:434`) — so the default build
and `bundled-fonts` count the same work. Nothing is read from disk, because
wasmtime, run with no directory granted, gives the `wasm32-wasip1` run no
filesystem, and nothing is fetched.

**The roster.** A `const` table with one entry per guard test: its guard, the
clause it holds, its test's name, the file it is in, the row that owns it and
the step it lands in, and whether it is landed or owed. `every_guard_is_landed_or_owed`
reads each file the roster names through `include_str!`, since the wasm run
has no filesystem to read them from, and fails when a landed entry is not an
un-ignored `#[test]` there or an owed one already is;
`the_roster_tells_a_running_test_from_every_other_case` holds that reading to
a source with a test that runs, two that are ignored, a function that is no
test and a name that is absent, so a reading gone blind cannot pass the
roster. An owning row lands its entry in the commit that fixes the
cause, flipping it from owed to landed. A guard that holds at the tip — A2's
subset, validation and split legs (`subset::collect` in `crates/tinker-pdf/src/subset.rs`,
`crates/tinker-pdf/src/pdfa/content.rs:90`) and A10's flat entries — lands
green. An entry a later row adds already landed, as P-18.5 adds A2's book, is
never owed.

**Pinned counts.** `the_tally_counts_the_same_on_every_target` holds the
`Stats` of a render, a text extraction, a save and a page walk over fixed
fixtures to committed values, and K-07's determinism roster holds it to being
an un-ignored `#[test]`. The guards each compare counts on one target; the
pins are what show the counts every guard reads do not depend on which target
read them. They are
goldens, so they hold the counters whose value is the fixture's rather than
the algorithm's, over one page of a line in the embedded face and an image
drawn once: a render's glyphs shown, and its streams decoded — the content,
the face and the samples, once each — with the bytes they inflate to; a text
extraction's one decode of the content; a save's image pass, its one walk of
the one-page tree and its two decodes, the content and the samples; and a
lookup's walk of the root and the page. A cache's hits and misses, the font
dictionaries read and the outlines filled are how the code does the work, and
are not pinned. A pin a fix has to move is a finding the owner rules on, as a
moved fingerprint is.

**No clock.** `the_guard_suite_reads_no_clock` reads the suite's sources —
`guards.rs` and `guard_allocations.rs`, `guard_support/mod.rs` and
`render_support/mod.rs`, which they are built with, and every file the roster
names — and fails on `Instant::now` or `SystemTime::now`, as
`bounds_ledger.rs` holds the sources its ledger lists
(`crates/tinker-pdf/tests/bounds_ledger.rs:3229-3255`); it puts its needles
together, so that `guards.rs` does not hold them whole.

**A2's bound** is 1.1 times the ratio of the two sizes: 4.4 for 4 000 pages
over 1 000, as the exit states it, and 11 for P-18.5's 1 000-chapter book over
100. A2 compares `work()`, and a failing leg prints every counter's ratio beside
it, so the counter that grew is named by the failure rather than found after
it. A2 sees only what is counted: a superlinear loop that counts nothing passes
it, which is why P-01.1 records, for each leg, the site that is superlinear at
the tip and the counter that sees it.

**A10** counts allocations, which no tally sees without instrumenting every
allocation. `crates/tinker-pdf/tests/guard_support/mod.rs` holds a counting
`#[global_allocator]`, `Counting`, which forwards every call to `System` and
counts each allocation against the thread that made it, in a `const`
thread-local `Cell` that neither allocates nor registers a destructor, so it
can count from inside an allocation. A test reads its own thread's
allocations, so a binary that installs it holds several tests without their
counts mixing — where a process-wide count would need a binary a test, the
rule `property_list_work.rs:30-31` keeps — and A10's guards land in
`guard_allocations.rs` beside the measurement. Every operation A10 names runs
on its caller's thread; an operation that handed work to other threads would
count only its caller's share, so P-15, whose pool is the first that does,
counts its workers into the operation or gives A10's render leg a binary of
its own. The thread-local is the test allocator's; the tally stays one `Work`
an operation.

`allocations_per_page_by_operation` (`guard_allocations.rs`) runs each
operation as the CLI runs it over a whole document — the pages counted, which
builds the page index (P-01), then every page looked up by its index and
read — over A2's documents of 1 000 and 4 000 pages, each written and opened
outside the count, and prints its allocations a page at both sizes, their
ratio, and what text and the render allocate a page beyond their lookup; the
figures are what it prints (ruling 16), and the default and `bundled-fonts`
builds count alike.

A count a page at 4 000 pages within 1.1 times the count at 1 000 is flat,
and A10's three guards hold each operation to it. A lookup copies one entry
out of the index the count built, whatever its index, so a whole document's
lookups allocate in proportion to its pages, and text and the render add what
their own page costs.

### The three targets

MS-01.8 runs the suite where ruling 4's measured targets run.

- **CI's tests**, on `ubuntu-latest` and `windows-latest` on every pull
  request, run `guards.rs` and `guard_allocations.rs` with the rest of
  `cargo test --workspace`, and again in `-p tinker-pdf`'s
  `--no-default-features` and `--features bundled-fonts` builds.
- **The `wasm32-wasip1` run** is local proof: `cargo test -p tinker-pdf --test
  guards --test guard_allocations --test producer_work --target
  wasm32-wasip1`, with wasmtime as the runner, in the default build.
- **A guard in a file of its own** — A7's two, in `images.rs` and
  `jpx_images.rs`, and P-18's producers', in `producer_work.rs` — runs in
  CI's tests as every integration test does, and joins the `wasm32-wasip1`
  run's command in its row's commit.
- **The pinned counts** join K-07's determinism roster
  (`xtask/src/determinism.rs`, which K-07.1 creates), so `cargo xtask check`
  fails when one of them is ignored or gone.

A green tick from a `cfg` that dropped the target or an `#[ignore]` added one
afternoon is the hazard "a check that can be absent is red, not green" names
(`docs/verification.md`). The rosters close the `#[ignore]` case:
`every_guard_is_landed_or_owed` and `cargo xtask determinism` each fail on a
landed entry that is not an un-ignored `#[test]` in its file. A `cfg` that
drops a target is caught by neither. `aarch64-apple-darwin` runs none of the
suite, so on that target the counts are claimed
(`docs/features/determinism.md`).

There are no threads on `wasm32-wasip1`: P-15's pool leaves the share of a
worker that will not start to the caller (P-15.2), so A9's legs there cut and
stitch on the caller and still compare against the serial render (P-15.9). The
1 000-, 4 000- and 10 000-page generators run in debug in CI's tests and under
wasmtime. A guard too slow for a target is never ignored or shrunk below its
page counts, and what keeping it costs goes to the owner.

### The roster

Every entry starts owed but A2's book, which entered landed. A guard
lands in the step named, in
`crates/tinker-pdf/tests/guards.rs` unless another file is named.

| Guard | Clause, as the row's exit states it | Test | Owner, and the step it lands in |
| --- | --- | --- | --- |
| A1 | `page(i)` visits its root and ≤ depth × fan-out nodes under it | `a1_page_lookup_reads_at_most_depth_times_fan_out_nodes` | P-01, P-01.2, landed |
| A1 | every page of a generated 10 000-page document ≤ 3 × 10 000 in total | `a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits` | P-01, P-01.3, landed |
| A2 | text: work(4 000 pages) ≤ 4.4 × work(1 000) | `a2_text_is_linear_in_pages` | P-01, P-01.3, landed |
| A2 | render-all at 36 dpi | `a2_render_all_at_36_dpi_is_linear_in_pages` | P-01, P-01.3, landed |
| A2 | rewrite | `a2_rewrite_is_linear_in_pages` | P-01, P-01.3, landed |
| A2 | rotate | `a2_rotate_is_linear_in_pages` | P-01, P-01.4, landed |
| A2 | merge | `a2_merge_is_linear_in_pages` | P-01, P-01.4, landed |
| A2 | split | `a2_split_is_linear_in_pages` | P-01, P-01.4, landed |
| A2 | subset | `a2_subset_is_linear_in_pages` | P-01, P-01.3, landed |
| A2 | redaction | `a2_redaction_is_linear_in_pages` | P-01, P-01.4, landed |
| A2 | validation | `a2_validation_is_linear_in_pages` | P-01, P-01.3, landed |
| A2 | a 1 000-chapter book against 100 chapters, beyond the exit's nine | `a2_a_thousand_chapter_book_costs_at_most_eleven_times_a_hundred` | P-18, P-18.5, entered landed |
| A3 | font dictionaries resolved per page ≤ distinct `/Font` entries used | `a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses` | P-02, P-02.3, landed |
| A4 | glyph masks rasterised ≤ distinct (font, glyph, scale, phase) keys, hit ratio ≥ 98 % on a 5 600-glyph page | `a4_glyph_masks_rasterised_are_at_most_their_distinct_keys` | P-06, P-06.3 |
| A5 | inflate calls per stream ≤ 1 per operation: a save | `a5_a_save_decodes_each_stream_at_most_once` | P-03, P-03.2, landed |
| A5 | a render | `a5_a_render_decodes_each_stream_at_most_once` | P-03, P-03.2, landed |
| A5 | a text extraction | `a5_text_decodes_each_stream_at_most_once` | P-03, P-03.2, landed |
| A5 | image extraction, the decode door | `a5_image_extraction_decodes_each_stream_at_most_once` | P-03, P-03.2, landed |
| A5 | a merge | `a5_a_merge_decodes_each_stream_at_most_once` | P-03, P-03.4, landed |
| A6 | merge deduplication decodes no byte-identical copy, and each differing encoding under one dictionary once a save (ruling 23) | `a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once` | P-03, P-03.6, landed |
| A7 | IDCT output ≤ 4 × drawn pixels for a quarter-scale draw | `a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel`, in `crates/tinker-pdf/tests/images.rs` | P-09, P-09.8, landed |
| A7 | JPX levels decoded ≤ levels needed | `a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer`, in `crates/tinker-pdf/tests/jpx_images.rs` | P-10, P-10.8, landed |
| A8 | whole-image scratch on the PNG path ≤ 1 × the image | `a8_the_png_path_holds_at_most_one_image_of_scratch` | P-04, P-04.5, landed |
| A9 | page-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads | `a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads` | P-15, P-15.3, landed |
| A9 | band-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads | `a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads` | P-15, P-15.7, landed |
| A10 | allocations O(1) per page: page lookup | `a10_page_lookup_allocates_a_flat_amount_per_page`, in `crates/tinker-pdf/tests/guard_allocations.rs` | P-01, P-01.3, landed |
| A10 | text | `a10_text_allocates_a_flat_amount_per_page`, in `guard_allocations.rs` | P-01, P-01.3, landed |
| A10 | render-all at 36 dpi | `a10_render_all_at_36_dpi_allocates_a_flat_amount_per_page`, in `guard_allocations.rs` | P-01, P-01.3, landed |

The names A7 and A9 carry, and A2's book, are the ones their rows' steps
already give; the rest are given here. A10's file is `guard_allocations.rs`, whose allocator counts a test's own
thread.

## Risks

| Risk | Mitigation |
| --- | --- |
| A count drifts from the work it names: a site moves, a path is added that does not count, a refactor counts twice | Every count is checked against a figure the test derives from its fixture |
| Counting changes what an operation does | Nothing reads a `Work` but a door; a kept scope handed out bound shares its caches, so a hit stays a hit; `stats_do_not_change_a_pixel` (pages and tiles) and the other doors' byte-identity checks hold it |
| Off is not free | An uncounted operation carries `None`: no lock, no allocation, one tested `Option` a site; MS-02's floors of 1 hold every operation to the baseline once that run exists |
| Two threads' counts mix | One `Work` an operation, never on `Document`, in a global or in a thread-local; an operation's own workers add to one total in any order |
| Hits and misses depend on interleaving when threads share a cache | Guards and pins read serial runs; A9 compares pixels and warnings, not counts; P-15.6's single-flight fills make a shared cache's counts deterministic and its test reads them |
| A count differs by target | `u64` counters with saturating adds, a `BTreeMap` for the per-stream decodes, no count of a hash-ordered walk; the pins are read on all three measured targets |
| The pins are goldens and the P rows exist to cut work | The pins hold counts that are the fixture's rather than the algorithm's, and say which; a pin a fix has to move is a finding the owner rules on, raised by the row that moves it |
| A2 reads only what is counted | A superlinear loop that counts nothing passes A2; P-01.1 records each leg's superlinear site at the tip and the counter that sees it, and a site outside P-01 is a finding, not absorbed |
| A byte total hides a step count | `work()` leaves out maxima and quantities, and a failing leg prints every counter's ratio |
| A guard is absent and reads as green | The roster checks each landed name is an un-ignored `#[test]` in its file, and `cargo xtask determinism` holds the suite's entries in K-07's roster to the same (`docs/verification.md`) |
| A guard is too slow for a target, in debug or under wasmtime | Pages carry the least content that exercises their leg; a slow guard is never ignored or shrunk below its page counts, and what keeping it costs goes to the owner |
| An allocation count sees another test's allocations | The allocator counts each thread's apart, and every operation A10 names runs on its caller's thread; P-15, whose pool is the first to hand work to other threads, counts its workers into the operation or gives A10's render leg a binary of its own |
| A hand-built `Bitmap` stops compiling | The field is public, so a literal needs `stats: None`; nothing in the C ABI or the bindings builds one |
| `Stats` names counts made above cos | Each counter's doc comment names the layer that counts it, and cos counts only its walk, its filter chain and its editor; the alternative was two ledgers an operation, and an editor able to carry only one of them |

## MS-02 — A paired base-versus-head wall-clock gate

In this design, a change that makes an operation of MS-04's set slower
fails the paired run that judges it, on whatever machine it runs on, with no
time recorded for that machine to compare against. Base, head and the
planning baseline `61adcde` are built in one run on one machine, every
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
- **The triggers**, and **what a run costs**: each part priced here on tier
  P's figures, and again on MS-04.10's measured set when it lands.

### Non-goals

- **An absolute figure.** A pair says that one build is slower than another
  on the machine both ran on, never that an operation takes more than so many
  milliseconds; that is MS-03's pinned machine (H-110). MS-04's runs against a
  machine's entry, and their bands, stay MS-04's.
- **A second timing.** Every figure is criterion's, read from the files it
  wrote; nothing re-times anything, and no clock enters `cargo test`.
- **Memory, counts and wasm**, which are MS-05's, MS-01's and MS-10's.
- **A price.** A floor below 1, a band wider than the A/A rule and any
  loosened gate are H-78's, and a target is H-77's. This section gives the
  figures each is decided on and decides none of them.
- **The planning comparison.** H-112's figures stay outside the repository;
  nothing here reads or writes them.

### Design

#### Three builds on one machine

The run checks out three revisions side by side — the head, the base and
`61adcde` — and builds each in release in a target directory of its own: the
bench binaries of MS-04's thirteen group files, and `tpdf`, which the
CLI-marked operations run as a child resolved beside the bench binary
(MS-04, "The CLI operations"). The base and the head build as committed.
`61adcde` builds with the head's `crates/tinker-pdf/benches/` and the head's
`[[bench]]` entries in place of its own, under the `cfg` below, beside its
own `tpdf`. Every timed process of all three then runs on the one machine,
so each ratio is of two builds on one host minutes apart. Hosts differ, and
one host's speed moves between runs (`docs/verification.md`, "The named
machine is `ubuntu-latest`"); a ratio taken on one host divides that out.

Each build names its bench targets, `--bench <group>` for each of the
thirteen, because `cargo bench -p tinker-pdf` also builds the library's own
unit tests as a bench target (its output lists `benches src/lib.rs`), which
no round runs; `tpdf` then builds in the same target directory, where it
reuses the engine the bench build compiled and compiles itself alone. The
run starts the binaries itself, never through `cargo bench`, whose start it
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
reads and the run keeps. A directory a round also means criterion never
finds a saved `base` beside the operation, so it never runs a comparison of
its own (`analysis/mod.rs:183-188`). The run never passes
`--discard-baseline`, under which criterion writes none of these files
(`lib.rs:1150-1154`).

#### The rounds' sampling

| Setting | In the rounds | How it is set | Why |
| --- | --- | --- | --- |
| Samples | 10 | `--sample-size 10` | criterion's least (`criterion-0.5.1/src/lib.rs:1096`, `benchmark_group.rs:98`); the command line overrides each file's `criterion_group!` `config` (MS-04, "Sampling, and a run's length") |
| Warm-up | one iteration | `--warm-up-time 0.001` | the warm-up loop stops at the first call whose time passes it (`routine.rs:257-282`) |
| Measurement | one iteration a sample | `--measurement-time 0.001` | flat sampling takes ⌈measurement time ÷ (samples × the warm-up's mean)⌉ iterations a sample (`lib.rs:1430-1454`), which is 1 for any operation over 0.1 ms |
| Mode | flat | by each group itself | no flag sets it (MS-04); linear sampling would take 55 iterations where flat takes 10 |
| Resamples | 1 000 | `--nresamples 1000` | the verdict reads the mean, which criterion takes from the sample before it resamples (`analysis/mod.rs:313`, `:323`); its default of 100 000 (`lib.rs:430`) is most of what a process costs beyond its iterations |
| Plots | none | `--noplot` | criterion 0.5.1 draws its report through gnuplot whenever one is on the path, whatever its features (`lib.rs:106-122`, `:421`, `:1029-1031`; `report.rs:328-330`), and the run's figures must not depend on what the machine has installed |

An operation of t therefore costs 11 t a build a round, one warm-up iteration
and ten samples of one, and 330 t over the run: criterion's floor of ten
samples an operation a round, the least it allows.

#### The inputs both sides are timed on

A paired ratio compares two times of one input, and two engines generating
"one" input write two. Every generator not pinned by formula writes through
the tree's own builder, encoders and editor (MS-04, "The inputs"), and
`scan()` and the encode group's bitmaps through its renderer, so a head that
changes any of them would be timed on other bytes than its base, and
`61adcde` on other bytes again; and a committed file is read from each
build's own checkout, so `61adcde` would read its own tree's. So the run sets
`TINKER_BENCH_FIXTURES` for every process, to a directory that starts empty,
and every group file takes each input through `support/inputs.rs`' store:
with the variable set, an input — generated, or committed and read by path —
is read from the file of its name there, and written there first when the
directory holds none. Round 1 runs the head first, so the first process to
need an input is the head's: all three builds are timed on the head's bytes,
and each generator runs once in the whole run rather than once a process. A
render's bitmap crosses as its pixels, a set of files as one bundle of them
by name, and a CLI operation's child reads the directory's file itself. A
generator builds from other generators, never from the directory, so a
process writes the inputs its operation reads and no other's. The
calibration's input is the one left each build's own: `calibration.rs` is
frozen and calls its generator itself, and `61adcde`'s figure for it measures
the machine, which a later change to the head's builder must not move
(H-77). An input a base or `61adcde` process cannot read fails that
operation's process, and so leaves its rounds missing on that side.

A process that times one operation builds nothing for the rest of its
group: the harness defines an operation — builds what it reads and the
state it starts from, checks them, declares its routine — the first time
criterion calls the operation's routine, outside the time criterion takes,
and keeps what it built for the rest of the process. An operation criterion
leaves out is never defined, so `--bench --exact <id>` builds that
operation's inputs alone and `--list` builds nothing; what the routine's
last run wrote is checked after its runs. Criterion takes the figure it
reports a rate in when an operation starts, before the definition, so the
harness hands it the one the group's list commits (MS-04, "`baseline.json`'s
lists").

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
  names — the full-size decode for the quarter-scale one,
  `decode/JPEG decode at quarter scale` — and the ratio is held to 1 ÷ the
  factor as a floor is.
- **calibrated**, where `61adcde` lacks the capability altogether: the head's
  figure over `61adcde`'s `calibration/text of 5 dense pages` in the same
  round, against the target over 37.6 ms (`P.md:43-48`), so 0.5 ms reads as
  at most 1/75.1.
- **a relation**: two operations of the head's group in the same round, the
  ratio of their per-call figures against the bound, in its direction.

Tier P writes the factor as the median against 1 ÷ the floor
(`P.md:32-35`) and the others as bounds. Read that way, a floor of 1 cannot
be met by a head that runs an operation at `61adcde`'s speed: with the two
speeds equal, the median of ten ratios lands above 1 in about half of all
runs, by symmetry, so on a day when most of the set runs where `61adcde` left
it, each of those operations fails about one run in two and the paired run
cannot be read green. This design holds each gate to the regression pair's rule
instead: the median beyond the bound by more than the operation's band, with
at least eight of the ten rounds beyond the bound, a relation taking the
wider of its two operations' bands. That loosens tier P's text by the band,
and loosening a gate is H-78's, as H-77's default says; MS-02.7 asks for it
with the bands H-109 approves, and until it is decided the run prints both
readings.

An operation whose gate names a baseline side that `61adcde`'s rounds lack
is refused, not reported: the gate data says it can be judged, which MS-02.4's
`every_operation_has_a_baseline_side_or_a_calibration` holds, so its absence
is a fault of the mapping, and a check that can be absent is red
(`docs/verification.md`). A head slower than `61adcde` beyond its band
on the day the floors are set is a finding against that operation's row; a
floor below 1 is H-78's to grant, and no step lowers one.

#### The bands

An A/A run's **paired spread** for an operation is how far its median
paired ratio lies from 1, in per cent: the statistic the verdict reads,
taken on two builds of one revision. The operation's band is the worst
spread of fifteen such runs, half again as wide, rounded up to the next
5 % and never under 5 % — the rule MS-04's bands are set by, for the reason
`docs/verification.md` gives under "The named machine is `ubuntu-latest`": a
spread over fifteen runs is a lower bound that has not converged. A band set so
clears the medians of its own fifteen runs by arithmetic, which is no evidence
that the band is wide enough; only a run no band was fitted to is. An A/A
run needs no `61adcde` rounds, since a band is read from base and head
alone. H-109 approves the bands; a band wider than the rule gives is H-78's.

#### Where the numbers live

| What | Where | Written by | Moved by |
| --- | --- | --- | --- |
| Each operation's floor, 1 to start, or held | its entry in `baseline.json`'s group list, as `gate`, beside its unit (MS-04, "`baseline.json`'s lists") | MS-02.3, from tier P's Gate column | raised by a P row in its closing commit; lowered only by H-78 |
| The operation `61adcde` runs in another's place | the same `gate`, naming it; the bench files' `cfg` arm runs it | MS-02.3 and MS-02.4 | the row that adds the operation |
| Calibrated targets | the same `gate`, in milliseconds a call or as the rate tier P states; the calibration operation and its 37.6 ms named once, ahead of the groups | MS-02.3 | re-stated by H-77; loosened by H-78 |
| Relations | the same `gate`: the other operation of the group, the bound and its direction | MS-02.3 | as calibrated targets |
| A/A bands | each operation's entry: its spread and its band; the fifteen runs named once, beside the calibration | MS-02.7 | approved by H-109; widened by H-78 |

```json
"calibration": { "operation": "calibration/text of 5 dense pages",
                 "planning_ms": 37.6 },
"paired_runs": { "runner": "…", "runs": [ ... ] },
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
at `"machine":` and `"id":` (`xtask/src/bench.rs:542`, `:612`), which no key
here contains. `cargo xtask baseline` refuses an operation with no gate and a
relation naming an unknown operation (MS-02.3), and a band without its runs
and spread (MS-02.7). H-112's planning comparison is in none of these
places.

#### The head's bench files on `61adcde`

- **The `cfg`**, `tinker_planning_baseline`, is set for that build alone,
  through `RUSTFLAGS` beside a `--check-cfg` naming it, with no
  `-D warnings`: `61adcde` is frozen, and a lint a later stable adds must not
  fail it. Its `tpdf` builds in the same target directory under the same
  flags, so the engine compiles once for both. Under the `cfg` each bench
  file's operation bodies call `61adcde`'s facade; the generators need no
  arm, calling only what that revision exposes (MS-04, "Files"), and the
  store's one, the bitmap it builds by hand from stored pixels, since that
  revision's `Bitmap` carries no statistics. An operation that revision
  lacks altogether is declared with `Group::calibrated`, which the build
  leaves out. That build holds nothing an operation declares it processes
  to the head's lists, since its own engine computes what it declares.
- **The declaration.** The head declares the `cfg` to `check-cfg` in
  `crates/tinker-pdf/Cargo.toml`, or `unexpected_cfgs` fails its clippy
  under `-D warnings`: a `[lints.rust]` table, the one in the tree, standing
  above the `[[bench]]` entries, so the copy takes the entries alone.
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

The paired run is local proof, made on one machine by whoever makes a change,
and recorded in the roadmap row the change closes; CI runs only the unit and
integration tests.

- **A change**: head the change, base the revision it starts from, so the
  pair is the change and nothing else, and a change of several commits is
  judged as one.
- **An A/A run**, base and head both one revision, checked out and built
  twice exactly as a real pair's are, so a band measures everything two
  builds differ by except the change.

Without committed bands the run prints every verdict and fails on none; with
them it fails as they say. A base or `61adcde` that cannot be checked out or
built fails the run, since a pair that cannot be judged is red, not green
(`docs/verification.md`).

#### What a run costs

A run is three release builds and thirty runs of the set. Its parts, what
each costs, and where the cost is read from:

| Part | Cost | By tier P's figures | Read from |
| --- | --- | --- | --- |
| Three checkouts, the toolchain and the cache | once | — | the run's log |
| Three release builds of the thirteen bench binaries and `tpdf` | 3 × b, each bench binary a link under the release profile's thin LTO and single codegen unit (`Cargo.toml:86-88`) | — | the run's log |
| 10 rounds × 3 builds × 138 operations, 11 iterations each | 330 × Σt | Σt, the time of one iteration of every operation, 207.7 s: 1 142 min | MS-04.10's figures |
| Setup inside an iteration, untimed | 330 × s | s, the 150-dpi first render of `a second render of the quarter-size JPEG page at 300 dpi under a 64 MiB budget` (`iter_batched`), taken at tier P's 652 ms for the A4 JPEG page at 72 dpi, whose JPEG holds sixteen times its samples, as a bound: 3.6 min | MS-04.10's figures |
| 4 140 processes: start, criterion's analysis, its files | 4 140 × p | — | the run's log |
| Two calls of `bench-check --paired`, and keeping the rounds | once | — | the run's log |

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

| Group | Operations | Σt | Its rounds in the run |
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

**What a run takes, by these figures.**

- **The rounds** take 19.0 hours before a build: most of a day of one
  machine. `rewrite 3000 pages` at 5.48 s alone needs 330 × 5.48 s, 30.1
  minutes, and the three that read 10 000 pages are 100.5 s of Σt's 207.7.
- **At any figures inside MS-04's window**, 138 operations of 20 ms or more
  cost at least 138 × 330 × 20 ms, 15.2 minutes of rounds.
- **The planning baseline's third**, 110 × Σt, is 6.3 hours and never
  shrinks: a P row makes the head faster, and `61adcde` stays what it was.

What a run can give up, each alone and at the same figures:

| What gives | Rounds | What it costs |
| --- | ---: | --- |
| The number of rounds: one | 33 × Σt, 1.9 h | a sign test of 8 in 10 needs ten |
| The planning baseline's half, to a run of its own | 220 × Σt, 12.7 h, in each of two runs | the baseline's run times the head beside it, since a pair is one machine's |
| The operations the paired run times: the 82 inside 20–300 ms | 330 × 8.3 s, 45.7 min | the rest of the set goes unjudged |

These are priced again on MS-04.10's measured figures, never on tier P's
column, before MS-02.6 builds the run.

### Risks

| Risk | Mitigation |
| --- | --- |
| A run takes most of a day of one machine | The costs above give the figures — 19.0 hours of rounds at tier P's, at least 15.2 minutes at any figures inside the window — and are priced again on MS-04.10's figures before the run is built |
| A pair times two inputs, because two engines wrote them | Every input through `TINKER_BENCH_FIXTURES`, written by the head first and read by all three builds |
| Two builds' times are taken on unlike hosts | Both sides of every pair run in one run on one machine, so the host divides out of a ratio |
| The host drifts while a round runs | An operation's three processes run back to back, so a pair spans one operation's run, and the first build rotates by round |
| The sign test fires on noise | It fires on 5.5 % of operations on identical code, so an operation fails only with its median past the band as well |
| A floor of 1 fails about half the runs of a head that matches `61adcde` | The gates take the band and the sign test, H-78's to approve; until then the run prints both readings |
| A head is slower than `61adcde` on the day the floors are set | A finding against that operation's row; the floor stays 1 unless H-78 grants less |
| An operation the base or `61adcde` cannot express | One the base lacks is reported, not judged; one `61adcde` lacks runs the operation its Gate column pairs it with, or is calibrated (MS-02.4) |
| `61adcde` cannot read an input the head writes | Its process of that operation fails, its rounds are missing, and an operation whose gate names a baseline side it lacks is refused, so the run fails on MS-02.4's mapping rather than passing the operation unjudged |
| The calibration moves within a round | Each calibrated ratio is taken against the same round's calibration, and the band absorbs what moves between the two |
| A lint a later stable adds fails the frozen baseline | Its build carries no `-D warnings` |
| A round writes nothing and reads as a pass | A directory a round and never `--discard-baseline`; the guard counts every operation in all ten rounds of every build (MS-02.6) |
| A base or `61adcde` that will not build lets the run pass on the head alone | The run fails: a pair that cannot be judged is red (`docs/verification.md`) |
| Criterion's own cost swamps the rounds | `--nresamples 1000` and `--noplot`, which take out most of what a process costs beyond its iterations |

## MS-04 — The benchmark set, replacing the seven

In this design, `cargo bench` times every operation of tier P's target
table that the tree can run, and the row's own, in twelve groups of one file
each, MS-09's `threads` among them: 173 operations over inputs the benches
generate or the tree holds, each
held by `cargo xtask bench-check --machine` to the band its machine's entry in
`baseline.json` records, from a local `cargo bench -p tinker-pdf` on that
machine. MS-02's paired run, MS-05's probe, MS-09's thread group, MS-10's wasm
ratio and every P row's floor are built on this set, and each later capability
row adds its operation
to it in the commit that adds the capability.

### Scope

- **The fourteen groups**: open, text, analysis, render, encode, decode,
  write, sign, layout, fonts, threads, scripts, OCR and calibration, one bench
  file each over a shared support module; scripts and OCR arrive with the rows
  that add their operations, and threads is MS-09's.
- **Every operation** of tier P's table (`docs/roadmap/P.md:63-135`) and of
  the row's Item whose capability exists: its criterion id, its unit and what
  one iteration covers, its input beside the input the table states, and where
  that input comes from.
- **The inputs the tree cannot write** — a progressive JPEG and two JPEG 2000
  codestreams at 2480×3508, and a face covering Devanagari — and the one
  encoder the benches own, LZW.
- **The CLI-marked operations**, timed by running this revision's `tpdf`.
- **`baseline.json`'s per-group lists**, criterion's sampling per group,
  how long a run of the set takes, the 20–300 ms window
  and how it is checked, where the two level-6 byte counts live, how the seven
  retire, and the operations each later row adds.

### Non-goals

- **A gate on a push or a pull request.** The set runs locally, as a change's
  proof, and CI's tests never run it; MS-02's paired run is the gate.
- **An input changed for any reason but the window.** An operation's input
  is the one that brings it inside 20–300 ms on the hosted runner, and tier
  P's table states its figures rescaled to that input; nothing else moves an
  input.
- **Floors, calibrations and relations.** What an operation is lives here;
  how it gates is MS-02.3's to commit beside it, and the thread floors are
  MS-09's, relations held inside one run (`docs/verification.md`).
- **Memory and wasm.** MS-05's probe, `tests/peak_memory.rs`, builds its
  cases from these inputs and keeps its own bands. MS-10.1 builds this set for
  `wasm32-wasip1` and runs it under a wasm runtime, less the operations
  `benches/wasm.json` lists as unable to run there — a child, a thread — so
  every file here compiles for that target and reads its committed inputs
  under the path `TINKER_BENCH_CRATE_DIR` gives the crate, which the
  runtime's directory grant reaches (`docs/verification.md`, "The wasm
  legs").
- **A number an outside program measures.** The level-6 byte counts are
  brought in once (ruling 17), and no outside program runs while the set does
  (ruling 13): a fixture an outside program made once is bytes with its
  provenance.
- **A capability the tree lacks.** Its row adds its operation, in the commit
  that adds the capability (the table under "What later rows add").

### Design

#### Files

```text
crates/tinker-pdf/benches/
  support/mod.rs     the harness: each group's sampling, the iteration shapes,
                     what an operation declares, and count mode
  support/inputs.rs  every generator, and the store every input is read
                     through
  support/samples.rs the raw samples, which write no document, and the two
                     level-6 byte counts beside them; inputs.rs re-exports it
  support/list.rs    the reader of baseline.json's lists
  support/cli.rs     the one spawner: this revision's tpdf, as a child
  open.rs  text.rs  analysis.rs  render.rs  encode.rs  decode.rs  write.rs
  sign.rs  layout.rs  fonts.rs  threads.rs  scripts.rs  ocr.rs
  calibration.rs
  data/              the inputs the tree cannot write, and README.md
  baseline.json
```

One `[[bench]]` per group file, `harness = false`. `inputs.rs`
holds no criterion type, no clock and no spawn, so a test reaches it by path
alone — MS-05's probe, `tests/peak_memory.rs`, for its cases, and
`tests/bench_inputs.rs` for the store — without compiling the harness or the
child, and `samples.rs` holds no generator that writes a document, so
`tests/level_six_counts.rs` and P-04.4's `deflate_size.rs` reach the level-6
counts through it alone; `list.rs` holds none either, and that test reaches it
the same way. `cli.rs` is the only file here that calls `Command::new`, so it is the
one `SPAWNERS` row `cargo xtask oracles` needs (`xtask/src/main.rs:984-1048`,
`:1076-1079`). Every input is generated in memory except what `data/` and
the tree already hold, and each group file takes every input through the
store: with `TINKER_BENCH_FIXTURES` naming a directory, each is read from
the file of its name there and written there first when the directory holds
none, which is how a paired run hands its three builds the same bytes
(MS-02, "The inputs both sides are timed on"); without it, an input is
generated in memory, and a file a CLI operation's child reads is written
into `CARGO_TARGET_TMPDIR`'s `bench-fixtures/`, a directory cargo gives
every bench target, each time the operation is defined.

The generators call only what `61adcde` already exposes — the builder calls
used below (`archival`, `tagged`, `output_intent`, `add_embedded_font`,
`add_base_font`, `add_image`, `add_icc_color_space`, `add_tiling_pattern`,
`set_info`, `raw`), the encoders (`jpeg_encode`, `ccitt_g4_encode`,
`jbig2_generic_encode`, `png_encode`, `deflate`, `zlib_compress`),
`BilevelCodec::Jbig2Generic`, `add_annotation`, `annot::square`, `FromHtml`
and `Document::open_markdown` are all public at that revision — so MS-02.4's
mapping onto the planning baseline covers operation bodies only, and the one
`Bitmap` the store builds by hand from a render's stored pixels, which at
that revision carries no statistics.
`PageBuilder::text_pieces` is not in `61adcde`, which is why producer-style
lines are
written through `raw`.

#### An operation: its id, its unit and its iteration

**The id** is criterion's, `<group>/<name>`: the group file opens its group,
`Group::new(c, "<group>")`, named for its group, and names each operation in
it through `Group::operation`.
`bench.rs` keys every estimate by the directory criterion wrote it under
(`xtask/src/bench.rs:1013-1044`), and criterion 0.5.1 makes each part of that
path filename-safe — it replaces `?`, `"`, `/`, `\`, `*`, `<`, `>`, `:`, `|`
and `^` with `_` and cuts the part at 64 bytes — so a name that breaks either
rule is recorded under another key, reported "new, with no recorded figure"
and never held to its band. Every name is therefore ASCII, at most 64 bytes,
free of those ten characters (PDF-A, not PDF/A; 9-7 is spelled irreversible),
and writes its numbers unspaced ("3000 pages"). A name says what the input is
and never how often it is repeated. On Windows criterion also trims each
part at its end and lowercases it (`criterion-0.5.1/src/report.rs:92-104`),
and `bench-check` reads each such directory, in a plain run and in a pair's
rounds, as the listed id it was made from (`xtask/src/bench.rs`,
`listed_id`).

**The unit** is what an iteration processes: pages, pixels (printed as
Mpixel/s), bytes (MB/s, a megabyte being 10⁶ bytes), outlines (tier P states
P-26's CFF target in outlines a second), or none, for an operation whose input
is none of these — a private-key operation, a shaped paragraph — which
reports milliseconds alone. Criterion keeps a group's throughput from one
operation to the next and has no call that clears it, so a group declares its
unitless operations first: the AES-256 open opens the open group, and
`Group::operation` refuses an operation with no unit after one with a unit. A render's pixels are its bitmap's; a decode's
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
- **Over prebuilt state**, built once before the loop, the first time an
  operation reads it, when the operation's name says it starts from it: prebuilt text, a recorded display list, a
  rendered bitmap, a decoder's input. The second render and the re-rendered
  annotation build theirs per iteration, untimed, through criterion's
  `iter_batched`.
- **Repeated.** An operation whose head figure would sit under 20 ms, today or
  at its target, runs `calls` times an iteration, the same calls on both sides
  of a pair, so a paired ratio does not move; `calls` is chosen below and
  confirmed at MS-04.10. A relation compares per-call figures, an iteration's
  mean over its `calls`, so each side of one keeps its own.
- **A child.** A CLI operation runs `tpdf` once a call (below).

An operation is defined — what it reads built and checked, its unit and its
routine declared — the first time criterion calls its routine, outside the
time criterion takes, and what it built is kept for the rest of the process;
an operation criterion leaves out is never defined, and what its routine's
last run wrote is checked after its runs. An input several operations read
is built in a `LazyCell` the first time a defined operation reads it, and
checked there. So criterion, which runs every operation, builds every input;
a process started for one, `--exact <id>`, builds that one's; `--list` builds
nothing; and count mode builds only what its one operation reads.

#### Count mode

A bench binary started with `TINKER_BENCH_COUNT=<group>/<name>` and
`TINKER_BENCH_ITERATIONS=<n>` runs that operation's routine exactly `n`
times outside criterion's sampler, prints
`count-mode: RAN <group>/<name> <n>` and exits; an operation that is none of
its group's, or an iteration count missing or no number, fails by name.
Criterion's sampler chooses its iteration count from the clock, so a count of
an ordinary run would count a different amount of work each time; count mode
reads no clock, so the instructions the process executes are a property of
the code and the input, and a run of `n + 1` less a run of `n` is one
iteration's, start-up and the inputs' construction cancelled. An operation
whose iterations each take an untimed input (`iter_batched`) counts that
setup with it, since nothing outside the process can tell their instructions
apart. Every operation is declared through `Group::operation`, which runs it
either way, and `cargo xtask baseline` refuses a group file that calls
criterion's `bench_function` or `benchmark_group` itself. `calibration.rs` is
the exception: frozen, it keeps criterion's calls and has no count mode, and
its input and call are `text/text of 5 dense pages`, which count mode runs.

#### The inputs

All but the committed ones are written by `support/inputs.rs`, by formula
where the bytes are pinned (seeded integer arithmetic, no float, so every
target computes the same bytes) and through the tree's own writers elsewhere.

| Input | What it is | How it is written | Tier P's input |
| --- | --- | --- | --- |
| `dense(n, seed)` | n A4 pages (595 × 842 pt), each about 5 600 characters of seeded words in 70 lines, drawn from about 80 distinct glyphs with a few accented letters | Liberation Serif Regular (`crates/tinker-pdf-font/data/liberation/`) embedded whole — `add_embedded_font` (`crates/tinker-pdf-cos/src/build.rs:5694`) with `set_subset_fonts(false)` (`:6795`), so subset-on-save has a program to cut | "5 dense pages", "the text page" (`dense(1, 0)`); 5 600 characters a page is P-02's ≥ 5.6 M characters/s at 1 000 pages/s |
| `dense_part(n, seed, d)` | n pages a d-th of A4 each way (`page_part(d)`), each holding what of its dense page's text fits the same type there: its first 70 / d lines, each its words within 78 / d characters, at `dense`'s size and leading, the margins a d-th of its, so a page draws about a d²-th of the glyphs and the pixels a dense page does | as `dense` | "half-size text pages", "quarter-size pages" |
| `producer(n)` | the dense pages' text in standard-14 Helvetica, unembedded, one `TJ` a line with kerning numbers | `add_base_font` (`build.rs:5561`) and `PageBuilder::raw` (`:4125`) | the Item's "100 producer-style standard-14 pages" |
| `small(n)` | n pages of one Helvetica line each, the line `three_pages()` writes | the builder's flat page tree, one `/Kids` of every page (`build.rs:8508-8516`) | "small pages", "10 000-page document", "4 000 small pages", "rewrite 100 / 1 000 / 3 000 pages" |
| `three_pages()` | the document `open/open a 3-page document` opens | `support/inputs.rs` | — |
| `report(n)` | n A4 pages of two columns of body text in the embedded face, a running head and foot, a page number and one ruled table each; untagged | the builder | "5 text pages", "5 pages" of the analysis rows |
| `claiming(n)` | `report(n)` tagged (`PageBuilder::tagged`, `build.rs:2980`) under `DocumentBuilder::archival` (`:9047`), claiming PDF/A-2b, PDF/UA-1 and PDF/X-1a:2003 (`set_info`'s `GTS_PDFXVersion`, `:7744`; an output intent, `:3306`, carrying `fuzz/corpus/icc_profile/cmyk-lut.icc`), so each validator runs its rules rather than reporting no claim | the builder | "5 pages" of the validation row |
| `photo(w, h, seed)` | RGB samples by formula: smooth fields, hard edges and a few levels of seeded noise | `samples.rs` | the photographic content of every image input |
| `page_like()` | 1240×1755 RGB (6 528 600 bytes) of text-like marks on white, about a tenth of the pixels inked, by formula — never a render, so it cannot move with the renderer | `samples.rs`; its length and SHA-256 pinned beside the level-6 counts | "page-like data" |
| `photo_like()` | `photo(1240, 1755, 0)`, 6 528 600 bytes, pinned the same way | `samples.rs` | "photo-like data" |
| `noise()` | 6 528 600 seeded bytes whose deflate stream is between 90 % and 100 % of their length, so inflate decodes coded literals, not stored blocks (asserted) | `inputs.rs` | "incompressible data" |
| `lzw(bytes)` | 7.4.4's LZW, 9- to 12-bit codes, early change 1, a clear code at 4 096 | an encoder in `inputs.rs`, about sixty lines, round-tripped through `lzw_decode` (`crates/tinker-pdf-filters/src/lib.rs:821`) whenever it runs | LZW-coded page-like data; the filters crate's test encoder packs the codes it is given and builds no dictionary from its input (`crates/tinker-pdf-filters/src/tiff/tests.rs:397`) |
| `baseline_jpeg()`, `baseline_jpeg_part(d)` | `photo(2480, 3508, 0)` as a baseline JPEG, Annex K's tables; and `photo` at `scan_part(d)`'s size, 2480×3508 a d-th each way, the same way | `jpeg_encode` (`crates/tinker-pdf-filters/src/jpeg/encode.rs:819`) | "JPEG decode 1240×1754, baseline" (`baseline_jpeg_part(2)`); the JPEG page `tests/peak_memory.rs` holds the peak of (`baseline_jpeg()`) |
| `scan()`, `scan_at(dpi)` | `dense(1, 0)`'s page rendered at 300 dpi, or at `dpi`, and thresholded to one bit | the renderer | "G4 / JBIG2 scan page" |
| `image_file()` | as many A4 pages as reach 23 000 000 bytes, each covered by its own image, `photo(2480, 3508, page)` DCT-coded, so no two pages share an image a writer could deduplicate | `jpeg_encode`, `add_image` (`build.rs:7206`) | "a 23 MB file", "a 23 MB image file" |
| `small_image_file()` | the first page of `image_file()`'s pages alone, 2 496 733 bytes | the same | "a 2.5 MB image file", and the CBC pair's bytes |
| `aes_file()` | `image_file()` saved at R6 with both passwords empty and fixed entropy, the file `tpdf encrypt` writes with neither password | `WriteOptions::encryption` (`crates/tinker-pdf-cos/src/write.rs:30-55`, `:124`) | the Item's "an AES-256 file" |
| `jpeg_page()`, `jpeg_page_part(d)`, `flate_page(d)`, `cmyk_page(d)` | a page a d-th of A4 each way (`jpeg_page()` A4 itself) covered by one image at `scan_part(d)`'s size: `baseline_jpeg()` or `baseline_jpeg_part(d)`; `photo` at that size, seed 1, through `zlib_compress` (`deflate.rs:223`); `photo` converted to CMYK by formula and Flate-coded under `/ICCBased` with `cmyk-lut.icc` (`add_icc_color_space`, `build.rs:6356`) | `add_image` | "half-size JPEG page", "quarter-size JPEG page", "Flate RGB image page", "an ICC-based CMYK image"; the CMYK image is not DCT-coded, so its operation times the conversion and no JPEG decode; `jpeg_page()` is the peak test's |
| `jpx_page()`, `jpx_page_part()` | an A4 page covered by `data/`'s 2480×3508 irreversible codestream, the peak test's; and a page a quarter of A4 each way covered by its 620×877 one | `ImageFilter::Jpx` (`build.rs:438`) | "full-page JPEG 2000" |
| `g4_page(d)`, `jbig2_page(d)` | a page a d-th of A4 each way covered by `scan_at(300 / d)`: G4-coded (`ccitt_g4_encode`, `ccitt/encode.rs:386`, as `ImageFilter::CcittFax`); and the same page saved through the image pass's `BilevelCodec::Jbig2Generic` (`crates/tinker-pdf/src/recode.rs:188-197`), the builder having no JBIG2 filter | the builder, then `write::save` | "G4 / JBIG2 scan page" |
| `paths(n)`, `curves(n)` | three in five of n filled polygons and closed curves at fractional coordinates, as `triangles()` draws them, and two in five stroked cubic curves, 0.5 to 4 pt wide, each of the nine cap-and-join pairs in turn, every fifth dashed; `curves(n)` is n such curves alone | `raw` | "250 paths", "100 paths", "the 250-path page's 100 stroked curves alone" |
| `triangles()`, `axial()`, `axial_part(d)` | the render group's 300 triangles and full-page axial shading, and the same shading over a page a d-th of A4 each way | `support/inputs.rs` | "300 triangles", "axial shading" |
| `tiling(fills)` | one tiling pattern (`add_tiling_pattern`, `build.rs:6276`) filling an A4 page in one rectangle, or in 50 strips that together cover what the one covers | the builder | "a page filled by one tiling pattern 50 times / once" |
| `annotated(base)` | `base` with one 100×100 pt square annotation on its first page, appearance included (`DocumentEditor::add_annotation`, `crates/tinker-pdf-cos/src/edit/annotations.rs:214`; `annot::square`, `edit/annot.rs:84`) | the editor | "a changed 100×100 pt annotation", "a 1 000-page file", "a 10-page one" |
| `book(chapters)` | an EPUB of a heading and one paragraph of about 600 characters a chapter, each chapter a page at the default box, Liberation Serif embedded through `@font-face` in the one stylesheet every chapter links so its glyphs are drawn in both builds | `tests/epub_support`'s OCF writer (`crates/tinker-pdf/tests/epub_support/mod.rs:543`), by path | "a generated 30-chapter book", "10 chapters" |
| `html_report()`, `markdown()` | 20 pages each at the default page box. The report is 20 sections, each a heading, two paragraphs and a ruled table of six rows on a page of its own; the Markdown is 20 sections, each a heading, three paragraphs and a list of five items, in one flow, CommonMark having no table | `FromHtml` (`crates/tinker-pdf/src/html.rs:181`); CommonMark for `Document::open_markdown` (`crates/tinker-pdf/src/lib.rs:1472`) | "a generated 20-page HTML report / 20-page Markdown document" |
| `message()` | 10 000 000 seeded bytes, and their RSA-2048 PKCS #1 v1.5 signature made in the bench with `pss-vect.txt`'s Example 10 key (`crates/tinker-pdf-crypto/tests/data/pkcs1/pss-vect.txt`) through `Modulus::pow` (`crates/tinker-pdf-crypto/src/bignum.rs:468`) | `inputs.rs` | "over 10 MB" |
| committed | the nine books (`crates/tinker-pdf/tests/epub/`); the mesh and transparency fixtures (`fuzz/corpus/render_page/`); every `.svg` in `crates/tinker-pdf-svg/tests/fixtures/` (17), every `.xps` and `.oxps` under `crates/tinker-pdf/tests/` (19) and every `.cbz` in `crates/tinker-pdf/tests/cbz/` (9); the three Liberation Regular faces; the four bare CFF programs in `fuzz/corpus/cff/` and the `CFF ` tables of the 189 committed OpenType files (185 in `crates/tinker-pdf-shape/data/aots/fonts/`, three text-rendering-tests faces and `fuzz/corpus/sfnt/opentype-cff.otf`); `TestShapeAran.ttf`; `data/`'s four codestreams; the vendored Devanagari face | read by path | as named |

#### The operations

Each group's table gives the operation's id, its unit and what one
iteration covers, its input, tier P's figure and gate, where it stands
against the window, and the step that adds it. Tier P's figures are the
planning machine's at `61adcde` (`docs/roadmap/P.md:22-28`), in milliseconds
unless marked, rescaled where an operation's input is smaller than tier P's,
as tier P's table rescales them; "twin" is an in-process operation the table
marks only for the CLI, which sits at floor 1 (MS-02.3's note).

**Window**, on the hosted runner, as MS-04.9's report-mode dispatch measured
each operation and MS-04.11 sized it: *in* — one call inside 20–300 ms; *×n*
— `calls` n, as many calls as bring a shorter call near the window's middle,
about 75 ms; one over the window takes a smaller input instead — fewer pages,
chapters or paths, or a part of A4 each way with its text, image or shading —
chosen the same way. *The seven's* — one of the seven, which keep one call
and the figures their fifteen dispatches recorded until MS-04.10 retires them
(below); two of them, `text/extract a page of text` and
`layout/paginate a book at 432x648 pt`, are MS-24's besides.

**open**

| Id | Unit · an iteration | Input | Tier P: today → target; gate | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `open/open a 3-page document` | bytes · the file a call | `three_pages()` | —; floor 1 | the seven's | built |
| `open/open and count the 23 MB image file` | bytes · 100 × the file | `image_file()` | twin; floor 1 | ×100 | built |
| `open/open the AES-256 file` | none · 6 calls | `aes_file()`, authenticated with the empty password (`lib.rs:1722`) and counted | the Item's; floor 1 | ×6 | built |
| `open/every page's size, 3000 pages` | pages · 12 × 3 000 | `small(3000)`, `page(i)` then `Page::size` (`lib.rs:1773`, `:2198`) for every i | 2 569 → 3; 856× | ×12 | built |
| `open/every page's size, 10000 pages` | pages · 4 × 10 000 | `small(10000)` | Θ(n²), about 28 500 from the 3 000-page figure → 10; relation ≤ 3.6 × 3 000 | ×4 | built |
| `open/open and count the 23 MB image file, CLI` | bytes · 24 files | `tpdf info` | 36 → 6; 6× | ×24 | built |
| `open/open a small file, CLI` | bytes · 10 files | `testdata/simple-text.pdf`, `tpdf info`, its page count read back | 4–5 → 5; held | ×10 | built |

**text**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `text/text of 5 dense pages` | pages · 5 | `dense(5, 0)`, every page's `text().plain_text()` by index, as `tpdf text` reads them (`tools/tpdf/src/main.rs:973-991`) | 37.6 → 5, rescaled from 100 pages' 751 → 100; 7.51× | in | built |
| `text/text of 20 standard-14 pages` | pages · 20 | `producer(20)` | the Item's; floor 1 | in | built |
| `text/text of 1000 small pages` | pages · 2 × 1 000 | `small(1000)` | twin (CLI 350); floor 1 | ×2 | built |
| `text/text of 4000 small pages` | pages · 4 000 | `small(4000)` | twin (CLI Θ(n²): 36 000 at 10 000 pages); floor 1 | in | built |
| `text/structured JSON of 5 pages` | pages · 5 | `dense(5, 0)` through `TextWriter`'s JSON, as `tpdf text --json` (`main.rs:962-972`) | twin (CLI 96.3, rescaled from 100 pages' 1 926); floor 1 | in | built |
| `text/search a phrase over 5 pages from scratch` | pages · 5 | `dense(5, 0)`, `TextPage::search` (`crates/tinker-pdf-content/src/text.rs:265`) | 36.1 → 5.5, rescaled from 100 pages' 722 → 110; 6.56× | in | built |
| `text/search 100 pages' prebuilt text` | pages · 500 | the 100 `TextPage`s, built once | 36 → 6; 6× | ×5 | built |
| `text/search 100 pages' prebuilt text, whole words, unaccented` | pages · 200 | the same, `search_with` whole-word and diacritic-insensitive (`search.rs:43-50`, `:108`) | 77 → 12; 6.41× | ×2 | built |
| `text/plain prebuilt search, folding and joining` | pages · 500 | the 100 `TextPage`s, each line's glyph texts lower-cased one by one and joined, and the needle, as `TextPage::search` does each call (`crates/tinker-pdf-content/src/text.rs:281`) | not in tier P's table; floor 1 | untimed | built |
| `text/plain prebuilt search, find and its restarts` | pages · 500 | the same lines, folded and joined once: `str::find` from each restart | not in tier P's table; floor 1 | untimed | built |
| `text/plain prebuilt search, mapping hits to glyphs` | pages · 500 | the same lines' hits, each walked back to its glyphs and their quad | not in tier P's table; floor 1 | untimed | built |
| `text/unaccented whole-word search, folding and joining` | pages · 200 | the 100 `TextPage`s, folded as `search_with` folds them (`search.rs:83`): lower-cased, then `fold_diacritics` (`:59`) | not in tier P's table; floor 1 | untimed | built |
| `text/unaccented whole-word search, UAX 29 word boundaries` | pages · 200 | the same lines, folded and joined once: `word_boundaries` (`crates/tinker-pdf-content/src/words.rs:158`) | not in tier P's table; floor 1 | untimed | built |
| `text/unaccented whole-word search, find and its restarts` | pages · 200 | the same lines and their boundaries: `str::find` from each restart, a candidate off a boundary resuming one character on | not in tier P's table; floor 1 | untimed | built |
| `text/unaccented whole-word search, mapping hits to glyphs` | pages · 200 | the same lines' hits, each walked back to its glyphs and their quad | not in tier P's table; floor 1 | untimed | built |
| `text/text of 5 dense pages, CLI` | pages · 5 | `tpdf text` | 33.9 → 6.5, rescaled from 100 pages' 678 → 130; 5.21× | in | built |
| `text/text of 1000 small pages, CLI` | pages · 2 × 1 000 | `tpdf text` | 350 → 15; 23.3× | ×2 | built |
| `text/text of 4000 small pages, CLI` | pages · 4 000 | `tpdf text` | Θ(n²) (36 000 at 10 000 pages) → 60, rescaled from 10 000 pages' 150; relation ≤ 4.4 × 1 000 | in | built |
| `text/structured JSON of 5 pages, CLI` | pages · 5 | `tpdf text --json` | 96.3 → 10, rescaled from 100 pages' 1 926 → 200; 9.63× | in | built |

The seven phase operations beside the two prebuilt-text searches are P-24's
profile of them (`docs/verification.md`, "Where prebuilt-text search spends
its time"): each makes its search's calls over its pages, so that
`cargo xtask search-split` can hold the phases together to the search. No run
has timed them, so the named machine's entry names the seven unmeasured, and
their window is *untimed*: MS-04.10's fifteen runs size them as they size the
set, and the split divides each by its own `calls`.

**analysis** — every relation's "text of the same pages" is the group's own
text of the document the operation reads, so all six stay inside the group.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `analysis/text of the 5 report pages` | pages · 5 | `report(5)` | the relations' denominator; floor 1 | in | built |
| `analysis/redact a rectangle on each of 5 pages` | pages · 14 × 5 | `report(5)`, `redact::apply` on every page of one editor a call (`crates/tinker-pdf/src/redact.rs:1106`), unsaved: saving is the write group's | not measured; ≤ 1.5 × text | ×14 | built |
| `analysis/reading order inferred over 5 pages` | pages · 5 | `report(5)`, `Document::inferred_orders` (`crates/tinker-pdf/src/reading_order.rs`); the document is untagged, so the default options infer (`InferenceOptions`'s `Default`) | not measured; ≤ 1.5 × text | in | built |
| `analysis/tables inferred over 5 pages` | pages · 5 | `report(5)`, `Page::tables(TableSource::Inferred)` (`crates/tinker-pdf/src/tables.rs:448`) | not measured; ≤ 1.5 × text | in | built |
| `analysis/text of the 5 claiming pages` | pages · 5 | `claiming(5)` | the validators' denominator; floor 1 | in | built |
| `analysis/PDF-A validation of 5 pages` | pages · 10 × 5 | `claiming(5)`, `validate_pdfa` (`lib.rs:1990`) | not measured; ≤ 3 × text | ×10 | built |
| `analysis/PDF-UA validation of 5 pages` | pages · 11 × 5 | `validate_pdfua` (`:2020`) | not measured; ≤ 3 × text | ×11 | built |
| `analysis/PDF-X validation of 5 pages` | pages · 22 × 5 | `validate_pdfx` (`:2043`) | not measured; ≤ 3 × text | ×22 | built |

**render** — a page render is `Page::render` (`lib.rs:2247`) at
`RenderOptions::at_dpi` (`:724`); "the text page" is `dense(1, 0)`'s page; a
*half-size* page is half of A4 each way and a *quarter-size* one a quarter
(`page_part`), its text, image or shading with it.

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `render/the text page at 72 dpi` | pixels · 2 × 595×842 | `dense(1, 0)` | 54 → 6; 9× | ×2 | built |
| `render/the text page at 150 dpi` | pixels · 2 × 1240×1755 | `dense(1, 0)` | 88 → 15; 5.86× | ×2 | built |
| `render/the text page at 300 dpi` | pixels · 2480×3509 | `dense(1, 0)` | 171 → 35; 4.88× | in | built |
| `render/record the text page's display list` | pages · 5 | `Page::display_list` (`crates/tinker-pdf/src/display.rs:129`) | 6.3 → 1.2; 5.25× | ×5 | built |
| `render/the 150-dpi text page scaled to a half` | pixels · 1240×1755, the render's | the text page's 150-dpi render, drawn once untimed, `Bitmap::scaled` to 620×877; the planning baseline renders the page in its place | not in tier P's table; floor 1, paired with the baseline's `the text page at 150 dpi`; a target is the owner's (H-77) | untimed | built |
| `render/the 150-dpi text page scaled to two thirds` | pixels · 1240×1755, the render's | the same, scaled to 826×1170 | the same | untimed | built |
| `render/the text page at 150 dpi in forced colours, fill to stroke` | pixels · 2 × 1240×1755 | `dense(1, 0)`, `RenderOptions::forced_colours` black on white with `fill_to_stroke`; the planning baseline draws the default render in its place | the same | untimed | built |
| `render/the text page at 150 dpi at a 4-bit text level` | pixels · 2 × 1240×1755 | `dense(1, 0)`, `RenderOptions::text_bits` 4; the planning baseline draws the default render in its place | not in tier P's table; floor 1, paired with the baseline's `the text page at 150 dpi`; a target is the owner's (H-77) | untimed | built |
| `render/250 paths at 150 dpi` | pixels · 1240×1755 | `paths(250)` | 66.3 → 22.5, rescaled from 5 000 paths' 1 326 → 450; 2.94× | in | built |
| `render/250 paths at 150 dpi under the centre rule` | pixels · 1240×1755 | `paths(250)`, `graphics_bits` 0 under `HardEdge::Centre`; the planning baseline draws the default render in its place | not in tier P's table; floor 1, paired with the baseline's `250 paths at 150 dpi`; a target is the owner's (H-77) | untimed | built |
| `render/250 paths at 150 dpi under the any-part rule` | pixels · 1240×1755 | the same under `HardEdge::AnyPart`, whose walk of each edge over every row it reaches is the one new cost | the same | untimed | built |
| `render/250 paths at 150 dpi at a 4-bit graphics level` | pixels · 1240×1755 | `paths(250)`, `graphics_bits` 4 | the same | untimed | built |
| `render/100 paths at 300 dpi` | pixels · 2480×3509 | `paths(100)` | 68.7 → 22, rescaled from 5 000 paths' 3 434 → 1 100; 3.12× | in | built |
| `render/the 100 stroked curves alone at 150 dpi` | pixels · 1240×1755 | `curves(100)`, the 250-path page's curves | not measured apart; as the page; 2.94×, paired with the baseline's same curves | in | built |
| `render/300 triangles at 150 dpi` | pixels · 2 × 1240×1755 | `triangles()` | 44 → 20; 2.2× | ×2 | MS-04.10 renames the seven's |
| `render/a full-page axial shading at 150 dpi` | pixels · 1240×1755 | `axial()` | 124–170 → 25; 4.96× | in | MS-04.10 renames the seven's |
| `render/an axial shading over a half-size page at 300 dpi` | pixels · 1240×1755 | `axial_part(2)` | 168.5 → 22.5, rescaled from A4's 674 → 90; 7.48× | in | built |
| `render/the mesh shadings fixture at 150 dpi` | pixels · 7 bitmaps | `fuzz/corpus/render_page/mesh-shadings.pdf` | 18.1 → 4; 4.52× | ×7 | built |
| `render/the mesh shadings fixture at 300 dpi` | pixels · 3 bitmaps | the same | 43.1 → 10; 4.31× | ×3 | built |
| `render/the transparency fixture at 150 dpi` | pixels · 9 bitmaps | `fuzz/corpus/render_page/transparency.pdf` | 8.4 → 3; 2.8× | ×9 | built |
| `render/a page tiled by one pattern fill at 150 dpi` | pixels · 1240×1755 a call | `tiling(1)` | not measured; relation: 50 fills ≤ 1.2 × one; counted | in | built |
| `render/a page tiled by 50 pattern fills at 150 dpi` | pixels · 1240×1755 a call | `tiling(50)` | the same relation | in | built |
| `render/the 300-dpi text page in 4 bands on 4 threads` | pixels · 2480×3509 | `dense(1, 0)`, four `RenderOptions::region` bands (`lib.rs:584`), one scoped thread each | relation ≥ 3 × the 1-thread speed (`the text page at 300 dpi`); MS-09's floor inside one run ≥ 2.6 × it | in | built |
| `render/8 half-size text pages at 150 dpi on 1 thread` | pages · 8 | `dense_part(8, 0, 2)` | 45.6 → 240 pages/s, rescaled from A4's 11.4 → 60; 5.26× | in | built |
| `render/8 half-size text pages at 150 dpi on 4 threads` | pages · 8 | the same, on `std::thread::scope` over owned pages, as `crates/tinker-pdf/examples/parallel.rs:87` does | 172 → 800 pages/s, rescaled from A4's 43 → 200; 4.65×; relation ≥ 3.33 × 1 thread | in | built |
| `render/the annotated text page at 150 dpi` | pixels · 2 × 1240×1755 | `annotated(dense(1, 0))` | the next one's full-page render; floor 1 | ×2 | built |
| `render/re-render a changed 100x100 pt annotation` | pixels · 1240×1755 | the same, its appearance replaced and appended through `write::append` untimed beside a copy of the full render's bitmap; the patch of that bitmap (`Appended::patch`) is timed; the planning baseline saves the change under `WriteMode::Incremental`, reopens the bytes and draws the changed page whole in its place | relation ≤ 10 % of the full-page render | in | built |
| `render/replay the half-size Flate RGB image page at 150 dpi` | pixels · 620×878 | `flate_page(2)`'s display list, recorded once; its renders share the decoded image (`resources.rs:532-546`) | 87.5 → 10, rescaled from A4's 350 → 40; 8.75× | in | built |
| `render/a quarter-size ICC-based CMYK image to sRGB at 300 dpi` | pixels · 620×877, the image's | `cmyk_page(4)`, drawn at the image's own resolution so each sample is converted once | not measured → ≥ 100 Mpixel/s (≤ 5.4); calibrated | in | built |
| `render/render an ICC page under each rendering intent at 150 dpi` | pixels · 1240×1755 | `intents_page()`: in each of four columns, under one of Table 69's intents by `ri`, a fill, `photo(310, 438, 0)` and an axial shading in an `ICCBased` space over a profile carrying a table for each intent on a media white other than D50 (RC-01); held before it is timed to paint each column's fill, image and shading in four colours; the planning baseline, which reads no intent, draws the same page | not in tier P's table; floor 1 | untimed | built |
| `render/the half-size JPEG page at 72 dpi` | pixels · 298×421 | `jpeg_page_part(2)` | 163 → 8.75, rescaled from A4's 652 → 35; 18.6× | in | built |
| `render/the quarter-size JPEG page at 300 dpi` | pixels · 620×878 | `jpeg_page_part(4)` | 99.1 → 9.4, rescaled from A4's 1 585 → 150; 10.5× | in | built |
| `render/a second render of the quarter-size JPEG page at 300 dpi under a 64 MiB budget` | pixels · 620×878 | `jpeg_page_part(4)` opened under a 64 MiB budget stated at open (`CacheLimits`, P-16), after the same `Page` rendered at 150 dpi, untimed: at both resolutions its JPEG is drawn larger than half its samples, so both renders ask its full-size decode and the second is served by the document's cache; held before it is timed to decode no image, miss nothing the document keeps and draw a first 300-dpi render's bytes; the planning baseline, which keeps nothing past a render, opens the page as it is | not measured; relation ≤ 30 % of the first (`the quarter-size JPEG page at 300 dpi`) | in | built |
| `render/the quarter-size JPEG 2000 page at 72 dpi` | pixels · 149×211 | `jpx_page_part()` | 172 → 15.6, rescaled from A4's 2 753 → 250; 11× | in | built |
| `render/the half-size G4 scan page at 150 dpi` | pixels · 620×878 | `g4_page(2)` | 160 → 12.5, rescaled from A4's 641 → 50; 12.8× | in | built |
| `render/the half-size JBIG2 scan page at 150 dpi` | pixels · 620×878 | `jbig2_page(2)` | 219 → 20, rescaled from A4's 875 → 80; 10.9× | in | built |
| `render/the quarter-size Flate RGB image page at 72 dpi` | pixels · 149×211 | `flate_page(4)` | 57.8 → 14.4, rescaled from A4's 924 → 230; 4.01× | in | built |
| `render/the half-size text page to PNG at 150 dpi, CLI` | pixels · 620×878 | `tpdf render` | 100.5 → 17.5, rescaled from A4's 402 → 70; 5.74× | in | built |
| `render/the quarter-size text page to PNG at 300 dpi, CLI` | pixels · 620×878 | `tpdf render` | 73.8 → 12.5, rescaled from A4's 1 180 → 200; 5.9× | in | built |
| `render/8 quarter-size pages to PNG at 150 dpi, 1 job, CLI` | pages · 8 | `tpdf render --jobs 1` | 171 → 32.5, rescaled from 20 A4 pages' 6 847 → 1 300; 5.26× | in | built |
| `render/8 quarter-size pages to PNG at 150 dpi, 4 jobs, CLI` | pages · 8 | `tpdf render --jobs 4` | 43.6 → 10, rescaled from 20 A4 pages' 1 743 → 400; 4.35× | in | built |

The bitmap toolbox's two scales and the forced-colour render (RC-09.7a,
`docs/design/bitmap-toolbox.md`) are the toolbox's operations with real cost;
its per-pixel maps are one pass over the bytes and have no operation of their
own. Before its runs each holds its result to its size and its ink, the
forced render to its grey. No run has timed them, so the named machine's entry
names all three unmeasured and their window is *untimed*: RC-09.7b takes their
figures, the first paired ratios among them, in the timing phase.

**encode**

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `encode/PNG of the 1240x1755 text page` | pixels · 1240×1755 | the text page's 150-dpi bitmap, rendered once, `Bitmap::to_png` (`lib.rs:861`) | 269 → 40; 6.72× | in | built |
| `encode/PNG of the near-blank page` | pixels · 2 × 1240×1755 | a 150-dpi bitmap of a page holding one line | 145 → 15; 9.66× | ×2 | built |
| `encode/deflate page-like data` | bytes · 6 528 600 | `page_like()`, `deflate_at` at `DeflateLevel::Ratio` (`deflate.rs:1297`); the planning baseline runs `deflate` | 40 → 120 MB/s (163 → 54); 3×; size ≤ 1.02 × level 6's, exact | in | built |
| `encode/deflate photo-like data` | bytes · 6 528 600 | `photo_like()`, the same | 28 → 40 MB/s (233 → 163); 1.4×; size likewise | in | built |
| `encode/deflate page-like data, fast level` | bytes · 6 528 600 | `page_like()`, `deflate_at` at `DeflateLevel::Fast` (`deflate.rs:1297`); the planning baseline runs `deflate` in its place | ≥ 300 MB/s; 7.5×, paired with the baseline's deflate, at floor 1 until P-04.6 | untimed | built |
| `encode/deflate photo-like data, fast level` | bytes · 6 528 600 | `photo_like()`, the same | ≥ 150 MB/s; 5.27×, paired likewise, at floor 1 until P-04.6 | untimed | built |

The fast level's two operations take one call on the ratio level's inputs.
No run has timed them, so the named machine's entry names both unmeasured,
and their window is *untimed*: MS-04.10's fifteen runs size them as they
size the set.

**decode** — each through its `tinker-pdf-filters` entry point
(`crates/tinker-pdf-filters/src/lib.rs:70-93`, `:721`, `:738`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `decode/inflate page-like data` | bytes out · 6 × 6 528 600 | `deflate(page_like())` | 279 → 500 MB/s (23.4 → 13.1 a call); 1.79× | ×6 | built |
| `decode/inflate near-incompressible data` | bytes out · 3 × 6 528 600 | `deflate(noise())` | 141 → 150 MB/s (46.3 → 43.5 a call); 1.06× | ×3 | built |
| `decode/LZW decode page-like data` | bytes out · 5 × 6 528 600 | `lzw(page_like())` | not measured → ≥ 200 MB/s (≤ 32.6 a call); calibrated | ×5 | built |
| `decode/baseline JPEG 1240x1754` | pixels · 1240×1754 | `baseline_jpeg_part(2)` | 111.5 → 15, rescaled from 2480×3508's 446 → 60; 7.43× | in | built |
| `decode/progressive JPEG 1240x1754` | pixels · 1240×1754 | `data/progressive-1240x1754.jpg` | 114.5 → 30, rescaled from 2480×3508's 458 → 120; 3.81× | in | built |
| `decode/JPEG decode at quarter scale` | pixels · 2 × 1240×1754, the stream's | `baseline_jpeg_part(2)` decoded at 1/4 through `jpeg_decode_strips` and gathered into one image; the planning baseline runs `jpeg_decode` at full size in its place | not available → ≤ 5, rescaled from 2480×3508's 20; 22.3×, paired with the baseline's full-size decode, at floor 1 until P-09.11 | ×2 | built |
| `decode/irreversible JPEG 2000 620x877` | pixels · 620×877 | `data/irreversible-620x877.jp2` | 160 → 62.5, rescaled from 2480×3508's 2 566 → 1 000; 2.56× | in | built |
| `decode/reversible JPEG 2000 310x438` | pixels · 310×438 | `data/reversible-310x438.jp2` | 102 → 42.2, rescaled from 2480×3508's 6 516 → 2 700; 2.41× | in | built |
| `decode/irreversible JPEG 2000 620x877, two levels discarded` | pixels · 4 × 155×220 | `data/irreversible-620x877.jp2`, `jpx_decode_request` at a discard of 2 (`crates/tinker-pdf-filters/src/jpx/mod.rs:618`); the planning baseline runs `jpx_decode` in its place | not available → 6.3, rescaled from 2480×3508's 100; 25.6×, paired with the baseline's full decode, at floor 1 until P-10.13 | untimed | built |
| `decode/reversible JPEG 2000 310x438, two levels discarded` | pixels · 8 × 78×110 | `data/reversible-310x438.jp2`, the same | not available → 2.8, rescaled from 2480×3508's 180; 36.2×, paired likewise, at floor 1 until P-10.13 | untimed | built |
| `decode/irreversible JPEG 2000 620x877, its 310x438 centre` | pixels · 310×438 | `data/irreversible-620x877.jp2`, `jpx_decode_request` for the window at (155, 219) at the full size, held to that rectangle of the full decode; the planning baseline runs `jpx_decode` in its place | not in tier P's table; floor 1, paired with the baseline's full decode; a target is the owner's (H-77) | untimed | built |
| `decode/reversible JPEG 2000 310x438, its 155x219 centre` | pixels · 155×219 | `data/reversible-310x438.jp2`, the window at (77, 109), held to that rectangle of `photo(310, 438, 0)` | the same | untimed | built |
| `decode/irreversible JPEG 2000 620x877 on 4 threads` | pixels · 620×877 | `data/irreversible-620x877.jp2`, `jpx_decode_on` on `Parallelism::Threads(4)`, held to the one-thread decode sample for sample; the planning baseline runs `jpx_decode` on one thread in its place | 160 → 21.9, rescaled from 2480×3508's 2 566 → 350; 7.33×, paired with the baseline's one-thread decode, at floor 1 until P-10.13 | untimed | built |
| `decode/decode the JBIG2 generic page` | pixels · `scan()`'s | `jbig2_generic_encode(scan())` (`jbig2/encode.rs:251`) | 255 → 60; 4.25× | in | built |
| `decode/decode the CCITT G4 page` | pixels · 2 × `scan()`'s | `ccitt_g4_encode(scan())` | 46 → 25; 1.84× | ×2 | built |
| `decode/extract a Flate image with predictors` | pixels · 1240×1754 | `predicted_page(2)`: `photo(1240, 1754, 2)`, each row under PNG's five filters in turn, Flate-coded under predictor 15; the document opened and `Page::images` read, the samples held to the photo exactly | 276 → 280; held | in | built |

The JPEG 2000 decodes asked for less read the two codestreams the full
decodes read: two levels discarded, in four and eight calls, which bring tier
P's targets for them above 20 ms; and each image's centre quarter at the full
size, the rectangle half its width and half its height a quarter of each
dimension in from its top-left corner, in one call. The four-thread decode
reads the 9/7 codestream whole, in one call, on four threads of the engine's
pool, which a wasm target cannot start, so `wasm.json` leaves it out. No run
has timed these five, so the named machine's entry names them unmeasured, and
their window is *untimed*: MS-04.10's fifteen runs size them as they size the
set.

**write** — a rewrite is the editor's save under `WriteMode::Rewrite`, as
`write/rewrite a document`'s is (`benches/write.rs`); merge and subset-on-save go through
`write::save` (`crates/tinker-pdf/src/write.rs:337`) as `tpdf merge` does
(`tools/tpdf/src/writing.rs:132-165`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `write/rewrite 100 pages` | pages · 200 × 100 | `small(100)` | twin; floor 1 | ×200 | built |
| `write/rewrite 1000 pages` | pages · 20 × 1 000 | `small(1000)` | twin (CLI 668); floor 1 | ×20 | built |
| `write/rewrite 3000 pages` | pages · 7 × 3 000 | `small(3000)` | 5 480 → 40; 137× | ×7 | built |
| `write/rewrite 100 pages into object streams` | pages · 200 × 100 | `small(100)`, `object_streams` (`crates/tinker-pdf-cos/src/write.rs:120`) | 2.6; held | ×200 | built |
| `write/rewrite 100 pages linearized` | pages · 75 × 100 | `small(100)`, `linearize` (`:112`) | 3.7; held | ×75 | built |
| `write/subset-on-save of 4 pages` | pages · 4 | `dense(4, 0)`, `SaveOptions::default()`: a rewrite, fonts subset (`crates/tinker-pdf/src/write.rs:144-157`) | twin (in-process 52.2, rescaled from 100 pages' 1 305); floor 1 | in | built |
| `write/merge 4 x 50 pages` | pages · 200 | `dense(50, 0..3)`: pages imported with `import_page`, saved with stream deduplication and `FontPolicy::Keep` | 1 428 → 12.5, rescaled from 4 × 100's 2 855 → 25; 114× | in | built |
| `write/AES-256-CBC decrypt 2.5 MB` | bytes · the file | `small_image_file()`'s bytes, encrypted once, `cbc_decrypt_with_iv_prefix` (`crates/tinker-pdf-crypto/src/aes.rs:238`) | 32 → 150 MB/s (about 78 → 16.6); 4.68× | in | built |
| `write/AES-256-CBC encrypt 2.5 MB` | bytes · the file | the same plain bytes, `cbc_encrypt_with_iv_prefix` (`:289`) | 57 → 80 MB/s (about 43.8 → 31.2); 1.4× | in | built |
| `write/edit an annotation, save, reopen, 1000 pages` | pages · 170 × 1 000 | `annotated(small(1000))`: the appearance replaced (`DocumentEditor::put_stream`) and appended through `write::append`, the update section written and the document reopened over it; the planning baseline saves under `WriteMode::Incremental` and reopens the bytes in its place | not measured → ≤ 2 plus output bytes; relation ≤ 2 × the 10-page one | ×170 | built |
| `write/edit an annotation, save, reopen, 10 pages` | pages · 2 700 × 10 | `annotated(small(10))` | the relation's other side | ×2 700 | built |
| `write/no-op incremental save of 100 pages` | pages · 2 000 × 100 | `small(100)`, its editor saved unchanged under `WriteMode::Incremental`, the original bytes its prefix | 0.015 → 0.02; held | ×2 000 | built |
| `write/rewrite 100 pages, CLI` | pages · 26 × 100 | `tpdf merge`, one input | 14.8 → 8; 1.85× | ×26 | built |
| `write/rewrite 1000 pages, CLI` | pages · 6 × 1 000 | `tpdf merge`, one input | 668 → 60; 11.1× | ×6 | built |
| `write/subset-on-save of 4 pages, CLI` | pages · 4 | `tpdf merge --font-policy subset` | 48.3 → 4.8, rescaled from 100 pages' 1 208 → 120; 10× | in | built |
| `write/merge 4 x 50 pages, CLI` | pages · 200 | `tpdf merge`, four inputs | 504 → 20, rescaled from 4 × 100's 1 008 → 40; 25.2× | in | built |
| `write/encrypt the 2.5 MB image file, CLI` | bytes · the file | `tpdf encrypt` | 82.8 → 16.1, rescaled from 23 MB's 770 → 150; 5.13× | in | built |

The merge keeps fonts because its target leaves no room for a subset pass —
20 ms for 200 pages where subset-on-save's is 4.8 ms for 4 — and
subset-on-save measures that pass on its own. The AES pair is tier P's "decrypt
/ encrypt", whose table states its targets as rates and no size, so the
2.5 MB file re-states no tier P figure.

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
| `layout/open and paginate <book> at 432x648 pt`, one for each of the nine | pages · n × the book's | each committed book, n opens a call: 18, 19 and 17 for `calibre-book-cover`, `calibre-book-nocover` and `pandoc-plates`, 86 for `kcc-fixed-layout`, 4 for the rest | 17–22 → 8; 2.12× | ×4 to ×86 | built |
| `layout/render every page of <book> at 72 dpi`, nine | pages · n × the book's | the same, every page n times a call: 12, 15, 7, 32, 9, 9, 11, 5 and 13 in the order the nine are named below | twin (CLI 84–130); floor 1 | ×5 to ×32 | built |
| `layout/open and paginate the 30-chapter book` | pages · its count | `book(30)` | not measured → ≥ 1 000 pages/s; calibrated; relation ≤ 3.3 × 10 chapters | in | built |
| `layout/render the 30-chapter book at 72 dpi` | pages · its count | `book(30)` | not measured → ≥ 300 pages/s; calibrated; the same relation | in | built |
| `layout/open and paginate the 10-chapter book` | pages · 2 × its count | `book(10)` | the relation's other side | ×2 | built |
| `layout/render the 10-chapter book at 72 dpi` | pages · its count | `book(10)` | the relation's other side; 33 at 300 pages/s | in | built |
| `layout/the 20-page HTML report to PDF` | pages · 3 × 20 | `html_report()`, laid out and finished | not measured → ≤ 60; calibrated | ×3 | built |
| `layout/the 20-page Markdown document to PDF` | pages · 3 × 20 | `markdown()` | not measured → ≤ 50; calibrated | ×3 | built |
| `layout/open each SVG fixture, render page 1` | pages · 4 × 17 | every committed `.svg`, its first page at 72 dpi | not measured; relation ≤ 1.5 × its synthesised PDF | ×4 | built |
| `layout/render each SVG fixture's synthesised PDF` | pages · 5 × 17 | each one's synthesised PDF, opened from bytes | the relation's other side | ×5 | built |
| `layout/open each XPS fixture, render page 1` | pages · 19 | every committed `.xps` and `.oxps` | the same relation | in | built |
| `layout/render each XPS fixture's synthesised PDF` | pages · 19 | the same | its other side | in | built |
| `layout/open each CBZ fixture, render page 1` | pages · 8 × 9 | every committed `.cbz` | the same relation | ×8 | built |
| `layout/render each CBZ fixture's synthesised PDF` | pages · 14 × 9 | the same | its other side | ×14 | built |
| `layout/render every page of <book> at 72 dpi, CLI`, nine | pages · the book's, 7 times for `kcc-fixed-layout` | `tpdf render` | 84–130 → 40; 2.1× | in; `kcc-fixed-layout` ×7 | built |

The nine books are `calibre-book-cover`, `calibre-book-nocover`,
`calibre-embedded-font`, `kcc-fixed-layout`, `pandoc-book-cover`,
`pandoc-book-epub2`, `pandoc-book-nocover`, `pandoc-embedded-font` and
`pandoc-plates`, each `.epub`, and the id carries the file name: the longest,
`render every page of calibre-embedded-font.epub at 72 dpi, CLI`, is 62
bytes. The containers are timed a format at a time, every fixture of the
format in one iteration, so the relation holds for each format's set: one
operation a fixture would be 90 operations and about seven minutes of the
layout group's run. A fixture the facade refused would be named in its generator's
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
| `fonts/outlines of Liberation Serif Regular` | outlines · 48 × 2 602 | the face, parsed and every glyph outlined | 2.1; held | ×48 | built |
| `fonts/outlines of Liberation Sans Regular` | outlines · 49 × 2 620 | the same | 3.0; held | ×49 | built |
| `fonts/outlines of Liberation Mono Regular` | outlines · 51 × 2 423 | the same | 1.8; held | ×51 | built |
| `fonts/outlines of every committed CFF program` | outlines · 2 × every program's | the 193 programs | not measured → ≥ 1 M outlines a second; calibrated | ×2 | built |
| `fonts/shape 1000 characters of Latin` | none · 125 calls | Liberation Serif Regular | not measured → ≤ 0.2; calibrated | ×125 | built |
| `fonts/shape 1000 characters of Arabic` | none · 50 calls | `TestShapeAran.ttf` | not measured → ≤ 0.5; calibrated | ×50 | built |
| `fonts/shape 1000 characters of Devanagari` | none · 8 calls | the vendored face | not measured → ≤ 0.6; calibrated | ×8 | built |

The 192 programs that parse as plain CFF hold 18 529 glyphs, and every one
outlines; the 193rd, `fuzz/corpus/sfnt/opentype-cff.otf`, is a fuzz seed
whose table the parser refuses, so it costs its refusal and adds no outline,
and the group's setup asserts all three counts. At a million outlines a
second the 18 529 take under 19 ms, so `calls` 2.

**threads** — MS-09's: the CLI's pool, `tpdf render --jobs`
(`tools/tpdf/src/main.rs:1338`), over one input on 1, 2, 4 and 8 jobs, so how
the pool scales is a ratio between operations one run times in one process
on one host. The input is cut by area rather than by count: a page drawn to
PNG through `tpdf` costs its render and its PNG, each in proportion to its
area, as the count record in `docs/verification.md` gives them for the text
page, so the group draws 24 quarter-size pages, one job's call inside the
window by that record and three pages for each of eight jobs. No run has
timed them, so the named machine's entry names all four unmeasured and their
window is *counted*. They embed their face, so every glyph is drawn.
The 300-dpi page in four bands is the render group's, beside the one-thread
render it is held against there, so it is timed once. The scaling floors
are relations `bench-check --machine` holds inside every plain run, the
oversubscription one armed only where the group's run recorded four logical
CPUs (`docs/verification.md`).

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `threads/24 quarter-size text pages to PNG at 150 dpi, 1 job, CLI` | pages · 24 | `dense_part(24, 0, 4)`, `tpdf render --jobs 1` | —; floor 1 | counted | built |
| `threads/24 quarter-size text pages to PNG at 150 dpi, 2 jobs, CLI` | pages · 24 | the same, `--jobs 2` | —; floor 1; inside one run ≥ 1.9 × 1 job | counted | built |
| `threads/24 quarter-size text pages to PNG at 150 dpi, 4 jobs, CLI` | pages · 24 | the same, `--jobs 4` | —; floor 1; inside one run ≥ 3.6 × 1 job | counted | built |
| `threads/24 quarter-size text pages to PNG at 150 dpi, 8 jobs, CLI` | pages · 24 | the same, `--jobs 8` | —; floor 1; inside one run ≥ 0.95 × 4 jobs, on four CPUs | counted | built |

**scripts** and **OCR** have no operation of MS-04's own; their rows create
their files (below). **calibration** holds one:

| Id | Unit · an iteration | Input | Tier P | Window | Step |
| --- | --- | --- | --- | --- | --- |
| `calibration/text of 5 dense pages` | pages · 5 | `dense(5, 0)`, the call `text/text of 5 dense pages` makes | 37.6 on the planning machine, rescaled from 100 pages' 751 (H-112); the operation every calibrated target is a fraction of | in | built |

Its file is frozen: it is the same input and call as the text group's, kept
apart so that nothing a later row adds to the text group can move it, and a
change to it re-states every calibrated target, which is the owner's (H-77).

**The sizes** are the hosted runner's, not tier P's: an operation that tier
P's planning figures put inside can fall outside on the runner and the other
way, so each is sized from what the runner read, and MS-04.10's fifteen
runs confirm every one inside on every run before any figure is
committed.

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

- **The progressive JPEG** is `baseline_jpeg_part(2)`'s coefficients
  re-scanned progressively by an outside program's lossless transcode, so a
  test holds its decode to `baseline_jpeg_part(2)`'s decode sample for
  sample: the two differ in their scan schedule and nothing else, which is
  what the baseline/progressive pair is for. `baseline_jpeg_part(2)` asserts
  the SHA-256 the README records as the transcode's source, so a change to
  the encoder fails there first, a finding rather than a quiet mismatch.
- **The three JPEG 2000 codestreams** are `photo` at 2480×3508, 620×877 and
  310×438, seed 0, each written once as a PPM and encoded by an outside JPEG
  2000 encoder as a JP2 file: one tile, five decomposition levels (P-10
  discards two), 64 × 64 code-blocks, one layer, LRCP; the two irreversible
  ones at one bit a pixel, the reversible one lossless — so a test holds the
  reversible decode to `photo(310, 438, 0)` exactly, the generator's input
  being the expected output, and the formula's digest is pinned beside the
  README's record of it. The full-page one is the JPEG 2000 page whose peak
  `tests/peak_memory.rs` holds; the two smaller ones are the sizes the decode
  group and the render group's JPEG 2000 page are timed at.
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
made from, and the program and the settings that made it; the file names
say what each is (`progressive-1240x1754.jpg`, `irreversible-2480x3508.jp2`,
`irreversible-620x877.jp2`, `reversible-310x438.jp2`), and `.jpg` and `.jp2`
are already binary (`.gitattributes:13-14`). The full-page codestream is the
largest, at about a megabyte, and the README states each one's size.

#### The CLI operations

Tier P marks thirteen operations CLI, and their figures include the process;
`tpdf` has no library half (`tools/tpdf/src` holds `main.rs`, `writing.rs` and
`images.rs`), so each is timed by running it. `support/cli.rs` resolves `tpdf`
beside the bench binary's profile directory — a bench binary lives in
`target/<profile>/deps/`, so `tpdf` is `target/<profile>/tpdf` — the layout
`xtask/src/corpus.rs:767-780` relies on for its own child, and refuses with
the command that builds it when it is not there. Each call is one child, its
stdout and stderr written to files in the operation's scratch directory, its
exit status checked every time: a failed child panics the bench, which the
group's guard then counts short, and is never timed. Test mode reads the same
files back, so the run that is timed and the run that is checked are the same
run.

| Operation | Command | Calls |
| --- | --- | --- |
| open and count the 23 MB image file | `tpdf info image.pdf` (`tools/tpdf/src/main.rs:903-957`; P-05's Today is this command) | 24 |
| text of 5 dense pages / 1000 / 4000 small pages | `tpdf text dense-5.pdf`, `small-1000.pdf`, `small-4000.pdf` | 1 / 2 / 1 |
| structured JSON of 5 pages | `tpdf text dense-5.pdf --json` | 1 |
| the half-size text page to PNG at 150 dpi / the quarter-size one at 300 dpi | `tpdf render dense-half-1.pdf --out DIR --dpi 150` / `dense-quarter-1.pdf --dpi 300` | 1 |
| 8 quarter-size pages to PNG at 150 dpi, 1 / 4 jobs | `tpdf render dense-quarter-8.pdf --out DIR --dpi 150 --jobs 1` / `--jobs 4` | 1 |
| 24 quarter-size text pages to PNG at 150 dpi, 1 / 2 / 4 / 8 jobs | `tpdf render dense-quarter-24.pdf --out DIR --dpi 150 --jobs 1` / `--jobs 2` / `--jobs 4` / `--jobs 8` | 1 |
| rewrite 100 / 1000 pages | `tpdf merge small-100.pdf --out OUT.pdf --font-policy keep` / `small-1000.pdf` | 26 / 6 |
| subset-on-save of 4 pages | `tpdf merge dense-4.pdf --out OUT.pdf --font-policy subset` | 1 |
| merge 4 x 50 pages | `tpdf merge dense-50-0.pdf dense-50-1.pdf dense-50-2.pdf dense-50-3.pdf --out OUT.pdf --font-policy keep` | 1 |
| encrypt the 2.5 MB image file | `tpdf encrypt small-image.pdf --out OUT.pdf --entropy ENTROPY --font-policy keep` | 1 |
| render every page of a book at 72 dpi | `tpdf render BOOK.epub --out DIR --dpi 72` | 1; `kcc-fixed-layout` 7 |

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
from the same checkout in the same run, so the helper's `SPAWNERS` row says it
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
        "per_iteration": 1001980, "calls": 2 },
      { "name": "the text page at 300 dpi", "unit": "pixels",
        "per_iteration": 8702320, "calls": 1 }
    ]
  }
],
```

`per_iteration` is an integer in the unit's base — pages, pixels, bytes,
outlines — covering every call, so throughput is `per_iteration` over the
mean and a call is the mean over `calls`. Criterion takes the figure it
reports a rate in, and records in each operation's `benchmark.json`, when the
operation starts, before the operation is defined, so the harness reads the
group's list (`support/list.rs`) and hands criterion the committed figure;
the bench file declares the same figure, and once the operation is defined
the harness refuses one whose declaration is not its list's, so a generator
and its list cannot drift apart unseen on any machine. The planning
baseline's build holds nothing to the list, since its own engine computes
what its operations declare and the list is the head's. An operation the owner names outside the
window carries `"outside_window"`, its reason, which `bench-check --sizes`
reports instead of failing; the set names none. A machine entry carries `"unmeasured"`, the ids it has no figure
for, ahead of its `"operations"`, so a hole is named and never silent. The
reader cuts the file at `"machines"` first, so no list entry can fall into a
machine's chunk. MS-02.3 adds each operation's
gate beside its unit, in the same list.

`cargo xtask baseline` then holds three things to each other: each
`[[bench]]`, its group file's names and its list; and it refuses a name
criterion would rewrite, a unit outside the
five, a `per_iteration` or `calls` under one, and a machine entry naming an
operation as both measured and unmeasured.

#### Sampling, and a run's length

| Groups | Samples | Warm-up | Measurement | Mode |
| --- | ---: | ---: | ---: | --- |
| open, text, analysis, render, encode, decode, write, layout, threads, OCR, calibration | 10 | 1 s | 3 s | flat |
| sign, fonts, scripts | 30 | 1 s | 3 s | flat |

The window puts every operation at 20 ms or more, where criterion's linear
sampling buys nothing and costs up to 55 iterations at ten samples; flat
sampling takes each sample as ⌈3 s ÷ (samples × t)⌉ iterations, so an
operation of t costs its warm-up — at least one iteration — and then about the
larger of 3 s and samples × t: about 4.7 s inside the window, and 11 × t above
it, ten samples being the least criterion 0.5.1 takes (its `sample_size`
asserts it). Thirty samples cost the sign, fonts and scripts groups nothing:
their operations sit near the window's middle, and at 100 ms or less thirty
samples fit the 3 s that ten would take. Each file sets its sample size
and times in `criterion_group!`'s `config`, which criterion's command line
overrides, never on a `BenchmarkGroup`, whose settings would beat the command
line, so MS-02's rounds can sample differently without editing a file; flat
sampling, which criterion sets only on a group and which has no command-line
flag, is the one setting each group makes itself.

Sized into the window, every operation measures for about 4.7 s under this
sampling, so the set's 147 measure for about eleven and a half minutes before
anything is built, group after group in one local `cargo bench -p tinker-pdf`.
Every
relation tier P draws between two operations has both in one group — the
analysis group carries its own text for that reason — so one bench binary
measures both, in one process on one host (MS-10.1's note).

| Group | Operations | Measurement |
| --- | ---: | ---: |
| layout | 40 | 3.1 min |
| render | 34 | 2.7 min |
| write | 17 | 1.3 min |
| text | 13 | 1.0 min |
| decode | 9 | 0.7 min |
| analysis | 8 | 0.6 min |
| open | 7 | 0.5 min |
| fonts | 7 | 0.5 min |
| encode | 4 | 0.3 min |
| threads | 4 | 0.3 min |
| sign | 3 | 0.2 min |
| calibration | 1 | 0.1 min |

A run builds each group's bench in release, as `cargo bench` does, and builds
`tpdf` in release first for the six groups with CLI operations. MS-02's
paired run, three builds and ten rounds of this set, is priced in MS-02's
section.

#### The window

`bench-check --sizes` reads each operation's mean from the run's estimates —
an iteration's, so `calls` included — and fails one outside 20–300 ms on the
head, unless its list names it outside by the owner's word, which it reports
instead. It runs after the bench, because the window is a property of the
machine the bench ran on and never of `cargo test`. A run in report mode
prints every operation outside with its mean, and the set is sized from that
list — its committed sizes from the hosted runner's — not from tier P's
column: a short operation by its `calls`, a long one by a smaller input.

#### The two level-6 byte counts

Tier P holds the deflate ratio level to 1.02 times the size level 6's
parameters give each of the two inputs, exactly: a planning number, computed
once by a reference deflate outside the engine and never measured again
against a build (`docs/roadmap/P.md:96-97`, `:137-141`; ruling 17). They live
in `benches/support/samples.rs` as `PAGE_LIKE_LEVEL_SIX` and
`PHOTO_LIKE_LEVEL_SIX`, two `LevelSix` constants beside `page_like()` and
`photo_like()`. Each holds the length of the raw RFC 1951 stream level 6's
parameters give on its input — a 32 KiB window, memory level 8 and the
default strategy, with no RFC 1950 wrapper, since `deflate`, the call the
encode group times, writes none — and the length and SHA-256 of the bytes it
was taken on, which each generator asserts of its own output.
Not in `baseline.json`, which holds a machine's times and is read as such: a
byte count is neither. Beside the generators, because a count is a fact about
those bytes alone, and a generator changed under it would leave it describing
other bytes — so the digest fails the generator first, and a moved digest is
a finding, never re-pinned. `tests/level_six_counts.rs` reaches them by path
and runs both generators under `cargo test`; P-04.4's `deflate_size.rs` reads
them the same way. Nothing in the tree runs level 6: no first-party encoder
has it, and an outside one may not decide a build (ruling 13).

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
| `text/extract a page of text` | `text/text of 20 standard-14 pages` | the same call over standard-14 Helvetica, 20 pages for one |
| `render/render a full-page axial shading at 150 dpi` | `render/a full-page axial shading at 150 dpi` | the same generator, `axial()` |
| `render/fill 300 anti-aliased paths at 150 dpi` | `render/300 triangles at 150 dpi` | the same 300 triangles, `triangles()` |
| `write/rewrite a document` | `write/rewrite 100 pages` | the same call, 100 pages for three |
| `layout/paginate a book at 432x648 pt` | `layout/open and paginate pandoc-book-cover.epub at 432x648 pt` | the same book, box and call |

#### What later rows add

A row whose capability does not exist yet adds its operation in the commit
that adds the capability, under an id these rules allow, in the group its
operation belongs to, listed unmeasured in the machine entry until the next
fifteen-run measurement (H-109), and sized by `calls` or a smaller input as
the set's are. Tier P's table and the Item name these:

| Row, step | Operation | Group | Tier P's gate |
| --- | --- | --- | --- |
| FJ-01, FJ-01.41 | `scripts/compile a 64 KiB document-level script` | scripts, creating `scripts.rs` | calibrated, ≤ 5 ms |
| FJ-16, FJ-16.1 | `scripts/a 500-field calculation cascade`, `scripts/500 field appearances regenerated` | scripts | calibrated, ≤ 20 and ≤ 25 ms |
| OC-17, OC-17.2 | `ocr/OCR a 300-dpi Latin page on one thread`, `ocr/OCR a 300-dpi Chinese page on one thread` | OCR, creating `ocr.rs` | calibrated, ≤ 1.5 and ≤ 4 s: above the window at their targets |
| SG-02, SG-02.7 | `sign/ECDSA P-256 signature`, `sign/ECDSA P-384 signature`, `sign/ECDSA P-521 signature`, `sign/ECDSA Brainpool P-256 signature` | sign | calibrated, ≤ 0.5, 1.2, 2.5 and 1 ms; `calls` 50, 21, 10 and 25 |
| MS-08, MS-08.4 | `open/first page of the 60-page linearized fixture over a slow link` | open | counted; the time follows from MS-08.3's counts, never slept |

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
| Two engines generating one input write different bytes, so a pair times two inputs | MS-02's paired run routes every input through the directory `TINKER_BENCH_FIXTURES` names, written by the head first |
| An input an engine encoder writes moves when the encoder changes (P-04.7's deflate, the JPEG encoder) | The row that changes the encoder names the operations it moved in its commit; the two inputs the level-6 counts describe are formulas pinned by length and digest, so they never move |
| A run of the set takes too long to make as a change's proof | Every operation sized into 20–300 ms, so the set measures for about eleven and a half minutes before a build |
| Tier P's figures are the planning machine's, so the budget and the window's list are predictions | The set is sized from the hosted runner's own figures, and MS-04.10's fifteen runs hold every operation inside on every run before any figure is committed |
| A repetition count and its committed `per_iteration` drift apart | Criterion records the list's figure, and the harness holds each operation's own declaration to it whenever the operation is defined |
| A CLI operation times a failed child, or a stale `tpdf` | The exit status is checked on every call and a failure panics the bench; `tpdf` is resolved beside the bench binary and built from the same checkout in the same run |
| The committed codestreams grow the repository and come from a program this project does not own | Four files with their bytes, digests, coding and sources recorded; their decodes are checked first-party, the reversible one against the generator's input and the progressive one against the baseline decode |
| A format's aggregate hides one container fixture's regression | Every fixture is opened and rendered in the operation and asserted in test mode; a figure for each fixture would take ninety operations, which a row that needs one adds |
| The calibration operation moves | Its file is frozen; a change re-states every calibrated target, which is the owner's (H-77) |
| A row's speed-up takes an operation under 20 ms | The row raises its `calls` in the commit that raises its floor, with the figure re-measured under H-109 |
| A machine's entry is read on a host it was not measured on | A pinned machine's entry carries its host's `cpu`, and `bench-check` refuses a host whose model name is not that one; a fleet's entry, such as `ubuntu-latest`'s, is banded per operation over fifteen runs (`docs/verification.md`, "The named machine is `ubuntu-latest`") |

## MS-07 — Corpus throughput and latency ratchet

In this design, each of the corpus run's three bars holds every corpus to
five time figures at the run's dpi — pages rendered a second, the mean
open-and-count latency, and the 50th, 95th and 99th percentile of a page's
render time — each inside a band set by the five runs the bar records, and
`cargo xtask corpus-run --check`, run locally against the bar, fails a figure
past its band. Every run prints each corpus's ten slowest files with their
`/Producer`, and each bar records them. The figures come from the runs the
bars already make, one child a logical CPU, so a run's time does not grow.

### Scope

- **What the child times**: one span for opening a document and counting its
  pages, and one for each page's render at the run's dpi, both apart from the
  strict and metamorphic passes, printed in microseconds in record format 7.
- **The five figures** a corpus is held to, and how each is computed from
  those spans.
- **Where the times come from**: the shared run, how its concurrent children
  bear on each figure, and what a pass of its own would cost a run.
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
- **A figure that cancels the machine.** A calibrated figure divides by an
  operation the same run times; the corpus run times none, so these figures
  are absolute and their bands absorb the host (the band rule, below).
- **Other platforms.** A bar's times are the hosts its five runs were made on;
  no other platform's are measured.
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
  (`tools/tpdf/src/main.rs:859`) is outside, since it measures the machine's
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
(`xtask/src/report.rs:825`). Each child renders on one thread: the engine
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
(`xtask/src/ratchet.rs:1-26`, `:432-439`). Every key is a whole number, which
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

#### A child a logical CPU, and what a pass of its own would cost

`corpus-run` runs one child a logical CPU unless told otherwise
(`xtask/src/corpus.rs:197`, `:678-706`), and a bar's run never tells it: four
on a machine of four logical CPUs. A span is wall time, so a page's includes
what its neighbours cost it — shared caches, memory bandwidth, and a core where
two logical CPUs share one — and seldom a wait for a CPU: single-threaded
children, one a logical CPU, do not outnumber the machine's logical CPUs, and
the runner's own worker threads sleep between polls
(`xtask/src/runner.rs:451`). That cost has
two parts. One is systematic, every child somewhat slower than it would run
alone, and a bar measured at the same job count carries it inside its figures;
the bar holds that constant by refusing times from a run at another job count.
The other varies with which files share the machine with which, and the five
runs' spread measures it. It sits under what a host does from one run to the
next (`docs/verification.md`, "The named machine is `ubuntu-latest`"), which
every band has to absorb anyway.

**So the times come from the shared run**: each bar's own `corpus-run`, at the
job count it runs at. Its cost is two clock reads and one record line a page,
and a run's time does not grow. A pass of its own, one child at a time over
open and render alone, would remove the neighbours and nothing else, and would
run each bar's files end to end rather than a logical CPU's worth at a time;
it does not buy enough of the smaller term to be worth that.

#### The record, the report and the bar

**The record** is version 7. After `pages`, a file that
opened prints `time open <micros>`: `Document::open` on bytes already read,
the faces, the authentication and the first `page_count`. The read, which
times the machine's disk rather than the engine, is outside it, and so are the
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
the second (`xtask/src/corpus.rs:799-844`). The runner keeps a file's open
span as an `Option` and its page spans as a list of page and microseconds
(`xtask/src/runner.rs:131-151`, `:222-229`), and drops a line that does not
parse, or that names a page outside 1 to `pages`, rather than reading it as
zero (`:690-703`, `:818-821`).

**The report** carries, per file, `open_micros`, `render_micros` (its pages'
sum) and `slowest_page` (`{ "page", "micros" }`), each absent where there was
nothing to time; per corpus, in its `summary` (`xtask/src/report.rs:541-591`),
`time` — `files`, `open_micros`, `pages`, `render_micros`, `p50_micros`,
`p95_micros` and `p99_micros` — and `slowest`, the ten slowest; `jobs` in its
`settings` (`:331-356`, `:525-539`); and a `host` beside them,
`/proc/cpuinfo`'s first model name and the logical CPU count, absent off
Linux, so that a figure that moved with the host is attributed rather than
banded. Its summary lines
(`xtask/src/report.rs:704-806`) carry a `time` line per corpus — pages/s at the
dpi, the latency and the three percentiles — and one for the run,
`time   N pages timed   M files opened`.

**The bar** carries `time_runs` at its top and `time` and `slowest` in each
corpus, and `jobs` but no `host` in its `settings`; each run's per-corpus
figures go into it whole:

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
  (`xtask/src/ratchet.rs:483-490`): each is a different measurement. A run
  that timed fewer files or pages in a corpus than the fewest of the five
  fails, as a peak over fewer children does (`:821-830`). A figure faster
  than the five's fastest is a note, not an improvement, because one run
  measures no band. A bar with no `time_runs` is an improvement, new with no
  recorded band, and `SCHEMA` stays 2, as it does for `peak`
  (`xtask/src/report.rs:23`, `xtask/src/ratchet.rs:82-90`).
- **`--record`** writes every other axis from its run
  (`xtask/src/corpus.rs:523-557`), and the run's ten slowest; it carries
  `time_runs` and each corpus's `time` over from the committed bar unchanged,
  and never writes a time figure from one run (`ratchet::carry_times`). A
  figure from one run would ratchet the bar onto whichever host that run
  was made on, and the next run on another host would fail on the
  machine rather than on the code. A committed bar that does not read as JSON
  is refused rather than overwritten, since overwriting it would drop the five
  runs' figures.
- **`ratchet::parse`** refuses a time block of other than five runs, or whose
  runs do not match `dispatches`, or that sits in a bar with no `time_runs`; a
  run with more files than `total`; and a zero figure over files or pages that
  were timed, a band nothing can sit under, as it refuses a peak of zero bytes
  (`xtask/src/ratchet.rs:1246-1251`).

#### The band rule over five runs

The five runs are five local runs at one revision of `develop`, each named in
the bar's `dispatches` with its host; a run at another revision is not one of
them. They need not be green: `corpus-run` writes a run's `--report` before it
checks the bar, so the report is kept whatever the check finds. For each
corpus and each figure:

- **The figure is the fastest of the five**, for the reason the bench's is the
  fastest of fifteen: the comparison is one-sided, so the bar worth holding is
  the best the machine has done (`docs/verification.md`, "The named machine is `ubuntu-latest`").
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
figure is at most the slowest, and the band is above the spread.
`ratchet::band` derives each band in integers, as it compares.

**What this does not catch.** A regression inside its band: on a figure
banded at 85 %, a page render half again slower passes in silence, and a
several-fold regression would not. This is
the corpus's ratchet against the catastrophic, on pages real producers wrote;
the finer instruments are MS-02's paired run over MS-04's set and MS-01's
counts, which no host moves.

#### The ten slowest

Per corpus, the ten files whose spans sum the most — open-and-count and every
page — slowest first, ties by path, each with its corpus, its path, its pages
timed and its sum, under its `/Producer` as the record states it
(`xtask/src/runner.rs:159-168`; `(none stated)` is a producer,
`tools/tpdf/src/main.rs:2618-2627`). The table is `producer_table`'s shape,
with its escaping of a producer's pipe (`xtask/src/report.rs:825`, `:867`),
printed after it on every run (`xtask/src/corpus.rs:505-510`), in each
corpus's report summary, and in the bar as `slowest`; nothing compares it. A
file with no spans is not ranked, so a file the runner killed is in the
producer table instead. Ranking by a file's sum favours long documents; the
report's `slowest_page` names the page that cost a file most.

### Risks

| Risk | Mitigation |
| --- | --- |
| A child's neighbours move its spans | One single-threaded child a logical CPU; the job count is in `time_runs` and a run at another is refused; the five runs' spread measures the rest |
| Five runs on one host say nothing of another | The fastest of five is the figure, and the band is 2.295 times the spread and never under 20 %; each run's host is recorded beside its id |
| Five runs read a spread below what fifteen would | The 1.53 the bench measured for tripling its sample; a band that fails on unchanged code is a finding, re-measured over five more runs and never widened by hand |
| A run that times fewer files or pages reads as a faster one | Files and pages timed are held to the fewest of the five; a killed child's spans are dropped, and its timeout is already the pass rate's regression |
| One run's figure ratchets the bar onto a fast host | Only five runs set figures; `--record` carries them and writes none; a faster figure is a note |
| A re-pinned corpus, a machine of another size or another dpi changes what is measured | Each is refused by name — `time.total`, `time_runs.jobs`, `time_runs.dpi` — until five runs re-measure, so a re-pin costs five runs before the bar's check passes again |
| A P row's speed-up leaves its corpora's bands slack | The bar is a backstop against the catastrophic; re-measuring is five more runs, never `--record`; MS-02's floors are what a speed-up raises |
| The clocks cost the child time | Two clock reads and one line a page |
| A probe that starts using threads changes what neighbours cost | The engine spawns none, and P-15's pool will run serially unless asked; a row that gives the probe threads re-measures the time block |
| A corpus's percentiles are one long document's | Said above for verapdf; the report keeps each file's sum and slowest page, so a moved percentile is followed to its file |
| A zero figure bands nothing | `parse` refuses one over files or pages that were timed |

## P-01 — Page index

*For [ROADMAP](../ROADMAP.md) row P-01.* A page lookup finds its page
without walking the tree from its root: out of the page index a count builds,
or by a `/Count` descent before one has (`pages::at_counted`,
`crates/tinker-pdf-cos/src/pages.rs`), and the editor looks its pages up in
the same index. This section fixes the order
a lookup tries its routes in, what the `/Count` descent may believe, when the
flat index is built, what it holds and when it ends, and which per-page site
each guard leg and each of the row's operations waits on.

### Scope

- **The lookup order**: Annex F's first page and hinted pages, then the
  memoised index, then a `/Count` descent, then the walk.
- **The descent** (P-01.2): what it checks at each node, and what it cannot
  see.
- **The index** (P-01.3): when it is built, what an entry holds, its life
  beside the object store and its end at authentication, and the page-tree
  warnings raised once.
- **The editor's indexed lookups** (P-01.4).
- **A1's and A2's counted quantities** in MS-01's tally, and the site each A2
  leg and each operation waits on.

### Non-goals

- **Hint tables.** Annex F's first-page and hinted lookups are
  `streaming-open.md`'s and stay first, unchanged.
- **Per-call structure binding**, which binds a page to the structure tree
  each time it is asked and is no lookup of a page.
- **Per-page work that does not grow with the page count**: a page's own
  content, fonts and images, which A2's legs already show flat once their
  lookups are taken out (MS-01.7's table above).

### Design

#### The lookup order

`pages::at(doc, i)` tries, in order:

1. **Annex F**: page one from the linearization dictionary's `/O`, any other
   page from the hint tables (`linearized_first_page`, `hinted_page`). No
   lookup builds the index, so a streamed document that pages through its
   hints never fetches its main table; counting its pages does
   (`asking_for_the_page_count_is_what_pays_for_the_tail`).
2. **The memoised index**, when it is built: the entry at `i`, copied out.
3. **The `/Count` descent**, when it is not.
4. **The walk**, when the descent declines.

The descent and the walk must hand back the same page for every tree whose
counts are honest (tier P: a cache hit returns what a miss computes), so the
index, the descent and the walk are three ways to one answer and never three
answers.

#### What the descent checks, and what it cannot see

From the root, a node's kids are taken in order, each read once: a kid
without `/Kids` is a leaf and counts one; a kid with `/Kids` counts its own
`/Count`; a kid that does not read counts none, as the walk finds no page in
it. Kids are skipped while their counts sum to no more than `i`, and the
descent enters the kid that holds page `i`, carrying the inherited attributes
(`/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`) down the path exactly as the
walk does (`Inherited::extend`). The kids after the one it enters are never
read: checking that every kid's count sums to the node's own would read the
whole of a flat tree to find its first page, which the walk finds in two
reads and MS-08.3 holds a streamed open to. At each node it checks:

- the node's `/Count` is a non-negative integer, and more than what is left
  of the index;
- the counts of the kids it reads, up to and including the one it enters,
  stay within it;
- a leaf is reached with nothing left of the index;
- the node is not already on the path (a cycle);
- the path is no deeper than `limits::MAX_NEST_DEPTH`;
- an index at or past `limits::MAX_PAGES` answers `None`, as the walk does.

Any check that fails falls back to the walk, warning `PageCountMismatch`
against the node; an index past the root's own `/Count` goes to the walk
without one. One lookup reads its root and at most fan-out kids at each
level under it: the root and depth × fan-out nodes. It counts as one walk of
the tree, as `collect_upto(1)` does, so a lookup's pinned count — one walk of
the root and the page — is unchanged.

**What it cannot see**: a lie in a kid it passes, whose `/Count` is believed
as a reader that never enters the kid must; and counts that are honest about
a subtree the walk reaches differently. A subtree two parents share is two subtrees to the walk,
which follows `/Kids` and guards only the path it is on, and two subtrees to
the descent, whose counts include it twice, so the two agree; a node whose
leaves the walk truncates at `MAX_PAGES` is answered by the walk. And a
tree whose kids are all leaves — `DocumentBuilder` writes every page under
one `/Pages` node — has a fan-out of its page count, so the descent reads
every kid before page `i` and only the index helps.

#### The index: its build rule

The index is the walk's page list, built once by the walk and kept. **It is
built by a count of the pages, and by nothing else**: `pages::count`, and an
editor's lookups while it has no page order of its own, which count the
document's pages first. The count walks the whole tree to answer, so keeping
what the walk found costs it nothing more. A document's lookup the index does
not answer descends, however many lookups came before it, and `collect` and
`collect_upto` read the index once it is built and walk without keeping what
they find before then. A page loop counts its pages first — the CLI's, the C
ABI's, the benches' — so it reads every page out of the index, and a lookup
on its own never pays for the tree.

That rule keeps both of A1's bounds:

- **Each `page(i)` within its root and depth × fan-out**: a lookup is the
  descent, or, once a count has built the index, one entry copied and no node
  read.
- **A page loop within 3 visits a page**: a loop over *n* pages is one walk,
  the count's (every node once: *n* leaves and the nodes above them, fewer
  than *n* for any fan-out of two or more), and *n* entries copied out of the
  index. On A1's balanced 10 000-page tree of fan-out 10 that is
  11 111 + 10 000, under 30 000.

**An entry** is the `Page` the walk builds — reference, index, the five boxes,
the rotation and the inherited `/Resources` — and the index keeps beside its
entries a map from each page's reference to its position, the first where a
shared subtree holds a page twice, which `dest::Resolver` reads to place a
destination's page. A lookup copies one entry out, and a read of every page
copies each;
**the entries copied are counted beside the nodes visited**, as a counter of
their own in MS-01's tally (`page_entries_copied`), so A1's total is nodes
visited plus entries copied.

#### Its life, and its end at authentication

The index lives on `CosDocument` beside the object store, so every clone of a
`Document`, which shares the `CosDocument`, shares it. `install_security`
drops it with the store and the object streams: an index read over ciphertext
holds inherited `/Resources` whose strings were never decrypted, and a tree
in an encrypted object stream reads as no pages at all, so the next count
after a password builds it again from plaintext (`encrypt_on_save.rs`
`the_page_index_is_dropped_when_a_password_arrives`). **A walk that missed
keeps nothing**: a load whose bytes a streamed source did not have reads as
null and is never published, so a count during which one missed answers from
its own walk and keeps no index, and the count after the host has supplied the
bytes walks again (`streaming.rs` `a_count_that_missed_keeps_no_page_index`).

**A count raises the page-tree warnings once**, by the walk that builds the
index: `PageTreeCycle`, `PageTreeTruncated`, `MediaBoxMissing` and
`PageCountMismatch` say what the tree is, and a lookup or a count that reads
the index raises none of them again.

#### The editor's indexed lookups

`DocumentEditor::page_count` and `DocumentEditor::page_ref` read the editor's
own page order when an edit has made one, and the document's index otherwise,
without walking or copying either: a lookup is one entry copied. Every
per-page operation in `edit/` uses them, and facade redaction's `apply` looks
its page up the same way; `import_page` and `import_page_as_form` read the
source page through `pages::at`, and `page_box` its own. `page_refs()` stays
for its callers, the CLI's merge loop and the C ABI's and bindings' editor
page counts among them: it clones the editor's order, or copies every
reference out of the index. `dest::Resolver` holds the index — the
document's when a count has built it, one walked for it and not kept
otherwise — and places a destination's page by the index's map, so a page's
links copy no page list (`page_lookups.rs`
`links_on_every_page_copy_no_page_list`).

#### A1's and A2's counted quantities

A1 reads `PageNodesVisited` and the entries copied: per lookup, its root and
at most depth × fan-out nodes; over every page of the 10 000-page tree, the
count's walk included, at most 30 000 nodes and entries together. A2 compares
`work()`: on a leg that looks pages up one at a time, the count's walk, an
entry a page, and each page's own streams, glyphs and fonts.

Each A2 leg, the per-page site superlinear in its pages where it has one, the
counter that sees it, and the step whose guard holds it. A leg with none grows
with its pages in its counted work and its time alike: the save with fonts
kept from 7 ms to 27 ms and with subsetting from 62 to 268 ms, at 1 000 and
4 000 pages of A2's document in release.

| Leg | Superlinear site | Counter that sees it | Step |
| --- | --- | --- | --- |
| text | none: the loop counts first, and each `page(i)` copies its entry out of the index | `PageEntriesCopied`, one a page | P-01.3, landed |
| render-all at 36 dpi | none: the same lookup | `PageEntriesCopied`, one a page | P-01.3, landed |
| rewrite | none: the serializer reads each object once, and the save's subset pass walks the tree a fixed number of times (`subset::apply`); the leg packs object streams and compresses | the serializer counts nothing; the leg's work is its subset pass's | P-01.3, landed |
| rotate | none: `rotate_page` looks its page up by `page_ref` (`edit/page_ops.rs`) | `PageEntriesCopied`, one a page, on a counting editor | P-01.4, landed |
| merge | none: `import_page` looks the source's page up by `pages::at`, out of the index the source's count built, and checks its place by `page_count` | `PageEntriesCopied`, one a page | P-01.4, landed |
| split | none: `keep_pages` reads the order once, and each half's save walks its tree a fixed number of times | — | P-01.4, landed |
| subset | none: a fixed number of walks (`subset::apply`) | — | P-01.3, landed |
| redaction | none: `redact::apply` looks its page up by `page_ref` | `PageEntriesCopied`, one a page, on a counting editor | P-01.4, landed |
| validation | none: every group `validate_pdfa` asks for, whose rules walk the tree a fixed number of times (`pdfa/content.rs`) | — | P-01.3, landed |

**The serializer counts nothing**, which is a finding rather than a site: a
superlinear loop in the writer would pass A2's rewrite leg, whose counted
work is the subset pass's. It is linear at the tip, so nothing waits on it;
a row that touches the writer's loops counts them.

The row's three operations:

| Operation | Superlinear site at the tip | Step |
| --- | --- | --- |
| Every page's size over 10 000 pages | none: the bench counts first (`benches/open.rs`), and each `page(i)` copies its entry out of the index | P-01.3, landed; P-01.5 records its floor |
| Text of 10 000 small pages, CLI | none: `tpdf text` counts first and looks each page up by index, the same lookup | P-01.3, landed; P-01.5 records its floor |
| Rewrite of 3 000 pages | none: the bench's input rewrites 1 000 pages in 6.6 ms and 3 000 in 21 ms, already inside tier P's 40 ms, so the floor P-01.5 raises is held by the tip | P-01.5 records it |

No A2 leg goes to a row outside P-01, so MS-01.9's depends gain nothing from
this section.

### Milestones

1. **P-01.2: the `/Count` descent** in `pages::at`. Exit: `pages.rs`
   `descent_and_walk_agree_on_every_page_of_honest_trees` (generated trees of
   fan-out 1 to 16 and depth 1 to 6, at most 4 096 pages each, with boxes,
   rotation and resources inherited at every level and a subtree two parents
   share, give every page what `collect` gives) and
   `a_lying_count_falls_back_to_the_walk_and_says_so`; `guards.rs`
   `a1_page_lookup_reads_at_most_depth_times_fan_out_nodes` flips from owed;
   `streaming_open.rs` unchanged.
2. **P-01.3: the memoised index**, under the build rule above. Exit: `guards.rs`
   `a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits`, A2's
   text and render-all legs, and `guard_allocations.rs` A10's three flip from
   owed, with A2's subset, validation and rewrite legs landed green;
   `encrypt_on_save.rs` `the_page_index_is_dropped_when_a_password_arrives`;
   `tinker-pdf-ffi`
   `a_page_loop_over_the_c_abi_stays_within_a1`; `streaming_open.rs` and
   `streaming_determinism.rs` unchanged.
3. **P-01.4: the editor's indexed lookups.** Exit: A2's rotate, merge and
   redaction legs flip from owed, with split landed green; `page_lookups.rs`
   `links_on_every_page_copy_no_page_list`; `edit/tests.rs`, `writer_names.rs`,
   `stream_dedup.rs` and every redaction test unchanged.
4. **P-01.5: the row's close** in MS-02's paired run: every page's size over
   3 000 pages at 856×, text of 1 000 small pages (CLI) at 23.3×, rewrite of
   3 000 pages at 137×, with the relations 10 000 pages ≤ 3.6 × 3 000 and
   ≤ 11 × 1 000; A1 and A2 green on linux and windows in CI's tests and on
   `wasm32-wasip1` locally.

### Risks

| Risk | Mitigation |
| --- | --- |
| The descent and the walk disagree on a tree whose counts are honest | `descent_and_walk_agree_on_every_page_of_honest_trees` over generated trees with every inherited attribute and a shared subtree |
| A count that lies sends the descent to the wrong page | Every count is checked against its kids' sum on the path, and a mismatch falls back to the walk with `PageCountMismatch` |
| The index serves ciphertext after a password | `install_security` drops it with the store; a test holds it |
| A streamed document builds the index and fetches its main table | Only a count builds it, and a count walks the whole tree anyway; `streaming_open.rs` unchanged |
| A single lookup pays for the whole tree | No lookup builds; a lookup the index does not answer descends |
| The serializer grows superlinear unseen | Named above: it counts nothing, and a row that changes its loops counts them |

## P-02 — Resolved-resource cache

*For [ROADMAP](../ROADMAP.md) row P-02.* A page's interpretation asks its
resource scope the same questions again for every glyph and every operator.
Each glyph shown asks `type3_glyph` whether its font is a Type 3 font, and the
scope answers by reading the font's dictionary out of `/Font` again
(`fonts::font_dict`): the `/Font` subdictionary copied when it is written
inline, the font's dictionary copied whole with its `/Widths`, and `/Font`,
`/Subtype` and the font's name looked up in the document's name table by their
bytes — all to answer "no" for every glyph of every font that is not one. A
Type 3 glyph walks its font's `/Differences` from the first item to find its
name. Each `cs`, `sc` and `scn` that names a resource reads its colour space out
of `/ColorSpace` and parses it again, and each `sh` its shading's function. And
a scope reads every font its `/Font` names when it is built, used or not. This
section fixes what one scope resolves once and under which key, the names the
name table pre-interns, the Type 3 code-to-name reading, what a hit says again
so that a hit and a miss agree, the counts the row's guards read, and, for each
of the row's four speed operations and six relations, the path it takes and the
step that brings it within bound.

### Scope

- **The scope's font table** (P-02.3): each font a scope's content selects,
  read on the first use of its name, and what an entry holds.
- **The Type 3 record** (P-02.4): a Type 3 font's codes, procedures and
  matrix, read on its first glyph.
- **Colour spaces, functions and graphics states** kept by the scope (P-02.5).
- **The names the table pre-interns** (P-02.2).
- **The structured-text writer's numbers** (P-02.6) and **batch inference's
  page reads** (P-02.7).
- **A3's counted quantity** and the counters the row adds; the CMap warnings
  that reading fonts on first use stops raising; and each operation's and
  relation's path, with the step that brings it within bound.

### Non-goals

- **A font cache shared across scopes or pages.** A name means what its own
  scope's `/Font` binds it to (7.8.3), so a table is one scope's: two pages
  that draw one face each read it, and a cache that outlives its scope is
  P-16's, held to a byte budget.
- **Font-program parsing**: P-26's. `selection` still parses the program for
  each code it is asked about.
- **The decoded-stream cache**: P-03's. A Type 3 procedure is decoded at each
  glyph (A5), and P-03.2's operation-scoped cache brings that to one.
- **Search over prebuilt text**: P-24's.
- **A cache keyed by region** (ruling 5). Every entry here is keyed by a name
  or an object, so a tile and its page read the same entries.
- **An XObject's dictionary at each `Do`**: `xobject` copies it out for each
  question a `Do` asks. No operation of this row draws a form or an image, and
  P-03.2 keeps a form's content.

### Design

#### What a scope reads again at the tip

| Question | Asked | What it reads each time | Step |
| --- | --- | --- | --- |
| `type3_glyph` | every glyph | the font's dictionary out of `/Font`, copied, with `/Font`, `/Subtype` and the font's name looked up by their bytes; for a Type 3 font, `/Encoding`, `/Differences` walked from its first item, `/CharProcs` and `/FontMatrix` | P-02.2 the keys, P-02.3 the kind and P-02.4 the record, landed |
| `font_object`, `resource_name`, `selection` | every glyph a save's subset walk records (`subset::show`), and each outline the renderer extracts | `font_ids` scanned for the id and the name copied; `font_object` the `/Font` subdictionary copied too (`font_table`) | P-02.3, landed |
| a scope's construction | every scope: a page's, a form's, a pattern cell's, an appearance's, the font rule's | every font its `/Font` names, read with its CMaps (`stats::fonts`), used or not; and `for_one_render` and `bind` copy the two maps it filled for every render and every counting asker | P-02.3, landed |
| `color_space`, `selected_space` | each `cs` and `CS` that names a resource, three readings; each `sc` and `scn` in one, two | `/ColorSpace`, the entry and its parse | P-02.5, landed |
| `function` | each `sh`, each shading pattern's fill | the shading's `/Function`, read and parsed | P-02.5, landed |
| `ext_g_state` | each `gs` | nothing: kept by name already (`Caches::states`) | P-02.5 counts it, landed |

#### The font table

**The key** is the name a content selects a font by — `Tf`'s operand, or the
name `fonts::state_font` gives a graphics state's `/Font` — as bytes, the form
every `FontSource` question already arrives in. **Its life** is the scope's:
the table lives in `Caches`, so `for_one_render` and `bind`, which hand a
render or an asker the scope over its caches, share it rather than copy it; a
retained page's renders read each font once between them, and a form's kept
scope (`form_scopes`) keeps its own table, shared by every asker it is handed
to.

**An entry** is read on the first question about its name, and kept:

- for a name the scope's `/Font` binds to a dictionary: the name's id
  (`u64::from(name.id())`, the id the interpreter's glyphs carry), the entry's
  reference — none for a font written inline — the dictionary as resolved,
  which for an indirect font is the document's own object, so nothing is
  copied, and the `Font`, read by `cos_font::read_in` through the scope's view,
  the counting one when the operation counts;
- for a reserved name (`fonts::reserved_reference`): the same, its id
  `fonts::reserved_id` and its reference the object it spells;
- for any other name, or one whose entry is no dictionary: nothing, kept as
  firmly, so a missing font is looked for once.

Beside the entries by name, the table keeps each entry by its id, which is what
`font_object`, `selection`, `program` and the renderer's outlines are asked
by. An id the table does not know yet was handed out by another scope over the
same dictionary — an appearance is drawn over two, the interpreter's and the
renderer's (`annots::draw_prepared`), and the renderer's is asked for outlines
by the ids the interpreter's gave — so it is looked for among the scope's
`/Font` names, whose keys carry the ids, and the font it names read on that
first question; a reserved name's id answers by the object it spells, and an
id no name carries answers nothing, kept as firmly. Every font `/Font` binds and every
reserved name a state gives is kept, which the dictionaries bound; the names
and ids that select nothing are kept while the table holds fewer than 4 096 of
each, and past that each is looked for again at each question.

**What reads it.**

| Question | Reads |
| --- | --- |
| `decode`, `is_vertical`, `vertical_metrics`, `font_name` | the entry's `Font` |
| `font_id` | the entry's id; a reserved name's id where it has no entry, as now; else 0 |
| `type3_glyph` | the entry's kind first: a glyph of any other font is answered from it, with nothing read or copied |
| `font_object` | the entry's reference, by id |
| `selection`, `program`, `extract_outline` | the entry by id: its name, its `Font`, and for a program its dictionary — the embedded program's descriptor and a substitute's request (`FontRequest::read`) read from it, not from a second lookup |
| `font_objects`, `direct_fonts` | the `/Font` subdictionary itself, unchanged: they list every font a scope names, used or not, which is what lets subsetting say a scope was walked and never showed a font |

**The kind** an entry records is the one `type3_glyph` reads: whether the
dictionary's own `/Subtype` is the name `/Type3`. It is not `Font::kind`, which
also follows a `/Subtype` written as a reference, so no glyph's answer moves.

#### The Type 3 record

A Type 3 font's entry is given its record on the font's first glyph, once a
scope whichever render asks:

- **The codes.** `/Encoding`'s `/Differences`, read with the interpreter's
  reading, item by item from the first: a counter starts at 0; an integer sets
  it (`u32::try_from`, so a negative one and one past `u32::MAX` set 0); a real
  sets it as `as u32` converts it (a negative one and NaN 0, one past the range
  `u32::MAX`); a name is given to the counter's code, and **the first name a
  code is given is the one it keeps**; the counter then steps by one, held at
  `u32::MAX`; every item is resolved before it is read, and an item of any
  other type is passed over. Names are kept as the array holds them, the
  symbols of their bytes, never as text. That is not the cos `Font`'s table,
  which lets a later entry win, passes over an indirect item and keeps lossy
  `String`s; and the reading `subset.rs`'s `names_of` and `redact.rs`'s
  `carrying_procedures` mirror is the interpreter's, which does not change, so
  they keep agreeing.
- **`/CharProcs`**, resolved, and **`/FontMatrix`**'s six numbers as
  `type3_glyph` reads them — each a direct number, or no matrix.

A glyph then costs one probe of the codes and one lookup in `/CharProcs`, and
its procedure is decoded as before. An `/Encoding` that is no dictionary, a
`/Differences` that is no array and a `/CharProcs` that is no dictionary leave
a record that answers every code with nothing, as the walk did. The walk is
kept test-only, and `the_type3_table_answers_what_the_walk_answered` holds the
record to it.

#### Colour spaces, functions and graphics states

- **A colour space, by name.** `named_space` answers every question a
  resource name asks of its space, and `Caches::spaces` keeps each name's two
  readings — as painted, which every conversion reads, and as selected, which
  `resolve_ink`, `initial_color` and `color_components` read: K-02.1's two
  answers to one selection — read together at the first question about the
  name (`PageResources::kept_space`), with the repairs reading them noted, and
  lent to each question rather than copied. A scope whose defaults put nothing
  in reads the entry once, the two readings being one. A name `/ColorSpace`
  does not hold is kept too, with Table 93's abbreviation it may be, so it is
  looked for once. A device space named directly is matched by its name and
  kept nowhere. `Caches::icc` keeps compiled profiles by stream and stays the
  one ICC cache.
- **A function, by object.** `PageResources::kept_function` keeps each
  function an object holds in `Caches::functions` — a shading's `/Function`
  and its members, a tint transform, a stitching function's pieces and the
  graphics states' functions alike — keyed by its object. A stitching
  function is keyed by its object and the depth it is read at, because its
  pieces nested past the reader's depth of eight read as nothing, so its
  reading depends on where it stands; every other function reads alike at
  every depth. A function written inline is a few numbers, read where it
  lies; a shading or a space that holds a kept function copies it, a graphics
  state shares it.
- **A graphics state, by name.** `Caches::states` keeps each `gs` name's state
  and the repairs its reading noted, and stays as it is.

Each keeps at most 4 096 entries, as `Caches::states` does.

#### What a hit says again

A render's lists — the fonts it could not resolve, the images that decoded with
damage, the colour spaces read with a repair — are that render's own
(`for_one_render`), so a cache shared between renders says again what its miss
said, or a render that hits would say less than one that misses.

- **A font**: what reading it tolerated, a CMap's leniencies, is the
  document's (`Document::warnings`), raised by the read; a render's own list
  hears of fonts only from `decode`, which names a name no font answers at
  every question, hit or miss. Nothing is kept beside an entry.
- **A Type 3 record** says nothing; its procedure's decode raises what that
  stream's decode raises at each glyph, as now.
- **A colour space**: the repairs its reading noted (`note_repaired_space`)
  are kept beside it and noted again at each hit, as `Caches::states` notes a
  state's, so a render's lists do not depend on whether it hit.
- **A function** says nothing. **A graphics state** says its repairs again, as
  now.

#### The names the table pre-interns

`NAMES` gains every key a page of text's interpretation looks up at a glyph or
an operator, and every key reading a font looks up:

- the resource subdictionaries: `/Font`, `/XObject`, `/ColorSpace`,
  `/ExtGState`, `/Pattern`, `/Shading` and `/Properties`;
- `/Subtype`;
- a font's: `/BaseFont`, `/Encoding`, `/BaseEncoding`, `/Differences`,
  `/FirstChar`, `/Widths`, `/ToUnicode`, `/FontDescriptor`, `/MissingWidth`,
  `/Flags`, `/FontFile`, `/FontFile2`, `/FontFile3`, `/DescendantFonts`,
  `/DW`, `/DW2`, `/W2` and `/CIDToGIDMap`, and a CMap stream's `/UseCMap`
  (`/W` is pre-interned already, for cross-reference streams);
- a Type 3 font's: `/CharProcs` and `/FontMatrix`.

`resources.rs`, `fonts.rs` and cos's `font.rs` take them as `Name` constants.
Every other name a scope looks up by its bytes — a resource name the content
gives, and a key read inside an XObject, a colour space, a shading, a pattern
or an inline image — goes through the scope's one helper, `PageResources::name`,
and the font helpers in `fonts.rs` the scope calls take its view.

**Ids shift.** Pre-interning moves the id of every name a document interns
after the table's own, and a font's id is its name's. No output carries one: the
text device compares two glyphs' ids for equality, no `TextChar` carries one,
no serialization writes one, and every fingerprint hashes pixels, text or
written bytes. Equality survives the shift, and so does the order of two names
interned after the table's.

**The counter.** `keys_looked_up` counts each lookup by its bytes of a name the
table pre-interns — a lookup a constant answers — through `PageResources::name`
and through the counting view (`Counting::intern`), so a font read through the
view is seen too. With every reader on those paths taking the constants it
reads zero, and the name-table lookups the text of a page whose only resources
are its fonts makes are then the resource names its content uses. A resource
name the table pre-interns is counted as well — a font named `/Font` — which is
how the guard's second page shows the counter sees a lookup where there is one.

#### A3's counted quantity, and the counters the row adds

A3 reads `font_dictionaries_resolved` on the render door and the text door for
one page and holds it to the distinct `/Font` entries the content selects a
font by, each scope's counted apart — the page's, and each form's that brings
its own `/Resources`. Its page names a font it never selects, gives one font two
names, and draws twice a form whose own `/Resources` name the page's font
again: three entries used, the page's two names and the form's one, whatever
the glyphs. At the tip the same page counts every font of every scope when the
scope is built and one more for every glyph.

| Counter | One unit is | Counted in | Step |
| --- | --- | --- | --- |
| `keys_looked_up` | a lookup by its bytes of a name the table pre-interns | the facade's scopes, and the counting view | P-02.2 |
| `font_dictionaries_resolved` | restated: a font dictionary a scope reads, once for each name its content selects a font by — the `/Font` entry, or a reserved name's object — when it is a dictionary | the facade's scopes | P-02.3 |
| `color_spaces_parsed` | a resource name's colour space a scope reads out of `/ColorSpace` and parses, both its readings at once | the facade's scopes | P-02.5 |
| `functions_read` | a function an object holds, read and parsed for a shading or a graphics state | the facade's scopes | P-02.5 |
| `graphics_states_read` | a `gs` name's state read out of `/ExtGState` | the facade's scopes | P-02.5 |
| `pages_interpreted` | a page's content interpreted for an inference (`Observed::read`), the page's own and a neighbour's | `reading_order.rs` | P-02.7 |

Each is a step, in `Stats::work`, and A2's legs hold them as they hold the rest.

#### The structured-text writer's numbers

A finite value is written as `format!("{value:.3}")` with its trailing zeros,
its point and a `-0`'s sign trimmed — the bytes `number`, a `String` for each
of the ten numbers a character carries, used to make — and `write_number`
writes them straight into the output. A double is its significand over a
power of two, so for a magnitude below 2^52 a thousand times the significand,
shifted down by that power, is the whole number of thousandths, and the bits
shifted out, against half of what the last place is worth, decide the rounding
exactly (`thousandths`); the digits are written from that integer. A tie at the
fourth decimal — an odd number of sixteenths, the only doubles a thousand times
which ends in exactly a half — and a magnitude of 2^52 or more are written by
the formatter itself into the output and trimmed there. Two of a line's sizes
are compared as they print: two values whose thousandths are decided print
alike exactly when those are equal, and any other pair is written into stack
buffers and compared. So no number allocates and none is decided differently;
`numbers_write_what_format_and_trim_wrote` holds every path to the old `number`,
kept test-only, and `text_allocations.rs`
`a_character_writes_its_numbers_without_allocating` counts what a character
costs. `TEXT_FORMAT_VERSION` does not move, because no byte does.

#### Batch inference

`Document::inferred_orders` reads each page once. A batch (`Batch` in
`reading_order.rs`) walks its range in order and keeps what one interpretation
of a page gave (`Observed`): its text for the page's own inference, and its
margin bands for its neighbours'. A page ahead of the one being inferred is
read when a neighbour first needs its bands and its observation kept until its
own turn, at most half of `RUNNING_WINDOW` of them; a page behind keeps only
its bands, until no page still to come is near enough to read them. What a
page's inference decides does not change, only how often a page is read, so
every `InferredOrder` is `Page::inferred_order`'s for the page: that one is a
batch of one page, which reads the page and its window. `Document::inferred_pages`
hands the batch's orders out a page at a time, an iterator, and
`inferred_orders` collects it. `pages_interpreted` counts each interpretation,
through `Document::inferred_orders_counted`, whose interpretations count what
they read and decode too: a batch over a range is its pages and the neighbours
outside it, each once. `tpdf text --order inferred` infers each run of
consecutive pages it lists through `inferred_pages`, writing each page as it
comes, and a document whose structure tree binds keeps `text_in`'s answer, its
stated order, page by page.

#### Each operation's path, and the step that brings it within bound

Read off MS-04's report-mode figures for the set as MS-04.11 sized it, taken on
the hosted runner; the factors themselves are held against `61adcde` in
P-02.8's paired run, and a relation is two operations of one run.

| Operation | Path | What the tip spends that this row removes | Step |
| --- | --- | --- | --- |
| Text of 5 dense pages, in-process and CLI | P-01's lookup, the content's decode, one interpretation into the text device through the page's scope, logical order; the CLI adds a process and the writes | at every glyph, a copy of the font's dictionary with its `/Widths`, and three lookups by bytes, to learn the font is not a Type 3 font | P-02.3, with P-02.2's keys |
| Display-list recording, a text page | the same interpretation into the recorder | the same, at every glyph | P-02.3 |
| Structured JSON of 5 pages, CLI | the text, then `TextWriter`'s JSON | the text's, and ten `String`s a character; the figures leave the writer too large a share of the operation for the text's steps alone to reach its factor, and the text too large a share for the writer's | P-02.3 for the text and P-02.6 for the writer, both |
| A phrase searched over 5 pages from scratch | the text, then `TextPage::search` | the text's; the figures put the search itself at a small part of the operation | P-02.3 |
| Redaction of a rectangle per page ≤ 1.5 × text | `redact::apply`'s own font reading and content walk over the editor (`fonts_in`, `cut_stream`): no `PageResources` | — | none: inside its bound at the tip, with room for the text of the same pages to grow the row's whole text factor faster |
| Reading order inferred ≤ 1.5 × text | `Observed::read`, one interpretation through a page's scope, for each page and again for each neighbour whose margin bands it reads | over 5 pages ten interpretations, each page's own and a second of every page as a neighbour; past its bound at the tip, the inference's own work a small part of it | P-02.7: one interpretation a page |
| Tables inferred ≤ 1.5 × text | `Observed::read` once, then the table inference | one interpretation, the text's own path | none: inside at the tip, and every step that makes the text faster makes its interpretation faster by the same work |
| PDF/A, PDF/UA and PDF/X validation ≤ 3 × text | their rules' own walks (`pdfa/content.rs`), and the font rule's scopes, one a font | — | none: inside at the tip, each with room for the row's whole text factor |

So every relation is inside its bound at the tip or brought inside by a step,
and none is raised against the row. The CLI rewrite of 100 and 1 000 pages,
which tier P's Gate column gives P-01 and P-02 together, reads no resource scope
(`tpdf merge` with `--font-policy keep`): no step of this row moves it, and
P-02.8 raises its floors on what P-01 and P-03 left.

#### The CMap warnings a page's unused fonts no longer raise

With fonts read on first use, a font a scope names and its content never
selects is not read by a render, a text extraction, an image extraction, a
validation's font scopes or a save's subset walk, so the leniencies its CMaps
need — `PredefinedCMapUnknown`, `PredefinedCMapApproximate`, and the `CMap`
warnings against its streams — are no longer raised into `Document::warnings`
on its account. A font the content selects is read at its first glyph and warns
as it did, after whatever the content raised before it, where building the
scope raised it first. `Document::fonts` reads no CMap and never raised them.
None of the suites that assert those warnings reads one from a font its page
does not use, so none is dropped: `predefined_cmap_warnings.rs`,
`predefined_cmap_rendering.rs` and `vertical_metrics.rs` show their one font,
`shaped_forms.rs`'s are the fill's, which cos's appearance writer reads, and
cos's `fonts.rs` reads its fonts through `cos_font::from_resources` itself.

### Milestones

1. **P-02.1: this section.**
2. **P-02.2: the keys as constants**, and `keys_looked_up`. Exit: `name.rs`
   `preinterned_ids_match_constants` covers every new constant;
   `resource_work.rs` `extracting_a_page_looks_up_no_well_known_key` — a dense
   page's text, through `Page::text_counted`, looks up no key, and a page whose
   font is named `/Font` shows the count sees one; every fingerprint,
   `text_options.rs` and `display_list.rs` unchanged, in both builds.
3. **P-02.3: the font table.** Exit: `guards.rs`
   `a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses` flips from owed
   and joins K-07's roster; `stats.rs`'s font counts restated to one an entry
   used; `text_options.rs`, `type3_fonts.rs`, `composite_fonts.rs`, every
   subset test and every fingerprint unchanged, in both builds; the CMap-warning
   suites above keep their warnings.
4. **P-02.4: the Type 3 record.** Exit: `resources.rs`
   `the_type3_table_answers_what_the_walk_answered` and
   `differences_are_read_once_per_scope`; `type3_fonts.rs`,
   `render_differential.rs`'s `a_type3_glyph_draws_what_the_same_path_draws`,
   and subsetting's and redaction's Type 3 tests unchanged, in both builds.
5. **P-02.5: colour spaces, functions and graphics states.** Exit:
   `resource_work.rs` `a_scope_parses_each_named_space_and_state_once`, over a
   page of a thousand `cs`, `scn`, `sh` and `gs` operators, with the three
   counters at most the distinct names and objects used; `colour_spaces.rs`,
   `blend_modes.rs`, `shading_functions.rs`, `soft_mask_clips.rs`,
   `render_regions.rs` and every fingerprint unchanged, in both builds.
6. **P-02.6: numbers without allocating.** Exit: `serialize.rs`
   `numbers_write_what_format_and_trim_wrote` against the old `number`, kept
   test-only; `text_allocations.rs`
   `a_character_writes_its_numbers_without_allocating`; `text_serialize.rs` and
   the CLI's `--json`, `--xml` and `--html` tests unchanged, in both builds.
7. **P-02.7: one interpretation a page.** Exit: `reading_order.rs`
   `batch_inference_interprets_each_page_once` — `pages_interpreted` is 40 over
   40 pages, every `InferredOrder` `Page::inferred_order`'s; the reading-order
   tests and the CLI's `--order inferred` tests unchanged, in both builds.
8. **P-02.8: the row's close** in MS-02's paired run: text of 5 dense pages at
   7.51× in-process and 5.21× CLI, display-list recording at 5.25×, structured
   JSON of 5 pages (CLI) at 9.63×, a phrase searched from scratch at 6.56×, the
   CLI rewrite of 100 and 1 000 pages at 1.85× and 11.1×, and the six relations
   holding; A3 and the row's counted guards green on linux and windows in CI's
   tests and on `wasm32-wasip1` locally.

### Risks

| Risk | Mitigation |
| --- | --- |
| A font read on first use answers differently from one read when the scope was built | The same reader (`cos_font::read_in`) over the same dictionary, through the same view; `text_options.rs`, `type3_fonts.rs`, `composite_fonts.rs` and every fingerprint unchanged |
| An id the table does not know reaches `font_object`, `selection` or `program` | It is looked for among the scope's `/Font` names, whose keys carry the ids, once; a reserved id still answers by its object; every subset test and the annotation suites unchanged |
| Subsetting stops seeing a font a scope names and never shows | `font_objects` and `direct_fonts` read the `/Font` subdictionary itself, unchanged |
| A hostile content names a million fonts that are not there | Names that select nothing are kept only while the table holds fewer than 4 096 entries |
| The Type 3 record and the walk disagree on an odd `/Differences` | `the_type3_table_answers_what_the_walk_answered` over generated arrays with repeated codes, negative and real counters and indirect items, against the walk kept test-only |
| A hit says less than its miss | A colour space's repairs are kept beside it and noted at each hit, as a graphics state's are; a font's leniencies are the document's, raised by the one read |
| Warnings a caller saw from a font its page never used stop | Recorded above; no suite asserts one, and `Document::fonts` lists every font without reading its CMaps |
| An output carries a font id the pre-interning moves | None does; every fingerprint, `text_options.rs` and `display_list.rs` unchanged |
| The factors are not reached by the six steps | P-02.8's paired run reads every operation against its floor; a floor the steps do not reach is the owner's (H-77), with the counted work beside it, never a lower floor |

## P-03 — A decoded-stream cache per operation, and deduplication on encoded bytes

*For [ROADMAP](../ROADMAP.md) row P-03.* An operation decodes one stream as
many times as it is reached: a font program once for every page's scope that
draws it, a form's content on every `Do`, a Type 3 procedure for every glyph,
and a merge's deduplication every candidate stream for its digest and again
for each comparison. This section fixes who holds the decoded bytes for an
operation and until when, what deduplication buckets and compares, what the
owner's answer to H-137 (ruling 23) leaves a save merging, and what A5 and A6
count.

### Scope

- **The operation-scoped decoded-stream cache** (P-03.2): who owns it for a
  render, a text extraction, an image extraction, a save and a merge; its
  key; what it holds and until when.
- **The hash** (P-03.3): XXH64, moved into `tinker-pdf-filters`.
- **Deduplication on encoded bytes** (P-03.4): the bucket key, the
  byte-for-byte confirmation, and a decode at most once a save.
- **H-137** (P-03.5, P-03.6): what each answer costs and keeps, and the
  answer ruling 23 gives.
- **A5's and A6's counted quantities**, and MS-05's merge band (P-03.7).

### Non-goals

- **A cache that outlives an operation**, or one held to a byte budget:
  P-16's.
- **Inflate speed**: P-12's. This row decodes less, not faster.
- **Resolving a resource once per scope**: P-02's. A scope's font table and
  its colour spaces and functions are read once by P-02; this row is about
  the bytes behind them.

### Design

#### What is decoded more than once at the tip

| Stream | Where it is decoded | Operation | Decodes at the tip | Step |
| --- | --- | --- | --- | --- |
| A font program | `PageResources::program` keeps it per scope (`Caches::programs`, `crates/tinker-pdf/src/resources.rs`); `program_for` decodes it for each scope that draws a glyph of it | a subset save over *n* pages drawing one face; a render or text of a page whose forms draw the page's fonts | one a scope: *n* over the save's pages, and once more when the cut reads it | P-03.2, landed |
| A font's CMaps (`/ToUnicode`, an embedded `/Encoding` CMap and its `usecmap` parents) | `read_cmap` (`crates/tinker-pdf-cos/src/font.rs`), for each font each scope reads | text or a render of a page whose forms use its fonts; a subset save, a scope a page | one a scope | P-03.2, landed |
| A form's content | `PageResources::form_from` (`resources.rs`), on every `Do`, and for a soft mask's `/G` once for each name a scope's `gs` sets it by (`Caches::states`) | a render or text of a page showing one form *k* times | *k* | P-03.2, landed |
| A Type 3 glyph procedure | `PageResources::type3_glyph` (`resources.rs`), for every glyph shown | a render of a page showing one Type 3 glyph *g* times | *g*, which P-02.4 leaves here | P-03.2, landed |
| A deduplication candidate | `dedup::deduplicate` (`crates/tinker-pdf-cos/src/dedup.rs`): once for its digest, and once more for each comparison in a bucket of two or more, every round | a rewrite with `deduplicate_streams`, which every merge is | two a round in a shared bucket | P-03.4 |

#### The cache: who holds it, and its key

One cache an operation, `tinker_pdf_cos::DecodedStreams`, which runs a chain
on a stream's first read and keeps what it ran, read through a `Resolve`
whose `stream_decoded` consults it (`tinker_pdf_cos::Cached`). The facade's
`resources::Operation` carries it with the operation's tally into every scope
the operation reads: `PageResources`, its nested form and pattern scopes, and
the font reader they call (`cos_font::read_in`, handed the `Cached` view over
the counting one), so a CMap or a program a nested scope reads is the one the
page's scope read.

- **A render, a text extraction and an image extraction** hold one for their
  page, made beside the operation's tally and dropped when the door returns;
  `Page::render_form` reads the form it is asked for in the render's, and an
  annotation's two scopes read in the render that draws it. A retained page's
  resources, shared by its renders, never hold one: two renders are two
  operations. A form's scope a scope's cache keeps holds none either, and is
  handed to each asker bound to the asker's (`PageResources::bind`).
- **A save** holds one across `subset::collect`'s pages and
  `subset::rewrite`'s reads of the same programs. A scope's
  `Caches::programs` still keeps each of its fonts' programs by the id its
  glyphs carry, so that no glyph asks again, and no other scope shares it,
  since its key is a resource name that two pages may give to two fonts; what
  it keeps is the operation's decode, by the stream.
- **A merge** is imports and a save: an import copies stored bytes and
  decodes nothing, and the save's deduplication keeps its own map of what it
  decoded (P-03.4), since it reads the object set it is about to write and
  not a document.

**The key is the stream's reference in the document the operation reads.**
A save's passes read the editor's view, whose numbers are the editor's, so a
stream the editor replaced is the editor's stream under its number and is
never served from a decode of the file's. The cut reads through the editor,
and a stream the editor has written or deleted
(`DocumentEditor::has_changed`) — a program the cut replaced, a procedure it
emptied — is read from the editor, never from the cache; any other is the
file's, which is what the view's untouched objects are. Keyed by stream, never
by region or by page, so a tile and its page hit alike (ruling 5).

**What it holds**: each stream's decoded bytes, behind an `Arc` so a hit
hands out a reference rather than a copy, or its chain's refusal, and nothing
else. The first decode raises the chain's warnings and a hit raises none, as a
second read of a cached object raises none. An image's samples are not held:
the scope's image cache already keeps each decoded image, and holding its
samples too would double the image in the peak MS-05 bands. Nor is a page's
own content, which the door decodes once, before any scope exists, and which
a save's walk would otherwise hold for every page until its end. **Until
when**: the operation's end. The bytes held are at most the decoded size of
every stream the operation reads; a byte budget is P-16's.

#### Deduplication: the bucket key, and the confirmation

A candidate's bucket is its **comparable dictionary's bytes** (without
`/Length`), which carry its filter chain, its **encoded length**, and **XXH64
of its encoded bytes**. The hash is XXH64, `tinker_pdf_filters::xxh64`, the
one the archive's Zstandard frame checksum carries, which both
`tinker-pdf-cos` and the archive reach there.

**The hash only buckets.** Two streams of one bucket merge when their
comparable dictionaries are equal and their encoded bytes are equal byte for
byte — undecoded. Equality is decided by bytes, as `dedup.rs`'s module rule
says, so a colliding hash costs a comparison and never a wrong merge
(`a_colliding_digest_never_merges_different_bytes`, whose injected digest
collides on everything).

**What is decoded, and how often.** Under ruling 23's answer to H-137, a
stream whose comparable dictionary matches another's and whose encoding
differs is compared decoded; each such stream is decoded at most once a save,
its decoded bytes kept in the pass's map until the pass ends, and counted on
the save door. A byte-identical copy merges without a decode, and no other
stream is decoded. Every round after the first reads the map, since redirecting
references changes dictionaries and never content.

**The whole-set copy and each round's rebuild stay.** The pass copies the
object set once (`deduplicate`'s `objects.clone()`) and rebuilds it in each
round that merged something; each moves every stream's stored bytes once,
linear in the output and memory-bound. A merge of four parts that share a
face merges in its first round and finds nothing in its second, so it makes
one copy and one rebuild: two passes over the merged file's bytes, beside
which the per-stream decodes this row removes are the merge's cost. A later
round runs only when a merge made two dictionaries equal.

#### H-137: what each answer keeps, and the answer

| Answer | The merges a save still makes | What A6 counts |
| --- | --- | --- |
| **Merge on encoded bytes alone** | every pair whose comparable dictionaries and encoded bytes are equal: a face, an image or a form imported once a page or a part; never two encodings of one content, so `two_encodings_of_one_content_merge` would become `two_encodings_of_one_content_stay_two` | a decode only when two different encodings share a bucket, which is a 64-bit hash collision at one length; over a merge of 4 × 100 pages whose encodings all differ, none |
| **Keep merging two encodings of one content** (ruling 23's answer) | those, and every pair whose dictionaries match and whose decoded bytes are equal | one decode a save of every stream whose comparable dictionary another shares with a different encoding — every page's content stream of a merge among them — and none of a byte-identical copy |

**The answer** (ruling 23): byte-identical encodings merge without decoding,
and differing encodings keep the decoded comparison, decoded once a save, so
`two_encodings_of_one_content_merge` holds. A6's clause and the merge floors
state that mechanism: A6 holds it over the guard suite's merge
(`a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once`),
and P-03.7 holds the merge of 4 × 50 pages to its floors with it.

**The price** (P-03.5). `dedup.rs`'s ignored
`h137_prices_the_decoded_comparison_kept_and_dropped` runs the pass both ways,
through a parameter only that module's tests reach, over a merge of four
100-page documents built as MS-04's generator builds them and over every
committed PDF and the fixture `two_encodings_of_one_content_merge` builds,
and prints a row for each input whose two modes differ, the merge always.
Each figure is kept / dropped; the time is the median of five runs of the
pass and the rewrite of its result:

| Input | Decodes | Merges | Merges lost | Output bytes | Release (ms) | Debug (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| merge of 4 × 100 pages | 0 / 0 | 300 / 300 | 0 | 4 255 883 / 4 255 883 | 94.2 / 95.1 | 1 994 / 1 979 |
| `two_encodings_of_one_content_merge`'s fixture | 2 / 0 | 1 / 0 | 1 | 298 / 1 158 | 0.02 / 0.01 | 0.20 / 0.10 |

None of the forty committed PDFs merges two encodings of one content. The
merge pays nothing for the decoded comparison: the builder writes page
contents and font programs without a filter, so comparing them runs no
chain, and the 300 copies of the face its imports make are one encoding,
merged undecoded. Regenerate with `cargo test -p tinker-pdf-cos --lib h137 --
--ignored --nocapture`, and again with `--release`.

#### A5's and A6's counted quantities

A5 reads `most_decodes_of_one_stream`, the most runs of one stream's filter
chain in the operation, from MS-01's per-stream decode counts on each door,
and holds it to one: a save over a 100-page document drawing one embedded
face with a `/ToUnicode` on every page; a render, a text extraction and an
image extraction through the decode door of a page showing one form three
times and one Type 3 glyph fifty times; and a merge's save (P-03.4). Each leg
but the merge's holds `streams_decoded` and `bytes_decoded` too, at the
streams its input holds and their decoded lengths, derived from the
generator. A6 reads
`streams_decoded`, `most_decodes_of_one_stream` and `bytes_decoded` on the
save of a merge of the guard suite's four 100-page A6 documents, whose faces
are one encoded stream and whose pages' contents all differ, and holds them to
ruling 23's mechanism: each encoding that differs under the dictionary the
pages' contents share with the face's program is decoded once — the 400
contents and the face, 401 — and none of the 300 copies of the face the
imports make, which the bytes decoded show by holding the face once. The
saved file keeps one program for the face and a content stream a page.

#### MS-05's merge band

MS-05's merge 4 × 100 case is set at the figure the fixed merge peaks at,
because the row names none (P-03.7): the set copy and the rebuild are its
largest allocations, and the decoded map holds only the streams ruling 23
compares decoded.

### Milestones

1. **P-03.2: the operation-scoped cache.** Exit: `guards.rs`
   `a5_a_save_decodes_each_stream_at_most_once`,
   `a5_a_render_decodes_each_stream_at_most_once`,
   `a5_text_decodes_each_stream_at_most_once` and
   `a5_image_extraction_decodes_each_stream_at_most_once` flip from owed;
   every subset test, `cff_subset_census.rs`'s fixtures, `write.rs`'s save
   tests, `type3_fonts.rs`, `render_regions.rs` and every fingerprint
   unchanged, in both builds.
2. **P-03.3: XXH64 in `tinker-pdf-filters`.** Exit:
   `xxh64_keeps_the_values_the_archive_copy_gave`; the Zstandard
   frame-checksum tests unchanged.
3. **P-03.4: buckets on encoded bytes.** Exit: `dedup.rs`
   `byte_identical_encodings_merge_undecoded`;
   `a5_a_merge_decodes_each_stream_at_most_once` flips from owed;
   `a_colliding_digest_never_merges_different_bytes` keeps its assertion, and
   every `stream_dedup.rs` test passes unchanged.
4. **P-03.5: H-137 priced.** Exit: the `#[ignore]`d pricing test in
   `dedup.rs` with its `RAN` banner, and its table committed here.
5. **P-03.6: the owner's answer** (ruling 23). Exit:
   `a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once`
   flips from owed, holding the mechanism the answer keeps, and
   `two_encodings_of_one_content_merge` stays.
6. **P-03.7: the row's close** in MS-02's paired run: subset-on-save of 4
   pages (CLI) at 10×, merge of 4 × 50 pages — byte-identical encodings
   merged undecoded, differing ones compared decoded once a save — at 25.2×
   CLI and 114× in-process, MS-05's merge band at the fixed figure, and A5
   and A6 green on every target.

### Risks

| Risk | Mitigation |
| --- | --- |
| The cache serves a stream the editor replaced | Keyed by reference in the document the operation reads, which for a save is the editor's view; the cut reads a stream the editor has written or deleted from the editor (`DocumentEditor::has_changed`) |
| A hit hides a warning a caller expects | The miss raises the chain's warnings, as a cached object's first read does |
| The cache raises an operation's peak | An image's samples are not held, and the operation drops it when it returns; MS-05's bands hold the cases it touches |
| A hash collision merges two different streams | The hash only buckets; equality is decided by bytes |
| The decoded comparison ruling 23 keeps costs a merge a decode of every page's content stream | Each is decoded once a save and its bytes held only until the pass ends (A5, A6); P-03.7 holds the merge floors with it, and P-03.5's price shows MS-04's merge running no chain for it, its contents and programs written unfiltered |

## P-04 — PNG and deflate

*For [ROADMAP](../ROADMAP.md) row P-04.* At the planning baseline the
encoder is one fixed-Huffman block over the whole input, its match finder
holds one `usize` per input byte, and `deflate` writes a stored copy of the
whole input beside it to keep the smaller; the PNG writer filters the whole
image into one buffer, compresses that, and copies the compressed stream into
its chunks. This section fixes the
encoder's two levels, the match finder's ring and how its positions are held,
dynamic-Huffman blocks and each block's choice, the streaming interface and
the window it keeps, the PNG writer's one pass and its chunks as the deflater
emits them, A8 and the scratch bound, how the ratio level is held to level
6's sizes, and which committed hashes the one change of written bytes moves.

### Scope

- **The two levels** (P-04.3, P-04.4): a ratio level for document writing
  and a fast level for rendered output.
- **The match finder's ring** (P-04.2), and the width of a position.
- **Dynamic-Huffman blocks** (P-04.4), each written dynamic, fixed or stored.
- **The streaming interface** (P-04.3), and the window it keeps.
- **The PNG writer's one pass** and IDAT as the deflater emits (P-04.5).
- **A8 and the scratch bound**, the level-6 size bound, and the written-bytes
  hashes P-04.7 moves.

### Non-goals

- **Inflate**: P-12's.
- **PNG palettes and interlace**: `png/encode.rs`'s header argues both away,
  and nothing here reopens it.
- **Threads**: P-15's. One deflater is one thread's.

### Design

#### The two levels

Both are hash-chain LZ77 over the ring below, with the same hash, the same
window and the same insertion of every position a match covers; they differ
in how far a position searches and whether it looks one byte on. The numbers
are zlib's published parameters for its levels 6 and 1, taken as numbers:
no outside program runs in the tree or decides a build (ruling 13).

| Level | Chain bound | Search ends at a match of | Lazy step | Used by |
| --- | --- | --- | --- | --- |
| **Ratio** | 128, a quarter of it once the match in hand reaches 8 | 128 | a match shorter than 16 is held while the next position searches, and the longer wins (a longer match one byte on is taken, and the byte before it is a literal); a three-byte match more than 4 096 back is dropped | document writing (`zlib_compress`, `deflate`), from P-04.7 |
| **Fast** | 4 | 8 | none: greedy | rendered output (PNG), from P-04.5, as H-42's default |

The chain bound is the hostile-input bound `MAX_CHAIN` already argues: the
input is the caller's, and the bound caps a position's work whatever it is.

#### The match finder's ring, and the width of a position

`head` keeps, for each of the 32 768 hash buckets, the newest position that
hashed there; **the ring** keeps, for each position, the position before it in
its bucket, at slot `position mod 32 768`. Both are 32 768 entries, so the
finder's scratch is the same for any input length (P-04.2), in place of
`prev`'s one entry per input byte. A slot is overwritten only by the position
a whole window later, and the walk stops at the first candidate further back
than the window, which the window test already rejects — so every slot the
walk reads holds the predecessor of the candidate it reached, and the ring
visits the candidates `prev` visits, in the same order.

**A position is a `u32` offset from a base**, which keeps the tables at
256 KiB together. An offset never reaches `2^31`: when the next one would,
the base moves forward by `2^31 − 32 768`, every entry of `head` and the ring
has that subtracted, and an entry the move takes below zero — older than the
window, so no walk could take it — becomes the empty sentinel `u32::MAX`,
which no offset below `2^31` equals. The move runs once in two gigabytes of
input, so a stream of any length never wraps, and the 64 Ki entries it
rewrites cost nothing a byte. P-04.2 lands the ring over the whole input,
where an offset is the input position until the first move.

#### Dynamic-Huffman blocks, and each block's choice

A block ends every 16 384 symbols — literals and matches — or at the end of
the input, so its boundaries are a function of the input alone and never of
how the input arrived (ruling 4). Each block is written whichever of three is
smallest, counted in bits before any is written, ties going to the simpler:

- **stored**, the block's bytes verbatim, five bytes a 65 535 and its
  alignment — only when the input it covers is still in the window buffer;
- **fixed**, RFC 1951 3.2.6's codes;
- **dynamic** (3.2.7), code lengths for the literal/length and distance
  alphabets from the block's symbol counts, limited to 15 bits by
  package-merge, which is optimal under the limit and breaks ties by symbol
  order; the code-length alphabet run-length coded with 16, 17 and 18 and
  limited to 7 bits; the header's counts trimmed of trailing zeros.

Fixed codes alone cannot reach level 6's sizes: the module's header puts
dynamic tables at about ten per cent, so dynamic blocks and lazy matching
carry the bound together (P-04.4).

#### The streaming interface, and the window it keeps

A `Deflater` at one level takes input in pieces and appends whole blocks to
the caller's output as they complete: `write(input, out)` and
`finish(out)`, the last block final and the stream aligned; a zlib wrapper
around it keeps the header and a running Adler-32. It keeps a **window
buffer of 2 × 32 KiB**: input is copied in at its end, a position is
searched only while 262 bytes follow it — a match's reach forward, a hash's
three and the next position's — or at the input's end, and when the buffer
is full and more input waits, what lies more than a window behind the scan
slides out, so the 32 KiB a match may reach back is always held and no more
than 64 KiB of input is ever retained. Positions are absolute offsets, so a
slide moves the buffer and no table entry. Beside the window, the block's
symbols, at most 16 384 of them; the output is the caller's. P-04.3 lands it
with stored and fixed blocks, and P-04.4 adds dynamic ones.

#### The PNG writer's one pass, and IDAT as the deflater emits

A scanline's five filters are costed in one integer pass over the row: at
each byte the five filtered values are formed from `a`, `b` and `c` and each
added, as a signed magnitude, to its own sum — 12.8's minimum sum of absolute
differences, as `cost_of` reads it — so the five sums are today's five costs,
and the cheapest wins with today's tie rule, the first of None, Sub, Up,
Average and Paeth. The row is then filtered once, with that filter, and its
type byte and bytes are fed to a fast-level deflater. **IDAT is written as the
deflater emits**: a chunk is opened in the output, the deflater's bytes are
appended to it, and it is closed — its length written and its CRC-32 taken
over its type and its data — when it reaches 64 KiB and at the end. The
path holds three rows (the one above, the one read, the one filtered), the
deflater's fixed state and the output: no whole filtered image, no whole
compressed stream and no stored copy.

#### A8, and the scratch bound

`tinker-pdf-filters` counts, on the PNG path, the bytes of every buffer it
sizes from the image rather than from a row or a constant — a filtered image,
a per-byte chain, a stored copy, a compressed stream or its copy into chunks,
of which the streaming writer holds none — and hands them back as a value,
`PngHeld`, beside its peak with the file; the facade's counted PNG door
(`Bitmap::to_png_counted`) adds the copy it converts, where it converts one,
and reports both in MS-01's `Stats`, as `png_image_scratch` and `png_peak`.
**A8** holds the first to one image
(`a8_the_png_path_holds_at_most_one_image_of_scratch`), which the writer meets
with none, and the row's scratch bound, the path's peak with its output at
most 1.2 × the image's bytes, on two A4 pages at 150 dpi, 1240 × 1755, written
in the guard suite: a text page and a near-blank page, the shapes of MS-04's
two PNG inputs, where three rows and the deflater's 384 KiB are a few per
cent of the image. MS-05's band for
rendering at 300 dpi to PNG holds the process's peak (P-04.8).

#### The level-6 size bound

Tier P holds the ratio level to 1.02 × the byte counts level 6's parameters
give MS-04's page-like and photo-like inputs, exactly: a byte count on
generated inputs. P-04.4's `crates/tinker-pdf/tests/deflate_size.rs` reaches
MS-04's raw samples by path, as `tests/peak_memory.rs` reaches its generators
(`#[path = "../benches/support/samples.rs"]`), builds `page_like()` and
`photo_like()` — each asserting its own length and SHA-256 — deflates each at
the ratio level through `tinker_pdf_filters`, and asserts
`100 × len ≤ 102 × count` in integers against the two counts committed beside
the generators, `PAGE_LIKE_LEVEL_SIX.count` and `PHOTO_LIKE_LEVEL_SIX.count`
(ruling 17). No level-6 encoder runs: the counts are committed numbers.

#### The written bytes P-04.7 moves

Only P-04.7 moves written bytes: with document writing on the ratio level,
every stream the writer deflates re-encodes and decodes to what it did. The
committed hashes over written bytes that hold a stream the encoder wrote are
two, both in `crates/tinker-pdf/tests/determinism.rs`, re-recorded once after
H-138's review:

| Hash | The encoder's stream in it |
| --- | --- |
| `the_synthesised_document_is_the_same_bytes_on_every_target` | the CBZ archive's RGBA PNG, decoded and re-compressed as samples and an `/SMask` (`png_embed`) |
| `the_synthesised_fixed_document_is_the_same_bytes_on_every_target` | the XPS package's RGBA images, split into samples and an `/SMask` and re-compressed (`png_embed`) |

The other committed hashes over written bytes hold none, and do not move:
`the_synthesised_book_is_the_same_bytes_on_every_target`, whose one raster is
an RGB PNG named only by the SVG cover this build refuses;
`pdfa_writer.rs`'s `the_archival_fixture_hashes_the_same_on_every_target`,
whose builder compresses nothing; and `form_transactions.rs`'s
`a_successful_fill_produces_the_bytes_it_always_did`, whose saves are
uncompressed. No committed hash is over PNG bytes: every PNG test compares
decoded samples, so P-04.5's change of PNG bytes moves no hash.

### Milestones

1. **P-04.2: the ring.** Exit: `deflate.rs`
   `the_match_finder_keeps_a_fixed_ring_whatever_the_input_length` and
   `the_ring_finds_what_the_vector_found`; every round trip and every
   `determinism.rs` writer fingerprint unchanged.
2. **P-04.3: the streaming deflater, stored and fixed blocks, both levels.**
   Exit: `the_stream_round_trips_however_it_is_fed`,
   `the_window_is_all_the_deflater_keeps` (retained input at most the 64 KiB
   window buffer), `lazy_matching_takes_the_longer_match_one_byte_on`;
   `the_committed_baseline_agrees_with_the_tree` and
   `every_operation_has_a_baseline_side_or_a_calibration` with the fast
   level's two operations.
3. **P-04.4: dynamic blocks.** Exit: `code_lengths_are_complete_and_at_most_fifteen`,
   `a_skewed_block_is_smaller_dynamic_than_fixed`, and `deflate_size.rs`
   `the_ratio_level_holds_level_six_size`; every round trip and
   `png/encode/tests.rs` unchanged.
4. **P-04.5: the PNG writer streams.** Exit: `encode/tests.rs`
   `the_one_pass_cost_chooses_what_five_passes_chose`; `guards.rs`
   `a8_the_png_path_holds_at_most_one_image_of_scratch` flips from owed;
   `png_output.rs` and `png_input.rs`'s round trips pass.
5. **P-04.6: the encoder's floors** in MS-02's paired run, with
   `the_ratio_level_holds_level_six_size` and A8 green on every measured
   target.
6. **P-04.7: document writing on the ratio level.** Exit: the two hashes
   above re-recorded once after H-138, agreeing on linux and windows in CI's
   tests and on `wasm32-wasip1` locally; every round trip, `stream_dedup.rs`
   and the `png_embed` and `tiff_embed` tests pass.
7. **P-04.8: the row's close** in MS-02's paired run, with MS-05's 300-dpi
   render-to-PNG band at 60 MB.

### Risks

| Risk | Mitigation |
| --- | --- |
| The ring visits a candidate `prev` did not, or in another order | `the_ring_finds_what_the_vector_found` compares bytes with the old finder, kept test-only until P-04.7 |
| A position wraps over a long stream, or the sentinel aliases one | Offsets stay below `2^31` by the base's move, and the sentinel is `u32::MAX` |
| Block boundaries follow the writes | Boundaries are symbol counts; `the_stream_round_trips_however_it_is_fed` compares the bytes of whole, byte-at-a-time and random-piece feeds |
| A dynamic code is incomplete or longer than the format allows | `code_lengths_are_complete_and_at_most_fifteen` holds Kraft equality within the limit, and every block round-trips through this crate's inflate |
| A fast-level PNG is larger than today's | P-04.5 records each PNG input's bytes at today's encoder and at both levels, and one larger than today's stops the step for H-42 |
| Written bytes move unseen | Only P-04.7 moves them, the two hashes above with them, under H-138 |

## P-06 — Glyph coverage cache

*For [ROADMAP](../ROADMAP.md) row P-06.* A filled glyph is drawn by
transforming its outline into the page's device frame and filling it there
(`Renderer::show_glyph`), so every glyph shown rasterises its outline again,
though a page of text shows a few dozen shapes at a handful of sizes thousands
of times. This section fixes how a glyph's shape is separated from where it
stands, so that one mask serves every glyph of one shape; the key a mask is
kept under, with a font identity no two scopes can share; the two steps the
shape is quantised to and the pixels each moves; which glyphs take masks and
which keep the path route; how a mask is composited through the clip, the
soft mask and a knockout group; the cache's byte bound and its eviction order;
A4's counts; and how P-16's budgets later take the bound over.

### Scope

- **The mask route** (P-06.2): a filled glyph's outline rasterised in its own
  frame under a quantised 2x2 and phase, and placed at a whole pixel.
- **The key** a mask is kept under, and the font identity in it (P-06.3).
- **The cache** (P-06.3): one render's masks, bounded by bytes and evicted in
  an order the sequence of requests alone decides.
- **A4's counts**, and the pins the route moves once.

### Non-goals

- **A cache across renders or pages**: P-16's. Under a stated budget the
  document keeps the masks (P-16.7); without one a mask lives as long as the
  render that rasterised it.
- **Threads**: P-15's. One render's cache is one thread's.
- **Strokes, pattern fills, the clipping modes and Type 3 glyphs.** A stroked
  glyph's pen is the graphics state's and a pattern fill reads pattern space,
  so neither is a function of the glyph alone; a clipping mode adds the
  outline to the text clip, which wants the path; a Type 3 glyph is a content
  stream the interpreter runs, never an outline. All keep the path route.
- **Hinting, or any shape the font does not state.** The quantised 2x2 is the
  glyph's own transform rounded to a fine grid, and nothing more.

### Design

#### The mask route

`crates/tinker-pdf-render/src/glyph.rs` splits a glyph's em-to-device
transform — its `Glyph::transform` then the renderer's `base`, the page's
transform for every canvas — into what its shape depends on and where it
stands:

- **the 2x2**, `a`, `b`, `c` and `d`, each rounded to the nearest 1/256 of a
  pixel an em (`MATRIX_STEPS`);
- **the phase**, the fraction of a pixel the origin `(e, f)` stands past the
  whole pixel `(floor(e), floor(f))`, rounded to the nearest sixteenth on each
  axis (`PHASES`); a fraction that rounds to sixteen sixteenths is the next
  pixel's phase 0, so there are sixteen phases an axis and no seventeenth, and
  up to 16 × 16 = 256 phases a shape;
- **the origin**, that whole pixel, in the page's frame.

`floor` and `round` are correctly rounded by IEEE 754 and `e - floor(e)` is
exact, so a placement is the same on every target (ruling 4). The outline is
transformed by the quantised 2x2 and the phase alone — every entry a dyadic
fraction a few bits long, so each is the number it names — and filled non-zero
over its own bounds, widened by the paint region's pixel of slack, into a mask
whose pixel (0, 0) stands for the origin's pixel. `antialias` off hardens it,
as `coverage` hardens a path's. A mask that would hold more than 65 536 pixels
(`MAX_GLYPH_MASK_PIXELS`), or stand outside `i32`, is not made: that glyph
keeps the path route, which rasterises only the pixels its clip and its canvas
can show, so a heading at 300 dpi or a glyph far larger than its page is never
rasterised whole. Whether a glyph takes the route is a function of its outline
and its placement, never of the canvas.

**Ruling 5.** The phase and the origin come from the glyph's position in the
page's frame, which `base` gives every canvas alike, and placing a mask is an
integer offset; a tile subtracts its corner only when it indexes the placed
mask. So a glyph on a phase boundary rounds the same way in a tile and on the
page under it, and `render_regions.rs`'s text page holds that at every scale it
tiles.

#### Which glyphs take it

A glyph takes the route when 9.3.6's mode is 0, its fill is a colour rather
than a pattern, its outline is not empty, its transform is finite and places,
and its mask is within the bound. Mode 2's fill keeps the path route with its
stroke, so the fill and the pen are rasterised at one position; modes 1 and 4
to 7, a pattern fill and a Type 3 glyph keep it as the non-goals say.

#### The blit

`Renderer::paint_glyph_mask` composites a mask as `paint_inked` composites a
path's coverage. The placed mask is windowed to the region `coverage` would
have asked `fill` for — its rectangle met with the canvas's, the clip's and the
soft mask's — the clip and then the soft mask multiply into the window, after
any hardening, a knockout group restores its backdrop under it (11.4.5), and
`Canvas::fill_mask_inked` composites it with the fill colour, its ink, the fill
alpha and the blend mode. Nothing of the clip, the soft mask or the group is in
the mask, so one mask serves a glyph wherever it is clipped. The paint's
device-dependent state is held as `paint_inked` holds it
([device-dependent-state.md](device-dependent-state.md)): the window times the
clip, without the soft mask, is the glyph's shape for the rendering-parameter
plane, and the ink tables are in force while it composites.

#### The key, and the font in it

A mask is a function of five things, and they are its key: **the font**, **the
code**, **the quantised 2x2**, **the phase**, a sixteenth on each axis and so
one of 256, and **the flattening tolerance** the outline is flattened at, the
flatness in force ([device-dependent-state.md](device-dependent-state.md)),
which is the device's 0.2 on a page that sets none. The origin is not in it. One shape at one size is therefore up
to 256 masks rather than one, and a page asks for as many of them as the
phases its glyphs of that shape fall at — a line's origins spread across the
sixteen phases of its axis, and a page's lines across the sixteen of the
other unless their leading is a whole number of pixels — never more than it
draws glyphs of that shape.

The font id a glyph carries cannot be the font: it is the interned id of the
resource name a content stream selects the font by (`ScopeFont::id`), and a
form's scope replaces the page's while the form draws (`Renderer::begin_form`),
so `/F1` in a form and `/F1` on its page are one id naming two fonts. The glyph
source says what the font is, so the renderer stays free of PDF dictionaries:

```rust
/// What a glyph-mask cache keys a font by.
pub enum FontIdentity {
    /// The font's object, `num << 16 | gen`: one font wherever a scope names it.
    Object(u64),
    /// A font written into the scope's own resource dictionary, which has no
    /// object: one font only within the scope the renderer holds it in.
    Scoped,
}

pub trait GlyphSource {
    // ...
    /// `None` keeps the glyph's mask out of the cache: it is rasterised at
    /// every request, to the same pixels.
    fn font_identity(&self, font_id: u64) -> Option<FontIdentity> {
        None
    }
}
```

The facade answers from the scope's font table (P-02.3): an entry's reference,
or the object a reserved name spells (`PageResources::font_object`), is
`Object`; a font written into the dictionary is `Scoped`; an id that selects no
font is `None`. The renderer keys a `Scoped` font by the font id beside its own
serial for the scope it holds — 0 for the page's, and the next number at each
`begin_form` that brings a scope and at each scope a caller pushes — so a font
written into a dictionary is shared only within one scope's turn, and two
scopes naming two fonts alike never share a mask.

#### The cache

`crates/tinker-pdf-render/src/glyph_cache.rs` holds one render's masks, owned
by its `Renderer` and dropped with it. **A hit is the mask a miss rasterises**:
a mask is a function of its key, and is kept only when its rasterisation ran
to the end — a mask a cancelled render stopped short is drawn and never kept —
so whether a glyph hit moves no pixel. The outline is still asked for at every
glyph, so what the glyph source reports — a glyph it could not resolve, a font
it could not read — and its own counts do not depend on a hit; a hit saves
building the path and filling it.

**Bounded by bytes.** Each entry is charged its mask's pixels and 64 bytes
beside them (`ENTRY_BYTES`), so a page of empty masks is bounded too, against
16 MiB a render (`MAX_GLYPH_CACHE_BYTES`). A mask is at most 64 KiB, so the
cache keeps 255 of the largest at once. The bound is sized for the phases: a
page's distinct masks are at most its glyphs, so a page of 3 000 glyphs at
300 dpi, whose masks are some fifty pixels a side, asks for at most 3 000 of
some 2 600 bytes each, under 8 MiB, and keeps every one with room; at 72 dpi
the same page's masks are some 200 bytes each and the bound holds eighty
thousand. A page that asks for more evicts, and a glyph it evicted is the same
pixels when it is rasterised again.

**Evicted by the sequence of requests alone.** Every request, hit or miss,
takes the next number of a counter, and an entry records the number of the
request that last asked for it. When a mask would not fit, entries are evicted
least recently asked first, through an ordered index from that number to the
key, until it does. A `HashMap` finds an entry by its key and is never
iterated, since its order is seeded per process, so which entries a full cache
evicts is the same on every run and every target.

#### A4's counts

P-06.3 adds `glyph_masks_rasterised` and `glyph_mask_hits` to `RenderCounts`,
and to `Stats` through `stats.rs` (`Counter::GlyphMasksRasterised`,
`Counter::GlyphMaskHits`), and `outlines_filled` comes to count an outline
built into a path: on the path route, and at a mask's miss. A4 renders
MS-01.6's page (`guards.rs`'s `glyph_page`) at 72 dpi and holds
`glyph_masks_rasterised` to at most the page's distinct keys and
`glyph_mask_hits / (glyph_mask_hits + glyph_masks_rasterised)` to at least
98 %. The page is built so that its keys can be counted: 5 600 glyphs of 80
codes in one embedded face at 12.5 points, every line starting on a whole point
and every advance eight points, so at a pixel a point every origin is a whole
pixel — phase 0 on both axes — and every glyph's 2x2 is `[3200, 0, 0, -3200]`
in steps of 1/256. Its distinct keys at these steps are its 80 codes, within
the 112 that 98 % of 5 600 leaves; 80 masks are far inside the byte bound, so
none is evicted: 80 are rasterised and 5 520 hit.

#### What the steps move

A kept mask is the mask a miss computes for its key, so only the quantisation
moves pixels, and it moves them once, in P-06.2, under H-76's and H-133's
review:

- **The phase step.** A glyph stands at most a thirty-second of a pixel from
  where its outline says on each axis, so only a pixel an edge passes through
  changes, by at most a sixteenth of its area; with anti-aliasing off, where
  every pixel is whole or empty, such a pixel may turn from one to the
  other.
- **The 2x2 step.** A point within an em of the origin moves at most 1/256 of a
  pixel — one unit of the scanline's horizontal resolution and a sixteenth of a
  sub-scanline — so a crossing moves at most one unit, and a horizontal edge
  crosses a sub-scanline only where it lay within 1/256 of one.
- **The frame.** The outline is transformed at the magnitude of its own em, not
  the page's, so an edge that sat exactly where a rounding turns may land one
  unit over.
- **The byte bound** moves nothing.

So P-06.2 re-records, once, every pin that fills an outline glyph in mode 0 in
a colour, in both builds alike, since every face they draw is embedded:

| Pin | The glyphs it fills |
| --- | --- |
| `determinism.rs`'s `rendering_is_stable_across_targets`: `text`, `gs_text`, `xps` and `epub` | the embedded face's two lines, the same lines selected by graphics states, the XPS package's glyph runs, the book's text |
| `epub_shaped.rs`'s `the_shaped_page_renders_to_the_bytes_it_always_did` | the Arabic page's shaped runs |
| `render_options.rs`'s `ink_output_is_pinned`, `transparent_and_premultiplied_output_is_pinned`, `hard_edged_output_is_pinned` and `the_default_is_unchanged` | the ink, layer and hard-edge pages' lines |
| `render_goldens.rs`'s `text` golden | its line of text |
| `surface_goldens.rs`'s `annotation-appearances` and `visible-signature` goldens | the text the annotations' appearances and the signature's appearance draw |

The other pins draw no glyph, or draw one only through a stroke, a pattern, a
clip or a Type 3 procedure — the `type3-glyph` golden among them — and do not
move. MS-01.6's pinned render counts do not move in P-06.2 either: a mask is an
outline built into a path and filled, once a glyph, which is what
`outlines_filled` counts; they move in P-06.3, where a hit builds none, each
with its reason.

#### How P-16 takes the bound over

P-16.7 makes the masks a kind of the document cache. The key stays — font
identity, code, quantised 2x2, phase and flattening tolerance, a `Scoped`
font keyed by its id and the document's scope written exactly rather than a
render's serial, with the render's anti-aliasing beside them, which one render
holds fixed and two need not — and only who holds the bytes changes: a mask
is charged what it is charged here and evicted in P-16.1's
order with the document's images, transforms and recordings under
`CacheLimits { bytes }`, and a mask evicted and drawn again is the same bytes,
which P-16.3's starved render holds. With no budget stated each render keeps
its own cache under this section's bound, as P-16.2 keeps an image for the
render that decoded it.

### Milestones

1. **P-06.1: this section.**
2. **P-06.2: the mask route.** Exit: `tinker-pdf-render`'s
   `a_glyph_moved_by_whole_pixels_draws_the_same_mask` (new);
   `render_regions.rs`'s `every_tile_is_byte_equal_to_the_page_under_it` and
   its siblings, with the text page's tiles at every scale they test; the pins
   above re-recorded once after H-76's and H-133's review, agreeing under
   `cargo test` on linux and windows and under `--target wasm32-wasip1`
   locally; `text_render_modes.rs`, `render_differential.rs` and `stats.rs`'s
   render counts unchanged, in both builds.
3. **P-06.3: the cache.** Exit: `guards.rs`
   `a4_glyph_masks_rasterised_are_at_most_their_distinct_keys` flips from owed
   and joins K-07's roster; `glyph_cache.rs`
   `a_full_cache_evicts_the_same_entries_on_every_run` and
   `two_scopes_naming_two_fonts_alike_never_share_a_mask` (new); `stats.rs`'s
   render counts restated, each with its reason; P-06.2's pins unchanged on the
   measured targets and `render_regions.rs`'s tiles byte-equal, in both builds.
4. **P-06.4: the row's close** in MS-02's paired run: the text page rendered at
   72, 150 and 300 dpi (pixels only) at 9×, 5.86× and 4.88× the planning
   baseline, with A4 and the determinism fingerprints green on linux and
   windows in CI's tests and on `wasm32-wasip1` locally.

### Risks

| Risk | Mitigation |
| --- | --- |
| A tile and its page round a glyph's phase apart | The phase is the page frame's, the offset an integer and the route a function of the glyph alone; `render_regions.rs`'s text page at every scale |
| A hit draws other pixels than its miss | A mask is a function of its key, and a cancelled rasterisation is never kept; P-06.3's pins are P-06.2's, and P-16.3 renders through a starved cache |
| Two fonts share a mask because their resource names intern alike | The key's font is the glyph source's identity, an object or a scope's turn, never the name's id; `two_scopes_naming_two_fonts_alike_never_share_a_mask` |
| Eviction depends on hash order | Entries are evicted through the request counter's ordered index and the `HashMap` is never iterated; `a_full_cache_evicts_the_same_entries_on_every_run` |
| A huge glyph, or a hostile one, is rasterised whole | A mask past 65 536 pixels, or outside `i32`, is not made, and the path route bounds such a glyph by its clip and canvas (ruling 1) |
| A mode 2 glyph's fill and stroke part | Mode 2 keeps the path route for both |
| What the glyph source reports depends on hits | The outline is asked for at every glyph, hit or miss |
| The quantisation moves pixels nobody reviews | It moves them once, in P-06.2, in the pins above, under H-76 and H-133; a later step that moves a glyph pin is a finding |

## P-16 — Caches with byte budgets and a caller memory budget

*For [ROADMAP](../ROADMAP.md) row P-16.* Every cache this engine keeps today
lives as long as one operation: a page's `PageResources` keeps its decoded
images, compiled ICC transforms, font programs, outlines and form scopes
(`Caches` in `crates/tinker-pdf/src/resources.rs`), a render keeps its glyph
masks (P-06), and an operation's decoded streams are its own (P-03). So a
second render of a page at another resolution decodes every image again, and
a form stamped on a hundred pages is interpreted a hundred times. This
section fixes one cache a document keeps across its renders, what it holds
and what each entry is charged, the budget a caller states and where, the one
order everything in it is evicted in and what that order means when threads
share it, why a hit is the bytes a miss computes, what an image too large for
the budget becomes in place of a refusal, how parsed objects leave the object
store under the same budget, how MS-05 reads resident memory against it, and
what a second render at another dpi reuses.

### Scope

- **The document cache** (P-16.2): one per document, shared by every clone of
  a `Document` and its pages, of typed entries each charged its bytes —
  decoded images keyed by the image object and the reduction their decode
  is made at, beside the frames of the JPEGs a draw's size reduces (P-16.2),
  compiled ICC transforms (P-16.4), form recordings (P-16.5, P-16.6) and
  P-06's glyph masks (P-16.7), with room for P-22.7's re-keyed transforms and
  P-23.2's tiling cells.
- **`CacheLimits { bytes }`**, where a caller states it, and what holds with
  none stated.
- **The eviction order**, one for every kind, and what a counted guard reads.
- **Reduced-resolution decode** in place of the placeholder (P-16.9).
- **Parsed objects dropped under pressure** in `tinker-pdf-cos`'s store
  (P-16.8).
- **MS-05's reading** of resident memory against a stated budget, and the
  second render at another dpi (P-16.10a, P-16.11).

### Non-goals

- **A default budget.** A cache kept with none stated would hold memory a
  document never held before, which is a price under H-77's memory ceilings
  and the owner's to set. With no budget stated, every entry lives as long as
  the render or display list that made it, as today, so nothing a document
  holds grows unasked.
- **A cache across documents.** Two documents never share an entry: an
  entry's key names objects of one document's bytes.
- **Inline images.** One has no object to key it by, and stays the render's,
  as today.
- **Eviction by size, cost or frequency.** One order, the sequence of
  requests, is the whole policy; a cleverer one would make which entry
  survives depend on more than what was asked, and buys nothing a counted
  guard can show.
- **Threads.** P-15's pool is how renders run at once; this section says only
  what the cache promises them.

### Design

#### The cache and its entries

`crates/tinker-pdf/src/cache.rs` (new, P-16.2) holds a document's cache
behind the `Arc` every clone of the `Document` shares, so a page handed out
and the document it came from meet one cache. An entry is a typed value under
a typed key:

| Kind | Key | Charged | Row |
| --- | --- | --- | --- |
| a decoded image | the image's object, what its decode is asked for — the reduction it is made at, 1 at full size, 2, 4 or 8 for a JPEG decoded at the size it is drawn at (P-09.8) and `2^d` for a JPEG 2000 codestream decoded with `d` levels dropped (P-10.8), and the budget's reduced decode (*Reduced-resolution decode*); and the window a codestream's decode holds (P-10.9) — the conversion its colours take where a rendering intent changes them, its own `/Intent`'s or the one in force at the draw (8.6.5.8, Table 89), and the default colour spaces its scope paints device spaces in (8.6.5.6), each `/ColorSpace` entry written exactly, the two things outside the object a decode reads | its samples and its soft mask's, at the decoded size, the words of what its decode reported and its key's defaults | P-16.2, P-16.9 |
| an image's frame | the image's object and its scope's defaults, as its decodes: what every draw of a JPEG or a JPEG 2000 codestream with no mask reads its ask against — a JPEG's width and height, a codestream's size, origin and levels — learnt by its first decode | its key's defaults | P-16.2, P-10.8 |
| the profile an ICC stream holds | the stream's object: what its bytes are, as the transforms are keyed, or that it would not decode | the profile's bytes and its key's 64 | P-22.7 |
| a compiled ICC transform | the profile's bytes, their SHA-256 standing for them in the map and the bytes compared in full on a hit, and the destination, sRGB; the conversion each transform is compiled for, intent and black-point compensation, is the kept transforms' own key within the entry (P-22.7) | the profile's bytes its key holds, and the most it can come to hold whichever intents compile after it is kept and whatever they convert (`Transforms::bytes_bound`): the profile's model, and its `A2B*` and `B2A*` tables and `D2B*` pipelines at the most reading each can allocate, beside relative colorimetric's transform and one for each other intent at the most any of the profile's sources compiles — three 4 096-entry curves for an RGB matrix model, or the largest grid a table or pipeline is read through with its every node filled — each with its largest row tables | P-16.4, P-22.7 |
| a form recording | the form's object, the scope its names resolve in where it brings no resources of its own, the usage the render names (its optional-content states), and `tinker_pdf_content::FormInvocation`: everything the interpreter carries into a form and does not reset, written exactly — the graphics state, a waiting clip, the operands before `Do`, the depth, the streams around it, the scope's remapped device spaces. The text and line matrices, the current point and the subpath's start are not in it: a recording notes each its drawing read before setting its own and replays only where they are what they were | the recorded calls, as a display list counts its own (`Event::kept_bytes`), with its budget operations | P-16.5, P-16.6 |
| a glyph mask | P-06.3's key — the font as the document names it, its object or a scoped font's id and its scope's resources written exactly, the code, the quantised 2x2, the phase and the flattening tolerance — and the render's anti-aliasing | P-06.3's charge, its pixels and 64 bytes, and its key's resources | P-16.7 |
| a tiling cell | P-23.2's | its pixels | P-23.2 |

Each kind is a map from its key to its value, so a lookup is one hash of the
key, and a failure is kept as firmly as a success, in the words the decode
used: an image that would not decode, or a profile that would not parse, is
refused once per document, and every render that meets it reports it again,
as `CachedImage` reports what its decode reported today. Every entry is
charged at least 64 bytes beside its payload, P-06.3's `ENTRY_BYTES`, so a
document of empty entries is bounded too.

**Filled single-flight.** A miss fills by `docs/design/parallelism.md`'s
rule, which the cache keeps in its own table under the lock its index is kept
under, so that a charge and a publication are one step: one thread fills a
slot and the threads that want it wait, unless a thread is itself in the
middle of a fill, in which case it computes the value for itself and publishes
nothing — the re-entry rule, for the reason `crates/tinker-pdf-cos/src/store.rs`
gives at its head, that a `/Length` pointing into its own stream re-enters. A
thread's open fills are counted where `tinker_pdf_cos::Flights` counts its
own (`tinker_pdf_cos::OpenFill`, `fill_open`), so the rule holds across this
table and every `Flights` at once. A fill that unwinds or is cancelled
publishes nothing, so a cancelled render's partial decode is never kept.
**A form's recording is the one kind not filled so**: it is made by the
render that draws the form, as it draws it, and a render cannot wait for
another's drawing in place of its own, so a miss is a request numbered as
any other, the interpretation that follows offers what it recorded, and the
first offer for a key is the one kept.

#### `CacheLimits { bytes }`, and where a caller states it

`CacheLimits { bytes: u64 }` is a document's budget: the most bytes its cache
and its object store's dropped kinds hold at once. A caller states it where a
document's other document-wide choices are stated: on `OpenOptions`, beside
its fonts (`OpenOptions::with_cache_limits`), and on an open document by
`Document::with_cache_limits`, which hands back a document sharing the bytes
with a cache of its own, as `Document::with_fonts` does. The object store is
the open's, shared by every document handed back from it, so only a budget
stated at open charges the parsed objects beside the cache; one stated on an
open document holds its cache alone, and the objects stay as the open left
them. The C ABI and the bindings project the two 1:1 (ruling 11), in the row
that lands each.

With none stated the cache keeps nothing past the render or display list that
made an entry: the render's own resources hold what they hold today, under
P-06.3's per-render bound for masks, and are dropped with the render.
`a_document_with_no_budget_keeps_nothing_past_its_render` pins it. A budget
of zero is stated, and keeps nothing at all: every request misses, which is
what P-16.3's starved render is.

#### The eviction order

**One order for every kind: least recently requested first.** The cache keeps
one request counter. Every request — a hit or a miss, of any kind — takes the
counter's next number, and the entry it reaches records it. A fill that would
take the bytes held past the budget evicts entries in the order of the
numbers they record, oldest first, through one ordered index from number to
key across every kind, until the new entry fits; an entry larger than the
whole budget is never kept, and is used once by the request that made it.
Each kind's map is a `HashMap` that is only ever looked up and never iterated,
since its order is seeded per process: which entries are evicted is decided
by the index alone.

**What deterministic means.** Three promises, of which only the first is
about pixels:

1. **What a render draws never depends on what is held.** A hit is the value a
   miss computes for the same key (*A hit is the bytes a miss computes*), so
   the same page draws the same bytes whatever was rendered before it, under
   any budget, warm or starved, on every target. P-16.3 holds every
   fingerprint to it.
2. **On one thread, the evictions are a function of the requests.** A serial
   caller asking the same sequence of things evicts the same entries on every
   run and target: `eviction_follows_the_request_sequence_and_never_passes_the_budget`
   scripts one sequence and pins the evictions.
3. **On several threads, the counts are the schedule's.** When renders share
   the cache, the order their requests take numbers is the order the
   scheduler gave them, so which entry a full cache evicts, and how many hits
   a render scores, can differ between runs. Nothing a caller reads otherwise
   moves — pixels, text and every warning are the same, by the first promise —
   and a counted guard reads a serial run, as `docs/design/parallelism.md`
   says every guard does.

**The bytes never pass the budget.** A charge is taken before an entry is
published, under the lock the index is kept under, and an entry is published
only once enough is evicted for it; so the bytes held, read at any moment,
are at most the budget, and an entry a render is still drawing from is held
by its own `Arc` and freed when that render lets it go, which is the one way
the resident bytes can briefly exceed the held ones.

#### A hit is the bytes a miss computes

Every value is a function of its key and the document's immutable bytes: a
decode of one image at one request, a transform of one profile, a recording of
one form under the state its key names, a mask of one quantised glyph. So
whether a request hits moves no pixel. Two rules keep it so:

- **Nothing outside the key reaches the value.** A form whose drawing reads a
  part of the invoking state its key leaves out must miss (P-16.5); a mask is
  P-06.3's function of its key.
- **A reduced decode is keyed by its reduction.** A reduced image is another
  entry from the full one, under its own request, and a request for the full
  decode is never served a reduced one, or the reverse.

#### Reduced-resolution decode

Today an image is decoded whole, whatever memory it takes, under the decode
ceiling every stream decodes under (`MAX_DECODED_STREAM`); a decode that
reaches the ceiling is drawn as far as it got and named by `DamagedImage`, and
a codec this build refuses draws ruling 2's placeholder, named by
`UnsupportedImage`. Nothing measures the image against memory, so there is
nothing yet that "aborts": the refusal a budget-bound engine would otherwise
reach for is the placeholder, and this replaces it with a smaller picture.

With a budget stated, **an image whose decode at the size it is drawn is
larger than the budget itself** — beside the rest of its entry's charge, its
64 bytes and its key's defaults, so that "fits" is "can be kept" — is decoded
at the least reduction past what that size asks whose bytes fit it — a DCT
image's scaled decode (P-09), the largest of a half, a quarter and an eighth;
a JPEG 2000 image's discarded levels (P-10), the fewest — and drawn from
that, named by a typed `RenderWarning`, `ReducedImage { name, factor }`,
which joins rendering's refusal table (ruling 10). A decode is three bytes a
sample, four for a codestream whose own opacity `/SMaskInData` puts to use,
and a codestream's is read off its whole reduced image, never the window a
canvas holds: the window is a tile's or a band's, and a page is drawn from
one decode in every part of it (ruling 5). The reduction is a function of the
image's size, the size it is drawn at and the stated budget, never of what
else the cache holds, so a page draws the same pixels whatever was rendered
before it; an image the budget can hold at its drawn size is decoded at it,
and room is made for it by eviction. An image no reduction fits — a codec with
no reduced decode of its own, an image with a mask or of palette indices, or
a factor past what its codec can give — is decoded as the draw asks and drawn
as today, and not kept: it is used once by the request that made it, as
every entry larger than the budget is. An inline image has no object to be
kept under and is never reduced. With no budget stated nothing changes, and
every fingerprint and `render_regions.rs` pass as they are.

#### Parsed objects under pressure

With no budget stated, `tinker-pdf-cos`'s slot store
(`crates/tinker-pdf-cos/src/store.rs`) keeps every parsed object for the
document's life. Under a budget stated at open, each object is charged an
estimate of its bytes — the extent of its tokens in the bytes it was parsed
from, a count the parser already keeps (for a stream, its dictionary up to its
data, which a parsed stream does not hold), with the 64 bytes every entry is
charged beside it — against the same budget, which the facade passes down
from `CacheLimits` through one accountant that both the store and the
facade's cache charge (`tinker_pdf_cos::Accountant`, handed to the store by
`CosDocument::charge_objects_to`). Every object a read reaches takes a number
from the accountant's counter as an entry of the cache does, so one order
covers both holders, and a charge past the budget evicts the oldest holdings
of either: an entry of the cache always, and an object only where **no caller
holds it** — an `Arc<Object>` whose only owner is the store. A charge that what
no caller holds cannot make room for is not kept, as one larger than the whole
budget is not, so the bytes held never pass the budget. A later read parses
again, from the same immutable bytes, so it gets an equal value
(`store.rs`'s purity, `a_store_over_its_budget_drops_objects_and_parses_them_again_equal`).

**One lock.** The accountant keeps the store's objects and the cache's table
inside its own ledger, behind one mutex, so an eviction of either is a step
taken under the lock the charge is taken under, and no other lock is taken
inside it; nothing is parsed or decoded under it. A document's objects are
charged to the first accountant handed it, and the objects read before it
took, the `/Encrypt` dictionary an open reads among them, are forgotten then.
A cache let go of — the one `Document::with_fonts` replaces — forgets its
entries from the ledger, which the store keeps alive.

Never dropped: the cross-reference table, the decoded object streams and their
indexes, the repair scanner's index, the page index and the hint tables, which
the reader cannot find anything without and which are never charged; an object
a caller holds; and, with no budget stated, anything at all, so the parse count
is today's. An object a reopened document reads through its base is held by
the base's store, and a budgeted store leaves it there rather than charging it
twice. A streamed document whose object is dropped may fetch the object's
bytes again from its source on the next read: the fetch is counted as every
fetch is (`docs/design/streaming-open.md`), and is the price of the budget.

#### MS-05's reading, and a stated budget

MS-05's probe (`crates/tinker-pdf/tests/peak_memory.rs`) runs each case in a
child of its own and reads its peak resident memory, `VmHWM`, from `/proc`.
A budget bounds the bytes the caches hold, not what a case must hold to run —
its input, its canvas, a decoder's working set — so each case is run under
its stated budget beside its unbudgeted figure, as P-16.10a records them. The
case holds its budget when its peak under the budget is at most its
unbudgeted peak plus the budget: the caches it held unasked move under the
budget, and nothing they hold passes it. P-16.11's exit holds every case to
that, at the figures P-16.10a commits, and a case whose peak passes is a
finding against the kind that held the bytes, never a larger budget.

**The budget each states is 64 MiB**, stated at open, so that the parsed
objects are charged beside the cache: one for every case, the threaded ones
and the open of the 23 MB file through the CLI's own open among them, and for
MS-04's second render. It is room for everything any case's renders compute —
the largest, `jpeg-300`'s A4 JPEG at full size, 26 MB — so no case draws from
a reduced decode (*Reduced-resolution decode*) and each case's figure under it
reads what keeping costs. Each budgeted case is its case again, its name
ending `-under-64-mib`, run in the same run, so the two peaks it is held
between are one machine's at one moment; its line in `peak_memory.tsv` names
it unheld until its first runs are read.

#### What a second render at another dpi reuses

A second render of a page under a stated budget large enough for the page
reuses every decoded image the second resolution draws at the first's
reduction (a decode is the same samples at every resolution that asks for it,
and the resampler draws from it): every image but a JPEG or a JPEG 2000
codestream, and those wherever the size they are drawn at gives the same
reduction — one that gives another is decoded once at that one. It reuses
every compiled transform, every
form recording (a recording is a function of the stream and the invoking
state, never of the canvas — ruling 5 — so a tile and the page hit it too),
every parsed object and every font program; it rasterises its glyph masks
again, since a mask's key is its quantised 2x2 and the dpi moves it, and it
fills, composites and encodes the page again. The images are what the first
render spent most of its time on in MS-04's 'a second render at another dpi',
which P-16.11 holds at 30 % of the first. So that operation draws its
quarter-size JPEG page first at 150 dpi, where its 620 by 877 JPEG is drawn
over more than 438.5 pixels down, past what a half holds, and then at 300
dpi: both ask the full-size decode, which the first keeps and the second
reuses, where a first render at 72 dpi would ask a quarter's and leave the
second to decode again.

### Milestones

1. **P-16.1: this section.**
2. **P-16.2: the cache and decoded images.** Exit: `caches.rs`
   `a_second_render_at_another_dpi_decodes_no_image` and
   `pages_sharing_an_image_object_decode_it_once` (MS-01's decode stats),
   `eviction_follows_the_request_sequence_and_never_passes_the_budget` and
   `a_document_with_no_budget_keeps_nothing_past_its_render`; `render_regions.rs`,
   `determinism.rs` and the display-list replay unchanged, in both builds.
3. **P-16.3: determinism for the caches.** Exit: `determinism.rs`
   `warm_and_starved_caches_render_the_fingerprints` in both builds on linux,
   windows and `wasm32-wasip1`, the `GOLDEN` table untouched.
4. **P-16.4: ICC transforms.** Exit: `caches.rs`
   `a_profile_named_on_every_page_compiles_once`; `colour_spaces.rs` and the
   starved render unchanged.
5. **P-16.5: kept form recordings in the interpreter.** Exit: `interpret.rs`
   `a_kept_form_recording_replays_as_the_form_interprets` and
   `a_kept_form_is_interpreted_once_per_key`; `cargo test -p tinker-pdf-content`.
6. **P-16.6: form recordings per document.** Exit: `caches.rs`
   `a_form_stamped_on_a_hundred_pages_is_interpreted_once`; `form_xobjects.rs`,
   `render_regions.rs`, the display-list replay and the starved render
   unchanged, in both builds.
7. **P-16.7: glyph masks under the document's budget.** Exit: `caches.rs`
   `glyph_masks_share_the_documents_budget`; guard A4 and the starved render
   unchanged, in both builds.
8. **P-16.8: parsed objects dropped under pressure.** Exit: `store.rs`
   `a_store_over_its_budget_drops_objects_and_parses_them_again_equal`;
   `cargo test -p tinker-pdf-cos`; `caches.rs`
   `a_budget_stated_at_open_holds_the_parsed_objects_too` and `determinism.rs`
   `budgets_stated_at_open_render_the_fingerprints`.
9. **P-16.9: reduced-resolution decode.** Exit: `caches.rs`
   `an_image_past_the_budget_draws_at_a_reduced_resolution_and_says_so`; with
   no budget every fingerprint and `render_regions.rs` unchanged.
10. **P-16.10a: the rows' operations**, unmeasured: MS-04's second render under
    a stated budget, and MS-05's cases under their budgets beside their
    unbudgeted figures;
    `xtask/src/bench.rs`'s `the_committed_baseline_agrees_with_the_tree`.
    **P-16.10b** takes the figures in the timing phase.
11. **P-16.11: the row's close.** Exit: MS-02's paired run green with the
    second render at most 30 % of the first; MS-05 holding every case within
    its budget as *MS-05's reading* states it; `caches.rs`'s guards in
    `cargo test` on linux and windows and on `wasm32-wasip1`.

### Risks

| Risk | Mitigation |
| --- | --- |
| A hit draws other pixels than its miss | Every value is a function of its key and the document's bytes, a cancelled fill publishes nothing, and a reduced decode is keyed by its reduction; P-16.3 renders every fingerprint warm and starved |
| What a page draws depends on what was rendered before it | The only budget-dependent choice, a reduction, is a function of the image and the stated budget, never of what the cache holds |
| Eviction depends on hash order | One request counter and one ordered index decide it; the maps are looked up and never iterated |
| Threads make a counted guard flake | The fills are single-flight, and the guards read a serial run; on threads only the counts move, never a pixel, a text or a warning |
| A thread waits on a fill that waits on it | `Flights`' rules: only a thread with no fill open waits, and re-entry computes |
| A dropped object reads back other than it was | Parsing is pure over immutable bytes; an object a caller holds is never dropped, and the table and the object-stream index never are |
| A budget is read as a bound on the whole process | It bounds the caches; MS-05 reads it as the peak under the budget less the unbudgeted peak, and says so |
| A document grows memory it never held, unasked | No default budget: with none stated an entry lives as long as its render, as today |
| A streamed document fetches again what it dropped | Counted as every fetch is; it is the price of a budget, and with none nothing is dropped |

## P-24 — Search over prebuilt text

*For [ROADMAP](../ROADMAP.md) row P-24.* A `TextPage` is built once and
searched many times — MS-04's two searches build the 100 pages' text outside
the timed loop and search it five times and twice an iteration — yet every
call of `TextPage::search` and `TextPage::search_with` does the whole of its
work again: it collects the page's lines, folds every glyph's text into a
`String` of its own and joins them, computes the joined line's UAX #29
boundaries for a whole-word search, runs `str::find` from each restart, and
maps each hit back to its glyphs by walking the line from its start
(`crates/tinker-pdf-content/src/text.rs:281`,
`crates/tinker-pdf-content/src/search.rs:108`). This section decides where a
`TextPage` keeps the folded forms and the tables that map them back, when a
form is built, how kept forms follow the blocks they were folded from, the
boundaries kept beside them, the matcher that reads them, and the counted door
the guards read, with the API change each costs.

P-24.1's seven phase operations split the two searches' time into those phases
(`docs/verification.md`, "Where prebuilt-text search spends its time"), and
`cargo xtask search-split` prints each phase's share of its search; their
figures are the timing phase's, recorded in `baseline.json`. The decisions
below do not wait on the shares: every phase but the matcher's own scan
computes, on every call, exactly what the call before it computed from the
same blocks, so keeping its output takes it out of every call after the first
whatever its share, and the matcher and the mapping are made linear in the
text and the hits. The split read after P-24.4 shows what remains.

### Scope

- **The kept forms** (P-24.3): for each of the four folds a search can ask
  for, each line's folded text and the table from its byte offsets to the
  glyphs they came from, built once a page and fold.
- **When a form is built**: at the first search that needs it.
- **How kept forms follow the blocks** (P-24.3): `TextPage::blocks` behind
  accessors, any change to the blocks dropping the forms, and the API change
  that is.
- **The boundaries** (P-24.4): each folded line's UAX #29 boundaries, computed
  once a form and kept beside it.
- **The matcher and the mapping** (P-24.4): one scan of a line's kept text for
  every occurrence of the needle, and each hit mapped to glyphs by a cursor
  that moves forward along the line.
- **The counted door and its counters** (P-24.3, P-24.4):
  `TextPage::search_counted`, through MS-01's tally.
- **The close** (P-24.5).

### Non-goals

- **Other answers.** `search` stays literal and case-insensitive by
  `str::to_lowercase`, `search_with` keeps every option's meaning and its
  overlap rules, and with `SearchOptions::default()` it stays `search`, quad
  for quad: the parity suite and every search fixture hold both.
- **A form built at extraction.** `Page::text` would fold every page for every
  caller, those that never search included, and charge P-02's text target for
  it (`docs/roadmap/P.md`, "Text, 5 dense pages").
- **A document-wide index.** Forms are one page's; a search across a document
  is FT-21's and FT-22's.
- **Threads.** A search is one thread's; P-15's pool has no part in it.
- **A regular expression.** The tree has no regex engine (`search.rs`'s
  documentation), and the matcher is a substring matcher.

### Design

#### What a form is, and where a `TextPage` keeps it

A **fold** is one of the four ways a search folds text: none (case-sensitive,
accents significant), lower-cased, diacritics removed (case-sensitive) and
both — `SearchOptions`' `case_sensitive` and `diacritic_insensitive`, and
`search`'s own lower-casing, which is the second. A **form** is one page's
text in one fold: for each line, in the order `TextPage::lines` gives them,
the folded text of every glyph joined into one `String`, and the **offset
table**, a `Vec<u32>` holding the byte offset in that `String` where each
glyph's folded piece starts, with the text's length after the last. A glyph
whose piece folding empties — a combining mark searched without diacritics —
starts where the next one does, as its piece occupies nothing, which is how
today's walk treats it. A form costs the page's text once more and four bytes
a glyph, and a page holds at most four.

`TextPage` gains **one private field**, `forms`, holding the four as
`std::sync::OnceLock<Form>`, one a fold. A `OnceLock` because `search` and
`search_with` take `&self` and fill it, and because a `TextPage` is `Send` and
`Sync` today and a `OnceCell` would take `Sync` away, which is a break of its
own; it is the same type on `wasm32`. Two threads searching one page in the
same fold at once build it once, the second waiting for the first. A clone of
a `TextPage` clones the forms it has built, which describe its blocks as they
describe the original's; `Default` builds none; the derived `Debug` shows
them.

#### When a form is built

**At the first search that needs it, never at extraction.** `Page::text`,
`text_with`, `text_counted` and every other door that builds a `TextPage`
build no form, so P-02's text target and every caller that never searches
pays nothing. The first `search` builds the lower-cased form; the first
`search_with` builds the form its options name, and a later search in the same
fold reads it. The first search of a page in a fold does today's folding and
joining once, and writes the offset table beside it; every search after it in
that fold folds nothing but its needle.

#### How kept forms follow the blocks

A form is a function of the blocks' glyph texts, so it is right only while the
blocks are the ones it was folded from. Today `blocks` is a public field, and
it is written after extraction: the facade puts a page into logical order in
place (`crates/tinker-pdf/src/text_order.rs:211`), places each `/ActualText`
span's text, dropping its glyphs from every line after the first and lines
and blocks left empty (`:256`, `:278`), and any caller may edit it. A form
kept beside a field anyone can write is stale the moment someone does, and
nothing could tell.

So **`blocks` goes behind accessors**, and the one way to change the blocks
drops the forms first:

- `TextPage::blocks(&self) -> &[TextBlock]`, to read them;
- `TextPage::blocks_mut(&mut self) -> &mut Vec<TextBlock>`, which empties
  every `OnceLock` of the forms before it hands the blocks out, so a search
  after any change folds the blocks as they now are;
- `TextPage::into_blocks(self) -> Vec<TextBlock>`;
- `TextPage::new(blocks, mcid_props, stated, warnings) -> TextPage`, for a
  page built by hand, with no form built.

`mcid_props`, `stated` and `warnings` stay public fields: no form reads them,
since the facade has put each `/ActualText` span's text on its glyphs before a
caller sees the page.

**The API change.** A `TextPage { … }` literal and a read or write of
`page.blocks` stop compiling, and become `TextPage::new(…)`, `page.blocks()`
and `page.blocks_mut()`. In the tree that is the content crate's own
`TextDevice::finish` (`text.rs:523`) and its serialiser's tests; the facade's
logical order, `/ActualText` placement, reading order, tables, word boxes,
Markdown and EPUB paths, which read the blocks or reorder them; and the tests that
build a page by literal, `text_serialize.rs:152` and `text_allocations.rs:62`
among them. The C ABI and the bindings read no field of a `TextPage` — they
carry text across as the structured JSON — so nothing outside Rust moves. The
workspace is at 0.0.1 and SU-18 has fixed no stability yet, so the change
breaks no promise, and the feature doc, `docs/features/content-and-text.md`,
states the accessors where it now states the field.

#### The boundaries

A whole-word search reads each folded line's UAX #29 boundaries, and they are a
function of the folded text, so they belong to the form: a
`OnceLock<Vec<Vec<usize>>>` beside each, filled at the first whole-word search
in that fold with `word_boundaries` of every line, and kept. A search in a
fold whole-word searches never asked for computes none.

#### The matcher

Today a rejected whole-word candidate resumes `str::find` one character on,
and the scan runs again from there, so a line dense with candidates is
scanned again from each. P-24.4's matcher scans each line's kept text **once**
for every occurrence of the folded needle, overlapping ones included, with the
Knuth–Morris–Pratt automaton over its bytes: the needle's failure table is
built once a search, and a byte match of a UTF-8 needle in UTF-8 text starts
and ends on character boundaries, as a `str::find` match does. The
occurrences come in increasing order, and today's overlap rules decide which
are hits:

- a hit's end is the first start the next hit may have, so an occurrence that
  begins inside a hit is passed over, as `find` resuming past the hit passes
  over it;
- a whole-word candidate that does not begin and end on a boundary is no hit
  and holds nothing back, so the next occurrence considered may overlap it —
  `x x` in `xx x x` — as `find` resuming one character on reaches it.

Whether a candidate's two ends are boundaries is read by a cursor over the
line's sorted boundaries, which moves forward with the candidates' starts and
ends, both increasing. So searching a line costs at most a constant times its
folded length plus its hits.

#### The mapping

Each hit is mapped to its glyphs through the offset table by a **cursor that
moves forward along the line**: its first glyph is the first whose piece ends
past the hit's start, and its last the last whose piece starts before the
hit's end, which is the walk today makes from the line's start for every hit,
made once along the line for all its hits in order. Its quad is then today's,
from the same two glyphs.

#### The counted door

`TextPage::search_counted(&self, needle: &str, options: &SearchOptions) ->
(Vec<Quad>, Stats)` is a counted sibling, as `Page::text_counted` is, by the
doors' rule (above): `search` takes no options value to carry a flag, and the
answer is a list. Its quads are the ones `search_with` returns for the same
arguments, to the bit, and it counts through MS-01's `Work`, which the content
crate already reaches (`interpret_counted`). Three counters join `Stats`, each
a step and each in `work()`:

| Counter | One unit is | Step |
| --- | --- | --- |
| `text_forms_built` | a page's form folded and joined, its offset table with it | P-24.3 |
| `word_boundaries_computed` | a form's lines' UAX #29 boundaries computed | P-24.4 |
| `search_steps` | a byte of a line's kept text the matcher read, or a glyph the mapping cursor moved past | P-24.4 |

### Milestones

1. **P-24.1: the split.** Seven phase operations beside MS-04's two searches
   and `cargo xtask search-split`; their figures are the timing phase's.
2. **P-24.2: this section.**
3. **P-24.3: the kept forms.** The four forms in their `OnceLock`s, built at
   the first search that needs them; `blocks` behind `blocks`, `blocks_mut`,
   `into_blocks` and `TextPage::new`, every user in the tree moved to them;
   `search_counted` and `text_forms_built`. Exit: `search_work.rs` (new)
   `folding_happens_once_per_page_whatever_the_searches` — ten searches per
   option set over a generated 100-page document's prebuilt text fold each
   page at most once per fold, by `text_forms_built`, and a search through
   kept forms returns the first search's quads bit for bit;
   `a_form_does_not_outlive_a_change_to_its_blocks` (new) — a search after
   `blocks_mut` reorders, drops or edits glyphs finds what a fresh page with
   those blocks finds; `search.rs`'s unit tests, `text_options.rs`,
   `tinker_parity.rs`, `text_logical_order.rs`, `hostile_input.rs` and
   `epub_fallback.rs` find the same quads, in the default build and with
   `--features bundled-fonts`.
4. **P-24.4: the boundaries, the matcher and the mapping.** Exit:
   `search_work.rs` `a_search_is_linear_in_its_text_and_its_hits` (new) — the
   matcher's and the mapping's steps, `search_steps`, at most a constant times
   the folded length plus the hits on a 4 000-character line of `a a a …`
   searched for `a` and `a a`, plain and whole-word;
   `boundaries_are_computed_once_per_page` (new) — `word_boundaries_computed`
   at most once a page per fold across searches; the overlap rules pinned
   (`x x` in `xx x x`, a hit resuming past itself); every search fixture's
   hits unchanged as in P-24.3, both builds.
5. **P-24.5: the close** in MS-02's paired run: MS-04's two prebuilt-text
   searches over 100 pages, plain and whole-word diacritic-insensitive, with
   their floors raised to 6× and 6.41× against the planning baseline, which
   folds on every search where the head's kept forms leave the matcher; and
   `search_work.rs`'s guards in `cargo test` on linux and windows, in both
   builds, and under `--target wasm32-wasip1` locally.

### Risks

| Risk | Mitigation |
| --- | --- |
| A kept form outlives a change to its blocks, and a search finds text that is gone | `blocks` is private, and `blocks_mut`, the one way to change it, drops every form first; `a_form_does_not_outlive_a_change_to_its_blocks` |
| A form built at extraction charges P-02's text target | None is: a form is built at the first search that needs it, and `Page::text` builds none |
| The matcher finds other hits than `find` did | Every occurrence is found and today's two overlap rules choose the hits; `search.rs`'s overlap pins, the parity suite and every search fixture hold each quad, in both builds |
| A glyph whose piece folds to nothing maps a hit to the wrong glyph | It starts where the next glyph starts, as today's walk counts it, and `options_compose_and_hits_cover_their_glyphs` holds a hit across a folded-away mark |
| Four forms hold a page's text four times over | A form is built only for a fold a search asks for, and costs the text once more and four bytes a glyph |
| Two threads search one page at once | A `OnceLock` builds each form once; the second waits for the build, and no lock is held across a search |
| The accessors break a caller outside the tree | The C ABI and the bindings read no field of a `TextPage`; at 0.0.1, before SU-18, no stability is promised, and the feature doc states the accessors |
| Counting moves an answer | `search_counted` returns `search_with`'s quads to the bit, which `folding_happens_once_per_page_whatever_the_searches` asserts |

## P-17 — Incremental reparse and dirty-region re-render

*For [ROADMAP](../ROADMAP.md) row P-17.* An edit, its incremental save and the
reopen of what it wrote cost what the edit changed, and a changed annotation
is re-rendered by its rectangle alone. At the tip `DocumentEditor::save`
under `WriteMode::Incremental` copies the whole original before it appends
(`write::incremental_update_reserving`), and the update is reopened from that
copy as a new file: every cross-reference section of the chain is walked
again and every entry probed against its header (`doc.rs` `validate`), so a
one-object edit of a 1 000-page file costs the file twice. A page whose
annotation changed is rendered whole. This section fixes the update section
the writer produces alone, the backing that reads a document over its
original and that section without joining them, the reopen that reads only
the section and keeps its base's state for everything else, the facade call
that does the three in one, the rectangle an annotation edit changes and how a
render of that rectangle over the previous bitmap is the full render, and the
counted quantity each step's guard reads.

### Scope

- **The update section** (P-17.2): the changed objects, a cross-reference
  section and a trailer, offsets counted from the original's end, written
  without the original's bytes.
- **A document over segments** (P-17.3): the original and its section, held
  as they were handed in, read without a copy.
- **The reopen** (P-17.4): what it reads, what it keeps of its base, what it
  parses afresh, which bases it opens whole and the reason it gives, and
  `DocumentEditor::view` through it.
- **The facade's call** (P-17.5): edit, save and reopen in one, the font
  provider and any archive report carried, counted on MS-01's stats.
- **The changed rectangle and the patch** (P-17.6), and the patched page's
  fingerprint (P-17.7).
- **MS-04's edit-and-reopen operations and the patch's** (P-17.8a), and the
  row's close (P-17.9).

### Non-goals

- **A writable object store.** A reopened document is read-only, as every
  `CosDocument` is; an edit is still a `DocumentEditor`'s, the one way a
  document changes.
- **A rewrite.** A rewrite renumbers and lays out the whole file, so nothing
  of a base survives it to keep; only an incremental update has a section to
  reopen over.
- **Comparing pixels.** The rectangle comes from the annotations' geometry,
  and the previous bitmap is written into, never read for what changed.
- **An edit that changes what the page draws besides its annotations.** A
  page whose content, resources or boxes changed is rendered whole.
- **RC-21's class.** Making a tile exact where an image run's overlap falls
  outside it is RC-21's; the patch grows around such a run instead (*The patch
  stays the full render*).

### Design

#### The update section

`write::update_section` writes what `write::incremental_update` appends: a
newline when the original does not end in one (7.5.6's update begins on a new
line), each changed object, the cross-reference section of their rows with
object zero's free head (`write_classic_xref`), and the trailer with `/Size`,
`/Prev` and the `/ID` whose first string the original keeps (14.4), every
offset the original's length plus the section's own. It is handed the
original as a `write::Prefix` — its length, and whether its last byte is a
newline — and nothing else, so it reads no byte of the original, and a
streamed document answers that with a one-byte window rather than a fetch of
the whole (`CosDocument::prefix`). `incremental_update` is the original
followed by that section, joined once into a buffer of the sum's length: the
file it writes at the tip, byte for byte. `DocumentEditor::save_update` hands
back the section alone for the save `DocumentEditor::save` would write under
`WriteMode::Incremental` — the same changed set (`changed_set`), trailer
(`update_trailer`) and inherited cipher — whatever `WriteOptions::mode` says,
and `save` in incremental mode is the document's bytes followed by it.

A signature reserves its object the same way: the section is written with the
reservation, the placeholder's two fields at their offsets in the whole file —
the original's length plus their place in the section — and the whole file is
joined once, because the digest covers it (`sign::seal`).

#### Two segments, one document

`Backing::Segments` holds a document's bytes as segments: the original, then
each section appended to it, in order, each shared as it was handed in. A read
inside one segment borrows that segment whole, as a buffer's read borrows the
buffer, standing at the segment's offset in the document; a read across a seam
gathers its own range and nothing more; and a whole-file operation —
`CosDocument::bytes`, a signature's byte range, `stream_raw_encrypted`, the
repair scanner — joins the segments once and keeps the join, as a chunked
backing keeps its whole. The segments are in hand, so a document over them
takes every decision a buffered open takes where a streamed one differs: the
chain is walked at open, every type-1 entry is probed against its header then
(through the segments, a header across a seam gathered), the ladder is decided
then, no hint table is read and no fetch is declared.
`CosDocument::open_appended(original, update)` is the open over two segments:
the document `CosDocument::open` gives the joined bytes, object for object,
without the join.

#### The reopen: what it reads

`CosDocument::reopen(base, update)` opens the base's bytes followed by
`update`, an incremental section written after them, over segments — the
base's own and the section — and of all of it reads only the section:

1. `startxref` from the tail, which names the section's cross-reference data;
2. that one section (`xref::build_limited`, one deep), with its `/XRefStm` when
   it is a hybrid one;
3. its trailer, which must chain to the base: `/Prev` the base's
   `last_startxref`, and `/Encrypt`, and on an encrypted base the first `/ID`
   string, the base's own;
4. each of the section's type-1 entries, probed against the header at its
   offset as validation probes every entry of a full open.

The trailer is the section's entries over the base's, newest first, as the
walker merges a chain; the revisions are the section's and then the base's;
`/Root` must be locatable through both. The section's entries — its changed
objects and its free head — are the reopened document's own table, and every
other number's entry is the base's, read where it lies; the merged table is
built only when `CosDocument::xref` asks for all of it.

#### What it keeps of its base

For a number the section does not define, the reopened document is its
base's: the entry, the parsed object, the decoded object stream and the
security. A load of such a number goes to the base, which parses it once into
its own store if no read has yet, and the reopened document takes that object
— unless its parse read a number the section redefines: a stream whose
indirect `/Length` the section changed, or a member of an object stream whose
container, or the container's indirect `/Length`, `/N` or `/First`, it
changed. Such an object is parsed afresh in the reopened document, against the
merged entries, as a full open of the same bytes parses it. A number the
section defines is parsed afresh from the section's bytes. A resolved chain
and a stream's extent are each read in the document asked, so nothing the base
resolves for itself stands in for what the update changed.

The sharing holds only while both documents keep the security the reopen
found: the reopened document is given the base's decryptor, key and
authorisation as `inherit_security` gives them, each document counts the times
its security is installed, and once either count moves — a password given to
one of them afterwards — the reopened document stops reading through its base
and parses every number itself, as a full open holding its own security would.
The base's warnings and ladder level carry over: the reopened document's
warnings are its base's, as they stand, and then its own — the section's, and
what its own parses raise — and its ladder level is its base's. Its page index
is its own, empty until a count builds it, since an update can change the page
tree.

A reopen of a reopened document is the same step one segment further: the
base's segments and the new section, the new section's entries over the base,
which reads through its own base in turn.

#### The bases it opens whole

A reopen that cannot read only the section opens the base's bytes and the
section whole, over segments (`open_appended`), gives that document the base's
security as `DocumentEditor::view` gives a view, and says why, as
`Reopened::full_open`:

| `FullOpen` | When | Why reading only the section would differ |
| --- | --- | --- |
| `Linearized` | the base opened on Annex F's head-only path | its table is the first page's until a read fetches `/T`, and its one revision is the whole file |
| `Streamed` | the base's bytes come from a source | its bytes are not segments to keep; they are fetched whole first, which a fetch declares (ruling 10) |
| `Repaired` | the base's ladder is `Patch` or `Rescan` | a full open of the joined bytes repairs from a scan of all of them, and a rescanned base has no section to chain to |
| `Shifted` | the base's header is not at byte zero (7.5.2) | every offset the section states is read through the shift's candidates |
| `Deep` | the base is itself 32 reopens deep | every read of a number the chain leaves alone walks it to the bottom |
| `Unchained` | the section's `startxref`, cross-reference data or trailer does not read, its `/Prev` is not the base's last section, `/Encrypt` or an encrypted base's first `/ID` string moved, it redefines the `/Encrypt` dictionary, an entry's header is not the object it names, the base's last revision has no `%%EOF` of its own, or the base's chain is already as long as a full open walks | a full open would walk, warn and repair what the section alone cannot say, and stop its chain where the section alone cannot see |

Nothing else sends a reopen to a full open, and a full open is never a guess:
each row is a test of the base or of the section the reopen has just read.

#### `DocumentEditor::view`

`view` is the reopen of the editor's document over `save_update`'s section,
`full_open` dropped: an untouched editor still answers with its document, and
a view is still a snapshot.

#### The facade's call

`tinker_pdf::write::append(document, editor, options)` takes an editor over
`document` — the same `Arc<CosDocument>`, or `AppendError::NotThisDocument` —
writes `save_update`'s section, reopens `document`'s object layer over it, and
hands back `Appended`: the section (`update`), the reopened `Document`
carrying `document`'s font provider and archive report, the `FullOpen` when
there was one, the numbers the update changed (the editor's changed set,
`DocumentEditor::changed_numbers`), and the reopen's counts when
`AppendOptions::stats` asked — on an editor that counts, its whole tally, as a
save hands it back. `Appended::patch`
re-renders one of its pages over a bitmap of the page before the edit, and
keeps the document the update was written over for it.

#### The counted quantities

**At the object layer** (`doc.rs`, under `cfg(test)`, per thread): the object
numbers an open touches — whose cross-reference row it reads, whose header it
probes, whose body it parses. A full open of a 1 000-page file touches every
number it has; a reopen touches the numbers of the section's rows, its changed
objects and object zero, and a cross-reference stream's own number when the
section is one. `a_reopen_parses_only_what_changed` holds a one-annotation
update of that file to those.

**At the facade** (MS-01's `Stats`, when `AppendOptions::stats` asks): the
reopen's `xref_entries_read`, at most the section's rows where a full open
reads every row of every section, and its `objects_parsed`, at most the
section's cross-reference stream objects, of which a classic section has none.
Their sum is the row's "objects re-parsed ≤ changed + cross-reference".

**The section** (`write.rs`, under `cfg(test)`, per thread): the bytes the
writer copies out of an original into what it writes — none for
`update_section`, and the original's length for `incremental_update`, the one
copy a whole-file save makes.

**The segments** (`source.rs`, under `cfg(test)`, per backing): the bytes
gathered across a seam and the bytes joined into a whole. A document opened
and read over segments joins nothing before a whole-file read, and gathers
only the windows that straddle a seam.

#### The changed rectangle

`Appended::patch(index, &mut previous, options)` takes page `index` of the
reopened document and `previous`, the bitmap `Page::render` gave the same page
of the base under the same `options`, and writes the page after the update
into `previous`; it hands back a `Patch` naming the rectangle it rendered, or
`None` when the reopened document has no page `index`. The page is patched
only when the update changed nothing it draws but annotations; otherwise it is
rendered whole and the patch says why (`PatchFallback`):

- **The page**: the base has another page at `index`, or none — pages
  inserted, deleted or moved (`OtherPage`).
- **The page itself**: its dictionary changed in anything but `/Annots` and
  `/Parent`, the boxes, turn or resources it inherits changed, the update
  changed a number its `/Contents`, its `/Group` or its resources, inherited or
  its own, reach, or the catalog's `/OCProperties`, which shows and hides the
  layers the page draws, changed or reaches a number the update changed
  (`ContentChanged`).
- **The annotations**: an annotation changed when the update changed its
  dictionary or a number its `/AP` reaches; one is added or removed when one
  page's `/Annots` holds it and the other's does not. Those neither changed,
  added nor removed are in the same order on both pages, or the page is
  rendered whole (`Reordered`).
- **Each changed, added or removed annotation's drawn bounds**, on the base's
  page and on the reopened one: none for one that draws nothing — hidden or
  `NoView` by its `/F`, a pop-up, no normal appearance; its `/Rect` for an
  appearance with a `/BBox`, which 12.5.5's fit places on the rectangle and the
  `/BBox` clip confines (`annots::prepare`); and no bound at all for an
  appearance without one, which is drawn where its own `/Matrix` puts it
  (`Unbounded`).

The rectangle is each such `/Rect`'s corners carried through the render's page
transform — its scale, the page's `/Rotate` and its crop box — and the hull of
all of them rounded outward to whole pixels, widened by one pixel on every side
(the renderer's `SLACK`, which a fill's region keeps for the same reason) and
clipped to the page. An edit that changes no drawn bounds changes no pixel, and
the patch is `previous`, as it is with `options.annotations` off. `previous`
must be the whole page at these options — its width, height, format,
premultiplication, stride and bytes the render's, and `options.region` none
(`Mismatch` otherwise) — and must carry no warnings (`PreviousWarned`
otherwise), for the reason below.

#### The patch stays the full render

The rectangle is rendered through `RenderOptions::region` and written over
`previous` at its place. Ruling 5 makes that region the full render's own
pixels, in one frame, and outside it the full render after the edit equals
`previous`: every pixel the changed annotations draw, before the edit and
after it, is inside the rectangle, and everything else the page draws is the
same objects in the same order on both pages, composited onto the same pixels.
Two things a tile cannot see are taken care of:

- **An image run** (ruling 5's named exception, RC-21's class). Whether an
  image joins the run held back from conflation is decided by its overlap with
  what the run holds over the canvas, so a tile can join what the page splits.
  The renderer keeps each run's bounds in the page's frame beside the run, and
  a region render reports (`tinker_pdf_render::RunReach`) where an image that
  could join a run meets the run's bounds outside the region — `unseen`, the
  only place the page could find an overlap the region does not — and every
  image a run could hold that the region does not hold whole (`cut`). Where
  the bounds an image and its run share lie inside the region, every pixel
  either finds covered is one the region holds for both, so the region asks
  what the page asks and answers alike. A render that reports nothing unseen
  is the patch. One that does grows the rectangle to hold what it reported
  and renders again; from the second round on it grows to hold every cut image
  as well, after which every image meets every run inside it. Each round
  reports only what lies outside its rectangle, so the rectangle grows every
  round and stops growing at the page, and the patch never falls back for a
  run.
- **Warnings.** A region render raises a part of what a full render raises: a
  tiling cell is drawn only where it meets the canvas, and its warnings with
  it. With `previous` clean, the page's content and its unchanged annotations
  raise nothing, and every warning the full render after the edit raises is a
  changed annotation's, drawn wholly inside the rectangle; so the patch's
  warnings are the region render's. A `previous` with warnings is rendered
  whole.

The patched bitmap is then the full render's, pixels and warnings. A patch
asked to count hands back what its renders counted, `Stats::pixels_rendered`
among them: the pixels of each canvas a render drew the page onto, every
renderer's but a tiling cell's.
`dirty_region.rs` holds it over moves, resizes, recolourings, removals and
additions on turned and cropped pages at 0.5× to 4× and over RC-21's
three-image page, and P-17.7's fingerprint holds one patched page on every
target.

#### What MS-04 times

`write/edit an annotation, save, reopen, 1000 pages` and its 10-page twin time,
an iteration, an editor over the document, the appearance replaced
(`put_stream`), and `write::append`: the section written and the document
reopened over it. "≤ 2 ms plus output bytes" allows the 2 ms and a time linear
in the section's bytes — the replaced appearance serialised, its rows and the
trailer — and nothing linear in the base: no copy of its bytes, no walk of its
chain, no probe of its entries, no merged table. The relation holds the
1 000-page file to twice the 10-page one, which only work that does not grow
with the base meets. `render/re-render a changed 100x100 pt annotation` times
`Appended::patch` over the bitmap `render/the annotated text page at 150 dpi`
renders: the rectangle's pixels and one interpretation of the page, held to
10 % of the full render.

### Milestones

1. **P-17.1: this section.**
2. **P-17.2: the update section.** Exit: `edit/tests.rs`
   `an_update_section_is_written_without_copying_the_original`;
   `an_incremental_save_keeps_the_original_bytes`,
   `an_incremental_save_chains_to_the_table_it_updates` and
   `encrypt_on_save.rs` unchanged, and `crates/tinker-pdf/tests/signing.rs`
   and `document_timestamp.rs` with them.
3. **P-17.3: segments.** Exit: `crates/tinker-pdf-cos/tests/reopen.rs`
   `two_segments_read_as_their_concatenation`; `source.rs`
   `opening_over_two_segments_copies_neither`; `streaming.rs` unchanged.
4. **P-17.4: the reopen.** Exit: `doc.rs` `a_reopen_parses_only_what_changed`;
   `reopen.rs` `a_reopen_reads_as_a_fresh_open_does`, over random edit
   sequences on encrypted, object-stream and hybrid-reference bases, and
   `every_base_a_reopen_cannot_keep_says_why`; `editor_view.rs` unchanged.
5. **P-17.5: the facade's call.** Exit: `incremental_reopen.rs`
   `an_annotation_edit_saved_and_reopened_parses_only_what_changed`, in both
   builds.
6. **P-17.6: the patch.** Exit: `dirty_region.rs`
   `a_patched_render_is_the_full_render_after_every_annotation_edit` and
   `a_patch_renders_only_its_rectangle`, in both builds.
7. **P-17.7: the patched page's fingerprint.** Exit: `determinism.rs`
   `a_patched_render_reproduces_the_edited_pages_fingerprint`, both paths at
   the one committed hash, under `cargo test` on linux and windows and under
   `--target wasm32-wasip1` locally; `cargo xtask check` green.
8. **P-17.8a: the operations**, through `write::append` and
   `Appended::patch`, unmeasured; `xtask/src/bench.rs`
   `the_committed_baseline_agrees_with_the_tree` holds them as relations.
   **P-17.8b** takes their figures in the timing phase.
9. **P-17.9: the row's close** in MS-02's paired run: the 1 000-page edit, save
   and reopen at most twice the 10-page one, and the patch at most 10 % of the
   full-page render, with `reopen.rs`, `incremental_reopen.rs` and
   `dirty_region.rs`'s counted guards green on linux and windows and on
   `wasm32-wasip1` locally.

### Risks

| Risk | Mitigation |
| --- | --- |
| The section's bytes differ from the tip's append | `incremental_update` is the original and the section; every incremental-save test, `signing.rs` and `document_timestamp.rs` unchanged |
| A read near a seam differs from the joined bytes | A read inside a segment borrows it whole and stops where the joined bytes would; one across a seam gathers its range; `two_segments_read_as_their_concatenation` reads every object, stream and window both ways |
| A kept object depends on a number the update changed | An indirect `/Length`, a container and its `/N` and `/First` are the dependencies a load has, each checked against the section's numbers, and a dependent object is parsed afresh; `a_reopen_reads_as_a_fresh_open_does` edits lengths and containers |
| A password given after the reopen mixes plaintext and ciphertext | Each document counts its security installs, and the reopened one reads through its base only while neither count has moved |
| A base the reopen should not keep is kept | Each `FullOpen` row is a test of the base or the section; `every_base_a_reopen_cannot_keep_says_why` builds one of each |
| A chain of reopens grows without bound | Past 32 deep a reopen opens whole (`Deep`) and starts a chain of its own |
| A patch misses a pixel the edit changed | The rectangle covers each changed annotation's `/Rect` before and after, rounded outward with a pixel of slack; changed content, an unbounded appearance and a reorder render whole; `a_patched_render_is_the_full_render_after_every_annotation_edit` |
| An image run decided in the tile differs from the page's | The region render reports where an image meets its run outside it, and the patch grows to hold that, then every image a run could hold |
| A patch's warnings differ from the full render's | A `previous` with warnings is rendered whole, and a clean page's only warnings after the edit are the changed annotations', inside the rectangle |

## P-25 — Transparency groups and soft masks

*For [ROADMAP](../ROADMAP.md) row P-25.* A transparency group pays for more
than its buffer. A non-isolated group copies its backdrop out of the canvas
beneath it and keeps the copy beside its buffer for as long as it is open, a
knockout group keeps a snapshot of its buffer's starting state beside it too,
and a luminosity soft mask whose `/BC` is not black is installed as a field
the size of the canvas rather than of its box. Inside a group's buffer, which
carries alpha and starts transparent, an opaque Normal paint takes `blend` at
nearly every pixel, because P-07's opaque path hands every pixel whose
destination is not yet opaque to `blend`. This section fixes how a
non-isolated group reads its initial backdrop and a knockout group its initial
state without those copies, how a soft mask carries the value it reads outside
its box, what each consumer of a mask does with that value, what the guards
count as a group-buffer pixel, and which paints into a group buffer take an
opaque Normal path beyond P-07's.

### Scope

- **Backdrops without a copy** (P-25.3): a non-isolated group reads its
  initial backdrop, and a knockout group its initial state, from the canvas
  it will be composited onto.
- **A soft mask at its box** (P-25.2): a `Mask` that reads a stated value
  outside its rectangle, every consumer's rule for it, and `to_mask` read
  row by row in integer.
- **The counts and the guards** (P-25.2, P-25.3): four renderer counts and
  the two bounds `crates/tinker-pdf/tests/group_buffers.rs` holds them to.
- **The opaque Normal path inside groups** (P-25.4): the paints and formats
  it takes, and why it is `blend` exactly.
- **The row's close** (P-25.5).

### Non-goals

- **Any change to a pixel.** 11.4.4's adoption, 11.4.5's per-element restore,
  11.4.7.2's removal and 11.6.5.2's soft masks compute what they compute
  at the tip; every fingerprint, golden and pinned render holds, on the four
  targets.
- **Shape and opacity as two channels.** `knock_out` weights a partially
  covered pixel between its state and the initial one, the approximation the
  engine's single alpha carries, and keeps doing so.
- **A buffer smaller than the group's region.** A group's buffer stays the
  rectangle `open_group` computes — the clip's overlap with the canvas, or the
  canvas — and a mask group's stays its `/BBox`'s paint region; bounding a
  group by what it paints is not this row's.
- **The mask group's own drawing.** The fixture's mask fills its box with an
  axial shading, whose per-pixel evaluation is P-14's.
- **Threads and display lists.** P-15's bands and the admission a display list
  records with are untouched; a group opens, holds and closes in one
  renderer.

### Design

#### What a group holds at the tip

`open_group` (`crates/tinker-pdf-render/src/lib.rs`) allocates the group's
buffer over its region and, for a non-isolated group, `Canvas::extract`s the
same rectangle of the canvas beneath into a second canvas in the group's
format, which `adopt_backdrop` composites into the buffer's colour channels and
then keeps as the buffer's `backdrop`. For a knockout group it then takes
`snapshot()`, a third canvas of the same size, kept in `GroupFrame::initial`
and read by every `knock_out`. `close_soft_mask` turns the mask group's buffer
into a `Mask` the size of its box, and when `/BC` reads as anything but zero
it allocates `Mask::uniform` over the whole canvas and `overwrite`s the box
into it, because a `Mask` reads zero outside itself.

What the copy is read for is `blend`'s `initial`, the backdrop's alpha `a0`,
at every pixel a paint composites (`fill_blended`, `blend_pixel_level`,
`composite`), and `C0`, the backdrop's colour, once a pixel when
`remove_backdrop` takes it back out at close. Any scheme without the copy has
to answer both.

#### The backdrop is the parent

While a group is open its parent canvas sits in `GroupFrame::parent` and
nothing paints into it: every paint goes to the group's buffer, and a nested
group's buffer replaces that one in turn. So the bytes the copy holds are the
parent's own bytes at the same device pixels, and P-25.3 reads them there.

**The parent moves into the group's buffer** for as long as the group is open,
into the slot the copy occupies at the tip, whatever the group's kind: a move
of the canvas, never of its pixels. A non-isolated buffer reads it as its
backdrop; an isolated one holds it and reads nothing from it, which is
11.4.4's zero `a0`. `close_group` takes it back out before anything else, as
it restores the parent before compositing at the tip, so a cancelled render
still hands back the page. `GroupFrame` keeps the rest — where the buffer
stands, the alpha, the mode, the masks, the frame and the clip depth — and no
canvas.

The parent is read at device pixels: the buffer's pixel `(col, row)` is device
pixel `origin + (col, row)`, which is the parent's pixel `that - its origin`.
A group's region never leaves its parent's canvas — `open_group` intersects
the clip with that canvas, or takes the canvas — so every read is in range;
one that were not would read as transparent, as `extract` answers off the
canvas at the tip.

- **`a0`** is the parent's alpha byte there, or 255 for a format that carries
  none, in the parent's own format: an alpha needs no conversion, so the read
  `blend` makes at every composited pixel is one byte, as it is at the tip.
- **`C0`** is the parent's pixel in the group's format, through `source_from`,
  the conversion `extract` reads through at the tip: `adopt_backdrop` writes it
  into the buffer at open and `remove_backdrop` reads it again at close, the
  same bytes both times.

**A knockout group's initial state is a function, not a snapshot.** The
snapshot is taken straight after adoption, so it is fully transparent in an
isolated group and, in a non-isolated one, the parent's colour in the group's
format at zero alpha — exactly what `adopt_backdrop` wrote. `knock_out` reads
it that way: the buffer's transparent start, `Color::TRANSPARENT` in its format
— which is not every byte zero for ink, where it is black ink at no alpha, or
for Lab — or `C0` from the parent with its alpha zero.
`GroupFrame::initial` becomes a `knockout` flag, which `in_knockout` reads.

Ruling 5 holds by the same fact that makes the copy redundant. A tile's group
buffer is the group's region clipped to the tile, its parent is the tile's
canvas, and the tile's canvas holds the page's bytes at every pixel it shares
with the page; so a tile reads, at every pixel, the backdrop a whole-page
render reads.

#### A soft mask at its box, and the value outside it

**A `Mask` gains `outside: u8`**, what it reads beyond its rectangle. Every
clip, every paint's coverage and every mask `to_mask` returns has zero there,
which is the tip's behaviour exactly — a clip must not change. An installed
soft mask carries `MaskFrame::outside`, the luminosity of `/BC` through
`/TR` (zero for `/Alpha` and for the black default), so `close_soft_mask`
installs the box-sized mask with that value instead of a field the size of
the canvas, and `Mask::uniform` and the field's `overwrite` leave that path.

Each consumer's rule:

- **`Mask::at`** returns `outside` beyond the rectangle, where it returns zero
  at the tip.
- **`overlap_at`**, the rectangle a consumer walks, returns the whole canvas
  for a mask whose `outside` is not zero, every canvas pixel then carrying
  coverage; for zero it returns the rectangle clamped to the canvas, as
  at the tip.
- **`intersect`** multiplies two masks into one whose `outside` is the product
  of theirs. Its rectangle is the common one when both outsides are zero (as
  at the tip), the zero-outside mask's own when one is — the product is zero
  outside it — and the smallest holding both when neither is.
  `combined_mask`, a clip times a soft mask, is the first or the second case,
  and reads zero outside the clip as it does at the tip.
- **`paint_region`** limits a paint by each mask's rectangle through
  `Bounds::of_mask`; a mask whose `outside` is not zero limits nothing, which
  is the limit the canvas-sized field set by covering the canvas.
- **`soft_bounds`**, the soft mask's rectangle on a whole-page render, stays
  the frame for a mask whose `outside` is not zero, as it is at the tip when
  the field is installed, and the box's page rectangle otherwise; so
  `page_region` and `page_limit`, and the frames groups open in, compute what
  they compute at the tip.
- **Every reader of `data` by index reads inside the rectangle.**
  `fill_opaque` slices mask rows, so a mask whose `outside` is not zero takes
  `fill_blended`'s per-pixel walk through `at`; `intersect_in_place`'s
  receiver is always a paint's own coverage, whose `outside` is zero, so the
  product keeps zero outside it; `harden` touches paint masks only;
  `knock_out` and `composite` read through `at`.

A tile's soft mask is the box clipped to the tile with the same `outside`,
and reads at every pixel of the tile what the page's reads there (ruling 5).

**`to_mask` reads rows**, per buffer format, in integer, where it asks
`Canvas::pixel` for a `Color` at the tip: a grey channel is its own
luminosity, the weights summing to 1 000; an RGB pixel takes
`Color::luminosity`'s integer weights directly; an ink pixel goes through
`cmyk_to_rgb`, which is integer; an alpha mask reads the alpha byte. A `LabA8`
buffer reaches light only through `lab_to_rgb`'s floats and keeps that read.
Each format's value is the one `pixel` and `luminosity` give, bit for bit.

#### What the guards count

`RenderCounts` gains four counts, handed back by `finish_counted` and added to
the operation's `Work` by `add_render`, as `glyphs_shown` is. All four are
quantities, so `work()` leaves them out.

| Count | One unit is |
| --- | --- |
| `group_buffer_pixels` | a pixel of a buffer a transparency group or a soft-mask group allocates for its own pixels while it is open — its canvas, and at the tip the backdrop copy and the knockout snapshot — counted at the allocation as width × height |
| `group_bound_pixels` | a pixel of a group's region, once a group: the rectangle `open_group` or `open_soft_mask` allocates its canvas over |
| `soft_mask_pixels` | a pixel of a soft mask `close_soft_mask` installs |
| `soft_mask_bound_pixels` | a pixel of that mask group's region, once a mask |

The parent a group reads is not a group-buffer pixel: it was allocated before
the group opened and belongs to the canvas beneath. A pattern cell's canvas, a
glyph mask and a paint's coverage are not group buffers.

`group_buffers.rs` holds `soft_mask_pixels` to at most 1.1 ×
`soft_mask_bound_pixels` (P-25.2) and `group_buffer_pixels` to at most 1.1 ×
`group_bound_pixels` (P-25.3), whole page and in tiles, the bounds then being
each tile's own regions. At the tip a non-isolated group counts twice its
region, a knockout one twice, a non-isolated knockout one three times, and a
pale `/BC` mask the canvas over its box, so each guard fails before its step
lands and holds after.

#### The opaque Normal path inside groups

At full effective alpha under `Normal`, `blend` writes the source and an alpha
of 255 whatever the destination holds. The union alpha is 255, Normal's mix is
the source channel, and the colour comes out `mul255(255 − ab, cs) +
mul255(ab, cs)` renormalised against 255, which is `cs` for every backdrop
alpha `ab` and every channel value `cs` — an identity in `mul255`'s rounding,
held exhaustively by P-25.4's test. A subtractive channel enters Normal
complemented and leaves it complemented again, so an ink buffer gets the
source ink.

So **wherever coverage, the colour's alpha and the state's alpha multiply to
255 under `Normal`, a paint into a group buffer writes the source bytes and
255**, whatever the buffer's alpha, its backdrop or its format: `GrayA8`,
`Rgba8`, `CmykA8` — the document's ink, as `fill_mask_inked` passes it — and
`LabA8`. That reaches past P-07's path three ways: P-07 sends a destination
that is not opaque to `blend`, and refuses a buffer with a backdrop and an ink
buffer outright; for a full-alpha pixel P-25.4 takes all three.

The paints it serves:

- **a fill and a glyph mask**: `fill_mask_inked`'s run of full coverage;
- **an image run and a shading's pixels**: `blend_pixel_level` at level 255
  over a sample whose alpha is 255;
- **a nested group's composite**: `composite` where the nested buffer's own
  alpha, the `Do`'s `ca` and the mask's coverage multiply to 255.

A partially covered pixel keeps the tip's route. In a knockout group the
restore runs before the paint, as at the tip, and a full-alpha paint replaces
whatever the restore left, which `blend` does too.

### Milestones

1. **P-25.1: this section.**
2. **P-25.2: soft masks at their box.** Exit: `group_buffers.rs`
   `a_soft_mask_is_kept_at_its_box` (new) — the installed soft mask's pixels
   for the committed transparency fixture's luminosity mask (`/BC [0.2]`) at
   150 dpi, whole page and in four tiles, at most 1.1 × its `/BBox` clipped to
   the region; `fill.rs` `every_mask_consumer_reads_the_outside_value` (new),
   each rule above against a canvas-sized field holding the same values;
   `canvas.rs` `to_mask_reads_every_format_as_pixel_does` (new), exhaustive
   over every value each format's reading depends on; `soft_mask_clips.rs`, `transparency_groups.rs`,
   `render_regions.rs`'s transparency tiles, the `transparency.ppm` golden and
   the `transparency` fingerprint unchanged on the four targets.
3. **P-25.3: group backdrops at their clipped bounds, once.** Exit:
   `group_buffers.rs` `group_buffers_hold_their_clipped_bounds_once` (new) —
   over the transparency fixture and generated pages (a non-isolated group
   under a blend mode, an isolated and a non-isolated knockout group, groups
   nested three deep, a page group), whole and in tiles, group-buffer pixels at
   most 1.1 × the groups' clipped bounds; `tinker-pdf-render`'s
   `a_group_left_open_hands_back_the_page` (new), a cancelled render and a
   renderer finished inside a non-isolated and an isolated group each
   handing back the page with its pixels; `blend_modes.rs`,
   `transparency_groups.rs`, `render_regions.rs`, the goldens and every
   fingerprint unchanged on the four targets.
4. **P-25.4: the opaque Normal path inside groups.** Exit: `canvas.rs`
   `the_opaque_normal_path_is_blend_in_every_group_format` (new) — over every
   source and destination channel value against a lattice of own and backdrop
   alphas, what the path writes is what `blend` writes, for `GrayA8`, `Rgba8`,
   `CmykA8` and `LabA8`; `an_opaque_paint_inside_a_group_skips_blend` (new),
   no `blend` call for an opaque Normal paint inside an isolated, a
   non-isolated and a knockout group; every group and blend test, golden and
   fingerprint unchanged on the four targets.
5. **P-25.5: the row's close** in MS-02's paired run: the transparency fixture
   at 150 dpi, its floor raised to 2.8× against the planning baseline once
   P-14 has landed; `group_buffers.rs`'s guards green on linux and windows in
   CI's tests and on `wasm32-wasip1` locally; the `transparency`, `blend` and
   `analytic_blend` fingerprints holding on the four targets.

### Risks

| Risk | Mitigation |
| --- | --- |
| The parent, moved into a group, is not handed back — a cancelled render, a group left open at `finish` | Taking it back is `close_group`'s first act, before compositing, as restoring is at the tip, and `finish` closes every group left open through it; `a_group_left_open_hands_back_the_page` and the renderer's cancellation tests |
| A read from the parent is not the byte the copy held | Both go through `source_from` from the same pixels, and nothing can paint into a parent a group owns; P-25.3's suites, goldens and fingerprints on four targets |
| A mask consumer reads `data` past the rectangle and takes the outside for zero, masking a pale-`/BC` page beyond the box | Every consumer is named above with its rule; `every_mask_consumer_reads_the_outside_value`, `soft_mask_clips.rs` and the renderer's pale-backdrop mask off the page's edge |
| The fast path rounds apart from `blend` | The identity is exhaustive in `mul255`, and `the_opaque_normal_path_is_blend_in_every_group_format` compares every channel and alpha lattice point per format |
| A group in a space other than its parent's converts `C0` at open and again at close, where the copy converted once | The common group shares its parent's format, where the conversion is a copy; `a0`, read at every pixel, needs none; P-25.5's paired run times the fixture's DeviceRGB group |
| A count changes what a render does | The counts are additions beside allocations, read only by the door; `stats.rs`'s `stats_do_not_change_a_pixel`, pages and tiles |
