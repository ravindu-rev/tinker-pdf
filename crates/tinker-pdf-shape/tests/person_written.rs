//! Person-written expectations: the bar a reader of a script's cases are
//! wired into.
//!
//! `docs/design/shaping.md`'s *Person-written expectations* is the
//! specification this file reads: the `.cases` format, what refuses a case,
//! where the trees are, and what is compared. Ruling 13 admits an expected
//! answer only where a person wrote it, so every case under
//! `data/person-written` is H-64's, and nothing here writes one. The one case
//! this file states itself is a transcription of `SHARAN-1/1`, an expectation
//! the people who publish text-rendering-tests wrote, used to show that the
//! comparison catches what it claims to.
//!
//! # What is compared
//!
//! A case is shaped as a consumer shapes a line — [`Shaper::shape_text`] over
//! its text and direction, its settings and language applied, the runs put in
//! drawn order by UAX #9's L2 over their levels and a right-to-left run walked
//! from its end — and each glyph's identity, its cluster where the case states
//! clusters, and where it is drawn, the pen plus its offset on each axis, are
//! held to the case's; so is the pen's end, where the case states it. What is
//! drawn is compared rather than how a glyph's place is split between its
//! advance and its offset, since the split is a shaper's convention and the
//! place is what a reader sees.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf_font::{Cff, Sfnt};
use tinker_pdf_shape::bidi::{self, BaseDirection, Level};
use tinker_pdf_shape::shape::Shaper;
use tinker_pdf_shape::unicode;
use tinker_pdf_shape::{ShapedGlyph, Tag};

/// Per script, by its ISO 15924 code: its cases, and how many of them this
/// crate reproduces.
///
/// A ratchet, as `text_rendering.rs`'s `PASSING` is: the run must find
/// exactly these rows, so a case delivered, lost or newly failing changes this
/// table in the commit that does it, and a count that falls is an edit
/// somebody made and can be asked about. Empty until H-64's first delivery.
const PERSON_WRITTEN: &[(&str, usize, usize)] = &[];

/// The extension a case file carries.
const EXTENSION: &str = "cases";

/// The keywords a file's header holds, each once.
const HEADER: &[&str] = &[
    "format",
    "script",
    "face",
    "author",
    "standing",
    "date",
    "face-source",
    "face-licence",
    "units-per-em",
];

/// The provenance every case carries, whose absence refuses its file.
const PROVENANCE: &[&str] = &["author", "standing", "date", "face-source", "face-licence"];

/// The keywords a case holds.
const CASE: &[&str] = &["case", "text", "direction", "features", "language", "glyph"];

/// The `units-per-em` range, the one `head` allows.
const EM_RANGE: std::ops::RangeInclusive<u32> = 16..=16_384;

/// ISO 15924's codes for the two scripts every script's text may also hold.
const COMMON: [u8; 4] = *b"Zyyy";
const INHERITED: [u8; 4] = *b"Zinh";

/// One glyph as a case names it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GlyphId {
    /// `gid` and a decimal index.
    Index(u16),
    /// A name, as the face's `post` table or `CFF ` charset gives it.
    Name(String),
}

/// One glyph line of a case.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Written {
    id: GlyphId,
    /// The code point index of the first character it stands for, where the
    /// case states clusters.
    cluster: Option<usize>,
    /// How far the pen moves after it; `None` only on the last glyph of a
    /// case that leaves its line's end unstated.
    advance: Option<(i64, i64)>,
    /// Where it is drawn, from the pen.
    offset: (i64, i64),
}

/// One case, as read.
#[derive(Clone, Debug)]
struct Case {
    name: String,
    text: String,
    direction: BaseDirection,
    settings: Vec<(Tag, u32)>,
    language: Option<Tag>,
    glyphs: Vec<Written>,
}

/// One case file, as read: its header and its cases.
#[derive(Clone, Debug)]
struct Delivery {
    script: [u8; 4],
    face: String,
    units_per_em: Option<u32>,
    cases: Vec<Case>,
}

/// A case whose glyph names have been resolved through its face.
struct Resolved {
    name: String,
    case: Case,
    glyphs: Vec<u16>,
}

/// Which one thing is wrong with a case this crate does not reproduce:
/// `text_rendering.rs`'s three verdicts, and the cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// The glyphs drawn are not the glyphs the case names.
    Set,
    /// They are, in another order.
    Order,
    /// They are, in order, and a glyph stands for other characters.
    Cluster,
    /// Only where a glyph is drawn, or where the line ends, differs.
    Position,
}

/// A case that did not reproduce: why, and from which glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mismatch {
    verdict: Verdict,
    /// The first glyph, in drawn order, that differs; the glyph count where
    /// only the line's end does.
    at: usize,
    detail: String,
}

/// One glyph as this crate draws it, in the case's units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drawn {
    glyph: u16,
    /// The code point index of its cluster; `None` for a byte offset no
    /// character starts at.
    cluster: Option<usize>,
    x: i64,
    y: i64,
}

/// The value of one statement line, or `None` for a line with no value.
fn statement(line: &str) -> (&str, Option<&str>) {
    match line.split_once(' ') {
        Some((keyword, value)) => {
            let value = value.trim();
            (keyword, (!value.is_empty()).then_some(value))
        }
        None => (line, None),
    }
}

/// Reads one case file: every refusal it earns, or the delivery.
///
/// A file is refused whole, every problem in it listed, because a delivery
/// with one malformed statement is a delivery to mend rather than one to take
/// in part.
fn read(text: &str) -> Result<Delivery, Vec<String>> {
    let mut refusals: Vec<String> = Vec::new();
    let mut header: BTreeMap<&str, String> = BTreeMap::new();
    let mut cases: Vec<(usize, Vec<(usize, &str, &str)>)> = Vec::new();

    for (index, line) in text.split('\n').enumerate() {
        let number = index + 1;
        if line.ends_with('\r') {
            refusals.push(format!(
                "line {number} ends in a carriage return; the format's lines end in LF"
            ));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, value) = statement(line);
        let Some(value) = value else {
            refusals.push(format!("line {number}: `{keyword}` has no value"));
            continue;
        };
        if HEADER.contains(&keyword) {
            if !cases.is_empty() {
                refusals.push(format!(
                    "line {number}: the header keyword `{keyword}` comes after the first case"
                ));
            } else if header.insert(keyword, value.to_string()).is_some() {
                refusals.push(format!("line {number}: `{keyword}` is given twice"));
            }
        } else if keyword == "case" {
            cases.push((number, vec![(number, keyword, value)]));
        } else if CASE.contains(&keyword) {
            match cases.last_mut() {
                Some((_, statements)) => statements.push((number, keyword, value)),
                None => refusals.push(format!(
                    "line {number}: `{keyword}` stands outside any case"
                )),
            }
        } else {
            refusals.push(format!(
                "line {number}: `{keyword}` is no keyword of the format"
            ));
        }
    }

    for field in PROVENANCE {
        if !header.contains_key(field) {
            refusals.push(format!("the provenance field `{field}` is missing"));
        }
    }
    for field in ["format", "script", "face"] {
        if !header.contains_key(field) {
            refusals.push(format!("the header field `{field}` is missing"));
        }
    }
    if let Some(format) = header.get("format") {
        if format != "1" {
            refusals.push(format!("`format {format}` is not format 1"));
        }
    }
    if let Some(date) = header.get("date") {
        if !is_calendar_day(date) {
            refusals.push(format!(
                "`date {date}` is no calendar day written YYYY-MM-DD"
            ));
        }
    }
    if let Some(licence) = header.get("face-licence") {
        if !is_spdx_identifier(licence) {
            refusals.push(format!(
                "`face-licence {licence}` is no SPDX identifier: letters, digits, `.` and `-`, \
                 and at most a closing `+`"
            ));
        }
    }
    let script = header.get("script").and_then(|code| {
        let code = iso15924(code);
        if code.is_none() {
            refusals.push(format!(
                "`script {}` is no ISO 15924 code: one capital and three small letters",
                header.get("script").map_or("", String::as_str)
            ));
        }
        code
    });
    let units_per_em = header.get("units-per-em").and_then(|value| {
        let units = value
            .parse::<u32>()
            .ok()
            .filter(|units| EM_RANGE.contains(units));
        if units.is_none() {
            refusals.push(format!(
                "`units-per-em {value}` is not a whole number from 16 to 16 384"
            ));
        }
        units
    });

    let mut read_cases: Vec<Case> = Vec::new();
    for (opened, statements) in &cases {
        match read_case(*opened, statements, script) {
            Ok(case) => {
                if read_cases.iter().any(|other| other.name == case.name) {
                    refusals.push(format!(
                        "line {opened}: the case name `{}` is used twice",
                        case.name
                    ));
                }
                read_cases.push(case);
            }
            Err(mut problems) => refusals.append(&mut problems),
        }
    }

    if !refusals.is_empty() {
        return Err(refusals);
    }
    let (Some(script), Some(face)) = (script, header.get("face")) else {
        return Err(vec!["the header names no script or no face".to_string()]);
    };
    Ok(Delivery {
        script,
        face: face.clone(),
        units_per_em,
        cases: read_cases,
    })
}

/// Reads one case's statements.
fn read_case(
    opened: usize,
    statements: &[(usize, &str, &str)],
    script: Option<[u8; 4]>,
) -> Result<Case, Vec<String>> {
    let mut refusals: Vec<String> = Vec::new();
    let mut name: Option<String> = None;
    let mut text: Option<String> = None;
    let mut direction: Option<BaseDirection> = None;
    let mut settings: Option<Vec<(Tag, u32)>> = None;
    let mut language: Option<Tag> = None;
    let mut glyphs: Vec<(usize, &str)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();

    for &(number, keyword, value) in statements {
        if keyword != "glyph" {
            if seen.contains(&keyword) {
                refusals.push(format!(
                    "line {number}: `{keyword}` is given twice in one case"
                ));
                continue;
            }
            seen.push(keyword);
        }
        match keyword {
            "case" => name = Some((*value).to_string()),
            "text" => match code_points(value) {
                Ok(read) => text = Some(read),
                Err(problem) => refusals.push(format!("line {number}: {problem}")),
            },
            "direction" => match value {
                "ltr" => direction = Some(BaseDirection::LeftToRight),
                "rtl" => direction = Some(BaseDirection::RightToLeft),
                "auto" => direction = Some(BaseDirection::Auto),
                other => refusals.push(format!(
                    "line {number}: `direction {other}` is not `ltr`, `rtl` or `auto`"
                )),
            },
            "features" => match feature_settings(value) {
                Ok(read) => settings = Some(read),
                Err(problem) => refusals.push(format!("line {number}: {problem}")),
            },
            "language" => match language_tag(value) {
                Some(tag) => language = Some(tag),
                None => refusals.push(format!(
                    "line {number}: `language {value}` is not one to four letters or digits"
                )),
            },
            _ => glyphs.push((number, value)),
        }
    }

    let name = name.unwrap_or_default();
    if !seen.contains(&"text") {
        refusals.push(format!("line {opened}: the case `{name}` has no `text`"));
    }
    if !seen.contains(&"direction") {
        refusals.push(format!(
            "line {opened}: the case `{name}` has no `direction`"
        ));
    }
    if glyphs.is_empty() {
        refusals.push(format!("line {opened}: the case `{name}` names no glyph"));
    }

    if let (Some(text), Some(script)) = (&text, script) {
        let mut own = false;
        for c in text.chars() {
            let code = unicode::script(c).iso15924();
            if code == script {
                own = true;
            } else if code != COMMON && code != INHERITED {
                refusals.push(format!(
                    "line {opened}: U+{:04X} is in `{}`, not in the file's `{}`",
                    u32::from(c),
                    String::from_utf8_lossy(&code),
                    String::from_utf8_lossy(&script),
                ));
            }
        }
        if !own {
            refusals.push(format!(
                "line {opened}: no character of the case `{name}` is in `{}`",
                String::from_utf8_lossy(&script)
            ));
        }
    }

    let length = text.as_ref().map_or(0, |text| text.chars().count());
    let last = glyphs.len().saturating_sub(1);
    let mut written: Vec<Written> = Vec::new();
    for (place, (number, value)) in glyphs.iter().enumerate() {
        match glyph_line(value, place == last, length) {
            Ok(glyph) => written.push(glyph),
            Err(problem) => refusals.push(format!("line {number}: {problem}")),
        }
    }
    let stated = written
        .iter()
        .filter(|glyph| glyph.cluster.is_some())
        .count();
    if stated != 0 && stated != written.len() {
        refusals.push(format!(
            "line {opened}: the case `{name}` states clusters for {stated} of its {} glyphs",
            written.len()
        ));
    }

    if !refusals.is_empty() {
        return Err(refusals);
    }
    let (Some(text), Some(direction)) = (text, direction) else {
        return Err(vec![format!(
            "line {opened}: the case `{name}` is incomplete"
        )]);
    };
    Ok(Case {
        name,
        text,
        direction,
        settings: settings.unwrap_or_default(),
        language,
        glyphs: written,
    })
}

/// `U+0710 U+0712`: each a Unicode scalar value.
fn code_points(value: &str) -> Result<String, String> {
    let mut out = String::new();
    for token in value.split(' ').filter(|token| !token.is_empty()) {
        let digits = token
            .strip_prefix("U+")
            .ok_or_else(|| format!("`{token}` is no code point written U+ and hex digits"))?;
        if !(4..=6).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "`{token}` is no code point written U+ and four to six hex digits"
            ));
        }
        let scalar = u32::from_str_radix(digits, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| {
                format!("`{token}` is no Unicode scalar value, so no code point of a text")
            })?;
        out.push(scalar);
    }
    if out.is_empty() {
        return Err("the text holds no code point".to_string());
    }
    Ok(out)
}

/// `liga=0 ss01=1`: each tag four letters or digits, each value decimal.
fn feature_settings(value: &str) -> Result<Vec<(Tag, u32)>, String> {
    let mut out = Vec::new();
    for token in value.split(' ').filter(|token| !token.is_empty()) {
        let (tag, setting) = token
            .split_once('=')
            .ok_or_else(|| format!("the feature setting `{token}` is not `tag=value`"))?;
        let tag: [u8; 4] = tag
            .as_bytes()
            .try_into()
            .ok()
            .filter(|tag: &[u8; 4]| tag.iter().all(u8::is_ascii_alphanumeric))
            .ok_or_else(|| format!("the feature tag of `{token}` is not four letters or digits"))?;
        let setting: u32 = setting
            .parse()
            .ok()
            .filter(|_| setting.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| format!("the value of `{token}` is not a decimal number"))?;
        out.push((Tag::new(&tag), setting));
    }
    Ok(out)
}

/// `SYR`: one to four letters or digits, padded with spaces to a tag.
fn language_tag(value: &str) -> Option<Tag> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 4 || !bytes.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    let mut tag = [b' '; 4];
    tag[..bytes.len()].copy_from_slice(bytes);
    Some(Tag::new(&tag))
}

/// `<id> <cluster> <x-advance> <y-advance> <x-offset> <y-offset>`.
fn glyph_line(value: &str, last: bool, length: usize) -> Result<Written, String> {
    let fields: Vec<&str> = value.split(' ').filter(|field| !field.is_empty()).collect();
    let &[id, cluster, x_advance, y_advance, x_offset, y_offset] = fields.as_slice() else {
        return Err(format!(
            "`glyph {value}` is not six fields: id, cluster, two advances, two offsets"
        ));
    };
    let id = match id.strip_prefix("gid") {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            GlyphId::Index(
                digits
                    .parse()
                    .map_err(|_| format!("`{id}` is past the 65 536 glyphs a face can hold"))?,
            )
        }
        _ => GlyphId::Name(id.to_string()),
    };
    let cluster = match cluster {
        "-" => None,
        text => {
            let index: usize = text.parse().map_err(|_| {
                format!("the cluster `{text}` is neither `-` nor a code point index")
            })?;
            if index >= length {
                return Err(format!(
                    "the cluster {index} is past the text's {length} code points"
                ));
            }
            Some(index)
        }
    };
    let number = |field: &str, what: &str| -> Result<i64, String> {
        field
            .parse::<i64>()
            .ok()
            .filter(|n| n.unsigned_abs() <= u64::from(u32::MAX))
            .ok_or_else(|| format!("the {what} `{field}` is not a whole number"))
    };
    let advance = match (x_advance, y_advance) {
        ("-", "-") if last => None,
        ("-", _) | (_, "-") => {
            return Err(
                "an advance is `-` only as both advances of a case's last glyph".to_string(),
            )
        }
        (x, y) => Some((number(x, "x-advance")?, number(y, "y-advance")?)),
    };
    let offset = (number(x_offset, "x-offset")?, number(y_offset, "y-offset")?);
    Ok(Written {
        id,
        cluster,
        advance,
        offset,
    })
}

/// A `YYYY-MM-DD` that names a day of the Gregorian calendar.
fn is_calendar_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = text.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// One SPDX identifier: SPDX's `idstring` characters, letters, digits, `.`
/// and `-`, with the `+` an identifier may close with.
fn is_spdx_identifier(text: &str) -> bool {
    let body = text.strip_suffix('+').unwrap_or(text);
    !body.is_empty()
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// An ISO 15924 code: one capital letter and three small ones.
fn iso15924(text: &str) -> Option<[u8; 4]> {
    let code: [u8; 4] = text.as_bytes().try_into().ok()?;
    (code[0].is_ascii_uppercase() && code[1..].iter().all(u8::is_ascii_lowercase)).then_some(code)
}

/// Each case of a delivery with its glyph names resolved through its face.
fn resolve(delivery: &Delivery, face: &Sfnt<'_>) -> Result<Vec<Resolved>, Vec<String>> {
    let cff = face
        .table(u32::from_be_bytes(*b"CFF "))
        .and_then(Cff::parse);
    let count = face.glyph_count();
    let mut refusals = Vec::new();
    let mut out = Vec::new();
    for case in &delivery.cases {
        let mut glyphs = Vec::new();
        for written in &case.glyphs {
            let glyph = match &written.id {
                GlyphId::Index(index) => {
                    if count.is_some_and(|count| *index >= count) {
                        refusals.push(format!(
                            "the case `{}` names gid{index}, past the face's {} glyphs",
                            case.name,
                            count.unwrap_or(0)
                        ));
                        continue;
                    }
                    Some(*index)
                }
                GlyphId::Name(name) => cff
                    .as_ref()
                    .and_then(|cff| cff.gid_for_name(name))
                    .or_else(|| face.glyph_for_name(name)),
            };
            match glyph {
                Some(glyph) => glyphs.push(glyph),
                None => refusals.push(format!(
                    "the case `{}` names `{}`, which the face's own names do not give",
                    case.name,
                    match &written.id {
                        GlyphId::Name(name) => name.as_str(),
                        GlyphId::Index(_) => "",
                    }
                )),
            }
        }
        out.push(Resolved {
            name: case.name.clone(),
            case: case.clone(),
            glyphs,
        });
    }
    if refusals.is_empty() {
        Ok(out)
    } else {
        Err(refusals)
    }
}

/// `units × em ÷ upem`, rounded half away from zero, in integers: the
/// arithmetic `text_rendering.rs` scales by, done once to each position.
fn scale(units: i64, em: u32, upem: u16) -> i64 {
    let (em, upem) = (i64::from(em), i64::from(upem.max(1)));
    let magnitude = units
        .saturating_abs()
        .saturating_mul(em)
        .saturating_add(upem / 2)
        / upem;
    if units < 0 {
        -magnitude
    } else {
        magnitude
    }
}

/// The level of the character a run starts at.
fn level_at(levels: &[Level], offsets: &[u32], start: usize, base: Level) -> Level {
    offsets
        .iter()
        .position(|offset| *offset as usize == start)
        .and_then(|at| levels.get(at).copied())
        .unwrap_or(base)
}

/// What this crate draws for a case, in drawn order and in the case's units,
/// and where the pen ends.
fn drawn(case: &Case, face: &Sfnt<'_>, em: Option<u32>) -> (Vec<Drawn>, (i64, i64)) {
    let mut shaper = Shaper::new(face).with_settings(&case.settings);
    if let Some(language) = case.language {
        shaper = shaper.with_language(language);
    }
    let (paragraph, runs) = shaper.shape_text(&case.text, case.direction);
    let levels: Vec<Level> = runs
        .iter()
        .map(|run| {
            level_at(
                paragraph.levels(),
                paragraph.offsets(),
                run.text().start,
                paragraph.base_level(),
            )
        })
        .collect();
    let starts: Vec<usize> = case.text.char_indices().map(|(at, _)| at).collect();
    let units = |value: i64| match em {
        Some(em) => scale(value, em, face.units_per_em),
        None => value,
    };

    let mut out = Vec::new();
    let (mut pen_x, mut pen_y) = (0i64, 0i64);
    for index in bidi::reorder(&levels) {
        let Some(run) = runs.get(index) else {
            continue;
        };
        let glyphs: Vec<ShapedGlyph> = if run.direction().is_forward() {
            run.glyphs().to_vec()
        } else {
            run.glyphs().iter().rev().copied().collect()
        };
        for glyph in glyphs {
            out.push(Drawn {
                glyph: glyph.glyph,
                cluster: starts.iter().position(|at| *at == glyph.cluster as usize),
                x: units(pen_x + i64::from(glyph.x_offset)),
                y: units(pen_y + i64::from(glyph.y_offset)),
            });
            pen_x += i64::from(glyph.x_advance);
            pen_y += i64::from(glyph.y_advance);
        }
    }
    (out, (units(pen_x), units(pen_y)))
}

/// Where a case's own numbers draw each glyph, and where they leave the pen.
fn stated(written: &[Written]) -> (Vec<(i64, i64)>, Option<(i64, i64)>) {
    let mut places = Vec::new();
    let (mut pen_x, mut pen_y) = (0i64, 0i64);
    let mut end = Some((0, 0));
    for glyph in written {
        places.push((pen_x + glyph.offset.0, pen_y + glyph.offset.1));
        match glyph.advance {
            Some((x, y)) => {
                pen_x += x;
                pen_y += y;
                end = Some((pen_x, pen_y));
            }
            None => end = None,
        }
    }
    (places, end)
}

/// Holds what was drawn to what a case states, glyph by glyph.
fn compare(
    written: &[Written],
    glyphs: &[u16],
    drawn: &[Drawn],
    end: (i64, i64),
) -> Result<(), Mismatch> {
    let ours: Vec<u16> = drawn.iter().map(|d| d.glyph).collect();
    if ours != glyphs {
        let first = ours
            .iter()
            .zip(glyphs)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| ours.len().min(glyphs.len()));
        let mut sorted_ours = ours.clone();
        let mut sorted_case = glyphs.to_vec();
        sorted_ours.sort_unstable();
        sorted_case.sort_unstable();
        let verdict = if sorted_ours == sorted_case {
            Verdict::Order
        } else {
            Verdict::Set
        };
        return Err(Mismatch {
            verdict,
            at: first,
            detail: format!("drawn {ours:?}, stated {glyphs:?}"),
        });
    }
    for (at, (glyph, ours)) in written.iter().zip(drawn).enumerate() {
        if let Some(cluster) = glyph.cluster {
            if ours.cluster != Some(cluster) {
                return Err(Mismatch {
                    verdict: Verdict::Cluster,
                    at,
                    detail: format!(
                        "glyph {at} stands for the character at {:?}, the case says {cluster}",
                        ours.cluster
                    ),
                });
            }
        }
    }
    let (places, stated_end) = stated(written);
    for (at, (place, ours)) in places.iter().zip(drawn).enumerate() {
        if *place != (ours.x, ours.y) {
            return Err(Mismatch {
                verdict: Verdict::Position,
                at,
                detail: format!(
                    "glyph {at} is drawn at ({}, {}), the case says ({}, {})",
                    ours.x, ours.y, place.0, place.1
                ),
            });
        }
    }
    if let Some(stated_end) = stated_end {
        if stated_end != end {
            return Err(Mismatch {
                verdict: Verdict::Position,
                at: drawn.len(),
                detail: format!("the line ends at {end:?}, the case says {stated_end:?}"),
            });
        }
    }
    Ok(())
}

/// Shapes one resolved case and holds it to what it states.
fn reproduce(resolved: &Resolved, face: &Sfnt<'_>, em: Option<u32>) -> Result<(), Mismatch> {
    let (drawn, end) = drawn(&resolved.case, face, em);
    compare(&resolved.case.glyphs, &resolved.glyphs, &drawn, end)
}

/// The crate's own directory.
fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The person-written trees: `data/person-written` and every
/// `data/person-written-<name>` beside it, sorted.
fn trees() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(crate_dir().join("data")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name == "person-written" || name.starts_with("person-written-")
                    })
        })
        .collect();
    out.sort();
    out
}

/// Every case file in one tree, sorted.
fn case_files(tree: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(tree) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == EXTENSION))
        .collect();
    out.sort();
    out
}

/// What running one case file found: its script, and each case's outcome.
struct Ran {
    script: String,
    outcomes: Vec<(String, Result<(), Mismatch>)>,
}

/// Reads, resolves and runs one case file whose face is in `dir`.
fn run(text: &str, dir: &Path) -> Result<Ran, Vec<String>> {
    let delivery = read(text)?;
    let path = dir.join(&delivery.face);
    let bytes = std::fs::read(&path).map_err(|error| {
        vec![format!(
            "the face `{}` cannot be read from {}: {error}",
            delivery.face,
            dir.display()
        )]
    })?;
    let face = Sfnt::parse(&bytes).ok_or_else(|| {
        vec![format!(
            "the face `{}` is no sfnt `Sfnt::parse` reads",
            delivery.face
        )]
    })?;
    let resolved = resolve(&delivery, &face)?;
    Ok(Ran {
        script: String::from_utf8_lossy(&delivery.script).into_owned(),
        outcomes: resolved
            .iter()
            .map(|case| {
                (
                    case.name.clone(),
                    reproduce(case, &face, delivery.units_per_em),
                )
            })
            .collect(),
    })
}

/// **The bar.** Every delivered case read, refused or shaped and compared,
/// and the per-script counts held to [`PERSON_WRITTEN`].
#[test]
fn every_person_written_case_reproduces() {
    let mut refused: Vec<String> = Vec::new();
    let mut failing: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, (usize, usize, Vec<String>)> = BTreeMap::new();
    let trees = trees();
    for tree in &trees {
        for path in case_files(tree) {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    refused.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            match run(&text, tree) {
                Err(problems) => {
                    for problem in problems {
                        refused.push(format!("{}: {problem}", path.display()));
                    }
                }
                Ok(ran) => {
                    let entry = counts.entry(ran.script.clone()).or_default();
                    for (name, outcome) in ran.outcomes {
                        if entry.2.contains(&name) {
                            refused.push(format!(
                                "{}: the case name `{name}` is used twice in `{}`",
                                path.display(),
                                ran.script
                            ));
                        }
                        entry.2.push(name.clone());
                        entry.0 += 1;
                        match outcome {
                            Ok(()) => entry.1 += 1,
                            Err(mismatch) => failing.push(format!(
                                "{} `{name}`: {:?} from glyph {}: {}",
                                ran.script, mismatch.verdict, mismatch.at, mismatch.detail
                            )),
                        }
                    }
                }
            }
        }
    }

    if counts.is_empty() {
        println!(
            "RAN person-written: no script has a case ({} trees read)",
            trees.len()
        );
    } else {
        let line: Vec<String> = counts
            .iter()
            .map(|(script, (cases, passing, _))| format!("{script} {passing} of {cases}"))
            .collect();
        println!("RAN person-written: {}", line.join(", "));
    }

    assert!(
        refused.is_empty(),
        "delivered case files that the format refuses — a delivery to mend, never a case \
         to pass over:\n{}",
        refused.join("\n")
    );
    let found: Vec<(String, usize, usize)> = counts
        .into_iter()
        .map(|(script, (cases, passing, _))| (script, cases, passing))
        .collect();
    let table: Vec<(String, usize, usize)> = PERSON_WRITTEN
        .iter()
        .map(|(script, cases, passing)| ((*script).to_string(), *cases, *passing))
        .collect();
    assert_eq!(
        found,
        table,
        "PERSON_WRITTEN is a ratchet: each script's cases and how many reproduce. The \
         cases that do not:\n{}",
        failing.join("\n")
    );
}

/// `SHARAN-1/1`, *لسان* in Nasta‘līq, as Unicode's text-rendering-tests
/// states it: the glyph names and positions its fixture writes, in its own
/// thousandths of an em and in drawn order, `sp0` — which `check.py` drops
/// for its empty outline — kept, since a person lists every glyph. The corpus
/// states no clusters and no line's end, so this states neither. Its offsets
/// are where the fixture's `<use>` elements stand; each advance is the
/// distance to the next glyph's pen, the mark's pen at its base's.
const SHARAN_1_1: &str = "\
# SHARAN-1/1 of unicode-org/text-rendering-tests, testcases/SHARAN-1.html.
format 1
script Arab
face TestShapeAran.ttf
author The authors of Unicode's text-rendering-tests, who wrote SHARAN-1's expected glyphs into its fixture
standing The conformance corpus Unicode publishes for text rendering, whose expectations its authors wrote
date 2026-08-24
face-source unicode-org/text-rendering-tests at 26cfb96, fonts/TestShapeAran.ttf
face-licence Unicode-3.0
units-per-em 1000

case SHARAN-1/1
text U+0644 U+0633 U+0627 U+0646
direction rtl
glyph OneDotEnclNS - 0 0 398 -1
glyph NoonxSep - 861 0 0 0
glyph AlefFin - 264 0 0 0
glyph SeenMed.inT2outT1 - 539 0 0 0
glyph sp0 - 0 0 0 0
glyph LamIni.outT2 - - - 0 223
";

/// The directory `SHARAN_1_1`'s face is in.
fn text_rendering_fonts() -> PathBuf {
    crate_dir().join("data/text-rendering-tests/fonts")
}

/// The one outcome of a one-case file.
fn outcome(text: &str) -> Result<(), Mismatch> {
    let ran = run(text, &text_rendering_fonts()).expect("the case reads and resolves");
    assert_eq!(ran.script, "Arab");
    assert_eq!(ran.outcomes.len(), 1, "one case");
    ran.outcomes
        .into_iter()
        .next()
        .map_or(Ok(()), |(_, outcome)| outcome)
}

/// **The bar compares what it claims to.** A published expectation, written
/// into the format, passes; with one glyph changed, one offset changed, or
/// two glyphs exchanged, it fails, with the verdict that names the change and
/// the glyph it starts at.
#[test]
fn the_harness_compares_what_it_claims() {
    assert_eq!(
        outcome(SHARAN_1_1),
        Ok(()),
        "SHARAN-1/1 as its fixture states it"
    );

    // One glyph: the alef's final form for the noon's.
    let glyph = SHARAN_1_1.replace("glyph AlefFin - 264", "glyph NoonxSep - 264");
    assert_ne!(glyph, SHARAN_1_1);
    let failed = outcome(&glyph).expect_err("a changed glyph fails");
    assert_eq!((failed.verdict, failed.at), (Verdict::Set, 2), "{failed:?}");

    // One offset: the noon's dot one unit to the left.
    let offset = SHARAN_1_1.replace("OneDotEnclNS - 0 0 398 -1", "OneDotEnclNS - 0 0 397 -1");
    assert_ne!(offset, SHARAN_1_1);
    let failed = outcome(&offset).expect_err("a moved offset fails");
    assert_eq!(
        (failed.verdict, failed.at),
        (Verdict::Position, 0),
        "{failed:?}"
    );

    // One rise: the lam one unit lower.
    let rise = SHARAN_1_1.replace("LamIni.outT2 - - - 0 223", "LamIni.outT2 - - - 0 222");
    assert_ne!(rise, SHARAN_1_1);
    let failed = outcome(&rise).expect_err("a moved rise fails");
    assert_eq!(
        (failed.verdict, failed.at),
        (Verdict::Position, 5),
        "{failed:?}"
    );

    // Two glyphs exchanged: the same glyphs in another order.
    let order = SHARAN_1_1.replace(
        "glyph NoonxSep - 861 0 0 0\nglyph AlefFin - 264 0 0 0",
        "glyph AlefFin - 264 0 0 0\nglyph NoonxSep - 861 0 0 0",
    );
    assert_ne!(order, SHARAN_1_1);
    let failed = outcome(&order).expect_err("an exchange fails");
    assert_eq!(
        (failed.verdict, failed.at),
        (Verdict::Order, 1),
        "{failed:?}"
    );

    // A glyph more, and a glyph fewer.
    let more = SHARAN_1_1.replace(
        "glyph sp0 - 0 0 0 0\n",
        "glyph sp0 - 0 0 0 0\nglyph sp0 - 0 0 0 0\n",
    );
    let failed = outcome(&more).expect_err("a glyph more fails");
    assert_eq!(failed.verdict, Verdict::Set, "{failed:?}");
    let fewer = SHARAN_1_1.replace("glyph sp0 - 0 0 0 0\n", "");
    let failed = outcome(&fewer).expect_err("a glyph fewer fails");
    assert_eq!(failed.verdict, Verdict::Set, "{failed:?}");

    // The same case at the face's own units would be another statement: read
    // as design units, every position is half the distance it should be.
    let units = SHARAN_1_1.replace("units-per-em 1000\n", "");
    let failed = outcome(&units).expect_err("the em is part of the statement");
    assert_eq!(failed.verdict, Verdict::Position, "{failed:?}");
}

/// **A case without its provenance is refused**, naming the field: each of
/// the five, missing alone, and all of them at once.
#[test]
fn a_case_without_its_provenance_is_refused() {
    for field in PROVENANCE {
        let without: String = SHARAN_1_1
            .lines()
            .filter(|line| !line.starts_with(&format!("{field} ")))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_ne!(without, SHARAN_1_1, "{field} was in the case");
        let refusals = read(&without).expect_err("refused");
        assert_eq!(
            refusals,
            vec![format!("the provenance field `{field}` is missing")],
            "{field}"
        );
    }

    let bare: String = SHARAN_1_1
        .lines()
        .filter(|line| {
            !PROVENANCE
                .iter()
                .any(|field| line.starts_with(&format!("{field} ")))
        })
        .map(|line| format!("{line}\n"))
        .collect();
    let refusals = read(&bare).expect_err("refused");
    for field in PROVENANCE {
        assert!(
            refusals.contains(&format!("the provenance field `{field}` is missing")),
            "{field}: {refusals:?}"
        );
    }

    // A value that is no value is no provenance.
    let blank = SHARAN_1_1.replace("date 2026-08-24", "date");
    let refusals = read(&blank).expect_err("refused");
    assert!(
        refusals.iter().any(|r| r.contains("`date` has no value")),
        "{refusals:?}"
    );
    assert!(
        refusals.contains(&"the provenance field `date` is missing".to_string()),
        "{refusals:?}"
    );
}

/// Whether reading `text` is refused with a refusal containing `needle`.
fn refused_with(text: &str, needle: &str) -> bool {
    match read(text) {
        Ok(_) => false,
        Err(refusals) => refusals.iter().any(|refusal| refusal.contains(needle)),
    }
}

/// **Every malformed statement is refused by name**: each refusal the design
/// lists that reading alone decides.
#[test]
fn every_malformed_statement_is_refused_by_name() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "glyph sp0 - 0 0 0 0",
            "colour red\nglyph sp0 - 0 0 0 0",
            "is no keyword of the format",
        ),
        (
            "case SHARAN-1/1",
            "case SHARAN-1/1\nauthor Somebody else",
            "comes after the first case",
        ),
        ("format 1", "format 1\nformat 1", "is given twice"),
        ("format 1", "format 2", "is not format 1"),
        ("date 2026-08-24", "date 2026-02-30", "is no calendar day"),
        ("date 2026-08-24", "date 2025-02-29", "is no calendar day"),
        (
            "date 2026-08-24",
            "date 24 August 2026",
            "is no calendar day",
        ),
        (
            "face-licence Unicode-3.0",
            "face-licence OFL 1.1",
            "is no SPDX identifier",
        ),
        ("script Arab", "script arab", "is no ISO 15924 code"),
        ("script Arab", "script Syrc", "not in the file's `Syrc`"),
        (
            "text U+0644 U+0633 U+0627 U+0646",
            "text U+0644 U+0041",
            "is in `Latn`",
        ),
        (
            "text U+0644 U+0633 U+0627 U+0646",
            "text U+0020",
            "no character of the case",
        ),
        (
            "text U+0644 U+0633 U+0627 U+0646",
            "text U+D800",
            "no Unicode scalar value",
        ),
        (
            "text U+0644 U+0633 U+0627 U+0646",
            "text U+110000",
            "no Unicode scalar value",
        ),
        (
            "text U+0644 U+0633 U+0627 U+0646",
            "text U+64G",
            "four to six hex digits",
        ),
        (
            "text U+0644 U+0633 U+0627 U+0646",
            "text 0644",
            "written U+",
        ),
        ("text U+0644 U+0633 U+0627 U+0646\n", "", "has no `text`"),
        ("direction rtl\n", "", "has no `direction`"),
        (
            "direction rtl",
            "direction backwards",
            "is not `ltr`, `rtl` or `auto`",
        ),
        (
            "direction rtl",
            "direction rtl\ndirection ltr",
            "is given twice in one case",
        ),
        (
            "glyph sp0 - 0 0 0 0",
            "glyph sp0 2 0 0 0 0",
            "states clusters for 1 of its 6 glyphs",
        ),
        (
            "glyph sp0 - 0 0 0 0",
            "glyph sp0 9 0 0 0 0",
            "past the text's 4 code points",
        ),
        (
            "glyph sp0 - 0 0 0 0",
            "glyph sp0 - - - 0 0",
            "only as both advances",
        ),
        (
            "glyph LamIni.outT2 - - - 0 223",
            "glyph LamIni.outT2 - - 0 0 223",
            "only as both advances",
        ),
        (
            "glyph sp0 - 0 0 0 0",
            "glyph sp0 - 0 0 0",
            "is not six fields",
        ),
        (
            "glyph sp0 - 0 0 0 0",
            "glyph sp0 - 0 0 x 0",
            "is not a whole number",
        ),
        (
            "glyph sp0 - 0 0 0 0",
            "glyph gid70000 - 0 0 0 0",
            "past the 65 536 glyphs",
        ),
        (
            "units-per-em 1000",
            "units-per-em 8",
            "is not a whole number from 16 to 16 384",
        ),
        (
            "units-per-em 1000",
            "units-per-em 20000",
            "is not a whole number from 16 to 16 384",
        ),
        (
            "direction rtl",
            "direction rtl\nfeatures liga",
            "is not `tag=value`",
        ),
        (
            "direction rtl",
            "direction rtl\nfeatures lig=1",
            "not four letters or digits",
        ),
        (
            "direction rtl",
            "direction rtl\nfeatures liga=on",
            "not a decimal number",
        ),
        (
            "direction rtl",
            "direction rtl\nlanguage URDUX",
            "not one to four letters or digits",
        ),
        (
            "direction rtl",
            "direction rtl\nlanguage UR-D",
            "not one to four letters or digits",
        ),
    ];
    for (from, to, needle) in cases {
        let changed = if to.is_empty() {
            SHARAN_1_1.replacen(from, "", 1)
        } else {
            SHARAN_1_1.replacen(from, to, 1)
        };
        assert_ne!(changed, SHARAN_1_1, "`{from}` is in the case");
        assert!(
            refused_with(&changed, needle),
            "`{to}` for `{from}` is refused with `{needle}`: {:?}",
            read(&changed).err()
        );
    }

    // Glyph lines with nothing before them, and a case with no glyph.
    let orphan = "format 1\nglyph sp0 - 0 0 0 0\n";
    assert!(refused_with(orphan, "stands outside any case"));
    let empty: String = SHARAN_1_1
        .lines()
        .filter(|line| !line.starts_with("glyph "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert!(refused_with(&empty, "names no glyph"));

    // A case named twice.
    let case = &SHARAN_1_1[SHARAN_1_1.find("case ").expect("a case")..];
    let twice = format!("{SHARAN_1_1}\n{case}");
    assert!(refused_with(&twice, "is used twice"));

    // CRLF, which the format does not use.
    let crlf = SHARAN_1_1.replace('\n', "\r\n");
    assert!(refused_with(&crlf, "carriage return"));

    // And the transcription itself reads, so every refusal above is the
    // change's.
    let delivery = read(SHARAN_1_1).expect("it reads");
    assert_eq!(delivery.script, *b"Arab");
    assert_eq!(delivery.units_per_em, Some(1000));
    assert_eq!(delivery.cases.len(), 1);
    let only = &delivery.cases[0];
    assert_eq!(only.text, "\u{0644}\u{0633}\u{0627}\u{0646}");
    assert_eq!(only.direction, BaseDirection::RightToLeft);
    assert_eq!(only.glyphs.len(), 6);
    assert_eq!(only.glyphs[0].id, GlyphId::Name("OneDotEnclNS".to_string()));
    assert_eq!(only.glyphs[0].offset, (398, -1));
    assert_eq!(only.glyphs[5].advance, None);
}

/// What reading gives a well-formed statement of each optional field, and
/// the comments and blank lines the format passes over.
#[test]
fn the_optional_fields_read_as_the_format_states_them() {
    let rich = SHARAN_1_1.replace(
        "direction rtl\n",
        "direction rtl\n# a comment\n\nfeatures liga=0 ss01=1 kern=0\nlanguage URD\n",
    );
    let delivery = read(&rich).expect("it reads");
    let case = &delivery.cases[0];
    assert_eq!(
        case.settings,
        vec![
            (Tag::new(b"liga"), 0),
            (Tag::new(b"ss01"), 1),
            (Tag::new(b"kern"), 0),
        ]
    );
    assert_eq!(case.language, Some(Tag::new(b"URD ")));

    let indexed = SHARAN_1_1.replace("glyph sp0 - 0 0 0 0", "glyph gid42 - 0 0 0 0");
    let delivery = read(&indexed).expect("it reads");
    assert_eq!(delivery.cases[0].glyphs[4].id, GlyphId::Index(42));

    let auto = SHARAN_1_1.replace("direction rtl", "direction auto");
    assert_eq!(
        read(&auto).expect("it reads").cases[0].direction,
        BaseDirection::Auto
    );
    let ltr = SHARAN_1_1.replace("direction rtl", "direction ltr");
    assert_eq!(
        read(&ltr).expect("it reads").cases[0].direction,
        BaseDirection::LeftToRight
    );

    // A day of a leap year, and an identifier closing with `+`.
    let leap = SHARAN_1_1
        .replace("date 2026-08-24", "date 2024-02-29")
        .replace("face-licence Unicode-3.0", "face-licence GPL-2.0+");
    assert!(read(&leap).is_ok(), "{:?}", read(&leap).err());
}

/// **A glyph the face does not name is refused**, as is an index past its
/// glyphs, a face that is not there, and a file that is no face.
#[test]
fn a_glyph_the_face_does_not_name_is_refused() {
    let dir = text_rendering_fonts();
    let unnamed = SHARAN_1_1.replace("glyph sp0 - 0 0 0 0", "glyph uniFFFF.nowhere - 0 0 0 0");
    let refusals = run(&unnamed, &dir).err().expect("refused");
    assert!(
        refusals
            .iter()
            .any(|r| r.contains("`uniFFFF.nowhere`, which the face's own names do not give")),
        "{refusals:?}"
    );

    let past = SHARAN_1_1.replace("glyph sp0 - 0 0 0 0", "glyph gid65535 - 0 0 0 0");
    let refusals = run(&past, &dir).err().expect("refused");
    assert!(
        refusals.iter().any(|r| r.contains("past the face's")),
        "{refusals:?}"
    );

    let missing = SHARAN_1_1.replace("face TestShapeAran.ttf", "face NoSuchFace.ttf");
    let refusals = run(&missing, &dir).err().expect("refused");
    assert!(
        refusals.iter().any(|r| r.contains("cannot be read")),
        "{refusals:?}"
    );

    // The corpus's licence text is a file and no face.
    let not_a_face = SHARAN_1_1.replace("face TestShapeAran.ttf", "face LICENSE");
    let refusals = run(&not_a_face, &crate_dir().join("data/text-rendering-tests"))
        .err()
        .expect("refused");
    assert!(
        refusals.iter().any(|r| r.contains("is no sfnt")),
        "{refusals:?}"
    );
}

/// The comparison reads a cluster, and the line's end, where a case states
/// them, and passes them over where it does not: held on lines built here,
/// glyph by glyph, with no face and no shaper in between.
#[test]
fn the_comparison_reads_clusters_and_the_line_end_where_a_case_states_them() {
    let line = |cluster: Option<usize>, advance: Option<(i64, i64)>| Written {
        id: GlyphId::Index(7),
        cluster,
        advance,
        offset: (0, 0),
    };
    let written = vec![
        line(Some(0), Some((500, 0))),
        Written {
            id: GlyphId::Index(9),
            cluster: Some(1),
            advance: Some((0, 0)),
            offset: (-250, 400),
        },
        line(Some(2), Some((300, 0))),
    ];
    let glyphs = [7, 9, 7];
    let drawn = |second_cluster: Option<usize>, mark_x: i64| {
        vec![
            Drawn {
                glyph: 7,
                cluster: Some(0),
                x: 0,
                y: 0,
            },
            Drawn {
                glyph: 9,
                cluster: second_cluster,
                x: mark_x,
                y: 400,
            },
            Drawn {
                glyph: 7,
                cluster: Some(2),
                x: 500,
                y: 0,
            },
        ]
    };
    assert_eq!(
        compare(&written, &glyphs, &drawn(Some(1), 250), (800, 0)),
        Ok(())
    );

    // A mark drawn at the same place by another split of pen and offset is
    // the same line.
    let split = vec![
        line(Some(0), Some((250, 0))),
        Written {
            id: GlyphId::Index(9),
            cluster: Some(1),
            advance: Some((250, 0)),
            offset: (0, 400),
        },
        line(Some(2), Some((300, 0))),
    ];
    assert_eq!(
        compare(&split, &glyphs, &drawn(Some(1), 250), (800, 0)),
        Ok(())
    );

    // A cluster that differs fails as a cluster.
    let failed = compare(&written, &glyphs, &drawn(Some(0), 250), (800, 0)).expect_err("fails");
    assert_eq!((failed.verdict, failed.at), (Verdict::Cluster, 1));

    // Where the case states none, it is not read.
    let unclustered: Vec<Written> = written
        .iter()
        .map(|glyph| Written {
            cluster: None,
            ..glyph.clone()
        })
        .collect();
    assert_eq!(
        compare(&unclustered, &glyphs, &drawn(None, 250), (800, 0)),
        Ok(())
    );

    // A line that ends elsewhere fails where its end is stated.
    let failed = compare(&written, &glyphs, &drawn(Some(1), 250), (801, 0)).expect_err("fails");
    assert_eq!((failed.verdict, failed.at), (Verdict::Position, 3));
    let mut open = written.clone();
    if let Some(last) = open.last_mut() {
        last.advance = None;
    }
    assert_eq!(
        compare(&open, &glyphs, &drawn(Some(1), 250), (801, 0)),
        Ok(())
    );

    // And a mark one unit off fails as a position, at the mark.
    let failed = compare(&written, &glyphs, &drawn(Some(1), 251), (800, 0)).expect_err("fails");
    assert_eq!((failed.verdict, failed.at), (Verdict::Position, 1));
}

/// Positions at another em are scaled once each, half away from zero, in
/// integers: `text_rendering.rs`'s worked figure among them.
#[test]
fn positions_at_another_em_are_scaled_once_and_rounded_half_away_from_zero() {
    // 427 × 1000 ÷ 2048 = 208.496…, the figure `text_rendering.rs` works.
    assert_eq!(scale(427, 1000, 2048), 208);
    assert_eq!(scale(-427, 1000, 2048), -208);
    // A half rounds away from zero, either side of it.
    assert_eq!(scale(1, 1, 2), 1);
    assert_eq!(scale(-1, 1, 2), -1);
    assert_eq!(scale(1024, 1000, 2048), 500);
    assert_eq!(scale(0, 1000, 2048), 0);
    // The face's own em is the identity.
    assert_eq!(scale(-12_345, 2048, 2048), -12_345);
}

/// The trees are where the design puts them, and none is a file.
#[test]
fn the_trees_are_read_where_the_design_puts_them() {
    for tree in trees() {
        let name = tree
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        assert!(
            name == "person-written" || name.starts_with("person-written-"),
            "{name}"
        );
        assert!(tree.is_dir());
    }
    // Every row of the ratchet names a script by its code.
    for (script, cases, passing) in PERSON_WRITTEN {
        assert!(iso15924(script).is_some(), "{script}");
        assert!(passing <= cases, "{script}");
        assert!(*cases > 0, "{script}: a script with no case has no row");
    }
}
