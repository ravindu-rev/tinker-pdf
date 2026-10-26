# Documentation

One doc per feature, each a record of what exists, derived from the code:
what it does, its API, what it refuses by name, and how it is verified.
Forward-looking work lives only in the [roadmap](ROADMAP.md) and its
[design docs](design/).

## Using the engine

| Doc | Covers |
| --- | --- |
| [features/opening.md](features/opening.md) | bytes → `Document`: sniffing, xref flavours, repair ladder, stream tiers, and opening from a `ByteSource` a range at a time |
| [features/filters.md](features/filters.md) | every stream filter and image codec |
| [features/encryption.md](features/encryption.md) | handlers R2–R6, authenticate, permissions, encrypt-on-save |
| [features/document-model.md](features/document-model.md) | metadata, page tree, outlines, destinations, trees, attachments, XMP |
| [features/fonts.md](features/fonts.md) | TrueType/CFF/Type 1/Type 3, CID, the predefined CMaps, vertical writing, `FontProvider` |
| [features/content-and-text.md](features/content-and-text.md) | interpreter, the `Device` seam, text extraction and search |
| [features/rasterizer.md](features/rasterizer.md) | deterministic AA fill, stroking, clipping, sampling, cancellation |
| [features/rendering.md](features/rendering.md) | colour, functions, shadings, patterns, transparency, optional content |
| [features/writing.md](features/writing.md) | rewrite, incremental update, object streams, encrypt-on-save, linearization |
| [features/editing.md](features/editing.md) | page surgery, annotations, flattening, redaction |
| [features/forms.md](features/forms.md) | field tree, fill, transactions, calculations |
| [features/creation.md](features/creation.md) | `DocumentBuilder`: pages, text, images, patterns, outlines |
| [features/signatures.md](features/signatures.md) | verifying what each signature proves, and signing on an incremental save |
| [features/pdfa.md](features/pdfa.md) | ISO 19005: the validator, its coverage, and the writer's archival profile |
| [features/pdfua.md](features/pdfua.md) | ISO 14289: the validator, the findings it decides, and the clauses it abstains on by name |
| [features/pdfx.md](features/pdfx.md) | ISO 15930: the claim, the 2003 levels' rules from the CGATS notes, and every clause not read, by name |
| [features/cbz.md](features/cbz.md) | comic archives as documents |
| [features/xps.md](features/xps.md) | XPS/OpenXPS as documents |
| [features/epub.md](features/epub.md) | books as documents: the CSS and layout engines |
| [features/bindings.md](features/bindings.md) | C, Python, JavaScript/wasm, .NET, Go, Ruby, Java; Swift unverified |

## Guarantees

| Doc | Covers |
| --- | --- |
| [features/determinism.md](features/determinism.md) | bit-identical output across targets; the fingerprint suite |
| [rulings.md](rulings.md) | the numbered engineering rulings — these override everything |
| [verification.md](verification.md) | how the engine is tested: fuzzing, corpora, ratchets, and what first-party verification cannot prove |

## Project

| Doc | Covers |
| --- | --- |
| [architecture.md](architecture.md) | crate DAG, leaf rule, error model, concurrency, per-crate map |
| [ROADMAP.md](ROADMAP.md) | what is not built yet, tiered by evidence: what a step is, how one runs, and the order of work |
| [roadmap/](roadmap/) | one file per tier, each row with its steps; the acts only a person can do (`H.md`); the hard limits (`NG.md`) |
| [design/](design/) | one design doc per major roadmap item |
| [pdf20-deltas.md](pdf20-deltas.md) | ISO 32000-2 deltas against the 1.7 baseline |
