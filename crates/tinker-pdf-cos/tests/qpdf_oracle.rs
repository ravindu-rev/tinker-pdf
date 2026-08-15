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

use tinker_pdf_cos::{
    CosDocument, DocumentBuilder, DocumentEditor, Encryption, WriteMode, WriteOptions,
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
