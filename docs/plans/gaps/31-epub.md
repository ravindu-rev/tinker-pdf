# An EPUB opens today, and the book it shows is its cover

An EPUB is an OCF container — a ZIP whose first entry is an uncompressed
`mimetype`, holding `META-INF/container.xml`, a package document naming a
spine, and XHTML content documents styled by CSS. It is the third and last of
the formats [28](28-tinker-integration-decisions.md) decided are built here,
and it is the one where neither the container nor the markup is the problem:
the problem is that a reflowable book has no pages until something lays it out.
When this is done, an `.epub` opens as a `Document` whose pages are the book
paginated at a stated page box, drawn from its own XHTML and CSS rather than
from whichever raster happens to be the cover. (XL+)

**This is the third of the three plans gap 28 promises**, and the last.
That document says, at the end of its option D section: *"Three new gap plans
will be written for them — CBZ, XPS and EPUB — after this gap closes."*
[29](29-cbz.md) closed at `b764917`; [30](30-xps.md) closed at `1574767`. Gap
28 sizes this one **XL+** — *"a layout engine rather than a renderer, and on
its own larger than the entire twenty-eight-plan gap programme just
completed"* — and this plan does not dispute the size. It disputes what the
sentence leaves out, which is the first of the corrections below.

## The owner's answer on scope, and what it commits this plan to

The owner was asked, on 19 August 2026, which of three targets this plan should
aim at: fixed-layout EPUB only, a bounded reflowable subset, or the full
reflowable engine. **The answer is the full reflowable engine** — a genuine CSS
cascade with specificity and inheritance, floats, tables, flexbox, the full box
model, media queries, line breaking, font fallback and pagination.

That is written here rather than left to the scope list because it forecloses
the descope that every reader of this plan will otherwise propose. A subset is
not planned; where a phase must be deferred it is deferred **as a staged
decision with its argument, amending its own row in place**, which is what gap
30's milestone 8 did to its `VisualBrush` row rather than quietly claiming it.

**The risk the owner was shown, and accepted, is the one this plan is built
around.** A half-built cascade does not fail. It renders *plausibly and wrong*,
and this repository has now met that failure three times in three different
codecs: [18a](18a-jpx-decoder.md) found a JPEG 2000 precision shift that passed
every boundary test because it produced a plausible photograph;
[07](07-stroked-patterns.md) found a gradient-stroked rule painting solid black
in silence; gap 30 found a simple-font fallback that draws readable, plausible
text wrong only where a font's cmap and WinAnsi disagree. **A partially
implemented CSS property is worse than all three**, because a codec that is
half right produces a picture somebody can look at and doubt, and a layout that
is half right produces a page that looks like a book. Nobody can tell by
looking. A margin dropped on one element moves every line below it, on every
page after it, and the result is a perfectly readable book with the wrong words
on the wrong pages.

So [How a partial implementation is made visible](#how-a-partial-implementation-is-made-visible)
is the central design section of this document, not an afterthought, and four
of the thirteen milestones exist to serve it.

## Which rule governs this, since it is not ruling 3

Ruling 3 schedules deferred capabilities by corpus hit-rate. Both
[10](10-mesh-shadings.md) and [18a](18a-jpx-decoder.md) had to argue over it in
writing before they could be built; [29](29-cbz.md) and [30](30-xps.md) each
had to say in as many words that a container format is outside its remit, and
the paragraph is owed a third time rather than left for a reader to
reconstruct.

Ruling 3 binds [02](../02-filters.md), [08](../08-rendering-device.md) and the
master plan's descope levers, and what it schedules is a **`Capability`** — a
codec inside a PDF that the engine defers behind a flag, degrades with a
placeholder under ruling 2, and builds when the nightly hit-rate report says
real documents need it. EPUB is none of those. It is a document format, not a
codec; it produces no `Capability` variant; and no PDF in any corpus will ever
contain one, so a corpus hit-rate for it is not a number that can exist.
[23](23-corpus-runner.md) measured 4 525 PDFs and could not have measured this
if it had run for a year.

What governs it is an **owner decision, dated 16 August 2026, recorded in plan
15 where the options used to be** and summarised in gap 28's `As built`,
extended by the scope decision of 19 August 2026 recorded in the section above.
That is the same authority [27](27-form-calculations-decision.md),
[29](29-cbz.md) and [30](30-xps.md) were built under. As gap 29 first noted,
plan 15's own answer mis-cites the rule that keeps the parsers ours: it is
**CONTRIBUTING rule 1**, backed by `deny.toml`, and not ruling 3. This plan
cites rule 1 for the CSS parser, for the layout engine, for SHA-1 and for the
line breaker. **There is no exception for a CSS parser and none for a line
breaker**, and the two are named because they are the two most reachable-for
dependencies in the whole twenty-nine-plan programme.

## What is wrong

**An `.epub` opens today, and what comes back is one page that is the cover.**

Measured on this machine, before a line of this plan's code exists, against six
EPUBs downloaded from Project Gutenberg and three built by hand:

| File | Result | `page_count()` | `page(0).size()` | Warnings |
| --- | --- | --- | --- | --- |
| `pg84-images.epub` — Frankenstein, EPUB 3, 41 entries, 31 content documents | **opens** | **1** | 1824 × 2726 pt | **none** |
| `pg1342-noimages.epub` — Pride and Prejudice, EPUB 2 | **opens** | **1** | 1500 × 2114 pt | **none** |
| `pg2701-images.epub` — Moby-Dick, EPUB 3 | **opens** | **1** | 780 × 1227 pt | **none** |
| `pg11-alice-images.epub` — EPUB 3 | **opens** | **1** | 800 × 1104 pt | **none** |
| `pg11-epub2-images.epub` — the same book, EPUB 2 | **opens** | **1** | 800 × 1104 pt | **none** |
| `pg16328-beowulf.epub` — EPUB 3 | **opens** | **1** | 1826 × 2726 pt | **none** |
| hand-built, one cover image | **opens** | 1 | 1824 × 2726 pt | none |
| hand-built, three images and an obfuscated font | **opens** | **3** | three different sizes | none |
| hand-built, no image at all | **refused** | — | — | `ArchiveRefusal::NoImages` |

Frankenstein is thirty-one XHTML content documents, three stylesheets and a
cover JPEG. It opens as **one page, 1824 × 2726 points — twenty-five inches by
thirty-eight** — which is Project Gutenberg's auto-generated cover at one pixel
to the point, under gap 29's CBZ convention. The thirty-one chapters, the three
stylesheets, the package document, the navigation document and the container
are all discarded, and `ArchiveReport::warnings()` is **empty**, because from
`cbz.rs`'s side nothing went wrong: it found one image entry and paged it.

For every file that opens: `xps_dialect()` is `None`, `parsed_parts()` is **0**,
`ladder_level()` is `Trust`, and every `PageOrigin.defect` is `None`. Nothing
anywhere says a book was lost.

**This is a live defect and it is gap 30's, unfixed.** That plan closed the
XPS half of it and said so plainly — *"one signature covers CBZ, XPS, EPUB,
ODF, OOXML and every JAR ever built"* — and EPUB is the entry in that list
nobody has been back for.

### The route, and the sharper finding

An EPUB does not reach ECMA-388 E.3's third step. It fails the **first** of
step 2's two checks and falls through to the comic path with the archive
unread:

```text
Document::open(bytes)                             lib.rs:335
  └ cbz::container(&bytes) → Some(Container::Zip)  cbz.rs:220
      (always true: OCF 4.3.2 requires an uncompressed `mimetype` first)
  └ open_container(Zip, &bytes)                    lib.rs:283
      └ xps::route(archive, …)                     xps.rs:788
          └ recognise(&mut package)                xps.rs:817
              package.item_index("[Content_Types].xml").is_none()
                  → Recognition::NotXps            ← every EPUB exits here
      └ cbz::pages_from_archive(archive, …)        cbz.rs:819
```

Two consequences, and the second is the one a reader would guess wrong:

- **`ArchiveRefusal::UnreadablePackage` is unreachable for an EPUB.** Gap 30
  built it for a package that carries OPC's own two items and will not resolve.
  EPUB is OCF and carries neither `[Content_Types].xml` nor `_rels/.rels` —
  measured, 0 of 9 files had either — so the refinement gap 30 argued at length
  never fires here, and the comic fallthrough is exactly what E.3's own text
  asks for. **Gap 30 is not wrong; EPUB is a different question it did not
  ask.**
- **`cbz::pages_from_archive` classifies by magic bytes only**, so `mimetype`,
  `META-INF/container.xml`, the OPF, every XHTML document, every stylesheet, the
  NCX and every font match nothing, `image_format` returns `None`, and the entry
  is **silently skipped with no warning at all** — a silence `cbz.rs`'s own
  module header justifies, correctly, for `ComicInfo.xml` and `Thumbs.db`.

The eight-of-nine that open are the serious half; the one that is refused is
the other half of gap 30's pair, arriving verbatim. `NoImages`'s own
documentation reads *"a valid archive with no image entries"* — said about a
book.

**The defence is milestone 3**, and it is the only one of this plan's thirteen
milestones that improves matters on its own. If this plan were ever descoped,
milestone 3 is the part that must still land.

## The five decisions taken during planning

Each is put here, before any file exists, because each is the kind that
otherwise gets made implicitly by whoever writes the first module — the failure
gap 18's risk table named for the fixed-point width, the reason
[18a](18a-jpx-decoder.md) exists as a separate document, and the reason gap 30's
milestone 8 found two matrix bugs that existed only because composition order
had not been settled in writing.

**1. An EPUB becomes a `Document` by synthesising a PDF at open**, as a CBZ and
an XPS do. The argument is gap 29's and gap 30's and is not rebuilt; what *is*
new is what "at open" now costs, which is decision 2. See
[The seam](#the-seam-synthesis-a-third-time-and-the-one-thing-that-is-new).

**2. Pagination is an input to `open`, not to `render`.** `Document::open`
grows a sibling, `Document::open_with(bytes, &OpenOptions)`, carrying the page
box, the base font size and the `FontProvider`. `open(bytes)` keeps its exact
signature and means `open_with(bytes, &OpenOptions::default())`. See
[Where pagination comes from](#where-pagination-comes-from).

**3. `tinker-pdf-xml` grows a doctype *mode*, and the four bombs are re-proved
under it.** XHTML in the wild carries doctypes — measured at 100 % of Project
Gutenberg's EPUB 2 content documents — and EPUB 3.3 §3.9 forbids exactly the
dangerous half. See [The doctype collision](#the-doctype-collision).

**4. Two new leaf crates, not one and not none**: `tinker-pdf-css` and
`tinker-pdf-layout`, the ninth and tenth. See
[Two crates, and why not one](#two-crates-and-why-not-one).

**5. A property is a parser variant only when a consumer exists**, and the
consumer's `match` is exhaustive, so a property that is parsed and ignored
**does not compile**. See
[How a partial implementation is made visible](#how-a-partial-implementation-is-made-visible).

## Corrections, made while this was written

**Gap 28's sizing sentence is right and incomplete.** It says EPUB is XL+
because it is *"XHTML plus a CSS cascade, a box model, line breaking,
pagination and font fallback — a layout engine rather than a renderer"*. Every
word is true and it names only the layout. It omits two things this plan
found:

- **the container is a live defect**, not a missing feature — an EPUB opens
  today and shows its cover, measured above; and
- **the writer is missing a half again.** Gap 30's whole correction to gap 28
  was that the reader was ahead and the writer behind. It is behind here too, in
  a different place: `DocumentBuilder` emits **no annotations at all** — gap 30
  named that as a non-goal in as many words — and no document outline. An EPUB's
  navigation document and its intra-book cross-references are both, so milestone
  5 is this plan's milestone 5 for gap 30's reason, and it lands before its
  consumers.

The size is unchanged. The sentence is amended in gap 28 and in the README row.

**Ruling 8's own amendment overstates the reuse, by one word.** Gap 30
milestone 9 wrote that *"[31], EPUB, reuses this crate and reuses none of gap
30's package layer"*. The second half is exactly right and measured — EPUB
carries neither of OPC's two items. The first half says "reuses" where the truth
is "reuses, after changing". `tinker-pdf-xml` as it stands refuses **every**
Project Gutenberg EPUB 2 content document and the cover wrapper of every EPUB 3
one, on `<!DOCTYPE`. Reuse of a leaf that must first grow a mode is a different
claim from reuse of a leaf as frozen, and the amendment is corrected to say so.

**`deny.toml` has no CSS crate, no HTML crate, no line-breaking crate and no
Unicode-data crate in it.** Sixty-six names are denied and not one of them is
`cssparser`, `html5ever`, `selectors`, `lightningcss`, `markup5ever`, `taffy`,
`unicode-linebreak`, `xi-unicode` or `harfbuzz`. This is the same hole gap 29
found for `zip` and gap 30 found for XML, a third time, and in the largest
format of the three. See
[`deny.toml` has a hole exactly here](#denytoml-has-a-hole-exactly-here).

**`tinker-pdf-crypto` has no SHA-1.** `aes.rs`, `handler.rs`, `md5.rs`,
`rc4.rs`, `sha2.rs` — and EPUB's font obfuscation key is a SHA-1 digest (3.3
§4.4.3). That is gap 29's CRC-32 finding arriving in a second crate: new code,
hand-rolled under rule 1, with published test vectors, and it goes beside
`md5.rs` for the same reason CRC-32 went into `filters`.

**One claim in the brief this plan was written from is refined rather than
corrected.** It suggests an EPUB "may well open as a comic, exactly as an XPS
did". It does, and the shape is one step worse: a Project Gutenberg EPUB
contains **exactly one image** — the auto-generated cover — whatever its
`.images` / `.noimages` variant says. Four titles were tried, illustrated ones
among them. So a thirty-one-chapter novel does not open as a thirty-one-page
book of the wrong things; it opens as **one page**, and the page is the one part
of the file with the weakest claim to being the book.

## Scope

- **OCF**: the abstract container (3.3 §4.2), the ZIP requirements (§4.3.2)
  including the uncompressed-`mimetype`-first rule, the `META-INF` directory and
  its six reserved files (§4.2.6.3), `container.xml` and its `rootfile`
  (§4.2.6.3.1), file-path and file-name restrictions (§4.2.3), and URL
  resolution within the container (§4.2.5).
- **The package document** (§5): `package` with `unique-identifier` (§5.4),
  `metadata` and the three required Dublin Core elements (§5.5.3.1),
  `manifest` and `item` with `properties` (§5.6.2), `spine` and `itemref`
  (§5.7), manifest fallbacks (§3.5.1) and core media types (§3.2).
- **XHTML content documents** (§6.1), read through `tinker-pdf-xml` with the
  doctype mode of decision 3, and turned into an element tree.
- **A CSS implementation**, in a new leaf crate: tokenizing and parsing
  (`css-syntax-3`), selectors with specificity (`selectors-4` §15) and the four
  combinators (§14), the cascade in its full sorting order (`css-cascade-5`
  §6.1), inheritance and the computed/used/actual value stages (§4.4–§4.6,
  §7.1–§7.2), media queries (`mediaqueries-4`), `@import`, `@font-face`
  (`css-fonts-4` §4.1) and `@page`.
- **A layout engine**, in a second new leaf crate: the box model
  (`css-box-3`), block and inline formatting contexts (CSS 2.2 §9.4.1, §9.4.2),
  line boxes, floats and `clear` (§9.5, §9.5.1, §9.5.2), positioning (§9.6),
  the table model and both layout algorithms (§17.2, §17.5.2.1, §17.5.2.2) with
  both border models (§17.6.1, §17.6.2), flexbox (`css-flexbox-1`), white-space
  processing (`css-text-3` §4.1.1, §4.1.2), line breaking (§5, §5.5, over UAX
  #14), alignment and justification (§6), and **fragmentation into pages**.
- **Fonts**: `@font-face` through `tinker-pdf-font`, the font matching
  algorithm (`css-fonts-4` §5), character-level fallback (§2.1, §5.3), and
  **both** de-obfuscation algorithms — IDPF's (3.3 §4.4.3, §4.4.4) and Adobe's
  `http://ns.adobe.com/pdf/enc#RC` — which requires SHA-1, which does not exist
  here.
- **Fixed-layout renditions** (§8.2): `rendition:layout: pre-paginated`, which
  3.3 RS §8.1 makes *"exactly one page per spine itemref"*, and the content
  document dimensions of §8.2.2.6.
- **The writer's missing half**: link annotations, so an internal cross-
  reference between two spine items survives synthesis, and a document outline
  built from the navigation document (§7, and its `epub:type="toc"` nav).
- **Telling an EPUB from a comic and from an XPS** inside `Document::open`, and
  `ArchiveRefusal` growing what an unreadable book needs.
- **A refusal, by name, everywhere the answer would otherwise be a plausible
  wrong book** — the feature, not the absence of one.

## Non-goals

Each of these is refused rather than approximated, and named so a reader does
not infer it from "EPUB works now".

- **Writing an EPUB.** Nothing in the shipped surface produces one. A
  synthesised document saves as a PDF, which is what it is.
- **Scripting** (§6.3.2). No JavaScript reaches a content document. This is not
  the ECMAScript subset [27](27-form-calculations-decision.md) built: that
  interpreter is a form-calculation host with a two-method `Host` and no DOM,
  and pointing it at a document tree is a different project. A scripted content
  document renders its unscripted state and **says so by name**, because a book
  whose content is written by a script renders convincingly empty.
- **Media overlays, audio and video** (§9). Recognised from the manifest and
  refused by name.
- **Remote resources** (§3.6, §3.8). This engine performs no I/O of any kind, so
  an `http://` or `file://` reference is not fetched. It is **refused by name**
  rather than left to fail as a missing part, for the reason gap 30's milestone
  9 recorded: refusing a scheme is what keeps a reference from becoming an
  attempt at I/O.
- **`data:` URLs** (§3.7) are in scope for images and out of scope for content
  documents and stylesheets, and the asymmetry is stated rather than discovered.
- **DRM, `rights.xml` and `signatures.xml`** (§4.2.6.3.5, §4.2.6.3.6). An
  encrypted publication is **refused by name**, and **nothing here claims a
  signature is valid** — that sentence is in the non-goals rather than left
  implicit, because silence about a signature is a security claim by omission.
  Font obfuscation is not DRM and is in scope; `encryption.xml` naming any
  algorithm other than the two obfuscation URIs refuses the book.
- **SVG content documents** (§6.2) as *spine items*. SVG-as-image is a separate
  question and is also out: this engine has no SVG parser, and building one is a
  fourth format inside the third. Refused by name at the element and at the
  spine.
- **MathML**, **ruby**, and **vertical writing modes**. `css-writing-modes-3`
  is in the CSS Snapshot's official definition and `writing-mode:
  vertical-rl` is what a Japanese EPUB is. This plan implements the
  **horizontal-tb** flow only and **refuses a vertical writing mode by name at
  the book level**, rather than laying it out horizontally — which is the single
  most plausible-and-wrong output this format can produce, and the reason it is
  a refusal rather than a degradation. Recorded as an explicit staged decision:
  it is the first thing a fourteenth milestone would build.
- **Bidirectional text** (UAX #9), beyond what [06](../06-content-and-text.md)
  already declines. `dir="rtl"` is recognised and **refused by name**.
- **Text shaping.** No `harfbuzz`, no GSUB/GPOS. Glyphs are laid out by advance
  width, which is what this engine already does for a PDF and is stated here
  because a reader will expect otherwise from a layout engine.
- **CSS animations, transitions, transforms, filters, grid, multi-column,
  shapes, `position: sticky`, and custom properties.** Each is *parsed to a
  known name and refused by name* rather than silently dropped, which is the
  whole of decision 5. CSS Grid and multi-column are the two worth flagging
  rather than filing under "rare", because a book that uses either will lay out
  as a single column and look entirely reasonable.
- **Progressive or partial open.** The book is synthesised whole at `open`,
  like every other document this engine produces — which is
  [what pagination costs](#where-pagination-comes-from).
- **EPUB 2 as a target.** EPUB 2's OPF 2.0 and NCX are read where a real book
  needs them — measured, two of six Gutenberg books are EPUB 2 — but this plan
  implements EPUB 3.3 and treats 2.0 as a compatibility surface. A `.epub`
  whose package version is neither is refused by name.

## Design

### The seam: synthesis, a third time, and the one thing that is new

`Document::cos(&self) -> &CosDocument` returns a **borrow, and not an
`Option`**. Gap 29 made that one signature the whole of its argument; gap 30
rebuilt the argument against a genuinely attractive alternative it had and CBZ
did not, and took synthesis anyway for five reasons. Neither argument has
moved, and neither is restated here. EPUB has no third alternative: there is no
`Device` vocabulary for a paragraph, and a book that emitted `fill_path` calls
would still have nothing to lend `cos()`.

**What is new is that synthesis is now expensive at a place it was not.** For a
CBZ and an XPS, "synthesised whole at `open`" cost a walk over already-final
structure. For an EPUB it costs **the entire layout of the book**, because there
are no pages until layout produces them. `page_count()` cannot be answered
without laying out every content document in the spine, so `open` on
Frankenstein is thirty-one XHTML parses, a cascade over every element, and a
full fragmentation pass.

Three consequences, all accepted with their arguments:

*The cost is bounded and the bound is the book.* Gap 29's pass-through argument
holds for images unchanged — a page's raster cost is a multiple of the entry,
not *w × h × 3* — and text is small. What layout adds is a box tree, which
[Bounds](#bounds-per-ruling-1) caps as a total rather than per page.

*`page_count()` is honest and cheap after `open`, and there is no lazy state.*
The alternative — paginate on first `page(i)` — makes `page_count()` either a
lie or a hidden full pass, and makes two calls to `page(3)` able to disagree
after a `with_fonts`. `Document(Arc<DocInner>)` is `Send + Sync` and plan 00
says so; a document that lays itself out lazily behind interior mutability is a
different concurrency contract than the one that document froze.

*And `Page::text()` comes free, once.* This is the property gap 30 recorded and
it is worth more here than there: `Page::render` drives the rasterising device
and `Page::text()` drives `TextDevice`, and **both go through `interpret` over
a content stream**. A book laid out into one content stream per page is read by
both. A second producer for text would be a second layout engine, and the two
would disagree about which page a sentence is on — which is not a rendering
difference, it is a different book.

The mechanism is gap 29's and gap 30's, unchanged: `DocumentBuilder` builds
objects, `finish()` serialises to `Vec<u8>`, and `CosDocument::open` parses
those bytes, so the synthesised book goes out through the writer and back in
through the reader and cannot take a shortcut around the parser every real PDF
goes through.

### Where pagination comes from

**A PDF page is fixed and a reflowable book is not**, and this is the question
that has no precedent in either earlier plan. A CBZ's page size is its image's
pixels; an XPS states its own in 1/96 inch. A book states nothing at all.

**Three facts constrain the answer, and all three are in the code today.**

1. `Document::open(bytes)` takes bytes and nothing else (`lib.rs:335`).
2. `RenderOptions` is a parameter of `Page::render`, so it arrives **after**
   pagination and cannot decide it. Its `scale` is documented as *"pixels per
   PDF point"*, which is a resolution and not a page box.
3. `Document::with_fonts(provider)` is a builder that arrives **after `open`**
   — and line breaking needs advance widths, so a book laid out at `open` with
   no provider would be laid out with metrics the render then does not use.

The third is the sharp one and it is the shape gap 30 found for its writer: a
seam that is fine for every format built so far and structurally wrong for this
one. A `with_fonts` that silently does not change the pagination is exactly the
invisible partial implementation this plan exists to prevent.

**So `OpenOptions` is the answer, and it is one new constructor and no break.**

```rust
/// What a reflowable document needs decided before it has any pages.
///
/// Every field is ignored by a PDF, a CBZ and an XPS, which have their own
/// page geometry and always did. It exists because an EPUB does not.
#[non_exhaustive]
pub struct OpenOptions {
    /// The page box a reflowable document is laid out into, in points.
    pub page: (f64, f64),
    /// The base font size, in points, that `1rem` and an unstyled paragraph
    /// resolve to.
    pub font_size: f64,
    /// Faces for text the book does not embed. Here rather than on
    /// `with_fonts`, because a substituted face's advance widths decide where
    /// every line breaks and therefore how many pages there are.
    pub fonts: Option<Arc<dyn FontProvider>>,
}

impl Document {
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<Document, OpenError> { … }
    pub fn open_with(
        bytes: impl Into<Arc<[u8]>>,
        options: &OpenOptions,
    ) -> Result<Document, OpenError> { … }
}
```

`open(bytes)` is `open_with(bytes, &OpenOptions::default())` and its signature
does not change, so ruling 12's `tinker_parity.rs` is untouched and no caller
recompiles.

**The default page box is 432 × 648 points — six inches by nine — at a
12-point base size.** A number has to be chosen and defended, exactly as gap 29
had to defend one image pixel to one PDF point:

- It is the size the medium is printed at. A trade paperback is 6 × 9 inches,
  and an EPUB is a book rather than a report; US Letter would produce
  ninety-character lines that no typographer would set.
- `Page::size()` then reports a number a host can size a viewport with
  directly, and `RenderOptions::default()` at 72 dpi renders it at 432 × 648
  pixels, which is readable.
- It is **stated on the type**, and the doc comment says the thing that must be
  said: **for a reflowable EPUB, the page count is a function of this number and
  is not a property of the file.** A page count that silently depends on an
  undocumented constant is a lie about what a page is. Two hosts that pass
  different boxes get different books, on purpose.

MuPDF's own answer is the same shape and is worth citing rather than
re-deriving: `mutool draw` takes `-W` and `-H` for *"page width/height in points
for EPUB layout"* and `-S` for *"font size in points for EPUB layout"*, all
three separate from the render resolution. That is three flags for exactly these
three fields, from the implementation gap 28 is removing, and agreeing with it
here is evidence the seam is in the right place.

**`with_fonts` on a reflowable document is a named warning, not a no-op.** It
cannot re-paginate — the document is already synthesised — and silently
accepting a provider that changes nothing is precisely the failure this section
is about. It warns, it is documented as too late, and `OpenOptions::fonts` is
where a provider goes.

**And pagination does not depend on the host's font directory.** With no
provider the generic families resolve to the standard-14 metrics that are
already built in — `serif` to Times, `sans-serif` to Helvetica, `monospace` to
Courier — so a book with no embedded font and no provider still lays out
deterministically and still draws nothing, reporting `UnreadableFont` exactly as
a PDF does. Ruling 4's contract is *same input, same bytes*; the provider is an
input, and it is an input to `open`.

### The doctype collision

`tinker-pdf-xml` refuses `<!DOCTYPE` **before one byte after it is read**, as
`Error::DoctypeUnsupported`. Gap 30 argued that at length and the argument is
right: ECMA-388 9.3.2 [M2.71] makes refusal the *conformant* behaviour for XPS,
four bombs are committed against it, and
`every_bomb_is_refused_as_a_doctype_and_not_as_a_cap` asserts that the parser
never enters the grammar that has the attack in it.

**XHTML in the wild carries doctypes, and this was measured rather than
assumed:**

- **Every** content document of both Project Gutenberg EPUB 2 books — sixteen
  and fourteen files — carries, on line 2, with **single quotes**:
  `<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.1//EN' 'http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd'>`
- Every Project Gutenberg EPUB 3 book carries `<!DOCTYPE html>` on exactly one
  file, the cover wrapper `OEBPS/wrap0000.xhtml`, and on no other.

So a reader built on the parser as it stands refuses 100 % of one producer's
EPUB 2 content documents. That is not an edge case; it is the first real book.

**The specification takes a side, and it is a more useful side than "allow
it".** EPUB 3.3 §3.9 says an XML publication resource *"MAY only specify a
document type declaration that references an external identifier appropriate for
its media type"* and *"MUST NOT contain external entity declarations in the
internal DTD subset"*. **Appendix B, "Allowed external identifiers", is what
"appropriate for its media type" means**: a closed set, holding SVG 1.1's and
MathML 3.0's public identifiers and — added later, per `epubcheck`'s own commit
history — the NCX's.

**And the XHTML 1.1 identifier that every measured EPUB 2 content document
carries is not in that set.** It was banned from EPUB 3 deliberately, so
Gutenberg's EPUB 2 books are non-conformant against 3.3 in exactly this respect,
which is unsurprising for EPUB 2 and is not a reason to refuse them. It is a
reason for the third behaviour rather than the two obvious ones: **the identifier
is skipped, the book is read, and an identifier outside Appendix B's set is
*named in a warning*.** Refusing loses the book; accepting silently loses the
fact. Ruling 10's shape, and the reason the mode reports rather than merely
tolerates.

That splits the construct exactly where the danger is. **Every one of gap 30's
four bombs lives in the internal subset**: billion laughs is nested internal
general entities; the quadratic-blowup variant is one large internal entity
referenced many times; XXE is an entity *declaration*; the parameter-entity form
reaches the same place through the internal subset's own grammar. The external
identifier is a public and a system literal naming a DTD, and this engine
performs no I/O, so it names a file that will never be opened.

**The settlement is a mode with two values and no third:**

| | `Doctype::Refuse` (today's behaviour, and the default) | `Doctype::SkipExternalId` |
| --- | --- | --- |
| `<!DOCTYPE html>` | `DoctypeUnsupported` | skipped |
| `<!DOCTYPE html PUBLIC "…" "…">`, identifier in Appendix B | `DoctypeUnsupported` | the two literals read and **discarded** |
| `<!DOCTYPE html PUBLIC "…" "…">`, identifier outside it | `DoctypeUnsupported` | discarded, and the identifier **named in a warning** |
| `<!DOCTYPE html SYSTEM "…">` | `DoctypeUnsupported` | the literal read and **discarded** |
| `<!DOCTYPE html [ … ]>` | `DoctypeUnsupported` | **`InternalSubset`**, refused by name |
| the four bombs | `DoctypeUnsupported` | **`InternalSubset`**, refused by name |

Three properties this shape has that a "lenient" flag would not:

- **The four bombs are re-asserted under the new mode**, each by its own name,
  which is the test that says the defence survived the relaxation. A suite that
  only tested them under `Refuse` would prove nothing about the mode EPUB uses.
- **Nothing in either mode parses a declaration.** `SkipExternalId` skips to the
  matching `>` while tracking string literals so that a `>` inside one does not
  end it, and refuses on `[`. There is no entity table, no expander, and no code
  path that could be one refactor away from resolving an external entity — which
  was gap 30's stated reason for removing the class rather than the instance.
- **XPS is unaffected.** `xps.rs` passes `Doctype::Refuse` and its conformance
  claim under [M2.71] is unchanged, which is asserted rather than assumed.

**One thing is genuinely unresolved and milestone 1 settles it.** XHTML's named
character references — `&nbsp;`, `&mdash;`, `&hellip;` — are declared *by the
DTD this mode discards*, and are not among XML's five predefined entities. A
document using one without a declaration is not well-formed XML, and EPUB 3.3
does not require a reading system to fetch the DTD. Producers overwhelmingly
write `&#160;`, and none of the nine measured files uses a named reference
outside the five. **The question is how many real books do**, and the answer
decides between three options, which are recorded here so the choice is made
with evidence rather than by whoever meets the first `&nbsp;`:

1. refuse, by name, per XML 1.0 — correct and brittle;
2. a static table of the ~250 XHTML 1.0 Latin-1/special/symbol entities,
   vendored as data under `THIRDPARTY.md`'s existing precedent, with **each use
   warning**, because the document is relying on something EPUB does not
   guarantee;
3. HTML's full named-reference table, ~2 200 entries, which is a larger data
   commitment than the whole CMap registry for a case nobody has measured.

This plan's working assumption is (2), and milestone 1's corpus is what
confirms or refutes it. Milestone 2 does not build it until milestone 1 has
counted.

**Amended, 19 August 2026, milestone 1: it is refuted, and the answer is (1).**
Zero named references across all 270 content documents of both corpora, and the
corroboration this section expected is wrong too — these producers do not write
`&#160;` either, they write the character. So **milestone 2 refuses an
undeclared named reference by name, per XML 1.0**, and the ~250-entry table, its
`THIRDPARTY.md` section and its per-use warning are **not built**. The
measurement and the reasoning are in
[Progress — milestone 1](#progress--19-august-2026-milestone-1).

### OCF is not OPC, and a fifth thing the ZIP reader does not do

Gap 30 wrote `crates/tinker-pdf/src/xps/opc.rs` in the facade with the argument
that it *"would have exactly one consumer forever: EPUB is **not** OPC"*. That
was right, and it is confirmed from this side: **0 of 9 measured files carries
`[Content_Types].xml` or `_rels/.rels`.** None of `opc.rs` is reused. OCF's own
layer is `crates/tinker-pdf/src/epub/ocf.rs`, in the facade, for gap 30's own
reason: what is left after `zip` and `xml` is name arithmetic over
already-validated input.

What OCF needs that OPC did not:

**The `mimetype` rule, and it is not the check a first implementation writes.**
3.3 §4.3.2 requires a file named `mimetype`, holding exactly the ASCII string
`application/epub+zip`, that **is the first file in the ZIP archive**, is
**uncompressed**, and has **no extra field**. Measured across all nine files:
present, method 0, 20 bytes, general-purpose flags `0x0000`.

Three traps in one sentence:

- **"First file in the archive" is not `entries()[0]`.** `tinker-pdf-zip`'s
  `entries()` is central-directory order, and gap 29's own natural-sort section
  records that APPNOTE puts **no ordering requirement on the central
  directory**. The check is `header_offset == 0`, which is physical order and is
  what §4.3.2 means. A naive `index == 0` passes on every real book and is
  wrong.
- **"No extra field" is not readable today.** `Entry` exposes `name`, `method`,
  `crc`, both sizes, `encrypted`, `streamed`, `header_offset` and `index` — and
  **not** the local header's extra-field length. Either `Entry` grows a field or
  the OCF layer reads the local header from the archive's own bytes. This is the
  **fifth** thing `tinker-pdf-zip` does not do for a caller, after gap 30's four,
  and milestone 3 decides which way rather than discovering it.
- **The rule is a `MUST` and this engine degrades rather than fails (ruling
  2).** A book whose `mimetype` is deflated, or is not first, is **warned about
  and read anyway**, because a reader that refuses it has refused a book over a
  ZIP field that changes nothing about the content. The signature that decides
  the *format* is the presence of `META-INF/container.xml`, not the `mimetype`
  ordering — and that asymmetry is decided here rather than by whoever writes
  `ocf.rs`.

**`META-INF`'s six reserved names** (§4.2.6.3) are recognised by name.
`container.xml` is required; `encryption.xml` is read for font obfuscation and
**refuses the book** for any other algorithm; `signatures.xml`, `rights.xml`,
`manifest.xml` and `metadata.xml` are recognised and ignored, with the signature
non-goal stated above.

**Path restrictions** (§4.2.3) are OCF's own and are not OPC's part-name
grammar: no leading `/`, no `.` or `..` segments, case-sensitive comparison,
and a length limit on the whole path. Relative-reference resolution (§4.2.5)
resolves against the *referring document's* path, which is the same
off-by-one-segment gap 30's milestone 3 had to get right for `.rels`, and the
same fixture shape catches it.

*Amended, 19 August 2026, milestone 3.* **The sentence above is right about
§4.2.5 and wrong about the one reference milestone 3 actually resolves.**
`container.xml`'s `full-path` is defined by §4.2.6.3.1 as a path from the
**container root**, so it is the single reference in this format whose base is
not the document it is written in — resolving it against the referring document
yields `META-INF/EPUB/content.opf`, which no book on earth holds. The
off-by-one-segment analogy survives intact and points the other way: gap 30's
`.rels` targets resolve against the part that *owns* the relationships part
rather than the `_rels` directory it sits in, and here `full-path` resolves
against the root rather than the `META-INF` directory it sits in. Both are a
segment too many, and both fail in the direction that looks like a missing file.
The general rule is still the general rule, and it is what milestone 4's
manifest `href`s use.

**Two constraints OPC imposed that OCF does not.** OPC forbade encryption and
every method but DEFLATE; OCF permits neither restriction to be assumed. In
practice `tinker-pdf-zip` refuses an encrypted entry and refuses
`Method::Other` anyway, so the behaviour is identical and the *reason* is
different — which is worth a comment rather than an inherited assumption.

### Two crates, and why not one

**`tinker-pdf-css` is the ninth leaf and `tinker-pdf-layout` the tenth.**

Ruling 8's August 2026 amendment makes the test the definition rather than the
list: *"a leaf is any crate that takes bytes and plain parameters and returns
bytes and values, whatever the list says."*

`tinker-pdf-css` is a textbook one: stylesheet bytes in, a parsed stylesheet
out; an element and a selector in, a match and a specificity out; a set of
declarations and an element in, a computed style out. No PDF vocabulary, no
EPUB vocabulary, and no document tree of its own — the tree lives in the facade
and reaches the crate through a small `Element` trait (local name, attribute,
parent, previous sibling, index among siblings), which is what keeps the crate
from knowing what XHTML is.

`tinker-pdf-layout` takes a box tree of plain structs plus a `Metrics` trait
supplying advance widths and line heights, and returns positioned fragments.
Not "bytes in" — and ruling 8's definition says *"bytes and plain
parameters"*, which a tree of plain structs is. It holds no PDF types, no CSS
*syntax*, and no font: `Metrics` is how a caller supplies measurement, which is
what keeps `layout` off `font` and off `filters`.

**Why not one crate.** They are used together and never apart, which is the
argument for merging, and it is outweighed:

*They have different threat models and would need one fuzz target for two.*
`css` is a **parser** — untrusted bytes, ruling 1, a byte corpus, and a fuzz
target in the shape `xml` and `zip_archive` established. `layout` is an
**algorithm** — its inputs are already-validated structures, its failure mode is
unbounded work rather than a panic on bytes, and its fuzz target needs a
structured generator. A merged crate gets one target that can only exercise the
first, and the second is where the quadratic blowups live.

*They fail at different levels and the warnings must say which.* A stylesheet
that will not parse is a book-level fact; a box that could not be laid out is a
page-level one. Two crates make that split structural instead of conventional.

*And one of them is reusable and the other is not.* A CSS parser is what an
SVG-in-EPUB milestone, an HTML-to-PDF tool or a `@page`-aware writer would
want. A layout engine specialised to paginated flow is not.

**Why not the facade.** Both would then be unfuzzable independently — the
property plan 00 names as the whole point of a leaf — and both would sit above
`cos`, so a CSS bug would be reachable only through a book.

**The DAG amendment is the fifth and sixth.** `("tinker-pdf-css", &[])` — a
third empty allow-list, joining `filters`, `crypto` and `xml`, and the empty
list is a *finding* rather than an omission in exactly the register gap 30's
fourth amendment established: `css` was checked against what it might have
wanted and wants none of it. Not `xml`, because a stylesheet is not markup and a
`<style>` element's contents arrive as a byte slice. Not `math`, because
`calc()` is add, multiply and divide, and percentage resolution is division —
none of it transcendental. Not `font`, because `@font-face` *names* a face and
does not read one.

`("tinker-pdf-layout", &["tinker-pdf-math"])` is the third leaf-to-leaf edge and
the argument is ruling 4's rather than convenience: line-height rounding,
justification distribution and percentage resolution all reach pixels, and
ruling 4's amendment put `cargo xtask libm` behind the rule that a pixel-path
crate may not call a platform transcendental. Whether layout needs one at all is
**an open question milestone 7 answers**: if it does not, the edge is dropped
and the crate joins the empty-list group, and the answer goes in that
milestone's `As built` rather than being guessed at here.

Both are added to the facade's row. The leaf count goes from **eight to ten**,
in the four places [the ledger sweep](#the-ledger-sweep-milestone-13-owes) names.

### How a partial implementation is made visible

**This is the central design problem of this plan.** Ruling 10's typed
warnings are the mechanism; the hard part is *detecting* it, because a property
that is parsed and ignored is invisible. Five devices, in increasing order of
what they catch and decreasing order of obviousness.

**1. A property is a parser variant only when a consumer exists — enforced by
the compiler.**

The CSS parser's output is not a string-keyed map. It is

```rust
enum Declaration {
    /// A property this build implements, at a value it implements.
    Known(Property),
    /// A property this build knows the name of and does not implement, or one
    /// it implements at a value it does not.
    Unsupported { property: &'static str, value: SmallString },
    /// A name no CSS specification this build cites defines.
    Unknown { property: SmallString },
}
```

and every consumer of `Property` matches **exhaustively, with no `_` arm**. So
adding `float` to the parser without adding it to the block-formatting consumer
**does not compile**. That is gap 29's `const`-block device — where
`MAX_CBZ_PAGES < MAX_ZIP_ENTRIES` is checked at compile time so a bad relation
does not build — applied one level up, and it is the strongest rung available:
a test can be forgotten and a `match` cannot.

The distinction between `Unsupported` and `Unknown` is not decoration. `Unknown`
is a typo or a vendor extension and is ordinary; `Unsupported` is **this
build's own gap**, named, and it is what an `As built` has to count.

**2. The set is keyed by (property, value), not by property.**

`display: flex` implemented without `flex-wrap` is the failure this catches.
Each property registers the exact set of values it honours; a value outside it
is `Unsupported` even though the property is "supported". `float: inline-start`
is not `float: left`; `position: sticky` is not `position: relative`;
`text-align: justify` without a justifier is not `text-align: left`. A build
that maps an unhandled value onto its nearest handled one is producing gap 07's
solid-black gradient in a stylesheet.

**3. The warning names the property, the element and the stylesheet — and is
deduplicated per book, not per element.**

Ruling 10 requires provenance. A book with `float: left` on four hundred
elements must produce **one** warning naming the property with a count, not four
hundred — the contract `tinker_pdf_zip::Warning` and `tinker_pdf_filters::Warning`
already carry, and the reason gap 29 gave: a warning surface that is noise from
the first release destroys the distinction between "it opened" and "it opened
cleanly" before anybody has looked at it. The count is the number that matters:
*"`float`, unimplemented, affected 412 elements"* is a sentence a host can show
and a reviewer can act on.

**4. Text conservation, which is the invariant that survives every level of CSS
partiality.**

This is the one that makes an *intermediate* state honest, and it is the reason
milestone 4 exists before any CSS does.

> **Every character of text in every content document in the spine appears
> exactly once in the paginated output, in document order.**

That claim is checkable from milestone 4 onward — when the book is still
thirteen placeholder pages — and it stays checkable at every milestone after.
It does not depend on a single CSS property being right. What it catches is
precisely the class of bug a rendered comparison cannot see:

- a float that pushed content off the page bottom and lost it — **missing
  text**;
- a `display: none` honoured where it should not have been — **missing text**;
- a `display: none` *not* honoured — **extra text**;
- a fragmentation bug that repeated a paragraph across a page break —
  **duplicated text**;
- a spine item whose XHTML failed to parse and was skipped — **a whole chapter
  missing**, which is gap 29's page-renumbering hazard at book scale.

Every one of those produces a book that renders beautifully. `Page::text()`
already exists and already returns quads, so the check costs a comparison
against the source's own character stream — and the `As built` records the
conservation figure per book, which is a number rather than a claim.

**5. A layout oracle that compares *flow*, not pixels.**

Two independent layout engines do not agree to the pixel and this plan does not
promise they will. What they can be made to agree about is **which page a
sentence is on and where on it**, and that comparison is exact enough to catch
a dropped margin and loose enough to survive a rounding difference. See
[Oracles](#oracles-per-ruling-9); it is the device that catches what devices 1
to 4 cannot, which is a property that is implemented, honoured, and **wrong**.

**What none of the five catches**, stated rather than left for a reader to
find: a property implemented correctly in isolation whose *interaction* with
another is wrong — a margin that should have collapsed and did not, a float
that should have been cleared by a table's own block formatting context. Device
5 is the only one with a chance at those, and it is a comparison against
implementations that are themselves imperfect. **That residue is the honest
limit of this plan's honesty machinery**, and it goes in the `As built` rather
than being papered over.

### The cascade, and the parts of it that are easy to get subtly wrong

`css-cascade-5` §6.1 gives the sorting order and it is implemented whole rather
than approximated by "specificity then order", because the two most common
shortcuts are both wrong on real books:

1. **Origin and importance** — and the important-declaration reversal is the
   part a first implementation drops. §6.1's order is *transition, important UA,
   important user, important author, animation, normal author, normal user,
   normal UA*: an `!important` author rule loses to an `!important` UA rule,
   which is backwards from the normal case and is how a reading system keeps
   control of what it must.
2. **Context** — shadow DOM. Not applicable here and named so its absence is a
   decision.
3. **Element-attached styles** — `style=""` beats every selector, which real
   books use constantly.
4. **Layers** — `@layer`. Out of scope, refused by name at the at-rule, and
   named here because "unsupported at-rule ignored" would silently invert the
   cascade for a book that uses one.
5. **Specificity** — `selectors-4` §15's A/B/C tuple.
6. **Order of appearance** — last wins.

**A UA stylesheet is required and is a deliverable, not a detail.** An XHTML
document with no author CSS still has block-level `<p>` with margins, bold
`<h1>` at a larger size, italic `<em>`, and a list with markers. Without one,
every book renders as one undifferentiated run of text — which is *plausible*,
because it is readable. The UA sheet is written as CSS and goes through the same
parser and the same cascade as an author's, so it cannot drift into a second
code path; and it is committed, so what it says is reviewable rather than
scattered through the layout engine as defaults.

**Inheritance is per-property and is not a walk.** Computed values propagate
from the parent's *computed* value (§7.2), which means the cascade runs
top-down once and not per-lookup. An implementation that resolves inheritance
lazily on demand is quadratic in tree depth and produces the same answer, which
is why it is worth deciding here.

**Media queries are needed early, not late.** `@media print` and `@media
screen` both appear in real stylesheets, and a build that ignores `@media`
either applies every rule inside every block or none — both of which are
plausible and wrong. The `MediaContext` is a plain struct: `width` and `height`
from `OpenOptions::page`, `media` fixed at what this engine is (see below),
`resolution`, `color`, `orientation`.

**And a decision that has to be taken rather than defaulted: this engine
evaluates `@media` as `screen`, not `print`.** An EPUB is authored for a
reading system; §8 and the `rendition:*` vocabulary are about screens. That the
*output* is a PDF is an implementation fact about synthesis, not a statement
about the medium, and a book whose `@media print` block hides its navigation
would lose it. Recorded so it is not flipped by whoever notices the output is a
PDF.

### Layout: what a book actually needs, in the order a real one needs it

The temptation is to build blocks, then inlines, then "advanced" features. Real
books say otherwise, and the evidence is measured rather than assumed. The union
of CSS property names across the sixteen stylesheets in the six Project
Gutenberg books is **forty-one**, and every one is in the first two milestones'
worth of work:

> `background`, `background-color`, `border`, `border-bottom`, `border-left`,
> `border-right`, `border-top`, `clear`, `color`, `display`, `float`,
> `font-family`, `font-size`, `font-style`, `font-variant`, `font-weight`,
> `height`, `letter-spacing`, `line-height`, `list-style-type`, `margin`,
> `margin-bottom`, `margin-left`, `margin-right`, `margin-top`, `max-width`,
> `padding`, `padding-bottom`, `padding-left`, `padding-right`, `padding-top`,
> `page-break-after`, `page-break-before`, `text-align`, `text-decoration`,
> `text-indent`, `vertical-align`, `visibility`, `width`, `word-spacing`, and
> `all` — the last really used, as `all: inherit` and `all: initial`.

**How that list was taken, because the method matters to what it proves.** It is
a regular expression over `name:` in the stylesheet source, which is a rough
instrument: it emitted `a` as a false positive from `a:link` and `a:visited`
selectors, and it was removed by hand. So the list is a floor rather than a
census, which is exactly why milestone 1 makes it a deliverable of milestone 1's
own tooling rather than leaving it as this paragraph.

Three things that list settles even so:

- **`float` and `clear` are in the first stylesheet of the first book**, not in
  an advanced tier. Both appear in Project Gutenberg's default sheet.
- **`page-break-before` and `page-break-after` appear in every one of the six**,
  which means fragmentation is not something to bolt on after flow works: a book
  that ignores them puts chapter openings mid-page, which looks like a
  typesetting choice.
- **`all` is real**, and a build that treats it as an unknown property gets the
  cascade wrong for every element under it.

What the list does *not* contain is the reason this plan still builds tables and
flexbox: Project Gutenberg is one producer with one converter, and it is a poor
sample of what a publisher's EPUB does. Milestone 1's corpus has to include a
non-Gutenberg book precisely so this table is not the whole evidence — and gap
30 closed owing exactly that, having found no non-Windows producer.

**Fragmentation is the part with no PDF analogue and the largest unknown.**
Breaking a flow into pages means deciding where a block may be split, honouring
`break-before`/`break-after`/`break-inside`, keeping a float with its
paragraph, not orphaning a heading, and re-laying-out what crossed the boundary.
`css-break-3` is not in the CSS Snapshot's official definition, so the plan
cites CSS 2.2 §13.3's page-breaking rules as the normative floor and treats the
`break-*` longhands as the modern spelling of the same thing: §13.3.1's three
page-break properties, §13.3.2's `orphans` and `widows`, and — the part an
implementation built from the property list alone does not have — **§13.3.3's
rules A to D for where a break is permitted at all**. **Where this plan
is uncertain, it says so**: fragmentation of a table across a page boundary, and
of a flex container, are the two cases most likely to need their own staged
decision, and milestones 11 and 12 are written to permit one.

### Line breaking, and the Unicode data question

Line breaking is `css-text-3` §5.5, over **UAX #14**, and there is **no Unicode
data anywhere in this repository** — no `LineBreak.txt`, no property tables, and
the only UAX reference in the whole tree is [06](../06-content-and-text.md)'s
non-goal for bidi.

Three routes, and the third is taken:

1. **A crate.** `unicode-linebreak`, `xi-unicode`, `unicode-segmentation`.
   Forbidden by CONTRIBUTING rule 1, which has no exception for a line breaker,
   and all three are denied by name in the milestone that writes the breaker.
2. **An ASCII heuristic** — break at spaces and after hyphens. It works on every
   English fixture, works on Project Gutenberg's entire catalogue, and is
   catastrophically wrong on CJK, where there are no spaces and every character
   is a break opportunity. A book that lays out Japanese as one enormous
   unbroken line is not subtly wrong; but a build that ships the heuristic and
   is *tested only on English* is exactly this plan's named failure mode, so the
   heuristic is refused rather than staged.
3. **Vendored UCD data, compiled by a build script** — which this repository
   already does, once, with a precedent written down.

`THIRDPARTY.md` records `crates/tinker-pdf-font/data/cmap-resources`, Adobe's
CMap registry, *"vendored verbatim, with the licence that came with them, and
compiled into static tables by a build script — so the raw files never reach a
released binary"*, checked by `cargo xtask vendor` against `deny.toml`'s licence
allowlist. `LineBreak.txt` and `EastAsianWidth.txt` from the UCD are the same
kind of object: published facts about text that re-deriving would reproduce with
more mistakes.

**And the licence already passes.** The UCD is `Unicode-3.0`, which is in
`deny.toml`'s `allow` list today, so `cargo xtask vendor` accepts it with **no
amendment to the gate** — a small result, and the one that decides this is
buildable rather than blocked.

The breaker itself is ours: UAX #14's pair table over the vendored classes,
with the tailorings `css-text-3` §5.5 requires — *"line breaking behavior
defined for the `WJ`, `ZW`, `GL`, and `ZWJ` Unicode line breaking classes must
be honored"* — plus `line-break` and `word-break`'s four strictness levels
(§5.1) and `overflow-wrap` (§5.4).

**White-space processing is `css-text-3` §4.1.1 and §4.1.2 and is not
optional.** Phase I collapses runs and transforms segment breaks; phase II trims
and positions. XHTML source is indented, and every one of the nine measured
books has real inter-element whitespace. A build that does not collapse it draws
visible gaps between every inline element — visibly wrong, which is a mercy —
and a build that collapses too eagerly loses the single space between two
`<em>`s, which is invisible and is what actually happens.

### Fonts: `@font-face`, two obfuscations, and SHA-1 that is not here

**`@font-face` goes through `tinker-pdf-font` unchanged**, exactly as gap 30's
ODTTF did: the crate receives a plain byte slice that is already a valid sfnt,
and `Sfnt::parse` accepts `0x00010000`, `true`, `OTTO` and `ttcf`. WOFF and
WOFF2 are EPUB core media types (§3.2) and **are refused by name** — WOFF is a
container this engine has no reader for and WOFF2 additionally needs Brotli,
which is a second decompressor and a second denied crate.

**De-obfuscation lives in the facade**, for gap 30's reason: the key comes from
the *package document's unique identifier*, which is a publication concept, and
handing one to a leaf whose subject is font programs would make that leaf know
about books.

**Two algorithms, and they differ in three places rather than one:**

| | IDPF (3.3 §4.4.3, §4.4.4) | Adobe |
| --- | --- | --- |
| `encryption.xml` algorithm URI | `http://www.idpf.org/2008/embedding` | `http://ns.adobe.com/pdf/enc#RC` |
| Key | **SHA-1** of the UTF-8 unique identifier with U+0020, U+0009, U+000D and U+000A removed — 20 bytes | the UUID's 32 hex digits read as **16 bytes** |
| Bytes obfuscated | the first **1040** | the first **1024** |

1040 is 52 × 20 and 1024 is 64 × 16, so in both cases the key tiles the region
exactly and there is no partial final repetition — which is the arithmetic
detail that decides whether an implementation written from a summary is right,
and it is stated here for the reason gap 30 stated its ODTTF permutation: the
failure hides inside an existing, expected `UnreadableFont` warning, so **the
test asserts the de-obfuscated bytes and not that a page drew**.

**SHA-1 does not exist in this repository.** `tinker-pdf-crypto` holds `aes.rs`,
`handler.rs`, `md5.rs`, `rc4.rs` and `sha2.rs`, and SHA-1 is neither. It is new
code, hand-rolled under rule 1, and it goes in `tinker-pdf-crypto` beside
`md5.rs` — which is gap 29's CRC-32 decision in a second crate, taken for the
same reason and with the same discipline: pinned against published vectors, and
a second implementation written a different way asserted to agree, because a
hash written wrong produces self-consistent wrong answers that testing it
against itself cannot see.

**The font fallback chain is where a book without embedded fonts lives**, and
it is the part gap 28's "font fallback" phrase compresses into two words. It is
`css-fonts-4` §5's matching algorithm — family list, then style/weight/stretch
matching within a family, then **per-character fallback** (§2.1: *"a user agent
iterates through the list of family names until it matches an available font
that contains a glyph for the character to be rendered"*). Two consequences
this engine cannot dodge:

- **Fallback is per character, not per run**, so one run of text can need three
  faces, and each becomes its own PDF text object with its own font resource.
- **A character no available face covers must produce a named warning and
  not a blank**, because a book with silently missing characters reads as a book
  with typos.

Whether a font provider that answers `None` for a family should fall through to
the next family or to the generic is a question `FontProvider`'s current
two-state contract cannot express, and **milestone 9 either extends the trait
or records why it does not** — rather than picking one silently, which is the
shape of this whole plan's central risk.

### What is refused, and at which level

Ruling 2 degrades rather than fails, and gap 17's finding was that *the refusal
is the feature*. EPUB has **four** levels, one more than XPS, and the extra one
is where most of the honesty lives.

**Book level — refuse at `open`.** Not a ZIP; no `META-INF/container.xml`; a
`container.xml` that will not parse or names no `rootfile`; a rootfile that does
not resolve or is not a package document; a package document with no `spine` or
an empty one; an `encryption.xml` naming an algorithm that is not one of the two
obfuscations; a package version that is neither 2.0 nor 3.x; a bound spent. Each
gets its own `ArchiveRefusal` variant. Gap 29 made both `OpenError` and
`ArchiveRefusal` `#[non_exhaustive]` in one commit precisely so this plan would
cost additions and not a break, and gap 30 already added five without one.

**Spine level — a page, and the page count is unchanged.** A spine item whose
content document is missing, will not parse, or is a media type this build does
not read becomes **at least one page anyway**, of the book's own page box,
carrying the neutral placeholder and a named warning. This is gap 29's rule and
gap 30's, taken for their stated reason and with more force here: dropping a
spine item does not renumber a page, it **removes a chapter**, and a book that
jumps from chapter 4 to chapter 6 reads as a bad conversion rather than as a
bug.

**Element level — the asymmetry gap 30 decided, inherited whole.** Geometry
unreadable means the element is not painted and warns; paint unreadable means it
is painted in the neutral placeholder grey and warns. Gap 07's lesson: a
default that could be right is worse than a default that is visibly a default.

**Declaration level — the new one, and it is decision 5's.** A declaration that
does not reach a consumer is `Unsupported`, counted, and reported. It changes no
pixel by itself, which is exactly why it needs the machinery: it is the only
level at which the failure is *nothing happening*.

**And one asymmetry specific to this format, decided here.** A stylesheet that
will not parse is **not** a book-level refusal and **not** silently dropped:
`css-syntax-3`'s error recovery is normative — a malformed declaration is
discarded and parsing resumes at the next semicolon, a malformed rule at the
next block — so a stylesheet with one bad rule yields the rest, and the count of
discarded constructs is a warning. A build that refuses the sheet renders an
unstyled book that looks fine; a build that silently discards has no way to say
how much it discarded.

## Where a half-implementation is worse than none

Nine, and the first is already shipping.

**An EPUB opening as its cover.** Measured above: a thirty-one-chapter novel
opens, reports one page, and shows a 1824 × 2726-point cover. Nothing warns.
Gap 30 closed this for XPS and named EPUB in the same sentence. The defence is
milestone 3, and it is why that milestone is early.

**A property parsed and ignored.** The failure this whole plan is organised
around. There is no pixel, no warning and no test that fails — the page simply
lays out as though the declaration were not there. The defence is decision 5,
and it is a compiler check rather than a test because a test can be forgotten.

**A value mapped onto its nearest implemented neighbour.** `float:
inline-start` treated as `float: left` is right for a left-to-right book and
silently wrong for the one case it exists for. `position: sticky` as `relative`
is right until the element scrolls. Gap 07's solid-black gradient, in a
stylesheet. The defence is keying the implemented set by (property, value).

**A spine item that will not parse, dropped.** A whole chapter gone from a book
that reports a plausible page count and reads continuously across the hole. Gap
29's renumbering hazard at book scale, and text conservation is what sees it.

**Line breaking by spaces.** Correct on every English fixture, correct on all
six measured books, and catastrophic on CJK — where the failure is *one line per
paragraph*, so it is at least visible. The subtler half is that a build tested
only on English cannot tell the two apart, and Project Gutenberg is English.

**Fragmentation that repeats or drops the boundary block.** A paragraph that
appears at the bottom of page 7 and again at the top of page 8 reads as a
printing error; one that appears on neither reads as nothing at all. No render
comparison sees either. Text conservation sees both.

**Inheritance resolved lazily and inconsistently.** Two elements with the same
computed style laying out differently because one resolved its parent's value
before a later rule won. Produces a book that is *nearly* right, in a way that
looks like a rounding difference.

**`@media` blocks applied unconditionally.** A print stylesheet's
`display: none` on the navigation, applied to a screen rendering — or a screen
sheet's rules applied to nothing. Both produce a complete-looking book.

**A `with_fonts` that silently changes nothing.** A host attaches a provider,
the text appears, and the pagination is the one computed with Times metrics. The
defence is `OpenOptions::fonts` plus a named warning on the late path, decided
in [Where pagination comes from](#where-pagination-comes-from) before any file
exists.

## Bounds, per ruling 1

Every number below is attacker-controlled. Two scars set the form, and gap 29
built the machine that checks it.

`5adf502 fix(render): bound the group buffers a page may open, not just their
depth` found an 1 851-byte page that took **19.3 seconds to render 9 600
pixels**, with `MAX_GROUP_DEPTH` in place the whole time: *"depth is not work
once the recursion branches"*. The CSS version of that sentence is that **a
per-rule cap and a per-element cap are both meaningless when the cost is their
product**, and selector matching is a product.

[18a](18a-jpx-decoder.md)'s milestone 8 found the other failure, in a constant
written to avoid the first: `MAX_JPX_WORK` was set *above* the most its own
inputs could ask for, so it could **never fire**.

| Name | Bounds | Why it cannot be a per-item cap |
| --- | --- | --- |
| `MAX_CSS_BYTES` | One stylesheet's source | — (per-item, and says so) |
| `MAX_CSS_TOKENS` | **A work cap.** Tokens produced across every stylesheet in one book | A per-sheet cap times a file-chosen sheet count is not a bound |
| `MAX_CSS_RULES` | **A work cap.** Qualified rules admitted across the book | The same, and it is the left-hand factor of the product below |
| `MAX_CSS_DECLARATIONS` | **A work cap.** Declarations admitted across the book | The same |
| `MAX_CSS_SELECTOR_PARTS` | Compound selectors in one complex selector | — (per-item; it bounds one match attempt's cost) |
| `MAX_CSS_IMPORT_DEPTH` | `@import` nesting, with a cycle guard | A cycle is two lines of CSS; a depth cap without the guard loops |
| `MAX_SELECTOR_MATCHES` | **The work cap of the cascade.** Selector-against-element attempts across the whole book. **Amended, 20 August 2026, milestone 6: it counts *compound*-against-element tests, not selector-against-element attempts.** Matching `a b c d` walks the ancestor chain with backtracking, so one attempt is `O(depth^parts)` compound tests and a cap on the attempts bounds a number that is not the work — `5adf502`'s sentence one level further down than this table had it. `MAX_CSS_SELECTOR_PARTS` bounds one attempt's shape and this bounds its cost, and they are different claims | `MAX_CSS_RULES` × `MAX_DOM_NODES` is the product, and **neither factor bounds the other**. This is `5adf502`'s sentence in its purest form and it is the single most important constant in this plan |
| `MAX_DOM_NODES` | Elements admitted from one content document | — (per-document). `MAX_XML_TOKENS` stands in front of it and is a million, so this must sit below that or it can never fire |
| `MAX_BOX_TREE_NODES` | **A work cap.** Boxes across the book | Boxes are not elements: anonymous block generation, `::before`/`::after` and table-structure fixup (CSS 2.2 §17.2.1) each create boxes the document did not write |
| `MAX_LAYOUT_WORK` | **The work cap of layout.** Box-layout operations across the book. **Amended, 20 August 2026, milestone 7: it is not in this build and the absence is argued where it would be declared.** The sentence to the right is right about a build with a second pass, and milestone 7 has none: every unit of layout work is one box or one line box, boxes are bounded by `MAX_BOX_TREE_NODES` and line boxes by `MAX_LINE_BREAK_WORK`, because a line box needs a character and every character is charged before the breaker is entered. A cap here would sit above what its own inputs can ask for — gap 18a milestone 8's failure — or below the box cap, where it would be the box cap under another name. It was written, its firing test was attempted, and it could not be made to fire without lowering itself. **It arrives with the multi-pass layout of milestones 10 and 11 or not at all**, which is the sentence `MAX_EPUB_PAGES` carried at milestone 4 and `MAX_XPS_VISUAL_DEPTH` carries in `bounds_ledger.rs`. Three quadratics were removed rather than charged for in the same milestone | A per-box cap is not a total once the file chooses the box count *and* the pass count: automatic table layout is two passes (§17.5.2.2), float placement re-flows a line, shrink-to-fit measures twice, and a nested table multiplies all three |
| `MAX_LINE_BREAK_WORK` | **A work cap.** Break opportunities evaluated across the book | The same shape one level down |
| `MAX_EPUB_MANIFEST_ITEMS` | Manifest items admitted | Must sit **below** `MAX_ZIP_ENTRIES`, or the archive refuses first and this can never fire |
| `MAX_EPUB_SPINE_ITEMS` | Spine itemrefs | Must sit below `MAX_EPUB_MANIFEST_ITEMS` for the same reason |
| `MAX_EPUB_PAGES` | Pages fragmented out of the book | **Deliberately *not* in the relation above**, and this is the trap. Gap 30 found `MAX_XPS_PAGES` was not bounded by `MAX_XPS_PARTS` because four thousand `PageContent` elements may name one part. Here it is worse: **one** spine item of 128 MiB of text fragments into as many pages as its length divided by the page height, so the page count is bounded by *content length* and not by item count at all. **Amended, 19 August 2026, milestone 4: it arrives with milestone 7's fragmentation and not before.** Milestone 4 puts exactly one page on each `<itemref>`, so a cap above `MAX_EPUB_SPINE_ITEMS` could never fire and one below it would be the spine cap under another name — gap 18a milestone 8's failure reached from the direction that writes the constant first, and the argument `bounds_ledger.rs` already carries for `MAX_XPS_VISUAL_DEPTH`'s absence. The sentence to the left is right about the build that fragments and is a constant that cannot fire in the one that does not. **Amended again, 20 August 2026, milestone 7: it has arrived, as `MAX_LAYOUT_PAGES` in `tinker-pdf-layout`.** It is declared there rather than in `epub.rs` for `MAX_DOM_NODES`'s reason one milestone earlier — a cap belongs where the thing it bounds is decided, and pages are decided by `layout::fragment` — and the ledger row carries the argument |
| `MAX_EPUB_FONTS` | Faces admitted from `@font-face` and the manifest | Each costs an embedded font program and a subset |
| `MAX_SYNTHESISED_PDF` | Bytes handed to `CosDocument::open` | Already exists in `cbz.rs`; reused rather than duplicated, and the ledger says so |

Per-item caps sit beside them and the comment on each says in as many words
that it is *not* a work cap, in the register `MAX_SCRIPT_STEPS`,
`MAX_MESH_TRIANGLES`, `tinker-pdf-zip`'s `limits.rs` and gap 30's XPS constants
already use.

**Four deliberately absent, argued where they are declared** — three when this was written and a fourth added by milestone 6 — because gap 29
established that writing down why a cap was *not* added is the cheaper half of
this discipline:

- **Nothing on a stylesheet's block nesting**, added 20 August 2026 by
  milestone 6 and argued where it is declared: a `{`-in-`(`-in-`[` chain is
  bounded by `MAX_CSS_TOKENS` for allocation and by a plain 256 for the
  *recursion*, which is `MAX_XML_DEPTH`'s number taken for `MAX_XML_DEPTH`'s
  reason — a stack, not a budget — and a construct past it is a malformed one
  the layer above already discards. It is not a ledger row because it bounds no
  allocation a cap already here does not.

- **Nothing on DOM depth.** `tinker_pdf_xml::limits::MAX_XML_DEPTH` is 256 and
  stands in front of every content document, so a separate constant could never
  fire — gap 18a milestone 8's failure reached from the other direction, and the
  same argument gap 30 used for `MAX_XPS_VISUAL_DEPTH`'s absence.
- **Nothing on de-obfuscation.** 1 040 XORs against a buffer `MAX_ZIP_ENTRY_BYTES`
  has already bounded, allocating nothing.
- **Nothing on OCF path depth**, for `tinker-pdf-zip`'s stated reason: nothing
  here touches a filesystem, so depth bounds no allocation and no recursion, and
  length is what costs.

**The relation goes in a `const` block**, so a build that breaks it **does not
compile**:

```text
MAX_EPUB_SPINE_ITEMS < MAX_EPUB_MANIFEST_ITEMS < MAX_ZIP_ENTRIES
MAX_DOM_NODES        < MAX_XML_TOKENS
MAX_CSS_RULES × MAX_DOM_NODES > MAX_SELECTOR_MATCHES
```

The third is the interesting one and it is written the opposite way round from
the other two: it asserts the product is **larger** than the cap, which is
`every_bound_can_fire`'s check promoted to compile time for the one constant
whose reachable ceiling is a product rather than a field width. Gap 29's device,
applied to the number this plan is most likely to get wrong.

**How each is measured.** Three figures per constant go in the `As built`: the
most any fixture in this repository legitimately spends, the most a plausible
real book spends, and the constant. The yardstick has to be named or the second
figure is a mood, so: **a 400-page novel of 120 000 words in 40 spine items,
with four stylesheets totalling 40 KB and two embedded faces** — measured
against milestone 1's corpus rather than invented, and corrected there if the
corpus disagrees. Each cap is proved to fire **by its own refusal or warning,
never by a clock** — `5adf502`'s method, for its stated reason.

### What this inherits from `bounds_ledger.rs`

`crates/tinker-pdf/tests/bounds_ledger.rs` has **seventeen** rows and five
checks, and every constant above joins **that** table rather than getting a
sweep of its own — the whole value of it is that it is one table. It inherits
all five:

1. **`every_bound_can_fire`** — the constant is below the most its own inputs
   can ask for. Gap 29's matrix records it as the **only** thing in the
   workspace that has ever caught `MAX_JPX_WORK`'s failure.
2. **`every_bound_publishes_the_number_it_is`** — the number in the ledger
   comment parses back to the `const`.
3. **A plausible real document fits under every one.** That test carries two
   yardsticks today, gap 29's comic and gap 30's fixed document, and gap 30's
   milestone 9 had to amend it because seven rows opted out with a `None` and
   *"a row that opts out of a check is a row that is not checked"*. **This plan
   adds a third yardstick and no row opts out**, which means the seventeen
   existing rows each acquire a book figure in the same milestone.
4. **`every_bound_names_a_test_that_exists`** — the named firing test exists, is
   a `#[test]`, and is not `#[ignore]`d.
5. **Nothing is proved by a clock.**

Two fuzz targets land with the code that needs them rather than at the end, per
plan 02's rule: `css` as the repository's **twenty-second**, and `layout` as its
**twenty-third** — the latter over a structured generator rather than a byte
corpus, which is the shape difference that argued for two crates. Each carries
the target's control byte in front, and **at least two of the five seeds set
every knob to the tightest value the target offers**, because a corpus in which
every seed is roomy explores the happy path and reaches no refusal.

## Oracles, per ruling 9

Ruling 9: *"mutool, pdftoppm, pdfium_test and qpdf are invoked as external CLIs
in CI; nothing links them, and their outputs are transient comparison
references, never committed or redistributed."* All four names are relevant, one
of them is much weaker here than in gap 30, and this plan proposes a fifth.

**qpdf, on the produced PDF.** Gap 29's and gap 30's route, unchanged, through
the CI job [20](20-linearization-validation.md) already built. This plan writes
object structure neither predecessor did — `/Annots` with link annotations and
an `/Outlines` tree — so there is again strictly more that only qpdf can see.
Gap 29's milestone 5 found every page sharing one image resource table, a defect
**qpdf alone** caught because the renderer drew the right picture either way.

**epubcheck, on the EPUB itself — and it checks the *input*, not this
engine.** `epubcheck` is the W3C's conformance checker, maintained by the DAISY
Consortium, a standalone Java command-line tool at v5.3.0, and it validates
EPUB 2 and 3 against EPUB 3.3. What it gives is exactly one thing and it is
worth having: **when this engine and a book disagree, epubcheck says whose fault
it is.** A book that epubcheck rejects is a book this engine is entitled to
refuse; a book it accepts and this engine mis-reads is this engine's bug. That
turns milestone 1's corpus from "files" into "files with a verdict".

What epubcheck cannot do is check a renderer. It has nothing to say about
whether a paragraph is on the right page. Saying so is better than gesturing at
it as though the W3C shipped a layout checker.

**mutool, on the EPUB — and the irony gap 30 recorded now cuts the other
way.** MuPDF reads EPUB; `mutool draw` lists `pdf`, `xps`, `cbz` and `epub`, and
takes `-W`, `-H`, `-S`, `-U` and `-X` for EPUB layout specifically. It is
already one of ruling 9's four, `mupdf-tools` is an apt package, and nothing
links it. So the same package gap 28's decision removes from Tinker's shipped
tree is again the closest available oracle.

**But the caveat is much stronger than gap 30's and it must be stated rather
than discovered.** Gap 30 wrote: *"where MuPDF is wrong about XPS this engine
will agree with it and both will be wrong"* — a bounded risk, because XPS
markup has one right answer. **CSS does not work like that.** MuPDF's EPUB
layout engine is itself a partial CSS implementation, so:

> **For XPS, agreeing with mutool was evidence. For EPUB, disagreeing with
> mutool is not evidence of a bug.**

That is a real weakening and it is why this plan proposes a fifth oracle rather
than leaning on the fourth.

**A headless browser, as the fifth — and ruling 9 is amended to name it.**
A browser is the reference implementation of CSS; comparing a CSS implementation
against a partial one is comparing it against nothing. Adding a fifth oracle is
a ruling amendment and is done in writing rather than quietly, in the milestone
that adds it. It stays an amendment rather than an exception because it changes
nothing about the rule's substance: invoked as a subprocess, never linked, its
output transient and never committed.

**What a browser comparison would and would not prove**, worked out rather than
asserted, because the obvious comparison is the wrong one:

- **A page-by-page pixel diff is impossible.** A browser lays a content document
  into one continuous column, not into pages, so there is no page 3 to compare
  against page 3.
- **What *is* exact is the continuous comparison.** Render one content document
  in a headless browser at exactly `OpenOptions::page.0` wide with no
  pagination, extract every text run's `y` offset and text, and compare against
  this engine's own layout of the same document before fragmentation. **The
  cascade, the box model, floats, tables, flex and line breaking all show up as
  a `y` offset**, and a single dropped margin displaces everything below it. That
  is the strongest signal available anywhere in this plan and it does not depend
  on a rasteriser at all.
- **And a paginated comparison is available too.** Chromium's `--print-to-pdf`
  paginates at a page size the caller chooses, so the same document at 432 × 648
  gives a page count and per-page text to compare against. That checks
  fragmentation, which the continuous comparison cannot.
- **What neither proves is pixels.** Two rasterisers, two hinting policies, two
  anti-aliasers. Gap 18a pre-argued the same point for a fixed-point wavelet
  against a float reference, and this plan does not promise byte equality with
  anything.
- **And the honest limit:** a browser's default UA stylesheet is not this
  engine's, so the comparison is only meaningful with a UA sheet injected on
  both sides. That is a real cost and it is milestone 8's to pay.

**Whatever the CI job is, it greps its own output.** Gap 20 found that a skipped
oracle test exits 0 and reads exactly like a pass; the `qpdf-oracle: RAN` /
`SKIPPED` pattern is copied verbatim for every new job, and each goes **red**
when its tool is missing. That matters more here than in either predecessor,
because — see the next section — **the real books are not committed**, so a test
over them can silently not run for a second reason as well.

## Real books, and which milestone owns getting them

Gap 29 closed having never opened a real `.cbz`. Gap 30 answered that
structurally, by buying eight real packages in milestone 1 *before* writing a
reader, and its Progress sections record **nine** things real files did that
ECMA-388 did not predict. **This plan does the same and its milestone 1 has
already found six**, which are the measurements in
[What is wrong](#what-is-wrong), the doctype split, the forty-one-property union,
Project Gutenberg's one-image-per-book habit, the `header_offset`-versus-`index`
trap and the missing extra-field accessor.

**But this plan cannot copy gap 30's answer on committing them, and the reason
is the licence gate.**

| Source | Licence | Committable? |
| --- | --- | --- |
| Project Gutenberg | Public-domain text under a **trademark** licence. Clause 1.E.1 requires the boilerplate notice to *"appear prominently whenever any copy … is accessed, displayed, performed, viewed, copied or distributed"*; 1.E.4 forbids detaching the licence terms | **No.** Clause 1.C permits it *"as long as all references to Project Gutenberg are removed"*, which is legal, fiddly, and leaves a fixture nobody can trace |
| IDPF/W3C `epub3-samples` | **CC-BY-SA 3.0** | **No** — and this is the finding worth recording. `deny.toml` says *"There is deliberately NO copyleft in this list — not even weak copyleft"*, and share-alike is weak copyleft. **The obvious source of committable EPUBs is barred by this repository's own rule**, which is a better outcome than discovering it in review |
| W3C `epub-tests` | Unverified. Probably the W3C Software and Document Licence | **Milestone 1 checks**, and records the answer either way |
| A real producer's output on our own text | Ours, under `fuzz/README.md`'s existing precedent | **Yes** — see below |

**The committable route is gap 30's, and it is the same precedent.**
`fuzz/README.md` records that the JPX seed corpus holds *"codestreams
`opj_compress` made from **our own** 32 × 32 images"*, under the reading that a
tool's output on our input is ours to commit, while ISO's conformance
codestreams stay out. Gap 30 used it for Windows' XPS serialiser. Here it means:
**author the text in this repository, run it through a real EPUB producer —
Calibre, Sigil, `pandoc` — and commit the output.** The result is a genuine
producer's idea of an EPUB, with that producer's OPF conventions, its container
layout, its stylesheet and its doctype habits, over content nobody else owns.

So milestone 1 delivers **two corpora with different jobs**, and the split is
stated rather than blurred:

- **Committed, and CI runs on it**: at least four books from at least two real
  producers, generated from text authored here. This is what every later
  milestone's fixtures come from.
- **Fetched, never committed, and CI *knows* when it did not run**: Project
  Gutenberg, `epub3-samples`, and whatever else milestone 1 can reach — behind a
  fetch script and an env-var gate that prints `epub-corpus: RAN` or `SKIPPED`,
  greppable exactly like gap 20's qpdf job. This is what proves the reader
  against files this repository did not commission.

**And the thing gap 30 closed owing is written into row 1 here.** That plan's
criterion asked for one package produced by something that is not Windows *"if
one can be found"*, and none was, so its corpus is one vendor's idea of the
format. The equivalent risk here is that every book is one converter's, and
**Project Gutenberg's is one converter's** — six books, one `ebookmaker`, forty-one
CSS properties between them. Milestone 1's criterion therefore names **two
producers minimum** and is not satisfiable by six Gutenberg books.

**One more correction learned from gap 30, applied before it can bite.** That
plan's milestone 1 committed two `#[ignore]`d tests to pin the present-day
failure, and its milestone 3 progress records that the exit criterion *"forced
the spine a milestone early"* because the test was written with `.expect`, so
the only way it could go green was for the book to open. **Milestone 1's pinned
tests here are written to accept either a correct read or a refusal by a name
that is true**, and the milestone table says which milestone turns which into
which — because a pinned test that over-specifies the fix schedules work its own
row did not ask for.

## `deny.toml` has a hole exactly here

Gap 29 denied eleven names for ZIP and CRC; gap 30 denied nine for XML, and
wrote *"the window in which the temptation exists is exactly the milestone that
writes the parser"*. `deny.toml` denies sixty-six crates today and **not one is
a CSS crate, an HTML crate, a line breaker or a Unicode-data crate.**

Denied in the milestone that writes each, not in the milestone that finishes the
plan:

- **With `tinker-pdf-css`** — `cssparser`, `lightningcss`, `selectors`,
  `simplecss`, `css-color-parser`, `stylo`.
- **With the element tree** — `html5ever`, `markup5ever`, `scraper`, `kuchiki`,
  `kuchikiki`, `tl`, `lol_html`, `html5gum`.
- **With `tinker-pdf-layout`** — `taffy`, `stretch`, `yoga`, `morphorm`.
- **With the line breaker** — `unicode-linebreak`, `xi-unicode`,
  `unicode-segmentation`, `unicode-width`, `unicode-bidi`, `icu_segmenter`,
  `harfbuzz_rs`, `rustybuzz` (already denied for fonts, and it is the shaping
  crate a line breaker's author reaches for next).
- **With SHA-1** — `sha1`, `sha-1`, `sha1_smol`, `hmac-sha1`.

`serde` stays out of the list for the reason gap 30 recorded: denying a
general-purpose crate on the grounds that it is often seen near a format would
make this file about taste rather than about rule 1.

**And `THIRDPARTY.md` gains a second vendored tree**, `LineBreak.txt` and
`EastAsianWidth.txt` from the UCD under `Unicode-3.0` — which `deny.toml`'s
licence allowlist **already permits**, so `cargo xtask vendor` passes with no
amendment. That was checked rather than assumed and it is the single fact that
makes UAX #14 buildable here rather than blocked.

## Milestones

The commit-boundary rule is per-plan
([00-execution-order.md](00-execution-order.md)); this one is thirteen commits,
one per milestone, each independently green under the full gate.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | **Real books, before any reader** | Two corpora with the split above: **at least four books from at least two real producers, generated from text authored in this repository** and committed under `fuzz/README.md`'s "our input through their tool" precedent, with per-file provenance and a regenerating script beside them; **and** a fetch script for Project Gutenberg and `epub3-samples`, committed, fetching nothing into the tree, whose test prints `epub-corpus: RAN` or `SKIPPED` and whose CI job goes red on `SKIPPED` — because a corpus that is not committed can fail to run for a reason gap 20's qpdf job already taught this repository to grep for; `epubcheck` run over every book in both corpora with its verdict recorded per file, so a later disagreement has a party to blame; a checked-in inventory naming every entry, its media type, its compression method, its `header_offset` and its size, recomputed on every run through `tinker-pdf-zip` so it cannot drift; **the doctype census** — how many content documents in the fetched corpus carry one and in which of the three shapes — and **the named-character-reference census** that decides between the three options in [The doctype collision](#the-doctype-collision); the CSS property census across every stylesheet in both corpora, which is what replaces this plan's forty-one-name list with evidence; **and the present-day failure pinned as tests that fail today, written to accept either a correct read or a refusal by a name that is true** — gap 30's milestone 3 correction, applied before it can bite | M |
| 2 | **`tinker-pdf-xml` grows a doctype mode, and the bombs are re-proved under it** | `Doctype::Refuse` and `Doctype::SkipExternalId`, two values and no third; `Refuse` is the default and `xps.rs` passes it explicitly, with a test asserting XPS's [M2.71] behaviour is unchanged; under `SkipExternalId` all three doctype shapes measured in milestone 1's census are skipped, including the single-quoted XHTML 1.1 form and a `>` inside a public literal; **an external identifier outside EPUB 3.3 Appendix B's closed set named in a warning rather than refused or silently accepted**, asserted in both directions — SVG 1.1's is in the set and XHTML 1.1's is not, and a build that warns about neither passes a one-sided test; **an internal subset `[` refuses by its own name, `InternalSubset`**, and **all four of gap 30's committed bombs are re-asserted under the new mode, each by that name** — the test that says the defence survived the relaxation; no entity table and no expander exists in either mode, asserted by the diff; the named-character-reference decision from milestone 1's census implemented, and if it is the vendored table then `THIRDPARTY.md`, `cargo xtask vendor` and the per-use warning land with it; every existing `tinker-pdf-xml` test unchanged | S |
| 3 | **OCF, the discrimination, and a book that is no longer a cover** | `Document::open` routes a ZIP by container: XPS first (gap 30's E.3, untouched), then **EPUB by the presence of `META-INF/container.xml`**, then CBZ as the fallthrough — and a comic archive that happens to carry a `META-INF/` directory is still a comic, from a fixture built for it; **milestone 1's pinned tests resolve**, and the passing test that records today's wrong answers is **deleted in this commit**, which is the point of having written it; `mimetype` checked per §4.3.2 with **`header_offset == 0` and not `index == 0`**, proved by a fixture whose central-directory order and physical order disagree, and a violation **warning rather than refusing** per the decision above; the extra-field question settled one way or the other and the choice recorded; `container.xml` parsed and its `rootfile` resolved, with §4.2.3's path restrictions and §4.2.5's relative-reference resolution against the *referring document*; the six `META-INF` reserved names recognised, `encryption.xml` read far enough to refuse a non-obfuscation algorithm **by name**; every book-level `ArchiveRefusal` variant returned by a fixture built for it; `Archive::read`'s inflation budget spent once per part, proved by asserting `inflated()` does not grow on a second resolution | M |
| 4 | **The package document, the spine, and an honest blank book** | `package` (§5.4), `metadata` with the three required Dublin Core elements (§5.5.3.1) including the `unique-identifier` milestone 9's obfuscation key needs, `manifest`/`item` with `properties` (§5.6.2), `spine`/`itemref` (§5.7), manifest fallbacks (§3.5.1) and core media types (§3.2); EPUB 2.0's OPF read as a compatibility surface and any other version refused by name; **`OpenOptions` and `Document::open_with` land here**, with `open(bytes)`'s signature unchanged and asserted so; `page_count()` equals the spine item count and every page is the box `OpenOptions` states, at 432 × 648 by default; **every page renders the neutral placeholder with a named warning rather than white**, because an empty page reported as success is what gap 17 spent itself on; a spine item that does not resolve still produces a page and keeps its position; **`with_fonts` on a reflowable document warns by name**; `Document::cos()` returns the synthesised document and **saving it produces a file qpdf reads clean**; **the text-conservation harness exists and is asserted here, against placeholders**, so every later milestone inherits it rather than acquiring it | M |
| 5 | **The writer's missing half, before anything needs it** | Link annotations: `/Annots` with `/Subtype /Link`, `/Rect`, `/Border [0 0 0]` and both an explicit `/Dest` and a `/A` URI action, since an EPUB cross-reference is internal and an `href` may be external; a document outline: `/Outlines` with `/First`, `/Last`, `/Count` and the negative-count closed form, nested to a stated depth; both round-tripped through **this repository's own reader** — write it, open it, read it back through the facade's existing outline and annotation surfaces — which is the comparison [21](21-metadata-absent-vs-empty.md) and the annotation work make possible and no other milestone here can; qpdf clean on a document using both; `DocumentBuilder`'s existing validation posture extended rather than bypassed; **nothing in this milestone mentions EPUB**, which is the test of whether it belongs in the writer | M |
| 6 | **`tinker-pdf-css`, the ninth leaf** | `css-syntax-3`'s tokenizer and the qualified-rule/at-rule grammar, **with its normative error recovery** — a malformed declaration discarded to the next semicolon, a malformed rule to the next block, and the count of each reported rather than swallowed; `selectors-4`'s type, universal, class, id, attribute (§6.1–§6.4) and the four combinators (§14), with **specificity per §15 asserted against a table of at least twenty selectors including the cases that trip a naive A/B/C** — `:not()`'s argument, `:is()`'s most-specific-argument rule, and a pseudo-element's C contribution; matching against a caller-supplied `Element` trait, so ruling 8 holds and no XHTML vocabulary is in the public API; the **whole** of `css-cascade-5` §6.1's sorting order including the `!important` origin reversal, with a fixture per criterion; inheritance as a single top-down pass over computed values (§7.2), with a test that a lazy resolution and this one agree — and a note saying the lazy one is quadratic; `@import` with the depth cap and a **cycle refused rather than recursed**; `@media` evaluated against a plain `MediaContext`, **as `screen`**, with the decision's argument in the module header; **decision 5's `Known`/`Unsupported`/`Unknown` split, and a compile-time proof that a property with no consumer does not build** — injected as a defect and asserted to fail the build, not a test; `@layer` refused by name; every bound firing by its own refusal; `xtask -- dag` green with the fifth amendment's argument; the **twenty-second** fuzz target — **amended, 20 August 2026, milestone 6: the twenty-third**, because gap 24's milestone 5 split `crypt_ciphers` out of `crypt` after this plan was written and took the number; the count in `fuzz/README.md` and in `ci.yml`'s per-PR job goes twenty-two to twenty-three, not twenty-one to twenty-two; `deny.toml` gains the CSS and HTML names | L |
| 7 | **`tinker-pdf-layout`, the tenth leaf** | The box model (`css-box-3`) with `box-sizing`, and **margin collapsing**, which is the rule a first implementation omits and whose omission moves every block on every page; block and inline formatting contexts (CSS 2.2 §9.4.1, §9.4.2) and line boxes; `css-text-3` §4.1.1 and §4.1.2's white-space processing in both phases, asserted against a fixture whose source is indented the way milestone 1's real books are; **UAX #14 line breaking over the vendored UCD tables**, with `css-text-3` §5.5's required class behaviour for `WJ`, `ZW`, `GL` and `ZWJ`, §5.1's four strictness levels and §5.4's `overflow-wrap` — and a **CJK fixture**, because a space-only breaker passes every English test ever written; §6's alignment and justification; fragmentation into pages, honouring CSS 2.2 §13.3.1's properties, §13.3.2's `orphans` and `widows` and **§13.3.3's rules A to D for where a break is permitted at all**, with `page-break-before` and `page-break-after` asserted because they appear in all six measured books; the `Metrics` trait, so nothing here depends on `font`; whether `math` is needed answered in the `As built` and the DAG edge dropped if it is not — **answered, 20 August 2026, milestone 7: it is not needed and the edge is dropped**, and an edge to `tinker-pdf-css` is taken in its place, both argued as the sixth DAG amendment; the **twenty-third** fuzz target — **amended, 20 August 2026, milestone 7: the twenty-fourth**, because milestone 6's own amendment took the twenty-third for `css` — over a structured generator; `deny.toml` gains the layout and line-breaking names | L |
| 8 | **The first book that reads** | XHTML through `tinker-pdf-xml`'s new mode into an element tree; **a committed UA stylesheet**, parsed by milestone 6's parser and cascaded like an author's, with a test that removing it produces an undifferentiated book — so its absence is visible rather than merely worse; the cascade over the tree, layout, fragmentation and synthesis into a `CosDocument`; **every book in the committed corpus opens, paginates and passes text conservation**, with the conservation figure recorded per book; `Page::text()` returns the words in reading order; cross-references between spine items reach the page as milestone 5's link annotations, and the navigation document as the outline; qpdf clean; **the browser oracle stands up here** — ruling 9 amended in writing with its argument, the continuous `y`-offset comparison built with a UA sheet injected on both sides, the paginated `--print-to-pdf` comparison beside it, and the job red when the browser is missing — **amended, 21 August 2026, milestone 8, in two places and both measured rather than assumed.** (a) The paginated comparison is made at **the browser's own default page box** and not at 432 × 648: Chromium honours an `@page { size: … }` for the output page and lays the document out at its own box anyway, then scales — asked for 432 × 648 it wrote a 432 × 648 page whose body text is set at 8.69 points rather than 12, which is 576/792, this page's height over US Letter's. A page count compared across that scale is a comparison of two different documents, so the comparison moves to the box the browser is not scaling and asserts the text size to keep that honest. (b) **A face is held fixed on both sides** as well as a UA sheet, because two faces disagree by about a line per paragraph and a line is worth about as much as a margin: with each side using its own, the tolerance needed (0.033) was larger than the injected dropped-margin defect it exists for (0.033), which is this plan's own *"thresholded into meaninglessness"*. With `Courier New` declared on both, the honest disagreement is 0.036 and the same defect measures 0.105; the census printed per book, which is the number this milestone is actually judged on | L |
| 9 | **Fonts** | `@font-face` (`css-fonts-4` §4.1) and the `src` descriptor (§4.3) with `format()` and the fallback list; the font matching algorithm (§5) including **per-character fallback** (§2.1, §5.3), with a fixture whose one run needs three faces and becomes three PDF text objects; **SHA-1 in `tinker-pdf-crypto`**, pinned against published vectors, with a second implementation written a different way asserted to agree over every length up to two blocks — gap 29's CRC-32 discipline, because a hash written wrong is self-consistently wrong; **both de-obfuscations, asserted on the de-obfuscated bytes and not on a page that drew** — IDPF's SHA-1 key over 1 040 bytes and Adobe's 16-byte UUID key over 1 024, each from a fixture built for it, with the whitespace-stripping of §4.4.3 proved by an identifier that has some; WOFF and WOFF2 refused **by name**; a character no available face covers producing a named warning rather than a blank; `FontProvider`'s per-family fallback question answered — the trait extended, or the reason it is not recorded; the generic families' standard-14 metrics asserted to make pagination independent of whether a provider is attached | M |
| 10 | **Floats and `clear`** | CSS 2.2 §9.5.1's **nine numbered constraints, each with its own fixture**, because they are a set and an implementation that satisfies eight produces a page that looks right on the ninth's absence; §9.5.2's `clear` and clearance; float interaction with line boxes — a line box shortened beside a float and restored below it; a float taller than its containing block; two floats that do not fit side by side; **a float that would fall off the page bottom**, which is the fragmentation interaction and the one that loses text; text conservation asserted across every float fixture, since a lost float is a lost paragraph; the browser comparison run over a float-heavy content document and its `y`-offset agreement recorded as a number | M |
| 11 | **Tables** | CSS 2.2 §17.2's model and **§17.2.1's anonymous table objects**, which is the fixup a real book needs because HTML tables in the wild omit `<tbody>`; §17.5.2.1's fixed layout and §17.5.2.2's automatic layout, the latter asserted to be the two-pass algorithm the spec describes rather than a one-pass approximation; §17.6.1's separated model and §17.6.2's collapsing model with §17.6.2.1's conflict resolution; `colspan` and `rowspan`; a nested table, since it multiplies the layout work cap; **table fragmentation across a page boundary** — or, if it is staged, the row amended in place with its argument, in the shape gap 30's milestone 8 amended its own. **Amended, 21 August 2026, milestone 11, twice.** *(1)* Fragmentation across a page boundary is **built between the rows and staged inside one**: a table breaks at any of its row bands and a band is the maximal run of rows a `rowspan` joins, which is where §13.3.3 puts a break position and is what every table in a real book needs. A band taller than a whole page has no break position inside it, is drawn where it is, and says `TableRowTallerThanPage`; slicing every cell of a band at one height and continuing them on the next page is `css-break-3`'s and is not here. *(2)* **The fixup a real book needs is not the missing `<tbody>`** — not for this corpus. Pandoc and calibre both write `<thead>` and `<tbody>` in full; what they write that needs §17.2.1 is *indentation*, so the step that fires on every table in the committed corpus is rule 3 and the row group fires on none of them. The bare-`<tr>` table is hand-written and legacy HTML's shape, is implemented, and has its own fixtures | L |
| 12 | **Flexbox, and fixed-layout renditions** | `css-flexbox-1`: `display: flex` and `inline-flex`, `flex-direction`, `flex-wrap`, the `flex` shorthand and its three components, `justify-content`, `align-items`, `align-self`, `align-content` and `order`, each with a fixture; the flex layout algorithm's line-breaking and free-space distribution asserted against the browser oracle rather than against arithmetic done twice; **fixed-layout renditions** (§8.2) — `rendition:layout: pre-paginated`, which EPUB RS 3.3 §8.1 makes *"exactly one page per spine itemref"*, §8.2.2.6's content-document dimensions from the viewport meta, and the initial containing block of RS §8.1.2 with content outside it clipped; a fixed-layout book from milestone 1's corpus if one can be obtained, and **recorded as owed rather than quietly dropped** if not — gap 30's own shortfall, named so it is not repeated by accident. **Amended at the milestone, with the arguments in the progress note.** (a) `display: inline-flex` is laid out as a **block-level** flex container and warned by name: this build has no inline-level box that is not text, and the only other answer throws the whole flex layout away. (b) A **flex line** is the unit of fragmentation, so a row container breaks between its lines and a `column` container is one unbreakable item that says `FlexLineTallerThanPage` when it is taller than a page; `css-break-3`'s fragmentation *inside* a line is not here. (c) `gap`, `row-gap` and `column-gap` stay `Unsupported` by name -- they are `css-align-3`'s and the row does not ask for them. (d) **No fixed-layout book was obtained**, exactly as milestone 1 predicted, and it is recorded as owed with the synthesised fixtures that stand in its place named as synthesised | M |
| 13 | **Bounds, determinism, ledgers, campaign** | Every constant joins `bounds_ledger.rs`'s **existing** table and passes all five checks, with three recorded numbers each; **the book yardstick added as the third, and every one of the seventeen existing rows given a figure for it** — no row opts out, because gap 30's milestone 9 had to fix exactly that; the three `const`-block relations, so a bad one **does not compile**, including the product relation for `MAX_SELECTOR_MATCHES`; the **fifteenth** determinism fingerprint — gap 30's XPS is the fourteenth — whose fixture is a real producer's book from milestone 1, plus a byte hash of the synthesised document beside it in the pair gap 29 established, reproduced on `wasm32-wasip1` with none of the other fourteen moving; **a fingerprint at a second page box**, which is this format's own determinism question and no earlier one had it: the same bytes at 432 × 648 and at 600 × 800 must each be stable and must differ; `cargo fuzz run css` and `cargo fuzz run layout` each surviving a session with no crash, no OOM and no timeout; peak memory recorded for the largest book in both corpora; the ledger sweep below | M |

**Milestone 1 comes first, and it is not gap 30's answer copied.** That plan
could commit its corpus; this one mostly cannot, and the licence table above is
why. So milestone 1 delivers a *committed* corpus it commissions itself and a
*fetched* one it can only gate on — and the gate is the deliverable, because a
corpus that silently does not run is worse than no corpus.

**Milestone 3 is early for the reason gap 30's milestone 3 was.** It is the
only one of the thirteen that improves matters on its own: after it, an EPUB is
refused by a name that is true instead of opening as its cover. If this plan
were ever descoped, milestone 3 is the part that must still land.

**Milestone 5 comes before milestone 8, and the ordering is the point rather
than a preference.** [18a](18a-jpx-decoder.md)'s M0 did this, gap 29's milestone
1 did it for `inflate_raw`, and gap 30's milestone 5 did it for the writer. A
build that reaches milestone 8 without link annotations will render a book whose
cross-references are ordinary blue text, and a build without an outline will
render one with no table of contents — both of which look like the book rather
than like a missing feature. Landing the writer first costs one commit's
ordering; landing it after costs a rewrite and a period in which the wrong shape
is what the tests assert.

**Milestones 6 and 7 are in that order for a reason worth stating**, because
the opposite is tempting: layout is where the interesting problems are, and CSS
is where the *inputs* are. A layout engine built first has to invent its own
input representation, and that representation then becomes what the cascade must
produce — which is how a cascade acquires shortcuts. Decision 5's compile-time
check only works if the parser's enum is the thing layout matches on, and that
means the parser exists first.

**Milestone 4's text-conservation harness is scheduled with the placeholders,
not with the first real layout**, and that is deliberate. Built at milestone 8
it would be a test written to pass; built at milestone 4 against thirteen grey
pages it is a test written before there is anything to make it pass, and every
milestone after inherits it as a constraint rather than acquiring it as a
formality.

### The ledger sweep milestone 13 owes

**The leaf count changes again, and it is written in four places**, none of
which the compiler can reach. It went five to seven with gap 29 and seven to
eight with gap 30, and gap 29 found it had already drifted once unnoticed. All
four go from **eight** to **ten**:

- **`docs/plans/00-architecture.md`**, the DAG diagram and the enumerated count,
  as a dated in-place amendment in the style that file's two existing ones use;
- **`docs/plans/99-consistency.md`** ruling 8 — whose gap 30 amendment predicts
  this update and, per the correction above, needs its "reuses this crate"
  sentence corrected in the same edit;
- **`CONTRIBUTING.md`** rule 3;
- **`README.md`**'s "Workspace" section, which is the fourth place gap 29 found.

Plus `xtask/src/main.rs`'s `ALLOWED` and its doc comment, which is where the
fifth and sixth amendments' arguments live.

The rest of the ordinary ledger:

- **`docs/plans/gaps/28-tinker-integration-decisions.md`**, whose EPUB sizing
  sentence is amended in place, dated, per the correction above — the size
  unchanged and the two omissions named.
- **`THIRDPARTY.md`**, which gains its **second** vendored tree, and
  `cargo xtask vendor` which must accept it.
- **`docs/STATUS.md`**, where EPUB moves from decided to built with its own row,
  and the test count, leaf count, fingerprint count and fuzz-target count all
  move.
- **`README.md`**'s paragraph under gap 28's amendment, which currently says two
  of the three are built.
- **`fuzz/README.md`**, whose target table and seed table each gain two rows and
  whose count goes from twenty-one to twenty-three — and the seeds are curated,
  not a campaign's working state, which is what `d9945a0` is about.
- **`.github/workflows/ci.yml`**'s per-PR fuzz job, whose comment names a target
  count and uses it to decide its own time budget. Gap 29 found it saying
  "fifteen" when there were twenty; gap 30 moved it to twenty-one.
- **`docs/plans/99-consistency.md`** ruling 9, amended in writing to name the
  browser as a fifth oracle, in milestone 8 rather than here.
- **`docs/plans/gaps/README.md`** and **`00-execution-order.md`**, updated with
  this plan rather than after it.

`docs/plans/13-bindings.md` also enumerates the leaves and is **deliberately not
amended**, for the reason gaps 29 and 30 both recorded: that list is a
publishing plan naming the crates gap 26 dry-ran to crates.io, and adding to it
is a claim about publishing rather than a correction to a count.

## Dependencies

**Needs first — all landed:**

- [28](28-tinker-integration-decisions.md), for the decision this plan
  implements and the size it was agreed at, and the scope answer of 19 August
  2026 recorded above.
- [29](29-cbz.md), for `tinker-pdf-zip`, for `ImageData::Compressed` and the
  pass-through that keeps a book's raster cost a multiple of its entries, for
  `ArchiveRefusal` and the `#[non_exhaustive]` that lets it grow without a
  break, for `bounds_ledger.rs`, and for the synthesis scaffold.
- [30](30-xps.md), for `tinker-pdf-xml` — **which this plan changes**, in
  milestone 2, and the change is a dependency in both directions — for the
  container discrimination this plan extends, for `/ExtGState`, `/Shading`,
  `/Pattern` and Type0/CIDFontType2 without which no styled text could be
  written at all, and for the `const`-block device.
- [20](20-linearization-validation.md), for the qpdf CI job and for its finding
  that a **skipped** oracle test exits 0 and reads exactly like a pass — which
  this plan leans on twice more, for the fetched corpus and for the browser.
- [24](24-fuzz-execution.md) M1–M4 for the fuzz toolchain. `cargo-fuzz` needs
  libFuzzer, which `x86_64-pc-windows-msvc` does not support; WSL2 with nightly
  is the local route, as six other plans now record.
- [25](25-wasm-determinism-leg.md) M1–M3 for the leg the fifteenth fingerprint
  is checked on.

**Needs, and is not in the repository:** real EPUBs, which milestone 1 obtains
and mostly cannot commit; the UCD's `LineBreak.txt` and `EastAsianWidth.txt`,
vendored as data under `THIRDPARTY.md`; `epubcheck`, `mupdf-tools` and a
headless browser on the CI runner, installed by their jobs rather than vendored
(ruling 9).

**Unblocks:** nothing. This is the last of gap 28's three, and closing it closes
the programme that document's option D spawned.

**Amends, in the same commits:** the ledger sweep above.

## Risks

| Risk | Mitigation |
| --- | --- |
| An EPUB keeps opening as its cover while the layout engine is built, because the discrimination is treated as part of the reader | Milestone 3, early and standalone; milestone 1 commits the failing tests so the defect has a name in the suite from the first commit |
| A CSS property is parsed and ignored, and nothing anywhere reports it | Decision 5: a property is a parser variant only when a consumer exists, and the consumer's `match` is exhaustive, so the omission **does not compile**. Milestone 6's exit criterion injects the defect and asserts the *build* fails, not a test |
| A value is mapped onto its nearest implemented neighbour, and the page looks right | The implemented set is keyed by (property, value); `float: inline-start` is not `float: left`. Gap 07's solid-black gradient, one format up |
| A spine item is dropped and the book reads continuously across the hole | Text conservation, built at milestone 4 against placeholders so every later milestone inherits it rather than acquiring it |
| Fragmentation repeats or loses the block that crosses a boundary, and no rendered comparison sees it | The same harness: a duplicated paragraph is extra text and a dropped one is missing text, and both are exact |
| Line breaking is done at spaces, passes every English fixture, and is catastrophic on CJK | UAX #14 over vendored UCD tables — whose licence was checked against `deny.toml` and already passes — with a CJK fixture as a named exit criterion, and the ASCII heuristic refused rather than staged |
| The XML parser is relaxed for XHTML and one of gap 30's four bombs survives | The mode has two values and no third; the internal subset is where all four bombs live and it is refused by its own name; **all four are re-asserted under the new mode**, which is the only test that proves the defence survived |
| `with_fonts` silently does not change the pagination | `OpenOptions::fonts` is the route; the late path warns by name; and the generic families' standard-14 metrics make pagination independent of the provider, asserted in milestone 9 |
| The page box is an undocumented constant and the page count is a lie | It is on the type, with a doc comment saying the page count is a function of it and not a property of the file; milestone 13 fingerprints two different boxes and asserts they differ |
| A cap is set above what its own inputs can reach and never fires | Gap 18a M8's exact failure. Every constant joins `bounds_ledger.rs`'s existing table and inherits `every_bound_can_fire`, the only thing in the workspace that has ever caught it |
| A per-rule or per-element cap is treated as the total, and selector matching runs quadratically | `MAX_SELECTOR_MATCHES` is the product's total, and the relation is a `const` block asserting the product **exceeds** it — `5adf502`'s lesson in the one place where the reachable ceiling is a multiplication |
| `MAX_EPUB_PAGES` is assumed to be bounded by the spine item count | It is not, and the table says so: one 128 MiB content document fragments into as many pages as its length allows. Gap 30 found the same shape for `MAX_XPS_PAGES` and this one is worse |
| The corpus is one converter's, so the CSS census is one converter's | Milestone 1 requires **two producers minimum** and is not satisfiable by six Gutenberg books; gap 30 closed owing exactly this and it is named in row 1 |
| The fetched corpus silently does not run in CI, and the suite is green on the committed books alone | Gap 20's finding, applied a third time: `epub-corpus: RAN` / `SKIPPED`, greppable, red on `SKIPPED` |
| A CSS, HTML, layout, line-breaking or SHA-1 crate is added because the rule lived only in prose | Denied by name in the milestone that writes each, not in the one that finishes the plan — gap 30's rule, and `deny.toml` has none of these names today |
| A pixel comparison against a browser is built, disagrees everywhere, and is thresholded into meaninglessness | The comparison is `y` offsets and page assignment, not pixels; what it can and cannot prove is worked out in [Oracles](#oracles-per-ruling-9) before anything is built |
| mutool is used as an oracle the way gap 30 used it, and its disagreements are treated as bugs | Stated in the oracle section as a reversal: MuPDF's EPUB layout is itself partial, so **disagreeing with it is not evidence of a bug**, and the browser is why a fifth oracle is proposed |
| A phase is quietly narrowed to a subset because the full engine is large | Every deferral is a staged decision that **amends its own row in place with its argument**, the way gap 30's milestone 8 amended row 8 — never a criterion silently unmet |
| An `As built` that reads as "EPUB works now" | The claim this plan can support is a horizontally-written, unscripted, reflowable or fixed-layout book of XHTML and CSS, at a page box the caller states. Vertical writing modes, bidi, SVG spine items, MathML, scripting, media overlays, DRM, grid and multi-column are refused **by name**, and the `As built` carries the `Unsupported` property census per book, which is the number that says how much of somebody else's format this build actually reads |

## Progress — 19 August 2026, milestone 1

**Two corpora have landed, and no reader.** Six books from two real producers
sit under `crates/tinker-pdf/tests/epub/` — 49 597 bytes, with per-file
provenance in a README beside them, an inventory of all seventy-two entries that
a test recomputes on every run, epubcheck's verdict per file, and the
present-day failure pinned as three `#[ignore]`d tests that fail when run.
Twenty more books that **cannot be committed** are fetched by
`tests/epub/fetch-corpus.sh` into `target/`, behind an env-var gate whose tests
print `epub-corpus: RAN` or `SKIPPED` and whose CI job goes red on the second.
Thirty tests in `tests/epub.rs` and eight in `tests/epub_fetched.rs`; the
workspace stands at **2 242**, up thirty-five, because an ignored test does not
count.

That was the point of scheduling this first. Gap 29 closed having never opened a
`.cbz` a real archiver wrote; gap 30 fixed that by obtaining eight genuine
packages before a reader existed and recorded seven things ECMA-388 did not
predict. This milestone found **fourteen**, and four of them would have produced
a reader that refuses one of the two producers outright.

### The producers, and the two corpora the licence gate forces

**pandoc 3.10.2** and **calibre 9.13.0** (`ebook-convert`), over
`tests/epub/source/book.md` and `source/figures.md` and four PNGs the corpus
script writes byte by byte from the PNG specification. Six books: EPUB 3 and
EPUB 2 from each producer, a cover-only book and a no-image book from each, and
one book of three plates at three different pixel sizes. Every byte of content
is authored here, so each book is *our input through their tool* — the reading
`fuzz/README.md` already applies to the JPX seeds and gap 30 applied to
Windows' XPS serialisers. pandoc is GPL-2.0-or-later and calibre GPL-3.0-only;
neither licence reaches the document a converter converts, nothing of either is
vendored or linked, and — unlike gap 30's corpus — **no font is involved at
all**, so there is nothing here anybody has to licence.

**Row 1's "two producers minimum" earned itself immediately.** Almost every
finding below is a place where the two disagree, and none of them exists in a
corpus of one. Gap 30 closed owing one non-Windows package; this milestone did
not repeat that.

**The licence table's three barred rows are confirmed, and the open one is
answered — with a sharper answer than the plan expected.**

- Project Gutenberg: unchanged. Clause 1.E.1's display obligation and 1.E.4's
  no-detach rule; not committed, fetched.
- `epub3-samples`: **CC-BY-SA 3.0**, confirmed at the repository's own README —
  *"Unless specified otherwise … all samples are licensed under CC-BY-SA 3.0"*.
  `deny.toml`'s *"deliberately NO copyleft … not even weak copyleft"* bars it.
  The obvious source of committable EPUBs is barred by this repository's own
  gate.
- `w3c/epub-tests`: the plan guessed *"probably the W3C Software and Document
  Licence"* and it is right — `LICENSE.md` says so in as many words. **But that
  is not the whole answer.** `deny.toml`'s allowlist holds eleven SPDX
  identifiers and **none of them is `W3C` or `W3C-20150513`**, and that list is
  what `cargo xtask vendor` checks committed **data** trees against. So the
  corpus is legally committable and mechanically is not: committing it needs an
  allowlist entry landing in the same commit as the files, in the shape
  `deny.toml`'s own OFL-1.1 comment prescribes. Recorded rather than done,
  because nothing needs those files yet.

### The measurement holds to the number

Every figure in [What is wrong](#what-is-wrong) was re-measured on this machine
against the same six Gutenberg books, and **not one of them moved**: Frankenstein
one page at 1824 × 2726 pt, Pride and Prejudice 1500 × 2114, Moby-Dick
780 × 1227, both Alices 800 × 1104, Beowulf 1826 × 2726, `warnings()` empty,
`parsed_parts()` 0, `ladder_level()` `Trust`, every `PageOrigin.defect` `None`.
The doctype split holds to the file count as well — sixteen and fourteen EPUB 2
content documents, all single-quoted XHTML 1.1; one `<!DOCTYPE html>` per
Gutenberg EPUB 3 book, on the cover wrapper and nowhere else.

**0 of 26** books carry `[Content_Types].xml` or `_rels/.rels`, so
`ArchiveRefusal::UnreadablePackage` is unreachable for an EPUB and the comic
fallthrough is exactly what ECMA-388 E.3 asks for. **Gap 30 is not wrong; EPUB
is a different question it did not ask**, and there is now a test in each corpus
that says so rather than a paragraph.

**26 of 26** put `mimetype` first in *both* physical and directory order, so no
real book can tell `header_offset == 0` from `index == 0`. That assertion would
be a tautology on its own, so `tests/epub.rs` builds the container that does
distinguish them — physical order `container.xml`, `mimetype`; directory order
reversed — and asserts the two verdict fields differ on it. Milestone 3 inherits
the fixture as well as the criterion.

### The mis-read is worse than "one page", away from Gutenberg

The plan's refinement — *"a Project Gutenberg EPUB contains exactly one image,
whatever its `.images` / `.noimages` variant says"* — is right about Gutenberg
and is not the general shape. Across the fetched corpus:

- `sample-internallinks.epub` opens as **ten pages** of publisher logos, store
  badges and screenshots of other reading systems, in filename order.
- `sample-svg-in-spine.epub` opens as **six**, three of which are decorative
  table-of-contents ornaments **one point wide** — 1 × 641, 1 × 4 and 1 × 7 pt
  pages, which is a page a viewer cannot show at all.
- `sample-linear-algebra.epub` — a megabyte, ninety-four content documents of
  MathML — is refused as `NoImages`, and so is `sample-hefty-water.epub`.

Eighteen of twenty open, two refuse, **none is read as the book it is**. Add the
committed six and it is 22 open or refuse and none read.

### What the real books showed that this plan did not predict

Seven of this plan's predictions held and are listed above. These are the ones
it did not have. Four of them would have produced a reader that refuses one of
the two producers outright, which is the same shape gap 30's milestone 1 found
twice.

- **The two producers disagree about whether to write a doctype at all.**
  pandoc writes one on **every** content document it produces — `<!DOCTYPE
  html>` under EPUB 3, the XHTML 1.1 public identifier under EPUB 2. calibre
  writes **none**, in either version. So the census's answer is not a percentage
  but a partition: on the parser as it stands, every calibre book reads and
  every pandoc book is refused. Milestone 2 is required by one producer and
  irrelevant to the other, and a corpus of either alone would have said the
  wrong thing about it.
- **Both quote characters are real, and neither corpus shows both.** The plan
  measured `-//W3C//DTD XHTML 1.1//EN` **single-quoted**, on Gutenberg's EPUB 2
  books — thirty content documents across the fetched corpus, and **zero**
  double-quoted. pandoc writes the same identifier **double-quoted**, five
  documents, and the committed corpus has zero single-quoted. The settlement
  table's two `PUBLIC` rows are supplied by two different corpora, and milestone
  2's fixtures need both.
- **No real document in 270 carries a `SYSTEM`-only declaration or an internal
  subset.** Milestone 2's fixtures for those two rows have to be written rather
  than found — and the internal subset being absent from every real book is what
  makes refusing it by name, with all four of gap 30's bombs behind the refusal,
  cost nothing at all.
- **The named-character-reference question is settled, and the answer is option
  1 rather than the plan's working assumption of option 2.** **Zero** named
  references across all 270 content documents of both corpora. And the
  corroboration is weaker than the plan expected, and in the more useful
  direction. Its sentence was *"producers overwhelmingly write `&#160;`"*. The
  fetched corpus writes the numeric form **65 times** against **83 240 literal
  non-ASCII characters** — eight hundredths of one per cent — and the two
  committed producers write it **not once**: the em dash, the ellipsis, the
  non-breaking hyphen and the Japanese line in `source/book.md` all reach the
  content documents as literal UTF-8. So the escaping habit the plan expected to
  find is a rounding error, and every book in both corpora is a book of
  characters rather than of references. Both censuses count non-ASCII characters
  alongside the references precisely so the zero means *"these producers escape
  nothing"* rather than *"this text is ASCII"* — without that column, a corpus
  of 83 240 non-ASCII characters and a corpus of none would report the same
  zero. So milestone 2 refuses an undeclared named reference by name, per XML
  1.0, and the ~250-entry vendored table and its `THIRDPARTY.md` section are
  **not built** — with the brittleness measured rather than feared: not one of
  270 real content documents would be lost by it.
- **pandoc puts a seventh file in `META-INF`.** Every pandoc book carries
  `META-INF/com.apple.ibooks.display-options.xml`, which is not one of
  §4.2.6.3's six reserved names — and **zero of the twenty fetched books carry
  anything unreserved there at all**. A milestone 3 that refused an unrecognised
  `META-INF` entry would refuse every book pandoc writes and pass the entire
  downloaded corpus. This is the single best argument the milestone has for
  having commissioned a corpus as well as fetched one.
- **calibre writes a `META-INF/` directory entry** — a name ending in `/`, zero
  bytes long, and **deflated**. pandoc writes none. A `META-INF` walk that
  treated every entry there as a file meets an empty one first.
- **A content document is not named `.xhtml`, and the extension does not agree
  with itself inside one book.** calibre writes `index_split_000.html` for the
  four files it split the input into and `.xhtml` for the two it generated
  itself, and declares all six `application/xhtml+xml` in the manifest. A census
  or a reader keyed on `.xhtml` sees two-thirds of a calibre book.
- **The package document is not always under a directory.** calibre puts
  `content.opf` at the archive root; pandoc puts it under `EPUB/`. §4.2.5's
  resolution against the *referring document* is load-bearing from the first
  real file rather than from an exotic one, and milestone 3's `rootfile`
  resolution meets both on day one.
- **One producer's EPUB 3 has no NCX and its EPUB 2 has no navigation document;
  the other's EPUB 3 has both.** calibre's EPUB 3 output ships `nav.xhtml` and
  no `toc.ncx`; its EPUB 2 output ships `toc.ncx` and no nav. pandoc's EPUB 3
  ships both. Milestone 5's outline and milestone 8's navigation reading cannot
  assume either is present.
- **Both ZIP methods appear inside one archive, from one producer, in one run.**
  pandoc **stores** two of `pandoc-plates.epub`'s three PNGs and deflates the
  third, because deflate does not help them. Gap 30 read the same habit in
  Microsoft's two serialisers as inconsistency; it is what a producer that
  measures does, and it is why `.gitattributes` gains `*.epub binary` — a
  normalised line ending inside a stored entry breaks its CRC-32.
- **The pictures are written in reverse of the order the book names them.**
  `file2.png`, `file1.png`, `file0.png` by header offset, against `file0`,
  `file1`, `file2` in the manifest and in the spine. Physical order, directory
  order and reading order are three different orders in one 8 KB file.
- **A default invocation of a mainstream producer does not produce a clean
  book.** calibre 9.13.0's EPUB 3 cover output warns `NAV-011` under epubcheck
  5.3.0 — *"toc nav must be in reading order"* — because the generated title
  page precedes the first entry the navigation document links to. Worth knowing
  before this engine's first disagreement with a producer.
- **The CSS census is more than twice the plan's list, and what it adds is a
  non-goal.** Forty-two distinct properties across the committed corpus's eight
  stylesheets and **eighty-four** across the fetched corpus's fifty-three,
  against the plan's forty-one names. Present in real books: `column-count`,
  `column-gap`, `column-rule`, `column-fill` and their `-webkit-` and `-moz-`
  spellings — **multi-column**, which [Non-goals](#non-goals) names as one of
  the two *"worth flagging rather than filing under rare, because a book that
  uses either will lay out as a single column and look entirely reasonable"*.
  Also `box-shadow`, `text-shadow`, `border-radius`, `content`, `visibility`,
  `table-layout`, `border-collapse`, `word-wrap`,
  `-epub-text-emphasis-style`, and Antenna House's `-ah-margin-start` /
  `-ah-margin-end`. Both producers write `page-break-before` and
  `page-break-after`, which is milestone 7's fragmentation criterion arriving on
  the first file.
- **`tinker_pdf_zip::Entry` has no extra-field accessor**, so §4.3.2's *"no
  extra field"* clause cannot be checked through it. The plan predicted this and
  it is confirmed; measured by hand across all twenty-six books, the answer is
  zero everywhere. Milestone 3 closes it with either an accessor on `Entry` or a
  byte check in the facade, and the test that owes it says so in a comment
  rather than skipping quietly.

### epubcheck, and what it is for

Version 5.3.0 under Temurin 21, run over all twenty-six books, verdict recorded
per file — `tests/epub/EPUBCHECK.tsv` for the six that are committed and this
section for the twenty that cannot be. Five of six committed are clean and the
sixth is calibre's `NAV-011`. Eighteen of twenty fetched are clean;
`sample-georgia-cfi.epub` has **seven `ERROR:RSC-020`**, a malformed URL, so it
is a book this engine may refuse without apology, and both obfuscated-font
samples report `INFO:RSC-004` — epubcheck saying it could not read the font,
which is the same fact milestone 9 de-obfuscates its way past.

`the_recorded_epubcheck_verdicts_still_hold` re-runs the tool when
`TINKER_EPUBCHECK` names it and compares every count and message code;
`every_book_has_a_recorded_epubcheck_verdict` checks the record itself and runs
whether or not Java exists, because a book added without a verdict is a gap in
the record on a machine with no JVM and that is exactly what a `SKIPPED` would
hide.

### The three pinned tests, and why there are three

Gap 30's milestone 1 committed two. This one commits three, because the defect
has three consequences and a test for one of them is not a test:

- `an_epub_whose_only_picture_is_its_cover_is_not_a_one_page_book_of_it` — the
  mis-read.
- `an_epub_with_no_picture_at_all_is_not_refused_as_having_no_images` — the
  false refusal, over both producers' no-image books. A build could fix either
  of these without the other.
- `not_one_committed_book_is_read_as_the_book_it_is` — the sweep. Gap 30's
  equivalent sweep found something its two examples did not, and so does this
  one: `pandoc-plates.epub` is a book of three chapters that opens as three
  pages of three different sizes, and neither of the first two would notice it.

All three **fail when run with `--ignored`**, which was checked rather than
assumed, and each fails with a message naming the book and the wrong answer.
Each accepts **either** a correct read **or** a refusal by a name that is true
of a book — gap 30's milestone-3 correction applied before it can bite, since
that plan's pinned test used `.expect` and so *"forced the spine a milestone
early"*. The predicate for "true of a book" excludes `NoImages`,
`UnreadablePackage` and the four damaged-ZIP names, and its catch-all answers
`true`, so whatever milestone 3 adds satisfies them without this row naming it.

**And the predicate is asserted by a test that runs**, which is the part that is
easy to miss: an `#[ignore]`d test that would pass is not a pin, and nothing in
a normal `cargo test` would say so.

`today_an_epub_opens_as_a_comic_and_this_is_what_it_reports` **passes** and
asserts the wrong answers exactly — the page count is the picture count, the
pages *are* the pictures in the comic path's own name order, and the three
silences are zero. `today_a_book_of_two_chapters_is_one_page_of_its_cover`
asserts the number beside it, because a relation that held while both sides were
wrong would still pass. Milestone 3 deletes both in the commit that un-ignores
the three.

### Injection

Twenty-two defects injected into the census logic, the provenance checks, the
inventory and the gate, over two rounds. **Twenty-one caught.**

Round one: twenty injected, eighteen caught, two survivors, and each survivor
became an assertion. A named-reference scanner that accepts a name beginning
with a digit survived, because nothing asserted that `&123;` is not a reference;
a doctype scanner that matches `PUBLIC` and `SYSTEM` as prefixes survived,
because `<!DOCTYPE publicity >` was in no fixture. Both are now asserted and
both mutations are caught.

The one survivor left is an **equivalent mutant**: widening the doctype scan's
word accumulator from ASCII-alphabetic to ASCII-alphanumeric changes no
classification for any input in either corpus or any fixture in the suite,
because no document type declaration in existence puts a digit against `PUBLIC`
or `SYSTEM`. It is recorded rather than papered over with a fixture invented to
kill it.

Two of the injections found real defects in code that had already been written
and reviewed, before they were run as injections: the CSS census read `a:hover`
inside an `@media` block as a property named `a` — it appeared in the fetched
corpus's property list on the first run — and `numeric_references` counted
`&#xZZ;`, because it accepted any alphanumeric where the hexadecimal form allows
only a hex digit. Both are fixed and both have their own tests; the second is
the reason a census's own scanner needs a test at all, since a scanner that
over-counts and a corpus that genuinely uses a construct are indistinguishable
from the number alone.

### What the later milestones inherit

- **Three failing tests with names, and two passing tests that must break.**
  Milestone 3 un-ignores the three and deletes the two in the same commit. If
  either still passes afterwards, the discrimination did not happen.
- **A container whose physical and directory orders disagree**, already built,
  for milestone 3's `header_offset`-not-`index` criterion.
- **A `META-INF` that is not only the six reserved names**, from a producer
  rather than from a fixture, and a **directory entry** in the same directory
  from the other producer.
- **A package document at the archive root and one under `EPUB/`**, so
  milestone 3's relative resolution is exercised in both directions by real
  files.
- **The named-reference decision, made**: option 1, and milestone 2 does not
  build a table.
- **Both external-identifier quote forms**, one per corpus, and the two shapes
  no real book supplies named as fixtures milestone 2 must write.
- **Forty-two CSS property names committed and eighty-four fetched** to build
  milestone 6's `Known`
  enum against, and the multi-column family named as a non-goal that real books
  use.
- **Two obfuscated-font books and an `encryption.xml`** in the fetched corpus,
  which is milestone 9's only real input and milestone 3's only real
  `encryption.xml`.
- **A book epubcheck rejects** (`sample-georgia-cfi.epub`, seven `RSC-020`), so
  the first disagreement between this engine and a book has a party to blame.
- **`tinker-pdf-zip` in the facade's `[dev-dependencies]` already**, from gap
  30 — no manifest change, and `xtask -- dag` sees no new edge.

### What is short

- **No fixed-layout book is in the committed corpus.** Milestone 12's criterion
  asks for one *"if one can be obtained"*; neither producer here emits
  `rendition:layout: pre-paginated`, and the fetched corpus's fixed-layout
  samples are the CC-BY-SA ones. Recorded as owed, in the shape gap 30 named its
  own shortfall rather than dropping it.
- **The extra-field check is owed**, as above: measured by hand, not by a test,
  because the accessor does not exist.
- **The doctype census cannot tell a declaration in the prolog from the text
  `<!DOCTYPE` inside a `<code>` element.** No book in either corpus contains
  the second, and both censuses print their per-file answers so a human can
  disbelieve them. Milestone 2's scanner has a position to work from and this
  one does not.

## Progress — 19 August 2026, milestone 2

**`tinker-pdf-xml` has two doctype modes and the four bombs still refuse under
both.** `Doctype::Refuse` and `Doctype::SkipExternalId`, two values and no
third. The workspace stands at **2 268**.

### The collision this settles, and why the relaxation is dangerous

Gap 30 milestone 2 made `<!DOCTYPE` `Error::DoctypeUnsupported` **before one
byte after it is read**, and that refusal *is* the whole defence against entity
expansion: not a budget, but never entering the grammar that has one.

Milestone 1 then measured what real books do. 100 % of Gutenberg's EPUB 2
content documents carry `<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.1//EN' …>`,
**single-quoted**; every EPUB 3 book carries `<!DOCTYPE html>` on its cover
wrapper; and the two producers **disagree about whether to write one at all** —
pandoc always, calibre never. The parser as it stood refused one producer
entirely and read the other.

**Skipping an external identifier means walking past two literals, and a `>`
may be inside either one.** A scanner that hunted the next `>` would leave the
declaration early and resume in the middle of it, which is precisely the hole
refusing `<!DOCTYPE` had closed. So `Cursor::literal` exists beside
`Cursor::attribute_value` rather than reusing it: **the quote is the only
terminator**, `<` is ordinary because §4.2.2's `SystemLiteral` is `[^"]*` where
`AttValue` is not, and
`a_greater_than_inside_a_literal_does_not_end_the_declaration` is the test that
holds it. The injection matrix confirms it is the only test that does.

### The bombs, refused twice over

All four of gap 30's committed bombs — billion laughs, the quadratic-blowup
variant, an external entity and an internal-subset parameter entity — are
re-asserted under `SkipExternalId`, and they refuse there by a **second** name:
`Error::InternalSubset`, because all four of them live in the internal subset
and the internal subset is what the relaxed mode still will not enter.

`no_bomb_is_read_into_its_internal_subset` asserts the **offset**, not only the
error, which is the difference between "it refused" and "it refused without
reading the thing". Under `Refuse` the cursor does not move at all; under
`SkipExternalId` it stops at the `[`.

A declaration **outside** the prolog is refused in both modes — as
`MisplacedDoctype` under the relaxed one — and the cursor does not move there
either, because reading it would mean parsing DTD content in order to report
that DTD content is not allowed.

### What the census settled against the plan

Milestone 1's census found **zero** uses of named character references in 270
content documents, against 65 occurrences of `&#160;` and 83 240 literal
non-ASCII characters. So there is **no entity table and no expander in either
mode**, an undeclared named reference is refused by name in both, and the plan's
own working assumption was amended in place rather than only in a Progress
section.

### The injection matrix

Eight defects, one at a time, each reverted before the next, the full workspace
re-run with `--no-fail-fast`. **Seven caught; one is an equivalent mutant and is
recorded rather than killed with an invented fixture.**

| Defect | Caught by |
| --- | --- |
| The relaxed mode becomes the default | ten tests |
| A declaration outside the prolog is skipped rather than refused | `a_declaration_outside_the_prolog_is_still_refused_in_the_relaxed_mode`, and one more |
| `Refuse` reads the declaration before refusing it | `every_bomb_is_refused_as_a_doctype_and_not_as_a_cap`, and four more |
| A second declaration is accepted | `a_declaration_the_prolog_has_no_room_for_is_refused_in_both_modes`, and one more |
| **A literal ends at the first `>` rather than at its quote** | `a_greater_than_inside_a_literal_does_not_end_the_declaration` — **alone** |
| Only the double quote is a literal delimiter | four tests |
| The external-identifier keyword matched as a fixed six-character prefix | twelve tests |
| **XPS stops passing `Refuse` explicitly** | **nothing — an equivalent mutant** |

The survivor is honest rather than a gap, and the pair is worth stating.
`Source::reader` already defaults to `Refuse`, so removing the explicit argument
from `xps.rs` changes no answer **while that default holds**. What it protects
against is the default changing — and *that* is caught, by ten tests, as the
first row shows. The two together are the property; neither alone is. Killing
this one would need a fixture that asserts the source text rather than the
behaviour, which is worse than recording it.

Milestone 1 had already recorded a second equivalent mutant here in the code
itself: widening `ascii_word` from alphabetic to alphanumeric. The sharper form
of that injection — a fixed six-character prefix, which reads `PUBLICITY` as
`PUBLIC` — **is** caught, by twelve tests.

### Still owed

- **The facade end-to-end test over the real books** is not written. The mode is
  proved at the crate's own level and against the committed corpus's markup; a
  test that opens a real EPUB and asserts its content documents parse belongs
  with milestone 3, which is where a book first becomes something other than a
  comic.
- **`tinker-pdf-xml`'s fuzz target does not exercise the relaxed mode.** Four new
  seeds are committed under `fuzz/corpus/xml/` — the HTML5 shape, the
  single-quoted XHTML 1.1 shape, a `>` inside a literal, and an internal subset
  — but the target still constructs its reader through `Source::reader`. Making
  the control byte choose the mode is milestone 13's campaign work, and until it
  does, the relaxed mode has had no fuzzing at all.
- **EPUB 3.3 Appendix B's set is small and closed**, and an identifier outside it
  is named rather than refused. That is the specified behaviour, but it means a
  book carrying an unknown identifier still parses — the warning is the only
  signal, and nothing above this crate reads it yet.

## Progress — 19 August 2026, milestone 3

**An EPUB is told from a comic, and the live defect this plan opens with is
fixed.** All six committed books and all twenty fetched ones now come back as
`ArchiveRefusal::UnpaginatedBook` — *"an EPUB, which this build recognises and
does not lay out yet"* — where before this commit `pg84-images.epub` opened as
one page of 1824 × 2726 pt and `sample-internallinks.epub` as ten pages of
publisher logos. Milestone 1's three pinned tests are ordinary tests now, and
the two that recorded the wrong answers are **deleted in this commit**, which is
the point of having written them. The workspace stands at **2 305**, up
thirty-seven, with three fewer ignored.

Row 3 said this milestone is *"the only one of the thirteen that improves
matters on its own"*. It is: nothing here lays out a page, and a host that opens
a book now gets a refusal it can show instead of a picture of the cover.

### The route, and the one ordering decision in it

`Document::open` opens the ZIP **once** and asks three questions in order —
ECMA-388 E.3's three steps, then `META-INF/container.xml`, then the comic path
as the fallthrough — with the archive travelling inside each router's enum so
nothing walks the central directory twice. Gap 30's E.3 is untouched, and
milestone 1's measurement is why it can be: **0 of 26 books** carries
`[Content_Types].xml` or `_rels/.rels`, so an EPUB fails E.3 at step 2's first
check having read nothing.

**XPS before EPUB is a decision and not a consequence**, and it needed a fixture
nobody would otherwise build: a conforming one-page XPS package carrying a
`META-INF/container.xml`. Nothing in OCF forbids a container from also carrying
OPC's two items, so a file can satisfy both tests — and the format that
publishes a recipe for recognising itself goes first. Under the opposite order
that package comes back as `RootfileMissing`, which is what
`a_package_that_is_both_an_xps_and_a_container_is_read_as_the_xps` catches and
what nothing else in the suite can see. Its page is 400 × 500 XPS units rather
than the natural 816 × 1056, because 816 × 1056 at 96 to the inch is US Letter
to the point — which is also `xps.rs`'s fallback page size, so the obvious
fixture would have passed with the markup never read.

**The signature is `META-INF/container.xml` and nothing else.** Not the
`mimetype` rule, which is argued below; not `META-INF/`, because one of
milestone 1's two producers writes a deflated, zero-byte `META-INF/` directory
record into every book it makes — so a comic archive that acquired one from an
archiver that preserves empty directories would be refused as a broken book by a
reader that tested the directory. And the comparison is byte-exact and
**case-sensitive**, per §4.2.3: `META-INF/Container.xml` is not the container,
where OPC 6.2.2.3 one module over folds ASCII case for the same comparison. Both
near misses have their own fixture and both injections were caught.

### §4.3.2's five clauses, and the pair no real book can separate

The `mimetype` rule is checked and **never refuses**. Five defects, one per
clause, because a book whose `mimetype` is deflated and a book whose `mimetype`
is second are two different bugs in two different producers; a container that
breaks four of them at once is warned about four times and read to exactly the
same place a conforming one reaches.

The clause the plan singled out is *"first file in the archive"*, and it is
`header_offset == 0` rather than `index == 0`. Milestone 1 measured all
twenty-six books putting `mimetype` first in **both** physical and directory
order, so no real file can tell the two checks apart, and it built the container
that does. Milestone 3 needed **two** of them and not one, which is this run's
clearest instance of the rule the last eight milestones keep finding: an archive
whose physical order is wrong and whose directory order is right catches a build
checking `index`, and an archive the other way round catches the same build
failing in the other direction, by warning about a container that conforms. One
fixture proves half a rule.

### The extra-field question, settled, and by neither of the two ways the plan named

The plan said *"either `Entry` grows a field or the OCF layer reads the local
header from the archive's own bytes"*, and the second is not available as
written: `Archive` does not lend its bytes and should not start. The answer is a
third — **`Archive::local_extra(index)`**, which parses the one local header a
caller asks about and hands back the area itself rather than its length.

The argument for it over a field on `Entry` is that the two answer different
questions. §4.3.2's clause is about the **local** header's extra area, which is
what puts `application/epub+zip` at offset 38 of a conforming container; APPNOTE
4.4.11 lets the local and central areas differ and routinely they do, so an
`Entry` field — the directory's view — would be right about a different thing.
Filling one in from the local header instead would mean parsing every local
header at `Archive::open`, and that reader's whole posture is that opening a
700 MB archive costs the directory and not the archive. So it is computed on
demand, for the entries somebody asks about, and nothing else in this workspace
has ever needed to know.

The fixture writes the area on the **local** side only, which is legal and is
what a real Info-ZIP build does with its extended timestamp — and it is what
tells the two implementations apart: injecting "read the central directory's
copy" is caught by three tests, one of them in `tinker-pdf-zip` itself.

Measured across all twenty-six books the answer is still zero everywhere, so
this clause is proved entirely by fixtures. That is the honest state of it, and
it is why the accessor was worth building rather than skipping: milestone 1
could only measure the clause by hand.

### The base that is not the referring document

**The plan's own design section was wrong about this and is amended in place.**
It says relative references resolve against the referring document's path — true
of every reference in the format except the one milestone 3 actually resolves.
§4.2.6.3.1 defines `container.xml`'s `full-path` as a path from the **container
root**, so resolving it against `META-INF/container.xml` yields
`META-INF/EPUB/content.opf`, which no book on earth holds.

The analogy to gap 30 survives and points the other way: a `.rels` part's
targets resolve against the part that owns it rather than the `_rels` directory
it sits in, and `full-path` resolves against the root rather than the `META-INF`
directory it sits in. Both are one segment too many and both fail in the
direction that looks like a missing file.

Which is why the fixture is not a book with a package document under `EPUB/`. On
such a book the wrong base fails visibly and any test would catch it. The
fixture is a book holding **both** `EPUB/content.opf` and
`META-INF/EPUB/content.opf`, so the wrong base resolves happily and hands back
the wrong package document — and the assertion is on which entry the rootfile
names, not on whether it resolved.

The general rule is still built, and it is exercised at a base milestone 3 has
no caller for, because a contract exercised only by its first caller means
whatever that caller happened to want. Milestone 4's manifest `href`s are what
will use it.

Two more decisions live in that function and both are recorded where they are
made. An over-climbing `..` is **refused** where RFC 3986 §5.2.4 clamps, and the
divergence from `xps/opc.rs`'s resolver is asserted in a test that calls both:
inside a container, discarding the segment renames a reference to a *different*
resource that may well exist, and §4.2.3 forbids `..` in a container path at
all, so there is nothing to clamp to. And dot segments are removed **before**
percent-decoding, per RFC 3986, so `%2E%2E` is a name and not a climb — the
input that separates the two orders is `a/%2E%2E/b.opf`, which decoding first
resolves to `b.opf`.

### Four refusals, and the one that is temporary

| Refusal | What it is a sentence about |
| --- | --- |
| `UnreadableContainer` | `META-INF`'s own structure: a `container.xml` that is not well formed, a root that is not `container`, no `rootfile` naming a package document, a `full-path` that is not a container path, or an `encryption.xml` that will not read |
| `RootfileMissing` | a container naming a package document the book does not hold — and the **only** observable difference between the two bases above |
| `EncryptedResources` | an `encryption.xml` naming something that is not one of OCF's two font obfuscations, or naming no algorithm at all |
| `UnpaginatedBook` | an EPUB, read as far as its container goes, with no layout engine yet |

Each has a fixture built for it and the sweep asserts each by name.
`EncryptedResources` is deliberately **not** `ArchiveRefusal::Encrypted`, which
is a sentence about a comic archive whose every page entry the ZIP reader
refused — and milestone 1's `refusal_is_true_of_a_book` predicate excludes that
variant, so reusing it would have failed the pinned tests it was written to
satisfy. That is a predicate written a milestone early doing the job it was
written for.

`UnpaginatedBook` is the one milestone 4 removes from the path a valid book
takes, and its doc comment says so.

### Recognising a name and acting on it are two rules

All six of §4.2.6.3's reserved names are recognised. **Two of them are parsed**
— `container.xml` and `encryption.xml` — and the other four are recognised and
*ignored*, which is the act rather than the absence of one. The test puts the
same unreadable bytes at each of the six names in turn: the two that are parsed
refuse and the four that are not do not. A build that parsed all six would
refuse a book over a `signatures.xml` it has no opinion about; a build that
parsed none would hand ciphertext to a font parser. Only asserting both halves
tells them apart.

A **seventh** file in `META-INF` is neither refused nor warned about, and that
is milestone 1's measurement rather than a preference: every pandoc book carries
`META-INF/com.apple.ibooks.display-options.xml` and zero of the twenty fetched
books carries anything unreserved there at all, so a refusal would have lost
every book one producer writes while passing the entire downloaded corpus. A
warning every book of one producer trips is not a warning either.

### What the real books forced that the plan did not predict

- **Both obfuscated samples write `encryption.xml` with a redeclared *default*
  namespace**, not a prefix: `<EncryptedData
  xmlns="http://www.w3.org/2001/04/xmlenc#">` inside an `<encryption>` root that
  is in OCF's own namespace. Every published example of this file uses an `enc:`
  prefix. A reader that matched the prefix — or that matched local names without
  checking the namespace at all — finds nothing and calls the book
  **unencrypted**, which is the dangerous direction of being wrong. The parser
  resolves namespaces, so it reads both; the test that says so is written from
  the real file rather than from the specification's example.
- **The package document sits in four different places across twenty-six
  books**, not the two milestone 1 recorded: `EPUB/`, `OEBPS/`, `OPS/` and the
  archive root. Every one of them is a path from the container root, which is
  what makes the base question above load-bearing on the first real file rather
  than on an exotic one.
- **Not one of the twenty-six books exercises a single one of §4.3.2's five
  clauses.** All twenty-six put `mimetype` first physically and in the
  directory, stored, twenty bytes, with no extra field. So every warning this
  milestone can emit is proved by a fixture and by nothing else — which is worth
  writing down, because a clause with no real example is a clause whose
  behaviour is a guess until somebody builds the container.
- **The fetched corpus is the check on the whole discrimination and it is
  unanimous.** Twenty books, twenty `UnpaginatedBook`, none refused for a
  container that would not read and none for a rootfile that named nothing. The
  sweep asserts the *distribution* and not only the predicate: a build whose
  resolution regressed would come back as `RootfileMissing` twenty times and
  would satisfy "a name that is true of a book" without satisfying this.

### The injection matrix

Twenty-four defects, one at a time, each reverted before the next, `cargo test
-p tinker-pdf -p tinker-pdf-zip --no-fail-fast` re-run over the whole facade and
the archive reader with the fetched corpus attached. **Twenty-three caught on
the first pass; the survivor was closed and its injection re-run, and it is now
twenty-four.**

| Defect | Caught by |
| --- | --- |
| EPUB routed before XPS | `a_package_that_is_both_an_xps_and_a_container_is_read_as_the_xps` — **alone** |
| The discrimination tests `META-INF/` rather than the file in it | `a_comic_that_carries_a_meta_inf_directory_is_still_a_comic`, and one more |
| The container's name compared with ASCII case folded | `a_comic_carrying_meta_inf_files_that_are_not_the_container_is_still_a_comic` — **alone** |
| **`index == 0` rather than `header_offset == 0`** | `the_mimetype_rule_is_physical_order_and_not_directory_order` — **alone**, and only because it carries both containers |
| The extra-field clause deleted | `each_of_the_mimetype_clauses_warns_on_its_own`, and one more |
| The extra area read from the **central directory's** copy | three, one of them `tinker-pdf-zip`'s own |
| A broken `mimetype` refuses rather than warns | `a_container_that_breaks_every_mimetype_clause_is_still_read_as_a_book` — **alone** |
| **`full-path` resolved against the referring document** | six |
| An over-climbing `..` clamped rather than refused | `a_climb_above_the_container_root_is_refused_where_rfc_3986_clamps`, and one more |
| Percent-decoding before dot-segment removal | `a_percent_escape_is_decoded_per_segment_and_a_decoded_separator_is_refused` — **alone** |
| The path length charged only *after* the merge | `a_content_path_past_the_length_cap_is_refused_before_it_is_merged` — **alone** |
| The path length charged only *before* the merge | the same test — **alone**, by its other half |
| An `<EncryptedData>` naming no algorithm accepted | `an_encrypted_data_that_names_no_algorithm_is_refused_too` — **alone** |
| Any encryption algorithm accepted as an obfuscation | three |
| `encryption.xml` never read at all | three |
| XML Encryption matched by prefix rather than by namespace | six |
| The read-once cache deleted | `resolving_a_file_twice_does_not_inflate_it_twice` — **alone** |
| The first `<rootfile>` taken whatever its media type | `the_default_rendition_is_the_first_rootfile_that_is_a_package_document` — **alone** |
| **The root element of `container.xml` not checked** | **nothing, on the first pass** — see below |
| `container.xml` read under the relaxed doctype mode | `a_doctype_on_container_xml_is_refused_rather_than_skipped` — **alone** |
| An unrecognised file in `META-INF` refused | three, including every committed book |
| An encrypted book reported as the comic path's `Encrypted` | three |
| `RootfileMissing` collapsed into `UnreadableContainer` | `every_book_level_refusal_is_returned_by_a_fixture_built_for_it`, and one more |
| The rootfile not checked against the entries the book holds | the same two |

**The survivor is the best thing the matrix found, and it is the failure mode
this run was told to watch for: a check that no test could see because every
fixture was refused by something else first.** Deleting the root-element check
in `container.xml` — so a file at that name is read whatever its root says —
changed no answer in the whole suite. All four fixtures the test carried were
refused by `out.is_empty()` instead: a wrong root over an empty `<rootfiles>`,
and a wrong root whose children are therefore in the wrong namespace, both yield
no `<rootfile>` at all and the emptiness check catches them. The check was
enforced twice and only the second was reachable.

What separates them is a **usable `<rootfile>` under a root that is not a
container** — the shape a `META-INF/container.xml` holding somebody else's XML
actually has. Two cases now: the right namespace with the wrong element, and the
right element with the wrong namespace and its children in OCF's, so the
`<rootfile>` is found either way. With the correct build both refuse; with the
injection both hand back a package document. The re-run catches it.

**Two of the twenty-four were written as injections before they had a test, and
both tests were written before the matrix ran.** Charging the path length only
after the merge, and taking the first `<rootfile>` whatever its media type,
were both predicted to survive while the injection list was being drawn up —
so `a_content_path_past_the_length_cap_is_refused_before_it_is_merged` gained
the reference that is nine bytes past the cap and five bytes after resolution,
and `the_default_rendition_is_the_first_rootfile_that_is_a_package_document` was
written. Both are recorded here rather than reported as clean catches, because
a matrix that quietly closes its own gaps before it runs is a matrix that
measures the person writing it.

**The harness itself had a defect worth recording, and it is a methodological
one.** The first pass reverted each injection with `shutil.copy2`, which
restores the pristine **mtime** along with the bytes — and cargo decides
freshness by mtime. A file reverted to a stamp older than the build that used
the injection looks up to date, so its crate is not rebuilt and the injection
survives into the next run. Within one crate this is invisible, because the next
injection's own edit forces a full recompile of that crate; **across a crate
boundary it is not.** The `tinker-pdf-zip` defect was still in the binary two
injections later, and the only reason it was noticed is that its failure list
carried a ZIP test into two runs that could not have touched one. Reverting now
copies the bytes and stamps the file, and every injection from that one onward
was re-run. A revert that the build system cannot see is not a revert.

### Still owed

- **The §4.3.2 warnings have nowhere to go through `Document::open`.** A refused
  book carries no `ArchiveReport`, so `OcfWarning` is reachable only through
  `epub::ocf::Ocf` and is asserted there. Milestone 4 maps it into
  `ArchiveWarning` in the same commit that gives a book a report to carry.
- **Two entries with one name are not detected.** §4.2.3 forbids a container
  from holding two files at one path, and `Ocf::index_of` takes the first, where
  `opc::Package::validate` refuses the analogous case outright. The argument for
  leaving it is that gap 30's refusal is over *part names*, which is the whole
  addressing model of that format, and OCF's equivalent bites in the package
  document's manifest rather than in the container. Milestone 4 decides it, with
  the manifest in front of it.
- **§4.2.3's 255-byte file-name limit is not enforced**, deliberately: it is an
  interoperability rule about file systems this engine never writes to, and
  refusing a path that names an entry the container actually holds would lose a
  book over somebody else's `PATH_MAX`. Recorded beside `MAX_OCF_PATH_LEN`,
  which is the bound that is enforced.
- **No fuzz target reaches the OCF layer.** The archive underneath has one and
  the XML above it has one; the name arithmetic between them has none, and
  milestone 13's campaign is where that lands.
- **Milestone 2's owed facade test is only half discharged.** A real book now
  reaches `tinker-pdf-xml` through `container.xml` and `encryption.xml` and
  through no content document at all, so *"a test that opens a real EPUB and
  asserts its content documents parse"* still has nowhere to live. It moves to
  milestone 8, which is the first milestone that reads one.
- **`MAX_OCF_PATH_LEN` is the eighteenth row of `bounds_ledger.rs`** and carries
  a zero against both existing yardsticks, which is an answer rather than a
  blank: a comic archive has no container paths and a fixed document has OPC
  part names, which are a different grammar. The book yardstick milestone 13
  adds is the one that will give it a real second figure.

## Progress — 19 August 2026, milestone 4

**A book has pages.** `tpdf info` on a real EPUB reports a spine's worth of
pages at the stated box, with the book's title, where milestone 3 left an
honest `UnpaginatedBook` refusal. The workspace stands at **2 355**.

Every page is the neutral grey placeholder carrying a named warning. That is
not a shortfall to be apologised for — it is what milestones 6 to 12 replace,
one property at a time, and a page that drew *nothing* and reported success
would be gap 17's failure in a tenth format.

### `OpenOptions`, and why the seam had to move

The plan found this structurally wrong and it is worth restating where the fix
lives. `RenderOptions` arrives **after** pagination and `with_fonts` arrives
**after** `open`, yet advance widths decide every line break — so a reflowable
book's page count cannot be a property of the file the way a PDF's is.

`Document::open_with(bytes, &OpenOptions)` is the answer, with `open(bytes)`'s
signature **untouched and asserted so**. The default box is six inches by nine
at 12 pt, and the doc comment says outright that **the page count is a function
of that number and not a property of the file**. MuPDF's `-W -H -S` is the same
shape, arrived at from the same constraint.

A font provider handed to `open_with` reaches the render; one handed to a
reflowable book *after* opening **warns by name**, because by then the lines
are already broken. That warning fires on a book and on nothing else — a PDF is
unaffected — and the injection matrix has a separate defect for each half.

### The text-conservation harness, built now on purpose

*Every character in the spine appears on some page, exactly once, in order.*

It is built here, against thirteen grey placeholders, where it is trivially
satisfiable — and that is the point rather than a weakness. Nine later
milestones **inherit** it. Acquiring it at milestone 10 would mean writing it
against code that already violates it, and the natural response then is to
weaken the harness rather than the layout.

Both sides are separately attacked in the matrix: the source side reads text
and not markup, the paginated side reads pages in order, extra text is
reported, and two elements' text does not run together.

### What the matrix found, and the one that survived

Twenty-nine defects, one at a time, each reverted before the next.
**Twenty-eight caught on the first pass; the survivor was a real gap and is
closed.**

| Defect | Caught by |
| --- | --- |
| the caller's page box is ignored and every book is the default | `every_committed_book_paginates_to_its_spine_at_the_box_it_was_given`, and 2 more |
| the default page box is US Letter rather than six by nine | `every_fetched_book_paginates_to_its_own_spine`, and 2 more |
| a spine item that does not resolve is dropped rather than paged | `a_container_that_breaks_every_mimetype_clause_is_still_read_as_a_book`, and 7 more |
| the placeholder is white rather than the neutral grey | `every_page_is_the_neutral_placeholder_and_every_page_says_why`, and 1 more |
| the placeholder page draws nothing at all | `every_page_is_the_neutral_placeholder_and_every_page_says_why`, and 1 more |
| no page carries a named warning | `an_unresolved_spine_item_still_makes_a_page_and_keeps_its_place`, and 2 more |
| the container's mimetype warnings never reach the report | `the_mimetype_clauses_reach_a_callers_report_now_that_a_book_has_one`, and 1 more |
| an unimplemented property is warned about once per item | `an_unimplemented_manifest_property_is_named_once_with_its_count` — **alone** |
| `nav` and `cover-image` are reported as unimplemented too | `an_unimplemented_manifest_property_is_named_once_with_its_count`, and 1 more |
| the unique identifier is the first dc:identifier whatever its id | `the_three_required_dublin_core_elements_and_the_unique_identifier` — **alone** |
| a manifest href resolves against the container root | `an_unresolved_spine_item_still_makes_a_page_and_keeps_its_place`, and 6 more |
| the fallback chain has no cycle guard | `a_fallback_chain_reaches_a_content_document_or_says_why_it_did_not` — **alone** |
| the fallback chain has no depth cap | `a_fallback_chain_reaches_a_content_document_or_says_why_it_did_not` — **alone** |
| a spine item terminates on any core media type, not a content document | `an_unresolved_spine_item_still_makes_a_page_and_keeps_its_place`, and 1 more |
| any package version is read as EPUB 3 | `a_book_whose_package_document_is_wrong_is_refused_by_name`, and 3 more |
| an empty spine opens as a book of no pages | `a_book_whose_package_document_is_wrong_is_refused_by_name`, and 2 more |
| the three required Dublin Core elements are not checked | `the_three_required_dublin_core_elements_and_the_unique_identifier` — **alone** |
| the book's metadata never reaches the synthesised document | `qpdf_reads_the_books_title_out_of_the_synthesised_documents_info`, and 1 more |
| `open_with` ignores the options it was handed | `an_unusable_page_box_is_replaced_and_named_rather_than_refused`, and 3 more |
| a font provider passed at open is dropped for a PDF | `a_font_provider_passed_at_open_reaches_the_render` — **alone** |
| a late font provider is accepted in silence | `a_late_font_provider_warns_on_a_book_and_on_nothing_else` — **alone** |
| a late font provider warns on every document, book or not | `a_late_font_provider_warns_on_a_book_and_on_nothing_else` — **alone** |
| a comic archive's report claims to be reflowable | `a_late_font_provider_warns_on_a_book_and_on_nothing_else` — **alone** |
| two entries at one path are not detected | `two_entries_at_one_path_are_warned_about_and_the_first_one_wins` — **alone** |
| the duplicate-path check reports every container | `a_seventh_file_in_meta_inf_is_ignored_rather_than_refused`, and 1 more |
| the conservation harness reads a stylesheet as the book's text | `the_source_side_reads_the_text_and_not_the_markup` — **alone** |
| the conservation harness never reports extra text | `a_paragraph_repeated_across_a_page_break_is_extra_text`, and 1 more |
| the conservation harness reads the pages in reverse | `the_paginated_side_reads_a_real_documents_pages_in_order` — **alone** |
| the conservation harness lets two elements' text run together | `the_source_side_reads_the_text_and_not_the_markup`, and 1 more |

**The survivor is the session's signature shape, wearing a comment that claimed
the opposite.** `the_source_side_reads_the_text_and_not_the_markup` excludes
`head`, `script` and `style`, and carries a comment saying *"each exclusion on
its own, because a build that dropped one of the three would still pass a test
that only looked at the joined string."* Its `<style>` was **inside `<head>`**.
The head rule already excluded it, so the style rule was never the reason, and
deleting `style` from the skip list changed no answer. A second stylesheet in
the **body** — which HTML allows in flow content and real books use — is what
separates them, and it is now there.

That the comment asserted the separation and the fixture did not provide it is
the whole lesson: a stated intention is not a test.

### The harness itself, twice

Milestone 3 found its injector restoring files with `shutil.copy2`, which
preserves mtime — and cargo decides freshness by mtime, so an injection in
another crate was still in the binary two injections later. This milestone's
injector writes bytes and calls `os.utime`, and that fix held.

The run was then interrupted at defect 16 with the defect **applied**. The
`APPLIED.json` marker named it, the one red test was the one written to catch
it, and the tree's state was never in doubt — which is what that marker was
added for after two earlier milestones were bitten. Reverting it needed care
the marker could not give: the injection *deleted* a block and set its
replacement to the empty string, so the harness's own revert had nothing to
match, and the file was uncommitted milestone work that `git checkout` would
have destroyed. The `NoTitle` check was reinstated by hand from its neighbours
and the matrix resumed from 16.

### Still owed

- **Thirteen pages of grey.** Every page is a placeholder until milestone 8
  reads a content document. The warning says so per page, and the conservation
  harness currently compares a book's text against nothing being laid out —
  which it must, because nothing is.
- **`with_fonts`'s warning has one consumer and no fixture from a real book**:
  no committed book needs a font this engine lacks, so the path is proved by
  hand-built fixtures only.
- **The duplicate-path check is milestone 3's deferral, landed here**, and it
  reports the first entry as the winner. Which entry a conforming reader should
  prefer is not specified; the choice is recorded rather than argued.
- Milestone 2's owed facade test still belongs to milestone 8, which is the
  first milestone that reads a content document.

## Progress — 19 August 2026, milestone 5

**The writer has link annotations and an outline**, and nothing consumes either
yet. That is row 5's own arrangement and gap 30 milestone 5's: build the writer
before the milestones that need it, because a fallback which produces plausible
output is worse than one that produces none. The workspace stands at **2 367**.

`DocumentBuilder::link` takes a rectangle and a `Target`; `set_outline` takes a
tree of `OutlineEntry`. **Nothing in either mentions EPUB** — row 5's last
clause, and the test of whether the work belongs in the writer rather than in a
format. `tests/writer_navigation.rs` says the word once, in a paragraph
explaining why it appears nowhere else.

### The round trip is through this repository's own reader

Every other milestone here checks a writer by reading its bytes back with an
assertion written beside the writer. This one does better and is the only one
that can: the facade **already reads** outlines and link annotations, so a
document is built, saved through `DocumentEditor::save`, reopened with
`Document::open`, and compared against what was asked for through the **public**
`Document::outline()` and `Page::links()`.

That is gap 30 milestone 5's argument about `MAX_FUNCTION_DEPTH` in a second
place: **a writer whose output its own reader cannot read is not a writer.**

### 12.3.3's `/Count` is a sign and a magnitude, and they are two claims

An open entry states its visible descendants; a closed one states the *negative*
of them. Get the sign wrong and the tree opens the wrong way in every viewer
while reading back identically through any reader that turns it into a `bool` —
which this repository's does. So the sign is attacked from **both** directions
in the matrix, because a build that wrote one sign for everything passes a test
that only has the other, and the magnitude is attacked separately again.

qpdf confirms all three numbers from outside: `/Count 1` on the open parent,
`/Count -1` on the closed one, and `/Count 3` at the root — three *visible*
items, because the fourth sits under a closed parent and is not one of them.

### What qpdf caught that nothing here could

**Two of the eleven defects are invisible to this repository entirely.**

`/Border [0 0 0]` omitted: the reader does not surface border style, so a link
that draws a visible box in every viewer round-trips perfectly. And **the `/Prev`
chain deleted**: 12.3.3's siblings link both ways, this reader walks `/Next`
forward only — which is enough to build the tree — so no round trip can see the
back-links missing. A viewer walking *up* from a selected entry can.

That is the third and fourth time in this session an external oracle has caught
something no internal test could, after gap 29 milestone 5's shared resource
table and gap 30 milestone 4's `/CropBox`.

The output format was read before it was asserted against, which is now a
standing habit rather than a precaution: `--json-key=objects` returns nothing
useful here, and the outline arrives instead as a resolved tree with `title`,
`open` and `destpageposfrom1`. Eight milestones running have had to rewrite an
assertion after assuming a format.

### The injection matrix

Eleven defects, one at a time, each reverted before the next. **Six caught on
the first pass; five survived, every one a real test gap, all five closed.**

| Defect | Caught by |
| --- | --- |
| The closed form writes a positive count | qpdf, and the sign test |
| The open form writes a negative count | the same two |
| A closed entry's descendants count toward the level above | qpdf — **alone** |
| An entry contributes nothing of its own to the count | qpdf, and the sign test |
| The root states its own children rather than every visible item | qpdf — **alone** |
| A link's `/Border` is left out | **qpdf — alone** |
| **`/Rect`'s corners written unordered** | survived — now `a_rectangle_given_backwards_is_written_normalised` |
| **A degenerate `/Rect` accepted** | survived — now `a_rectangle_with_no_area_is_refused_and_leaves_nothing_behind` |
| **An unwritable link target written anyway** | survived — now the refusal test, which needed a second consequence |
| **An unwritable outline replaces the one already set** | survived — now `an_unwritable_outline_leaves_the_previous_one_standing` |
| **The sibling chain has no `/Prev`** | **survived — now qpdf, alone** |

**The five survivors share one shape and it is worth naming.** Each was a
property visible only from a direction no fixture had come from. Every `/Rect`
fixture gave its corners already ordered, which is what a writer's own author
naturally writes. Nobody wrote a zero-area link. Every outline fixture set one
outline *once*, so a build that cleared the document before validating the
replacement passed — and that failure is silent, leaving a caller who ignores
the `false` with no navigation at all rather than the navigation it had.

### One assertion of mine was wrong and the contract was right

`set_outline(vec![])` **clears** an existing outline and answers true. The first
version of that test asserted it answered false. The contract is explicit and
the distinction is gap 21's whole subject: a document with no `/Outlines` and a
document with an empty outline dictionary are different files. The test now sets
an outline and then clears it, because passing an empty vector to a builder that
never had one proves nothing — every implementation passes that, including one
whose empty case is `return true` and nothing else.

### Still owed

- **Nothing consumes any of this.** Milestone 8 is where a content document's
  `href` becomes a link and its headings become an outline; this is the seam,
  built early on purpose.
- **A named destination is not writable.** `Target` has a page and a URI and no
  third arm: a name is only a destination once the catalog carries a
  `/Names /Dests` tree, and building one is not this milestone's.
- **`/GoToR` is not written either** — a destination in another file. An EPUB's
  cross-references are internal, so nothing here needs it, and writing an
  action this repository cannot then resolve would be the shape row 5 exists to
  avoid.

## Progress — 20 August 2026, milestone 6

**`tinker-pdf-css` is the ninth leaf and it has no dependencies at all** — the
fourth crate in this workspace in that position, beside `filters`, `crypto` and
`xml`. 2 458 tests, 7 ignored, up from 2 367; ninety-one of them are new —
eighty-three the crate's own unit tests, five the compile-time proof, one a
doctest, and two in the facade over the six committed books.

The empty allow-list turned out to be load-bearing rather than tidy, and that is
the finding this milestone would keep if it could keep only one. Because the
crate has no internal dependency, no third-party one and no build script, **the
whole of it compiles with a bare `rustc`** — no dependency resolution, no
`--extern`, nothing. That is what lets decision 5's proof be a real build of the
real source with a defect injected into it, rather than a small copy of the
pattern that would prove only that `match` is exhaustive in Rust.

### The compile-time proof, and it does fail the build

Row 6's unusual exit criterion is that the defect is *"injected as a defect and
asserted to fail the build, not a test"*. It is, and here is what it says:

```
error[E0004]: non-exhaustive patterns: `&property::Property::Widows(_)` not covered
   --> crates\tinker-pdf-css\src\cascade.rs:248:11
    |
248 |     match property {
    |           ^^^^^^^^ pattern `&property::Property::Widows(_)` not covered
```

`tests/unimplemented_property_does_not_build.rs` copies `src/` into
`CARGO_TARGET_TMPDIR`, adds one variant to `Property`, withholds one consumer's
arm, and runs `rustc --emit=metadata` over it. Four things about its shape are
deliberate:

- **It compiles the pristine copy first and asserts that succeeds.** Without
  that, a harness that wrote a broken tree — a missing module, a bad path, a
  `rustc` that is not on `PATH` — would report every injection as "the build
  failed", which is the answer they are asserting, and the whole file would pass
  while proving nothing. Gap 20's finding, arriving through a harness instead of
  through CI.
- **It compiles the same variant with *all three* arms supplied and asserts that
  builds.** Otherwise the three failures above would be true of any edit at all,
  and "the build broke" is not "the build broke *because* the property has no
  consumer".
- **The three consumers are injected separately**, because one `match` is one
  consequence. `cascade::apply` is the one the plan names — a property parsed
  and never written into a computed style. `Property::name` is the second: a
  property applied and then anonymous in every warning and in the `Unsupported`
  census the milestone is judged on. `Property::inherited` is the third, and it
  is the quietest of them — a property that neither inherits nor does not is
  right on the element that sets it and wrong on every descendant.
- **It refuses to guess an anchor.** The injector asserts the marker comment
  occurs exactly once and panics otherwise, because a silent no-op would make
  every injection a copy of the pristine build.

The variant injected is `widows`, and that is not arbitrary. `widows` is in
`UNSUPPORTED_PROPERTIES` today — CSS 2.2 §13.3.2, present in both producers'
books, genuinely unimplemented because the fragmentation that would consume it
is milestone 7's. So the defect is the exact edit somebody will make when that
milestone arrives: promote the name out of the unsupported list into the enum.
The test asserts what happens if they stop there.

`rustc` is not optional and the test does not skip when it is missing. A proof
that quietly does not run reads exactly like a proof that passed.

### The specificity table, and the rows that earned it

Thirty-one selectors, against row 6's twenty. Two — `#s12:not(foo)` and `.foo
:is(.bar, #baz)` — are copied verbatim from `selectors-4` §15's own worked
table, so part of the arithmetic is the specification's rather than this
author's. The rows that matter:

| Selector | A/B/C | Why it is in the table |
| --- | --- | --- |
| `.a` | 0,1,0 | The control for the row below it |
| `:not(.a)` | 0,1,0 | **Equal to `.a`.** `:not()` contributes its *argument*'s specificity and nothing of its own; a build that counted it as a pseudo-class makes it one step stronger, and that only shows when the two meet in one cascade |
| `:not(#a)` | 1,0,0 | The same rule from the other side, so "always 0,1,0" fails too |
| `:not(em, strong#foo)` | 1,0,1 | The **most specific** argument, not the first and not the sum |
| `:is(#x, p)` | 1,0,0 | §15's most-specific-argument rule |
| `:is(p, #x)` | 1,0,0 | The same list reordered, which is what tells "most specific" from "first" |
| `:where(#x, p)` | 0,0,0 | Zero however specific its argument — the whole reason `:where()` exists |
| `:is(:not(#a), .b)` | 1,0,0 | Nested: the inner `:not` decides the outer `:is` |
| `::before` | 0,0,1 | A pseudo-**element** counts in C, like a type selector |
| `p::before` | 0,0,2 | And adds to the type selector rather than replacing it |
| `p:before` | 0,0,2 | CSS 2.1's one-colon spelling is the same pseudo-element, and real books write it |
| `p:hover` | 0,1,1 | A pseudo-class counts in B **even though this build never matches it** |
| `:has(#x)` | 1,0,0 | §15 gives `:has()` its most specific argument too, and nothing here evaluates it |
| `*` | 0,0,0 | The universal selector counts nowhere |
| `p > *` | 0,0,1 | Which is what makes this 1 and not 2 |
| `#a#b` | 2,0,0 | Two ids in one compound, which a build keeping only one would get wrong twice over |
| `.a.b.c.d.e.f.g.h.i.j.k` | 0,11,0 | Eleven, and `#x` still beats it — the row that fails for a build packing A/B/C into `a * 100 + b * 10 + c` |

The last is the one worth keeping. Every stylesheet with fewer than ten classes
on a selector passes a base-ten implementation, and no book announces that it
has an eleventh.

### What the design got wrong, and how I found out

**The `Unsupported`/invalid line was in the wrong place, and the injection
matrix is what said so.** The first version classified any value a property
could not read as `Unsupported` — decision 5's own count — which meant
`margin-top: red` and `color: rgb(1;2;3)` were filed as gaps in *this build*.
They are not; they are the author's typos, and §5.4.4 discards them like any
other malformed declaration. The number the whole milestone is judged on would
have been inflated by every stylesheet error in every book. Three classifiers
came out of that — `LenOutcome`, `ColourOutcome` and the CSS-wide keyword
check — each of which has to say `Unsupported`, `Invalid` or a value, and the
distinction is written out where they are declared. `width: 50vw` is this
build's gap and `width: red` is not.

**The tokenizer was right and my test was wrong, once.** `<!--a-->` does not
produce a CDC: `-` is a name code point, so the identifier runs `a--` and what
is left is a `>` delim. The test now asserts both spellings and says which one
is the reason.

**The metric length units cannot be asserted exactly.** `2.54cm` is
95.999999999999989, because the conversion is a division by a value that is not
a binary fraction. Asserting 96.0 would be asserting something untrue about IEEE
754. Ruling 4 asks for the *same* answer on every target, which a correctly
rounded multiply and divide give; it does not ask for the decimal one. `in`,
`pt`, `pc` and `px` are whole-number ratios of 96 and are asserted exactly, and
the three that are not are asserted to 1e-9 with the reason beside them.

**`MAX_SELECTOR_MATCHES` counts the wrong thing in the plan, and the row is
amended in place.** The bounds table calls it *"selector-against-element
attempts"*. Matching `a b c d` walks the ancestor chain with backtracking, so
one attempt costs `O(depth^parts)` compound tests — a cap on the attempts bounds
a number that is not the work. It charges per **compound**-against-element test,
which is `5adf502`'s sentence one level further down than the plan had it.

**`MAX_DOM_NODES` had to be declared in the CSS crate**, which is not where gap
31's table puts it. The `const` block the plan asks for —
`MAX_CSS_RULES × MAX_DOM_NODES > MAX_SELECTOR_MATCHES` — can only name constants
its own crate can reach, and this crate's allow-list is empty. So the product
relation lives here and the *other* half the plan asks for,
`MAX_DOM_NODES < MAX_XML_TOKENS`, cannot: it is owed by the facade at milestone
8, and until then it is this row's `reachable` column in `bounds_ledger.rs`,
which is exactly the check it would be.

### The index, and why it needed a test of its own

The matcher buckets each selector by its rightmost compound's most selective key
— id, then class, then type, then a universal bucket — and tests an element only
against the candidates. Without it a 400-page novel would spend on the order of five
million compound tests and the cap would have to sit above that, which would
make the firing fixture take a quarter of a minute; with it the same book is
estimated at about half a million. Both figures are extrapolations and are
labelled as such in the ledger: nothing in this repository cascades a real book
until milestone 8, and what *is* measured is the parse side — see below.

**The index is an optimisation and it is not the bound.** A stylesheet whose
every rule names one class puts every rule in one bucket and gets the full
rules-times-elements product, which is exactly what a hostile book would write
and exactly what the firing fixture builds: 2 001 rules against 2 000 elements
is 4 002 000 tests against a cap of 4 000 000.

And a bucketing bug is invisible in the worst way — it produces a book styled
slightly *less* than it should be, which reads as a plain stylesheet rather than
as a defect. So `an_indexed_lookup_and_a_brute_force_one_agree` compares the
index's answer against testing every selector, per element, and asserts each
element matched something so the comparison is about more than two empty lists.
The fuzz target makes the same comparison over stylesheets nobody wrote.

### What the committed books showed that this milestone did not predict

`tests/epub_css.rs` parses all eight stylesheets of all six committed books
through the real parser at the shipped limits and asserts the maxima, so
`limits.rs`'s first column is a **measurement** recomputed on every run rather
than a number somebody remembered. The figures: largest stylesheet **5 009**
bytes, most tokens in one sheet **1 392**, most rules **45**, most declarations
**99**, longest selector **5** compounds, largest content document **69**
elements, and **no book uses `@import` at all** — asserted rather than assumed,
because the parse is given `NoImports` and one would warn by name. Four
estimates in the first draft of that column were wrong by two to five times in
the flattering direction, which is what a measurement is for.

Not one construct is discarded across the whole corpus. That is asserted too: a
recovery count above zero on two real producers' own output would be evidence
about this parser rather than about the producers.

**pandoc 3.10.2 writes `light-dark()`.** `css-color-5`'s function, on
`background-color`, on `color` and inside both `border-*` shorthands —
`light-dark(transparent, #232629)`. Every one of those properties is
implemented here and the function is not, so each lands as `Unsupported` with
the value beside it. That is decision 5's second device meeting real input on
its first day, and it is the strongest evidence the milestone has that keying
the implemented set by **(property, value)** was not over-engineering: a build
keyed by property alone would have taken the first argument, or the second, and
produced a book that is entirely plausible and the wrong colour throughout.

**calibre 9.13.0 writes `text-align: inherit`.** Which is survivor 41
corroborated by a real producer inside the same hour it was closed: a
`css-cascade-5` §7.1 keyword on a property whose own keywords are `left`,
`right`, `center` and `justify`.

**Two names were missing from `UNSUPPORTED_PROPERTIES` and the corpus found
both.** `list-style` — the shorthand, where the three longhands were all
present — and `color-scheme`, which pandoc writes on `:root` because
`light-dark()` requires it. Both were being reported as `Unknown`, which is to
say as somebody else's vendor extension rather than as this build's gap. The
committed corpus's `Unknown` set is now **empty**, and that is the interesting
answer rather than a boring one: milestone 1 measured `-webkit-column-count`,
`-epub-text-emphasis-style` and `-ah-margin-start` in the *fetched* corpus, and
neither producer of the committed six writes a single vendor extension.

The whole `Unsupported` set over the committed corpus is twenty properties, of
which **six** are value gaps on properties this build implements —
`background-color`, `border-bottom`, `border-top`, `color`, `display`,
`text-align` — and fourteen are properties it does not: `border-collapse`,
`border-spacing`, `color-scheme`, `hyphens`, `list-style`, `max-width`,
`orphans`, `overflow`, `overflow-wrap`, `overflow-x`, `page-break-inside`,
`quotes`, `vertical-align`, `widows`. Against 772 longhands it does read.

### The injection matrix

**Fifty-two defects, fifty caught on the first pass.** Both survivors were real
gaps, both are closed, and the whole matrix was re-run against the tree as
committed: **fifty-two of fifty-two**.

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | A form feed is not a newline (§3.3) | `preprocessing_folds_three_newlines_and_the_null` |
| 2 | A newline in a string is consumed rather than reconsumed | `a_newline_in_a_string_is_bad_and_the_newline_survives` |
| 3 | An escape of `\0` or a surrogate is not U+FFFD | `escapes_resolve_and_three_become_the_replacement_character` |
| 4 | Non-ASCII does not start a name | `a_no_break_space_is_part_of_an_identifier`, and one more |
| 5 | Whitespace does not collapse across a comment | `a_comment_between_whitespace_is_one_whitespace_token` |
| 6 | Every hash is an id, so `#0f0` is a selector | `a_hash_is_an_id_only_when_it_starts_an_identifier`, `malformed_selectors_are_refused_one_at_a_time` |
| 7 | `!important` is matched case-sensitively | `important_is_the_last_two_values_and_is_case_insensitive` |
| 8 | A declaration whose name is not an identifier is not counted | **survived** — closed, see below |
| 9 | A rule that reaches EOF keeps its prelude (§5.4.2) | `a_rule_with_no_block_is_discarded_and_counted` |
| 10 | The `@import` cycle guard is deleted | `an_import_cycle_is_refused_rather_than_recursed` |
| 11 | The `@import` depth cap is deleted | `an_import_chain_past_the_depth_cap_warns_by_its_own_name` — **as a stack overflow**, not an assertion |
| 12 | `@layer` becomes an ordinary unsupported at-rule | `layer_is_refused_by_name` |
| 13 | A `@media` block is applied unconditionally | `media_queries_are_evaluated_in_both_directions`, and two more |
| 14 | An `@import` after a rule is honoured | `an_import_after_a_rule_is_named_rather_than_read` |
| 15 | Warnings are not deduplicated | `an_unsupported_at_rule_carries_its_name` |
| 16 | The medium is `print` | `media_queries_are_evaluated_in_both_directions`, and two more |
| 17 | An unknown media feature evaluates true | `an_unreadable_media_query_is_false_and_does_not_spread` |
| 18 | `min-` and `max-` are swapped | `media_queries_are_evaluated_in_both_directions` |
| 19 | `:not()` counts as a pseudo-class | `the_specificity_table` |
| 20 | `:is()` takes its first argument | `the_specificity_table` |
| 21 | `:where()` carries its argument | `the_specificity_table`, `is_and_where_match_the_same_set` |
| 22 | A pseudo-element counts in B | `the_specificity_table` |
| 23 | The universal selector counts | `the_specificity_table` |
| 24 | "Most specific argument" is the least specific | `the_specificity_table` |
| 25 | A `::before` rule styles its originating element | `a_pseudo_element_matches_nothing_and_is_named` |
| 26 | `:not(a, b)` is a disjunction | `not_is_a_conjunction_of_negations` |
| 27 | The index buckets on the **leftmost** compound | `an_indexed_lookup_and_a_brute_force_one_agree` |
| 28 | `[href^=""]` matches | `the_attribute_matchers` |
| 29 | `!important` does not reverse the origin order | `criterion_one_important_reverses_the_origin_order`, `the_six_reachable_ranks_are_in_the_specifications_order` |
| 30 | Specificity is sorted above origin | **the build** — `CascadeKey`'s field order is the specification's order, and moving it breaks the type |
| 31 | An inline declaration is an ordinary one | `criterion_three_an_inline_declaration_beats_every_selector` |
| 32 | `font-size` is applied in cascade order | `font_size_is_resolved_before_anything_relative_to_it` |
| 33 | `display` inherits | `inheritance_carries_the_computed_value_and_resets_the_rest` |
| 34 | `font-size` does not inherit | `a_lazy_resolution_and_the_single_pass_agree` |
| 35 | `bolder` adds a hundred | `bolder_and_lighter_follow_the_table` |
| 36 | The **first** declaration wins rather than the last | three cascade criteria at once |
| 37 | The document-order check is dropped | `a_tree_out_of_document_order_is_refused_by_name` |
| 38 | An unknown `float` value becomes `left` | `a_value_outside_a_supported_property_is_unsupported_and_not_its_neighbour` |
| 39 | A refused unit is filed as a typo | the same |
| 40 | An unknown colour name is filed as a typo | the same |
| 41 | The CSS-wide keywords are not reported | **survived** — closed, see below |
| 42 | `margin: 1px 2px 3px` takes its left from the top | `the_box_shorthand_expands_at_every_arity` |
| 43 | The `border` shorthand does not reset what it omits | `the_border_shorthand_resets_what_it_does_not_name` |
| 44 | A `line-height` percentage inherits as a length | `a_line_height_number_inherits_as_a_factor` |
| 45 | An `hsl()` hue is clamped rather than wrapped | `the_colour_syntaxes` |
| 46 | An alpha byte is truncated rather than rounded | `the_colour_syntaxes` |
| 47 | `MAX_SELECTOR_MATCHES` is raised above its own product | **the build** — the `const` block, which is what it is for |
| 48 | The byte cap refuses one byte early | `a_stylesheet_past_the_byte_cap_is_refused_by_name` |
| 49 | The token total is refunded per sheet | `the_token_total_is_spent_across_sheets_and_not_per_sheet` |
| 50 | The match total is never charged | `the_match_budget_refuses_a_stylesheet_that_defeats_the_index` |
| 51 | `cascade::apply` gains a `_` arm | all three compile-time proofs |
| 52 | `Property::name` gains a `_` arm | all three compile-time proofs |

Three of them are worth more than a row.

**Number 11 is not caught by an assertion.** Deleting the `@import` depth cap
leaves the cycle guard in place, and a resolver that answers every href at a
*deeper* address never repeats one — so the recursion is infinite and the test
process overflows its stack. That is a finding rather than a failure of the
test: it is gap 24's `Md5::update` shape, where a missing guard is a hang and
not a wrong answer, and the reason the fuzz target's `@import` invariant is
written as a comment saying libFuzzer's `-timeout` is what reports it. The
matrix reads a non-zero exit code as caught for exactly this case.

**Number 8's survival is the same shape milestone 4 found.** §5.4.4 has two ways
for a declaration to fail — the name is not an identifier, and the identifier is
not followed by a colon — and every fixture in the file took the second, because
`not a declaration` starts with the identifier `not`. The branch rejecting a
non-identifier name had never run. `p { 42px; color: red }` and four others
separate them, and the count is asserted alongside the surviving declaration so
a build that discarded *both* fails too.

**Number 41's survival is milestone 3's shape: the rule was enforced twice and
only one half was reachable.** Disabling the CSS-wide-keyword branch entirely
changed no answer for `color: inherit` or `display: initial` — a colour that is
not a colour and a keyword that is not one of a property's own keywords are
*already* `Unsupported` under the (property, value) rule. The half nobody
reached is a **length**-valued property, where an identifier that is not one of
its keywords is `Invalid`: `margin-top: inherit` would have been filed as the
author's typo rather than as a gap in this engine, and `inherit` is in every
real stylesheet. Seven length-valued properties now assert it, and
`margin-top: red` asserts the other direction so the test is about the five
keywords rather than about identifiers in general.

### The fuzz target is the twenty-third, not the twenty-second

Row 6 says twenty-second. Gap 24's milestone 5 split `crypt_ciphers` out of
`crypt` after this plan was written and took that number, so `css` is the
twenty-third; the row is amended in place and `fuzz/README.md` and `ci.yml`'s
per-PR job go twenty-two to twenty-three.

It cannot be *run* on this host — libFuzzer is not available on
`x86_64-pc-windows-msvc` — so its body was lifted verbatim into a scratch crate
and driven over all six committed seeds and five prefixes of each. Every
assertion in it has executed at least once, which is the difference between a
target that type-checks and a target that works. The campaign itself is
milestone 13's.

Two of the six seeds sit behind a control byte of zero. The tightest value took
a correction: it started at zero bytes, which refuses every non-empty body at
the byte cap so the token total is never reached at all — a knob whose tightest
setting makes the *other* cap unreachable. It reads sixty-four bytes and stops
at four tokens now, and the second value is the one that fires the byte cap.

### `deny.toml`'s hole, measured before it was closed

The plan says the file denies sixty-six crates and not one is a CSS crate, an
HTML crate, a layout engine, a line breaker or a Unicode-data crate. Confirmed,
and fourteen names land here — six CSS and eight HTML. The licence gate would
have caught two of the six on its own and waved four through: `cssparser` and
`selectors` are MPL-2.0, which the allowlist already bars, and `lightningcss`,
`simplecss`, `css-color-parser` and `stylo` are not.

The HTML names carry a second argument beyond rule 1, written into the file: an
EPUB content document is XHTML, which is XML, and this engine reads it with
`tinker-pdf-xml` under a doctype mode that refuses the internal subset. An HTML5
tree builder is a different parser with a different error-recovery model and a
different threat surface, and adopting one would quietly undo gap 30's whole
defence against entity expansion.

### Still owed

- **Nothing consumes any of this**, which is milestone 7's and milestone 8's.
  The `Unsupported` census is a number this crate can produce and no book has
  been run through it, because there is no element tree yet.
- **`MAX_DOM_NODES < MAX_XML_TOKENS`** is owed by the facade at milestone 8, for
  the reason above. It is in the ledger's `reachable` column meanwhile.
- **The leaf count is still written as eight in four places.** The ledger sweep
  is milestone 13's and takes it to ten in one edit; taking it to nine here and
  to ten there would be two edits to four files and a window in which three of
  them disagree.
- **The `font` and `list-style` shorthands are `Unsupported` rather than
  expanded**, deliberately: `font` resets six longhands and this build has three
  of them, so a partial expansion would set those three and leave the others at
  whatever they inherited — decision 5's failure one level up from a value.
- **The viewport units are `Unsupported`.** A reflowable book's viewport is the
  page box, and whether a *fragmented* page is a viewport at all is milestone
  7's decision; resolving them against something plausible now is exactly what
  device 2 exists to prevent.
- **`currentColor` is not a keyword**, so `border: solid` takes black rather
  than the computed `color`. It is recorded in `border_shorthand` where the
  simplification is made rather than left to be discovered on a page.
- **The named-colour table is the CSS 2.1 sixteen plus about thirty**, not
  `css-color-4`'s hundred and forty-eight. A name outside it is `Unsupported`
  and counted; the argument is this gap's own — a typo in a hex value produces a
  colour that is slightly wrong and looks entirely plausible, and a table of a
  hundred and forty-eight hand-entered values is a hundred and forty-eight
  chances at exactly that.

## Progress — 20 August 2026, milestone 7

**`tinker-pdf-layout` is the tenth leaf, and it answers UAX #14's own
conformance file 19 338 times out of 19 338.** 2 530 tests, 7 ignored, up from
2 458; seventy-two of them are new — sixty-six the crate's own unit tests, two
the conformance run, three the compile-time proof and one a doctest — and four
existing tests were changed because the injection matrix showed they could not
fail.

The crate takes a tree of plain structs and a `Metrics` trait and returns pages
of positioned fragments. It knows nothing of PDF, of EPUB, of XHTML or of a
font file. What it does know is `tinker-pdf-css`, and that edge is the first of
this milestone's two answers to questions the plan left open.

### The `math` question, answered: no, and the edge is dropped

Gap 31's design section predicts `("tinker-pdf-layout", &["tinker-pdf-math"])`
and then says, in as many words, that *"whether layout needs one at all is an
open question milestone 7 answers: if it does not, the edge is dropped and the
crate joins the empty-list group"*. **It does not.**

The interrogation is written out in `xtask/src/main.rs` as the sixth amendment,
because an answer is worth what the search behind it was. `tinker-pdf-math`
exists for ruling 4: a pixel-path crate may not call a platform transcendental,
since glibc, musl, the MSVC runtime, Apple's libm and the wasm shim each round
`sin`, `exp` and `ln` their own way. Every arithmetic operation in this crate
was listed against that rule — the box model is add, subtract and one
multiply-divide for a percentage; margin collapsing is `max` and `min`;
half-leading is `(line_height - (ascent + descent)) / 2`; justification is
`slack / spaces`; fragmentation compares a running `y` against a page height.
Not one is transcendental and not one is even a `sqrt`, which would have been
fine anyway since IEEE 754 requires that one to be correctly rounded. Taking
the edge would have been the failure this file's own history records, from the
other direction: a dependency in a manifest that nothing needs.

**What it takes instead is `tinker-pdf-css`, which the plan did not predict**,
and the argument is the plan's own ordering argument read to its conclusion.
Milestone 6 comes before milestone 7 because *"a layout engine built first has
to invent its own input representation, and that representation then becomes
what the cascade must produce — which is how a cascade acquires shortcuts"*. A
layout crate with no edge to `css` would have to declare a second style type and
the facade would convert between them, which is precisely that second
representation; and decision 5's compile-time device would stop at the cascade,
because the thing layout matched on would no longer be the parser's own output.

### Decision 5, one milestone further than the crate that invented it

Milestone 6's `Still owed` says *"nothing consumes any of this"*. That is the
hole this closes, and it is the same failure one level up: a property can be
parsed, cascaded, written into `ComputedStyle` — and read by nobody. The book
still renders. It renders slightly differently.

`style::consume` destructures `ComputedStyle` with **no `..`**, so a field added
to that struct without a consumer here is

```text
error[E0027]: pattern does not mention field `hyphens`
   --> ...\src\style.rs:130:9
```

`tests/uncascaded_field_does_not_build.rs` proves it, and it is a real build of
the real files: `tinker-pdf-css` compiles with a bare `rustc` — milestone 6's
finding, and still load-bearing — so the harness compiles the css crate to
metadata, adds `pub hyphens: bool` to `ComputedStyle` and its initial value,
then compiles the **real** `style.rs`, `metrics.rs`, `uax14.rs` and `unicode.rs`
against it with `--extern`. Three of the four builds are controls, for
milestone 6's reasons: the pristine pair must compile or every injection would
report "the build failed" and the file would pass while proving nothing; the
same field *with* a binding must compile or the failure would be true of any
edit; and the error must be `E0027` and must name the field.

The injected property is `hyphens` rather than an invented one, for milestone
6's reason and with a correction it forced. Milestone 6 injected `widows`
because it was *"the exact edit somebody will make when that milestone
arrives"*. That milestone arrived: `widows` is implemented here, so the
injection became a duplicate variant that fails the build for the wrong reason
and would have broken the control too. **That the constant had to move is the
evidence it was the right kind of constant**, and it now names
`border-collapse`, which is milestone 11's.

**Six properties came out of `UNSUPPORTED_PROPERTIES` and into the enum**, each
with a layout consumer: `orphans`, `widows`, `page-break-inside`,
`overflow-wrap`, `line-break` and `word-break`. The committed corpus's
`Unsupported` set falls from twenty names to sixteen and the longhands read rise
from 772 to **788**.

One of the six took a decision rather than a reading. **CSS 2.2 §13.3.1's own
table says `page-break-inside` is inherited, and this build makes it not.**
`css-break-3` §4.1 defines `break-inside` as non-inherited and makes
`page-break-inside` a legacy alias of it; gap 31's plan says it *"treats the
`break-*` longhands as the modern spelling of the same thing"*; and the
alternative is that one `page-break-inside: avoid` written on a figure cascades
to every descendant of wherever it lands, forbidding every page break in the
book — a book that is one enormous page rather than a visible failure. The
argument is written where the `match` arm is, and the injection matrix attacks
it from both sides.

### UAX #14, against Unicode's own file

The line breaker is the specification's rules by number over the vendored UCD,
and it is checked against `LineBreakTest.txt`: **19 338 cases, 19 338 passing**,
driven through `opportunities` — the same entry point a book goes through — at
`Tailoring::UAX14`, which is `line-break: strict` and not a private mode. A
conformance run against a private code path proves that the private code path is
conformant.

That file is why the vendored tree has five files rather than two. The plan
names `LineBreak.txt` and `EastAsianWidth.txt`; the other three are what turned
out to be needed. `DerivedGeneralCategory.txt` supplies `Mn`/`Mc` for LB1's `SA`
resolution, `Cn` for LB30b's unassigned pictographs and `Pi`/`Pf` for LB15a and
LB15b — a build without the last pair treats “ and ” alike and breaks after an
opening quotation mark. `emoji-data.txt` is `Extended_Pictographic`. And
`LineBreakTest.txt` is the oracle, vendored rather than fetched because gap 20's
finding holds a third time: a skipped oracle exits 0 and reads exactly like a
pass.

**It is not a pair table**, and the header of `uax14.rs` says why: eleven of the
rules are not about a pair at all. LB8, LB14, LB15a, LB16 and LB17 look back
across a run of spaces; LB25 looks back across `SY`/`IS` and forward past an
`OP`; LB15b, LB15c and LB19a look forward one unit; LB28a looks two units each
way; and LB30a counts the **parity** of a run of regional indicators. A table
indexed by the two classes either side gets about two thirds of it and fails the
rest quietly.

Two things had to be corrected before the file agreed, and both are worth
keeping.

**`LineBreak.txt`'s header describes what the file has already done, not what a
reader must do.** It lists four ranges whose unassigned code points default to
`ID` or `PR`, and applying them is the careful-looking thing. It is wrong: the
file already carries those defaults as explicit rows — `1F02C..1F02F ; ID # Cn`
is a reserved range written out — so a second pass gives U+1F8FF, unassigned and
inside the header's `1F000..1FAFF`, the class `ID` where the conformance file
says `XX`. Ten cases failed on exactly that and nothing else in this repository
would ever have noticed.

**LB20a's right-hand side is `AL | HL` and not `AL`.** `05BE 05D0` — a Hebrew
maqaf and an alef — is `×` by rule 20.1, and a reading that took the rule text's
`AL` as the Line_Break class alone breaks between them.

The generated table names `Class::AL` rather than the number `2`, and that costs
nothing and buys the thing an index cannot: **a Line_Break class this crate has
never heard of fails to build.** Unicode 16.0 added `HH` and 15.1 added `AK`,
`AP`, `AS`, `VF` and `VI`; a build that mapped an unknown name onto a default
would have laid out Brahmi-family scripts and unambiguous hyphens as though the
additions had never happened.

### The CJK fixture, and what a space-scanner answers

東京都 is three ideographs and three break opportunities. A breaker that splits
at U+0020 finds **one**, at the end of the string, and the test writes both
numbers down rather than asserting only the first — because the sentence gap
31's risk table is about is *"passes every English fixture ever written"*, and
the way to test that claim is to compute what the heuristic would have said.

Beside it: 「東京、京都」 asserts that no line ends after an opening bracket
(LB14), before an ideographic comma (LB13) or before a closing bracket; あぁ
asserts that `strict` and every other value disagree about a small kana, which
is the whole of what `line-break: strict` means (§6.1 resolves `CJ` to `NS` for
that style and to `ID` for the others); 日々 asserts that `loose` adds the
iteration mark and `normal` does not; and `word-break: keep-all` holds 東京都
together while leaving the space in `ab cd` alone, because an explicit
opportunity is not an implicit one.

**§5.5's four required classes are asserted under every combination of
`line-break` and `word-break`**, four classes at a time — `WJ`, `ZW`, `GL` and
`ZWJ` — because a build that honoured three of them passes any test that looks
at one. `line-break: anywhere` is tested separately, since it is the only value
css-text-3 permits to disregard them, and the difference between it and
`word-break: break-all` is exactly that: `break-all` opens a Latin word and
still may not open an emoji ZWJ sequence.

That distinction did not hold when it was first written, and the injection
matrix is not what found it — the test was. The tailorings were applied on top
of whatever the rules decided, so `break-all` opened every boundary before a
letter including the one after a word joiner. `decide` now returns a `Verdict`
carrying whether §5.5 makes the answer untouchable, covering LB4 to LB12a, and
a tailoring may not change one.

### Margin collapsing: three cases, one accumulator, three tests

Row 7 calls it *"the rule a first implementation omits and whose omission moves
every block on every page"*, and there is a sharper version: it is the rule
whose **partial** implementation is most plausible. §8.3.1's three cases are
adjacent siblings, a parent and its first child, and a box whose own margins
collapse *through* it.

All three fall out of one object. `Pending` holds the margins adjoining at the
current position and is not committed until something that is not a margin
arrives — a border, a padding, a line box. A parent with none of those between
itself and its first child never commits, so the two meet in the accumulator; a
box with nothing at all inside it never commits either, so its own two meet
there. Writing them as three special cases is how an implementation ends up
with two of them.

Each is asserted on its own, and so are the two clauses that decide *when*
rather than *that*: a border between a parent and its first child stops case 2,
and a negative margin is **added** to the largest positive one rather than
losing a `max` to it — §8.3.1 says *"the maximum of the positive adjoining
margins, plus the minimum of the negative ones"*, and a build that took `max()`
over signed values gets every ordinary book right and every drop cap wrong.

### §13.3.3's rules A to D, and the escape

Where a break is *permitted* is a different question from where one is
*preferred*, and an implementation written from the property list alone answers
only the second. §13.3.3 gives two kinds of position and four rules over them,
and all four are implemented and attacked separately.

**Rule B is about a *common* ancestor and not about either side.** That is a
real difference and not a nicety: the margin between an ordinary paragraph and
the first child of a `page-break-inside: avoid` figure has no common ancestor
that avoids anything, so a break there is legal — and a build that recorded
"either side is inside something that avoids" would refuse the one margin that
is the natural place for a break and push the whole figure to the next page for
no reason anybody could see. `Pending` therefore carries the **intersection** of
the open avoiding ancestors, narrowed once per contributing box, and the test
asserts both directions.

**`orphans` and `widows` are two constraints that interact**, so one fixture
answers three ways: the same three-line paragraph breaks after two lines at
(1, 1), after one line at (1, 2), and cannot be broken at all at (2, 2) — where
§13.3.3's own escape has to drop rule C and says so through a warning.

**The escape is the part an implementation omits**, and it is three tiers rather
than two: rules B and D are dropped first, then A and C. Without it, a book with
`page-break-inside: avoid` on `body` — a thing real stylesheets do — is one page
as tall as the book and every page after the first is blank.

### What the design got wrong, and how I found out

**LB3 makes the end of the text a mandatory break, and the line filler asked
"is this mandatory?" before "does this fit?".** So the first candidate it
examined was the end of the paragraph, and every paragraph was one line. Eight
tests failed at once and every one of them was about something else — alignment,
backgrounds, orphans — which is what a defect in the innermost loop looks like.
The order of those two tests is now the whole of that function and its doc
comment says so. It is also the reason a fixture shorter than a line proves
nothing about line breaking.

**`MAX_LAYOUT_WORK` cannot fire in this build, and the plan's row is amended in
place.** It was written, and its firing test was attempted, and there was no
input that reached it: with no float re-flow, no two-pass table layout and no
shrink-to-fit, every unit of layout work is one box or one line box — and boxes
are bounded by `MAX_BOX_TREE_NODES`, line boxes by `MAX_LINE_BREAK_WORK`,
because a line box needs a character and every character is charged before the
breaker is entered. A cap there would sit above what its own inputs can ask for,
which is gap 18a milestone 8's failure exactly, or below the box cap, where it
would be the box cap under another name. **The bound arrives with the multi-pass
layout of milestones 10 and 11 or not at all.**

Earning that absence cost three fixes rather than none, and they are the
interesting half. *Depth is not work once the recursion branches* has a
loop-shaped twin, and three places were quadratic: the line filler restarted its
scan of the break opportunities at zero for every line, which for a page one
point wide is `O(characters²)`; `piece_at` scanned the span list once per
boundary, so a paragraph of a thousand `<em>`s cost `O(pieces × characters)`;
and the list-item ordinal counted from the first child for every item. **A work
cap would have charged for all three instead of removing them**, which is the
argument for looking before adding one.

**`MAX_EPUB_PAGES` has arrived, as `MAX_LAYOUT_PAGES`.** Milestone 4 deferred it
with the sentence *"it arrives with milestone 7's fragmentation and not
before"*; it is declared in the crate that fragments rather than in `epub.rs`,
for `MAX_DOM_NODES`'s reason one milestone earlier.

**`MAX_BOX_DEPTH` exists where the plan says a depth cap would not.** That
plan's *"four deliberately absent"* list argues that `MAX_XML_DEPTH` stands in
front of every content document, so a second constant could never fire. That is
right about the facade and wrong about this crate: **its input is a caller-built
tree rather than a parsed document**, and the twenty-fourth fuzz target builds
one from a structured generator with no parser anywhere in front of it. It is
the only row in `bounds_ledger.rs` whose reachable ceiling is unbounded.

**`tab-size` is not a constant here, and that is this crate's own rule applied
to itself.** A `TAB_SIZE` with no consumer would read as though preserved tabs
advanced to a tab stop. They do not; under `pre` a tab is measured as one
character of the element's font, and that is recorded in `Still owed` rather
than approximated by a number nobody reads.

### The injection matrix

**Fifty-seven defects. Forty-six caught on the first pass, seven real gaps
closed, two injections repaired, and the whole matrix re-run against the tree as
committed: fifty-five of fifty-seven, with two survivors that are equivalent
mutants and are argued below.**

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | `box-sizing: border-box` is `content-box` | `box_sizing_is_the_difference_between_a_hundred_and_a_hundred_and_thirty` |
| 2 | An `auto` width forgets its own margins | `an_auto_width_block_fills_what_is_left_of_its_containing_block` |
| 3 | Two `auto` margins do not centre | `two_auto_margins_centre_a_block` |
| 4 | A percentage margin is of the height | `a_percentage_margin_is_of_the_width_even_at_the_top` |
| 5 | `border-width` applies with `border-style: none` | `a_border_width_with_no_border_style_moves_nothing` |
| 6 | Adjoining margins **add** rather than collapsing | all three collapsing tests at once |
| 7 | A parent commits its margin before its first child | `margins_collapse_between_a_parent_and_its_first_child`, and two more |
| 8 | An empty box does not collapse through itself | `an_empty_boxs_margins_collapse_through_it` |
| 9 | A border does not stop the parent collapse | `a_border_stops_a_parent_collapsing_with_its_first_child` |
| 10 | A collapsed margin is `max()` over the signed values | `a_negative_margin_is_added_rather_than_beaten` |
| 11 | No anonymous block for mixed children (§9.2.1.1) | `a_background_covers_the_lines_it_holds`, and two more |
| 12 | A line box has no strut (§10.8.1) | `a_line_height_number_and_a_length_are_different_things`, and two more |
| 13 | Half-leading is full leading | `a_background_covers_the_lines_it_holds`, and two more |
| 14 | `visibility: hidden` is not painted **and** not laid out | `hidden_is_laid_out_and_none_is_not` |
| 15 | `display: none` is laid out | `display_none_removes_exactly_its_own_text` |
| 16 | Phase I does not collapse a run of spaces | `phase_one_collapses_an_indented_source`, and two more |
| 17 | Phase I is per element rather than per formatting context | **survived** — closed, see below |
| 18 | A segment break is removed rather than becoming a space | `phase_one_collapses_an_indented_source` |
| 19 | `pre` collapses like `normal` | `pre_and_pre_line_differ_about_the_spaces_and_agree_about_the_breaks` |
| 20 | Phase II does not trim a line's ends | `phase_two_trims_the_space_a_line_broke_at`, and two more |
| 21 | `nowrap` wraps | `nowrap_collapses_and_does_not_wrap` |
| 22 | The end of the text is tested before the fit | **survived** — equivalent, see below |
| 23 | LB1 does not resolve `CJ` by the tailoring | `strict_and_normal_disagree_about_a_small_kana` |
| 24 | LB9 does not attach a combining mark | the conformance file, and two more |
| 25 | LB13 lets a line end before closing punctuation | the conformance file, `japanese_punctuation_does_not_start_a_line` |
| 26 | LB14 does not look across a run of spaces | the conformance file |
| 27 | LB20a glues a hyphen anywhere | the conformance file |
| 28 | LB30 ignores East Asian width | the conformance file, `east_asian_width_decides_whether_a_bracket_glues` |
| 29 | LB30a is a pair rather than a parity | the conformance file |
| 30 | A tailoring may override a §5.5 required class | `the_four_required_classes_hold_under_every_tailoring`, `break_all_breaks_inside_a_latin_word` |
| 31 | `LineBreak.txt`'s block defaults are applied twice | the conformance file |
| 32 | `word-break: keep-all` does not hold a run together | `keep_all_holds_a_cjk_run_together` |
| 33 | `line-break: loose` is `normal` | `loose_breaks_before_an_iteration_mark_and_normal_does_not` |
| 34 | `overflow-wrap` never breaks a word | `overflow_wrap_decides_what_happens_to_a_word_longer_than_the_line` |
| 35 | Justification stretches the last line too | **survived** — closed, see below |
| 36 | `text-indent` applies to every line | **survived** — closed, see below |
| 37 | `text-align: center` is `right` | `the_alignments_put_a_line_where_they_say` |
| 38 | Rule A is not checked | `rule_a_moves_the_break_up_a_block` |
| 39 | Rule A lets an `avoid` beat a forced break | **survived** — equivalent, see below |
| 40 | Rule B is either side rather than a common ancestor | `rule_b_refuses_a_margin_inside_an_avoiding_ancestor` |
| 41 | Rule C checks `orphans` and not `widows` | `orphans_and_widows_are_two_constraints_over_one_paragraph`, and one more |
| 42 | Rule C checks `widows` and not `orphans` | `orphans_and_widows_are_two_constraints_over_one_paragraph` |
| 43 | Rule D is not checked | `rule_d_refuses_a_break_between_the_lines_of_an_avoiding_block` |
| 44 | The rules are never dropped | **survived** — closed, see below |
| 45 | A forced break is ignored | `a_forced_break_starts_a_page_with_room_to_spare`, and two more |
| 46 | The margin a page breaks in survives the break | **survived** — closed, see below |
| 47 | A line taller than the page is dropped | `a_line_taller_than_the_page_is_kept_and_reported` |
| 48 | The alphabetic counter is ordinary base 26 | `the_marker_counters_are_computed_rather_than_tabled` |
| 49 | A generated marker joins the conserved stream | `a_marker_is_on_the_page_and_out_of_the_conserved_stream` |
| 50 | The depth cap is never checked in the block walk | **survived** — closed, see below |
| 51 | The box total is never charged | `a_tree_past_the_box_cap_is_refused_by_name` |
| 52 | The break total is refunded per context | **survived** — closed, see below |
| 53 | The page cap is off by one | `a_book_past_the_page_cap_is_refused_by_name` |
| 54 | A page with no area is paginated | `a_page_with_no_room_is_refused_by_name` |
| 55 | `ComputedStyle` is read with a `..` | **the build** — all three compile-time proofs |
| 56 | `orphans` and `widows` do not inherit | **the build** |
| 57 | `page-break-inside` inherits | **the build** |

Seven of the eleven first-pass survivors were real gaps and every one has the
same shape: **a fixture that gives the right answer for the wrong reason.**

- **17** — `<em>a </em><em> b</em>` cannot tell a shared collapser from a
  per-element one, because the first run *ends* with a space and a fresh
  collapser deletes the second run's leading space as a *leading* space. The
  fixture that separates them is `<em>a</em><em> b</em>`, where the shared one
  keeps the space and the per-element one welds two words into `ab`.
- **35** — the last line of `aa bb cc dd` is one word, so there is no gap to
  stretch and a build that justified it answers identically. The fixture now
  ends `dd ee`.
- **36** — `text-indent` moves the first line's `x` *and* narrows its measure,
  and the fixture only checked the `x`. `aa bb cc` at six characters a line sets
  two lines correctly and three lines if the indent is subtracted from every
  line.
- **44** — with rules B and D standing, `choose` finds nothing and falls to its
  last resort, which cuts at the overflowing item — usually the same place the
  third tier would cut. The middle tier answers differently exactly when the
  highest permitted position is **below** the overflow, which `widows: 2` on a
  four-line paragraph produces.
- **46** — every fragmentation fixture had zero-height margins, so a build that
  carried the margin over to the next page put nothing there.
- **50** — **the best of them, and this session's named failure mode.** The
  depth cap is enforced twice, in the block walk and in the inline gather, and
  the fixture was a chain of blocks ending in *text* — which the gather catches
  one level deeper. Deleting the block walk's check changed no answer. There are
  two tests now: a chain of blocks with no text anywhere, and a chain of inlines.
- **52** — the firing test is one paragraph over the cap, so a build that
  *assigned* the count instead of adding it answered identically. Three
  paragraphs under the cap and over it together separate them, which is
  `tinker-pdf-css`'s `the_token_total_is_spent_across_sheets_and_not_per_sheet`
  one crate up.

Two of the eleven were **bad injections** and are recorded as such rather than
quietly fixed: number 6 bound a value and discarded it, and number 27 added a
tautology to a condition that still stood. Both were rewritten into defects that
change an answer, and both are caught.

Two survive the re-run and both are equivalent mutants:

- **22** — moving the mandatory test to *after* the fit test makes it
  unreachable for the end of the text, because the end of the text is only
  examined when everything before it fits, and returning it then is what the
  loop does anyway. The `justify` flag is unaffected: it already tests
  `end < content.len()`. Reachable only if a *hard* break could also be the end
  of the text, which it cannot.
- **39** — `allowed_by_a` loses its `forced ||`, which matters only where a
  forced margin is a candidate in `choose`. It never is: `paginate` scans for
  the first forced margin *before* it scans for an overflow, so a forced margin
  at or before the overflow has already been taken. The clause is right as
  written and is unreachable in this build; it becomes reachable the day a
  forced break can be refused, which `page-break-before: left` will do when
  spreads land.

### Text conservation still holds, and this is the first build that could break it

Milestone 4 built the harness against thirteen grey placeholders *"so that this
milestone inherits it rather than acquires it"*. All thirteen of its tests pass
unchanged, including `every_committed_book_conserves_the_figure_the_record_states` over the six committed books — the facade still paginates placeholders,
so what that proves is that nothing in this milestone moved the harness.

What is new is the same invariant one level down, where it can now fail.
`Layout::text` returns every laid-out character in reading order,
`BoxNode::source_text` returns the tree's own, and the crate asserts they agree
across a forty-chapter flow at 120 × 60 points — nine pages of real
fragmentation — and that `display: none` removes **exactly** its own subtree.
The twenty-fourth fuzz target asserts it on every input, over trees nobody
wrote, and it is the reason the target computes its own expected text rather
than calling `source_text`: `display: none` is the one legitimate way to lose
some, and modelling it in six lines beside the assertion is what keeps the
assertion an equality rather than a containment.

A marker is generated content and carries `generated: true` so that it reaches
the page and stays out of the conserved stream. A build with no such flag either
loses the invariant or loses the markers.

### The fuzz target is the twenty-fourth, not the twenty-third

Row 7 says twenty-third; milestone 6's own amendment took that number for `css`
after gap 24 split `crypt_ciphers` out of `crypt`. The row is amended in place
and `fuzz/README.md` and `ci.yml`'s per-PR job go twenty-three to twenty-four.

It is **the only target here whose input is not bytes**, which is one of the two
crates' reasons for being two. The bytes drive a structured generator: a tree of
boxes, their styles, and their text out of a twenty-four character alphabet
chosen for the line breaker's own rules — an ideograph, a small kana, a
no-break space, a word joiner, a zero-width space, a joiner, an unambiguous
hyphen, and both a narrow and a full-width bracket. A target that handed these
bytes to a parser would spend its session being refused at the door.

libFuzzer is not available on `x86_64-pc-windows-msvc`, so the body was lifted
verbatim into a scratch crate and driven over all six committed seeds, every
prefix of each, all 256 control bytes against each, and **200 000
pseudo-random inputs**: 201 578 executions, no assertion failed. That is not a
campaign — milestone 13's is — but every assertion in the target has executed.

### `deny.toml`, and the licence that already passed

Thirteen names land: four layout engines and nine line-breaking and
Unicode-data crates, taking the file from eighty denied crates to ninety-three.
None of the thirteen would have tripped the licence gate — `taffy` is MIT — which
is the hole rule 1 exists to close, since a licence list has nothing to say
about what a crate *does*.

The **data** is a different question with a different answer, and the file says
so: the UCD is vendored under THIRDPARTY.md and compiled by a build script, and
a table of published facts is not an implementation. Taking a crate that carries
the table *and* implements the algorithm would take the second along with the
first. `Unicode-3.0` was already in the allowlist — checked rather than assumed
before anything was fetched, and it is the single fact that made UAX #14
buildable here rather than blocked.

### Still owed, and what was narrowed

- **Nothing consumes any of this yet**, which is milestone 8's. The element
  tree, the UA stylesheet and the synthesis of a laid-out page into a
  `CosDocument` are that milestone, and until then a book is still thirteen grey
  placeholders. The `Metrics` a real book needs is milestone 9's.
- **`display: inline-block` is laid out as a block-level box and warns by
  name**, and this is a narrowing said out loud rather than a criterion quietly
  met. Row 7 asks for §9.4.1 and §9.4.2 — block and inline formatting contexts
  and line boxes — and an atomic inline is neither; doing it properly needs
  shrink-to-fit, which needs min-content and max-content widths, which is the
  machinery milestone 11's automatic table layout brings. The warning is the
  honest half: the box is in the wrong place and something says so.
- **No replaced content.** An `<img>` has no box here, so a book's figures are
  milestone 8's or 9's. Row 7 does not name it and it is recorded rather than
  discovered.
- **`line-break: loose`'s fourth group is not implemented.** css-text-3 §5.1
  names four sets of characters `loose` makes breakable; the hyphens and the
  iteration marks are here, `CJ` resolving to `ID` covers the small kana, and
  breaks before centred punctuation are **not** there. A list that is nearly
  right is what device 2 exists to prevent, so the omission is a named constant
  with the missing group written above it.
- **`overflow-wrap: break-word` and `anywhere` behave alike here.** They differ
  only in whether the opportunity counts toward a box's min-content size, and
  this build computes no min-content sizes. Recorded where the enum is declared
  rather than collapsed into one variant.
- **A preserved tab is one character wide**, not a tab stop; see above.
- **`font-variant: small-caps` and `text-decoration` reach the page and are not
  measured.** Both are carried on the run for the painter; a small-caps run's
  advance is the face's own, which is a question for milestone 9's metrics.
- **The browser oracle is milestone 8's**, and it is the only device that can
  catch a property implemented, honoured and *wrong* — which is the residue gap
  31's honesty machinery names as its own limit.

## Progress — 21 August 2026, milestone 8

**Every one of the six committed books reads, and every character of every one
of them is on exactly one page.** 2 578 tests, 7 ignored, up from 2 530;
forty-eight of them are new and ten existing ones were changed, because the
first real book to reach a page made several of them assertions about something
else.

The reading path is `xhtml.rs` → `read.rs` → `tinker-pdf-css` →
`tinker-pdf-layout` → `paint.rs` → `DocumentBuilder`, and none of those arrows
is new work in a leaf: milestones 2, 6 and 7 built the three engines and this
milestone is the join. What it had to decide is what neither leaf could —
where the user-agent stylesheet lives, what a text node's style is, which face
a run is drawn in, and what happens to a character `WinAnsiEncoding` cannot
spell.

### The user-agent stylesheet is a file, and it is CSS

`src/epub/ua.css` is 250 lines of HTML §15's own rules, `include_str!`d and
parsed by milestone 6's parser at `Origin::UserAgent`. It is not a table of
Rust constants and not a `match` on element names, and the reason is not
tidiness: **a UA sheet written in Rust is a second style system**, with its own
specificity, its own cascade order and no way for an author to beat it. Written
as CSS it loses to a book's own rules by `css-cascade-5` §6.1's ordinary
machinery, which is what a reading system is required to do — and calibre's
`.calibre2 { display: block; margin: 1em 0 }` beating `p { margin-top: 1em }`
on specificity is that rule working on a real book.

**One rule in it is deliberately outside the implemented set.** HTML sets
`table { display: table }` and this build has no table box until milestone 11.
The sheet says it anyway, and `tinker-pdf-css` charges it to the census **per
element it reached** — so `calibre-book-cover.epub` reports `display`,
unimplemented, 38 elements, and `pandoc-plates.epub`, which has no table,
reports no `display` at all. A sheet that stayed silent would set every table
in the corpus as inline text with nothing anywhere saying so, which is the
failure this whole plan is organised around. The nine `display` values for the
table box tree are the only such declarations, and `ua.css`'s header lists what
is *not* there and why.

### What the design got wrong, and how I found out

**1. The box tree was rooted at `<body>`, which made `head { display: none }` a
rule that changed nothing.** Text conservation passed, the head's `<title>` was
not on any page, and every assertion about the user-agent sheet was green — for
the wrong reason. The subtree was gone by *position*, not by `display`, so
deleting the rule from `ua.css` would have changed no output at all.

What found it was writing the sheet's own test in the direction the plan asks
for: *removing it produces an undifferentiated book*. The third of the three
consequences that test asserts is that the `<title>` and the `<style>` element's
own CSS **do** reach the page once the sheet is gone — and that is an assertion
that cannot fail if the subtree was never in the tree. The tree is now rooted at
the document element, `head { display: none }` is load-bearing, and the
injection matrix's first row is the proof: deleting it takes text conservation
down on all six books.

This is exactly the failure mode milestone 7 recorded seven of — *a fixture
giving the right answer for the wrong reason* — and it is the reason the plan
asks for the sheet's absence to be **visible** rather than merely worse.

**2. `OpenOptions::font_size` did not reach the cascade, and the first way I
wired it up was silently overridden by two thirds of the corpus.** The obvious
route is a generated `html { font-size: 12pt }` rule at the user-agent origin.
pandoc writes `html, body, div, span, … { font-size: 100% }` as a CSS reset on
every book it produces, which is an **author** declaration and beats any
user-agent rule — so four of the six books ignored the host's number entirely
and paginated identically at 8 point and at 18.

The mistake is a real one about CSS rather than about wiring: a percentage
font-size on the root element resolves against the **initial** value, and
`css-fonts-4`'s initial value is `medium`, which is *the user's preferred
size*. `tinker-pdf-css` had it as a constant. So `cascade::cascade_from` takes
the initial `ComputedStyle` as a parameter and `cascade` is that function at
`ComputedStyle::initial()`; the facade passes the caller's size in it, and
`the_callers_base_font_size_changes_the_pagination` is what says it arrives.
pandoc's reset now means what it says — *whatever the reader chose* — which is
also what a browser does with it.

**3. The text-conservation harness had been decoding the source as Latin-1
since milestone 4, and nothing could see it.** `visible_text` pushed one `char`
per **byte** of an already-decoded `&str`, so every em dash became three
characters and every kanji three more. At milestone 4 every page was empty: the
sweep compared a mangled source against no pages at all and reported `0`
conserved either way. The first book that laid out reported 2 454 characters of
spurious divergence on `calibre-book-cover.epub` alone.

`the_source_side_decodes_and_does_not_transliterate` is the assertion milestone
4 owed and did not write, and it is written now rather than only fixed.

**4. The conserved stream's definition was short by one of HTML's four
removals.** The harness dropped `<head>`, `<script>` and `<style>` — the three
that generate no box — and said in as many words that they go because *"no
reading system sets them into the flow"*. HTML §15.3.1 names a fourth:
`[hidden]`. Three of the six committed books carry a `<nav … hidden="hidden">`
landmarks list, and honouring the rule takes twenty-four to twenty-nine
characters out of each of them.

The choice was between an engine that sets text every browser hides — including
the browser this milestone compares against — and a definition of the conserved
stream that names three of four exclusions. **The definition was what was
wrong.** The harness's scanner still asks the engine nothing: it reads the
attribute out of the bytes with the same crude parser it reads `<head>` with,
and `the_source_side_drops_hidden_and_keeps_everything_else` asserts it in both
directions, with `aria-hidden`, `data-hidden` and `class="hidden"` as the three
near misses a substring scan would eat.

**5. A list marker is drawn and must not be extracted, and there was no PDF-level
way to say so.** `tinker-pdf-layout` already flags generated content —
milestone 7 wrote `TextRun::generated` precisely so that *"text conservation
stays an equality rather than becoming a containment"* — but that flag stops at
the crate boundary, and a bullet drawn onto a page is a character
`Page::text()` reports. Nine characters of every book with a list in it.

14.8.2.2 has the answer and this repository's reader did not implement it: an
**artifact** is *"a graphics object that is not part of the author's original
content"*, and 14.8.2 excludes it from the logical content a consumer reads. So
`Device::begin_marked_content` now carries the scope's **tag**, `TextDevice`
keeps a stack of open `/Artifact` scopes and drops the glyphs inside them, and
`paint.rs` wraps a generated run in `/Artifact BMC … EMC`. The renderer acts on
`/OC` and the text device on `/Artifact`; that asymmetry is the reason the tag
had to be passed at all, and it is why an artifact is drawn and not read while
an invisible layer is read and not drawn.

### The Japanese line, and the encoding it does not fit in

Milestone 1 put a line of Japanese in five of the six books to catch a
space-only line breaker. It catches a `WinAnsiEncoding`-only writer too: 25
kanji and kana are not in Windows code page 1252, and a build that wrote them
as UTF-8 bytes into a simple font's string would put mojibake on the page and
lose them from `Page::text()` — text conservation failing on the one sentence
the corpus exists for.

`DocumentBuilder::add_named_font` writes a standard-14 face under an
`/Encoding` whose `/Differences` names each glyph by the Adobe Glyph List's
algorithmic `uniXXXX` form, and `PageBuilder::encoded_text` writes bytes the
caller chose rather than a `&str`'s UTF-8. 9.10.2's second step resolves a code
through `/Differences` to a name and the name to a character, so the **text**
extracts correctly while the standard face has no **glyph** and the page shows
a notdef. That asymmetry is stated rather than hidden:
`ArchiveWarning::UnrepresentedCharacters` counts what got no code at all.

**One overflow font per face, 224 codes, and no more.** That is
`/Differences`'s own size rather than a cap invented here — the array is
allocated at that size whatever the input says — so it is not a bound in ruling
1's sense and does not join `bounds_ledger.rs`. A book with more than 224
distinct out-of-encoding characters for one face loses the excess and says so;
milestone 9's `@font-face` is where that stops being true, and it is recorded as
owed below.

### Text conservation, per book

**Six of six, at 100%.** The figures are in `tests/epub/CONSERVATION.tsv`, which
`every_committed_book_conserves_the_figure_the_record_states` compares against
on every run, so a milestone that moves one has to re-measure rather than argue.

| Book | Spine | Characters | Conserved | Pages |
| --- | --- | --- | --- | --- |
| `calibre-book-cover.epub` | 5 | 2 958 | **2 958** | 5 |
| `calibre-book-nocover.epub` | 4 | 2 958 | **2 958** | 4 |
| `pandoc-book-cover.epub` | 5 | 3 156 | **3 156** | 7 |
| `pandoc-book-epub2.epub` | 5 | 3 156 | **3 156** | 7 |
| `pandoc-book-nocover.epub` | 4 | 3 156 | **3 156** | 6 |
| `pandoc-plates.epub` | 5 | 749 | **749** | 5 |

The source figures moved because of finding 3 above — the Latin-1 bug inflated
every one of them — so the milestone-4 numbers in the same file are not
comparable to these and the file records the current measurement rather than
both.

**The page count is no longer the spine's length**, which is milestone 4's own
sentence read forwards: `pandoc-book-cover.epub`'s five itemrefs need seven
pages at 432 × 648 and eight at 300 × 500, and the *spine* — the sequence of
distinct consecutive page origins — is five at both. Three tests were rewritten
for that and each kept both halves rather than relaxing to the weaker one.

### The `Unsupported` census, per book

**This is the number the milestone is judged on**, and it reaches a caller
rather than a test: `ArchiveWarning::UnimplementedProperty { property,
elements }`, counted by the **elements it reached** rather than by the
declarations that asked. A `float: left` in a rule that matches nothing is not
a gap the book noticed; `.calibre13 { display: table-cell }` matching eighteen
cells is eighteen and not one.

| Book | Properties | Elements | The three largest |
| --- | --- | --- | --- |
| `calibre-book-cover.epub` | 5 | 67 | `display` 38, `vertical-align` 18, `text-align` 9 |
| `calibre-book-nocover.epub` | 5 | 67 | `display` 38, `vertical-align` 18, `text-align` 9 |
| `pandoc-book-cover.epub` | 11 | 150 | `vertical-align` 101, `display` 19, `hyphens` 6 |
| `pandoc-book-epub2.epub` | 11 | 141 | `vertical-align` 92, `display` 19, `hyphens` 6 |
| `pandoc-book-nocover.epub` | 11 | 142 | `vertical-align` 96, `display` 19, `hyphens` 6 |
| `pandoc-plates.epub` | 5 | 67 | `vertical-align` 49, `background-color` 5, `color` 5 |

The full table is `tests/epub/CENSUS.tsv` and
`the_unsupported_census_is_the_one_the_record_states` is the ratchet.

Two of these are worth reading rather than counting. **`vertical-align` is the
largest number in every pandoc book and it is one declaration**: their CSS reset
sets it on ninety-odd element names at once, and almost none of those elements
would move if it were implemented — which is the honest limit of counting by
element and is why the property list is printed beside the total.
**`background-color` and `color` are unsupported on five elements each in every
pandoc book, and the value is `light-dark()`** — a `css-color-5` function this
build does not have. The declaration before it in the same rule is a plain hex
colour that *is* implemented and wins nothing, because §6.1 gives the later
declaration the win and this build then refuses it. That is
`css-cascade-5` working correctly and producing a worse page than ignoring the
rule would have, and it is recorded rather than special-cased.

### The browser oracle, and ruling 9's fifth entry

Ruling 9 is amended in `docs/plans/99-consistency.md`, in writing, with the
argument rather than the conclusion: MuPDF reads EPUB and is already one of the
four, but **its EPUB layout is itself a partial CSS implementation**, so
disagreeing with it names no culprit. The amendment changes nothing about the
ruling's substance — the browser is a subprocess, nothing links it, its output
is transient — and it carries two constraints the plan worked out in advance:
it is never a pixel comparison, and **the job is red when the browser is
missing**.

**What is installed here is Google Chrome 151.0.7922.169** at
`C:\Program Files\Google\Chrome\Application\chrome.exe`, and Microsoft Edge
beside it. The test finds a browser by `TINKER_BROWSER`, then by the four
Windows paths and four Linux paths a Chromium-family build installs to; CI
installs `chromium-browser` and passes `TINKER_BROWSER` explicitly. Every test
prints `browser-oracle: RAN` with the path it used or `browser-oracle: SKIPPED`
with the reason, and `.github/workflows/ci.yml`'s `browser-oracle` job greps
for the second and fails with an `::error::`.

**The continuous comparison agrees to 0.036 of the column, and the injected
defect measures 0.105.** Twenty block boxes of `ch001.xhtml`, in document order,
each side reporting the first line of every block that has text of its own.

Three things about it are worth writing down because each was got wrong first.

- **The block *sequence* is compared exactly, with no tolerance at all.** A
  `display` value not honoured, an element the cascade lost, a `<head>` set into
  the flow: every one changes that list and none changes it by a small amount.
  The `y` offsets are the second assertion, not the only one.
- **Normalising each side by its own column cannot see the defect it exists
  for.** That was the first metric, and it is scale-invariant: dropping every
  paragraph's margin shortens the column in the same proportion and moves the
  normalised positions by 0.033 against an honest disagreement of 0.030. Both
  sides are now divided by **one** denominator — the reference implementation's
  — and the same defect measures 0.105 against an honest 0.036.
- **One variable is held fixed on both sides, and it is named.** The two builds
  set the same text in different faces, so a paragraph takes one more or one
  fewer line on one side and a line is worth about as much as a margin. Both
  sides are told `* { font-family: "Courier New", monospace !important }`,
  whose 600/1000 advances are the Courier this build measures with. It is a
  rule appended to both stylesheets rather than a fudge factor on one, and the
  control test is what says the result can still fail.

The remaining 0.036 is itemised rather than tolerated: a constant 13 points
because the browser reports a line box's **top** and this engine a **baseline**
(0.007), one `<table>` this build sets as inline text because milestone 11 has
not landed (0.019), and one paragraph that broke a line differently because
`<code>`'s `font-size: 85%` is a size this build has one face for (0.010).

**The paginated comparison found something the continuous one structurally
cannot, and it was Chromium doing it.** Asked for `@page { size: 432pt 648pt }`
Chromium writes a 432 × 648 page — and lays the document out at its **own**
default box and scales the result to fit. Reading the printed PDF back through
this repository's own reader showed the body text set at **8.69 points** rather
than 12, which is 576/792: this page's height over US Letter's. A page count
compared across that scale is a comparison of two different documents. So the
comparison is made at the box the browser is not scaling — its own — and the
assertion that the body is set at 12 points is what keeps that honest.

At 612 × 792 with a 36-point margin, **the browser makes two pages of the
chapter and so does this engine**, carrying 1 504 / 1 225 characters against
1 633 / 1 022. Both sides' pages are asserted to be a contiguous ordered
partition of their own text: each of four distinctive sentences appears exactly
once across the pages of each side, which is the fragmentation defect — a
paragraph repeated across a break, or lost at one — that a page count cannot
see.

### What qpdf said

qpdf 12.3.2, through the job gap 20 built. `--check` reports *"No syntax or
stream encoding errors found"* on the synthesised book and on the same document
after a `Document::cos()` round trip through the writer, with the new
`/Differences` font dictionaries, the `/Annots` link arrays and the `/Outlines`
tree in it.

`qpdf_decodes_a_placeholder_page_and_a_page_that_reads` is the one that
changed, and the change is the point. Until this milestone every page of every
book was grey, so the test asserted grey on all three pages of its fixture and
would have passed on a build that greyed everything. The fixture is now two
kinds of page: `--filtered-stream-data` shows
`0.7490196078431373 g 0 0 432 648 re f` and no `Tj` on the spine item that does
not resolve, and `BT /Bk0 12 Tf 0 Tc 0 Tw 42 590.004 Td (One.) Tj ET` with no
`g` on the two chapters either side of it. Reading the operators is the only
thing that can tell those apart.

### The injection matrix

**Forty-two defects. Ten survived a pass: eight were real gaps and are closed,
and two were injections into unreachable code, which is deleted and the
injections re-pointed at the half that can fire. The matrix as committed is
forty-two of forty-two.**

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | `head { display: none }` deleted from `ua.css` | `every_committed_book_conserves_the_figure_the_record_states`, and two more |
| 2 | `[hidden] { display: none }` deleted | `every_committed_book_conserves_the_figure_the_record_states`, `conservation_does_not_depend_on_the_page_box` |
| 3 | No element is block-level | `the_browser_and_this_engine_lay_the_same_column_out_the_same_way`, and the conservation sweep |
| 4 | `p { margin: 1em 0 }` deleted | **survived** — closed, see below |
| 5 | `h1` is body size and body weight | `without_the_ua_stylesheet_a_book_has_no_block_structure_at_all` |
| 6 | An `<li>` is `block` rather than `list-item` | `the_committed_sheet_is_what_a_book_is_set_with`, and three more |
| 7 | The table rules say nothing | `the_unsupported_census_is_the_one_the_record_states` |
| 8 | An anchor is not underlined | `a_list_marker_is_drawn_and_is_not_extracted` |
| 9 | A text node counts as an element sibling | `siblings_skip_the_whitespace_between_them` |
| 10 | Every namespace is XHTML | `a_foreign_element_is_kept_and_is_not_html` |
| 11 | A `<![CDATA[…]]>` section is dropped | `a_cdata_section_is_text_and_not_markup` |
| 12 | A reader refusal is not recorded as `Truncated` | `an_unterminated_element_is_the_readers_refusal_and_not_a_second_check` — **replaced injection**, see below |
| 13 | `class` is one token rather than a list | `the_class_attribute_is_a_token_list` |
| 14 | `Dom::contains` is equality rather than an ancestor walk | `containment_is_the_ancestor_chain` |
| 15 | A text node takes its parent's whole computed style | `a_paragraph_does_not_pay_its_own_margin_twice` |
| 16 | `text-decoration` does not reach the text it marks | `a_paragraph_does_not_pay_its_own_margin_twice` |
| 17 | The box tree is rooted at `<body>` | `without_the_ua_stylesheet_a_book_has_no_block_structure_at_all` |
| 18 | `rel="alternate stylesheet"` is applied | **survived** — closed, see below |
| 19 | The census is ranked upwards | `the_census_counts_elements_and_not_declarations` |
| 20 | The census counts nothing at all | `the_unsupported_census_is_the_one_the_record_states`, and two more |
| 21 | The root's `rem` is the specification's constant, not the caller's | **survived** — closed, see below |
| 22 | The root does not start from the caller's values | `the_initial_font_size_is_the_callers_and_rem_follows_it`, `the_callers_base_font_size_changes_the_pagination` |
| 23 | A character past U+00FF is truncated to a byte | `an_unencodable_character_gets_one_stable_code` |
| 24 | An East Asian glyph is a Latin space wide | `an_east_asian_character_is_one_em_wide` |
| 25 | A repeated character is given a fresh code | **survived** — closed, see below |
| 26 | A list marker is not marked as an artifact | `every_committed_book_conserves_the_figure_the_record_states`, `a_list_marker_is_drawn_and_is_not_extracted` |
| 27 | A run is drawn in one font whatever its codes | `every_committed_book_conserves_the_figure_the_record_states`, and two more |
| 28 | A link's rectangle sits on the baseline rather than over the words | **survived** — closed, see below |
| 29 | Every family resolves to `serif` | `a_family_this_build_does_not_have_falls_through_to_the_next` |
| 30 | Every fragment resolves to its chapter's first page | `a_cross_reference_becomes_a_link_to_the_page_its_target_is_on` |
| 31 | A same-document `href="#x"` points nowhere | **survived** — closed, see below |
| 32 | An SVG content document is laid out as XHTML | `an_unresolved_spine_item_still_makes_a_page_and_keeps_its_place` |
| 33 | A placeholder page draws nothing rather than grey | `a_page_that_will_not_read_is_grey_and_a_page_that_reads_is_not` |
| 34 | The caller's font size does not reach the cascade | `the_callers_base_font_size_changes_the_pagination` |
| 35 | The first `<nav>` wins over `epub:type="toc"` | `the_toc_nav_wins_over_the_landmarks_nav` |
| 36 | A `navPoint` takes the last `<text>` it saw | `an_ncx_navmap_becomes_nested_entries` — **replaced injection**, see below |
| 37 | The NCX wins over the navigation document | `every_book_gets_an_outline_from_whichever_toc_it_has` |
| 38 | An artifact scope is a flag rather than a stack | **survived** — closed, see below |
| 39 | Artifact content is extracted | `an_artifact_is_drawn_and_is_not_extracted`, and three more |
| 40 | A literal string's `(` and `)` are not escaped | **survived** — closed, see below |
| 41 | The harness matches `hidden` as a substring | `the_source_side_drops_hidden_and_keeps_everything_else` |
| 42 | The harness reads the source as Latin-1 | `the_source_side_decodes_and_does_not_transliterate` |

#### The eight real gaps

**Row 4 — a rule both producers mask.** Deleting `p { margin: 1em 0 }` from the
user-agent sheet changed nothing about any book in the corpus, because calibre
and pandoc both set the same rule in their own stylesheets. The only place it
is reachable is a fixture with no author sheet at all — which the user-agent
test has — and it now asserts that two paragraphs are more than one line apart,
which is the difference between a block rule and a block rule with a margin.

**Row 18 — a rule no book in either corpus can reach.** Neither the committed
nor the fetched corpus contains `rel="alternate stylesheet"`. It is a function
now, `read::applies_as_stylesheet`, with a test over the spellings that matter:
`rel="stylesheet next"`, which a build comparing the whole attribute would drop,
and `rel="stylesheets"`, which a substring scan would accept.

**Row 21 — the one place a seed can be observed.** `cascade_from`'s
`root_font_size` starts at the caller's size and is immediately overwritten by
the root element's computed size, so it is visible **only** through a `rem` on
the root element itself — which `css-values-3` §5.1.1 says refers to
`font-size`'s *initial* value rather than to the root's own. The first version
of the test put the `rem` on a descendant and passed with the seed replaced by
the constant; it now declares `letter-spacing: 2rem` on the root.

**Row 25 — a defect no page and no extracted string can show.** Allocating an
overflow code per *occurrence* rather than per *character* draws every book
identically, because `encode` finds the first matching entry either way. What
it costs is the 224 codes, four times as fast. `Fonts::codes` exists so the
number can be asserted at all.

**Row 28 — a rectangle that is near the words rather than over them.** A hit
area beginning at the baseline covers the top of every glyph and none of the
bottom, and it passes every bound the test had: on the page, narrower than the
measure, shorter than a line. It now asserts that some glyph's **origin** is
strictly inside the rectangle, which is what "over the words" means.

**Row 31 — a branch the corpus cannot reach.** pandoc writes `href="#toc"` only
inside the landmarks `<nav hidden>`, which is `display: none` and generates no
run to hang an annotation on; every other cross-reference in the corpus carries
a path. So the same-document branch of the resolver had no input.
`a_same_document_reference_reaches_the_page_it_points_at` builds a container
with one chapter long enough to need three pages and asserts the anchor on page
one resolves to the last.

**Row 38 — a stack nothing in this repository nests.** `/Artifact` scopes are
counted with a stack because marked content nests; no content stream this build
writes puts a scope inside one, so a defect reducing it to a flag survived
everything. `a_scope_inside_an_artifact_does_not_end_it` is the fixture.

**Row 40 — a second producer of the same syntax.** `PageBuilder::text` has
escaped 7.3.4.2's three characters since it was written and `encoded_text` is a
second place to forget. No run in gap 31's corpus carries a parenthesis, so the
defect survived that entire suite — and a content stream with an unbalanced `(`
is a page no reader can lex. The test also found the *harness* pointing at the
wrong crate: a defect in `tinker-pdf-cos` was being measured by
`cargo test -p tinker-pdf`.

#### The two injections into unreachable code

**Row 12.** `xhtml::read` had a second check for elements left open after the
loop, and a defect that disabled it survived the whole suite — because
`tinker-pdf-xml` refuses a document that ends inside an element with
`Error::Unterminated(Construct::Element)`, which the error arm has already
recorded. **A rule enforced twice hides the reachable half**, which is milestone
7's own best find. The second enforcement is deleted, the reason is written
where it was, and the injection now targets the arm that can fire.
`MarkupDefect::Mismatched` went in the same edit: nothing can produce it,
because the reader emits one `End` per `Start` and refuses a stray end tag
before this loop sees it.

**Row 36.** The NCX reader guarded `<text>` on both the `Start` arm and the
`End` arm, and only the `End` one is reachable — it has to test the stack
anyway, because it needs a `navPoint` to put the title on. The `Start` guard is
gone and the injection now targets the guard that decides which of a
`navPoint`'s two `<text>` elements is its title: the DTD gives it a `navInfo`
beside its `navLabel`, and a build that took the last one it saw would title
every chapter with its description. The fixture grew a `navInfo` for it.

#### And one thing the harness itself got wrong

`inject.py` restores by writing the pristine bytes back and stamping the
original timestamps, which is what earlier passes did and what keeps cargo from
rebuilding twice per defect. **Cargo decides whether a *dependency* crate needs
rebuilding by comparing its sources' mtimes against the last build's
fingerprint, and an mtime that went backwards is not newer.** So the two
`tinker-pdf-css` defects at rows 21 and 22 left a defective `rlib` linked into
every later defect's build, and nineteen rows after them reported that css
defect's failing test as their own catch. Four of them were not caught at all.

The first pass read 38 of 42 and the honest number was 36. The fix is one line —
stamp the restore with the current time and pay one rebuild per defect — and it
is written into the harness's own docstring so the next milestone inherits the
finding rather than the bug.

### Still owed, and what was narrowed

- **Tables are set as inline text**, which conserves every character and puts
  none of them in a cell. Milestone 11, and the census says so per book:
  `display`, unimplemented, 38 elements on each calibre book and 19 on each
  pandoc one.
- **`vertical-align` is the largest census entry in every pandoc book and none
  of it is honoured.** Most of it goes with milestone 11's tables and milestone
  12's flex; `sup` and `sub` are neither, and are owed.
- **A character no face covers is a notdef on the page**, correct in the text
  layer and blank in the picture. Row 9's exit criterion asks for *"a character
  no available face covers producing a named warning rather than a blank"*, and
  `ArchiveWarning::UnrepresentedCharacters` covers only the ones that got no
  code at all. A code that draws a notdef is not warned about, and that is
  milestone 9's along with `@font-face`.
- **More than 224 distinct out-of-encoding characters for one face lose the
  excess.** No book in either corpus is near it — the largest is 25 — and
  milestone 9's embedded faces remove the limit rather than raising it.
- **`dashed`, `dotted` and `double` borders are drawn solid.** The border is in
  the right place at the right width in the right colour and the pattern is
  wrong. Named here rather than left to be found, because it is the one place in
  this milestone where a value is honoured *approximately* — a third thing from
  implemented and from `Unsupported`, and the census cannot see it.
- **`light-dark()` takes a colour with it.** Five elements in every pandoc book
  declare a plain hex colour and then the same colour through `light-dark()`;
  §6.1 gives the later declaration the win and this build then refuses it, so
  the page is worse than if the rule had been absent. That is the cascade
  working correctly, and the fix is `css-color-5`'s function rather than a
  special case.
- **The `<img>` in `pandoc-plates.epub` is not drawn.** Three of its five pages
  are a figure and a caption and only the caption is on the page. Images in a
  content document are milestone 9's neighbour rather than this milestone's, and
  the book still conserves every character it has.
- **`mutool` is not run over an EPUB anywhere.** Ruling 9's amendment says
  disagreeing with it is not evidence of a bug, which is a reason not to *gate*
  on it and not a reason to skip it; a job recording the disagreement as a
  number would still be worth having and is not built.
- **Milestone 2's owed facade test is closed here**, which milestone 4 predicted:
  every content document of every committed book now goes through
  `Doctype::SkipExternalId` in the facade's own path, including the EPUB 2
  book's XHTML 1.1 public identifier, and text conservation is what says they
  parsed. A build that passed `Doctype::Refuse` would refuse all six.
- **Milestone 4's `with_fonts` warning still has no fixture from a real book**,
  unchanged: no committed book needs a face this engine lacks.

## Progress — 21 August 2026, milestone 9

**A run needing three faces is three PDF text objects, at three origins each
starting where the last one left off, and qpdf agrees.** 2 626 tests, 7
ignored, up from 2 578 at milestone 8 and from 2 599 for the implementation
that was in the tree when this milestone's testing began.

The implementation reached this milestone already written — `sha1.rs`,
`obfuscation.rs`, `typeface.rs`, `paint.rs`'s per-character rewrite and
`tinker-pdf-css`'s `font_face.rs` — and compiling, and passing. What it did not
have was the assertion row 9 is actually judged on. Writing that assertion, and
the fixture it needs, found **two bugs in the new code and one rule one crate
down that this feature's own success turns into a regression**, none of which
any existing test could see. That is what the rest of this section is about: the
feature was written, and it was not finished, and the difference is a fixture
that can state its own coverage.

### The headline, and why it is a count

`css-fonts-4` §5.3 makes font matching **per character**: the `font-family`
list is walked for each character in turn and the first family with a face that
*has a glyph for that character* wins. Milestone 8 resolved the list once per
run and said so; this build resolves it per character, and the consequence is
visible in the content stream rather than only in the picture. A PDF string is
bytes in **one** font, so a run needing three faces is three `BT … ET` objects.

`a_run_needing_three_faces_becomes_three_text_objects` asserts four things, and
each fails on its own:

- **three objects**, because a build that resolved the list once per run writes
  one with six notdefs in it;
- **three different resources**, in the faces' declaration order, so the three
  segments are not three copies of one font;
- **the glyphs are the `cmap`'s** — each face numbers its own from 1, so `ABC`,
  `DEF` and `GHI` are `<000100020003>` three times over, and a build that
  passed the character through as a code would write `<004100420043>`;
- **the origins continue**, each by exactly the advance the previous face's own
  `hmtx` states. The three faces are given three different advances — 500, 750
  and 250 thousandths — precisely so this can fail: a build that measured every
  character with the first face's metrics puts objects two and three in the
  wrong place and draws a perfectly plausible page.

Beside it are two controls, because the count on its own is not a measurement.
`one_face_that_covers_the_whole_run_is_one_text_object` is the same nine
characters through a face that covers all of them: a build that started a new
object per character passes the first test and fails this one.
`a_declared_family_that_covers_nothing_is_stepped_over` declares a family whose
face exists and covers none of the run, and asserts the **resource** as well as
the count — because drawing the whole paragraph in that face is also one text
object, and is the exact failure §5.3's per-character step exists to prevent.

**qpdf reads the same three objects.** `epub_qpdf.rs` grew
`qpdf_reads_a_books_three_embedded_faces_and_their_own_widths`: `--check` is
clean, `--filtered-stream-data` decodes three `BT /Bf0`, `/Bf1`, `/Bf2` objects,
and each descendant font's `/W` is that face's own advance — `[1 [500 500
500]]`, `[1 [750 750 750]]`, `[1 [250 250 250]]`. That last one is the claim
this repository cannot make about itself: the widths a viewer will place text by
and the advances this build paginated with are the same numbers, because one is
computed from the other.

### Writing the fixture is what found the bugs

`tests/epub_package.rs`'s `boxy_font` emits `head`, `loca` and `glyf` and
nothing else. That is exactly enough to answer *"did a glyph get drawn"* for a
`FontProvider`, which reaches a glyph by the code a document wrote and asks no
`cmap` anything. It cannot answer *"which characters does this face have"*, and
§5.3's matching is that question and nothing else.

So `tests/epub_support/typeface.rs` builds a real minimal TrueType —
`cmap` format 4 in the (3, 1) Windows BMP encoding, `hmtx`, `hhea`, `maxp`,
`loca`, `glyf`, `head`, `name` — **parameterised by which characters it
covers**, so three faces with disjoint coverage can be made and a test can state
its own premise. `epub_package.rs`'s copy of `boxy_font` moved there beside it
rather than being replaced by it, and the module says why: the two answer
different questions, and giving the provider fixture a `cmap` would change what
four existing tests mean. `tests/substitute_fonts.rs` keeps its own, because it
is a test binary that includes no `epub_support` and the face it needs is
plan 05's rather than this one's.

Three of the four things that fixture made assertable were wrong.

**1. `hhea`'s ascender was read from the table's version field.** `ascender` is
at byte 4 and `descender` at byte 6; bytes 0 to 3 are the version, which is
`0x0001_0000` in every font there has ever been. So every embedded face got an
ascent of **one unit** — a thousandth of an em — and a descent of zero,
identically, whatever the file said. It is exactly the shape this plan keeps
finding: the page still has its text on it, every line is simply set a little
high, and no test in the tree could tell. `an_embedded_faces_baseline_is_placed_by_its_own_hhea`
is what says so now, with **three** faces rather than two: one face cannot
separate the file's numbers from a plausible constant, and two faces with both
fields changed cannot separate the ascent from the descent.

**2. `Page::text()` broke a paragraph at every face change.**
`tinker-pdf-content`'s text device treated `ET` as a hard line break, with that
comment beside it. It is a defensible rule until something produces the
counter-example, and per-character fallback is that counter-example: `ABCDEFGHI`
in three faces extracted as `"ABC\nDEF\nGHI\n"`, three lines, on one baseline —
so `search("ABCDEFGHI")` found nothing, because `search` matches within a line.
It is not an EPUB-only shape either: a producer that emits a text object per
styled span writes the same page, and every bold word in the middle of a
sentence is one.

The fix is that an `ET` puts the **pen** down rather than ending the line: the
line is held open and the next glyph has to *continue* it — same baseline, same
direction, and within half an em of where the last glyph's box ended — rather
than merely land somewhere on the same line, which is what two cells of a table
row do. `a_text_object_boundary_is_not_a_line_break_and_a_gap_still_is` asserts
both halves, because a build that never broke at all joins the table row.
Nothing else in the workspace moved.

**3. The `@font-face` de-duplication did not work for a `<style>` element.**
`synthesise` collapses equal rules across the spine, with a comment saying that
thirteen chapters sharing one stylesheet declare one face. That is true for a
`<link>`ed sheet, whose faces carry **that sheet's** address. A `<style>`
element has no address of its own, so `read.rs` fills in the **content
document's** — a different string in every chapter — and the rules are not equal
and cannot be. Thirteen chapters with the same `<style>` block were thirteen
rules, thirteen inflations and thirteen parses of the same font program.

The fix is in `typeface::load`, against the **resolved container path**, which
is the only place the two are the same thing; the defect list is deliberately
*not* collapsed with it, because thirteen rules that all failed are thirteen
rules and `ArchiveWarning::FontFace`'s count is where that is said. The test
asserts both: two chapters produce one face, and two chapters produce one
warning saying `rules: 2`.

### And one assertion of mine was wrong

The first version of the `hhea` test measured the **line height** — the gap
between two baselines — and found 14.4 points for a face with a 0.8 ascent and
for a face with a 1.6 ascent alike. That is not a bug: CSS 2.1 §10.8 makes the
line *box* `line-height`, which is `normal` here and so a multiple of the font
size whatever face is used, and what the face's metrics decide is where the
baseline sits **inside** it, by the half-leading rule. The test moved to the
baseline, and the half-leading constant cancels out of a difference of two —
which is why the assertion is arithmetic the test states rather than a number
copied out of a run.

### `FontProvider`'s per-family question, answered: the trait is not extended

Row 9 asks for *"`FontProvider`'s per-family fallback question answered — the
trait extended, or the reason it is not recorded"*. **It is not extended, and
the reason is that the trait already answers per family.**

`FontRequest` carries `base_font`, and this build's synthesis writes a
**distinct** `/BaseFont` per generic family — `Times-Roman`, `Helvetica`,
`Courier`, twelve names across the standard 14 — so a host with a serif face and
a sans face is already asked for each by name and can already answer
differently. Extending the trait would add a second way to say the same thing
and a second way for the two to disagree.

That is evidence rather than an assertion:
`a_provider_is_asked_per_family_and_the_three_generics_arrive_by_name` attaches
a recording provider to a book with a `serif`, a `sans-serif` and a `monospace`
paragraph, renders a page, and asserts the three names that arrived — sorted and
deduplicated, so a build that asked once with one name fails.

What a provider **cannot** do is change the pagination, and that is deliberate
rather than a limitation of the trait. It is milestone 4's whole argument for
`OpenOptions::fonts`, and it now has both halves asserted:

- `the_page_count_does_not_depend_on_whether_a_provider_is_attached` opens three
  committed books twice, with and without a provider, and compares the page
  count **and the first page's whole content stream** — byte for byte, so a
  build in which a provider moved one glyph fails;
- `the_three_generic_families_measure_at_their_own_published_advances` is the
  reason it can hold: Times-Roman's `a` is 444 thousandths, Helvetica's 556 and
  Courier's 600, and the three ascent-and-descent pairs are the three families'
  own AFM numbers. A build that gave all three the same numbers would paginate
  consistently and wrongly, and the first test would still pass.

### The font census, per book

Milestone 8's `Still owed` named this milestone's two: *"a notdef glyph unwarned
and more than 224 out-of-encoding characters per face lost"*. Both moved.

| Book | Notdefs drawn | Characters lost | `@font-face` defects |
| --- | --- | --- | --- |
| `calibre-book-cover.epub` | **24** | 0 | 0 |
| `calibre-book-nocover.epub` | **24** | 0 | 0 |
| `pandoc-book-cover.epub` | **25** | 0 | 0 |
| `pandoc-book-epub2.epub` | **25** | 0 | 0 |
| `pandoc-book-nocover.epub` | **25** | 0 | 0 |
| `pandoc-plates.epub` | **0** | 0 | 0 |

The record is `tests/epub/FONTS.tsv` and
`the_font_census_is_the_one_the_record_states` is the ratchet, in `CENSUS.tsv`'s
shape and for its reason: a milestone that gives this build a CJK face has to
re-measure rather than argue.

**Every one of those twenty-four was there at milestone 8 and none was
reported.** The number is not new work on the page — it is the line of Japanese
milestone 1 put in five of the six books, drawn as a notdef through the overflow
font — it is new work in the *report*, and the distinction it rests on is the
one `ArchiveWarning` now makes: an **unrepresented** character is missing from
the picture *and* from `Page::text()`, and an **uncovered** one is missing from
the picture and present in the text. A reader that reported them as one number
would make a book whose text can still be searched indistinguishable from one
whose cannot. `pandoc-plates.epub` is the control: it is the one committed book
with no Japanese in it, it reports zero, and
`the_notdef_count_is_a_property_of_the_book_and_not_a_constant` is what stops
the whole table being satisfied by a constant.

**The 224-code ceiling is gone for a book that brings its own face**, which is
the second half. An embedded face draws through a **composite** font under
`/Identity-H`, where the two-byte code *is* the glyph index, so `/Differences`'s
224 codes are not in the question.
`an_embedded_face_removes_the_overflow_ceiling_and_the_warning` sets three
hundred distinct characters — past the ceiling by seventy-six — twice: the
control loses seventy-six and draws two hundred and twenty-four notdefs, and the
same book with a face covering all three hundred reports **neither** warning and
extracts every character. It is not raised for a book without a face, and that
is recorded rather than fixed: the ceiling is `/Differences`'s own size and the
answer to it is an embedded face, not a larger array.

### The injection matrix

**Forty-five defects, forty-three caught on the first pass.** One survivor was a
defect that changed no behaviour; the other was a real gap in the fixtures, and
it is closed. The whole matrix was re-run against the tree as committed:
**forty-five of forty-five**.

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | paint: the family list is resolved once per run | `a_face_the_family_list_never_mentions_is_still_the_system_fallback`, and three more |
| 2 | paint: a segment never ends, so a run is one object | `every_committed_book_conserves_the_figure_the_record_states`, and six more |
| 3 | paint: every segment starts at the run origin | `a_run_needing_three_faces_becomes_three_text_objects` |
| 4 | paint: the advance is one character wide for all | `a_run_needing_three_faces_becomes_three_text_objects` |
| 5 | paint: an embedded face measures at half an em | `a_run_needing_three_faces_becomes_three_text_objects` |
| 6 | paint: an unknown family falls straight to serif | `a_family_this_build_does_not_have_falls_through_to_the_next`, and three more |
| 7 | paint: §5.3 has no system fallback | `a_face_the_family_list_never_mentions_is_still_the_system_fallback` |
| 8 | paint: a notdef is counted per distinct character | `a_character_no_face_covers_is_named_rather_than_left_blank`, `the_font_census_is_the_one_the_record_states` |
| 9 | paint: a notdef is not counted at all | `a_character_no_face_covers_is_named_rather_than_left_blank`, and three more |
| 10 | paint: a lost character is reported as a notdef | `characters_past_the_overflow_font_are_counted`, `an_embedded_face_removes_the_overflow_ceiling_and_the_warning` |
| 11 | paint: an overflow code is spent per occurrence | `an_unencodable_character_gets_one_stable_code` |
| 12 | paint: the standard 14 claim every character | `a_face_the_family_list_never_mentions_is_still_the_system_fallback` |
| 13 | face: hhea ascender read from the version field | `an_embedded_faces_baseline_is_placed_by_its_own_hhea` |
| 14 | face: hhea descender read from the ascender slot | `an_embedded_faces_baseline_is_placed_by_its_own_hhea` |
| 15 | face: a cmap answering notdef is a match | `a_declared_family_that_covers_nothing_is_stepped_over`, and two more |
| 16 | face: best ignores coverage | `a_declared_family_that_covers_nothing_is_stepped_over`, and two more |
| 17 | face: a format hint is never refused | `a_face_declared_in_every_chapter_is_loaded_once_and_every_rule_is_counted`, and two more |
| 18 | face: an unknown format keyword is refused | `an_unrecognised_format_hint_does_not_refuse_a_perfectly_good_file` |
| 19 | face: a wOFF signature is not sniffed | `woff_and_woff2_are_refused_by_name_on_the_hint_and_on_the_bytes` |
| 20 | face: wOF2 is sniffed as woff | `woff_and_woff2_are_refused_by_name_on_the_hint_and_on_the_bytes` |
| 21 | face: the src list stops at the first failure | `a_src_list_is_walked_past_a_refused_entry_and_the_refusal_is_still_named` |
| 22 | face: a rule that worked is reported as a whole failure | `a_face_declared_in_every_chapter_is_loaded_once_and_every_rule_is_counted`, and three more |
| 23 | face: local() is reported as a missing url | `a_local_source_is_refused_under_its_own_name` |
| 24 | face: one file declared twice is loaded twice | `a_face_declared_in_every_chapter_is_loaded_once_and_every_rule_is_counted` |
| 25 | obfuscation: IDPF covers 1024 bytes | `a_pretty_printed_identifier_still_opens_its_own_obfuscated_font`, `the_idpf_obfuscation_is_undone_over_its_own_thousand_and_forty_bytes` |
| 26 | obfuscation: Adobe covers 1040 bytes | `the_adobe_obfuscation_is_undone_over_its_own_thousand_and_twenty_four_bytes` |
| 27 | obfuscation: the identifier is trimmed, not stripped | `the_idpf_key_strips_the_whitespace_section_4_4_3_says_to_strip` |
| 28 | obfuscation: the empty identifier is hashed anyway | `the_idpf_key_strips_the_whitespace_section_4_4_3_says_to_strip` |
| 29 | obfuscation: a UUID of any length is a key | `an_identifier_that_is_not_a_uuid_has_no_adobe_key_and_says_so` |
| 30 | obfuscation: the key's nibbles are swapped | `the_adobe_obfuscation_is_undone_over_its_own_thousand_and_twenty_four_bytes` |
| 31 | obfuscation: the xor uses one key byte throughout | `a_font_shorter_than_the_obfuscated_run_is_covered_to_its_end`, and three more |
| 32 | content: an ET is a hard line break again | `a_text_object_boundary_is_not_a_line_break_and_a_gap_still_is` |
| 33 | content: a closed line resumes anywhere on its baseline | `a_text_object_boundary_is_not_a_line_break_and_a_gap_still_is` |
| 34 | sha1: the message schedule is not rotated (SHA-0) | `the_two_implementations_agree_over_every_length_to_two_blocks`, and two more |
| 35 | sha1: b is not rotated by thirty | `the_two_implementations_agree_over_every_length_to_two_blocks`, and two more |
| 36 | sha1: the reference implementation drifts | `the_reference_implementation_is_pinned_to_a_published_vector`, `the_two_implementations_agree_over_every_length_to_two_blocks` |
| 37 | css: an src that parses to nothing is still an src | `a_src_that_parses_to_nothing_makes_the_rule_invalid` |
| 38 | css: the src list keeps only its first entry | `the_src_fallback_list_keeps_every_entry_in_order` |
| 39 | epub: a face defect never reaches the report | `a_local_source_is_refused_under_its_own_name`, and three more |
| 40 | epub: the notdef warning is never raised | `a_character_no_face_covers_is_named_rather_than_left_blank`, and three more |
| 41 | harness: the fixture cmap maps every code to glyph one | `a_fixture_face_covers_what_it_says_and_reads_back_through_sfnt`, and three more |
| 42 | harness: the fixture advance is written as zero | `a_fixture_face_covers_what_it_says_and_reads_back_through_sfnt`, and two more |
| 43 | harness: the fixture numbers its glyphs from zero | `a_fixture_face_covers_what_it_says_and_reads_back_through_sfnt`, and nine more |
| 44 | harness: text_objects sees only the first object | `a_run_needing_three_faces_becomes_three_text_objects` |
| 45 | harness: origin_of reads the operands after Td | `a_run_needing_three_faces_becomes_three_text_objects`, `an_embedded_faces_baseline_is_placed_by_its_own_hhea` |

#### The survivor that was mine, not the build's

`face: a rule that worked is reported as a whole failure` replaced `None =>`
with `_ =>` in a match whose other arm is `Some(...)`. After a `Some` arm, `_`
**is** the `None` arm: the injection compiled, changed nothing, and read as a
survivor. Rewritten to push the defect unconditionally, it is caught by four
tests. A defect that changes no behaviour is not a defect, and it is worth
naming because it costs a pass to notice.

#### The survivor that was real: a fixture and a reader sharing one derivation

`obfuscation: the key's nibbles are swapped` survived every assertion in the
file. Every fixture obfuscated its font with the key `adobe_key` handed it and
then asked this build to undo it — with the key the **same function** handed it.
A wrong key XORs and un-XORs exactly, so the bytes came back right and the page
came out right and the test proved that the function agrees with itself.

This plan's own list of ways a milestone can be lost has it: *"a fixture giving
the right answer for the wrong reason"*. The fix is to state both keys as
constants — `ADOBE_KEY`, the sixteen bytes of the identifier's UUID read straight
off it, and `IDPF_KEY`, the SHA-1 digest `0abddf6a…` — obfuscate with those, and
assert the two functions against them. That is the only form of the assertion a
shared derivation cannot satisfy, and it is what a second implementation is for
one crate down: `tinker-pdf-crypto`'s SHA-1 has one, and the matrix confirms it
works in both directions — a defect in the primary implementation and a defect
in the reference one are each caught, the second by
`the_reference_implementation_is_pinned_to_a_published_vector` as well as by the
agreement test.

#### The harness

The copy in the scratchpad has none of the three bugs earlier milestones found
in it, and its docstring now names all three so the next one inherits the
finding rather than the bug: milestone 3's `shutil.copy2` preserving an mtime
(nothing is copied), milestone 4's unrevertable empty-string replacement
(nothing is reversed — the restore writes pristine bytes), and milestone 8's
restore stamping the *original* mtime and leaving a defective dependency rlib
linked (the restore stamps the current time and pays one rebuild per defect).
Every anchor was read out of the file it names before it was written, and all
forty-five matched exactly once on the first verification pass.

### Still owed

- **The fetched corpus is not here, so no *real* book's fonts were read.**
  `tests/epub/README.md` names `wasteland-otf-obf` and `wasteland-woff-obf` as
  milestone 9's real-book evidence and `TINKER_EPUB_CORPUS` is unset on this
  machine, so `epub_fetched.rs` skipped and every `@font-face` this milestone
  read came from a fixture. The fixtures are honest about what they are —
  synthesised, with their coverage stated — but a real producer's obfuscated OTF
  has not been through this path. It is the same gap milestone 8 recorded for
  `with_fonts`, one file further along.
- **WOFF and WOFF2 are refused rather than decoded**, which is the plan's own
  scope decision and not a defect. What is owed is the number: no committed book
  has one, so how much of a real corpus this loses is unmeasured until the
  fetched corpus runs.
- **`local()` is always unavailable.** A reading system with no installed faces
  is what this build is, and the defect names it; a host that *has* faces has no
  way to offer them to the `@font-face` path, because `FontProvider` is consulted
  at render and this resolution happens at `open`. Answering it would mean a
  second seam and it is not built.
- **`unicode-range` is not read**, so a book that splits one family across four
  subsetted files gets all four as candidates for every character and the first
  that covers it wins. That is the right glyph in every case this build can
  construct — coverage is what §5.3 asks about and the files do not overlap — but
  it is not §4.4's algorithm and a book with deliberately overlapping ranges
  would resolve to the wrong file.
- **`font-stretch`, `font-feature-settings`, `font-variation-settings` and
  `font-display` are parsed past rather than read**, unchanged from the CSS
  crate's own record.
- **A `<style>` element's faces are deduplicated by container path and a
  `<link>`ed sheet's by rule**, which is two mechanisms for one property. They
  agree on every book that can be built today; a single mechanism would be
  better and is not worth a public type change yet.
- **The `hhea` fallback is the standard face's proportions**, not the file's
  `OS/2` `sTypoAscender`, which is what a browser prefers when `USE_TYPO_METRICS`
  is set. No fixture and no committed book has an `OS/2` at all.
- Milestone 8's list is otherwise unchanged: tables set as inline text,
  `vertical-align` unhonoured, dashed and dotted borders drawn solid,
  `light-dark()` taking a colour with it, `pandoc-plates.epub`'s `<img>`
  undrawn, and `mutool` never run over an EPUB.

## Progress — 21 August 2026, milestone 10

**Nine constraints, nine fixtures, and each one fails on its own.** Floats and
`clear` are laid out rather than named: CSS 2.2 §9.5.1's nine placement rules,
§9.5.2's clearance, §9.5's line boxes shortened beside a float and shifted below
one, §10.3.5's shrink-to-fit, and the fragmentation interaction the row calls
*"the one that loses text"*. **2 653 tests, 7 ignored**, up from 2 626 at
milestone 9.

The two warnings this crate has carried since milestone 7 — `FloatInFlow` and
`ClearIgnored` — are **gone**, which is the only honest way for a warning of
that kind to end. One replaces them and it names a smaller gap:
`FloatBrokenAcrossPages`.

### What the design got wrong, and how it found out

**1. Rules 4, 5 and 6 cannot be told apart by any fixture with only positive
margins in it.** They are three ceilings on one number — a float's outer top may
not be above *its containing block's top*, above *any earlier box's top*, or
above *any earlier line box's top* — and in an ordinary document the float's
static position is already below all three, so a build with all three deleted
lays every book out identically. Worse, the obvious way to make one of them bind
makes the other two bind at the same value: a float pushed up by a negative
margin inside its containing block is pushed up past a box whose own top *is*
that containing block's top.

The three fixtures are built out of negative margins for that reason, and each
puts its own ceiling strictly below the other two:

- rule 4's containing block has a 50-point padding and its first paragraph a
  negative *top* margin, so the earlier box's top is 10 and the earlier line's
  is 10 and only the containing block's 50 can be what stopped the float;
- rule 5's earlier box is an empty spacer with a top margin, so its border-box
  top is 42 while every line box in the document is at 0;
- rule 6's earlier box has a *padding* rather than a margin, so its border-box
  top is 0 and its line box is at 40.

The injection matrix confirms the separation: deleting any one of the three
fails that one's fixture and no other float fixture at all.

**2. The linear column cannot hold a float.** `fragment` cuts pages by walking
one vector of items whose `y` never goes backwards; a float is placed *above*
the line boxes that flow around it, at a position the column has already passed.
So a float is a `FloatRecord` — its own items, its own block records, in the same
coordinates — and pagination draws whatever of each record falls inside the
page's window.

**3. Reading order stops being emission order the moment a float exists**, and
text conservation is an *ordered* comparison. A float written in the middle of a
paragraph is laid out before the words that follow it and drawn beside them; a
float broken over a page boundary finishes on the page **after** text that
precedes it in the book. Neither loses a character and both fail an ordered
comparison, so every `TextRun` carries an `order` — its position in document
order — a page's runs are sorted by it, and `Layout::text` sorts by it across the
whole book. For a book with no floats the two orders are the same order and the
stable sort changes nothing.

**4. A float can outlive the column it was written in, and pagination stopped
where the column did.** A figure at the foot of the last page of a chapter
extends past the last line of it; the old loop ran while there were items left
in the column, so the rest of the float was never drawn. It is the defect this
plan uses to introduce text conservation — *"a float that pushed content off the
page bottom and lost it"* — and it is now a second loop with a fixture and an
injection of its own.

**5. `Warning::InlineBlockAsBlock` could not fire, and had not since milestone
7.** Deleting the two float warnings left the two tests that *count* warnings
without a subject, and the obvious replacement did not work: an `inline-block`
is not block-level, so it never reaches the block builder where that warning was
raised. It is now raised where inline content is gathered, which is where an
`inline-block` actually arrives, and renamed `InlineBlockAsInline` because that
is what this build does with one.

**6. The browser oracle's comparisons shared one scratch file.** `cargo test`
runs the tests in a file in parallel and both wrote their page to
`continuous.html`, so the float comparison's first run measured the corpus
chapter the other test had just written. What caught it is milestone 8's own
decision — *the block sequence is asserted exactly, with no tolerance* — which
said the browser's blocks were about ZIP containers and this engine's were about
figures. A y-offset tolerance would have failed with a number and told nobody
why.

### The bound milestone 7 said would arrive here

`limits.rs` and `bounds_ledger.rs` both carried milestone 7's argument for
`MAX_LAYOUT_WORK`'s absence, and both ended it with the same sentence: **the
bound arrives with the multi-pass layout or not at all, which is milestones 10
and 11.** Floats are that layout, and the quadratic is not hypothetical:
§9.5.1 places each float against **every float already placed**, §9.5's line
boxes ask all of them for their measure, and §9.5.2 asks them again for every
cleared block. `MAX_BOX_TREE_NODES` is 262 144, so a book may float that many
boxes and the last of them is examined against the other 262 143 — **6.9e10
examinations with every other cap satisfied**. It is the first row in the ledger
whose ceiling is the *square* of another row.

The cap is **4 000 000** float examinations, spent across a whole book. A
400-page novel with a figure per spine item spends about 24 000 of it and one
with a figure every fourth page about 240 000; the ledger row carries both
numbers. It is charged in **three** places, because it is three scans, and it
has **three** firing fixtures for the reason this run keeps finding: a rule
enforced in three places has three reachable halves, and the injection matrix
below has a line for each.

### The browser agreement, as a number

| | Deviation, as a fraction of the browser's column |
| --- | --- |
| **This build against Chrome 151, float-heavy document** | **0.0154** |
| of which: the browser reports a line box's top and this engine a baseline, at the 32-pixel `<h1>` the offsets are measured from | 0.0150 |
| of which: everything else, over sixteen blocks and six figures | 0.0004 |
| The cap | 0.05 |
| **The injected defect: the same document with `float: none` on this engine's side** | **0.2396** |

The second row is the finding. The per-block deltas printed by
`TINKER_BROWSER_TABLE=1` are 0.0150, 0.0151, 0.0152 … all the way down — **a
constant established at the first block and never added to**. Every float in
that document is on the same line of the same paragraph as Chrome's, to within
half a pixel.

**Two variables are held fixed and both are stated on both sides.** Milestone 8
fixed the face; this fixes what `line-height: normal` means. This build resolves
it as 1.2 and Chrome resolves it from Courier New's own metrics, which is 1.133:
six per cent of every line in the document, accumulating down the column, and
nothing whatever to do with where a float went. Measured without that rule the
two columns disagree by 0.0449, and the whole systematic part of it is that
ratio. It is a rule in the fixture's own stylesheet, which reaches the browser
and the cascade by the same path.

### The injection matrix

**Fifty-six injections of forty-eight distinct defects**, in two passes: forty
against the placement, the clearance, the line boxes, the fragmentation and the
document order, and eight more against the work cap after it was declared. Nine
of the first forty survived, and **every one of the nine was a real gap in the
fixtures rather than an equivalent mutant** — the whole section was built out of
left floats, so the right-hand half of rules 2, 3 and 7 was untested; clearance
had a float for a predecessor in every fixture, so the half of it that stops
margins collapsing was untested; and two of them were wrong for reasons worth
their own paragraphs below. One defect was caught by the browser oracle and by
nothing else, which is this run's named failure mode and is closed by a unit
fixture in the same row.

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | **Rule 1** — a left float clamped to the page rather than to its containing block | `rule_1_a_float_does_not_leave_its_containing_block` |
| 2 | **Rule 1** — a right float's own edge measured from the wrong side | `rule_1_a_float_does_not_leave_its_containing_block`, `rule_3_a_float_does_not_cross_an_earlier_float_on_the_other_side` |
| 3 | **Rule 2** — an earlier float on the same side ignored | `rule_2_a_float_does_not_overlap_an_earlier_float_on_its_own_side`, `rule_7_two_floats_that_do_not_fit_side_by_side_stack` |
| 4 | The same, on the right | **survived** — closed by `rule_2_a_float_does_not_overlap_an_earlier_float_on_its_own_side` |
| 5 | **Rule 3** — an earlier float on the other side ignored | `rule_3_a_float_does_not_cross_an_earlier_float_on_the_other_side` |
| 6 | The same, on the right | **survived** — closed by `rule_3_a_float_does_not_cross_an_earlier_float_on_the_other_side` |
| 7 | **Rule 4** — the containing block's top is not a ceiling | `rule_4_a_float_does_not_rise_above_its_containing_block` |
| 8 | **Rule 5** — an earlier box's top is not a ceiling | `rule_5_a_float_does_not_rise_above_an_earlier_box`, `rule_8_a_float_is_placed_as_high_as_it_fits` |
| 9 | **Rule 6** — an earlier line box's top is not a ceiling | `rule_6_a_float_does_not_rise_above_an_earlier_line_box` |
| 10 | **Rule 7** — two stacked floats may leave the containing block | `rule_7_two_floats_that_do_not_fit_side_by_side_stack`, `rule_8_a_float_is_placed_as_high_as_it_fits` |
| 11 | The same, on the right | **survived** — closed by `rule_7_two_floats_that_do_not_fit_side_by_side_stack` |
| 12 | Its condition dropped: every float clamped whether stacked or not | **survived** — closed by `rule_7_two_floats_that_do_not_fit_side_by_side_stack` |
| 13 | **Rule 8** — the candidates tried lowest last rather than first | `rule_1_a_float_does_not_leave_its_containing_block`, `rule_2_a_float_does_not_overlap_an_earlier_float_on_its_own_side` |
| 14 | **Rule 8** — only the ceiling is ever tried | `rule_3_a_float_does_not_cross_an_earlier_float_on_the_other_side`, `rule_7_two_floats_that_do_not_fit_side_by_side_stack` |
| 15 | **Rule 9** — the two ends of the range swapped | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so`, `a_float_inside_a_paragraph_keeps_the_paragraph_whole` |
| 16 | A float above the range still counted as beside it | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so`, `a_float_taller_than_its_containing_block_goes_on_shortening_lines` |
| 17 | The inner edge of a float taken from its outer side | `a_float_inside_a_paragraph_keeps_the_paragraph_whole`, `a_float_taller_than_its_containing_block_goes_on_shortening_lines` |
| 18 | The rule 5 ceiling recorded from nothing: no box ever contributes | `rule_5_a_float_does_not_rise_above_an_earlier_box` |
| 19 | The rule 6 ceiling recorded from nothing: no line ever contributes | `rule_6_a_float_does_not_rise_above_an_earlier_line_box` |
| 20 | A float's static position taken before the margins standing at it | `rule_5_a_float_does_not_rise_above_an_earlier_box`, `rule_6_a_float_does_not_rise_above_an_earlier_line_box` |
| 21 | A float placed but not moved to where it was placed | `rule_1_a_float_does_not_leave_its_containing_block`, `rule_2_a_float_does_not_overlap_an_earlier_float_on_its_own_side` |
| 22 | Clearance added to the box's own margin rather than measured with it | `clearance_is_what_is_still_needed_and_not_the_floats_bottom` |
| 23 | Clearance computed and not applied | `clearance_moves_a_box_below_the_floats_it_names` |
| 24 | Every clear value clears both sides | `clearance_moves_a_box_below_the_floats_it_names` |
| 25 | Clearance no longer stops the margins collapsing through it | **survived** — closed by `clearance_does_not_let_the_margins_it_sits_between_collapse` |
| 26 | A line box not shortened by a left float | `a_float_inside_a_paragraph_keeps_the_paragraph_whole`, `a_float_taller_than_its_containing_block_goes_on_shortening_lines` |
| 27 | A line box not shortened by a right float | **only the browser oracle** — closed by `a_line_box_is_shortened_beside_a_float_and_restored_below_it` |
| 28 | A line with no room beside a float is set there anyway | `a_line_with_no_room_beside_a_float_goes_under_it` |
| 29 | A line pushed past every float rather than under the next one | **survived** — closed by `a_line_with_no_room_beside_a_float_goes_under_it` |
| 30 | A word too long for the measure sends its line under the floats | `a_line_with_no_room_beside_a_float_goes_under_it` |
| 31 | An auto-width float given the whole containing block | `a_float_with_no_width_is_shrunk_to_fit` |
| 32 | Shrink-to-fit without its preferred-width clamp | `a_float_with_no_width_is_shrunk_to_fit` |
| 33 | A float that does not fit its page drawn off the bottom of it | **survived** — closed by `a_float_that_would_fall_off_the_page_bottom_is_pushed_whole` |
| 34 | A float never broken, so what did not fit is drawn off the page | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so`, `a_float_outliving_the_column_still_gets_a_page` |
| 35 | A float broken and not said to be | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so` |
| 36 | A float continuing on a page it is drawn above the top of | **survived** — closed by `a_float_continuing_onto_a_page_starts_at_the_top_of_it` |
| 37 | The column stops where the text does, and the float below it is lost | `a_float_outliving_the_column_still_gets_a_page` |
| 38 | Reading order is emission order again | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so`, `a_float_outliving_the_column_still_gets_a_page` |
| 39 | A page's runs left in the order the boxes were made | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so`, `a_float_inside_a_paragraph_keeps_the_paragraph_whole` |
| 40 | Every run stamped with the same document position | `a_float_broken_across_a_page_keeps_all_of_itself_and_says_so`, `a_float_inside_a_paragraph_keeps_the_paragraph_whole` |

| # | Defect against `MAX_LAYOUT_WORK` | Caught by |
| --- | --- | --- |
| 1 | The placement scan is not charged | `a_book_past_the_float_work_total_is_refused_by_name`, `a_book_past_the_float_work_total_through_its_clearances_is_refused_by_name` |
| 2 | The candidate scan is not charged | `a_book_past_the_float_work_total_through_its_clearances_is_refused_by_name` |
| 3 | A line box's search for a band is not charged | `a_book_past_the_float_work_total_through_its_line_boxes_is_refused_by_name` |
| 4 | A line box's own band is not charged | **survived** — and the scan it charged for is gone: it recomputed, at the same height and over the same list, the band the loop above it had already found |
| 5 | A clearance scan is not charged | `a_book_past_the_float_work_total_through_its_clearances_is_refused_by_name` |
| 6 | A float's own clearance scan is not charged | **survived** — recorded below |
| 7 | The float total is counted and never accumulated | `a_book_past_the_float_work_total_is_refused_by_name`, `a_book_past_the_float_work_total_through_its_clearances_is_refused_by_name` |
| 8 | The float total is spent without being refused | `a_book_past_the_float_work_total_is_refused_by_name`, `a_book_past_the_float_work_total_through_its_clearances_is_refused_by_name` |

#### The two survivors that were findings rather than fixtures

**`a float that does not fit its page drawn off the bottom of it` survived
because the fixture named for the push was going through the break path.** The
test asked whether the float's *first item* fits the remaining space, and a
float's first item is a zero-height top margin: it always fits, the push never
ran, and what actually happened was that the margin was drawn on one page and
everything else on the next — same runs, same positions, one extra warning that
nothing asserted the absence of. It is this plan's own *"a fixture giving the
right answer for the wrong reason"*, and the fix is that the **margin box**
decides: a figure that would fit on a page of its own is pushed whole, one that
would not is broken wherever it starts. The fixture now asserts the warning is
**absent**, which is the only thing that separates the two behaviours.

**`rule 7's condition dropped` survived because the answer it changes is the
fallback nobody reaches.** Clamping every float to its containing block's far
edge, rather than only one that has another float beside it, rejects a
too-wide float at every candidate height — and the correct code also puts a lone
too-wide float exactly where the fallback does. The two agree in every
arrangement the section had. They part when the *correct* answer is not the
fallback: a lone float wider than its containing block with an earlier float on
the **other** side, which rule 3 sends below it and the broken build puts back
at the top, straight through it. That is now the third half of rule 7's fixture.

#### The one that is recorded rather than closed

`a float's own clearance scan is not charged` survives, and it is bounded rather
than untested. A float with `clear` is scanned once against the placed floats
before it is placed — and then **placed**, which charges the same list at least
three times over in the same call. Deleting the clearance charge under-counts a
float's cost by at most a quarter and cannot change what the cap is for: the
work is still `O(floats²)`, still charged, and still refused at the same order
of magnitude. A firing fixture for it would have to hold the placement charge
constant while varying only the clearance one, which is a fixture about
arithmetic rather than about a book. The block-level clearance scan — the one
that is *not* accompanied by a placement — is charged, has its own firing
fixture, and its deletion is caught.

#### The harness

`inject_m10.py` in the scratchpad is `inject_m9.py` with a second stage, and its
docstring names all three of the harness bugs this run has found so the next
milestone inherits the finding rather than the bug: milestone 3's `shutil.copy2`
preserving an mtime, milestone 4's unrevertable empty-string replacement, and
milestone 8's restore stamping the original mtime and leaving a defective
dependency rlib linked. The second stage is new and it earns its place: the
layout crate's own tests run in a second, so only a defect that survives them
costs three minutes of the whole workspace — and a defect that reaches the second
stage is **reported as such**, which is how row 27 was found to be caught by the
browser and by nothing else. Every anchor was read out of the file it names
before it was written, and all fifty-six matched exactly once on the first
verification pass.


### Still owed, and what was narrowed

- **A float met inside a paragraph is placed at the top of that paragraph, not
  on the line it was written on.** The paragraph is *not* split — the words
  either side of the float are one inline formatting context and set as one
  line, which is what §9.5 asks for and the shape a real book uses — but
  §9.5.1's rule 6 wants the float's outer top no higher than the line box it sat
  on, and the line boxes do not exist when the float is met. A drop cap or a
  lead figure at the start of a paragraph is exactly right; one written halfway
  down a long paragraph floats up to the top of it.
- **`css-break-3` would push a float that has already begun**, and this build
  breaks it. A float that has *not* begun **is** pushed whole to the next page,
  which is the common case and costs nothing; once part of it is drawn, pushing
  the rest would mean re-laying-out the lines that were shortened beside it,
  which is a second layout of the content rather than a second position for the
  box. Nothing is lost either way and `FloatBrokenAcrossPages` names it.
- **Shrink-to-fit measures text and not boxes.** A block inside an auto-width
  float with a stated `width` does not widen the float, because counting block
  records would make the preferred width of every float the width of the trial
  measure — the shrink-to-fit bug that produces a page-wide float holding one
  word.
- **An auto-width float is laid out three times**: once at a measure nothing
  reaches, once at a measure nothing fits, once for real. Every pass is charged
  to the same budget as everything else, and a float with a stated `width` pays
  for none of them.
- **`overflow` does not establish a block formatting context**, because
  `overflow` is not a property this build has. The pre-`flow-root` clearfix — a
  float and a following `overflow: hidden` block — is laid out as if the
  property were absent.
- **A line box's band is probed at the container's own `line-height`**, not at
  the line's finished height, which is not known until it is filled and cannot
  be filled until its width is known. The case it gets wrong is one oversized
  inline on a line beside a float.
- Milestone 9's list is otherwise unchanged: the fetched corpus is unset on this
  machine, WOFF is refused, `local()` is unavailable, `unicode-range` is unread,
  tables are set as inline text, `vertical-align` is unhonoured, and `mutool`
  has never been run over an EPUB.


## Progress — 21 August 2026, milestone 11

**Nine generation steps, two width algorithms, two border models, and a
comparison against Chrome that agrees to 0.0005 of the column.** CSS 2.2 §17 is
laid out rather than named: §17.2.1's anonymous table objects, §17.5's grid with
`colspan` and `rowspan`, §17.5.2.1's fixed algorithm and §17.5.2.2's *two-pass*
automatic one, §17.6.1's separated model and §17.6.2's collapsing model with
§17.6.2.1's five ordered rules, a nested table, and fragmentation between the
rows. **2 722 tests, 7 ignored**, up from 2 653 at milestone 10.

Fourteen `display` values where there were five, three properties where there
were none — `border-collapse`, `border-spacing`, `table-layout` — and the
`display` row of the committed corpus's unsupported census is **empty**, which
is the first time this plan has emptied one.

### What the design got wrong, and how it found out

**1. The row's own sentence about `<tbody>` is false for this corpus, and the
fixture that was written to assert it failed.** The plan says the fixup a real
book needs is the missing row group, *"because HTML tables in the wild omit
`<tbody>`"*. Hand-written and legacy HTML does; **pandoc and calibre do not**.
Both write `<thead>` and `<tbody>` in full, and the first corpus fixture — which
asserted `!document.contains("<tbody")` — failed on its first run. What every
one of them *does* write is indentation between the tags, so the step that fires
on every table in the committed corpus is §17.2.1's **rule 3**, white space
between two proper table children, and without it every real table gets an empty
anonymous cell between every pair of real ones. The fixture now asserts the
measurement in the direction it came out, the row is amended in place, and the
bare-`<tr>` table keeps its own fixtures because it is still the markup a
hand-written book uses.

**2. §17.5.2.1's own first sentence is the one an implementation skips.** *"A
value of `auto` means use the automatic table layout algorithm"* — so
`table-layout: fixed` with no stated `width` is **not** the fixed algorithm. A
build that read only the property name divides the containing block evenly among
the columns and draws a table that looks entirely reasonable. It has its own
fixture and its own injected defect.

**3. The obvious fixture for "the fixed algorithm reads only the first row"
cannot fail.** Stating a width on the *same* column in both rows makes the two
builds agree by accident: the first row is walked first either way and
§17.5.2.1's *"a column already given a width keeps it"* discards the second
row's. The injection matrix found it — the defect survived — and the fixture now
states a width on a column the first row left `auto`, which is the only
arrangement in which the two answers differ.

**4. §17.6.2.1's rule 1 was zeroing the width in two places, and only one half
was reachable.** `resolve` returned a `hidden` winner with `width: 0.0` *and*
`Edge::used_width` returned zero for `hidden`. A defect deleting the second
survived every test in the suite because nothing could reach it — this plan's
own *"a rule enforced twice hides the reachable half"*. `resolve` now returns the
winner as it was declared and §8.5.3 is applied in one place, which is the one
that answers *what is drawn* rather than *what won*. The comment that claimed a
build comparing used widths would lose rule 3 to a solid border was wrong for the
same reason — rules 1 and 2 dispose of `hidden` and `none` before rule 3 ever
compares a width — and it is corrected in place rather than deleted.

**5. `border-collapse` and `border-spacing` inherit, and in a document made of
`<table>` elements that is unobservable.** Both are *inherited: yes*, both are
read off the **table** box, and the user-agent sheet declares both on every
`<table>` — HTML's own `border-spacing: 2px` — so a table nested in a table has
a declaration of its own and a declared value beats an inherited one. Every
browser behaves the same way. The first fixture written for it therefore failed;
what has no user-agent rule is a `display: table` that is not a `<table>`, which
is what a stylesheet writes on a `<div>`, and that box inherits. Two fixtures,
one per property, and the two defects are two rows of the matrix rather than one.

**6. A row is the wrong unit of fragmentation and a *band* is the right one.**
A page may break between two rows and may not break across a cell that spans
them, so the item the fragmenter sees is the maximal run of grid rows a `rowspan`
joins. With no `rowspan` in the table every band is one row, which is where a
book's table breaks; with one, the rows it spans move whole.

**7. A page count cannot see whether a table broke *legally*.** With the spacing
between two bands emitted as an edge rather than a margin there is no break
position at all — and §13.3.3's escape then drops rules A to D and cuts anyway,
producing **the same page count** and one warning. The defect survived until the
fixture asserted `BreakForcedPastTheRules` is *absent*, which is milestone 10's
finding arriving in a second place.

**8. Reading order stops being emission order in two more ways.** Milestone 10
found the first: a float is laid out where it is written and drawn where it is
placed. A table adds `<tfoot>`, which HTML 4.01 required to be written *before*
the bodies and which §17.2 renders *after* them; and a row's cells, which sit
beside one another rather than under. Both are survivable for one reason —
`TextRun::order` — and the design that follows from it is that **the cells are
laid out in document order and the rows are emitted in visual order**, which are
two different loops over the same table. Two injected defects, one per loop.

**9. The layout total is three quantities and not one.** `colspan` × `rowspan`
per cell is what a hostile file inflates; grid rows × grid columns is what a
table of two thousand rows whose first one spans two thousand columns inflates
without inflating the first; and §17.5.2.2's distribution over every spanning
cell's span is a third again. Each has a firing fixture that the other two
charges do not fire, and **a nested table multiplies every one of them**: an
outer cell is laid out three times — twice to measure and once to set — so the
same inner table is under the total alone and past it nested. That pair is
`a_nested_table_multiplies_the_work_total` and it is the clearest statement of
what `MAX_LAYOUT_WORK` is for that this workspace has.

### The two-pass algorithm, asserted as an algorithm

§17.5.2.2 computes a minimum and a maximum content width per column **first**,
and distributes the table's width over them **second**. A one-pass
approximation — a share of the available width in proportion to each column's
content — is an ordinary thing to write and agrees with this everywhere except
where a column's minimum is greater than its proportional share.

So the fixture is one where they differ, and it asserts three things rather than
one: the first pass's own output, the second pass's answer, and the one-pass
answer computed in the test file so the difference is a number rather than a
claim.

| | Column A (nine short words) | Column B (one fifty-point word) |
| --- | --- | --- |
| Pass 1, minimum | 10 | 50 |
| Pass 1, maximum | 170 | 50 |
| **Pass 2, at a table width of 110** | **60** | **50** |
| A one-pass share of 110 in proportion to the maximum | 85 | **25 — below its own minimum** |

`table::constraints` and `table::distribute` are two functions with the pass-1
result as a value between them, which is what makes the intermediate assertable
at all; `the_two_pass_widths_are_the_widths_the_cells_get` then asserts the same
numbers on a laid-out page.

### The browser comparison, over a table-heavy document

| | Worst deviation, as a fraction of the browser's column |
| --- | --- |
| **The table-heavy comparison** | **0.0005** |
| The injected defect — `td, th { display: block }` | 0.1245 |
| One line of this column, for scale | 0.016 |
| The cap | 0.02 |

Thirty-two blocks over a document holding a `<caption>`, a `<thead>`, a
`<tbody>`, a table with no `<tbody>` at all, a `colspan`, a `rowspan`, a
`<tfoot>` written before the body it is drawn under, a collapsing-border table
beside a separated one, and a nested table. The **block sequence agrees
exactly**, which is the sharper half: it is an ordered list, both sides walk the
document, and the `<tfoot>` is reported before its `<tbody>` on both — so the
*offsets* are what say it was drawn under it.

Two variables are held fixed beyond milestone 8's face and milestone 10's
`line-height`, and both are named rather than tolerated. **`vertical-align`**:
HTML's own user-agent sheet puts `middle` on a table and `inherit` on a cell, so
every browser centres a short cell in a tall row, and this build has no §17.5.4
at all. Measured without stating it, the fixture disagreed by **0.1070** —
*larger than the injected defect the oracle exists to catch*, which is precisely
the *"oracle whose noise floor is its own defect"* the risk table warns about.
And **the heading size**: the two sides report a line box's top and a baseline
respectively, `deviation` cancels the constant by subtracting the first block,
and that works only while every block shares one — an `h1` at `2em` does not, and
is worth 0.0104 on its own.

The **corpus** comparison is unchanged at **0.0360**, at the same block, and the
0.019 its itemisation attributed to the table is still there. What moved is the
reason — the interval holding the table is now 35 pt *shorter* on this engine's
side rather than being a paragraph of inline text — and it is deliberately not
claimed to be localised: what is left there is a variable the table fixture holds
and the corpus one does not.

### The injection matrix

**Fifty-two injected, fifty-two caught, none survived** — after a first pass of
fifty-one in which six survived and every one of the six was a real gap closed.
The five distinct causes are §1 (a fixture asserting the wrong direction), §3 (a
fixture that could not fail), §4 (a rule enforced twice), §5 (a property nothing
observed) and §7 (a page count that cannot see a rule), and one more: the
`colspan="2 "` fixture, which `str::parse` on a trimmed string reads perfectly
well, so the leading-digits rule it was written for was never exercised. It is
`"2x"` now.

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | 17.2.1 (1): a column's children generate boxes | `a_columns_children_generate_no_boxes` |
| 2 | 17.2.1 (2): every child of a column group is a column | `a_column_groups_non_column_child_generates_no_box` |
| 3 | 17.2.1 (3): white space between two rows is a row | `whitespace_between_two_rows_is_not_a_row_of_its_own` |
| 4 | 17.2.1 (3): the 'if any' clause needs a neighbour on both sides | `whitespace_between_two_rows_is_not_a_row_of_its_own` |
| 5 | 17.2.1 (4): white space ends a misparented run | `whitespace_between_two_misparented_cells_does_not_end_the_run` |
| 6 | 17.2.1 (5): a table's stray child generates no row | `a_stray_cell_outside_a_table_gets_an_anonymous_table`, `a_tables_stray_child_gets_an_anonymous_row` |
| 7 | 17.2.1 (6): a row group's stray child generates no row | `a_row_groups_stray_child_gets_an_anonymous_row` |
| 8 | 17.2.1 (7): a row's stray child generates no cell | `a_rows_stray_child_gets_an_anonymous_cell` |
| 9 | 17.2: each bare row gets a row group of its own | `a_rowspan_keeps_its_rows_on_one_page`, `a_rowspan_pushes_the_next_rows_cells_right`, `a_table_of_bare_rows_gets_the_row_group_the_book_left_out` |
| 10 | 17.2.1 (9): a misparented internal box gets no anonymous table | `a_stray_cell_outside_a_table_gets_an_anonymous_table`, `whitespace_between_two_misparented_cells_does_not_end_the_run` |
| 11 | 17.5: colspan is ignored | `a_colspan_takes_the_slots_it_says`, `a_hostile_colspan_is_refused_by_the_work_total`, `a_nested_table_multiplies_the_work_total` |
| 12 | 17.5: rowspan is ignored | `a_rowspan_keeps_its_rows_on_one_page`, `a_rowspan_pushes_the_next_rows_cells_right`, `a_rowspan_of_zero_reaches_the_end_of_its_row_group` |
| 13 | 17.5: rowspan=0 is clamped to one instead of the group | `a_rowspan_of_zero_reaches_the_end_of_its_row_group` |
| 14 | 17.5: a rowspan past its group is not clamped to it | `a_rowspan_past_its_row_group_is_clamped_and_says_so` |
| 15 | 17.5: a cell is placed without skipping occupied slots | `a_rowspan_of_zero_reaches_the_end_of_its_row_group`, `a_rowspan_pushes_the_next_rows_cells_right` |
| 16 | 17.5.2.2: one pass, in proportion to the maximum | `table_layout_fixed_with_an_auto_width_uses_the_automatic_algorithm`, `the_automatic_algorithm_is_two_pass_and_a_one_pass_answer_differs`, `the_two_pass_widths_are_the_widths_the_cells_get` |
| 17 | 17.5.2.2: a spanning cell is applied in the first pass | `a_spanning_cell_raises_its_columns_after_the_single_ones` |
| 18 | 17.5.2.2: an auto-width table fills its containing block | `an_auto_width_table_takes_its_maximum_and_not_the_measure`, `border_collapse_ignores_border_spacing`, `border_spacing_is_two_directions` |
| 19 | 17.5.2.2: a table wider than its maximum does not share the surplus | `a_colspan_takes_the_slots_it_says` |
| 20 | 17.5.2.2: the minimum trial is run at the maximum measure | `a_cells_background_fills_its_whole_row`, `a_row_taller_than_a_page_is_drawn_and_says_so`, `table_layout_fixed_with_an_auto_width_uses_the_automatic_algorithm` |
| 21 | 17.5.2.1: fixed is used even when the width is auto | `table_layout_fixed_with_an_auto_width_uses_the_automatic_algorithm` |
| 22 | 17.5.2.1: every row's widths are read, not only the first | `the_fixed_algorithm_reads_the_first_row_and_ignores_the_rest` |
| 23 | 17.5.2.1: a first-row cell's width beats its column's | `a_columns_width_beats_the_first_rows_cell` |
| 24 | 17.6.1: the spacing is between the cells and not at the edges | `border_spacing_is_between_the_cells_and_at_the_edges` |
| 25 | 17.6.1: one spacing number serves both directions | `border_spacing_is_two_directions` |
| 26 | 17.6.2: border-spacing is honoured under collapse | `border_collapse_ignores_border_spacing` |
| 27 | 17.6.2: a collapsed border is drawn whole by both cells | `a_collapsed_border_is_shared_between_the_cells_beside_it`, `two_adjacent_borders_collapse_into_one` |
| 28 | 17.6.2.1 rule 1: hidden does not beat a wider border | `a_hidden_border_beats_a_wider_one`, `a_hidden_border_leaves_no_ink_where_it_won` |
| 29 | 17.6.2.1 rule 1: the winner keeps hidden's stated width | `a_hidden_border_beats_a_wider_one`, `a_hidden_border_leaves_no_ink_where_it_won` |
| 30 | 17.6.2.1 rule 2: none is compared on its width | `a_none_border_loses_to_a_narrower_one` |
| 31 | 17.6.2.1 rule 3: the narrower border wins | `the_wider_border_wins` |
| 32 | 17.6.2.1 rule 4: double does not outrank solid | `at_equal_widths_the_style_order_decides` |
| 33 | 17.6.2.1 rule 5: the table beats the cell | `at_equal_widths_and_styles_the_box_decides` |
| 34 | 17: a rowspan does not join its rows into one band | `a_rowspan_keeps_its_rows_on_one_page` |
| 35 | 13.3.3: the spacing between two bands is not a break position | `a_table_breaks_between_its_rows` |
| 36 | 17: a band taller than a page says nothing | `a_row_taller_than_a_page_is_drawn_and_says_so` |
| 37 | 17.5.3: a cell's box is its content's height, not its row's | `a_cells_background_fills_its_whole_row` |
| 38 | 17.5.3: a spanning cell's height is applied with the single ones | `the_browser_and_this_engine_lay_the_same_tables_out_the_same_way` |
| 39 | 17.2: the row groups are rendered in document order | `a_footer_group_written_first_is_read_first_and_drawn_last` |
| 40 | 17.2: the cells are laid out in visual order, so the stamps are too | `a_footer_group_written_first_is_read_first_and_drawn_last` |
| 41 | MAX_LAYOUT_WORK: the grid placement is charged one unit a cell | `the_width_distribution_is_charged_as_well_as_the_grid` |
| 42 | MAX_LAYOUT_WORK: the grid itself is not charged | `a_grid_of_many_rows_and_many_columns_is_refused_by_the_work_total`, `the_width_distribution_is_charged_as_well_as_the_grid` |
| 43 | MAX_LAYOUT_WORK: the width distribution is not charged | `the_width_distribution_is_charged_as_well_as_the_grid` |
| 44 | 17.5: colspan and rowspan never leave the markup | `the_browser_and_this_engine_lay_the_same_tables_out_the_same_way`, `a_colspan_with_trailing_rubbish_is_its_leading_digits`, `a_rowspan_of_zero_reaches_the_end_of_its_row_group` |
| 45 | HTML: an attribute is parsed with str::parse rather than by its digits | `a_colspan_with_trailing_rubbish_is_its_leading_digits` |
| 46 | HTML: rowspan=0 is clamped away at the door | `a_rowspan_of_zero_reaches_the_end_of_its_row_group` |
| 47 | 15.3.8: the user-agent sheet's border-spacing is gone | `the_user_agent_sheet_carries_htmls_own_border_spacing` |
| 48 | 15.3.8: the user-agent sheet's cell padding is gone | `the_user_agent_sheet_carries_htmls_own_cell_padding` |
| 49 | 15.3.8: a <td> is a block rather than a cell | `the_table_comparison_notices_a_cell_that_was_not_a_cell`, `the_browser_and_this_engine_lay_the_same_column_out_the_same_way`, `the_browser_and_this_engine_fragment_the_same_document_into_the_same_pages` |
| 50 | 17.6: border-collapse does not inherit | `a_table_that_is_not_a_table_element_inherits_its_border_collapse` |
| 51 | 17.6.1: border-spacing does not inherit | `a_table_that_is_not_a_table_element_inherits_its_border_spacing` |
| 52 | 17.6.1: border-spacing takes one length and copies it | `border_spacing_takes_one_length_or_two_and_they_are_two_directions` |

#### The harness

`inject_m11.py` and `defects_m11.py` in the scratchpad are milestone 10's, and
the docstring now names a **fourth** harness bug for the next milestone to
inherit: `pathlib.Path.write_text` on Windows translates `\n` to `\r\n`, so
every file a milestone edits with a patch script is CRLF in the working copy —
invisible in `git diff`, because `.gitattributes` says `eol=lf`, and fatal to the
harness, because `verify_all` reads with universal newlines and `apply` reads
bytes. The two disagreed, `verify_all` reported *"all 51 anchors match exactly
once"*, and the run died on the first defect. Normalising the working copy to LF
before a campaign is the fix; the note is in the docstring beside milestone 3's
`shutil.copy2`, milestone 4's unrevertable empty-string replacement and
milestone 8's mtime-restoring restore.

Every anchor was read out of the file it names **after `cargo fmt`** and all
fifty-two matched exactly once.

### Still owed, and what was narrowed

- **A band taller than a page is drawn past the page bottom rather than
  sliced.** This is the staged half of table fragmentation and the row is
  amended in place with the argument. Breaking *between* bands is built and is
  what a real book's table needs; slicing one — every cell cut at the same height
  and continued on the next page — is `css-break-3`'s. `TableRowTallerThanPage`
  names it, and nothing is lost either way.
- **`<thead>` is not repeated on each page of a table that spans several.**
  §17.5.1 says a header and footer group *"may be repeated on each page"*; this
  build draws each exactly once.
- **`vertical-align` is not implemented, and on a table it is §17.5.4.** Every
  cell is set from its own content top. It is the **largest single gap the
  committed corpus measures** — thirty-four elements — it is the compile-time
  proof's injected property now that `border-collapse` is implemented, and it is
  worth 0.1070 of the browser comparison's column when it is not held fixed.
- **`caption-side` is `Unsupported` by name**, so every caption is set above its
  table and one asked for at the bottom is a reported gap rather than a caption
  in the wrong place. `empty-cells` is unsupported for the same reason.
- **CAPMIN is zero.** §17.5.2.2's caption minimum cannot widen a table here,
  because captions are laid out at the *containing block's* width before the
  table's own is known — which is also what keeps a caption's reading-order
  stamps ahead of the cells'. HTML requires `<caption>` to be a table's first
  element child, so document order and this order agree for every conforming
  book.
- **A column box's background and borders are not painted.** §17.5.1's six
  rendering layers are not implemented; a `<col>`'s `width` is read, which is
  §17.5.2.1's second source, and `ColumnBoxNotPainted` names the rest.
- **`display: inline-table` is `BadValue` by name.** It is an inline-level table
  and this build has no inline-level box that is not text; mapping it onto
  `table` would put a table on a line of its own and look entirely reasonable.
- **A spanning cell's excess width is shared in proportion to the columns'
  maxima**, which CSS 2.2 does not specify — it says only *"should be
  increased"* — and is what every browser does. The note is in `constraints`
  because a reader is entitled to know which sentence is the specification's.
- **A collapsed border at the table's outer edge is drawn whole *inside* the
  table box** rather than centred on the grid line, which would put half of it
  outside. The ink is the same width and the table is half a border narrower
  than a browser's.
- **§17.2.1's rules 1 and 2 destroy text, and they are unreachable from valid
  markup.** A `table-column`'s children generate no boxes at all, so a `<col>`
  holding a word loses it — and text conservation would report it as missing.
  It cannot arise from a real book: HTML makes `<col>` a void element and lets
  `<colgroup>` hold nothing but `<col>`, so both rules are reachable only from a
  caller-built box tree. Named here rather than left for a fuzz corpus to find.
- **The layout fuzz target does not generate tables.** Its structured generator
  picks from four `display` values and none of them is one of the nine, so the
  whole of §17 is unfuzzed. Adding them needs the target's `expected()` to model
  the two rules above — the one legitimate way §17 loses text — which is
  milestone 13's, where the fuzz work is.
- Milestone 10's list is otherwise unchanged: the fetched corpus is unset on this
  machine, WOFF is refused, `local()` is unavailable, `unicode-range` is unread,
  a float met inside a paragraph floats to the top of it, `overflow` establishes
  no formatting context, and `mutool` has never been run over an EPUB.


## Progress — 21 August 2026, milestone 12

**Ten flexbox properties, two pagination rules, and a comparison against Chrome
that agrees to 0.0000 across the measure.** `css-flexbox-1` is laid out rather
than named: §5.1's four directions, §5.2's three wraps, §5.4's `order`,
§7's `flex` shorthand and its three components, §8.2's six distributions,
§8.3's five alignments with its per-item override, §8.4's line distribution,
§9.2's base sizing, §9.3's line breaking, §9.7's flexible-length **loop** and
§9.4 step 11's stretch — each with a fixture that fails when that step alone
is wrong. Beside it, EPUB 3.3 §8.2's fixed-layout renditions: `rendition:layout`
in the package and in the itemref, §8.2.2.6's viewport, EPUB RS 3.3 §8.1's
*"exactly one page per spine itemref"* and §8.1.2's initial containing block
with what falls outside it clipped. **2 780 tests, 7 ignored**, up from 2 722 at
milestone 11.

`display` has sixteen values where it had fourteen; `tinker-pdf-css` implements
seventy-six property names where it implemented sixty-five, and
`UNSUPPORTED_PROPERTIES` is eleven names shorter.

### What the design got wrong, and how it found out

**1. Every oracle in this repository compared *y* offsets, and `justify-content`
does not move anything in *y*.** The browser oracle has run over four
specifications — margin collapsing, floats, §17, and the corpus chapter —
and every one of them compared the shape of a **column**: a list of block boxes
and how far down each one sits. That is the right measurement for every
specification the plan had reached, because all of them put their boxes one
*under* another. §8.2 puts them one *beside* another.

The consequence is neither subtle nor hypothetical.
`the_flex_comparison_notices_an_ignored_justify_content` replaces every
`justify-content` declaration in the fixture with its initial value, leaves the
flex layout otherwise intact, and measures **0.6333 across the measure and
0.0004 down the column** — the defect is two thirds of the page wide and
*entirely invisible* to the oracle as it stood. `Block::left` was added for it,
`flex_deviation` returns two numbers rather than one, and that control is the
test which says a *y*-only oracle would have passed a build that ignored the
property.

**2. `flex: 1` and `flex-grow: 1` are different declarations, and the difference
is the whole of what a three-column layout is.** §7.2's shorthand gives an
omitted `flex-basis` a specified value of **`0%`**, where the longhand's initial
value is `auto`. With `auto` an item is sized to its content and then grown, so
three items of different content lengths end up three different widths; with
`0%` the line is shared out in proportion to the factors and they end up equal.
A build that expanded the shorthand to its longhands' initial values — which
is what every other shorthand in this crate does, and what `flex-flow` two
functions away correctly does — gets the first. It is the one shorthand in the
file whose omitted component is not its longhand's initial value, and it is
written out as a paragraph where the function is because it reads like a bug.

**3. §9.7 is a loop, and the one-pass answer differs by the whole of the
second item's overflow.** Distributing the free space once and clamping the
answers to each item's minimum loses the space the clamped items gave back. Two
items at `flex-shrink: 1` and a base size of 100 in a container of 100: one pass
gives 50 and 50, clamps the first to its 90-pixel minimum, and stops at 140 in a
container of 100. The specification freezes the item that violated its minimum
and **redistributes**, which gives 90 and 10. `resolve` is asserted at that
arrangement directly rather than through a page, because a page would agree with
either answer to within a line.

**4. The comment explaining why §9.7 step 2 exists was wrong, and the injection
matrix said so twice before the right answer appeared.** Step 2 freezes the
items whose flex factor is zero and the items already on the wrong side of their
hypothetical size. The first draft's comment said this stops a zero-factor item
absorbing a share of the distribution — which is false: a zero factor divides
to a zero share, and step 4's loop clamps by the same minimum step 2's
hypothetical size already encodes, so **deleting the freeze changes nothing** in
every ordinary arrangement. Both halves survived the first pass and both
survived the second, and the argument for calling them equivalent mutants was
already drafted.

It was wrong. `initial_free` is computed **once**, in step 3, out of the
frozen/unfrozen split as it then stood: a frozen item contributes its
hypothetical size and an unfrozen one contributes its base size, and those
differ exactly when a minimum bit. Step 4b then multiplies that number by the
flex factors when they sum to **less than one**, and the two answers part
company — 75 against 100 for the growing half, 77.5 against 87.5 for the
shrinking one. `step_two_freezes_before_step_three_measures_the_free_space` is
one arrangement per half, and the comment is corrected in place.

**5. §4.5's automatic minimum size is the clause that decides whether a real
book's flex row overflows, and it is the easiest one to skip.** A flex item's
`min-width: auto` resolves to its content-based minimum, so `flex-shrink` cannot
take an item below its longest word — *unless* the item states a size, which
§4.5 makes the clamp on the clamp. It is deliberately **not** neutralised in
the browser fixture: stating `min-width: 0` on the items would switch it off on
both sides and the shrinking container would then agree for the wrong reason.

**6. Fixed layout is a second pagination rule and it contradicts milestone 4's
premise — both have to hold.** `OpenOptions`'s own documentation says *"for a
reflowable EPUB the page count is a function of these numbers and is not a
property of the file"*. EPUB RS 3.3 §8.1 says a pre-paginated content document
is *"exactly one page per spine itemref"*. Neither is a special case of the
other, and `reflowable_paginates_by_the_box_and_pre_paginated_by_the_spine`
opens the **same three content documents** four times — reflowable and
pre-paginated, at two page boxes each — so the two rules are asserted against
each other rather than one at a time.

The structural consequence is that the page box stopped being a property of a
book and became a property of a **chapter**: §8.2.2.6 puts the dimensions in
the *content* document, one `<meta name="viewport">` per spine item, so two
items of one book may legitimately be two different sizes, and
`two_fixed_chapters_may_be_two_different_page_sizes` asserts three.

**7. The spine rule was enforced twice and only one half was reachable.**
`Chapter::page_count` returned 1 for a fixed chapter *and* the layout truncated
a fixed chapter's pages to the first. The matrix deleted the first and nothing
failed, because the truncation had already made the vector one page long. That
is milestone 11's own finding met again — *a rule enforced twice hides the
reachable half* — and the answer is the same: the sentence lives where the
pages are decided, `page_count` answers only what it is holding, and the
truncation has an injected defect of its own.

**8. §3's *"`float` does not create floating for flex items"* needed no code at
all**, and the code that was there could not be reached. A float is placed by
`Builder::children` when a block container walks its children; a flex item never
goes through that function, because the driver hands each item straight to
`sublayout`, which establishes a formatting context with an empty float set. The
two assignments zeroing `float` and `clear` were deleted, the fixture stayed,
and it now asserts the behaviour rather than the assignment.

**9. A fixed-layout document has to be *cascaded* against its own viewport, and
the viewport is inside the document.** `@media (max-width: 500px)` in a
pre-paginated book is a question about that document's initial containing block
and not about the reading system's page. The dimensions are not knowable until
the markup has been read, so the substitution happens inside `read_document`
after the tree exists rather than at the caller — a caller could only supply
them by parsing the markup a second time.
`media_queries_in_a_fixed_document_are_about_its_viewport` opens two books whose
only difference is the viewport, at one caller page box wider than the query's
threshold, and asserts they disagree.

**10. §8.1.2's clipping is two mechanisms and a test for one is not a test for
the other.** Vertically it is pagination's answer: the second page a reflowable
document would get does not exist, so the characters are dropped — and
*counted*, because they are gone from `Page::text()` as well as from the
picture, which is text conservation's business.
`ArchiveWarning::FixedLayoutContentClipped` carries the number. Horizontally
nothing about pagination can see the problem at all: a box wider than the
viewport is on the page it belongs to at an `x` past its right edge, and only a
clip path in the content stream stops it being drawn.
`a_fixed_page_clips_to_its_initial_containing_block` reads the operators out of
the stream through this repository's own reader, and its control is a reflowable
page with no clip in it.

**11. §5.2's `wrap-reverse` has two consequences and applying the flip once
gets one of them.** It stacks the *lines* the other way **and** exchanges what
`align-items: flex-start` means inside each line. The first draft flipped the
keyword as well as the line position, which double-flips and is wrong for
`baseline` and `stretch`; the shipped design computes every cross-axis offset
relative to cross-start and applies `cross_position` **twice** — once for a
line inside the container and once for an item inside its line — so the
keyword keeps its meaning and the coordinate system moves, which is what §5.2
actually says.

### The injection matrix

**Seventy-three injections in three passes, fifty-two caught on first
presentation, and one recorded as an equivalent mutant.** The three passes are
listed because the second and third are where the milestone's own argument was
tested: the first pass' sixteen survivors were closed with twelve new fixtures,
two deleted rules and — after a wrong equivalence argument was written down
and then disproved — one more fixture covering two of them.

| # | Defect | Caught by |
| --- | --- | --- |
| 1 | §5.4: the order sort is unstable | `a_flex_container_puts_its_items_on_one_line` and two more |
| 2 | §9.3: `nowrap` wraps like `wrap` | `a_flex_line_always_takes_one_item`, `flex_wrap_is_the_difference_between_overflowing_and_a_second_line` |
| 3 | §9.3: a line may be collected empty | `a_flex_line_always_takes_one_item` |
| 4 | §9.7 step 1 always grows | `flex_shrink_is_scaled_by_the_base_size` |
| 5 | §9.7 step 2 does not freeze a zero factor | **survived twice** — closed by `step_two_freezes_before_step_three_measures_the_free_space`, see finding 4 |
| 6 | §9.7 step 2 does not freeze the wrong side | **survived twice** — closed the same way |
| 7 | §9.7: shrinking uses the raw factor | `flex_shrink_is_scaled_by_the_base_size` |
| 8 | §9.7: the shrink share is not scaled by the base size | `flex_shrink_is_scaled_by_the_base_size` |
| 9 | §9.7 step 4b drops the factors-below-one clause | `flex_factors_below_one_leave_the_rest_of_the_space_empty` |
| 10 | §9.7 step 4 is one pass rather than a loop | `the_flexible_length_resolution_redistributes_after_a_minimum_bites` |
| 11 | §8.2: `space-between` divides by the item count | `justify_content_puts_the_line_where_it_says` |
| 12 | §8.2: `space-around` has no half share at the ends | `justify_content_puts_the_line_where_it_says` |
| 13 | §8.2: `space-evenly` is `space-around` | `justify_content_puts_the_line_where_it_says` |
| 14 | §9.3 fallback: `space-between` does not fall back | **survived** — closed by `an_overflowing_line_falls_back_to_a_different_alignment` |
| 15 | §9.3 fallback: `space-around` does not fall back | added in the second pass; caught by the same fixture |
| 16 | §8.3: `align-items: flex-end` is `flex-start` | `align_items_puts_a_short_item_where_it_says` |
| 17 | §8.3: `align-items: center` does not halve | `align_items_puts_a_short_item_where_it_says` |
| 18 | §8.3: `align-items: baseline` is `flex-start` | **survived** — closed by `align_items_baseline_lines_the_text_up` |
| 19 | §8.3: the line's baseline is not the largest ascent | added in the second pass, **survived**, closed by the same fixture's second assertion |
| 20 | §8.4: `align-content: stretch` does not stretch | **survived** — closed by `align_content_stretch_makes_the_lines_taller` |
| 21 | §8.3: `align-self` does not override `align-items` | `align_self_overrides_the_containers_align_items` |
| 22 | §5.1: `row-reverse` is `row` | `row_reverse_puts_the_first_item_last` |
| 23 | §5.2: `wrap-reverse` is `wrap` | `wrap_reverse_stacks_the_lines_upwards` |
| 24 | §3: `float` applies to a flex item | **survived** — the rule was unreachable and is deleted, see finding 8 |
| 25 | §4: an `inline` item is not blockified | **survived twice — an equivalent mutant, recorded** in `FlexPass::apply`: `Builder::block` reads `display` to ask four questions and an inline item answers all four the way a block one does |
| 26 | §4: an `inline-flex` item is not blockified | added in the second pass; caught by `an_inline_flex_item_is_blockified_and_does_not_warn` |
| 27 | `box-sizing` is ignored on a flex base size | **survived** — closed by `box_sizing_border_box_shrinks_a_flex_basis_by_its_padding` |
| 28 | §9.2: `flex-basis` is read after `width` | `flex_basis_beats_the_width_property` |
| 29 | §4.5: the minimum is not clamped by the stated size | **survived** — closed by `the_automatic_minimum_is_clamped_by_a_stated_size` |
| 30 | §9.2 step 4: the hypothetical size is not clamped | **survived** — closed by `the_hypothetical_size_decides_where_a_line_wraps` |
| 31 | §5.4: the items are laid out in order-modified order | `order_moves_the_boxes_and_not_the_reading_order`, `a_flex_container_conserves_its_text` |
| 32 | §9.4 step 8: a single line ignores the container cross size | `align_content_moves_the_lines_and_only_when_there_are_two` |
| 33 | §9.4 step 11 does not stretch | `stretch_makes_an_item_as_tall_as_its_line` |
| 34 | §5.2: the lines are emitted in line order, not physical order | `wrap_reverse_stacks_the_lines_upwards` |
| 35 | §4: a whitespace-only run becomes an anonymous item | **survived** — closed by `a_whitespace_run_is_not_an_item_and_the_spacing_says_so` |
| 36 | §4: a run of child text is not wrapped at all | `a_run_of_child_text_is_an_anonymous_flex_item` |
| 37 | §7.2: the shorthand's omitted basis is `auto` | `the_flex_shorthands_omitted_basis_is_zero_and_not_auto` |
| 38 | §5.3: `flex-flow` does not reset the omitted longhand | `flex_flow_resets_the_longhand_that_was_left_out` |
| 39 | §5.4: `order` refuses the negative integers | `order_takes_the_negative_integers_the_other_integer_reader_refuses` |
| 40 | §7.1: a negative flex factor is accepted | `a_negative_flex_factor_is_malformed_and_not_a_gap` |
| 41 | the flexbox properties inherit | `no_flexbox_property_inherits` |
| 42 | §7.2.3: `flex-basis: auto` computes to zero | **survived** — closed by `flex_basis_computes_to_a_size_and_auto_stays_auto` |
| 43 | §7.2.3: a negative `flex-basis` is not clamped | added in the second pass; caught by the same fixture |
| 44 | RS §8.1: a pre-paginated item paginates by the box | **survived** — the rule was enforced twice and this half was unreachable, see finding 7 |
| 45 | RS §8.1: a fixed chapter is not truncated to one page | added in the second pass; caught by four fixtures |
| 46 | §8.2.2: the itemref property does not override the book | `an_itemrefs_property_overrides_the_books_declaration_both_ways` |
| 47 | §8.2.2: the itemref vocabulary is the metadata one | `an_itemrefs_property_overrides_the_books_declaration_both_ways` |
| 48 | §8.2.1: an unknown `rendition:layout` is silently reflowed | `an_unknown_rendition_layout_is_named_and_reflowed` |
| 49 | §8.2.2.6: `device-width` is read as a viewport | **survived** — the fixture named only one axis; closed by naming both |
| 50 | §8.2.2.6: a fixed document is cascaded against the caller box | `media_queries_in_a_fixed_document_are_about_its_viewport` |
| 51 | RS §8.1.2: the initial containing block is not clipped | `a_fixed_page_clips_to_its_initial_containing_block` |
| 52 | RS §8.1.2: the clipped characters are not counted | `content_below_the_initial_containing_block_is_clipped_and_counted` |
| 53 | §8.2.2.6: a fixed page keeps the reading system margin | **survived** — closed by `a_fixed_page_has_no_reading_system_margin` |
| 54 | §8.2.2.6: a fixed item with no viewport is not named | `a_fixed_item_with_no_viewport_is_named_and_still_one_page` |
| 55 | §8.2.2.6: a fixed chapter is laid into the book box | **survived** — closed by `a_fixed_chapter_is_set_at_its_viewports_measure` |

Rows 5, 6, 14, 15, 18–20, 24–27, 29, 30, 35, 42–45, 49, 53 and 55 are the
survivors and the rows added to chase them; every other row was caught on its
first presentation.

**The one recorded equivalent mutant is row 25**, and its argument is written
where the code is rather than only here: §4 blockifies a flex item's `display`,
and in this build `Builder::block` reads `display` only to ask whether the box
generates no box, is a `list-item`, is a table or is a flex container — an
`inline` or `inline-block` item answers all four the way a `block` one does. The
arm is kept because it is what §4 says; it is recorded as unobservable so a
later reader does not go looking for the fixture.

### The browser oracle, and what is pinned

`the_browser_and_this_engine_lay_the_same_flex_containers_out_the_same_way`
compares **46 block boxes across eighteen flex containers** against Chrome 151,
in two axes.

| | Of the measure (x) | Of the column (y) |
| --- | --- | --- |
| The measured disagreement | **0.0000** | **0.0004** |
| Injected defect: `display: block` on every container | 0.8000 | 0.2068 |
| Injected defect: `justify-content` at its initial value, everything still flexed | 0.6333 | 0.0004 |

The x row of the first line is exact rather than rounded: every item in the
fixture is where Chrome put it to the two decimal places the browser reports.
The cap is **0.02, which is one line of this column** — `line-height: 1.2` at
16 px is 19.2 px of the 984 px the browser's column spans, or 0.0195 — which
is the rule `MAX_TABLE_DEVIATION` was set by one milestone earlier. A cap
tighter than a line would fail the first time Chrome wrapped one item's text one
word differently; a looser one would admit a whole flex line.

**What is pinned, on both sides:**

- **The face.** `SAME_FACE`'s `* { font-family: "Courier New", monospace
  !important }`, milestone 8's variable and the one this build cannot share
  until it embeds a face of its own.
- **`line-height: 1.2`.** This build resolves `normal` as 1.2 and Chrome
  resolves it from Courier New's own metrics as 1.133 — six per cent of every
  line in the document, accumulating down the column, and nothing whatever to do
  with §9.
- **The heading size**, at `1em`. `deviation` cancels the browser's line-box-top
  against this engine's baseline by subtracting the first block; that works only
  while every block shares one constant, and a heading at 2em does not.
- **The paragraph margins inside the items**, at zero, so an item's height is
  its text's.

**What is deliberately *not* pinned, and why:** `min-width`. §4.5's automatic
minimum size is implemented here, it is exactly the clause an implementation
skips, and stating `min-width: 0` on the items would switch it off on **both**
sides — the shrinking container would then agree for the wrong reason. The
table comparison one milestone earlier found the opposite lesson about
`vertical-align`, which measured 0.1070 unpinned and was *larger than the defect
the oracle exists to catch*; the difference is that `vertical-align` is a gap
this build has and §4.5 is a rule it implements, so pinning the first was
honest and pinning the second would be hiding.

### Was a fixed-layout book obtained?

**No, and it is recorded as owed rather than dropped.** Milestone 1's `What is
short` predicted this and the prediction held: neither producer of the committed
six emits `rendition:layout`, `TINKER_EPUB_CORPUS` is unset on this machine, and
the fetched corpus's fixed-layout samples are the CC-BY-SA ones `deny.toml`'s
no-copyleft rule bars. Nothing was bought and nothing was smuggled in.

What stands in its place is **built rather than borrowed**, and the difference
is stated so nobody reads more into it than it says.
`tests/epub_fixed_layout.rs` synthesises its books from the same
`epub_support::ocf_zip` every OCF fixture in this plan uses. That gives complete
control of the arrangement — a book-wide declaration, a per-itemref override
in each direction, three different viewports, a missing viewport, a
`width=device-width` viewport, a misspelt `rendition:layout` — and gives **no**
evidence at all about what a real producer writes. Gap 30 closed with *"no
non-Windows producer"* named for exactly this reason; this is the same sentence
about a different absence.

The concrete consequences, so a later milestone can act on them:

- **No claim is made about how a real fixed-layout book is packaged.** In
  particular this build has never seen `rendition:spread`,
  `rendition:orientation`, `rendition:page-spread-left`/`-right`, or the
  `rendition:` prefix declared through a `prefix` attribute pointing somewhere
  unexpected.
- **The `<meta property="rendition:layout">` reader assumes the OPF namespace
  and a `property` attribute.** Real books also carry the EPUB 2
  `<meta name="…" content="…">` form for other vocabularies, and a
  producer that wrote the rendition vocabulary that way would be silently
  reflowed here — with no warning, because the `<meta>` would match nothing at
  all rather than matching at a value outside the vocabulary.
- **Nothing here has been through epubcheck**, and a fixed-layout book has
  conformance requirements a reflowable one does not.

### Still owed, and what was narrowed

Milestone 11's list stands except where noted, and this milestone adds:

- **`inline-flex` is laid out as a block-level flex container**, warned by name
  as `Warning::InlineFlexAsBlock`. This build has no inline-level box that is
  not text; the only other answer is to set the container as inline text, which
  throws the whole flex layout away. `inline-table` took that other answer one
  milestone earlier, for the opposite reason — a table's contents are nothing
  like a line of text either way, so there was nothing to keep.
- **A flex line is the unit of fragmentation.** A row container may be broken
  between its lines, which is where a real page break in one goes; a `column`
  container is **one** line whatever its length, so a long one is drawn where it
  is and says `Warning::FlexLineTallerThanPage`. `css-break-3`'s fragmentation
  *inside* a line — every item cut at the same height and continued on the next
  page — is not here, and is the same staged half `Warning::TableRowTallerThanPage`
  names.
- **A wrapping `column` container's items are measured at their fit-content
  cross size and stretched to their line's afterwards**, so a multi-line column
  container's main sizes were resolved against a slightly different width from
  the one the items end up at. A single-line column container — which is what
  `flex-direction: column` without `flex-wrap` is, and what a book writes — is
  exact.
- **`baseline` in a `wrap-reverse` container aligns from the flipped
  cross-start**, which is the coordinate system doing the right thing and is not
  asserted against a browser; `css-align-3` arguably makes it a last-baseline
  alignment. No fixture combines the two.
- **`gap`, `row-gap` and `column-gap` stay `Unsupported` by name.** They are
  `css-align-3`'s, the row does not ask for them, and a build that mapped them
  onto margins would put the gap outside the container's edges as well as
  between its items.
- **`min-width`, `max-width`, `min-height` and `max-height` are still
  unimplemented**, so §9.7's max violations are unreachable and §9.2 step 4's
  clamp is a floor rather than a range. The automatic minimum of §4.5 *is*
  implemented, which is the half a book depends on.
- **`rendition:spread`, `rendition:orientation` and the page-spread properties
  are unread**, and an itemref carrying one is not warned about: they are
  §8.2's other vocabulary and a two-page spread is a page-pairing decision
  this build has no concept for.
- **A fixed-layout page is not scaled to the reading system's page.** It is
  emitted at the viewport's own size in points, so a host that wanted every page
  of a mixed book to be one size has to do that itself — which is honest, and
  is the reason `Page::size()` differs between two pages of one document.
