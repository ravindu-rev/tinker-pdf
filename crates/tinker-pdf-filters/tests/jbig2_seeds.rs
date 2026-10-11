//! The committed JBIG2 fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/jbig2/` is twenty-six inputs a fuzzer found or a person
//! wrote,
//! and the target that consumes them needs nightly. So the seeds were only ever
//! exercised when somebody ran `cargo fuzz`, which is not on every commit — and
//! a seed corpus nothing reads is a corpus that stops describing the decoder
//! without anybody noticing.
//!
//! This replays each one through the same split and the same assertions the
//! target makes, minus the mutation. It is not fuzzing and does not pretend to
//! be: it is a regression test over inputs that were once interesting, which is
//! what a seed corpus is. It prints `RAN` or `SKIPPED` for the reason every
//! check that can be absent does ([verification](../../../docs/verification.md)).
//!
//! **This test used to take about twelve seconds in a debug build and seven of
//! them were one seed**, `symbol-dictionary-spends-the-pixel-budget`, which is
//! the input the 20 September 2026 CI run timed out on. It spent
//! `MAX_JBIG2_SYMBOL_PIXELS` in full — 67 219 222 decoded pixels across 546
//! symbols, from 105 bytes — on a page of one pixel by one pixel, and that
//! total budget was the only thing that ever stopped it.
//!
//! **It takes ten milliseconds now**, because
//! `MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` refuses that dictionary at its *first*
//! symbol: 69 pixels wide against a page one pixel wide. That one seed went
//! from 6.49 s to 0.99 ms in a debug build and from 564 ms to 23.5 µs in
//! release.
//! [`the_pixel_budget_seed_is_refused_at_its_first_symbol`] pins that by its
//! cause rather than by its duration, which is the whole discipline here — a
//! budget proved by a clock passes on a fast machine with the budget removed.
//! `docs/verification.md` records why the total budget is not lowered instead.
//!
//! *Amended, October 2026.* **And the shape that bound left open came back**,
//! as `symbol-dictionary-spun-past-its-marker`: a page of 37 by 56 is large
//! enough for a symbol a hundred pixels wide, and 257 bytes asked for 38 730 of
//! them. It is held the same way, by its cause:
//! [`the_marker_spun_seed_stops_at_the_work_budget`] names the refusal and
//! counts the symbols. It is the one seed here that spends a whole budget.
//! `symbol-dictionary-template-0-custom-at` is the same sweep's slow unit, a
//! dictionary of 3 072 symbols at template 0 with its adaptive pixels off the
//! figure's places, which stays under the budget and was slow for what each
//! pixel cost; the replay holds it to the refusal like every other seed, and
//! `a_decoded_pixel_costs_the_reads_its_rolled_context_needs` holds the cost.
//! The whole replay is about a second and a half in a debug build.
//!
//! `symbol-dictionary-export-runs-of-nothing` is the review of that budget's:
//! twenty million export runs of zero, which the arithmetic road's loop decided
//! uncharged and unguarded.
//! [`the_export_run_seed_stops_at_the_turn_guard`] pins it by the guard's
//! name and the decode's own count.

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{
    jbig2_decode, jbig2_decode_measured, Capability, FilterError, Jbig2Params, Jbig2Refusal,
    MAX_JBIG2_SYMBOLS, MAX_JBIG2_WORK_MULTIPLE,
};

/// The seed directory, from this crate rather than from the working directory.
fn seeds() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/jbig2");
    dir.is_dir().then_some(dir)
}

/// The target's own reading of its first byte (`fuzz/fuzz_targets/jbig2.rs`),
/// restated so a seed means here exactly what it means there.
fn knobs(data: &[u8]) -> (u32, u32, &[u8], &[u8]) {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
    let width = match knobs & 3 {
        0 => 1,
        1 => 8,
        2 => 37,
        _ => 64,
    };
    let height = match (knobs >> 2) & 3 {
        0 => 1,
        1 => 8,
        2 => 56,
        _ => 61,
    };
    let split = match (knobs >> 4) & 3 {
        0 => 0,
        1 => body.len() / 4,
        2 => body.len() / 2,
        _ => body.len(),
    };
    let (globals, own) = body.split_at(split.min(body.len()));
    (width, height, globals, own)
}

/// **Every committed seed still decodes to a page or to the refusal**, and to
/// nothing else.
///
/// The two assertions are the contract the render path rests on: a decode that
/// succeeds returns exactly the packed page its caller sized, because the
/// caller indexes it by row without measuring it again; and a decode that fails
/// fails with the named capability, because that refusal is the whole of this
/// codec's degradation contract (rulings 2 and 3).
#[test]
fn every_committed_seed_decodes_or_refuses_by_name() {
    let Some(dir) = seeds() else {
        println!("jbig2-seeds: SKIPPED (no fuzz/corpus/jbig2)");
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the seed directory reads")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "the seed directory is empty");

    for path in &paths {
        let data = std::fs::read(path).expect("a seed reads");
        let (width, height, globals, own) = knobs(&data);
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals,
            width,
            height,
            work_ceiling: None,
        };
        let ceiling = 1 << 16;
        match jbig2_decode(own, &params, ceiling, &mut warnings) {
            Ok(page) => {
                let stride = (width as usize).div_ceil(8);
                assert_eq!(
                    page.len(),
                    stride * height as usize,
                    "{name}: a successful decode returned a page that is not \
                     the size its caller asked for"
                );
                assert!(
                    page.len() <= ceiling,
                    "{name}: the output ceiling was exceeded"
                );
            }
            Err(error) => assert_eq!(
                error,
                FilterError::Unsupported(Capability::Jbig2),
                "{name}: failed with something other than the refusal"
            ),
        }

        // The warning set is closed and each variant is recorded at most once
        // per decode, so a stream of a million bad segments cannot turn
        // leniency into an allocation attack.
        let mut seen = warnings.clone();
        seen.sort_by_key(|w| format!("{w:?}"));
        seen.dedup_by_key(|w| format!("{w:?}"));
        assert_eq!(
            seen.len(),
            warnings.len(),
            "{name}: a warning was recorded more than once"
        );
    }

    println!("jbig2-seeds: RAN over {} committed seeds", paths.len());
}

/// **The seed the 20 September 2026 fuzz run timed out on, refused at its
/// first symbol.**
///
/// The regression guard for `docs/verification.md`'s `jbig2` row, and it is
/// deliberately not a timing assertion. What made that input twenty seconds
/// under instrumentation was 67 219 222 decoded pixels across 546 symbols, and
/// what makes it a millisecond now is that the **first** of those symbols is
/// 69 pixels wide against a page one pixel wide — so the thing worth pinning is
/// the symbol count and the name of the refusal, not the clock.
///
/// Every figure here is read out of the seed rather than asserted about it:
/// `symbols` is 1 because the dictionary stops there, `widest` is that symbol's
/// own width, and `SymbolLargerThanPage` is the reason. A change that let the
/// dictionary run on would move `symbols` off 1 whether or not the machine
/// running this test was fast enough to hide it.
#[test]
fn the_pixel_budget_seed_is_refused_at_its_first_symbol() {
    let Some(dir) = seeds() else {
        println!("jbig2-seed-budget: SKIPPED (no fuzz/corpus/jbig2)");
        return;
    };
    let path = dir.join("symbol-dictionary-spends-the-pixel-budget");
    let data = std::fs::read(&path).expect("the seed reads");
    assert_eq!(data.len(), 106, "one control byte and 105 of payload");
    let (width, height, globals, own) = knobs(&data);
    assert_eq!(
        (width, height),
        (1, 1),
        "the seed's first byte chooses a one-pixel page, which is the whole \
         finding: 67 million decoded pixels for a page that holds one"
    );

    let params = Jbig2Params {
        globals,
        width,
        height,
        work_ceiling: None,
    };
    let mut refusals = Vec::new();
    let (out, extent) = jbig2_decode_measured(own, &params, 1 << 16, &mut refusals);
    assert!(out.is_err(), "the seed decoded to a page");
    assert!(
        refusals.contains(&Jbig2Refusal::SymbolLargerThanPage),
        "the seed is no longer refused for being larger than its page: \
         {refusals:?}"
    );
    assert!(
        !refusals.contains(&Jbig2Refusal::SymbolPixelCap),
        "the total pixel budget is what stopped it again, which is the row \
         this test closes: {refusals:?}"
    );
    assert_eq!(
        (extent.symbols, extent.widest, extent.tallest),
        (1, 69, 1),
        "the seed's first symbol is 69 by 1 and the dictionary stops there"
    );
    println!(
        "jbig2-seed-budget: RAN; refused at symbol {} of {}x{}",
        extent.symbols, extent.widest, extent.tallest
    );
}

/// **The seed the October 2026 fuzz sweep timed out on, stopped by the work
/// budget.**
///
/// `MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` closed the one-pixel page and nothing else:
/// on a page of 37 by 56 a symbol may be 148 by 224, and this seed's dictionary
/// declares a count its coded data stops describing after 945 decisions. The
/// coder then reaches a marker and E.3.4 has it feed itself 1-bits, which it
/// goes on deciding as symbols — **38 730 of them, 51 532 206 pixels**, from
/// 257 bytes, under every per-dictionary bound there is, twice per fuzz input,
/// at twenty seconds a time under the sanitizers.
///
/// What stops it is `MAX_JBIG2_WORK_MULTIPLE`, and that is what is pinned: the
/// refusal by name, the decode's own count of what it spent, and the symbol
/// count it got to — a few thousand rather than 38 730. None of it is a clock:
/// a change that let the dictionary run on would move the symbol count and lose
/// the refusal on any machine.
#[test]
fn the_marker_spun_seed_stops_at_the_work_budget() {
    let Some(dir) = seeds() else {
        println!("jbig2-seed-work: SKIPPED (no fuzz/corpus/jbig2)");
        return;
    };
    let path = dir.join("symbol-dictionary-spun-past-its-marker");
    let data = std::fs::read(&path).expect("the seed reads");
    assert_eq!(data.len(), 257, "one control byte and 256 of payload");
    let (width, height, globals, own) = knobs(&data);
    assert_eq!(
        (width, height),
        (37, 56),
        "the seed's first byte chooses a page the symbol-page bound admits a \
         hundred-pixel symbol on, which is why that bound did not stop it"
    );

    // The fuzz target's own ceiling, and so its own budget.
    let ceiling = 1 << 16;
    let budget = ceiling as u64 * 8 * MAX_JBIG2_WORK_MULTIPLE;
    for (label, globals, own) in [("straight", globals, own), ("swapped", own, globals)] {
        let params = Jbig2Params {
            globals,
            width,
            height,
            work_ceiling: None,
        };
        let mut refusals = Vec::new();
        let (out, extent) = jbig2_decode_measured(own, &params, ceiling, &mut refusals);
        assert!(out.is_err(), "{label}: the seed decoded to a page");
        assert!(
            refusals.contains(&Jbig2Refusal::WorkCap),
            "{label}: the work budget is no longer what stops this seed: \
             {} symbols, {} units, {refusals:?}",
            extent.symbols,
            extent.work
        );
        // Past the budget — so it was the budget, and not something smaller,
        // that stopped it — and by at most the one symbol whose charge was
        // refused, which the symbol-page bound keeps under 148 x 224 here.
        assert!(
            extent.work > budget && extent.work <= budget + 148 * 224,
            "{label}: {} units against a budget of {budget}",
            extent.work
        );
        assert!(
            extent.symbols < 38_730 / 4,
            "{label}: the dictionary got to symbol {} of the 38 730 it asks for",
            extent.symbols
        );
        println!(
            "jbig2-seed-work: RAN {label}; stopped at symbol {} after {} units of {budget}",
            extent.symbols, extent.work
        );
    }
}

/// **The input the review of the work budget timed out on, stopped at the
/// export runs' turn guard.**
///
/// A two-symbol dictionary whose 6.5.10 export runs are one, then twenty
/// million runs of zero, then one: 400 bytes, written through the decoder's own
/// test encoder. A run of zero is legal and makes no progress, and the
/// arithmetic road's loop was charged once after it ended and had no guard on
/// its turns, so the fuzz build decided eighty million times before the budget
/// heard of any of it — 12.8 s for this input, and a timeout for one five times
/// as long. Both roads now stop at `MAX_JBIG2_SYMBOLS` turns, and the budget is
/// settled on each of them underneath that. Pinned by the refusal's name and by
/// the decode's own count of what it spent, both ways round.
#[test]
fn the_export_run_seed_stops_at_the_turn_guard() {
    let Some(dir) = seeds() else {
        println!("jbig2-seed-export: SKIPPED (no fuzz/corpus/jbig2)");
        return;
    };
    let path = dir.join("symbol-dictionary-export-runs-of-nothing");
    let data = std::fs::read(&path).expect("the seed reads");
    assert_eq!(data.len(), 401, "one control byte and 400 of payload");
    let (width, height, globals, own) = knobs(&data);

    for (label, globals, own) in [("straight", globals, own), ("swapped", own, globals)] {
        let params = Jbig2Params {
            globals,
            width,
            height,
            work_ceiling: None,
        };
        let mut refusals = Vec::new();
        let (out, extent) = jbig2_decode_measured(own, &params, 1 << 16, &mut refusals);
        assert!(out.is_err(), "{label}: the seed decoded to a page");
        assert!(
            refusals.contains(&Jbig2Refusal::ExportRunGuard),
            "{label}: the turn guard is no longer what stops this seed: {} units, \
             {refusals:?}",
            extent.work
        );
        // Every turn up to the guard was charged and decided at most one
        // integer, which A.2 reads in 38 decisions at the most.
        let turns = u64::from(MAX_JBIG2_SYMBOLS);
        assert!(
            extent.work >= 4 * turns && extent.work <= 38 * (turns + 1),
            "{label}: {} units for {turns} turns",
            extent.work
        );
        println!(
            "jbig2-seed-export: RAN {label}; stopped at the guard after {} units",
            extent.work
        );
    }
}
