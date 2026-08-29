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
HTML §15), author sheets and `style=""` attributes; `@media` evaluated
against a `MediaContext`; `@import` with cycle and depth bounds;
`@font-face` descriptors collected into a `FaceSet`. Around fifty computed
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

## Refused by name

| What | Typed variant | Why | See |
| --- | --- | --- | --- |
| SVG content documents in the spine | `SpineDefect::SvgContentDocument` | placeholder page; no SVG renderer | [ROADMAP.md](../ROADMAP.md) |
| `min-width`, `max-width`, `min-height`, `max-height`, `vertical-align`, `gap`, multi-column (`column-*`), `position` other than `static` | `ArchiveWarning::UnimplementedProperty { property, elements }` | parsed and counted by the elements reached; a property with no layout consumer cannot be cascaded silently — the build enforces it | [ROADMAP.md](../ROADMAP.md) Tier 4 |
| `@layer` | `tinker_pdf_css::Warning::LayerRefused` | `css-cascade-5` §6.1 sorts layers above specificity, so reading the block as ordinary rules would invert the cascade; refused by name | [ROADMAP.md](../ROADMAP.md) |
| Other at-rules (`@supports`, `@page`, …) | `tinker_pdf_css::Warning::AtRuleUnsupported(name)` | skipped by the spec's own recovery, named | — |
| `:hover`, `:focus`, `:focus-within`, `:focus-visible`, `:active`, `:target`, `:visited` — **seven, and the whole of what never matches** | `tinker_pdf_css::Warning::PseudoClassUnsupported(name)` | each names a state of a reading *session*: a pointer, a focus ring, a press, a fragment the reader navigated to, a history. A paginated document has none of them, for any element, ever — so never matching is `selectors-4`'s **answer** here and not this build's gap. Still counted, because a rule that had no effect is something the book said (ruling 10), and the count is asserted by number so a shrinking list cannot read as a passing one | — |
| `:nth-child(An+B of S)` | `parser::Report::discarded_rules` | the only pseudo-class syntax refused outright. Reading it as the `An+B` without the `of` would style every second row instead of every second `.a`, which is a book that renders beautifully and is wrong; §3.1 drops the rule instead, counted | [ROADMAP.md](../ROADMAP.md) |
| `::before`, `::after`, other pseudo-elements | `tinker_pdf_css::Warning::PseudoElementUnsupported(name)` | parsed; no box generated, so the rule matches nothing rather than colouring the originating element | [ROADMAP.md](../ROADMAP.md) |
| `display: inline-block` | `tinker_pdf_layout::Warning::InlineBlockAsInline` | laid out as inline text; width/height/vertical margins ignored | [ROADMAP.md](../ROADMAP.md) |
| A table row taller than a page | `tinker_pdf_layout::Warning::TableRowTallerThanPage` | the row overflows; intra-row slicing is staged, not built | [ROADMAP.md](../ROADMAP.md) |
| A flex line taller than a page | `tinker_pdf_layout::Warning::FlexLineTallerThanPage` | same | [ROADMAP.md](../ROADMAP.md) |
| A float broken across pages | `tinker_pdf_layout::Warning::FloatBrokenAcrossPages` | warned rather than pushed; a reading-order defect in the float path is pinned in `epub_fetched.rs` | [ROADMAP.md](../ROADMAP.md) |
| WOFF / WOFF2, `local()` sources | `ArchiveWarning::FontFace(FaceDefect)` | only sfnt programs are read; the family falls back | [ROADMAP.md](../ROADMAP.md) |
| Characters no face covers | `ArchiveWarning::{UnrepresentedCharacters, UncoveredCharacters}` | counted, never silently dropped — conservation still holds for the text | [fonts](fonts.md) |
| Fonts attached after open | `ArchiveWarning::FontsAttachedAfterPagination` | advances decide line breaks, so faces must arrive in `OpenOptions` | — |
| Page box or font size the caller passed that cannot be used | `ArchiveWarning::UnusableOption(BookOptionDefect::{PageWidth, PageHeight, FontSize})` | the default is laid out instead and the caller is told which number was thrown away — a warning, not a refusal, because it is a claim about the caller, not the file | — |
| Encrypted resources, missing rootfile, unreadable package document, unsupported package version, empty spine, a book that could not be paginated | `ArchiveRefusal::{EncryptedResources, RootfileMissing, UnreadablePackageDocument, UnsupportedPackageVersion, EmptySpine, UnpaginatedBook, UnreadableContainer}` | refused at open, by name | [cbz](cbz.md) |
| Scripting, MathML layout, media overlays | `ArchiveWarning::UnimplementedFeature` | declared in `properties`, reported, content rendered as its fallback text | — |

**Corpus caveats, stated**: no fixed-layout book from a real producer and
no real producer's font through the `@font-face` path are in the committed
set ([ROADMAP.md](../ROADMAP.md) Tier 4).

## Verified

- **Six committed books from two real producers** (pandoc 3.10.2, calibre
  9.13.0) over text authored here, in `crates/tinker-pdf/tests/epub/`,
  with epubcheck 5.3.0 verdicts recorded in `EPUBCHECK.tsv`. Those verdicts
  are now a **dated measurement and not a check** — ruling 13 ended the
  re-run, so when this engine and a book disagree there is no arbiter. What
  the record still holds is that every book has a verdict and that the set of
  books the tool was unhappy with is the one recorded. **Twenty more are
  fetched**,
  never committed (Project Gutenberg's trademark licence and `epub3-samples`'
  CC-BY-SA are both barred by this repository's own no-copyleft gate), and
  `epub_fetched.rs` prints `epub-corpus: RAN` / `SKIPPED` so the CI job goes
  red on a skip. `INVENTORY.tsv` is recomputed through `tinker-pdf-zip` on
  every `cargo test`; `CONSERVATION.tsv` is a ratchet, re-measured by
  `epub_conservation.rs`.
- `tests/epub.rs`, `epub_ocf.rs`, `epub_package.rs`, `epub_reading.rs`,
  `epub_css.rs`, `epub_tables.rs`, `epub_fixed_layout.rs`, `epub_fonts.rs`,
  `epub_memory.rs`; the layout crate's own suite (`layout/src/tests.rs`,
  floats and tables step by step, UAX #14 conformance over the full pair
  table); the CSS crate's tokenizer, selector and cascade suites.
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
