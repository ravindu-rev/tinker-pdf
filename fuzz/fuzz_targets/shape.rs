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
