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
`a_styled_mark_in_a_widened_block_reads_in_order_with_its_block_scaled_about_its_own_text`:
an Arabic paragraph with no `dir` that holds, twice, a word; an inline block
widened by `scaleX(2)` that holds a word and a styled `?!`; a word; an inline
block squeezed by `scaleX(0.6)`, `0.7` or `0.8` that holds a word; and a word
and a stop, at `word-spacing: 0.5em` or `0.55em`, flat, turned a quarter or
scaled along the line. Layout sets a line's runs and boxes in logical order,
and a transformed element's matrix is built about its box fragment
(`Effects::locals`, `local_matrix`): where UAX #9's L2 moved a block's runs
and left its fragment, the block was scaled about a point its text was not
drawn at, and its glyphs landed `1 − s` times the distance between text and
box from where CSS draws them, for a scale `s` along the line. Its background,
border and outline (`draw_box`, `draw_outlines`) stood where its text was not,
and an atomic inline that did not touch its neighbours cut its line in two,
each half ordered alone.

**The rule, as landed.** An atomic inline — an inline block, an `inline-flex`
or a replaced element set inline — is one unit of its line's order, which
UAX #9 reads as U+FFFC OBJECT REPLACEMENT CHARACTER, a neutral
(`css-writing-modes-3` §2.4). Which elements are atomic inlines, and which
anchors each holds, is read from the box tree layout was given, as layout's
`Builder::gather` gathers a box into a line (`paint::Atomics`): the page marks
none, an unpainted inline block has no fragment, and two table cells on one
baseline look like a block on a line. `split_at_levels` sets a stand-in for
each on the line whose baseline crosses its box (`set_objects`, `Hull::on`):
an unpainted run of one U+FFFC, in its line's paragraph, inside the
embeddings its inline ancestors open, as wide as the stretch between its
neighbours. The atomic inline's own runs are lines of their own, ordered as
the paragraph of their own they are, in its own `direction`. Where L2 moves
the stand-in, `visual_lines` moves everything set in the atomic inline by the
same distance — its runs, box fragments, pictures and clips (`carry`) — so
its transform's origin, background, border and outline stand where its text
is drawn, and takes the stand-ins off. The line's extent does not change, so
neither does its alignment.

Three changes beside it keep a block's text readable where its own paragraph
draws it otherwise than its line does. Layout keeps the space after an atomic
inline, which the collapser had folded into the space before it
(`Collapser::passed_atomic`). A slice that opens with a word space may draw it
against the glyph after it (`Cut::Hugged`), so that its box does not reach the
block before it: a reader then reads a block set more than half an em apart
as a line of its own, which the comma tie-break reads left to right, its
trailing `?!` after its word. And on a line under a matrix that turns its
baseline, a right-to-left run's words may be written in reading order
(`Cut::Reading`), and where nothing reads the line's letters in order the
drawing that reads most of its runs in order is taken (`Fonts::search_closer`).

**What stays.** What a transform paints past its box is CSS's own (non-goal):
scaled about its own text, the widened block paints over both its neighbours,
8 glyphs a paragraph at `0.5em`, 7 at `0.55em` and 1 turned a quarter, and the
case's `apart` verdict counts them; the squeezed block draws inside its box.
A block's own paragraph is left to right in a paragraph with no `dir`, so its
trailing `?!` is drawn right of its word: where the block touches its
neighbours, a reader reads it with them, from the right, and the `?!` before
its word.

**Known limits.** A stand-in is set only where its box meets the line's runs:
an atomic inline its margin sets apart from the run at a line's end is not
set, and is not moved; an empty inline block that paints nothing has neither
a fragment nor a run to find it by; and a line of nothing but atomic inlines
has no run to find, and is not ordered. A right-to-left inline block of
`width: auto` is as wide as its line, layout's shrink-to-fit counting the edge
right-aligned text reaches, so it is a line of its own. And the search for
where each atomic inline stands is bounded: a page past `paint::OBJECT_WORK`
comparisons, which only a page a book crafts with thousands of atomic inlines
reaches, sets none, and its lines are ordered by their runs alone.

**What holds it.** The case above, pinned at each paragraph's verdicts and
glyphs over others in both builds; `epub_paint.rs`'s
`an_inline_blocks_background_border_and_outline_stand_around_its_own_text_in_a_reordered_line`,
`a_turned_or_skewed_inline_block_turns_about_its_own_text_in_a_reordered_line`,
`an_inline_block_in_a_line_with_no_right_to_left_character_moves_nothing`
and `an_inside_marker_is_on_its_items_start_side_beside_an_inline_block`;
`epub_images.rs`'s `an_inline_img_is_ordered_with_its_words_in_a_right_to_left_line`;
and `tinker-pdf-layout`'s `the_space_after_an_inline_block_is_kept`,
`the_space_after_an_inline_picture_is_kept` and
`spaces_beside_an_atomic_inline_collapse_where_css_collapses_them`.

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
by `rotate(180deg)` at `word-spacing: 1em`. `TextDevice` measured a
horizontal line on the page's axes: the pen at the right edge of a glyph's
box, a `Stretch` kept only where the spacing moved the pen rightwards, a
text object's end resumed within half an em of the pen along the page's `x`
(`disjoint`), a glyph three ems left of the last cut off (`backwards`), and
one half an em off the baseline along the page's `y` (`far`). A half turn
runs a line to the page's left, so the pen stood where its last glyph
started, no stretch was kept, a text object's end parted a glyph gone back
along the line past half an em and none gone on, and `backwards` cut one gone
on past three ems; a quarter turn set every glyph half an em up the page from
the last. The writer's models of the reader measure where a turn counts for
nothing (`paint::along_the_line`), so the rules drew the line as unturned and
it was read otherwise: its fourth line, opening with `25٪`, read `25` alone,
then `يوم٪`, and the paragraph did not conserve.

**The rule, as landed.** A horizontal line is measured in its own frame
(`text.rs`'s `Frame`): `u`, its first glyph's direction over its length, and
`n`, `u` turned a quarter; a point's place along the line is its dot product
with `u`, and across it its dot product with `n`. `PendingLine::pen` is the
furthest place of a glyph's corners, a new line's pen its anchor's, and a
`Stretch`'s pen that of `corner(advance + spacing, 0)`, kept where it is past
the pen; `disjoint`, `backwards`, `carried` and `spaced` take places along
the line and `far` across it, each glyph in the frame of the line it
continues; and the ends Left (3)'s mend compares (`End`) are the nearest
places of their glyphs' corners. The rest of a line (`PendingLine::rest`)
takes the frame of the line it is the rest of, so that `mends` compares the
two lines' ends on one axis: a glyph drawn mirrored is not `turned`, and
measured in a frame of its own the rest would be compared on the opposite
one. `turned`, the thresholds, vertical lines and `finish` are as they were.
A line whose direction is `(a, 0)` with `a > 0` — every unturned line and
every one under a positive scale — takes the page's `x` and `y` themselves,
not a dot product, so its lines, characters and order are those the page's
axes give, to the bit; so does a vertical line, and one whose direction has
no finite, non-zero length. Quads, origins, `text_order` and the writer's
rules do not change; what the writer draws changes only where its read-back
reads a turned line otherwise.

**What it reads.** A turned line reads as the line unturned, character for
character, but where a gap lies exactly on a threshold, which the turned sums
can resolve otherwise. Left (6) conserves, as its twin does. One such
sentence under a half turn reads whole and its twin does not, by such a tie:
its first `2` stands exactly three ems back from the word before it, which
the turned sums put past `backwards`'s threshold and the page's `x` does not,
so under the turn the writer draws its first word space with its word, and a
reader reads it; unturned, the space is drawn alone, dropped as a line of
white space, and the line reads `كتاب25٪`. Rows that conserved, read whole
or found every word only because a reader cut a turned line on the page's
axes read as their twins: the punctuation shape under `rotate(12deg)`, whose
climbing baseline was cut every half an em up the page, reads as its twin,
which a reading held to its words reads whole; and the squeezed Hebrew blocks of
`a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`,
turned a quarter and three quarters, conserve, hold their order and find
every word, as unturned, in both builds. A Latin line under a quarter turn
reads whole. No committed document holds turned text, so no plain PDF's text
changes, and no glyph moves.

**What remains.** The line that opens with `25٪` is read alone, `25 ٪يوم`,
turned or not: the writer draws the sign left of the digits, `AN` after the
paragraph's Arabic (UAX #9's W2), and read alone the line opens with one
European number, `25٪` (W5), where its paragraph would keep the sign with its
number. So the paragraph does not read whole. ROADMAP's Bugs list holds it,
and FT-37 resolves a paragraph across its lines.

**What holds it.** `text.rs`'s
`a_line_under_a_half_turn_is_continued_and_cut_as_it_is_unturned`: the
glyphs of the tests that hold where a line is cut, `justified`'s and
`objects_spaced`'s among them, drawn under a half and a quarter turn at
places exact in binary, read into the same lines, characters and order; and
`an_unturned_line_reads_as_before`: each threshold met exactly — a text
object's end resumed half an em past the pen, a glyph three ems back and one
half an em up the page, a word space's stretch carried and mended within a
twentieth of an em — under the identity, a shift and `scaleX(2)`, continued at
the tie and cut past it, as the page's `x` and `y` decide. In
`a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on`,
each half-turn row and the `rotate(12deg)` row beside its unturned twin, each
held to what it reads, the three sentences and their twin pinned at
`cons logical found apart`; and the turned rows of
`a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`, held
to what they read unturned.

### A reading held to the line's words

**The rule, as landed.** `Fonts::search` holds a reading to the line's
words. A drawing tried that reads them is taken; one that reads the letters
in order and not the words, a word cut where a reader parts the spacing or
two run together (`Read::Letters`), is kept while the search goes on, within
the same 128 readings, and drawn only where none reads the words; and a line
whose rules' drawing reads it so is searched on the same way, the rules'
drawing kept where nothing reads the words (`Search::Letters`). No drawing is
taken that sets more glyphs over others than the rules'. Of the options
CD-19.9 priced, holding each drawing tried to the words (W), searching on
where the rules' drawing runs two words together (WJ) and where it cuts one
(WJC), this is WJC, which reads every case the other two read better and
more, and none worse: the owner's rules took it (ruling 14, H-150).

**What it reads.** In both builds it reads whole Left (5)'s punctuation shape
at `word-spacing: 0.6em`, flat and under `rotate(12deg)`, and at
`line-height: 0.6`; Left (5)'s Arabic line with `;` glued to a word,
letter-spaced and justified; Left (4)'s Persian digits, `٢٠١٠. ۴۵` in a
letter-spaced, word-spaced and justified right-to-left Hebrew paragraph; the
lines whose rules' drawing a reader reads with a number run into the word
before it, `كتاب25٪`; `25٪ <b>W</b>` with no `dir` and `﴾٢٥﴿ <b>W</b>` at
`0.75em` and `1em`; a styled `?!` glued to a Hebrew word; styled `.`, `...`
and `!!` glued to a word; and the first of two right-to-left table cells under
`scaleX(2)` on one line. With `bundled-fonts` it reads whole, too, the
dir-less Hebrew paragraph of inline blocks squeezed by `scaleX(0.5)` at
`0.6em`, flat and turned a quarter and three quarters. It reads no case
worse in either build and sets no glyph over another.

**What it costs.** A line no drawing reads with its words spends the whole
budget: a letter-spaced line, which a reader parts at its spacing whatever is
drawn, is read back 128 times, and 256 under a transform, where it is read
again in its runs' own space. Over `epub_fallback.rs` and
`epub_conservation.rs`, one test at a time, the read-back takes 1.09 to 1.14
times the time in release, and 1.11 to 1.14 in debug, of one that held a
reading to the letters. Ruling 14 records the price.

**What holds it.** `epub_fallback.rs`'s
`a_reading_that_cuts_or_joins_a_word_is_searched_past_for_the_lines_words`,
and the cases it reads right in
`a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on`,
`a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed`,
`a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading`,
`a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space` and
`two_cells_are_read_back_as_a_reader_parts_them_on_the_page`;
`epub_conservation.rs`'s
`a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop`; and
`epub::tests`'
`a_line_whose_rules_drawing_reads_its_letters_in_order_is_searched_for_its_words`
and `a_line_of_units_the_read_back_has_read_is_not_searched_again`, which
hold its readings by count.

### D: each distinct drawing of a line read once

**What decides it.** `Fonts::search` reads a line in each of its runs'
drawings other than the rules' (`drawings_of`), then two runs' at a time, and
keeps the rules' drawing after 128 readings (`READINGS_PER_LINE`). Many of a
run's drawings write the same text objects as its rules' drawing or as
another of its own, and each is read again: on a line of a few spaced runs
the pair that reads it right comes after the budget. `epub_fallback.rs`'s
`a_line_whose_right_drawing_lies_past_the_budget_is_drawn_by_the_rules` pins
Left (8)'s lines so, and what reading each drawing once costs; `epub::tests`'
`a_line_of_units_the_read_back_has_read_is_not_searched_again` holds the
readings of a line that spends the budget, by count.

**The option.** A run's drawing is planned before it is read
(`Fonts::plan_run`), and one whose plan writes what the run's rules' drawing
writes, or what an earlier drawing of the run writes, is not read; the pairs
are of the drawings left. The budget, the order of the drawings and the bound
on glyphs over others stay as they are.

**What CD-19.10 measures**, D prototyped in `Fonts::search` on the landed
`paint.rs` before a reading was held to the line's words, in both builds,
for this section and H-151's row:

- each case whose verdicts change, better or worse, by its `CASE` line:
  `cargo test -p tinker-pdf --test epub_fallback -- --nocapture` and
  `cargo test -p tinker-pdf --test epub_conservation -- --nocapture`, each
  again with `--features bundled-fonts`, and
  `cargo test -p tinker-pdf-shape --test bidi_conformance`;
- each test whose outcome D changes, in those suites and in
  `cargo test -p tinker-pdf --lib epub::`, with the readings `epub::tests`
  counts (`paint::READINGS`);
- the time of `epub_fallback.rs` and `epub_conservation.rs`, one test at a
  time (`--test-threads=1`), in release and debug: D's ratio to the tree
  without it in the same round, the median over seven rounds in debug and
  fifteen in release.

**What D reads.** It sets no glyph over another the tree does not, reads
`epub_conservation.rs` as the tree does, and does not reach
`bidi_conformance.rs`; release reads every case as debug does.

| Option | `epub_fallback.rs`, default (232 cases) | with `bundled-fonts` (235) | `epub_conservation.rs` (119), each build |
|---|---|---|---|
| D | 6 better, 2 worse | 7 better, 2 worse | none changes |

Better, in both builds: Left (8)'s lines, a Hebrew and an Arabic paragraph
after a generated `§ ` at `word-spacing: 0.6em` and `1em`, read whole; an
Arabic line of a styled `?!` glued to the word after it, letter-spaced and
justified, conserves and reads whole; and an Arabic paragraph of styled
`?! ?!` glued to a word at `0.51em` conserves. With `bundled-fonts`, Left
(9)'s dir-less Hebrew line of `1 234،` under `scaleX(2)` at `0.6em` conserves
too, its comma still read apart from its word. Worse, in both builds: the
justified Arabic paragraph of styled stops and brackets at
`line-height: 0.4`, with no `dir` and right to left, loses its structure
tree's order, as the pin names it: a reader runs its rows into the next, and
the drawing D finds right for one line alone is read with the next line on
the page, `(نور)` and `بيت` into one another. The tests D fails are that pin,
by its Left (8) rows and its two rows of what D costs, and
`a_line_of_units_the_read_back_has_read_is_not_searched_again`, whose counts
it lowers.

**What it costs.** D reads less: the line of styled neutrals that spends the
tree's whole budget is read back 93 times, its distinct drawings and their
pairs all read, against 128, and 94 times against 129 under `scaleX(2)`.
Over the two suites, one test at a time, the median ratio to the tree, with
the rounds out of all that ran slower:

| Option | debug, default | debug, `bundled-fonts` | release, default | release, `bundled-fonts` |
|---|---|---|---|---|
| D | 0.91 (0 of 7) | 0.94 (1 of 7) | 0.92 (3 of 15) | 0.94 (2 of 15) |

The tree's own rounds spread over an interquartile range of 5 to 8 per cent
of their median, so D is faster by about the noise, in all but a few rounds:
planning a run's drawings before they are read costs less than the readings
it saves on these suites. D reads two cases worse in each build, so ruling 14
leaves it to the owner, case by case (H-151).

### The other cases

Each gets its section from its design step: Left (4), the lines the reader
reads from the right whatever the writer draws (CD-19.22); Left (9), a comma read
apart after a number a NO-BREAK SPACE holds together (CD-19.32); and
Left (5), its findings outside ruling 14's rules (CD-19.34). The options whose price the owner decides get
their figures here from their measure steps: D (CD-19.10) above, H1
(CD-19.11) and H6 (CD-19.12). Left (2) reads in two, as a
Latin word does, by the owner's decision (CD-19.18).

## Milestones

Every fix step also passes `epub_fallback.rs`, `epub_conservation.rs` and
`bidi_conformance.rs` in both builds.

1. CD-19.6: the page's plans keyed by each run's place in the page; exit:
   `epub/tests.rs`'s `a_run_is_drawn_from_its_plan_by_its_place_in_the_page`.
2. CD-19.7: Left (10)'s rule prototyped and priced; exit: its figures in this
   section and in H-149's row, regenerated by the commands above.
3. CD-19.8: the rule landed as the owner decided on H-149; exit:
   `a_styled_mark_in_a_widened_block_reads_in_order_with_its_block_scaled_about_its_own_text`
   asserting each paragraph's verdicts and glyphs over others in both builds,
   and a test for each of the class's other shapes.
4. CD-19.9 to CD-19.12: the options priced; exit: each option's figures here
   and in its H row, H-150 to H-153.
5. CD-19.14: WJC, as the owner's rules took it; exit: `epub_fallback.rs`'s
   `a_reading_that_cuts_or_joins_a_word_is_searched_past_for_the_lines_words`
   and `epub::tests`'
   `a_line_whose_rules_drawing_reads_its_letters_in_order_is_searched_for_its_words`
   holding the new readings.
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
13. CD-19.30 and CD-19.31: Left (7), reached by CD-19.8, which finds its word
    with `bundled-fonts` in
    `a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`.
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
| Which elements are atomic inlines is decided twice, by layout and by the writer, and the two drift | The writer reads the box tree as layout's `Builder::gather` reads it (`paint::Atomics`), and the class's tests hold the two together; an atomic inline the writer does not find is ordered as its runs were before, unmoved |
| A left-to-right page moves | `visual_lines` touches no line without a right-to-left character; `no_left_to_right_page_moves` and the determinism fingerprints hold that |
| A block's own transform still paints over its neighbours, so `apart` never holds for Left (10) | CD-19.8 pins each paragraph at the glyphs over others its transforms ask for, and CD-19's exit counts only the others |
