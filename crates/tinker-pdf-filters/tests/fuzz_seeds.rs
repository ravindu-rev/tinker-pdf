//! Eight committed fuzz corpora, replayed on stable, measured for *reach*.
//!
//! `jbig2_seeds.rs`, `jpx_seeds.rs` and `jxr_seeds.rs` do this one corpus at a
//! time because each needs the target's own knob handling restated. These
//! eight are simpler — most take the input whole — so they share a file.
//!
//! # The question these answer, which "it did not panic" does not
//!
//! A cargo-fuzz target that decodes arbitrary bytes is *supposed* to spend
//! most of its time being refused. That makes a corpus of seeds which are all
//! refused indistinguishable, from the outside, from a corpus of seeds which
//! all decode: both run clean, and both report the same seed count.
//!
//! The difference matters enormously. A seed that decodes puts the fuzzer's
//! mutations one bit away from a *valid* file, which is where the interesting
//! defects are; a seed that is refused on its first byte leaves the mutator
//! exploring the space of things that are not the format at all.
//!
//! `fuzz/corpus/jxr` held forty-two seeds and decoded **none** of them, and
//! `fuzz/corpus/jpx` held six more of the same shape — in both cases because
//! the target eats a control byte the seeds were not written with. Neither was
//! visible until something counted. So these tests count, and each asserts
//! that **at least one** seed reaches a successful decode.
//!
//! # What they are not
//!
//! Not fuzzing, and not a correctness check either. A decode that *succeeded*
//! is not compared against anything here — that is what
//! `crates/tinker-pdf-filters/tests/vectors.rs` and the PNG suite are for.
//! These answer one question only: does the corpus reach the code it was
//! written for?

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{
    ascii85_decode, ascii_hex_decode, brotli_decode, ccitt_decode, flate_decode, jpeg_decode,
    lzw_decode, png_decode, png_scan, run_length_decode, tiff_decode, tiff_scan, CcittParams,
    Limits,
};

/// One corpus directory, or `None` when the fuzz tree is not in this checkout.
fn seeds(name: &str) -> Option<Vec<(String, Vec<u8>)>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fuzz/corpus/{name}"));
    if !dir.is_dir() {
        return None;
    }
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path: PathBuf = entry.path();
        if path.is_file() {
            let label = path.file_name()?.to_string_lossy().into_owned();
            out.push((label, std::fs::read(&path).ok()?));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Some(out)
}

/// Reports the count and holds the floor: a corpus that reaches nothing is a
/// corpus that has stopped describing its decoder.
fn report(name: &str, seeds: usize, reached: usize) {
    assert!(seeds > 0, "fuzz/corpus/{name} is empty");
    assert!(
        reached > 0,
        "fuzz/corpus/{name}: not one of {seeds} seeds reaches a successful \
         decode, so the corpus exercises only the refusal path"
    );
    println!("RAN {name}-seeds: {seeds} seeds, {reached} reach the decoder");
}

/// The one-byte output-ceiling knob `png`, `tiff` and `brotli` share.
fn ceiling_knob(data: &[u8]) -> (Limits, &[u8]) {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
    let limits = Limits::new(match knobs & 3 {
        0 => 1,
        1 => 1 << 10,
        2 => 1 << 16,
        _ => 1 << 22,
    });
    (limits, body)
}

#[test]
fn the_png_seeds_reach_the_decoder() {
    let Some(files) = seeds("png") else {
        println!("png-seeds: SKIPPED (no fuzz/corpus/png)");
        return;
    };
    let mut reached = 0;
    for (label, data) in &files {
        let (limits, body) = ceiling_knob(data);
        // The scan is a public entry point of its own, so reaching *either* is
        // reaching the decoder — a seed the ceiling refuses still walked the
        // chunks to find out.
        let scanned = png_scan(body).is_ok();
        let roomy = Limits::new(1 << 22);
        if scanned && png_decode(body, &roomy).is_ok() {
            reached += 1;
        }
        // A ceiling of one byte is deliberately in the knob set, so a refusal
        // here is expected rather than a defect; what is checked is that the
        // *format* was understood.
        let _ = png_decode(body, &limits);
        assert!(
            !scanned || png_scan(body).is_ok(),
            "{label}: png_scan is not deterministic"
        );
    }
    report("png", files.len(), reached);
}

#[test]
fn the_tiff_seeds_reach_the_decoder() {
    let Some(files) = seeds("tiff") else {
        println!("tiff-seeds: SKIPPED (no fuzz/corpus/tiff)");
        return;
    };
    let mut reached = 0;
    for (_label, data) in &files {
        let (_, body) = ceiling_knob(data);
        let roomy = Limits::new(1 << 22);
        if tiff_scan(body).is_ok() && tiff_decode(body, &roomy).is_ok() {
            reached += 1;
        }
    }
    report("tiff", files.len(), reached);
}

#[test]
fn the_brotli_seeds_reach_the_decoder() {
    let Some(files) = seeds("brotli") else {
        println!("brotli-seeds: SKIPPED (no fuzz/corpus/brotli)");
        return;
    };
    let mut reached = 0;
    for (label, data) in &files {
        let (_, body) = ceiling_knob(data);
        let roomy = Limits::new(1 << 22);
        if let Ok(out) = brotli_decode(body, &roomy) {
            reached += 1;
            // The target's own invariant: a roomier ceiling may not change the
            // answer. Restated here because it is cheap and because this
            // corpus is the largest in the tree.
            let roomier = Limits::new(1 << 24);
            assert_eq!(
                brotli_decode(body, &roomier).ok().as_ref(),
                Some(&out),
                "{label}: the ceiling changed the output"
            );
        }
    }
    report("brotli", files.len(), reached);
}

#[test]
fn the_ccitt_seeds_reach_the_decoder() {
    let Some(files) = seeds("ccitt") else {
        println!("ccitt-seeds: SKIPPED (no fuzz/corpus/ccitt)");
        return;
    };
    let mut reached = 0;
    for (label, data) in &files {
        // The target's two control bytes, restated so a seed means here what it
        // means there. `columns` comes from the second byte, which is why this
        // corpus needs a two-byte prefix and not one.
        let (control, body) = data.split_at(data.len().min(2));
        let knobs = control.first().copied().unwrap_or(0);
        let params = CcittParams {
            k: match knobs & 3 {
                0 => 0,
                1 => -1,
                _ => 4,
            },
            columns: u32::from(control.get(1).copied().unwrap_or(8)).max(1),
            rows: match (knobs >> 6) & 3 {
                0 => 0,
                1 => 1,
                2 => 4,
                _ => 64,
            },
            black_is_1: knobs & 4 != 0,
            byte_align: knobs & 8 != 0,
            end_of_line: knobs & 16 != 0,
            end_of_block: knobs & 32 == 0,
        };
        let (packed, _) = ccitt_decode(body, &params, 1 << 20);
        // CCITT has no failure return: an undecodable stream yields the rows it
        // managed. So "reached" is "produced at least one whole row", which is
        // the weakest honest reading of the same question.
        let stride = (params.columns as usize).div_ceil(8);
        if !packed.is_empty() {
            reached += 1;
            assert_eq!(
                packed.len() % stride,
                0,
                "{label}: a partial row puts every row after it at the wrong offset"
            );
        }
    }
    report("ccitt", files.len(), reached);
}

#[test]
fn the_inflate_seeds_reach_the_decoder() {
    let Some(files) = seeds("inflate") else {
        println!("inflate-seeds: SKIPPED (no fuzz/corpus/inflate)");
        return;
    };
    let limits = Limits::new(1 << 20);
    let mut reached = 0;
    for (_label, data) in &files {
        if flate_decode(data, &limits, None).is_ok() {
            reached += 1;
        }
    }
    report("inflate", files.len(), reached);
}

#[test]
fn the_lzw_seeds_reach_the_decoder() {
    let Some(files) = seeds("lzw") else {
        println!("lzw-seeds: SKIPPED (no fuzz/corpus/lzw)");
        return;
    };
    let limits = Limits::new(1 << 20);
    let mut reached = 0;
    for (label, data) in &files {
        // Both conventions, because the early-change bit is the whole reason
        // this decoder is interesting and a seed that only works under one of
        // them is still reaching the code.
        let early = lzw_decode(data, &limits, true, None);
        let late = lzw_decode(data, &limits, false, None);
        if early.is_ok() || late.is_ok() {
            reached += 1;
        }
        if let (Ok(a), Ok(b)) = (&early, &late) {
            // Not an equality: the two conventions legitimately differ. What is
            // checked is that neither invented output past the ceiling.
            assert!(a.data.len() <= limits.max_output, "{label}");
            assert!(b.data.len() <= limits.max_output, "{label}");
        }
    }
    report("lzw", files.len(), reached);
}

#[test]
fn the_jpeg_seeds_reach_the_decoder() {
    let Some(files) = seeds("jpeg") else {
        println!("jpeg-seeds: SKIPPED (no fuzz/corpus/jpeg)");
        return;
    };
    let mut reached = 0;
    for (label, data) in &files {
        if let Ok(image) = jpeg_decode(data, 1 << 22) {
            reached += 1;
            // The geometry has to describe the samples it returned, which is
            // the assertion `fuzz/fuzz_targets/jpeg.rs` does not make — see
            // that file's header.
            assert!(
                image.width > 0 && image.height > 0,
                "{label}: a zero-sized success"
            );
        }
    }
    report("jpeg", files.len(), reached);
}

#[test]
fn the_ascii_filter_seeds_reach_the_decoders() {
    let Some(files) = seeds("ascii_filters") else {
        println!("ascii_filters-seeds: SKIPPED (no fuzz/corpus/ascii_filters)");
        return;
    };
    let limits = Limits::new(1 << 20);
    let mut reached = 0;
    for (label, data) in &files {
        // These three never fail — they are lenient by contract — so "reached"
        // means "produced bytes", which for a corpus of text filters is the
        // same question one step less strictly asked.
        let hex = ascii_hex_decode(data, &limits);
        let a85 = ascii85_decode(data, &limits);
        let rle = run_length_decode(data, &limits);
        if !hex.data.is_empty() || !a85.data.is_empty() || !rle.data.is_empty() {
            reached += 1;
        }
        for out in [&hex, &a85, &rle] {
            assert!(
                out.data.len() <= limits.max_output,
                "{label}: a decoder exceeded its ceiling"
            );
        }
    }
    report("ascii_filters", files.len(), reached);
}
