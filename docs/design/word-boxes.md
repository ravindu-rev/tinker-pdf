# Word boxes: one model, five formats

A page's words with their boxes, in one model that born-digital text fills
from `Page::text` and a recogniser (ROADMAP row OC-10) fills with no page at
all, written in the five formats consumers of word-level layout read: TSV,
bbox HTML, hOCR, ALTO v4 and PAGE XML. Each format is a mapping of the model,
and each mapping is held to the format's published definition — a column
list, an element and attribute list, a class and property list, or an XML
schema — by a first-party test (ROADMAP row FT-24). Two of the five are
defined only by another tool's published documentation of its output; that
documentation is transcribed below, named by the format's role, with its
sources and the day each was read in [THIRDPARTY.md](../../THIRDPARTY.md)'s
*The word-box formats' published definitions* (ruling 21).

## Scope

- **The model**, `tinker_pdf_content::word_boxes`: a document of pages, each
  of blocks, each of paragraphs, each of lines, each of words; every level
  with a box, a word with its text and an optional confidence. Plain data
  with public fields, so a caller builds one without a page.
- **The born-digital fill**, `tinker_pdf::word_boxes`: `Page::word_boxes`
  and `Document::word_boxes` carry `Page::text` into the model's frame.
- **Five writers** over the model, beside `serialize.rs`: TSV (FT-24.3),
  bbox HTML and hOCR (FT-24.4), ALTO v4 and PAGE XML (FT-24.5).
- **The schema harness** (FT-24.2): the ALTO and PAGE XML schemas fetched and
  pinned, and a test-side validator of the XML Schema constructs they use.
- **Determinism** (FT-24.5) and **speed** (FT-24.6): the five formats
  fingerprinted on every target, and a bench operation.

## Non-goals

- **Recognition.** Nothing here reads a scan. The model is the seam OC-10's
  recogniser fills, and OC-01.2 reads hOCR, ALTO and PAGE XML from an
  outside recogniser into it; this design writes the formats and reads none.
- **Characters and glyphs.** The model's smallest unit is the word: hOCR's
  `x_bboxes` and `ocr_cinfo`, ALTO's `Glyph` and PAGE's `Glyph` are not
  written. `TextPage` keeps every character's quad for a caller that needs
  one.
- **Styles, fonts and languages.** ALTO's `Styles`, hOCR's `x_font` and
  `x_fsize`, and the formats' language attributes: a born-digital page says
  its fonts through `TextWriter`'s JSON, XML and HTML, and nothing here
  guesses a language.
- **Regions that are not text.** Images, tables, separators and graphics
  are not in the model; hOCR's floats, ALTO's `Illustration` and
  `GraphicalElement` and PAGE's non-text regions are not written.
- **Other versions.** ALTO 4.4 and PAGE XML 2019-07-15 only. PAGE XML's
  2024-07-15 release declares another namespace, which a reader of the
  2019 one refuses, and the earlier versions of both are not written.
- **A reading order beyond the model's.** The writers write the model's
  order; ALTO's and PAGE's `ReadingOrder` elements are not written, since
  element order already is the order.

## Design

### The model

```text
WordBoxes                     created, modified (PAGE XML's two dates)
└ WordPage                    index, width, height, resolution, image
  └ WordBlock                 quad
    └ WordParagraph           quad
      └ WordLine              quad, direction
        └ WordBox             text, quad, confidence
```

- `WordPage::index` is the page's zero-based index in its document;
  `width` and `height` the page's size in the frame's pixels, whole numbers;
  `resolution` the frame's pixels per inch; `image` the name of the image
  the boxes stand on, `None` for a born-digital page.
- Every box is a `tinker_pdf_content::Quad` **in the frame below**: four
  corners, upper-left, upper-right, lower-left and lower-right as the text
  reads, so a turned word keeps its turned box. A recogniser's axis-aligned
  rectangle is the quad of its four corners.
- `WordLine::direction` is `LeftToRight`, `RightToLeft` or `TopToBottom`;
  a line's words are in reading order, which for a line `Page::text` reads
  is logical order (ruling 14).
- `WordBox::confidence` is `Option<f32>`, a percentage from 0 to 100, higher
  meaning surer. `None` is a word whose text is not a recognition result —
  every word of a born-digital page, whose text the page states. A value
  outside 0 to 100 is written clamped into it, and one that is not a number
  is written as `None`.
- `WordBoxes::created` and `modified` are dates and times in XML Schema's
  `dateTime` form, written only by PAGE XML, which requires both.
- Each writer is a method of `WordBoxes` returning its document as a
  `String` — `tsv`, `bbox_html`, `hocr` and `alto` — and `page_xml`, which
  returns one document a page.

### The frame

The boxes are where a reader sees the words: the page **as displayed** —
its crop box, `/Rotate` applied (7.7.3.3) — with the origin at the top-left
corner, `x` running right and `y` running down, in pixels of
`1 / resolution` inch. That is a render's frame: the transform is
`tinker_pdf_render::page_view_transform(crop_box, rotation, scale)` at the
scale `Page::render` uses for `RenderOptions::scale = resolution / 72`, its
clamp for an enormous page included, and the page's `width` and `height` are
`Page::pixel_size` at that scale. So the boxes land on the pixels of a render
at the same resolution, which is how a recogniser's boxes stand on its scan.
`WordBoxOptions::resolution` is 72 by default, which makes a pixel a point; a
value that is not a finite positive number is read as 72.

The facade carries every quad into the frame corner by corner, so a corner
named upper-left in the text's own direction stays so. `Quad::bounds` in the
frame is `(left, top, right, bottom)`.

### What a paragraph is

A born-digital page has blocks and lines and no paragraphs: `TextPage` joins
lines into a block by their geometry and says nothing about indents. Until
FT-17's first-line-indent breaks exist, **a paragraph is its block**: each
block of the fill holds one paragraph whose quad is the block's. A recogniser
that finds paragraphs fills several.

### The fill from `Page::text`

`Page::word_boxes(&WordBoxOptions)` returns the page's `WordPage`. It reads
`Page::text` — so its lines are in logical order and its stated text stands
in place of its glyphs — and makes one block per `TextBlock` and one line per
`TextLine`, in their order.

**A word is one of `TextLine::words` with the punctuation it touches.** The
published TSV's words carry their punctuation — `(quick)`, `jumps!`,
`$43,456.78` — and `TextLine::words` returns the UAX #29 words and drops the
punctuation between them, so a page of boxes made of those alone would lose
every comma and full stop. Each of `TextLine::words` is a word here, and the
line's characters that are in no word and are not white space — punctuation
and symbols — join the word before them where no white space stands between,
and otherwise the word after them where none does; a run of them with white
space or a line's end on both sides is a word of its own. So every character
but white space is in a word, `jumps!` and `(quick)` are one word each, and a
run of ideographs stays one word an ideograph, as `TextLine::words` breaks it.
A word's quad is the union of its characters' quads in the frame of the
line's own baseline — `TextLine::words`' own union, `words.rs`'s
`oriented_union` — carried into the frame; its text is the characters' text
in line order.

A word whose box does not meet the page — no part of it inside
`0..width` by `0..height` — is text no reader of the page sees, and is left
out; a line left with no word, and a block left with no line, go with it. A
line's direction is `TopToBottom` for a vertical writing mode, and otherwise
`RightToLeft` where `TextLine::rtl` says the line reads right to left. No
word has a confidence.

`Document::word_boxes(&WordBoxOptions)` returns a `WordBoxes` of every page,
with `created` and `modified` from the document information dictionary's
`/CreationDate` and `/ModDate` (14.3.3, dates as 7.9.4 writes them), each
standing for the other where only one reads as a date.

### From a quad to each format's box

Every format but PAGE XML writes an axis-aligned box, and every box is
computed one way:

1. **The enclosing rectangle** of the quad's four corners in the frame,
   which is the word's box for an upright word and the smallest upright box
   holding a turned one.
2. **Clipped to the page**, `0..width` by `0..height`: the formats describe
   boxes on a page's image, and part of a box past its edge is not on it.
3. **Rounded outward** where the format's numbers are whole — the left and
   top down, the right and bottom up — so the box written still holds the
   word. The page's edges are whole numbers, so the box stays on the page.

| Format | Box | A turned line |
| --- | --- | --- |
| TSV | `left`, `top`, `width`, `height`, whole, the rectangle above | nothing: the format has no column for it |
| bbox HTML | `xMin`, `yMin`, `xMax`, `yMax`, six decimals, clipped and not rounded | nothing: the published definition has no attribute for it |
| hOCR | `bbox x0 y0 x1 y1`, whole | the line's `textangle` |
| ALTO | `HPOS`, `VPOS`, `WIDTH`, `HEIGHT`, whole | a `Shape` holding a `Polygon` of the quad, on the line and on each of its words |
| PAGE XML | `Coords points`, the quad's four corners | the polygon is already the quad |

A line is **turned** when its baseline — the quad's lower-left to lower-right
edge — is not horizontal in the frame to two decimal places of a degree:
`textangle` is that edge's angle, counter-clockwise as the page is seen, in
degrees from 0 up to 360, two decimals, computed with `tinker_pdf_math`'s
`atan2` (ruling 4). A vertical line is not a turn and carries none. A polygon
is the quad's corners in the order upper-left, upper-right, lower-right,
lower-left, each rounded to the nearest whole number and clamped into the
page, written `x,y` and separated by single spaces — PAGE XML's points
grammar, and the form ALTO's `PointsType` documentation recommends.

### Confidence, and its absence

| Format | Present | Absent |
| --- | --- | --- |
| TSV | `conf`, six decimals, `0.000000` to `100.000000` | `-1`, the published definition's own value for a row with none |
| bbox HTML | not written: the published definition has no attribute for it | nothing |
| hOCR | `x_wconf`, the percentage to two decimals (4.21: higher is surer, a percentage where possible) | no `x_wconf` |
| ALTO | `WC` on the `String`, the percentage over 100, four decimals (`WCType`, 0 to 1) | no `WC` |
| PAGE XML | `conf` on the word's `TextEquiv`, over 100, four decimals (`ConfSimpleType`, 0 to 1) | no `conf` |

Only words carry one: a line, a paragraph and a block are boxes the page or
the recogniser drew round words, not readings.

### TSV

**Published definition** (transcribed; sources in THIRDPARTY.md). The OCR
TSV is a tab-separated table whose header line is these twelve names, in
this order, one tab between each two:

```text
level page_num block_num par_num line_num word_num left top width height conf text
```

and whose rows the published example shows one per page, block, paragraph,
line and word, in that nesting, the level written first: `1` a page, `2` a
block, `3` a paragraph, `4` a line, `5` a word. Each `_num` column numbers its
level from 1 within its parent and is 0 at the levels above it — the example
restarts `word_num` at each line — so a page row is `1 1 0 0 0 0` and a word
row `5 1 1 1 2 1`. `left`, `top`, `width` and `height` are whole pixels from
the image's top-left corner, the page row's `0 0 640 500` the whole image.
`conf` is `-1` on every row but a word's, and a word's six-decimal number
(`96.063751`). `text` is the word, and empty on every other row. The manual
adds that each recognised word is one row with its bounding box, confidence
and text. The encoding is the `text/tab-separated-values` media type's: one
record a line, fields separated by a tab, the first line the field names,
every record the same number of fields, and no field holding a tab.

**Mapping.** One page row per `WordPage`, `page_num` its index plus one;
then each block's row, each of its paragraphs' rows, each paragraph's lines'
rows and each line's words' rows, depth first in the model's order. A row
that is not a word's writes `-1` and an empty `text`, so it ends in a tab and
every record has twelve fields. Lines end in a line feed. A word's text has
every character that would end a field or a record — tab, line feed,
vertical tab, form feed, carriage return, U+0085, U+2028 and U+2029 — written
as a space, so the word stays in its row.

### bbox HTML

**Published definition** (transcribed; sources in THIRDPARTY.md). The bbox
HTML of a PDF text converter is "an XHTML file containing bounding box
information for each word in the file", and with layout "for each block,
line, and word in the file"; the boxes are the media box's, or the crop box's
when asked; the resolution is 72 dpi unless another is given. The published
sample of its output is one `word` element a word, its text the element's
content and its box four attributes, `xMin`, `yMin`, `xMax` and `yMax`, each
written with six decimals, `y` growing down the page. The published
definition names no other element and no attribute of the levels above the
word.

**Mapping.** An XHTML 1.0 document (the W3C Recommendation, Transitional):
the XML declaration, the Transitional document type, an `html` element in the
XHTML namespace, a `head` holding an empty `title` and a `Content-Type`
`meta` naming UTF-8, and a `body` holding the boxes as elements named by the
definition's own words: one `page` element a page, with `width` and `height`
attributes, the page's size in the frame written as the boxes are; in it one
`block` a block; in that one `line` a line; in that one `word` a word, its
text the content. `block`, `line` and `word` carry the four box attributes.
The format has no paragraph
level, so a block's lines are its paragraphs' lines in order; a born-digital
block has one paragraph and loses nothing. Text and attribute values are
escaped as `serialize.rs` escapes its HTML.

### hOCR

**Published definition**: *hOCR — OCR Workflow and Output embedded in HTML*,
version 1.2, <https://kba.github.io/hocr-spec/1.2/>. An element is an HTML
element whose `class` holds exactly one name starting `ocr_` or `ocrx_`
(2.2.1); its properties are `key value` pairs separated by `;` in its
`title`, which serves no other purpose (2.2.2, 2.4). The classes written:

| Class | Element | Properties (spec) | Written |
| --- | --- | --- | --- |
| `ocr_page` | `div` | `bbox` required; `image`, `imagemd5`, `ppageno`, `lpageno` recommended (3.1.1) | `bbox 0 0 width height`, `ppageno` the page's index (4.9: the front cover is page 0), `image` where the model names one |
| `ocr_carea` | `div` | `bbox` required; in reading order (3.1.3) | `bbox` |
| `ocr_par` | `p`, as 3.3.5 recommends | `bbox` is a general property (4.2) | `bbox` |
| `ocr_line` | `span` (3.1.4: "should be in a span") | `bbox` required; `baseline` and others allowed | `bbox`, `textangle` where the line is turned |
| `ocrx_word` | `span` | "any kind of word returned by an OCR system" (3.5.3) | `bbox`, `x_wconf` where the word has a confidence |

`bbox` is four whole numbers, left, top, right and bottom, from the image's
top-left corner in pixels (4.2); `textangle` a number of degrees by which the
text is turned counter-clockwise (4.14), which 3.1.4 does not list among a
line's allowed properties and which 4.14 defines for exactly what a line is,
textual content — this mapping reads 4.14 and writes it on the line;
`x_wconf` a number, higher surer, a percentage where possible (4.21). The
head carries exactly one `ocr-system` and one `ocr-capabilities` `meta`
(6), and `ocr-number-of-pages`. Writing direction is HTML's `dir` attribute
(5.3), declared by the `ocrp_dir` capability (6.2); a capability lists what
the producer can generate, whether or not one document holds it, and its
absence forbids the markup (6.2).

**Mapping.** An XHTML 1.0 document as bbox HTML's, its head adding
`ocr-system` `tinker-pdf word-boxes/1` — the producer and the version of this
mapping, which a release does not move — `ocr-capabilities`
`ocr_page ocr_carea ocr_par ocr_line ocrx_word ocrp_dir ocrp_wconf` (the
last after 6.2's `ocrp_` rule for the property 4.21 defines), and
`ocr-number-of-pages`. Each element has an `id` built from the TSV numbering —
`page_1`, `block_1_1`, `par_1_1_1`, `line_1_1_1_1`, `word_1_1_1_1_1` — so the
two formats name one word alike. A right-to-left line carries `dir="rtl"`.
Words in a line are separated by one space and lines by a line feed, so the
document's text reads as the page's. Text and attribute values are escaped as
`serialize.rs` escapes its HTML.

### ALTO v4

**Published definition**: the ALTO schema, version **4.4**, namespace
`http://www.loc.gov/standards/alto/ns-v4#`, at
<https://www.loc.gov/standards/alto/v4/alto-4-4.xsd>, SHA-256
`5564fd29d2dd090d8102b8a0aa081906afd677cd5ecc632312e56f21ea14702b`. It
imports the XLink attributes from
<http://www.loc.gov/standards/xlink/xlink.xsd>, SHA-256
`f1f5bb6003165cdd8f6c1fcc32f8fd1f965e1681010f3b9806d9460bcffa8a3c`, which the
validator loads with it.

**Mapping.**

```text
alto                 SCHEMAVERSION="4.4", xsi:schemaLocation naming the schema above
├ Description
│ └ MeasurementUnit  pixel
└ Layout
  └ Page             ID page_n, PHYSICAL_IMG_NR the index plus one, WIDTH, HEIGHT
    └ PrintSpace     HPOS 0, VPOS 0, WIDTH, HEIGHT
      └ ComposedBlock  a block of two paragraphs or more, ID block_…
        └ TextBlock    a paragraph, ID par_…; a block of one paragraph is that TextBlock alone
          └ TextLine   ID line_…, BASEDIRECTION rtl or ttb where not left to right
            ├ String   ID word_…, CONTENT, WC
            └ SP       between two words
```

Every block, line and word carries `HPOS`, `VPOS`, `WIDTH` and `HEIGHT`, a
turned line and its words a `Shape` (above). ALTO has no paragraph element
and a `TextBlock` is its block of text, so a paragraph is a `TextBlock` and a
block of several is a `ComposedBlock` holding theirs, which loses nothing;
a born-digital block, of one paragraph, is a `TextBlock` in the `PrintSpace`.
A model holds one image per page and ALTO's `sourceImageInformation` one for
the document, so none is written. Escaped as `serialize.rs` escapes its XML.

### PAGE XML

**Published definition**: the PAGE content schema, version **2019-07-15**,
namespace `http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15`,
at
<https://www.primaresearch.org/schema/PAGE/gts/pagecontent/2019-07-15/pagecontent.xsd>,
SHA-256 `5d7da5af5f5e06d3b9cd1e78b407ffca1862f78ad9823ed89c302fb6409932d5`.
A `PcGts` holds exactly one `Page`, so a document is one PAGE XML file a page.

**Mapping.**

```text
PcGts                xsi:schemaLocation naming the schema above
├ Metadata
│ ├ Creator          tinker-pdf word-boxes/1
│ ├ Created          WordBoxes::created
│ └ LastChange       WordBoxes::modified
└ Page               imageFilename, imageWidth, imageHeight, imageXResolution, imageYResolution, imageResolutionUnit PPI
  └ TextRegion       a block of two paragraphs or more, id block_…
    └ TextRegion     a paragraph, id par_…; a block of one paragraph is that region alone
      ├ Coords
      └ TextLine     id line_…, readingDirection where not left to right
        ├ Coords
        ├ Word       id word_…
        │ ├ Coords
        │ └ TextEquiv  conf; Unicode the word's text
        └ TextEquiv    Unicode the line's words joined by spaces
```

`Created` and `LastChange` are required, and a time the writer read off a
clock would differ on every run (ruling 4), so they are the model's; a model
with neither is written `1970-01-01T00:00:00Z` for both. `imageFilename` is
required too, and is the page's image name or empty, which is a string the
schema admits. A region's `TextEquiv` is not written: its lines carry the
text. Escaped as `serialize.rs` escapes its XML.

### Escaping

TSV writes its eight record and field breakers as spaces and nothing else
needs it. bbox HTML and hOCR are XHTML, escaped as `serialize.rs`'s HTML:
`&`, `<`, `>`, `"` and `'` as references, and every control character but tab
and line feed, and every noncharacter, as U+FFFD — so each is both
well-formed XML and a document an HTML parser reads. ALTO and PAGE XML
are XML, escaped as `serialize.rs`'s XML, the characters XML 1.0 cannot carry
written as U+FFFD.

### The schemas' licences, and the fetch

Nothing vendored here, and the harness fetches all three files into
`target/` and checks each against its SHA-256 above (FT-24.2), as
`crates/tinker-pdf/tests/commonmark/fetch-spec.sh` does: ALTO 4.4's schema
names Creative Commons Attribution-ShareAlike 4.0, which `deny.toml` does not
admit, beside a waiver that names no licence (ruling 18); PAGE XML's schema
as served at its namespace states none, and the copies in the repository that
publishes its releases under Apache-2.0 differ from it in their bytes; the
XLink schema states none.

### The XML Schema constructs the validator implements

Read from the three files, against *XML Schema Part 1: Structures* and
*Part 2: Datatypes* (W3C Recommendations, Second Edition):

- **Schemas**: `schema` with `targetNamespace`, `elementFormDefault`
  (`qualified`) and `attributeFormDefault` (`unqualified`); `import` of
  another namespace (ALTO's XLink); `annotation` and `documentation`, read
  past.
- **Elements**: global declarations (one root each, `alto` and `PcGts`) and
  local ones, with `name`, `type`, `minOccurs` and `maxOccurs` (a number or
  `unbounded`), or an anonymous complex type.
- **Complex types**: named and anonymous; `mixed="false"`; `abstract`, on
  types only ever extended, so no instance needs `xsi:type`.
- **Content models**: `sequence` and `choice`, nested, each with
  `minOccurs` and `maxOccurs`; named model groups and `group ref`; the
  wildcard `any` with `namespace="##any"` and `processContents="lax"`.
- **Attributes**: `attribute` with `name`, `type`, `use` (`optional`,
  `required`), an anonymous simple type, `ref` to a global declaration,
  `fixed` and `form="qualified"`; `attributeGroup` definitions and
  references, across namespaces; `xsi:schemaLocation` on any element.
- **Derivation**: `complexContent` with `extension` (the base's content, then
  the extension's, and both sets of attributes) and `simpleContent` with
  `extension`.
- **Simple types**: named and anonymous; `restriction` with `enumeration`,
  `pattern` (PAGE's one, `([0-9]+,[0-9]+ )+([0-9]+,[0-9]+)`, in the subset
  of the regular-expression language it needs: character-class ranges,
  literals, groups and `+`), `length` with `fixed`, `minLength`, `maxLength`,
  `minInclusive`, `maxInclusive` and `whiteSpace`; `list`, by `itemType` or an
  anonymous item type; `union` by `memberTypes`.
- **Built-in types**: `string`, `boolean`, `float`, `int`, `integer`, `date`,
  `dateTime`, `gYear`, `gYearMonth`, `anyURI`, `language`, `hexBinary`, `ID`,
  `IDREF` and `IDREFS`, with an `ID` unique in its document and each `IDREF`
  naming one, and each type's own white-space rule.

None of the three uses identity constraints, substitution groups, `all`,
`anyAttribute`, `include`, `redefine`, element defaults or fixed values,
`nillable` or complex restriction, and the validator does not implement
them; a schema construct outside the list is refused by name, so a schema
that grows one fails to load rather than validating less.

### Determinism

Each writer is a function of the model and nothing else: no clock, no
environment, no hash-map order, and no version string a release moves. Numbers
are formatted by `core`'s formatting, which is exact on every target, and the
one transcendental, `textangle`'s angle, is `tinker_pdf_math`'s (ruling 4).

## Milestones

1. **The schema harness** (FT-24.2). `crates/tinker-pdf/tests/word-box-schemas/fetch.sh`
   fetches the three files above into `target/` and checks their SHA-256; a
   test-side validator implements the constructs above over documents read
   with `tinker-pdf-xml`. Exit: `crates/tinker-pdf/tests/word_box_schemas.rs`'s
   `the_validator_holds_each_construct` always runs, a hand-written schema
   and documents per construct, each with a document it must reject;
   `the_published_schemas_load` prints `word-box-schemas: RAN` once both
   fetched schemas are read into the validator, and `SKIPPED` without them; a
   local run of the fetch and the test, a skip counting as a failure, prints
   `RAN`.
2. **The model and TSV** (FT-24.3). Exit: `crates/tinker-pdf/tests/word_boxes.rs`'s
   `tsv_holds_its_published_column_definition`, re-read in the test against
   the transcription above (header, rows, numbering and boxes, every word's
   box the one derived above from its line's quads);
   `a_model_filled_without_a_page_writes_tsv`;
   `a_word_holding_a_tab_or_line_break_stays_in_its_row`; on rotated and
   right-to-left pages, in the default and `bundled-fonts` builds.
3. **bbox HTML and hOCR** (FT-24.4). Exit: `word_boxes.rs`'s
   `bbox_html_holds_its_published_elements` and
   `hocr_holds_its_published_classes_and_properties`, each read back with
   `tinker_pdf_xml::html::parse` and held to the transcription above with every
   word's box the model's, and `a_hostile_word_stays_inside_its_markup` for
   both; on rotated and right-to-left pages, in both builds.
4. **ALTO and PAGE XML, and the fingerprints** (FT-24.5). Exit:
   `word_box_schemas.rs`'s `alto_output_validates_against_its_published_schema`
   and `page_xml_output_validates_against_its_published_schema`, over
   born-digital pages and a model filled directly, print `RAN` in a local run
   with the schemas fetched; `word_boxes.rs`'s
   `alto_and_page_xml_carry_every_word_s_box` runs everywhere, in both
   builds; `determinism.rs`'s `word_box_formats_are_stable_across_targets`
   passes under `cargo test` on Linux and Windows and under
   `--target wasm32-wasip1`.
5. **Speed** (FT-24.6). Exit: `cargo xtask baseline` holds the operation —
   the five formats written for 100 dense pages' prebuilt text — in MS-04's
   text group with the bench file and `baseline.json` agreeing
   (`xtask/src/bench.rs`'s `the_committed_baseline_agrees_with_the_tree`), and
   `cargo bench -p tinker-pdf` reports its time.

## Risks

| Risk | Mitigation |
| --- | --- |
| **bbox HTML's published definition is thin**: two sentences of a manual and a published sample name the word element and its four attributes, and nothing names an element above the word | The vocabulary is the definition's own words, `page`, `block`, `line`, `word`; a consumer written against details nothing publishes does not find them, and this design says so rather than guessing at them |
| A consumer filters TSV words on `conf` and drops born-digital words written `-1` | `-1` is the published definition's value for a row with no confidence, and a born-digital word has none; writing 100 would claim a recognition that never happened |
| A schema is republished in place: PAGE XML's 2019 revision is published in patched releases | The SHA-256 pins fail the fetch loudly, and the harness's test is red rather than validating against another file |
| `textangle` on a line reads past 3.1.4's list of a line's properties | 4.14 defines it for textual content; the reading is stated above, and a consumer that ignores it still has the line's `bbox` |
| A word partly off the page is written with a smaller box than its glyphs | The formats describe boxes on a page's image, and the part off the page is on no image; the model keeps the whole quad for a caller that wants it |
| Joining punctuation to words parts from `TextLine::words`, which other outputs use | The words here are the formats' words, as the published TSV writes them; `TextLine::words` is unchanged, and the joining is in this model alone |
