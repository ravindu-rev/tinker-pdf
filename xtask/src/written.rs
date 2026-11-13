//! `cargo xtask written`: a test that writes a document holds what it wrote
//! to the validate-on-save helper (`docs/design/validate-on-save.md`).
//!
//! The helper, `written_support`, holds a written document to the strict
//! validator and to every profile it claims. This check makes calling it the
//! rule rather than a habit: a test file, or a `src` test module, that names a
//! writer entry point and calls none of the helper's doors fails, unless it is
//! on [`PENDING`] — the files not migrated yet, a list that only shrinks.
//!
//! # The units
//!
//! Each is judged on its own text:
//!
//! - every `.rs` file under a `tests/` directory of a crate under `crates/` or
//!   `tools/`, support modules included, except the helper's own files;
//! - under `src/`, every module declared with a `#[cfg(...)]` whose predicate
//!   names `test`: an inline one from its attribute to its closing brace, the
//!   inline ones of one file read together under that file's path, and an
//!   out-of-line one as the whole file the declaration names;
//! - a file outside those that a unit includes by `#[path]` is read as part of
//!   the unit that includes it, so a generator a test takes from a bench's
//!   support module is that test's.
//!
//! A `#[cfg(test)]` item that is not a module is not a unit, nothing under
//! `xtask/`, `fuzz/`, `bindings/`, `benches/` or `examples/` is a unit of its
//! own, and no comment is read.
//!
//! # Reading a unit
//!
//! A file is lexed first ([`lex`]): every comment and the inside of every
//! string, raw-string, byte-string and character literal is blanked, and a
//! lifetime is told from a character. A writer named in a comment or inside a
//! literal is not a write, and an inline module's extent is its braces
//! counted outside literals: `line.split("//")` would cut a line at the `//`
//! of a URL inside a literal, and a `'{'` would unbalance the count.
//!
//! # What counts
//!
//! A unit writes when its code names an entry point of [`WRITERS`], the closed
//! table of the public functions that hand back a document's bytes, or calls a
//! CLI writing command ([`COMMANDS`]) through `writing::`, or as a free call
//! from `tools/tpdf/src/writing.rs`'s own test module. It calls the helper
//! when it names `written_support` — in a `mod`, a `use` or a `#[path]` — and
//! calls a door, through the module or imported from it. A fixture function
//! that shares a door's name does not count without the module.
//!
//! # Outcomes
//!
//! A unit that writes, calls no door and is not pending fails, naming the
//! entry point it matched and its line. A pending unit that calls the helper
//! fails until it is taken off the list, as `SPAWNERS` fails a row whose file
//! stopped spawning: an allowance that outlives its need is how the next one
//! gets in. So does a pending entry that names no unit or a unit that no
//! longer writes, and a list that is not sorted or names a file twice.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The units that write a document and do not call the helper yet, by path
/// from the repository root, an inline `src` test module under its file's.
///
/// Every change to this list is a deletion: a file leaves it in the commit
/// that migrates it, and nothing joins it.
pub const PENDING: &[&str] = &[
    "crates/tinker-pdf-cos/src/appearance.rs",
    "crates/tinker-pdf-cos/src/build.rs",
    "crates/tinker-pdf-cos/src/dedup.rs",
    "crates/tinker-pdf-cos/src/doc.rs",
    "crates/tinker-pdf-cos/src/edit/sanitise.rs",
    "crates/tinker-pdf-cos/src/edit/save.rs",
    "crates/tinker-pdf-cos/src/edit/tests.rs",
    "crates/tinker-pdf-cos/src/png_embed/tests.rs",
    "crates/tinker-pdf-cos/src/tiff_embed/tests.rs",
    "crates/tinker-pdf-cos/src/validate/hints.rs",
    "crates/tinker-pdf-cos/src/write.rs",
    "crates/tinker-pdf-cos/tests/cff_subsetting.rs",
    "crates/tinker-pdf-cos/tests/document.rs",
    "crates/tinker-pdf-cos/tests/editor_overlay_reads.rs",
    "crates/tinker-pdf-cos/tests/editor_structures.rs",
    "crates/tinker-pdf-cos/tests/editor_view.rs",
    "crates/tinker-pdf-cos/tests/encrypt_on_save.rs",
    "crates/tinker-pdf-cos/tests/encryption.rs",
    "crates/tinker-pdf-cos/tests/form_calculations.rs",
    "crates/tinker-pdf-cos/tests/form_document_scripts.rs",
    "crates/tinker-pdf-cos/tests/form_events.rs",
    "crates/tinker-pdf-cos/tests/form_fixture.rs",
    "crates/tinker-pdf-cos/tests/form_transactions.rs",
    "crates/tinker-pdf-cos/tests/linearized.rs",
    "crates/tinker-pdf-cos/tests/linearized_numbering.rs",
    "crates/tinker-pdf-cos/tests/owned_pages.rs",
    "crates/tinker-pdf-cos/tests/page_operations.rs",
    "crates/tinker-pdf-cos/tests/sanitise.rs",
    "crates/tinker-pdf-cos/tests/save_refusals.rs",
    "crates/tinker-pdf-cos/tests/streaming.rs",
    "crates/tinker-pdf-cos/tests/strict_validator.rs",
    "crates/tinker-pdf-cos/tests/surface_support/mod.rs",
    "crates/tinker-pdf-cos/tests/text_string_roundtrip.rs",
    "crates/tinker-pdf-cos/tests/trailer_overlay.rs",
    "crates/tinker-pdf-cos/tests/tree_writer.rs",
    "crates/tinker-pdf-cos/tests/validated_output.rs",
    "crates/tinker-pdf-ffi/src/docops/tests.rs",
    "crates/tinker-pdf-ffi/src/forms/tests.rs",
    "crates/tinker-pdf-ffi/src/graphics/tests.rs",
    "crates/tinker-pdf-ffi/src/lib.rs",
    "crates/tinker-pdf-ffi/src/raw_enum_tests.rs",
    "crates/tinker-pdf-ffi/src/read/tests.rs",
    "crates/tinker-pdf-ffi/src/tagging/tests.rs",
    "crates/tinker-pdf-ffi/tests/open_copies.rs",
    "crates/tinker-pdf/src/annots.rs",
    "crates/tinker-pdf/src/epub/tests.rs",
    "crates/tinker-pdf/src/pdfua.rs",
    "crates/tinker-pdf/src/pdfx.rs",
    "crates/tinker-pdf/src/reading_order.rs",
    "crates/tinker-pdf/src/redact.rs",
    "crates/tinker-pdf/src/resources.rs",
    "crates/tinker-pdf/tests/appearance_synthesis.rs",
    "crates/tinker-pdf/tests/bench_inputs.rs",
    "crates/tinker-pdf/tests/cbz_images.rs",
    "crates/tinker-pdf/tests/cbz_real.rs",
    "crates/tinker-pdf/tests/cff_fonts.rs",
    "crates/tinker-pdf/tests/cff_subset_census.rs",
    "crates/tinker-pdf/tests/deflate_size.rs",
    "crates/tinker-pdf/tests/determinism.rs",
    "crates/tinker-pdf/tests/epub.rs",
    "crates/tinker-pdf/tests/epub_analytic.rs",
    "crates/tinker-pdf/tests/epub_css.rs",
    "crates/tinker-pdf/tests/epub_fallback.rs",
    "crates/tinker-pdf/tests/epub_fetched.rs",
    "crates/tinker-pdf/tests/epub_float_order.rs",
    "crates/tinker-pdf/tests/epub_fonts.rs",
    "crates/tinker-pdf/tests/epub_ocf.rs",
    "crates/tinker-pdf/tests/epub_paint.rs",
    "crates/tinker-pdf/tests/epub_pseudo.rs",
    "crates/tinker-pdf/tests/epub_reading.rs",
    "crates/tinker-pdf/tests/epub_reftest.rs",
    "crates/tinker-pdf/tests/epub_structure.rs",
    "crates/tinker-pdf/tests/epub_support/mod.rs",
    "crates/tinker-pdf/tests/epub_tables.rs",
    "crates/tinker-pdf/tests/ext_gstate.rs",
    "crates/tinker-pdf/tests/facade_read.rs",
    "crates/tinker-pdf/tests/guards.rs",
    "crates/tinker-pdf/tests/image_recode.rs",
    "crates/tinker-pdf/tests/images.rs",
    "crates/tinker-pdf/tests/level_six_counts.rs",
    "crates/tinker-pdf/tests/metamorphic_classes.rs",
    "crates/tinker-pdf/tests/page_geometry.rs",
    "crates/tinker-pdf/tests/page_images.rs",
    "crates/tinker-pdf/tests/pdfa_fonts.rs",
    "crates/tinker-pdf/tests/pdfua.rs",
    "crates/tinker-pdf/tests/pdfua_rules.rs",
    "crates/tinker-pdf/tests/pdfx_rules.rs",
    "crates/tinker-pdf/tests/peak_memory.rs",
    "crates/tinker-pdf/tests/png_output.rs",
    "crates/tinker-pdf/tests/png_passthrough.rs",
    "crates/tinker-pdf/tests/reading_order.rs",
    "crates/tinker-pdf/tests/reading_order_census.rs",
    "crates/tinker-pdf/tests/render_analytic.rs",
    "crates/tinker-pdf/tests/render_differential.rs",
    "crates/tinker-pdf/tests/render_goldens.rs",
    "crates/tinker-pdf/tests/render_options.rs",
    "crates/tinker-pdf/tests/render_parts.rs",
    "crates/tinker-pdf/tests/render_regions.rs",
    "crates/tinker-pdf/tests/render_support/mod.rs",
    "crates/tinker-pdf/tests/signature_shapes.rs",
    "crates/tinker-pdf/tests/signatures.rs",
    "crates/tinker-pdf/tests/stats.rs",
    "crates/tinker-pdf/tests/streaming_determinism.rs",
    "crates/tinker-pdf/tests/streaming_open.rs",
    "crates/tinker-pdf/tests/stroke_parameters.rs",
    "crates/tinker-pdf/tests/substitute_fonts.rs",
    "crates/tinker-pdf/tests/surface_goldens.rs",
    "crates/tinker-pdf/tests/svg_output.rs",
    "crates/tinker-pdf/tests/table_census.rs",
    "crates/tinker-pdf/tests/tables.rs",
    "crates/tinker-pdf/tests/tagged_pdf.rs",
    "crates/tinker-pdf/tests/text_logical_order.rs",
    "crates/tinker-pdf/tests/text_options.rs",
    "crates/tinker-pdf/tests/text_render_modes.rs",
    "crates/tinker-pdf/tests/tinker_parity.rs",
    "crates/tinker-pdf/tests/verdicts.rs",
    "crates/tinker-pdf/tests/word_boxes.rs",
    "crates/tinker-pdf/tests/xps.rs",
    "crates/tinker-pdf/tests/xps_markup.rs",
    "crates/tinker-pdf/tests/xps_opc.rs",
    "crates/tinker-pdf/tests/xps_resources.rs",
    "crates/tinker-pdf/tests/xps_spine.rs",
    "tools/tpdf/src/doubled_peak.rs",
    "tools/tpdf/src/images.rs",
    "tools/tpdf/src/main.rs",
    "tools/tpdf/src/writing.rs",
];

/// The helper's own files, which are never a unit and never read into one.
pub const HELPER: [&str; 3] = [
    "crates/tinker-pdf-cos/tests/written_support/held.rs",
    "crates/tinker-pdf-cos/tests/written_support/mod.rs",
    "crates/tinker-pdf/tests/written_support/mod.rs",
];

/// The helper's doors.
pub const DOORS: [&str; 4] = ["authored", "rewritten", "expecting", "synthesised"];

/// The CLI's writing commands, every one of which writes through `save_to`.
pub const COMMANDS: [&str; 8] = [
    "merge", "split", "rotate", "encrypt", "decrypt", "attach", "stamp", "sanitise",
];

/// The file whose own test module calls [`COMMANDS`] as free functions.
const CLI_WRITING: &str = "tools/tpdf/src/writing.rs";

/// One entry point, as code names it.
pub struct Writer {
    /// What a problem names.
    pub entry: &'static str,
    /// What is matched in a unit's code, at identifier boundaries.
    pub needle: &'static str,
    /// For a needle that is not a writer's alone, the names one of which the
    /// unit must also carry; empty when the needle is enough.
    pub beside: &'static [&'static str],
}

/// What counts as a write: every public function that hands back a
/// document's bytes, by the name a test calls it by. `.finish(` and `.save(`
/// count only beside their writer's type, because a hand-built file's
/// `b.finish(…)` and a digest's `.finish()` are not writers. A conversion
/// writes inside `Document::open`, a name every reading test shares, so it is
/// recognised by what only a conversion test names.
pub const WRITERS: &[Writer] = &[
    Writer {
        entry: "DocumentBuilder::finish_reporting",
        needle: ".finish_reporting(",
        beside: &[],
    },
    Writer {
        entry: "DocumentBuilder::finish_archival",
        needle: ".finish_archival(",
        beside: &[],
    },
    Writer {
        entry: "DocumentBuilder::finish",
        needle: ".finish(",
        beside: &["DocumentBuilder"],
    },
    Writer {
        entry: "save_signed",
        needle: ".save_signed(",
        beside: &[],
    },
    Writer {
        entry: "save_timestamped",
        needle: ".save_timestamped(",
        beside: &[],
    },
    Writer {
        entry: "DocumentEditor::save_sealed",
        needle: ".save_sealed(",
        beside: &[],
    },
    Writer {
        entry: "DocumentEditor::save",
        needle: ".save(",
        beside: &["DocumentEditor", ".editor()"],
    },
    Writer {
        entry: "tinker_pdf::write::save",
        needle: "write::save",
        beside: &[],
    },
    Writer {
        entry: "tinker_pdf::write::SaveOptions",
        needle: "SaveOptions",
        beside: &[],
    },
    Writer {
        entry: "write::incremental_update",
        needle: "incremental_update(",
        beside: &[],
    },
    Writer {
        entry: "linearize::linearize",
        needle: "linearize::linearize",
        beside: &[],
    },
    Writer {
        entry: "tpdf_builder_finish",
        needle: "tpdf_builder_finish",
        beside: &[],
    },
    Writer {
        entry: "tpdf_editor_save",
        needle: "tpdf_editor_save",
        beside: &[],
    },
    Writer {
        entry: "Document::open_markdown",
        needle: "open_markdown",
        beside: &[],
    },
    Writer {
        entry: "Document::archive, a conversion's report",
        needle: ".archive()",
        beside: &[],
    },
    Writer {
        entry: "epub_support, a conversion's fixtures",
        needle: "epub_support",
        beside: &[],
    },
    Writer {
        entry: "xps_support, a conversion's fixtures",
        needle: "xps_support",
        beside: &[],
    },
    Writer {
        entry: "cbz_support, a conversion's fixtures",
        needle: "cbz_support",
        beside: &[],
    },
];

/// `cargo xtask written`, over the tree at `root`.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let mut files = BTreeMap::new();
    for tree in ["crates", "tools"] {
        collect(root, &root.join(tree), &mut files, &mut problems);
    }
    problems.extend(judge(&files, PENDING));
    if problems.is_empty() {
        Ok(())
    } else {
        problems.sort();
        Err(problems)
    }
}

/// Every `.rs` file under `dir`, by its path from `root` with forward slashes.
fn collect(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<String, String>,
    problems: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out, problems);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    out.insert(shown, text);
                }
                // Never a silent skip: a file this cannot read is a file this
                // cannot vouch for.
                Err(_) => problems.push(format!(
                    "{shown}: could not be read, so it cannot be checked"
                )),
            }
        }
    }
}

/// The verdict over `files`, every source file by its path from the
/// repository root, with `pending` as the list of units not migrated yet.
pub fn judge(files: &BTreeMap<String, String>, pending: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    let sources: BTreeMap<&str, Source> = files
        .iter()
        .map(|(path, text)| (path.as_str(), Source::new(text)))
        .collect();
    let units = units(&sources, &mut problems);

    for window in pending.windows(2) {
        if window[0] >= window[1] {
            problems.push(format!(
                "`PENDING` in xtask/src/written.rs is not sorted, or names a file twice: \
                 `{}` then `{}`",
                window[0], window[1]
            ));
        }
    }

    for (path, unit) in &units {
        let listed = pending.contains(&path.as_str());
        let write = first_write(path, unit);
        let calls = calls_helper(unit);
        match (write, calls, listed) {
            (Some((file, line, entry)), false, false) => {
                let place = if file == *path {
                    format!("{path}:{line}: writes a document through `{entry}`")
                } else {
                    format!(
                        "{path}: writes a document through `{entry}` at {file}:{line}, \
                         which it includes by `#[path]`,"
                    )
                };
                problems.push(format!(
                    "{place} and holds none of it to the validate-on-save helper: call a \
                     door of `written_support` (docs/design/validate-on-save.md)"
                ));
            }
            (_, true, true) => problems.push(format!(
                "{path}: calls the validate-on-save helper and is still on `PENDING` in \
                 xtask/src/written.rs: take it off, because an allowance that outlives its \
                 need is how the next one gets in"
            )),
            (None, false, true) => problems.push(format!(
                "{path}: is on `PENDING` in xtask/src/written.rs and writes no document: \
                 take it off"
            )),
            _ => {}
        }
    }
    for entry in pending {
        if !units.contains_key(*entry) {
            problems.push(format!(
                "{entry}: is on `PENDING` in xtask/src/written.rs and names no test file \
                 or `src` test module"
            ));
        }
    }
    problems
}

// ---- the units -----------------------------------------------------------------

/// One stretch of one file's code that belongs to a unit.
struct Segment<'a> {
    file: &'a str,
    source: &'a Source,
    from: usize,
    to: usize,
}

impl<'a> Segment<'a> {
    fn code(&self) -> &'a str {
        self.source.code.get(self.from..self.to).unwrap_or_default()
    }

    /// The declarations that begin inside this stretch.
    fn declarations(&self) -> impl Iterator<Item = &'a Declaration> + '_ {
        self.source
            .declarations
            .iter()
            .filter(|decl| self.from <= decl.start && decl.start < self.to)
    }
}

/// Every unit in the tree, by the path [`PENDING`] names it by, each the
/// stretches of code it is judged on.
fn units<'a>(
    sources: &'a BTreeMap<&'a str, Source>,
    problems: &mut Vec<String>,
) -> BTreeMap<String, Vec<Segment<'a>>> {
    // The `src` files an out-of-line test module declaration names: units
    // whole, and not read again for inline test modules of their own.
    let mut module_files: BTreeSet<&'a str> = BTreeSet::new();
    for (path, source) in sources {
        if !in_src(path) {
            continue;
        }
        for decl in &source.declarations {
            if !decl.test || decl.inline.is_some() {
                continue;
            }
            match module_file(path, source, decl, sources) {
                Some(file) => {
                    module_files.insert(file);
                }
                None => problems.push(format!(
                    "{path}: declares the test module `{}`, whose file could not be found, \
                     so it cannot be checked",
                    decl.name
                )),
            }
        }
    }

    let mut units = BTreeMap::new();
    for (path, source) in sources {
        let whole = (in_tests(path) && !HELPER.contains(path)) || module_files.contains(path);
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        if whole {
            ranges.push((0, source.code.len()));
        } else if in_src(path) {
            for decl in source.declarations.iter().filter(|decl| decl.test) {
                let Some((_, close)) = decl.inline else {
                    continue;
                };
                // A test module inside another is already inside its extent.
                if !ranges.iter().any(|&(_, to)| decl.start < to) {
                    ranges.push((decl.start, close + 1));
                }
            }
        }
        if ranges.is_empty() {
            continue;
        }
        let mut segments: Vec<Segment<'a>> = ranges
            .into_iter()
            .map(|(from, to)| Segment {
                file: path,
                source,
                from,
                to,
            })
            .collect();
        fold(path, &mut segments, sources, &module_files, problems);
        units.insert((*path).to_string(), segments);
    }
    units
}

/// Reads into a unit every file outside the units that it includes by
/// `#[path]`, and what those include in turn.
fn fold<'a>(
    path: &'a str,
    segments: &mut Vec<Segment<'a>>,
    sources: &'a BTreeMap<&'a str, Source>,
    module_files: &BTreeSet<&str>,
    problems: &mut Vec<String>,
) {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    seen.insert(path.to_string());
    let mut index = 0;
    while index < segments.len() {
        let mut found = Vec::new();
        for decl in segments[index].declarations() {
            let Some(target) = decl.path.as_deref() else {
                continue;
            };
            let segment = &segments[index];
            let base = base_directory(segment.file, segment.source, decl.start, true);
            found.push((segment.file, normalise(&format!("{base}{target}"))));
        }
        index += 1;
        for (file, resolved) in found {
            let is_unit = in_tests(&resolved) || module_files.contains(resolved.as_str());
            if HELPER.contains(&resolved.as_str()) || is_unit || !seen.insert(resolved.clone()) {
                continue;
            }
            match sources.get_key_value(resolved.as_str()) {
                Some((included, source)) => segments.push(Segment {
                    file: included,
                    source,
                    from: 0,
                    to: source.code.len(),
                }),
                None => problems.push(format!(
                    "{file}: includes `{resolved}` by `#[path]`, which could not be read, \
                     so it cannot be checked"
                )),
            }
        }
    }
}

/// Whether a file is under a `tests/` directory of a crate under `crates/` or
/// `tools/`.
fn in_tests(path: &str) -> bool {
    under(path, "tests")
}

/// Whether a file is under a `src/` directory of a crate under `crates/` or
/// `tools/`.
fn in_src(path: &str) -> bool {
    under(path, "src")
}

fn under(path: &str, dir: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() >= 4 && matches!(parts[0], "crates" | "tools") && parts[2] == dir
}

/// Whether a file's own modules resolve beside it rather than under a
/// directory named after it: a crate root — `lib.rs`, `main.rs`, a binary
/// under `src/bin/`, a test binary directly under `tests/` — or a `mod.rs`.
fn is_mod_rs(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let name = parts.last().copied().unwrap_or_default();
    let crate_tree = parts.len() >= 4 && matches!(parts[0], "crates" | "tools");
    matches!(name, "lib.rs" | "main.rs" | "mod.rs")
        || (crate_tree && parts.len() == 4 && parts[2] == "tests")
        || (crate_tree && parts.len() == 5 && parts[2] == "src" && parts[3] == "bin")
}

/// The directory a declaration at `at` resolves from, ending in `/`.
///
/// The reference's rule: a `#[path]` outside any inline module resolves from
/// the file's own directory; a module without one, from there in a `mod.rs`-like
/// file and from a directory named after the file in any other; and inside
/// inline modules, from a directory for each of them, after the one named for
/// the file when the file is not `mod.rs`-like.
fn base_directory(file: &str, source: &Source, at: usize, has_path: bool) -> String {
    let mut dir = match file.rfind('/') {
        Some(slash) => file[..=slash].to_string(),
        None => String::new(),
    };
    let enclosing: Vec<&str> = source
        .declarations
        .iter()
        .filter(|decl| {
            decl.inline
                .is_some_and(|(open, close)| open < at && at < close)
        })
        .map(|decl| decl.name.as_str())
        .collect();
    if (!enclosing.is_empty() || !has_path) && !is_mod_rs(file) {
        dir.push_str(stem(file));
        dir.push('/');
    }
    for name in enclosing {
        dir.push_str(name);
        dir.push('/');
    }
    dir
}

/// The file an out-of-line module declaration names: its `#[path]`, or else
/// `name.rs` or `name/mod.rs` where the declaring file's modules resolve.
fn module_file<'a>(
    file: &str,
    source: &Source,
    decl: &Declaration,
    sources: &'a BTreeMap<&'a str, Source>,
) -> Option<&'a str> {
    let base = base_directory(file, source, decl.start, decl.path.is_some());
    let candidates = match &decl.path {
        Some(path) => vec![normalise(&format!("{base}{path}"))],
        None => vec![
            format!("{base}{}.rs", decl.name),
            format!("{base}{}/mod.rs", decl.name),
        ],
    };
    candidates
        .iter()
        .find_map(|candidate| sources.get_key_value(candidate.as_str()))
        .map(|(path, _)| *path)
}

fn stem(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.strip_suffix(".rs").unwrap_or(name)
}

/// `a/b/../c/./d` as `a/c/d`.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

// ---- what a unit does ------------------------------------------------------------

/// The first write a unit makes, in its own file before anything it includes:
/// the file and line it is on, and the entry point it names.
fn first_write(path: &str, unit: &[Segment<'_>]) -> Option<(String, usize, &'static str)> {
    let names = |name: &str| {
        unit.iter()
            .any(|segment| !find_all(segment.code(), name).is_empty())
    };
    let writers: Vec<&Writer> = WRITERS
        .iter()
        .filter(|writer| writer.beside.is_empty() || writer.beside.iter().any(|name| names(name)))
        .collect();
    for segment in unit {
        let code = segment.code();
        let mut first: Option<(usize, &'static str)> = None;
        let mut keep = |at: usize, entry: &'static str| {
            if first.is_none_or(|(earlier, _)| at < earlier) {
                first = Some((at, entry));
            }
        };
        for writer in &writers {
            if let Some(&at) = find_all(code, writer.needle).first() {
                keep(at, writer.entry);
            }
        }
        for command in COMMANDS {
            let through = format!("writing::{command}");
            if let Some(&at) = calls(code, &through).first() {
                keep(at, "a CLI writing command");
            }
            if path == CLI_WRITING && segment.file == CLI_WRITING {
                if let Some(&at) = free_calls(code, command).first() {
                    keep(at, "a CLI writing command");
                }
            }
        }
        if let Some((at, entry)) = first {
            let line = line_of(&segment.source.code, segment.from + at);
            return Some((segment.file.to_string(), line, entry));
        }
    }
    None
}

/// Whether a unit calls one of the helper's doors: it names the helper, and
/// calls a door through a module bound to it or imported from one.
fn calls_helper(unit: &[Segment<'_>]) -> bool {
    let mut modules: BTreeSet<&str> = BTreeSet::new();
    for segment in unit {
        if !find_all(segment.code(), "written_support").is_empty() {
            modules.insert("written_support");
        }
        for decl in segment.declarations() {
            if decl
                .path
                .as_deref()
                .is_some_and(|path| path.contains("written_support"))
            {
                modules.insert(decl.name.as_str());
            }
        }
    }
    if modules.is_empty() {
        return false;
    }

    let mut imported: BTreeSet<&str> = BTreeSet::new();
    for segment in unit {
        let code = segment.code();
        for module in &modules {
            if DOORS
                .iter()
                .any(|door| !calls(code, &format!("{module}::{door}")).is_empty())
            {
                return true;
            }
        }
        for statement in uses(code) {
            for module in &modules {
                let Some(&from) = find_all(statement, &format!("{module}::")).first() else {
                    continue;
                };
                let rest = &statement[from..];
                for door in DOORS {
                    if !find_all(rest, door).is_empty() || rest.contains('*') {
                        imported.insert(door);
                    }
                }
            }
        }
    }
    unit.iter().any(|segment| {
        imported
            .iter()
            .any(|door| !free_calls(segment.code(), door).is_empty())
    })
}

/// Every place `path` is called in `code`: the path, then `(` after any space.
fn calls(code: &str, path: &str) -> Vec<usize> {
    find_all(code, path)
        .into_iter()
        .filter(|&at| {
            code.get(at + path.len()..)
                .is_some_and(|rest| rest.trim_start().starts_with('('))
        })
        .collect()
}

/// Every free call of `name` in `code`: a call not after a `.`, which is a
/// method of the same name, a `::`, which is a path, or `fn`, which is its
/// definition.
fn free_calls(code: &str, name: &str) -> Vec<usize> {
    calls(code, name)
        .into_iter()
        .filter(|&at| {
            let before = code[..at].trim_end();
            !before.ends_with('.')
                && !before.ends_with("::")
                && !(before.ends_with("fn")
                    && !before[..before.len() - 2].ends_with(|c: char| is_ident(c as u8)))
        })
        .collect()
}

/// Each `use` statement in `code`, from the keyword to its `;`.
fn uses(code: &str) -> Vec<&str> {
    find_all(code, "use")
        .into_iter()
        .filter_map(|at| {
            let rest = &code[at..];
            Some(&rest[..rest.find(';')?])
        })
        .collect()
}

/// Every position of `needle` in `code` at identifier boundaries: not after an
/// identifier byte when it begins with one, and not before one when it ends
/// with one.
fn find_all(code: &str, needle: &str) -> Vec<usize> {
    let bytes = code.as_bytes();
    let first = needle.bytes().next().unwrap_or(b' ');
    let last = needle.bytes().last().unwrap_or(b' ');
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = code.get(from..).and_then(|rest| rest.find(needle)) {
        let at = from + found;
        let end = at + needle.len();
        let open = !is_ident(first) || at == 0 || !is_ident(bytes[at - 1]);
        let close = !is_ident(last) || end >= bytes.len() || !is_ident(bytes[end]);
        if open && close {
            out.push(at);
        }
        from = at + 1;
    }
    out
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// The one-based line of byte `at` of `text`.
fn line_of(text: &str, at: usize) -> usize {
    text.as_bytes()
        .get(..at)
        .map_or(0, |head| head.iter().filter(|&&b| b == b'\n').count())
        + 1
}

// ---- a file, lexed and read for its modules --------------------------------------

/// A file's code with every comment and the inside of every literal blanked to
/// spaces, line breaks kept, so a byte's offset and line are the same in the
/// code as in the file; and its `mod` declarations.
pub struct Source {
    /// The code, blanked.
    pub code: String,
    /// Each string literal's contents as the file spells them, by where they
    /// begin, in order.
    strings: Vec<(usize, String)>,
    /// Every `mod` declaration, with the attributes before it.
    declarations: Vec<Declaration>,
}

impl Source {
    fn new(text: &str) -> Source {
        let (code, strings) = lex(text);
        let mut source = Source {
            code,
            strings,
            declarations: Vec::new(),
        };
        source.declarations = declarations(&source);
        source
    }
}

/// One `mod` declaration, with the attributes before it.
struct Declaration {
    /// Where its first attribute begins, or its visibility, or the keyword.
    start: usize,
    name: String,
    /// Whether a `#[cfg(...)]` before it names `test`.
    test: bool,
    /// Its `#[path]`, when it has one.
    path: Option<String>,
    /// An inline module's braces.
    inline: Option<(usize, usize)>,
}

/// Every `mod` declaration in a lexed file.
fn declarations(source: &Source) -> Vec<Declaration> {
    let code = source.code.as_str();
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    for at in find_all(code, "mod") {
        let name_start = skip_space(bytes, at + 3);
        let mut name_end = name_start;
        while name_end < bytes.len() && is_ident(bytes[name_end]) {
            name_end += 1;
        }
        if name_end == name_start {
            continue;
        }
        let next = skip_space(bytes, name_end);
        let inline = match bytes.get(next) {
            Some(b'{') => match matching(bytes, next) {
                Some(close) => Some((next, close)),
                None => continue,
            },
            Some(b';') => None,
            _ => continue,
        };

        let mut start = back_over_visibility(bytes, at);
        let mut test = false;
        let mut path = None;
        while let Some((hash, inner)) = attribute_before(code, start) {
            let inner = inner.trim_start();
            if let Some(predicate) = inner.strip_prefix("cfg") {
                let predicate = predicate.trim_start();
                if predicate.starts_with('(') && !find_all(predicate, "test").is_empty() {
                    test = true;
                }
            } else if let Some(rest) = inner.strip_prefix("path") {
                if rest.trim_start().starts_with('=') {
                    path = source
                        .strings
                        .iter()
                        .find(|(from, _)| *from > hash)
                        .map(|(_, text)| text.clone());
                }
            }
            start = hash;
        }
        out.push(Declaration {
            start,
            name: code[name_start..name_end].to_string(),
            test,
            path,
            inline,
        });
    }
    out
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

/// Where a declaration's tokens begin once a `pub` or `pub(...)` before `at`
/// is taken in.
fn back_over_visibility(bytes: &[u8], at: usize) -> usize {
    let word_before = |end: usize| -> Option<usize> {
        let mut end = end;
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        (end >= 3 && &bytes[end - 3..end] == b"pub" && (end == 3 || !is_ident(bytes[end - 4])))
            .then(|| end - 3)
    };
    let mut end = at;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] == b')' {
        // `pub(crate)`, `pub(super)`, `pub(in path)`.
        return (0..end)
            .rev()
            .find(|&i| bytes[i] == b'(')
            .and_then(word_before)
            .unwrap_or(at);
    }
    word_before(at).unwrap_or(at)
}

/// The outer attribute that ends just before `at`, if one does: where its `#`
/// is, and the code inside its brackets.
fn attribute_before(code: &str, at: usize) -> Option<(usize, &str)> {
    let bytes = code.as_bytes();
    let mut end = at;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 || bytes[end - 1] != b']' {
        return None;
    }
    let mut depth = 0usize;
    let mut open = end - 1;
    loop {
        match bytes[open] {
            b']' => depth += 1,
            b'[' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        open = open.checked_sub(1)?;
    }
    let mut hash = open;
    while hash > 0 && bytes[hash - 1].is_ascii_whitespace() {
        hash -= 1;
    }
    if hash == 0 || bytes[hash - 1] != b'#' {
        return None;
    }
    Some((hash - 1, code.get(open + 1..end - 1)?))
}

/// The `}` that closes the `{` at `open`, counted in code, outside literals.
fn matching(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in bytes.get(open..)?.iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Lexes `text`: line comments and nested block comments, string, raw-string,
/// byte-string and C-string literals, and character and byte literals are
/// blanked to spaces, line breaks kept; a lifetime or a label is code. Hands
/// back the code and each string literal's contents, by where they begin.
pub fn lex(text: &str) -> (String, Vec<(usize, String)>) {
    let src = text.as_bytes();
    let mut code = src.to_vec();
    let mut strings = Vec::new();
    let mut blank = |code: &mut Vec<u8>, from: usize, to: usize, string: bool| {
        let to = to.min(src.len());
        if string {
            strings.push((from, String::from_utf8_lossy(&src[from..to]).into_owned()));
        }
        for byte in &mut code[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut at = 0;
    while at < src.len() {
        let byte = src[at];
        let token_start = at == 0 || !is_ident(src[at - 1]);

        if byte == b'/' && src.get(at + 1) == Some(&b'/') {
            let end = src[at..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(src.len(), |p| at + p);
            blank(&mut code, at, end, false);
            at = end;
            continue;
        }
        if byte == b'/' && src.get(at + 1) == Some(&b'*') {
            let mut depth = 0usize;
            let mut end = at;
            while end < src.len() {
                if src[end] == b'/' && src.get(end + 1) == Some(&b'*') {
                    depth += 1;
                    end += 2;
                } else if src[end] == b'*' && src.get(end + 1) == Some(&b'/') {
                    depth -= 1;
                    end += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    end += 1;
                }
            }
            blank(&mut code, at, end, false);
            at = end;
            continue;
        }

        // A literal's prefix — `b`, `c`, `r`, `br`, `cr` — counts only at the
        // start of a token, so `bar"` is no byte string.
        let prefixed = token_start && matches!(byte, b'b' | b'c');
        let quote = if prefixed { at + 1 } else { at };
        if (token_start || quote > at) && src.get(quote) == Some(&b'r') {
            let mut open = quote + 1;
            while src.get(open) == Some(&b'#') {
                open += 1;
            }
            if src.get(open) == Some(&b'"') {
                let hashes = open - quote - 1;
                let closing: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let from = open + 1;
                let to = src[from..]
                    .windows(closing.len())
                    .position(|window| window == closing.as_slice())
                    .map_or(src.len(), |p| from + p);
                blank(&mut code, from, to, true);
                at = to + closing.len();
                continue;
            }
        }
        if src.get(quote) == Some(&b'"') {
            let from = quote + 1;
            let mut end = from;
            while end < src.len() && src[end] != b'"' {
                end += if src[end] == b'\\' { 2 } else { 1 };
            }
            blank(&mut code, from, end, true);
            at = end + 1;
            continue;
        }
        if src.get(quote) == Some(&b'\'') && (quote == at || byte == b'b') {
            let from = quote + 1;
            let close = if src.get(from) == Some(&b'\\') {
                // An escape: `'\n'`, `'\''`, `'\u{1F600}'`.
                src.get(from + 2..)
                    .and_then(|rest| rest.iter().position(|&b| b == b'\''))
                    .map(|p| from + 2 + p)
            } else {
                // One character, of however many bytes, then the quote; or
                // else a lifetime or a label, which is code.
                let width = match src.get(from) {
                    Some(&lead) if lead >= 0xF0 => 4,
                    Some(&lead) if lead >= 0xE0 => 3,
                    Some(&lead) if lead >= 0xC0 => 2,
                    Some(_) => 1,
                    None => 0,
                };
                (width > 0 && src.get(from + width) == Some(&b'\'')).then_some(from + width)
            };
            if let Some(close) = close {
                blank(&mut code, from, close, false);
                at = close + 1;
                continue;
            }
        }
        at += 1;
    }
    let code = String::from_utf8(code)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    (code, strings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> BTreeMap<String, String> {
        files
            .iter()
            .map(|(path, text)| ((*path).to_string(), (*text).to_string()))
            .collect()
    }

    const BUILDS: &str = "use tinker_pdf::DocumentBuilder;\n\
        #[test]\n\
        fn builds() {\n    \
            let builder = DocumentBuilder::new();\n    \
            let bytes = builder.finish();\n\
        }\n";

    const HOLDS: &str = "mod written_support;\n\
        use tinker_pdf::DocumentBuilder;\n\
        #[test]\n\
        fn builds() {\n    \
            let bytes = DocumentBuilder::new().finish();\n    \
            written_support::authored(\"built\", bytes);\n\
        }\n";

    /// A test file that writes and calls no door fails, naming the entry point
    /// and its line; the same file calling a door passes, through the module,
    /// through a module `#[path]` binds to it, or by a door imported from it;
    /// a local function of a door's name is not the helper, and nor is naming
    /// the module without calling a door.
    #[test]
    fn a_writing_test_that_skips_the_helper_fails() {
        let problems = judge(&tree(&[("crates/x/tests/builds.rs", BUILDS)]), &[]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with(
                "crates/x/tests/builds.rs:5: writes a document through \
                 `DocumentBuilder::finish`"
            ),
            "{problems:?}"
        );

        assert!(judge(&tree(&[("crates/x/tests/builds.rs", HOLDS)]), &[]).is_empty());

        let imported = "mod written_support;\n\
            use written_support::{authored, Expected};\n\
            use tinker_pdf::DocumentEditor;\n\
            fn t(editor: DocumentEditor) { authored(\"saved\", editor.save(&o)); }\n";
        let files = tree(&[("crates/x/tests/imported.rs", imported)]);
        assert!(judge(&files, &[]).is_empty(), "{:?}", judge(&files, &[]));

        let by_path = "#[path = \"../../tinker-pdf/tests/written_support/mod.rs\"]\n\
            mod reading;\n\
            use tinker_pdf::DocumentBuilder;\n\
            fn t() { reading::authored(\"x\", DocumentBuilder::new().finish()); }\n";
        let files = tree(&[("crates/x/tests/by_path.rs", by_path)]);
        assert!(judge(&files, &[]).is_empty(), "{:?}", judge(&files, &[]));

        let local = "use tinker_pdf::DocumentBuilder;\n\
            fn rewritten() -> Vec<u8> { DocumentBuilder::new().finish() }\n\
            fn t() { let _ = rewritten(); }\n";
        let problems = judge(&tree(&[("crates/x/tests/local.rs", local)]), &[]);
        assert_eq!(
            problems.len(),
            1,
            "a fixture named like a door: {problems:?}"
        );

        let named_only = "mod written_support;\n\
            use tinker_pdf::DocumentBuilder;\n\
            fn t() { let _ = DocumentBuilder::new().finish(); \
            let _ = written_support::Expected::Defect; }\n";
        let problems = judge(&tree(&[("crates/x/tests/named.rs", named_only)]), &[]);
        assert_eq!(problems.len(), 1, "a module and no door: {problems:?}");
    }

    /// A pending unit that calls the helper fails until it is taken off; one
    /// that still writes without it is what the list is for.
    #[test]
    fn a_listed_file_that_now_calls_the_helper_fails() {
        let files = tree(&[("crates/x/tests/builds.rs", HOLDS)]);
        let problems = judge(&files, &["crates/x/tests/builds.rs"]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("is still on `PENDING`"),
            "{problems:?}"
        );

        let files = tree(&[("crates/x/tests/builds.rs", BUILDS)]);
        assert!(judge(&files, &["crates/x/tests/builds.rs"]).is_empty());
    }

    /// A writer named in a comment, a doc comment or a literal is not a write;
    /// a lifetime is code, and a character literal is not.
    #[test]
    fn a_writer_named_in_a_comment_is_not_a_write() {
        let text = "//! Uses `DocumentBuilder` and `.finish()`.\n\
            use tinker_pdf::DocumentBuilder;\n\
            // builder.finish();\n\
            /* editor.save( /* nested */ builder.finish( */\n\
            fn t<'a>(x: &'a str) -> &'a str {\n    \
                let _ = \"builder.finish() https://example.org/x\";\n    \
                let _ = r#\"open_markdown(\"#;\n    \
                let _ = b\".archive()\";\n    \
                let _ = '{';\n    \
                x\n\
            }\n";
        let files = tree(&[("crates/x/tests/quiet.rs", text)]);
        assert!(judge(&files, &[]).is_empty(), "{:?}", judge(&files, &[]));

        let (code, strings) = lex(text);
        assert!(code.contains("fn t<'a>(x: &'a str) -> &'a str {"), "{code}");
        assert!(
            !code.contains("finish") && !code.contains("open_markdown"),
            "{code}"
        );
        assert!(
            !code.contains('{') || code.matches('{').count() == 1,
            "{code}"
        );
        assert_eq!(code.len(), text.len(), "offsets are kept");
        assert_eq!(code.lines().count(), text.lines().count(), "lines are kept");
        let literals: Vec<&str> = strings.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(
            literals,
            [
                "builder.finish() https://example.org/x",
                "open_markdown(",
                ".archive()"
            ]
        );
    }

    /// A conversion writes inside `Document::open`, so it is recognised by what
    /// only a conversion test names; a CLI command by `writing::`.
    #[test]
    fn a_conversion_is_a_write() {
        for (name, text) in [
            (
                "markdown",
                "fn t() { let _ = Document::open_markdown(b, &o); }\n",
            ),
            ("report", "fn t(d: Document) { let _ = d.archive(); }\n"),
            ("epub", "mod epub_support;\nfn t() {}\n"),
            (
                "xps",
                "#[path = \"xps_support/mod.rs\"]\nmod xps_support;\nfn t() {}\n",
            ),
            ("cbz", "fn t() { let _ = cbz_support::comic(&[]); }\n"),
            ("cli", "fn t() { writing::merge(&o).unwrap(); }\n"),
        ] {
            let path = format!("crates/x/tests/{name}.rs");
            let problems = judge(&tree(&[(path.as_str(), text)]), &[]);
            assert_eq!(problems.len(), 1, "{name}: {problems:?}");
        }
        let other = "fn t(zip: Zip) { let archive = zip.archive; let _ = d.archives(); }\n";
        assert!(judge(&tree(&[("crates/x/tests/other.rs", other)]), &[]).is_empty());
    }

    /// Writer code outside a `src` file's test modules is not a write; an
    /// inline test module is read to its own closing brace, whatever braces its
    /// literals hold; and an out-of-line one is its whole file.
    #[test]
    fn only_the_test_modules_of_a_src_file_are_read() {
        let src = "pub fn write(b: DocumentBuilder) -> Vec<u8> { b.finish() }\n\
            #[cfg(test)]\n\
            mod tests {\n    \
                const OPEN: &str = \"{{{\";\n    \
                const CLOSE: char = '}';\n    \
                #[test]\n    \
                fn t() {}\n\
            }\n\
            pub fn save(e: DocumentEditor) -> Vec<u8> { e.save(&o) }\n";
        let files = tree(&[("crates/x/src/lib.rs", src)]);
        assert!(judge(&files, &[]).is_empty(), "{:?}", judge(&files, &[]));

        let writing = "pub fn helper() {}\n\
            #[cfg(test)]\n\
            mod tests {\n    \
                use tinker_pdf::DocumentBuilder;\n    \
                #[test]\n    \
                fn t() { let _ = DocumentBuilder::new().finish(); }\n\
            }\n";
        let problems = judge(&tree(&[("crates/x/src/write.rs", writing)]), &[]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with("crates/x/src/write.rs:6:"),
            "{problems:?}"
        );

        let declared = tree(&[
            (
                "crates/x/src/edit/mod.rs",
                "#[cfg(test)]\nmod tests;\nmod save;\n",
            ),
            (
                "crates/x/src/edit/tests.rs",
                "use crate::DocumentEditor;\nfn t(e: DocumentEditor) { let _ = e.save(&o); }\n",
            ),
            (
                "crates/x/src/edit/save.rs",
                "pub fn s(e: DocumentEditor) -> Vec<u8> { e.save(&o) }\n",
            ),
            (
                "crates/x/src/png.rs",
                "#[cfg(test)]\npub(crate) mod tests;\n",
            ),
            (
                "crates/x/src/png/tests.rs",
                "fn t() { let _ = DocumentBuilder::new().finish(); }\n",
            ),
        ]);
        let problems = judge(&declared, &[]);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems[0].starts_with("crates/x/src/edit/tests.rs:2:"),
            "{problems:?}"
        );
        assert!(
            problems[1].starts_with("crates/x/src/png/tests.rs:1:"),
            "{problems:?}"
        );

        let missing = tree(&[("crates/x/src/lib.rs", "#[cfg(test)]\nmod gone;\n")]);
        let problems = judge(&missing, &[]);
        assert!(
            problems.len() == 1 && problems[0].contains("could not be found"),
            "{problems:?}"
        );
    }

    /// A file a unit includes by `#[path]` from outside the units is read as
    /// part of it, wherever the unit's inline modules put the path's base.
    #[test]
    fn a_file_a_unit_includes_by_path_is_part_of_it() {
        let files = tree(&[
            (
                "crates/x/tests/counts.rs",
                "#[path = \"../benches/support/inputs.rs\"]\nmod inputs;\nfn t() {}\n",
            ),
            (
                "crates/x/benches/support/inputs.rs",
                "use tinker_pdf::DocumentBuilder;\n\npub fn page() -> Vec<u8> { DocumentBuilder::new().finish() }\n",
            ),
            (
                "crates/x/src/write.rs",
                "#[cfg(test)]\nmod tests {\n    \
                 #[path = \"../../../tests/written_support/mod.rs\"]\n    \
                 mod written_support;\n    \
                 fn t(b: DocumentBuilder) { written_support::authored(\"x\", b.finish()); }\n}\n",
            ),
        ]);
        let problems = judge(&files, &[]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with(
                "crates/x/tests/counts.rs: writes a document through \
                 `DocumentBuilder::finish` at crates/x/benches/support/inputs.rs:3, which it \
                 includes by `#[path]`,"
            ),
            "{problems:?}"
        );
        // The file it includes is not a unit of its own, and is pending under
        // the unit that reads it.
        assert!(judge(&files, &["crates/x/tests/counts.rs"]).is_empty());
    }

    /// A pending entry that no longer writes fails, and so does one that names
    /// no unit, and a list out of order.
    #[test]
    fn a_listed_file_that_no_longer_writes_fails() {
        let quiet = "fn t() { let _ = 1; }\n";
        let files = tree(&[("crates/x/tests/quiet.rs", quiet)]);
        let problems = judge(&files, &["crates/x/tests/quiet.rs"]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("writes no document"), "{problems:?}");

        let problems = judge(&files, &["crates/x/tests/gone.rs"]);
        assert!(
            problems.len() == 1 && problems[0].contains("names no test file"),
            "{problems:?}"
        );

        let files = tree(&[
            ("crates/x/tests/a.rs", BUILDS),
            ("crates/x/tests/b.rs", BUILDS),
        ]);
        assert!(judge(&files, &["crates/x/tests/a.rs", "crates/x/tests/b.rs"]).is_empty());
        let problems = judge(&files, &["crates/x/tests/b.rs", "crates/x/tests/a.rs"]);
        assert!(
            problems.len() == 1 && problems[0].contains("is not sorted"),
            "{problems:?}"
        );
    }
}
