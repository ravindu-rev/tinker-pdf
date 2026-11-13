//! The document's cache (`docs/design/performance.md`, P-16): what a document
//! with a stated budget keeps across its renders, counted through MS-01's
//! tally, and what it keeps of nothing without one.
//!
//! Every count expected here is derived from what the fixture holds, beside
//! the figure. A count that moves is a change in what a render does.
//!
//! What a render does, once, so the derivations can cite it:
//!
//! - an image XObject a render draws is looked up once a draw; in a document
//!   with a budget, in the document's cache, under the image's object, the
//!   reduction it is decoded at and the defaults its scope paints device
//!   spaces in, and without one in the render's own scope, under its name;
//! - every image here is decoded at full size but the JPEG, a JPEG with no
//!   mask, which is decoded at the smallest of an eighth, a quarter and a
//!   half that its drawn size allows, and whose draw first looks up its
//!   frame — a second lookup a draw, the frame charged 64 bytes — to read
//!   that reduction against; under a budget whose room, beside an entry's
//!   own charge, cannot hold that decode, a JPEG or a codestream with no mask
//!   is decoded at the least reduction past it that fits, and says so;
//! - a decoded image is charged its samples, its alpha and its two-colour
//!   bytes at the decoded size, the words of what its decode reported, and
//!   the cache's 64 bytes beside them; a raw `/DeviceRGB` image of `w` by `h`
//!   is held as `3wh` bytes of samples and nothing else;
//! - every lookup takes the next number of the cache's one counter, and a
//!   fill that would pass the budget evicts the entries whose last lookup is
//!   oldest until it fits;
//! - a budget stated at open charges the document's parsed objects to the
//!   same counter and the same budget, and one stated on the open document
//!   holds the cache alone: a test that counts the cache's evictions under a
//!   budget the page's objects would crowd states it on the open document
//!   ([`cache_only`]).

use std::sync::Arc;

use tinker_pdf::{
    Bitmap, CacheLimits, Document, OpenOptions, Page, Parallelism, PixelRegion, RenderOptions,
    RenderWarning, SimpleFontProvider, Stats, Usage,
};

mod render_support;

/// What the cache charges an entry beside its payload.
const ENTRY_BYTES: u64 = 64;

/// A PDF of `objects`, numbered from 1 in order, with a cross-reference table
/// so that it opens without repair.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A stream object: `dict`'s entries, its `/Length`, and `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A document of `pages`, each `(resources, content)` on a 100-point square,
/// after `shared` objects numbered from 3: object 1 is the catalog, 2 the page
/// tree, then the shared objects, then each page and its content in turn.
fn document(shared: &[Vec<u8>], pages: &[(&str, &str)]) -> Vec<u8> {
    let first = 3 + shared.len();
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", first + 2 * i))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            pages.len(),
            kids.join(" ")
        )
        .into_bytes(),
    ];
    objects.extend(shared.iter().cloned());
    for (index, (resources, content)) in pages.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources {resources} \
                 /Contents {} 0 R >>",
                first + 2 * index + 1
            )
            .into_bytes(),
        );
        objects.push(stream("", content.as_bytes()));
    }
    pdf(&objects)
}

/// A raw `/DeviceRGB` image of `width` by `height`, its samples a pattern
/// `seed` picks, so that two images of one size draw differently.
fn rgb_image(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let samples: Vec<u8> = (0..width * height * 3)
        .map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed))
        .collect();
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8"
        ),
        &samples,
    )
}

/// `data` as a zlib stream of stored deflate blocks (RFC 1950, RFC 1951
/// 3.2.4): no compression, every byte as it is.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(0xFFFF).collect();
    if chunks.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    for (index, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(index + 1 == chunks.len()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// A `/DeviceGray` image of `width` by `height` through `/FlateDecode`.
fn flate_gray_image(width: u32, height: u32) -> Vec<u8> {
    let samples: Vec<u8> = (0..width * height).map(|i| (i * 29 % 256) as u8).collect();
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"
        ),
        &zlib_stored(&samples),
    )
}

/// Packs `0` and `1` into bytes, most significant bit first, anything else
/// ignored.
fn bits(pattern: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut byte, mut count) = (0u8, 0u32);
    for c in pattern.chars().filter(|c| *c == '0' || *c == '1') {
        byte = (byte << 1) | u8::from(c == '1');
        count += 1;
        if count % 8 == 0 {
            out.push(byte);
            byte = 0;
        }
    }
    if count % 8 != 0 {
        out.push(byte << (8 - count % 8));
    }
    out
}

/// `ccitt.rs`'s damaged row: a fax whose second row will not decode, which
/// decodes with damage tolerated.
fn damaged_fax() -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Image /Width 8 /Height 4 \
         /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
         /DecodeParms << /K -1 /Columns 8 /Rows 4 >>",
        &bits(concat!("001 00110101 011 1 ", "111 ", "00000001")),
    )
}

/// `images.rs`'s placeholder: a JPX codestream of four zero bytes, which this
/// build refuses.
fn refused_jpx() -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode",
        &[0, 0, 0, 0],
    )
}

/// A 4:2:0 baseline JPEG of `width` by `height` from the engine's own
/// encoder, as a `/DeviceRGB` image XObject with no mask.
fn jpeg_image(width: u32, height: u32) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"
        ),
        &jpeg_bytes(width, height),
    )
}

/// The bytes of [`jpeg_image`]'s JPEG.
fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
    use tinker_pdf_filters::{JpegOptions, JpegSampling, JpegSource, JpegSourceColour};

    let samples: Vec<u8> = (0..width * height * 3)
        .map(|i| (i * 53 % 251) as u8)
        .collect();
    tinker_pdf_filters::jpeg_encode(
        &JpegSource {
            width,
            height,
            colour: JpegSourceColour::Rgb,
            stride: width as usize * 3,
            data: &samples,
        },
        &JpegOptions {
            sampling: JpegSampling::FourTwoZero,
            ..JpegOptions::default()
        },
    )
    .expect("the raster encodes")
}

/// `name` drawn into the square at `(x, y)` of side `side`.
fn draw(name: &str, x: u32, y: u32, side: u32) -> String {
    format!("q {side} 0 0 {side} {x} {y} cm /{name} Do Q\n")
}

/// The options of a counted render at `dpi`.
fn counting(dpi: f64) -> RenderOptions {
    RenderOptions {
        stats: true,
        ..RenderOptions::at_dpi(dpi)
    }
}

/// `page` rendered at `dpi`, counting, and the bitmap with what it counted.
fn counted(page: &Page, dpi: f64) -> (Bitmap, Stats) {
    let bitmap = page.render(&counting(dpi));
    let stats = bitmap.stats.expect("a counted render hands its stats back");
    (bitmap, stats)
}

/// `bytes` opened under a budget of `budget` bytes, stated at open: the
/// document's cache and its parsed objects charged to it together.
fn budgeted(bytes: Vec<u8>, budget: u64) -> Document {
    Document::open_with(
        bytes,
        &OpenOptions::default().with_cache_limits(CacheLimits::new(budget)),
    )
    .expect("the fixture opens")
}

/// `bytes` opened, and given a budget of `budget` bytes on the open document:
/// one its cache alone holds, the parsed objects being the open's, so that
/// what a tight budget evicts is the cache's own.
fn cache_only(bytes: Vec<u8>, budget: u64) -> Document {
    Document::open(bytes)
        .expect("the fixture opens")
        .with_cache_limits(CacheLimits::new(budget))
}

/// Asserts two renders are one picture: size, format, every byte and every
/// warning.
#[track_caller]
fn same_render(budgeted: &Bitmap, plain: &Bitmap, what: &str) {
    assert_eq!(
        (budgeted.width, budgeted.height, budgeted.format),
        (plain.width, plain.height, plain.format),
        "{what}: a different shape"
    );
    assert!(budgeted.data == plain.data, "{what}: different pixels");
    assert_eq!(
        budgeted.warnings, plain.warnings,
        "{what}: different warnings"
    );
}

/// The page every image here is drawn on: `/A`, a raw RGB image, and `/B`, a
/// Flate grey one, each drawn once.
fn two_images_page() -> Vec<u8> {
    document(
        &[rgb_image(4, 4, 1), flate_gray_image(4, 4)],
        &[(
            "<< /XObject << /A 3 0 R /B 4 0 R >> >>",
            &format!("{}{}", draw("A", 10, 10, 40), draw("B", 50, 50, 40)),
        )],
    )
}

/// **A second render at another resolution decodes no image**: under a
/// stated budget, the first render at 72 dpi decodes both of the page's
/// images and keeps them, and the render at 150 dpi finds both kept — and
/// draws what a document with no budget draws at 150 dpi, byte for byte.
#[test]
fn a_second_render_at_another_dpi_decodes_no_image() {
    let document = budgeted(two_images_page(), 1 << 20);
    let page = document.page(0).expect("a page");
    let (_, first) = counted(&page, 72.0);
    assert_eq!(first.images_decoded, 2, "each image once: {first:?}");
    assert_eq!((first.image_misses, first.image_hits), (2, 0));
    assert_eq!((first.cache_misses, first.cache_hits), (2, 0));
    // 4 x 4 RGB is 48 bytes and 4 x 4 grey, held as RGB, 48 too.
    assert_eq!(first.cache_bytes_held, 2 * (48 + ENTRY_BYTES));

    let (again, second) = counted(&page, 150.0);
    assert_eq!(
        second.images_decoded, 0,
        "nothing decoded again: {second:?}"
    );
    assert_eq!((second.image_misses, second.image_hits), (0, 2));
    assert_eq!((second.cache_misses, second.cache_hits), (0, 2));
    assert_eq!(second.cache_evictions, 0);

    let plain = Document::open(two_images_page()).expect("it opens");
    let unbudgeted = plain
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(150.0));
    same_render(&again, &unbudgeted, "the second render");

    // A tile of the page at a third resolution is the page's own request,
    // and finds both images kept too (ruling 5).
    let tile = page.render(&RenderOptions {
        region: Some(PixelRegion::new(0, 0, 60, 60)),
        ..counting(300.0)
    });
    let tile_stats = tile.stats.expect("asked");
    assert_eq!(tile_stats.images_decoded, 0, "{tile_stats:?}");
}

/// **A JPEG drawn at two sizes keeps a decode at each, and its frame once**:
/// a 64 by 48 JPEG drawn over 8 by 6 points spans 8 by 6 pixels at 72 dpi,
/// which an eighth of it holds exactly — 8 by 6 samples, 144 bytes — and 16
/// by 12 at 144 dpi, which a quarter holds, 16 by 12 samples, 576 bytes.
/// Under a budget the first render misses the frame, decodes at an eighth
/// and keeps the decode and the frame; a second render at 72 dpi finds both;
/// the first at 144 dpi finds the frame and decodes at a quarter; and every
/// render draws what a document with no budget draws at its resolution, byte
/// for byte. Under a budget of zero each draw decodes once, at its own size.
#[test]
fn a_jpeg_drawn_at_two_sizes_keeps_a_decode_at_each() {
    let bytes = document(
        &[jpeg_image(64, 48)],
        &[(
            "<< /XObject << /J 3 0 R >> >>",
            "q 8 0 0 6 20 20 cm /J Do Q\n",
        )],
    );
    let plain = Document::open(bytes.clone()).expect("it opens");
    let unbudgeted = |dpi: f64| {
        plain
            .page(0)
            .expect("a page")
            .render(&RenderOptions::at_dpi(dpi))
    };
    let eighth = 8 * 6 * 3 + ENTRY_BYTES;
    let quarter = 16 * 12 * 3 + ENTRY_BYTES;

    let document = budgeted(bytes.clone(), 1 << 20);
    let page = document.page(0).expect("a page");
    let (first, stats) = counted(&page, 72.0);
    assert_eq!(stats.images_decoded, 1, "{stats:?}");
    assert_eq!((stats.image_misses, stats.image_hits), (1, 0));
    // The frame and the eighth's decode, each missed and kept.
    assert_eq!((stats.cache_misses, stats.cache_hits), (2, 0));
    assert_eq!(stats.cache_bytes_held, ENTRY_BYTES + eighth);
    same_render(&first, &unbudgeted(72.0), "the first render at 72 dpi");

    let (again, stats) = counted(&page, 72.0);
    assert_eq!(stats.images_decoded, 0, "{stats:?}");
    assert_eq!((stats.image_misses, stats.image_hits), (0, 1));
    assert_eq!((stats.cache_misses, stats.cache_hits), (0, 2));
    same_render(&again, &unbudgeted(72.0), "the second render at 72 dpi");

    let (larger, stats) = counted(&page, 144.0);
    assert_eq!(stats.images_decoded, 1, "a quarter's decode: {stats:?}");
    assert_eq!((stats.image_misses, stats.image_hits), (1, 0));
    // The frame found, the quarter's decode missed and kept beside the
    // eighth's.
    assert_eq!((stats.cache_misses, stats.cache_hits), (1, 1));
    assert_eq!(stats.cache_bytes_held, ENTRY_BYTES + eighth + quarter);
    assert_eq!(stats.cache_evictions, 0);
    same_render(&larger, &unbudgeted(144.0), "the render at 144 dpi");

    for dpi in [72.0, 144.0] {
        let (bitmap, stats) = counted(&page, dpi);
        assert_eq!(stats.images_decoded, 0, "at {dpi} dpi: {stats:?}");
        assert_eq!((stats.cache_misses, stats.cache_hits), (0, 2));
        same_render(&bitmap, &unbudgeted(dpi), &format!("again at {dpi} dpi"));
    }

    let starved = budgeted(bytes, 0);
    let page = starved.page(0).expect("a page");
    for dpi in [72.0, 72.0, 144.0] {
        let (bitmap, stats) = counted(&page, dpi);
        assert_eq!(stats.images_decoded, 1, "at {dpi} dpi: {stats:?}");
        assert_eq!((stats.cache_misses, stats.cache_hits), (2, 0));
        assert_eq!(stats.cache_bytes_held, 0);
        same_render(&bitmap, &unbudgeted(dpi), &format!("starved at {dpi} dpi"));
    }
}

/// **Pages that share one image object decode it once between them**: five
/// pages draw object 3, two of them under a name of their own, and under a
/// budget only the first page's render decodes it; every page draws what it
/// draws with no budget.
#[test]
fn pages_sharing_an_image_object_decode_it_once() {
    let pages: Vec<(String, String)> = (0..5)
        .map(|index| {
            let name = if index % 2 == 0 { "Shared" } else { "Other" };
            (
                format!("<< /XObject << /{name} 3 0 R >> >>"),
                draw(name, 10 + index * 5, 10, 50),
            )
        })
        .collect();
    let pages: Vec<(&str, &str)> = pages
        .iter()
        .map(|(resources, content)| (resources.as_str(), content.as_str()))
        .collect();
    let bytes = document(&[rgb_image(6, 5, 7)], &pages);
    let document = budgeted(bytes.clone(), 1 << 20);
    let plain = Document::open(bytes).expect("it opens");

    let mut decoded = 0;
    for index in 0..5 {
        let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
        decoded += stats.images_decoded;
        let expected_hits = u64::from(index > 0);
        assert_eq!(
            (stats.image_hits, stats.cache_hits),
            (expected_hits, expected_hits),
            "page {index}: {stats:?}"
        );
        let unbudgeted = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(&bitmap, &unbudgeted, &format!("page {index}"));
    }
    assert_eq!(decoded, 1, "one decode for five pages");
}

/// The bytes the cache charges a raw `/DeviceRGB` image of `width` by
/// `height` in a scope that puts no default in: its samples and 64 beside.
fn rgb_charge(width: u64, height: u64) -> u64 {
    3 * width * height + ENTRY_BYTES
}

/// One scripted sequence of renders over three pages, each drawing one image
/// of its own, under a budget that holds two of them, which the cache alone
/// holds: per render, its image hits and misses, the entries it evicted and
/// the most the cache held.
fn scripted() -> Vec<(u64, u64, u64, u64)> {
    let bytes = document(
        &[
            rgb_image(10, 10, 1),
            rgb_image(10, 10, 2),
            rgb_image(10, 10, 3),
        ],
        &[
            ("<< /XObject << /I 3 0 R >> >>", &draw("I", 0, 0, 100)),
            ("<< /XObject << /I 4 0 R >> >>", &draw("I", 0, 0, 100)),
            ("<< /XObject << /I 5 0 R >> >>", &draw("I", 0, 0, 100)),
        ],
    );
    let budget = 2 * rgb_charge(10, 10);
    let document = cache_only(bytes, budget);
    [0, 1, 0, 2, 0, 1]
        .into_iter()
        .map(|index| {
            let (_, stats) = counted(&document.page(index).expect("a page"), 72.0);
            assert!(
                stats.cache_bytes_held <= budget,
                "page {index} held {} of {budget}",
                stats.cache_bytes_held
            );
            (
                stats.image_hits,
                stats.image_misses,
                stats.cache_evictions,
                stats.cache_bytes_held,
            )
        })
        .collect()
}

/// **Eviction follows the request sequence and never passes the budget.**
/// Three images of 364 bytes each under a budget of two: A and B fill it, A
/// hits, C evicts B (requested longer ago than A), A hits, and B evicts C —
/// the same evictions on every run, whatever the hash order of the process.
#[test]
fn eviction_follows_the_request_sequence_and_never_passes_the_budget() {
    let one = rgb_charge(10, 10);
    let expected = vec![
        (0, 1, 0, one),
        (0, 1, 0, 2 * one),
        (1, 0, 0, 2 * one),
        (0, 1, 1, 2 * one),
        (1, 0, 0, 2 * one),
        (0, 1, 1, 2 * one),
    ];
    let first = scripted();
    assert_eq!(first, expected, "A, B, A, C, A, B");
    for _ in 0..4 {
        assert_eq!(scripted(), first, "the same evictions on every run");
    }
}

/// **A document with no budget keeps nothing past its render**: each render
/// decodes the page's image again, finding it kept only within the render
/// that decoded it — the second draw of one page — and the document's cache
/// counts nothing.
#[test]
fn a_document_with_no_budget_keeps_nothing_past_its_render() {
    let bytes = document(
        &[rgb_image(4, 4, 3)],
        &[(
            "<< /XObject << /I 3 0 R >> >>",
            &format!("{}{}", draw("I", 0, 0, 40), draw("I", 50, 50, 40)),
        )],
    );
    let document = Document::open(bytes).expect("it opens");
    assert_eq!(document.cache_limits(), None);
    let page = document.page(0).expect("a page");
    for render in 0..3 {
        let (_, stats) = counted(&page, 72.0 + f64::from(render) * 30.0);
        assert_eq!(
            (stats.images_decoded, stats.image_misses, stats.image_hits),
            (1, 1, 1),
            "render {render}: decoded at its first draw and found at its second"
        );
        assert_eq!(
            (
                stats.cache_hits,
                stats.cache_misses,
                stats.cache_evictions,
                stats.cache_bytes_held
            ),
            (0, 0, 0, 0),
            "render {render}: the document's cache counts nothing"
        );
    }
    // A display list keeps its own page's images for its own renders, as it
    // always has.
    let list = page.display_list();
    let replays: Vec<u64> = (0..2)
        .map(|_| {
            list.render(&counting(72.0))
                .stats
                .expect("asked")
                .images_decoded
        })
        .collect();
    assert_eq!(
        replays,
        [1, 0],
        "the list's first replay decodes, its second finds"
    );
}

/// **A budget too small to keep anything decodes at every draw**, and draws
/// what a document with no budget draws: the starved render.
#[test]
fn a_budget_of_zero_decodes_an_image_at_every_draw() {
    let bytes = document(
        &[rgb_image(4, 4, 5)],
        &[(
            "<< /XObject << /I 3 0 R >> >>",
            &format!("{}{}", draw("I", 0, 0, 40), draw("I", 50, 50, 40)),
        )],
    );
    let document = budgeted(bytes.clone(), 0);
    assert_eq!(document.cache_limits(), Some(CacheLimits::new(0)));
    let (bitmap, stats) = counted(&document.page(0).expect("a page"), 72.0);
    assert_eq!(
        (stats.images_decoded, stats.image_misses, stats.image_hits),
        (2, 2, 0),
        "every draw a miss: {stats:?}"
    );
    assert_eq!((stats.cache_misses, stats.cache_bytes_held), (2, 0));
    let plain = Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    same_render(&bitmap, &plain, "the starved render");
}

/// **A kept image reports what its decode reported, under each render's own
/// name for it.** The damaged fax is `/Fax` on page 0 and `/Scan` on page 1;
/// page 1's render finds it kept and names the damage under `/Scan`, as a
/// render of its own decode does, and the refused JPEG 2000 image is refused
/// once a document, each render saying its codec.
#[test]
fn a_kept_image_reports_what_its_decode_reported_under_each_name() {
    let bytes = document(
        &[damaged_fax(), refused_jpx()],
        &[
            (
                "<< /XObject << /Fax 3 0 R /Odd 4 0 R >> >>",
                &format!("{}{}", draw("Fax", 0, 0, 40), draw("Odd", 50, 50, 20)),
            ),
            (
                "<< /XObject << /Scan 3 0 R /Odd 4 0 R >> >>",
                &format!("{}{}", draw("Scan", 0, 0, 40), draw("Odd", 50, 50, 20)),
            ),
        ],
    );
    let document = budgeted(bytes.clone(), 1 << 20);
    let plain = Document::open(bytes).expect("it opens");
    let damage = |bitmap: &Bitmap| -> Vec<String> {
        bitmap
            .warnings
            .iter()
            .filter_map(|warning| match warning {
                RenderWarning::DamagedImage { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    let refused = |bitmap: &Bitmap| {
        bitmap
            .warnings
            .iter()
            .filter(|warning| matches!(warning, RenderWarning::UnsupportedImage { .. }))
            .count()
    };

    let (first, stats) = counted(&document.page(0).expect("a page"), 72.0);
    assert_eq!(stats.images_decoded, 2, "{stats:?}");
    assert_eq!(damage(&first), ["Fax"]);
    assert_eq!(refused(&first), 1);
    let (second, stats) = counted(&document.page(1).expect("a page"), 72.0);
    assert_eq!(
        (stats.images_decoded, stats.image_hits),
        (0, 2),
        "both kept, the refusal as firmly as the picture: {stats:?}"
    );
    assert_eq!(damage(&second), ["Scan"], "the damage under page 1's name");
    assert_eq!(refused(&second), 1);
    for (index, bitmap) in [(0, &first), (1, &second)] {
        let unbudgeted = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(bitmap, &unbudgeted, &format!("page {index}"));
    }
}

/// **The defaults a scope paints device spaces in are part of the key**: one
/// `/DeviceRGB` image drawn in a scope whose `/DefaultRGB` is a `/CalRGB`
/// space and in one that names none is two entries, each drawn as a document
/// with no budget draws it, and a third scope naming the same default finds
/// the first's.
#[test]
fn two_scopes_painting_one_image_in_different_defaults_keep_two_entries() {
    let calibrated = "<< /XObject << /I 3 0 R >> /ColorSpace << /DefaultRGB [/CalRGB \
                      << /WhitePoint [0.9505 1 1.089] /Gamma [2.2 2.2 2.2] >>] >> >>";
    let bytes = document(
        &[rgb_image(4, 4, 9)],
        &[
            (calibrated, &draw("I", 0, 0, 100)),
            ("<< /XObject << /I 3 0 R >> >>", &draw("I", 0, 0, 100)),
            (calibrated, &draw("I", 0, 0, 100)),
        ],
    );
    let document = budgeted(bytes.clone(), 1 << 20);
    let plain = Document::open(bytes).expect("it opens");
    let mut pictures = Vec::new();
    for (index, decodes) in [(0, 1), (1, 1), (2, 0)] {
        let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
        assert_eq!(stats.images_decoded, decodes, "page {index}: {stats:?}");
        let unbudgeted = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(&bitmap, &unbudgeted, &format!("page {index}"));
        pictures.push(bitmap.data);
    }
    assert!(
        pictures[0] != pictures[1],
        "the default changes the picture, so a shared entry would show"
    );
    assert!(pictures[0] == pictures[2]);
}

/// **Clones and pages share one cache; a new budget and new fonts each start
/// a cache of their own**, and the same budget at open and on an open
/// document keeps the same images.
#[test]
fn clones_share_the_cache_and_a_new_budget_or_new_fonts_start_another() {
    let budget = CacheLimits::new(1 << 20);
    let document = Document::open(two_images_page())
        .expect("it opens")
        .with_cache_limits(budget);
    assert_eq!(document.cache_limits(), Some(budget));
    let decodes = |document: &Document| counted(&document.page(0).expect("a page"), 72.0).1;
    assert_eq!(decodes(&document).images_decoded, 2);
    let clone = document.clone();
    assert_eq!(
        decodes(&clone).images_decoded,
        0,
        "a clone shares the cache"
    );
    let page = document.page(0).expect("a page");
    let held = page.clone();
    assert_eq!(
        counted(&held, 96.0).1.images_decoded,
        0,
        "a page's clone too"
    );

    let rebudgeted = document.clone().with_cache_limits(budget);
    assert_eq!(rebudgeted.cache_limits(), Some(budget));
    assert_eq!(
        decodes(&rebudgeted).images_decoded,
        2,
        "a cache of its own, which has kept nothing yet"
    );
    let refonted =
        document
            .clone()
            .with_fonts(std::sync::Arc::new(tinker_pdf::SimpleFontProvider::new(
                Vec::new(),
            )));
    assert_eq!(
        refonted.cache_limits(),
        Some(budget),
        "the budget carries over"
    );
    assert_eq!(
        decodes(&refonted).images_decoded,
        2,
        "what the old cache kept was drawn with the old faces"
    );
    assert_eq!(
        decodes(&document).images_decoded,
        0,
        "the first still keeps"
    );
}

/// **A display list's renders keep their images in the document's cache**:
/// with a budget, a list, the page it was recorded from and another list of
/// it decode each image once between them.
#[test]
fn display_lists_keep_their_images_in_the_documents_cache() {
    let document = budgeted(two_images_page(), 1 << 20);
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    let replay = list.render(&counting(72.0));
    assert_eq!(replay.stats.expect("asked").images_decoded, 2);
    let (direct, stats) = counted(&page, 72.0);
    assert_eq!(
        stats.images_decoded, 0,
        "the direct render finds the list's"
    );
    let other = page.display_list();
    assert_eq!(
        other
            .render(&counting(144.0))
            .stats
            .expect("asked")
            .images_decoded,
        0,
        "and so does another list of the page"
    );
    same_render(&replay, &direct, "the replay and the direct render");
}

/// **Renders on four threads decode a shared image once**: four pages that
/// draw one image object, rendered through the facade's page walk on four
/// threads under a budget, decode it once between them — the fill is
/// single-flight, so a page that reaches the image while another decodes it
/// waits for that decode — and each draws what it draws alone. On a target
/// that starts no thread the walk runs on the caller, and the count is the
/// same.
#[test]
fn renders_on_four_threads_decode_a_shared_image_once() {
    let pages: Vec<(&str, String)> = (0..4)
        .map(|index| {
            (
                "<< /XObject << /I 3 0 R >> >>",
                draw("I", index * 10, 0, 60),
            )
        })
        .collect();
    let pages: Vec<(&str, &str)> = pages
        .iter()
        .map(|(resources, content)| (*resources, content.as_str()))
        .collect();
    let bytes = document(&[rgb_image(64, 64, 4)], &pages);
    let document = budgeted(bytes.clone(), 1 << 20);
    let options = RenderOptions {
        parallelism: Parallelism::Threads(4),
        ..counting(72.0)
    };
    let mut decoded = 0;
    let mut bitmaps = Vec::new();
    document.render_pages(0..4, &options, |index, bitmap| {
        let bitmap = bitmap.expect("every page renders");
        decoded += bitmap.stats.expect("asked").images_decoded;
        bitmaps.push((index, bitmap));
    });
    assert_eq!(decoded, 1, "one decode for four renders");
    let plain = Document::open(bytes).expect("it opens");
    for (index, bitmap) in &bitmaps {
        let alone = plain
            .page(*index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(bitmap, &alone, &format!("page {index}"));
    }
}

// ---- ICC transforms ---------------------------------------------------------

/// An ICC profile of device class `class` over the data space `space` into
/// XYZ, carrying `tags` (ICC.1 7.2 and 7.3): the 128-byte header with what a
/// reader checks — the size, a version 2.4 number, the class, the two spaces
/// and `acsp` — the tag table, and each tag's data on a four-byte boundary.
fn icc_profile(class: &[u8; 4], space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let start = 128 + 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (signature, body) in tags {
        while (start + data.len()) % 4 != 0 {
            data.push(0);
        }
        entries.push((*signature, start + data.len(), body.len()));
        data.extend_from_slice(body);
    }
    while (start + data.len()) % 4 != 0 {
        data.push(0);
    }
    let size = start + data.len();
    let mut profile = vec![0u8; 128];
    profile[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    profile[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    profile[12..16].copy_from_slice(class);
    profile[16..20].copy_from_slice(space);
    profile[20..24].copy_from_slice(b"XYZ ");
    profile[36..40].copy_from_slice(b"acsp");
    profile.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for (signature, offset, len) in entries {
        profile.extend_from_slice(&signature);
        profile.extend_from_slice(&(offset as u32).to_be_bytes());
        profile.extend_from_slice(&(len as u32).to_be_bytes());
    }
    profile.extend_from_slice(&data);
    profile
}

/// An s15Fixed16Number (ICC.1 4.6).
fn s15(value: f64) -> [u8; 4] {
    (((value * 65536.0).round() as i32) as u32).to_be_bytes()
}

/// An `XYZ ` tag of one value.
fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for value in [x, y, z] {
        tag.extend_from_slice(&s15(value));
    }
    tag
}

/// A `curv` tag of one gamma, as a u8Fixed8Number.
fn gamma_tag(gamma: f64) -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend_from_slice(&1u32.to_be_bytes());
    tag.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    tag
}

/// A matrix/TRC RGB display profile: sRGB's primaries adapted to D50 under a
/// gamma of 1.8 on each channel, so its colours come out other than
/// DeviceRGB's.
fn rgb_profile() -> Vec<u8> {
    icc_profile(
        b"mntr",
        b"RGB ",
        &[
            (*b"rXYZ", xyz_tag(0.436_065, 0.222_488, 0.013_916)),
            (*b"gXYZ", xyz_tag(0.385_147, 0.716_873, 0.097_076)),
            (*b"bXYZ", xyz_tag(0.143_066, 0.060_607, 0.714_096)),
            (*b"rTRC", gamma_tag(1.8)),
            (*b"gTRC", gamma_tag(1.8)),
            (*b"bTRC", gamma_tag(1.8)),
            (*b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
        ],
    )
}

/// `determinism.rs`'s `/DefaultCMYK` profile: a version 2 printer profile
/// whose one `mft2` table takes four inks to XYZ on a grid of two points an
/// axis, each corner the light of its inks squeezed into a tenth to nine
/// tenths.
fn cmyk_profile() -> Vec<u8> {
    let mut tag = b"mft2\0\0\0\0".to_vec();
    tag.extend_from_slice(&[4, 3, 2, 0]);
    for row in 0..3 {
        for column in 0..3 {
            tag.extend_from_slice(&s15(if row == column { 1.0 } else { 0.0 }));
        }
    }
    tag.extend_from_slice(&2u16.to_be_bytes());
    tag.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..4 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    let primaries = [
        [0.436_065, 0.222_488, 0.013_916],
        [0.385_147, 0.716_873, 0.097_076],
        [0.143_066, 0.060_607, 0.714_096],
    ];
    for corner in 0..16u32 {
        let ink = |bit: u32| f64::from((corner >> bit) & 1);
        let (c, m, y, k) = (ink(3), ink(2), ink(1), ink(0));
        let light = [
            0.1 + 0.8 * (1.0 - c) * (1.0 - k),
            0.1 + 0.8 * (1.0 - m) * (1.0 - k),
            0.1 + 0.8 * (1.0 - y) * (1.0 - k) * 0.7,
        ];
        for axis in 0..3 {
            let value: f64 = primaries
                .iter()
                .zip(light)
                .map(|(primary, amount)| primary[axis] * amount)
                .sum();
            tag.extend_from_slice(&((value * 32768.0).round() as u16).to_be_bytes());
        }
    }
    for _ in 0..3 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    icc_profile(b"prtr", b"CMYK", &[(*b"A2B0", tag)])
}

/// Twenty pages, each naming an RGB profile (object 3) and a CMYK one
/// (object 4) as `/CS0` and `/CS1` and filling a square in each, then
/// drawing a form (object 5) whose own resources name the same two objects
/// as `/FormRGB` and `/FormCMYK` and fill two more.
fn profiles_page() -> Vec<u8> {
    let form = "/FormRGB cs 0.2 0.4 0.6 sc 0 0 20 20 re f \
                /FormCMYK cs 0.1 0.2 0.3 0.4 sc 20 0 20 20 re f";
    let shared = [
        stream("/N 3", &rgb_profile()),
        stream("/N 4", &cmyk_profile()),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 20] /Resources << /ColorSpace << \
             /FormRGB [/ICCBased 3 0 R] /FormCMYK [/ICCBased 4 0 R] >> >>",
            form.as_bytes(),
        ),
    ];
    let resources = "<< /ColorSpace << /CS0 [/ICCBased 3 0 R] /CS1 [/ICCBased 4 0 R] >> \
                     /XObject << /Fm 5 0 R >> >>";
    let content = "/CS0 cs 0.9 0.1 0.1 sc 0 50 50 50 re f \
                   /CS1 cs 0.1 0.6 0.3 0.2 sc 50 50 50 50 re f \
                   q 1 0 0 1 10 10 cm /Fm Do Q";
    let pages = vec![(resources, content); 20];
    document(&shared, &pages)
}

/// **A profile named on every page and in a form compiles once**: twenty
/// pages, each naming one RGB and one CMYK profile and drawing a form whose
/// own resources name both again, rendered twice each under a budget,
/// compile two transforms between them — each scope's first question about
/// a name finds the profile's transforms kept from the first one's — and
/// every page draws what a document with no budget draws, which compiles
/// each profile once in every scope of every render: the page's and the
/// form's.
#[test]
fn a_profile_named_on_every_page_compiles_once() {
    let bytes = profiles_page();
    let document = budgeted(bytes.clone(), 1 << 20);
    let plain = Document::open(bytes).expect("it opens");
    let (mut misses, mut hits, mut unbudgeted_misses) = (0, 0, 0);
    for pass in 0..2 {
        for index in 0..20 {
            let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
            misses += stats.icc_transform_misses;
            hits += stats.icc_transform_hits;
            let (alone, alone_stats) = counted(&plain.page(index).expect("a page"), 72.0);
            unbudgeted_misses += alone_stats.icc_transform_misses;
            assert_eq!(
                alone_stats.icc_transform_misses, 4,
                "page {index}: each of the page's two names and the form's two"
            );
            same_render(&bitmap, &alone, &format!("pass {pass}, page {index}"));
        }
    }
    assert_eq!(misses, 2, "two transforms compiled for forty renders");
    assert_eq!(
        hits,
        4 * 40 - 2,
        "every other question found the transforms kept"
    );
    assert_eq!(unbudgeted_misses, 4 * 40);
}

/// **A profile that will not parse is refused once a document**, its
/// refusal kept as firmly as a transform: every later page reads the space
/// by its alternate (8.6.5.5) without reading the profile again, and draws
/// what a document with no budget draws.
#[test]
fn a_profile_that_will_not_parse_is_refused_once_a_document() {
    let page = (
        "<< /ColorSpace << /CS0 [/ICCBased 3 0 R] >> >>",
        "/CS0 cs 0.9 0.1 0.1 sc 0 0 50 50 re f",
    );
    let bytes = document(
        &[stream("/N 3", b"not an ICC profile")],
        &[page, page, page],
    );
    let document = budgeted(bytes.clone(), 1 << 20);
    let plain = Document::open(bytes).expect("it opens");
    for index in 0..3 {
        let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
        assert_eq!(
            (stats.icc_transform_misses, stats.icc_transform_hits),
            (u64::from(index == 0), u64::from(index > 0)),
            "page {index}: {stats:?}"
        );
        let alone = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(&bitmap, &alone, &format!("page {index}"));
    }
}

/// **Transforms and images share one budget and one order**: a page naming
/// a profile and drawing an image under a budget with room for the image
/// alone — one the cache alone holds — keeps the image and compiles the
/// transforms at every render — the transforms, asked for first, are evicted
/// for the image or never fit beside it — the bytes held never pass the
/// budget, and every render draws what a document with no budget draws.
#[test]
fn transforms_and_images_share_one_budget() {
    let bytes = document(
        &[stream("/N 3", &rgb_profile()), rgb_image(10, 10, 6)],
        &[(
            "<< /ColorSpace << /CS0 [/ICCBased 3 0 R] >> /XObject << /I 4 0 R >> >>",
            &format!(
                "/CS0 cs 0.9 0.1 0.1 sc 0 0 50 50 re f {}",
                draw("I", 50, 50, 50)
            ),
        )],
    );
    let budget = rgb_charge(10, 10);
    let document = cache_only(bytes.clone(), budget);
    let plain = Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    for render in 0..3 {
        let (bitmap, stats) = counted(&document.page(0).expect("a page"), 72.0);
        assert!(
            stats.cache_bytes_held <= budget,
            "render {render}: {stats:?}"
        );
        assert_eq!(
            stats.icc_transform_misses, 1,
            "render {render}: the transforms never stay beside the image"
        );
        same_render(&bitmap, &plain, &format!("render {render}"));
    }
}

// ---- form recordings --------------------------------------------------------

/// A stamp: a red rule, a blue frame and the word "Draft" in Helvetica, the
/// form's content `q`-bracketed and its text in a text object of its own.
const STAMP: &str = "q 0.8 0 0 rg 5 5 90 2 re f 0 0 1 RG 2 w 10 10 80 30 re S \
                     BT /Helv 12 Tf 20 20 Td (Draft) Tj ET Q";

/// `pages` pages each drawing [`STAMP`] (object 3) after text and a path of
/// its own, which leave the text matrix and the current point somewhere else
/// on every page. Where `own` the stamp brings its own resources naming
/// Helvetica (object 4); otherwise every page's resources name it, and the
/// stamp resolves its font there.
fn stamped_pages(pages: u32, own: bool) -> Vec<u8> {
    let stamp = if own {
        "/Type /XObject /Subtype /Form /BBox [0 0 100 50] /Resources << /Font << /Helv 4 0 R >> >>"
    } else {
        "/Type /XObject /Subtype /Form /BBox [0 0 100 50]"
    };
    let shared = [
        stream(stamp, STAMP.as_bytes()),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    let resources = if own {
        "<< /XObject << /Stamp 3 0 R >> >>"
    } else {
        "<< /XObject << /Stamp 3 0 R >> /Font << /Helv 4 0 R >> >>"
    };
    let contents: Vec<String> = (0..pages)
        .map(|index| {
            format!(
                "BT /Helv 10 Tf 10 {} Td (Page {index}) Tj ET 0 0 0 rg {} {} m 60 60 l S \
                 q 1 0 0 1 0 40 cm /Stamp Do Q",
                80 - index % 7,
                index % 11,
                index % 13
            )
        })
        .collect();
    let pages: Vec<(&str, &str)> = contents
        .iter()
        .map(|content| (resources, content.as_str()))
        .collect();
    document(&shared, &pages)
}

/// A page whose form `/Fm` paints a green left half and, inside marked
/// content of an optional-content group a print render turns off, a red
/// right half: the group is on in every render but one naming print
/// (8.11.4.4).
fn usage_page() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] \
          /D << /ON [5 0 R] /AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>] >> >> >>"
            .to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] \
          /Resources << /XObject << /Fm 6 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        stream("", b"/Fm Do"),
        b"<< /Type /OCG /Name (Layer) /Usage << /Print << /PrintState /OFF >> >> >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 40] \
             /Resources << /Properties << /MC0 5 0 R >> >>",
            b"0 1 0 rg 0 0 20 40 re f /OC /MC0 BDC 1 0 0 rg 20 0 20 40 re f EMC",
        ),
    ])
}

/// **A form drawn for two usages keeps a drawing for each**: the usage a
/// render names binds the optional-content states the form's marked content
/// reads, so a render naming none and one naming print each interpret the
/// form once and keep their own drawing, and each later render of either
/// replays its own — never the other's — drawing what a document with no
/// budget draws for that usage, byte for byte.
#[test]
fn a_form_drawn_for_two_usages_keeps_a_drawing_for_each() {
    let bytes = usage_page();
    let render = |document: &Document, usage: Option<Usage>| {
        let bitmap = document.page(0).expect("a page").render(&RenderOptions {
            usage,
            ..counting(72.0)
        });
        let stats = bitmap.stats.expect("a counted render hands its stats back");
        (bitmap, stats)
    };
    let plain = Document::open(bytes.clone()).expect("it opens");
    let (shown, _) = render(&plain, None);
    let (printed, _) = render(&plain, Some(Usage::Print));
    assert!(
        shown.data != printed.data,
        "the fixture is what it says: print hides the red half"
    );

    let document = budgeted(bytes, 16 << 20);
    for (round, expected) in [(0, (1, 0)), (1, (0, 1))] {
        for (usage, alone) in [(None, &shown), (Some(Usage::Print), &printed)] {
            let (bitmap, stats) = render(&document, usage);
            assert_eq!(
                (stats.forms_interpreted, stats.forms_replayed),
                expected,
                "round {round}, {usage:?}: {stats:?}"
            );
            same_render(&bitmap, alone, &format!("round {round}, {usage:?}"));
        }
    }
}

/// **A form stamped on a hundred pages is interpreted once**: under a stated
/// budget the first page's render interprets the stamp and keeps its
/// drawing, and the ninety-nine after it replay it — each page's own text and
/// path leave the text matrix and the current point elsewhere, which the
/// stamp, setting its own, never reads — and every page draws what it draws
/// with no budget, where every render interprets it. So too where the stamp
/// resolves its font in every page's resources, which are one dictionary's
/// worth between them.
#[test]
fn a_form_stamped_on_a_hundred_pages_is_interpreted_once() {
    for own in [true, false] {
        let bytes = stamped_pages(100, own);
        let document = budgeted(bytes.clone(), 16 << 20);
        let plain = Document::open(bytes).expect("it opens");
        let (mut interpreted, mut replayed) = (0, 0);
        for index in 0..100 {
            let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
            interpreted += stats.forms_interpreted;
            replayed += stats.forms_replayed;
            let (alone, alone_stats) = counted(&plain.page(index).expect("a page"), 72.0);
            assert_eq!(
                (alone_stats.forms_interpreted, alone_stats.forms_replayed),
                (0, 0),
                "with no budget neither is counted"
            );
            same_render(&bitmap, &alone, &format!("own {own}, page {index}"));
        }
        assert_eq!(
            (interpreted, replayed),
            (1, 99),
            "own resources {own}: one interpretation for a hundred pages"
        );
    }
}

/// **A tile and the page hit the same drawing** (ruling 5): a recording is a
/// function of the stream and the invoking state, never of the canvas, so a
/// region of the page and a render at another resolution replay what the
/// whole page's render recorded, and draw what they draw with no budget.
#[test]
fn a_tile_and_the_page_replay_one_drawing() {
    let bytes = stamped_pages(1, true);
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let (_, whole) = counted(&page, 72.0);
    assert_eq!((whole.forms_interpreted, whole.forms_replayed), (1, 0));
    let options = RenderOptions {
        region: Some(PixelRegion::new(5, 10, 40, 30)),
        ..counting(150.0)
    };
    let tile = page.render(&options);
    let tile_stats = tile.stats.expect("asked");
    assert_eq!(
        (tile_stats.forms_interpreted, tile_stats.forms_replayed),
        (0, 1)
    );
    let alone = plain.page(0).expect("a page").render(&options);
    same_render(&tile, &alone, "the tile");
}

/// **A hit reports again what the form's interpretation reported**: a form
/// with no resources of its own that selects a font its pages do not define
/// is kept, and a page that replays it says, as one that interprets it does,
/// that it met a font it could not read; one that reads a colour space with a
/// repair is never kept, since the repair is said among what the renderer
/// says, in an order a recording cannot keep — and every page draws and
/// reports what it does with no budget.
#[test]
fn a_kept_form_reports_what_its_interpretation_reported() {
    let missing = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
        b"BT /Missing 12 Tf 10 10 Td (Gone) Tj ET 0 0 1 rg 0 0 10 10 re f",
    );
    let repaired = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
        b"/Lab0 cs 50 10 10 sc 20 20 30 30 re f",
    );
    let resources = "<< /XObject << /Missing 3 0 R /Repaired 4 0 R >> \
                     /ColorSpace << /Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] \
                     /Range [100 -100 100 -100] >>] >> >>";
    let page = (resources, "/Missing Do /Repaired Do");
    let bytes = document(&[missing, repaired], &[page, page, page]);
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("it opens");
    for index in 0..3 {
        let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
        assert_eq!(
            (stats.forms_interpreted, stats.forms_replayed),
            if index == 0 { (2, 0) } else { (1, 1) },
            "page {index}: the missing font's form kept, the repaired space's not"
        );
        assert!(
            bitmap.warnings.contains(&RenderWarning::UnreadableFont),
            "page {index}: {:?}",
            bitmap.warnings
        );
        assert!(
            bitmap
                .warnings
                .iter()
                .any(|warning| matches!(warning, RenderWarning::RepairedColorSpace { .. })),
            "page {index}: {:?}",
            bitmap.warnings
        );
        let alone = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(&bitmap, &alone, &format!("page {index}"));
    }
}

/// **Display lists record through the kept drawing**: under a budget, ten
/// pages' display lists are recorded through the document's cache, so each
/// list replays as its page renders and a direct render after them replays
/// the stamp's drawing a list's recording kept.
#[test]
fn display_lists_record_a_stamp_once() {
    let bytes = stamped_pages(10, false);
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("it opens");
    let mut interpreted = 0;
    for index in 0..10 {
        let list = document.page(index).expect("a page").display_list();
        assert!(list.is_retained());
        let replay = list.render(&counting(72.0));
        interpreted += replay.stats.expect("asked").forms_interpreted;
        let alone = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(&replay, &alone, &format!("page {index}'s list"));
    }
    assert_eq!(
        interpreted, 0,
        "a list's replay replays its recording and draws no form of its own"
    );
    let (_, direct) = counted(&document.page(3).expect("a page"), 72.0);
    assert_eq!(
        (direct.forms_interpreted, direct.forms_replayed),
        (0, 1),
        "the lists' recordings kept the stamp's drawing for the direct render too"
    );
}

/// **With no budget, or a budget of zero, nothing is kept and nothing about
/// forms is counted**, and a cancelled render keeps nothing it drew: the
/// next render interprets the stamp.
#[test]
fn a_budget_of_zero_or_a_cancelled_render_keeps_no_drawing() {
    let bytes = stamped_pages(3, true);
    for budget in [None, Some(0)] {
        let document = match budget {
            Some(budget) => budgeted(bytes.clone(), budget),
            None => Document::open(bytes.clone()).expect("it opens"),
        };
        for index in 0..3 {
            let (_, stats) = counted(&document.page(index).expect("a page"), 72.0);
            assert_eq!(
                (stats.forms_interpreted, stats.forms_replayed),
                (0, 0),
                "budget {budget:?}, page {index}"
            );
        }
    }

    let document = budgeted(bytes, 16 << 20);
    let cancel = tinker_pdf::CancelToken::new();
    cancel.cancel();
    let cancelled = document.page(0).expect("a page").render(&RenderOptions {
        cancel: Some(cancel),
        ..counting(72.0)
    });
    assert!(cancelled.warnings.contains(&RenderWarning::Cancelled));
    let (_, after) = counted(&document.page(0).expect("a page"), 72.0);
    assert_eq!(
        (after.forms_interpreted, after.forms_replayed),
        (1, 0),
        "nothing the cancelled render drew was kept"
    );
}

/// `bytes` opened with every font it does not embed read as
/// `render_support`'s curvy face, under `budget` where one is stated.
fn curvy(bytes: Vec<u8>, budget: Option<u64>) -> Document {
    let mut options = OpenOptions::default().with_fonts(Arc::new(SimpleFontProvider::new(
        render_support::curvy_font(),
    )));
    if let Some(budget) = budget {
        options = options.with_cache_limits(CacheLimits::new(budget));
    }
    Document::open_with(bytes, &options).expect("the fixture opens")
}

/// A page of an image and text: `/A`, a raw 8 by 8 RGB image, drawn first,
/// then `ABC` in `/F` twice, the second line 20 points above the first, so
/// each glyph's second draw stands at its first's phase. `/F` embeds nothing
/// and is read through [`curvy`], whose face takes `A`, `B` and `C` to three
/// shapes of its six.
fn glyphs_page() -> Vec<u8> {
    document(
        &[
            rgb_image(8, 8, 3),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ],
        &[(
            "<< /XObject << /A 3 0 R >> /Font << /F 4 0 R >> >>",
            "q 40 0 0 40 50 50 cm /A Do Q\n\
             BT /F 12 Tf 5 20 Td (ABC) Tj ET\n\
             BT /F 12 Tf 5 40 Td (ABC) Tj ET\n",
        )],
    )
}

/// **Glyph masks share the document's budget**: the page's three masks, each
/// drawn twice, are a render's own with no budget, and under a stated one the
/// document's — kept beside the image, found by the second line and by a
/// second render, and looked up with every other entry. Under a budget with
/// room for the image and nothing beside it, which the cache alone holds, the
/// image and the masks take turns: every render decodes the image again, since the first mask kept
/// evicts it, and rasterises as the first render did, since the image kept
/// again evicts every mask; the bytes held never pass the budget, and every
/// render draws what a document with no budget draws, byte for byte.
#[test]
fn glyph_masks_share_the_documents_budget() {
    let bytes = glyphs_page();
    let plain = curvy(bytes.clone(), None);
    let (alone, stats) = counted(&plain.page(0).expect("a page"), 72.0);
    assert_eq!(
        (stats.glyph_masks_rasterised, stats.glyph_mask_hits),
        (3, 3),
        "the fixture is what it says, three masks each drawn twice: {stats:?}"
    );
    assert_eq!((stats.cache_misses, stats.cache_hits), (0, 0), "no budget");

    let roomy = curvy(bytes.clone(), Some(1 << 20));
    let page = roomy.page(0).expect("a page");
    let (first, stats) = counted(&page, 72.0);
    assert_eq!(
        (stats.glyph_masks_rasterised, stats.glyph_mask_hits),
        (3, 3),
        "{stats:?}"
    );
    // The image and three masks missed, the second line's masks found.
    assert_eq!((stats.cache_misses, stats.cache_hits), (4, 3));
    assert_eq!(stats.cache_evictions, 0);
    assert!(
        stats.cache_bytes_held > rgb_charge(8, 8) + 3 * ENTRY_BYTES,
        "the masks are charged their pixels beside the image: {stats:?}"
    );
    same_render(&first, &alone, "the first render under a roomy budget");
    let (second, stats) = counted(&page, 72.0);
    assert_eq!(
        (
            stats.images_decoded,
            stats.glyph_masks_rasterised,
            stats.glyph_mask_hits
        ),
        (0, 0, 6),
        "everything found: {stats:?}"
    );
    assert_eq!((stats.cache_misses, stats.cache_hits), (0, 7));
    same_render(&second, &alone, "the second render under a roomy budget");

    let budget = rgb_charge(8, 8) + ENTRY_BYTES;
    let tight = curvy(bytes, None).with_cache_limits(CacheLimits::new(budget));
    let page = tight.page(0).expect("a page");
    let mut rasterised = Vec::new();
    for render in 0..3 {
        let (bitmap, stats) = counted(&page, 72.0);
        assert!(
            stats.cache_bytes_held <= budget,
            "render {render}: {stats:?}"
        );
        assert!(stats.cache_evictions > 0, "render {render}: {stats:?}");
        assert_eq!(
            stats.images_decoded, 1,
            "render {render}: the image evicted by a mask and decoded again"
        );
        rasterised.push(stats.glyph_masks_rasterised);
        same_render(
            &bitmap,
            &alone,
            &format!("render {render} under {budget} bytes"),
        );
    }
    assert!(
        rasterised[0] >= 3,
        "every mask at least once: {rasterised:?}"
    );
    assert!(
        rasterised.iter().all(|&count| count == rasterised[0]),
        "each render evicted and rasterised again as the first: {rasterised:?}"
    );
}

// ---- parsed objects ---------------------------------------------------------

/// What reading `document`'s object `num` through the counted door parses:
/// 0 where the object store keeps it, 1 where the read parses it again.
fn parses(document: &Document, num: u32) -> u64 {
    let work = tinker_pdf_cos::Work::new();
    document
        .cos()
        .get_counted(tinker_pdf_cos::ObjRef::new(num, 0), &work)
        .expect("the object reads");
    work.stats().objects_parsed
}

/// **A budget stated at open holds the parsed objects too**
/// (`docs/design/performance.md`, P-16.8): the document's objects are charged
/// to its cache's budget, so under a budget of zero no object is kept — the
/// catalog, the page and the image read through the counted door parse at
/// every read, render after render — and under a roomy one what a render read
/// is kept, so a read after it parses nothing. A budget stated on an open
/// document leaves the objects as the open left them, kept as they always
/// were, and so does a document handed back with new fonts. Every render
/// draws what a document with no budget draws.
#[test]
fn a_budget_stated_at_open_holds_the_parsed_objects_too() {
    let bytes = two_images_page();
    let plain = Document::open(bytes.clone()).expect("it opens");
    let alone = plain
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(!plain.cos().objects_charged(), "no budget, no charge");

    // The catalog is object 1, the image `/A` object 3 and the page object 5.
    let starved = budgeted(bytes.clone(), 0);
    assert!(starved.cos().objects_charged());
    for render in 0..3 {
        let bitmap = starved
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(&bitmap, &alone, &format!("starved render {render}"));
        for num in [1, 3, 5] {
            assert_eq!(
                (render, num, parses(&starved, num)),
                (render, num, 1),
                "a budget of zero keeps no object"
            );
        }
    }

    let roomy = budgeted(bytes.clone(), 1 << 20);
    let bitmap = roomy
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    same_render(&bitmap, &alone, "the roomy render");
    for num in [1, 3, 5] {
        assert_eq!(parses(&roomy, num), 0, "object {num} kept from the render");
    }
    let refonted = roomy
        .clone()
        .with_fonts(Arc::new(SimpleFontProvider::new(Vec::new())));
    assert!(
        refonted.cos().objects_charged(),
        "the same objects, charged where they were"
    );
    assert_eq!(parses(&refonted, 1), 0);

    let later = cache_only(bytes, 0);
    assert!(
        !later.cos().objects_charged(),
        "a budget stated on an open document holds its cache alone"
    );
    let bitmap = later
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    same_render(&bitmap, &alone, "the render under a budget stated later");
    for num in [1, 3, 5] {
        assert_eq!(
            parses(&later, num),
            0,
            "object {num} kept, as with no budget"
        );
    }
}

/// **A document appended under a budget stated at open charges its own
/// objects as its open did**: `write::append`'s reopened document keeps the
/// budget in a cache of its own, and where the budget was stated at open its
/// parsed objects are charged to that cache's, so a starved one parses the
/// update's page at every read; one appended under a budget stated later, or
/// none, charges nothing. Each reads the edit as made.
#[test]
fn an_appended_document_charges_its_objects_as_its_open_did() {
    let bytes = two_images_page();
    for (label, document, charged) in [
        ("stated at open", budgeted(bytes.clone(), 0), true),
        ("stated later", cache_only(bytes.clone(), 0), false),
        (
            "none",
            Document::open(bytes.clone()).expect("it opens"),
            false,
        ),
    ] {
        let mut editor = document.editor();
        assert!(editor.rotate_page(0, 90), "{label}: the page turns");
        let appended = tinker_pdf::write::append(
            &document,
            &editor,
            &tinker_pdf::write::AppendOptions::default(),
        )
        .expect("it appends");
        let reopened = &appended.document;
        assert_eq!(reopened.cos().objects_charged(), charged, "{label}");
        assert_eq!(
            reopened.page(0).expect("a page").rotation(),
            90,
            "{label}: the edit as made"
        );
        // The page object the update redefines is the reopened document's
        // own, read once already by the page lookup above: a budget of zero
        // stated at open keeps it nowhere, so each read parses it, and with
        // the objects not charged it is kept as an open keeps it.
        assert_eq!(
            (0..2).map(|_| parses(reopened, 5)).sum::<u64>(),
            if charged { 2 } else { 0 },
            "{label}: the update's page read twice"
        );
    }
}

/// **Parsed objects and the cache's entries leave in one order** under a
/// budget stated at open: a hundred stamped pages rendered one after another
/// under a budget a few pages' objects fill keep the bytes held inside it —
/// the images, transforms, drawings and masks the cache holds beside the
/// objects never passing it — draw what a document with no budget draws, and
/// parse objects again as they leave: the first page's content stream, read
/// a hundred pages ago and held by nothing since, is gone.
#[test]
fn parsed_objects_and_entries_leave_one_budget_in_one_order() {
    let bytes = stamped_pages(100, true);
    let plain = Document::open(bytes.clone()).expect("it opens");
    let budget = 4_096;
    let document = budgeted(bytes, budget);
    for index in 0..100 {
        let (bitmap, stats) = counted(&document.page(index).expect("a page"), 72.0);
        assert!(
            stats.cache_bytes_held <= budget,
            "page {index}: the cache's own share passed the budget: {stats:?}"
        );
        let alone = plain
            .page(index)
            .expect("a page")
            .render(&RenderOptions::default());
        same_render(
            &bitmap,
            &alone,
            &format!("page {index} under {budget} bytes"),
        );
    }
    // The first page's content stream (object 6) was read a hundred pages
    // ago, and nothing has held it since: dropped, it parses again.
    assert_eq!(
        parses(&document, 6),
        1,
        "an object least recently read left"
    );
}

// ---- An image past the budget, decoded at a reduced resolution -------------
//
// `docs/design/performance.md`, P-16, "Reduced-resolution decode". Under a
// budget, a JPEG or a JPEG 2000 image with no mask whose decode at the size
// it is drawn would not fit the budget beside the 64 bytes an entry is
// charged — every image here sits in a scope with no defaults, so its key
// holds nothing more — is decoded at the least reduction past that size's
// whose samples fit, three bytes a sample; and where none fits, at the size
// it is drawn, as with no budget, and not kept.

/// The samples a decode of the JPEG `data` at `scale` holds, as the decoder
/// hands them a strip at a time — RGB, three bytes a sample — with their
/// width and height and the samples its inverse transforms output
/// (`Stats::jpeg_samples_transformed`).
fn jpeg_at(data: &[u8], scale: tinker_pdf_filters::JpegScale) -> (u32, u32, Vec<u8>, u64) {
    use tinker_pdf_filters::{JpegLayout, JpegSink};

    #[derive(Default)]
    struct Rows {
        width: u32,
        height: u32,
        samples: Vec<u8>,
    }
    impl JpegSink for Rows {
        fn begin(&mut self, layout: JpegLayout) {
            self.width = layout.width;
            self.height = layout.height;
        }

        fn strip(&mut self, _y: u32, _rows: u32, data: &[u8]) {
            self.samples.extend_from_slice(data);
        }
    }
    let mut rows = Rows::default();
    let frame = tinker_pdf_filters::jpeg_decode_strips(data, 1 << 28, scale, &mut rows)
        .expect("the JPEG decodes");
    (rows.width, rows.height, rows.samples, frame.transformed)
}

/// One of the decoder's own codestreams (`tinker-pdf-filters/tests/jpx`).
fn codestream(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tinker-pdf-filters/tests/jpx"
        ))
        .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The samples of the codestream `data` decoded with `discard` levels
/// dropped, whole, with their width and height: three 8-bit components a
/// sample, which is RGB.
fn jpx_at(data: &[u8], discard: u8) -> (u32, u32, Vec<u8>) {
    let (image, _) = tinker_pdf_filters::jpx_decode_request(
        data,
        &tinker_pdf_filters::Limits::new(1 << 28),
        &tinker_pdf_filters::JpxRequest {
            discard,
            window: None,
        },
        &mut Vec::new(),
    )
    .expect("the codestream decodes");
    assert_eq!(
        (image.components, image.precision),
        (3, 8),
        "three 8-bit components"
    );
    (image.width, image.height, image.samples)
}

/// The decoder's 32 by 24 colour codestream of two levels, at the grid's
/// origin, so that each level down halves both sides exactly.
fn two_level_codestream() -> Vec<u8> {
    let jpx = codestream("c1-3.jp2");
    let header = tinker_pdf_filters::jpx_header(&jpx, &tinker_pdf_filters::Limits::new(1 << 28))
        .expect("its header reads");
    assert_eq!(
        (header.width, header.height, header.origin, header.levels),
        (32, 24, (0, 0), 2),
        "the fixture these figures are worked from"
    );
    jpx
}

/// A JPEG 2000 image XObject of `width` by `height` holding the codestream
/// `data`, in the codestream's own colour (8.9.5.4), with no mask.
fn jpx_image(width: u32, height: u32, data: &[u8]) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} /Filter /JPXDecode"
        ),
        data,
    )
}

/// A raw `/DeviceRGB` image of `width` by `height` holding `samples`.
fn rgb_samples_image(width: u32, height: u32, samples: &[u8]) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8"
        ),
        samples,
    )
}

/// A page drawing `image`, object 3 under the name `/J`, over the whole of
/// its 100-point square.
fn covered(image: Vec<u8>) -> Vec<u8> {
    document(
        &[image],
        &[(
            "<< /XObject << /J 3 0 R >> >>",
            "q 100 0 0 100 0 0 cm /J Do Q\n",
        )],
    )
}

/// The rendering of a raw image of `samples`, `width` by `height`, over the
/// whole of [`covered`]'s page at `scale`: what a page drawing a decode that
/// holds those samples draws, where the decode's samples span the image's
/// square exactly.
fn drawn_raw(width: u32, height: u32, samples: &[u8], scale: f64) -> Bitmap {
    Document::open(covered(rgb_samples_image(width, height, samples)))
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            scale,
            ..RenderOptions::default()
        })
}

/// The pixels of `bitmap` inside `region`, its rows one after another.
fn cut(bitmap: &Bitmap, region: PixelRegion) -> Vec<u8> {
    let per = bitmap.format.components();
    let mut out = Vec::new();
    for y in region.y..region.y + region.height {
        let start = y as usize * bitmap.stride + region.x as usize * per;
        out.extend_from_slice(&bitmap.data[start..start + region.width as usize * per]);
    }
    out
}

/// Whether `bitmap` says an image was drawn reduced.
fn says_reduced(bitmap: &Bitmap) -> bool {
    bitmap
        .warnings
        .iter()
        .any(|warning| matches!(warning, RenderWarning::ReducedImage { .. }))
}

/// **An image past the budget draws at a reduced resolution and says so.**
///
/// A 64 by 48 JPEG drawn over the 100-pixel page at 72 dpi asks for every
/// sample, 9 216 bytes, which a budget of 4 096 cannot hold; a half holds 32
/// by 24 samples, 2 304 bytes, which it can. So the render decodes the JPEG
/// at a half — its inverse transforms output a half's samples — draws exactly
/// what those samples draw as a raw image over the same square, says
/// `ReducedImage { name: "J", factor: 2 }` beside what a render with no
/// budget says, and keeps the decode and the frame, 2 304 + 64 and 64 bytes,
/// under the budget. A second render finds both and says it again. A render
/// at a fifth of the scale, 20 pixels a side, asks for a half of its own,
/// finds that decode, and draws and says what a document with no budget
/// draws and says there: a hit is the bytes its request's miss computes. A
/// budget stated at open, charging the parsed objects beside, draws and says
/// the same.
///
/// The 32 by 24 codestream of two levels likewise: 2 304 bytes whole, 576 one
/// level down, which a budget of 1 024 holds — one level dropped, counted,
/// `factor: 2` — and a budget of 639, which leaves 575 beside the entry's 64,
/// two, 8 by 6 samples, `factor: 4`. With no budget each image is decoded at
/// the size it is drawn at and nothing is said.
#[test]
fn an_image_past_the_budget_draws_at_a_reduced_resolution_and_says_so() {
    use tinker_pdf_filters::JpegScale;

    let jpeg = jpeg_bytes(64, 48);
    let bytes = covered(jpeg_image(64, 48));
    let plain = Document::open(bytes.clone()).expect("it opens");
    let (unbudgeted, stats) = counted(&plain.page(0).expect("a page"), 72.0);
    let (_, _, _, every) = jpeg_at(&jpeg, JpegScale::Full);
    assert_eq!(stats.jpeg_samples_transformed, every, "no budget: whole");
    assert!(!says_reduced(&unbudgeted), "{:?}", unbudgeted.warnings);

    let (width, height, samples, half) = jpeg_at(&jpeg, JpegScale::Half);
    assert_eq!((width, height), (32, 24));
    let mut says = unbudgeted.warnings.clone();
    says.push(RenderWarning::ReducedImage {
        name: "J".to_string(),
        factor: 2,
    });
    let budget = 4_096;
    let document = cache_only(bytes.clone(), budget);
    let page = document.page(0).expect("a page");
    let (first, stats) = counted(&page, 72.0);
    assert_eq!(stats.images_decoded, 1, "{stats:?}");
    assert_eq!(stats.jpeg_samples_transformed, half, "decoded at a half");
    assert_eq!(first.warnings, says);
    assert_eq!(stats.cache_bytes_held, (2_304 + ENTRY_BYTES) + ENTRY_BYTES);
    assert!(stats.cache_bytes_held <= budget);
    let raw = drawn_raw(32, 24, &samples, 1.0);
    assert_eq!((first.width, first.height), (raw.width, raw.height));
    assert!(first.data == raw.data, "the page draws the half's samples");
    assert!(first.data != unbudgeted.data, "and not every sample's");

    let (again, stats) = counted(&page, 72.0);
    assert_eq!(stats.images_decoded, 0, "{stats:?}");
    assert_eq!((stats.cache_misses, stats.cache_hits), (0, 2));
    same_render(&again, &first, "the second render");

    let fifth = RenderOptions {
        scale: 0.2,
        stats: true,
        ..RenderOptions::default()
    };
    let small = page.render(&fifth);
    assert_eq!(small.stats.expect("asked").images_decoded, 0, "the half's");
    let alone = plain.page(0).expect("a page").render(&fifth);
    assert!(small.data == alone.data, "a fifth of the scale");
    assert_eq!(
        small.warnings, alone.warnings,
        "a draw that asks for a half itself"
    );

    let at_open = budgeted(bytes, budget);
    let (opened, stats) = counted(&at_open.page(0).expect("a page"), 72.0);
    assert_eq!(stats.jpeg_samples_transformed, half, "at open too");
    same_render(&opened, &first, "a budget stated at open");

    let jpx = two_level_codestream();
    let bytes = covered(jpx_image(32, 24, &jpx));
    let plain = Document::open(bytes.clone()).expect("it opens");
    let (unbudgeted, stats) = counted(&plain.page(0).expect("a page"), 72.0);
    assert_eq!(stats.jpx_levels_discarded, 0, "no budget: whole");
    assert!(!says_reduced(&unbudgeted), "{:?}", unbudgeted.warnings);
    for (budget, discard, factor, side) in [(1_024, 1, 2, (16, 12)), (639, 2, 4, (8, 6))] {
        let (width, height, samples) = jpx_at(&jpx, discard);
        assert_eq!((width, height), side);
        let document = cache_only(bytes.clone(), budget);
        let page = document.page(0).expect("a page");
        let mut says = unbudgeted.warnings.clone();
        says.push(RenderWarning::ReducedImage {
            name: "J".to_string(),
            factor,
        });
        let (first, stats) = counted(&page, 72.0);
        assert_eq!(stats.images_decoded, 1, "under {budget}: {stats:?}");
        assert_eq!(
            stats.jpx_levels_discarded,
            u64::from(discard),
            "under {budget}"
        );
        assert_eq!(first.warnings, says, "under {budget}");
        let held = 3 * u64::from(width * height) + ENTRY_BYTES + ENTRY_BYTES;
        assert_eq!(stats.cache_bytes_held, held, "under {budget}");
        assert!(held <= budget);
        let raw = drawn_raw(width, height, &samples, 1.0);
        assert!(
            first.data == raw.data,
            "under {budget}: the reduced samples"
        );
        assert!(
            first.data != unbudgeted.data,
            "under {budget}: not every sample's"
        );
        let (again, stats) = counted(&page, 72.0);
        assert_eq!(stats.images_decoded, 0, "under {budget}: {stats:?}");
        same_render(&again, &first, &format!("again under {budget}"));
    }
    // A tenth of the scale, 10 pixels a side, asks for one level dropped of
    // its own: the decode the budget of 1 024 took the page to.
    let document = cache_only(bytes, 1_024);
    let page = document.page(0).expect("a page");
    let _ = counted(&page, 72.0);
    let tenth = RenderOptions {
        scale: 0.1,
        stats: true,
        ..RenderOptions::default()
    };
    let small = page.render(&tenth);
    assert_eq!(
        small.stats.expect("asked").images_decoded,
        0,
        "one level down's"
    );
    let alone = plain.page(0).expect("a page").render(&tenth);
    assert!(small.data == alone.data, "a tenth of the scale");
    assert_eq!(small.warnings, alone.warnings);
}

/// **An image no reduction fits is drawn as it always was, and not kept.**
/// Under a budget of 200 bytes, 136 beside an entry's 64, the 64 by 48 JPEG's
/// eighth is 8 by 6 samples, 144 bytes, and the two-level codestream's two
/// levels down are 8 by 6 too: no decode fits. Each image is decoded at the
/// size it is drawn at, at every render, draws what a document with no
/// budget draws and says what it says, and only its frame, 64 bytes, is
/// kept.
#[test]
fn an_image_no_reduction_fits_is_drawn_as_it_always_was() {
    for (label, bytes) in [
        ("the JPEG", covered(jpeg_image(64, 48))),
        (
            "the codestream",
            covered(jpx_image(32, 24, &two_level_codestream())),
        ),
    ] {
        let plain = Document::open(bytes.clone())
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        let document = cache_only(bytes, 200);
        let page = document.page(0).expect("a page");
        for pass in ["first", "second"] {
            let (bitmap, stats) = counted(&page, 72.0);
            assert_eq!(stats.images_decoded, 1, "{label}, {pass}: {stats:?}");
            assert_eq!(stats.jpx_levels_discarded, 0, "{label}, {pass}");
            assert_eq!(
                stats.cache_bytes_held, ENTRY_BYTES,
                "{label}, {pass}: its frame alone"
            );
            same_render(&bitmap, &plain, &format!("{label}, {pass} render"));
        }
    }
}

/// A grey JPEG of `width` by `height` from the engine's own encoder.
fn grey_jpeg(width: u32, height: u32) -> Vec<u8> {
    use tinker_pdf_filters::{JpegOptions, JpegSource, JpegSourceColour};

    let samples: Vec<u8> = (0..width * height).map(|i| (i * 29 % 256) as u8).collect();
    tinker_pdf_filters::jpeg_encode(
        &JpegSource {
            width,
            height,
            colour: JpegSourceColour::Gray,
            stride: width as usize,
            data: &samples,
        },
        &JpegOptions::default(),
    )
    .expect("the raster encodes")
}

/// **An image no draw reduces is decoded whole under any budget.** Under the
/// budget of 4 096 that takes the 64 by 48 JPEG to a half, a raw image of its
/// size, the JPEG with a soft mask (11.6.5.3), a grey JPEG whose samples are
/// indices into a palette (8.6.6.3) and the JPEG drawn inline — none of
/// which a draw's size reduces, and the last of which has no object to be
/// kept under — each draw what a document with no budget draws and say what
/// it says, and none says it was reduced.
#[test]
fn an_image_no_draw_reduces_is_decoded_whole_under_any_budget() {
    let jpeg = jpeg_bytes(64, 48);
    let rgb = "/Type /XObject /Subtype /Image /Width 64 /Height 48 /ColorSpace /DeviceRGB \
               /BitsPerComponent 8";
    let mask: Vec<u8> = (0..64 * 48).map(|i| (i % 256) as u8).collect();
    let masked = document(
        &[
            stream(&format!("{rgb} /Filter /DCTDecode /SMask 4 0 R"), &jpeg),
            stream(
                "/Type /XObject /Subtype /Image /Width 64 /Height 48 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &mask,
            ),
        ],
        &[(
            "<< /XObject << /J 3 0 R >> >>",
            "q 100 0 0 100 0 0 cm /J Do Q\n",
        )],
    );
    let palette: String = (0..256u32)
        .map(|i| format!("{:02X}{:02X}{:02X}", i, 255 - i, i / 2))
        .collect();
    let indexed = covered(stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 64 /Height 48 \
             /ColorSpace [/Indexed /DeviceRGB 255 <{palette}>] /BitsPerComponent 8 \
             /Filter /DCTDecode"
        ),
        &grey_jpeg(64, 48),
    ));
    let hex: String = jpeg.iter().map(|byte| format!("{byte:02X}")).collect();
    let inline_content = format!(
        "q 100 0 0 100 0 0 cm BI /W 64 /H 48 /CS /RGB /BPC 8 /F [/AHx /DCT] ID {hex}> EI Q\n"
    );
    let inline = document(&[], &[("<< >>", inline_content.as_str())]);
    for (label, bytes) in [
        ("a raw image", covered(rgb_image(64, 48, 3))),
        ("a soft-masked JPEG", masked),
        ("a JPEG of indices", indexed),
        ("an inline JPEG", inline),
    ] {
        let plain = Document::open(bytes.clone())
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        let document = cache_only(bytes, 4_096);
        for pass in ["first", "second"] {
            let bitmap = document
                .page(0)
                .expect("a page")
                .render(&RenderOptions::default());
            assert!(
                !says_reduced(&bitmap),
                "{label}, {pass}: {:?}",
                bitmap.warnings
            );
            same_render(&bitmap, &plain, &format!("{label}, {pass} render"));
        }
    }
}

/// **Every part of a page under a budget is drawn from the page's reduced
/// decode** (ruling 5): which reduction a budget takes an image to is read
/// off its whole decode at that reduction, never the part a canvas shows, so
/// each tile of the 100-pixel page — 37 by 23, the last row and column
/// partial — is byte-equal to its rectangle of the page and says what the
/// page says, the codestream's windows included; and the page drawn in bands
/// from its display list on three threads is the page, said once.
#[test]
fn every_part_of_a_page_under_a_budget_draws_the_pages_reduced_decode() {
    for (label, bytes, budget) in [
        ("the JPEG", covered(jpeg_image(64, 48)), 4_096),
        (
            "the codestream",
            covered(jpx_image(32, 24, &two_level_codestream())),
            1_024,
        ),
    ] {
        let whole = cache_only(bytes.clone(), budget)
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        assert!(says_reduced(&whole), "{label}: {:?}", whole.warnings);
        let document = cache_only(bytes.clone(), budget);
        let page = document.page(0).expect("a page");
        for y in (0..whole.height).step_by(23) {
            for x in (0..whole.width).step_by(37) {
                let region =
                    PixelRegion::new(x, y, 37.min(whole.width - x), 23.min(whole.height - y));
                let tile = page.render(&RenderOptions {
                    region: Some(region),
                    ..RenderOptions::default()
                });
                assert!(
                    tile.data == cut(&whole, region),
                    "{label}: the tile at ({x}, {y}) is not the page under it"
                );
                assert_eq!(tile.warnings, whole.warnings, "{label}: ({x}, {y})");
            }
        }
        let banded = cache_only(bytes, budget)
            .page(0)
            .expect("a page")
            .display_list()
            .render(&RenderOptions {
                parallelism: Parallelism::Threads(3),
                ..RenderOptions::default()
            });
        same_render(&banded, &whole, &format!("{label} in bands"));
    }
}
