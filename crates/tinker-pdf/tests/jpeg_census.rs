//! **What JPEG frames the corpus actually carries** (roadmap Tier 2).
//!
//! Two capabilities are refused in `tinker-pdf-filters` by name — arithmetic
//! coding and every frame type outside SOF0, SOF1 and SOF2 — and the roadmap
//! row for them said *a count first*. This is that count, and it is taken
//! through the COS layer rather than off the raw bytes: a `/DCTDecode` stream
//! inside an object stream or an encrypted document is invisible to a byte
//! scan, and both are ordinary. An earlier scan of the same corpora by raw
//! bytes reported five arithmetic frames and nine at a precision other than
//! eight, every one of which vanished when the marker chain was required to be
//! self-consistent. That is what an instrument outside the reader is worth,
//! and it is why this one is inside it.
//!
//! **The marker walk here shares nothing with `jpeg.rs`**, for
//! `jbig2_census.rs`'s reason: a census taken with the decoder's own reader
//! would agree with that reader by construction, including wherever it is
//! wrong.
//!
//! ```sh
//! cargo test -p tinker-pdf --test jpeg_census -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};

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

/// Every `/DCTDecode` stream in one document, decoded as far as the JPEG.
fn jpeg_streams(bytes: Vec<u8>) -> Vec<Vec<u8>> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
    if doc.is_encrypted() {
        let _ = doc.authenticate("");
    }
    let cos = doc.cos();
    let filter = cos.intern(b"Filter");
    let named = |object: &Object| {
        object
            .as_name()
            .and_then(|name| cos.name_bytes(name))
            .is_some_and(|bytes| bytes.as_ref() == b"DCTDecode")
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
        let is_jpeg = match &*filters {
            Object::Array(items) => items.iter().any(named),
            other => named(other),
        };
        if is_jpeg {
            if let Ok(data) = cos.stream_decoded(reference) {
                out.push(data);
            }
        }
    }
    out
}

/// One frame header, as this census reads it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Frame {
    marker: u8,
    precision: u8,
    components: u8,
}

/// Every SOF in one stream, off a marker chain that has to be self-consistent.
///
/// A JPEG is SOI, then a run of marker segments each carrying its own length,
/// then entropy-coded data. This walks the segments only — it never enters the
/// entropy-coded data, because a byte inside it can read as `FF C1` as easily
/// as a marker can, and it stops at SOS for exactly that reason. A stream
/// whose chain does not add up yields nothing rather than a guess.
fn frames(data: &[u8]) -> Option<Vec<Frame>> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 2usize;
    loop {
        // Fill bytes are legal between segments.
        while data.get(at) == Some(&0xFF) && data.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if data.get(at)? != &0xFF {
            return None;
        }
        let marker = *data.get(at + 1)?;
        at += 2;
        match marker {
            // Standalone: no length follows.
            0x01 | 0xD0..=0xD7 => continue,
            // The scan is the end of what can be walked safely.
            0xDA | 0xD9 => return Some(out),
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
        if length < 2 {
            return None;
        }
        let body = data.get(at + 2..at + length)?;
        // Every SOF marker: C0..CF except C4 (DHT), C8 (reserved) and CC (DAC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            out.push(Frame {
                marker,
                precision: *body.first()?,
                components: *body.get(5)?,
            });
        }
        at += length;
    }
}

/// **Which JPEG frames five corpora carry, and at what precision.**
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_jpeg_frames() {
    let Some(root) = corpus_root() else {
        println!("jpeg-census: SKIPPED (no corpus; set TINKER_CORPUS)");
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
    println!("jpeg-census: RAN over {} files", files.len());

    let mut bearing = 0u32;
    let mut streams = 0u32;
    let mut unwalkable = 0u32;
    let mut markers: BTreeMap<u8, u32> = BTreeMap::new();
    let mut precisions: BTreeMap<u8, u32> = BTreeMap::new();
    let mut components: BTreeMap<u8, u32> = BTreeMap::new();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let found = jpeg_streams(bytes);
        if found.is_empty() {
            continue;
        }
        bearing += 1;
        for stream in found {
            if !seen.insert(stream.clone()) {
                continue; // one image embedded a hundred times is one image
            }
            streams += 1;
            match frames(&stream) {
                None => unwalkable += 1,
                Some(list) => {
                    for frame in list {
                        *markers.entry(frame.marker).or_default() += 1;
                        *precisions.entry(frame.precision).or_default() += 1;
                        *components.entry(frame.components).or_default() += 1;
                    }
                }
            }
        }
    }

    println!("\n{bearing} files carry a /DCTDecode stream; {streams} distinct streams\n");
    println!("frame markers:");
    for (marker, count) in &markers {
        let name = match marker {
            0xC0 => "SOF0, baseline sequential",
            0xC1 => "SOF1, extended sequential, Huffman",
            0xC2 => "SOF2, progressive, Huffman",
            0xC3 => "SOF3, lossless, Huffman",
            0xC5 => "SOF5, differential sequential, Huffman",
            0xC6 => "SOF6, differential progressive, Huffman",
            0xC7 => "SOF7, differential lossless, Huffman",
            0xC9 => "SOF9, extended sequential, arithmetic",
            0xCA => "SOF10, progressive, arithmetic",
            0xCB => "SOF11, lossless, arithmetic",
            0xCD => "SOF13, differential sequential, arithmetic",
            0xCE => "SOF14, differential progressive, arithmetic",
            0xCF => "SOF15, differential lossless, arithmetic",
            _ => "?",
        };
        println!("  {count:>6}  {marker:#04x}  {name}");
    }
    println!("\nsample precision:");
    for (bits, count) in &precisions {
        println!("  {count:>6}  {bits} bits");
    }
    println!("\ncomponents:");
    for (n, count) in &components {
        println!("  {count:>6}  {n}");
    }
    println!("\n{unwalkable} streams whose marker chain does not add up");

    // Pinned against `corpus/corpora.lock` as it stands, September 2026.
    assert_eq!(bearing, BEARING, "the corpus's JPEG population moved");
    assert_eq!(streams, STREAMS, "the distinct-stream count moved");
    let arithmetic: u32 = markers
        .iter()
        .filter(|(marker, _)| matches!(marker, 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF))
        .map(|(_, count)| count)
        .sum();
    assert_eq!(arithmetic, ARITHMETIC, "the arithmetic frame count moved");
    let lossless: u32 = markers
        .iter()
        .filter(|(marker, _)| matches!(marker, 0xC3 | 0xC5 | 0xC6 | 0xC7))
        .map(|(_, count)| count)
        .sum();
    assert_eq!(lossless, LOSSLESS, "the lossless/differential count moved");
    assert_eq!(
        precisions.keys().copied().collect::<Vec<u8>>(),
        PRECISIONS,
        "a sample precision the corpus did not carry has appeared"
    );
}

/// Files carrying at least one `/DCTDecode` stream.
///
/// The largest population any census in this tree measures: 688 files of 5 605,
/// against 118 for JBIG2 and 39 for JPX. JPEG is what a PDF actually carries,
/// which is worth remembering beside the two zeros below.
const BEARING: u32 = 688;

/// Distinct streams among them — one image embedded in a hundred files is one.
///
/// 10 603 of these carry a frame header this census can walk; the other three
/// have a marker chain that does not add up, and are reported rather than
/// guessed at.
const STREAMS: u32 = 10606;

/// **Arithmetic-coded frames: none.**
///
/// SOF9, SOF10, SOF11, SOF13, SOF14 and SOF15 across five corpora, one of
/// which is a thousand documents a crawler found on the open web. This is the
/// number the roadmap row asked for.
const ARITHMETIC: u32 = 0;

/// **Lossless and differential frames: none.** SOF3, SOF5, SOF6 and SOF7.
const LOSSLESS: u32 = 0;

/// **Every sample precision the corpus carries.**
///
/// T.81 B.2.2 allows 8 for a baseline frame and 8 or 12 otherwise. Twelve bits
/// decode now; the interest here is whether anything else appears, since a
/// precision outside those two is a header this build refuses.
const PRECISIONS: [u8; 1] = [8];
