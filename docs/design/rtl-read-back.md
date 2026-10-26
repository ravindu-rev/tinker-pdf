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
  no right-to-left character is untouched (`text_logical_order.rs`'s
  `no_left_to_right_page_moves`).
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

The same fragments are drawn where the text is not in every reordered line:
an inline box's background and border (`draw_page_tagged`'s `draw_box`) and
its outline (`draw_outlines`). And `visual_lines` orders runs alone, so a
replaced element set inline (`LayoutPage::replaced`) takes no part in its
line's order.

**The rule.** An atomic inline (an inline block or a replaced element) is one
unit of its line's order: UAX #9 reads it as U+FFFC OBJECT REPLACEMENT
CHARACTER, a neutral (`css-writing-modes-3` §2.4). `visual_lines` moves the
unit whole, its box fragment and every run set in it by the same distance, so
its transform's origin, background, border and outline stand where its text
is drawn; its own runs are ordered inside it, as the paragraph of their own
they are. The line's extent does not change, so neither does its alignment.
An inline box whose text L2 parts, a styled span across a level boundary, has
its fragment cut where its runs part, one piece for each place its text is
drawn.

**What stays.** What a transform paints past its box is CSS's own (non-goal):
the widened block still paints over its neighbours, and the case's `apart`
verdict counts that. CD-19.8 holds each paragraph at the glyphs over others it
then draws, which its transforms alone ask for.

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
  and an outline on an inline block and on a span in a reordered line, an
  inline image in one, a block rotated and a block skewed, each in a
  right-to-left paragraph and in one with no `dir`, and the same with no
  right-to-left character, which moves nothing;
- the suites' time in release and debug, against the tree without the rule.

### The other cases

Each gets its section from its design step: Left (3), a word space styled
apart across a two-em stretch (CD-19.19); Left (4), the lines the reader reads
from the right whatever the writer draws (CD-19.22); Left (6), a line under a
half turn (CD-19.27); Left (7), a word search does not find under
`scaleX(0.5)` (CD-19.30); Left (9), a comma read apart after a number a
NO-BREAK SPACE holds together (CD-19.32); and Left (5), its findings outside
ruling 14's rules (CD-19.34). The options whose price the owner decides get
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
    half-turn rows of that pin holding.
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
| A span's fragment cut where L2 parts its text draws a border's ends at each cut | A piece keeps only the edges its box has where the box itself starts or ends, and CD-19.8 tests a bordered span across a level boundary |
| A left-to-right page moves | `visual_lines` touches no line without a right-to-left character; `no_left_to_right_page_moves` and the determinism fingerprints hold that |
| A block's own transform still paints over its neighbours, so `apart` never holds for Left (10) | CD-19.8 pins each paragraph at the glyphs over others its transforms ask for, and CD-19's exit counts only the others |
