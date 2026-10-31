# Right-to-left read-back

*For [ROADMAP](../ROADMAP.md) row FT-37.* The EPUB writer draws a line that
holds a right-to-left character so that a reader of the page reads it back in
logical order, and reads each such line back on the page it is drawn on to
choose how it is drawn ([features/epub.md](../features/epub.md), "Each line is
read back on the page it is drawn on"; [ruling 14](../rulings.md)). Each design
section takes a case that would not conserve, read whole, be found by search,
or keep its glyphs apart: the case reproduced from the test that pins it, the
code that decides it, and the rule that reads it right, with what the rule
costs.

## Scope

- The cases below, each reproduced from its pinning test in the default and
  `bundled-fonts` builds.
- For each, the mechanism, named by the code that decides it, and the rule:
  what it reads, what it costs and the tests that hold it.
- What decides a change, by the owner's rules in ruling 14: a change of the
  read-back that reads no case worse is taken whatever its time, and a change
  of the reader where more plain-PDF and conformance lines read better than
  worse; a change that reads a case worse is the owner's to take or leave,
  case by case, and a case no change taken reaches is listed in the roadmap's
  Bugs.
- The suites a change is weighed with:
  `crates/tinker-pdf/tests/epub_fallback.rs`,
  `crates/tinker-pdf/tests/epub_conservation.rs` and
  `crates/tinker-pdf-shape/tests/bidi_conformance.rs`, with
  `crates/tinker-pdf/tests/text_logical_order.rs` where one changes how a plain
  PDF is read. Each case prints a `CASE` line with its verdicts and its glyphs
  over others (`epub_support::reading::check`), and a change is weighed, in
  both builds, by:
  - each case whose verdicts change, better or worse, by its `CASE` line:
    `cargo test -p tinker-pdf --test epub_fallback -- --nocapture` and
    `cargo test -p tinker-pdf --test epub_conservation -- --nocapture`, each
    again with `--features bundled-fonts`, and
    `cargo test -p tinker-pdf-shape --test bidi_conformance`;
  - plain PDFs: `cargo test -p tinker-pdf --test text_logical_order`, and
    each line whose text changes in the documents and books
    `no_left_to_right_page_moves` reads;
  - `cargo test -p tinker-pdf-content`, and each test it fails;
  - the suites' time in release and debug, against the tree without the
    change.

## Non-goals

- Taking a change that reads any case worse: that is the owner's, case by
  case (ruling 14).
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

### A transformed block in a reordered line

**The case.** `epub_fallback.rs`'s
`a_styled_mark_in_a_widened_block_reads_in_order_with_its_block_scaled_about_its_own_text`:
an Arabic paragraph with no `dir` that holds, twice, a word; an inline block
widened by `scaleX(2)` that holds a word and a styled `?!`; a word; an inline
block squeezed by `scaleX(0.6)`, `0.7` or `0.8` that holds a word; and a word
and a stop, at `word-spacing: 0.5em` or `0.55em`, flat, turned a quarter or
scaled along the line. Layout sets a line's runs and boxes in logical order,
and a transformed element's matrix is built about its box fragment
(`Effects::locals`, `local_matrix`). Were UAX #9's L2 to move a block's runs
and leave its fragment, the block would be scaled about a point its text is
not drawn at, its glyphs landing `1 − s` times the distance between text and
box from where CSS draws them, for a scale `s` along the line; its
background, border and outline (`draw_box`, `draw_outlines`) would stand
where its text is not; and an atomic inline that did not touch its
neighbours would cut its line in two, each half ordered alone.

**The rule.** An atomic inline — an inline block, an `inline-flex` or a
replaced element set inline — is one unit of its line's order, which UAX #9
reads as U+FFFC OBJECT REPLACEMENT CHARACTER, a neutral
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
atomic inline's own runs are lines of their own, ordered as the paragraph of
their own they are, in its own `direction`. Where L2 moves the stand-in,
`visual_lines` moves everything set in the atomic inline by the same
distance — its runs, box fragments, pictures and clips (`carry`) — so its
transform's origin, background, border and outline stand where its text is
drawn, and takes the stand-ins off. The line's extent does not change, so
neither does its alignment.

Three rules beside it keep a block's text readable where its own paragraph
draws it otherwise than its line does. Layout keeps the space after an atomic
inline, which the collapser would otherwise fold into the space before it
(`Collapser::passed_atomic`). A slice that opens with a word space may draw it
against the glyph after it (`Cut::Hugged`), so that its box does not reach the
block before it: a reader reads a block set more than half an em apart as a
line of its own, which the comma tie-break reads left to right, its trailing
`?!` after its word. And on a line under a matrix that turns its baseline, a
right-to-left run's words may be written in reading order (`Cut::Reading`),
and a line nothing tried reads right is drawn by the rules and states its
text (`Fonts::stated`).

**What stays.** What a transform paints past its box is CSS's own (non-goal):
scaled about its own text, the widened block paints over both its
neighbours, and the case pins, paragraph by paragraph, how many glyphs that
draws over others (`Expect::Drawing`); the squeezed block draws inside its
box. A block's own paragraph is left to right in a paragraph with no `dir`, so
its trailing `?!` is drawn right of its word: where the block touches its
neighbours, a reader would read it with them, from the right, the `?!` before
its word, and the three rules above keep it readable, each paragraph of the
case held to `cons logical whole found`.

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

### A word space styled apart across a two-em stretch

**The case.** `epub_conservation.rs`'s
`a_dirless_line_with_a_right_to_left_gloss`, its row of a bold Greek word
before a bare gloss and a Latin tail at `2em`:
`<p>αλφα <b>βήτα</b> ספר בית xyz.</p>` in a Hebrew book at
`word-spacing: 2em`, left to right with no `dir`. Every word space is a
standard face's code 32 that `Tw` moves on two ems past its box, and the one
between `βήτα` and the gloss is a run of its own, styled apart from the bold
word and at the paragraph's level.

**Which code decides it.** `TextDevice::show_glyph`, alike in both builds.
The device cuts the line after the two spaces that end a text object, at `β`
and at `ת`, the gloss's first drawn glyph, though each starts where `Tw` left
the pen (`Stretch::reaches`): past the half em of slack within which it
resumes a line an `ET` closed, only `Stretch::carries` crosses a stretch, and
it refuses both, the word before each gap and a letter of the line being left
to right. Cut there, the piece `תיב רפס xyz.` has a Hebrew letter at its left
end and a Latin one at its right, so `bidi.rs`'s `first_strong_drawn` reads
it by its majority as right to left, `.xyz ספר בית`; `text_order`'s rejoin
joins only within the same half em. Nothing `Fonts::search` tries within
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
(`first_strong_drawn`'s first arm). The case then reads
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
`שלום abcdef <b>עולם</b>` with no `dir`. And left-to-right text, which it
leaves as collected.

**What must not change.** `Stretch::carries`, the slack, `text_order`'s
rejoin, the read-back's drawings, and every `text.rs` test as written: each
line `a_justified_word_space_beside_left_to_right_text_cuts_as_before` cuts
has a right-to-left glyph at one end or holds none. Its `a b` row holds
left-to-right text here; `no_left_to_right_page_moves` reads both its sides
through `TextDevice` and cannot see a change in it.

**What it reads.** In both builds the case reads whole, and in a default
build so does a `<bdi>` of Latin with Hebrew in it in a Hebrew paragraph at
`word-spacing: 1em`; it reads no case worse and changes no plain PDF's text.

**What holds it.** `text.rs`'s
`a_cut_at_a_word_spaces_stretch_is_mended_where_the_line_reads_left_to_right`:
a line cut at a stretched word space after `a` and mended with a `ב c` set at
the stretched pen, and the case's three pieces read as `x` and `y ב z`; and
`a_cut_at_a_word_spaces_stretch_stays_where_the_line_would_not_read_left_to_right`:
a `ב c` cell set two points past or short of the stretched pen, `ג` after
`א b`, Hebrew at both ends, and left-to-right text each stay a line apart.
And the case's row of `a_dirless_line_with_a_right_to_left_gloss`, held to
`cons logical whole found apart` in both builds, beside Cyrillic and Armenian
rows at `2em`.

### A line under a half turn

**The case.** `epub_fallback.rs`'s
`a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on`:
three sentences of `25٪ <b>W</b> ١٢٣ … 2010،` in an Arabic paragraph turned
by `rotate(180deg)` at `word-spacing: 1em`. Measured on the page's axes — the
pen at the right edge of a glyph's box, a `Stretch` kept only where the
spacing moves the pen rightwards, a text object's end resumed within half an
em of the pen along the page's `x` (`disjoint`), a glyph three ems left of the
last cut off (`backwards`), and one half an em off the baseline along the
page's `y` (`far`) — a line under a half turn runs to the page's left: its pen
would stand where its last glyph starts, no stretch would be kept, a text
object's end would part a glyph gone back along the line past half an em and
none gone on, and `backwards` would cut one gone on past three ems; a quarter
turn would set every glyph half an em up the page from the last. The writer's
models of the reader measure where a turn counts for nothing
(`paint::along_the_line`), so the rules draw the line as unturned, and read on
the page's axes its fourth line, opening with `25٪`, would read `25` alone,
then `يوم٪`, and the paragraph would not conserve.

**The rule.** A horizontal line is measured in its own frame (`text.rs`'s
`Frame`): `u`, its first glyph's direction over its length, and `n`, `u`
turned a quarter; a point's place along the line is its dot product with `u`,
and across it its dot product with `n`. `PendingLine::pen` is the furthest
place of a glyph's corners, a new line's pen its anchor's, and a `Stretch`'s
pen that of `corner(advance + spacing, 0)`, kept where it is past the pen;
`disjoint`, `backwards`, `carried` and `spaced` take places along the line and
`far` across it, each glyph in the frame of the line it continues; and the
ends a stretch's mend compares (`End`) are the nearest places of their
glyphs' corners. The rest of a line (`PendingLine::rest`) takes the frame of
the line it is the rest of, so that `mends` compares the two lines' ends on
one axis: a glyph drawn mirrored is not `turned`, and measured in a frame of
its own the rest would be compared on the opposite one. A line whose
direction is `(a, 0)` with `a > 0` — every unturned line and every one under a
positive scale — takes the page's `x` and `y` themselves, not a dot product,
so its lines, characters and order are those the page's axes give, to the
bit; so does a vertical line, and one whose direction has no finite, non-zero
length. Quads, origins, `text_order` and the writer's rules do not take the
frame.

**What it reads.** A turned line reads as the line unturned, character for
character, but where a gap lies exactly on a threshold, which the turned sums
can resolve otherwise. The case conserves and reads whole, as its twin does:
the line that opens with `25٪` is stated, turned and unturned (*A page that
says how it reads*). One such sentence under a half turn reads whole by such
a tie: its first `2` stands exactly three ems back from the word before it,
which the turned sums put past `backwards`'s threshold and the page's `x` does
not, so under the turn the writer draws its first word space with its word;
unturned, the rules' drawing sets the space alone, which a reader drops,
`كتاب25٪`, and the read-back searches on to a drawing that reads the words
(*A reading held to the line's words*), so the twin reads whole too. Each
turned row reads as its
unturned twin: the punctuation shape under `rotate(12deg)`, whose climbing
baseline the page's axes would cut every half an em up the page, reads whole
as its twin does under a reading held to its words; and the squeezed Hebrew
blocks of `a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`,
turned a quarter and three quarters, conserve, hold their order and find
every word, as unturned, in both builds. A Latin line under a quarter turn
reads whole. No committed document holds turned text.

**Known limits.** Where the page does not state its text, the line that opens
with `25٪` is read alone, `25 ٪يوم`, turned or not: the sign is drawn left of
the digits, `AN` after the paragraph's Arabic (UAX #9's W2), and read alone
the line opens with one European number, `25٪` (W5), where its paragraph
would keep the sign with its number. The writer's book states the line, so it
reads whole there; ROADMAP's Bugs list holds the line on a page that does not
state it, and FT-37 resolves a paragraph across its lines.

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
held to what it reads, the three sentences and their twin held to
`cons logical whole found apart`; and the turned rows of
`a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`, held
to what they read unturned.

### A reading held to the line's words

**The rule.** `Fonts::search` holds a reading to the line's words. A drawing
tried that reads them is taken; one that reads the letters in order and not
the words, a word cut where a reader parts the spacing or two run together
(`Read::Letters`), is kept while the search goes on, within the same 128
readings, and drawn only where none reads the words; and a line whose rules'
drawing reads it so is searched on the same way, the rules' drawing kept where
nothing reads the words (`Search::Letters`). No drawing is taken that sets
more glyphs over others than the rules'. Narrower holds — each drawing tried
held to the words alone, or searched past only where the rules' drawing runs
two words together — read better only cases this reads better, and fewer;
this reads none worse (ruling 14).

**What it reads.** In both builds it reads whole the punctuation shape at
`word-spacing: 0.6em`, flat and under `rotate(12deg)`, and at
`line-height: 0.6`; an Arabic line with `;` glued to a word, letter-spaced and
justified; the Persian digits `٢٠١٠. ۴۵` in a letter-spaced, word-spaced and
justified right-to-left Hebrew paragraph; the lines whose rules' drawing a
reader reads with a number run into the word before it, `كتاب25٪`;
`25٪ <b>W</b>` with no `dir` and `﴾٢٥﴿ <b>W</b>` at `0.75em` and `1em`; a
styled `?!` glued to a Hebrew word; styled `.`, `...` and `!!` glued to a
word; and the first of two right-to-left table cells under `scaleX(2)` on one
line. With `bundled-fonts` it reads whole, too, the dir-less Hebrew paragraph
of inline blocks squeezed by `scaleX(0.5)` at `0.6em`, flat and turned a
quarter and three quarters. It reads no case worse in either build and sets
no glyph over another.

**What it costs.** A line no drawing reads with its words spends the whole
budget: a letter-spaced line, which a reader parts at its spacing whatever is
drawn, is read back 128 times, and 256 under a transform, where it is read
again in its runs' own space. Ruling 14 records the time it costs.

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

**The rule.** `Fonts::search` reads a line in each of its runs' drawings other
than the rules' (`drawings_of`), then two runs' at a time, and keeps the
rules' drawing after 128 readings (`READINGS_PER_LINE`). A run's drawing is
planned before it is read (`Fonts::plan_run`), and one whose plan writes what
the run's rules' drawing writes, or what an earlier drawing of the run
writes, reads as that one does and is not read again (`Fonts::distinct`): the
pairs are of the drawings left. The budget, the order of the drawings and the
bound on glyphs over others do not depend on it.

**What it reads.** In both builds the lines of a Hebrew and an Arabic
paragraph after a generated `§ ` at `word-spacing: 0.6em` and `1em` read
whole, their right drawing inside the budget; an Arabic line of a styled `?!`
glued to the word after it, letter-spaced and justified, conserves and reads
whole; and an Arabic paragraph of styled `?! ?!` glued to a word at `0.51em`
conserves. With `bundled-fonts`, the dir-less Hebrew line of `1 234،` under
`scaleX(2)` at `0.6em` conserves too; in a default build the dir-less Hebrew
paragraph of inline blocks squeezed by `scaleX(0.5)` at `1em` reads whole.
Lines whose verdicts it leaves read the spaces typed beside a number or a
stop, `ספר 25٪` and `٢٠١٠. ۴۵`, where the rules' drawing reads `ספר25٪` and
`٢٠١٠ .۴۵`. It sets no glyph over another.

**What it costs.** The justified Arabic paragraph of styled stops and
brackets at `line-height: 0.4`, with no `dir` and right to left, loses its
structure tree's order: a reader runs its rows into the next, and the drawing
found right for one line alone is read with the next line on the page,
`(نور)` and `بيت` into one another. Ruling 14 records that price. Where a
line's drawings repeat, it is read back fewer times than the budget allows;
`epub::tests`' `a_line_of_units_the_read_back_has_read_is_not_searched_again`
pins the counts.

**What holds it.** `epub_fallback.rs`'s
`a_line_whose_right_drawing_lies_past_the_budget_is_reached_by_reading_each_drawing_once`:
the `§ ` lines read whole, the paragraph at `line-height: 0.4` pinned at its
price, and the pins of the cases it reads better holding them; and
`epub::tests`' `a_line_of_units_the_read_back_has_read_is_not_searched_again`,
by its counts.

### A page that says how it reads: declared direction and stated text

**Why no rule over the drawn line serves.** `Page::text` reads a line alone,
from its glyphs and where they stand, and UAX #9 draws two typed texts alike:
`<p>בראשית. אור ٦٠.</p>` with no `dir` or with `dir="ltr"` is drawn
`٦٠ רוא .תישארב.`, which a right-to-left paragraph draws for
`.בראשית. אור ٦٠`, and `בראשית (אלהים).` after a `<br/>` is drawn
`)םיהלא( תישארב.`. The comma tie-break takes neither line, since its left end
is a number or a closing bracket and no right-to-left letter, so read off the
line each reads right to left, its stop first (`epub_conservation.rs`'s
`a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop`). A rule that
takes them moves the hole rather than closing it: it reads those lines with
their stop last, but reads worse right-to-left lines that open with stops
before a styled pair, which a reader reads in two after the pair,
`)בראשית(?!`, and, of Unicode's conformance files read as a drawn line is,
every line of a right-to-left paragraph it changes, `— שלום 2026` reading
`שלום 2026 —`; ruling 14 records the figures. Nothing on such a line says
which paragraph drew it, so the reader takes what the page says.

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
   element claims, or two directions, is read off the line; a vertical
   writing mode decides nothing, its element's or an ancestor's, since a
   vertical line is not reordered.
2. *Stated text.* An `/ActualText` on a marked-content property list
   (14.9.4), with an `/MCID` or without, replaces the glyphs it encloses in
   `Page::text`, and so in every view that reads it: `TextPage::search`,
   `Page::structured_text` and the text writers. The outermost span open at
   a glyph is the one that holds it (`TextChar::stated`), since a span
   replaces everything inside it. A line all of whose glyphs one span holds
   reads as its text; a span holding some of a line's glyphs is one unit of
   the line's order, read as U+FFFC OBJECT REPLACEMENT CHARACTER is, and
   stands for its text there; a span whose glyphs stand on several lines
   puts its text on the first of them the page reads. Each character of the
   text stands on a glyph of the span that draws it — the first not yet taken
   whose text the text continues with, each line's glyphs in the order a
   line of them reads in the text's direction, the line it is read on first
   (`text_order::read_in`) — and takes its quad, `/MCID`, stream and font,
   so that search finds the text where it is drawn and the structure tree's
   order holds; a character no glyph draws takes the span's box.
   `/ActualText` on a structure element stays the structured view's: an
   element can span lines and pages, where the line has no place for it.

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
   book with no right-to-left text carries no `/WritingMode` and no stated
   span.
2. The read-back reads each line every run of which is declared alike in
   that direction, as the reader will (`Fonts::read_back`,
   `Fonts::declared_of`), and a line no drawing reads right is drawn by the
   rules and stated (`Fonts::stated`, `PageBuilder::begin_stated`): inside a
   `/Span` whose `/ActualText` is the text of the runs drawn in it, in
   reading order, as typed, one span where two lines' runs interleave; a line
   with no other drawing to try is read once for it. The open sequence is
   closed before the span and reopened inside it, and closed and reopened
   after it, so every run keeps its element and the structure tree its
   order. A stated line reads as typed.

**What it reads.** In the writer's own books, in both builds, each of these
conserves, reads whole, is found and draws no glyph over another: the
dir-less Hebrew paragraph that ends a number and a stop, and the dir-less
line whose left end is a closing bracket; the lines a reader reads from the
right whatever is drawn — a Latin gloss before a bracketed right-to-left
word, table cells with no `dir`, a dir-less Hebrew line ending an ARABIC
QUESTION MARK, ARABIC FULL STOP or SOF PASUQ after `(abc …)`, and pointed
Hebrew with two bracket pairs ending in the ARABIC COMMA; a dir-less Arabic
paragraph whose inline `word-spacing: 1em` span cuts a word, and the
structure tree of books with a `'.` at `word-spacing: 0.5em`; and the
dir-less Hebrew line whose comma a NO-BREAK SPACE inside the number before it
would set apart under `scaleX(2)`. The prices ruling 14 records for a line
read alone — a right-to-left line that opens with a dash, an ellipsis, a
closing quotation mark or a run of neutrals, and one that opens with stops
before a bracket pair — read as typed in their declared direction. In other
PDFs, a tagged PDF that declares its directions reads in them, and one that
states its text reads it.

**What it costs.** A PDF from another producer that carries `/ActualText` on
marked content reads that text in `Page::text` as well as in the structured
view (`tagged_pdf.rs` pins the flat extraction reading it); one whose declared
direction its text contradicts reads in the declared one. The lookup reads
one entry of the parent tree a stream, found by its `/Limits`, and each
element once, for a page with a block that holds right-to-left text. The
writer's output grows by an attribute on each element that holds
right-to-left text and a span around each stated line.

A line its declared direction reads right is drawn by the rules, and a reader
parts a word space stretched past half an em before a left-to-right letter
there as it parts a Latin line's: `abc <b>def</b> (ספר) בית` at
`word-spacing: 12px` reads on two lines, as `abc <b>def</b> ghi jkl` does. No
case of `epub_fallback.rs` or `epub_conservation.rs` reads worse in either
build, and none draws a glyph more over another; ruling 14 records the cases
read better, those whose white space reads otherwise, and the time.

### A run that mixes the stops ends no sentence

**The rule.** Where the comma tie-break reads a line left to right and the
line ends a sentence — read leftward from its right end, any closing
quotation marks, then one stop or more — the bracket tie-break passes over a
pair with a bracket just left of the stops, whose level a left-to-right
paragraph took from the line above (`OpenedBefore::before_its_punctuation`).
A run's stops are of one of UAX #14's classes, `EX` (`!`, `?`), `IS` (the
full stop, the comma, the colon, the semicolon and the ARABIC COMMA) or `IN`
(the ellipses), and a run that mixes them, `.!` or `!…`, ends no sentence
(`bidi::Stop`): the pair just left of it is not passed over, and the bracket
tie-break weighs it as on any other line.

**What it reads.** In both builds the mixed runs `.!` and `!…` before a styled
pair after a `<br/>`, in Arabic and Hebrew, conserve and read whole:
`<p dir="rtl">كتاب عوامل<br/>.!<b>(</b>جميل<b>)</b> بيت نعرف</p>` reads its
second line `.!(جميل) بيت نعرف`. No case reads worse, no glyph is set over
another, and no line of Unicode's conformance files, read as a drawn line
is, reads otherwise. A run of one class, `.`, `...`, `?!`, `!!` or `…`, ends a
sentence, and its right-to-left line reads as ruling 14's price reads it.

**What it costs.** A line of a paragraph with no `dir` that ends a pair, a
word and the same run read the other way, `(نور) يوم!.`, broken before the
pair, draws the right-to-left line's picture and reads as it does, its run
first; the suites hold no such line.

**What holds it.** `bidi.rs`'s `a_run_that_mixes_the_stops_ends_no_sentence`,
and the mixed runs' rows of `epub_conservation.rs`'s
`a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed`.

### A word an inline block cuts

**The rule.** A word of Greek, Cyrillic or Armenian that an inline block cuts
reads in two, as a Latin word so cut does (ruling 14).

**What holds it.** `epub_fallback.rs`'s
`a_word_of_an_overflow_script_an_inline_block_cuts_reads_as_a_latin_word_does`,
holding that reading in both builds.

## Risks

| Risk | Mitigation |
|---|---|
| An atomic inline read as U+FFFC gives its neighbours other levels, and a line reads otherwise | Every case of both suites is held to its reading in both builds (`epub_support::reading::check`), so a line that reads otherwise fails its pin |
| Moving a box moves what else reads its fragment: a link's rectangle, a clip, a figure's tag | Every reader of `LayoutPage::boxes` and `LayoutPage::replaced` moves with it |
| Which elements are atomic inlines is decided twice, by layout and by the writer, and the two drift | Decided once: layout records each atomic inline on its page (`Page::atomics`), and the writer sets its stand-ins from that record |
| A left-to-right page moves | `visual_lines` touches no line without a right-to-left character; `no_left_to_right_page_moves` and the determinism fingerprints hold that |
| A block's own transform paints over its neighbours, so `apart` never holds for a widened block | The case pins each paragraph at the glyphs over others its transforms paint (`Expect::Drawing`), so a glyph drawn over another for any other reason fails it |
