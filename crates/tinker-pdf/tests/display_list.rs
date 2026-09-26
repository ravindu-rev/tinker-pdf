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
