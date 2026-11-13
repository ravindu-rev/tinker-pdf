# OCR

The tier's design: the stages a page passes through on its way to
searchable text, the crate each stage lives in and the value it hands to the
next, what says whether each one is right, and — in full — the stage that
comes first and needs no recogniser. That is the **import seam**: the words an
outside recogniser wrote in hOCR, ALTO or PAGE XML, read into FT-24's
word-box model and written over the page as an invisible text layer. The
stages after it add their sections to this document in their own rows
(searchable output, OC-02; preprocessing, OC-03; skew and geometry, OC-04;
orientation and script, OC-05). The recognition runtime, the models and the
layout analysis of scans have design documents of their own, named by their
rows: `ocr-inference.md` (OC-07), `ocr-models.md` (OC-08a) and
`scan-layout.md` (OC-06).

## Scope

- **The pipeline**, stage by stage: what each takes and returns, the crate
  it lives in and the row that builds it.
- **What adjudicates the tier** under ruling 13: first-party synthetic pages
  for every error rate, and a real scan only with ground truth a person
  transcribed (H-71).
- **The import** (OC-01.2): hOCR 1.2, ALTO and PAGE XML files read into
  FT-24's word-box model — pages, blocks, paragraphs, lines and words, each
  with its box, every word with its text and, where the file states one, its
  confidence — in the order the file states.
- **The text layer** (OC-01.3): the words of an imported page written over a
  PDF page as invisible text (render mode 3, ISO 32000-2 9.3.6) in a
  glyph-less font fitted to each word's box, with a `/ToUnicode` that gives
  the imported text back.
- **The crate** OC-01.4 creates, `tinker-pdf-ocr`, and the one edge it is
  declared with.

## Non-goals

- **Judging the imported text.** The recogniser that wrote a file supplies
  bytes and decides nothing (ruling 13). The import reads what the file
  states: it corrects no word, moves no box and re-segments no line. What
  holds the import to its formats is hand-written files whose words, boxes
  and order the tests state.
- **Running a recogniser.** The seam reads a file it is handed and spawns
  nothing, so `SPAWNERS` in `xtask/src/main.rs` does not move.
- **A layer that paints.** The layer is invisible on every reader; text drawn
  in shapes taken from the scan is OC-14's.
- **Changing what a page draws.** The layer goes over the page beside its
  content streams, which stay the objects they were, byte for byte.
  Replacing an earlier OCR layer is OC-11's redo mode, through ED-36f.

## Design

### The pipeline

| Stage | Takes | Returns | Lives in | Row |
| --- | --- | --- | --- | --- |
| Import | an hOCR, ALTO or PAGE XML file | FT-24's model, a page per page of the file | the facade, `ocr/import.rs` | OC-01.2 |
| Text layer | a model's page and a PDF page | the page with an invisible text layer over it | the facade, `ocr/layer.rs` | OC-01.3, OC-02 |
| Pixels | a PDF page | a raster: the image a scan draws, decoded, or the page rendered | the facade | OC-02, OC-11 |
| Preprocessing | a raster | a binarised, cleaned raster | `tinker-pdf-ocr` | OC-03 |
| Skew and geometry | a raster | its skew, and the raster deskewed or flattened | `tinker-pdf-ocr` | OC-04 |
| Layout | a binarised raster | blocks, columns, lines and words with their boxes | `tinker-pdf-ocr` (`scan-layout.md`) | OC-06 |
| Orientation and script | a page's lines | the page's turn and each block's script | `tinker-pdf-ocr` | OC-05 |
| Recognition | a line's raster | characters with positions and confidences | `tinker-pdf-ocr` (`ocr-inference.md`, `ocr-models.md`) | OC-07, OC-08a–e, OC-09 |
| Into the model | recognised lines | FT-24's model | the facade | OC-08a.6 |
| Output | the model | the formats, a searchable PDF, the workflows and `tpdf ocr` | the facade | OC-10, OC-02, OC-11 |

**FT-24's model is the seam the tier turns on.** The import and recognition
both end in it and every output starts from it, so the layer OC-01.3 writes
for an imported file is the layer OC-02 writes for a recognised one, and the
formats OC-10 writes are FT-24's writers over the same value.

**The leaf crate takes pixels and returns values.** `tinker-pdf-ocr` takes a
raster — width, height and samples as bytes — and plain parameters, and
returns plain values: boxes, angles, text and confidences, with no PDF
vocabulary in its API (ruling 8). Everything that knows what a page is
belongs to the facade: finding the image a scan draws, rendering a page,
filling the model, ordering a scan's blocks through FT-30's inference
(`scan-layout.md`), and writing the layer. The crate's one internal edge is
`tinker-pdf-math`, because the runtime's activations (OC-07.3) and the
geometry's angles are transcendental and ruling 4 sends every one of those
through it; and since a recognised word is as much an output that must not
differ by target as a pixel is, the crate is on `cargo xtask libm`'s
`PIXEL_PATHS`. Its row in `ALLOWED` is `("tinker-pdf-ocr",
&["tinker-pdf-math"])`, and the facade's row gains `tinker-pdf-ocr`.

**Scope, models and bundling are the owner's answers.** Printed text comes
first — Latin, then Cyrillic and Greek, Arabic and Hebrew, the Indic scripts
and CJK — and handwriting last (H-38). Models are trained here from data with
known licences (H-39). They are supplied by the host, with one small bundled
Latin model behind the `ocr` build profile (H-40), which holds its own
footprint budget (ruling 19).

### What adjudicates

**No recogniser this repository does not own says whether a recognised page
is right** (ruling 13). Every error rate the tier states is computed in
`cargo test` against **first-party synthetic pages**: known text set through
the engine, its words' boxes read back through FT-24's model, and the page
degraded deterministically from a seed — noise, blur, skew, warp, ink spread
(OC-03.2's generator). The test knows the text because it chose it, so the
rate answers to the page the test built and to nothing else's reading of it.

**A real scan counts only with ground truth a person transcribed** — people
other than the implementers, with the character and word error targets per
script the owner approves (H-71). An implementer's own annotation of a scan
proves nothing about the implementation it was made beside, and another
recogniser's output is an opinion: either may be an input, recorded with its
provenance, and neither is a check.

**An outside recogniser's file is an input to the import seam, never a
verdict.** The seam's tests hold the reader to hand-written files whose
words, boxes and order the test states — files written from the formats'
definitions, not taken from a recogniser's output — and FT-24.2's
first-party validator holds what the writers emit to the published schemas.

### The import

One entry point, `tinker_pdf::ocr::import(bytes)`, reads a file and returns
its pages in file order, each as FT-24's model of that page, with the
warnings the read raised; the caller pairs the file's pages with a
document's. The format is told from the file: a root element `alto` in no
namespace or in one of ALTO's (`http://www.loc.gov/standards/alto/ns-v2#`,
`ns-v3#`, `ns-v4#`) is ALTO; a root element `PcGts` in a namespace under
`http://schema.primaresearch.org/PAGE/gts/pagecontent/` is PAGE XML; markup
holding an element whose class list holds `ocr_page` is hOCR; anything else
is refused by name. ALTO and PAGE XML are read with `tinker-pdf-xml`'s reader,
which refuses a document type declaration with an internal subset before it
reads past it. hOCR's media type is HTML (`text/vnd.hocr+html`), so it is read
with `tinker_pdf_xml::html::parse_bytes`, as HTML, whether the file is
written as HTML or as XHTML.

The versions are the ones each format's own vocabulary reaches. hOCR is read
as its 1.2 specification states it (`https://kba.github.io/hocr-spec/1.2/`).
ALTO is read in its v2, v3 and v4 namespaces: v4 is the version FT-24's
writer emits, the elements and attributes the three share are spelled alike,
and what only a later version states — `ReadingOrder` and `BASEDIRECTION`,
from 4.3 — is simply absent from an earlier one's files. PAGE XML is read as
its 2019-07-15 schema states it, and the earlier dated schemas the same way:
before 2013-07-15 a `Coords` lists its polygon as `Point` children rather
than as a `points` attribute, and is read as the same polygon.

**What each level is read from:**

| Model | hOCR 1.2 | ALTO | PAGE XML |
| --- | --- | --- | --- |
| page | `ocr_page`: `bbox`, `image`, `ppageno`, `scan_res` | `Page`: `WIDTH`, `HEIGHT`, `PHYSICAL_IMG_NR`; `MeasurementUnit` | `Page`: `imageWidth`, `imageHeight`, `imageXResolution`, `imageYResolution` |
| block | `ocr_carea`, `ocrx_block` (and `ocr_column`, its old name) | `TextBlock`, wherever it is: in `PrintSpace`, a margin or a `ComposedBlock` | `TextRegion`, nested ones included |
| paragraph | `ocr_par` | the block | the region |
| line | `ocr_line`, `ocrx_line`, or `ocr_header`, `ocr_footer`, `ocr_caption` or `ocr_textfloat` holding words directly | `TextLine` | `TextLine` |
| word | `ocrx_word` | `String`, its `HYP` joined to the last | `Word` |
| box | `bbox x0 y0 x1 y1` | `HPOS`, `VPOS`, `WIDTH`, `HEIGHT` | the bounds of `Coords`' points |
| text | the element's text content | `CONTENT` | the `Unicode` of the `TextEquiv` with the lowest `index`, or the first |
| confidence | `x_wconf`, 0 to 100 | `WC`, 0 to 1 | `TextEquiv`'s `conf`, 0 to 1 |

The paragraph is the level only hOCR states; where a file states none, the
model's paragraph is what FT-24.1 says a paragraph is when a page has no
paragraphs. Each confidence is carried onto the model's scale (FT-24.1) from
the scale its format states, and a value outside that scale is dropped with
a warning: the word is kept with no confidence.

**Words, read whole.** hOCR's `title` is read by its grammar — `key value`
pairs separated by semicolons, a value an ASCII word or a double-quoted
string — and `bbox`'s four numbers are read as decimal numbers, the integers
its grammar states and the decimals some files carry alike. A word's text is
its element's text content with an `alternatives` span contributing its
first `ins` alone, the most probable reading; ASCII white space is taken off
both ends and a run of it inside collapsed to one space, and a `&nbsp;` stays
U+00A0. ALTO's `SP` separates strings and draws nothing; its `HYP`, which
stands only at a line's end, joins its content to the line's last `String`,
and its box, where it states one, to that word's box, so the word the model
holds is the one printed on the line. A `SUBS_CONTENT` — the whole of a word
hyphenated across two lines — is not what the line shows, and the model keeps
`CONTENT`. A line with text and no word elements — an hOCR line holding text
directly, or a PAGE `TextLine` with a `TextEquiv` and no `Word` — becomes one
word: the line's text over the line's box. Characters that are no text — the
C0 and C1 controls, U+FEFF and the noncharacters — are taken out of a word,
each with a warning naming it, so no layer's `/ToUnicode` ever states one.

**Order is the file's.** The model's order is the reading order the file
states, and a file that states none is read in document order:

| Format | Blocks | Lines | Words |
| --- | --- | --- | --- |
| hOCR | document order, which the specification makes the reading order of `ocr_carea`; siblings that all carry `order` in its order | document order; siblings that all carry `order` in its order | document order |
| ALTO | the first group of `ReadingOrder` (4.3 and later), depth first, an `ElementRef` to a line or a string placing its block at its first mention; else each `IDNEXT` chain from a block nothing names; else document order | document order | document order |
| PAGE XML | `ReadingOrder`'s groups depth first, ordered groups by `index`; else document order | `index`, where every line of the region carries one; else document order | document order |

Blocks no stated order names follow those it names, in document order, and
an unordered group's members are in document order among themselves.

**The frame is the file's.** All three formats state a page in one frame:
origin at the top-left corner of the page image, x to the right, y
downwards — hOCR's `bbox` is "with reference to the top-left corner of the
document image and measured in pixels", ALTO's coordinates are "absolute
coordinates referring to the upper-left corner of a page", and PAGE XML's
points have "0,0 in the upper left corner of the root image". The unit is
the pixel for hOCR and PAGE XML, and ALTO's `MeasurementUnit` for ALTO:
`pixel`, `mm10` or `inch1200`. An imported page keeps the file's own numbers
in that frame, with its extent — `ocr_page`'s `bbox`, `Page`'s `WIDTH` and
`HEIGHT`, `imageWidth` and `imageHeight` — and the resolution where the file
states one (`scan_res`, `imageXResolution` and `imageYResolution`; `mm10` and
`inch1200` are 254 and 1 200 units an inch by their names). No coordinate is
rescaled on the way in, so a model filled from a file writes back through
FT-24's writers in the file's own frame; the one mapping onto a PDF page is
the layer's.

**What else a line needs.** The model holds boxes; a layer also needs each
line's direction, which the model does not hold, so the import's page carries
it beside the model, line for line:

- **the turn**: the angle the file states the text is turned by,
  counter-clockwise — hOCR's `textangle`, ALTO's `ROTATION` on the page or
  the block, and PAGE XML's `orientation` on the region (the angle that
  turns the region clockwise back upright), inherited from the nearest
  element that states one — and a line ALTO states `btt` or PAGE XML
  `bottom-to-top` is turned a quarter turn, one stated `ttb` or
  `top-to-bottom` three quarters;
- **the paragraph direction**, where the file states one: hOCR's `dir`
  attribute, inherited as HTML inherits it, ALTO's `BASEDIRECTION` and PAGE
  XML's `readingDirection`.

**What the import tolerates, and what it refuses.** Ruling 1 binds it — a file
is untrusted markup, and every count, depth and number it reads is bounded —
and ruling 10 makes every leniency a typed warning naming the file's element,
by its `id` or `ID` where it has one and by its page, line and word index
where it does not:

| Case | What happens |
| --- | --- |
| A word with no box, or with a box whose far edge is before its near one | skipped, `ImportWarning::WordWithoutBox` or `InvertedBox` |
| A word whose text is empty once read | skipped, `ImportWarning::EmptyWord` |
| A character that is no text | taken out of the word, `ImportWarning::NotText` |
| A confidence outside its format's scale | the word kept with none, `ImportWarning::ConfidenceOutOfRange` |
| A box reaching outside its page | kept as the file states it, `ImportWarning::OutsidePage` |
| A reading order naming an element that is not there, or naming one twice | the name skipped, `ImportWarning::OrderNamesNothing` |
| More words on a page than `MAX_IMPORTED_WORDS` | the rest dropped, `ImportWarning::TooManyWords`, a bound with its row in `bounds_ledger.rs` |
| A file that is none of the three formats | `ImportError::NotRecognised` |
| Markup the reader refuses | `ImportError::Markup`, with the reader's own refusal |
| A page with no extent: an ALTO `Page` with no `WIDTH` or `HEIGHT`, an `ocr_page` with no `bbox` | the file refused, `ImportError::PageWithoutExtent` naming the page, since no layer can place a page it cannot measure |
| An ALTO `MeasurementUnit` other than the three | `ImportError::UnknownUnit` |

### The text layer

`tinker_pdf::ocr::add_text_layer(editor, page, imported, onto)` writes an
imported page's words over page `page` of the document `editor` edits, and
says which words it drew and which it could not.

**The frame onto the page.** By default the file's page is the PDF page:
its extent is mapped onto the page as it is displayed — the crop box turned
by `/Rotate` — scaled on each axis by the ratio of the two, since a file
describing a scan describes the page the scan is. A caller whose scan is
drawn on a part of its page names that rectangle instead, in default user
space as the page displays it. The default guesses nothing about where a
page's image is.

**The layer is one form XObject stamped over the page.** The form
(`DocumentEditor::add_form`) has the file's page as its `/BBox`, a `/Matrix`
that is the frame above, with the file's downward y turned upward, and its
own `/Resources` holding the layer's font. `DocumentEditor::stamp` with
`StampPlacement::Over` draws it after everything the page draws and closes
whatever the page's content leaves open first, so the layer runs in the
page's initial state; and the page's own content streams are not rewritten,
which keeps an incremental save small and lets a signature over them still
say that the content it signed is what the page draws.

**The glyph-less font.** One composite font a layer (9.7): a Type 0 font
whose encoding is `/Identity-H` (9.7.5.2), so every code is two bytes and is
its own CID, over one CIDFontType2 descendant:

- **an embedded program of two glyphs**, `.notdef` and one empty glyph, each
  1 000 units wide on a 1 000-unit em: a TrueType program (`/FontFile2`)
  holding the six tables 9.9 requires — `glyf`, empty, `head`, `hhea`,
  `hmtx`, `loca` and `maxp` — with `post`, and no `cmap`, which 9.9 says a
  CIDFont's program shall not carry;
- **`/CIDToGIDMap` a stream** mapping every CID the layer draws to the empty
  glyph (9.7.4.2), so no code shows `.notdef`, and **`/DW 1000`**, the
  program's own width, so the dictionary and the program agree on every
  width;
- **a font descriptor whose box is the glyph box the extractor draws**:
  `/FontBBox [0 -200 1000 800]`, `/Ascent 800`, `/Descent -200`. Text
  extraction boxes a glyph from a fifth of the font size below its baseline
  to four fifths above it, whatever the font (`TextDevice::show_glyph`), and
  a reader that boxes text by the font's own metrics reads the same
  rectangle;
- **a `/ToUnicode`** (9.10.3) mapping each code to its character. A layer
  gives each distinct Unicode scalar value it draws a CID, numbered from 1 in
  the order they first appear, U+0020 among them; CID 0 is never drawn. A
  layer draws at most 65 535 distinct characters, the CID range, and a page
  needing more is refused by name (`LayerError::TooManyCharacters`).

A composite font rather than a Type 3 one, whose glyph procedures could
paint nothing and need no program: a Type 3 glyph does not reach the text
device today (FT-06.2), and a simple font has 256 codes where a page's text
can need thousands. An embedded program rather than none: a CIDFontType2 with
no program selects its glyphs through a font the reader finds by name
(9.7.4.2), which a reader with no such font substitutes; a program of empty
glyphs is the same font on every reader. Render mode 3 paints nothing
(9.3.6), and an empty glyph paints nothing either, so a reader that ignored
the mode would still draw nothing.

**Each word fitted to its box.** In the form's space a word whose box is
`w` wide and `h` high and whose text is `n` characters is shown one glyph a
character, at font size `h`, with its baseline a fifth of `h` above the
box's bottom edge, and with the horizontal scaling (9.3.4) that makes the `n`
one-em advances span `w`: `Tz` is `100 · w / (n · h)`. The extractor's glyph
boxes then abut along the line and their union is the word's box: its
bottom edge a fifth of the size below the baseline, its top four fifths
above, its sides where the word starts and where its advances end. The
layer writes its numbers to six decimals, so each edge comes back within
10⁻³ of the file's unit.

**A line is one text object.** A line's words are shown in one `BT` … `ET`
and share one baseline, the first word's; each word is then raised or
lowered onto its own box by text rise (9.3.7), because the extractor decides
which line a glyph is on at the baseline a rise leaves where it was, so a
line whose words' boxes differ in height is still one line. A U+0020 glyph is
shown after each word but the last, its scaling taking it to the next word's
box where that starts further along the line, and a hundredth of an em
where it does not; the extractor joins a line's glyphs as they are drawn, so
the line's text is its words with one space between each two.

**Turns and directions.** A line is shown along the quarter turn nearest
its stated turn: right, up, left or down. An axis-aligned box holds a
quarter-turned word exactly — its run along the box's height for a quarter
or three-quarter turn, and its size the box's width — and an angle between
quarter turns is one no box can state, so the word fills its box along the
turn nearest it. A line holding a right-to-left character is shown in the
visual order UAX #9 gives its imported text
(`tinker_pdf_shape::bidi::order_units`, the drawing direction ruling 14
checks every reading against), in the paragraph direction the file states
or else the one P2 and P3 find, each word's glyphs over its own box; and the
line is enclosed in a marked-content sequence
`/Span << /ActualText (…) >> BDC … EMC` (14.9.4) stating its
imported text, so that it reads back as imported even where UAX #9 draws two
texts alike (ruling 14, *What a page says*). A line with no right-to-left
character carries no span: ruling 14 leaves such a line exactly as it is
drawn.

**What the layer refuses, and what it skips.** A word whose box has no width
or no height cannot be fitted and is skipped,
`LayerWarning::WordWithoutArea`, naming its line and word; a page the
document does not have is `LayerError::NoSuchPage`. The layer never fails a
page for one word.

## Milestones

1. **OC-01.2, the import.** `tinker_pdf::ocr::import`, its warnings and its
   refusals; `docs/features/ocr.md` (new), which `docs/README.md` indexes,
   describes it; and a fuzz target of its own, as every parser has.
   Exit: `crates/tinker-pdf/tests/ocr_import.rs` (new) —
   `an_hocr_file_reads_to_the_words_boxes_and_order_it_states` (blocks,
   paragraphs, lines and words, a line holding text and no words, an
   `alternatives` span, `order` on sibling blocks, and two pages);
   `an_alto_file_reads_to_the_words_boxes_and_order_it_states` (a
   `ReadingOrder` that differs from document order, an `IDNEXT` chain, a
   `HYP`, a `String` in a `ComposedBlock`; the same page in `mm10` and in
   `inch1200`, and in the v2 and v3 namespaces, reading to the same model in
   their units); `a_page_xml_file_reads_to_the_words_boxes_and_order_it_states`
   (polygon `Coords`, a `ReadingOrder` that differs from document order,
   lines ordered by `index`, a `TextLine` with no `Word`, and the same page
   under the 2013-07-15 namespace); `confidences_reach_the_models_scale`;
   `each_tolerated_defect_is_a_named_warning` (one case per warning row
   above); `a_file_that_is_none_of_the_three_is_refused_by_name`;
   `arbitrary_bytes_never_panic` (proptest). `fuzz/fuzz_targets/ocr_import.rs`
   (new), seeded with the three formats' hand-written files under
   `fuzz/corpus/ocr_import/`, listed in `fuzz/Cargo.toml` and in
   `xtask/src/fuzzaudit.rs`'s `UNSIGNED` (markup text has no header to
   look for), and `cargo xtask fuzz` is green with it.
2. **OC-01.3, the layer.** `tinker_pdf::ocr::add_text_layer` and the
   glyph-less font; `docs/features/ocr.md` gains the layer. Exit:
   `crates/tinker-pdf/tests/ocr_import.rs` —
   `the_layer_extracts_as_the_imported_text_in_the_imported_order` (each
   format's file layered over a page with no text, and over a page with text
   of its own, whose lines come before the layer's);
   `every_word_s_box_equals_the_file_s` (the union of the quads of each
   word's characters, carried back through the frame, within 10⁻³ of the
   file's unit on every edge, on pages at
   `/Rotate` 0, 90, 180 and 270 with a crop box offset from the media box,
   and placed onto a rectangle the caller names);
   `the_page_renders_to_the_same_pixels_as_before` (byte for byte, raising
   no warning the page did not raise before);
   `a_turned_line_extracts_as_one_line` (each quarter turn);
   `a_right_to_left_line_extracts_as_imported`;
   `a_word_without_area_is_skipped_and_named`; and the saved document,
   reopened, held to the strict validator as every writing test is
   (`docs/design/validate-on-save.md`). In the default and `bundled-fonts`
   builds.
3. **OC-01.4, the crate.** `tinker-pdf-ocr` in the workspace, in `ALLOWED`
   as `("tinker-pdf-ocr", &["tinker-pdf-math"])`, the facade's row gaining
   it, and on `PIXEL_PATHS`. Exit: `cargo xtask check` is green with the
   crate in it, and `cargo test -p tinker-pdf-ocr` runs.
4. **The stages' own sections**, each added by its row's design step and
   naming that row's exit tests: searchable output (OC-02.1), preprocessing
   (OC-03.1), skew and geometry (OC-04.1), and orientation and script
   (OC-05.1).

## Risks

| Risk | Mitigation |
| --- | --- |
| The box equality rests on the extractor's glyph box, a fifth of the size below the baseline and four fifths above; a change to that rule moves every layer's boxes | The font's descriptor states the same box, so a reading by the font's metrics agrees with it, and `every_word_s_box_equals_the_file_s` fails at the first change |
| Two lines the file states apart, on one baseline and within half an em of each other, extract as one line: the extractor resumes a line an `ET` closed within half an em, which is a space and not a column | Stated. Such lines are one line to any reader of the page, and the layer cannot make a reader part what it reads as one |
| A file whose words do not run along their line in the order it states them — a recogniser's quirk — extracts as two lines where the pen moves back more than three ems | Stated. The text still comes back in the imported order, with a line break where the pen went back |
| A column of vertical text the file states no direction for, which hOCR has no property for, reads as a line a word | Stated. Its words come back in order, one to a line; a file that states the direction gets the turned line it states |
| A right-to-left line reads as imported by its `/ActualText`; a reader that ignores replacement text reads the drawn visual order instead | The drawn order is the one UAX #9 gives the imported text, which a reader running UAX #9 backwards reads back the same, except for the texts it draws alike — which is what the span is for |
| The layer is content no structure element claims: on a tagged page `Page::structured_text` counts its characters as orphans, and an accessibility claim does not cover them | Stated. A searchable document this tier writes decides its own structure (OC-02); a layer added to a tagged document says nothing about the tree it is added to |
| A scan drawn on a part of its page, layered by the default, gets a layer at the page's scale rather than the image's | The caller names the rectangle; the default maps onto the page and guesses nothing |
| A hostile file — deep nesting, a vast word count, numbers past every range — exhausts the reader | `tinker-pdf-xml`'s limits bound the markup, `MAX_IMPORTED_WORDS` the words, every number is checked finite, and `ocr_import` fuzzes the whole read |
| A word of many characters in a narrow, tall box scales its glyphs to a sliver | Nothing reads the glyphs' shape: the scaling is a number written to six decimals, and the box test holds the edges it produces |
