//! Type 1 to CFF: a Type 1 program rewritten as a bare CFF of Type 2
//! charstrings (`docs/design/subsetting.md`, *Type 1 to Type 2*).
//!
//! The two formats draw one outline language spelled two ways. Each
//! charstring runs on a machine that is [`crate::type1`]'s reader step for
//! step — the same number forms, the same 48-operand stack, the same calls,
//! the same `callothersubr`/`pop` protocol and the same bounds — and what it
//! draws is written as Type 2: the side bearing folded into the first move,
//! `closepath` left to the next move and to `endchar`, every subroutine
//! inlined, a flex as Type 2's `flex`, a `seac` as the base's path and the
//! accent's in one charstring, and the stems every hint replacement declares
//! gathered at the start, with a `hintmask` wherever the set changes.
//!
//! **Each operand is chosen so the Type 2 reader adds what the Type 1 reader
//! added.** Where the two pens stand at the same point the program's own
//! operand is written — a `div` as Type 2's `div` — and elsewhere the exact
//! difference, or the side bearing and the operand summed by Type 2's `add`;
//! a coordinate no Type 2 operand reaches exactly refuses the program, never
//! rounds it. Then every glyph is outlined and advanced in both programs, and
//! every name and every built-in code read back, before an answer is given:
//! a conversion that disagrees anywhere is a [`Type1CffRefusal`], and the
//! caller keeps the Type 1 program (ruling 2).

use std::collections::{BTreeMap, BTreeSet};

use crate::cff::{round_to_fixed, Cff, STANDARD_ENCODING, STANDARD_STRINGS};
use crate::cff_subset::{charstring_int, dict_int, dict_op, dict_operand, write_index};
use crate::encoding::{base_glyph_name, BaseEncoding};
use crate::type1::{find, ps_number, scan_subrs, Key, Layout, Type1};

/// How deep a call may nest, as the reader's (chapter 6, *callsubr*).
const MAX_DEPTH: u32 = 10;
/// The operators one glyph's run reads, as the reader's.
const MAX_OPS: u32 = 100_000;
/// The operands either stack holds: the reader's Type 1 stack, and Type 2's
/// (5177, appendix B).
const MAX_OPERANDS: usize = 48;
/// The stem hints a Type 2 charstring declares at most (5177, appendix B).
const MAX_STEMS: usize = 96;
/// The bytes a Type 2 charstring holds at most (5177, appendix B).
const MAX_CHARSTRING: usize = 65_535;
/// The steps one conversion may spend running and verifying its glyphs: a
/// program is attacker-controlled (ruling 1), each glyph's run is bounded,
/// and a program of many glyphs is not.
const WORK_BUDGET: u64 = 40_000_000;
/// The subroutine indices the reader stores.
const MAX_SUBRS: usize = 65_536;
/// The glyphs a CFF holds, `.notdef` among them: a CharStrings INDEX counts
/// in two bytes.
const MAX_GLYPHS: usize = 65_535;

/// A Type 1 program rewritten as a bare CFF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Type1Cff {
    /// The CFF: one font, Type 2 charstrings, no subroutines, `CharstringType`
    /// unstated.
    pub program: Vec<u8>,
    /// Each glyph's name in the CFF's glyph order, `.notdef` first.
    pub names: Vec<Vec<u8>>,
    /// The CFF glyph each Type 1 glyph became, by the glyph's index in the
    /// order [`Type1::parse`] reads the charstrings.
    pub glyphs: Vec<u16>,
}

/// Why a Type 1 program was not converted (`docs/design/subsetting.md`,
/// *Type 1 to Type 2*). Each names a construct the converter would have to
/// guess at; the caller keeps the Type 1 program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Type1CffRefusal {
    /// The reader declines the program, or it carries no charstring.
    Unreadable,
    /// A multiple-master program: its glyphs are its masters blended at an
    /// instance, which one CFF cannot carry.
    MultipleMaster,
    /// More glyphs than a CFF holds, `.notdef` included.
    TooManyGlyphs,
    /// Two charstrings under one name: a PostScript interpreter keeps the
    /// last and this reader the first.
    DuplicateName,
    /// A glyph name that is empty or not UTF-8, which the CFF reader cannot
    /// look up.
    GlyphName,
    /// No `/FontName`, or one that is not 1 to 127 bytes of printable ASCII
    /// outside the characters Technical Note 5176 reserves.
    FontName,
    /// A glyph calls a subroutine the program does not define.
    MissingSubroutine,
    /// A glyph's run calls past ten subroutines deep.
    SubroutineDepth,
    /// A glyph's run spends the reader's operator budget, or the conversion
    /// spends its own.
    Budget,
    /// A glyph's run pushes more operands than the stack holds, or its Type 2
    /// charstring would.
    StackOverflow,
    /// `sbw` with a vertical side bearing or a vertical width other than 0:
    /// Type 2 has no vertical metrics.
    VerticalMetrics,
    /// An OtherSubrs entry past 3, which only a PostScript interpreter
    /// answers.
    OtherSubr,
    /// `pop` with nothing the flex or hint-replacement protocol left for it,
    /// `setcurrentpoint` anywhere but after a flex, or a value either
    /// protocol hands `pop` used as an operand (chapter 8).
    Protocol,
    /// A flex that is not chapter 8.3's: not seven points, one inside
    /// another, one never ended, one interrupted by anything but its moves,
    /// or one drawn where no path is open.
    Flex,
    /// A hint replacement that names no subroutine the program defines, or
    /// whose subroutine does more than declare stems.
    HintReplacement,
    /// More stem hints than a Type 2 charstring declares.
    TooManyStems,
    /// A path drawn before the glyph's first move or after a `closepath`,
    /// which a Type 2 charstring cannot draw without a move.
    Path,
    /// A `seac` whose base or accent the program lacks.
    Accent,
    /// A number the converted charstring needs that no Type 2 operand holds
    /// exactly.
    Number,
    /// A converted charstring past 65 535 bytes, or charstrings that together
    /// pass sixteen times the program's own length.
    TooLarge,
    /// The converted program does not outline or advance a glyph, name it,
    /// or map a built-in code to it, as the program does.
    Mismatch,
}

impl core::fmt::Display for Type1CffRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Type1CffRefusal::Unreadable => "the program does not read as Type 1",
            Type1CffRefusal::MultipleMaster => "a multiple-master program",
            Type1CffRefusal::TooManyGlyphs => "more glyphs than a CFF holds",
            Type1CffRefusal::DuplicateName => "two charstrings under one name",
            Type1CffRefusal::GlyphName => "a glyph name the CFF reader cannot look up",
            Type1CffRefusal::FontName => "no /FontName a CFF can carry",
            Type1CffRefusal::MissingSubroutine => "a glyph calls a missing subroutine",
            Type1CffRefusal::SubroutineDepth => "a glyph calls subroutines past ten deep",
            Type1CffRefusal::Budget => "a glyph spends the operator budget",
            Type1CffRefusal::StackOverflow => "a glyph overflows the operand stack",
            Type1CffRefusal::VerticalMetrics => "an sbw with vertical metrics",
            Type1CffRefusal::OtherSubr => "an OtherSubrs entry past 3",
            Type1CffRefusal::Protocol => "a pop or setcurrentpoint outside its protocol",
            Type1CffRefusal::Flex => "a flex outside chapter 8.3's form",
            Type1CffRefusal::HintReplacement => "a hint replacement that declares no stems",
            Type1CffRefusal::TooManyStems => "more than 96 stem hints",
            Type1CffRefusal::Path => "a path drawn without a move",
            Type1CffRefusal::Accent => "a seac whose component the program lacks",
            Type1CffRefusal::Number => "a number no Type 2 operand holds exactly",
            Type1CffRefusal::TooLarge => "converted charstrings past their bound",
            Type1CffRefusal::Mismatch => {
                "the conversion does not draw, name or encode a glyph as the program does"
            }
        })
    }
}

type Refusal = Type1CffRefusal;

/// Rewrites a Type 1 program as a bare CFF: every charstring a Type 2
/// charstring drawing what [`Type1::outline`] draws and advancing as
/// [`Type1::advance`] does, the charset naming every glyph with `.notdef`
/// first — an empty one where the program has none — the built-in encoding
/// carried, the predefined Standard one where it is that, and the program's
/// `FontMatrix`, `FontBBox`, `FontInfo` and hinting values in the Top and
/// Private DICTs.
///
/// # Errors
///
/// A [`Type1CffRefusal`] naming the construct it would not map; the caller
/// keeps the Type 1 program.
pub fn type1_to_cff(program: &[u8]) -> Result<Type1Cff, Type1CffRefusal> {
    let layout = Layout::read(program).ok_or(Refusal::Unreadable)?;
    let font = Type1::parse(program).ok_or(Refusal::Unreadable)?;
    let count = font.glyph_count();
    if count == 0 {
        return Err(Refusal::Unreadable);
    }
    if font.is_multiple_master() {
        return Err(Refusal::MultipleMaster);
    }

    // ---- the names --------------------------------------------------------

    let mut names: Vec<&[u8]> = Vec::with_capacity(count);
    let mut seen: BTreeSet<&[u8]> = BTreeSet::new();
    for index in 0..count {
        let name = font.glyph_name(index).ok_or(Refusal::Unreadable)?;
        if name.is_empty() || core::str::from_utf8(name).is_err() {
            return Err(Refusal::GlyphName);
        }
        if !seen.insert(name) {
            return Err(Refusal::DuplicateName);
        }
        names.push(name);
    }
    let notdef = names.iter().position(|name| *name == b".notdef");
    let total = count + usize::from(notdef.is_none());
    if total > MAX_GLYPHS {
        return Err(Refusal::TooManyGlyphs);
    }
    let facts = Facts::read(&layout)?;

    // ---- every glyph drawn ------------------------------------------------

    let defined: BTreeSet<usize> = scan_subrs(&layout.private)
        .entries
        .iter()
        .filter_map(|entry| match entry.key {
            Key::Subr(index) => stored(index),
            Key::Glyph(_) => None,
        })
        .collect();
    let mut work = 0u64;
    let mut drawn: Vec<Drawn> = Vec::with_capacity(count);
    for glyph in 0..count {
        drawn.push(draw(&font, &defined, glyph, &mut work)?);
    }

    // ---- the glyph order --------------------------------------------------

    // The glyph each built-in code reaches, `.notdef` not counted: 5176's
    // encoding gives glyph 0 to no code.
    let mut by_code: [Option<usize>; 256] = [None; 256];
    let mut primary: Vec<Option<u8>> = vec![None; count];
    for code in 0..=255u8 {
        let Some(glyph) = font.glyph_for_code(code).map(usize::from) else {
            continue;
        };
        if Some(glyph) == notdef || glyph >= count {
            continue;
        }
        by_code[usize::from(code)] = Some(glyph);
        primary[glyph].get_or_insert(code);
    }
    // `.notdef` first (5176), then the encoded glyphs in the order of the
    // lowest code each answers to, which the encoding's tables need, then
    // every other glyph in program order.
    let mut encoded: Vec<usize> = (0..count).filter(|g| primary[*g].is_some()).collect();
    encoded.sort_by_key(|g| primary[*g]);
    let mut order: Vec<Option<usize>> = vec![notdef];
    order.extend(encoded.iter().map(|g| Some(*g)));
    order.extend(
        (0..count)
            .filter(|g| Some(*g) != notdef && primary[*g].is_none())
            .map(Some),
    );
    let mut gids = vec![0u16; count];
    for (gid, glyph) in order.iter().enumerate() {
        if let Some(glyph) = glyph {
            gids[*glyph] = u16::try_from(gid).map_err(|_| Refusal::TooManyGlyphs)?;
        }
    }
    let order_names: Vec<&[u8]> = order
        .iter()
        .map(|glyph| glyph.map_or(b".notdef".as_slice(), |g| names[g]))
        .collect();

    // ---- the charstrings --------------------------------------------------

    let bound = program.len().saturating_mul(16).saturating_add(65_536);
    let mut charstrings: Vec<Vec<u8>> = Vec::with_capacity(order.len());
    let mut written = 0usize;
    for glyph in &order {
        let code = match glyph {
            Some(glyph) => write_glyph(&drawn[*glyph])?,
            // A program without `.notdef`: one that draws nothing and
            // advances nothing, which is what a name the program lacks draws.
            None => vec![14],
        };
        written = written.saturating_add(code.len());
        if written > bound {
            return Err(Refusal::TooLarge);
        }
        charstrings.push(code);
    }

    let mut desired = [0u16; 256];
    for (code, glyph) in by_code.iter().enumerate() {
        if let Some(glyph) = glyph {
            desired[code] = gids[*glyph];
        }
    }
    let out = assemble(
        &font,
        &facts,
        &order_names,
        &charstrings,
        &desired,
        encoded.len(),
    )?;
    verify(&font, &out, &gids, notdef.is_none(), &mut work)?;
    Ok(Type1Cff {
        program: out,
        names: order_names.iter().map(|name| name.to_vec()).collect(),
        glyphs: gids,
    })
}

/// The index the reader stores a `/Subrs` entry at (`type1.rs`): a negative
/// one at 0, and one past 65 535 nowhere.
fn stored(index: i64) -> Option<usize> {
    usize::try_from(index.max(0))
        .ok()
        .filter(|index| *index < MAX_SUBRS)
}

// ---------------------------------------------------------------------------
// Operands.
// ---------------------------------------------------------------------------

/// One operand on the converter's Type 1 stack: the value the reader holds,
/// and what it is.
#[derive(Clone, Debug, PartialEq)]
struct Operand {
    value: f64,
    kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// Type 2 bytes that push exactly `value`, needing `depth` slots of the
    /// stack while they run.
    Code { bytes: Vec<u8>, depth: usize },
    /// A number no Type 2 operand holds exactly.
    Unwritable,
    /// The x `pop` takes from a flex's end (chapter 8.3), which only
    /// `setcurrentpoint` may consume.
    FlexX,
    /// The y `pop` takes from a flex's end.
    FlexY,
    /// The subroutine number `pop` takes from a hint replacement (chapter
    /// 8.1), which only `callsubr` may consume.
    Hint,
}

impl Operand {
    /// A number as itself, where a Type 2 operand holds it.
    fn number(value: f64) -> Operand {
        let kind = match literal(value) {
            Some(bytes) => Kind::Code { bytes, depth: 1 },
            None => Kind::Unwritable,
        };
        Operand { value, kind }
    }

    /// The 0 the reader takes for an operand the stack does not hold.
    fn zero() -> Operand {
        Operand::number(0.0)
    }

    /// Whether this is a value the flex or hint-replacement protocol handed
    /// `pop`.
    fn is_protocol(&self) -> bool {
        matches!(self.kind, Kind::FlexX | Kind::FlexY | Kind::Hint)
    }

    /// The bytes that push it and the stack they need, where it is written.
    fn code(&self) -> Option<(&[u8], usize)> {
        match &self.kind {
            Kind::Code { bytes, depth } => Some((bytes.as_slice(), *depth)),
            _ => None,
        }
    }
}

/// `value` as one Type 2 operand: an integer where it is one within 16 bits,
/// otherwise a 16.16 fraction where one holds it exactly (5177 §3.2).
fn literal(value: f64) -> Option<Vec<u8>> {
    if !value.is_finite() {
        return None;
    }
    if value.fract() == 0.0 && (-32_768.0..=32_767.0).contains(&value) {
        return charstring_int(value as i32);
    }
    let fixed = value * 65_536.0;
    if fixed.fract() == 0.0 && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&fixed) {
        let mut out = vec![255u8];
        out.extend_from_slice(&(fixed as i32).to_be_bytes());
        return Some(out);
    }
    None
}

/// Whether the Type 2 reader keeps `value` as it is: its arithmetic sets a
/// result that is not finite, or not normal, to 0 (`cff.rs`'s `settle`).
fn settled(value: f64) -> bool {
    value.is_finite() && (value == 0.0 || value.abs() >= f64::MIN_POSITIVE)
}

// ---------------------------------------------------------------------------
// What a glyph draws.
// ---------------------------------------------------------------------------

/// Where the reader's pen stands on one axis after a step.
#[derive(Clone, Debug)]
struct Coord {
    /// The reader's coordinate.
    value: f64,
    /// The program's operand the reader added to reach it; `None` where the
    /// step left the axis as it was.
    operand: Option<Operand>,
    /// The reader's coordinate before the step, where it is a value the
    /// program wrote — the side bearing `hsbw` or `sbw` set — so a Type 2 pen
    /// at 0 reaches `value` by adding the two.
    base: Option<Operand>,
}

#[derive(Clone, Debug)]
struct Point {
    x: Coord,
    y: Coord,
}

/// One thing a glyph draws, in order.
#[derive(Clone, Debug)]
enum Event {
    /// The stem set at this index of [`Drawn::sets`] holds from here on.
    Hints(usize),
    Move(Point),
    Line(Point),
    Curve([Point; 3]),
    /// Chapter 8.3's two curves, from the flex's six points after its
    /// reference point, and the flex height.
    Flex {
        points: [Point; 6],
        depth: Operand,
    },
}

/// One stem hint, in the glyph's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stem {
    horizontal: bool,
    edge: f64,
    width: f64,
}

/// A `seac`'s components and the accent's displacement.
#[derive(Clone, Copy, Debug)]
struct Seac {
    base: u8,
    accent: u8,
    adx: f64,
    ady: f64,
}

/// One glyph as the reader draws it.
#[derive(Clone, Debug)]
struct Drawn {
    /// The advance, as the program wrote it.
    width: Operand,
    events: Vec<Event>,
    /// The stem sets: the first holds from the start, each hint replacement
    /// starts another.
    sets: Vec<Vec<Stem>>,
}

/// Draws one glyph as the reader does: its own charstring, or — for a
/// `seac` — its two components, each its own charstring and never composed
/// again, the accent moved by `(adx, ady)`.
fn draw(
    font: &Type1<'_>,
    defined: &BTreeSet<usize>,
    glyph: usize,
    work: &mut u64,
) -> Result<Drawn, Refusal> {
    let (drawn, seac) = run(font, defined, glyph, work)?;
    let Some(seac) = seac else {
        return Ok(drawn);
    };
    let component = |code: u8, work: &mut u64| -> Result<Drawn, Refusal> {
        let name = base_glyph_name(BaseEncoding::Standard, code).ok_or(Refusal::Accent)?;
        let at = font
            .glyph_for_name(name.as_bytes())
            .ok_or(Refusal::Accent)?;
        Ok(run(font, defined, usize::from(at), work)?.0)
    };
    let base = component(seac.base, work)?;
    let accent = component(seac.accent, work)?;

    let offset = base.sets.len();
    let mut events = base.events;
    events.push(Event::Hints(offset));
    events.extend(accent.events.into_iter().map(|event| match event {
        Event::Hints(set) => Event::Hints(set + offset),
        Event::Move(p) => Event::Move(shift(p, seac)),
        Event::Line(p) => Event::Line(shift(p, seac)),
        Event::Curve(points) => Event::Curve(points.map(|p| shift(p, seac))),
        Event::Flex { points, depth } => Event::Flex {
            points: points.map(|p| shift(p, seac)),
            depth,
        },
    }));
    let mut sets = base.sets;
    sets.extend(accent.sets.into_iter().map(|set| {
        set.into_iter()
            .map(|stem| Stem {
                edge: stem.edge + if stem.horizontal { seac.ady } else { seac.adx },
                ..stem
            })
            .collect()
    }));
    Ok(Drawn {
        width: drawn.width,
        events,
        sets,
    })
}

/// An accent's point moved as the reader moves it, `x + adx` after the
/// accent is drawn.
fn shift(point: Point, seac: Seac) -> Point {
    let moved = |coord: Coord, by: f64| Coord {
        value: coord.value + by,
        operand: coord.operand,
        base: None,
    };
    Point {
        x: moved(point.x, seac.adx),
        y: moved(point.y, seac.ady),
    }
}

/// Runs one charstring on the machine, from its start at depth 0, as the
/// reader's `outline` and `advance` do.
fn run(
    font: &Type1<'_>,
    defined: &BTreeSet<usize>,
    glyph: usize,
    work: &mut u64,
) -> Result<(Drawn, Option<Seac>), Refusal> {
    let code = font.charstring(glyph).ok_or(Refusal::Unreadable)?;
    let mut machine = Machine {
        font,
        defined,
        stack: Vec::new(),
        ps: Vec::new(),
        x: 0.0,
        y: 0.0,
        open: false,
        width: Operand::zero(),
        lsb: (0.0, 0.0),
        base_x: None,
        in_flex: false,
        flex: Vec::new(),
        ops: 0,
        seac: None,
        events: Vec::new(),
        sets: vec![Vec::new()],
        current: 0,
        hint_work: 0,
        refusal: None,
    };
    machine.execute(code, 0);
    if machine.in_flex {
        machine.refuse(Refusal::Flex);
    }
    *work = work
        .saturating_add(u64::from(machine.ops))
        .saturating_add(machine.hint_work);
    if *work > WORK_BUDGET {
        return Err(Refusal::Budget);
    }
    if let Some(refusal) = machine.refusal {
        return Err(refusal);
    }
    Ok((
        Drawn {
            width: machine.width,
            events: machine.events,
            sets: machine.sets,
        },
        machine.seac,
    ))
}

/// The reader's Type 1 machine (`type1.rs`'s `Run`), recording what it
/// draws instead of drawing it.
struct Machine<'f, 'a> {
    font: &'f Type1<'a>,
    /// The subroutine indices the program defines.
    defined: &'f BTreeSet<usize>,
    stack: Vec<Operand>,
    /// The PostScript stack `callothersubr` and `pop` talk over.
    ps: Vec<Operand>,
    x: f64,
    y: f64,
    /// Whether a path is open: a move opens one, `closepath` closes it.
    open: bool,
    width: Operand,
    /// The left side bearing point, which stems are relative to (chapter 6).
    lsb: (f64, f64),
    /// The side bearing `hsbw` or `sbw` set, which the next step on x starts
    /// from.
    base_x: Option<Operand>,
    in_flex: bool,
    /// The points a flex has collected.
    flex: Vec<Point>,
    ops: u32,
    seac: Option<Seac>,
    events: Vec<Event>,
    sets: Vec<Vec<Stem>>,
    /// The stem set now holding.
    current: usize,
    /// The steps hint-replacement subroutines took, which the reader never
    /// runs.
    hint_work: u64,
    refusal: Option<Refusal>,
}

impl Machine<'_, '_> {
    /// Stops the run, keeping the first refusal.
    fn refuse(&mut self, why: Refusal) {
        self.refusal.get_or_insert(why);
    }

    /// The top operand, or the 0 the reader takes for none; one either
    /// protocol handed `pop` is refused.
    fn pop(&mut self) -> Operand {
        let operand = self.stack.pop().unwrap_or_else(Operand::zero);
        if operand.is_protocol() {
            self.refuse(Refusal::Protocol);
        }
        operand
    }

    /// The top operand without taking it, as the reader's `last()`.
    fn last(&mut self) -> Operand {
        let operand = self.stack.last().cloned().unwrap_or_else(Operand::zero);
        if operand.is_protocol() {
            self.refuse(Refusal::Protocol);
        }
        operand
    }

    /// The last `n` operands, oldest first, padded in front with zeros, as
    /// the reader's `take`.
    fn take(&mut self, n: usize) -> Vec<Operand> {
        let at = self.stack.len().saturating_sub(n);
        let mut out: Vec<Operand> = self.stack[at..].to_vec();
        while out.len() < n {
            out.insert(0, Operand::zero());
        }
        if out.iter().any(Operand::is_protocol) {
            self.refuse(Refusal::Protocol);
        }
        out
    }

    /// The step on x by `operand`, from where the reader's pen stands.
    fn step_x(&mut self, from: f64, operand: Option<Operand>) -> Coord {
        let value = operand.as_ref().map_or(from, |op| from + op.value);
        Coord {
            value,
            operand,
            base: self.base_x.take(),
        }
    }

    fn step_y(from: f64, operand: Option<Operand>) -> Coord {
        let value = operand.as_ref().map_or(from, |op| from + op.value);
        Coord {
            value,
            operand,
            base: None,
        }
    }

    fn move_to(&mut self, point: Point) {
        self.x = point.x.value;
        self.y = point.y.value;
        if self.in_flex {
            // Between othersubrs 1 and 0 a move is a flex point.
            self.flex.push(point);
            return;
        }
        self.events.push(Event::Move(point));
        self.open = true;
    }

    fn line_to(&mut self, point: Point) {
        if !self.open {
            self.refuse(Refusal::Path);
            return;
        }
        self.x = point.x.value;
        self.y = point.y.value;
        self.events.push(Event::Line(point));
    }

    fn curve_to(&mut self, points: [Point; 3]) {
        if !self.open {
            self.refuse(Refusal::Path);
            return;
        }
        self.x = points[2].x.value;
        self.y = points[2].y.value;
        self.events.push(Event::Curve(points));
    }

    /// Records a stem in the set holding now.
    fn stem(&mut self, horizontal: bool, edge: f64, width: f64) {
        let base = if horizontal { self.lsb.1 } else { self.lsb.0 };
        if let Some(set) = self.sets.get_mut(self.current) {
            set.push(Stem {
                horizontal,
                edge: base + edge,
                width,
            });
        }
    }

    /// `y dy hstem` or `x dx vstem`, or three of either: the top `pairs`
    /// pairs, relative to the left side bearing point. A hint with fewer
    /// operands than it takes declares nothing.
    fn stems(&mut self, horizontal: bool, pairs: usize) {
        if self.stack.len() >= pairs * 2 {
            let values = self.take(pairs * 2);
            for pair in values.chunks_exact(2) {
                self.stem(horizontal, pair[0].value, pair[1].value);
            }
        }
        self.stack.clear();
    }

    fn execute(&mut self, code: &[u8], depth: u32) {
        if self.refusal.is_some() {
            return;
        }
        if depth > MAX_DEPTH {
            self.refuse(Refusal::SubroutineDepth);
            return;
        }
        let mut i = 0usize;
        while i < code.len() {
            if self.refusal.is_some() {
                return;
            }
            self.ops += 1;
            if self.ops > MAX_OPS {
                self.refuse(Refusal::Budget);
                return;
            }
            let b = code[i];
            i += 1;

            // The reader's number forms (chapter 6.2): 255 is a 32-bit
            // integer, a byte the charstring lacks read as 0.
            if b >= 32 {
                let value = if b <= 246 {
                    f64::from(i16::from(b) - 139)
                } else if b <= 250 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from((i16::from(b) - 247) * 256 + i16::from(w) + 108)
                } else if b <= 254 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from(-((i16::from(b) - 251) * 256) - i16::from(w) - 108)
                } else {
                    let mut v = 0i32;
                    for _ in 0..4 {
                        v = (v << 8) | i32::from(*code.get(i).unwrap_or(&0));
                        i += 1;
                    }
                    f64::from(v)
                };
                if self.stack.len() < MAX_OPERANDS {
                    self.stack.push(Operand::number(value));
                } else {
                    self.refuse(Refusal::StackOverflow);
                }
                continue;
            }

            // Inside a flex the protocol moves, calls and computes, and does
            // nothing else.
            if self.in_flex && !matches!(b, 4 | 10 | 11 | 12 | 21 | 22) {
                self.refuse(Refusal::Flex);
                return;
            }

            match b {
                1 => self.stems(true, 1),
                3 => self.stems(false, 1),
                4 => {
                    // vmoveto
                    let dy = self.last();
                    let (x, y) = (self.x, self.y);
                    let point = Point {
                        x: self.step_x(x, None),
                        y: Self::step_y(y, Some(dy)),
                    };
                    self.move_to(point);
                    self.stack.clear();
                }
                5 => {
                    // rlineto
                    let dy = self.pop();
                    let dx = self.pop();
                    let (x, y) = (self.x, self.y);
                    let point = Point {
                        x: self.step_x(x, Some(dx)),
                        y: Self::step_y(y, Some(dy)),
                    };
                    self.line_to(point);
                    self.stack.clear();
                }
                6 => {
                    // hlineto
                    let dx = self.last();
                    let (x, y) = (self.x, self.y);
                    let point = Point {
                        x: self.step_x(x, Some(dx)),
                        y: Self::step_y(y, None),
                    };
                    self.line_to(point);
                    self.stack.clear();
                }
                7 => {
                    // vlineto
                    let dy = self.last();
                    let (x, y) = (self.x, self.y);
                    let point = Point {
                        x: self.step_x(x, None),
                        y: Self::step_y(y, Some(dy)),
                    };
                    self.line_to(point);
                    self.stack.clear();
                }
                8 => {
                    // rrcurveto
                    let v = self.take(6);
                    let (x, y) = (self.x, self.y);
                    let c1 = Point {
                        x: self.step_x(x, Some(v[0].clone())),
                        y: Self::step_y(y, Some(v[1].clone())),
                    };
                    let c2 = Point {
                        x: Self::step_y(c1.x.value, Some(v[2].clone())),
                        y: Self::step_y(c1.y.value, Some(v[3].clone())),
                    };
                    let end = Point {
                        x: Self::step_y(c2.x.value, Some(v[4].clone())),
                        y: Self::step_y(c2.y.value, Some(v[5].clone())),
                    };
                    self.curve_to([c1, c2, end]);
                    self.stack.clear();
                }
                9 => {
                    // closepath: Type 2 closes at the next move and at
                    // `endchar`, so it is written as nothing.
                    self.open = false;
                    self.stack.clear();
                }
                10 => {
                    // callsubr
                    let index = self.stack.pop().unwrap_or_else(Operand::zero);
                    if matches!(index.kind, Kind::FlexX | Kind::FlexY) {
                        self.refuse(Refusal::Protocol);
                        return;
                    }
                    let target = usize::try_from(index.value as i64).ok();
                    let font = self.font;
                    match target.filter(|t| self.defined.contains(t)) {
                        Some(target) => {
                            let Some(subr) = font.subr(target) else {
                                self.refuse(Refusal::MissingSubroutine);
                                return;
                            };
                            self.execute(subr, depth + 1);
                        }
                        // A call past the array, or into a hole of it, which
                        // an interpreter reading the array as written fails.
                        None => self.refuse(Refusal::MissingSubroutine),
                    }
                }
                // return, and endchar, which the reader also ends a
                // subroutine with.
                11 | 14 => return,
                13 => {
                    // hsbw: the side bearing is where the pen starts.
                    let width = self.pop();
                    let sbx = self.pop();
                    self.width = width;
                    self.lsb = (sbx.value, 0.0);
                    self.x = sbx.value;
                    self.y = 0.0;
                    self.base_x = Some(sbx);
                    self.stack.clear();
                }
                21 => {
                    // rmoveto
                    let dy = self.pop();
                    let dx = self.pop();
                    let (x, y) = (self.x, self.y);
                    let point = Point {
                        x: self.step_x(x, Some(dx)),
                        y: Self::step_y(y, Some(dy)),
                    };
                    self.move_to(point);
                    self.stack.clear();
                }
                22 => {
                    // hmoveto
                    let dx = self.last();
                    let (x, y) = (self.x, self.y);
                    let point = Point {
                        x: self.step_x(x, Some(dx)),
                        y: Self::step_y(y, None),
                    };
                    self.move_to(point);
                    self.stack.clear();
                }
                30 => {
                    // vhcurveto
                    let v = self.take(4);
                    let (x, y) = (self.x, self.y);
                    let c1 = Point {
                        x: self.step_x(x, None),
                        y: Self::step_y(y, Some(v[0].clone())),
                    };
                    let c2 = Point {
                        x: Self::step_y(c1.x.value, Some(v[1].clone())),
                        y: Self::step_y(c1.y.value, Some(v[2].clone())),
                    };
                    let end = Point {
                        x: Self::step_y(c2.x.value, Some(v[3].clone())),
                        y: Self::step_y(c2.y.value, None),
                    };
                    self.curve_to([c1, c2, end]);
                    self.stack.clear();
                }
                31 => {
                    // hvcurveto
                    let v = self.take(4);
                    let (x, y) = (self.x, self.y);
                    let c1 = Point {
                        x: self.step_x(x, Some(v[0].clone())),
                        y: Self::step_y(y, None),
                    };
                    let c2 = Point {
                        x: Self::step_y(c1.x.value, Some(v[1].clone())),
                        y: Self::step_y(c1.y.value, Some(v[2].clone())),
                    };
                    let end = Point {
                        x: Self::step_y(c2.x.value, None),
                        y: Self::step_y(c2.y.value, Some(v[3].clone())),
                    };
                    self.curve_to([c1, c2, end]);
                    self.stack.clear();
                }
                12 => {
                    let b2 = *code.get(i).unwrap_or(&0);
                    i += 1;
                    if self.in_flex && !matches!(b2, 12 | 16) {
                        self.refuse(Refusal::Flex);
                        return;
                    }
                    self.escape(b2);
                }
                _ => self.stack.clear(),
            }
        }
    }

    /// The two-byte operators.
    fn escape(&mut self, op: u8) {
        match op {
            // dotsection: no Type 2 counterpart, and nothing drawn.
            0 => self.stack.clear(),
            1 => self.stems(false, 3),
            2 => self.stems(true, 3),
            6 => {
                // seac: drawn once the charstring is done (`draw`).
                let v = self.take(5);
                self.seac = Some(Seac {
                    base: v[3].value as u8,
                    accent: v[4].value as u8,
                    adx: v[1].value,
                    ady: v[2].value,
                });
                self.stack.clear();
            }
            7 => {
                // sbw
                let v = self.take(4);
                if v[1].value != 0.0 || v[3].value != 0.0 {
                    self.refuse(Refusal::VerticalMetrics);
                    return;
                }
                self.lsb = (v[0].value, v[1].value);
                self.width = v[2].clone();
                self.x = v[0].value;
                self.y = v[1].value;
                self.base_x = Some(v[0].clone());
                self.stack.clear();
            }
            12 => self.divide(),
            16 => self.othersubr(),
            17 => {
                // pop: only what a protocol left.
                match self.ps.pop() {
                    Some(value) => self.stack.push(value),
                    None => self.refuse(Refusal::Protocol),
                }
            }
            33 => {
                // setcurrentpoint: chapter 8.3 sets the pen to where the
                // flex left it, which is where it stands.
                let n = self.stack.len();
                let flexed = n >= 2
                    && self.stack[n - 2].kind == Kind::FlexX
                    && self.stack[n - 1].kind == Kind::FlexY
                    && self.stack[n - 2].value == self.x
                    && self.stack[n - 1].value == self.y;
                if !flexed {
                    self.refuse(Refusal::Protocol);
                    return;
                }
                self.stack.clear();
            }
            _ => self.stack.clear(),
        }
    }

    /// `div`, written as Type 2's where both operands are written, and as its
    /// quotient where an operand is not and one Type 2 operand holds that.
    fn divide(&mut self) {
        let b = self.stack.pop().unwrap_or_else(|| Operand::number(1.0));
        let a = self.stack.pop().unwrap_or_else(Operand::zero);
        if a.is_protocol() || b.is_protocol() {
            self.refuse(Refusal::Protocol);
            return;
        }
        let value = if b.value == 0.0 {
            0.0
        } else {
            a.value / b.value
        };
        let kind = match (a.code(), b.code()) {
            _ if !settled(value) => Kind::Unwritable,
            (Some((ab, ad)), Some((bb, bd))) => {
                let mut bytes = Vec::with_capacity(ab.len() + bb.len() + 2);
                bytes.extend_from_slice(ab);
                bytes.extend_from_slice(bb);
                bytes.extend_from_slice(&[12, 12]);
                Kind::Code {
                    bytes,
                    depth: ad.max(1 + bd),
                }
            }
            _ => Operand::number(value).kind,
        };
        self.stack.push(Operand { value, kind });
    }

    /// `callothersubr`: flex's three entries and hint replacement's
    /// (chapter 8); any other is a PostScript procedure, refused.
    fn othersubr(&mut self) {
        let index = self.stack.pop().unwrap_or_else(Operand::zero);
        let count = self.stack.pop().unwrap_or_else(Operand::zero);
        if index.is_protocol() || count.is_protocol() {
            self.refuse(Refusal::Protocol);
            return;
        }
        let index = index.value as i64;
        let count = count.value.max(0.0) as usize;
        let at = self.stack.len().saturating_sub(count);
        let args = self.stack.split_off(at);
        if args.iter().any(Operand::is_protocol) {
            self.refuse(Refusal::Protocol);
            return;
        }
        match index {
            0 => {
                // The flex's end: its reference point and six more, from the
                // pen the flex started at, which must be on an open path.
                if !self.in_flex || self.flex.len() != 7 || args.len() != 3 || !self.open {
                    self.refuse(Refusal::Flex);
                    return;
                }
                let collected: Vec<Point> = core::mem::take(&mut self.flex)
                    .into_iter()
                    .skip(1)
                    .collect();
                let Ok(points) = <[Point; 6]>::try_from(collected) else {
                    self.refuse(Refusal::Flex);
                    return;
                };
                self.in_flex = false;
                self.x = points[5].x.value;
                self.y = points[5].y.value;
                self.events.push(Event::Flex {
                    points,
                    depth: args[0].clone(),
                });
                // The two values `pop` asks for: the flex's last point.
                self.ps.push(Operand {
                    value: self.y,
                    kind: Kind::FlexY,
                });
                self.ps.push(Operand {
                    value: self.x,
                    kind: Kind::FlexX,
                });
            }
            1 => {
                if self.in_flex {
                    self.refuse(Refusal::Flex);
                    return;
                }
                self.in_flex = true;
                self.flex.clear();
            }
            2 => {}
            3 => {
                // Hint replacement: `subr# 1 3 callothersubr pop callsubr`.
                // The reader calls subroutine 3; the stems are the named
                // subroutine's.
                if self.in_flex {
                    self.refuse(Refusal::Flex);
                    return;
                }
                let named = args
                    .first()
                    .and_then(|subr| usize::try_from(subr.value as i64).ok())
                    .filter(|subr| self.defined.contains(subr));
                let Some(named) = named else {
                    self.refuse(Refusal::HintReplacement);
                    return;
                };
                let mut stems = Vec::new();
                if let Err(why) = self.hint_subroutine(named, 1, &mut stems) {
                    self.refuse(why);
                    return;
                }
                self.sets.push(stems);
                self.current = self.sets.len() - 1;
                self.events.push(Event::Hints(self.current));
                self.ps.push(Operand {
                    value: 3.0,
                    kind: Kind::Hint,
                });
            }
            _ => self.refuse(Refusal::OtherSubr),
        }
    }

    /// The stems a hint-replacement subroutine declares, as an interpreter
    /// that replaces hints reads them: numbers, `div`, the four stem
    /// operators, `dotsection`, `callsubr` and `return`. Anything else draws
    /// or computes what this reader never ran.
    fn hint_subroutine(
        &mut self,
        index: usize,
        depth: u32,
        stems: &mut Vec<Stem>,
    ) -> Result<(), Refusal> {
        if depth > MAX_DEPTH {
            return Err(Refusal::HintReplacement);
        }
        let font = self.font;
        let code = font.subr(index).ok_or(Refusal::HintReplacement)?;
        let mut stack: Vec<f64> = Vec::new();
        let mut i = 0usize;
        let lsb = self.lsb;
        while i < code.len() {
            self.hint_work += 1;
            if self.hint_work > WORK_BUDGET {
                return Err(Refusal::Budget);
            }
            let b = code[i];
            i += 1;
            if b >= 32 {
                let value = if b <= 246 {
                    f64::from(i16::from(b) - 139)
                } else if b <= 250 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from((i16::from(b) - 247) * 256 + i16::from(w) + 108)
                } else if b <= 254 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from(-((i16::from(b) - 251) * 256) - i16::from(w) - 108)
                } else {
                    let mut v = 0i32;
                    for _ in 0..4 {
                        v = (v << 8) | i32::from(*code.get(i).unwrap_or(&0));
                        i += 1;
                    }
                    f64::from(v)
                };
                if stack.len() >= MAX_OPERANDS {
                    return Err(Refusal::HintReplacement);
                }
                stack.push(value);
                continue;
            }
            match b {
                1 => declare(&mut stack, stems, true, 1, lsb),
                3 => declare(&mut stack, stems, false, 1, lsb),
                10 => {
                    let target = stack
                        .pop()
                        .and_then(|v| usize::try_from(v as i64).ok())
                        .filter(|t| self.defined.contains(t))
                        .ok_or(Refusal::HintReplacement)?;
                    // The nested subroutine's stems join this one's.
                    let mut nested = Vec::new();
                    self.hint_subroutine(target, depth + 1, &mut nested)?;
                    stems.extend(nested);
                }
                11 => return Ok(()),
                12 => {
                    let b2 = *code.get(i).unwrap_or(&0);
                    i += 1;
                    match b2 {
                        0 => stack.clear(),
                        1 => declare(&mut stack, stems, false, 3, lsb),
                        2 => declare(&mut stack, stems, true, 3, lsb),
                        12 => {
                            let divisor = stack.pop().unwrap_or(1.0);
                            let dividend = stack.pop().unwrap_or(0.0);
                            stack.push(if divisor == 0.0 {
                                0.0
                            } else {
                                dividend / divisor
                            });
                        }
                        _ => return Err(Refusal::HintReplacement),
                    }
                }
                _ => return Err(Refusal::HintReplacement),
            }
        }
        Ok(())
    }
}

/// A hint-replacement subroutine's `hstem` or `vstem`, or three of either:
/// the top `pairs` pairs of `stack`, relative to the left side bearing point
/// `lsb`.
fn declare(
    stack: &mut Vec<f64>,
    stems: &mut Vec<Stem>,
    horizontal: bool,
    pairs: usize,
    lsb: (f64, f64),
) {
    if stack.len() >= pairs * 2 {
        let at = stack.len() - pairs * 2;
        let base = if horizontal { lsb.1 } else { lsb.0 };
        for pair in stack[at..].chunks_exact(2) {
            stems.push(Stem {
                horizontal,
                edge: base + pair[0],
                width: pair[1],
            });
        }
    }
    stack.clear();
}

// ---------------------------------------------------------------------------
// Writing a Type 2 charstring.
// ---------------------------------------------------------------------------

/// The stems of `set` in declaration order — horizontal before vertical,
/// each by edge and then width — once each.
fn normalise(mut set: Vec<Stem>) -> Vec<Stem> {
    set.sort_by(|a, b| {
        b.horizontal
            .cmp(&a.horizontal)
            .then(a.edge.total_cmp(&b.edge))
            .then(a.width.total_cmp(&b.width))
    });
    set.dedup();
    set
}

/// The Type 2 operand a stem value is written as: the value at 16.16, which
/// is what a Type 2 operand holds. Stems guide a rasteriser and are no part
/// of the outline either reader draws.
fn hint_operand(value: f64) -> Result<Operand, Refusal> {
    let rounded = round_to_fixed(value);
    let operand = Operand::number(rounded);
    match operand.kind {
        Kind::Code { .. } => Ok(operand),
        _ => Err(Refusal::Number),
    }
}

/// A run of drawing operators one Type 2 operator carries.
enum Pending {
    None,
    /// `rlineto`'s pairs.
    Lines(Vec<Operand>),
    /// `hlineto` or `vlineto`, alternating: the first one's axis, the
    /// operands, and the axis the next one would take.
    Alternating {
        horizontal: bool,
        operands: Vec<Operand>,
        next: bool,
    },
    /// `rrcurveto`'s sixes.
    Curves(Vec<Operand>),
    /// `hvcurveto` or `vhcurveto`, alternating.
    Bends {
        horizontal: bool,
        operands: Vec<Operand>,
        next: bool,
    },
}

/// The deepest the Type 2 stack goes while `operands` are pushed in order.
fn peak<'o>(operands: impl IntoIterator<Item = &'o Operand>) -> Option<usize> {
    let mut deepest = 0usize;
    for (at, operand) in operands.into_iter().enumerate() {
        let (_, depth) = operand.code()?;
        deepest = deepest.max(at + depth);
    }
    Some(deepest)
}

/// Writes one glyph's Type 2 charstring.
struct Writer {
    out: Vec<u8>,
    /// The width, until the first stack-clearing operator takes it; `None`
    /// for a width of 0, `defaultWidthX`.
    width: Option<Operand>,
    /// Type 2's pen.
    x: f64,
    y: f64,
    pending: Pending,
    /// Each stem set's `hintmask` bytes, where the glyph's sets differ.
    masks: Option<Vec<Vec<u8>>>,
    /// The set the glyph has asked for, and the mask last written.
    wanted: usize,
    active: Option<usize>,
}

fn write_glyph(drawn: &Drawn) -> Result<Vec<u8>, Refusal> {
    let width = if drawn.width.value == 0.0 {
        None
    } else if drawn.width.code().is_some() {
        Some(drawn.width.clone())
    } else {
        let written = Operand::number(drawn.width.value);
        written.code().ok_or(Refusal::Number)?;
        Some(written)
    };
    let mut writer = Writer {
        out: Vec::new(),
        width,
        x: 0.0,
        y: 0.0,
        pending: Pending::None,
        masks: None,
        wanted: 0,
        active: None,
    };

    let sets: Vec<Vec<Stem>> = drawn.sets.iter().map(|s| normalise(s.clone())).collect();
    let all = normalise(sets.iter().flatten().copied().collect());
    if all.len() > MAX_STEMS {
        return Err(Refusal::TooManyStems);
    }
    let masked = sets.iter().any(|set| Some(set) != sets.first());
    let (horizontal, vertical): (Vec<Stem>, Vec<Stem>) =
        all.iter().copied().partition(|stem| stem.horizontal);
    writer.stems(&horizontal, if masked { 18 } else { 1 })?;
    writer.stems(&vertical, if masked { 23 } else { 3 })?;
    if masked {
        let bytes = all.len().div_ceil(8);
        writer.masks = Some(
            sets.iter()
                .map(|set| {
                    let mut mask = vec![0u8; bytes];
                    for (at, stem) in all.iter().enumerate() {
                        if set.contains(stem) {
                            mask[at / 8] |= 0x80 >> (at % 8);
                        }
                    }
                    mask
                })
                .collect(),
        );
    }

    for event in &drawn.events {
        writer.event(event)?;
    }
    writer.flush()?;
    writer.operator(&[], &[14], true)?;
    Ok(writer.out)
}

impl Writer {
    /// Writes one operator and its operands, the width first where it is the
    /// first stack-clearing one.
    fn operator(&mut self, operands: &[Operand], op: &[u8], clears: bool) -> Result<(), Refusal> {
        let width = if clears { self.width.take() } else { None };
        let deepest = peak(width.iter().chain(operands)).ok_or(Refusal::Number)?;
        if deepest > MAX_OPERANDS {
            return Err(Refusal::StackOverflow);
        }
        for operand in width.iter().chain(operands) {
            if let Some((bytes, _)) = operand.code() {
                self.out.extend_from_slice(bytes);
            }
        }
        self.out.extend_from_slice(op);
        if self.out.len() > MAX_CHARSTRING {
            return Err(Refusal::TooLarge);
        }
        Ok(())
    }

    /// Declares `stems` with `op`, in operators of as many pairs as the stack
    /// holds, each operator's first edge from 0 and every later one from the
    /// top of the stem before it.
    fn stems(&mut self, stems: &[Stem], op: u8) -> Result<(), Refusal> {
        let mut at = 0usize;
        while at < stems.len() {
            let room = (MAX_OPERANDS - usize::from(self.width.is_some())) / 2;
            let chunk = &stems[at..stems.len().min(at + room)];
            let mut operands = Vec::with_capacity(chunk.len() * 2);
            let mut top = 0.0f64;
            for stem in chunk {
                let edge = round_to_fixed(stem.edge);
                let width = round_to_fixed(stem.width);
                operands.push(hint_operand(edge - top)?);
                operands.push(hint_operand(width)?);
                top = edge + width;
            }
            self.operator(&operands, &[op], true)?;
            at += chunk.len();
        }
        Ok(())
    }

    /// Writes what is pending as its operator.
    fn flush(&mut self) -> Result<(), Refusal> {
        let (operands, op) = match core::mem::replace(&mut self.pending, Pending::None) {
            Pending::None => return Ok(()),
            Pending::Lines(operands) => (operands, 5),
            Pending::Alternating {
                horizontal,
                operands,
                ..
            } => (operands, if horizontal { 6 } else { 7 }),
            Pending::Curves(operands) => (operands, 8),
            Pending::Bends {
                horizontal,
                operands,
                ..
            } => (operands, if horizontal { 31 } else { 30 }),
        };
        self.operator(&operands, &[op], false)
    }

    /// Writes the `hintmask` the glyph has asked for, before a drawing
    /// operator, where it differs from the one in force.
    fn mask(&mut self) -> Result<(), Refusal> {
        let Some(masks) = &self.masks else {
            return Ok(());
        };
        let wanted = masks.get(self.wanted).ok_or(Refusal::Mismatch)?.clone();
        if let Some(active) = self.active {
            if masks.get(active) == Some(&wanted) {
                self.active = Some(self.wanted);
                return Ok(());
            }
        }
        self.flush()?;
        self.out.push(19);
        self.out.extend_from_slice(&wanted);
        if self.out.len() > MAX_CHARSTRING {
            return Err(Refusal::TooLarge);
        }
        self.active = Some(self.wanted);
        Ok(())
    }

    /// Appends one drawing operator's operands to what is pending, or starts
    /// it anew: lines with lines, curves with curves, and an alternating run
    /// where the axis alternates, within the stack.
    fn segment(&mut self, kind: Shape, operands: Vec<Operand>) -> Result<(), Refusal> {
        let joined = match (&mut self.pending, kind) {
            (Pending::Lines(have), Shape::Line) | (Pending::Curves(have), Shape::Curve) => {
                fits(have, &operands).then(|| have.extend(operands.iter().cloned()))
            }
            (
                Pending::Alternating {
                    operands: have,
                    next,
                    ..
                },
                Shape::Straight(horizontal),
            )
            | (
                Pending::Bends {
                    operands: have,
                    next,
                    ..
                },
                Shape::Bend(horizontal),
            ) if *next == horizontal => fits(have, &operands).then(|| {
                have.extend(operands.iter().cloned());
                *next = !horizontal;
            }),
            _ => None,
        };
        if joined.is_some() {
            return Ok(());
        }
        self.flush()?;
        self.pending = match kind {
            Shape::Line => Pending::Lines(operands),
            Shape::Curve => Pending::Curves(operands),
            Shape::Straight(horizontal) => Pending::Alternating {
                horizontal,
                operands,
                next: !horizontal,
            },
            Shape::Bend(horizontal) => Pending::Bends {
                horizontal,
                operands,
                next: !horizontal,
            },
        };
        Ok(())
    }

    fn event(&mut self, event: &Event) -> Result<(), Refusal> {
        match event {
            Event::Hints(set) => {
                self.wanted = *set;
                Ok(())
            }
            Event::Move(point) => {
                self.flush()?;
                self.mask()?;
                let dx = delta(self.x, &point.x)?;
                let dy = delta(self.y, &point.y)?;
                if dy.value == 0.0 {
                    self.operator(&[dx], &[22], true)?;
                } else if dx.value == 0.0 {
                    self.operator(&[dy], &[4], true)?;
                } else {
                    self.operator(&[dx, dy], &[21], true)?;
                }
                self.x = point.x.value;
                self.y = point.y.value;
                Ok(())
            }
            Event::Line(point) => {
                self.mask()?;
                let dx = delta(self.x, &point.x)?;
                let dy = delta(self.y, &point.y)?;
                self.x = point.x.value;
                self.y = point.y.value;
                if dy.value == 0.0 {
                    self.segment(Shape::Straight(true), vec![dx])
                } else if dx.value == 0.0 {
                    self.segment(Shape::Straight(false), vec![dy])
                } else {
                    self.segment(Shape::Line, vec![dx, dy])
                }
            }
            Event::Curve([c1, c2, end]) => {
                self.mask()?;
                let d1x = delta(self.x, &c1.x)?;
                let d1y = delta(self.y, &c1.y)?;
                let d2x = delta(c1.x.value, &c2.x)?;
                let d2y = delta(c1.y.value, &c2.y)?;
                let d3x = delta(c2.x.value, &end.x)?;
                let d3y = delta(c2.y.value, &end.y)?;
                self.x = end.x.value;
                self.y = end.y.value;
                if d1y.value == 0.0 && d3x.value == 0.0 {
                    self.segment(Shape::Bend(true), vec![d1x, d2x, d2y, d3y])
                } else if d1x.value == 0.0 && d3y.value == 0.0 {
                    self.segment(Shape::Bend(false), vec![d1y, d2x, d2y, d3x])
                } else {
                    self.segment(Shape::Curve, vec![d1x, d1y, d2x, d2y, d3x, d3y])
                }
            }
            Event::Flex { points, depth } => {
                self.flush()?;
                self.mask()?;
                let mut operands = Vec::with_capacity(13);
                let (mut x, mut y) = (self.x, self.y);
                for point in points {
                    operands.push(delta(x, &point.x)?);
                    operands.push(delta(y, &point.y)?);
                    x = point.x.value;
                    y = point.y.value;
                }
                operands.push(match depth.code() {
                    Some(_) => depth.clone(),
                    None => {
                        let written = Operand::number(depth.value);
                        written.code().ok_or(Refusal::Number)?;
                        written
                    }
                });
                self.operator(&operands, &[12, 35], false)?;
                self.x = x;
                self.y = y;
                Ok(())
            }
        }
    }
}

/// Which operator family one drawn segment takes.
#[derive(Clone, Copy)]
enum Shape {
    Line,
    Curve,
    /// `hlineto` (true) or `vlineto`.
    Straight(bool),
    /// `hvcurveto` (true) or `vhcurveto`.
    Bend(bool),
}

/// Whether `more` joins `have` within the Type 2 stack.
fn fits(have: &[Operand], more: &[Operand]) -> bool {
    peak(have.iter().chain(more)).is_some_and(|deepest| deepest <= MAX_OPERANDS)
}

/// The operand that moves Type 2's pen from `current` to the coordinate the
/// reader reached: the program's own, where adding it lands there; the exact
/// difference, where one Type 2 operand holds it; or, from a pen at 0, the
/// side bearing and the program's operand added by `add` (12 10).
fn delta(current: f64, coord: &Coord) -> Result<Operand, Refusal> {
    let target = coord.value;
    if let Some(operand) = &coord.operand {
        if operand.code().is_some() && current + operand.value == target {
            return Ok(operand.clone());
        }
    }
    let difference = target - current;
    if current + difference == target {
        if let Some(bytes) = literal(difference) {
            return Ok(Operand {
                value: difference,
                kind: Kind::Code { bytes, depth: 1 },
            });
        }
    }
    if current == 0.0 {
        if let Some(base) = &coord.base {
            if let Some((base_bytes, base_depth)) = base.code() {
                match &coord.operand {
                    None if base.value == target => return Ok(base.clone()),
                    Some(operand) => {
                        if let Some((bytes, depth)) = operand.code() {
                            let sum = base.value + operand.value;
                            if settled(sum) && sum == target {
                                let mut code = base_bytes.to_vec();
                                code.extend_from_slice(bytes);
                                code.extend_from_slice(&[12, 10]);
                                return Ok(Operand {
                                    value: sum,
                                    kind: Kind::Code {
                                        bytes: code,
                                        depth: base_depth.max(1 + depth),
                                    },
                                });
                            }
                        }
                    }
                    None => {}
                }
            }
        }
    }
    Err(Refusal::Number)
}

// ---------------------------------------------------------------------------
// What the program says about itself.
// ---------------------------------------------------------------------------

/// One PostScript token of the clear text or the private section.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    /// `/name`.
    Name(Vec<u8>),
    /// An executable name: `def`, `true`, `readonly`.
    Word(Vec<u8>),
    Number(f64),
    /// `(…)` or `<…>`, its bytes.
    String(Vec<u8>),
    /// `[` or `{`.
    Open,
    /// `]` or `}`.
    Close,
}

/// Whether `b` is PostScript white space or a delimiter.
fn delimits(b: u8) -> bool {
    b.is_ascii_whitespace() || b == 0 || b"()<>[]{}/%".contains(&b)
}

/// The tokens of `text`: comments skipped, strings decoded, `<<` and `>>`
/// passed over.
fn tokens(text: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(&b) = text.get(at) {
        match b {
            b'%' => {
                while text.get(at).is_some_and(|b| *b != b'\n' && *b != b'\r') {
                    at += 1;
                }
            }
            b'[' | b'{' => {
                out.push(Token::Open);
                at += 1;
            }
            b']' | b'}' => {
                out.push(Token::Close);
                at += 1;
            }
            b'(' => {
                let (string, end) = literal_string(text, at + 1);
                out.push(Token::String(string));
                at = end;
            }
            b'<' if text.get(at + 1) == Some(&b'<') => at += 2,
            b'>' => at += 1,
            b'<' => {
                let mut bytes = Vec::new();
                let mut high: Option<u8> = None;
                at += 1;
                while let Some(&c) = text.get(at) {
                    at += 1;
                    if c == b'>' {
                        break;
                    }
                    let Some(digit) = (c as char).to_digit(16) else {
                        continue;
                    };
                    let digit = digit as u8;
                    match high.take() {
                        Some(h) => bytes.push((h << 4) | digit),
                        None => high = Some(digit),
                    }
                }
                if let Some(h) = high {
                    bytes.push(h << 4);
                }
                out.push(Token::String(bytes));
            }
            b'/' => {
                let start = at + 1;
                let mut end = start;
                while text.get(end).is_some_and(|b| !delimits(*b)) {
                    end += 1;
                }
                out.push(Token::Name(text[start..end].to_vec()));
                at = end;
            }
            _ if delimits(b) => at += 1,
            _ => {
                let start = at;
                while text.get(at).is_some_and(|b| !delimits(*b)) {
                    at += 1;
                }
                let word = &text[start..at];
                out.push(match ps_number(word) {
                    Some(value) => Token::Number(value),
                    None => Token::Word(word.to_vec()),
                });
            }
        }
    }
    out
}

/// A literal string's bytes from just past its `(` (PostScript's escapes and
/// nesting), and where it ends.
fn literal_string(text: &[u8], mut at: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 1usize;
    while let Some(&b) = text.get(at) {
        at += 1;
        match b {
            b'(' => {
                depth += 1;
                out.push(b);
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                out.push(b);
            }
            b'\\' => {
                let Some(&next) = text.get(at) else { break };
                at += 1;
                match next {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'\r' => {
                        if text.get(at) == Some(&b'\n') {
                            at += 1;
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut value = u32::from(next - b'0');
                        for _ in 0..2 {
                            match text.get(at) {
                                Some(&d @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(d - b'0');
                                    at += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push((value & 0xFF) as u8);
                    }
                    other => out.push(other),
                }
            }
            _ => out.push(b),
        }
    }
    (out, at)
}

/// What follows the first `/key` in `tokens`.
fn after<'t>(tokens: &'t [Token], key: &[u8]) -> Option<&'t [Token]> {
    let at = tokens
        .iter()
        .position(|token| matches!(token, Token::Name(name) if name == key))?;
    tokens.get(at + 1..)
}

/// `/key (string)`.
fn string_value(tokens: &[Token], key: &[u8]) -> Option<Vec<u8>> {
    match after(tokens, key)?.first()? {
        Token::String(bytes) => Some(bytes.clone()),
        _ => None,
    }
}

/// `/key number`.
fn number_value(tokens: &[Token], key: &[u8]) -> Option<f64> {
    match after(tokens, key)?.first()? {
        Token::Number(value) => Some(*value),
        _ => None,
    }
}

/// `/key true` or `/key false`.
fn bool_value(tokens: &[Token], key: &[u8]) -> Option<f64> {
    match after(tokens, key)?.first()? {
        Token::Word(word) if word == b"true" => Some(1.0),
        Token::Word(word) if word == b"false" => Some(0.0),
        _ => None,
    }
}

/// `/key [numbers]` or `/key {numbers}`: `None` where anything but a number
/// stands inside.
fn array_value(tokens: &[Token], key: &[u8]) -> Option<Vec<f64>> {
    let rest = after(tokens, key)?;
    if rest.first()? != &Token::Open {
        return None;
    }
    let mut out = Vec::new();
    for token in rest.get(1..)? {
        match token {
            Token::Number(value) => out.push(*value),
            Token::Close => return Some(out),
            _ => return None,
        }
    }
    None
}

/// The program's own facts a CFF carries in its Name INDEX and DICTs.
struct Facts {
    font_name: Vec<u8>,
    bbox: Option<[f64; 4]>,
    /// The Top DICT's strings, by operator.
    strings: Vec<(u16, Vec<u8>)>,
    /// The Top DICT's numbers, by operator.
    numbers: Vec<(u16, f64)>,
    /// The Private DICT, by operator, each array a delta one where 5176
    /// makes it one.
    private: Vec<(u16, Vec<f64>)>,
}

impl Facts {
    fn read(layout: &Layout<'_>) -> Result<Facts, Refusal> {
        let clear = tokens(layout.data.get(..layout.eexec).unwrap_or_default());
        let font_name = match after(&clear, b"FontName").and_then(<[Token]>::first) {
            Some(Token::Name(name)) => name.clone(),
            _ => return Err(Refusal::FontName),
        };
        // 5176 §7: 1 to 127 printable bytes, none of them its reserved ones.
        let valid = (1..=127).contains(&font_name.len())
            && font_name
                .iter()
                .all(|b| (33..=126).contains(b) && !b"[](){}<>/%".contains(b));
        if !valid {
            return Err(Refusal::FontName);
        }
        let bbox = array_value(&clear, b"FontBBox")
            .filter(|values| values.len() == 4)
            .map(|v| [v[0], v[1], v[2], v[3]]);
        let mut strings = Vec::new();
        for (key, op) in [
            (b"version".as_slice(), 0u16),
            (b"Notice", 1),
            (b"Copyright", 0x0C00),
            (b"FullName", 2),
            (b"FamilyName", 3),
            (b"Weight", 4),
        ] {
            if let Some(value) = string_value(&clear, key) {
                strings.push((op, value));
            }
        }
        let mut numbers = Vec::new();
        if let Some(value) = bool_value(&clear, b"isFixedPitch") {
            numbers.push((0x0C01, value));
        }
        for (key, op) in [
            (b"ItalicAngle".as_slice(), 0x0C02u16),
            (b"UnderlinePosition", 0x0C03),
            (b"UnderlineThickness", 0x0C04),
            (b"PaintType", 0x0C05),
            (b"StrokeWidth", 0x0C08),
        ] {
            if let Some(value) = number_value(&clear, key) {
                numbers.push((op, value));
            }
        }

        // The private section's text runs up to its first binary entry.
        let private = &layout.private;
        let cut = [find(private, b"/Subrs"), find(private, b"/CharStrings")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(private.len());
        let text = tokens(private.get(..cut).unwrap_or_default());
        let mut dict = Vec::new();
        // Table 23's pairs of zones and snaps, each within its limit and in
        // pairs where they pair.
        for (key, op, limit, pairs) in [
            (b"BlueValues".as_slice(), 6u16, 14usize, true),
            (b"OtherBlues", 7, 10, true),
            (b"FamilyBlues", 8, 14, true),
            (b"FamilyOtherBlues", 9, 10, true),
            (b"StemSnapH", 0x0C0C, 12, false),
            (b"StemSnapV", 0x0C0D, 12, false),
        ] {
            let Some(values) = array_value(&text, key) else {
                continue;
            };
            if values.is_empty() || values.len() > limit || (pairs && values.len() % 2 == 1) {
                continue;
            }
            let mut deltas = Vec::with_capacity(values.len());
            let mut previous = 0.0;
            for value in values {
                deltas.push(value - previous);
                previous = value;
            }
            dict.push((op, deltas));
        }
        for (key, op) in [(b"StdHW".as_slice(), 10u16), (b"StdVW", 11)] {
            if let Some(value) = array_value(&text, key).and_then(|v| v.first().copied()) {
                dict.push((op, vec![value]));
            }
        }
        for (key, op) in [
            (b"BlueScale".as_slice(), 0x0C09u16),
            (b"BlueShift", 0x0C0A),
            (b"BlueFuzz", 0x0C0B),
            (b"LanguageGroup", 0x0C11),
            (b"ExpansionFactor", 0x0C12),
        ] {
            if let Some(value) = number_value(&text, key) {
                dict.push((op, vec![value]));
            }
        }
        if let Some(value) = bool_value(&text, b"ForceBold") {
            dict.push((0x0C0E, vec![value]));
        }
        Ok(Facts {
            font_name,
            bbox,
            strings,
            numbers,
            private: dict,
        })
    }
}

// ---------------------------------------------------------------------------
// The CFF.
// ---------------------------------------------------------------------------

/// The CFF's strings: the standard ones by their SIDs, every other in the
/// String INDEX after them.
struct Strings {
    custom: Vec<Vec<u8>>,
    at: BTreeMap<Vec<u8>, u16>,
}

impl Strings {
    fn sid(&mut self, name: &[u8]) -> Result<u16, Refusal> {
        if let Some(sid) = core::str::from_utf8(name)
            .ok()
            .and_then(|name| STANDARD_STRINGS.iter().position(|s| *s == name))
        {
            return u16::try_from(sid).map_err(|_| Refusal::TooManyGlyphs);
        }
        if let Some(sid) = self.at.get(name) {
            return Ok(*sid);
        }
        let sid = u16::try_from(STANDARD_STRINGS.len() + self.custom.len())
            .map_err(|_| Refusal::TooManyGlyphs)?;
        self.custom.push(name.to_vec());
        self.at.insert(name.to_vec(), sid);
        Ok(sid)
    }
}

/// A charset of `sids` for glyphs 1 on: the predefined ISOAdobe one where it
/// is that, otherwise the smallest of 5176's three formats.
fn charset(sids: &[u16]) -> Option<Vec<u8>> {
    let iso = sids.len() <= 228
        && sids
            .iter()
            .enumerate()
            .all(|(at, sid)| usize::from(*sid) == at + 1);
    if iso {
        return None;
    }
    let mut zero = vec![0u8];
    for sid in sids {
        zero.extend_from_slice(&sid.to_be_bytes());
    }
    let ranges = |wide: bool| {
        let mut out = vec![if wide { 2u8 } else { 1u8 }];
        let limit = if wide { u16::MAX } else { 255 };
        let mut at = 0usize;
        while let Some(&first) = sids.get(at) {
            let mut left = 0u16;
            while left < limit
                && sids
                    .get(at + usize::from(left) + 1)
                    .is_some_and(|next| u32::from(*next) == u32::from(first) + u32::from(left) + 1)
            {
                left += 1;
            }
            out.extend_from_slice(&first.to_be_bytes());
            if wide {
                out.extend_from_slice(&left.to_be_bytes());
            } else {
                out.push(left as u8);
            }
            at += usize::from(left) + 1;
        }
        out
    };
    [zero, ranges(false), ranges(true)]
        .into_iter()
        .min_by_key(Vec::len)
}

/// An encoding giving each of glyphs 1 to `encoded` its lowest code, in
/// 5176's smaller format, and every other code a glyph answers to as a
/// supplement by its SID.
fn encoding(desired: &[u16; 256], encoded: usize, sids: &[u16]) -> Option<Vec<u8>> {
    let mut main: Vec<u8> = vec![0; encoded];
    let mut assigned = vec![false; encoded];
    let mut supplements: Vec<(u8, u16)> = Vec::new();
    for (code, &gid) in desired.iter().enumerate() {
        if gid == 0 {
            continue;
        }
        let slot = usize::from(gid) - 1;
        let code = u8::try_from(code).ok()?;
        if slot < encoded && !assigned[slot] {
            main[slot] = code;
            assigned[slot] = true;
        } else {
            supplements.push((code, *sids.get(slot)?));
        }
    }
    if assigned.iter().any(|done| !done) {
        return None;
    }
    let flag = if supplements.is_empty() { 0 } else { 0x80 };
    let mut formats = Vec::new();
    if let Ok(count) = u8::try_from(encoded) {
        let mut zero = vec![flag, count];
        zero.extend_from_slice(&main);
        formats.push(zero);
    }
    let mut ranges: Vec<(u8, u8)> = Vec::new();
    for &code in &main {
        match ranges.last_mut() {
            Some((first, left))
                if *left < 255 && u16::from(*first) + u16::from(*left) + 1 == u16::from(code) =>
            {
                *left += 1;
            }
            _ => ranges.push((code, 0)),
        }
    }
    if let Ok(count) = u8::try_from(ranges.len()) {
        let mut one = vec![flag | 1, count];
        for (first, left) in &ranges {
            one.push(*first);
            one.push(*left);
        }
        formats.push(one);
    }
    let mut out = formats.into_iter().min_by_key(Vec::len)?;
    if !supplements.is_empty() {
        out.push(u8::try_from(supplements.len()).ok()?);
        for (code, sid) in supplements {
            out.push(code);
            out.extend_from_slice(&sid.to_be_bytes());
        }
    }
    Some(out)
}

/// Writes the whole CFF: header, Name INDEX, Top DICT INDEX, String INDEX,
/// an empty global subroutine INDEX, the charset, the encoding, the
/// CharStrings INDEX and the Private DICT, the Top DICT's offsets in the
/// fixed five-byte form so its length is known before they are.
fn assemble(
    font: &Type1<'_>,
    facts: &Facts,
    names: &[&[u8]],
    charstrings: &[Vec<u8>],
    desired: &[u16; 256],
    encoded: usize,
) -> Result<Vec<u8>, Refusal> {
    let mut strings = Strings {
        custom: Vec::new(),
        at: BTreeMap::new(),
    };
    let mut sids = Vec::with_capacity(names.len().saturating_sub(1));
    for name in names.iter().skip(1) {
        sids.push(strings.sid(name)?);
    }

    // The predefined Standard encoding where it gives every code the glyph
    // the program's does, through this charset.
    let mut by_sid: BTreeMap<u16, u16> = BTreeMap::new();
    for (at, sid) in sids.iter().enumerate() {
        by_sid.entry(*sid).or_insert((at + 1) as u16);
    }
    let mut standard = [0u16; 256];
    for (code, sid) in STANDARD_ENCODING {
        standard[usize::from(*code)] = by_sid.get(sid).copied().unwrap_or(0);
    }
    let encoding_bytes = if standard == *desired {
        None
    } else {
        Some(encoding(desired, encoded, &sids).ok_or(Refusal::Mismatch)?)
    };
    let charset_bytes = charset(&sids);

    let mut top_entries: Vec<(u16, Vec<f64>)> = Vec::new();
    for (op, value) in &facts.strings {
        top_entries.push((*op, vec![f64::from(strings.sid(value)?)]));
    }
    for (op, value) in &facts.numbers {
        top_entries.push((*op, vec![*value]));
    }
    if font.font_matrix != [0.001, 0.0, 0.0, 0.001, 0.0, 0.0] {
        top_entries.push((0x0C07, font.font_matrix.to_vec()));
    }
    if let Some(bbox) = facts.bbox {
        top_entries.push((5, bbox.to_vec()));
    }

    let mut private = Vec::new();
    for (op, values) in &facts.private {
        for value in values {
            private.extend_from_slice(&dict_operand(*value));
        }
        private.extend_from_slice(&dict_op(*op));
    }

    let top = |charset_at: i32, encoding_at: i32, charstrings_at: i32, private_at: i32| {
        let mut out = Vec::new();
        for (op, values) in &top_entries {
            for value in values {
                out.extend_from_slice(&dict_operand(*value));
            }
            out.extend_from_slice(&dict_op(*op));
        }
        out.extend_from_slice(&dict_int(charset_at));
        out.push(15);
        out.extend_from_slice(&dict_int(encoding_at));
        out.push(16);
        out.extend_from_slice(&dict_int(charstrings_at));
        out.push(17);
        out.extend_from_slice(&dict_int(private.len() as i32));
        out.extend_from_slice(&dict_int(private_at));
        out.push(18);
        out
    };

    let names_index = write_index(&[facts.font_name.clone()]).ok_or(Refusal::TooLarge)?;
    let strings_index = write_index(&strings.custom).ok_or(Refusal::TooLarge)?;
    let gsubrs_index = write_index(&[]).ok_or(Refusal::TooLarge)?;
    let charstrings_index = write_index(charstrings).ok_or(Refusal::TooLarge)?;
    let top_len = top(0, 0, 0, 0).len();
    let top_index_len = write_index(&[vec![0u8; top_len]])
        .ok_or(Refusal::TooLarge)?
        .len();

    let offset = |at: usize| i32::try_from(at).map_err(|_| Refusal::TooLarge);
    let mut cursor =
        4 + names_index.len() + top_index_len + strings_index.len() + gsubrs_index.len();
    let charset_at = match &charset_bytes {
        Some(bytes) => {
            let here = cursor;
            cursor += bytes.len();
            offset(here)?
        }
        None => 0,
    };
    let encoding_at = match &encoding_bytes {
        Some(bytes) => {
            let here = cursor;
            cursor += bytes.len();
            offset(here)?
        }
        None => 0,
    };
    let charstrings_at = offset(cursor)?;
    cursor += charstrings_index.len();
    let private_at = offset(cursor)?;

    let top_bytes = top(charset_at, encoding_at, charstrings_at, private_at);
    let top_index = write_index(&[top_bytes]).ok_or(Refusal::TooLarge)?;
    if top_index.len() != top_index_len {
        return Err(Refusal::Mismatch);
    }

    let mut out = Vec::with_capacity(cursor + private.len());
    out.extend_from_slice(&[1, 0, 4, 4]);
    out.extend_from_slice(&names_index);
    out.extend_from_slice(&top_index);
    out.extend_from_slice(&strings_index);
    out.extend_from_slice(&gsubrs_index);
    if let Some(bytes) = &charset_bytes {
        out.extend_from_slice(bytes);
    }
    if let Some(bytes) = &encoding_bytes {
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(&charstrings_index);
    if out.len() != cursor {
        return Err(Refusal::Mismatch);
    }
    out.extend_from_slice(&private);
    Ok(out)
}

/// Reads the CFF back: every Type 1 glyph found at its glyph by its name,
/// outlining and advancing as the program draws it; a `.notdef` the program
/// lacked empty; the font matrix the program's; and every built-in code
/// reaching the glyph the program's does.
fn verify(
    font: &Type1<'_>,
    program: &[u8],
    gids: &[u16],
    notdef_added: bool,
    work: &mut u64,
) -> Result<(), Refusal> {
    let mut spend = |steps: u64| -> Result<(), Refusal> {
        *work = work.saturating_add(steps);
        if *work > WORK_BUDGET {
            return Err(Refusal::Budget);
        }
        Ok(())
    };
    let cff = Cff::parse(program).ok_or(Refusal::Mismatch)?;
    if cff.glyph_count() != gids.len() + usize::from(notdef_added)
        || cff.is_cid()
        || cff.font_matrix != font.font_matrix
        || cff.glyph_name(0) != Some(".notdef")
    {
        return Err(Refusal::Mismatch);
    }
    for (index, &gid) in gids.iter().enumerate() {
        let name = font.glyph_name(index).ok_or(Refusal::Mismatch)?;
        let named = cff.glyph_name(gid).ok_or(Refusal::Mismatch)?;
        if named.as_bytes() != name || cff.gid_for_name(named) != Some(gid) {
            return Err(Refusal::Mismatch);
        }
        let glyph = u16::try_from(index).map_err(|_| Refusal::Mismatch)?;
        let (before, before_steps) = font.outline_counted(glyph);
        let (after, after_steps) = cff.outline_counted(gid);
        spend(before_steps.saturating_add(after_steps).saturating_mul(2))?;
        match (before, after) {
            (Some(a), Some(b)) if a.segments == b.segments => {}
            _ => return Err(Refusal::Mismatch),
        }
        if font.advance(glyph) != cff.advance(gid) {
            return Err(Refusal::Mismatch);
        }
    }
    if notdef_added {
        let empty = cff.outline(0).is_some_and(|outline| outline.is_empty());
        if !empty || cff.advance(0) != Some(0.0) {
            return Err(Refusal::Mismatch);
        }
    }
    let notdef = font.glyph_for_name(b".notdef");
    for code in 0..=255u8 {
        let expected = font
            .glyph_for_code(code)
            .filter(|glyph| Some(*glyph) != notdef)
            .and_then(|glyph| gids.get(usize::from(glyph)).copied());
        if cff.gid_for_code(code) != expected {
            return Err(Refusal::Mismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
