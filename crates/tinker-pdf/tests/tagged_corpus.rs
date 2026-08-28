//! The structure-tree census over the fetched corpora (gap 14, milestone 1).
//!
//! `Document::structure` has to yield a tree on real tagged files, and the
//! only honest way to say so is to run it over all of them and print what came
//! back. The corpora are **fetched, never committed** (`corpus/corpora.lock`),
//! so this cannot run on a clean checkout and is `#[ignore]`d:
//!
//! ```text
//! cargo xtask corpus-fetch
//! TINKER_CORPUS=corpus/files \
//!   cargo test --release -p tinker-pdf --test tagged_corpus -- --ignored --nocapture
//! ```
//!
//! `TINKER_CORPUS` names the directory the fetch writes into — `corpus/files`,
//! holding one subdirectory per corpus — and **not** one corpus inside it. The
//! distinction is not cosmetic: the count of files carrying a
//! `/StructTreeRoot` is 716 across the four fetched corpora and 588 in
//! veraPDF's alone, and a census pointed at the wrong root asserts a number it
//! cannot reach and reads as a regression. The per-corpus table below exists so
//! that mistake reports itself.
//!
//! # Skipped, not silently passed
//!
//! A test over bytes that are not in the repository can fail to run for a
//! reason that looks exactly like a pass, so it prints [`RAN`] or [`SKIPPED`]
//! and a job depending on it greps its own output for the second
//! (`docs/verification.md`).
//!
//! # Ruling 13
//!
//! Nothing outside this repository measures anything here. The corpora supply
//! **bytes**, with provenance pinned in the lock; every count below is this
//! engine reading them. The one thing taken from upstream is which files are
//! PDF/UA, which the veraPDF corpus states in its own directory names — a fact
//! about how the corpus is organised, not a verdict about a document. Diffing
//! this engine's answers against upstream's pass/fail annotations is milestone
//! 5 and is deliberately not done here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, StructureWarning};

/// Printed once when the census actually read the corpora. CI greps it.
const RAN: &str = "structure-census: RAN";

/// Printed once when it could not. CI greps for this one too, and fails.
const SKIPPED: &str = "structure-census: SKIPPED";

/// Every `.pdf` under `TINKER_CORPUS`, in path order, with the corpus it
/// belongs to.
///
/// A directory that exists **and holds at least one PDF**, rather than a
/// directory that exists: an interrupted fetch leaves an empty tree, and a
/// sweep over nothing passes.
///
/// By extension, because that is what the corpora themselves mean by a
/// fixture — pdf.js stores `*.pdf.link` files for the ones it does not
/// redistribute, and a census that opened those would report several hundred
/// unreadable URLs.
fn corpus() -> Option<Vec<(String, PathBuf)>> {
    let root = PathBuf::from(std::env::var_os("TINKER_CORPUS")?);
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                let name = path
                    .strip_prefix(&root)
                    .ok()
                    .and_then(|rest| rest.components().next())
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .unwrap_or_else(|| "?".to_string());
                out.push((name, path));
            }
        }
    }
    if out.is_empty() {
        return None;
    }
    out.sort();
    Some(out)
}

/// Whether the corpus files this path under one of its PDF/UA directories.
///
/// The corpus's own organisation, read as a fact about where a file sits. It
/// is not a verdict about the file: `PDF_UA-1` holds both conforming and
/// deliberately non-conforming fixtures, which is exactly what makes milestone
/// 5's diff a separate piece of work.
fn is_pdf_ua(path: &Path) -> bool {
    path.components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with("PDF_UA"))
}

/// What one file contributed.
#[derive(Default)]
struct Row {
    opened: bool,
    tree: bool,
    marked: bool,
    suspects: bool,
    elements: usize,
    /// How many *distinct* indirect objects those elements came from.
    ///
    /// The `/K` walk's visited set is scoped to the path from the root
    /// (the discipline `trees.rs` uses), so a subtree named from two places is
    /// read twice — which is right for a file that legitimately shares one and
    /// is also how a hostile file inflates its own element count. This is the
    /// number that tells the two apart, and a corpus where it tracks
    /// `elements` closely is a corpus of trees rather than of graphs.
    distinct: usize,
    content: usize,
    objects: usize,
    matched: usize,
    orphans: usize,
    unmarked: usize,
    warnings: Vec<&'static str>,
}

/// What one corpus contributed.
#[derive(Default)]
struct Totals {
    files: usize,
    unopened: usize,
    trees: usize,
    marked: usize,
    elements: usize,
    content: usize,
}

fn kind(warning: &StructureWarning) -> &'static str {
    match warning {
        StructureWarning::KidCycle { .. } => "kid-cycle",
        StructureWarning::DepthCapped { .. } => "depth-capped",
        StructureWarning::ElementCapped => "element-capped",
        StructureWarning::KidsCapped { .. } => "kids-capped",
        StructureWarning::UntypedElement { .. } => "untyped-element",
        StructureWarning::UnreadableKid { .. } => "unreadable-kid",
        StructureWarning::RoleMapLoop { .. } => "role-map-loop",
        StructureWarning::ParentTreeDisagreement { .. } => "parent-tree-disagreement",
    }
}

/// Reads one file. Never fails: a corpus file that will not open is a fact
/// about the corpus, counted as such rather than as a file with no tree.
fn census_of(path: &Path) -> Row {
    let mut row = Row::default();
    let Ok(bytes) = std::fs::read(path) else {
        return row;
    };
    let Ok(doc) = Document::open(bytes) else {
        return row;
    };
    row.opened = true;
    let Some(tree) = doc.structure() else {
        return row;
    };

    row.tree = true;
    row.marked = tree.marked;
    row.suspects = tree.suspects;
    row.elements = tree.element_count();
    row.distinct = tree
        .elements()
        .iter()
        .filter_map(|element| element.reference)
        .collect::<BTreeSet<_>>()
        .len();
    row.content = tree.content_count();
    row.objects = tree.object_count();
    for warning in &tree.warnings {
        row.warnings.push(kind(warning));
    }

    // Bounded: the census is about how far the join gets, not about extracting
    // every page of every file, and a document claiming a hundred thousand
    // pages would turn a measurement into a timeout.
    for index in 0..doc.page_count().min(8) {
        let Some(page) = doc.page(index) else {
            continue;
        };
        let structured = tree.text_for_page(index, &page.text());
        row.matched += structured.matched;
        row.orphans += structured.orphans;
        row.unmarked += structured.unmarked;
        for warning in &structured.warnings {
            row.warnings.push(kind(warning));
        }
    }
    row
}

/// The census, printed in full and pinned by its file counts.
///
/// The hard numbers are counts of *files*: how many were read, how many carry
/// a `/StructTreeRoot` this engine could read, and how many the veraPDF corpus
/// files as PDF/UA. Each moves only if this engine stopped finding a tree it
/// used to find, or the corpora changed under the lock — and either is
/// something to look at rather than to re-record.
///
/// The aggregate element and character counts are printed and asserted as
/// **floors**. They are what milestone 4 turns into ratchet bars in
/// `corpus/ratchet.json`; until then a floor is what says the walk did not
/// quietly stop finding things, without pinning this test to a number a
/// genuine improvement in the COS layer would move upward.
#[test]
#[ignore = "reads the fetched corpora; set TINKER_CORPUS=corpus/files"]
fn the_fetched_corpora_yield_structure_trees() {
    let Some(files) = corpus() else {
        println!(
            "{SKIPPED} the structure census -- TINKER_CORPUS is unset or holds \
             no PDFs; fetch with `cargo xtask corpus-fetch` and point it at \
             corpus/files"
        );
        return;
    };
    println!("{RAN} the structure census ({} files)", files.len());

    let mut per_corpus: BTreeMap<String, Totals> = BTreeMap::new();
    let mut unopened = 0usize;
    let mut with_tree = 0usize;
    let mut marked = 0usize;
    let mut suspects = 0usize;
    let mut ua_files = 0usize;
    let mut ua_with_tree = 0usize;
    let mut ua_marked = 0usize;
    let (mut elements, mut content, mut objects) = (0usize, 0usize, 0usize);
    let mut distinct = 0usize;
    let (mut matched, mut orphans, mut unmarked) = (0usize, 0usize, 0usize);
    let mut warnings: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut biggest: (usize, String) = (0, String::new());

    for (corpus_name, path) in &files {
        let row = census_of(path);
        let ua = is_pdf_ua(path);
        let totals = per_corpus.entry(corpus_name.clone()).or_default();
        totals.files += 1;
        totals.elements += row.elements;
        totals.content += row.content;

        if !row.opened {
            unopened += 1;
            totals.unopened += 1;
        }
        if ua {
            ua_files += 1;
        }
        if row.tree {
            with_tree += 1;
            totals.trees += 1;
            if ua {
                ua_with_tree += 1;
            }
            if row.elements > biggest.0 {
                biggest = (row.elements, path.display().to_string());
            }
        }
        if row.marked {
            marked += 1;
            totals.marked += 1;
            if ua {
                ua_marked += 1;
            }
        }
        if row.suspects {
            suspects += 1;
        }
        elements += row.elements;
        distinct += row.distinct;
        content += row.content;
        objects += row.objects;
        matched += row.matched;
        orphans += row.orphans;
        unmarked += row.unmarked;
        for name in row.warnings {
            *warnings.entry(name).or_default() += 1;
        }
    }

    println!();
    println!("corpus           files  unopened  trees  marked  elements  content");
    for (name, totals) in &per_corpus {
        println!(
            "{name:<15} {:>5}  {:>8}  {:>5}  {:>6}  {:>8}  {:>7}",
            totals.files,
            totals.unopened,
            totals.trees,
            totals.marked,
            totals.elements,
            totals.content,
        );
    }
    println!();
    println!("files scanned            {}", files.len());
    println!("did not open             {unopened}");
    println!("with /StructTreeRoot     {with_tree}");
    println!("/MarkInfo /Marked true   {marked}");
    println!("/MarkInfo /Suspects true {suspects}");
    println!("PDF/UA files (by path)   {ua_files}");
    println!("  of those, with a tree  {ua_with_tree}");
    println!("  of those, marked       {ua_marked}");
    println!("struct elements          {elements}");
    println!("  distinct objects       {distinct}");
    println!("struct content items     {content}");
    println!("struct object refs       {objects}");
    println!("chars matched            {matched}");
    println!("chars orphaned           {orphans}");
    println!("chars unmarked           {unmarked}");
    println!("largest tree             {} in {}", biggest.0, biggest.1);
    if warnings.is_empty() {
        println!("warnings                 none");
    }
    for (name, count) in &warnings {
        println!("warn {name:<24} {count}");
    }

    assert_eq!(
        files.len(),
        4605,
        "TINKER_CORPUS is not corpus/files with every corpus in \
         corpus/corpora.lock fetched"
    );
    assert_eq!(
        with_tree, 716,
        "files carrying a /StructTreeRoot this engine could read"
    );
    assert_eq!(
        ua_files, 434,
        "files the veraPDF corpus files under PDF_UA-*"
    );

    assert!(
        elements >= 4000,
        "the corpora yielded {elements} structure elements, fewer than they \
         hold: the walk stopped finding them"
    );
    assert!(
        content >= 2000,
        "the corpora yielded {content} content items"
    );
    assert!(
        matched > orphans,
        "more characters were orphaned ({orphans}) than matched ({matched}): \
         the join, not the corpora, is what would have to be wrong"
    );
}
