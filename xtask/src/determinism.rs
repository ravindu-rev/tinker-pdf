//! The determinism roster (K-07): every test that holds a fingerprint across
//! targets.
//!
//! Ruling 4 is proved by tests that hash what the engine made and compare it
//! with a value committed in the tree, on every target they run on: Linux and
//! Windows in CI's tests on every pull request, and `wasm32-wasip1` locally.
//! Such a test proves nothing where it does not run, and an `#[ignore]` added
//! one afternoon leaves a green tick behind it. So each test is named here,
//! and `cargo xtask determinism` holds each to being a `#[test]` in its file
//! with no `#[ignore]`. A row that lands a fingerprint adds its entry in the
//! same commit.

use std::path::Path;

/// One cross-target fingerprint test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// Its file, from the repository root.
    pub file: &'static str,
    /// Its name.
    pub test: &'static str,
}

/// Every cross-target fingerprint test, and the counted guards held beside
/// them.
pub const ROSTER: &[Fingerprint] = &[
    // The render and document hashes, and the display list's replay of every
    // fingerprinted page.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "rendering_is_stable_across_targets",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_synthesised_document_is_the_same_bytes_on_every_target",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_synthesised_fixed_document_is_the_same_bytes_on_every_target",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_synthesised_book_is_the_same_bytes_on_every_target",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "a_display_list_replays_every_fingerprinted_page_byte_for_byte",
    },
    // Every fingerprinted page drawn twice through a warm document cache and
    // twice through a starved one, held to its fingerprint (P-16).
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "warm_and_starved_caches_render_the_fingerprints",
    },
    // The same with each budget stated at open, which charges the parsed
    // objects beside the cache: a budget of zero keeps no object (P-16.8).
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "budgets_stated_at_open_render_the_fingerprints",
    },
    // Every fingerprinted page's command stream replayed byte for byte, and
    // its bytes' hash (RC-18.4).
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "a_command_stream_replays_every_fingerprinted_page_byte_for_byte",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_command_stream_is_the_same_bytes_on_every_target",
    },
    // Every fingerprinted page's mark box, from the page analysis.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_mark_boxes_are_the_same_on_every_target",
    },
    // Every fingerprinted page drawn in four bands, held to its fingerprint.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "banded_rendering_reproduces_every_fingerprint",
    },
    // Every fingerprinted page walked and drawn in bands on 2, 3, 4 and 7
    // threads of the engine's pool, held to its fingerprint: on the caller on
    // `wasm32-wasip1`.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "parallel_and_banded_renders_are_the_fingerprints",
    },
    // An annotation edit appended and its page patched by the rectangle it
    // changed, and the same page rendered whole, at one fingerprint.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "a_patched_render_reproduces_the_edited_pages_fingerprint",
    },
    // The SVG writer's output for every fingerprinted page.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "svg_output_is_stable_across_targets",
    },
    // The engine's own sRGB profile, computed on the build host by
    // `tinker-pdf-color/build.rs`: its bytes on every target (RC-19).
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_first_party_srgb_profile_is_one_byte_string_on_every_target",
    },
    // The archival packet's hash.
    Fingerprint {
        file: "crates/tinker-pdf/tests/pdfa_writer.rs",
        test: "the_archival_fixture_hashes_the_same_on_every_target",
    },
    // The shaped page's pixels.
    Fingerprint {
        file: "crates/tinker-pdf/tests/epub_shaped.rs",
        test: "the_shaped_page_renders_to_the_bytes_it_always_did",
    },
    // The render options' pinned hashes.
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "ink_output_is_pinned",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "transparent_and_premultiplied_output_is_pinned",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "hard_edged_output_is_pinned",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "levels_rules_and_widths_are_pinned",
    },
    // The forced-colour page with and without fill-to-stroke (RC-09.6).
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "forced_colour_output_is_pinned",
    },
    // A /NoZoom and /NoRotate annotation on a turned page at a magnification
    // other than 1 (RC-13.4).
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "placed_annotation_output_is_pinned",
    },
    // The bitmap toolbox's every operation over one golden: the maps, the
    // reductions, scale, subsample and conversion (RC-09.6).
    Fingerprint {
        file: "crates/tinker-pdf/tests/bitmap_toolbox.rs",
        test: "the_toolbox_is_stable_across_targets",
    },
    // MS-01's guard suite: its roster of guards, and the work counts it pins
    // for every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "every_guard_is_landed_or_owed",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "the_tally_counts_the_same_on_every_target",
    },
    // MS-01's A4 glyph-mask guard: the masks a 5 600-glyph page rasterises,
    // held on every target to its distinct keys, and its hits to 98 %.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a4_glyph_masks_rasterised_are_at_most_their_distinct_keys",
    },
    // MS-01's A1 lookup guard: the nodes one page lookup reads, held on every
    // target to its root and depth x fan-out under it.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a1_page_lookup_reads_at_most_depth_times_fan_out_nodes",
    },
    // MS-01's A1 total, A2 and A10 guards: a page loop over the 10 000-page
    // tree within 3 x 10 000 visits, every A2 leg's work linear in its pages,
    // and the allocations a page of three page loops costs flat in the pages.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_text_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_render_all_at_36_dpi_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_rewrite_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_subset_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_validation_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guard_allocations.rs",
        test: "a10_page_lookup_allocates_a_flat_amount_per_page",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guard_allocations.rs",
        test: "a10_text_allocates_a_flat_amount_per_page",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guard_allocations.rs",
        test: "a10_render_all_at_36_dpi_allocates_a_flat_amount_per_page",
    },
    // MS-01's A2 editor legs: every page turned, a second document's every
    // page imported, two halves kept and saved, and every page redacted.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_rotate_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_merge_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_split_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_redaction_is_linear_in_pages",
    },
    // P-18's guards: a generated book's work at 1 000 chapters within 11 times
    // its work at 100, A2's, and every other producer's at ten times its
    // input within 11 times its work at the input.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_a_thousand_chapter_book_costs_at_most_eleven_times_a_hundred",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/producer_work.rs",
        test: "every_producer_is_linear_in_its_input",
    },
    // MS-01's A3 guard: the fonts a page's render and text read, at most the
    // distinct `/Font` entries its content selects a font by.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a3_fonts_resolved_per_page_are_at_most_the_fonts_it_uses",
    },
    // MS-01's A5 legs: no stream decoded twice in a save, a render, a text
    // extraction, an image extraction or a merge's save.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a5_a_save_decodes_each_stream_at_most_once",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a5_a_render_decodes_each_stream_at_most_once",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a5_text_decodes_each_stream_at_most_once",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a5_image_extraction_decodes_each_stream_at_most_once",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a5_a_merge_decodes_each_stream_at_most_once",
    },
    // MS-01's A6 guard: what a merge's deduplication decodes, no byte-identical
    // copy and each differing encoding under one dictionary once.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once",
    },
    // MS-01's A7 guard, its JPEG half: a JPEG drawn at a quarter of its size
    // each way transforms at most four samples a drawn pixel, on every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/images.rs",
        test: "a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel",
    },
    // MS-01's A7 guard, its JPX half: a JPEG 2000 image drawn at a quarter of
    // its size each way synthesises two levels fewer a tile-component, on
    // every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/jpx_images.rs",
        test: "a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer",
    },
    // P-22's converter guard: the colours an image asks of its space's
    // conversion do not grow with its pixels, and what it allocates does not
    // grow with its colours (A10), on every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/image_conversion.rs",
        test: "a_converter_is_chosen_once_per_image",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/image_conversion.rs",
        test: "an_image_converts_in_allocations_flat_in_its_colours",
    },
    // P-22's transform guard: the transforms built over a document are at
    // most its distinct profile keys, and the icc page rendered twice through
    // one document is its fingerprint both times, on every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/icc_cache.rs",
        test: "transforms_built_are_at_most_the_distinct_keys",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_icc_page_rendered_twice_through_one_document_is_its_fingerprint",
    },
    // P-23's cell guard: the cells a document's fills rasterise are at most
    // their distinct keys, and the tiling page rendered twice through one
    // document is its fingerprint both times, on every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/pattern_cache.rs",
        test: "cell_rasterisations_are_at_most_the_distinct_keys",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_tiling_page_rendered_twice_through_one_document_is_its_fingerprint",
    },
    // MS-01's A8 guard: the PNG path's whole-image scratch, none, and its
    // peak with the file at most 1.2 x the image.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a8_the_png_path_holds_at_most_one_image_of_scratch",
    },
    // MS-01's A9 guard, its page half: every page walk on 2, 3, 4 and 7
    // threads is the serial walk, pixels, warnings and text, on every target,
    // `wasm32-wasip1` walking on the caller.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads",
    },
    // MS-01's A9 guard, its band half: every page drawn in bands on 2, 3, 4
    // and 7 threads is the page drawn whole, pixels and warnings, on every
    // target, `wasm32-wasip1` cutting and stitching on the caller.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads",
    },
    // MS-08's counted guard: the requests page one of the linearized fixture
    // and of its unlinearized twin makes, measured once and held on every
    // target beside the row's ceilings.
    Fingerprint {
        file: "crates/tinker-pdf/tests/streaming_open.rs",
        test: "a_linearized_first_page_costs_at_most_six_requests_and_never_its_main_table",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/streaming_open.rs",
        test: "the_same_pages_unlinearized_cost_their_tail_table_and_page_one_in_at_most_twelve_requests",
    },
    // The shaper's own fingerprints.
    Fingerprint {
        file: "crates/tinker-pdf-shape/tests/fingerprints.rs",
        test: "shaping_is_stable_across_targets",
    },
    Fingerprint {
        file: "crates/tinker-pdf-shape/tests/fingerprints.rs",
        test: "bidi_is_stable_across_targets",
    },
];

/// `cargo xtask determinism`: the roster against its files.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    for entry in ROSTER {
        match std::fs::read_to_string(root.join(entry.file)) {
            Ok(source) => problems.extend(unignored(&source, entry)),
            Err(e) => problems.push(format!("{}: {e}", entry.file)),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Whether `entry` is a `#[test]` in `source` with no `#[ignore]` among the
/// attributes above it, as a problem naming what is wrong.
fn unignored(source: &str, entry: &Fingerprint) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let signature = format!("fn {}(", entry.test);
    let Some(at) = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&signature))
    else {
        return Some(format!(
            "{}: the roster names `{}` and the file has no such test",
            entry.file, entry.test
        ));
    };
    // The attributes and doc comment directly above the function.
    let above: Vec<&str> = lines[..at]
        .iter()
        .rev()
        .map(|line| line.trim())
        .take_while(|line| line.starts_with("#[") || line.starts_with("///"))
        .collect();
    if !above.contains(&"#[test]") {
        return Some(format!("{}: `{}` is not a #[test]", entry.file, entry.test));
    }
    if above.iter().any(|line| line.starts_with("#[ignore")) {
        return Some(format!(
            "{}: `{}` is #[ignore]d, so nothing runs it",
            entry.file, entry.test
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A roster test must be a `#[test]` its file holds, and not `#[ignore]`d.
    #[test]
    fn a_roster_test_is_present_and_not_ignored() {
        let entry = Fingerprint {
            file: "x.rs",
            test: "a_hash_is_pinned",
        };
        let present = "/// Pinned.\n#[test]\nfn a_hash_is_pinned() {}\n";
        assert_eq!(unignored(present, &entry), None);
        let ignored = "#[test]\n#[ignore = \"slow\"]\nfn a_hash_is_pinned() {}\n";
        assert!(unignored(ignored, &entry).is_some_and(|p| p.contains("#[ignore]d")));
        let plain = "fn a_hash_is_pinned() {}\n";
        assert!(unignored(plain, &entry).is_some_and(|p| p.contains("is not a #[test]")));
        assert!(unignored("", &entry).is_some_and(|p| p.contains("no such test")));
    }

    /// The committed roster and its files agree.
    #[test]
    fn the_committed_roster_holds() {
        check(&crate::repo_root()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    }
}
