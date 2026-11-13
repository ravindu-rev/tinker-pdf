# Engineering rulings

Cross-cutting decisions that bind more than one feature. When a feature doc
and a ruling disagree, the ruling wins. **Numbering is frozen**: source
comments cite these by number, so a retired ruling keeps its number and is
marked retired; new rulings append. Every time two features could plausibly
answer the same question differently, the answer gets a numbered ruling here
instead of living in one feature's head.

## Rulings

1. **Never panic on untrusted input.** Binds every crate. Malformed bytes
   produce errors or warnings, never a panic; the rule is enforced by the
   per-format fuzzers ([verification.md](verification.md)), and a fuzz crash
   is a release blocker, not a backlog item.

2. **Degrade, don't fail.** Binds [filters](features/filters.md) and
   [rendering](features/rendering.md). A missing capability (a JBIG2 symbol
   dictionary, a refused JPX marker, full ICC) renders a neutral placeholder
   and appends a structured warning to `Bitmap.warnings`. A page render never
   hard-fails because one image used a rare codec.

3. **Capability scheduling is evidence-driven.** Binds
   [filters](features/filters.md), [rendering](features/rendering.md) and the
   [roadmap](ROADMAP.md). Every capability the field offers is in scope —
   every capability a mature, widely deployed PDF engine offers and every one
   the rest of the field offers, with speed verified by tests, form
   JavaScript a complete implementation, signing with a private key in the
   engine, and OCR last — and none waits for a corpus to ask for it.

   Evidence decides the order. The tiers run in the order the roadmap
   states; inside a tier, corpus hit-rate evidence
   ([verification.md](verification.md)) puts the row a real document needs
   before the row none has asked for yet, and a capability whose count is
   zero is late in its tier rather than refused. A count is what a
   scheduling argument cites, however interesting a capability looks. A
   capability not yet built refuses by name (ruling 2) until its row closes,
   and a scope choice is never filed as a limit: the roadmap's named
   non-goals hold only what is outside this repository's power, and a
   capability declined for want of a count is a row, however late its tier.

4. **Determinism is a contract, not a hope.** Binds
   [rasterizer](features/rasterizer.md) and
   [rendering](features/rendering.md). Fixed-point coverage accumulation,
   integer inner loops, no platform libm on any pixel path; the same input
   produces bit-identical bitmaps on linux, windows, macos and wasm, and the
   committed fingerprints prove it ([determinism](features/determinism.md)).

   The rule has a crate and a check behind it. `tinker-pdf-math` supplies
   `sin`, `cos`, `atan2`, `ln`, `powf` and their relatives, built from
   nothing but the operations IEEE 754 pins exactly, and it is `no_std` so
   that `x.sin()` does not compile inside it. `cargo xtask libm` fails the
   build if any pixel-path crate calls a platform transcendental.

   The boundary is worth stating precisely: `sqrt`, `floor`, `ceil`,
   `round`, `trunc` and `abs` **are** correctly rounded by the standard, so
   they are identical on every target and pixel-path code may use them
   freely. It is only the transcendental family that diverges.

5. **Tiles share the full-page code path.** Binds
   [rasterizer](features/rasterizer.md) and
   [rendering](features/rendering.md). A clipped render is the same pipeline
   over a smaller canvas that stands at its corner of the page — never a
   second implementation, and never a second frame. A tile is pinned
   byte-equal to the full-page subregion, and that test is the permanent
   guard.

   **The guard is `crates/tinker-pdf/tests/render_regions.rs`.**
   `every_tile_is_byte_equal_to_the_page_under_it` renders each fixture
   whole and then one tile at a time through `RenderOptions::region`, and
   asserts each tile equals its rectangle of the whole render byte for byte,
   with no tolerance. The fixtures are text, a diagonal axial shading, an
   image and strokes — four different paths through the rasterizer, only one
   of which the scanline filler anti-aliases — turned a quarter or
   three-quarter turn, with a crop box offset from the media box on both
   axes, and both at once; and pages that put a canvas somewhere inside the
   page — transparency and soft-mask groups, a tiling pattern's cell and a
   Gouraud mesh's buffer. The tile sizes are 64, 37 and 23 against a 91×131
   page, so no lattice divides it and the last row and column are always
   partial. `tiles_at_other_scales_are_byte_equal_too` repeats the whole set
   at 0.5×, 2× and 4×, and
   `tiles_at_the_scales_where_two_frames_rounded_apart_are_byte_equal` at
   0.75×, 1.5× and 3×; `single_pixel_tiles_are_byte_equal` runs a one-pixel
   lattice; `a_stroke_whose_corners_sit_on_a_sub_scanline_tiles_exactly_at_1x`
   holds a stroked rectangle whose miter corners sit on a sub-scanline;
   `annotations_are_tiled_with_the_page` covers the annotation layer, which
   no other fixture there draws from.

   **The mechanism is one frame.** A frame of a tile's own — the page's
   transform with a whole number of pixels taken off `e` and `f` — is exact
   as arithmetic and not as floating point: `fl(u + e)` and `fl(u + e − tx)`
   are two roundings at two magnitudes, and the last ulp reaches a byte
   wherever the exact value sits on one of the rasterizer's grids. So the
   page is drawn through the one transform whatever part of it is asked
   for; the canvas carries its origin (`Canvas::origin`), every mask,
   sampled coordinate, mesh buffer and image run is in device pixels of the
   page, and only the index into a canvas subtracts the corner — an integer
   subtraction, which cannot round. A group's buffer and a soft mask's stand
   in the same frame rather than translating `base`. So every coordinate a
   tile computes is the number the page computes, and the sampler, the
   filler's `ceil`, the image's quad and the mesh all see one lattice.

   **What one frame does not reach, and what reaches it instead.** A few
   things are decided by a *rectangle* rather than pixel by pixel, and a
   mask in a tile only knows the tile's rectangle: which cells of a tiling
   lattice are composited (a cell's rounded-out buffer spills a pixel past
   its box), which neighbours a mesh's anti-aliased fringe takes its colour
   from, and whether a lattice, a mesh or an image run is within its budget.
   The renderer keeps, beside each clip and soft mask, the rectangle it
   would have on a render of the whole page, computed from nothing but
   device coordinates (`Bounds`), and takes all of those decisions there.

   **One decision is the canvas's, so this ruling has an exception, and it
   is pinned.** Whether an image joins the run of images held back so that
   abutting ones do not conflate is decided by whether it *overlaps* what
   the run already holds, and the run holds fragments only over the canvas.
   Two images that overlap outside a tile and abut inside it are one run in
   the tile and two on the page, and the abutting edge then conflates on the
   page and not in the tile. `render_regions.rs`'s
   `an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`
   pins exactly that, and the roadmap holds the gap (RC-21).

6. **Destinations are an enum, everywhere.** Binds
   [document-model](features/document-model.md),
   [writing](features/writing.md), [creation](features/creation.md).
   `Explicit | Named | Uri` — reading never collapses them, writing
   round-trips them. A URI destination is never rewritten into a named one
   and a named one is never flattened to explicit, because the collapse
   loses information a round-trip cannot recover — and it must hold on both
   sides of the API forever.

7. **The `Device` trait is the only seam between interpretation and
   consumers.** Binds [content-and-text](features/content-and-text.md) and
   [rendering](features/rendering.md). Text extraction, rasterization and
   SVG output are all devices; nothing reaches around the interpreter to
   read content streams directly, except the rewrite path built for
   redaction ([editing](features/editing.md)).

8. **Leaf crates stay PDF-free.** Binds every leaf crate. Bytes and plain
   parameters in, bytes and values out; no COS types, no PDF-spec vocabulary
   in their public APIs. This is what keeps them independently fuzzable,
   testable and publishable.

   A leaf is defined, not enumerated: any crate that takes bytes and plain
   parameters and returns bytes and values is a leaf, whatever any list
   says. The leaves and their edges are listed in
   [architecture.md](architecture.md); a leaf-to-leaf edge does not weaken
   this ruling, which is about public APIs rather than about edges.
   `tinker-pdf-layout` is the one leaf whose input is not bytes — a caller
   hands it a tree of plain structs — so it is a leaf on the definition
   rather than on the shape, and its fuzz target drives a structured
   generator with no parser in front of it. Format semantics stay in the
   facade: `zip` turns an archive into names and byte ranges with no opinion
   about what an entry is *for*; `xml` returns markup events with no XPS or
   EPUB vocabulary in its API.

9. **Oracles are subprocesses, never dependencies.** *Retired; superseded
   by ruling 13.* No outside program is a check, so there is no oracle to
   hold at arm's length. One part of it is restated in ruling 13: a check
   that can be absent announces whether it ran, which is why the `RAN` /
   `SKIPPED` grep is the pattern for every job that depends on something
   being present.

   Its reasoning is the argument ruling 13 answers. XPS markup has one right
   answer, so agreement with a second reader is evidence there — bounded by
   the risk that where the second reader is wrong about XPS, both engines
   agree and both are wrong. **CSS does not work like that.** An engine
   whose EPUB support is itself a partial CSS implementation is not a
   reference, so disagreeing with one is not evidence of a bug. A browser is
   the reference implementation of CSS; under ruling 13 none is run, and
   that is a limit this repository names
   ([features/epub.md](features/epub.md)) rather than a job it runs.

10. **Warnings carry provenance.** Binds all reading paths. Every leniency
    action (repaired xref, truncated stream decoded short, substituted
    font, placeholder image) is a typed warning naming the object it
    touched — so "it opened" and "it opened cleanly" are distinguishable,
    which is what makes the leniency ladder debuggable.

11. **The facade is the only public surface.** Binds
    [bindings](features/bindings.md). Bindings and the C ABI project the
    `tinker-pdf` crate 1:1; no binding adds logic, caching or defaults of
    its own. If a binding needs behavior, the facade grows it first. The
    one other crate a binding may project is `tools/sysfonts`, the host's
    system-font provider kept outside the engine crates, projected 1:1 the
    same way (ruling 23, H-24).

12. **Own the parity tests verbatim.** Binds
    [verification.md](verification.md). `tinker_parity.rs` ports the host
    application's test files assertion-for-assertion; when those tests
    change (bug fixes only), the ports follow. Parity claims are
    `cargo test` output, not judgment.

13. **Verification is first-party.** Binds
    [verification.md](verification.md) and every feature. Nothing outside this repository renders, parses, validates or
    measures a document as evidence. Ruling 9 is retired by this one.

    The line is drawn between *adjudicating* and *supplying*, because "no
    third party" read literally is unsatisfiable — the compiler, the CI
    runner and `curl` are all third-party programs. A third-party program
    may host this code, execute it, fetch bytes for it, or generate inputs
    for it. It may never be the thing that says whether the output is
    right. The reason is the same one that put every filter, cipher, font
    parser and rasterizer in this tree: an implementation this project does
    not own is one it cannot answer for.

    Third-party **bytes** stay admissible, with provenance recorded — the
    fetched corpora, fixtures real producers emitted, published normative
    data (Adobe's CMap resources, the Unicode character database), and the
    committed output of a tool that was run once, which is a recorded
    measurement rather than a check. A corpus is not an implementation; it
    is what an implementation is for.

    A one-time comparison against an outside program, a visual one against
    a viewer included, is admissible as an input and never as a check. It
    has the standing the committed JPEG 2000 decodes have in
    `jpx_reference.rs` and the epubcheck verdicts have in
    `tests/epub/EPUBCHECK.tsv`: bytes a program produced once, recorded with
    the tool, its version and the command. Such a record may be cited in a
    document and read by a person. It is not re-run, it gates nothing, and
    it is not called a test — the moment it decides a build, an
    implementation this project does not own has said whether the output is
    right. A recorded measurement is evidence about the run that made it:
    nobody re-runs it, so it cannot silently start disagreeing. A check is
    evidence about now, and a check this project does not own is one it
    cannot answer for on the day it breaks.

    What this costs is written down rather than absorbed.
    [verification.md](verification.md) names the properties a first-party
    suite does not prove. A known gap is manageable; a suite that has
    quietly stopped proving something is not.

14. **Extracted text is in logical order.** Binds
    [content-and-text](features/content-and-text.md),
    [fonts](features/fonts.md), [epub](features/epub.md) and
    [forms](features/forms.md), and every output that reads `Page::text`:
    `plain_text`, `search`, the structured view and the JSON, XML and HTML
    formats alike. A page draws right-to-left text in visual order, and
    reading it in the order the content stream drew it reads an Arabic or
    Hebrew line backwards — a searchable Arabic PDF that searches as nothing
    and a screen reader that reads every word from its last letter. So a line
    holding a right-to-left character is put back into the order it is read
    in by **UAX #9 read backwards and checked forwards**: the line is sorted
    along its baseline, marks kept with the base glyph they sit on, and the
    order returned is one whose text the algorithm draws as the line stands
    (`tinker_pdf_shape::bidi::logical_order`). It is not L2 applied to levels
    resolved over the drawn line: L2 undoes itself only with the logical
    line's levels, and rules W2, W5, W7 and N1 read neighbours, so the drawn
    line resolves to other levels — `نسبة 50%` is drawn with the `%` on the
    number's left, and read that way it comes back as `نسبة %50`. The
    paragraph direction is the one the page declares for the line, where it
    declares one, and the text the page states for its glyphs is read in
    their place (*What a page says*, below). Elsewhere the paragraph
    direction is P2 read off the drawn line, by `Bidi_Class`: a
    left-to-right paragraph draws its first strong character leftmost and a
    right-to-left one rightmost, so the two ends decide, and the majority
    only where they disagree — with the two tie-breaks below.

    Reordering is a decision about every PDF this engine reads, not one a
    book can take, which is why it is a ruling. The three properties that
    make it safe for every PDF are each asserted rather than argued:

    - **A line with no right-to-left character is untouched**, byte for byte
      and quad for quad — `text_logical_order.rs`'s
      `no_left_to_right_page_moves` compares every committed `testdata`
      document and every committed EPUB book against the opt-out.
    - **Both producer habits give one answer.** A producer that draws a
      right-to-left word in visual order and one that draws it in reading
      order with the pen moving left describe the same page; the line is
      sorted by where its glyphs are before it is read back, so the content
      stream's order decides nothing. Reversing the stream instead would
      break the second habit.
    - **The algorithm is Unicode's, held to Unicode's file in the direction
      extraction uses it.** `bidi_conformance.rs` feeds every visual order
      `BidiCharacterTest.txt` states, but for the cases holding a character
      X9 removes, back through `logical_order`: every answer is a
      permutation, every answer but three, named there, draws the stated
      line, how many come back to the file's own text is pinned, and each
      that does not holds a bracket pair. The drawing direction,
      `order_units`, which the check calls, runs the whole of both files.

    UAX #9 is not one-to-one, so "back to the text that was typed" is not a
    property any reader can have: in a right-to-left paragraph
    `שלום 2026 now` and `שלום now 2026` are one picture, and this reads it
    as the second. What it does not undo is named in
    `crates/tinker-pdf/src/text_order.rs`: texts drawn alike; mirroring (L4),
    because whether a producer's `/ToUnicode` names a mirrored glyph's
    character or its shape is not on the page, and bracket pairs, which N0
    reads off the logical text; a paragraph, because a line is resolved
    alone; and vertical lines. The opt-out is additive —
    `Page::text_with(&TextOptions { content_order: true })` is the order the
    content stream drew — and the reordering lives in the facade, so
    `tinker-pdf-content` keeps no edge to `tinker-pdf-shape` and
    `TextDevice` itself collects in stream order.

    **The comma tie-break.** A line that holds at least one right-to-left
    strong character (`R` or `AL`) and **no** left-to-right one (`L`) is read
    as a right-to-left paragraph, since both of its ends are right to left,
    with one exception: where the line, sorted along its baseline, has a
    strong right-to-left character as its **leftmost** unit and **closing
    punctuation** as its **rightmost** — a unit of `Bidi_Class` `CS`, `ON`,
    `ES` or `ET`; not a number (`EN`, `AN`) or a mark (`NSM`); and not an
    opening bracket or quotation mark, `General_Category` `Ps` or `Pi`,
    because a mark that trails a left-to-right reading closes — it is read as
    a **left-to-right** paragraph, so the punctuation trails the word. Units
    that draw nothing are passed over at either end, the set rule L1 resets
    with whitespace at a line's end: `WS`, `S`, `B`, the characters X9
    removes and the isolate formatting characters, and also the two
    `White_Space` characters whose class is `CS` because they separate
    digits, U+00A0 NO-BREAK SPACE and U+202F NARROW NO-BREAK SPACE, so that
    neither is taken for a comma. Every other line keeps the rule above, and
    a line with an `L` character anywhere is out of the tie-break's reach.
    And the tie-break is taken **only where a left-to-right paragraph draws
    the line**: `logical_order`'s search, run for a left-to-right reading,
    must reach an order that `order_units` draws as the line stands — the
    forward check every answer passes, asked once more before the direction
    is chosen — or the line stays right to left.
    `tinker_pdf_shape::bidi::drawn_direction` chooses and asks;
    `logical_order` for a given direction is the function
    `bidi_conformance.rs` holds to Unicode's file.

    *Why.* A right-to-left word and its comma set alone in a left-to-right
    paragraph — Project Gutenberg's Moby-Dick, which CI's `epub-corpus` job
    reads, sets the Hebrew `חו,` alone in a cell of its etymology table —
    draw the comma right of the word; read as a right-to-left line, the cell
    extracts `,חו`, the comma first. A right-to-left paragraph whose line
    *opens* with punctuation draws the same picture, so nothing on the line
    decides between the two; trailing punctuation after a right-to-left word
    quoted in left-to-right text is far commoner than a lone right-to-left
    line that opens with punctuation, so the commoner reading wins.

    *What it reads otherwise.* A line of a right-to-left paragraph that is
    alone on its line, with no left-to-right character, and opens with a
    mark of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is not an opening
    bracket or quotation mark (`Ps`, `Pi`) reads with that mark at its end.
    That class is wide: a dialogue dash (`— שלום`, drawn `םולש —`, reads
    `שלום —`), a bullet, `*`, `+`, `#`, `%`, an ellipsis (`…שלום` reads
    `שלום…`), and a straight or closing-form quotation mark used to open
    (`"שלום` reads `שלום"`; `”` and `»` likewise) — `"` and `'` are `Po`,
    closing as often as they open. **An opening bracket or quotation mark**
    reads as typed where the producer's `/ToUnicode` names the typed
    character: `(١) بند`, `“שלום` and `«مرحبا`. Where `/ToUnicode` names the
    mirrored glyph instead — `»` for `«`, `)` for `(`, as one derived from a
    font's `cmap` may — the line ends in a closing mark and reads like a dash
    (`«مرحبا` drawn with a mirrored `»` reads `مرحبا»`), unless a European
    number follows the mark, as below; which character a producer named is
    not on the page. **A European number after the mark** spares the line:
    `(1) פריט`, `— 2026 שלום` and `• 5 תפוחים` are drawn with the number
    next to the mark, `טירפ )1(`, which no left-to-right paragraph draws —
    there a European number with no right-to-left letter before it is left
    to right (W7), and one with a letter before it is drawn left of that
    letter — so they read as typed. **Two shapes are not spared**:
    Arabic-Indic digits, which are `AN` and untouched by W7, so `— ١ بند`
    reads `١ بند —`; and a number that is not next to the mark,
    `— שלום 5 חו`, which reads `שלום 5 חו —`. All of it is pinned by name,
    so that a change to it is a visible one. **The EPUB writer's book does
    not pay it**: it declares each right-to-left paragraph's direction, so
    `<p dir="rtl">?! שלום</p>` reads `?! שלום`, as typed, the mark styled
    apart or not (`epub_fallback.rs`'s
    `a_run_of_neutrals_opening_a_right_to_left_line_is_drawn_as_typed`).

    *The tests that hold it.* `text_logical_order.rs`:
    `a_right_to_left_word_and_its_comma_alone_on_a_line_read_as_written`
    (`חו,` and `حوت,` on a left-to-right page),
    `a_right_to_left_line_ending_in_a_full_stop_still_reads_right_to_left`
    (`שלום.`, the stop drawn leftmost),
    `a_line_holding_a_left_to_right_character_keeps_its_rule`,
    `a_right_to_left_line_opening_with_a_mark_and_a_number_reads_as_typed`
    (the forward check),
    `a_right_to_left_line_opening_with_a_bracket_or_quotation_mark_reads_as_typed`
    (what opens), `a_right_to_left_line_opening_with_a_dash_reads_the_dash_last`
    (what it reads otherwise) and `no_left_to_right_page_moves`;
    `epub_fallback.rs`'s
    `a_standard_14_hebrew_word_alone_on_its_line_is_drawn_right_to_left`
    (the book's table, `חו,`); `text_order.rs`'s
    `a_word_and_its_comma_alone_read_as_their_paragraph_drew_them` (the
    comma drawn at either end of the word, and `TextLine::rtl` saying which
    direction was read); and in `tinker-pdf-shape`'s `bidi.rs`, the
    tie-break's classes and both readings drawing the line
    (`a_right_to_left_word_and_its_punctuation_alone_read_left_to_right`,
    `the_tie_break_reaches_no_other_line`,
    `a_mark_and_a_number_opening_a_right_to_left_line_read_as_typed`,
    `an_opening_bracket_or_quotation_mark_at_the_right_does_not_trail`,
    `the_blank_separators_are_the_white_space_that_bidi_class_misses`, and
    `a_right_to_left_line_opening_with_a_dash_reads_it_trailing`, which
    pins the shapes it reads otherwise); `unicode.rs`'s
    `what_opens_is_general_category_ps_and_pi`.

    **The bracket tie-break.** After the rules above have chosen a line's
    paragraph direction, a line that draws a **closing bracket left of an
    opening one of its kind** — `)histone deacetylase(`, each hollow turned
    away from what it encloses — is read in the other direction where three
    things hold of that direction's reading: it draws the line as it stands
    (the forward check); it pairs **more brackets in all**, by BD16 over the
    text it reads, than the reading in the direction chosen; and **more of
    its pairs face outward and hug what they enclose** — no whitespace just
    inside either bracket, as a bracket is set against its own text. A pair
    stands facing outward on the page in exactly one case: at a
    right-to-left level, where `/ToUnicode` names the typed character — as
    this engine's writers name it, and as this ruling's reading, which swaps
    no character, already takes it. A pair at a left-to-right level faces
    inward, and so does a pair at a right-to-left level whose `/ToUnicode`
    names the mirrored glyph; **a pair facing inward turns nothing**, since
    nothing on the page says which of the two drew it. But two brackets that
    are **no pair as typed** stand facing outward all the time in
    left-to-right text — a list marker and a later parenthesis,
    `Step 1) open the file (שלום) and 2) close it.`, a smiley, `:)`, a
    parenthesis opened at the end of a line and closed on the next — and
    read from the right they pair; the other two conditions keep those lines
    as typed. Such a reading pairs no more brackets in all than the typed
    text (`(שלום)` read as typed, `) open the file (` read from the right:
    one each), or pairs them with the spaces that stand outside the two
    brackets as typed inside the pair (`see( מלחמה )b` from
    `b) מלחמה (see`).

    And **a pair that may have taken its level from the line before is no
    evidence**: N0 resolves a pair enclosing only the other direction's
    letters by the strong character before its opening bracket, which for a
    pair that opens a line is on the line above, so a left-to-right
    paragraph's line opening with `(הארץ)` after a line that ended in Hebrew
    draws the pair facing outward at its left end, as a right-to-left
    paragraph draws a pair that ends it. A pair with a bracket at the end of
    the line where a reading in the direction chosen starts, enclosing no
    strong character of that direction, is passed over by both readings
    where the line's letters say the direction chosen — they outnumber the
    other's and the reading in that direction pairs a bracket facing outward
    of its own, or they are at least twice the other's and at least eight
    more. And where the comma tie-break chooses left to right for a line
    that ends a sentence — read leftward from its right end, any closing
    quotation marks, then one stop or more — a left-to-right paragraph draws
    that line as right-to-left text with the sentence's end after it, and
    the line opens just left of the stops: a pair with a bracket there,
    enclosing no left-to-right letter, is passed over too. The stops are the
    full stop, the comma, the colon, the semicolon, `!`, `?`, the ARABIC
    COMMA and the ellipses U+2024, U+2025, U+2026 and U+FE19, one or a run of
    one of UAX #14's classes (`.`, `...`, `?!`): `EX`, `!` and `?`; `IS`, the
    full stop, the comma, the colon, the semicolon and the ARABIC COMMA; and
    `IN`, the ellipses. A run that mixes the classes, `.!` or `!…`, ends no
    sentence. The closing quotation marks are `"`, `'` and the ten
    of `General_Category` `Pf` (`»`, `’`, `”`, `›`, U+2E03, U+2E05, U+2E0A,
    U+2E0D, U+2E1D and U+2E21), so that `."`, `.»` and `.”` end a sentence
    as `.` does. A closing quotation mark alone ends none, nor does a
    closing bracket. UAX #14 (Unicode 17.0, the `LineBreak.txt`
    `tinker-pdf-layout` vendors) puts each of these first on a line only
    after a forced break or after a space in the one case it names: no line
    breaks before `!` or `?`, its `EX`, even after a space (LB13, `× EX`);
    before the full stop, the comma, the colon, the semicolon or the ARABIC
    COMMA, its `IS`, only after a space and before a digit (LB15c,
    `SP ÷ IS NU`, which opens a line with `.5`; LB15d, `× IS`, refuses every
    other); before an ellipsis, its `IN`, only after a space (LB22, `× IN`,
    which LB18, `SP ÷`, precedes); and before a closing quotation mark, its
    `QU`, only after a space (LB19, `× [QU - Pi]`, after LB18), and before a
    `Pf` one that a space, a stop, another closing mark or the text's end
    follows not even then (LB15b). `tinker_pdf_shape::bidi::drawn_direction`
    asks it last, of a line that draws a closing bracket left of an opening
    one and of no other; `logical_order` for a given direction does not
    change, and `bidi_conformance.rs` holds it to Unicode's file.

    *Why.* A right-to-left paragraph narrowed so that a line opens with a
    bracketed Latin run, `(histone deacetylase). وقد أصبحنا نعرف الآن`,
    holds eighteen Latin letters and seventeen Arabic ones; its ends
    disagree, and the majority reads it as a left-to-right paragraph,
    `وقد أصبحنا نعرف الآن .)histone deacetylase(` — a text a left-to-right
    paragraph draws as that line stands, so the forward check cannot refuse
    it. Both readings draw the line; only the brackets tell them apart, and
    only because they face outward. A pair from the line before would turn
    lines that read right: a justified English paragraph quoting Hebrew in
    brackets, `Words שלום עולם (מלחמה) and more English text, …`, whose line
    opens with `(הארץ)`; and a paragraph with no `dir`,
    `جميل بيت كبير (نور) يوم.`, broken before the bracket, which draws
    `موي )رون(.`, which the comma tie-break reads left to right and which,
    read from the right, pairs `(نور)` and reads its full stop first.

    *What it reads otherwise.* A text typed with brackets that pair only
    read the other way round, each set against the text between them:
    `x)שלום(y` in a left-to-right paragraph, drawn `x)םולש(y`, reads
    `y(שלום)x`; `bidi.rs` pins it by name. And a right-to-left paragraph
    whose line opens with a run of `!` and `?`, a closing quotation mark
    before stops, or an ellipsis, before a bracket pair — `…(نور) يوم` or
    `?!(نور) يوم` after a `<br/>` or a soft wrap — draws exactly what a
    paragraph with no `dir` draws for a line ending `(نور) يوم…`, and reads
    as that one does, its ending last: `نور) يوم(…`, the pair the other way
    round as well, since the line above that gave it its level is not on
    the line. With the brackets styled apart, a book's structure tree holds
    their order, but a closing quotation mark before the stops comes back
    after them there too: the line's `".` reads `."`. A soft wrap after a
    space can put `"`, `'`, `”…` or an ellipsis first on a line, and such a
    line can read the same way. A right-to-left line that opens with a run
    that mixes the stops, `.!(نور) يوم` after a `<br/>`, ends no sentence and
    reads as typed; a line of a paragraph with no `dir` that ends a pair, a
    word and the same run read the other way, `(نور) يوم!.`, broken before the
    pair, draws that line and reads as it does, its run first. The EPUB
    writer's book declares each paragraph's direction where it holds
    right-to-left text, and reads such lines as typed.

    *What it leaves.* A pair facing inward — a left-to-right paragraph with
    no `dir` whose line has Arabic at both ends and a Latin run in brackets
    between, `… عدة (HDAC). وقد …`, draws exactly what a right-to-left
    paragraph whose `/ToUnicode` names the mirrored glyph draws for
    `وقد … (HDAC) … عدة`, and reads as the second, `(HDAC).` as `.)HDAC(` —
    two texts drawn alike (`content-and-text.md` names it). Taking it the
    other way would assume the typed character on every page, and would
    read every such right-to-left line of a producer that names the mirrored
    glyph with its runs in left-to-right order; this ruling does not make
    that assumption. Nor does it reach a line holding one bracket of a pair
    a line break split, or quotation marks, which BD16 does not pair
    (`content-and-text.md` names both).

    *The tests that hold it.* `tinker-pdf-shape`'s `bidi.rs`:
    `a_pair_drawn_facing_outward_reads_in_the_direction_that_pairs_it`,
    `a_pair_drawn_facing_inward_decides_nothing`,
    `two_pairs_side_by_side_are_not_broken_for_the_bracket_between_them` (the
    pairs in all),
    `brackets_that_are_no_pair_as_typed_keep_the_lines_direction`,
    `brackets_typed_closing_first_against_a_word_read_as_the_other_direction_pairs_them`
    (what it reads otherwise),
    `a_pair_the_line_above_resolved_turns_no_line`,
    `a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line`,
    `a_pair_before_closing_quotes_and_several_stops_turns_no_line` and
    `a_run_that_mixes_the_stops_ends_no_sentence` (a sentence's end);
    `text_logical_order.rs`:
    `a_right_to_left_line_opening_with_a_bracketed_latin_run_reads_as_written`
    and
    `a_list_marker_and_a_wrapped_parenthesis_in_left_to_right_text_read_as_typed`;
    `epub_conservation.rs`:
    `a_bracketed_run_and_its_full_stop_in_right_to_left_text_read_as_written`,
    `a_dirless_line_with_arabic_at_both_ends_and_latin_in_brackets_reads_as_its_drawn_alike_twin`,
    `list_markers_and_glosses_in_left_to_right_text_conserve`,
    `a_bracket_pair_the_line_above_resolved_turns_no_line`,
    `a_dirless_line_opening_with_a_bracket_pair_reads_its_full_stop_last` and
    `a_dirless_line_opening_with_a_bracket_pair_reads_its_closing_quotes_and_stops_last`
    (a sentence's end).

    **What a page says.** A page can say two things its drawn line
    cannot, and `Page::text` takes each where it is said. A line every
    character of which that draws something belongs to marked-content
    sequences whose structure elements declare one horizontal
    `/WritingMode` (ISO 32000-2 §14.8.5.4.2) — `/LrTb` or `/LrBt` left to
    right, `/RlTb` or `/RlBt` right to left — in their `/A` attribute
    objects or the `/ClassMap` classes their `/C` names, or inherited from
    an ancestor along `/P`, is read in that direction: `logical_order` for
    it, no direction read off the line and no tie-break asked. A line with a
    character no element claims, or in sequences that declare two
    directions or none, is read off the line; a vertical mode declares
    nothing. And an `/ActualText` on a marked-content property list (14.9.4)
    replaces the glyphs it encloses in every output that reads `Page::text`:
    a line one span holds reads as its text, in the line's declared
    direction or else its first strong character's; a span holding part of
    a line is one unit of the line's order and stands for its text there;
    and each character of the text stands on the glyph of the span that
    draws it, in the order the line reads its glyphs. The owner widened the
    last to a stated table cell (H-166): where a cell's text is in part a
    structure *element's* `/ActualText`, the table reader stands that text
    on the glyphs its replaced content draws by the same rule, and a cell
    whose replaced content draws nothing that reads keeps no characters;
    the structured view still gives an element's text none. A page that says
    neither reads as the rest of this ruling reads it, to the byte, and the
    lines each *What it reads otherwise* and *What it leaves* names are such
    a page's.

    The EPUB writer says both. Each structure element a run under which
    holds a right-to-left character, and every run under which is set in a
    paragraph of one direction, declares that direction, `/LrTb` or
    `/RlTb`; the read-back reads each line every run of which is declared
    alike in that direction, as the reader will; and a line no drawing tried
    reads right is drawn by the rules inside a `/Span` whose `/ActualText`
    is its text as typed, the sequence open there closed before the span
    and reopened inside it, so that the line reads as typed and each run
    keeps its element. A book with no right-to-left text is written as
    before, byte for byte.

    *Why.* UAX #9 draws two typed texts alike, and nothing on such a line
    says which drew it, so a rule over the drawn line moves a hole rather
    than closing it: one that read a dir-less Hebrew paragraph's stop last
    read 21 of `epub_conservation.rs`'s cases worse in a default build and
    22 with `bundled-fonts`, and of Unicode's conformance files 1 246 lines
    better and 1 326 worse. A page that says how it reads needs no guess.

    *What it costs.* Over `epub_fallback.rs` and `epub_conservation.rs`,
    each case compared with the reader and writer before it, no case reads
    worse in either build and none draws a glyph more over another: in a
    default build 99 of the first's 232 cases and 77 of the second's 119
    read better, and with `bundled-fonts` 94 of 235 and 76 of 119; the
    cases the design doc's *A page that says how it reads* names — a
    dir-less Hebrew paragraph that ends a number and a stop, lines a reader
    reads from the right whatever is drawn, a dir-less Arabic paragraph whose
    inline `word-spacing` span cuts a word, and a comma that a NO-BREAK SPACE
    in the number before it would set apart — conserve, read whole, are found
    and draw no glyph over another. 75 cases of both builds
    read the same characters in the same order with their white space
    otherwise: a line its declared direction reads right is drawn by the
    rules, and where a reader parts a word space stretched past half an em
    before a left-to-right letter it parts that line as it parts a Latin
    one, so in 40 cases a line breaks where it did not, and in 12 a break
    goes.
    The two suites take 1.00 times the time in release and 1.02 in debug,
    each suite within 0.95 to 1.03. The committed documents read as before,
    in either order, and `text_logical_order.rs`, `bidi_conformance.rs`,
    `tagged_pdf.rs` and `pdfua.rs` pass in both builds. A PDF from another
    producer that carries `/ActualText` on marked content reads that text in
    `Page::text`, and one whose declared direction its text contradicts
    reads in the declared one.

    *The tests that hold it.* `text_logical_order.rs`:
    `a_line_a_span_states_whole_reads_as_its_text`,
    `a_span_inside_a_right_to_left_line_is_one_unit_of_its_order`,
    `a_span_across_two_lines_keeps_each_characters_own_glyph`,
    `a_right_to_left_word_a_span_states_keeps_each_letter_on_its_own_glyph`,
    `a_line_its_element_declares_left_to_right_reads_its_stop_last`,
    `a_writing_mode_inherited_or_stated_by_a_class_holds`,
    `a_line_declared_in_part_or_two_ways_or_vertically_reads_as_before` and
    `a_line_the_stream_split_reads_in_its_declared_direction`;
    `tagged_pdf.rs`'s
    `actual_text_on_a_property_list_replaces_its_own_sequence`;
    `tagged_writer.rs`'s
    `a_writing_mode_is_written_alone_and_beside_table_attributes` and
    `a_stated_span_holds_whole_sequences_of_the_elements_drawn_in_it`;
    `epub::tests`'
    `each_element_holding_right_to_left_text_declares_its_paragraphs_direction`,
    `mixed_directions_declare_nothing_and_plaintext_declares_what_p2_finds`,
    `a_line_is_read_back_in_the_direction_its_structure_declares` and
    `a_line_no_drawing_reads_right_states_its_text_round_its_runs_sequences`;
    and `epub_conservation.rs`'s
    `a_dirless_line_with_arabic_at_both_ends_and_latin_in_brackets_reads_as_its_drawn_alike_twin`,
    which reads one page with its structure tree and without.

    **The EPUB writer's read-back.** The EPUB writer reads each line that
    holds a right-to-left character back as a reader of the page reads it,
    on the page it is drawn on, and draws it otherwise where that reads it
    right. Each run is planned once a page for each drawing a reading tries
    it in, and a line of units a reading puts in logical order is searched
    for its reading once a page (`text_order::Readings`). A reading is held
    to the line's words: a line whose rules' drawing reads it with a word
    cut where a reader parts the spacing, or two run together, is searched
    on for a drawing that reads the words, and a line no drawing
    tried reads with its words states its text (*What a page says*). The owner's rules took the time
    that costs: a letter-spaced line, which a reader parts at its spacing
    whatever is drawn, is read back in the whole budget, 128 readings against
    one, and over `epub_fallback.rs` and `epub_conservation.rs` the read-back
    takes 1.09 to 1.14 times the time in release and 1.11 to 1.14 in debug of
    one that held a reading to the letters. Each distinct drawing of a line
    is read once: a run's drawing that writes what its rules' drawing or an
    earlier drawing of the run writes is not read again, so the drawings
    within the budget reach further. The owner took its price: a
    justified Arabic paragraph at `line-height: 0.4`, with no `dir` or right
    to left, whose rows a reader runs into the next, loses its structure
    tree's order, the drawing found right for one line alone read with the
    next line on the page. Under a transform, a line whose
    rules' drawing reads on the page with its letters in order and not its
    words, which nothing tried there reads with its words, is read again in
    its runs' own space, and what that finds is drawn where it reads the
    words right on the page. Where reading each glyph back as the page's
    font gives it back finds no drawing that reads the line right — in
    `W W <b>1 234</b> <i>?!</i> W.`, the number's halves a NO-BREAK SPACE
    apart, a reader reads that space by its glyph name and UAX #9 then reads
    two numbers — the line states its text and reads as typed
    (`epub_fallback.rs`'s
    `a_styled_mark_after_a_number_a_no_break_space_holds_together_reads_as_typed`,
    `a_styled_mark_after_a_number_reads_as_typed_beside_a_letter_the_page_gives_back_otherwise`
    and `a_line_the_page_gives_back_otherwise_than_typed_reads_as_typed`).
    A change of the read-back that reads the cases of `epub_fallback.rs` and
    `epub_conservation.rs` better is taken whatever time it costs. A change of how the reader reads every PDF is
    taken where more lines of the suites and of Unicode's conformance files
    read better than worse, and each line it reads worse is recorded here
    with its figures. A change that reads any case worse is the owner's to
    take or leave, case by case. A word of Greek, Cyrillic or Armenian that
    an inline block cuts reads in two, as a Latin word so cut does. A case
    no change taken reaches is listed in ROADMAP's Bugs. A line turned on the
    page is read along its own baseline, as it reads unturned; the owner took
    the price, that a paragraph read whole only where a turn cut its lines
    otherwise reads as its twin, whatever its twin reads.

15. **Every exit is a local proof.** Binds
    [verification](verification.md) and the [roadmap](ROADMAP.md). An exit
    that says CI runs a test on every pull request is met by the same test
    run locally — natively on Windows, on Linux in a container, and on
    `wasm32-wasip1` under wasmtime — and recorded in its row; CI runs the
    tests again when the owner merges `develop` into `main`, and no pull
    request is opened and no workflow dispatched to meet an exit. Corpora are
    fetched by `cargo xtask corpus-fetch` into the ignored `corpus/` folders,
    fetched again only when `corpus/corpora.lock` changes, run locally and
    never committed. Benchmark figures are the local machine's, in its own
    `baseline.json` entry, taken with nothing else running on it; a step
    that only adds an operation lands it unmeasured.

16. **A measured figure lives in committed data, never in prose.** Binds
    every doc. A count, a time, a size or a band that a run measures is
    committed in the data or test file that holds it — `baseline.json`, the
    ratchet files, a census's floors, a TSV — beside the command that
    regenerates it; a doc states the rule the figure is held to and names
    the file, and the run, its revision and its day go in the commit
    message. A step whose text puts a figure or a date in a doc writes it to
    that file instead.

17. **A band the fifteen-run rule computes is approved in advance.** Binds
    the benches ([verification](verification.md)). The bands
    `verification.md`'s rule gives — each operation's spread half again as
    wide, rounded up to the next 5 %, never under 5 % — are committed
    without a further approval; a wider band or a lowered floor stays the
    owner's (H-78). The planning number P-04 is held to is the byte count a
    level-6 deflate gives on each of MS-04's page-like and photo-like
    inputs, computed once by a reference deflate outside the engine and never
    measured again against a build: it states a target and judges no output.

18. **Vendored data carries a licence the allowlist already admits.** Binds
    every crate that vendors data and every test suite. Test vectors, faces
    and data tables enter the repository only under a licence `deny.toml`
    admits; data under any other licence is fetched locally for a test and
    never committed, or its licence goes to the owner first.

19. **Each wasm build profile holds its own footprint budget.** Binds
    [bindings](features/bindings.md). The default profile keeps the
    release's 2.5 MiB gzipped gate. Data a capability needs beyond it — CJK
    and fallback faces, the script engine, OCR models, locale data — goes in
    an opt-in profile, each held at its first measured size and tolerance
    (MS-06); raising the default profile's gate is the owner's, on MS-06's
    measured figure.

20. **Private-key operations run in the engine, on constant-time paths.**
    Binds [signatures](features/signatures.md) and every crate that holds a
    secret. The engine signs and decrypts with the keys it is handed: every
    operation on a secret operand runs on a path whose branches and memory
    accesses do not depend on the secret, entropy is the caller's, and key
    material is zeroised when it is dropped. The algorithms and key formats
    are tier SG's list; nonces are RFC 6979's deterministic ones, hedged on
    request; a PSS salt is the caller's; keys cross the C ABI and the
    bindings as bytes and a passphrase, never logged, with `RawSigner` and
    two-phase signing for a key held elsewhere; the signing callback is the
    C ABI's one callback for a key it does not hold, a script host's
    requests are queued, and warnings and progress cross as callbacks.

21. **Implementers read specifications and published format documentation,
    never another implementation.** Binds every capability row. Only
    planning reads other engines' public documentation and source listings,
    and only to inventory capabilities; no implementer reads another
    implementation's code, nothing is copied, paraphrased or ported, and no
    other engine's name enters the repository outside the provenance files.
    Where a format's only definition is another tool's published
    documentation of its output, an implementer may read that documentation,
    never the tool's code, and records the source in the provenance file,
    the format named by its role.

22. **A shading may differ from its clause's expression by at most one level
    in 255.** Binds [rendering](features/rendering.md). Axial and radial
    parameters stepped along a scanline, mesh colour stepped along a span
    and sampled functions read from tables (P-14) evaluate ISO 32000-1
    8.7.4.5.3 to 8.7.4.5.5 by increments and tables instead of at every
    pixel. A shaded pixel stays within one level of 255 of the expression
    evaluated directly, the analytic tier's shading assertions hold that
    bound in place of equality, and a fingerprint or golden the mechanism
    moves is reviewed by the owner before it is committed (H-76, H-133).

23. **The owner's answers stand as rules.** Binds every row and step that
    names one. Each owner row below left the roadmap on the answer written
    here, and a step that lands the owner's answer to one lands this. The
    earlier rulings answer the rest: ruling 17 is H-109's and H-112's, ruling
    18 H-92's and H-116's, ruling 19 H-04's, ruling 20 H-09's, ruling 21
    H-06's and ruling 22 H-139's.

    | Row | Question | Answer |
    | --- | --- | --- |
    | H-05 | The roadmap's scope | as written, in the order the roadmap's Order of work gives |
    | H-07 | The never-name rule's reach | the roadmap names no PDF engine, viewer, validator or document tool, each corpus named by its role; the provenance files (`corpus/corpora.lock`, `corpus/README.md`, `THIRDPARTY.md`) keep the names, because recording provenance is their job; fixture bytes untouched |
    | H-08 | The parity amendment | ruling 3 as amended; `architecture.md`'s locked decisions stand, with their pointers to SD-18 and P-15 |
    | H-10 | The save doors and `check_save` | `check_save` and `check_decrypt` projected everywhere; the doors as they are |
    | H-11 | Redaction clause (a): Type 3 procedures | owed work (ED-35) |
    | H-12 | Redaction clause (d) and annotation looks | drawn from a design this repository states, each a reviewed golden |
    | H-13 | A book's `@page` | an opt-in option honours it; the caller's box is the default; HTML creation honours `@page` |
    | H-14 | PDF 2.0's `/Lab` transparency group | composited in Lab with a typed warning |
    | H-16 | The hard rules against the speed mandate | `forbid(unsafe_code)` stays; safe, auto-vectorisable code; a facade pool behind `Parallelism::Serial` by default; a host instruction counter on the bench machine admitted as a dev tool (it executes the code and judges nothing) |
    | H-17 | The regular-expression syntax | a documented linear-time subset |
    | H-18 | Writing weak encryption (R4 AES-128, RC4) | behind an explicit legacy option with a typed weakness warning; R5 never written |
    | H-19 | An archival profile on rewrite | the refusal channel added before 0.1.0 |
    | H-20 | Lossy and fax codecs; partial image redaction | the caller names the policy; redaction recodes losslessly |
    | H-24 | System fonts in the CLI and bindings | a host provider in `tools/sysfonts`, outside the engine crates; ruling 11 admits it |
    | H-25 | A password seam for encrypted archives | beside the PDF password path |
    | H-26 | EPUB scripting; vendor DRM | scripting deny-by-default as form scripts; DRM refused |
    | H-27 | Trust and revocation | no bundled roots; host-supplied lists; the engine builds requests and the host fetches |
    | H-28 | Callbacks across the C ABI | ruling 20 |
    | H-29 | ECDH and other recipient shapes | added with the signing work |
    | H-30 | JPEG XR encoding and JPEG 2000 Part 2 | last in their tiers |
    | H-31 | SVG filters | exact mappings as vectors, the rest rasterised as an option |
    | H-32 | Signing scope | ruling 20: tier SG's list |
    | H-33 | Entropy and nonces | ruling 20 |
    | H-34 | Key material across the C ABI and bindings | ruling 20 |
    | H-35 | "Complete" for form JavaScript | the 2025 editions of ECMA-262 and ECMA-402, complete, with empty exclusion lists; a later edition becomes the target as new milestone rows when it is published; the whole object model the published API reference and ISO 21757-1 describe |
    | H-36 | Document code in a server-side library | a complete engine, deny by default, hosts opt in per trigger |
    | H-37 | The host-facing script APIs | each a typed host request behind FJ-14's gates, the collaboration, network, database and mail objects included; a policy may deny any of them at run time, by name |
    | H-38 | OCR scope | printed text first (Latin, then Cyrillic and Greek, Arabic and Hebrew, Indic, CJK); handwriting last |
    | H-39 | OCR models and training data | trained here from data with known licences |
    | H-40 | OCR models bundled or host-supplied | host-supplied models; one small bundled Latin model behind the `ocr` profile |
    | H-41 | The writer's PDF/UA claim | made only on the caller's explicit assertion that a person reviewed the meaning clauses |
    | H-42 | PNG output and the CLI's jobs | the fast level for rendered output; `--jobs 1` |
    | H-43 | Native CPU baselines for distributed binaries | the baseline build only |
    | H-44 | wasm build flags | `+simd128` by default with a plain fallback; no threaded build |
    | H-45 | Profile-guided optimisation | not adopted |
    | H-48 | Document-AI features | out of the roadmap |
    | H-77 | The performance targets | the table's targets, factors, calibrations and relations gate as written in MS-02's paired comparison; an approval may tighten or re-state one, and loosening one is H-78's |
    | H-135 | A device CMYK colour under `/DefaultCMYK` | an ink render keeps a `k` colour's own components; the light render substitutes the default |
    | H-136 | Native against wasm for K-04 | the replay form: K-04.11 as written, its inputs drawn from three targets that each carry a coverage figure |
    | H-137 | Merge deduplication across encodings | byte-identical encodings merge without decoding; differing encodings keep the decoded comparison, decoded once a save, so `two_encodings_of_one_content_merge` holds; A6 and the merge floors state that mechanism |
    | H-140 | A 16-bit sample | the row as written: the top byte is today's 8-bit arithmetic, bit for bit, the low byte refines it, and no 8-bit fingerprint or golden moves |
    | H-143 | Kashida justification | kashida and `JSTF` justify a line only under the `text-justify` value FT-07e.1 assigns them; under `auto` a line is justified over its spaces |
    | H-145 | Canonical combining order in Brahmic runs | lands only if FT-08.2 finds it loses no passing case; otherwise the gap stays, recorded in `docs/features/fonts.md` with its price |
    | H-146 | A case the vendored face cannot produce | nothing is declined: such a case stays in `TRIAGE` and `PASSING`'s shortfall |
    | H-147 | Metric-matched substitution | opt-in: FT-14.2 to FT-14.4 land behind the switch, off, and no fingerprint, golden or pinned render moves |
    | FT-01.3 | A substituted face for a composite font | read through its registry collection only, never by CID as a glyph index: an Identity-ordered CIDFontType2 with no `/ToUnicode` draws `.notdef` and reports `UnreadableFont` even where the host's face is the document's own; the owner took that price |
    | ED-06a.3 | A recoloured shading under a non-linear conversion | exact wherever the shading's own function can carry the conversion (a Type 4 calculator composed with it); a mesh's per-vertex colours are resampled, subdividing until the worst deviation is within one level of 255, which is reported |
    | ED-24.1 | The optimisation presets | every preset lossless; presets differ in resolution caps, font subsetting and object streams |
    | ED-36e.2–ED-36e.4 | Redaction residue outside the content | scrubbed by default — links and annotations in the area, field values and annotation `/Contents`, `/ActualText`, `/Alt` and `/E`, outline titles, named destinations and attachments — with a typed opt-out naming each place kept; `tpdf redact` inherits it |
    | K-03.9 | A passed-through PNG | verified by one streamed inflate at open; a PNG whose data does not match its header takes the decode route; the owner took the open-time cost |

## How to add a ruling

State it in one bold sentence, name the features it binds, give the reason
in two or three more. If a ruling reverses a feature doc's text, edit the
feature doc in the same commit and cite the ruling number there. A ruling is
never renumbered and never deleted — a retired one keeps its number and a
`*Retired*` marker, because the number is cited from source comments that
outlive any doc edit.
