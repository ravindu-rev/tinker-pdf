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
