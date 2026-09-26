//! 14.2.4's **remote** resource dictionary: `Source` naming a separate part.
//!
//! # What the pair is here
//!
//! A remote dictionary has two independent halves and a test for one is not a
//! test for the other:
//!
//! - the part is **found and parsed** — its keys answer `{StaticResource}` on
//!   the page that named it;
//! - the part is **not** found, or is not a dictionary, and the page still
//!   draws — one named defect, and every key that wanted it takes the
//!   placeholder grey rather than the page being lost.
//!
//! A build that resolved the part but never consulted it, and a build that
//! consulted a table it never filled, each pass one of those and fail the
//! other.
//!
//! # The chain's two guards
//!
//! A dictionary part's own root may state a `Source`, so the parts form a
//! chain. Long-without-repeating and repeating-without-being-long are two
//! different failures under two names — `BrushTooDeep` and `BrushCyclic` — and
//! they are the same pair `paint.rs`'s `{StaticResource}` lookup and a
//! `VisualBrush` nest each carry. Three places, one distinction.
//!
//! # Counted injections
//!
//! Each check was verified by reintroducing the defect it exists to catch and
//! running `cargo test -p tinker-pdf --no-fail-fast`, whose baseline is
//! **1 239 tests**.
//!
//! | Injection | Caught by |
//! | --- | --- |
//! | `Remotes::get` answers an empty table for every part it holds | 4 |
//! | the chain's cycle guard is removed, the depth cap kept | 1 |
//! | the chain's depth cap is removed, the cycle guard kept | 1 |
//! | a chain link resolves against the chain's *first* part, not its own | 1 |
//! | inline children are merged in behind a `Source` | 1 |
//!
//! Nothing fired zero, and one of these had to be *made* non-zero. The
//! resolution-base injection was caught by **nothing** at first: the chain
//! test then in the file put both parts in one folder, where resolving against
//! the naming part, against the chain's first part and against the page all
//! give the same answer. `each_link_of_a_chain_resolves_against_its_own_part`
//! exists because of that run — three links across two folders is the shortest
//! chain that can tell the three bases apart, and 18.2's rule is only actually
//! asserted by a fixture that can.
//!
//! The two guards fire one test each and a *different* one. Removing the cycle
//! guard does not hang — the depth cap stops it — it reports the wrong name,
//! which is exactly why a suite with only one of the two would pass with the
//! other deleted.

mod xps_support;

use tinker_pdf::{ArchiveWarning, Document, XpsElementDefect};
use xps_support::{
    archive, before_content_types, content_types_with, one_page_package, part, with, Part, XPS_NS,
};

/// The resource-dictionary key namespace, which every real package binds.
const KEY_NS: &str = "http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key";

/// A content-types item that also resolves `.dict` parts.
///
/// Named rather than left to a `Default` this fixture never declared: a part
/// with no content type is not one 7.2.3.5 resolved, and a test that got its
/// media type by accident would be testing the accident.
fn types() -> String {
    content_types_with(
        r#"<Default Extension="dict" ContentType="application/vnd.ms-package.xps-resourcedictionary+xml" />"#,
    )
}

/// A `ResourceDictionary` part holding `body`.
fn dictionary(name: &str, body: &str) -> Part {
    part(
        name,
        &format!(
            r#"<?xml version="1.0" encoding="utf-8"?><ResourceDictionary xmlns="{XPS_NS}" xmlns:x="{KEY_NS}">{body}</ResourceDictionary>"#
        ),
    )
}

/// A `ResourceDictionary` part whose **root** names another part.
///
/// The `Source` goes on the root and not on a child, which is what makes the
/// parts a chain: a `ResourceDictionary` written *inside* another one is an
/// entry, and an entry is not a redirection.
fn redirect(name: &str, source: &str) -> Part {
    part(
        name,
        &format!(
            r#"<?xml version="1.0" encoding="utf-8"?><ResourceDictionary xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Source="{source}" />"#
        ),
    )
}

/// A package whose one page carries `body` and which also holds `extra`.
fn package(body: &str, extra: Vec<Part>) -> Vec<u8> {
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    let mut parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
    parts = with(parts, "[Content_Types].xml", &types());
    for one in extra {
        parts = before_content_types(parts, one);
    }
    archive(parts)
}

fn defects(bytes: &[u8]) -> Vec<XpsElementDefect> {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect()
}

fn stream(bytes: &[u8]) -> String {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// A page naming `source` for its resources and filling a square with `key`.
fn body(source: &str, key: &str) -> String {
    format!(
        r#"<FixedPage.Resources><ResourceDictionary Source="{source}" /></FixedPage.Resources>
           <Path Data="M0,0L200,0 200,200 0,200Z" Fill="{{StaticResource {key}}}" />"#
    )
}

/// A green brush, which is what most of these dictionaries hold.
const GREEN: &str = r##"<SolidColorBrush x:Key="b" Color="#FF00FF00" />"##;

/// The dictionary is **read from the other part**, and its key answers.
///
/// `0 1 0 rg` is the whole assertion: the colour is nowhere in the page's own
/// markup, so a build that did not read the part could not write it.
#[test]
fn a_remote_dictionary_answers_a_static_resource_on_the_page() {
    let bytes = package(
        &body("/Resources/d.dict", "b"),
        vec![dictionary("Resources/d.dict", GREEN)],
    );
    assert_eq!(defects(&bytes), []);
    let page = stream(&bytes);
    assert!(
        page.contains("0 1 0 rg"),
        "the remote colour is used: {page}"
    );
    assert!(
        !page.contains("0.749 0.749 0.749 rg"),
        "and not the placeholder: {page}"
    );
}

/// A relative `Source` resolves against the **page part's own name**, which is
/// 18.2's rule and the form OpenXPS writes.
///
/// XPS 1.0 writes `/Resources/…` and OpenXPS writes `../../../Resources/…` for
/// the same part — milestone 1 measured both — so a build that only handled the
/// absolute form would pass the test above and fail every real OpenXPS package.
#[test]
fn a_relative_source_resolves_against_the_page_part() {
    let bytes = package(
        &body("../../../Resources/d.dict", "b"),
        vec![dictionary("Resources/d.dict", GREEN)],
    );
    assert_eq!(defects(&bytes), []);
    assert!(stream(&bytes).contains("0 1 0 rg"));
}

/// A `Source` naming no part is **one** named defect, and the page still draws.
///
/// One `ResourceDictionaryUnresolved` rather than one `BrushUnresolved` per
/// use: a dictionary that is not there is one fact about the file, and
/// reporting it once per key would report the same fact as many times as the
/// page happened to use it.
#[test]
fn a_source_naming_no_part_is_named_once_and_the_page_survives() {
    let bytes = package(&body("/Resources/missing.dict", "b"), Vec::new());
    assert_eq!(
        defects(&bytes),
        [
            XpsElementDefect::ResourceDictionaryUnresolved,
            XpsElementDefect::BrushUnresolved,
        ]
    );
    assert!(
        stream(&bytes).contains("0.749 0.749 0.749 rg"),
        "the shape keeps the placeholder grey"
    );
}

/// A part that **is** there and is not a dictionary is a different fact, under
/// a different name.
///
/// "No such part" and "that part is not a dictionary" say different things
/// about the package, and only the second says it is internally inconsistent.
#[test]
fn a_source_naming_a_part_that_is_not_a_dictionary_is_unreadable_not_unresolved() {
    let bytes = package(
        &body("/Resources/d.dict", "b"),
        vec![part(
            "Resources/d.dict",
            &format!(r#"<Canvas xmlns="{XPS_NS}" />"#),
        )],
    );
    assert_eq!(
        defects(&bytes),
        [
            XpsElementDefect::ResourceDictionaryUnreadable,
            XpsElementDefect::BrushUnresolved,
        ]
    );
}

/// A dictionary part may name a third part, and the chain is followed.
///
/// The chain's *first* link is what the tests above pin. This is the second,
/// because the depth cap and the cycle guard below are meaningless if two links
/// never worked.
#[test]
fn a_dictionary_part_may_name_another_one() {
    let bytes = package(
        &body("/Resources/a.dict", "b"),
        vec![
            redirect("Resources/a.dict", "b.dict"),
            dictionary("Resources/b.dict", GREEN),
        ],
    );
    // The inner `Source` is written on `a.dict`, so it resolves against
    // `Resources/` — not against the page's folder, which is `Documents/1/Pages/`.
    assert_eq!(defects(&bytes), []);
    assert!(stream(&bytes).contains("0 1 0 rg"));
}

/// Every link resolves against **the part it is written on**, not against the
/// first part of the chain and not against the page.
///
/// Three links across two folders, which is the only shape that can tell those
/// three bases apart: `c.dict` written on `Resources/sub/b.dict` is
/// `Resources/sub/c.dict`, and resolving it against either `Resources/a.dict`
/// or the page's own `Documents/1/Pages/` folder names a part that is not
/// there. A two-link chain inside one folder — which is what the test above is
/// — gives all three bases the same answer, so it cannot see this at all.
#[test]
fn each_link_of_a_chain_resolves_against_its_own_part() {
    let bytes = package(
        &body("/Resources/a.dict", "b"),
        vec![
            redirect("Resources/a.dict", "sub/b.dict"),
            redirect("Resources/sub/b.dict", "c.dict"),
            dictionary("Resources/sub/c.dict", GREEN),
        ],
    );
    assert_eq!(defects(&bytes), []);
    assert!(stream(&bytes).contains("0 1 0 rg"));
}

/// A chain that returns to a part it already passed through is a **cycle**.
///
/// Two links, so the depth cap is nowhere near firing — which is what makes the
/// two guards two rules rather than one.
#[test]
fn a_dictionary_chain_that_returns_to_itself_is_a_cycle() {
    let bytes = package(
        &body("/Resources/a.dict", "b"),
        vec![
            redirect("Resources/a.dict", "b.dict"),
            redirect("Resources/b.dict", "a.dict"),
        ],
    );
    assert_eq!(
        defects(&bytes),
        [
            XpsElementDefect::BrushCyclic,
            XpsElementDefect::BrushUnresolved,
        ]
    );
}

/// A chain longer than the cap is refused **as a depth**, and is not a cycle.
///
/// Twenty links, none of which repeats a part, so a build whose only guard was
/// the cycle guard would follow every one of them.
#[test]
fn a_dictionary_chain_past_the_depth_cap_is_named() {
    let mut parts = vec![dictionary("Resources/d19.dict", GREEN)];
    for step in 0..19 {
        parts.push(redirect(
            &format!("Resources/d{step}.dict"),
            &format!("d{}.dict", step + 1),
        ));
    }
    let bytes = package(&body("/Resources/d0.dict", "b"), parts);
    assert_eq!(
        defects(&bytes),
        [
            XpsElementDefect::BrushTooDeep,
            XpsElementDefect::BrushUnresolved,
        ]
    );
}

/// `Source` and inline content are **alternatives**, not a pair.
///
/// A dictionary that states a part is that part's; the children written under
/// it are not merged in behind it. A build that merged them would answer a key
/// from whichever happened to be inserted second, which is a document whose
/// colours depend on a hash order.
#[test]
fn a_source_replaces_inline_content_rather_than_merging_with_it() {
    let inline = r##"<FixedPage.Resources><ResourceDictionary Source="/Resources/d.dict">
              <SolidColorBrush x:Key="i" Color="#FFFF0000" />
            </ResourceDictionary></FixedPage.Resources>
            <Path Data="M0,0L200,0 200,200 0,200Z" Fill="{StaticResource i}" />"##;
    let bytes = package(inline, vec![dictionary("Resources/d.dict", GREEN)]);
    // The inline key is not in scope: the dictionary is the remote part's.
    assert_eq!(defects(&bytes), [XpsElementDefect::BrushUnresolved]);
    assert!(
        stream(&bytes).contains("0.749 0.749 0.749 rg"),
        "and the shape takes the placeholder rather than the inline red"
    );
}

/// One dictionary part read once, however many pages name it.
///
/// The table is keyed by the **part**, so two pages naming one dictionary two
/// ways are one dictionary — which is what makes this a pass rather than a
/// lookup, and what stops a four-thousand-page document re-parsing one part
/// four thousand times.
#[test]
fn two_pages_naming_one_dictionary_both_resolve_it() {
    let page = |source: &str| {
        format!(
            r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{}</FixedPage>"#,
            body(source, "b")
        )
    };
    let mut parts = one_page_package();
    parts = with(
        parts,
        "Documents/1/FixedDocument.fdoc",
        &xps_support::document(&["Pages/1.fpage", "Pages/2.fpage"]),
    );
    parts = with(
        parts,
        "Documents/1/Pages/1.fpage",
        &page("/Resources/d.dict"),
    );
    parts = with(
        parts,
        "Documents/1/Pages/2.fpage",
        &page("../../../Resources/d.dict"),
    );
    parts = with(parts, "[Content_Types].xml", &types());
    parts = before_content_types(parts, dictionary("Resources/d.dict", GREEN));
    let bytes = archive(parts);
    assert_eq!(defects(&bytes), []);

    let document = Document::open(bytes).expect("an XPS");
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    assert_eq!(pages.len(), 2);
    for page in &pages {
        let content =
            String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned();
        assert!(
            content.contains("0 1 0 rg"),
            "both spellings reach one dictionary: {content}"
        );
    }
}
