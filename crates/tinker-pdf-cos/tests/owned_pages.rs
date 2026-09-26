//! Pages built without a closure, and the timing that makes them the same
//! pages (gap 32 milestone 1, `docs/design/bindings-write.md`).
//!
//! `DocumentBuilder::add_page(w, h, |page| ..)` takes a closure and a closure
//! does not cross a foreign-function boundary, so ruling 11 makes the facade
//! grow the closure-free equivalent first: `begin_page` hands a `PageBuilder`
//! back and `push_page` takes it.
//!
//! The whole risk in that split is **when the page's resource set is
//! snapshotted**. `add_page` copies the builder's resources into the page at
//! the moment it constructs one, so a font registered afterwards is invisible
//! to that page. If `begin_page` copied at *push* time instead, the two forms
//! would silently disagree for exactly one program: the one that registers a
//! resource between beginning a page and pushing it. That program is rare,
//! which is what makes the divergence dangerous rather than obvious — so the
//! timing is asserted here rather than assumed, in both directions:
//!
//! - the two forms produce byte-identical documents for that program, and
//! - the page they produce genuinely lacks the late resource, so the equality
//!   above is not two identically-wrong answers.
//!
//! Counted injection, run August 2026, and the count is what was observed
//! rather than what was expected. Defect reintroduced: `begin_page` changed to
//! take an empty `ResourceSet` and `push_page` to fill it in
//! (`page.resources = self.resources.clone()`), which is the snapshot moving
//! from begin to push. Result: **2 of these 6 tests fail, on 2 assertions** —
//! one per test, because a Rust assertion aborts its test and the later ones
//! in the same test are then unreached.
//!
//! - `beginning_a_page_snapshots_resources_when_add_page_does`, on the
//!   byte-equality assertion: FNV 10924893674392590639 against
//!   6479374663068860923;
//! - `the_late_resource_is_absent_from_both_forms`, on its **second** line —
//!   `["F1", "F2"]` against `["F1"]`. That it is the second and not the first
//!   is the finding: the closure form is untouched by the injection and only
//!   the owned form drifts, which is precisely the silent divergence this file
//!   exists to catch.
//!
//! The plain sugar-vs-primitive test does **not** catch it, and neither do the
//! three below it. That is the point rather than a gap: they register nothing
//! between begin and push, so both snapshots are of the same resource set and
//! both forms agree while the contract is broken. A milestone that shipped
//! only the byte-equality test would have shipped this defect.

use tinker_pdf_cos::{pages, CosDocument, DocumentBuilder, Object};

/// FNV-1a, 64 bit. Hand-rolled so the byte-identity assertions depend on
/// nothing, and stable across platforms because it is integer arithmetic —
/// the same helper `form_transactions.rs` carries, for the same reason.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// The names in page zero's `/Resources` `/Font`, sorted.
///
/// The dictionary is read back out of the finished file rather than asked of
/// the builder, because what a page may name is a property of the document
/// somebody opens, not of the object that wrote it.
fn page_font_names(bytes: &[u8]) -> Vec<String> {
    let doc = CosDocument::open(bytes.to_vec()).expect("the built document opens");
    let page = pages::collect(&doc)
        .into_iter()
        .next()
        .expect("it has a page");
    let Some(resources) = page.resources else {
        return Vec::new();
    };
    let Some(fonts) = resources.get_dict(doc.intern(b"Font")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = fonts
        .iter()
        .map(|(name, _)| {
            let bytes = doc.name_bytes(*name).expect("a registered name");
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Everything the two forms draw, so the only difference between them is
/// which form drew it.
fn draw(page: &mut tinker_pdf_cos::PageBuilder) {
    page.text(b"F1", 12.0, 20.0, 160.0, "drawn with the early font");
    // Deliberately names a font registered *after* the page began. The
    // operator reaches the content stream either way — a page builder writes
    // what it is told — and whether `/F2` reaches `/Resources` is the timing
    // question this file exists to pin.
    page.text(b"F2", 12.0, 20.0, 130.0, "drawn with the late font");
    page.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25);
}

/// The closure form: the resource snapshot happens inside `add_page`, so `F2`
/// — registered on the next line — is not in it.
fn built_with_the_closure() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(200.0, 200.0, draw);
    builder.add_base_font(b"F2", b"Courier");
    builder.finish()
}

/// The owned form, with the late registration in the one place that can tell
/// the two snapshots apart: between `begin_page` and `push_page`.
fn built_with_the_pair() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    let mut page = builder.begin_page(200.0, 200.0);
    builder.add_base_font(b"F2", b"Courier");
    draw(&mut page);
    builder.push_page(page);
    builder.finish()
}

/// The equivalence the design claims, asserted rather than assumed.
#[test]
fn beginning_a_page_snapshots_resources_when_add_page_does() {
    let closure = built_with_the_closure();
    let pair = built_with_the_pair();

    assert_eq!(
        fnv(&closure),
        fnv(&pair),
        "a resource registered between begin and push must be as invisible to \
         the page as one registered after add_page returns"
    );
    assert_eq!(closure, pair);

    assert_eq!(
        page_font_names(&pair),
        ["F1"],
        "the owned page carries the font that existed when it began, and not \
         the one registered afterwards"
    );
}

/// The other half of the same claim: both forms genuinely *lack* the late
/// resource. Without this, a `begin_page` that snapshotted at push time and an
/// `add_page` that had been changed to match would agree with each other and
/// both be wrong.
#[test]
fn the_late_resource_is_absent_from_both_forms() {
    assert_eq!(page_font_names(&built_with_the_closure()), ["F1"]);
    assert_eq!(page_font_names(&built_with_the_pair()), ["F1"]);
}

/// The plain case, with nothing registered in between: the same document, by
/// hash and by bytes.
///
/// This is the milestone's "sugar-vs-primitive byte-hash equality on a built
/// document", and it is deliberately the *weaker* of the two equalities here —
/// see this file's header for what it does not catch.
#[test]
fn the_closure_page_and_the_owned_page_write_the_same_document() {
    let sugar = {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F1", b"Helvetica");
        builder.add_page(200.0, 200.0, |page| {
            page.text(b"F1", 14.0, 20.0, 170.0, "Page one");
            page.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25);
        });
        builder.add_page(200.0, 200.0, |page| {
            page.text(b"F1", 14.0, 20.0, 170.0, "Page two");
        });
        builder.set_info(b"Title", "two pages");
        builder.finish()
    };

    let primitive = {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F1", b"Helvetica");
        let mut one = builder.begin_page(200.0, 200.0);
        one.text(b"F1", 14.0, 20.0, 170.0, "Page one");
        one.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25);
        builder.push_page(one);
        let mut two = builder.begin_page(200.0, 200.0);
        two.text(b"F1", 14.0, 20.0, 170.0, "Page two");
        builder.push_page(two);
        builder.set_info(b"Title", "two pages");
        builder.finish()
    };

    assert_eq!(fnv(&sugar), fnv(&primitive));
    assert_eq!(sugar, primitive);
    assert_eq!(pages::count(&CosDocument::open(sugar).expect("opens")), 2);
}

/// A page begun and never pushed is simply dropped, and the document is what
/// it would have been.
///
/// This is the property that lets the pair cross an ABI where a caller may
/// abandon a handle: there is no half-added page, and no counter to unwind.
#[test]
fn a_page_that_is_never_pushed_leaves_no_trace() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    let mut kept = builder.begin_page(200.0, 200.0);
    kept.text(b"F1", 14.0, 20.0, 170.0, "kept");
    builder.push_page(kept);

    let mut abandoned = builder.begin_page(400.0, 400.0);
    abandoned.text(b"F1", 14.0, 20.0, 170.0, "abandoned, and never pushed");
    drop(abandoned);

    let bytes = builder.finish();
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    assert_eq!(pages::count(&doc), 1, "only the pushed page is in the file");

    let text = String::from_utf8_lossy(&bytes);
    assert!(
        !text.contains("abandoned, and never pushed"),
        "and nothing it drew reached the file"
    );

    // The same document the abandoned page was never begun in.
    let mut clean = DocumentBuilder::new();
    clean.add_base_font(b"F1", b"Helvetica");
    let mut only = clean.begin_page(200.0, 200.0);
    only.text(b"F1", 14.0, 20.0, 170.0, "kept");
    clean.push_page(only);
    assert_eq!(bytes, clean.finish(), "byte for byte");
}

/// Pages arrive in the order they are pushed, which is the order `add_page`
/// gives them — the property a caller holding two page handles at once needs.
#[test]
fn pages_arrive_in_the_order_they_are_pushed() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");

    // Both pages exist before either is pushed, which is the shape the closure
    // form cannot express at all.
    let mut first = builder.begin_page(200.0, 200.0);
    let mut second = builder.begin_page(300.0, 300.0);
    first.text(b"F1", 14.0, 20.0, 170.0, "first");
    second.text(b"F1", 14.0, 20.0, 170.0, "second");

    builder.push_page(second);
    builder.push_page(first);

    let doc = CosDocument::open(builder.finish()).expect("it opens");
    let collected = pages::collect(&doc);
    assert_eq!(collected.len(), 2);
    assert_eq!(
        (
            collected[0].media_box.width(),
            collected[1].media_box.width()
        ),
        (300.0, 200.0),
        "pushed second-then-first, so the 300-wide page is page zero"
    );
}

/// `PageBuilder::tagged` stays a closure, and this test is where that decision
/// is written down rather than left as an omission.
///
/// It nests *within* one page: the closure's scope is the structure element's
/// extent, and the `BDC`/`EMC` pair it writes is opened and closed by the same
/// call. Nothing about it crosses a language boundary — a binding that
/// projects `begin_page`/`push_page` can project a tagged span as
/// `open_tag`/`close_tag` on the page handle whenever a binding needs one,
/// which is a separate question with a separate answer, and gap 32 does not
/// need it: neither parity script tags anything.
///
/// What is asserted here is only that the two forms compose — a page begun
/// with `begin_page` can still be tagged — so the decision costs nothing that
/// has to be recovered later.
#[test]
fn an_owned_page_can_still_be_tagged_with_the_closure_form() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    let mut page = builder.begin_page(200.0, 200.0);
    page.tagged(b"P", |inner| {
        inner.text(b"F1", 12.0, 20.0, 160.0, "a tagged paragraph");
    });
    builder.push_page(page);

    let bytes = builder.finish();
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    assert_eq!(pages::count(&doc), 1);

    let catalog = doc.catalog().expect("a catalog");
    assert!(
        matches!(
            catalog.get(doc.intern(b"StructTreeRoot")),
            Some(Object::Ref(_))
        ),
        "the tag reached the document's structure tree, from a page that was \
         begun rather than added"
    );
    assert!(
        String::from_utf8_lossy(&bytes).contains("/P <</MCID 0>> BDC"),
        "and the marked-content pair is in the content stream"
    );
}
