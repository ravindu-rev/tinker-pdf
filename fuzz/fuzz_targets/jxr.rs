//! JPEG XR (ITU-T T.832): Annex A's tag-based container and clause 8's
//! codestream.
//!
//! It lands with the *first* milestone of the decoder rather than the last,
//! per the rule that every decoder gets a target the day it exists. The
//! reason is specific to this format: Annex A is a TIFF-shaped directory of
//! **file offsets**, and clause 8 hands out a macroblock grid derived by
//! *subtraction* — 8.3.25 infers the last tile's width by taking the declared
//! ones away from `MBWidth`, so a file that declares too much underflows a
//! `u32` into four billion macroblocks. Both of those are structure the
//! fuzzer must reach before there are pixels behind them.
//!
//! Three attacker-controlled surfaces meet here:
//!
//! - **Annex A offsets.** `IMAGE_OFFSET` and `IMAGE_BYTE_COUNT` are 32-bit
//!   and unrelated to the file's length; `VALUES_OR_OFFSET` is either a value
//!   or a pointer depending on a size the same entry declares.
//! - **The tile grid.** 8.3.23 and 8.3.24 are 12-bit counts, so a header can
//!   claim 16 777 216 tiles, and 8.5's index table is then one `VLW_ESC( )`
//!   per tile per band — read before a single macroblock exists.
//! - **`VLW_ESC( )` itself** (8.2.4), whose first byte selects a two-, four-
//!   or eight-byte width, and three of whose values mean "escape mode" and
//!   return zero rather than failing.
//!
//! What is asserted beyond "it did not panic":
//!
//! - a decode that **succeeded** returns exactly as many bytes as its own
//!   reported geometry implies, and no more than the ceiling it was given.
//! - the reported bit depth is 8 or 16 and nothing else, because
//!   `JxrImage::data`'s layout contract is stated in terms of it.
//! - the warning set stays deduplicated, so a file with a million damaged
//!   tiles cannot turn leniency into an allocation attack.
//! - a failure is a `JxrError`, which is a closed enum of decisions — there
//!   is no "unknown error" arm for a caller to have to guess at.
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**: the raster is the size its own
//! geometry implies, the ceiling held, the depth is 8 or 16, and no warning
//! was recorded twice. None of them asks whether the numbers are the ones the
//! input actually describes, so a decode that is well-formed and *wrong*
//! passes this target exactly as a correct one does.
//!
//! Correctness lives in `crates/tinker-pdf-filters/tests/jxr_fixtures.rs`,
//! whose lossless identity is bit-exact against rasters this repository
//! authored — the one check in this tree that would catch a wrong JPEG XR
//! decode, which is otherwise a soft plausible picture with faint seams at
//! the block edges.
//!
//! This is the shape `brotli` records at length, and the difference is worth
//! keeping in view: there, nothing in the tree can supply the missing check
//! at all — rule 1 leaves no encoder to round-trip against and ruling 13 bars
//! a second decoder. Here the check exists, and it is somewhere else.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_filters::{jxr_decode, JxrError, Limits};

/// Annex A's four-byte file header (A.5.2 to A.5.5) with the first IFD at
/// byte 8, which is where a wrapper puts it.
const FILE_HEADER: [u8; 8] = [0x49, 0x49, 0xBC, 0x01, 0x08, 0x00, 0x00, 0x00];

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Kept small so the fuzzer's time goes into the parsers rather than into
    // reconstructing a megapixel. The budget inside the decoder is the other
    // half of that.
    let ceiling = match knobs & 3 {
        0 => 1 << 8,
        1 => 1 << 12,
        2 => 1 << 16,
        _ => 1 << 20,
    };
    let limits = Limits::new(ceiling);

    // `jxr_decode` takes both a whole Annex A file and a bare `CODED_IMAGE( )`
    // (9.1.5.1's XPS part is the first; a system that carries the codestream
    // alone is the second), and the two take different paths through the
    // front of the decoder. A flat corpus reaches both only if something here
    // splits them — the same trick the `jbig2` target uses for its globals
    // stream and the `jpx` one for its box wrapper.
    let mut inputs: Vec<Vec<u8>> = vec![body.to_vec()];
    if knobs & 4 != 0 {
        // Prefix a file header so an arbitrary body is walked as a directory
        // of offsets rather than rejected on the magic.
        let mut wrapped = FILE_HEADER.to_vec();
        wrapped.extend_from_slice(body);
        inputs.push(wrapped);
    }
    if knobs & 8 != 0 {
        // And prefix 8.3.2's signature so an arbitrary body is walked as a
        // codestream, which is the deeper of the two paths.
        let mut bare = b"WMPHOTO\0".to_vec();
        bare.extend_from_slice(body);
        inputs.push(bare);
    }

    for input in inputs {
        match jxr_decode(&input, &limits) {
            Ok(image) => {
                let bytes = usize::from(image.bits_per_component() / 8);
                let want = (image.width as usize)
                    .checked_mul(image.height as usize)
                    .and_then(|n| n.checked_mul(usize::from(image.channels())))
                    .and_then(|n| n.checked_mul(bytes));
                assert_eq!(
                    Some(image.data.len()),
                    want,
                    "a successful decode returned a raster that is not the size \
                     its own geometry implies"
                );
                assert!(image.data.len() <= ceiling, "the output ceiling was exceeded");
                assert!(
                    image.bits_per_component() == 8 || image.bits_per_component() == 16,
                    "an output depth that is neither 8 nor 16"
                );
                assert!(image.width > 0 && image.height > 0, "a zero-sized success");

                let mut seen = image.warnings.clone();
                seen.sort_by_key(|w| w.as_str());
                seen.dedup_by_key(|w| w.as_str());
                assert_eq!(
                    seen.len(),
                    image.warnings.len(),
                    "a warning was recorded more than once"
                );
            }
            Err(error) => {
                // The error type is a closed enum of decisions; this match
                // exists so that adding a variant without deciding whether a
                // fuzzer may reach it fails to compile.
                match error {
                    JxrError::NotJxr
                    | JxrError::UnsupportedFileVersion(_)
                    | JxrError::UnsupportedCodestreamVersion(_)
                    | JxrError::Truncated
                    | JxrError::MissingRequiredTag(_)
                    | JxrError::ReservedValue(_)
                    | JxrError::BadDimensions
                    | JxrError::BadTiling
                    | JxrError::BadIndexTable
                    | JxrError::BadProfileLevel
                    | JxrError::BadAlphaPlane
                    | JxrError::BadTileStartCode(_)
                    | JxrError::TooManySamples { .. }
                    | JxrError::TooManyTiles { .. }
                    | JxrError::TooManyComponents { .. }
                    | JxrError::TooManyMacroblocks { .. }
                    | JxrError::ExceedsOutputLimit { .. }
                    | JxrError::Unsupported(_) => {}
                }
            }
        }
    }
});
