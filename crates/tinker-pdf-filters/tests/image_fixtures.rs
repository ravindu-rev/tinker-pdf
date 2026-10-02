//! The container-image decoders held to pictures somebody else encoded.
//!
//! Ruling 13's rule for a lossless codec is that **the expected output is the
//! generator's input**. Every file under `tests/images/` that a
//! `make-images.py` wrote is a formula — [`recipe`] here, the same functions
//! in the script — handed once to a third-party encoder, and every test below
//! recomputes the formula and compares it with what this repository's decoder
//! makes of the bytes. No decoder outside this repository is asked anything,
//! and none of these files was produced by this repository's own writers.
//!
//! The bmpsuite files are the other kind of evidence, and the README in that
//! directory says what they can and cannot prove: they are pictures Jason
//! Summers' generator wrote in thirty ways, and what is asserted is that the
//! ways which describe **one** picture decode to one picture. That is a
//! relation between two reads rather than an expected answer, and it is where
//! RLE4, RLE8, 16-bit bit fields and the OS/2 headers are held, because no
//! encoder on this machine writes any of them.

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{
    bmp_decode, gif_decode, tiff_decode, tiff_scan, tiff_scan_directory, webp_decode, BmpError,
    BmpImage, GifError, GifImage, ImagePixels, Limits, TiffColour, TiffCompression, TiffImage,
    TiffLayout, TiffSampleFormat, Warning, WebpError, WebpImage,
};

const CAP: Limits = Limits::new(1 << 24);

/// The pictures `make-images.py` encodes. Keep in step with that script.
mod recipe {
    pub fn rgb(x: u32, y: u32) -> [u8; 3] {
        [
            ((x * 29 + y * 7) % 256) as u8,
            ((x * 3 + y * 41 + 17) % 256) as u8,
            ((x * y + 101) % 256) as u8,
        ]
    }

    pub fn alpha(x: u32, y: u32) -> u8 {
        ((x * 13 + y * 19 + 5) % 256) as u8
    }

    pub fn grey(x: u32, y: u32) -> u8 {
        ((x * 5 + y * 9) % 256) as u8
    }

    pub fn index(x: u32, y: u32, n: u32) -> u32 {
        (x + 3 * y) % n
    }

    pub fn palette(i: u32) -> [u8; 3] {
        [
            ((i * 7) % 256) as u8,
            ((i * 13 + 50) % 256) as u8,
            ((255 - i) % 256) as u8,
        ]
    }

    pub fn bit(x: u32, y: u32) -> bool {
        (x + y) % 3 == 0
    }

    pub fn noise(x: u32, y: u32, k: u32) -> u8 {
        let h =
            (u64::from(x) * 73_856_093) ^ (u64::from(y) * 19_349_663) ^ (u64::from(k) * 83_492_791);
        ((h % (1 << 32)) >> 13 & 255) as u8
    }

    /// Tiles of the recipe repeated (back-references) beside noise (literals).
    pub fn mixed(x: u32, y: u32) -> [u8; 3] {
        if (x / 8 + y / 8) % 2 == 0 {
            rgb(x % 8, y % 8)
        } else {
            [noise(x, y, 1), noise(x, y, 2), noise(x, y, 3)]
        }
    }

    /// Noise constant along each anti-diagonal: every pixel is its top-right
    /// neighbour.
    pub fn diagonal(x: u32, y: u32) -> [u8; 3] {
        [noise(x + y, 0, 1), noise(x + y, 0, 2), noise(x + y, 0, 3)]
    }
}

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("images")
        .join(name)
}

fn read(sub: &str, name: &str) -> Vec<u8> {
    let path = dir(sub).join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every pixel of a decode as RGBA, whatever layout it came back in.
fn colours(width: u32, height: u32, pixels: &ImagePixels) -> Vec<[u8; 4]> {
    (0..(width * height) as usize)
        .map(|i| pixels.rgba_at(i).expect("a pixel for every position"))
        .collect()
}

/// Asserts a decode is exactly `expect(x, y)` everywhere, and says where not.
fn assert_picture(
    label: &str,
    width: u32,
    height: u32,
    pixels: &ImagePixels,
    expect: impl Fn(u32, u32) -> [u8; 4],
) {
    let got = colours(width, height, pixels);
    let mut wrong = 0usize;
    let mut first = None;
    for y in 0..height {
        for x in 0..width {
            let want = expect(x, y);
            let have = got[(y * width + x) as usize];
            if want != have {
                wrong += 1;
                first.get_or_insert((x, y, want, have));
            }
        }
    }
    assert_eq!(
        wrong, 0,
        "{label}: {wrong} pixels differ; first at {first:?} (x, y, expected, decoded)"
    );
}

fn bmp(sub: &str, name: &str) -> BmpImage {
    let img = bmp_decode(&read(sub, name), &CAP).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(
        img.complete,
        "{name} decoded incomplete: {:?}",
        img.warnings
    );
    img
}

// ---- BMP: authored pixels, third-party encoders -------------------------

#[test]
fn pillow_bmps_decode_to_the_pixels_they_were_made_from() {
    let one = bmp("bmp", "pillow-1bit-21x5.bmp");
    assert_eq!((one.width, one.height), (21, 5));
    assert!(matches!(one.pixels, ImagePixels::Indexed { .. }));
    assert_picture("1 bit", 21, 5, &one.pixels, |x, y| {
        let v = if recipe::bit(x, y) { 255 } else { 0 };
        [v, v, v, 255]
    });

    let grey = bmp("bmp", "pillow-grey-13x7.bmp");
    assert!(matches!(grey.pixels, ImagePixels::Indexed { .. }));
    assert_picture("8-bit grey", 13, 7, &grey.pixels, |x, y| {
        let v = recipe::grey(x, y);
        [v, v, v, 255]
    });

    let palette = bmp("bmp", "pillow-palette-13x7.bmp");
    let ImagePixels::Indexed { indices, .. } = &palette.pixels else {
        panic!("an 8-bit BMP stays indexed");
    };
    // The indices themselves, not only the colours they name: Pillow was
    // handed this palette in this order and writes it back unchanged.
    let want: Vec<u8> = (0..7)
        .flat_map(|y| (0..13).map(move |x| recipe::index(x, y, 256) as u8))
        .collect();
    assert_eq!(indices, &want);
    assert_picture("8-bit palette", 13, 7, &palette.pixels, |x, y| {
        let [r, g, b] = recipe::palette(recipe::index(x, y, 256));
        [r, g, b, 255]
    });

    // 13 pixels at three bytes is 39, so every row carries one byte of the
    // four-byte padding.
    let rgb = bmp("bmp", "pillow-rgb-13x7.bmp");
    assert!(matches!(rgb.pixels, ImagePixels::Rgb(_)));
    assert_picture("24-bit", 13, 7, &rgb.pixels, |x, y| {
        let [r, g, b] = recipe::rgb(x, y);
        [r, g, b, 255]
    });
}

/// Pillow writes an RGBA image as 32-bit `BI_RGB` with the alpha in the byte
/// `BITMAPINFOHEADER` says is unused. The colour is the picture, exactly; the
/// alpha is **not read**, which is the decision `bmp.rs`'s module note argues
/// — and this is the file that makes the decision visible, because the
/// recipe's alpha is nowhere near 255.
#[test]
fn a_32_bit_bi_rgb_file_is_its_colour_and_opaque() {
    let img = bmp("bmp", "pillow-rgba-13x7.bmp");
    assert!(matches!(img.pixels, ImagePixels::Rgb(_)));
    assert_picture("32-bit BI_RGB", 13, 7, &img.pixels, |x, y| {
        let [r, g, b] = recipe::rgb(x, y);
        [r, g, b, 255]
    });
}

/// imagecodecs writes RGBA as `BI_BITFIELDS` under a `BITMAPV4HEADER` with an
/// alpha mask — the one way a BMP states opacity — and every sample of all
/// four channels comes back.
#[test]
fn an_alpha_mask_is_read_exactly() {
    let img = bmp("bmp", "imagecodecs-rgba-13x7.bmp");
    assert!(matches!(img.pixels, ImagePixels::Rgba(_)));
    assert_picture("V4 bit fields", 13, 7, &img.pixels, |x, y| {
        let [r, g, b] = recipe::rgb(x, y);
        [r, g, b, recipe::alpha(x, y)]
    });
}

// ---- BMP: bmpsuite, as relations -------------------------------------------

fn suite(name: &str) -> BmpImage {
    bmp("bmpsuite", name)
}

fn same_picture(a: &str, b: &str) {
    let (x, y) = (suite(a), suite(b));
    assert_eq!(
        (x.width, x.height),
        (y.width, y.height),
        "{a} and {b}: size"
    );
    assert_eq!(
        colours(x.width, x.height, &x.pixels),
        colours(y.width, y.height, &y.pixels),
        "{a} and {b} are one picture and decoded to two"
    );
}

/// RLE8 and RLE4 are held to the uncompressed file of the same picture, and
/// the uncompressed 8-bit path is held to Pillow's authored palette above —
/// so the chain ends at an expected answer nobody decoded.
#[test]
fn bmpsuite_rle_files_are_their_uncompressed_twins() {
    same_picture("g-pal8.bmp", "g-pal8rle.bmp");
    same_picture("g-pal4.bmp", "g-pal4rle.bmp");
}

#[test]
fn bmpsuite_headers_and_orders_that_describe_one_picture_agree() {
    for other in [
        "g-pal8topdown.bmp",
        "g-pal8os2.bmp",
        "g-pal8v5.bmp",
        "q-pal8os2v2.bmp",
        "q-pal8oversizepal.bmp",
    ] {
        same_picture("g-pal8.bmp", other);
    }
    // 8-8-8 at three and at four bytes, the latter with default and with
    // shuffled bit-field masks, and with garbage in the unused byte — which
    // is the file that says the fourth byte of `BI_RGB` is not alpha.
    for other in ["g-rgb32.bmp", "g-rgb32bf.bmp", "q-rgb32fakealpha.bmp"] {
        same_picture("g-rgb24.bmp", other);
    }
    same_picture("g-rgb16.bmp", "g-rgb16bfdef.bmp");
    // Three alpha files: masks in the usual order, in an unusual one, and
    // `BI_ALPHABITFIELDS`.
    same_picture("q-rgba32-1.bmp", "q-rgba32-2.bmp");
    same_picture("q-rgba32-1.bmp", "q-rgba32abf.bmp");
    assert!(matches!(
        suite("q-rgba32-1.bmp").pixels,
        ImagePixels::Rgba(_)
    ));
}

/// Every depth the suite's good set carries, decoded whole: 1, 2 (Windows CE),
/// 4, 8 at a width whose padding is two bytes, 5-6-5, and the rest above.
#[test]
fn bmpsuite_depths_decode_whole() {
    for name in [
        "g-pal1.bmp",
        "q-pal2.bmp",
        "g-pal8w126.bmp",
        "g-rgb16-565.bmp",
    ] {
        let img = suite(name);
        assert!(img.width > 0 && img.height > 0);
        assert!(img.warnings.is_empty(), "{name}: {:?}", img.warnings);
    }
    // 5-6-5 is not 5-5-5, and a decoder that ignored the masks would make
    // them agree.
    let (a, b) = (suite("g-rgb16.bmp"), suite("g-rgb16-565.bmp"));
    assert_ne!(
        colours(a.width, a.height, &a.pixels),
        colours(b.width, b.height, &b.pixels)
    );
}

/// An RLE delta leaves pixels undefined. What the suite calls "questionable"
/// is held to two things: every pixel it *does* define is its uncompressed
/// twin's, and every pixel it does not is palette entry 0 with the warning
/// that says so.
#[test]
fn bmpsuite_rle_deltas_define_what_they_define_and_warn_about_the_rest() {
    for (rle, plain) in [
        ("q-pal8rletrns.bmp", "g-pal8.bmp"),
        ("q-pal4rletrns.bmp", "g-pal4.bmp"),
    ] {
        let img = bmp_decode(&read("bmpsuite", rle), &CAP).expect("decodes");
        assert!(img.warnings.contains(&Warning::BmpRleUndefinedPixels));
        let twin = suite(plain);
        let ImagePixels::Indexed { palette, .. } = &img.pixels else {
            panic!("indexed");
        };
        let zero = [palette[0], palette[1], palette[2], 255];
        let (got, want) = (
            colours(img.width, img.height, &img.pixels),
            colours(twin.width, twin.height, &twin.pixels),
        );
        let undefined = got
            .iter()
            .zip(&want)
            .filter(|(g, w)| g != w)
            .inspect(|(g, _)| assert_eq!(**g, zero, "{rle}: a defined pixel differs"))
            .count();
        assert!(undefined > 0, "{rle}: the delta skipped nothing");
    }
}

#[test]
fn a_file_that_is_not_a_bitmap_is_refused_by_name() {
    assert_eq!(
        bmp_decode(&read("bmp", "../make-images.py"), &CAP),
        Err(BmpError::NotBmp)
    );
}

// ---- GIF: authored pixels, two third-party encoders --------------------------

fn gif(name: &str) -> GifImage {
    let img = gif_decode(&read("gif", name), &CAP).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(
        img.complete,
        "{name} decoded incomplete: {:?}",
        img.warnings
    );
    img
}

/// The first image descriptor's packed byte and the LZW minimum code size
/// after it, read straight out of the file — so a test can say which feature
/// of the format its fixture actually exercises, rather than trusting the
/// encoder's documentation for it.
fn first_descriptor(bytes: &[u8]) -> (u8, u8) {
    let table = |packed: u8| {
        if packed & 0x80 != 0 {
            3usize << ((packed & 7) + 1)
        } else {
            0
        }
    };
    let mut at = 13 + table(bytes[10]);
    loop {
        match bytes[at] {
            0x21 => {
                at += 2;
                while bytes[at] != 0 {
                    at += 1 + bytes[at] as usize;
                }
                at += 1;
            }
            0x2C => {
                let packed = bytes[at + 9];
                return (packed, bytes[at + 10 + table(packed)]);
            }
            other => panic!("block {other:#x} before any image"),
        }
    }
}

fn recipe_colour(x: u32, y: u32) -> [u8; 4] {
    let [r, g, b] = recipe::palette(recipe::index(x, y, 256));
    [r, g, b, 255]
}

#[test]
fn pillow_gifs_decode_to_the_pixels_they_were_made_from() {
    let img = gif("pillow-palette-13x7.gif");
    assert!(matches!(
        img.pixels,
        ImagePixels::Indexed {
            transparent: None,
            ..
        }
    ));
    assert_picture("palette", 13, 7, &img.pixels, recipe_colour);

    let grey = gif("pillow-grey-13x7.gif");
    assert_picture("grey", 13, 7, &grey.pixels, |x, y| {
        let v = recipe::grey(x, y);
        [v, v, v, 255]
    });

    // Pillow codes with 256 roots whatever the palette; the omggif files
    // below are where the root size moves.
    assert_eq!(
        first_descriptor(&read("gif", "pillow-palette-13x7.gif")).1,
        8
    );
}

/// Interlaced: the fixture's own descriptor says so, and the rows come back in
/// raster order.
#[test]
fn an_interlaced_gif_is_put_back_in_row_order() {
    let bytes = read("gif", "pillow-interlaced-40x24.gif");
    assert_ne!(
        first_descriptor(&bytes).0 & 0x40,
        0,
        "the fixture is interlaced"
    );
    let img = gif("pillow-interlaced-40x24.gif");
    assert_eq!((img.width, img.height), (40, 24));
    assert_picture("interlaced", 40, 24, &img.pixels, recipe_colour);
}

/// A local colour table on the first image, per the descriptor's own flag.
#[test]
fn a_local_table_is_the_table_the_image_is_read_against() {
    let bytes = read("gif", "pillow-local-table-13x7.gif");
    assert_ne!(
        first_descriptor(&bytes).0 & 0x80,
        0,
        "the fixture carries a local table"
    );
    let img = gif("pillow-local-table-13x7.gif");
    assert_picture("local table", 13, 7, &img.pixels, recipe_colour);
}

/// The graphic control extension's transparent index stays an index: every
/// pixel the recipe put index 5 on is transparent and every other pixel is
/// its colour.
#[test]
fn a_transparent_index_is_carried_and_nothing_else_is_transparent() {
    let img = gif("pillow-transparent-13x7.gif");
    assert!(matches!(
        img.pixels,
        ImagePixels::Indexed {
            transparent: Some(_),
            ..
        }
    ));
    let clear = (0..7)
        .flat_map(|y| (0..13).map(move |x| (x, y)))
        .filter(|&(x, y)| recipe::index(x, y, 256) == 5)
        .count();
    assert!(clear > 0, "the recipe puts index 5 somewhere");
    assert_picture("transparent", 13, 7, &img.pixels, |x, y| {
        let colour = recipe_colour(x, y);
        if recipe::index(x, y, 256) == 5 {
            [colour[0], colour[1], colour[2], 0]
        } else {
            colour
        }
    });
}

/// An animation is its first frame, and says it has others.
#[test]
fn an_animated_gif_is_its_first_frame() {
    let img = gif("pillow-animated-13x7.gif");
    assert!(img.warnings.contains(&Warning::GifFramesIgnored));
    assert_picture("first frame", 13, 7, &img.pixels, recipe_colour);
}

/// omggif sizes the LZW root set from the palette: two bits for four colours,
/// two (its floor) for two, four for sixteen. Each is the recipe exactly.
#[test]
fn omggif_root_sizes_two_and_four_decode_exactly() {
    for (name, n, w, h, code_size) in [
        ("omggif-4colour-13x7.gif", 4, 13, 7, 2),
        ("omggif-2colour-21x5.gif", 2, 21, 5, 2),
        ("omggif-16colour-21x9.gif", 16, 21, 9, 4),
    ] {
        assert_eq!(first_descriptor(&read("gif", name)).1, code_size, "{name}");
        let img = gif(name);
        assert_eq!((img.width, img.height), (w, h));
        assert_picture(name, w, h, &img.pixels, |x, y| {
            let [r, g, b] = recipe::palette(recipe::index(x, y, n));
            [r, g, b, 255]
        });
    }
}

/// A first image smaller than its screen, on the global table: the uncovered
/// pixels are §18's background colour and the picture stays indexed.
#[test]
fn an_uncovered_screen_is_the_background_colour() {
    let img = gif("omggif-global-offset-13x7.gif");
    assert!(matches!(img.pixels, ImagePixels::Indexed { .. }));
    assert_picture("global offset", 13, 7, &img.pixels, |x, y| {
        let i = if (2..10).contains(&x) && (1..6).contains(&y) {
            recipe::index(x - 2, y - 1, 16)
        } else {
            7
        };
        let [r, g, b] = recipe::palette(i);
        [r, g, b, 255]
    });
}

/// The same, with a local table and a transparent index: two index spaces on
/// one canvas, so the picture is expanded to RGBA — the background from the
/// global table, the image from its own, index 2 transparent.
#[test]
fn a_local_table_on_part_of_the_screen_is_expanded() {
    let img = gif("omggif-local-offset-13x7.gif");
    assert!(matches!(img.pixels, ImagePixels::Rgba(_)));
    assert_picture("local offset", 13, 7, &img.pixels, |x, y| {
        if (3..9).contains(&x) && (2..6).contains(&y) {
            let i = recipe::index(x - 3, y - 2, 8);
            if i == 2 {
                return [0, 0, 0, 0];
            }
            let [r, g, b] = recipe::palette(i + 100);
            [r, g, b, 255]
        } else {
            let [r, g, b] = recipe::palette(1);
            [r, g, b, 255]
        }
    });
}

#[test]
fn a_file_that_is_not_a_gif_is_refused_by_name() {
    assert_eq!(
        gif_decode(&read("bmp", "pillow-rgb-13x7.bmp"), &CAP),
        Err(GifError::NotGif)
    );
}

// ---- TIFF: the archive row's additions, from tifffile ------------------------

fn tiff(name: &str) -> TiffImage {
    let img = tiff_decode(&read("tiff", name), &CAP).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(
        img.complete,
        "{name} decoded incomplete: {:?}",
        img.warnings
    );
    img
}

/// Sixteen-bit samples as numbers.
fn words(data: &[u8]) -> Vec<u16> {
    data.chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect()
}

fn each(width: u32, height: u32, f: impl Fn(u32, u32) -> Vec<u8>) -> Vec<u8> {
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y))
        .collect()
}

/// `PhotometricInterpretation` 5: four ink amounts, handed back as the file
/// holds them — cyan, magenta and yellow the RGB recipe, black the alpha one.
#[test]
fn a_cmyk_tiff_is_its_ink_amounts() {
    for name in ["tifffile-cmyk-13x7.tif", "tifffile-cmyk-deflate-13x7.tif"] {
        let img = tiff(name);
        assert_eq!(img.colour, TiffColour::Cmyk, "{name}");
        let want = each(13, 7, |x, y| {
            let [c, m, ye] = recipe::rgb(x, y);
            vec![c, m, ye, recipe::alpha(x, y)]
        });
        assert_eq!(img.data, want, "{name}");
    }
}

/// `SampleFormat` 2 at 8, 16 and 32 bits (the last with `Predictor` 2), each
/// mapped by §19's default range — the full range of its type — onto the
/// output: exactly the offset by half the range at 8 and 16, and at 32 the
/// same line rounded onto sixteen bits.
#[test]
fn signed_tiffs_are_offset_by_half_their_range() {
    let file = read("tiff", "tifffile-int8-13x7.tif");
    let scan = tiff_scan(&file).expect("scans");
    assert_eq!(scan.sample_format, TiffSampleFormat::Signed);
    let img = tiff("tifffile-int8-13x7.tif");
    assert_eq!(img.bits_per_component, 8);
    assert_eq!(img.data, each(13, 7, |x, y| vec![recipe::grey(x, y)]));

    let img = tiff("tifffile-int16-13x7.tif");
    assert_eq!(img.bits_per_component, 16);
    let want: Vec<u16> = (0..7u32)
        .flat_map(|y| (0..13u32).map(move |x| ((x * 1000 + y * 7777) % 65536) as u16))
        .collect();
    assert_eq!(words(&img.data), want);

    let file = read("tiff", "tifffile-int32-predictor-13x7.tif");

    let scan = tiff_scan(&file).expect("scans");
    assert_eq!((scan.bits_per_sample, scan.predictor), (32, 2));
    let img = tiff("tifffile-int32-predictor-13x7.tif");
    let top = (1u128 << 32) - 1;
    let want: Vec<u16> = (0..7u64)
        .flat_map(|y| {
            (0..13u64).map(move |x| {
                // The authored value plus 2^31, which is the recipe before
                // the generator subtracted it.
                let offset = u128::from((x * 123_456_789 + y * 987_654_321) % (1 << 32));
                ((offset * 65_535 + top / 2) / top) as u16
            })
        })
        .collect();
    assert_eq!(words(&img.data), want);
}

/// `SampleFormat` 3 at 16, 32 and 64 bits — the 32-bit file under Technical
/// Note 3's `Predictor` 3 — read as the intensity itself on [0, 1], clamped,
/// at sixteen bits. The recipe is whole 128ths, which every width holds
/// exactly, so the expected number is the recipe and nothing else.
#[test]
fn float_tiffs_are_their_own_intensities_clamped() {
    let want: Vec<u16> = (0..7u32)
        .flat_map(|y| {
            (0..13u32).map(move |x| {
                let v = (3.0 * f64::from(recipe::grey(x, y)) - 64.0) / 128.0;
                (v.clamp(0.0, 1.0) * 65_535.0 + 0.5).floor() as u16
            })
        })
        .collect();
    assert!(
        want.contains(&0) && want.contains(&65_535),
        "the recipe reaches both clamps"
    );
    let file = read("tiff", "tifffile-float32-predictor3-13x7.tif");
    let scan = tiff_scan(&file).expect("scans");
    assert_eq!(
        (scan.sample_format, scan.predictor),
        (TiffSampleFormat::Float, 3)
    );
    for (name, bits) in [
        ("tifffile-float16-13x7.tif", 16),
        ("tifffile-float32-predictor3-13x7.tif", 32),
        ("tifffile-float64-13x7.tif", 64),
    ] {
        let file = read("tiff", name);
        let scan = tiff_scan(&file).expect("scans");
        assert_eq!(scan.bits_per_sample, bits, "{name}");
        let img = tiff(name);
        assert_eq!(img.bits_per_component, 16, "{name}");
        assert_eq!(words(&img.data), want, "{name}");
    }
}

/// BigTIFF in both byte orders: the header says so, and the picture is the
/// RGB recipe.
#[test]
fn a_bigtiff_is_read_in_both_byte_orders() {
    for name in [
        "tifffile-bigtiff-rgb-13x7.tif",
        "tifffile-bigtiff-mm-rgb-13x7.tif",
    ] {
        let bytes = read("tiff", name);
        assert!(
            bytes.starts_with(b"II\x2b\x00\x08\x00\x00\x00")
                || bytes.starts_with(b"MM\x00\x2b\x00\x08\x00\x00"),
            "{name} is a BigTIFF"
        );
        let img = tiff(name);
        assert_eq!(
            img.data,
            each(13, 7, |x, y| recipe::rgb(x, y).to_vec()),
            "{name}"
        );
    }
}

/// `Compression` 34712, one strip and a 16 x 16 tile grid with padded edges,
/// both lossless: the RGB recipe exactly.
#[test]
fn jpeg_2000_tiffs_decode_to_the_recipe() {
    let file = read("tiff", "tifffile-jpeg2000-rgb-13x7.tif");
    let scan = tiff_scan(&file).expect("scans");
    assert_eq!(scan.compression, TiffCompression::Jpeg2000);
    let img = tiff("tifffile-jpeg2000-rgb-13x7.tif");
    assert_eq!(img.data, each(13, 7, |x, y| recipe::rgb(x, y).to_vec()));

    let file = read("tiff", "tifffile-jpeg2000-tiled-40x24.tif");

    let scan = tiff_scan(&file).expect("scans");
    assert_eq!(
        scan.layout,
        TiffLayout::Tiles {
            width: 16,
            height: 16
        }
    );
    let img = tiff("tifffile-jpeg2000-tiled-40x24.tif");
    assert_eq!(img.data, each(40, 24, |x, y| recipe::rgb(x, y).to_vec()));
}

/// Four directories: every one scans by index, the third says it is a
/// reduced-resolution copy, and each decodes to what was written into it.
#[test]
fn every_directory_of_a_multipage_tiff_is_its_own_picture() {
    let bytes = read("tiff", "tifffile-multipage.tif");
    let scans: Vec<_> = (0..4)
        .map(|i| tiff_scan_directory(&bytes, i).unwrap_or_else(|e| panic!("directory {i}: {e}")))
        .collect();
    assert!(tiff_scan_directory(&bytes, 4).is_err());
    assert!(scans.iter().all(|s| s.pages == 4));
    assert_eq!(
        scans.iter().map(|s| s.subfile).collect::<Vec<_>>(),
        [0, 0, 1, 0]
    );
    let grey = each(13, 7, |x, y| vec![recipe::grey(x, y)]);
    let inverted: Vec<u8> = grey.iter().map(|v| 255 - v).collect();
    assert_eq!(scans[0].decode(&CAP).expect("decodes").data, grey);
    assert_eq!(
        scans[1].decode(&CAP).expect("decodes").data,
        each(13, 7, |x, y| recipe::rgb(x, y).to_vec())
    );
    assert_eq!(
        scans[2].decode(&CAP).expect("decodes").data,
        each(7, 4, |x, y| vec![recipe::grey(x * 2, y * 2)])
    );
    assert_eq!(scans[3].decode(&CAP).expect("decodes").data, inverted);
}

// ---- WebP lossless: authored pixels, libwebp through two bindings -------------

fn webp(name: &str) -> WebpImage {
    let img = webp_decode(&read("webp", name), &CAP).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(
        img.complete,
        "{name} decoded incomplete: {:?}",
        img.warnings
    );
    img
}

/// The leading VP8L transforms a file uses, read from its own header bits —
/// RFC 9649 §3.5 — so a test can say what its fixture exercises.
fn vp8l_transforms(bytes: &[u8]) -> Vec<u32> {
    let at = bytes
        .windows(4)
        .position(|w| w == b"VP8L")
        .expect("a VP8L chunk");
    let stream = &bytes[at + 8..];
    assert_eq!(stream[0], 0x2f);
    let mut bit = 8u32 + 14 + 14 + 1 + 3;
    let read = |bit: &mut u32, n: u32| {
        let mut v = 0u32;
        for i in 0..n {
            let b = *bit + i;
            v |= u32::from((stream[(b / 8) as usize] >> (b % 8)) & 1) << i;
        }
        *bit += n;
        v
    };
    let mut kinds = Vec::new();
    // A transform's type is readable without decoding anything until the
    // first one that carries data; subtract-green (2) carries none, so the
    // walk goes on past it and stops at any other.
    while read(&mut bit, 1) == 1 {
        let kind = read(&mut bit, 2);
        kinds.push(kind);
        if kind != 2 {
            break;
        }
    }
    kinds
}

#[test]
fn lossless_webps_decode_to_the_pixels_they_were_made_from() {
    let img = webp("pillow-lossless-rgb-13x7.webp");
    assert!(
        matches!(img.pixels, ImagePixels::Rgb(_)),
        "opaque comes back RGB"
    );
    assert_picture("RGB", 13, 7, &img.pixels, |x, y| {
        let [r, g, b] = recipe::rgb(x, y);
        [r, g, b, 255]
    });
    for name in [
        "pillow-lossless-rgba-13x7.webp",
        "imagecodecs-lossless-rgba-13x7.webp",
    ] {
        let img = webp(name);
        assert!(matches!(img.pixels, ImagePixels::Rgba(_)), "{name}");
        assert_picture(name, 13, 7, &img.pixels, |x, y| {
            let [r, g, b] = recipe::rgb(x, y);
            [r, g, b, recipe::alpha(x, y)]
        });
    }
}

/// libwebp's method 6 at quality 100 tries every transform; the header says
/// the fixture carries at least one, and the picture is still the recipe.
#[test]
fn every_transform_libwebp_chose_is_undone_exactly() {
    for (name, alpha) in [
        ("pillow-lossless-m6-96x64.webp", false),
        ("pillow-lossless-rgba-m6-96x64.webp", true),
    ] {
        let bytes = read("webp", name);
        assert!(
            !vp8l_transforms(&bytes).is_empty(),
            "{name} uses a transform"
        );
        let img = webp(name);
        assert_eq!((img.width, img.height), (96, 64));
        assert_picture(name, 96, 64, &img.pixels, |x, y| {
            let [r, g, b] = recipe::rgb(x, y);
            [r, g, b, if alpha { recipe::alpha(x, y) } else { 255 }]
        });
    }
}

/// Repeated tiles beside noise: back-references, the colour cache and
/// literals all carry pixels here, and every one lands. Counted when the file
/// was committed, by a decoder instrumented for the purpose: a 10-bit colour
/// cache, three prefix-code groups chosen block by block by a meta prefix
/// image, and 2 416 literals, 1 371 back-references and 4 304 cache hits.
#[test]
fn back_references_and_the_colour_cache_reproduce_the_picture() {
    let img = webp("pillow-lossless-mixed-160x96.webp");
    assert_picture("mixed", 160, 96, &img.pixels, |x, y| {
        let [r, g, b] = recipe::mixed(x, y);
        [r, g, b, recipe::alpha(x, y) | 1]
    });
}

/// Every pixel is its top-right neighbour, so libwebp chooses the top-right
/// predictor (mode 3) — and, counted when the file was committed, chooses it
/// in the last column on all 31 rows that are predicted, where §3.5.1 makes
/// the top-right pixel "the leftmost pixel on the current row" rather than
/// anything above. A decoder that reached up there instead is wrong on every
/// row of the right edge.
#[test]
fn the_top_right_of_the_last_column_is_the_first_pixel_of_its_own_row() {
    let name = "pillow-lossless-diagonal-64x32.webp";
    assert!(
        vp8l_transforms(&read("webp", name)).contains(&0),
        "{name} predicts"
    );
    let img = webp(name);
    assert_picture(name, 64, 32, &img.pixels, |x, y| {
        let [r, g, b] = recipe::diagonal(x, y);
        [r, g, b, 255]
    });
}

/// Two, four and sixteen colours: the colour-indexing transform at each of
/// its bundling widths — the header says transform 3 is there.
#[test]
fn the_colour_indexing_transform_unbundles_at_every_width() {
    for (n, w, h) in [(2u32, 21u32, 5u32), (4, 13, 7), (16, 21, 9)] {
        let name = format!("pillow-lossless-{n}colour-{w}x{h}.webp");
        let bytes = read("webp", &name);
        assert_eq!(
            vp8l_transforms(&bytes).first(),
            Some(&3),
            "{name} indexes colours"
        );
        let img = webp(&name);
        assert_picture(&name, w, h, &img.pixels, |x, y| {
            let [r, g, b] = recipe::palette(recipe::index(x, y, n));
            [r, g, b, 255]
        });
    }
}

/// An animation is its first frame on the `VP8X` canvas, and says so.
#[test]
fn an_animated_webp_is_its_first_frame() {
    let bytes = read("webp", "pillow-animated-lossless-13x7.webp");
    assert!(
        bytes.windows(4).any(|w| w == b"ANMF"),
        "the fixture animates"
    );
    let img = webp("pillow-animated-lossless-13x7.webp");
    assert!(img.warnings.contains(&Warning::WebpFramesIgnored));
    assert_picture("first frame", 13, 7, &img.pixels, |x, y| {
        let [r, g, b] = recipe::rgb(x, y);
        [r, g, b, 255]
    });
}

#[test]
fn a_file_that_is_not_a_webp_is_refused_by_name() {
    assert_eq!(
        webp_decode(&read("gif", "pillow-palette-13x7.gif"), &CAP),
        Err(WebpError::NotWebp)
    );
}
