//! The counted-work guard suite (`docs/design/performance.md`, "The guard
//! suite"): the inputs the guards read, the roster of every guard with the row
//! that owns it, counts pinned so that every target is seen to count alike,
//! and the rule that the suite reads no clock.
//!
//! A guard is a relation over the counts a door hands back as a `Stats` — a
//! bound, or one count at two sizes — and never a time. A guard today's code
//! fails is owed to the row that fixes its cause; that row adds the test and
//! flips its roster entry to landed in the commit that makes it hold.
//!
//! Every input is written here, object by object, so that its shape is the
//! generator's statement rather than a writer's choice, and each generator
//! has a test that holds it to its shape. Nothing is read from disk or
//! fetched: the `wasm32-wasip1` leg has no filesystem. A page that draws text
//! draws it in an embedded face (`render_support::curvy_font`), so the
//! default and `bundled-fonts` builds count the same work.

mod epub_support;
mod guard_support;
mod render_support;
mod written_support;

use std::collections::BTreeSet;
use std::sync::Arc;

use epub_support::book::generated_book;
use guard_support::{content, deflated, document, face, line, object, pdf, stream, A2_PAGES};
use tinker_pdf::write::{
    BilevelCodec, ContinuousCodec, FontPolicy, ImagePolicy, ImageRecoding, SaveOptions,
};
use tinker_pdf::{Document, OpenOptions, RenderOptions, SimpleFontProvider, Stats, TextOptions};
use tinker_pdf_cos::Object;

// ---- The roster -------------------------------------------------------------

/// Whether a guard's test is in the suite yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Its test is an un-ignored `#[test]` in the file it names.
    Landed,
    /// Its test fails against today's code, and the row named adds it with
    /// the fix.
    Owed,
}

/// One guard test.
struct Guard {
    /// The guard the row's exit names, `A1` to `A10`.
    guard: &'static str,
    /// What it holds, as the exit states it.
    clause: &'static str,
    /// The test's name.
    test: &'static str,
    /// The file under `crates/tinker-pdf/tests/` the test is in.
    file: &'static str,
    /// The row that owns it.
    owner: &'static str,
    /// The step of that row it lands in.
    lands: &'static str,
    /// Whether it has.
    state: State,
}

/// A guard owed to `owner`, whose test will be in this file.
const fn owed(
    guard: &'static str,
    clause: &'static str,
    test: &'static str,
    owner: &'static str,
    lands: &'static str,
) -> Guard {
    Guard {
        guard,
        clause,
        test,
        file: "guards.rs",
        owner,
        lands,
        state: State::Owed,
    }
}

/// Every guard test, with the row that owns it, as the design's roster gives
/// them. Each entry starts owed, because each fails against today's code; its
/// row flips it in the commit that fixes the cause. A2's 1 000-chapter book
/// entered landed, with P-18's guards on every producer.
///
/// A10's entries follow `guard_allocations.rs`'s table: every operation it
/// counts allocates a page in proportion to the document, through its lookup
/// of the page, so each is owed to P-01's index. Their tests will be in that
/// file, whose allocator counts a test's own thread.
const ROSTER: &[Guard] = &[
    Guard {
        state: State::Landed,
        ..owed(
            "A1",
            "page(i) visits its root and at most depth x fan-out nodes under it",
            "a1_page_lookup_reads_at_most_depth_times_fan_out_nodes",
            "P-01",
            "P-01.2",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A1",
            "every page of a generated 10 000-page document costs at most 3 x 10 000 visits",
            "a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "text: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_text_is_linear_in_pages",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "render-all at 36 dpi: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_render_all_at_36_dpi_is_linear_in_pages",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "rewrite: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_rewrite_is_linear_in_pages",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "rotate: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_rotate_is_linear_in_pages",
            "P-01",
            "P-01.4",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "merge: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_merge_is_linear_in_pages",
            "P-01",
            "P-01.4",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "split: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_split_is_linear_in_pages",
            "P-01",
            "P-01.4",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "subset: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_subset_is_linear_in_pages",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "redaction: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_redaction_is_linear_in_pages",
            "P-01",
            "P-01.4",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "validation: work at 4 000 pages at most 4.4 x work at 1 000",
            "a2_validation_is_linear_in_pages",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A2",
            "a generated book: work at 1 000 chapters at most 11 x work at 100",
            "a2_a_thousand_chapter_book_costs_at_most_eleven_times_a_hundred",
            "P-18",
            "P-18.5",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A3",
            "font dictionaries resolved per page at most the distinct /Font entries used",
            "a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses",
            "P-02",
            "P-02.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A4",
            "glyph masks rasterised at most the distinct (font, glyph, scale, phase) keys, \
             hit ratio at least 98 % on a 5 600-glyph page",
            "a4_glyph_masks_rasterised_are_at_most_their_distinct_keys",
            "P-06",
            "P-06.3",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A5",
            "inflate calls per stream at most 1 per operation: a save",
            "a5_a_save_decodes_each_stream_at_most_once",
            "P-03",
            "P-03.2",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A5",
            "inflate calls per stream at most 1 per operation: a render",
            "a5_a_render_decodes_each_stream_at_most_once",
            "P-03",
            "P-03.2",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A5",
            "inflate calls per stream at most 1 per operation: a text extraction",
            "a5_text_decodes_each_stream_at_most_once",
            "P-03",
            "P-03.2",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A5",
            "inflate calls per stream at most 1 per operation: image extraction",
            "a5_image_extraction_decodes_each_stream_at_most_once",
            "P-03",
            "P-03.2",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A5",
            "inflate calls per stream at most 1 per operation: a merge",
            "a5_a_merge_decodes_each_stream_at_most_once",
            "P-03",
            "P-03.4",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A6",
            "merge deduplication decodes no byte-identical copy, and each differing encoding \
             under one dictionary once a save",
            "a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once",
            "P-03",
            "P-03.6",
        )
    },
    Guard {
        file: "images.rs",
        ..owed(
            "A7",
            "IDCT output at most 4 x drawn pixels for a quarter-scale draw",
            "a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel",
            "P-09",
            "P-09.8",
        )
    },
    Guard {
        file: "jpx_images.rs",
        ..owed(
            "A7",
            "JPX levels decoded at most the levels needed",
            "a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer",
            "P-10",
            "P-10.8",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A8",
            "whole-image scratch on the PNG path at most 1 x the image",
            "a8_the_png_path_holds_at_most_one_image_of_scratch",
            "P-04",
            "P-04.5",
        )
    },
    Guard {
        state: State::Landed,
        ..owed(
            "A9",
            "page-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads",
            "a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads",
            "P-15",
            "P-15.3",
        )
    },
    owed(
        "A9",
        "band-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads",
        "a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads",
        "P-15",
        "P-15.7",
    ),
    Guard {
        file: "guard_allocations.rs",
        state: State::Landed,
        ..owed(
            "A10",
            "allocations O(1) per page: page lookup",
            "a10_page_lookup_allocates_a_flat_amount_per_page",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        file: "guard_allocations.rs",
        state: State::Landed,
        ..owed(
            "A10",
            "allocations O(1) per page: text",
            "a10_text_allocates_a_flat_amount_per_page",
            "P-01",
            "P-01.3",
        )
    },
    Guard {
        file: "guard_allocations.rs",
        state: State::Landed,
        ..owed(
            "A10",
            "allocations O(1) per page: render-all at 36 dpi",
            "a10_render_all_at_36_dpi_allocates_a_flat_amount_per_page",
            "P-01",
            "P-01.3",
        )
    },
];

/// Each file the roster names, read when the suite is built: the wasm leg
/// has no filesystem to read it from when the suite runs.
const SOURCES: &[(&str, &str)] = &[
    ("guards.rs", include_str!("guards.rs")),
    ("guard_allocations.rs", include_str!("guard_allocations.rs")),
    ("images.rs", include_str!("images.rs")),
    ("jpx_images.rs", include_str!("jpx_images.rs")),
];

/// The other sources the suite is built from, which hold no guard.
const SUPPORT: &[(&str, &str)] = &[
    ("guard_support/mod.rs", include_str!("guard_support/mod.rs")),
    (
        "render_support/mod.rs",
        include_str!("render_support/mod.rs"),
    ),
    ("epub_support/mod.rs", include_str!("epub_support/mod.rs")),
    ("epub_support/book.rs", include_str!("epub_support/book.rs")),
    (
        "epub_support/conservation.rs",
        include_str!("epub_support/conservation.rs"),
    ),
    (
        "epub_support/layout.rs",
        include_str!("epub_support/layout.rs"),
    ),
    (
        "epub_support/reading.rs",
        include_str!("epub_support/reading.rs"),
    ),
    (
        "epub_support/typeface.rs",
        include_str!("epub_support/typeface.rs"),
    ),
];

/// What a source has under a function's name.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    /// A `#[test]` that runs.
    Runs,
    /// A `#[test]` that is ignored.
    Ignored,
    /// A function that is not a test.
    NotATest,
}

/// What `source` has named `name`, by the attributes above `fn name(`; `None`
/// when it has no such function.
fn find(source: &str, name: &str) -> Option<Found> {
    let at = source.find(&format!("fn {name}("))?;
    let above = &source[..source[..at].rfind('\n').unwrap_or(0)];
    let attributes: Vec<&str> = above
        .lines()
        .rev()
        .map(str::trim)
        .take_while(|line| line.starts_with("#[") || line.starts_with("//"))
        .filter(|line| line.starts_with("#["))
        .collect();
    let test = attributes.contains(&"#[test]");
    let ignored = attributes.iter().any(|line| line.contains("ignore"));
    Some(match (test, ignored) {
        (true, false) => Found::Runs,
        (true, true) => Found::Ignored,
        (false, _) => Found::NotATest,
    })
}

/// Whether `id` is a row's: a tier's prefix, a hyphen and a number, as `P-01`
/// or `ED-36d`.
fn is_row(id: &str) -> bool {
    let Some((prefix, number)) = id.split_once('-') else {
        return false;
    };
    let digits = number.trim_end_matches(|c: char| c.is_ascii_lowercase());
    (1..=2).contains(&prefix.len())
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
}

/// What the roster makes of `entry` in `source`: nothing when it is landed and
/// runs there, or owed and not there yet; otherwise what is wrong.
fn judge(entry: &Guard, source: &str) -> Result<(), String> {
    match (entry.state, find(source, entry.test)) {
        (State::Landed, Some(Found::Runs)) | (State::Owed, None) => Ok(()),
        (State::Landed, found) => Err(format!(
            "{} is landed, so an un-ignored #[test] in {}, which has {found:?}",
            entry.test, entry.file
        )),
        (State::Owed, Some(found)) => Err(format!(
            "{} is owed to {}, and {} has it already ({found:?}): land its entry",
            entry.test, entry.owner, entry.file
        )),
    }
}

/// Every roster entry is landed — an un-ignored `#[test]` in the file it
/// names — or owed to a row and not yet written. A landed name gone missing or
/// ignored fails, and so does an owed one already there, whose row flips its
/// entry in the commit that adds it.
#[test]
fn every_guard_is_landed_or_owed() {
    let mut names = BTreeSet::new();
    for entry in ROSTER {
        let what = format!("{} ({}: {})", entry.test, entry.guard, entry.clause);
        assert!(names.insert(entry.test), "{what} is in the roster twice");
        assert!(
            is_row(entry.owner),
            "{what}: {:?} names no row",
            entry.owner
        );
        assert!(
            entry.lands.contains(entry.owner),
            "{what}: lands in {:?}, no step of {}",
            entry.lands,
            entry.owner
        );
        let (_, source) = SOURCES
            .iter()
            .find(|(file, _)| *file == entry.file)
            .unwrap_or_else(|| panic!("{what}: {} is not a source the suite reads", entry.file));
        if let Err(why) = judge(entry, source) {
            panic!("{why}");
        }
    }
    // Every guard the exit names.
    for guard in ["A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8", "A9", "A10"] {
        assert!(
            ROSTER.iter().any(|entry| entry.guard == guard),
            "{guard} has no entry"
        );
    }
}

/// The roster's reading of a source tells a test that runs from one that is
/// ignored, from a function that is no test, and from no function at all, and
/// it judges a landed entry and an owed one by what it read: so a reading that
/// went blind could not pass every entry.
#[test]
fn the_roster_tells_a_running_test_from_every_other_case() {
    let source = "use something;

/// Runs.
#[test]
fn runs() {}

#[test]
// It needs a corpus.
#[ignore = \"a corpus\"]
fn ignored() {}

#[test]
#[cfg_attr(target_family = \"wasm\", ignore)]
fn ignored_on_one_target() {}

fn helper() {}

#[test]
fn runs_with_a_longer_name() {}
";
    assert_eq!(find(source, "runs"), Some(Found::Runs));
    assert_eq!(find(source, "ignored"), Some(Found::Ignored));
    assert_eq!(find(source, "ignored_on_one_target"), Some(Found::Ignored));
    assert_eq!(find(source, "helper"), Some(Found::NotATest));
    assert_eq!(find(source, "runs_with_a_longer_name"), Some(Found::Runs));
    assert_eq!(find(source, "absent"), None);
    assert_eq!(
        find(source, "run"),
        None,
        "a prefix of a name is not the name"
    );

    let entry = |test: &'static str, state: State| Guard {
        test,
        state,
        ..owed("A1", "a clause", "", "P-01", "P-01.2")
    };
    assert!(judge(&entry("runs", State::Landed), source).is_ok());
    assert!(judge(&entry("absent", State::Owed), source).is_ok());
    for (test, state) in [
        ("absent", State::Landed),
        ("ignored", State::Landed),
        ("helper", State::Landed),
        ("runs", State::Owed),
        ("ignored", State::Owed),
    ] {
        assert!(
            judge(&entry(test, state), source).is_err(),
            "{test}, {state:?}: judged right"
        );
    }

    assert!(is_row("P-01") && is_row("ED-36d") && is_row("A-1"));
    assert!(!is_row("P01") && !is_row("P-") && !is_row("p-01") && !is_row("ABC-1"));
}

/// The suite holds counts, never times: no source it is built from or names
/// reads a clock. The needles are put together here, so that this file does
/// not hold them whole.
#[test]
fn the_guard_suite_reads_no_clock() {
    let clocks = [concat!("Instant", "::now"), concat!("SystemTime", "::now")];
    for (file, source) in SOURCES.iter().chain(SUPPORT) {
        for clock in clocks {
            assert!(!source.contains(clock), "{file} reads a clock: {clock}");
        }
    }
}

/// The nodes of a page tree, level by level from its root: each level the
/// kids of the one above, in order. A node that is not a dictionary, or a
/// kid that is not a reference, fails.
fn levels(document: &Document) -> Vec<Vec<tinker_pdf_cos::ObjRef>> {
    let cos = document.cos();
    let (pages, kids) = (cos.intern(b"Pages"), cos.intern(b"Kids"));
    let root = cos
        .catalog()
        .and_then(|catalog| catalog.get_ref(pages))
        .expect("a catalog naming its page tree");
    let mut levels = vec![vec![root]];
    loop {
        let mut next = Vec::new();
        for node in levels.last().expect("the root's level") {
            let object = cos.get(*node).expect("a node");
            let dict = object.as_dict().expect("a dictionary");
            if let Some(list) = dict.get(kids).and_then(Object::as_array) {
                next.extend(list.iter().map(|kid| kid.as_objref().expect("a reference")));
            }
        }
        if next.is_empty() {
            return levels;
        }
        levels.push(next);
    }
}

// ---- A1: a balanced tree of ten thousand pages -----------------------------

/// A1's tree is ten thousand pages, ten kids a node over four levels: 10^4, the
/// shape under which a lookup's bound of depth x fan-out is small enough to
/// mean something.
const A1_DEPTH: u32 = 4;
/// The kids every node of A1's tree has.
const A1_FAN_OUT: usize = 10;

/// A page tree `depth` levels of `fan_out` kids under its root, every level
/// full, whose leaves are its `fan_out^depth` pages and share one content
/// stream: a lookup reads nodes, and a page here is nothing else.
fn tree(depth: u32, fan_out: usize) -> Vec<u8> {
    // 1 the catalog, then the tree breadth first from 2, its root, and the
    // content stream last.
    let mut levels: Vec<Vec<usize>> = vec![vec![2]];
    let mut next = 3;
    for _ in 0..depth {
        let width = levels[levels.len() - 1].len() * fan_out;
        levels.push((next..next + width).collect());
        next += width;
    }
    let content = next;
    let mut objects = vec![object("<< /Type /Catalog /Pages 2 0 R >>")];
    for (level, nodes) in levels.iter().enumerate() {
        for at in 0..nodes.len() {
            let parent = match level {
                0 => String::new(),
                _ => format!(" /Parent {} 0 R", levels[level - 1][at / fan_out]),
            };
            if level as u32 == depth {
                objects.push(object(&format!(
                    "<< /Type /Page{parent} /MediaBox [0 0 200 100] /Resources << >>\n\
                     /Contents {content} 0 R >>"
                )));
            } else {
                let kids: Vec<String> = levels[level + 1][at * fan_out..(at + 1) * fan_out]
                    .iter()
                    .map(|kid| format!("{kid} 0 R"))
                    .collect();
                objects.push(object(&format!(
                    "<< /Type /Pages{parent} /Kids [{}] /Count {} >>",
                    kids.join(" "),
                    fan_out.pow(depth - level as u32)
                )));
            }
        }
    }
    objects.push(stream("", b"0 0 10 10 re f"));
    pdf(&objects)
}

/// A1's tree is what its constants state: ten thousand pages, every node
/// above them ten kids wide, every page four levels under the root.
#[test]
fn the_a1_tree_is_ten_thousand_pages_ten_kids_a_node_over_four_levels() {
    let document = Document::open(tree(A1_DEPTH, A1_FAN_OUT)).expect("the tree opens");
    assert_eq!(document.page_count(), 10_000);
    let levels = levels(&document);
    let widths: Vec<usize> = levels.iter().map(Vec::len).collect();
    assert_eq!(
        widths,
        [1, 10, 100, 1_000, 10_000],
        "a full level each time"
    );
    let cos = document.cos();
    let kind = cos.intern(b"Type");
    for (depth, level) in levels.iter().enumerate() {
        let expected: &[u8] = if depth as u32 == A1_DEPTH {
            b"Page"
        } else {
            b"Pages"
        };
        for node in level {
            let object = cos.get(*node).expect("a node");
            let name = object
                .as_dict()
                .and_then(|dict| dict.get_name(kind))
                .and_then(|name| cos.name_bytes(name))
                .expect("a /Type");
            assert_eq!(name.as_ref(), expected, "a node at depth {depth}");
        }
    }
}

/// **A1: `page(i)` reads its root and at most depth x fan-out nodes under
/// it**, through MS-01's document door: every page of A1's tree looked up by
/// index on its own, each lookup's count held to the bound, and the last page
/// of every level reading exactly it — the root and all ten kids at each of
/// four levels.
#[test]
fn a1_page_lookup_reads_at_most_depth_times_fan_out_nodes() {
    let document = Document::open(tree(A1_DEPTH, A1_FAN_OUT)).expect("the tree opens");
    let bound = 1 + u64::from(A1_DEPTH) * A1_FAN_OUT as u64;
    let mut most = 0;
    for index in 0..10_000u32 {
        let (page, stats) = document.page_counted(index);
        assert!(page.is_some(), "page {index} is in the tree");
        assert!(
            stats.page_nodes_visited <= bound,
            "page {index} read {} nodes, over {bound}",
            stats.page_nodes_visited
        );
        most = most.max(stats.page_nodes_visited);
    }
    assert_eq!(
        most, bound,
        "the last page reads the root and every kid on its path"
    );
    println!("guard-a1-lookup: RAN 10 000 lookups, at most {most} nodes each, bound {bound}");
}

/// **A1: every page of the 10 000-page tree in at most 3 x 10 000 visits**, as
/// a page loop makes them: the count, whose walk builds the page index, then
/// each page by its index, read out of it. Visits are the nodes read and the
/// entries copied, through MS-01's document doors.
#[test]
fn a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits() {
    let document = Document::open(tree(A1_DEPTH, A1_FAN_OUT)).expect("the tree opens");
    let (pages, counting) = document.page_count_counted();
    assert_eq!(pages, 10_000);
    let mut visits = counting.page_nodes_visited + counting.page_entries_copied;
    for index in 0..pages {
        let (page, stats) = document.page_counted(index);
        assert!(page.is_some(), "page {index}");
        visits += stats.page_nodes_visited + stats.page_entries_copied;
    }
    assert!(
        visits <= 30_000,
        "every page of ten thousand cost {visits} visits, over 30 000"
    );
    println!("guard-a1-total: RAN 10 000 pages in {visits} visits, the count's walk included");
}

// ---- A2 and A6: documents of pages to read, write and merge ---------------

/// A6 merges four documents of a hundred pages.
const A6_PARTS: usize = 4;
/// The pages of each document A6 merges.
const A6_PAGES: usize = 100;

/// The documents A6 merges.
fn merge_parts() -> Vec<Vec<u8>> {
    (0..A6_PARTS).map(|part| document(part, A6_PAGES)).collect()
}

/// `document`'s shape, for a document of `pages` pages that is part `part`:
/// its page count, a flat tree, a claim, and every page's line in the
/// embedded face.
fn assert_document(bytes: Vec<u8>, part: usize, pages: usize) {
    let document = Document::open(bytes).expect("the document opens");
    assert_eq!(document.page_count() as usize, pages);
    let widths: Vec<usize> = levels(&document).iter().map(Vec::len).collect();
    assert_eq!(widths, [1, pages], "one flat tree");
    let packet = document.xmp_metadata().expect("a packet");
    assert!(
        String::from_utf8_lossy(&packet).contains(r#"pdfaid:part="2""#),
        "it claims PDF/A-2"
    );
    for at in [0, pages / 2, pages - 1] {
        let page = document.page(at as u32).expect("a page");
        assert_eq!(page.text().plain_text().trim(), line(part, at), "page {at}");
    }
    assert!(
        document.fonts().iter().any(|font| font.program.is_some()),
        "the face is embedded"
    );
}

/// A2's documents are a thousand and four thousand pages, each page a line of
/// text in the embedded face.
#[test]
fn the_a2_documents_are_a_thousand_and_four_thousand_pages_of_text() {
    for pages in A2_PAGES {
        assert_document(document(0, pages), 0, pages);
    }
}

/// A6's parts are four documents of a hundred pages whose faces are the same
/// encoded stream and whose pages' contents all differ.
#[test]
fn the_a6_parts_are_four_documents_of_a_hundred_pages_sharing_one_face() {
    let parts = merge_parts();
    assert_eq!(parts.len(), A6_PARTS);
    let mut faces = BTreeSet::new();
    let mut contents = BTreeSet::new();
    for (part, bytes) in parts.into_iter().enumerate() {
        let document = Document::open(bytes.clone()).expect("a part opens");
        let cos = document.cos();
        // Object 5 is the face's program, and 8, 10, ... the pages' contents.
        faces.insert(
            cos.stream_raw(tinker_pdf_cos::ObjRef::new(5, 0))
                .expect("the program"),
        );
        for at in 0..A6_PAGES {
            let content = tinker_pdf_cos::ObjRef::new(8 + 2 * at as u32, 0);
            contents.insert(cos.stream_raw(content).expect("a content stream"));
        }
        assert_document(bytes, part, A6_PAGES);
    }
    assert_eq!(
        faces.len(),
        1,
        "one face, the same encoded bytes in each part"
    );
    assert_eq!(contents.len(), A6_PARTS * A6_PAGES, "no two pages alike");
}

// ---- A4: a page of 5 600 glyphs --------------------------------------------

/// The lines A4's page shows, each the same eighty codes.
const A4_LINES: usize = 70;
/// The first of the eighty codes, `!`.
const A4_FIRST: u8 = 0x21;
/// How many distinct codes, `!` to `p`.
const A4_DISTINCT: u8 = 80;

/// A4's page: 5 600 glyphs of 80 distinct codes, seventy lines of the same
/// eighty at 12.5 points. The face's 640-unit advance is eight points there,
/// and every line starts on a whole point, thirteen below the last, so at
/// 72 dpi every glyph's origin is on a whole pixel: the page's distinct
/// (glyph, scale, phase) keys at that scale are its eighty glyphs.
fn glyph_page() -> Vec<u8> {
    let codes: String = (A4_FIRST..A4_FIRST + A4_DISTINCT)
        .map(|code| format!("{code:02X}"))
        .collect();
    let mut content = String::from("BT /F1 12.5 Tf 13 TL 10 920 Td\n");
    for _ in 0..A4_LINES {
        content.push_str(&format!("<{codes}> Tj T*\n"));
    }
    content.push_str("ET\n");
    // 1 the catalog, 2 the page tree, 3 the page, 4 its content, 5 to 7 the
    // face.
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 700 940]\n\
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        deflated("", content.as_bytes()),
    ];
    objects.extend(face(5));
    pdf(&objects)
}

/// A4's page shows 5 600 glyphs of 80 distinct codes, every one with an
/// outline in the face.
#[test]
fn the_a4_page_shows_5600_glyphs_of_80_distinct() {
    let document = Document::open(glyph_page()).expect("the page opens");
    let page = document.page(0).expect("its page");
    let text = page.text().plain_text();
    let glyphs: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(glyphs.len(), 5_600);
    let distinct: BTreeSet<char> = glyphs.iter().copied().collect();
    assert_eq!(distinct.len(), usize::from(A4_DISTINCT));
    assert!(
        distinct.iter().all(|c| c.is_ascii_graphic()),
        "every code a printable one, so every glyph has an outline"
    );
}

/// **A4**: a render of [`glyph_page`] at 72 dpi rasterises at most one glyph
/// mask for each of its distinct (font, glyph, 2x2, phase) keys — its eighty
/// codes, every origin on a whole pixel and every glyph at one size — and
/// draws at least 98 % of its glyphs from a mask its cache held. Eighty masks
/// are far inside the cache's bound, so none is evicted.
#[test]
fn a4_glyph_masks_rasterised_are_at_most_their_distinct_keys() {
    let document = Document::open(glyph_page()).expect("the A4 page opens");
    let page = document.page(0).expect("its page");
    let stats = page
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        })
        .stats
        .expect("asked");
    let keys = u64::from(A4_DISTINCT);
    let glyphs = (A4_LINES as u64) * keys;
    assert_eq!(
        stats.glyphs_shown, glyphs,
        "every glyph is shown: {stats:?}"
    );
    assert_eq!(
        stats.glyph_masks_rasterised + stats.glyph_mask_hits,
        glyphs,
        "every glyph is drawn from a mask: {stats:?}"
    );
    assert!(
        stats.glyph_masks_rasterised <= keys,
        "{} masks rasterised over {keys} distinct keys: {stats:?}",
        stats.glyph_masks_rasterised
    );
    // At least 98 %, in integers: hits x 100 >= 98 x requests.
    assert!(
        stats.glyph_mask_hits * 100 >= 98 * glyphs,
        "{} hits of {glyphs}: {stats:?}",
        stats.glyph_mask_hits
    );
    assert_eq!(
        stats.outlines_filled, stats.glyph_masks_rasterised,
        "a hit builds no path, and only a miss is an outline filled"
    );
    println!(
        "guard-a4: RAN {} masks rasterised over {keys} keys, {} hits of {glyphs}",
        stats.glyph_masks_rasterised, stats.glyph_mask_hits
    );
}

// ---- Pinned counts ----------------------------------------------------------

/// The pinned page's content: a line in the embedded face, and one image drawn
/// once.
const PINNED_CONTENT: &str =
    "BT /F1 12 Tf 10 50 Td (Guarded!) Tj ET\nq 20 0 0 20 150 40 cm /Im1 Do Q\n";

/// The pinned image's samples: 2 x 2 RGB.
const SAMPLES: [u8; 12] = [0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0];

/// The page the pins are counted over: [`PINNED_CONTENT`], deflated, the
/// embedded face, and the image, a one-page tree.
fn pinned_page() -> Vec<u8> {
    // 1 the catalog, 2 the page tree, 3 the page, 4 its content, 5 to 7 the
    // face, 8 the image.
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 8 0 R >> >> /Contents 4 0 R >>",
        ),
        deflated("", PINNED_CONTENT.as_bytes()),
    ];
    objects.extend(face(5));
    objects.push(stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8",
        &SAMPLES,
    ));
    pdf(&objects)
}

/// The counts every guard reads do not depend on the target that read them: a
/// render, a text extraction, a save and a page lookup over [`pinned_page`],
/// held to committed values on every leg.
///
/// Pinned are the counts that are the fixture's rather than the algorithm's,
/// so a row that cuts work leaves them standing: the glyphs the page shows;
/// the streams decoded, each once, and the bytes they inflate to; and the
/// nodes of a one-page tree. A cache's hits and misses, the font dictionaries
/// read and the outlines filled are how today's code does the work, and the P
/// rows exist to move them. A pin a fix has to move is a finding the owner
/// rules on, as a moved fingerprint is.
#[test]
fn the_tally_counts_the_same_on_every_target() {
    // What the pins are made of: the content, the face and the samples.
    assert_eq!(PINNED_CONTENT.len(), 71);
    assert_eq!(render_support::curvy_font().len(), 892);
    assert_eq!(SAMPLES.len(), 12);
    let document = Document::open(pinned_page()).expect("the pinned page opens");
    let page = document.page(0).expect("its page");

    let render = page
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        })
        .stats
        .expect("asked");
    assert_eq!(
        (
            render.glyphs_shown,
            render.streams_decoded,
            render.most_decodes_of_one_stream,
            render.bytes_decoded,
        ),
        (8, 3, 1, 975),
        "a render: \"Guarded!\", and the content, the face and the samples once each"
    );

    let (_, text) = page.text_counted(&TextOptions::default());
    assert_eq!(
        (
            text.streams_decoded,
            text.most_decodes_of_one_stream,
            text.bytes_decoded,
        ),
        (1, 1, 71),
        "a text extraction: the content once"
    );

    // The image pass alone, which walks the tree to place the image, reads
    // the content to find it, and reads the samples to weigh them.
    let saved = held_save(
        &mut document.editor(),
        &SaveOptions {
            fonts: FontPolicy::Keep,
            images: ImagePolicy::Recode(
                ImageRecoding::new(ContinuousCodec::Keep, BilevelCodec::Keep).with_max_ppi(72.0),
            ),
            stats: true,
            ..SaveOptions::default()
        },
    )
    .stats
    .expect("asked");
    assert_eq!(
        (
            saved.page_walks,
            saved.page_nodes_visited,
            saved.streams_decoded,
            saved.most_decodes_of_one_stream,
            saved.bytes_decoded,
        ),
        (1, 2, 2, 1, 83),
        "a save: one walk of the root and the page; the content and the samples once each"
    );

    let (_, lookup) = document.page_counted(0);
    assert_eq!(
        (lookup.page_walks, lookup.page_nodes_visited),
        (1, 2),
        "a lookup: one walk of the root and the page"
    );
}

// ---- A2's legs ---------------------------------------------------------------

/// A2's bound: the work at 4 000 pages at most 1.1 times the ratio of the sizes
/// times the work at 1 000.
const A2_BOUND: f64 = 1.1 * 4.0;

/// One A2 leg: `leg` over A2's document at each size, its `work()` at 4 000
/// pages held to `A2_BOUND` times its work at 1 000, every counter's ratio
/// printed beside it so a failure names the counter that grew.
fn a2(name: &str, leg: impl Fn(&Document) -> Stats) {
    let measured: Vec<Stats> = A2_PAGES
        .iter()
        .map(|&pages| {
            let document = Document::open(document(0, pages)).expect("the document opens");
            leg(&document)
        })
        .collect();
    let (small, large) = (measured[0], measured[1]);
    assert!(
        small.work() > 0,
        "{name}: the leg counted nothing at {} pages",
        A2_PAGES[0]
    );
    let ratio = large.work() as f64 / small.work() as f64;
    let each = |a: u64, b: u64| if a == 0 { 0.0 } else { b as f64 / a as f64 };
    let counters =
        format!(
        "page walks {:.2}, nodes {:.2}, entries {:.2}, streams {:.2}, glyphs {:.2}, fonts {:.2}",
        each(small.page_walks, large.page_walks),
        each(small.page_nodes_visited, large.page_nodes_visited),
        each(small.page_entries_copied, large.page_entries_copied),
        each(small.streams_decoded, large.streams_decoded),
        each(small.glyphs_shown, large.glyphs_shown),
        each(small.font_dictionaries_resolved, large.font_dictionaries_resolved),
    );
    assert!(
        ratio <= A2_BOUND,
        "{name}: work {} at {} pages against {} at {}, {ratio:.2}x, over {A2_BOUND}; {counters}",
        large.work(),
        A2_PAGES[1],
        small.work(),
        A2_PAGES[0]
    );
    println!("guard-a2-{name}: RAN {ratio:.2}x within {A2_BOUND}; {counters}");
}

/// Every page, as the CLI reads a whole document: the count, then each page
/// by its index and `read` of it, every door's tally summed.
fn every_page(document: &Document, read: impl Fn(&tinker_pdf::Page) -> Stats) -> Stats {
    let (pages, mut total) = document.page_count_counted();
    for index in 0..pages {
        let (page, lookup) = document.page_counted(index);
        total += lookup;
        total += read(&page.expect("a counted page is there"));
    }
    total
}

/// **A2, text**: every page's text, looked up by its index.
#[test]
fn a2_text_is_linear_in_pages() {
    a2("text", |document| {
        every_page(document, |page| {
            page.text_counted(&TextOptions::default()).1
        })
    });
}

/// **A2, render-all at 36 dpi**: every page rendered at half of 72 dpi.
#[test]
fn a2_render_all_at_36_dpi_is_linear_in_pages() {
    a2("render", |document| {
        every_page(document, |page| {
            page.render(&RenderOptions {
                scale: 0.5,
                stats: true,
                ..RenderOptions::default()
            })
            .stats
            .expect("asked")
        })
    });
}

/// **A2, rewrite**: a rewrite that repacks the document into object streams
/// and compresses it, with the save's font pass. The serializer counts
/// nothing, so the pass's walk and decodes are what is held
/// (`docs/design/performance.md`, P-01).
#[test]
fn a2_rewrite_is_linear_in_pages() {
    a2("rewrite", |document| {
        held_save(
            &mut document.editor(),
            &SaveOptions {
                write: tinker_pdf::WriteOptions {
                    object_streams: true,
                    compress: true,
                    ..tinker_pdf::WriteOptions::default()
                },
                stats: true,
                ..SaveOptions::default()
            },
        )
        .stats
        .expect("asked")
    });
}

/// **A2, subset**: a save that cuts every embedded program to the glyphs the
/// document draws.
#[test]
fn a2_subset_is_linear_in_pages() {
    a2("subset", |document| {
        held_save(
            &mut document.editor(),
            &SaveOptions {
                stats: true,
                ..SaveOptions::default()
            },
        )
        .stats
        .expect("asked")
    });
}

/// **A2, validation**: PDF/A, every group this build has rules for, as
/// `validate_pdfa` asks, through MS-01.3's door.
#[test]
fn a2_validation_is_linear_in_pages() {
    a2("validation", |document| {
        document
            .validate_pdfa_counted(tinker_pdf::PdfACoverage::IMPLEMENTED)
            .1
    });
}

/// A2's editor legs read the editor's tally: what was counted over it since
/// `count_work`.
fn edited(editor: &tinker_pdf::DocumentEditor) -> Stats {
    editor.work().expect("the editor counts").stats()
}

/// **A2, rotate**: every page turned a quarter on a counting editor, each
/// looked up by its index.
#[test]
fn a2_rotate_is_linear_in_pages() {
    a2("rotate", |document| {
        let mut editor = document.editor();
        editor.count_work();
        for index in 0..editor.page_count() {
            assert!(editor.rotate_page(index, 90), "page {index} turns");
        }
        edited(&editor)
    });
}

/// **A2, merge**: every page of a second document of as many pages imported
/// at the end on a counting editor, as `tpdf merge` imports them.
#[test]
fn a2_merge_is_linear_in_pages() {
    a2("merge", |document| {
        let pages = document.page_count();
        let second = Document::open(guard_support::document(1, pages as usize))
            .expect("the second document opens");
        let mut editor = document.editor();
        editor.count_work();
        for index in 0..second.page_count() {
            let at = editor.page_count();
            editor
                .import_page(second.cos(), index, at)
                .expect("the page imports");
        }
        assert_eq!(editor.page_count(), 2 * pages, "every page imported");
        edited(&editor)
    });
}

/// **A2, split**: the document cut into two halves, each kept on a counting
/// editor and rewritten with what it no longer reaches collected, as `tpdf
/// split` writes its pieces.
#[test]
fn a2_split_is_linear_in_pages() {
    a2("split", |document| {
        let pages = document.page_count();
        let mut total = Stats::default();
        for half in [
            (0..pages / 2).collect::<Vec<u32>>(),
            (pages / 2..pages).collect(),
        ] {
            let mut editor = document.editor();
            editor.count_work();
            assert!(editor.keep_pages(&half), "the half is kept");
            total += held_save(
                &mut editor,
                &SaveOptions {
                    write: tinker_pdf::WriteOptions {
                        mode: tinker_pdf::WriteMode::Rewrite,
                        garbage_collect: true,
                        ..tinker_pdf::WriteOptions::default()
                    },
                    stats: true,
                    ..SaveOptions::default()
                },
            )
            .stats
            .expect("asked");
        }
        total
    });
}

/// **A2, redaction**: one rectangle across every page's line on a counting
/// editor, each page looked up by its index.
#[test]
fn a2_redaction_is_linear_in_pages() {
    a2("redaction", |document| {
        let mut editor = document.editor();
        editor.count_work();
        let areas = [tinker_pdf::redact::Redaction {
            area: tinker_pdf::Rect {
                x0: 0.0,
                y0: 40.0,
                x1: 200.0,
                y1: 70.0,
            },
            mark: false,
        }];
        for index in 0..editor.page_count() {
            let report =
                tinker_pdf::redact::apply(&mut editor, index, &areas).expect("the page is there");
            assert!(report.glyphs > 0, "page {index}: its line is cut");
        }
        edited(&editor)
    });
}

// ---- A2's book ------------------------------------------------------------------

/// A2's book, at tier P's two lengths: a hundred chapters and a thousand.
const A2_CHAPTERS: [usize; 2] = [100, 1_000];

/// Tier P's bound for ten times the chapters: 1.1 times the ratio.
const A2_BOOK_BOUND: f64 = 11.0;

/// What a book of `chapters` chapters costs as a reader meets it: opened and
/// paginated, its pages counted and each looked up by its index and rendered
/// at 72 dpi. The book's half is what its `BookCost` spent and what its
/// synthesis wrote; the reader's half is every door's tally summed. Every
/// glyph is drawn in `render_support::curvy_font`, supplied when the book
/// opens, so both builds count alike — `producer_work.rs`'s census reads the
/// book by the same counts.
fn book_work(chapters: usize) -> (u64, Stats) {
    let options = OpenOptions::default().with_fonts(Arc::new(SimpleFontProvider::new(
        render_support::curvy_font(),
    )));
    let document = Document::open_with(generated_book(chapters), &options).expect("the book opens");
    let report = document.archive().expect("a book has a report");
    let cost = report.book_cost().expect("a book has a cost");
    let spent = [
        cost.manifest_items,
        cost.spine_items,
        cost.css_tokens,
        cost.css_rules,
        cost.css_declarations,
        cost.selector_matches,
        cost.boxes,
        cost.break_work,
        cost.layout_work,
        cost.pages,
        report.pages().len(),
        report.parsed_parts(),
        report.synthesised_bytes(),
    ]
    .into_iter()
    .map(|n| u64::try_from(n).unwrap_or(u64::MAX))
    .fold(0u64, u64::saturating_add);
    let rendered = RenderOptions {
        stats: true,
        ..RenderOptions::at_dpi(72.0)
    };
    let read = every_page(&document, |page| {
        page.render(&rendered)
            .stats
            .expect("a render asked to count counts")
    });
    (spent, read)
}

/// **A2, a generated book** (P-18): a book of a thousand chapters, a page
/// each, costs at most eleven times a book of a hundred — what the book
/// spent and wrote and what reading it back took, summed.
#[test]
fn a2_a_thousand_chapter_book_costs_at_most_eleven_times_a_hundred() {
    let [(small_spent, small_read), (large_spent, large_read)] = A2_CHAPTERS.map(book_work);
    let small = small_spent.saturating_add(small_read.work());
    let large = large_spent.saturating_add(large_read.work());
    assert!(
        small_read.work() > 0,
        "the hundred-chapter book read nothing back"
    );
    let ratio = large as f64 / small as f64;
    let each = |a: u64, b: u64| if a == 0 { 0.0 } else { b as f64 / a as f64 };
    let counters = format!(
        "the book's own {:.2}, page walks {:.2}, nodes {:.2}, glyphs {:.2}, fonts {:.2}, \
         operators {:.2}",
        each(small_spent, large_spent),
        each(small_read.page_walks, large_read.page_walks),
        each(small_read.page_nodes_visited, large_read.page_nodes_visited),
        each(small_read.glyphs_shown, large_read.glyphs_shown),
        each(
            small_read.font_dictionaries_resolved,
            large_read.font_dictionaries_resolved
        ),
        each(small_read.operators_run, large_read.operators_run),
    );
    assert!(
        ratio <= A2_BOOK_BOUND,
        "book: work {large} at {} chapters against {small} at {}, {ratio:.2}x, over \
         {A2_BOOK_BOUND}; {counters}",
        A2_CHAPTERS[1],
        A2_CHAPTERS[0]
    );
    println!("guard-a2-book: RAN {ratio:.2}x within {A2_BOOK_BOUND}; {counters}");
}

// ---- A3: the fonts a page reads -------------------------------------------------

/// A3's page: the embedded face under two names, a third font the page names
/// and never selects, and a form drawn twice whose own `/Resources` name the
/// face again beside a font the form never selects. 1 the catalog, 2 the page
/// tree, 3 the page, 4 its content, 5 to 7 the face, 8 the unused font, 9 the
/// form.
fn a3_page() -> Vec<u8> {
    let content = "BT /F1 12 Tf 10 70 Td (Shared) Tj /F2 12 Tf ( twice) Tj ET\n\
                   q 1 0 0 1 10 10 cm /Fm1 Do Q\nq 1 0 0 1 10 40 cm /Fm1 Do Q\n";
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 5 0 R /F2 5 0 R /F3 8 0 R >> /XObject << /Fm1 9 0 R >> >>\n\
             /Contents 4 0 R >>",
        ),
        deflated("", content.as_bytes()),
    ];
    objects.extend(face(5));
    objects.push(object(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ));
    objects.push(deflated(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 20]\n\
         /Resources << /Font << /F1 5 0 R /F4 8 0 R >> >>",
        b"BT /F1 10 Tf 2 5 Td (form) Tj ET",
    ));
    pdf(&objects)
}

/// The distinct `/Font` entries [`a3_page`]'s content selects a font by, each
/// scope's apart: the page's `F1` and `F2`, and the form's `F1`.
const A3_ENTRIES_USED: u64 = 3;

/// **A3**: the font dictionaries a page's render and its text extraction read
/// are at most the distinct `/Font` entries its content selects a font by,
/// whatever the glyphs: a font named twice is read under each name, a font
/// the page names and never selects is not read, and a form's own scope reads
/// its own once however often the form is drawn.
#[test]
fn a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses() {
    let document = Document::open(a3_page()).expect("the A3 page opens");
    let page = document.page(0).expect("its page");
    let render = page
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        })
        .stats
        .expect("asked");
    let (text, extraction) = page.text_counted(&TextOptions::default());
    let read = text.plain_text();
    for shown in ["Shared", "twice", "form"] {
        assert!(read.contains(shown), "{shown:?} is read: {read:?}");
    }
    assert!(render.glyphs_shown > 0, "the render draws its glyphs");
    for (door, stats) in [("render", render), ("text", extraction)] {
        assert!(
            stats.font_dictionaries_resolved <= A3_ENTRIES_USED,
            "{door}: {} font dictionaries resolved over {A3_ENTRIES_USED} entries used: {stats:?}",
            stats.font_dictionaries_resolved
        );
        println!(
            "guard-a3-{door}: RAN {} font dictionaries resolved, at most {A3_ENTRIES_USED}",
            stats.font_dictionaries_resolved
        );
    }
}

// ---- A5: the documents a save, a render and two extractions read ------------

/// The pages A5's save reads.
const A5_PAGES: usize = 100;

/// The embedded face's `/ToUnicode`: every code `/WinAnsiEncoding` reads it
/// to draw, `<20>` to `<7E>`, mapped to the character of its own number, so
/// the face's text reads as it did without one.
const TO_UNICODE: &str = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
    /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
    /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
    1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
    1 beginbfrange\n<20> <7E> <0020>\nendbfrange\n\
    endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";

/// The embedded face with its `/ToUnicode`, as four objects numbered from
/// `at`: [`face`]'s font, descriptor and program, the font naming the CMap
/// at `at + 3`, deflated.
fn face_with_to_unicode(at: usize) -> [Vec<u8>; 4] {
    let [mut font, descriptor, program] = face(at);
    assert!(font.ends_with(b">>"), "the face's font is a dictionary");
    font.truncate(font.len() - 2);
    font.extend_from_slice(format!("/ToUnicode {} 0 R >>", at + 3).as_bytes());
    [
        font,
        descriptor,
        program,
        deflated("", TO_UNICODE.as_bytes()),
    ]
}

/// A5's save: [`A5_PAGES`] pages, each a line of text in the one embedded
/// face from a content stream of its own, every page's resources naming the
/// face, whose `/ToUnicode` each page's scope reads with it. 1 the catalog, 2
/// the page tree, 3 to 6 the face and its CMap, then each page and its
/// content from 7; every stream deflated.
fn a5_document() -> Vec<u8> {
    let kids: Vec<String> = (0..A5_PAGES)
        .map(|at| format!("{} 0 R", 7 + 2 * at))
        .collect();
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object(&format!(
            "<< /Type /Pages /Count {A5_PAGES} /Kids [{}] >>",
            kids.join(" ")
        )),
    ];
    objects.extend(face_with_to_unicode(3));
    for at in 0..A5_PAGES {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            8 + 2 * at
        )));
        objects.push(deflated("", content(0, at).as_bytes()));
    }
    pdf(&objects)
}

/// Whether object `number` of `cos` is a stream whose stored bytes are not
/// its decoded bytes: one whose decode is an inflate.
fn inflated(cos: &tinker_pdf_cos::CosDocument, number: u32) -> bool {
    let r = tinker_pdf_cos::ObjRef::new(number, 0);
    let stored = cos.stream_raw(r).expect("a stream");
    stored != cos.stream_decoded(r).expect("it decodes")
}

/// A5's save reads a hundred pages, each its own line in the one embedded
/// face from a content stream of its own, every page's resources naming that
/// face, which embeds its program and a `/ToUnicode`; each is deflated.
#[test]
fn the_a5_document_is_a_hundred_pages_drawing_one_face_with_a_to_unicode() {
    let document = Document::open(a5_document()).expect("the A5 document opens");
    assert_eq!(document.page_count() as usize, A5_PAGES);
    let cos = document.cos();
    let face = tinker_pdf_cos::ObjRef::new(3, 0);
    let font = tinker_pdf_cos::font::at(cos, face).expect("the face reads");
    assert!(font.has_to_unicode(), "the face has its /ToUnicode");
    assert!(font.program().is_some(), "and embeds its program");
    let (fonts, f1) = (cos.intern(b"Font"), cos.intern(b"F1"));
    let pages = tinker_pdf_cos::pages::collect(cos);
    assert_eq!(pages.len(), A5_PAGES);
    for (at, page) in pages.iter().enumerate() {
        let resources = page.resources.as_ref().expect("its resources");
        let named = cos
            .resolve_key(resources, fonts)
            .as_dict()
            .and_then(|table| table.get_ref(f1));
        assert_eq!(named, Some(face), "page {at} draws the face");
    }
    for at in [0, A5_PAGES / 2, A5_PAGES - 1] {
        let page = document.page(at as u32).expect("a page");
        assert_eq!(page.text().plain_text().trim(), line(0, at), "page {at}");
    }
    for number in [5, 6, 8, 8 + 2 * (A5_PAGES as u32 - 1)] {
        assert!(inflated(cos, number), "object {number} is deflated");
    }
}

/// How often A5's page draws its form.
const A5_FORM_DRAWS: usize = 3;
/// How often A5's page shows its Type 3 glyph.
const A5_TYPE3_SHOWN: usize = 50;
/// The line A5's page shows in the face.
const A5_PAGE_LINE: &str = "Page";
/// The line A5's form shows in the face at each draw.
const A5_FORM_LINE: &str = "Form";
/// The Type 3 glyph's procedure: a square, three quarters of its em.
const A5_PROCEDURE: &[u8] = b"1000 0 0 0 750 750 d1\n0 0 750 750 re f";
/// The bytes A5's images' profile holds, which no parser accepts: the
/// profile is read, and refused, and its images drawn by their components.
const A5_PROFILE: &[u8] = b"not an ICC profile";

/// A5's page's content: its line in the face, its form three times, its
/// Type 3 glyph fifty times in one string, and its image once.
fn a5_page_content() -> String {
    let mut out = format!("BT /F1 12 Tf 10 85 Td ({A5_PAGE_LINE}) Tj ET\n");
    for at in 0..A5_FORM_DRAWS {
        out.push_str(&format!("q 1 0 0 1 10 {} cm /Fm1 Do Q\n", 10 + 20 * at));
    }
    out.push_str(&format!(
        "BT /T3 2 Tf 50 72 Td ({}) Tj ET\n",
        "A".repeat(A5_TYPE3_SHOWN)
    ));
    out.push_str("q 20 0 0 20 150 10 cm /Im1 Do Q\n");
    out
}

/// A5's form's content: its line in the face, and its image.
fn a5_form_content() -> String {
    format!("BT /F1 10 Tf 2 5 Td ({A5_FORM_LINE}) Tj ET\nq 10 0 0 10 60 5 cm /Im2 Do Q")
}

/// A 2 x 2 image in A5's profile, its samples deflated.
fn a5_image() -> Vec<u8> {
    deflated(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace [/ICCBased 14 0 R]\n\
         /BitsPerComponent 8",
        &SAMPLES,
    )
}

/// A5's page: one form drawn three times, whose own resources name the
/// page's embedded face and an image of their own, and one Type 3 glyph shown
/// fifty times, beside a line in the face and an image whose profile the
/// form's image shares. 1 the catalog, 2 the page tree, 3 the page, 4 its
/// content, 5 to 8 the face and its `/ToUnicode`, 9 the Type 3 font, 10 its
/// procedure, 11 the form, 12 the page's image, 13 the form's, 14 their
/// profile; every stream deflated.
fn a5_page() -> Vec<u8> {
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 5 0 R /T3 9 0 R >> /XObject << /Fm1 11 0 R /Im1 12 0 R >> >>\n\
             /Contents 4 0 R >>",
        ),
        deflated("", a5_page_content().as_bytes()),
    ];
    objects.extend(face_with_to_unicode(5));
    objects.push(object(
        "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 750 750] /FontMatrix [0.001 0 0 0.001 0 0]\n\
         /CharProcs << /square 10 0 R >> /Encoding << /Type /Encoding /Differences [65 /square] >>\n\
         /FirstChar 65 /LastChar 65 /Widths [1000] >>",
    ));
    objects.push(deflated("", A5_PROCEDURE));
    objects.push(deflated(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 20]\n\
         /Resources << /Font << /F1 5 0 R >> /XObject << /Im2 13 0 R >> >>",
        a5_form_content().as_bytes(),
    ));
    objects.push(a5_image());
    objects.push(a5_image());
    objects.push(deflated("/N 3", A5_PROFILE));
    pdf(&objects)
}

/// A5's page draws its form three times and shows its Type 3 glyph fifty
/// times, a procedure of its own; the page's scope and the form's both name
/// the face, which has a `/ToUnicode`, and the form's image is drawn at each
/// of the form's draws, in the profile the page's image is drawn in. Every
/// stream is deflated.
#[test]
fn the_a5_page_shows_one_form_three_times_and_one_type3_glyph_fifty_times() {
    let content = a5_page_content();
    assert_eq!(content.matches("/Fm1 Do").count(), A5_FORM_DRAWS);
    let shown = format!("({}) Tj", "A".repeat(A5_TYPE3_SHOWN));
    assert_eq!(content.matches(&shown).count(), 1, "fifty in one string");

    let document = Document::open(a5_page()).expect("the A5 page opens");
    let cos = document.cos();
    let type3 = tinker_pdf_cos::font::at(cos, tinker_pdf_cos::ObjRef::new(9, 0))
        .expect("the Type 3 font reads");
    assert_eq!(type3.kind(), tinker_pdf_cos::FontKind::Type3);
    assert_eq!(type3.glyph_name(65), Some("square"), "the code's procedure");
    let face =
        tinker_pdf_cos::font::at(cos, tinker_pdf_cos::ObjRef::new(5, 0)).expect("the face reads");
    assert!(face.has_to_unicode(), "the face has its /ToUnicode");

    let page = document.page(0).expect("its page");
    let text = page.text().plain_text();
    assert_eq!(text.matches(A5_PAGE_LINE).count(), 1, "{text:?}");
    assert_eq!(
        text.matches(A5_FORM_LINE).count(),
        A5_FORM_DRAWS,
        "the form's line at each draw: {text:?}"
    );
    let images = page.images();
    let drawn = |name: &[u8]| {
        let image = images
            .iter()
            .find(|image| image.name == name)
            .unwrap_or_else(|| panic!("{} is listed", String::from_utf8_lossy(name)));
        let Some(tinker_pdf::ImageSpace::Icc { profile, .. }) = &image.color_space else {
            panic!("an ICC space: {:?}", image.color_space);
        };
        assert_eq!(profile.as_slice(), A5_PROFILE, "the shared profile");
        image.placements.len()
    };
    assert_eq!(drawn(b"Im1"), 1, "the page's image, once");
    assert_eq!(drawn(b"Im2"), A5_FORM_DRAWS, "the form's, at each draw");
    assert_eq!(images.len(), 2);
    for number in [4, 7, 8, 10, 11, 12, 13, 14] {
        assert!(inflated(cos, number), "object {number} is deflated");
    }
}

/// The decoded lengths of A5's page's streams, by the legs that read them.
struct A5Bytes {
    /// What every leg reads: the content, the face's `/ToUnicode`, the Type 3
    /// procedure and the form.
    read: usize,
    /// The face's program, which only a render's outlines read.
    program: usize,
    /// Both images' samples and their profile, which a render and an image
    /// extraction read.
    images: usize,
}

/// [`A5Bytes`] for the page.
fn a5_bytes() -> A5Bytes {
    A5Bytes {
        read: a5_page_content().len()
            + TO_UNICODE.len()
            + A5_PROCEDURE.len()
            + a5_form_content().len(),
        program: render_support::curvy_font().len(),
        images: 2 * SAMPLES.len() + A5_PROFILE.len(),
    }
}

/// The glyphs A5's page shows the renderer: its line and the form's at each
/// draw, in the face. A Type 3 glyph whose procedure runs is not shown, so
/// this is also what says the procedure ran at every one of its fifty.
fn a5_glyphs_shown() -> u64 {
    (A5_PAGE_LINE.len() + A5_FORM_DRAWS * A5_FORM_LINE.len()) as u64
}

/// **A5, a save**: the default save of A5's hundred pages — the font pass's
/// walk of every page, and its cut of the face's program — decodes no stream
/// twice. Every page's scope reads the face, its program and its
/// `/ToUnicode`; the first page's decodes both, every later page's is handed
/// those decodes, and the cut reads the program from them: each page's
/// content, the program and the CMap are decoded once each.
#[test]
fn a5_a_save_decodes_each_stream_at_most_once() {
    let document = Document::open(a5_document()).expect("the A5 document opens");
    let saved = held_save(
        &mut document.editor(),
        &SaveOptions {
            stats: true,
            ..SaveOptions::default()
        },
    );
    let report = saved.fonts.report().expect("the font pass ran");
    assert_eq!(
        report.subsetted.len() + report.untouched.len(),
        1,
        "the face's one program went through the cut: {report:?}"
    );
    let stats = saved.stats.expect("asked");
    let contents: usize = (0..A5_PAGES).map(|page| content(0, page).len()).sum();
    assert_eq!(
        (
            stats.streams_decoded,
            stats.most_decodes_of_one_stream,
            stats.bytes_decoded,
        ),
        (
            A5_PAGES as u64 + 2,
            1,
            (contents + render_support::curvy_font().len() + TO_UNICODE.len()) as u64
        ),
        "each page's content, and the face's program and /ToUnicode, once each"
    );
    println!(
        "guard-a5-save: RAN {} decodes over {A5_PAGES} pages, at most {} of one stream",
        stats.streams_decoded, stats.most_decodes_of_one_stream
    );
}

/// **A5, a render**: a render of A5's page decodes no stream twice — the
/// form at the first of its three draws, the Type 3 procedure at the first
/// of its fifty glyphs, the face's program and `/ToUnicode` in whichever of
/// the page's scope and the form's reads them first, and the profile in
/// whichever of the two images' scopes compiles it first — and the eight
/// streams it reads, each image's samples among them, once each.
#[test]
fn a5_a_render_decodes_each_stream_at_most_once() {
    let document = Document::open(a5_page()).expect("the A5 page opens");
    let page = document.page(0).expect("its page");
    let stats = page
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        })
        .stats
        .expect("asked");
    let bytes = a5_bytes();
    assert_eq!(stats.glyphs_shown, a5_glyphs_shown(), "{stats:?}");
    assert_eq!(
        (
            stats.streams_decoded,
            stats.most_decodes_of_one_stream,
            stats.bytes_decoded,
        ),
        (8, 1, (bytes.read + bytes.program + bytes.images) as u64),
        "every stream the page reads, once each: {stats:?}"
    );
    println!(
        "guard-a5-render: RAN {} decodes, at most {} of one stream",
        stats.streams_decoded, stats.most_decodes_of_one_stream
    );
}

/// **A5, a text extraction**: A5's page's text decodes no stream twice —
/// the content, the form, the Type 3 procedure, which the interpreter runs at
/// every glyph as a render does, and the face's `/ToUnicode`, which the
/// page's scope and the form's both read — once each, and no program, since
/// no outline is read.
#[test]
fn a5_text_decodes_each_stream_at_most_once() {
    let document = Document::open(a5_page()).expect("the A5 page opens");
    let page = document.page(0).expect("its page");
    let (text, stats) = page.text_counted(&TextOptions::default());
    assert_eq!(
        text.plain_text().matches(A5_FORM_LINE).count(),
        A5_FORM_DRAWS
    );
    assert_eq!(
        (
            stats.streams_decoded,
            stats.most_decodes_of_one_stream,
            stats.bytes_decoded,
        ),
        (4, 1, a5_bytes().read as u64),
        "the content, the form, the procedure and the CMap, once each: {stats:?}"
    );
    println!(
        "guard-a5-text: RAN {} decodes, at most {} of one stream",
        stats.streams_decoded, stats.most_decodes_of_one_stream
    );
}

/// **A5, image extraction, the decode door**: listing A5's page's images
/// decodes no stream twice — what the text extraction decodes, as it
/// interprets the page the same way, and each image's samples and their
/// shared profile, which both images' descriptions read, once each.
#[test]
fn a5_image_extraction_decodes_each_stream_at_most_once() {
    let document = Document::open(a5_page()).expect("the A5 page opens");
    let page = document.page(0).expect("its page");
    let (images, stats) = page.images_counted();
    assert_eq!(images.len(), 2, "the page's image and the form's");
    let bytes = a5_bytes();
    assert_eq!(
        (
            stats.streams_decoded,
            stats.most_decodes_of_one_stream,
            stats.bytes_decoded,
        ),
        (7, 1, (bytes.read + bytes.images) as u64),
        "every stream the listing reads, once each: {stats:?}"
    );
    println!(
        "guard-a5-images: RAN {} decodes, at most {} of one stream",
        stats.streams_decoded, stats.most_decodes_of_one_stream
    );
}

// ---- A5 and A6: a merge's save ------------------------------------------------

/// The four A6 parts merged as `tpdf merge` merges them — the first the base
/// and every page of the rest imported — and saved with stream deduplication
/// and fonts kept, counted: with no font pass to run, deduplication is all the
/// save decodes.
fn merged() -> tinker_pdf::write::Saved {
    let parts: Vec<Document> = merge_parts()
        .into_iter()
        .map(|bytes| Document::open(bytes).expect("a part opens"))
        .collect();
    let mut editor = parts[0].editor();
    for part in &parts[1..] {
        for index in 0..part.page_count() {
            let at = editor.page_count();
            editor
                .import_page(part.cos(), index, at)
                .expect("the page imports");
        }
    }
    held_save(
        &mut editor,
        &SaveOptions {
            fonts: FontPolicy::Keep,
            write: tinker_pdf::WriteOptions {
                mode: tinker_pdf::WriteMode::Rewrite,
                deduplicate_streams: true,
                ..tinker_pdf::WriteOptions::default()
            },
            stats: true,
            ..SaveOptions::default()
        },
    )
}

// ---- A5: each stream decoded at most once an operation -----------------------

/// **A5, a merge**: the four A6 parts merged as `tpdf merge` merges them — the
/// first the base and every page of the rest imported — and saved with stream
/// deduplication and fonts kept. No stream is decoded twice in the save. The
/// face, copied with every imported page, is one encoding and its copies merge
/// undecoded; each page's content and the face's program share one dictionary,
/// `/Filter /FlateDecode` alone, and differ in their encodings, so each is
/// compared decoded, once.
#[test]
fn a5_a_merge_decodes_each_stream_at_most_once() {
    let stats = merged().stats.expect("asked");
    assert_eq!(
        stats.streams_decoded,
        (A6_PARTS * A6_PAGES) as u64 + 1,
        "every page's content and the face's program compared decoded, once each"
    );
    assert_eq!(
        stats.most_decodes_of_one_stream, 1,
        "no stream decoded twice in the save"
    );
    println!(
        "guard-a5-merge: RAN {} decodes, at most {} of one stream",
        stats.streams_decoded, stats.most_decodes_of_one_stream
    );
}

// ---- A6: what a merge's deduplication decodes --------------------------------

/// **A6: a merge's deduplication decodes no byte-identical copy, and each
/// differing encoding under one dictionary once a save** — ruling 23's answer
/// to H-137: byte-identical encodings merge without decoding, and differing
/// ones keep the decoded comparison, so that two encodings of one content
/// still merge. Over the four A6 parts merged as `tpdf merge` saves them, the
/// face, copied with each of the 300 imported pages, is one encoding: its
/// copies merge, and the bytes decoded hold the face once. Every page's
/// content and the face's program share one dictionary, `/Filter
/// /FlateDecode` alone, in 401 encodings that all differ: each is decoded
/// once, and none merges with another.
#[test]
fn a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once() {
    let saved = merged();
    let stats = saved.stats.expect("asked");
    let encodings = (A6_PARTS * A6_PAGES) as u64 + 1;
    let contents: usize = (0..A6_PARTS)
        .flat_map(|part| (0..A6_PAGES).map(move |page| content(part, page).len()))
        .sum();
    let face = render_support::curvy_font().len();
    assert_eq!(
        (
            stats.streams_decoded,
            stats.most_decodes_of_one_stream,
            stats.bytes_decoded,
        ),
        (encodings, 1, (contents + face) as u64),
        "each differing encoding decoded once, the face's among them, and none of its copies"
    );

    // What the pass made of them: one program for every copy of the face, and
    // each page's content still an object of its own.
    let document = Document::open(saved.bytes).expect("the merge reopens");
    let programs: BTreeSet<tinker_pdf_cos::ObjRef> = document
        .fonts()
        .iter()
        .filter_map(|font| font.program.map(|program| program.stream))
        .collect();
    assert_eq!(programs.len(), 1, "the face's copies are one program");
    let cos = document.cos();
    let key = cos.intern(b"Contents");
    let pages: BTreeSet<tinker_pdf_cos::ObjRef> = tinker_pdf_cos::pages::collect(cos)
        .iter()
        .map(|page| {
            let object = cos.get(page.reference).expect("a page");
            object
                .as_dict()
                .and_then(|dict| dict.get_ref(key))
                .expect("its content")
        })
        .collect();
    assert_eq!(
        pages.len(),
        A6_PARTS * A6_PAGES,
        "no two pages' contents merged"
    );
    println!(
        "guard-a6-merge: RAN {} decodes of {encodings} differing encodings, {} bytes, \
         one program for the face",
        stats.streams_decoded, stats.bytes_decoded
    );
}

// ---- A8: the PNG path's scratch ------------------------------------------------

/// The text page's lines: A4 at 9 pt on 11, from the top margin to the
/// bottom.
const A8_LINES: usize = 70;

/// Line `line` of A8's pages: words of three to nine letters from a seeded
/// sequence, so that no two lines are alike.
fn a8_line(line: usize) -> String {
    let mut state = 0x2545_F491_4F6C_DD1Du64 ^ (line as u64 + 1);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut text = String::new();
    while text.len() < 80 {
        for _ in 0..3 + next() % 7 {
            text.push(char::from(b'a' + (next() % 26) as u8));
        }
        text.push(' ');
    }
    text
}

/// An A4 page of `lines` lines in the embedded face, the shape of the encode
/// group's two PNG inputs: the text page, and the near-blank page of one
/// line. 1 the catalog, 2 the page tree, 3 to 5 the face, 6 the page, 7 its
/// content.
fn a8_page(lines: usize) -> Vec<u8> {
    let mut content = String::from("BT /F1 9 Tf 11 TL 36 806 Td\n");
    for line in 0..lines {
        content.push_str(&format!("({}) Tj T*\n", a8_line(line)));
    }
    content.push_str("ET");
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [6 0 R] >>"),
    ];
    objects.extend(face(3));
    objects.push(object(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842]\n\
         /Resources << /Font << /F1 3 0 R >> >> /Contents 7 0 R >>",
    ));
    objects.push(deflated("", content.as_bytes()));
    pdf(&objects)
}

/// A8's pages read back as the lines they were written with: seventy on the
/// text page, none alike, and one on the near-blank page.
#[test]
fn a8s_pages_hold_their_lines() {
    for lines in [A8_LINES, 1] {
        let document = Document::open(a8_page(lines)).expect("the A8 page opens");
        let read = document.page(0).expect("its page").text().plain_text();
        let read: Vec<&str> = read
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let written: Vec<String> = (0..lines).map(a8_line).collect();
        assert_eq!(read.len(), lines, "{read:?}");
        for (read, written) in read.iter().zip(&written) {
            assert_eq!(read.trim(), written.trim());
        }
        let distinct: BTreeSet<&String> = written.iter().collect();
        assert_eq!(distinct.len(), lines, "no two lines alike");
    }
}

/// **A8**: writing a page to PNG holds no buffer sized from the image — the
/// rows stream through the deflater into the file — and holds at most 1.2
/// times the image's bytes with the file, on a page of text and a near-blank
/// page at 150 dpi, each read back as the bitmap it was.
#[test]
fn a8_the_png_path_holds_at_most_one_image_of_scratch() {
    for (name, lines) in [("text", A8_LINES), ("near-blank", 1)] {
        let document = Document::open(a8_page(lines)).expect("the A8 page opens");
        let page = document.page(0).expect("its page");
        let bitmap = page.render(&RenderOptions::at_dpi(150.0));
        assert_eq!((bitmap.width, bitmap.height), (1240, 1755), "{name}: A4");
        assert!(
            bitmap.data.iter().any(|&byte| byte < 128),
            "{name}: the page draws its text"
        );
        let image = bitmap.data.len() as u64;
        let (png, stats) = bitmap.to_png_counted();
        let png = png.expect("the bitmap is a picture");
        assert_eq!(stats.png_image_scratch, 0, "{name}: no whole-image scratch");
        assert!(
            10 * stats.png_peak <= 12 * image,
            "{name}: {} bytes held for an image of {image}",
            stats.png_peak
        );
        assert!(
            stats.png_peak >= png.len() as u64,
            "{name}: the file is held"
        );
        assert_eq!(stats.work(), 0, "a write takes no counted step");
        let back = tinker_pdf::Bitmap::from_png(&png).expect("the file reads back");
        assert_eq!(back.data, bitmap.data, "{name}: the file is the bitmap");
        println!(
            "guard-a8-{name}: RAN {} bytes of image scratch, {} held with a file of {}, \
             for an image of {image}",
            stats.png_image_scratch,
            stats.png_peak,
            png.len()
        );
    }
}

// ---- A9: a page walk is the serial walk ----------------------------------------

/// A9's text document: five pages of 200 by 100 points. Page 0 is a line in
/// the embedded face; page 1 three lines at two sizes over a fill; page 2 a
/// line whose second half is shown in `/F9`, a font its resources do not
/// hold, so that its render and its text both warn; page 3 is empty; page 4
/// draws text clipped to a rectangle and under a stroke. 1 the catalog, 2 the
/// page tree, 3 to 5 the face, then each page and its content from 6.
fn a9_text_document() -> Vec<u8> {
    let contents = [
        "BT /F1 12 Tf 10 50 Td (Walked, page one) Tj ET",
        concat!(
            "0.85 0.9 1 rg 5 5 190 90 re f 0 g BT /F1 9 Tf 10 80 Td (A line) Tj ",
            "0 -11 Td (and another) Tj /F1 16 Tf 0 -24 Td (larger) Tj ET"
        ),
        "BT /F1 12 Tf 10 50 Td (Found) Tj /F9 12 Tf ( lost) Tj ET",
        "",
        concat!(
            "q 20 20 120 40 re W n BT /F1 30 Tf 10 30 Td (Clipped text) Tj ET Q ",
            "0.7 0.1 0.2 RG 3 w 15 15 m 185 85 l S"
        ),
    ];
    let kids: Vec<String> = (0..contents.len())
        .map(|at| format!("{} 0 R", 6 + 2 * at))
        .collect();
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object(&format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            contents.len(),
            kids.join(" ")
        )),
    ];
    objects.extend(face(3));
    for (at, content) in contents.iter().enumerate() {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            7 + 2 * at
        )));
        objects.push(deflated("", content.as_bytes()));
    }
    pdf(&objects)
}

/// Bytes from a linear congruential generator, the same on every target.
fn a9_noise(count: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(17);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

/// `bytes` as `ASCIIHexDecode` text, end-of-data marker included.
fn a9_hex(bytes: &[u8]) -> String {
    let mut out: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    out.push('>');
    out
}

/// A9's image document: five pages of 100 by 100 points. Page 0 draws a
/// Flate RGB image magnified at an offset; page 1 the same image twice, a
/// one-bit image and an image mask in a fill colour; page 2 an image under an
/// 8-bit `/SMask` of another size, over a fill; page 3 an RGB inline image
/// and an inline mask; page 4 an image no decoder here reads, which draws the
/// placeholder and is named, and a Flate image cut short, which draws what
/// decoded and is named as damaged. 1 the catalog, 2 the page tree, 3 to 7
/// the pages, 8 to 12 their contents, then the images from 13.
fn a9_image_document() -> Vec<u8> {
    let rgb = a9_noise(16 * 12 * 3, 9);
    let grey = a9_noise(8 * 6, 10);
    let bilevel = render_support::bilevel_samples();
    let (bw, bh) = (
        render_support::BILEVEL_WIDTH,
        render_support::BILEVEL_HEIGHT,
    );
    let mut cut = tinker_pdf_filters::zlib_compress(&a9_noise(16 * 12 * 3, 11));
    cut.truncate(cut.len() * 2 / 3);
    let rgb_dict = "/Type /XObject /Subtype /Image /Width 16 /Height 12 \
                    /ColorSpace /DeviceRGB /BitsPerComponent 8";
    let inline_rgb = a9_hex(&a9_noise(4 * 3 * 3, 12));
    let inline_mask = a9_hex(&[0b1010_0110, 0b0101_1001]);
    let contents = [
        "q 70.3 0 0 52.7 10.4 12.6 cm /Im0 Do Q".to_string(),
        concat!(
            "q 31 0 0 23 5 5 cm /Im0 Do Q q 8 0 0 6 50 40 cm /Im0 Do Q ",
            "q 42 0 0 30 5 60 cm /Im1 Do Q 0.8 0.1 0.25 rg q 14 0 0 10 60 70 cm /Im2 Do Q"
        )
        .to_string(),
        "0.9 0.9 0.2 rg 0 0 100 100 re f q 60 0 0 45 15 20 cm /Im3 Do Q".to_string(),
        format!(
            "q 40 0 0 30 10 10 cm BI /W 4 /H 3 /CS /RGB /BPC 8 /F /AHx ID {inline_rgb} EI Q \
             0.1 0.6 0.3 rg q 30 0 0 20 55 60 cm BI /W 8 /H 2 /IM true /F /AHx \
             ID {inline_mask} EI Q"
        ),
        "q 30 0 0 30 10 10 cm /Im4 Do Q q 40 0 0 30 50 50 cm /Im5 Do Q".to_string(),
    ];
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 5 /Kids [3 0 R 4 0 R 5 0 R 6 0 R 7 0 R] >>"),
    ];
    for at in 0..contents.len() {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
             /Resources << /XObject << /Im0 13 0 R /Im1 14 0 R /Im2 15 0 R /Im3 16 0 R \
             /Im4 18 0 R /Im5 19 0 R >> >> /Contents {} 0 R >>",
            8 + at
        )));
    }
    for content in &contents {
        objects.push(deflated("", content.as_bytes()));
    }
    // 13 to 19.
    objects.push(deflated(rgb_dict, &rgb));
    objects.push(stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {bw} /Height {bh} \
             /ColorSpace /DeviceGray /BitsPerComponent 1"
        ),
        &bilevel,
    ));
    objects.push(stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {bw} /Height {bh} \
             /ImageMask true /BitsPerComponent 1"
        ),
        &bilevel,
    ));
    objects.push(deflated(&format!("{rgb_dict} /SMask 17 0 R"), &rgb));
    objects.push(deflated(
        "/Type /XObject /Subtype /Image /Width 8 /Height 6 \
         /ColorSpace /DeviceGray /BitsPerComponent 8",
        &grey,
    ));
    objects.push(stream(
        "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceGray \
         /BitsPerComponent 1 /Filter /JBIG2Decode",
        b"no JBIG2 segment here",
    ));
    objects.push(stream(&format!("{rgb_dict} /Filter /FlateDecode"), &cut));
    pdf(&objects)
}

/// A9's annotated document: three pages of 120 by 90 points, each with a
/// line in the embedded face. Page 0 carries a square whose normal
/// appearance strokes a box and a hidden one (`/F 2`) that draws nothing;
/// page 1 a check box whose `/AS` picks its `/On` appearance of two; page 2 a
/// square with no appearance, which the engine draws from its `/C`, `/IC` and
/// `/BS`. 1 the catalog, 2 the page tree, 3 to 5 the pages, 6 to 8 their
/// contents, 9 to 11 the face, then the annotations and their appearances
/// from 12.
fn a9_annotated_document() -> Vec<u8> {
    let appearance = |content: &str| {
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 30]",
            content.as_bytes(),
        )
    };
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [14 0 R] >> >>"),
        object("<< /Type /Pages /Count 3 /Kids [3 0 R 4 0 R 5 0 R] >>"),
    ];
    for (at, annots) in ["[12 0 R 17 0 R]", "[14 0 R]", "[18 0 R]"]
        .iter()
        .enumerate()
    {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 90] \
             /Resources << /Font << /F1 9 0 R >> >> /Contents {} 0 R /Annots {annots} >>",
            6 + at
        )));
    }
    for at in 0..3 {
        objects.push(deflated(
            "",
            format!("BT /F1 10 Tf 8 70 Td (Annotated page {}) Tj ET", at + 1).as_bytes(),
        ));
    }
    objects.extend(face(9));
    // 12: a square with an appearance, and 13 the appearance.
    objects.push(object(
        "<< /Type /Annot /Subtype /Square /Rect [20 10 60 40] /F 4 /AP << /N 13 0 R >> >>",
    ));
    objects.push(appearance("1 0 0 RG 2 w 2 2 36 26 re S"));
    // 14: a check box, its widget merged with its field, and 15 and 16 its
    // two states.
    objects.push(object(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (box) /V /On /AS /On /F 4 \
         /Rect [30 15 70 45] /AP << /N << /On 15 0 R /Off 16 0 R >> >> >>",
    ));
    objects.push(appearance("0 0.6 0.2 rg 4 4 32 22 re f"));
    objects.push(appearance("0 G 1 w 4 4 32 22 re S"));
    // 17: a hidden square over the first, with the same appearance.
    objects.push(object(
        "<< /Type /Annot /Subtype /Square /Rect [50 20 90 50] /F 2 /AP << /N 13 0 R >> >>",
    ));
    // 18: a square with no appearance of its own.
    objects.push(object(
        "<< /Type /Annot /Subtype /Square /Rect [15 10 95 55] /F 4 /C [0 0 1] \
         /IC [1 1 0] /BS << /W 2 >> >>",
    ));
    pdf(&objects)
}

/// What a rendered page is compared by: its size, its pixels and its
/// warnings.
type A9Drawn = (u32, u32, Vec<u8>, Vec<tinker_pdf::RenderWarning>);

fn a9_drawn(bitmap: &tinker_pdf::Bitmap) -> A9Drawn {
    (
        bitmap.width,
        bitmap.height,
        bitmap.data.clone(),
        bitmap.warnings.clone(),
    )
}

/// **A9, the page half: every page walk is the serial walk.**
///
/// Every page of a text, an image and an annotated document, rendered
/// through `Document::render_pages` and read through `Document::text_pages`
/// on 2, 3, 4 and 7 threads, at 72 dpi with annotations and at 150 dpi
/// without them: each bitmap is the page's serial render — size, pixels and
/// warnings, the ones a missing font, a codec this build lacks and a damaged
/// image earn among them — and each page's text the serial text, its warnings
/// included, in page order. The same in the default and `bundled-fonts`
/// builds, and on `wasm32-wasip1`, where the walk runs on the caller.
#[test]
fn a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads() {
    use tinker_pdf::Parallelism;

    let documents = [
        ("text", a9_text_document()),
        ("image", a9_image_document()),
        ("annotated", a9_annotated_document()),
    ];
    let option_sets = [
        RenderOptions::default(),
        RenderOptions {
            annotations: false,
            ..RenderOptions::at_dpi(150.0)
        },
    ];
    let (mut compared, mut warned) = (0usize, 0usize);
    for (name, bytes) in documents {
        let document = Document::open(bytes).expect("the A9 document opens");
        let pages = document.page_count();
        assert!(pages >= 3, "{name}: {pages} pages");
        let serial_text: Vec<String> = (0..pages)
            .map(|index| format!("{:?}", document.page(index).expect("a page").text()))
            .collect();
        for options in &option_sets {
            let serial: Vec<A9Drawn> = (0..pages)
                .map(|index| a9_drawn(&document.page(index).expect("a page").render(options)))
                .collect();
            warned += serial.iter().filter(|page| !page.3.is_empty()).count();
            for threads in [2usize, 3, 4, 7] {
                let walked = RenderOptions {
                    parallelism: Parallelism::Threads(threads),
                    ..options.clone()
                };
                let mut rendered = Vec::new();
                document.render_pages(0..pages, &walked, |index, bitmap| {
                    rendered.push((index, bitmap.as_ref().map(a9_drawn)));
                });
                assert_eq!(rendered.len(), pages as usize, "{name} on {threads}");
                for (at, (index, bitmap)) in rendered.iter().enumerate() {
                    assert_eq!(*index as usize, at, "{name} on {threads}: page order");
                    let bitmap = bitmap.as_ref().expect("every page renders");
                    assert!(
                        *bitmap == serial[at],
                        "{name} on {threads} threads at scale {}: page {at} parts from its \
                         serial render ({:?} walked, {:?} serially)",
                        options.scale,
                        bitmap.3,
                        serial[at].3
                    );
                    compared += 1;
                }
            }
        }
        for threads in [2usize, 3, 4, 7] {
            let walked = TextOptions {
                parallelism: Parallelism::Threads(threads),
                ..TextOptions::default()
            };
            let mut read = Vec::new();
            document.text_pages(0..pages, &walked, |index, text| {
                read.push((index, text.map(|text| format!("{text:?}"))));
            });
            let expected: Vec<(u32, Option<String>)> = serial_text
                .iter()
                .enumerate()
                .map(|(index, text)| (index as u32, Some(text.clone())))
                .collect();
            assert_eq!(read, expected, "{name} on {threads}: the serial text");
        }
    }
    assert!(
        warned >= 4,
        "the warnings compared include real ones: {warned}"
    );
    println!(
        "guard-a9-pages: RAN {compared} walked pages, each byte-identical to its serial \
         render with the same warnings, {warned} of the serial renders warning"
    );
}

/// The facade's save door, what it wrote held to the validate-on-save helper as
/// a rewrite of the bytes the editor was opened over. The save's own tally is
/// taken before the hold, which reads its own copies.
#[track_caller]
fn held_save(
    editor: &mut tinker_pdf::DocumentEditor,
    options: &tinker_pdf::write::SaveOptions,
) -> tinker_pdf::write::Saved {
    let saved = tinker_pdf::write::save(editor, options);
    written_support::rewritten("a save", editor.document().bytes(), &saved.bytes);
    saved
}
