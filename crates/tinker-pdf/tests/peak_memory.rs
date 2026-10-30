//! Peak memory per operation: each case's peak resident set, read from the
//! kernel and held to a committed figure.
//!
//! A process's peak belongs to the whole process, so every case runs in a
//! process of its own: this binary re-executes itself once a case, running
//! [`peak_memory_child`] alone, which performs that one operation on the input
//! the parent wrote to disk and prints the peak the kernel tracked since exec
//! (`/proc/self/status`'s `VmHWM`, read as `tpdf`'s `peak_bytes` reads it). The
//! parent writes every input first, from the benchmark set's own generators
//! (`benches/support/inputs.rs`, one source for both), so a child's peak holds
//! the operation and the bytes it reads, never the generator that made them.
//! A child reads its input once, into the buffer it opens, so the bytes are
//! held once.
//!
//! The parent holds each case to its figure for this build in
//! `peak_memory.tsv` plus [`TOLERANCE_PERCENT`], the tolerance the corpus bars
//! give their peaks (`PEAK_TOLERANCE_PERCENT`, `xtask/src/ratchet.rs`), and
//! refuses a case the table has no figure for. With `TINKER_PEAK_RECORD` set
//! it prints each case's line for the table instead of holding it.
//!
//! The child is this test binary, built from the same revision, so this file
//! is a `SPAWNERS` row in `xtask/src/main.rs`: it hosts one case a process and
//! adjudicates nothing.
//!
//! The run is Linux-only and `#[ignore]`d: it renders at 300 dpi, merges four
//! hundred pages and reads ten thousand, which is a release binary's work.
//! What it decides with the numbers is plain functions, tested on every
//! target.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// The benchmark set's generators: the cases use a few of them.
#[allow(
    dead_code,
    reason = "the cases use a few of the benchmark set's generators"
)]
#[path = "../benches/support/inputs.rs"]
mod inputs;

/// How far over its committed figure a case may read before it fails.
const TOLERANCE_PERCENT: u64 = 2;

/// The committed figures: a build, a case and its bytes a line.
const TABLE: &str = include_str!("peak_memory.tsv");

/// The build this binary is, as the table names it.
const BUILD: &str = if cfg!(feature = "bundled-fonts") {
    "bundled-fonts"
} else {
    "default"
};

/// The builds the table may name.
const BUILDS: [&str; 2] = ["default", "bundled-fonts"];

/// The cases, in the order a run measures them.
const CASES: [&str; 7] = [
    "png-300",
    "jpeg-300",
    "jpx-72",
    "png-20-on-4",
    "text-10000",
    "merge-4x100",
    "open-23mb",
];

/// What a child prints before its case and its peak.
const REPORT: &str = "peak-memory-case";

/// One committed figure: a case's peak in one build, in bytes.
#[derive(Debug, PartialEq, Eq)]
struct Figure {
    build: String,
    case: String,
    bytes: u64,
}

/// `VmHWM` out of a `/proc/<pid>/status` text, in bytes: the file writes `kB`
/// and means KiB.
fn vm_hwm(status: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let kib: u64 = line
            .strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        kib.checked_mul(1024)
    })
}

/// The committed figures. Lines starting with `#` and blank lines are passed
/// over; any other line that is not a build, a case and a count of bytes,
/// tab-separated, is refused by its number.
fn figures(table: &str) -> Result<Vec<Figure>, String> {
    table
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|(at, line)| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [build, case, bytes] = fields[..] else {
                return Err(format!(
                    "peak_memory.tsv line {}: {} fields, not a build, a case and bytes",
                    at + 1,
                    fields.len()
                ));
            };
            let bytes = bytes.trim().parse().map_err(|_| {
                format!(
                    "peak_memory.tsv line {}: `{bytes}` is no count of bytes",
                    at + 1
                )
            })?;
            Ok(Figure {
                build: build.to_string(),
                case: case.to_string(),
                bytes,
            })
        })
        .collect()
}

/// The table's line for a case's peak, as the record switch prints it.
fn line(build: &str, case: &str, bytes: u64) -> String {
    format!("{build}\t{case}\t{bytes}")
}

/// What holding `measured`, a run of `build`, to `figures` finds: a line for
/// each case above its figure and [`TOLERANCE_PERCENT`] over it, and one for
/// each case the table has no figure for in `build`. Empty when every case
/// holds.
fn judge(figures: &[Figure], build: &str, measured: &[(&str, u64)]) -> Vec<String> {
    measured
        .iter()
        .filter_map(|&(case, bytes)| {
            let Some(figure) = figures
                .iter()
                .find(|figure| figure.build == build && figure.case == case)
            else {
                return Some(format!(
                    "{case}: no figure for the {build} build; run with TINKER_PEAK_RECORD set \
                     and commit its line"
                ));
            };
            // The ratchet's comparison, in the same width.
            (u128::from(bytes) * 100
                > u128::from(figure.bytes) * u128::from(100 + TOLERANCE_PERCENT))
            .then(|| {
                format!(
                    "{case}: a peak of {bytes} bytes, above the {build} build's figure of {} \
                         and its {TOLERANCE_PERCENT} % tolerance",
                    figure.bytes
                )
            })
        })
        .collect()
}

/// The peak a child reported for `case` in its standard output. The report is
/// found wherever it sits on its line, since the harness may have begun that
/// line with the test's name.
fn reported(stdout: &str, case: &str) -> Option<u64> {
    let marker = format!("{REPORT} {case} ");
    let at = stdout.find(&marker)? + marker.len();
    let digits = &stdout[at..];
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().ok()
}

/// Where the parent writes the cases' inputs: under the directory cargo
/// gives every test target for its own files.
fn input_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("peak-memory");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("the input directory {}: {error}", dir.display()));
    dir
}

/// Writes `case`'s input files into `dir`, from the benchmark set's
/// generators.
fn write_inputs(case: &str, dir: &Path) {
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes)
            .unwrap_or_else(|error| panic!("the input {}: {error}", path.display()));
    };
    match case {
        "png-300" => write("png-300.pdf", &inputs::dense(1, 0)),
        "jpeg-300" => write("jpeg-300.pdf", &inputs::jpeg_page()),
        "jpx-72" => write("jpx-72.pdf", &inputs::jpx_page()),
        "png-20-on-4" => write("png-20-on-4.pdf", &inputs::dense(20, 0)),
        "text-10000" => write("text-10000.pdf", &inputs::small(10_000)),
        "merge-4x100" => {
            for seed in 0..4 {
                write(
                    &format!("merge-4x100-{seed}.pdf"),
                    &inputs::dense(100, seed),
                );
            }
        }
        "open-23mb" => write("open-23mb.pdf", &inputs::image_file()),
        "hold-64-mib" => {}
        other => panic!("no case is named {other}"),
    }
}

/// One child: this binary re-executed to run `case` alone, and the peak it
/// reports.
fn run_child(case: &str, dir: &Path) -> u64 {
    let exe = std::env::current_exe().expect("the test binary's own path");
    let output = Command::new(&exe)
        .args([
            "peak_memory_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TINKER_PEAK_CASE", case)
        .env("TINKER_PEAK_INPUT", dir)
        .output()
        .unwrap_or_else(|error| panic!("{} did not start: {error}", exe.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the {case} child failed with {}: {stdout}{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    reported(&stdout, case).unwrap_or_else(|| panic!("the {case} child reported no peak: {stdout}"))
}

/// The input file `name` in `dir`, read once into the buffer a document
/// opens: a zero-filled `Arc<[u8]>` of the file's length, collected in place,
/// so no second copy of the bytes is ever made.
fn read_input(dir: &Path, name: &str) -> Arc<[u8]> {
    let path = dir.join(name);
    let read = || -> std::io::Result<Arc<[u8]>> {
        let mut file = std::fs::File::open(&path)?;
        let length = usize::try_from(file.metadata()?.len()).expect("the input fits in memory");
        let mut bytes: Arc<[u8]> = std::iter::repeat_n(0, length).collect();
        file.read_exact(Arc::get_mut(&mut bytes).expect("the buffer's one handle"))?;
        Ok(bytes)
    };
    read().unwrap_or_else(|error| panic!("the input {}: {error}", path.display()))
}

/// The operation `case` names, on the input its parent wrote into `dir`.
fn perform(case: &str, dir: &Path) {
    use std::hint::black_box;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tinker_pdf::write::{self, FontPolicy, ImagePolicy, SaveOptions};
    use tinker_pdf::{Document, RenderOptions, WriteMode, WriteOptions};

    let open = |name: &str| Document::open(read_input(dir, name)).expect("the input opens");
    let png = |doc: &Document, index: u32, dpi: f64| {
        let page = doc.page(index).expect("the page");
        page.render(&RenderOptions::at_dpi(dpi))
            .to_png()
            .expect("the render encodes")
    };
    match case {
        "png-300" => {
            black_box(png(&open("png-300.pdf"), 0, 300.0));
        }
        "jpeg-300" => {
            let doc = open("jpeg-300.pdf");
            black_box(
                doc.page(0)
                    .expect("the page")
                    .render(&RenderOptions::at_dpi(300.0)),
            );
        }
        "jpx-72" => {
            let doc = open("jpx-72.pdf");
            black_box(
                doc.page(0)
                    .expect("the page")
                    .render(&RenderOptions::at_dpi(72.0)),
            );
        }
        // Tier P's '20 pages at 150 dpi to PNG' on four workers, run on this
        // process's own scoped threads, each taking the next page as it frees.
        "png-20-on-4" => {
            let doc = open("png-20-on-4.pdf");
            let pages = doc.page_count();
            assert_eq!(pages, 20, "the input's pages");
            let next = AtomicU32::new(0);
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    let (doc, next) = (&doc, &next);
                    scope.spawn(move || loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= pages {
                            return;
                        }
                        black_box(png(doc, index, 150.0));
                    });
                }
            });
        }
        "text-10000" => {
            let doc = open("text-10000.pdf");
            assert_eq!(doc.page_count(), 10_000, "the input's pages");
            for index in 0..doc.page_count() {
                black_box(doc.page(index).expect("the page").text().plain_text());
            }
        }
        // `tpdf merge`'s shape, as the write group times it: the first file's
        // editor, every page of the others imported after its own, saved with
        // stream deduplication and the programs kept as they are.
        "merge-4x100" => {
            let parts: Vec<Document> = (0..4)
                .map(|seed| open(&format!("merge-4x100-{seed}.pdf")))
                .collect();
            let mut editor = parts[0].editor();
            for part in &parts[1..] {
                for page in 0..part.page_count() {
                    let at = editor.page_refs().len() as u32;
                    editor
                        .import_page(part.cos(), page, at)
                        .expect("the page imports");
                }
            }
            let options = SaveOptions {
                write: WriteOptions {
                    mode: WriteMode::Rewrite,
                    deduplicate_streams: true,
                    ..WriteOptions::default()
                },
                fonts: FontPolicy::Keep,
                images: ImagePolicy::Keep,
            };
            black_box(write::save(&mut editor, &options).bytes);
        }
        "open-23mb" => {
            black_box(open("open-23mb.pdf").page_count());
        }
        "hold-64-mib" => {
            black_box(vec![1u8; 64 << 20]);
        }
        other => panic!("no case is named {other}"),
    }
}

/// The child's side: nothing unless a parent named a case, and then that
/// case performed and its peak printed.
#[test]
#[ignore = "a child of the peak-memory run, which starts it with a case to perform"]
fn peak_memory_child() {
    let Some(case) = std::env::var_os("TINKER_PEAK_CASE") else {
        return;
    };
    let case = case.to_string_lossy().into_owned();
    let dir = PathBuf::from(std::env::var_os("TINKER_PEAK_INPUT").expect("the input directory"));
    perform(&case, &dir);
    let status = std::fs::read_to_string("/proc/self/status").expect("the kernel's status file");
    let peak = vm_hwm(&status).expect("a VmHWM line");
    println!("{REPORT} {case} {peak}");
}

/// Every case, a process each, held to this build's figures or, with the
/// record switch set, printed for the table.
#[test]
#[ignore = "seven processes, one rendering at 300 dpi and one reading ten thousand pages: a \
            release binary's run"]
fn peak_memory_of_every_case() {
    if !cfg!(target_os = "linux") {
        println!("SKIPPED: peak memory, read from /proc, which only Linux has");
        return;
    }
    let dir = input_dir();
    let mut measured = Vec::new();
    for case in CASES {
        write_inputs(case, &dir);
        let bytes = run_child(case, &dir);
        println!("{}", line(BUILD, case, bytes));
        measured.push((case, bytes));
    }
    println!(
        "RAN: peak memory, {} cases a process each, in the {BUILD} build",
        measured.len()
    );
    if std::env::var_os("TINKER_PEAK_RECORD").is_some() {
        return;
    }
    let committed = figures(TABLE).unwrap_or_else(|error| panic!("{error}"));
    let problems = judge(&committed, BUILD, &measured);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The probe sees what a child holds: one that touches 64 MiB reports a peak
/// of at least that.
#[test]
#[ignore = "starts a child process, as the peak-memory run does"]
fn a_child_holding_64_mib_reports_at_least_that() {
    if !cfg!(target_os = "linux") {
        println!("SKIPPED: peak memory, read from /proc, which only Linux has");
        return;
    }
    let peak = run_child("hold-64-mib", &input_dir());
    assert!(peak >= 64 << 20, "a peak of {peak} bytes");
}

#[test]
fn the_status_line_is_read_in_kib() {
    let status =
        "Name:\tpeak_memory\nVmPeak:\t  999999 kB\nVmHWM:\t    2048 kB\nVmRSS:\t    1024 kB\n";
    assert_eq!(vm_hwm(status), Some(2048 * 1024));
    assert_eq!(vm_hwm("Name:\tpeak_memory\n"), None);
    assert_eq!(vm_hwm("VmHWM:\t  many kB\n"), None);
}

fn figure(build: &str, case: &str, bytes: u64) -> Figure {
    Figure {
        build: build.to_string(),
        case: case.to_string(),
        bytes,
    }
}

#[test]
fn a_case_over_its_band_fails_alone() {
    let committed = [
        figure("default", "png-300", 1000),
        figure("default", "jpx-72", 1000),
    ];
    // 1020 is the band's edge and holds; 1021 is over it.
    let problems = judge(
        &committed,
        "default",
        &[("png-300", 1020), ("jpx-72", 1021)],
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("jpx-72: a peak of 1021"),
        "{problems:?}"
    );
}

#[test]
fn a_case_with_no_figure_is_refused() {
    let committed = [
        figure("default", "png-300", 1000),
        figure("default", "open-23mb", 1000),
    ];
    let problems = judge(
        &committed,
        "default",
        &[("png-300", 900), ("text-10000", 1)],
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("text-10000: no figure for the default build"),
        "{problems:?}"
    );
    // Another build's figure is no figure for this one.
    let problems = judge(&committed, "bundled-fonts", &[("png-300", 900)]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("png-300: no figure for the bundled-fonts build"),
        "{problems:?}"
    );
}

#[test]
fn the_record_switch_prints_the_line_it_would_hold() {
    let printed = line("bundled-fonts", "merge-4x100", 123_456_789);
    assert_eq!(printed, "bundled-fonts\tmerge-4x100\t123456789");
    let table = format!("# build\tcase\tbytes\n\n{printed}\n");
    let read = figures(&table).expect("the printed line reads back");
    assert_eq!(read, [figure("bundled-fonts", "merge-4x100", 123_456_789)]);
    assert!(judge(&read, "bundled-fonts", &[("merge-4x100", 123_456_789)]).is_empty());
    assert!(
        figures("bundled-fonts merge-4x100 123\n").is_err(),
        "a line without tabs is refused"
    );
    assert!(
        figures("default\tpng-300\tmany\n").is_err(),
        "a line whose bytes are no count is refused"
    );
}

#[test]
fn a_report_is_found_after_the_harness_began_its_line() {
    let stdout = "\nrunning 1 test\ntest peak_memory_child ... peak-memory-case png-300 4096\nok\n";
    assert_eq!(reported(stdout, "png-300"), Some(4096));
    assert_eq!(reported(stdout, "jpx-72"), None);
    assert_eq!(reported("peak-memory-case png-300 \n", "png-300"), None);
}

#[test]
fn the_committed_table_names_only_builds_and_cases() {
    let committed = figures(TABLE).unwrap_or_else(|error| panic!("{error}"));
    for figure in &committed {
        assert!(
            BUILDS.contains(&figure.build.as_str()),
            "the table names the build {}, which is none",
            figure.build
        );
        assert!(
            CASES.contains(&figure.case.as_str()),
            "the table names the case {}, which is none",
            figure.case
        );
    }
}
