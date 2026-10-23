//! The ratchet of the line families: each line's verdict against the
//! committed baseline.
//!
//! A line family writes one TSV row a line through [`table`], as its probe
//! wrote it to a file, and prints its totals through `emit!`. A row carries a
//! verdict, `OK` or `NO` (the line read back as typed, or not), and for
//! `bct-auto.tsv` a second, `draws` or `undrawn` (ruling 14's reading draws
//! the line again in one direction, or in neither). The key of a row is every
//! column before its verdict.
//!
//! The baseline of a TSV is `tools/probes/rtl/baselines/<BASELINE>/lines/<name>.not-ok.tsv`:
//! plain text, because this crate has no decompressor and takes no dependency
//! for a test, holding the number of rows and every row that was not right,
//! by key and verdicts. [`family`] fails when a key has more rows `NO` (or
//! `undrawn`) than the baseline gives it, which is a line read right at the
//! baseline and wrong now, or when rows are missing. Gains are printed.
//!
//! The lines a family prints carry no verdict (`zz_fv_shape`'s `LINE`
//! readings, the totals); they are compared with the family's output in the
//! baseline, `lines/<family>.txt`, and a change is printed for a person to
//! read. `zz_fv_shape`'s own assertions are its verdict.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The tree the line families are held to.
pub(crate) const BASELINE: &str = "935b077";

/// How many of each kind a run prints.
const SHOWN: usize = 40;

thread_local! {
    static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static TABLES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// One line a family prints, as its probe printed it to stdout.
pub(crate) fn emit(line: String) {
    LINES.with(|lines| lines.borrow_mut().push(line));
}

/// A TSV a family writes, as its probe wrote it to a file.
pub(crate) fn table(name: &str, text: String) {
    TABLES.with(|tables| tables.borrow_mut().push((name.to_owned(), text)));
}

fn baselines() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/probes/rtl/baselines"
    ))
    .join(BASELINE)
    .join("lines")
}

/// One row's verdicts: read back as typed, and drawn again (`bct-auto.tsv` only).
fn verdicts(fields: &[&str]) -> Option<(usize, bool, Option<bool>)> {
    let at = fields.iter().position(|f| *f == "OK" || *f == "NO")?;
    let draws = fields[at + 1..].iter().find_map(|f| match *f {
        "draws" => Some(true),
        "undrawn" => Some(false),
        _ => None,
    });
    Some((at, fields[at] == "OK", draws))
}

/// Per key, how many rows are not read right and how many are not drawn; and
/// the number of rows.
#[derive(Debug, Default)]
struct Wrong {
    rows: usize,
    misread: BTreeMap<String, usize>,
    undrawn: BTreeMap<String, usize>,
}

impl Wrong {
    /// Counts one row's verdicts under its key; false when it carries none.
    fn tally(&mut self, fields: &[&str]) -> bool {
        let Some((at, ok, draws)) = verdicts(fields) else {
            return false;
        };
        let key = fields[..at].join("\t");
        if !ok {
            *self.misread.entry(key.clone()).or_default() += 1;
        }
        if draws == Some(false) {
            *self.undrawn.entry(key).or_default() += 1;
        }
        true
    }
}

/// Today's TSV, every row of which carries a verdict, as the baseline's must:
/// a row without one would count among the rows and never as wrong.
fn wrong_in_tsv(tsv: &str) -> Wrong {
    let mut out = Wrong::default();
    for line in tsv.lines() {
        out.rows += 1;
        assert!(
            out.tally(&line.split('\t').collect::<Vec<_>>()),
            "a row of today's TSV without a verdict: {line:?}"
        );
    }
    out
}

/// The `.not-ok.tsv` form of a TSV, as `scripts/pack.py` writes it: a `#`
/// line, `rows<TAB>N`, and each row not right as its key and verdicts.
fn wrong_in_baseline(text: &str) -> Wrong {
    let mut out = Wrong::default();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(n) = line.strip_prefix("rows\t") {
            out.rows = n.parse().expect("the baseline's row count");
            continue;
        }
        assert!(
            out.tally(&line.split('\t').collect::<Vec<_>>()),
            "a baseline row without a verdict: {line:?}"
        );
    }
    out
}

/// One TSV's rows against the baseline's: a key with more rows `NO`, or
/// `undrawn`, than at the baseline, or fewer rows, is worse; fewer is a gain.
fn compare_table(
    table_name: &str,
    was: &Wrong,
    is: &Wrong,
    worse: &mut Vec<String>,
    gains: &mut Vec<String>,
) {
    if is.rows < was.rows {
        worse.push(format!("{table_name}: rows {} -> {}", was.rows, is.rows));
    }
    for (what, a, b) in [
        ("misread", &was.misread, &is.misread),
        ("undrawn", &was.undrawn, &is.undrawn),
    ] {
        for (key, n) in b {
            let m = a.get(key).copied().unwrap_or(0);
            if *n > m {
                worse.push(format!("{table_name}: {what} {m} -> {n}: {}", short(key)));
            }
        }
        for (key, m) in a {
            let n = b.get(key).copied().unwrap_or(0);
            if n < *m {
                gains.push(format!("{table_name}: {what} {m} -> {n}: {}", short(key)));
            }
        }
    }
}

/// The data lines of a family's baseline output: what its probe printed, the
/// `#` note and the cargo lines left out.
fn printed_in_baseline(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_uppercase()) && !l.starts_with("Running "))
        .collect()
}

/// The printed lines that differ, each side named; they carry no verdict.
fn changed_lines(was: &[&str], is: &[&str]) -> Vec<String> {
    let mut changed = Vec::new();
    for line in is {
        if !was.contains(line) {
            changed.push(format!("now: {}", short(line)));
        }
    }
    for line in was {
        if !is.contains(line) {
            changed.push(format!("was: {}", short(line)));
        }
    }
    changed
}

fn short(key: &str) -> String {
    let flat = key.replace('\t', " | ");
    if flat.chars().count() > 160 {
        flat.chars().take(160).collect::<String>() + "..."
    } else {
        flat
    }
}

/// Runs one line family and holds its rows to the baseline.
///
/// # Panics
/// When a line reads worse than at [`BASELINE`], naming each.
pub(crate) fn family(name: &str, run: fn()) {
    LINES.with(|lines| lines.borrow_mut().clear());
    TABLES.with(|tables| tables.borrow_mut().clear());
    run();
    let printed: Vec<String> = LINES.with(|lines| std::mem::take(&mut *lines.borrow_mut()));
    let tables: Vec<(String, String)> = TABLES.with(|t| std::mem::take(&mut *t.borrow_mut()));

    let mut worse: Vec<String> = Vec::new();
    let mut gains: Vec<String> = Vec::new();
    let mut rows = 0;
    for (table_name, tsv) in &tables {
        let path = baselines().join(format!(
            "{}.not-ok.tsv",
            table_name.trim_end_matches(".tsv")
        ));
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (was, is) = (wrong_in_baseline(&text), wrong_in_tsv(tsv));
        rows += is.rows;
        compare_table(table_name, &was, &is, &mut worse, &mut gains);
    }
    let path = baselines().join(format!("{name}.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let was = printed_in_baseline(&text);
    let mut is: Vec<&str> = printed.iter().flat_map(|l| l.split('\n')).collect();
    is.sort_unstable();
    let changed = changed_lines(&was, &is);
    write_out(name, &is, &tables);

    println!(
        "RAN rtl_lines {name} against {BASELINE}: {rows} rows, {} printed lines; {} worse, {} gains, {} changed without a verdict",
        is.len(),
        worse.len(),
        gains.len(),
        changed.len()
    );
    for line in &is {
        println!("    {line}");
    }
    for (label, list) in [("WORSE", &worse), ("GAIN", &gains), ("CHANGED", &changed)] {
        for entry in list.iter().take(SHOWN) {
            println!("{label} {name} {entry}");
        }
        if list.len() > SHOWN {
            println!("{label} {name} ... and {} more", list.len() - SHOWN);
        }
    }
    assert!(
        worse.is_empty(),
        "{name}: {} keys read worse than at {BASELINE}:\n{}",
        worse.len(),
        worse
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// With `TINKER_RTL_KIT_OUT` set, the run's lines and TSVs go to
/// `<dir>/lines/`, the layout of a kit run, for the kit's comparators.
fn write_out(name: &str, lines: &[&str], tables: &[(String, String)]) {
    let Some(root) = std::env::var_os("TINKER_RTL_KIT_OUT") else {
        return;
    };
    let dir = PathBuf::from(root).join("lines");
    std::fs::create_dir_all(&dir).expect("the output directory");
    let mut text = format!("Running tests/{name}.rs\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("exit 0\n");
    std::fs::write(dir.join(format!("{name}.txt")), text).expect("a family's output");
    for (table_name, tsv) in tables {
        std::fs::write(dir.join(table_name), tsv).expect("a family's TSV");
    }
}

// ---- the ratchet's own checks ----------------------------------------------
//
// Not `#[ignore]`d: they read no `BidiCharacterTest.txt` line and take
// milliseconds, so every `cargo test` holds the comparison to what the line
// suite relies on.

/// The worse and the gains of `now` against `base`, both TSV texts.
fn compared(base: &str, now: &str) -> (Vec<String>, Vec<String>) {
    let (mut worse, mut gains) = (Vec::new(), Vec::new());
    compare_table(
        "t.tsv",
        &wrong_in_tsv(base),
        &wrong_in_tsv(now),
        &mut worse,
        &mut gains,
    );
    (worse, gains)
}

#[test]
fn a_line_read_right_at_the_baseline_and_wrong_now_reads_worse() {
    let base = "t0\tar\tw20\ttyped\tOK\nt0\tar\tw20\ttyped\tNO\nt1\the\tw33\tmirrored\tOK\n";
    // One more row wrong of a key that had one, and a key that had none.
    let now = "t0\tar\tw20\ttyped\tNO\nt0\tar\tw20\ttyped\tNO\nt1\the\tw33\tmirrored\tNO\n";
    let (worse, gains) = compared(base, now);
    assert_eq!(
        worse,
        [
            "t.tsv: misread 1 -> 2: t0 | ar | w20 | typed",
            "t.tsv: misread 0 -> 1: t1 | he | w33 | mirrored",
        ]
    );
    assert!(gains.is_empty(), "{gains:?}");
}

#[test]
fn a_line_drawn_at_the_baseline_and_undrawn_now_reads_worse() {
    // `bct-auto.tsv`'s second verdict, after the first.
    let base = "55\tOK\tdraws\n";
    let (worse, _) = compared(base, "55\tOK\tundrawn\n");
    assert_eq!(worse, ["t.tsv: undrawn 0 -> 1: 55"]);
    let (worse, _) = compared(base, "55\tNO\tdraws\n");
    assert_eq!(worse, ["t.tsv: misread 0 -> 1: 55"]);
}

#[test]
fn fewer_rows_read_worse_and_a_line_read_right_now_is_a_gain() {
    let base = "a\tOK\nb\tNO\nc\tOK\n";
    let (worse, gains) = compared(base, "a\tOK\nb\tOK\n");
    assert_eq!(worse, ["t.tsv: rows 3 -> 2"]);
    assert_eq!(gains, ["t.tsv: misread 1 -> 0: b"]);
}

#[test]
#[should_panic(expected = "a row of today's TSV without a verdict: \"b\\tmaybe\"")]
fn a_row_of_todays_tsv_without_a_verdict_is_refused_as_the_baselines_is() {
    // Counted among the rows and never as wrong, it would hide a line nobody read.
    wrong_in_tsv("a\tOK\nb\tmaybe\nc\tNO\n");
}

#[test]
#[should_panic(expected = "a baseline row without a verdict")]
fn a_row_of_the_baseline_without_a_verdict_is_refused() {
    wrong_in_baseline("# t.tsv at x\nrows\t3\nb\tmaybe\n");
}

#[test]
fn the_baselines_reduced_form_reads_as_the_tsv_it_came_from() {
    let tsv = "a\tx\tOK\tdraws\na\tx\tNO\tundrawn\nb\tNO\nc\tOK\n";
    // What `rtlkit.not_ok` writes for it.
    let reduced = "# t.tsv at x\nrows\t4\na\tx\tNO\tundrawn\nb\tNO\n";
    let (was, is) = (wrong_in_baseline(reduced), wrong_in_tsv(tsv));
    assert_eq!(
        (was.rows, &was.misread, &was.undrawn),
        (is.rows, &is.misread, &is.undrawn)
    );
    let (mut worse, mut gains) = (Vec::new(), Vec::new());
    compare_table("t.tsv", &was, &is, &mut worse, &mut gains);
    assert!(worse.is_empty() && gains.is_empty(), "{worse:?} {gains:?}");
}

#[test]
fn every_committed_baseline_parses_with_its_row_count() {
    for (name, rows) in [
        ("bct-auto", 91_616),
        ("wrapped", 2_235),
        ("sent", 13_300),
        ("laneR", 36_724),
    ] {
        let path = baselines().join(format!("{name}.not-ok.tsv"));
        let text = std::fs::read_to_string(&path).expect("the baseline");
        let wrong = wrong_in_baseline(&text);
        assert_eq!(wrong.rows, rows, "{name}");
        // Every row listed is wrong one way or both.
        let listed = text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("rows\t"))
            .count();
        let (misread, undrawn): (usize, usize) =
            (wrong.misread.values().sum(), wrong.undrawn.values().sum());
        assert!(
            listed > 0 && misread.max(undrawn) <= listed && listed <= misread + undrawn,
            "{name}: {misread} misread, {undrawn} undrawn, {listed} listed"
        );
    }
}

#[test]
fn a_printed_line_that_differs_is_a_change_and_not_worse() {
    let base = "# zz at x\nRunning tests/zz.rs\nSENT total=2 ok=1\nexit 0\n";
    let was = printed_in_baseline(base);
    assert_eq!(was, ["SENT total=2 ok=1"]);
    assert_eq!(
        changed_lines(&was, &["SENT total=2 ok=2"]),
        ["now: SENT total=2 ok=2", "was: SENT total=2 ok=1"]
    );
    assert!(changed_lines(&was, &was).is_empty());
}
