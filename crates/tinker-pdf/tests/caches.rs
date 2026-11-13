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
//!   that reduction against;
//! - a decoded image is charged its samples, its alpha and its two-colour
//!   bytes at the decoded size, the words of what its decode reported, and
//!   the cache's 64 bytes beside them; a raw `/DeviceRGB` image of `w` by `h`
//!   is held as `3wh` bytes of samples and nothing else;
//! - every lookup takes the next number of the cache's one counter, and a
//!   fill that would pass the budget evicts the entries whose last lookup is
//!   oldest until it fits.

use tinker_pdf::{
    Bitmap, CacheLimits, Document, OpenOptions, Page, Parallelism, PixelRegion, RenderOptions,
    RenderWarning, Stats, Usage,
};

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
    use tinker_pdf_filters::{JpegOptions, JpegSampling, JpegSource, JpegSourceColour};

    let samples: Vec<u8> = (0..width * height * 3)
        .map(|i| (i * 53 % 251) as u8)
        .collect();
    let data = tinker_pdf_filters::jpeg_encode(
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
    .expect("the raster encodes");
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"
        ),
        &data,
    )
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

/// `bytes` opened under a budget of `budget` bytes.
fn budgeted(bytes: Vec<u8>, budget: u64) -> Document {
    Document::open_with(
        bytes,
        &OpenOptions::default().with_cache_limits(CacheLimits::new(budget)),
    )
    .expect("the fixture opens")
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
/// of its own, under a budget that holds two of them: per render, its image
/// hits and misses, the entries it evicted and the most the cache held.
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
    let document = budgeted(bytes, budget);
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
/// document is the same document.
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
/// alone keeps the image and compiles the transforms at every render — the
/// transforms, asked for first, are evicted for the image or never fit
/// beside it — the bytes held never pass the budget, and every render draws
/// what a document with no budget draws.
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
    let document = budgeted(bytes.clone(), budget);
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
