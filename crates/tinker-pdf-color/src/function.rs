//! PDF functions (7.10).
//!
//! Four types, all of which appear in real documents: sampled tables,
//! exponential interpolation, stitched pieces, and a small PostScript
//! calculator. Shadings and tint transforms are built from them.
//!
//! # The type 4 calculator is document-controlled input
//!
//! `Function::PostScript` interprets a program the document supplies, so its
//! bounds are a security property rather than tidiness. The language is the
//! arithmetic subset of 7.10.5 and nothing more: it has no file, name, string
//! or dictionary operators, no way to define a procedure, and no access to
//! anything outside its own operand stack. Evaluation is bounded on every
//! axis that could otherwise run away — the stack is capped at the
//! specification's 100 entries, `if`/`ifelse` nesting at 32, the token count
//! at 65536 — and division by zero yields zero rather than an infinity that
//! would propagate into a colour. It cannot loop: the language has no loop
//! operator and the interpreter never revisits an instruction.

use tinker_pdf_math as math;

/// A function from `m` inputs to `n` outputs.
#[derive(Clone, Debug, PartialEq)]
pub enum Function {
    /// Type 0: a sampled table, multilinearly interpolated (7.10.2).
    Sampled {
        /// Input range per dimension.
        domain: Vec<(f64, f64)>,
        /// Output range per component.
        range: Vec<(f64, f64)>,
        /// Samples per input dimension.
        size: Vec<usize>,
        /// Bits per sample: 1, 2, 4, 8, 12, 16, 24 or 32.
        bits: u32,
        /// How input maps to sample indices; defaults to `0..size-1`.
        encode: Vec<(f64, f64)>,
        /// How samples map to outputs; defaults to `range`.
        decode: Vec<(f64, f64)>,
        /// The sample data, packed big-endian.
        samples: Vec<u8>,
    },
    /// Type 2: `c0 + x^n · (c1 − c0)` (7.10.3).
    Exponential {
        /// Input range.
        domain: (f64, f64),
        /// Output at zero.
        c0: Vec<f64>,
        /// Output at one.
        c1: Vec<f64>,
        /// The exponent.
        n: f64,
    },
    /// Type 3: sub-functions joined at bounds (7.10.4).
    Stitching {
        /// Input range.
        domain: (f64, f64),
        /// The pieces.
        functions: Vec<Function>,
        /// The interior boundaries.
        bounds: Vec<f64>,
        /// Per-piece input mapping.
        encode: Vec<(f64, f64)>,
    },
    /// Type 4: a PostScript calculator (7.10.5).
    PostScript {
        /// Input range per dimension.
        domain: Vec<(f64, f64)>,
        /// Output range per component.
        range: Vec<(f64, f64)>,
        /// The parsed program.
        program: Vec<PsOp>,
    },
    /// 7.10.1: an *array* of functions, one per output component.
    ///
    /// A shading may give `/Function [f_r f_g f_b]` instead of one function
    /// with three outputs. Taking only the first — which is what happens if
    /// this variant does not exist — renders an RGB gradient as a red ramp on
    /// black, and does it silently, because a one-output function is
    /// perfectly valid on its own.
    Array(Vec<Function>),
    /// A function that could not be read; the identity on its inputs.
    Identity,
}

/// One instruction of a type 4 function.
#[derive(Clone, Debug, PartialEq)]
pub enum PsOp {
    /// A literal.
    Number(f64),
    /// An operator, by name.
    Op(&'static str),
    /// `{ … }` guarded by `if` or `ifelse`.
    Block(Vec<PsOp>),
}

impl Function {
    /// Evaluates the function.
    ///
    /// Never fails: a malformed function returns its inputs unchanged rather
    /// than stopping a page, which is what a viewer must do with one.
    #[must_use]
    pub fn eval(&self, inputs: &[f64]) -> Vec<f64> {
        let mut out = Vec::new();
        self.eval_into(inputs, &mut out);
        out
    }

    /// [`Function::eval`] into a buffer the caller keeps: `out` is cleared and
    /// then holds the outputs, the same values to the bit.
    ///
    /// For a caller that evaluates once a pixel. Nothing here allocates once
    /// `out` has grown as far as an evaluation takes it — a type 4 program's
    /// operand stack runs in `out` too — except a type 4 program that has more
    /// than eight `{ … }` procedures waiting for an `if` or `ifelse` at once in
    /// one body, whose ninth and later are kept on the heap.
    pub fn eval_into(&self, inputs: &[f64], out: &mut Vec<f64>) {
        out.clear();
        self.append(inputs, out);
    }

    /// Whether this is a function of one input whose values a table can stand
    /// in for: a type 0 table of one input, a type 2 exponential, a type 3
    /// stitch of such functions, or an array of them (7.10.2 to 7.10.4). A
    /// type 4 program, a table of more inputs and the identity a function that
    /// could not be read becomes are evaluated where they are asked.
    #[must_use]
    pub fn tabulable(&self) -> bool {
        match self {
            Function::Sampled { size, domain, .. } => size.len() == 1 && domain.len() <= 1,
            Function::Exponential { .. } => true,
            Function::Stitching { functions, .. } => functions.iter().all(Function::tabulable),
            Function::Array(functions) => {
                !functions.is_empty() && functions.iter().all(Function::tabulable)
            }
            Function::PostScript { .. } | Function::Identity => false,
        }
    }

    /// Appends the outputs to `out`, leaving what it already held alone.
    fn append(&self, inputs: &[f64], out: &mut Vec<f64>) {
        match self {
            Function::Identity => out.extend_from_slice(inputs),

            // Each member contributes its own outputs, in order. Members are
            // one-output functions by the spec's own requirement, but a
            // member that yields several is concatenated rather than
            // truncated: dropping values would be the same silent loss this
            // variant exists to prevent.
            Function::Array(functions) => {
                for function in functions {
                    function.append(inputs, out);
                }
            }

            Function::Exponential { domain, c0, c1, n } => {
                let x = clamp(inputs.first().copied().unwrap_or(0.0), *domain);
                let factor = if *n == 1.0 { x } else { math::pow(x, *n) };
                let factor = if factor.is_finite() { factor } else { 0.0 };
                let len = c0.len().max(c1.len());
                out.extend((0..len).map(|i| {
                    let a = c0.get(i).copied().unwrap_or(0.0);
                    let b = c1.get(i).copied().unwrap_or(1.0);
                    a + factor * (b - a)
                }));
            }

            Function::Stitching {
                domain,
                functions,
                bounds,
                encode,
            } => {
                let x = clamp(inputs.first().copied().unwrap_or(0.0), *domain);
                // Which piece: the first bound greater than x.
                let index = bounds.iter().position(|b| x < *b).unwrap_or(bounds.len());

                let low = if index == 0 {
                    domain.0
                } else {
                    bounds.get(index - 1).copied().unwrap_or(domain.0)
                };
                let high = bounds.get(index).copied().unwrap_or(domain.1);

                let (e0, e1) = encode.get(index).copied().unwrap_or((0.0, 1.0));
                let mapped = interpolate(x, low, high, e0, e1);

                match functions.get(index) {
                    Some(f) => f.append(&[mapped], out),
                    None => out.push(0.0),
                }
            }

            Function::Sampled {
                domain,
                range,
                size,
                bits,
                encode,
                decode,
                samples,
            } => sampled(
                domain, range, size, *bits, encode, decode, samples, inputs, out,
            ),

            Function::PostScript {
                domain,
                range,
                program,
            } => {
                // The operand stack is `out` above what it already held,
                // starting as the clamped inputs.
                let floor = out.len();
                out.extend(
                    inputs
                        .iter()
                        .enumerate()
                        .map(|(i, v)| clamp(*v, domain.get(i).copied().unwrap_or((0.0, 1.0)))),
                );
                run_ps(program, &mut Stack { values: out, floor }, 0);

                // The outputs are the last `range.len()` values on the stack,
                // moved down to where it began. Each is read before anything
                // is written at or past its slot, because the slot it moves to
                // is never after the one it came from.
                let n = range.len();
                let first = floor + out.len().saturating_sub(floor).saturating_sub(n);
                for i in 0..n {
                    let value = out.get(first + i).copied().unwrap_or(0.0);
                    let value = clamp(value, range.get(i).copied().unwrap_or((0.0, 1.0)));
                    match out.get_mut(floor + i) {
                        Some(slot) => *slot = value,
                        None => out.push(value),
                    }
                }
                out.truncate(floor + n);
            }
        }
    }
}

fn clamp(value: f64, (low, high): (f64, f64)) -> f64 {
    if !value.is_finite() {
        return low;
    }
    value.clamp(low.min(high), high.max(low))
}

fn interpolate(x: f64, x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    if (x1 - x0).abs() < f64::EPSILON {
        return y0;
    }
    let t = (x - x0) / (x1 - x0);
    let value = y0 + t * (y1 - y0);
    if value.is_finite() {
        value
    } else {
        y0
    }
}

/// How many inputs a type 0 table is interpolated across in full.
///
/// 7.10.2's multilinear interpolation reads the 2^m corners of the cell an
/// input falls in, so eight inputs are 256 reads and PDF's own ceiling of 32
/// colorants would be four thousand million. Past eight the nearest sample
/// is read instead — one read, and exact at every grid point.
const MULTILINEAR_INPUTS: usize = 8;

/// How many samples one multilinear evaluation may read: its corners times its
/// outputs.
///
/// Eight inputs of a four-output tint transform are 1 024 reads. A colour
/// space never asks a table for more outputs than PDF's 32 colorants, which at
/// eight inputs is this figure; a table that states more — a `/Range` of a
/// thousand pairs is a file's to write, and each pair is an output read at
/// every corner — is read at its nearest sample, as a table past
/// [`MULTILINEAR_INPUTS`] is. So an evaluation costs what the clause costs for
/// a table a colour space can use, and an image that evaluates one a pixel
/// pays no more for an absurd one.
const MULTILINEAR_READS: usize = 8192;

/// Type 0 (7.10.2): linear interpolation along one input, and multilinear
/// across several.
///
/// *Corrected 3 October 2026.* This used to read a table **along its first
/// input only**, whatever the others said — its own note called that a
/// nearest-sample read, and it was not one: a two-colorant `/DeviceN` tint
/// transform painted every tint of the second colorant as none of it. The
/// one-input path is the same arithmetic it always was.
///
/// The outputs are appended to `out`.
#[allow(clippy::too_many_arguments)]
fn sampled(
    domain: &[(f64, f64)],
    range: &[(f64, f64)],
    size: &[usize],
    bits: u32,
    encode: &[(f64, f64)],
    decode: &[(f64, f64)],
    samples: &[u8],
    inputs: &[f64],
    out: &mut Vec<f64>,
) {
    let outputs = range.len().max(1);
    let max = ((1u64 << bits.min(32)) - 1) as f64;

    let read = |index: usize, component: usize| -> f64 {
        // Checked, because a multi-input table's index is the file's `/Size`
        // multiplied out, and a sample past the data reads as zero either way.
        let Some(sample) = index
            .checked_mul(outputs)
            .and_then(|sample| sample.checked_add(component))
        else {
            return 0.0;
        };
        read_sample(samples, sample, bits)
    };

    if size.len() > 1 {
        sampled_across(domain, range, size, max, encode, decode, &read, inputs, out);
        return;
    }

    let first_domain = domain.first().copied().unwrap_or((0.0, 1.0));
    let first_size = size.first().copied().unwrap_or(2).max(1);
    let x = clamp(inputs.first().copied().unwrap_or(0.0), first_domain);

    let (e0, e1) = encode
        .first()
        .copied()
        .unwrap_or((0.0, (first_size - 1) as f64));
    let position =
        interpolate(x, first_domain.0, first_domain.1, e0, e1).clamp(0.0, (first_size - 1) as f64);

    let low = position.floor() as usize;
    let high = low.saturating_add(1).min(first_size - 1);
    let fraction = position - position.floor();

    out.extend((0..outputs).map(|component| {
        let a = read(low, component);
        let b = read(high, component);
        let raw = a + (b - a) * fraction;

        let (d0, d1) = decode
            .get(component)
            .copied()
            .or_else(|| range.get(component).copied())
            .unwrap_or((0.0, 1.0));
        let value = interpolate(raw, 0.0, max, d0, d1);
        clamp(value, range.get(component).copied().unwrap_or((0.0, 1.0)))
    }));
}

/// One sample of a 7.10.2 table: the `sample`th value of `bits` bits, packed
/// big-endian from the first byte, or zero where the data or the address runs
/// out.
///
/// The widths that start on a byte — 8, 16, 24 and 32, which is what every
/// producer writes and what `tinker_pdf_cos` writes — are read a byte at a
/// time; the rest a bit at a time. The two agree on every width they share,
/// which `the_byte_read_is_the_bit_read` holds them to. Only the first 32 bits
/// of a wider sample are read, as 7.10.2 allows no wider one.
fn read_sample(samples: &[u8], sample: usize, bits: u32) -> f64 {
    let width = bits.min(32);
    if bits == width && bits % 8 == 0 && bits > 0 {
        let bytes = (bits / 8) as usize;
        let Some(start) = sample.checked_mul(bytes) else {
            return 0.0;
        };
        let tail = samples.get(start..).unwrap_or(&[]);
        let value = (0..bytes).fold(0u64, |value, at| {
            (value << 8) | u64::from(tail.get(at).copied().unwrap_or(0))
        });
        return value as f64;
    }
    read_sample_bits(samples, sample, bits)
}

/// [`read_sample`] a bit at a time, for any width.
fn read_sample_bits(samples: &[u8], sample: usize, bits: u32) -> f64 {
    let Some(bit) = (sample as u64).checked_mul(u64::from(bits)) else {
        return 0.0;
    };
    let mut value = 0u64;
    for i in 0..bits.min(32) {
        let Some(at) = bit.checked_add(u64::from(i)) else {
            return 0.0;
        };
        let byte = usize::try_from(at / 8)
            .ok()
            .and_then(|index| samples.get(index))
            .copied()
            .unwrap_or(0);
        let shifted = (byte >> (7 - (at % 8))) & 1;
        value = (value << 1) | u64::from(shifted);
    }
    value as f64
}

/// [`sampled`] across more than one input: 7.10.2's table, the first input
/// varying fastest, interpolated multilinearly up to [`MULTILINEAR_INPUTS`]
/// inputs and read at the nearest sample past that.
///
/// # What one evaluation costs
///
/// A point is inside a cell of the table, and 7.10.2's interpolation is a
/// weighted sum of that cell's corners — 2^m of them across m inputs, which
/// is the cost of the clause and not of this code. What this code does not
/// add is work beyond it: no allocation per call, a byte-aligned sample read
/// in a few instructions ([`read_sample`]), and only the axes the point is
/// strictly *inside* enumerated. A point on a grid line along an axis weighs
/// nothing on that axis's upper corners, so a tint that is 0 or 1 in a
/// colorant — the common case for a spot ink an image leaves untouched —
/// halves the corners for each. *Corrected on review, 3 October 2026*: an
/// eight-input table of sixteen-bit samples took about 300 µs an evaluation
/// in a debug build, enumerating and weighing every corner and reading every
/// sample a bit at a time, and an image is evaluated once a pixel. The sums
/// are taken in the same order as before, so every answer is the same to the
/// bit.
#[allow(clippy::too_many_arguments)]
fn sampled_across(
    domain: &[(f64, f64)],
    range: &[(f64, f64)],
    size: &[usize],
    max: f64,
    encode: &[(f64, f64)],
    decode: &[(f64, f64)],
    read: &dyn Fn(usize, usize) -> f64,
    inputs: &[f64],
    out: &mut Vec<f64>,
) {
    let outputs = range.len().max(1);
    // The cell's low corner as one index into the table, the nearest sample
    // as another, and for each axis the point is strictly inside, the step to
    // the cell's upper corner on it and the fraction of the way there.
    // Saturating, because `/Size` is the file's and a product that overflows
    // is a table no file holds: `read` answers zero past the samples there
    // are, and a saturated sum is the same whichever order its terms are
    // added in.
    let mut base = 0usize;
    let mut closest = 0usize;
    let mut inside = [(0usize, 0.0f64); MULTILINEAR_INPUTS];
    let mut live = 0usize;
    let mut stride = 1usize;
    for (axis, &points) in size.iter().enumerate() {
        let points = points.max(1);
        let span = domain.get(axis).copied().unwrap_or((0.0, 1.0));
        let x = clamp(inputs.get(axis).copied().unwrap_or(0.0), span);
        let (e0, e1) = encode
            .get(axis)
            .copied()
            .unwrap_or((0.0, (points - 1) as f64));
        let position = interpolate(x, span.0, span.1, e0, e1).clamp(0.0, (points - 1) as f64);
        let low = position.floor() as usize;
        let high = low.saturating_add(1).min(points - 1);
        let fraction = position - position.floor();
        base = base.saturating_add(low.saturating_mul(stride));
        let at = if fraction < 0.5 { low } else { high };
        closest = closest.saturating_add(at.saturating_mul(stride));
        // `high` is `low + 1` wherever `fraction` is not zero: a position
        // clamped to the last sample has no fraction left.
        if fraction > 0.0 && high > low {
            if let Some(slot) = inside.get_mut(live) {
                *slot = (stride, fraction);
            }
            live += 1;
        }
        stride = stride.saturating_mul(points);
    }
    let multilinear = size.len() <= MULTILINEAR_INPUTS
        && live <= MULTILINEAR_INPUTS
        && (1usize << live).saturating_mul(outputs) <= MULTILINEAR_READS;

    // The raw sums are taken in `out` itself, past what it already held, and
    // decoded where they lie.
    let start = out.len();
    out.resize(start.saturating_add(outputs), 0.0);
    let Some(raw) = out.get_mut(start..) else {
        return;
    };
    if !multilinear {
        for (component, slot) in raw.iter_mut().enumerate() {
            *slot = read(closest, component);
        }
    } else {
        // `inside` has `MULTILINEAR_INPUTS` slots, filled in order, and
        // `multilinear` holds `live` to at most that many: the slice is in
        // range and the shift is at most eight.
        let inside = &inside[..live];
        for corner in 0..(1usize << live) {
            let mut weight = 1.0f64;
            let mut index = base;
            for (axis, &(step, fraction)) in inside.iter().enumerate() {
                if corner & (1 << axis) != 0 {
                    weight *= fraction;
                    index = index.saturating_add(step);
                } else {
                    weight *= 1.0 - fraction;
                }
            }
            // Every corner enumerated is read, so the reads are 2^live times
            // the outputs: a corner of no weight is one the axes above left
            // out, not one skipped here.
            for (component, slot) in raw.iter_mut().enumerate() {
                *slot += weight * read(index, component);
            }
        }
    }

    for (component, slot) in raw.iter_mut().enumerate() {
        let (d0, d1) = decode
            .get(component)
            .copied()
            .or_else(|| range.get(component).copied())
            .unwrap_or((0.0, 1.0));
        let value = interpolate(*slot, 0.0, max, d0, d1);
        *slot = clamp(value, range.get(component).copied().unwrap_or((0.0, 1.0)));
    }
}

/// A type 4 program's operand stack: the values of a buffer from `floor` up.
///
/// The buffer is the one [`Function::eval_into`] was handed, so the stack
/// costs no allocation of its own, and what an array's earlier members put
/// below `floor` is out of the program's reach: a pop at the floor answers
/// nothing, as a pop from an empty stack does.
struct Stack<'a> {
    values: &'a mut Vec<f64>,
    floor: usize,
}

impl Stack<'_> {
    fn len(&self) -> usize {
        self.values.len().saturating_sub(self.floor)
    }

    fn push(&mut self, value: f64) {
        self.values.push(value);
    }

    fn pop(&mut self) -> Option<f64> {
        if self.values.len() > self.floor {
            self.values.pop()
        } else {
            None
        }
    }

    fn truncate(&mut self, len: usize) {
        self.values.truncate(self.floor.saturating_add(len));
    }

    /// The value `index` places above the floor.
    fn get(&self, index: usize) -> Option<f64> {
        self.values.get(self.floor.checked_add(index)?).copied()
    }

    /// Where the top `n` values begin in the buffer, `n` held to the stack's
    /// length, so never below the floor.
    fn top(&self, n: usize) -> usize {
        self.values.len().saturating_sub(n.min(self.len()))
    }

    /// The top `n` values, or as many as there are.
    fn top_mut(&mut self, n: usize) -> &mut [f64] {
        let from = self.top(n);
        self.values.get_mut(from..).unwrap_or_default()
    }

    /// Pushes a copy of the top `n` values, or of as many as there are.
    fn copy_top(&mut self, n: usize) {
        // `top` is at most the buffer's length, so the range is in bounds.
        let from = self.top(n);
        self.values.extend_from_within(from..);
    }
}

/// How many `{ … }` procedures one body of a type 4 program holds in place
/// before [`Blocks`] keeps the rest on the heap.
const INLINE_BLOCKS: usize = 8;

/// The procedures a body of a type 4 program has pushed and no `if` or
/// `ifelse` has yet taken, the newest on top: the first [`INLINE_BLOCKS`] in
/// place and any beyond them on the heap, so that an evaluation of an
/// ordinary program allocates nothing.
struct Blocks<'p> {
    inline: [&'p [PsOp]; INLINE_BLOCKS],
    len: usize,
    spill: Vec<&'p [PsOp]>,
}

impl<'p> Blocks<'p> {
    fn new() -> Self {
        Blocks {
            inline: [&[]; INLINE_BLOCKS],
            len: 0,
            spill: Vec::new(),
        }
    }

    fn push(&mut self, body: &'p [PsOp]) {
        match self.inline.get_mut(self.len) {
            Some(slot) => {
                *slot = body;
                self.len += 1;
            }
            None => self.spill.push(body),
        }
    }

    /// The newest procedure: the spill is pushed to only while the places are
    /// full, so whatever it holds is newer than all of them.
    fn pop(&mut self) -> Option<&'p [PsOp]> {
        if let Some(body) = self.spill.pop() {
            return Some(body);
        }
        self.len = self.len.checked_sub(1)?;
        self.inline.get(self.len).copied()
    }
}

/// Runs a type 4 program.
fn run_ps(program: &[PsOp], stack: &mut Stack<'_>, depth: u32) {
    if depth > 32 {
        return;
    }

    let mut blocks = Blocks::new();

    for item in program {
        // The stack is bounded: 7.10.5 caps it at 100 and a hostile program
        // would otherwise grow it without limit.
        if stack.len() > 100 {
            stack.truncate(100);
        }

        match item {
            PsOp::Number(v) => stack.push(*v),
            PsOp::Block(body) => blocks.push(body),
            PsOp::Op(name) => {
                let mut pop = || stack.pop().unwrap_or(0.0);
                match *name {
                    "add" => {
                        let (b, a) = (pop(), pop());
                        stack.push(a + b);
                    }
                    "sub" => {
                        let (b, a) = (pop(), pop());
                        stack.push(a - b);
                    }
                    "mul" => {
                        let (b, a) = (pop(), pop());
                        stack.push(a * b);
                    }
                    "div" => {
                        let (b, a) = (pop(), pop());
                        stack.push(if b == 0.0 { 0.0 } else { a / b });
                    }
                    "idiv" => {
                        let (b, a) = (pop(), pop());
                        stack.push(if b as i64 == 0 {
                            0.0
                        } else {
                            ((a as i64) / (b as i64)) as f64
                        });
                    }
                    "mod" => {
                        let (b, a) = (pop(), pop());
                        stack.push(if b as i64 == 0 {
                            0.0
                        } else {
                            ((a as i64) % (b as i64)) as f64
                        });
                    }
                    "neg" => {
                        let a = pop();
                        stack.push(-a);
                    }
                    "abs" => {
                        let a = pop();
                        stack.push(a.abs());
                    }
                    "sqrt" => {
                        let a = pop();
                        stack.push(a.max(0.0).sqrt());
                    }
                    "sin" => {
                        let a = pop();
                        stack.push(math::sin(math::to_radians(a)));
                    }
                    "cos" => {
                        let a = pop();
                        stack.push(math::cos(math::to_radians(a)));
                    }
                    "atan" => {
                        let (b, a) = (pop(), pop());
                        let mut degrees = math::to_degrees(math::atan2(a, b));
                        if degrees < 0.0 {
                            degrees += 360.0;
                        }
                        stack.push(degrees);
                    }
                    "exp" => {
                        let (b, a) = (pop(), pop());
                        stack.push(math::pow(a, b));
                    }
                    "ln" => {
                        let a = pop();
                        stack.push(if a > 0.0 { math::ln(a) } else { 0.0 });
                    }
                    "log" => {
                        let a = pop();
                        stack.push(if a > 0.0 { math::log10(a) } else { 0.0 });
                    }
                    "cvi" | "truncate" => {
                        let a = pop();
                        stack.push(a.trunc());
                    }
                    "cvr" => {}
                    "floor" => {
                        let a = pop();
                        stack.push(a.floor());
                    }
                    "ceiling" => {
                        let a = pop();
                        stack.push(a.ceil());
                    }
                    "round" => {
                        let a = pop();
                        stack.push(a.round());
                    }
                    "dup" => {
                        let a = pop();
                        stack.push(a);
                        stack.push(a);
                    }
                    "pop" => {
                        pop();
                    }
                    "exch" => {
                        let (b, a) = (pop(), pop());
                        stack.push(b);
                        stack.push(a);
                    }
                    "copy" => {
                        let n = pop().clamp(0.0, 32.0) as usize;
                        stack.copy_top(n);
                    }
                    "index" => {
                        let n = pop().max(0.0) as usize;
                        let value = stack
                            .len()
                            .checked_sub(n.saturating_add(1))
                            .and_then(|i| stack.get(i))
                            .unwrap_or(0.0);
                        stack.push(value);
                    }
                    "roll" => {
                        let j = pop() as i64;
                        let n = pop().clamp(0.0, 64.0) as usize;
                        if n > 0 && n <= stack.len() {
                            let shift = j.rem_euclid(n as i64) as usize;
                            stack.top_mut(n).rotate_right(shift);
                        }
                    }
                    "eq" => bool_op(stack, |a, b| a == b),
                    "ne" => bool_op(stack, |a, b| a != b),
                    "gt" => bool_op(stack, |a, b| a > b),
                    "ge" => bool_op(stack, |a, b| a >= b),
                    "lt" => bool_op(stack, |a, b| a < b),
                    "le" => bool_op(stack, |a, b| a <= b),
                    "and" => int_op(stack, |a, b| a & b),
                    "or" => int_op(stack, |a, b| a | b),
                    "xor" => int_op(stack, |a, b| a ^ b),
                    "not" => {
                        let a = pop();
                        // Doubles as logical and bitwise negation, which is
                        // what PostScript does with its untyped stack.
                        stack.push(if a == 0.0 {
                            1.0
                        } else if a == 1.0 {
                            0.0
                        } else {
                            !(a as i64) as f64
                        });
                    }
                    "bitshift" => {
                        let (shift, value) = (pop() as i64, pop() as i64);
                        stack.push(if shift >= 0 {
                            (value << shift.min(63)) as f64
                        } else {
                            (value >> (-shift).min(63)) as f64
                        });
                    }
                    "true" => stack.push(1.0),
                    "false" => stack.push(0.0),
                    "if" => {
                        let condition = pop();
                        if let Some(body) = blocks.pop() {
                            if condition != 0.0 {
                                run_ps(body, stack, depth + 1);
                            }
                        }
                    }
                    "ifelse" => {
                        let condition = pop();
                        let (Some(alternative), Some(consequent)) = (blocks.pop(), blocks.pop())
                        else {
                            continue;
                        };
                        run_ps(
                            if condition != 0.0 {
                                consequent
                            } else {
                                alternative
                            },
                            stack,
                            depth + 1,
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn bool_op(stack: &mut Stack<'_>, f: impl Fn(f64, f64) -> bool) {
    let b = stack.pop().unwrap_or(0.0);
    let a = stack.pop().unwrap_or(0.0);
    stack.push(if f(a, b) { 1.0 } else { 0.0 });
}

fn int_op(stack: &mut Stack<'_>, f: impl Fn(i64, i64) -> i64) {
    let b = stack.pop().unwrap_or(0.0) as i64;
    let a = stack.pop().unwrap_or(0.0) as i64;
    stack.push(f(a, b) as f64);
}

/// Parses a type 4 program's source.
#[must_use]
pub fn parse_postscript(source: &[u8]) -> Vec<PsOp> {
    const NAMES: [&str; 42] = [
        "add", "sub", "mul", "div", "idiv", "mod", "neg", "abs", "sqrt", "sin", "cos", "atan",
        "exp", "ln", "log", "cvi", "cvr", "floor", "ceiling", "round", "truncate", "dup", "pop",
        "exch", "copy", "index", "roll", "eq", "ne", "gt", "ge", "lt", "le", "and", "or", "xor",
        "not", "bitshift", "true", "false", "if", "ifelse",
    ];

    fn parse(tokens: &mut std::slice::Iter<'_, Vec<u8>>, depth: u32) -> Vec<PsOp> {
        let mut out = Vec::new();
        if depth > 32 {
            return out;
        }
        while let Some(token) = tokens.next() {
            match token.as_slice() {
                b"{" => out.push(PsOp::Block(parse(tokens, depth + 1))),
                b"}" => return out,
                other => {
                    let text = String::from_utf8_lossy(other);
                    if let Ok(number) = text.parse::<f64>() {
                        out.push(PsOp::Number(number));
                    } else if let Some(name) = NAMES.iter().find(|n| **n == text) {
                        out.push(PsOp::Op(name));
                    }
                }
            }
        }
        out
    }

    // Split on whitespace, keeping braces as their own tokens.
    let mut tokens: Vec<Vec<u8>> = Vec::new();
    let mut current = Vec::new();
    for &b in source {
        match b {
            b'{' | b'}' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(vec![b]);
            }
            _ if b.is_ascii_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(b),
        }
        if tokens.len() > 1 << 16 {
            break;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut iter = tokens.iter();
    let parsed = parse(&mut iter, 0);
    // The whole program is normally wrapped in one pair of braces.
    match parsed.as_slice() {
        [PsOp::Block(body)] => body.clone(),
        _ => parsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exponential_interpolates_between_its_endpoints() {
        let f = Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0, 0.0],
            c1: vec![1.0, 0.5],
            n: 1.0,
        };
        assert_eq!(f.eval(&[0.0]), vec![0.0, 0.0]);
        assert_eq!(f.eval(&[1.0]), vec![1.0, 0.5]);
        assert_eq!(f.eval(&[0.5]), vec![0.5, 0.25]);
        // Outside the domain clamps.
        assert_eq!(f.eval(&[2.0]), vec![1.0, 0.5]);
    }

    #[test]
    fn a_stitching_function_selects_its_piece() {
        let f = Function::Stitching {
            domain: (0.0, 1.0),
            functions: vec![
                Function::Exponential {
                    domain: (0.0, 1.0),
                    c0: vec![0.0],
                    c1: vec![1.0],
                    n: 1.0,
                },
                Function::Exponential {
                    domain: (0.0, 1.0),
                    c0: vec![10.0],
                    c1: vec![20.0],
                    n: 1.0,
                },
            ],
            bounds: vec![0.5],
            encode: vec![(0.0, 1.0), (0.0, 1.0)],
        };
        assert_eq!(f.eval(&[0.0]), vec![0.0]);
        assert_eq!(f.eval(&[0.25]), vec![0.5], "halfway through the first half");
        assert_eq!(f.eval(&[0.5]), vec![10.0], "the second piece begins");
        assert_eq!(f.eval(&[1.0]), vec![20.0]);
    }

    #[test]
    fn a_sampled_function_interpolates_its_table() {
        // Two samples, 8 bits: 0 and 255 over 0..1.
        let f = Function::Sampled {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            size: vec![2],
            bits: 8,
            encode: vec![(0.0, 1.0)],
            decode: vec![(0.0, 1.0)],
            samples: vec![0, 255],
        };
        assert_eq!(f.eval(&[0.0]), vec![0.0]);
        assert_eq!(f.eval(&[1.0]), vec![1.0]);
        let mid = f.eval(&[0.5]);
        assert!(
            (mid.first().copied().unwrap_or(0.0) - 0.5).abs() < 0.01,
            "got {mid:?}"
        );
    }

    /// Two inputs, 2 x 2 samples: 7.10.2's table varies the **first input
    /// fastest**, and a point between the four is their bilinear blend. The
    /// samples 0, 85, 170, 255 at (0,0), (1,0), (0,1), (1,1).
    #[test]
    fn a_two_input_sampled_function_is_read_across_both_inputs() {
        let f = Function::Sampled {
            domain: vec![(0.0, 1.0); 2],
            range: vec![(0.0, 1.0)],
            size: vec![2, 2],
            bits: 8,
            encode: Vec::new(),
            decode: Vec::new(),
            samples: vec![0, 85, 170, 255],
        };
        let at = |x: f64, y: f64| f.eval(&[x, y])[0];
        assert!(
            (at(1.0, 0.0) - 85.0 / 255.0).abs() < 1e-9,
            "the first input moves one sample"
        );
        assert!(
            (at(0.0, 1.0) - 170.0 / 255.0).abs() < 1e-9,
            "the second moves two"
        );
        assert!((at(1.0, 1.0) - 1.0).abs() < 1e-9);
        assert!((at(0.5, 0.5) - 0.5).abs() < 1e-9, "the four corners' mean");
        assert!(
            (at(0.0, 0.5) - 85.0 / 255.0).abs() < 1e-9,
            "halfway up the second"
        );
    }

    /// Past [`MULTILINEAR_INPUTS`] the nearest sample is read, and a table
    /// whose `/Size` multiplies past the address space reads zero rather than
    /// panicking.
    #[test]
    fn a_wide_or_absurd_sampled_function_is_still_read() {
        let inputs = MULTILINEAR_INPUTS + 1;
        let mut samples = vec![0u8; 1 << inputs];
        samples[(1 << inputs) - 1] = 255;
        let f = Function::Sampled {
            domain: vec![(0.0, 1.0); inputs],
            range: vec![(0.0, 1.0)],
            size: vec![2; inputs],
            bits: 8,
            encode: Vec::new(),
            decode: Vec::new(),
            samples,
        };
        assert_eq!(
            f.eval(&vec![0.9; inputs]),
            vec![1.0],
            "the nearest is the last corner"
        );
        assert_eq!(f.eval(&vec![0.1; inputs]), vec![0.0]);

        let absurd = Function::Sampled {
            domain: vec![(0.0, 1.0); 4],
            range: vec![(0.0, 1.0)],
            size: vec![usize::MAX; 4],
            bits: 32,
            encode: Vec::new(),
            decode: Vec::new(),
            samples: vec![255; 8],
        };
        assert_eq!(absurd.eval(&[1.0; 4]).len(), 1);
    }

    /// Every byte-aligned width reads the value the bit-at-a-time walk reads,
    /// at every sample of a table and past its end, where both read zero.
    #[test]
    fn the_byte_read_is_the_bit_read() {
        let samples: Vec<u8> = (0..61u32).map(|i| (i * 97 % 256) as u8).collect();
        for bits in [8, 16, 24, 32] {
            for sample in 0..(samples.len() * 8 / bits as usize + 3) {
                assert_eq!(
                    read_sample(&samples, sample, bits),
                    read_sample_bits(&samples, sample, bits),
                    "{bits} bits, sample {sample}"
                );
            }
            assert_eq!(read_sample(&samples, usize::MAX, bits), 0.0);
        }
        // A width past 32 is read as its first 32 bits on its own stride.
        assert_eq!(
            read_sample(&samples, 1, 40),
            read_sample_bits(&samples, 1, 40)
        );
    }

    /// The reads one evaluation of `f` makes at `inputs`, counted.
    fn reads(size: &[usize], outputs: usize, inputs: &[f64]) -> (usize, Vec<f64>) {
        let count = std::cell::Cell::new(0usize);
        let read = |index: usize, component: usize| {
            count.set(count.get() + 1);
            ((index * 7 + component) % 256) as f64
        };
        let domain = vec![(0.0, 1.0); size.len()];
        let range = vec![(0.0, 1.0); outputs];
        let mut values = Vec::new();
        sampled_across(
            &domain,
            &range,
            size,
            255.0,
            &[],
            &[],
            &read,
            inputs,
            &mut values,
        );
        (count.get(), values)
    }

    /// One evaluation reads no more than [`MULTILINEAR_READS`] samples,
    /// whatever the table's `/Range` asks for: eight inputs strictly inside
    /// their cells are 256 corners, and a sixty-four-output table there is
    /// read at its nearest sample — 64 reads, not 16 384.
    #[test]
    fn one_evaluation_reads_no_more_than_its_budget() {
        let size = [3; MULTILINEAR_INPUTS];
        let inside = [0.3; MULTILINEAR_INPUTS];
        let (four, _) = reads(&size, 4, &inside);
        assert_eq!(four, 256 * 4, "a tint transform's own cost");
        let (wide, _) = reads(&size, 64, &inside);
        assert!(wide <= MULTILINEAR_READS, "{wide} reads");
        assert_eq!(wide, 64, "the nearest sample");
    }

    /// An axis the point sits on a grid line of weighs nothing on its upper
    /// corners, so it is not enumerated: a tint of 0 or 1 in seven of eight
    /// colorants is two corners, and the answer is the one-input table's.
    #[test]
    fn an_axis_on_a_grid_line_adds_no_corners() {
        let size = [3; MULTILINEAR_INPUTS];
        let mut at = [0.0; MULTILINEAR_INPUTS];
        at[3] = 0.25;
        at[5] = 1.0;
        let (count, values) = reads(&size, 4, &at);
        assert_eq!(count, 2 * 4);
        // The cell's low corner is 2 x 3^5 along the sixth axis; the fourth
        // axis moves 0.5 of the way to the next sample, 3^3 further on.
        let low = 2 * 243;
        let high = low + 27;
        for (component, value) in values.iter().enumerate() {
            let sample = |index: usize| ((index * 7 + component) % 256) as f64;
            let want = (sample(low) * 0.5 + sample(high) * 0.5) / 255.0;
            assert!(
                (value - want).abs() < 1e-12,
                "{component}: {value} vs {want}"
            );
        }
    }

    #[test]
    fn a_postscript_function_computes() {
        let program = parse_postscript(b"{ 2 mul }");
        let f = Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 2.0)],
            program,
        };
        assert_eq!(f.eval(&[0.25]), vec![0.5]);
        assert_eq!(f.eval(&[1.0]), vec![2.0]);
    }

    #[test]
    fn postscript_conditionals_work() {
        // Return 1 when the input exceeds a half, else 0.
        let program = parse_postscript(b"{ 0.5 gt { 1 } { 0 } ifelse }");
        let f = Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            program,
        };
        assert_eq!(f.eval(&[0.9]), vec![1.0]);
        assert_eq!(f.eval(&[0.1]), vec![0.0]);
    }

    #[test]
    fn postscript_stack_operators_behave() {
        let f = |src: &[u8], input: f64| {
            Function::PostScript {
                domain: vec![(0.0, 100.0)],
                range: vec![(-100.0, 100.0)],
                program: parse_postscript(src),
            }
            .eval(&[input])
        };
        assert_eq!(f(b"{ dup add }", 3.0), vec![6.0]);
        assert_eq!(f(b"{ 10 exch sub }", 4.0), vec![6.0]);
        assert_eq!(f(b"{ 2 3 pop mul }", 5.0), vec![10.0]);
        assert_eq!(f(b"{ neg }", 7.0), vec![-7.0]);
        // Division by zero yields zero rather than an infinity.
        assert_eq!(f(b"{ 0 div }", 5.0), vec![0.0]);
    }

    /// `copy`, `index` and `roll` as 7.10.5 takes them from PostScript: `copy`
    /// duplicates the top `n`, `index` reads the `n`th below the top, and
    /// `j roll` turns the top `n` by `j`, a positive `j` towards the top.
    /// Where the stack is too shallow for the operand, `copy` copies what is
    /// there and `index` reads zero.
    #[test]
    fn postscript_copy_index_and_roll_move_the_stack_as_postscript_does() {
        let f = |src: &[u8], outputs: usize| {
            Function::PostScript {
                domain: vec![(0.0, 100.0)],
                range: vec![(-100.0, 100.0); outputs],
                program: parse_postscript(src),
            }
            .eval(&[5.0])
        };
        assert_eq!(f(b"{ 1 2 3 copy }", 6), vec![5.0, 1.0, 2.0, 5.0, 1.0, 2.0]);
        assert_eq!(f(b"{ 9 copy }", 2), vec![5.0, 5.0], "as many as there are");
        assert_eq!(f(b"{ 1 2 2 index }", 4), vec![5.0, 1.0, 2.0, 5.0]);
        assert_eq!(
            f(b"{ 1 9 index }", 3),
            vec![5.0, 1.0, 0.0],
            "below the bottom"
        );
        assert_eq!(f(b"{ 1e300 index }", 2), vec![5.0, 0.0], "far below it");
        assert_eq!(f(b"{ 1 2 3 3 1 roll }", 4), vec![5.0, 3.0, 1.0, 2.0]);
        assert_eq!(f(b"{ 1 2 3 3 -1 roll }", 4), vec![5.0, 2.0, 3.0, 1.0]);
    }

    /// [`Function::eval_into`] leaves nothing of what its buffer held, and an
    /// array's type 4 member runs on a stack of its own even though that
    /// stack shares the buffer with the members before it: the program below
    /// pops past its own input, and a pop there answers zero rather than the
    /// first member's output.
    #[test]
    fn eval_into_starts_afresh_and_a_type_4_member_keeps_to_its_own_stack() {
        let mut out = vec![9.0; 7];
        let ramp = Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0],
            c1: vec![1.0],
            n: 1.0,
        };
        ramp.eval_into(&[0.25], &mut out);
        assert_eq!(out, vec![0.25]);

        let array = Function::Array(vec![
            ramp.clone(),
            Function::PostScript {
                domain: vec![(0.0, 1.0)],
                range: vec![(0.0, 1.0)],
                program: parse_postscript(b"{ pop 0.5 add }"),
            },
            ramp,
        ]);
        array.eval_into(&[0.25], &mut out);
        assert_eq!(out, vec![0.25, 0.5, 0.25]);
        assert_eq!(array.eval(&[0.25]), out);

        // The 100-entry ceiling is the member's own, above the floor.
        let deep = Function::Array(vec![
            Function::Identity,
            Function::PostScript {
                domain: vec![(0.0, 1.0)],
                range: vec![(0.0, 1.0); 2],
                program: parse_postscript(" 1".repeat(150).as_bytes()),
            },
        ]);
        deep.eval_into(&[0.75], &mut out);
        assert_eq!(out, vec![0.75, 1.0, 1.0]);
    }

    /// Procedures waiting for `if` and `ifelse` are taken newest first, the
    /// ninth and later from the heap and the first eight from their places,
    /// in whatever order the two are pushed and taken.
    #[test]
    fn waiting_procedures_are_taken_newest_first_in_place_or_not() {
        let bodies: Vec<Vec<PsOp>> = (0..12).map(|i| vec![PsOp::Number(f64::from(i))]).collect();
        let mut blocks = Blocks::new();
        let mut model: Vec<&[PsOp]> = Vec::new();
        let mut taken = Vec::new();
        let mut expected = Vec::new();
        // Push ten, take three, push two, take everything.
        for body in &bodies[..10] {
            blocks.push(body);
            model.push(body);
        }
        for _ in 0..3 {
            taken.push(blocks.pop());
            expected.push(model.pop());
        }
        for body in &bodies[10..] {
            blocks.push(body);
            model.push(body);
        }
        while let Some(body) = model.pop() {
            taken.push(blocks.pop());
            expected.push(Some(body));
        }
        taken.push(blocks.pop());
        expected.push(None);
        assert_eq!(taken, expected);

        // And through a program: ten procedures, each `if` running the newest.
        let mut source = String::from("{");
        for i in 1..=10 {
            source.push_str(&format!(" {{ {i} }}"));
        }
        source.push_str(&" true if".repeat(10));
        source.push('}');
        let f = Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 10.0); 10],
            program: parse_postscript(source.as_bytes()),
        };
        let wanted: Vec<f64> = (1..=10).rev().map(f64::from).collect();
        assert_eq!(f.eval(&[0.0]), wanted);
    }

    #[test]
    fn a_malformed_function_returns_something_usable() {
        assert_eq!(Function::Identity.eval(&[0.3, 0.7]), vec![0.3, 0.7]);

        // A program that underflows its stack must not panic.
        let f = Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            program: parse_postscript(b"{ add add add mul mul }"),
        };
        assert_eq!(f.eval(&[0.5]).len(), 1);

        // Nonsense source parses to something that evaluates.
        for src in [b"".as_slice(), b"{{{{{{", b"}}}}", b"garbage tokens"] {
            let _ = Function::PostScript {
                domain: vec![(0.0, 1.0)],
                range: vec![(0.0, 1.0)],
                program: parse_postscript(src),
            }
            .eval(&[0.5]);
        }
    }

    /// The functions a table stands in for: one input, of types 0, 2 or 3,
    /// or an array of them, all the way down a stitch.
    #[test]
    fn a_table_stands_in_for_one_input_types_0_2_and_3() {
        let exponential = Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0],
            c1: vec![1.0],
            n: 2.0,
        };
        let one_input = Function::Sampled {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            size: vec![4],
            bits: 8,
            encode: vec![],
            decode: vec![],
            samples: vec![0, 80, 160, 255],
        };
        let two_inputs = Function::Sampled {
            domain: vec![(0.0, 1.0); 2],
            range: vec![(0.0, 1.0)],
            size: vec![2, 2],
            bits: 8,
            encode: vec![],
            decode: vec![],
            samples: vec![0, 80, 160, 255],
        };
        let calculator = Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0)],
            program: parse_postscript(b"{ 2 mul }"),
        };
        let stitch = |pieces: Vec<Function>| Function::Stitching {
            domain: (0.0, 1.0),
            bounds: vec![0.5; pieces.len().saturating_sub(1)],
            encode: vec![(0.0, 1.0); pieces.len()],
            functions: pieces,
        };
        assert!(exponential.tabulable());
        assert!(one_input.tabulable());
        assert!(stitch(vec![exponential.clone(), one_input.clone()]).tabulable());
        assert!(Function::Array(vec![exponential.clone(), one_input.clone()]).tabulable());
        assert!(!two_inputs.tabulable());
        assert!(!calculator.tabulable());
        assert!(!Function::Identity.tabulable());
        assert!(!stitch(vec![exponential.clone(), calculator.clone()]).tabulable());
        assert!(!Function::Array(vec![exponential, calculator]).tabulable());
        assert!(!Function::Array(Vec::new()).tabulable());
    }
}
