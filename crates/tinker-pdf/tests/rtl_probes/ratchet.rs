//! The ratchet: one family's books, read as the kit's comparators read them,
//! against the committed baseline of the same family in the same build.
//!
//! A family's module prints its lines through [`emit`] (the `emit!` macro)
//! exactly as its probe printed them to stdout, and hands any TSV to
//! [`table`]. [`family`] runs the module, parses what it printed the way
//! `tools/probes/rtl/scripts/rtlkit.py` parses a run, and fails if any book
//! reads worse than at [`BASELINE`] on any metric the line carries:
//!
//! - a book the baseline has and the run lacks, or fewer rows;
//! - `content`, `logical`, `cons`, read whole (`text`), found by search
//!   (`search=a/b` with `a == b`) or `holds` true at the baseline and not now;
//! - a glyph drawn over another more often (`overlap=a/b`'s `b`, `VOV`,
//!   `VOVX`, `TWROW`'s `vov`, `ZZN`), or the overflow font showing code 32 at a
//!   word spacing other than zero more often (`TWROW`'s `twbad`);
//! - a `FAIL` key of `zz_vx_probe_paren` the baseline did not have;
//! - more dir-less lines misread (`TALLY`), or more misread `LINE`s in one
//!   book;
//! - a fixture whose text or content-stream hashes moved (`FIX`), a book whose
//!   probe panicked (`PANIC`), or the td probe writing fewer rows.
//!
//! Gains, the same metrics the other way, are printed and never fail. Lines
//! that carry no verdict (the td probe's rows, `thash`, `chash`) are counted
//! and printed when they change, and never fail either: a change there is for
//! a person to read.
//!
//! The baselines are `xz` files, read by [`xz`] through the engine's own
//! LZMA2 decoder (ruling 13: no test spawns a program).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

/// The tree every family is held to: its outputs are
/// `tools/probes/rtl/baselines/<BASELINE>/<build>/<family>.txt.xz`.
pub(crate) const BASELINE: &str = "935b077";

/// The build this binary is, and so the baseline it reads.
pub(crate) const BUILD: &str = if cfg!(feature = "bundled-fonts") {
    "bundled"
} else {
    "default"
};

/// How many worse books, gains and changes a run prints of each kind.
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

/// The kit's directory.
pub(crate) fn kit() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/probes/rtl"
    ))
}

/// Runs one family's books and holds them to the baseline.
///
/// # Panics
/// When a book reads worse than at [`BASELINE`], naming each.
pub(crate) fn family(name: &str, run: fn()) {
    LINES.with(|lines| lines.borrow_mut().clear());
    TABLES.with(|tables| tables.borrow_mut().clear());
    run();
    let printed: Vec<String> = LINES.with(|lines| std::mem::take(&mut *lines.borrow_mut()));
    let tables: Vec<(String, String)> = TABLES.with(|t| std::mem::take(&mut *t.borrow_mut()));
    let data = data_lines(printed.iter().flat_map(|l| l.split('\n')));

    let dir = kit().join("baselines").join(BASELINE).join(BUILD);
    let base_text = read_baseline(&dir.join(format!("{name}.txt.xz")));
    let base = Output::parse(&data_lines(base_text.lines()));
    let now = Output::parse(&data);

    let mut report = Report::default();
    compare(&base, &now, &mut report);
    let (mut rows, mut base_rows) = (now.rows(), base.rows());
    for (table_name, text) in &tables {
        let base_tsv = read_baseline(&dir.join(format!("{table_name}.xz")));
        compare_tsv(table_name, &base_tsv, text, &mut report);
        rows += text.lines().count();
        base_rows += base_tsv.lines().count();
    }
    write_out(name, &data, &tables);

    println!(
        "RAN rtl_probes {name} [{BUILD}] against {BASELINE}: {rows} rows (the baseline has {base_rows}), \
         {} books worse, {} books better, {} changed without a verdict",
        report.worse.len(),
        report.gains.len(),
        report.changed.len()
    );
    report.print(name);
    assert!(
        report.worse.is_empty(),
        "{name} [{BUILD}]: {} books read worse than at {BASELINE}:\n{}",
        report.worse.len(),
        Report::lines(&report.worse, SHOWN).join("\n")
    );
}

/// What the kit's normaliser keeps of a run: the data lines, sorted.
///
/// A data line is a probe's upper-case tag followed by a tab or a space
/// (`ROW`, `VOVX`, `TWROW`, ...), or one of its own summaries (`rows=`,
/// `books=`, `twbooks=`, `opened=`, `fails=`), as `rtlkit.DATA` and
/// `rtlkit.SUMMARY` define them.
pub(crate) fn data_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = lines.filter(|l| is_data(l)).map(str::to_owned).collect();
    out.sort();
    out
}

fn is_data(line: &str) -> bool {
    let bytes = line.as_bytes();
    let tagged = bytes.first().is_some_and(u8::is_ascii_uppercase) && {
        let tail = bytes.iter().skip(1).position(|b| {
            !(b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
        });
        matches!(tail, Some(at) if at >= 1 && matches!(bytes[at + 1], b'\t' | b' '))
    };
    let summary = ["rows=", "books=", "twbooks=", "opened=", "fails="]
        .iter()
        .any(|p| {
            line.strip_prefix(p)
                .is_some_and(|rest| rest.chars().next().is_some_and(|c| !c.is_whitespace()))
        });
    tagged || summary
}

fn read_baseline(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let text = xz(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    String::from_utf8(text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// With `TINKER_RTL_KIT_OUT` set to a directory, a run writes what it printed
/// there in the layout of a kit run (`<dir>/<build>/<family>.txt`, and its
/// TSVs beside it), so that `cmp.py` and `table.py` read it as they read
/// `run.sh`'s output. It changes nothing a run asserts.
fn write_out(name: &str, data: &[String], tables: &[(String, String)]) {
    let Some(root) = std::env::var_os("TINKER_RTL_KIT_OUT") else {
        return;
    };
    let dir = PathBuf::from(root).join(BUILD);
    std::fs::create_dir_all(&dir).expect("the output directory");
    let mut text = format!("Running tests/{name}.rs\n");
    for line in data {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("exit 0\n");
    std::fs::write(dir.join(format!("{name}.txt")), text).expect("a family's output");
    for (table_name, tsv) in tables {
        std::fs::write(dir.join(table_name), tsv).expect("a family's TSV");
    }
}

/// The metrics of one book, each true, false or not carried.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Metrics {
    content: Option<bool>,
    logical: Option<bool>,
    cons: Option<bool>,
    text: Option<bool>,
    search: Option<bool>,
    holds: Option<bool>,
}

impl Metrics {
    fn each(&self) -> [(&'static str, Option<bool>); 6] {
        [
            ("content", self.content),
            ("logical", self.logical),
            ("cons", self.cons),
            ("text", self.text),
            ("search", self.search),
            ("holds", self.holds),
        ]
    }
}

/// One family's output, parsed as `rtlkit.Output` parses it.
#[derive(Debug, Default)]
pub(crate) struct Output {
    /// Lines that are a book or a fixture (`ROW`, `BOOK`, `FIX`, `TWROW`, `ZZN`).
    rows_seen: usize,
    rows: BTreeMap<String, Metrics>,
    /// Glyphs drawn over another: the rr3m lines' `overlap=a/b`'s `b`, `TWROW`'s `vov`.
    over: BTreeMap<String, u64>,
    /// `TWROW`'s `twbad`: code-32 strings of the overflow font shown at a word spacing other than zero.
    twbad: BTreeMap<String, u64>,
    /// The rr3m lines' `thash` and `chash`.
    hashes: BTreeMap<String, String>,
    vovx: BTreeMap<String, u64>,
    vov: BTreeMap<String, (u64, String)>,
    paren: BTreeSet<String>,
    paren_rows: Option<u64>,
    fix: BTreeMap<String, String>,
    zzn: BTreeMap<String, u64>,
    /// `LINE` lines per book (the first three fields after the tag), and every `LINE` line.
    lines_per_book: BTreeMap<String, u64>,
    lines: BTreeSet<String>,
    tally: BTreeMap<String, u64>,
    zz: BTreeMap<String, u64>,
    panics: BTreeSet<String>,
}

fn flag(value: &str) -> Option<bool> {
    match value {
        "0" | "false" => Some(false),
        "1" | "true" => Some(true),
        _ => None,
    }
}

fn number(value: &str) -> Option<u64> {
    value.parse().ok()
}

impl Output {
    pub(crate) fn parse(data: &[String]) -> Self {
        let mut out = Self::default();
        for line in data {
            out.line(line);
        }
        out
    }

    fn line(&mut self, line: &str) {
        let p: Vec<&str> = line.split('\t').collect();
        let tag = p[0].split(' ').next().unwrap_or("");
        match tag {
            "ROW" => {
                self.rows_seen += 1;
                let mut m = Metrics::default();
                let key;
                if p.len() > 3 && p[3].starts_with("m=") {
                    key = format!("{}\t{}", p[1], p[2]);
                    for f in &p[3..] {
                        let Some((name, value)) = f.split_once('=') else {
                            continue;
                        };
                        match name {
                            "text" if value.len() == 1 => m.text = flag(value),
                            "cons" if value.len() == 1 => m.cons = flag(value),
                            "logical" if value.len() == 1 => m.logical = flag(value),
                            "search" => {
                                if let Some((a, b)) = value.split_once('/') {
                                    if number(a).is_some() && number(b).is_some() {
                                        m.search = Some(a == b);
                                    }
                                }
                            }
                            "overlap" => {
                                if let Some(b) = value.split_once('/').and_then(|(_, b)| number(b))
                                {
                                    self.over.insert(key.clone(), b);
                                }
                            }
                            "thash" | "chash" => {
                                let entry = self.hashes.entry(key.clone()).or_default();
                                let _ = write!(entry, "{name}={value} ");
                            }
                            _ => {}
                        }
                    }
                } else {
                    let mut parts = Vec::new();
                    let mut any = false;
                    for f in &p[1..] {
                        let metric = f.split_once('=').filter(|(n, v)| {
                            matches!(*n, "content" | "logical" | "search")
                                && !v.is_empty()
                                && v.chars().all(|c| c.is_alphanumeric() || c == '_')
                        });
                        match metric {
                            Some((n, v)) => {
                                any = true;
                                let value = Some(v == "true");
                                match n {
                                    "content" => m.content = value,
                                    "logical" => m.logical = value,
                                    _ => m.search = value,
                                }
                            }
                            None if !any => parts.push(*f),
                            None => {}
                        }
                    }
                    key = parts.join("\t");
                }
                self.rows.insert(key, m);
            }
            "BOOK" => {
                self.rows_seen += 1;
                let key = p.get(1..4).map(|k| k.join("\t")).unwrap_or_default();
                let m = Metrics {
                    holds: Some(line.contains("holds=true")),
                    ..Metrics::default()
                };
                self.rows.insert(key, m);
            }
            "FAIL" => {
                let key: Vec<&str> = p
                    .iter()
                    .copied()
                    .filter(|x| {
                        !(x.starts_with("content=")
                            || x.starts_with("logical=")
                            || x.starts_with('['))
                    })
                    .collect();
                self.paren.insert(key.join("\t"));
            }
            "FIX" => {
                self.rows_seen += 1;
                self.fix.insert(
                    p.get(1).copied().unwrap_or("").to_owned(),
                    p.get(2..).map(|r| r.join("\t")).unwrap_or_default(),
                );
            }
            "TWROW" => {
                self.rows_seen += 1;
                let key = format!("{}\t{}", p.get(1).unwrap_or(&""), p.get(2).unwrap_or(&""));
                let mut m = Metrics::default();
                let mut vov = 0;
                for f in p.iter().skip(3) {
                    let Some((name, value)) = f.split_once('=') else {
                        continue;
                    };
                    let Some(n) = number(value) else { continue };
                    match name {
                        "cons" => m.cons = Some(n == 1),
                        "logical" => m.logical = Some(n == 1),
                        "vov" => vov = n,
                        "twbad" => {
                            self.twbad.insert(key.clone(), n);
                        }
                        _ => {}
                    }
                }
                self.over.insert(key.clone(), vov);
                self.rows.insert(key, m);
            }
            "VOVX" => {
                let key = p.get(1..4).map(|k| k.join("\t")).unwrap_or_default();
                self.vovx
                    .insert(key, p.get(4).and_then(|n| number(n)).unwrap_or(0));
            }
            "ZZN" => {
                self.rows_seen += 1;
                let count = line
                    .split_once("overprinted ")
                    .and_then(|(_, rest)| {
                        number(
                            rest.split(|c: char| !c.is_ascii_digit())
                                .next()
                                .unwrap_or(""),
                        )
                    })
                    .unwrap_or(0);
                self.zzn
                    .insert(format!("{}\t{}", p[0], p.get(1).unwrap_or(&"")), count);
            }
            "LINE" => {
                let book = p.get(1..4).map(|k| k.join("\t")).unwrap_or_default();
                *self.lines_per_book.entry(book).or_default() += 1;
                self.lines.insert(line.to_owned());
            }
            "VOV" => {
                let key = format!("{}\t{}", p.get(1).unwrap_or(&""), p.get(2).unwrap_or(&""));
                let n = p.get(3).and_then(|n| number(n)).unwrap_or(0);
                self.vov
                    .insert(key, (n, p.get(4).copied().unwrap_or("").to_owned()));
            }
            "TALLY" => {
                if p.len() >= 4 {
                    if let Some(n) = number(p[p.len() - 1]) {
                        self.tally.insert(p[1..p.len() - 1].join("\t"), n);
                    }
                }
            }
            "ZZ" => {
                let words: Vec<&str> = line.split_whitespace().collect();
                if words.len() >= 3 {
                    if let Some(n) = number(words[words.len() - 1]) {
                        self.zz.insert(words[1..words.len() - 1].join(" "), n);
                    }
                }
            }
            "PANIC" => {
                self.panics.insert(p[1..].join("\t"));
            }
            _ => {
                if let Some(rest) = line.strip_prefix("rows=") {
                    if line.contains("fails=") {
                        self.paren_rows = number(rest.split(' ').next().unwrap_or(""));
                    }
                }
            }
        }
    }

    /// The rows a family's banner counts: its books and fixtures, and the
    /// rows `zz_vx_probe_paren` counts itself (`rows=N fails=M`), whose
    /// lines are its `FAIL`s and `VOVX`s only.
    fn rows(&self) -> usize {
        self.rows_seen
            + self
                .paren_rows
                .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// `TALLY`'s lines read wrong, every gloss and kind together: the lines
    /// read right are no measure on their own, since a change of line
    /// breaking changes how many lines there are.
    fn misread(&self) -> u64 {
        self.tally
            .iter()
            .filter(|(k, _)| !k.ends_with("\tright"))
            .map(|(_, n)| n)
            .sum()
    }
}

/// What a comparison found: per book, what it lost (`worse`) or gained
/// (`gains`), and the changes that carry no verdict.
#[derive(Debug, Default)]
pub(crate) struct Report {
    pub(crate) worse: BTreeMap<String, Vec<String>>,
    pub(crate) gains: BTreeMap<String, Vec<String>>,
    pub(crate) changed: Vec<String>,
}

impl Report {
    fn worse(&mut self, book: &str, what: String) {
        self.worse.entry(book.to_owned()).or_default().push(what);
    }

    fn gain(&mut self, book: &str, what: String) {
        self.gains.entry(book.to_owned()).or_default().push(what);
    }

    /// Up to `n` entries of a book list, one a line.
    fn lines(list: &BTreeMap<String, Vec<String>>, n: usize) -> Vec<String> {
        list.iter()
            .take(n)
            .map(|(book, what)| format!("{}: {}", short(book), what.join("; ")))
            .collect()
    }

    fn print(&self, name: &str) {
        for (label, list) in [("WORSE", &self.worse), ("GAIN", &self.gains)] {
            for line in Self::lines(list, SHOWN) {
                println!("{label} {name} [{BUILD}] {line}");
            }
            if list.len() > SHOWN {
                println!(
                    "{label} {name} [{BUILD}] ... and {} more",
                    list.len() - SHOWN
                );
            }
        }
        for entry in self.changed.iter().take(SHOWN) {
            println!("CHANGED {name} [{BUILD}] {entry}");
        }
        if self.changed.len() > SHOWN {
            println!(
                "CHANGED {name} [{BUILD}] ... and {} more",
                self.changed.len() - SHOWN
            );
        }
    }
}

/// A book's key, one line, cut to a length a log can show.
fn short(key: &str) -> String {
    let flat = key.replace('\t', " | ");
    if flat.chars().count() > 160 {
        flat.chars().take(160).collect::<String>() + "..."
    } else {
        flat
    }
}

/// The family itself, for what is not one book's (rows missing, totals).
const FAMILY: &str = "(the family)";

/// The book-by-book comparison of two outputs of one family, base to now.
pub(crate) fn compare(base: &Output, now: &Output, report: &mut Report) {
    if now.rows_seen < base.rows_seen {
        report.worse(
            FAMILY,
            format!("rows {} -> {}", base.rows_seen, now.rows_seen),
        );
    }
    for (key, was) in &base.rows {
        let Some(is) = now.rows.get(key) else {
            report.worse(key, "missing".to_owned());
            continue;
        };
        let mut lost = Vec::new();
        let mut won = Vec::new();
        for ((metric, a), (_, b)) in was.each().iter().zip(is.each().iter()) {
            if *a == Some(true) && *b != Some(true) {
                lost.push(*metric);
            }
            if *a == Some(false) && *b == Some(true) {
                won.push(*metric);
            }
        }
        if !lost.is_empty() {
            report.worse(key, format!("lost {}", lost.join(",")));
        }
        if !won.is_empty() {
            report.gain(key, format!("gained {}", won.join(",")));
        }
    }
    for (what, a, b) in [
        ("overprint", &base.over, &now.over),
        ("code 32 at a Tw other than 0", &base.twbad, &now.twbad),
    ] {
        for (key, was) in a {
            match b.get(key) {
                Some(is) if is > was => report.worse(key, format!("{what} {was} -> {is}")),
                Some(is) if is < was => report.gain(key, format!("{what} {was} -> {is}")),
                _ => {}
            }
        }
    }
    for (key, (is, at)) in &now.vov {
        let was = base.vov.get(key).map_or(0, |v| v.0);
        if *is > was && base.rows.contains_key(key) {
            report.worse(key, format!("VOV {was} -> {is} at {at}"));
        }
    }
    for (key, (was, _)) in &base.vov {
        let is = now.vov.get(key).map_or(0, |v| v.0);
        if is < *was && now.rows.contains_key(key) {
            report.gain(key, format!("VOV {was} -> {is}"));
        }
    }
    for (key, was) in &base.vovx {
        match now.vovx.get(key) {
            None => report.worse(&format!("VOVX {key}"), "missing".to_owned()),
            Some(is) if is > was => {
                report.worse(&format!("VOVX {key}"), format!("overprint {was} -> {is}"))
            }
            Some(is) if is < was => {
                report.gain(&format!("VOVX {key}"), format!("overprint {was} -> {is}"))
            }
            _ => {}
        }
    }
    for key in now.paren.difference(&base.paren) {
        report.worse(key, "a FAIL line new".to_owned());
    }
    for key in base.paren.difference(&now.paren) {
        report.gain(key, "its FAIL line gone".to_owned());
    }
    if let Some(was) = base.paren_rows {
        let is = now.paren_rows.unwrap_or(0);
        if is < was {
            report.worse(FAMILY, format!("probe_paren rows {was} -> {is}"));
        }
    }
    for (key, was) in &base.fix {
        match now.fix.get(key) {
            None => report.worse(key, "fixture missing".to_owned()),
            Some(is) if is != was => {
                report.worse(key, format!("fixture's hashes moved: {was} -> {is}"))
            }
            _ => {}
        }
    }
    for (key, was) in &base.zzn {
        match now.zzn.get(key) {
            None => report.worse(key, "missing".to_owned()),
            Some(is) if is > was => report.worse(key, format!("overprint {was} -> {is}")),
            Some(is) if is < was => report.gain(key, format!("overprint {was} -> {is}")),
            _ => {}
        }
    }
    if !base.tally.is_empty() || !now.tally.is_empty() {
        let (was, is) = (base.misread(), now.misread());
        if is > was {
            report.worse(FAMILY, format!("TALLY lines misread {was} -> {is}"));
        } else if is < was {
            report.gain(FAMILY, format!("TALLY lines misread {was} -> {is}"));
        }
    }
    let books: BTreeSet<&String> = base
        .lines_per_book
        .keys()
        .chain(now.lines_per_book.keys())
        .collect();
    for book in books {
        let was = base.lines_per_book.get(book).copied().unwrap_or(0);
        let is = now.lines_per_book.get(book).copied().unwrap_or(0);
        if is > was {
            report.worse(book, format!("LINE lines misread {was} -> {is}"));
        } else if is < was {
            report.gain(book, format!("LINE lines misread {was} -> {is}"));
        }
    }
    for line in now.lines.symmetric_difference(&base.lines) {
        let side = if now.lines.contains(line) {
            "now"
        } else {
            "was"
        };
        report.changed.push(format!("LINE {side}: {}", short(line)));
    }
    for (key, was) in &base.zz {
        let is = now.zz.get(key).copied().unwrap_or(0);
        if is < *was {
            report.worse(FAMILY, format!("ZZ {key} {was} -> {is}"));
        }
    }
    for key in now.panics.difference(&base.panics) {
        report.worse(key, "its probe panicked".to_owned());
    }
    for (key, was) in &base.hashes {
        if let Some(is) = now.hashes.get(key) {
            if is != was {
                report
                    .changed
                    .push(format!("hashes {was}-> {is}: {}", short(key)));
            }
        }
    }
}

/// A TSV a family wrote against the baseline's: the td probe's rows carry no
/// verdict, so a row that differs is a change for a person to read, and a row
/// the baseline has and the run lacks is a book missing.
fn compare_tsv(name: &str, base: &str, now: &str, report: &mut Report) {
    let rows = |text: &str| -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for line in text.lines() {
            let (key, rest) = line.split_once('\t').unwrap_or((line, ""));
            let mut k = key.to_owned();
            let mut n = 1;
            while out.contains_key(&k) {
                n += 1;
                k = format!("{key}#{n}");
            }
            out.insert(k, rest.to_owned());
        }
        out
    };
    let (was, is) = (rows(base), rows(now));
    for (key, row) in &was {
        match is.get(key) {
            None => report.worse(&format!("{name}: {key}"), "row missing".to_owned()),
            Some(other) if other != row => {
                report
                    .changed
                    .push(format!("{name}: {key}: {} -> {}", short(row), short(other)))
            }
            _ => {}
        }
    }
}

// ---- xz ----------------------------------------------------------------------

/// The CRC-64 of ECMA-182, reflected, as `xz` checks a block with it.
fn crc64(data: &[u8]) -> u64 {
    const POLY: u64 = 0xC96C_5795_D787_0F42;
    let mut table = [0u64; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u64;
        for _ in 0..8 {
            c = if c & 1 == 1 { (c >> 1) ^ POLY } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = !0u64;
    for b in data {
        crc = table[((crc ^ u64::from(*b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

fn le32(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "truncated".to_owned())
}

fn varint(data: &[u8], at: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..63).step_by(7) {
        let b = *data.get(*at).ok_or("truncated varint")?;
        *at += 1;
        value |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("varint too long".to_owned())
}

fn size(value: u64) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| "size past usize".to_owned())
}

/// One `.xz` file (a single stream of LZMA2 blocks, as `xz -9` and Python's
/// `lzma` write them), decoded by `tinker-pdf-archive`'s LZMA2 decoder, with
/// every header's CRC-32 and each block's CRC-64 or CRC-32 checked.
///
/// # Errors
/// Anything outside that shape, or a check that does not hold.
pub(crate) fn xz(data: &[u8]) -> Result<Vec<u8>, String> {
    const MAGIC: [u8; 6] = [0xFD, b'7', b'z', b'X', b'Z', 0];
    if data.len() < 32 || data[..6] != MAGIC || data[data.len() - 2..] != *b"YZ" {
        return Err("not an xz stream".to_owned());
    }
    let flags = [data[6], data[7]];
    if tinker_pdf_filters::crc32(&flags) != le32(data, 8)? || flags[0] != 0 {
        return Err("bad stream header".to_owned());
    }
    let check = flags[1];
    let check_size = match check {
        0 => 0,
        1 => 4,
        4 => 8,
        // SHA-256 (0x0A) and the reserved types: a check this reader does
        // not compute is refused, not taken on trust.
        _ => return Err(format!("check type {check}")),
    };
    let footer = data.len() - 12;
    if tinker_pdf_filters::crc32(&data[footer + 4..footer + 10]) != le32(data, footer)?
        || data[footer + 8..footer + 10] != flags
    {
        return Err("bad stream footer".to_owned());
    }
    let backward = (size(u64::from(le32(data, footer + 4)?))? + 1) * 4;
    let index = footer
        .checked_sub(backward)
        .ok_or("index before the start")?;
    if data[index] != 0
        || tinker_pdf_filters::crc32(&data[index..footer - 4]) != le32(data, footer - 4)?
    {
        return Err("bad index".to_owned());
    }
    let mut at = index + 1;
    let records = varint(data, &mut at)?;
    let mut blocks = Vec::new();
    for _ in 0..records {
        let unpadded = size(varint(data, &mut at)?)?;
        let unpacked = size(varint(data, &mut at)?)?;
        blocks.push((unpadded, unpacked));
    }
    let mut out = Vec::new();
    let mut pos = 12;
    for (unpadded, unpacked) in blocks {
        let header_size = (usize::from(*data.get(pos).ok_or("truncated")?) + 1) * 4;
        let header = data
            .get(pos..pos + header_size)
            .ok_or("truncated block header")?;
        if tinker_pdf_filters::crc32(&header[..header_size - 4]) != le32(header, header_size - 4)? {
            return Err("bad block header".to_owned());
        }
        let block_flags = header[1];
        if block_flags & 0x3C != 0 || block_flags & 0x03 != 0 {
            return Err("a block with more than one filter".to_owned());
        }
        let mut h = 2;
        if block_flags & 0x40 != 0 {
            varint(header, &mut h)?;
        }
        if block_flags & 0x80 != 0 {
            varint(header, &mut h)?;
        }
        if varint(header, &mut h)? != 0x21 || varint(header, &mut h)? != 1 {
            return Err("a filter other than LZMA2".to_owned());
        }
        let packed = unpadded
            .checked_sub(header_size + check_size)
            .ok_or("bad unpadded size")?;
        let start = pos + header_size;
        let stream = data.get(start..start + packed).ok_or("truncated block")?;
        let limits = tinker_pdf_archive::lzma::Limits {
            max_unpacked: unpacked,
        };
        let block = tinker_pdf_archive::lzma::decode_lzma2(stream, unpacked, &limits)
            .map_err(|e| format!("{e:?}"))?;
        let check_at = (start + packed).div_ceil(4) * 4;
        let field = data
            .get(check_at..check_at + check_size)
            .ok_or("truncated check")?;
        let holds = match check {
            1 => tinker_pdf_filters::crc32(&block).to_le_bytes()[..] == *field,
            4 => crc64(&block).to_le_bytes()[..] == *field,
            _ => check == 0,
        };
        if !holds {
            return Err("a block's check does not hold".to_owned());
        }
        out.extend_from_slice(&block);
        pos = check_at + check_size;
    }
    if pos != index {
        return Err("bytes between the blocks and the index".to_owned());
    }
    Ok(out)
}

// ---- the ratchet's own checks ----------------------------------------------
//
// Not `#[ignore]`d: they build no book and take milliseconds, so every
// `cargo test` holds the comparator and the decoder to what the suites rely on.

/// A probe line of the rr3m kind for one book, with the metrics given.
fn rr3m(name: &str, text: u8, search: &str, cons: u8, logical: u8, overlap: &str) -> String {
    format!(
        "ROW\tp {{ margin: 0 }}\t{name}\tm=0\ttext={text}\tsearch={search}\tltr=-\tcons={cons}\t\
         logical={logical}\tride=0\toverlap={overlap}\tlines=1\tmaxx=1.000\tthash=0\tchash=0\tread\t\t\tnf="
    )
}

/// The report of `now` against `base`, both a family's printed lines.
fn report_of(base: &[String], now: &[String]) -> Report {
    let parse = |lines: &[String]| Output::parse(&data_lines(lines.iter().map(String::as_str)));
    let mut report = Report::default();
    compare(&parse(base), &parse(now), &mut report);
    report
}

fn worse_of(report: &Report) -> Vec<String> {
    Report::lines(&report.worse, usize::MAX)
}

#[test]
fn a_book_that_loses_a_metric_its_line_carries_reads_worse() {
    let base = [rr3m("a", 1, "3/3", 1, 1, "0/0")];
    for (metric, now) in [
        ("cons", rr3m("a", 1, "3/3", 0, 1, "0/0")),
        ("logical", rr3m("a", 1, "3/3", 1, 0, "0/0")),
        ("text", rr3m("a", 0, "3/3", 1, 1, "0/0")),
        ("search", rr3m("a", 1, "2/3", 1, 1, "0/0")),
    ] {
        let report = report_of(&base, &[now]);
        assert_eq!(
            worse_of(&report),
            [format!("p {{ margin: 0 }} | a: lost {metric}")]
        );
        assert!(report.gains.is_empty());
    }
}

#[test]
fn a_book_that_gains_a_metric_is_a_gain_and_not_worse() {
    let report = report_of(
        &[rr3m("a", 0, "2/3", 0, 1, "0/0")],
        &[rr3m("a", 1, "3/3", 1, 1, "0/0")],
    );
    assert!(report.worse.is_empty(), "{:?}", report.worse);
    assert_eq!(
        Report::lines(&report.gains, 9),
        ["p { margin: 0 } | a: gained cons,text,search"]
    );
}

#[test]
fn a_book_or_rows_the_run_lacks_read_worse() {
    let base = [
        rr3m("a", 1, "3/3", 1, 1, "0/0"),
        rr3m("b", 1, "3/3", 1, 1, "0/0"),
    ];
    let report = report_of(&base, &[rr3m("a", 1, "3/3", 1, 1, "0/0")]);
    assert_eq!(
        worse_of(&report),
        ["(the family): rows 2 -> 1", "p { margin: 0 } | b: missing"]
    );
}

#[test]
fn a_glyph_drawn_over_another_more_often_reads_worse() {
    let base = [rr3m("a", 1, "3/3", 1, 1, "0/0")];
    let now = [
        rr3m("a", 1, "3/3", 1, 1, "1/2"),
        "VOV\tp { margin: 0 }\ta\t2\tx@1.00..2.00/y@1.50".to_owned(),
    ];
    assert_eq!(
        worse_of(&report_of(&base, &now)),
        ["p { margin: 0 } | a: overprint 0 -> 2; VOV 0 -> 2 at x@1.00..2.00/y@1.50"]
    );
    let vovx = |n: u32| format!("VOVX\tp {{ margin: 0 }}\tar\t00ff\t{n}\tx");
    assert_eq!(
        worse_of(&report_of(&[vovx(0)], &[vovx(3)])),
        ["VOVX p { margin: 0 } | ar | 00ff: overprint 0 -> 3"]
    );
    let zzn = |n: u32| format!("ZZN -4px\tar\t\"x\"\toverprinted {n}\t[]");
    assert_eq!(
        worse_of(&report_of(&[zzn(0)], &[zzn(1)])),
        ["ZZN -4px | ar: overprint 0 -> 1"]
    );
}

#[test]
fn the_overflow_font_at_a_word_spacing_other_than_zero_reads_worse() {
    let tw = |bad: u32, cons: u8| {
        format!("TWROW\tp {{ word-spacing: 1em }}\tar/plain/rtl\tbx=4\tbx32=2\ttwbad={bad}\tvov=0\tcons={cons}\tlogical=1\t\t")
    };
    assert_eq!(
        worse_of(&report_of(&[tw(0, 1)], &[tw(2, 0)])),
        ["p { word-spacing: 1em } | ar/plain/rtl: lost cons; code 32 at a Tw other than 0 0 -> 2"]
    );
}

#[test]
fn the_older_lines_and_the_dirless_books_read_worse_when_they_lose() {
    let k11 = |content: bool| format!("ROW\ts0\trtl-mid\tcontent={content}\tlogical=true\t\"x\"");
    assert_eq!(
        worse_of(&report_of(&[k11(true)], &[k11(false)])),
        ["s0 | rtl-mid: lost content"]
    );
    let book = |holds: bool| format!("BOOK\t\"\"\tHDAC\tx1\tholds={holds}\t[]");
    assert_eq!(
        worse_of(&report_of(&[book(true)], &[book(false)])),
        ["\"\" | HDAC | x1: lost holds"]
    );
    let tally = |n: u32| {
        vec![
            "TALLY\tHDAC\tright\t9".to_owned(),
            format!("TALLY\tHDAC\tmisread-neither-right-drawn-alike\t{n}"),
        ]
    };
    assert_eq!(
        worse_of(&report_of(&tally(1), &tally(2))),
        ["(the family): TALLY lines misread 1 -> 2"]
    );
    let line = "LINE\t\"\"\tHDAC\tx1\tmisread\tgot=\"x\"\tltr=\"x\"\trtl=\"x\"".to_owned();
    assert_eq!(
        worse_of(&report_of(&[], &[line])),
        ["\"\" | HDAC | x1: LINE lines misread 0 -> 1"]
    );
}

#[test]
fn a_new_fail_line_a_moved_fixture_and_a_panic_read_worse() {
    let fail =
        "FAIL twin\tstyle=\"\"\tdir=<p>\tinside=HDAC\trepeat=1\tcontent=false\tlogical=false\t[]"
            .to_owned();
    assert_eq!(
        worse_of(&report_of(&[], &[fail])),
        ["FAIL twin | style=\"\" | dir=<p> | inside=HDAC | repeat=1: a FAIL line new"]
    );
    let fix = |h: &str| {
        format!("FIX\t/testdata/a.pdf\tpages=1\tlines=1\trtl=0\tlogical={h}\tcontent=0\tstreams=0")
    };
    assert_eq!(
        worse_of(&report_of(&[fix("01")], &[fix("02")])),
        ["/testdata/a.pdf: fixture's hashes moved: pages=1\tlines=1\trtl=0\tlogical=01\tcontent=0\tstreams=0 -> pages=1\tlines=1\trtl=0\tlogical=02\tcontent=0\tstreams=0"]
    );
    let panic = "PANIC\tp { margin: 0 }\ta".to_owned();
    assert_eq!(
        worse_of(&report_of(&[], &[panic])),
        ["p { margin: 0 } | a: its probe panicked"]
    );
}

#[test]
fn what_the_normaliser_keeps_is_what_the_ratchet_reads() {
    for (line, data) in [
        ("ROW\tx", true),
        ("BCT-AUTO ran=1", true),
        ("books=12", true),
        ("rows=480 fails=90", true),
        ("Running tests/x.rs", false),
        // cargo's header on Windows, which the normaliser writes as the one above.
        (
            "     Running tests\\x.rs (target\\debug\\deps\\x-0123.exe)",
            false,
        ),
        ("test probe ... ok", false),
        ("R\tx", false),
        ("books=", false),
    ] {
        assert_eq!(is_data(line), data, "{line:?}");
    }
}

#[test]
fn a_tsv_row_that_differs_is_a_change_and_one_the_run_lacks_is_worse() {
    let mut report = Report::default();
    compare_tsv("t.tsv", "a\t1\nb\t2\n", "a\t9\n", &mut report);
    assert_eq!(worse_of(&report), ["t.tsv: b: row missing"]);
    assert_eq!(report.changed, ["t.tsv: a: 1 -> 9"]);
}

#[test]
fn a_baseline_decodes_through_the_engines_lzma2_and_a_damaged_one_does_not() {
    let path = kit()
        .join("baselines")
        .join(BASELINE)
        .join("default")
        .join("zz_code1_neg.txt.xz");
    let bytes = std::fs::read(&path).expect("the baseline");
    let text = String::from_utf8(xz(&bytes).expect("it decodes")).expect("UTF-8");
    assert!(
        text.starts_with("# zz_code1_neg at 935b077, default"),
        "{text:?}"
    );
    assert_eq!(text.lines().filter(|l| l.starts_with("ZZN ")).count(), 8);
    // A byte of the compressed block flipped, the block's CRC-64 changed, and
    // the file cut short: each refused, none read as a shorter text.
    let mut flipped = bytes.clone();
    flipped[40] ^= 0x10;
    assert!(xz(&flipped).is_err());
    let mut check = bytes.clone();
    let index = bytes.len()
        - 12
        - (u32::from_le_bytes([
            bytes[bytes.len() - 8],
            bytes[bytes.len() - 7],
            bytes[bytes.len() - 6],
            bytes[bytes.len() - 5],
        ]) as usize
            + 1)
            * 4;
    check[index - 1] ^= 0x01;
    assert!(xz(&check).is_err());
    assert!(xz(&bytes[..bytes.len() - 1]).is_err());
    assert!(xz(b"not xz at all, and long enough to be read as one").is_err());
    // The same stream, its header and footer saying SHA-256, with their
    // CRC-32s made right: a check this reader does not compute is refused,
    // not taken on trust.
    let mut sha = bytes.clone();
    let footer = sha.len() - 12;
    sha[7] = 0x0A;
    sha[footer + 9] = 0x0A;
    let header_crc = tinker_pdf_filters::crc32(&sha[6..8]).to_le_bytes();
    sha[8..12].copy_from_slice(&header_crc);
    let footer_crc = tinker_pdf_filters::crc32(&sha[footer + 4..footer + 10]).to_le_bytes();
    sha[footer..footer + 4].copy_from_slice(&footer_crc);
    assert_eq!(xz(&sha), Err("check type 10".to_owned()));
}
