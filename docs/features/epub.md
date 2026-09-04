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
from the referring document (§4.2.5) — four package locations in the real
corpus, `EPUB/`, `OEBPS/`, `OPS/` and the root.

**The package** (`package`, `nav`). OPF 2.0 and 3.x; manifest, spine with
`linear` and per-item `rendition:layout`, fallbacks (depth-bounded),
`properties` (`nav`, `cover-image`, `svg`, `mathml`, `scripted`), metadata.
The navigation document or the NCX — one producer's EPUB 3 has no NCX and
its EPUB 2 has no nav; another's EPUB 3 has both — becomes the PDF outline.
Obfuscated fonts (`obfuscation`) are de-obfuscated by the IDPF and Adobe
algorithms.

**The content document** (`xhtml`). Parsed by `tinker-pdf-xml` in its
doctype mode, because pandoc writes `<!DOCTYPE html>` on every document and
calibre writes none; the internal subset — where every entity bomb lives —
stays refused by name under both modes.

**The CSS engine** (`tinker-pdf-css`, a leaf crate): a `css-syntax-3`
tokenizer with the spec's normative error recovery; `selectors-4` matching
and specificity, with **every pseudo-class a static document can decide
actually decided** — `:nth-child()` and its three relatives over §6.6.2's
whole `An+B` grammar, the `of-type` family, `:empty`, `:has()` (a relative
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
the previous **layer**; `@media` evaluated
against a `MediaContext`; `@import` with cycle and depth bounds;
`@font-face` descriptors collected into a `FaceSet`; `@layer` in all three
syntaxes — block, statement and anonymous — with dotted and nested names
resolving to one layer, first mention fixing a layer's position, the order
kept per origin, and **`!important` reversing it**, so an important
declaration in the first layer beats an important one in the last. Around fifty computed
properties reach `ComputedStyle`. **A property parsed with no consumer does
not compile** — `tinker-pdf-css/tests/unimplemented_property_does_not_build.rs`
and `tinker-pdf-layout/tests/uncascaded_field_does_not_build.rs` make a
silently-ignored property a build failure rather than a rendering surprise.

**The layout engine** (`tinker-pdf-layout`, a leaf crate whose input is a
caller-built tree rather than bytes): CSS 2.2's box model and the three
margin-collapsing cases of §8.3.1; block and inline formatting contexts
(§9.4); all nine float rules of §9.5, each its own step and fixture; the
§17 table model with §17.2.1's anonymous-box generation, §17.5.2.2's
two-pass automatic width and §17.6.2.1's five-rule border conflict
resolution, `rowspan` clamped; `css-flexbox-1`'s line algorithm with
grow/shrink freeze-and-redistribute and `order`; §13.3 fragmentation rules
A–D with the spec's own escape; `page-break-*`, `orphans`, `widows`. Line
breaking is **UAX #14** over vendored Unicode 17.0.0 data, passing
**19 338 of 19 338** pairs of Unicode's own `LineBreakTest.txt`. Advances
come from the face (embedded, host-provided, or the standard-14 metrics for
an unembedded family), one per character.

**Painting** (`paint`, `typeface`). An embedded face's run is shaped whole and
written through `DocumentBuilder::glyph_run`, which states **every glyph's own
position** — so `GPOS`'s offsets reach the page and a mark sits at its anchor
rather than at its advance, as 9.4.3's `TJ` adjustments and `Ts`.
`letter-spacing` is folded into those positions and `0 Tc` written, because a
reader applies `Tc` per glyph while layout measures it per character, and a
joined word would otherwise be drawn narrower than the box it was measured
into. A run in one of the standard 14 is unshaped and one glyph per character,
and keeps `PageBuilder::glyphs`. Faces are subset to what the book draws; every
run that could not be represented is counted (`UnrepresentedCharacters`,
`UncoveredCharacters`), and a run the writer refused is
`UnwritableTextRun`. Images, borders, backgrounds, list markers and links are
drawn; every internal link and every navigation entry becomes a link
annotation or outline item.

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

## Tagged output

The PDF an EPUB becomes carries a **structure tree**: `/MarkInfo << /Marked
true >>`, a `/StructTreeRoot`, a `/ParentTree`, `/StructParents` on every page
that has content, and a `BDC`/`EMC` pair with an `/MCID` around every run. The
logical order is the **source document's**: each run carries the index of the
element that wrote it, so the tree is the XHTML tree and not a description of
the page.

XHTML names become ISO 32000 Table 333's standard types — `p` to `/P`, `h1` to
`/H1`, `ul` to `/L`, `li` to `/LI`, `table`/`tr`/`th`/`td` to `/Table`/`/TR`/
`/TH`/`/TD`, `section` and `article` to `/Sect`, anything else block-level to
`/Div` and anything else to `/Span`. A list marker is an `/Artifact` (§14.8.2.2)
and stays out of the tree, which is what keeps text conservation an equality.

**Verified against the source and not against this engine.** Reading our own
output back through our own reader proves the two halves agree, not that either
is right, so the load-bearing assertion in `epub_structure.rs` compares the
tree's logical order against the book's own XHTML: every character, in source
order, at nine page boxes.

**What this first pass does not do**, each named rather than absent:

| Not done | Why |
| --- | --- |
| A PDF/UA conformance claim | a structure tree is necessary for it and nowhere near sufficient |
| `/Alt` on images | `PageBuilder` cannot write one, so an `<img alt="…">` becomes a `/Figure` and its alternate text is dropped |
| `/Lang`, per element or on the catalog | not written |
| A `/RoleMap` | not needed: every tag emitted is already a standard type. The cost is that the XHTML name is not recoverable — `<em>` and `<strong>` are both `/Span` |
| `<a>` as a `/Link` | §14.8.4.4.2 wants an `/OBJR` for the annotation and this writer cannot emit one; a bare `/Link` would claim an association the file does not contain. It is a `/Span`, and the annotation itself is still written |
| Table `/Headers`, `/Scope`, `/Summary` | a `<th>` is a `/TH` with no association to the cells it heads |

## Refused by name

| What | Typed variant | Why | See |
| --- | --- | --- | --- |
| Inside an SVG content document: `<filter>`, `<mask>`, `<pattern>` as a paint, `<marker>`, `<foreignObject>`, SMIL animation, `<script>`, `<textPath>`/`<tref>`/`<altGlyph>`, and `spreadMethod` other than `pad` | `ArchiveWarning::Svg { item, warning }` | the document draws; each of these is a subsystem this build declines, named per document and deduplicated by the crate that met it. §14.5's group `opacity` is **flattened** into each descendant's alpha — exact for a shape painted one way, too dark where a fill and a stroke overlap, so `GroupOpacityFlattened` fires only where it shows | [design/svg.md](../design/svg.md) |
| An `<image>` inside an SVG whose reference does not resolve, or whose bytes are neither JPEG nor PNG | `ArchiveWarning::SvgImageUnresolved { item, images }` | those two are embedded through the same `ImageData` path `cbz.rs` uses; anything else is counted per page rather than drawn as nothing | [design/svg.md](../design/svg.md) |
| An SVG content document that produced no picture at all | `SpineDefect::SvgUnreadable(tinker_pdf_svg::Refusal)` | six named causes — not XML, not an `<svg>` root, a `<use>` that reaches its own ancestor, or one of four ceilings — and the refusal travels, so a caller can tell a bomb from a truncated file | [design/svg.md](../design/svg.md) |
| `position: fixed` — **not a refusal, a stated answer** | — | CSS 2.2 §9.6.1: *"in the case of paged media, fixed boxes are repeated on every page, and are fixed with respect to the page box"*. So a fixed box is positioned against the page box and drawn on every page of the document. That is the specification's own paged answer, not a degradation of the screen behaviour, and it is what a stylesheet asking for a running header meant | — |
| `position: sticky` — **also a stated answer** | — | `css-position-3` §3.4: a sticky box is offset by how far its nearest scrollport has scrolled, clamped to its containing block. A paginated document has no scrollport, so that distance is zero on every page and §3.4's own words are that it is then *"the same as `relative`"*. The value of a parameter this medium does not have, rather than a gap | — |
| `column-span: all` | `tinker_pdf_layout::Warning::ColumnSpanAsNone` | `css-multicol-1` §6: a spanning box interrupts the columns and resumes them below itself, which is three column sets where this build has one. The box is laid out in the column it fell in, counted per box | [ROADMAP.md](../ROADMAP.md) |
| A single box taller than a page inside a multi-column container | `tinker_pdf_layout::Warning::ColumnTallerThanPage` | the third of the three `Abreast` shapes and the same sentence as the other two: the container is cut across pages, and what is left is one atomic box no cut can halve | [ROADMAP.md](../ROADMAP.md) |
| A `max-height` shorter than the content | `tinker_pdf_layout::Warning::MaxHeightAsAuto` | CSS 2.2 §10.7's clamp, and its two halves land differently here: `min-height` pads the flow out and `max-height` would have to shorten it, which a column whose `y` never goes backwards cannot do once the items are emitted. So the box is its content's height and the declaration is named rather than half honoured, which is this build shape for a value it honours in one direction only | [ROADMAP.md](../ROADMAP.md) |
| Other at-rules (`@supports`, `@page`, …) | `tinker_pdf_css::Warning::AtRuleUnsupported(name)` | skipped by the spec's own recovery, named | — |
| `:hover`, `:focus`, `:focus-within`, `:focus-visible`, `:active`, `:target`, `:visited` — **seven, and the whole of what never matches** | `tinker_pdf_css::Warning::PseudoClassUnsupported(name)` | each names a state of a reading *session*: a pointer, a focus ring, a press, a fragment the reader navigated to, a history. A paginated document has none of them, for any element, ever — so never matching is `selectors-4`'s **answer** here and not this build's gap. Still counted, because a rule that had no effect is something the book said (ruling 10), and the count is asserted by number so a shrinking list cannot read as a passing one | — |
| `:nth-child(An+B of S)` | `parser::Report::discarded_rules` | the only pseudo-class syntax refused outright. Reading it as the `An+B` without the `of` would style every second row instead of every second `.a`, which is a book that renders beautifully and is wrong; §3.1 drops the rule instead, counted | [ROADMAP.md](../ROADMAP.md) |
| `::first-line`, `::first-letter` | `tinker_pdf_css::Warning::PseudoElementUnsupported(name)` | parsed, no box generated. Neither is *generated* content: both select part of an **already laid out** box, so honouring either means a second layout pass, which is a different feature from `::before`. `::marker`, `::placeholder` and `::selection` are not parsed at all — `::selection` for the reason the pseudo-class row gives, that it names a state of a reading *session* and a paginated document has none | [ROADMAP.md](../ROADMAP.md) |
| `content: url()`, `counter()`, `counters()`, `open-quote` and its three siblings | `ArchiveWarning::UnimplementedProperty { property: "content", .. }` | `::before` and `::after` **do** generate boxes now, from strings, `attr()` and any concatenation of the two. These three families do not: `url()` is a replaced element needing a size before the image is fetched; `counter()` needs `counter-reset`, `counter-increment` and a scoped counter tree, and resolved to nothing would number every list item zero; the quote keywords read `quotes`, which is unimplemented and which one producer writes four times in the committed corpus — guessing `"` is wrong in every language that does not use it. Counted, so a book that asks is not quietly given an empty box | [ROADMAP.md](../ROADMAP.md) |
| A single box taller than a page **inside** a table band | `tinker_pdf_layout::Warning::TableRowTallerThanPage` | the band itself is now cut across pages (`css-break-3` §3.1's class-3 break), so what is left is one atomic box — a line box, or a nested band — that no cut can halve. It is drawn where it starts and overflows | [ROADMAP.md](../ROADMAP.md) |
| The same inside a flex line | `tinker_pdf_layout::Warning::FlexLineTallerThanPage` | same, and still two variants: a host with no table in its book must not be told a table row overflowed | [ROADMAP.md](../ROADMAP.md) |
| A float whose **content-stream** order differs from its reading order | `tinker_pdf_layout::Warning::FloatBrokenAcrossPages`, and the content-order figure pinned in `epub_fetched.rs` | **Not a defect any more: a statement about two orders, both measured.** §9.5.1 places a float by geometry, so `clear` can push its box a page past the text it was written among — and a glyph is only on the page it is drawn on. So the *content stream* of `pg16328-beowulf.epub` genuinely reads 2 182 characters out of source order, and that figure stays pinned because it is true of any extractor that ignores the structure tree, which is what §14.8 says to do when there is none. **There is one now, and in logical order the same book conserves exactly: 0 extra, 0 missing.** ISO 32000 §14.7.2 Table 323 lets a structure element's kids name different pages, so the gloss sits under its own element in source position while its marked content stays on the page its glyphs landed on, written as an `/MCR` with its own `/Pg`. Nineteen of the twenty fetched books conserve exactly in logical order; the twentieth is the one with no glyphs at all. Four earlier fixes moved nothing and are worth naming so nobody rebuilds them: moving the float's page decision, removing `css-break-3`'s push (sixty times worse), tightening the reach further, and emitting a structure tree **per page** — which reproduces page order exactly and measures 2 182 too | [ROADMAP.md](../ROADMAP.md) |
| `local()` sources | `ArchiveWarning::FontFace(FaceDefect::LocalUnavailable)` | names a face installed on the reading system, and this engine reads no font directories **by policy** — that is an operating-system dependency `wasm32-unknown-unknown` does not have ([fonts](fonts.md)). Permanent for the engine; a host with installed faces answers it through `FontProvider`. **WOFF and WOFF2 left this row**: both are unpacked, and only a container that will not unpack is refused, by `FaceDefect::PackedContainer` carrying the decoder's own reason | [fonts](fonts.md) |
| Characters no face covers | `ArchiveWarning::{UnrepresentedCharacters, UncoveredCharacters}` | counted, never silently dropped — conservation still holds for the text | [fonts](fonts.md) |
| Fonts attached after open | `ArchiveWarning::FontsAttachedAfterPagination` | advances decide line breaks, so faces must arrive in `OpenOptions` | — |
| Page box or font size the caller passed that cannot be used | `ArchiveWarning::UnusableOption(BookOptionDefect::{PageWidth, PageHeight, FontSize})` | the default is laid out instead and the caller is told which number was thrown away — a warning, not a refusal, because it is a claim about the caller, not the file | — |
| Encrypted resources, missing rootfile, unreadable package document, unsupported package version, empty spine, a book that could not be paginated | `ArchiveRefusal::{EncryptedResources, RootfileMissing, UnreadablePackageDocument, UnsupportedPackageVersion, EmptySpine, UnpaginatedBook, UnreadableContainer}` | refused at open, by name | [cbz](cbz.md) |
| Scripting, MathML layout, media overlays | `ArchiveWarning::UnimplementedFeature` | declared in `properties`, reported, content rendered as its fallback text | — |

**SVG in the spine, as built.** Tier 4's SVG lane closed the row that used to
stand here — *"placeholder page; no SVG renderer"* — and the three rows above
are what is left of it. `sample-svg-in-spine.epub`'s six spine items are six
pages that draw: an Illustrator cover of 339 gradient-filled paths under a
58-class `<style>` element, two Inkscape drawings, and three pages that are one
`<image>` and nothing else.
`the_six_svg_spine_items_draw_rather_than_placehold` renders all six and asserts
each is more than one colour, because a build that read every SVG and wrote a
blank page would satisfy every count in this file.

What that reader is, precisely: SVG 1.1's §8.3 path data, §7's transforms and
viewports, §9's seven basic shapes, §11's painting from all three of §6.4's
sources, §13.2's gradients including `xlink:href` inheritance between paint
servers, §14.3's clipping, §5.6's `<use>` with the bomb refused by name, §5.7's
`<image>`, and §10's text — the last through the same `css-fonts-4` §5.3 matcher
and the same shaper that set the rest of the book, because SVG text and XHTML
text in one book must not resolve `serif` two different ways.

**One thing that half-worked and was found by a test rather than by reading.**
`DocumentBuilder::begin_page` snapshots the document's resource set, so a
pattern, an `/ExtGState` or an image registered while *drawing* is invisible to
the page naming it: the gradient, the transparency or the photograph is silently
gone while every solid stroke still draws. The corpus test passed anyway,
because that cover strokes its paths black. `epub::svg::Registry` is the
ordering made into a type — `draw` takes a `&Registry` and cannot register
anything.

**Corpus caveats, stated**: the committed set now holds a fixed-layout book
from a real producer (KCC 11.0.1, `rendition:layout: pre-paginated`) and two
books carrying a real producer's font through the `@font-face` path
(Liberation Serif, unmodified, from calibre and from pandoc). Three things are
**still owed**. **No stylesheet in the corpus declares a cascade layer** —
`epub_css.rs` asserts the layer count is zero rather than assuming it, and the
three books added since do not change it, so `@layer` is verified against this
engine's own reading of `css-cascade-5` §6.4.2 and against no producer at all.
**No book in the corpus carries a web font.** The row above is closed and the
files behind it are real — seven of them, in
`crates/tinker-pdf-font/tests/woff/`, from three encoders with no code in
common — but they are packings of a face this repository wrote, not a book a
producer shipped. The objection that kept the row open was never that WOFF
could not be read: it was that no producer here emits one and OFL-1.1's
reserved-name clause bars repacking a vendored face, and only the second half
of that is answered. So the decoders are held against real encoders' output
and the *`@font-face` path* is held against containers this repository packs
in `epub_fonts.rs`; what nobody here has is a calibre or pandoc book with a
`.woff2` in its ZIP. And three books carry **no epubcheck verdict** — they
postdate the tool's removal under ruling 13, so
`EPUBCHECK.tsv` marks them `-` rather than zero
([ROADMAP.md](../ROADMAP.md) Tier 4).

## Verified

- `crates/tinker-pdf-css/src/tests/defaulting.rs` — **§7.1's five keywords,
  arranged around the two pairs that are easy to collapse into one.** `unset`
  against `inherit` and `initial`, asserted on an inherited and a
  non-inherited property in the same fixture because either one alone agrees
  with two of the three keywords; and `unset` asserted to be exactly *not
  declaring the property* over **all eighty-three longhands**, not a sample,
  because §7.1's definition and `ComputedStyle::inherit_from`'s behaviour are
  the same rule written twice. `revert` against `revert-layer` in one fixture
  with a user-agent rule and two author layers, where the two keywords have
  **different** answers — the only shape that can tell them apart, and the
  reason it is one fixture asserting both rather than two that could each pass
  alone. Eight counted injections, each measured by making the edit; none
  fires zero, and the pair `revert`/`revert-layer` fires 1 and 2, which is
  what says a single fixture carries that distinction.
- `crates/tinker-pdf-css/tests/unimplemented_property_does_not_build.rs` — the
  compile-time proof, which **said nothing about a value** until this landed.
  It proved a *property* with no consumer does not build; a defaulting keyword
  is a value, valid on every property, and nothing stopped one being added
  with nothing to write it down. `Longhand` and its three exhaustive consumers
  are the answer, and three new injections withhold each: the sharpest is a
  property name the cascade cannot write a defaulted value into, which is
  `inherit` parsing, cascading, winning, and doing nothing. `Longhand::ALL` is
  a list and not a `match`, so `rustc` cannot check it — `defaulting.rs`
  checks it against `IMPLEMENTED_NAMES` instead, and the proof does not
  pretend to.

- **Nine committed books from three real producers** (pandoc 3.10.2 and 3.11,
  calibre 9.13.0 and 9.14.0, KCC 11.0.1) over text authored here, in
  `crates/tinker-pdf/tests/epub/`, with epubcheck 5.3.0 verdicts recorded in
  `EPUBCHECK.tsv` for the six that predate ruling 13. Those verdicts are a
  **dated measurement and not a check** — ruling 13 ended the re-run, so when
  this engine and a book disagree there is no arbiter, and for the three books
  with no verdict at all there never was one. What the record still holds is
  that every book has a *row*, that a count is a number or an explicit `-`, and
  that the sets of books the tool was unhappy with and never saw are the ones
  recorded. The three tier-4 books closed the roadmap's fixed-layout and
  `@font-face` rows and found **eleven things** the first six could not, listed
  in `tests/epub/README.md` — including that a fixed-layout comic reaches this
  build as correctly-sized, correctly-clipped, entirely blank pages, because no
  path here paints a replaced element. **Twenty more are fetched**,
  never committed (Project Gutenberg's trademark licence and `epub3-samples`'
  CC-BY-SA are both barred by this repository's own no-copyleft gate), and
  `epub_fetched.rs` needs `TINKER_EPUB_CORPUS` set to an **absolute** path
  — a test binary runs from its crate directory, so a relative one resolves
  under `crates/tinker-pdf/` and every sweep skips while passing — and
  `TINKER_EPUB_CORPUS_REQUIRED=1` makes a skip a failure. It prints
  `epub-corpus: RAN` / `SKIPPED` so the CI job goes
  red on a skip. `INVENTORY.tsv` is recomputed through `tinker-pdf-zip` on
  every `cargo test`; `CONSERVATION.tsv` is a ratchet, re-measured by
  `epub_conservation.rs`.
- `tests/epub.rs`, `epub_ocf.rs`, `epub_package.rs`, `epub_reading.rs`,
  `epub_css.rs`, `epub_tables.rs`, `epub_fixed_layout.rs`, `epub_fonts.rs`,
  `epub_memory.rs`; the layout crate's own suite (`layout/src/tests.rs`,
  floats and tables step by step, UAX #14 conformance over the full pair
  table); the CSS crate's tokenizer, selector and cascade suites.
- `epub_fonts.rs`'s WOFF half: a book whose `@font-face` names a WOFF or a
  WOFF2 sets in **that face**, asserted on the resource the page draws with
  rather than on the absence of a warning — a book that fell back to the
  standard 14 reports nothing either, which is the failure this replaces. The
  WOFF 1.0 side is packed in the test itself, every table **stored** rather
  than deflated, because there is no zlib encoder in this tree and §5's own
  signal for a stored table is `compLength == origLength`; the WOFF 2.0 side
  is the committed `synthetic-2.woff2`, Brotli and transformed `glyf` and all.
  What is embedded is asserted to be the sfnt **byte for byte** and not the
  container, which is the one claim every other test here would pass without:
  9.9 gives font programs `/FontFile2` and `/FontFile3` and neither has a
  subtype for a web container, so a build that passed the WOFF through would
  still name the face on the page. Three damaged containers keep the refusal
  row and are asserted to carry the decoder's own reason.
- `epub_validated.rs`: every synthesised book is held to the strict
  validator, its pages are read at the box the caller stated, its content
  streams are decoded operator by operator so a placeholder page and a page
  that reads are told apart, and its three embedded faces keep their own
  `/W` advances. It replaced the qpdf oracle.
- `epub_analytic.rs` and `epub_reftest.rs`, which replaced the browser.
  Analytic layout sets every document in `monospace`, where Courier's
  600/1000 advance makes the line breaker a division, and **computes every
  expected number in the test**: characters to a line, baselines
  `line-height` apart, margins collapsed to the larger, padding and border on
  the content edge, `text-indent` on the first line only, a float shortening
  the lines beside it and none below, and CSS 2.2 §13.3.2's `orphans` and
  `widows` deciding the break. Reftests need no expected number at all: they
  lay out pairs the specification says are one document — a shorthand against
  its longhands, `1.5em` against `24px`, `50%` against `120px`, an implied
  `<tbody>` against an explicit one — and each pair carries a mismatch
  reference that must fail.
  **Both sides of every check here are written by the people who wrote the
  engine**, so this engine's CSS is now verified against its own reading of
  the specifications and nothing else
  ([ROADMAP](../ROADMAP.md), [verification](../verification.md)).
- The `epub` determinism fingerprint asserts stability at two page boxes
  *and* that the two differ; the synthesised book's bytes are hashed too
  ([determinism](determinism.md)).
- Compile-time enforcement: `unimplemented_property_does_not_build.rs`,
  `uncascaded_field_does_not_build.rs`.
- Fuzz targets `css`, `layout` (a structured generator, no parser in
  front), `xml`, `zip_archive`; every EPUB cap is a `bounds_ledger.rs` row
  measured against a real book — two caps were found set *below* a real
  book and raised.
