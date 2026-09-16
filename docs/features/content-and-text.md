# Content streams and text extraction

A content stream is postfix — operands, then an operator — and everything a
page shows passes through one interpreter that never knows who is listening.
The listener is a `Device` (ruling 7, [rulings](../rulings.md)): text
extraction and rasterisation are both devices behind the same trait, which is
why `Page::text()` needs no rasteriser, no glyph outlines and no font files —
widths come from the font dictionary, and the text device assembles glyphs
into characters, lines and blocks with a selection quad for every character.

## What it does

**Tokenizing (7.8.2).** `tinker-pdf-content`'s tokenizer splits a stream into
numbers, strings, names, array and dictionary brackets, booleans, `null` and
operators. Literal strings take every escape of 7.3.4.2 (octal above 255 wraps
mod 256, line continuations join), hex strings pad an odd digit count
(7.3.4.3), names decode `#xx` (7.3.5), comments run to end of line (7.2.3).
Numbers parse leniently to their longest sensible prefix — real streams
contain `--5`, `.5.3` and bare `-`, and none of them stops the page.

**Interpretation (8.2, 9.4).** Operands accumulate on a stack until an
operator consumes them; an operator with the wrong operand count is skipped
and the stack cleared, so one malformed instruction cannot desynchronise the
rest. The full graphics state is modelled (8.4.1): `q`/`Q`/`cm` (8.4.4),
`gs` with alphas, `/BM` and `/SMask` through the resource seam (8.4.5,
11.3.5, 11.6.5.1); the complete text state — `Tf` `Tc` `Tw` `Tz` `TL` `Ts`
`Tr` (9.3) — positioning `Td` `TD` `Tm` `T*` (9.4.2) and showing `Tj` `TJ`
`'` `"` (9.4.3), with glyph placement built exactly per 9.4.4. Word spacing
applies to the single-byte code 32 and never to a byte 32 inside a two-byte
CID (9.3.3). Vertical writing displaces the pen by 9.7.4.3's signed `w1` and
places the glyph by minus the position vector `v`, with `TJ` adjustments
running along the writing direction. All eight text render modes of 9.3.6
are carried, including `Tr 3` — invisible text is still text, extracted and
not painted, which is what a scanned page's OCR layer is. Every stroke
parameter reaches the device: `w` `J` `j` `M` `d` (8.4.3), with a dash array
of zeros or negatives meaning a solid line rather than an invisible one.
Paths accumulate through `m` `l` `c` `v` `y` `h` `re` (8.5.2), paint through
`S` `s` `f` `f*` `B` `B*` `b` `b*` `n` (8.5.3), and `W`/`W*` clips take
effect at the painting operator that ends the path (8.5.4). `sh` forwards as
a shading event (8.7.4.2). Colour operators (8.6.8) resolve device spaces
immediately and named spaces through the resource seam, including `scn`'s
trailing pattern name and the components a `PaintType 2` pattern paints with
(8.7.3.2).

**Recursion.** `Do` runs a form XObject with its `/Matrix` composed onto the
CTM and its content clipped to `/BBox` (8.10.2); a transparency group is
offered to the device, and only if accepted are the alphas and blend mode
reset inside it (11.6.6) — declining is the default, because a device with
no buffer must not have `ca 0.5` reset underneath it. A soft-mask group is
run at the `gs` that installs it, at the CTM in force there (11.6.5.2). A
Type 3 glyph is a content stream, not an outline, so it is run the way a
form is, with `/FontMatrix` inside the placing transform and the advance
scaled by it (9.6.5). Recursion of all three kinds stops at a shared depth
cap, and hostile streams meet bounds throughout — operand stack, save
stack, path length — rather than allocation.

**Marked content and optional content.** `BMC`/`BDC`/`EMC` scopes reach the
device with their tag, their own visibility and — when hidden — the layer's
name (14.6.2, 8.11.3.2; ruling 10). An `/OC` entry on a form or image
XObject hides the whole XObject through the same scope mechanism (8.11.4.4).
The two devices deliberately act on different tags: the renderer suppresses
hidden `/OC` layers, and the text device drops `/Artifact` content —
14.8.2.2's running heads and rules are drawn and not read, while an
invisible layer is read and not drawn. A stream that ends with scopes open
has them closed for it, so a form cannot leave its caller inside a layer;
`MP`, `DP`, `BX` and `EX` are recognised no-ops (14.6.1, 7.8.2).

**Inline images (8.9.7).** The span from `BI` to `EI` is scanned at the byte
level — `EI` is accepted only where what follows looks like a content stream
again — and handed to the device as dictionary bytes plus data bytes. The
facade expands Table 93's abbreviations *name by name* (`/F` →
`/Filter`, `/AHx` → `/ASCIIHexDecode`, `/Fl` → `/FlateDecode`, `/CCF` →
`/CCITTFaxDecode`, `/DCT` → `/DCTDecode`, and the rest of Tables 93 and 6),
because a text substitution misses `/F/Fl` written without a space — 7.2.2
makes `/` its own delimiter. The rewritten dictionary then runs the same
COS filter chain every stream uses: predictors, LZW with `/EarlyChange`,
DCT (progressive included), CCITT — one image path, not two.

**The text device.** Glyphs become `TextChar`s with a device-space `Quad`
each (9.4.4), grouped into `TextLine`s by baseline continuation and
`TextBlock`s by vertical proximity. An `ET` is *not* a line break — a
producer that emits one text object per styled span writes one paragraph as
many `BT … ET` pairs on one baseline, so a line closed by `ET` is resumed
when the next glyph continues within half an em of where the pen stopped,
while a real gap on the same baseline (two table cells) stays two lines.
Writing mode and directionality are separate properties: `TextLine::wmode`
is the font's 9.7.4.3 wmode, `TextLine::rtl` is counted from the characters
themselves, because vertical Japanese and right-to-left Arabic are not the
same thing. Warnings are deduplicated — a page whose font is unknown says
so once, not once per glyph — so "this page has no text" and "this page has
text this build could not decode" stay distinguishable (ruling 2).

**The recording device.** `tinker-pdf-content`'s `record.rs` carries a third
`Device`, beside the text device here and the rasterising one in
`tinker-pdf-render`: `RecordingDevice` keeps **every** call, in order, as a
typed `Event` with a copy of the graphics state that call saw. It observes
and decides nothing — no accumulated clip, no bounding boxes, no decoded
image samples, and a `q`/`Q` pair that changed nothing is still two events —
so two consumers wanting different scenes build them from one transcript
rather than having to agree first.

Three decisions are recorded in the module itself, because a later reader
would otherwise have to re-derive them:

- **The list is flat, with explicit begin and end events, rather than a
  tree.** `q`/`Q` and `BMC`/`EMC` nest independently and may cross, so a tree
  would have to name one of them the parent and be wrong in whichever
  direction it chose; and the interpreter's own repairs — a stray `EMC`, a
  scope refused past the depth cap, a stream that ended inside a scope —
  have no parent to be given one. The price is that "what was open here" is
  a prefix walk, paid only by the consumers that ask.
- **Each scope's own visibility is what is stored**, with the enclosing
  answer derived on demand, because the reverse cannot be undone: a nested
  `/OC` naming a layer that is on, inside one that is off, is hidden, and a
  recorder that stored only "hidden here" could never say which scope hid it.
- **The state is copied at the call**, which is what the retained-page row's
  byte-equal replay needs and what a recorder holding one shared handle
  silently loses. `Capture` turns whole categories — and the state copy —
  off for the two consumers that want only glyphs.

It exists as a **prerequisite rather than as a capability**, which is why it
has no [roadmap](../ROADMAP.md) row of its own. Six rows need it and only one
of them says the words: a retained page (whose exit criterion already read "a
recording `Device`"), PDF to SVG, structured text serialisation, table
reconstruction from geometry, inferred reading order for untagged pages, and
the glyph-usage walk that font subsetting on rewrite drives. Promoting it out
of the interpreter's test module deleted five ad-hoc recorders that had grown
there, each keeping what one test needed.

**The first of the six has landed**, and the fit is reported rather than
assumed. `crates/tinker-pdf/src/subset.rs` is one `interpret` into a
`RecordingDevice` and one pass over its events — no second interpreter, no
device of its own. What did *not* fit is `Capture::GLYPHS`, the preset
introduced for that consumer, which turns the bracketing events off:
`Glyph::font_id` is the interned **resource name**, and a resource name is
scope-relative, so without `BeginForm`/`EndForm` there is no way to say which
`/F1` a glyph meant and one font's glyphs would be credited to another — which
drops the glyphs the other font needs. The walk runs under `text` **and**
`structure` instead. The preset is still right for the other glyph-only
consumer named above, inferred reading order, which wants glyphs in document
order and never asks which dictionary a font came from; its name is what is
one preset short, and that is written down in `subset.rs` rather than fixed
by renaming a public constant from a row that is about fonts.

## API

Everything is on the facade (ruling 11): `Page::text()` returns a
`TextPage` of `blocks` → `lines` → `chars`, plus `warnings`. `TextPage`
offers `plain_text()` (one line per line), `lines()` (flattened), and
`search(needle)` — literal, case-insensitive, one `Quad` per match, mapped
back to the glyphs it covers. `Quad` carries four corners and
`bounds()` for the enclosing rectangle.

```rust
let doc = tinker_pdf::Document::open(bytes)?;
let page = &doc.pages()[0];
let text = page.text();
println!("{}", text.plain_text());
for quad in text.search("invoice") {
    let (x0, y0, x1, y1) = quad.bounds(); // PDF user space, y upward
}
for warning in &text.warnings {
    eprintln!("tolerated: {warning:?}"); // TextWarning
}
```

### The structured view (14.7, 14.8)

A tagged document says its own reading order, and that order is often not
the geometric one. `Document::structure()` returns the tree — `None` for
the majority of documents, which carry none, and **nothing is inferred for
them**: a tree guessed from geometry would be this engine's opinion about
reading order presented as the file's own statement of it.

```rust
let Some(tree) = doc.structure() else { return };      // untagged
let page = &doc.pages()[0];
let structured = tree.text_for_page(0, &page.text());  // the SAME TextPage
println!("{}", structured.plain_text());               // in structure order
println!("{} claimed, {} orphaned, {} unmarked",
    structured.matched, structured.orphans, structured.unmarked);
```

Three properties are worth stating because each was a decision:

- **It is a join, never a second extractor.** `text_for_page` takes the
  `TextPage` `page.text()` already produced. Two extractors would be two
  answers about one page, and the first bug would be a caller finding text
  in one that the other does not have.
- **Nodes are runs, not elements.** An element's content and its child
  elements interleave — `/P [ 3 /Span[4] 5 ]` reads 3, then 4, then 5 —
  so one node per element would have to report 3 and 5 together and put 4
  after them. That is a reordering in the middle of a sentence, and it
  reads as a layout opinion rather than as a bug.
- **Orphans are counted, not appended.** A character carrying an `/MCID`
  no element claims is reported as a number, not silently added to the end
  where it would look like reading order.
- **A content item is identified by its stream and its number, never by
  the number alone.** 14.7.4.2 numbers marked-content sequences *within a
  content stream*, so `/MCID 0` in one form XObject and `/MCID 0` in
  another are two sequences; `/MCR /Stm` says which, `/StmOwn` says which
  object owns that stream, and both are read. An `/MCR` naming no `/Stm`
  means the page's own content stream, which is what almost every one of
  them is. Without the pair, two forms on one page with overlapping ids
  give every element both sequences — a page that says everything twice
  while `matched`, `orphans` and `unmarked` all still look right.

Element types are kept **twice** — `raw_type` as the file wrote it and
`standard_type` after `/RoleMap` — because a consumer that wants to know a
paragraph is a paragraph and one that wants the file's own vocabulary are
both real, and keeping one name loses the other. An unmapped custom type
resolves to itself (ruling 2).

On the write side, `PageBuilder::tagged(tag, |page| …)` draws inside a
marked-content sequence and records the element that claims it, so the tree
is correct by construction: there is no way to name a marked-content id
that was never written, and none to write one no element claims.

Coordinates are PDF user space, y upward — the space the page's own boxes
are in; a display transform is the caller's. The `Device` trait, the
interpreter, `TextDevice` and `RecordingDevice` live in `tinker-pdf-content`
and are architecture rather than public API — the facade projects none of
them, and ruling 11 is about what a *document* exposes, not about whether a
crate has an API of its own; see [architecture](../architecture.md).

## Refused by name

| What | Typed name | Why (one line) | See |
| --- | --- | --- | --- |
| A font resource the page names but the build cannot resolve | `TextWarning::UnknownFont { name }` | emitted by `Page::text()`, deduplicated, naming which resource — absence stays distinguishable from emptiness | ruling 2, [rulings](../rulings.md) |
| A code with no `/ToUnicode` entry and no encoding that names it | `TextWarning::UnmappedCode { code }` | the typed vocabulary for a per-code guess; a glyph that decodes to no text at all is dropped from the page rather than invented | 9.10.3 |
| Form XObject / Type 3 / soft-mask nesting past 16 levels | `MAX_FORM_DEPTH` | recursion is refused rather than allowed to overflow the stack | 8.10 |
| More than 4 096 open marked-content scopes | `MAX_MARKED_CONTENT_DEPTH` | scopes past the cap go unreported, and unreported means *visible* — a runaway stream must not hide a page | 14.6.2 |
| Text shaping — Arabic joining, ligature substitution, bidi reordering | — | `TextLine::rtl` reports the dominant direction and reorders nothing; shaping is staged as its own work | [ROADMAP](../ROADMAP.md) |
| Reading order for an **untagged** document | — | `plain_text()` orders lines and blocks geometrically and always has; a structure tree is read when the document carries one, and never invented when it does not | 14.8 |
| An `/MCR` whose `/Stm` does not name a content stream | `StructureWarning::ContentStreamNotAStream { element, stream }` | a stream is always indirect (7.3.8), so the value names nothing that could hold a sequence; read as though `/Stm` were absent rather than keyed on an object with no content, which would make the sequence findable nowhere | 14.7.4.2 |
| An `/MCR` carrying `/StmOwn` without the `/Stm` it qualifies | `StructureWarning::StreamOwnerWithoutStream { element, owner }` | Table 324 permits the owner only beside a stream; an owner alone names the owner of a stream nobody named, so it is dropped | 14.7.4.2 |
| An `/MCR` with no `/Stm` whose `/MCID` is in no page-stream sequence but in exactly one other stream on the page | `StructureWarning::ContentStreamAssumed { page, mcid }` | a producer that tags content inside a form and omits `/Stm` writes something 14.7.4.2 does not define; where one reading exists it is taken and named, and where two streams share the identifier it is refused, because that is the collision `/Stm` exists to resolve | 14.7.4.2 |
| A marked-content sequence in a stream `/StmOwn` says another object owns — an annotation's `/AP` | — | page text extraction runs the page's stream and the forms it invokes, never an annotation's appearance, so such a reference matches nothing here rather than taking whatever else shares its number; extracting appearance-stream text is separate work | 14.7.4.2, 12.5.5 |
| `/ActualText` on a property list carrying no `/MCID` | — | the map is keyed by `(stream, /MCID)`, so a list with no identifier reaches no consumer | 14.9.4 |
| `/Alt`, `/ActualText`, `/E` and `/Lang` on **written** structure elements | — | `PageBuilder::tagged` writes the type and the content, not the 14.9 properties; an empty element is therefore dropped rather than kept, since an empty `Figure` carrying `/Alt` is the case that would want one | 14.9 |

The rendering side of a hidden layer is reported too —
`RenderWarning::HiddenOptionalContent { layer }` names which layer was not
painted — but that row belongs to [rendering](rendering.md).

## Verified

Unit tests live beside the code: `crates/tinker-pdf-content/src/tokenizer.rs`
(every escape form, malformed numbers, arbitrary-byte termination),
`text.rs` (artifact scopes nest, `ET` continuation versus baseline gaps,
search hit geometry, wmode/rtl separation, non-finite glyphs dropped),
`interpret.rs` (group offer/decline, state save discipline), `record.rs` (a
small stream's whole event list asserted in order including the nesting; two
paints with different states asserted separately, which is what catches a
recorder that shares one handle and reports every event with the last state;
`q`/`Q` one for one; a declined form has no end; a glyph-only capture keeps
glyphs and copies no state; the per-event size pinned) and `state.rs` (matrix
convention, render-mode predicates).

Every test in `interpret.rs` drives `RecordingDevice`. It used to carry five
devices of its own — `Recorder`, `GroupEvents`, `Painted`, `Scopes` and
`Images` — and all five are gone with what they asserted unchanged. One
assertion grew rather than moved: the hidden-image test's device used to
suppress a hidden image *itself* while its comment claimed it recorded
"whether it was asked at all", so it proved the weaker of the two. The
interpreter hands a hidden image to the device inside a hidden scope, and the
test now says so.

Facade integration tests: `crates/tinker-pdf/tests/inline_images.rs` (a
predictor-filtered inline image matches the identical XObject pixel for
pixel; compact `/F/Fl` dictionaries; `/EarlyChange` LZW; progressive inline
JPEG; `/Decode` inversion; a zlib bomb stops at the shared ceiling),
`stroke_parameters.rs`, `text_render_modes.rs`, `type3_fonts.rs`,
`vertical_metrics.rs`, `form_xobjects.rs`, `transparency_groups.rs`, and
`optional_content.rs` — which pins the asymmetry both ways: hidden text is
still extracted and still not drawn, and a hidden form's text still
extracts. `tinker_parity.rs` ports Tinker's own `text_and_search.rs`
assertions verbatim (ruling 12).

Two of the 15 determinism render fingerprints exercise this path directly —
`text` and `optional` in `crates/tinker-pdf/tests/determinism.rs` — beside
the 3 document byte-hashes. The `content_tokenizer` and `render_page` fuzz
targets are among the 24 with committed seed corpora; `hostile_input.rs`
sweeps the same shapes on stable. Nothing compares this extractor against
another one: ruling 13 ended that, and the harness that could have driven one
is deleted. Across the corpus, 5 516 of
5 525 files rendered every page with 0 crashes, and `cargo test
--workspace` stands at 4 741 passed / 0 failed / 56 ignored across 215 suites
(Windows x86_64, 14 September 2026).
