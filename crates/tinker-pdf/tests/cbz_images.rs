//! Comic pages in the formats that have no pass-through route — BMP and GIF —
//! and the TIFF shapes the archive row added: CMYK, signed and floating-point
//! samples, JPEG 2000 strips and directories after the first.
//!
//! `tinker-pdf-filters/tests/image_fixtures.rs` holds each decoder to the
//! pixels a third-party encoder was handed. This file holds the **page** to
//! them: the same committed files, packed into an archive by this repository's
//! own ZIP writer (`cbz_support`), opened through `Document::open`, and read
//! back two ways — the image XObject's own samples and colour space out of
//! the synthesised document, and the rendered page. The expected answer is
//! the recipe `make-images.py` encoded, recomputed here; nothing outside this
//! repository decodes anything.

mod cbz_support;

use std::path::Path;

use cbz_support::{zip, Damage, ZipFile};
use tinker_pdf::cbz::{self, Limits};
use tinker_pdf::{ArchiveRefusal, Bitmap, Container, Document, PageDefect, RenderOptions};
use tinker_pdf_cos::{ObjRef, Object};

/// The pictures `tinker-pdf-filters/tests/images/make-images.py` encodes.
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
}

fn fixture(path: &str) -> Vec<u8> {
    let full = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf-filters/tests/images")
        .join(path);
    std::fs::read(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

fn open(files: &[(&str, Vec<u8>)]) -> Document {
    let entries: Vec<ZipFile> = files
        .iter()
        .map(|(name, data)| ZipFile::stored(name, data))
        .collect();
    Document::open(zip(&entries, Damage::None)).expect("the archive opens")
}

fn render(document: &Document, page: u32) -> Bitmap {
    document
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn rendered_rgb(bitmap: &Bitmap, x: u32, y: u32) -> [u8; 3] {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = &bitmap.data[at..at + 3];
    [p[0], p[1], p[2]]
}

/// The one image XObject a synthesised page carries, as its reference.
fn page_image(document: &Document, page: usize) -> ObjRef {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let resources = pages[page].resources.as_ref().expect("/Resources");
    let xobjects = cos.resolve_key(resources, cos.intern(b"XObject"));
    let xobjects = xobjects.as_dict().expect("/XObject");
    let (_, object) = xobjects.iter().next().expect("one image");
    match object {
        Object::Ref(r) => *r,
        other => panic!("an indirect image, got {other:?}"),
    }
}

/// Every page is the picture and none is a placeholder.
fn assert_no_placeholders(document: &Document) {
    let report = document.archive().expect("a report");
    for origin in report.pages() {
        assert_eq!(origin.defect, None, "{} is a placeholder", origin.name);
    }
}

#[test]
fn bmp_pages_are_the_pictures_they_were_made_from() {
    let document = open(&[
        ("p1.bmp", fixture("bmp/pillow-palette-13x7.bmp")),
        ("p2.bmp", fixture("bmp/pillow-rgb-13x7.bmp")),
        ("p3.bmp", fixture("bmp/imagecodecs-rgba-13x7.bmp")),
        ("p4.bmp", fixture("bmpsuite/g-pal8rle.bmp")),
    ]);
    assert_eq!(document.page_count(), 4);
    assert_no_placeholders(&document);
    for page in 0..3 {
        assert_eq!(document.page(page).expect("a page").size(), (13.0, 7.0));
    }
    assert_eq!(document.page(3).expect("a page").size(), (127.0, 64.0));

    // Page one is kept indexed: `/Indexed` over the file's own palette, one
    // byte a pixel, and those bytes are the indices Pillow was handed.
    let cos = document.cos();
    let image = page_image(&document, 0);
    let dict = cos.get(image).expect("the image");
    let dict = &dict.as_stream().expect("a stream").dict;
    let space = cos.resolve_key(dict, cos.intern(b"ColorSpace"));
    let space = space.as_array().expect("an /Indexed array");
    assert_eq!(space[0].as_name(), Some(cos.intern(b"Indexed")));
    let samples = cos.stream_decoded(image).expect("decodes");
    let want: Vec<u8> = (0..7)
        .flat_map(|y| (0..13).map(move |x| recipe::index(x, y, 256) as u8))
        .collect();
    assert_eq!(samples, want);

    // And the rendered page is the recipe, pixel for pixel, on both opaque
    // pages.
    for (page, expect) in [
        (
            0u32,
            &(|x, y| recipe::palette(recipe::index(x, y, 256))) as &dyn Fn(u32, u32) -> [u8; 3],
        ),
        (1, &recipe::rgb),
    ] {
        let bitmap = render(&document, page);
        assert_eq!((bitmap.width, bitmap.height), (13, 7));
        for y in 0..7 {
            for x in 0..13 {
                assert_eq!(
                    rendered_rgb(&bitmap, x, y),
                    expect(x, y),
                    "page {page} ({x}, {y})"
                );
            }
        }
    }

    // Page three's alpha is an `/SMask` holding exactly the recipe's alpha,
    // over colour that is exactly the recipe's colour.
    let image = page_image(&document, 2);
    let colour = cos.stream_decoded(image).expect("decodes");
    let want: Vec<u8> = (0..7)
        .flat_map(|y| (0..13).flat_map(move |x| recipe::rgb(x, y)))
        .collect();
    assert_eq!(colour, want);
    let object = cos.get(image).expect("the image");
    let dict = &object.as_stream().expect("a stream").dict;
    let Some(Object::Ref(mask)) = dict.get(cos.intern(b"SMask")) else {
        panic!("an /SMask reference");
    };
    let alpha = cos.stream_decoded(*mask).expect("decodes");
    let want: Vec<u8> = (0..7)
        .flat_map(|y| (0..13).map(move |x| recipe::alpha(x, y)))
        .collect();
    assert_eq!(alpha, want);
}

/// A BMP the decoder refuses is still a page, and it says why.
#[test]
fn a_bmp_that_will_not_decode_keeps_its_page_number() {
    let mut scrgb = fixture("bmp/pillow-rgb-13x7.bmp");
    // biBitCount 64: scRGB, refused by name.
    scrgb[28..30].copy_from_slice(&64u16.to_le_bytes());
    let document = open(&[
        ("p1.bmp", fixture("bmp/pillow-rgb-13x7.bmp")),
        ("p2.bmp", scrgb),
    ]);
    assert_eq!(document.page_count(), 2);
    let report = document.archive().expect("a report");
    assert_eq!(report.pages()[0].defect, None);
    assert_eq!(report.pages()[1].defect, Some(PageDefect::Undecodable));
    assert_eq!(document.page(1).expect("a page").size(), (13.0, 7.0));
}

/// The dictionary of a page's image.
fn image_dict(document: &Document, page: usize) -> tinker_pdf_cos::Dict {
    let cos = document.cos();
    let object = cos.get(page_image(document, page)).expect("the image");
    object.as_stream().expect("a stream").dict.clone()
}

#[test]
fn gif_pages_are_the_pictures_they_were_made_from() {
    let document = open(&[
        ("p1.gif", fixture("gif/pillow-palette-13x7.gif")),
        ("p2.gif", fixture("gif/pillow-interlaced-40x24.gif")),
        ("p3.gif", fixture("gif/pillow-transparent-13x7.gif")),
        ("p4.gif", fixture("gif/omggif-local-offset-13x7.gif")),
        ("p5.gif", fixture("gif/pillow-animated-13x7.gif")),
    ]);
    assert_eq!(document.page_count(), 5);
    assert_no_placeholders(&document);
    let cos = document.cos();

    // Opaque pages render to the recipe pixel for pixel, the interlaced one
    // and the animation's first frame included.
    let palette_colour = |x: u32, y: u32| recipe::palette(recipe::index(x, y, 256));
    for (page, (w, h)) in [(0u32, (13, 7)), (1, (40, 24)), (4, (13, 7))] {
        let bitmap = render(&document, page);
        assert_eq!((bitmap.width, bitmap.height), (w, h), "page {page}");
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    rendered_rgb(&bitmap, x, y),
                    palette_colour(x, y),
                    "page {page} ({x}, {y})"
                );
            }
        }
    }
    // Page three stays indexed and its transparent index is a colour key: a
    // `/Mask` of one range, one index wide, and no `/SMask`.
    let dict = image_dict(&document, 2);
    let space = cos.resolve_key(&dict, cos.intern(b"ColorSpace"));
    assert_eq!(
        space.as_array().expect("/Indexed")[0].as_name(),
        Some(cos.intern(b"Indexed"))
    );
    let mask = cos.resolve_key(&dict, cos.intern(b"Mask"));
    let mask = mask.as_array().expect("a colour-key /Mask");
    assert_eq!(mask.len(), 2);
    assert_eq!(mask[0].as_int(), mask[1].as_int());
    assert!(dict.get(cos.intern(b"SMask")).is_none());
    // And the transparent pixels show the page through them.
    let bitmap = render(&document, 2);
    for y in 0..7 {
        for x in 0..13 {
            let want = if recipe::index(x, y, 256) == 5 {
                [255, 255, 255]
            } else {
                palette_colour(x, y)
            };
            assert_eq!(rendered_rgb(&bitmap, x, y), want, "({x}, {y})");
        }
    }

    // Page four had a local table on part of its screen, so it arrives as RGB
    // over an `/SMask` holding exactly which pixels were transparent.
    let dict = image_dict(&document, 3);
    let Some(Object::Ref(smask)) = dict.get(cos.intern(b"SMask")) else {
        panic!("an /SMask reference");
    };
    let alpha = cos.stream_decoded(*smask).expect("decodes");
    let want: Vec<u8> = (0..7u32)
        .flat_map(|y| {
            (0..13u32).map(move |x| {
                let inside = (3..9).contains(&x) && (2..6).contains(&y);
                if inside && recipe::index(x - 3, y - 2, 8) == 2 {
                    0
                } else {
                    255
                }
            })
        })
        .collect();
    assert_eq!(alpha, want);
}

/// The name a page's image XObject gives its `/Filter`.
fn filter_of(document: &Document, page: usize) -> Option<String> {
    let cos = document.cos();
    let dict = image_dict(document, page);
    let filter = cos.resolve_key(&dict, cos.intern(b"Filter"));
    let name = filter.as_name()?;
    Some(String::from_utf8_lossy(&cos.name_bytes(name)?).into_owned())
}

/// The name of a page's image `/ColorSpace`, when it is a name.
fn space_of(document: &Document, page: usize) -> Option<String> {
    let cos = document.cos();
    let dict = image_dict(document, page);
    let space = cos.resolve_key(&dict, cos.intern(b"ColorSpace"));
    let name = space.as_name()?;
    Some(String::from_utf8_lossy(&cos.name_bytes(name)?).into_owned())
}

/// A multi-page TIFF is one page per directory that is a page: grey, RGB and
/// the grey inverted, with the reduced-resolution copy tifffile was told to
/// write between them skipped. Every page carries the entry's own name, in
/// directory order.
#[test]
fn a_multipage_tiff_is_a_page_per_directory() {
    let document = open(&[
        ("a.tif", fixture("tiff/tifffile-multipage.tif")),
        // A page after it, which must keep its place after three.
        ("b.gif", fixture("gif/pillow-palette-13x7.gif")),
    ]);
    assert_eq!(
        document.page_count(),
        4,
        "three directories that are pages, then b"
    );
    assert_no_placeholders(&document);
    let names: Vec<String> = document
        .archive()
        .expect("a report")
        .pages()
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, ["a.tif", "a.tif", "a.tif", "b.gif"]);

    let grey = |x: u32, y: u32| {
        let v = recipe::grey(x, y);
        [v, v, v]
    };
    let inverted = |x: u32, y: u32| {
        let v = 255 - recipe::grey(x, y);
        [v, v, v]
    };
    for (page, expect) in [
        (0u32, &grey as &dyn Fn(u32, u32) -> [u8; 3]),
        (1, &recipe::rgb),
        (2, &inverted),
    ] {
        let bitmap = render(&document, page);
        assert_eq!((bitmap.width, bitmap.height), (13, 7), "page {page}");
        for y in 0..7 {
            for x in 0..13 {
                assert_eq!(
                    rendered_rgb(&bitmap, x, y),
                    expect(x, y),
                    "page {page} ({x}, {y})"
                );
            }
        }
    }
}

/// Every directory is a page the caps count, so one entry cannot page past
/// `max_pages`: three directories under a cap of two is the cap's own refusal.
#[test]
fn a_multipage_tiff_counts_against_the_page_cap_page_by_page() {
    let archive = zip(
        &[ZipFile::stored(
            "a.tif",
            &fixture("tiff/tifffile-multipage.tif"),
        )],
        Damage::None,
    );
    let tight = Limits {
        max_pages: 2,
        ..Limits::default()
    };
    assert!(matches!(
        cbz::synthesise(Container::Zip, &archive, &tight),
        Err(ArchiveRefusal::TooLarge)
    ));
    let enough = Limits {
        max_pages: 3,
        ..Limits::default()
    };
    let (_, report) = cbz::synthesise(Container::Zip, &archive, &enough).expect("fits");
    assert_eq!(report.pages().len(), 3);
}

/// CMYK, uncompressed (decoded) and deflated (placed as its own bytes): both
/// are `/DeviceCMYK` images holding exactly the recipe's four inks.
#[test]
fn cmyk_tiff_pages_are_device_cmyk_ink_for_ink() {
    let document = open(&[
        ("p1.tif", fixture("tiff/tifffile-cmyk-13x7.tif")),
        ("p2.tif", fixture("tiff/tifffile-cmyk-deflate-13x7.tif")),
    ]);
    assert_no_placeholders(&document);
    let cos = document.cos();
    let want: Vec<u8> = (0..7)
        .flat_map(|y| {
            (0..13).flat_map(move |x| {
                let [c, m, ye] = recipe::rgb(x, y);
                [c, m, ye, recipe::alpha(x, y)]
            })
        })
        .collect();
    for page in 0..2 {
        assert_eq!(space_of(&document, page).as_deref(), Some("DeviceCMYK"));
        assert_eq!(filter_of(&document, page).as_deref(), Some("FlateDecode"));
        let samples = cos
            .stream_decoded(page_image(&document, page))
            .expect("decodes");
        assert_eq!(samples, want, "page {page}");
    }
}

/// A one-strip JPEG 2000 TIFF is placed as `/JPXDecode` over the strip's own
/// codestream and renders to the recipe; a tiled one is decoded and does too.
#[test]
fn jpeg_2000_tiff_pages_are_placed_or_decoded_and_are_the_recipe() {
    let document = open(&[
        ("p1.tif", fixture("tiff/tifffile-jpeg2000-rgb-13x7.tif")),
        ("p2.tif", fixture("tiff/tifffile-jpeg2000-tiled-40x24.tif")),
    ]);
    assert_no_placeholders(&document);
    assert_eq!(filter_of(&document, 0).as_deref(), Some("JPXDecode"));
    assert_eq!(filter_of(&document, 1).as_deref(), Some("FlateDecode"));
    for (page, (w, h)) in [(0u32, (13, 7)), (1, (40, 24))] {
        let bitmap = render(&document, page);
        assert_eq!((bitmap.width, bitmap.height), (w, h));
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    rendered_rgb(&bitmap, x, y),
                    recipe::rgb(x, y),
                    "page {page} ({x}, {y})"
                );
            }
        }
    }
}

/// Signed and floating-point pages arrive as the mapped samples — the signed
/// eight-bit one exactly the grey recipe, the float one sixteen bits deep —
/// and a BigTIFF page is the RGB recipe.
#[test]
fn signed_float_and_bigtiff_pages_are_their_pictures() {
    let document = open(&[
        ("p1.tif", fixture("tiff/tifffile-int8-13x7.tif")),
        (
            "p2.tif",
            fixture("tiff/tifffile-float32-predictor3-13x7.tif"),
        ),
        ("p3.tif", fixture("tiff/tifffile-bigtiff-mm-rgb-13x7.tif")),
    ]);
    assert_no_placeholders(&document);
    let cos = document.cos();
    let grey: Vec<u8> = (0..7)
        .flat_map(|y| (0..13).map(move |x| recipe::grey(x, y)))
        .collect();
    assert_eq!(
        cos.stream_decoded(page_image(&document, 0))
            .expect("decodes"),
        grey
    );
    let dict = image_dict(&document, 1);
    let bits = cos.resolve_key(&dict, cos.intern(b"BitsPerComponent"));
    assert_eq!(bits.as_int(), Some(16));
    let bitmap = render(&document, 2);
    for y in 0..7 {
        for x in 0..13 {
            assert_eq!(rendered_rgb(&bitmap, x, y), recipe::rgb(x, y), "({x}, {y})");
        }
    }
}
