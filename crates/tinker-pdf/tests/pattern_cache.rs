//! A tiling pattern's cell kept between fills and pages in the document's
//! cache (`docs/design/performance.md`, P-16, and row P-23), counted through
//! MS-01's tally (`Stats::tiling_cells_rasterised`): under a stated budget a
//! cell is rasterised once for each distinct key its fills ask for — its
//! pattern, the exact device matrix and sub-pixel phase it is drawn at, its
//! buffer and how it is drawn — however many fills and pages ask, a
//! `PaintType 2` cell serving every colour it is filled in; past the budget
//! every fill rasterises, and every page draws what a document with no budget
//! draws.
//!
//! Every count expected here is derived beside it from what the document
//! holds: a fill of a tiling pattern asks for its cell once, whatever the
//! lattice repeats it over.

use tinker_pdf::{Bitmap, CacheLimits, Document, OpenOptions, RenderOptions, Stats};

/// The fills each pattern takes on a page of [`patterned`].
const FILLS: usize = 10;

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

/// The coloured pattern, object 3: a 10-point cell of a red square and a blue
/// triangle, its own resources empty.
fn coloured() -> Vec<u8> {
    stream(
        "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
         /XStep 10 /YStep 10 /Resources << >>",
        b"1 0 0 rg 1 1 4 4 re f 0 0 1 rg 6 6 m 9 6 l 7.5 9 l f",
    )
}

/// The uncoloured pattern, object 4: an 8-point cell of a diagonal bar and a
/// dot, a shape the fill colours, which names no resources and so resolves
/// in the page's.
fn uncoloured() -> Vec<u8> {
    stream(
        "/Type /Pattern /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 8 8] \
         /XStep 8 /YStep 8",
        b"0 0 m 2 0 l 8 6 l 8 8 l 6 8 l 0 2 l f 5 1 1.5 1.5 re f",
    )
}

/// A page of [`FILLS`] fills of each pattern on a 100-point square under the
/// media box `media`: the coloured one, and the uncoloured one alternately in
/// red and in blue, each fill a rectangle of its own.
fn page(media: &str, content: usize) -> Vec<u8> {
    format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox {media} \
         /Resources << /Pattern << /P1 3 0 R /P2 4 0 R >> \
         /ColorSpace << /U [/Pattern /DeviceRGB] >> >> /Contents {content} 0 R >>"
    )
    .into_bytes()
}

/// The content of a [`page`]: the fills, each at a place of its own.
fn fills() -> Vec<u8> {
    let mut content = String::new();
    for fill in 0..FILLS {
        let (x, y) = (5 + 9 * fill, 5 + 4 * fill);
        content.push_str(&format!("/Pattern cs /P1 scn {x} {y} 8 30 re f\n"));
        let colour = if fill % 2 == 0 { "1 0 0" } else { "0 0 1" };
        content.push_str(&format!("/U cs {colour} /P2 scn {} {} 30 8 re f\n", y, x));
    }
    content.into_bytes()
}

/// A document of `media.len()` [`page`]s, each under its media box, sharing
/// the two patterns: 1 the catalog, 2 the page tree, 3 and 4 the patterns,
/// then each page and its content.
fn patterned(media: &[&str]) -> Vec<u8> {
    let kids: Vec<String> = (0..media.len())
        .map(|page| format!("{} 0 R", 5 + 2 * page))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            media.len(),
            kids.join(" ")
        )
        .into_bytes(),
        coloured(),
        uncoloured(),
    ];
    for (index, media) in media.iter().enumerate() {
        objects.push(page(media, 6 + 2 * index));
        objects.push(stream("", &fills()));
    }
    pdf(&objects)
}

/// The square media box every page here but the shifted one has.
const SQUARE: &str = "[0 0 100 100]";

/// A media box a quarter of a point from [`SQUARE`]: the page's base matrix,
/// and so every cell's, a quarter of a pixel along at 72 dpi.
const SHIFTED: &str = "[0.25 0 100.25 100]";

/// `page` of `document` rendered at `scale`, counting.
fn counted(document: &Document, page: u32, scale: f64) -> (Bitmap, Stats) {
    let bitmap = document.page(page).expect("a page").render(&RenderOptions {
        scale,
        stats: true,
        ..RenderOptions::default()
    });
    let stats = bitmap.stats.expect("a counted render hands its stats back");
    (bitmap, stats)
}

/// `bytes` opened under a budget of `budget` bytes, stated at open.
fn budgeted(bytes: Vec<u8>, budget: u64) -> Document {
    Document::open_with(
        bytes,
        &OpenOptions::default().with_cache_limits(CacheLimits::new(budget)),
    )
    .expect("the document opens")
}

/// Asserts two renders are one picture: size, every byte and every warning.
#[track_caller]
fn same_render(kept: &Bitmap, plain: &Bitmap, what: &str) {
    assert_eq!(
        (kept.width, kept.height, kept.format),
        (plain.width, plain.height, plain.format),
        "{what}: a different shape"
    );
    assert!(kept.data == plain.data, "{what}: different pixels");
    assert_eq!(kept.warnings, plain.warnings, "{what}: different warnings");
}

/// **Cell rasterisations are at most the distinct keys.** Three pages under
/// one media box, each filling the coloured pattern ten times and the
/// uncoloured one ten times in two colours, rendered once each under a
/// budget, rasterise two cells between them — each pattern's first fill —
/// every other fill finding its pattern's cell kept, the uncoloured one's in
/// either colour; and each page draws what a document with no budget draws,
/// which rasterises a cell at every fill. A page whose media box stands a
/// quarter of a point along draws each cell a quarter of a pixel along, and
/// adds a cell of each pattern; so does a render at twice the scale, a cell
/// of each at its own matrix; and each draws what a document with no budget
/// draws there. The renders again rasterise nothing.
#[test]
fn cell_rasterisations_are_at_most_the_distinct_keys() {
    let bytes = patterned(&[SQUARE, SQUARE, SQUARE, SHIFTED]);
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("the document opens");
    let mut drawn = 0;
    for page in 0..3 {
        let (bitmap, stats) = counted(&document, page, 1.0);
        drawn += stats.tiling_cells_rasterised;
        let (alone, alone_stats) = counted(&plain, page, 1.0);
        assert_eq!(
            alone_stats.tiling_cells_rasterised,
            2 * FILLS as u64,
            "page {page}: with no budget every fill rasterises its cell"
        );
        same_render(&bitmap, &alone, &format!("page {page}"));
    }
    assert_eq!(drawn, 2, "two patterns, one cell each, over three pages");

    let (shifted, stats) = counted(&document, 3, 1.0);
    assert_eq!(
        stats.tiling_cells_rasterised, 2,
        "a quarter of a pixel's phase is a cell of each pattern's own"
    );
    same_render(&shifted, &counted(&plain, 3, 1.0).0, "the shifted page");

    let (larger, stats) = counted(&document, 0, 2.0);
    assert_eq!(
        stats.tiling_cells_rasterised, 2,
        "another matrix is a cell of each pattern's own"
    );
    same_render(&larger, &counted(&plain, 0, 2.0).0, "twice the scale");

    for (page, scale) in [(0, 1.0), (1, 1.0), (2, 1.0), (3, 1.0), (0, 2.0)] {
        let (bitmap, stats) = counted(&document, page, scale);
        assert_eq!(
            stats.tiling_cells_rasterised, 0,
            "page {page} at {scale}x again: {stats:?}"
        );
        same_render(
            &bitmap,
            &counted(&plain, page, scale).0,
            &format!("page {page} at {scale}x again"),
        );
    }
}

/// **Past the budget every fill rasterises, and draws the same page.** Under
/// a budget of 64 bytes, below what one cell is charged, no cell is kept:
/// every fill of every page rasterises its own, twenty a page, the bytes held
/// never pass the budget, and every page draws what a document with no budget
/// draws, byte for byte, twice over.
#[test]
fn under_a_budget_below_one_cell_every_fill_rasterises() {
    let bytes = patterned(&[SQUARE, SQUARE, SQUARE]);
    let budget = 64;
    let document = budgeted(bytes.clone(), budget);
    let plain = Document::open(bytes).expect("the document opens");
    for pass in 0..2 {
        for page in 0..3 {
            let (bitmap, stats) = counted(&document, page, 1.0);
            assert_eq!(
                stats.tiling_cells_rasterised,
                2 * FILLS as u64,
                "pass {pass}, page {page}: nothing kept to find"
            );
            assert!(stats.cache_bytes_held <= budget, "{stats:?}");
            same_render(
                &bitmap,
                &counted(&plain, page, 1.0).0,
                &format!("pass {pass}, page {page}"),
            );
        }
    }
}

/// **A kept cell says again what its drawing reported.** A coloured cell
/// whose content names a font its own resources do not hold and draws an
/// image whose data stops short: the first fill under a budget rasterises it
/// and reports the font and what the image's decode tolerated, and the next
/// fill and the next page, which find it kept, report both again, as a
/// render with no budget, which draws it at every fill, does.
#[test]
fn a_kept_cell_says_again_what_its_drawing_reported() {
    let short_image = {
        let mut image = b"<< /Type /XObject /Subtype /Image /Width 4 /Height 4 \
                          /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode \
                          /Length 6 >>\nstream\n"
            .to_vec();
        // A zlib header and a stored block's header, its data never coming.
        image.extend_from_slice(&[0x78, 0x01, 0x00, 0x10, 0x00, 0xEF]);
        image.extend_from_slice(b"\nendstream");
        image
    };
    let reporting = stream(
        "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
         /XStep 10 /YStep 10 /Resources << /XObject << /Im 4 0 R >> >>",
        b"BT /Missing 6 Tf 1 1 Td (a) Tj ET q 8 0 0 8 1 1 cm /Im Do Q",
    );
    let content = b"/Pattern cs /P1 scn 0 0 40 40 re f /P1 scn 50 50 40 40 re f";
    let page = |contents: usize| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
             /Resources << /Pattern << /P1 3 0 R >> >> /Contents {contents} 0 R >>"
        )
        .into_bytes()
    };
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 2 /Kids [5 0 R 7 0 R] >>".to_vec(),
        reporting,
        short_image,
        page(6),
        stream("", content),
        page(8),
        stream("", content),
    ]);
    let document = budgeted(bytes.clone(), 16 << 20);
    let plain = Document::open(bytes).expect("the document opens");
    for (page, want) in [(0, 1), (1, 0)] {
        let (bitmap, stats) = counted(&document, page, 1.0);
        let (alone, alone_stats) = counted(&plain, page, 1.0);
        assert_eq!(
            alone_stats.tiling_cells_rasterised, 2,
            "page {page}: both fills draw the cell with no budget"
        );
        assert_eq!(
            stats.tiling_cells_rasterised, want,
            "page {page}: the cell drawn once a document"
        );
        assert!(
            !alone.warnings.is_empty(),
            "page {page}: the cell's drawing reports something"
        );
        same_render(&bitmap, &alone, &format!("page {page}"));
    }
}
