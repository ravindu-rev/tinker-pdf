# JPEG XR fixtures

Every `.jxr` here is **this repository's own raster**, encoded by the JPEG XR
encoder that ships with Windows. That sentence is the whole design, and it is
what makes the fixtures evidence rather than a comparison.

ITU-T T.832's conformance bitstreams are not freely licensed, so they are not
here. Ruling 13 ([`docs/rulings.md`](../../../../docs/rulings.md)) forbids
asking another decoder what a picture should look like. What is left is an
identity:

1. `RASTERS` in [`../jxr_fixtures.rs`](../jxr_fixtures.rs) authors a raster
   from integer arithmetic in this repository.
2. `make-fixtures.ps1` hands those bytes to `WmpBitmapEncoder` with
   `Lossless = true` and commits what comes back.
3. The decoder must return the raster from step 1, **bit for bit**.

The pixels going in are ours, so step 3 compares against a value this
repository chose — not against another program's opinion. Windows is
*supplying* bytes, which ruling 13 admits explicitly ("a third-party program
may host this code, execute it, fetch bytes for it, or generate inputs for
it"); it never says whether the output is right.

## Regenerating

Two commands, in this order, from the repository root:

```powershell
cargo test -p tinker-pdf-filters --test jxr_fixtures -- --ignored write_source_rasters
pwsh -File crates/tinker-pdf-filters/tests/jxr/make-fixtures.ps1
```

The first writes the `.raw` sources and `manifest.txt`; the second writes the
`.jxr` files. The `.raw` files are build intermediates and are **not
committed**: the raster's one definition is `raster()` in the Rust file, so
there is nothing for a committed copy to drift from. `manifest.txt` is
committed so a reviewer can see the set without running anything, and a test
pins it to the `RASTERS` table so it cannot drift either.

The fuzz seed corpus is written from these same files by a third command,
which is why the seeds and the fixtures cannot disagree:

```powershell
cargo test -p tinker-pdf-filters --test jxr_fixtures -- --ignored write_fuzz_seeds
```

## What produced them

| Producer | What it is |
| --- | --- |
| `System.Windows.Media.Imaging.WmpBitmapEncoder` | WPF's `PresentationCore` wrapper over the Windows Imaging Component JPEG XR codec (`WMPhoto`). Driven from `make-fixtures.ps1`; needs no elevation and no printer. |

Machine: Windows 11 Pro, build **10.0.26200.0**, x64. Produced **30 August
2026**. The same provenance discipline as
[`crates/tinker-pdf/tests/xps/README.md`](../../../tinker-pdf/tests/xps/README.md),
and for the same reason: a fixture whose origin is not written down is a
fixture nobody can re-derive.

## Whether they may be committed

Yes, and there is in-tree precedent rather than a judgement call.
`fuzz/README.md` records that the JPX seed corpus holds codestreams
`opj_compress` made from **our own** 32 × 32 images, under the reading that a
tool's output on our input is ours to commit, while ISO/IEC 15444-4's
conformance codestreams stay out. These are the same thing one format over:
the content of every file here — a ramp, a gradient, a checkerboard, a
rectangle and a diagonal — is authored in `jxr_fixtures.rs`, in this
repository. Nothing here is a conformance bitstream and nothing here came from
a third party's image.

## What the set covers

`manifest.txt` is the list. Grouped by what each group is *for*:

| Group | Files | What it is evidence of |
| --- | --- | --- |
| Pixel formats | `gray8`, `gray16`, `rgb24`, `bgr24`, `bgr32`, `bgra32`, `rgb48`, `rgba64` | Eight of Table A.6's rows, each held to the bit-identical round trip. 48 × 32 is three macroblocks across and two down, so none of them can pass while getting macroblock order wrong. |
| Overlap modes | `overlap0`, `overlap1`, `overlap2` | 8.3.10's three values of `OVERLAP_MODE`, over 4 × 4 checkerboard content that puts energy in the HP band. |
| Tiling | `tiled`, `tiled_gray`, `frequency_tiled` | 96 × 64 is six by four macroblocks in a 2 × 2 tile grid — the smallest shape that can see a tile-ordering defect at all. |
| Frequency mode | `frequency`, `frequency_tiled` | 8.3.7's frequency-ordered codestream layout. |
| Seams | `seam0`, `seam1`, `seam2` | A horizontal ramp, **lossy**, at each overlap mode. Lossy on purpose: a lossless ramp reconstructs exactly whatever the overlap filter does, so the seam property would only be measuring the identity again. |
| Monotonicity | `quant48`, `quant16`, `quant4` | One source at three quantization parameters, for the weakest of the three checks. |

**Two encoder findings are recorded here because they cost a fixture set
each, and both are the same failure — a knob that is silently ignored looks
exactly like a knob that worked.**

- `ImageQualityLevel` has **no effect** once `UseCodecOptions` is set. Three
  fixtures asked for 30 %, 60 % and 90 % of it and came back byte-identical.
  The quantization fixtures use `QualityLevel` — the codec's own QP, where 1
  is lossless and larger is coarser.
- `HorizontalTileSlices` and `VerticalTileSlices` count **slices, not extra
  slices**: 1 means one slice and leaves `TILING_FLAG` clear. The first
  fixture set asked for 1 and produced single-tile files whose tile tests all
  passed vacuously. `src/jxr/tests/fixtures.rs` now asserts the tile count,
  the overlap mode and the frequency flag actually reached the codestream, so
  a knob that stops working fails a test instead of quietly emptying a claim.

## What the set does *not* cover

Recorded here as well as in
[`docs/features/filters.md`](../../../../docs/features/filters.md), because a
fixture directory is where somebody looks when they want to know what was
tested:

- **The interleaved alpha image plane** (8.3.18). WIC writes alpha as a
  *separate* plane at `ALPHA_OFFSET` (A.3.2), which is what `bgra32` and
  `rgba64` carry; the interleaved form is refused by name and has no fixture.
- **Subsampled internal colour formats** (YUV420, YUV422) and YUVK. The
  script asks for `SubsamplingLevel = 3` so that every fixture is 4:4:4.
- **CMYK, CMYKDIRECT, NCOMPONENT and RGBE** output formats, and every
  fixed-point, half-float and float row of Table A.6.
- **Packed output depths** — BD1, BD5, BD565, BD10.
- **A windowed origin**: a non-zero `TOP_MARGIN` or `LEFT_MARGIN`.
- **`HARD_TILING_FLAG`**, which changes whether overlap filtering crosses a
  tile boundary. WIC does not expose it, so both settings cannot be produced
  here and only the soft-tile path has a fixture.
