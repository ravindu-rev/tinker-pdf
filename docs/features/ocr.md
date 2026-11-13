# OCR

The OCR tier's facade half. What exists is the first half of its import seam
(ROADMAP row OC-01): the words an outside recogniser wrote in hOCR, ALTO or
PAGE XML, read into FT-24's word-box model ([word boxes](../design/word-boxes.md))
by `tinker_pdf::ocr::import`. The design, the pipeline the tier builds around
this seam and what adjudicates it are [design/ocr.md](../design/ocr.md).

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
- `src/ocr/import.rs`'s unit tests: the `title` grammar, a word's text cleaned
  and its removed characters named, hOCR's `order` over sibling groups, a
  stated reading order placing each block once, and the turn's range.
- `fuzz/fuzz_targets/ocr_import.rs`, seeded with the three files under
  `fuzz/corpus/ocr_import/`, which
  `the_fuzz_seeds_are_the_files_these_tests_read` holds to the tests' files;
  its memory limits are `fuzz/limits.tsv`'s row, derived in
  `bounds_ledger.rs`.
