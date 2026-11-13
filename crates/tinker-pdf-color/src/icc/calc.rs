//! ICC.2's calculator element (`calc`, ICC.2:2023 11.2.1): a program of
//! operations in reverse Polish notation over a profile's channels, read
//! and validated when its table is built and run there, never per colour
//! (`docs/design/icc.md`, "Version 5").
//!
//! A calculator element names a main function — a `func` of operations,
//! eight bytes each, a signature and four data bytes — and sub-elements its
//! operations may invoke: curve sets, matrices, grids and calculators of
//! their own. The function keeps a stack of numbers, reads its input
//! channels, writes its output channels and keeps temporary channels, each
//! zero until set; conditional operations (`if`, `else`) and selections
//! (`sel`, `case`, `dflt`) carry streams of operations of their own.
//!
//! # Validated before it runs, and bounded on every axis
//!
//! A program is read whole and checked before any of it runs, as 11.2.1
//! has a CMM do: every operation one this build runs, every channel index
//! in range, every sub-element one of the type its operation names, and
//! the stack never taken below what is on it — found by walking the
//! program once, each conditional's streams leaving the stack as deep as
//! one another. Every count it can spend is capped, and a program past a cap
//! is refused as [`IccError::CalculatorLimit`] naming it (ruling 1):
//!
//! - [`MAX_CALC_OPERATIONS`]: the operations the element and its
//!   sub-elements hold, each counted once, and the steps one run can take —
//!   an operation a step for each value it takes from the stack or gives to
//!   it, one at the least, a `solv` its elimination's multiplications
//!   besides, a conditional its longer stream, and a sub-element its own
//!   steps each time it is invoked: a curve set a curve, a matrix a
//!   coefficient, a grid a corner and output, a calculator its run's;
//! - [`MAX_CALC_STACK`]: the deepest the stack goes;
//! - [`MAX_CALC_DEPTH`]: sub-elements within one another, and conditional
//!   streams within one another;
//! - [`MAX_CALC_TEMPORARIES`]: the temporary channels a program reaches;
//! - [`MAX_CALC_WORK`]: the steps a tag's calculators take while it is read,
//!   every run of them — a calculator stage's at each grid node, a sampled
//!   calculator curve's at each entry — counted before it is made.
//!
//! A `calculatorLimits` extension (`clmt`) stricter than a cap is honoured,
//! for the element and its sub-calculators: its operation count bounds the
//! operations they hold, as 11.2.1 states it. The precedent is
//! `Function::PostScript` (ISO 32000 7.10.5), bounded on every axis: no loop
//! operator, so a run is as long as its program.
//!
//! A calculator's sub-elements, and a curve set's curves among them, are
//! each read once: positions naming one place are one sub-element or one
//! curve, and places otherwise apart (11.2.1, 11.2.2.1), so a calculator's
//! bytes are read once however its sub-elements name them, and every table
//! they hold is an entry spent from its tag's `MAX_CLUT_ENTRIES`.
//!
//! The arithmetic is `f64`'s four operations and square root, which every
//! target rounds alike, and `tinker_pdf_math`'s transcendentals, so a
//! program gives one answer everywhere (ruling 4). The colour appearance
//! operations (`tJab`, `fJab`) and tint arrays (`tint`) are refused by name:
//! they are outside what this engine reads (`docs/design/icc.md`,
//! "Non-goals").

use super::mpe::{self, Budget};
use super::{IccError, Reader};

/// The most operations a calculator element may hold, its sub-elements'
/// among them, and the most steps one run of it may take.
///
/// The operation count is a 32-bit field read before the operations it
/// describes, an operation may take or give a vector of values, and a
/// sub-element runs once for every invocation, so a few bytes can describe a
/// run of any length; this bounds both, the bound `Function::PostScript`
/// holds its tokens to. A step is a value an operation takes from the stack
/// or gives to it, one at the least for each operation; a `solv` adds its
/// elimination's multiplications, and an invoked sub-element its own steps.
///
/// | | Operations |
/// | --- | --- |
/// | The most any fixture in this repository spends | 65 536 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 64 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **65 536** |
///
/// The document's 64 is ICC.2 Annex F's projector model (F.3), the longest
/// of its examples, rounded up; a comic page and a book picture carry no
/// profile this parser opens. The fixture is the test that runs a program of
/// exactly this many.
///
/// Reachable: the count is a 32-bit field, so a 12-byte function may
/// declare 4 294 967 295 operations;
/// `a_calculator_past_the_operation_cap_is_refused_before_it_runs` builds
/// one past the cap.
pub const MAX_CALC_OPERATIONS: usize = 1 << 16;

/// The deepest a calculator's stack may go.
///
/// 11.2.1 has a CMM keep room for 65 535 values where a profile states no
/// limit; a program that needs more than this is refused instead.
///
/// | | Values |
/// | --- | --- |
/// | The most any fixture in this repository spends | 1 024 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 16 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **1 024** |
///
/// The document's 16 is Annex F's projector model, whose stack holds nine
/// values at its deepest, rounded up.
///
/// Reachable: one `copy` duplicates up to 65 536 values 65 536 times;
/// `a_calculator_past_the_stack_cap_is_refused_before_it_runs` builds a
/// program one value past the cap.
pub const MAX_CALC_STACK: usize = 1024;

/// How deep sub-elements may sit within one another, and conditional streams
/// within one another, in one calculator.
///
/// | | Levels |
/// | --- | --- |
/// | The most any fixture in this repository spends | 16 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 4 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **16** |
///
/// The document's 4 is Annex F's projector model, three conditionals deep
/// in one function, and one level more.
///
/// Reachable: each level costs one operation slot or one sub-element, so
/// [`MAX_CALC_OPERATIONS`] slots may nest as deep;
/// `a_calculator_nested_past_the_depth_cap_is_refused_before_it_runs`
/// builds one a level past the cap.
pub const MAX_CALC_DEPTH: usize = 16;

/// The most temporary channels a calculator's program may reach.
///
/// 11.2.1 allows 65 535; a program reaching further than this is refused.
///
/// | | Channels |
/// | --- | --- |
/// | The most any fixture in this repository spends | 1 024 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 8 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **1 024** |
///
/// The document's 8 is Annex F's projector model, which keeps six, rounded
/// up.
///
/// Reachable: a channel operation names a 16-bit start and a 16-bit count,
/// so it may reach channel 131 070;
/// `a_calculator_past_the_temporary_cap_is_refused_before_it_runs` builds
/// one reaching a channel past the cap.
pub const MAX_CALC_TEMPORARIES: usize = 1024;

/// The most steps the calculators of one `multiProcessElementsType` tag may
/// take while it is read: every run of them, a calculator stage's at each
/// node of its grid and a sampled calculator curve's at each of its entries,
/// counted as the steps its program takes ([`MAX_CALC_OPERATIONS`]) and one
/// more for the run, before any of them is made.
///
/// | | Steps |
/// | --- | --- |
/// | The most any fixture in this repository spends | 67 108 864 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 8 388 608 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **67 108 864** |
///
/// The document's is Annex F's projector model (F.3), whose run takes 170
/// steps, at each of a three-input stage's 35 937 nodes — 6 109 290 —
/// rounded up; a comic page and a book picture carry no profile this parser
/// opens. The fixture is the test that samples a calculator curve of exactly
/// this many.
///
/// Reachable: a tag's grids may hold `MAX_CLUT_ENTRIES` entries, each a run
/// of [`MAX_CALC_OPERATIONS`] steps;
/// `a_tag_past_the_calculator_work_cap_is_refused_before_it_runs` builds
/// one a step past the cap.
pub const MAX_CALC_WORK: usize = 1 << 26;

/// Which of a calculator's caps a program passes (`IccError::CalculatorLimit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalculatorLimit {
    /// [`MAX_CALC_OPERATIONS`], or a stricter `clmt`'s count.
    Operations,
    /// [`MAX_CALC_STACK`], or a stricter `clmt`'s stack.
    Stack,
    /// [`MAX_CALC_DEPTH`].
    Depth,
    /// [`MAX_CALC_TEMPORARIES`], or a stricter `clmt`'s count.
    Temporaries,
    /// [`MAX_CALC_WORK`].
    Work,
}

/// The caps one calculator, and every sub-calculator within it, is held to:
/// this module's, or a `clmt`'s where it is stricter.
#[derive(Clone, Copy, Debug)]
struct Limits {
    operations: usize,
    stack: usize,
    temporaries: usize,
}

impl Limits {
    /// This module's caps.
    const CAPS: Limits = Limits {
        operations: MAX_CALC_OPERATIONS,
        stack: MAX_CALC_STACK,
        temporaries: MAX_CALC_TEMPORARIES,
    };

    /// These, with each a `clmt` states — zero being no limit — taken where
    /// it is stricter.
    fn within(self, stack: u32, temporaries: u32, operations: u32) -> Limits {
        let stricter = |ours: usize, theirs: u32| match theirs {
            0 => ours,
            theirs => ours.min(theirs as usize),
        };
        Limits {
            operations: stricter(self.operations, operations),
            stack: stricter(self.stack, stack),
            temporaries: stricter(self.temporaries, temporaries),
        }
    }
}

/// A calculator element, read and validated: what one run of it needs.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Calculator {
    /// Its input channels.
    inputs: usize,
    /// Its output channels.
    outputs: usize,
    /// Its main function.
    main: Vec<Op>,
    /// Its sub-elements, one each distinct position.
    subs: Vec<Sub>,
    /// For each of its sub-element positions, which of `subs` it names.
    positions: Vec<usize>,
    /// The temporary channels the program reaches.
    temporaries: usize,
    /// The deepest its stack goes.
    stack: usize,
    /// The steps one run takes at the most ([`MAX_CALC_OPERATIONS`]).
    steps: usize,
}

/// A sub-element a calculator's operations invoke, evaluated in `f64` as the
/// operation reaches it.
#[derive(Clone, Debug, PartialEq)]
enum Sub {
    /// A curve set: its curves, each read once, and for each channel which
    /// of them it takes.
    Curves {
        curves: Vec<mpe::Curve>,
        channels: Vec<usize>,
    },
    /// A matrix: `outputs` rows of `inputs` coefficients, then an offset a
    /// row.
    Matrix {
        inputs: usize,
        outputs: usize,
        values: Vec<f64>,
    },
    /// A grid: points along each input axis, the last fastest, `outputs`
    /// values a point.
    Grid {
        grids: Vec<usize>,
        outputs: usize,
        values: Vec<f64>,
    },
    /// A calculator.
    Calculator(Box<Calculator>),
    /// `bACS` or `eACS`: its channels passed through.
    Placeholder(usize),
}

impl Sub {
    /// The channels it takes and the channels it gives.
    fn counts(&self) -> (usize, usize) {
        match self {
            Sub::Curves { channels, .. } => (channels.len(), channels.len()),
            Sub::Matrix {
                inputs, outputs, ..
            } => (*inputs, *outputs),
            Sub::Grid { grids, outputs, .. } => (grids.len(), *outputs),
            Sub::Calculator(calculator) => (calculator.inputs, calculator.outputs),
            Sub::Placeholder(channels) => (*channels, *channels),
        }
    }

    /// The steps one invocation takes besides the values it moves: a curve
    /// a channel, a coefficient a row and column, a corner and output a grid
    /// point it mixes, a calculator its run's and one more for the run.
    fn steps(&self) -> usize {
        match self {
            Sub::Curves { channels, .. } => channels.len(),
            Sub::Matrix {
                inputs, outputs, ..
            } => inputs * outputs,
            Sub::Grid { grids, outputs, .. } => {
                (1usize << grids.len().min(31)).saturating_mul(*outputs)
            }
            Sub::Calculator(calculator) => calculator.steps.saturating_add(1),
            Sub::Placeholder(_) => 0,
        }
    }

    /// Its outputs for `x`.
    fn apply(&self, x: &[f64]) -> Vec<f64> {
        match self {
            Sub::Curves { curves, channels } => channels
                .iter()
                .zip(x)
                .map(|(&curve, &v)| curves.get(curve).map_or(v, |curve| curve.eval(v)))
                .collect(),
            Sub::Matrix {
                inputs,
                outputs,
                values,
            } => (0..*outputs)
                .map(|row| {
                    let sum: f64 = (0..*inputs)
                        .map(|column| {
                            values.get(row * inputs + column).copied().unwrap_or(0.0)
                                * x.get(column).copied().unwrap_or(0.0)
                        })
                        .sum();
                    sum + values.get(inputs * outputs + row).copied().unwrap_or(0.0)
                })
                .collect(),
            Sub::Grid {
                grids,
                outputs,
                values,
            } => interpolate(grids, *outputs, values, x),
            Sub::Calculator(calculator) => calculator.evaluate(x),
            Sub::Placeholder(_) => x.to_vec(),
        }
    }
}

/// A grid of `values` at `x`, each input held to `0..=1` (10.14.2.4),
/// interpolated multilinearly in `f64`.
fn interpolate(grids: &[usize], outputs: usize, values: &[f64], x: &[f64]) -> Vec<f64> {
    let inputs = grids.len();
    let mut base = Vec::with_capacity(inputs);
    let mut fraction = Vec::with_capacity(inputs);
    for (axis, &points) in grids.iter().enumerate() {
        let v = x.get(axis).copied().unwrap_or(0.0);
        let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        let p = v * (points.max(2) - 1) as f64;
        let index = (p.floor() as usize).min(points.max(2) - 2);
        base.push(index);
        fraction.push(p - index as f64);
    }
    (0..outputs)
        .map(|channel| {
            let mut sum = 0.0;
            for corner in 0..1usize << inputs {
                let mut weight = 1.0;
                let mut offset = 0usize;
                for axis in 0..inputs {
                    let high = (corner >> (inputs - 1 - axis)) & 1;
                    weight *= if high == 1 {
                        fraction[axis]
                    } else {
                        1.0 - fraction[axis]
                    };
                    offset = offset * grids[axis] + (base[axis] + high).min(grids[axis] - 1);
                }
                if weight != 0.0 {
                    sum += weight
                        * values
                            .get(offset * outputs + channel)
                            .copied()
                            .unwrap_or(0.0);
                }
            }
            sum
        })
        .collect()
}

/// One operation of a program.
#[derive(Clone, Debug, PartialEq)]
enum Op {
    /// `data`: a float32 pushed.
    Push(f64),
    /// A channel vector operation: `in`, `out`, `tget`, `tput` or `tsav` over
    /// channels `start ..= start + extra`.
    Channels {
        kind: Channel,
        start: usize,
        extra: usize,
    },
    /// `env`: a CMM environment variable and whether it is there.
    Environment([u8; 4]),
    /// A sub-element invocation: position `index`'s sub-element, of the type
    /// `kind` names.
    Invoke { kind: Invoke, index: usize },
    /// A stack operation over `size + 1` values, `times + 1` times.
    Stack {
        kind: StackOp,
        size: usize,
        times: usize,
    },
    /// `solv`: `y = A x` solved for `x`, `A` of `rows` by `columns`.
    Solve { rows: usize, columns: usize },
    /// `tran`: a matrix of `rows` by `columns` transposed.
    Transpose { rows: usize, columns: usize },
    /// A sequence function over `count` values to one.
    Sequence { kind: SequenceOp, count: usize },
    /// A vector function over vectors of `size` values.
    Vector { kind: VectorOp, size: usize },
    /// `if`, with its stream, and an `else` with its.
    If {
        then: Vec<Op>,
        otherwise: Option<Vec<Op>>,
    },
    /// `sel`, with its `case` streams and a `dflt` with its.
    Select {
        cases: Vec<Vec<Op>>,
        default: Option<Vec<Op>>,
    },
}

/// What a sub-element invocation requires its sub-element to be (Table 94):
/// `curv` a curve set, `mtx` a matrix, `clut` a grid, `calc` a calculator,
/// `elem` any element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Invoke {
    Curves,
    Matrix,
    Grid,
    Calculator,
    Any,
}

impl Invoke {
    /// Whether `sub` is of the type this invocation requires.
    fn admits(self, sub: &Sub) -> bool {
        match self {
            Invoke::Curves => matches!(sub, Sub::Curves { .. }),
            Invoke::Matrix => matches!(sub, Sub::Matrix { .. }),
            Invoke::Grid => matches!(sub, Sub::Grid { .. }),
            Invoke::Calculator => matches!(sub, Sub::Calculator(_)),
            Invoke::Any => true,
        }
    }
}

/// Which channels a channel operation reads or writes (Table 89).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    In,
    Out,
    Get,
    Put,
    Save,
}

/// The stack operations (Table 96).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StackOp {
    Copy,
    RotateLeft,
    RotateRight,
    Duplicate,
    Flip,
    Pop,
}

/// The sequence functions (Table 100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SequenceOp {
    Sum,
    Product,
    Min,
    Max,
    And,
    Or,
}

/// The vector functions (Table 102), by how many vectors they take and give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VectorOp {
    // None in, none out, or the stack's count, or one constant out.
    Noop,
    Count,
    Pi,
    Infinity,
    NegativeInfinity,
    NotANumber,
    // Two vectors to one.
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    ArcTangent2,
    Less,
    LessOrEqual,
    Equal,
    Near,
    GreaterOrEqual,
    Greater,
    Minimum,
    Maximum,
    And,
    ExclusiveOr,
    Or,
    // A vector and a scalar to a vector.
    Gamma,
    AddScalar,
    SubtractScalar,
    MultiplyScalar,
    DivideScalar,
    // One vector to one.
    Square,
    SquareRoot,
    Cube,
    CubeRoot,
    Absolute,
    Negate,
    Round,
    Floor,
    Ceiling,
    Truncate,
    Sign,
    Exponential,
    Log10,
    NaturalLog,
    Not,
    Sine,
    Cosine,
    Tangent,
    ArcSine,
    ArcCosine,
    ArcTangent,
    Real,
    // Two vectors to two.
    CartesianToPolar,
    PolarToCartesian,
    // Three vectors to three.
    ToLab,
    ToXyz,
}

impl VectorOp {
    /// The operation a signature names, or `None`.
    fn of(signature: &[u8; 4]) -> Option<VectorOp> {
        Some(match signature {
            b"noop" => VectorOp::Noop,
            b"scnt" => VectorOp::Count,
            b"pi  " => VectorOp::Pi,
            b"+INF" => VectorOp::Infinity,
            b"-INF" => VectorOp::NegativeInfinity,
            b"NaN " => VectorOp::NotANumber,
            b"add " => VectorOp::Add,
            b"sub " => VectorOp::Subtract,
            b"mul " => VectorOp::Multiply,
            b"div " => VectorOp::Divide,
            b"mod " => VectorOp::Modulo,
            b"pow " => VectorOp::Power,
            b"atn2" => VectorOp::ArcTangent2,
            b"lt  " => VectorOp::Less,
            b"le  " => VectorOp::LessOrEqual,
            b"eq  " => VectorOp::Equal,
            b"near" => VectorOp::Near,
            b"ge  " => VectorOp::GreaterOrEqual,
            b"gt  " => VectorOp::Greater,
            b"vmin" => VectorOp::Minimum,
            b"vmax" => VectorOp::Maximum,
            b"vand" => VectorOp::And,
            b"vxor" => VectorOp::ExclusiveOr,
            b"vor " => VectorOp::Or,
            b"gama" => VectorOp::Gamma,
            b"sadd" => VectorOp::AddScalar,
            b"ssub" => VectorOp::SubtractScalar,
            b"smul" => VectorOp::MultiplyScalar,
            b"sdiv" => VectorOp::DivideScalar,
            b"sq  " => VectorOp::Square,
            b"sqrt" => VectorOp::SquareRoot,
            b"cb  " => VectorOp::Cube,
            b"cbrt" => VectorOp::CubeRoot,
            b"abs " => VectorOp::Absolute,
            b"neg " => VectorOp::Negate,
            b"rond" => VectorOp::Round,
            b"flor" => VectorOp::Floor,
            b"ceil" => VectorOp::Ceiling,
            b"trnc" => VectorOp::Truncate,
            b"sign" => VectorOp::Sign,
            b"exp " => VectorOp::Exponential,
            b"log " => VectorOp::Log10,
            b"ln  " => VectorOp::NaturalLog,
            b"not " => VectorOp::Not,
            b"sin " => VectorOp::Sine,
            b"cos " => VectorOp::Cosine,
            b"tan " => VectorOp::Tangent,
            b"asin" => VectorOp::ArcSine,
            b"acos" => VectorOp::ArcCosine,
            b"atan" => VectorOp::ArcTangent,
            b"rnum" => VectorOp::Real,
            b"ctop" => VectorOp::CartesianToPolar,
            b"ptoc" => VectorOp::PolarToCartesian,
            b"tLab" => VectorOp::ToLab,
            b"tXYZ" => VectorOp::ToXyz,
            _ => return None,
        })
    }

    /// Whether 11.2.1 has its size selector be zero: the operations over no
    /// vector, which push a constant or the stack's count or do nothing.
    fn sizeless(self) -> bool {
        matches!(
            self,
            VectorOp::Noop
                | VectorOp::Count
                | VectorOp::Pi
                | VectorOp::Infinity
                | VectorOp::NegativeInfinity
                | VectorOp::NotANumber
        )
    }

    /// The values it takes off the stack and the values it puts on, for
    /// vectors of `n` values.
    fn effect(self, n: usize) -> (usize, usize) {
        use VectorOp::*;
        match self {
            Noop => (0, 0),
            Count | Pi | Infinity | NegativeInfinity | NotANumber => (0, 1),
            Add | Subtract | Multiply | Divide | Modulo | Power | ArcTangent2 | Less
            | LessOrEqual | Equal | Near | GreaterOrEqual | Greater | Minimum | Maximum | And
            | ExclusiveOr | Or => (2 * n, n),
            Gamma | AddScalar | SubtractScalar | MultiplyScalar | DivideScalar => (n + 1, n),
            CartesianToPolar | PolarToCartesian => (2 * n, 2 * n),
            ToLab | ToXyz => (3 * n, 3 * n),
            _ => (n, n),
        }
    }
}

impl Calculator {
    /// The calculator element `element`, read and validated, held to this
    /// module's caps and its own `clmt` where that is stricter, its
    /// sub-elements' tables and its sampled calculator curves' runs spent
    /// from `budget`, its tag's.
    ///
    /// # Errors
    ///
    /// [`IccError::CalculatorLimit`] for a program past a cap, or a sampled
    /// calculator curve whose runs pass the steps `budget` has left;
    /// [`IccError::TooLarge`] for sub-element tables past the entries it has
    /// left; [`IccError::UnsupportedElement`] for an operation, or a
    /// sub-element, this build does not run, by its signature;
    /// [`IccError::MalformedTag`] for a program 11.2.1 does not allow: an
    /// operation it does not define, a channel index out of range, a stack
    /// taken below what is on it, streams leaving it at different depths, a
    /// sub-element of the wrong type or one outside the element, overlapping
    /// another or the main function, or a curve set's curves overlapping one
    /// another.
    pub(super) fn read(element: &[u8], budget: &mut Budget) -> Result<Calculator, IccError> {
        let mut spent = 0usize;
        Calculator::read_within(element, Limits::CAPS, &mut spent, 0, budget)
    }

    /// The calculator `element` within `limits`, its operations added to
    /// `spent`, sitting `depth` sub-elements deep, its sub-elements spent
    /// from `budget`.
    fn read_within(
        element: &[u8],
        limits: Limits,
        spent: &mut usize,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<Calculator, IccError> {
        if depth > MAX_CALC_DEPTH {
            return Err(IccError::CalculatorLimit(CalculatorLimit::Depth));
        }
        let reader = Reader { bytes: element };
        if reader.sig(0) != Some(*b"calc") {
            return Err(IccError::MalformedTag);
        }
        let inputs = usize::from(reader.u16(8).ok_or(IccError::MalformedTag)?);
        let outputs = usize::from(reader.u16(10).ok_or(IccError::MalformedTag)?);
        let count = usize::from(reader.u16(14).ok_or(IccError::MalformedTag)?);
        let position = |at: usize| -> Result<(usize, usize), IccError> {
            let offset = reader.u32(at).ok_or(IccError::MalformedTag)? as usize;
            let size = reader.u32(at + 4).ok_or(IccError::MalformedTag)? as usize;
            let end = offset.checked_add(size).ok_or(IccError::MalformedTag)?;
            if end > element.len() {
                return Err(IccError::MalformedTag);
            }
            Ok((offset, end))
        };
        let main = position(16)?;
        let table_end = 24 + 8 * count;
        if table_end > element.len() {
            return Err(IccError::MalformedTag);
        }
        // 11.2.1, Table 85b: a `clmt` after the positions states limits for
        // the element and its sub-calculators.
        let (limits, data_start) = if reader.sig(table_end) == Some(*b"clmt") {
            let field = |at: usize| reader.u32(table_end + at).ok_or(IccError::MalformedTag);
            (
                limits.within(field(8)?, field(12)?, field(16)?),
                table_end + 28,
            )
        } else {
            (limits, table_end)
        };
        if main.0 < data_start {
            return Err(IccError::MalformedTag);
        }

        // The sub-elements: one each distinct position, inside the element,
        // after its header, apart from the main function and from one
        // another (11.2.1), so reading them all reads the element's bytes
        // once.
        let mut distinct: Vec<(usize, usize)> = Vec::new();
        let mut positions = Vec::with_capacity(count);
        for index in 0..count {
            let place = position(24 + 8 * index)?;
            let found = match distinct.iter().position(|&other| other == place) {
                Some(found) => found,
                None => {
                    distinct.push(place);
                    distinct.len() - 1
                }
            };
            positions.push(found);
        }
        let mut regions: Vec<(usize, usize)> = distinct.clone();
        regions.push(main);
        regions.sort_unstable();
        for pair in regions.windows(2) {
            if pair[0].1 > pair[1].0 {
                return Err(IccError::MalformedTag);
            }
        }
        if regions
            .first()
            .is_some_and(|&(start, _)| start < data_start)
        {
            return Err(IccError::MalformedTag);
        }
        let mut subs = Vec::with_capacity(distinct.len());
        for &(start, end) in &distinct {
            subs.push(sub_element(
                &element[start..end],
                limits,
                spent,
                depth,
                budget,
            )?);
        }

        // The main function: `func`, four reserved bytes, the count, then
        // eight bytes an operation — the count held to the cap before the
        // operations are read.
        let function = &element[main.0..main.1];
        let function_reader = Reader { bytes: function };
        if function_reader.sig(0) != Some(*b"func") {
            return Err(IccError::MalformedTag);
        }
        let operations = function_reader.u32(8).ok_or(IccError::MalformedTag)? as usize;
        if operations > limits.operations || spent.saturating_add(operations) > limits.operations {
            return Err(IccError::CalculatorLimit(CalculatorLimit::Operations));
        }
        if operations
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(12))
            .is_none_or(|end| end > function.len())
        {
            return Err(IccError::MalformedTag);
        }
        *spent += operations;
        let mut parser = Parser {
            reader: function_reader,
            at: 12,
        };
        let main_ops = parser.stream(operations, 0)?;

        let (deepest, temporaries, steps) = {
            let mut check = Check {
                inputs,
                outputs,
                subs: &subs,
                positions: &positions,
                limits,
                deepest: 0,
                temporaries: 0,
            };
            let (_, steps) = check.stream(&main_ops, 0)?;
            (check.deepest, check.temporaries, steps)
        };
        Ok(Calculator {
            inputs,
            outputs,
            main: main_ops,
            subs,
            positions,
            temporaries,
            stack: deepest,
            steps,
        })
    }

    /// How many channels it takes.
    pub(super) fn inputs(&self) -> usize {
        self.inputs
    }

    /// How many channels it gives.
    pub(super) fn outputs(&self) -> usize {
        self.outputs
    }

    /// The steps one run takes at the most, no more than
    /// [`MAX_CALC_OPERATIONS`].
    pub(super) fn steps(&self) -> usize {
        self.steps
    }

    /// The program run over `inputs`, a missing input zero: its output
    /// channels, each zero until the program sets it.
    pub(super) fn evaluate(&self, inputs: &[f64]) -> Vec<f64> {
        let mut machine = Machine {
            calculator: self,
            inputs,
            stack: Vec::with_capacity(self.stack),
            temporaries: vec![0.0; self.temporaries],
            outputs: vec![0.0; self.outputs],
        };
        machine.run(&self.main);
        machine.outputs
    }
}

/// A sub-element of a calculator, read from `data`, the tables it holds
/// spent from `budget`.
fn sub_element(
    data: &[u8],
    limits: Limits,
    spent: &mut usize,
    depth: usize,
    budget: &mut Budget,
) -> Result<Sub, IccError> {
    let reader = Reader { bytes: data };
    let signature = reader.sig(0).ok_or(IccError::MalformedTag)?;
    let p = usize::from(reader.u16(8).ok_or(IccError::MalformedTag)?);
    let q = usize::from(reader.u16(10).ok_or(IccError::MalformedTag)?);
    if p == 0 || q == 0 {
        return Err(IccError::MalformedTag);
    }
    Ok(match &signature {
        b"cvst" => {
            if p != q || p > super::MAX_LUT_INPUTS {
                return Err(IccError::MalformedTag);
            }
            // Each channel's curve's place. Channels naming one place share
            // its curve (11.2.2.1), and the curves are otherwise stored one
            // after another, so no place overlaps another: each is read
            // once, as the calculator's own sub-elements are.
            let mut places = Vec::with_capacity(p);
            for channel in 0..p {
                let at = 12 + 8 * channel;
                let offset = reader.u32(at).ok_or(IccError::MalformedTag)? as usize;
                let size = reader.u32(at + 4).ok_or(IccError::MalformedTag)? as usize;
                let end = offset.checked_add(size).ok_or(IccError::MalformedTag)?;
                if end > data.len() {
                    return Err(IccError::MalformedTag);
                }
                places.push((offset, end));
            }
            let mut distinct: Vec<(usize, usize)> = Vec::with_capacity(p);
            let mut channels = Vec::with_capacity(p);
            for &place in &places {
                let found = match distinct.iter().position(|&other| other == place) {
                    Some(found) => found,
                    None => {
                        distinct.push(place);
                        distinct.len() - 1
                    }
                };
                channels.push(found);
            }
            let mut sorted = distinct.clone();
            sorted.sort_unstable();
            if sorted.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                return Err(IccError::MalformedTag);
            }
            let mut curves = Vec::with_capacity(distinct.len());
            for &(start, end) in &distinct {
                // A sampled calculator curve's calculator sits a level
                // deeper, within this element's limits and count.
                let curve = mpe::read_curve(&data[start..end], budget, &mut |bytes, budget| {
                    Calculator::read_within(bytes, limits, spent, depth + 1, budget)
                })?;
                budget.spend_entries(curve.held())?;
                curves.push(curve);
            }
            Sub::Curves { curves, channels }
        }
        b"matf" => {
            if p > super::MAX_LUT_INPUTS || q > super::MAX_LUT_INPUTS {
                return Err(IccError::TooLarge);
            }
            budget.spend_entries(p * q + q)?;
            Sub::Matrix {
                inputs: p,
                outputs: q,
                values: mpe::matrix_values(data, p, q)?,
            }
        }
        b"clut" | b"xclt" => {
            if p > super::MAX_LUT_INPUTS || q > super::MAX_LUT_INPUTS {
                return Err(IccError::TooLarge);
            }
            let (grids, values) = mpe::grid_values(data, p, q, budget)?;
            Sub::Grid {
                grids,
                outputs: q,
                values,
            }
        }
        b"calc" => Sub::Calculator(Box::new(Calculator::read_within(
            data,
            limits,
            spent,
            depth + 1,
            budget,
        )?)),
        b"bACS" | b"eACS" if p == q => Sub::Placeholder(p),
        b"bACS" | b"eACS" => return Err(IccError::MalformedTag),
        other => return Err(IccError::UnsupportedElement(*other)),
    })
}

/// A program's operations, read in order from a function's bytes.
struct Parser<'a> {
    reader: Reader<'a>,
    /// The byte of the next operation.
    at: usize,
}

impl Parser<'_> {
    /// The next operation's signature and its four data bytes.
    fn next(&mut self) -> Result<([u8; 4], [u8; 4]), IccError> {
        let signature = self.reader.sig(self.at).ok_or(IccError::MalformedTag)?;
        let data = self.reader.sig(self.at + 4).ok_or(IccError::MalformedTag)?;
        self.at += 8;
        Ok((signature, data))
    }

    /// The signature of the operation after the next `ahead`, without
    /// reading it.
    fn peek(&self, ahead: usize) -> Option<[u8; 4]> {
        self.reader.sig(self.at + 8 * ahead)
    }

    /// A stream of `slots` operation slots — a conditional's streams among
    /// them — nested `nesting` streams deep.
    fn stream(&mut self, slots: usize, nesting: usize) -> Result<Vec<Op>, IccError> {
        if nesting > MAX_CALC_DEPTH {
            return Err(IccError::CalculatorLimit(CalculatorLimit::Depth));
        }
        let end = self.at + 8 * slots;
        let mut ops = Vec::new();
        while self.at < end {
            let remaining = (end - self.at) / 8;
            let (signature, data) = self.next()?;
            let word = u32::from_be_bytes(data);
            let (s, t) = (
                usize::from(u16::from_be_bytes([data[0], data[1]])),
                usize::from(u16::from_be_bytes([data[2], data[3]])),
            );
            let op = match &signature {
                b"data" => Op::Push(f64::from(f32::from_bits(word))),
                b"in  " | b"out " | b"tget" | b"tput" | b"tsav" => Op::Channels {
                    kind: match &signature {
                        b"in  " => Channel::In,
                        b"out " => Channel::Out,
                        b"tget" => Channel::Get,
                        b"tput" => Channel::Put,
                        _ => Channel::Save,
                    },
                    start: s,
                    extra: t,
                },
                b"env " => Op::Environment(data),
                b"curv" | b"mtx " | b"clut" | b"calc" | b"elem" => {
                    if t != 0 {
                        return Err(IccError::MalformedTag);
                    }
                    let kind = match &signature {
                        b"curv" => Invoke::Curves,
                        b"mtx " => Invoke::Matrix,
                        b"clut" => Invoke::Grid,
                        b"calc" => Invoke::Calculator,
                        _ => Invoke::Any,
                    };
                    Op::Invoke { kind, index: s }
                }
                // Tint arrays and the colour appearance operations are
                // outside what this engine reads; refused by name.
                b"tint" | b"tJab" | b"fJab" | b"tJa`" | b"fJa`" => {
                    return Err(IccError::UnsupportedElement(signature))
                }
                b"copy" | b"rotl" | b"rotr" | b"posd" | b"flip" | b"pop " => {
                    let kind = match &signature {
                        b"copy" => StackOp::Copy,
                        b"rotl" => StackOp::RotateLeft,
                        b"rotr" => StackOp::RotateRight,
                        b"posd" => StackOp::Duplicate,
                        b"flip" => StackOp::Flip,
                        _ => StackOp::Pop,
                    };
                    if matches!(kind, StackOp::Flip | StackOp::Pop) && t != 0 {
                        return Err(IccError::MalformedTag);
                    }
                    Op::Stack {
                        kind,
                        size: s,
                        times: t,
                    }
                }
                b"solv" => Op::Solve {
                    rows: s + 1,
                    columns: t + 1,
                },
                b"tran" => Op::Transpose {
                    rows: s + 1,
                    columns: t + 1,
                },
                b"sum " | b"prod" | b"min " | b"max " | b"and " | b"or  " => {
                    if t != 0 {
                        return Err(IccError::MalformedTag);
                    }
                    Op::Sequence {
                        kind: match &signature {
                            b"sum " => SequenceOp::Sum,
                            b"prod" => SequenceOp::Product,
                            b"min " => SequenceOp::Min,
                            b"max " => SequenceOp::Max,
                            b"and " => SequenceOp::And,
                            _ => SequenceOp::Or,
                        },
                        count: s + 2,
                    }
                }
                b"if  " => {
                    // An `else` immediately after the `if`, both headers
                    // before their streams (Table 104).
                    let then = word as usize;
                    let otherwise = if remaining > 1 && self.peek(0) == Some(*b"else") {
                        let (_, data) = self.next()?;
                        Some(u32::from_be_bytes(data) as usize)
                    } else {
                        None
                    };
                    let headers = 1 + usize::from(otherwise.is_some());
                    let needed = then
                        .checked_add(otherwise.unwrap_or(0))
                        .and_then(|streams| streams.checked_add(headers));
                    if needed.is_none_or(|needed| needed > remaining) {
                        return Err(IccError::MalformedTag);
                    }
                    let then = self.stream(then, nesting + 1)?;
                    let otherwise = match otherwise {
                        Some(slots) => Some(self.stream(slots, nesting + 1)?),
                        None => None,
                    };
                    Op::If { then, otherwise }
                }
                b"sel " => {
                    // One `case` header or more, then a `dflt`, then the
                    // streams in that order (Tables 105, 106).
                    let mut case_slots = Vec::new();
                    while case_slots.len() + 1 < remaining && self.peek(0) == Some(*b"case") {
                        let (_, data) = self.next()?;
                        case_slots.push(u32::from_be_bytes(data) as usize);
                    }
                    if case_slots.is_empty() {
                        return Err(IccError::MalformedTag);
                    }
                    let default =
                        if case_slots.len() + 1 < remaining && self.peek(0) == Some(*b"dflt") {
                            let (_, data) = self.next()?;
                            Some(u32::from_be_bytes(data) as usize)
                        } else {
                            None
                        };
                    let headers = 1 + case_slots.len() + usize::from(default.is_some());
                    let needed = case_slots
                        .iter()
                        .chain(default.iter())
                        .try_fold(headers, |sum, &slots| sum.checked_add(slots));
                    if needed.is_none_or(|needed| needed > remaining) {
                        return Err(IccError::MalformedTag);
                    }
                    let mut cases = Vec::with_capacity(case_slots.len());
                    for slots in case_slots {
                        cases.push(self.stream(slots, nesting + 1)?);
                    }
                    let default = match default {
                        Some(slots) => Some(self.stream(slots, nesting + 1)?),
                        None => None,
                    };
                    Op::Select { cases, default }
                }
                signature => match VectorOp::of(signature) {
                    Some(kind) => {
                        if t != 0 || (kind.sizeless() && s != 0) {
                            return Err(IccError::MalformedTag);
                        }
                        Op::Vector { kind, size: s + 1 }
                    }
                    // `else`, `case` and `dflt` out of place, and anything
                    // 11.2.1 does not define.
                    None => return Err(IccError::MalformedTag),
                },
            };
            ops.push(op);
        }
        if self.at != end {
            return Err(IccError::MalformedTag);
        }
        Ok(ops)
    }
}

/// A program's validation: the stack's depth walked through it once.
struct Check<'a> {
    inputs: usize,
    outputs: usize,
    subs: &'a [Sub],
    positions: &'a [usize],
    limits: Limits,
    /// The deepest the stack has gone.
    deepest: usize,
    /// The temporary channels reached, one past the highest.
    temporaries: usize,
}

impl Check<'_> {
    /// `ops` from a stack `depth` deep: the depth after them and the steps
    /// they take at the most ([`MAX_CALC_OPERATIONS`]).
    fn stream(&mut self, ops: &[Op], mut depth: usize) -> Result<(usize, usize), IccError> {
        let mut steps = 0usize;
        for op in ops {
            // The values the operation takes and gives, and the steps it
            // takes: one a value it moves, one at the least.
            let (taken, given, spent) = match op {
                Op::Push(_) => (0, 1, 1),
                Op::Channels { kind, start, extra } => {
                    let last = start + extra;
                    let n = extra + 1;
                    match kind {
                        Channel::In if last >= self.inputs => return Err(IccError::MalformedTag),
                        Channel::Out if last >= self.outputs => return Err(IccError::MalformedTag),
                        Channel::Get | Channel::Put | Channel::Save
                            if last >= self.limits.temporaries =>
                        {
                            return Err(IccError::CalculatorLimit(CalculatorLimit::Temporaries))
                        }
                        _ => {}
                    }
                    if matches!(kind, Channel::Get | Channel::Put | Channel::Save) {
                        self.temporaries = self.temporaries.max(last + 1);
                    }
                    match kind {
                        Channel::In | Channel::Get => (0, n, n),
                        Channel::Out | Channel::Put => (n, 0, n),
                        Channel::Save => (n, n, 2 * n),
                    }
                }
                Op::Environment(_) => (0, 2, 2),
                Op::Invoke { kind, index } => {
                    let sub = self
                        .positions
                        .get(*index)
                        .and_then(|&at| self.subs.get(at))
                        .filter(|sub| kind.admits(sub))
                        .ok_or(IccError::MalformedTag)?;
                    let (p, q) = sub.counts();
                    (p, q, (p + q).saturating_add(sub.steps()))
                }
                Op::Stack { kind, size, times } => {
                    let n = size + 1;
                    match kind {
                        StackOp::Copy => {
                            let given = n.saturating_mul(times + 2);
                            (n, given, given.saturating_add(n))
                        }
                        StackOp::RotateLeft | StackOp::RotateRight => (n, n, 2 * n),
                        // One value read, and `times + 1` copies of it given.
                        StackOp::Duplicate => (n, n + times + 1, times + 2),
                        StackOp::Flip => (n + 1, n + 1, 2 * (n + 1)),
                        StackOp::Pop => (n, 0, n),
                    }
                }
                // The elimination multiplies at most a row by a column for
                // each column.
                Op::Solve { rows, columns } => {
                    let matrix = rows.saturating_mul(*columns);
                    let taken = matrix.saturating_add(*rows);
                    let given = columns + 1;
                    let eliminate = matrix.saturating_mul(*columns);
                    (taken, given, (taken + given).saturating_add(eliminate))
                }
                Op::Transpose { rows, columns } => {
                    let matrix = rows.saturating_mul(*columns);
                    (matrix, matrix, matrix.saturating_mul(2))
                }
                Op::Sequence { count, .. } => (*count, 1, count + 1),
                Op::Vector { kind, size } => {
                    let (taken, given) = kind.effect(*size);
                    (taken, given, (taken + given).max(1))
                }
                // A conditional takes its test, then runs one of its
                // streams: the longer is what it can take.
                Op::If { then, otherwise } => {
                    let below = depth.checked_sub(1).ok_or(IccError::MalformedTag)?;
                    let (after, then_steps) = self.stream(then, below)?;
                    let (other, other_steps) = match otherwise {
                        Some(stream) => self.stream(stream, below)?,
                        None => (below, 0),
                    };
                    // 11.2.1: every stream leaves the stack as deep.
                    if after != other {
                        return Err(IccError::MalformedTag);
                    }
                    depth = after;
                    steps = steps
                        .saturating_add(1)
                        .saturating_add(then_steps.max(other_steps));
                    if steps > MAX_CALC_OPERATIONS {
                        return Err(IccError::CalculatorLimit(CalculatorLimit::Operations));
                    }
                    continue;
                }
                Op::Select { cases, default } => {
                    let below = depth.checked_sub(1).ok_or(IccError::MalformedTag)?;
                    let mut after = None;
                    let mut longest = 0usize;
                    let none = Vec::new();
                    let streams = cases
                        .iter()
                        .chain(default.iter())
                        .chain(default.is_none().then_some(&none));
                    for stream in streams {
                        let (ends, spent) = self.stream(stream, below)?;
                        if after.is_some_and(|after| after != ends) {
                            return Err(IccError::MalformedTag);
                        }
                        after = Some(ends);
                        longest = longest.max(spent);
                    }
                    depth = after.unwrap_or(below);
                    steps = steps.saturating_add(1).saturating_add(longest);
                    if steps > MAX_CALC_OPERATIONS {
                        return Err(IccError::CalculatorLimit(CalculatorLimit::Operations));
                    }
                    continue;
                }
            };
            depth = depth.checked_sub(taken).ok_or(IccError::MalformedTag)?;
            depth = depth.saturating_add(given);
            if depth > self.limits.stack {
                return Err(IccError::CalculatorLimit(CalculatorLimit::Stack));
            }
            self.deepest = self.deepest.max(depth);
            // The steps are this module's cap, not a `clmt`'s: 11.2.1's limit
            // counts the operations a calculator holds, which the read counts.
            steps = steps.saturating_add(spent);
            if steps > MAX_CALC_OPERATIONS {
                return Err(IccError::CalculatorLimit(CalculatorLimit::Operations));
            }
        }
        Ok((depth, steps))
    }
}

/// One run of a program.
struct Machine<'a> {
    calculator: &'a Calculator,
    inputs: &'a [f64],
    stack: Vec<f64>,
    temporaries: Vec<f64>,
    outputs: Vec<f64>,
}

impl Machine<'_> {
    /// The top value, taken off; zero on an empty stack, which validation
    /// rules out.
    fn pop(&mut self) -> f64 {
        self.stack.pop().unwrap_or(0.0)
    }

    /// The top `n` values, taken off, deepest first.
    fn take(&mut self, n: usize) -> Vec<f64> {
        let from = self.stack.len().saturating_sub(n);
        let mut taken = self.stack.split_off(from);
        // Validation rules out a short stack; were one met, the missing
        // values are zeros below the rest.
        while taken.len() < n {
            taken.insert(0, 0.0);
        }
        taken
    }

    /// `ops`, in order.
    fn run(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Push(value) => self.stack.push(*value),
                Op::Channels { kind, start, extra } => {
                    let range = *start..=start + extra;
                    match kind {
                        Channel::In => {
                            for channel in range {
                                let value = self.inputs.get(channel).copied().unwrap_or(0.0);
                                self.stack.push(value);
                            }
                        }
                        Channel::Get => {
                            for channel in range {
                                let value = self.temporaries.get(channel).copied().unwrap_or(0.0);
                                self.stack.push(value);
                            }
                        }
                        // The top value goes to the last channel (Table 89).
                        Channel::Out | Channel::Put | Channel::Save => {
                            let values = self.take(extra + 1);
                            let target = match kind {
                                Channel::Out => &mut self.outputs,
                                _ => &mut self.temporaries,
                            };
                            for (channel, &value) in range.zip(&values) {
                                if let Some(slot) = target.get_mut(channel) {
                                    *slot = value;
                                }
                            }
                            if *kind == Channel::Save {
                                self.stack.extend(values);
                            }
                        }
                    }
                }
                // Table 92: `true` is there and one, `ndef` and every other
                // variable this engine is not given are not there.
                Op::Environment(name) => {
                    let value = f64::from(u8::from(name == b"true"));
                    self.stack.push(value);
                    self.stack.push(value);
                }
                Op::Invoke { index, .. } => {
                    let sub = self
                        .calculator
                        .positions
                        .get(*index)
                        .and_then(|&at| self.calculator.subs.get(at));
                    if let Some(sub) = sub {
                        let (p, _) = sub.counts();
                        let x = self.take(p);
                        let y = sub.apply(&x);
                        self.stack.extend(y);
                    }
                }
                Op::Stack { kind, size, times } => self.stack_op(*kind, size + 1, times + 1),
                Op::Solve { rows, columns } => self.solve(*rows, *columns),
                Op::Transpose { rows, columns } => {
                    let a = self.take(rows * columns);
                    for column in 0..*columns {
                        for row in 0..*rows {
                            self.stack.push(a[row * columns + column]);
                        }
                    }
                }
                Op::Sequence { kind, count } => {
                    let x = self.take(*count);
                    let result = match kind {
                        SequenceOp::Sum => x.iter().sum(),
                        SequenceOp::Product => x.iter().product(),
                        SequenceOp::Min => x.iter().copied().fold(f64::INFINITY, f64::min),
                        SequenceOp::Max => x.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        SequenceOp::And => truth(x.iter().all(|&v| v >= 0.5)),
                        SequenceOp::Or => truth(x.iter().any(|&v| v >= 0.5)),
                    };
                    self.stack.push(result);
                }
                Op::Vector { kind, size } => self.vector(*kind, *size),
                Op::If { then, otherwise } => {
                    let test = self.pop();
                    // 11.2.1.10: at or above one half, or not a number.
                    if test >= 0.5 || test.is_nan() {
                        self.run(then);
                    } else if let Some(stream) = otherwise {
                        self.run(stream);
                    }
                }
                Op::Select { cases, default } => {
                    let selector = round(self.pop());
                    let case = (selector >= 0.0 && selector < cases.len() as f64)
                        .then(|| cases.get(selector as usize))
                        .flatten();
                    match (case, default) {
                        (Some(stream), _) | (None, Some(stream)) => self.run(stream),
                        (None, None) => {}
                    }
                }
            }
        }
    }

    /// A stack operation over the top `n` values, `times` times (Table 96).
    fn stack_op(&mut self, kind: StackOp, n: usize, times: usize) {
        match kind {
            StackOp::Copy => {
                let top = self.take(n);
                for _ in 0..=times {
                    self.stack.extend_from_slice(&top);
                }
            }
            StackOp::RotateLeft | StackOp::RotateRight => {
                let mut top = self.take(n);
                if !top.is_empty() {
                    let by = times % top.len();
                    if kind == StackOp::RotateLeft {
                        top.rotate_left(by);
                    } else {
                        top.rotate_right(by);
                    }
                }
                self.stack.extend(top);
            }
            // The value `n - 1` below the top, pushed `times` times.
            StackOp::Duplicate => {
                let at = self.stack.len().checked_sub(n);
                let value = at.and_then(|at| self.stack.get(at)).copied().unwrap_or(0.0);
                for _ in 0..times {
                    self.stack.push(value);
                }
            }
            StackOp::Flip => {
                let mut top = self.take(n + 1);
                top.reverse();
                self.stack.extend(top);
            }
            StackOp::Pop => {
                self.take(n);
            }
        }
    }

    /// `solv` (Table 98): `y = A x` for `x`, `A` square and invertible, by
    /// Gaussian elimination with partial pivoting; `x` zero and the success
    /// flag zero for a matrix that is not, which 11.2.1 allows a CMM.
    fn solve(&mut self, rows: usize, columns: usize) {
        let y = self.take(rows);
        let a = self.take(rows * columns);
        let mut x = vec![0.0; columns];
        let solved = rows == columns && gauss(a, y, &mut x);
        if !solved {
            x.iter_mut().for_each(|value| *value = 0.0);
        }
        self.stack.extend(x);
        self.stack.push(truth(solved));
    }

    /// A vector function (Table 102) over vectors of `n` values.
    fn vector(&mut self, kind: VectorOp, n: usize) {
        use tinker_pdf_math as math;
        use VectorOp::*;
        let unary = |f: fn(f64) -> f64, machine: &mut Machine<'_>| {
            let x = machine.take(n);
            machine.stack.extend(x.into_iter().map(f));
        };
        let binary = |f: fn(f64, f64) -> f64, machine: &mut Machine<'_>| {
            let y = machine.take(n);
            let x = machine.take(n);
            machine
                .stack
                .extend(x.into_iter().zip(y).map(|(x, y)| f(x, y)));
        };
        let scalar = |f: fn(f64, f64) -> f64, machine: &mut Machine<'_>| {
            let y = machine.pop();
            let x = machine.take(n);
            machine.stack.extend(x.into_iter().map(|x| f(x, y)));
        };
        match kind {
            Noop => {}
            Count => {
                let count = self.stack.len() as f64;
                self.stack.push(count);
            }
            Pi => self.stack.push(std::f64::consts::PI),
            Infinity => self.stack.push(f64::INFINITY),
            NegativeInfinity => self.stack.push(f64::NEG_INFINITY),
            NotANumber => self.stack.push(f64::NAN),
            Add => binary(|x, y| x + y, self),
            Subtract => binary(|x, y| x - y, self),
            Multiply => binary(|x, y| x * y, self),
            Divide => binary(|x, y| x / y, self),
            Modulo => binary(|x, y| x - (x / y).trunc() * y, self),
            Power => binary(math::pow, self),
            ArcTangent2 => binary(|x, y| math::atan2(y, x), self),
            Less => binary(|x, y| truth(x < y), self),
            LessOrEqual => binary(|x, y| truth(x <= y), self),
            Equal => binary(|x, y| truth(x == y), self),
            Near => binary(|x, y| truth(y - 1e-8 <= x && x <= y + 1e-8), self),
            GreaterOrEqual => binary(|x, y| truth(x >= y), self),
            Greater => binary(|x, y| truth(x > y), self),
            Minimum => binary(f64::min, self),
            Maximum => binary(f64::max, self),
            And => binary(|x, y| truth(x >= 0.5 && y >= 0.5), self),
            ExclusiveOr => binary(|x, y| truth((x >= 0.5) != (y >= 0.5)), self),
            Or => binary(|x, y| truth(x >= 0.5 || y >= 0.5), self),
            Gamma => scalar(math::pow, self),
            AddScalar => scalar(|x, y| x + y, self),
            SubtractScalar => scalar(|x, y| x - y, self),
            MultiplyScalar => scalar(|x, y| x * y, self),
            DivideScalar => scalar(|x, y| x / y, self),
            Square => unary(|x| x * x, self),
            SquareRoot => unary(f64::sqrt, self),
            Cube => unary(|x| x * x * x, self),
            CubeRoot => unary(math::cbrt, self),
            Absolute => unary(f64::abs, self),
            Negate => unary(|x| -x, self),
            Round => unary(round, self),
            Floor => unary(f64::floor, self),
            Ceiling => unary(f64::ceil, self),
            Truncate => unary(f64::trunc, self),
            // Table 102 prints -1 for a positive value as well; a sign is
            // +1 there.
            Sign => unary(
                |x| {
                    if x < 0.0 {
                        -1.0
                    } else if x > 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                },
                self,
            ),
            Exponential => unary(math::exp, self),
            Log10 => unary(math::log10, self),
            NaturalLog => unary(math::ln, self),
            Not => unary(|x| truth(x < 0.5), self),
            Sine => unary(math::sin, self),
            Cosine => unary(math::cos, self),
            Tangent => unary(math::tan, self),
            ArcSine => unary(|x| math::atan2(x, (1.0 - x * x).sqrt()), self),
            ArcCosine => unary(|x| math::atan2((1.0 - x * x).sqrt(), x), self),
            ArcTangent => unary(math::atan, self),
            Real => unary(|x| truth(x.is_finite()), self),
            CartesianToPolar => {
                let y = self.take(n);
                let x = self.take(n);
                let radius: Vec<f64> = x
                    .iter()
                    .zip(&y)
                    .map(|(x, y)| (x * x + y * y).sqrt())
                    .collect();
                let angle: Vec<f64> = x
                    .iter()
                    .zip(&y)
                    .map(|(&x, &y)| {
                        let degrees = math::to_degrees(math::atan2(y, x));
                        if degrees < 0.0 {
                            degrees + 360.0
                        } else {
                            degrees
                        }
                    })
                    .collect();
                self.stack.extend(radius);
                self.stack.extend(angle);
            }
            PolarToCartesian => {
                let angle = self.take(n);
                let radius = self.take(n);
                let x: Vec<f64> = radius
                    .iter()
                    .zip(&angle)
                    .map(|(&r, &a)| r * math::cos(math::to_radians(a)))
                    .collect();
                let y: Vec<f64> = radius
                    .iter()
                    .zip(&angle)
                    .map(|(&r, &a)| r * math::sin(math::to_radians(a)))
                    .collect();
                self.stack.extend(x);
                self.stack.extend(y);
            }
            ToLab => {
                let z = self.take(n);
                let y = self.take(n);
                let x = self.take(n);
                let f = |t: f64| {
                    const DELTA: f64 = 6.0 / 29.0;
                    if t > DELTA * DELTA * DELTA {
                        math::cbrt(t)
                    } else {
                        841.0 / 108.0 * t + 4.0 / 29.0
                    }
                };
                let l: Vec<f64> = y.iter().map(|&y| 116.0 * f(y) - 16.0).collect();
                let a: Vec<f64> = x
                    .iter()
                    .zip(&y)
                    .map(|(&x, &y)| 500.0 * (f(x) - f(y)))
                    .collect();
                let b: Vec<f64> = y
                    .iter()
                    .zip(&z)
                    .map(|(&y, &z)| 200.0 * (f(y) - f(z)))
                    .collect();
                self.stack.extend(l);
                self.stack.extend(a);
                self.stack.extend(b);
            }
            ToXyz => {
                let b = self.take(n);
                let a = self.take(n);
                let l = self.take(n);
                let finv = |t: f64| {
                    const DELTA: f64 = 6.0 / 29.0;
                    if t > DELTA {
                        t * t * t
                    } else {
                        108.0 / 841.0 * (t - 4.0 / 29.0)
                    }
                };
                let fy: Vec<f64> = l.iter().map(|&l| (l + 16.0) / 116.0).collect();
                let x: Vec<f64> = fy
                    .iter()
                    .zip(&a)
                    .map(|(&fy, &a)| finv(fy + a / 500.0))
                    .collect();
                let y: Vec<f64> = fy.iter().map(|&fy| finv(fy)).collect();
                let z: Vec<f64> = fy
                    .iter()
                    .zip(&b)
                    .map(|(&fy, &b)| finv(fy - b / 200.0))
                    .collect();
                self.stack.extend(x);
                self.stack.extend(y);
                self.stack.extend(z);
            }
        }
    }
}

/// One for true, zero for false.
fn truth(value: bool) -> f64 {
    f64::from(u8::from(value))
}

/// 11.2.1's rounding (`rond`, Table 102): half away from zero.
fn round(x: f64) -> f64 {
    if x < 0.0 {
        (x - 0.5).trunc()
    } else {
        (x + 0.5).trunc()
    }
}

/// `y = A x` solved for `x` by Gaussian elimination with partial pivoting,
/// `A` square and row-major; false where it is singular or the answer not
/// finite.
fn gauss(mut a: Vec<f64>, mut y: Vec<f64>, x: &mut [f64]) -> bool {
    let n = y.len();
    if a.len() != n * n || x.len() != n {
        return false;
    }
    for column in 0..n {
        let pivot = (column..n)
            .max_by(|&p, &q| a[p * n + column].abs().total_cmp(&a[q * n + column].abs()))
            .unwrap_or(column);
        let size = a[pivot * n + column];
        if size == 0.0 || !size.is_finite() {
            return false;
        }
        if pivot != column {
            for k in 0..n {
                a.swap(pivot * n + k, column * n + k);
            }
            y.swap(pivot, column);
        }
        for row in column + 1..n {
            let factor = a[row * n + column] / a[column * n + column];
            for k in column..n {
                a[row * n + k] -= factor * a[column * n + k];
            }
            y[row] -= factor * y[column];
        }
    }
    for row in (0..n).rev() {
        let mut sum = y[row];
        for k in row + 1..n {
            sum -= a[row * n + k] * x[k];
        }
        x[row] = sum / a[row * n + row];
    }
    x.iter().all(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bytes` read as a calculator element with a whole tag's budget.
    fn read(bytes: &[u8]) -> Result<Calculator, IccError> {
        let mut budget = Budget::TAG;
        Calculator::read(bytes, &mut budget)
    }

    /// One operation's eight bytes: a signature and four data bytes.
    fn op(signature: &[u8; 4], data: [u8; 4]) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(signature);
        out[4..].copy_from_slice(&data);
        out
    }

    /// `data`: `value` pushed, as a float32.
    fn push(value: f64) -> [u8; 8] {
        op(b"data", (value as f32).to_bits().to_be_bytes())
    }

    /// An operation of a 16-bit `s` and `t`.
    fn st(signature: &[u8; 4], s: u16, t: u16) -> [u8; 8] {
        let mut data = [0u8; 4];
        data[..2].copy_from_slice(&s.to_be_bytes());
        data[2..].copy_from_slice(&t.to_be_bytes());
        op(signature, data)
    }

    /// An operation of a 32-bit count: `if`, `else`, `case`, `dflt`.
    fn counted(signature: &[u8; 4], count: usize) -> [u8; 8] {
        op(signature, (count as u32).to_be_bytes())
    }

    /// `if` over `then`, and `else` over `otherwise` where it is given, both
    /// headers before their streams.
    fn if_else(then: &[[u8; 8]], otherwise: Option<&[[u8; 8]]>) -> Vec<[u8; 8]> {
        let mut out = vec![counted(b"if  ", then.len())];
        if let Some(otherwise) = otherwise {
            out.push(counted(b"else", otherwise.len()));
        }
        out.extend_from_slice(then);
        if let Some(otherwise) = otherwise {
            out.extend_from_slice(otherwise);
        }
        out
    }

    /// `sel` over `cases`, and `dflt` over `default` where it is given, the
    /// headers before the streams.
    fn select(cases: &[Vec<[u8; 8]>], default: Option<&[[u8; 8]]>) -> Vec<[u8; 8]> {
        let mut out = vec![op(b"sel ", [0; 4])];
        for case in cases {
            out.push(counted(b"case", case.len()));
        }
        if let Some(default) = default {
            out.push(counted(b"dflt", default.len()));
        }
        for case in cases {
            out.extend_from_slice(case);
        }
        if let Some(default) = default {
            out.extend_from_slice(default);
        }
        out
    }

    /// A calculator element of `inputs` and `outputs` channels whose main
    /// function is `ops`, with sub-elements `subs` and, where given, a `clmt`
    /// of a stack, temporaries and operations limit.
    fn element(
        inputs: u16,
        outputs: u16,
        ops: &[[u8; 8]],
        subs: &[Vec<u8>],
        limits: Option<[u32; 3]>,
    ) -> Vec<u8> {
        let mut out = b"calc".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&inputs.to_be_bytes());
        out.extend_from_slice(&outputs.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&(subs.len() as u16).to_be_bytes());
        let header = 24 + 8 * subs.len() + if limits.is_some() { 28 } else { 0 };
        let function = 12 + 8 * ops.len();
        out.extend_from_slice(&(header as u32).to_be_bytes());
        out.extend_from_slice(&(function as u32).to_be_bytes());
        let at = header + function;
        let mut body = Vec::new();
        for sub in subs {
            while (at + body.len()) % 4 != 0 {
                body.push(0);
            }
            out.extend_from_slice(&((at + body.len()) as u32).to_be_bytes());
            out.extend_from_slice(&(sub.len() as u32).to_be_bytes());
            body.extend_from_slice(sub);
        }
        if let Some([stack, temporaries, operations]) = limits {
            out.extend_from_slice(b"clmt");
            out.extend_from_slice(&[0; 4]);
            for value in [stack, temporaries, operations] {
                out.extend_from_slice(&value.to_be_bytes());
            }
            out.extend_from_slice(&[0; 8]);
        }
        assert_eq!(out.len(), header);
        out.extend_from_slice(b"func");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(ops.len() as u32).to_be_bytes());
        for operation in ops {
            out.extend_from_slice(operation);
        }
        out.extend(body);
        out
    }

    /// The program `ops` over `inputs` channels to `outputs`, read and run
    /// over `x`.
    fn run(inputs: u16, outputs: u16, ops: &[[u8; 8]], x: &[f64]) -> Vec<f64> {
        read(&element(inputs, outputs, ops, &[], None))
            .unwrap_or_else(|error| panic!("{error:?}"))
            .evaluate(x)
    }

    /// `got` is `want`, value for value, within `1e-12` of each's size.
    fn close(got: &[f64], want: &[f64], what: &str) {
        assert_eq!(got.len(), want.len(), "{what}: {got:?} against {want:?}");
        for (g, w) in got.iter().zip(want) {
            let agree =
                g == w || (g.is_nan() && w.is_nan()) || (g - w).abs() <= 1e-12 * w.abs().max(1.0);
            assert!(agree, "{what}: {got:?} against {want:?}");
        }
    }

    /// **Each vector function computes what Table 102 says**, over vectors of
    /// two: arithmetic, powers and roots, rounding, transcendentals, the
    /// comparisons and logic, and the conversions between Cartesian and
    /// polar coordinates and between XYZ and Lab, each recomputed here from
    /// the table's definitions.
    #[test]
    fn each_vector_function_computes_what_table_102_says() {
        let (x, y) = ([0.3, -1.7], [2.5, 0.4]);
        let pair = |signature: &[u8; 4], f: fn(f64, f64) -> f64| {
            let mut ops = vec![
                push(x[0]),
                push(x[1]),
                push(y[0]),
                push(y[1]),
                st(signature, 1, 0),
            ];
            ops.push(st(b"out ", 0, 1));
            let want = [0, 1].map(|i| f(stored(x[i]), stored(y[i])));
            close(
                &run(1, 2, &ops, &[0.0]),
                &want,
                std::str::from_utf8(signature).unwrap_or("?"),
            );
        };
        pair(b"add ", |a, b| a + b);
        pair(b"sub ", |a, b| a - b);
        pair(b"mul ", |a, b| a * b);
        pair(b"div ", |a, b| a / b);
        pair(b"mod ", |a, b| a - (a / b).trunc() * b);
        pair(b"pow ", f64::powf);
        pair(b"atn2", |a, b| b.atan2(a));
        pair(b"lt  ", |a, b| f64::from(u8::from(a < b)));
        pair(b"le  ", |a, b| f64::from(u8::from(a <= b)));
        pair(b"eq  ", |a, b| f64::from(u8::from(a == b)));
        pair(b"ge  ", |a, b| f64::from(u8::from(a >= b)));
        pair(b"gt  ", |a, b| f64::from(u8::from(a > b)));
        pair(b"vmin", f64::min);
        pair(b"vmax", f64::max);
        pair(b"vand", |a, b| f64::from(u8::from(a >= 0.5 && b >= 0.5)));
        pair(b"vxor", |a, b| {
            f64::from(u8::from((a >= 0.5) != (b >= 0.5)))
        });
        pair(b"vor ", |a, b| f64::from(u8::from(a >= 0.5 || b >= 0.5)));

        let one = |signature: &[u8; 4], value: f64, f: fn(f64) -> f64| {
            let ops = [push(value), st(signature, 0, 0), st(b"out ", 0, 0)];
            close(
                &run(1, 1, &ops, &[0.0]),
                &[f(stored(value))],
                std::str::from_utf8(signature).unwrap_or("?"),
            );
        };
        for value in [0.6, -2.5, 2.5, 0.0] {
            one(b"sq  ", value, |v| v * v);
            one(b"cb  ", value, |v| v * v * v);
            one(b"abs ", value, f64::abs);
            one(b"neg ", value, |v| -v);
            one(b"rond", value, |v| {
                if v < 0.0 {
                    (v - 0.5).trunc()
                } else {
                    (v + 0.5).trunc()
                }
            });
            one(b"flor", value, f64::floor);
            one(b"ceil", value, f64::ceil);
            one(b"trnc", value, f64::trunc);
            one(b"sign", value, |v| {
                if v < 0.0 {
                    -1.0
                } else if v > 0.0 {
                    1.0
                } else {
                    0.0
                }
            });
            one(b"not ", value, |v| f64::from(u8::from(v < 0.5)));
            one(b"rnum", value, |_| 1.0);
        }
        for value in [0.25, 0.81, 1.7] {
            one(b"sqrt", value, f64::sqrt);
            one(b"cbrt", value, f64::cbrt);
            one(b"exp ", value, f64::exp);
            one(b"log ", value, f64::log10);
            one(b"ln  ", value, f64::ln);
            one(b"sin ", value, f64::sin);
            one(b"cos ", value, f64::cos);
            one(b"tan ", value, f64::tan);
            one(b"atan", value, f64::atan);
        }
        for value in [0.25, -0.81, 1.0] {
            one(b"asin", value, f64::asin);
            one(b"acos", value, f64::acos);
        }

        // A vector and a scalar.
        let scalar = |signature: &[u8; 4], f: fn(f64, f64) -> f64| {
            let ops = [
                push(x[0]),
                push(x[1]),
                push(0.5),
                st(signature, 1, 0),
                st(b"out ", 0, 1),
            ];
            let want = [0, 1].map(|i| f(stored(x[i]), 0.5));
            close(
                &run(1, 2, &ops, &[0.0]),
                &want,
                std::str::from_utf8(signature).unwrap_or("?"),
            );
        };
        scalar(b"sadd", |a, b| a + b);
        scalar(b"ssub", |a, b| a - b);
        scalar(b"smul", |a, b| a * b);
        scalar(b"sdiv", |a, b| a / b);

        // The constants, `scnt`, `noop`, and the real-number check.
        let ops = [
            st(b"pi  ", 0, 0),
            st(b"+INF", 0, 0),
            st(b"-INF", 0, 0),
            st(b"NaN ", 0, 0),
            st(b"noop", 0, 0),
            st(b"scnt", 0, 0),
            st(b"out ", 0, 4),
        ];
        let got = run(1, 5, &ops, &[0.0]);
        close(
            &got,
            &[
                std::f64::consts::PI,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NAN,
                4.0,
            ],
            "the constants and the count",
        );
        let ops = [
            st(b"+INF", 0, 0),
            st(b"NaN ", 0, 0),
            push(1.0),
            st(b"rnum", 2, 0),
            st(b"out ", 0, 2),
        ];
        close(&run(1, 3, &ops, &[0.0]), &[0.0, 0.0, 1.0], "rnum");

        // Cartesian to polar and back, the angle in degrees from 0 to 360.
        let ops = [
            push(-1.0),
            push(3.0),
            push(-1.0),
            push(-4.0),
            st(b"ctop", 1, 0),
            st(b"out ", 0, 3),
        ];
        let got = run(1, 4, &ops, &[0.0]);
        let angle = |x: f64, y: f64| {
            let degrees = y.atan2(x).to_degrees();
            if degrees < 0.0 {
                degrees + 360.0
            } else {
                degrees
            }
        };
        let want = [2.0f64.sqrt(), 5.0, angle(-1.0, -1.0), angle(3.0, -4.0)];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "ctop: {got:?} against {want:?}");
        }
        let ops = [
            push(2.0),
            push(1.0),
            push(30.0),
            push(225.0),
            st(b"ptoc", 1, 0),
            st(b"out ", 0, 3),
        ];
        let got = run(1, 4, &ops, &[0.0]);
        let want = [
            2.0 * 30f64.to_radians().cos(),
            225f64.to_radians().cos(),
            2.0 * 30f64.to_radians().sin(),
            225f64.to_radians().sin(),
        ];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "ptoc: {got:?} against {want:?}");
        }

        // XYZ, normalized to the white, as Lab and back.
        let ops = [
            push(0.2),
            push(0.3),
            push(0.0005),
            st(b"tLab", 0, 0),
            st(b"tXYZ", 0, 0),
            st(b"out ", 0, 2),
        ];
        let got = run(1, 3, &ops, &[0.0]);
        for (g, w) in got.iter().zip([0.2, 0.3, 0.0005].map(stored)) {
            assert!((g - w).abs() < 1e-9, "tLab then tXYZ: {got:?}");
        }
        let ops = [
            push(0.2),
            push(0.3),
            push(0.0005),
            st(b"tLab", 0, 0),
            st(b"out ", 0, 2),
        ];
        let got = run(1, 3, &ops, &[0.0]);
        let f = |t: f64| {
            if t > (6.0f64 / 29.0).powi(3) {
                t.cbrt()
            } else {
                841.0 / 108.0 * t + 4.0 / 29.0
            }
        };
        let (fx, fy, fz) = (f(stored(0.2)), f(stored(0.3)), f(stored(0.0005)));
        let want = [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "tLab: {got:?} against {want:?}");
        }
    }

    /// What a float32 holding `value` reads back as.
    fn stored(value: f64) -> f64 {
        f64::from(value as f32)
    }

    /// **The sequence functions and the stack operations do what Tables 100
    /// and 96 say**: a sum, a product, a minimum, a maximum and the
    /// half-threshold `and` and `or` over three values; `copy`, `rotl`,
    /// `rotr`, `posd`, `flip` and `pop`, their sizes and repeats; the
    /// channel operations, the last channel taking the top value, a
    /// temporary channel zero until set and `tsav` leaving its values; and
    /// an output never set left zero.
    #[test]
    fn the_sequence_and_stack_operations_do_what_their_tables_say() {
        let three = [push(0.2), push(0.7), push(0.5)];
        let sequence = |signature: &[u8; 4], want: f64| {
            let mut ops = three.to_vec();
            ops.push(st(signature, 1, 0));
            ops.push(st(b"out ", 0, 0));
            close(
                &run(1, 1, &ops, &[0.0]),
                &[want],
                std::str::from_utf8(signature).unwrap_or("?"),
            );
        };
        let (a, b, c) = (stored(0.2), stored(0.7), stored(0.5));
        sequence(b"sum ", a + b + c);
        sequence(b"prod", a * b * c);
        sequence(b"min ", a);
        sequence(b"max ", b);
        sequence(b"and ", 0.0);
        sequence(b"or  ", 1.0);

        // The stack as the outputs read it, deepest first.
        let stack = |ops: &[[u8; 8]], count: u16| run(1, count, ops, &[0.0]);
        let with = |tail: &[[u8; 8]], count: u16| {
            let mut ops = vec![push(1.0), push(2.0), push(3.0)];
            ops.extend_from_slice(tail);
            ops.push(st(b"out ", 0, count - 1));
            stack(&ops, count)
        };
        close(
            &with(&[st(b"copy", 1, 1)], 7),
            &[1.0, 2.0, 3.0, 2.0, 3.0, 2.0, 3.0],
            "copy(2, 2)",
        );
        close(&with(&[st(b"rotl", 2, 0)], 3), &[2.0, 3.0, 1.0], "rotl");
        close(
            &with(&[st(b"rotr", 2, 1)], 3),
            &[2.0, 3.0, 1.0],
            "rotr twice",
        );
        close(
            &with(&[st(b"posd", 2, 1)], 5),
            &[1.0, 2.0, 3.0, 1.0, 1.0],
            "posd",
        );
        close(&with(&[st(b"flip", 1, 0)], 3), &[3.0, 2.0, 1.0], "flip");
        close(&with(&[st(b"pop ", 1, 0)], 1), &[1.0], "pop");

        // Channels: the top value to the last channel; `tsav` keeps them.
        let ops = [
            st(b"in  ", 0, 1),
            st(b"tsav", 3, 1),
            st(b"tput", 0, 1),
            st(b"tget", 0, 4),
            st(b"out ", 1, 4),
        ];
        let got = run(2, 6, &ops, &[0.25, 0.75]);
        close(&got, &[0.0, 0.25, 0.75, 0.0, 0.25, 0.75], "channels");
    }

    /// **`solv` and `tran` do what Table 98 says**: a 2 by 2 system solved
    /// with its success flag, a singular one giving zeros and a flag of
    /// zero, and a 2 by 3 matrix transposed.
    #[test]
    fn solve_and_transpose_do_what_table_98_says() {
        // [[2, 1], [1, 3]] x = [3, 5]: x = [0.8, 1.4].
        let ops = [
            push(2.0),
            push(1.0),
            push(1.0),
            push(3.0),
            push(3.0),
            push(5.0),
            st(b"solv", 1, 1),
            st(b"out ", 0, 2),
        ];
        let got = run(1, 3, &ops, &[0.0]);
        for (g, w) in got.iter().zip([0.8, 1.4, 1.0]) {
            assert!((g - w).abs() < 1e-12, "{got:?}");
        }
        let singular = [
            push(1.0),
            push(2.0),
            push(2.0),
            push(4.0),
            push(1.0),
            push(1.0),
            st(b"solv", 1, 1),
            st(b"out ", 0, 2),
        ];
        close(&run(1, 3, &singular, &[0.0]), &[0.0, 0.0, 0.0], "singular");
        let ops = [
            push(1.0),
            push(2.0),
            push(3.0),
            push(4.0),
            push(5.0),
            push(6.0),
            st(b"tran", 1, 2),
            st(b"out ", 0, 5),
        ];
        close(
            &run(1, 6, &ops, &[0.0]),
            &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0],
            "transposed",
        );
    }

    /// **Conditionals and selections run one stream** (11.2.1.10, 11.2.1.11):
    /// `if` at one half or more, or not a number, runs its stream and `else`
    /// the other; `sel` rounds its selector and runs that case, or `dflt` for
    /// one out of range; and the environment answers `true` and nothing
    /// else.
    #[test]
    fn conditionals_and_selections_run_one_stream() {
        let branch = if_else(&[push(1.0)], Some(&[push(2.0)]));
        for (test, want) in [(0.5, 1.0), (0.49, 2.0), (f64::NAN, 1.0), (-3.0, 2.0)] {
            let mut ops = vec![st(b"in  ", 0, 0)];
            ops.extend_from_slice(&branch);
            ops.push(st(b"out ", 0, 0));
            close(&run(1, 1, &ops, &[test]), &[want], &format!("if at {test}"));
        }
        let cases = select(
            &[vec![push(10.0)], vec![push(11.0)], vec![push(12.0)]],
            Some(&[push(99.0)]),
        );
        for (selector, want) in [
            (0.4, 10.0),
            (0.5, 11.0),
            (1.6, 12.0),
            (-0.6, 99.0),
            (3.0, 99.0),
        ] {
            let mut ops = vec![st(b"in  ", 0, 0)];
            ops.extend_from_slice(&cases);
            ops.push(st(b"out ", 0, 0));
            close(
                &run(1, 1, &ops, &[selector]),
                &[want],
                &format!("sel {selector}"),
            );
        }
        // No `dflt`, and cases that leave the stack as it was.
        let quiet = select(&[vec![push(5.0), st(b"tput", 0, 0)]], None);
        let mut ops = vec![st(b"in  ", 0, 0)];
        ops.extend_from_slice(&quiet);
        ops.extend_from_slice(&[st(b"tget", 0, 0), st(b"out ", 0, 0)]);
        close(&run(1, 1, &ops, &[0.0]), &[5.0], "case 0");
        close(&run(1, 1, &ops, &[7.0]), &[0.0], "no case");

        let mut name = [0u8; 4];
        name.copy_from_slice(b"true");
        let ops = [
            op(b"env ", name),
            op(b"env ", *b"ndef"),
            op(b"env ", *b"xx  "),
            st(b"out ", 0, 5),
        ];
        close(
            &run(1, 6, &ops, &[0.0]),
            &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            "the environment",
        );
    }

    /// A one-segment `curf` of ICC.1's type 0, `(a x + b)^γ + c`.
    fn curf(gamma: f64, a: f64, b: f64, c: f64) -> Vec<u8> {
        let mut out = b"curf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(b"parf");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        for value in [gamma, a, b, c] {
            out.extend_from_slice(&(value as f32).to_bits().to_be_bytes());
        }
        out
    }

    /// An element's header and `body`.
    fn header(signature: &[u8; 4], p: u16, q: u16, body: &[u8]) -> Vec<u8> {
        let mut out = signature.to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&p.to_be_bytes());
        out.extend_from_slice(&q.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// Floats as float32 bytes.
    fn floats(values: &[f64]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|&v| (v as f32).to_bits().to_be_bytes())
            .collect()
    }

    /// **Sub-elements run as the operation names them** (11.2.1.5): a curve
    /// set through `curv`, a matrix through `mtx`, a grid through `clut`, a
    /// calculator through `calc` and any through `elem`, each taking its
    /// inputs off the stack and putting its outputs on; an operation naming
    /// a sub-element of another type, or a position past the table, is
    /// malformed, and a sub-element of a type this build does not read is
    /// refused by name.
    #[test]
    fn sub_elements_run_as_the_operation_names_them() {
        let curve = curf(2.0, 1.0, 0.0, 0.1);
        let mut curves = header(b"cvst", 1, 1, &20u32.to_be_bytes());
        curves.extend_from_slice(&(curve.len() as u32).to_be_bytes());
        curves.extend_from_slice(&curve);
        let matrix = header(b"matf", 2, 1, &floats(&[0.5, 0.25, 0.1]));
        let mut counts = [0u8; 16];
        counts[0] = 2;
        let mut grid = header(b"clut", 1, 1, &counts);
        grid.extend(floats(&[0.2, 0.6]));
        let inner = element(
            1,
            1,
            &[st(b"in  ", 0, 0), st(b"sq  ", 0, 0), st(b"out ", 0, 0)],
            &[],
            None,
        );
        let subs = [curves, matrix, grid, inner];
        let ops = [
            st(b"in  ", 0, 0),
            st(b"curv", 0, 0),
            st(b"in  ", 0, 1),
            st(b"mtx ", 1, 0),
            st(b"in  ", 0, 0),
            st(b"clut", 2, 0),
            st(b"in  ", 0, 0),
            st(b"calc", 3, 0),
            st(b"in  ", 0, 0),
            st(b"elem", 3, 0),
            st(b"out ", 0, 4),
        ];
        let calculator = read(&element(2, 5, &ops, &subs, None)).expect("the program reads");
        let (x, y) = (0.4, 0.8);
        let got = calculator.evaluate(&[x, y]);
        let want = [
            x * x + stored(0.1),
            0.5 * x + 0.25 * y + stored(0.1),
            stored(0.2) + (stored(0.6) - stored(0.2)) * x,
            x * x,
            x * x,
        ];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "{got:?} against {want:?}");
        }

        for (ops, what) in [
            (
                vec![st(b"in  ", 0, 0), st(b"mtx ", 0, 0), st(b"out ", 0, 0)],
                "a curve set as a matrix",
            ),
            (
                vec![st(b"in  ", 0, 0), st(b"curv", 9, 0), st(b"out ", 0, 0)],
                "a position past the table",
            ),
        ] {
            assert_eq!(
                read(&element(1, 1, &ops, &subs[..1], None)),
                Err(IccError::MalformedTag),
                "{what}"
            );
        }
        let appearance = header(b"JtoX", 3, 3, &[0; 16]);
        assert_eq!(
            read(&element(
                1,
                1,
                &[push(0.0), st(b"out ", 0, 0)],
                &[appearance],
                None
            )),
            Err(IccError::UnsupportedElement(*b"JtoX"))
        );
    }

    /// **A program is validated before it runs** (11.2.1): an operation
    /// 11.2.1 does not define, an input or output index out of range, a stack
    /// taken below what is on it, conditional streams leaving it at
    /// different depths, a `sel` with no `case`, an `else` out of place, a
    /// size selector where the table has it zero, an operation count its
    /// bytes cannot hold, and sub-elements overlapping one another or the
    /// main function are each malformed; tint arrays and the colour
    /// appearance operations are refused by name.
    #[test]
    fn a_program_is_validated_before_it_runs() {
        let malformed: Vec<(Vec<[u8; 8]>, &str)> = vec![
            (
                vec![op(b"zzzz", [0; 4])],
                "an operation 11.2.1 does not define",
            ),
            (vec![st(b"in  ", 1, 0), st(b"out ", 0, 0)], "input 1 of one"),
            (vec![push(0.0), st(b"out ", 1, 0)], "output 1 of one"),
            (
                vec![st(b"add ", 0, 0), st(b"out ", 0, 0)],
                "an add on an empty stack",
            ),
            (
                [
                    vec![push(1.0)],
                    if_else(&[push(1.0)], Some(&[])),
                    vec![st(b"out ", 0, 0)],
                ]
                .concat(),
                "streams leaving the stack at different depths",
            ),
            (
                vec![push(1.0), op(b"sel ", [0; 4]), push(2.0)],
                "a sel with no case",
            ),
            (
                vec![counted(b"else", 0), push(1.0), st(b"out ", 0, 0)],
                "an else out of place",
            ),
            (vec![st(b"pi  ", 1, 0), st(b"out ", 0, 0)], "pi of a size"),
            (
                vec![push(1.0), push(2.0), st(b"flip", 0, 1)],
                "flip repeated",
            ),
            (
                vec![push(1.0), counted(b"if  ", 5), push(1.0)],
                "an if past its function",
            ),
        ];
        for (ops, what) in malformed {
            assert_eq!(
                read(&element(1, 1, &ops, &[], None)),
                Err(IccError::MalformedTag),
                "{what}"
            );
        }
        for signature in [b"tint", b"tJab", b"fJab"] {
            assert_eq!(
                read(&element(1, 1, &[st(signature, 0, 0)], &[], None)),
                Err(IccError::UnsupportedElement(*signature))
            );
        }

        // Two sub-elements sharing bytes without sharing a position.
        let matrix = header(b"matf", 1, 1, &floats(&[1.0, 0.0]));
        let mut overlapping = element(
            1,
            1,
            &[push(0.0), st(b"out ", 0, 0)],
            &[matrix.clone(), matrix],
            None,
        );
        let second = 24 + 8;
        let first_offset = u32::from_be_bytes([
            overlapping[24],
            overlapping[25],
            overlapping[26],
            overlapping[27],
        ]);
        overlapping[second..second + 4].copy_from_slice(&(first_offset + 4).to_be_bytes());
        assert_eq!(read(&overlapping), Err(IccError::MalformedTag));
        // A main function whose operations its bytes cannot hold.
        let mut short = element(1, 1, &[push(0.0), st(b"out ", 0, 0)], &[], None);
        short[20..24].copy_from_slice(&20u32.to_be_bytes());
        assert_eq!(read(&short), Err(IccError::MalformedTag));
    }

    /// **A calculator past the operation cap is refused before it runs**
    /// (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs` names
    /// as proving [`MAX_CALC_OPERATIONS`]: a main function declaring one
    /// operation past the cap, and four billion, with none of their bytes
    /// there, is `CalculatorLimit::Operations` before an operation is read,
    /// where a reader that walked first would answer `MalformedTag`; one
    /// declaring the cap is admitted to the walk. A program of exactly the
    /// cap reads and runs; a sub-calculator of 256 operations invoked 257
    /// times makes a run past it; and a `clmt` of ten operations refuses
    /// eleven.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_calculator_past_the_operation_cap_is_refused_before_it_runs() {
        let declaring = |count: u32| {
            let mut bytes = element(1, 1, &[], &[], None);
            // The main function starts at byte 24, its count at 32.
            bytes[32..36].copy_from_slice(&count.to_be_bytes());
            bytes
        };
        for count in [MAX_CALC_OPERATIONS as u32 + 1, u32::MAX] {
            assert_eq!(
                read(&declaring(count)),
                Err(IccError::CalculatorLimit(CalculatorLimit::Operations)),
                "{count}"
            );
        }
        assert_eq!(
            read(&declaring(MAX_CALC_OPERATIONS as u32)),
            Err(IccError::MalformedTag),
            "the cap itself is walked, and its operations are not there"
        );

        let mut full = Vec::with_capacity(MAX_CALC_OPERATIONS);
        for _ in 0..MAX_CALC_OPERATIONS / 2 - 1 {
            full.extend_from_slice(&[push(0.5), st(b"pop ", 0, 0)]);
        }
        full.extend_from_slice(&[push(0.5), st(b"out ", 0, 0)]);
        assert_eq!(full.len(), MAX_CALC_OPERATIONS);
        let calculator = read(&element(1, 1, &full, &[], None)).expect("exactly the cap");
        assert_eq!(calculator.evaluate(&[0.0]), vec![0.5]);

        let mut inner_ops = vec![st(b"in  ", 0, 0)];
        inner_ops.extend(std::iter::repeat_n(st(b"noop", 0, 0), 254));
        inner_ops.push(st(b"out ", 0, 0));
        let inner = element(1, 1, &inner_ops, &[], None);
        let mut outer = vec![push(0.0)];
        outer.extend(std::iter::repeat_n(st(b"calc", 0, 0), 257));
        outer.push(st(b"out ", 0, 0));
        assert_eq!(
            read(&element(1, 1, &outer, &[inner], None)),
            Err(IccError::CalculatorLimit(CalculatorLimit::Operations)),
            "257 runs of 256 operations"
        );

        let eleven: Vec<[u8; 8]> = [vec![push(0.0); 10], vec![st(b"out ", 0, 0)]].concat();
        assert_eq!(
            read(&element(1, 1, &eleven, &[], Some([0, 0, 10]))),
            Err(IccError::CalculatorLimit(CalculatorLimit::Operations)),
            "a clmt of ten"
        );
    }

    /// **A calculator past the stack cap is refused before it runs** (ruling
    /// 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs` names as
    /// proving [`MAX_CALC_STACK`]: a program pushing one value past the cap
    /// is `CalculatorLimit::Stack`, found by walking it and not by running
    /// it; one pushing exactly the cap reads and runs; and a `clmt` of a
    /// four-value stack refuses a fifth.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_calculator_past_the_stack_cap_is_refused_before_it_runs() {
        let deep = |values: usize| {
            let mut ops = vec![push(0.25); values];
            ops.push(st(b"out ", 0, 0));
            ops
        };
        assert_eq!(
            read(&element(1, 1, &deep(MAX_CALC_STACK + 1), &[], None)),
            Err(IccError::CalculatorLimit(CalculatorLimit::Stack))
        );
        let calculator =
            read(&element(1, 1, &deep(MAX_CALC_STACK), &[], None)).expect("exactly the cap");
        assert_eq!(calculator.evaluate(&[0.0]), vec![0.25]);
        assert_eq!(
            read(&element(1, 1, &deep(5), &[], Some([4, 0, 0]))),
            Err(IccError::CalculatorLimit(CalculatorLimit::Stack)),
            "a clmt of four"
        );
        assert!(read(&element(1, 1, &deep(4), &[], Some([4, 0, 0]))).is_ok());
    }

    /// **A calculator nested past the depth cap is refused before it runs**
    /// (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs` names
    /// as proving [`MAX_CALC_DEPTH`]: conditionals nested one past the cap
    /// are `CalculatorLimit::Depth` while the program is read, and nested to
    /// it read and run; calculators within one another one past it are
    /// refused the same way.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_calculator_nested_past_the_depth_cap_is_refused_before_it_runs() {
        let nested = |levels: usize| {
            let mut stream = vec![push(0.75), st(b"tput", 0, 0)];
            for _ in 0..levels {
                let mut outer = vec![push(1.0)];
                outer.extend(if_else(&stream, None));
                stream = outer;
            }
            stream.extend_from_slice(&[st(b"tget", 0, 0), st(b"out ", 0, 0)]);
            stream
        };
        assert_eq!(
            read(&element(1, 1, &nested(MAX_CALC_DEPTH + 1), &[], None)),
            Err(IccError::CalculatorLimit(CalculatorLimit::Depth))
        );
        let calculator =
            read(&element(1, 1, &nested(MAX_CALC_DEPTH), &[], None)).expect("exactly the cap");
        assert_eq!(calculator.evaluate(&[0.0]), vec![0.75]);

        let calculators = |levels: usize| {
            let pass = [st(b"in  ", 0, 0), st(b"out ", 0, 0)];
            let mut inner = element(1, 1, &pass, &[], None);
            let through = [st(b"in  ", 0, 0), st(b"calc", 0, 0), st(b"out ", 0, 0)];
            for _ in 0..levels {
                inner = element(1, 1, &through, &[inner], None);
            }
            inner
        };
        assert_eq!(
            read(&calculators(MAX_CALC_DEPTH + 1)),
            Err(IccError::CalculatorLimit(CalculatorLimit::Depth))
        );
        let calculator = read(&calculators(MAX_CALC_DEPTH)).expect("exactly the cap");
        assert_eq!(calculator.evaluate(&[0.3]), vec![0.3]);
    }

    /// **A calculator past the temporary cap is refused before it runs**
    /// (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs` names
    /// as proving [`MAX_CALC_TEMPORARIES`]: a program reaching the temporary
    /// channel at the cap's index is `CalculatorLimit::Temporaries`, and one
    /// reaching the channel below it reads and runs, the channel zero until
    /// set; a channel operation's 16-bit start and count may reach further
    /// still, and are refused the same way; and a `clmt` of four temporary
    /// channels refuses the fifth.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_calculator_past_the_temporary_cap_is_refused_before_it_runs() {
        let reaching = |start: u16, extra: u16| {
            vec![
                st(b"tget", start, extra),
                st(b"pop ", extra, 0),
                push(0.5),
                st(b"out ", 0, 0),
            ]
        };
        let cap = MAX_CALC_TEMPORARIES as u16;
        for (start, extra) in [(cap, 0), (0, cap), (u16::MAX, u16::MAX)] {
            assert_eq!(
                read(&element(1, 1, &reaching(start, extra), &[], None)),
                Err(IccError::CalculatorLimit(CalculatorLimit::Temporaries)),
                "tget[{start}, {extra}]"
            );
        }
        let calculator = read(&element(1, 1, &reaching(cap - 1, 0), &[], None))
            .expect("the last channel under the cap");
        assert_eq!(calculator.evaluate(&[0.0]), vec![0.5]);
        assert_eq!(
            read(&element(1, 1, &reaching(4, 0), &[], Some([0, 4, 0]))),
            Err(IccError::CalculatorLimit(CalculatorLimit::Temporaries)),
            "a clmt of four"
        );
    }

    /// A sampled calculator curve (`clcf`, 11.2.2.3) of `entries` entries
    /// from `first` to `last`, clipped past them, sampling `calculator`.
    fn clcf(entries: u32, first: f64, last: f64, calculator: &[u8]) -> Vec<u8> {
        let mut out = b"clcf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&entries.to_be_bytes());
        out.extend_from_slice(&floats(&[first, last]));
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(calculator);
        out
    }

    /// A curve set whose channels name `places` — each an offset from the
    /// set's start and a size — over the curve bytes `body`, which start
    /// right after the positions.
    fn curves_at(places: &[(usize, usize)], body: &[u8]) -> Vec<u8> {
        let channels = places.len() as u16;
        let mut out = header(b"cvst", channels, channels, &[]);
        for &(offset, size) in places {
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(size as u32).to_be_bytes());
        }
        out.extend_from_slice(body);
        out
    }

    /// An `mpet` tag of three channels in and out holding the one element
    /// `element`.
    fn tag_of(element: &[u8]) -> Vec<u8> {
        let mut out = b"mpet".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&24u32.to_be_bytes());
        out.extend_from_slice(&(element.len() as u32).to_be_bytes());
        out.extend_from_slice(element);
        out
    }

    /// **A tag past the calculator work cap is refused before it runs**
    /// (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs` names
    /// as proving [`MAX_CALC_WORK`]: a sampled calculator curve of 4 096
    /// entries whose calculator takes 16 383 steps spends exactly the cap,
    /// one run and its steps an entry, and reads; a step more a run is
    /// `CalculatorLimit::Work`, found before the calculator runs and
    /// spending nothing. The steps are the tag's, shared by its calculators:
    /// a budget a curve leaves too little of refuses the next. A calculator
    /// stage spends a run a node of its grid, so a three-input program of
    /// 1 867 steps — 35 937 runs of 1 868, 67 130 316 — is refused before a
    /// node is run, and a short one reads.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_tag_past_the_calculator_work_cap_is_refused_before_it_runs() {
        // A one-channel calculator of `steps` steps: `in`, `noop`s, `out`,
        // a step each.
        let through = |steps: usize| {
            let mut ops = vec![st(b"in  ", 0, 0)];
            ops.extend(std::iter::repeat_n(st(b"noop", 0, 0), steps - 2));
            ops.push(st(b"out ", 0, 0));
            element(1, 1, &ops, &[], None)
        };
        let past = IccError::CalculatorLimit(CalculatorLimit::Work);

        // 4 096 entries, the most a sampled calculator curve is sampled at.
        let steps = MAX_CALC_WORK / 4096 - 1;
        assert_eq!(steps, 16_383);
        let mut budget = Budget::TAG;
        let curve = mpe::read_curve(
            &clcf(4096, 0.0, 1.0, &through(steps)),
            &mut budget,
            &mut Calculator::read,
        )
        .expect("exactly the cap");
        assert_eq!(budget.steps, 0, "the whole cap spent");
        assert!((curve.eval(0.25) - 0.25).abs() < 1e-9);

        let mut budget = Budget::TAG;
        assert_eq!(
            mpe::read_curve(
                &clcf(4096, 0.0, 1.0, &through(steps + 1)),
                &mut budget,
                &mut Calculator::read,
            ),
            Err(past)
        );
        assert_eq!(budget, Budget::TAG, "nothing spent");

        // Sixteen runs of five steps and one are 96 of a budget of 100.
        let small = clcf(16, 0.0, 1.0, &through(5));
        let mut budget = Budget {
            steps: 100,
            ..Budget::TAG
        };
        assert!(mpe::read_curve(&small, &mut budget, &mut Calculator::read).is_ok());
        assert_eq!(budget.steps, 4);
        assert_eq!(
            mpe::read_curve(&small, &mut budget, &mut Calculator::read),
            Err(past)
        );
        assert_eq!(budget.steps, 4);

        // A stage: `in[0,2]` and `out[0,2]` three steps each.
        let stage = |steps: usize| {
            let mut ops = vec![st(b"in  ", 0, 2)];
            ops.extend(std::iter::repeat_n(st(b"noop", 0, 0), steps - 6));
            ops.push(st(b"out ", 0, 2));
            tag_of(&element(3, 3, &ops, &[], None))
        };
        assert_eq!(35_937 * 1_868, 67_130_316);
        assert!(67_130_316 > MAX_CALC_WORK);
        assert_eq!(mpe::read(&stage(1_867), false).map(|_| ()), Err(past));
        assert_eq!(mpe::read(&stage(8), false).map(|_| ()), Ok(()));
    }

    /// **A curve set's curves are each read once** (11.2.2.1): channels
    /// naming one place share its curve, whose sampled calculator curve
    /// runs and whose entries are spent once, not a channel each; curves at
    /// places apart are read each; and a place overlapping another, not
    /// naming it, is malformed — so a curve's bytes are read once however
    /// many channels name them.
    #[test]
    fn a_curve_sets_curves_are_each_read_once() {
        // Twice its input: `in`, `data`, `mul`, `out`, six steps.
        let double = element(
            1,
            1,
            &[
                st(b"in  ", 0, 0),
                push(2.0),
                st(b"mul ", 0, 0),
                st(b"out ", 0, 0),
            ],
            &[],
            None,
        );
        let curve = clcf(16, 0.0, 1.0, &double);
        assert_eq!(curve.len() % 4, 0);
        let start = 12 + 8 * 2;
        let ops = [st(b"in  ", 0, 1), st(b"curv", 0, 0), st(b"out ", 0, 1)];
        let spent = |set: Vec<u8>| {
            let mut budget = Budget::TAG;
            let calculator = Calculator::read(&element(2, 2, &ops, &[set], None), &mut budget)
                .expect("the program reads");
            close(&calculator.evaluate(&[0.5, 0.25]), &[1.0, 0.5], "doubled");
            (
                MAX_CALC_WORK - budget.steps,
                Budget::TAG.entries - budget.entries,
            )
        };

        let shared = curves_at(&[(start, curve.len()), (start, curve.len())], &curve);
        assert_eq!(spent(shared), (16 * 7, 16), "one curve, read once");
        let apart = curves_at(
            &[(start, curve.len()), (start + curve.len(), curve.len())],
            &[curve.clone(), curve.clone()].concat(),
        );
        assert_eq!(spent(apart), (2 * 16 * 7, 2 * 16), "two curves");

        let overlapping = curves_at(
            &[(start, curve.len()), (start + 4, curve.len() - 4)],
            &curve,
        );
        assert_eq!(
            read(&element(2, 2, &ops, &[overlapping], None)),
            Err(IccError::MalformedTag)
        );
    }

    /// **The steps a run takes count every value an operation moves**: a
    /// program of six hundred operations, vectors of 256 added and copied
    /// over and over, takes exactly the cap of steps — each operation a step
    /// a value it takes or gives — and reads and runs; one more add and copy
    /// is `CalculatorLimit::Operations` though it holds a hundredth of the
    /// operations the cap allows. A `clmt`'s operation count is of the
    /// operations a calculator holds (11.2.1), which a program within it
    /// reads at however many steps it takes.
    #[test]
    fn the_steps_a_run_takes_count_every_value_an_operation_moves() {
        // 512 ones (512 steps), then `pairs` of an add of 256 (512 taken
        // and 256 given) and a copy of 256 (256 taken and 512 given), 1 536
        // steps a pair, then 511 popped and one out: 1 024 + 1 536 a pair.
        let doubling = |pairs: usize| {
            let mut ops = vec![push(1.0); 512];
            for _ in 0..pairs {
                ops.extend_from_slice(&[st(b"add ", 255, 0), st(b"copy", 255, 0)]);
            }
            ops.extend_from_slice(&[st(b"pop ", 510, 0), st(b"out ", 0, 0)]);
            ops
        };
        assert_eq!(1_024 + 1_536 * 42, MAX_CALC_OPERATIONS);
        let ops = doubling(42);
        assert_eq!(ops.len(), 598);
        let calculator = read(&element(1, 1, &ops, &[], None)).expect("exactly the cap");
        assert_eq!(calculator.steps(), MAX_CALC_OPERATIONS);
        assert_eq!(calculator.evaluate(&[0.0]), vec![2f64.powi(42)]);
        assert_eq!(
            read(&element(1, 1, &doubling(43), &[], None)),
            Err(IccError::CalculatorLimit(CalculatorLimit::Operations))
        );

        // Four operations, six steps: `in` one, `data` one, `add` three,
        // `out` one.
        let four = [
            st(b"in  ", 0, 0),
            push(0.5),
            st(b"add ", 0, 0),
            st(b"out ", 0, 0),
        ];
        let calculator = read(&element(1, 1, &four, &[], Some([0, 0, 4]))).expect("a clmt of four");
        assert_eq!(calculator.steps(), 6);
        assert_eq!(calculator.evaluate(&[0.25]), vec![0.75]);
    }
}
