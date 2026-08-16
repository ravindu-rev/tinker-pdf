//! Gates 1 and 3: this decoder against OpenJPEG's, reversible and lossy.
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
//! The 9/7 fixtures are `-i<levels>` for a rate-1 irreversible stream and
//! `-q20` for one truncated to a twentieth of its size, and each comes with
//! the oracle's own decode of it as `.opj.pgm` — the reference image, since a
//! lossy stream has no source image to compare against. Regenerate with:
//!
//! ```text
//! opj_compress -i tests/jpx/r1.pgm -o tests/jpx/r1-2.jp2   -r 1  -n 2
//! opj_compress -i tests/jpx/r1.pgm -o tests/jpx/r1-i2.jp2  -I -r 1  -n 2
//! opj_compress -i tests/jpx/r1.pgm -o tests/jpx/r1-q20.jp2 -I -r 20 -n 2
//! opj_decompress -i tests/jpx/r1-i2.jp2 -o tests/jpx/r1-i2.opj.pgm
//! ```
//!
//! `opj_decompress` writes a `#OpenJPEG-2.5.0` comment into the PGM header,
//! so a whole-file `cmp` fails on a correct decode. Compare samples.
//!
//! # The colour fixtures
//!
//! `c1` to `c3` are the PPM equivalents of `r1` to `r3` — a gradient whose
//! three channels vary on different axes, an odd 17 by 13, and a four-colour
//! diagonal of saturated primaries, which is the hardest case a component
//! transform gets. `opj_compress` turns the multiple component transform on
//! by default for a three-component input, so `c*-1.jp2` and friends are
//! **RCT** streams and `c*-i2.jp2` are **ICT** ones.
//!
//! `s1` to `s3` carry subsampled components, made with `-s`. Their reference
//! images come from `opj_decompress -upsample`, which is the flag that makes
//! the oracle put a subsampled component back on the reference grid — and it
//! does it by **replication**, which is why this build does too. Without the
//! flag `opj_decompress` writes the component grid instead and there is
//! nothing to compare against.
//!
//! ```text
//! opj_compress -i tests/jpx/c1.ppm -o tests/jpx/c1-2.jp2 -r 1 -n 2
//! opj_compress -i tests/jpx/c1.ppm -o tests/jpx/s1.jp2 -s 2,2 -r 1 -n 2
//! opj_decompress -i tests/jpx/s1.jp2 -o tests/jpx/s1.opj.ppm -upsample
//! ```

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpx")).join(name)
}

/// A binary PGM's or PPM's dimensions and samples, comments and all.
fn read_pnm(name: &str) -> (u32, u32, u32, Vec<u8>) {
    let bytes = std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut at = 0usize;
    let mut fields = Vec::new();
    // P5 or P6, width, height, maxval — with `#` comments to end of line
    // anywhere between them, which is where `opj_decompress` puts its
    // signature.
    while fields.len() < 4 && at < bytes.len() {
        match bytes[at] {
            b'#' => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            c if c.is_ascii_whitespace() => at += 1,
            _ => {
                let start = at;
                while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'#' {
                    at += 1;
                }
                fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
            }
        }
    }
    let channels = match fields[0].as_str() {
        "P5" => 1,
        "P6" => 3,
        other => panic!("{name} is a {other}, not a binary PGM or PPM"),
    };
    assert_eq!(fields[3], "255", "{name} is not eight bits");
    let width: u32 = fields[1].parse().unwrap();
    let height: u32 = fields[2].parse().unwrap();
    // Exactly one whitespace byte separates the header from the samples.
    let samples = bytes[at + 1..].to_vec();
    assert_eq!(
        samples.len(),
        (width as usize) * (height as usize) * (channels as usize),
        "{name} carries the wrong number of samples"
    );
    (width, height, channels, samples)
}

/// How far a decode is from a reference image, sample by sample: how many
/// moved at all, how many moved past `threshold`, and the worst of them.
fn compare(name: &str, got: &[u8], want: &[u8], threshold: i32) -> (usize, usize, i32) {
    assert_eq!(
        got.len(),
        want.len(),
        "{name} decoded {} samples against the reference's {}",
        got.len(),
        want.len()
    );
    let (mut differing, mut over, mut worst) = (0usize, 0usize, 0i32);
    for (&got, &reference) in got.iter().zip(want) {
        let moved = (i32::from(got) - i32::from(reference)).abs();
        worst = worst.max(moved);
        if moved > 0 {
            differing += 1;
        }
        if moved > threshold {
            over += 1;
        }
    }
    (differing, over, worst)
}

fn decode(name: &str) -> tinker_pdf_filters::JpxImage {
    let path = fixture(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let mut warnings = Vec::new();
    tinker_pdf_filters::jpx_decode(
        &bytes,
        &tinker_pdf_filters::Limits::new(1 << 24),
        &mut warnings,
    )
    .unwrap_or_else(|e| panic!("{name} refused: {e:?}, warnings {warnings:?}"))
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

/// Gate 3: an irreversible 9/7 decode is within `pdfcmp`'s own tolerance of
/// OpenJPEG's.
///
/// Looser than gate 2 deliberately, and the looseness has a **stated cause**
/// rather than being slack. T.800 E.1.1.2 leaves the reconstruction point `r`
/// inside a quantisation interval to the decoder, and on a stream truncated
/// to a twentieth of its size that choice moves pixels by more than the
/// wavelet ever will. This build uses `r = 0.5`, which is what OpenJPEG's own
/// tier-1 carries as a half bit below the last plane it decoded; `r1-q20`
/// exists to hold that claim to a number rather than to a convention.
///
/// The numbers are `tools/pdfcmp`'s defaults, which are Tinker's own
/// `visual_regression.rs` tolerance: a sample counts as differing when it
/// moves by more than 12 of 255, and at most 0.05 per cent of them may.
#[test]
fn an_irreversible_decode_is_within_tolerance_of_openjpeg() {
    // pdfcmp's defaults, spelled out so a change to them is a change here.
    const THRESHOLD: i32 = 12;
    const BUDGET: f64 = 0.0005;

    let cases: &[&str] = &[
        "r1-i2", "r1-i3", "r2-i2", "r2-i3", "r3-i2", "r3-i3", "r4-i1", "r5-i1", "r1-q20", "r2-q20",
    ];

    let mut compared = 0;
    for &name in cases {
        let (w, h, _, want) = read_pnm(&format!("{name}.opj.pgm"));
        let image = decode(&format!("{name}.jp2"));
        assert_eq!(
            (image.width, image.height),
            (w, h),
            "{name} decoded at the wrong size"
        );

        let (differing, over, worst) = compare(name, &image.samples, &want, THRESHOLD);
        let fraction = over as f64 / want.len() as f64;
        assert!(
            fraction <= BUDGET,
            "{name}: {over} of {} samples moved more than {THRESHOLD} of 255 \
             ({fraction:.5} against a budget of {BUDGET}); worst {worst}, \
             {differing} differ at all. If the whole image has shifted rather \
             than a few pixels, E.1.1.2's reconstruction point r is the first \
             thing to check -- measure it, do not widen this",
            want.len()
        );
        // The measured figure, held rather than merely reported. Every one of
        // these ten fixtures agrees with OpenJPEG to **one level of 255**, and
        // the residual has a known cause rather than being slack: OpenJPEG's
        // irreversible path is `f32` and it rounds the final sample
        // half-to-even through `lrintf`, where this build rounds half-up on
        // an arithmetic shift because that is the same on every target
        // (ruling 4). Ties are common on a synthetic ramp and rare on a
        // photograph, which is why `r4` and `r5` carry most of them.
        //
        // A new fixture truncated far harder than `-r 20` could legitimately
        // exceed this, because E.1.1.2's reconstruction point genuinely is
        // the decoder's choice there. Nothing else may.
        assert!(
            worst <= 1,
            "{name}: a sample moved {worst} levels from OpenJPEG, past the one \
             level every committed fixture measures at. That is a defect \
             rather than a tolerance unless a far more truncated fixture has \
             just been added"
        );
        compared += 1;
    }

    assert!(
        compared >= 10,
        "only {compared} lossy fixtures were compared; a gate that silently \
         stops comparing is worse than no gate"
    );
}

/// Gate 1, extended to three components: a losslessly coded RGB stream comes
/// back **byte-identical** to the image it was made from.
///
/// The inverse RCT is exact integer arithmetic, so it needed no gate of its
/// own — it joins the one that was already the strongest thing in the plan.
/// What this adds to that gate is real rather than incidental: it is the only
/// check in the repository that can see G.2.1's two chrominance terms
/// transposed, because that transposition round-trips perfectly against a
/// forward RCT with the same mistake and is invisible to every unit test a
/// round trip can write. `opj_compress` turns the transform on by default for
/// a three-component input, so every fixture here exercises it.
#[test]
fn a_reversible_rgb_decode_is_byte_identical_to_openjpeg() {
    let cases: &[(&str, u32, u32)] = &[("c1", 32, 24), ("c2", 17, 13), ("c3", 8, 8)];

    let mut compared = 0;
    for &(name, w, h) in cases {
        let (sw, sh, channels, want) = read_pnm(&format!("{name}.ppm"));
        assert_eq!((sw, sh, channels), (w, h, 3), "{name}.ppm");
        for level in [1u32, 2, 3] {
            let image = decode(&format!("{name}-{level}.jp2"));
            assert_eq!(
                (image.width, image.height),
                (w, h),
                "{name}-{level} decoded at the wrong size"
            );
            assert_eq!(image.components, 3, "{name}-{level} is RGB");
            assert_eq!(image.precision, 8, "{name}-{level} is eight bits");
            assert!(
                image.opacity.is_none(),
                "{name}-{level} carries no cdef opacity channel"
            );

            let (differing, _, worst) = compare(name, &image.samples, &want, 0);
            assert_eq!(
                differing,
                0,
                "{name}-{level}: {differing} of {} samples differ from the \
                 source image (worst {worst}). For a reversible transform and \
                 an exact RCT there is no tolerance to negotiate: one of the \
                 two decoders is wrong. If red and blue have exchanged, G.2.1 \
                 puts red on the second chrominance and blue on the first",
                want.len()
            );
            compared += 1;
        }
    }
    assert!(
        compared >= 9,
        "only {compared} RGB fixtures were compared; a gate that silently \
         stops comparing is worse than no gate"
    );
}

/// Gate 1 again, on components that do not cover the reference grid: a
/// subsampled component is replicated onto it, **byte-identically** to
/// `opj_decompress -upsample`.
///
/// This is the whole justification for replicating rather than filtering, and
/// it is worth being exact about why the two differ. `docs/plans/02-filters.md`
/// chose a triangle filter for JPEG's chroma because *its* oracle,
/// libjpeg-turbo, interpolates. OpenJPEG does not: it takes the sample at
/// `floor(X / XRsiz)` and repeats it. Byte-identity is only reachable against
/// the decoder one is actually compared with, so the two plans disagree on
/// purpose and each says so.
///
/// `s2` is the one that matters most — `-s 2,1` on a 17-wide image, so the
/// subsampling divides neither the width nor the tile origin and the last
/// output column reads a component sample that has to be clamped rather than
/// read past.
#[test]
fn a_subsampled_component_is_replicated_like_openjpeg() {
    let cases: &[(&str, &str, u32)] = &[
        ("s1", "s1.opj.ppm", 3),
        ("s2", "s2.opj.ppm", 3),
        ("s3", "s3.opj.pgm", 1),
    ];

    let mut compared = 0;
    for &(name, reference, channels) in cases {
        let (w, h, got_channels, want) = read_pnm(reference);
        assert_eq!(got_channels, channels, "{reference}");
        let image = decode(&format!("{name}.jp2"));
        assert_eq!(
            (image.width, image.height),
            (w, h),
            "{name} decoded at the wrong size: a subsampled component's image \
             is the *reference grid*, not the component grid"
        );
        assert_eq!(u32::from(image.components), channels, "{name}");

        let (differing, _, worst) = compare(name, &image.samples, &want, 0);
        assert_eq!(
            differing,
            0,
            "{name}: {differing} of {} samples differ from `opj_decompress \
             -upsample` (worst {worst}). A worst of one or two at block edges \
             means this build interpolated where the oracle replicated",
            want.len()
        );
        compared += 1;
    }
    assert!(
        compared >= 3,
        "only {compared} subsampled fixtures were compared"
    );
}

/// Gate 3, extended to the ICT: an irreversible RGB decode is within
/// `pdfcmp`'s own tolerance of OpenJPEG's.
///
/// The ICT is a float transform in T.800 G.2.2 and this build runs it in the
/// 9/7's own Q12 through the 9/7's own `mul_q24`, so it carries the same
/// residual for the same reason: OpenJPEG's irreversible path is `f32` and
/// rounds the final sample half-to-even through `lrintf`, where this build
/// rounds half-up on an arithmetic shift because that is identical on every
/// target (ruling 4).
#[test]
fn an_irreversible_rgb_decode_is_within_tolerance_of_openjpeg() {
    const THRESHOLD: i32 = 12;
    const BUDGET: f64 = 0.0005;

    let cases: &[&str] = &["c1-i2", "c2-i2", "c3-i2", "c1-q20", "c2-q20", "c3-q20"];

    let mut compared = 0;
    for &name in cases {
        let (w, h, channels, want) = read_pnm(&format!("{name}.opj.ppm"));
        assert_eq!(channels, 3, "{name}");
        let image = decode(&format!("{name}.jp2"));
        assert_eq!(
            (image.width, image.height),
            (w, h),
            "{name} decoded at the wrong size"
        );

        let (differing, over, worst) = compare(name, &image.samples, &want, THRESHOLD);
        let fraction = over as f64 / want.len() as f64;
        assert!(
            fraction <= BUDGET,
            "{name}: {over} of {} samples moved more than {THRESHOLD} of 255 \
             ({fraction:.5} against a budget of {BUDGET}); worst {worst}, \
             {differing} differ at all. A whole image shifted rather than a \
             few pixels means G.2.2's constants, not the tolerance",
            want.len()
        );
        // The same measured figure the greyscale fixtures hold, and held for
        // the same reason: it is a rounding tie rather than arithmetic, so it
        // cannot quietly drift into being two.
        assert!(
            worst <= 1,
            "{name}: a sample moved {worst} levels from OpenJPEG, past the one \
             level every committed fixture measures at"
        );
        compared += 1;
    }
    assert!(
        compared >= 6,
        "only {compared} lossy RGB fixtures were compared"
    );
}
