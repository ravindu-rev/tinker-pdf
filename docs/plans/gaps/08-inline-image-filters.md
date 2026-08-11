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
