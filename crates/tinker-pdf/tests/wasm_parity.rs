//! Native against wasm, on the same inputs: every file of a directory opened
//! and drawn, and the render's hash printed for each, so that a run on this
//! host and one on `wasm32-wasip1` can be held to one list
//! (`docs/design/structured-fuzzing.md`, *Native against wasm*; ruling 4).
//!
//! The directory is `WASM_PARITY_DIR`. Its files are documents — the
//! minimised `render_page` corpus as it stands, and the documents the fuzz
//! library writes from the `content_stream` and `render_rewrite` corpora,
//! whose entries are choice sequences — and each is opened and its pages, to
//! [`MAX_PAGES`], drawn at the default options. A line is the file's name and
//! the SHA-256 `determinism.rs` fingerprints with, over the page count, each
//! page's size and bytes, or the refusal's text for a file that does not
//! open. With `WASM_PARITY_OUT` the list is written to that file; with
//! `WASM_PARITY_EXPECTED` it is held to one written so, line for line, which
//! is how the wasm run is held to the native one.
//!
//! Under wasmtime the test binary reaches no file it is not granted, so the
//! run grants the directory and names it to the guest: a directory that
//! cannot be read, or holds nothing, fails rather than printing a banner over
//! nothing. With no directory named the test prints `SKIPPED`, which a run
//! that must compare refuses through `WASM_PARITY_REQUIRED`.

use std::path::{Path, PathBuf};

use tinker_pdf::{Document, RenderOptions};

/// The most pages of one file drawn: a hostile file may claim any number.
const MAX_PAGES: u32 = 16;

/// The bytes a file's line hashes: its page count and each page drawn — its
/// width, its height and every byte — or the refusal's own words.
fn drawn(bytes: Vec<u8>) -> Vec<u8> {
    let mut input = Vec::new();
    match Document::open(bytes) {
        Err(error) => {
            input.extend_from_slice(b"refused ");
            input.extend_from_slice(error.to_string().as_bytes());
        }
        Ok(doc) => {
            let pages = doc.page_count();
            input.extend_from_slice(&pages.to_be_bytes());
            for at in 0..pages.min(MAX_PAGES) {
                match doc.page(at) {
                    None => input.extend_from_slice(b"no page"),
                    Some(page) => {
                        let bitmap = page.render(&RenderOptions::default());
                        input.extend_from_slice(&bitmap.width.to_be_bytes());
                        input.extend_from_slice(&bitmap.height.to_be_bytes());
                        input.extend_from_slice(&bitmap.data);
                    }
                }
            }
        }
    }
    input
}

/// One file's line: its name and the hash of what it draws.
fn line_of(name: &str, bytes: Vec<u8>) -> String {
    let hash: String = tinker_pdf_crypto::sha2::sha256(&drawn(bytes))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{name} {hash}")
}

/// The line of the file at `path`.
fn line(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path:?} cannot be read: {e}"));
    line_of(&name, bytes)
}

/// The files directly in `dir`, in name order.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "WASM_PARITY_DIR {dir:?} cannot be read: {e}; under wasmtime the directory \
             must be granted with --dir and named as the guest sees it"
        )
    });
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    files
}

/// **H-136's replay form**: one line for every file of `WASM_PARITY_DIR`,
/// printed after a `RAN` banner with the count, written to
/// `WASM_PARITY_OUT` and held to `WASM_PARITY_EXPECTED` where those are set.
#[test]
fn print_the_render_hash_of_every_input() {
    let Some(dir) = std::env::var_os("WASM_PARITY_DIR").map(PathBuf::from) else {
        println!("wasm-parity: SKIPPED (no directory; set WASM_PARITY_DIR)");
        assert!(
            std::env::var_os("WASM_PARITY_REQUIRED").is_none(),
            "WASM_PARITY_REQUIRED is set and WASM_PARITY_DIR is not: this run would \
             have compared nothing"
        );
        return;
    };
    let files = files_in(&dir);
    assert!(
        !files.is_empty(),
        "WASM_PARITY_DIR {dir:?} holds no file: a directory wasmtime was not granted \
         reads as nothing, and a list of nothing agrees with everything"
    );
    println!("wasm-parity: RAN over {} files", files.len());
    let lines: Vec<String> = files.iter().map(|path| line(path)).collect();
    for line in &lines {
        println!("{line}");
    }
    if let Some(out) = std::env::var_os("WASM_PARITY_OUT") {
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&out, text).unwrap_or_else(|e| panic!("{out:?} cannot be written: {e}"));
    }
    if let Some(expected) = std::env::var_os("WASM_PARITY_EXPECTED") {
        let text = std::fs::read_to_string(&expected)
            .unwrap_or_else(|e| panic!("{expected:?} cannot be read: {e}"));
        let want: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            want.len(),
            lines.len(),
            "the other target's list holds {} files and this one {}",
            want.len(),
            lines.len()
        );
        for (theirs, mine) in want.iter().zip(&lines) {
            assert_eq!(
                *theirs,
                mine.as_str(),
                "a file draws otherwise on this target than on the other"
            );
        }
    }
}

/// A line is the file's name and sixty-four hexadecimal digits, two files
/// that draw alike have one hash whatever their names, and a file that does
/// not open hashes its refusal. Nothing here reads a directory, so it runs
/// under wasmtime granted none.
#[test]
fn a_line_is_a_name_and_a_hash_of_what_the_file_draws() {
    let a = line_of("refused.pdf", Vec::new());
    let b = line_of("again.pdf", Vec::new());
    let (name, hash) = a.split_once(' ').expect("a name and a hash");
    assert_eq!(name, "refused.pdf");
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(Some(hash), b.split_once(' ').map(|(_, h)| h));
    assert!(drawn(Vec::new()).starts_with(b"refused "));
}
