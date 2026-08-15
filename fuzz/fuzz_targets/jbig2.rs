//! JBIG2 (ITU-T T.88): segment headers, the MQ coder, generic regions.
//!
//! Two attacker-controlled surfaces meet here and neither is small. Clause
//! 7.2's segment header is a chain of variable-width fields where each one
//! decides how wide the next is — a referred-to count whose own encoding
//! changes above six, referred-to numbers that widen with the segment number,
//! a page association that is one byte or four, a data length that may be a
//! sentinel — so a header is a small parser in its own right. Behind it,
//! 7.4.1 and 7.4.6 hand a region 32-bit dimensions and eight signed AT offsets
//! that address pixels outside the bitmap by design.
//!
//! The first byte chooses the page dimensions and how the rest is split
//! between `/JBIG2Globals` and the image's own stream, so one corpus explores
//! the embedded organisation of Annex D.3 as well as the segments themselves.
//! Every knob is in that byte and each new one takes a higher bit, so a seed
//! written for an earlier set keeps its meaning.
//!
//! What is asserted beyond "it did not panic": a decode that *succeeded*
//! returns exactly the packed 1-bpp page its caller sized, because the render
//! path indexes that buffer by row without re-checking it, and a page one row
//! short would be read past. And a decode that *failed* is the capability
//! error rather than any other — the refusal is the whole degradation contract
//! for this codec, and a decoder that started returning something else would
//! change what the caller draws.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{jbig2_decode, Capability, FilterError, Jbig2Params};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Kept small so a fuzzer's time goes into the parser and the coder rather
    // than into decoding a megapixel of arithmetic. The ceiling below is the
    // other half of that: a region is bounded before it allocates, and a
    // generous ceiling here would let one input spend seconds inside
    // `decode_arithmetic` and time the run out instead of exploring it.
    let width = match knobs & 3 {
        0 => 1,
        1 => 8,
        2 => 37,
        _ => 64,
    };
    let height = match (knobs >> 2) & 3 {
        0 => 1,
        1 => 8,
        2 => 56,
        _ => 61,
    };

    // Annex D.3 puts a document's shared segments in a separate stream that
    // every image names, and the two are enumerated as one sequence. Splitting
    // the body between them reaches that seam; a split of zero leaves the
    // ordinary no-globals shape reachable, which is what most seeds are.
    let split = match (knobs >> 4) & 3 {
        0 => 0,
        1 => body.len() / 4,
        2 => body.len() / 2,
        _ => body.len(),
    };
    let (globals, own) = body.split_at(split.min(body.len()));

    let mut warnings = Vec::new();
    let params = Jbig2Params {
        globals,
        width,
        height,
    };
    let ceiling = 1 << 16;
    match jbig2_decode(own, &params, ceiling, &mut warnings) {
        Ok(page) => {
            // The page is indexed by row on the render path without being
            // re-measured, so a short buffer is read past rather than noticed.
            let stride = (width as usize).div_ceil(8);
            assert_eq!(
                page.len(),
                stride * height as usize,
                "a successful decode returned a page that is not the size its \
                 caller asked for"
            );
            assert!(page.len() <= ceiling, "the output ceiling was exceeded");
        }
        Err(error) => {
            // Ruling 2 and ruling 3: the only failure this codec has is the
            // named capability, and the caller draws the placeholder for it.
            // `BadParams` would mean the API was held wrong, which no input can
            // do.
            assert_eq!(
                error,
                FilterError::Unsupported(Capability::Jbig2),
                "a JBIG2 stream failed with something other than the refusal"
            );
        }
    }

    // The warning set is closed and each variant is recorded at most once per
    // decode (the crate's contract), so a stream of a million bad segments
    // cannot turn leniency into an allocation attack.
    let mut seen = warnings.clone();
    seen.sort_by_key(|w| w.as_str());
    seen.dedup();
    assert_eq!(
        seen.len(),
        warnings.len(),
        "a warning was recorded more than once"
    );

    // The same bytes with the two streams the other way round. The globals are
    // read first and a page information segment in them changes what every
    // segment after it is measured against, so the order is part of the
    // parser's state rather than a detail of who supplied what.
    let swapped = Jbig2Params {
        globals: own,
        width,
        height,
    };
    let mut other = Vec::new();
    let _ = jbig2_decode(globals, &swapped, ceiling, &mut other);
});
