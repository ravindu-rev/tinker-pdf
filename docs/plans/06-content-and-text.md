# Phase 06 — Content interpretation and text

When this phase is done, `tinker-pdf-content` can walk any page's content streams through a
complete graphics and text state machine and report everything it sees through `trait Device`,
and a structured text device built on that trait turns glyph events into characters with quads,
lines, blocks, plain text, and search hits — with zero dependency on the rasterizer. The shape
is deliberate: the seam between interpretation and consumption is a trait, not a pixel buffer,
which is what lets Checkpoint A (text/outline/metadata/encryption parity, per
[PLAN.md](../PLAN.md)) ship while `tinker-pdf-raster` is still being built. The same
interpreter, unchanged, later drives the rasterizing device in [08-render](08-rendering-device.md) and
the redaction rewriter in [10-redaction](10-editing.md).

## Scope

- Content stream tokenizer (7.8.2): operands (numbers, strings, names, arrays, dicts,
  booleans, null — direct objects only; content streams cannot contain indirect references)
  and operators, including marked-content `BMC`/`BDC`/`EMC`/`MP`/`DP` (14.6), compatibility
  `BX`/`EX` (unknown operators inside are ignored silently, outside they warn), and Type 3
  `d0`/`d1` as tokens.
- Inline images (8.9.7): `BI` dict with abbreviated keys, `ID`, filter-aware data length
  determination, `EI` recovery scan as fallback. PDF 2.0's explicit `/L` length key honored
  when present and tracked in the 2.0 delta list.
- Graphics state machine: `q`/`Q`/`cm`/`gs` (8.4.4, ExtGState 8.4.5); full text state —
  `Tf`/`Tz`/`Tc`/`Tw`/`TL`/`Ts`/`Tr` (9.3), positioning `Td`/`TD`/`Tm`/`T*` (9.4.2), showing
  `Tj`/`TJ`/`'`/`"` (9.4.3), with text space math per 9.4.4.
- Colour operators `CS`/`cs`/`SC`/`SCN`/`sc`/`scn`/`G`/`g`/`RG`/`rg`/`K`/`k` (8.6.8) recorded
  into the graphics state, values resolved through `tinker-pdf-color`.
- Path construction `m`/`l`/`c`/`v`/`y`/`h`/`re`, painting `S`/`s`/`f`/`F`/`f*`/`B`/`B*`/`b`/
  `b*`/`n`, clipping `W`/`W*` (8.5) — accumulated and streamed to the device as path events,
  not interpreted here. `sh` (8.7.4) forwarded as a shading event.
- Form XObject recursion (`Do`, 8.8/8.10) with resource scoping and a recursion guard.
- `trait Device` — the load-bearing seam between interpretation and consumers.
- Structured text device: glyph events → chars with quads, origin, size, font ref; line
  assembly (baseline clustering, direction vector); block segmentation and reading-order
  heuristics; `wmode` and `rtl` as separate fields; soft-hyphen handling and ligature
  expansion (ToUnicode multi-char, 9.10.3); plain-text assembly.
- Rules honored explicitly: word spacing applies only to single-byte code 32 (9.3.3 — the
  classic CID bug); `Tz`/`Ts`/rise participate in quad geometry; render mode 3 (invisible)
  text is still extracted; modes 4–7 are recorded for [08-render](08-rendering-device.md).
- Search: literal, case-insensitive by simple case fold, mapping matches back to quads;
  multi-line matches produce multiple quads; `Quad::bounds()` (kills the first row of
  Tinker's MuPDF limitation #8 table).
- `ContentFilter` visitor seam: per-operator and per-glyph visit-and-rewrite hooks compiled
  in now, exercised by redaction in [10-redaction](10-editing.md) (kills the
  `pdf_filter_page_contents` gap in the MuPDF limitations report).

## Non-goals

- Rasterization. Path, glyph, image, and shading events are consumed by
  [08-render](08-rendering-device.md); clip accumulation for `W`/`W*` and text render modes 4–7 is
  recorded here, implemented there.
- Type 3 glyph procedure execution (9.6.5). Extraction needs only Type 3 widths and encoding,
  which [05-fonts](05-fonts.md) supplies; running charprocs through this interpreter is
  [08-render](08-rendering-device.md)'s job.
- Pattern and shading paint semantics — recorded and forwarded only.
- Redaction policy. Only the `ContentFilter` mechanics land now; what to drop and how to
  reflow is [10-redaction](10-editing.md).
- Full Unicode bidi (UAX #9) and script shaping. `rtl` is a per-line heuristic flag; chars
  stay in show order. Consumers that need visual reordering do it themselves.
- Tagged-PDF structure-based reading order (14.7/14.8). This phase orders text geometrically;
  structure-tree order is tracked in [PLAN.md](../PLAN.md), not scheduled here.
- Case-sensitive and whole-word search refinement. Tinker filters engine hits today
  (`tinker-core/src/engine/text.rs`) and its tests keep that division; the engine ships the
  case-insensitive default only.
- Deciding *which* streams to run. The interpreter executes any stream handed to it;
  scheduling annotation appearance streams belongs to the phases that assemble pages.

## Design

### Tokenizer

The tokenizer operates on fully decoded bytes — `tinker-pdf-cos` and `tinker-pdf-filters`
have already applied stream filters. A page's `/Contents` array is concatenated into one
buffer with a whitespace byte between parts before lexing. The spec requires producers to
break streams only between tokens (7.8.2); real files violate this, and concatenating first
means a token split across parts still lexes. That is more lenient than the spec and matches
every viewer.

Operands accumulate on a bounded stack (cap 64; overflow discards oldest with a warning —
Annex C's historical limits are far lower, so anything near the cap is garbage). An operator
consumes its operands; an unknown operator outside `BX`/`EX` drops the pending operands and
warns. Numbers lex to `f32` — graphics math throughout the engine is `f32`, matching the
rasterizer and keeping wasm builds lean.

### Inline images

`BI … ID` parses the dict (abbreviated and full key forms both accepted: `/W`, `/H`, `/BPC`,
`/CS`, `/F`, `/DP`, `/D`, `/IM`, `/I`). Data length, in order of preference:

1. PDF 2.0 `/L` — trust it, verify `EI` follows; on mismatch fall through.
2. No filter: exactly `ceil(W × BPC × components / 8) × H` bytes (rows byte-padded).
3. Self-terminating filters: `AHx` ends at `>`, `A85` at `~>`, `RL` at code 128, `DCT` at
   EOI. For `Fl`/`LZW`, the `tinker-pdf-filters` decoders report bytes consumed — every leaf
   decoder exposes consumed-length precisely so this phase can use it. `CCF` with EOFB
   likewise; `CCF` without EOFB is length-ambiguous and falls through.
4. Fallback scan: find whitespace + `EI` + delimiter, validate that decoding the implied data
   succeeds and the bytes after resume as plausible content tokens; prefer the earliest
   candidate that validates. This is the honest messy part, and it is fuzzed hard — including
   fixtures whose image data contains a literal `EI`.

### Graphics and text state

```rust
pub struct GraphicsState {
    pub ctm: Matrix,
    pub fill: ColorValue,
    pub stroke: ColorValue,
    pub line_width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub dash: DashPattern,
    pub alpha_fill: f32,      // /ca
    pub alpha_stroke: f32,    // /CA
    pub blend: BlendMode,
    pub soft_mask: Option<SoftMaskRef>,
    pub text: TextState,
}

pub struct TextState {
    pub font: Option<FontRef>, // resolved via tinker-pdf-font
    pub size: f32,             // Tfs
    pub char_spacing: f32,     // Tc
    pub word_spacing: f32,     // Tw
    pub h_scale: f32,          // Tz / 100
    pub leading: f32,          // TL
    pub rise: f32,             // Ts
    pub mode: TextRenderMode,  // Tr 0..=7
}
```

`q`/`Q` push/pop a `Vec<GraphicsState>`. `gs` merges the named ExtGState (`/Font`, `/LW`,
`/CA`, `/ca`, `/BM`, `/SMask`, `/D`, `/RI`, `/FL`, …). `BT` resets `Tm = Tlm = identity`;
positioning per 9.4.2 (`Td` translates `Tlm` and copies to `Tm`; `TD` also sets `TL`; `T*`
is `0 -TL Td`; `Tm` sets both). `'` is `T*` then `Tj`; `"` sets `Tw`/`Tc` first.

### The glyph loop

For each show operator, the string is decoded by the font (from
[05-fonts](05-fonts.md): code → CID/GID, width `w0` (or `w1` vertical) in glyph-space
thousandths, ToUnicode string, wmode, ascent/descent). Per 9.4.4:

```text
Trm = [ Tfs·Th  0  0 ]
      [ 0     Tfs  0 ] × Tm × CTM
      [ 0      Ts  1 ]

tx = ((w0 − TJadj/1000)·Tfs + Tc + Tw?)·Th        (horizontal)
ty =  (w1 − TJadj/1000)·Tfs + Tc + Tw?            (vertical; no Th)
```

`Tw?` applies only when the code is the *single-byte* value 32 (9.3.3) — never to a byte 32
inside a multi-byte code of a composite font. This is the classic CID bug and gets a
dedicated fixture. The glyph quad is the glyph-space box `[0, descent] × [w0, ascent]`
mapped through `Trm` — four corners, not an axis-aligned rect, so `Tz`, `Ts`, and rotation
fall out of the math instead of being special cases. Mode 3 (invisible) glyphs still reach
the device: OCR text layers are exactly this, and extracting them is the point.

### Form XObjects

`Do` on a `/Form`: save state, `ctm = /Matrix × ctm`, record the `/BBox` clip, recurse into
the form's stream with its own `/Resources`; lookups fall back to the parent's resources
when the form has none or the name is missing (the compatibility behaviour of 8.10.2 —
strictly wrong, universally required). Guard: depth cap 32 plus a visited set of stream
object ids along the current path, so a self-referential form terminates with a warning
rather than a stack overflow.

### `trait Device`

```rust
pub trait Device {
    fn begin_page(&mut self, media_box: Rect, ctm: Matrix) {}
    fn fill_path(&mut self, path: &Path, even_odd: bool, st: &GraphicsState) {}
    fn stroke_path(&mut self, path: &Path, st: &GraphicsState) {}
    fn clip_path(&mut self, path: &Path, even_odd: bool) {}
    fn show_glyph(&mut self, glyph: &GlyphEvent<'_>, st: &GraphicsState) {}
    fn image(&mut self, image: &ImageEvent<'_>, st: &GraphicsState) {}
    fn shading(&mut self, shading: &ShadingRef, st: &GraphicsState) {}
    fn begin_form(&mut self, bbox: Rect, matrix: Matrix) {}
    fn end_form(&mut self) {}
    fn begin_marked_content(&mut self, tag: &Name, props: Option<&Dict>) {}
    fn end_marked_content(&mut self) {}
    fn end_page(&mut self) {}
}

pub struct GlyphEvent<'a> {
    pub code: u32,
    pub gid: u32,
    pub unicode: &'a str,   // may be multi-char (ligature expansion)
    pub origin: Point,      // device space
    pub advance: Vector,
    pub quad: Quad,
    pub font: &'a FontRef,
    pub wmode: WMode,
}
```

Every method has an empty default so a device takes only what it needs. The text device
implements `show_glyph` and the marked-content pair and nothing else — no raster types
appear anywhere in this crate, which is the entire reason Checkpoint A can exist. The trait
is reviewed against [08-render](08-rendering-device.md)'s needs before it freezes; changing it is
cheap now and expensive after integration.

### Structured text device

Pipeline: glyph events → `TextChar` → lines → blocks → page.

```rust
pub struct TextChar {
    pub unicode: SmallStr,  // post-expansion, possibly multi-char
    pub quad: Quad,
    pub origin: Point,
    pub size: f32,
    pub font: FontId,
    pub flags: CharFlags,   // SYNTHETIC_SPACE | LIGATURE | SOFT_HYPHEN | NO_UNICODE
}

pub struct TextLine {
    pub chars: Range<usize>,
    pub dir: Vector,   // baseline unit vector, from Trm
    pub wmode: WMode,  // Horizontal | Vertical — a property of the font
    pub rtl: bool,     // script-direction guess — a property of the text
    pub bbox: Quad,
}
```

`wmode` and `rtl` are separate fields on purpose. Tinker's current `TextLine.rtl` is
computed as `line.wmode() != Horizontal` (`tinker-core/src/engine/text.rs`) — it flags
vertical CJK writing, not right-to-left scripts. The engine fixes the semantics at its
boundary: `wmode` comes from the font, `rtl` from a strong-RTL codepoint majority over the
line (Hebrew/Arabic ranges — a heuristic, stated as one). The integration shim can map
`rtl = wmode != Horizontal` if bug-compatibility is ever needed; the plan is to fix the
consumer instead.

Line assembly: a char joins the open line when its baseline direction matches (`dot(dir) >
0.95`) and its origin's perpendicular offset from the line's baseline is under `0.4 × size`.
A gap along the baseline greater than `0.3 ×` the font's space width (or `0.25 × size` when
the font has none) inserts a `SYNTHETIC_SPACE` char spanning the gap. Blocks group lines
whose inter-baseline distance is under `1.7 ×` line height with horizontal overlap. Reading
order: horizontal text sorts blocks top-to-bottom, then left-to-right within a band;
vertical wmode sorts columns right-to-left. All thresholds live in one `tuning` module and
are calibrated against the corpus gate below — they are heuristics and will never be
perfect; the gate is what keeps them honest.

Soft hyphens (U+00AD) at line end are kept in the structured layer with a flag; plain-text
assembly drops them and joins the split word. Ligatures expand through ToUnicode multi-char
mappings (9.10.3): one glyph, one quad, several chars in `unicode`; the offset map from
plain text back to chars is many-to-one, so a match starting mid-ligature highlights the
whole glyph. Missing ToUnicode falls back through the font's encoding, then AGL glyph
names, then U+FFFD with `NO_UNICODE` set — the fallback chain is [05-fonts](05-fonts.md)'s,
this device just consumes it. Plain text joins lines with `\n` and blocks with `\n`,
matching what `mutool draw -F text` observably does, because that is the comparison target.

### Search

```rust
impl Quad {
    /// Enclosing axis-aligned rectangle. Correct for upright text,
    /// approximate for rotated — the corners remain available.
    pub fn bounds(&self) -> Rect;
}

pub struct SearchHit {
    pub range: Range<usize>, // byte range in plain text
    pub quads: Vec<Quad>,    // one per line the match crosses
}

impl StructuredText {
    pub fn search(&self, needle: &str) -> Vec<SearchHit>;
}
```

Literal matching over the plain text, case-insensitive via Unicode *simple* case fold (1:1,
no full fold — `Straße` does not match `STRASSE`; accepted, and close to MuPDF's own
behaviour). An empty needle returns no hits. The offset map turns a match range into per-line
char runs: one quad per line, so a match across a line break produces multiple quads instead
of one nonsense rectangle. Search runs over the dehyphenated text, so a word split by a soft
hyphen is findable and its hit carries both fragments' quads. Because hits carry text
ranges, Tinker's context extraction becomes a substring instead of today's quad-overlap
heuristic, and its case-sensitive/whole-word refinement keeps working unchanged.

### `ContentFilter`

```rust
pub enum OpAction    { Keep, Drop, Replace(Vec<Op>) }
pub enum GlyphAction { Keep, Drop }

pub trait ContentFilter {
    fn op(&mut self, op: &Op, st: &GraphicsState) -> OpAction { OpAction::Keep }
    fn glyph(&mut self, g: &GlyphEvent<'_>, st: &GraphicsState) -> GlyphAction {
        GlyphAction::Keep
    }
}
```

In filter mode the interpreter routes every operator through the filter before emission and
re-serializes the survivors through `tinker-pdf-cos`. Dropping a glyph inside `Tj`/`TJ`
splits the show string and synthesizes a `TJ` adjustment (or `Td`) equal to the dropped
advance, so surviving glyphs do not move. This is the seam MuPDF never exposed
(`pdf_filter_page_contents`, "Reported" table of Tinker's `docs/mupdf-limitations.md`); the
mechanics land now so [10-redaction](10-editing.md) is policy, not plumbing.

### Leniency and failure policy

The interpreter never errors on content; it warns and continues, because a page that half
renders beats no page. Specifically: `Q` on an empty stack is a no-op warning; unbalanced
`q` at stream end pops implicitly; a missing `ET` closes at stream end; `BT` inside `BT`
implies `ET`; wrong operand types coerce where safe (int↔real) and skip the operator
otherwise; `Tf` naming a missing font substitutes fallback metrics from
[05-fonts](05-fonts.md) so positions keep advancing. `InterpretOptions` carries the form
depth cap and an optional op-count fuel for untrusted input. Diagnostics go to a capped
sink. The crate is pure computation over slices — no I/O, clocks, or threads — so
`wasm32-unknown-unknown` (first-class from day one) needs nothing special, and CI builds it
for that target from the first milestone. Tokenizer and interpreter (against a null device)
are cargo-fuzz targets; panics are bugs, full stop.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Tokenizer + inline images | Fuzz targets in CI, zero crashes; every content stream in the pdf.js corpus tokenizes (warnings allowed, errors not); inline-image fixtures — uncompressed, AHx, A85, DCT, CCITT-without-EOFB, data containing literal `EI` — all recover exact data length; crate builds for wasm32-unknown-unknown | S |
| 2 | State machine, `trait Device`, form recursion | Conformance suite of hand-written streams with expected state snapshots per operator; Trm and displacement math match hand-computed values including `Tz`/`Ts`/`TL` cases; unbalanced `q`/`Q` fixtures warn, never fail; self-referential and 33-deep forms terminate; resource-shadowing fixture resolves form-first with page fallback | M |
| 3 | Structured text device: chars, lines, blocks, plain text | Per-glyph quads match hand-computed boxes on rotated/scaled/risen fixtures; CID fixture with byte 32 inside two-byte codes shows no spurious word spacing (9.3.3); `Tr 3` text extracted; vertical-CJK fixture reports `wmode=Vertical`, `rtl=false`; ≥98% whitespace-normalized similarity vs `mutool draw -F text` over the pdf.js corpus via oracle-diff (soft hyphens normalized on both sides) | M |
| 4 | Search + `Quad::bounds` | Engine-level tests replicate `tinker-core/tests/text_and_search.rs` semantics: `TINKER` found case-insensitively on a page saying `Tinker`, empty needle finds nothing, fragment matches loosely; a match spanning a line break yields one quad per line; hit ranges reproduce Tinker's context lines by substring | S |
| 5 | `ContentFilter` seam | Keep-everything filter re-serializes corpus pages to output that renders identically under the mutool oracle (render-diff via oracle-diff; our own renderer does not exist yet); a drop-one-string's-glyphs filter leaves every surviving glyph at its original position under the same oracle | S |

## Dependencies

Requires: [01-cos](01-cos-and-object-model.md) for objects, streams, the page tree, and resource dictionary
access; [02-filters](02-filters.md) for stream decoding and the consumed-length reporting the
inline-image scan leans on; [05-fonts](05-fonts.md) for string decoding, widths, ToUnicode,
wmode, and vertical metrics; [08-rendering-device](08-rendering-device.md) for resolving colour operators into
recorded values. Crypto is only indirect, via cos.

Unblocks: Checkpoint A — this phase is its text half; [08-render](08-rendering-device.md), which
implements the same `Device` trait against the rasterizer; [10-redaction](10-editing.md),
which is a `ContentFilter` plus policy; and the `tinker-pdf` facade's text and search API.

## Risks

| Risk | Mitigation |
| --- | --- |
| Inline-image `EI` recovery guesses wrong on hostile or broken data | Decoder-consumed-length is primary and exact; the scan is a validated fallback; differential test against mutool's recovery over the corpus; fuzz fixtures with `EI` embedded in image data |
| Line/block heuristics diverge from mutool enough to miss the 98% gate | The gate compares normalized plain text, not layout; all thresholds in one `tuning` module, calibrated against corpus failures; per-file similarity report from oracle-diff to find the worst offenders first |
| Word-spacing and vertical-writing subtleties (9.3.3, 9.4.4) ship wrong | Dedicated fixtures per rule, written before the code; CJK subset of the corpus in the differential gate |
| ToUnicode absence or lies degrades extraction silently | Fallback chain ends in flagged U+FFFD, never dropped text; `NO_UNICODE` counts surface in tpdf so degradation is measurable, not invisible |
| `trait Device` shape proves wrong for the rasterizer | Trait reviewed against [08-render](08-rendering-device.md)'s design before freezing; it stays minimal (events, not policy); pre-integration changes are cheap and treated as such |
| `ContentFilter` string-splitting perturbs positioning | Identity-filter round trip must render identically under the oracle before any dropping filter is trusted; positioning synthesized from the exact dropped advances, not re-measured text |
| Per-glyph dyn dispatch too slow on text-heavy pages | Measure with criterion before optimizing; extraction budgets are modest, and the glyph loop is allocation-free by construction — `GlyphEvent` borrows |
| Recursion and resource edge cases in the wild (missing `/Resources`, cycles) | Compatibility lookup order implemented from the start; depth cap + cycle set; corpus files that exercise these become fixtures the day they are found |
