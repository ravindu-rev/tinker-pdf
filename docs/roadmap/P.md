# Tier P — performance

[The roadmap](../ROADMAP.md) says how a step is worked and what comes next.

Speed is verified by tests and the engine optimised
against them. Tier 0 builds the instruments (MS-01 to MS-12); this tier
spends them. **Rules every row keeps**: no `unsafe` in engine crates, so
SIMD comes from autovectorisation-friendly code (fixed-width `chunks_exact`
loops over integer slices, bounds checks hoisted, no calls, libm or `u128`
division in inner loops), from per-build target features (`+simd128` for
wasm), and later from `std::simd` once stable; no third-party crates — the
thread pool is the engine's own on `std::thread`; determinism across
targets — parallel work writes into pre-sized outputs and joins in index
order, and a cache hit returns exactly the bytes a miss would compute;
never panic on untrusted input — every accelerator (a page index, xref
acceleration) validates what a document tells it and falls back to the walk
with a typed warning. **Order**: P-01…P-05 first (algorithmic, small, each
a 3–30× win on its operation), then raster, codecs, parallelism and caches,
crypto and shading, then the rest. Each row lands with its counted-work
guard (MS-01).

**How a target gates, with no person in the loop.** "Today" is this
engine's own measurement at `61adcde`, the *planning baseline*, on a 4-vCPU
2.1 GHz x86-64 machine — in-process unless marked CLI, single-threaded
unless marked — and MS-04's committed benchmark set
re-derives each figure in-tree before it is quoted as current. Every target
has a form that a local run checks and fails on, named in the **Gate** column:

- **a factor** — Today ÷ Target, the speed-up the row owes, rounded down
  to three significant figures so a factor is never stricter than the
  target it stands for — against
  the planning baseline. MS-02's paired run builds the baseline engine
  and the head on one machine, runs each operation paired and
  interleaved, and fails when the median paired ratio, head over baseline,
  is above 1 ÷ the operation's committed floor. Floors start at 1, so no
  operation may lose time against the baseline; a P row closes in the commit
  that raises its operations' floors to the table's factors with that run
  green, and a later change that gives some of the speed-up back fails the
  same run (only H-78 lowers a floor). The answer does not depend on the
  machine, because both engines run on it minutes apart. An operation the
  baseline cannot express — a quarter-scale decode, a discarded level — is
  paired with the baseline operation the cell names;
- **calibrated**, for an operation the baseline lacks entirely (a
  capability a later row adds): the target in milliseconds is held as a
  fraction of a calibration operation timed in the same run — the baseline's
  "text of 5 dense pages", 37.6 ms on the planning machine (rescaled from
  100 pages' 751 ms) — so a 0.5 ms target reads as at most 1/75.1 of it on
  whatever machine the run is on;
- **relation**: two operations of one build compared in one run, with no
  baseline;
- **held**: a factor of 1 — the operation may not lose time against the
  baseline;
- **counted** or **MS-05**: a counted-work guard (MS-01) or a peak-memory
  band (MS-05), properties of the binary and the input rather than of the
  machine.

The absolute figures are what the pinned machine checks (MS-03, H-110), a
second and tighter check that never by itself decides whether a target is
met. A target gates as written until the owner re-states it (H-77), never
because of the planning-time comparison that suggested the number, which
is never re-run against a build (H-112).

| Operation | Today | Target | Gate | Row |
| --- | ---: | ---: | --- | --- |
| Open and count a 23 MB file, CLI | 36 ms | ≤ 6 ms | 6× | P-05 |
| Every page's size, 10 000-page document | Θ(n²) (3 000 pages: 2 569 ms) | ≤ 10 ms | 856× at 3 000 pages (2 569 → ≤ 3 ms); relation: 10 000 pages ≤ 3.6 × 3 000 | P-01 |
| Text, 5 dense pages, in-process / CLI | 37.6 / 33.9 ms (rescaled from 100 pages' 751 / 678) | ≤ 5 / ≤ 6.5 ms (≥ 1 000 pages/s) | 7.51× / 5.21× | P-02 |
| Text, 4 000 small pages, CLI | Θ(n²) (10 000 pages: 36.0 s) | ≤ 60 ms (rescaled from 10 000 pages' 150) | 23.3× at 1 000 pages (0.35 s → ≤ 15 ms); relation: 4 000 pages ≤ 4.4 × 1 000 | P-01 |
| Search a phrase over 5 pages, from scratch | 36.1 ms (rescaled from 100 pages' 722) | ≤ 5.5 ms | 6.56× | P-02 |
| Search 100 pages' prebuilt text, plain / whole-word and diacritic-insensitive | 36 / 77 ms | ≤ 6 / ≤ 12 ms | 6× / 6.41× | P-24 |
| Structured JSON, 5 pages, CLI | 96.3 ms (rescaled from 100 pages' 1 926) | ≤ 10 ms | 9.63× | P-02 |
| Display-list recording, a text page | 6.3 ms | ≤ 1.2 ms | 5.25× | P-02 |
| Redact one rectangle on each of 5 text pages | not measured | ≤ 1.5 × text of the same pages | relation | P-02 |
| Reading order inferred / tables inferred, 5 pages | not measured | each ≤ 1.5 × text of the same pages | relation | P-02, FT-30, FT-31 |
| PDF/A, PDF/UA and PDF/X validation, 5 pages | not measured | each ≤ 3 × text of the same pages | relation | P-02, SD-01, SD-04, SD-11 |
| Render the 150-dpi text page (pixels only) | 88 ms | ≤ 15 ms | 5.86× | P-06 |
| Render the text page at 72 / 300 dpi (pixels only) | 54 / 171 ms | ≤ 6 / ≤ 35 ms | 9× / 4.88× | P-06 |
| Render the half-size text page at 150 dpi to PNG, CLI | 100.5 ms (rescaled from A4's 402) | ≤ 17.5 ms | 5.74× | P-04, P-06 |
| Render the quarter-size text page at 300 dpi to PNG, CLI; A4's peak | 73.8 ms (rescaled from A4's 1 180); 258 MB | ≤ 12.5 ms; ≤ 60 MB | 5.9×; MS-05 | P-04, P-06 |
| 250 paths at 150 dpi / 100 paths at 300 dpi | 66.3 / 68.7 ms (rescaled from 5 000 paths' 1 326 / 3 434) | ≤ 22.5 / ≤ 22 ms | 2.94× / 3.12× | P-07 |
| The 250-path page's 100 stroked curves alone (every cap and join, every fifth dashed), 150 dpi | not measured apart | as the page | 2.94×, paired with the baseline's same curves | P-07 |
| 300 triangles at 150 dpi | 44 ms | ≤ 20 ms | 2.2× | P-07 |
| Axial shading over A4 at 150 dpi / over a half-size page at 300 dpi | 124–170 / 168.5 ms (rescaled from A4's 674) | ≤ 25 / ≤ 22.5 ms | 4.96× / 7.48× | P-14 |
| Mesh shadings (`fuzz/corpus/render_page/mesh-shadings.pdf`) at 150 / 300 dpi | 18.1 / 43.1 ms | ≤ 4 / ≤ 10 ms | 4.52× / 4.31× | P-14 |
| Transparency groups and soft masks (`fuzz/corpus/render_page/transparency.pdf`) at 150 dpi | 8.4 ms | ≤ 3 ms | 2.8× | P-25 |
| A page filled by one tiling pattern 50 times / once | not measured | 50 fills ≤ 1.2 × one fill; cell rasterisations ≤ distinct keys | relation; counted | P-23 |
| Image replay, the half-size Flate RGB page at 150 dpi | 87.5 ms (rescaled from A4's 350) | ≤ 10 ms | 8.75× | P-08 |
| An ICC-based CMYK image converted to sRGB, 620×877 | not measured | ≥ 100 Mpixel/s (≤ 5.4 ms) | calibrated | P-22 |
| JPEG page drawn, half-size at 72 dpi / quarter-size at 300 dpi | 163 / 99.1 ms (rescaled from A4's 652 / 1 585) | ≤ 8.75 / ≤ 9.4 ms | 18.6× / 10.5× | P-09 |
| Quarter-size JPEG 2000 page drawn at 72 dpi; A4's peak | 172 ms (rescaled from A4's 2 753); 220 MB | ≤ 15.6 ms; ≤ 140 MB | 11×; MS-05 | P-10 |
| Half-size G4 / JBIG2 scan page at 150 dpi | 160 / 219 ms (rescaled from A4's 641 / 875) | ≤ 12.5 / ≤ 20 ms | 12.8× / 10.9× | P-08, P-11 |
| Quarter-size Flate RGB image page at 72 dpi | 57.8 ms (rescaled from A4's 924) | ≤ 14.4 ms | 4.01× | P-08 |
| PNG encode, 1240×1755 text page / near-blank page | 269 / 145 ms | ≤ 40 / ≤ 15 ms | 6.72× / 9.66× | P-04 |
| Inflate, page-like / incompressible data | 279 / 141 MB/s | ≥ 500 / ≥ 150 MB/s | 1.79× / 1.06× | P-12 |
| LZW decode, page-like data | not measured | ≥ 200 MB/s | calibrated | P-12 |
| Deflate, page-like data | 40 MB/s, output 16 % larger than level 6's | ≥ 120 MB/s within 2 % of level 6's size; fast level ≥ 300 MB/s | 3×; fast level 7.5×; size: a byte count, exact | P-04 |
| Deflate, photo-like data | 28 MB/s, output 9 % larger than level 6's | ≥ 40 MB/s within 2 % of level 6's size; fast level ≥ 150 MB/s | 1.4×; fast level 5.27×; size: a byte count, exact | P-04 |
| JPEG decode 1240×1754, baseline / progressive | 111.5 / 114.5 ms (rescaled from 2480×3508's 446 / 458) | ≤ 15 / ≤ 30 ms | 7.43× / 3.81× | P-09 |
| JPEG decode of the 1240×1754 baseline at quarter scale | not available | ≤ 5 ms (rescaled from 2480×3508's 20) | 22.3×, paired with the baseline's full-size decode | P-09 |
| JPEG 2000 decode, 9/7 at 620×877 / 5/3 at 310×438 | 160 / 102 ms (rescaled from 2480×3508's 2 566 / 6 516) | ≤ 62.5 (≤ 21.9 on 4 threads) / ≤ 42.2 ms | 2.56× (7.33× on 4 threads) / 2.41× | P-10 |
| JPEG 2000 decode with two levels discarded, 9/7 at 620×877 / 5/3 at 310×438 | not available | ≤ 6.3 / ≤ 2.8 ms (rescaled from 2480×3508's 100 / 180) | 25.6× / 36.2×, paired with the baseline's full decode | P-10 |
| JBIG2 generic page / CCITT G4 page decode | 255 / 46 ms | ≤ 60 / ≤ 25 ms | 4.25× / 1.84× | P-11 |
| Rewrite 100 / 1 000 pages, CLI | 14.8 / 668 ms | ≤ 8 / ≤ 60 ms | 1.85× / 11.1× | P-01, P-02 |
| Rewrite 3 000 pages | 5.48 s | ≤ 40 ms | 137× | P-01 |
| Rewrite 100 pages into object streams / linearized | 2.6 / 3.7 ms | ≤ 2.6 / ≤ 3.7 ms | held | P-21 |
| Subset-on-save, 4 pages | 48.3 ms CLI (52.2 in-process) (rescaled from 100 pages' 1 208 / 1 305) | ≤ 4.8 ms | 10× (CLI) | P-03 |
| Merge 4 × 50 pages, CLI / in-process, byte-identical encodings merged undecoded and differing ones compared decoded once a save (ruling 23) | 504 / 1 428 ms (rescaled from 4 × 100's 1 008 / 2 855) | ≤ 20 / ≤ 12.5 ms | 25.2× / 114× | P-03 |
| Encrypt a 2.5 MB image file with AES-256, CLI | 82.8 ms (rescaled from 23 MB's 770) | ≤ 16.1 ms | 5.13× | P-13 |
| AES-256-CBC decrypt / encrypt | 32 / 57 MB/s | ≥ 150 / ≥ 80 MB/s | 4.68× / 1.4× | P-13 |
| SHA-256 | 262 MB/s | ≥ 350 MB/s | 1.33× | P-13 |
| Verify an RSA-2048 signature over 10 MB | about 38 ms (the digest at 262 MB/s; the public operation 0.042 ms) | ≤ 30 ms | 1.26× | P-13 |
| RSA-2048 signature, private key, constant time | 6.0 ms without CRT (not yet a signing path) | ≤ 2 ms | 3×, paired with the baseline's private-exponent `pow` | P-13 |
| ECDSA signature, constant time: P-256 / P-384 / P-521 / Brainpool P-256 | absent (SG-02) | ≤ 0.5 / ≤ 1.2 / ≤ 2.5 / ≤ 1 ms | calibrated | SG-02 |
| Parse each committed TrueType face and every outline (Liberation Serif / Sans / Mono) | 2.1 / 3.0 / 1.8 ms | the same | held | P-21 |
| Parse every committed CFF program and every outline | not measured | ≥ 1 M outlines a second | calibrated | P-26 |
| Shape a 1 000-character paragraph: Latin / Arabic / Devanagari | not measured | ≤ 0.2 / ≤ 0.5 / ≤ 0.6 ms | calibrated | P-26 |
| EPUB open and paginate, a committed book | 17–22 ms | ≤ 8 ms | 2.12× | P-18 |
| EPUB render every page at 72 dpi, CLI | 84–130 ms | ≤ 40 ms | 2.1× | P-18 |
| A generated 30-chapter book: open and paginate / render at 72 dpi | not measured | ≥ 1 000 / ≥ 300 pages a second | calibrated; relation: 30 chapters ≤ 3.3 × 10 chapters | P-18 |
| A generated 20-page HTML report / 20-page Markdown document, laid out and written as PDF | not measured | ≤ 60 / ≤ 50 ms | calibrated | P-18 |
| Each committed SVG, XPS and CBZ fixture opened and its first page rendered | not measured | ≤ 1.5 × rendering the PDF it synthesises, opened from bytes | relation | P-18 |
| 8 half-size text pages at 150 dpi, 1 / 4 threads | 45.6 / 172 pages/s (rescaled from A4's 11.4 / 43) | ≥ 240 / ≥ 800 pages/s | 5.26× / 4.65×; relation: 4 threads ≥ 3.33 × 1 thread | P-15 |
| One 300-dpi page in 4 bands on 4 threads | 2.7× text, 4.1× paths, 5× a JPEG page (through the display list) | ≥ 3× the 1-thread speed | relation | P-15 |
| 8 quarter-size pages at 150 dpi to PNG, CLI, 1 / 4 jobs; 20 A4 pages' peak on 4 | 171 / 43.6 ms (rescaled from 20 A4 pages' 6 847 / 1 743); 262 MB | ≤ 32.5 / ≤ 10 ms; ≤ 120 MB | 5.26× / 4.35×; MS-05 | P-15, P-04 |
| A second render of a page at another dpi | not measured | ≤ 30 % of the first | relation | P-16 |
| Edit one annotation, save incrementally, reopen, 1 000-page file | not measured | ≤ 2 ms plus output bytes | relation: the 1 000-page file ≤ 2 × a 10-page one | P-17 |
| Re-render a changed 100×100 pt annotation | not measured | ≤ 10 % of a full-page render | relation | P-17 |
| Time to first page over a source with 50 ms latency at 10 Mbit/s, the 60-page linearized fixture | 29 696 bytes in 3 requests | ≤ 29 696 bytes in ≤ 6 requests | counted | MS-08 |
| wasm / native, every suite operation | 1.0–1.94× | ≤ 1.5× | relation (MS-10) | P-19 |
| `+simd128` / plain wasm, raster and decoder operations | 1.00–1.11× | ≥ 1.3× | relation (MS-10) | P-19 |
| Open a small file, CLI (held) | 4–5 ms | ≤ 5 ms | held | P-21 |
| Extract a Flate image with predictors (held) | 276 ms | ≤ 280 ms | held | P-21 |
| No-op incremental save, 100 pages (held) | 0.015 ms | ≤ 0.02 ms | held | P-21 |
| A 500-field cascade / 500 appearances regenerated / a 64 KiB script compiled | absent (FJ-07) | ≤ 20 / ≤ 25 / ≤ 5 ms | calibrated | FJ-16 |
| OCR of a 300-dpi printed page on one thread: Latin / Chinese | absent (OC-08a, OC-08e) | ≤ 1.5 / ≤ 4 s | calibrated | OC-17 |

The deflate sizes are byte counts on the bench's own generated inputs: the
size level 6's parameters produce for each input is a planning number kept
beside its generator in `benches/support/inputs.rs` (ruling 17), and the
output is held to 1.02 times it exactly. A capability a later row adds brings
its row into this table in the commit that adds it, with a number.

Each P row below closes when a local MS-02 paired run is green with its
operations' floors raised to the Gate column's factors, its relations
holding and its counted guards passing; the bold absolute figures in its
exit criterion are the pinned machine's (MS-03).

## P-01 — Page index

**Page index**: descend by each `/Pages` node's `/Count` and memoise a flat page index lazily, shared by clones of the document; keep the bounded head-only property for a streamed linearized file; fall back to the walk, with a typed warning, where `/Count` lies

**Exit criterion.** guards A1, A2; streaming-open tests unchanged; **every page's size over 10 000 pages ≤ 10 ms; text of 4 000 small pages ≤ 60 ms (CLI); rewrite of 3 000 pages ≤ 40 ms**

**Doc:** [design/performance.md](../design/performance.md), P-01 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-01.5 | ci | Close the row in MS-02's paired run: every page's size over 3 000 pages raised to a floor of 856x with the relation 10 000 pages <= 3.6 x 3 000, text of 1 000 small pages (CLI) to 23.3x with the relation 10 000 pages <= 11 x 1 000, and rewrite of 3 000 pages to 137x, against the planning baseline; then take the row out of the roadmap. *Note:* The CLI rewrite of 100 and 1 000 pages (1.85x, 11.1x) is shared with P-02 in tier P's Gate column and is raised when P-02 closes, after this row. P-01.1 finds no superlinear site for rewrite of 3 000 pages: the tip rewrites the bench's 3 000 pages inside tier P's 40 ms, so its 137x is held by the tip and recorded here. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those three floors and two relations, A1 and A2 run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally, and every `streaming_open.rs` test passes unchanged. | P-01.3, P-01.4, MS-01.8, MS-02, MS-04 |

## P-02 — Resolved-resource cache

**Resolved-resource cache**: fonts resolved once per resource dictionary, not per glyph; a Type 3 font's `/Differences` built into a code-to-name table once; colour spaces, functions and graphics states resolved once; well-known names interned as constants

**Exit criterion.** guard A3; **text of 5 dense pages ≤ 5 ms (≥ 1 000 pages/s, ≥ 5.6 M characters/s), ≤ 6.5 ms CLI; display-list recording ≤ 1.2 ms a page; structured JSON of 5 pages ≤ 10 ms CLI; a phrase searched over 5 pages from scratch ≤ 5.5 ms**; relations: redaction of a rectangle per page, reading-order and table inference each ≤ 1.5 × and PDF/A, UA and X validation each ≤ 3 × text of the same pages

**Doc:** [design/performance.md](../design/performance.md), P-02 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-02.8 | ci | Close the row in MS-02's paired run: text of 5 dense pages raised to 7.51x in-process and 5.21x CLI, display-list recording to 5.25x, structured JSON of 5 pages (CLI) to 9.63x, a phrase searched from scratch to 6.56x, and the CLI rewrite of 100 and 1 000 pages (shared with P-01) to 1.85x and 11.1x, with the relations holding: redaction, reading order and tables each <= 1.5 x, PDF/A, UA and X validation each <= 3 x text of the same pages; then take the row out of the roadmap. *Note:* After P-01, whose page lookups the shared rewrite floors and inference's `pages::count` and `pages::at` calls rest on. Tier P's table gives the relations no figure today; a relation that misses is H-77's or H-78's to re-state, not a band to widen. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those seven floors and six relations, and A3 and P-02's counted guards run green in `cargo test` in both builds on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-02.5, P-02.6, P-02.7, P-01, MS-01.8, MS-02, MS-04 |

## P-03 — A decoded-stream cache per operation, and deduplication on encoded bytes

**A decoded-stream cache per operation, and deduplication on encoded bytes** (bucket by encoded length and a 64-bit hash of encoded bytes and filter chain, confirm byte for byte, so byte-identical encodings merge undecoded; differing encodings under one dictionary are compared decoded, each once a save, as ruling 23 answers H-137)

**Exit criterion.** guards A5, A6; **subset-on-save of 4 pages ≤ 4.8 ms; merge of 4 × 50 pages ≤ 20 ms CLI (≤ 12.5 ms in-process)**, byte-identical encodings merged undecoded and differing ones compared decoded once a save (ruling 23); merge's peak held by an MS-05 band set at the fixed figure

**Doc:** [design/performance.md](../design/performance.md), P-03 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-03.7 | ci | Close the row in MS-02's paired run: subset-on-save of 4 pages (CLI) raised to 10x and merge of 4 x 50 pages, byte-identical encodings merged undecoded and differing ones compared decoded once a save (ruling 23), to 25.2x CLI and 114x in-process, against the planning baseline, with MS-05's merge 4 x 100 band set at the figure the fixed merge peaks at; then take the row out of the roadmap. *Note:* After P-01 and P-02: a merge imports every page through the editor's page lookups (P-01), and the CLI subsets on save, whose glyph walk clones a font dictionary per glyph until P-02. The band is set at the fixed figure because the row names none. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `crates/tinker-pdf/tests/peak_memory.tsv` (new), `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those three floors, MS-05's merge 4 x 100 case in `peak_memory.tsv` holds its new figure at 2 % on Linux in both builds, and A5 and A6 run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-03.2, P-03.6, P-01, P-02, MS-01.8, MS-02, MS-04, MS-05 |

## P-04 — PNG and deflate

**PNG and deflate**: rows streamed into a streaming deflate (no whole filtered image), one filter choice per row in one vectorisable pass, the hash-chain table a 32 K-entry ring instead of one `usize` per input byte (`crates/tinker-pdf-filters/src/deflate.rs:78`), bounded chains and lazy matching; a fast level for rendered output beside the ratio level for document writing

**Exit criterion.** guard A8 and an MS-05 band; **PNG of the text page ≤ 40 ms, near-blank ≤ 15 ms, scratch ≤ 1.2 × the image; deflate of page-like data ≥ 120 MB/s and of photo-like data ≥ 40 MB/s, each within 2 % of level 6's size, fast level ≥ 300 and ≥ 150 MB/s; render the 150-dpi text page to PNG (CLI) ≤ 70 ms, the 300-dpi one ≤ 200 ms and ≤ 60 MB peak**

**Doc:** [design/performance.md](../design/performance.md), P-04 · **Depends on:** MS-01, MS-05

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-04.6 | ci | Raise the encoder's floors in MS-02's paired run: PNG of the 1240x1755 text page to 6.72x and of the near-blank page to 9.66x; deflate of page-like data to 3x at the ratio level and 7.5x at the fast level, and of photo-like data to 1.4x and 5.27x; the ratio level's two sizes stay held at 1.02 x level 6's, exact, by `deflate_size.rs`. *Note:* The deflate floors are the levels' own, reached through the filters crate, so they do not wait on document writing's switch (P-04.7). The fast level has no baseline twin and is paired with the baseline's deflate, as the Gate column's factor says. A byte count is no gate kind of MS-02's paired run (floors, calibrations, relations and holds, MS-02.3), so the size bound stays the test's. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md` | A local MS-02 paired run, recorded here, is green with those six floors, `deflate_size.rs`'s `the_ratio_level_holds_level_six_size` passes in `cargo test`, and A8 runs green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-04.5, MS-01.8, MS-02, MS-04 |
| P-04.8 | ci | Close the row in MS-02's paired run: the 150-dpi text page rendered to PNG (CLI) raised to 5.74x and the 300-dpi one to 5.9x against the planning baseline, with MS-05's band for rendering at 300 dpi to PNG at 60 MB; then take the row out of the roadmap. *Note:* Shared with P-06 in tier P's Gate column: the render half is P-06's glyph cache, so these floors rise once both rows have landed. The two CLI render-to-PNG operations are MS-04's, run through the same-revision `tpdf`. The 20-page CLI render on 1 and 4 jobs is P-15's. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `crates/tinker-pdf/tests/peak_memory.tsv` (new), `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those two floors and P-04.6's six, and MS-05's 300-dpi render-to-PNG case in `peak_memory.tsv` holds at 60 MB or less on Linux in both builds. | P-04.6, P-04.7, P-06, MS-05 |

## P-05 — A file-backed source in the CLI and bindings

**A file-backed source in the CLI and bindings**: positioned reads of the tail, the cross-reference data and objects on demand; no second copy of the input; an `Arc<[u8]>` input stays zero-copy in the facade (`Document::open` copies a slice it is handed, `crates/tinker-pdf/src/lib.rs:1399`)

**Exit criterion.** **open and count a 23 MB file ≤ 6 ms CLI, peak ≤ 10 MB** (MS-05 band "open 23 MB")

**Doc:** — · **Depends on:** MS-05

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-05.9 | ci | Close the row in MS-02's paired run: opening and counting the 23 MB file (CLI) raised to a floor of 6x against the planning baseline, with MS-05's 'open 23 MB' band at 10 MB; then take the row out of the roadmap. *Note:* P-13's 23 MB CLI encrypt floor waits on this row, since opening its input is P-05's. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `crates/tinker-pdf/tests/peak_memory.tsv` (new), `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the 6x floor, MS-05's open-23-MB case in `peak_memory.tsv` holds at 10 MB or less on Linux in both builds, and `streaming_open.rs`, `streaming_determinism.rs` and the CLI's tests pass on Linux and Windows under CI. | P-05.4, P-05.5, P-05.6, P-05.7, P-05.8, MS-02, MS-04, MS-05 |

## P-06 — Glyph coverage cache

**Glyph coverage cache** keyed by font, glyph, quantised 2×2 transform and sub-pixel phase, bounded by bytes with deterministic eviction; masks blitted through the existing mask path

**Exit criterion.** guard A4; **text page ≤ 6 ms at 72 dpi, ≤ 15 ms at 150, ≤ 35 ms at 300**; if phase quantisation changes pixels, the one-time fingerprint change is reviewed (H-76)

**Doc:** [design/performance.md](../design/performance.md), P-06 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-06.4 | ci | Close the row in MS-02's paired run: the text page rendered at 72, 150 and 300 dpi (pixels only) raised to 9x, 5.86x and 4.88x against the planning baseline; then take the row out of the roadmap. *Note:* After P-02: every glyph a render draws also passes `type3_glyph` (`crates/tinker-pdf-content/src/interpret.rs:2085`), which clones its font dictionary until P-02 lands. The render-to-PNG floors shared with P-04 rise in P-04's last step. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those three floors, and A4 and the determinism fingerprints run green in `cargo test` in both builds on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-06.3, P-02, MS-01.8, MS-02, MS-04 |

## P-07 — Rasteriser and stroker

**Rasteriser and stroker**: sort-free signed-area accumulation (integer cells, prefix-summed per scanline), a solid-span fast path for interior runs, a Normal-blend opaque fast path that skips the general blend, scanline scratch reused; the stroker emitting its outline straight into the accumulator and cutting dashes along each flattened segment without re-flattening

**Exit criterion.** fingerprints unchanged on all four targets; **250 paths ≤ 22.5 ms at 150 dpi and 100 paths ≤ 22 ms at 300; 300 triangles ≤ 20 ms at 150; the 250-path page's 100 stroked curves alone 2.9× faster; fills ≥ 2× faster with `+simd128` than without**

**Doc:** [design/rasterizer-accumulation.md](../design/rasterizer-accumulation.md) · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-07.9b | measure | The `+simd128` relation the fill loops are written for, taken in the timing phase on the named machine with nothing else running (ruling 15): MS-10's wasm legs and `cargo xtask bench-check --wasm` report the `+simd128`/plain gain of `render/fill 300 anti-aliased paths at 150 dpi`, `render/250 paths at 150 dpi` and `render/100 paths at 300 dpi`, committed as those three operations' `gain` figures in `wasm.json`, as ruling 16 says. *Note:* Split from P-07.9 by the owner's timing choice; P-07.9a landed the loops unmeasured: a touched cell's lanes holding one crossing taken in one sixteen-wide `i32` loop with no branch, the rule a mask on the winding; an edge none of whose crossings can saturate stepped by one add a sub-scanline; the opaque Normal composite walked in blocks of sixteen pixels, a block of full coverage over opaque pixels one slice copy and any other over opaque pixels one loop of sixteen-bit lanes. An operation short of 2x is a finding against the loop that owns it; only H-78 lowers the figure once set. | `crates/tinker-pdf/benches/wasm.json` | MS-10's local run reports `+simd128` over plain at or above 2x on the three operations, committed as their gain figures, and `cargo xtask baseline` holds them; P-07.2's harness, every fingerprint on the four targets and every golden unchanged. | MS-04.10, MS-10.1b |
| P-07.10 | ci | Close the row in MS-02's paired run: raise the floors of 250 paths at 150 dpi to 2.94x and 100 paths at 300 dpi to 3.12x, of the 250-path page's 100 stroked curves alone to 2.94x paired with the planning baseline's same curves, and of 300 triangles at 150 dpi to 2.2x; hold P-07.9's `+simd128` relation in MS-10's run; take the row out of the roadmap. *Note:* MS-04 brings the 250- and 100-path pages and the first's stroked curves into the render group; the 300 triangles are today's `fill 300 anti-aliased paths at 150 dpi` (`crates/tinker-pdf/benches/render.rs`). The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those four floors and MS-10's with the relation; `rendering_is_stable_across_targets` reproduces every fingerprint unchanged under `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally, and the facade's suite passes in the default build and with `--features bundled-fonts`; P-07's counted guards run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-07.9a, MS-01.8, MS-02, MS-04, MS-10 |

## P-08 — Image drawing

**Image drawing**: an integer fixed-point resampler with per-column weights computed once (no `u128`, no libm), an 8-bit Gray/RGB identity fast path, a 1-bit to 8-bit table, bilevel images drawn as coverage masks, and a decode-to-target-size handshake with P-09 and P-10

**Exit criterion.** **the half-size Flate RGB page's image replay ≤ 10 ms at 150 dpi; the half-size G4 scan page ≤ 12.5 ms and JBIG2 scan page ≤ 20 ms at 150; the quarter-size Flate RGB image page ≤ 14.4 ms at 72**

**Doc:** [design/image-edges.md](../design/image-edges.md), P-08 · **Depends on:** P-09, P-10

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-08.9 | test | The decode-to-target-size handshake held end to end once P-09 and P-10 answer it: JPEGs decoded at 1/2, 1/4 and 1/8 and JPEG 2000 codestreams with levels discarded and windows, of odd sizes and drawn axis-aligned, flipped, quarter-turned and sheared at the sizes that pick each answer, through the integer resampler, checked for where they land, how far they were reduced and that no reduced grid is drawn magnified. *Note:* No engine change: the decode half is P-09's drawn-size request and P-10.8 and P-10.9; this row's share is that its resampler draws their answers, which this suite holds across codecs and transforms. | `crates/tinker-pdf/tests/image_requests.rs` (new) | `image_requests.rs` (new) `every_reduced_answer_lands_where_the_full_decode_does`: ink bounds and the unit square's edge coverage equal the full decode's; `a_reduced_answer_is_never_magnified`: the decode's output, read from MS-01's decode stats, has at least as many samples as drawn pixels along each image axis; guard A7 (IDCT output at most 4x drawn pixels, JPX levels at most those needed) holds; every tile is byte-equal to the page (ruling 5). | P-08.3, P-09, P-10 |
| P-08.10 | ci | Close the row in MS-02's paired run: raise the floors of the half-size image replay at 150 dpi to 8.75x, the half-size G4 scan page at 150 dpi to 12.8x, the half-size JBIG2 scan page at 150 dpi to 10.9x and the quarter-size Flate RGB image page at 72 dpi to 4.01x against the planning baseline, and take the row out of the roadmap. *Note:* The two scan pages share their gate with P-11 in tier P's table, and P-11's decode speed-ups are part of those floors, which is why P-11 is a dependency here though the row names only P-09 and P-10. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those four floors; P-08's counted guards run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally; the `image`, `bilevel`, `jbig2`, `jpx` and `cbz` fingerprints hold on the four targets. | P-08.8, P-08.9, P-11, MS-01.8, MS-02, MS-04 |

## P-09 — JPEG decoder

**JPEG decoder**: look-ahead Huffman tables, an integer IDCT over 32-bit lanes, fused upsampling and YCbCr→RGB on row strips, **DCT-domain scaling at 1/2, 1/4 and 1/8 chosen from the drawn size**, strip output with no whole-image planes

**Exit criterion.** guard A7; **baseline ≤ 60 ms (≥ 145 Mpixel/s), progressive ≤ 120 ms, quarter-scale ≤ 20 ms, peak ≤ 1.3× output; a full-page JPEG drawn at 72 dpi ≤ 35 ms, at 300 dpi ≤ 150 ms**

**Doc:** [design/jpeg-speed.md](../design/jpeg-speed.md) · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-09.9b | measure | The wall-clock figure P-09.9a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-09.9a's exit names one, and committed as ruling 16 says. *Note:* Split from P-09.9 by the owner's timing choice; P-09.9a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-09.9a's exit names is committed and `cargo xtask baseline` holds it. | P-09.9a, MS-04.10 |
| P-09.11 | ci | Close the row in MS-02's paired run: raise the floors of the 1240x1754 baseline and progressive decodes to 7.43x and 3.81x, the quarter-scale decode to 22.3x against the baseline's full-size decode, and the JPEG page drawn half-size at 72 dpi and quarter-size at 300 dpi to 18.6x and 10.5x; add the 2480x3508 baseline decode to MS-05's probe as a case of its own and commit its band at its measured peak, at or under 1.3x its output; take the row out of the roadmap. *Note:* The bold absolute figures, 145 Mpixel/s for the baseline decode among them, are MS-03's second check, never this run's. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md`, `crates/tinker-pdf/tests/peak_memory.rs` (new), `crates/tinker-pdf/tests/peak_memory.tsv` (new) | A local MS-02 paired run, recorded here, is green with those five floors; MS-05's peak-memory test holds the baseline decode within its band at or under 1.3x its output; guard A7's JPEG half and P-09's counted guards pass in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-09.8, P-09.9a, MS-01.8, MS-02, MS-04, MS-05 |

## P-10 — JPEG 2000 decoder

**JPEG 2000 decoder**: tier-1 with packed per-stripe significance state, the MQ decoder kept in registers, row-blocked lifting, **resolution-level discard** and window decode from the drawn size, independent code-blocks decoded in parallel

**Exit criterion.** guard A7; **9/7 ≤ 1 000 ms (≤ 350 ms on 4 threads); 5/3 ≤ 2 700 ms; two levels discarded ≤ 100 ms (9/7) and ≤ 180 ms (5/3); a JPX page at 72 dpi ≤ 250 ms and ≤ 140 MB**

**Doc:** [design/jpx-speed.md](../design/jpx-speed.md) · **Depends on:** P-15

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-10.4b | measure | The wall-clock figure P-10.4a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-10.4a's exit names one, and committed as ruling 16 says. *Note:* Split from P-10.4 by the owner's timing choice; P-10.4a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-10.4a's exit names is committed and `cargo xtask baseline` holds it. | P-10.4a, MS-04.10 |
| P-10.12a | measure | The parallel decode's speed row: the 620x877 9/7 decode on four threads joins MS-04's decode group, paired with the planning baseline's single-thread full decode of the same codestream, the pairing behind the Gate column's 7.33x on 4 threads. *Note:* The threads exist only once P-10.10 and P-10.11 land, so MS-04 cannot carry this operation itself; without it P-10.13 would raise a floor no benchmark measures. | `crates/tinker-pdf/benches`, `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md` | `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree` holds the bench file and `baseline.json` to one list with the four-thread operation in it, and its first paired ratio against the planning baseline, taken by a local MS-02 paired run, is recorded in `docs/verification.md`. | P-10.11, MS-02, MS-04 |
| P-10.12b | measure | The wall-clock figure P-10.12a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-10.12a's exit names one, and committed as ruling 16 says. *Note:* Split from P-10.12 by the owner's timing choice; P-10.12a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-10.12a's exit names is committed and `cargo xtask baseline` holds it. | P-10.12a, MS-04.10 |
| P-10.13 | ci | Close the row in MS-02's paired run: raise the JPX operations' floors to the Gate column's factors (9/7 2.56x and 7.33x on 4 threads, 5/3 2.41x, two levels discarded 25.6x for 9/7 and 36.2x for 5/3 against the baseline's full decode, the JPX page at 72 dpi 11x), commit MS-05's JPX-page band at its measured peak, at or under 140 MB, and take the row out of the roadmap. *Note:* The row's bold absolute figures are the pinned machine's second check (MS-03, H-110), which never by itself decides the row. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those floors; guard A7's JPX half passes in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally; MS-05's peak-memory test holds the JPX page at 72 dpi within its band at or under 140 MB. | P-10.4a, P-10.5, P-10.6, P-10.7, P-10.8, P-10.9, P-10.10, P-10.11, P-10.12a, MS-02, MS-04, MS-05 |

## P-11 — JBIG2 and CCITT

**JBIG2 and CCITT**: a rolling context register for generic and refinement regions, word-at-a-time typical prediction; CCITT run tables

**Exit criterion.** **JBIG2 generic page ≤ 60 ms; G4 page ≤ 25 ms**

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-11.5 | ci | Close the row in MS-02's paired run: the JBIG2 generic page and the CCITT G4 page of MS-04's decode group, generated in the tree since the page the 255 ms was measured on is not, paired against the planning baseline with floors raised to 4.25x and 1.84x; the in-tree figures are recorded in `docs/verification.md`, which holds no JBIG2 or G4 figure today, and the row leaves the roadmap. *Note:* The rolling register landed after the planning baseline, so the paired factor already counts its gain. The outside page is never fetched; MS-04's generated page stands in for it, as the tier's in-tree re-derivation asks. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the JBIG2 generic page at a floor of 4.25x and the G4 page at 1.84x against the planning baseline, and P-11's four counted guards run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-11.1, P-11.2, P-11.4, MS-01.8, MS-02, MS-04 |

## P-12 — Inflate and LZW

**Inflate and LZW**: combined literal/length tables, a 64-bit bit buffer, word-sized match copies with overlap handling; LZW strings decoded from a table of (prefix, byte) entries written backwards into place

**Exit criterion.** **inflate ≥ 500 MB/s page-like, ≥ 150 MB/s incompressible; LZW ≥ 200 MB/s page-like**

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-12.5 | ci | Close the row in MS-02's paired run: inflate of MS-04's page-like and near-incompressible data with floors raised to 1.79x and 1.06x against the planning baseline, and LZW of page-like data held at 200 MB/s or better through the run's calibration operation; then take the row out of the roadmap. *Note:* LZW gates as a calibration, as the Gate column says. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the two inflate floors and the LZW calibration, and P-12's counted guards run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-12.3, P-12.4, MS-01.8, MS-02, MS-04 |

## P-13 — Crypto throughput, constant-time

**Crypto throughput, constant-time**: byte-sliced AES with multi-block CBC decryption; a SHA-256 message-schedule rewrite; fixed-window Montgomery exponentiation with CRT and blinding for private-key operations (shared with SG-01)

**Exit criterion.** **AES-256-CBC decrypt ≥ 150 MB/s, encrypt ≥ 80 MB/s; SHA-256 ≥ 350 MB/s; RSA-2048 signature ≤ 2 ms constant-time; an RSA-2048 signature over 10 MB verified ≤ 30 ms; encrypt a 2.5 MB image file ≤ 16.1 ms CLI**; a timing-independence test of the private-key path by operation count (no secret-dependent branch counts)

**Doc:** [design/crypto-throughput.md](../design/crypto-throughput.md) · **Depends on:** SG-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-13.9 | ci | Close the row in MS-02's paired run: floors raised to AES-256-CBC decrypt 4.68x and encrypt 1.4x, SHA-256 1.33x, an RSA-2048 signature verified over 10 MB 1.26x, the RSA-2048 private-key signature 3x against the baseline's private-exponent `pow`, and the 23 MB CLI encrypt 5.13x; then take the row out of the roadmap. *Note:* MS-04's sign group times the RSA-2048 private-key operation as the private-exponent `pow`, the pairing the 3x floor needs. Opening the 23 MB input is P-05's (today the CLI reads it and copies it into an `Arc<[u8]>`), so the CLI encrypt's floor waits on it. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those six floors, and the private-key timing-independence test (no secret-dependent branch counts) and the two AES trace tests run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-13.3, P-13.6, P-13.7, P-13.8, P-05, MS-01.8, MS-02, MS-04, H-167 |

## P-14 — Shading

**Shading**: per-scanline incremental parameters; functions sampled into a lookup table built with `tinker-pdf-math`, integer interpolation for types 2 and 3 and sampled functions; mesh triangles scan-converted with colour stepped incrementally along each span

**Exit criterion.** fingerprints; **axial ≤ 25 ms at 150 dpi, ≤ 90 ms at 300; the mesh fixture ≤ 4 ms at 150, ≤ 10 ms at 300**

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-14.6 | ci | Close the row in MS-02's paired run: the full-page axial shading at 150 and 300 dpi with floors raised to 4.96x and 7.48x, and the committed mesh fixture (`fuzz/corpus/render_page/mesh-shadings.pdf`) at 150 and 300 dpi to 4.52x and 4.31x, against the planning baseline; then take the row out of the roadmap. *Note:* The 300 dpi axial and both mesh operations enter MS-04's render group with MS-04; the 150 dpi axial is today's `render a full-page axial shading at 150 dpi` in `benches/render.rs`. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those four floors; `determinism.rs`'s shading, pattern, mesh and analytic fingerprints hold under `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally; P-14's counted guards pass in `cargo test`. | P-14.3, P-14.4, P-14.5, MS-02, MS-04 |

## P-15 — The engine's own thread pool on the facade

**The engine's own thread pool on the facade**: `Parallelism::Serial` by default or `Threads(n)`; page-parallel render, text and subset walks; per-object stream compression on save; band-parallel single-page render from a display list

**Exit criterion.** guard A9; **8 half-size text pages at 150 dpi ≥ 240 pages/s on 1 thread and ≥ 800 pages/s on 4; one 300-dpi page in 4 bands ≥ 3× the 1-thread speed; 8 quarter-size pages at 150 dpi to PNG (CLI) ≤ 32.5 ms on 1 job and ≤ 10 ms on 4, and 20 A4 pages on 4 jobs ≤ 120 MB**

**Doc:** [design/parallelism.md](../design/parallelism.md) (amends [architecture.md](../architecture.md)) · **Depends on:** H-16, MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-15.10b | measure | The wall-clock figure P-15.10a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-15.10a's exit names one, and committed as ruling 16 says. *Note:* Split from P-15.10 by the owner's timing choice; P-15.10a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-15.10a's exit names is committed and `cargo xtask baseline` holds it. | P-15.10a, MS-04.10 |
| P-15.11 | ci | Close the row in MS-02's paired run: '8 half-size text pages at 150 dpi' floors raised to 5.26x on 1 thread and 4.65x on 4 with the relation 4 threads at least 3.33 times 1 thread, the 4-band relation at least 3 times 1 thread, '8 quarter-size pages to PNG at 150 dpi, CLI' to 5.26x on 1 job and 4.35x on 4; MS-05's 20-page case moved to the facade's pool and committed at or under 120 MB; then take the row out of the roadmap. *Note:* The one-thread factors are 5.26 times a single thread's own work, which no pool supplies: the 150-dpi text page is P-06's and the PNG path P-04's, hence both rows here. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `crates/tinker-pdf/tests/peak_memory.rs` (new), `crates/tinker-pdf/tests/peak_memory.tsv` (new), `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those floors and relations; MS-05's peak-memory test holds 20 pages at 150 dpi on 4 jobs within its band at or under 120 MB on linux; guard A9 passes in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-15.4, P-15.5, P-15.9, P-15.10a, P-04, P-06, MS-02, MS-04, MS-05, MS-25.2b |

## P-16 — Caches with byte budgets and a caller memory budget

**Caches with byte budgets and a caller memory budget**: decoded images and form display lists per document; ICC transforms; glyph masks (P-06); `CacheLimits { bytes }` with deterministic eviction; degrade by reduced-resolution decode rather than abort; drop parsed objects under pressure

**Exit criterion.** counted hits and misses; **a second render of a page at another dpi ≤ 30 % of the first; resident memory within the stated budget under the MS-05 cases**

**Doc:** [design/performance.md](../design/performance.md), P-16 · **Depends on:** MS-05

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-16.9 | build | Reduced-resolution decode rather than abort: an image whose full decode would not fit what the budget leaves is decoded at the largest reduction that fits, through P-09's DCT scaling or P-10's level discard, drawn and named by a new typed `RenderWarning`; the cache keys a reduced decode by its request, so a hit is the bytes that request's miss computes, and with no budget stated nothing changes. *Note:* The warning joins rendering's refusal table (ruling 10). An image no reduction fits is handled as P-16.1 states 'abort' was handled before. | `crates/tinker-pdf/src/resources.rs`, `crates/tinker-pdf/src/cache.rs` (new), `crates/tinker-pdf-render/src/lib.rs`, `crates/tinker-pdf/tests/caches.rs` (new), `docs/features/rendering.md` | `caches.rs` `an_image_past_the_budget_draws_at_a_reduced_resolution_and_says_so` (new): a JPEG and a JPEG 2000 page under budgets smaller than their full decodes draw from the reduced decode, report the warning and hold their bytes under the budget; with no budget every fingerprint and `render_regions.rs` pass unchanged. | P-16.2, P-09, P-10 |
| P-16.10a | measure | The caches' speed and memory rows: MS-04's 'a second render at another dpi' run over a document opened with a stated budget, so its second render is served by the cache, and MS-05's seven cases each run under a stated budget beside their unbudgeted figures; first figures recorded. *Note:* The budget each case states is the one P-16.1 sets for it; a case whose peak passes its budget is a finding against the kind that held the bytes, never a larger budget. | `crates/tinker-pdf/benches/render.rs`, `crates/tinker-pdf/benches/baseline.json`, `crates/tinker-pdf/tests/peak_memory.rs` (new), `crates/tinker-pdf/tests/peak_memory.tsv` (new), `docs/verification.md` | `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree` holds the operation with its stated budget, and its first ratio to the first render in one run is recorded in `docs/verification.md`; MS-05's probe prints `RAN` and each case's peak under its stated budget on linux, recorded beside the unbudgeted peaks. | P-16.4, P-16.6, P-16.7, P-16.8, P-16.9, MS-02, MS-04, MS-05 |
| P-16.10b | measure | The wall-clock figure P-16.10a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-16.10a's exit names one, and committed as ruling 16 says. *Note:* Split from P-16.10 by the owner's timing choice; P-16.10a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-16.10a's exit names is committed and `cargo xtask baseline` holds it. | P-16.10a, MS-04.10 |
| P-16.11 | ci | Close the row: MS-02's paired run holds the relation 'a second render of a page at another dpi' at most 30 % of the first, MS-05's test holds each case's resident memory within its stated budget at its committed figure, and P-16's counted guards run on every target; then take the row out of the roadmap. *Note:* A relation of one build in one run, as tier P's table gives it, so it needs no baseline. The bold figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `crates/tinker-pdf/tests/peak_memory.tsv` (new), `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the second-render relation; MS-05's peak-memory test holds every case within its stated budget on linux; `caches.rs`'s guards pass in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-16.3, P-16.10a, MS-02, MS-05, MS-25.2b |

## P-17 — Incremental reparse and dirty-region re-render

**Incremental reparse and dirty-region re-render**: a copy-on-write overlay so an edit, an incremental save and a reopen cost O(changed objects); re-render only the changed rectangle through the region path (ruling 5)

**Exit criterion.** counted objects re-parsed ≤ changed + cross-reference; **one annotation edit, incremental save and reopen of a 1 000-page file ≤ 2 ms plus output bytes; re-render of a changed 100×100 pt annotation ≤ 10 % of a full-page render**

**Doc:** [design/performance.md](../design/performance.md), P-17 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-17.8b | measure | The wall-clock figure P-17.8a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-17.8a's exit names one, and committed as ruling 16 says. *Note:* Split from P-17.8 by the owner's timing choice; P-17.8a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-17.8a's exit names is committed and `cargo xtask baseline` holds it. | P-17.8a, MS-04.10 |
| P-17.9 | ci | Close the row in MS-02's paired run: the relations 'the 1 000-page edit, save and reopen at most 2 times the 10-page one' and 'the changed 100x100 pt annotation re-rendered at most 10 % of a full-page render', with P-17's counted guards green on every target; then take the row out of the roadmap. *Note:* P-01 is here because until its index lands a lookup of the edited page walks the tree from the root (`crates/tinker-pdf-cos/src/pages.rs:488`), a cost that grows with the page count inside the very operation the relation holds to twice the 10-page one. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with both relations; `reopen.rs`, `incremental_reopen.rs` and `dirty_region.rs`'s counted guards pass in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-17.7, P-17.8a, P-01, MS-02 |

## P-18 — EPUB, HTML and the containers

**EPUB, HTML and the containers**: font choice by pre-folded family keys instead of substring scans per run; cascade memoisation; CRC once per entry; HTML and Markdown creation and SVG, XPS and CBZ synthesis on the same paths, each linear in its input

**Exit criterion.** guard A2 over a generated 1 000-chapter book; **open and paginate a committed book ≤ 8 ms; render every page at 72 dpi ≤ 40 ms CLI; the 30-chapter book paginated at ≥ 1 000 pages a second and rendered at 72 dpi at ≥ 300; a 20-page HTML report ≤ 60 ms and a 20-page Markdown document ≤ 50 ms to PDF**; relation: each committed SVG, XPS and CBZ fixture opened and its first page rendered ≤ 1.5 × rendering its synthesised PDF from bytes

**Doc:** — · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-18.6 | ci | Close the row in MS-02's paired run: floors raised to 2.12x for opening and paginating a committed book and to 2.1x for rendering its every page at 72 dpi through the CLI; the 30-chapter book at 1 000 pages a second paginated and 300 rendered at 72 dpi, and the 20-page HTML report and Markdown document at 60 and 50 ms, calibrated; 30 chapters at most 3.3 times 10; each SVG, XPS and CBZ fixture at most 1.5 times its synthesised PDF; then take the row out. *Note:* `tpdf render` writes every page through `Bitmap::to_png` (`tools/tpdf/src/main.rs:1299-1304`), so the CLI render-all figure carries P-04's encoder and P-06's text pages, hence both rows here. The bold absolute figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with those floors, calibrations and relations; guard A2 over the 1 000-chapter book and `every_producer_is_linear_in_its_input` pass in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally. | P-18.5, P-04, P-06, MS-02, MS-04 |

## P-19 — wasm

**wasm**: publish with `+simd128`; decoder tables computed at first use rather than static, to protect the size budget; measure in a browser-class host as well as a standalone runtime

**Exit criterion.** **wasm ≤ 1.5× native on every suite operation; `+simd128` ≥ 1.3× the plain build on raster and decoder operations after P-07…P-12; no growth past the size budget**

**Doc:** — · **Depends on:** MS-10, H-44

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-19.2 | build | Publish with `+simd128` and a plain fallback, H-44's default: the npm package carries the binding built with `-C target-feature=+simd128` and built plain, and its entry point validates a small SIMD module and loads the plain build where the host lacks SIMD; `release.yml`'s npm job builds both and holds both to its size gate, the local `bindings-parity` run installs the pair, and MS-06's footprint check gains the `simd128` build as a profile with its figure. *Note:* No threaded build, H-44's other default. `release.yml` checks one `.wasm` today (`.github/workflows/release.yml:400-406`), and the README's 'one build' argument (`bindings/js/README.md`) is restated: one source, two target-feature builds held byte-identical. | `bindings/js/loader.mjs` (new), `bindings/js/README.md`, `bindings/js/tests/node_smoke.mjs`, `.github/workflows/release.yml`, `bindings/js/footprint.json`, `docs/features/bindings.md` | `bindings/js/tests/node_smoke.mjs` renders a page through the installed package with SIMD and again with the probe forced to fail, the two bitmaps byte-identical (ruling 4), in a local `bindings-parity` run, recorded here, and in `release.yml`'s npm job; MS-06's local footprint check records the `simd128` profile and holds the default and `bundled-fonts` ones; `release.yml` expects two `.wasm` files. | H-44, MS-06 |
| P-19.3 | ci | A browser-class host beside the standalone runtime: the suite operations the JavaScript binding reaches (open, render, text and save over MS-04's generated fixtures) timed in a JavaScript host whose WebAssembly engine is a browser's, the host `release.yml`'s npm job already installs the package in, for the `+simd128` and plain builds, each operation's wasm/native ratio and SIMD gain printed beside MS-10's standalone figures. *Note:* The host runs the code and judges nothing (ruling 13). An operation the binding does not reach (a raw decode, a font parse) is measured in the standalone runtime alone and listed so in the run's output. | `bindings/js/bench/host_bench.mjs` (new), `xtask/src/bench.rs`, `docs/verification.md` | A local run beside MS-10's prints, for every operation the binding reaches, both hosts' wasm/native ratio and SIMD gain; `xtask/src/bench.rs` `a_host_leg_missing_an_operation_is_refused` (new) passes; its first run's figures are recorded in `docs/verification.md`. | P-19.2, MS-10.2 |
| P-19.4 | measure | After P-07 to P-12, the figures that close the row: MS-10's run and P-19.3's host leg run locally over the whole set, each operation's wasm/native ratio in both hosts and the `+simd128` gain on the raster and decoder operations recorded, every operation above 1.5x and every raster or decoder operation under 1.3x named with the row whose loop it times. *Note:* An operation that misses is a finding against the row that owns its loop (tier P's autovectorisation rule), filed before P-19.5, never an eased figure. | `crates/tinker-pdf/benches/wasm.json`, `docs/verification.md` | `docs/verification.md` holds the table, regenerated by MS-10's `bench-check --wasm` and P-19.3's leg, and `cargo xtask baseline` holds `wasm.json` to the set. | P-07, P-08, P-09, P-10, P-11, P-12, P-19.1, P-19.2, P-19.3, MS-10.3 |
| P-19.5 | ci | Close the row: `wasm.json`'s committed ratios lowered to at most 1.5x on every operation and its `+simd128` gains raised to at least 1.3x on the raster and decoder operations, MS-06's figures held for every profile, and the row taken out of the roadmap. *Note:* 'The size budget' is H-04's; until the owner sets it, MS-06's committed figures are the budget this holds, as H-04's default says. Only H-78 raises a committed ratio or lowers a gain. | `crates/tinker-pdf/benches/wasm.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-10 run is green with every operation's wasm/native ratio at or under 1.5x and every raster and decoder gain at or above 1.3x, recorded here; MS-06's local footprint check is green for every profile, `simd128` included. | P-19.4, MS-06, MS-10.3 |

## P-20 — Release build profile

**Release build profile**: fat LTO measured; profile-guided optimisation if the toolchain step is accepted

**Exit criterion.** adopted only if **≥ 5 % on the suite's geometric mean** in the paired gate

**Doc:** — · **Depends on:** H-45

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-20.1b | measure | The wall-clock figure P-20.1a's exit names, taken in the timing phase on the named machine with nothing else running (ruling 15): by MS-04.10's fifteen runs, or by the paired run where P-20.1a's exit names one, and committed as ruling 16 says. *Note:* Split from P-20.1 by the owner's timing choice; P-20.1a lands the operation unmeasured. | `crates/tinker-pdf/benches/baseline.json` | The figure P-20.1a's exit names is committed and `cargo xtask baseline` holds it. | P-20.1a, MS-04.10 |
| P-20.3 | measure | Fat LTO measured and adopted by the row's rule: a local MS-02 paired run with `lto = "fat"` as head against today's profile as base over MS-04's suite, its geometric mean (P-20.1), build times and native and wasm sizes committed; `[profile.release]` and the bindings' copies take `lto = "fat"` only if the mean is at least 5 % faster. *Note:* The rule is the row's own, so this step applies it and decides nothing else; build time is recorded for the owner, not weighed here. A larger wasm under fat LTO is H-04's to accept. | `Cargo.toml`, `bindings/js/Cargo.toml`, `bindings/python/Cargo.toml`, `docs/verification.md` | `docs/verification.md` records the paired geometric mean, the build times and the sizes; the committed profile is the one the rule picks; with fat LTO adopted, a local MS-02 paired run, MS-06's local footprint check and `determinism.rs` are green, recorded here. | P-20.1a, P-20.2, MS-02, MS-04, MS-06.3 |
| P-20.6 | ci | Close the row: the profile the rule picked for fat LTO and, where H-45 accepted it, for profile-guided optimisation stands in `Cargo.toml`, the bindings and `release.yml`, with the measurement behind each recorded; take the row out of the roadmap. *Note:* Should the owner decline H-45, P-20.4 and P-20.5 are not built and this step rests on P-20.3 and H-45's recorded answer. | `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run is green with the committed profile, recorded here, and `docs/verification.md` holds the measurements each choice rests on. | P-20.3 |

## P-21 — Hold today's figures with bands

**Hold today's figures with bands** rather than effort: opening a small file, raw extraction of a Flate image with predictors, a no-op incremental save, a rewrite into object streams, linearization, and loading the committed TrueType faces

**Exit criterion.** each a bench row with a band in `baseline.json`; **open a small file ≤ 5 ms CLI; extract the predictor image ≤ 280 ms; no-op incremental save ≤ 0.02 ms**; object streams, linearization and face loading held at a factor of 1

**Doc:** — · **Depends on:** MS-04

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-21.2 | measure | Bands for the three: fifteen local `cargo bench` runs on the named machine set each one's fastest figure, swing and band in the named machine's entry, and fifteen local A/A paired runs set each one's paired band, by the rules MS-04 and MS-02 measured the rest of the set by; committed in `baseline.json`. *Note:* H-109 approves the bands; a band wider than the rule gives is H-78's. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md` | `baseline.json` carries a figure, swing and band and a paired band for each of the six P-21 operations, the three MS-04 measured included; `every_committed_band_fires_at_its_own_edge_and_not_before` pushes on each, and each run that set them passes them. | P-21.1, H-109 |
| P-21.3 | ci | Close the row in MS-02's paired run: the six operations held at a floor of 1 against the planning baseline (opening a small file through the CLI, the predictor image's extraction, the no-op incremental save, the 100-page rewrite into object streams and linearized, and each Liberation face parsed with every outline); then take the row out of the roadmap. *Note:* Held is a floor of 1, which only H-78 lowers. The bold figures (5 ms, 280 ms, 0.02 ms) are MS-03's absolute check on the pinned machine (H-110), never this run's. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run is green with each of the six gated 'held' at a floor of 1, recorded here. | P-21.2, MS-02 |

## P-22 — Colour conversion throughput

**Colour conversion throughput**: ICC transforms cached per document, keyed by source and destination profile, intent and black-point compensation, inside P-16's byte budget; each transform precomputed into an integer lookup table with fixed-point tetrahedral interpolation; an 8-bit CMYK-to-RGB fast path through a 17- or 33-point grid; Gray and RGB identity paths; an image converted once at its decoded size rather than per drawn pixel

**Exit criterion.** counted guard: transforms built ≤ distinct (profile pair, intent, compensation) keys per document; fingerprints unchanged on four targets, or the one-time change reviewed (H-76); **an ICC-based CMYK image converted to sRGB at ≥ 100 Mpixel/s**

**Doc:** [design/icc.md](../design/icc.md), P-22 · **Depends on:** MS-01, MS-04

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-22.6 | build | Images converted by a converter chosen once per image: the XObject and inline sample loops stop calling `ColorSpace::to_rgb` per pixel over a fresh `Vec`, `samples_in_space` (a JPEG's or JPEG 2000 image's samples in its dictionary's space) per pixel, and `jpeg_rgb` (a JPEG strip in its own colour) per CMYK pixel, and choose by space, depth and `/Decode`: a copy for 8-bit DeviceRGB, a table of at most 2^bpc colours built through `to_rgb` for a one-component space (DeviceGray among them), a table for 8-bit DeviceCMYK, P-22.5's row path for an ICC space, and `to_rgb` over one reused buffer with `Remembered` for the rest. *Note:* The Gray and RGB identity paths are the first two arms; tables built through `to_rgb` make a device pixel the same byte by construction. P-08's 8-bit Gray/RGB identity path and 1-bit to 8-bit table change these loops first (`resources.rs:3319`, `:3659`), so this step builds on the loop as P-08 leaves it, and an arm P-08 already converts is held by the guard rather than built twice. | `crates/tinker-pdf-color/src/lib.rs`, `crates/tinker-pdf/src/resources.rs`, `crates/tinker-pdf/tests/image_conversion.rs` (new) | `image_conversion.rs` (new) `a_converter_is_chosen_once_per_image`: `to_rgb` calls per image (MS-01's tally) do not grow with its pixels (none for 8-bit DeviceRGB and ICC images, one fixed table for 8-bit DeviceCMYK, at most 2^bpc for one component); allocations per image O(1) (A10); each device arm's bytes equal per-pixel `to_rgb` over all its inputs; `images.rs`, `inline_images.rs`, `colour_spaces.rs`, `render_goldens.rs` and the `image`, `jbig2`, `jpx` and `icc` fingerprints unchanged. | P-22.5, P-08, MS-01.8 |
| P-22.7 | build | ICC transforms cached per document: the document cache P-16 builds keys a compiled transform by its source profile's bytes (a digest confirmed byte for byte, not the stream's object number), the sRGB destination and the intent and compensation it was compiled with, so a profile named on many pages, forms and renders, or embedded as many objects, compiles once; each transform's grid bytes are charged to P-16's budget, and past it a transform compiles per use as today. *Note:* Today's `IccCache` is per `PageResources` and keyed by object number (`resources.rs:109`, `:680-706`); a refused profile's `None` is kept under the same key, so it is refused once per document. The guard is the row's: transforms built at most the distinct (profile pair, intent, compensation) keys per document. | `crates/tinker-pdf/src/resources.rs`, `crates/tinker-pdf/tests/icc_cache.rs` (new), `crates/tinker-pdf/tests/determinism.rs`, `docs/features/rendering.md` | `icc_cache.rs` (new) `transforms_built_are_at_most_the_distinct_keys`: a generated 20-page document naming one RGB and one CMYK profile through a different object on every page, in a form and in an appearance stream, rendered twice, builds two transforms (MS-01's tally); under a budget below one grid every page is byte-identical; `determinism.rs`: the `icc` page rendered twice through one document hashes to its fingerprint both times. | P-22.4, P-16, MS-01.8 |
| P-22.8 | ci | Close the row in MS-02's paired run: MS-04's quarter-size ICC-based CMYK image converted to sRGB (620×877) held at its calibrated target, at least 100 Mpixel/s, which is at most 5.4/37.6 of the run's calibration operation, with the row's counted guards green on every target; then take the row out of the roadmap. *Note:* The operation converts a non-DCT image, so it times the conversion and no JPEG decode. Its profile, `fuzz/corpus/icc_profile/cmyk-lut.icc`, is a seed built to be refused (`NeedsLut`), so until this step gives the operation a four-input `mft2` profile it times 8.6.5.5's component-count fallback and no transform (`docs/design/icc.md`, P-22, milestone 8). The bold figure is MS-03's second check. | `crates/tinker-pdf/benches/support/inputs.rs`, `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the calibrated ICC operation; `icc_cache.rs`, `image_conversion.rs` and `icc.rs`'s evaluation count run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally; the `icc` fingerprint holds on the four targets. | P-22.6, P-22.7, MS-01.8, MS-02, MS-04 |

## P-23 — A tiling-pattern cell cache

**A tiling-pattern cell cache**: a cell kept between fills and pages in P-16's byte-budgeted cache, keyed by pattern object, device matrix and sub-pixel phase, `PaintType 2` cells kept as coverage and recoloured at the blit; the per-fill rasterisation the fallback past the budget

**Exit criterion.** counted guard: cell rasterisations ≤ distinct (pattern, matrix, phase) keys over a document; fingerprints unchanged on four targets; relation: **a page filled 50 times by one pattern ≤ 1.2 × the same page filled once**

**Doc:** — · **Depends on:** P-16, MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-23.2 | build | Cells kept between fills and pages in the document cache P-16 builds: the facade's `tile` looks a cell up by its pattern object, the exact bits of `TileRequest::to_pixels` (the device matrix and the sub-pixel phase, unquantised, so a hit is the bytes a miss would draw), the buffer's size and format, anti-aliasing and nesting depth; a hit reports again what the cell's rasterisation reported, a cancelled cell is never kept, and past P-16's budget each fill rasterises as today. *Note:* A hit re-reports what the cell's rasterisation added: its `Tile::warnings` and the missing fonts, damaged images and repaired spaces merged from the cell's own resources (`resources.rs:2015-2037`), as `CachedImage` does for an image. A render option that changes what a cell draws (RC-13's usage, RC-14's configuration) joins the key when it lands. `features/rendering.md`'s pattern paragraph says the cell is kept. | `crates/tinker-pdf/src/resources.rs`, `crates/tinker-pdf/tests/pattern_cache.rs` (new), `crates/tinker-pdf/tests/determinism.rs`, `docs/features/rendering.md` | `pattern_cache.rs` (new) `cell_rasterisations_are_at_most_the_distinct_keys`: a generated three-page document filling a `PaintType 1` pattern and a `PaintType 2` one (in two colours) ten times a page rasterises two cells (MS-01's tally); another matrix or phase adds its own; under a budget below one cell every fill rasterises and the pages are byte-identical; `determinism.rs`: the `tiling` page rendered twice through one document hashes to its fingerprint both times on the four targets. | P-23.1, P-16, MS-01.8 |
| P-23.3 | ci | Close the row in MS-02's paired run: MS-04's page tiled by one pattern 50 times held at or below 1.2 × the same page tiled once, a relation of one build in one run, with the cell guard green on every target; then take the row out of the roadmap. *Note:* A relation and not a factor. It holds only if MS-04's 50 fills together cover what its one fill covers, since compositing runs per fill over the fill's own coverage. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the 50-fill page within 1.2 × the one-fill page; `pattern_cache.rs`'s guard runs green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally; the `tiling` and `pattern` fingerprints hold on the four targets. | P-23.2, MS-01.8, MS-02, MS-04 |

## P-24 — Search over prebuilt text

**Search over prebuilt text**: a `TextPage` keeps its case-folded and diacritic-stripped forms once built; a linear-time substring matcher over them; whole-word boundaries read from UAX #29 breaks computed once

**Exit criterion.** counted guard: folding work per page ≤ 1 across any number of searches; every search fixture's hits unchanged; **≤ 6 ms plain, ≤ 12 ms whole-word and diacritic-insensitive**

**Doc:** [design/performance.md](../design/performance.md), P-24 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-24.5 | ci | Close the row in MS-02's paired run: MS-04's two prebuilt-text search operations over 100 pages, plain and whole-word diacritic-insensitive, with floors raised to 6× and 6.41× against the planning baseline; then take the row out of the roadmap. *Note:* MS-04 builds the pages once outside the timed search, so with kept forms the head times the matcher while the baseline folds on every search, which is the speed-up the row owes. The bold figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the two floors; `search_work.rs`'s guards run green in `cargo test` on Linux and Windows, in both builds (CI runs them on every pull request), and under `--target wasm32-wasip1` locally. | P-24.4, MS-01.8, MS-02, MS-04 |

## P-25 — Transparency groups and soft masks

**Transparency groups and soft masks**: group backdrops allocated to the group's bounds clipped to the region, not the page; a Normal-blend opaque fast path inside groups; luminosity soft masks computed in integer at the mask's bounds

**Exit criterion.** counted guard: group-buffer pixels ≤ 1.1 × the groups' clipped bounds; fingerprints unchanged on four targets; **the transparency fixture ≤ 3 ms at 150 dpi**

**Doc:** [design/performance.md](../design/performance.md), P-25 · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-25.5 | ci | Close the row in MS-02's paired run: the committed transparency fixture (`fuzz/corpus/render_page/transparency.pdf`) rendered at 150 dpi, its floor raised to 2.8× against the planning baseline; then take the row out of the roadmap. *Note:* The fixture's mask group fills its box with an axial shading (object 7's `/Sh0 sh`), whose per-pixel evaluation P-14 rewrites, so the floor is raised after P-14 lands. The bold figure is MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green at 2.8×; `group_buffers.rs`'s guards run green in `cargo test` on Linux and Windows (CI runs it on every pull request) and under `--target wasm32-wasip1` locally; the `transparency`, `blend` and `analytic_blend` fingerprints hold on the four targets. | P-25.2, P-25.3, P-25.4, P-14, MS-01.8, MS-02, MS-04 |

## P-26 — Font programs and shaping

**Font programs and shaping**: CFF charstrings interpreted into a reused path buffer with subroutine biases resolved once per face; shaping lookups indexed by coverage once per face and feature set

**Exit criterion.** shaping fixtures unchanged; **every committed CFF program's outlines at ≥ 1 M a second; a 1 000-character paragraph shaped in ≤ 0.2 ms (Latin), ≤ 0.5 ms (Arabic), ≤ 0.6 ms (Devanagari)**

**Doc:** — · **Depends on:** MS-04

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| P-26.5 | ci | Close the row in MS-02's paired run: MS-04's fonts group, every committed CFF program parsed with every outline at 1 M outlines a second or more and a 1 000-character paragraph shaped in Latin, Arabic and Devanagari within 0.2, 0.5 and 0.6 ms, held at those calibrated targets through the run's calibration operation; then take the row out of the roadmap. *Note:* No committed face covers Devanagari, so that operation waits on the face MS-04 commits for it, vendored only under a licence H-116's default admits. The bold figures are MS-03's second check. | `crates/tinker-pdf/benches/baseline.json`, `docs/verification.md`, `docs/roadmap/P.md`, `docs/ROADMAP.md` | A local MS-02 paired run, recorded here, is green with the four calibrated targets, each held as its fraction of the calibration operation's 751 ms; P-26's counted guards run green in `cargo test` on Linux and Windows, in both builds of `tinker-pdf-font` (CI runs them on every pull request), and under `--target wasm32-wasip1` locally; the shaping fingerprints hold on the four targets. | P-26.1, P-26.4, MS-01.8, MS-02, MS-04 |
