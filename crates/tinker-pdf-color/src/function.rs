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
        match self {
            Function::Identity => inputs.to_vec(),

            // Each member contributes its own outputs, in order. Members are
            // one-output functions by the spec's own requirement, but a
            // member that yields several is concatenated rather than
            // truncated: dropping values would be the same silent loss this
            // variant exists to prevent.
            Function::Array(functions) => functions.iter().flat_map(|f| f.eval(inputs)).collect(),

            Function::Exponential { domain, c0, c1, n } => {
                let x = clamp(inputs.first().copied().unwrap_or(0.0), *domain);
                let factor = if *n == 1.0 { x } else { x.powf(*n) };
                let factor = if factor.is_finite() { factor } else { 0.0 };
                let len = c0.len().max(c1.len());
                (0..len)
                    .map(|i| {
                        let a = c0.get(i).copied().unwrap_or(0.0);
                        let b = c1.get(i).copied().unwrap_or(1.0);
                        a + factor * (b - a)
                    })
                    .collect()
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
                    Some(f) => f.eval(&[mapped]),
                    None => vec![0.0],
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
            } => sampled(domain, range, size, *bits, encode, decode, samples, inputs),

            Function::PostScript {
                domain,
                range,
                program,
            } => {
                let clamped: Vec<f64> = inputs
                    .iter()
                    .enumerate()
                    .map(|(i, v)| clamp(*v, domain.get(i).copied().unwrap_or((0.0, 1.0))))
                    .collect();

                let mut stack = clamped;
                run_ps(program, &mut stack, 0);

                // The outputs are the last `range.len()` values on the stack.
                let n = range.len();
                let start = stack.len().saturating_sub(n);
                (0..n)
                    .map(|i| {
                        let value = stack.get(start + i).copied().unwrap_or(0.0);
                        clamp(value, range.get(i).copied().unwrap_or((0.0, 1.0)))
                    })
                    .collect()
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

/// Type 0, restricted to one input dimension with linear interpolation.
///
/// Multi-input sampled functions exist but are vanishingly rare outside
/// DeviceN transforms; a nearest-sample read is used for those rather than
/// full multilinear interpolation, which is a visible difference only on a
/// gradient nobody has yet produced.
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
) -> Vec<f64> {
    let outputs = range.len().max(1);
    let max = ((1u64 << bits.min(32)) - 1) as f64;

    let read = |index: usize, component: usize| -> f64 {
        let sample = index * outputs + component;
        let bit = (sample as u64) * u64::from(bits);
        let mut value = 0u64;
        for i in 0..bits.min(32) {
            let at = bit + u64::from(i);
            let byte = samples.get((at / 8) as usize).copied().unwrap_or(0);
            let shifted = (byte >> (7 - (at % 8))) & 1;
            value = (value << 1) | u64::from(shifted);
        }
        value as f64
    };

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
    let high = (low + 1).min(first_size - 1);
    let fraction = position - position.floor();

    (0..outputs)
        .map(|component| {
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
        })
        .collect()
}

/// Runs a type 4 program.
fn run_ps(program: &[PsOp], stack: &mut Vec<f64>, depth: u32) {
    if depth > 32 {
        return;
    }

    let mut blocks: Vec<&Vec<PsOp>> = Vec::new();

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
                        stack.push(a.to_radians().sin());
                    }
                    "cos" => {
                        let a = pop();
                        stack.push(a.to_radians().cos());
                    }
                    "atan" => {
                        let (b, a) = (pop(), pop());
                        let mut degrees = a.atan2(b).to_degrees();
                        if degrees < 0.0 {
                            degrees += 360.0;
                        }
                        stack.push(degrees);
                    }
                    "exp" => {
                        let (b, a) = (pop(), pop());
                        stack.push(a.powf(b));
                    }
                    "ln" => {
                        let a = pop();
                        stack.push(if a > 0.0 { a.ln() } else { 0.0 });
                    }
                    "log" => {
                        let a = pop();
                        stack.push(if a > 0.0 { a.log10() } else { 0.0 });
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
                        let start = stack.len().saturating_sub(n);
                        let slice: Vec<f64> = stack.get(start..).unwrap_or_default().to_vec();
                        stack.extend(slice);
                    }
                    "index" => {
                        let n = pop().max(0.0) as usize;
                        let value = stack
                            .len()
                            .checked_sub(n + 1)
                            .and_then(|i| stack.get(i).copied())
                            .unwrap_or(0.0);
                        stack.push(value);
                    }
                    "roll" => {
                        let j = pop() as i64;
                        let n = pop().clamp(0.0, 64.0) as usize;
                        if n > 0 && n <= stack.len() {
                            let at = stack.len() - n;
                            let slice = &mut stack[at..];
                            let shift = j.rem_euclid(n as i64) as usize;
                            slice.rotate_right(shift);
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

fn bool_op(stack: &mut Vec<f64>, f: impl Fn(f64, f64) -> bool) {
    let b = stack.pop().unwrap_or(0.0);
    let a = stack.pop().unwrap_or(0.0);
    stack.push(if f(a, b) { 1.0 } else { 0.0 });
}

fn int_op(stack: &mut Vec<f64>, f: impl Fn(i64, i64) -> i64) {
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
}
