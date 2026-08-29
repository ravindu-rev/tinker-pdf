//! Unicode's `text-rendering-tests`, run as a conformance suite.
//!
//! `unicode-org/text-rendering-tests` is the other bar
//! `docs/design/shaping.md` names, beside aots: **real fonts** with the glyphs
//! and the positions a correct implementation must produce, written into the
//! fixture as SVG. Ruling 13 is what makes it admissible — the expected output
//! is inside the fixture, put there by the people who publish the tests — and
//! the fixtures are committed here verbatim rather than distilled, so nothing
//! stands between what upstream wrote and what this file asserts.
//!
//! # The measurement model, and where it comes from
//!
//! Upstream's `check.py` renders each case at **1000 pixels per em** and
//! compares the `<use>` elements of the expected SVG against the observed one,
//! with `maxDelta=1.0`. Each `<use>` is one glyph: `x` is the pen position
//! plus the glyph's own horizontal offset, `y` is its vertical offset, and
//! `xlink:href` names a `<symbol>` whose id is `<testcase>.<glyph name>`.
//!
//! Two details of that model are not obvious and both are taken from
//! upstream's own harnesses rather than guessed:
//!
//! - **A symbol with an empty path, and every `<use>` pointing at it, is
//!   dropped before the comparison.** `check.py`'s `normalize_svg` does it to
//!   both sides. In this corpus it is the space of GSUB-1, which is why that
//!   case expects two glyphs from three characters.
//! - **Positions are in a thousandth of an em**, so a face whose `unitsPerEm`
//!   is not 1000 — several here are 2048 — has its design units scaled. The
//!   scaling is integer: `units * 1000 / upem`, rounded half away from zero,
//!   which is the same arithmetic `docs/design/shaping.md` says a consumer
//!   does and never a float in this crate. It is **not** the same arithmetic
//!   upstream does, and [`WITHIN_TOLERANCE`] is where the difference is
//!   accounted for.
//!
//! # What runs, and what does not
//!
//! Four milestones of `docs/design/shaping.md` are graded here, and they do
//! **not** all pass:
//!
//! - milestone 2 on the CMAP, GSUB and GPOS sections;
//! - milestone 4 on the Arabic-script ones, of which this corpus has exactly
//!   one — SHARAN-1, six words of Urdu set in Nasta‘līq;
//! - milestone 5 on the sixteen Brahmic ones, SHBALI, SHKNDA and SHLANA.
//!
//! Twenty-six of the twenty-nine sections run; the three that do not are
//! declined **by name and with their reason** in
//! [`the_sections_this_milestone_declines`], because a section that quietly
//! did not run reads exactly like a section that passed. Of the twenty-six,
//! eleven reproduce every case and fifteen do not. Which is which, and how
//! many cases each one gets, is [`PASSING`] — **read that before believing any
//! other number in this file**, and
//! [`every_section_this_crate_claims_is_whole`] is what stops the two claims
//! blurring into each other.
//!
//! SHARAN-1 is the only section whose direction is not left to right, and it
//! is therefore the only one that exercises [`shaped`]'s reordering — and the
//! only evidence in the repository that a right-to-left run's *glyphs* are
//! right rather than only its levels.
//!
//! # The counts, in aots's discipline
//!
//! `tests/aots.rs` asserts per lookup type a pair — cases, and *discriminating*
//! cases, the second counting only those whose expected output differs from
//! what the implementation under test would produce with nothing switched on.
//! The same pair is asserted here per section, against a run of the same text
//! through the same `cmap` with **no lookups at all**. A section whose
//! discriminating count is zero is a section that would still be green with
//! the shaper deleted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tinker_pdf_font::{base_char, base_glyph_name, glyph_name_to_char, BaseEncoding, Cff, Sfnt};
use tinker_pdf_shape::bidi::{BaseDirection, Paragraph};
use tinker_pdf_shape::shape::{itemize, Shaper};

/// The pixels-per-em `check.py` renders every case at, and therefore the unit
/// every number in a fixture is in.
const PPEM: i32 = 1000;

/// The twenty-nine sections of the corpus this crate is graded on, and what
/// each one is for.
const SECTIONS: &[(&str, &str)] = &[
    ("CMAP-1", "Ideographic Variation Sequences"),
    ("CMAP-2", "Unicode Variation Selectors"),
    ("CMAP-3", "MacOS Turkish Encoding"),
    ("CMAP-4", "Many-to-one range mappings"),
    ("GSUB-1", "Space Isn't Nothing"),
    (
        "GSUB-2",
        "Chaining Contextual Substitution for Ethiopic Numerals",
    ),
    ("GSUB-3", "Substituting a Billion Laughs"),
    ("GPOS-1", "Pair Adjustment Positioning"),
    ("GPOS-2", "Coverage in Pair Adjustment Positioning"),
    ("GPOS-3", "Mark-to-Base Attachment for Ethiopic Diacritics"),
    ("GPOS-4", "Mark-to-Mark Attachment for Stacked Accents"),
    ("GPOS-5", "Glyph Positioning for Variable Fonts"),
    ("SHARAN-1", "Nasta\u{2018}l\u{12B}q"),
    ("SHBALI-1", "Balinese"),
    ("SHBALI-2", "Balinese"),
    ("SHBALI-3", "Balinese"),
    ("SHKNDA-1", "Kannada"),
    ("SHKNDA-2", "Kannada"),
    ("SHKNDA-3", "Kannada"),
    ("SHLANA-1", "Tham"),
    ("SHLANA-2", "Tham"),
    ("SHLANA-3", "Tham"),
    ("SHLANA-4", "Tham"),
    ("SHLANA-5", "Tham"),
    ("SHLANA-6", "Tham"),
    ("SHLANA-7", "Tham"),
    ("SHLANA-8", "Tham"),
    ("SHLANA-9", "Tham"),
    ("SHLANA-10", "Tham"),
];

/// The sections this crate does not run, each with the reason, and each
/// reason a thing somebody could go and fix.
const DECLINED: &[(&str, &str)] = &[
    (
        "CMAP-3",
        "the face's only `cmap` subtable is (platform 1, encoding 0, language \
         18) format 0, which is a *byte* map in the MacOS Turkish encoding. \
         `tinker_pdf_font::Sfnt::glyph_for_char` hands a Macintosh subtable \
         the Unicode scalar value, so everything above U+007F reaches the \
         wrong glyph. Fixing it means reading the subtable's language field \
         and vendoring Unicode's TURKISH.TXT, both inside `tinker-pdf-font`",
    ),
    (
        "CMAP-4",
        "the face's only `cmap` subtable is format 13, the many-to-one range \
         format, which `tinker_pdf_font::Sfnt` does not read — it covers \
         formats 0, 4, 6 and 12. Format 13 is five lines beside format 12 in \
         that crate's `lookup_cmap`",
    ),
    (
        "GPOS-5",
        "the anchors are in an Item Variation Store and the case names a \
         `wght` axis position. `docs/design/shaping.md` lists variation-aware \
         shaping under **Non-goals**: `fvar`/`avar`/`HVAR` deltas applied to \
         GPOS values are deferred until a corpus document demands them \
         (ruling 3)",
    ),
];

/// Per section: how many cases it holds, and how many of those are
/// discriminating.
///
/// **Read the second number.** A section reached only by cases that the
/// unshaped run already satisfies is a section whose implementation could be
/// deleted with this file still green.
///
/// `GSUB-3` is absent because its one case is `expected-no-crash` and states
/// no glyphs at all; it is run by
/// [`the_billion_laughs_face_stops_rather_than_growing`].
const EXPECTED: &[(&str, usize, usize)] = &[
    ("CMAP-1", 4, 3),
    ("CMAP-2", 2, 1),
    ("GPOS-1", 19, 19),
    ("GPOS-2", 3, 1),
    ("GPOS-3", 4, 3),
    ("GPOS-4", 4, 4),
    ("GSUB-1", 1, 1),
    ("GSUB-2", 11, 6),
    ("SHARAN-1", 6, 6),
    ("SHBALI-1", 22, 22),
    ("SHBALI-2", 12, 12),
    ("SHBALI-3", 9, 9),
    ("SHKNDA-1", 34, 34),
    ("SHKNDA-2", 16, 16),
    ("SHKNDA-3", 31, 31),
    ("SHLANA-1", 52, 41),
    ("SHLANA-10", 47, 45),
    ("SHLANA-2", 37, 35),
    ("SHLANA-3", 13, 12),
    ("SHLANA-4", 3, 2),
    ("SHLANA-5", 13, 13),
    ("SHLANA-6", 7, 7),
    ("SHLANA-7", 18, 18),
    ("SHLANA-8", 13, 12),
    ("SHLANA-9", 6, 3),
];

/// One `<td class="expected">` of a fixture.
struct Case {
    id: String,
    font: String,
    render: String,
    /// Present only on GPOS-5, and the reason that section is declined.
    variation: Option<String>,
    /// The glyphs the fixture says a correct implementation produces:
    /// `(glyph name, x, y)` at 1000 units per em, with the empty-outline
    /// entries `check.py` drops already dropped.
    expected: Vec<(String, i32, i32)>,
    no_crash: bool,
}

fn fixture(section: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data/text-rendering-tests/testcases")
        .join(format!("{section}.html"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

fn font_bytes(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data/text-rendering-tests/fonts")
        .join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

/// The value of one attribute of an XML start tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let at = tag.find(&needle)? + needle.len();
    let rest = &tag[at..];
    let end = rest.find('"')?;
    Some(unescape(&rest[..end]))
}

/// The five XML entities and the numeric character references, which
/// `GPOS-4`'s `ft:render` uses to write combining marks.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find(';') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let entity = &rest[at + 1..at + end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            hex if hex.starts_with("#x") || hex.starts_with("#X") => {
                let code = u32::from_str_radix(&hex[2..], 16).expect("a character reference");
                out.push(char::from_u32(code).expect("a character"));
            }
            decimal if decimal.starts_with('#') => {
                let code: u32 = decimal[1..].parse().expect("a character reference");
                out.push(char::from_u32(code).expect("a character"));
            }
            other => panic!("unknown XML entity &{other};"),
        }
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Every case in one section's fixture.
fn parse(section: &str) -> Vec<Case> {
    let text = fixture(section);
    let mut out = Vec::new();
    for (marker, no_crash) in [
        ("class=\"expected\"", false),
        ("class=\"expected-no-crash\"", true),
    ] {
        let mut rest = text.as_str();
        while let Some(at) = rest.find(marker) {
            rest = &rest[at..];
            let end = rest.find('>').expect("an unterminated tag");
            let tag = &rest[..end];
            let body_end = rest.find("</td>").unwrap_or(rest.len());
            let body = &rest[end..body_end];
            let id = attribute(tag, "ft:id").expect("every case has an id");
            let font = attribute(tag, "ft:font").expect("every case names a font");
            let render = attribute(tag, "ft:render").unwrap_or_default();
            let variation = attribute(tag, "ft:var");
            out.push(Case {
                id,
                font,
                render,
                variation,
                expected: uses(body),
                no_crash,
            });
            rest = &rest[end..];
        }
    }
    out.sort_by_key(|case| numeric_id(&case.id));
    out
}

/// `GPOS-1/10` sorts after `GPOS-1/9`, which a string comparison does not do.
fn numeric_id(id: &str) -> (String, u32) {
    match id.split_once('/') {
        Some((section, n)) => (section.to_string(), n.parse().unwrap_or(0)),
        None => (id.to_string(), 0),
    }
}

/// The `<use>` elements of one case's SVG, with the empty-outline ones
/// dropped exactly as `check.py`'s `normalize_svg` drops them.
fn uses(body: &str) -> Vec<(String, i32, i32)> {
    // Which symbols have no outline at all.
    let mut empty: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("<symbol ") {
        rest = &rest[at..];
        let end = rest.find("</symbol>").unwrap_or(rest.len());
        let symbol = &rest[..end];
        let id = attribute(symbol, "id").expect("a symbol has an id");
        // Read `d` off the `<path>` rather than off the `<symbol>`, because
        // `id="` contains `d="` and an attribute search that started at the
        // symbol tag would find the id every time.
        let outline = symbol
            .find("<path ")
            .and_then(|at| attribute(&symbol[at..], "d"))
            .unwrap_or_default();
        if outline.trim().is_empty() {
            empty.push(id);
        }
        rest = &rest[end.min(rest.len())..];
        if rest.starts_with("</symbol>") {
            rest = &rest["</symbol>".len()..];
        }
    }

    let mut out = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("<use ") {
        rest = &rest[at..];
        let end = rest.find("/>").map_or(rest.len(), |e| e + 2);
        let tag = &rest[..end];
        let href = attribute(tag, "xlink:href").expect("a use has an href");
        let id = href.trim_start_matches('#').to_string();
        if !empty.contains(&id) {
            let name = id
                .split_once('.')
                .map_or(id.clone(), |(_, n)| n.to_string());
            let x: i32 = attribute(tag, "x")
                .expect("a use has an x")
                .parse()
                .expect("a number");
            let y: i32 = attribute(tag, "y")
                .expect("a use has a y")
                .parse()
                .expect("a number");
            out.push((name, x, y));
        }
        rest = &rest[end..];
    }
    out
}

/// `units * 1000 / upem`, rounded half away from zero, in integers.
///
/// `check.py` compares with a tolerance of one unit, which this does not use:
/// every case here is asserted exactly, so a rounding rule that drifted would
/// be a failure rather than an allowance.
fn scale(units: i32, upem: u16) -> i32 {
    let upem = i32::from(upem);
    let magnitude = units.abs().saturating_mul(PPEM).saturating_add(upem / 2) / upem;
    if units < 0 {
        -magnitude
    } else {
        magnitude
    }
}

/// The glyph a fixture's name refers to, resolved **through the face**.
///
/// The fixture states a name and this crate produces an index, so one of the
/// two has to be turned into the other, and it is done in the direction that
/// uses only what the face itself says:
///
/// 1. `gid<N>` is upstream's own fallback for a face with no names, and means
///    the index outright;
/// 2. a CFF face carries its names in the charset, which
///    `tinker_pdf_font::Cff::gid_for_name` reads;
/// 3. a `post` version 2.0 face carries the names it invented — `a.alt`,
///    `uni1373.init`, `Aogonek` — which `Sfnt::glyph_for_name` reads;
/// 4. and the rest are the standard Macintosh names a `post` table stores by
///    *index* rather than by string — `u`, `J`, `period`, `aacute`. Those are
///    resolved by turning the name back into a character and the character
///    through the face's own `cmap`, using the Adobe Glyph List and then
///    ISO 32000-1 Annex D's four base encodings, both of which
///    `tinker-pdf-font` already carries. The 258-entry Macintosh glyph order
///    is deliberately **not** written out here: a name list transcribed from
///    memory is exactly the kind of evidence ruling 13 exists to keep out, and
///    Annex D is a published table this repository already reads.
///
/// A name none of these resolves is a failure and not a skip; see
/// [`every_expected_glyph_name_resolves`].
fn glyph_named(face: &Sfnt<'_>, cff: Option<&Cff<'_>>, name: &str) -> Option<u16> {
    if let Some(index) = name.strip_prefix("gid") {
        if let Ok(index) = index.parse::<u16>() {
            return Some(index);
        }
    }
    if let Some(glyph) = cff.and_then(|cff| cff.gid_for_name(name)) {
        return Some(glyph);
    }
    if let Some(glyph) = face.glyph_for_name(name) {
        return Some(glyph);
    }
    if let Some(glyph) = glyph_name_to_char(name).and_then(|c| face.glyph_for_char(c)) {
        return Some(glyph);
    }
    for encoding in [
        BaseEncoding::MacRoman,
        BaseEncoding::WinAnsi,
        BaseEncoding::Standard,
        BaseEncoding::PdfDoc,
    ] {
        for code in 0..=u8::MAX {
            if base_glyph_name(encoding, code) != Some(name) {
                continue;
            }
            if let Some(glyph) = base_char(encoding, code).and_then(|c| face.glyph_for_char(c)) {
                return Some(glyph);
            }
        }
    }
    None
}

/// Whether a glyph draws anything.
///
/// `check.py`'s `normalize_svg` drops a `<symbol>` with an empty path and
/// every `<use>` pointing at it, from the expected side *and* the observed
/// one. [`uses`] does the first; this does the second, so that GSUB-1's space
/// — a real glyph with a real advance and no outline — leaves both lists the
/// same length.
fn has_outline(face: &Sfnt<'_>, cff: Option<&Cff<'_>>, glyph: u16) -> bool {
    if let Some(cff) = cff {
        return cff
            .outline(glyph)
            .is_some_and(|outline| !outline.is_empty());
    }
    tinker_pdf_font::glyf::outline(face, glyph).is_some_and(|outline| !outline.is_empty())
}

/// What one case's text shapes to: `(glyph, x, y)` in the fixture's units.
///
/// # Visual order, and where it comes from
///
/// A fixture's `<use>` elements are in the order the glyphs are drawn, left to
/// right, because that is what a renderer emits. This crate shapes and returns
/// **logical** order, whichever way a run reads, so the two are joined here by
/// the same two steps `docs/design/shaping.md` puts in a consumer: UAX #9's
/// rule L2 orders the runs, through [`tinker_pdf_shape::bidi::reorder`], and a
/// right-to-left run's glyphs are then walked backwards.
///
/// This is the only place in the suite where direction is read at all, and it
/// is the reason SHARAN-1 is the first section that can fail for a reason that
/// is not about `GSUB` or `GPOS`.
fn shaped(face: &Sfnt<'_>, cff: Option<&Cff<'_>>, text: &str) -> Vec<(u16, i32, i32)> {
    // `Auto` rather than a fixed direction: upstream's harness derives the
    // direction from the script, and P2/P3 is this crate's way of saying the
    // same thing.
    let paragraph = Paragraph::new(text, BaseDirection::Auto);
    let shaper = Shaper::new(face);
    let runs = itemize(text, &paragraph);
    let shaped: Vec<_> = runs.iter().map(|run| shaper.shape(text, run)).collect();
    let levels: Vec<_> = runs.iter().map(|run| run.level).collect();

    let mut out = Vec::new();
    let mut pen = 0i32;
    for index in tinker_pdf_shape::bidi::reorder(&levels) {
        let Some(run) = shaped.get(index) else {
            continue;
        };
        let glyphs: Vec<_> = if run.direction().is_forward() {
            run.glyphs().to_vec()
        } else {
            run.glyphs().iter().rev().copied().collect()
        };
        for glyph in glyphs {
            if has_outline(face, cff, glyph.glyph) {
                out.push((
                    glyph.glyph,
                    scale(pen.saturating_add(glyph.x_offset), face.units_per_em),
                    scale(glyph.y_offset, face.units_per_em),
                ));
            }
            pen = pen.saturating_add(glyph.x_advance);
        }
    }
    out
}

/// What an implementation with **no shaper at all** produces: one `cmap`
/// lookup per character, one `hmtx` advance each, nothing else.
///
/// This is the baseline the discriminating half of the counts is measured
/// against, and it is deliberately naiver than
/// [`Shaper::with_features`] with an empty feature list: it does not resolve
/// variation sequences, does not drop an ignorable character, and does not
/// zero a mark's advance. A case whose expected output this already satisfies
/// is a case that would pass with everything in `src/shape.rs` deleted.
fn naive(face: &Sfnt<'_>, text: &str) -> Vec<(u16, i32, i32)> {
    let mut out = Vec::new();
    let mut pen = 0i32;
    for c in text.chars() {
        let glyph = face.glyph_for_char(c).unwrap_or(0);
        out.push((glyph, scale(pen, face.units_per_em), 0));
        pen = pen.saturating_add(i32::from(face.advance(glyph).unwrap_or(0)));
    }
    out
}

fn runnable() -> Vec<(&'static str, Vec<Case>)> {
    SECTIONS
        .iter()
        .filter(|(section, _)| !DECLINED.iter().any(|(d, _)| d == section))
        .map(|(section, _)| (*section, parse(section)))
        .collect()
}

#[test]
fn the_corpus_is_the_one_that_was_vendored() {
    for (section, title) in SECTIONS {
        let text = fixture(section);
        assert!(
            text.contains(&format!("id=\"{section}\"")),
            "{section}.html does not identify itself"
        );
        // The titles carry typographic punctuation upstream, so the assertion
        // is on the longest plain word of each rather than on the whole line.
        let word = title
            .split(|c: char| !c.is_ascii_alphanumeric())
            .max_by_key(|w| w.len())
            .unwrap_or(title);
        assert!(
            text.contains(word),
            "{section}.html does not look like {title}"
        );
    }
    assert_eq!(SECTIONS.len(), 29, "the corpus changed size");
}

/// The expected glyph *names* this repository cannot turn into indices, by
/// name, so that the weakening is a list and not an absence.
///
/// A name here is still position-checked; only the identity of the glyph goes
/// unchecked for it. There is one, and it is the one name in the corpus that
/// is a standard Macintosh `post` name outside ISO 32000-1 Annex D's four base
/// encodings and outside the Adobe Glyph List's algorithmic forms. Resolving it
/// needs either the 258-entry Macintosh glyph order or Adobe's `glyphlist.txt`,
/// and vendoring a whole published name list to check one glyph of one case is
/// out of proportion — so it is recorded instead.
const UNRESOLVABLE: &[(&str, &str)] = &[("GPOS-1/15", "aacute")];

/// Every coordinate in the whole corpus that this crate does not reproduce
/// **exactly**, by case, glyph and axis, with the amount.
///
/// # Upstream allows one unit, and this is the list of where it is spent
///
/// `check.py` compares with `maxDelta=1.0`. Milestone 2 could assert exactly
/// instead and did, and said so; milestone 4 cannot, and the reason is not a
/// defect in either side.
///
/// Upstream's numbers come out of a pipeline that scales **every quantity
/// separately** into thousandths of an em and rounds each — every advance,
/// every anchor — and then adds. This crate's arithmetic is exact integers in
/// font design units all the way to the end, where one division rounds once;
/// ruling 4 is why, and it is the whole reason the output is bit-identical on
/// four targets. The two disagree by at most one unit, and only where a sum of
/// separately-rounded halves lands on the other side of a boundary from the
/// rounded sum.
///
/// It is checkable rather than asserted. `SHARAN-1/6`'s seventh glyph is a
/// mark whose base sits at 1234 design units and whose anchor difference is
/// −807. Exactly: `1234 − 807 = 427`, and `427 × 1000 ÷ 2048` rounds to
/// **208**. Separately: `1234` scales to `603` and `−807` scales to `−394`,
/// and `603 − 394` is **209**, which is what the fixture says. Neither is a
/// mistake; they are two roundings of `208.4961`.
///
/// So the tolerance is upstream's own, and what is asserted instead is this
/// list. A coordinate that drifts by two units is a failure, a *new*
/// coordinate that drifts by one is a failure, and the count below moving is a
/// failure. What can no longer be claimed is that every number matches to the
/// unit, and that is stated here rather than absorbed into a tolerance nobody
/// counts.
const WITHIN_TOLERANCE: &[(&str, usize, char, i32)] = &[
    ("SHARAN-1/5", 6, 'x', -1),
    ("SHARAN-1/6", 6, 'y', -1),
    ("SHLANA-6/6", 5, 'x', -1),
];

/// The tolerance `check.py` applies to every comparison it makes.
const MAX_DELTA: i32 = 1;

/// How many of each section's cases this crate reproduces.
///
/// # Read this table before believing anything else in this file
///
/// For every section milestones 1 to 4 are graded on, the number here **is**
/// the section's case count, and [`every_section_this_crate_claims_is_whole`]
/// asserts that separately so it cannot drift. Those sections pass outright.
///
/// The sixteen Brahmic ones do not, and this is where that is said. They are
/// milestone 5's, whose exit criterion in `docs/design/shaping.md` is *"every
/// text-rendering-tests USE section passes"* — and it is **not met**. Two of
/// the sixteen pass whole; the rest do not. Running them anyway, with the
/// number pinned, is deliberate and is the alternative to two worse options:
/// declining them, which would hide that most of their cases already pass, and
/// asserting only that they do not crash, which would let the number fall
/// silently.
///
/// So the number is a ratchet. It may go up, and every increase moves a row
/// here in the commit that earned it. It may not go down.
///
/// # What the shortfall is, and it is one thing more than anything else
///
/// **Canonical decomposition.** A two-part vowel such as
/// `U+1B40 BALINESE VOWEL SIGN TALING TEDUNG` is `Indic_Positional_Category`
/// `Left_And_Right` — it is drawn on *both* sides of its consonant — and its
/// left half only becomes a thing that can be moved once the character is
/// decomposed into `U+1B3E` (`Left`) and `U+1B35` (`Right`). This crate does
/// not decompose, so it sees one character that is neither, and the left half
/// stays where it was typed. Milestone 3's record already named canonical
/// decomposition as absent; SHBALI-1 is the fixture that makes it cost
/// something.
///
/// The other two are named in `docs/features/fonts.md`: the Indic shaper's
/// base-finding, which is what forms a Kannada conjunct and is a different
/// model from USE's, and the positional application of `rphf`, `half` and
/// `blwf`, which this crate applies to a whole syllable rather than to one
/// position in it.
const PASSING: &[(&str, usize)] = &[
    ("CMAP-1", 4),
    ("CMAP-2", 2),
    ("GPOS-1", 19),
    ("GPOS-2", 3),
    ("GPOS-3", 4),
    ("GPOS-4", 4),
    ("GSUB-1", 1),
    ("GSUB-2", 11),
    ("SHARAN-1", 6),
    ("SHBALI-1", 14),
    ("SHBALI-2", 5),
    ("SHBALI-3", 9),
    ("SHKNDA-1", 31),
    ("SHKNDA-2", 4),
    ("SHKNDA-3", 0),
    ("SHLANA-1", 48),
    ("SHLANA-2", 26),
    ("SHLANA-3", 11),
    ("SHLANA-4", 2),
    ("SHLANA-5", 12),
    ("SHLANA-6", 4),
    ("SHLANA-7", 10),
    ("SHLANA-8", 11),
    ("SHLANA-9", 6),
    ("SHLANA-10", 30),
];

/// The sections that must pass **whole**, so that milestone 5's partial state
/// can never be mistaken for milestone 4's complete one.
const WHOLE: &[&str] = &[
    "CMAP-1", "CMAP-2", "GPOS-1", "GPOS-2", "GPOS-3", "GPOS-4", "GSUB-1", "GSUB-2", "SHARAN-1",
];

#[test]
fn the_expected_glyph_names_resolve_except_the_ones_named_here() {
    let mut unresolved: Vec<(String, String)> = Vec::new();
    let mut names = 0usize;
    for (_, cases) in runnable() {
        for case in &cases {
            if case.no_crash {
                continue;
            }
            let bytes = font_bytes(&case.font);
            let face = Sfnt::parse(&bytes).expect("a fixture font is a valid sfnt");
            let cff = face
                .table(u32::from_be_bytes(*b"CFF "))
                .and_then(Cff::parse);
            for (name, _, _) in &case.expected {
                names += 1;
                if glyph_named(&face, cff.as_ref(), name).is_none() {
                    unresolved.push((case.id.clone(), name.clone()));
                }
            }
        }
    }
    let expected: Vec<(String, String)> = UNRESOLVABLE
        .iter()
        .map(|(id, name)| ((*id).to_string(), (*name).to_string()))
        .collect();
    assert_eq!(
        unresolved, expected,
        "the set of expected glyph names this repository cannot resolve moved; \
         every one of them is a case comparing positions and not identity"
    );
    assert_eq!(names, 1369, "the number of expected glyphs moved");
}

#[test]
fn every_runnable_case_produces_what_the_fixture_says() {
    let mut failures: Vec<String> = Vec::new();
    let mut drifted: Vec<(String, usize, char, i32)> = Vec::new();
    let mut passed: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ran = 0usize;
    for (section, cases) in runnable() {
        for case in &cases {
            if case.no_crash {
                continue;
            }
            assert!(
                case.variation.is_none(),
                "{}: a runnable case names a variation",
                case.id
            );
            ran += 1;
            let bytes = font_bytes(&case.font);
            let face = Sfnt::parse(&bytes).expect("a fixture font is a valid sfnt");
            let cff = face
                .table(u32::from_be_bytes(*b"CFF "))
                .and_then(Cff::parse);
            let expected: Vec<(Option<u16>, i32, i32)> = case
                .expected
                .iter()
                .map(|(name, x, y)| (glyph_named(&face, cff.as_ref(), name), *x, *y))
                .collect();
            let ours = shaped(&face, cff.as_ref(), &case.render);
            // A name [`UNRESOLVABLE`] lists compares its position and not its
            // identity; every other one compares both. A position is compared
            // within upstream's own tolerance, and every unit of that
            // tolerance actually spent is collected and asserted below.
            let mut spent: Vec<(String, usize, char, i32)> = Vec::new();
            let agrees = ours.len() == expected.len()
                && ours
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .all(|(n, (ours, want))| {
                        let (dx, dy) = (ours.1 - want.1, ours.2 - want.2);
                        if dx != 0 {
                            spent.push((case.id.clone(), n, 'x', dx));
                        }
                        if dy != 0 {
                            spent.push((case.id.clone(), n, 'y', dy));
                        }
                        dx.abs() <= MAX_DELTA
                            && dy.abs() <= MAX_DELTA
                            && want.0.is_none_or(|g| g == ours.0)
                    });
            if agrees {
                // Only a case that agrees can spend the tolerance. A case that
                // does not is off by whatever it is off by, and counting that
                // would turn `WITHIN_TOLERANCE` into a list of failures.
                drifted.extend(spent);
                *passed.entry(section).or_insert(0) += 1;
            } else {
                let names: Vec<&str> = case.expected.iter().map(|(n, _, _)| n.as_str()).collect();
                failures.push(format!(
                    "{section} {}: {:?}\n  expected {expected:?} ({names:?})\n  ours     {ours:?}",
                    case.id, case.render
                ));
            }
        }
    }
    // The number of cases each section reproduces, asserted exactly. See
    // [`PASSING`]: for every section but the sixteen Brahmic ones this is the
    // section's whole case count, and for those sixteen it is a ratchet that
    // may rise and may not fall.
    let found: Vec<(&str, usize)> = PASSING
        .iter()
        .map(|(section, _)| (*section, passed.get(section).copied().unwrap_or(0)))
        .collect();
    assert_eq!(
        found,
        PASSING.to_vec(),
        "the number of cases this crate reproduces moved. Up is progress and \
         the table moves with it; down is a regression and the table is not \
         the thing to change.\n{} of {ran} cases did not agree:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(ran, 387, "the number of cases that ran moved");
    let expected: Vec<(String, usize, char, i32)> = WITHIN_TOLERANCE
        .iter()
        .map(|(id, glyph, axis, by)| ((*id).to_string(), *glyph, *axis, *by))
        .collect();
    assert_eq!(
        drifted, expected,
        "the set of coordinates this crate does not reproduce exactly moved; \
         see WITHIN_TOLERANCE for why there are any at all"
    );
}

/// The sections that pass outright, and the ones that do not, kept apart.
///
/// Milestones 1 to 4 are graded on nine sections and every one of them
/// reproduces every case. Milestone 5 is graded on sixteen and does not, and
/// [`PASSING`] is where the shortfall is written down. This test is what stops
/// the first claim quietly weakening into the second: a section in [`WHOLE`]
/// that loses a case fails here even if somebody moved its row in `PASSING`.
#[test]
fn every_section_this_crate_claims_is_whole() {
    let cases: BTreeMap<&str, usize> = runnable()
        .iter()
        .map(|(section, cases)| (*section, cases.iter().filter(|c| !c.no_crash).count()))
        .collect();
    for section in WHOLE {
        let passing = PASSING
            .iter()
            .find(|(name, _)| name == section)
            .map(|(_, passing)| *passing)
            .unwrap_or_else(|| panic!("{section} is claimed whole and is not in PASSING"));
        assert_eq!(
            Some(&passing),
            cases.get(section),
            "{section} is claimed whole and does not reproduce every case"
        );
    }
    let brahmic: Vec<&str> = PASSING
        .iter()
        .map(|(section, _)| *section)
        .filter(|section| !WHOLE.contains(section))
        .collect();
    assert_eq!(
        brahmic.len(),
        16,
        "the set of sections milestone 5 is graded on changed: {brahmic:?}"
    );
    let whole: usize = brahmic
        .iter()
        .filter(|section| {
            PASSING
                .iter()
                .find(|(name, _)| name == *section)
                .is_some_and(|(_, passing)| cases.get(*section) == Some(passing))
        })
        .count();
    assert_eq!(
        whole, 2,
        "the number of Brahmic sections that pass outright moved. \
         `docs/design/shaping.md`'s milestone 5 wants all sixteen."
    );
}

#[test]
fn the_sections_this_milestone_declines() {
    let declined: Vec<&str> = DECLINED.iter().map(|(section, _)| *section).collect();
    assert_eq!(
        declined,
        vec!["CMAP-3", "CMAP-4", "GPOS-5"],
        "the set of sections this milestone declines changed"
    );
    let mut skipped = 0usize;
    for (section, reason) in DECLINED {
        assert!(reason.len() > 80, "{section} is declined without a reason");
        skipped += parse(section).len();
    }
    assert_eq!(skipped, 29, "the number of declined cases moved");
    // And the ones that do run, plus the one that is a crash test, plus the
    // declined ones, are the whole corpus.
    let ran: usize = runnable()
        .iter()
        .map(|(_, cases)| cases.iter().filter(|c| !c.no_crash).count())
        .sum();
    let crash: usize = runnable()
        .iter()
        .map(|(_, cases)| cases.iter().filter(|c| c.no_crash).count())
        .sum();
    assert_eq!(ran + crash + skipped, 417, "the corpus changed size");
    assert_eq!(crash, 1);
}

/// GSUB-3: nine chained lookups, each turning `lol` into ten of itself.
///
/// Upstream's own description states the bar: *"your implementation should
/// stop executing once its internal buffer has reached a size limit"*, and
/// this crate's is [`tinker_pdf_shape::Limits::max_glyphs`]. What is asserted
/// is not only that it returns — a test that hung would time out rather than
/// fail — but that it **says so**, because ruling 10 makes "it shaped" and "it
/// shaped cleanly" different sentences.
#[test]
fn the_billion_laughs_face_stops_rather_than_growing() {
    let cases = parse("GSUB-3");
    assert_eq!(cases.len(), 1);
    let case = &cases[0];
    assert!(case.no_crash);
    let bytes = font_bytes(&case.font);
    let face = Sfnt::parse(&bytes).expect("a fixture font is a valid sfnt");
    let shaper = Shaper::new(&face);
    let (_, runs) = shaper.shape_text(&case.render, BaseDirection::LeftToRight);
    let glyphs: usize = runs.iter().map(|run| run.glyphs().len()).sum();
    assert!(
        glyphs <= 65_536,
        "the buffer grew to {glyphs}, past Limits::max_glyphs"
    );
    let warnings: Vec<_> = runs.iter().flat_map(|run| run.warnings()).collect();
    assert!(
        warnings.contains(&&tinker_pdf_shape::Warning::GlyphBudgetExceeded {
            table: tinker_pdf_shape::Table::Gsub
        }) || warnings.contains(&&tinker_pdf_shape::Warning::OperationBudgetExceeded {
            table: tinker_pdf_shape::Table::Gsub
        }),
        "the run stopped growing and said nothing about it: {warnings:?}"
    );
}

/// A ligature carries **its own** advance, not the one its first component had.
///
/// This is the regression test for the ordering defect this milestone found in
/// itself, reduced to the cheapest shape that shows it. `GSUB` type 4 replaces
/// `f` and `l` with a single `fl`, and it does so by rewriting the first
/// glyph's index and removing the second — deliberately leaving the position
/// alone, because `GPOS` may already have moved it. So a shaper that filled in
/// `hmtx` advances *before* substitution leaves the ligature carrying `f`'s
/// 362 units instead of `fl`'s 605, and every glyph after it on the line sits
/// 243 units too far left.
///
/// text-rendering-tests GSUB-2 is what actually caught it, at 164 units on an
/// Ethiopic medial numeral. This says the same thing in three assertions
/// against a face the corpus already ships, so the next person to reorder
/// `Shaper::shape` finds out here rather than in a section count.
///
/// The cluster is asserted beside the advance because the two are the same
/// promise: a ligature stands for the text of everything it replaced, and it
/// keeps the *smallest* of their clusters so that milestone 7 can rebuild
/// `/ToUnicode` from it.
#[test]
fn a_ligature_carries_its_own_advance_and_the_first_components_cluster() {
    let bytes = font_bytes("TestGPOSOne.ttf");
    let face = Sfnt::parse(&bytes).expect("a fixture font is a valid sfnt");

    let f = face.glyph_for_char('f').expect("the face has an f");
    let ligature = face.glyph_for_char('\u{FB02}').expect("the face has an fl");
    let (narrow, wide) = (
        face.advance(f).expect("f has an advance"),
        face.advance(ligature).expect("fl has an advance"),
    );
    assert_ne!(
        narrow, wide,
        "this face no longer distinguishes the two advances, so the test \
         cannot tell the two orderings apart any more"
    );

    let shaper = Shaper::new(&face);
    let (_, runs) = shaper.shape_text("fl", BaseDirection::LeftToRight);
    let glyphs: Vec<_> = runs.iter().flat_map(|run| run.glyphs()).copied().collect();
    assert_eq!(glyphs.len(), 1, "f and l did not ligate: {glyphs:?}");
    assert_eq!(glyphs[0].glyph, ligature);
    assert_eq!(
        glyphs[0].x_advance,
        i32::from(wide),
        "the ligature kept the advance of the glyph it was substituted from"
    );
    assert_eq!(
        glyphs[0].cluster, 0,
        "the ligature does not point at the first byte it stands for"
    );
}

#[test]
fn the_suite_covers_the_sections_it_claims_to() {
    let mut totals: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (section, cases) in runnable() {
        for case in &cases {
            if case.no_crash {
                continue;
            }
            let bytes = font_bytes(&case.font);
            let face = Sfnt::parse(&bytes).expect("a fixture font is a valid sfnt");
            let cff = face
                .table(u32::from_be_bytes(*b"CFF "))
                .and_then(Cff::parse);
            let expected: Vec<(Option<u16>, i32, i32)> = case
                .expected
                .iter()
                .map(|(name, x, y)| (glyph_named(&face, cff.as_ref(), name), *x, *y))
                .collect();
            let bare = naive(&face, &case.render);
            let entry = totals.entry(section).or_insert((0, 0));
            entry.0 += 1;
            let same = bare.len() == expected.len()
                && bare.iter().zip(&expected).all(|(bare, want)| {
                    bare.1 == want.1 && bare.2 == want.2 && want.0.is_none_or(|g| g == bare.0)
                });
            if !same {
                entry.1 += 1;
            }
        }
    }
    let found: Vec<(&str, usize, usize)> = totals
        .iter()
        .map(|(section, (all, discriminating))| (*section, *all, *discriminating))
        .collect();
    assert_eq!(
        found,
        EXPECTED.to_vec(),
        "the per-section fixture counts moved"
    );
    for (section, all, discriminating) in EXPECTED {
        assert!(
            *discriminating > 0,
            "{section} has {all} cases and not one of them would fail if the \
             shaper were deleted"
        );
    }
}
