//! The retained page's answers: a recording is the renderer's picture only if
//! it was made with the renderer's answers to the interpreter's questions.
//!
//! `determinism.rs`'s `a_display_list_replays_every_fingerprinted_page_byte_for_byte`
//! holds a replay equal to a direct render over every fingerprinted page, and
//! `render_regions.rs`'s `a_display_list_tiles_as_the_page_does` holds its
//! tiles equal to the page's. Neither can see the property this file is
//! about, because **on every one of those pages the renderer accepts every
//! group and every soft mask it is offered** — so a recorder that simply
//! accepted everything would pass both. The pages here are the ones where the
//! renderer says no: past the group-buffer budget, and inside hidden optional
//! content. On each, what the interpreter does next depends on the answer,
//! and a recording made with any other answer holds states the renderer
//! never sees.

use tinker_pdf::{CancelToken, Document, RenderOptions, RenderWarning};

mod render_support;

/// A one-page document of `width` x `height` points around `content`, with
/// `/G` a transparency-group form drawing a green square at half opacity and
/// `/Off` an optional-content group the default configuration hides.
fn pdf(content: &str, width: u32, height: u32) -> Vec<u8> {
    let form = "/GS0 gs 0 0.6 0 rg 2 2 6 6 re f";
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [6 0 R] /D << /OFF [6 0 R] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Resources << /XObject << /G 5 0 R >> /Properties << /Off 6 0 R >>\n\
                 /ExtGState << /GS0 << /ca 0.5 >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 10]\n\
   /Group << /S /Transparency >> /Resources << /ExtGState << /GS0 << /ca 0.5 >> >> >>\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
6 0 obj\n<< /Type /OCG /Name (Hidden) >>\nendobj\n",
        content.len(),
        form.len()
    );
    out.push_str("trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    out.into_bytes()
}

/// Asserts a recording of `bytes`' first page replays to what the page
/// renders, at two scales, and returns the direct render's warnings.
#[track_caller]
fn replays_as_it_renders(bytes: Vec<u8>, what: &str) -> Vec<RenderWarning> {
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    let mut warnings = Vec::new();
    for scale in [1.0, 2.5] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        let direct = page.render(&options);
        let replayed = list.render(&options);
        assert_eq!(
            (replayed.width, replayed.height),
            (direct.width, direct.height),
            "{what} at {scale}x: a different size"
        );
        let differing = replayed
            .data
            .iter()
            .zip(direct.data.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "{what} at {scale}x: bytes of the replay differ"
        );
        assert_eq!(
            replayed.warnings, direct.warnings,
            "{what} at {scale}x: the replay reported differently"
        );
        warnings = direct.warnings;
    }
    warnings
}

/// **Past the group-buffer budget**, the renderer declines every further
/// group, and the interpreter then draws a declined group's content straight
/// onto the page with the state it was invoked under rather than with the
/// alphas and blend mode reset (11.6.6). A recording made with the group
/// accepted holds the reset states inside a bracket the renderer then
/// declines — and a replay can only drop what is inside such a bracket, so
/// the green square past the budget, at `x = 20`, would be missing.
#[test]
fn a_page_past_the_group_budget_replays_as_it_renders() {
    // 2 000 is `MAX_GROUP_BUFFERS`; the page spends them all, then asks once
    // more somewhere that shows.
    let mut content = String::new();
    for _ in 0..2_000 {
        content.push_str("/G Do\n");
    }
    content.push_str("q 1 0 0 1 20 0 cm /G Do Q\n");
    let warnings = replays_as_it_renders(pdf(&content, 40, 12), "the budget page");
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::GroupBudgetSpent { .. })),
        "the page really is past the budget: {warnings:?}"
    );
}

/// **Inside hidden optional content** the renderer declines a group, and a
/// soft mask it does not: the mask is graphics state that outlives the scope.
/// Nothing hidden paints, so the pixels cannot tell a wrong answer here; the
/// bookkeeping can, because a group the recording entered and the renderer
/// declined leaves an `EndGroup` with nothing to end. The page then draws
/// after the scope, where a skewed bracket would show.
#[test]
fn a_group_inside_hidden_content_replays_as_it_renders() {
    let content = "/OC /Off BDC /G Do /G Do EMC \
                   0.9 0.1 0.1 rg 11 1 8 8 re f /G Do";
    replays_as_it_renders(pdf(content, 30, 12), "the hidden-group page");
}

/// A cancelled render and a cancelled replay are the same short page, and say
/// so the same way.
#[test]
fn a_cancelled_replay_is_a_cancelled_render() {
    let document = Document::open(pdf("/G Do 0 0 1 rg 1 1 8 8 re f", 12, 12)).expect("opens");
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    let cancel = CancelToken::new();
    cancel.cancel();
    let options = RenderOptions {
        cancel: Some(cancel),
        ..RenderOptions::default()
    };
    let direct = page.render(&options);
    let replayed = list.render(&options);
    assert_eq!(replayed.data, direct.data);
    assert_eq!(replayed.warnings, direct.warnings);
    assert!(direct.warnings.contains(&RenderWarning::Cancelled));
}

/// What the list says about itself, and that it can cross threads: a viewer
/// records on one and draws on another.
#[test]
fn a_display_list_is_a_plain_value() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<tinker_pdf::DisplayList>();

    let document = Document::open(pdf("0 0 1 rg 1 1 8 8 re f /G Do", 12, 12)).expect("opens");
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    assert_eq!(list.page_index(), 0);
    assert!(list.is_retained(), "a page of five calls is kept");
    assert!(!list.is_empty());
    assert!(
        list.len() >= 4,
        "a fill, a form, a group and what is inside them: {}",
        list.len()
    );
    assert!(format!("{list:?}").contains("DisplayList"));

    let empty = Document::open(pdf("", 12, 12)).expect("opens");
    assert!(empty.page(0).expect("a page").display_list().is_empty());
}

// ---- the warnings a replay reports are its own ------------------------------
//
// A retained page keeps its resources so a decoded image or a glyph outline is
// paid for once; those resources also hold what the page had to tolerate — a
// font the interpretation could not resolve, an image that decoded with damage,
// what a pattern cell met. A replay reports what *it* met, as a direct render
// does with resources of its own, and not what an earlier render or the
// recording met.

/// A one-page document of `width` x `height` points around `content`, with
/// `resources` and further objects numbered from 5.
fn page_with(
    content: &str,
    width: u32,
    height: u32,
    resources: &str,
    objects: &[&[u8]],
) -> Vec<u8> {
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Resources {resources} /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    )
    .into_bytes();
    for (index, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", index + 5).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 5
        )
        .as_bytes(),
    );
    out
}

/// A stream object around `body`.
fn stream_object(dict: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
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

/// Asserts a list's render reports what a direct render reports, for each
/// set of options in turn and in that order — so an earlier render of the
/// list is in front of every later one.
#[track_caller]
fn reports_as_it_renders(bytes: Vec<u8>, asks: &[RenderOptions], what: &str) {
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    for (index, options) in asks.iter().enumerate() {
        let direct = page.render(options);
        let replayed = list.render(options);
        assert_eq!(
            replayed.data, direct.data,
            "{what}, render {index}: the pixels"
        );
        assert_eq!(
            replayed.warnings, direct.warnings,
            "{what}, render {index}: the replay reported differently"
        );
    }
}

/// **A font the interpretation could not resolve**, which the list met once
/// when it was recorded: a cancelled replay draws nothing and must not report
/// it, and an uncancelled one must.
#[test]
fn a_cancelled_replay_reports_no_font_it_never_reached() {
    let bytes = page_with(
        "BT /Nope 12 Tf 10 10 Td (Hi) Tj ET 0 0 1 rg 1 1 8 8 re f",
        40,
        40,
        "<< >>",
        &[],
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = RenderOptions {
        cancel: Some(cancel),
        ..RenderOptions::default()
    };
    reports_as_it_renders(
        bytes,
        &[cancelled.clone(), RenderOptions::default(), cancelled],
        "the missing-font page",
    );
}

/// **What a pattern cell met** is reported by the render that drew the cell:
/// a region that misses the patterned fill draws no cell, so a direct render
/// of it says nothing about the font the cell could not resolve, and a replay
/// of it after a full render must not repeat what the full render said.
#[test]
fn a_region_that_draws_no_cell_reports_nothing_a_cell_met() {
    let cell = stream_object(
        "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 \
         /BBox [0 0 10 10] /XStep 10 /YStep 10 /Resources << >>",
        b"BT /Nope 6 Tf 1 1 Td (x) Tj ET 1 0 0 rg 0 0 5 5 re f",
    );
    let bytes = page_with(
        "/Pattern cs /P scn 0 60 40 40 re f",
        100,
        100,
        "<< /Pattern << /P 5 0 R >> >>",
        &[&cell],
    );
    let region = RenderOptions {
        region: Some(tinker_pdf::PixelRegion::new(50, 0, 50, 50)),
        ..RenderOptions::default()
    };
    let document = Document::open(bytes.clone()).expect("it opens");
    let page = document.page(0).expect("a page");
    assert!(
        page.render(&RenderOptions::default())
            .warnings
            .contains(&RenderWarning::UnreadableFont)
            && page.render(&region).warnings.is_empty(),
        "the fixture is what it says"
    );
    reports_as_it_renders(
        bytes,
        &[
            RenderOptions::default(),
            region.clone(),
            RenderOptions::default(),
            region,
        ],
        "the patterned page",
    );
}

/// **An image that decoded with damage**, and one this build cannot decode at
/// all, are decoded once and kept — and every render that draws them says so,
/// in the words a direct render's own decode uses.
#[test]
fn every_replay_names_the_images_the_page_draws() {
    // `ccitt.rs`'s damaged row: a fax whose second row will not decode.
    let fax = stream_object(
        "/Type /XObject /Subtype /Image /Width 8 /Height 4 \
         /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
         /DecodeParms << /K -1 /Columns 8 /Rows 4 >>",
        &bits(concat!("001 00110101 011 1 ", "111 ", "00000001")),
    );
    // `images.rs`'s placeholder: a JPX codestream of four zero bytes.
    let odd = stream_object(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode",
        &[0, 0, 0, 0],
    );
    let bytes = page_with(
        "q 20 0 0 20 0 0 cm /Fax Do Q q 10 0 0 10 25 5 cm /Odd Do Q \
         q 5 0 0 5 25 20 cm /Odd Do Q",
        40,
        30,
        "<< /XObject << /Fax 5 0 R /Odd 6 0 R >> >>",
        &[&fax, &odd],
    );
    let document = Document::open(bytes.clone()).expect("it opens");
    let direct = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        direct
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::DamagedImage { .. }))
            && direct
                .warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
        "the fixture is what it says: {:?}",
        direct.warnings
    );
    // `/Odd` is drawn twice and fails once: the second draw meets the failure
    // in the cache and says what the first said — its codec, not its resource
    // name standing in for one, which is what a cached failure used to come
    // back as.
    assert_eq!(
        direct
            .warnings
            .iter()
            .filter(|w| matches!(w, RenderWarning::UnsupportedImage { .. }))
            .collect::<Vec<_>>(),
        [&RenderWarning::UnsupportedImage {
            codec: "Unsupported(Jpx)".to_string()
        }],
        "one codec named, once"
    );
    let twice = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    reports_as_it_renders(
        bytes,
        &[RenderOptions::default(), twice, RenderOptions::default()],
        "the damaged-image page",
    );
}

/// **A glyph its font resolves to `.notdef`**, extracted once and kept in the
/// outline cache — and reported by every render that draws it, as a direct
/// render's own extraction reports it.
///
/// A composite font over `render_support`'s TrueType face whose
/// `/CIDToGIDMap` sends CID 1 to glyph 0, which 9.7.4.2 makes a CID the font
/// does not carry, and CID 2 to a glyph it does.
#[test]
fn every_replay_names_the_glyph_its_font_could_not_resolve() {
    let face = render_support::curvy_font();
    let type0: &[u8] = b"<< /Type /Font /Subtype /Type0 /BaseFont /Curvy /Encoding /Identity-H \
                         /DescendantFonts [6 0 R] >>";
    let cid_font: &[u8] = b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Curvy \
                            /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                            /FontDescriptor 7 0 R /CIDToGIDMap 8 0 R /DW 600 >>";
    let descriptor: &[u8] = b"<< /Type /FontDescriptor /FontName /Curvy /Flags 4 \
                              /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
                              /CapHeight 700 /StemV 80 /FontFile2 9 0 R >>";
    let map = stream_object("", &[0, 0, 0, 0, 0, 3]);
    let program = stream_object("", &face);
    let bytes = page_with(
        "BT /F0 20 Tf 5 10 Td <00010002> Tj ET",
        60,
        40,
        "<< /Font << /F0 5 0 R >> >>",
        &[type0, cid_font, descriptor, &map, &program],
    );
    let document = Document::open(bytes.clone()).expect("it opens");
    let direct = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        direct.warnings.contains(&RenderWarning::UnreadableFont),
        "the fixture is what it says: {:?}",
        direct.warnings
    );
    assert!(
        direct.data.chunks_exact(3).any(|p| p != [255, 255, 255]),
        "and CID 2 draws"
    );
    let twice = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    reports_as_it_renders(
        bytes,
        &[RenderOptions::default(), twice, RenderOptions::default()],
        "the unresolved-glyph page",
    );
}

// ---- a page too large to retain ---------------------------------------------

/// A page whose forms fan out: four forms deep, each invoking the one below
/// it four times at four offsets, and the page invoking the top one the same
/// way — 4^4 = 256 leaves from a file of under fifty kilobytes. Each leaf is
/// a blue one-point square, so the leaves tile a 16 x 16 grid two points
/// apart, and sixteen red fills of four hundred segments each, off the page,
/// which is what makes a recorded leaf large and leaves a render of it cheap.
fn fan_out_page() -> Vec<u8> {
    let heavy = format!(
        "1 0 0 rg 91 1 m {}f\n",
        "92 1 l 92 2 l 91 2 l 91 1 l ".repeat(100)
    );
    let leaf = format!("0 0 1 rg 1 1 1 1 re f\n{}", heavy.repeat(16));
    let fan = |step: u32, name: &str| {
        [(0, 0), (step, 0), (0, step), (step, step)]
            .iter()
            .map(|(x, y)| format!("q 1 0 0 1 {x} {y} cm /{name} Do Q "))
            .collect::<String>()
    };
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // Object 4 + k is form F(k); F1 is the leaf, and F(k) steps by 2^(k-1)
    // points so each level doubles the grid.
    for k in 1..=4u32 {
        let (content, resources) = if k == 1 {
            (leaf.clone(), "<< >>".to_string())
        } else {
            (
                fan(1 << (k - 1), &format!("F{}", k - 1)),
                format!("<< /XObject << /F{} {} 0 R >> >>", k - 1, k + 3),
            )
        };
        objects.push(stream_object(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Resources {resources}"),
            content.as_bytes(),
        ));
    }
    let refs: Vec<&[u8]> = objects.iter().map(Vec::as_slice).collect();
    page_with(
        &fan(16, "F4"),
        40,
        40,
        "<< /XObject << /F4 8 0 R >> >>",
        &refs,
    )
}

/// **The recording has a budget, and a page past it is not retained** —
/// `tinker_pdf_render::MAX_DISPLAY_LIST_BYTES`, fired at its own value and
/// not a lowered one. The fan-out page records 4 096 fills of four hundred
/// segments, more than 90 MB by the recorder's count, so the list keeps none
/// of it; and what it draws is still the page, because every render of a
/// list that is not retained is a direct one: the same pixels — all 256
/// squares — the same warnings, and every leaf in the SVG.
#[test]
fn a_page_too_large_to_retain_is_drawn_the_direct_way() {
    let document = Document::open(fan_out_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    assert!(!list.is_retained(), "past the budget: {list:?}");
    assert_eq!(list.len(), 0, "and nothing of it is kept");
    assert!(list.is_empty());

    let options = RenderOptions::default();
    let direct = page.render(&options);
    assert_eq!(
        direct
            .data
            .chunks_exact(3)
            .filter(|p| *p == [0, 0, 255])
            .count(),
        256,
        "every leaf's square, one pixel each"
    );
    let replayed = list.render(&options);
    assert_eq!(
        (replayed.width, replayed.height, &replayed.warnings),
        (direct.width, direct.height, &direct.warnings)
    );
    assert!(replayed.data == direct.data, "the same pixels");

    let svg = list.to_svg(&tinker_pdf::SvgOptions::default());
    assert!(svg.warnings.is_empty(), "{:?}", svg.warnings);
    assert_eq!(
        (
            svg.markup.matches("fill=\"#0000ff\"").count(),
            svg.markup.matches("fill=\"#ff0000\"").count()
        ),
        (256, 4_096),
        "every leaf, interpreted into the writer"
    );
}

/// **The review's own page**: ten forms, each invoking the next four times,
/// the last filling a small square — about two kilobytes that recorded
/// 1 310 719 calls and 635 MB before the budget. Recorded now, it stops at
/// the budget and keeps nothing. Not rendered here: a direct render of a
/// million fills is the interpreter's own cost — it bounds how deep forms
/// nest, not how wide they fan — and a time exposure this list no longer adds
/// memory to.
#[test]
fn the_review_s_fan_out_of_forms_is_not_retained() {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    for k in 0..10u32 {
        let (content, resources) = if k == 0 {
            ("0 0 1 rg 1 1 2 2 re f".to_string(), "<< >>".to_string())
        } else {
            (
                "/N Do ".repeat(4),
                format!("<< /XObject << /N {} 0 R >> >>", k + 4),
            )
        };
        objects.push(stream_object(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources {resources}"),
            content.as_bytes(),
        ));
    }
    let refs: Vec<&[u8]> = objects.iter().map(Vec::as_slice).collect();
    let bytes = page_with("/N Do", 10, 10, "<< /XObject << /N 14 0 R >> >>", &refs);
    assert!(bytes.len() < 3_000, "{} bytes", bytes.len());
    let document = Document::open(bytes).expect("it opens");
    let list = document.page(0).expect("a page").display_list();
    assert!(!list.is_retained() && list.is_empty(), "{list:?}");
}
