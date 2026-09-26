//! Comic pages in the formats that have no pass-through route: BMP.
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
use tinker_pdf::{Bitmap, Document, PageDefect, RenderOptions};
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
