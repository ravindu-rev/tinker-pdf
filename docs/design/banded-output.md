# Banded output

*For [ROADMAP](../ROADMAP.md) row RC-16.* A page rendered whole holds its
bitmap: the 300-dpi A4 page is 2 480 × 3 509 pixels, 26 106 960 bytes in
`Rgb8`, and a render grows with the page's area until `MAX_PAGE_PIXELS`
scales it down. This document fixes how a page renders **in bands into a
streaming writer**: each band of rows drawn as a region of the page and handed
to a sink the moment it is drawn, the sink writing its file as the bands
arrive, so what a render adds is one band and the writer's state. It settles
the sink a writer implements, the band height, how each band is drawn, what
the PNG writer and the Netpbm writer keep between bands so that no band
boundary shows in a file, the pixel cap for banded output, and the memory
account MS-05 holds. A file written in bands is the file the whole page
writes, byte for byte.

It builds on [parallelism](parallelism.md)'s bands of one page (P-15.6 to
P-15.10a), which cut a page into regions and stitch them into one bitmap for
speed; the cuts, the region renders and the warning merge are reused here, and
the bitmap is not. The PNG writer is [performance](performance.md)'s P-04
writer, and the 1-bit outputs are
[output-pixel-models](output-pixel-models.md)'s (RC-06).

## Scope

- **The sink** a writer implements, and the doors that drive it (RC-16.3).
- **The band height**, and who chooses it.
- **How each band is drawn**: a region of the page's display list, or the
  direct way for a page that is not retained.
- **The PNG writer in bands** (RC-16.2): the previous row, the deflater's
  state and the IDAT framing it carries from band to band, so the banded file
  and the whole one share one framing.
- **Netpbm in bands** (RC-16.4): the header first, and the halftone of PBM and
  PKM carrying its state from band to band.
- **The `MAX_PAGE_PIXELS` clamp** for banded output (RC-16.5).
- **The memory account** MS-05 holds (RC-16.6a), with each format's writer
  state computed in bytes.

## Non-goals

- **P-15's threaded bands.** Those draw a page's bands at once on the pool and
  stitch them into one bitmap, for speed; these draw one band after another
  and keep none, for memory. A sink's page is drawn serially whatever
  `RenderOptions::parallelism` says: drawing `n` bands at once would hold `n`.
- **Decoding an image a band at a time.** An image is decoded once, at the
  size it is drawn (P-10's levels), and kept for the page (*Risks*).
- **Ruling 5's named exception.** The cuts keep every run of image draws in
  one band, as P-15's planner does; deciding runs in the page frame is
  RC-21's.
- **The CLI.** `tpdf render` keeps writing whole pages; nothing here changes
  its output.
- **Other file formats**, TIFF and the rest: each is its own FO row, and a
  writer that takes rows can implement the sink when it lands.
- **PNG palettes and interlace**, as P-04 leaves them.

## Design

### The sink

A sink takes a page's pixels a band at a time, top to bottom, each row of the
page in exactly one band:

```rust
/// Where a page rendered in bands goes: told the page's size, then handed
/// each band as soon as it is drawn, top to bottom, then told it has had the
/// last.
pub trait BandSink {
    fn begin(&mut self, page: &BandedPage) -> std::io::Result<()>;
    fn band(&mut self, band: &Band<'_>) -> std::io::Result<()>;
    fn end(&mut self) -> std::io::Result<()>;
}

/// The page a sink is about to be handed, in the layout its bands come in.
pub struct BandedPage {
    pub width: u32,
    pub height: u32,
    /// The format a render with the same options hands back
    /// (`handed_back`), premultiplied where the options ask.
    pub format: PixelFormat,
    pub premultiplied: bool,
}

/// One band: rows `top .. top + rows` of the page, `stride` bytes apart.
pub struct Band<'a> {
    pub top: u32,
    pub rows: u32,
    pub stride: usize,
    pub data: &'a [u8],
}
```

`Page::render_into(&RenderOptions, rows, &mut dyn BandSink)` and
`DisplayList::render_into` drive it, returning the render's warnings and
stats or the first error a sink returned; the render stops at that error. The
facade's `PngSink<W: std::io::Write>` is a sink over RC-16.2's writer, writing
the file into `W` as the bands arrive; RC-16.4 adds a Netpbm sink of each
kind FO-01 writes. The PNG sink converts a band as `Bitmap::to_png` converts a
page — premultiplied colour straightened, a blue-first order reordered red
first, ink and Lab converted to RGBA — per pixel and so per band, into a copy
of the band's rows where it converts, and hands the four layouts PNG has
through as they are; the Netpbm sink converts as FO-01's writer converts a
bitmap. A sink keeps nothing of a band once its call returns: the band's rows
are the render's, gone when the next band is drawn.

The bands are the view's — the region `RenderOptions::region` asks for, or the
page — so a sink can be handed a region in bands as well as a page, and every
other option applies as it does to `Page::render`.

### The band height, and who chooses it

The caller, in rows: `rows` is the height of every band but the last, which
takes what is left. Zero reads as `DEFAULT_BAND_ROWS`, 256 — at 300 dpi an
A4 page's 3 509 rows in fourteen bands, each 1 904 640 bytes in `Rgb8` — and a
height past the view's is one band, the whole view. A band is at least one row,
so a page's width bounds a band from below (*Risks*).

**A band may be taller than asked.** The cuts are
[parallelism](parallelism.md)'s planner fed this sink's rows: the nominal cut
after each band is at its top plus `rows`, and a cut inside the rows of a run
of two or more image draws moves to the nearest row no such run covers,
between its neighbours, or is dropped. So a band holding such a run grows by
the run's rows, and a page one run covers draws as one band, which is ruling
5's named exception kept out of the file at the price of memory; RC-21 lifts
it. The sink is handed the bands the planner cut, of whatever heights they
are.

### How each band is drawn

**A region of the page's display list.** The page is recorded once, as
`DisplayList::render` records it (`Page::render_into` records it for the call;
`DisplayList::render_into` uses its own), and each band replays the list as a
region render (`RenderOptions::region`) through `Page::draw_layer`: the page's
one frame, the canvas standing at the band's corner (`Canvas::origin`), so
ruling 5 makes each band byte-equal to its rows of the page drawn whole. The
bands share the list's caches — images, font programs, outlines, scopes — so
what one band decodes is decoded once for the page; each band's renderer has
its own glyph cache and fill scratch, dropped with the band. The transfer
plane (RC-08.2) is the band's own, as a tile's is the page's under it.

**The direct way** for a page that is not retained: a list whose recording
passed `MAX_DISPLAY_LIST_BYTES` keeps no calls, and each band then interprets
the page as a region render does — the same pixels, the interpretation paid
once a band. A render naming another usage than the list's draws the direct
way likewise, as `DisplayList::render` does.

**Warnings** are the whole page's: each band stamps what it meets with the
ordinal of the device call it met it at, and the bands' lists merge by least
ordinal into the serial order, as
[parallelism](parallelism.md)'s "Warnings equal the serial render's" merges a
banded page's. **Stats** count into one tally. **Cancellation**: every band is
handed over, so a sink always receives the whole page; after the token fires
each band is drawn as a cancelled render draws — stopped at its first check —
and the render reports `Cancelled`. A sink's error stops the render at once,
and the sink is not told `end`.

### The PNG writer in bands (RC-16.2)

`tinker-pdf-filters` takes rows, not a raster (ruling 8): `PngWriter::new`
appends the signature and IHDR to the caller's output; `PngWriter::rows`
takes the next rows, any number of them, `stride` bytes apart, and appends
every chunk that closes; `PngWriter::finish` appends the last IDAT and IEND.
The caller drains its output after each call, so the file never accumulates
in the writer. `png_encode` becomes this writer fed every row at once.

**One framing, the whole page's.** Each row is costed under the five filters
and filtered once against the row above it, and the deflater is handed the
filter type byte, then the filtered row in pieces of at most `FEED`, 64 KiB —
P-04.5's writes. An IDAT chunk closes after the write that brings its data to
`IDAT_SIZE`, 64 KiB, or more, and at the end: P-04.5's framing, kept. The
writer makes the same writes in the same order whatever band a row arrives
in, and the deflater's output is a function of its input alone — arrival is
not an input (P-04) — so its blocks complete at the same writes and its chunks
close at the same writes: a file written in bands of any height is the
whole-page file byte for byte, and no PNG's bytes move.

**The open chunk is the writer's.** A chunk's length precedes its data, so the
chunk being filled is held by the writer and appended to the output whole when
it closes. Before a write its data is under 64 KiB, and one write appends at
most 137 516 bytes: a block written in whichever form is fewest bits is never
longer than its fixed-code form — three bits of header, a literal 8 or 9 bits,
a match at most 31 (an 8-bit length code with 5 extra bits, a 5-bit distance
code with 13), a 7-bit end — and one write completes at most the block in
hand, at most 16 383 symbols from before it at 31 bits, and the symbols of its
own at most 65 536 bytes and the 262 the deflater held back, at most 9 bits a
byte (a match costs fewer bits a byte it covers than a literal), in at most five
blocks: `⌈(16 383 × 31 + 65 798 × 9 + 5 × 10) / 8⌉ = 137 514`, and the
stream's two-byte header on its first write.

**What it keeps between bands**, `w` bytes a row (7 440 for the probe's
2 480-pixel RGB page):

| Part | Bytes | The probe's |
| --- | --- | --- |
| The three rows: the one above, the one read, the one filtered | `3w` | 22 320 |
| The deflater's tables, `head` and the ring: 32 768 positions each as `u32` | 262 144 | 262 144 |
| Its window buffer, two 32 KiB windows | 65 536 | 65 536 |
| Its block's symbols, 16 384 as `u32` | 65 536 | 65 536 |
| The open chunk: its length and type, its data under 65 536 before a write, and one write's 137 516 | at most 203 059 | 203 059 |
| The running Adler-32 and the open chunk's CRC-32 | 8 | 8 |
| **In all**, the structs' own fields aside | `3w + 596 283` | **618 603** |

Nothing is sized from the image's height, so A8's `png_image_scratch` stays at
none for the whole-page writer as for the banded one; the open chunk joins the
peak `PngHeld` reports.

### Netpbm in bands (RC-16.4)

A Netpbm file is its header — the magic, the width and height, and the maxval
or PAM's depth, maxval and tuple type — and then its samples, row by row, so
the sink writes the header at `begin`, from `BandedPage`, and each band's rows
as it arrives. Whichever module FO-01 puts its writer in takes rows the same
way; the whole-page file is that writer fed every row at once, so no byte of it
changes.

**The halftone carries its state across bands.** PBM is RC-06's 1-bit page:
each band rendered as its `Rgba8` render, opaque, its grey the `Gray8`
extraction's, through the grey transfer function, then screened:

- **Ordered dither carries nothing.** The threshold at device pixel `(x, y)` is
  the cell's at `((x − ox) mod 16, (y − oy) mod 16)` in the page frame, so a
  band starting at row `top` reads the page's thresholds there and its phase is
  its rows' own.
- **Error diffusion carries its row.** Floyd–Steinberg runs in raster order
  from the page's first row (RC-06.1), so the band below continues the band
  above: the error carried into the next row — sixteenths of a level, `i32`, a
  pixel each — and the sixteenths dropped at the edges are `Diffusion`'s state,
  kept from band to band. The bands come top to bottom and every row is
  diffused once, which is the order and the work the whole page's diffusion
  takes.

PKM, FO-01's 1-bit CMYK, screens each of its four planes as PBM screens its
one, each plane carrying what PBM's carries.

**What each kind keeps between bands**, at the probe's 2 480 pixels:

| Kind | Between bands | Bytes |
| --- | --- | --- |
| PGM, PPM, PAM of a grey or light band | nothing: the band's rows are the samples | 0 |
| PAM of an ink band | nothing between bands; one row converted to CMYK while it is written | `4 × width`: 9 920 |
| PBM by ordered dither | the cell, 256 thresholds as `u16`; one grey row and one packed row while a row is screened | `512 + width + ⌈width / 8⌉`: 3 302 |
| PBM by error diffusion | the carried row and the dropped count; one grey row and one packed row while a row is screened | `4 × width + 8 + width + ⌈width / 8⌉`: 12 718 |
| PKM | four planes, each as PBM's | four times PBM's carried part: 39 712 by error diffusion |

### Past the pixel cap (RC-16.5)

`MAX_PAGE_PIXELS` exists because a whole render allocates the page, and an
allocation that fails aborts the process. A banded render allocates a band, so
its doors render at the asked scale whatever the page's area: `Page::scales`
takes the clamp it applies — the page's area for `Page::render`, a region and
`Page::pixel_size`, which keep today's answer, and for the banded doors each
side held to 2 147 483 647 pixels, PNG's limit (11.2.2), which Netpbm shares
here so the two sinks draw one page. A page whose side passes it at the asked
scale is scaled down to fit, keeping its aspect, and reports
`PageScaledDown`, as the area clamp does; a page under the area cap is drawn
exactly as `Page::render` draws it. Each band is a region of the unclamped
page through the one pipeline, its canvas at the band's corner: the row index
fits the canvas's `i32` origin, since a side is under `2^31`.

### The memory account (RC-16.6a)

A peak is a whole process's ([verification](../verification.md), "Peak memory,
a process a case"), so the account is a difference of the probe's cases on one
input. Each banded case is read **over its open-only baseline**, a child that
opens the same input into the same buffer and takes the page, drawing nothing:
the file and the document's objects are in both, so the difference is what the
render adds. A banded case is held to its committed band, set at its measured
figure by ruling 17's rule, and that band must sit **at or under one band plus
the writer's state**:

- **One band** is what drawing one band holds, measured and not invented: the
  probe's case drawing the page's first band alone, as a region through the same
  path — the list recorded, the band replayed — and dropping it, over the same
  baseline. It holds the band's canvas (`width × rows ×` the canvas's bytes a
  pixel: 1 904 640 for 256 rows of the probe's RGB page), its conversion where
  the sink converts, the display list the bands replay, and the fonts, glyph
  masks and images the band reads.
- **The writer's state** is the tables above: 618 603 bytes for the probe's
  PNG.

A banded page holding more than one band's drawing and its writer — the whole
page drawn and then cut, a band kept after its call returned — is a finding
against RC-16.3, never a wider band. The probe's pages draw text and
rectangles and no image, and the dense page repeats its lines' words, so every
band reads the fonts and glyph masks the first does and nothing accumulates
from band to band; what a page of images adds is a risk below, named rather
than folded into the bound.

## Milestones

1. **RC-16.2, the PNG writer in bands.** `PngWriter` in `tinker-pdf-filters`,
   `png_encode` the writer fed once. Exit: `png/encode/tests.rs`'s
   `a_png_written_in_bands_is_the_png_written_whole` — rasters of the four
   colour types, padded and tight, written in bands of 1 row, of every prime
   height up to 61 and of all rows at once, equal `png_encode`'s bytes;
   `tests/png_suite.rs` and the encoder's existing tests pass unchanged.
2. **RC-16.3, the sink and the PNG sink.** `BandSink`, `Page::render_into`
   and `DisplayList::render_into`, `PngSink`. Exit: `banded_output.rs`'s
   `a_page_rendered_in_bands_to_png_is_the_whole_page_png` — every
   `region_support` fixture, at 300 dpi and at `render_regions.rs`'s seven
   scales, in each page format with transparent and premultiplied output, at
   band heights 1, 37, 256 and the page's, equals
   `page.render(&options).to_png()` byte for byte, the sink receiving each
   band once and in order; `determinism.rs` pins one fixture's banded PNG hash
   in K-07's roster, held on Linux, Windows and `wasm32-wasip1`.
3. **RC-16.4, Netpbm in bands.** Exit: `banded_output.rs`'s
   `a_page_rendered_in_bands_to_netpbm_is_the_whole_page_file` — every Netpbm
   kind FO-01 writes, PBM and PKM by each screening, of each region fixture at
   300 dpi in band heights 1, 37 and 256, equals the whole-page file byte for
   byte.
4. **RC-16.5, past the pixel cap.** Exit: `banded_output.rs`'s
   `a_page_past_the_pixel_cap_renders_in_bands_at_the_asked_scale` — a page of
   pixel-aligned rectangles at a scale whose area passes `MAX_PAGE_PIXELS`
   writes a PNG header of the asked size and no `PageScaledDown`, every band's
   rows the values the test computes from the rectangles, through a sink that
   checks each band and keeps none; a page under the cap unchanged.
5. **RC-16.6a, the numbers.** Exit: `peak_memory.rs` holding each banded case
   — the 300-dpi page in 256-row bands to PNG and to Netpbm, and the page past
   the cap in bands to PNG — at its band on Linux, each beside its open-only
   baseline and its one-band case, the figure and the bound it sits under
   recorded in `docs/verification.md`; MS-04's render group gains the 300-dpi
   text page in 256-row bands to PNG, held by `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` and its first paired ratio
   against the planning baseline's whole-page render and `to_png` recorded.
   **RC-16.6b** takes the wall-clock figure in the timing phase.

Where SU-12's coverage check has landed, the new API is projected or named in
`docs/features/bindings.md`'s refusal table in the step that adds it.

## Risks

| Risk | Mitigation |
| --- | --- |
| A banded render that draws the whole page and then cuts it passes every byte test | MS-05's account holds the banded case under one band's drawing plus the writer's state (RC-16.6a); a page drawn whole holds the page |
| A band boundary shows in a file | One writer for both, its writes and its framing the same whatever band a row arrives in; RC-16.2's test at every prime band height to 61, RC-16.3's and RC-16.4's at 1, 37 and 256 rows |
| **Decoded images are held for the whole page**: an image is decoded once, at the size it is drawn, and kept in the list's caches for every band, so a page of large images holds them all by its last band, and a full-page scan at 300 dpi is a whole page of pixels whatever the band height | Named, not folded into the bound: the account's pages draw no image; P-10's levels never decode an image larger than it is drawn; a caller with a page of large images renders it no larger than a whole render would hold |
| **A page past the cap costs time in proportion to its area**, and each band replays the whole list | It renders only on the banded doors, at the scale the caller asked for, so the time is the caller's to spend; the cancel token stops the drawing between bands and inside one, and every band is still handed over |
| A run of image draws across a cut parts a banded page from the whole one (ruling 5's named exception) | The cuts keep every run of two or more image draws in one band, as P-15's planner does; a band may be taller than asked, and a page one run covers draws as one band |
| A wide page's one row is large: a band is at least one row, and a page 2 147 483 647 pixels wide is gigabytes a row | A band's bytes are `width × rows ×` bytes a pixel, stated here; the caller chooses the rows with the width in mind, and a side past PNG's limit is scaled down |
| Error diffusion in bands drifts from the whole page | The carried row and the dropped count are the whole page's diffusion state at that row; RC-16.4's test at band heights 1, 37 and 256 |
| The writer's open chunk grows without bound | One write's output is bounded by the deflater's constants above; nothing the writer holds is sized from the image's height |
