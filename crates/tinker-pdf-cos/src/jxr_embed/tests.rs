//! The three arrangements a JPEG XR needs before it is a PDF image.
//!
//! # Why these read the filters crate's fixtures
//!
//! `png_embed` and `tiff_embed` build their inputs by hand, because a PNG or
//! a TIFF header is a few dozen bytes anyone can write. A JPEG XR codestream
//! is not: producing one means running an encoder, and the only encoder here
//! is the one Windows ships, which
//! `crates/tinker-pdf-filters/tests/jxr/make-fixtures.ps1` drives offline.
//! Those files are committed, so these tests read them across the crate
//! boundary rather than committing a second copy that could drift from the
//! first.
//!
//! # The permutation is checked by a relation, not by a table
//!
//! `rgb24` and `bgr24` are **the same raster**, authored once in
//! `jxr_fixtures.rs` and handed to the encoder in two different channel
//! orders. So after arrangement their samples must be **byte-identical** —
//! which is a check on the permutation that needs no expected values at all,
//! and which a decoder that ignored channel order entirely would fail.

use super::*;
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    // The fixtures belong to the crate whose decoder they exercise.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tinker-pdf-filters/tests/jxr")
}

fn embed(name: &str) -> JxrImageData {
    let path = fixture_dir().join(format!("{name}.jxr"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    jxr_image(&bytes, &Limits::new(1 << 24)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn bgr_and_rgb_of_one_raster_arrive_as_the_same_samples() {
    // Table A.6's `24bppBGR` and `24bppRGB` rows differ only in channel
    // order, and `/DeviceRGB` has exactly one. Two files of one raster must
    // therefore embed identically — and a build that dropped the permutation
    // would return one with red and blue swapped, which is obvious in a
    // photograph and invisible in a test that only checks the length.
    let rgb = embed("rgb24");
    let bgr = embed("bgr24");
    assert_eq!(rgb.width, bgr.width);
    assert_eq!(rgb.height, bgr.height);
    assert!(rgb.rgb && bgr.rgb);
    assert_eq!(rgb.data, bgr.data, "the two channel orders disagree");
    // And `32bppBGR`'s padding byte is not a channel: it must arrive as the
    // same three samples again.
    assert_eq!(embed("bgr32").data, rgb.data);
}

#[test]
fn an_alpha_row_splits_into_colour_and_a_soft_mask() {
    // 11.6.5.3 wants opacity as its own `/DeviceGray` image, so the
    // interleaved fourth channel is split out. The colour half must be the
    // same three samples the alpha-less rows of the same raster gave.
    let bgra = embed("bgra32");
    let rgb = embed("rgb24");
    assert_eq!(bgra.data, rgb.data, "the colour half changed");
    let mask = bgra.soft_mask.as_ref().expect("32bppBGRA carries alpha");
    assert_eq!(
        mask.len(),
        (bgra.width * bgra.height) as usize,
        "one alpha sample a pixel"
    );
    // Not every sample opaque, or the split is not reaching the output — the
    // same thing an absent alpha plane would produce.
    assert!(
        mask.iter().any(|&a| a != 0xFF),
        "every alpha sample is opaque"
    );
    // A row without alpha has no mask at all, rather than an opaque one:
    // `/SMask` costs a whole second image XObject.
    assert!(rgb.soft_mask.is_none());
    assert!(embed("gray8").soft_mask.is_none());
}

#[test]
fn sixteen_bit_samples_are_big_endian_where_the_container_was_little() {
    // A.7.3 makes the container little-endian; ISO 32000-2 8.9.5.2 makes an
    // image sample big-endian. Every 16-bit sample is therefore swapped, and
    // a build that forgot would embed a gradient as noise — the low byte
    // varies fastest, so the picture would be unrecognisable rather than
    // subtly wrong.
    let path = fixture_dir().join("rgb48.jxr");
    let bytes = std::fs::read(&path).expect("a committed fixture");
    let decoded = jxr_decode(&bytes, &Limits::new(1 << 24)).expect("rgb48");
    let embedded = embed("rgb48");
    assert_eq!(embedded.bits_per_component, 16);
    assert_eq!(embedded.data.len(), decoded.data.len());
    let mut swapped = 0;
    for (out, src) in embedded
        .data
        .chunks_exact(2)
        .zip(decoded.data.chunks_exact(2))
    {
        assert_eq!(
            u16::from_be_bytes([out[0], out[1]]),
            u16::from_le_bytes([src[0], src[1]]),
            "a sample did not survive the byte swap"
        );
        if out[0] != out[1] {
            swapped += 1;
        }
    }
    // A raster whose every sample had two equal bytes would pass the loop
    // above without the swap ever mattering. This one does not.
    assert!(
        swapped > 0,
        "no sample in this fixture distinguishes the two byte orders"
    );
}

#[test]
fn a_grey_row_stays_one_channel() {
    // `/DeviceGray` and not three copies of the same sample: widening it
    // would triple the page for nothing, and 8.6.4's component count is what
    // the dictionary declares.
    let gray = embed("gray8");
    assert!(!gray.rgb);
    assert_eq!(
        gray.data.len(),
        (gray.width * gray.height) as usize,
        "one sample a pixel"
    );
    let gray16 = embed("gray16");
    assert!(!gray16.rgb);
    assert_eq!(gray16.bits_per_component, 16);
    assert_eq!(
        gray16.data.len(),
        (gray16.width * gray16.height * 2) as usize
    );
}

#[test]
fn the_image_dictionary_says_what_the_samples_are() {
    // The arrangement above is only useful if the dictionary agrees with it.
    // A `/DeviceRGB` image whose data is one channel a pixel is a page that
    // reads a third of its picture and then runs off the end.
    for (name, want_rgb, want_bits) in [
        ("gray8", false, 8u8),
        ("gray16", false, 16),
        ("rgb24", true, 8),
        ("rgb48", true, 16),
        ("bgra32", true, 8),
    ] {
        let embedded = embed(name);
        let ImageData::Compressed(image) = embedded.image() else {
            panic!("{name} is not embedded as raw samples");
        };
        let components = usize::from(want_rgb) * 2 + 1;
        assert_eq!(image.bits_per_component, want_bits, "{name}");
        assert_eq!(
            matches!(image.color_space, ImageColorSpace::DeviceRgb),
            want_rgb,
            "{name} colour space"
        );
        assert!(
            image.filter.is_none(),
            "{name} declares a filter it has not applied"
        );
        assert_eq!(
            image.data.len(),
            (image.width * image.height) as usize * components * usize::from(want_bits / 8),
            "{name}: the samples do not fill the geometry the dictionary declares"
        );
        assert!(image.color_key_mask.is_none(), "{name}");
    }
}

#[test]
fn the_resolution_is_the_files_own_and_is_not_defaulted_here() {
    // 13.4.1's 96 belongs to whatever is drawing the picture, not to this
    // module — the same division `tiff_embed` draws. What is checked here is
    // that a resolution the file *states* survives to the caller: WIC writes
    // 96 into every fixture, so `None` would mean the tags are not being read
    // at all and the XPS layer's default would be masking it.
    let embedded = embed("rgb24");
    let (x, y) = embedded.dpi().expect("Annex A's resolution tags");
    assert!(x > 0.0 && y > 0.0, "a resolution must be positive");
    assert_eq!(x, y, "the fixture is square-pixelled");
}

#[test]
fn every_fixture_embeds_completely() {
    // No dropped tile, no lost alpha plane: `complete` is what a caller reads
    // to decide whether the page it is about to draw is the whole picture.
    for name in [
        "gray8", "gray16", "rgb24", "bgr24", "bgr32", "bgra32", "rgb48", "rgba64",
    ] {
        let embedded = embed(name);
        assert!(embedded.complete(), "{name} was not complete");
        assert!(
            embedded.warnings().is_empty(),
            "{name}: {:?}",
            embedded.warnings()
        );
    }
}
