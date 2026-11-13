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

mod guard_support;
mod render_support;

use std::collections::BTreeSet;

use guard_support::{content, deflated, document, face, line, object, pdf, stream, A2_PAGES};
use tinker_pdf::write::{
    save, BilevelCodec, ContinuousCodec, FontPolicy, ImagePolicy, ImageRecoding, SaveOptions,
};
use tinker_pdf::{Document, RenderOptions, Stats, TextOptions};
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
/// joins landed with P-18.5.
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
            "A3",
            "font dictionaries resolved per page at most the distinct /Font entries used",
            "a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses",
            "P-02",
            "P-02.3",
        )
    },
    owed(
        "A4",
        "glyph masks rasterised at most the distinct (font, glyph, scale, phase) keys, \
         hit ratio at least 98 % on a 5 600-glyph page",
        "a4_glyph_masks_rasterised_are_at_most_their_distinct_keys",
        "P-06",
        "P-06.3",
    ),
    owed(
        "A5",
        "inflate calls per stream at most 1 per operation: a save",
        "a5_a_save_decodes_each_stream_at_most_once",
        "P-03",
        "P-03.2",
    ),
    owed(
        "A5",
        "inflate calls per stream at most 1 per operation: a render",
        "a5_a_render_decodes_each_stream_at_most_once",
        "P-03",
        "P-03.2",
    ),
    owed(
        "A5",
        "inflate calls per stream at most 1 per operation: a text extraction",
        "a5_text_decodes_each_stream_at_most_once",
        "P-03",
        "P-03.2",
    ),
    owed(
        "A5",
        "inflate calls per stream at most 1 per operation: image extraction",
        "a5_image_extraction_decodes_each_stream_at_most_once",
        "P-03",
        "P-03.2",
    ),
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
    owed(
        "A9",
        "page-parallel renders byte-identical to serial at 2, 3, 4 and 7 threads",
        "a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads",
        "P-15",
        "P-15.3",
    ),
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
    let saved = save(
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
        save(
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
        save(
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
            total += save(
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
    save(
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
