//! A record of what [`crate::Shaper::shape`] does to a run, kept for the one
//! test that reads it: FT-08.1's trace of the `TRIAGE` rows of
//! `tests/text_rendering.rs` whose cause the tree did not record
//! (`docs/design/shaping.md`, *The traced rows*).
//!
//! Test-only, as `apply::probes` is. The hooks in `crate::shape` and
//! `crate::apply` call into here under `cfg(test)` and nowhere else, so no
//! build a caller makes carries any of it, and nothing here moves a glyph:
//! with no mode set every hook returns at once.
//!
//! # Two modes
//!
//! **Recording** keeps the buffer after `cmap`, after each `GSUB` stage, after
//! the reordering pause and after `GPOS`, and the buffer before and after
//! every lookup the runner runs. **Replaying** runs one lookup again over the
//! buffer it saw and keeps each position it was tried at and where its
//! context stopped matching: the matcher's own answer, from the matcher's own
//! code, rather than a second reading of the subtable that could disagree
//! with the first.
//!
//! A third switch lifts the per-syllable `GSUB` confinement for one run, so
//! the trace can say which lookups the confinement is what stops.

use std::cell::RefCell;

use crate::buffer::Buffer;
use crate::Table;

/// Which part of a rule a miss is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// The input sequence: the glyphs the rule rewrites.
    Input,
    /// The glyphs before it.
    Backtrack,
    /// The glyphs after it.
    Lookahead,
}

/// Why a rule stopped matching at a glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The glyph there is not one the rule names.
    Glyph,
    /// The feature's mask does not reach the glyph.
    Mask,
    /// The glyph is in another cluster than the one the rule started in.
    Syllable,
    /// No glyph the lookup can see is left in that direction.
    End,
    /// The rule is longer than `Limits::max_context_length`.
    TooLong,
}

/// One place a rule stopped matching.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Miss {
    part: Part,
    step: usize,
    at: Option<usize>,
    glyph: Option<u16>,
    cause: Cause,
}

/// The buffer at one named moment of a run.
struct Snapshot {
    label: String,
    buffer: Buffer,
}

/// One lookup the runner ran, and the buffer either side of it.
struct Ran {
    table: Table,
    lookup: u16,
    mask: u32,
    /// How many snapshots had been taken when it ran: the snapshot at this
    /// index is the one its stage ends with.
    stage: usize,
    before: Buffer,
    after: Buffer,
}

/// One position a replayed lookup was tried at.
struct Attempt {
    at: usize,
    glyph: u16,
    syllable: u16,
    applied: bool,
    misses: Vec<Miss>,
    nested: Vec<(u16, usize)>,
}

/// What is kept, and when.
enum Mode {
    Recording {
        notes: Vec<String>,
        stages: Vec<Snapshot>,
        ran: Vec<Ran>,
    },
    Replaying {
        attempts: Vec<Attempt>,
        pending: Vec<Miss>,
        nested: Vec<(u16, usize)>,
        passed: Vec<(usize, &'static str)>,
    },
}

thread_local! {
    static MODE: RefCell<Option<Mode>> = const { RefCell::new(None) };
    static UNCONFINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the per-syllable `GSUB` confinement is lifted on this thread: the
/// trace's second run of a row, which says what the confinement blocks.
pub(crate) fn unconfined() -> bool {
    UNCONFINED.with(std::cell::Cell::get)
}

/// Whether a run is being recorded on this thread.
pub(crate) fn recording() -> bool {
    MODE.with(|mode| matches!(*mode.borrow(), Some(Mode::Recording { .. })))
}

/// A line about the run as a whole: its plan and the script tag it asked.
pub(crate) fn note(text: String) {
    MODE.with(|mode| {
        if let Some(Mode::Recording { notes, .. }) = mode.borrow_mut().as_mut() {
            notes.push(text);
        }
    });
}

/// The buffer after one named moment of the run.
pub(crate) fn stage(label: &str, buffer: &Buffer) {
    MODE.with(|mode| {
        if let Some(Mode::Recording { stages, .. }) = mode.borrow_mut().as_mut() {
            stages.push(Snapshot {
                label: label.to_string(),
                buffer: buffer.clone(),
            });
        }
    });
}

/// One lookup the runner ran, with the buffer before and after it.
pub(crate) fn lookup_ran(table: Table, lookup: u16, mask: u32, before: &Buffer, after: &Buffer) {
    MODE.with(|mode| {
        if let Some(Mode::Recording { ran, stages, .. }) = mode.borrow_mut().as_mut() {
            ran.push(Ran {
                table,
                lookup,
                mask,
                stage: stages.len(),
                before: before.clone(),
                after: after.clone(),
            });
        }
    });
}

/// Where a rule stopped matching, while a lookup is replayed.
pub(crate) fn missed(part: Part, step: usize, at: Option<usize>, glyph: Option<u16>, cause: Cause) {
    MODE.with(|mode| {
        if let Some(Mode::Replaying { pending, .. }) = mode.borrow_mut().as_mut() {
            pending.push(Miss {
                part,
                step,
                at,
                glyph,
                cause,
            });
        }
    });
}

/// A rule that matched and named a lookup that then did not apply.
pub(crate) fn nested_missed(lookup: u16, at: usize) {
    MODE.with(|mode| {
        if let Some(Mode::Replaying { nested, .. }) = mode.borrow_mut().as_mut() {
            nested.push((lookup, at));
        }
    });
}

/// A position the replayed lookup stepped over without trying.
pub(crate) fn passed_over(at: usize, why: &'static str) {
    MODE.with(|mode| {
        if let Some(Mode::Replaying { passed, .. }) = mode.borrow_mut().as_mut() {
            passed.push((at, why));
        }
    });
}

/// The end of one attempt at one position, which takes the misses recorded
/// since the last.
pub(crate) fn attempted(at: usize, glyph: u16, syllable: u16, applied: bool) {
    MODE.with(|mode| {
        if let Some(Mode::Replaying {
            attempts,
            pending,
            nested,
            ..
        }) = mode.borrow_mut().as_mut()
        {
            attempts.push(Attempt {
                at,
                glyph,
                syllable,
                applied,
                misses: std::mem::take(pending),
                nested: std::mem::take(nested),
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Write as _;
    use std::path::PathBuf;

    use tinker_pdf_font::{glyph_name_to_char, Cff, Sfnt};

    use super::{Attempt, Buffer, Cause, Miss, Mode, Part, Ran, Snapshot, Table, MODE};
    use crate::apply::Runner;
    use crate::bidi::{BaseDirection, Paragraph};
    use crate::common::{Coverage, Lookup, Tag};
    use crate::limits::Limits;
    use crate::read::Bytes;
    use crate::shape::itemize;
    use crate::unicode::{self, Script};
    use crate::{Layout, LayoutTable, Shaper};

    /// The twenty-six rows, in `TRIAGE`'s order: the fifteen expecting gid311
    /// where this crate draws gid314, `SHBALI-1/14` and `/15`, the four
    /// `Advance`, `SHLANA-10/4`, `SHLANA-10/28`, and the three the
    /// confinement blocks. `SHKNDA-2`'s six are not here; `universal.rs`
    /// records their cause and its price.
    const ROWS: &[&str] = &[
        "SHBALI-1/14",
        "SHBALI-1/15",
        "SHLANA-1/35",
        "SHLANA-2/2",
        "SHLANA-2/3",
        "SHLANA-2/4",
        "SHLANA-2/7",
        "SHLANA-2/35",
        "SHLANA-3/3",
        "SHLANA-4/2",
        "SHLANA-7/5",
        "SHLANA-7/17",
        "SHLANA-8/5",
        "SHLANA-8/6",
        "SHLANA-10/4",
        "SHLANA-10/8",
        "SHLANA-10/28",
        "SHLANA-10/29",
        "SHLANA-10/30",
        "SHLANA-10/38",
        "SHLANA-10/39",
        "SHLANA-10/40",
        "SHLANA-10/42",
        "SHLANA-10/45",
        "SHLANA-10/46",
        "SHLANA-10/47",
    ];

    /// The units every fixture position is in: a thousandth of an em.
    const PPEM: i32 = 1000;

    /// One case of a fixture: its font, its text and what it expects, as
    /// `(glyph name, x, y)` with the empty-outline glyphs dropped.
    struct Case {
        font: String,
        render: String,
        expected: Vec<(String, i32, i32)>,
        /// Each glyph name's outline as the fixture draws it: its
        /// `<symbol>`'s path, in thousandths of an em.
        symbols: BTreeMap<String, String>,
    }

    fn data(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/text-rendering-tests")
            .join(path)
    }

    fn attribute(tag: &str, name: &str) -> Option<String> {
        let needle = format!("{name}=\"");
        let at = tag.find(&needle)? + needle.len();
        let rest = tag.get(at..)?;
        let end = rest.find('"')?;
        Some(unescape(rest.get(..end)?))
    }

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

    /// The case `id` names, read from its section's fixture.
    fn case(id: &str) -> Case {
        let section = id.split_once('/').map_or(id, |(section, _)| section);
        let text = std::fs::read_to_string(data(&format!("testcases/{section}.html")))
            .expect("a vendored fixture");
        let marker = format!("ft:id=\"{id}\"");
        let at = text.find(&marker).expect("the case is in its fixture");
        let start = text[..at].rfind("<td ").expect("a case is a <td>");
        let rest = &text[start..];
        let tag_end = rest.find('>').expect("a terminated tag");
        let tag = &rest[..tag_end];
        let body_end = rest.find("</td>").unwrap_or(rest.len());
        let body = &rest[tag_end..body_end];
        let mut symbols = BTreeMap::new();
        let mut scan = body;
        while let Some(at) = scan.find("<symbol ") {
            scan = &scan[at..];
            let end = scan.find("</symbol>").unwrap_or(scan.len());
            let symbol = &scan[..end];
            if let Some(id) = attribute(symbol, "id") {
                let name = id
                    .split_once('.')
                    .map_or(id.clone(), |(_, n)| n.to_string());
                let path = symbol
                    .find("<path ")
                    .and_then(|at| attribute(&symbol[at..], "d"))
                    .unwrap_or_default();
                symbols.insert(name, path);
            }
            scan = &scan[end..];
            if let Some(after) = scan.strip_prefix("</symbol>") {
                scan = after;
            }
        }
        Case {
            font: attribute(tag, "ft:font").expect("a case names its font"),
            render: attribute(tag, "ft:render").unwrap_or_default(),
            expected: uses(body),
            symbols,
        }
    }

    /// A glyph's outline as a path in thousandths of an em, written as the
    /// fixtures write theirs, so the two can be read side by side.
    fn outline_path(face: &Sfnt<'_>, glyph: u16) -> String {
        use tinker_pdf_font::Segment;
        let Some(outline) = tinker_pdf_font::glyf::outline(face, glyph) else {
            return "no outline".to_string();
        };
        let upem = face.units_per_em;
        #[allow(clippy::cast_possible_truncation)]
        let point = |x: f64, y: f64| -> String {
            format!(
                "{},{}",
                scale(x.round() as i32, upem),
                scale(y.round() as i32, upem)
            )
        };
        outline
            .segments
            .iter()
            .map(|segment| match *segment {
                Segment::MoveTo { x, y } => format!("M{}", point(x, y)),
                Segment::LineTo { x, y } => format!("L{}", point(x, y)),
                Segment::QuadTo { cx, cy, x, y } => format!("Q{} {}", point(cx, cy), point(x, y)),
                Segment::CurveTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => format!("C{} {} {}", point(c1x, c1y), point(c2x, c2y), point(x, y)),
                Segment::Close => "Z".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The `<use>` elements of a case's SVG, the empty-outline ones dropped
    /// as the suite drops them.
    fn uses(body: &str) -> Vec<(String, i32, i32)> {
        let mut empty: Vec<String> = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find("<symbol ") {
            rest = &rest[at..];
            let end = rest.find("</symbol>").unwrap_or(rest.len());
            let symbol = &rest[..end];
            let id = attribute(symbol, "id").expect("a symbol has an id");
            let outline = symbol
                .find("<path ")
                .and_then(|at| attribute(&symbol[at..], "d"))
                .unwrap_or_default();
            if outline.trim().is_empty() {
                empty.push(id);
            }
            rest = &rest[end..];
            if let Some(after) = rest.strip_prefix("</symbol>") {
                rest = after;
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
                    .map_or(id.clone(), |(_, name)| name.to_string());
                let x: i32 = attribute(tag, "x")
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                let y: i32 = attribute(tag, "y")
                    .and_then(|y| y.parse().ok())
                    .unwrap_or(0);
                out.push((name, x, y));
            }
            rest = &rest[end..];
        }
        out
    }

    /// A fixture's glyph name as the face's index: `gid<N>`, the face's own
    /// `post` name, or the character the name spells through its `cmap`.
    fn glyph_named(face: &Sfnt<'_>, name: &str) -> Option<u16> {
        if let Some(index) = name.strip_prefix("gid").and_then(|n| n.parse().ok()) {
            return Some(index);
        }
        face.glyph_for_name(name)
            .or_else(|| glyph_name_to_char(name).and_then(|c| face.glyph_for_char(c)))
    }

    fn has_outline(face: &Sfnt<'_>, glyph: u16) -> bool {
        if let Some(cff) = face
            .table(u32::from_be_bytes(*b"CFF "))
            .and_then(Cff::parse)
        {
            return cff
                .outline(glyph)
                .is_some_and(|outline| !outline.is_empty());
        }
        tinker_pdf_font::glyf::outline(face, glyph).is_some_and(|outline| !outline.is_empty())
    }

    fn scale(units: i32, upem: u16) -> i32 {
        let upem = i32::from(upem);
        let magnitude = units.abs().saturating_mul(PPEM).saturating_add(upem / 2) / upem;
        if units < 0 {
            -magnitude
        } else {
            magnitude
        }
    }

    /// What one run recorded.
    struct Recorded {
        script: Script,
        notes: Vec<String>,
        stages: Vec<Snapshot>,
        ran: Vec<Ran>,
    }

    /// Shapes `text` as the suite's `shaped` does, recording every run.
    fn shape_recorded(face: &Sfnt<'_>, text: &str) -> (Vec<(u16, i32, i32)>, Vec<Recorded>) {
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        let shaper = Shaper::new(face);
        let runs = itemize(text, &paragraph);
        let mut recorded = Vec::new();
        let mut shaped = Vec::new();
        for run in &runs {
            MODE.with(|mode| {
                *mode.borrow_mut() = Some(Mode::Recording {
                    notes: Vec::new(),
                    stages: Vec::new(),
                    ran: Vec::new(),
                });
            });
            shaped.push(shaper.shape(text, run));
            let taken = MODE.with(|mode| mode.borrow_mut().take());
            if let Some(Mode::Recording {
                mut notes,
                stages,
                ran,
            }) = taken
            {
                notes.insert(
                    0,
                    format!(
                        "run {:?} script {:?} level {:?}",
                        run.text, run.script, run.level
                    ),
                );
                recorded.push(Recorded {
                    script: run.script,
                    notes,
                    stages,
                    ran,
                });
            }
        }
        let levels: Vec<_> = runs.iter().map(|run| run.level).collect();
        let mut out = Vec::new();
        let mut pen = 0i32;
        for index in crate::bidi::reorder(&levels) {
            let Some(run) = shaped.get(index) else {
                continue;
            };
            let glyphs: Vec<_> = if run.direction().is_forward() {
                run.glyphs().to_vec()
            } else {
                run.glyphs().iter().rev().copied().collect()
            };
            for glyph in glyphs {
                if has_outline(face, glyph.glyph) {
                    out.push((
                        glyph.glyph,
                        scale(pen.saturating_add(glyph.x_offset), face.units_per_em),
                        scale(glyph.y_offset, face.units_per_em),
                    ));
                }
                pen = pen.saturating_add(glyph.x_advance);
            }
        }
        (out, recorded)
    }

    /// One glyph of a buffer, with what the cluster model carries on it.
    fn describe_glyph(buffer: &Buffer, at: usize) -> String {
        let glyph = buffer.glyph(at).copied().unwrap_or_default();
        let props = buffer.props(at);
        let mut out = format!(
            "{}@c{} s{} {:?}",
            glyph.glyph, glyph.cluster, props.syllable, props.category
        );
        if props.mask != Buffer::GLOBAL {
            let _ = write!(out, " m{:#x}", props.mask);
        }
        if props.repha {
            out.push_str(" reph");
        }
        if props.ignorable {
            out.push_str(" ign");
        }
        out
    }

    fn describe(buffer: &Buffer) -> String {
        (0..buffer.len())
            .map(|at| describe_glyph(buffer, at))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn positions(buffer: &Buffer) -> String {
        buffer
            .glyphs()
            .iter()
            .map(|g| {
                format!(
                    "{}(adv {},{} off {},{})",
                    g.glyph, g.x_advance, g.y_advance, g.x_offset, g.y_offset
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The script tag `Shaper::shape` asks a table for: the first of the
    /// script's registry tags the table declares, then the default rule's,
    /// then `DFLT`.
    fn script_tag(table: &LayoutTable<'_>, script: Script) -> Tag {
        let scripts = table.scripts();
        for tag in script.opentype_tags() {
            if scripts.find(tag).is_some() {
                return tag;
            }
        }
        let default = script.opentype_tag();
        if scripts.find(default).is_some() {
            return default;
        }
        Tag::DEFAULT_SCRIPT
    }

    /// Each lookup the script's default language system names, with the
    /// features that name it; the required feature is named `(required)`.
    fn features_of(table: &LayoutTable<'_>, script: Tag) -> BTreeMap<u16, Vec<String>> {
        let mut out: BTreeMap<u16, Vec<String>> = BTreeMap::new();
        let scripts = table.scripts();
        let Some(found) = scripts
            .find(script)
            .or_else(|| scripts.find(Tag::DEFAULT_SCRIPT))
        else {
            return out;
        };
        let Some(lang) = found.lang_sys(None) else {
            return out;
        };
        let list = table.features();
        let mut indices: Vec<(u16, String)> = Vec::new();
        if let Some(required) = lang.required_feature() {
            let tag = list
                .tag(usize::from(required))
                .map_or_else(|| "?".to_string(), |t| t.to_string());
            indices.push((required, format!("{tag}(required)")));
        }
        for n in 0..lang.len() {
            if let Some(index) = lang.feature(n) {
                let tag = list
                    .tag(usize::from(index))
                    .map_or_else(|| "?".to_string(), |t| t.to_string());
                indices.push((index, tag));
            }
        }
        for (index, tag) in indices {
            let Some(feature) = list.get(usize::from(index)) else {
                continue;
            };
            for n in 0..feature.len() {
                if let Some(lookup) = feature.lookup(n) {
                    let entry = out.entry(lookup).or_default();
                    if !entry.contains(&tag) {
                        entry.push(tag.clone());
                    }
                }
            }
        }
        out
    }

    /// A `GSUB` subtable with its extension indirections followed.
    fn resolved<'a>(lookup: &Lookup<'a>, sub: usize) -> Option<(u16, Bytes<'a>)> {
        resolved_in(lookup, sub, 7)
    }

    /// Every glyph a coverage table holds, in coverage-index order.
    fn covered(coverage: Coverage<'_>) -> Vec<u16> {
        let mut ranges = Vec::new();
        let mut budget = usize::MAX;
        let _ = coverage.ranges(&mut ranges, &mut budget);
        let mut glyphs: Vec<(u16, u16)> = Vec::new();
        for (first, last) in ranges {
            for glyph in first..=last {
                if let Some(index) = coverage.index_of(glyph) {
                    glyphs.push((index, glyph));
                }
            }
        }
        glyphs.sort_unstable();
        glyphs.dedup();
        glyphs.into_iter().map(|(_, glyph)| glyph).collect()
    }

    /// What a substituting subtable turns into what: `(input, output)` pairs,
    /// the input a ligature's whole component list.
    fn edges(kind: u16, data: Bytes<'_>) -> Vec<(Vec<u16>, Vec<u16>)> {
        let mut out = Vec::new();
        let Some(coverage) = data.offset16(2).map(Coverage::new) else {
            return out;
        };
        let glyphs = covered(coverage);
        match (kind, data.u16(0)) {
            (1, Some(1)) => {
                let delta = data.i16(4).unwrap_or(0);
                for glyph in glyphs {
                    #[allow(clippy::cast_sign_loss)]
                    out.push((vec![glyph], vec![glyph.wrapping_add(delta as u16)]));
                }
            }
            (1, Some(2)) | (8, Some(1)) => {
                // Type 1 format 2 keeps its substitutes at 6; type 8 after its
                // backtrack and lookahead arrays.
                let at = if kind == 1 {
                    Some(4usize)
                } else {
                    (|| {
                        let backtrack = usize::from(data.u16(4)?);
                        let lookahead_at = 6 + backtrack * 2;
                        let lookahead = usize::from(data.u16(lookahead_at)?);
                        Some(lookahead_at + 2 + lookahead * 2)
                    })()
                };
                let Some(at) = at else {
                    return out;
                };
                for (index, glyph) in glyphs.iter().enumerate() {
                    if let Some(to) = data.u16_at(at + 2, index) {
                        out.push((vec![*glyph], vec![to]));
                    }
                }
            }
            (2 | 3, Some(1)) => {
                for (index, glyph) in glyphs.iter().enumerate() {
                    let Some(set) = data.offset16_at(6, index) else {
                        continue;
                    };
                    // A sequence is one output; an alternate set is several,
                    // each a possible one, and all of them are kept, so that
                    // what can be derived is never understated.
                    let count = usize::from(set.u16(0).unwrap_or(0));
                    let to: Vec<u16> = (0..count).filter_map(|n| set.u16_at(2, n)).collect();
                    out.push((vec![*glyph], to));
                }
            }
            (4, Some(1)) => {
                for (index, glyph) in glyphs.iter().enumerate() {
                    let Some(set) = data.offset16_at(6, index) else {
                        continue;
                    };
                    for n in 0..usize::from(set.u16(0).unwrap_or(0)) {
                        let Some(entry) = set.offset16_at(2, n) else {
                            continue;
                        };
                        let Some(ligature) = entry.u16(0) else {
                            continue;
                        };
                        let components = usize::from(entry.u16(2).unwrap_or(0));
                        let mut input = vec![*glyph];
                        input.extend((1..components).filter_map(|c| entry.u16_at(4, c - 1)));
                        out.push((input, vec![ligature]));
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// The lookups a contextual subtable's rules name.
    fn nested(kind: u16, data: Bytes<'_>) -> Vec<u16> {
        let mut out = Vec::new();
        let records = |rule: Bytes<'_>, at: usize, count: usize, out: &mut Vec<u16>| {
            for n in 0..count {
                if let Some(lookup) = rule.u16(at + n * 4 + 2) {
                    out.push(lookup);
                }
            }
        };
        let plain_rule = |rule: Bytes<'_>, out: &mut Vec<u16>| {
            let glyphs = usize::from(rule.u16(0).unwrap_or(0));
            let count = usize::from(rule.u16(2).unwrap_or(0));
            records(rule, 4 + glyphs.saturating_sub(1) * 2, count, out);
        };
        let chain_rule = |rule: Bytes<'_>, out: &mut Vec<u16>| {
            let backtrack = usize::from(rule.u16(0).unwrap_or(0));
            let input_at = 2 + backtrack * 2;
            let input = usize::from(rule.u16(input_at).unwrap_or(0));
            let lookahead_at = input_at + 2 + input.saturating_sub(1) * 2;
            let lookahead = usize::from(rule.u16(lookahead_at).unwrap_or(0));
            let count_at = lookahead_at + 2 + lookahead * 2;
            let count = usize::from(rule.u16(count_at).unwrap_or(0));
            records(rule, count_at + 2, count, out);
        };
        let sets = |data: Bytes<'_>, at: usize, chained: bool, out: &mut Vec<u16>| {
            for s in 0..usize::from(data.u16(at).unwrap_or(0)) {
                let Some(set) = data.offset16_at(at + 2, s) else {
                    continue;
                };
                for r in 0..usize::from(set.u16(0).unwrap_or(0)) {
                    if let Some(rule) = set.offset16_at(2, r) {
                        if chained {
                            chain_rule(rule, out);
                        } else {
                            plain_rule(rule, out);
                        }
                    }
                }
            }
        };
        match (kind, data.u16(0)) {
            (5, Some(1)) => sets(data, 4, false, &mut out),
            (5, Some(2)) => sets(data, 6, false, &mut out),
            (5, Some(3)) => {
                let glyphs = usize::from(data.u16(2).unwrap_or(0));
                let count = usize::from(data.u16(4).unwrap_or(0));
                records(data, 6 + glyphs * 2, count, &mut out);
            }
            (6, Some(1)) => sets(data, 4, true, &mut out),
            (6, Some(2)) => sets(data, 10, true, &mut out),
            (6, Some(3)) => {
                let backtrack = usize::from(data.u16(2).unwrap_or(0));
                let input_at = 4 + backtrack * 2;
                let input = usize::from(data.u16(input_at).unwrap_or(0));
                let lookahead_at = input_at + 2 + input * 2;
                let lookahead = usize::from(data.u16(lookahead_at).unwrap_or(0));
                let count_at = lookahead_at + 2 + lookahead * 2;
                let count = usize::from(data.u16(count_at).unwrap_or(0));
                records(data, count_at + 2, count, &mut out);
            }
            _ => {}
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Every lookup of a table: its resolved type, the substitutions it
    /// makes directly, and the lookups its rules name.
    struct Catalogue {
        kinds: BTreeMap<u16, u16>,
        edges: BTreeMap<u16, Vec<(Vec<u16>, Vec<u16>)>>,
        calls: BTreeMap<u16, Vec<u16>>,
    }

    fn catalogue(table: &LayoutTable<'_>) -> Catalogue {
        let lookups = table.lookups();
        let mut out = Catalogue {
            kinds: BTreeMap::new(),
            edges: BTreeMap::new(),
            calls: BTreeMap::new(),
        };
        for index in 0..lookups.len() {
            let Some(lookup) = lookups.get(index) else {
                continue;
            };
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            for sub in 0..lookup.len() {
                let Some((kind, data)) = resolved(&lookup, sub) else {
                    continue;
                };
                out.kinds.insert(index, kind);
                out.edges
                    .entry(index)
                    .or_default()
                    .extend(edges(kind, data));
                out.calls
                    .entry(index)
                    .or_default()
                    .extend(nested(kind, data));
            }
        }
        out
    }

    impl Catalogue {
        /// Whether `lookup` can put `glyph` in the buffer, itself or through a
        /// lookup its rules name.
        fn emits(&self, lookup: u16, glyph: u16, seen: &mut BTreeSet<u16>) -> bool {
            if !seen.insert(lookup) {
                return false;
            }
            let direct = self
                .edges
                .get(&lookup)
                .is_some_and(|edges| edges.iter().any(|(_, to)| to.contains(&glyph)));
            direct
                || self.calls.get(&lookup).is_some_and(|calls| {
                    calls.iter().any(|called| self.emits(*called, glyph, seen))
                })
        }

        /// Every lookup reachable from `roots` through the lookups their
        /// rules name.
        fn reachable(&self, roots: &[u16]) -> BTreeSet<u16> {
            let mut out: BTreeSet<u16> = BTreeSet::new();
            let mut todo: Vec<u16> = roots.to_vec();
            while let Some(lookup) = todo.pop() {
                if out.insert(lookup) {
                    if let Some(calls) = self.calls.get(&lookup) {
                        todo.extend(calls.iter().copied());
                    }
                }
            }
            out
        }

        /// Every glyph some sequence of the lookups in `within` can put in a
        /// buffer that starts as `start`, contexts set aside: a substitution
        /// is taken once every glyph of its input is derivable, a ligature's
        /// components included. Each glyph maps to the substitution that
        /// first derived it, or `None` for one `start` holds.
        ///
        /// Setting the contexts aside makes this an over-approximation, so a
        /// glyph it does not reach is one no sequence of those lookups can
        /// produce from that text, whatever the order or the context.
        fn derivable(
            &self,
            within: &BTreeSet<u16>,
            start: &BTreeSet<u16>,
        ) -> BTreeMap<u16, Option<(u16, Vec<u16>)>> {
            let mut have: BTreeMap<u16, Option<(u16, Vec<u16>)>> =
                start.iter().map(|glyph| (*glyph, None)).collect();
            loop {
                let mut changed = false;
                for lookup in within {
                    for (input, output) in self.edges.get(lookup).into_iter().flatten() {
                        if input.is_empty() || !input.iter().all(|g| have.contains_key(g)) {
                            continue;
                        }
                        for glyph in output {
                            if !have.contains_key(glyph) {
                                have.insert(*glyph, Some((*lookup, input.clone())));
                                changed = true;
                            }
                        }
                    }
                }
                if !changed {
                    return have;
                }
            }
        }
    }

    /// How [`Catalogue::derivable`] reached `glyph`, as the substitutions
    /// back to the text's own glyphs.
    fn derivation(
        have: &BTreeMap<u16, Option<(u16, Vec<u16>)>>,
        glyph: u16,
        depth: usize,
        out: &mut Vec<String>,
    ) {
        if depth > 8 {
            return;
        }
        match have.get(&glyph) {
            Some(Some((lookup, input))) => {
                out.push(format!(
                    "{glyph} <- lookup {lookup} from {}",
                    input
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("+")
                ));
                for from in input {
                    derivation(have, *from, depth + 1, out);
                }
            }
            Some(None) => out.push(format!("{glyph} is the text's")),
            None => {}
        }
    }

    /// One lookup run again over `buffer`, under `mask`, each position it
    /// was tried at with where its rules stopped.
    fn replay(
        layout: &Layout<'_>,
        table: Table,
        lookup: u16,
        mask: u32,
        buffer: &Buffer,
    ) -> (Vec<Attempt>, Vec<(usize, &'static str)>, Buffer) {
        let mut copy = buffer.clone();
        let Some(list) = (match table {
            Table::Gsub => layout.gsub(),
            Table::Gpos => layout.gpos(),
        }) else {
            return (Vec::new(), Vec::new(), copy);
        };
        MODE.with(|mode| {
            *mode.borrow_mut() = Some(Mode::Replaying {
                attempts: Vec::new(),
                pending: Vec::new(),
                nested: Vec::new(),
                passed: Vec::new(),
            });
        });
        let mut runner = Runner::new(
            table,
            list.lookups(),
            layout.gdef(),
            Limits::for_glyphs(buffer.len()),
        );
        runner.run(&mut copy, &[(lookup, mask)]);
        let taken = MODE.with(|mode| mode.borrow_mut().take());
        match taken {
            Some(Mode::Replaying {
                attempts, passed, ..
            }) => (attempts, passed, copy),
            _ => (Vec::new(), Vec::new(), copy),
        }
    }

    fn miss_text(miss: &Miss, names: &dyn Fn(u16) -> String) -> String {
        let part = match miss.part {
            Part::Input => "input",
            Part::Backtrack => "backtrack",
            Part::Lookahead => "lookahead",
        };
        let place = match (miss.at, miss.glyph) {
            (Some(at), Some(glyph)) => format!(" at {at} ({})", names(glyph)),
            (Some(at), None) => format!(" at {at}"),
            _ => String::new(),
        };
        let cause = match miss.cause {
            Cause::Glyph => "not the glyph the rule names",
            Cause::Mask => "the feature's mask does not reach it",
            Cause::Syllable => "in another cluster",
            Cause::End => "nothing left to see",
            Cause::TooLong => "rule longer than the context limit",
        };
        format!("{part} {}{place}: {cause}", miss.step)
    }

    /// Everything the trace says about one run's missing glyph: each lookup
    /// that can emit it, the features naming it, and where it stopped each
    /// time it ran.
    #[allow(clippy::too_many_arguments)]
    fn explain_missing(
        out: &mut String,
        layout: &Layout<'_>,
        recorded: &Recorded,
        script: Tag,
        glyph: u16,
        catalogue: &Catalogue,
        features: &BTreeMap<u16, Vec<String>>,
        names: &dyn Fn(u16) -> String,
    ) {
        let _ = writeln!(
            out,
            "  missing {}: the lookups that can emit it",
            names(glyph)
        );
        let mut any = false;
        for lookup in catalogue.kinds.keys() {
            if !catalogue.emits(*lookup, glyph, &mut BTreeSet::new()) {
                continue;
            }
            any = true;
            let kind = catalogue.kinds.get(lookup).copied().unwrap_or(0);
            let named = features
                .get(lookup)
                .map_or_else(|| "no feature of this script".to_string(), |f| f.join(" "));
            let direct: Vec<String> = catalogue
                .edges
                .get(lookup)
                .into_iter()
                .flatten()
                .filter(|(_, to)| to.contains(&glyph))
                .map(|(from, to)| {
                    format!(
                        "{} -> {}",
                        from.iter().map(|g| names(*g)).collect::<Vec<_>>().join("+"),
                        to.iter().map(|g| names(*g)).collect::<Vec<_>>().join("+")
                    )
                })
                .collect();
            let calls: Vec<String> = catalogue
                .calls
                .get(lookup)
                .into_iter()
                .flatten()
                .filter(|called| catalogue.emits(**called, glyph, &mut BTreeSet::new()))
                .map(|called| called.to_string())
                .collect();
            let _ = writeln!(
                out,
                "    lookup {lookup} type {kind} [{named}] in script {script}{}{}",
                if direct.is_empty() {
                    String::new()
                } else {
                    format!(": {}", direct.join(", "))
                },
                if calls.is_empty() {
                    String::new()
                } else {
                    format!(" (calls {})", calls.join(" "))
                }
            );
            let runs: Vec<&Ran> = recorded
                .ran
                .iter()
                .filter(|ran| ran.table == Table::Gsub && ran.lookup == *lookup)
                .collect();
            let tries: Vec<(String, u32, &Buffer)> = if runs.is_empty() {
                let last = recorded
                    .stages
                    .iter()
                    .rev()
                    .find(|stage| stage.label.starts_with("GSUB"))
                    .map(|stage| &stage.buffer);
                last.map(|buffer| {
                    (
                        "never ran in this run; tried over the buffer after GSUB".to_string(),
                        Buffer::GLOBAL,
                        buffer,
                    )
                })
                .into_iter()
                .collect()
            } else {
                runs.iter()
                    .map(|ran| {
                        let stage = recorded
                            .stages
                            .get(ran.stage)
                            .map_or_else(|| "?".to_string(), |stage| stage.label.clone());
                        (
                            format!("ran in [{stage}] under mask {:#x}", ran.mask),
                            ran.mask,
                            &ran.before,
                        )
                    })
                    .collect()
            };
            for (said, mask, buffer) in tries {
                let _ = writeln!(out, "      {said}; the buffer it saw: {}", describe(buffer));
                let (attempts, passed, after) = replay(layout, Table::Gsub, *lookup, mask, buffer);
                if !passed.is_empty() {
                    let _ = writeln!(
                        out,
                        "        stepped over: {}",
                        passed
                            .iter()
                            .map(|(at, why)| format!("{at} ({why})"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                let mut quiet: Vec<usize> = Vec::new();
                for attempt in &attempts {
                    if attempt.applied {
                        let _ = writeln!(
                            out,
                            "        at {} ({}, cluster {}): applied",
                            attempt.at,
                            names(attempt.glyph),
                            attempt.syllable
                        );
                    } else if attempt.misses.is_empty() && attempt.nested.is_empty() {
                        quiet.push(attempt.at);
                    } else {
                        // One line per distinct stop, with how many of the
                        // subtable's rules stopped there.
                        let mut said: Vec<String> = Vec::new();
                        let mut counts: Vec<usize> = Vec::new();
                        for text in attempt.misses.iter().map(|m| miss_text(m, names)).chain(
                            attempt.nested.iter().map(|(called, at)| {
                                format!("the rule matched; lookup {called} did not apply at {at}")
                            }),
                        ) {
                            if let Some(at) = said.iter().position(|s| *s == text) {
                                if let Some(count) = counts.get_mut(at) {
                                    *count += 1;
                                }
                            } else {
                                said.push(text);
                                counts.push(1);
                            }
                        }
                        let said: Vec<String> = said
                            .into_iter()
                            .zip(counts)
                            .map(|(text, count)| {
                                if count > 1 {
                                    format!("{text} (x{count})")
                                } else {
                                    text
                                }
                            })
                            .collect();
                        let _ = writeln!(
                            out,
                            "        at {} ({}, cluster {}): {}",
                            attempt.at,
                            names(attempt.glyph),
                            attempt.syllable,
                            said.join("; ")
                        );
                    }
                }
                if !quiet.is_empty() {
                    let _ = writeln!(
                        out,
                        "        no subtable covers the glyph at {}",
                        quiet
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                let _ = writeln!(out, "        after it: {}", describe(&after));
            }
        }
        if !any {
            let _ = writeln!(out, "    no lookup of the face emits it");
        }
    }

    /// An anchor table's point.
    fn anchor(data: Option<Bytes<'_>>) -> String {
        match data.and_then(|a| Some((a.i16(2)?, a.i16(4)?))) {
            Some((x, y)) => format!("({x},{y})"),
            None => "none".to_string(),
        }
    }

    /// What each `GPOS` lookup that covers the glyph at `at` would make of
    /// it: a mark lookup's class and anchor for the glyph, and the anchor of
    /// the glyph it would attach to.
    fn positioning_of(
        out: &mut String,
        layout: &Layout<'_>,
        buffer: &Buffer,
        at: usize,
        features: &BTreeMap<u16, Vec<String>>,
        names: &dyn Fn(u16) -> String,
    ) {
        let Some(gpos) = layout.gpos() else {
            return;
        };
        let Some(glyph) = buffer.glyph_id(at) else {
            return;
        };
        let class_of = |g: u16| layout.gdef().map(|gdef| gdef.glyph_class(g));
        // The nearest glyph before `at` that is not a mark, and the nearest
        // glyph before it at all.
        let base = (0..at)
            .rev()
            .find(|b| {
                buffer
                    .glyph_id(*b)
                    .is_some_and(|g| class_of(g) != Some(crate::gdef::GlyphClass::Mark))
            })
            .and_then(|b| buffer.glyph_id(b));
        let previous = at.checked_sub(1).and_then(|p| buffer.glyph_id(p));
        let lookups = gpos.lookups();
        for index in 0..lookups.len() {
            let Some(lookup) = lookups.get(index) else {
                continue;
            };
            for sub in 0..lookup.len() {
                let Some((kind, data)) = resolved_in(&lookup, sub, 9) else {
                    continue;
                };
                let Some(first) = data.offset16(2).map(Coverage::new) else {
                    continue;
                };
                let Some(mark_index) = first.index_of(glyph) else {
                    continue;
                };
                let named = u16::try_from(index)
                    .ok()
                    .and_then(|i| features.get(&i))
                    .map_or_else(|| "-".to_string(), |f| f.join(" "));
                let mut said = format!(
                    "    GPOS lookup {index} type {kind} [{named}] flags {:#06x}{} covers it",
                    lookup.flags(),
                    lookup
                        .mark_filtering_set()
                        .map_or_else(String::new, |set| format!(" mark set {set}"))
                );
                if kind == 6 {
                    // Every glyph back to the base, as this lookup sees it.
                    let set = lookup
                        .mark_filtering_set()
                        .and_then(|set| layout.gdef()?.mark_glyph_set(usize::from(set)));
                    let mark2 = data.offset16(4).map(Coverage::new);
                    let mut seen: Vec<String> = Vec::new();
                    for back in (0..at).rev() {
                        let Some(g) = buffer.glyph_id(back) else {
                            continue;
                        };
                        let class = class_of(g);
                        let filtered = class == Some(crate::gdef::GlyphClass::Mark)
                            && set.is_some_and(|set| !set.covers(g));
                        seen.push(format!(
                            "{back}:{}{}{}",
                            names(g),
                            if filtered {
                                " (outside its mark set)"
                            } else {
                                ""
                            },
                            if mark2.is_some_and(|c| c.covers(g)) {
                                " (in mark2)"
                            } else {
                                ""
                            }
                        ));
                        if class != Some(crate::gdef::GlyphClass::Mark) {
                            break;
                        }
                    }
                    let _ = write!(said, "; back to the base: {}", seen.join(", "));
                }
                if matches!(kind, 4 | 6) {
                    let marks = data.offset16(8);
                    let record = |array: Option<Bytes<'_>>, i: usize| -> (u16, String) {
                        let class = array.and_then(|m| m.u16(2 + i * 4)).unwrap_or(0);
                        let point = anchor(array.and_then(|m| m.offset16(2 + i * 4 + 2)));
                        (class, point)
                    };
                    let (class, point) = record(marks, usize::from(mark_index));
                    let _ = write!(said, ": mark class {class}, its anchor {point}");
                    let classes = usize::from(data.u16(6).unwrap_or(0));
                    let target = if kind == 4 { base } else { previous };
                    if let Some(target) = target {
                        let covered = data.offset16(4).map(Coverage::new);
                        match covered.and_then(|c| c.index_of(target)) {
                            Some(t) => {
                                let array = data.offset16(10);
                                let at = 2 + (usize::from(t) * classes + usize::from(class)) * 2;
                                let point = anchor(array.and_then(|a| a.offset16(at)));
                                let _ = write!(
                                    said,
                                    "; {} {} anchor for the class {point}",
                                    if kind == 4 { "base" } else { "previous mark" },
                                    names(target)
                                );
                            }
                            None => {
                                let _ = write!(
                                    said,
                                    "; {} {} is not in its {} coverage",
                                    if kind == 4 { "base" } else { "previous mark" },
                                    names(target),
                                    if kind == 4 { "base" } else { "mark2" }
                                );
                            }
                        }
                    }
                }
                let _ = writeln!(out, "{said}");
            }
        }
        let _ = writeln!(
            out,
            "    GDEF: the glyph {:?}, the base before it {}, the glyph before it {}",
            class_of(glyph),
            base.map_or_else(
                || "none".to_string(),
                |b| format!("{} {:?}", names(b), class_of(b))
            ),
            previous.map_or_else(
                || "none".to_string(),
                |p| format!("{} {:?}", names(p), class_of(p))
            )
        );
    }

    /// Every way the face's mark lookups could attach the glyph at `at` to a
    /// glyph before it — each lookup of type 4, 5 or 6 whose mark coverage
    /// holds it, and each earlier glyph its other coverage holds — with the
    /// position each would draw it at, in thousandths of an em, beside the
    /// fixture's; and the position it is drawn at unattached.
    fn attachments_that_fit(
        out: &mut String,
        face: &Sfnt<'_>,
        layout: &Layout<'_>,
        buffer: &Buffer,
        at: usize,
        wanted: (i32, i32),
        names: &dyn Fn(u16) -> String,
    ) {
        let Some(gpos) = layout.gpos() else {
            return;
        };
        let Some(glyph) = buffer.glyph_id(at) else {
            return;
        };
        let upem = face.units_per_em;
        let pen = |to: usize| -> i32 {
            (0..to)
                .filter_map(|k| buffer.glyph(k))
                .map(|g| g.x_advance)
                .fold(0i32, i32::saturating_add)
        };
        let draw = |x: i32, y: i32| (scale(pen(at).saturating_add(x), upem), scale(y, upem));
        let fits = |p: (i32, i32)| (p.0 - wanted.0).abs() <= 1 && (p.1 - wanted.1).abs() <= 1;
        let unattached = draw(0, 0);
        let _ = writeln!(
            out,
            "    unattached it would be drawn at {unattached:?}{}",
            if fits(unattached) {
                ", which is the fixture's"
            } else {
                ""
            }
        );
        let lookups = gpos.lookups();
        let mut any = false;
        for index in 0..lookups.len() {
            let Some(lookup) = lookups.get(index) else {
                continue;
            };
            for sub in 0..lookup.len() {
                let Some((kind, data)) = resolved_in(&lookup, sub, 9) else {
                    continue;
                };
                if !matches!(kind, 4 | 6) {
                    continue;
                }
                let Some(mark_index) = data
                    .offset16(2)
                    .map(Coverage::new)
                    .and_then(|c| c.index_of(glyph))
                else {
                    continue;
                };
                let marks = data.offset16(8);
                let class = usize::from(
                    marks
                        .and_then(|m| m.u16(2 + usize::from(mark_index) * 4))
                        .unwrap_or(0),
                );
                let Some(mark_anchor) = marks
                    .and_then(|m| m.offset16(2 + usize::from(mark_index) * 4 + 2))
                    .and_then(|a| Some((i32::from(a.i16(2)?), i32::from(a.i16(4)?))))
                else {
                    continue;
                };
                let classes = usize::from(data.u16(6).unwrap_or(0));
                let others = data.offset16(4).map(Coverage::new);
                let array = data.offset16(10);
                for parent in 0..at {
                    let Some(p) = buffer.glyph_id(parent) else {
                        continue;
                    };
                    let Some(t) = others.and_then(|c| c.index_of(p)) else {
                        continue;
                    };
                    let Some(anchor) = array
                        .and_then(|a| a.offset16(2 + (usize::from(t) * classes + class) * 2))
                        .and_then(|a| Some((i32::from(a.i16(2)?), i32::from(a.i16(4)?))))
                    else {
                        continue;
                    };
                    // The parent's own offset as this run left it, and the
                    // pen walked back from the mark to it.
                    let parent_glyph = buffer.glyph(parent).copied().unwrap_or_default();
                    let walk = pen(at) - pen(parent);
                    let x = parent_glyph.x_offset + anchor.0 - mark_anchor.0 - walk;
                    let y = parent_glyph.y_offset + anchor.1 - mark_anchor.1;
                    let drawn = draw(x, y);
                    if fits(drawn) {
                        any = true;
                    }
                    let _ = writeln!(
                        out,
                        "    lookup {index} type {kind} on {parent}:{} would draw it at {drawn:?}{}",
                        names(p),
                        if fits(drawn) { ", which is the fixture's" } else { "" }
                    );
                }
            }
        }
        if !any {
            let _ = writeln!(
                out,
                "    no mark lookup of the face attaches it where the fixture draws it"
            );
        }
    }

    /// A subtable with its extension indirections followed, in a table whose
    /// extension type is `extension`.
    fn resolved_in<'a>(
        lookup: &Lookup<'a>,
        sub: usize,
        extension: u16,
    ) -> Option<(u16, Bytes<'a>)> {
        let mut kind = lookup.kind();
        let mut data = lookup.subtable(sub)?;
        let mut hops = 0;
        while kind == extension && hops < 4 {
            if data.u16(0)? != 1 {
                return None;
            }
            kind = data.u16(2)?;
            data = data.offset32(4)?;
            hops += 1;
        }
        Some((kind, data))
    }

    /// One row, traced.
    #[allow(clippy::too_many_lines)]
    fn trace_row(id: &str) -> String {
        let case = case(id);
        let bytes = std::fs::read(data(&format!("fonts/{}", case.font))).expect("a vendored face");
        let face = Sfnt::parse(&bytes).expect("a fixture face is an sfnt");
        let layout = Layout::parse(&face);
        // A glyph is named by the fixture's name for it, or by the character
        // of the text `cmap` gives it; any other is its index alone.
        let mut known: BTreeMap<u16, String> = BTreeMap::new();
        for (name, _, _) in &case.expected {
            if let Some(glyph) = glyph_named(&face, name) {
                known.entry(glyph).or_insert_with(|| name.clone());
            }
        }
        for c in case.render.chars() {
            if let Some(glyph) = face.glyph_for_char(c) {
                known
                    .entry(glyph)
                    .or_insert_with(|| format!("U+{:04X}", u32::from(c)));
            }
        }
        let names = |glyph: u16| -> String {
            known.get(&glyph).map_or_else(
                || format!("gid{glyph}"),
                |name| format!("gid{glyph}={name}"),
            )
        };
        let mut out = String::new();
        let characters: Vec<String> = case
            .render
            .chars()
            .map(|c| {
                format!(
                    "U+{:04X}({:?}/{:?}/{:?})",
                    u32::from(c),
                    unicode::indic_syllabic(c),
                    unicode::indic_positional(c),
                    unicode::mark_category(c)
                )
            })
            .collect();
        let _ = writeln!(out, "== {id} [{}] {}", case.font, characters.join(" "));
        let _ = writeln!(
            out,
            "  the face's U+25CC: {}",
            face.glyph_for_char('\u{25CC}')
                .map_or_else(|| "none".to_string(), |g| format!("gid{g}"))
        );
        let expected: Vec<(Option<u16>, i32, i32)> = case
            .expected
            .iter()
            .map(|(name, x, y)| (glyph_named(&face, name), *x, *y))
            .collect();
        let (ours, recorded) = shape_recorded(&face, &case.render);
        let show = |list: &[(Option<u16>, i32, i32)]| -> String {
            list.iter()
                .map(|(g, x, y)| format!("{}@{x},{y}", g.map_or_else(|| "?".to_string(), names)))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let ours_listed: Vec<(Option<u16>, i32, i32)> =
            ours.iter().map(|(g, x, y)| (Some(*g), *x, *y)).collect();
        let _ = writeln!(out, "  expected {}", show(&expected));
        let _ = writeln!(out, "  ours     {}", show(&ours_listed));
        for run in &recorded {
            for note in &run.notes {
                let _ = writeln!(out, "  {note}");
            }
            for stage in &run.stages {
                let _ = writeln!(out, "  [{}] {}", stage.label, describe(&stage.buffer));
                if stage.label == "GPOS" {
                    let _ = writeln!(out, "    positions {}", positions(&stage.buffer));
                }
            }
            let gsub_script = layout
                .gsub()
                .map(|gsub| (gsub, script_tag(gsub, run.script)));
            let gpos_script = layout
                .gpos()
                .map(|gpos| (gpos, script_tag(gpos, run.script)));
            let gsub_features = gsub_script
                .map(|(gsub, tag)| features_of(gsub, tag))
                .unwrap_or_default();
            let gpos_features = gpos_script
                .map(|(gpos, tag)| features_of(gpos, tag))
                .unwrap_or_default();
            for ran in &run.ran {
                let changed = ran.before.glyphs() != ran.after.glyphs();
                if !changed {
                    continue;
                }
                let features = match ran.table {
                    Table::Gsub => &gsub_features,
                    Table::Gpos => &gpos_features,
                };
                let named = features
                    .get(&ran.lookup)
                    .map_or_else(|| "-".to_string(), |f| f.join(" "));
                match ran.table {
                    Table::Gsub => {
                        let _ = writeln!(
                            out,
                            "    GSUB lookup {} [{named}]: {} => {}",
                            ran.lookup,
                            ran.before
                                .glyphs()
                                .iter()
                                .map(|g| g.glyph.to_string())
                                .collect::<Vec<_>>()
                                .join(" "),
                            ran.after
                                .glyphs()
                                .iter()
                                .map(|g| g.glyph.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        );
                    }
                    Table::Gpos => {
                        let moved: Vec<String> = ran
                            .before
                            .glyphs()
                            .iter()
                            .zip(ran.after.glyphs())
                            .enumerate()
                            .filter(|(_, (a, b))| a != b)
                            .map(|(at, (a, b))| {
                                format!(
                                    "{at}:{} adv {}->{} off ({},{})->({},{})",
                                    b.glyph,
                                    a.x_advance,
                                    b.x_advance,
                                    a.x_offset,
                                    a.y_offset,
                                    b.x_offset,
                                    b.y_offset
                                )
                            })
                            .collect();
                        let _ = writeln!(
                            out,
                            "    GPOS lookup {} [{named}]: {}",
                            ran.lookup,
                            moved.join("; ")
                        );
                    }
                }
            }
            // The glyphs drawn where the fixture does not draw them, each with
            // what every `GPOS` lookup covering it would make of it.
            if recorded.len() == 1 && ours.len() == expected.len() {
                if let Some(stage) = run.stages.iter().find(|s| s.label == "GPOS") {
                    let visible: Vec<usize> = (0..stage.buffer.len())
                        .filter(|at| {
                            stage
                                .buffer
                                .glyph_id(*at)
                                .is_some_and(|g| has_outline(&face, g))
                        })
                        .collect();
                    for (n, ((glyph, x, y), (want, wx, wy))) in
                        ours.iter().zip(&expected).enumerate()
                    {
                        let moved = (x - wx).abs() > 1 || (y - wy).abs() > 1;
                        if *want != Some(*glyph) || !moved {
                            continue;
                        }
                        let _ = writeln!(
                            out,
                            "  {} drawn at ({x},{y}), the fixture's at ({wx},{wy})",
                            names(*glyph)
                        );
                        if let Some(at) = visible.get(n) {
                            positioning_of(
                                &mut out,
                                &layout,
                                &stage.buffer,
                                *at,
                                &gpos_features,
                                &names,
                            );
                            attachments_that_fit(
                                &mut out,
                                &face,
                                &layout,
                                &stage.buffer,
                                *at,
                                (*wx, *wy),
                                &names,
                            );
                            // Each `GPOS` lookup the run ran, tried again at
                            // this glyph: whether it applied there, and where
                            // its rules stopped.
                            for ran in run.ran.iter().filter(|ran| ran.table == Table::Gpos) {
                                let (attempts, _, _) =
                                    replay(&layout, Table::Gpos, ran.lookup, ran.mask, &ran.before);
                                // A contextual lookup is shown at every
                                // position it was tried at, since its rule
                                // may start before the glyph it moves.
                                let contextual = layout
                                    .gpos()
                                    .and_then(|gpos| gpos.lookups().get(usize::from(ran.lookup)))
                                    .and_then(|l| resolved_in(&l, 0, 9))
                                    .is_some_and(|(kind, _)| matches!(kind, 7 | 8));
                                for attempt in attempts
                                    .iter()
                                    .filter(|a| a.at == *at || (contextual && !a.misses.is_empty()))
                                {
                                    let mut said: Vec<String> = attempt
                                        .misses
                                        .iter()
                                        .map(|m| miss_text(m, &names))
                                        .collect();
                                    said.dedup();
                                    let _ = writeln!(
                                        out,
                                        "    GPOS lookup {} [{}] tried at {} ({}): {}",
                                        ran.lookup,
                                        gpos_features
                                            .get(&ran.lookup)
                                            .map_or_else(|| "-".to_string(), |f| f.join(" ")),
                                        attempt.at,
                                        names(attempt.glyph),
                                        if attempt.applied {
                                            "applied".to_string()
                                        } else if said.is_empty() {
                                            "no subtable takes the glyph".to_string()
                                        } else {
                                            said.join("; ")
                                        }
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // The glyphs the fixture wants and this run does not make.
            let mut wanted: BTreeMap<u16, i64> = BTreeMap::new();
            for (glyph, _, _) in &expected {
                if let Some(glyph) = glyph {
                    *wanted.entry(*glyph).or_insert(0) += 1;
                }
            }
            for (glyph, _, _) in &ours {
                *wanted.entry(*glyph).or_insert(0) -= 1;
            }
            let missing: Vec<u16> = wanted
                .iter()
                .filter(|(_, count)| **count > 0)
                .map(|(glyph, _)| *glyph)
                .collect();
            let extra: Vec<u16> = wanted
                .iter()
                .filter(|(_, count)| **count < 0)
                .map(|(glyph, _)| *glyph)
                .collect();
            if !missing.is_empty() || !extra.is_empty() {
                let _ = writeln!(
                    out,
                    "  lacks [{}], has besides [{}]",
                    missing
                        .iter()
                        .map(|g| names(*g))
                        .collect::<Vec<_>>()
                        .join(" "),
                    extra
                        .iter()
                        .map(|g| names(*g))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            if let Some((gsub, script)) = gsub_script {
                let catalogue = catalogue(gsub);
                for glyph in &missing {
                    explain_missing(
                        &mut out,
                        &layout,
                        run,
                        script,
                        *glyph,
                        &catalogue,
                        &gsub_features,
                        &names,
                    );
                }
                // Whether any sequence of the lookups this script's features
                // reach can put the missing glyph in a buffer that starts as
                // this text's `cmap` glyphs.
                let roots: Vec<u16> = gsub_features.keys().copied().collect();
                let within = catalogue.reachable(&roots);
                let start: BTreeSet<u16> = run
                    .stages
                    .first()
                    .map(|stage| stage.buffer.glyphs().iter().map(|g| g.glyph).collect())
                    .unwrap_or_default();
                let have = catalogue.derivable(&within, &start);
                // And the same over every lookup the face carries, whatever
                // script, language system or feature names it.
                let every: BTreeSet<u16> = catalogue.kinds.keys().copied().collect();
                let anything = catalogue.derivable(&every, &start);
                for glyph in &missing {
                    if anything.contains_key(glyph) {
                        let mut steps = Vec::new();
                        derivation(&anything, *glyph, 0, &mut steps);
                        let _ = writeln!(
                            out,
                            "  {} is derivable from this text through the face's lookups, any script or feature, contexts set aside: {}",
                            names(*glyph),
                            steps.join("; ")
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "  no sequence of any of the face's lookups puts {} in a buffer that starts as this text",
                            names(*glyph)
                        );
                    }
                    if have.contains_key(glyph) {
                        let mut steps = Vec::new();
                        derivation(&have, *glyph, 0, &mut steps);
                        let _ = writeln!(
                            out,
                            "  {} is derivable from this text through the {script} features' lookups, contexts set aside: {}",
                            names(*glyph),
                            steps.join("; ")
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "  no sequence of the {script} features' lookups puts {} in a buffer that starts as this text, in any order or context",
                            names(*glyph)
                        );
                    }
                }
            }
            // The same text with the per-syllable confinement lifted: which
            // lookups it blocks here, and whether the row reproduces without
            // it.
            super::UNCONFINED.with(|flag| flag.set(true));
            let (free, free_recorded) = shape_recorded(&face, &case.render);
            super::UNCONFINED.with(|flag| flag.set(false));
            let agrees = free.len() == expected.len()
                && free
                    .iter()
                    .zip(&expected)
                    .all(|((g, x, y), (want, wx, wy))| {
                        (x - wx).abs() <= 1 && (y - wy).abs() <= 1 && want.is_none_or(|w| w == *g)
                    });
            let changed = |ran: &Ran| ran.before.glyphs() != ran.after.glyphs();
            let blocked: Vec<String> = free_recorded
                .iter()
                .flat_map(|r| r.ran.iter())
                .filter(|ran| ran.table == Table::Gsub && changed(ran))
                .filter(|ran| {
                    !run.ran.iter().any(|mine| {
                        mine.table == Table::Gsub && mine.lookup == ran.lookup && changed(mine)
                    })
                })
                .map(|ran| {
                    format!(
                        "lookup {} [{}]: {} => {}",
                        ran.lookup,
                        gsub_features
                            .get(&ran.lookup)
                            .map_or_else(|| "-".to_string(), |f| f.join(" ")),
                        ran.before
                            .glyphs()
                            .iter()
                            .map(|g| g.glyph.to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                        ran.after
                            .glyphs()
                            .iter()
                            .map(|g| g.glyph.to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                })
                .collect();
            let free_listed: Vec<(Option<u16>, i32, i32)> =
                free.iter().map(|(g, x, y)| (Some(*g), *x, *y)).collect();
            let _ = writeln!(
                out,
                "  with the confinement lifted: {} {}",
                if agrees {
                    "reproduces the fixture;"
                } else {
                    "still differs;"
                },
                show(&free_listed)
            );
            for line in blocked {
                let _ = writeln!(out, "    only unconfined: {line}");
            }
            // The outlines of what is missing and what is drawn instead, the
            // fixture's beside the face's.
            for glyph in missing.iter().chain(&extra) {
                let fixture = case
                    .expected
                    .iter()
                    .find(|(name, _, _)| glyph_named(&face, name) == Some(*glyph))
                    .and_then(|(name, _, _)| case.symbols.get(name).map(|d| (name, d)));
                if let Some((name, path)) = fixture {
                    let _ = writeln!(out, "  outline: the fixture's {name}: {path}");
                }
                let _ = writeln!(
                    out,
                    "  outline: the face's {}: {}",
                    names(*glyph),
                    outline_path(&face, *glyph)
                );
            }
        }
        out
    }

    /// FT-08.1: each `TRIAGE` row the tree gave no cause for, shaped with
    /// every stage recorded, and for each glyph its fixture wants that the
    /// run lacks, every lookup of the face that can emit it and where its
    /// context stops matching. Asserts nothing: the row-to-mechanism table in
    /// `docs/design/shaping.md` is read from what it prints.
    #[test]
    #[ignore = "FT-08.1's trace, which docs/design/shaping.md reads; run by name"]
    fn every_unexplained_triage_row_is_traced() {
        println!("RAN use_trace: {} rows", ROWS.len());
        for id in ROWS {
            print!("{}", trace_row(id));
        }
    }
}
