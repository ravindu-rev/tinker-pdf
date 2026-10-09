# Opening documents

`Document::open` takes bytes and gives back a document, or a typed reason why
not — and "why not" is deliberately rare. A damaged file opens anyway wherever
anything can be recovered; what the recovery cost is recorded on the document
itself, as a ladder level and a list of typed, object-addressed warnings, so
"it opened" and "it opened cleanly" stay distinguishable facts (ruling 10,
[rulings](../rulings.md)). Below the facade sits the COS layer: one immutable
buffer, a merged cross-reference table in every flavour the spec defines, and
a lazy `Send + Sync` object store over it.

## What it does

**Format routing.** Bytes that begin with a container signature are not read
as a PDF. A ZIP (`PK\x03\x04` at offset zero) is one signature over several
formats, so it is opened once and asked what it is: an XPS package is
recognised by ECMA-388 E.3's three steps, an EPUB by OCF's
`META-INF/container.xml`, and anything else falls through to the comic-archive
path — each synthesised into a real document with a real catalog and page
tree. **A tar, a 7z and a RAR 5 open too** (tier 4), through
`tinker-pdf-archive`; a RAR 4 is refused by its own signature, and a RAR 5
entry this build cannot decompress is a placeholder page rather than a refused
archive. The signatures are tested at a
fixed position only, so a PDF carrying `PK\x03\x04` inside a stream is
unaffected. See [cbz](cbz.md), [xps](xps.md) and [epub](epub.md); a
reflowable book additionally takes `OpenOptions`, because its page count is a
function of the page box the caller passes, not a property of the file.

**One-file documents that are not PDFs** open too (tier 5's formats row),
through `tinker_pdf::standalone`: a **standalone SVG**, a **bare image** and a
**loose XHTML file**. Each had a reader in the tree and was refused as
not-a-PDF because nothing asked. The sniff comes after the containers and
before the PDF parser, and a PDF always wins it — anything with `%PDF-` in its
first 4 096 bytes, which is where the COS parser itself looks for a header
(`tinker_pdf_cos::limits::MAX_HEADER_SCAN`), is a PDF, so a polyglot and a PDF
with junk in front stay PDFs wherever the parser would have opened them as one.
(7.5.2 names no window; Acrobat's implementation note says 1 024 bytes, and a
sniff that used it turned a PDF behind 1 500 bytes of junk that began like a
JPEG or an SVG into a synthesised placeholder.) Past that, an image is told
by its magic at offset zero (JPEG, PNG, TIFF, JPEG 2000, GIF, WebP, AVIF — the
comic path's own classifier, less BMP's two-byte signature) and a markup
document by its root element once the prolog is walked: byte-order mark, white
space, the XML declaration, processing instructions, comments and a doctype
with its internal subset, all inside the first 4 096 bytes (`SNIFF_WINDOW`),
in UTF-8 or in UTF-16 of either byte order, marked or in XML Appendix F's
unmarked shape, which is what `tinker-pdf-xml` decodes. A root named `svg` is an SVG; one named
`html`, `head`, `body`, `p`, `div`, `table` or another of the fifteen WHATWG
MIME Sniffing §7.1 takes to mean HTML, in any case, or a doctype naming
`html`, is an HTML document. **Each is built by the code that builds
the larger document it would be one part of**: an SVG is a book of one
pre-paginated chapter — one page, the size its root states, the caller's
`OpenOptions::page` as the viewport a root with no size fills, and read a
second time, as a book's is, where a bounding-box effect on its text needs the
box its faces give the runs (`a_bounding_box_gradient_on_svg_text_spans_the_text_it_paints`); an XHTML file is
a book of one reflowable chapter at the caller's box with the book's 36-point
margin, its `<title>` the document's `/Title` — read as XML first, so that a
well-formed XHTML file is exactly an EPUB's chapter, and **when it is not XML,
read again by HTML's own parser** (below); a bare image is the comic of
its one picture, paged by the comic path's own body with no archive around it,
one pixel to one point — a GIF and a WebP drawn, a multi-page TIFF one page per
directory. `tests/standalone.rs` holds that as equalities — the same XHTML
bytes render to the same pixels alone and as an EPUB's one chapter, and a bare
PNG, JPEG, TIFF, GIF, WebP and three-page TIFF to the same pages, pixels and
warnings as a one-entry CBZ.
Nothing on these paths refuses a document the sniff recognised: an SVG the
reader will not take and an undecodable picture are each a page saying so in
the report (ruling 2).

**Tag soup is a document.** HTML that is not XML — a `<p>` or `<li>` left
open, an unquoted attribute, a `&nbsp` without its semicolon, formatting
misnested across a block, a cell with no row — is read by
`tinker_pdf_xml::html`, a hand-written WHATWG HTML parser: §13.2.5's
tokenizer, every state, and §13.2.6's tree builder, every insertion mode the
standard still has, the adoption agency, foster parenting, foreign content
and `<template>`, with HTML's 2 231 named character references. It reads
every input to its end, as a browser does, and the tree it builds is the
one the EPUB reader lays out (`epub::xhtml::read_markup_or_html`), with
`ArchiveWarning::Markup(MarkupDefect::NotXml)` in the report. Bytes HTML's
parser decodes are decoded as §13.2.3 says — a byte order mark; then the
prescan of the first kilobyte: UTF-16's `<?x` with no mark, a `<meta charset>`
naming an encoding, and, when the prescan runs out of bytes without one, the
encoding an `<?xml … ?>` at the start declares; then UTF-8 if the bytes are
UTF-8, then windows-1252 — and a `<meta>` the tree builder meets in `<head>`
naming another encoding while that one is tentative changes it, and the bytes
are read again (§13.2.3.4), which is how a `<meta>` past the first kilobyte
is read. The encodings decoded are UTF-8, UTF-16 and the Encoding Standard's
twenty-eight single-byte encodings; a multi-byte one named in a `<meta>` or a
declaration is `MarkupDefect::EncodingNotDecoded`, over the guess. html5lib's
encoding tests, vendored beside its trees, hold it: **82 of 82**. **An XHTML
file whose XML declaration names a single-byte encoding is read in it** — as
XML when it is well-formed, as an FB2 is. **And a file is read in one
encoding whether or not it is well-formed**: one the XML reader decoded — by
its byte order mark, its UTF-16 shape (marked or not), the encoding its
declaration names, or as UTF-8 — and refused for its syntax goes to HTML's
parser as those characters, with `MarkupDefect::Undecodable` for a byte a
single-byte table leaves unmapped; only bytes the XML reader could not decode
at all (an encoding it does not read, malformed bytes, a character XML does
not admit, such as a form feed) are decoded by §13.2.3 as above, whose prescan
reads the same declaration. A well-formed file declaring Shift_JIS is
therefore the guess with `EncodingNotDecoded`, as a `<meta charset=shift_jis>`
is (`a_loose_file_xml_cannot_read_is_read_in_the_encoding_it_names`).
Scripting is disabled, always — nothing
here runs a script — so a `<noscript>`'s content is markup and is drawn. It is
held to html5lib's own suite, vendored: **1 779 of the 1 784
tree-construction tests** that run with scripting disabled build exactly the
suite's tree, the five that do not are named with their reasons, and **every
one of the 7 028 tokenizer runs** a Rust string can express emits exactly the
suite's tokens. An element the adoption agency nests past the XML reader's
depth cap keeps its text in the deepest element the cap allows
(`MarkupDefect::TooDeep`), because every reader after this one was written
against that cap. A file opened from its bytes has nothing
beside it, so a stylesheet, a picture or a face it names by a relative
reference is **missing and named** (`StylesheetUnresolved`, `ImageNotDrawn`,
`SvgImageUnresolved`, `FontFace`, and an SVG `<style>` element's `@import` as
`Svg { warning: ImportUnresolved }`); RFC 2397's `data:` URL carries its own
bytes and resolves. A streamed open of one is whole-file, as a container's is, and
its wider sniff is read only when the first kilobyte holds no PDF header, so
a streamed PDF whose header is in that kilobyte makes the reads it always
made; one with more junk in front costs one read of the 4 096-byte head the
parser searches anyway.

**An FB2 opens like a loose XHTML file** — a root named `FictionBook`,
and the `.fb2.zip` it is usually shipped as, a ZIP of one file whose bytes
sniff as FB2. `tinker_pdf::fb2` translates FictionBook 2.1 into an XHTML
document: sections, titles, epigraphs, poems, cites, subtitles and tables
become `<div>`s and `<p>`s carrying the FB2 name as a class, which
`fb2::STYLESHEET` — a reading system's sheet for a format with no presentation
of its own — sets ahead of the book's own `<stylesheet>`; the inline elements
become HTML's; a note reference is a link that lands on the note's page;
`<book-title>` and the first `<author>` are `/Title` and `/Author`, the cover
is the first page's picture, and the rest of the `<description>` is metadata
and not text. Pictures are `<img src="#id">`, answered from the book's own
`<binary>` elements through the `epub::read::Resources` seam. **An FB2 in an
8-bit encoding is read in it**: a declaration naming one of the WHATWG
Encoding Standard's twenty-eight single-byte encodings — `windows-1251` and
`koi8-r`, which a great many real books are, and their siblings, by any of
the standard's labels — is decoded by that encoding's index, vendored from
`whatwg/encoding` and compiled into `tinker-pdf-xml`
(`Source::with_declared_encoding`, `tinker_pdf_xml::encoding`). A byte the
index leaves unmapped is U+FFFD and counted
(`TranslationDefect::UnmappedByte`). By the standard's own table
`iso-8859-1`, `latin1` and `us-ascii` name windows-1252, which is what such a
book means by its curly quotes.

**Markdown opens by name, not by sniff.** `Document::open_markdown(bytes,
&OpenOptions)` reads the bytes as UTF-8 (each malformed sequence U+FFFD,
counted), translates them with `tinker_pdf::markdown` — a hand-written
CommonMark 0.31.2 reader held to the specification's own 652 examples, 651 of
which it passes exactly — into an XHTML document, and lays that out as a loose
XHTML file is, its first heading as `/Title`. It is a separate entry point
because Markdown has no signature: a sniff that called text Markdown would turn
every file that is not a PDF into a document of its own bytes, where `open`
answers `NotAPdf` and `tinker_parity.rs` holds it to that. Raw HTML is set as
the text it is (`TranslationDefect::RawHtmlAsText`), because a tag that is not
well-formed XML would stop the reader and lose the rest of the document.

**COS parsing.** A hand-written lexer covers the full token grammar of 7.2
and every object form of 7.3: literal strings with all escapes and octal
(7.3.4.2), hex strings (7.3.4.3), names with `#xx` escapes (7.3.5), numbers
with the wild's malformations tolerated and warned (7.3.3 — clamped
overflows, doubled signs, exponent notation), arrays and dictionaries (7.3.6,
7.3.7), streams (7.3.8) and indirect references (7.3.10). The lexer never
fails; every leniency emits a `WarningKind` variant instead of a log line, so
a file that parses with an empty sink is a file that obeyed 7.2 and 7.3.

**Cross-references, every flavour.** Classic tables (7.5.4) with 19- and
21-byte entries resynchronised on the entry grammar; cross-reference streams
(7.5.8) with `/W`, `/Index` defaulting to `[0 /Size]`, and unknown entry
types read as references to null (7.5.8.3); hybrid files via `/XRefStm`
(7.5.8.4); `/Prev` chains with a visited-offset set and a depth cap; and
incremental updates (7.5.6) recorded per revision — each `Revision` carries
the byte range a signature covers and the offset its own `startxref` names
(7.5.5 Table 15). The merged table is built newest-revision-first with
first-writer-wins, dense to a documented cap and spilling to a map above it,
because `/Size` is a claim, not a fact. Objects inside object streams (7.5.7)
are decompressed once and cached; 7.5.7's two rules — generation 0, never
itself a stream — are enforced rather than assumed.

**The leniency ladder.** No offset is trusted: every table entry is validated
against its `N G obj` header when the document opens, eagerly, so the ladder
is decided by the bytes alone rather than by a caller's access order.
Level 1 (`LadderLevel::Trust`) uses the tables as written. Level 2
(`Patch`) repairs a lying entry from the repair scanner — one forward pass
matching `digits ws digits ws obj` at token boundaries, skipping stream
bodies so `N G obj` sequences inside stream data never poison the index —
and keeps the rest of the table. Level 3 (`Rescan`) discards the tables
entirely: no usable `startxref`, an unlocatable `/Root`, or four-plus
validation failures that are also a majority. The scan index becomes truth
and a trailer is synthesised if the file's own is gone. The ladder is
deterministic — same bytes, same path, same warnings ([determinism](determinism.md)).

**Three stream tiers.** `stream_raw_encrypted` is forensic — the exact bytes
the file holds, for signature checking and byte-identical revision copies.
`stream_raw` is extraction — decrypted, undecoded, so a JPEG extracted is the
JPEG embedded. `stream_decoded` is what an interpreter eats, and it is the
only tier with a decode ceiling. `/Length` policy (7.3.8.2): trust the number
when `endstream` sits where it points; otherwise recover from the keyword and
carry both lengths in the warning; with no `endstream` at all, truncate at
the next object header.

**The lazy store.** Objects load on first read and are shared as
`Arc<Object>` — one parse, any number of concurrent readers, no copy on
read. Publication is a compare-and-swap, not a lock around the parse, so
mutually referencing objects loaded from two threads cannot deadlock because
nobody waits. Cycles — a `/Length` pointing into its own stream — read as
null with a warning rather than hanging. The same code runs single-threaded
on `wasm32-unknown-unknown`, which is why the input is bytes rather than a
path: that target has no filesystem ([architecture](../architecture.md)).

**Opening from a source rather than a buffer.** `Document::open_streaming`
takes a `ByteSource` — length plus ranged reads, synchronous, with a typed
miss — and reads what it needs. The engine performs no transport: `SliceSource`
wraps bytes already in hand, and a host that fetches over HTTP range requests
or a memory map implements the trait itself, exactly as it supplies fonts.
Discovery is windowed: a head window for 7.5.2's header scan, the two
`startxref` probes, then each cross-reference section as its own window. The
same engine and the same answers — `streaming_determinism.rs` renders every
fixture from a buffer, from a slice source and from one that answers a single
byte at a time, and compares the pixels and the warnings.

**Annex F, from the head.** A linearized file whose `/L` is the length of the
file opens from its head alone: the first-page cross-reference section is
parsed where it sits, `/O` names page one's page object so the page tree — whose
root a linearized file may leave in the tail, and qpdf's linearizer does — is
not walked, and **the page offset hint table places every page after it**.
Table F.4 item 2 locates a page by accumulating the lengths of the pages before
it; what the tables supply is that byte range and nothing else, because the
objects inside it are read from the file's own `N G obj` headers and each one
still passes `parse_at`'s header check. Hints accelerate, they never decide: a
table that disagrees with the first-page section, or names a range whose
leading object is not a page leaf carrying its own `/MediaBox` and
`/Resources`, or whose runs do not begin one after another, gets
`WarningKind::LinearizedHintsUnusable` or `LinearizedPageHintRejected` and the
page tree walk instead. Page one of the
60-page fixture costs **29,696 bytes of 1,631,075**, page 31 costs **37,888**,
and neither reads the main cross-reference table at `/T`; `main_table_fetched`
and `whole_file_fetched` are the observables that say so. What still costs the
tail is declared: the page *count*, `xref()`, a repair rescan, a save, and a
signature's byte range each fetch and warn first. See
[design/streaming-open.md](../design/streaming-open.md).

**Encryption at open.** An encrypted document opens perfectly well; the
`/Encrypt` scalars are extracted and everything else waits. `readable()` is
what separates "not a PDF" from "wants a password", and
`authenticate` reports which password matched. 7.6.2's exemptions —
cross-reference streams, the metadata stream under `/EncryptMetadata false`,
`/Crypt /Identity`, strings inside object streams — are each honoured. See
[encryption](encryption.md).

## API

The facade entry points, all on `tinker_pdf::Document`: `open(bytes)`,
`open_with(bytes, &OpenOptions)` (the seam where a reflowable book's page box
and a `FontProvider` arrive early enough to decide pagination), `sniff(bytes)`
(a cheap header probe — `%PDF-` in the first 1024 bytes, no parsing),
`readable()`, `authenticate(password)`, `ladder_level()`, `warnings()`, and
`cos()` — the escape hatch to `CosDocument`, whose types (`Object`, `Dict`,
`Name`, `PdfString`, `ObjRef`, `StreamObj`, `XrefTable`, `XrefEntry`,
`Revision`) are re-exported on the facade so the hatch can be named without a
second dependency. On `CosDocument`: `get`, `resolve`, `trailer`,
`revisions`, `xref`, and the tiers `stream_raw_encrypted` / `stream_raw` /
`stream_decoded` (plus `stream_image_input`, decoded up to but not through an
image codec).

`tinker_pdf::standalone` exposes the sniff as its own question —
`sniff(bytes) -> Option<Standalone>` with `Standalone::{Svg, Html,
Image(ImageFormat)}` (`#[non_exhaustive]`, re-exported on the facade) and
`SNIFF_WINDOW` — and the two decoders a loose file's references go through,
`data_url` (RFC 2397) and `base64_decode` (RFC 4648 §4, white space skipped,
padding optional), with `DataUrls`, the `epub::read::Resources` provider that
answers `data:` URLs and hands every other reference to the one behind it.
`Document::open_markdown(bytes, &OpenOptions)` is Markdown's door;
`tinker_pdf::markdown` exposes the reader as `to_html` (CommonMark's HTML, raw
HTML passed through) and `to_xhtml` (the document the page is laid out from,
with what the translation did), and its two caps,
`MAX_MARKDOWN_NESTING` (100 containers) and `MAX_MARKDOWN_REFERENCE_BYTES`
(100 KiB, or the document's length if larger, of what reference links copy
out of their definitions). What a translation had to do is
`ArchiveWarning::Translation { item, defect: TranslationDefect, count }`.
`tinker_pdf::fb2` exposes `to_xhtml` (the translation, without its
pictures), `STYLESHEET` and `FB2_NAMESPACE`; `Standalone::Fb2` is the sniff's
answer.

The streaming seam adds `open_streaming(source)` and
`open_streaming_with(source, &OpenOptions)`, the `ByteSource` trait with
`SliceSource`, `CountingSource` and `ShreddedSource`, the `CHUNK_SIZE` the
chunk cache reads in, and four observables a caller measures a streamed open
by: `is_streamed()`, `first_page_end()` (Annex F's `/E`, `None` unless the
head-only path engaged), `main_table_fetched()` and `whole_file_fetched()`.
`complete_validation()` runs the eager offset probe a streamed open defers and
returns the ladder level a buffered open would have reported.

```rust
let bytes = std::fs::read("report.pdf")?;
let doc = tinker_pdf::Document::open(bytes)?;
doc.readable()?; // Err(PasswordRequired) — opened fine, wants a password
if doc.ladder_level() != tinker_pdf::LadderLevel::Trust {
    for w in doc.warnings() {
        eprintln!("tolerated: {w}");
    }
}
```

## Refused by name

Hard refusals are `OpenError` and `DocumentError`; everything else is a cap
that degrades with a named warning rather than failing the open. The caps are
hardening limits, not conformance limits — Annex C is advisory and real files
exceed it routinely — declared in one place,
[`limits.rs`](../../crates/tinker-pdf-cos/src/limits.rs).

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| Zero bytes | `OpenError::Empty` | Almost always a caller's bug — a path that did not exist — and telling that apart from a bad file matters | `crates/tinker-pdf/src/lib.rs` |
| Nothing PDF-shaped | `OpenError::NotAPdf` | Not one indirect object found, even after a full rescan | `CosDocument::open` → `OpenError::NoObjects` |
| A RAR 4 | `OpenError::UnsupportedArchive(ArchiveRefusal::NotAZip)` | Recognised by its own signature and refused as *that version*; no producer here can write one to hold a decoder to | [cbz](cbz.md), [design/comic-archives.md](../design/comic-archives.md) |
| Customizable `<select>`'s `<selectedcontent>` copy of the selected `<option>` | the `<selectedcontent>` element is built, empty | the copy is a DOM behaviour the parser triggers when an `<option>` is popped, and it needs the option *selectedness* algorithm and the `selected` attribute's dirtiness, which a tree builder does not have. html5lib's `webkit02.dat` #45–#48 are the four tests it fails, by name | `crates/tinker-pdf-xml/tests/html5lib.rs` |
| An HTML tag, attribute or DOCTYPE name past 1 024 bytes, more than 256 attributes on one element (a tag's, or an `<html>` or `<body>` that later tags' attributes are merged into), nesting past 256, more than a million tokens, nodes and copied attributes, more than 1 024 entries in the list of active formatting elements, or more than 64 MiB of attributes copied onto reopened formatting elements | the parse stops there, `MarkupDefect::Truncated`, the tree so far kept | `tinker-pdf-xml`'s four caps, which the HTML parser shares with the XML reader, and two of HTML's own (`MAX_HTML_ACTIVE_FORMATTING`, `MAX_HTML_CLONE_BYTES`). The token cap counts every node the tree builder creates and every attribute a clone copies, because reopening formatting elements makes a few bytes ask for hundreds; the clone cap bounds how long those copies are, since one long attribute value reopened by every paragraph after it asked for 1 700 times the input. html5lib's `tests1.dat` #77, an attribute name of 1 100 characters, is the one suite test a cap stops | `crates/tinker-pdf-xml/src/limits.rs` |
| A `<meta charset>` naming a multi-byte legacy encoding — Shift_JIS, GBK, Big5, EUC-KR — in loose HTML | `MarkupDefect::EncodingNotDecoded`; read as UTF-8 if the bytes are UTF-8 and windows-1252 if not | `tinker_pdf_xml::encoding` decodes the single-byte family and no multi-byte one; what is left of the FB2 row is the same work | [ROADMAP](../ROADMAP.md) |
| Scripts in HTML, and html5lib's `#script-on` tests | never run; `<noscript>` is drawn | there is no script engine, by design; the tree a parser with scripting enabled builds is not this build's | — |
| Raw HTML in Markdown | `ArchiveWarning::Translation { defect: TranslationDefect::RawHtmlAsText, .. }` | set as the text it is: CommonMark passes it through, and a tag that is not well-formed XML would stop the XML reader and lose the rest of the document. Every character still reaches the page | `crates/tinker-pdf/src/markdown.rs` |
| A named character reference outside XHTML 1.0's 253, in Markdown | the reference stays literal | CommonMark resolves HTML's 2 231 names; this repository vendors XHTML 1.0's sets (W3C) and not HTML's list, so `&HilbertSpace;` is text. The one CommonMark example of 652 the reader fails | [THIRDPARTY.md](../../THIRDPARTY.md) |
| Markdown containers past 100 deep, inlines nested past the 202 elements that bounds a document to, references past their copy budget | `TranslationDefect::{NestingTooDeep, ReferenceBudgetSpent}` | read as the text they then are — a too-deep emphasis or link keeps its text and loses its element, so the XML reader's depth cap is never what stops a document and nothing after a deep nest is lost; the two caps are `bounds_ledger.rs` rows. A reference is the one construct whose output is not bounded by its input, so its copies are held to 100 KiB or the document's own length, cmark's rule | `crates/tinker-pdf/src/markdown.rs` |
| An FB2 in a multi-byte legacy encoding — GBK, gb18030, Big5, EUC-JP, ISO-2022-JP, Shift_JIS, EUC-KR | one empty page and `ArchiveWarning::Markup(Truncated)` | `tinker-pdf-xml` decodes UTF-8, UTF-16 and the Encoding Standard's single-byte encodings and refuses the rest by name, and a book set in the wrong letters would be worse than one not set. Each is a state machine over an index of thousands of rows; what is left of the FB2 row | [ROADMAP](../ROADMAP.md) |
| A byte an FB2's declared single-byte encoding leaves unmapped | `ArchiveWarning::Translation { defect: TranslationDefect::UnmappedByte, count }` | U+FFFD, the Encoding Standard's own *replacement* error mode; windows-1253's 0xAA is one | `crates/tinker-pdf/src/fb2.rs` |
| An FB2 element the schema does not define; a `<binary>` that is not base64 | `TranslationDefect::{UnknownElement, BinaryUnreadable}`, and `ImageNotDrawn` for the picture | the unknown element's text is kept and its structure is not; the picture has nothing to draw | `crates/tinker-pdf/src/fb2.rs` |
| A bare BMP | `OpenError::NotAPdf` | `BM` is two bytes, and also how a text file about a car begins; the comic path can afford it because an archive's entries are already pictures, and a sniff over every input cannot | `crates/tinker-pdf/src/standalone.rs` |
| An SVG or HTML whose root element is past byte 4 096 | `OpenError::NotAPdf` | the prolog is walked inside `SNIFF_WINDOW` and not searched past it, because a sniff that scans is one that finds `<svg` inside a PDF's stream | `crates/tinker-pdf/src/standalone.rs` |
| A bare AVIF | `ArchiveWarning::PlaceholderPage { defect: PageDefect::UnsupportedFormat(Avif), .. }` | recognised by magic and not decoded here; one placeholder page naming the format, which is what a one-entry comic archive holding it produces, because a bare picture is paged by the comic path itself | [cbz](cbz.md) |
| What a loose file names beside itself | `StylesheetUnresolved`, `ImageNotDrawn { defect: Unresolved }`, `SvgImageUnresolved`, `FontFace { defect: ResourceMissing }`, `Svg { warning: ImportUnresolved }` | bytes arrive with no directory, so a relative reference has nothing to resolve against; each is named by the warning that already exists for it. RFC 2397's `data:` URL is the exception and resolves | `crates/tinker-pdf/src/standalone.rs` |
| Encrypted, nothing authenticated | `DocumentError::PasswordRequired` | The document opened; reading it is the thing that waits | [encryption](encryption.md) |
| Encryption handler not implemented | `DocumentError::UnsupportedEncryption` | A handler outside R2–R6 cannot be pretended at | [encryption](encryption.md) |
| Decompression bomb | `WarningKind::Filter(Warning::OutputCapHit)` | `stream_decoded` output capped at `MAX_DECODED_STREAM` (128 MiB), so a 1 KB stream cannot buy unbounded memory | `limits.rs` |
| Unknown `/Filter` name | `WarningKind::FilterUnknown` | The chain stops there; bytes decoded so far come back still encoded | [filters](filters.md) |
| Container nesting past 256 | `WarningKind::DepthCapHit` | The container is skipped and reads as null; parser recursion stays bounded | `limits.rs` |
| `/Prev` chains past 64 links | `WarningKind::XrefChainCapHit` | Cycles are caught by the visited set; this bounds the acyclic-but-absurd chain | `limits.rs` |
| `Ref → Ref` chains past 32 hops | `WarningKind::ResolveDepthCapHit` | Legal but never deep; the chain reads as null | `limits.rs` |
| Self-referential loads | `WarningKind::ObjectCycle` | A `/Length` into its own stream reads as null rather than hanging | `crates/tinker-pdf-cos/src/store.rs` |
| Warning floods past 10 000 | `WarningKind::WarningCapReached` | A pathological file cannot flood memory with its own diagnostics; the cap warns exactly once | `limits.rs` |

## Verified

As of 14 September 2026, `cargo test --workspace` runs 4 879 tests (0 failed,
58 ignored, Windows x86_64), and the parts that cover opening are named
([verification](../verification.md)):

- **`crates/tinker-pdf-cos/tests/corrupt.rs`** — the ladder on damage built
  byte by byte: truncated tails, junk before `%PDF-`, lying offsets, dead
  `startxref`, each asserting the exact rung and the exact warning.
- **`crates/tinker-pdf-cos/tests/strict_validator.rs`** — the ladder read
  from the other side. `validate` opens a file with every leniency counted as
  a defect and walks the cross-reference sections out of the bytes, which is
  how two repairs nobody could see were found: an entry whose offset points a
  few bytes *before* its object, and one whose generation the table invented.
  Both open at `Trust` with no warning, because the reader lexes forward for
  the first and normalises the second.
- **`crates/tinker-pdf-cos/tests/document.rs`, `object_grammar.rs`,
  `proptest_lexer.rs`, `proptest_document.rs`** — the grammar, the store and
  property-tested round-trips for string and name escaping.
- **`crates/tinker-pdf/tests/hostile_input.rs`** — real fixtures put through
  seeded deterministic damage on every `cargo test`, asserting only that
  nothing panics and nothing hangs (ruling 1, on stable, so it cannot be
  skipped by accident).
- **`crates/tinker-pdf/tests/tinker_parity.rs`** — compares `OpenError`
  values by ruling 12, which is why the enum stays `Copy + PartialEq + Eq`.
- **Fuzzing** — `cos_document` (the whole file parser, every ladder rung
  reachable from arbitrary bytes, plus a bounded page-tree walk) and
  `cos_object` are two of the 50 fuzz targets, run briefly in CI on every
  commit over committed seed corpora.
- **Corpus** — 5 525 files, 5 516 of them rendered every page, 0 crashes
  (August 2026), in the ratcheted corpus run
  ([verification](../verification.md)); the canonical fixtures in
  `crates/tinker-pdf-cos/tests/document.rs` are mutool-written and must open
  at `Trust` with an empty warning list.
- **`crates/tinker-pdf/tests/streaming_open.rs`** — what a streamed open
  costs, in bytes, against committed budgets: the generic path, page one of a
  linearized file, and page 31 of it. Annex F's hint tables are put to three
  lies made one byte at a time out of a file that was correct before — a table
  disagreeing with the first-page section, a page run whose leading object is
  not a page, a hint stream the section places elsewhere — and each asserts the
  same page comes out, off the page tree, with the leniency named — and a
  fourth, a page length of zero, which is the one hint that could hand back the
  page before it. Over the fetched qpdf corpus: 43 files open from their heads
  and 41 draw the page one the page tree draws (the two exceptions are named,
  and are files the walk cannot answer for at all); 29 of the 43 have a page
  two, **every one of the 29 draws the page the main table draws**, and 20
  reach it without that table.
- **`crates/tinker-pdf/tests/standalone.rs`** — a standalone SVG, a bare image
  and a loose XHTML file through `Document::open`: an SVG's page at the size
  its root states with its pixels and its text, a loose XHTML file **pixel for
  pixel** the one chapter of an EPUB holding the same bytes, a bare PNG pixel
  for pixel the one page of a CBZ, a JPEG, a G4 TIFF, a GIF, two WebPs and a
  three-page TIFF each the same pages, pixels and warnings as a one-entry CBZ,
  the placeholders for an AVIF, an undecodable PNG and a GIF with no image, the `data:` URL resolved and the missing
  references named, tag soup — three files the XML reader stops at in their
  first lines — **pixel for pixel** the XHTML of the tree HTML's parser
  builds (`tag_soup_opens_as_the_tree_html_builds_pixel_for_pixel`), an
  undeclared windows-1252 page read in it, an XHTML file declaring
  windows-1251 read in it whether or not it is well-formed
  (`a_loose_xhtml_file_is_read_in_the_single_byte_encoding_it_declares`), and a
  streamed open the same document. It also holds the defect the row found in
  the streaming sniff: the container window was one `read`, a source that
  answers in pieces gave it one byte of `PK\x03\x04`, and a comic archive
  streamed from one was `NotAPdf`. The sniff's own walk and the two decoders
  are unit tests in `src/standalone.rs`, held to RFC 4648 §10's vectors and
  RFC 2397 §4's examples; `hostile_input.rs` sweeps all seven kinds damaged,
  buffered and streamed; `fuzz/fuzz_targets/standalone.rs` is the deep
  version.
- **`crates/tinker-pdf/tests/fb2.rs`** — an FB2 written from FictionBook
  2.1's schema: its words on its pages and its description off them, its
  title and author as document information, set by the format's sheet (a
  title centred, an epigraph to the right), **pixel for pixel** the XHTML it
  translates to, its cover and picture drawn from its own `<binary>`, a note
  link landing on its note, its own `<stylesheet>` winning over the format's,
  an unknown element and a broken binary named, a Russian book in
  `windows-1251` and in `koi8-r` — its bytes encoded in the test from each
  code chart by hand — the same words, information, warnings and pixels as its
  UTF-8 twin (`an_fb2_in_an_eight_bit_encoding_is_the_book_its_utf8_twin_is`),
  an unmapped byte counted and a Shift_JIS book an empty page that says so, a
  cut book read as far as it goes, and an `.fb2.zip` the
  same book while a ZIP of one picture stays a comic. `hostile_input.rs` sweeps a damaged FB2 and holds its translation to
  being XML; `fuzz/fuzz_targets/fb2.rs` is the deep version.
- **`crates/tinker-pdf-xml/tests/html5lib.rs`** and
  **`crates/tinker-pdf-xml/src/html/suite.rs`** — the HTML parser held to
  html5lib's tree-construction and tokenizer tests, vendored at the last
  commit that holds both (THIRDPARTY.md), compared exactly: **1 779 of 1 784**
  trees, the five that do not pass named as a list rather than counted, and
  **7 028 of 7 028** tokenizer runs (four runs holding a lone surrogate, which
  a Rust string cannot, are not attempted, and nor are `xmlViolation.test`'s
  four, counted by name, which expect a parser coercing its output to an XML
  infoset, §13.2.9), and **82 of 82** encoding tests, through `parse_bytes`'s
  prescan and change of encoding. `tests/html.rs` beside them
  crosses each of the six caps at its shipped value — the token cap by
  reopened formatting elements, fifty kilobytes asking for a million nodes,
  and by the attributes they copy; the attribute cap by merged `<html>` and
  `<body>` tags; the clone cap by one hundred-kilobyte value reopened seven
  hundred times — holds the tree builder's moves linear in a parent's
  children, fostered text included, and Noah's Ark linear in a tag's
  attributes (and `src/html/tree.rs`'s unit tests count the steps of each,
  with a merge into `<html>` or `<body>` costing its own tag's attributes,
  so that a quadratic loop fails rather than slows), and holds the §13.2.3
  decoding order — the XML declaration and
  UTF-16's `<?x` past a prescan with no `<meta>`, a `<meta>` the bytes end
  inside naming nothing, a `<meta>` past the prescan read while parsing;
  `hostile_input.rs`'s
  `mutated_tag_soup_never_panics_the_html_parser` and
  `fuzz/fuzz_targets/html.rs` hold the tree to being a tree.
- **`crates/tinker-pdf/tests/commonmark_spec.rs`** — the Markdown reader held
  to CommonMark 0.31.2's 652 examples, compared exactly, over a `spec.txt`
  fetched at its pinned tag and SHA-256 by `tests/commonmark/fetch-spec.sh`
  (CC-BY-SA 4.0, so never committed); it also asserts it reads the same 652
  the tag's `spec_tests.py --dump-tests` writes as `spec.json`, by a recorded
  fingerprint. **651 pass**, measured 2 October 2026, and every section but
  the entity one is held whole; the `commonmark-spec` CI job greps its `RAN`
  banner. `tests/markdown.rs` runs on every `cargo test`: reader answers worked
  out from the rules, a Markdown document pixel for pixel the XHTML it
  translates to, raw HTML and malformed UTF-8 counted, and both caps fired.
  `hostile_input.rs` sweeps the shapes that make a CommonMark reader
  quadratic at a size where one would hang, and holds the translation's XHTML
  to being XML; `fuzz/fuzz_targets/markdown.rs` is the deep version.
- **`crates/tinker-pdf/tests/streaming_determinism.rs`** — ruling 4 over a byte
  source. Every fixture, linearized ones included, renders identically from a
  buffer, from a slice source and from one answering a byte at a time.
- **Determinism** — the 15 render fingerprints and 3 document byte-hashes all
  pass through `Document::open` first, so a change to opening moves them
  ([determinism](determinism.md)).
