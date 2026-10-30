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
- Another reading of the page than ruling 14's: a line with no
  right-to-left character is untouched, and a line whose page says nothing
  of its direction or text reads as the ruling reads it. `text_logical_order.rs`'s
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
(`css-writing-modes-3` §2.4). Layout records each atomic inline on its page:
its margin box and the unit its line set for it, where the line set the box,
as wide, on the line's baseline, in its paragraph, inside the embeddings its
inline ancestors open, and stamped with its place in reading order
(`tinker_pdf_layout::Page::atomics`). `split_at_levels` sets a stand-in for
each from that record (`set_objects`): an unpainted run of one U+FFFC where
the unit stood, among the page's runs in reading order, in one walk of them.
An empty inline block that paints nothing, one its margins set apart from
the run beside it and a line of nothing but atomic inlines are each set so.
Which anchors each holds is the box tree's to say (`paint::Atomics`). The
atomic inline's own runs are lines of their own, ordered as
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
(`Cut::Reading`), and a line nothing tried reads right is drawn by the rules
and states its text (`Fonts::stated`).

**What stays.** What a transform paints past its box is CSS's own (non-goal):
scaled about its own text, the widened block paints over both its neighbours,
8 glyphs a paragraph at `0.5em`, 7 at `0.55em` and 1 turned a quarter, and the
case's `apart` verdict counts them; the squeezed block draws inside its box.
A block's own paragraph is left to right in a paragraph with no `dir`, so its
trailing `?!` is drawn right of its word: where the block touches its
neighbours, a reader reads it with them, from the right, and the `?!` before
its word.

**Known limits.** A right-to-left inline block of `width: auto` is as wide as
its line, layout's shrink-to-fit counting the edge right-aligned text
reaches, so it is a line of its own.

**What holds it.** The case above, pinned at each paragraph's verdicts and
glyphs over others in both builds; `epub_paint.rs`'s
`an_inline_blocks_background_border_and_outline_stand_around_its_own_text_in_a_reordered_line`,
`a_turned_or_skewed_inline_block_turns_about_its_own_text_in_a_reordered_line`,
`an_inline_block_in_a_line_with_no_right_to_left_character_moves_nothing`
and `an_inside_marker_is_on_its_items_start_side_beside_an_inline_block`;
`epub_images.rs`'s `an_inline_img_is_ordered_with_its_words_in_a_right_to_left_line`;
`epub_fallback.rs`'s
`an_atomic_inline_is_one_unit_of_its_line_wherever_layout_set_it`, the
three shapes and their guard; `epub::tests`'
`a_page_of_many_atomic_inlines_sets_each_from_its_record`; and
`tinker-pdf-layout`'s `a_page_records_each_atomic_inline_with_its_margin_box`,
`the_space_after_an_inline_block_is_kept`,
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

### Each distinct drawing of a line read once

**The rule, as landed.** `Fonts::search` reads a line in each of its runs'
drawings other than the rules' (`drawings_of`), then two runs' at a time, and
keeps the rules' drawing after 128 readings (`READINGS_PER_LINE`). A run's
drawing is planned before it is read (`Fonts::plan_run`), and one whose plan
writes what the run's rules' drawing writes, or what an earlier drawing of
the run writes, reads as that one does and is not read again
(`Fonts::distinct`): the pairs are of the drawings left. The budget, the
order of the drawings and the bound on glyphs over others stay as they are.
Of the options CD-19.10 priced this is D, which the owner took with its price
(H-151).

**What it reads.** In both builds Left (8)'s lines, a Hebrew and an Arabic
paragraph after a generated `§ ` at `word-spacing: 0.6em` and `1em`, read
whole, their right drawing now inside the budget; an Arabic line of a styled
`?!` glued to the word after it, letter-spaced and justified, conserves and
reads whole; and an Arabic paragraph of styled `?! ?!` glued to a word at
`0.51em` conserves. With `bundled-fonts`, Left (9)'s dir-less Hebrew line of
`1 234،` under `scaleX(2)` at `0.6em` conserves too, its comma still read
apart from its word; in a default build the dir-less Hebrew paragraph of
inline blocks squeezed by `scaleX(0.5)` at `1em` reads whole. Lines whose
verdicts do not change read the spaces typed beside a number or a stop,
`ספר 25٪` and `٢٠١٠. ۴۵` where the rules' drawing read `ספר25٪` and
`٢٠١٠ .۴۵`. It sets no glyph over another.

**What it costs.** The justified Arabic paragraph of styled stops and
brackets at `line-height: 0.4`, with no `dir` and right to left, loses its
structure tree's order: a reader runs its rows into the next, and the drawing
found right for one line alone is read with the next line on the page,
`(نور)` and `بيت` into one another. The owner took that price (ruling 14). D
reads less: the line of styled neutrals that spends the whole budget is read
back 93 times, its distinct drawings and their pairs all read, against 128,
and 186 times under `scaleX(2)`, where it is read again in its runs' own
space, against 256. Over `epub_fallback.rs` and `epub_conservation.rs`, one
test at a time, it takes 0.88 to 0.90 of the time without it in release and debug, faster
in every round.

**What holds it.** `epub_fallback.rs`'s
`a_line_whose_right_drawing_lies_past_the_budget_is_reached_by_reading_each_drawing_once`,
Left (8)'s lines read whole and the paragraph at `line-height: 0.4` pinned at
its price, and the pins of the cases it reads better holding them;
`epub::tests`' `a_line_of_units_the_read_back_has_read_is_not_searched_again`,
by its counts.

### A page that says how it reads: declared direction and stated text

**Why no rule over the drawn line closes Left (1).** `Page::text` reads a
line alone, from its glyphs and where they stand, and UAX #9 draws two typed
texts alike: `<p>בראשית. אור ٦٠.</p>` with no `dir` or with `dir="ltr"` is
drawn `٦٠ רוא .תישארב.`, which a right-to-left paragraph draws for
`.בראשית. אור ٦٠`, and `בראשית (אלהים).` after a `<br/>` is drawn
`)םיהלא( תישארב.`. The comma tie-break takes neither line, since its left
end is a number or a closing bracket and no right-to-left letter, so each is
read right to left, its stop first (`epub_conservation.rs`'s
`a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop`). A rule that
takes them (H1, which CD-19.11 measured) moves the hole rather than closing
it: it reads all of Left (1)'s lines with their stop last, and reads 21 of
`epub_conservation.rs`'s cases worse in a default build and 22 with
`bundled-fonts` — right-to-left lines that open with stops before a styled
pair, which a reader reads in two after the pair, `)בראשית(?!` — and of
Unicode's conformance files, read as a drawn line is, 1 246 lines better and
1 326 worse, every one better of a left-to-right paragraph and every one
worse of a right-to-left one, `— שלום 2026` reading `שלום 2026 —`. Nothing on
such a line says which paragraph drew it. The owner declined H1 and every
price: the page says how it reads (H-152).

**The reader's rules.** A PDF can say two things the drawn line cannot, and
the reader takes each where it is said.

1. *A declared direction.* A structure element's Layout attribute
   `/WritingMode` (ISO 32000-2 §14.8.5.4.2) — `/LrTb` or `/LrBt` left to
   right, `/RlTb` or `/RlBt` right to left — on the element or inherited from
   an ancestor, in its `/A` attribute objects or in the `/ClassMap` classes
   its `/C` names, `/A` first. The reader finds the element of each
   sequence on the page through the page's `/StructParents` and the
   `/ParentTree` (14.7.4.4), and of a form's sequences through the form's own
   `/StructParents`; only for a page with a block that holds a right-to-left
   character, and only for the identifiers in such a block, whose pieces of a
   line the stream split are joined before the line is read. A line every
   character of which that draws something belongs to sequences whose
   elements declare one horizontal direction is read in it:
   `bidi::logical_order` for that direction, the paragraph's direction not
   read off the line and no tie-break asked. A line with a character no
   element claims, or two directions, reads as before; a vertical writing
   mode decides nothing, its element's or an ancestor's, since a vertical
   line is not reordered.
2. *Stated text.* An `/ActualText` on a marked-content property list
   (14.9.4), with an `/MCID` or without, replaces the glyphs it encloses in
   `Page::text`, and so in every view that reads it: `TextPage::search`,
   `Page::structured_text` and the text writers. The outermost span open at
   a glyph is the one that holds it (`TextChar::stated`), since a span
   replaces everything inside it. A line all of whose glyphs one span holds
   reads as its text; a span holding some of a line's glyphs is one unit of
   the line's order, read as U+FFFC OBJECT REPLACEMENT CHARACTER is, and
   stands for its text there; a span whose glyphs stand on several lines
   puts its text on the first of them the page reads. Each character of the text
   stands on a glyph of the span that draws it — the first not yet taken, in
   the order the content stream drew them, whose text the text continues
   with, on whichever line it stands — and takes its quad, `/MCID`, stream
   and font, so that search finds
   the text where it is drawn and the structure tree's order holds; a
   character no glyph draws takes the span's box. `/ActualText` on a
   structure element stays the structured view's: an element can span lines
   and pages, where the line has no place for it.

A PDF that says neither reads as ruling 14 reads it, to the byte.

**The writer's rules.**

1. Every structure element a run under which holds a right-to-left
   character, and every run under which is set in a paragraph of one
   direction, declares it (`TextRun::paragraph_rtl`, or what P2 and P3 find
   over the paragraph where `unicode-bidi: plaintext` leaves it to them),
   `/WritingMode /LrTb` or `/RlTb`, beside its Table attributes where it has
   them, decided over the whole content document so an element's halves
   declare alike (`tagging::writing_modes`). An element whose runs are set
   in paragraphs of both directions declares nothing, so whatever reaches a
   run, its element's or inherited, is its own paragraph's direction; an
   element holding no right-to-left character declares nothing either, so a
   book with no right-to-left text is written as before, byte for byte.
2. The read-back reads each line every run of which is declared alike in
   that direction, as the reader will (`Fonts::read_back`,
   `Fonts::declared_of`), and a line no drawing reads right
   is drawn by the rules and stated (`Fonts::stated`,
   `PageBuilder::begin_stated`): inside a `/Span` whose `/ActualText` is
   the text of the runs drawn in it, in reading order, as typed, one span
   where two lines' runs interleave; a line with no other drawing to try is
   read once for it. The open sequence is closed before the span and
   reopened inside it, and closed and reopened after it, so every run keeps
   its element and the structure tree its order. A stated line reads as
   typed.

**What it closes.** In the writer's own books, Left (1), which its declared
direction reads left to right; the prices ruling 14 records for a line read
alone — a right-to-left line that opens with a dash, an ellipsis or a
closing quotation mark, and one that opens with stops before a bracket pair
— which their declared direction reads as typed; and every line no drawing
reads right, stated: Left (4)'s, Left (5)'s findings, Left (9)'s comma and
the price of reading each drawing once. In other PDFs, a tagged PDF that
declares its directions reads in them, and one that states its text reads
it.

**What it costs.** A PDF from another producer that carries `/ActualText` on
marked content reads that text in `Page::text` as well as in the structured
view (`tagged_pdf.rs` pins the flat extraction reading it); one
whose declared direction its text contradicts reads in the declared one.
The lookup reads one entry of the parent tree a stream, found by its
`/Limits`, and each element once, for a page with a block that holds
right-to-left text. The writer's output grows by an attribute on each
element that holds right-to-left text and a span around each stated line.
What the suites read and what it costs in time is CD-19.45's to measure.

**The steps.** CD-19.41, the reader's stated text in `tinker-pdf-content`'s
`TextDevice` and in `text_order`; CD-19.42, the reader's declared direction
in `text_order` and the structure lookup; CD-19.43, the writer's
`/WritingMode` and the read-back read in it; CD-19.44, the writer's stated
lines; and CD-19.45, the suites measured, the cases re-pinned by what they
read and ruling 14 amended.

### A run that mixes the stops ends no sentence

**The rule, as landed.** Where the comma tie-break reads a line left to right
and the line ends a sentence — read leftward from its right end, any closing
quotation marks, then one stop or more — the bracket tie-break passes over a
pair with a bracket just left of the stops, whose level a left-to-right
paragraph took from the line above (`OpenedBefore::before_its_punctuation`).
A run's stops are of one of UAX #14's classes, `EX` (`!`, `?`), `IS` (the
full stop, the comma, the colon, the semicolon and the ARABIC COMMA) or `IN`
(the ellipses), and a run that mixes them, `.!` or `!…`, ends no sentence
(`bidi::Stop`): the pair just left of it is not passed over, and the bracket
tie-break weighs it as on any other line. Of the options CD-19.12 priced this
is H6, which the owner took (H-153).

**What it reads.** In both builds Left (5)'s mixed runs, `.!` and `!…` before
a styled pair after a `<br/>`, in Arabic and Hebrew, conserve and read whole:
`<p dir="rtl">كتاب عوامل<br/>.!<b>(</b>جميل<b>)</b> بيت نعرف</p>` reads its
second line `.!(جميل) بيت نعرف`. No case reads worse, no glyph is set over
another, and no line of Unicode's conformance files, read as a drawn line
is, reads otherwise. A run of one class, `.`, `...`, `?!`, `!!` or `…`,
still ends a sentence, and its right-to-left line reads as ruling 14's price
reads it.

**What it costs.** A line of a paragraph with no `dir` that ends a pair, a
word and the same run read the other way, `(نور) يوم!.`, broken before the
pair, draws the right-to-left line's picture and reads as it does, its run
first; the suites hold no such line. Over `epub_fallback.rs`,
`epub_conservation.rs` and `text_logical_order.rs`, one test at a time, it
takes 0.98 to 1.02 of the time without it in release and debug, within the
noise.

**What holds it.** `bidi.rs`'s `a_run_that_mixes_the_stops_ends_no_sentence`,
and the mixed runs' rows of `epub_conservation.rs`'s
`a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed`.

### The other cases

Each gets its section from its design step: Left (4), the lines the reader
reads from the right whatever the writer draws (CD-19.22); Left (9), a comma read
apart after a number a NO-BREAK SPACE holds together (CD-19.32); and
Left (5), its findings outside ruling 14's rules (CD-19.34). The options whose price the owner decides get
their figures here from their measure steps, D's and H6's as landed above. Left (2) reads in two, as a
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
   `a_line_whose_right_drawing_lies_past_the_budget_is_reached_by_reading_each_drawing_once`
   holding Left (8)'s lines read whole and the price pinned.
7. CD-19.41 to CD-19.45: the page says how it reads, as the owner chose;
   exit: `text_logical_order.rs` tests of stated text and declared
   direction, `epub::tests` of the writer's `/WritingMode` and stated lines,
   and every case of `epub_fallback.rs` and `epub_conservation.rs` pinned by
   what it reads, Left (1)'s conserving.
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
| Which elements are atomic inlines is decided twice, by layout and by the writer, and the two drift | Decided once: layout records each atomic inline on its page (`Page::atomics`), and the writer sets its stand-ins from that record |
| A left-to-right page moves | `visual_lines` touches no line without a right-to-left character; `no_left_to_right_page_moves` and the determinism fingerprints hold that |
| A block's own transform still paints over its neighbours, so `apart` never holds for Left (10) | CD-19.8 pins each paragraph at the glyphs over others its transforms ask for, and CD-19's exit counts only the others |
