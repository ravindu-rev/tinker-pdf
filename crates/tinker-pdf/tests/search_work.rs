//! Search over prebuilt text, counted (`docs/design/performance.md`, P-24):
//! a page's text is folded once a fold however many searches read it, its
//! word boundaries computed once a fold, a search's matcher and mapping are
//! linear in its text and its hits, and what a search finds is what a page
//! that has kept nothing finds.
//!
//! Every type is named from `tinker_pdf` (ruling 11), and every document here
//! is built in the test and held through the validate-on-save helper.

mod written_support;

use std::collections::BTreeMap;

use tinker_pdf::{
    Document, Quad, SearchOptions, TextBlock, TextChar, TextLine, TextPage, WritingMode,
};
use tinker_pdf_cos::DocumentBuilder;

/// The words a generated page draws, WinAnsi-coded as a content stream
/// writes them: accented, capitalised and repeated, so each fold parts from
/// the others somewhere.
const WORDS: [&str; 16] = [
    "r\\351sum\\351",
    "resume",
    "RESUME",
    "R\\311SUM\\311",
    "caf\\351",
    "cafe",
    "M\\374ller",
    "muller",
    "office",
    "invoice",
    "total",
    "Tinker",
    "tinker",
    "aa",
    "aaaa",
    "page",
];

/// A generated document of `pages` pages, each six lines of eight of
/// [`WORDS`] chosen by a fixed sequence, and its page number.
fn document(pages: usize) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    let mut state = 0x2545_F491u32;
    for index in 0..pages {
        let mut content = String::from("BT /F0 10 Tf 20 370 Td 14 TL\n");
        for _ in 0..6 {
            let mut line = Vec::new();
            for _ in 0..8 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                line.push(WORDS[(state >> 16) as usize % WORDS.len()]);
            }
            content.push_str(&format!("({}) Tj T*\n", line.join(" ")));
        }
        content.push_str(&format!("(page {index}) Tj\nET"));
        builder.add_page(400.0, 400.0, |page| page.raw(content.as_bytes()));
    }
    written_support::authored("generated search pages", builder.finish())
}

/// The option sets searched: each of the four folds, and whole-word in two
/// of them, which fold as their case and accents say.
const OPTIONS: [SearchOptions; 6] = [
    SearchOptions {
        case_sensitive: false,
        whole_word: false,
        diacritic_insensitive: false,
    },
    SearchOptions {
        case_sensitive: true,
        whole_word: false,
        diacritic_insensitive: false,
    },
    SearchOptions {
        case_sensitive: false,
        whole_word: false,
        diacritic_insensitive: true,
    },
    SearchOptions {
        case_sensitive: true,
        whole_word: false,
        diacritic_insensitive: true,
    },
    SearchOptions {
        case_sensitive: false,
        whole_word: true,
        diacritic_insensitive: false,
    },
    SearchOptions {
        case_sensitive: true,
        whole_word: true,
        diacritic_insensitive: true,
    },
];

/// Ten needles: words in each case and with and without their accents,
/// repeats and a miss.
const NEEDLES: [&str; 10] = [
    "resume",
    "r\u{e9}sum\u{e9}",
    "RESUME",
    "cafe",
    "m\u{fc}ller",
    "office",
    "aa",
    "tinker",
    "page 7",
    "zzz",
];

/// Page `at`'s text.
fn text_of(document: &Document, at: usize) -> TextPage {
    let index = u32::try_from(at).expect("a page number");
    document.page(index).expect("a page").text()
}

/// The fold `options` search in, as a number: case, then accents.
fn fold(options: &SearchOptions) -> usize {
    usize::from(options.case_sensitive) * 2 + usize::from(options.diacritic_insensitive)
}

/// Every quad's corners as bits, so two answers compare to the bit.
fn bits(quads: &[Quad]) -> Vec<[u64; 8]> {
    quads
        .iter()
        .map(|q| {
            [
                q.ul.0, q.ul.1, q.ur.0, q.ur.1, q.ll.0, q.ll.1, q.lr.0, q.lr.1,
            ]
            .map(f64::to_bits)
        })
        .collect()
}

/// **Folding happens once a page and fold, whatever the searches.** Ten
/// searches in each option set over a generated 100-page document's
/// prebuilt text fold each page at most once in each fold — exactly once in
/// each fold a search asked for — by `text_forms_built`; every search
/// returns, to the bit, what the same search of the page extracted afresh
/// returns; and a second round, through the kept forms, returns the first
/// round's quads and folds nothing.
#[test]
fn folding_happens_once_per_page_whatever_the_searches() {
    const PAGES: usize = 100;
    let document = document(PAGES);
    let pages: Vec<TextPage> = (0..PAGES).map(|at| text_of(&document, at)).collect();
    let mut built: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    let mut first: BTreeMap<(usize, usize, usize), Vec<[u64; 8]>> = BTreeMap::new();
    let mut found = [0usize; OPTIONS.len()];
    for (o, options) in OPTIONS.iter().enumerate() {
        for (n, needle) in NEEDLES.iter().enumerate() {
            for (at, page) in pages.iter().enumerate() {
                let (quads, stats) = page.search_counted(needle, options);
                *built.entry((at, fold(options))).or_insert(0) += stats.text_forms_built;
                found[o] += quads.len();
                first.insert((o, n, at), bits(&quads));
            }
        }
    }
    for at in 0..PAGES {
        for f in 0..4 {
            assert_eq!(
                built.get(&(at, f)).copied().unwrap_or(0),
                1,
                "page {at}, fold {f}: folded once"
            );
        }
    }
    assert!(
        found.iter().all(|&n| n > 0),
        "every option set finds something: {found:?}"
    );
    assert!(
        found[0] < found[2],
        "ignoring accents finds more than keeping them: {found:?}"
    );
    assert!(found[1] < found[0], "matching case finds less: {found:?}");

    let afresh: Vec<TextPage> = (0..PAGES).map(|at| text_of(&document, at)).collect();
    for (o, options) in OPTIONS.iter().enumerate() {
        for (n, needle) in NEEDLES.iter().enumerate() {
            for (at, page) in pages.iter().enumerate() {
                let (quads, stats) = page.search_counted(needle, options);
                assert_eq!(
                    stats.text_forms_built, 0,
                    "page {at}: {needle:?} folded again"
                );
                let want = first.get(&(o, n, at)).expect("the first round's");
                assert_eq!(&bits(&quads), want, "page {at}, {needle:?}, {options:?}");
                assert_eq!(
                    &bits(&afresh[at].search_with(needle, options)),
                    want,
                    "page {at}, {needle:?}, {options:?}: a page that kept nothing"
                );
            }
        }
    }
    // `search` reads the default options' form, which is kept already.
    for page in &pages {
        let _ = page.search("resume");
        let (_, stats) = page.search_counted("tinker", &SearchOptions::default());
        assert_eq!(stats.text_forms_built, 0);
    }
}

/// **A form does not outlive a change to its blocks.** A page searched in
/// every fold and then changed through `blocks_mut` — its lines reversed, a
/// glyph dropped, a glyph's text edited — finds what a page built afresh from
/// the changed blocks finds, and folds again once.
#[test]
fn a_form_does_not_outlive_a_change_to_its_blocks() {
    let document = document(3);
    type Change = fn(&mut Vec<tinker_pdf::TextBlock>);
    let changes: [(&str, Change); 3] = [
        ("lines reversed", |blocks| {
            for block in blocks.iter_mut() {
                block.lines.reverse();
            }
            blocks.reverse();
        }),
        ("a glyph dropped", |blocks| {
            if let Some(line) = blocks.first_mut().and_then(|b| b.lines.first_mut()) {
                if line.chars.len() > 2 {
                    line.chars.remove(2);
                }
            }
        }),
        ("a glyph edited", |blocks| {
            for line in blocks.iter_mut().flat_map(|b| b.lines.iter_mut()) {
                if let Some(c) = line.chars.first_mut() {
                    c.text = "Q".to_string();
                }
            }
        }),
    ];
    for (what, change) in changes {
        for at in 0..3 {
            let mut kept = document.page(at).expect("a page").text();
            for options in &OPTIONS {
                for needle in NEEDLES {
                    let _ = kept.search_with(needle, options);
                }
            }
            change(kept.blocks_mut());
            let afresh = TextPage::new(
                kept.blocks().to_vec(),
                kept.mcid_props.clone(),
                kept.stated.clone(),
                kept.warnings.clone(),
            );
            let mut folded = BTreeMap::new();
            for options in &OPTIONS {
                for needle in NEEDLES.iter().chain(&["q", "Q"]) {
                    let (quads, stats) = kept.search_counted(needle, options);
                    *folded.entry(fold(options)).or_insert(0u64) += stats.text_forms_built;
                    assert_eq!(
                        bits(&quads),
                        bits(&afresh.search_with(needle, options)),
                        "{what}, page {at}, {needle:?}, {options:?}"
                    );
                }
            }
            assert!(
                folded.values().all(|&n| n == 1),
                "{what}, page {at}: each fold folded again once: {folded:?}"
            );
        }
    }
}

/// A page of one line, `text`, a glyph a character, each five units wide.
fn line_page(text: &str) -> TextPage {
    let quad = |x: f64, width: f64| Quad {
        ul: (x, 710.0),
        ur: (x + width, 710.0),
        ll: (x, 700.0),
        lr: (x + width, 700.0),
    };
    let chars: Vec<TextChar> = text
        .chars()
        .enumerate()
        .map(|(at, c)| TextChar {
            text: c.to_string(),
            quad: quad(at as f64 * 5.0, 5.0),
            size: 10.0,
            origin: (at as f64 * 5.0, 700.0),
            mcid: None,
            stream: 0,
            font: None,
            stated: None,
            artifact: false,
        })
        .collect();
    let bounds = quad(0.0, chars.len() as f64 * 5.0);
    TextPage::new(
        vec![TextBlock {
            quad: bounds,
            lines: vec![TextLine {
                chars,
                text: text.to_string(),
                quad: bounds,
                wmode: WritingMode::Horizontal,
                rtl: false,
                size: 10.0,
            }],
        }],
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
    )
}

/// **A search is linear in its text and its hits.** On a 4 000-character
/// line of `a a a …`, searched for `a`, which every word is, and for `a a`,
/// which every other pair is, plain and whole-word, the matcher's and the
/// mapping's steps (`search_steps`) are at most four times the folded length
/// plus the hits, and a line a tenth as long takes about a tenth of them; the
/// hits are what restarting the search after each would find.
#[test]
fn a_search_is_linear_in_its_text_and_its_hits() {
    let line = |words: usize| vec!["a"; words].join(" ");
    let (long, short) = (line(2_000), line(200));
    assert_eq!(long.len(), 3_999);
    let (long_page, short_page) = (line_page(&long), line_page(&short));
    let whole = SearchOptions {
        whole_word: true,
        ..SearchOptions::default()
    };
    for options in [SearchOptions::default(), whole] {
        for (needle, hits) in [("a", 2_000), ("a a", 1_000)] {
            let (quads, stats) = long_page.search_counted(needle, &options);
            assert_eq!(quads.len(), hits, "{needle:?}, {options:?}");
            let bound = 4 * (long.len() + quads.len()) as u64;
            assert!(
                stats.search_steps <= bound,
                "{needle:?}, {options:?}: {} steps, past {bound}",
                stats.search_steps
            );
            assert!(
                stats.search_steps >= long.len() as u64,
                "every byte is read"
            );
            let (few, at_short) = short_page.search_counted(needle, &options);
            assert_eq!(few.len() * 10, quads.len(), "{needle:?}, {options:?}");
            assert!(
                stats.search_steps <= 11 * at_short.search_steps,
                "{needle:?}, {options:?}: {} steps against {} at a tenth the length",
                stats.search_steps,
                at_short.search_steps
            );
            // Each hit a word on from the last, its own glyphs.
            let step = if needle == "a" { 10.0 } else { 20.0 };
            for (index, quad) in quads.iter().enumerate() {
                let (x0, _, x1, _) = quad.bounds();
                let at = index as f64 * step;
                assert!((x0 - at).abs() < 1e-9, "{needle:?}: hit {index} at {x0}");
                let width = if needle == "a" { 5.0 } else { 15.0 };
                assert!((x1 - x0 - width).abs() < 1e-9, "{needle:?}: hit {index}");
            }
        }
    }
}

/// **Word boundaries are computed once a page and fold**, across any number
/// of whole-word searches, by `word_boundaries_computed`; a search that is not
/// whole-word computes none.
#[test]
fn boundaries_are_computed_once_per_page() {
    const PAGES: usize = 20;
    let document = document(PAGES);
    let pages: Vec<TextPage> = (0..PAGES).map(|at| text_of(&document, at)).collect();
    let mut computed: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    for options in &OPTIONS {
        for needle in NEEDLES {
            for (at, page) in pages.iter().enumerate() {
                let (_, stats) = page.search_counted(needle, options);
                if !options.whole_word {
                    assert_eq!(stats.word_boundaries_computed, 0, "{needle:?}, {options:?}");
                }
                *computed.entry((at, fold(options))).or_insert(0) += stats.word_boundaries_computed;
            }
        }
    }
    for at in 0..PAGES {
        for options in OPTIONS.iter().filter(|o| o.whole_word) {
            assert_eq!(
                computed.get(&(at, fold(options))).copied(),
                Some(1),
                "page {at}, {options:?}: computed once"
            );
        }
    }
    let plain_only: Vec<usize> = OPTIONS
        .iter()
        .filter(|o| !OPTIONS.iter().any(|w| w.whole_word && fold(w) == fold(o)))
        .map(fold)
        .collect();
    for at in 0..PAGES {
        for &f in &plain_only {
            assert_eq!(
                computed.get(&(at, f)).copied(),
                Some(0),
                "page {at}, fold {f}"
            );
        }
    }
}
