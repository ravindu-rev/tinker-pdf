//! qpdf as an external arbiter for linearized output (gap 20, ruling 9).
//!
//! Plan 09 names `qpdf --check` and `qpdf --show-linearization` as the
//! arbiters for Annex F, and until this file existed neither had ever run.
//! `tests/linearized.rs` checks everything the engine can check about its own
//! output; what it cannot check is whether the format was *understood*, and
//! the hint tables are where that mattered — they were written by this
//! repository and read by nothing in it, and they were wrong in five separate
//! ways while every test passed.
//!
//! # Ruling 9
//!
//! qpdf is a subprocess, never a dependency. Nothing links it, no part of it
//! is vendored, and its output is a transient comparison reference that is
//! read and dropped. The fixtures are written into `CARGO_TARGET_TMPDIR`
//! because qpdf reads files rather than pipes; nothing there is committed.
//!
//! # Skipped, not silently passed
//!
//! A check that quietly succeeds when its oracle is missing is a check that
//! will one day be missing everywhere. When qpdf cannot be found these tests
//! print [`SKIPPED`] and do nothing, and the CI job greps its own output for
//! [`RAN`] and fails if it does not find it — the same shape as the
//! `wasm-determinism` job, which had the same hazard: a run and a non-run both
//! exit zero and look identical.
//!
//! Set `TINKER_QPDF` to an absolute path when qpdf is installed somewhere the
//! `PATH` does not reach.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tinker_pdf_cos::dest::DestKind;
use tinker_pdf_cos::{
    BlendMode, CosDocument, DeviceSpace, DocumentBuilder, DocumentEditor, Encryption, ExtGState,
    FormXObject, Function, Glyph, MaskKind, OutlineEntry, Shading, StateMask, Target,
    TilingPattern, TilingType, TransparencyGroup, WriteMode, WriteOptions,
};

/// Printed once per test that actually invoked qpdf. CI greps for it.
const RAN: &str = "qpdf-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "qpdf-oracle: SKIPPED";

/// Where qpdf is, or `None`.
///
/// A version query rather than a `which`: an executable that cannot run is
/// not an oracle, and this is the one call whose failure means *skip* rather
/// than *fail*.
fn qpdf() -> Option<PathBuf> {
    let named =
        std::env::var_os("TINKER_QPDF").map_or_else(|| PathBuf::from("qpdf"), PathBuf::from);
    let output = Command::new(&named).arg("--version").output().ok()?;
    output.status.success().then_some(named)
}

/// Runs qpdf and returns its exit code with stdout and stderr joined.
///
/// Joined because qpdf writes its warnings to stderr and its findings to
/// stdout, and every assertion here is about the pair.
fn run(qpdf: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(qpdf)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("qpdf {args:?} did not run: {e}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code().unwrap_or(-1), text)
}

/// The oracle, or a printed skip.
///
/// Every test begins with this. The banner is deliberately shouted: a skipped
/// oracle in a green run is the thing this whole file exists to make visible.
macro_rules! oracle {
    ($what:expr) => {
        match qpdf() {
            Some(path) => {
                println!("{} {} ({})", RAN, $what, path.display());
                path
            }
            None => {
                println!(
                    "{} {} -- qpdf is not on PATH and TINKER_QPDF is unset",
                    SKIPPED, $what
                );
                return;
            }
        }
    };
}

// ---- Fixtures ---------------------------------------------------------------

/// The same document `linearized.rs` uses: one font every page shares.
fn document(page_count: usize) -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "linearized");
    for index in 0..page_count {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

/// A document with a real part 8: page one uses one font, the rest use
/// another that page one never touches, so it is shared between pages two
/// onward and belongs nowhere else.
fn shared_resource_document() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 12.0, 10.0, 50.0, "page 0");
    });
    builder.add_base_font(b"F1", b"Courier");
    for index in 1..6 {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F1", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

fn save(doc: Arc<CosDocument>, encryption: Option<Encryption>) -> Vec<u8> {
    DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        encryption,
        ..WriteOptions::default()
    })
}

fn linearized(page_count: usize) -> Vec<u8> {
    save(document(page_count), None)
}

/// Writes a fixture where qpdf can read it and hands back the path.
///
/// `CARGO_TARGET_TMPDIR` is cargo's own scratch directory for an integration
/// test: inside `target/`, never committed, and the same on every platform.
///
/// A directory per test, because cargo runs them on threads and several
/// point qpdf at the same four documents. Sharing one filename between two
/// threads means one of them rewriting a file the other has open, which on
/// Windows fails outright and elsewhere fails worse — qpdf reading half a
/// file and reporting a defect that is not there.
fn fixture(test: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("qpdf-oracle")
        .join(test);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the fixture is written");
    path
}

/// Every fixture `--check` is pointed at, as (name, bytes).
fn unencrypted_fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        // One page: no shared objects at all, and every hint-table delta is
        // necessarily zero.
        ("one-page.pdf", linearized(1)),
        // Two pages: the smallest document with anything in the shared table
        // to get wrong, and the smallest one that failed before.
        ("two-page.pdf", linearized(2)),
        // Six: enough for a per-page delta to be non-zero more than once.
        ("six-page.pdf", linearized(6)),
        // And one with a part 8, which the other three do not have: their
        // only shared object is the font, and page one uses it, so F.3.8 puts
        // it in part 6 and leaves the shared section empty.
        (
            "shared-resource.pdf",
            save(shared_resource_document(), None),
        ),
    ]
}

// ---- Milestone 2: `qpdf --check` -------------------------------------------

/// The arbiter plan 09 names, run at last.
///
/// Before the hint-table fix this reported
/// `error encountered while checking linearization data: overflow reading bit
/// stream: wanted = 1; available = 0` on everything with more than one page,
/// and four further mismatches on the one-page fixture — `/T`, `/E`, the
/// first page object offset, and a page length — while the whole test suite
/// was green.
#[test]
fn qpdf_check_finds_nothing_wrong_with_the_linearized_output() {
    let qpdf = oracle!("--check over the linearized fixtures");

    for (name, bytes) in unencrypted_fixtures() {
        let path = fixture("check", name, &bytes);
        let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);

        assert!(
            text.contains("File is linearized"),
            "{name}: qpdf does not think it is linearized:\n{text}"
        );
        assert!(
            text.contains("No syntax or stream encoding errors found"),
            "{name}: qpdf found errors:\n{text}"
        );
        assert!(!text.contains("WARNING"), "{name}: qpdf warned:\n{text}");
        assert_eq!(code, 0, "{name}: qpdf exited {code}:\n{text}");
    }
}

/// `--check` alone would pass on a file with no `/Linearized` in it at all,
/// so this says the fixtures are the thing under test.
///
/// It is the same trap gap 19 found in `/H`: an assertion that holds whether
/// or not the feature works is not an assertion about the feature.
#[test]
fn qpdf_says_an_ordinary_file_is_not_linearized() {
    let qpdf = oracle!("--check over an unlinearized file");

    let bytes = DocumentEditor::new(document(6)).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        ..WriteOptions::default()
    });
    let path = fixture("unlinearized", "not-linearized.pdf", &bytes);
    let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);

    assert_eq!(code, 0, "the ordinary layout is still valid:\n{text}");
    assert!(
        text.contains("File is not linearized"),
        "and qpdf can tell the two apart:\n{text}"
    );
}

// ---- Reading our own file, so qpdf has something to be compared against ----
//
// Everything below is recovered from the bytes rather than from the writer's
// internals. `linearize.rs`'s own round-trip reader already checks the tables
// against what `Plan` computed; the point of this half is to arrive at the
// same numbers by a different route, so that a wrong number the writer and
// its reader agree about has somewhere left to be caught.

/// One integer out of the linearization parameter dictionary.
fn parameter(bytes: &[u8], key: &str) -> u64 {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).into_owned();
    let at = text
        .find(&format!("/{key} "))
        .unwrap_or_else(|| panic!("no /{key} in the parameter dictionary: {text}"));
    text[at + key.len() + 2..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

/// One element of the `/H` array: where the hint stream is, and how long.
fn hint(bytes: &[u8], index: usize) -> u64 {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).into_owned();
    let at = text.find("/H [ ").expect("an /H array");
    text[at + 5..]
        .split_whitespace()
        .nth(index)
        .expect("both /H elements")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

/// The offset the last line points at, which is the first-page table (F.3.4).
fn final_startxref(bytes: &[u8]) -> usize {
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64)..]).into_owned();
    let at = tail.rfind("startxref\n").expect("a startxref");
    tail[at + 10..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

/// The in-use entries of one classic cross-reference table (7.5.4).
fn classic_xref(bytes: &[u8], at: usize) -> Vec<(u32, u64)> {
    let head = &bytes[at..];
    assert!(head.starts_with(b"xref\n"), "a table begins at {at}");
    let rest = &head[5..];
    let eol = rest
        .iter()
        .position(|b| *b == b'\n')
        .expect("a header line");
    let header = std::str::from_utf8(&rest[..eol]).expect("ASCII");
    let mut parts = header.split(' ');
    let start: u32 = parts.next().and_then(|t| t.parse().ok()).expect("a number");
    let count: usize = parts.next().and_then(|t| t.parse().ok()).expect("a count");

    let entries = &rest[eol + 1..];
    (0..count)
        .filter_map(|index| {
            let entry = entries.get(index * 20..index * 20 + 20)?;
            (entry[17] == b'n').then(|| {
                let offset = std::str::from_utf8(&entry[..10])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .expect("an offset");
                (start + index as u32, offset)
            })
        })
        .collect()
}

/// Where the main table's `xref` keyword is: the front trailer's `/Prev`.
fn main_xref_offset(bytes: &[u8]) -> usize {
    let at = final_startxref(bytes);
    let head = &bytes[at..];
    let found = find(head, b"trailer\n").expect("a trailer follows the front table");
    let end = find(&head[found..], b">>\n").expect("it ends") + found;
    let text = String::from_utf8_lossy(&head[found..end]).into_owned();
    let prev = text
        .rfind("/Prev ")
        .expect("the front trailer chains onward");
    text[prev + 6..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

/// Where every object begins, and how many bytes it occupies.
///
/// The length is the distance to whatever the file holds next, which is what
/// qpdf measures a page against: objects are written back to back, so an
/// object's span is the gap to its successor and the last one runs to the
/// main table.
fn object_extents(bytes: &[u8]) -> std::collections::BTreeMap<u32, (u64, u64)> {
    let offsets: std::collections::BTreeMap<u32, u64> =
        classic_xref(bytes, main_xref_offset(bytes))
            .into_iter()
            .chain(classic_xref(bytes, final_startxref(bytes)))
            .collect();

    let mut in_file_order: Vec<u64> = offsets.values().copied().collect();
    in_file_order.push(main_xref_offset(bytes) as u64);
    in_file_order.sort_unstable();

    offsets
        .iter()
        .map(|(num, at)| {
            let next = in_file_order
                .iter()
                .copied()
                .find(|other| *other > *at)
                .unwrap_or(bytes.len() as u64);
            (*num, (*at, next - *at))
        })
        .collect()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&at| &haystack[at..at + needle.len()] == needle)
}

// ---- Reading qpdf's report -------------------------------------------------

/// One page's entry as qpdf prints it back.
#[derive(Default)]
struct PageHint {
    nobjects: u32,
    length: u64,
    nshared: u32,
    identifiers: Vec<u32>,
}

/// `qpdf --show-linearization`, parsed.
///
/// Top-level `key: value` lines go into `values`; the indented lines under a
/// `Page N:` or `Shared Object N:` heading belong to that block. Indentation
/// is the whole grammar, which is why `nobjects` in a page block and
/// `nbits_nobjects` in the shared header cannot be confused.
#[derive(Default)]
struct Report {
    values: std::collections::BTreeMap<String, i64>,
    pages: Vec<PageHint>,
    groups: Vec<u64>,
}

impl Report {
    fn get(&self, key: &str) -> i64 {
        *self
            .values
            .get(key)
            .unwrap_or_else(|| panic!("qpdf printed no {key}: {:?}", self.values))
    }
}

fn parse_report(text: &str) -> Report {
    let mut report = Report::default();
    let mut in_page = false;
    let mut in_group = false;

    for line in text.lines() {
        let indented = line.starts_with(' ');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !indented {
            if trimmed.starts_with("Page ") && trimmed.ends_with(':') {
                report.pages.push(PageHint::default());
                in_page = true;
                in_group = false;
                continue;
            }
            if trimmed.starts_with("Shared Object ") && trimmed.ends_with(':') {
                report.groups.push(0);
                in_page = false;
                in_group = true;
                continue;
            }
        }

        let Some((key, value)) = trimmed.split_once(": ") else {
            continue;
        };
        let Ok(number) = value.trim().parse::<i64>() else {
            continue;
        };

        if indented && in_page {
            let page = report.pages.last_mut().expect("a page block");
            match key {
                "nobjects" => page.nobjects = number as u32,
                "length" => page.length = number as u64,
                "nshared_objects" => page.nshared = number as u32,
                _ if key.starts_with("identifier ") => page.identifiers.push(number as u32),
                _ => {}
            }
        } else if indented && in_group {
            if key == "group length" {
                *report.groups.last_mut().expect("a group block") = number as u64;
            }
        } else {
            report.values.insert(key.to_string(), number);
            in_page = false;
            in_group = false;
        }
    }
    report
}

// ---- Milestone 3: `qpdf --show-linearization` ------------------------------

/// Which object a shared-table entry describes (F.4.2).
fn shared_entry_object(report: &Report, first_page_object: u32, index: u32) -> u32 {
    let first_page_entries = report.get("nshared_first_page") as u32;
    if index < first_page_entries {
        first_page_object + index
    } else {
        report.get("first_shared_obj") as u32 + (index - first_page_entries)
    }
}

/// Everything qpdf reads out of the file is what the writer put in.
///
/// The parameter dictionary first, then the hint tables — and the hint tables
/// are the part nothing had ever read, here or anywhere. Each expectation is
/// recomputed from the bytes: `first_page_offset` against the cross-reference
/// entry for `/O`, each page's object count against the gap to the next
/// page's object number, each page's length and each shared group's length
/// against the actual spans of the objects in them.
///
/// qpdf prints hint-table offsets already adjusted — F.4 stores them as
/// though the hint stream were absent and qpdf adds `H_length` back — so the
/// values compared here are plain file offsets.
fn assert_the_report_matches_the_file(report: &Report, bytes: &[u8], pages: &[u32], name: &str) {
    let extents = object_extents(bytes);
    let length_of = |num: u32| extents.get(&num).map_or(0, |(_, len)| *len);
    let offset_of = |num: u32| extents.get(&num).map(|(at, _)| *at);

    // The parameter dictionary, F.2.2.
    assert_eq!(report.get("file_size") as u64, bytes.len() as u64, "{name}");
    assert_eq!(report.get("npages") as usize, pages.len(), "{name}");
    assert_eq!(report.get("npages") as u64, parameter(bytes, "N"), "{name}");
    assert_eq!(
        report.get("first_page_object") as u64,
        parameter(bytes, "O"),
        "{name}: /O"
    );
    assert_eq!(
        report.get("first_page_object") as u32,
        pages[0],
        "{name}: and /O really is the first page"
    );
    assert_eq!(
        report.get("first_page_end") as u64,
        parameter(bytes, "E"),
        "{name}: /E"
    );
    assert_eq!(
        report.get("xref_zero_offset") as u64,
        parameter(bytes, "T"),
        "{name}: /T"
    );
    assert_eq!(report.get("H_offset") as u64, hint(bytes, 0), "{name}: /H");
    assert_eq!(report.get("H_length") as u64, hint(bytes, 1), "{name}: /H");

    // Table F.3 item 2: a byte offset, and the one field that held an object
    // number until this gap. The two are not confusable in either direction —
    // `/O` is a single digit in these fixtures and its object sits hundreds of
    // bytes in.
    assert_eq!(
        report.get("first_page_offset") as u64,
        offset_of(pages[0]).expect("/O is in a table"),
        "{name}: the first page's page object is where the hint table says"
    );

    // Table F.4: a page is a run of consecutive numbers from its page object,
    // so its count is the gap to the next page's number.
    assert_eq!(report.pages.len(), pages.len(), "{name}");
    for (index, page) in report.pages.iter().enumerate() {
        let start = pages[index];
        if let Some(next) = pages.get(index + 1) {
            assert_eq!(
                page.nobjects,
                next - start,
                "{name}: page {index}'s run reaches the next page's object"
            );
        } else {
            assert!(page.nobjects >= 1, "{name}: page {index} owns something");
        }

        let run: Vec<u32> = (start..start + page.nobjects).collect();
        for num in &run {
            assert!(
                extents.contains_key(num),
                "{name}: page {index}'s run names object {num}, which no table carries"
            );
        }
        assert_eq!(
            page.length,
            run.iter().map(|num| length_of(*num)).sum::<u64>(),
            "{name}: page {index}'s declared length is the bytes of its run"
        );

        assert_eq!(
            page.nshared as usize,
            page.identifiers.len(),
            "{name}: page {index} lists as many identifiers as it declares"
        );
        for id in &page.identifiers {
            assert!(
                i64::from(*id) < report.get("nshared_total"),
                "{name}: page {index} names shared entry {id}, which does not exist"
            );
            assert!(
                !run.contains(&shared_entry_object(report, pages[0], *id)),
                "{name}: page {index} names a shared entry that is its own object"
            );
        }
    }
    assert_eq!(
        report.pages[0].nshared, 0,
        "{name}: page one's own objects are the first shared entries, so it names none"
    );

    // Table F.5 and Table F.6: the first `nshared_first_page` entries are
    // part 6's objects, consecutively from `/O`; the rest are part 8's,
    // consecutively from `first_shared_obj`.
    assert_eq!(
        report.get("nshared_first_page") as u32,
        report.pages[0].nobjects,
        "{name}: part 6's objects are the first shared entries"
    );
    assert_eq!(
        report.get("nshared_total") as usize,
        report.groups.len(),
        "{name}"
    );
    let first_shared = report.get("first_shared_obj") as u32;
    if report.get("nshared_total") > report.get("nshared_first_page") {
        assert!(first_shared != 0, "{name}: part 8 has a first object");
        assert_eq!(
            report.get("first_shared_offset") as u64,
            offset_of(first_shared).expect("part 8's first object is in a table"),
            "{name}: and it is where the hint table says"
        );
    } else {
        assert_eq!(first_shared, 0, "{name}: there is no part 8");
        assert_eq!(report.get("first_shared_offset"), 0, "{name}");
    }
    for (index, group) in report.groups.iter().enumerate() {
        let num = shared_entry_object(report, pages[0], index as u32);
        assert_eq!(
            *group,
            length_of(num),
            "{name}: shared entry {index} is object {num}, whose span it must be"
        );
    }
}

/// The page objects, in page order, as this engine's own reader sees them.
fn page_objects(bytes: &[u8]) -> Vec<u32> {
    let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
    tinker_pdf_cos::pages::collect(&doc)
        .iter()
        .map(|page| page.reference.num)
        .collect()
}

/// Milestone 3, over the same four fixtures `--check` sees.
#[test]
fn qpdf_reads_the_hint_tables_back_as_the_writer_meant_them() {
    let qpdf = oracle!("--show-linearization over the linearized fixtures");

    for (name, bytes) in unencrypted_fixtures() {
        let path = fixture("show", name, &bytes);
        let (code, text) = run(
            &qpdf,
            &["--show-linearization", &path.display().to_string()],
        );

        assert_eq!(code, 0, "{name}: qpdf exited {code}:\n{text}");
        assert!(!text.contains("WARNING"), "{name}: qpdf warned:\n{text}");
        assert!(
            !text.contains("overflow reading bit stream"),
            "{name}: the tables do not parse:\n{text}"
        );

        let report = parse_report(&text);
        assert_the_report_matches_the_file(&report, &bytes, &page_objects(&bytes), name);
    }
}

/// The shared-resource fixture is the only one with a part 8, and this says
/// so out of qpdf's mouth rather than the writer's.
///
/// Without it the shared-object table's two offsets would be zero in every
/// fixture, and a writer that always wrote zeros there would pass milestone 3
/// unchallenged.
#[test]
fn qpdf_finds_a_shared_section_only_where_there_is_one() {
    let qpdf = oracle!("--show-linearization over a part 8");

    let mut with = 0usize;
    for (name, bytes) in unencrypted_fixtures() {
        let path = fixture("part-eight", name, &bytes);
        let (_, text) = run(
            &qpdf,
            &["--show-linearization", &path.display().to_string()],
        );
        let report = parse_report(&text);

        let extra = report.get("nshared_total") - report.get("nshared_first_page");
        if name == "shared-resource.pdf" {
            assert_eq!(extra, 1, "one font page one never uses:\n{text}");
            assert!(report.get("first_shared_obj") > 0, "{name}");
            assert!(report.get("first_shared_offset") > 0, "{name}");
            with += 1;
        } else {
            assert_eq!(extra, 0, "{name} has no part 8:\n{text}");
        }
    }
    assert_eq!(with, 1, "the shared-resource fixture was actually tested");
}

// ---- Milestone 4: the encrypted linearized output --------------------------

/// The password gap 19's fixture opens with.
const PASSWORD: &str = "open-me";

/// Forty-eight deterministic bytes, as in `linearized.rs`. The padding AES
/// adds is a function of the plaintext's length, so the layout is
/// reproducible exactly when the entropy is.
fn entropy() -> [u8; 48] {
    let mut bytes = [0u8; 48];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    bytes
}

/// Gap 19's fixture, byte for byte: `encrypted_linearized(6)` with the
/// deterministic entropy and the password `linearized.rs` uses.
fn encrypted_linearized(page_count: usize) -> Vec<u8> {
    save(
        document(page_count),
        Some(Encryption {
            user_password: PASSWORD.to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy: entropy(),
        }),
    )
}

/// The one complaint an encrypted file this engine writes still earns, and
/// the only one allowed through.
///
/// 7.5.5 Table 15 requires an `/ID` whenever `/Encrypt` is present, and
/// nothing here writes one on any path — so qpdf reports
/// `invalid /ID in trailer dictionary` on every encrypted file it is shown,
/// linearized or not. Gap 19 found it and left it as a live audit row rather
/// than inventing an identifier: R6 does not mix `/ID` into key derivation,
/// so nothing fails to decrypt, and all forty-eight entropy bytes are already
/// spoken for.
///
/// It costs this milestone one thing and it is worth naming. `--check` on an
/// encrypted file suppresses its `No syntax or stream encoding errors found`
/// summary once anything has warned, so that line cannot be asserted here —
/// which is why the decrypted copy is checked as well, where it can be.
///
/// Allowed for by name rather than ignored: a warning that does not mention
/// `/ID` fails, so this cannot become an amnesty for whatever qpdf says next.
fn assert_only_the_missing_id_was_reported(text: &str, name: &str) {
    for warning in text.lines().filter(|line| line.contains("WARNING")) {
        assert!(
            warning.contains("/ID"),
            "{name}: qpdf warned about something other than the missing /ID: {warning}"
        );
    }

    // The complaints this gap exists to remove, by the words qpdf uses for
    // them: a failed hint-table parse arrives wrapped in `error encountered
    // while checking linearization data`, and every field disagreement is a
    // `mismatch`. Named rather than matched loosely, because
    // `--show-linearization` prints the word "linearization" on its first
    // line whatever happens.
    for phrase in ["error encountered", "mismatch", "overflow reading"] {
        assert!(
            !text.contains(phrase),
            "{name}: qpdf reported \"{phrase}\":\n{text}"
        );
    }
}

/// Milestone 4, first half: `qpdf --check` on gap 19's encrypted fixture.
///
/// Gap 19 predicted that gap 20 would find the *same* hint-table error here
/// as on the unencrypted file, and said a difference between the two would be
/// an encryption defect rather than this gap's. There is no error on either
/// now, which is the same statement with its sign flipped: encryption changes
/// every length in the file and none of its claims, so the encrypted layout
/// has to satisfy exactly what the plaintext one does.
#[test]
fn qpdf_check_finds_nothing_wrong_with_the_encrypted_linearized_output() {
    let qpdf = oracle!("--check over the encrypted linearized fixture");
    let name = "encrypted-check.pdf";

    let bytes = encrypted_linearized(6);
    let path = fixture("encrypted-check", name, &bytes);
    let file = path.display().to_string();
    let (_, text) = run(&qpdf, &["--password=open-me", "--check", &file]);

    // The encryption qpdf sees is the encryption gap 19 built; nothing below
    // this means anything unless qpdf actually authenticated.
    for needle in [
        "R = 6",
        "Supplied password is user password",
        "stream encryption method: AESv3",
        "string encryption method: AESv3",
        "File is linearized",
    ] {
        assert!(
            text.contains(needle),
            "{name}: {needle} is missing:\n{text}"
        );
    }
    assert_only_the_missing_id_was_reported(&text, name);

    // And the whole-file claim, on a decrypted copy where qpdf will still
    // make it. `--check` calling this free of syntax and stream encoding
    // errors means every object offset resolved and every stream decrypted —
    // which is the strongest single statement available about a layout
    // measured from ciphertext.
    let copy = path.with_file_name("encrypted-check-decrypted.pdf");
    let copy_name = copy.display().to_string();
    let (_, decrypted) = run(
        &qpdf,
        &["--password=open-me", "--decrypt", &file, &copy_name],
    );
    assert_only_the_missing_id_was_reported(&decrypted, name);

    let (code, text) = run(&qpdf, &["--check", &copy_name]);
    assert_eq!(code, 0, "the decrypted copy: qpdf exited {code}:\n{text}");
    assert!(
        text.contains("No syntax or stream encoding errors found"),
        "every offset resolved and every stream decrypted:\n{text}"
    );
    assert!(!text.contains("WARNING"), "and cleanly:\n{text}");
}

/// Milestone 4, second half: `--show-linearization` on the same file, held to
/// the same assertions the plaintext fixtures are.
///
/// Deliberately the *same* function rather than a weaker copy written beside
/// it, exactly as gap 19 did with its offset assertions. Both cross-reference
/// tables and the parameter dictionary stay in the clear (7.6.1), so
/// `assert_the_report_matches_the_file` can recompute every expectation from
/// the raw bytes with no key involved — and that it can is itself the
/// assertion that they are clear.
#[test]
fn qpdf_reads_the_encrypted_hint_tables_back_as_the_writer_meant_them() {
    let qpdf = oracle!("--show-linearization over the encrypted linearized fixture");
    let name = "encrypted-show.pdf";

    let bytes = encrypted_linearized(6);
    let path = fixture("encrypted-show", name, &bytes);
    let (_, text) = run(
        &qpdf,
        &[
            "--password=open-me",
            "--show-linearization",
            &path.display().to_string(),
        ],
    );
    assert_only_the_missing_id_was_reported(&text, name);

    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    assert_eq!(
        doc.authenticate(PASSWORD),
        Ok(tinker_pdf_cos::AuthLevel::User),
        "the fixture really is encrypted"
    );

    let report = parse_report(&text);
    assert_the_report_matches_the_file(&report, &bytes, &page_objects(&bytes), name);
}

/// The encrypted file describes the same document and different bytes.
///
/// The hint tables describe a *layout*, and encryption moves every byte of it
/// while leaving the document alone. So the structure must come back
/// identical — the same page count, the same object count for every page, the
/// same shared entries and identifiers — and every page's declared length
/// must be strictly larger, because each page owns a content stream and
/// AES-CBC never returns one the same size.
///
/// That is the assertion gap 19's `/H` defect would have failed. A table
/// measured from the plaintext gives an encrypted file the *unencrypted*
/// lengths, which is the one shape this comparison forbids.
#[test]
fn the_encrypted_hint_tables_describe_the_same_pages_at_greater_length() {
    let qpdf = oracle!("--show-linearization on both halves of one document");

    let read = |name: &str, bytes: &[u8], password: bool| -> Report {
        let path = fixture("comparison", name, bytes);
        let mut args: Vec<String> = Vec::new();
        if password {
            args.push(format!("--password={PASSWORD}"));
        }
        args.push("--show-linearization".to_string());
        args.push(path.display().to_string());
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        parse_report(&run(&qpdf, &borrowed).1)
    };

    let plain = read("comparison-plain.pdf", &linearized(6), false);
    let sealed = read("comparison-sealed.pdf", &encrypted_linearized(6), true);

    assert_eq!(plain.get("npages"), sealed.get("npages"));
    assert_eq!(
        plain.get("nshared_first_page"),
        sealed.get("nshared_first_page")
    );
    assert_eq!(plain.get("nshared_total"), sealed.get("nshared_total"));
    assert_eq!(plain.pages.len(), sealed.pages.len());

    let mut compared = 0usize;
    for (index, (before, after)) in plain.pages.iter().zip(sealed.pages.iter()).enumerate() {
        assert_eq!(
            before.nobjects, after.nobjects,
            "page {index} owns the same objects either way"
        );
        assert_eq!(before.nshared, after.nshared, "page {index}");
        assert_eq!(
            before.identifiers, after.identifiers,
            "page {index} shares the same entries"
        );
        assert!(
            after.length > before.length,
            "page {index} is {} bytes encrypted against {} plain, and a page holding a \
             content stream cannot fail to grow",
            after.length,
            before.length
        );
        compared += 1;
    }
    assert_eq!(compared, 6, "every page was compared");

    // And the file grew by more than one object's worth, so the comparison
    // above is measuring padding across the whole layout rather than a single
    // stream at the end.
    let growth = sealed.get("file_size") - plain.get("file_size");
    assert!(
        growth > 150,
        "the encrypted file is only {growth} bytes longer"
    );
}

// ---- Gap 30 milestone 5: the writer's missing half -------------------------
//
// Everything this milestone added is object structure — `/ExtGState`
// dictionaries, form XObjects with `/Group`, shadings over functions,
// patterns, and a Type0 font with a descendant and a `/ToUnicode`. Gap 30's
// oracle section says so in as many words: this plan "writes strictly more
// object structure than gap 29 did … so there is strictly more that only qpdf
// can see."
//
// That is not an abstract worry. Everything else written for this milestone is
// this engine reading its own output through its own parser, and this engine's
// reader is lenient by design: `read_shading` defaults a missing `/Extend` to
// `(false, false)`, `parse_function` defaults a missing `/Domain` to `[0 1]`,
// and the tiling reader falls back to the cell's own size for a `/XStep` it
// cannot find. A shading written with the wrong key names would round-trip
// through all of them and be an empty dictionary to anybody else.

/// A four-glyph TrueType program with real metrics.
///
/// Small enough to read in a dump and complete enough to embed: `glyf`,
/// `loca`, `head`, `hhea`, `hmtx`, `maxp` and a `cmap`. The advances are 600,
/// 700, 800 and 900 font units against 1 000 per em, so `/W` is legible in
/// qpdf's own output rather than being something only this engine can check.
fn metric_font() -> Vec<u8> {
    let mut square = Vec::new();
    square.extend_from_slice(&1i16.to_be_bytes());
    for value in [0i16, 0, 700, 700] {
        square.extend_from_slice(&value.to_be_bytes());
    }
    square.extend_from_slice(&3u16.to_be_bytes());
    square.extend_from_slice(&0u16.to_be_bytes());
    square.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    for dx in [0i16, 700, 0, -700] {
        square.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 700, 0] {
        square.extend_from_slice(&dy.to_be_bytes());
    }

    let size = square.len() as u32;
    let mut glyf = Vec::new();
    for _ in 0..3 {
        glyf.extend_from_slice(&square);
    }
    let mut loca = Vec::new();
    for offset in [0u32, 0, size, size * 2, size * 3] {
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&4u16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&4u16.to_be_bytes());

    let mut hmtx = Vec::new();
    for advance in [600u16, 700, 800, 900] {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    let first = u16::from(b'A');
    let mut sub = Vec::new();
    for value in [4u16, 32, 0, 4, 4, 1, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for value in [
        first,
        0xFFFF,
        0,
        first,
        0xFFFF,
        1u16.wrapping_sub(first),
        1,
        0,
        0,
    ] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);

    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"maxp", &maxp),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// One page using **every** construct this milestone added.
///
/// One document rather than nine, because `--check` reads the whole file and
/// what is being asked is whether a third party can walk all of it at once.
fn whole_surface_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_info(b"Title", "the writer's missing half");
    assert!(builder.add_cid_font(b"C0", b"Oracle", &metric_font()));

    assert!(builder.add_ext_gstate(
        b"GHalf",
        &ExtGState {
            fill_alpha: Some(0.5),
            stroke_alpha: Some(0.25),
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"FmMask",
        &FormXObject {
            bbox: [0.0, 0.0, 150.0, 200.0],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Gray,
                isolated: true,
                knockout: false,
            }),
            content: b"1 g 0 0 150 200 re f",
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GMask",
        &ExtGState {
            soft_mask: Some(StateMask::Group {
                kind: MaskKind::Luminosity,
                form: b"FmMask",
                backdrop: Some(&[0.0]),
            }),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GOff",
        &ExtGState {
            soft_mask: Some(StateMask::None),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"FmKnock",
        &FormXObject {
            bbox: [0.0, 0.0, 100.0, 100.0],
            matrix: Some([1.0, 0.0, 0.0, 1.0, 20.0, 20.0]),
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Rgb,
                isolated: false,
                knockout: true,
            }),
            content: b"/GHalf gs 1 0 0 rg 0 0 60 60 re f 0 0 1 rg 30 30 60 60 re f",
        }
    ));
    assert!(builder.add_shading(
        b"ShAxial",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [0.0, 0.0, 300.0, 0.0],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![1.0, 0.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    assert!(builder.add_shading(
        b"ShRadial",
        &Shading::Radial {
            color_space: DeviceSpace::Rgb,
            coords: [150.0, 100.0, 0.0, 150.0, 100.0, 90.0],
            function: Function::Stitching {
                domain: [0.0, 1.0],
                functions: vec![
                    Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![1.0, 1.0, 0.0],
                        c1: vec![0.0, 1.0, 0.0],
                        n: 1.0,
                    },
                    Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![0.0, 1.0, 0.0],
                        c1: vec![0.0, 0.0, 1.0],
                        n: 2.0,
                    },
                ],
                bounds: vec![0.35],
                encode: vec![[0.0, 1.0], [0.0, 1.0]],
            },
            extend: (false, true),
        }
    ));
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, 10.0, 10.0],
            x_step: 12.0,
            y_step: 14.0,
            matrix: Some([1.0, 0.0, 0.0, 1.0, 3.0, 5.0]),
            tiling_type: TilingType::NoDistortion,
            content: b"0 0 1 rg 0 0 10 10 re f",
        }
    ));

    builder.add_page(300.0, 200.0, |page| {
        page.raw(b"q 0 0 300 200 re W n");
        assert!(page.shading(b"ShAxial"));
        page.raw(b"Q");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GMask"));
        assert!(page.set_fill_pattern(b"P0"));
        page.raw(b"10 10 120 60 re f");
        assert!(page.set_ext_gstate(b"GOff"));
        page.raw(b"Q");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GHalf"));
        assert!(page.form(b"FmKnock"));
        page.raw(b"Q");
        page.raw(b"q 200 20 80 80 re W n");
        assert!(page.shading(b"ShRadial"));
        page.raw(b"Q");
        page.set_stroke_rgb(0.0, 0.0, 0.0);
        assert!(page.set_stroke_pattern(b"P0"));
        page.raw(b"2 w 10 150 m 290 150 l S");
        assert!(page.glyphs(
            b"C0",
            18.0,
            20.0,
            170.0,
            &[
                Glyph { id: 1, text: "f" },
                Glyph { id: 2, text: "f" },
                Glyph { id: 3, text: "ffi" },
            ]
        ));
    });
    builder.finish()
}

/// Every object of the document, flattened to single spaces and joined.
///
/// Read out of `--show-object` per object rather than out of one dump,
/// because the object numbering is the writer's business and a test that
/// hardcoded it would break on a change that is not a defect. Gap 29's
/// milestone 5 and gap 30's milestone 4 each had to rebuild an assertion
/// against qpdf's real output after assuming its shape; this walks whatever is
/// there.
fn objects(qpdf: &Path, path: &Path) -> Vec<(u32, String)> {
    (1..64u32)
        .filter_map(|number| {
            let (code, text) = run(
                qpdf,
                &[
                    &format!("--show-object={number}"),
                    &path.display().to_string(),
                ],
            );
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            // An object number past the end prints `null` and still exits 0,
            // which is qpdf saying "no such object" rather than failing.
            (code == 0 && flat != "null").then_some((number, flat))
        })
        .collect()
}

/// One stream's data, decoded.
///
/// `--show-object` prints a stream's **dictionary** and says `Object is
/// stream.` where the data would be, so a `/ToUnicode` CMap is invisible to
/// it. `--filtered-stream-data` is the flag that hands the bytes over, and it
/// was found by running the tool rather than by assuming the first command
/// showed everything — the same lesson gap 30's milestone 4 recorded about
/// `--show-pages` and gap 29's milestone 5 recorded before it.
fn stream_data(qpdf: &Path, path: &Path, number: u32) -> String {
    let (code, text) = run(
        qpdf,
        &[
            &format!("--show-object={number}"),
            "--filtered-stream-data",
            &path.display().to_string(),
        ],
    );
    assert_eq!(code, 0, "qpdf --show-object={number}: {text}");
    text
}

/// The object number an indirect reference under `key` names, out of a
/// dictionary qpdf printed.
fn referenced(object: &str, key: &str) -> u32 {
    let rest = object
        .split_once(key)
        .unwrap_or_else(|| panic!("no {key} in {object}"))
        .1;
    rest.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{key} is not a reference in {object}"))
}

/// Everything qpdf printed, one object to a line, for a failure message.
fn listing(dump: &[(u32, String)]) -> String {
    dump.iter()
        .map(|(number, text)| format!("{number}: {text}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// qpdf reads the whole of the writer's new surface without complaint, in both
/// write modes.
#[test]
fn qpdf_check_finds_nothing_wrong_with_the_writers_whole_surface() {
    let qpdf = oracle!("--check over the writer's whole surface");

    let built = whole_surface_document();
    let document = Arc::new(CosDocument::open(built.clone()).expect("it opens"));
    let cases = [
        ("built.pdf", built),
        (
            "rewritten.pdf",
            DocumentEditor::new(Arc::clone(&document)).save(&WriteOptions {
                mode: WriteMode::Rewrite,
                ..WriteOptions::default()
            }),
        ),
        ("linearized.pdf", save(Arc::clone(&document), None)),
        (
            "compressed.pdf",
            DocumentEditor::new(document).save(&WriteOptions {
                mode: WriteMode::Rewrite,
                compress: true,
                object_streams: true,
                ..WriteOptions::default()
            }),
        ),
    ];

    for (name, bytes) in cases {
        let path = fixture("surface-check", name, &bytes);
        let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);
        assert_eq!(code, 0, "qpdf --check on {name}:\n{text}");
        assert!(
            text.contains("No syntax or stream encoding errors found"),
            "{name}: qpdf found something:\n{text}"
        );
    }
}

/// qpdf sees the graphics state, the two groups and the two gradients as they
/// were written.
///
/// The entries chosen are the ones whose *absence* this engine's own reader
/// would supply a default for, so a comparison here cannot share a mistake
/// with the round-trip tests: `/Extend`, `/XStep`, `/YStep`, `/Bounds` and
/// `/Encode` each have a reader-side fallback, and `/I` and `/K` default to
/// false.
#[test]
fn qpdf_reads_back_the_states_the_groups_the_gradients_and_the_pattern() {
    let qpdf = oracle!("--show-object over the writer's whole surface");
    let path = fixture("surface-objects", "surface.pdf", &whole_surface_document());
    let dump = objects(&qpdf, &path);
    let joined = listing(&dump);
    let holds = |needle: &str| dump.iter().any(|(_, object)| object.contains(needle));

    // A note on the spelling of everything below. **qpdf sorts a dictionary's
    // keys**, so it prints `/Group << /CS /DeviceGray /I true /S /Transparency
    // >>` for a group this writer emits in 11.6.6's own order. Every
    // expectation here was read off the tool before it was asserted against,
    // which is what gap 30's milestone 4 and gap 29's milestone 5 each had to
    // do after assuming a format instead.

    // 11.6.4.4 and 11.3.5: three parameters in one dictionary, and `/ca`
    // against `/CA` is a distinction qpdf preserves and a case-insensitive
    // reader would not.
    assert!(
        holds("/BM /Multiply") && holds("/CA 0.25") && holds("/ca 0.5"),
        "the graphics state:\n{joined}"
    );
    assert!(holds("/SMask /None"), "and the one that turns a mask off");
    assert!(
        holds("/S /Luminosity"),
        "and the luminosity mask's own kind"
    );
    assert!(holds("/BC [ 0 ]"), "with the backdrop it was given");

    // 11.6.6: two flags, written only when true, and this document sets one
    // group's isolation and the other's knockout so neither can stand in for
    // the other here either.
    assert!(
        holds("/Group << /CS /DeviceGray /I true /S /Transparency >>"),
        "the isolated luminosity group:\n{joined}"
    );
    assert!(
        holds("/Group << /CS /DeviceRGB /K true /S /Transparency >>"),
        "the knockout group:\n{joined}"
    );

    // 8.7.4.5.3 and 8.7.4.5.4: four coordinates against six.
    assert!(
        holds("/ShadingType 2") && holds("/Coords [ 0 0 300 0 ]"),
        "the axial shading:\n{joined}"
    );
    assert!(
        holds("/ShadingType 3") && holds("/Coords [ 150 100 0 150 100 90 ]"),
        "the radial shading:\n{joined}"
    );
    assert!(
        holds("/Extend [ true true ]") && holds("/Extend [ false true ]"),
        "and each keeps its own pair:\n{joined}"
    );

    // 7.10.3 and 7.10.4: the stitch, and the sub-functions it stitches.
    assert!(
        holds("/FunctionType 3") && holds("/Bounds [ 0.35 ]"),
        "the stitching function:\n{joined}"
    );
    assert!(
        holds("/Encode [ 0 1 0 1 ]"),
        "and its encode pairs:\n{joined}"
    );
    assert!(
        holds("/C0 [ 1 1 0 ] /C1 [ 0 1 0 ] /Domain [ 0 1 ] /FunctionType 2 /N 1")
            && holds("/C0 [ 0 1 0 ] /C1 [ 0 0 1 ] /Domain [ 0 1 ] /FunctionType 2 /N 2"),
        "and both sub-functions, each a type 2 with its own exponent:\n{joined}"
    );

    // 8.7.3.1: the two steps differ from each other and from the cell.
    assert!(
        holds("/PatternType 1") && holds("/PaintType 1") && holds("/TilingType 2"),
        "the tiling pattern:\n{joined}"
    );
    assert!(
        holds("/XStep 12") && holds("/YStep 14") && holds("/BBox [ 0 0 10 10 ]"),
        "with the cell and the spacing it was given:\n{joined}"
    );
    assert!(
        holds("/Matrix [ 1 0 0 1 3 5 ]"),
        "and its own matrix:\n{joined}"
    );
}

/// qpdf reads the composite font back as 9.7's published structure.
///
/// The three entries that make a glyph index addressable are asserted from
/// outside: `/Encoding /Identity-H` on the Type0 font, `/CIDToGIDMap
/// /Identity` on the descendant, and a `/W` whose numbers are the font
/// program's own advances. A `/ToUnicode` stream is there too, and it is read
/// back rather than assumed — `--show-object` prints a stream's data, so the
/// CMap's own text is in the dump.
#[test]
fn qpdf_reads_back_the_composite_font_as_identity_h_over_a_cid_font() {
    let qpdf = oracle!("--show-object over a Type0 font");
    let path = fixture("surface-font", "surface.pdf", &whole_surface_document());
    let dump = objects(&qpdf, &path);
    let joined = listing(&dump);
    let holds = |needle: &str| dump.iter().any(|(_, object)| object.contains(needle));

    assert!(
        holds("/Subtype /Type0") && holds("/Encoding /Identity-H"),
        "the code is the CID:\n{joined}"
    );
    assert!(
        holds("/Subtype /CIDFontType2") && holds("/CIDToGIDMap /Identity"),
        "and the CID is the glyph index:\n{joined}"
    );
    assert!(
        holds("/Ordering (Identity) /Registry (Adobe) /Supplement 0"),
        "the descendant's ordering agrees with the encoding:\n{joined}"
    );
    // 700, 800 and 900 font units at 1 000 per em, run together because the
    // glyphs the page drew are consecutive.
    assert!(
        holds("/W [ 1 [ 700 800 900 ] ]"),
        "the widths are the font's own hmtx:\n{joined}"
    );
    assert!(holds("/DW 1000"), "with a stated default:\n{joined}");
    assert!(
        holds("/Flags 4"),
        "and a symbolic descriptor, since there is no encoding to look a code up in"
    );

    // The mapping a simple font cannot hold, read out of the CMap stream
    // itself: two glyphs standing for one character, and one standing for
    // three.
    let type0 = dump
        .iter()
        .find(|(_, object)| object.contains("/Subtype /Type0"))
        .map(|(_, object)| object.clone())
        .unwrap_or_else(|| panic!("no Type0 font:\n{joined}"));
    let cmap = stream_data(&qpdf, &path, referenced(&type0, "/ToUnicode"));
    assert!(
        cmap.contains("/CMapType 2") && cmap.contains("<0000> <FFFF>"),
        "the CMap is a two-byte /ToUnicode:\n{cmap}"
    );
    assert!(
        cmap.contains("3 beginbfchar"),
        "with one entry per glyph the page drew:\n{cmap}"
    );
    assert!(
        cmap.contains("<0001> <0066>") && cmap.contains("<0002> <0066>"),
        "many glyphs to one character:\n{cmap}"
    );
    assert!(
        cmap.contains("<0003> <006600660069>"),
        "and one glyph to many:\n{cmap}"
    );
}

/// A document carrying both a link annotation and an outline, read by qpdf.
///
/// The round trip in `tinker-pdf/tests/writer_navigation.rs` proves this
/// repository can read its own navigation back. This proves somebody else can,
/// which is a different claim and the one that catches a structure only this
/// reader is lenient about — gap 29 milestone 5 found a defect **only** qpdf
/// caught, and gap 30 milestone 4 found `/CropBox` written where it should have
/// been absent.
///
/// The output format was read before it was asserted against. qpdf prints
/// `/Count -1` with the space, sorts a dictionary's keys, and renders an
/// outline through `--show-object` rather than any dedicated flag — seven
/// milestones running have had to rewrite an assertion after assuming
/// otherwise.
#[test]
fn qpdf_reads_a_documents_links_and_its_outline() {
    let qpdf = oracle!("--check over links and an outline");

    let mut builder = DocumentBuilder::new();
    for _ in 0..3 {
        builder.add_page(200.0, 300.0, |_| {});
    }
    builder.add_page(200.0, 300.0, |page| {
        assert!(page.link(
            10.0,
            20.0,
            90.0,
            40.0,
            &Target::Page {
                index: 2,
                view: DestKind::Fit
            }
        ));
        assert!(page.link(
            10.0,
            50.0,
            90.0,
            70.0,
            &Target::Uri("https://example.org/".to_string())
        ));
    });
    assert!(builder.set_outline(vec![
        OutlineEntry {
            title: "Open".to_string(),
            target: None,
            open: true,
            children: vec![OutlineEntry {
                title: "Leaf".to_string(),
                target: Some(Target::Page {
                    index: 1,
                    view: DestKind::Fit
                }),
                open: true,
                children: Vec::new(),
            }],
        },
        OutlineEntry {
            title: "Closed".to_string(),
            target: None,
            open: false,
            children: vec![OutlineEntry {
                title: "Hidden".to_string(),
                target: Some(Target::Page {
                    index: 2,
                    view: DestKind::Fit
                }),
                open: true,
                children: Vec::new(),
            }],
        },
    ]));

    let path = fixture("navigation", "nav.pdf", &builder.finish());
    let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);
    assert_eq!(code, 0, "qpdf --check:\n{text}");
    assert!(
        text.contains("No syntax or stream encoding errors found"),
        "qpdf found something:\n{text}"
    );

    // Read out of qpdf rather than out of the builder. Its `--json` resolves
    // the outline into a tree with the destinations already followed, which is
    // a stronger statement than the dictionary: it says the `/First`, `/Last`,
    // `/Next`, `/Prev` and `/Parent` chain is walkable by somebody else.
    let (code, dump) = run(&qpdf, &["--json=latest", &path.display().to_string()]);
    assert_eq!(code, 0, "qpdf --json:\n{dump}");
    for title in ["Open", "Leaf", "Closed", "Hidden"] {
        assert!(
            dump.contains(&format!("\"title\": \"{title}\"")),
            "qpdf reads the title {title}:\n{dump}"
        );
    }
    assert!(
        dump.contains("\"open\": true") && dump.contains("\"open\": false"),
        "and both view states, which is what the sign of /Count means:\n{dump}"
    );
    assert!(
        dump.contains("\"destpageposfrom1\": 2"),
        "the leaf's destination resolves to the second page:\n{dump}"
    );

    // And the counts themselves, out of the objects, because the tree above
    // reports `open` as a boolean and throws the magnitude away. 12.3.3: an
    // open entry states its visible descendants, a closed one states the
    // negative of them, and the root states every visible item in the whole
    // tree -- three here, since `Hidden` sits under a closed parent and is not
    // one of them.
    let mut counts: Vec<String> = Vec::new();
    for number in 1..40 {
        let (code, text) = run(
            &qpdf,
            &[
                &format!("--show-object={number}"),
                &path.display().to_string(),
            ],
        );
        if code == 0 && text.contains("/Title") || text.contains("/Type /Outlines") {
            if let Some(at) = text.find("/Count ") {
                let rest = &text[at + 7..];
                let value: String = rest
                    .chars()
                    .take_while(|c| *c == '-' || c.is_ascii_digit())
                    .collect();
                if !value.is_empty() {
                    counts.push(value);
                }
            }
        }
    }
    assert!(
        counts.contains(&"1".to_string()),
        "the open parent states one visible descendant: {counts:?}"
    );
    assert!(
        counts.contains(&"-1".to_string()),
        "the closed one states minus its own: {counts:?}"
    );
    assert!(
        counts.contains(&"3".to_string()),
        "and the root states every visible item: {counts:?}"
    );

    // Both link actions, which are two different dictionaries and not two
    // spellings of one (12.5.6.5).
    let (code, page) = run(&qpdf, &["--show-object=6", &path.display().to_string()]);
    assert_eq!(code, 0, "{page}");
    assert!(page.contains("/Annots"), "the page carries them:\n{page}");
    let (_, first) = run(&qpdf, &["--show-object=11", &path.display().to_string()]);
    let (_, second) = run(&qpdf, &["--show-object=12", &path.display().to_string()]);
    let both = format!("{first}{second}");
    assert!(both.contains("/Link"), "as /Link annotations:\n{both}");
    assert!(both.contains("/Dest"), "one by /Dest:\n{both}");
    assert!(both.contains("/URI"), "and one by a /URI action:\n{both}");
    assert!(
        both.contains("/Border [ 0 0 0 ]"),
        "with no visible border:\n{both}"
    );

    // 12.3.3's sibling chain runs **both ways**: `/Next` forward and `/Prev`
    // back. A reader that walks forward only -- which this repository's does,
    // and which is enough to build the tree -- cannot tell the back-links are
    // missing, so the injection matrix found deleting `/Prev` survived every
    // round trip. A viewer that walks up from a selected entry cannot.
    let (_, second_item) = run(&qpdf, &["--show-object=15", &path.display().to_string()]);
    assert!(
        second_item.contains("/Prev"),
        "the second top-level entry points back at the first:\n{second_item}"
    );
    let (_, first_item) = run(&qpdf, &["--show-object=14", &path.display().to_string()]);
    assert!(
        first_item.contains("/Next"),
        "and the first points forward at the second:\n{first_item}"
    );
    assert!(
        !first_item.contains("/Prev"),
        "the first has nothing before it:\n{first_item}"
    );
}
