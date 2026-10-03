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
    // The PDF/A rule engine walks every object in the cross-reference table,
    // follows the nesting inside each one, and pull-parses whatever the
    // metadata stream turned out to hold. Three parsers over hostile input,
    // and the design doc puts the call here for that reason. Each rule group
    // is asked for on its own as well as together, because a group that only
    // ever ran beside another has never been shown to survive alone.
    let _ = doc.validate_pdfa();
    let _ = doc.validate_pdfa_with(tinker_pdf::PdfACoverage::SYNTAX);
    let _ = doc.validate_pdfa_with(tinker_pdf::PdfACoverage::METADATA);
    let _ = doc.metadata();
    let _ = doc.pdf_version();
    let _ = doc.page_count();
    let _ = doc.outline();
    let _ = doc.page_labels();
    let _ = doc.viewer_preferences();
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
        let _ = signature.validation_key();
    }
    // The security store's arrays and `/VRI` are shaped by the file.
    if let Some(store) = doc.security_store() {
        let _ = store.entries.len();
        let _ = store.warnings.len();
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
        let _ = page.bleed_box();
        let _ = page.trim_box();
        let _ = page.art_box();
        let _ = page.rotation();

        let text = page.text();
        let _ = text.plain_text();
        let _ = text.search("e");
        let _ = text.lines();
        let _ = text.blocks.len();
        // UAX #29 over whatever a mutated font decoded to, and the search
        // options, which fold and segment the same text.
        for line in text.lines() {
            let _ = line.words();
        }
        let _ = text.search_with(
            "e",
            &tinker_pdf::SearchOptions {
                case_sensitive: false,
                whole_word: true,
                diacritic_insensitive: true,
            },
        );
        // And the three serialisations, whose escaping is what a hostile
        // `/ToUnicode` or `/BaseFont` attacks.
        for format in [
            tinker_pdf::TextFormat::Json,
            tinker_pdf::TextFormat::Xml,
            tinker_pdf::TextFormat::Html,
        ] {
            let _ = text.serialize(format, &page.text_frame());
        }

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

        // Image extraction describes every image dictionary the page reaches
        // — its space, palette, masks and samples — which the renderer reads
        // only as far as drawing needs.
        let _ = page.images();

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

/// FDF and XFDF (`tinker_pdf::form_data`), damaged the same way.
///
/// The four hand-authored fixtures in `tests/form_data/`, put through the
/// sweep above: both readers see every mutation — the two formats announce
/// themselves, so a damaged FDF is a hostile XFDF too — and whatever either
/// reads is written back out in both formats and read again, because the
/// writers take names and values a hostile file chose.
/// `fuzz/fuzz_targets/form_data.rs` is the deep version of this; this is the
/// one that runs on every commit.
#[test]
fn mutated_form_data_never_panics() {
    use tinker_pdf::form_data::{read_fdf, read_xfdf, FormData};

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/form_data");
    let mut inputs: Vec<(String, Vec<u8>)> = Vec::new();
    for name in [
        "form-fields.fdf",
        "hierarchy.fdf",
        "form-fields.xfdf",
        "hierarchy.xfdf",
    ] {
        let bytes = std::fs::read(dir.join(name)).unwrap_or_default();
        assert!(
            !bytes.is_empty(),
            "{name} is missing, so this proves nothing"
        );
        inputs.push((name.to_string(), bytes));
    }

    let rewrite = |data: &FormData| {
        let _ = read_fdf(&data.to_fdf());
        if let Ok(xml) = data.to_xfdf() {
            let _ = read_xfdf(xml.as_bytes());
        }
    };
    for (name, original) in &inputs {
        let mut rng = Rng(0xF0F0_1207_8000_0001 ^ name.len() as u64);
        for case in 0..sweep(2000) {
            let mutated = mutate(original, &mut rng);
            let label = format!("{name} case {case}");
            let _guard = Guard(&label);
            if let Ok(data) = read_fdf(&mutated) {
                rewrite(&data);
            }
            if let Ok(data) = read_xfdf(&mutated) {
                rewrite(&data);
            }
        }
    }
}

/// What the form data readers hand back stays inside `MAX_FORM_DATA_BYTES`,
/// on every shape that once multiplied a small file into a large allocation,
/// at every scale from small to past the budget.
///
/// Each shape repeats one thing the file says once: a field's 32 KiB name in
/// every warning met inside it, in FDF and in XFDF; one indirect `/T` in every
/// name beneath it, down a chain of nested field objects; one indirect `/V`
/// in every field; a 32 KiB inline name in every kid's. Before the budget the
/// largest of these asked for 184 MB, more than 1 GB, 394 MB, 128 MiB and
/// 148 MB, from under 150 KiB each. At every scale a read either refuses the
/// file whole (`FormDataError::TooLarge`) or hands back no more than the
/// budget — counted from what it handed back, so a copy the accounting forgot
/// shows here — and the largest scale of every shape is refused. A `/Kids`
/// array whose entries name it as their own `/Kids` is here too: it was
/// walked `2^256` times, and now returns.
#[test]
fn form_data_hands_back_no_more_than_its_budget() {
    use std::mem::size_of;
    use tinker_pdf::form_data::{
        read_fdf, read_xfdf, FieldData, FormData, FormDataError, FormDataWarning,
        MAX_FORM_DATA_BYTES,
    };
    use tinker_pdf::FieldValue;

    type Shape<'a> = (
        &'a str,
        bool,
        Box<dyn Fn(usize) -> Vec<u8> + 'a>,
        [usize; 5],
    );

    fn fdf(fields: &str, objects: &str) -> Vec<u8> {
        format!(
            "%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [ {fields} ] >> >>\nendobj\n{objects}\
             trailer\n<< /Root 1 0 R >>\n%%EOF\n"
        )
        .into_bytes()
    }
    /// What `data` holds, counted the way the budget counts it.
    fn held(data: &FormData) -> usize {
        let value = |value: &FieldValue| match value {
            FieldValue::Text(text) | FieldValue::State(text) => text.len(),
            FieldValue::Many(values) => values.iter().map(|v| size_of::<String>() + v.len()).sum(),
            _ => 0,
        };
        let fields: usize = data
            .fields
            .iter()
            .map(|f| size_of::<FieldData>() + f.name.len() + value(&f.value))
            .sum();
        let warnings: usize = data
            .warnings
            .iter()
            .map(|w| {
                size_of::<FormDataWarning>()
                    + match w {
                        FormDataWarning::NotRead { what, field } => what.len() + field.len(),
                        FormDataWarning::ValueUnreadable { field }
                        | FormDataWarning::TreeCut { field } => field.len(),
                        _ => 0,
                    }
            })
            .sum();
        fields + warnings + data.source.as_ref().map_or(0, String::len)
    }

    let name = "n".repeat(32 * 1024);
    let shared = format!("2 0 obj\n({})\nendobj\n", "s".repeat(16 * 1024));
    let shapes: Vec<Shape<'_>> = vec![
        (
            "unread keys in a long-named field",
            false,
            Box::new(|n| {
                let keys: String = (0..n).map(|i| format!("/K{i} 1 ")).collect();
                fdf(&format!("<< /T ({name}) /V (x) {keys} >>"), "")
            }),
            [250, 500, 1000, 2000, 4000],
        ),
        (
            "one indirect /T down a chain of field objects",
            false,
            Box::new(|n| {
                let mut objects = shared.clone();
                for level in 0..n {
                    let kids = if level + 1 == n {
                        String::new()
                    } else {
                        format!("/Kids [ {} 0 R ]", level + 11)
                    };
                    objects.push_str(&format!(
                        "{} 0 obj\n<< /T 2 0 R /V (x) {kids} >>\nendobj\n",
                        level + 10
                    ));
                }
                fdf("10 0 R", &objects)
            }),
            [16, 32, 64, 128, 256],
        ),
        (
            "one indirect /V in every field",
            false,
            Box::new(|n| {
                let fields: String = (0..n)
                    .map(|i| format!("<< /T (f{i}) /V 2 0 R >> "))
                    .collect();
                fdf(&fields, &shared)
            }),
            [300, 600, 1200, 2400, 5000],
        ),
        (
            "kids beneath a long inline name",
            false,
            Box::new(|n| {
                let kids = "<< /T (a) /V (v) >> ".repeat(n);
                fdf(&format!("<< /T ({name}) /Kids [ {kids} ] >>"), "")
            }),
            [250, 500, 1000, 2000, 4000],
        ),
        (
            "unread XFDF elements in a long-named field",
            true,
            Box::new(|n| {
                format!(
                    "<xfdf><fields><field name=\"{name}\">{}<value>v</value></field></fields></xfdf>",
                    "<x/>".repeat(n)
                )
                .into_bytes()
            }),
            [250, 500, 1000, 2000, 4000],
        ),
    ];

    for (label, xml, build, scales) in &shapes {
        for (at, &scale) in scales.iter().enumerate() {
            let bytes = build(scale);
            assert!(
                bytes.len() < 150 * 1024,
                "{label} at {scale}: the input is small"
            );
            let read = if *xml {
                read_xfdf(&bytes)
            } else {
                read_fdf(&bytes)
            };
            match read {
                Ok(data) => {
                    assert!(
                        held(&data) <= MAX_FORM_DATA_BYTES,
                        "{label} at {scale}: {} bytes handed back",
                        held(&data)
                    );
                    assert!(at + 1 < scales.len(), "{label}: the largest is read");
                }
                Err(error) => assert_eq!(error, FormDataError::TooLarge, "{label} at {scale}"),
            }
        }
    }

    let doubling = fdf(
        "<< /T (r) /Kids 5 0 R >>",
        "5 0 obj\n[ << /T (a) /Kids 5 0 R >> << /T (b) /Kids 5 0 R >> ]\nendobj\n",
    );
    let data = read_fdf(&doubling).expect("a self-naming /Kids array reads, and returns");
    assert!(held(&data) < 1024);
}

/// One-file documents that are not PDFs (tier 5's formats row), damaged the
/// same way: a standalone SVG, a loose XHTML file, tag soup, and a PNG, JPEG and
/// TIFF each opened bare. The sniff, the XML prolog walk, the `data:` URL and
/// base64 readers, the cascade and the image embedders all see hostile bytes
/// here, buffered and streamed. `fuzz/fuzz_targets/standalone.rs` is the deep
/// version.
#[test]
fn mutated_standalone_documents_never_panic() {
    let png = {
        let mut out = b"\x89PNG\r\n\x1A\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x02\x08\x02\0\0\0".to_vec();
        out.extend_from_slice(
            b"\xFD\xD4\x9A\x73\0\0\0\x0CIDATx\x9Cc\xF8\xCF\xC0\0\0\x03\x01\x01\0",
        );
        out.extend_from_slice(b"\xC9\xFE\x92\xEF\0\0\0\0IEND\xAEB`\x82");
        out
    };
    let inputs: Vec<(&str, Vec<u8>)> = vec![
        (
            "svg",
            concat!(
                r##"<?xml version="1.0"?><!-- a comment --><svg xmlns="http://www.w3.org/2000/svg" "##,
                r##"xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="20" viewBox="0 0 40 20">"##,
                r##"<rect width="20" height="20" fill="#f00"/><circle cx="30" cy="10" r="5"/>"##,
                r##"<text x="2" y="15" font-size="8">Hi</text>"##,
                r##"<image width="4" height="4" xlink:href="data:image/png;base64,iVBORw0KGgo="/></svg>"##
            )
            .as_bytes()
            .to_vec(),
        ),
        (
            "xhtml",
            concat!(
                r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml">"#,
                r#"<head><title>t</title><style>p { margin: 1em; font-size: 14px } h1 { color: red }</style>"#,
                r#"<link rel="stylesheet" href="a.css"/></head><body><h1 id="a">Head</h1>"#,
                r##"<p>one <b>two</b> <a href="#a">three</a></p><ul><li>x</li></ul>"##,
                r#"<table><tr><td>c</td></tr></table><img src="data:,abc"/></body></html>"#
            )
            .as_bytes()
            .to_vec(),
        ),
        (
            "soup",
            b"<!DOCTYPE html><html><body><p>a<p>b<br><img src=x></body>".to_vec(),
        ),
        (
            "fb2",
            concat!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook ",
                "xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" ",
                "xmlns:l=\"http://www.w3.org/1999/xlink\"><description><title-info>",
                "<author><first-name>A</first-name></author><book-title>T</book-title>",
                "<coverpage><image l:href=\"#c\"/></coverpage></title-info></description>",
                "<body><title><p>T</p></title><section id=\"s\"><title><p>One</p></title>",
                "<p>a <emphasis>b</emphasis> <a l:href=\"#n\" type=\"note\">1</a></p>",
                "<poem><stanza><v>v</v></stanza></poem><image l:href=\"#c\"/>",
                "<table><tr><td>c</td></tr></table></section></body>",
                "<body name=\"notes\"><section id=\"n\"><p>note</p></section></body>",
                "<binary id=\"c\" content-type=\"image/png\">iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC</binary>",
                "</FictionBook>"
            )
            .as_bytes()
            .to_vec(),
        ),
        ("png", png),
        (
            "jpeg",
            b"\xFF\xD8\xFF\xC0\0\x0B\x08\0\x08\0\x08\x01\x01\x11\0\xFF\xD9".to_vec(),
        ),
        (
            "tiff",
            b"II\x2A\0\x08\0\0\0\x03\0\0\x01\x03\0\x01\0\0\0\x02\0\0\0\x01\x01\x03\0\x01\0\0\0\x02\0\0\0\x11\x01\x04\0\x01\0\0\0\x30\0\0\0\0\0\0\0\xFF\x00\xFF\x00"
                .to_vec(),
        ),
    ];
    for (name, original) in &inputs {
        assert!(
            tinker_pdf::standalone::sniff(original).is_some(),
            "{name} is not sniffed, so its sweep would test the PDF parser"
        );
        let mut rng = Rng(0x0005_7A4D_A10E ^ name.len() as u64);
        for case in 0..sweep(400) {
            let mutated = mutate(original, &mut rng);
            let label = format!("{name} case {case}");
            let _guard = Guard(&label);
            exercise(mutated.clone());
            // The FB2 translation writes every element it opens and closes it,
            // so what it hands the reader is XML wherever the FB2 was — the
            // property `fuzz/fuzz_targets/fb2.rs` holds deeply.
            if *name == "fb2" {
                if let Some(xhtml) = tinker_pdf::fb2::to_xhtml(&mutated) {
                    let dom = tinker_pdf::epub::read::markup(
                        xhtml.as_bytes(),
                        &tinker_pdf_xml::Limits::DEFAULT,
                    );
                    assert!(
                        dom.defects.is_empty(),
                        "{label}: the translation is not XML: {:?}",
                        dom.defects
                    );
                }
            }
            // The creation call reads the same markup through the same reader
            // with a stylesheet of the caller's in front, and what it builds is
            // finished and opened like anything else.
            if matches!(*name, "xhtml" | "soup") {
                use tinker_pdf::{DocumentBuilder, FromHtml, PageBox};
                if let Ok((builder, _)) = DocumentBuilder::from_html(
                    &mutated,
                    "p { margin: 1em } h1 { font-size: 2em } @import url(x.css);",
                    PageBox::new(300.0, 200.0),
                ) {
                    exercise(builder.finish());
                }
            }
            if case % 4 == 0 {
                exercise_streamed(mutated);
            }
        }
    }
}

/// Markdown is read from any bytes by a caller who says it is Markdown, so
/// every byte sequence is an input (tier 5's Markdown row). Two halves: the
/// shapes that make a CommonMark reader quadratic — a run of openers with no
/// closer, a line of a hundred thousand `>`, a paragraph of definitions, links
/// after a run of `[` — each written out at a size where a quadratic would be
/// minutes, and a real document mutated. Over both, the XHTML the translation
/// hands the reader must be **well-formed XML**, every time: raw HTML is
/// escaped, nothing else is passed through, and an inline that would nest past
/// the reader's depth cap is set without its element, so a `Truncated` of any
/// kind is a defect here. (Until the lane's review this accepted the depth cap
/// "by design", which is how three hundred nested `<em>` losing every block
/// after them went unflagged.)
/// `fuzz/fuzz_targets/markdown.rs` is the deep version.
#[test]
fn markdown_never_panics_hangs_or_hands_the_reader_bad_xml() {
    use tinker_pdf::epub::read::markup;
    use tinker_pdf::markdown::{to_html, to_xhtml};
    use tinker_pdf::OpenOptions;

    let n = 20_000;
    let mut shapes: Vec<(&str, String)> = vec![
        ("stars", "*a ".repeat(n)),
        (
            "nested emphasis",
            "*a ".repeat(n / 40) + "x" + &" a*".repeat(n / 40) + "\n\nafter\n",
        ),
        ("underscores", "_".repeat(n)),
        (
            "alternating",
            "_*".repeat(n / 2) + "x" + &"*_".repeat(n / 2),
        ),
        ("brackets", "[".repeat(n) + &"]".repeat(n)),
        ("images", "![".repeat(n / 2) + "x"),
        (
            "links after brackets",
            "[".repeat(n / 4) + &"[a](b) ".repeat(n / 4),
        ),
        ("open destinations", "[a](".repeat(n / 4)),
        (
            "nested parentheses",
            "[a](".to_owned() + &"(".repeat(n) + ")",
        ),
        ("comments", "<!--".repeat(n / 4)),
        ("instructions", "<?".repeat(n / 2)),
        ("cdata", "<![CDATA[".repeat(n / 8)),
        ("declarations", "<!X".repeat(n / 3)),
        ("open tags", "<a x=\"".repeat(n / 6)),
        (
            "entities",
            "&#".repeat(n / 2) + "&amp" + &"&x".repeat(n / 2),
        ),
        ("quotes", ">".repeat(5 * n) + "\n" + &"a\n".repeat(n / 10)),
        (
            "lists",
            (0..300)
                .map(|i| format!("{}- x\n", "  ".repeat(i)))
                .collect(),
        ),
        ("definitions", "[a]: /u\n".repeat(n / 8) + "text [a]"),
        (
            "references that multiply",
            format!("[a]: /{}\n\n{}", "d".repeat(n / 2), "[a]".repeat(n / 6)),
        ),
        ("tabs", "\t>\t-\t".repeat(n / 5)),
        ("fences", "```\n".repeat(n / 4)),
        (
            "controls",
            (0u8..32).map(char::from).collect::<String>().repeat(50),
        ),
    ];
    let mut ticks = String::new();
    for k in (1..=150).rev() {
        ticks.push_str(&"`".repeat(k));
        ticks.push('x');
    }
    shapes.push(("tick runs", ticks));

    let check = |name: &str, text: &str| {
        let _ = to_html(text);
        let (xhtml, _) = to_xhtml(text);
        let dom = markup(xhtml.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT);
        assert!(
            dom.defects.is_empty(),
            "{name}: the XHTML did not read as XML: {:?}",
            dom.defects
        );
    };
    for (name, text) in &shapes {
        let _guard = Guard(name);
        check(name, text);
        if let Ok(doc) = Document::open_markdown(text.clone().into_bytes(), &OpenOptions::default())
        {
            let _ = doc.page_count();
            if let Some(page) = doc.page(0) {
                let _ = page.text();
            }
        }
    }

    let note = "# Title\n\nSome *emphasis*, `code`, [a link](http://x.org \"t\") and ![pic](p.png).\n\n\
                > - quoted list\n>   continued\n\n1. one\n2. two\n\n    indented\n\n```\nfenced\n```\n\n\
                <div>raw</div>\n\n[ref]: /url\n";
    let mut rng = Rng(0x4D41_524B_444F_574E);
    for case in 0..sweep(2000) {
        let mutated = mutate(note.as_bytes(), &mut rng);
        let label = format!("markdown case {case}");
        let _guard = Guard(&label);
        let text = String::from_utf8_lossy(&mutated).into_owned();
        check(&label, &text);
        if case % 8 == 0 {
            if let Ok(doc) = Document::open_markdown(mutated, &OpenOptions::default()) {
                let _ = doc.page_count();
                if let Some(page) = doc.page(0) {
                    let _ = page.text();
                    let _ = page.render(&RenderOptions {
                        scale: 0.25,
                        ..RenderOptions::default()
                    });
                }
            }
        }
    }
}
