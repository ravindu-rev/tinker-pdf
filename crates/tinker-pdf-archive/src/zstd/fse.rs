//! Finite State Entropy: reading a table description (RFC 8878 §4.1.1) and
//! building the decoding table it describes, and the three predefined
//! distributions of §3.1.1.3.2.2.

use super::bits::Forward;

/// One row of a decoding table: the symbol this state decodes to, and how to
/// reach the next state — read `bits` bits and add them to `base`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) symbol: u8,
    pub(super) bits: u8,
    pub(super) base: u16,
}

/// A decoding table of `1 << log` rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Table {
    pub(super) log: u32,
    pub(super) rows: Vec<Row>,
}

impl Table {
    /// The row for `state`. States come from `log` bits or from a row's
    /// `base` plus `bits` bits, and the table is built so that both land
    /// inside it; the fallback is never taken, and is a row rather than a
    /// panic so that belief is not load-bearing.
    pub(super) fn row(&self, state: usize) -> Row {
        self.rows.get(state).copied().unwrap_or_default()
    }

    /// The table `RLE_Mode` names: one symbol, every state, no bits.
    pub(super) fn rle(symbol: u8) -> Table {
        Table {
            log: 0,
            rows: vec![Row {
                symbol,
                bits: 0,
                base: 0,
            }],
        }
    }
}

/// Why a description was refused. The caller turns it into its own section's
/// error: the same fault is a bad literals section in one place and a bad
/// sequences section in another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    /// The description ran past the bytes it was given.
    Truncated,
    /// Anything else: an accuracy log past the context's maximum, a symbol
    /// past its alphabet, or probabilities that do not sum to the table.
    Corrupt,
}

/// Reads a table description from the start of `src`: the normalised
/// probabilities, `-1` for "less than one", and the accuracy log. Returns
/// them with the bytes the description took.
///
/// `max_symbol` is the largest symbol the context allows and `max_log` its
/// largest accuracy log. RFC 8878 asks for "the expected number of
/// symbols"; every encoder writes the distribution only up to its last
/// present symbol, and the reference decoder accepts that, so this takes
/// "at most" for "expected" and refuses only a symbol past the alphabet.
pub(super) fn read_distribution(
    src: &[u8],
    max_symbol: usize,
    max_log: u32,
) -> Result<(Vec<i16>, u32, usize), Fault> {
    let mut bits = Forward::new(src);
    let log = bits.read(4) + 5;
    if log > max_log {
        return Err(Fault::Corrupt);
    }
    // Points still to hand out, plus one: a symbol may take all of them.
    let mut remaining: i32 = (1 << log) + 1;
    // The largest power of two not above `remaining`; values are read in
    // `width` or `width - 1` bits around it.
    let mut threshold: i32 = 1 << log;
    let mut width: u32 = log + 1;
    let mut probs: Vec<i16> = Vec::new();
    let mut after_zero = false;

    loop {
        if after_zero {
            // Two-bit repeat flags: that many more zeros, and a 3 means
            // another flag follows.
            loop {
                let repeat = bits.read(2);
                probs.extend(std::iter::repeat_n(0, repeat as usize));
                if repeat != 3 || probs.len() > max_symbol {
                    break;
                }
            }
            if probs.len() > max_symbol {
                break;
            }
        }
        // §4.1.1's small-values-use-one-bit-less scheme: of the `width`-bit
        // field, the low values that do not need the top bit are read in
        // `width - 1` bits.
        let max = (2 * threshold - 1) - remaining;
        let field = bits.peek(width) as i32;
        let mut value = field & (threshold - 1);
        if value < max {
            bits.skip(width - 1);
        } else {
            value = field & (2 * threshold - 1);
            if value >= threshold {
                value -= max;
            }
            bits.skip(width);
        }
        // `value` is at most `remaining` (the scheme cannot express more),
        // so a probability never takes `remaining` below one.
        let prob = value - 1;
        remaining -= prob.abs();
        probs.push(prob as i16);
        after_zero = prob == 0;
        if remaining < threshold {
            if remaining <= 1 {
                break;
            }
            // One bit past `floor(log2 remaining)`.
            width = 32 - (remaining as u32).leading_zeros();
            threshold = 1 << (width - 1);
        }
        if probs.len() > max_symbol {
            break;
        }
    }
    // Stopped by the alphabet rather than by the points running out: a
    // symbol past the alphabet would have needed the rest.
    if remaining != 1 || probs.len() > max_symbol + 1 {
        return Err(Fault::Corrupt);
    }
    let used = bits.bytes_used();
    if used > src.len() as u64 {
        return Err(Fault::Truncated);
    }
    Ok((probs, log, used as usize))
}

/// Builds the decoding table for `probs` at accuracy `log` (§4.1.1, "From
/// Normalized Distribution to Decoding Tables").
pub(super) fn build(probs: &[i16], log: u32) -> Result<Table, Fault> {
    let size = 1usize << log;
    // "Less than one" symbols take one row each from the top of the table;
    // `high` is the lowest row they hold, and every row from it up is
    // theirs.
    let mut high = size;
    let mut symbols = vec![0u8; size];
    // Per symbol, the next state number to hand out: its probability, or 1
    // for a "less than one" symbol.
    let mut next = [0u32; 256];
    let mut total = 0usize;
    for (symbol, &prob) in probs.iter().enumerate() {
        let (Ok(symbol_u8), Some(slot)) = (u8::try_from(symbol), next.get_mut(symbol)) else {
            return Err(Fault::Corrupt);
        };
        if prob == -1 {
            high = high.checked_sub(1).ok_or(Fault::Corrupt)?;
            *symbols.get_mut(high).ok_or(Fault::Corrupt)? = symbol_u8;
            *slot = 1;
            total += 1;
        } else if prob > 0 {
            *slot = prob as u32;
            total += prob as usize;
        } else if prob < -1 {
            return Err(Fault::Corrupt);
        }
    }
    if total != size {
        return Err(Fault::Corrupt);
    }

    // The spread: each symbol in natural order takes `prob` rows, stepping
    // through the table by a stride coprime to its size and skipping the
    // rows at the top. With the counts summing to the table, every row below
    // `high` is visited exactly once.
    let step = (size >> 1) + (size >> 3) + 3;
    let mask = size - 1;
    let mut position = 0usize;
    for (symbol, &prob) in probs.iter().enumerate() {
        for _ in 0..prob.max(0) {
            *symbols.get_mut(position).ok_or(Fault::Corrupt)? = symbol as u8;
            position = (position + step) & mask;
            while position >= high {
                position = (position + step) & mask;
            }
        }
    }
    if position != 0 {
        return Err(Fault::Corrupt);
    }

    // Each symbol's rows, in state order, get the state numbers from its
    // probability up to twice it: a number `n` reads `log - floor(log2 n)`
    // bits onto `(n << bits) - size`.
    let mut rows = Vec::with_capacity(size);
    for &symbol in &symbols {
        let slot = next.get_mut(usize::from(symbol)).ok_or(Fault::Corrupt)?;
        let n = *slot;
        *slot += 1;
        if n == 0 {
            return Err(Fault::Corrupt);
        }
        let bits = log - (31 - n.leading_zeros());
        let base = (n << bits) as usize - size;
        rows.push(Row {
            symbol,
            bits: bits as u8,
            base: u16::try_from(base).map_err(|_| Fault::Corrupt)?,
        });
    }
    Ok(Table { log, rows })
}

/// §3.1.1.3.2.2.1: literals length codes 0–35, accuracy 6.
pub(super) const LITERALS_LENGTH_DEFAULT: [i16; 36] = [
    4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, //
    2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, //
    -1, -1, -1, -1,
];

/// §3.1.1.3.2.2.2: match length codes 0–52, accuracy 6.
pub(super) const MATCH_LENGTH_DEFAULT: [i16; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, //
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, //
    -1, -1, -1, -1, -1,
];

/// §3.1.1.3.2.2.3: offset codes 0–28, accuracy 5.
pub(super) const OFFSET_DEFAULT: [i16; 29] = [
    1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, //
    1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1,
];
