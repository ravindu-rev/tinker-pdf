//! OpenType Layout: `GDEF`, `GSUB` and `GPOS` executed against a glyph run.
//!
//! The repository's **twenty-fifth** target, and the one
//! `docs/design/shaping.md` names as the mitigation for its own untrusted-
//! input risk — *"malformed `GSUB`/`GPOS` from untrusted faces (cycles,
//! out-of-range offsets)"* — with the design requiring it to land in the same
//! milestone as the parser rather than after it.
//!
//! The surface is unusual in this tree and worth stating plainly. A layout
//! table is not a stream to be decoded once; it is a **graph of 16-bit
//! offsets that the font chooses**, walked repeatedly, with lookups that name
//! other lookups by index and subtables that name other subtables by 32-bit
//! offset. A file can therefore be small, valid-looking and unbounded, and
//! the three ways it can be — recursion, indirection and growth — are what
//! `tinker_pdf_shape::Limits` exists to stop.
//!
//! ## What the input is
//!
//! Two bytes of control, a glyph run, and the rest as a face. The face half
//! is driven **two ways**, because the two reach different code:
//!
//! 1. **As an sfnt**, through `Sfnt::parse` and `Layout::parse`. This is a
//!    real face's path, and a mutator that finds a table directory explores
//!    every table behind it.
//! 2. **As each of the three tables directly**, through
//!    `Layout::from_tables`. Without this the table parsers are reachable
//!    only behind a valid twelve-byte directory with a plausible checksum, and
//!    a mutator would spend its whole session on the container.
//!
//! The control byte picks the **limits** rather than the input, for the reason
//! `css` and `layout` already record: a target whose ceilings are all at
//! their shipped defaults never explores a refusal, because a 500 000-
//! operation budget cannot be crossed inside one iteration of anything a
//! mutator will build.
//!
//! ## What is asserted beyond "it did not panic"
//!
//! - **The buffer never exceeds the glyph ceiling.** Not "was refused
//!   afterwards" — never exceeded, which is the difference between a budget
//!   and a post-mortem.
//! - **Clusters never go backwards, and none is invented.** Every cluster in
//!   the output was a cluster in the input, in order. This is the property
//!   milestone 7's `/ToUnicode` is built on: after a ligature there is
//!   nothing left but the cluster to say which characters went in, and a
//!   shaper that renumbered would produce text extraction that is plausible
//!   and wrong.
//! - **Shaping is deterministic**, which is ruling 4 asserted on an
//!   algorithm. Not free: the warning list is built by scanning for
//!   duplicates, and the lookup list is sorted.
//! - **A warning is recorded once.** A hostile face can make one refusal fire
//!   a hundred thousand times, and a list that grew with it would be the
//!   denial of service the budgets were added to prevent.
//! - **`lookups_for` is sorted and deduplicated**, whatever order the feature
//!   tags arrive in and however many features name one lookup — because a
//!   lookup run twice ligates the ligature.
//! - **Every accessor answers rather than panicking**, for every glyph, on
//!   every coverage and class-definition table the face carries.

//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**: shaping the same input twice
//! gives the same answer, `lookups_for` returns a sorted list without
//! repeats, clusters do not go backwards, and the glyph ceiling is refused
//! rather than exceeded. None of them asks whether the numbers are the ones
//! the input actually describes, so a decode that is well-formed and *wrong*
//! passes this target exactly as a correct one does.
//!
//! Determinism is the weakest of those and the easiest to misread: a shaper
//! that is consistently wrong is perfectly deterministic. Correctness lives
//! in `crates/tinker-pdf-shape/tests/aots.rs`, `text_rendering.rs` and
//! `fingerprints.rs`, which compare the glyphs that come out.
//!
//! This is the shape `brotli` records at length, and the difference is worth
//! keeping in view: there, nothing in the tree can supply the missing check
//! at all — rule 1 leaves no encoder to round-trip against and ruling 13 bars
//! a second decoder. Here the check exists, and it is somewhere else.
//!
//! # The first session, the hang it found, and the session after the fix
//!
//! **4 September 2026: 24 115 executions in 78 seconds — 309 a second — and
//! a timeout.** The corpus went from 8 seeds to 893 before it stopped.
//! `rustlang/rust:nightly`, rustc 1.100.0-nightly (a69a63265 2026-09-03),
//! cargo-fuzz 0.13.2, one core, on `x86_64-unknown-linux-gnu` in Docker
//! because libFuzzer does not build for `x86_64-pc-windows-msvc`. The session
//! was asked for 600 seconds and used 78.
//!
//! **Fixed, and the session re-run to completion.** Ruling 1's property is
//! "did not panic, hang, or exhaust memory", and this was the middle one: a
//! single 2 743-byte input ran for more than 28 seconds against a `-timeout`
//! of 25, on a target whose median execution takes about three milliseconds.
//! `cargo fuzz tmin` reduced it to **1 158 bytes**, sha256
//! `bc9832fe15aab8c359f56bcde6c63f5ba59854ee25e719ee9614381f819d3b9a`, and
//! could go no further; every minimisation step has to wait out the timeout,
//! which is why it stopped there rather than at something small enough to
//! paste into this header the way `pki_der`'s 42 bytes are. It is committed
//! instead, as `fuzz/corpus/shape/lookup-list-subtable-storm`.
//!
//! Where the time goes was then measured directly, on the host, in release
//! and without the sanitizer instrumentation the fuzz build carries. **The
//! whole input costs 3.37 seconds there**, and it is not spread out:
//!
//! - `Sfnt::parse` takes 3.1 µs and succeeds, but `Layout::parse` off the
//!   parsed face finds **neither `GSUB` nor `GPOS`**, so pass 1 — the face
//!   path, all three scripts — costs single-digit microseconds in total. It
//!   contributes nothing.
//! - **Pass 2 is all of it.** `Layout::from_tables(face, face, face)` reads
//!   the same 1 150 bytes *as* a lookup list and finds **174 lookups**;
//!   `substitute` over them takes 1.55 s and `position` 1.82 s. And the
//!   target runs `shape` twice there to compare the results, so the input
//!   costs about twice that per execution. The container's instrumented build
//!   is the rest of the distance to 28 seconds.
//!
//! Only **three glyphs** are being shaped, and the two control bytes decode
//! to the loosest ceilings the knobs offer: `max_operations` 500 000 and
//! `max_glyphs` 65 536, with `max_context_length` 64.
//!
//! One structural fact, read from `apply.rs` rather than inferred, and it was
//! the whole of it: the operation budget is per shaping run — `Runner::new`
//! sets `ops: 0` once per `substitute` or `position` — and `apply_at` charged
//! exactly **one** operation and then looped `for sub in 0..lookup.len()`
//! over every subtable of the lookup. So the budget bounded the number of
//! *attempts* and not the work each attempt did, while `subTableCount` is a
//! 16-bit field the input chooses. On this input the ceiling of half a
//! million was never reached at all: the run finished in its own time, having
//! grown three glyphs to 9 201.
//!
//! The fix is that the unit is the **subtable** — what is actually resolved
//! through any extension indirection and searched — with the attempt's own
//! charge paying for the first of them, so a lookup with one subtable costs
//! exactly one as before and no well-formed face's accounting moves. Measured
//! the same way on the machine that made the fix — where the *unfixed* code
//! costs 2.81 s for one pass rather than the 3.37 s above, which is what a
//! differently loaded host measures — the same input now costs **2.63 ms**.
//! It records `OperationBudgetExceeded` for both tables where it previously
//! recorded neither, and is driven by
//! `the_lookup_list_storm_reaches_the_operation_ceiling_and_returns` in
//! `crates/tinker-pdf-shape/tests/synthetic.rs` — with no clock in it, for
//! the reason `crates/tinker-pdf/tests/bounds_ledger.rs` gives.
//!
//! **The re-run, 4 September 2026: 24 586 executions in 601 seconds — 40 a
//! second — and clean.** No artifact, and `slowest_unit_time_sec` 0 against
//! the same `-timeout=25`, so nothing came within a second of the limit that
//! stopped the first session. The corpus went from 9 seeds to 1 017, 1 275
//! units added; peak RSS 506 MB. Same image and toolchain as above, one core.
//!
//! **40 a second is the slowest rate this repository has recorded, and it is
//! the fix showing up in the number rather than a second defect.** The first
//! session managed 309 because it spent 78 seconds and then stopped on one
//! input; this one spent the whole budget on a corpus that grew to a thousand
//! mutated lookup lists, and every one of those now runs *to* its ceiling
//! instead of running until somebody gives up. Half a million subtable
//! searches, over two tables, two runs and three scripts, is real work — tens
//! of milliseconds an execution under the sanitizer. So the ceiling is doing
//! what it is for, and whether half a million is the right number for a
//! shaping run is a separate question this session does not answer: ruling 3
//! says a ceiling moves on evidence, and the evidence here is that no input
//! reached a second, not that the ceiling is tight.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::{Buffer, Layout, Limits, MarkWidths, ShapedGlyph, Tag};

/// The feature tags asked for, in an order that is not sorted, so that
/// `lookups_for`'s own sorting is exercised rather than inherited.
const FEATURES: [Tag; 6] = [
    Tag::new(b"test"),
    Tag::new(b"liga"),
    Tag::new(b"ccmp"),
    Tag::new(b"kern"),
    Tag::new(b"mark"),
    Tag::new(b"mkmk"),
];

/// The scripts tried, including one no face declares so the `DFLT` fallback
/// is on the path an ordinary input takes.
const SCRIPTS: [Tag; 3] = [
    Tag::new(b"latn"),
    Tag::new(b"arab"),
    Tag::new(b"zzzz"),
];

/// Runs both tables over one buffer and returns what came out.
fn shape(
    layout: &Layout<'_>,
    glyphs: &[u16],
    limits: Limits,
    script: Tag,
    marks: MarkWidths,
) -> Vec<ShapedGlyph> {
    let mut buffer = Buffer::from_glyphs(glyphs);
    for at in 0..buffer.len() {
        if let Some(glyph) = buffer.glyph_mut(at) {
            glyph.x_advance = 1000;
        }
    }

    if let Some(gsub) = layout.gsub() {
        let lookups = gsub.lookups_for(script, None, &FEATURES);
        assert!(
            lookups.windows(2).all(|pair| pair[0] < pair[1]),
            "lookups_for returned an unsorted or repeating list"
        );
        let warnings = layout.substitute(&mut buffer, &lookups, limits);
        check_warnings(&warnings);
        check_buffer(&buffer, glyphs, limits);
    }
    if let Some(gpos) = layout.gpos() {
        let lookups = gpos.lookups_for(script, None, &FEATURES);
        assert!(
            lookups.windows(2).all(|pair| pair[0] < pair[1]),
            "lookups_for returned an unsorted or repeating list"
        );
        let warnings = layout.position(&mut buffer, &lookups, limits, marks);
        check_warnings(&warnings);
        check_buffer(&buffer, glyphs, limits);
    }
    buffer.glyphs().to_vec()
}

fn check_warnings(warnings: &[tinker_pdf_shape::Warning]) {
    for (index, warning) in warnings.iter().enumerate() {
        assert!(
            !warnings[..index].contains(warning),
            "a warning was recorded twice instead of once"
        );
    }
}

/// The invariants a caller of this crate is entitled to rely on.
fn check_buffer(buffer: &Buffer, input: &[u16], limits: Limits) {
    // The ceiling bounds *growth*, so a run that arrived longer than the
    // ceiling is not a violation of it — nothing may make it longer still.
    assert!(
        buffer.len() <= limits.max_glyphs.max(input.len()),
        "the glyph ceiling was exceeded rather than refused"
    );
    let mut previous = None::<u32>;
    for glyph in buffer.glyphs() {
        if let Some(previous) = previous {
            assert!(
                glyph.cluster >= previous,
                "clusters went backwards: no lookup in this milestone reorders"
            );
        }
        previous = Some(glyph.cluster);
        assert!(
            usize::try_from(glyph.cluster).is_ok_and(|at| at < input.len()),
            "a cluster that was never in the input"
        );
    }
}

/// Asks every table accessor about every glyph the run mentions, so the
/// refusal paths are walked as often as the acceptance paths.
fn interrogate(layout: &Layout<'_>, glyphs: &[u16]) {
    if let Some(gdef) = layout.gdef() {
        for glyph in glyphs {
            let _ = gdef.glyph_class(*glyph);
            let _ = gdef.mark_attach_class(*glyph);
            if let Some(attachments) = gdef.attachments() {
                let _ = attachments.points(*glyph);
                let _ = attachments.coverage().map(|c| c.covers(*glyph));
            }
            if let Some(carets) = gdef.ligature_carets() {
                for caret in carets.carets(*glyph) {
                    if let tinker_pdf_shape::Caret::Coordinate { device, .. } = caret {
                        if let Some(device) = device {
                            let _ = device.delta(0);
                            let _ = device.delta(u16::MAX);
                            let _ = device.variation_index();
                        }
                    }
                }
            }
        }
        for set in 0..gdef.mark_glyph_set_count().min(64) {
            if let Some(coverage) = gdef.mark_glyph_set(set) {
                for glyph in glyphs {
                    let _ = coverage.covers(*glyph);
                }
            }
        }
        let _ = gdef.item_variation_store();
    }

    for table in [layout.gsub(), layout.gpos()].into_iter().flatten() {
        let scripts = table.scripts();
        for index in 0..scripts.len().min(64) {
            let _ = scripts.tag(index);
            let Some(script) = scripts.get(index) else {
                continue;
            };
            for lang in 0..script.len().min(64) {
                let _ = script.tag(lang);
                let Some(lang) = script.get(lang) else {
                    continue;
                };
                let _ = lang.required_feature();
                for feature in 0..lang.len().min(64) {
                    let _ = lang.feature(feature);
                }
            }
        }
        let features = table.features();
        for index in 0..features.len().min(64) {
            let _ = features.tag(index);
            if let Some(feature) = features.get(index) {
                for lookup in 0..feature.len().min(64) {
                    let _ = feature.lookup(lookup);
                }
            }
        }
        let lookups = table.lookups();
        for index in 0..lookups.len().min(64) {
            let Some(lookup) = lookups.get(index) else {
                continue;
            };
            let _ = lookup.kind();
            let _ = lookup.flags();
            let _ = lookup.mark_filtering_set();
            for sub in 0..lookup.len().min(64) {
                let _ = lookup.subtable(sub);
            }
        }
        if let Some(variations) = table.feature_variations() {
            for index in 0..variations.len().min(64) {
                if let Some(conditions) = variations.conditions(index) {
                    for condition in 0..conditions.len().min(64) {
                        let _ = conditions.get(condition);
                    }
                }
                if let Some(substitutions) = variations.substitutions(index) {
                    for at in 0..substitutions.len().min(64) {
                        let _ = substitutions.get(at);
                    }
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (control, rest) = data.split_at(data.len().min(2));
    let knobs = control.first().copied().unwrap_or(0);
    let more = control.get(1).copied().unwrap_or(0);

    // Small enough that every ceiling is crossable inside one iteration, and
    // varied enough that both sides of each are reachable from one corpus.
    let limits = Limits {
        max_nesting_depth: u32::from(knobs & 3),
        max_extension_depth: u32::from((knobs >> 2) & 3),
        max_context_length: match (knobs >> 4) & 3 {
            0 => 1,
            1 => 4,
            2 => 64,
            _ => 4_096,
        },
        max_operations: match (knobs >> 6) & 3 {
            0 => 0,
            1 => 16,
            2 => 4_096,
            _ => 500_000,
        },
        max_glyphs: match more & 3 {
            0 => 1,
            1 => 8,
            2 => 512,
            _ => 65_536,
        },
    };
    let marks = if more & 4 == 0 {
        MarkWidths::ZeroByGdef
    } else {
        MarkWidths::AsSupplied
    };

    // The glyph run: as many glyphs as the third control nibble asks for,
    // read from the front of the body. Small on purpose — the interesting
    // failures are in the table walk, not in how many glyphs it walks over.
    let count = usize::from(more >> 3);
    let glyphs: Vec<u16> = rest
        .chunks_exact(2)
        .take(count)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let face = rest.get(count.saturating_mul(2)..).unwrap_or(&[]);

    // 1. The bytes as a face, which is the path a real font takes.
    if let Some(sfnt) = Sfnt::parse(face) {
        let layout = Layout::parse(&sfnt);
        interrogate(&layout, &glyphs);
        for script in SCRIPTS {
            let once = shape(&layout, &glyphs, limits, script, marks);
            let twice = shape(&layout, &glyphs, limits, script, marks);
            assert!(once == twice, "shaping is not deterministic");
        }
    }

    // 2. The same bytes as each of the three tables, so a malformed lookup
    //    list is reachable without a valid directory in front of it.
    let layout = Layout::from_tables(Some(face), Some(face), Some(face));
    interrogate(&layout, &glyphs);
    let once = shape(&layout, &glyphs, limits, SCRIPTS[0], marks);
    let twice = shape(&layout, &glyphs, limits, SCRIPTS[0], marks);
    assert!(once == twice, "shaping is not deterministic");
});
