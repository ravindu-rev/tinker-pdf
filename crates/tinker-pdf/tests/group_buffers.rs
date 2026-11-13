//! What transparency groups and soft masks hold, counted rather than timed
//! (`docs/design/performance.md`, P-25).
//!
//! A render asked for its stats hands back four pixel counts beside its
//! steps: `group_buffer_pixels`, every pixel of a buffer a transparency group
//! or a soft mask's group allocated for its own pixels while it was open, and
//! `group_bound_pixels`, every pixel of the regions those groups opened over,
//! once a group; `soft_mask_pixels`, every pixel of a soft mask installed, and
//! `soft_mask_bound_pixels`, every pixel of the region of the group it was read
//! from, once a mask. The guards here hold each count to at most 1.1 times its
//! bound, whole page and in tiles, where the bounds are each tile's own
//! regions, and hold the bounds to the boxes the page states, worked out here
//! from its numbers.
//!
//! A tile draws the page's pixels under it throughout (ruling 5): a group's
//! buffer and a soft mask stand in the page's frame, so a smaller buffer is
//! one that holds fewer of the same pixels.

use tinker_pdf::{Bitmap, Document, Page, PixelRegion, RenderOptions, Stats};

/// The committed transparency fixture: a non-isolated group under
/// `Multiply`, an isolated knockout group, and a luminosity soft mask whose
/// group is an axial shading over the box `[10 30 110 76]`, its `/BC` the
/// grey 0.2 and its `/TR` a square, on a 120 by 80 page.
const TRANSPARENCY: &[u8] = include_bytes!("../../../fuzz/corpus/render_page/transparency.pdf");

/// The fixture's soft mask's box, in default user space: `[x0 y0 x1 y1]`.
const MASK_BOX: [f64; 4] = [10.0, 30.0, 110.0, 76.0];

/// The fixture's page height in points, which the view flips about.
const FIXTURE_HEIGHT: f64 = 80.0;

/// A PDF of `objects`, numbered from 1 in order, the first its catalog, with
/// a cross-reference table so that it opens without repair.
fn pdf(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
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
fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// A one-page document `width` by `height` points: the page dictionary's
/// further entries `page_extra`, its resources `resources`, its content
/// `content`, and `objects` after the first four, numbered from 5.
fn page_document(
    width: f64,
    height: f64,
    page_extra: &str,
    resources: &str,
    content: &str,
    objects: &[String],
) -> Vec<u8> {
    let mut all = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] {page_extra}\n\
             /Resources {resources} /Contents 4 0 R >>"
        ),
        stream("", content),
    ];
    all.extend(objects.iter().cloned());
    pdf(&all)
}

/// `page` rendered as `options` asks, counting, over `region`.
fn counted(page: &Page, options: &RenderOptions, region: Option<PixelRegion>) -> (Bitmap, Stats) {
    let bitmap = page.render(&RenderOptions {
        stats: true,
        region,
        ..options.clone()
    });
    let stats = bitmap
        .stats
        .expect("a render asked for its stats hands them back");
    (bitmap, stats)
}

/// The page's four quarters, the last row and column of quarters taking what
/// an odd size leaves.
fn quarters(width: u32, height: u32) -> [PixelRegion; 4] {
    let (half_w, half_h) = (width / 2, height / 2);
    [
        PixelRegion::new(0, 0, half_w, half_h),
        PixelRegion::new(half_w, 0, width - half_w, half_h),
        PixelRegion::new(0, half_h, half_w, height - half_h),
        PixelRegion::new(half_w, half_h, width - half_w, height - half_h),
    ]
}

/// Holds `tile` to the rectangle of `page` under it, byte for byte.
#[track_caller]
fn tile_is_the_page(tile: &Bitmap, page: &Bitmap, region: PixelRegion, what: &str) {
    assert_eq!(
        (tile.width, tile.height, tile.format),
        (region.width, region.height, page.format),
        "{what}: the tile's shape"
    );
    let components = page.components();
    for row in 0..region.height as usize {
        let from = (region.y as usize + row) * page.stride + region.x as usize * components;
        let want = &page.data[from..from + region.width as usize * components];
        let got =
            &tile.data[row * tile.stride..row * tile.stride + region.width as usize * components];
        assert!(
            got == want,
            "{what}: row {row} of the tile is not the page's"
        );
    }
}

/// The pixels of the box `[x0 y0 x1 y1]` of default user space on a page
/// `height` points tall drawn at `scale` pixels a point, clipped to `region`:
/// the fewest a region holding the box can have, the box rounded inward, and
/// the most, the box rounded outward and widened by a pixel on every side.
fn box_pixels(bbox: [f64; 4], height: f64, scale: f64, region: PixelRegion) -> (u64, u64) {
    let [x0, y0, x1, y1] = bbox;
    // The view flips the page about its height, so the box's top is `y1`.
    let (left, right) = (x0 * scale, x1 * scale);
    let (top, bottom) = ((height - y1) * scale, (height - y0) * scale);
    let (rx0, ry0) = (f64::from(region.x), f64::from(region.y));
    let (rx1, ry1) = (
        rx0 + f64::from(region.width),
        ry0 + f64::from(region.height),
    );
    let span = |lo: f64, hi: f64, at: f64, to: f64| (hi.min(to) - lo.max(at)).max(0.0) as u64;
    let inner =
        span(left.ceil(), right.floor(), rx0, rx1) * span(top.ceil(), bottom.floor(), ry0, ry1);
    let outer = span(left.floor() - 1.0, right.ceil() + 1.0, rx0, rx1)
        * span(top.floor() - 1.0, bottom.ceil() + 1.0, ry0, ry1);
    (inner, outer)
}

/// Holds a render's soft-mask pixels to at most 1.1 times their bound.
#[track_caller]
fn soft_masks_within_their_bound(stats: &Stats, what: &str) {
    assert!(
        stats.soft_mask_pixels * 10 <= stats.soft_mask_bound_pixels * 11,
        "{what}: {} soft-mask pixels against a bound of {}",
        stats.soft_mask_pixels,
        stats.soft_mask_bound_pixels
    );
}

/// **A soft mask is kept at its box** (P-25.2): the committed transparency
/// fixture's luminosity mask, whose `/BC` is grey 0.2 and so reads a pale
/// value outside its group's box, installs at most 1.1 times the pixels of
/// its box clipped to the region, at 150 dpi, whole page and in each quarter
/// of it and in a strip below the box that holds none of it — where the mask
/// is of no pixels and still reads its backdrop at every one. The bound is
/// the box the page states, rounded either way and clipped to the region, and
/// each tile draws the page's pixels under it.
#[test]
fn a_soft_mask_is_kept_at_its_box() {
    let document = Document::open(TRANSPARENCY.to_vec()).expect("the fixture opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::at_dpi(150.0);
    let scale = 150.0 / 72.0;

    let (whole, stats) = counted(&page, &options, None);
    let full = PixelRegion::new(0, 0, whole.width, whole.height);
    let (inner, outer) = box_pixels(MASK_BOX, FIXTURE_HEIGHT, scale, full);
    assert!(
        (inner..=outer).contains(&stats.soft_mask_bound_pixels),
        "the whole page: the mask's group's region holds {} pixels, where its box holds {inner} \
         to {outer}",
        stats.soft_mask_bound_pixels
    );
    soft_masks_within_their_bound(&stats, "the whole page");
    assert!(
        stats.soft_mask_bound_pixels < u64::from(whole.width) * u64::from(whole.height),
        "the whole page: the box is smaller than the page"
    );

    let mut regions = quarters(whole.width, whole.height).to_vec();
    // Below the box, which ends at 30 points up a page of 80: the bottom
    // fifth of the picture.
    regions.push(PixelRegion::new(
        0,
        whole.height * 4 / 5,
        whole.width,
        whole.height / 5,
    ));
    for region in regions {
        let what = format!("the tile {region:?}");
        let (tile, stats) = counted(&page, &options, Some(region));
        tile_is_the_page(&tile, &whole, region, &what);
        let (inner, outer) = box_pixels(MASK_BOX, FIXTURE_HEIGHT, scale, region);
        assert!(
            (inner..=outer).contains(&stats.soft_mask_bound_pixels),
            "{what}: the mask's group's region holds {} pixels, where its box holds {inner} to \
             {outer}",
            stats.soft_mask_bound_pixels
        );
        soft_masks_within_their_bound(&stats, &what);
    }
}

/// A page of three soft masks, each over its own box: a luminosity mask
/// whose backdrop is white, one whose backdrop is the black default, and an
/// alpha mask, each set before a fill of the whole page, the white one's box
/// hanging off the page's left edge.
fn three_masks_page() -> Vec<u8> {
    let content = "q /Pale gs 0.9 0.2 0.1 rg 0 0 200 120 re f Q\n\
                   q /Dark gs 0.1 0.2 0.9 rg 0 0 200 120 re f Q\n\
                   q /Shape gs 0.2 0.8 0.3 rg 0 0 200 120 re f Q";
    let group = "/Type /XObject /Subtype /Form /Group << /S /Transparency /CS /DeviceGray >>";
    page_document(
        200.0,
        120.0,
        "",
        "<< /ExtGState << \
           /Pale << /SMask << /S /Luminosity /G 5 0 R /BC [1] >> >> \
           /Dark << /SMask << /S /Luminosity /G 6 0 R >> >> \
           /Shape << /SMask << /S /Alpha /G 7 0 R >> >> >> >>",
        content,
        &[
            stream(
                &format!("{group} /BBox [-30 10 60 70]"),
                "0.5 g -30 10 50 60 re f",
            ),
            stream(
                &format!("{group} /BBox [70 20 130 100]"),
                "1 g 80 30 40 40 re f",
            ),
            stream(
                &format!("{group} /BBox [140 5 190 115]"),
                "0 g 150 10 30 90 re f",
            ),
        ],
    )
}

/// The boxes of [`three_masks_page`]'s masks.
const THREE_BOXES: [[f64; 4]; 3] = [
    [-30.0, 10.0, 60.0, 70.0],
    [70.0, 20.0, 130.0, 100.0],
    [140.0, 5.0, 190.0, 115.0],
];

/// **Every kind of soft mask is kept at its box**: a pale luminosity mask,
/// a black one and an alpha mask, each installed over its group's region and
/// no more, at two scales, whole and in tiles; the regions together are the
/// three boxes the page states, clipped to what is drawn.
#[test]
fn every_kind_of_soft_mask_is_kept_at_its_box() {
    let document = Document::open(three_masks_page()).expect("the page opens");
    let page = document.page(0).expect("a page");
    for dpi in [72.0, 150.0] {
        let options = RenderOptions::at_dpi(dpi);
        let scale = dpi / 72.0;
        let (whole, stats) = counted(&page, &options, None);
        let full = PixelRegion::new(0, 0, whole.width, whole.height);
        let mut regions = quarters(whole.width, whole.height).to_vec();
        regions.push(full);
        for region in regions {
            let what = format!("{dpi} dpi, {region:?}");
            let (tile, stats) = if region == full {
                (whole.clone(), stats)
            } else {
                counted(&page, &options, Some(region))
            };
            tile_is_the_page(&tile, &whole, region, &what);
            let (inner, outer) = THREE_BOXES.iter().fold((0, 0), |(inner, outer), bbox| {
                let (i, o) = box_pixels(*bbox, 120.0, scale, region);
                (inner + i, outer + o)
            });
            assert!(
                (inner..=outer).contains(&stats.soft_mask_bound_pixels),
                "{what}: the masks' groups' regions hold {} pixels, where their boxes hold \
                 {inner} to {outer}",
                stats.soft_mask_bound_pixels
            );
            assert_eq!(
                stats.soft_mask_pixels, stats.soft_mask_bound_pixels,
                "{what}: each mask is its group's region"
            );
            soft_masks_within_their_bound(&stats, &what);
        }
    }
}

/// The fixture's groups' boxes: `/Grp`, non-isolated under `Multiply`;
/// `/Knock`, isolated and knockout; and the soft mask's group, whose buffer
/// is a group buffer too.
const FIXTURE_GROUPS: [[f64; 4]; 3] = [
    [0.0, 0.0, 58.0, 56.0],
    [58.0, 0.0, 118.0, 56.0],
    [10.0, 30.0, 110.0, 76.0],
];

/// The resources the generated pages of groups share: the forms `/G1` to
/// `/G4` at objects 5 to 8, and the states their content sets.
const GROUP_RESOURCES: &str = "<< /XObject << /G1 5 0 R /G2 6 0 R /G3 7 0 R /G4 8 0 R >> \
     /ExtGState << /Mul << /BM /Multiply /ca 0.8 >> /Scr << /BM /Screen >> \
     /Half << /ca 0.5 >> >> >>";

/// A form XObject over `bbox`, a transparency group isolated or not and
/// knockout or not, drawing `content`.
fn group_form(bbox: [f64; 4], isolated: bool, knockout: bool, content: &str) -> String {
    let [x0, y0, x1, y1] = bbox;
    stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [{x0} {y0} {x1} {y1}] \
             /Group << /S /Transparency /I {isolated} /K {knockout} >>"
        ),
        content,
    )
}

/// A page of groups: its name, its bytes, its height in points, and the boxes
/// its groups' regions stand over — each group's `/BBox` within the boxes of
/// the groups that enclose it, in default user space.
struct Grouped {
    name: &'static str,
    bytes: Vec<u8>,
    height: f64,
    boxes: Vec<[f64; 4]>,
}

/// Two overlapping rectangles at half alpha, the first's corner at `(x, y)`:
/// what each generated group draws, so that its backdrop and its knockout
/// both show.
fn two_at(x: f64, y: f64) -> String {
    format!(
        "/Half gs 0.1 0.3 0.9 rg {x} {y} 40 30 re f 0.8 0.1 0.2 rg {} {} 40 30 re f",
        x + 20.0,
        y + 12.0
    )
}

/// A form no page draws, filling the slot `at` of [`GROUP_RESOURCES`].
fn unused(at: usize) -> String {
    group_form(
        [0.0, 0.0, 1.0, 1.0],
        true,
        false,
        &format!("0 g 0 0 1 1 re f % object {at}"),
    )
}

/// A 200 by 120 page over a yellow backdrop, drawing `content` with
/// [`GROUP_RESOURCES`] and `forms` as objects 5 to 8.
fn grouped_page(
    name: &'static str,
    page_extra: &str,
    content: &str,
    forms: [String; 4],
    boxes: Vec<[f64; 4]>,
) -> Grouped {
    let content = format!("0.9 0.8 0.1 rg 0 0 200 120 re f\n{content}");
    Grouped {
        name,
        bytes: page_document(200.0, 120.0, page_extra, GROUP_RESOURCES, &content, &forms),
        height: 120.0,
        boxes,
    }
}

/// The generated pages `group_buffers_hold_their_clipped_bounds_once` reads:
/// a non-isolated group under a blend mode; an isolated and a non-isolated
/// knockout group; groups nested three deep, the innermost reaching past both
/// boxes around it and past the page; and a page that declares a page group,
/// which is the page's own canvas and no buffer, holding a non-isolated group.
fn grouped_pages() -> Vec<Grouped> {
    vec![
        grouped_page(
            "a non-isolated group under a blend mode",
            "",
            "q /Mul gs /G1 Do Q",
            [
                group_form([20.0, 20.0, 120.0, 80.0], false, false, &two_at(30.0, 30.0)),
                unused(6),
                unused(7),
                unused(8),
            ],
            vec![[20.0, 20.0, 120.0, 80.0]],
        ),
        grouped_page(
            "an isolated and a non-isolated knockout group",
            "",
            "q /G1 Do Q q /Mul gs /G2 Do Q",
            [
                group_form([10.0, 10.0, 90.0, 70.0], true, true, &two_at(15.0, 15.0)),
                group_form(
                    [100.0, 30.0, 190.0, 110.0],
                    false,
                    true,
                    &two_at(110.0, 40.0),
                ),
                unused(7),
                unused(8),
            ],
            vec![[10.0, 10.0, 90.0, 70.0], [100.0, 30.0, 190.0, 110.0]],
        ),
        grouped_page(
            "groups nested three deep",
            "",
            "q /Mul gs /G1 Do Q",
            [
                group_form(
                    [10.0, 10.0, 190.0, 110.0],
                    false,
                    false,
                    &format!("{} /G2 Do", two_at(15.0, 15.0)),
                ),
                group_form(
                    [30.0, 20.0, 150.0, 100.0],
                    true,
                    true,
                    "0.9 0.1 0.1 rg 40 30 50 50 re f /G3 Do",
                ),
                group_form(
                    [60.0, -10.0, 220.0, 60.0],
                    false,
                    false,
                    "/Scr gs 0.1 0.1 0.9 rg 70 -5 140 40 re f",
                ),
                unused(8),
            ],
            vec![
                [10.0, 10.0, 190.0, 110.0],
                [30.0, 20.0, 150.0, 100.0],
                [60.0, 20.0, 150.0, 60.0],
            ],
        ),
        grouped_page(
            "a page group holding a non-isolated group",
            "/Group << /S /Transparency /CS /DeviceGray /I true >>",
            "q /Mul gs /G4 Do Q",
            [
                unused(5),
                unused(6),
                unused(7),
                group_form([40.0, 30.0, 160.0, 90.0], false, false, &two_at(50.0, 40.0)),
            ],
            vec![[40.0, 30.0, 160.0, 90.0]],
        ),
    ]
}

/// Holds a render's group-buffer pixels to at most 1.1 times their bound, and
/// the bound to `boxes`, the boxes the page states, rounded either way and
/// clipped to `region`.
#[track_caller]
fn group_buffers_within_their_bound(
    stats: &Stats,
    boxes: &[[f64; 4]],
    height: f64,
    scale: f64,
    region: PixelRegion,
    what: &str,
) {
    let (inner, outer) = boxes.iter().fold((0, 0), |(inner, outer), bbox| {
        let (i, o) = box_pixels(*bbox, height, scale, region);
        (inner + i, outer + o)
    });
    assert!(
        (inner..=outer).contains(&stats.group_bound_pixels),
        "{what}: the groups' regions hold {} pixels, where their boxes hold {inner} to {outer}",
        stats.group_bound_pixels
    );
    assert!(
        stats.group_buffer_pixels * 10 <= stats.group_bound_pixels * 11,
        "{what}: {} group-buffer pixels against a bound of {}",
        stats.group_buffer_pixels,
        stats.group_bound_pixels
    );
}

/// **Group buffers hold their clipped bounds once** (P-25.3): a group's
/// buffer is its region and nothing else — no copy of its backdrop for a
/// non-isolated group, no snapshot of its initial state for a knockout one,
/// the canvas beneath read where it lies — over the committed transparency
/// fixture and generated pages of a non-isolated group under a blend mode, an
/// isolated and a non-isolated knockout group, groups nested three deep and a
/// page group, at 72 and 150 dpi, whole and in each quarter of the page:
/// group-buffer pixels at most 1.1 times the groups' regions, the regions the
/// boxes the page states clipped to what is drawn, and each tile the page's
/// pixels under it.
#[test]
fn group_buffers_hold_their_clipped_bounds_once() {
    let mut pages = vec![Grouped {
        name: "the committed transparency fixture",
        bytes: TRANSPARENCY.to_vec(),
        height: FIXTURE_HEIGHT,
        boxes: FIXTURE_GROUPS.to_vec(),
    }];
    pages.extend(grouped_pages());
    for grouped in &pages {
        let document = Document::open(grouped.bytes.clone()).expect("the page opens");
        let page = document.page(0).expect("a page");
        for dpi in [72.0, 150.0] {
            let options = RenderOptions::at_dpi(dpi);
            let scale = dpi / 72.0;
            let (whole, stats) = counted(&page, &options, None);
            let full = PixelRegion::new(0, 0, whole.width, whole.height);
            let what = format!("{}, {dpi} dpi, the whole page", grouped.name);
            assert!(stats.group_bound_pixels > 0, "{what}: groups were opened");
            group_buffers_within_their_bound(
                &stats,
                &grouped.boxes,
                grouped.height,
                scale,
                full,
                &what,
            );
            for region in quarters(whole.width, whole.height) {
                let what = format!("{}, {dpi} dpi, {region:?}", grouped.name);
                let (tile, stats) = counted(&page, &options, Some(region));
                tile_is_the_page(&tile, &whole, region, &what);
                group_buffers_within_their_bound(
                    &stats,
                    &grouped.boxes,
                    grouped.height,
                    scale,
                    region,
                    &what,
                );
            }
        }
    }
}
