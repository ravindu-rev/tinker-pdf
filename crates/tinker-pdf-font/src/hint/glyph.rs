//! A glyph hinted end to end (`docs/design/hinting.md`, "The pipeline"):
//! a face read once and its `fpgm` run ([`Face`]), the face at a size with
//! its control value table scaled and its `prep` run ([`AtSize`]), and each
//! glyph's points scaled to the size in 26.6 with its four phantom points,
//! its program run over them from a copy of the size's state, and the
//! hinted points read back into an [`Outline`] through `glyf`'s own contour
//! reading ([`Hinted`]). A composite hints each component in turn with its
//! own program, places it by its offset, rounded to the grid where it asks,
//! or by matching a point of it to one already placed, takes its metrics
//! from the component that asks, and then runs its own program over the
//! points assembled, inside `glyf`'s depth and component budgets.
//!
//! A face whose `fpgm` breaks a rule is unhinted at every size, a size whose
//! `prep` does at that size, and a glyph whose program does — a component's
//! among them — is that glyph alone: each answers [`Unhinted::Failed`],
//! naming the program and the error, for the caller to report, and the glyph
//! is drawn from the outline its font states. Nothing is retried leniently.

use std::sync::Arc;

use super::fixed;
use super::state::{Dropout, GraphicsState, Point, Zone};
use super::storage::Size;
use super::vm::{Error, Limits, Program, Vm};
use crate::glyf;
use crate::outline::Outline;
use crate::sfnt::Sfnt;

const TAG_CVT: u32 = u32::from_be_bytes(*b"cvt ");
const TAG_FPGM: u32 = u32::from_be_bytes(*b"fpgm");
const TAG_GASP: u32 = u32::from_be_bytes(*b"gasp");
const TAG_HEAD: u32 = u32::from_be_bytes(*b"head");
const TAG_MAXP: u32 = u32::from_be_bytes(*b"maxp");
const TAG_PREP: u32 = u32::from_be_bytes(*b"prep");

/// `head.flags` bit 3: "Force ppem to integer values for all internal scaler
/// math".
const INTEGER_PPEM: u16 = 1 << 3;
/// `gasp`'s `GASP_GRIDFIT`: "Use gridfitting".
const GASP_GRIDFIT: u16 = 1;

/// The fewest pixels per em a glyph is hinted at, in 26.6: below a pixel
/// there is no grid to fit.
const LEAST_PPEM: i32 = fixed::ONE;
/// The most, in 26.6: a glyph larger than 1 024 pixels an em takes the
/// renderer's path route, and is not hinted (*When a glyph is hinted*).
const MOST_PPEM: i32 = 1_024 * fixed::ONE;

/// The most contour points a composite assembles: as many as a simple glyph's
/// sixteen-bit contour ends can number, and as `maxp`'s
/// `maxCompositePoints` can declare.
pub const MAX_POINTS: usize = 1 << 16;

/// Why a glyph is drawn from the outline its font states rather than
/// hinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unhinted {
    /// The face carries no TrueType profile to hint by: no version 1.0
    /// `maxp`. A CFF face, or one cut down past it.
    NoProfile,
    /// The size is one the face does not hint at: its `gasp` range asks for
    /// no grid-fitting, a cut-short `gasp` covers no range for it, or it is
    /// below one pixel or above 1 024 pixels an em.
    NotAtThisSize,
    /// The glyph's data does not read as a glyph: past `loca`, or ending
    /// before its points do.
    UnreadableGlyph,
    /// A composite whose components assemble more than [`MAX_POINTS`]
    /// points, the most a glyph's zone holds.
    TooManyPoints,
    /// A program broke a rule or a bound of the interpreter
    /// (`docs/design/hinting.md`, "Failure"): the face's `fpgm`, which leaves
    /// it unhinted at every size; the size's `prep`, which leaves the size;
    /// or a glyph's own, which leaves the glyph. The caller reports it.
    Failed {
        /// The program that broke it.
        program: Program,
        /// What it broke.
        error: Error,
        /// The steps the run took before it stopped.
        steps: u64,
    },
}

impl core::fmt::Display for Unhinted {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Unhinted::NoProfile => write!(f, "the face has no TrueType profile"),
            Unhinted::NotAtThisSize => write!(f, "the face is not hinted at this size"),
            Unhinted::UnreadableGlyph => write!(f, "the glyph's data does not read"),
            Unhinted::TooManyPoints => {
                write!(f, "the composite assembles more than {MAX_POINTS} points")
            }
            Unhinted::Failed { program, error, .. } => {
                let which = match program {
                    Program::Font => "the font program",
                    Program::ControlValue => "the control value program",
                    Program::Glyph => "the glyph's program",
                };
                write!(f, "{which} failed: {error}")
            }
        }
    }
}

/// A glyph hinted at a size.
#[derive(Clone, Debug)]
pub struct Hinted {
    /// The hinted outline in pixels about the glyph's origin, y up as glyph
    /// space is: each point a multiple of 1/64, an implied midpoint half
    /// way between two.
    pub outline: Outline,
    /// The four phantom points as the programs left them, in 26.6: the
    /// horizontal origin, the advance width point, the vertical origin and
    /// the advance height point. The hinted advance is the second's `x` less
    /// the first's.
    pub phantom: [Point; 4],
    /// The dropout control the size's and the glyph's programs left, at the
    /// size's whole pixels per em, for a glyph drawn whole or empty
    /// (`docs/design/hinting.md`, "Monochrome dropout control").
    pub dropout: Option<Dropout>,
    /// Whether a program ran over the glyph: one it carries, where the
    /// control value program did not inhibit it.
    pub ran_program: bool,
    /// The steps the glyph's program took.
    pub steps: u64,
}

/// A face read for hinting, its `fpgm` run: the functions and instruction
/// definitions every later program calls, the tables each size is set up
/// from, and the `gasp` and `head` settings that decide a size.
#[derive(Clone, Debug)]
pub struct Face {
    /// The machine `fpgm` ran on: its definitions, and the face's limits.
    vm: Vm,
    /// The `cvt ` table's bytes, scaled anew at each size.
    cvt: Arc<[u8]>,
    /// The control value program.
    prep: Arc<[u8]>,
    /// `gasp`'s ranges, each its `rangeMaxPPEM` and its behaviour; `None` for
    /// a face with no `gasp` this engine reads.
    gasp: Option<Vec<(u16, u16)>>,
    /// Whether `head` asks for integer pixels per em.
    integer_ppem: bool,
    /// `head.unitsPerEm`, as [`Sfnt`] reads it.
    units_per_em: u16,
    /// The steps `fpgm` took.
    steps: u64,
}

/// `gasp`'s ranges in a version 0 or 1 table, in order; `None` for a face
/// with none, or a table of another version or too short for its header,
/// which says nothing a size can be held to. A table whose ranges stop before
/// its count says is read as far as it goes.
fn gasp_ranges(sfnt: &Sfnt) -> Option<Vec<(u16, u16)>> {
    let gasp = sfnt.table(TAG_GASP)?;
    let field = |at: usize| -> Option<u16> {
        let bytes = gasp.get(at..at + 2)?;
        Some(u16::from_be_bytes([bytes[0], bytes[1]]))
    };
    let version = field(0)?;
    let count = field(2)?;
    if version > 1 {
        return None;
    }
    Some(
        (0..usize::from(count))
            .map_while(|i| Some((field(4 + 4 * i)?, field(6 + 4 * i)?)))
            .collect(),
    )
}

#[cfg(test)]
thread_local! {
    /// How many times this thread ran a font program, a control value
    /// program and a glyph program: what
    /// `fpgm_runs_once_per_face_and_prep_once_per_size` counts.
    pub(crate) static RUNS: std::cell::Cell<[u32; 3]> = const { std::cell::Cell::new([0; 3]) };
}

/// Counts a run of `program` on this thread, in a test build.
fn counted(program: Program) {
    #[cfg(test)]
    RUNS.with(|runs| {
        let mut counts = runs.get();
        counts[program as usize] += 1;
        runs.set(counts);
    });
    #[cfg(not(test))]
    let _ = program;
}

impl Face {
    /// A face read for hinting: `maxp`'s limits, `head`'s units per em and
    /// flags, `cvt `, `fpgm`, `prep` and `gasp`, and its `fpgm` run once from
    /// the default graphics state, asking `stop` as a run asks it.
    ///
    /// # Errors
    ///
    /// [`Unhinted::NoProfile`] for a face with no version 1.0 `maxp`, and
    /// [`Unhinted::Failed`] for an `fpgm` that breaks a rule: either way the
    /// face is unhinted at every size.
    pub fn new(sfnt: &Sfnt, stop: Option<&dyn Fn() -> bool>) -> Result<Face, Unhinted> {
        let limits = sfnt
            .table(TAG_MAXP)
            .and_then(Limits::from_maxp)
            .ok_or(Unhinted::NoProfile)?;
        let flags = sfnt
            .table(TAG_HEAD)
            .and_then(|head| head.get(16..18))
            .map_or(0, |bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
        let table = |tag: u32| -> Arc<[u8]> { Arc::from(sfnt.table(tag).unwrap_or_default()) };
        let fpgm = table(TAG_FPGM);
        let mut vm = Vm::new(limits);
        counted(Program::Font);
        let result = vm.run(&fpgm, Program::Font, stop);
        let steps = u64::from(vm.steps());
        result.map_err(|error| Unhinted::Failed {
            program: Program::Font,
            error,
            steps,
        })?;
        Ok(Face {
            vm,
            cvt: table(TAG_CVT),
            prep: table(TAG_PREP),
            gasp: gasp_ranges(sfnt),
            integer_ppem: flags & INTEGER_PPEM != 0,
            units_per_em: sfnt.units_per_em,
            steps,
        })
    }

    /// The steps the face's `fpgm` took.
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Whether the face is hinted at `ppem` pixels per em, in 26.6: within
    /// one pixel to 1 024, and where the face has a `gasp`, the first range
    /// whose `rangeMaxPPEM` is at least the size's whole pixels per em asks
    /// for grid-fitting. A size past the last range, which a table cut short
    /// leaves, is not hinted; a face with no `gasp` is hinted at every size.
    #[must_use]
    pub fn hints_at(&self, ppem: i32) -> bool {
        if !(LEAST_PPEM..=MOST_PPEM).contains(&ppem) {
            return false;
        }
        let Some(ranges) = &self.gasp else {
            return true;
        };
        let whole = fixed::round(ppem) / fixed::ONE;
        ranges
            .iter()
            .find(|&&(most, _)| i32::from(most) >= whole)
            .is_some_and(|&(_, behaviour)| behaviour & GASP_GRIDFIT != 0)
    }

    /// The face at a size — `ppem` pixels per em and an em of `points`
    /// points, each in 26.6, drawn in grey levels or, `monochrome`, whole or
    /// empty — set up once: `ppem` rounded to a whole pixel where `head`
    /// asks for integer sizes, the control value table scaled to it, the
    /// storage area cleared to zeros and the twilight zone set to the
    /// origin, then `prep` run from the default graphics state. What `prep`
    /// leaves of the settings is what each glyph of the size starts from
    /// (*Zones, points and the graphics state*), and its `INSTCTRL` flags
    /// stand for the size.
    ///
    /// # Errors
    ///
    /// [`Unhinted::NotAtThisSize`] where [`Face::hints_at`] says no, before
    /// `prep` runs; [`Unhinted::Failed`] for a `prep` that breaks a rule,
    /// which leaves the size unhinted.
    pub fn at_size(
        &self,
        ppem: i32,
        points: i32,
        monochrome: bool,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<AtSize, Unhinted> {
        if !self.hints_at(ppem) {
            return Err(Unhinted::NotAtThisSize);
        }
        let ppem = if self.integer_ppem {
            fixed::round(ppem)
        } else {
            ppem
        };
        let mut vm = self.vm.clone();
        vm.set_size(Size {
            ppem,
            points,
            units_per_em: self.units_per_em,
            monochrome,
        });
        vm.load_cvt(&self.cvt);
        let limits = vm.limits();
        vm.storage = vec![0; limits.storage()];
        vm.zones[0] = Zone::twilight(limits.twilight());
        vm.set_graphics_state(GraphicsState::default());
        counted(Program::ControlValue);
        let result = vm.run(&self.prep, Program::ControlValue, stop);
        let steps = u64::from(vm.steps());
        result.map_err(|error| Unhinted::Failed {
            program: Program::ControlValue,
            error,
            steps,
        })?;
        let left = *vm.graphics_state();
        // INSTCTRL's second selector: glyph programs start from the summary's
        // defaults rather than the control value program's settings.
        let glyph_state = if left.instruct_control & 2 != 0 {
            GraphicsState {
                instruct_control: left.instruct_control,
                ..GraphicsState::default()
            }
        } else {
            GraphicsState {
                auto_flip: left.auto_flip,
                control_value_cut_in: left.control_value_cut_in,
                delta_base: left.delta_base,
                delta_shift: left.delta_shift,
                instruct_control: left.instruct_control,
                minimum_distance: left.minimum_distance,
                round_state: left.round_state,
                scan_control: left.scan_control,
                scan_type: left.scan_type,
                single_width_cut_in: left.single_width_cut_in,
                single_width_value: left.single_width_value,
                angle_weight: left.angle_weight,
                ..GraphicsState::default()
            }
        };
        Ok(AtSize {
            vm,
            glyph_state,
            steps,
        })
    }
}

/// A face at one size, its `prep` run: the control value table, the storage
/// area and the twilight zone `prep` left, kept for the size, and the
/// graphics state each glyph starts from. Every glyph is hinted from a copy
/// of them, so no glyph's program changes another's, and a glyph hints the
/// same whichever glyphs were hinted before it.
#[derive(Clone, Debug)]
pub struct AtSize {
    /// The machine `prep` ran on.
    vm: Vm,
    /// What each glyph's program starts from.
    glyph_state: GraphicsState,
    /// The steps `prep` took.
    steps: u64,
}

impl AtSize {
    /// The size the face is hinted at: its pixels per em as the points and
    /// the control values were scaled to.
    #[must_use]
    pub fn size(&self) -> Size {
        self.vm.size()
    }

    /// The steps the size's `prep` took.
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// The control value table `prep` left, in 26.6.
    #[must_use]
    pub fn cvt(&self) -> &[i32] {
        self.vm.cvt()
    }

    /// Glyph `glyph` of `sfnt`, the face this was made from, hinted: a
    /// simple glyph's points and its four phantom points scaled to the size in
    /// 26.6 as the control values are, each both current and original, and its
    /// program run over them as a glyph program from a copy of the size's
    /// state, unless it carries none or the control value program's `INSTCTRL`
    /// inhibits grid-fitting, which leaves it its scaled points; a composite
    /// assembled from its components, each hinted with its own program and
    /// placed by its offset or by point matching, its metrics taken from the
    /// component that asks, and then its own program run over the points
    /// assembled (`docs/design/hinting.md`, "The pipeline"). The points
    /// then read back into an outline through `glyf`'s contour reading, by the
    /// on-curve flags as the programs left them.
    ///
    /// # Errors
    ///
    /// [`Unhinted::UnreadableGlyph`] for a glyph `glyf` does not read,
    /// [`Unhinted::TooManyPoints`] for a composite past [`MAX_POINTS`], and
    /// [`Unhinted::Failed`] for a program that breaks a rule, the glyph's own
    /// or a component's, which leaves the glyph unhinted.
    pub fn glyph(
        &self,
        sfnt: &Sfnt,
        glyph: u16,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<Hinted, Unhinted> {
        let mut budget = glyf::MAX_COMPONENTS;
        let glyph = self.assemble(sfnt, glyph, 0, &mut budget, true, stop)?;
        let positions: Vec<(f64, f64)> = glyph
            .current
            .iter()
            .map(|p| {
                (
                    f64::from(p.x) / f64::from(fixed::ONE),
                    f64::from(p.y) / f64::from(fixed::ONE),
                )
            })
            .collect();
        let outline = glyf::outline_of(&glyph.ends, &glyph.on_curve, &positions);
        Ok(Hinted {
            outline,
            phantom: glyph.phantom,
            dropout: glyph.dropout,
            ran_program: glyph.ran,
            steps: glyph.steps,
        })
    }

    /// Glyph `glyph`'s points, `depth` composites down with `budget`
    /// components left of `glyf`'s 256, its programs run where `hint` says.
    ///
    /// Past `glyf`'s depth of 8, or with the budget spent, a glyph adds no
    /// points, as it draws none in `glyf`'s own reading. A simple glyph is
    /// [`AtSize::simple`]'s. A composite takes its records in order, at most
    /// 64 of them: each component assembled one level down — hinted, unless
    /// it stands under a scale or a 2×2, which this engine assembles from its
    /// scaled points with no program anywhere under it, since its programs
    /// measured an outline the transform has changed — then transformed by
    /// its 2×2, each coordinate rounded half away from zero, and placed: by
    /// its offset, scaled as the points are and, where `ROUND_XY_TO_GRID` is
    /// set, rounded to the grid on each axis in its hinted position; or,
    /// where `ARGS_ARE_XY_VALUES` is clear, by moving its point the second
    /// argument numbers onto the point already placed the first numbers, its
    /// hinted position onto the hinted one and its original onto the
    /// original — with no offset where either point is not there, as `glyf`
    /// reads such a record. A component that does not read adds nothing. The
    /// composite's phantom points are its own metrics', scaled, or the hinted
    /// phantom points of the last component carrying `USE_MY_METRICS`, not
    /// moved by its offset, as `glyf` takes them. Where it is hinted and
    /// carries a program, that runs last, over the assembled points standing
    /// at their hinted positions over their original ones and its four
    /// phantom points, untouched, as a glyph program.
    fn assemble(
        &self,
        sfnt: &Sfnt,
        glyph: u16,
        depth: u32,
        budget: &mut u32,
        hint: bool,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<Assembled, Unhinted> {
        if depth > glyf::MAX_COMPONENT_DEPTH || *budget == 0 {
            return Ok(Assembled::default());
        }
        *budget -= 1;
        let Some(composite) = glyf::composite_glyph(sfnt, glyph) else {
            return self.simple(sfnt, glyph, hint, stop);
        };
        let size = self.vm.size();
        let scaled = |[x, y]: [i32; 2]| Point {
            x: size.scale(x),
            y: size.scale(y),
        };
        let own = glyf::phantom_points(sfnt, glyph).map(scaled);
        let mut out = Assembled {
            phantom: own,
            phantom_original: own,
            dropout: self.glyph_state.dropout(size.pixels_per_em()),
            ..Assembled::default()
        };
        let mut metrics = None;
        for component in composite.components.iter().take(glyf::MAX_RECORDS as usize) {
            let transformed = component.transform != glyf::Component::IDENTITY;
            let part = match self.assemble(
                sfnt,
                component.glyph,
                depth + 1,
                budget,
                hint && !transformed,
                stop,
            ) {
                Ok(part) => part,
                Err(Unhinted::UnreadableGlyph) => Assembled::default(),
                Err(other) => return Err(other),
            };
            let part = if transformed {
                part.transformed(component.transform)
            } else {
                part
            };
            let (current, original) = if component.offset_is_xy() {
                let [x, y] = component.arguments;
                let at = (size.scale(x), size.scale(y));
                let hinted = if component.flags & glyf::Component::ROUND_XY_TO_GRID != 0 {
                    (fixed::round(at.0), fixed::round(at.1))
                } else {
                    at
                };
                (hinted, at)
            } else {
                // The first argument numbers a point already placed, the
                // second a point of the component.
                let [parent, child] = component
                    .arguments
                    .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
                let onto = |to: Option<&Point>, from: Option<&Point>| match (to, from) {
                    (Some(to), Some(from)) => {
                        (to.x.wrapping_sub(from.x), to.y.wrapping_sub(from.y))
                    }
                    _ => (0, 0),
                };
                (
                    onto(out.current.get(parent), part.current.get(child)),
                    onto(out.original.get(parent), part.original.get(child)),
                )
            };
            if component.flags & glyf::Component::USE_MY_METRICS != 0 {
                metrics = Some((part.phantom, part.phantom_original));
            }
            if out.current.len() + part.current.len() > MAX_POINTS {
                return Err(Unhinted::TooManyPoints);
            }
            out.append(part, current, original);
            if *budget == 0 {
                break;
            }
        }
        if let Some((phantom, original)) = metrics {
            out.phantom = phantom;
            out.phantom_original = original;
        }
        if hint && !composite.instructions.is_empty() && self.glyph_state.instruct_control & 1 == 0
        {
            self.run_over(&mut out, composite.instructions, stop)?;
        }
        Ok(out)
    }

    /// A simple glyph's points, or one with no outline's four phantom points:
    /// scaled, and its program run over them where `hint` says and it carries
    /// one, as [`AtSize::glyph`] describes.
    fn simple(
        &self,
        sfnt: &Sfnt,
        glyph: u16,
        hint: bool,
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<Assembled, Unhinted> {
        let simple = glyf::simple_glyph(sfnt, glyph).ok_or(Unhinted::UnreadableGlyph)?;
        let size = self.vm.size();
        let count = simple.contour_points();
        let points: Vec<Point> = simple
            .points
            .iter()
            .map(|&[x, y]| Point {
                x: size.scale(x),
                y: size.scale(y),
            })
            .collect();
        let mut phantom = [Point::default(); 4];
        for (slot, point) in phantom.iter_mut().zip(&points[count..]) {
            *slot = *point;
        }
        let mut out = Assembled {
            current: points[..count].to_vec(),
            original: points[..count].to_vec(),
            on_curve: simple.flags.iter().map(|flag| flag & 1 != 0).collect(),
            ends: simple.contours().iter().map(|&(_, end)| end).collect(),
            phantom,
            phantom_original: phantom,
            dropout: self.glyph_state.dropout(size.pixels_per_em()),
            ran: false,
            steps: 0,
        };
        if hint && !simple.instructions.is_empty() && self.glyph_state.instruct_control & 1 == 0 {
            self.run_over(&mut out, simple.instructions, stop)?;
        }
        Ok(out)
    }

    /// Runs `code` as a glyph program over `glyph`'s points and its four
    /// phantom points, from a copy of the size's state, and takes back where
    /// it left them, their on-curve flags and the dropout state.
    fn run_over(
        &self,
        glyph: &mut Assembled,
        code: &[u8],
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<(), Unhinted> {
        let count = glyph.current.len();
        let mut current = glyph.current.clone();
        current.extend(glyph.phantom);
        let mut original = glyph.original.clone();
        original.extend(glyph.phantom_original);
        let mut on_curve = glyph.on_curve.clone();
        on_curve.extend([false; 4]);
        let zone = Zone::with_positions(&current, &original, &on_curve, &glyph.ends)
            .ok_or(Unhinted::UnreadableGlyph)?;
        let mut vm = self.vm.clone();
        vm.set_graphics_state(self.glyph_state);
        vm.set_glyph(zone);
        counted(Program::Glyph);
        let result = vm.run(code, Program::Glyph, stop);
        glyph.steps = glyph.steps.saturating_add(u64::from(vm.steps()));
        result.map_err(|error| Unhinted::Failed {
            program: Program::Glyph,
            error,
            steps: glyph.steps,
        })?;
        let hinted = &vm.zones[1];
        glyph.current.copy_from_slice(&hinted.current[..count]);
        for (i, on) in glyph.on_curve.iter_mut().enumerate() {
            *on = hinted.on_curve(i);
        }
        for (k, slot) in glyph.phantom.iter_mut().enumerate() {
            *slot = hinted.current[count + k];
        }
        glyph.dropout = vm.graphics_state().dropout(self.vm.size().pixels_per_em());
        glyph.ran = true;
        Ok(())
    }
}

/// A glyph's points as [`AtSize::assemble`] gathers them: each contour
/// point's hinted and original positions in 26.6, its on-curve flag, each
/// contour's last point, the four phantom points hinted and original, the
/// dropout state the glyph's own program left or, where it ran none, the
/// size's, whether a program ran over any of them, its own or a
/// component's, and the steps every run took.
#[derive(Clone, Debug, Default)]
struct Assembled {
    current: Vec<Point>,
    original: Vec<Point>,
    on_curve: Vec<bool>,
    ends: Vec<usize>,
    phantom: [Point; 4],
    phantom_original: [Point; 4],
    dropout: Option<Dropout>,
    ran: bool,
    steps: u64,
}

impl Assembled {
    /// The points under a component's 2×2, F2Dot14 `[xscale, scale01,
    /// scale10, yscale]`: `x' = xscale · x + scale10 · y` and `y' = scale01 ·
    /// x + yscale · y`, as `glyf` places such a component, each coordinate
    /// rounded half away from zero and wrapped. The phantom points stay, as
    /// `glyf` takes a component's metrics untransformed.
    fn transformed(mut self, [a, b, c, d]: [i16; 4]) -> Assembled {
        let map = |p: &mut Point| {
            let (x, y) = (i64::from(p.x), i64::from(p.y));
            let (a, b, c, d) = (i64::from(a), i64::from(b), i64::from(c), i64::from(d));
            p.x = fixed::round_div(a * x + c * y, 16_384) as i32;
            p.y = fixed::round_div(b * x + d * y, 16_384) as i32;
        };
        self.current.iter_mut().for_each(map);
        self.original.iter_mut().for_each(map);
        self
    }

    /// `part` after the points gathered so far, moved by `current` where it
    /// stands hinted and by `original` where it stood, its contours renumbered
    /// after them.
    fn append(&mut self, part: Assembled, current: (i32, i32), original: (i32, i32)) {
        let base = self.current.len();
        let moved = |p: &Point, (dx, dy): (i32, i32)| Point {
            x: p.x.wrapping_add(dx),
            y: p.y.wrapping_add(dy),
        };
        self.current
            .extend(part.current.iter().map(|p| moved(p, current)));
        self.original
            .extend(part.original.iter().map(|p| moved(p, original)));
        self.on_curve.extend(part.on_curve);
        self.ends.extend(part.ends.iter().map(|end| base + end));
        self.ran |= part.ran;
        self.steps = self.steps.saturating_add(part.steps);
    }
}
