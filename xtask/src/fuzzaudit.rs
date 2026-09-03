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
//! # Why this is a tool and not a paragraph
//!
//! `docs/verification.md` has carried the wrong number twice — it said 33
//! targets when there were 38, and 38 when there were 39 — because the list
//! was prose that someone had to remember to update. Everything below is
//! **derived from the directory and from the target sources**, so it cannot
//! drift: the count is a `read_dir`, the control prefix is parsed out of the
//! target's own `split_at`, and the signature check reads the seeds.
//!
//! # What a passing run does and does not mean
//!
//! It means every target has a non-empty corpus, every corpus has a target,
//! `fuzz/Cargo.toml` agrees with both, and no corpus with a **declared
//! signature** has that signature in the wrong place.
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
        "cff_subset",
        "fuzzes a writer: the body is a glyph set, not a file",
    ),
    ("cmap", "a CMap is PostScript text"),
    (
        "content_tokenizer",
        "a content stream is unframed operator text",
    ),
    ("cos_object", "one COS object, which has no file header"),
    (
        "crypt",
        "a structured generator: the body is carved into fields",
    ),
    (
        "crypt_ciphers",
        "key, IV and plaintext carved from the body",
    ),
    ("css", "a stylesheet is text"),
    ("form_script", "a generator over field and action shapes"),
    ("inflate", "a raw DEFLATE stream begins with a bit field"),
    ("jbig2", "an embedded JBIG2 stream has no file header"),
    (
        "layout",
        "a structured generator: the body names a tree of boxes",
    ),
    ("lzw", "a raw LZW stream begins with a code, not a magic"),
    ("pki_cms", "DER: a SEQUENCE tag, which any DER shares"),
    ("pki_der", "DER: a SEQUENCE tag, which any DER shares"),
    ("render_page", "a generator over page content, not a file"),
    (
        "shape",
        "the face sits after a glyph run whose length is a knob",
    ),
    (
        "shape_text",
        "the face sits after a text run whose length is a knob",
    ),
    ("signatures", "a generator over signature dictionaries"),
    ("svg", "SVG is XML text"),
    (
        "type1",
        "a Type 1 font may be PFB-framed or bare PostScript",
    ),
    ("xml", "XML is text"),
];

/// What the audit found about one target.
pub struct Target {
    pub name: String,
    pub seeds: usize,
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
    /// The audit's verdict, as the table prints it.
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

fn seeds_in(dir: &Path) -> Vec<PathBuf> {
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

/// Walks `fuzz/` and reports one row per target.
pub fn survey(root: &Path) -> Result<Vec<Target>, Vec<String>> {
    let targets_dir = root.join("fuzz/fuzz_targets");
    let corpus_dir = root.join("fuzz/corpus");
    let mut problems = Vec::new();
    let sigs = signatures();

    let mut names: Vec<String> = Vec::new();
    match std::fs::read_dir(&targets_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "rs") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        Err(e) => {
            problems.push(format!("fuzz/fuzz_targets is unreadable: {e}"));
            return Err(problems);
        }
    }
    names.sort();

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

/// The marker `docs/verification.md` pastes the generated table under.
const TABLE_MARKER: &str = "<!-- Generated by `cargo run -p xtask -- fuzz --table`";

/// Checks that the table pasted into `docs/verification.md` is the one this
/// tool would generate now.
///
/// Without this the doc drifts the moment a seed is added, which is the same
/// failure the rest of this file exists for, one level up: a table that looks
/// authoritative and is stale. It has happened twice already in that file's
/// prose — 33 targets when there were 38, then 38 when there were 39.
fn check_pasted_table(root: &Path, rows: &[Target]) -> Result<(), String> {
    let path = root.join("docs/verification.md");
    let Ok(doc) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Some(start) = doc.find(TABLE_MARKER) else {
        return Err("docs/verification.md has no generated fuzz table".to_string());
    };
    if doc[start..].starts_with(&render(rows)) {
        Ok(())
    } else {
        Err(concat!(
            "docs/verification.md's fuzz table is stale: regenerate it with ",
            "`cargo run -p xtask -- fuzz --table`"
        )
        .to_string())
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
    if let Err(stale) = check_pasted_table(root, &rows) {
        problems.push(stale);
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The audit as a markdown table, for `docs/verification.md`.
pub fn table(root: &Path) -> Result<String, Vec<String>> {
    Ok(render(&survey(root)?))
}

/// The table itself, so the generator and the staleness check below
/// cannot differ. A check that renders the table a second way is a check
/// that can disagree with the thing it is checking.
fn render(rows: &[Target]) -> String {
    let total: usize = rows.iter().map(|r| r.seeds).sum();
    let mut out = String::new();
    out.push_str(&format!(
        "<!-- Generated by `cargo run -p xtask -- fuzz --table`. {} targets, {total} seeds. -->\n\n",
        rows.len()
    ));
    out.push_str("| Target | Seeds | Control prefix | Signature check |\n");
    out.push_str("| --- | ---: | ---: | --- |\n");
    for row in rows {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            row.name,
            row.seeds,
            if row.prefix == 0 {
                "—".to_string()
            } else {
                format!(
                    "{} byte{}",
                    row.prefix,
                    if row.prefix == 1 { "" } else { "s" }
                )
            },
            row.verdict()
        ));
    }
    out
}

/// Prints the survey for a person.
pub fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    if args.iter().any(|a| a == "--table") {
        let table = table(root)?;
        println!("{table}");
        return Ok("table written to stdout".to_string());
    }
    let rows = survey(root)?;
    println!(
        "{:<18} {:>6} {:>7}  signature check",
        "target", "seeds", "prefix"
    );
    for row in &rows {
        println!(
            "{:<18} {:>6} {:>7}  {}",
            row.name,
            row.seeds,
            row.prefix,
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
