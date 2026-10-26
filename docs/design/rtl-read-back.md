# Right-to-left read-back

*For [ROADMAP](../ROADMAP.md) row CD-19.* The EPUB writer draws a line that
holds a right-to-left character so that a reader of the page reads it back in
logical order, and reads each such line back on the page it is drawn on to
choose how it is drawn ([features/epub.md](../features/epub.md), "Each line is
read back on the page it is drawn on"; [ruling 14](../rulings.md)). CD-19
lists the cases that still do not conserve, do not read whole, are not found
by search, or draw a glyph over another. This doc has a design section for
each case, added by the design step that takes it: the case reproduced from
the test that pins it, the code that decides it, and the rule that fixes it or
the options whose price the owner decides.

## Scope

- CD-19's open cases, Left (1) to Left (10), each reproduced from its pinning
  test in the default and `bundled-fonts` builds.
- For each, the mechanism, named by the code that decides it, and either a
  rule with its exit test or the options that would cost something that works
  today, each with what its measure step prices: the cases it reads better and
  worse in each build, the glyphs it draws over others, and its time in
  release and debug.
- What decides, by the owner's rules in ruling 14: a change that reads no
  case worse is taken whatever its time, and a change of the reader where
  more plain-PDF and conformance lines read better than worse; a case read
  worse goes to the owner, and a case no change taken reaches is listed in
  the roadmap's Bugs.
- The suites every rule and option is priced with:
  `crates/tinker-pdf/tests/epub_fallback.rs`,
  `crates/tinker-pdf/tests/epub_conservation.rs` and
  `crates/tinker-pdf-shape/tests/bidi_conformance.rs`, with
  `crates/tinker-pdf/tests/text_logical_order.rs` where one changes how a plain
  PDF is read. Each case prints a `CASE` line with its verdicts and its glyphs
  over others (`epub_support::reading::check`), which is what a measure step
  compares.

## Non-goals

- Taking a change that reads any case worse: that is the owner's, case by
  case, through the H row its step names (ruling 14).
- Another reading of the page: the reader stays ruling 14's, and a line with
  no right-to-left character is untouched. `text_logical_order.rs`'s
  `no_left_to_right_page_moves` holds that for ruling 14's reordering; it
  reads both its sides through `TextDevice`, so a change of `TextDevice`
  holds it by its own condition and `text.rs`'s tests.
- An outside program deciding whether a reading is right (ruling 13).
- What a transform itself paints over other content. CSS Transforms 1 applies
  a transform to the rendering and leaves layout as it is, so a block widened
  by `scaleX(2)` paints past both sides of its box over its neighbours, and the
  writer draws it so.

## Design

### Left (10): a transformed block in a reordered line

**The case.** `epub_fallback.rs`'s
`a_styled_mark_in_a_widened_block_reads_in_order_and_is_pinned_at_what_it_draws_over`:
an Arabic paragraph with no `dir` that holds, twice, a word; an inline block
widened by `scaleX(2)` that holds a word and a styled `?!`; a word; an inline
block squeezed by `scaleX(0.6)`, `0.7` or `0.8` that holds a word; and a word
and a stop, at `word-spacing: 0.5em` or `0.55em`, flat, turned a quarter or
scaled along the line. Every paragraph reads in logical order and draws glyphs
over others, the same in both builds.

**No acceptance path takes a drawing that sets glyphs over others.**
Reproduced from the pin, with each place a drawing is taken traced, in both
builds:

- Where a drawing other than the rules' is taken, by `Fonts::search`'s own
  `Overprint` bound on the page or by `Fonts::reads_closer` after the search
  in the runs' own space, it sets no more glyphs over others on the page than
  the rules' drawing does. `Fonts::reads_whole` takes none.
- On the first line of each paragraph, the search in the runs' own space finds
  the styled run drawn with its neutrals in their level's order
  (`Drawing::level_order`): the `?!` the other way round from the rules'
  drawing. On the page that drawing sets fewer glyphs over others than the
  rules' drawing and reads the line out of order, and `Fonts::reads_closer`,
  or `Fonts::reads_whole` where the rules' drawing reads the line's letters in
  order, refuses it. The rules' drawing is kept.

So the glyphs over others are the rules' drawing's own, and a guard on the
read-back has no drawing to refuse. One that took the refused drawing instead
would give up the line's order and turn the `?!` about, to move glyphs that
stand where they do for another reason.

**Where the glyphs over others come from.** Layout sets a line's runs and
boxes in logical order. `paint::visual_lines` then moves each run of a line
that holds a right-to-left character to where UAX #9's L2 puts it, and leaves
the line's box fragments (`LayoutPage::boxes`) where layout set them. An
inline block's own text, touching the line it sits in, is ordered as runs of
that line (`paint::same_line`), each run moved by itself. A transformed
element's matrix is built about its box fragment (`Effects::locals`,
`local_matrix`), so in a reordered line an inline block is scaled about a
point its text is not drawn at: for a scale `s` along the line, its glyphs
land `1 − s` times the distance between its text and its box from where CSS
draws them. Reproduced with one block transformed at a time: the block
squeezed by `scaleX(0.6)`, which CSS draws inside its own box, sets its word
over its neighbour's; the block widened by `scaleX(2)` is displaced as well as
painted past its box; and with neither transformed nothing is drawn over
anything.

The same fragments place an inline block's background, border and outline
(`draw_page_tagged`'s `draw_box`, `draw_outlines`), so in a reordered line
they stand where the block's text is not. Layout makes no fragment for an
inline box that is not atomic, so a span's text moves with nothing beside it
to move. An atomic inline that does not touch its neighbours, a picture or a
block with padding, a border, a margin or a width past its text, ends its
line for `paint::same_line`, and each half is ordered alone: a right-to-left
line holding a picture keeps its halves in left-to-right order.

**The rule.** An atomic inline (an inline block or a replaced element) is one
unit of its line's order: UAX #9 reads it as U+FFFC OBJECT REPLACEMENT
CHARACTER, a neutral (`css-writing-modes-3` §2.4). `visual_lines` moves the
unit whole, its box fragment and every run, picture and clip set in it by the
same distance, so its transform's origin, background, border and outline
stand where its text is drawn; its own runs are ordered inside it, as the
paragraph of their own they are, in the block's own `direction`. The line's
extent does not change, so neither does its alignment. Which elements are
atomic inlines, and which runs and fragments each holds, is layout's to
record on the page, each atomic inline's anchor and margin box: the page does
not mark one, an unpainted inline block has no fragment, and two table cells
on one baseline look like a block on a line.

**What stays.** What a transform paints past its box is CSS's own (non-goal):
scaled about its own text, the widened block paints over both its
neighbours, more glyphs than it set over others about a misplaced origin,
every one its transform's own, and the case's `apart` verdict counts them.
And a block's own text is a paragraph of its own in the block's `direction`:
in a paragraph with no `dir`, left to right, so a block's trailing `?!` is
drawn right of its word, and a reader of the whole line reads it before the
word. CD-19.8 holds each paragraph at the glyphs over others it then draws.

**What the prototype found.** With the box tree marking the atomic inlines and
a U+FFFC standing in for each in its line's text, the rule reads many of
`epub_fallback.rs`'s cases better in both builds and none of
`epub_conservation.rs`'s otherwise, and no suite's time moves past its noise.
It reads some worse: Left (10)'s paragraphs turned a quarter, whose widened
block now stands over the line's last word, and squeezed blocks holding a
styled `?!`, read reversed as above. Every inline block's decoration and
transform stands around its own text, an inline picture is ordered with its
words, and nothing on a line with no right-to-left character moves. H-149's
row holds the figures. Taken from the box tree, the rule misses an atomic
inline its margin sets apart at its line's end and an empty unpainted one, and
orders no line of atomic inlines alone; recorded by layout, it would not.

**What CD-19.7 measures**, with the rule prototyped against the landed
`paint.rs`, in the default and `bundled-fonts` builds:

- each case of `epub_fallback.rs`, `epub_conservation.rs` and
  `bidi_conformance.rs` whose verdicts change, better or worse, by its `CASE`
  line: `cargo test -p tinker-pdf --test epub_fallback -- --nocapture` and
  `cargo test -p tinker-pdf --test epub_conservation -- --nocapture`, each
  again with `--features bundled-fonts`, and
  `cargo test -p tinker-pdf-shape --test bidi_conformance`;
- Left (10)'s glyphs over others, paragraph by paragraph, and how many of them
  its transforms alone ask for;
- the class's other shapes, which CD-19.8's tests hold: a background, a border
  and an outline on an inline block in a reordered line, an inline image in
  one, a block rotated and a block skewed, each in a
  right-to-left paragraph and in one with no `dir`, and the same with no
  right-to-left character, which moves nothing;
- the suites' time in release and debug, against the tree without the rule.

### Left (3): a word space styled apart across a two-em stretch

**The case.** `epub_conservation.rs`'s
`a_dirless_line_with_a_right_to_left_gloss`, its Left (3) row:
`<p>αλφα <b>βήτα</b> ספר בית xyz.</p>` in a Hebrew book at
`word-spacing: 2em`, left to right with no `dir`. Both builds read it
`αλφα \nβήτα \n.xyz ספר בית`, found and apart only: pinned so in a default
build, held to `found apart` with `bundled-fonts`.

**Which code decides it.** Traced through `TextDevice::show_glyph`, alike in
both builds. Every word space is a standard face's code 32 that `Tw` moves on
two ems past its box, and the one between `βήτα` and the gloss is a run of
its own, styled apart from the bold word and at the paragraph's level. The
device cuts the line after the two spaces that end a text object, at `β` and
at `ת`, the gloss's first drawn glyph, though each starts where `Tw` left the
pen (`Stretch::reaches`): past the half em of slack within which it resumes
a line an `ET` closed, only `Stretch::carries` crosses a stretch, and it
refuses both, the word before each gap and a letter of the line being left
to right. The piece `תיב רפס xyz.` has a Hebrew letter at its left end and a
Latin one at its right, so `bidi.rs`'s `first_strong_drawn` reads it by its
majority as right to left, `.xyz ספר בית`; `text_order`'s rejoin joins only
within the same half em. Nothing `Fonts::search` tries within
`READINGS_PER_LINE` reads otherwise: the lone space leaves the two ems after
its box, or with `Cut::Carried` before it, and with `Cut::Centered` an em
either side, twice the slack, after a line holding no word and so no
`Stretch`. So the stretch's width does not decide: the carry crosses any
width the spacing accounts for, and refuses this one by its directions.

**The rule.** A cut at a word space's stretch is mended once the piece after
it ends. The cut is one `show_glyph` makes for the slack alone (not for a
turn, another baseline or a jump back), after a code 32 that decodes to
white space and that `Tw` moved on, which `Stretch` records, where the next
glyph starts at the stretched pen within `STRETCH_ROUNDING` and is not
carried; the line it starts is marked as the rest of the one before. When
that line ends, as the next starts or in `TextDevice::finish`, it is
appended to the line before where the two hold a right-to-left glyph of
their own (`PendingLine::holds_rtl`) and the leftmost and rightmost of their
glyphs that read either way (`Strong`, kept as glyphs arrive) both read left
to right, so that ruling 14 reads the joined line by its ends, left to right
(`first_strong_drawn`'s first arm). Left (3) then reads
`αλφα \nβήτα ספר בית xyz.`, `αλφα` apart since it and `βήτα` hold no
right-to-left glyph, as the Armenian row at `12px` reads with every verdict
holding. The mend waits for the line's far end, which the gap does not show:
taken there, `αλφα <b>βήτα</b> ספר בית` is one line with a Hebrew right end,
read by its majority, the join the carry refuses.

**What it must not join.** Two table cells, two columns, words a plain PDF
sets apart: each is placed by padding and `border-spacing`, a gutter, a `Td`
or a `Tm`, not where `Tw` left the pen, and `Stretch::reaches` refuses a gap
further than `STRETCH_ROUNDING` from it, as for the carry. A joined line
with a right-to-left glyph at either end, which `first_strong_drawn` reads by
its majority or from the right: `The word` before a longer Arabic phrase,
`שלום abcdef <b>עולם</b>` with no `dir`. And left-to-right text, collected
exactly as now.

**What must not change.** `Stretch::carries`, the slack, `text_order`'s
rejoin, the read-back's drawings, and every `text.rs` test as written: each
line `a_justified_word_space_beside_left_to_right_text_cuts_as_before` cuts
has a right-to-left glyph at one end or holds none. Its `a b` row holds
left-to-right text here; `no_left_to_right_page_moves` reads both its sides
through `TextDevice` and cannot see a change in it.

**What CD-19.20 measures**, in both builds, for this section and H-155's row:

- each case whose verdicts change, better or worse, by its `CASE` line:
  `cargo test -p tinker-pdf --test epub_fallback -- --nocapture` and
  `cargo test -p tinker-pdf --test epub_conservation -- --nocapture`, each
  again with `--features bundled-fonts`, and
  `cargo test -p tinker-pdf-shape --test bidi_conformance`;
- plain PDFs: `cargo test -p tinker-pdf --test text_logical_order` in both
  builds, and each line whose text changes in the documents and books
  `no_left_to_right_page_moves` reads;
- `cargo test -p tinker-pdf-content`, and each test it fails;
- the suites' time in release and debug, against the tree without the rule.

**What the rule reads.** Prototyped as stated, in both builds, it reads
Left (3) whole, and in a default build a `<bdi>` of Latin with Hebrew in it
in a Hebrew paragraph at `word-spacing: 1em` (Left (8)) as well; it reads no
case worse, changes no plain PDF's text and moves no time past the noise, so
the owner's rules take it.

**CD-19.21's exit tests**, where the owner takes it: in `text.rs`, a line
cut at a stretched word space after `a` and mended with a `ב c` set at the
stretched pen, Left (3)'s three pieces read as `x` and `y ב z`, and the
guards that a `ב c` cell set `2px` past or short of the stretched pen, and
`ג` after `א b`, Hebrew at both ends, each stay a line apart; Left (3)'s row
of `a_dirless_line_with_a_right_to_left_gloss` held to
`cons logical whole found apart` in both builds, beside Cyrillic and
Armenian rows at `2em`; and `epub_fallback.rs`, `epub_conservation.rs` and
`bidi_conformance.rs` passing in both builds.

### Left (6): a line under a half turn

**The case.** `epub_fallback.rs`'s
`a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on`:
three sentences of `25٪ <b>W</b> ١٢٣ … 2010،` in an Arabic paragraph turned
by `rotate(180deg)` at `word-spacing: 1em`. In both builds it reads in
logical order, is found and draws nothing over anything, but does not
conserve or read whole: its fourth line, opening with `25٪`, reads `25` alone,
then `يوم٪`.

**`TextDevice` measures a horizontal line on the page's axes.** `show_glyph`
keeps `PendingLine::pen` as `x1.max(x0)`, the right edge of the glyph's box,
and a new line's as `anchor.0`; keeps a `Stretch` where
`corner(advance + spacing, 0.0).0` is past it; hands `Stretch::carries` and
`Stretch::reaches` the origin's `x`; asks `origin.0 > line.pen + slack` once
`end_text` has closed the line (`disjoint`) and
`anchor.0 + expected_gap < line.origin.0` (`backwards`); and `far` reads the
page's `y`. A half turn runs the line to the page's left: `line.pen` is where
the last glyph starts, no stretch is kept, a text object's end parts a glyph
gone back along the line past half an em and none gone on, and `backwards`
cuts one gone on past three ems and none gone back. `turned`, the thresholds,
`far` and `finish`'s blocks read a half turn as no turn. `text_order` already
measures along each line's baseline, and the writer's models of the reader
(`page_lines`, `parted`) measure where a turn counts for nothing
(`along_the_line`): the rules draw the line as unturned, and it is read
otherwise.

**Why no drawing tried reads it right.** Traced in both builds: the fourth
line's `25` is a text object and the next goes back past half an em, so every
drawing `Fonts::search` reads on the page reads `25` apart, an em's stretch
from the rest, which `rejoin_split_lines` does not bridge; in the runs' own
space the rules' drawing reads `25 ٪يوم`, `Search::Letters` and no drawing
found, so the rules' drawing is kept.

**The rule.** A horizontal line is measured in its own frame: `u`, its
`direction` over its length, and `n`, `u` turned a quarter; a point's place
is its dot product with `u` along the line, with `n` across it. `line.pen` is
the furthest place of the glyph's quad's corners, a new line's pen its
anchor's, `Stretch::pen` that of `corner(advance + spacing, 0.0)`, kept where
past `line.pen`; `disjoint`, `backwards`, `carried` and `spaced` take places
along the line and `far` across it, each glyph in the frame of the line it
continues or starts. `turned`, the thresholds, vertical lines and `finish`
stay; a direction of no finite, non-zero length takes the page's axes.

**What must not change.** A line whose direction is `(a, 0)` with `a > 0`,
every unturned line and every one under a positive scale, takes the page's
`x` and `y` themselves, not a dot product: its lines, characters and order,
bit for bit. Quads, origins, `text_order` and `paint.rs` do not change.

**What the rule reaches.** Prototyped in both builds, it reads a half-turned
line as the line unturned, character for character, but where a comparison
lies exactly on its threshold, which the turned sums can resolve otherwise.
So Left (6) conserves, and its fourth line reads `25 ٪يوم`, as unturned, in
every drawing tried, searched on past its letters or not: the writer draws
the sign left of the digits, `AN` after the paragraph's Arabic (UAX #9's W2),
and read alone the line opens with one European number, `25٪` (W5), the
limit `text_order.rs` names, a line resolved on its own. A turned row that
reads whole where its unturned twin does not reads as the twin: the
punctuation shape under `rotate(12deg)`, `كتاب?!`; and the one-sentence row
keeps its reading only by a tie of `backwards`, its first `2` exactly three
ems back from `كتاب`'s last letter: resolved as flat, its word space drawn
alone is dropped, and it reads `كتاب25٪`, as its twin is pinned. A Latin line
under a quarter turn, cut today wherever a glyph stands over half an em up
the page from the last (`far`), and left so by `text_order`, reads whole.

**What CD-19.28 measures**, with the rule prototyped on `text.rs` as CD-19.21
leaves it, in the default and `bundled-fonts` builds:

- each case whose verdicts change, by its `CASE` line, and each turned row
  against its paragraph unturned, with the tie behind any that differs:
  `cargo test -p tinker-pdf --test epub_fallback -- --nocapture` and
  `cargo test -p tinker-pdf --test epub_conservation -- --nocapture`, each
  again with `--features bundled-fonts`, and
  `cargo test -p tinker-pdf-shape --test bidi_conformance`, which reads no
  `TextDevice`;
- the plain PDFs: `cargo test -p tinker-pdf --test text_logical_order`, again
  with `--features bundled-fonts`, `cargo test -p tinker-pdf-content`, and
  each line that changes in a document `no_left_to_right_page_moves` walks;
- the suites' time in release and debug, against the tree without the rule.

**What CD-19.29's tests hold.** `text.rs`'s
`a_line_under_a_half_turn_is_continued_and_cut_as_it_is_unturned`: the
glyphs of the tests that hold where a line is cut, `justified`'s and
`objects_spaced`'s among them, drawn under a half and a quarter turn at
coordinates exact in binary, read into the same lines, characters and order;
and `an_unturned_line_reads_as_before`: gaps exactly on each threshold, under
the identity, a shift and `scaleX(2)`, cut and continued as the page's `x`
decides. In the pin, each half-turn row beside a new unturned twin holds what
the twin holds, Left (6)'s `cons logical found apart`, and a row read worse
holds what the owner takes on H-157.

### The other cases

Each gets its section from its design step: Left (4), the lines the reader
reads from the right whatever the writer draws (CD-19.22); Left (7), a word
search does not find under `scaleX(0.5)` (CD-19.30); Left (9), a comma read
apart after a number a NO-BREAK SPACE holds together (CD-19.32); and
Left (5), its findings outside ruling 14's rules (CD-19.34). The options whose price the owner decides get
their figures here from their measure steps: W, WJ and WJC (CD-19.9), D
(CD-19.10), H1 (CD-19.11) and H6 (CD-19.12). Left (2) reads in two, as a
Latin word does, by the owner's decision (CD-19.18).

## Milestones

Every fix step also passes `epub_fallback.rs`, `epub_conservation.rs` and
`bidi_conformance.rs` in both builds.

1. CD-19.6: the page's plans keyed by each run's place in the page; exit:
   `epub/tests.rs`'s `a_run_is_drawn_from_its_plan_by_its_place_in_the_page`.
2. CD-19.7: Left (10)'s rule prototyped and priced; exit: its figures in this
   section and in H-149's row, regenerated by the commands above.
3. CD-19.8: the rule landed as the owner decided on H-149; exit:
   `a_styled_mark_in_a_widened_block_reads_in_order_and_is_pinned_at_what_it_draws_over`,
   renamed for it, asserting each paragraph's verdicts and glyphs over others
   in both builds, and a test for each of the class's other shapes.
4. CD-19.9 to CD-19.12: the options priced; exit: each option's figures here
   and in its H row, H-150 to H-153.
5. CD-19.14: W, WJ or WJC, as the owner chose; exit:
   `epub_fallback.rs`'s
   `a_reading_that_cuts_or_joins_a_word_is_taken_as_935b077_took_it` and
   `epub::tests`'
   `a_line_whose_rules_drawing_reads_its_letters_in_order_is_read_back_once`
   holding the new readings, or the pins citing the owner's word.
6. CD-19.15: D, as the owner chose; exit:
   `a_line_whose_right_drawing_lies_past_the_budget_is_drawn_by_the_rules`,
   renamed for it, holding Left (8)'s lines read right, or the pin citing the
   owner's word.
7. CD-19.16: H1, as the owner chose; exit: a `bidi.rs` test of a line whose
   left end is a number or a closing bracket, and `epub_conservation.rs`'s
   `a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop` holding
   Left (1).
8. CD-19.17: H6, as the owner chose; exit: a `bidi.rs` test that a run mixing
   the stops does not open a line before a pair, and the mixed runs' rows of
   `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed`
   holding.
9. CD-19.18: Left (2) as the owner decided, in two as a Latin word reads;
   exit: `a_word_of_an_overflow_script_an_inline_block_cuts_reads_as_a_latin_word_does`
   holding that reading in both builds.
10. CD-19.19 to CD-19.21: Left (3)'s section, its reader rule priced and
    landed; exit: the section, reproduced from `epub_conservation.rs`'s
    `a_dirless_line_with_a_right_to_left_gloss`; then `text.rs` tests of a
    line resumed across a stretch its spacing accounts for and of two table
    cells set apart staying two lines, and Left (3)'s rows of that pin
    holding.
11. CD-19.22 to CD-19.25: Left (4)'s section, its rules priced and landed;
    exit: the section, reproducing each shape from its pin; then, for each
    rule taken, a `bidi.rs`, `text_order.rs` or `text.rs` test with its guard,
    the rows it reaches of `a_dirless_line_with_a_right_to_left_gloss`,
    `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed`
    and `a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading`
    holding, and `no_left_to_right_page_moves` passing.
12. CD-19.27 to CD-19.29: Left (6)'s section, its reader priced and landed;
    exit: the section, reproduced from
    `a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on`;
    then `text.rs` tests of a line under a half turn continued and cut as it
    is unturned, and that an unturned line reads as before, with the
    half-turn rows of that pin holding what their unturned twins hold.
13. CD-19.30 and CD-19.31: Left (7); exit: the section, reproduced from
    `a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`;
    then its rows of that pin finding the word with `bundled-fonts`, or the case in the roadmap's Bugs.
14. CD-19.32 and CD-19.33: Left (9); exit: the section, reproduced from
    `what_a_line_read_as_typed_finds_is_drawn_only_where_it_reads_closer_on_the_page`;
    then its rows of that pin conserving with `bundled-fonts`, or the case in the roadmap's Bugs.
15. CD-19.34 to CD-19.36: Left (5)'s findings; exit: the section, reproducing
    each from its pin; then the `'.` rows of
    `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed`
    holding their structure tree's order, and the span's and the `;`'s rows
    of `a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed`
    conserving and reading whole, or the cases in the roadmap's Bugs.

## Risks

| Risk | Mitigation |
|---|---|
| An atomic inline read as U+FFFC gives its neighbours other levels, and a line reads otherwise | CD-19.7 compares every case in both builds before anything lands, and anything read worse is a price for H-149 |
| Moving a box moves what else reads its fragment: a link's rectangle, a clip, a figure's tag | Every reader of `LayoutPage::boxes` and `LayoutPage::replaced` moves with it, and CD-19.8 tests each |
| Which elements are atomic inlines is decided twice, by layout and by the writer, and the two drift | Layout records each atomic inline on its page, its anchor and margin box, and the writer reads that record (CD-19.8) |
| A left-to-right page moves | `visual_lines` touches no line without a right-to-left character; `no_left_to_right_page_moves` and the determinism fingerprints hold that |
| A block's own transform still paints over its neighbours, so `apart` never holds for Left (10) | CD-19.8 pins each paragraph at the glyphs over others its transforms ask for, and CD-19's exit counts only the others |
