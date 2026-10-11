//! The Huffman code literals are written in: its description (RFC 8878
//! §4.2.1), the prefix codes it implies, and the one- and four-stream
//! layouts a literals section carries them in (§4.2.2, §3.1.1.3.1.6).

use super::bits::Backward;
use super::fse::{self, Fault};

/// §4.2.1: "This specification limits the maximum code length to 11 bits."
const MAX_BITS: u32 = 11;

/// A decoding table: indexed by the next `max_bits` bits of a stream, each
/// row the symbol those bits begin with and the length of its code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Huffman {
    max_bits: u32,
    rows: Vec<(u8, u8)>,
}

/// How a tree description was written, for the census.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Weights {
    /// FSE-compressed, two interleaved states (§4.2.1.2).
    Fse,
    /// Four bits each (§4.2.1.1).
    Direct,
}

/// Reads a `Huffman_Tree_Description` from the start of `src`. Returns the
/// table, how its weights were written and the bytes it took.
pub(super) fn read_tree(src: &[u8]) -> Result<(Huffman, Weights, usize), Fault> {
    let (&header, rest) = src.split_first().ok_or(Fault::Truncated)?;
    let mut weights = Vec::with_capacity(256);
    if header < 128 {
        let size = usize::from(header);
        let data = rest.get(..size).ok_or(Fault::Truncated)?;
        fse_weights(data, &mut weights)?;
        Ok((build(&weights)?, Weights::Fse, 1 + size))
    } else {
        // `headerByte - 127` weights, two to a byte, high nibble first.
        let count = usize::from(header) - 127;
        let bytes = count.div_ceil(2);
        let data = rest.get(..bytes).ok_or(Fault::Truncated)?;
        for &byte in data {
            weights.push(byte >> 4);
            weights.push(byte & 0x0F);
        }
        weights.truncate(count);
        Ok((build(&weights)?, Weights::Direct, 1 + bytes))
    }
}

/// §4.2.1.2: the weights as one FSE bitstream with two states sharing a
/// table, taking turns. The count is not written: decoding stops when a
/// state update runs out of bits, and the other state's symbol is the last.
fn fse_weights(data: &[u8], weights: &mut Vec<u8>) -> Result<(), Fault> {
    let (probs, log, used) = fse::read_distribution(data, 255, 6)?;
    let table = fse::build(&probs, log)?;
    let mut bits = Backward::new(data.get(used..).unwrap_or(&[])).ok_or(Fault::Corrupt)?;
    let mut states = [bits.read(log) as usize, bits.read(log) as usize];
    // Two initial states the stream does not hold are corruption, not two
    // weights read from zeros: the zstd format document's current text says
    // so, and it is what `golden-decompression-errors/truncated_huff_state`
    // holds a decoder to.
    if bits.overflowed() {
        return Err(Fault::Corrupt);
    }
    let mut turn = 0usize;
    loop {
        // At most 255 weights: the 256th symbol's is never written. The
        // check leaves room for the one the other state still holds.
        if weights.len() > 253 {
            return Err(Fault::Corrupt);
        }
        let state = states.get_mut(turn).ok_or(Fault::Corrupt)?;
        let row = table.row(*state);
        weights.push(row.symbol);
        *state = usize::from(row.base) + bits.read(u32::from(row.bits)) as usize;
        if bits.overflowed() {
            let other = states.get(turn ^ 1).copied().unwrap_or(0);
            weights.push(table.row(other).symbol);
            return Ok(());
        }
        turn ^= 1;
    }
}

/// §4.2.1.3: the prefix codes the weights imply. The last symbol's weight is
/// the one that completes the sum to a power of two; codes are handed out
/// from the lowest weight up, and in symbol order within a weight.
fn build(weights: &[u8]) -> Result<Huffman, Fault> {
    if weights.len() > 255 {
        return Err(Fault::Corrupt);
    }
    let mut total: u32 = 0;
    for &w in weights {
        if u32::from(w) > MAX_BITS {
            return Err(Fault::Corrupt);
        }
        if w > 0 {
            total += 1 << (w - 1);
        }
    }
    if total == 0 {
        return Err(Fault::Corrupt);
    }
    let max_bits = 32 - total.leading_zeros();
    if max_bits > MAX_BITS {
        return Err(Fault::Corrupt);
    }
    let rest = (1u32 << max_bits) - total;
    if !rest.is_power_of_two() {
        return Err(Fault::Corrupt);
    }
    let last = rest.trailing_zeros() + 1;
    let mut all = weights.to_vec();
    all.push(last as u8);
    // §4.2.1: "If no literal has a Weight of 1, then the data is considered
    // corrupted."
    if !all.contains(&1) {
        return Err(Fault::Corrupt);
    }

    // Where each weight's codes start, lowest weight first; a weight-`w`
    // code covers `2^(w-1)` rows of the table.
    let mut count = [0u32; MAX_BITS as usize + 1];
    for &w in &all {
        if let Some(c) = count.get_mut(usize::from(w)) {
            *c += 1;
        }
    }
    let mut start = [0usize; MAX_BITS as usize + 1];
    let mut at = 0usize;
    for w in 1..=max_bits as usize {
        if let (Some(s), Some(&c)) = (start.get_mut(w), count.get(w)) {
            *s = at;
            at += (c as usize) << (w - 1);
        }
    }
    let size = 1usize << max_bits;
    if at != size {
        return Err(Fault::Corrupt);
    }
    let mut rows = vec![(0u8, 0u8); size];
    for (symbol, &w) in all.iter().enumerate() {
        if w == 0 {
            continue;
        }
        let span = 1usize << (w - 1);
        let from = start.get_mut(usize::from(w)).ok_or(Fault::Corrupt)?;
        let length = (max_bits + 1 - u32::from(w)) as u8;
        for row in rows.get_mut(*from..*from + span).ok_or(Fault::Corrupt)? {
            *row = (symbol as u8, length);
        }
        *from += span;
    }
    Ok(Huffman { max_bits, rows })
}

impl Huffman {
    /// Decodes `count` symbols from one stream onto `out`. The stream must
    /// be used exactly: §4.2.2, "If a bitstream is not entirely and exactly
    /// consumed ... the decoding process is considered faulty."
    pub(super) fn decode_stream(
        &self,
        src: &[u8],
        count: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Fault> {
        let mut bits = Backward::new(src).ok_or(Fault::Corrupt)?;
        for _ in 0..count {
            let (symbol, length) = self
                .rows
                .get(bits.peek(self.max_bits) as usize)
                .copied()
                .unwrap_or((0, 0));
            bits.skip(u32::from(length));
            if bits.overflowed() {
                return Err(Fault::Corrupt);
            }
            out.push(symbol);
        }
        if bits.finished() {
            Ok(())
        } else {
            Err(Fault::Corrupt)
        }
    }

    /// Decodes four streams behind their six-byte jump table (§3.1.1.3.1.6):
    /// the first three each hold `(total + 3) / 4` symbols and the fourth
    /// the rest.
    pub(super) fn decode_four(
        &self,
        src: &[u8],
        total: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Fault> {
        let (jump, streams) = src.split_first_chunk::<6>().ok_or(Fault::Corrupt)?;
        let [a0, a1, b0, b1, c0, c1] = *jump;
        let sizes = [
            usize::from(u16::from_le_bytes([a0, a1])),
            usize::from(u16::from_le_bytes([b0, b1])),
            usize::from(u16::from_le_bytes([c0, c1])),
        ];
        let segment = total.div_ceil(4);
        let last = total.checked_sub(3 * segment).ok_or(Fault::Corrupt)?;
        let mut rest = streams;
        for size in sizes {
            let (stream, tail) = rest.split_at_checked(size).ok_or(Fault::Corrupt)?;
            self.decode_stream(stream, segment, out)?;
            rest = tail;
        }
        // "Stream4_Size is necessarily >= 1": an empty fourth stream has no
        // end mark and `decode_stream` refuses it.
        self.decode_stream(rest, last, out)
    }
}
