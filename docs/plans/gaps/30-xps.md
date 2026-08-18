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

## Progress — 18 August 2026, milestone 5

**The writer's missing half has landed**, and nothing in this repository uses
it. `/ExtGState` with `/ca`, `/CA`, `/BM` and `/SMask`; form XObjects with
`/Group`; `/Shading` types 2 and 3 over type 2 and type 3 `/Function`s;
`/Pattern` PatternType 1; and a Type0 font over a CIDFontType2 descendant with
`/Encoding /Identity-H`, `/CIDToGIDMap /Identity`, `/W` from the font's own
`hmtx` and a `/ToUnicode` built from the text each glyph stands for. All of it
in `crates/tinker-pdf-cos/src/build.rs`, with twenty-three tests beside the
code, fourteen in the new `crates/tinker-pdf/tests/writer_graphics.rs` and
three more in `crates/tinker-pdf-cos/tests/qpdf_oracle.rs`. The workspace
stands at **2 068**, up forty-one.

Row 5's last clause — *"nothing in this milestone mentions XPS, which is the
test of whether it belongs in the writer"* — holds in the sense it was written
for. No fixed page, no brush, no `Indices`, no part name and no OPC anything
reaches this code; the only occurrences of the string are three provenance
citations of the form "gap 30 milestone 5", which is the register the rest of
`build.rs` already cites gap 29 in. The API is PDF's vocabulary throughout, and
the test of it is that the whole of `writer_graphics.rs` is written against
clause numbers rather than against markup.

### What the design got wrong, and how it was found out

**1. The missing half was not four dictionary writers. It was the resource
table.** [The writer's missing half](#the-writers-missing-half)'s table lists
what `DocumentBuilder` cannot emit and answers `stroke state` and
`paths and clips` with *"`raw` covers it, no API needed"*. That is true of
operators and **false of everything with a name**: `PageBuilder::raw` appends
operator bytes and cannot put an entry in `/Resources`, because a page's
resource dictionary was assembled inside `finish` out of two hard-coded lists,
`fonts` and `images`. So `/GS0 gs` written through `raw` names a resource no
page carries, and the first thing this milestone actually built was
`ResourceSet` — one value holding six kinds, with one assembly.

That is not a detail, because the same value is what a **form XObject** and a
**pattern cell** need. 8.10 and 8.7.3.1 each give them a `/Resources` of their
own, and three copies of the assembly would have been three places for a
resource kind to be forgotten.

**2. A form's resources make a cycle unreachable, and the plan schedules a cap
for it.** `add_form` gives the form the document's resources *at the moment it
is called*, which is `add_page`'s own rule. The consequence is that a form
cannot name itself — it is not registered until the call returns — so a form
referring to a form is a chain that only points backwards.
`MAX_XPS_RESOURCE_DEPTH` and `MAX_XPS_VISUAL_DEPTH` still have work to do on
the **reading** side, where a `{StaticResource}` graph is the file's to choose;
on the writing side the cycle cannot be constructed, and
`a_form_carries_its_geometry_its_content_and_the_resources_before_it` asserts
both halves rather than leaving the property to be rediscovered.

**3. The Type0 row lists four entries and there are five.** 9.7.3's
`/CIDSystemInfo` has to say `Identity` ordering, and a descendant claiming some
other ordering while its Type0 parent says `/Encoding /Identity-H` is a font
whose two halves disagree about what a code means. Nothing in this engine reads
that entry — `cos/font.rs` keys on `/CIDToGIDMap` and on the CMap — so it is a
statement only a third party can check, and qpdf is where it is checked.

**4. `/W` has a case the plan has no answer for: a font with no `hmtx`.** The
simple-font path invents `500` for every code it cannot measure, because
9.6.6.4 makes `/Widths` mandatory and leaves it nowhere else to go. A composite
font has `/DW`, so the honest answer is **no `/W` at all**, and that is what
`width_array` returns `None` for. Writing 500s would have been a number this
document made up presented as the font's, which is [Where a half-implementation
is worse than none](#where-a-half-implementation-is-worse-than-none)'s shape in
the one place the plan did not look for it.

**5. `/PaintType 2` is not written, and that is a decision rather than a
gap.** An uncoloured pattern is named through a `[/Pattern base]` colour space,
which has to be a `/ColorSpace` resource, and this writer has no API for one.
Emitting a `/PaintType 2` pattern that no page could then name would be a
feature that does not work. Recorded on the type itself, so the next reader
finds the argument where the omission is.

### This milestone adds no bound, and here is the argument

`bounds_ledger.rs` stays at **thirteen** rows.

Every input to this code is a value the caller already built in memory: an
`ExtGState`, a `Function`, a `&[Glyph]`. None of it is read from a file, so a
constant over any of it could never be the thing that stopped an attacker —
which is gap 18a milestone 8's failure reached from the other side, and the
same argument milestone 4 made for its three new counts. What bounds this
writer is whatever bounds the caller: milestone 6's `MAX_XPS_ELEMENTS` and
`MAX_XPS_SEGMENTS` are where a file-chosen count is refused, and they are that
milestone's to add.

One constant does arrive and it is deliberately **not** a ledger row.
`MAX_FUNCTION_DEPTH` is eight, and eight is not a resource budget — it is
**this repository's own reader's limit**, read out of `resources.rs`'s
`parse_function`, which stops at depth eight and returns nothing. A writer that
emitted a deeper nest would produce a file this engine cannot read back, and a
writer whose output its own reader refuses is not a writer. It is checked
iteratively rather than recursively, so a caller handing over a nest a thousand
deep is refused rather than overflowing a stack finding out, and
`a_function_nested_deeper_than_the_reader_will_walk_is_refused` proves both at
eight, nine and one thousand and eight. It sits beside `MAX_PAGE_UNITS`, which
milestone 3 kept out of the ledger for the same class of reason and wrote the
argument down in the same place.

### What qpdf said

Three tests in `crates/tinker-pdf-cos/tests/qpdf_oracle.rs`, which is the file
the `qpdf-linearization` CI job already runs — so this milestone needed no CI
change, unlike milestone 4, which found gap 29's container oracle had never
been in a job at all. qpdf 12.3.2.

- **`--check` is clean** on a single page using every construct at once, in
  four write modes: the builder's own output, a rewrite through
  `DocumentEditor`, a linearised save, and a save with compression and object
  streams on. The last is the one that matters for `maybe_compress`: the form's
  content, the pattern cell and the `/ToUnicode` CMap all become
  `/FlateDecode` streams and qpdf still reads every one of them.
- **The output format had to be read before it could be asserted against, and
  it was the same lesson twice.** qpdf **sorts a dictionary's keys**, so a
  `/Group` this writer emits in 11.6.6's own order prints as
  `<< /CS /DeviceGray /I true /S /Transparency >>`, and `/CIDSystemInfo` prints
  as `/Ordering (Identity) /Registry (Adobe) /Supplement 0`. Two of the first
  assertions written here were wrong for exactly that reason. And
  `--show-object` prints a stream's **dictionary** and the words `Object is
  stream.` where the data would be, so the `/ToUnicode` CMap is invisible to
  it; `--filtered-stream-data` is the flag that hands the bytes over, and it
  was found by running the tool. Gap 29's milestone 5 and gap 30's milestone 4
  each recorded having to rebuild an assertion against the real tool after
  assuming a format; this makes three.
- **`/ca 0.5` and `/CA 0.25` come back as two entries**, which is a distinction
  a case-insensitive reader would lose and which no round trip through this
  engine could have proved on its own.
- **The entries qpdf was pointed at are the ones this engine's own reader
  supplies a default for**, chosen on purpose: `read_shading` defaults a
  missing `/Extend` to `(false, false)`, `parse_function` defaults a missing
  `/Domain` to `[0 1]`, the tiling reader falls back to the cell's own size for
  a `/XStep` it cannot find, and `/I` and `/K` default to false. Every one of
  those would round-trip through this repository unchanged and be an empty
  dictionary to anybody else.
- `/W [ 1 [ 700 800 900 ] ]` — the font's own advances, run together, read back
  by a program that has never seen the font program.

### The injection matrix

Thirty-six defects, one at a time, each reverted before the next, the full
workspace re-run with `--no-fail-fast`. **All thirty-six are caught.** One of
them was caught only after a test was written for it, and that one is the
finding below.

| Defect | Caught by |
| --- | --- |
| `/ca` and `/CA` swapped | `an_alpha_from_the_builder_draws_what_a_hand_written_one_draws`, and one more |
| `/CA` never written | `fill_alpha_and_stroke_alpha_are_two_entries_and_not_one`, and `...reach_different_pixels` |
| `/BM` always `/Normal` | `a_blend_mode_from_the_builder_draws_what_a_hand_written_one_draws` |
| A soft mask's `/S` always `/Luminosity` | `a_soft_mask_is_a_group_a_kind_and_a_backdrop_and_none_is_not_absence` |
| A soft mask accepted over a form with no `/Group` | `a_graphics_state_that_is_not_one_is_refused` |
| `/BC` never written | the soft-mask test, and qpdf |
| `/SMask /None` collapsed to saying nothing | `a_soft_mask_of_none_turns_an_inherited_mask_off` |
| An alpha outside 11.6.4.4's range accepted | `a_graphics_state_that_is_not_one_is_refused` |
| `/I` and `/K` swapped | `an_isolated_group_from_the_builder_ignores_the_page_and_a_joined_one_does_not` |
| `/K` never written | `isolated_and_knockout_are_independent_flags` |
| `/Group` never written | four tests |
| A degenerate form `/BBox` accepted | `a_form_that_cannot_be_one_is_refused_and_leaves_nothing_behind` |
| A form's `/Matrix` never written | `a_form_carries_its_geometry_its_content_and_the_resources_before_it` |
| A form's `/Resources` never written | the same, and the radial-shading comparison |
| A radial shading written as type 2 | `an_axial_shading_is_type_two_and_a_radial_is_type_three`, and qpdf |
| `/Extend` written as the first flag twice | `a_shading_that_does_not_describe_a_gradient_is_refused`, and one more |
| `/Bounds` never written | `a_stitching_functions_bounds_decide_where_its_middle_colour_lands`, and qpdf |
| Each `/Encode` pair reversed | the same four |
| 7.10.4's arity check dropped | `a_shading_that_does_not_describe_a_gradient_is_refused` |
| The sub-functions never validated | `a_function_nested_deeper_than_the_reader_will_walk_is_refused`, and one more |
| The function depth guard never advancing | the same |
| `/XStep` written from the vertical step | `a_tiling_pattern_carries_its_cell_its_steps_and_its_matrix`, and qpdf |
| A pattern's `/Matrix` never written | `a_tiling_pattern_that_cannot_tile_is_refused` |
| A pattern with a zero step accepted | `a_composite_font_addresses_a_glyph_by_index_and_a_simple_font_cannot`, and one more |
| `/Encoding` written as `/WinAnsiEncoding` | `a_gap_in_the_glyphs_drawn_starts_a_new_width_run`, and one more |
| `/CIDToGIDMap` never written | `a_composite_font_is_identity_h_over_a_cid_font_with_an_identity_map`, and qpdf |
| `/DW` never written | `a_font_that_states_no_advances_gets_no_width_array`, and qpdf |
| `/W` from a nominal width rather than from `hmtx` | `a_font_whose_glyphs_stand_for_nothing_carries_no_to_unicode`, and one more |
| `/W` not scaled by the units per em | `the_width_array_is_the_fonts_own_hmtx_scaled_and_run_together` |
| `/ToUnicode` never written | `to_unicode_maps_many_glyphs_to_one_character_and_one_glyph_to_many` |
| A `bfchar` destination truncated to one code unit | the same, `page_text_comes_back_through_the_to_unicode_the_writer_wrote`, and qpdf |
| `/ToUnicode` deduplicated by destination text | the same three |
| **The last text a glyph is drawn with wins** | **nothing that asserts anything about `/ToUnicode`** — now `a_glyph_drawn_twice_with_different_text_keeps_the_first` |
| Glyph codes written one byte each | `a_composite_run_positions_its_glyphs_from_the_widths_the_font_states`, and three more |
| Glyphs accepted into a simple font | `a_radial_shading_from_the_builder_draws_what_a_hand_written_one_draws`, and one more |
| `/Shading` dropped from a page's resources | the same |

### The survivor, and the shape it shares with the last three milestones

**Reversing the `/ToUnicode` mapping to last-wins was caught by nothing that
asserts anything about `/ToUnicode`.** The one test that failed was the
kitchen-sink form fixture, and it failed on `the form has resources` — three
constructs away from the mapping, for reasons that have nothing to do with the
defect. A verdict of "caught" that rests on an unrelated assertion in a fixture
that touches everything is a verdict worth reading twice, and this one did not
survive reading.

The rule matters because it is the difference between a mapping that is a
property of the *font* and one that is a property of the *draw order*. A glyph
a caller draws once as "fi" and later as "fl" has to lose one — `/ToUnicode`
maps a code to one string and there is no spelling that says "either" — but if
which one survives depends on the order the page happened to emit them, then
the same document built from the same glyphs in a different order extracts as
different text. The thirteenth determinism fingerprint is a hash of exactly
that.

This is the fourth milestone running to find the same shape, and the four
together are worth stating as one rule: **when a thing has two independent
consequences, a test for one of them is not a test.** Milestone 2 had two rules
sharing a name (XML 1.0 and Namespaces §5.3 on duplicate attributes); milestone
3 had one clause with two consequences (OPC case folding, on holding a part and
on looking one up); milestone 4 had one cache with two halves (geometry and
children); this milestone has one mapping with two orders. Each had tests. Each
had tests for one side.

### Still owed after milestone 5

- **Nothing consumes any of this.** That is row 5's whole point and milestones
  6, 7 and 8 are where it is spent; it is named here so that "the writer can
  emit a gradient now" is not read as "an XPS draws one".
- **`/PaintType 2` and shading patterns are not written**, argued above. A
  gradient fills a *clip* through `sh` rather than a *shape* through a pattern,
  which is enough for 8.7.4.1 and is not the whole of 8.7.3.
- **Function types 0 and 4 are not written**, and `Shading` covers two of
  8.7.4.5's seven. Both enums are `#[non_exhaustive]` so each is an addition
  rather than a break. The structs are not, deliberately: a `#[non_exhaustive]`
  struct cannot be built with a literal outside this crate at all, which is
  why `CompressedImage` and `SoftMask` are not marked either, and
  `..ExtGState::default()` keeps a new field from breaking a caller.
- **A soft mask's `/TR` transfer function is not written.** 11.6.5.2 allows one
  and this engine's renderer reads one; no criterion here asks for it and
  nothing would have exercised it.
- **The `/ToUnicode` is `bfchar` throughout.** A `bfrange` says consecutive
  codes map to consecutive characters, which is true of a Latin subset and
  false of everything else, so emitting one on the evidence of two adjacent
  glyphs would assert a relationship the font never claimed. The cost is size,
  and it is a real cost for a document drawing thousands of distinct glyphs.
- **A form and an image share the `/XObject` namespace and the last
  registration under a name wins**, which is `add_image`'s existing rule
  extended rather than changed. `ResourceSet::dict`'s comment says so; nothing
  refuses the collision, because refusing in `add_form` while `add_image`
  shadows silently would be two rules for one namespace.
- **`Page::size()`, the determinism fingerprint and `docs/STATUS.md`** are
  untouched, as they have been since milestone 3; the ledger sweep is
  milestone 9's.
- **The campaign has not run** (milestone 9) and **no non-Windows producer**
  (milestone 1), both inherited.

## Progress — 18 August 2026, milestone 6

**A fixed page draws.** 11.2.3's abbreviated geometry and 11.2.1's element
syntax, 14.4's transforms, 14.3's canvases with composable transforms, clips
and opacity, 15's solid and gradient brushes, and 14.2's resource dictionaries
with `{StaticResource}` — `crates/tinker-pdf/src/xps/markup.rs`, `geometry.rs`,
`brush.rs` and `paint.rs`, with fifty-eight tests in
`crates/tinker-pdf/tests/xps_markup.rs` and three more in `xps_qpdf.rs`. The
workspace stands at **2 129**, up sixty-one.

**Two of milestone 1's eight real packages now render with nothing owed at
all** — `wpf-shapes-only.xps` and `wpf-three-pages.xps` — and two more do in
both dialects once the gradients are counted: `wpf-gradients.xps` and
`xpsom-gradients.oxps`. The remaining four owe exactly what milestones 7 and 8
own, by name, at the element: a `Glyphs` run and an `ImageBrush`.
`every_package_in_the_corpus_draws_and_says_what_it_does_not` is the table.

### What the design got wrong, and how it was found out

**1. Row 6's headline criterion cannot mean what it says, and the milestone
table is why.** It asks that *"the whole of the real one-page package in the
design section renders"*. That package is `wpf-image-and-text.xps`, and its
body is a `Path` filled with an `ImageBrush` behind a `{StaticResource}` and a
`Glyphs` run — which are **row 8's** and **row 7's**. Read literally the
criterion cannot be met before milestone 8, and a milestone whose exit
criterion depends on two later ones is a milestone that never closes.

What it can mean, and what
`the_real_one_page_package_from_the_design_section_renders` asserts, is that
the page **reads whole**: no page-level warning at all, the `Path`'s geometry
and transform arriving as the markup's own numbers, the `{StaticResource}`
**resolving** — and the brush behind it going grey under
`BrushUnsupported`, which is a *different name* from the
`BrushUnresolved` a dictionary miss would produce. That distinction is the
whole of what the criterion is worth: the four things the design section calls
*"not optional, from the first file"* that belong to this milestone all work,
and the two that do not are named rather than silent. Row 6 is amended in
place rather than absorbed, the way milestone 3 amended row 4.

**2. "A pull parser, not a tree" is right about the drawing and wrong about the
values.** The design section says a tree *"would allocate the whole of a fixed
page before anything looked at it, and a fixed page is the one part of this
format whose size is chosen by the file"*, and that is exactly the reason
`Canvas` and `FixedPage` are streamed here — each opens a scope with its own
content buffer and is folded into its parent at its end tag, and no drawable
tree exists at any point.

What cannot be streamed is a **property element**. XAML writes a value too big
for an attribute as a child named `Owner.Property`, and a value has to be
complete before it can be used: a `GradientStop` list is not a gradient until
its last stop, and a `ResourceDictionary` cannot answer a lookup until its last
entry. So those subtrees are materialised and nothing else is, each element
charged against `MAX_XPS_ELEMENTS`. The buffer earns its keep twice, because it
is also what makes document order inside a canvas irrelevant: a
`Canvas.RenderTransform` written *after* a child still wraps it.

**3. `MAX_XPS_ELEMENTS` cannot fire from one part, and that is the proof that
it is a total rather than a defect in it.** `tinker_pdf_xml::limits::MAX_XML_TOKENS`
bounds one part at a million events, of which at most half can be start tags —
so a single fixed page tops out at about 524 288 elements, comfortably under a
cap of 1 048 576. Only a **document** can reach it.
`a_page_past_the_element_cap_is_refused_by_name` therefore builds three parts
rather than one, and a per-page cap set to the same number would never fire at
all. That is the plan's own sentence arriving from the other side: *"a
per-element cap times a file-chosen element count is not a bound"* — and the
converse, that a total set where a per-item cap would sit is not a total.

**4. The bounds table does not say that `MAX_XPS_SEGMENTS` is also the
peak-memory bound, and that is what decides its value.** A segment has to be
materialised before it is written — a geometry's bounding box is what fixes a
transparency group's `/BBox` and what decides whether a canvas's children
overlap — so the worst case is one `Data` attribute holding the whole total. At
56 bytes a segment that is about 470 MiB, which sits under `MAX_ZIP_INFLATED`'s
1 GiB that this build already admits. So the cap is set just above the plan's
own dense-document yardstick (8 000 000) rather than comfortably above it: the
headroom a larger constant would buy is over a number nothing has ever reached,
and it would be paid for in the one number an attacker can drive.

**5. `XpsPageDefect::NotDrawn` had to be deleted, and the report needed a
second level.** The design section names three levels of refusal — package,
page and element — and milestones 3 to 5 built two of them, so every page of
every package carried `NotDrawn`. Keeping it here would leave a variant nothing
produces; keeping it *and* reporting element defects through it would make "this
page is a placeholder" and "this page drew and its text did not" one sentence.
`ArchiveWarning::XpsElement` is the second level, `XpsPageDefect` is now what
its name says, and `NotDrawn` is gone — which is milestone 3's own argument for
deleting `today_an_xps_opens_as_a_comic_and_this_is_what_it_reports` one level
up: a record of old behaviour that does not break when the behaviour changes is
not a record.

**6. The element-level rules are three bullets over two parsers, and one parser
has two consequences.** *"Geometry unreadable → the element is not painted"*
and *"a transform, a clip or an opacity that cannot be read refuses its
element"* are written as separate rules, and `Path.Data` and `Path.Clip` are
the **same** call to `geometry::abbreviated` returning the **same**
`GeometryError::Syntax`. One is a missing shape and the other is every shape
the clip existed to hide, and a build that answered them the same way would
pass whichever half its fixtures happened to cover.
`a_data_that_will_not_read_is_not_painted_and_a_clip_that_will_not_read_refuses`
is one test over two fixtures for that reason.

**7. `{StaticResource}` has no obvious chain, and without one neither of row
6's two guards could ever fire.** The bounds table justifies
`MAX_XPS_RESOURCE_DEPTH` with *"a dictionary entry may reference another"*, and
in XPS's actual element vocabulary there is no `<StaticResource>` element and
no brush whose *whole value* is a reference. 14.2.3 does permit a reference on
any attribute-settable property, and `Color` is the one where a reference names
a value of the same kind the entry would otherwise carry — so
`<SolidColorBrush x:Key="a" Color="{StaticResource b}"/>` is read as an alias
for whatever `b` names. Deliberately **only** `Color`: a reference on a
`Transform` names a transform and not a brush, and treating one as an alias
would hand a `MatrixTransform` back where a brush was asked for. Without that
reading, both guards are decoration with a `MAX_` prefix, which is gap 18a
milestone 8's failure by another route.

**8. PDF cannot spell two of `SpreadMethod`'s three values.** Row 6 lists
`SpreadMethod` beside `GradientStops` and `Transform` as though all three were
attributes to read. 8.7.4.5.3 gives a shading `/Extend` and nothing else, so
`Pad` is one flag pair and `Repeat` and `Reflect` are **replications**: the axis
grows eight periods each way and the function is stitched from seventeen copies
of itself, of which a reflected gradient runs every other one backwards. The
two differ in nothing but that predicate, which is why they are built from one
function and one closure rather than from two code paths that would agree by
accident — and why `the_three_spread_methods_are_three_shadings` asserts three
distinct files rather than three attributes parsed.

**9. Strokes are in no row of the milestone table, and a `Path` that only
strokes would have drawn nothing.** Row 6 names brushes for `Fill` and rows 7
and 8 never mention `Stroke`. A solid stroke is forty lines and the alternative
is a shape that silently vanishes, so it is built — and a **gradient** stroke
takes the placeholder grey with `BrushUnsupported`, because a shading pattern
is what one needs and milestone 5 records in as many words that it writes none.
That is [07](07-stroked-patterns.md)'s headline defect said from the other
side: its gradient-stroked rule painted solid black, silently, and the first
stop of the gradient would have been exactly as plausible and exactly as wrong.

### The bounds

| Constant | Fixtures | A dense fixed document | Cap | In front of it | Proved to fire by |
| --- | --- | --- | --- | --- | --- |
| `MAX_XPS_ELEMENTS` | 1 048 575 | 400 000 | 1 048 576 | `MAX_XPS_PARTS` x `MAX_XML_TOKENS`/2 | `a_page_past_the_element_cap_is_refused_by_name` |
| `MAX_XPS_SEGMENTS` | 8 388 608 | 8 000 000 | 8 388 608 | `MAX_ZIP_ENTRY_BYTES`/2, one path | `a_geometry_past_the_segment_cap_is_refused_by_name` |
| `MAX_XPS_RESOURCE_DEPTH` | 16 | 2 | 16 | `MAX_XML_TOKENS`, chained | `a_static_resource_chain_past_the_depth_cap_is_named` |

All three fire at their **shipped** values against real packages built past
them — a three-part document of 1 048 578 elements, a single `Data` of
8 388 609 segments in an archive that deflates to a hundred kilobytes, and a
chain of seventeen aliases — and each test also asserts that one fewer opens,
so the cap is what stopped it rather than something else about a large file.
`bounds_ledger.rs` is now **sixteen** rows and `no_bound_refuses_a_dense_fixed_document`
covers nine.

**The third row is here rather than in milestone 8** because row 6's own
criterion asks for *"a depth cap and a cycle refused rather than recursed"*,
and those are two rules: a chain of twenty distinct keys is not a cycle and a
cycle of two is not deep. They answer under different names —
`BrushTooDeep` and `BrushCyclic` — and deleting either leaves the other passing
every test written for it, which is why the injection matrix carries both.

**Two counts arrived and neither wanted a constant.** The painting cache is one
`Drawn` per **part**, for the reason milestone 4 built its own two: a
`FixedDocument` may show one page part four thousand times and `Source::new`
walks every character of a part before it yields an event. Cached, the markup
this synthesis paints totals the distinct parts it read, which the archive
reader already bounds from both sides. And the overlap test a canvas opacity
needs is quadratic in the child count, which `MAX_XPS_ELEMENTS` already bounds
and which stops at the first pair that covers each other.

**The cache has an observable, and it is the element total itself.** Milestone
4's rule — *"a cache with no observable is a cache a test cannot tell from its
own absence"* — is satisfied without a counter here:
`a_part_shown_on_a_thousand_pages_is_painted_once` builds a two-thousand-element
page part shown a thousand times, which charges two thousand if it is painted
once and two million if it is not, and two million is past the cap. So the
package opens with the cache and is refused as `TooLarge` without it. 12.3.1
permits exactly that document.

### What qpdf said

Three new tests in `tests/xps_qpdf.rs`, qpdf 12.3.2, in the house form gap 29
established — `RAN`/`SKIPPED` printed so a skipped oracle cannot read as a
pass, fixtures under `CARGO_TARGET_TMPDIR`, the `oracle!` macro. Milestone 5
wrote the `/ExtGState`, the form XObject with its `/Group`, the `/Shading` and
the two `/Function`s and nothing consumed any of them; this is the first
document in this repository in which all of them are produced **from a file**.

- **`--check` is clean** on `wpf-shapes-only.xps` and `wpf-gradients.xps`
  synthesised, on a built page that uses a transparency group and a three-stop
  gradient at once, and on that page saved back through `DocumentEditor` with
  compression and object streams on and off.
- **The gradient is an axial shading over a stitching function**, followed from
  the page's `/Resources` rather than guessed at: `/ShadingType 2`,
  `/Coords [ 0 300 400 500 ]` — the markup's own `StartPoint` and `EndPoint` —
  `/Extend [ true true ]`, `/FunctionType 3`, `/Bounds [ 0.5 ]` and
  `/Encode [ 0 1 0 1 ]`.
- **The canvas opacity is a transparency group**, and qpdf prints it in its own
  sorted order: `/Group << /CS /DeviceRGB /I true /S /Transparency >>`, over a
  `/BBox [ 0 0 150 150 ]` that is the union of what the canvas drew, with
  `/ca 0.5` and `/CA 0.5` on the `Do`. **The file's own spelling is not qpdf's**
  — this writer emits `/Bounds [0.5]` with no spaces inside the brackets and
  qpdf prints `[ 0.5 ]` — so a test written against the bytes would have passed
  over the tool that is supposed to be checking it. That is the fourth
  milestone running to record having to read the tool's output before asserting
  on it.
- **`--with-images` still finds no image XObject on any page.** This gap's
  headline defect, said from outside, on a file that used to open as a one-page
  comic whose page *was* a resource.

### What the real packages forced

- **Two spellings of the geometry, and neither producer is wrong.**
  `M0,0L200,0 200,200 0,200Z` from WPF against
  `M 0,0 L 200,0 200,200 0,200 Z` from the XPS object model, for the same page
  of the same document. `the_two_producers_spellings_of_one_rectangle_are_one_geometry`
  is one assertion and it is the only one in the geometry tests whose two
  inputs this repository did not write.
- **Two spellings of every colour**: `#FFDC143C` against `#dc143c`, eight
  digits upper case against six lower. A reader that took one length refuses
  half the corpus, and 15.2.4 defines two more forms nobody in it uses.
- **The object model drops what is default**, so `SpreadMethod="Pad"` and
  `ColorInterpolationMode` are present in one dialect's twin of a page and
  absent from the other's. An absent `SpreadMethod` is `Pad`, and a reader that
  required the attribute would refuse every OpenXPS gradient.
- **The image is behind a `{StaticResource}`**, which is the risk table's own
  entry: *"resource dictionaries are treated as advanced and deferred, so the
  first real file cannot be drawn"*. The mitigation held — the dictionary,
  the reference, the geometry and the transform all work on the first real
  file, and the only thing missing from that page is the raster itself.
- **A comment sits between `<FixedPage>` and `<FixedPage.Resources>`**, and
  inter-element whitespace is real. Both fall out of the streaming walk
  ignoring every event that is not a start or an end, which milestone 2's
  parser made possible and which no hand-written fixture would have exercised.

### The injection matrix, and the three that survived

Forty-one defects, each applied alone, reverted before the next, the whole
workspace re-run with `--no-fail-fast`. Thirty-eight were caught. **Three
survived**, and every one of them is the same shape this gap has now found in
six milestones running.

**Milestone 5's warning was taken literally**: a verdict of "caught" that rests
on a broad fixture failing for an unrelated reason is not a verdict, so every
row below was read for *which* assertion failed. Two of the rows here catch
sixteen and twenty-one tests each — the omitted-repeat form and the element
defects that never reach the report — and neither is counted on: each also
fails the one test written for exactly that rule.

| Defect | Caught by |
| --- | --- |
| Relative commands read as absolute | five, including `a_relative_line_names_a_displacement` |
| Absolute commands read as relative | twelve, including `an_absolute_line_names_a_point` |
| `Z` leaves the pen where the last command left it | `a_close_returns_the_pen_to_the_figures_first_point`, and one more |
| A figure re-opened after `Z` starts at the origin | the same two |
| The omitted-repeat form reads one operand | seventeen, including `the_omitted_repeat_form_re_applies_the_command` |
| The fill rule defaults to non-zero | four, including `the_fill_rule_defaults_to_even_odd_and_the_operators_differ` |
| `F0` and `F1` swapped | the same, and two more |
| The arc's two flags agree the other way round | `an_elliptical_arc_becomes_cubics_that_end_where_the_file_said` |
| The arc ends where the trigonometry landed | the same |
| A quadratic's controls sit halfway rather than two thirds | `a_quadratic_is_raised_to_the_cubic_that_draws_it` |
| A smooth cubic reflects nothing | `a_smooth_cubic_reflects_the_last_cubics_control_point` |
| `IsFilled` ignored | `a_figure_that_is_not_filled_is_still_stroked` |
| `IsClosed` ignored | `the_element_syntax_and_the_abbreviated_syntax_produce_the_identical_segment_list` |
| A transform that will not read defaults to the identity | `a_transform_that_is_not_six_numbers_refuses_its_element` |
| Path segments not charged against the total | `a_geometry_past_the_segment_cap_is_refused_by_name` |
| **A property element matched by prefix rather than by segment** | **nothing — a survivor, below** |
| `#AARRGGBB` read with the alpha last | three, including `a_colour_comes_in_four_hex_lengths_and_two_cases` |
| `#RGB` expanded by sixteen rather than seventeen | the same |
| `sc#` written straight, with no transfer function | `an_sc_rgb_colour_goes_through_the_srgb_transfer_function` |
| `Reflect` behaves as `Repeat` | `the_three_spread_methods_are_three_shadings` |
| A repeating gradient does not extend its axis | the same |
| An elliptical radial gradient drawn as a circle | `an_elliptical_radial_gradient_is_a_circle_in_a_scaled_space` |
| **Gradient stops taken in document order** | **nothing — a survivor, below** |
| `ContextColor` painted black | `a_context_colour_is_the_placeholder_grey_and_not_black` |
| A canvas opacity is **always** a transparency group | `a_canvas_opacity_over_children_that_do_not_overlap_is_not_a_group` |
| A canvas opacity is **never** a transparency group | `a_canvas_opacity_over_overlapping_children_is_a_transparency_group`, and both new qpdf tests |
| A `Clip` that will not read treated like a `Data` that will not | `a_data_that_will_not_read_is_not_painted_and_a_clip_that_will_not_read_refuses` |
| A transform that will not read leaves its element in place | `a_transform_that_is_not_six_numbers_refuses_its_element` |
| `{StaticResource}` searches outermost first | `an_inner_dictionary_shadows_an_outer_one` |
| `{StaticResource}` searches only the innermost dictionary | `a_canvas_child_finds_a_key_the_page_declared` |
| The `{StaticResource}` cycle guard dropped | `a_static_resource_cycle_is_refused_rather_than_recursed` |
| The `{StaticResource}` depth cap never fires | `a_static_resource_chain_past_the_depth_cap_is_named` |
| Elements not charged against the total | `a_page_past_the_element_cap_is_refused_by_name` |
| A `Glyphs` run skipped in silence | three, including `a_glyphs_run_is_named_rather_than_silently_skipped` |
| A canvas's resource dictionary never unbound | `an_inner_dictionary_shadows_an_outer_one`'s second leg |
| **One alpha serving the fill and the stroke** | **nothing — a survivor, below** |
| Markup from another vocabulary dropped and its children kept | `markup_from_another_vocabulary_is_skipped_whole_and_named` |
| The page's own `cm` not flipped | three, including `the_real_one_page_package_from_the_design_section_renders` |
| A part painted once per page rather than once | `a_part_shown_on_a_thousand_pages_is_painted_once` |
| Element defects never reaching the report | twenty-one, including every degradation test in the file |
| A page whose markup will not read is blank rather than grey | `a_page_that_will_not_read_is_grey_and_a_page_that_draws_nothing_is_white` |

### The three survivors, and the one shape they share

**1. `Owner.Property` compared as a prefix rather than as a segment.**
`Node::property_of` matches the name before the dot exactly, and comparing
prefixes instead failed nothing in the workspace — because every fixture in it
spells the owner exactly, which is what every real producer does. What a
prefix comparison lets through is not cosmetic: a `<CanvasBackdrop.Clip>` an
unknown vocabulary hangs on a page would be read as the **canvas's** clip, and
a clip that came out of markup this build does not understand hides whatever
it hides. `a_property_element_belongs_to_the_element_whose_name_it_carries`
closes it in both directions — the real property applies and the near miss does
not — because a positive assertion alone cannot catch a weakened check.

**2. A gradient's stops taken in document order.** 15.4.2 orders a gradient by
its stops' `Offset` values and says nothing about the order they are *written*
in. Every fixture in the corpus and in this milestone's own tests writes them
ascending, so a build that sorted by nothing passed all of them. And what it
would produce is worse than a refusal: 7.10.4 wants `/Bounds` strictly
increasing, this reader repairs a non-increasing offset by nudging it — which
is right for 15.4.2's *hard stop*, two stops at one offset — and that same
repair silently flattens an out-of-order gradient into a ramp between the wrong
two colours. `a_gradients_stops_are_read_by_offset_and_not_by_document_order`
asserts that the shuffled spelling and the ascending one are the **same file**.

**3. One alpha serving the fill and the stroke.** This is milestone 5's own
`/ca`-and-`/CA` finding arriving one layer up. That milestone built the writer
around 11.6.4.4's two parameters and proved they are two entries; here the
*reader* has to keep them apart, because an element carries one `Opacity` and
**two brushes**, each with an alpha of its own. Every fixture before this either
filled or stroked, so collapsing the two passed the lot.
`a_fills_alpha_and_a_strokes_alpha_are_two_numbers` gives one `Path` a
quarter-opaque fill and a three-quarter-opaque stroke and reads both numbers
out of the object model, then multiplies both by an element `Opacity` to show
that 14.3's opacity is a factor rather than a third alpha.

Each was re-run after its test was written and each is now caught by exactly
that test. **This is the sixth milestone in this gap to find the same shape**,
and milestone 5's statement of it needs no amendment: *when a thing has two
independent consequences, a test for one of them is not a test.* Two of the
three here are literally a pair — a fill alpha and a stroke alpha, an owner and
a property — and the third is a rule (offset order) whose only observable is a
document no producer writes.

### Still owed after milestone 6

- **`Glyphs` is not drawn** (milestone 7) and **`ImageBrush` and `VisualBrush`
  are not painted** (milestone 8). Both are named at the element, in the
  report, on every page that carries one.
- **`OpacityMask` refuses its element rather than being applied.** 14.3 allows
  one, this engine's renderer reads the `/SMask` that would carry it, and
  milestone 5 wrote the `/ExtGState` entry — what is missing is the brush, which
  is milestone 8's. Refusing rather than ignoring is the design section's rule
  for an opacity, and a mask ignored draws a whole shape where a sliver was
  meant.
- **11.2.2's `IsStroked` is read for its syntax and not honoured.** This
  milestone strokes a whole geometry or none of it; a per-segment flag would
  need a second segment list. `IsFilled` **is** honoured, which is the half a
  figure can express.
- **`Repeat` and `Reflect` are finite.** Eight periods each way is past the
  edge of any page a gradient of that period could be stated on, and past that
  the `/Extend` pads — but it is a replication rather than a repetition, and a
  gradient whose period is a hundredth of its shape would show it.
- **`MappingMode="RelativeToBoundingBox"` is tested and no real file uses it.**
  Both producers write `Absolute`, so
  `a_relative_gradient_is_stated_in_fractions_of_the_shape_it_fills` is a
  hand-built claim about a path no measured file exercises — gap 29's shape, in
  a corner rather than everywhere.
- **A gradient fills a clip through `sh` rather than a shape through a
  pattern**, which is milestone 5's own recorded limit. It is exact for a fill
  and has no spelling for a stroke, which is why a gradient stroke degrades.
- **Nothing renders a `Canvas` nested past `MAX_XML_DEPTH`**, and
  `MAX_XPS_VISUAL_DEPTH` — the cross-part cap — is milestone 8's, with the
  remote resource dictionary it bounds.
- **The campaign has not run** (milestone 9), **no non-Windows producer**
  (milestone 1), and **`docs/STATUS.md` still says 1 872 tests** — the ledger
  sweep, the README rows and the fourteenth fingerprint are milestone 9's.

## Progress — 18 August 2026, milestone 7

**Text appears.** 9.1.7.3's thirty-two XORs, 9.1.7's face selector, 12.1.3's
`Indices` grammar in full and 12.1's run drawn through milestone 5's Identity-H
font — `crates/tinker-pdf/src/xps/font.rs` and `glyphs.rs`, a `glyphs` arm in
`paint.rs`, and one new method in `crates/tinker-pdf-cos/src/build.rs`.
Forty-four tests in `crates/tinker-pdf/tests/xps_glyphs.rs`, two more in
`xps_qpdf.rs` and two in `writer_graphics.rs`. The workspace stands at
**2 177**, up forty-eight.

**The two packages milestone 6 left owing two things now owe one.**
`wpf-image-and-text.xps` and `xpsom-image-and-text.oxps` draw their `<Glyphs>`
run — seven distinct glyphs of Cascadia Mono out of a part that was thirty-two
XORs away from being a font — and `Page::text()` returns `"Page one"` from both.
What each still owes is `BrushUnsupported`, by name, at the element, and it is
milestone 8's.

### What the design got wrong, and how it was found out

**1. The writer's missing half was one method short, and the method it was
short of is the one that cannot be split in two.** Milestone 5 built
`PageBuilder::glyphs`, and row 5's own summary says the Type0 work is done. It
is not usable from here, for a reason that has nothing to do with fonts: that
method writes into a **page's** content buffer and records into a **page's**
glyph map, and the XPS painter has neither. A part's markup is painted **once**
and shown on as many pages as the document names it — milestone 6's own cache —
so the content is a buffer the painter owns, and the `/W` and `/ToUnicode` a run
owes are the **document's** rather than any one page's.

So `DocumentBuilder::glyph_run` arrives, and the shape it takes is the finding.
The obvious decomposition is two calls — one that formats the operators into the
caller's buffer and one that notes the glyphs — and that is precisely gap 30's
recurring failure written into an API: **two independent consequences, one of
them droppable.** A run that draws and is not recorded produces a page that
looks right, a `/W` that is absent and a `/ToUnicode` that is empty, and
`Page::text()` comes back blank on a document that renders perfectly. One call
does both, and the injection matrix's *"a run's glyphs are drawn and not
recorded"* row is what that decision is worth.

Row 5's *"nothing in this milestone mentions XPS"* still holds of the new
method: its vocabulary is 9.4.3's, its argument is a text matrix and a list of
placements in unscaled text space, and `writer_graphics.rs` could have been
written against it a milestone earlier.

**2. The advances a run positions from have to be the *reader's*, not the
font's.** `width_array` rounds `hmtx` to a whole thousandth of an em, because
that is what `/W` holds. XPS states each glyph's own position, so the `TJ`
adjustment between two glyphs is the difference between where the pen is and
where the file put it — and *where the pen is* is whatever the reader computed
from `/W`, not what the font's `hmtx` says. A pen model built on the unrounded
figure drifts a glyph at a time.

The real package shows it to the digit. Cascadia Mono is 1200 `hmtx` units over
2048 per em; `/W` publishes 586 thousandths; at `FontRenderingEmSize="24"` the
two are 14.064 and 14.0625, and the synthesised stream carries a `0.0625`
adjustment before each of the six glyphs after the first. Those six numbers are
the correction, and a build that computed from `hmtx` would emit none of them
and put the last glyph nine thousandths of a point out. So `glyph_run` computes
the adjustments from the same function `/W` is written from — one number, two
readers, and no way for them to disagree.

**3. `MAX_XPS_GLYPHS` had to exist, and the bounds table has no row for it.**
Gap 30's table has eleven rows and **not one of them sees a glyph**. A `Glyphs`
is *one* element and *no* path segments; the number of glyphs it draws is the
length of two of its attributes. `1;` is two bytes and one more glyph mapping,
so a single `Indices` in a part this build already admits — 128 MiB — is a
hundred million mappings, at about eighty bytes of value each.

The second half is where the cap goes rather than what it is. Charging it as
each glyph is *placed* would be charging after `indices()` had already
materialised the whole `Vec<Mapping>` — a cap checked after the allocation it
exists to stop. The mappings are separated by `;`, so the count is one more than
the separators and is known without allocating any of them, and that is where
the charge is: `tinker-pdf-zip`'s own posture, where a permit is what has been
promised. The one over-charge is a trailing empty mapping that turns out to be a
separator, which is one glyph out of two million.

**4. Row 7's *"implemented or refused by name, never ignored"* reads as one
rule over three attributes and it is three rules, and they do not agree.** What
decides each is what *ignoring* it would produce, which is the design section's
own asymmetry applied one element down:

- **`IsSideways`** rotates every glyph a quarter turn about its origin. Drawn
  upright it is a different picture in the same place, so the run is **not
  painted**.
- **An odd `BidiLevel`** is a right-to-left run, whose origin is its *right*
  edge. Drawn left to right it is the same picture somewhere else — the
  wrong-place failure the whole asymmetry exists for — so the run is **not
  painted**. An **even** level is a left-to-right run at some embedding depth
  and is ordinary text: refusing every non-zero level would refuse text this
  build draws exactly, so the even case is the *implemented* half and the test
  carries both.
- **`StyleSimulations`** adds a synthetic slant or weight to glyphs that are
  otherwise exactly the ones the file names, at exactly the widths and positions
  it states. That is the *paint* side of the asymmetry rather than the geometry
  side, so the run **is painted** and says so. Refusing it would drop a page of
  text to avoid drawing it unslanted, which is a worse answer than the one it
  was trying to avoid.

Three answers out of one sentence, and a build that gave all three the same one
would have been defensible on any single fixture.

**5. A cluster's `ClusterGlyphCount` is not "this mapping makes `n` glyphs".**
The plan describes `(m:n)` as *"precisely a many-to-many mapping"*, which is
true of what it means and silent about how it is written. 12.1.3 puts the counts
on the cluster's **first** mapping, and the `n − 1` mappings *after* it are the
rest of the cluster: each is a glyph with its own index, advance and offsets,
and none of them consumes a code unit. The first reading of it here produced
`n` glyphs out of one mapping and then read the following mappings as new
clusters — which draws the **right number of glyphs from the wrong text**, and
the only place that shows is the `/ToUnicode`.

That is this milestone's own instance of the rule, and it is the fourth
independent statement inside one attribute:
`a_cluster_maps_m_code_units_to_n_glyphs_and_both_counts_matter` distinguishes
three builds by glyph count alone — the right one draws three glyphs from
`"ABA"`, one that ignores the code-unit count draws four, and one that ignores
the glyph count draws two — and then reads the `/ToUnicode` for which text
landed on which of them.

**6. The fonts had to be resolved *before* the drawing walk, and the reason the
answer is a second pass is memory.** `Package::read_part` hands back a borrow of
the package and the painter is already holding one for the markup it is walking,
so a `FontUri` looked up mid-walk needs the page's bytes copied out first. A
fixed page part is the one part of this format whose size the file chooses, so
that copy is up to 128 MiB, transient, per part. A second walk over the same
part costs time proportional to it and **no** memory — and gap 30 has already
decided that trade once, in `MAX_XPS_SEGMENTS`'s own note that *"this cap is
also the peak-memory bound"*. `Fonts::load` is that pass, once per part, keyed
on the part and face a URI resolves to so a page naming one font twice loads it
once.

The pass had a bug worth one line, because it is a shape rather than a slip:
`let Ok(Event::Start(element)) = event else { break };` reads as a filter and is
a **terminator** — the first end tag ends the walk, and the corpus test caught
it on the first run because a real fixed page has an end tag before its
`<Glyphs>`.

**7. Milestone 1's transposed key takes three pairs, not two.** `tests/xps/README.md`
records the trap as *"two pairs transposed"*, with the symptom that the sfnt
version and every table tag survived and `searchRange`, `entrySelector` and
`rangeShift` were garbage. Reproducing that symptom needs all **three** of the
segment's two-byte groups — `B11B10`, `B21B20` and `B30B31` — left in the order
the part name spells them, because those three groups are exactly what the key's
bytes 6 to 11 come from and bytes 6 to 11 are exactly those three fields.
`the_key_is_the_guid_reversed_and_not_its_b_names_transcribed` builds that key
and asserts the whole symptom: version and table count intact, `DSIG` and `GDEF`
intact, all three fields wrong. The finding stands; the count was one short, and
this is the amendment.

### The bound

| Constant | Fixtures | A dense fixed document | Cap | In front of it | Proved to fire by |
| --- | --- | --- | --- | --- | --- |
| `MAX_XPS_GLYPHS` | 2 097 152 | 1 000 000 | 2 097 152 | `MAX_ZIP_ENTRY_BYTES`/2, one `Indices` | `a_run_past_the_glyph_cap_is_refused_by_name`, and `…_through_its_unicode_string_…` |

It fires at its **shipped** value against a real package built past it — one
`Indices` attribute of 2 097 153 mappings, four megabytes of markup that
deflates to a few kilobytes — and the test also asserts that a run of exactly
the cap **opens, warns about nothing and reaches the page**, so the cap is what
stopped the other one rather than something else about a large attribute. An
`Ok` on its own would not have said that: a package refused one level down
produces a placeholder page and an `Ok` too.

The yardstick is gap 30's own, read for text rather than for drawing: five
thousand glyphs a page is a page of small type with no white space on it, and
two hundred of those is a million against a cap at twice that. The margin is
narrow for `MAX_XPS_SEGMENTS`'s reason — **this cap is also a peak-memory
bound**, because a run is materialised whole before any of it is written (the
extent is what fixes the box a canvas opacity groups over). A `GlyphMapping`
is eighty bytes and a placed glyph forty-eight, both measured rather than
estimated, so the whole total is about 260 MiB with its content stream on top
— under `MAX_ZIP_INFLATED`'s 1 GiB that this build already admits.

`bounds_ledger.rs` is now **seventeen** rows and `no_bound_refuses_a_dense_fixed_document`
covers ten.

`the_glyph_total_is_a_total_and_not_a_per_run_cap` is the other half: two runs
of half the cap each are past it together, which a per-run cap set to the same
number would not see. That is `5adf502`'s finding for the third time in this
gap.

### What qpdf said

Two new tests in `tests/xps_qpdf.rs`, qpdf 12.3.2, in the house form — `RAN`/`SKIPPED`
printed, fixtures under `CARGO_TARGET_TMPDIR`, the `oracle!` macro. This is the
first composite font in this repository that came out of a **file**, and the
font program in it was thirty-two XORs away from a stream qpdf would have
refused outright.

- **`--check` is clean** on `xpsom-image-and-text.oxps` saved three ways —
  rewritten, linearised, and compressed with object streams on. The last is the
  one that matters: the `/ToUnicode` CMap and the embedded program both become
  `/FlateDecode` streams, and a subset that was *almost* a font would show up
  there rather than on the page.
- **The font, followed from the page's `/Resources` rather than guessed at**:
  `/Subtype /Type0`, `/Encoding /Identity-H`, a `/CIDFontType2` descendant with
  `/CIDToGIDMap /Identity` and `/DW 1000`, and `/Ordering (Identity)` — printed
  in qpdf's own sorted order, which milestone 5 recorded having to learn.
- **`/W [ 146 [ 586 ] 222 [ 586 ] 260 [ 586 ] 284 [ 586 ] 336 [ 586 ] 345 [ 586 ] 861 [ 586 ] ]`**
  — seven glyphs, each at Cascadia Mono's own 1200 `hmtx` units over 2048 per
  em, rounded to a thousandth, read by a program that has never seen the ODTTF
  and could not have de-obfuscated it if it had.
- **Seven `bfchar` entries out of eight characters**, because `"Page one"` has
  two `e`s and they are one glyph. That is the whole difference between a
  mapping keyed by *code* and one keyed by *character*, said by somebody else.
- **The subset is 20 749 bytes.** Milestone 1 measured that eight characters of
  text cost a **189 252**-byte font part, because WPF keeps a variable font's
  `gvar` whole; the document this synthesises carries a ninth of that under a
  `/BaseFont /WZKFDK+XpsFont`, because `subset` drops the tables the seven
  glyphs do not need. The two numbers belong together and neither was known
  before this milestone.
- **`--filtered-stream-data` again.** `--show-object` prints `Object is stream.`
  where the CMap would be. That is the **fifth** milestone running in this
  programme to have had to read the tool's output before asserting on it.

### What the real packages forced

- **`Indices=",53"` is the whole of 12.1.3's least obvious form and the corpus
  carries nothing else.** Both producers wrote it, unchanged, for the same page
  — an empty `GlyphIndex`, an advance, and seven characters after it with
  nothing said about them at all. Every other form in the grammar is exercised
  by fixtures this repository wrote, and that is recorded rather than glossed:
  no cluster, no offset, no exponent and no second mapping appears in any of the
  eight real packages.
- **The advance the file states is not the advance the font states**, and it is
  not close. 53 hundredths of the em against Cascadia Mono's own 58.59, for the
  first glyph only. A build that honoured the font and ignored the file would
  put every glyph after the first 1.344 units to the right, which on this page
  is a fifth of a character — visible, plausible, and exactly the sort of thing
  that reads as a font problem.
- **One relative reference and one absolute, for one font.** XPS 1.0 writes
  `FontUri="/Resources/….ODTTF"` and OpenXPS writes
  `"../../../Resources/….ODTTF"` for the same part of the same document, which
  is milestone 1's finding arriving where it was said it would.
  `both_dialects_resolve_the_same_font_and_draw_the_same_run` asserts the two
  produce the **identical** run, which is one assertion for the resolution and
  for everything downstream of it.
- **A `<Default Extension="ODTTF">` in upper case against a part named
  `….ODTTF`**, in both dialects, and a media type that is the thing that
  actually selects the path. The corpus cannot tell those two apart — its
  extension and its content type agree — so both lies are fixtures this
  repository wrote, in both directions.

### The injection matrix, and the seven that survived

Forty-one defects, each applied alone, reverted before the next, the whole
workspace re-run with `--no-fail-fast`. **Thirty-four were caught. Seven
survived**, and every one of them has a test now.

Milestone 6's warning was taken literally: a verdict of "caught" that rests on
a broad fixture failing for an unrelated reason is not a verdict, so every row
was read for **which** tests failed. Twenty-one of the thirty-four failed
exactly **one** test — the one written for that rule and nothing else — and the
five rows that fail seventeen or more (the key, the empty `GlyphIndex`, the
one-byte codes) each also fail the single test written for exactly that rule.

| Defect | Caught by |
| --- | --- |
| The GUID key is not reversed | thirty-six, including `the_real_part_de_obfuscates_to_the_font_milestone_one_measured` |
| The key is applied once rather than twice | seventeen, including `only_the_first_thirty_two_bytes_are_obfuscated` |
| Every byte of the part is XORed | twenty-one, including the same |
| A part name of any length is read as a GUID | `a_part_name_that_is_not_a_guid_has_no_key` |
| **The extension** selects the de-obfuscation | `the_content_type_selects_the_de_obfuscation_and_the_extension_does_not` |
| The media type is compared case-sensitively | `the_obfuscated_media_type_is_compared_without_regard_to_case` |
| A face other than the first is drawn in the first | `a_font_uri_naming_a_face_other_than_the_first_is_refused_by_name` |
| A part with no GUID is handed over unchanged | `an_obfuscated_part_with_no_guid_in_its_name_is_named_rather_than_unreadable` |
| **A `FontUri` resolves against the package root** | **nothing — a survivor, below** |
| The advance and the `uOffset` are swapped | four, including `an_advance_moves_the_pen_and_an_offset_moves_only_the_glyph` |
| A `uOffset` moves the pen as well as the glyph | the same |
| **A `vOffset` is measured the other way up** | **nothing — a survivor, below** |
| An advance is read as ems rather than hundredths | three, including the same |
| An empty `GlyphIndex` is `.notdef` rather than a `cmap` lookup | eighteen, including `an_empty_glyph_index_looks_the_code_unit_up_in_the_fonts_own_cmap` |
| `ClusterCodeUnitCount` ignored | `a_cluster_maps_m_code_units_to_n_glyphs_and_both_counts_matter` |
| `ClusterGlyphCount` ignored | the same, and one more |
| Every glyph of a cluster claims the cluster's text | the same |
| The code units past the last mapping are dropped | twenty, including `a_unicode_string_alone_addresses_glyphs_through_the_fonts_own_cmap` |
| A trailing empty mapping is always a glyph | `a_trailing_empty_mapping_is_a_separator_and_not_a_glyph` |
| A fifth field accepted | `indices_that_are_not_the_grammar_are_named_and_not_painted` |
| A glyph index wider than sixteen bits truncated | the same |
| A real parsed without the finiteness check | the same |
| **The glyph total not charged for the string's own glyphs** | **nothing — a survivor, below** |
| The glyph total not charged for the mappings | `a_run_past_the_glyph_cap_is_refused_by_name`, and one more |
| A cluster promising more glyphs than the list holds accepted | `cluster_counts_that_do_not_add_up_refuse_the_run` |
| `IsSideways` ignored | `is_sideways_is_refused_by_name_rather_than_drawn_upright` |
| An odd `BidiLevel` drawn left to right | `an_odd_bidi_level_is_refused_by_name_and_an_even_one_draws` |
| A `StyleSimulations` ignored | `a_style_simulation_is_named_and_the_run_still_draws` |
| The run's text matrix not flipped | three, including `a_run_is_not_drawn_upside_down` |
| An element `Opacity` does not reach the run | `an_element_opacity_multiplies_the_brushs_own_alpha` |
| A gradient over text drawn in its first colour | `a_gradient_over_text_is_the_placeholder_grey_and_named` |
| **A run contributes no box to its canvas's overlap test** | **nothing — a survivor, below** |
| **A `Glyphs` clip that will not read is not painted rather than refusing** | **nothing — a survivor, below** |
| A run's glyphs drawn and **not recorded** | eight, including `page_text_comes_back_from_the_unicode_string_through_the_to_unicode` |
| A `TJ` adjustment signed the other way | three, including `an_advance_moves_the_pen_and_an_offset_moves_only_the_glyph` |
| The pen model uses `hmtx` rather than the rounded `/W` | `the_real_run_draws_the_cmaps_glyphs_and_the_files_own_first_advance` |
| **A rise is left set when the run ends** | **nothing — a survivor, below** |
| A rise is never written | `a_v_offset_is_a_rise_and_the_rise_is_put_back` |
| Glyph codes written one byte each | seventeen, including `indices_names_a_glyph_the_cmap_would_never_reach` |
| **A run accepted into a simple font** | **nothing — a survivor, below** |
| The runs written outside a page never reach `finish` | eight, including the `/ToUnicode` and qpdf tests |

### The seven survivors, and the three shapes between them

**Two are a corpus that cannot tell two rules apart.**

**1. A `FontUri` resolved against the package root.** The only relative
`FontUri` in eight real packages is OpenXPS's `../../../Resources/….ODTTF` on a
page at `/Documents/1/Pages/1.fpage` — three segments down, so it climbs
**exactly** to the package root and both bases give the same part. So
`both_dialects_resolve_the_same_font_and_draw_the_same_run` — the assertion
written for precisely this rule — cannot see it, and no producer will ever
write the file that can.
`a_font_uri_resolves_against_the_fixed_page_part_and_not_the_package_root`
puts the font part **beside** the page and asserts both directions: a
page-relative reference finds a sibling, and the same reference does not reach
a part of that name at the root.

**2. A run accepted into a simple font.** Every font an XPS run reaches is
registered through `add_cid_font`, so from gap 30's side the composite check is
unreachable and nothing here could ever have exercised it. It is the writer's
rule and it belongs in the writer's tests:
`a_run_into_a_simple_font_is_refused_and_writes_nothing` is a pair — the
composite font takes the run and the simple one is refused having written
nothing — beside the one milestone 5 wrote for `PageBuilder::glyphs`.

**Two are a `contains` that a sign or a prefix satisfies.** Both are in one
test, and both are the same three characters.

**3. A `vOffset` measured the other way up**, and **4. a rise left set when the
run ends.** `a_v_offset_is_a_rise_and_the_rise_is_put_back` asserted
`contains("30 Ts")` and `contains("0 Ts")` — and `"-30 Ts"` contains the first,
and `"30 Ts"` contains the second. Two assertions, both vacuous, in the one
test written for the rule. The fixes are not tighter string matches: the
direction of an offset is a **picture**, so `a_v_offset_moves_the_glyph_up_the_page`
renders and reads three pixels; and a rise outliving its run is only observable
in a stream nobody bracketed, so `a_run_puts_its_rise_back_before_the_next_one_starts`
writes two runs into one buffer through `DocumentBuilder::glyph_run` and renders
the second on its own baseline. **A substring assertion over a signed number is
not an assertion**, and it is worth writing down because it looks exactly like
one.

**Three are a rule whose second consequence had no fixture.** This is the shape
the last six milestones each found, and it is the seventh.

**5. The glyph total not charged for the string's own glyphs.**
`MAX_XPS_GLYPHS` is charged in two independent places — once for the mappings
before they are parsed, once for the code units the mappings did not reach —
and the cap's own fixture states `Indices` and no `UnicodeString`, so it drove
one of them. `a_run_past_the_glyph_cap_through_its_unicode_string_is_refused_by_name`
drives the other, with two megabytes of `UnicodeString` and no `Indices` at all.

**6. A run contributing no box to its canvas's overlap test.** 14.3's opacity
is a transparency group only where two children cover the same place, and the
test reads the boxes the children pushed. A `Glyphs` that pushed none leaves a
canvas holding one box, which overlaps nothing — so a canvas of a `Path` and a
run drew without a group and every canvas test in the suite is about two
`Path`s. `a_run_contributes_its_box_to_its_canvass_overlap_test` is a pair, for
milestone 6's own reason.

**7. A `Glyphs` clip that will not read, not painted rather than refusing.**
`a_transform_or_a_clip_that_will_not_read_refuses_the_run` asserted the
warning's **name** and not its **consequence**, so a build that warned and then
drew the run unclipped passed it. That is milestone 6's finding — one parser,
two answers — arriving on a run: an unreadable clip that is ignored draws every
glyph the clip existed to hide. `a_clip_that_will_not_read_leaves_the_run_undrawn`
asserts the run is not there, and that a clip which **does** read leaves it
there inside one.

Each was re-run after its test was written and each is now caught by **exactly
that test**. Milestone 5's rule needs no amendment for the seventh milestone
running: *when a thing has two independent consequences, a test for one of them
is not a test.* What this milestone adds to it is the corollary the two vacuous
assertions found: **a test that cannot fail is not a test either**, and
`contains` over a number that can carry a sign is the cheapest way to write
one.

### A note on the campaign itself, because it went wrong once

The first run of the matrix was killed by the harness after twenty-seven
defects, **while the twenty-eighth was applied** — and that defect's patch was
a *deletion*, so the working tree looked ordinary and `git diff --stat` showed
nothing unusual. Twelve subsequent verdicts were recorded against a source file
missing six lines, and every one of them was invalid. Gap 30's own instruction
to *"verify after the run that no injection is left applied"* is the thing that
caught it, one milestone after it was written down.

The harness now writes an `APPLIED` marker beside its log before it patches and
removes it after it restores, so a killed campaign is distinguishable from a
finished one by a file rather than by reading the diff — and it classifies a
run with no `test result:` line as `COMPILE` rather than as a survivor, because
a build that did not run measured nothing whatever its exit code said.

### Still owed after milestone 7

- **`ImageBrush` and `VisualBrush` are not painted** (milestone 8). That is now
  the *only* thing either `image-and-text` package owes.
- **`IsSideways` and an odd `BidiLevel` are refused rather than implemented.**
  Both are drawable in principle — a quarter-turn text matrix and a run measured
  from its right edge — and neither is built, because neither appears in any
  package here and a layout this build cannot check is worse than a named
  refusal.
- **`StyleSimulations` is not simulated.** 9.3.6's render mode 2 with a stroke
  width would fake the weight and a sheared text matrix the slant, and both are
  numbers this engine would have to invent.
- **`DeviceFontName` (12.1.2) is not read.** It names a device font a printer
  may substitute; the embedded `FontUri` is still required and is what draws, so
  ignoring it changes no picture — but it is named here rather than left for
  somebody to find.
- **A gradient over text is the placeholder grey**, for the gradient stroke's
  reason: it needs a shading pattern and milestone 5 writes none.
- **The run's box is its advance and one em above the baseline**, not its ink.
  It decides two things — 14.3's overlap test and a `RelativeToBoundingBox`
  brush — and both are questions about where a thing is rather than which pixels
  it covers, but a descender or an overhanging glyph is outside it.
- **The `/ToUnicode` is `bfchar` throughout** and **`/PaintType 2` is not
  written**, both inherited from milestone 5.
- **Text extraction order is the order the runs are written in.** There is no
  `DocumentStructure`, which is a non-goal of the whole plan, and no attempt to
  re-order runs geometrically beyond what the extractor already does for a PDF.
- **The campaign has not run** (milestone 9), **no non-Windows producer**
  (milestone 1), and **`docs/STATUS.md` still says 1 872 tests** — the ledger
  sweep, the README rows and the fourteenth fingerprint are milestone 9's.

## Progress — 19 August 2026, milestone 8

**An `ImageBrush` paints.** `crates/tinker-pdf/src/xps/image.rs` resolves image
parts, `brush.rs` reads 15.3's grammar, and `paint.rs` turns the pair into a PDF
tiling pattern. Thirteen tests in the new `tests/xps_images.rs`, and the
workspace stands at **2 190**.

**All eight of milestone 1's real packages now render with nothing owed** — four
after milestone 6, six after milestone 7, eight here. The design section's own
one-page package, quoted at the top of this plan as the file that says why
resource dictionaries cannot be deferred, reads whole.

### Row 8 is amended, not claimed: `VisualBrush` is refused by name

Row 8 asks for `VisualBrush` "through a tiling pattern, bounded by
`MAX_XPS_VISUAL_DEPTH` **across parts**", and that is **not built**. The reason
is structural rather than a shortage of time: an `ImageBrush`'s cell is a *part*,
which [`image::Images`] resolves in a pass before the drawing walk for the same
reason the fonts are; a `VisualBrush`'s cell is a **subtree of markup**, so
painting one means re-entering the drawing walk from inside a brush, carrying
18.2's cross-part depth with it, while `brush.rs` is deliberately pure and knows
nothing of the package.

So the brush is `BrushUnsupported` and the shape keeps the placeholder grey,
which is the same answer every other unpainted brush gets. `MAX_XPS_VISUAL_DEPTH`
is **not** added to `bounds_ledger.rs`, because a cap over a thing nothing walks
is a constant that could never fire — gap 18a milestone 8's failure, reached from
the direction milestone 4 also refused it from. The bound arrives with the walk
or not at all. Milestone 6 amended its own row this way and the precedent is the
reason this is written down rather than quietly deferred.

### The two defects the tests caught, both in one matrix

Neither was subtle in hindsight and both drew *something*, which is the point.

**`markup::concat(first, second)` composes innermost-first**, and the transform
chain was assembled outermost-first. The picture landed 4 336 points down a
792-point page — off the sheet entirely, so the page rendered with the text on it
and no picture, which reads exactly like "the image brush is not implemented yet"
and had been the truth for two milestones. The fix is four lines and the comment
above them says which way round is wrong, because the wrong way compiles and
runs.

**The page height arrives already in points.** `plan.size` is scaled by
`UNITS_TO_POINTS` where it is built, so scaling it again inside the painter was
the same bug in a smaller place — and it survives the first fix, because both
errors move the picture in the same direction.

The corrected `/Matrix` is `[4.686846 0 0 -4.686846 75 717]`, and every number in
it is checkable by hand: the viewport's origin is `(0,0)` in element space, the
element translates by `(100,100)` XPS units, and 18.1 turns that into
`(75, 792 − 75)` points. That is why the test asserts the numbers rather than the
presence of a pattern.

### Three tests amended rather than deleted

All three asserted the `ImageBrush` was **not** drawn, which was true when they
were written.

- `an_image_brush_is_the_placeholder_grey_and_named` asserted
  `BrushUnsupported`. Its fixture names a part no package holds, so the answer is
  now `ImageUnresolved` — a different sentence about a different fault, and the
  two are separate variants precisely so a report can tell "this build cannot
  draw that" from "the file pointed at nothing". Renamed to say so.
- The corpus table said four packages owed `BrushUnsupported`. It now says none
  do, and the count stays in the test's own doc comment because that line is the
  one place in the suite that records how much of somebody else's format this
  build actually reads.
- **Milestone 1's pinned test is the interesting one.** Its last assertion was
  that the document *must not come back silent*, and that was right for five
  milestones: while anything on the page was owed, silence could only mean the
  original defect, an XPS read as a comic — which complains about nothing,
  because from `cbz.rs`'s side nothing went wrong. Now the page is complete and
  legitimately silent, and the assertion inverted.

  What separates the two silences is **not** the warning list. It is whether the
  page is the size the markup states and whether the picture is actually there.
  Both are now asserted and the second is asserted in pixels: the original defect
  produced a 32 × 32 page that was entirely the PNG, and a build that read the
  markup and drew none of it produces a 612 × 792 page that is entirely white.
  Neither passes.

### What the design got right and what it did not say

**Right:** the pass-through is inherited whole. Nothing in this milestone decodes
a picture. `png_image` is the one door and it chooses its own route;
`ImageData::Jpeg` places a JPEG verbatim. A page's peak cost is a multiple of the
part, which is gap 29's argument arriving in a second format with no second
implementation.

**Not said:** 13.4.1's resolution is load-bearing and the plan treats it as a
detail. An `ImageBrush` states its `Viewbox` in the image's **own** units, and the
real package says `Viewbox="0,0,32.004467,32.004467"` for a 32-pixel PNG — those
are the same thing only at 96 dpi. A reader that assumed 96 for a 300 dpi scan
would draw it at three times its size, and the arithmetic lives in
`Image::units` with the reason beside it.

**Also not said:** the five `TileMode`s are five rules and PDF has none of them.
8.7.3.1 gives a cell, two steps and a matrix, and no reflection at all — so
`FlipX` is a cell twice the image wide with the image drawn into it twice, and
`FlipXY` is four times and four drawings. `TileMode="None"` is the odd one: PDF
has no pattern that draws once, so the step is made larger than any page can be
and the shape's own extent does the clipping.

### The bounds

**No new row.** `bounds_ledger.rs` stays at seventeen, and the argument is
milestone 4's and milestone 5's: an image part is charged where every other part
is, by `MAX_ZIP_INFLATED` and `MAX_XPS_PARTS`, and the cell content this milestone
writes is bounded by the markup that named it — `MAX_XPS_ELEMENTS` for the brush
and at most four `Do` operators for the flips. `MAX_XPS_VISUAL_DEPTH` is argued
above.

### The injection matrix

Twenty-eight defects, one at a time, each reverted before the next, the full
workspace re-run with `--no-fail-fast`. The run was **stopped at defect ten** to
land this commit, and the harness left that defect applied — which the
`M8_APPLIED` marker caught immediately, and a verification pass over all
twenty-eight anchors confirmed the tree is clean before anything was committed.
That marker exists because milestone 5 and milestone 7 were each bitten by a
run killed mid-injection, milestone 7's by a *deletion* patch that left the tree
looking entirely ordinary.

**Completed in milestone 9: twenty-eight of twenty-eight run, twenty-seven
caught.** The nine below were caught in this milestone; the other nineteen ran
first thing in milestone 9 and are recorded in its own section, where ten
survived and eight tests were written to close them.

| Defect | Caught by |
| --- | --- |
| The viewbox and the viewport swapped | `the_viewbox_scales_to_the_viewport_and_not_the_other_way` |
| One scale for both axes | `the_two_units_attributes_are_read_separately` |
| The viewport's offset dropped from the matrix | the real-package render |
| The element's transform left out of the pattern matrix | `the_real_one_page_package_from_the_design_section_renders` |
| The composition order reversed | the same |
| 18.1's flip left out of the transform in force | the same |
| The scopes walked outermost first | the same |
| `TileMode="None"` given the cell's own step | `tile_mode_none_puts_one_picture_on_the_page` |
| `FlipX` given a cell one image wide | `each_flip_makes_the_cell_large_enough_to_hold_its_reflections` |

### Still owed

- **Nineteen injections**, above. Milestone 9's first task.
- **`VisualBrush`**, argued above, with `MAX_XPS_VISUAL_DEPTH` waiting on it.
- **A remote resource dictionary part** is not resolved, and row 8 asks for one.
  `ResourceDictionaryRemote` names it, which is where milestone 6 left it.
- **`ImageBrush.Transform` is parsed and composed, and no real package states
  one** — so that path is exercised only by fixtures this repository wrote.
- **The qpdf oracle has not been extended to a pattern.** Milestone 5 asserted
  `add_tiling_pattern`'s output through `qpdf_reads_back_the_states_the_groups_the_gradients_and_the_pattern`,
  so the writer's half is checked by a third party; the *painter's* half — that
  the pattern this milestone builds is the one qpdf reads — is not, and it is the
  natural first assertion of milestone 9's campaign.
- **No non-Windows producer** (milestone 1), and **the fuzz campaign,
  the fourteenth fingerprint and `docs/STATUS.md`** (milestone 9), all inherited.

