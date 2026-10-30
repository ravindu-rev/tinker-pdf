# EPUB

An `.epub` opens as a `Document` whose pages are its laid-out spine. The
book is recognised by `META-INF/container.xml`, its package document is
read, each content document is parsed, styled by the engine's own CSS
engine, laid out and fragmented by its own layout engine, and drawn into a
real PDF through [`DocumentBuilder`](creation.md) — so a book gets every
capability a PDF has. **A reflowable book's page count is a function of the
page box the caller passes, not a property of the file**, and the type says
so.

## What it does

**The container** (`ocf`). OCF 3.3 §4.2.6.3 makes `META-INF/container.xml`
the one file every EPUB must hold, and it is the signature: an ODF has
`manifest.xml`, a JAR has `MANIFEST.MF`, a comic with a `META-INF/`
directory entry has no file in it. Paths compare case-sensitively (§4.2.3).
The `mimetype` rule (§4.3.2: first, stored, exactly
`application/epub+zip`) is a `MUST` the engine *warns about and reads
anyway* — refusing a book over a ZIP field that changes nothing about its
contents would lose the book (ruling 2, [rulings.md](../rulings.md)).
`full-path` resolves from the container root (§4.2.6.3.1), everything else
from the referring document (§4.2.5).

**The package** (`package`, `nav`). OPF 2.0 and 3.x; manifest, spine with
`linear` and per-item `rendition:layout`, fallbacks (depth-bounded),
`properties` (`nav`, `cover-image`, `svg`, `mathml`, `scripted`), metadata.
The navigation document or the NCX becomes the PDF outline. Obfuscated fonts
(`obfuscation`) are de-obfuscated by the IDPF and Adobe algorithms.

**The content document** (`xhtml`). Parsed by `tinker-pdf-xml` in its
doctype mode, because pandoc writes `<!DOCTYPE html>` on every document and
calibre writes none; the internal subset — where every entity bomb lives —
is refused by name under both modes. A document whose declaration names an
XHTML 1.x DTD — XHTML 1.1's identifier, XHTML 1.0's three or XHTML Basic's
two — has XHTML 1.0's 253 named character references resolved (`&nbsp;`,
`&mdash;`, `&eacute;`), from the W3C's three entity sets vendored in
`tinker-pdf-xml/data/xhtml-entities`; one with `<!DOCTYPE html>` or none does
not, since no DTD declares them there, and `&nbsp;` in it is refused by name
as XML 1.0 requires.

**The CSS engine** (`tinker-pdf-css`, a leaf crate): a `css-syntax-3`
tokenizer with the spec's normative error recovery; `selectors-4` matching
and specificity, with **every pseudo-class a static document can decide
actually decided** — `:nth-child()` and its three relatives over §6.6.2's
whole `An+B` grammar, with §14.4.1's `of S` on `:nth-child()` and
`:nth-last-child()` (the element matches `S` and is counted among the
siblings that do), the `of-type` family, `:empty`, `:has()` (a relative
selector against `:scope`, bounded by the match budget), `:lang()` by
RFC 4647 extended filtering, `:dir()`, `:link`/`:any-link` and §12's form
states. The ones whose meaning is the *document language's* are answered by
`epub::xhtml`'s element and appear nowhere in the CSS crate, which is what
keeps ruling 8's boundary real: `xml:lang` beats `lang` there, an `<a>` is a
link only with an `href`, and white-space-only character data does not stop a
cell being `:empty`. `css-cascade-5`'s complete sort order (origin, layer,
specificity, order) with the UA sheet (`read::UA_STYLESHEET`, derived from
HTML §15), author sheets and `style=""` attributes; §7.1's five explicit
defaulting keywords on every property and every shorthand — `inherit`,
`initial`, `unset`, and the two that roll the cascade back rather than
replacing a value, `revert` to the previous **origin** and `revert-layer` to
the previous **layer**; `@media` evaluated against a `MediaContext`;
`@supports` (`css-conditional-3` §6) evaluated against **this build** — a
declaration test is true where the property and its value would be accepted
in a style rule, so `@supports (display: flex)` applies its rules and
`@supports (display: grid)` or a value refused by value does not, with `not`,
`and`, `or`, `css-conditional-4`'s `selector()` (true where the selector
parses and nothing in it is refused or warned about) and an unknown test
false; `@import` with cycle and depth bounds; `@font-face` descriptors
collected into a `FaceSet`; `@layer` in all three syntaxes — block, statement
and anonymous — with dotted and nested names resolving to one layer, first
mention fixing a layer's position, the order kept per origin, and
**`!important` reversing it**, so an important declaration in the first layer
beats an important one in the last. **A property parsed with no consumer does
not compile** — `tinker-pdf-css/tests/unimplemented_property_does_not_build.rs`
and `tinker-pdf-layout/tests/uncascaded_field_does_not_build.rs` make a
silently-ignored property a build failure rather than a rendering surprise.

**The layout engine** (`tinker-pdf-layout`, a leaf crate whose input is a
caller-built tree rather than bytes): CSS 2.2's box model and the three
margin-collapsing cases of §8.3.1; block and inline formatting contexts
(§9.4), an inline box split round an in-flow block inside it (§9.2.1.1); all
nine float rules of §9.5, each its own step and fixture; the §17 table model
with §17.2.1's anonymous-box generation — cells written straight into a table
or a row group are one anonymous row of those cells — §17.5.2.2's two-pass
automatic width, §17.5.1's column and column-group backgrounds painted under
the cells that originate in them, and §17.6.2.1's five-rule border conflict
resolution, `rowspan` clamped; `css-flexbox-1`'s line algorithm with
grow/shrink freeze-and-redistribute and `order`, and `inline-flex` as an
atomic inline-level box whose inside is that layout and whose baseline is its
first (§8.5); §13.3 fragmentation rules A–D with the spec's own escape;
`page-break-*`, `orphans`, `widows`, and `css-break-3`'s `break-before`,
`break-after` and `break-inside`, which §3.4 makes the same three properties
under their modern names — `break-before: page` is `page-break-before:
always` and `avoid-page` is `avoid`, since the page is the one fragmentation
context this build breaks across. `column`, `avoid-column`, `region`,
`avoid-region`, `recto` and `verso` are refused by value
(`UnimplementedProperty`): the first four name contexts this build has none
of, and the last two resolve through a page progression direction it does not
read. §3.1's **replaced elements**, sized by §10.3.2 and §10.6.2 with §10.4's
eleven-row constraint table — which is the algorithm `img { max-width: 100% }`
takes, and the one that keeps a narrowed picture from being squashed rather
than scaled. The intrinsic size is three values (a width, a height, a ratio)
and never the bytes: `css-images-3` §4 distinguishes all three and the leaf
crate holds no decoder ([design/epub-layout.md](../design/epub-layout.md)).
An atomic inline-level box — an inline block, an `inline-flex`, a picture set
inline — is not white space to `css-text-3` §4.1.1's collapsing: the space
after one does not follow the one before it and is not at a line's start, so
a word either side of an inline block or a picture keeps its space
(`Collapser::passed_atomic`).
Line breaking is **UAX #14** over vendored Unicode 17.0.0 data, checked
against every case of Unicode's own `LineBreakTest.txt`
(`tinker-pdf-layout/tests/uax14_conformance.rs`). Advances come from the face
(embedded, host-provided, or the standard-14 metrics for an unembedded
family), one per character.

**A `<br>` is a forced line break** — HTML §15.3.4's `br { display-outside:
newline; }`, from the XML reader's tree and HTML's parser's alike.
`epub::read` lays it out as the line feed CSS 2.1's own sample sheet made it
(`br:before { content: "\A"; white-space: pre-line }`, Appendix D), so it
ends its line under every `white-space`, `nowrap` and `pre` included, the
spaces either side of it go as they do round a preserved newline, and nothing
is drawn for it; two in a row leave an empty line, a trailing one leaves
none, and a block that is one `<br>` is one line tall (CSS 2.2 §9.4.2). Being
U+000A, bidi type B, it ends its bidi paragraph as well as its line
(`css-writing-modes-3` §2.4, UAX #9 P1) — HTML's *"this also has bidi
implications"* — which a U+2028 does not. `display: none` takes it away;
no other `display`, nor a `float` or a `position`, moves it out of its line;
a `::first-letter` after one is not on the first line and is not made; and
`::before` and `::after` on a `<br>` generate nothing. The line feed is in
an inline box of the element's own, not loose text, so a container that
sorts its children sees an element: a `<br>` that is a flex container's child
is a flex item of its own (`css-flexbox-1` §4) — `aaa<br/>bbb` in a row is
three items on one line, and the `<br>`'s, blockified, is the block a lone
`<br>` is, one line tall, so a column sets it as a line between — and one
between two table rows is an anonymous row a line tall rather than white
space the table fix-up discards (`css-tables-3` §2.2.1 discards only
anonymous inline boxes). `epub_analytic.rs`'s `a_br_…` and `two_brs_…`
tests, `epub_paint.rs`'s `a_br_is_a_forced_line_break_and_draws_nothing`,
`epub_shaped.rs`'s `a_br_ends_a_bidi_paragraph_and_not_only_a_line`,
`standalone.rs`'s `a_br_breaks_the_line_in_tag_soup_and_in_xhtml` and
`epub_reftest.rs`'s `a_br_is_a_preserved_line_feed` hold it. `<wbr>`, HTML's
`display-outside: break-opportunity`, is not a break opportunity.

**`text-transform`** (`css-text-3` §2.1) runs between white-space collapsing
and line breaking, which is §1.3's order, so a transformed run is measured as
the characters it becomes. The mapping is Unicode §3.13's **full** case
mapping from the same vendored UCD 17.0.0 (`UnicodeData.txt`,
`SpecialCasing.txt`, `DerivedCoreProperties.txt`) — `ß` uppercases to `SS`,
`ﬁ` to `FI`, and Σ lowercases to ς at the end of a word (Final_Sigma, the one
language-independent condition). `capitalize` titlecases the first letter or
number of each white-space-delimited word if it is lowercase, so `ǆ` becomes
`ǅ`, and a word continues across elements: the second half of a word set in a
`<span>` is not capitalised. The transformed characters are what is drawn and
what text extraction returns, as in a browser's own PDF; so in a book that
uses the property the conservation harness counts a transformed letter as one
missing and one extra.

**Painting** (`paint`, `typeface`). An embedded face's run is shaped whole and
written through `DocumentBuilder::glyph_run`, which states **every glyph's own
position** — so `GPOS`'s offsets reach the page and a mark sits at its anchor
rather than at its advance, as 9.4.3's `TJ` adjustments and `Ts`.
`letter-spacing` is folded into those positions and `0 Tc` written, because a
reader applies `Tc` per glyph while layout measures it per character — per
typographic character unit, every character but a nonspacing mark, which is
its letter's (`css-text-3` §10.2) — and a joined word would otherwise be drawn
narrower than the box it was measured into. A run is shaped against its
neighbours' text where they are in the same face, so a word with a styled
letter still joins and a glyph its neighbour positions keeps the offset, and
each visual line's runs are put in UAX #9's order (`paint::visual_lines`)
before anything is drawn, so a right-to-left line of two styled spans reads
right to left, as does a justified or `word-spacing` line, whose words are
laid in that order too ([fonts](fonts.md)). A context is a neighbour that
touches the run on its line — not the next line's first word — and a run
whose own glyphs a context in the other direction would split is shaped
alone. Layout measures each run in the same context
(`tinker_pdf_layout::metrics::Shaper::shape_in`), so a pair kerned across a
span boundary or a joined form wider than the isolated one is the width the
line was broken at, not a gap or an overlap between two runs. A run that
mixes directions is cut at its line's level boundaries first
(`paint::split_at_levels`), each piece taking its share of the run's
measured width, so `a ب<span>ح</span>م b` draws its Arabic word last letter
first and joined across both span boundaries. The levels are the
**paragraph's**: UAX #9's X1 to I2 run once over every line of the bidi
paragraph `flow.rs` set the runs in (`TextRun::paragraph`), across pages,
and only L1 and L2 per line, so where a line wraps does not change the order
inside it — `abc (de` in a right-to-left paragraph draws `(de` on its second
line, as it does unwrapped. A line whose runs are not all of one paragraph
is resolved by itself.

**`hyphens`** (`css-text-3` §5.4) is read at `none` and `manual`, its initial
value: a soft hyphen (U+00AD) stays in a run's text, since the text is the
book's, measures nothing and is not drawn where no line breaks at it; a line
that breaks at one is measured with room for a hyphen and draws one there
(`TextRun::hyphenated`, `paint::hyphenate`); and `none` takes the break
away, and shows no hyphen where `overflow-wrap` breaks a word just after
one. `auto` asks for a hyphenation dictionary this build does not have and
is refused by value.

**`direction` and `unicode-bidi`** (`css-writing-modes-3` §2) decide the
line, not each line's first letter: every run carries its block's direction
as its paragraph's base level, so a left-to-right paragraph that begins with
an Arabic word stays left to right; `start` and `end` alignment, the side
`text-indent` is taken from (`css-text-3` §8.1) and an outside marker's side
follow it, and an `inside` marker — its item's first inline box, and an
isolate by `css-lists-3` §3.1's user-agent rule — is ordered with its line as
one unit at the paragraph's level, where N1 or N2 puts a neutral at a
paragraph's start: first on the left of a left-to-right item and first on the
right of a right-to-left one, drawn in that direction (`paint::visual_lines`),
an atomic inline beside it on the item's first line one unit of the line too
(see *An atomic inline is one unit of its line's order* below). An inline
box's `embed`, `isolate` and `plaintext` are carried on its runs as the
levels §2.4.2 maps them to — one stack shared by every run set under the same
boxes, no deeper than UAX #9's own 125
(`tinker_pdf_layout::limits::MAX_EMBEDDING_DEPTH`) — and written as
formatting characters only into the text the painter resolves, never into the
page's, where an isolate's `LRI`, `RLI`, `FSI` and `PDI`, which X9 keeps
and an embedding's do not have, are ordered by L2 at their own levels
between the runs either side (`TextRun::bidi_gap`), so an isolate is not
reversed together with the text beside it as an embedding is
(`an_isolate_is_kept_apart_from_the_text_beside_it_and_an_embedding_is_not`);
and a block's `plaintext` gives each paragraph its own first strong
direction — a paragraph separator starts one, which is a forced break of bidi
type B (a preserved newline, a `<br>`, CR, NEL or U+2029), and a U+2028 line
separator, a forced break that is not, ends its line and not its paragraph
(§2.4). HTML's `dir` and `<bdi>` are presentational hints (`epub::xhtml`):
`dir="ltr"` and `"rtl"` set `direction` and isolate, and `dir="auto"` and a
`<bdi>` with no `dir` isolate with the direction HTML's auto directionality
gives them — the first strong character of their text, skipping `bdi`,
`script`, `style`, `textarea` and anything with a `dir` of its own, `ltr`
where there is none — which everything inside them inherits; only
`<pre dir="auto">` and `<textarea dir="auto">` are `plaintext`
(HTML §15.3.5). `:dir()` is handed `auto` as written, and matches neither
keyword there. A cut piece is drawn at the level its line resolved it at, so
the space and `!` that end a right-to-left paragraph are drawn `! ` and not
` !`.

**The standard 14.** A run in one of the standard 14 is unshaped and one
glyph per character, and keeps `PageBuilder::glyphs`; a piece of one that
holds a right-to-left character is written in L2's order
(`paint::coded_order`), each letter with the nonspacing marks after it as one
unit (L3), so a Hebrew word no face covers is drawn `ו` left of `ח` and reads
back as written (`epub_fallback.rs`'s
`a_standard_14_hebrew_word_extracts_as_written`), and `חו,` alone in a table
cell, a left-to-right paragraph that draws its comma right of the word, reads
`חו,` by ruling 14's comma tie-break
(`a_standard_14_hebrew_word_alone_on_its_line_is_drawn_right_to_left`). A
piece with nothing right-to-left in it is written in its level's order too
where its **line** holds a right-to-left character (`TextRun::bidi_line_rtl`,
set where the line's levels are resolved), and as typed where it holds none:
ruling 14 reads a line with no right-to-left character in the order it was
drawn, so a line of neutrals alone in a right-to-left paragraph reads as
written, and the `.,` between two right-to-left words, drawn `,.`, reads as
written too
(`a_standard_14_punctuation_run_between_right_to_left_words_reads_as_written`).
The character is one ruling 14 can undo the level with: a letter of class `R`
or `AL`, or an `RLI`, which stays on the line as a unit of its own; not an
`RLE` or `RLO`, which rule X9 removes before a reader sees them, so
`<p>x &#x202B;?!&#x202C; y</p>` is drawn as typed and reads as typed. L4's
mirroring is not applied, since a simple font's code names one character and a
mirrored glyph would extract as the other bracket, so a reader **sees** a
bracket pair at a right-to-left level facing outward:
`<p dir="rtl">חו (וח) חו.</p>` draws its middle word, left to right, as `)`
`ח` `ו` `(`, each bracket's hollow turned away from the word it encloses, and
extracts as written.

**Neutrals.** A piece of neutrals written in its level's order is **one text
object**, each font's stretch a piece of it (`paint::draw_coded`), wherever
its run's spacing may part two text objects (`paint::PARTED_FROM`, below): a
neutral past `WinAnsiEncoding` is the overflow font's and the word space
beside it a standard face's, and as two text objects a stretched word space
would cut the line between them, which `TextDevice` crosses only into a
right-to-left letter
(`a_non_winansi_neutral_after_a_word_space_reads_with_its_number`, at
`0.5em`, `0.6em` and `1em`). Where a reader of the page would not undo a
piece's level order it is written as typed (`paint::neutrals_drawn_as_typed`),
and reads as written:

- one that the line alone, read in the direction a reader reads it in,
  resolves at other levels than its paragraph did — `($1,234.56). وقد` broken
  from the Arabic word before it, whose W2 made the digits Arabic ones and
  the `$` a neutral; one that opens a line ruling 14 reads left to right by
  its comma tie-break, a right-to-left paragraph's line holding nothing left
  to right that opens with punctuation, so `<p dir="rtl"><i>?!</i> שלום</p>`
  reads `שלום ?!`; and a piece that holds a letter too where any of its other
  characters is so resolved, since a face that draws its neutrals and not its
  letters draws them as a slice of their own, as `5% يوم` opening a wrapped
  Arabic line in sans-serif is
  (`a_sign_after_a_number_opening_a_wrapped_arabic_line_reads_as_typed`). The
  direction is the reader's, not the paragraph's: resolved left to right, as
  its paragraph is, a line with no `dir` and Arabic at both ends would lose
  `… عدّة (2010). وقد …`'s full stop from its bracket;
- one a reader reads on its own for its spacing: a reader resumes a line
  across a text object's end only within half an em, and spacing carries its
  pen only into a right-to-left letter, so a piece half an em or more from
  what is drawn beside it on each side, counting the spacing after each glyph
  from it to the nearest one that is not white space — its run's
  `letter-spacing`, and after a word space its `word-spacing`, which
  justification stretches — is read run by run
  (`a_run_of_neutrals_a_reader_cannot_undo_is_drawn_as_typed`,
  `a_run_of_neutrals_its_word_spaces_part_is_drawn_as_typed`);
- and one that ends, on its right, what a reader reads together between two
  such gaps, where that reads left to right — a styled `?!` glued to the word
  after it, letter-spaced and justified
  (`a_styled_pair_glued_to_the_word_after_it_reads_as_typed_letter_spaced`).

And in a word-spaced run a slice of neutrals whose glyphs read alike either
way round, ` (` or ` #`, is written as typed, its word space first
(`paint::opens_with_its_word_space`): a reader reads the same glyphs in the
same order, the space alone on their other side, and the space is stretched
inside the slice's own text object, so a sans-serif book's ` (` at the left
end of a justified Arabic run meets the Arabic word after it
(`a_word_spaced_slice_reading_alike_is_drawn_with_its_space_first`).

**Marks.** A nonspacing mark has no advance in either build — the overflow
font measures one at zero (`paint::standard_width`), in layout and in its
`/Widths` alike, and the Liberation stand-in's have none — and takes no
`letter-spacing` of its own, in layout and in every painter alike: a letter
and the marks after it are one typographic character unit, spaced once
(`css-text-3` §10.2, `Metrics::letter_spaced`;
`a_standard_14_letter_and_its_marks_are_spaced_once`). It is drawn after its
letter, inside the letter's box, its own box — a thousandth of an em to a
reader — ending a millionth of an em past where layout measured the letter to
end (`paint::EXACT_MARK_INSET`): a letter a simple font draws is as wide as
layout measured it, and one the Liberation stand-in draws, whose `/W` is
rounded to a thousandth, is drawn where layout put it, a piece of its own,
when it carries a mark. A slice holding a mark is **one text object**
(`PageBuilder::text_pieces`), the mark and what follows it each moved to with
`Td`, so no line is cut inside it. So an Arabic letter keeps both its marks
(`a_standard_14_arabic_letter_with_two_marks_keeps_both`), a pointed word
under letter-spacing reads on one line
(`a_standard_14_pointed_word_under_letter_spacing_reads_on_one_line`), a
decomposed accent is found under letter-spacing
(`a_standard_14_decomposed_accent_is_found_under_letter_spacing`), and ruling
14 reads every mark with its letter, four to a letter, at every half-pixel
size from 9 to 24.5 and at every spacing the test sweeps from `-1px` to `1em`,
in a line of either direction
(`a_standard_14_mark_reads_with_its_letter_at_every_size_and_spacing`).

**The overflow font is written at no word spacing**: a text object of its own
at `0 Tw`, and a piece of a text object of several as
`PieceText::UnspacedCodes`, which `text_pieces` writes at `0 Tw` between the
pieces that take the run's (`paint::Segment::unspaced`). Its code 32 is a
character of the book's, the first past `WinAnsiEncoding` its face met, and
9.3.3 applies `Tw` to every single-byte code 32, so at the run's word spacing
every glyph after that code in its string would be drawn a word spacing right
of where layout put it
(`a_standard_14_overflow_letter_at_code_32_takes_no_word_spacing`,
`a_styled_neutral_glued_to_a_word_under_word_spacing_keeps_it_searchable`,
`a_standard_14_pointed_arabic_letter_is_not_drawn_over_the_next_under_word_spacing`,
`a_number_in_word_spaced_right_to_left_text_is_not_drawn_over_its_bracket`).
The stretched word gap the cells of a right-to-left table at
`word-spacing: 1em` leave after a cell ending in a Latin word is one
`TextDevice` bridges, since a gap no spacing accounts for ends the word before
it ([content and text](content-and-text.md);
`a_word_spaced_right_to_left_cell_after_a_latin_word_reads_whole`).

**The inside of a spaced slice is one text object** (`paint::draw_coded`). A
slice that holds a character past `WinAnsiEncoding` needs the overflow font
for it and the face's own for its word spaces; a reader parts two text
objects past half an em, and `TextDevice` carries a pen across spacing only
into a right-to-left letter after right-to-left text. So from its first unit
that is not white space to its last, such a slice is one text object, each
change of resource a piece of it, wherever its run's spacing may part two
objects — its `letter-spacing`, or after a word space its `letter-spacing`
and `word-spacing` with what justification adds, past nine tenths of half an
em (`paint::PARTED_FROM`) — at its paragraph's left-to-right level or at a
right-to-left one, and holding no embedding, override or isolate formatting
character (`paint::joins_its_inside`;
`the_inside_of_a_spaced_slice_is_one_text_object`). The white space at its
ends, a left-to-right slice inside right-to-left text and a slice holding
such a character keep a text object at each change, since written whole each
would draw a line up to its neighbour that ruling 14 reads worse
(`a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole`). So
does a right-to-left slice of a left-to-right paragraph on a line that holds
another paragraph's text too — table cells set touching
(`border-spacing: 0; padding: 0`), which a reader reads as one line
(`paint::sharing_a_line`;
`a_right_to_left_cell_touching_a_left_to_right_one_is_left_in_pieces`).

**Each line is read back** (`paint::Fonts::set_readings`). Every rule above
models what `TextDevice` and ruling 14 will do with a line — where the reader
cuts it, where it carries its pen, which pieces it rejoins and which way it
reads each. So the painter draws each line of a page that holds a
right-to-left character into the reader itself — the text objects it would
write, each glyph where the page has it and as wide as its font says, the
stand-in's as its `/W` rounds it, fed to a `TextDevice` and read in logical
order — and holds what it reads to the line's own text, white space aside.
Where they differ it draws each run another way in turn, then two at a time,
up to 128 readings a line (`paint::READINGS_PER_LINE`): a run's neutrals in
the other order, or its spaced slice whole, in a text object at each change
of resource, whole with the white space at its right end drawn at the end of
its stretch, against what is drawn next, or — a word space styled apart, a
run of its own — at the middle of its stretch, so that the gap either side of
it is half the stretch; whole with the white space at its left end drawn at
the end of its stretch, against the glyph after it, so that its box does not
reach what is drawn before the slice (`paint::Cut::Hugged`); and, a
right-to-left run of more than one word, each word a text object written in
reading order, every glyph where the rules draw it (`paint::Cut::Reading`).
The first that reads the line right is drawn. A line under a matrix that turns
its baseline, which nothing tried reads with its letters in order, is searched
again the same way, up to as many readings, for the first drawing that reads
it closer to the order written than the rules' drawing — more of its runs with
their own characters in order, or as many and more pairs of its words — and
sets no more glyphs over others (`paint::Fonts::search_closer`). Any other
line the rules read right, or that no drawing tried reads right, is drawn by
the rules; a line with a run in a face of the book's own is not read back,
since that face is shaped
(`neutrals_a_reader_reads_with_the_word_beside_them_are_drawn_for_that_reading`,
`a_line_a_reader_cuts_where_it_is_spaced_apart_is_drawn_to_read_whole`).

**Each line is read back on the page it is drawn on**
(`paint::OnPage::matrix`, `paint::along_the_line`, `paint::Segment::read_as`,
`paint::Given`, `paint::Words`, `paint::Read::Letters`). The page draws a run
under the `cm` its CSS `transform` writes (`paint::OnPage::open`), and a
reader's thresholds are in the glyphs' size on the page: a transform that
scales one axis more than the other widens or narrows every gap against it.
So each line is read back under the matrix the page draws each of its runs
under, and the two rules that ask a reader's question — whether a reader
parts two text objects of a run (`paint::parted`) and which runs it reads as
one line (`paint::page_lines`) — ask in a reader's ems on the page: an em of a
run's own spacing is `along_the_line` of its matrix, the scale along the
baseline over the square root of the determinant's magnitude, which is how
`TextDevice` sizes a glyph — `√2` under `scaleX(2)`, one under a turn, a
shift or `scale(2)`. And each glyph is read back as the page's font gives it
back: a standard face draws a NO-BREAK SPACE at `WinAnsiEncoding`'s `0xA0`,
which a reader reads by its glyph name as a space, and the overflow font
names no letter of the supplementary planes, so an Adlam word is spaces to a
reader, and a styled `?!` between two of them is drawn as typed, not turned
for letters the page does not have. Where nothing tried reads such a line
right, it is searched again with each glyph read as typed, and what that
finds is drawn only where it reads more of the line's pairs of letters in
order on the page than the rules' drawing: read as its font gives it back,
`1 234` is two numbers to UAX #9, which no drawing puts back together, and
the styled `?!` after it in `W W <b>1 234</b> <i>?!</i> W.` reads as typed by
that second search ([ruling 14](../rulings.md)). A reading is held to the
line's words: the first drawing tried that reads them is drawn; one that
reads the line's letters in order, white space aside, and not its words — a
word cut where a reader parts the spacing, or two run together
(`paint::Read::Letters`) — is drawn only where none reads the words; and a
line whose rules' drawing reads so is searched on, its rules' drawing kept
where nothing reads the words. A letter-spaced line, which a reader parts at
its spacing whatever is drawn, spends the whole budget so, a time ruling 14
takes for the words the search reads elsewhere. Under a transform, a line the
rules' drawing reads on the page with its letters in order and not its words,
which nothing tried there reads with its words, is searched in its runs' own
space too, and what that finds is drawn only where it reads the words right
on the page (`paint::Fonts::reads_whole`). Where nothing tried
reads a line right on the page under a transform — glyphs the transform sets
over others, whose letters no drawing reads in order — the line is searched
again in its runs' own space, and what that finds is drawn only where, read
back on the page, it holds more of the text's pairs of letters in order than
the rules' drawing does there (`paint::Fonts::reads_closer`). And no drawing
that sets more glyphs over others than the rules' is taken
(`paint::Overprint`, which counts a glyph over another by its box, so that
under a skew a slanted glyph's box reaches over its neighbour's where nothing
is drawn over anything). `paint::leading_mark_at`, which places a mark of no
advance against half an em, keeps the run's ems. Each run is planned once a
page for every reading of its line, and the page writes the plan its line was
read in, found by the run's place in the page's list of runs, wherever the run
is held by then, while a text shadow, a copy with no place, is planned anew
(`paint::Fonts::plans`, `paint::place_in`), and a line of units whose reading the page
has searched is not searched again (`text_order::Readings`)
(`a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on`,
`a_line_is_read_back_as_the_pages_fonts_give_its_characters_back`,
`a_styled_mark_between_letters_the_page_gives_back_as_spaces_reads_as_typed`,
`a_styled_mark_after_a_number_a_no_break_space_holds_together_reads_as_typed`,
`what_a_line_read_as_typed_finds_is_drawn_only_where_it_reads_closer_on_the_page`,
`a_styled_mark_after_a_number_reads_as_typed_beside_a_letter_the_page_gives_back_otherwise`,
`each_run_is_written_as_the_read_back_planned_it_though_two_share_a_place`,
`a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space`,
`a_line_a_reader_cuts_on_the_page_under_a_transform_is_searched_in_its_runs_space_too`,
`a_drawing_that_sets_more_glyphs_over_others_than_the_rules_is_not_taken`,
`two_cells_are_read_back_as_a_reader_parts_them_on_the_page`, and in
`epub::tests`,
`a_line_whose_rules_drawing_reads_its_letters_in_order_is_searched_for_its_words`,
`a_line_of_units_the_read_back_has_read_is_not_searched_again` and
`a_run_is_drawn_from_its_plan_by_its_place_in_the_page`).

**What the read-back leaves** (ROADMAP CD-19). Reading each drawing once is
not done. These lines no drawing tried reads right, and they are drawn by the
rules:

- `25٪ <b>W</b>` in an Arabic paragraph at `word-spacing: 1em`, turned by
  `rotate(180deg)` or not, whose line that opens with `25٪` a reader reads
  alone, `25 ٪يوم` (ROADMAP's Bugs, FT-37); and, with `bundled-fonts`,
  `W 1 234، <b>W</b> W.` in Hebrew, the number's halves a NO-BREAK SPACE
  apart, under `scaleX(2)` at `word-spacing: 0.6em`, which reads its comma
  apart;
- a Hebrew paragraph with no `dir` that ends a number and a stop,
  `בראשית. אור ٦٠.` at `word-spacing: 0.5em`, whose stop abuts the
  right-to-left run and is read first, since the comma tie-break asks for a
  right-to-left letter at the line's left end; and so is the stop of a line
  whose left end is a closing bracket, `W <b>W</b> W <i>W</i>, (W).` with no
  `dir`, letter-spaced or under a transform;
- a word space styled apart across a stretch of two ems,
  `αλφα <b>βήτα</b> ספר בית xyz.` at `word-spacing: 2em`, whose stretch no
  placement of a space parts into halves a reader resumes a line across;
- a paragraph of Arabic with no `dir` whose inline `word-spacing: 1em` span
  cuts a word;
- lines the reader reads from the right whatever the writer draws — a Latin
  gloss before a bracketed right-to-left word, table cells (two right-to-left
  cells with no `dir` touching, or a cell `(ספר) בית` alone on its line), an
  ARABIC COMMA or QUESTION MARK after Latin text;
- and a line whose right drawing comes after the read-back's budget: a
  right-to-left paragraph after a generated `§ ` at `word-spacing: 0.6em` or
  `1em`.

A word of Greek, Cyrillic or Armenian that an inline block cuts,
`αλφα<span style="display: inline-block; width: 1em"> </span>βήτα`, reads in
two, as a Latin word so cut does ([ruling 14](../rulings.md)).

A glyph is drawn over another where a negative `letter-spacing` sets it
there, which layout itself does; where a `transform` paints a box past its
edges, as CSS draws it; and, with `bundled-fonts`, where the stand-in draws a
`U+200E`, `U+202A`, `U+202C` or `U+202D` as a glyph of no advance whose
thousandth-of-an-em box lies on the glyph after it.

**An atomic inline is one unit of its line's order.** In a line UAX #9
reorders, an inline block, an `inline-flex` or a picture set inline is
ordered as one U+FFFC OBJECT REPLACEMENT CHARACTER, a neutral
(`css-writing-modes-3` §2.4), set inside the embeddings its inline ancestors
open; its own text is ordered as the paragraph of its own it is, in its own
`direction`. Layout records each atomic inline on its page, its margin box
and the unit its line set for it — where the line set the box, as wide, on
its baseline, in its paragraph, and stamped with its place in reading order
(`tinker_pdf_layout::Page::atomics`, `AtomicFragment`) — and the painter
sets each from that record: an empty one that paints nothing, one its
margins set apart from the run beside it and a line of nothing but atomic
inlines are each one unit of their line's order (`paint::set_objects`). The
box tree says only what is set in each (`paint::Atomics`). Where the
reordering moves one,
`paint::visual_lines` moves everything set in it by the same distance — its
runs, its box fragment with its background, border and outline, its
pictures and its clips — so the origin of its `transform`
(`paint::Effects::locals`) stands where its text is drawn: a block turned,
skewed or scaled turns about its own text, and one squeezed draws inside its
box. A picture is ordered with its words, and a line with no right-to-left
character moves nothing (`epub_paint.rs`'s
`an_inline_blocks_background_border_and_outline_stand_around_its_own_text_in_a_reordered_line`,
`a_turned_or_skewed_inline_block_turns_about_its_own_text_in_a_reordered_line`,
`an_inline_block_in_a_line_with_no_right_to_left_character_moves_nothing`
and `an_inside_marker_is_on_its_items_start_side_beside_an_inline_block`;
`epub_images.rs`'s `an_inline_img_is_ordered_with_its_words_in_a_right_to_left_line`;
`epub_fallback.rs`'s
`an_atomic_inline_is_one_unit_of_its_line_wherever_layout_set_it`;
`tinker-pdf-layout`'s `a_page_records_each_atomic_inline_with_its_margin_box`).
Each is set in one walk of a page's runs, so a page a book crafts with
thousands of them sets every one (`epub::tests`'
`a_page_of_many_atomic_inlines_sets_each_from_its_record`).
What a transform paints past its box is CSS's: an inline block widened by
`scaleX(2)` paints its glyphs over its neighbours', and the paragraph that
holds it is pinned at the count (`epub_fallback.rs`'s
`a_styled_mark_in_a_widened_block_reads_in_order_with_its_block_scaled_about_its_own_text`).
A block's own paragraph is left to right in a paragraph with no `dir`, so a
`?!` it ends on is drawn right of its word; where the block touches its
neighbours a reader reads it with them, from the right, the `?!` first;
where the word space after it is drawn against the word that follows
(`paint::Cut::Hugged`), a block set more than half an em from its neighbours
is read as a line of its own, left to right, the `?!` after its word. A
right-to-left inline block of `width: auto` is as wide as its line, so it is
a line of its own ([design/rtl-read-back.md](../design/rtl-read-back.md)).

**A run that ends on a mark.** A slice that ends on a mark ends its text
object a spacing less a millionth of an em from the next run, so a run that
ends on a mark — over an alpha, a Cyrillic `а`, an eng, a schwa or an `e`, in
either build, or a mark styled apart from its letter — is not cut from what
follows up to half an em of spacing, half an em included, and below half an
em its line reads whole, the four-mark SHIN ending a bold run in a
left-to-right line too
(`a_standard_14_run_ending_on_a_mark_keeps_to_what_follows_to_half_an_em`).
After the same letter alone a reader's pen stands where the letter's drawn
box ends: where layout measured it to, for a simple font's letter, and up to
half a thousandth of an em either side of that for a stand-in's — so a mark
moves where its line is cut by a millionth of an em after a simple font's
letter and, near half an em, by up to that rounding after a stand-in's
(`0.5001em` with `bundled-fonts`: `x <b>ə</b>y z.` reads `x \nəy z.` and
`x <b>ə\u{301}</b>y z.` reads `x \nə\u{301}\ny z.`, the schwa's `/W`
rounding layout's width up). Past half an em of spacing, and past that
millionth and rounding, a run boundary cuts a line, marks or none, and each
piece is read as a line of its own, so a right-to-left paragraph's pieces
come in the order they are drawn: `<p dir="rtl">חו … חו.</p>` with a Latin
run between reads its full stop before its last word at `0.6em`. A run
ending on a mark over a stand-in's letter is cut past half an em and a
millionth, as its letter alone is
(`a_standard_14_run_ending_on_a_mark_is_cut_past_half_an_em_as_its_letter_is`),
and a search across the cut does not find its word. A slice holding a mark is
never cut inside.

**The half-em band**, a known limit. `TextDevice` resumes a line across an
`ET` only where the next glyph starts within half an em of where the last
one's box ended, and at a spacing of half an em the box's end is the reader's
sum (a `Td`, plus the glyph's width) and the next glyph's start is the
painter's (its own `Td`): two sums of one value, which can differ in their
last place. Within right-to-left text `TextDevice` takes the pen where `Tc`
left it, so the next word starts where the pen stands and a right-to-left
line at exactly half an em reads whole, pointed and not
(`a_standard_14_right_to_left_line_at_exactly_half_an_em_reads_whole`). A run
boundary beside a left-to-right run, within a thousandth of an em of half an
em, is cut or kept by that last place: at half an em exactly the boundary
after a mark is half an em less a millionth, whole, where after the letter
alone it is a tie, so `<p dir="rtl">חו x <b>e\u{301}</b>y חו.</p>`
(monospace, `0.5em`) keeps `e\u{301}y` on one line where `<b>e</b>y` is cut
by its last place.

**A pointed word is as wide as the same word unpointed**: a letter and its
marks are spaced once and the overflow font's marks have no advance, so its
paragraph breaks its lines where the unpointed one does, and reads as it
does. `<p dir="rtl">חו cafe\u{301} na\u{308}ive Nguye\u{302}\u{303}n
q\u{307}\u{323} o\u{302}\u{301} חו.</p>` justified at `0.5em` ends on the
line `Nguyen q o חו.`: a left-to-right run, then, drawn more than three ems
left of where it ended, the right-to-left word with its full stop, which a
reader cuts into a line of its own and ruling 14 reads in drawn order,
`.חו Nguyen q o`, pointed and unpointed alike
(`a_narrower_pointed_word_breaks_its_line_where_the_unpointed_word_does`).

And a mark drawn at its letter's end moves where a reader takes the line to
stand: `TextDevice` starts a new line where a glyph is drawn more than three
ems left of the last one drawn, and after a letter with marks the last one
drawn is its last mark, at the letter's end, not the letter at its start.
`The words CANTILLATED, <b>HASHABBAT</b> end.` at `1.7px`, in a default
build, draws its bold run exactly three ems left of the four-mark SHIN's
origin and further than that from its last mark, so the run starts a line of
its own and the paragraph reads `The words \n.end שָּׁ֑לֵם, הַשַּׁבָּת`. Where on
its letter a mark stands is not where `GPOS` would put it, over the middle:
an unshaped run reads none. A mark whose letter another run draws — styled
apart from it, or left to the standard 14 by a face of the book's own that
draws the letter — is drawn just inside where that letter ended in a
left-to-right run
(`a_standard_14_accent_drawn_apart_from_its_letter_keeps_to_it`). In a
right-to-left run, a known limit, it rides on nothing in its own slice and is
drawn where the pen stands, at its own run's right end, which is where its
letter starts; but the content stream wrote its letter's run before the
mark's own, beside that run's other end, and ruling 14 pairs a mark only with
a neighbour in content order, so it is read with the glyph drawn just before
it: `<p dir="rtl">א <b>ח</b>ָו ב.</p>` extracts `א חוָ ב.`
(`a_standard_14_right_to_left_mark_styled_apart_rides_on_nothing`).

Faces are subset to what the book draws; every run that could not be
represented is counted (`UnrepresentedCharacters`, `UncoveredCharacters`),
and a run the writer refused is `UnwritableTextRun`.

**`opacity`** (`css-color-4` §15.1) is an `/ExtGState` whose `/ca` and `/CA`
are the product of every opacity from the root down, set around each
fragment the element's subtree draws — its box, its pictures and its glyphs,
the last inside their marked-content sequences so the two kinds of bracket
nest (`paint::Effects`, registered before the chapter's first page begins).
Wherever nothing in the subtree paints over anything else in it, that is the
group §15.1 composites; where something does, it is counted (see below).
**`border-radius`** (`css-backgrounds-3` §5, the four longhands and the
shorthand with `/`) rounds a box's background and border: each corner one
cubic whose control points sit `4(√2−1)/3` along its tangents, horizontal
percentages of the border box's width and vertical ones of its height, §5.5's
one factor scaling all four corners when two on a side would overlap, the
padding edge's radii the border edge's less the border width (§5.3), and each
side's colour filled between the two shapes inside a clip from the outer
corner to the inner one. A box cut by a page boundary rounds only its real
ends — `box-decoration-break: slice` — though its percentages resolve against
the fragment, and so does §5.5's overlap factor: a short last slice of a box
with large radii has its corners scaled to the slice's height, horizontal
radii included, where `css-break-3` §5.4 would cut the unbroken box's
corners. The whole box's height is unknown on the page that draws its top,
and a corner taller than its slice would need the unbroken shape drawn under
a clip rather than a rounded rectangle of the slice. **`outline`**
(`css-ui-4` §5, its four longhands and the shorthand) is four bands
`outline-offset` out from the border edge, drawn after the text, moving
nothing; it is rectangular around a rounded box, which §5 leaves to the user
agent, `outline-color: invert` is refused by value, and `currentColor` is the
initial colour whether written or omitted.

**`overflow`** (`css-overflow-3` §3.1: `overflow-x`, `overflow-y` and the
shorthand, `overlay` read as §3.1's alias of `auto`) clips a box's content to
its padding box — on §5.3's padding-edge curve where the box has rounded
corners — and a box whose content fits writes no clip at all, so a book's
`pre { overflow: auto }` round code that fits costs its page nothing.
`visible` beside a scrolling axis computes to `auto` and `clip` to `hidden`,
so a table's `overflow-x: auto` clips both ways. `hidden`, `scroll` and `auto`
make the box a scroll container and so a block formatting context of its own
(CSS 2.2 §9.4.1): its first and last children's margins stay inside it, it
grows to contain its floats (§10.6.7), the floats outside it do not reach in,
and one beside a float is cleared below it (§9.5's *should*); `clip` cuts and
does nothing else. A box that clips its block axis is as tall as its `height`
or `max-height` says, and the content past its padding box leaves the column
— kept as text laid out and not painted, as `visibility: hidden` text is, so
conservation loses nothing — and the box after it follows the used height.
The hidden text reads in document order in each case a test holds it to: a
clip inside a clip (`a_clip_inside_a_clip_hides_its_text_in_document_order`);
a hidden tail inside a float or an absolutely positioned box, which is folded
into the box's own items and broken over pages with it
(`a_hidden_tail_is_broken_over_pages_with_the_float_it_is_in`); one in the
column, which is drawn on the page of the line it follows rather than the page
its height falls on, the two differing once a negative margin takes the column
back up (`a_hidden_tail_is_drawn_on_the_page_of_the_line_it_follows`); and one
in a multi-column container, which goes in the column of the line it follows
and not the column its height falls in
(`a_hidden_tail_in_a_column_set_is_drawn_in_the_column_of_its_line`). A
table row, a flex line or a multi-column container cut across pages draws
what a negative margin pulled above its top on the page that draws its top,
and a hidden tail in a cell or a flex item reads after its line wherever the
cut falls (`a_band_cut_over_pages_draws_what_a_negative_margin_pulled_above_it`).
A box's `height` is measured from where its content starts, below a top margin
that collapses into it (§8.3.1 puts that margin outside the border box), so an
`overflow: clip` box, which opens no formatting context, clips at the height it
states and keeps its first line inside the clip
(`a_clip_measures_its_height_from_where_its_content_starts`), and a box that
does not clip is as tall as it says
(`a_stated_height_is_measured_below_the_margin_that_collapses_into_the_box`).
A flex item that `css-flexbox-1` §9.4 stretches to its line is laid out a
second time and keeps the reading-order place its first layout had
(`a_stretched_flex_item_keeps_its_place_in_reading_order`: `a` beside a taller
`bbbb cccc dddd` reads `a` first), and a band cut over pages is drawn once
when the next band is cut on the same page
(`a_band_is_drawn_once_when_the_next_band_is_cut_on_the_same_page`). The
`layout` fuzz target builds no table, flex or multi-column container, so the
tests named here are what hold those three. Who is clipped is the element
tree's question (`paint::Effects`): every fragment is cut by the clipping
elements above it, an element's own text by its own clip, and an absolutely
positioned box only through its containing block (CSS 2.2 §11.1.1). The
root's `overflow` — or `<body>`'s, under an `<html>` that has none — belongs
to the page (§3.3) and leaves the element `visible`; and a scroll container
in a flex line has no content-based minimum (`css-flexbox-1` §4.5).

**`background-image`** (`css-backgrounds-3` §2.2, one layer, a raster `url()`
or a gradient) is drawn with **`background-repeat`**,
**`background-position`** and **`background-size`**, and the `background`
shorthand sets all five longhands this build has — a colour alone takes an
image away, as it does in a browser. The `url()` is relative to the sheet
that wrote it (`css-values-4` §4.5), and the picture is read after layout,
only for the boxes that reached a page, by the reader an `<img>` takes: one
resource per entry however many sheets name it. It is positioned against the
fragment's padding box and clipped to its border box — on its curve where the
box is rounded — over the box's colour and under its border; one image is one
placement, and a repeating one a tiling pattern whose cell is the image
(`epub::paint::tiling`), `space` spreading whole images edge to edge and
`round` rescaling them to fit (§2.3, §2.4). A reference that names nothing,
or bytes that are no picture, is `BackgroundImageNotDrawn`, counted by
element. A **`linear-gradient()`** or **`radial-gradient()`** (`css-images-3`
§3) is an image with no size of its own, so it is the positioning area's
size unless `background-size` says otherwise (§5.3), and is placed and
repeated as a raster one is, its cell an axial or radial shading
(`epub::paint::gradient_paint`): the line through the tile's centre at its
angle and `|w sin A| + |h cos A|` long, a corner's angle the one that puts the
other two corners on the 50% line; the ending shape's radii from its size
keyword, an ellipse a circle in a space squashed by `ry / rx`; and §3.5.3's
stop fix-up, coincident stops a hard edge. The angle's sine and cosine are
`tinker-pdf-math`'s, through the SVG crate's `rotation`, and a corner's the
box's sides over its diagonal, so the shading's coordinates are the same
bytes on every target (ruling 4). A stop before a radial gradient's
centre is folded into the colour there, and §3.2.4's degenerate shapes are
its three cases: a circle of no radius still rings out to stops placed by
length, a shape of no width is a horizontal gradient mirrored about the
centre, and only a shape of no height is its last colour. A gradient whose
book-given numbers make a geometry that is not finite — a stop at `1e308%`,
radii whose ratio overflows — or whose shading or pattern the writer
refuses is not drawn and is counted against `background-image` by element.
**`box-shadow`** (`css-backgrounds-3` §7.1) is drawn hard-edged: an outer
shadow is the border box offset and grown by the spread — its corners grown
by §7.1.1's `r + s(1 + (r/s − 1)³)` where the radius is under the spread, so a
square corner stays square — under the background and clipped to the page
less the border box, so a box with no background does not show its own shadow
through itself; an `inset` one fills the padding box less that box offset and
shrunk, over the background and image and under the border. The first in the
list is on top. **`text-shadow`** (`css-text-decor-3` §4) is the run drawn
again, offset and in the shadow's colour, before any of the page's text and
marked `/Artifact`, so text extraction reads the words once. A translucent
shadow colour is its alpha times the element's composed `opacity`, since an
`/ExtGState`'s `/ca` replaces the one in force. A shadow or an outline whose
geometry is not finite — `1e400px` reads as infinite — draws nothing, as a
transform with no inverse does, rather than writing `inf` or `NaN`, which are
not PDF numbers (7.3.3).

**`transform`** (`css-transforms-1`, the two-dimensional functions of
§13.1, with **`transform-origin`**) is one `cm` per transformed element:
translate to the origin, the list with its leftmost function outermost,
translate back, the CSS matrix carried from a downward axis in pixels onto
the page's upward one in points. The reference box is the element's border
box **on the page being drawn** — a box cut across pages turns each slice
about its own origin, the whole box's height being unknown on the page that
draws its top. Every fragment the element's subtree draws is wrapped in it:
boxes, pictures, text and its shadows, outlines, and the clips of the
clipping elements inside it, written outermost first so a nested transform
composes without a matrix being inverted. A repeating background's pattern
carries the transform in its own `/Matrix`, since 8.7.3.1 maps a pattern
onto the page's default space and no `cm` reaches it. The rotations and
skews go through the SVG crate's sine (ruling 4). A matrix with no inverse —
`scale(0)` — draws nothing. A transformed box is the containing block of its
absolutely positioned descendants (§2), in the layout and in the clips.

Borders, backgrounds, list markers and links are drawn;
**list markers are counters** (`css-lists-3` §4): `counter-reset`,
`counter-increment` and `counter-set` are walked over the element tree in
document order once the cascade is done (`tinker_pdf_css::counter`), with
§4.5's scoping — a reset reaches the following siblings, a sibling's reset of
the same name replaces it and a descendant's nests inside it, and an element
that generates no box changes nothing — and §4.6's implicit `list-item`
increment. HTML's `ol, ul, menu { counter-reset: list-item }` is in the
user-agent sheet, and `<ol start>` and `<li value>` are the presentational
hints HTML §15.3.8 makes them, cascaded at author level with no specificity
ahead of every author rule. A marker is the item's `list-item` counter in its
`list-style-type` — `css-counter-styles-3` §6's `decimal`, `lower-`/`upper-alpha`
(and `-latin`), `lower-`/`upper-roman`, `disc`, `circle`, `square` and `none`,
each falling back to decimal outside its range — and `counter()` and
`counters()` in `content` read the same tree, and so do the four quote
keywords (`css-content-3` §3.3): one quote depth for the whole document, an
open mark the pair `quotes` names at that depth — the last pair past the
list — and a close one level out, with HTML's `q::before { content:
open-quote }` and `q::after` in the user-agent sheet. `list-style-position: inside`
sets the marker as the item's first inline box, on an anonymous line of its
own where the item starts with a block, and `list-style` is the shorthand of
the two (an image in it is refused by value, since `list-style-image` is not
implemented). **`::first-letter`** (`css-pseudo-4` §2.2) is found while the
box tree is built, before any line is broken: the first typographic letter
unit of a block container's first in-flow text — through inline boxes and into
a first child block, with the punctuation either side and the letter's
combining marks — wrapped in a box of the pseudo-element's style and the text's
own anchor, so a floated one is a drop cap and extraction reads the same
characters in the same order. It inherits from the originating block rather
than from an inline box round the letter, a letter an element boundary
separates from its opening quotation mark is not found, and where nested
block containers each have one the letter is in the innermost's box only (its
declarations are the ones CSS 2.1 §5.12.2's fictional tag sequence shows; an
outer one's border or background round it is not drawn);
every internal link and every navigation entry becomes a link annotation or
outline item. **An XHTML `<img>` is a replaced box** at the picture's own
dimensions, drawn inside its content box as an `/XObject` registered before the
first page begins — JPEG and PNG through the same pass-through path `cbz.rs`
takes, so such a plate is neither decoded nor re-encoded, and GIF and WebP
decoded, each classified by magic bytes and never by name or by the manifest's
`media-type`. An `<img>` that does not reach the page is named
(`ImageNotDrawn`, four defects) rather than leaving a hole the surrounding
text closes over.

**Reflowable and fixed-layout.** A reflowable book is paginated into
`OpenOptions::page` (default 432 × 648 pt, 36 pt margin, 12 pt base size);
the same book at two boxes must be stable on every target *and differ*,
which the determinism suite asserts. A fixed-layout book
(`rendition:layout: pre-paginated`) takes each document's `viewport` as its
page; a fixed document without one is warned
(`FixedLayoutWithoutViewport`), content past the viewport is clipped and
said so (`FixedLayoutContentClipped`).

**Visible partiality is the design.** Everything the book asked for that
this build does not implement reaches the `ArchiveReport` by name, counted
by the elements it affected (`ArchiveWarning::UnimplementedProperty`,
`UnimplementedFeature`), so "how much of this book was read" is a number
on the book in front of you, not a claim about a specification. And the
**conservation invariant** holds: every non-whitespace character of every
content document in the spine appears exactly once in the paginated output,
in document order — recomputed per book by `epub_conservation.rs`.

## API

```rust
use tinker_pdf::{Document, OpenOptions};

// The page box decides the page count. Six by nine inches:
let options = OpenOptions::at_page(432.0, 648.0).with_fonts(provider);
let doc = Document::open_with(epub_bytes, &options)?;

let report = doc.archive().unwrap();
let layout = report.layout();               // the numbers that produced the pages
for w in report.warnings() { /* UnimplementedProperty { property, elements } etc. */ }

let outline = doc.outline();                // from the nav document or NCX
let pdf = doc.editor().save(&Default::default());
```

`OpenOptions { page, font_size, fonts }` (`#[non_exhaustive]`; `at_page`,
`with_fonts`), `Document::open_with`, `Document::archive() ->
Option<&ArchiveReport>`. `tinker_pdf::epub` exposes `DEFAULT_PAGE`,
`DEFAULT_FONT_SIZE`, `PAGE_MARGIN`, `MAX_PAGE_SIDE`, `Limits`, `BookLayout`,
`BookCost`, `BookOptionDefect`, `SpineDefect` and the `ocf`, `package`,
`nav`, `xhtml`, `read`, `typeface`, `paint` and `obfuscation` modules.

**Where a content document's references are read from** is a trait:
`read::Resources` resolves and reads a `<link href>`, an `@import`, an
`<img src>` and an `@font-face` `url()` written in a given document,
answering the path and the bytes or `Unavailable::{Missing, Unreadable}`.
`Ocf` implements it as §4.2.5 says; `read::NoResources` answers every
reference missing. So `read::read_document` — and `read::read_dom`, which
takes a tree something else built, and `read::markup`, the XML reader that
never fails — read a document that did not come out of a container by the
same cascade and the same layout. `read::Context::author` carries sheets a
caller supplies ahead of every sheet the document links; a book's is empty. A
**loose content document is a book of one chapter** through the same passes
(`epub::lay_out_one`, crate-internal): a standalone SVG and a loose XHTML file
open that way ([opening](opening.md)), and `DocumentBuilder::from_html` builds
one from markup and a stylesheet ([creation](creation.md)). Markdown and FB2
reach it as translations into XHTML; an FB2's pictures are answered by a
provider over its own `<binary>` elements, and every loose document's
`data:` URLs by one in front of whatever else answers.

## Tagged output

The PDF an EPUB becomes carries a **structure tree**: `/MarkInfo << /Marked
true >>`, a `/StructTreeRoot`, a `/ParentTree`, `/StructParents` on every page
that has content, and a `BDC`/`EMC` pair with an `/MCID` around every run. The
logical order is the **source document's**: each run carries the index of the
element that wrote it, so the tree is the XHTML tree and not a description of
the page.

XHTML names become ISO 32000 Table 333's standard types — `p` to `/P`, `h1` to
`/H1`, `ul`, `ol` and `dl` to `/L`, `li`, `dt` and `dd` to `/LI`,
`table`/`tr`/`th`/`td` to `/Table`/`/TR`/`/TH`/`/TD`, `section`, `article`,
`aside` and the other sectioning elements to `/Sect`, `blockquote` to
`/BlockQuote`, `caption` and `figcaption` to `/Caption`, `code` and `pre` to
`/Code`, `sub` and `sup` to `/Span`,
MathML's `math` to `/Formula`, anything else block-level to `/Div` and
anything else to `/Span`. **The name is kept where the type would lose it**:
an element whose standard type is not its own spelling is written as itself
— `/S /em`, `/S /section` — with a `/RoleMap` entry (14.7.3) saying what it
is, so `<em>` and `<strong>` are two names a reader can tell apart and two
`/Span`s to a reader that knows only the standard set
(`every_element_keeps_its_name_and_says_what_it_is`). A list marker is an
`/Artifact` (§14.8.2.2) and stays out of the tree, which is what keeps text
conservation an equality.

**Verified against the source and not against this engine.** Reading our own
output back through our own reader proves the two halves agree, not that either
is right, so the load-bearing assertion in `epub_structure.rs` compares the
tree's logical order against the book's own XHTML: every character, in source
order, at nine page boxes.

**What the markup says about itself reaches the tree** (`epub/tagging.rs`),
and each is asserted in `epub_structure.rs` against the XHTML read on its own
with the XML leaf's event reader:

- **`<img alt>` is a `/Figure`'s `/Alt`** (14.9.3). An empty `alt` is HTML's
  statement that the picture is decoration, so it is drawn inside
  `/Artifact BMC … EMC` (14.8.2.2) and is in the tree nowhere; an `<img>`
  with no `alt` is a `/Figure` with no `/Alt`, because the book did not say.
  `<figure>` is a `/Div` round the picture and its `/Caption` — as a
  `/Figure` of its own it would be a figure with no description wrapped round
  one that has it. A picture is drawn before the text in painting order, and
  **reads where it was written**: the layout stamps runs with a reading
  position and pictures with none, so the position is recovered from the
  element tree (an element before the picture's, or the picture's ancestor's
  text up to the child holding it, counted in characters that are not white
  space), and a paragraph whose picture falls between two of its runs is split
  there (`PageBuilder::continue_at`) — `<p>a <img/> b</p>` is text, `/Figure`,
  text. `every_img_alt_is_a_figure_alt`.
- **`xml:lang`/`lang` is `/Lang`** (14.9.2), `xml:lang` first as the selector
  engine reads it. The package's first `dc:language` is the catalog's
  `/Lang`; an element states its own only where its markup does, and the
  elements at a chapter's top carry the chapter's `<html>`/`<body>` language
  when it differs from the book's, since those two are not in the tree. A
  declaration not shaped like a language tag (`en_US`) is treated as no
  declaration — the element inherits — and is named,
  `ArchiveWarning::LanguageTagIgnored`. `every_language_declaration_is_a_lang`
  compares, text by text, the language the source gives each element with the
  one 14.9.2's hierarchy gives it in the tree.
- **`<a href>` is a `/Link` holding its annotation** (14.8.4.4.2): the
  anchor's text, an `/OBJR` to each link annotation drawn for it — one per
  rectangle, so a link broken across a line holds two — and each
  annotation's `/StructParent` naming the `/Link` back through the
  `/ParentTree` (14.7.4.4). The annotations are found by the anchor's key
  (`PageBuilder::link_for`). An `<a>` with no `href`, or with one this build
  could not resolve, has no annotation and is a `/Span`: a bare `/Link` would
  claim an association the file does not contain.
  `every_a_href_is_a_link_holding_its_annotation`.
- **A table says what heads what** (14.8.5.7): `<table summary>` is the
  `/Table`'s `/Summary`; a `<th>` or `<td>` with an `id` carries it as `/ID`,
  qualified by its content document (`EPUB/ch1.xhtml#apple`) because an
  identifier is unique in the whole PDF and two chapters may both say
  `id="h1"`; a cell's `headers` is `/Headers`, an id naming no cell the
  document writes left out — a cell with nothing drawn in it, empty or
  `display: none`, is never opened as an element and carries no `/ID`
  (`a_header_cell_that_is_not_written_is_not_named`); a `<th>`'s `scope` is `/Scope` (`col` and `colgroup`
  `/Column`, `row` and `rowgroup` `/Row`, `auto` unstated); a `colspan` or
  `rowspan` above one is `/ColSpan` or `/RowSpan`.
  `every_table_attribute_is_carried_from_the_source` resolves every cell's
  headers both ways — by `id` in the source, through `element_by_id` in the
  tree — and compares the header cells' text.

**The PDF/UA census runs over this output** (`pdfua.rs`,
`this_engines_own_epub_output_is_censused_and_what_remains_is_named`): the
same `Document::validate_pdfua` the corpus census applies to other producers'
files, over a book this path converted. These fire and are named there:
`PdfUaIdentifierMissing`, because the output claims no PDF/UA conformance and
should not, with `MetadataMissing` and `DisplayDocTitleNotSet` beside it;
`FontNotEmbedded`, because a book with no `@font-face` is set in the
unembedded standard 14; and `TabOrderNotStructure`,
`AnnotationDescriptionMissing` and `LinkContentsMissing` — a link annotation,
inside its `Link` element, with no `/Contents` and no `/Alt`, on a page with no
`/Tabs /S`, which is the tagged writer's to close.

**What is not done yet**, each named rather than absent:

| Not done | Why |
| --- | --- |
| A PDF/UA conformance claim | a structure tree is necessary for it and nowhere near sufficient |
| An empty table cell | the tree is built from the runs a page draws, so a `<td>` with nothing in it draws nothing and is not in the tree, and the cells after it in its row read one column early. `Tag::keep_empty` is the writer's answer; the EPUB path has no run to hang it on |

## Refused by name

| What | Typed variant | Why | See |
| --- | --- | --- | --- |
| A document type declaration with an **internal subset** | `tinker_pdf_xml::Error::InternalSubset`, reaching the page as `ArchiveWarning::Markup` | **permanent, not a debt.** The subset is where every entity bomb lives — billion laughs, quadratic blowup, an external entity and a parameter entity are all declarations inside `[ … ]` — and EPUB 3.3 §3.9 forbids one in a content document in as many words. Reading it would need a declaration grammar and an expander this parser is built not to have; it is refused at the `[` with nothing inside read, under an XHTML declaration as under any other (`an_xhtml_declaration_does_not_open_the_internal_subset`) | `crates/tinker-pdf-xml/tests/bombs.rs` |
| A named character reference nothing declared: any name but XML's five in a document whose declaration names no XHTML 1.x DTD, or a name outside XHTML 1.0's 253 in one that does | `tinker_pdf_xml::Error::UnknownEntity`, as `ArchiveWarning::Markup` | XML 1.0's *Entity Declared* constraint; the chapter keeps what came before the reference. **Permanent**: the HTML standard's larger list is not what an XHTML DTD declares, and it has names that expand to two code points | [THIRDPARTY.md](../../THIRDPARTY.md) |
| An external identifier outside EPUB 3.3 Appendix B's three (SVG 1.1, MathML 3.0, NCX) | `tinker_pdf_xml::Warning::ExternalIdentifierNotAllowed` | a warning, not a refusal, and **not a debt**: the three are the specification's closed list, and XHTML 1.1's identifier is outside it because EPUB 3 bans it — so an EPUB 2 book warns, reads, and still gets its named references | — |
| Inside an SVG content document: `<filter>`, `<foreignObject>`, SMIL animation, `<script>` and `<textPath>`/`<tref>`/`<altGlyph>` | `ArchiveWarning::Svg { item, warning }` | the document draws; each of these is a subsystem this build declines, named per document and deduplicated by the crate that met it. `<filter>`, `<foreignObject>` and `<script>` are roadmap rows (CD-10, CD-11, CD-12), SMIL animation is a limit (NG-11), and `<textPath>`, `<tref>` and `<altGlyph>` are in the roadmap's SVG row (CD-08); [design/svg.md](../design/svg.md)'s non-goals point at each. The same warning names what is drawn without its effect or not drawn: `TextBoxUnmeasured`, a `<tspan>`'s own mask or clip, which takes the whole `<text>`'s box and is built before that box is known, drawn without the effect; `ClipChildIgnored`, a clip-path child §14.3.5 does not admit — a `<g>`, or a `<use>` of one; `GeometryOverflow`, a shape, run or picture whose coordinates pass a double's range once its transforms are composed — two `scale(1e300)`s, each legal — not drawn; `MarkerUnresolved` and `MaskUnresolved`, a reference naming no `<marker>` or no `<mask>`; `ImportUnresolved`, a `<style>` element's `@import` the container does not hold, the rules after it still applied; and `AtRuleIgnored`, an at-rule other than `@media`, `@import`, `@font-face` and `@charset`, or one of those invalid or past a bound (an `@import` after a rule, into a layer, past `MAX_CSS_IMPORT_DEPTH`, importing its own ancestor, or past the token budget or the `MAX_CSS_BYTES` every import shares; a face with no source). A pattern whose tile paints with itself is refused like the `<use>` bomb | [design/svg.md](../design/svg.md) |
| An `<image>` inside an SVG whose reference does not resolve, or whose bytes are neither JPEG nor PNG | `ArchiveWarning::SvgImageUnresolved { item, images }` | those two are embedded through the same `ImageData` path `cbz.rs` uses; anything else is counted per page rather than drawn as nothing | [design/svg.md](../design/svg.md) |
| An XHTML `<img>` that did not become a box on the page | `ArchiveWarning::ImageNotDrawn { item, defect, images }` | four defects, because each is a different party's fault: `Unresolved` (no `src`, or one the container has no entry for), `UnsupportedFormat(f)` (BMP, TIFF, JPEG 2000 and AVIF are foreign resources an `<img>` does not place even where the comic path reads them — each named by format; every EPUB 3.3 §3.2 core raster type has a decoder, so none lands here), `Unknown` (bytes matching no magic number — **an SVG lands here**, having none, and is a spine item in this build rather than a replaced box) and `Undecodable` (a JPEG, PNG, GIF or WebP whose bytes would not make an image; a GIF is drawn as its first image, an animated WebP as its first frame). Counted per content document and per defect, so a comic whose pictures are all TIFF is one sentence a host can act on. The ruling 10 companion to `SvgImageUnresolved`, which is an SVG `<image>` and could never say this | [design/epub-layout.md](../design/epub-layout.md) |
| More distinct element names than one `/RoleMap` carries to this engine's reader (`MAX_DICT_ENTRIES`, 4 096) | `ArchiveWarning::ElementNamesUnmapped { item, names }` | each name past the map is written as its standard type — `/Span`, `/Div` — rather than as itself: a mapping past the cap would be written and then dropped on read, and its elements would read as types no standard defines in a document claiming `/Marked true`. The elements are still tagged and still say what they are; the book's own name for them is what is lost, and counted per content document (`element_names_past_what_a_role_map_carries_are_written_as_their_types`) | ISO 32000-1 14.7.3 |
| An `xml:lang`, `lang` or `dc:language` not shaped like a language tag — `en_US`, a stray space | `ArchiveWarning::LanguageTagIgnored { item, tags }` | not written as `/Lang`: a value a reader cannot use says nothing, so the element inherits its ancestor's language as one that declared none would, and the book is told how many declarations went unwritten. Counted per content document, and once for the package document's `dc:language`, which would have been the catalog's | [design/tagged-pdf.md](../design/tagged-pdf.md) |
| A `<link rel="stylesheet">` whose `href` produced no sheet | `ArchiveWarning::StylesheetUnresolved { item, sheets }` | the document is set without rules its author wrote and the page looks finished, which is `ImageNotDrawn`'s hole for the other reference a content document makes. Counted per content document | [opening](opening.md) |
| A refused `<img>` — **not a refusal, a stated answer** | — | HTML §4.8.4.4 makes an element *"expected to be treated as a replaced element"* **only when the image is available**, so an unavailable one is an ordinary empty inline and generates **no box**. §10.3.2's 300 by 150 default would put a blank postcard into a paragraph for a reference that was merely misspelled, and carrying `alt` into it would put characters on the page the spine's markup does not contain — one per refused image, with no source character to answer it. Asserted as a byte-for-byte identity against the same book with an empty `<span>` in the `<img>`'s place | [design/epub-layout.md](../design/epub-layout.md) |
| `text-transform` in Lithuanian, Turkish or Azeri, and its `full-width` and `full-size-kana` values | `ArchiveWarning::UnimplementedProperty { property: "text-transform", .. }` | §2.1 requires `SpecialCasing.txt`'s language-conditional mappings when the element's language is known, and the layout crate that applies the transform is handed computed styles and never a language — so the cascade counts every element in one of the three languages (by `xml:lang`/`lang`, inherited) that has a casing transform, rather than letting a Turkish heading set with an English `I` read as honoured. `full-width` and `full-size-kana` map to *other characters*, not cases, and are refused by value alone or beside a casing keyword | `crates/tinker-pdf-layout/src/case.rs` |
| `font-kerning: normal`, or a feature switched on by `font-feature-settings`, for text set in one of the standard 14; an alternate index above one; a `font-feature-settings` list longer than `tinker_pdf_css::limits::MAX_CSS_FEATURE_SETTINGS` (32) | `ArchiveWarning::UnimplementedProperty { property: "font-kerning" \| "font-feature-settings", .. }` | an embedded face's run is shaped, and both properties reach its shaper (`font-kerning: none` as `kern` off, `font-feature-settings` switching features off out of the plan and on into it, `css-fonts-4` §7.2's precedence putting the low-level property last), measured and drawn alike. The standard 14 are drawn a character at a time from their widths — no `GSUB`, no `GPOS`, and none of their AFM kerning pairs, which this build does not carry — so a request switched **on** for such text cannot be met and its element is counted, at paint time, where the face a character is set in is known; a feature switched off is met there already, and `auto` kerning is the user agent's to decide. An alternate index is a parameter the shaper's alternate substitution does not take (it takes the first), and a list past the cap would be copied into every element that inherits it, so both are refused by value | `crates/tinker-pdf/tests/epub_paint.rs`, `crates/tinker-pdf/tests/epub_shaped.rs` |
| `unicode-bidi: bidi-override` and `isolate-override` — every `<bdo>` — and `direction: rtl` on a table, a flex container or a multi-column container, or on the containing block of an over-constrained block, table, flex container or block-level picture, of a relatively positioned box with both `left` and `right`, or of an absolutely positioned box | `ArchiveWarning::UnimplementedProperty { property: "unicode-bidi" \| "direction", .. }` | an override makes every character inside it strong in one direction, so a Latin word under `rtl` is drawn letter by letter backwards — a glyph order the painter's shaper never produces, since it shapes a slice in that slice's own direction — and it is refused by value rather than drawn as an isolate. `direction` is met where it decides a paragraph; CSS 2.2 §9.10 gives it three more jobs this layout does not do — a table's columns (§17.5), a flex row's main axis (`css-flexbox-1` §2) and a multi-column container's columns (`css-multicol-1` §3) run from the right, and a block-level box over-constrained in a right-to-left containing block — a block, a list item, a table or a flex container with a definite width and neither margin `auto` — gives up its `margin-left` rather than its `margin-right` (§10.3.3), a relatively positioned box there with both `left` and `right` is offset by `right` (§9.4.3), and an absolutely positioned box in one is placed by `right` and from its static position's right edge (§10.3.7) — so each such element is counted rather than read mirror-wise. A width is definite when `width` states it, when §10.4 runs §10.3.3 again with a `max-width` that narrows the box or a `min-width` that widens it as its `width`, and when the box is a block-level `<img>`, whose used width §10.3.4 takes from the picture and never leaves `auto`; whether a `max-width` or `min-width` clamps is the containing block's width to say, which the cascade does not know, so every one that may is counted — a `max-width` of 100% or more over margins that are not negative never clamps and is not — and an `<img>` whose picture did not resolve is counted with the ones that did. A box with an `auto` margin whose width and other margin are wider than its containing block is over-constrained as well, the `auto` taken as zero, and is not counted: counting every `auto` margin beside a width would count every centred block, and `ContentOverflowedPage` names the box where it is itself wider than its containing block. Which margin or inset gives way is the **containing block's** direction to decide, not the box's own: a right-to-left block with a width in a left-to-right body is laid out as §10.3.3 says and is not counted. A flex container without a `width`, or a `min-width` or `max-width` that may clamp one, fills its line, as a table does in this layout, and gives up no margin; an inline box is not offset by `position: relative` at all here, so it is not counted against `direction`. An `inside` list marker in a right-to-left item is drawn at the right, its item's text to its left, an inline block on its line too (`an_inside_marker_is_on_its_items_start_side`, `an_inside_marker_is_ordered_at_its_paragraphs_level`, `an_inside_marker_is_on_its_items_start_side_beside_an_inline_block`). A standard-14 piece holding a right-to-left character is drawn in L2's order (see *The standard 14* above), with two known limits: L4's mirroring is not applied, so a bracket pair at a right-to-left level faces outward and extracts as written; and a mark whose letter another run draws in a right-to-left run is read with the glyph drawn just before it (`a_standard_14_right_to_left_mark_styled_apart_rides_on_nothing`) | `crates/tinker-pdf/tests/epub_paint.rs`, `crates/tinker-pdf/tests/epub_images.rs`, `crates/tinker-pdf/tests/epub_shaped.rs`, `crates/tinker-pdf/tests/epub_fallback.rs` |
| `color-scheme` naming `dark` and not `light` | `ArchiveWarning::UnimplementedProperty { property: "color-scheme", .. }` | a page is printed in the **light** scheme — paper is the light canvas, and print media's `prefers-color-scheme` is `light` — so `normal` and every list that names `light` (pandoc's `:root { color-scheme: light dark }`) is the scheme this build draws, and the property is the no-op it is on paper. A list asking for the dark scheme asks for a canvas and system colours this build does not have, and is refused by value rather than drawn light and called honoured | `crates/tinker-pdf/tests/epub_paint.rs` |
| `opacity` on an element whose subtree holds a box that paints a background or border **and** has content over it | `ArchiveWarning::UnimplementedProperty { property: "opacity", .. }` | §15.1 composites the element as one group and then fades it; this painter fades each fragment, which is the same picture until something inside the element paints over something else inside it — text over its own box's background, each at half alpha, shows the background through the text. The group is a transparency-group form XObject, and the element's glyphs are tagged marked content the structure writer puts in the page stream rather than in a form, so the group is owed to that writer; the elements where the two differ are counted by element, and the rest are exact. An archival profile that forbids transparency refuses the alpha outright, and those elements are drawn opaque and counted the same way | `crates/tinker-pdf/tests/epub_paint.rs` |
| A repeating or conic gradient, a gradient with a translucent stop, an interpolation hint or an interpolation colour space, one of more stops than `tinker_pdf_css::limits::MAX_CSS_GRADIENT_STOPS` (32), any other `<image>` function, a second background layer, `background-attachment` other than `scroll`, and a `<box>` in the `background` shorthand | `ArchiveWarning::UnimplementedProperty { property: "background-image" \| "background" \| …, .. }` | a PDF shading has no alpha, and a gradient with a translucent stop drawn opaque would be a different picture; a hint bends the ramp between two stops, which one straight piece per pair cannot; a gradient past the cap is a stitching function written for every fragment on every page its box crosses. The other functions of `css-images-4` are images this build does not draw, §2's comma-separated layers are one image too many, and `background-origin` and `background-clip` — which the shorthand's two `<box>`es set — are not implemented: each is refused by value rather than given its first part, which would draw a background the author did not write | [ROADMAP.md](../ROADMAP.md) |
| A blurred `box-shadow` or `text-shadow`, and a shadow list longer than `tinker_pdf_css::limits::MAX_CSS_SHADOWS` (32) | `ArchiveWarning::UnimplementedProperty { property: "box-shadow" \| "text-shadow", .. }` | a blur is a soft edge — a Gaussian of the shape — and this painter draws none; a hard shadow in its place is a different picture, so the whole declaration is refused by value and counted by element. A list past the cap is refused the same way: a text shadow is the run drawn again, so the cap is how many times a page may draw its text. A translucent shadow under an archival profile that forbids transparency is drawn opaque and counted the same way | `crates/tinker-pdf/tests/epub_paint.rs` |
| A three-dimensional `transform` function (`matrix3d()`, `translate3d()`, `translateZ()`, `scale3d()`, `scaleZ()`, `rotate3d()`, `rotateX()`, `rotateY()`, `perspective()`), a non-zero `z` in `transform-origin`, a `position: fixed` box under a transformed one, and a link under one | `ArchiveWarning::UnimplementedProperty { property: "transform" \| "transform-origin", .. }` | a page is flat, and the one 3D function dropped from a list would be a different flat picture from the one a 3D renderer projects, so the declaration is refused by value. §2 makes a transformed box the containing block of its fixed descendants too, and this layout places a fixed box against the page and repeats it — so each one is counted. A link's annotation is the run's rectangle before the transform, so its active area does not turn with its text; each such link is counted | `crates/tinker-pdf/tests/epub_paint.rs` |
| A background image on a box cut across pages — **a stated answer** | — | each page's fragment positions the image against its own padding box, for `border-radius`'s reason: the whole box's height is not known on the page that draws its top. So a `no-repeat` image is drawn once on every page the box crosses, at its position in each fragment, where `box-decoration-break: slice` would draw it once | `crates/tinker-pdf/tests/epub_images.rs` |
| `object-fit`, `object-position` | `ArchiveWarning::UnimplementedProperty` | a replaced box's content fills its content box exactly, which is what CSS says happens when the property that would say otherwise is absent. An author who states a `width` and a `height` that disagree with the picture's proportions gets a stretched picture, asserted rather than assumed | [ROADMAP.md](../ROADMAP.md) |
| An SVG content document that produced no picture at all | `SpineDefect::SvgUnreadable(tinker_pdf_svg::Refusal)` | six named causes — not XML, not an `<svg>` root, a `<use>` that reaches its own ancestor, or one of four ceilings — and the refusal travels, so a caller can tell a bomb from a truncated file | [design/svg.md](../design/svg.md) |
| `position: fixed` — **not a refusal, a stated answer** | — | CSS 2.2 §9.6.1: *"in the case of paged media, fixed boxes are repeated on every page, and are fixed with respect to the page box"*. So a fixed box is positioned against the page box and drawn on every page of the document. That is the specification's own paged answer, not a degradation of the screen behaviour, and it is what a stylesheet asking for a running header meant | — |
| `position: sticky` — **also a stated answer** | — | `css-position-3` §3.4: a sticky box is offset by how far its nearest scrollport has scrolled, clamped to its containing block. A paginated document has no scrollport, so that distance is zero on every page and §3.4's own words are that it is then *"the same as `relative`"*. The value of a parameter this medium does not have, rather than a gap | — |
| An absolutely positioned or fixed box written inside a line, with `top` and `bottom` both `auto` or `left` and `right` both `auto` | `tinker_pdf_layout::Warning::PositionedInLine` | the box is out of flow (CSS 2.2 §9.6) and is laid out against its containing block, taken out of the line the way a float is. An inset pair left `auto` places it at its **static position**, where it would have been in flow: on the line it was written in, or below that line for a block-level box. The lines do not exist yet when it is taken out, so its static position is the inline formatting context's top left, a float's own limit, and each such box is named. With an inset stated in each pair it is exact (`epub_reftest.rs`'s `a_positioned_box_inside_a_line_is_taken_out_of_it`) | [ROADMAP.md](../ROADMAP.md) |
| `column-span: all` below a multi-column container's own children | `tinker_pdf_layout::Warning::ColumnSpanAsNone` | `css-multicol-1` §6: a spanning box interrupts the columns and resumes them below itself. **On a child of the container it does**: the children either side are column sets of their own, each balanced, and the spanner is a block across the container between them. A spanner inside one of the children would split that child round itself, which this build does not; it is laid out in the column it fell in, counted per box | [ROADMAP.md](../ROADMAP.md) |
| A single box taller than a page inside a multi-column container | `tinker_pdf_layout::Warning::ColumnTallerThanPage` | the third of the three `Abreast` shapes and the same sentence as the other two: the container is cut across pages, and what is left is one atomic box no cut can halve | [ROADMAP.md](../ROADMAP.md) |
| A `max-height` shorter than the content, on a box whose `overflow-y` is `visible` | `tinker_pdf_layout::Warning::MaxHeightAsAuto` | CSS 2.2 §10.7's clamp, and its two halves land differently here: `min-height` pads the flow out and `max-height` would have to shorten it, which a column whose `y` never goes backwards cannot do once the items are emitted. So the box is its content's height and the declaration is named rather than half honoured, which is this build's shape for a value it honours in one direction only. A box that clips its block axis is exact: the content past its padding box is clipped and leaves the column, kept as text laid out and not painted, so its `max-height` is its height (`a_block_axis_clip_drops_the_content_past_the_used_height`, and the reftest pair `max_height_on_a_clipping_box_is_the_height_it_clamps_to`) | [ROADMAP.md](../ROADMAP.md) |
| `overflow: scroll` and `overflow: auto` — **a stated answer** | — | a page has no scrolling mechanism, so a scroll container is printed at its initial scroll position — its padding box from the top left — which CSS 2.2 §11.1.1 permits for print and a browser's own print does. What is past it is clipped, and along the block axis out of the column, laid out and not painted | `crates/tinker-pdf/tests/epub_paint.rs` |
| A scroll container beside a float — **a stated answer** | — | CSS 2.2 §9.5: its border box must not overlap the float, and *"implementations should clear the said element by placing it below any preceding floats, but may place it adjacent to such floats if there is sufficient space"*. This build takes the *should*, and a browser the *may*: a picture floated beside an `overflow: hidden` box of text sets the text below the picture here and beside it there. Only the floats crossing the box's top edge are looked for, the box's height being unknown when it is placed | `crates/tinker-pdf-layout/src/tests.rs` |
| Ink past a clipping box's padding box that no advance reaches — **a stated answer** | — | whether a box overflowed is decided on its content's extents, `css-overflow-3` §2.2's scrollable overflow, and not on its ink (§2.1): an italic's overhang past the last advance of a box whose advances fit is drawn rather than cut, since the box writes no clip | — |
| Other at-rules (`@page`, `@counter-style`, …) | `tinker_pdf_css::Warning::AtRuleUnsupported(name)` | skipped by the spec's own recovery, named | — |
| `:hover`, `:focus`, `:focus-within`, `:focus-visible`, `:active`, `:target`, `:visited` — **seven, and the whole of what never matches** | `tinker_pdf_css::Warning::PseudoClassUnsupported(name)` | each names a state of a reading *session*: a pointer, a focus ring, a press, a fragment the reader navigated to, a history. A paginated document has none of them, for any element, ever — so never matching is `selectors-4`'s **answer** here and not this build's gap. Still counted, because a rule that had no effect is something the book said (ruling 10), and the count is asserted by number so a shrinking list cannot read as a passing one | — |
| `::first-line` | `tinker_pdf_css::Warning::PseudoElementUnsupported(name)` | parsed, no box generated. It is not *generated* content: it selects the part of an **already laid out** box that landed on its first line, so honouring it means a second layout pass, which is a different feature from `::before`. `::marker`, `::placeholder` and `::selection` are not parsed at all — `::selection` for the reason the pseudo-class row gives, that it names a state of a reading *session* and a paginated document has none | [ROADMAP.md](../ROADMAP.md) |
| `content: url()`, and `counter()`/`counters()` in a counter style this build does not format | `ArchiveWarning::UnimplementedProperty { property: "content", .. }` | `::before` and `::after` generate boxes from strings, `attr()`, `counter()`, `counters()`, the four quote keywords and any concatenation of them. What they do not: `url()` is a replaced element needing a size before the image is fetched, and a counter in a `<counter-style>` outside the nine this build formats (`lower-greek`, `armenian`, `symbols()`) would be a Greek list numbered in the wrong alphabet. Counted, so a book that asks is not quietly given an empty box | [ROADMAP.md](../ROADMAP.md) |
| `quotes: auto`, the initial value, under a quote keyword | `ArchiveWarning::UnimplementedProperty { property: "quotes", .. }` | `css-content-3` §3.2 makes `auto` the marks *"appropriate for the content language"*, which is HTML §15.3.6's per-language table, and this build does not carry it. A quote keyword under `auto` still moves the depth and draws nothing, and each box that asked is counted: English marks in a French book are a plausible wrong page. A `<q>` is `q::before { content: open-quote }` in the user-agent sheet, so every `<q>` in a book that names no `quotes` is counted here. `match-parent` is refused by value | [ROADMAP.md](../ROADMAP.md) |
| `<ol reversed>`, and `counter-reset: reversed(…)` | `ArchiveWarning::UnimplementedProperty { property: "counter-reset", .. }` | a reversed counter starts at the number of items it will count, which is a count of the list this build does not take before numbering it; the list numbers upwards from its `start` and the element is counted rather than the list silently counting the wrong way | [ROADMAP.md](../ROADMAP.md) |
| A single box taller than a page **inside** a table band | `tinker_pdf_layout::Warning::TableRowTallerThanPage` | the band itself is cut across pages (`css-break-3` §3.1's class-3 break), so what is left is one atomic box — a line box, or a nested band — that no cut can halve. It is drawn where it starts and overflows | [ROADMAP.md](../ROADMAP.md) |
| The same inside a flex line | `tinker_pdf_layout::Warning::FlexLineTallerThanPage` | same, and two variants: a host with no table in its book must not be told a table row overflowed | [ROADMAP.md](../ROADMAP.md) |
| A float whose **content-stream** order differs from its reading order | `tinker_pdf_layout::Warning::FloatBrokenAcrossPages`, and the content-order figure pinned in `epub_fetched.rs` | **A statement about two orders, both measured.** §9.5.1 places a float by geometry, so `clear` can push its box a page past the text it was written among — and a glyph is only on the page it is drawn on. So the *content stream* of `pg16328-beowulf.epub` reads characters out of source order, and `epub_fetched.rs` pins that figure because it is true of any extractor that ignores the structure tree, which is what §14.8 says to do when there is none. **In logical order the same book conserves exactly.** ISO 32000 §14.7.2 Table 323 lets a structure element's kids name different pages, so the gloss sits under its own element in source position while its marked content stays on the page its glyphs landed on, written as an `/MCR` with its own `/Pg` | [design/tagged-pdf.md](../design/tagged-pdf.md) |
| `local()` sources | `ArchiveWarning::FontFace(FaceDefect::LocalUnavailable)` | names a face installed on the reading system, and this engine reads no font directories **by policy** — that is an operating-system dependency `wasm32-unknown-unknown` does not have ([fonts](fonts.md)). Permanent for the engine; a host with installed faces answers it through `FontProvider`. WOFF and WOFF2 are unpacked, and only a container that will not unpack is refused, by `FaceDefect::PackedContainer` carrying the decoder's own reason | [fonts](fonts.md) |
| Characters no face covers | `ArchiveWarning::{UnrepresentedCharacters, UncoveredCharacters}` | counted, never silently dropped — conservation still holds for the text. In a `bundled-fonts` build a character outside `WinAnsiEncoding` that the standard face's Liberation stand-in covers is drawn in it as a composite font (`/Identity-H`, so no 224-code ceiling and no notdef), and only what the stand-in lacks reaches either count; in a default build there is no face to key a CID font to and the ceiling stays (`epub_fallback.rs` asserts both) | [fonts](fonts.md) |
| Fonts attached after open | `ArchiveWarning::FontsAttachedAfterPagination` | advances decide line breaks, so faces must arrive in `OpenOptions` | — |
| Page box or font size the caller passed that cannot be used | `ArchiveWarning::UnusableOption(BookOptionDefect::{PageWidth, PageHeight, FontSize})`, and `Margin` from a creation call ([creation](creation.md)), which a book never reports since its margin is never the caller's | the default is laid out instead and the caller is told which number was thrown away — a warning, not a refusal, because it is a claim about the caller, not the file | — |
| Encrypted resources, missing rootfile, unreadable package document, unsupported package version, empty spine, a book that could not be paginated | `ArchiveRefusal::{EncryptedResources, RootfileMissing, UnreadablePackageDocument, UnsupportedPackageVersion, EmptySpine, UnpaginatedBook, UnreadableContainer}` | refused at open, by name | [cbz](cbz.md) |
| Scripting, MathML layout, media overlays | `ArchiveWarning::UnimplementedFeature` | declared in `properties`, reported, content rendered as its fallback text | [ROADMAP](../ROADMAP.md) CD-04, CD-05, CD-12 |

**SVG in the spine.** `sample-svg-in-spine.epub`'s six spine items are six
pages that draw: an Illustrator cover of gradient-filled paths under a
`<style>` element, two Inkscape drawings, and three pages that are one
`<image>` and nothing else. `the_six_svg_spine_items_draw_rather_than_placehold`
renders all six and asserts each is more than one colour, because a build
that read every SVG and wrote a blank page would satisfy every count in this
file.

What that reader is, precisely: SVG 1.1's §8.3 path data, §7's transforms and
viewports, §9's seven basic shapes, §11's painting from all three of §6.4's
sources, §13.2's gradients including `xlink:href` inheritance between paint
servers, a `<style>` element's at-rules — `@media` asked about paper, as a
page is, `@import` fetched from the container against the document's path,
and `@font-face` loaded beside the book's own faces so a run naming the
family is set in it (`an_svg_reaches_its_container_for_its_imports_and_faces`)
— §14.3's clipping, §14.5's group opacity as a transparency group
(`a_groups_opacity_is_composited_once`) and a container's `clip-path` as the
clip of its rendering, §14.4's masks, §11.6's markers (`tests/markers.rs` in
the leaf crate), §5.6's `<use>` with the bomb refused by name, §5.7's
`<image>`, and §10's text — the last through the same `css-fonts-4` §5.3
matcher and the same shaper that set the rest of the book, because SVG text
and XHTML text in one book must not resolve `serif` two different ways, and
painted as its `fill` and `stroke` say: a colour, a gradient or a pattern
each, by 9.3.6's rendering modes — `fill="none"` is mode 3, invisible text a
reader still extracts (`svg_text_is_painted_as_its_fill_and_stroke_say`).
A gradient, a pattern, a mask or a clip in `objectBoundingBox` units on text
is a fraction of its glyph cells, which the leaf has from this reader
(`a_bounding_box_gradient_on_svg_text_spans_the_text_it_paints`): an SVG
is read before the book's faces are loaded, since its own `@font-face` rules
are among them, so one whose text needed a box is read a second time once
they are, with `BookMetrics` measuring each run as its page will place it, and
the report follows that read.

**Resources are registered before a page begins.**
`DocumentBuilder::begin_page` snapshots the document's resource set, so a
pattern, an `/ExtGState` or an image registered while *drawing* is invisible
to the page naming it: the gradient, the transparency or the photograph is
silently gone while every solid stroke still draws. `epub::svg::Registry` is
the ordering made into a type — `draw` takes a `&Registry` and cannot
register anything.

**Corpus caveats**: the committed set holds a fixed-layout book from a real
producer (KCC 11.0.1, `rendition:layout: pre-paginated`) and books carrying a
real producer's font through the `@font-face` path (Liberation Serif,
unmodified, from calibre and from pandoc). **No stylesheet in the corpus
declares a cascade layer** — `epub_css.rs` asserts the layer count is zero
rather than assuming it — so `@layer` is verified against this engine's own
reading of `css-cascade-5` §6.4.2 and against no producer at all. **No book in
the corpus carries a web font.** The WOFF and WOFF2 files in
`crates/tinker-pdf-font/tests/woff/` come from three encoders with no code in
common, but they are packings of a face this repository wrote, not a book a
producer shipped: no producer here emits one, and OFL-1.1's reserved-name
clause bars repacking a vendored face. So the decoders are held against real
encoders' output and the *`@font-face` path* against containers this
repository packs in `epub_fonts.rs`; what nobody here has is a calibre or
pandoc book with a `.woff2` in its ZIP ([ROADMAP.md](../ROADMAP.md) CD-07).
And a book epubcheck never saw has no verdict: `EPUBCHECK.tsv` marks it `-`
rather than zero.

## Verified

- `crates/tinker-pdf-css/src/tests/defaulting.rs` — **§7.1's five keywords,
  arranged around the two pairs that are easy to collapse into one.** `unset`
  against `inherit` and `initial`, asserted on an inherited and a
  non-inherited property in the same fixture because either one alone agrees
  with two of the three keywords; and `unset` asserted to be exactly *not
  declaring the property* over **every longhand**, not a sample, because
  §7.1's definition and `ComputedStyle::inherit_from`'s behaviour are the
  same rule written twice. `revert` against `revert-layer` in one fixture
  with a user-agent rule and two author layers, where the two keywords have
  **different** answers — the only shape that can tell them apart, and the
  reason it is one fixture asserting both rather than two that could each pass
  alone.
- `crates/tinker-pdf-css/tests/unimplemented_property_does_not_build.rs` — the
  compile-time proof, for a *property* and for a *value*. A property with no
  consumer does not build; a defaulting keyword is a value, valid on every
  property, and `Longhand` and its three exhaustive consumers are what stop
  one being added with nothing to write it down, each withheld in turn and
  asserted not to build: the sharpest is a property name the cascade cannot
  write a defaulted value into, which is `inherit` parsing, cascading,
  winning, and doing nothing. `Longhand::ALL` is a list and not a `match`, so
  `rustc` cannot check it — `defaulting.rs` checks it against
  `IMPLEMENTED_NAMES` instead, and the proof does not pretend to.
- **Committed books from real producers** (pandoc 3.10.2 and 3.11, calibre
  9.13.0 and 9.14.0, KCC 11.0.1) over text authored here, in
  `crates/tinker-pdf/tests/epub/`, with epubcheck 5.3.0 verdicts recorded in
  `EPUBCHECK.tsv`. Under ruling 13 those verdicts are a recorded measurement
  and not a check, so when this engine and a book disagree there is no
  arbiter. What the record holds is that every book has a *row*, that a count
  is a number or an explicit `-`, and that the sets of books the tool was
  unhappy with and never saw are the ones recorded. The fixed-layout comic's
  pages are asserted, each, to be more than one colour: a correctly sized,
  correctly clipped, entirely blank page satisfies every other count. Further
  books are fetched, never committed (Project Gutenberg's trademark licence
  and `epub3-samples`' CC-BY-SA are both barred by this repository's own
  no-copyleft gate), and `epub_fetched.rs` needs `TINKER_EPUB_CORPUS` set to
  an **absolute** path — a test binary runs from its crate directory, so a
  relative one resolves under `crates/tinker-pdf/` and every sweep skips while
  passing — and `TINKER_EPUB_CORPUS_REQUIRED=1` makes a skip a failure. It
  prints `epub-corpus: RAN` / `SKIPPED` so the CI job goes red on a skip.
  `INVENTORY.tsv` is recomputed through `tinker-pdf-zip` on every
  `cargo test`; `CONSERVATION.tsv` records each committed book's conserved
  figure, which `epub_conservation.rs` re-measures and holds the file to.
- `tests/epub.rs`, `epub_ocf.rs`, `epub_package.rs`, `epub_reading.rs`,
  `epub_css.rs`, `epub_tables.rs`, `epub_fixed_layout.rs`, `epub_fonts.rs`,
  `epub_images.rs`, `epub_memory.rs`; the layout crate's own suite
  (`src/tests.rs`, floats and tables step by step, and
  `tests/uax14_conformance.rs`, UAX #14 over Unicode's own conformance file);
  the CSS crate's tokenizer, selector and cascade suites.
- `epub_fonts.rs`'s WOFF half: a book whose `@font-face` names a WOFF or a
  WOFF2 sets in **that face**, asserted on the resource the page draws with
  rather than on the absence of a warning — a book that fell back to the
  standard 14 reports nothing either. The WOFF 1.0 side is packed in the test
  itself, every table **stored** rather than deflated, because there is no
  zlib encoder in this tree and §5's own signal for a stored table is
  `compLength == origLength`; the WOFF 2.0 side is the committed
  `synthetic-2.woff2`, Brotli and transformed `glyf` and all. What is embedded
  is asserted to be the sfnt **byte for byte** and not the container, which
  is the one claim every other test here would pass without: 9.9 gives font
  programs `/FontFile2` and `/FontFile3` and neither has a subtype for a web
  container, so a build that passed the WOFF through would still name the face
  on the page. Damaged containers keep the refusal row and are asserted to
  carry the decoder's own reason.
- `epub_validated.rs`: every synthesised book is held to the strict
  validator, its pages are read at the box the caller stated, its content
  streams are decoded operator by operator so a placeholder page and a page
  that reads are told apart, and its embedded faces keep their own `/W`
  advances.
- `epub_analytic.rs` and `epub_reftest.rs`. Analytic layout sets every
  document in `monospace`, where Courier's 600/1000 advance makes the line
  breaker a division, and **computes every expected number in the test**:
  characters to a line, baselines `line-height` apart, margins collapsed to
  the larger, padding and border on the content edge, `text-indent` on the
  first line only, a float shortening the lines beside it and none below, and
  CSS 2.2 §13.3.2's `orphans` and `widows` deciding the break. Reftests need
  no expected number at all: they lay out pairs the specification says are one
  document — a shorthand against its longhands, `1.5em` against `24px`, `50%`
  against `120px`, an implied `<tbody>` against an explicit one — and each pair
  carries a mismatch reference that must fail. **Both sides of every check
  here are written by the people who wrote the engine**, so this engine's CSS
  is verified against its own reading of the specifications and nothing else
  ([ROADMAP](../ROADMAP.md), [verification](../verification.md)).
- The `epub` determinism fingerprint asserts stability at two page boxes
  *and* that the two differ; the synthesised book's bytes are hashed too
  ([determinism](determinism.md)).
- Compile-time enforcement: `unimplemented_property_does_not_build.rs`,
  `uncascaded_field_does_not_build.rs`.
- Fuzz targets `css`, `layout` (a structured generator, no parser in
  front), `xml`, `zip_archive`; every EPUB cap is a `bounds_ledger.rs` row
  measured against a real book.
