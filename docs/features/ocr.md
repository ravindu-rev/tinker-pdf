# OCR

The OCR tier's facade half. What exists is its import seam (ROADMAP row
OC-01): the words an outside recogniser wrote in hOCR, ALTO or PAGE XML, read
into FT-24's word-box model ([word boxes](../design/word-boxes.md)) by
`tinker_pdf::ocr::import`, and written over a PDF page as an invisible text
layer by `tinker_pdf::ocr::add_text_layer`. The design, the pipeline the tier
builds around this seam and what adjudicates it are
[design/ocr.md](../design/ocr.md).

The recogniser that wrote a file supplies bytes and decides nothing (ruling 13).
The import reads what the file states: it corrects no word, moves no box and
re-segments no line.

## What it does

**The format is told from the file.** A root element `alto`, in no namespace
or in ALTO's v2, v3 or v4 namespace (`ALTO_NAMESPACES`), is ALTO; a root
element `PcGts` in a namespace under
`http://schema.primaresearch.org/PAGE/gts/pagecontent/`
(`PAGE_NAMESPACE_PREFIX`) is PAGE XML. Both are read with `tinker-pdf-xml`'s
reader, which refuses a document type declaration with an internal subset
before it reads past it. Anything else is parsed as HTML, hOCR's media type,
with `tinker_pdf_xml::html::parse_bytes` — so an hOCR file written as XHTML is
read the same way as one written as HTML — and is hOCR when an element's class
list holds `ocr_page`. Anything else again is refused by name.

**What each level is read from.**

| Model | hOCR 1.2 | ALTO | PAGE XML |
| --- | --- | --- | --- |
| page | each `ocr_page` not inside another: `bbox`, `image`, `ppageno`, `scan_res` | each `Page` of the `Layout`: `WIDTH`, `HEIGHT`, `PHYSICAL_IMG_NR`; the description's `MeasurementUnit` and `fileName` | the `Page`: `imageWidth`, `imageHeight`, `imageXResolution`, `imageYResolution` (and `imageResolutionUnit`), `imageFilename` |
| block | `ocr_carea`, `ocrx_block` and `ocr_column`, the outermost where they nest | each `TextBlock`, in the `PrintSpace`, a margin or a `ComposedBlock` | each `TextRegion` that holds a `TextLine`, nested ones included |
| paragraph | `ocr_par` | the block | the region |
| line | `ocr_line`, `ocrx_line`, and `ocr_header`, `ocr_footer`, `ocr_caption` and `ocr_textfloat` where they hold no line | `TextLine` | `TextLine` |
| word | `ocrx_word` | `String`, its `HYP` joined to the last | `Word` |
| box | `bbox x0 y0 x1 y1` | `HPOS`, `VPOS`, `WIDTH`, `HEIGHT` | the bounds of `Coords`' points |
| text | the element's text content | `CONTENT` | the `Unicode` of the `TextEquiv` with the lowest `index`, or the first |
| confidence | `x_wconf`, 0 to 100 | `WC`, 0 to 1 | `TextEquiv`'s `conf`, 0 to 1 |

Where a file states no paragraph, a block's paragraph is the block (FT-24.1's
rule for a page that states none): an hOCR block's lines outside any `ocr_par`
are one paragraph a run, and an ALTO block or a PAGE XML region is one. Lines
an hOCR page holds outside any block are one block a run, and words outside
any line one line a run. A box a file does not state is the union of what it
holds.

**Words, read whole.** hOCR's `title` is read by its grammar (hOCR 1.2 2.4):
properties separated by semicolons, each a name and values, a value a word or
a double-quoted string, a semicolon inside quotes part of the value. `bbox`'s
four numbers are read as decimal numbers, the integers its grammar states and
the decimals some files carry alike. A word's text is its element's text
content, an `alternatives` span contributing its first `ins` alone (5.7). ASCII
white space is taken off both ends of a word and a run of it inside is one
space; a `&nbsp;` stays U+00A0. An ALTO `HYP` joins its content to its line's
last `String`, and its `HPOS` and `WIDTH` (and `VPOS` and `HEIGHT` where it
states them) to that word's box, so the word held is the one printed on the
line; a `HYP` with no `String` before it is a word of its own. `SUBS_CONTENT`
is not what the line shows, and `CONTENT` is kept. A line with text and no
word — an hOCR line holding text directly, or a PAGE XML `TextLine` with a
`TextEquiv` and no `Word` — is one word: the line's text over the line's box.
PAGE XML's `Coords` is read from its `points` attribute, or from `Point`
children as the schemas before 2013-07-15 write it.

**Order is the file's.**

| Format | Blocks | Lines | Words |
| --- | --- | --- | --- |
| hOCR | document order; siblings that all carry `order` (4.11) in that order | document order; siblings that all carry `order` in that order | document order |
| ALTO | the first group of the document's `ReadingOrder`, depth first, an `ElementRef` to a line or a string placing its block at its first mention and one to a `ComposedBlock` its blocks; else each `IDNEXT` chain from a block nothing names, in document order; else document order | document order | document order |
| PAGE XML | the `ReadingOrder`'s group depth first, a group's own `regionRef` before its members, an ordered group's members by `index`; else document order | `index`, where every line of the region carries one; else document order | document order |

Blocks no stated order names follow those it names, in document order; an
unordered group's members are in document order among themselves, by the
first block each names. An `IDNEXT` naming a block on another page ends its
chain.

**The frame is the file's.** All three formats state a page in one frame:
origin at the top-left corner of the page image, `x` to the right, `y`
downwards. An imported page keeps the file's numbers in that frame and in the
file's unit — the pixel for hOCR and PAGE XML, and ALTO's `pixel`, `mm10` or
`inch1200` — and no coordinate is rescaled on the way in. Each page is an
`ImportedPage`:

- `page`, the model's `WordPage`: every box the rectangle the file states, as
  an upright quad; `width` and `height` the extent's far edges rounded up to
  whole numbers; `resolution` the horizontal resolution the file states, or
  the unit's own for `mm10` (254 an inch) and `inch1200` (1 200), and 72 where
  the file states none; `image` the page's image name; and `index` the
  `ppageno` (hOCR) or `PHYSICAL_IMG_NR` less one (ALTO), or else the page's
  position in the file. A line's direction is `TopToBottom` for a line the file
  states runs top to bottom, `RightToLeft` for one whose paragraph it states
  runs right to left, and `LeftToRight` otherwise. A word's confidence is the
  one its file states, carried onto the model's 0 to 100.
- `extent`: `ocr_page`'s `bbox`, or the origin to ALTO's `WIDTH` and `HEIGHT`
  or PAGE XML's `imageWidth` and `imageHeight`.
- `unit`, and `resolution`, the horizontal and vertical resolution as the file
  states them (PAGE XML's `PPCM` carried to pixels an inch).
- `lines`: one `LineSetting` a line of the model, in the model's order, with
  what a layer needs and the model does not hold — the **turn**, the angle the
  file states the line's text is turned by, counter-clockwise, from 0 up to
  360: hOCR's `textangle`, ALTO's `ROTATION` on the page, a composed block or a
  block, and PAGE XML's `orientation` on the page or a region, inherited from
  the nearest element that states one, with a quarter turn more for a line
  ALTO states `btt` or PAGE XML `bottom-to-top` and three quarters for one
  stated `ttb` or `top-to-bottom`; and the **paragraph direction** where the
  file states one: hOCR's `dir`, inherited as HTML inherits it, ALTO's
  `BASEDIRECTION` and PAGE XML's `readingDirection`, inherited from the line's
  region and page.

**The text layer.** `add_text_layer(editor, page, imported, onto)` writes
an imported page's words over page `page` of the document a
`DocumentEditor` edits, as text in render mode 3 (9.3.6), which paints
nothing, in a font whose glyphs are empty, which would paint nothing either.

- **The frame.** The file's extent is mapped onto the page as displayed —
  its crop box turned by `/Rotate` — scaled on each axis, or onto the
  rectangle `onto` names, in points on the displayed page with its origin at
  the lower-left corner and `y` up. The default guesses nothing about where a
  page's image is.
- **One form, stamped over the page.** The layer is one form XObject drawn
  by `DocumentEditor::stamp` with `StampPlacement::Over`, after everything
  the page draws and outside whatever its content leaves open, so the page's
  own content streams stay the objects they were. The form's space is the
  displayed page in points, so every number the layer writes is a point to
  six decimals and carries back to well inside a thousandth of the file's
  unit; its `/Matrix` turns that space back into the page's own, and its
  `/BBox` is the file's page placed on it.
- **The glyph-less font.** A Type 0 font under `/Identity-H` over a
  CIDFontType2 whose embedded TrueType program has `.notdef` and one empty
  glyph, each 1 000 units wide on a 1 000-unit em, in `glyf`, `head`,
  `hhea`, `hmtx`, `loca`, `maxp` and `post` and no `cmap`; `/CIDToGIDMap`
  sends every CID the layer draws to the empty glyph, `/DW` is 1 000, the
  descriptor's box is `[0 -200 1000 800]` — the box text extraction gives a
  glyph — and a `/ToUnicode` gives each CID back its character. Each distinct
  character the layer draws is a CID, from 1 in the order it is first drawn,
  the space among them; CID 0 is never drawn.
- **Each word fitted to its box.** A word whose box on the page is `w` wide
  and `h` high and whose text is `n` characters is shown one glyph a
  character at size `h`, its baseline a fifth of `h` above the box's bottom
  edge, under the horizontal scaling `100 · w / (n · h)`, so its glyph boxes
  abut and their union is the word's box.
- **A line is one text object**, its words on the first word's baseline and
  each raised or lowered onto its own box by text rise (9.3.7), so the
  extractor, which asks which line a glyph is on at the baseline, reads one
  line. A space glyph follows each word but the last, as long as the gap to
  the next word's start where that starts further along the line and a
  hundredth of an em where it does not, so the line reads as its words with
  one space between each two.
- **Turns and directions.** A line is shown along the quarter turn nearest
  its setting's turn — right, up, left or down — each word filling its box
  across the turn and along it. A line holding a right-to-left character
  (UAX #9's `R` or `AL`) is drawn in the visual order UAX #9 gives its text,
  `tinker_pdf_shape::bidi::order_units` in the paragraph direction the file
  states or else the one rules P2 and P3 find, each word's glyphs over its
  own box, inside `/Span << /ActualText (…) >> BDC … EMC` stating the
  imported text (14.9.4), so it reads back as imported; a line holding none
  carries no span.

`Layered` says what was drawn: the form stamped (none where no word could
be drawn, and then nothing is added), how many words, and a `LayerWarning`
for each word skipped.

**Bounds.** A file is untrusted markup (ruling 1). `tinker-pdf-xml`'s limits
bound the markup — its depth, its events, its names — for both readers, every
number read is checked finite, every walk over a parsed file is bounded by
depth, and a page keeps at most `MAX_IMPORTED_WORDS` words, in reading order.

## API

| Item | What it is |
| --- | --- |
| `ocr::import(&[u8]) -> Result<Imported, ImportError>` | the file's pages, in file order |
| `Imported` | `format` (`ImportFormat::{Hocr, Alto, PageXml}`), `pages`, `warnings` |
| `ImportedPage` | `page` (a `WordPage`), `extent` (an `Extent`), `unit` (a `Unit`), `resolution`, `lines` (a `LineSetting` a line) |
| `LineSetting` | `turn` in degrees, `direction` (`Option<Direction>`) |
| `Unit::per_inch` | 254 for `Mm10`, 1 200 for `Inch1200`, none for `Pixel` |
| `Located` | the page's position in the file, the element's `id` or `ID`, the line's position on the page and the word's in its line, each in document order |
| `MAX_IMPORTED_WORDS`, `ALTO_NAMESPACES`, `PAGE_NAMESPACE_PREFIX` | the word cap and the namespaces the format is told by |
| `ocr::add_text_layer(&mut DocumentEditor, page, &ImportedPage, Option<Rect>) -> Result<Layered, LayerError>` | the page's words written over page `page` as an invisible text layer |
| `Layered` | `form` (the stamped form, `Option<ObjRef>`), `drawn`, `warnings` (`LayerWarning`s) |
| `MAX_LAYER_CHARACTERS` | the distinct characters one layer names, 65 535 |

The model is FT-24's, so an imported page is written back by its writers
([word boxes](../design/word-boxes.md)) in the file's own frame.

## Refused by name

Every leniency is a typed warning naming the file's element — by its `id` or
`ID` where it has one, and by its page, line and word position always — and
every refusal is an error (ruling 10).

| Case | Typed variant | What happens |
| --- | --- | --- |
| A word with no box, or a box that is not four finite numbers | `ImportWarning::WordWithoutBox` | skipped |
| A word whose box's far edge is before its near one | `ImportWarning::InvertedBox` | skipped |
| A word whose text is empty once read | `ImportWarning::EmptyWord` | skipped |
| A character that is no text — a control character (Unicode's `Cc`: C0, DEL and C1), U+FEFF or a noncharacter | `ImportWarning::NotText` | taken out of the word, one warning a character, so no layer's `/ToUnicode` ever states one |
| A confidence that is not a number on its format's scale | `ImportWarning::ConfidenceOutOfRange` | the word kept with none |
| A word's box reaching outside its page | `ImportWarning::OutsidePage` | kept as the file states it |
| A reading order naming an element the file does not hold, or naming one a second time | `ImportWarning::OrderNamesNothing` | the name skipped; the page's for PAGE XML and an `IDNEXT`, none for ALTO's document-wide `ReadingOrder` |
| More words on a page than `MAX_IMPORTED_WORDS` | `ImportWarning::TooManyWords` | the rest dropped, once a page; a `bounds_ledger.rs` row |
| A file that is none of the three formats | `ImportError::NotRecognised` | refused |
| ALTO or PAGE XML that is not well-formed, hOCR past the HTML parser's caps, or a document type declaration with an internal subset in a file that holds no hOCR page | `ImportError::Markup` | refused, with the reader's own refusal |
| A page with no extent, or one with no area | `ImportError::PageWithoutExtent` | refused, naming the page's position: no layer can place a page it cannot measure |
| An ALTO `MeasurementUnit` other than `pixel`, `mm10` and `inch1200` | `ImportError::UnknownUnit` | refused |
| A word whose box on the page has no width or no height, or is not finite | `LayerWarning::WordWithoutArea` | skipped, named by its line among the page's and its place in the line; the rest of the page drawn |
| A word with no text, in a model a caller built | `LayerWarning::EmptyWord` | skipped, named the same way |
| A page the document does not have | `LayerError::NoSuchPage` | no layer written |
| An imported extent, a rectangle named or a crop box with no area | `LayerError::NoArea` | no layer written |
| More distinct characters than `MAX_LAYER_CHARACTERS` | `LayerError::TooManyCharacters` | no layer written: a layer's two-byte codes name no more |

## Verified

- `crates/tinker-pdf/tests/ocr_import.rs` — files of each format written here
  from the formats' own definitions, never taken from a recogniser:
  `an_hocr_file_reads_to_the_words_boxes_and_order_it_states` (blocks,
  paragraphs, lines and words, a line holding text and no word, an
  `alternatives` span, `order` on sibling blocks, two pages),
  `an_alto_file_reads_to_the_words_boxes_and_order_it_states` (a
  `ReadingOrder` that differs from document order, an `IDNEXT` chain, a
  `HYP`, a `String` in a `ComposedBlock`, the same page in `mm10` and in
  `inch1200`, and in the v2, v3 and no namespace),
  `a_page_xml_file_reads_to_the_words_boxes_and_order_it_states` (polygon
  `Coords`, a `ReadingOrder` that differs from document order, lines ordered
  by `index`, a `TextLine` with no `Word`, the same page under 2013-07-15 and
  with `Point` children under 2010-03-19),
  `confidences_reach_the_models_scale`,
  `each_tolerated_defect_is_a_named_warning` (one case per row above, the word
  cap included), `a_file_that_is_none_of_the_three_is_refused_by_name`,
  `turns_and_directions_reach_the_line_settings`, and
  `arbitrary_bytes_never_panic`, a proptest over arbitrary bytes and the three
  files mutated and cut short, holding every import that succeeds to what the
  model promises and to the same answer twice.
- `ocr_import.rs` holds the layer to the same files, written over pages it
  builds, saved incrementally, reopened and held to the strict validator:
  `the_layer_extracts_as_the_imported_text_in_the_imported_order` (each
  format's page over a page with no text, and over one with text of its own,
  whose lines come first), `every_word_s_box_equals_the_file_s` (the union of
  each word's character quads, through `Page::word_boxes` and back through the
  frame, within a thousandth of the file's unit on every edge, at `/Rotate`
  0, 90, 180 and 270 with a crop box offset from the media box, and onto a
  rectangle a caller names), `the_page_renders_to_the_same_pixels_as_before`
  (byte for byte, no warning the page did not raise),
  `a_turned_line_extracts_as_one_line` (each quarter turn),
  `a_right_to_left_line_extracts_as_imported`,
  `a_word_without_area_is_skipped_and_named` and `a_layer_is_refused_by_name`.
- `src/ocr/layer.rs`'s unit tests: the program a font this engine reads,
  whose whole sums to `head`'s magic; numbers to six decimals; the nearest
  quarter turn; a fitted word filling its box at every turn; the
  `/ToUnicode` in blocks of a hundred, a character past the BMP as a
  surrogate pair; the frame's inverse; and characters numbered as first
  drawn.
- `src/ocr/import.rs`'s unit tests: the `title` grammar, a word's text cleaned
  and its removed characters named, hOCR's `order` over sibling groups, a
  stated reading order placing each block once, and the turn's range.
- `fuzz/fuzz_targets/ocr_import.rs`, seeded with the three files under
  `fuzz/corpus/ocr_import/`, which
  `the_fuzz_seeds_are_the_files_these_tests_read` holds to the tests' files;
  its memory limits are `fuzz/limits.tsv`'s row, derived in
  `bounds_ledger.rs`.
