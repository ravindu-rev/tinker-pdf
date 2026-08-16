//! Gate 1: a reversible 5/3 decode is byte-identical to OpenJPEG's.
//!
//! This is the strongest check the JPEG 2000 decoder has, and until milestone
//! 4 it could not be run at all: tier-1 emits coefficients, and until the
//! wavelet turns them into samples there is nothing to compare. Milestones 1
//! to 3 had to be checked against transcribed tables and a round trip through
//! a writer that shares this decoder's assumptions.
//!
//! Byte-identity here pins, in one comparison and against a decoder sharing
//! no code with this one: the JP2 container, tier-2's packet arithmetic,
//! tier-1's context *numbering*, dequantisation, the inverse 5/3, and the DC
//! level shift. Only the 9/7's fixed-point arithmetic is outside it, and that
//! is milestone 5 with gates of its own.
//!
//! It matters more than an ordinary oracle check because **T.800 publishes no
//! datastream annex** — there is no equivalent of T.88's Annex H.1, which is
//! the artefact gap 17 leaned on for JBIG2. This comparison carries that
//! weight instead.
//!
//! # The fixtures
//!
//! Made from *our own* images with `opj_compress -r 1` (lossless, so the
//! reversible 5/3), at one, two and three decomposition levels, and committed
//! under ruling 9 — the oracle is invoked, never vendored, and ISO's own
//! conformance codestreams are not redistributable in any case. The shapes
//! are deliberate: an even grid, an odd one (17 by 13, so every subband has a
//! partial code-block), a one-pixel checkerboard, and two single-row/column
//! images where the 1D lifting degenerates.
//!
//! Regenerate with:
//!
//! ```text
//! opj_compress -i tests/jpx/r1.pgm -o tests/jpx/r1-2.jp2 -r 1 -n 2
//! ```

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpx")).join(name)
}

/// Every committed fixture decodes to exactly the samples it was made from.
#[test]
fn a_reversible_decode_is_byte_identical_to_openjpeg() {
    // (name, width, height, decomposition levels available)
    let cases: &[(&str, u32, u32, &[u32])] = &[
        ("r1", 32, 24, &[1, 2, 3]),
        ("r2", 17, 13, &[1, 2, 3]),
        ("r3", 8, 8, &[1, 2, 3]),
        ("r4", 64, 1, &[1]),
        ("r5", 1, 64, &[1]),
    ];

    let mut compared = 0;
    for &(name, w, h, levels) in cases {
        let pgm = std::fs::read(fixture(&format!("{name}.pgm")))
            .unwrap_or_else(|e| panic!("{name}.pgm: {e}"));
        // The PGM header is three lines; the samples are the tail.
        let want = &pgm[pgm.len() - (w as usize) * (h as usize)..];

        for &lv in levels {
            let path = fixture(&format!("{name}-{lv}.jp2"));
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));

            let mut warnings = Vec::new();
            let image = tinker_pdf_filters::jpx_decode(
                &bytes,
                &tinker_pdf_filters::Limits::new(1 << 24),
                &mut warnings,
            )
            .unwrap_or_else(|e| panic!("{name}-{lv} refused: {e:?}, warnings {warnings:?}"));

            assert_eq!(
                (image.width, image.height),
                (w, h),
                "{name}-{lv} decoded at the wrong size"
            );
            assert_eq!(image.components, 1, "{name}-{lv} is greyscale");
            assert_eq!(image.precision, 8, "{name}-{lv} is eight bits");

            let differing = image
                .samples
                .iter()
                .zip(want)
                .filter(|(got, want)| got != want)
                .count();
            assert_eq!(
                differing,
                0,
                "{name}-{lv}: {differing} of {} samples differ from OpenJPEG's, \
                 which for a reversible transform means one of the two decoders \
                 is wrong rather than that a tolerance needs widening",
                want.len()
            );
            compared += 1;
        }
    }

    assert!(
        compared >= 11,
        "only {compared} fixtures were compared; a gate that silently stops \
         comparing is worse than no gate"
    );
}
