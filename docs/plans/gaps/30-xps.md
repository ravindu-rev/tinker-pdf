# An XPS opens today, and the page it shows is one of its resources

An XPS document is an OPC package — a ZIP holding `[Content_Types].xml`, a
relationship graph under `_rels/`, and a `FixedDocumentSequence` naming
`FixedDocument`s naming `FixedPage`s whose markup is a XAML dialect. It is the
second of the three formats [28](28-tinker-integration-decisions.md) decided
are built here, and it is the one where the container is the smallest part of
the problem. When this is done, an `.xps` opens as a `Document` whose pages are
its fixed pages, at the size the markup states, drawn from the markup rather
than from whatever raster resources happen to be lying beside it. (L)

**This is the second of the three plans gap 28 promises.** That document says,
at the end of its option D section: *"Three new gap plans will be written for
them — CBZ, XPS and EPUB — after this gap closes."* [29](29-cbz.md) closed at
`b764917` and is the first; EPUB (XL+) is the third and is not planned here.
Gap 28 sizes this one **L**, and this plan does not dispute the size — but it
does dispute the reason, which is the first of the corrections below.

## Which rule governs this, since it is not ruling 3

Ruling 3 schedules deferred capabilities by corpus hit-rate, and both
[10](10-mesh-shadings.md) and [18a](18a-jpx-decoder.md) had to argue over it in
writing before they could be built. [29](29-cbz.md) had to say in as many words
that a container format is outside its remit, and the same paragraph is owed
here rather than left for a reader to reconstruct.

Ruling 3 binds [02](../02-filters.md), [08](../08-rendering-device.md) and the
master plan's descope levers, and what it schedules is a **`Capability`** — a
codec inside a PDF that the engine defers behind a flag, degrades with a
placeholder under ruling 2, and builds when the nightly hit-rate report says
real documents need it. XPS is none of those things. It is a document format,
not a codec; it produces no `Capability` variant; and no PDF in any corpus will
ever contain one, so a corpus hit-rate for it is not a number that can exist.
[23](23-corpus-runner.md) measured 4 525 PDFs and could not have measured this
if it had run for a year.

What governs it is an **owner decision, dated 16 August 2026, recorded in plan
15 where the options used to be** and summarised in gap 28's `As built`. That
is the same authority [27](27-form-calculations-decision.md) and
[29](29-cbz.md) were built under. As gap 29 notes, plan 15's own answer
mis-cites the rule that keeps the parsers ours: it is **CONTRIBUTING rule 1**,
backed by `deny.toml`, and not ruling 3. This plan cites rule 1 for the XML
parser for the same reason.

## What is wrong

Not a missing feature this time. **An `.xps` opens today, and what comes back
is a plausible wrong document.**

Measured on this machine, before a line of this plan's code exists, against two
packages written by Microsoft's own XPS serialiser (see
[A real `.xps`](#a-real-xps-and-gap-29s-largest-debt)):

- A one-page 816 × 1056 XPS carrying a 4 × 4 PNG resource and a line of text
  **opens**. `tpdf info` reports `pages 1` and `first page 4 x 4 pt`. The page
  is the *resource*, at the resource's pixel size. The page dimensions, the
  `<Glyphs>` run, the ODTTF font and the fixed-page markup are all discarded,
  and **there is no warning**, because from `cbz.rs`'s point of view nothing
  went wrong: it found one image entry and paged it.
- The same document with no raster resource at all is refused as
  `OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)` — *"it is a valid
  archive and not one entry produced a page"* — said about a document that has
  a page.

The first is the serious one, and it is this repository's own named failure
mode: gap 17's blank page returned as success and gap 18a's plausible
photograph, arriving in the facade. A host asks `page_count()`, gets 1, renders
it, and shows a piece of the document presented as the document. A ten-page
report with sixty image resources opens as a **sixty-page book in filename
order**.

**Gap 29 did not cause this; it inherited it.** Before gap 29 the answer was
`NotAPdf`, which was at least honest, and gap 28's deletion checklist would
have taken MuPDF's `Doc::Other` route with it either way. What changed is that
`Document::open` now recognises the *container* and assumes the *format*, and
one ZIP signature covers CBZ, XPS, EPUB, ODF, OOXML and every JAR ever built.
That is why the discrimination in
[Telling the two ZIPs apart](#telling-the-two-zips-apart) is milestone 3's
first exit criterion rather than a detail, and why it is the one part of this
plan that improves matters even if nothing else in it is built.

## The three decisions taken during planning

Each is put here, before any file exists, because each is the kind that
otherwise gets made implicitly by whoever writes the first module — the failure
gap 18's risk table named for the fixed-point width and the reason
[18a](18a-jpx-decoder.md) exists as a separate document.

**1. An XPS becomes a `Document` by synthesising a PDF at open**, as a CBZ
does — *not* by emitting to the existing `Device` trait, which is the
genuinely attractive alternative CBZ never had. The argument is in
[Design](#the-seam-synthesis-again-and-a-different-argument-for-it) and it is
not gap 29's argument.

**2. The XML parser is a leaf crate of its own, `tinker-pdf-xml`**, and it
refuses DTD content outright rather than bounding entity expansion. See
[An XML parser does not exist here](#an-xml-parser-does-not-exist-here).

**3. Both dialects are read.** ECMA-388's `http://schemas.openxps.org/oxps/v1.0`
and XPS 1.0's `http://schemas.microsoft.com/xps/2005/06` are one vocabulary
with two spellings, and a reader that takes only the standard's namespace
refuses every file Windows writes. See
[Two dialects, one reader](#two-dialects-one-reader).

## Three corrections, made while this was written

**Gap 28's sizing sentence is right about the size and wrong about the
reason.** It says XPS is L because it is *"an OPC ZIP, a hand-rolled XML
parser, and fixed-page markup that maps closely onto the path, glyph and brush
calls the `Device` seam already has."* The `Device` seam is the **reader**
side, and this plan writes to the **writer** side, which has none of those
calls: `DocumentBuilder` can emit a rectangle fill, an image placement, a text
run in a WinAnsi simple font and a fill colour, and nothing else. No
`/ExtGState`, so no opacity and no soft mask; no `/Shading` and no `/Pattern`,
so no gradient and no tiling brush; no Type0 font, so no glyph addressed by
index — which is how XPS addresses every glyph it means precisely. Milestone 5
exists because of that, and a good part of the L lives there. The size stands;
the sentence is amended in gap 28 and in the README row.

**`crates/tinker-pdf-cos/src/outline.rs`'s XMP comment is falsified by this
plan.** It says XMP is handed back unparsed because *"parsing it needs an XML
reader this engine does not have and should not grow"*. The first half is true
today and the second stops being true here. The comment is amended in the
ledger sweep, per CONTRIBUTING rule 4 — and `xmp_metadata` still hands back
bytes, because bringing a parser into the tree is not the same as deciding that
the metadata surface should use it.

**Gap 29's `ComicInfo.xml` non-goal stands, and is not discharged here.** That
plan defers it saying *"gap 30, XPS, is where one arrives"*, which is true of
the parser and not a commitment to the feature. Reading `ComicInfo.xml` is
still nobody's scope, and it is named here so that "gap 30 brought an XML
parser" is not read as "comic metadata works now".

## Scope

- **An XML parser**, in a new leaf crate: a well-formed-document subset of XML
  1.0 with namespaces — elements, attributes, comments, processing
  instructions, CDATA sections, the five predefined entities and numeric
  character references, UTF-8 and UTF-16 with BOM detection — bounded
  throughout, and **refusing DTD content by name**.
- **An OPC package layer**: `[Content_Types].xml` with `Default` and
  `Override`, part-name syntax and equivalence, the ZIP-item-name mapping in
  both directions, package and part relationships, and relative-reference
  resolution.
- **The fixed payload**: `FixedDocumentSequence` → `FixedDocument` →
  `FixedPage`, resolved through relationships rather than by filename
  convention.
- **Fixed-page markup**: `Canvas`, `Path` with the abbreviated geometry syntax,
  `Glyphs`, the five brushes, resource dictionaries and `{StaticResource}`
  references, clips, transforms, opacity and opacity masks.
- **ODTTF de-obfuscation** — thirty-two bytes XORed with a GUID taken from the
  part name.
- **The writer's missing half**: `/ExtGState`, transparency groups, type 2 and
  3 shadings with their functions, tiling patterns, and Type0/CIDFontType2
  fonts with `/Encoding /Identity-H`, `/CIDToGIDMap /Identity`, `/W` and a
  `/ToUnicode` built from `UnicodeString`.
- **Telling an XPS from a CBZ** inside `Document::open`, and `ArchiveRefusal`
  growing what an unreadable package needs.
- **A refusal, by name, everywhere the answer would otherwise be a plausible
  wrong page** — the feature, not the absence of one.

## Non-goals

Each of these is refused rather than approximated, and named so a reader does
not infer it from "XPS works now".

- **Writing XPS.** Nothing in the shipped surface produces one. A synthesised
  document saves as a PDF, which is what it is.
- **DocumentStructure and StoryFragments** (ECMA-388 16.1). Reading order,
  outlines, tables and figure semantics. Text extraction comes from the
  `/ToUnicode` the synthesiser writes and the geometric order it writes the
  runs in, which is the same guarantee this engine gives a PDF.
- **Digital signatures and SignatureDefinitions** (17.2). A signed XPS opens
  exactly like an unsigned one and **nothing here claims a signature is
  valid**. That sentence is in the non-goals rather than left implicit, because
  silence about a signature is a security claim by omission.
- **DiscardControl** (17.1, 18.2 [O10.5]) and interleaved `.piece` parts (OPC 7.2.4,
  7.3.7). Both are streaming accommodations for a consumer that pages out;
  this engine synthesises whole at open, like every other document it
  produces. A package whose items carry `.piece` names is **refused by name**
  rather than half-assembled — see
  [Interleaving is refused, with an escape hatch](#interleaving-is-refused-with-an-escape-hatch).
- **PrintTicket parts** (9.1.9). Print settings, not content.
- **3D content** (the `oxps-3d` namespace, D.1, and the `model/x3d+xml` parts).
  Recognised and refused by name.
- **TIFF and JPEG XR image parts** (9.1.5). This engine has neither decoder.
  JPEG XR is the one worth flagging rather than filing under "rare": 9.1.5.1
  recommends it *over* CMYK JPEG, so it is where a colour-managed producer is
  told to go.
- **ICC profiles, N-channel and named colours** (15.1.8, 15.2.5, 15.2.6).
  `ContextColor` carries a profile part URI and channel floats and **no sRGB
  fallback** — 15.2.5's syntax has nowhere to put one — so there is no cheap
  approximation available and the shape is degraded under ruling 2 rather than
  guessed at.
- **Hyperlinks** (16.2) and `FixedPage.NavigateUri`. The writer emits no
  annotations at all; adding link annotations is authoring work with its own
  scope.
- **`ComicInfo.xml`**, per the correction above.
- **Progressive or partial open.** The document is synthesised whole at `open`.

## Design

### The seam: synthesis again, and a different argument for it

`Document::cos(&self) -> &CosDocument` returns a **borrow, and not an
`Option`**. Gap 29 made that one signature the whole of its argument, and the
constraint has not moved. But gap 29's alternative was an enum inside
`Document`, and XPS has a second alternative that a CBZ never had, which is why
this section exists rather than pointing at that one.

**The tempting alternative is to emit straight to `trait Device`.** It is
public in `tinker-pdf-content`, `Renderer` is public in `tinker-pdf-render`,
and both are documented as drivable by something other than the interpreter —
`MAX_GROUP_DEPTH`'s comment says so in as many words. And the vocabulary is
genuinely closer to XPS than to PDF: `PathSegment::{MoveTo, LineTo, CurveTo,
Close}`, a `Matrix`, an `Rgb`, fill and stroke alphas, a `BlendMode`, clips,
`begin_group`/`end_group` with isolation and knockout, `begin_soft_mask` with a
pre-sampled transfer curve, and `save_state`/`restore_state`. XPS has no
operator stack and no content stream; it is a tree of elements with composable
transforms and opacities, and a save/restore pair around each subtree is
exactly what that tree is.

**Synthesis is taken anyway. Five reasons, in decreasing obviousness.**

*It does not answer `cos()`.* An XPS emitting to a `Device` still has no
`CosDocument` to lend, so the whole of gap 29's argument returns untouched:
either `cos()` becomes fallible or `Document` becomes an enum, and that is the
one genuinely breaking change available on this facade. Gap 29 paid to avoid
it; spending it here would make that payment retroactively pointless.

*It needs a second producer for text.* `Page::render` drives the rasterising
device and `Page::text()` drives `TextDevice`, and **both go through
`interpret` over a content stream**. A `Device`-emitting XPS would have to grow
a second producer for text extraction, and the two would then have to agree —
which is precisely the two-paths-two-pictures failure gap 29's milestone 4
found for `/Mask`, where a pass-through rendered opaque while its decoded twin
rendered transparent. Synthesis has one producer and both devices read it.

*It needs a `GlyphSource`, and the only one is private and PDF-shaped.*
`Renderer::new` takes `&dyn GlyphSource`, whose six methods resolve outlines,
images, shadings, patterns and tiles. The one real implementation,
`crates/tinker-pdf/src/resources.rs`'s `PageResources`, lives in a private
module and answers every one of them out of a `/Resources` dictionary. XPS
would need a second implementation of the hardest resource-resolution code in
the repository, and the two would drift.

*Nothing checks a `Device` call sequence.* [20](20-linearization-validation.md)
put qpdf in CI, and gap 29's milestone 5 recorded a defect — every page sharing
one image resource table — that **only qpdf caught**, because the renderer drew
the right picture either way. A synthesised XPS is a PDF a third party reads.
A sequence of `fill_path` calls is a claim this repository makes about itself.

*The facade re-exports none of the `Device` seam.* Not `Device`, not
`PathSegment`, not `Matrix`, not `GraphicsState`, not `Renderer`. Taking that
route means either publishing all of it — a much larger surface to freeze at
Checkpoint B than this plan wants to be responsible for — or duplicating it.

**And the cost is stated rather than buried.** Synthesis means generating PDF
operator bytes, and `PageBuilder::raw` — which takes operator bytes — is the
only general door into a content stream. Everything XPS needs beyond a
rectangle, an image and a WinAnsi text run has to be *written* first, which is
milestone 5 and is where a `Device`-emitting build would have had a genuine
head start. That is the honest shape of the trade: the reader side of this
engine is far ahead of the writer side, and this plan takes the route that
makes the writer catch up, because the writer catching up is worth something
outside XPS and a second render path is worth nothing outside it.

The mechanism is gap 29's: `DocumentBuilder` builds objects, `finish()`
serialises to `Vec<u8>`, and `CosDocument::open` parses those bytes — so the
synthesised document goes out through the writer and back in through the
reader and cannot take a shortcut around the parser every real PDF goes
through.

### An XML parser does not exist here

Confirmed rather than assumed. There is **no XML parser anywhere in this
repository**: no `xml` module, no markup tokenizer, no third-party dependency
(the workspace has none of any kind for format work). The files that touch
`<` are PDF lexers — `cos/lexer.rs` for `<<` and hex strings, `content/
tokenizer.rs` for content-stream tokens, `filters/ascii.rs` for ASCIIHex's
terminator, `font/cmap.rs` for PostScript CMap hex codes — and none of them
parses an element. XMP is handed back as an opaque byte blob with a comment
saying why. The single occurrence of an XML declaration in the whole tree is a
`cbz` test asserting that `image_format(b"<?xml version=\"1.0\"?>")` is `None`.

So this is new code, it is hand-rolled under CONTRIBUTING rule 1, and it is
needed three times over: fixed-page markup, `[Content_Types].xml`, and every
`.rels` part.

**It is its own leaf crate, `tinker-pdf-xml`.** Plan 00 says of the leaves that
they are *"bytes-in/values-out with zero PDF types … the property that makes
each one independently fuzzable"*, and ruling 8's August 2026 amendment makes
the test the definition rather than the list: *"a leaf is any crate that takes
bytes and plain parameters and returns bytes and values, whatever the list
says."* An XML reader is a textbook one — bytes in, a stream of events out, no
PDF vocabulary and no XPS vocabulary either. It does not go in
`tinker-pdf-filters`, whose name and module documentation both say PDF *stream
filters*; it does not go in `tinker-pdf-zip`, which turns an archive into names
and byte ranges and should not learn what is inside one; and it does not go in
the facade, because then [31](README.md) cannot reuse it without an edge that
points the wrong way.

It has **no dependencies at all**, which makes it the third crate in that
position alongside `filters` and `crypto`. The DAG amendment is therefore
smaller than gap 29's: one node with an empty allow-list, and `"tinker-pdf-xml"`
added to the facade's row. It is the **fourth** amendment to `ALLOWED` and the
first that adds no leaf-to-leaf edge, and its doc-comment paragraph is written
in the register the other three use rather than added silently — the file's own
commentary records what happened last time a crate was listed without care.

**A pull parser, not a tree.** Events out, one at a time, with the caller
holding whatever state it wants. A tree would allocate the whole of a fixed
page before anything looked at it, and a fixed page is the one part of this
format whose size is chosen by the file.

### The bomb this parser must refuse by name

This is the ruling 1 subject of the whole plan, and the specification takes the
same side, which is worth having in writing.

**ECMA-388 9.3.2, rule 2:** *"The XML 1.0 Standard allows for the usage of Data
Type Definitions (DTDs), which enable Denial of Service attacks, typically
through the use of an internal entity expansion technique. As mitigation for
this potential threat, DTD content MUST NOT be used in the XML markup defined
in this Standard, and consumers MUST instantiate an error condition when
encountering DTD content [M2.71]."*

So the answer is not a bounded expander. **A `<!DOCTYPE` refuses the part, by
name, before one byte after it is read**, and the refusal is a conformance
requirement rather than a hardening choice. Named in the tests, because a
mitigation nobody named is a mitigation nobody can check:

- **"Billion laughs"** — nested internal general entities, ten levels of ten,
  expanding to 10⁹ characters from under a kilobyte.
- **The quadratic-blowup variant** — one large entity referenced tens of
  thousands of times, which defeats a depth cap because its depth is one.
- **XXE** — an external entity naming a local file or a URL. This engine
  performs no I/O of any kind, so it could not fetch one, but a parser that
  *parses* the declaration is one refactor away from a parser that resolves it,
  and refusing the declaration removes the class rather than the instance.
- **A parameter entity in the internal subset**, which is the form that
  reaches the same place through the DTD's own grammar.

Each of the four is a committed input and each is asserted to be refused as
`DtdContent` specifically, not merely refused — gap 29's milestone 2 lesson,
where "this archive promised 5 000 bytes and produced 4 000" and "checksum
wrong" both refused and a test asserting only "refused" could not tell them
apart.

The five predefined entities (`&lt; &gt; &amp; &apos; &quot;`) and numeric
character references are **not** DTD content and are supported; they cannot
expand, because each produces exactly one character.

### OPC is not "a ZIP with names in it"

The package layer is small in code and long in rules, and every rule below is
one a naive reader gets wrong on the first real file.

**A citation warning first, because it will otherwise waste somebody's
afternoon.** ECMA-388's normative reference is *"ECMA-376, 1st edition, Office
Open XML File Formats (December 2006), Part 2, 'Open Packaging Conventions'"*,
and its clause numbers are that edition's — E.3 cites the ZIP test as OPC §9.2
and the content-types stream as §8.1.2. **The clause numbers in this section are
the 5th edition's (December 2021)**, which is what Ecma publishes today and what
was read to write this: the same rules sit at 7.3 and 7.2.3 there. The rules are
unchanged; the numbering is not, and a plan that quoted one edition's numbers
while citing the other's would be unusable.

**Part names are not ZIP item names.** OPC 6.2.2.2 gives the grammar —
`part_name = 1*( "/" isegment-nz )` over RFC 3987 segments, with no
percent-encoded slash or backslash, no percent-encoded unreserved character,
and no segment ending in a dot. OPC 7.3.4 and 7.3.5 give the mapping in both
directions: going in, strip the leading `/` and percent-encode every non-ASCII
character; coming out, un-percent-encode and prepend a `/`. `tinker-pdf-zip`
does neither, and says so in its own doc comment — *"this crate neither
normalises the path nor resolves `..`, because it never touches a filesystem
and inventing a canonical form would be a claim about one"* — which is the
right posture for a leaf and puts the work exactly here.

**Part names are ASCII-case-insensitive, and derivability is an error.** OPC
6.2.2.3: a package holding `/a` may not hold `/A`, and the name of one part may
not be *derivable* from another's — if `/segment1/segment2` exists, `/segment1`
may not. Both are refused rather than resolved to whichever came first,
because "whichever came first" is directory order and directory order is
whatever the producing tool walked.

**Media types are resolved by rule, not by extension.** OPC 7.2.3.5 is an
ordered algorithm: compare the part name against every `Override` element's
`PartName` (ASCII case-insensitive); failing that, take the substring right of
the rightmost dot in the rightmost segment and compare it against every
`Default` element's `Extension` (ASCII case-insensitive); failing that, the
part has no media type. The case-insensitivity is not decoration — a real
package produced for this plan carries `<Default Extension="ODTTF" …>` in
upper case against a part named `….ODTTF`, and a byte comparison against
`odttf` would find nothing.

**`[Content_Types].xml` is not first.** It is an item name, not a part name —
OPC 7.3.7 notes the brackets were chosen *because* they violate the part-name
grammar, so it can never collide with one — and its position in the archive is
unconstrained. In the packages measured for this plan it is the **last of
seven items**. A reader that assumes position is wrong on the first real file
it meets.

**Relationships are parts, and their names are derived.** OPC 6.5.2: the
package relationships part is `/_rels/.rels`; a part `/foo/bar.xml`'s
relationships part is `/foo/_rels/bar.xml.rels`, formed by inserting a `_rels`
segment before the last and appending `.rels` to it. Both forms are reserved
names under 6.2.2.2.

**Targets are relative references and must be resolved.** Not optional, and not
rare: in a real package written by Microsoft's own serialiser, one page's
relationships part carries
`Target="../../../Resources/21970891-….png"` while the page markup carries
`ImageSource="/Resources/21970891-….png"` for the same part. Both forms in one
file. Relative targets in a part relationships part resolve against the *source
part's* name (6.5.2), not against the relationships part's own, which is the
off-by-one-segment that a first implementation gets wrong and that no
absolute-only fixture can see.

**Two constraints OPC imposes that this tree already enforces.** OPC 7.3.6:
*"ZIP-based packages shall not include encryption"* and *"shall not use
compression algorithms except DEFLATE"*. `tinker-pdf-zip` already refuses an
encrypted entry and already refuses `Method::Other`, so for once a
specification and an existing refusal agree exactly, and the tests say which
clause each refusal is now also satisfying.

**Where it lives, and why not a crate.** OPC goes in the facade, as
`crates/tinker-pdf/src/xps/opc.rs`. It is close to a leaf by ruling 8's
definition, and the reason it is not one is that it would have exactly one
consumer forever: EPUB is **not** OPC — it uses its own OCF container with
`META-INF/container.xml` — so gap 31 does not reuse this. The byte-level risk
lives in `zip` and `xml`, which are leaves and are independently fuzzed; what
is left here is name arithmetic and graph resolution over already-validated
input, and it is covered by an `.xps` seed in the whole-pipeline fuzz target.
Recorded as a decision so it is not re-litigated in review.

### What `tinker-pdf-zip` already does, and the four things it does not

Gap 29 built the ZIP reader partly for this plan — *"that is the second reason
it is its own crate rather than a module in the facade"* — so the first
question this plan has to answer is whether that was true. It was, with four
qualifications, none of which is a change to the crate.

What arrives free: both routes (central directory, and a local-header scan when
there is no EOCD), Zip64's locator and record, stored and deflated entries,
data descriptors in all four shapes, CRC-32 compared on **every** entry, an
encrypted entry refused, a multi-disk archive refused, names decoded under
general-purpose bit 11, a per-entry cap and a per-archive inflation total that
is spent and never refunded, and `read` returning `Cow::Borrowed` for a stored
entry so a package's parts are subslices of its own bytes. A well-formed OPC
package always takes the central-directory route and nothing is inflated at
`open`.

What is owed by the caller:

1. **There is no lookup by name.** `entries()` is a slice and `read` takes an
   index. OPC resolves parts by name constantly, so the package layer builds
   its own index — and must build it over *normalised* names, which is the
   previous section's work.
2. **`Archive::read` charges the inflation budget on every call and never
   refunds it.** Reading `[Content_Types].xml` once to decide the format and
   again to use it spends it twice. The package layer caches what it reads,
   and that is a correctness requirement rather than an optimisation.
3. **A truncated name is kept, not dropped.** A name past `MAX_ZIP_NAME_LEN`
   (1 024) is truncated on a char boundary and the entry survives with a
   `Warning::NameTruncated`. For a comic that is the right leniency; for a
   package it produces a part name that resolves to the wrong part or to none.
   The package layer must read that warning and make the part unresolvable —
   and this is exactly the shape of gap 29's own milestone-6 survivor, where
   `Archive::warnings` was read into the report and **nothing asserted that it
   was**, so deleting the loop failed nothing in the workspace. The exit
   criterion is written in both directions for that reason.
4. **Duplicate names are neither detected nor deduplicated.** Two entries may
   carry one name with distinct indices, and OPC 6.2.2.3 makes that invalid.
   The package refuses rather than picking one.

One finding to confirm rather than assert: `scan::extent` in
`crates/tinker-pdf-zip/src/scan.rs` appears to take a `&mut Budget` and never
spend it, so the recovery route's inflate at `open` is uncharged against
`MAX_ZIP_INFLATED` and bounded only per entry. A conforming package never takes
that route; a damaged one does, and this plan will hand it damaged ones.
Milestone 3 confirms it and, if it holds, fixes it there rather than filing it.

### Two dialects, one reader

ECMA-388 Table D–2 gives the OpenXPS namespace as
`http://schemas.openxps.org/oxps/v1.0`, and every example in the standard uses
it. **Every package Microsoft's own XPS serialiser writes uses
`http://schemas.microsoft.com/xps/2005/06` instead**, which is XPS 1.0's
namespace. Measured rather than assumed: the two probe packages produced for
this plan carry it on `FixedDocumentSequence`, on `FixedDocument`, on
`FixedPage`, on the resource-dictionary key namespace, and on both relationship
types (`…/fixedrepresentation`, `…/required-resource`).

A reader that accepts only the standard's namespace refuses every `.xps` on a
Windows machine. A reader that accepts only Microsoft's refuses every
conforming `.oxps`. **Both are accepted**, resolved to one internal vocabulary
at the point the namespace is read, and a document mixing them across parts is
accepted too, because nothing in either specification makes that an error and
a package assembled by merging two documents is how it would happen.

**What does *not* discriminate the dialect is the content type.** ECMA-388
Table D–4 keeps `application/vnd.ms-package.xps-fixeddocumentsequence+xml` for
OpenXPS — the same string XPS 1.0 uses — and the probe packages carry exactly
those strings. So the obvious sniff is the wrong one, and it is recorded here
because it looks right.

**One thing is unresolved and milestone 1 settles it.** Some sources report
that Windows' OpenXPS output uses `oxps-`-prefixed content types
(`application/vnd.ms-package.oxps-fixeddocumentsequence+xml`) rather than the
`xps-` strings ECMA-388 tabulates. This plan has not seen a genuine `.oxps`:
the route that produces one is the "Microsoft XPS Document Writer" printer,
which since Windows 8 writes OpenXPS by default and which is **not installed on
the machine this was written on**. Milestone 1 obtains one and the answer goes
in its `As built`. Until then the reader keys on the **namespace**, which is
measured, and treats the content type as corroboration rather than as the
decision.

Note also ECMA-388 E.1: a producer *"MUST NOT create OpenXPS Documents with
filenames that end in … `.xps`"* and *"SHOULD NOT use
`application/vnd.ms-xpsdocument`"*. Extension is meant to discriminate.
`Document::open` takes bytes and no filename, so extension is not available and
sniffing is by content, which is the same posture the PDF path already takes.

### Telling the two ZIPs apart

`Document::open` sniffs `PK\x03\x04` at offset zero and hands everything to
`cbz::synthesise`. That becomes: open the archive **once**, ask the package
what it is, and route.

The test is ECMA-388 E.3's own three-step recipe, which is informative in the
standard and exactly right here:

1. the bytes are a ZIP (already true, by the offset-zero signature);
2. an item named `[Content_Types].xml` exists and a package relationships part
   `_rels/.rels` exists and parses;
3. `_rels/.rels` carries a relationship of either dialect's
   fixed-representation type whose target resolves to a part whose media type
   is the FixedDocumentSequence one.

All three, or it is not an XPS. **A comic archive that happens to carry a
`[Content_Types].xml` is still a comic archive**, which is the case that
decides the order of the test, and it is a fixture rather than a remark.

The cost of getting this wrong in the other direction is smaller and still
real: an XPS mis-routed to CBZ is today's defect, and a CBZ mis-routed to XPS
is a refusal where a document used to open. So the XPS test is the strict one
and CBZ is the fallthrough, unchanged.

### Geometry: 1/96 inch, top-left, y down

ECMA-388 18.1: *"In the x,y coordinate system, one unit is initially equal to
1/96 inch … The initial origin of the coordinate system is the top left corner
of the fixed page. The x-coordinate value increases from left to right; the
y-coordinate value increases from top to bottom."* A PDF page is 1/72 inch,
origin bottom-left, y up.

**A FixedPage's `/MediaBox` is `[0 0 W×0.75 H×0.75]`, and the page's content
stream opens with one `cm`: `0.75 0 0 -0.75 0 H×0.75`.** Verified against a
real file: 816 × 1056 becomes 612 × 792, which is US Letter to the point.

Consequences, all intended:

- `Page::size()` reports points, and reports the same numbers the same document
  printed to PDF would. Unlike a CBZ — which has pixels and no physical size,
  so gap 29 had to *invent* a convention — an XPS states its size, and no
  convention is needed.
- `RenderOptions::default()` (`scale: 1.0`, 72 dpi) renders Letter at
  612 × 792. `at_dpi(96)` renders one output pixel per XPS unit, which is this
  format's identity case and is worth saying on the type.
- **The flip is one `cm` rather than a negation applied to every coordinate**,
  so the numbers in the synthesised content stream are the numbers in the
  markup. A diff between the two is then readable, which is a debugging
  property worth not throwing away by pre-multiplying — and it is the property
  that makes milestone 6's tests assertable against the markup rather than
  against arithmetic done twice.

**18.1.2's coordinate rounding is deliberately not implemented**, and the
reason is recorded so it is not added later by someone reading the clause. It
rounds *device* coordinates to 1/16 of a unit, which is a rule for the
consumer's own rasteriser; this engine's rasteriser has its own, fixed-point
under ruling 4 and proved on two targets. Applying a 1/16 grid at synthesis
would quantise the *document* rather than the render, permanently, at whatever
resolution the file was later viewed at.

Similarly `ContentBox` and `BleedBox` (10.3.1, 10.3.2) map onto PDF's
`/CropBox` and `/BleedBox` and are written when present — one line each, and
the alternative is a document that loses the only statement it made about where
its content is.

### Fonts: ODTTF is thirty-two XORs, and the order is the whole of it

ECMA-388 9.1.7.3 [M2.53]: a consumer removes the extension from the last
segment of the part name, reads the remaining characters as a GUID, and XORs
the first 32 bytes of the part with the sixteen GUID bytes **in the order B37,
B36, B35, B34, B33, B32, B31, B30, B20, B21, B10, B11, B00, B01, B02, B03,
repeating the array once**, where the part name's last segment is
`B03B02B01B00-B11B10-B21B20-B30B31-B32B33B34B35B36B37`. The content type is
`application/vnd.ms-package.obfuscated-opentype` (Table D–4), which is what
selects the path — **not** the extension, which 9.1.7.3 says MAY be arbitrary
and which the standard only *recommends* be `.odttf`.

That permutation is the entire feature and it is entirely unmemorable, so it
was checked against a real file before this plan was written rather than after.
Part
`Resources/fe450c64-eeb3-40ba-8f52-8bee078061db.ODTTF`, first sixteen bytes
`db 60 80 07 ee 9c 53 8f ba 44 b3 9e 23 48 00 b8`, XOR key in the order above
`db 61 80 07 ee 8b 52 8f ba 40 b3 ee 64 0c 45 fe`, result
`00 01 00 00 00 17 01 00 00 04 00 70 47 44 45 46` — sfnt version `0x00010000`,
23 tables, and the first table tag `GDEF`. Bytes 16 to 31 come out as that
table's checksum, offset and length followed by the tag `GPOS`, and byte 32
onward is untouched and continues `GPOS`'s entry into `GSUB`'s tag. The
arithmetic is right and this plan is not guessing at it.

**Where it lives: the facade, not `tinker-pdf-font`.** The de-obfuscation needs
the *part name* to know its key, and a part name is a package concept — handing
one to a leaf whose subject is font programs would make that leaf know about
containers. `tinker-pdf-font` receives a plain byte slice that is already a
valid sfnt, exactly as it does today, and `Sfnt::parse` accepts
`0x00010000`, `true`, `OTTO` and `ttcf` unchanged. The TrueType-collection case
is real and cheap: 9.1.7's `FontUri` may carry a `#n` fragment naming a face,
and `Sfnt::parse` currently takes the first face of a `ttcf` — so a collection
with a fragment other than `#0` is **refused by name** in this plan rather than
silently drawn in the wrong face.

**No cap is added for de-obfuscation, and here is the argument**, in the form
`tinker-pdf-zip`'s `limits.rs` established: it is 32 XORs against a buffer the
ZIP reader has already bounded by `MAX_ZIP_ENTRY_BYTES`, it allocates nothing,
and a constant over it could never fire — which is gap 18a milestone 8's
failure reached from the other direction.

### The writer's missing half

This is where the L is, and it is worth enumerating because the milestone table
otherwise reads as one line.

| XPS feature | PDF answer | What `DocumentBuilder` has today |
| --- | --- | --- |
| `Opacity` on a leaf | `/ExtGState` `/ca` and `/CA` | nothing — no `/ExtGState` at all |
| `Opacity` on a `Canvas` | form XObject with `/Group`, `/ca` on the `Do` | nothing |
| `OpacityMask` | `/ExtGState` `/SMask` with a luminosity group | nothing |
| `SolidColorBrush` | `rg`/`RG` | `set_fill_rgb` only; stroke colour via `raw` |
| `LinearGradientBrush` | `/Shading` type 2 + a type 2 or 3 `/Function` | nothing |
| `RadialGradientBrush` | `/Shading` type 3 | nothing |
| `ImageBrush`, `TileMode` other than `None` | `/Pattern` PatternType 1 | nothing |
| `VisualBrush` | `/Pattern` PatternType 1 over a content stream | nothing |
| `Glyphs` with `Indices` | Type0/CIDFontType2, `/Identity-H`, `/CIDToGIDMap /Identity`, `/W`, `/ToUnicode` | a simple `/TrueType` with `/WinAnsiEncoding` and `/FirstChar 32` |
| stroke state | `w J j M d` | `raw` covers it, no API needed |
| paths and clips | `m l c h re W n f f* S B` | `raw` covers it |

Two remarks the table cannot carry.

**The renderer can already read every one of these.** Transparency groups and
soft masks landed in [11](11-transparency-groups.md), tiling patterns in
[09](09-tiling-patterns.md), shadings including the mesh types in
[10](10-mesh-shadings.md). It is only the *writer* that cannot emit them, which
is why this milestone is additive rather than risky: everything it writes has a
reader in this repository that a test can compare against.

**The Type0 row is the one that decides whether this plan is honest.** XPS's
`Indices` attribute addresses glyphs by index (12.1.3, `GlyphIndex` being *"the
index of the glyph (16-bit) in the physical font"*), and a WinAnsi simple font
cannot express that. The obvious fallback — write `UnicodeString` through
WinAnsi and drop `Indices` — produces text that is readable, plausible and
**wrong** exactly when the font's cmap and WinAnsi disagree, and correct on
every Latin fixture anybody would write. That is this plan's version of gap
18a's plausible photograph, and milestone 5 exists so that it is never the
shape the build takes. The pieces are already here: `add_embedded_font` runs
`Sfnt::parse` before embedding, `Sfnt::advance` gives `/W` from `hmtx`, and
`subset.rs` exists.

The `/ToUnicode` is not decoration either. `UnicodeString` carries the text and
`Indices` carries the mapping — including 12.1.3's cluster form,
`(ClusterCodeUnitCount:ClusterGlyphCount)`, which is precisely a many-to-many
mapping and precisely what `/ToUnicode` exists to record. Writing it is what
makes `Page::text()` work on an XPS at all, and it comes free with synthesis
only if somebody writes it.

### Fixed-page markup: what the first real file already needs

The temptation is to build a spine, then paths, then "advanced" features like
resource dictionaries. A real file says otherwise. Here is the entire body of a
one-page package written by Microsoft's serialiser for a picture and a line of
text:

```xml
<FixedPage xmlns="http://schemas.microsoft.com/xps/2005/06"
           xmlns:x="http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key"
           xml:lang="en-us" Width="816" Height="1056">
  <FixedPage.Resources><ResourceDictionary>
    <ImageBrush x:Key="b0" ViewportUnits="Absolute" TileMode="None"
                ViewboxUnits="Absolute" Viewbox="0,0,4.00055837631226,4.00055837631226"
                Viewport="0,0,200,200" ImageSource="/Resources/21970891-….png" />
  </ResourceDictionary></FixedPage.Resources>
  <Path Fill="{StaticResource b0}" RenderTransform="1,0,0,1,100,100"
        Data="M0,0L200,0 200,200 0,200Z" />
  <Glyphs OriginX="100" OriginY="425.9" FontRenderingEmSize="24"
          FontUri="/Resources/b8665960-….ODTTF" UnicodeString="Page one"
          Indices=",53" Fill="#FF000000" />
</FixedPage>
```

Five things that are not optional, from the *first* file:

- **XPS has no image element.** An image is a `<Path>` filled with an
  `<ImageBrush>`. Every raster on every page arrives this way.
- **Resource dictionaries and `{StaticResource}`** (14.2) are how that brush is
  named. They cannot be deferred; the first picture needs them.
- **Abbreviated geometry** (11.2.3) is how every path is written. Even a
  rectangle is `M…L…L…L…Z`.
- **`RenderTransform`** is a six-number comma list (14.4.1) and is present even
  when it is a translation.
- **`Indices=",53"`** is 12.1.3's grammar in its least obvious form: an
  *empty* `GlyphIndex` — legal, meaning "look the code unit up in the font's
  cmap" — followed by an advance width, for one of the eight characters, with
  the rest defaulting. A parser that requires a digit before the comma fails on
  the first real file.

So the milestone order below builds the spine first because a page has to exist
before anything can be drawn on it, and then builds **path, brush, resource
dictionary and transform together**, because a build that has any three of them
cannot draw the file above.

### What is refused, and at which level

Ruling 2 degrades rather than fails, and gap 17's finding was that *the refusal
is the feature*. XPS has one more level than CBZ did, and the extra level is
where most of the honesty lives.

**Package level — refuse at `open`.** Not a ZIP; no `[Content_Types].xml`; no
package relationships part; no fixed representation; a part name that is
invalid, duplicated or derivable from another; `.piece` items; a bound spent; a
`FixedDocumentSequence` naming no documents at all. Each gets its own
`ArchiveRefusal` variant, and `ArchiveRefusal` is already reachable through
`OpenError::UnsupportedArchive`, so no new `OpenError` variant is needed — gap
29 made both enums `#[non_exhaustive]` in one commit precisely so this plan
would cost one addition and not one break.

**Page level — a placeholder page, and the page count is unchanged.** A
`PageContent` whose `Source` does not resolve, a FixedPage part that is not
well-formed XML, a page whose `Width` or `Height` is missing or unusable: each
becomes a page anyway, of the book's own size, carrying the neutral placeholder
and a named warning. This is gap 29's rule and it is taken for gap 29's stated
reason — dropping the page renumbers every page after it, and a reader sees a
document that jumps and blames the file.

**Element level — the rule that decides how honest a page is.** Two cases and
they are not symmetric:

- **Geometry unreadable → the element is not painted, and warns.** A `Path`
  whose `Data` will not parse, a `Glyphs` whose font cannot be read, a
  transform that is not six numbers. A missing shape is visible as a missing
  shape.
- **Geometry readable, paint unreadable → the element is painted in the neutral
  placeholder grey, and warns.** An unresolved `{StaticResource}`, a
  `ContextColor` naming an ICC profile, a `TIFF` or JPEG XR `ImageSource`, an
  N-channel colour. The shape and its position are known and correct; only the
  colour is not, and the engine already has one answer for "I know where it is
  and not what it looks like".

The asymmetry is deliberate and it is [07](07-stroked-patterns.md)'s lesson:
that gap's headline defect was a gradient-stroked rule painting **solid black,
silently**, and black is a plausible colour where the placeholder grey is not.
A default that could be right is worse than a default that is visibly a
default.

**A transform, a clip or an opacity that cannot be read refuses its element
rather than defaulting.** The identity matrix, the absent clip and full opacity
are each a plausible wrong answer that draws the right content in the wrong
place at the right size — which is the failure this whole section exists to
prevent. This is the one place where "default" and "refuse" genuinely diverge
in the tree, and it is written down here rather than decided by whoever writes
`canvas.rs`.

### Interleaving is refused, with an escape hatch

OPC 7.2.4 and 7.3.7 let a part be split across items named
`name/[0].piece`, `name/[1].piece`, …, `name/[n].last.piece`, and ECMA-388 17.1
is four pages on how to produce them well. `tinker-pdf-zip` will list them as
ordinary entries with those literal names, so a reader that does nothing sees
parts whose names end in `.piece` and no part with the name they belong to.

**They are recognised and refused by name**, because reassembling them is a
second addressing model layered on the first and neither probe package produced
for this plan uses one. But the refusal is tied to evidence rather than to
taste: **if milestone 1's corpus turns up an interleaved package, this plan is
amended and the case is built** — the thing that must not happen is the case
being handled by accident, or a `.piece` item being silently treated as a part
in its own right, which is what a reader that never looks would do.

## Where a half-implementation is worse than none

Seven, and the first is already shipping.

**An XPS opening as a comic.** Measured above: a real document opens, reports
one page, and shows a 4 × 4 resource. Nothing warns. This is gap 17's blank
page reported as success, arriving through a container sniff that recognises
the container and assumes the format. The defence is milestone 3's
discrimination, and it is the reason that milestone is early.

**A page that draws some of its elements.** XPS is a tree, and a `Canvas` whose
`RenderTransform` failed to parse and was defaulted to identity draws every
descendant in the wrong place, at the right size, with the right colours. It
looks like a layout bug in the producer. The defence is the asymmetry above:
transform, clip and opacity refuse; fill degrades.

**A `PageContent` whose `Source` does not resolve, dropped.** Gap 29's
renumbering hazard, arriving through markup rather than through filenames — and
worse here, because a `FixedDocument` that references ten pages and resolves
nine produces a nine-page document with no gap anywhere in it. The page count
comes from the markup and every entry in it produces a page.

**`{StaticResource}` resolved to black.** Gap 07's exact defect one format up.
An unresolved brush is the placeholder grey and a warning, never a colour that
could have been the right one.

**Glyph indices addressed through a WinAnsi font.** Correct on every Latin
fixture, wrong the moment a font's cmap and WinAnsi disagree, and produced by
the fallback that a build without milestone 5 is pushed towards. Milestone 5 is
before milestone 7 for this reason and the ordering is the point.

**ODTTF de-obfuscated with the key bytes in the wrong order.** The result is
32 bytes of garbage at the head of the font, `Sfnt::parse` returning `None`,
and a page that draws no text with an `UnreadableFont` warning — which is
indistinguishable from a font this build genuinely cannot read, so the failure
hides inside an existing, expected warning. The defence is the arithmetic being
checked against a real file byte for byte, which this plan has already done
once, and a test that asserts the *de-obfuscated bytes*, not that a page drew.

**An XML parser that expands entities.** The one input in this plan that can
take a machine down rather than produce a wrong picture, and the one where the
specification does the deciding: 9.3.2 [M2.71] says DTD content MUST NOT be
used and a consumer MUST instantiate an error condition on meeting it.

## Bounds, per ruling 1

Every number below is attacker-controlled. Two scars set the form and gap 29
built the machine that checks it.

`5adf502 fix(render): bound the group buffers a page may open, not just their
depth` found an 1 851-byte page that took **19.3 seconds to render 9 600
pixels**, with `MAX_GROUP_DEPTH` in place the whole time: *"depth is not work
once the recursion branches"*. The XML version of that sentence is that **a
per-element cap is not a total once the element count is chosen by the file**,
and the `{StaticResource}` graph is where it branches.

[18a](18a-jpx-decoder.md)'s milestone 8 found the other failure, in a constant
written to avoid the first: `MAX_JPX_WORK` was set *above* the most its own
inputs could ask for, so it could **never fire**.

| Name | Bounds | Why it cannot be a per-item cap |
| --- | --- | --- |
| `MAX_XML_DEPTH` | Element nesting within one part | — (a genuine per-part depth cap; it bounds the parser's own stack) |
| `MAX_XML_ATTRIBUTES` | Attributes on one element | — (per-element, and says so) |
| `MAX_XML_NAME_LEN` | One element or attribute name | — (per-item) |
| `MAX_XML_TOKENS` | **A work cap.** Events produced across one part | A per-element cap times a file-chosen element count is not a bound |
| `MAX_XPS_PARTS` | Parts admitted from one package | Must sit **below** `MAX_ZIP_ENTRIES`, or the ZIP reader refuses first and this can never fire |
| `MAX_XPS_PAGES` | FixedPages synthesised | Must sit below `MAX_XPS_PARTS` for the same reason |
| `MAX_XPS_ELEMENTS` | **The work cap.** Drawable elements across the whole document | A per-page cap times a file-chosen page count is not a bound |
| `MAX_XPS_SEGMENTS` | **A work cap.** Path segments produced by abbreviated geometry, across the document | `L 0,0` is six bytes; one `Data` attribute in a 128 MiB part is twenty million segments, and a per-path cap does not see it |
| `MAX_XPS_RESOURCE_DEPTH` | `{StaticResource}` resolution depth, with a cycle guard | A dictionary entry may reference another; a cycle is two lines of markup |
| `MAX_XPS_VISUAL_DEPTH` | `VisualBrush` / `Canvas` nesting **across parts** | `MAX_XML_DEPTH` bounds nesting *within* a part and a remote resource dictionary recurses *between* parts, so one cannot substitute for the other. ECMA-388 18.2 recommends 16 for each and [M11.5] makes refusing past a consumer's limit conformant |
| `MAX_SYNTHESISED_PDF` | Bytes handed to `CosDocument::open` | Already exists in `cbz.rs`; reused rather than duplicated, and the ledger says so |

Per-item caps sit beside them and the comment on each says in as many words
that it is *not* a work cap, in the register `MAX_SCRIPT_STEPS`,
`MAX_MESH_TRIANGLES` and `tinker-pdf-zip`'s `limits.rs` already use.

**Three deliberately absent, argued where they are declared**, because gap 29
established that writing down why a cap was *not* added is the cheaper half of
this discipline: nothing on ODTTF de-obfuscation (32 XORs over an
already-bounded buffer); nothing on the number of relationships (each is one
element and `MAX_XML_TOKENS` already bounds them); nothing on part-name
*depth*, for `tinker-pdf-zip`'s stated reason — nothing here touches a
filesystem, so depth bounds no allocation and no recursion, and length is what
costs.

**How each is measured.** Three figures per constant go in the `As built`: the
most any fixture in this repository legitimately spends, the most a plausible
real document spends, and the constant. The yardstick has to be named or the
second number is a mood, so: **a 200-page fixed document at roughly 2 000
drawable elements and 40 000 path segments a page**, which is a dense
report or a technical drawing rather than a letter. Each cap is proved to fire
**by its own refusal or warning, never by a clock** — `5adf502`'s method, taken
for its stated reason.

### What this inherits from `bounds_ledger.rs`

Gap 29's milestone 6 built `crates/tinker-pdf/tests/bounds_ledger.rs`
specifically so that `MAX_JPX_WORK`'s class of failure could not repeat, and it
checks the **set** rather than each constant from where it stands. Every new
constant above joins **that** table rather than getting a second sweep of its
own — the whole value of it is that it is one table — and inherits all five of
its checks:

1. **`every_bound_can_fire`** — arithmetically, the constant is below the most
   its own inputs can ask for. This is the check that caught `MAX_ZIP_INFLATED`
   raised to 1 PiB and was the **only** thing in the workspace that did.
2. **`every_bound_publishes_the_number_it_is`** — the number in the ledger
   comment parses back to the `const`, so `**This cap** | **1 GiB**` and
   `1 << 30` cannot drift apart. It was the only thing that caught
   `MAX_ZIP_NAME_LEN` changed without its table.
3. **A plausible real document fits under every one**, so none is a missing
   feature wearing a `MAX_` prefix. Gap 29's yardstick was a 200-page comic;
   this plan adds its own, named above, and the test carries both.
4. **`every_bound_names_a_test_that_exists`** — the named firing test exists,
   is a `#[test]`, and is not `#[ignore]`d. It caught a renamed test and an
   ignored one, alone.
5. **Nothing is proved by a clock**, asserted by the sweep itself.

And gap 29's milestone 5 added one rung stronger than a test: the relation
`MAX_CBZ_PAGES < MAX_ZIP_ENTRIES` is checked in a `const` block, so a build
that breaks it **does not compile**. `MAX_XPS_PAGES < MAX_XPS_PARTS <
MAX_ZIP_ENTRIES` is written the same way, for the same reason.

Two fuzz targets land with the code that needs them rather than at the end, per
plan 02's rule: `xml` as the repository's **twenty-first**, and an `.xps` seed
added to the whole-pipeline target that already exists, which is what covers
the OPC layer. Each carries the target's control byte in front, and — the
lesson `d9945a0` and gap 29's milestone 6 both record — **at least two of the
five seeds set every knob to the tightest value the target offers**, because a
corpus in which every seed is roomy explores the happy path and reaches no
refusal.

## Oracles, per ruling 9

Ruling 9: *"mutool, pdftoppm, pdfium_test and qpdf are invoked as external CLIs
in CI; nothing links them, and their outputs are transient comparison
references, never committed or redistributed."* Three of the four matter here
and one of them is a surprise.

**qpdf, on the produced PDF.** Gap 29's route, unchanged, through the CI job
[20](20-linearization-validation.md) already built. It earned its place on the
first milestone that used it: gap 29's milestone 5 found that every page shared
one image resource table, a defect **qpdf alone** caught, because the renderer
drew the right picture either way. This plan writes strictly more object
structure than gap 29 did — `/ExtGState` dictionaries, form XObjects with
`/Group`, pattern and shading resources, a Type0 font with a descendant and a
`/ToUnicode` — so there is strictly more that only qpdf can see. `--show-pages
--with-images` and `--show-object` per page, from **independent commands**, is
the shape gap 29's milestone 5 had to rebuild against the real tool and is the
shape this one starts with.

**mutool, on the XPS itself, and the irony is worth writing down.** MuPDF reads
XPS — `mutool draw` lists `pdf`, `xps`, `cbz` and `epub` as its input formats —
and mutool is already one of ruling 9's four named oracles. So **the library
gap 28's decision removes from Tinker's shipped tree is the best available
oracle for the format it is being removed for**, and ruling 9 permits exactly
that, because an oracle is invoked and never linked. `mupdf-tools` is an apt
package, so the `ubuntu-latest` runner installs it beside qpdf.

Two things it gives, and they are not the same thing:

- **`mutool draw`'s `trace` output format** emits a device-call trace as XML — a *structural*
  oracle. It says which paths were filled with what, in what order, under what
  matrix, and it can agree with this engine while disagreeing about every
  anti-aliased pixel. That is the comparison that can be made exact, and it is
  the one to build the criterion on.
- **A rendered PNG**, compared through `pdfcmp`'s perceptual diff at a stated
  threshold. Two independent rasterisers are **not** byte-equal and this plan
  does not promise they will be; gap 18a's oracle discussion pre-argued the
  same point for a fixed-point wavelet against a float reference.

And the caveat that must be stated rather than discovered: **it is the same
engine Tinker is leaving**, so where MuPDF is wrong about XPS this engine will
agree with it and both will be wrong. An oracle bounds the space of
disagreements; it does not certify the answer.

**The Windows XPS stack, as a local cross-check rather than a CI gate.**
`System.Windows.Xps.Packaging.XpsDocument` with `RenderTargetBitmap` rasterises
an XPS headlessly from PowerShell — a third renderer, and the reference one.
Two honest limits: it is Windows-only, so it cannot gate a job on
`ubuntu-latest`; and it is the same code that *wrote* the fixtures, so it is
independent of this engine and not of the producer. Recorded in the `As built`
as a measurement, not wired into CI.

**The XPS Viewer is not an oracle.** It is a GUI application, absent from the
default Windows install since 1803 and available as an optional feature. It
cannot be scripted into a comparison, and saying so is better than gesturing at
it as though Windows shipped a checker.

**Whatever the CI job is, it greps its own output.** Gap 20 found that a
skipped oracle test exits 0 and reads exactly like a pass; the `qpdf-oracle:
RAN` / `SKIPPED` pattern is copied verbatim for the mutool job, and the job
goes **red** when the tool is missing.

## A real `.xps`, and gap 29's largest debt

Gap 29 closed with this, as a limitation of the whole gap rather than of one
commit:

> **No `.cbz` written by a real archiver has ever been opened by this code.**
> Every archive in the tree is hand-built from APPNOTE 6.3.10's field layouts
> and every image from its own specification's. … **The first real archive this
> meets may find something, and nothing here would have.**

Three milestones recorded it as owed and the sixth did not discharge it. **This
plan does not repeat that, and the way it does not is structural: obtaining
real documents is milestone 1, before the XML parser, before the package layer,
before anything.** Every later milestone's fixtures then come from files this
repository did not write, and a milestone that cannot be tested against one has
to say so in its own exit criteria.

**Where they come from.** Three routes, and the first two are Windows' own XPS
serialisation stack rather than anything hand-built:

1. **`System.Windows.Xps.Packaging.XpsDocument` from PowerShell.** No printer,
   no elevation, no third-party software. **Verified while writing this plan**:
   two packages were produced, of seven and eight items, and both are the
   evidence quoted throughout this document. This is Microsoft's own
   `ReachFramework` serialiser, so the bytes are the reference producer's.
   It writes **XPS 1.0**.
2. **The "Microsoft XPS Document Writer" printer.** The Windows optional
   feature that supplies it is `Printing-XPSServices-Features`, and it is **not
   installed on the machine this was written on**. Enabling it needs elevation,
   which is also why that feature name is stated from documentation here rather
   than read back from this machine. Since Windows 8 it writes **OpenXPS
   (`.oxps`)** by default, and its save dialog offers both.
   That is why it matters rather than being a duplicate of route 1: **the two
   routes together give both dialects**, which is what
   [Two dialects, one reader](#two-dialects-one-reader) needs and what settles
   the content-type question that section leaves open.
3. **Third-party producers** — LibreOffice exports XPS, and various print
   drivers emit it. Unverified here, and worth a pass because a format's
   conventions are what a fuzzer cannot find.

**Whether they can be committed.** Yes, for routes 1 and 2, and there is an
in-tree precedent rather than a judgement call: `fuzz/README.md` records that
the JPX seed corpus holds *"codestreams `opj_compress` made from **our own**
32 × 32 images"* under the reading that **a tool's output on our input is ours
to commit**, while ISO/IEC 15444-4's conformance codestreams stay out. An
`.xps` written by Windows' XPS serialiser from content this repository authored
sits in exactly that place. Documents from elsewhere are fetched and never
committed, per ruling 9 and gap 17's SerenityOS handling.

**What opening one has already found**, before any code: the two failures in
[What is wrong](#what-is-wrong), the `[Content_Types].xml`-is-last surprise,
the UTF-8 BOM on every XML part, the `<Default Extension="ODTTF">` case
mismatch, the `Indices=",53"` empty-glyph-index form, the absolute-and-relative
target pair in one file, and the namespace divergence that is the whole of
decision 3. **None of those is in ECMA-388**, and none would have been found by
a fixture written from it.

## `deny.toml` gains the XML crates

Gap 29 wrote: *"The XML crates gap 30 will need denied are that plan's to add,
not this one's."* This is that plan.

`deny.toml` denies fifty-seven crates by name under a comment saying the
hand-rolled rule *"lived only in prose, so a new dependency that happened to be
MIT-licensed would have passed every check in this file unnoticed"*. It denies
no XML crate at all, and an XML reader is the single most reachable-for
dependency in this whole programme: `quick-xml`, `roxmltree`, `xml-rs`,
`xmlparser`, `serde-xml-rs`, `minidom`, `sxd-document`, `libxml`, `rustyxml`.
All are denied in the milestone that writes the parser, not in the milestone
that finishes the plan, because the window in which the temptation exists is
exactly the milestone that writes the parser.

`serde` and `serde_derive` are **not** added, and the reason is recorded so the
omission does not look like an oversight: they are not format implementations,
they are used by nothing here, and denying a general-purpose crate on the
grounds that it is often seen near XML would make this file about taste rather
than about rule 1.

## Milestones

The commit-boundary rule is per-plan
([00-execution-order.md](00-execution-order.md)); this one is nine commits, one
per milestone, each independently green under the full gate.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | **A real XPS, before any reader** | At least **six** genuine packages from producers this repository did not write, with provenance recorded per file: both routes above exercised, so **both dialects are represented** and the `.oxps` content-type question this plan leaves open is answered in the `As built`; at least one multi-page, one with a raster image, one with an ODTTF font, one with a gradient or a tiling brush, and one produced by something that is not Windows if one can be found; each committed under `fuzz/README.md`'s "our input through their tool" precedent, or recorded as fetched-not-committed with the reason; a checked-in inventory naming every part, its media type, its compression method and its size, produced by a script committed beside it so the inventory cannot drift from the files; **and the two present-day failures re-measured and pinned as tests that fail today** — an XPS with a raster resource opening as a one-page comic, and one without being refused as `NoImages` — so milestone 3 has something to turn green | S |
| 2 | **`tinker-pdf-xml`, the eighth leaf** | A pull parser over a well-formed subset of XML 1.0 with namespaces: elements, attributes, comments, processing instructions, CDATA, the five predefined entities, numeric character references in both radixes, UTF-8 and UTF-16 with BOM detection, and `xml:lang`/`xml:space` passed through as ordinary attributes; **`<!DOCTYPE` refused as its own named error before one byte after it is read**, with committed inputs for billion laughs, the quadratic-blowup variant, an external entity and an internal-subset parameter entity, each asserted to be refused *by that name* and not merely refused; well-formedness errors distinguished from each other rather than collapsed — mismatched end tag, duplicate attribute, undeclared namespace prefix, illegal character, unterminated construct; every bound in the table above firing by its own refusal; ruling 8 holds, no PDF and no XPS vocabulary in the public API; `cargo run -p xtask -- dag` green with the new node and the `ALLOWED` doc comment carrying the fourth amendment's argument; the **twenty-first** fuzz target; `deny.toml` gains the nine XML crates | M |
| 3 | **OPC, and the discrimination** | `Document::open` opens the archive **once** and routes by ECMA-388 E.3's three steps; **milestone 1's two failing tests go green** — an XPS with a raster resource is no longer a one-page comic, and one without is no longer `NoImages`; a CBZ carrying a `[Content_Types].xml` is still a CBZ, from a fixture built for it; part names normalised per OPC 7.3.4/7.3.5 in **both** directions with a round-trip test over every name in milestone 1's inventory; 6.2.2.3's ASCII-case-insensitive equivalence, with `/a` beside `/A` and a derivable pair each refused by name; media type resolved by 7.2.3.5's ordered algorithm with `Override` beating `Default` and both compared case-insensitively — asserted against the real `<Default Extension="ODTTF">`; relationships resolved from `/_rels/.rels` and from a part's derived `_rels` name, with relative targets resolved against the **source part** and a fixture carrying `../../../` alongside an absolute sibling; a `.piece` item refused by name; `Warning::NameTruncated` making that part unresolvable, asserted **in both directions** — a truncated name is unresolvable and an untruncated one is not — so gap 29's dropped-warnings survivor cannot recur; a read-once cache proved by asserting `Archive::inflated()` does not grow on a second resolution of the same part; `scan::extent`'s uncharged budget confirmed and fixed or shown not to exist | M |
| 4 | **The spine, and an honest blank page** | `FixedDocumentSequence` → `FixedDocument` → `FixedPage` resolved through relationships and media types, never by extension; a multi-page package from milestone 1 opens with `page_count()` equal to its `PageContent` elements **in markup order**, and a package whose document order and filename order disagree proves the difference; `Page::size()` is `Width × 0.75` by `Height × 0.75`, asserted as 612 × 792 on a Letter fixture; `/CropBox` and `/BleedBox` written from `ContentBox` and `BleedBox` when present; every page renders the **neutral placeholder with a named warning** rather than white, because an empty page reported as success is the thing gap 17 spent itself on; `Document::cos()` returns the synthesised document and **saving it produces a file qpdf reads clean**; a `PageContent` whose `Source` does not resolve still produces a page and keeps its number; both dialects' namespaces accepted, from two real files | S |
| 5 | **The writer's missing half, before anything needs it** | `/ExtGState` with `/ca`, `/CA`, `/BM` and `/SMask`; form XObjects with `/Group` for isolated and knockout groups; `/Shading` types 2 and 3 with type 2 and type 3 `/Function`s; `/Pattern` PatternType 1; **Type0/CIDFontType2 with `/Encoding /Identity-H`, `/CIDToGIDMap /Identity`, `/W` from the font's own `hmtx`, and a `/ToUnicode` accepting a many-to-many mapping**; every one of them round-tripped through **this repository's own renderer** — write it, open it, render it, and compare against the same picture built by the existing paths — which is the comparison [11](11-transparency-groups.md), [09](09-tiling-patterns.md) and [10](10-mesh-shadings.md) make possible and no other milestone here can; qpdf clean on a document using all of them; `maybe_compress`'s contract and `add_image`'s validation posture extended rather than bypassed; **nothing in this milestone mentions XPS**, which is the test of whether it belongs in the writer | M |
| 6 | **Paths, brushes, transforms, resources** | 11.2.3's abbreviated geometry in full — `F M L H V C Q S A Z` in both cases, the omitted-repeat form (`L 100,200 300,400`), a relative first `Move` from `0,0`, a relative command after `Close` starting from the previous figure's first point, and the elliptical arc; `PathGeometry`/`PathFigure`/`PolyLineSegment` and friends as the non-abbreviated equivalent, asserted to produce **the identical** segment list; `MatrixTransform` and the six-number attribute form; `Canvas` with composable transform and opacity, the latter as a transparency group when the canvas has children that overlap; `Canvas.Clip`, `Path.Clip`; `SolidColorBrush` with `#RRGGBB`, `#AARRGGBB` and `sc#`; `LinearGradientBrush` and `RadialGradientBrush` with `GradientStops`, `SpreadMethod` and `Transform`; `FixedPage.Resources`, `Canvas.Resources`, `ResourceDictionary` and `{StaticResource}` with 14.2.5's scoping, a depth cap and a **cycle refused rather than recursed**; the element-level refusal rules from the design section, each with a fixture — a bad `Data` not painted, a bad `Fill` painted grey, a bad transform refusing its element; **the whole of the real one-page package in the design section renders**, which is the criterion this milestone exists for | L |
| 7 | **Glyphs, and ODTTF** | 9.1.7.3's de-obfuscation asserted on the **de-obfuscated bytes**, not on a page that drew — the real part quoted in the design section, out to its first table tag; the content type selecting the path and the extension not selecting it, from a fixture whose extension lies; a TrueType collection with a `#n` fragment other than `#0` refused by name; 12.1.3's `Indices` grammar in full, including an **empty `GlyphIndex`**, the cluster form `(m:n)`, advance width and both offsets, exponents in the reals, and a trailing empty mapping; `UnicodeString` alone, with the font's own cmap doing the lookup through `Sfnt::glyph_for_char`; `BidiLevel`, `IsSideways` and `StyleSimulations` each either implemented or **refused by name**, never ignored; the run reaching the page through milestone 5's Identity-H font, so `Indices="53"` draws glyph 53 and a fixture whose cmap and WinAnsi disagree proves it; `Page::text()` returning the `UnicodeString` through the `/ToUnicode`, which is the test that says synthesis paid for text extraction | M |
| 8 | **Images and tiling brushes** | `ImageBrush` with `Viewbox`, `Viewport`, `ViewboxUnits`, `ViewportUnits` and `TileMode` in all five values, the flips built as a four-times cell since PDF patterns have no flip; JPEG and PNG image parts through gap 29's existing `ImageData::Jpeg` and `png_embed` paths, **passed through rather than decoded**, so a page's peak cost is a multiple of the part rather than *w × h × 3*; the image's own resolution read for the 13.4.1 default of 96 dpi and its precedence order; TIFF and JPEG XR parts recognised by content type **and** by magic bytes and refused by name at the element, so the rest of the page draws; `VisualBrush` with `VisualBrush.Visual` through a tiling pattern, bounded by `MAX_XPS_VISUAL_DEPTH` **across parts**; a remote resource dictionary part resolved and its cycle refused | S |
| 9 | **Bounds, determinism, ledgers, campaign** | Every constant in the bounds table joins `bounds_ledger.rs`'s **existing** table and passes all five of its checks, with three recorded numbers each and the new yardstick named beside gap 29's; `MAX_XPS_PAGES < MAX_XPS_PARTS < MAX_ZIP_ENTRIES` in a `const` block, so a bad relation **does not compile**; the **fourteenth** determinism fingerprint — gap 29's `cbz` is the thirteenth — whose fixture is a real package from milestone 1 rather than a hand-built one — the first fingerprint in that file whose input this repository did not author — plus a byte hash of the synthesised document beside it, in the pair gap 29's milestone 6 established, reproduced on `wasm32-wasip1` with none of the other thirteen moving; the mutool oracle job, red when the tool is missing; `cargo fuzz run xml` surviving a session with no crash, no OOM and no timeout, and the whole-pipeline target re-run with `.xps` seeds; the ledger sweep below; peak memory recorded for milestone 1's largest package | S |

**Milestone 1 comes first and it is the whole answer to gap 29's largest
debt.** That plan's fixtures were hand-built from field layouts to the very
end, and its close says plainly that the first real file may find something
nothing there would have. Here the first real file is milestone 1 and it has
already found seven things.

**Milestone 5 comes before milestone 6 and 7**, and the ordering is the point
rather than a preference — [18a](18a-jpx-decoder.md)'s M0 did exactly this, and
gap 29's milestone 1 did it again for `inflate_raw`. A build that reaches
milestone 6 without a writer that can emit an `/ExtGState` will approximate
opacity, and a build that reaches milestone 7 without a Type0 font will address
glyphs through WinAnsi. Both approximations render correctly on every fixture
anybody would write by hand, and both are wrong. Landing the writer first costs
one commit's ordering; landing it after costs a rewrite of two milestones and a
period in which the wrong shape is what the tests assert.

**Milestone 3 is early for a different reason.** It is the only milestone that
improves matters on its own: after it, an XPS is refused by a name that is true
instead of opening as a comic. If this plan were ever descoped, milestone 3 is
the part that must still land.

### The ledger sweep milestone 9 owes

**The leaf count changes again, and it is written in four places**, none of
which the compiler can reach. Gap 29 found it had already drifted once —
`tinker-pdf-math` arrived and not one of the three prose statements moved — and
found a fourth place nobody had connected to the count. All four go from
**seven** to **eight**:

- **`docs/plans/00-architecture.md`**, the DAG diagram and the enumerated
  count, as a dated in-place amendment in the style that file's `cos -> font`
  amendment uses;
- **`docs/plans/99-consistency.md`** ruling 8, whose amendment already says the
  rule is wider than its list — so this is the first update that the amendment
  itself predicted;
- **`CONTRIBUTING.md`** rule 3;
- **`README.md`**'s "Workspace" section, which is the fourth place gap 29
  found.

Plus `xtask/src/main.rs`'s `ALLOWED` and its doc comment, which is where the
argument lives.

The rest of the ordinary ledger:

- **`crates/tinker-pdf-cos/src/outline.rs`**'s XMP comment, per the correction
  above — CONTRIBUTING rule 4, and it must say that the parser now exists and
  that `xmp_metadata` still does not use it, so the next reader does not take
  the amendment as a promise.
- **`docs/plans/gaps/28-tinker-integration-decisions.md`**, whose XPS sizing
  sentence names the `Device` seam as the reason for L. Amended in place, dated,
  with the reason corrected and the size unchanged.
- **`docs/STATUS.md`**, where XPS moves from decided to built with its own row,
  and the test count, fingerprint count and fuzz-target count all move.
- **`README.md`**'s paragraph under gap 28's amendment, which currently says
  the first of the three is built and the other two are not.
- **`fuzz/README.md`**, whose target table and seed table each gain a row, and
  whose count goes from twenty to twenty-one — and the seeds are curated, not a
  campaign's working state, which is what `d9945a0` is about.
- **`.github/workflows/ci.yml`**'s per-PR fuzz job, whose comment names a target
  count and uses it to decide its own time budget. Gap 29 found it saying
  "fifteen" when there were twenty.
- **`docs/plans/gaps/README.md`** and **`00-execution-order.md`**, updated with
  this plan rather than after it.

`docs/plans/13-bindings.md` also enumerates the leaves and is **deliberately not
amended**, for the reason gap 29 recorded: that list is a publishing plan naming
the crates gap 26 dry-ran to crates.io, and adding to it is a claim about
publishing rather than a correction to a count.

## Dependencies

**Needs first — all landed:**

- [28](28-tinker-integration-decisions.md), for the decision this plan
  implements and the size it was agreed at.
- [29](29-cbz.md), for `tinker-pdf-zip`, for `ImageData::Compressed` and
  `png_embed`, for `ArchiveRefusal` and the `#[non_exhaustive]` that lets it
  grow without a break, for `bounds_ledger.rs`, and for the synthesis scaffold
  this plan copies wholesale.
- [09](09-tiling-patterns.md), [10](10-mesh-shadings.md) and
  [11](11-transparency-groups.md), because milestone 5 writes patterns,
  shadings, groups and soft masks and the only way to check what it writes is
  that this engine already reads them.
- [20](20-linearization-validation.md), for the qpdf CI job and for its finding
  that a **skipped** oracle test exits 0 and reads exactly like a pass.
- [24](24-fuzz-execution.md) M1–M4 for the fuzz toolchain. `cargo-fuzz` needs
  libFuzzer, which `x86_64-pc-windows-msvc` does not support; WSL2 with nightly
  is the local route, as five other plans now record.
- [25](25-wasm-determinism-leg.md) M1–M3 for the leg the fourteenth fingerprint
  is checked on.

**Needs, and is not in the repository:** real XPS documents, which milestone 1
obtains; and `mupdf-tools` on the CI runner, installed by the job rather than
vendored (ruling 9).

**Unblocks:** nothing structurally. [31](README.md), EPUB, reuses
`tinker-pdf-xml` and reuses **none** of the OPC layer, because EPUB's container
is OCF with `META-INF/container.xml` rather than OPC — which is the argument
against making OPC a crate and is recorded in both places.

**Amends, in the same commits:** the ledger sweep above.

## Risks

| Risk | Mitigation |
| --- | --- |
| An XPS keeps opening as a comic while the rest of the plan is built, because the discrimination is treated as part of the reader | Milestone 3, early and standalone; milestone 1 commits the failing tests so the defect has a name in the suite from the first commit |
| The XML parser bounds entity expansion instead of refusing DTDs, and one of the four named bombs survives | 9.3.2 [M2.71] makes refusal the conformant behaviour; four committed inputs, each asserted to be refused *by name* rather than merely refused |
| The build reaches glyphs before the writer can emit a Type0 font, and `Indices` is approximated through WinAnsi | Milestone 5 before milestone 7, with the ordering argued in the milestone notes; the exit criterion is a fixture whose cmap and WinAnsi **disagree** |
| Resource dictionaries are treated as advanced and deferred, so the first real file cannot be drawn | The design section quotes a real one-page package in which the only image is an `ImageBrush` behind a `{StaticResource}`; milestone 6's criterion is that whole file rendering |
| Only ECMA-388's namespace is accepted, so every file Windows writes is refused | Decision 3, measured on two real packages before any code; milestone 4's criterion names both dialects and two real files |
| The ODTTF key order is wrong, and the failure hides inside an expected `UnreadableFont` warning | Milestone 7 asserts the **de-obfuscated bytes**, not that a page drew; the arithmetic is already checked against a real file in this document |
| A transform or clip that will not parse defaults to identity, and a page draws plausibly in the wrong place | The refusal asymmetry is decided here, before any file exists — geometry-and-placement refuse, paint degrades — with a fixture for each half |
| A cap is set above what its own inputs can reach and never fires | Gap 18a M8's exact failure. Every new constant joins `bounds_ledger.rs`'s **existing** table and inherits `every_bound_can_fire`, which is the only thing in the workspace that has ever caught it |
| A per-element cap is treated as the total, and a page with a million path segments runs | `MAX_XPS_ELEMENTS` and `MAX_XPS_SEGMENTS` are totals, spent and never refunded — `5adf502`'s lesson, `MAX_TILE_WORK`'s, `MAX_SCRIPT_TOTAL`'s and `MAX_ZIP_INFLATED`'s |
| `MAX_XML_DEPTH` is assumed to bound visual nesting, and a remote resource dictionary recurses between parts | The two caps are separate by construction and the table says why; milestone 8's criterion is a cross-part cycle refused |
| `Archive::warnings` is read into a report and nothing asserts that it is — gap 29's own milestone-6 survivor | Milestone 3's truncated-name criterion is written in **both** directions, because every healthy fixture asserts an empty warning list and so would a build that reports nothing |
| An XML crate is added because the rule lived only in prose | Nine names denied in milestone 2, the milestone where the temptation exists, rather than in milestone 9 |
| The fixtures are hand-built from ECMA-388 and the first real file finds something — gap 29's own closing sentence | Milestone 1 is first, and the real files have already found seven things this plan would otherwise have got wrong |
| An `As built` that reads as "XPS works now" | The claim this plan can support is fixed-page markup with paths, glyphs, images and the five brushes, from a package whose parts are JPEG, PNG and OpenType. Structure, signatures, print tickets, 3D, TIFF, JPEG XR, ICC and N-channel colour are refused **by name**, and the `As built` says which of the milestone-1 corpus needed each of them |

## Progress — 18 August 2026, milestone 1

**Eight genuine XPS packages have landed**, under
`crates/tinker-pdf/tests/xps/`, with per-file provenance in a README beside
them, an inventory of all fifty-two parts that a test recomputes on every run,
and the two present-day failures pinned as `#[ignore]`d tests that fail when
run. Nine tests in `crates/tinker-pdf/tests/xps.rs`, seven of them green; the
workspace stands at **1 881**, up seven, because an ignored test does not count.

No reader code. That was the point of scheduling this first.

### Both routes were tried, and route 2 was blocked

**Route 1 worked exactly as this plan reported.** `ReachFramework` under
Windows PowerShell 5.1 with `-STA`, no printer and no elevation, six packages,
XPS 1.0 throughout.

**Route 2 could not be used, and here is precisely what was tried.**
`Get-WindowsOptionalFeature -Online`, `Enable-WindowsOptionalFeature -Online
-FeatureName Printing-XPSServices-Features -NoRestart` and `dism /online
/get-featureinfo` each answered *"The requested operation requires elevation"* —
DISM error 740. `mxdwdrv.dll` is absent from `System32`, `Get-Printer` lists no
XPS printer, `xpsrchvw.exe` is absent, and `Add-PrinterDriver "Microsoft XPS
Document Writer v4"` answers *"The specified driver does not exist in the driver
store"* even though two copies of `mxdwdrv.dll` sit under
`DriverStore\FileRepository`, because the INF is not staged. Elevation was not
obtainable non-interactively. So this plan's sentence — that the feature is not
installed and that enabling it needs elevation — is confirmed on this machine a
day later, and the feature name it quoted from documentation is the right one.

**A third Microsoft producer stands in for it, and it is not a hand-built
file.** The XPS Document API's object model — the "XPS Object Factory" coclass
`{E974D26D-3D9B-4D47-88CC-3872F2DC3585}`, served by `XpsServices.dll`
10.0.26100.8972 — **is registered on a stock Windows 11 install without the
printing feature**, which this plan did not know.
`IXpsOMPackage1::WriteToFile1` takes an `XPS_DOCUMENT_TYPE`, and
`XPS_DOCUMENT_TYPE_OPENXPS` makes Microsoft code write every byte of an OpenXPS
package. `tests/xps/to-openxps.ps1` declares the two interfaces from the Windows
SDK's own headers — vtable order and IIDs read out of `xpsobjectmodel.h` and
`xpsobjectmodel_1.h` rather than remembered — and calls it. Two `.oxps` files
came out.

So **both dialects are represented**, which is what route 2 was for. What is
**not** established is that the MXDW printer's bytes match the object model's,
and that is stated rather than assumed: the printer is a different component and
nobody here has seen its output. If a later milestone gets elevation, the cheap
check is whether an MXDW `.oxps` differs from `xpsom-image-and-text.oxps` in
anything beyond part GUIDs.

### The `.oxps` content-type question is settled: `xps-`, not `oxps-`

This plan left it open and made this milestone answer it. **Windows' OpenXPS
output uses ECMA-388 Table D–4's `xps-` strings, unchanged.** Measured on both
`.oxps` files:

```text
<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml" />
<Default Extension="fdoc"  ContentType="application/vnd.ms-package.xps-fixeddocument+xml" />
<Default Extension="fpage" ContentType="application/vnd.ms-package.xps-fixedpage+xml" />
<Default Extension="ODTTF" ContentType="application/vnd.ms-package.obfuscated-opentype" />
```

Byte-identical to the XPS 1.0 packages' content types. The sources reporting
`application/vnd.ms-package.oxps-fixeddocumentsequence+xml` are wrong about what
this producer writes, and
`both_dialects_are_represented_and_the_content_type_does_not_tell_them_apart`
asserts that **no package in the corpus contains the substring `oxps-` in its
content-types item at all**.

**So decision 3 stands, and its reasoning is now measured rather than assumed.**
The reader keys on the **namespace** — `http://schemas.openxps.org/oxps/v1.0`
against `http://schemas.microsoft.com/xps/2005/06`, on the elements and on the
relationship types alike — and treats the content type as corroboration.
[Two dialects, one reader](#two-dialects-one-reader)'s warning that *"the
obvious sniff is the wrong one, and it is recorded here because it looks
right"* is exactly right, and it is now right for a reason a test can check.

### The two failures, re-measured and pinned

This plan's report is **confirmed to the number**, on a package it did not
write. `wpf-image-and-text.xps` is one 816 × 1056 fixed page — 612 × 792 pt, US
Letter to the point — with a 32 × 32 PNG resource and a `<Glyphs>` run.
`Document::open` accepts it, `page_count()` is 1, `Page::size()` is
`(32.0, 32.0)`, and `ArchiveReport::warnings()` is **empty**. The markup, the
text, the ODTTF and the page size are discarded in silence.
`wpf-shapes-only.xps` — a fixed page of three filled `Path` elements, no image
part anywhere in the package — is refused as
`OpenError::UnsupportedArchive(ArchiveRefusal::NoImages)`, whose own
documentation reads *"a valid archive with no image entries"*, said about a
document that has a page.

Three tests carry it:

- `an_xps_with_a_raster_resource_is_not_a_one_page_comic` and
  `an_xps_without_a_raster_resource_is_not_refused_as_having_no_images` are
  `#[ignore]`d with the reason naming milestone 3, and **both fail when run with
  `--ignored`**, which was checked rather than assumed. They are the two
  milestone 3's exit criterion turns green.
- `today_an_xps_opens_as_a_comic_and_this_is_what_it_reports` **passes**, and
  asserts the wrong answers exactly. It exists so the defect is watched rather
  than described: when milestone 3 lands, that test fails, and it is deleted in
  the same commit that un-ignores the other two. A record of old behaviour that
  does not break when the behaviour changes is not a record — which is gap 29's
  milestone-6 survivor in a different costume.

`every_package_in_the_corpus_is_mis_read_today` sweeps all eight, and the
result is worse than this plan's two examples in a way worth writing down: the
page count is the count of **raster parts**, so `wpf-three-pages.xps` — a
three-page document — is refused as `NoImages`, and `wpf-tiled-brush.xps`
reports one page because its two brushes share one PNG. Five of the eight
refuse; three open as a one-page comic; **not one is read as the document it
is**, in either dialect. The plan's *"a ten-page report with sixty image
resources opens as a sixty-page book in filename order"* understates it: a
document with no raster at all does not open, and a document with one raster
opens as one page however many pages it has.

### What the real files showed that this plan did not predict

The full list is in `tests/xps/README.md`. Seven of this plan's predictions held
— `[Content_Types].xml` last, the `<Default Extension="ODTTF">` case mismatch,
`Indices=",53"`, the relative-and-absolute target pair, the namespace
divergence, the geometry, and the two failures. These are the ones it did not
have, each of which a fixture written from ECMA-388 would have got wrong:

- **The UTF-8 BOM is a WPF habit, not an XPS one.** [A real
  `.xps`](#a-real-xps-and-gap-29s-largest-debt) lists *"the UTF-8 BOM on every
  XML part"* among what opening a real file already found. True of WPF's output;
  **the object model writes no BOM on any part**. A reader that required one
  refuses every OpenXPS file Windows writes. Scope's *"UTF-8 and UTF-16 with BOM
  detection"* stands, but *detection* has to mean detection and not expectation.
- **A fixed page part may carry no XML declaration at all.** WPF's `.fpage`,
  `.fdoc` and `.fdseq` begin directly with their root element. Four spellings of
  the prolog appear across eight files, including `<?xml version="1.0"?>` with no
  encoding, and `encoding` spelled both `utf-8` and `UTF-8`.
- **A comment sits inside element content**, not in the prolog: `<!-- Generated
  by: Microsoft XPS Object Model, … -->` between `<FixedPage>` and
  `<FixedPage.Resources>`. Milestone 2's parser meets it on the first real file.
- **`ImageSource` and `FontUri` are relative in OpenXPS.**
  `../../../Resources/….png` in the markup, where XPS 1.0 writes
  `/Resources/….png` for the same part. [OPC is not "a ZIP with names in
  it"](#opc-is-not-a-zip-with-names-in-it) predicts a relative *relationship
  target* beside an absolute *markup* reference and quotes exactly that pair;
  the other combination is also real, so relative-reference resolution is owed on
  markup attributes too, resolved against the fixed page part's own name.
  Milestones 3 and 6 inherit it.
- **Two spellings of everything, in one corpus.** Abbreviated geometry as
  `M0,0L200,0 200,200 0,200Z` and as `M 0,0 L 200,0 200,200 0,200 Z`; colours as
  `#FF000000` and as `#000000`, the latter lower case. Both producers are
  Microsoft and they do not agree with each other.
- **Inter-element whitespace is real.** WPF writes newlines and four-space
  indentation inside `FixedPage.Resources`, with no `xml:space`.
- **The object model drops the defaults WPF writes** — `TileMode="None"`,
  `SpreadMethod="Pad"`, `ColorInterpolationMode="SRgbLinearInterpolation"` — so
  an attribute present in one dialect's twin of a page is absent from the
  other's.
- **Both ZIP methods appear, and neither producer is consistent about it.**
  `_rels/.rels` is **stored** in the WPF packages and **deflated** in the object
  model's; image parts are stored and ODTTF parts deflated in both.
- **Eight characters of text cost a 189 252-byte font part.** WPF's subsetter
  keeps a variable font's `gvar` table whole — 142 688 bytes of it — out of
  Cascadia Mono's 371 352. Well under `MAX_ZIP_ENTRY_BYTES` (128 MiB), but it is
  the measured figure for the ledger's *"the most any fixture in this repository
  legitimately spends"* column and for milestone 9's peak-memory record, and the
  shape matters more than the number: **a real XPS's font part is not
  proportional to the text on its page.**
- **A dialect conversion does not re-obfuscate the font.** The ODTTF part keeps
  its name, its GUID, its content type and all 189 252 of its bytes across the
  two `image-and-text` packages, so one de-obfuscation reference serves both
  dialects.

**No `.piece` item appears anywhere in the corpus**, so [Interleaving is refused,
with an escape hatch](#interleaving-is-refused-with-an-escape-hatch)'s condition
for building the case — *"if milestone 1's corpus turns up an interleaved
package, this plan is amended"* — is not met, and the refusal stands as written.

**Nothing was produced by anything that is not Windows.** The criterion asks for
one *"if one can be found"* and none was: no LibreOffice, Ghostscript or
Inkscape is installed here and nothing else on the machine emits XPS. Recorded
as owed rather than quietly dropped, and it is the one part of row 1 that is
short.

### The ODTTF key order is a reversal, and the B-notation is a trap

[Fonts: ODTTF is thirty-two XORs](#fonts-odttf-is-thirty-two-xors-and-the-order-is-the-whole-of-it)'s
permutation — *"B37, B36, B35, B34, B33, B32, B31, B30, B20, B21, B10, B11,
B00, B01, B02, B03"* — read against a last segment written
`B03B02B01B00-B11B10-B21B20-B30B31-B32B33B34B35B36B37`, is **exactly the sixteen
bytes of the hex string reversed**. Transcribing it as the B-names instead
transposed two pairs on the first attempt here, and the result was a font whose
first eight bytes were right, whose table tags read correctly as `DSIG`, `GDEF`,
`GPOS`, `GSUB`, and whose `searchRange`, `entrySelector` and `rangeShift` were
garbage — plausible enough to look like a different bug entirely. This plan's
judgement that the permutation *"is entirely unmemorable"*, and its decision to
check it against a real file before rather than after, is now confirmed from the
other side. The corrected arithmetic and the reference bytes for
`Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF` are in
`tests/xps/README.md`; milestone 7 asserts against them.

### What was committed, and under which precedent

172 868 bytes, under `fuzz/README.md`'s *"a tool's output on our input is ours
to commit"* reading: the content of every package — four coloured quadrants
under a white diagonal, three rectangles, two gradients, eight characters of
text — is authored in `make-corpus.ps1` in this repository, and only the font is
not ours. That font is **Cascadia Mono**, chosen because its own `name` table
carries the SIL Open Font License grant — *"use, study, copy, merge, embed,
modify, redistribute"*, with *"The requirement for fonts to remain under this
license does not apply to any document created using the Font Software"* — and
because it is the **only** font on a stock Windows 11 install that does. Every
other face in `C:\Windows\Fonts` is Monotype's or Microsoft's under terms that
do not say so, and none of them is used. `fsType` is `Installable`.

`.gitattributes` gains `*.xps` and `*.oxps` as binary, with the reason written
down: a normalised line ending inside a **stored** part would break its CRC-32,
and these files cannot be regenerated byte for byte, because both serialisers
mint a fresh GUID for every resource part and a fresh `Id` for every
relationship. A second run of the script is a different file. The README carries
a hash per file for that reason, and says so.

### The inventory, and a cross-check that came free

`INVENTORY.tsv` names all fifty-two parts with media type, ZIP method and both
sizes, written by `tests/xps/inventory.ps1` through .NET's
`System.IO.Compression`. `inventory_matches_the_packages` recomputes name,
method and both sizes through **`tinker-pdf-zip`** and compares every row, so
the inventory cannot drift from the files, and two independent ZIP readers have
to agree about all fifty-two.

That comparison also discharges something gap 29 could not: **it is the first
time this repository's archive reader has been pointed at an archive it did not
write.** It read all eight by the central-directory route, with **no warnings
and no leniency of any kind**, which is a small result and the one that was
owed.

The media-type column is deliberately **not** checked by that test: resolving
one is OPC 7.2.3.5's ordered algorithm and it does not exist yet. Milestone 3's
round-trip criterion is where the column becomes checkable, and it now has a
committed table of real answers to check against — including the
`Extension="ODTTF"` case mismatch in both dialects, and the `(none)` that
`[Content_Types].xml` itself resolves to, since it is an item and not a part.

`tinker-pdf-zip` is added to the facade's `[dev-dependencies]`. It is already a
normal dependency, so `xtask -- dag` sees no new edge; the manifest comment says
so, because a crate appearing in a manifest without its argument is the failure
`ALLOWED`'s own commentary records.

### What milestones 2 and 3 inherit

- **Two failing tests with names, and a passing test that must break.**
  Milestone 3 un-ignores the two and deletes
  `today_an_xps_opens_as_a_comic_and_this_is_what_it_reports` in the same
  commit. If that third test still passes afterwards, the discrimination did not
  happen.
- **Comments in element content, and no BOM.** Milestone 2's parser meets both
  on the first real file, and neither would be in a hand-written fixture.
- **Relative references in markup, not only in relationships.** Milestone 3's
  resolver and milestone 6's `ImageSource` handling both need it, resolved
  against the source part.
- **A real `<Default Extension="ODTTF">` to compare case-insensitively
  against**, in both dialects, out of `INVENTORY.tsv` rather than out of prose.
- **A corpus in which `[Content_Types].xml` is last in all eight packages**, and
  a test that says so, so a positional assumption fails in CI rather than on
  somebody's machine.
- **`scan::extent`'s uncharged budget is already fixed**, at `84ee3b7`, before
  this milestone landed. Milestone 3's criterion to *"confirm and fix or show
  not to exist"* is discharged; the finding held, and every package here takes
  the central-directory route, so none of them exercises the fix.

## Progress — 18 August 2026, milestone 2

**`tinker-pdf-xml` has landed**, as the eighth leaf and the first crate in this
repository that depends on nothing at all except `tinker-pdf-math` does.
`lib.rs`, `limits.rs`, `scan.rs`, `text.rs`, sixty tests beside the code and ten
more over milestone 1's real packages. The workspace stands at **1 959**.

### The defence is structural, and the tests say which kind it is

`<!DOCTYPE` is [`Error::DoctypeUnsupported`] **before one byte after it is
read**. That is the whole answer to entity expansion: billion laughs, the
quadratic-blowup variant, an external entity and an internal-subset parameter
entity are each committed, and each is asserted to be refused *by that name*.

The assertion that matters is `every_bomb_is_refused_as_a_doctype_and_not_as_a_cap`.
A test asserting only "the bomb is refused" would pass on a parser that entered
the grammar, expanded entities and hit `MAX_XML_TOKENS` on the way — which is a
defence that works until somebody tunes a cap. Naming the error is what says
the parser never enters the grammar that has the attack in it. The distinction
is exactly gap 17's: the refusal is the feature, and *which* refusal is part of
it.

### What milestone 1's real files changed about this parser

Every finding milestone 1 recorded became an assertion here, and two of them
would have produced a parser that refuses genuine files:

- **A reader requiring a BOM refuses every OpenXPS file Windows writes.** The
  object model writes none, on any part; WPF writes one on every part. Both
  are real, and `one_producer_writes_a_byte_order_mark_on_every_part_and_the_other_writes_none`
  holds the two apart.
- **A fixed page may carry no XML declaration at all.** Four prolog spellings
  appear across eight packages and one of them is nothing.
- A comment arrives **inside element content**, not in the prolog.
- Inter-element whitespace is real text with real line ends to normalise.
- Two spellings of the geometry and two of the colours, in one corpus.

That is the milestone-1 investment paying: none of these would have been in a
hand-built fixture, because a fixture author writes the file they already have
in mind. The ten tests in `crates/tinker-pdf/tests/xml_real_packages.rs` are
the only XML assertions in this repository whose inputs the repository did not
write.

### The bounds

| Constant | Fixtures | A dense fixed page | Cap | Proved to fire by |
| --- | --- | --- | --- | --- |
| `MAX_XML_DEPTH` | 256 | 24 | 256 | `nesting_past_the_depth_cap_is_refused_by_name` |
| `MAX_XML_ATTRIBUTES` | 256 | 24 | 256 | `more_attributes_than_the_cap_is_refused_by_name` |
| `MAX_XML_NAME_LEN` | 1 024 | 48 | 1 024 | a name past the cap |
| `MAX_XML_TOKENS` | 1 048 576 | ~92 000 | 1 << 20 | `more_events_than_the_token_cap_is_refused_by_name` |

All four join `bounds_ledger.rs`'s existing table and pass its five checks,
`every_bound_can_fire` included — the one thing in the workspace that catches
`MAX_JPX_WORK`'s failure.

**`MAX_XML_TOKENS` is a total and not a per-element cap**, and
`the_token_cap_is_a_total_and_not_a_per_element_cap` is what holds it there.
That is `MAX_ZIP_INFLATED`'s lesson in a second format: a per-item cap is not a
total once the file chooses how many items there are, and a document of a
million one-attribute elements needs no nesting at all.

Note that the first column equals the cap in every row, which is different from
gap 29's ledgers, where it was what the *ordinary* fixtures spend. Here the
fixture that drives a cap to its edge is counted, so the column reads "a
fixture reaches this" rather than "the fixtures leave this much headroom". The
headroom is the second column against the fourth, and it is an order of
magnitude in every row.

### The injection matrix

Fourteen defects, one at a time, each reverted before the next, the full
workspace re-run with `--no-fail-fast`. **All fourteen caught; none survived.**

| Defect | Caught by |
| --- | --- |
| DOCTYPE parsed rather than refused before it is read | six tests, including `every_bomb_is_refused_as_a_doctype_and_not_as_a_cap` |
| The depth cap counting one too many | `nesting_past_the_depth_cap_is_refused_by_name` |
| The depth cap never firing | the same |
| The attribute cap never firing | `more_attributes_than_the_cap_is_refused_by_name` |
| The token cap never firing | `more_events_than_the_token_cap_is_refused_by_name`, and the total/per-element test |
| An undeclared prefix resolving to nothing rather than refusing | three tests |
| **Bindings not unbound when their scope closes** | `a_prefix_is_resolved_from_its_own_scope_and_unbound_when_that_scope_closes` |
| **Duplicates compared only by spelling, not by expanded name** | `two_prefixes_bound_to_one_namespace_are_one_attribute_name` |
| **Duplicates compared only by expanded name, not by spelling** | `a_duplicate_attribute_is_refused_by_name` |
| An element name resolved as though it were an attribute name | `the_default_namespace_applies_to_elements_and_never_to_attributes`, and two more |
| A character reference accepting any scalar the encoder takes | `a_character_reference_naming_no_character_is_refused_by_name` |
| XML 1.0's `Char` production unchecked after decoding | the same |
| `]]>` in ordinary character data accepted silently | `a_cdata_close_in_ordinary_text_warns_and_the_text_is_kept` |

The two duplicate-attribute injections are the pair worth keeping. XML 1.0 and
Namespaces §5.3 are **different rules that share a name**: the first forbids one
spelling twice, the second forbids one expanded name twice however differently
it is spelled. Deleting either leaves the other passing every test written for
it, so a suite with one test for "duplicate attributes" would have caught only
whichever it happened to exercise. Two rules, two tests, two injections.

### The DAG and the denylist

`ALLOWED` gains `tinker-pdf-xml` as the **fourth amendment**, and its argument
is the unusual one: it needs nothing. Not `filters`, because there is no
compression in an XML document; not `math`, because there is no arithmetic past
counting. It is the second node in the table with an empty dependency list, and
the doc comment records what it was checked against rather than leaving "no
dependencies" to look like an omission.

`deny.toml` gains nine names — `quick-xml`, `roxmltree`, `xml-rs`,
`xmlparser`, `serde-xml-rs`, `minidom`, `sxd-document`, `libxml`, `rustyxml` —
because a parser is the single easiest thing in this plan to reach for a crate
for, and CONTRIBUTING rule 1 lived only in prose until gap 29 started writing
these names down.

The **twenty-first fuzz target**, `xml`, in the shape `zip_archive` established:
the control byte picks the bounds rather than the input, away from the shipped
defaults, because a 1 048 576-event cap cannot fire inside a fuzz iteration and
a target that used it would leave the crate's only total unexplored.

### Still owed

- **The campaign has not run.** libFuzzer is unavailable on
  `x86_64-pc-windows-msvc`; the target compiles and the WSL2 route gap 29
  milestone 6 established works, but the session belongs to milestone 9.
- **UTF-16 is decoded and no real package uses it.** Both producers write
  UTF-8. The UTF-16 tests are hand-built, and that is a hand-built claim about
  a path no measured file exercises — the same shape as gap 29's fixtures, in a
  corner rather than everywhere.
- **No non-Windows producer**, inherited from milestone 1 and not this
  milestone's to discharge.

## Progress — 18 August 2026, milestone 3

**An `.xps` no longer opens as a comic.** `Document::open` opens a ZIP **once**
and routes it by ECMA-388 E.3's three steps; `crates/tinker-pdf/src/xps/opc.rs`
is the package layer and `crates/tinker-pdf/src/xps.rs` is the discrimination,
the spine and the synthesis. Milestone 1's two `#[ignore]`d tests are green, and
`today_an_xps_opens_as_a_comic_and_this_is_what_it_reports` is **deleted** —
which was the point of writing it: it failed the moment the behaviour changed.
The workspace stands at **2 003**, up forty-four.

All eight real packages now open as the documents they are, in both dialects,
with the page counts their `PageContent` elements say and every page 612 x 792
pt from `Width="816" Height="1056"` at 18.1's 1/96 inch. Before this commit five
of the eight were refused as `NoImages` and three opened as one-page comics
showing a resource.

### The exit criterion forced the spine a milestone early, and that is a correction

Row 3 says *"milestone 1's two failing tests go green"*, and the first of those
tests is not satisfiable by a refusal:

```rust
let document = Document::open(package("wpf-image-and-text.xps"))
    .expect("today it opens; after milestone 3 it opens as a fixed document");
assert_eq!(page.size(), (612.0, 792.0));
```

`.expect` on an `Err` is a failure, so the only way that test goes green is if
the package **opens** and its first page is the size the markup states — which
is row 4's `FixedDocumentSequence` → `FixedDocument` → `FixedPage` resolution
and row 4's geometry. Milestone 1's own progress note reads *"either read as a
fixed document or refused by a name that is true"*, and the test it committed
permits only the first.

So this milestone reads the spine and synthesises one page per `FixedPage`, at
the size the markup states, carrying the neutral placeholder and
`XpsPageDefect::NotDrawn`. **Row 4 is amended rather than absorbed**: what it
still owes is the markup-order proof against a package whose document order and
filename order disagree, `/CropBox` and `/BleedBox` from `ContentBox` and
`BleedBox`, resolving a fixed page through its **media type** rather than
through its root element, and qpdf on the produced file.

### E.3 has a hole exactly where this milestone lives, and it is closed on purpose

The plan writes the test as *"all three, or it is not an XPS"* with the comic
path as the fallthrough. Taken literally, an XPS whose `[Content_Types].xml`
will not parse fails step 3 and is **paged as a comic** — which is this gap's
headline defect surviving in the corner where the package is damaged, and a
damaged package is the one an attacker writes.

The refinement, argued in `xps.rs`'s header and held apart by two fixtures:

- **No fixed representation at all** — a `.docx`, an `.odf`, anything else OPC —
  is "not an XPS" and falls through to the comic path, unchanged. That is the
  direction the plan is explicit about and it is untouched:
  `an_opc_package_that_names_no_fixed_representation_is_still_a_comic` builds a
  ZIP with a `word/media/image1.png`, a real `officeDocument` relationship and a
  content-types item, and it still opens as a one-page comic.
- **A fixed representation that is there and will not resolve** — an unparseable
  content-types item or relationships part, a target naming a part the package
  does not hold, an external target, a media type that is not the sequence's —
  is `ArchiveRefusal::UnreadablePackage`.

Step 2 is what keeps a comic a comic and it wants **both** of OPC's items, so
`a_comic_carrying_a_content_types_part_is_still_a_comic` never reaches a read at
all. That is the near miss the plan asked for, and
`recognising_a_comic_archive_costs_no_read` is the other half of "opens the
archive once": `xps::route` takes an `Archive` by value and hands the same one
back as `Routing::NotXps` with `inflated()` still zero.

### The empty-element trap, and milestone 1's corpus paying a second time

The first run of the content-types reader failed on **all eight real packages**
with `PackageDefect::Unreadable` — *"the part is not the markup it must be"*,
which is a perfectly plausible thing to say about a file you have mis-parsed.

`tinker-pdf-xml` documents in its header that an empty-element tag produces
**two** events, `Start` then `End`, *"so a caller matching on starts and ends
never has to special-case it"*. A caller that tracks depth from `Start` alone
reads `<Types><Default/><Default/>…</Types>` as a nest six deep, and everything
below the second `Default` falls into the "an element 7.2.3.2 does not define"
arm. The leaf's contract is right; the caller's reading of it was not.

A hand-written fixture with one `Default` element would have passed. Six is what
a real package carries, and that is the milestone-1 investment paying in a place
nobody planned for it: the fixtures in `tests/xps_opc.rs` are all written by this
repository, and every one of them was written **after** the corpus had already
found the defect.

### `MAX_XPS_PAGES` could not be deferred, and the pair is the interesting part

The bounds table schedules `MAX_XPS_PAGES` as *"FixedPages synthesised"*, which
reads like row 4's. Once the spine is read here the page count is
attacker-controlled here, and — this is the part worth writing down — **it is
not bounded by `MAX_XPS_PARTS`**: four thousand `PageContent` elements may name
**one** part between them. What stands in front of it is
`tinker_pdf_xml::limits::MAX_XML_TOKENS`, a million events in one part, so a
single 20 KB `FixedDocument` asks for a million pages.

Both constants land here, and so does the whole relation.

| Constant | Fixtures | A dense fixed document | Cap | In front of it | Proved to fire by |
| --- | --- | --- | --- | --- | --- |
| `MAX_XPS_PARTS` | 8 192 | 505 | 8 192 | `MAX_ZIP_ENTRIES`, 16 384 | `a_package_past_the_part_cap_is_refused_by_name` |
| `MAX_XPS_PAGES` | 4 096 | 200 | 4 096 | `MAX_XML_TOKENS`, 1 048 576 | `a_page_count_past_the_xps_cap_is_refused_by_name` |

Both fire at their **shipped** values against packages built past them — a real
8 193-part archive and a real 4 097-page `FixedDocument` — never against a
lowered copy of the constant, and each test also asserts that one fewer opens,
so the cap is what stopped it rather than something else about a large package.
`MAX_XPS_PAGES < MAX_XPS_PARTS < MAX_ZIP_ENTRIES` is a `const` block beside the
constants, so a build that breaks it **does not compile**. Both rows join
`bounds_ledger.rs`'s existing table and pass all five of its checks; the sweep is
now thirteen rows and `no_bound_refuses_a_dense_fixed_document` covers six.

`MAX_SYNTHESISED_PDF` is reused rather than duplicated, as the plan asks, and
`the_synthesised_document_fits_inside_what_was_charged_for_it` measures the
charge against the produced bytes at one, three and sixty-four pages.

**One cap is deliberately not a cap**, and it is written down where it is
declared: `MAX_PAGE_UNITS` bounds what a `FixedPage` may state as its `Width` or
`Height`. It allocates nothing, refuses nothing a document could want — 10 416
inches is about a sixth of a mile — and a page past it *degrades* to the book's
own size with `XpsPageDefect::SizeUnusable` rather than refusing. It is a sanity
check on a number the file chose, not a resource bound, and it stays out of the
ledger for that reason.

### What the real packages forced that this plan did not predict

- **The package relationship's target is relative in OpenXPS**, which puts
  relative-reference resolution inside E.3's *third step* rather than after it.
  [OPC is not "a ZIP with names in it"](#opc-is-not-a-zip-with-names-in-it)
  predicts a relative `Target` in a **part** relationships part beside an
  absolute markup reference, and milestone 1 added that markup references are
  relative in OpenXPS. Neither predicted `Target="FixedDocumentSequence.fdseq"`
  in `/_rels/.rels`, where WPF writes `/FixedDocumentSequence.fdseq`. A reader
  that resolved the package relationship by stripping a leading slash refuses
  every `.oxps` **before it has read a single part** — it fails the
  discrimination, not the payload, so the symptom is "your file is not an XPS"
  rather than a page that will not draw.
- **`_rels/.rels` is stored in WPF's packages and deflated in the object
  model's**, which milestone 1 measured and which decides how the read-once
  cache can be tested at all. `Archive::inflated()` is the only public measure of
  the budget and a **stored** entry spends none of it, so a cache test over a
  WPF-shaped fixture asserts `0 == 0` and passes with the cache deleted. The
  fixture in `resolving_a_part_twice_does_not_inflate_it_twice` is deflated and
  the test asserts `after_one > 0` before it asserts the second read is free.
- **The two producers disagree about attribute order.** WPF writes
  `Type Target Id` on a `Relationship`; the object model writes `Target Id Type`.
  Nothing here reads by position, but a fixture frozen from one producer would
  have let a positional reader ship.
- **`[Content_Types].xml` needs no special case.** 7.3.7's brackets fail
  6.2.2.2's `pchar` production, so `PartName::from_item` refuses it as a
  consequence of the grammar rather than by a name check —
  `every_item_name_in_the_inventory_round_trips` asserts that rather than
  skipping the item, because a reader that special-cased it by name would be
  papering over a validator that does not work.

### The inventory's media-type column is checked for the first time

Milestone 1 wrote fifty-two `media_type` values through .NET and could not check
them, because OPC 7.2.3.5's ordered algorithm did not exist. It does now, and
`inventory_matches_the_packages` resolves every one of them: forty-four parts
against a committed table, and eight `(none)` — one per package, each of them
the content-types item, which resolves to nothing because it is not a part.

The upper-case `<Default Extension="ODTTF">` is the case the column was worth
having for. It appears in **both** dialects against a part named `….ODTTF`, and a
byte comparison against `odttf` finds nothing.

### The injection matrix, and the one that survived

Twenty-seven defects, each applied alone, reverted before the next, the whole
workspace re-run with `--no-fail-fast`. Twenty-six were caught. **One survived**,
and the test that closes it is the most useful thing in this milestone's suite.

| Defect | Caught by |
| --- | --- |
| Step 2 needs only the content-types item, not `_rels/.rels` as well | `a_comic_carrying_a_content_types_part_is_still_a_comic` |
| Step 3 accepts any media type on the fixed representation's target | three, including `a_fixed_representation_naming_the_wrong_media_type_is_refused_by_name` |
| A broken OPC package falls through to the comic path | `an_xps_whose_package_relationships_part_is_damaged_is_refused_by_name`, and one more |
| Only XPS 1.0's namespace is a dialect | `every_package_in_the_corpus_opens_as_the_document_it_is`, and one more |
| Only XPS 1.0's fixed-representation relationship type is recognised | the same two |
| `..` is ignored when a relative reference is resolved | `a_page_relationship_resolves_three_levels_up_against_its_source_part`, and one more |
| A `Default` extension is compared byte for byte | `inventory_matches_the_packages`, and one more |
| A `Default` beats an `Override` | `an_override_beats_a_default_and_both_compare_case_insensitively` |
| Every percent-escape is decoded, not only the non-ASCII ones | three, including `a_non_ascii_part_name_round_trips_through_percent_encoding` |
| 6.2.2.2's percent-encoded-unreserved prohibition is dropped | `a_name_that_is_not_a_part_name_is_not_read_as_one`, and one more |
| **6.2.2.3's equivalence compares spelling rather than the case fold** | **nothing — the survivor, below** |
| 6.2.2.3's derivability half is dropped | `two_part_names_one_package_may_not_both_hold_are_refused_by_name` |
| A relationship `Id` may repeat | `a_relationships_part_that_repeats_an_id_is_refused_by_name` |
| One part may carry two `Override`s | `a_content_types_item_that_declares_one_thing_twice_is_refused_by_name` |
| `.piece` items are not recognised | `an_interleaved_package_is_refused_by_name` |
| A directory record is taken for a part name | `directory_records_are_not_parts` |
| A truncated name is taken as an ordinary part name | `a_truncated_part_name_is_unresolvable_and_an_untruncated_one_is_not` |
| The read-once cache re-reads on every resolution | `resolving_a_part_twice_does_not_inflate_it_twice` |
| `validate` does not count the parts | `a_package_past_the_part_cap_is_refused_by_name` |
| The page cap never fires | `a_page_count_past_the_xps_cap_is_refused_by_name` |
| A page may be any size the file states | `a_fixed_page_with_no_usable_size_takes_the_books_size` |
| 18.1's 1/96 inch is read as 1/72 | six, including both of milestone 1's |
| A synthesised page carries no warning | eight, including `an_xps_with_a_raster_resource_is_not_a_one_page_comic` |
| The archive's own warnings are not read into the report | `a_truncated_part_name_is_unresolvable_and_an_untruncated_one_is_not` |
| A `PageContent` that does not resolve is dropped | `a_page_content_that_does_not_resolve_keeps_its_number` |
| A fixed page part's root element is not checked | `a_fixed_page_part_that_is_not_one_is_a_named_placeholder`, and one more |
| Any descendant of a fixed document is a page, not only a child | `only_a_direct_child_of_a_fixed_document_is_a_page` |

**The survivor is 6.2.2.3 read as one rule when it is two.** Deleting the case
fold from `PartName`'s `PartialEq` — so `/a` and `/A` are different names —
failed **nothing in the workspace**, with three tests already written for the
clause. The reason is that the clause has two consequences and this suite only
covered one: a package may not *hold* `/a` beside `/A`, which `validate` checks
by folding names into a set and never comparing two `PartName`s at all; and a
*lookup* must find the part however the reference spells its case, which nothing
asked for, because every fixture spelled every reference the way the part was
named.

That is gap 29's milestone-5 lesson from a third direction. A positive assertion
cannot catch a weakened check: every reference in every fixture matched exactly,
so exact matching passed all of them. `a_part_resolves_however_a_reference_spells_its_case`
closes it in both places — through `Package::has` and through a `PageContent`
whose `Source` is spelled `PAGES/1.FPAGE` — and the injection was re-run
afterwards and is caught.

Five of the twenty-seven exist because the first pass exposed gaps rather than
defects: nothing had asked what happens to a **directory record** (neither
Microsoft serialiser writes one, and every general-purpose archiver does, so a
reader that took `Documents/` for a part name would refuse every repacked
package), nothing forbade a repeated relationship `Id` or a twice-overridden
part, nothing pinned that only a *direct* child of a `FixedDocument` is a page,
and nothing reached `MAX_PAGE_UNITS`.

### Still owed after milestone 3

- **Nothing on a page is drawn.** Every page is the neutral placeholder and says
  so by name. Milestones 6 to 8 are the markup and milestone 5 is the writer work
  they need.
- **A fixed document and a fixed page are resolved by relationship and by root
  element**, not by media type. Row 4's *"never by extension"* holds already —
  nothing here reads an extension for the payload — but the media-type
  corroboration ECMA-388 Table D-4 offers is not asserted for the two payload
  types, only for the sequence. Row 4.
- **`Package::relationships` re-parses.** The part's *bytes* are cached and the
  markup is not, so asking a part for its relationships twice parses twice. It is
  bounded by `MAX_XML_TOKENS` per call and costs no budget; recorded so it is a
  decision rather than an oversight.
- **An item that is not a part name refuses the whole package.** That is the
  design section's rule (*"a part name that is invalid, duplicated or derivable
  from another"* refuses at open) and it is stricter than ruling 2 would be on
  its own. No package in the corpus trips it, and OPC requires a space to be
  percent-encoded, so a producer that wrote a raw space into an item name would
  be refused where a leaner reader would simply not resolve that one part. Named
  here so row 4 can revisit it if a real file turns one up.
- **No corpus package exercises the recovery route, `.piece` items, a duplicate
  name or a truncated name**, so every fixture for those is hand-built. That is
  gap 29's shape in a corner rather than everywhere, and it is the honest limit
  of what eight conforming packages can prove.
- **Zip64 is still written and not proven**, inherited from the plan's note on
  `tinker-pdf-zip`; no package here is one.
- **`docs/STATUS.md` still says 1 872 tests**, and the leaf-count sweep, the
  README rows and the fingerprint are milestone 9's, per the plan's own ledger
  section.
- **The campaign has not run**, inherited from milestone 2. What did land is the
  two `.xps` seeds the plan asks for, in `fuzz/corpus/render_page/` — the
  whole-pipeline target is where `Document::open` routes a `PK\x03\x04`, so it is
  the only target the OPC layer is reachable from.

## Progress — 18 August 2026, milestone 4

**Row 4's remainder has landed**, in the shape milestone 3 narrowed it to: pages
in **markup order**, `/CropBox` and `/BleedBox` from `ContentBox` and
`BleedBox`, the payload parts routed by **media type** rather than by their root
element, and **qpdf** on the produced file. `crates/tinker-pdf/tests/xps_spine.rs`
and `crates/tinker-pdf/tests/xps_qpdf.rs` are new, the three XPS test binaries
now share `crates/tinker-pdf/tests/xps_support/`, and `PageBuilder` grew the two
box setters the mapping needs. The workspace stands at **2 027**, up
twenty-four.

### What the design got wrong, and how it was found out

**1. The `/CropBox` mapping is right and what it costs is written down
nowhere.** [Geometry](#geometry-196-inch-top-left-y-down) says `ContentBox` and
`BleedBox` *"map onto PDF's `/CropBox` and `/BleedBox` and are written when
present — one line each"*. The mapping is taken exactly as written. What the
plan does not say, and what a host meets on the first fixed page that carries a
`ContentBox`, is that **`Page::size()` is the crop box**:
`cos_pages::Page::display_size` returns the crop box's dimensions, which is what
every PDF viewer lays out. So an 816 x 1056 fixed page stating
`ContentBox="96,48,192,96"` reports 144 x 72 pt and renders at 144 x 72 rather
than at 612 x 792. Nothing is lost — `media_box()` still carries the whole page
— but the number a host asks for changes, and "one line each" reads as though it
could not. `a_content_box_and_a_bleed_box_reach_the_pdf_scaled_once_and_flipped_once`
asserts the consequence rather than leaving it to be discovered. No package in
milestone 1's corpus states either box, so nothing already measured moves.

**2. "One line each" is three conversions stacked, and each is the identity on
the only box a hand-written fixture would have had.** A page box carries a unit
(1/96 inch to 1/72), an origin (top-left with y down to bottom-left with y up)
and a *form* (10.3's `x,y,width,height` against PDF's `x0 y0 x1 y1`) — and on a
box at the page's own origin, `0,0,w,h`, the flip and the form are both the
identity. `page_box`'s doc comment enumerates what each mistake produces for the
fixture's own box: no flip gives `[72, 36, 216, 108]`, no scale
`[96, 912, 288, 1008]`, the scale twice `[54, 513, 162, 567]`, and reading the
four numbers as a PDF rectangle `[72, 720, 144, 756]`. Four wrong answers, four
distinct rectangles, and the fixture's box is deliberately neither square nor at
the origin so that every one of them is visible.

**3. The find: a payload part was parsed once per *reference*, and a reference
is not a part.**

`tinker_pdf_xml::Source::new` decodes the part and then walks **every character
of it** against XML 1.0's `Char` production before it yields a single event, so
it costs O(part) whatever the caller stops at — and milestone 3's reader called
it once per `PageContent` and once per `DocumentReference`. Nothing makes those
counts the part count, and milestone 3's own `MAX_XPS_PAGES` note says so in as
many words: *"four thousand `PageContent` elements may name **one** part between
them"*. Two consequences, and the second is worse:

- 4 096 pages naming one 128 MiB part — `MAX_ZIP_ENTRY_BYTES`, which this build
  already admits — is **512 GiB of scanning**, out of a package that deflates to
  a few hundred kilobytes.
- A `DocumentReference` whose document holds **no** `PageContent` pushes no plan
  and charges nothing against the page cap, so `MAX_XML_TOKENS` was the only
  thing in front of the sequence loop: a million references, each of them a full
  scan of whatever it names.

That is `5adf502`'s *"depth is not work once the recursion branches"* in a third
format, and this plan's own sentence about it: **a per-item cap is not a total
once the file chooses how many items there are.**

Two fixes, and **neither is a new constant**:

- **`xps::Payload` caches one parse per part.** Cached, the markup this
  synthesis parses totals the **distinct** parts it read, and the archive reader
  already bounds that from both sides — a deflated part against
  `MAX_ZIP_INFLATED` (1 GiB) and a stored one against the length of the file
  itself. A constant over it could never be the thing that stopped anything,
  which is gap 18a milestone 8's failure reached from the other side. A work cap
  would also have to refuse something conforming: a fixed document that shows
  one page part four thousand times is legal XPS and costs one part.
- **The sequence's reference count is bounded at `MAX_XPS_PAGES`**, because a
  sequence naming more documents than this build will synthesise pages cannot
  produce a document it would finish. A second thing an existing constant
  bounds, rather than a fourteenth row in `bounds_ledger.rs`.

Proved by a count and never by a clock, which is ruling 1's rule about how a
bound is shown to work. `ArchiveReport::parsed_parts()` is published for
`synthesised_bytes()`'s stated reason — a cache with no observable is a cache a
test cannot tell from its own absence — and `Archive::inflated()` cannot stand
in for it, because the **bytes** were already cached a layer down and it is the
**parse** that repeated. A thousand references to one part is three parses;
three references to three distinct parts is five; a comic archive is zero, which
is a real answer rather than a placeholder.

**4. `Package::relationships` re-parsed, which milestone 3 recorded as a
decision.** Closed, cached against the entry it came from, and `Package::parses()`
is the observable for the same reason — a test written against `inflated()` would
assert `n == n` and pass with the cache deleted, which is gap 29's milestone-6
survivor exactly.

### This milestone adds no bound, and here is the argument

Three new counts arrived and none of them wanted a constant of its own. The
markup total is bounded by the parts already charged to `MAX_ZIP_INFLATED` and
by the file's own length, once the cache exists. The sequence's reference count
is bounded by `MAX_XPS_PAGES`, which was already there, already publishes its
three numbers and already fires. `page_box` allocates nothing and refuses at the
fifth number, so a `ContentBox` of a million fields costs one linear pass over
an attribute the XML reader had already produced and bounded. `bounds_ledger.rs`
stays at **thirteen** rows, and `MAX_XPS_PAGES` gains a second firing site
rather than a twin.

### The decision milestone 3 left open: the refusal stands, and it is narrow

Milestone 3 flagged that *"an item that is not a part name refuses the whole
package"* is stricter than ruling 2 would be on its own, and asked row 4 to
revisit it *"if a real file turns one up"*. **No real file turns one up, so it
stands** — on the same footing as the `.piece` refusal, tied to evidence rather
than to taste.

What is new is the measurement that makes it a decision rather than an
inheritance. `the_invalid_part_name_refusal_is_narrower_than_it_looks` shows
that the names a general-purpose archiver leaves behind when a package is
repacked are **all valid part names** under 6.2.2.2 — `Thumbs.db`,
`Documents/1/.DS_Store`, `__MACOSX/._FixedDocumentSequence.fdseq` and a
percent-encoded space are admitted and the package opens — and it pins the four
shapes that are refused: a **raw** space, a backslash, a percent-encoded
unreserved character, and a segment ending in a dot. The raw space is the one
that would change this decision, because it is the only one a third-party
producer could plausibly write; nothing in the corpus carries one, and neither
Microsoft serialiser can, since both name every resource part with a GUID.

### What qpdf said

Four tests in `tests/xps_qpdf.rs`, in gap 29's house form: `RAN`/`SKIPPED`
printed so a skipped oracle cannot read as a pass, fixtures under
`CARGO_TARGET_TMPDIR`, the `oracle!` macro. qpdf 12.3.2, and its output format
was read off the tool before anything was asserted against it — gap 29's
milestone 5 had to rebuild these assertions against `--show-pages`'s real shape
after assuming one, so the page objects here are parsed out of qpdf's own dump
rather than predicted from the writer's numbering.

- **`--check` is clean**, on three real packages' synthesised documents —
  `wpf-three-pages.xps`, `wpf-image-and-text.xps` and `xpsom-gradients.oxps`, so
  both dialects — and on the same document saved back through
  `Document::editor().save()` in both write modes. Linearised, qpdf also says
  *"File is linearized"*, which is gap 20's job speaking about gap 30's output.
- **`--show-pages` puts the pages in markup order.** 612 x 792, 300 x 450,
  150 x 225 for a document whose parts are named `p10`, `p1`, `p2`. Natural
  order would have said 450, 225, 792; lexicographic 450, 792, 225; the
  archive's own storage order 225, 792, 450. Four orders, all distinct, which is
  the whole reason the fixture exists.
- **`--with-images` finds no image XObject on any page**, which is this gap's
  headline defect said from outside: the same file used to open as a one-page
  comic whose page *was* a resource.
- **The two boxes are in the file as written** — `/CropBox [ 72 684 216 756 ]`
  and `/BleedBox [ 36 18 576 774 ]` — and a page that stated neither carries **no
  key at all** rather than one equal to the media box. That is the half this
  engine cannot check about itself: its own page reader normalises a rectangle
  and clips a crop box to the media box on the way in, so a box written back to
  front, or written where the file stated none, reads back correct.

**And qpdf exposed a CI hole that is gap 29's rather than this milestone's.**
That plan's row 5 says its criterion holds *"through the CI job
[20](20-linearization-validation.md) already built"*, and that job runs
`-p tinker-pdf-cos --test qpdf_oracle` and nothing else — so `tests/cbz_qpdf.rs`
has only ever run under `cargo test --workspace`, where a skipped oracle exits 0
and reads exactly like a pass. That is the precise failure gap 20 built the grep
for, surviving beside the grep. `qpdf-linearization` gains a second step running
**both** container oracles with the same SKIPPED-checked-first shape.

### The injection matrix, and the one that survived

Twenty-four defects, each applied alone, reverted before the next, the whole
workspace re-run with `--no-fail-fast`. Twenty-three were caught. **One
survived**, and it is the same lesson this gap has now learned three times.

| Defect | Caught by |
| --- | --- |
| Page order taken from the part names rather than the markup | five, including `pages_come_in_markup_order_and_in_no_order_over_their_names` and qpdf |
| Markup order reversed | eight, including both qpdf tests and `the_real_multi_page_package_has_one_page_per_page_content` |
| A `PageContent` that does not resolve dropped rather than placeheld | `an_unresolved_page_keeps_its_place_in_markup_order`, and one more |
| A `PageContent` naming the wrong media type dropped | three, including `a_fixed_page_is_routed_by_media_type_and_not_by_its_name` |
| `/CropBox` written from `BleedBox` rather than `ContentBox` | three, including qpdf |
| A page box left in XPS units, so the scale is never applied | `a_content_box_and_a_bleed_box_reach_the_pdf_scaled_once_and_flipped_once`, and qpdf |
| A page box scaled twice | three, including qpdf |
| A page box scaled and not flipped | the same two |
| A page box read as PDF's `x0 y0 x1 y1` rather than 10.3's `x,y,w,h` | the same two |
| A box that will not read silently dropped rather than named | `a_page_box_that_will_not_read_is_named_rather_than_defaulted` |
| A page that stated no box given one equal to the media box | three, including qpdf |
| The boxes never reaching the page | three, including qpdf |
| 14.11.2's reduction to the media box dropped | `a_bleed_box_that_runs_off_the_page_is_reduced_to_the_page` |
| A fixed page routed by extension rather than by 7.2.3.5 | three, including milestone 3's own `a_part_resolves_however_a_reference_spells_its_case` |
| A fixed page's media type not checked at all | three |
| A fixed document's media type not checked at all | `a_document_reference_naming_something_that_is_not_a_document_is_named` |
| The second warning a page owes dropped | `a_page_box_that_will_not_read_is_named_rather_than_defaulted` |
| The placeholder painted white rather than the neutral grey | `every_page_is_the_neutral_grey_rather_than_white` |
| The placeholder not painted at all, so a page is blank | the same |
| `Package::relationships` re-parsing on every ask | `asking_a_part_for_its_relationships_twice_parses_once` |
| `/CropBox` and `/BleedBox` never written by the builder | four, including `a_page_carries_the_boxes_it_was_given_and_no_others` and qpdf |
| **The payload's `children` cache never used** | **nothing — the survivor, below** |
| The payload's page-geometry cache never used | `a_part_named_by_a_thousand_references_is_parsed_once` |
| The sequence's reference count not bounded | `a_sequence_naming_more_documents_than_the_page_cap_is_refused_by_name` |

**The survivor is one test standing for two caches.** `Payload` holds two of
them — a part's children and a fixed page's geometry — and they are two pieces
of code. `a_part_named_by_a_thousand_references_is_parsed_once` was written for
both and could only see one: its fixture names one `FixedDocument` **once**, so
`Payload::children` was called once per distinct part whether it cached or not,
and the parse count did not move when the cache was deleted. The geometry half
was covered, because a thousand `PageContent` elements name one page part.

That is gap 29's milestone-5 lesson arriving for the third time in this gap —
after `PartName`'s case fold in milestone 3 and the two duplicate-attribute
rules in milestone 2 — and the shape is identical every time: **a positive
assertion cannot catch a weakened check, and one test cannot stand for two rules
that share a sentence.** The closing leg names one document three times from one
sequence, which 12.3.1 permits and which is three pages out of three parses; the
injection was re-run afterwards and is caught.

### Still owed after milestone 4

- **Nothing on a page is drawn**, inherited. Milestones 6 to 8 are the markup
  and milestone 5 is the writer work they need.
- **10.3's containment rules are not enforced.** This reader parses both boxes,
  converts them, and reduces them to the page per PDF 14.11.2; it does not
  refuse a `ContentBox` that lies outside the page's `BleedBox` or check either
  against the other. No fixture exists in either direction, and refusing on a
  rule nothing here has seen a producer break would be a claim about files
  rather than about this reader.
- **No real package states either box**, so every assertion about the box
  arithmetic is against a fixture this repository wrote. That is milestone 1's
  investment failing to reach one corner, and it is worth naming rather than
  letting the corpus's presence imply coverage it does not have.
- **`Package::index_of` is a linear scan** over every item, so resolving *n*
  references costs *n* x parts. With the two counts now bounded at 4 096 and
  8 192 that is at most about 67 million `PartName` comparisons for a package
  built to provoke it, and nothing in the corpus comes near it. Recorded rather
  than fixed: a name index changes the package layer's shape, and this milestone
  had no measurement asking for one.
- **`Page::size()` on a page that states a `ContentBox` is the content box.**
  Argued above. It follows from this plan's own mapping and from PDF's own rule,
  and if it is the wrong trade it is the plan's to reverse — one line, in
  `plan_page`.
- **`ArchiveReport::parsed_parts` is new public surface**, and milestone 9's
  ledger sweep is where it should reach whatever enumerates the facade's API.
- **The campaign has not run** (milestone 9), and **no non-Windows producer**
  (milestone 1); both inherited.
- **`docs/STATUS.md` still says 1 872 tests**, and the leaf-count sweep, the
  README rows and the fourteenth fingerprint remain milestone 9's, per this
  plan's own ledger section.
