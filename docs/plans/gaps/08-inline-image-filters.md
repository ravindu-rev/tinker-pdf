# An inline image with a predictor decodes to noise

`BI … ID … EI` embeds a small image directly in the content stream. If it is
Flate-compressed with a PNG predictor — the ordinary way to compress image
samples — the samples come out as noise, and no error is raised. If it is a
JPEG or a fax, it is refused outright. When this is done, an inline image
decodes exactly as the same image would in an XObject. (S)

## What is wrong

`decode_inline` in `crates/tinker-pdf/src/resources.rs` hand-rolls its own
filter chain:

```rust
b"FlateDecode" | b"Fl" => flate_decode(&bytes, &limits, None)…,
b"LZWDecode"   | b"LZW" => lzw_decode(&bytes, &limits, true, None)…,
```

The final `None` is the predictor. `/DP` *is* parsed — it is in the
abbreviation table, so the key survives into the re-parsed dictionary — and
`decode_inline` never reads it. The `true` hardcodes `/EarlyChange`, ignoring
the parameter.

DCT and CCITT hit the catch-all and return `Err`, which surfaces as an
unsupported-codec warning and a grey placeholder. 8.9.7 permits both inline.

The non-inline path does all of this correctly, three files away: it routes
through `stream_decoded`, which builds a chain with `PredictorParams` from
`/DecodeParms` and Table 10's defaults, handles the per-filter array form,
reads `/EarlyChange`, and has DCT and CCITT branches that call
`stream_image_input` and `decode_parms`.

## Scope

- Build the inline chain with `PredictorParams` from `/DP`, with Table 10's
  defaults (predictor 1, colors 1, bpc 8, columns 1).
- Honour `/EarlyChange` for LZW rather than hardcoding it.
- Handle the per-filter `/DecodeParms` array form.
- Route inline DCT and CCITT to the same decoders the XObject path uses.
- Use `limits::MAX_DECODED_STREAM` rather than a hardcoded ceiling.

## Non-goals

- **Unifying the two sample-unpacking loops.** They are near-duplicates and
  the duplication is real, but merging them is a refactor with its own risk
  and this is a correctness fix. Note it and leave it.
- **Caching inline images.** XObject images are cached by name; inline ones
  have no name. Not worth inventing one.

## Design

The chain builder that does this properly (`build_chain` and `spec` in
`crates/tinker-pdf-cos/src/streams.rs`) is `pub(crate)` to the COS crate, so
`tinker-pdf` cannot call it. Two options:

**Rebuild the chain from the public filter surface.** `filters::apply_chain`,
`FilterSpec::new`/`with_predictor`/`with_early_change`, `PredictorParams` and
`ChainOutput` are all public. `ChainOutput::EncodedImage` is exactly the
"stopped at the codec" result the DCT and CCITT branches want. This duplicates
about twenty lines of parameter marshalling.

**Or make the COS builder public.** One line, no duplication, and it puts the
Table 10 defaults in one place rather than two — which matters, because a
default that drifts between the two paths is precisely the class of bug this
document exists to fix.

Prefer the second. `decode_parms` is already a private method on
`PageResources` and is directly callable for the CCITT parameters.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Predictor and `/EarlyChange` honoured; array `/DP` form | A Flate-with-PNG-predictor inline image decodes to the same samples as the identical XObject image | S |
| 2 | Inline DCT and CCITT decode | An inline JPEG renders; an inline fax renders; neither reports an unsupported codec | S |
| 3 | Shared limits | The inline path uses the same ceiling as the XObject path, asserted by a test that trips it | S |

## Dependencies

**Needs first:** nothing. `stream_image_input` already exists — it was added
when the XObject path was fixed.

**Unblocks:** nothing; this is a correctness fix on a path that is used less
often than XObject images but is not rare.

## Risks

| Risk | Mitigation |
| --- | --- |
| A shared chain builder could change XObject behaviour while fixing inline | The XObject path already has tests; run them unchanged, and add the inline test that compares the two paths on identical bytes |
| Inline images are attacker-controlled and sit inside a content stream, so a limit regression is a denial of service | The shared ceiling is the point; the test that trips it is the guard |

## As built — August 2026

Three commits, one per milestone: `c4a96a9`, `870a54d`, `e7ef2d9`. Every defect
this document describes was live against the code as it stood after gaps 06, 12
and 16 had all edited `resources.rs`, and each was measured before it was fixed.

**The second option was taken.** `CosDocument::filter_chain(dict, sink)` is
`build_chain` — the function the three stream tiers have always used — made
reachable from outside the COS crate, rather than `DocNames` being made public:
the builder needs a document's name table and its resolver, so a method on the
document is the smaller surface and the caller cannot assemble the arguments
wrongly. Table 10's defaults are now in one place, which was the whole argument.
`xtask dag` is unaffected — no crate edge changed, only a function's visibility.

**The inline CCITT path goes through `ccitt_samples` and nowhere else.** That is
gap 16's seam, used as it asked to be used.

### Two defects the plan does not describe, both blocking it

Neither could be left, because with either of them present `/DP` still never
reaches the chain.

**Table 93's abbreviations were expanded by text substitution on `"/Fl "` and
its siblings** — the trailing space mattered. 7.2.2 makes `/` a delimiter in
its own right, so `/F/Fl` and `[/AHx/Fl]` are ordinary PDF that the search
finds nothing in: the whole `/Filter` entry stayed a two-letter name, and the
compressed bytes were sampled as though they were pixels. Measured: mid-grey
`(67, 67, 67)` where the image is red. The rewrite now reads name by name,
which also fixes `/CS/G` and `/D[1 0]/DP<<…>>`.

**8.9.7's white space before `EI` was being kept as data.** It costs nothing on
unfiltered samples, whose length the dictionary gives, but it is one byte past
the end of a complete zlib or LZW stream, and every decoder here calls that
`TrailingGarbage`. It is dropped in `split_inline_image` for the same reason
7.3.8.2 drops the end-of-line before `endstream` — which mattered only once the
chain's warnings stopped being discarded.

That discarding was itself gap 16's finding repeating on this path: chain
warnings now reach `damaged_images`, so an inline image reports what it forgave
exactly as an XObject one does (ruling 10).

### What each defect was worth, measured

| Defect | Before | After |
| --- | --- | --- |
| Predictor passed as `None` | One shifted row and three black ones | Red and blue stripes, byte-identical to the XObject |
| `/EarlyChange` hardcoded `true` | Correct to row seven, noise below | Correct to the last row |
| DCT refused | Grey placeholder, `UnsupportedImage { codec: "DCTDecode" }` | Decodes; `/Decode [1 0]` inverts it |
| CCITT refused | Grey placeholder, `UnsupportedImage { codec: "CCITTFaxDecode" }` | Four quadrants, byte-identical to the XObject |
| Ceiling of `1 << 26` | Truncated between 64 and 128 MiB where the XObject was whole | `limits::MAX_DECODED_STREAM`, both ways |

### What the injections found

Six deliberate injections, each re-run with `--no-fail-fast`. Five were caught
by the assertions written for them. One was not, and it is the interesting one.

An injection that replaced `ccitt_samples` with a **parallel call site that
happened to guess `/Columns` right** was caught only by
`a_damaged_inline_fax_row_is_reported_the_way_an_xobject_one_is`, which fails
because a parallel site discards the decoder's warnings.
`an_inline_group_4_fax_renders_to_the_same_pixels_as_the_xobject` passed
against it. The quadrant test *does* catch the byte-per-pixel contract — the
right-hand pixels, not the left, since a negative at one eighth width still
fills the left-hand eighth — but it cannot see a second route that reaches the
same answer. Ruling 10's warning is what pins the *route*, and that is why both
tests exist.

### Non-goals, honoured

**The two sample-unpacking loops are still two.** `decode_image_at` and
`decode_inline` each carry a near-identical loop over bits, `/Decode` and
`/ImageMask`. The duplication is real and was seen rather than missed; merging
them is a refactor with its own risk and this was a correctness fix. The
shared parts that could be lifted without touching either loop were:
`jpeg_image` (the JPEG branch, so both get the same ceiling and the same
`/Decode` reversal) and `ccitt_samples` (already shared, by construction).

**Inline images are still not cached.** They have no name to key on.

### Not done

No fuzz campaign. Two seeds are added — `content_tokenizer/inline-image-filters`
and `render_page/inline-image-filters.pdf`, each carrying a predicted Flate
image with compact spellings, a G4 fax and a hex-armoured JPEG, all inline —
and they are reachable **by construction**, not measured: the seed renders with
an empty warnings list, which is only possible if all three decoded, but no
`cargo fuzz run` was executed for this gap. Gap [24](24-fuzz-execution.md) M5
owns that.

No determinism fixture. None of the seven uses an inline image and none moved,
on native Windows or on `wasm32-wasip1`; this gap's pixels are covered by
`crates/tinker-pdf/tests/inline_images.rs` and `tests/ccitt.rs`.
