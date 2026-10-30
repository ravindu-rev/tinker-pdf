# Content streams and text extraction

A content stream is postfix — operands, then an operator — and everything a
page shows passes through one interpreter that never knows who is listening.
The listener is a `Device` (ruling 7, [rulings](../rulings.md)): text
extraction and rasterisation are both devices behind the same trait, which is
why `Page::text()` needs no rasteriser, no glyph outlines and no font files —
widths come from the font dictionary, and the text device assembles glyphs
into characters, lines and blocks with a selection quad for every character.

## What it does

**Tokenizing (7.8.2).** `tinker-pdf-content`'s tokenizer splits a stream into
numbers, strings, names, array and dictionary brackets, booleans, `null` and
operators. Literal strings take every escape of 7.3.4.2 (octal above 255 wraps
mod 256, line continuations join), hex strings pad an odd digit count
(7.3.4.3), names decode `#xx` (7.3.5), comments run to end of line (7.2.3).
Numbers parse leniently to their longest sensible prefix — real streams
contain `--5`, `.5.3` and bare `-`, and none of them stops the page.

**Interpretation (8.2, 9.4).** Operands accumulate on a stack until an
operator consumes them; an operator with the wrong operand count is skipped
and the stack cleared, so one malformed instruction cannot desynchronise the
rest. The full graphics state is modelled (8.4.1): `q`/`Q`/`cm` (8.4.4),
`gs` with alphas, `/BM` and `/SMask` through the resource seam (8.4.5,
11.3.5, 11.6.5.1); the complete text state — `Tf` `Tc` `Tw` `Tz` `TL` `Ts`
`Tr` (9.3) — positioning `Td` `TD` `Tm` `T*` (9.4.2) and showing `Tj` `TJ`
`'` `"` (9.4.3), with glyph placement built exactly per 9.4.4. Word spacing
applies to the single-byte code 32 and never to a byte 32 inside a two-byte
CID (9.3.3). Vertical writing displaces the pen by 9.7.4.3's signed `w1` and
places the glyph by minus the position vector `v`, with `TJ` adjustments
running along the writing direction. All eight text render modes of 9.3.6
are carried, including `Tr 3` — invisible text is still text, extracted and
not painted, which is what a scanned page's OCR layer is. Every stroke
parameter reaches the device: `w` `J` `j` `M` `d` (8.4.3), with a dash array
of zeros or negatives meaning a solid line rather than an invisible one.
Paths accumulate through `m` `l` `c` `v` `y` `h` `re` (8.5.2), paint through
`S` `s` `f` `f*` `B` `B*` `b` `b*` `n` (8.5.3), and `W`/`W*` clips take
effect at the painting operator that ends the path (8.5.4). `sh` forwards as
a shading event (8.7.4.2). Colour operators (8.6.8) resolve device spaces
immediately and named spaces through the resource seam, including `scn`'s
trailing pattern name and the components a `PaintType 2` pattern paints with
(8.7.3.2).

**Recursion.** `Do` runs a form XObject with its `/Matrix` composed onto the
CTM and its content clipped to `/BBox` (8.10.2); a transparency group is
offered to the device, and only if accepted are the alphas and blend mode
reset inside it (11.6.6) — declining is the default, because a device with
no buffer must not have `ca 0.5` reset underneath it. A soft-mask group is
run at the `gs` that installs it, at the CTM in force there (11.6.5.2). A
Type 3 glyph is a content stream, not an outline, so it is run the way a
form is, with `/FontMatrix` inside the placing transform and the advance
scaled by it (9.6.5). Recursion of all three kinds stops at a shared depth
cap, and hostile streams meet bounds throughout — operand stack, save
stack, path length — rather than allocation.

**Marked content and optional content.** `BMC`/`BDC`/`EMC` scopes reach the
device with their tag, their own visibility and — when hidden — the layer's
name (14.6.2, 8.11.3.2; ruling 10). An `/OC` entry on a form or image
XObject hides the whole XObject through the same scope mechanism (8.11.4.4).
The two devices deliberately act on different tags: the renderer suppresses
hidden `/OC` layers, and the text device drops `/Artifact` content —
14.8.2.2's running heads and rules are drawn and not read, while an
invisible layer is read and not drawn. A stream that ends with scopes open
has them closed for it, so a form cannot leave its caller inside a layer;
`MP`, `DP`, `BX` and `EX` are recognised no-ops (14.6.1, 7.8.2).

**Inline images (8.9.7).** The span from `BI` to `EI` is scanned at the byte
level — `EI` is accepted only where what follows looks like a content stream
again — and handed to the device as dictionary bytes plus data bytes. The
facade expands Table 93's abbreviations *name by name* (`/F` →
`/Filter`, `/AHx` → `/ASCIIHexDecode`, `/Fl` → `/FlateDecode`, `/CCF` →
`/CCITTFaxDecode`, `/DCT` → `/DCTDecode`, and the rest of Tables 93 and 6),
because a text substitution misses `/F/Fl` written without a space — 7.2.2
makes `/` its own delimiter. The rewritten dictionary then runs the same
COS filter chain every stream uses: predictors, LZW with `/EarlyChange`,
DCT (progressive included), CCITT — one image path, not two.

**Image extraction.** `Page::images()` runs the page through the same
interpreter with a fourth device, which follows form scopes the way the
renderer's does, and returns one `PageImage` per image XObject the page's
content draws — once for each space it is drawn in, however often, with every
placement — and one per inline image. Each carries its **samples before any
colour conversion**: rows from the top, each padded to a byte, `components`
values a pixel at `bits_per_component` bits, sixteen-bit values big-endian and
whole. They come from the same functions the renderer's decode calls —
`stream_samples`, `jpeg_samples`, `jpx_samples` and `inline_samples` — so the
two read one set of samples through one set of rules. Beside them: the colour
space as the image states it (`ImageSpace`: the three device spaces, `CalGray`,
`CalRGB` and `Lab` with their parameters, `ICCBased` with its `/N`, its
`/Alternate` and the profile's bytes, `Indexed` with its base, `hival` and
palette, `Separation` and `DeviceN` with their colorants' names and alternate),
`/Decode` as written and not applied, `/ImageMask`, `/Mask` as colour-key
ranges or a stencil image — decided by what a reference reaches, so an indirect
`[0 0]` is a colour key as the renderer reads it — `/SMask` as an image of its
own, the current transformation matrix at each drawing, and the object
reference. Beside the stated space, `substituted_space` is the space the
drawing's scope puts in for it (8.6.5.6): the stated space with each device
space in it the scope remaps replaced by its default, read by the renderer's
rules — an `/Indexed` base and a selected separation's alternate, not an
`ICCBased` space's `/Alternate` — and `None` where nothing was replaced, a
stencil, a mask and a soft mask included; `sample_space()` answers whichever
the samples are in. An XObject one scope draws under a default and another
without one is listed once for each space, and a default its scope refused puts
nothing in and is named in the `warnings` of an image drawn in its family. A
JPEG's samples are the frame's own components (YCbCr and Adobe's inverted CMYK
already undone by the decoder); a JPEG 2000 image's are the codestream's, at
its precision, with any opacity channel left out; a fax or JBIG2 image's are
one bit a pixel in PDF's polarity whatever the dictionary claims. An inline
image may name a page colour space resource (8.9.7) and that is followed; an
image XObject's `/ColorSpace` may not (8.6.3), and a name other than a device
family is reported as `Unreadable`. An image that will not decode is still
listed, with no samples and `refused` naming why; a stream longer than its
geometry is cut to it and a shorter one is reported short. What a decoder
tolerated for an image — a fax row replicated from the one above, a JBIG2
segment skipped, a JPEG 2000 codestream cut short — is on that image's
`warnings`, named as the render's `RenderWarning::DamagedImage` names it
(ruling 10), so a damaged image is not listed as a clean one. What is not
walked: images inside a tiling pattern's cell, a soft-mask group or an
annotation appearance, which are not drawings of the page's own content.

`tpdf images <file> [--out DIR]` is the same list from the command line, a
wrapper with no logic of its own (ruling 11): a line per image with its
geometry, depth, component count, codec, space, masks and how many times it
is drawn, and with `--out` the samples written exactly as `PageImage` holds
them — `<stem>-pNNNN-NNN.raw`, with `-smask.raw` and `-mask.raw` beside it for
a soft or stencil mask. The space goes out whole, as far as `ImageSpace`
carries it: a `CalGray`, `CalRGB` or `Lab` space's white point, gamma, matrix
and range on the listing line, an `/Indexed` palette as `-palette.raw`, and
every ICC profile wherever in the space it sits — `.icc` for the image's own,
`-base.icc` for the space a palette's entries are in, `-alternate.icc` for a
separation's or another profile's alternate, a step of the suffix for each
level down. The one part not written is a `/Separation` or `/DeviceN` tint
transform, which `ImageSpace` does not carry: it names the colorants and the
alternate, not the function between them. It converts nothing to a picture
format: that would evaluate the colour space, and the listing and the files
beside the samples are what reading the bytes needs instead.

**The text device.** Glyphs become `TextChar`s with a device-space `Quad`
each (9.4.4), grouped into `TextLine`s by baseline continuation and
`TextBlock`s by vertical proximity. An `ET` is *not* a line break — a
producer that emits one text object per styled span writes one paragraph as
many `BT … ET` pairs on one baseline, so a line closed by `ET` is resumed
when the next glyph continues within half an em of where the pen stopped,
while a real gap on the same baseline (two table cells) stays two lines.
An em is the glyph's size on the page, the expansion of its text rendering
matrix (9.4.4), so a transform that scales one axis more than the other
widens or narrows a gap against it: the EPUB writer, which reads each line it
draws back through this device, reads it under the matrix the page draws it
under ([epub](epub.md)).

A horizontal line is measured along its own baseline (`text.rs`'s `Frame`):
along its first glyph's direction, and across it at a quarter turn from it.
Where the pen stands after a glyph is the furthest of its box's corners along
the line, and where the spacing left it the same; whether a glyph after a
text object's end resumes the line within half an em, stands three ems back,
or is carried across a stretch is asked along the line, and whether it
leaves the baseline by half an em across it. So a line under a half turn or
a quarter turn is continued and cut as it would be upright, but for a gap
that lies exactly on a threshold, which a turn's sums can round the other
way. A line whose direction is the page's `x` — every upright line, and every
one under a positive scale — is measured on the page's `x` and `y`
themselves, not by a dot product, so its lines are exactly those the page's
axes give; so is a vertical line, and one whose direction has no length. The
rest of a line cut at a word space's stretch, below, is measured along the
line it is the rest of.

Within right-to-left text, where the pen stopped after a glyph is past the
glyph's box by the spacing 9.4.4 moved it on by — `Tc` after every glyph,
and `Tw` as well after a word space — so a justified or letter-spaced line
is not cut where its spacing stretched past half an em. The pen is carried
there only into a glyph that reads right to left by its own class — a letter
of `Bidi_Class` `R` or `AL`, or the ARABIC COMMA, the one `CS` of the
right-to-left blocks, which a right-to-left word that ends in one is drawn
from; not a digit, an Arabic-Indic one (`AN`) included, though it stands in
the Arabic block, nor the blocks' other punctuation, whose class is `AN`,
`ET` or `ON`, such as the ORNATE PARENTHESES `﴿` `﴾`, which are brackets like
`(` (`tinker-pdf-content`'s `reads_right_to_left`, from the vendored
`UnicodeData.txt`) — starting where the spacing put it give or take a
twentieth of an em, after a right-to-left word, or after neutrals on a line
that holds a right-to-left letter **of its own** and no left-to-right one. It
is carried nowhere else — not into the `(` that opens a Latin run after a
right-to-left word, not further than the spacing reaches, and not after a
neutral the device cut from the Latin text it belongs to — because each such
carry joins pieces that read right alone into a line that falls into ruling
14's limits. A word is its glyphs since the
white space before it, a mark taking its base letter's direction, and a word
with a left-to-right letter anywhere in it is a left-to-right word; a word a
cut on one baseline interrupted goes on into the next piece where the
spacing put that piece, give or take the same twentieth of an em, and there
counts against a carry and never for one: `[ŋ̊ə̃] חו` letter-spaced by half
an em does not carry the `] ` into the Hebrew, and a table cell's last
Hebrew word, its cell and the next with no space between, does not carry the
next cell's `[` into its Hebrew. A gap no spacing accounts for — a table
cell's or a column's — ends the word as white space does, so a right-to-left
cell's own word carries the pen across its stretched space as any other
does, and the cell is one line to the device whether or not a moved glyph
closes the gap. `Tw` is taken after a code 32 the font decodes to white
space — a two-byte one too, as the interpreter moves the pen, though 9.3.3
applies it to a single-byte one alone — so the device is carried to where the
interpreter drew the next glyph; after a code 32 that decodes to anything
else, a letter such as the overflow font's, the interpreter moves the pen on
by `Tw` all the same and the device takes no `Tw` from it.

A cut the device makes after such a word space, for the slack alone and
where the next glyph starts at the pen the stretch left, is mended once the
piece it starts ends: the piece is joined to the line before where one of
the two holds a right-to-left glyph of its own and the leftmost and
rightmost glyphs of the two that read either way both read left to right, so
that ruling 14 reads the joined line left to right by its ends. A line with
no `dir` that sets a bold Greek word before a bare Hebrew gloss and a Latin
tail at `word-spacing: 2em` reads whole so. A glyph a gutter, a cell's
padding, a `Td` or a `Tm` places other than at the stretched pen is never
joined, nor a line whose joined ends would read right to left, and
left-to-right text is collected as before
(`a_cut_at_a_word_spaces_stretch_is_mended_where_the_line_reads_left_to_right`,
`a_cut_at_a_word_spaces_stretch_stays_where_the_line_would_not_read_left_to_right`).

Writing mode and directionality are separate properties: `TextLine::wmode`
is the font's 9.7.4.3 wmode, `TextLine::rtl` is counted from the characters
themselves, because vertical Japanese and right-to-left Arabic are not the
same thing (through `Page::text`, a line holding a right-to-left character
carries the direction ruling 14 read it in instead).

**Logical order (ruling 14).** `TextDevice` collects a line's characters in
the order the content stream showed them, and `Page::text` then puts every
line holding a right-to-left character into the order it is read in: marks
paired with the base glyph they sit on, the line sorted along its baseline,
and read back through UAX #9 as an order whose text the algorithm draws as
the line stands, every answer checked forwards; the paragraph direction is
P2 read off the drawn line by `Bidi_Class`, the two ends first and the
majority where they disagree (`crates/tinker-pdf/src/text_order.rs`, through
`tinker_pdf_shape::bidi::logical_order` and `drawn_direction`).

A line with no left-to-right character drawn with a right-to-left letter
leftmost and punctuation that opens nothing rightmost — `חו,` alone in a
table cell of a left-to-right book, drawn `וח,` — reads as a left-to-right
paragraph, the comma trailing, where a left-to-right paragraph draws it (the
forward check, asked before the direction is chosen): the comma tie-break of
ruling 14, because both paragraphs draw that line and the quoted word is the
commoner. Its known limit: a lone right-to-left line that opens with a mark
of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is not `Ps` or `Pi` — a dash, a
bullet, `*`, `#`, `%`, an ellipsis, or a straight or closing-form quotation
mark used to open — reads with the mark at its end, `— שלום` as `שלום —`,
where its page declares no direction for it (below). An
opening bracket or quotation mark (`Ps`, `Pi`) is not trailing punctuation,
so `(١) بند` and `“שלום` read as typed where `/ToUnicode` names the typed
character; where it names the mirrored glyph (`»` for `«`, `)` for `(`), the
line ends in a closing mark and reads like a dash. A European number after
the mark spares the line too — `(1) פריט`, `— 2026 שלום` and `• 5 תפוחים`
read as typed, since no left-to-right paragraph draws the number next to the
mark — but Arabic-Indic digits (`AN`) do not, `— ١ بند` reading `١ بند —`,
and neither does a number further on, `— שלום 5 חו`.

A line that draws a bracket pair **facing outward** — its closing bracket
left of its opening one, which is how a pair at a right-to-left level stands
where `/ToUnicode` names the typed character — reads in the paragraph
direction whose reading pairs those brackets, where both directions draw the
line, the other one's reading pairs more brackets in all, and more of its
pairs face outward and hug what they enclose (no whitespace just inside
either bracket): the bracket tie-break of ruling 14, so
`(histone deacetylase). وقد أصبحنا نعرف الآن`, with more Latin letters than
Arabic ones, reads as written rather than as the majority's
`… .)histone deacetylase(`. A pair facing inward turns nothing, since a
left-to-right paragraph draws one so and so does a right-to-left one whose
`/ToUnicode` names the mirrored glyph. And brackets that are no pair as
typed — a list marker before a parenthesis,
`Step 1) open the file (שלום) and 2) close it.`, a smiley, a parenthesis
wrapped onto the next line — stand facing outward in left-to-right text and
pair read from the right, but no more of them than as typed, or with the
spaces that stood outside them inside the pair (`see( מלחמה )b` from
`b) מלחמה (see`): those lines keep their direction. Nor is a pair that may
have taken its level from the line before evidence: N0 resolves a pair
enclosing only the other direction's letters by the strong character before
its opening bracket, on the line above for a pair that opens a line, so an
English paragraph's line opening with `(הארץ)` after a line that ended in
Hebrew draws it facing outward at its left end. Such a pair — a bracket at
the end where a reading in the direction chosen starts, nothing of that
direction inside — counts for neither reading where the line's letters say
the direction chosen (more of them, and that reading pairs a bracket facing
outward itself, or at least twice as many and at least eight more), so a
justified English paragraph quoting Hebrew in brackets keeps its direction.

And where the comma tie-break reads a line left to right for the sentence's
end at its right — read leftward, any closing quotation marks (`"`, `'` and
the `Pf` ones, `»`, `’`, `”` …), then one stop or more of one of UAX #14's
classes: a full stop, a comma, a colon, a semicolon or the ARABIC COMMA
(`IS`), `!` or `?` (`EX`), or an ellipsis (`IN`), so `.`, `...`, `?!`, `…`,
`."` and `.»` alike, each a mark UAX #14 puts first on a line only after a
forced break or a space it names (ruling 14) — the line
opens just left of the stops, and a pair with a bracket there, enclosing no
left-to-right letter, counts for neither reading: `جميل بيت كبير (نور) يوم.`
with no `dir`, broken before the bracket, draws `موي )رون(.` and reads as
written. A closing quotation mark alone ends no sentence, nor does a closing
bracket. The rule's known limit: a right-to-left line that opens with such
an ending before a pair — a run of `!` and `?`, an ellipsis, a closing
quotation mark before stops; `…(نور) يوم` or `?!(نور) يوم` after a `<br/>` or
a soft wrap — draws what the line with no `dir` draws and reads as that one
does, its ending last, `نور) يوم(…` (read alone, neither line can see the
line above that gave the pair its level). A run that mixes the classes,
`.!` or `!…`, ends no sentence: a right-to-left line that opens with one
before a pair reads as typed, and a line with no `dir` that ends a pair, a
word and the same run read the other way, `(نور) يوم!.`, broken before the
pair, draws that line and reads as it does, its run first.

A visually drawn Hebrew word and the same word drawn in reading order with
the pen moving left extract the same; digits and a percentage inside a
right-to-left line keep their own order; a line with no right-to-left
character is exactly as collected. A line the content stream's order cut in
pieces is read as one first: `TextDevice` resumes a line after an `ET` only
where its last glyph stopped, so a right-to-left word of several text
objects drawn in reading order inside a left-to-right line —
`a ب<span>ح</span>م b` as an EPUB draws it — reaches `Page::text` as three
lines on one baseline. Consecutive lines of a block that sit on one baseline
to half an em and meet end to end to within the device's own half-em slack
are a chain, and a chain any piece of which holds a right-to-left character
is joined before it is ordered: a piece joins the group before it where
either holds a right-to-left character, and a chain whose groups are more
than one is joined whole only where, read whole, ruling 14's bracket
tie-break turns it. A right-to-left line that opens with
`(histone deacetylase)` reaches the device as the `(`, the Latin words and
the Arabic; the first two hold nothing right to left, so the `(` is a group
of its own — read whole, the line draws its pair facing outward and reads as
written. A chain whose groups read right apart is left in them:
`deacetylase). מִלְחָמָה סֵפֶר בַּיִת.`, the second line of a bracketed run a
line break parted, reads right as its groups, `deacetylase` and
`). מִלְחָמָה …`, where joined whole it would fall into the limit such a line
meets. And a line that overlaps the chain by more than half the shorter
one's extent is not part of it: that is text drawn over itself, a fake bold
or a hand-made shadow, and each copy reads back whole rather than
interleaved with the other. `Page::text_with` with
`TextOptions::content_order` is the opt-out.

**Declared direction.** A line every character of which that draws
something belongs to marked-content sequences whose structure elements
declare one horizontal `/WritingMode` (ISO 32000-2 §14.8.5.4.2) — `/LrTb` or
`/LrBt` left to right, `/RlTb` or `/RlBt` right to left — is read in that
direction, in the order UAX #9 draws as the line stands from there
(`logical_order`), no tie-break asked: the page's word, where P2 read off
the line can only take the commoner of two texts drawn alike. An element's
mode is its own Layout attribute, in its `/A` or else in a `/ClassMap` class
its `/C` names (14.7.6.2), or the nearest ancestor's along `/P`, as the
attribute is inherited; a vertical mode declares no direction a line is read
in, and no ancestor's either. Each sequence's element is found through the
`/StructParents` of the page or the form that drew it and the
`/ParentTree` (14.7.4.4), one entry looked up a stream, each element read
once, and only for a page with a block that holds a right-to-left character
(`structure::declared_directions`). A line with a character drawn outside
any sequence, in one no element claims, or in sequences that declare two
directions or none, is read off the line as before; a line the stream split
is declared whole once joined. The same units read in a declared direction
and off the line are kept apart (`text_order::Readings`).
(`text_logical_order.rs`'s
`a_line_its_element_declares_left_to_right_reads_its_stop_last`,
`a_right_to_left_line_opening_with_a_dash_declared_right_to_left_reads_as_typed`,
`a_writing_mode_inherited_or_stated_by_a_class_holds`,
`a_line_declared_in_part_or_two_ways_or_vertically_reads_as_before`,
`a_line_the_stream_split_reads_in_its_declared_direction` and
`a_sequence_a_form_draws_finds_its_element_through_the_forms_entry`.)

**Stated text.** An `/ActualText` on a marked-content property list (14.9.4),
with an `/MCID` or without, replaces the glyphs it encloses in `Page::text`,
in either order `Page::text_with` gives, and so in search, the structured
view and the text writers (ruling 14). `TextDevice` holds each glyph to the
outermost span open when it was shown (`TextChar::stated`, an index into
`TextPage::stated`), since a span replaces everything inside it, the spans
inside it too. A line all of whose glyphs one span holds reads as the span's
text, its direction the one its line is declared in (above), or else the
text's first strong character's; a span holding
part of a line is one unit of the line's order, read as U+FFFC OBJECT
REPLACEMENT CHARACTER, a neutral, and stands for its text there; a span
holding part of a line ruling 14 does not reorder stands for its text at its
first glyph; and a span whose glyphs stand on several lines puts its text on
the first of them the page reads, its glyphs on the others reading nothing.
Each character of the text stands on a glyph of the span that draws it —
the first not yet taken whose text the text continues with, the glyphs of
the line the text is read on first and then each other line's, each line's
in the order a line of them reads in the text's direction (as drawn, on a
vertical line), so that a letter a right-to-left word repeats stands on its
own glyph — with that glyph's quad, `/MCID`, stream and font, so search
finds the text where it is drawn and a structure tree's order holds; a
character no glyph draws stands on the span's box. An
`/ActualText` on a structure element stays the structured view's: an
element can span lines and pages, where a line has no place for it.
(`text_logical_order.rs`'s `a_line_a_span_states_whole_reads_as_its_text`,
`a_span_inside_a_left_to_right_line_stands_where_its_glyphs_do`,
`a_span_inside_a_right_to_left_line_is_one_unit_of_its_order`,
`a_span_across_two_lines_states_its_text_on_the_first`,
`a_span_across_two_lines_keeps_each_characters_own_glyph`,
`a_right_to_left_word_a_span_states_keeps_each_letter_on_its_own_glyph` and
`a_span_with_an_mcid_states_its_text_in_its_sequence`; `tagged_pdf.rs`'s
`actual_text_on_a_property_list_replaces_its_own_sequence`.) Search boxes a match from
whichever of its ends starts first along the baseline, so a logical-order
match on a right-to-left line is not boxed inside out. Warnings are
deduplicated — a page whose font is unknown says so once, not once per
glyph — so "this page has no text" and "this page has text this build could
not decode" stay distinguishable (ruling 2).

**The recording device.** `tinker-pdf-content`'s `record.rs` carries a third
`Device`, beside the text device here and the rasterising one in
`tinker-pdf-render`: `RecordingDevice` keeps **every** call, in order, as a
typed `Event` with a copy of the graphics state that call saw. It observes
and decides nothing — no accumulated clip, no bounding boxes, no decoded
image samples, and a `q`/`Q` pair that changed nothing is still two events —
so two consumers wanting different scenes build them from one transcript
rather than having to agree first.

Three design decisions are recorded in the module itself:

- **The list is flat, with explicit begin and end events, rather than a
  tree.** `q`/`Q` and `BMC`/`EMC` nest independently and may cross, so a tree
  would have to name one of them the parent and be wrong in whichever
  direction it chose; and the interpreter's own repairs — a stray `EMC`, a
  scope refused past the depth cap, a stream that ended inside a scope —
  have no parent to be given one. The price is that "what was open here" is
  a prefix walk, paid only by the consumers that ask.
- **Each scope's own visibility is what is stored**, with the enclosing
  answer derived on demand, because the reverse cannot be undone: a nested
  `/OC` naming a layer that is on, inside one that is off, is hidden, and a
  recorder that stored only "hidden here" could never say which scope hid it.
- **The state is copied at the call**, which is what the retained page's
  byte-equal replay needs and what a recorder holding one shared handle
  silently loses. `Capture` turns whole categories — and the state copy —
  off for consumers that want only glyphs.

It is a **prerequisite rather than a capability**, which is why it has no
[roadmap](../ROADMAP.md) row of its own. Its consumers are the glyph-usage
walk that font subsetting on rewrite drives, the retained page and the SVG
writer.

`crates/tinker-pdf/src/subset.rs` is one `interpret` into a
`RecordingDevice` and one pass over its events — no second interpreter, no
device of its own. It does not run under `Capture::GLYPHS`, the preset that
turns the bracketing events off: `Glyph::font_id` is the interned **resource
name**, and a resource name is scope-relative, so without
`BeginForm`/`EndForm` there is no way to say which `/F1` a glyph meant and
one font's glyphs would be credited to another — which drops the glyphs the
other font needs. The walk runs under `text` **and** `structure` instead.

**Structured text serialisation does not use the recorder.** It serialises
the `TextPage` the text device already builds — a second walk of the content
stream would be a second extractor, the failure the structured view below is
designed against. Because a resource name is scope-relative, the font cannot
be looked up from `Glyph::font_id` afterwards: the interpreter asks
`FontSource::font_name` at each text-showing operator, in the scope that
operator runs in, and the answer rides on `Glyph::font_name` into
`TextChar::font` — which `a_forms_own_font_is_the_one_its_text_reports` in
`crates/tinker-pdf/tests/text_serialize.rs` holds, with a page and a form
that both call their font `/F0`.

**The retained page replays.** `replay.rs` beside `record.rs` hands a
`Device` every call a transcript kept, in order and with its state, with no
interpreter involved; `Page::display_list` records a page once and
`DisplayList::render` replays it into the renderer at any scale
([rendering](rendering.md)). What the transcript alone cannot carry is the
answer to the three questions a device is asked — `begin_form`,
`begin_group`, `begin_soft_mask` — because the interpreter acts on the
answer and the states it records afterwards depend on it. So a recorder's
answers can be set a call at a time (`RecordingDevice::set_answers`), and
the retained page records through `tinker-pdf-render`'s `DisplayRecorder`,
which asks the renderer's own `Admission` before each question. A replay
that meets a different answer from the one recorded counts it
(`Replayed::disagreed`), skips a bracket the device declined and closes at
once one the recording declined; the module header says which of those an
interpreter would also have done and which lose something.
The SVG writer (`Page::to_svg`) also consumes a replay and is not the
renderer: it answers the three questions itself — every form, a group unless
hidden content holds it, never a soft mask — and the replay's disagreement
handling is what makes declining a mask that the recording accepted skip the
mask's content rather than paint it.

## API

Everything is on the facade (ruling 11): `Page::text()` returns a
`TextPage` of `blocks` → `lines` → `chars`, plus `warnings`, every line in
logical order (ruling 14); `Page::text_with(&TextOptions)` is the same with
`content_order` as the opt-out to the content stream's order. `TextPage`
offers `plain_text()` (one line per line), `lines()` (flattened), and
`search(needle)` — literal, case-insensitive, one `Quad` per match, mapped
back to the glyphs it covers. `Quad` carries four corners and
`bounds()` for the enclosing rectangle. `search_with(needle, options)` and
`plain_text_with(options)` are the opt-in siblings of `search` and
`plain_text()`, below, `TextLine::words()` splits a line into words with a
box each, and `TextWriter` serialises pages as JSON, XML or HTML.
`Page::images()` returns `Vec<PageImage>`; `ImageSpace`, `ImageMask` and
`SampleCodec` are the types it is built from, all `#[non_exhaustive]`.

**Counted work.** `Page::text_counted(&TextOptions)` returns what
`Page::text_with` returns for the same options, to the byte, with a `Stats` of
what extracting it did ([rendering](rendering.md) names the counters): the
fonts the page's scope reads when it is built, the lookup of its font's
dictionary each glyph makes, and each form's scope the interpreter asks for,
over a scope with no font provider, so a build's bundled faces change nothing.
`Page::images_counted()` returns `Page::images()`'s list with the same:
`images_decoded` is one decoder run for each entry listed, an image's masks
part of its decode, and one for each inline image, and a form's scope is asked
for twice, by the interpreter and by the extraction at the form's start.
Nothing is drawn, so no glyph is shown and no outline is read. Each is a
counted sibling rather than a flag because `TextOptions` is written out whole
by its callers and `Page::images` takes nothing to ask with; the design is in
[performance](../design/performance.md).

```rust
let doc = tinker_pdf::Document::open(bytes)?;
let page = &doc.pages()[0];
let text = page.text();
println!("{}", text.plain_text());
for quad in text.search("invoice") {
    let (x0, y0, x1, y1) = quad.bounds(); // PDF user space, y upward
}
for warning in &text.warnings {
    eprintln!("tolerated: {warning:?}"); // TextWarning
}
```

### Search options

`search_with(needle, &SearchOptions)` is `search`'s sibling, and with
`SearchOptions::default()` it returns `search`'s quads, one for one. Three
switches, each off by default and each composable with the others:

- **`case_sensitive`** — match case exactly. Off, both sides are lower-cased
  character by character, as `search` does: it is `str::to_lowercase`, not
  Unicode case folding, so `ß` does not find `SS`.
- **`whole_word`** — a match counts only where it begins and ends on a
  **UAX #29 word boundary** of the text searched, the same segmentation
  `TextLine::words()` reports below. `cat` is not found in `concat`, in `scat`,
  or — since UAX #29 keeps an apostrophe between letters inside a word — in
  `cat's`. A rejected candidate does not hide an overlapping one that is whole
  (`x x` in `xx x x` is found once, starting inside the rejected one).
- **`diacritic_insensitive`** — both sides are **canonically decomposed**
  (`UnicodeData.txt`'s untagged mappings, fully expanded) and every
  nonspacing mark that Unicode also classes `Diacritic` is removed.
  `resume` finds `résumé` whether its accents are precomposed or combining;
  Arabic harakat and Hebrew points are ignored; a Devanagari vowel sign, which
  is `Mn` but a vowel rather than an accent, still has to match. The property
  decides and this build does not second-guess it: the Arabic hamza and madda
  above (U+0653..U+0655) are not `Diacritic`, so `أ` and `آ` still differ
  from `ا`. Compatibility mappings (`ﬁ`, `①`) are not applied, Hangul
  syllables stay composed, and canonical reordering is not performed.
  `tinker_pdf_content::fold_diacritics` is the folding on its own.

Regular expressions are not an option: the tree has no regex engine and
links none, and a hand-rolled linear-time one is [roadmap](../ROADMAP.md) row
FT-20.

```rust
let options = SearchOptions { whole_word: true, diacritic_insensitive: true, ..Default::default() };
for quad in page.text().search_with("resume", &options) { /* … */ }
```

The decomposition and `Diacritic` tables are compiled from the same vendored
UCD tree as the word-break table below.

### Words and word boxes (UAX #29)

`TextLine::words()` returns the line's words as `TextWord`s — the word's
`text`, its `quad`, and the range of `TextLine::chars` it covers — on
**UAX #29's default word boundaries**, not on spaces. `can't`, `3.14` and
`a_b` are one word each, `e.g.` is `e.g` and a full stop, a combining accent
stays with its letter, and a regional-indicator pair is one flag. The spaces
and punctuation between words are segments too, and are not returned: a
segment is a word when it holds a letter or a digit (`Word_Break` `ALetter`,
`Hebrew_Letter`, `Numeric` or `Katakana`, or Unicode `Alphabetic` or
`Numeric`, which is how an ideograph is a word of its own).
`tinker_pdf_content::word_boundaries` is the unfiltered segmentation.

The rules are the default ones, untailored, and UAX #29 names their limit:
scripts written without spaces between words need a dictionary, which this
build does not carry, so an unspaced run of Han breaks after every ideograph
and an unspaced run of Thai is one segment.

A word's `quad` is the union of its characters' quads **in the frame of the
line's own baseline**: the enclosing rectangle for upright text, and a turned
rectangle for a turned line, where an axis-aligned one would cover the
neighbouring words too. A boundary that falls inside one character — a
ligature UAX #29 splits — puts that character in both words, since its quad is
the only box either half has.

The `Word_Break` table is compiled from a third vendored copy of the UCD
(`crates/tinker-pdf-content/data/ucd`, [THIRDPARTY.md](../../THIRDPARTY.md))
by the crate's `build.rs`, the way `tinker-pdf-layout` compiles UAX #14's.

### Hyphen rejoining, opt-in

`plain_text()` reports what the page drew and never changes it; the parity
suite pins that. `plain_text_with(&PlainTextOptions)` is the sibling that may,
and with `PlainTextOptions::default()` it returns `plain_text()` to the byte.
With `rejoin_hyphens: true` it applies two rules, different in kind and
therefore counted apart in the returned `PlainText::hyphens`:

- **A soft hyphen, always.** U+00AD is discretionary — invisible except where
  a line breaks at it — so one ending a line is a word the producer broke,
  joined whatever follows (`soft_joins`), and one inside a line is a break the
  producer recorded and did not use. Every one is removed (`soft_removed`).
- **A hard hyphen at a line end, before a lower-case start.** U+002D or
  U+2010, after a letter, when the next line's first character has the
  Unicode `Lowercase` property (`hard_joins`). This one is an **inference**:
  a compound broken at its own hyphen (`well-` / `known`) comes back as
  `wellknown`, and nothing on the page says which it was — which is why it is
  opt-in and counted separately from the certain case. U+2011 NON-BREAKING
  HYPHEN, the dashes, the small and full-width hyphen-minus, a hyphen after a
  space, and a hyphen before a capital, a digit or an uncased letter are left
  alone.

A join joins consecutive lines in the order `plain_text()` emits them, block
boundaries included, because a word broken at the foot of one column
continues at the head of the next. `StructuredText::plain_text_with` applies
the same function to structure runs, with one consequence of what a run is: a
run holds no line ends, so a word hyphenated inside one paragraph already
reads `hyphen-ation` there and only its soft hyphens can be removed.

```rust
let joined = page.text().plain_text_with(&PlainTextOptions { rejoin_hyphens: true });
println!("{}", joined.text);
eprintln!("{} joins, {} of them inferred", joined.hyphens.joins(), joined.hyphens.hard_joins);
```

### Structured text: JSON, XML and HTML

`TextWriter` writes pages of text as JSON, XML or HTML with their fonts,
sizes and boxes, a page at a time; `TextPage::serialize(format, frame)` is one
page as a whole document, and `Page::text_frame()` supplies the frame — the
page's index, its crop box and its `/Rotate`, which a `TextPage` does not know.
The writers are hand-written (rule 1); `crates/tinker-pdf-content/src/serialize.rs`
carries the model and every escaping decision in its module documentation.

```rust
let mut writer = TextWriter::new(TextFormat::Json);
for page in doc.pages() {
    writer.page(&page.text_frame(), &page.text());
    print!("{}", writer.take()); // a page at a time
}
print!("{}", writer.finish());
```

The model is the same in all three, one level per `TextPage` level plus one:

| Level | Fields |
| --- | --- |
| document | `format` `"tinker-pdf/text"`, `version` (`TEXT_FORMAT_VERSION`, 1) |
| page | `index`, `box` (the crop box), `rotation`, `blocks`, `warnings` (`unknown-font` with `name`, `unmapped-code` with `code`) |
| block | `bbox`, `lines` |
| line | `bbox`, `wmode` (`horizontal`, `vertical`), `rtl`, `size`, `text`, `spans` |
| span | `font`, `size`, `bbox`, `text`, `chars` |
| char | `c`, `quad` (upper-left, upper-right, lower-left, lower-right, eight numbers), `origin` |

A **span** is a run of consecutive characters on one line sharing a font name
and a size, and is where the font name is carried. The name is `/BaseFont` as
the file writes it — subset tag included, since two subsets of one face are
two fonts to the file — and absent (`null` in JSON) where the font states
none. Coordinates are PDF user space, y upward, with `rotation` **not**
applied, at three decimal places; a non-finite number is `null` in JSON and an
absent attribute in XML. JSON has `pages` as an array, XML a `document`
element of `page` elements with the model in attributes, and HTML a page of
absolutely positioned lines with the model in `data-` attributes — the font
name only ever in `data-font`, never in CSS.

Escaping is per format: JSON per RFC 8259 §7, every control character escaped
(and U+2028/U+2029 for JavaScript's sake), and lossless; XML with markup as
entities and tab, line feed and carriage return as references, and the
characters XML 1.0 cannot carry in any form — C0 controls other than those
three, U+FFFE, U+FFFF — written as U+FFFD, which is a loss this format
cannot avoid; HTML with markup as references and every control character but
tab and line feed, and every noncharacter, as U+FFFD. `tpdf text --json`
(`--xml`, `--html`) writes the same through the same writer and adds nothing.

**Reading order is named, and an inferred one is a type of its own.**
`Page::text_in(ReadingOrder)` answers with an `OrderedText` labelled by the
order it is: `Stream` is `Page::text()`, `Stated` is the structure tree's
(`None` for an untagged document), and `Inferred` is
`Page::inferred_order(&InferenceOptions)` — an `InferredOrder` carrying the
permutation back to stream order, never a `TextPage` and never the default
([design/reading-order.md](../design/reading-order.md)). A request for
`Inferred` on a tagged page comes back `Stated`, because a guess is never
preferred to a statement; `InferenceOptions::hide_structure` reads past the
tree only so the inference can be measured against it. A ruled table the
table inference finds is one `Role::Table` block of an inferred order, read
in the table's own order (`InferenceWarning::TableSuspected`), and a page
mostly table, vertical or rotated is declined (`DeclineReason`). `tpdf text
--order stream|stated|inferred` prints the order asked for and says on
standard error which order each page got.

**Tables a producer stated are read as a grid.** `Page::stated_tables()`
walks the tree's `Table`, `TR`, `TH` and `TD` elements and places each cell
by its `/RowSpan` and `/ColSpan` (Table 349), each with the characters the
structure join claims for it on the page; a span that does not add up is
`TableWarning::SpanInconsistent` and a row of the wrong width
`TableWarning::RaggedRows`, never a repair
([design/table-reconstruction.md](../design/table-reconstruction.md)).
`Page::inferred_tables(&TableOptions)` — opt-in, labelled by its evidence,
never a structure element — builds tables from the rules a page draws
(`Page::table_rules()`), `TableEvidence::Ruled`, and where no rule stands
from columns of text whose edges three rows share, `TableEvidence::Aligned`
with `TableWarning::NoRules`, never averaged with the ruled ones; each
character is in the cell holding its centre, spans are read where an
interior rule is missing, `HeaderEvidence` says what the ink shows about the
first row, and the permutation back to stream order is beside them. On a
page whose tree states a table the stated one is the answer, and
`Page::tables(TableSource)` says which it gave. `tpdf text --tables` prints
them, a line for each cell.

### The structured view (14.7, 14.8)

A tagged document says its own reading order, and that order is often not
the geometric one. `Document::structure()` returns the tree — `None` for
the majority of documents, which carry none, and **nothing is inferred for
them**: a tree guessed from geometry would be this engine's opinion about
reading order presented as the file's own statement of it.

```rust
let Some(tree) = doc.structure() else { return };      // untagged
let page = &doc.pages()[0];
let structured = tree.text_for_page(0, &page.text());  // the SAME TextPage
println!("{}", structured.plain_text());               // in structure order
println!("{} claimed, {} orphaned, {} unmarked",
    structured.matched, structured.orphans, structured.unmarked);
```

Each of these properties is a design decision:

- **It is a join, never a second extractor.** `text_for_page` takes the
  `TextPage` `page.text()` already produced. Two extractors would be two
  answers about one page, and the first bug would be a caller finding text
  in one that the other does not have.
- **Nodes are runs, not elements.** An element's content and its child
  elements interleave — `/P [ 3 /Span[4] 5 ]` reads 3, then 4, then 5 —
  so one node per element would have to report 3 and 5 together and put 4
  after them. That is a reordering in the middle of a sentence, and it
  reads as a layout opinion rather than as a bug.
- **Orphans are counted, not appended.** A character carrying an `/MCID`
  no element claims is reported as a number, not silently added to the end
  where it would look like reading order.
- **A content item is identified by its stream and its number, never by
  the number alone.** 14.7.4.2 numbers marked-content sequences *within a
  content stream*, so `/MCID 0` in one form XObject and `/MCID 0` in
  another are two sequences; `/MCR /Stm` says which, `/StmOwn` says which
  object owns that stream, and both are read. An `/MCR` naming no `/Stm`
  means the page's own content stream, which is what almost every one of
  them is. Without the pair, two forms on one page with overlapping ids
  give every element both sequences — a page that says everything twice
  while `matched`, `orphans` and `unmarked` all still look right.

Element types are kept **twice** — `raw_type` as the file wrote it and
`standard_type` after `/RoleMap` — because a consumer that wants to know a
paragraph is a paragraph and one that wants the file's own vocabulary are
both real, and keeping one name loses the other. An unmapped custom type
resolves to itself (ruling 2).

On the write side, `PageBuilder::tagged(tag, |page| …)` draws inside a
marked-content sequence and records the element that claims it, so the tree
is correct by construction: there is no way to name a marked-content id
that was never written, and none to write one no element claims.

`tagged_with(&Tag, |page| …)` is the same with what an element says about
itself: `Tag::new(b"Figure").alt("…")`, `.actual_text`, `.expansion` (`/E`),
`.lang` and `.title` (`/T`) write 14.9's properties as text strings encoded
for the declared version, each only when stated. `open_tag(&Tag)` and
`close_tag()` are the same without a closure, for a caller whose paragraph
starts in one call and ends several calls — or pages — later: an element
still open when its page is pushed is closed there and reopened on the next
page begun, and `finish` writes the two halves as one element whose kids on
the second page are `/MCR`s with their own `/Pg` (14.7.2 Table 323). A
closure's element is the closure's to close — `close_tag` cannot reach it,
and what the closure opened and left open is closed when it returns. A
layer's closure (`optional`) is the same kind of scope, because its `EMC`
closes whichever sequence is innermost: an element opened outside the layer
cannot be closed inside it, and one opened inside it and left open is closed
when it returns — each would otherwise be a stream with a stray `EMC` or
content drawn after the layer still inside it. An
element that draws nothing is **kept when it says something** — an `/Alt`,
a `/Lang`, or `Tag::keep_empty()` for an empty table cell — and dropped
when it says nothing. `DocumentBuilder::set_language` writes the
catalog's `/Lang`, and `is_language_tag` is the shape check (14.9.2's
RFC 3066, BCP 47 in 2.0) a caller holding someone else's text makes first.
`DocumentBuilder::map_role(custom, standard)` writes the structure tree
root's `/RoleMap` (14.7.3), so a producer can keep its own type names and
still say what each one is: the element is written `/S /Chapitre` and read
with `raw_type` `Chapitre` and `standard_type` `Sect`. It refuses what a
reader could not use — remapping a standard type (ISO 14289-1 7.1: *standard
tags shall not be remapped*; the list is `STANDARD_STRUCTURE_TYPES`, shared
with the PDF/A validator's level A rule), an empty or identity entry, a
second target for one name, a loop, and a mapping past `MAX_DICT_ENTRIES`
(4 096) — the most entries of one dictionary this engine's parser keeps, so
a longer `/RoleMap` would be written and partly dropped on read.
`map_role_in` stops at the same number per namespace.
**PDF 2.0's structure namespaces** (ISO 32000-2 14.7.4, 14.8.6) are read and
written. `DocumentBuilder::add_namespace(uri)` registers one — refused below
2.0, since `/NS` and `/Namespaces` are 2.0 keys — and `Tag::namespace(id)`
puts an element's type in it: the element gains `/NS`, an indirect reference
to a Table 356 namespace dictionary, and the root `/Namespaces` lists every
namespace registered. `map_role_in(ns, custom, target, target_ns)` writes
that namespace's `/RoleMapNS` as the `[/type ns]` pair the approved errata's
14.8.6.2 EXAMPLE 1 shows, and refuses what veraPDF's published PDF/UA-2
rules forbid: a mapping inside one namespace (8.2.4-3), a standard
namespace's type mapped out of the standard namespaces (8.2.4-4), a type
the 1.7 namespace does not define, a second target and a loop.
`PDF_1_7_NAMESPACE`, `PDF_2_0_NAMESPACE` and `MATHML_NAMESPACE` are the
three URIs the specification names. The reader keeps
`StructElement::namespace` (the element's `/NS` URI) and
`standard_namespace` (where `standard_type` ended): it follows `/RoleMapNS`
across namespaces, puts an element naming none in the default namespace
after `/RoleMap` (14.8.6.1 as the errata state it), and says `None` rather
than guessing where no source this build could read says — a bare-name
`/RoleMapNS` value, or a namespaced element the global `/RoleMap` moved.
`StructureTree::namespaces` lists the root's array.
`Tag::associated_file` writes an element's `/AF` (ISO 32000-2 14.13) and
`StructElement::associated_files` reads it back
([document-model](document-model.md) has the type). The `/A`, `/Headers` and
`/AF` entries the whole walk reads are capped together at
`structure::MAX_STRUCTURE_VALUES` (2^20), since one shared array named by
every element would otherwise be read once per element; past it they are
dropped and `StructureWarning::ValuesCapped` says so once. What the walk
copies out — every string and name an element, its attributes, its files and
the namespaces hand back — is capped at `structure::MAX_STRUCTURE_BYTES`
(64 MiB), since one string may be named from every element; past it a string
reads as absent and `StructureWarning::BytesCapped` says so once, naming the
element. A namespace's `/RoleMapNS` is looked up one type at a time where the
document holds it and never copied, because any number of namespace
dictionaries may share one map.
`continue_at(order)` continues the innermost open element in a fresh
sequence that reads at `order`, for an element with something drawn
elsewhere — a picture painted before its text — that reads between two of
its runs.
A link annotation added with `PageBuilder::link` **while an element is
open** is a content item of that element: its `/K` gains an `/OBJR`
(14.7.4.3) naming the annotation and its page, and the annotation a
`/StructParent` whose `/ParentTree` value is a reference to the element
(14.7.4.4) — so `tagged(b"Link", …)` around the text and the `link` call is
14.8.4.4.2's `/Link` element. `link_for(key, …)` does the same for an
element named by `tagged_keyed`'s key, on any page and whether it is open
or not, for a layout that measures link rectangles after drawing. A link
outside every element is written in no structure; one `finish` does not
write — a dangling named destination — leaves no `/OBJR` and takes no key.
`Tag::id(bytes)` writes `/ID` and the root's `/IDTree` (14.7.2 Table
322); an identifier is one element's, so the first in reading order keeps it
and `DocumentBuilder::duplicate_element_ids()` names the rest before
`finish`. `Tag::table(TableAttributes)` writes an attribute object owned by
`/Table` (14.8.5.7): `/Headers` (identifiers of the header cells that head a
cell), `/Scope` (`TableScope::Row`, `Column`, `Both`), `/Summary`, and the
spans. The reader hands back the same type — `StructElement::table` — and
`StructElement::id`, and `StructureTree::element_by_id` resolves a header;
a value Table 349 does not define is read as absent with
`StructureWarning::AttributeIgnored` naming the element, owner and key.
Nesting stops one level short of the reader's `MAX_NEST_DEPTH`, because
`finish` puts every page's elements under one `/Document`
(`opens_past_the_depth_cap_are_refused_and_still_paired`).
Appending to an element costs the same however many kids it already holds:
where its kids reach, which every resumption after a child and every link
asks, is kept beside them rather than walked each time, and `finish` finds a
keyed element's sibling, a `link_for` key's element and where that element's
kids reach through indexes rather than walks
(`an_element_of_many_kids_costs_its_kids` and
`a_tree_of_many_keyed_elements_costs_its_elements` count every kid's order
read and every key compared).

Coordinates are PDF user space, y upward — the space the page's own boxes
are in; a display transform is the caller's. The `Device` trait, the
interpreter, `TextDevice` and `RecordingDevice` live in `tinker-pdf-content`
and are architecture rather than public API — the facade projects none of
them, and ruling 11 is about what a *document* exposes, not about whether a
crate has an API of its own; see [architecture](../architecture.md).

## Refused by name

| What | Typed name | Why (one line) | See |
| --- | --- | --- | --- |
| A font resource the page names but the build cannot resolve | `TextWarning::UnknownFont { name }` | emitted by `Page::text()`, deduplicated, naming which resource — absence stays distinguishable from emptiness | ruling 2, [rulings](../rulings.md) |
| A code with no `/ToUnicode` entry and no encoding that names it | `TextWarning::UnmappedCode { code }` | the typed vocabulary for a per-code guess; a glyph that decodes to no text at all is dropped from the page rather than invented | 9.10.3 |
| Form XObject / Type 3 / soft-mask nesting past 16 levels | `MAX_FORM_DEPTH` | recursion is refused rather than allowed to overflow the stack | 8.10 |
| More than 4 096 open marked-content scopes | `MAX_MARKED_CONTENT_DEPTH` | scopes past the cap go unreported, and unreported means *visible* — a runaway stream must not hide a page | 14.6.2 |
| Text shaping while **reading** — Arabic joining, ligature substitution | — | the producer positioned every glyph and re-shaping them would be wrong; a ligature extracts as whatever its `/ToUnicode` says. Bidi *reordering* is not refused: ruling 14 puts a right-to-left line into logical order | [design/shaping.md](../design/shaping.md) |
| L4 mirroring undone in extraction, and the paragraph as a bidi unit | — | ruling 14 resolves each line alone and swaps no character: whether a producer's `/ToUnicode` names a mirrored glyph's character or its shape is not on the page. Alone, on a page that declares no direction for it (a structure element's `/WritingMode`, above), a line of a right-to-left paragraph that holds nothing left to right and opens with a mark of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is not `Ps` or `Pi` — a dash, a bullet, `*`, `#`, `%`, an ellipsis, a straight or closing-form quotation mark used to open; `— שלום` — draws as a quoted word and its trailing mark, and reads `שלום —`: the comma tie-break's known limit, pinned by `a_right_to_left_line_opening_with_a_dash_reads_the_dash_last`. Not with an opening bracket or quotation mark where `/ToUnicode` names the typed character (`(١) بند`, `“שלום`; `a_right_to_left_line_opening_with_a_bracket_or_quotation_mark_reads_as_typed`) — where it names the mirrored glyph, `»` for `«`, the line reads like a dash — nor with a European number after the mark (`(1) פריט`, `— 2026 שלום`, `• 5 תפוחים`, which no left-to-right paragraph draws; `a_right_to_left_line_opening_with_a_mark_and_a_number_reads_as_typed`); still with Arabic-Indic digits, `— ١ بند` reading `١ بند —`, or a number further on, `— שלום 5 חו`. A mark of several characters reads reversed as well, in the order drawn: the line `<p dir="rtl">?! שלום</p>` draws reads `שלום !?` where nothing declares its direction. The EPUB writer's book declares it, and reads `?! שלום` as typed, the mark styled apart or not (`a_run_of_neutrals_opening_a_right_to_left_line_is_drawn_as_typed`) | ruling 14 |
| Telling apart two texts UAX #9 draws alike | — | the algorithm is not one-to-one, so no reader can: `logical_order` returns the first order its search reaches that draws the line, and says which (`שלום now 2026`, not `שלום 2026 now`). Some of `BidiCharacterTest.txt`'s lines, each holding a bracket pair, read back as another text the same picture could be (N0 pairs brackets in logical order, and they are drawn mirrored). The paragraph's direction is such a choice where the line alone cannot make it: with no `dir`, a line with Arabic at both ends and a Latin run in brackets between, `… عوامل عدّة (HDAC). وقد …`, is a left-to-right paragraph's, its pair drawn facing inward — exactly the line a right-to-left paragraph whose `/ToUnicode` names the mirrored glyph draws for `وقد … (HDAC) … عوامل عدّة` — and on a page that declares no direction for it, it reads as that one, `(HDAC).` coming back `.)HDAC(`: both readings draw the line, and ruling 14's bracket tie-break counts only a pair facing outward. A page that declares the line's direction (a structure element's `/WritingMode`, above) reads it as typed, as the EPUB writer's book does. Pinned by `a_dirless_line_with_arabic_at_both_ends_and_latin_in_brackets_reads_as_its_drawn_alike_twin` | ruling 14 |
| Brackets typed closing before opening, each set against the text between them | — | a known limit of ruling 14's bracket tie-break: where both paragraph directions draw a line that draws a closing bracket left of an opening one, and the other direction's reading pairs more brackets in all and more of them facing outward and set against what they enclose, that reading is taken — so `x)שלום(y` in a left-to-right paragraph, drawn `x)םולש(y`, reads `y(שלום)x`, and so does a sentence written in this shape, `x)שלום(y and a)עולם(b`; a text typed so is a fragment rather than prose. Pinned by `brackets_typed_closing_first_against_a_word_read_as_the_other_direction_pairs_them` | ruling 14 |
| A line holding one bracket of a pair a line break parted, or quotation marks, whose two ends disagree | — | ruling 14 reads a line alone, and BD16 pairs no quotation mark, so the bracket tie-break has no pair to count on such a line: where its ends disagree its majority decides, and where its Latin letters outnumber its right-to-left ones it reads as a left-to-right paragraph's. A right-to-left paragraph's line that holds the second half of a bracketed Latin run, `deacetylase). وقد أصبحنا`, reads `وقد أصبحنا .)deacetylase`; one whose `(see` the line before holds, `[HDAC] here). وقد …`, is read as a text in which `[HDAC]`, drawn facing inward because the paragraph's `see` stood before it, pairs nothing, `HDAC] here[).`; and `“histone deacetylase”` closing a paragraph reads `”histone deacetylase“`. A justified line that opens with `( ` is cut at its stretched space, since the pen is not carried after neutrals alone (the text device above), and its bracket reads apart from its pair; and where the EPUB writer draws a run that opens with a stretched word space, ` (`, with its space first (`paint::opens_with_its_word_space`), the space falls inside the pair, which then hugs nothing, and the bracket tie-break leaves the line left to right. Reading a line with its block's other lines would be the paragraph as a bidi unit, which the mirroring row names | ruling 14 |
| Reading order for an **untagged** document | — | `plain_text()` reports lines and blocks in content-stream order — geometry decides only whether two glyphs are one line and two lines one block, and within a line only where it holds a right-to-left character (ruling 14); a structure tree is read when the document carries one, and never invented when it does not. `Page::text_in(ReadingOrder::Inferred)` is the opt-in request, answered by a type of its own — columns found from the whitespace between lines, blocks ordered down each column, a block across the columns read before the ones under it, running heads, feet and page numbers found by their recurring on the pages around and read first and last with their roles, footnotes read after the body in the order of their reference marks, a ruled table read as one block in its own order — and never the default ([design/reading-order.md](../design/reading-order.md)) | 14.8 |
| An `/MCR` whose `/Stm` does not name a content stream | `StructureWarning::ContentStreamNotAStream { element, stream }` | a stream is always indirect (7.3.8), so the value names nothing that could hold a sequence; read as though `/Stm` were absent rather than keyed on an object with no content, which would make the sequence findable nowhere | 14.7.4.2 |
| An `/MCR` carrying `/StmOwn` without the `/Stm` it qualifies | `StructureWarning::StreamOwnerWithoutStream { element, owner }` | Table 324 permits the owner only beside a stream; an owner alone names the owner of a stream nobody named, so it is dropped | 14.7.4.2 |
| An `/MCR` with no `/Stm` whose `/MCID` is in no page-stream sequence but in exactly one other stream on the page | `StructureWarning::ContentStreamAssumed { page, mcid }` | a producer that tags content inside a form and omits `/Stm` writes something 14.7.4.2 does not define; where one reading exists it is taken and named, and where two streams share the identifier it is refused, because that is the collision `/Stm` exists to resolve | 14.7.4.2 |
| A marked-content sequence in a stream `/StmOwn` says another object owns — an annotation's `/AP` | — | page text extraction runs the page's stream and the forms it invokes, never an annotation's appearance, so such a reference matches nothing here rather than taking whatever else shares its number; extracting appearance-stream text is separate work | 14.7.4.2, 12.5.5; [ROADMAP](../ROADMAP.md) FT-27 |
| Images inside a tiling pattern's cell, a soft-mask group or an annotation appearance | `Page::images()` does not list them | none is a drawing of the page's content: the renderer reaches a pattern cell and a mask group through its own device, not through the interpreter's `Do`, and an appearance belongs to the annotation | 8.7.3, 11.6.5, 12.5.5 |
| A JPEG 2000 image's own opacity channel in extraction | `PageImage::samples` holds the colour channels only | `/SMaskInData` decides what the channel means (8.9.5.4), and a soft mask carried out of the codestream is not an `/SMask` image the type can name; the renderer applies it | 8.9.5.4 |
| A table attribute with a value Table 349 does not define — a `/Scope` that is not `/Row`, `/Column` or `/Both`, a span below one, a `/Headers` entry that is not a string | `StructureWarning::AttributeIgnored { element, owner, key }` | read as absent rather than guessed at; the element and its other attributes are read as usual, so a table whose headers head nothing stays distinguishable from one that said something this reader could not read | 14.8.5.7 |
| More than 2^20 `/A`, `/Headers` and `/AF` entries across one structure tree (`MAX_STRUCTURE_VALUES`) | `StructureWarning::ValuesCapped`, once | a per-array cap does not bound them: one array shared by reference among every element is read once per element, 2^38 entries from a file of kilobytes (`shared_header_and_file_arrays_are_retained_within_one_budget`, `a_shared_attribute_array_is_visited_within_the_walks_values_budget`); every element is still read | ruling 1 |
| More than 64 MiB of strings and names copied out of one structure tree (`MAX_STRUCTURE_BYTES`) | `StructureWarning::BytesCapped { element }`, once; the string reads as absent | one indirect `/NS`, `/ID`, `/Alt` or `/Headers` string may be named from every element and would be copied once per mention — a mebibyte `/NS` shared by 2^18 elements is a quarter of a terabyte (`a_shared_namespace_is_copied_within_the_walks_copy_budget`); every element and its type are still read | ruling 1 |
| An element `/NS` that is not an indirect reference to a namespace dictionary carrying Table 356's required `/NS` URI | `StructureWarning::NamespaceIgnored { element }` | read as naming no namespace — the default one, after `/RoleMap` — rather than keyed on an object that names nothing; like `AttributeIgnored`, not a fault in the tree's shape, and the PDF/UA census does not count it as one | ISO 32000-2 Table 355 |
| Which namespace a type lands in after a **bare-name** `/RoleMapNS` value, or after the global `/RoleMap` moves a type that named a namespace | `StructElement::standard_namespace` is `None` | the Arlington model permits the bare name and the errata quote only the `[type ns]` pair and the global map's use for elements in no namespace; neither says which namespace either result is in, and `None` is that, said rather than guessed. The type itself is still resolved | ISO 32000-2 14.8.6.2 |
| Table attributes reached through an element's `/C` and the root's `/ClassMap` | — | `StructElement::table` reads the `/Table` owner's attribute objects in `/A` only; 14.7.6.2's classes are a second source of the same attributes that no writer here emits for a table, so it is named rather than half-read. A Layout `/WritingMode` is read through both (declared direction, above) | 14.7.6.2 |
| A written link that wraps across lines as **one** annotation with `/QuadPoints` | — | `PageBuilder::link` takes one rectangle, so a wrapped link is one annotation per rectangle, each an `/OBJR` of the one `/Link` element — which ISO 32000-1 14.8.4.4.2 permits ("one or more link annotations") and which the PDF Association's approved erratum 133 to ISO 32000-2 replaces, for a 2.0 document, with a single `/OBJR` to one annotation whose `/QuadPoints` mark each line | 14.8.4.4.2; ISO 32000-2 14.8.4.7.3 |

The rendering side of a hidden layer is reported too —
`RenderWarning::HiddenOptionalContent { layer }` names which layer was not
painted — but that row belongs to [rendering](rendering.md).

## Verified

`crates/tinker-pdf/tests/page_images.rs` builds each fixture with
`DocumentBuilder` from a known sample array and asserts `Page::images()`
returns exactly that array and the stated space — grey, RGB, CMYK, indexed
with its palette, ICC with its profile's bytes, one bit a sample with its row
padding, sixteen bits a sample whole, `/Separation` and `/DeviceN` with their
colorants, a colour-key mask and a soft mask; an image drawn twice and inside
a form listed once with three placements; an image only a form's resources
name found through the form's scope; inline images, one naming a page colour
space resource; a fax, lossless, one bit a sample even where its dictionary
claims eight, and a damaged fax, as an XObject and inline, whose `warnings`
are the reasons the render of the same page names; an indirect colour-key
mask read as the renderer reads it; a JPEG held to the decoder's own output;
hostile dictionaries and hostile inline images listed without a panic; and a
deterministic mutation sweep. `hostile_input.rs` and the `render_page` fuzz
target call `images()` on every page they reach.

`tpdf images` is held the same way in `tools/tpdf/src/images.rs`: a document
built from known arrays — RGB drawn twice, grey, a one-bit indexed image with
a soft mask, a one-component ICC-based image — is listed and written out, and
each file is the array the builder was handed: the indices, and beside them
the palette they index, the soft mask's opacity beside its image, the profile
beside the ICC-based samples, and nothing else in the directory. A
hand-written page holds the rest of the space the same way — a `CalGray`,
`CalRGB` and `Lab` image's parameters on the listing line, and the profile
under an indexed base, a separation's alternate and an ICC space's own
`/Alternate` each in its own file — and a stencil mask's samples, a refused
image and a damaged fax's leniency each reported under its image.

Unit tests live beside the code: `crates/tinker-pdf-content/src/tokenizer.rs`
(every escape form, malformed numbers, arbitrary-byte termination),
`text.rs` (artifact scopes nest, `ET` continuation versus baseline gaps,
a justified word space carrying the pen within right-to-left text and
cutting beside a left-to-right word, letter-spacing carrying it
into a right-to-left glyph and into nothing else — not a bracket, an angle
bracket or a dash after a right-to-left word, nor anything after neutrals on
a line that holds a Latin letter — and only as far as a twentieth of an em,
a word taken whole across a cut, a cut at a word space's stretch mended and
kept, the glyphs of each of those under a half and a quarter turn read into
the lines they make upright, and every threshold met exactly on an upright
line, under the identity, a shift and `scaleX(2)`, decided as the page's `x`
and `y` decide, search hit geometry,
wmode/rtl separation, non-finite glyphs dropped),
`crates/tinker-pdf/src/text_order.rs` (ruling 14 on hand-built lines: a
visual Hebrew line, one already in reading order, a mark on either side of
its base, digits in a right-to-left line, an out-of-order left-to-right line
left alone, a word and its comma alone read as the paragraph that drew them,
a line of twenty thousand marks, a line the stream cut in three
joined, a chain whose right-to-left character is in its last piece joined
whole where the bracket tie-break turns it, group by group where it does
not, and one with none left as collected, a zero-width mark drawn apart
joined to its base's line, and lines
apart on one baseline, on two baselines, with no right-to-left character or
drawn over each other left apart), and
`crates/tinker-pdf/tests/text_logical_order.rs` (the same on built pages:
both producer habits, the opt-out, search's box, the three structured
formats and the line's words, the comma tie-break — `חו,` and `حوت,` alone on
lines of a left-to-right page, `שלום.` still right to left, a line holding a
Latin letter out of its reach, a mark and a number opening a right-to-left
line read as typed, and so an opening bracket or quotation mark, and its
limit, a leading dash read last — the bracket tie-break, a right-to-left
line opening with a bracketed Latin run drawn as three pieces, a list marker
and a wrapped parenthesis in left-to-right text read as typed, and
every committed `testdata` document and EPUB book unchanged against the
opt-out),
`words.rs` (the table compiled from the file, segments of contractions,
decimals and abbreviations, combining marks, flag pairs, word boxes from a
real content stream, a raised character widening its word's box, a turned
line's turned box, a ligature inside its word, a 200 000-indicator line in
linear time), `search.rs` (the default equal to `search` quad for quad over
repeats, a two-character lower case, ligatures and lone marks; each option
alone and composed; whole word at UAX #29 boundaries without hiding an
overlapping candidate; precomposed and combining accents folded alike; harakat
and niqqud folded and a Devanagari vowel sign kept), `plain.rs` (hyphen
rejoining: a soft hyphen at a line end and inside one, a
hard hyphen before lower case, upper case, a digit and an uncased letter,
the dashes and U+2011 left alone, joins chaining, the default unchanged),
`interpret.rs` (group offer/decline, state save discipline), `record.rs` (a
small stream's whole event list asserted in order including the nesting; two
paints with different states asserted separately, which is what catches a
recorder that shares one handle and reports every event with the last state;
`q`/`Q` one for one; a declined form has no end; a glyph-only capture keeps
glyphs and copies no state; the per-event size pinned) and `state.rs` (matrix
convention, render-mode predicates).

Every test in `interpret.rs` drives `RecordingDevice`; the hidden-image test
asserts that the interpreter hands a hidden image to the device inside a
hidden scope.

`crates/tinker-pdf-content/tests/uax29_conformance.rs` runs every case of
`WordBreakTest.txt` against the segmenter `words()` calls; the case count is
pinned, so a truncated file fails.

`serialize.rs`'s unit tests hold each format's escaping to exact strings over
one hostile string — every C0 control, DEL, NEL, U+2028/U+2029, markup
characters, both noncharacters U+FFFE/U+FFFF, U+FDD0, and a character outside
the BMP — and check that a whole document in each format, over a page whose
text and font name are hostile, keeps them inside their quotes; spans split
at a font or size change and nowhere else.

Facade integration tests: `crates/tinker-pdf/tests/text_options.rs` (the
text options through `Page::text()` on a written document, every type named
from the facade), `text_serialize.rs` (`testdata/simple-text.pdf` written as
JSON and read back by a strict hand-written RFC 8259 parser in the test, with
Helvetica at 18 points and the boxes asserted on the parsed structure; the
same font and size in the XML and HTML; hostile text and a hostile font name
round-tripping through JSON exactly; a page and a form that both name their
font `/F0` reporting two different fonts), `inline_images.rs` (a
predictor-filtered inline image matches the identical XObject pixel for
pixel; compact `/F/Fl` dictionaries; `/EarlyChange` LZW; progressive inline
JPEG; `/Decode` inversion; a zlib bomb stops at the shared ceiling),
`stroke_parameters.rs`, `text_render_modes.rs`, `type3_fonts.rs`,
`vertical_metrics.rs`, `form_xobjects.rs`, `transparency_groups.rs`, and
`optional_content.rs` — which pins the asymmetry both ways: hidden text is
still extracted and still not drawn, and a hidden form's text still
extracts. `tinker_parity.rs` ports Tinker's own `text_and_search.rs`
assertions verbatim (ruling 12).

The `text` and `optional` determinism render fingerprints in
`crates/tinker-pdf/tests/determinism.rs` exercise this path directly. The
`content_tokenizer` and `render_page` fuzz targets have committed seed
corpora; `hostile_input.rs` sweeps the same shapes on stable. Nothing
compares this extractor against another one (ruling 13).
