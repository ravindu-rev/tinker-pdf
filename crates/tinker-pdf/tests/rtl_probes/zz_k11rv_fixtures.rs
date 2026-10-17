//! `zz_k11rv_fixtures` (kit, K-11 round 4 kit): the books of
//! `tools/probes/rtl/probes/pdf/zz_k11rv_fixtures.rs`, which is the probe as it ran.
//!
//! Re-review probe: every local fixture's text,
//! in logical and content order, and every page's content stream bytes.
//!
//! A fixture is named by its path below the repository's root, a `/` before
//! each component ([`fixture_name`]), which is the name the baseline holds,
//! and a seed of the fuzz corpus is read by that name whatever its extension
//! ([`read_by_the_family`]). Both are built from the path's components, never
//! from its text: the text carries the platform's separator, so on Windows the
//! probe named every fixture `\crates\...` and the ratchet read all 107 as
//! missing, and its filter found `fuzz/corpus` in a path only because the walk
//! joined those two components with a `/` of its own.

use super::common::hash;
use std::path::{Path, PathBuf};
use tinker_pdf::{Document, TextOptions};

/// The repository's root, as the probe wrote it.
fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// A fixture's name: its path below `root`, a `/` before each component, as
/// the probe printed it on Linux, where the baseline was made.
fn fixture_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("a file the walk found below the root")
        .components()
        .map(|part| format!("/{}", part.as_os_str().to_string_lossy()))
        .collect()
}

/// Whether the family reads a file: a seed of the fuzz corpus whatever its
/// extension, and elsewhere a document its extension names.
fn read_by_the_family(root: &Path, path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    fixture_name(root, path).contains("fuzz/corpus")
        || matches!(
            ext.as_str(),
            "pdf" | "xps" | "oxps" | "epub" | "cbz" | "fb2" | "fdf"
        )
}

/// Every file the family reads, with its name, in the order the probe read them.
fn files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    walk(&root.join("testdata"), &mut files);
    walk(&root.join("fuzz/corpus"), &mut files);
    let crates = std::fs::read_dir(root.join("crates")).unwrap();
    let mut crate_dirs: Vec<_> = crates.filter_map(Result::ok).map(|e| e.path()).collect();
    crate_dirs.sort();
    for c in crate_dirs {
        walk(&c.join("tests"), &mut files);
        walk(&c.join("testdata"), &mut files);
        walk(&c.join("fixtures"), &mut files);
    }
    files
        .into_iter()
        .filter(|path| read_by_the_family(root, path))
        .map(|path| (fixture_name(root, &path), path))
        .collect()
}

fn fixtures() {
    let root = root();
    let mut opened = 0;
    for (name, path) in files(&root) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes.len() > 30_000_000 {
            continue;
        }
        let doc = match std::panic::catch_unwind(|| Document::open(bytes)) {
            Ok(Ok(doc)) => doc,
            _ => continue,
        };
        opened += 1;
        let mut logical = String::new();
        let mut content = String::new();
        let mut lines = 0usize;
        for at in 0..doc.page_count().min(400) {
            let Some(page) = doc.page(at) else { continue };
            logical.push_str(&page.text().plain_text());
            logical.push('\u{c}');
            let drawn = page.text_with(&TextOptions {
                content_order: true,
            });
            lines += drawn.lines().len();
            content.push_str(&drawn.plain_text());
            content.push('\u{c}');
        }
        let cos = doc.cos();
        let mut all = Vec::new();
        for page in tinker_pdf_cos::pages::collect(cos) {
            all.extend_from_slice(&tinker_pdf_cos::pages::content_bytes(cos, &page));
        }
        let rtl = logical
            .chars()
            .any(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFEFF));
        emit!(
            "FIX\t{name}\tpages={}\tlines={lines}\trtl={}\tlogical={:016x}\tcontent={:016x}\tstreams={:016x}",
            doc.page_count(),
            rtl as u8,
            hash(logical.as_bytes()),
            hash(content.as_bytes()),
            hash(&all)
        );
    }
    emit!("opened={opened}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    fixtures();
}

// ---- the names, on every platform -------------------------------------------
//
// Not `#[ignore]`d: they open no fixture, so every `cargo test` holds the
// family's names to the baseline's on the platform it runs on.

/// `root` and then `parts`, each joined the platform's way.
fn joined(root: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

#[test]
fn a_fixture_is_named_by_its_path_below_the_root_whatever_separator_joined_it() {
    let root = root();
    for (parts, name) in [
        (
            &["crates", "tinker-pdf", "tests", "cbz", "7z-deflate.cbz"][..],
            "/crates/tinker-pdf/tests/cbz/7z-deflate.cbz",
        ),
        (
            &["testdata", "simple-text.pdf"],
            "/testdata/simple-text.pdf",
        ),
        (
            &["fuzz", "corpus", "cos_object", "dictionary"],
            "/fuzz/corpus/cos_object/dictionary",
        ),
        // As the walk joins a seed: `fuzz/corpus` with its own `/`, the rest
        // with the platform's separator.
        (
            &["fuzz/corpus", "render_page", "fixed-document.xps"],
            "/fuzz/corpus/render_page/fixed-document.xps",
        ),
    ] {
        assert_eq!(
            fixture_name(&root, &joined(&root, parts)),
            name,
            "{parts:?}"
        );
    }
}

#[test]
fn a_path_written_with_slashes_keeps_the_name_the_probe_gave_it() {
    // The baseline's names, made on Linux from the path's text with the root's
    // text taken out: built from the components, each is the same string.
    let root = Path::new("/home/r/tinker-pdf/crates/tinker-pdf/../..");
    for name in [
        "/crates/tinker-pdf/tests/cbz/7z-deflate.cbz",
        "/fuzz/corpus/cos_object/dictionary",
        "/testdata/simple-text.pdf",
        "/crates/tinker-pdf/tests/epub/a book, ספר (2).epub",
    ] {
        let path = PathBuf::from(format!("{}{name}", root.display()));
        let as_the_probe_named_it = path
            .to_string_lossy()
            .replace(&root.to_string_lossy().to_string(), "");
        assert_eq!(as_the_probe_named_it, name);
        assert_eq!(fixture_name(root, &path), name);
    }
}

#[test]
fn a_seed_of_the_fuzz_corpus_is_read_whatever_its_extension_or_separator() {
    let root = root();
    for parts in [
        &["fuzz", "corpus", "cos_object", "dictionary"][..],
        &["fuzz/corpus", "cos_object", "dictionary"],
        &["fuzz", "corpus", "jpeg", "tiny-gray.jpg"],
    ] {
        assert!(
            read_by_the_family(&root, &joined(&root, parts)),
            "{parts:?}"
        );
    }
    // Elsewhere a file is read by its extension, in any case, and nothing else.
    for (parts, read) in [
        (&["testdata", "simple-text.pdf"][..], true),
        (&["crates", "tinker-pdf", "tests", "epub", "A.EPUB"], true),
        (
            &["crates", "tinker-pdf", "tests", "form_data", "x.fdf"],
            true,
        ),
        (&["crates", "tinker-pdf", "tests", "cbz.rs"], false),
        (&["testdata", "tiny-gray.jpg"], false),
        (&["crates", "tinker-pdf", "tests", "corpus", "seed"], false),
    ] {
        assert_eq!(
            read_by_the_family(&root, &joined(&root, parts)),
            read,
            "{parts:?}"
        );
    }
}

#[test]
fn every_fixture_the_baseline_names_is_a_file_the_family_reads_by_that_name() {
    use crate::rtl_probes::ratchet::{kit, xz, BASELINE, BUILD};
    let path = kit()
        .join("baselines")
        .join(BASELINE)
        .join(BUILD)
        .join("zz_k11rv_fixtures.txt.xz");
    let bytes = std::fs::read(&path).expect("the baseline");
    let text = String::from_utf8(xz(&bytes).expect("it decodes")).expect("UTF-8");
    let named: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("FIX\t"))
        .filter_map(|rest| rest.split('\t').next())
        .collect();
    let opened = text
        .lines()
        .find_map(|line| line.strip_prefix("opened="))
        .and_then(|n| n.parse::<usize>().ok())
        .expect("the baseline's opened= line");
    assert!(opened > 0 && named.len() == opened, "{opened} {named:?}");
    let read: std::collections::BTreeSet<String> =
        files(&root()).into_iter().map(|(name, _)| name).collect();
    let missing: Vec<&&str> = named.iter().filter(|name| !read.contains(**name)).collect();
    assert!(
        missing.is_empty(),
        "{} of the baseline's {opened} fixtures are no name the family reads: {missing:?}; it reads {:?}",
        missing.len(),
        read.iter().take(3).collect::<Vec<_>>()
    );
}

#[test]
fn the_kits_probe_names_and_reads_a_fixture_as_this_module_does() {
    // The probe is no workspace member, so no cargo command compiles it: hold
    // its two functions to this module's, text for text, and its walk to them.
    let probe_path = crate::rtl_probes::ratchet::kit()
        .join("probes")
        .join("pdf")
        .join("zz_k11rv_fixtures.rs");
    let probe = std::fs::read_to_string(&probe_path).expect("the kit's probe");
    let here = include_str!("zz_k11rv_fixtures.rs");
    let item = |source: &str, name: &str| -> Option<String> {
        let source = source.replace("\r\n", "\n");
        let start = source.find(&format!("\nfn {name}("))?;
        let end = start + source[start..].find("\n}\n")? + 3;
        Some(
            source[start..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        )
    };
    for name in ["fixture_name", "read_by_the_family"] {
        let theirs = item(&probe, name);
        assert!(theirs.is_some(), "the probe has no fn {name}");
        assert_eq!(theirs, item(here, name), "fn {name}");
    }
    let calls: String = probe.split_whitespace().collect();
    for call in [
        "if!read_by_the_family(&root,&path)",
        "letname=fixture_name(&root,&path);",
    ] {
        assert!(calls.contains(call), "the probe does not call {call}");
    }
}
