# Capability tier — formats out and print production

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every raster and vector output a host asks for, the print
streams a printer takes, the container and office formats a reader takes
away, and the print-production work — flattening, separations, trapping,
preflight — that the standards tier's conversions need.

## FO-01 — Netpbm output

**Netpbm output**: PBM (halftoned 1-bit), PGM, PPM, PAM with alpha and CMYK, and PKM (1-bit CMYK)

**Today.** PNG only (`Bitmap::to_png`)

**Exit criterion.** each file's samples equal the bitmap's; headers held to the format description

**Doc:** — · **Depends on:** RC-06

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-01.1 | build | Netpbm output from a bitmap: PBM halftoned to 1 bit, PGM, PPM, PAM with alpha and CMYK, and PKM's 1-bit CMYK, beside `Bitmap::to_png`, and `tpdf render`'s formats. *Note:* The writer joins FI-05.1's reader in `netpbm.rs`, and the test re-reads each file through that reader. | `crates/tinker-pdf-filters/src/netpbm.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/raster_outputs.rs` (new) | `raster_outputs.rs` (new): each file's samples equal the bitmap's and its header holds to the format description. | RC-06.5, RC-06.6, FI-05.1 |

**Closes the row:** FO-01.1 writes every Netpbm form with samples equal to the bitmap's and headers per the description, which is the row's exit.

## FO-02 — JPEG raster output

**JPEG raster output** and image-to-buffer helpers (data URIs)

**Today.** absent — the owner's parity decision of 9 October 2026; `Bitmap` offers `to_png` only (`crates/tinker-pdf/src/lib.rs:831`); the baseline JPEG encoder exists and only image recompression calls it (`crates/tinker-pdf/src/recode.rs:1277`)

**Exit criterion.** outputs decode through the in-tree decoder within the bound computed from the caller's tables

**Doc:** — · **Depends on:** ED-28

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-02.1 | build | JPEG raster output through the baseline encoder with ED-28's tables or presets, and helpers that give an image as bytes or a data URI. | `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/raster_outputs.rs` (new) | Outputs decode through the in-tree decoder within the bound computed from the caller's tables; a data URI decodes to the same bytes. | ED-28 |

**Closes the row:** FO-02.1's outputs decode within the bound computed from the caller's tables, which is the row's exit.

## FO-03 — TIFF output

**TIFF output**: multi-page; none, PackBits, LZW, Deflate, JPEG, G3 1D and 2D, G4; CMYK and one plane per ink; BigTIFF; resolution and ICC tags; fax class F

**Today.** a TIFF reader and a G4 encoder exist

**Exit criterion.** every file re-read by the in-tree TIFF reader yields the samples written; a class F file carries the tags and values TIFF Class F's description requires

**Doc:** — · **Depends on:** RC-04

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-03.1 | build | TIFF output: multi-page, uncompressed, PackBits, LZW, Deflate and JPEG, CMYK and one plane per ink from RC-04's separations, BigTIFF, and resolution and ICC tags. | `crates/tinker-pdf-filters/src/tiff_write.rs` (new), `crates/tinker-pdf-filters/src/lib.rs`, `crates/tinker-pdf/tests/raster_outputs.rs` (new) | Every file re-read by the in-tree TIFF reader yields the samples written. | RC-04.16 |
| FO-03.2 | build | Fax output: G3 1D and 2D encoders beside the G4 one, and TIFF Class F's tags. | `crates/tinker-pdf-filters/src/ccitt.rs`, `crates/tinker-pdf-filters/src/tiff_write.rs` (new) | G3 and G4 files re-read to their samples; a Class F file carries the tags and values Class F's description requires. | FO-03.1 |

**Closes the row:** FO-03.1 and FO-03.2 write every form, each re-read to its samples, and a conforming Class F file, which is the row's exit.

## FO-04 — PSD output

**PSD output** with spot channels

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpsd\b' crates tools` finds nothing

**Exit criterion.** channels equal the separations rendered (RC-04)

**Doc:** — · **Depends on:** RC-04

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-04.1 | build | PSD output with spot channels: the composite and one channel per separation RC-04 renders, named in the image resources. | `crates/tinker-pdf-filters/src/psd.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/raster_outputs.rs` (new) | Read back through FI-05's PSD reader, each channel equals the separation RC-04 renders. | RC-04.16, FI-05 |
| FO-04.2 | build | `tpdf render --format psd`. | `tools/tpdf/src/main.rs` | A CLI test writes a PSD whose channels match the facade's. | FO-04.1 |

**Closes the row:** FO-04.1 writes channels equal to the rendered separations, which is the row's exit.

## FO-05 — PNG options

**PNG options**: 16-bit, an embedded ICC profile, physical resolution, the fast level (P-04)

**Today.** 8-bit PNG without colour or resolution chunks

**Exit criterion.** each chunk re-read; 16-bit samples equal the RC-07 render

**Doc:** — · **Depends on:** P-04, RC-07

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-05.1 | build | PNG options: 16-bit samples from RC-07's render, an embedded ICC profile, physical resolution and P-04's fast level. | `crates/tinker-pdf-filters/src/png.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/raster_outputs.rs` (new) | Each chunk re-reads as written; 16-bit samples equal the RC-07 render. | P-04, RC-07.14 |

**Closes the row:** FO-05.1 re-reads each chunk and matches the 16-bit render, which is the row's exit.

## FO-06 — JPEG 2000 raster output

**JPEG 2000 raster output**

**Today.** absent — the owner's parity decision of 9 October 2026; no JPEG 2000 encoder exists (ED-26)

**Exit criterion.** outputs decode within the bound (lossless exactly)

**Doc:** — · **Depends on:** ED-26

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-06.1 | build | JPEG 2000 raster output through ED-26's encoder, lossless and lossy. | `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/raster_outputs.rs` (new) | Outputs decode within the bound computed in the test, lossless exactly. | ED-26.15 |

**Closes the row:** FO-06.1's outputs decode within the bound, lossless exactly, which is the row's exit.

## FO-07a — A WebP encoder

**A WebP encoder**, lossless and lossy

**Today.** absent — the owner's parity decision of 9 October 2026; WebP is read only (`features/filters.md`)

**Exit criterion.** lossless outputs decode through the in-tree decoder to the samples written; lossy within a bound computed in the test

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-07a.1 | build | WebP lossless (VP8L) encoding: the transforms, LZ77 with the colour cache, and prefix coding. | `crates/tinker-pdf-filters/src/webp_encode.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: lossless outputs decode through the in-tree decoder to the samples written. | — |
| FO-07a.2 | build | WebP lossy (VP8) encoding: intra prediction, transforms, quantisation, the boolean entropy coder and loop-filter settings. | `crates/tinker-pdf-filters/src/webp_encode.rs` (new) | Unit tests: lossy outputs decode within the bound computed in the test. | FO-07a.1 |
| FO-07a.3 | build | The RIFF container with alpha, and WebP raster output from the facade and CLI. | `crates/tinker-pdf-filters/src/webp_encode.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs` | A bitmap with alpha writes and decodes back to its samples (lossless) or within the bound (lossy). | FO-07a.2 |

**Closes the row:** FO-07a.1 and FO-07a.2 decode lossless outputs exactly and lossy ones within the bound, which is the row's exit.

## FO-07b — An AVIF encoder

**An AVIF encoder** (AV1 intra)

**Today.** absent — the owner's parity decision of 9 October 2026; AVIF is not even read yet (FI-08)

**Exit criterion.** outputs decode through FI-08's decoder within a bound computed in the test, lossless exactly

**Doc:** `design/avif.md` (owed) · **Depends on:** FI-08

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-07b.1 | design | Add the AV1 intra encoder to `docs/design/avif.md`: its headers, symbol encoding, mode and partition choices, transforms and quantisation, and lossless mode. | `docs/design/avif.md` | The section names FO-07b.2 to FO-07b.5's exit tests and is reviewed. | FI-08.1 |
| FO-07b.2 | build | Sequence and frame headers, OBUs and the symbol encoder with adapting CDFs. | `crates/tinker-pdf-filters/src/avif/encode.rs` (new) | Unit tests: written headers and symbols decode through FI-08's decoder. | FO-07b.1, FI-08 |
| FO-07b.3 | build | Partition and intra-mode choice, forward transforms and quantisation. | `crates/tinker-pdf-filters/src/avif/encode.rs` (new) | Unit tests: outputs decode through FI-08's decoder within the bound computed in the test. | FO-07b.2 |
| FO-07b.4 | build | AV1's lossless mode. | `crates/tinker-pdf-filters/src/avif/encode.rs` (new) | Unit tests: lossless outputs decode to the samples written exactly. | FO-07b.3 |
| FO-07b.5 | build | The AVIF container written, and AVIF raster output from the facade. | `crates/tinker-pdf-filters/src/avif/mod.rs` (new), `crates/tinker-pdf/src/lib.rs` | A bitmap writes as AVIF and decodes back within the bound, lossless exactly. | FO-07b.4 |

**Closes the row:** FO-07b.3 and FO-07b.4 decode through FI-08 within the bound and losslessly exactly, which is the row's exit.

## FO-07c — A JPEG XL encoder

**A JPEG XL encoder**, modular (lossless) and VarDCT

**Today.** absent — the owner's parity decision of 9 October 2026; JPEG XL is not read yet (FI-07a)

**Exit criterion.** lossless outputs decode through FI-07a's decoder to the samples written; VarDCT within a bound computed in the test

**Doc:** `design/jpeg-xl.md` (owed) · **Depends on:** FI-07b

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-07c.1 | design | Add the JPEG XL encoder to `docs/design/jpeg-xl.md`: headers, entropy encoding, modular tree choice, VarDCT's choices and the bound each output is held to. | `docs/design/jpeg-xl.md` | The section names FO-07c.2 to FO-07c.5's exit tests and is reviewed. | FI-07a.1 |
| FO-07c.2 | build | The container, headers and the entropy encoder. | `crates/tinker-pdf-filters/src/jxl/encode.rs` (new) | Unit tests: written headers and entropy-coded data decode through FI-07a's decoder. | FO-07c.1, FI-07b |
| FO-07c.3 | build | Modular (lossless) encoding with a chosen tree, predictors, RCT and palette. | `crates/tinker-pdf-filters/src/jxl/encode.rs` (new) | Unit tests: lossless outputs decode through FI-07a's decoder to the samples written. | FO-07c.2 |
| FO-07c.4 | build | VarDCT encoding: XYB, transform choice, quantisation and low-frequency coding. | `crates/tinker-pdf-filters/src/jxl/encode.rs` (new) | Unit tests: VarDCT outputs decode within the bound computed in the test. | FO-07c.2 |
| FO-07c.5 | build | JPEG XL raster output from the facade and CLI. | `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs` | A bitmap writes in both modes and decodes back exactly or within the bound. | FO-07c.3, FO-07c.4 |

**Closes the row:** FO-07c.3 decodes lossless outputs exactly and FO-07c.4 VarDCT within the bound, which is the row's exit.

## FO-08a — Raster PostScript output

**Raster PostScript output**, banded

**Today.** absent — the owner's parity decision of 9 October 2026; a decision row of the roadmap until 9 October 2026, on the argument that print pipelines are the only consumer

**Exit criterion.** the output interpreted by FI-17a's interpreter reproduces the bitmap

**Doc:** — · **Depends on:** FI-17a

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-08a.1 | build | Raster PostScript output, banded: each band an image with its filter, in a minimal DSC structure. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/ps_output.rs` (new) | `ps_output.rs` (new): the output interpreted by the PostScript interpreter reproduces the bitmap. | FI-17a, FI-17b |

**Closes the row:** FO-08a.1's output interpreted by the PostScript interpreter reproduces the bitmap, which is the row's exit.

## FO-08b — Vector PostScript Level 2 and 3 and EPS

**Vector PostScript Level 2 and 3 and EPS**: DSC-conforming; fonts as Type 42, Type 1, CFF and CID with CMaps; smooth shading with a Level 2 fallback; masked images; separations; `setpagedevice` paper and duplex; OPI comments for images

**Today.** as FO-08a

**Exit criterion.** every fingerprint fixture written and interpreted by FI-17b's interpreter renders byte-identically to the PDF; DSC and OPI comments held to the conventions' text

**Doc:** `design/postscript-output.md` (owed) · **Depends on:** FO-10, FI-17b

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-08b.1 | design | Create `docs/design/postscript-output.md`: page content mapped to Level 2 and 3 operators, fonts by kind, masked images and shading with Level 2 fallbacks, separations, page devices, transparency through FO-10, DSC and OPI comments, and EPS. | `docs/design/postscript-output.md` (new) | The doc exists in the design-doc order, names FO-08b.2 to FO-08b.9's exit tests, and is reviewed. | FO-10, FI-17b |
| FO-08b.2 | build | The document's DSC structure, the graphics state, paths and painting. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | Path fixtures written and interpreted by FI-17b's interpreter render byte-identically to the PDF. | FO-08b.1, FO-08a.1 |
| FO-08b.3 | build | Fonts as Type 42, Type 1, CFF and CID with CMaps, and text. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | Text fixtures interpreted render byte-identically to the PDF, in both builds. | FO-08b.2 |
| FO-08b.4 | build | Images, masked images at Level 3 with a Level 2 fallback. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | Image fixtures interpreted render byte-identically at each level. | FO-08b.2 |
| FO-08b.5 | build | Smooth shading at Level 3 with a Level 2 fallback. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | Shading fixtures interpreted render byte-identically at each level. | FO-08b.2 |
| FO-08b.6 | build | Separations, and `setpagedevice` paper and duplex. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | Separation fixtures interpreted render as RC-04's separations; page devices read back as set. | FO-08b.2 |
| FO-08b.7 | build | Transparency flattened through FO-10 before writing, and OPI comments for images. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | Transparent fixtures interpreted render as FO-10's flattened pages; OPI comments hold to the conventions' text. | FO-08b.4 |
| FO-08b.8 | build | EPS output. | `crates/tinker-pdf/src/ps_out.rs` (new), `crates/tinker-pdf/tests/ps_output.rs` (new) | An EPS of a page, placed through FI-17c, draws as the page. | FO-08b.2, FI-17c |
| FO-08b.9 | test | Every fingerprint fixture written and interpreted, DSC held to the conventions, and `tpdf convert --to ps\|eps`. | `crates/tinker-pdf/tests/ps_output.rs` (new), `tools/tpdf/src/main.rs`, `docs/features/writing.md` | Every fingerprint fixture renders byte-identically to the PDF after the round through PostScript; DSC comments hold to the conventions' text. | FO-08b.3, FO-08b.5, FO-08b.6, FO-08b.7, FO-08b.8 |

**Closes the row:** FO-08b.9 renders every fingerprint fixture byte-identically after the round through PostScript with DSC and OPI held to the conventions, which is the row's exit.

## FO-09a — PCL raster output

**PCL raster output**, monochrome and colour, with the printer presets and options a driver needs

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpcl\b' crates tools` finds nothing

**Exit criterion.** the byte stream re-read by FI-18a's interpreter reproduces the bitmap

**Doc:** — · **Depends on:** FI-18a

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-09a.1 | build | PCL raster output, monochrome and colour, with its compression modes and the presets and options a driver needs: paper, duplex, tray, copies and resolution. | `crates/tinker-pdf/src/pcl_out.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/print_streams.rs` (new) | `print_streams.rs` (new): the byte stream re-read by FI-18a's interpreter reproduces the bitmap. | FI-18a |

**Closes the row:** FO-09a.1's stream re-read by FI-18a reproduces the bitmap, which is the row's exit.

## FO-09b — PCLm output

**PCLm output** (compression, strip height)

**Today.** absent — the owner's parity decision of 9 October 2026; PCLm appears only as two corpus files' names in raster comments

**Exit criterion.** the output re-opened as PDF yields the bitmap strip by strip

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-09b.1 | build | PCLm output: the bitmap in strips of a stated height, each compressed as PCLm allows. | `crates/tinker-pdf/src/pclm_out.rs` (new), `crates/tinker-pdf/tests/print_streams.rs` (new) | The output re-opened as PDF yields the bitmap strip by strip. | — |

**Closes the row:** FO-09b.1's output re-opened as PDF yields the bitmap strip by strip, which is the row's exit.

## FO-09c — PWG raster output

**PWG raster output** with every header field settable

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpwg\b' crates tools` finds nothing

**Exit criterion.** headers and pixels re-read by a first-party parser in the test

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-09c.1 | build | PWG raster output with every header field settable, and a first-party parser in the test. | `crates/tinker-pdf/src/pwg_out.rs` (new), `crates/tinker-pdf/tests/print_streams.rs` (new) | Headers and pixels re-read by the parser in the test equal what was set and rendered. | — |

**Closes the row:** FO-09c.1's headers and pixels re-read as written, which is the row's exit.

## FO-09d — URF raster output

**URF raster output**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\burf\b' crates tools` finds nothing

**Exit criterion.** as FO-09c

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-09d.1 | build | URF raster output, and a first-party parser in the test. | `crates/tinker-pdf/src/pwg_out.rs` (new), `crates/tinker-pdf/tests/print_streams.rs` (new) | Headers and pixels re-read by the parser in the test equal what was written. | FO-09c.1 |

**Closes the row:** FO-09d.1's headers and pixels re-read as written, which is the row's exit.

## FO-09e — PCL XL vector output

**PCL XL vector output**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pclxl|pcl.xl' crates tools` finds nothing

**Exit criterion.** re-read by FI-18b's interpreter to the same pixels

**Doc:** — · **Depends on:** FI-18b

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-09e.1 | build | PCL XL vector output: pages, the graphics state, paths and painting. | `crates/tinker-pdf/src/pclxl_out.rs` (new), `crates/tinker-pdf/tests/print_streams.rs` (new) | Path fixtures re-read by FI-18b's interpreter render to the same pixels. | FI-18b |
| FO-09e.2 | build | Images and raster patterns. | `crates/tinker-pdf/src/pclxl_out.rs` (new), `crates/tinker-pdf/tests/print_streams.rs` (new) | Image fixtures re-read render to the same pixels. | FO-09e.1 |
| FO-09e.3 | build | Downloaded fonts and text. | `crates/tinker-pdf/src/pclxl_out.rs` (new), `crates/tinker-pdf/tests/print_streams.rs` (new) | Text fixtures re-read render to the same pixels. | FO-09e.1 |

**Closes the row:** FO-09e.1 to FO-09e.3 re-read through FI-18b to the same pixels, which is the row's exit.

## FO-10 — Transparency flattening

**Transparency flattening**: atomic regions, complex areas rasterised at a set resolution, vector text kept where possible, an outline-text option, overprint-aware; a transparency-free PDF out

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'flatten_transparency|transparency_flatten' crates tools` finds nothing

**Exit criterion.** flattened pages render within a stated tolerance of the original (computed per pixel in the test) and contain no transparency; the PDF/A-1 and PDF/X-1a transparency rules pass

**Doc:** `design/flattening.md` (owed) · **Depends on:** RC-03

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-10.1 | design | Create `docs/design/flattening.md`: which objects take part in transparency, path booleans and atomic regions, regions computed as vectors and areas rasterised at a set resolution, text kept or outlined, overprint (RC-03), the stated tolerance, and the PDF/A-1 and PDF/X-1a rules the output passes. | `docs/design/flattening.md` (new) | The doc exists in the design-doc order, names FO-10.2 to FO-10.11's exit tests and the tolerance, and is reviewed. | RC-03.4, RC-03.8 |
| FO-10.2 | build | Transparency analysis: each object classed by its soft mask, blend mode, opacity and group. | `crates/tinker-pdf/src/flatten/mod.rs` (new), `crates/tinker-pdf/src/flatten/analyse.rs` (new), `crates/tinker-pdf/src/lib.rs` | Unit tests: fixtures' objects are classed as the test expects. | FO-10.1 |
| FO-10.3 | build | Polygon booleans on flattened paths: intersection, difference and union with exact winding. | `crates/tinker-pdf-raster/src/boolean.rs` (new), `crates/tinker-pdf-raster/src/lib.rs` | Unit tests: result areas equal those computed in the test for every case, degenerate ones included. | FO-10.1 |
| FO-10.4 | build | Curves through the booleans within a stated flattening tolerance, robust against near-coincident edges. | `crates/tinker-pdf-raster/src/boolean.rs` (new) | Unit tests: curved operands give areas within the tolerance; a property test finds no failure. | FO-10.3 |
| FO-10.5 | build | Atomic regions: the planar map of overlapping transparent objects. | `crates/tinker-pdf/src/flatten/regions.rs` (new) | Unit tests: fixtures' regions equal the expected ones. | FO-10.2, FO-10.4 |
| FO-10.6 | build | Simple regions flattened to opaque vector fills with their blended colour computed. | `crates/tinker-pdf/src/flatten/regions.rs` (new), `crates/tinker-pdf/tests/flatten.rs` (new) | `flatten.rs` (new): constant-colour fixtures render within the tolerance of the original, computed per pixel. | FO-10.5 |
| FO-10.7 | build | Complex areas (gradients, images and masks under transparency) rasterised at the set resolution and placed under clips. | `crates/tinker-pdf/src/flatten/raster.rs` (new), `crates/tinker-pdf/tests/flatten.rs` (new) | Complex fixtures render within the tolerance per pixel. | FO-10.6 |
| FO-10.8 | build | Vector text kept where nothing transparent covers it, and the outline-text option through ED-05. | `crates/tinker-pdf/src/flatten/regions.rs` (new), `crates/tinker-pdf/tests/flatten.rs` (new) | Kept text still extracts; outlined text renders within the tolerance; both builds. | FO-10.6, ED-05 |
| FO-10.9 | build | Overprint-aware flattening. | `crates/tinker-pdf/src/flatten/regions.rs` (new), `crates/tinker-pdf/tests/flatten.rs` (new) | Overprinting fixtures render within the tolerance under RC-03's simulation. | FO-10.6 |
| FO-10.10 | build | A transparency-free PDF out: no soft masks, blend modes, partial opacity or groups. | `crates/tinker-pdf/src/flatten/mod.rs` (new), `crates/tinker-pdf/tests/flatten.rs` (new) | Outputs contain no transparency and pass the PDF/A-1 and PDF/X-1a transparency rules. | FO-10.7, FO-10.8, FO-10.9 |
| FO-10.11 | build | `tpdf flatten`, a speed row and a fingerprint of one flattened page. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/writing.md` | The command flattens `testdata/` fixtures to the facade's output; the speed row is in the benchmark set; the fingerprint holds on every K-07 target. | FO-10.10, MS-04.8, K-07 |

**Closes the row:** FO-10.6 to FO-10.9 render flattened pages within the stated tolerance per pixel and FO-10.10 leaves no transparency, passing the PDF/A-1 and PDF/X-1a rules, which is the row's exit.

## FO-11 — A CBZ writer

**A CBZ writer** (pages rasterised into a comic archive) and the ZIP writer under it

**Today.** container writing was a decision item of the roadmap until 9 October 2026 — CBZ, XPS and EPUB were read-only conversions; `tinker-pdf-zip` reads only

**Exit criterion.** the written archive re-opens through the CBZ reader to the same pixels

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-11.1 | build | A ZIP writer: stored and deflated entries, the central directory, ZIP64 when needed and UTF-8 names. | `crates/tinker-pdf-zip/src/write.rs` (new), `crates/tinker-pdf-zip/src/lib.rs` | Unit tests: written archives re-read through the ZIP reader byte for byte, ZIP64 ones included. | — |
| FO-11.2 | build | A CBZ writer: pages rasterised into a comic archive. | `crates/tinker-pdf/src/cbz_out.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs` | The written archive re-opens through the CBZ reader to the same pixels. | FO-11.1 |

**Closes the row:** FO-11.2's archive re-opens through the CBZ reader to the same pixels, on FO-11.1's ZIP writer, which is the row's exit.

## FO-12a — An XPS writer

**An XPS writer**

**Today.** absent — the owner's parity decision of 9 October 2026; the same decision item as FO-11

**Exit criterion.** written packages re-open through the XPS reader and render byte-identically to the source

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-12a.1 | build | An XPS package: content types, relationships and fixed documents and pages, with paths and images. | `crates/tinker-pdf/src/xps_out.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/xps_output.rs` (new) | `xps_output.rs` (new): path and image fixtures re-open through the XPS reader and render byte-identically to the source. | FO-11 |
| FO-12a.2 | build | Glyphs with embedded, obfuscated fonts. | `crates/tinker-pdf/src/xps_out.rs` (new), `crates/tinker-pdf/tests/xps_output.rs` (new) | Text fixtures re-open, render byte-identically and extract their text, in both builds. | FO-12a.1 |
| FO-12a.3 | build | Gradients, opacity and clips, and what XPS cannot express flattened through FO-10. | `crates/tinker-pdf/src/xps_out.rs` (new), `crates/tinker-pdf/tests/xps_output.rs` (new) | Every fingerprint fixture re-opens and renders byte-identically to its source. | FO-12a.2, FO-10 |
| FO-12a.4 | build | `tpdf convert --to xps`. | `tools/tpdf/src/main.rs`, `docs/features/xps.md` | A CLI test writes a package that re-opens to the facade's render. | FO-12a.3 |

**Closes the row:** FO-12a.1 to FO-12a.3 re-open written packages through the XPS reader rendering byte-identically, which is the row's exit.

## FO-12b — An EPUB writer

**An EPUB writer** from structured or tagged content

**Today.** absent — the owner's parity decision of 9 October 2026; the same decision item as FO-11 (tagged EPUB is read and written into PDF, never out of it)

**Exit criterion.** a tagged committed document written as EPUB re-opens through the EPUB reader with its text, order and images

**Doc:** `design/epub-writer.md` (owed) · **Depends on:** FT-30

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-12b.1 | design | Create `docs/design/epub-writer.md`: the EPUB 3 package, content from the structure tree or FT-30's inferred order, styles to CSS, images and fonts carried, and first-party re-opening as the check. | `docs/design/epub-writer.md` (new) | The doc exists in the design-doc order, names FO-12b.2 to FO-12b.6's exit tests, and is reviewed. | FT-30 |
| FO-12b.2 | build | The OCF container, the package document and the navigation document from the outline. | `crates/tinker-pdf/src/epub_out.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/epub_output.rs` (new) | `epub_output.rs` (new): written packages re-open through the EPUB reader with their navigation. | FO-12b.1, FO-11 |
| FO-12b.3 | build | XHTML content from the structure tree, text styles as CSS. | `crates/tinker-pdf/src/epub_out.rs` (new), `crates/tinker-pdf/tests/epub_output.rs` (new) | A tagged committed document written as EPUB re-opens with its text in its order. | FO-12b.2 |
| FO-12b.4 | build | Images and fonts carried, fonts obfuscated as OCF states. | `crates/tinker-pdf/src/epub_out.rs` (new), `crates/tinker-pdf/tests/epub_output.rs` (new) | The re-opened book draws its images and fonts. | FO-12b.3 |
| FO-12b.5 | build | Untagged documents written in FT-30's inferred order. | `crates/tinker-pdf/src/epub_out.rs` (new), `crates/tinker-pdf/tests/epub_output.rs` (new) | An untagged document re-opens with its text in the inferred order. | FO-12b.3 |
| FO-12b.6 | build | `tpdf convert --to epub` and a fingerprint of one written book. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/epub.md` | The command writes `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target; in the default and `bundled-fonts` builds. | FO-12b.4, FO-12b.5, K-07 |

**Closes the row:** FO-12b.3 and FO-12b.4 re-open a tagged committed document written as EPUB with its text, order and images, which is the row's exit.

## FO-12c — tar and 7z writers

**tar and 7z writers** for archive output

**Today.** a non-goal of the archive design until 9 October 2026 (`design/comic-archives.md:49-50`)

**Exit criterion.** archives re-open through the readers byte for byte

**Doc:** — · **Depends on:** FO-11

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-12c.1 | build | tar (ustar and pax) and 7z writers. | `crates/tinker-pdf-archive/src/tar.rs`, `crates/tinker-pdf-archive/src/sevenz.rs`, `crates/tinker-pdf-archive/src/write.rs` (new) | Unit tests: written archives re-open through the readers byte for byte. | FO-11 |

**Closes the row:** FO-12c.1's archives re-open through the readers byte for byte, which is the row's exit.

## FO-13 — SVG output, complete

**SVG output, complete**: soft masks, non-Normal blend modes and knockout groups expressed rather than refused; shadings and patterns as SVG gradients and patterns where exact; text as text or as paths; images reused

**Today.** `SvgWarning::SoftMaskRefused`, `BlendModeRefused`, `KnockoutRefused`, `Rasterised` (`features/rendering.md:737-738`)

**Exit criterion.** the SVG re-opened through the in-tree SVG reader renders within the stated tolerance of the page for every fingerprint fixture

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-13.1 | build | Soft masks written as SVG masks, luminosity and alpha, retiring `SvgWarning::SoftMaskRefused`. | `crates/tinker-pdf/src/svg_out.rs`, `crates/tinker-pdf/tests/svg_output.rs` | `svg_output.rs`: masked fixtures re-opened through the in-tree SVG reader render within the stated tolerance of the page; failing first on today's refusal. | — |
| FO-13.2 | build | Non-Normal blend modes and knockout groups expressed, retiring `BlendModeRefused` and `KnockoutRefused`. | `crates/tinker-pdf/src/svg_out.rs`, `crates/tinker-pdf/tests/svg_output.rs` | Blend and knockout fixtures re-opened render within the tolerance. | FO-13.1 |
| FO-13.3 | build | Shadings and patterns as SVG gradients and patterns where exact, rasterised with `Rasterised` only where not. | `crates/tinker-pdf/src/svg_out.rs`, `crates/tinker-pdf/tests/svg_output.rs` | Exact cases write gradients and patterns and render within the tolerance; inexact ones are named. | FO-13.1 |
| FO-13.4 | build | Text as text or as paths by option, and repeated images reused. | `crates/tinker-pdf/src/svg_out.rs`, `crates/tinker-pdf/tests/svg_output.rs`, `docs/features/rendering.md` | Text fixtures render within the tolerance either way and extract as text when kept; a repeated image is written once. | FO-13.1 |
| FO-13.5 | test | Every fingerprint fixture written as SVG and re-opened. | `crates/tinker-pdf/tests/svg_output.rs` | Every fingerprint fixture's SVG re-opened through the in-tree SVG reader renders within the stated tolerance of the page. | FO-13.2, FO-13.3, FO-13.4 |

**Closes the row:** FO-13.5 renders every fingerprint fixture's SVG within the stated tolerance through the in-tree reader, which is the row's exit.

## FO-14a — DOCX export

**DOCX export**: paragraphs and styles, tables, headers and footers, images, links

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-02b; nothing writes an office format

**Exit criterion.** exported documents re-read by FI-02a yield the source's text, structure and images

**Doc:** `design/office-export.md` (owed) · **Depends on:** FT-30, FT-31, FI-02a

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-14a.1 | design | Create `docs/design/office-export.md` for FO-14a to FO-14d: an intermediate model from structured text, tags, FT-30's order and FT-31's tables, written as WordprocessingML, ODF, SpreadsheetML and PresentationML, checked by re-reading through FI-02a. | `docs/design/office-export.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones FO-14b to FO-14d extend, and is reviewed. | FT-30, FT-31, FI-02a |
| FO-14a.2 | build | A DOCX package with paragraphs and styles from the text's geometry and fonts. | `crates/tinker-pdf/src/office_out/mod.rs` (new), `crates/tinker-pdf/src/office_out/docx.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/office_export.rs` (new) | `office_export.rs` (new): exported documents re-read by FI-02a yield the source's text and paragraph structure. | FO-14a.1 |
| FO-14a.3 | build | Tables from FT-31's reconstruction. | `crates/tinker-pdf/src/office_out/docx.rs` (new), `crates/tinker-pdf/tests/office_export.rs` (new) | Re-read tables equal the reconstructed grids. | FO-14a.2 |
| FO-14a.4 | build | Headers and footers from running heads, images and links. | `crates/tinker-pdf/src/office_out/docx.rs` (new), `crates/tinker-pdf/tests/office_export.rs` (new) | Re-read documents yield the source's headers, footers, images and links. | FO-14a.2 |
| FO-14a.5 | build | `tpdf convert --to docx` and a fingerprint of one export. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/office.md` (new) | The command exports `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | FO-14a.3, FO-14a.4, K-07 |

**Closes the row:** FO-14a.2 to FO-14a.4 export documents that FI-02a re-reads to the source's text, structure and images, which is the row's exit.

## FO-14b — ODT export

**ODT export**

**Today.** absent — the owner's parity decision of 9 October 2026; ODF appears only as the EPUB container's namespace (`crates/tinker-pdf/src/epub/ocf.rs:105`)

**Exit criterion.** as FO-14a

**Doc:** `design/office-export.md` (owed) · **Depends on:** FO-14a

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-14b.1 | design | Add ODT to `docs/design/office-export.md`. | `docs/design/office-export.md` | The section names FO-14b.2 and FO-14b.3's exit tests and is reviewed. | FO-14a.1 |
| FO-14b.2 | build | An ODT package from the same intermediate model. | `crates/tinker-pdf/src/office_out/odt.rs` (new), `crates/tinker-pdf/tests/office_export.rs` (new) | Exported documents re-read by FI-02a yield the source's text, structure and images. | FO-14b.1, FO-14a |
| FO-14b.3 | build | `tpdf convert --to odt`. | `tools/tpdf/src/main.rs` | A CLI test exports a fixture that re-reads as the facade's export. | FO-14b.2 |

**Closes the row:** FO-14b.2's exports re-read through FI-02a to the source, as FO-14a's exit states.

## FO-14c — XLSX export

**XLSX export** with typed cells and tables stitched across pages

**Today.** absent — the owner's parity decision of 9 October 2026; nothing writes an office format (FO-14a); a spreadsheet's media type appears only in a test's associated file (`crates/tinker-pdf/tests/pdf20.rs:345`)

**Exit criterion.** exported tables re-read to the reconstructed grid

**Doc:** `design/office-export.md` (owed) · **Depends on:** FT-32

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-14c.1 | design | Add XLSX to `docs/design/office-export.md`: typed cells and tables stitched across pages. | `docs/design/office-export.md` | The section names FO-14c.2 and FO-14c.3's exit tests and is reviewed. | FO-14a.1 |
| FO-14c.2 | build | An XLSX package: a sheet per table, FT-32's tables stitched across pages, cells typed as numbers, dates or text. | `crates/tinker-pdf/src/office_out/xlsx.rs` (new), `crates/tinker-pdf/tests/office_export.rs` (new) | Exported tables re-read to the reconstructed grid, each cell of its type. | FO-14c.1, FT-32.4, FT-32.5, FT-32.6, FO-14a.2 |
| FO-14c.3 | build | `tpdf convert --to xlsx`. | `tools/tpdf/src/main.rs` | A CLI test exports a fixture's tables that re-read as the facade's export. | FO-14c.2 |

**Closes the row:** FO-14c.2's tables re-read to the reconstructed grid, which is the row's exit.

## FO-14d — PPTX export

**PPTX export**, a text box per block per slide

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pptx|presentationml' crates tools` finds nothing

**Exit criterion.** re-read to the same blocks

**Doc:** `design/office-export.md` (owed) · **Depends on:** FO-14a

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-14d.1 | design | Add PPTX to `docs/design/office-export.md`: a slide per page, a text box per block. | `docs/design/office-export.md` | The section names FO-14d.2 and FO-14d.3's exit tests and is reviewed. | FO-14a.1 |
| FO-14d.2 | build | A PPTX package: a slide per page, a text box per block at its place, and images. | `crates/tinker-pdf/src/office_out/pptx.rs` (new), `crates/tinker-pdf/tests/office_export.rs` (new) | Exports re-read through FI-02a to the same blocks. | FO-14d.1, FO-14a |
| FO-14d.3 | build | `tpdf convert --to pptx`. | `tools/tpdf/src/main.rs` | A CLI test exports a fixture that re-reads as the facade's export. | FO-14d.2 |

**Closes the row:** FO-14d.2's exports re-read to the same blocks, which is the row's exit.

## FO-14e — RTF export

**RTF export**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\brtf\b' crates tools` finds nothing

**Exit criterion.** re-read by FI-02g

**Doc:** — · **Depends on:** FI-02g

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-14e.1 | build | RTF export from the intermediate model: paragraphs, styles, tables and images, with `tpdf convert --to rtf`. | `crates/tinker-pdf/src/office_out/rtf.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/office_export.rs` (new) | Exports re-read by FI-02g yield the source's text and structure. | FI-02g, FO-14a |

**Closes the row:** FO-14e.1's exports re-read through FI-02g, which is the row's exit.

## FO-15a — High-fidelity HTML

**High-fidelity HTML**: absolutely positioned selectable text in embedded web fonts (WOFF and WOFF2 written with corrected `cmap`s), vector art as SVG or background images, per page or one file, the outline as navigation

**Today.** structured text as HTML (`TextWriter`)

**Exit criterion.** the HTML re-laid out by the engine's own HTML path renders within the stated tolerance of the page; every glyph's text extracts

**Doc:** `design/html-export.md` (owed) · **Depends on:** FO-13

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-15a.1 | design | Create `docs/design/html-export.md`: positioned selectable text in embedded web fonts with corrected `cmap`s, vector art as SVG or background images, per page or one file, the outline as navigation, and re-layout through the engine's HTML path as the check. | `docs/design/html-export.md` (new) | The doc exists in the design-doc order, names FO-15a.2 to FO-15a.5's exit tests and the tolerance, and is reviewed. | FO-13 |
| FO-15a.2 | build | WOFF and WOFF2 writers, WOFF2's tables transformed and compressed through FO-18, each font's `cmap` corrected so every used glyph maps to its text. | `crates/tinker-pdf-font/src/woff_write.rs` (new), `crates/tinker-pdf-font/src/lib.rs` | Unit tests: written fonts re-read through the in-tree WOFF readers; every used glyph maps to its text. | FO-15a.1, FO-18 |
| FO-15a.3 | build | Page HTML: text runs absolutely positioned in those fonts, with their sizes and transforms. | `crates/tinker-pdf/src/html_out.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/html_output.rs` (new) | `html_output.rs` (new): every glyph's text extracts from the HTML re-laid out by the engine's HTML path. | FO-15a.2 |
| FO-15a.4 | build | Vector art as SVG through FO-13, or as a background image. | `crates/tinker-pdf/src/html_out.rs` (new), `crates/tinker-pdf/tests/html_output.rs` (new) | The HTML re-laid out by the engine's HTML path renders within the stated tolerance of the page. | FO-15a.3 |
| FO-15a.5 | build | Per-page files or one file, the outline as navigation, `tpdf convert --to html`, and a fingerprint. | `crates/tinker-pdf/src/html_out.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/writing.md` | Both layouts write and re-lay out within the tolerance; the navigation equals the outline; the fingerprint holds on every K-07 target; in the default and `bundled-fonts` builds. | FO-15a.4, K-07 |

**Closes the row:** FO-15a.3 extracts every glyph's text and FO-15a.4 renders within the stated tolerance after re-layout, which is the row's exit.

## FO-15b — Accessible HTML from tagged PDF

**Accessible HTML from tagged PDF**: standard structure types mapped to HTML, attributes to CSS, alternate text, MathML, table headers

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'accessible_html|structure_to_html' crates tools` finds nothing

**Exit criterion.** each committed tagged document's HTML re-read yields the structure tree's order, headings, lists and tables

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-15b.1 | build | Accessible HTML from tagged PDF: standard structure types mapped to HTML, attributes to CSS, alternate text, MathML and table headers. *Note:* Its own module beside FO-15a's `html_out.rs`: this writes the structure tree as semantic HTML, that positioned runs in embedded fonts, and the two share only HTML escaping, which whichever lands second takes from the other rather than writing its own. | `crates/tinker-pdf/src/accessible_html.rs` (new), `crates/tinker-pdf/tests/accessible_html.rs` (new), `docs/features/writing.md` | `accessible_html.rs` (new): each committed tagged document's HTML re-read yields the structure tree's order, headings, lists and tables. | — |
| FO-15b.2 | build | `tpdf convert --to html --accessible`. | `tools/tpdf/src/main.rs` | A CLI test writes a tagged fixture's HTML equal to the facade's. | FO-15b.1 |

**Closes the row:** FO-15b.1's HTML re-reads to the structure tree's order, headings, lists and tables, which is the row's exit.

## FO-15c — Hybrid HTML

**Hybrid HTML**: graphics as a background image, text over it

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'hybrid_html' crates tools` finds nothing

**Exit criterion.** text positions equal the glyph boxes

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-15c.1 | build | Hybrid HTML: the page rendered without its text as a background image, the text positioned over it. | `crates/tinker-pdf/src/html_out.rs` (new), `crates/tinker-pdf/tests/html_output.rs` (new) | Text positions equal the glyph boxes. | FO-15a.3 |

**Closes the row:** FO-15c.1 places text at the glyph boxes, which is the row's exit.

## FO-16 — Device trace and diagnostics output

**Device trace and diagnostics output**: every device call with parameters as XML, a bounding-box log, glyph-detail XML

**Today.** `display_list` in Rust only

**Exit criterion.** the trace re-parsed equals the recorded events; `tpdf trace` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-16.1 | build | Device traces as XML: every device call with its parameters, a bounding-box log and glyph detail, from the display list, and `tpdf trace`. | `crates/tinker-pdf/src/display.rs`, `crates/tinker-pdf/src/trace_out.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/device_trace.rs` (new) | `device_trace.rs` (new): the trace re-parsed equals the recorded events; `tpdf trace` runs over `testdata/` fixtures. | — |

**Closes the row:** FO-16.1's trace re-parses to the recorded events and `tpdf trace` runs, which is the row's exit.

## FO-17 — A uniform document-writer API and option grammar

**A uniform document-writer API and option grammar**: one page-by-page writer chosen by format, with typed options and an option-string grammar for the CLI and bindings (unknown options reported)

**Today.** each output has its own call

**Exit criterion.** every output format reachable through one writer; option strings round-trip to typed options; an unknown option is a typed error; `tpdf convert` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-17.1 | build | One page-by-page writer chosen by format, beside each output's own call, with the formats that exist registered. | `crates/tinker-pdf/src/writers.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/writers.rs` (new), `docs/features/writing.md` | `writers.rs` (new): each registered format writes through the one writer to the bytes its own call gives. | — |
| FO-17.2 | build | Typed options per format and an option-string grammar for the CLI and bindings. | `crates/tinker-pdf/src/writers.rs` (new), `crates/tinker-pdf/tests/writers.rs` (new) | Option strings round-trip to typed options; an unknown option is a typed error naming it. | FO-17.1 |
| FO-17.3 | build | `tpdf convert` through the writer. | `tools/tpdf/src/main.rs` | `tpdf convert` writes `testdata/` fixtures in each registered format to the facade's output. | FO-17.2 |
| FO-17.4 | test | Every output format registered once each format row has landed. | `crates/tinker-pdf/tests/writers.rs` (new) | A table test reaches every output format through the one writer. | FO-17.3, FO-01, FO-02, FO-03, FO-04, FO-05, FO-06, FO-07a, FO-07b, FO-07c, FO-08b, FO-09a, FO-09b, FO-09c, FO-09d, FO-09e, FO-11, FO-12a, FO-12b, FO-14a, FO-15a, FO-19, FO-20 |

**Closes the row:** FO-17.1 to FO-17.4 reach every format through one writer, round-trip option strings and type unknown options, and FO-17.3 runs `tpdf convert`, which is the row's exit.

## FO-18 — A Brotli encoder

**A Brotli encoder** for `/BrotliDecode` streams on write

**Today.** absent — the owner's parity decision of 9 October 2026; the Brotli module decodes only (`crates/tinker-pdf-filters/src/brotli.rs` has no `compress` or `encode`)

**Exit criterion.** encoder outputs decode through RFC 7932's decoder path to their input; savings recorded

**Doc:** — · **Depends on:** FI-15

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-18.1 | build | A Brotli encoder (RFC 7932): meta-blocks with literal, insert-and-copy and distance codes, LZ77 matching and quality levels. | `crates/tinker-pdf-filters/src/brotli_encode.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: outputs decode through the in-tree decoder to their input at every quality. | FI-15 |
| FO-18.2 | build | `/BrotliDecode` streams on write as a save option, and the savings recorded. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/brotli_write.rs` (new), `docs/features/writing.md` | `brotli_write.rs` (new): outputs validate and render identically; the savings on the committed fixtures are recorded in the feature doc. | FO-18.1 |

**Closes the row:** FO-18.1 decodes every output to its input and FO-18.2 writes and records the savings, which is the row's exit.

## FO-19 — EMF and EMF+ output

**EMF and EMF+ output** (Windows metafiles)

**Today.** absent — the owner's parity decision of 9 October 2026; no metafile writer exists (`rg -i 'metafile' crates` finds one comment in `bmp.rs`)

**Exit criterion.** records held to the published specifications; every written file re-read through FI-36's reader to the same drawing

**Doc:** — · **Depends on:** FI-36

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-19.1 | build | EMF output from the display list: paths, fills and strokes, text, bitmaps, clips and transforms. | `crates/tinker-pdf/src/metafile/emf_write.rs` (new), `crates/tinker-pdf/tests/emf_output.rs` (new) | `emf_output.rs` (new): records hold to the specification; every written file re-reads through FI-36's reader to the same drawing. | FI-36 |
| FO-19.2 | build | EMF+ records in dual mode: gradients, transparency and hints. | `crates/tinker-pdf/src/metafile/emf_write.rs` (new), `crates/tinker-pdf/tests/emf_output.rs` (new) | Dual-mode files re-read through FI-36 to the same drawing in both modes. | FO-19.1 |
| FO-19.3 | build | `tpdf convert --to emf`. | `tools/tpdf/src/main.rs` | A CLI test writes a fixture whose drawing re-reads as the facade's. | FO-19.2 |

**Closes the row:** FO-19.1 and FO-19.2 write records held to the specifications that re-read to the same drawing, which is the row's exit.

## FO-20 — A JPEG XR encoder

**A JPEG XR encoder**

**Today.** absent — the owner's parity decision of 9 October 2026; a named non-goal of the roadmap until 9 October 2026 — a scope choice under ruling 8 and never a limit, unlike JPEG XR's unadjudicated list (NG-06) ([design/jpeg-xr.md](../design/jpeg-xr.md))

**Exit criterion.** outputs decode through the in-tree decoder; lossless exactly

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-20.1 | build | A JPEG XR encoder: the forward transforms, quantisation, coefficient coding, tiles, the container, and lossless mode. *Note:* The run takes this row after every other row of its tier: H-30's default orders it last. | `crates/tinker-pdf-filters/src/jxr/encode.rs` (new), `crates/tinker-pdf-filters/src/jxr/mod.rs` | Unit tests: outputs decode through the in-tree decoder, lossless exactly. | — |
| FO-20.2 | build | The pixel formats the encoder writes, and JPEG XR raster output. | `crates/tinker-pdf-filters/src/jxr/encode.rs` (new), `crates/tinker-pdf/src/lib.rs` | Each format's outputs decode through the in-tree decoder, lossless exactly. | FO-20.1 |

**Closes the row:** FO-20.1 and FO-20.2 decode every output through the in-tree decoder, lossless exactly, which is the row's exit.

## FO-21 — A print-layout API

**A print-layout API**: scaling (none, fit, shrink, custom), auto-rotate and centre, tiling oversized pages, N-up and booklets, duplex and collation, ranges, annotations and comment summaries included or not, `/ViewerPreferences` print keys honoured, print-as-image fallback, reverse order

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'print_layout|PrintOptions|PrintParams' crates tools` finds nothing

**Exit criterion.** each setting's placement computed in the test; viewer-preference keys read and applied

**Doc:** — · **Depends on:** ED-08

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-21.1 | build | Print layout onto a target paper: scaling (none, fit, shrink, custom), auto-rotate and centre, ranges and reverse order. | `crates/tinker-pdf/src/print_layout.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/print_layout.rs` (new) | `print_layout.rs` (new): each setting's placement equals the test's computation. | ED-11 |
| FO-21.2 | build | Tiling oversized pages, N-up and booklets through ED-08, and duplex and collation. | `crates/tinker-pdf/src/print_layout.rs` (new), `crates/tinker-pdf/tests/print_layout.rs` (new) | Each setting's placements and sheet order equal the test's computation. | FO-21.1, ED-08 |
| FO-21.3 | build | Annotations and comment summaries included or not (AN-07), `/ViewerPreferences`' print keys honoured, and a print-as-image fallback. | `crates/tinker-pdf/src/print_layout.rs` (new), `crates/tinker-pdf/tests/print_layout.rs` (new), `docs/features/rendering.md` | Viewer-preference keys are read and applied; the image fallback places the rendered page as computed. | FO-21.1, AN-07 |

**Closes the row:** FO-21.1 to FO-21.3 place every setting as computed and read and apply the viewer-preference keys, which is the row's exit.

## FO-22 — Print-intent rendering and printer marks

**Print-intent rendering and printer marks**: crop, bleed and registration marks, colour bars, page information, hairline fix-up

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'crop_marks|registration_marks|colou?r_bars' crates tools` finds nothing

**Exit criterion.** marks placed at the geometry computed from the boxes; hairlines at the stated minimum

**Doc:** — · **Depends on:** RC-13

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-22.1 | build | Printer marks: crop, bleed and registration marks, colour bars and page information, placed from the boxes. | `crates/tinker-pdf/src/print_marks.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/print_marks.rs` (new) | `print_marks.rs` (new): marks sit at the geometry computed from the boxes. | — |
| FO-22.2 | build | Print-intent rendering with a hairline fix-up to a stated minimum width. | `crates/tinker-pdf-render/src/lib.rs`, `crates/tinker-pdf/tests/print_marks.rs` (new) | Under print usage, hairlines render at the stated minimum. | RC-13 |

**Closes the row:** FO-22.1 places marks from the boxes and FO-22.2 draws hairlines at the minimum, which is the row's exit.

## FO-23 — Ink coverage and total-area-coverage analysis

**Ink coverage and total-area-coverage analysis**: per page and plate, a heat map and its maximum, spot and overprint inventories

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'ink_coverage|total_area_coverage' crates tools` finds nothing

**Exit criterion.** coverage equals the plates' sums computed in the test

**Doc:** — · **Depends on:** RC-04

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-23.1 | build | Ink coverage per page and plate from RC-04's separations, a total-area-coverage heat map with its maximum, and spot and overprint inventories. | `crates/tinker-pdf/src/ink.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/ink_coverage.rs` (new) | `ink_coverage.rs` (new): coverage equals the plates' sums computed in the test. | RC-04.16 |
| FO-23.2 | build | `tpdf inks`, the report as text and JSON. | `tools/tpdf/src/main.rs` | A CLI test prints the report the facade gives. | FO-23.1 |

**Closes the row:** FO-23.1's coverage equals the plates' sums, which is the row's exit.

## FO-24 — Trapping

**Trapping**: raster trapping on separations and vector `/TrapNet` generation from presets

**Today.** `TrapNet` annotations are read; no trapping

**Exit criterion.** traps placed where the colour-pair rules computed in the test require

**Doc:** `design/trapping.md` (owed) · **Depends on:** RC-04

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-24.1 | design | Create `docs/design/trapping.md`: presets, the colour-pair rules (neutral density, steps, spreads and chokes, black and images), raster trapping on separations and vector `/TrapNet` generation. | `docs/design/trapping.md` (new) | The doc exists in the design-doc order, names FO-24.2 to FO-24.6's exit tests, and is reviewed. | RC-04.16 |
| FO-24.2 | build | The colour-pair rules: whether an edge traps, spread or choke, and the trap colour. | `crates/tinker-pdf/src/trapping/mod.rs` (new), `crates/tinker-pdf/src/lib.rs` | Unit tests: rules for each colour pair equal the test's computation. | FO-24.1 |
| FO-24.3 | build | Raster trapping on RC-04's separations. | `crates/tinker-pdf/src/trapping/raster.rs` (new), `crates/tinker-pdf/tests/trapping.rs` (new) | `trapping.rs` (new): traps are placed where the rules require on raster fixtures. | FO-24.2 |
| FO-24.4 | build | Vector edges from the page's objects (ED-01) and trap paths along them. | `crates/tinker-pdf/src/trapping/vector.rs` (new), `crates/tinker-pdf/tests/trapping.rs` (new) | Trap paths lie where the rules require on vector fixtures. | FO-24.2, ED-01.7, ED-01.17, ED-01.18 |
| FO-24.5 | build | `/TrapNet` annotations generated with their appearances, as 14.11.6 states. | `crates/tinker-pdf/src/trapping/vector.rs` (new), `crates/tinker-pdf/tests/trapping.rs` (new) | Generated trap networks re-read and their appearances place the traps the rules require. | FO-24.4 |
| FO-24.6 | build | Presets and `tpdf trap`. | `crates/tinker-pdf/src/trapping/mod.rs` (new), `tools/tpdf/src/main.rs`, `docs/features/rendering.md` | Each preset traps fixtures as its rules state; the CLI runs over `testdata/` fixtures. | FO-24.3, FO-24.5 |

**Closes the row:** FO-24.3 to FO-24.5 place traps where the colour-pair rules computed in the test require, which is the row's exit.

## FO-25 — Processing steps

**Processing steps** (ISO 19593-1) on optional content groups, read and written

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'processing_?steps|iso.?19593' crates tools` finds nothing

**Exit criterion.** fixtures per step type round-trip

**Doc:** — · **Depends on:** H-102

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-25.1 | build | ISO 19593-1 processing steps on optional content groups, read and written. | `crates/tinker-pdf/src/optional.rs`, `crates/tinker-pdf-cos/src/edit/layers.rs`, `crates/tinker-pdf/tests/processing_steps.rs` (new), `docs/features/document-model.md` | `processing_steps.rs` (new): fixtures per step type round-trip. | H-102 |

**Closes the row:** FO-25.1 round-trips a fixture per step type, which is the row's exit.

## FO-26 — Spectral spot-colour data

**Spectral spot-colour data** (CxF/X-4, ISO 17972-4), read, written and used for spot rendering

**Today.** absent — the owner's parity decision of 9 October 2026; "spectral" appears only as JPEG's spectral selection (`crates/tinker-pdf-filters/src/jpeg.rs`); `rg -i 'cxf' crates tools` finds nothing

**Exit criterion.** spectral-to-Lab computed in the test from the CIE tables

**Doc:** — · **Depends on:** RC-04, H-102

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-26.1 | build | CxF/X-4 spectral spot-colour data read and written. | `crates/tinker-pdf/src/spectral.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/spectral.rs` (new) | `spectral.rs` (new): ISO 17972-4's structures round-trip. | H-102 |
| FO-26.2 | build | Spectral data to Lab from the CIE observer and illuminant tables. | `crates/tinker-pdf-color/src/spectral.rs` (new), `crates/tinker-pdf-color/src/lib.rs` | Unit tests: spectral-to-Lab equals the computation from the CIE tables in the test. | FO-26.1 |
| FO-26.3 | build | Spot colours rendered from their spectral data. | `crates/tinker-pdf-render/src/lib.rs`, `crates/tinker-pdf/tests/spectral.rs` (new) | A spot with spectral data renders as its computed Lab converted to the output. | FO-26.2, RC-04.16 |

**Closes the row:** FO-26.2 computes spectral-to-Lab from the CIE tables and FO-26.1 and FO-26.3 read, write and render with it, which is the row's exit.

## FO-27 — A preflight and fix-up framework

**A preflight and fix-up framework**: profiles of checks (PDF/A, PDF/X, PDF/UA, custom), fix-ups (convert colours, embed fonts, set boxes, remove objects, flatten, downsample), an inventory (fonts, images with effective resolution, colour spaces, overprint, transparency), and a report document with highlights

**Today.** validators report typed findings

**Exit criterion.** each fix-up removes the finding it targets on fixtures built to carry it, with no new finding

**Doc:** `design/preflight.md` (owed) · **Depends on:** SD-02, SD-12, ED-06b

*Steps not yet reviewed: the run that takes each one reviews it against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FO-27.1 | design | Create `docs/design/preflight.md`: profiles of checks built on the validators, the fix-up catalogue and each fix-up's row, the inventory, and the report document. | `docs/design/preflight.md` (new) | The doc exists in the design-doc order, names FO-27.2 to FO-27.7's exit tests, and is reviewed. | SD-02, SD-12 |
| FO-27.2 | build | Profiles and their runner over the PDF/A, PDF/X and PDF/UA validators, with custom checks. | `crates/tinker-pdf/src/preflight/mod.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/preflight.rs` (new) | `preflight.rs` (new): each profile reports the typed findings its fixtures carry. | FO-27.1 |
| FO-27.3 | build | The inventory: fonts, images with effective resolution, colour spaces, overprint and transparency. | `crates/tinker-pdf/src/preflight/inventory.rs` (new), `crates/tinker-pdf/tests/preflight.rs` (new) | The inventory equals each fixture's contents. | FO-27.2 |
| FO-27.4 | build | Fix-ups for colour conversion (ED-06b), font embedding from a host face, boxes and object removal. | `crates/tinker-pdf/src/preflight/fixups.rs` (new), `crates/tinker-pdf/tests/preflight.rs` (new) | Each fix-up removes the finding it targets on a fixture built to carry it, with no new finding. | FO-27.2, ED-06b.4, ED-06b.5, ED-06b.8 |
| FO-27.5 | build | Fix-ups for flattening (FO-10) and downsampling (ED-25). | `crates/tinker-pdf/src/preflight/fixups.rs` (new), `crates/tinker-pdf/tests/preflight.rs` (new) | Each removes its finding with no new finding. | FO-27.4, FO-10, ED-25 |
| FO-27.6 | build | The report document: findings listed with their places, pages annotated where they occur. | `crates/tinker-pdf/src/preflight/report.rs` (new), `crates/tinker-pdf/tests/preflight.rs` (new) | The report lists each finding once and annotates its location. | FO-27.3 |
| FO-27.7 | build | `tpdf preflight --profile` with fix-ups. | `tools/tpdf/src/main.rs`, `docs/features/writing.md` | The command reports and fixes `testdata/` fixtures to the facade's output. | FO-27.5, FO-27.6 |

**Closes the row:** FO-27.4 and FO-27.5 remove each targeted finding with no new one, on FO-27.2's profiles, which is the row's exit.
