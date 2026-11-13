//! The fuzz corpus audit: does each target's seed corpus actually reach it?
//!
//! # The defect this exists to catch
//!
//! A cargo-fuzz target receives one byte slice. Most of the targets here spend
//! the first byte or two of it on *knobs* — an output ceiling, a set of
//! parameters, which of two entry points to take — and hand the rest to the
//! decoder. That convention is fine, and it is what lets one flat corpus reach
//! both halves of a two-entry-point decoder.
//!
//! It has one failure mode, and it is silent. If the seeds are written as
//! **raw files** while the target eats a prefix, every seed has its first byte
//! swallowed and its signature destroyed before the decoder sees it. The
//! corpus then exercises the *refusal* path of every parser and nothing else,
//! `cargo fuzz` runs clean, and the run reads as coverage it does not have.
//!
//! That is not hypothetical: `fuzz/corpus/jxr` held forty-two seeds and
//! produced **zero** successful decodes for exactly this reason, and
//! `fuzz/corpus/jpx` held six more of the same shape. Neither was visible from
//! a green fuzz run, a seed count, or a directory listing.
//!
//! # Derived, not written down
//!
//! Everything below is **derived from the directory and from the target
//! sources**: the count is a `read_dir`, the control prefix is parsed out of
//! the target's own `split_at`, and the signature check reads the seeds.
//!
//! # What a passing run does and does not mean
//!
//! It means every target has a non-empty corpus, every corpus has a target,
//! `fuzz/Cargo.toml` agrees with both, every target has exactly one row of
//! memory limits in `fuzz/limits.tsv` and no other name has one, and no corpus
//! with a **declared signature** has that signature in the wrong place.
//!
//! The limits are read here and not derived: `bounds_ledger.rs`'s
//! `the_fuzz_limits_are_derived_from_this_ledger` holds the table to the
//! derivation (`docs/design/resource-exhaustion.md`), and this audit holds it
//! to the roster, so a target added without its row fails `cargo xtask check`
//! before a session runs it under libFuzzer's defaults.
//!
//! It does **not** mean the seeds are good. A signature this table does not
//! declare is unchecked rather than passing, and [`UNSIGNED`] lists those by
//! name so the gap is countable. Whether a seed reaches anything interesting
//! past the signature is a question for the per-crate replay tests, which
//! assert it directly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A format signature and where it sits inside the body the target decodes.
struct Signature {
    /// Offset from the start of the *body*, after the control prefix.
    within: usize,
    bytes: &'static [u8],
}

const fn sig(within: usize, bytes: &'static [u8]) -> Signature {
    Signature { within, bytes }
}

/// Corpora whose format has a signature this audit can find, and what it is.
///
/// A row here turns the "did the prefix eat the magic" question into an
/// arithmetic one. Everything not listed is in [`UNSIGNED`].
fn signatures() -> BTreeMap<&'static str, Vec<Signature>> {
    let mut m: BTreeMap<&'static str, Vec<Signature>> = BTreeMap::new();
    m.insert("png", vec![sig(0, b"\x89PNG\r\n\x1a\n")]);
    m.insert("bmp", vec![sig(0, b"BM")]);
    m.insert("gif", vec![sig(0, b"GIF87a"), sig(0, b"GIF89a")]);
    m.insert("webp", vec![sig(8, b"WEBP")]);
    m.insert(
        "tiff",
        vec![
            sig(0, b"II*\0"),
            sig(0, b"MM\0*"),
            sig(0, b"II+\0"),
            sig(0, b"MM\0+"),
        ],
    );
    m.insert(
        "zip_archive",
        vec![sig(0, b"PK\x03\x04"), sig(0, b"PK\x05\x06")],
    );
    m.insert("sevenz", vec![sig(0, b"7z\xbc\xaf\x27\x1c")]);
    // `BZh` and a level digit; `BZh` is the part every level shares.
    m.insert("bzip2", vec![sig(0, b"BZh")]);
    // 7z's range coder opens with a zero byte, after the three parameter bytes.
    m.insert("ppmd", vec![sig(0, b"\0")]);
    // RFC 8878's frame magic, `0xFD2FB528` little-endian. A skippable frame
    // may come first in a stream, but every seed starts with a real one.
    m.insert("zstd", vec![sig(0, b"\x28\xb5\x2f\xfd")]);
    m.insert(
        "rar",
        vec![sig(0, b"Rar!\x1a\x07\0"), sig(0, b"Rar!\x1a\x07\x01")],
    );
    // p.2 of POSIX.1: the magic sits at offset 257 of the first header block.
    m.insert("tar", vec![sig(257, b"ustar")]);
    m.insert(
        "jpx",
        vec![
            // T.800's SOC immediately followed by SIZ, which is a bare
            // codestream, or ISO/IEC 15444-2's JP2 signature box.
            sig(0, b"\xff\x4f\xff\x51"),
            sig(4, b"jP  "),
            sig(4, b"jP\x1a\x1a"),
        ],
    );
    m.insert("jxr", vec![sig(0, b"II\xbc"), sig(0, b"WMPHOTO\0")]);
    m.insert("woff", vec![sig(0, b"wOFF"), sig(0, b"wOF2")]);
    m.insert("jpeg", vec![sig(0, b"\xff\xd8\xff")]);
    m.insert("cos_document", vec![sig(0, b"%PDF-")]);
    // ICC.1's profile signature, at a fixed offset in the 128-byte header.
    m.insert("icc_profile", vec![sig(36, b"acsp")]);
    for name in ["sfnt", "truetype"] {
        m.insert(
            name,
            vec![
                sig(0, b"\0\x01\0\0"),
                sig(0, b"OTTO"),
                sig(0, b"true"),
                sig(0, b"ttcf"),
            ],
        );
    }
    m
}

/// Corpora this audit does **not** signature-check, and why.
///
/// Listed rather than omitted: a target missing from both tables would be
/// silently unaudited, which is the shape of defect this whole file exists to
/// stop. Each of these is genuinely unsignatured — a raw coded stream, a
/// structured generator, or text — so there is nothing to look for.
///
/// **The text corpora cannot be audited mechanically at all**, and an attempt
/// was made and removed rather than kept. Requiring the body to be valid
/// UTF-8 sounds like a weak version of the signature check; it is not, because
/// it fires on legitimate seeds. `xml/utf16` is a UTF-16 document, which XML
/// permits, and `css/repeated-class` carries raw `0xFF` bytes on purpose to
/// drive the parser's recovery. Keeping the rule would have meant an exception
/// list, and a check with an exception list is a check that has been talked
/// out of firing. `css`, `svg` and `xml` were verified by eye instead — every
/// seed's body starts with `<`, `@`, `.`, `*` or a path command, so the knob
/// byte is present — and `svg` has a replay in
/// `crates/tinker-pdf-svg/src/tests.rs` that restates the target's own split.
const UNSIGNED: &[(&str, &str)] = &[
    (
        "annotation_appearance",
        "one annotation dictionary as COS text, which has no file header",
    ),
    (
        "ascii_filters",
        "ASCII85 and ASCIIHex are text with no header",
    ),
    (
        "brotli",
        "RFC 7932 §9's stream begins with a bit field, not a magic",
    ),
    (
        "ccitt",
        "T.4 and T.6 are raw coded bits; the parameters are the knobs",
    ),
    (
        "cff",
        "a bare CFF has a four-byte header but no distinguishing magic",
    ),
    (
        "cff_charstrings",
        "a structured generator: the body chooses a CFF's INDEXes, DICTs and charstrings",
    ),
    (
        "cff_subset",
        "fuzzes a writer: the body is a glyph set, not a file",
    ),
    ("cmap", "a CMap is PostScript text"),
    (
        "content_stream",
        "a structured generator: the body chooses a page's operators, resources and render",
    ),
    (
        "content_tokenizer",
        "a content stream is unframed operator text",
    ),
    ("cos_object", "one COS object, which has no file header"),
    (
        "cos_syntax",
        "a structured generator: the body chooses a file's objects, sections and spellings",
    ),
    (
        "crypt",
        "a structured generator: the body is carved into fields",
    ),
    (
        "crypt_ciphers",
        "key, IV and plaintext carved from the body",
    ),
    ("css", "a stylesheet is text"),
    (
        "css_structured",
        "a structured generator: the body chooses the sheets, their rules and the tree they cascade over",
    ),
    (
        "fb2",
        "FB2 is XML text, and a knob byte picks whether it is framed",
    ),
    (
        "form_data",
        "FDF and XFDF are text, and both readers see every seed whichever it is",
    ),
    ("form_script", "a generator over field and action shapes"),
    (
        "glyf_outlines",
        "a structured generator: the body chooses an sfnt's glyphs, components and tables",
    ),
    (
        "html",
        "HTML is text, and tag soup need not begin with any one tag",
    ),
    (
        "html_structured",
        "a structured generator: the body chooses an element tree and the soup it is written as",
    ),
    ("inflate", "a raw DEFLATE stream begins with a bit field"),
    ("jbig2", "an embedded JBIG2 stream has no file header"),
    (
        "jpx_codestream",
        "a structured generator: the body chooses a codestream's markers, packets and code-block data",
    ),
    (
        "layout",
        "a structured generator: the body names a tree of boxes",
    ),
    ("lzw", "a raw LZW stream begins with a code, not a magic"),
    (
        "markdown",
        "Markdown is text with no header and no control byte",
    ),
    (
        "ocr_import",
        "hOCR, ALTO and PAGE XML are markup text, which has no header to look for",
    ),
    ("pki_cms", "DER: a SEQUENCE tag, which any DER shares"),
    ("pki_der", "DER: a SEQUENCE tag, which any DER shares"),
    ("render_page", "a generator over page content, not a file"),
    (
        "render_rewrite",
        "a structured generator: the body chooses a page, its document's sections and their spellings",
    ),
    (
        "render_tile",
        "a structured generator: the body chooses a page, its render and the regions it is drawn in",
    ),
    (
        "shape",
        "the face sits after a glyph run whose length is a knob",
    ),
    (
        "shape_text",
        "the face sits after a text run whose length is a knob",
    ),
    ("signatures", "a generator over signature dictionaries"),
    (
        "standalone",
        "SVG and XHTML text and bare images of five formats, the sniff being what is under test",
    ),
    ("svg", "SVG is XML text"),
    (
        "type1",
        "a Type 1 font may be PFB-framed or bare PostScript",
    ),
    ("xml", "XML is text"),
    (
        "xml_structured",
        "a structured generator: the body chooses an element tree and the XML it is written as",
    ),
];

/// The header `fuzz/limits.tsv` opens with, its columns in order.
const LIMITS_HEADER: &str = "target\trss_limit_mb\tmalloc_limit_mb\tstack_kib";

/// One target's row of `fuzz/limits.tsv`: what the local fuzz command passes
/// it as `-rss_limit_mb` and `-malloc_limit_mb`, and the stack it runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub rss_limit_mb: u64,
    pub malloc_limit_mb: u64,
    pub stack_kib: u64,
}

/// Reads `fuzz/limits.tsv` against the roster.
///
/// Exactly one well-formed row for each target, and none for a name that is
/// not one: a target with no row would run under libFuzzer's defaults, which
/// is the state the table exists to end, and a row with no target is a limit
/// nothing reads. Every figure must be a positive integer, because a zero
/// turns libFuzzer's limit off.
fn limit_rows(text: &str, names: &[String]) -> Result<BTreeMap<String, Limits>, Vec<String>> {
    let mut problems = Vec::new();
    let mut lines = text.lines();
    match lines.next() {
        Some(header) if header.trim_end() == LIMITS_HEADER => {}
        _ => problems.push(format!(
            "fuzz/limits.tsv: the first line is not `{}`",
            LIMITS_HEADER.replace('\t', "<tab>")
        )),
    }
    let mut rows: BTreeMap<String, Limits> = BTreeMap::new();
    for (index, line) in lines.enumerate() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let at = index + 2;
        let fields: Vec<&str> = line.split('\t').collect();
        let figures: Vec<Option<u64>> = fields
            .iter()
            .skip(1)
            .map(|field| field.parse::<u64>().ok().filter(|n| *n > 0))
            .collect();
        let (Some(name), [Some(rss), Some(malloc), Some(stack)]) =
            (fields.first(), figures.as_slice())
        else {
            problems.push(format!(
                "fuzz/limits.tsv line {at}: not a target and three positive integers: `{line}`"
            ));
            continue;
        };
        if !names.iter().any(|known| known == name) {
            problems.push(format!(
                "fuzz/limits.tsv line {at}: `{name}` is not a fuzz target, so nothing reads its limits"
            ));
            continue;
        }
        let limits = Limits {
            rss_limit_mb: *rss,
            malloc_limit_mb: *malloc,
            stack_kib: *stack,
        };
        if rows.insert((*name).to_string(), limits).is_some() {
            problems.push(format!(
                "fuzz/limits.tsv line {at}: a second row for `{name}`"
            ));
        }
    }
    for name in names {
        if !rows.contains_key(name) {
            problems.push(format!(
                "fuzz/limits.tsv has no row for {name}, so a session would run it under \
                 libFuzzer's default limits; `cargo test -p tinker-pdf --test bounds_ledger \
                 write_the_fuzz_limits -- --ignored` writes the table from the derivation"
            ));
        }
    }
    if problems.is_empty() {
        Ok(rows)
    } else {
        Err(problems)
    }
}

/// What the audit found about one target.
pub struct Target {
    pub name: String,
    pub seeds: usize,
    /// Its row of `fuzz/limits.tsv`.
    pub limits: Option<Limits>,
    /// Bytes the target takes off the front before decoding, parsed from its
    /// own `split_at(data.len().min(N))`.
    pub prefix: usize,
    /// Seeds whose declared signature sits at the right place.
    pub carried: usize,
    /// Seeds whose signature sits at offset 0 while a prefix is eaten — the
    /// defect.
    pub eaten: usize,
    /// Seeds with no signature found anywhere near the front.
    pub neither: usize,
    pub signatured: bool,
}

impl Target {
    /// The audit's verdict, as the survey prints it.
    pub fn verdict(&self) -> String {
        if self.seeds == 0 {
            return "NO SEEDS".to_string();
        }
        if !self.signatured {
            return "unsignatured".to_string();
        }
        if self.eaten > 0 {
            return format!("BROKEN: {} of {} eaten", self.eaten, self.seeds);
        }
        if self.carried == 0 {
            return format!("no signature in {} seeds", self.seeds);
        }
        format!("{}/{} carried", self.carried, self.seeds)
    }
}

/// Bytes the target removes before decoding.
///
/// Parsed from the source rather than declared here, so a target that changes
/// its prefix cannot leave this audit checking the old one. Every target in
/// this tree spells it `split_at(data.len().min(N))`; anything else reads as
/// no prefix, which is the safe direction — it makes the signature check
/// stricter, not looser.
fn control_prefix(source: &str) -> usize {
    const NEEDLE: &str = "split_at(data.len().min(";
    let Some(at) = source.find(NEEDLE) else {
        return 0;
    };
    let rest = &source[at + NEEDLE.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// The seeds of one corpus directory: every file directly in it, in path
/// order. A directory inside it is no seed, and a corpus that is not there has
/// none.
pub fn seeds_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn matches(data: &[u8], offset: usize, sigs: &[Signature]) -> bool {
    sigs.iter().any(|s| {
        let at = offset + s.within;
        data.get(at..at + s.bytes.len()) == Some(s.bytes)
    })
}

/// The fuzz targets, by name: the stem of every `.rs` file in
/// `fuzz/fuzz_targets/`, sorted.
///
/// The roster is this directory and nothing else, so no list of names is
/// written anywhere: the audit, the service build (`fuzz/service/build.sh`)
/// and its check (`fuzzservice`) all read it here.
pub fn roster(root: &Path) -> Result<Vec<String>, String> {
    let targets_dir = root.join("fuzz/fuzz_targets");
    let entries = std::fs::read_dir(&targets_dir)
        .map_err(|e| format!("fuzz/fuzz_targets is unreadable: {e}"))?;
    let mut names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rs") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Walks `fuzz/` and reports one row per target.
pub fn survey(root: &Path) -> Result<Vec<Target>, Vec<String>> {
    let targets_dir = root.join("fuzz/fuzz_targets");
    let corpus_dir = root.join("fuzz/corpus");
    let mut problems = Vec::new();
    let sigs = signatures();

    let names = roster(root).map_err(|e| vec![e])?;

    // The fourth place the list is written down, and the one a session reads
    // its memory limits from.
    let limits = match std::fs::read_to_string(root.join("fuzz/limits.tsv")) {
        Ok(text) => match limit_rows(&text, &names) {
            Ok(rows) => rows,
            Err(found) => {
                problems.extend(found);
                BTreeMap::new()
            }
        },
        Err(e) => {
            problems.push(format!("fuzz/limits.tsv is unreadable: {e}"));
            BTreeMap::new()
        }
    };

    let unsigned: BTreeMap<&str, &str> = UNSIGNED.iter().copied().collect();
    let mut rows = Vec::new();
    for name in &names {
        let source =
            std::fs::read_to_string(targets_dir.join(format!("{name}.rs"))).unwrap_or_default();
        let prefix = control_prefix(&source);
        // Ruling 10's shape, applied to the fuzz layer: a target that cannot
        // see a whole class of defect has to say so, or a clean run gets read
        // as a correct decoder. Fourteen of these assert nothing beyond "it
        // did not panic" and thirteen more assert only structural invariants;
        // every one of the thirty-nine now carries a section saying which it
        // is and where correctness actually lives. This keeps that true for
        // the next one.
        if !source.contains("//! # What this target ") {
            problems.push(format!(
                "fuzz/fuzz_targets/{name}.rs: no `# What this target ...` section, so nothing says what a green run does not prove"
            ));
        }
        let dir = corpus_dir.join(name);
        let files = seeds_in(&dir);
        if files.is_empty() {
            problems.push(format!(
                "fuzz/corpus/{name}: no seeds, so the target starts from random bytes"
            ));
        }
        let declared = sigs.get(name.as_str());
        if declared.is_none() && !unsigned.contains_key(name.as_str()) {
            problems.push(format!(
                "{name}: neither a signature nor a row in UNSIGNED — a target that is \
                 in neither table is silently unaudited"
            ));
        }
        let (mut carried, mut eaten, mut neither) = (0, 0, 0);
        if let Some(declared) = declared {
            for file in &files {
                let data = std::fs::read(file).unwrap_or_default();
                // The signature must sit where the *decoder* will see it,
                // which is after the control prefix. A target that eats
                // nothing wants it at zero, and the two cases are one test.
                if matches(&data, prefix, declared) {
                    carried += 1;
                } else if matches(&data, 0, declared) {
                    eaten += 1;
                } else {
                    neither += 1;
                }
            }
        }
        rows.push(Target {
            name: name.clone(),
            seeds: files.len(),
            limits: limits.get(name).copied(),
            prefix,
            carried,
            eaten,
            neither,
            signatured: declared.is_some(),
        });
    }

    // Every corpus directory must belong to a target, or it is seeds nothing
    // reads — the same silence from the other side.
    if let Ok(entries) = std::fs::read_dir(&corpus_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !names.contains(&name) {
                problems.push(format!(
                    "fuzz/corpus/{name}: no fuzz target of that name reads it"
                ));
            }
        }
    }

    // `fuzz/Cargo.toml` is the third place the list is written down.
    let manifest = std::fs::read_to_string(root.join("fuzz/Cargo.toml")).unwrap_or_default();
    let declared = manifest.matches("[[bin]]").count();
    if declared != names.len() {
        problems.push(format!(
            "fuzz/Cargo.toml declares {declared} [[bin]] targets and \
             fuzz/fuzz_targets holds {}",
            names.len()
        ));
    }
    for name in &names {
        if !manifest.contains(&format!("name = \"{name}\"")) {
            problems.push(format!(
                "fuzz/Cargo.toml has no [[bin]] for {name}, so it never builds"
            ));
        }
    }

    if problems.is_empty() {
        Ok(rows)
    } else {
        Err(problems)
    }
}

/// The gate: every target has seeds, every corpus a target, and no declared
/// signature sits where the control prefix will eat it.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let rows = survey(root)?;
    let mut problems = Vec::new();
    for row in &rows {
        if row.eaten > 0 {
            problems.push(format!(
                "fuzz/corpus/{}: {} of {} seeds carry their signature at offset 0 while \
                 the target eats {} byte(s) of control prefix, so those seeds never reach \
                 the decoder",
                row.name, row.eaten, row.seeds, row.prefix
            ));
        }
        if row.signatured && row.carried == 0 && row.seeds > 0 {
            problems.push(format!(
                "fuzz/corpus/{}: not one of {} seeds carries the format's signature",
                row.name, row.seeds
            ));
        }
    }
    problems.extend(compiles(root).err().unwrap_or_default());
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Every fuzz target still compiles against the crates it drives.
///
/// **This exists because the fuzzers stopped compiling and nothing said so
/// for a month.** `fuzz/` is its own workspace, deliberately — it pulls in
/// `libfuzzer-sys` and builds under a sanitizer, and neither belongs in the
/// engine's dependency graph. The cost of that separation is that
/// `cargo test --workspace`, `cargo clippy --workspace` and every other
/// command a contributor runs step straight past it. So when
/// `tinker_pdf_layout::Content` gained a `Replaced` variant and
/// `tinker_pdf_css::Declaration` gained `Defaulted` and `Content`, three
/// exhaustive matches in `fuzz/fuzz_targets/` stopped building and every
/// local gate stayed green. Ruling 1 says a fuzz crash is a release blocker;
/// a fuzzer that does not build cannot crash, so the rule had quietly become
/// unenforceable.
///
/// `cargo check` rather than `cargo fuzz build`: this is about the *types*
/// agreeing, it runs on stable, and it needs no sanitizer; `cargo fuzz build`
/// builds them for real, locally (`docs/verification.md`). Every target, so
/// the crate's library and its unit tests are checked beside the binaries.
fn compiles(root: &Path) -> Result<(), Vec<String>> {
    let manifest = root.join("fuzz").join("Cargo.toml");
    if !manifest.is_file() {
        return Err(vec![format!("{} is missing", manifest.display())]);
    }
    let out = std::process::Command::new("cargo")
        .args(["check", "--quiet", "--all-targets", "--manifest-path"])
        .arg(&manifest)
        .output();
    let out = match out {
        Ok(out) => out,
        Err(e) => return Err(vec![format!("could not run cargo check on fuzz/: {e}")]),
    };
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut problems = vec![concat!(
        "fuzz/: a fuzz target does not compile, so it cannot crash, ",
        "and ruling 1 is unenforceable for it"
    )
    .to_string()];
    problems.extend(
        stderr
            .lines()
            .filter(|l| l.starts_with("error"))
            .take(10)
            .map(|l| format!("fuzz/: {l}")),
    );
    Err(problems)
}

/// Prints the survey for a person.
pub fn run(root: &Path) -> Result<String, Vec<String>> {
    let rows = survey(root)?;
    println!(
        "{:<22} {:>6} {:>7} {:>8} {:>10} {:>6}  signature check",
        "target", "seeds", "prefix", "rss MiB", "malloc MiB", "stack"
    );
    for row in &rows {
        let (rss, malloc, stack) = row.limits.map_or((0, 0, 0), |l| {
            (l.rss_limit_mb, l.malloc_limit_mb, l.stack_kib)
        });
        println!(
            "{:<22} {:>6} {:>7} {:>8} {:>10} {:>5}K  {}",
            row.name,
            row.seeds,
            row.prefix,
            rss,
            malloc,
            stack,
            row.verdict()
        );
    }
    check(root)?;
    let total: usize = rows.iter().map(|r| r.seeds).sum();
    Ok(format!("{} targets, {total} seeds", rows.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_prefix_is_read_out_of_the_targets_own_split() {
        // The one piece of logic here that could silently break: if this
        // stopped finding the split, every corpus would be checked at offset
        // zero and the audit would pass while checking nothing.
        assert_eq!(
            control_prefix("let (control, body) = data.split_at(data.len().min(1));"),
            1
        );
        assert_eq!(
            control_prefix("let (control, rest) = data.split_at(data.len().min(2));"),
            2
        );
        assert_eq!(control_prefix("fn main() {}"), 0);
        // A split written some other way reads as no prefix, which makes the
        // signature check stricter rather than looser — the safe direction.
        assert_eq!(control_prefix("data.split_at(1)"), 0);
    }

    #[test]
    fn a_signature_is_found_only_where_it_actually_sits() {
        let sigs = vec![sig(0, b"II*\0")];
        assert!(matches(b"\x03II*\0rest", 1, &sigs), "at the prefix");
        assert!(!matches(b"\x03II*\0rest", 0, &sigs), "not at zero");
        assert!(matches(b"II*\0rest", 0, &sigs), "raw, at zero");
        // Short input cannot match, rather than panicking on the slice.
        assert!(!matches(b"II", 0, &sigs));
        assert!(!matches(b"", 4, &sigs));
    }

    /// K-05.2's exit: a target with no row in `fuzz/limits.tsv` fails, and so
    /// does a row for a name that is no target, a second row, a figure that
    /// is not a positive integer and a header that is not the table's.
    #[test]
    fn a_target_without_a_limit_row_fails_the_audit() {
        let names = vec!["inflate".to_string(), "png".to_string()];
        let table = |rows: &str| format!("{LIMITS_HEADER}\n{rows}");

        let whole = limit_rows(&table("inflate\t327\t3\t512\npng\t359\t9\t512\n"), &names)
            .expect("one row a target passes");
        assert_eq!(
            whole.get("png"),
            Some(&Limits {
                rss_limit_mb: 359,
                malloc_limit_mb: 9,
                stack_kib: 512,
            })
        );

        let missing =
            limit_rows(&table("inflate\t327\t3\t512\n"), &names).expect_err("png has no row");
        assert!(
            missing.iter().any(|p| p.contains("no row for png")),
            "{missing:?}"
        );

        for (rows, says) in [
            (
                "inflate\t327\t3\t512\npng\t359\t9\t512\nlzw\t325\t3\t512\n",
                "`lzw` is not a fuzz target",
            ),
            (
                "inflate\t327\t3\t512\npng\t359\t9\t512\npng\t359\t9\t512\n",
                "a second row for `png`",
            ),
            (
                "inflate\t327\t0\t512\npng\t359\t9\t512\n",
                "three positive integers",
            ),
            (
                "inflate\t327\t3\npng\t359\t9\t512\n",
                "three positive integers",
            ),
            (
                "inflate 327 3 512\npng\t359\t9\t512\n",
                "three positive integers",
            ),
        ] {
            let problems = limit_rows(&table(rows), &names).expect_err(says);
            assert!(problems.iter().any(|p| p.contains(says)), "{problems:?}");
        }

        let headless =
            limit_rows("inflate\t327\t3\t512\npng\t359\t9\t512\n", &names).expect_err("no header");
        assert!(
            headless.iter().any(|p| p.contains("first line")),
            "{headless:?}"
        );

        // A checkout that ends its lines with `\r\n` reads as one that does not.
        let crlf = table("inflate\t327\t3\t512\npng\t359\t9\t512\n").replace('\n', "\r\n");
        assert_eq!(limit_rows(&crlf, &names).map(|rows| rows.len()), Ok(2));
    }

    #[test]
    fn an_offset_signature_is_measured_from_the_body_not_the_file() {
        // POSIX.1's `ustar` sits at byte 257 of the tar header, so a seed with
        // a one-byte knob carries it at 258. Getting this relative-to-the-body
        // rule wrong would make every tar seed read as broken.
        let sigs = vec![sig(257, b"ustar")];
        let mut file = vec![0u8; 300];
        file[1 + 257..1 + 257 + 5].copy_from_slice(b"ustar");
        assert!(matches(&file, 1, &sigs));
        assert!(!matches(&file, 0, &sigs));
    }
}
