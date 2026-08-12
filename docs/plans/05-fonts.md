# Phase 05 — Fonts

When this phase is done, `tinker-pdf-font` parses every font program format PDF 1.7 can embed
(TrueType/sfnt, CFF, bare Type 1, plus the AFM and CMap text formats) as pure
bytes-in/values-out code with zero PDF types, and a binder layer in `tinker-pdf-content` turns
any PDF font dictionary — Type1, TrueType, Type0, Type3, MMType1 — into the one mapping the
rest of the engine needs: string bytes → codes → (glyph, advance, Unicode). The work is split
into two waves because the consumers arrive months apart: wave 1 (metrics + Unicode, L) is a
hard prerequisite for Checkpoint A, which extracts text with no rasterizer in the build; wave 2
(outlines + glyph rasterization, XL) feeds Checkpoint B and is, stated plainly, the hardest
visual-parity item in the whole engine — glyph rendering is where a from-scratch renderer is
most visibly worse than a FreeType-backed one, and the milestones below budget for that.

All hand-rolled per the locked project decisions: no `ttf-parser`, no FreeType, no HarfBuzz
equivalent, ever. Dev tooling exemptions (`cargo-fuzz`, `criterion`, `proptest`) apply.

## Scope

### Wave 1 — metrics and Unicode (L)

- Font dictionary model for the five /Subtype values: Type1 (9.6.2, MMType1 folded in),
  TrueType (9.6.3), Type3 (9.6.5), Type0 with a single descendant CIDFontType0/2 (9.7.6).
- Simple-font widths: /Widths + /FirstChar + /LastChar, /MissingWidth fallback from the
  FontDescriptor (9.8, Table 122).
- CID widths: /W and /DW (9.7.4.3), both list forms (`c [w…]` and `c_lo c_hi w`); vertical
  metrics /W2 and /DW2 stored now, consumed by the interpreter in [06-content](06-content-and-text.md).
- /Encoding resolution (9.6.6): builtin encoding from the font program, base-encoding name or
  /BaseEncoding, /Differences overlay, and the symbolic-TrueType glyph lookup order of 9.6.6.4.
- Annex D encoding tables as build-time data: Standard, WinAnsi, MacRoman, MacExpert, PDFDoc,
  plus the Symbol and ZapfDingbats builtin encodings.
- ToUnicode CMap parsing (9.10.3): codespace ranges, bfchar, bfrange including destination
  arrays and UTF-16BE surrogate pairs.
- Predefined CMaps (9.7.5.2): Identity-H/V compiled in from day one; the full Adobe CMap set
  parsed at build time from Adobe's `cmap-resources` data (BSD-3-Clause — license and size
  handling in Design).
- Base-14 AFM metrics as build-time data; AGL/AGLFN glyph-name → Unicode tables.
- TrueType parsing: table directory, head/hhea/hmtx/maxp, cmap formats 0/4/6/12, post 2.0/3.0.
- CFF parsing: INDEXes, Top/Private DICTs, charset, encoding, and charstring *execution for
  advance widths only* (the width prefix on the first stem/move operator).
- Type 1 parsing: eexec decryption, charstring decryption (lenIV), /CharStrings, hsbw/sbw.
- FontDescriptor fallbacks when widths are missing or garbage; broken-embedded-font repair
  leniency: wrong /Length1/2/3 (9.9, locate eexec ourselves), truncated sfnt tables, CFF
  trailing garbage.
- The single-byte code-32 word-spacing rule (9.3.3) is *noted* here — the font layer exposes
  per-code byte length so [06-content](06-content-and-text.md) can implement it. Not implemented here.

### Wave 2 — outlines and rasterization (XL)

- glyf/loca: simple glyphs (quadratic on/off points, implied on-curve midpoints) and composite
  glyphs (offset args, scale and 2×2 transforms, point matching, USE_MY_METRICS, depth cap).
- CFF/Type 2 charstring interpreter: local/global subrs with bias, hintmask/cntrmask operand
  counting (including the implicit vstem before the first hintmask), seac via the 4-argument
  endchar form, flex/flex1/hflex/hflex1.
- Type 1 charstring interpreter: seac, flex and hint replacement via othersubrs 0–3,
  callothersubr/pop/div machinery.
- Type3 glyphs: /CharProcs delegated to the content interpreter with /FontMatrix, d0/d1,
  /Resources scoping.
- CIDFontType0: FDSelect formats 0 and 3, per-FD Private DICTs and local subrs; CIDFontType2:
  /CIDToGIDMap name or stream. *Amended August 2026:* the first belongs to the leaf crate and
  the second does not. `/CIDToGIDMap` is an entry in a PDF font dictionary, so by the Design
  section's own rule — and by ruling 8 — it is read by the binder and the leaf crate is handed
  the glyph index that falls out. See [gap 02](gaps/02-cid-to-gid.md).
- Glyph → path → raster through [07-raster](07-rasterizer.md), with a per-(face, glyph, size)
  raster cache; hints parsed for stem darkening only — no hinting engine.
- Base-14 substitute faces (Liberation), the Symbol/ZapfDingbats gap, and `trait FontProvider`
  for host-supplied system fonts.

## Non-goals

- **Hinting.** No TrueType bytecode VM, no Type 1 hint-substitution rendering — permanently.
  Stems are parsed and used for darkening only. The consequence is owned in Risks.
- **Shaping.** PDF content streams position pre-shaped glyphs; GSUB/GPOS, ligature formation,
  and bidi are the producing application's job and will never be ours.
- **Word-spacing semantics and vertical layout math.** Tw's code-32 rule and the application
  of /W2//DW2 position vectors to the text matrix live in [06-content](06-content-and-text.md); this
  phase only stores the metrics and exposes code byte lengths.
- **Font subsetting and embedding on write.** Lives with the writer phases — see
  [PLAN.md](../PLAN.md) for the phase index.
- **MM Type1 interpolation.** Without embedded instance data the spec says treat it as the
  base font; we snapshot it as ordinary Type 1 and emit a warning. No axis math.
- **Color and bitmap glyph formats** (COLR/CBDT/sbix, bitmap-only sfnt): degrade to substitute
  or notdef. PDF 2.0 deltas are tracked in the separate 2.0 delta document per
  [PLAN.md](../PLAN.md).
- **OS font discovery.** No fontconfig/DirectWrite/CoreText in the engine. `FontProvider`
  pushes enumeration to hosts; Tinker's own provider is written in the integration phase.

## Design

### Where the code lives

`tinker-pdf-font` stays a leaf: it parses sfnt, CFF, Type 1, AFM, and CMap *byte streams* and
knows nothing about PDF objects, which keeps every parser independently fuzzable. CMaps
qualify because a CMap is PostScript-ish text, not a COS object — `tinker-pdf-cos` +
`tinker-pdf-filters` produce decoded bytes, the leaf parses them. Everything that needs a PDF
dictionary — /Widths, /Encoding merging, /W, ToUnicode wiring, FontFile extraction, repair —
is the *binder*, `tinker_pdf_content::font::LoadedFont`. This split also dissolves the Type3
cycle: Type3 glyph procs need the content interpreter, and the code that needs it already
lives in `tinker-pdf-content`. Phase 05 owns the binder code even though it sits in 06's crate.

### Leaf parsers

All parsers borrow the input; nothing is copied at parse time except tables that must be
re-indexed (CFF charsets into a GID→SID map, loca into offsets).

```rust
// tinker-pdf-font — representative surface, not exhaustive
pub struct TrueTypeFace<'a> { /* table directory over borrowed bytes */ }

impl<'a> TrueTypeFace<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, FaceError>;
    pub fn units_per_em(&self) -> u16;
    pub fn advance(&self, gid: GlyphId) -> Option<u16>;          // hmtx + hhea rules
    pub fn gid_for_unicode(&self, c: char) -> Option<GlyphId>;   // cmap (3,1)/(0,x): 4, 12, 6, 0
    pub fn gid_for_symbol(&self, code: u32) -> Option<GlyphId>;  // cmap (3,0), code and 0xF000|code
    pub fn gid_for_mac(&self, code: u8) -> Option<GlyphId>;      // cmap (1,0)
    pub fn glyph_name(&self, gid: GlyphId) -> Option<&str>;      // post 2.0; 3.0 → None
}
```

`CffFont` and `Type1Font` have the same shape plus name-keyed lookup (`gid_for_name` via
charset/`/CharStrings`). Wave 1 executes charstrings only far enough to read the width: for
Type 2 that means detecting the optional width operand before the first
hstem/vstem/cntrmask/hintmask/moveto/endchar against nominalWidthX/defaultWidthX; for Type 1
it means hsbw/sbw. This is deliberately a separate, tiny evaluator — the full interpreter in
wave 2 replaces it, and the width-only path stays as the fuzz-cheap fallback.

`Cmap` (the CMap-syntax parser) produces codespace ranges plus sorted single and range
mappings. Lookup walks the codespace to determine code length (1–4 bytes) — this is what makes
multi-byte string splitting correct and what 06 needs for the Tw rule — then binary-searches
the mappings. ToUnicode reuses the same parser with bf* destinations decoded as UTF-16BE,
surrogate pairs included.

### The binder

```rust
// tinker-pdf-content::font
pub struct Code { pub value: u32, pub byte_len: u8 }

pub struct LoadedFont { /* subtype, program, widths, encoding, tounicode, … */ }

impl LoadedFont {
    pub fn load(dict: &CosDict, res: &Resources, warn: &WarnSink) -> LoadedFont; // never fails
    pub fn codes<'a>(&'a self, s: &'a [u8]) -> impl Iterator<Item = Code> + 'a;
    pub fn advance(&self, code: Code) -> f32;          // text-space units (/1000 or /W)
    pub fn to_unicode(&self, code: Code) -> Option<CompactStr>;
    pub fn glyph(&self, code: Code) -> GlyphRef;       // wave 2
}
```

`load` never fails: every failure mode degrades (embedded program broken → metrics-only →
substitute face; no widths → FontDescriptor /MissingWidth → AFM/substitute metrics) and
reports through the warning channel established in the COS phase. A font dictionary that is
itself missing entries still produces a `LoadedFont` that answers every query.

**Encoding resolution (simple fonts, 9.6.6):** start from the font program's builtin encoding;
replace with the named base encoding or /BaseEncoding if present (default Standard for
nonsymbolic fonts with no builtin); overlay /Differences last. Glyph lookup for TrueType
follows 9.6.6.4: symbolic → (3,0) cmap with `code` then `0xF000|code`, then (1,0);
nonsymbolic → name → Unicode via AGL → (3,1) cmap, falling back to (1,0) via MacRoman.
Name-keyed formats (Type 1, CFF) look up by glyph name directly.

**Unicode resolution order (extraction):** ToUnicode CMap → glyph name through AGL including
`uniXXXX`/`uXXXXXX` forms → the encoding table's Unicode column → the code itself as a last
resort, flagged low-confidence in the text device's output so Tinker's search can rank it.

**Widths:** simple fonts store a dense `[f32; 256]` resolved once at load. CID fonts store /W
as sorted, non-overlapping ranges with binary search, /DW as the default; /W2//DW2 the same
shape for vertical. Exit tolerance is 0.1 pt against the oracle at rendered size — tight
enough to catch nominalWidthX/defaultWidthX confusion, which is the classic CFF width bug.

### Build-time data

A build script (`tinker-pdf-font/build.rs`) compiles vendored data into static tables; raw
sources never ship in the crate binary.

| Data | Source license | Handling |
| --- | --- | --- |
| Annex D encodings, Symbol, ZapfDingbats | spec text | typed tables, trivial size |
| AGL + AGLFN | BSD-3-Clause (adobe-type-tools) | perfect-hash name→char table |
| Base-14 AFMs | Adobe's redistribution notice | advances + FontBBox only |
| Predefined CMaps | BSD-3-Clause (cmap-resources) | delta-encoded ranges, deflated with our own filter code |
| Liberation faces (wave 2) | OFL-1.1 | deflated TTFs, 12 faces |

Licenses ride along in `THIRDPARTY.md`; OFL and BSD data assets do not affect the
MIT OR Apache-2.0 code license, and CI license checks are taught the distinction. Size is a
real cost against the wasm budget: the CJK CMap set is ~10 MB of source text, expected to
compile+deflate to low single-digit MB, and Liberation is ~2 MB deflated — both numbers are
estimates until measured at M4/M10, and the measured figures get recorded in this file.
Feature gates: `cmap-predefined` (default on; Identity-H/V are always compiled in regardless)
and `bundled-fonts` (default on; wasm hosts that supply a `FontProvider` can drop both).

### Wave 2 — outline extraction

One sink trait, three producers (glyf, Type 2, Type 1), so the rasterizer and any future
consumer see identical geometry regardless of source format:

```rust
pub trait OutlineSink {
    fn move_to(&mut self, p: Point);
    fn line_to(&mut self, p: Point);
    fn quad_to(&mut self, c: Point, p: Point);
    fn curve_to(&mut self, c1: Point, c2: Point, p: Point);
    fn close(&mut self);
}

pub struct GlyphMetrics { pub advance: f32, pub stems: StemSet }

impl<'a> TrueTypeFace<'a> {
    pub fn outline(&self, gid: GlyphId, sink: &mut impl OutlineSink)
        -> Result<GlyphMetrics, GlyphError>;
}
```

Interpreter notes, because these are where every implementation gets bitten: Type 2 hintmask
operand counting must account for an implicit vstem when stack args precede the first
hintmask; subr indices are biased by count thresholds (107/1131/32768); seac arrives as
4-argument endchar and recurses through the *standard encoding*, not the font's. Type 1 flex
and hint replacement go through othersubrs 0–3 with the callothersubr/pop dance; division
results are real numbers on an integer-looking stack. Composite glyf point-matching aligns a
component by making two point indices coincide — rare, but real fonts use it. All recursion
(subrs, seac, composites, Type3 procs) shares one depth budget with a hard cap; exceeding it
yields notdef, not a stack overflow. Interpreters run against per-glyph raster diffs from
`oracle-diff` (FreeType via the oracle subprocesses), which is the only realistic way to find
the long tail of charstring edge cases before users do.

Type3 glyphs re-enter the content interpreter with the char proc stream, /FontMatrix composed
into the CTM, and d0/d1 supplying the advance. That machinery belongs to
[06-content](06-content-and-text.md); this phase contributes the dispatch and the tests.

### Rasterization and the cache

```rust
pub struct GlyphKey {
    face: FaceId,     // interned
    gid: GlyphId,
    ppem_q: u16,      // 26.6-quantized pixels-per-em
    x_phase: u8,      // 4 subpixel buckets in x, none in y
    flags: RenderFlags,
}
```

Upright, axis-aligned text — the overwhelmingly common case — renders glyph outlines at the
quantized ppem through the AA filler in [07-raster](07-rasterizer.md) into 8-bit coverage masks,
cached in a byte-budget sharded LRU (default 8 MiB native, 2 MiB wasm, host-configurable).
Rotated, skewed, or stroked text bypasses the cache and fills the transformed path directly;
caching those is a size/complexity trade that loses.

**Hinting stance, stated honestly:** the oracle renderers hint (or autohint) at small sizes;
we do not, and unhinted small-size text is THE visual risk of this phase against MuPDF
goldens. Mitigation, not cure: stems collected during charstring execution drive
ppem-dependent stem darkening (embolden by a fraction of a pixel below ~20 ppem; TrueType has
no declared stems, so it gets a heuristic constant), gamma-aware coverage mapping in 07, and
per-dpi perceptual thresholds in `pdfcmp` — looser at 72 dpi, tight at 300 dpi where hinting
stops mattering. The 72 dpi gate is a human legibility review once, then pixel-locked against
our own output so regressions are caught without pretending we match FreeType pixel-for-pixel.

### Substitutes and FontProvider

Non-embedded base-14 (and non-embedded anything, after `FontProvider` declines) resolves to
the Liberation family — OFL-1.1, metric-compatible with the Helvetica/Times/Courier cores, 12
faces bundled deflated. Symbol and ZapfDingbats have no bundled metric-compatible face: that
is an **open licensing item** (candidates exist but none currently clears MIT/Apache-adjacent
redistribution comfort); until resolved, their codes remap through the Annex D tables to
Unicode and render from a bundled face where coverage exists, notdef where it does not.
Corpus telemetry at M10 measures how often this path is actually hit.

```rust
pub trait FontProvider: Send + Sync {
    fn find(&self, req: &FaceRequest<'_>) -> Option<FaceData>; // owned or mmap'd bytes
}

pub struct FaceRequest<'a> {
    pub post_script_name: &'a str,
    pub flags: FontFlags,                       // serif / fixed-pitch / symbolic / italic
    pub weight: u16,                            // StemV-derived when absent
    pub cid: Option<(&'a str, &'a str)>,        // Registry, Ordering
}
```

Lookup order: embedded program → `FontProvider` → bundled substitutes → notdef. The engine
itself never touches the OS.

### Error and leniency policy

Face-level parse failure is an error the binder catches and degrades; glyph-level failure is
never an error to the caller — it is notdef plus a warning. Repair specifics: /Length1/2/3 are
treated as hints, with the eexec boundary and the binary/hex sections located by scanning;
sfnt table offsets/lengths are validated against the file and individually dropped (a bad
`post` costs glyph names, not the font); charstring runaways hit instruction and depth budgets.
Fuzzers (`cargo-fuzz` targets `sfnt`, `cff`, `type1`, `cmap` in the leaf crate) assert
no-panic/no-OOM/no-hang only — leniency means malformed input yielding *something* is correct
behavior, so differential width/raster checks against oracles, not the fuzzer, catch silent
wrongness.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | sfnt front end in `tinker-pdf-font`: table directory, head/hhea/hmtx/maxp, cmap 0/4/6/12, post 2.0/3.0 | Fixture faces report units-per-em, advances, and gid lookups matching a FreeType dump exactly; `sfnt` fuzz target clean over a seeded corpus | S |
| 2 | CFF and Type 1 metrics: INDEX/DICT/charset/encoding, width-only charstring evaluation; eexec + charstring decryption, /CharStrings | Every glyph advance in fixture faces matches FreeType within 1/1000 em; `cff` and `type1` fuzz targets clean | M |
| 3 | Binder `LoadedFont`: all five subtypes, /Widths + CID /W //DW //W2 //DW2, encoding resolution incl. 9.6.6.4, Annex D + AGL + base-14 AFM build-time data, FontDescriptor fallbacks | Text-space advances within 0.1 pt of the mutool oracle across the width corpus; per-table encoding unit tests; `LoadedFont::load` total (never fails) under fuzzed dicts | M |
| 4 | CMap machinery: syntax parser, ToUnicode incl. surrogate pairs, Identity-H/V, build-time compiled predefined CMaps behind `cmap-predefined` | CJK fixtures map code→CID→GID identically to oracle text dumps; wasm build with the feature off shrinks by a measured, recorded delta | M |
| 5 | Repair leniency + wave-1 gate: /Length1 lies, truncated tables, metrics-only degradation, warning channel; full corpus run wired into CI | **W1 exit:** every corpus glyph maps to (advance, Unicode) without panic; widths within 0.1 pt of oracle; Checkpoint A's font prerequisite green | S |
| 6 | glyf/loca outlines: simple + composite (transforms, point matching, USE_MY_METRICS), shared `OutlineSink` | Per-glyph raster diff vs FreeType via `oracle-diff` under threshold on all TrueType fixtures; composite depth cap tested; fuzzer extended to outlines | M |
| 7 | Type 2 interpreter: subr bias, hintmask counting, flex ops, endchar-seac, stem collection; CIDFontType0 FDSelect/FDArray; CIDFontType2 CIDToGIDMap | Per-glyph diff gate on CFF/OTF fixtures including a CJK CID face; width-only and full-interpreter advances agree; `cff` fuzzer clean with outline execution | M |
| 8 | Type 1 interpreter: flex/seac/othersubrs, hint replacement; Type3 delegation with /FontMatrix, d0/d1, resource scoping | Type 1 per-glyph diff gate; Type3 fixtures render through the 06 interpreter; shared recursion budget tested at the cap | M |
| 9 | Glyph raster pipeline: outline → 07 fill, `GlyphKey` LRU with byte budget, rotated/skewed bypass, stem darkening | 300 dpi text pages within `pdfcmp` budget vs the MuPDF oracle; >90 % cache hit rate on the text corpus; cache growth bounded under adversarial input | M |
| 10 | Substitutes + provider: Liberation bundling behind `bundled-fonts`, Symbol/ZapfDingbats Unicode-remap fallback, `FontProvider` + lookup order | Non-embedded corpus renders via substitutes without panic; wasm size delta measured and recorded here; round-trip test with a host-supplied face; Symbol-path hit rate measured | S |
| 11 | Quality + wave-2 gate: 72 dpi legibility pass, per-dpi perceptual thresholds, long fuzz soak | **W2 exit:** fixtures legible at 72 dpi (human-reviewed once, then pixel-locked); 500-page text corpus within perceptual budget vs MuPDF oracle; 24 h fuzz soak clean on `cff`/`sfnt`/`type1` | M |

Wave 1 is M1–M5 (L band); wave 2 is M6–M11 (XL band). The XL band's upper half is deliberate
slack for M7, M9, and M11 — charstring edge cases and small-size quality tuning are open-ended
in a way table parsing is not, and at least one of the three will run long.

## Dependencies

- **Needs first:** the COS phase (font dictionaries, FontFile stream access with decryption
  and decoding already applied) and the filters phase (FlateDecode for FontFile2/3 and for the
  bundled-asset pipeline) — see [PLAN.md](../PLAN.md) for their numbers. `tinker-pdf-crypto`
  is only an indirect dependency through COS. The leaf crate itself depends on nothing, which
  is what keeps its fuzz targets honest.
- **Wave 2 additionally needs:** the path filler from [07-raster](07-rasterizer.md); Type3 needs
  the interpreter from [06-content](06-content-and-text.md) (no crate cycle — the delegating code lives
  in `tinker-pdf-content`).
- **Unblocks:** the text device and text-run positioning in [06-content](06-content-and-text.md), and
  therefore Checkpoint A (wave 1); text rendering in `tinker-pdf-render` and therefore
  Checkpoint B (wave 2); `pdfcmp`'s text corpus becomes meaningful once M9 lands.

## Risks

| Risk | Mitigation |
| --- | --- |
| Unhinted small-size text visibly worse than FreeType-hinted oracle output — the phase's, and likely the project's, single largest visual-parity risk | Stem darkening from parsed stems; per-dpi perceptual thresholds in `pdfcmp` (loose at 72 dpi, tight at 300); one-time human legibility review then pixel-lock against our own output; accept documented divergence rather than build a hinting engine |
| Charstring interpreter edge cases (hintmask counting, seac, flex, othersubrs) produce *silently* wrong outlines that no fuzzer flags | Per-glyph differential rastering vs FreeType through `oracle-diff` in CI, not just page-level diffs; the width-only evaluator cross-checks advances against the full interpreter |
| CFF width-prefix detection bugs skew every advance by nominalWidthX | The 0.1 pt corpus-wide width gate at M3/M5 catches systematic offsets by construction |
| Bundled data blows the wasm budget (CMaps + Liberation, MBs even deflated) | `cmap-predefined` and `bundled-fonts` features; own-filter deflate; sizes measured at M4/M10 and recorded here; `FontProvider` lets wasm hosts drop bundles entirely |
| Symbol/ZapfDingbats substitute gap has no comfortably-licensed answer | Tracked as an open licensing item; Unicode-remap fallback ships regardless; M10 telemetry says how much real-world content actually hits it before more is invested |
| Data licensing (Adobe CMaps, AFMs, AGL, OFL faces) mishandled in a MIT/Apache repo | All vendored with license texts in `THIRDPARTY.md`; build-time compilation keeps raw data out of crates; CI license check taught the code/data distinction |
| Broken embedded fonts in the wild are stranger than the fixture set | Total `load` (never fails), staged degradation to metrics-only then substitute, warning channel for triage, and a `tpdf font dump` subcommand so field reports turn into fixtures fast |
| Recursion bombs: subrs, seac, glyf composites, Type3 procs | One shared depth budget and per-glyph instruction cap; exceeded → notdef + warning; enforced by fuzz targets and explicit cap tests |
| Glyph cache unbounded growth or thrash on pathological documents (thousands of sizes/faces) | Byte-budget LRU with shard eviction; rotated/skewed bypass keeps adversarial transforms out of the cache; growth asserted bounded under fuzz at M9 |

---

See [PLAN.md](../PLAN.md) for the phase index and checkpoint definitions, and
[06-content](06-content-and-text.md) / [07-raster](07-rasterizer.md) for the neighbors this phase feeds.
