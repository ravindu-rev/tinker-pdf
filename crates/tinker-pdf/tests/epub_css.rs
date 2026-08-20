//! What the committed corpus's stylesheets actually spend against milestone
//! 6's caps — recomputed on every run, so the ledger cannot drift.
//!
//! `tinker-pdf-css/src/limits.rs` publishes three numbers per constant, and the
//! first of them is *"the most any fixture in this repository spends"*. That
//! number is a **measurement** and this is where it is measured: every
//! stylesheet in every committed book is parsed through the real parser, at the
//! shipped limits, and the maxima are asserted against what the ledger says.
//!
//! It is here rather than in the leaf for the reason `xml_real_packages.rs` is:
//! reading a book needs `tinker-pdf-zip`, and a leaf that reaches sideways to
//! test itself is a leaf with an edge it did not need.
//!
//! **Nothing in this file lays anything out.** The cascade needs an element
//! tree and that is milestone 8's; what this can say is what the *parser* costs
//! on real input, which is five of the eight rows.

mod epub_support;

use std::path::PathBuf;

use epub_support::{entries, is_stylesheet};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::property::Declaration;
use tinker_pdf_css::{parse, Budget, Limits, NoImports};
use tinker_pdf_zip::{Archive, Limits as ZipLimits};

/// The six books milestone 1 commissioned. Named here rather than globbed,
/// because a directory listing that comes back empty is a test that passes.
const BOOKS: &[&str] = &[
    "pandoc-book-cover.epub",
    "pandoc-book-nocover.epub",
    "pandoc-book-epub2.epub",
    "pandoc-plates.epub",
    "calibre-book-cover.epub",
    "calibre-book-nocover.epub",
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
}

fn book(name: &str) -> Vec<u8> {
    let path = corpus_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every stylesheet in a book: its entry name and its bytes.
fn stylesheets(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = Archive::open(bytes, &ZipLimits::DEFAULT).expect("a book is a ZIP");
    let chosen: Vec<(usize, String)> = entries(bytes)
        .iter()
        .enumerate()
        .filter(|(_, e)| is_stylesheet(&e.name))
        .map(|(i, e)| (i, e.name.clone()))
        .collect();
    chosen
        .into_iter()
        .map(|(index, name)| {
            let data = archive
                .read(index)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"))
                .into_owned();
            (name, data)
        })
        .collect()
}

/// The measurement, and the assertion that the ledger's first column is it.
///
/// Every maximum is asserted **exactly**, not as an upper bound. A `<=` here
/// would pass on the day a producer's stylesheet halves, and the point of the
/// column is to say how much room the cap actually has — which is a different
/// claim from "it fits".
#[test]
fn the_committed_corpus_spends_what_the_ledger_says() {
    let limits = Limits::DEFAULT;
    let mut sheets = 0usize;
    let mut largest_sheet = 0usize;
    let mut most_tokens = 0usize;
    let mut most_rules = 0usize;
    let mut most_declarations = 0usize;
    let mut longest_selector = 0usize;
    let mut discarded_declarations = 0usize;
    let mut discarded_rules = 0usize;

    for name in BOOKS {
        let bytes = book(name);
        // One budget per book, because a book is what the totals are spent
        // across — the same object the facade will hand its forty sheets at
        // milestone 8.
        let mut budget = Budget::new(&limits);
        for (entry, data) in stylesheets(&bytes) {
            sheets += 1;
            largest_sheet = largest_sheet.max(data.len());
            let before = (budget.tokens(), budget.rules(), budget.declarations());
            let sheet = parse(
                &data,
                Some(&entry),
                &NoImports,
                &MediaContext::screen(432.0, 648.0),
                &limits,
                &mut budget,
            )
            .unwrap_or_else(|e| panic!("{name}!{entry}: a real stylesheet was refused: {e}"));
            most_tokens = most_tokens.max(budget.tokens() - before.0);
            most_rules = most_rules.max(budget.rules() - before.1);
            most_declarations = most_declarations.max(budget.declarations() - before.2);
            for rule in &sheet.rules {
                for selector in &rule.selectors {
                    longest_selector = longest_selector.max(selector.compounds.len());
                }
            }
            discarded_declarations += sheet.report.discarded_declarations;
            discarded_rules += sheet.report.discarded_rules;
            // `MAX_CSS_IMPORT_DEPTH`'s ledger says no book here uses `@import`
            // at all, and this is what says it rather than assuming it: the
            // parse is given `NoImports`, so one would warn by name.
            assert!(
                !sheet
                    .report
                    .warnings
                    .iter()
                    .any(|(w, _)| *w == tinker_pdf_css::Warning::ImportUnresolved),
                "{name}!{entry} uses @import, which the import-depth ledger says none does"
            );
        }
    }

    println!("  {sheets} stylesheets across {} books", BOOKS.len());
    println!("  largest sheet        {largest_sheet} bytes");
    println!("  most tokens          {most_tokens}");
    println!("  most rules           {most_rules}");
    println!("  most declarations    {most_declarations}");
    println!("  longest selector     {longest_selector} compounds");

    assert_eq!(sheets, 8, "milestone 1 committed eight stylesheets");
    assert_eq!(largest_sheet, 5_009);
    assert_eq!(most_tokens, 1_392);
    assert_eq!(most_rules, 45);
    assert_eq!(most_declarations, 99);
    assert_eq!(longest_selector, 5);

    // **Not one construct is discarded.** Two real producers wrote these, and a
    // recovery count above zero on a producer's own output would be evidence
    // about this parser rather than about the producer.
    assert_eq!(discarded_declarations, 0);
    assert_eq!(discarded_rules, 0);
}

/// What a real book asks for that this build does not implement, per property.
///
/// This is the `Unsupported` census gap 31 says the `As built` is judged on,
/// as far as milestone 6 can compute it: parse-time, over the whole committed
/// corpus, with no element tree to say how many elements each reached. The
/// number that matters is the *set* — which properties two real producers write
/// that this engine does not read.
#[test]
fn the_unsupported_census_over_the_committed_corpus() {
    let limits = Limits::DEFAULT;
    let mut unsupported: Vec<(&'static str, usize)> = Vec::new();
    let mut unknown: Vec<(String, usize)> = Vec::new();
    let mut implemented = 0usize;

    for name in BOOKS {
        let bytes = book(name);
        let mut budget = Budget::new(&limits);
        for (entry, data) in stylesheets(&bytes) {
            let sheet = parse(
                &data,
                Some(&entry),
                &NoImports,
                &MediaContext::screen(432.0, 648.0),
                &limits,
                &mut budget,
            )
            .expect("a real stylesheet");
            for (property, count) in sheet.report.unsupported {
                match unsupported.iter_mut().find(|(p, _)| *p == property) {
                    Some(slot) => slot.1 += count,
                    None => unsupported.push((property, count)),
                }
            }
            for (property, count) in sheet.report.unknown {
                match unknown.iter_mut().find(|(p, _)| *p == property) {
                    Some(slot) => slot.1 += count,
                    None => unknown.push((property, count)),
                }
            }
            for rule in &sheet.rules {
                for declared in &rule.declarations {
                    if matches!(declared.declaration, Declaration::Known(_)) {
                        implemented += 1;
                    }
                }
            }
        }
    }

    unsupported.sort_unstable();
    unknown.sort();
    println!("  {implemented} longhands implemented");
    println!("  unsupported: {unsupported:?}");
    println!("  unknown:     {unknown:?}");

    // The set, asserted rather than counted, because a set is what a reader can
    // act on and a count is a mood. Every name here is one two real producers
    // write and this build does not read.
    let names: Vec<&str> = unsupported.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        names,
        [
            // A **value** gap, not a property one, and the best thing this
            // census found: pandoc 3.10.2 writes `css-color-5`'s
            // `light-dark(transparent, #232629)` on `background-color`, `color`
            // and both `border-*` shorthands. Every one of those properties is
            // implemented; the function is not, and reporting it as this
            // build's gap rather than resolving it to its first argument is
            // decision 5's second device doing exactly its job on real input.
            "background-color",
            "border-bottom",
            "border-collapse",
            "border-spacing",
            "border-top",
            "color",
            "color-scheme",
            // Also a value gap, from both producers: pandoc writes
            // `display: flex` and calibre writes six `display: table*` values.
            "display",
            "hyphens",
            "list-style",
            "max-width",
            "overflow",
            "overflow-x",
            "quotes",
            // And the third value gap, which is the one the injection matrix
            // found first and this corroborates: calibre writes
            // `text-align: inherit`, a `css-cascade-5` §7.1 keyword on a
            // property whose own keywords are `left`, `right`, `center` and
            // `justify`.
            "text-align",
            "vertical-align",
        ]
    );

    // And the other half of decision 5's split. **Nothing at all**, and that is
    // the interesting answer rather than a boring one: milestone 1's census
    // found `-webkit-column-count`, `-epub-text-emphasis-style` and Antenna
    // House's `-ah-margin-start` in the *fetched* corpus, and neither producer
    // of the committed six writes a single vendor extension or custom property.
    // The two names that used to land here — `list-style` and `color-scheme` —
    // were this table's own gaps and were moved into it by this test.
    assert!(
        unknown.is_empty(),
        "a committed book writes a property no specification this build cites defines: \
         {unknown:?}"
    );

    // The census means nothing if almost nothing was implemented, so the
    // denominator is asserted too.
    assert!(
        implemented > 200,
        "only {implemented} longhands were read out of the whole corpus"
    );
}
