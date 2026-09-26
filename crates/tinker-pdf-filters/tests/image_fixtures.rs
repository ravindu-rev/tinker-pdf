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
    bmp_decode, gif_decode, BmpError, BmpImage, GifError, GifImage, ImagePixels, Limits, Warning,
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
