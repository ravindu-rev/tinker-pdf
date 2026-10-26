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
    its own. If a binding needs behavior, the facade grows it first.

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
    paragraph direction is P2 read off the drawn line, by `Bidi_Class`: a
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
    so that a change to it is a visible one. **The EPUB writer does not
    widen it**: a run of neutrals styled apart that opens such a line,
    `<p dir="rtl"><i>?!</i> שלום</p>`, is drawn as typed, so that it reads
    `שלום ?!`, the mark as typed; drawn in its level's order it would read
    `שלום !?` (`epub_fallback.rs`'s
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
    COMMA and the ellipses U+2024, U+2025, U+2026 and U+FE19, one or a run
    (`.`, `...`, `?!`); the closing quotation marks are `"`, `'` and the ten
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
    line can read the same way. A run that mixes the stops, `.!` or `!…`, is
    a run of stops and reads the same way, and a `'.` line where the
    overflow font at `0 Tw` meets this rule can lose its structure tree's
    order; both are open in ROADMAP CD-19.

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
    `a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line` and
    `a_pair_before_closing_quotes_and_several_stops_turns_no_line` (a
    sentence's end); `text_logical_order.rs`:
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

    **The EPUB writer's read-back.** The EPUB writer reads each line that
    holds a right-to-left character back as a reader of the page reads it,
    on the page it is drawn on, and draws it otherwise where that reads it
    right. Each run is planned once a page for each drawing a reading tries
    it in, and a line of units a reading puts in logical order is searched
    for its reading once a page (`text_order::Readings`). Under a transform, a line whose rules' drawing
    reads on the page with its letters in order and not its words is read
    again in its runs' own space. Where reading each glyph back as the
    page's font gives it back finds nothing — in `W W <b>1 234</b> <i>?!</i>
    W.`, the number's halves a NO-BREAK SPACE apart, a reader reads that
    space by its glyph name and UAX #9 then reads two numbers — the line is
    searched again with each glyph read as typed, and what that finds is
    drawn only where it reads more of the line's pairs of letters in order on
    the page (`epub_fallback.rs`'s
    `a_styled_mark_after_a_number_a_no_break_space_holds_together_reads_as_typed`
    and
    `a_styled_mark_after_a_number_reads_as_typed_beside_a_letter_the_page_gives_back_otherwise`).
    A change of the read-back that reads CD-19's cases better is taken
    whatever time it costs. A change of how the reader reads every PDF is
    taken where more lines of the suites and of Unicode's conformance files
    read better than worse, and each line it reads worse is recorded here
    with its figures. A change that reads any case worse is the owner's to
    take or leave, case by case. A word of Greek, Cyrillic or Armenian that
    an inline block cuts reads in two, as a Latin word so cut does. A case
    no change taken reaches is listed in ROADMAP's Bugs.

## How to add a ruling

State it in one bold sentence, name the features it binds, give the reason
in two or three more. If a ruling reverses a feature doc's text, edit the
feature doc in the same commit and cite the ruling number there. A ruling is
never renumbered and never deleted — a retired one keeps its number and a
`*Retired*` marker, because the number is cited from source comments that
outlive any doc edit.
