//! JPEG 2000 (ITU-T T.800): the JP2 box walk and the Annex A codestream.
//!
//! This is the eighteenth target and it lands with the *first* milestone of
//! the decoder rather than the last, per plan 02's rule that every decoder
//! gets one the day it exists. The reason is specific to this format:
//! everything built after the headers parses attacker-controlled *structure*
//! — tag trees, packet headers, precinct partitions, code-block segment
//! lengths — so the target has to exist before that structure does, not after.
//!
//! Two attacker-controlled surfaces meet here. Annex I's boxes are a length
//! field that may be 32-bit, 64-bit, or "to the end of the file", and a value
//! below eight describes a box shorter than its own header — which a walk
//! that clamped would loop on forever. Annex A's markers hand out a tile grid
//! whose `XTsiz` is a divisor and whose `XOsiz` is a subtrahend, a component
//! count the standard puts at 16 384, and 32-bit dimensions whose product is
//! bounded by nothing in the file.
//!
//! What is asserted beyond "it did not panic":
//!
//! - a decode that **failed** is `Unsupported(Jpx)` and nothing else. That is
//!   the whole degradation contract for this codec — the caller draws the
//!   neutral placeholder — and a decoder that started returning something
//!   else would change what a reader sees.
//! - a decode that **succeeded** returns exactly as many sample bytes as its
//!   own reported geometry implies, and no more than the ceiling it was
//!   given. Nothing succeeds yet; the assertion is here from the first day so
//!   that the milestone which makes it succeed cannot make it succeed wrongly.
//! - the warning set stays deduplicated, so a stream of a million bad markers
//!   cannot turn leniency into an allocation attack.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{jpx_decode, Capability, FilterError, Limits};

/// The twelve bytes a JP2 file opens with (T.800 I.5.1), and the empty
/// `jp2h` needed to make the wrapper a walk rather than a rejection.
const SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Kept small so a fuzzer's time goes into the parsers rather than into
    // decoding a megapixel. The budget inside the decoder is the other half
    // of that: a generous ceiling here would let one input spend seconds in
    // tier-1 and time the run out instead of exploring it.
    let ceiling = match knobs & 3 {
        0 => 1 << 8,
        1 => 1 << 12,
        2 => 1 << 16,
        _ => 1 << 20,
    };
    let limits = Limits::new(ceiling);

    // Bit 2 wraps the body in JP2 boxes. PDF permits both a whole JP2 file
    // and a bare codestream (7.4.9) and they take different paths through the
    // container, so a flat corpus reaches both only if something here splits
    // them — the same trick the `jbig2` target uses for its globals stream.
    let mut inputs: Vec<Vec<u8>> = vec![body.to_vec()];
    if knobs & 4 != 0 {
        let mut jp2 = SIGNATURE.to_vec();
        jp2.extend_from_slice(&boxed(b"ftyp", b"jp2 \x00\x00\x00\x00jp2 "));
        jp2.extend_from_slice(&boxed(
            b"jp2h",
            &boxed(b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]),
        ));
        jp2.extend_from_slice(&boxed(b"jp2c", body));
        inputs.push(jp2);
    }

    for input in inputs {
        let mut warnings = Vec::new();
        match jpx_decode(&input, &limits, &mut warnings) {
            Ok(image) => {
                let bytes = if image.precision > 8 { 2usize } else { 1 };
                let want = (image.width as usize)
                    .checked_mul(image.height as usize)
                    .and_then(|n| n.checked_mul(image.components as usize))
                    .and_then(|n| n.checked_mul(bytes));
                assert_eq!(
                    Some(image.samples.len()),
                    want,
                    "a successful decode returned samples that are not the size \
                     its own geometry implies"
                );
                assert!(
                    image.samples.len() <= ceiling,
                    "the output ceiling was exceeded"
                );
                assert!(
                    image.precision == 8 || image.precision == 16,
                    "an output precision that is neither 8 nor 16"
                );
            }
            Err(error) => {
                // Rulings 2 and 3: the only failure this codec has is the
                // named capability, and the caller draws the placeholder for
                // it. `BadParams` would mean the API was held wrong, which no
                // input can do.
                assert_eq!(
                    error,
                    FilterError::Unsupported(Capability::Jpx),
                    "a JPX stream failed with something other than the refusal"
                );
                // A refusal that says nothing is a refusal a reader cannot
                // act on (ruling 10).
                assert!(!warnings.is_empty(), "a refusal left no warning");
            }
        }

        let mut seen = warnings.clone();
        seen.sort_by_key(|w| w.as_str());
        seen.dedup();
        assert_eq!(
            seen.len(),
            warnings.len(),
            "a warning was recorded more than once"
        );
    }
});
