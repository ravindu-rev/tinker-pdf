//! Every committed JPEG 2000 codestream's decode, pinned.
//!
//! The decoder's speed work (`docs/design/jpx-speed.md`) computes the
//! numbers it computes today in less time, so at a full request — no level
//! discarded, no window — every committed codestream must decode to exactly
//! the samples it decodes to now. `jpx_reference.rs` holds the reversible
//! decodes byte-identical to the committed reference decodes, but the
//! irreversible ones only within a tolerance, which a sample moving by one
//! level passes. So each decode is pinned here by what it is: its size, its
//! channels, its precision, whether it was truncated, and the CRC-32 of its
//! samples and of its opacity channel, taken before any of that work landed.
//!
//! A moved pin is a finding about the change that moved it, never an edit.

use std::path::Path;

use tinker_pdf_filters::{crc32, jpx_decode, jpx_decode_request, JpxRequest, Limits};

/// One committed codestream's decode: `(file, width, height, components,
/// precision, truncated, CRC-32 of the samples, CRC-32 of the opacity
/// channel where there is one)`.
type Pin = (&'static str, u32, u32, u8, u8, bool, u32, Option<u32>);

/// Every `.jp2` under `tests/jpx/`, by name.
const PINS: &[Pin] = &[
    ("c1-1.jp2", 32, 24, 3, 8, false, 877789221, None),
    ("c1-2.jp2", 32, 24, 3, 8, false, 877789221, None),
    ("c1-3.jp2", 32, 24, 3, 8, false, 877789221, None),
    ("c1-i2.jp2", 32, 24, 3, 8, false, 763299821, None),
    ("c1-q20.jp2", 32, 24, 3, 8, false, 768810341, None),
    ("c2-1.jp2", 17, 13, 3, 8, false, 1391365984, None),
    ("c2-2.jp2", 17, 13, 3, 8, false, 1391365984, None),
    ("c2-3.jp2", 17, 13, 3, 8, false, 1391365984, None),
    ("c2-i2.jp2", 17, 13, 3, 8, false, 224737970, None),
    ("c2-q20.jp2", 17, 13, 3, 8, false, 2751480611, None),
    ("c3-1.jp2", 8, 8, 3, 8, false, 2672215763, None),
    ("c3-2.jp2", 8, 8, 3, 8, false, 2672215763, None),
    ("c3-3.jp2", 8, 8, 3, 8, false, 2672215763, None),
    ("c3-i2.jp2", 8, 8, 3, 8, false, 906496034, None),
    ("c3-q20.jp2", 8, 8, 3, 8, false, 2855330057, None),
    ("r1-1.jp2", 32, 24, 1, 8, false, 3220669738, None),
    ("r1-2.jp2", 32, 24, 1, 8, false, 3220669738, None),
    ("r1-3.jp2", 32, 24, 1, 8, false, 3220669738, None),
    ("r1-i2.jp2", 32, 24, 1, 8, false, 3332534050, None),
    ("r1-i3.jp2", 32, 24, 1, 8, false, 3011193082, None),
    ("r1-q20.jp2", 32, 24, 1, 8, false, 2371693599, None),
    ("r2-1.jp2", 17, 13, 1, 8, false, 2328013500, None),
    ("r2-2.jp2", 17, 13, 1, 8, false, 2328013500, None),
    ("r2-3.jp2", 17, 13, 1, 8, false, 2328013500, None),
    ("r2-i2.jp2", 17, 13, 1, 8, false, 2942245209, None),
    ("r2-i3.jp2", 17, 13, 1, 8, false, 3028040989, None),
    ("r2-q20.jp2", 17, 13, 1, 8, false, 958350132, None),
    ("r3-1.jp2", 8, 8, 1, 8, false, 3880122039, None),
    ("r3-2.jp2", 8, 8, 1, 8, false, 3880122039, None),
    ("r3-3.jp2", 8, 8, 1, 8, false, 3880122039, None),
    ("r3-i2.jp2", 8, 8, 1, 8, false, 3880122039, None),
    ("r3-i3.jp2", 8, 8, 1, 8, false, 3880122039, None),
    ("r4-1.jp2", 64, 1, 1, 8, false, 955437983, None),
    ("r4-i1.jp2", 64, 1, 1, 8, false, 2987596884, None),
    ("r5-1.jp2", 1, 64, 1, 8, false, 955437983, None),
    ("r5-i1.jp2", 1, 64, 1, 8, false, 2987596884, None),
    ("s1.jp2", 63, 47, 3, 8, false, 1385374381, None),
    ("s2.jp2", 33, 13, 3, 8, false, 1396086416, None),
    ("s3.jp2", 63, 47, 1, 8, false, 57881810, None),
];

/// The pin a decode of `bytes` reads as.
fn pin_of(name: &'static str, image: &tinker_pdf_filters::JpxImage) -> Pin {
    (
        name,
        image.width,
        image.height,
        image.components,
        image.precision,
        image.truncated,
        crc32(&image.samples),
        image
            .opacity
            .as_ref()
            .map(|opacity| crc32(&opacity.samples)),
    )
}

/// The committed codestreams, by file name, sorted.
fn committed() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jpx");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.expect("a directory entry").file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".jp2"))
        .collect();
    names.sort();
    names
}

#[test]
fn every_committed_decode_is_its_pin() {
    let names = committed();
    assert!(names.len() > 30, "the committed codestreams: {names:?}");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jpx");
    let mut got = Vec::new();
    for name in &names {
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut warnings = Vec::new();
        let image = jpx_decode(&bytes, &Limits::new(1 << 26), &mut warnings)
            .unwrap_or_else(|e| panic!("{name} decodes: {e:?} {warnings:?}"));
        // A name the table can hold: the leaked string lives for the test.
        let name: &'static str = Box::leak(name.clone().into_boxed_str());
        got.push(pin_of(name, &image));
    }
    let lines: Vec<String> = got.iter().map(|pin| format!("    {pin:?},")).collect();
    assert_eq!(
        got,
        PINS,
        "the committed decodes are not their pins; today's are:\n{}",
        lines.join("\n")
    );
}

/// Every committed codestream decoded dropping 0 to 6 levels: the applied
/// discard is the asked one up to the codestream's own levels and clamped
/// there, each side of the image is the full side halved that many times —
/// rounded either way by the origin (T.800 B.5) — and dropping more levels
/// decodes no more code-blocks and synthesises no more levels.
#[test]
fn every_committed_codestream_decodes_at_every_discard() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jpx");
    let limits = Limits::new(1 << 26);
    for name in committed() {
        let bytes = std::fs::read(dir.join(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let decode = |discard: u8| {
            let mut warnings = Vec::new();
            jpx_decode_request(&bytes, &limits, &JpxRequest { discard }, &mut warnings)
                .unwrap_or_else(|e| panic!("{name} dropping {discard}: {e:?} {warnings:?}"))
        };
        let (whole, _) = decode(0);
        let levels = decode(u8::MAX).0.discard;
        let mut previous = None;
        for discard in 0..=6u8 {
            let (image, counts) = decode(discard);
            assert_eq!(
                image.discard,
                discard.min(levels),
                "{name} dropping {discard}"
            );
            let d = u32::from(image.discard);
            for (full, got) in [(whole.width, image.width), (whole.height, image.height)] {
                let (full, got) = (u64::from(full), u64::from(got));
                assert!(
                    (full >> d..=full.div_ceil(1 << d)).contains(&got),
                    "{name} dropping {discard}: a side of {got} from {full}"
                );
            }
            assert_eq!(
                (image.components, image.precision),
                (whole.components, whole.precision),
                "{name} dropping {discard}"
            );
            if let Some((blocks, levels)) = previous {
                assert!(
                    counts.code_blocks_decoded <= blocks,
                    "{name} dropping {discard}"
                );
                assert!(
                    counts.levels_synthesised <= levels,
                    "{name} dropping {discard}"
                );
            }
            previous = Some((counts.code_blocks_decoded, counts.levels_synthesised));
        }
    }
}

/// The full request is the decode `jpx_decode` makes: the same image, and
/// counts of every level and every code-block.
#[test]
fn the_default_request_is_the_full_decode() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jpx");
    for name in committed() {
        let bytes = std::fs::read(dir.join(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let limits = Limits::new(1 << 26);
        let mut warnings = Vec::new();
        let whole = jpx_decode(&bytes, &limits, &mut warnings).expect("it decodes");
        let mut requested = Vec::new();
        let (image, counts) =
            jpx_decode_request(&bytes, &limits, &JpxRequest::default(), &mut requested)
                .expect("it decodes");
        assert_eq!(image, whole, "{name}");
        assert_eq!(requested, warnings, "{name}");
        assert_eq!(image.discard, 0, "{name}");
        assert!(counts.code_blocks_decoded > 0, "{name}: {counts:?}");
    }
}
