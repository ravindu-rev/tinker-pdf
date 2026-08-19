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
| `MAX_SELECTOR_MATCHES` | **The work cap of the cascade.** Selector-against-element attempts across the whole book | `MAX_CSS_RULES` × `MAX_DOM_NODES` is the product, and **neither factor bounds the other**. This is `5adf502`'s sentence in its purest form and it is the single most important constant in this plan |
| `MAX_DOM_NODES` | Elements admitted from one content document | — (per-document). `MAX_XML_TOKENS` stands in front of it and is a million, so this must sit below that or it can never fire |
| `MAX_BOX_TREE_NODES` | **A work cap.** Boxes across the book | Boxes are not elements: anonymous block generation, `::before`/`::after` and table-structure fixup (CSS 2.2 §17.2.1) each create boxes the document did not write |
| `MAX_LAYOUT_WORK` | **The work cap of layout.** Box-layout operations across the book | A per-box cap is not a total once the file chooses the box count *and* the pass count: automatic table layout is two passes (§17.5.2.2), float placement re-flows a line, shrink-to-fit measures twice, and a nested table multiplies all three |
| `MAX_LINE_BREAK_WORK` | **A work cap.** Break opportunities evaluated across the book | The same shape one level down |
| `MAX_EPUB_MANIFEST_ITEMS` | Manifest items admitted | Must sit **below** `MAX_ZIP_ENTRIES`, or the archive refuses first and this can never fire |
| `MAX_EPUB_SPINE_ITEMS` | Spine itemrefs | Must sit below `MAX_EPUB_MANIFEST_ITEMS` for the same reason |
| `MAX_EPUB_PAGES` | Pages fragmented out of the book | **Deliberately *not* in the relation above**, and this is the trap. Gap 30 found `MAX_XPS_PAGES` was not bounded by `MAX_XPS_PARTS` because four thousand `PageContent` elements may name one part. Here it is worse: **one** spine item of 128 MiB of text fragments into as many pages as its length divided by the page height, so the page count is bounded by *content length* and not by item count at all. **Amended, 19 August 2026, milestone 4: it arrives with milestone 7's fragmentation and not before.** Milestone 4 puts exactly one page on each `<itemref>`, so a cap above `MAX_EPUB_SPINE_ITEMS` could never fire and one below it would be the spine cap under another name — gap 18a milestone 8's failure reached from the direction that writes the constant first, and the argument `bounds_ledger.rs` already carries for `MAX_XPS_VISUAL_DEPTH`'s absence. The sentence to the left is right about the build that fragments and is a constant that cannot fire in the one that does not |
| `MAX_EPUB_FONTS` | Faces admitted from `@font-face` and the manifest | Each costs an embedded font program and a subset |
| `MAX_SYNTHESISED_PDF` | Bytes handed to `CosDocument::open` | Already exists in `cbz.rs`; reused rather than duplicated, and the ledger says so |

Per-item caps sit beside them and the comment on each says in as many words
that it is *not* a work cap, in the register `MAX_SCRIPT_STEPS`,
`MAX_MESH_TRIANGLES`, `tinker-pdf-zip`'s `limits.rs` and gap 30's XPS constants
already use.

**Three deliberately absent, argued where they are declared**, because gap 29
established that writing down why a cap was *not* added is the cheaper half of
this discipline:

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
| 6 | **`tinker-pdf-css`, the ninth leaf** | `css-syntax-3`'s tokenizer and the qualified-rule/at-rule grammar, **with its normative error recovery** — a malformed declaration discarded to the next semicolon, a malformed rule to the next block, and the count of each reported rather than swallowed; `selectors-4`'s type, universal, class, id, attribute (§6.1–§6.4) and the four combinators (§14), with **specificity per §15 asserted against a table of at least twenty selectors including the cases that trip a naive A/B/C** — `:not()`'s argument, `:is()`'s most-specific-argument rule, and a pseudo-element's C contribution; matching against a caller-supplied `Element` trait, so ruling 8 holds and no XHTML vocabulary is in the public API; the **whole** of `css-cascade-5` §6.1's sorting order including the `!important` origin reversal, with a fixture per criterion; inheritance as a single top-down pass over computed values (§7.2), with a test that a lazy resolution and this one agree — and a note saying the lazy one is quadratic; `@import` with the depth cap and a **cycle refused rather than recursed**; `@media` evaluated against a plain `MediaContext`, **as `screen`**, with the decision's argument in the module header; **decision 5's `Known`/`Unsupported`/`Unknown` split, and a compile-time proof that a property with no consumer does not build** — injected as a defect and asserted to fail the build, not a test; `@layer` refused by name; every bound firing by its own refusal; `xtask -- dag` green with the fifth amendment's argument; the **twenty-second** fuzz target; `deny.toml` gains the CSS and HTML names | L |
| 7 | **`tinker-pdf-layout`, the tenth leaf** | The box model (`css-box-3`) with `box-sizing`, and **margin collapsing**, which is the rule a first implementation omits and whose omission moves every block on every page; block and inline formatting contexts (CSS 2.2 §9.4.1, §9.4.2) and line boxes; `css-text-3` §4.1.1 and §4.1.2's white-space processing in both phases, asserted against a fixture whose source is indented the way milestone 1's real books are; **UAX #14 line breaking over the vendored UCD tables**, with `css-text-3` §5.5's required class behaviour for `WJ`, `ZW`, `GL` and `ZWJ`, §5.1's four strictness levels and §5.4's `overflow-wrap` — and a **CJK fixture**, because a space-only breaker passes every English test ever written; §6's alignment and justification; fragmentation into pages, honouring CSS 2.2 §13.3.1's properties, §13.3.2's `orphans` and `widows` and **§13.3.3's rules A to D for where a break is permitted at all**, with `page-break-before` and `page-break-after` asserted because they appear in all six measured books; the `Metrics` trait, so nothing here depends on `font`; whether `math` is needed answered in the `As built` and the DAG edge dropped if it is not; the **twenty-third** fuzz target, over a structured generator; `deny.toml` gains the layout and line-breaking names | L |
| 8 | **The first book that reads** | XHTML through `tinker-pdf-xml`'s new mode into an element tree; **a committed UA stylesheet**, parsed by milestone 6's parser and cascaded like an author's, with a test that removing it produces an undifferentiated book — so its absence is visible rather than merely worse; the cascade over the tree, layout, fragmentation and synthesis into a `CosDocument`; **every book in the committed corpus opens, paginates and passes text conservation**, with the conservation figure recorded per book; `Page::text()` returns the words in reading order; cross-references between spine items reach the page as milestone 5's link annotations, and the navigation document as the outline; qpdf clean; **the browser oracle stands up here** — ruling 9 amended in writing with its argument, the continuous `y`-offset comparison built with a UA sheet injected on both sides, the paginated `--print-to-pdf` comparison beside it, and the job red when the browser is missing; the `Unsupported` census printed per book, which is the number this milestone is actually judged on | L |
| 9 | **Fonts** | `@font-face` (`css-fonts-4` §4.1) and the `src` descriptor (§4.3) with `format()` and the fallback list; the font matching algorithm (§5) including **per-character fallback** (§2.1, §5.3), with a fixture whose one run needs three faces and becomes three PDF text objects; **SHA-1 in `tinker-pdf-crypto`**, pinned against published vectors, with a second implementation written a different way asserted to agree over every length up to two blocks — gap 29's CRC-32 discipline, because a hash written wrong is self-consistently wrong; **both de-obfuscations, asserted on the de-obfuscated bytes and not on a page that drew** — IDPF's SHA-1 key over 1 040 bytes and Adobe's 16-byte UUID key over 1 024, each from a fixture built for it, with the whitespace-stripping of §4.4.3 proved by an identifier that has some; WOFF and WOFF2 refused **by name**; a character no available face covers producing a named warning rather than a blank; `FontProvider`'s per-family fallback question answered — the trait extended, or the reason it is not recorded; the generic families' standard-14 metrics asserted to make pagination independent of whether a provider is attached | M |
| 10 | **Floats and `clear`** | CSS 2.2 §9.5.1's **nine numbered constraints, each with its own fixture**, because they are a set and an implementation that satisfies eight produces a page that looks right on the ninth's absence; §9.5.2's `clear` and clearance; float interaction with line boxes — a line box shortened beside a float and restored below it; a float taller than its containing block; two floats that do not fit side by side; **a float that would fall off the page bottom**, which is the fragmentation interaction and the one that loses text; text conservation asserted across every float fixture, since a lost float is a lost paragraph; the browser comparison run over a float-heavy content document and its `y`-offset agreement recorded as a number | M |
| 11 | **Tables** | CSS 2.2 §17.2's model and **§17.2.1's anonymous table objects**, which is the fixup a real book needs because HTML tables in the wild omit `<tbody>`; §17.5.2.1's fixed layout and §17.5.2.2's automatic layout, the latter asserted to be the two-pass algorithm the spec describes rather than a one-pass approximation; §17.6.1's separated model and §17.6.2's collapsing model with §17.6.2.1's conflict resolution; `colspan` and `rowspan`; a nested table, since it multiplies the layout work cap; **table fragmentation across a page boundary** — or, if it is staged, the row amended in place with its argument, in the shape gap 30's milestone 8 amended its own | L |
| 12 | **Flexbox, and fixed-layout renditions** | `css-flexbox-1`: `display: flex` and `inline-flex`, `flex-direction`, `flex-wrap`, the `flex` shorthand and its three components, `justify-content`, `align-items`, `align-self`, `align-content` and `order`, each with a fixture; the flex layout algorithm's line-breaking and free-space distribution asserted against the browser oracle rather than against arithmetic done twice; **fixed-layout renditions** (§8.2) — `rendition:layout: pre-paginated`, which EPUB RS 3.3 §8.1 makes *"exactly one page per spine itemref"*, §8.2.2.6's content-document dimensions from the viewport meta, and the initial containing block of RS §8.1.2 with content outside it clipped; a fixed-layout book from milestone 1's corpus if one can be obtained, and **recorded as owed rather than quietly dropped** if not — gap 30's own shortfall, named so it is not repeated by accident | M |
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

