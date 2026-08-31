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
//!
//! One test here is not about the ledger at all, and it is here for the reason
//! the header above gives: the pseudo-classes `selectors-4` defers to the
//! document language are answered by `epub::xhtml`'s element and by nothing in
//! the CSS crate, so the only place the wiring can be checked is a crate that
//! has both. `tinker-pdf-css`'s own suite proves the matcher; this proves that
//! `xml:lang`, `href` and `checked` reach it.

mod epub_support;

use std::path::PathBuf;

use epub_support::{entries, is_stylesheet};
use tinker_pdf::epub::xhtml;
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
    let mut layers = 0usize;

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
            layers += sheet.layers.len();
        }
    }

    println!("  {sheets} stylesheets across {} books", BOOKS.len());
    println!("  largest sheet        {largest_sheet} bytes");
    println!("  most tokens          {most_tokens}");
    println!("  most rules           {most_rules}");
    println!("  most declarations    {most_declarations}");
    println!("  longest selector     {longest_selector} compounds");
    println!("  layers declared      {layers}");

    // **No committed book uses `@layer`, and that is measured rather than
    // assumed.** The exit criterion for reading it asked for a case over a real
    // producer's stylesheet with layers in it; neither pandoc 3.10.2 nor
    // calibre 9.13.0 writes one, so this is the case there is — the number,
    // asserted exactly, so the day a producer starts writing them this test
    // says so instead of quietly still passing. The corpus caveat below the
    // refusal table in `docs/features/epub.md` is this line's prose.
    assert_eq!(
        layers, 0,
        "a committed book declares a cascade layer, which this ledger says none does"
    );

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
    // write and this build does not read. Ten of them, and two left in tier 4
    // when §7.1's defaulting keywords landed.
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
            "border-top",
            "color",
            "color-scheme",
            // **`display` used to be here and milestone 11 removed it**, and
            // the way it left is the census earning its keep. Its whole count
            // over this corpus was calibre's six `display: table*` values, all
            // of which are now read; the `display: flex` this list's old
            // comment blamed on pandoc is inside a `/* … */` in pandoc's own
            // stylesheet and never reached the parser at all. A census that
            // counted names rather than asserting the set would have gone from
            // fourteen to thirteen and nobody would have known which one went.
            "hyphens",
            "list-style",
            // **`max-width` used to be here and tier 4 removed it**, the same
            // way `display` left one milestone earlier: pandoc's whole count
            // over this corpus was `img { max-width: 100% }`, and CSS 2.2
            // §10.4's clamp is now applied rather than reported.
            "overflow",
            "overflow-x",
            "quotes",
            // **`text-align` and `vertical-align` used to be here and tier 4
            // removed them both, in one edit, without touching either
            // property.** Both were value gaps and both were the *same* value
            // gap: calibre writes `text-align: inherit` and
            // `vertical-align: inherit` on four of its five table classes, and
            // `css-cascade-5` §7.1's five explicit defaulting keywords were
            // implemented on no property at all.
            //
            // They are the strongest thing this census has said. Nothing about
            // either property changed — `vertical-align` had already been
            // implemented at all ten of its values a milestone earlier and this
            // row survived it, which is what said the row was about the value.
            // A census that counted names would have gone from twelve to ten
            // and left a reader to guess which two, and guessing "the two most
            // recently implemented properties" would have been wrong.
        ]
    );

    // `border-collapse` and `border-spacing` left the same way and at the same
    // milestone. Neither is written by either producer here — they were in
    // `UNSUPPORTED_PROPERTIES` and had a count of zero — so their departure
    // changes this list and not this corpus's numbers; the user-agent sheet is
    // where they are now written and `epub_tables.rs` is where that is
    // asserted.

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

/// The pseudo-classes whose meaning is XHTML's, matched through the real
/// element tree.
///
/// The CSS crate's own suite proves the matcher against a fixture element that
/// answers whatever the test tells it to. That leaves exactly one thing
/// unproved and it is the thing ruling 8 is about: whether the *document
/// language* answers arrive at all. `:lang()` here has to find `xml:lang` on a
/// grandparent, `:checked` has to find HTML's attribute on HTML's element and
/// `:empty` has to survive pandoc's indentation — and none of that is
/// something `tinker-pdf-css` can be asked, because none of it is in it.
#[test]
fn the_document_language_answers_the_pseudo_classes_that_are_its_own() {
    // Indented on purpose: every text node between these tags is white space,
    // and `:empty` has to be unmoved by all of it.
    let markup = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-GB" lang="fr">
  <body>
    <ul id="list">
      <li>one</li>
      <li>two</li>
      <li>three</li>
      <li>four</li>
    </ul>
    <p id="linked"><a id="hyper" href="ch02.xhtml">a link</a><a id="anchor">not a link</a></p>
    <table>
      <tr>
        <td id="bare"></td>
        <td id="spaced">
        </td>
        <td id="full">text</td>
      </tr>
    </table>
    <form>
      <input id="ticked" type="checkbox" checked="checked" disabled="disabled"/>
      <input id="plain" type="text" required="required"/>
    </form>
    <p id="rtl" dir="rtl"><span id="inherits">child</span></p>
    <p id="auto" dir="auto"><span id="unresolved">child</span></p>
  </body>
</html>
"#;
    let dom = xhtml::read(markup.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT).expect("markup");
    assert!(dom.defects.is_empty(), "{:?}", dom.defects);

    let at = |id: &str| dom.by_id(id).unwrap_or_else(|| panic!("no #{id}"));
    let hits = |selector: &str, index: usize| {
        let limits = Limits::DEFAULT;
        let mut budget = Budget::new(&limits);
        let sheet = parse(
            format!("{selector} {{ color: red }}").as_bytes(),
            None,
            &NoImports,
            &MediaContext::screen(432.0, 648.0),
            &limits,
            &mut budget,
        )
        .expect("a one-rule sheet");
        assert_eq!(sheet.rules.len(), 1, "`{selector}` did not parse");
        tinker_pdf_css::selector::matches(
            &sheet.rules[0].selectors[0],
            &dom.nodes,
            index,
            &mut budget,
        )
        .expect("under every cap")
    };

    // `:nth-child()` over a real `<ul>`, where the sibling links were built by
    // the reader rather than written down by a fixture — and where the text
    // between the `<li>`s is not a sibling.
    let items: Vec<usize> = (0..dom.nodes.len())
        .filter(|index| dom.nodes[*index].name == "li")
        .collect();
    assert_eq!(items.len(), 4);
    assert!(hits("li:nth-child(odd)", items[0]));
    assert!(!hits("li:nth-child(odd)", items[1]));
    assert!(hits("li:nth-child(2n)", items[1]));
    assert!(hits("li:nth-last-child(1)", items[3]));
    assert!(!hits("li:nth-last-child(1)", items[2]));
    assert!(hits("li:first-of-type", items[0]));
    assert!(hits("li:last-of-type", items[3]));
    assert!(hits("ul:has(> li)", at("list")));
    assert!(!hits("ul:has(> p)", at("list")));

    // `xml:lang` beats `lang`, and both are found from a descendant: the
    // declaration is on `<html>` and the question is about a `<li>`.
    assert!(hits("li:lang(en)", items[0]));
    assert!(hits("li:lang(en-GB)", items[0]));
    assert!(
        !hits("li:lang(fr)", items[0]),
        "`lang=fr` is shadowed by `xml:lang=en-GB` on the same element"
    );

    // §6.6.1's link, which is the attribute and not the element name.
    assert!(hits("a:link", at("hyper")));
    assert!(hits("a:any-link", at("hyper")));
    assert!(!hits("a:link", at("anchor")), "an `<a>` with no href");
    assert!(!hits("a:visited", at("hyper")));

    // §6.6.3, and the whole reason it is the document language's answer: the
    // second cell holds a newline and two spaces, and it is still empty.
    assert!(hits("td:empty", at("bare")));
    assert!(hits("td:empty", at("spaced")), "indentation is not content");
    assert!(!hits("td:empty", at("full")));

    // §12, over HTML's own vocabulary.
    assert!(hits("input:checked", at("ticked")));
    assert!(hits("input:disabled", at("ticked")));
    assert!(!hits("input:enabled", at("ticked")));
    assert!(!hits("input:checked", at("plain")));
    assert!(hits("input:enabled", at("plain")));
    assert!(hits("input:required", at("plain")));
    assert!(hits("input:optional", at("ticked")));
    assert!(hits("input:read-write", at("plain")));
    // HTML's rule, stated out loud because it surprises: everything that is
    // not editable is `:read-only`, a paragraph included.
    assert!(hits("p:read-only", at("linked")));
    assert!(!hits("p:read-write", at("linked")));
    assert!(!hits("p:enabled", at("linked")), "a `<p>` is neither");
    assert!(!hits("p:disabled", at("linked")));

    // §6.6's direction, inherited down and unresolved where HTML says the
    // content decides.
    assert!(hits("span:dir(rtl)", at("inherits")));
    assert!(!hits("span:dir(ltr)", at("inherits")));
    assert!(!hits("span:dir(rtl)", at("unresolved")), "`dir=auto`");
    assert!(!hits("span:dir(ltr)", at("unresolved")));
    // And the document element's default, which is HTML's and not this
    // crate's: a book that never writes `dir` is `ltr` throughout.
    assert!(hits("li:dir(ltr)", items[0]));

    // The seven that never match, over a real document: each is parsed, each
    // is counted, and none of them styles anything.
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let sheet = parse(
        b"a:hover, a:focus, a:focus-within, a:focus-visible, a:active, a:target, a:visited \
          { color: red }",
        None,
        &NoImports,
        &MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
    .expect("a one-rule sheet");
    let stateless: usize = sheet
        .report
        .warnings
        .iter()
        .filter(|(warning, _)| {
            matches!(warning, tinker_pdf_css::Warning::PseudoClassUnsupported(_))
        })
        .map(|(_, count)| *count)
        .sum();
    assert_eq!(stateless, 7, "{:?}", sheet.report.warnings);
    for selector in &sheet.rules[0].selectors {
        for index in 0..dom.nodes.len() {
            assert!(
                !tinker_pdf_css::selector::matches(selector, &dom.nodes, index, &mut budget)
                    .expect("under every cap"),
                "a state this document does not have matched element {index}"
            );
        }
    }
}
