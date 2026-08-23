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

Coordinates are PDF user space, y upward — the space the page's own boxes
are in; a display transform is the caller's. The `Device` trait, the
interpreter and `TextDevice` live in `tinker-pdf-content` and are
architecture rather than public API; see [architecture](../architecture.md).

## Refused by name

| What | Typed name | Why (one line) | See |
| --- | --- | --- | --- |
| A font resource the page names but the build cannot resolve | `TextWarning::UnknownFont { name }` | emitted by `Page::text()`, deduplicated, naming which resource — absence stays distinguishable from emptiness | ruling 2, [rulings](../rulings.md) |
| A code with no `/ToUnicode` entry and no encoding that names it | `TextWarning::UnmappedCode { code }` | the typed vocabulary for a per-code guess; a glyph that decodes to no text at all is dropped from the page rather than invented | 9.10.3 |
| Form XObject / Type 3 / soft-mask nesting past 16 levels | `MAX_FORM_DEPTH` | recursion is refused rather than allowed to overflow the stack | 8.10 |
| More than 4 096 open marked-content scopes | `MAX_MARKED_CONTENT_DEPTH` | scopes past the cap go unreported, and unreported means *visible* — a runaway stream must not hide a page | 14.6.2 |
| Text shaping — Arabic joining, ligature substitution, bidi reordering | — | `TextLine::rtl` reports the dominant direction and reorders nothing; shaping is staged as its own work | [ROADMAP](../ROADMAP.md) |
| Tagged-PDF structure-based reading order (14.7, 14.8) | — | lines and blocks are ordered geometrically; no structure tree is read anywhere | [ROADMAP](../ROADMAP.md) |

The rendering side of a hidden layer is reported too —
`RenderWarning::HiddenOptionalContent { layer }` names which layer was not
painted — but that row belongs to [rendering](rendering.md).

## Verified

Unit tests live beside the code: `crates/tinker-pdf-content/src/tokenizer.rs`
(every escape form, malformed numbers, arbitrary-byte termination),
`text.rs` (artifact scopes nest, `ET` continuation versus baseline gaps,
search hit geometry, wmode/rtl separation, non-finite glyphs dropped),
`interpret.rs` (group offer/decline, state save discipline) and `state.rs`
(matrix convention, render-mode predicates).

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
sweeps the same shapes on stable. `tools/oracle-diff` can run an external
text extractor for comparison, but it is wired into no test and no CI job,
and ruling 13 retires it. Across the corpus, 4 484 of
4 525 files rendered every page with 0 crashes, and `cargo test
--workspace` stands at 2 924 passed / 0 failed / 8 ignored (Windows x86_64,
as of August 2026).
