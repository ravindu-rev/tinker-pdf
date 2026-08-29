//! Never-panic under hostile input, enforced on every `cargo test`.
//!
//! The fuzz targets in `fuzz/` cover the same entry points far more deeply,
//! but they need a nightly toolchain and `cargo-fuzz`, so nobody runs them by
//! accident. This runs on stable, in CI, on every commit, and it is what
//! stops ruling 1 from being enforced by review alone.
//!
//! The inputs are the real fixtures put through deterministic damage —
//! truncation, byte flips, chunk deletion, and structural word corruption
//! aimed at the things a PDF parser trusts. Every mutation is derived from a
//! fixed seed (ruling 4), so a failure names the exact input that caused it
//! and reproduces on any machine.
//!
//! What is asserted is only that nothing panics and nothing hangs. A mutated
//! file that opens is not required to be *right* — half of them are nonsense,
//! and reporting nonsense calmly is the whole point of the leniency ladder.

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf::{Document, RenderOptions, ShreddedSource, SliceSource};

/// A hand-rolled xorshift, so the corpus is identical everywhere.
///
/// A random-number crate would be a dependency for the sake of one function,
/// and a seeded one from the standard library does not exist.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next() % bound as u64) as usize
    }
}

/// How many inputs to sweep.
///
/// The same work costs eighty times more without optimisation, so a debug
/// `cargo test` takes a representative slice and a release run — which is
/// what CI does — takes the whole sweep. Scaling the corpus rather than
/// skipping the test keeps it honest in both.
fn sweep(full: usize) -> usize {
    if cfg!(debug_assertions) {
        (full / 20).max(8)
    } else {
        full
    }
}

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("testdata");

    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "pdf") {
            if let Ok(bytes) = std::fs::read(&path) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.push((name, bytes));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// One deterministic mutation of a file.
fn mutate(original: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut bytes = original.to_vec();
    if bytes.is_empty() {
        return bytes;
    }

    match rng.next() % 6 {
        0 => {
            // Truncation, which is what a half-finished download looks like
            // and what the repair scanner exists for.
            let keep = rng.below(bytes.len());
            bytes.truncate(keep);
        }
        1 => {
            // A handful of byte flips.
            for _ in 0..1 + rng.below(16) {
                let at = rng.below(bytes.len());
                bytes[at] ^= 1 << rng.below(8);
            }
        }
        2 => {
            // A chunk removed from the middle, which shifts every offset
            // after it and makes the cross-reference table lie.
            let start = rng.below(bytes.len());
            let end = (start + 1 + rng.below(256)).min(bytes.len());
            bytes.drain(start..end);
        }
        3 => {
            // The structural keywords a parser trusts, damaged in place.
            for needle in [
                b"xref".as_slice(),
                b"trailer",
                b"startxref",
                b"stream",
                b"endobj",
                b"/Length",
                b"/Root",
            ] {
                if let Some(at) = find(&bytes, needle) {
                    let offset = at + rng.below(needle.len());
                    bytes[offset] = b'?';
                }
            }
        }
        4 => {
            // A number replaced with something absurd: lengths and offsets
            // are where a trusting parser allocates or indexes wrongly.
            if let Some(at) = find(&bytes, b"/Length") {
                let replacement = b" 999999999999 ";
                let end = (at + 7 + replacement.len()).min(bytes.len());
                bytes.splice(at + 7..end, replacement.iter().copied());
            }
        }
        _ => {
            // Random bytes spliced in, which is the least structured damage
            // and the most likely to reach an unexpected branch.
            let at = rng.below(bytes.len());
            let junk: Vec<u8> = (0..1 + rng.below(64)).map(|_| rng.next() as u8).collect();
            bytes.splice(at..at, junk);
        }
    }

    bytes
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Everything a caller can reach, run over one input.
///
/// A document that opens and then panics on use is no better than one that
/// panics on open, so this exercises the whole surface rather than stopping
/// at `open`.
fn exercise(bytes: Vec<u8>) {
    let Ok(doc) = Document::open(bytes) else {
        return;
    };

    let _ = doc.ladder_level();
    let _ = doc.warnings();
    // Ruling 13's validator reads the cross-reference sections out of the
    // bytes itself, which is a second parser over hostile input and belongs
    // here for exactly that reason.
    let _ = doc.validate();
    let _ = doc.metadata();
    let _ = doc.pdf_version();
    let _ = doc.page_count();
    let _ = doc.outline();
    let _ = doc.page_labels();
    let _ = doc.form_fields();
    // 14.7: `/K` is a graph with no promise of acyclicity and `/RoleMap` is a
    // rewriting system the file writes for itself, so the structure walk is
    // one of the few readers here whose *input shape* is chosen by the
    // attacker rather than merely corrupted by them.
    let structure = doc.structure();
    if let Some(tree) = &structure {
        let _ = tree.element_count();
        let _ = tree.content_count();
        let _ = tree.object_count();
        let _ = tree.elements().len();
    }
    // The signature reader indexes the raw file buffer with offsets the
    // document supplies, which is the shape ruling 1 exists for. Digesting
    // each one exercises the span arithmetic as well as the parse.
    for signature in doc.signatures() {
        let _ = signature.digest(&doc, tinker_pdf::DigestAlgorithm::Sha256);
        let _ = signature.digest(&doc, tinker_pdf::DigestAlgorithm::Sha1);
        let _ = signature.covers_whole_file();
    }
    let _ = doc.permissions();
    let _ = doc.is_encrypted();
    let _ = doc.auth_level();
    // Both the right password and a wrong one, because the failure paths in a
    // security handler are the ones an attacker controls.
    let _ = doc.authenticate("");
    let _ = doc.authenticate("open-sesame");
    let _ = doc.authenticate("\u{0}\u{FFFD}");

    for index in 0..doc.page_count().min(4) {
        let Some(page) = doc.page(index) else {
            continue;
        };
        let _ = page.size();
        let _ = page.media_box();
        let _ = page.crop_box();
        let _ = page.rotation();

        let text = page.text();
        let _ = text.plain_text();
        let _ = text.search("e");
        let _ = text.lines();
        let _ = text.blocks.len();

        // The join, over the same `TextPage`. Reached through the tree bound
        // above rather than through `Page::structured_text` so the walk is
        // not repeated per page, which on a mutated file claiming a thousand
        // pages is the difference between a sweep and a timeout.
        if let Some(tree) = &structure {
            let joined = tree.text_for_page(index, &text);
            let _ = joined.plain_text();
            let _ = joined.orphans;
            let _ = joined.unmarked;
        }

        // Deliberately coarse: a mutated file may claim a vast page box, and
        // the interesting failures are in the operators rather than in how
        // many pixels they cover. Rasterizing a full page of every mutation
        // costs minutes and finds nothing the operators do not.
        let _ = page.render(&RenderOptions {
            scale: 0.05,
            ..RenderOptions::default()
        });
    }
}

#[test]
fn mutated_fixtures_never_panic() {
    let fixtures = fixtures();
    assert!(
        !fixtures.is_empty(),
        "the fixtures are missing, so this test proves nothing"
    );

    for (name, original) in &fixtures {
        // A fixed seed per file, so a failure is reproducible and the case
        // number in the panic message identifies the exact input.
        let mut rng = Rng(0x5DEE_CE66_D1CE_4001 ^ name.len() as u64);
        for case in 0..sweep(400) {
            let mutated = mutate(original, &mut rng);
            // The panic message has to say which case, or reproducing it
            // means running the whole sweep again by hand.
            let label = format!("{name} case {case}");
            let _guard = Guard(&label);
            exercise(mutated);
        }
    }
}

/// The same document, over a source that answers one byte at a time.
///
/// Ruling 1 binds the streaming path as much as the buffered one, and the
/// streaming path has arithmetic the buffered one does not: window bases,
/// offsets rebased from window to document, chunk boundaries, and a growth
/// loop that must terminate. A hostile file drives all of it.
///
/// Fewer operations than [`exercise`] on purpose. The point here is the read
/// path -- open, page tree, render -- rather than every reader in the facade,
/// which the buffered sweep already covers over the same inputs.
fn exercise_streamed(bytes: Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    let source = Arc::new(ShreddedSource::new(SliceSource::new(bytes)));
    let Ok(doc) = Document::open_streaming(source) else {
        return;
    };
    let _ = doc.ladder_level();
    let _ = doc.warnings();
    let _ = doc.is_streamed();
    let _ = doc.first_page_end();
    let _ = doc.page_count();
    if let Some(page) = doc.page(0) {
        let _ = page.size();
        let _ = page.render(&RenderOptions {
            scale: 0.25,
            ..RenderOptions::default()
        });
        let _ = page.text();
    }
    let _ = doc.cos().complete_validation();
    let _ = doc.whole_file_fetched();
}

/// The mutated corpus again, streamed.
#[test]
fn mutated_fixtures_never_panic_over_a_shredded_source() {
    let fixtures = fixtures();
    assert!(
        !fixtures.is_empty(),
        "the fixtures are missing, so this test proves nothing"
    );

    for (name, original) in &fixtures {
        // The same seeds as the buffered sweep, so the two see the same
        // inputs and a case number means the same thing in both.
        let mut rng = Rng(0x5DEE_CE66_D1CE_4001 ^ name.len() as u64);
        for case in 0..sweep(120) {
            let mutated = mutate(original, &mut rng);
            let label = format!("{name} case {case} shredded");
            let _guard = Guard(&label);
            exercise_streamed(mutated);
        }
    }
}

/// Names the case in flight, so a panic message points at it.
///
/// The panic itself is what fails the test; this only makes the report
/// actionable, which matters when the input is one of forty thousand.
struct Guard<'a>(&'a str);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("hostile_input: panicked on {}", self.0);
        }
    }
}

/// Bytes that are not a PDF at all, which is what a mistyped filename or a
/// content-type mismatch delivers.
#[test]
fn arbitrary_bytes_never_panic() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF0);

    for case in 0..sweep(4000) {
        let length = rng.below(4096);
        let mut bytes: Vec<u8> = (0..length).map(|_| rng.next() as u8).collect();

        // Half of them start with a header, so the parser commits to treating
        // them as a PDF rather than rejecting them at the first byte.
        if case % 2 == 0 {
            bytes.splice(0..0, b"%PDF-1.7\n".iter().copied());
        }

        let _guard = Guard("arbitrary");
        exercise(bytes);
    }
}

/// The shapes that have historically broken PDF parsers, written out rather
/// than stumbled upon: a fuzzer needs a long time to invent a cycle, and
/// these are cheap to state directly.
#[test]
fn known_hostile_shapes_never_panic() {
    let cases: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("header only", b"%PDF-1.7"),
        ("no trailer", b"%PDF-1.7\n1 0 obj\n<< >>\nendobj\n"),
        (
            "a page tree that points at itself",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [2 0 R] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "a page that is its own parent",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 3 0 R /MediaBox [0 0 1 1] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "an object that resolves to itself",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 1 0 R >>\nendobj\n\
trailer\n<< /Size 2 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "a media box of infinities",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [-1e400 -1e400 1e400 1e400] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "a content stream of nothing but operators",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 44 >>\nstream\n\
Q Q Q Q ET ET S f W n BT Tj TJ ' \" cm gs Do\n\
endstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "a stream whose length lies",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 99999999 >>\nstream\n0 0 1 1 re f\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "a startxref pointing past the end",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
startxref\n999999999\n%%EOF\n",
        ),
        (
            "a structure element that is its own kid",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /StructParents 0 >>\nendobj\n\
5 0 obj\n<< /Type /StructTreeRoot /K 6 0 R /RoleMap << /A /B /B /A >> >>\nendobj\n\
6 0 obj\n<< /S /A /Pg 3 0 R /K [6 0 R 0 6 0 R] >>\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "a parent tree that points at the structure root",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /StructParents 0 >>\nendobj\n\
5 0 obj\n<< /Type /StructTreeRoot /K [<< /S /P /Pg 3 0 R /K 0 >>] /ParentTree 5 0 R >>\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "an inline property list with no end",
            b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 62 >>\nstream\n\
/P << /MCID << /MCID << /MCID 1 >> BDC 0 0 1 1 re f EMC EMC\n\
endstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        ),
        (
            "nested dictionaries far past any real depth",
            b"%PDF-1.7\n1 0 obj\n<< /A << /A << /A << /A << /A << /A << /A << /A \
<< /A << /A << /A << /A << /A << /A << /A << /A << /A << /A << /A << /A \
<< /A 1 >> >> >> >> >> >> >> >> >> >> >> >> >> >> >> >> >> >> >> >>\nendobj\n\
trailer\n<< /Size 2 /Root 1 0 R >>\n%%EOF\n",
        ),
    ];

    for (name, bytes) in cases {
        let _guard = Guard(name);
        exercise(bytes.to_vec());
    }
}
