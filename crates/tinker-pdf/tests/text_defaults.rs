//! Today's default extraction of every committed document, pinned
//! (`docs/design/extraction-options.md`, "The pin of the default"; ROADMAP
//! row FT-17).
//!
//! Each of FT-17's options keeps `Page::text()` as it is unless a caller asks
//! for it, and this suite is what holds that. Every page of every document
//! of [`SETS`] that opens without a key is read by `Page::text()` and written
//! as a record of named fields — each block's and line's box, each line's
//! writing mode, direction, size and text, each character's text, quad,
//! origin, font and size, the page's warnings and its `plain_text()` — and the
//! record's SHA-256 is held to the committed `text_defaults.tsv`. A field
//! another row adds to `TextChar` is in no record, so it moves no digest.
//!
//! The table is written only by [`write_text_defaults`], run by name in each
//! build whose rows it holds:
//!
//! ```text
//! cargo test -p tinker-pdf --test text_defaults -- --ignored write_text_defaults --exact
//! cargo test -p tinker-pdf --features bundled-fonts --test text_defaults -- --ignored write_text_defaults --exact
//! ```
//!
//! A book or an XPS document is laid out or converted by this engine with the
//! faces its build bundles, so every row is kept per build ([`build`]). A
//! digest that moves is a finding put to the owner, never re-pinned to match.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, Quad, TextPage, TextWarning, WritingMode};

/// The committed documents whose default extraction is pinned: each
/// directory, from the repository root, with the extensions read from it.
///
/// The set `text_logical_order.rs`'s `no_left_to_right_page_moves` reads —
/// `testdata` and the committed books — widened to every committed PDF and
/// XPS document. The comic archives are not in it: every page of one is an
/// image, with no text to pin.
const SETS: &[(&str, &[&str])] = &[
    ("testdata", &["pdf"]),
    ("crates/tinker-pdf/tests/signature_support", &["pdf"]),
    ("crates/tinker-pdf/tests/pubsec_support", &["pdf"]),
    ("crates/tinker-pdf/tests/xps/source", &["pdf"]),
    ("fuzz/corpus/cos_document", &["pdf"]),
    ("fuzz/corpus/render_page", &["pdf", "xps", "oxps"]),
    ("crates/tinker-pdf/tests/epub", &["epub"]),
    ("crates/tinker-pdf/tests/xps", &["xps", "oxps"]),
    ("crates/tinker-pdf/tests/xps_rows", &["xps"]),
    ("crates/tinker-pdf/tests/xps_interleaved", &["xps"]),
];

/// The builds whose rows the table must hold: the two the gates test the
/// facade in. A page of either with no row fails as unrecorded; a build of
/// other features with no row at all is passed over and says so.
const RECORDED_BUILDS: [&str; 2] = ["default", "bundled-fonts"];

/// The table's column header, the line after its `#` comments.
const COLUMNS: &str = "build\tdocument\tpage\tdigest";

/// The committed table, which [`write_text_defaults`] writes.
const TABLE: &str = include_str!("text_defaults.tsv");

/// The table's own path, for the writer.
fn table_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/text_defaults.tsv")
}

/// The repository's root, which [`SETS`] and the table's documents are read
/// from.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// This build, by the features that change what a document reads as:
/// `cmap-predefined`, which decodes a predefined CMap's codes, and
/// `bundled-fonts` and `hyphenation`, which lay out a book. `default` and
/// `bundled-fonts` are the gates' two; any other is named by its features.
fn build() -> String {
    let predefined = cfg!(feature = "cmap-predefined");
    let bundled = cfg!(feature = "bundled-fonts");
    let hyphenation = cfg!(feature = "hyphenation");
    match (predefined, bundled, hyphenation) {
        (true, false, false) => "default".to_owned(),
        (true, true, false) => "bundled-fonts".to_owned(),
        _ => {
            let mut parts = vec![if predefined {
                "cmap-predefined"
            } else {
                "no-default-features"
            }];
            if bundled {
                parts.push("bundled-fonts");
            }
            if hyphenation {
                parts.push("hyphenation");
            }
            parts.join("+")
        }
    }
}

/// The documents of [`SETS`], each by its path from the repository root with
/// forward slashes, in path order within each set, with its bytes.
fn documents() -> Vec<(String, Vec<u8>)> {
    let root = root();
    let mut out = Vec::new();
    for (dir, extensions) in SETS {
        let entries = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|e| panic!("{dir}: the committed fixtures are present ({e})"));
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter(|path| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| extensions.contains(&e))
            })
            .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
            .collect();
        names.sort();
        for name in names {
            let shown = format!("{dir}/{name}");
            let bytes = std::fs::read(root.join(dir).join(&name))
                .unwrap_or_else(|e| panic!("{shown}: readable ({e})"));
            out.push((shown, bytes));
        }
    }
    out
}

/// What a read of the set gave: each page's digest by its document and its
/// index, and the documents that did not open.
struct Read {
    digests: BTreeMap<(String, u32), String>,
    documents: usize,
    unopened: Vec<String>,
}

/// Every page of every document of the set, read by `Page::text()` and
/// digested.
fn read_the_set() -> Read {
    let mut digests = BTreeMap::new();
    let mut documents = 0usize;
    let mut unopened = Vec::new();
    for (name, bytes) in self::documents() {
        let Ok(doc) = Document::open(bytes) else {
            // An encrypted fixture opens with a key, which is not the
            // question here.
            unopened.push(name);
            continue;
        };
        documents += 1;
        for page in doc.pages() {
            let digest = digest(&record(&page.text()));
            digests.insert((name.clone(), page.index()), digest);
        }
    }
    Read {
        digests,
        documents,
        unopened,
    }
}

/// `value` to the thousandth, as `{:.3}` writes it, a negative zero as a
/// zero: the same text on every target and every toolchain, since Rust's
/// formatting rounds the exact binary value.
fn number(out: &mut String, value: f64) {
    let text = format!("{value:.3}");
    out.push_str(if text == "-0.000" { "0.000" } else { &text });
}

/// `text` between quotation marks, with `"`, `\`, the C0 controls and DEL
/// escaped as `\"`, `\\` and `\u{…}`, and every other character as it is: a
/// rendering that depends on no Unicode table, so no toolchain moves it.
fn quoted(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A quad's enclosing rectangle, as four numbers after a space each.
fn bounds(out: &mut String, quad: &Quad) {
    let (x0, y0, x1, y1) = quad.bounds();
    for value in [x0, y0, x1, y1] {
        out.push(' ');
        number(out, value);
    }
}

/// A page's record: its named fields, one item a line.
fn record(text: &TextPage) -> String {
    let mut out = String::new();
    for block in text.blocks() {
        out.push_str("block");
        bounds(&mut out, &block.quad);
        out.push('\n');
        for line in &block.lines {
            out.push_str("line");
            bounds(&mut out, &line.quad);
            out.push_str(match line.wmode {
                WritingMode::Horizontal => " horizontal",
                WritingMode::Vertical => " vertical",
            });
            out.push_str(if line.rtl { " rtl " } else { " ltr " });
            number(&mut out, line.size);
            out.push(' ');
            quoted(&mut out, &line.text);
            out.push('\n');
            for c in &line.chars {
                out.push_str("char ");
                quoted(&mut out, &c.text);
                for (x, y) in [c.quad.ul, c.quad.ur, c.quad.ll, c.quad.lr, c.origin] {
                    out.push(' ');
                    number(&mut out, x);
                    out.push(' ');
                    number(&mut out, y);
                }
                out.push(' ');
                match &c.font {
                    Some(font) => quoted(&mut out, font),
                    None => out.push('-'),
                }
                out.push(' ');
                number(&mut out, c.size);
                out.push('\n');
            }
        }
    }
    for warning in &text.warnings {
        match warning {
            TextWarning::UnknownFont { name } => {
                out.push_str("warning unknown-font ");
                quoted(&mut out, name);
            }
            TextWarning::UnmappedCode { code } => {
                let _ = write!(out, "warning unmapped-code {code}");
            }
        }
        out.push('\n');
    }
    out.push_str("plain ");
    quoted(&mut out, &text.plain_text());
    out.push('\n');
    out
}

/// A record's SHA-256, in lowercase hexadecimal.
fn digest(record: &str) -> String {
    tinker_pdf_crypto::sha2::sha256(record.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// One row of the table.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    build: String,
    document: String,
    page: u32,
    digest: String,
}

/// The table's rows, in its order, or what is wrong with a line of it.
fn rows(table: &str) -> Result<Vec<Row>, String> {
    let mut out = Vec::new();
    let mut columns = false;
    for (at, line) in table.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !columns {
            if line != COLUMNS {
                return Err(format!(
                    "line {}: {line:?} is not the column header",
                    at + 1
                ));
            }
            columns = true;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [build, document, page, digest] = fields[..] else {
            return Err(format!("line {}: {line:?} is not four fields", at + 1));
        };
        let page = page
            .parse::<u32>()
            .map_err(|_| format!("line {}: {page:?} is not a page index", at + 1))?;
        let hex = digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !digest.is_empty() && !hex {
            return Err(format!(
                "line {}: {digest:?} is not a lowercase SHA-256",
                at + 1
            ));
        }
        out.push(Row {
            build: build.to_owned(),
            document: document.to_owned(),
            page,
            digest: digest.to_owned(),
        });
    }
    if !columns {
        return Err("the table has no column header".to_owned());
    }
    Ok(out)
}

/// The table's header: what it is and the commands that write it.
const HEADER: &str = "\
# The default extraction of every committed document of text_defaults.rs's
# set, a SHA-256 of each page's record of named fields, by build
# (docs/design/extraction-options.md, \"The pin of the default\").
# Written only by these commands, each writing its build's rows and keeping
# the others':
# cargo test -p tinker-pdf --test text_defaults -- --ignored write_text_defaults --exact
# cargo test -p tinker-pdf --features bundled-fonts --test text_defaults -- --ignored write_text_defaults --exact
# A digest that moves is a finding put to the owner, never re-pinned to match.
";

/// **The writer**: this build's rows, from a read of the set, written into
/// `text_defaults.tsv` beside every other build's rows as the table holds
/// them.
#[test]
#[ignore = "the writer: writes text_defaults.tsv, which is committed"]
fn write_text_defaults() {
    let build = build();
    let on_disk = std::fs::read_to_string(table_path()).expect("the table is beside this file");
    let kept = rows(&on_disk).unwrap_or_else(|problem| panic!("text_defaults.tsv: {problem}"));
    let read = read_the_set();
    let mut table: BTreeSet<Row> = kept.into_iter().filter(|row| row.build != build).collect();
    for ((document, page), digest) in &read.digests {
        table.insert(Row {
            build: build.clone(),
            document: document.clone(),
            page: *page,
            digest: digest.clone(),
        });
    }
    let mut out = String::from(HEADER);
    out.push_str(COLUMNS);
    out.push('\n');
    for row in &table {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}",
            row.build, row.document, row.page, row.digest
        );
    }
    std::fs::write(table_path(), out).expect("the table is written beside this file");
    println!(
        "text-defaults-writer: RAN {build}: {} documents, {} pages written ({} not opened)",
        read.documents,
        read.digests.len(),
        read.unopened.len()
    );
}

/// **Today's default extraction of every committed document is pinned**
/// (FT-17.3): each page's digest is the one the table holds for this build,
/// and every row of this build names a page that is read. A moved digest
/// fails naming its document and page; so does a page of a recorded build
/// with no row, as unrecorded.
#[test]
fn default_extraction_of_every_committed_fixture_is_pinned() {
    let build = build();
    let table = rows(TABLE).unwrap_or_else(|problem| panic!("text_defaults.tsv: {problem}"));
    let ours: BTreeMap<(String, u32), String> = table
        .iter()
        .filter(|row| row.build == build)
        .map(|row| ((row.document.clone(), row.page), row.digest.clone()))
        .collect();
    if ours.is_empty() && !RECORDED_BUILDS.contains(&build.as_str()) {
        println!("text-defaults: SKIPPED, the table holds no digests for the {build} build");
        return;
    }

    let read = read_the_set();
    let mut problems = Vec::new();
    for ((document, page), digest) in &read.digests {
        match ours.get(&(document.clone(), *page)) {
            None => problems.push(format!(
                "{document} page {page}: unrecorded, {digest} (the table holds no row)"
            )),
            Some(pinned) if pinned.is_empty() => problems.push(format!(
                "{document} page {page}: unrecorded, {digest} (the row holds no digest)"
            )),
            Some(pinned) if pinned != digest => problems.push(format!(
                "{document} page {page}: moved, {pinned} pinned and {digest} read"
            )),
            Some(_) => {}
        }
    }
    for (document, page) in ours.keys() {
        if !read.digests.contains_key(&(document.clone(), *page)) {
            problems.push(format!(
                "{document} page {page}: a row of the {build} build names a page that is not read"
            ));
        }
    }
    println!(
        "text-defaults: RAN {build}: {} documents, {} pages read ({} not opened: {})",
        read.documents,
        read.digests.len(),
        read.unopened.len(),
        read.unopened.join(", ")
    );
    assert!(
        problems.is_empty(),
        "{} of {} pages of the {build} build differ from text_defaults.tsv; a moved digest is a \
         finding for the owner, and an unrecorded one is written by the command in the table's \
         header:\n{}",
        problems.len(),
        read.digests.len(),
        problems.join("\n")
    );
    assert!(read.documents > 0, "no document of the set opened");
}

/// The committed table is the writer's: its header, its column line, and
/// rows of four fields in the writer's order, each page of a build at most
/// once, every build a name [`build`] gives.
#[test]
fn the_committed_table_is_the_writers_shape() {
    assert!(
        TABLE.starts_with(HEADER),
        "the table opens with the writer's header"
    );
    let table = rows(TABLE).unwrap_or_else(|problem| panic!("text_defaults.tsv: {problem}"));
    let key = |row: &Row| (row.build.clone(), row.document.clone(), row.page);
    for pair in table.windows(2) {
        assert!(
            key(&pair[0]) < key(&pair[1]),
            "{:?} then {:?}: the rows are in the writer's order, each page of a build once",
            pair[0],
            pair[1]
        );
    }
    for row in &table {
        let named = row.build == "default"
            || row.build == "bundled-fonts"
            || row.build.split('+').enumerate().all(|(at, part)| match at {
                0 => part == "cmap-predefined" || part == "no-default-features",
                _ => part == "bundled-fonts" || part == "hyphenation",
            });
        assert!(named, "{row:?}: a build the writer names");
        assert!(
            SETS.iter()
                .any(|(dir, _)| row.document.starts_with(&format!("{dir}/"))),
            "{row:?}: a document of the set"
        );
    }
    println!("text-defaults-table: RAN {} rows", table.len());
}

/// The record names what it reads, and escapes what would part its lines or
/// fields: a quotation mark, a backslash, a line feed, a tab and DEL; a
/// negative zero is a zero; a character with no font is a dash.
#[test]
fn a_record_is_its_named_fields_escaped() {
    let mut out = String::new();
    quoted(&mut out, "a\"b\\c\nd\te\u{7f}\u{e9}");
    assert_eq!(out, "\"a\\\"b\\\\c\\u{a}d\\u{9}e\\u{7f}\u{e9}\"");
    let mut out = String::new();
    for value in [-0.0, -0.0004, 1.0005, 2.25, f64::NAN] {
        number(&mut out, value);
        out.push(' ');
    }
    assert_eq!(out, "0.000 0.000 1.000 2.250 NaN ");

    let empty = TextPage::new(Vec::new(), BTreeMap::new(), Vec::new(), Vec::new());
    assert_eq!(record(&empty), "plain \"\"\n");
    let warned = TextPage::new(
        Vec::new(),
        BTreeMap::new(),
        Vec::new(),
        vec![
            TextWarning::UnknownFont {
                name: "F\"1".into(),
            },
            TextWarning::UnmappedCode { code: 129 },
        ],
    );
    assert_eq!(
        record(&warned),
        "warning unknown-font \"F\\\"1\"\nwarning unmapped-code 129\nplain \"\"\n"
    );
    assert_eq!(digest(""), digest(""));
    assert_ne!(digest(&record(&empty)), digest(&record(&warned)));
    assert_eq!(digest("").len(), 64);
}
