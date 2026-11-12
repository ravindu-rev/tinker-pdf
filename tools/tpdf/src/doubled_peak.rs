//! The doubled render's peak, a process a shape (MS-27).
//!
//! `tpdf probe`'s doubled render starts from live memory alone ([`doubled`]):
//! what the relations before it freed goes back to the kernel first, so its
//! peak is its own canvas and work over what the probe still holds, and not
//! what the allocator happened to keep. This holds that on a generated
//! document of each shape whose corpus reading moved — one A4 page, whose
//! doubled render is the largest part of its probe, and ten thousand A4
//! pages, whose rewrites free far more than the doubled render needs — by
//! rendering it several times in one process, each time after another part
//! that frees memory the allocator keeps, and reading the resident set while
//! the doubled bitmap is held.
//!
//! A resident set belongs to the whole process, so each shape runs in a child
//! of its own: this test binary re-executed to run [`doubled_peak_child`]
//! alone, which makes this file a `SPAWNERS` row. It hosts one shape a process
//! and adjudicates nothing.
//!
//! Linux with glibc alone: the resident set is read from `/proc`, and the
//! allocator whose keeping this is about is glibc's.

use std::collections::BTreeMap;
use std::process::Command;

use tinker_pdf::{Bitmap, Document, DocumentBuilder, ImageData, Page, RenderOptions};

use super::{cropping, doubled, release_freed, rotation, status_bytes, Marks};

/// The variable that names the shape a child renders.
const SHAPE: &str = "TPDF_DOUBLED_SHAPE";

/// What a child prints before its shape and its readings.
const REPORT: &str = "doubled-resident";

/// The shapes, by the names a child is given.
const SHAPES: [&str; 2] = ["one-page", "many-pages"];

/// The parts run before each doubled render, in order: nothing, the probe's
/// own relations that rewrite the document, and small blocks freed in the
/// order they were made and in reverse.
const EARLIER: [&str; 4] = ["nothing", "rewrites", "freed-in-order", "freed-in-reverse"];

/// The pages of the second shape, as many as the conformance corpus's
/// implementation-limit case has.
const PAGES: u32 = 10_000;

/// How far a reading may sit from the first.
///
/// A resident set is counted in pages, so where each small allocation lands
/// moves it by a few, and a kernel that states it from per-CPU counters folded
/// in batches can read it a few hundred pages away; what the allocator kept
/// moved a reading by megabytes.
const TOLERANCE: u64 = 1 << 20;

/// One A4 page in the shape of the PDF 2.0 example corpus's largest file:
/// forty lines of standard-14 text over an image, tagged, so that its doubled
/// render is the largest part of its probe.
fn one_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    let samples: Vec<u8> = (0..64u32 * 64 * 3).map(|n| (n * 53 % 251) as u8).collect();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 64,
            height: 64,
            data: &samples,
        }
    ));
    builder.add_page(595.32, 841.92, |page| {
        page.tagged(b"P", |page| {
            for line in 0..40u32 {
                page.text(
                    b"F0",
                    11.0,
                    72.0,
                    780.0 - 16.0 * f64::from(line),
                    &format!("Line {line}: the quick brown fox jumps over the lazy dog."),
                );
            }
        });
        page.image(b"Im0", 72.0, 60.0, 200.0, 120.0);
    });
    builder.finish()
}

/// [`PAGES`] A4 pages of a line each, in the shape of the conformance
/// corpus's implementation-limit case: a rewrite of it frees far more than
/// its doubled render needs.
fn many_pages() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for number in 0..PAGES {
        builder.add_page(595.0, 842.0, |page| {
            page.text(
                b"F0",
                11.0,
                72.0,
                780.0,
                &format!("Page {number}: a line of its own."),
            );
        });
    }
    builder.finish()
}

/// Twelve mebibytes in blocks of six sizes from 64 bytes to 20 000, every
/// byte written, so every page of them is resident.
fn blocks() -> Vec<Vec<u8>> {
    const SIZES: [usize; 6] = [64, 200, 1_000, 3_000, 8_000, 20_000];
    let mut blocks = Vec::new();
    let mut total = 0;
    for size in SIZES.iter().cycle() {
        if total >= 12 << 20 {
            break;
        }
        blocks.push(vec![(blocks.len() % 251) as u8 + 1; *size]);
        total += size;
    }
    blocks
}

/// Runs the earlier part `part` before a doubled render, and returns what has
/// to stay alive past the reading: after freed blocks, an allocation made
/// after them, so the heap's top lies above what they freed and the allocator
/// cannot give it back on its own.
fn earlier(
    part: &str,
    doc: &Document,
    page: &Page,
    base: &Bitmap,
    render: &RenderOptions,
) -> Option<Vec<u8>> {
    match part {
        "nothing" => None,
        // As `metamorphic` asks them: each rewrites, reopens and renders the
        // document, and frees all of it as it returns.
        "rewrites" => {
            let mut marks = Marks::default();
            let mut sink = std::io::sink();
            let _ = rotation(&mut sink, &mut marks, doc, base, render, None);
            let _ = cropping(&mut sink, &mut marks, doc, page, base, render, None);
            None
        }
        "freed-in-order" | "freed-in-reverse" => {
            let mut blocks = blocks();
            let after = vec![1u8; 64];
            if part == "freed-in-reverse" {
                while blocks.pop().is_some() {}
            }
            drop(blocks);
            Some(after)
        }
        other => panic!("no earlier part is named {other}"),
    }
}

/// The child's side: nothing unless a parent named a shape, and then that
/// shape's readings printed — the live memory before its first doubled
/// render, the doubled bitmap's bytes, and the resident set with the doubled
/// render held after each of [`EARLIER`].
#[test]
#[ignore = "a child of the doubled render's peak test, which starts it with a shape"]
fn doubled_peak_child() {
    let Some(shape) = std::env::var_os(SHAPE) else {
        return;
    };
    let shape = shape.to_string_lossy().into_owned();
    let bytes = match shape.as_str() {
        "one-page" => one_page(),
        "many-pages" => many_pages(),
        other => panic!("no shape is named {other}"),
    };
    let doc = Document::open(bytes).expect("the shape opens");
    let page = doc.page(0).expect("its first page");
    let render = RenderOptions::at_dpi(72.0);
    let base = page.render(&render);
    // Once through first, so the document's caches and every table made on
    // first use exist before a reading: what is made once is live, not kept.
    let _ = earlier("rewrites", &doc, &page, &base, &render);
    let canvas = doubled(&page, &render).data.len();
    release_freed();
    let live = status_bytes("VmRSS").expect("the resident set");
    let mut line = format!("{REPORT} {shape} live={live} canvas={canvas}");
    for part in EARLIER {
        let kept = earlier(part, &doc, &page, &base, &render);
        let held = doubled(&page, &render);
        let resident = status_bytes("VmRSS").expect("the resident set");
        drop(held);
        drop(kept);
        line.push_str(&format!(" {part}={resident}"));
    }
    println!("{line}");
}

/// The readings a child reported for `shape`, by name, found wherever the
/// report sits on its line, since the harness may have begun that line with
/// the test's name.
fn reported(stdout: &str, shape: &str) -> Option<BTreeMap<String, u64>> {
    let marker = format!("{REPORT} {shape} ");
    let at = stdout.find(&marker)? + marker.len();
    let line = stdout[at..].lines().next()?;
    line.split_whitespace()
        .map(|field| {
            let (name, bytes) = field.split_once('=')?;
            Some((name.to_string(), bytes.parse().ok()?))
        })
        .collect()
}

/// One child: this binary re-executed to run `shape` alone, and its readings.
///
/// Under one allocator arena (`MALLOC_ARENA_MAX=1`): the harness runs a test
/// on a thread of its own, which glibc would give an arena of its own, and the
/// probe allocates from the main arena alone, on the one thread it has.
fn child(shape: &str) -> BTreeMap<String, u64> {
    let exe = std::env::current_exe().expect("the test binary's own path");
    let output = Command::new(&exe)
        .args([
            "doubled_peak::doubled_peak_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SHAPE, shape)
        .env("MALLOC_ARENA_MAX", "1")
        .output()
        .unwrap_or_else(|error| panic!("{} did not start: {error}", exe.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the {shape} child failed with {}: {stdout}{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    reported(&stdout, shape)
        .unwrap_or_else(|| panic!("the {shape} child reported no readings: {stdout}"))
}

/// What holding `readings`, one child's, finds: a line for each earlier part
/// whose reading sits more than [`TOLERANCE`] from the first, and one if the
/// first does not hold the doubled canvas over the live memory before it,
/// within the same tolerance. Empty when the doubled render's peak is its own
/// memory after every part.
fn judge(shape: &str, readings: &BTreeMap<String, u64>) -> Vec<String> {
    let get = |name: &str| readings.get(name).copied();
    let (Some(live), Some(canvas), Some(first)) = (get("live"), get("canvas"), get(EARLIER[0]))
    else {
        return vec![format!("{shape}: readings missing from {readings:?}")];
    };
    let mut problems = Vec::new();
    if first.saturating_add(TOLERANCE) < live.saturating_add(canvas) {
        problems.push(format!(
            "{shape}: {first} bytes resident with the doubled render held, under the {live} live \
             before it and its {canvas}-byte canvas"
        ));
    }
    for part in &EARLIER[1..] {
        match get(part) {
            None => problems.push(format!("{shape}: no reading after {part}")),
            Some(bytes) if bytes.abs_diff(first) > TOLERANCE => problems.push(format!(
                "{shape}: {bytes} bytes resident with the doubled render held after {part}, \
                 against {first} after nothing"
            )),
            Some(_) => {}
        }
    }
    problems
}

/// **The doubled render's peak is its own memory, whatever was freed before
/// it** (MS-27): on each shape, in one process, after nothing, after the
/// probe's own rewriting relations and after small blocks freed in either
/// order, the resident set with the doubled render held reads within
/// [`TOLERANCE`] of the first, which holds its canvas over the live memory
/// before it.
#[test]
fn the_doubled_render_peaks_at_its_own_memory_whatever_was_freed_before_it() {
    if !cfg!(all(target_os = "linux", target_env = "gnu")) {
        println!(
            "SKIPPED: the doubled render's peak, read from /proc over glibc's allocator, which \
             Linux with glibc alone has"
        );
        return;
    }
    let mut problems = Vec::new();
    for shape in SHAPES {
        let readings = child(shape);
        println!("{shape}: {readings:?}");
        problems.extend(judge(shape, &readings));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    println!(
        "RAN: the doubled render's peak, {} shapes a process each, {} earlier parts each",
        SHAPES.len(),
        EARLIER.len()
    );
}

/// The judgement on readings written out, on every target: a reading past
/// the tolerance after an earlier part is named, and so is a first reading
/// that does not hold the canvas.
#[test]
fn a_reading_past_the_tolerance_or_without_its_canvas_is_named() {
    let readings = |pairs: &[(&str, u64)]| -> BTreeMap<String, u64> {
        pairs
            .iter()
            .map(|(name, bytes)| ((*name).to_string(), *bytes))
            .collect()
    };
    let steady = readings(&[
        ("live", 10 << 20),
        ("canvas", 6 << 20),
        ("nothing", 17 << 20),
        ("rewrites", (17 << 20) + 4096),
        ("freed-in-order", (17 << 20) - 4096),
        ("freed-in-reverse", 17 << 20),
    ]);
    assert!(judge("one-page", &steady).is_empty());

    let kept = readings(&[
        ("live", 10 << 20),
        ("canvas", 6 << 20),
        ("nothing", 17 << 20),
        ("rewrites", 17 << 20),
        ("freed-in-order", 29 << 20),
        ("freed-in-reverse", 17 << 20),
    ]);
    assert_eq!(
        judge("one-page", &kept),
        [format!(
            "one-page: {} bytes resident with the doubled render held after freed-in-order, \
             against {} after nothing",
            29 << 20,
            17 << 20
        )]
    );

    let recycled = readings(&[
        ("live", 10 << 20),
        ("canvas", 6 << 20),
        ("nothing", 12 << 20),
        ("rewrites", 12 << 20),
        ("freed-in-order", 12 << 20),
        ("freed-in-reverse", 12 << 20),
    ]);
    assert_eq!(
        judge("one-page", &recycled).len(),
        1,
        "the canvas is not in it"
    );
}

#[test]
fn a_report_is_found_after_the_harness_began_its_line() {
    let stdout = "running 1 test\ntest doubled_peak::doubled_peak_child ... \
                  doubled-resident one-page live=10 canvas=6 nothing=17\nok\n";
    let found = reported(stdout, "one-page").expect("the report");
    assert_eq!(found.get("live"), Some(&10));
    assert_eq!(found.get("nothing"), Some(&17));
    assert_eq!(reported(stdout, "many-pages"), None);
}
