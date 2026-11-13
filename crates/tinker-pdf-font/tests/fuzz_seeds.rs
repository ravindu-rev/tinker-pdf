//! Nine committed font corpora, replayed on stable, measured for *reach*.
//!
//! `woff_seeds.rs` does the same for `fuzz/corpus/woff`. These nine share a
//! file because they share a shape: eight of the nine targets open by parsing
//! their input — `let Some(x) = X::parse(data) else { return; }`, or
//! `type42`'s `let Ok(…)` — and return at once where it does not parse, so a
//! seed that does not parse makes the whole target a no-op and the fuzzer
//! spends its budget mutating something that was never a font.
//!
//! # Why "it did not panic" is not the question here
//!
//! A fuzz target over arbitrary bytes is supposed to be refused most of the
//! time, so a corpus in which *every* seed is refused runs exactly as clean as
//! one in which every seed parses. `fuzz/corpus/jxr` held forty-two seeds and
//! decoded none of them; `fuzz/corpus/jpx` held six of the same. Neither was
//! visible from a seed count, a directory listing, or a green `cargo fuzz`.
//!
//! So these count, and each asserts that **at least one** seed reaches the
//! parser, and names the ones that do not.
//!
//! The first draft of this file asserted that *every* seed parses, on the
//! reasoning that a small hand-curated font corpus has no reason to hold a
//! non-font. Two seeds failed it and both are deliberate:
//! `type1/truncated-eexec.pfb` is a **negative** seed that exists to drive the
//! eexec decryptor's refusal, and `truetype/cmap12-glyph-id-overflow.ttf` is a
//! `cmap`-only face with no outline table at all, which is exactly what a
//! format-12 overflow test wants. A corpus needs both kinds, so the rule is
//! the weaker true one rather than the stronger false one — with the split
//! printed, so a corpus drifting towards all-refusals is visible in the
//! output rather than only in a fuzz run nobody reads.
//!
//! # What they are not
//!
//! Not a correctness check. Nothing below compares an outline, an advance or a
//! character mapping against an expected value — that is what this crate's own
//! tests and `crates/tinker-pdf/tests/cff_fonts.rs` are for. These answer one
//! question: does the corpus reach the code it was written for?

use std::path::{Path, PathBuf};

use std::collections::BTreeSet;

use tinker_pdf_font::cff2::Cff2;
use tinker_pdf_font::hint::{Error as HintError, Program, Unhinted};
use tinker_pdf_font::variation::gvar::Gvar;
use tinker_pdf_font::variation::{tag_value, DesignSpace, Instance, Location};
use tinker_pdf_font::{
    cmap, glyf, subset_type1, substitute_glyph, BuiltInEncoding, Cff, Sfnt, Type1, Type42,
};

/// The `truetype_hinting` target's input and its run: the target's own file,
/// so the seeds are written with its encoder and replayed through its run.
#[allow(
    dead_code,
    reason = "the target's file; the replay reaches all of it but a few accessors"
)]
#[path = "../../../fuzz/src/hinting.rs"]
mod hinting;

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

/// Reports the count and holds the floor.
///
/// The floor is "more than none". A corpus in which nothing parses is the
/// jxr defect; a corpus in which one seed deliberately does not is a corpus
/// with a negative case in it, which is a different and healthy thing.
fn report(name: &str, seeds: usize, reached: usize, unreached: &[String]) {
    assert!(seeds > 0, "fuzz/corpus/{name} is empty");
    assert!(
        reached > 0,
        "fuzz/corpus/{name}: not one of {seeds} seeds parses, so every run of this target returns at once"
    );
    if unreached.is_empty() {
        println!("RAN {name}-seeds: {seeds} seeds, {reached} reach the parser");
    } else {
        println!(
            "RAN {name}-seeds: {seeds} seeds, {reached} reach the parser, {} refused on purpose: {unreached:?}",
            unreached.len()
        );
    }
}

#[test]
fn the_cff_seeds_reach_the_parser() {
    let Some(files) = seeds("cff") else {
        println!("cff-seeds: SKIPPED (no fuzz/corpus/cff)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        match Cff::parse(data) {
            Some(cff) => {
                reached += 1;
                // Touching what the target touches, so a seed that parses and
                // then has nothing in it is visible as well.
                let count = cff.glyph_count().min(64) as u16;
                for glyph in 0..count {
                    let _ = cff.outline(glyph);
                    let _ = cff.advance(glyph);
                }
            }
            None => missed.push(label.clone()),
        }
    }
    report("cff", files.len(), reached, &missed);
}

/// A committed variable face, which a seed of the same name is a copy of.
fn committed_face(file: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("variable")
        .join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The locations the variable targets read a face at, as far as a replay
/// needs them: the default and the named instances the face resolves.
fn default_and_named(space: &DesignSpace) -> Vec<Location> {
    let mut out = vec![space.default_location()];
    for index in 0..space.instances().len().min(8) {
        if let Ok(location) = space.location(&Instance::Named(index as u16)) {
            out.push(location);
        }
    }
    out
}

#[test]
fn the_cff2_seeds_reach_the_parser() {
    let Some(files) = seeds("cff2") else {
        println!("cff2-seeds: SKIPPED (no fuzz/corpus/cff2)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        // As the target reads its input: an sfnt carrying the table, or the
        // bare table.
        let sfnt = Sfnt::parse(data);
        let table = sfnt
            .as_ref()
            .and_then(|font| font.table(tag_value(*b"CFF2")));
        let (bytes, space) = match (&sfnt, table) {
            (Some(font), Some(table)) => (table, DesignSpace::read(font).unwrap_or_default()),
            _ => (data.as_slice(), DesignSpace::default()),
        };
        match Cff2::parse(bytes) {
            Ok(cff2) => {
                reached += 1;
                // Touching what the target touches, so a seed that parses
                // and then draws nothing at any location is visible too.
                let count = cff2.glyph_count().min(64) as u16;
                let mut drawn = 0usize;
                for location in default_and_named(&space) {
                    for glyph in 0..count {
                        if cff2
                            .outline_at(glyph, &location)
                            .is_ok_and(|outline| !outline.segments.is_empty())
                        {
                            drawn += 1;
                        }
                    }
                }
                assert!(drawn > 0, "{label}: parses and draws no glyph anywhere");
            }
            Err(_) => missed.push(label.clone()),
        }
    }
    // The seed written from the committed face is that face, so the corpus
    // starts from the table `variations.rs` describes.
    let committed = committed_face("variable-cff2.otf");
    assert!(
        files
            .iter()
            .any(|(label, data)| label == "variable.cff2" && *data == committed),
        "fuzz/corpus/cff2/variable.cff2 is not tests/variable/variable-cff2.otf"
    );
    report("cff2", files.len(), reached, &missed);
}

#[test]
fn the_cff_subset_seeds_reach_the_subsetter() {
    let Some(files) = seeds("cff_subset") else {
        println!("cff_subset-seeds: SKIPPED (no fuzz/corpus/cff_subset)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        // The target returns on a failed parse *and* on a zero glyph count, so
        // both are "not reached" — a font with no glyphs subsets nothing.
        match Cff::parse(data) {
            Some(cff) if cff.glyph_count() > 0 => reached += 1,
            _ => missed.push(label.clone()),
        }
    }
    report("cff_subset", files.len(), reached, &missed);
}

/// Two `cff_subset` seeds are programs renumbering declines — a computed
/// subroutine number, and one subroutine entered in two hint states — so the
/// target reaches flattening through them: each subsets, and every glyph it
/// keeps draws as the original's.
#[test]
fn the_cff_subset_seeds_reach_flattening() {
    let Some(files) = seeds("cff_subset") else {
        println!("cff_subset-flattening: SKIPPED (no fuzz/corpus/cff_subset)");
        return;
    };
    let glyphs: BTreeSet<u16> = BTreeSet::from([1, 2]);
    for name in ["computed_call.cff", "two_hint_states.cff"] {
        let (_, data) = files
            .iter()
            .find(|(label, _)| label == name)
            .unwrap_or_else(|| panic!("fuzz/corpus/cff_subset/{name} is committed"));
        let subset = tinker_pdf_font::subset(data, &glyphs)
            .unwrap_or_else(|| panic!("{name}: flattening takes it"));
        let before = Cff::parse(data).expect("the seed parses");
        let after = Cff::parse(&subset).expect("the subset parses");
        for &glyph in &glyphs {
            assert_eq!(
                before.outline(glyph).map(|outline| outline.segments),
                after.outline(glyph).map(|outline| outline.segments),
                "{name}, glyph {glyph}"
            );
        }
        assert!(subset.len() < data.len(), "{name}: the subset is smaller");
    }
    println!("RAN cff_subset-flattening: 2 seeds flattened");
}

#[test]
fn the_sfnt_seeds_reach_the_directory_walk() {
    let Some(files) = seeds("sfnt") else {
        println!("sfnt-seeds: SKIPPED (no fuzz/corpus/sfnt)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        match Sfnt::parse(data) {
            Some(font) => {
                reached += 1;
                for tag in [b"head", b"hhea", b"maxp", b"cmap"] {
                    let _ = font.table(u32::from_be_bytes(*tag));
                }
                // The route a substituted symbolic font is drawn by.
                let _ = font.glyph_for_name("alpha");
                let _ = font.glyph_for_unicode('\u{03B1}');
                let _ = font.glyph_for_symbol_code(0x61);
                let _ = substitute_glyph(data, BuiltInEncoding::Symbol, Some("alpha"), 0x61);
            }
            None => missed.push(label.clone()),
        }
    }
    report("sfnt", files.len(), reached, &missed);
}

/// Whether a face's `glyf` holds a **composite** glyph, which is a
/// `numberOfContours` below zero at some glyph's start.
///
/// Read off the tables here rather than asked of the crate, because the crate
/// has nothing to ask. `glyf::outline` returns an `Outline`, and a composite
/// and the simple glyph it assembles are the same `Outline` by the time it
/// does — that is the whole point of the function. So "the corpus reaches the
/// recursion" is not a question the public API can answer, and a test that
/// wants it answered has to open `loca` itself.
fn carries_a_composite(font: &Sfnt<'_>) -> bool {
    let (Some(head), Some(loca), Some(glyf)) = (
        font.table(u32::from_be_bytes(*b"head")),
        font.table(u32::from_be_bytes(*b"loca")),
        font.table(u32::from_be_bytes(*b"glyf")),
    ) else {
        return false;
    };
    let Some(format) = head.get(50..52) else {
        return false;
    };
    // head.indexToLocFormat: 0 means 16-bit offsets stored halved.
    let long = i16::from_be_bytes([format[0], format[1]]) != 0;
    let width = if long { 4 } else { 2 };

    // A seed corpus, not a hostile input — but the ceiling costs nothing and
    // this test may not be the thing that hangs on a bad `loca`.
    for glyph in 0..4096usize {
        let Some(pair) = loca.get(glyph * width..glyph * width + width * 2) else {
            return false; // Past the last glyph.
        };
        let (start, end) = if long {
            (
                u32::from_be_bytes([pair[0], pair[1], pair[2], pair[3]]) as usize,
                u32::from_be_bytes([pair[4], pair[5], pair[6], pair[7]]) as usize,
            )
        } else {
            (
                usize::from(u16::from_be_bytes([pair[0], pair[1]])) * 2,
                usize::from(u16::from_be_bytes([pair[2], pair[3]])) * 2,
            )
        };
        if end <= start {
            continue; // An empty range: a space.
        }
        if let Some(header) = glyf.get(start..start + 2) {
            if i16::from_be_bytes([header[0], header[1]]) < 0 {
                return true;
            }
        }
    }
    false
}

/// The floor on `truetype` seeds carrying an outline table.
///
/// Five is what the corpus holds, so this is a ratchet rather than a wish: a
/// seed deleted, or one that quietly stopped parsing, takes it below the
/// floor.
const TRUETYPE_OUTLINED: usize = 5;

/// The floor on `truetype` seeds carrying a **composite** glyph.
///
/// The target's own header calls composite recursion "the interesting part":
/// `composite-transforms.ttf`, `composite-nested.ttf` and `variable.ttf` reach
/// it from three starting points, and a lower floor would let one of them go
/// without the count moving.
const TRUETYPE_COMPOSITE: usize = 3;

/// The floor on `truetype` seeds that vary: an `fvar` with axes and a `gvar`
/// the crate reads, so the target's variable half — outlines, phantom points
/// and metrics at its locations — starts from a face that reaches it.
/// `variable.ttf` is the committed `glyf` face.
const TRUETYPE_VARIABLE: usize = 1;

#[test]
fn the_truetype_seeds_reach_the_outlines() {
    let Some(files) = seeds("truetype") else {
        println!("truetype-seeds: SKIPPED (no fuzz/corpus/truetype)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    let mut outlineless: Vec<String> = Vec::new();
    let mut outlined: Vec<String> = Vec::new();
    let mut composite: Vec<String> = Vec::new();
    let mut variable: Vec<String> = Vec::new();
    for (label, data) in &files {
        match Sfnt::parse(data) {
            Some(font) => {
                reached += 1;
                if font.table(u32::from_be_bytes(*b"glyf")).is_none()
                    && font.table(u32::from_be_bytes(*b"CFF ")).is_none()
                {
                    // Reported, not refused. `cmap12-glyph-id-overflow.ttf` is
                    // a `cmap`-only face on purpose, and the target's
                    // `glyph_for_char` half is real code that it reaches.
                    outlineless.push(label.clone());
                } else {
                    outlined.push(label.clone());
                    if carries_a_composite(&font) {
                        composite.push(label.clone());
                    }
                }
                // The variable half reaches a seed whose design space varies
                // and whose `gvar` reads; touched as the target touches it,
                // every glyph at the default and at each named instance.
                let space = DesignSpace::read(&font).unwrap_or_default();
                if let Ok(Some(gvar)) = Gvar::read(&font, &space) {
                    variable.push(label.clone());
                    for location in default_and_named(&space) {
                        for glyph in 0..16u16 {
                            let _ = glyf::outline_at(&font, glyph, Some(&gvar), &location);
                        }
                    }
                }
            }
            None => missed.push(label.clone()),
        }
    }
    if !outlineless.is_empty() {
        println!("  truetype: {outlineless:?} carry no outline table, by design");
    }
    println!(
        "  truetype: {outlined:?} carry one, of which {composite:?} are composite; \
         {variable:?} vary"
    );
    assert!(
        variable.len() >= TRUETYPE_VARIABLE,
        "fuzz/corpus/truetype: {} seeds vary, floor is {TRUETYPE_VARIABLE} — {variable:?}. \
         The target's variable half now starts from random bytes.",
        variable.len()
    );
    let committed = committed_face("variable-glyf.ttf");
    assert!(
        files
            .iter()
            .any(|(label, data)| label == "variable.ttf" && *data == committed),
        "fuzz/corpus/truetype/variable.ttf is not tests/variable/variable-glyf.ttf"
    );

    // The count, not the reach. `report` below asks whether the corpus parses
    // at all; these ask whether it reaches the half of the target that the
    // target's own header says is the half worth reaching.
    assert!(
        outlined.len() >= TRUETYPE_OUTLINED,
        "fuzz/corpus/truetype: {} of {} seeds carry an outline table, floor is \
         {TRUETYPE_OUTLINED} — {outlined:?}. A seed was deleted, or one stopped \
         parsing and is now reaching the directory walk and nothing else.",
        outlined.len(),
        files.len()
    );
    assert!(
        composite.len() >= TRUETYPE_COMPOSITE,
        "fuzz/corpus/truetype: {} seeds carry a composite glyph, floor is \
         {TRUETYPE_COMPOSITE} — {composite:?}. Composite recursion is what this \
         target's header calls the interesting part, and the fuzzer is now \
         starting from fewer places in it. `glyf.rs`'s `write_the_fuzz_seeds` \
         writes them.",
        composite.len()
    );

    report("truetype", files.len(), reached, &missed);
}

#[test]
fn the_type1_seeds_reach_the_charstrings() {
    let Some(files) = seeds("type1") else {
        println!("type1-seeds: SKIPPED (no fuzz/corpus/type1)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        match Type1::parse(data) {
            Some(font) => {
                reached += 1;
                // Two layers of stream cipher stand between the bytes and the
                // interpreter, so a program that decrypts to nothing still
                // "parses". The glyph count is what says it did not.
                assert!(
                    font.glyph_count() > 0,
                    "{label}: a program that parsed with no glyphs in it"
                );
            }
            None => missed.push(label.clone()),
        }
    }
    report("type1", files.len(), reached, &missed);
}

#[test]
fn the_type42_seeds_reach_the_face() {
    let Some(files) = seeds("type42") else {
        println!("type42-seeds: SKIPPED (no fuzz/corpus/type42)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        assert!(
            Type42::is_type42(data),
            "{label}: every seed is a Type 42 program by its first line"
        );
        match Type42::parse(data) {
            Ok(font) => {
                reached += 1;
                // A wrapper that read with no face behind it, or no glyph to
                // outline, would leave the target nothing to do past the
                // wrapper.
                let face = font.sfnt().expect("a parsed program's face reads");
                assert!(font.charstring_count() > 0, "{label}: no /CharStrings");
                for (_, glyph) in font.glyph_names() {
                    let _ = tinker_pdf_font::glyf::outline(&face, glyph);
                }
            }
            Err(_) => missed.push(label.clone()),
        }
    }
    report("type42", files.len(), reached, &missed);
}

#[test]
fn the_type1_subset_seeds_reach_the_subsetter() {
    let Some(files) = seeds("type1_subset") else {
        println!("type1_subset-seeds: SKIPPED (no fuzz/corpus/type1_subset)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        // The target's first selection is the first glyph alone. A seed the
        // subsetter declines there reaches a refusal and writes nothing, which
        // is the writer path this corpus exists to drive.
        match subset_type1(data, &BTreeSet::from([0])) {
            Ok(subset) => {
                reached += 1;
                assert!(
                    Type1::parse(&subset.program).is_some(),
                    "{label}: a subset its own reader refuses"
                );
            }
            Err(_) => missed.push(label.clone()),
        }
    }
    report("type1_subset", files.len(), reached, &missed);
}

#[test]
fn the_cmap_seeds_reach_the_parser() {
    let Some(files) = seeds("cmap") else {
        println!("cmap-seeds: SKIPPED (no fuzz/corpus/cmap)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        // `cmap::parse` is infallible, so "reached" cannot be "it returned".
        // A CMap that found no codespace range decodes nothing, which is the
        // observable form of the same failure.
        let map = cmap::parse(data);
        if map.decode_codes(data).is_empty() {
            missed.push(label.clone());
        } else {
            reached += 1;
        }
    }
    report("cmap", files.len(), reached, &missed);
}

// ---- truetype_hinting: the interpreter over programs it was never meant to see -

/// The opcodes the hinting seeds' programs are written in, by the
/// specification's names: the first of a range for an instruction with
/// flags, which a seed adds.
mod op {
    pub const SVTCA: u8 = 0x00;
    pub const SPVTCA: u8 = 0x02;
    pub const SFVTCA: u8 = 0x04;
    pub const SPVTL: u8 = 0x06;
    pub const SFVTL: u8 = 0x08;
    pub const SPVFS: u8 = 0x0A;
    pub const SFVFS: u8 = 0x0B;
    pub const GPV: u8 = 0x0C;
    pub const GFV: u8 = 0x0D;
    pub const SFVTPV: u8 = 0x0E;
    pub const ISECT: u8 = 0x0F;
    pub const SRP0: u8 = 0x10;
    pub const SRP1: u8 = 0x11;
    pub const SRP2: u8 = 0x12;
    pub const SZP0: u8 = 0x13;
    pub const SZP1: u8 = 0x14;
    pub const SZP2: u8 = 0x15;
    pub const SZPS: u8 = 0x16;
    pub const SLOOP: u8 = 0x17;
    pub const RTG: u8 = 0x18;
    pub const RTHG: u8 = 0x19;
    pub const SMD: u8 = 0x1A;
    pub const ELSE: u8 = 0x1B;
    pub const JMPR: u8 = 0x1C;
    pub const SCVTCI: u8 = 0x1D;
    pub const SSWCI: u8 = 0x1E;
    pub const SSW: u8 = 0x1F;
    pub const DUP: u8 = 0x20;
    pub const POP: u8 = 0x21;
    pub const CLEAR: u8 = 0x22;
    pub const SWAP: u8 = 0x23;
    pub const DEPTH: u8 = 0x24;
    pub const CINDEX: u8 = 0x25;
    pub const MINDEX: u8 = 0x26;
    pub const ALIGNPTS: u8 = 0x27;
    pub const UTP: u8 = 0x29;
    pub const LOOPCALL: u8 = 0x2A;
    pub const CALL: u8 = 0x2B;
    pub const FDEF: u8 = 0x2C;
    pub const ENDF: u8 = 0x2D;
    pub const MDAP: u8 = 0x2E;
    pub const IUP: u8 = 0x30;
    pub const SHP: u8 = 0x32;
    pub const SHC: u8 = 0x34;
    pub const SHZ: u8 = 0x36;
    pub const SHPIX: u8 = 0x38;
    pub const IP: u8 = 0x39;
    pub const MSIRP: u8 = 0x3A;
    pub const ALIGNRP: u8 = 0x3C;
    pub const RTDG: u8 = 0x3D;
    pub const MIAP: u8 = 0x3E;
    pub const NPUSHB: u8 = 0x40;
    pub const NPUSHW: u8 = 0x41;
    pub const WS: u8 = 0x42;
    pub const RS: u8 = 0x43;
    pub const WCVTP: u8 = 0x44;
    pub const RCVT: u8 = 0x45;
    pub const GC: u8 = 0x46;
    pub const SCFS: u8 = 0x48;
    pub const MD: u8 = 0x49;
    pub const MPPEM: u8 = 0x4B;
    pub const MPS: u8 = 0x4C;
    pub const FLIPON: u8 = 0x4D;
    pub const FLIPOFF: u8 = 0x4E;
    pub const DEBUG: u8 = 0x4F;
    pub const LT: u8 = 0x50;
    pub const LTEQ: u8 = 0x51;
    pub const GT: u8 = 0x52;
    pub const GTEQ: u8 = 0x53;
    pub const EQ: u8 = 0x54;
    pub const NEQ: u8 = 0x55;
    pub const ODD: u8 = 0x56;
    pub const EVEN: u8 = 0x57;
    pub const IF: u8 = 0x58;
    pub const EIF: u8 = 0x59;
    pub const AND: u8 = 0x5A;
    pub const OR: u8 = 0x5B;
    pub const NOT: u8 = 0x5C;
    pub const DELTAP1: u8 = 0x5D;
    pub const SDB: u8 = 0x5E;
    pub const SDS: u8 = 0x5F;
    pub const ADD: u8 = 0x60;
    pub const SUB: u8 = 0x61;
    pub const DIV: u8 = 0x62;
    pub const MUL: u8 = 0x63;
    pub const ABS: u8 = 0x64;
    pub const NEG: u8 = 0x65;
    pub const FLOOR: u8 = 0x66;
    pub const CEILING: u8 = 0x67;
    pub const ROUND: u8 = 0x68;
    pub const NROUND: u8 = 0x6C;
    pub const WCVTF: u8 = 0x70;
    pub const DELTAP2: u8 = 0x71;
    pub const DELTAP3: u8 = 0x72;
    pub const DELTAC1: u8 = 0x73;
    pub const DELTAC2: u8 = 0x74;
    pub const DELTAC3: u8 = 0x75;
    pub const SROUND: u8 = 0x76;
    pub const S45ROUND: u8 = 0x77;
    pub const JROT: u8 = 0x78;
    pub const JROF: u8 = 0x79;
    pub const ROFF: u8 = 0x7A;
    pub const RUTG: u8 = 0x7C;
    pub const RDTG: u8 = 0x7D;
    pub const SANGW: u8 = 0x7E;
    pub const AA: u8 = 0x7F;
    pub const FLIPPT: u8 = 0x80;
    pub const FLIPRGON: u8 = 0x81;
    pub const FLIPRGOFF: u8 = 0x82;
    pub const SCANCTRL: u8 = 0x85;
    pub const SDPVTL: u8 = 0x86;
    pub const GETINFO: u8 = 0x88;
    pub const IDEF: u8 = 0x89;
    pub const ROLL: u8 = 0x8A;
    pub const MAX: u8 = 0x8B;
    pub const MIN: u8 = 0x8C;
    pub const SCANTYPE: u8 = 0x8D;
    pub const INSTCTRL: u8 = 0x8E;
    pub const PUSHB: u8 = 0xB0;
    pub const PUSHW: u8 = 0xB8;
    pub const MDRP: u8 = 0xC0;
    pub const MIRP: u8 = 0xE0;
}

/// The instructions that push `values` in order, the last on top: `PUSHB`
/// where every value is a byte, `PUSHW` otherwise, each value a word.
fn push(values: &[i32]) -> Vec<u8> {
    assert!(
        (1..=8).contains(&values.len()),
        "a push of one to eight values"
    );
    let bytes = values.iter().all(|v| (0..=255).contains(v));
    let count = values.len() as u8;
    let mut code = vec![if bytes {
        op::PUSHB + count - 1
    } else {
        op::PUSHW + count - 1
    }];
    for &value in values {
        if bytes {
            code.push(value as u8);
        } else {
            let word = i16::try_from(value).expect("a pushed value fits a word");
            code.extend(word.to_be_bytes());
        }
    }
    code
}

/// Pieces of a program, in order.
fn cat(pieces: &[&[u8]]) -> Vec<u8> {
    pieces.concat()
}

/// What a hinting seed's run comes to.
#[derive(Clone, Copy, Debug)]
enum Comes {
    /// Every answer at every size a hint.
    Hints,
    /// One answer at least the named failure of the named program.
    Fails(Program, HintError),
    /// One size at least declined by the face's `gasp`, and every other
    /// answer a hint.
    Declined,
}

/// One seed of `fuzz/corpus/truetype_hinting`: its name, its bytes and what
/// its run comes to.
struct HintingSeed {
    name: &'static str,
    bytes: Vec<u8>,
    comes: Comes,
}

/// The input every seed but `empty` starts from: hinted at 7, 12 and 16
/// pixels an em; `maxp`'s limits roomy enough for the programs; four control
/// values, 1 408, 300, 100 and 50; and the glyph the programs point at, two
/// contours of four points — (100, 0), (1 000, 0), (1 000, 1 400), (100,
/// 1 400) and (300, 300), (800, 300) off the curve, (800, 1 100), (300,
/// 1 100) — so points 0 to 7 are its contour points, and 8 to 11 its phantom
/// points.
fn base_input() -> hinting::Input {
    hinting::Input {
        sizes: 0b0001_1010,
        options: 0,
        profile: [8, 32, 32, 4, 256],
        cvt: [1_408i16, 300, 100, 50]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect(),
        points: vec![
            (100, 0, 1),
            (1_000, 0, 1),
            (1_000, 1_400, 1),
            (100, 1_400, 3),
            (300, 300, 1),
            (800, 300, 0),
            (800, 1_100, 1),
            (300, 1_100, 3),
        ],
        ..hinting::Input::default()
    }
}

/// Every seed, written by [`write_the_truetype_hinting_seeds`] and replayed
/// by [`the_truetype_hinting_seeds_reach_the_interpreter`]: one for each
/// instruction family, each program running straight through the family's
/// instructions with every operand in range, so it hints; the composite in
/// every placement; `maxp` at its ceilings; a `gasp` that declines a size;
/// every size, monochrome and at integer sizes; and a run past each of the
/// bounds that keep a hostile program finite, each refused by its name.
#[allow(clippy::too_many_lines)]
fn hinting_seeds() -> Vec<HintingSeed> {
    use op::*;
    let hints = |name, input: hinting::Input| HintingSeed {
        name,
        bytes: input.encode(),
        comes: Comes::Hints,
    };
    let fails = |name, input: hinting::Input, program, error| HintingSeed {
        name,
        bytes: input.encode(),
        comes: Comes::Fails(program, error),
    };
    let mut seeds = Vec::new();

    // The stack, logic, arithmetic and DEBUG, each operand traced: 5 7 9,
    // DUP and POP, SWAP to 5 9 7, DEPTH and CINDEX 3 to 5 9 7 5, MINDEX 2 to
    // 5 9 5 7, ROLL to 5 5 7 9, CLEAR; 1 2 3 300 −2, ADD, SUB, MUL to 1 −9,
    // ABS, NEG, MAX to 1, DIV by 128 to 0, MIN, FLOOR, CEILING of 65 and the
    // six comparisons, AND, OR and NOT, each leaving one value, and DEBUG
    // popping the last.
    seeds.push(hints(
        "stack-and-arithmetic",
        hinting::Input {
            glyph: cat(&[
                &push(&[5, 7, 9]),
                &[DUP, POP, SWAP, DEPTH, CINDEX],
                &push(&[2]),
                &[MINDEX, ROLL, CLEAR],
                &[NPUSHB, 3, 1, 2, 3],
                &[NPUSHW, 2, 0x01, 0x2C, 0xFF, 0xFE],
                &[ADD, SUB, MUL, ABS, NEG, MAX],
                &push(&[128]),
                &[DIV],
                &push(&[100]),
                &[MIN, FLOOR],
                &push(&[65]),
                &[CEILING, LT],
                &push(&[1]),
                &[LTEQ],
                &push(&[0]),
                &[GT],
                &push(&[1]),
                &[GTEQ],
                &push(&[1]),
                &[EQ],
                &push(&[0]),
                &[NEQ],
                &push(&[1]),
                &[AND],
                &push(&[0]),
                &[OR, NOT, DEBUG],
            ]),
            ..base_input()
        },
    ));

    // Flow: fpgm defines function 0, function 1 holding an IF and an ELSE,
    // and an instruction for 0x91, which no instruction of the set has; the
    // glyph calls 0, loops it three times, calls 1, takes an IF's false and
    // true arms, and jumps over a push by JMPR, JROT on true and JROF on
    // false, falls through a JROF on true, and runs 0x91.
    seeds.push(hints(
        "flow-and-definitions",
        hinting::Input {
            fpgm: cat(&[
                &push(&[0]),
                &[FDEF],
                &push(&[1]),
                &[POP, ENDF],
                &push(&[1]),
                &[FDEF],
                &push(&[1]),
                &[IF],
                &push(&[2]),
                &[POP, ELSE],
                &push(&[3]),
                &[POP, EIF, ENDF],
                &push(&[0x91]),
                &[IDEF],
                &push(&[4]),
                &[POP, ENDF],
            ]),
            glyph: cat(&[
                &push(&[0]),
                &[CALL],
                &push(&[3, 0]),
                &[LOOPCALL],
                &push(&[1]),
                &[CALL],
                &push(&[0]),
                &[IF],
                &push(&[9]),
                &[POP, EIF],
                &push(&[1]),
                &[IF],
                &push(&[9]),
                &[POP, ELSE],
                &push(&[8]),
                &[POP, EIF],
                &push(&[3]),
                &[JMPR],
                &push(&[99]),
                &push(&[3, 1]),
                &[JROT],
                &push(&[98]),
                &push(&[3, 0]),
                &[JROF],
                &push(&[97]),
                &push(&[3, 1]),
                &[JROF, 0x91],
            ]),
            ..base_input()
        },
    ));

    // Storage, the control value table and the engine's answers, in prep
    // and in the glyph: WS, RS, WCVTP, WCVTF, RCVT, MPPEM, MPS, GETINFO for
    // one selector and for all, and INSTCTRL, which in the glyph's program
    // pops its two values and changes nothing.
    seeds.push(hints(
        "storage-and-the-engine",
        hinting::Input {
            prep: cat(&[
                &push(&[0, 77]),
                &[WS],
                &push(&[1, 64]),
                &[WCVTP],
                &push(&[2, 100]),
                &[WCVTF],
                &push(&[0]),
                &[RS, POP],
                &push(&[1]),
                &[RCVT, POP, MPPEM, POP, MPS, POP],
                &push(&[1]),
                &[GETINFO, POP],
                &push(&[0x3F]),
                &[GETINFO, POP],
                &push(&[0, 3]),
                &[INSTCTRL],
            ]),
            glyph: cat(&[
                &push(&[0]),
                &[RS],
                &push(&[1]),
                &[RCVT, POP, POP, MPPEM, POP],
                &push(&[33]),
                &[GETINFO, POP],
                &push(&[0, 1]),
                &[INSTCTRL],
            ]),
            ..base_input()
        },
    ));

    // The graphics state: every vector instruction, the reference points,
    // the zone pointers, the loop, every round state with SROUND's and
    // S45ROUND's, ROUND, NROUND, ODD and EVEN, the distance and delta
    // settings, auto flip, SANGW, and the scan converter's.
    seeds.push(hints(
        "graphics-state",
        hinting::Input {
            glyph: cat(&[
                &[SVTCA, SVTCA + 1, SPVTCA, SPVTCA + 1, SFVTCA, SFVTCA + 1],
                &push(&[2, 1]),
                &[SPVTL],
                &push(&[2, 1]),
                &[SPVTL + 1],
                &push(&[3, 0]),
                &[SFVTL],
                &push(&[3, 0]),
                &[SFVTL + 1],
                &push(&[6, 4]),
                &[SDPVTL],
                &push(&[6, 4]),
                &[SDPVTL + 1],
                &push(&[0, 16_384]),
                &[SPVFS],
                &push(&[16_384, 0]),
                &[SFVFS, GPV, POP, POP, GFV, POP, POP, SFVTPV],
                &push(&[0]),
                &[SRP0],
                &push(&[1]),
                &[SRP1],
                &push(&[2]),
                &[SRP2],
                &push(&[1]),
                &[SZP0],
                &push(&[1]),
                &[SZP1],
                &push(&[1]),
                &[SZP2],
                &push(&[1]),
                &[SZPS],
                &push(&[1]),
                &[SLOOP, RTG, RTHG, RTDG, RDTG, RUTG, ROFF],
                &push(&[0x48]),
                &[SROUND],
                &push(&[0x48]),
                &[S45ROUND],
                &push(&[100]),
                &[ROUND, POP],
                &push(&[100]),
                &[ROUND + 3, POP],
                &push(&[100]),
                &[NROUND, POP],
                &push(&[64]),
                &[ODD, POP],
                &push(&[64]),
                &[EVEN, POP],
                &push(&[64]),
                &[SMD],
                &push(&[68]),
                &[SCVTCI],
                &push(&[0]),
                &[SSWCI],
                &push(&[0]),
                &[SSW, FLIPON, FLIPOFF],
                &push(&[0]),
                &[SANGW],
                &push(&[9]),
                &[SDB],
                &push(&[3]),
                &[SDS],
                &push(&[0x1FF]),
                &[SCANCTRL],
                &push(&[4]),
                &[SCANTYPE, SVTCA + 1],
            ]),
            ..base_input()
        },
    ));

    // Measuring and moving, across and then up: GC both ways, SCFS, MD both
    // ways, MDAP both ways, MIAP both ways, MSIRP both ways, MDRP and MIRP
    // under several of their flag sets, every point and CVT entry in range.
    seeds.push(hints(
        "measuring-and-moving",
        hinting::Input {
            glyph: cat(&[
                &[SVTCA + 1],
                &push(&[0]),
                &[GC, POP],
                &push(&[0]),
                &[GC + 1, POP],
                &push(&[0, 128]),
                &[SCFS],
                &push(&[2, 1]),
                &[MD, POP],
                &push(&[2, 1]),
                &[MD + 1, POP],
                &push(&[0]),
                &[MDAP],
                &push(&[1]),
                &[MDAP + 1],
                &push(&[2, 0]),
                &[MIAP],
                &push(&[3, 1]),
                &[MIAP + 1],
                &push(&[4, 64]),
                &[MSIRP],
                &push(&[5, 32]),
                &[MSIRP + 1],
                &push(&[6]),
                &[MDRP + 0x04],
                &push(&[7]),
                &[MDRP + 0x0D],
                &push(&[4]),
                &[MDRP + 0x16],
                &push(&[5, 2]),
                &[MIRP + 0x05],
                &push(&[6, 3]),
                &[MIRP + 0x1E, SVTCA],
                &push(&[1]),
                &[MDAP + 1],
                &push(&[2, 0]),
                &[MIAP + 1],
                &push(&[3]),
                &[MDRP + 0x04],
            ]),
            ..base_input()
        },
    ));

    // The other point instructions, across: SHP and SHC by rp2 and by rp1,
    // SHZ, SHPIX, IP between points 0 and 2, ALIGNRP, ALIGNPTS, ISECT of the
    // bottom edge and the inner contour's left side, UTP, FLIPPT, FLIPRGON,
    // FLIPRGOFF and AA.
    seeds.push(hints(
        "shifting-and-interpolating",
        hinting::Input {
            glyph: cat(&[
                &[SVTCA + 1],
                &push(&[0]),
                &[SRP0],
                &push(&[1]),
                &[SRP1],
                &push(&[2]),
                &[SRP2],
                &push(&[3]),
                &[SHP],
                &push(&[4]),
                &[SHP + 1],
                &push(&[1]),
                &[SHC],
                &push(&[0]),
                &[SHC + 1],
                &push(&[1]),
                &[SHZ],
                &push(&[2, 32]),
                &[SHPIX],
                &push(&[0]),
                &[SRP1],
                &push(&[5]),
                &[IP],
                &push(&[0]),
                &[SRP0],
                &push(&[3]),
                &[ALIGNRP],
                &push(&[1, 2]),
                &[ALIGNPTS],
                &push(&[6, 0, 1, 4, 7]),
                &[ISECT],
                &push(&[6]),
                &[UTP],
                &push(&[5]),
                &[FLIPPT],
                &push(&[4, 5]),
                &[FLIPRGON],
                &push(&[4, 5]),
                &[FLIPRGOFF],
                &push(&[0]),
                &[AA],
            ]),
            ..base_input()
        },
    ));

    // Interpolation and exceptions: IUP both ways after points touched each
    // way, DELTAP1 to DELTAP3 in the glyph, DELTAC1 to DELTAC3 in prep.
    seeds.push(hints(
        "interpolation-and-exceptions",
        hinting::Input {
            prep: cat(&[
                &push(&[0x0F, 0, 1]),
                &[DELTAC1],
                &push(&[0x0F, 1, 1]),
                &[DELTAC2],
                &push(&[0x0F, 2, 1]),
                &[DELTAC3],
            ]),
            glyph: cat(&[
                &[SVTCA + 1],
                &push(&[0]),
                &[MDAP + 1],
                &push(&[2]),
                &[MDAP + 1, IUP + 1, SVTCA],
                &push(&[1]),
                &[MDAP + 1, IUP],
                &push(&[0x3F, 1, 1]),
                &[DELTAP1],
                &push(&[0x0F, 2, 1]),
                &[DELTAP2],
                &push(&[0x0F, 3, 1]),
                &[DELTAP3],
            ]),
            ..base_input()
        },
    ));

    // The composite every way it places a component: the first's offset
    // rounded to the grid and its metrics taken, the second matched by a
    // point and scaled by a half, and the composite's own program over its
    // sixteen points.
    seeds.push(hints(
        "composites-placed-every-way",
        hinting::Input {
            options: hinting::ROUND_XY | hinting::MATCHED | hinting::SCALED | hinting::MY_METRICS,
            glyph: cat(&[&[SVTCA + 1], &push(&[0]), &[MDAP + 1]]),
            composite: cat(&[
                &[SVTCA + 1],
                &push(&[0]),
                &[MDAP + 1],
                &push(&[9]),
                &[MDAP + 1, IUP + 1],
            ]),
            ..base_input()
        },
    ));

    // maxp at every ceiling: function 32 767 defined and called, storage
    // location 32 767 written and read, twilight point 16 383 moved.
    seeds.push(hints(
        "maxp-at-its-ceilings",
        hinting::Input {
            profile: [u16::MAX; 5],
            fpgm: cat(&[&push(&[32_767]), &[FDEF], &push(&[1]), &[POP, ENDF]]),
            prep: cat(&[&push(&[32_767, 5]), &[WS]]),
            glyph: cat(&[
                &push(&[0]),
                &[SZP0],
                &push(&[16_383]),
                &[MDAP + 1],
                &push(&[1]),
                &[SZP0],
                &push(&[32_767]),
                &[CALL],
                &push(&[32_767]),
                &[RS, POP],
            ]),
            ..base_input()
        },
    ));

    // A gasp that grid-fits to 16 pixels: 12 hints and 33 is declined.
    seeds.push(HintingSeed {
        name: "gasp-declines-past-16",
        bytes: hinting::Input {
            sizes: 0b0010_1000,
            options: hinting::GASP,
            glyph: cat(&[&[SVTCA + 1], &push(&[0]), &[MDAP + 1]]),
            ..base_input()
        }
        .encode(),
        comes: Comes::Declined,
    });

    // Every size, from a pixel to 1 024, monochrome and at integer sizes,
    // prep turning dropout control on.
    seeds.push(hints(
        "every-size-monochrome-and-integer",
        hinting::Input {
            sizes: 0xFF,
            options: hinting::MONOCHROME | hinting::INTEGER_PPEM,
            prep: cat(&[&push(&[0x1FF]), &[SCANCTRL], &push(&[5]), &[SCANTYPE]]),
            glyph: cat(&[&[SVTCA + 1], &push(&[0]), &[MDAP + 1, IUP + 1]]),
            ..base_input()
        },
    ));

    // The empty input: every field zero, no program and no point.
    seeds.push(HintingSeed {
        name: "empty",
        bytes: Vec::new(),
        comes: Comes::Hints,
    });

    // fpgm jumps back to its own start for ever: −3 pushed, then JMPR.
    seeds.push(fails(
        "fpgm-runs-to-its-budget",
        hinting::Input {
            fpgm: vec![PUSHW, 0xFF, 0xFD, JMPR],
            ..base_input()
        },
        Program::Font,
        HintError::Budget,
    ));

    // Function 0 calls itself, past 64 nested calls.
    seeds.push(fails(
        "calls-nest-past-their-depth",
        hinting::Input {
            fpgm: cat(&[&push(&[0]), &[FDEF], &push(&[0]), &[CALL, ENDF]]),
            glyph: cat(&[&push(&[0]), &[CALL]]),
            ..base_input()
        },
        Program::Glyph,
        HintError::CallDepth,
    ));

    // Each loop pushes one value more than it pops: 1, then −5 and a JMPR
    // back to the start, past the stack's 288 entries.
    seeds.push(fails(
        "the-stack-past-its-bound",
        hinting::Input {
            glyph: vec![PUSHB, 1, PUSHW, 0xFF, 0xFB, JMPR],
            ..base_input()
        },
        Program::Glyph,
        HintError::StackOverflow,
    ));

    // A definition with no ENDF.
    seeds.push(fails(
        "a-definition-with-no-end",
        hinting::Input {
            fpgm: cat(&[&push(&[0]), &[FDEF], &push(&[1]), &[POP]]),
            ..base_input()
        },
        Program::Font,
        HintError::UnterminatedDefinition,
    ));

    // prep divides by zero, so every size is unhinted.
    seeds.push(fails(
        "prep-divides-by-zero",
        hinting::Input {
            prep: cat(&[&push(&[1, 0]), &[DIV]]),
            ..base_input()
        },
        Program::ControlValue,
        HintError::DivideByZero,
    ));

    seeds
}

/// The instruction family `op` belongs to, as `docs/design/hinting.md`'s
/// milestones divide the set and `src/hint/tests/` holds them, a file a
/// family; an opcode no instruction has is flow's, where an `IDEF` defines
/// it.
fn family(op: u8) -> &'static str {
    match op {
        0x40
        | 0x41
        | 0xB0..=0xBF
        | 0x20..=0x26
        | 0x8A
        | 0x50..=0x55
        | 0x5A..=0x5C
        | 0x60..=0x67
        | 0x8B
        | 0x8C
        | 0x4F => "stack",
        0x43 | 0x42 | 0x45 | 0x44 | 0x70 | 0x4B | 0x4C | 0x88 | 0x8E => "storage",
        0x00..=0x0E
        | 0x10..=0x1A
        | 0x1D..=0x1F
        | 0x3D
        | 0x4D
        | 0x4E
        | 0x56
        | 0x57
        | 0x5E
        | 0x5F
        | 0x68..=0x6F
        | 0x76
        | 0x77
        | 0x7A
        | 0x7C..=0x7E
        | 0x85..=0x87
        | 0x8D => "state",
        0x46..=0x4A | 0x2E | 0x2F | 0x3E | 0x3F | 0x3A | 0x3B | 0xC0..=0xFF => "moves",
        0x32..=0x39 | 0x3C | 0x27 | 0x0F | 0x29 | 0x80..=0x82 | 0x7F => "shift",
        0x30 | 0x31 | 0x5D | 0x71..=0x75 => "iup",
        _ => "flow",
    }
}

/// The opcodes of `code`, walked an instruction at a time: a push's data is
/// passed over, and a push cut short ends the walk.
fn opcodes(code: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(&op) = code.get(at) {
        out.push(op);
        let count = code.get(at + 1).map_or(0, |&n| usize::from(n));
        at += match op {
            0x40 => 2 + count,
            0x41 => 2 + 2 * count,
            0xB0..=0xB7 => 1 + usize::from(op - 0xAF),
            0xB8..=0xBF => 1 + 2 * usize::from(op - 0xB7),
            _ => 1,
        };
    }
    out
}

/// Writes `fuzz/corpus/truetype_hinting` from [`hinting_seeds`], each seed
/// under its name. Run with `--ignored` when a seed changes; the corpus is
/// committed, and a run that rewrites it is a diff to read.
#[test]
#[ignore = "writes fuzz/corpus/truetype_hinting, which is committed"]
fn write_the_truetype_hinting_seeds() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/truetype_hinting");
    std::fs::create_dir_all(&dir).expect("the corpus directory");
    for seed in hinting_seeds() {
        std::fs::write(dir.join(seed.name), &seed.bytes).expect("a seed is written");
    }
}

/// **The `truetype_hinting` seeds reach the interpreter** (FT-04b.13): the
/// corpus is [`hinting_seeds`]' bytes, seed for seed; every seed comes to
/// what it was written for, each refusal a named `Unhinted`, through the
/// target's own run; the seeds that hint run every instruction family
/// between them; and every prefix of every seed, and every seed with any one
/// byte inverted, reaches an answer rather than a panic.
#[test]
fn the_truetype_hinting_seeds_reach_the_interpreter() {
    let Some(files) = seeds("truetype_hinting") else {
        println!("truetype_hinting-seeds: SKIPPED (no fuzz/corpus/truetype_hinting)");
        return;
    };
    let mut written = hinting_seeds();
    written.sort_by(|a, b| a.name.cmp(b.name));
    let on_disk: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let generated: Vec<(&str, &[u8])> = written
        .iter()
        .map(|seed| (seed.name, seed.bytes.as_slice()))
        .collect();
    assert!(
        on_disk == generated,
        "fuzz/corpus/truetype_hinting is not the seeds this file writes: \
         `cargo test -p tinker-pdf-font --test fuzz_seeds -- --ignored \
         write_the_truetype_hinting_seeds --exact` writes them"
    );

    let mut families: Vec<&str> = Vec::new();
    let mut variants = 0usize;
    for seed in &written {
        let report = hinting::exercise(&seed.bytes);
        let refusals = report.refusals();
        match seed.comes {
            Comes::Hints => assert!(
                refusals.is_empty(),
                "{}: written to hint, refused {refusals:?}",
                seed.name
            ),
            Comes::Fails(program, error) => assert!(
                refusals.iter().any(|why| matches!(
                    why,
                    Unhinted::Failed { program: p, error: e, .. } if *p == program && *e == error
                )),
                "{}: written to fail {program:?} with {error:?}, came to {refusals:?}",
                seed.name
            ),
            Comes::Declined => {
                assert!(
                    !refusals.is_empty()
                        && refusals.iter().all(|why| *why == Unhinted::NotAtThisSize),
                    "{}: written for a declined size, came to {refusals:?}",
                    seed.name
                );
                assert!(
                    report.sizes.iter().any(|(_, size, glyphs)| size.is_ok()
                        && glyphs.len() == 2
                        && glyphs.iter().all(Result::is_ok)),
                    "{}: no size hinted beside the declined one",
                    seed.name
                );
            }
        }
        if !matches!(seed.comes, Comes::Fails(..)) {
            let input = hinting::Input::decode(&seed.bytes);
            for code in [&input.fpgm, &input.prep, &input.glyph, &input.composite] {
                for op in opcodes(code) {
                    let family = family(op);
                    if !families.contains(&family) {
                        families.push(family);
                    }
                }
            }
        }
        for end in 0..seed.bytes.len() {
            let _ = hinting::exercise(&seed.bytes[..end]);
            variants += 1;
        }
        for at in 0..seed.bytes.len() {
            let mut flipped = seed.bytes.clone();
            flipped[at] ^= 0xFF;
            let _ = hinting::exercise(&flipped);
            variants += 1;
        }
    }
    families.sort_unstable();
    assert_eq!(
        families,
        ["flow", "iup", "moves", "shift", "stack", "state", "storage"],
        "every instruction family runs in a seed that hints"
    );
    println!(
        "RAN truetype_hinting-seeds: {} seeds, every family run, {variants} prefixes and \
         flips answered",
        written.len()
    );
}
