//! **Why each JBIG2-bearing corpus file is refused, by name** (roadmap Tier 2).
//!
//! [`crates/tinker-pdf/tests/jbig2_census.rs`] answers *what the corpus
//! contains*, from bytes, sharing no code with the decoder. This answers the
//! other question, and it can only be answered by the decoder: *why did this
//! build refuse this file?*
//!
//! The reason it needs a test of its own is that the warning a render reports
//! cannot say. `Warning::Jbig2SegmentSkipped` covers the random-access
//! organisation, an unknown segment data length, a segment type nobody has
//! implemented, and a region whose callee already refused — and
//! `Warning::Jbig2SymbolLimitHit` covers a hardening cap, a malformed
//! dimension, and a stream that contradicts its own header. Those are
//! different answers to the question the roadmap asks of every refusal: is
//! this a capability this build lacks, or a file that is broken?
//!
//! [`tinker_pdf_filters::jbig2_decode_attributed`] is the same decode with the
//! precise reason kept, and this walks the corpus through it.
//!
//! ```sh
//! cargo test -p tinker-pdf --test jbig2_attribution -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};
use tinker_pdf_filters::{jbig2_decode_attributed, Jbig2Params, Jbig2Refusal};

/// The ceiling every decode here runs under, matching `jbig2_refinement.rs`.
const CEILING: usize = 1 << 22;

/// The corpora, if they have been fetched.
fn corpus_root() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("TINKER_CORPUS") {
        let path = PathBuf::from(named);
        return path.is_dir().then_some(path);
    }
    let guess = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()?;
    guess.is_dir().then_some(guess)
}

/// Whether a census that finds no corpus should fail rather than skip.
///
/// A campaign whose subject can skip must be able to force it to run, or a
/// missing corpus reads as a clean sweep.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}

fn pdfs_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

/// Every JBIG2 image in one document: its stream, its globals, and its size.
///
/// `stream_decoded` stops at an image filter rather than running it, so what
/// comes back is the embedded organisation of D.3, which is what the decoder
/// takes. Every image rather than the first: a real scan is one JBIG2 stream
/// per page, and the file's refusal may be on page 200.
fn jbig2_images(bytes: Vec<u8>) -> Vec<(Vec<u8>, Vec<u8>, u32, u32)> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
    let cos = doc.cos();
    let filter = cos.intern(b"Filter");
    let parms = cos.intern(b"DecodeParms");
    let globals_key = cos.intern(b"JBIG2Globals");
    let width_key = cos.intern(b"Width");
    let height_key = cos.intern(b"Height");

    let named = |object: &Object| {
        object
            .as_name()
            .and_then(|name| cos.name_bytes(name))
            .is_some_and(|bytes| bytes.as_ref() == b"JBIG2Decode")
    };

    let mut out = Vec::new();
    for (number, entry) in cos.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            _ => 0,
        };
        let reference = ObjRef::new(number, generation);
        let Ok(object) = cos.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let filters = cos.resolve_key(dict, filter);
        let is_jbig2 = match &*filters {
            Object::Array(items) => items.iter().any(named),
            other => named(other),
        };
        if !is_jbig2 {
            continue;
        }
        let (Some(width), Some(height)) = (
            cos.resolve_key(dict, width_key)
                .as_int()
                .and_then(|v| u32::try_from(v).ok()),
            cos.resolve_key(dict, height_key)
                .as_int()
                .and_then(|v| u32::try_from(v).ok()),
        ) else {
            continue;
        };
        let mut globals = Vec::new();
        let parms_value = cos.resolve_key(dict, parms);
        if let Some(parms_dict) = parms_value.as_dict() {
            if let Some(reference) = parms_dict.get_ref(globals_key) {
                if let Ok(data) = cos.stream_decoded(reference) {
                    globals = data;
                }
            }
        }
        if let Ok(data) = cos.stream_decoded(reference) {
            out.push((data, globals, width, height));
        }
    }
    out
}

/// The short name of a refusal, for a table a person reads.
fn name(refusal: Jbig2Refusal) -> String {
    match refusal {
        Jbig2Refusal::UnhandledSegmentType(kind) => format!("UnhandledSegmentType({kind})"),
        other => format!("{other:?}"),
    }
}

/// **Every JBIG2 file the corpus refuses, and what it is waiting on.**
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn every_refused_jbig2_file_is_attributed() {
    let Some(root) = corpus_root() else {
        println!("jbig2-attribution: SKIPPED (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at \
             TINKER_CORPUS: this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!(
        "jbig2-attribution: RAN over {} files under {}",
        files.len(),
        root.display()
    );

    let mut bearing = 0u32;
    let mut refused_files = 0u32;
    // How many *files* each refusal appears in, which is the scheduling unit.
    let mut by_refusal: BTreeMap<String, u32> = BTreeMap::new();
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    let mut malformed_files = 0u32;

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let images = jbig2_images(bytes);
        if images.is_empty() {
            continue;
        }
        bearing += 1;

        let mut refusals: Vec<Jbig2Refusal> = Vec::new();
        for (data, globals, width, height) in &images {
            let params = Jbig2Params {
                globals,
                width: *width,
                height: *height,
            };
            let mut found = Vec::new();
            let _ = jbig2_decode_attributed(data, &params, CEILING, &mut found);
            for refusal in found {
                if !refusals.contains(&refusal) {
                    refusals.push(refusal);
                }
            }
        }
        if refusals.is_empty() {
            continue;
        }
        refused_files += 1;
        if refusals.iter().any(|r| r.is_malformed()) {
            malformed_files += 1;
        }
        let names: Vec<String> = refusals.iter().copied().map(name).collect();
        for one in &names {
            *by_refusal.entry(one.clone()).or_default() += 1;
        }
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        rows.push((shown, names));
    }

    println!("\n{bearing} files carry JBIG2; {refused_files} report a refusal\n");
    println!("--- by refusal, in files ---");
    let mut ordered: Vec<(&String, &u32)> = by_refusal.iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (refusal, count) in &ordered {
        println!("  {count:>4}  {refusal}");
    }

    println!("\n--- per file ---");
    rows.sort();
    for (path, names) in &rows {
        println!("  {path:<58} {}", names.join(" "));
    }

    println!("\nfiles whose refusal says the *file* is broken: {malformed_files}");
}
