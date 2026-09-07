//! **Why each JPX-bearing corpus file is refused, by name** (roadmap Tier 2).
//!
//! `jbig2_attribution.rs`'s argument, applied to the other codec that refuses
//! by name. `Warning` is a closed set of seven and `JpxStructureInvalid` alone
//! covers every one of `Refusal::Structure`'s conditions — a tile-part naming
//! a tile outside the grid, tile-parts out of order, two parts disagreeing
//! about `TNsot`, a box shorter than its header, a marker in the wrong place.
//! A corpus run cannot tell those apart, and the roadmap row that concluded
//! "nothing here is reached by a real document" was written from the coarse
//! number.
//!
//! ```sh
//! cargo test -p tinker-pdf --test jpx_attribution -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};
use tinker_pdf_filters::{jpx_decode_attributed, Limits};

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

/// Every JPX-coded image stream in one document.
fn jpx_streams(bytes: Vec<u8>) -> Vec<Vec<u8>> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
    let cos = doc.cos();
    let filter = cos.intern(b"Filter");
    let named = |object: &Object| {
        object
            .as_name()
            .and_then(|name| cos.name_bytes(name))
            .is_some_and(|bytes| bytes.as_ref() == b"JPXDecode")
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
        let is_jpx = match &*filters {
            Object::Array(items) => items.iter().any(named),
            other => named(other),
        };
        if is_jpx {
            if let Ok(data) = cos.stream_decoded(reference) {
                out.push(data);
            }
        }
    }
    out
}

/// **Every JPX file the corpus refuses, and what it is waiting on.**
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn every_refused_jpx_file_is_attributed() {
    let Some(root) = corpus_root() else {
        println!("jpx-attribution: SKIPPED (no corpus; set TINKER_CORPUS)");
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
    println!("jpx-attribution: RAN over {} files", files.len());

    let limits = Limits::new(1 << 28);
    let mut bearing = 0u32;
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let streams = jpx_streams(bytes);
        if streams.is_empty() {
            continue;
        }
        bearing += 1;
        let mut refusals: Vec<String> = Vec::new();
        for stream in &streams {
            if let Err(refusal) = jpx_decode_attributed(stream, &limits) {
                let named = format!("{refusal:?}");
                if !refusals.contains(&named) {
                    refusals.push(named);
                }
            }
        }
        if refusals.is_empty() {
            continue;
        }
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        rows.push((shown, refusals));
    }

    rows.sort();
    println!(
        "\n{bearing} files carry JPX; {} report a refusal\n",
        rows.len()
    );
    for (path, refusals) in &rows {
        println!("  {path}");
        for refusal in refusals {
            println!("      {refusal}");
        }
    }

    // Pinned against `corpus/corpora.lock` as it stands, September 2026.
    assert_eq!(bearing, BEARING, "the corpus's JPX population moved");
    let mut unique: Vec<String> = rows
        .iter()
        .flat_map(|(_, refusals)| refusals.iter().cloned())
        .collect();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique, REASONS,
        "the reasons the corpus is refused for changed"
    );
}

/// JPX-bearing files, as this census counts them.
const BEARING: u32 = 39;

/// **Every reason left, and not one of them is a coding capability.**
///
/// This is the measurement the roadmap's JPX row was missing. It concluded
/// that "nothing here is reached by a real document" from a coarse count, and
/// it was right by accident: the two SafeDocs files it did not account for are
/// refused for **truncation**, not for a marker or a code-block style.
///
/// - `a codestream with no complete tile` — every tile declared more parts
///   than the file carried. A file that stops early; a partial picture is
///   drawn wherever any tile survives, and here none does, so there is nothing
///   to degrade to.
/// - `neither a JP2 signature box nor an SOC/SIZ codestream` — a `/JPXDecode`
///   stream whose bytes are not JPEG 2000 at all.
/// - the two `colr` entries are veraPDF fixtures that are deliberately
///   non-conformant, and the budget is a ruling 1 limit rather than a gap.
///
/// So RGN, POC, PPM, PPT, CRG, `BYPASS`, `TERMALL` and precision above sixteen
/// bits are reached by **zero** corpus files, real or fixture.
const REASONS: [&str; 5] = [
    r#"Budget("tile-component samples")"#,
    r#"Feature("a colr EnumCS this build cannot map")"#,
    r#"Feature("a colr method this build cannot map")"#,
    r#"Structure("a codestream with no complete tile")"#,
    r#"Structure("neither a JP2 signature box nor an SOC/SIZ codestream")"#,
];
