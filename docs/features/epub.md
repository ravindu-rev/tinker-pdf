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
stays refused by name under both modes. A document whose declaration names an
XHTML 1.x DTD — the XHTML 1.1 identifier every EPUB 2 book of one measured
producer carries, or XHTML 1.0's three, or XHTML Basic's two — has XHTML 1.0's
253 named character references resolved (`&nbsp;`, `&mdash;`, `&eacute;`), from
the W3C's three entity sets vendored in `tinker-pdf-xml/data/xhtml-entities`;
one with `<!DOCTYPE html>` or none does not, since no DTD declares them there,
and `&nbsp;` in it is refused by name as XML 1.0 requires.

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
the previous **layer**; `@media` evaluated
against a `MediaContext`; `@supports` (`css-conditional-3` §6) evaluated
against **this build** — a declaration test is true where the property and
its value would be accepted in a style rule, so `@supports (display: flex)`
applies its rules and `@supports (display: grid)` or a value refused by value
does not, with `not`, `and`, `or`, `css-conditional-4`'s `selector()` (true
where the selector parses and nothing in it is refused or warned about) and an
unknown test false; `@import` with cycle and depth bounds;
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
(§9.4), an inline box split round an in-flow block inside it (§9.2.1.1); all nine float rules of §9.5, each its own step and fixture; the
§17 table model with §17.2.1's anonymous-box generation — cells written
straight into a table or a row group are one anonymous row of those cells —
§17.5.2.2's two-pass automatic width, §17.5.1's column and column-group
backgrounds painted under the cells that originate in them, and §17.6.2.1's
five-rule border conflict resolution, `rowspan` clamped; `css-flexbox-1`'s line algorithm with
grow/shrink freeze-and-redistribute and `order`, and `inline-flex` as an
atomic inline-level box whose inside is that layout and whose baseline is its
first (§8.5); §13.3 fragmentation rules
A–D with the spec's own escape; `page-break-*`, `orphans`, `widows`, and
`css-break-3`'s `break-before`, `break-after` and `break-inside`, which §3.4
makes the same three properties under their modern names — `break-before:
page` is `page-break-before: always` and `avoid-page` is `avoid`, since the
page is the one fragmentation context this build breaks across. `column`,
`avoid-column`, `region`, `avoid-region`, `recto` and `verso` are refused by
value (`UnimplementedProperty`): the first four name contexts this build has
none of, and the last two resolve through a page progression direction it
does not read.
§3.1's **replaced elements**, sized by §10.3.2 and §10.6.2 with §10.4's
eleven-row constraint table — which is the algorithm `img { max-width: 100% }`
takes, and the one that keeps a narrowed picture from being squashed rather
than scaled. The intrinsic size is three values (a width, a height, a ratio) and
never the bytes: `css-images-3` §4 distinguishes all three and the leaf crate
holds no decoder ([design/epub-layout.md](../design/epub-layout.md)). Line
breaking is **UAX #14** over vendored Unicode 17.0.0 data, passing
**19 338 of 19 338** pairs of Unicode's own `LineBreakTest.txt`. Advances
come from the face (embedded, host-provided, or the standard-14 metrics for
an unembedded family), one per character.

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
what text extraction returns, as in a browser's own PDF; so a book that uses
the property is one the conservation harness would count a transformed
letter in as one missing and one extra. No committed book declares it.

**Painting** (`paint`, `typeface`). An embedded face's run is shaped whole and
written through `DocumentBuilder::glyph_run`, which states **every glyph's own
position** — so `GPOS`'s offsets reach the page and a mark sits at its anchor
rather than at its advance, as 9.4.3's `TJ` adjustments and `Ts`.
`letter-spacing` is folded into those positions and `0 Tc` written, because a
reader applies `Tc` per glyph while layout measures it per character, and a
joined word would otherwise be drawn narrower than the box it was measured
into. A run is shaped against its neighbours' text where they are in the
same face, so a word with a styled letter still joins and a glyph its
neighbour positions keeps the offset, and each visual line's runs are put in
UAX #9's order (`paint::visual_lines`) before anything is drawn, so a
right-to-left line of two styled spans reads right to left, as does a
justified or `word-spacing` line, whose words are laid in that order too
([fonts](fonts.md)). A context is a neighbour that touches the run on its
line — not the next line's first word — and a run whose own glyphs a context
in the other direction would split is shaped alone. Layout measures each run
in the same context (`tinker_pdf_layout::metrics::Shaper::shape_in`), so a
pair kerned across a span boundary or a joined form wider than the isolated
one is the width the line was broken at, not a gap or an overlap between two
runs. A run that mixes
directions is cut at its line's level boundaries first
(`paint::split_at_levels`), each piece taking its share of the run's
measured width, so `a ب<span>ح</span>م b` draws its Arabic word last letter
first and joined across both span boundaries. The levels are the
**paragraph's**: UAX #9's X1 to I2 run once over every line of the bidi
paragraph `flow.rs` set the runs in (`TextRun::paragraph`), across pages,
and only L1 and L2 per line, so where a line wraps does not change the order
inside it — `abc (de` in a right-to-left paragraph draws `(de` on its second
line, as it does unwrapped. A line whose runs are not all of one paragraph
is resolved by itself. **`hyphens`** (`css-text-3`
§5.4) is read at `none` and `manual`, its initial value: a soft hyphen
(U+00AD) stays in a run's text, since the text is the book's, measures
nothing and is not drawn where no line breaks at it; a line that breaks at
one is measured with room for a hyphen and draws one there
(`TextRun::hyphenated`, `paint::hyphenate`); and `none` takes the break
away, and shows no hyphen where `overflow-wrap` breaks a word just after
one. `auto` asks for a hyphenation dictionary this build does not have and
is refused by value. **`direction` and `unicode-bidi`**
(`css-writing-modes-3` §2) decide the line, not each line's first letter:
every run carries its block's direction as its paragraph's base level, so a
left-to-right paragraph that begins with an Arabic word stays left to right;
`start` and `end` alignment, the side `text-indent` is taken from
(`css-text-3` §8.1) and an outside marker's side follow it; an inline
box's `embed`, `isolate` and `plaintext` are carried on its runs as the
levels §2.4.2 maps them to and written as formatting characters only into the
text the painter resolves, never into the page's; and a block's `plaintext`
gives each paragraph — a forced break starts one — its own first strong
direction. HTML's `dir` and `<bdi>` are presentational hints
(`epub::xhtml`): `dir="ltr"` and `"rtl"` set `direction` and isolate, and
`dir="auto"` and a `<bdi>` with no `dir` isolate with the direction HTML's
auto directionality gives them — the first strong character of their text,
skipping `bdi`, `script`, `style`, `textarea` and anything with a `dir` of
its own, `ltr` where there is none — which everything inside them inherits;
only `<pre dir="auto">` and `<textarea dir="auto">` are `plaintext`
(HTML §15.3.5). `:dir()` is still handed `auto` as written, and matches
neither keyword there. A cut piece is drawn at the level its line resolved it at,
so the space and `!` that end a right-to-left paragraph are drawn `! ` and
not ` !`. A run in one of the standard 14 is unshaped and one glyph per character,
and keeps `PageBuilder::glyphs`. Faces are subset to what the book draws; every
run that could not be represented is counted (`UnrepresentedCharacters`,
`UncoveredCharacters`), and a run the writer refused is
`UnwritableTextRun`. **`opacity`** (`css-color-4` §15.1) is an `/ExtGState`
whose `/ca` and `/CA` are the product of every opacity from the root down,
set around each fragment the element's subtree draws — its box, its pictures
and its glyphs, the last inside their marked-content sequences so the two
kinds of bracket nest (`paint::Effects`, registered before the chapter's first
page begins). Wherever nothing in the subtree paints over anything else in it,
that is the group §15.1 composites; where something does, it is counted (see
below). **`border-radius`** (`css-backgrounds-3` §5, the four longhands and
the shorthand with `/`) rounds a box's background and border: each corner one
cubic whose control points sit `4(√2−1)/3` along its tangents, horizontal
percentages of the border box's width and vertical ones of its height, §5.5's
one factor scaling all four corners when two on a side would overlap, the
padding edge's radii the border edge's less the border width (§5.3), and each
side's colour filled between the two shapes inside a clip from the outer
corner to the inner one. A box cut by a page boundary rounds only its real
ends — `box-decoration-break: slice` — though its percentages resolve against
the fragment, and so does §5.5's overlap factor: a short last slice of a box
with large radii has its corners scaled to the slice's height, horizontal radii
included, where `css-break-3` §5.4 would cut the unbroken box's corners. The
whole box's height is unknown on the page that draws its top, and a corner
taller than its slice would need the unbroken shape drawn under a clip rather
than a rounded rectangle of the slice. **`outline`** (`css-ui-4` §5, its four longhands and the shorthand) is
four bands `outline-offset` out from the border edge, drawn after the text,
moving nothing; it is rectangular around a rounded box, which §5 leaves to
the user agent, `outline-color: invert` is refused by value, and
`currentColor` is the initial colour whether written or omitted.
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
conservation loses nothing — and the box after it follows the used height. Who is clipped is the
element tree's question (`paint::Effects`): every fragment is cut by the
clipping elements above it, an element's own text by its own clip, and an
absolutely positioned box only through its containing block (CSS 2.2
§11.1.1). The root's `overflow` — or `<body>`'s, under an `<html>` that has
none — belongs to the page (§3.3) and leaves the element `visible`; and a
scroll container in a flex line has no content-based minimum
(`css-flexbox-1` §4.5). **`background-image`** (`css-backgrounds-3` §2.2,
one layer, a raster `url()` or a gradient) is drawn with **`background-repeat`**,
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
refuses is not drawn and is counted against `background-image` by element,
where it once wrote `inf` into its pattern or vanished unnamed. **`box-shadow`** (`css-backgrounds-3` §7.1) is drawn hard-edged: an
outer shadow is the border box offset and grown by the spread — its corners
grown by §7.1.1's `r + s(1 + (r/s − 1)³)` where the radius is under the
spread, so a square corner stays square — under the background and clipped to
the page less the border box, so a box with no background does not show its
own shadow through itself; an `inset` one fills the padding box less that box
offset and shrunk, over the background and image and under the border. The
first in the list is on top. **`text-shadow`** (`css-text-decor-3` §4) is the
run drawn again, offset and in the shadow's colour, before any of the page's
text and marked `/Artifact`, so text extraction reads the words once. A
translucent shadow colour is its alpha times the element's composed
`opacity`, since an `/ExtGState`'s `/ca` replaces the one in force. A shadow
or an outline whose geometry is not finite — `1e400px` reads as infinite —
draws nothing, as a transform with no inverse does, rather than writing `inf`
or `NaN`, which are not PDF numbers (7.3.3).
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
first page begins — JPEG and PNG, classified by magic bytes and never by name
or by the manifest's `media-type`, through the same pass-through path `cbz.rs`
takes, so a plate is neither decoded nor re-encoded. An `<img>` that does not
reach the page is named (`ImageNotDrawn`, four defects) rather than leaving a
hole the surrounding text closes over.

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

**Where a content document's references are read from** is a trait, since
tier 5's formats row: `read::Resources` resolves and reads a `<link href>`, an
`@import`, an `<img src>` and an `@font-face` `url()` written in a given
document, answering the path and the bytes or `Unavailable::{Missing,
Unreadable}`. `Ocf` implements it as §4.2.5 says, which is what the four call
sites did before; `read::NoResources` answers every reference missing. So
`read::read_document` — and `read::read_dom`, which takes a tree something
else built, and `read::markup`, the XML reader that never fails — read a
document that did not come out of a container by the same cascade and the
same layout. `read::Context::author` carries sheets a caller supplies ahead of
every sheet the document links; a book's is empty. A **loose content
document is a book of one chapter** through the same passes
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
(`every_element_keeps_its_name_and_says_what_it_is`). `<sub>` used to be
written `/Sub`, which is not one of ISO 32000-1's types and had no role map
to explain it. A list marker is an `/Artifact` (§14.8.2.2) and stays out of
the tree, which is what keeps text conservation an equality.

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
  one that has it. A picture is drawn where it always was, before the text in
  painting order, and **reads where it was written**: the layout stamps runs
  with a reading position and pictures with none, so the position is
  recovered from the element tree (an element before the picture's, or the
  picture's ancestor's text up to the child holding it, counted in characters
  that are not white space), and a paragraph whose picture falls between two
  of its runs is split there (`PageBuilder::continue_at`) — `<p>a <img/> b</p>`
  is text, `/Figure`, text. `every_img_alt_is_a_figure_alt`.
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
  `/ParentTree` (14.7.4.4). The annotations are written as they always were
  and found by the anchor's key (`PageBuilder::link_for`). An `<a>` with no
  `href`, or with one this build could not resolve, has no annotation and is
  a `/Span`: a bare `/Link` would claim an association the file does not
  contain. `every_a_href_is_a_link_holding_its_annotation`.
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
files, over a book this path converted. Seven still fire and are named there:
`PdfUaIdentifierMissing`, because the output claims no PDF/UA conformance and
should not, with `MetadataMissing` and `DisplayDocTitleNotSet` beside it;
`FontNotEmbedded`, because a book with no `@font-face` is set in the
unembedded standard 14; and, since the PDF/UA annotation rules (October
2026), `TabOrderNotStructure`, `AnnotationDescriptionMissing` and
`LinkContentsMissing` — a link annotation, inside its `Link` element, with no
`/Contents` and no `/Alt`, on a page with no `/Tabs /S`, which is the tagged
writer's to close.

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
| An external identifier outside EPUB 3.3 Appendix B's three (SVG 1.1, MathML 3.0, NCX) | `tinker_pdf_xml::Warning::ExternalIdentifierNotAllowed` | a warning, not a refusal, and **not a debt**: the three are the specification's closed list, and XHTML 1.1's identifier is outside it because EPUB 3 banned it — so an EPUB 2 book warns, reads, and still gets its named references | — |
| Inside an SVG content document: `<filter>`, `<foreignObject>`, SMIL animation, `<script>` and `<textPath>`/`<tref>`/`<altGlyph>` | `ArchiveWarning::Svg { item, warning }` | the document draws; each of these is a subsystem this build declines, named per document and deduplicated by the crate that met it. The first four are **kept as decisions** — the reasons are in [design/svg.md](../design/svg.md)'s non-goals and the roadmap's *Named non-goals* — and `<textPath>`, `<tref>` and `<altGlyph>` are **unscheduled**, waiting in the roadmap's SVG row for a count (ruling 3). **§14.5's group `opacity` left this row**: a container's opacity, and a shape's where it both fills and strokes, is a transparency group composited once (`a_groups_opacity_is_composited_once`), and `GroupOpacityFlattened` is gone with it. **`<marker>` left this row** too: drawn at its vertices (`tests/markers.rs` in the leaf crate), and a reference naming no `<marker>` is `MarkerUnresolved`. **`spreadMethod` `reflect` and `repeat` left it** with them: a calculator function tiles the ramp past the axis, and `SpreadMethodUnsupported` is gone. **§10.4's per-glyph lists left it too**: an `x`, `y`, `dx`, `dy` or `rotate` per character is set per character, and `TextPositionListIgnored` is gone. **`<mask>` left it**: a luminance soft mask over a transparency group, and a reference naming no `<mask>` is `MaskUnresolved`. **`<pattern>` as a paint left it**: a tiling pattern, one cell of the tile's own nodes, and `PatternUnsupported` is gone; a pattern whose tile has no area paints nothing, §13.3's rule, and one whose tile paints with itself is refused like the `<use>` bomb. **Either on text in `objectBoundingBox` units** — a mask, a clip path, a gradient or a pattern — **left it too**: the leaf takes the runs' glyph cells from the caller, which measures them with the metrics its pages are set with, so the effect is a fraction of the text's real box, and a `<tspan>`'s paint of the whole `<text>`'s (`a_bounding_box_gradient_on_svg_text_spans_the_text_it_paints`). What is left is `TextBoxUnmeasured`, drawn without the effect: a `<tspan>`'s own mask or clip, which takes the whole `<text>`'s box and is built before that box is known. A run under `visibility: hidden` is laid out and not painted, so it is in that box and moves the pen for the text after it (`a_hidden_run_moves_the_pen_and_draws_nothing`). **A clip path's `<use>` and `<text>` children left it**: a `<use>` of a shape clips as that shape, a clip holding text masks by the silhouettes of its shapes and runs, and a child §14.3.5 does not admit — a `<g>`, or a `<use>` of one — is `ClipChildIgnored`; both kinds of child had been dropped without a warning before. A shape, run or picture whose coordinates pass a double's range once its transforms are composed — two `scale(1e300)`s, each legal — is not drawn and is `GeometryOverflow`, where it used to reach the page as infinities **A `<style>` element's at-rules left it** too: `@media` is evaluated as print, `@import` is fetched from the container — one it does not hold is `ImportUnresolved`, and the rules after it apply — and `@font-face` is loaded with the book's faces; what is left is `AtRuleIgnored`, for an at-rule other than those three and `@charset`, or one of them invalid or past a bound (an `@import` after a rule, into a layer, past `MAX_CSS_IMPORT_DEPTH`, importing its own ancestor, or past the token budget or the `MAX_CSS_BYTES` every import shares; a face with no source) | [design/svg.md](../design/svg.md) |
| An `<image>` inside an SVG whose reference does not resolve, or whose bytes are neither JPEG nor PNG | `ArchiveWarning::SvgImageUnresolved { item, images }` | those two are embedded through the same `ImageData` path `cbz.rs` uses; anything else is counted per page rather than drawn as nothing | [design/svg.md](../design/svg.md) |
| An XHTML `<img>` that did not become a box on the page | `ArchiveWarning::ImageNotDrawn { item, defect, images }` | four defects, because each is a different party's fault: `Unresolved` (no `src`, or one the container has no entry for), `UnsupportedFormat(f)` (BMP, TIFF, JPEG 2000 and AVIF are foreign resources an `<img>` does not place even where the comic path reads them — each named by format; every EPUB 3.3 §3.2 core raster type now has a decoder, so none lands here), `Unknown` (bytes matching no magic number — **an SVG lands here**, having none, and is a spine item in this build rather than a replaced box) and `Undecodable` (a JPEG, PNG, GIF or WebP whose bytes would not make an image; a GIF is drawn as its first image, an animated WebP as its first frame). Counted per content document and per defect, so a comic whose forty pictures are all WebP is one sentence a host can act on. The ruling 10 companion to `SvgImageUnresolved`, which is an SVG `<image>` and could never say this | [design/epub-layout.md](../design/epub-layout.md) |
| More distinct element names than one `/RoleMap` carries to this engine's reader (`MAX_DICT_ENTRIES`, 4 096) | `ArchiveWarning::ElementNamesUnmapped { item, names }` | each name past the map is written as its standard type — `/Span`, `/Div` — rather than as itself: a mapping past the cap would be written and then dropped on read, and its elements would read as types no standard defines in a document claiming `/Marked true`. The elements are still tagged and still say what they are; the book's own name for them is what is lost, and counted per content document (`element_names_past_what_a_role_map_carries_are_written_as_their_types`) | ISO 32000-1 14.7.3 |
| An `xml:lang`, `lang` or `dc:language` not shaped like a language tag — `en_US`, a stray space | `ArchiveWarning::LanguageTagIgnored { item, tags }` | not written as `/Lang`: a value a reader cannot use says nothing, so the element inherits its ancestor's language as one that declared none would, and the book is told how many declarations went unwritten. Counted per content document, and once for the package document's `dc:language`, which would have been the catalog's | [design/tagged-pdf.md](../design/tagged-pdf.md) |
| A `<link rel="stylesheet">` whose `href` produced no sheet | `ArchiveWarning::StylesheetUnresolved { item, sheets }` | the document is set without rules its author wrote and the page looks finished, which is `ImageNotDrawn`'s hole for the other reference a content document makes. Counted per content document. Silent until tier 5's formats row, where a loose XHTML file — which has nothing beside it — made every linked sheet one of these | [opening](opening.md) |
| A refused `<img>` — **not a refusal, a stated answer** | — | HTML §4.8.4.4 makes an element *"expected to be treated as a replaced element"* **only when the image is available**, so an unavailable one is an ordinary empty inline and generates **no box**. §10.3.2's 300 by 150 default would put a blank postcard into a paragraph for a reference that was merely misspelled, and carrying `alt` into it would put characters on the page the spine's markup does not contain — one per refused image, with no source character to answer it. Asserted as a byte-for-byte identity against the same book with an empty `<span>` in the `<img>`'s place | [design/epub-layout.md](../design/epub-layout.md) |
| `text-transform` in Lithuanian, Turkish or Azeri, and its `full-width` and `full-size-kana` values | `ArchiveWarning::UnimplementedProperty { property: "text-transform", .. }` | §2.1 requires `SpecialCasing.txt`'s language-conditional mappings when the element's language is known, and the layout crate that applies the transform is handed computed styles and never a language — so the cascade counts every element in one of the three languages (by `xml:lang`/`lang`, inherited) that has a casing transform, rather than letting a Turkish heading set with an English `I` read as honoured. `full-width` and `full-size-kana` map to *other characters*, not cases, and are refused by value alone or beside a casing keyword | `crates/tinker-pdf-layout/src/case.rs` |
| `font-kerning: normal`, or a feature switched on by `font-feature-settings`, for text set in one of the standard 14; an alternate index above one; a `font-feature-settings` list longer than `tinker_pdf_css::limits::MAX_CSS_FEATURE_SETTINGS` (32) | `ArchiveWarning::UnimplementedProperty { property: "font-kerning" \| "font-feature-settings", .. }` | an embedded face's run is shaped, and both properties reach its shaper (`font-kerning: none` as `kern` off, `font-feature-settings` switching features off out of the plan and on into it, `css-fonts-4` §7.2's precedence putting the low-level property last), measured and drawn alike. The standard 14 are drawn a character at a time from their widths — no `GSUB`, no `GPOS`, and none of their AFM kerning pairs, which this build does not carry — so a request switched **on** for such text cannot be met and its element is counted, at paint time, where the face a character is set in is known; a feature switched off is met there already, and `auto` kerning is the user agent's to decide. An alternate index is a parameter the shaper's alternate substitution does not take (it takes the first), and a list past the cap would be copied into every element that inherits it, so both are refused by value | `crates/tinker-pdf/tests/epub_paint.rs`, `crates/tinker-pdf/tests/epub_shaped.rs` |
| `unicode-bidi: bidi-override` and `isolate-override` — every `<bdo>` — and `direction: rtl` on a table, a flex container or a multi-column container, or on the containing block of an over-constrained block or of an absolutely positioned box | `ArchiveWarning::UnimplementedProperty { property: "unicode-bidi" \| "direction", .. }` | an override makes every character inside it strong in one direction, so a Latin word under `rtl` is drawn letter by letter backwards — a glyph order the painter's shaper never produces, since it shapes a slice in that slice's own direction — and it is refused by value rather than drawn as an isolate. `direction` is met where it decides a paragraph; CSS 2.2 §9.10 gives it three more jobs this layout does not do — a table's columns (§17.5), a flex row's main axis (`css-flexbox-1` §2) and a multi-column container's columns (`css-multicol-1` §3) run from the right, and a block over-constrained in a right-to-left containing block gives up its `margin-left` rather than its `margin-right` (§10.3.3), as an absolutely positioned box in one is placed by `right` and from its static position's right edge (§10.3.7) — so each such element is counted rather than read mirror-wise. Which margin gives way is the **containing block's** direction to decide, not the block's own: a right-to-left block with a width in a left-to-right body is laid out as §10.3.3 says and is not counted. A standard-14 piece of more than one character at an odd level is still drawn in logical order, `draw_coded`'s known limit | `crates/tinker-pdf/tests/epub_paint.rs`, `crates/tinker-pdf/tests/epub_shaped.rs` |
| `color-scheme` naming `dark` and not `light` | `ArchiveWarning::UnimplementedProperty { property: "color-scheme", .. }` | a page is printed in the **light** scheme — paper is the light canvas, and print media's `prefers-color-scheme` is `light` — so `normal` and every list that names `light` (pandoc's `:root { color-scheme: light dark }`) is the scheme this build draws, and the property is the no-op it is on paper. A list asking for the dark scheme asks for a canvas and system colours this build does not have, and is refused by value rather than drawn light and called honoured | `crates/tinker-pdf/tests/epub_paint.rs` |
| `opacity` on an element whose subtree holds a box that paints a background or border **and** has content over it | `ArchiveWarning::UnimplementedProperty { property: "opacity", .. }` | §15.1 composites the element as one group and then fades it; this painter fades each fragment, which is the same picture until something inside the element paints over something else inside it — text over its own box's background, each at half alpha, shows the background through the text. The group is a transparency-group form XObject, and the element's glyphs are tagged marked content the structure writer puts in the page stream rather than in a form, so the group is owed to that writer; meanwhile the elements where the two differ are counted by element, and the rest are exact. An archival profile that forbids transparency refuses the alpha outright, and those elements are drawn opaque and counted the same way | `crates/tinker-pdf/tests/epub_paint.rs` |
| A repeating or conic gradient, a gradient with a translucent stop, an interpolation hint or an interpolation colour space, one of more stops than `tinker_pdf_css::limits::MAX_CSS_GRADIENT_STOPS` (32), any other `<image>` function, a second background layer, `background-attachment` other than `scroll`, and a `<box>` in the `background` shorthand | `ArchiveWarning::UnimplementedProperty { property: "background-image" \| "background" \| …, .. }` | a PDF shading has no alpha, and a gradient with a translucent stop drawn opaque would be a different picture; a hint bends the ramp between two stops, which one straight piece per pair cannot; a gradient past the cap is a stitching function written for every fragment on every page its box crosses. The other functions of `css-images-4` are images this build does not draw, §2's comma-separated layers are one image too many, and `background-origin` and `background-clip` — which the shorthand's two `<box>`es set — are not implemented: each is refused by value rather than given its first part, which would draw a background the author did not write | [ROADMAP.md](../ROADMAP.md) |
| A blurred `box-shadow` or `text-shadow`, and a shadow list longer than `tinker_pdf_css::limits::MAX_CSS_SHADOWS` (32) | `ArchiveWarning::UnimplementedProperty { property: "box-shadow" \| "text-shadow", .. }` | a blur is a soft edge — a Gaussian of the shape — and this painter draws none; a hard shadow in its place is a different picture, so the whole declaration is refused by value and counted by element. A list past the cap is refused the same way: a text shadow is the run drawn again, so the cap is how many times a page may draw its text. A translucent shadow under an archival profile that forbids transparency is drawn opaque and counted the same way | `crates/tinker-pdf/tests/epub_paint.rs` |
| A three-dimensional `transform` function (`matrix3d()`, `translate3d()`, `translateZ()`, `scale3d()`, `scaleZ()`, `rotate3d()`, `rotateX()`, `rotateY()`, `perspective()`), a non-zero `z` in `transform-origin`, a `position: fixed` box under a transformed one, and a link under one | `ArchiveWarning::UnimplementedProperty { property: "transform" \| "transform-origin", .. }` | a page is flat, and the one 3D function dropped from a list would be a different flat picture from the one a 3D renderer projects, so the declaration is refused by value. §2 makes a transformed box the containing block of its fixed descendants too, and this layout places a fixed box against the page and repeats it — so each one is counted. A link's annotation is the run's rectangle before the transform, so its active area does not turn with its text; each such link is counted | `crates/tinker-pdf/tests/epub_paint.rs` |
| A background image on a box cut across pages — **a stated answer** | — | each page's fragment positions the image against its own padding box, for `border-radius`'s reason: the whole box's height is not known on the page that draws its top. so a `no-repeat` image is drawn once on every page the box crosses, at its position in each fragment, where `box-decoration-break: slice` would draw it once | `crates/tinker-pdf/tests/epub_images.rs` |
| `object-fit`, `object-position` | `ArchiveWarning::UnimplementedProperty` | a replaced box's content fills its content box exactly, which is what CSS says happens when the property that would say otherwise is absent. An author who states a `width` and a `height` that disagree with the picture's proportions gets a stretched picture, asserted rather than assumed | [ROADMAP.md](../ROADMAP.md) |
| An SVG content document that produced no picture at all | `SpineDefect::SvgUnreadable(tinker_pdf_svg::Refusal)` | six named causes — not XML, not an `<svg>` root, a `<use>` that reaches its own ancestor, or one of four ceilings — and the refusal travels, so a caller can tell a bomb from a truncated file | [design/svg.md](../design/svg.md) |
| `position: fixed` — **not a refusal, a stated answer** | — | CSS 2.2 §9.6.1: *"in the case of paged media, fixed boxes are repeated on every page, and are fixed with respect to the page box"*. So a fixed box is positioned against the page box and drawn on every page of the document. That is the specification's own paged answer, not a degradation of the screen behaviour, and it is what a stylesheet asking for a running header meant | — |
| `position: sticky` — **also a stated answer** | — | `css-position-3` §3.4: a sticky box is offset by how far its nearest scrollport has scrolled, clamped to its containing block. A paginated document has no scrollport, so that distance is zero on every page and §3.4's own words are that it is then *"the same as `relative`"*. The value of a parameter this medium does not have, rather than a gap | — |
| An absolutely positioned or fixed box written inside a line, with `top` and `bottom` both `auto` or `left` and `right` both `auto` | `tinker_pdf_layout::Warning::PositionedInLine` | the box is out of flow (CSS 2.2 §9.6) and is laid out against its containing block, taken out of the line the way a float is — its text was set in the line until October 2026's eighth wave. An inset pair left `auto` places it at its **static position**, where it would have been in flow: on the line it was written in, or below that line for a block-level box. The lines do not exist yet when it is taken out, so its static position is the inline formatting context's top left, a float's own limit, and each such box is named. With an inset stated in each pair it is exact (`epub_reftest.rs`'s `a_positioned_box_inside_a_line_is_taken_out_of_it`) | [ROADMAP.md](../ROADMAP.md) |
| `column-span: all` below a multi-column container's own children | `tinker_pdf_layout::Warning::ColumnSpanAsNone` | `css-multicol-1` §6: a spanning box interrupts the columns and resumes them below itself. **On a child of the container it does**, since October 2026: the children either side are column sets of their own, each balanced, and the spanner is a block across the container between them. A spanner inside one of the children would split that child round itself, which this build does not; it is laid out in the column it fell in, counted per box | [ROADMAP.md](../ROADMAP.md) |
| A single box taller than a page inside a multi-column container | `tinker_pdf_layout::Warning::ColumnTallerThanPage` | the third of the three `Abreast` shapes and the same sentence as the other two: the container is cut across pages, and what is left is one atomic box no cut can halve | [ROADMAP.md](../ROADMAP.md) |
| A `max-height` shorter than the content, on a box whose `overflow-y` is `visible` | `tinker_pdf_layout::Warning::MaxHeightAsAuto` | CSS 2.2 §10.7's clamp, and its two halves land differently here: `min-height` pads the flow out and `max-height` would have to shorten it, which a column whose `y` never goes backwards cannot do once the items are emitted. So the box is its content's height and the declaration is named rather than half honoured, which is this build shape for a value it honours in one direction only. **A box that clips its block axis left this row**: the content past its padding box is clipped and leaves the column, kept as text laid out and not painted, so its `max-height` is its height (`a_block_axis_clip_drops_the_content_past_the_used_height`, and the reftest pair `max_height_on_a_clipping_box_is_the_height_it_clamps_to`) | [ROADMAP.md](../ROADMAP.md) |
| `overflow: scroll` and `overflow: auto` — **a stated answer** | — | a page has no scrolling mechanism, so a scroll container is printed at its initial scroll position — its padding box from the top left — which CSS 2.2 §11.1.1 permits for print and a browser's own print does. What is past it is clipped, and along the block axis out of the column, laid out and not painted | `crates/tinker-pdf/tests/epub_paint.rs` |
| A scroll container beside a float — **a stated answer** | — | CSS 2.2 §9.5: its border box must not overlap the float, and *"implementations should clear the said element by placing it below any preceding floats, but may place it adjacent to such floats if there is sufficient space"*. This build takes the *should*, and a browser the *may*: a picture floated beside an `overflow: hidden` box of text sets the text below the picture here and beside it there. Only the floats crossing the box's top edge are looked for, the box's height being unknown when it is placed | `crates/tinker-pdf-layout/src/tests.rs` |
| Ink past a clipping box's padding box that no advance reaches — **a stated answer** | — | whether a box overflowed is decided on its content's extents, `css-overflow-3` §2.2's scrollable overflow, and not on its ink (§2.1): an italic's overhang past the last advance of a box whose advances fit is drawn rather than cut, since the box writes no clip | — |
| Other at-rules (`@page`, `@counter-style`, …) | `tinker_pdf_css::Warning::AtRuleUnsupported(name)` | skipped by the spec's own recovery, named. **`@supports` left this row** in October 2026, evaluated against what this build implements | — |
| `:hover`, `:focus`, `:focus-within`, `:focus-visible`, `:active`, `:target`, `:visited` — **seven, and the whole of what never matches** | `tinker_pdf_css::Warning::PseudoClassUnsupported(name)` | each names a state of a reading *session*: a pointer, a focus ring, a press, a fragment the reader navigated to, a history. A paginated document has none of them, for any element, ever — so never matching is `selectors-4`'s **answer** here and not this build's gap. Still counted, because a rule that had no effect is something the book said (ruling 10), and the count is asserted by number so a shrinking list cannot read as a passing one | — |
| `::first-line` | `tinker_pdf_css::Warning::PseudoElementUnsupported(name)` | parsed, no box generated. It is not *generated* content: it selects the part of an **already laid out** box that landed on its first line, so honouring it means a second layout pass, which is a different feature from `::before`. **`::first-letter` left this row** in October 2026: its letter is found while the box tree is built, before any line is broken, and wrapped in a box of its own (see the CSS engine above). `::marker`, `::placeholder` and `::selection` are not parsed at all — `::selection` for the reason the pseudo-class row gives, that it names a state of a reading *session* and a paginated document has none | [ROADMAP.md](../ROADMAP.md) |
| `content: url()`, and `counter()`/`counters()` in a counter style this build does not format | `ArchiveWarning::UnimplementedProperty { property: "content", .. }` | `::before` and `::after` generate boxes from strings, `attr()`, `counter()`, `counters()`, the four quote keywords and any concatenation of them. What they do not: `url()` is a replaced element needing a size before the image is fetched, and a counter in a `<counter-style>` outside the nine this build formats (`lower-greek`, `armenian`, `symbols()`) would be a Greek list numbered in the wrong alphabet. Counted, so a book that asks is not quietly given an empty box | [ROADMAP.md](../ROADMAP.md) |
| `quotes: auto`, the initial value, under a quote keyword | `ArchiveWarning::UnimplementedProperty { property: "quotes", .. }` | `css-content-3` §3.2 makes `auto` the marks *"appropriate for the content language"*, which is HTML §15.3.6's per-language table, and this build does not carry it. A quote keyword under `auto` still moves the depth and draws nothing, and each box that asked is counted: English marks in a French book are a plausible wrong page. A `<q>` is `q::before { content: open-quote }` in the user-agent sheet, so every `<q>` in a book that names no `quotes` is counted here. `match-parent` is refused by value | [ROADMAP.md](../ROADMAP.md) |
| `<ol reversed>`, and `counter-reset: reversed(…)` | `ArchiveWarning::UnimplementedProperty { property: "counter-reset", .. }` | a reversed counter starts at the number of items it will count, which is a count of the list this build does not take before numbering it; the list numbers upwards from its `start` and the element is counted rather than the list silently counting the wrong way | [ROADMAP.md](../ROADMAP.md) |
| A single box taller than a page **inside** a table band | `tinker_pdf_layout::Warning::TableRowTallerThanPage` | the band itself is now cut across pages (`css-break-3` §3.1's class-3 break), so what is left is one atomic box — a line box, or a nested band — that no cut can halve. It is drawn where it starts and overflows | [ROADMAP.md](../ROADMAP.md) |
| The same inside a flex line | `tinker_pdf_layout::Warning::FlexLineTallerThanPage` | same, and still two variants: a host with no table in its book must not be told a table row overflowed | [ROADMAP.md](../ROADMAP.md) |
| A float whose **content-stream** order differs from its reading order | `tinker_pdf_layout::Warning::FloatBrokenAcrossPages`, and the content-order figure pinned in `epub_fetched.rs` | **Not a defect any more: a statement about two orders, both measured.** §9.5.1 places a float by geometry, so `clear` can push its box a page past the text it was written among — and a glyph is only on the page it is drawn on. So the *content stream* of `pg16328-beowulf.epub` genuinely reads 2 182 characters out of source order, and that figure stays pinned because it is true of any extractor that ignores the structure tree, which is what §14.8 says to do when there is none. **There is one now, and in logical order the same book conserves exactly: 0 extra, 0 missing.** ISO 32000 §14.7.2 Table 323 lets a structure element's kids name different pages, so the gloss sits under its own element in source position while its marked content stays on the page its glyphs landed on, written as an `/MCR` with its own `/Pg`. Nineteen of the twenty fetched books conserve exactly in logical order; the twentieth is the one with no glyphs at all. Four earlier fixes moved nothing and are worth naming so nobody rebuilds them: moving the float's page decision, removing `css-break-3`'s push (sixty times worse), tightening the reach further, and emitting a structure tree **per page** — which reproduces page order exactly and measures 2 182 too | [design/tagged-pdf.md](../design/tagged-pdf.md) |
| `local()` sources | `ArchiveWarning::FontFace(FaceDefect::LocalUnavailable)` | names a face installed on the reading system, and this engine reads no font directories **by policy** — that is an operating-system dependency `wasm32-unknown-unknown` does not have ([fonts](fonts.md)). Permanent for the engine; a host with installed faces answers it through `FontProvider`. **WOFF and WOFF2 left this row**: both are unpacked, and only a container that will not unpack is refused, by `FaceDefect::PackedContainer` carrying the decoder's own reason | [fonts](fonts.md) |
| Characters no face covers | `ArchiveWarning::{UnrepresentedCharacters, UncoveredCharacters}` | counted, never silently dropped — conservation still holds for the text. In a `bundled-fonts` build a character outside `WinAnsiEncoding` that the standard face's Liberation stand-in covers is drawn in it as a composite font (`/Identity-H`, so no 224-code ceiling and no notdef), and only what the stand-in lacks reaches either count; in a default build there is no face to key a CID font to and the ceiling stays (`epub_fallback.rs` asserts both) | [fonts](fonts.md) |
| Fonts attached after open | `ArchiveWarning::FontsAttachedAfterPagination` | advances decide line breaks, so faces must arrive in `OpenOptions` | — |
| Page box or font size the caller passed that cannot be used | `ArchiveWarning::UnusableOption(BookOptionDefect::{PageWidth, PageHeight, FontSize})`, and `Margin` from a creation call ([creation](creation.md)), which a book never reports since its margin is never the caller's | the default is laid out instead and the caller is told which number was thrown away — a warning, not a refusal, because it is a claim about the caller, not the file | — |
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
servers, a `<style>` element's at-rules — `@media` asked about paper, as a
page is, `@import` fetched from the container against the document's path,
and `@font-face` loaded beside the book's own faces so a run naming the
family is set in it (`an_svg_reaches_its_container_for_its_imports_and_faces`)
— §14.3's clipping, §14.5's group opacity as a transparency group and
a container's `clip-path` as the clip of its rendering, §14.4's masks, §11.6's markers, §5.6's `<use>` with the
bomb refused by name, §5.7's `<image>`, and §10's text — the last through the same `css-fonts-4` §5.3 matcher
and the same shaper that set the rest of the book, because SVG text and XHTML
text in one book must not resolve `serif` two different ways, and painted as
its `fill` and `stroke` say: a colour, a gradient or a pattern each, by 9.3.6's
rendering modes — `fill="none"` is mode 3, invisible text a reader still
extracts (`svg_text_is_painted_as_its_fill_and_stroke_say`). *Corrected 4
October 2026*: a run's solid fill was set and nothing else, so a run filled
with a gradient, a pattern or `none` drew in black and a stroke never drew.
A gradient, a pattern, a mask or a clip in `objectBoundingBox` units on text
is a fraction of its glyph cells, which the leaf has from this reader: an SVG
is read before the book's faces are loaded, since its own `@font-face` rules
are among them, so one whose text needed a box is read a second time once
they are, with `BookMetrics` measuring each run as its page will place it, and
the report follows that read.

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
  declaring the property* over **all one hundred and eight longhands**, not a sample,
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
  in `tests/epub/README.md` — including that a fixed-layout comic reached this
  build as correctly-sized, correctly-clipped, **entirely blank pages**, because
  no path here painted a replaced element. That one is closed: its six pages
  measured 1, 1, 1, 1, 1, 1 distinct colours and now measure 44, 45, 42, 51, 50
  and 63, asserted per page as more than one. **Twenty more are fetched**,
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
  `epub_images.rs`, `epub_memory.rs`; the layout crate's own suite (`layout/src/tests.rs`,
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
