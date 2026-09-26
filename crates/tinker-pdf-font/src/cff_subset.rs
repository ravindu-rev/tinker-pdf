//! CFF subsetting: the same font program with the charstrings nobody drew
//! replaced by nothing, and the subroutines nobody reached removed.
//!
//! `glyf` can be cut with scissors — a dropped glyph becomes a zero-length
//! `loca` entry and every byte around it is untouched. A CFF cannot. Its
//! charstrings live in an INDEX whose offsets are a contiguous array, so
//! removing one rewrites the table; the subroutines a charstring calls are
//! *biased* indices into two more INDEXes, so removing a subroutine changes
//! the number every surviving caller must use; and the Top DICT's pointers to
//! all of it are absolute file offsets whose own encoded width changes where
//! everything lands. Rebuilding one structure means rebuilding all four.
//!
//! **Glyph identifiers do not change**, for [`crate::subset`]'s reason and
//! with more riding on it: the charset — which is what maps a name or a CID
//! onto a glyph — is copied through byte for byte, so a renumbering subset
//! would have to rewrite it, the encoding, `FDSelect`, and outside this crate
//! `/Widths`, `/W`, `/CIDToGIDMap` and `/ToUnicode`. A dropped glyph keeps its
//! slot in the CharStrings INDEX and holds a single `endchar`, which is a
//! well-formed charstring that draws nothing and costs one byte.
//!
//! # What is rebuilt, and what is copied
//!
//! Rebuilt, because the subset changes it: the CharStrings INDEX, the global
//! subroutine INDEX, every local subroutine INDEX, the Private DICTs that
//! point at those (their `Subrs` offset moves), the Font DICTs that point at
//! the Private DICTs, and the Top DICT that points at everything.
//!
//! Copied verbatim, because glyph identifiers do not move and so it is still
//! exactly right: the Name INDEX, the String INDEX (SIDs are unchanged, so a
//! charset entry still names what it named), the charset, the encoding and
//! `FDSelect`. Copying beats rebuilding here — a charset this code re-encoded
//! is a chance to map a glyph to the wrong name, and there is nothing to gain
//! by taking it.
//!
//! # Subroutine renumbering
//!
//! A `callsubr` operand is the subroutine number *less a bias*, and the bias
//! is a step function of how many subroutines the INDEX holds: 107 below
//! 1 240, 1 131 below 33 900, 32 768 above (Type 2 charstring specification,
//! §4.7). Subsetting shrinks the INDEX, so the bias can change even for a
//! subroutine that kept its position — which is why the operand is recomputed
//! from the *new* index and the *new* bias rather than adjusted.
//!
//! A global subroutine is shared by every glyph, but the local subroutines it
//! can call are the caller's — in a CID-keyed font, its Font DICT's. So one
//! global subroutine reached from two Font DICTs needs two different sets of
//! renumbered `callsubr` operands, and gets **two entries** in the new global
//! INDEX. Duplicating costs the bytes of the subroutine; sharing would cost
//! the wrong glyph.
//!
//! # When this declines
//!
//! Returning `None` embeds the whole face, which is larger and correct
//! (ruling 2). It happens when the program says something this cannot rewrite
//! without guessing: a `callsubr` whose operand is not the token immediately
//! before it (so the number was computed, and no static rewrite can know it),
//! a call to a subroutine that is not there, a subroutine reached with two
//! different hint states so that its `hintmask` lengths disagree, a
//! subroutine that calls itself, or `CharstringType 1`. Every one of those is
//! a font this code would have to invent an answer for.

use std::collections::{BTreeMap, BTreeSet};

use crate::cff::{bias, dict_get, parse_dict, read_fd_select, Cff, Index};

/// How deep a `callsubr` chain is followed, which is the reader's own bound
/// (`cff.rs`): a call at depth 11 does not run, so a subroutine only reachable
/// there is not kept.
const MAX_DEPTH: u32 = 10;

/// How many charstring tokens the whole walk may read. A font is
/// attacker-controlled (ruling 1) and a subroutine graph can be dense; this is
/// what makes the walk finite whatever the graph looks like.
const WALK_BUDGET: u32 = 40_000_000;

/// How many distinct `(subroutine, hint state)` pairs are remembered. The
/// memo is what keeps the walk from re-reading a subroutine once per call;
/// the cap is what keeps a font from making the memo the denial of service.
const MAX_STATES: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Writing the primitives.
// ---------------------------------------------------------------------------

/// Wraps `items` as a CFF INDEX, with the narrowest offsets that reach the
/// end of the data.
fn write_index(items: &[Vec<u8>]) -> Option<Vec<u8>> {
    // A CFF INDEX states its count in two bytes, and an empty one is those
    // two bytes and nothing else.
    let count = u16::try_from(items.len()).ok()?;
    if count == 0 {
        return Some(vec![0, 0]);
    }

    let total: usize = items
        .iter()
        .try_fold(0usize, |a, i| a.checked_add(i.len()))?;
    // Offsets are one-based, so the last one is the total plus one.
    let last = u32::try_from(total.checked_add(1)?).ok()?;
    let off_size: usize = match last {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };

    let mut out = Vec::with_capacity(3 + (items.len() + 1) * off_size + total);
    out.extend_from_slice(&count.to_be_bytes());
    out.push(off_size as u8);
    let mut offset = 1u32;
    out.extend_from_slice(&offset.to_be_bytes()[4 - off_size..]);
    for item in items {
        offset = offset.checked_add(u32::try_from(item.len()).ok()?)?;
        out.extend_from_slice(&offset.to_be_bytes()[4 - off_size..]);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    Some(out)
}

/// A DICT integer in the fixed five-byte form (CFF specification, Table 3).
///
/// Fixed width on purpose, and it is the whole reason the offset back-patching
/// terminates: a Top DICT holding `charstrings` at 40 000 encodes that operand
/// in three bytes, and moving the CharStrings INDEX past 65 535 would make it
/// five and push the INDEX two bytes further along, which moves it again.
/// Five bytes always means the DICT written with zeroes is exactly as long as
/// the DICT written with the answer, so one measuring pass settles the layout.
fn dict_int(value: i32) -> [u8; 5] {
    let b = value.to_be_bytes();
    [29, b[0], b[1], b[2], b[3]]
}

/// A DICT real (Table 5): nibbles, terminated by `f`.
fn dict_real(value: f64) -> Vec<u8> {
    let mut nibbles: Vec<u8> = Vec::new();
    // `{:?}` is the shortest decimal that reads back as the same double, which
    // is what a copied `FontMatrix` needs and what `{}` does not promise.
    let text = format!("{value:?}");
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '0'..='9' => nibbles.push(c as u8 - b'0'),
            '.' => nibbles.push(0x0A),
            '-' => nibbles.push(0x0E),
            'e' | 'E' => {
                if chars.peek() == Some(&'-') {
                    chars.next();
                    nibbles.push(0x0C);
                } else {
                    if chars.peek() == Some(&'+') {
                        chars.next();
                    }
                    nibbles.push(0x0B);
                }
            }
            _ => {}
        }
        if nibbles.len() > 60 {
            break;
        }
    }
    nibbles.push(0x0F);
    if nibbles.len() % 2 == 1 {
        nibbles.push(0x0F);
    }

    let mut out = vec![30u8];
    for pair in nibbles.chunks(2) {
        let (hi, lo) = (
            pair.first().copied().unwrap_or(0x0F),
            pair.get(1).copied().unwrap_or(0x0F),
        );
        out.push((hi << 4) | lo);
    }
    out
}

/// One DICT operand, integral where it can be and real where it cannot.
fn dict_operand(value: f64) -> Vec<u8> {
    if !value.is_finite() {
        // The reader turns an unparsable real into zero; writing one back is
        // the only value that does not invent something.
        return dict_int(0).to_vec();
    }
    if value.fract() == 0.0 && (-2_147_483_648.0..=2_147_483_647.0).contains(&value) {
        return dict_int(value as i32).to_vec();
    }
    dict_real(value)
}

/// A DICT operator, one byte or two.
fn dict_op(op: u16) -> Vec<u8> {
    if op > 0xFF {
        vec![12, (op & 0xFF) as u8]
    } else {
        vec![op as u8]
    }
}

/// A Type 2 charstring integer, in the narrowest form that holds it.
///
/// The forms are the reader's, read backwards (Type 2 specification, §4).
/// Nothing wider than the 16-bit form is emitted: a subroutine number is
/// bounded by the 65 535 an INDEX can hold, less a bias of at most 32 768, so
/// it always fits — and the 16.16 fixed form would make a *fraction* of a
/// subroutine number, which is not a thing.
fn charstring_int(value: i32) -> Option<Vec<u8>> {
    match value {
        -107..=107 => Some(vec![(value + 139) as u8]),
        108..=1131 => {
            let v = value - 108;
            Some(vec![(247 + (v >> 8)) as u8, (v & 0xFF) as u8])
        }
        -1131..=-108 => {
            let v = -value - 108;
            Some(vec![(251 + (v >> 8)) as u8, (v & 0xFF) as u8])
        }
        -32768..=32767 => {
            let b = (value as i16).to_be_bytes();
            Some(vec![28, b[0], b[1]])
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Measuring the tables that are copied through.
// ---------------------------------------------------------------------------

/// How many bytes the charset at `at` occupies, in each of the three formats.
///
/// The charset is copied verbatim, so its *length* is the only thing this
/// needs — but the length is only knowable by walking the runs, because
/// format 1 and 2 say how many glyphs each range covers and stop when the
/// glyphs run out.
fn charset_span(data: &[u8], at: usize, glyphs: usize) -> Option<usize> {
    let format = *data.get(at)?;
    match format {
        // One SID per glyph after `.notdef`, which is not written down.
        0 => {
            let bytes = 1 + 2 * glyphs.checked_sub(1)?;
            (at.checked_add(bytes)? <= data.len()).then_some(bytes)
        }
        1 | 2 => {
            let step = if format == 1 { 3 } else { 4 };
            let mut covered = 1usize;
            let mut cursor = at.checked_add(1)?;
            while covered < glyphs {
                let range = data.get(cursor..cursor.checked_add(step)?)?;
                let left = if format == 1 {
                    usize::from(range[2])
                } else {
                    usize::from(u16::from_be_bytes([range[2], range[3]]))
                };
                covered = covered.saturating_add(left).saturating_add(1);
                cursor += step;
            }
            cursor.checked_sub(at)
        }
        _ => None,
    }
}

/// How many bytes the encoding at `at` occupies (Tables 11 and 12).
fn encoding_span(data: &[u8], at: usize) -> Option<usize> {
    let format = *data.get(at)?;
    let mut cursor = at.checked_add(1)?;
    match format & 0x7F {
        0 => {
            let codes = usize::from(*data.get(cursor)?);
            cursor = cursor.checked_add(1)?.checked_add(codes)?;
        }
        1 => {
            let ranges = usize::from(*data.get(cursor)?);
            cursor = cursor.checked_add(1)?.checked_add(ranges.checked_mul(2)?)?;
        }
        _ => return None,
    }
    // The high bit says a supplement table follows, three bytes an entry.
    if format & 0x80 != 0 {
        let supplements = usize::from(*data.get(cursor)?);
        cursor = cursor
            .checked_add(1)?
            .checked_add(supplements.checked_mul(3)?)?;
    }
    (cursor <= data.len()).then_some(cursor - at)
}

/// How many bytes the FDSelect at `at` occupies (Tables 27 and 28).
fn fd_select_span(data: &[u8], at: usize, glyphs: usize) -> Option<usize> {
    let bytes = match *data.get(at)? {
        0 => 1usize.checked_add(glyphs)?,
        3 => {
            let ranges = usize::from(u16::from_be_bytes([*data.get(at + 1)?, *data.get(at + 2)?]));
            // Three bytes a range, then the sentinel that ends the last one.
            3usize.checked_add(ranges.checked_mul(3)?)?.checked_add(2)?
        }
        _ => return None,
    };
    (at.checked_add(bytes)? <= data.len()).then_some(bytes)
}

// ---------------------------------------------------------------------------
// Reading a charstring, one token at a time.
// ---------------------------------------------------------------------------

/// Which INDEX a body came out of.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Charstring,
    Local,
    Global,
}

/// What one step of the byte stream turned out to be.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// A number, now on the stack.
    Operand,
    /// `callsubr` or `callgsubr`, with the biased number it popped.
    Call { global: bool, number: f64 },
    /// `endchar` carrying four or five operands: the deprecated `seac`, which
    /// builds an accented letter out of two other glyphs named by
    /// StandardEncoding codes.
    Seac { bchar: u8, achar: u8 },
    /// Anything else, already consumed whole — a `hintmask`'s mask bytes
    /// included, which is the one place the byte stream is not
    /// self-delimiting.
    Plain,
    /// `return`, `endchar`, or an operand the body is too short to hold.
    /// Nothing after this runs.
    End,
}

/// One step, and the bytes it occupies.
#[derive(Clone, Copy, Debug)]
struct Token {
    step: Step,
    start: usize,
    end: usize,
}

/// Pushes as the reader does, so the stack depth this sees is the depth the
/// reader will have — and the stack depth is what decides how many stem hints
/// a `hintmask` has to skip past.
fn push(stack: &mut Vec<f64>, value: f64) {
    if stack.len() < 48 && value.is_finite() {
        stack.push(value);
    }
}

/// Reads one token, advancing `stems` and `stack` exactly as `cff.rs`'s
/// interpreter would.
///
/// The agreement is the point. If this counted stem hints differently the
/// `hintmask` byte count would differ, the token boundaries after it would
/// differ, and a `callsubr` operand would be rewritten in the middle of some
/// other operator — which produces a font that parses and draws nonsense.
fn next_token(code: &[u8], at: usize, stems: &mut usize, stack: &mut Vec<f64>) -> Option<Token> {
    let start = at;
    let cut = |end: usize| {
        Some(Token {
            step: Step::End,
            start,
            end: end.min(code.len()),
        })
    };
    let b = *code.get(at)?;
    match b {
        32..=246 => {
            push(stack, f64::from(b) - 139.0);
            Some(Token {
                step: Step::Operand,
                start,
                end: at + 1,
            })
        }
        247..=250 => {
            let Some(&next) = code.get(at + 1) else {
                return cut(code.len());
            };
            push(
                stack,
                (f64::from(b) - 247.0) * 256.0 + f64::from(next) + 108.0,
            );
            Some(Token {
                step: Step::Operand,
                start,
                end: at + 2,
            })
        }
        251..=254 => {
            let Some(&next) = code.get(at + 1) else {
                return cut(code.len());
            };
            push(
                stack,
                -(f64::from(b) - 251.0) * 256.0 - f64::from(next) - 108.0,
            );
            Some(Token {
                step: Step::Operand,
                start,
                end: at + 2,
            })
        }
        28 => {
            let (Some(&hi), Some(&lo)) = (code.get(at + 1), code.get(at + 2)) else {
                return cut(code.len());
            };
            push(stack, f64::from(i16::from_be_bytes([hi, lo])));
            Some(Token {
                step: Step::Operand,
                start,
                end: at + 3,
            })
        }
        255 => {
            let Some(bytes) = code.get(at + 1..at + 5) else {
                return cut(code.len());
            };
            let value = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            push(stack, f64::from(value) / 65536.0);
            Some(Token {
                step: Step::Operand,
                start,
                end: at + 5,
            })
        }
        // hstem, vstem, hstemhm, vstemhm.
        1 | 3 | 18 | 23 => {
            *stems += stack.len() / 2;
            stack.clear();
            Some(Token {
                step: Step::Plain,
                start,
                end: at + 1,
            })
        }
        // hintmask and cntrmask: an implicit vstem, then a bitmask whose
        // length is the stem count rounded up to whole bytes.
        19 | 20 => {
            *stems += stack.len() / 2;
            stack.clear();
            let mask = stems.div_ceil(8).max(1);
            let end = at.saturating_add(1).saturating_add(mask);
            if end > code.len() {
                return cut(code.len());
            }
            Some(Token {
                step: Step::Plain,
                start,
                end,
            })
        }
        // callsubr and callgsubr. An empty stack ends the charstring, which is
        // what the reader does with it.
        10 | 29 => {
            let Some(number) = stack.pop() else {
                return cut(at + 1);
            };
            Some(Token {
                step: Step::Call {
                    global: b == 29,
                    number,
                },
                start,
                end: at + 1,
            })
        }
        11 => cut(at + 1),
        14 => {
            let n = stack.len();
            let seac = (n == 4 || n == 5)
                .then(|| {
                    let bchar = u8::try_from(*stack.get(n - 2)? as i64).ok()?;
                    let achar = u8::try_from(*stack.get(n - 1)? as i64).ok()?;
                    Some((bchar, achar))
                })
                .flatten();
            stack.clear();
            match seac {
                Some((bchar, achar)) => Some(Token {
                    step: Step::Seac { bchar, achar },
                    start,
                    end: at + 1,
                }),
                None => cut(at + 1),
            }
        }
        12 => {
            let Some(&second) = code.get(at + 1) else {
                return cut(code.len());
            };
            match second {
                // put and get, the transient array. The value is not tracked;
                // only the depth it leaves behind matters here.
                20 => {
                    if stack.pop().is_none() || stack.pop().is_none() {
                        return cut(at + 2);
                    }
                }
                21 => {
                    if stack.pop().is_none() {
                        return cut(at + 2);
                    }
                    push(stack, 0.0);
                }
                _ => stack.clear(),
            }
            Some(Token {
                step: Step::Plain,
                start,
                end: at + 2,
            })
        }
        _ => {
            stack.clear();
            Some(Token {
                step: Step::Plain,
                start,
                end: at + 1,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Walking: which subroutines are reached, and where their numbers are written.
// ---------------------------------------------------------------------------

/// One call site: the bytes holding the biased subroutine number, and the
/// subroutine it resolves to in the *original* font.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Edit {
    start: usize,
    end: usize,
    global: bool,
    index: usize,
}

/// How one body is turned into its subset form: the call sites to renumber,
/// and where the body stops being reachable.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Plan {
    edits: Vec<Edit>,
    /// Bytes to keep. A `return` or an `endchar` is the end of the body in
    /// Type 2 — the language has no branches — so whatever follows one is
    /// unreachable and is dropped rather than carried and renumbered.
    keep: usize,
}

/// The interpreter state one body inherits from its caller and hands back.
///
/// It is one state and not one per body because the reader's is: `cff.rs`
/// runs a whole glyph on a single stack, so a subroutine may be handed
/// operands by its caller and may leave some behind — and the stem count a
/// `hintmask` is measured against accumulates across the call.
#[derive(Clone, Default)]
struct Machine {
    stems: usize,
    stack: Vec<f64>,
}

/// A body, identified by the INDEX it came from and the Private DICT its
/// `callsubr` operands are relative to.
type Body = (usize, Kind, usize);

/// A body together with the hint state it was entered in. Two entries in the
/// same state read the same bytes the same way, which is what makes the memo
/// sound.
type State = (usize, Kind, usize, usize, usize);

struct Walk<'a, 'b> {
    cff: &'b Cff<'a>,
    charstrings: &'b Index<'a>,
    gsubrs: &'b Index<'a>,
    locals: &'b [Index<'a>],
    gsubr_bias: i32,
    local_bias: &'b [i32],
    context: &'b [usize],

    keep: BTreeSet<u16>,
    pending: Vec<u16>,
    used_local: BTreeSet<(usize, usize)>,
    used_global: BTreeSet<(usize, usize)>,
    plans: BTreeMap<Body, Plan>,
    /// Where a body in a given hint state leaves the machine, so a body
    /// called twice the same way is read once.
    memo: BTreeMap<State, Machine>,
    /// The states currently on the stack, which is how a subroutine that
    /// calls itself is noticed rather than followed.
    active: BTreeSet<State>,
    budget: u32,
    refused: bool,
}

impl Walk<'_, '_> {
    fn want(&mut self, glyph: u16) {
        if usize::from(glyph) < self.charstrings.len() && self.keep.insert(glyph) {
            self.pending.push(glyph);
        }
    }

    fn body(
        &mut self,
        ctx: usize,
        kind: Kind,
        index: usize,
        code: &[u8],
        depth: u32,
        machine: &mut Machine,
    ) {
        if self.refused || depth > MAX_DEPTH {
            return;
        }
        let state = (ctx, kind, index, machine.stems, machine.stack.len());
        if self.active.contains(&state) {
            // A subroutine that reaches itself in the same state never
            // terminates on paper; the reader stops it with a depth cap and
            // draws whatever it had. Rewriting that is guesswork.
            self.refused = true;
            return;
        }
        if let Some(exit) = self.memo.get(&state) {
            machine.clone_from(exit);
            return;
        }
        if self.memo.len() >= MAX_STATES {
            self.refused = true;
            return;
        }
        self.active.insert(state);

        let mut plan = Plan {
            edits: Vec::new(),
            keep: code.len(),
        };
        let mut operand: Option<(usize, usize)> = None;
        let mut at = 0usize;

        while at < code.len() {
            if self.budget == 0 {
                self.refused = true;
                break;
            }
            self.budget -= 1;
            let Some(token) = next_token(code, at, &mut machine.stems, &mut machine.stack) else {
                break;
            };
            // A token that consumed nothing would spin; the reader's own
            // arithmetic never produces one, and this is what says so.
            at = token.end.max(at + 1);

            match token.step {
                Step::Operand => operand = Some((token.start, token.end)),
                Step::Plain => operand = None,
                Step::End => {
                    plan.keep = token.end;
                    break;
                }
                Step::Seac { bchar, achar } => {
                    // The two components are named by StandardEncoding codes.
                    // A subset that kept the accented letter and dropped them
                    // draws blank space in every reader that honours `seac`.
                    for code_point in [bchar, achar] {
                        if let Some(glyph) = self.cff.gid_for_standard_code(code_point) {
                            self.want(glyph);
                        }
                    }
                    plan.keep = token.end;
                    break;
                }
                Step::Call { global, number } => {
                    // The number has to be the token immediately before the
                    // call. Anything else — a number computed by the
                    // arithmetic operators, or one left on the stack by the
                    // caller — has no byte to rewrite.
                    let Some((start, end)) = operand.filter(|(_, end)| *end == token.start) else {
                        self.refused = true;
                        break;
                    };
                    operand = None;

                    let applied = if global {
                        self.gsubr_bias
                    } else {
                        self.local_bias.get(ctx).copied().unwrap_or(107)
                    };
                    let Some(target) = f64_to_index(number, applied) else {
                        self.refused = true;
                        break;
                    };
                    let source = if global {
                        self.gsubrs.get(target)
                    } else {
                        self.locals.get(ctx).and_then(|index| index.get(target))
                    };
                    let Some(source) = source else {
                        // The font calls a subroutine it does not carry. The
                        // reader skips it; a subset cannot, because the new
                        // bias would make the same operand name a different
                        // subroutine — one that exists.
                        self.refused = true;
                        break;
                    };

                    plan.edits.push(Edit {
                        start,
                        end,
                        global,
                        index: target,
                    });
                    if global {
                        self.used_global.insert((ctx, target));
                    } else {
                        self.used_local.insert((ctx, target));
                    }
                    let kind = if global { Kind::Global } else { Kind::Local };
                    self.body(ctx, kind, target, source, depth + 1, machine);
                    if self.refused {
                        break;
                    }
                }
            }
        }

        self.active.remove(&state);
        self.memo.insert(state, machine.clone());

        // The same body read in two different hint states can tokenize two
        // different ways — a `hintmask` is as long as the stems declared
        // before it, and those can come from the caller. One body cannot have
        // two subset forms, so disagreement is a refusal rather than a coin
        // toss.
        match self.plans.get(&(ctx, kind, index)) {
            Some(existing) if *existing != plan => self.refused = true,
            Some(_) => {}
            None => {
                self.plans.insert((ctx, kind, index), plan);
            }
        }
    }
}

/// The subroutine a biased operand names, or `None` when it names none.
fn f64_to_index(number: f64, applied: i32) -> Option<usize> {
    if !number.is_finite() {
        return None;
    }
    let raw = number as i64;
    let biased = raw.checked_add(i64::from(applied))?;
    usize::try_from(biased).ok()
}

// ---------------------------------------------------------------------------
// The subsetter.
// ---------------------------------------------------------------------------

/// Builds a CFF program holding only `glyphs`, their `seac` components and the
/// subroutines they reach.
///
/// Returns `None` when the program says something that cannot be rewritten
/// without guessing — see the module documentation for the list — so a caller
/// embeds the original instead of a subset that renders *almost* right.
///
/// Glyph identifiers are unchanged, and so are the charset, the encoding and
/// `FDSelect`: everything outside this function that addresses a glyph keeps
/// addressing the same one.
#[must_use]
pub fn subset_cff(program: &[u8], glyphs: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let cff = Cff::parse(program)?;

    // The structures, read again here rather than borrowed from the reader:
    // the reader keeps what it needs to *run* a charstring, and a writer needs
    // where every table starts and how long it is.
    let header_size = usize::from(*program.get(2)?);
    let (_, names_end) = Index::parse(program, header_size)?;
    let (top_dicts, top_end) = Index::parse(program, names_end)?;
    let (_, strings_end) = Index::parse(program, top_end)?;
    let (gsubrs, _) = Index::parse(program, strings_end)?;
    let top = parse_dict(top_dicts.get(0)?);

    // 12 6 CharstringType: this rewrites Type 2, and a font declaring Type 1
    // charstrings has a different language in its CharStrings INDEX.
    if let Some(&kind) = dict_get(&top, 0x0C06).and_then(<[f64]>::first) {
        if kind != 2.0 {
            return None;
        }
    }

    let charstrings_at = usize_from(dict_get(&top, 17).and_then(<[f64]>::first).copied()?)?;
    let (charstrings, _) = Index::parse(program, charstrings_at)?;
    let glyph_count = charstrings.len();
    if glyph_count == 0 {
        return None;
    }

    let is_cid = dict_get(&top, 0x0C1E).is_some();

    // The FDArray's Font DICTs, parsed but not yet rewritten.
    let mut font_dicts: Vec<Vec<(u16, Vec<f64>)>> = Vec::new();
    if let Some(&at) = dict_get(&top, 0x0C24).and_then(<[f64]>::first) {
        let (index, _) = Index::parse(program, usize_from(at)?)?;
        for i in 0..index.len() {
            font_dicts.push(parse_dict(index.get(i)?));
        }
    }

    // One Private DICT context per Font DICT, plus the top-level one — which
    // is the context of any glyph FDSelect does not place, exactly as the
    // reader falls back to it.
    let top_context = font_dicts.len();
    let mut privates: Vec<Option<PrivateDict<'_>>> = Vec::with_capacity(top_context + 1);
    for dict in &font_dicts {
        privates.push(read_private(program, dict));
    }
    privates.push(read_private(program, &top));

    let locals: Vec<Index<'_>> = privates
        .iter()
        .map(|p| p.as_ref().map_or_else(Index::default, |p| p.subrs.clone()))
        .collect();
    let local_bias: Vec<i32> = locals.iter().map(|index| bias(index.len())).collect();

    // Which context each glyph draws from.
    let fd_select = match dict_get(&top, 0x0C25).and_then(<[f64]>::first) {
        Some(&at) if !font_dicts.is_empty() => read_fd_select(program, at, glyph_count),
        _ => Vec::new(),
    };
    let context: Vec<usize> = (0..glyph_count)
        .map(|gid| match fd_select.get(gid) {
            Some(&fd) if usize::from(fd) < font_dicts.len() => usize::from(fd),
            _ => top_context,
        })
        .collect();

    // ---- walk ------------------------------------------------------------

    let mut walk = Walk {
        cff: &cff,
        charstrings: &charstrings,
        gsubrs: &gsubrs,
        locals: &locals,
        gsubr_bias: bias(gsubrs.len()),
        local_bias: &local_bias,
        context: &context,
        keep: BTreeSet::new(),
        pending: Vec::new(),
        used_local: BTreeSet::new(),
        used_global: BTreeSet::new(),
        plans: BTreeMap::new(),
        memo: BTreeMap::new(),
        active: BTreeSet::new(),
        budget: WALK_BUDGET,
        refused: false,
    };
    // Glyph 0 is `.notdef` and is what a reader draws for anything the font
    // does not cover, so it is never optional.
    walk.want(0);
    for &glyph in glyphs {
        walk.want(glyph);
    }
    while let Some(glyph) = walk.pending.pop() {
        let Some(code) = walk.charstrings.get(usize::from(glyph)) else {
            continue;
        };
        let ctx = walk.context.get(usize::from(glyph)).copied().unwrap_or(0);
        let mut machine = Machine::default();
        walk.body(
            ctx,
            Kind::Charstring,
            usize::from(glyph),
            code,
            0,
            &mut machine,
        );
        if walk.refused {
            return None;
        }
    }
    if walk.refused {
        return None;
    }

    // ---- renumber --------------------------------------------------------

    // Every context's surviving local subroutines, in their original order,
    // so the mapping is a function of the font and not of the walk order.
    let mut local_map: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); top_context + 1];
    let mut kept_local: Vec<Vec<usize>> = vec![Vec::new(); top_context + 1];
    for &(ctx, index) in &walk.used_local {
        kept_local.get_mut(ctx)?.push(index);
    }
    for (ctx, slot) in kept_local.iter().enumerate() {
        for (new, &old) in slot.iter().enumerate() {
            local_map.get_mut(ctx)?.insert(old, new);
        }
    }

    // A global subroutine gets one entry per context that reaches it: the
    // `callsubr` operands inside it are renumbered against that context's
    // local INDEX, and two contexts do not agree on those numbers.
    let kept_global: Vec<(usize, usize)> = walk.used_global.iter().copied().collect();
    let global_map: BTreeMap<(usize, usize), usize> = kept_global
        .iter()
        .enumerate()
        .map(|(new, &key)| (key, new))
        .collect();

    let new_gsubr_bias = bias(kept_global.len());
    let new_local_bias: Vec<i32> = kept_local.iter().map(|slot| bias(slot.len())).collect();

    let rewrite = |ctx: usize, kind: Kind, index: usize, code: &[u8]| -> Option<Vec<u8>> {
        let plan = walk.plans.get(&(ctx, kind, index))?;
        let mut out: Vec<u8> = Vec::with_capacity(plan.keep);
        let mut at = 0usize;
        for edit in &plan.edits {
            if edit.start < at || edit.end > plan.keep {
                return None;
            }
            out.extend_from_slice(code.get(at..edit.start)?);
            let (new, applied) = if edit.global {
                (*global_map.get(&(ctx, edit.index))?, new_gsubr_bias)
            } else {
                (
                    *local_map.get(ctx)?.get(&edit.index)?,
                    *new_local_bias.get(ctx)?,
                )
            };
            let value = i32::try_from(new).ok()?.checked_sub(applied)?;
            out.extend_from_slice(&charstring_int(value)?);
            at = edit.end;
        }
        out.extend_from_slice(code.get(at..plan.keep)?);
        Some(out)
    };

    // ---- the rebuilt INDEXes --------------------------------------------

    let mut new_charstrings: Vec<Vec<u8>> = Vec::with_capacity(glyph_count);
    for gid in 0..glyph_count {
        let glyph = u16::try_from(gid).ok()?;
        if !walk.keep.contains(&glyph) {
            // A single `endchar`: a well-formed charstring that draws nothing
            // and takes the default width. A zero-length entry would be the
            // exact analogue of the dropped `loca` entry, and it would also be
            // a charstring with no terminating operator.
            new_charstrings.push(vec![14]);
            continue;
        }
        let code = charstrings.get(gid)?;
        let ctx = context.get(gid).copied().unwrap_or(top_context);
        new_charstrings.push(rewrite(ctx, Kind::Charstring, gid, code)?);
    }
    let charstrings_bytes = write_index(&new_charstrings)?;

    let mut new_gsubrs: Vec<Vec<u8>> = Vec::with_capacity(kept_global.len());
    for &(ctx, index) in &kept_global {
        new_gsubrs.push(rewrite(ctx, Kind::Global, index, gsubrs.get(index)?)?);
    }
    let gsubrs_bytes = write_index(&new_gsubrs)?;

    let mut locals_bytes: Vec<Vec<u8>> = Vec::with_capacity(top_context + 1);
    for (ctx, slot) in kept_local.iter().enumerate() {
        let mut items: Vec<Vec<u8>> = Vec::with_capacity(slot.len());
        for &index in slot {
            let code = locals.get(ctx)?.get(index)?;
            items.push(rewrite(ctx, Kind::Local, index, code)?);
        }
        locals_bytes.push(write_index(&items)?);
    }

    // ---- the tables copied through --------------------------------------

    let names_bytes = program.get(header_size..names_end)?;
    let strings_bytes = program.get(top_end..strings_end)?;

    // 15 charset: 0, 1 and 2 name a predefined charset rather than a position,
    // and a predefined one is still right because no glyph moved.
    let charset_operand = dict_get(&top, 15)
        .and_then(<[f64]>::first)
        .copied()
        .unwrap_or(0.0);
    let charset_source = match usize_from(charset_operand)? {
        0..=2 => None,
        at => Some(program.get(at..at + charset_span(program, at, glyph_count)?)?),
    };

    // 16 Encoding, which a CID-keyed font does not have. 0 and 1 are the two
    // predefined ones.
    let encoding_operand = if is_cid {
        0.0
    } else {
        dict_get(&top, 16)
            .and_then(<[f64]>::first)
            .copied()
            .unwrap_or(0.0)
    };
    let encoding_source = match usize_from(encoding_operand)? {
        0 | 1 => None,
        at => Some(program.get(at..at + encoding_span(program, at)?)?),
    };

    let fd_select_source = match dict_get(&top, 0x0C25).and_then(<[f64]>::first) {
        Some(&at) if !font_dicts.is_empty() => {
            let at = usize_from(at)?;
            Some(program.get(at..at + fd_select_span(program, at, glyph_count)?)?)
        }
        _ => None,
    };

    // ---- the Private DICTs, whose Subrs offset moves ---------------------

    let mut private_bodies: Vec<Option<Vec<u8>>> = Vec::with_capacity(top_context + 1);
    for (ctx, private) in privates.iter().enumerate() {
        let has_subrs = kept_local.get(ctx).is_some_and(|slot| !slot.is_empty());
        private_bodies.push(match private {
            Some(private) => Some(write_private(&private.dict, has_subrs)?),
            None => None,
        });
    }

    // ---- layout ----------------------------------------------------------

    // The *shape* of the Top DICT is known before any of its offsets are: which
    // of the six pointer operators the subset will carry. Measuring the DICT
    // with that shape and zero for every offset gives its length, and because
    // every offset takes the fixed five-byte form the length does not move when
    // the real numbers go in.
    let mut at = Offsets {
        charset: 0,
        encoding: 0,
        charstrings: 0,
        private: private_bodies.get(top_context)?.as_ref().map(|_| (0, 0)),
        fd_array: (!font_dicts.is_empty()).then_some(0),
        fd_select: fd_select_source.map(|_| 0),
    };
    let top_len = write_top(&top, &at, is_cid)?.len();
    let top_index_len = write_index(&[vec![0u8; top_len]])?.len();

    let mut cursor = 4usize; // the header this writes
    cursor = cursor.checked_add(names_bytes.len())?;
    cursor = cursor.checked_add(top_index_len)?;
    cursor = cursor.checked_add(strings_bytes.len())?;
    cursor = cursor.checked_add(gsubrs_bytes.len())?;

    at.charset = match charset_source {
        Some(bytes) => {
            let here = cursor;
            cursor = cursor.checked_add(bytes.len())?;
            i32::try_from(here).ok()?
        }
        None => i32::try_from(usize_from(charset_operand)?).ok()?,
    };
    at.encoding = match encoding_source {
        Some(bytes) => {
            let here = cursor;
            cursor = cursor.checked_add(bytes.len())?;
            i32::try_from(here).ok()?
        }
        None => i32::try_from(usize_from(encoding_operand)?).ok()?,
    };
    if let Some(bytes) = fd_select_source {
        at.fd_select = Some(i32::try_from(cursor).ok()?);
        cursor = cursor.checked_add(bytes.len())?;
    }
    at.charstrings = i32::try_from(cursor).ok()?;
    cursor = cursor.checked_add(charstrings_bytes.len())?;

    let mut private_at: Vec<Option<(i32, i32)>> = Vec::with_capacity(top_context + 1);
    for (ctx, body) in private_bodies.iter().enumerate() {
        match body {
            Some(body) => {
                private_at.push(Some((
                    i32::try_from(body.len()).ok()?,
                    i32::try_from(cursor).ok()?,
                )));
                cursor = cursor.checked_add(body.len())?;
                if kept_local.get(ctx).is_some_and(|slot| !slot.is_empty()) {
                    cursor = cursor.checked_add(locals_bytes.get(ctx)?.len())?;
                }
            }
            None => private_at.push(None),
        }
    }
    at.private = private_at.get(top_context).copied().flatten();
    if !font_dicts.is_empty() {
        at.fd_array = Some(i32::try_from(cursor).ok()?);
    }
    let body_end = cursor;

    // ---- write -----------------------------------------------------------

    let top_bytes = write_top(&top, &at, is_cid)?;
    // The fixed five-byte operand form is what makes this hold; if it ever
    // does not, the offsets in the DICT point at the wrong bytes and every
    // glyph is wrong. Refusing beats writing that.
    if top_bytes.len() != top_len {
        return None;
    }
    let top_index = write_index(&[top_bytes])?;
    if top_index.len() != top_index_len {
        return None;
    }

    let mut out: Vec<u8> = Vec::with_capacity(cursor + 64);
    // A four-byte header: major 1, minor 0, its own size, and the offset size
    // a Top DICT operand takes — which this writes as four throughout.
    out.extend_from_slice(&[1, 0, 4, 4]);
    out.extend_from_slice(names_bytes);
    out.extend_from_slice(&top_index);
    out.extend_from_slice(strings_bytes);
    out.extend_from_slice(&gsubrs_bytes);
    if let Some(bytes) = charset_source {
        out.extend_from_slice(bytes);
    }
    if let Some(bytes) = encoding_source {
        out.extend_from_slice(bytes);
    }
    if let Some(bytes) = fd_select_source {
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(&charstrings_bytes);
    for (ctx, body) in private_bodies.iter().enumerate() {
        if let Some(body) = body {
            out.extend_from_slice(body);
            if kept_local.get(ctx).is_some_and(|slot| !slot.is_empty()) {
                out.extend_from_slice(locals_bytes.get(ctx)?);
            }
        }
    }
    // Everything above was placed by the cursor that computed the offsets now
    // in the Top DICT. A disagreement here means one of them points at the
    // wrong byte, which is a font whose every glyph is wrong.
    if out.len() != body_end {
        return None;
    }
    if !font_dicts.is_empty() {
        let mut items: Vec<Vec<u8>> = Vec::with_capacity(font_dicts.len());
        for (ctx, dict) in font_dicts.iter().enumerate() {
            items.push(write_font_dict(
                dict,
                private_at.get(ctx).copied().flatten(),
            )?);
        }
        out.extend_from_slice(&write_index(&items)?);
    }

    verify(program, &out, &walk.keep).then_some(out)
}

/// Outlines every retained glyph in both programs and compares the segments.
///
/// This is the property the whole exercise rests on, checked by the writer on
/// its own output rather than only in a test: a subset that renders one glyph
/// differently is worse than a font that is merely large (ruling 2), and the
/// cost here is proportional to the subset rather than to the face.
fn verify(original: &[u8], subset: &[u8], keep: &BTreeSet<u16>) -> bool {
    let (Some(before), Some(after)) = (Cff::parse(original), Cff::parse(subset)) else {
        return false;
    };
    if before.glyph_count() != after.glyph_count() || before.is_cid() != after.is_cid() {
        return false;
    }
    for &glyph in keep {
        let (Some(a), Some(b)) = (before.outline(glyph), after.outline(glyph)) else {
            return false;
        };
        if a.segments != b.segments {
            return false;
        }
        if before.font_matrix_for(glyph) != after.font_matrix_for(glyph) {
            return false;
        }
    }
    true
}

/// A Private DICT as it was found: its operators, and its local subroutines.
struct PrivateDict<'a> {
    dict: Vec<(u16, Vec<f64>)>,
    subrs: Index<'a>,
}

/// Reads the Private DICT a Top DICT or a Font DICT points at.
fn read_private<'a>(data: &'a [u8], dict: &[(u16, Vec<f64>)]) -> Option<PrivateDict<'a>> {
    let operands = dict_get(dict, 18)?;
    let (&size, &offset) = (operands.first()?, operands.get(1)?);
    let (size, offset) = (usize_from(size)?, usize_from(offset)?);
    let bytes = data.get(offset..offset.checked_add(size)?.min(data.len()))?;
    let parsed = parse_dict(bytes);
    // 19 Subrs, whose offset is measured from the Private DICT rather than
    // from the start of the font.
    let subrs = match dict_get(&parsed, 19).and_then(<[f64]>::first) {
        Some(&rel) => usize_from(rel)
            .and_then(|rel| Index::parse(data, offset.checked_add(rel)?))
            .map_or_else(Index::default, |(index, _)| index),
        None => Index::default(),
    };
    Some(PrivateDict {
        dict: parsed,
        subrs,
    })
}

/// Writes a Private DICT: everything it said, less `Subrs`, plus the `Subrs`
/// this subset needs.
fn write_private(dict: &[(u16, Vec<f64>)], has_subrs: bool) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (op, operands) in dict {
        // 19 Subrs is recomputed; everything else — the widths, the blue
        // zones, the stem snaps — is what another reader hints with, and this
        // engine dropping it would make the subset render worse elsewhere
        // than the face does.
        if *op == 19 {
            continue;
        }
        for value in operands {
            out.extend_from_slice(&dict_operand(*value));
        }
        out.extend_from_slice(&dict_op(*op));
    }
    if has_subrs {
        // The INDEX follows the DICT immediately, so the offset is the DICT's
        // own length — including the six bytes this entry adds to it.
        let total = out.len().checked_add(6)?;
        out.extend_from_slice(&dict_int(i32::try_from(total).ok()?));
        out.push(19);
        if out.len() != total {
            return None;
        }
    }
    Some(out)
}

/// Writes one Font DICT, repointed at its Private DICT.
fn write_font_dict(dict: &[(u16, Vec<f64>)], private: Option<(i32, i32)>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (op, operands) in dict {
        if *op == 18 {
            continue;
        }
        for value in operands {
            out.extend_from_slice(&dict_operand(*value));
        }
        out.extend_from_slice(&dict_op(*op));
    }
    if let Some((size, offset)) = private {
        out.extend_from_slice(&dict_int(size));
        out.extend_from_slice(&dict_int(offset));
        out.push(18);
    }
    Some(out)
}

/// Where each rebuilt table landed.
#[derive(Clone, Copy, Default)]
struct Offsets {
    charset: i32,
    encoding: i32,
    charstrings: i32,
    private: Option<(i32, i32)>,
    fd_array: Option<i32>,
    fd_select: Option<i32>,
}

/// Writes the Top DICT: everything the original said, with the six operators
/// that hold an offset replaced by where this subset put the thing.
fn write_top(dict: &[(u16, Vec<f64>)], at: &Offsets, is_cid: bool) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (op, operands) in dict {
        // The offsets are recomputed below. Everything else is copied: `ROS`,
        // which is what makes the font CID-keyed and must stay first;
        // `FontMatrix`, without which the glyphs are the wrong size; the SIDs
        // naming the face, which still index the copied String INDEX.
        if matches!(op, 15 | 16 | 17 | 18 | 0x0C24 | 0x0C25) {
            continue;
        }
        for value in operands {
            out.extend_from_slice(&dict_operand(*value));
        }
        out.extend_from_slice(&dict_op(*op));
    }

    out.extend_from_slice(&dict_int(at.charset));
    out.push(15);
    if !is_cid {
        out.extend_from_slice(&dict_int(at.encoding));
        out.push(16);
    }
    out.extend_from_slice(&dict_int(at.charstrings));
    out.push(17);
    if let Some((size, offset)) = at.private {
        out.extend_from_slice(&dict_int(size));
        out.extend_from_slice(&dict_int(offset));
        out.push(18);
    }
    if let Some(fd_array) = at.fd_array {
        out.extend_from_slice(&dict_int(fd_array));
        out.extend_from_slice(&dict_op(0x0C24));
        // FDSelect is only meaningful with an FDArray, and a font with one
        // Font DICT may leave it out: every glyph is then in Font DICT 0.
        if let Some(fd_select) = at.fd_select {
            out.extend_from_slice(&dict_int(fd_select));
            out.extend_from_slice(&dict_op(0x0C25));
        }
    }
    Some(out)
}

/// A DICT operand as a position in the file, refusing anything that is not
/// one.
fn usize_from(value: f64) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value > 2_147_483_647.0 {
        return None;
    }
    Some(value as usize)
}

#[cfg(test)]
mod tests;
