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
//! held once. The open case's child opens its file as `tpdf info` does
//! instead, through the CLI's own open (`tools/tpdf/src/source.rs`, included
//! by path): read on demand, and kept streamed only when it answers as a
//! buffered open of the same bytes would. It asserts that the open kept it
//! streamed and that it counts the pages the parent's buffered open of the
//! bytes counted, written beside the file;
//! [`the_open_case_reads_its_file_on_demand`] runs that case alone and holds
//! its peak below the file's length.
//!
//! Every case runs twice: as it is, and under a cache budget stated at open,
//! [`BUDGET`], its name then ending [`UNDER_BUDGET`]
//! (`docs/design/performance.md`, P-16, "MS-05's reading, and a stated
//! budget"). A budget bounds what a document keeps, not what an operation
//! needs to run, so a budgeted case holds its budget when its peak is at most
//! the peak of the case it extends, read in the same run, plus the budget:
//! what the case's caches held unasked moves under the budget, and nothing
//! they hold passes it ([`over_budget`]).
//!
//! The parent holds each case to its figure for this build in
//! `peak_memory.tsv` plus [`TOLERANCE_PERCENT`], the tolerance the corpus bars
//! give their peaks (`PEAK_TOLERANCE_PERCENT`, `xtask/src/ratchet.rs`), and
//! refuses a case the table has no line for. A line whose bytes read
//! [`UNHELD`] names a case measured and printed but held to no figure: the
//! owner's word (H-78) for a case whose runs spread past the tolerance, or
//! whose two runs have not been read, until they are. With
//! `TINKER_PEAK_RECORD` set it prints each case's line for the table instead
//! of holding it.
//!
//! The child is this test binary, built from the same revision, so this file
//! is a `SPAWNERS` row in `xtask/src/main.rs`: it hosts one case a process and
//! adjudicates nothing.
//!
//! The run is Linux-only and `#[ignore]`d: it renders at 300 dpi, merges four
//! hundred pages and reads ten thousand, each twice, which is a release
//! binary's work.
//! What it decides with the numbers is plain functions, tested on every
//! target.

mod written_support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};

/// The benchmark set's generators: the cases use a few of them.
#[allow(
    dead_code,
    reason = "the cases use a few of the benchmark set's generators"
)]
#[path = "../benches/support/inputs.rs"]
mod inputs;

/// The CLI's own open: the file source `tpdf` reads a file through and the
/// rule it keeps a streamed document by. The open case calls the open alone.
#[allow(
    dead_code,
    reason = "the open case calls the CLI's open and nothing else"
)]
#[path = "../../../tools/tpdf/src/source.rs"]
mod source;

/// How far over its committed figure a case may read before it fails.
const TOLERANCE_PERCENT: u64 = 2;

/// The committed figures: a build, a case and its bytes a line.
const TABLE: &str = include_str!("peak_memory.tsv");

/// What a line's bytes read for a case held to no figure.
const UNHELD: &str = "unheld";

/// The build this binary is, as the table names it.
const BUILD: &str = if cfg!(feature = "bundled-fonts") {
    "bundled-fonts"
} else {
    "default"
};

/// The builds the table may name.
const BUILDS: [&str; 2] = ["default", "bundled-fonts"];

/// The cases, in the order a run measures them.
///
/// `png-20-on-4-one-arena` is `png-20-on-4` again with the platform
/// allocator held to one arena besides ([`allocator`]), so the threaded
/// render is held both where its threads allocate from arenas of their own
/// and where they share one. The eight after the first eight are those eight
/// again, each under [`BUDGET`] stated at open ([`operation`]), so a case
/// and its budgeted twin are read in one run.
const CASES: [&str; 16] = [
    "png-300",
    "jpeg-300",
    "jpx-72",
    "png-20-on-4",
    "png-20-on-4-one-arena",
    "text-10000",
    "merge-4x100",
    OPEN,
    "png-300-under-64-mib",
    "jpeg-300-under-64-mib",
    "jpx-72-under-64-mib",
    "png-20-on-4-under-64-mib",
    "png-20-on-4-one-arena-under-64-mib",
    "text-10000-under-64-mib",
    "merge-4x100-under-64-mib",
    "open-23mb-under-64-mib",
];

/// The cache budget every budgeted case states at open: P-16's for MS-05,
/// one for every case, as MS-04's second render states it
/// (`docs/design/performance.md`, P-16, "MS-05's reading, and a stated
/// budget"). Room for what any case's renders compute — the largest,
/// `jpeg-300`'s A4 JPEG decoded at full size, 26 MB — so that none is drawn
/// from a reduced decode and each reads what keeping costs.
const BUDGET: u64 = 64 << 20;

/// What a budgeted case's name ends in: the case before it, run with
/// [`BUDGET`] stated at open.
const UNDER_BUDGET: &str = "-under-64-mib";

/// The operation `case` performs and the budget it states at open: the case
/// itself and none, or, for a case whose name ends [`UNDER_BUDGET`], the case
/// its name extends and [`BUDGET`].
fn operation(case: &str) -> (&str, Option<u64>) {
    match case.strip_suffix(UNDER_BUDGET) {
        Some(base) => (base, Some(BUDGET)),
        None => (case, None),
    }
}

/// What every document `case` opens is opened with: its budget stated at
/// open, where it states one, so that the document's parsed objects and
/// what its renders compute are charged to it together.
fn open_options(case: &str) -> tinker_pdf::OpenOptions {
    let options = tinker_pdf::OpenOptions::default();
    match operation(case).1 {
        Some(bytes) => options.with_cache_limits(tinker_pdf::CacheLimits::new(bytes)),
        None => options,
    }
}

/// What holding each budgeted case of `measured` to its budget finds: a line
/// for each whose peak passes the peak of the case it extends, read in the
/// same run, by more than the budget it states, and one for each whose case
/// was not measured beside it. Empty when every budgeted case holds.
fn over_budget(measured: &[(&str, u64)]) -> Vec<String> {
    measured
        .iter()
        .filter_map(|&(case, bytes)| {
            let (base, budget) = operation(case);
            let budget = budget?;
            let Some(&(_, unbudgeted)) = measured.iter().find(|(other, _)| *other == base) else {
                return Some(format!(
                    "{case}: {base} was not measured beside it, so its budget holds nothing"
                ));
            };
            (bytes > unbudgeted.saturating_add(budget)).then(|| {
                format!(
                    "{case}: a peak of {bytes} bytes, past {base}'s {unbudgeted} and the {budget} \
                     bytes the case states: a finding against what held them, never a larger \
                     budget"
                )
            })
        })
        .collect()
}

/// The case that opens the 23 MB image file as `tpdf info` opens it.
const OPEN: &str = "open-23mb";

/// The open case's file.
const OPEN_FILE: &str = "open-23mb.pdf";

/// What the parent writes beside the open case's file: the pages a buffered
/// open of its bytes counts, in decimal, which the child's count is held to.
const OPEN_PAGES: &str = "open-23mb.pages";

/// The suffix that runs a case's child under one allocator arena.
const ONE_ARENA: &str = "-one-arena";

/// What the names of the cases drawing their pages on [`WORKERS`] threads
/// begin with.
const THREADED: &str = "png-20-on-4";

/// The threads the threaded cases draw their pages on.
const WORKERS: u32 = 4;

/// What a child prints before its case and its peak.
const REPORT: &str = "peak-memory-case";

/// One committed figure: a case's peak in one build, in bytes, or `None` for
/// a case the table names [`UNHELD`].
#[derive(Debug, PartialEq, Eq)]
struct Figure {
    build: String,
    case: String,
    bytes: Option<u64>,
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
/// over; any other line that is not a build, a case and a count of bytes or
/// [`UNHELD`], tab-separated, is refused by its number.
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
            let bytes = match bytes.trim() {
                UNHELD => None,
                count => Some(count.parse().map_err(|_| {
                    format!(
                        "peak_memory.tsv line {}: `{bytes}` is no count of bytes",
                        at + 1
                    )
                })?),
            };
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
/// each case the table has no line for in `build`; a case it names
/// [`UNHELD`] is held to nothing. Empty when every case holds.
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
            let held = figure.bytes?;
            // The ratchet's comparison, in the same width.
            let allowed = u128::from(held) * u128::from(100 + TOLERANCE_PERCENT);
            (u128::from(bytes) * 100 > allowed).then(|| {
                format!(
                    "{case}: a peak of {bytes} bytes, above the {build} build's figure of \
                     {held} and its {TOLERANCE_PERCENT} % tolerance"
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

/// Where the test `run` writes its cases' inputs: a directory of its own,
/// under the one cargo gives every test target for its own files, so tests
/// the harness runs at once never write over the file another's child reads.
fn input_dir(run: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("peak-memory")
        .join(run);
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("the input directory {}: {error}", dir.display()));
    dir
}

/// `bytes` written to the file `name` in `dir`.
fn write_input(dir: &Path, name: &str, bytes: &[u8]) {
    let path = dir.join(name);
    std::fs::write(&path, bytes)
        .unwrap_or_else(|error| panic!("the input {}: {error}", path.display()));
}

/// `bytes` written as the open case's file in `dir`, and beside it the pages
/// a buffered open of them counts.
fn write_open_input(dir: &Path, bytes: Arc<[u8]>) {
    let pages = tinker_pdf::Document::open(Arc::clone(&bytes))
        .expect("the open case's file opens")
        .page_count();
    written_support::authored("the open case's file", Arc::clone(&bytes));
    write_input(dir, OPEN_FILE, &bytes);
    write_input(dir, OPEN_PAGES, pages.to_string().as_bytes());
}

/// The pages the parent wrote beside the open case's file in `dir`.
fn open_pages(dir: &Path) -> u32 {
    let path = dir.join(OPEN_PAGES);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the input {}: {error}", path.display()));
    text.trim()
        .parse()
        .unwrap_or_else(|error| panic!("the input {}: `{text}`, {error}", path.display()))
}

/// Writes `case`'s input files into `dir`, from the benchmark set's
/// generators: a budgeted case's are the case's it extends.
fn write_inputs(case: &str, dir: &Path) {
    let write = |name: &str, bytes: &[u8]| {
        written_support::authored(name, bytes);
        write_input(dir, name, bytes);
    };
    match operation(case).0 {
        "png-300" => write("png-300.pdf", &inputs::dense(1, 0)),
        "jpeg-300" => write("jpeg-300.pdf", &inputs::jpeg_page()),
        "jpx-72" => write("jpx-72.pdf", &inputs::jpx_page()),
        "png-20-on-4" | "png-20-on-4-one-arena" => {
            write("png-20-on-4.pdf", &inputs::dense(20, 0));
        }
        "text-10000" => write("text-10000.pdf", &inputs::small(10_000)),
        "merge-4x100" => {
            for seed in 0..4 {
                write(
                    &format!("merge-4x100-{seed}.pdf"),
                    &inputs::dense(100, seed),
                );
            }
            // The merge the child measures, made and held here, where no peak
            // is read: the save is the same bytes in either process, and
            // under a budget, which keeps what is read and writes nothing.
            written_support::rewritten(
                "the four parts merged",
                read_input(dir, "merge-4x100-0.pdf"),
                merged(dir, &open_options(case)),
            );
        }
        OPEN => write_open_input(dir, inputs::image_file()),
        "hold-64-mib" => {}
        other => panic!("no case is named {other}"),
    }
}

/// The platform allocator's settings `case`'s child runs under, as the
/// environment it is started with: the allocator reads them at the child's
/// start. A budgeted case runs under its case's.
///
/// The threaded cases hold glibc's mmap threshold at the 128 KiB it starts
/// at (`MALLOC_MMAP_THRESHOLD_`), which stops it rising to the size of the
/// largest mapped block a thread has freed. Left to rise, it rises with the
/// order the four threads free their pages' buffers, and it decides whether
/// the next buffer is mapped, and handed back to the kernel when it is freed,
/// or carved from a thread's arena, which may keep it after it is freed: the
/// peak then moves with which thread freed first. Held, every block of
/// 128 KiB or more is mapped and handed back when it is freed, and the peak
/// is what the waves ([`in_waves`]) hold alive at once.
/// `png-20-on-4-one-arena` holds the allocator to one arena besides
/// (`MALLOC_ARENA_MAX`). Every other case runs under the allocator as it
/// comes.
fn allocator(case: &str) -> Vec<(&'static str, &'static str)> {
    let case = operation(case).0;
    let mut settings = Vec::new();
    if case.starts_with(THREADED) {
        settings.push(("MALLOC_MMAP_THRESHOLD_", "131072"));
    }
    if case.ends_with(ONE_ARENA) {
        settings.push(("MALLOC_ARENA_MAX", "1"));
    }
    settings
}

/// One child: this binary re-executed to run `case` alone, and the peak it
/// reports.
fn run_child(case: &str, dir: &Path) -> u64 {
    let exe = std::env::current_exe().expect("the test binary's own path");
    let mut command = Command::new(&exe);
    command
        .args([
            "peak_memory_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TINKER_PEAK_CASE", case)
        .env("TINKER_PEAK_INPUT", dir)
        .envs(allocator(case));
    let output = command
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

/// Draws `pages` pages on `workers` scoped threads in waves: worker `k` draws
/// pages `k`, `k + workers`, … in turn, and every worker waits at each wave's
/// start until all of them have finished the wave before. The pages alive at
/// once are then one wave's, the same on every run whichever thread finishes
/// first, where threads that each took the next page as they freed would hold
/// a different mix of pages, and of each page's render and encode, from run
/// to run, and so a different peak.
fn in_waves(pages: u32, workers: u32, draw: impl Fn(u32, u32) + Sync) {
    let start = Barrier::new(workers as usize);
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let (start, draw) = (&start, &draw);
            scope.spawn(move || {
                for wave in 0..pages.div_ceil(workers) {
                    start.wait();
                    let index = wave * workers + worker;
                    if index < pages {
                        draw(worker, index);
                    }
                }
            });
        }
    });
}

/// The operation `case` names, on the input its parent wrote into `dir`,
/// every document it opens opened with the budget the case states, if any
/// ([`open_options`]).
fn perform(case: &str, dir: &Path) {
    use std::hint::black_box;
    use tinker_pdf::{Document, RenderOptions};

    let options = open_options(case);
    let case = operation(case).0;
    let open =
        |name: &str| Document::open_with(read_input(dir, name), &options).expect("the input opens");
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
        // process's own scoped threads in waves of four pages.
        "png-20-on-4" | "png-20-on-4-one-arena" => {
            let doc = open("png-20-on-4.pdf");
            let pages = doc.page_count();
            assert_eq!(pages, 20, "the input's pages");
            in_waves(pages, WORKERS, |_, index| {
                black_box(png(&doc, index, 150.0));
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
            black_box(merged(dir, &options));
        }
        // What `tpdf info` does to its file: the CLI's own open, which keeps
        // the document streamed only when it answers as a buffered open of the
        // same bytes would, and the pages counted, every read made.
        OPEN => {
            let path = dir.join(OPEN_FILE);
            let path = path.to_str().expect("the input's path in UTF-8");
            let (doc, kept) = source::open_on_demand_with(path, &options).expect("the input opens");
            assert!(doc.is_streamed(), "the CLI's open keeps the file streamed");
            let pages = doc.page_count();
            let file = kept.expect("the source a streamed document reads through");
            assert_eq!(file.failure(), None, "every read through the file was made");
            assert_eq!(pages, open_pages(dir), "the file's pages");
            black_box(pages);
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

/// Every case, a process each, held to this build's figures and each
/// budgeted case to its budget ([`over_budget`]) or, with the record switch
/// set, printed for the table.
#[test]
#[ignore = "sixteen processes, two rendering at 300 dpi and two reading ten thousand pages: a \
            release binary's run"]
fn peak_memory_of_every_case() {
    if !cfg!(target_os = "linux") {
        println!("SKIPPED: peak memory, read from /proc, which only Linux has");
        return;
    }
    let dir = input_dir("every-case");
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
    let mut problems = judge(&committed, BUILD, &measured);
    problems.extend(over_budget(&measured));
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
    let peak = run_child("hold-64-mib", &input_dir("hold-64-mib"));
    assert!(peak >= 64 << 20, "a peak of {peak} bytes");
}

/// MS-05's open case alone: its child opens the 23 MB image file as
/// `tpdf info` does, asserts that the CLI's open kept it streamed and counts
/// its pages, and holds less than the file at its peak, which a read of the
/// whole file could not.
#[test]
#[ignore = "starts a child process over the 23 MB image file, as the peak-memory run does"]
fn the_open_case_reads_its_file_on_demand() {
    if !cfg!(target_os = "linux") {
        println!("SKIPPED: peak memory, read from /proc, which only Linux has");
        return;
    }
    let dir = input_dir("open-case");
    write_inputs(OPEN, &dir);
    let length = std::fs::metadata(dir.join(OPEN_FILE))
        .expect("the input's length")
        .len();
    let peak = run_child(OPEN, &dir);
    println!("{}", line(BUILD, OPEN, peak));
    assert!(
        peak < length,
        "a peak of {peak} bytes, not below the {length}-byte file the child opened"
    );
    println!(
        "RAN: the open case in the {BUILD} build, {} pages counted through the CLI's own \
         open of a {length}-byte file kept streamed, at a peak of {peak} bytes",
        open_pages(&dir)
    );
}

/// The open case's operation, in this process and on every target, over a
/// small document written as its input: the CLI's open keeps the file
/// streamed and counts the pages the buffered open wrote beside it, with no
/// budget and under the budgeted case's, which the streamed open states.
#[test]
fn the_open_case_counts_a_file_it_keeps_streamed() {
    let dir = input_dir("open-case-in-process");
    write_open_input(&dir, inputs::small(3).into());
    assert_eq!(open_pages(&dir), 3, "the buffered open's count");
    perform(OPEN, &dir);
    let budgeted = format!("{OPEN}{UNDER_BUDGET}");
    perform(&budgeted, &dir);
    let path = dir.join(OPEN_FILE);
    let (doc, _) =
        source::open_on_demand_with(path.to_str().expect("UTF-8"), &open_options(&budgeted))
            .expect("the input opens");
    assert_eq!(
        doc.cache_limits(),
        Some(tinker_pdf::CacheLimits::new(BUDGET)),
        "the budgeted open states its budget"
    );
}

/// **A budgeted case is its case under the budget**: its name is the case's
/// and [`UNDER_BUDGET`], it performs the case's operation on the case's
/// input under [`BUDGET`] stated at open, and every case has its budgeted
/// twin in the run, so each is read beside the other.
#[test]
fn every_case_runs_again_under_its_budget() {
    assert_eq!(operation("png-300"), ("png-300", None));
    assert_eq!(
        operation("png-300-under-64-mib"),
        ("png-300", Some(64 << 20))
    );
    assert_eq!(
        operation("png-20-on-4-one-arena-under-64-mib"),
        ("png-20-on-4-one-arena", Some(BUDGET))
    );
    assert_eq!(open_options("jpeg-300").cache_limits, None);
    assert_eq!(
        open_options("jpeg-300-under-64-mib").cache_limits,
        Some(tinker_pdf::CacheLimits::new(BUDGET))
    );
    let (plain, budgeted): (Vec<&str>, Vec<&str>) =
        CASES.iter().partition(|case| operation(case).1.is_none());
    assert_eq!(plain.len(), budgeted.len(), "one budgeted twin a case");
    for case in &plain {
        assert!(
            budgeted.contains(&format!("{case}{UNDER_BUDGET}").as_str()),
            "{case} has no budgeted twin"
        );
    }
    for case in &budgeted {
        assert!(plain.contains(&operation(case).0), "{case} extends no case");
    }
}

/// **A budgeted case holds its case's peak plus its budget, and no more**:
/// a peak at exactly the case's own read in the run plus [`BUDGET`] holds, a
/// byte more fails that case alone, a peak below its case's holds, and a
/// budgeted case whose case the run did not read is named, since its budget
/// would hold nothing.
#[test]
fn a_budgeted_case_holds_its_case_plus_its_budget() {
    let measured = [
        ("png-300", 1_000),
        ("png-300-under-64-mib", 1_000 + BUDGET),
        ("jpx-72", 5_000),
        ("jpx-72-under-64-mib", 4_000),
    ];
    assert!(
        over_budget(&measured).is_empty(),
        "{:?}",
        over_budget(&measured)
    );
    let over = [
        ("png-300", 1_000),
        ("png-300-under-64-mib", 1_001 + BUDGET),
        ("jpx-72", 5_000),
        ("jpx-72-under-64-mib", 5_000),
    ];
    let problems = over_budget(&over);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with(&format!(
            "png-300-under-64-mib: a peak of {} bytes, past png-300's 1000",
            1_001 + BUDGET
        )),
        "{problems:?}"
    );
    let alone = [("text-10000-under-64-mib", 1)];
    let problems = over_budget(&alone);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("text-10000-under-64-mib: text-10000 was not measured"),
        "{problems:?}"
    );
    assert!(
        over_budget(&[("merge-4x100", u64::MAX)]).is_empty(),
        "a case states no budget to hold"
    );
}

/// The waves hold whichever thread finishes first: worker `k` draws pages
/// `k`, `k + 4`, …, every page once, the last wave short, and no page of a
/// wave starts until every page of the wave before has finished, though a
/// wave's last worker draws for the shortest time.
#[test]
fn the_threaded_cases_draw_their_pages_in_waves() {
    use std::sync::Mutex;
    use std::time::Duration;

    // A page, the worker drawing it and whether it starts, in the order the
    // workers come to them.
    let events = Mutex::new(Vec::new());
    in_waves(10, WORKERS, |worker, index| {
        events.lock().expect("the log").push((index, worker, true));
        std::thread::sleep(Duration::from_millis(u64::from(2 * (WORKERS - worker))));
        events.lock().expect("the log").push((index, worker, false));
    });
    let events = events.into_inner().expect("the log");
    let mut drawn: Vec<u32> = events
        .iter()
        .filter(|&&(_, _, starts)| starts)
        .map(|&(index, _, _)| index)
        .collect();
    drawn.sort_unstable();
    assert_eq!(drawn, (0..10).collect::<Vec<_>>(), "every page once");
    for &(index, worker, _) in &events {
        assert_eq!(
            worker,
            index % WORKERS,
            "page {index} drawn by worker {worker}"
        );
    }
    for (at, &(index, _, starts)) in events.iter().enumerate() {
        if !starts {
            continue;
        }
        let wave = index / WORKERS;
        let unfinished = events[at..]
            .iter()
            .find(|&&(other, _, other_starts)| !other_starts && other / WORKERS < wave);
        assert!(
            unfinished.is_none(),
            "page {index} of wave {wave} started before {unfinished:?} finished"
        );
    }
}

/// The threaded cases state the allocator they run under, the one-arena one
/// a single arena besides, and every other case runs under it as it comes.
#[test]
fn each_case_states_the_allocator_it_runs_under() {
    let threshold = ("MALLOC_MMAP_THRESHOLD_", "131072");
    assert_eq!(allocator("png-20-on-4"), [threshold]);
    assert_eq!(
        allocator("png-20-on-4-one-arena"),
        [threshold, ("MALLOC_ARENA_MAX", "1")]
    );
    for case in CASES.iter().filter(|case| !case.starts_with(THREADED)) {
        assert!(allocator(case).is_empty(), "{case}");
    }
    assert!(allocator("hold-64-mib").is_empty());
    // A budgeted case runs under its case's allocator.
    assert_eq!(allocator("png-20-on-4-under-64-mib"), [threshold]);
    assert_eq!(
        allocator("png-20-on-4-one-arena-under-64-mib"),
        [threshold, ("MALLOC_ARENA_MAX", "1")]
    );
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
        bytes: Some(bytes),
    }
}

/// A case the table names unheld is measured and held to nothing, in that
/// build only, and a line that names it is still a line: a case with none is
/// refused as before.
#[test]
fn a_case_named_unheld_is_held_to_nothing() {
    let table = "default\tpng-20-on-4\tunheld\ndefault\tpng-300\t1000\n";
    let committed = figures(table).expect("an unheld line reads");
    assert_eq!(committed[0].bytes, None);
    let problems = judge(
        &committed,
        "default",
        &[("png-20-on-4", u64::MAX), ("png-300", 1000)],
    );
    assert!(problems.is_empty(), "{problems:?}");
    let problems = judge(&committed, "bundled-fonts", &[("png-20-on-4", 1)]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("png-20-on-4: no figure for the bundled-fonts build"),
        "{problems:?}"
    );
}

/// The committed table reads and names every case in both builds, leaving
/// unheld only the threaded cases and the budgeted ones, whose first runs
/// have not been read.
#[test]
fn the_committed_table_names_every_case_in_both_builds() {
    let committed = figures(TABLE).unwrap_or_else(|error| panic!("{error}"));
    for build in BUILDS {
        for case in CASES {
            let line = committed
                .iter()
                .find(|figure| figure.build == build && figure.case == case)
                .unwrap_or_else(|| panic!("{case}: no line for the {build} build"));
            assert_eq!(
                line.bytes.is_none(),
                case.starts_with(THREADED) || operation(case).1.is_some(),
                "{case} in the {build} build"
            );
        }
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

/// `tpdf merge`'s shape, as the write group times it, over the four parts the
/// parent wrote into `dir`, each opened with `options`: the first file's
/// editor, every page of the others imported after its own, saved with
/// stream deduplication and the programs kept as they are.
fn merged(dir: &Path, options: &tinker_pdf::OpenOptions) -> Vec<u8> {
    use tinker_pdf::write::{self, FontPolicy, ImagePolicy, SaveOptions};
    use tinker_pdf::{Document, WriteMode, WriteOptions};

    let parts: Vec<Document> = (0..4)
        .map(|seed| {
            Document::open_with(read_input(dir, &format!("merge-4x100-{seed}.pdf")), options)
                .expect("the input opens")
        })
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
        ..SaveOptions::default()
    };
    write::save(&mut editor, &options).bytes
}
