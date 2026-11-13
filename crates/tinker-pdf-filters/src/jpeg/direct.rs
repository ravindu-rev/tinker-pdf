//! The direct forms of the decoder's fast paths, kept in the test build to
//! hold those paths to (`docs/design/jpeg-speed.md`, "The invariant").
//!
//! Each is the straightforward reading of its clause — a bit at a time, every
//! product made — and each fast path in `super` is tested against it over
//! inputs that reach every branch of both, so that a full-size decode stays
//! byte-identical for every stream.

use super::HuffmanTable;

/// Reads bits from the entropy-coded segment a byte at a time, unstuffing as
/// it goes: [`super::BitReader`] without its word.
pub(super) struct BitwiseReader<'a> {
    data: &'a [u8],
    pub(super) at: usize,
    bits: u32,
    pub(super) count: u32,
    /// True once the reader has run past the end.
    pub(super) exhausted: bool,
    /// Set when `bit` walked over a restart marker by itself, so `restart`
    /// knows not to skip a second one and lose a whole interval.
    pub(super) crossed_restart: bool,
}

impl<'a> BitwiseReader<'a> {
    pub(super) fn new(data: &'a [u8]) -> BitwiseReader<'a> {
        BitwiseReader {
            data,
            at: 0,
            bits: 0,
            count: 0,
            exhausted: false,
            crossed_restart: false,
        }
    }

    /// One bit, MSB first. Past the end this returns zeros, which lets a
    /// truncated image decode to something rather than nothing.
    pub(super) fn bit(&mut self) -> u32 {
        if self.count == 0 {
            let Some(&byte) = self.data.get(self.at) else {
                self.exhausted = true;
                return 0;
            };
            self.at += 1;

            // T.81 F.1.2.3: a 0xFF in entropy-coded data is followed by a
            // stuffed zero; anything else is a marker and ends the segment.
            if byte == 0xFF {
                match self.data.get(self.at) {
                    Some(0x00) => self.at += 1,
                    Some(&m) if (0xD0..=0xD7).contains(&m) => {
                        // A restart marker: skip it and carry on.
                        self.at += 1;
                        self.crossed_restart = true;
                        return self.bit();
                    }
                    _ => {
                        self.exhausted = true;
                        return 0;
                    }
                }
            }
            self.bits = u32::from(byte);
            self.count = 8;
        }
        self.count -= 1;
        (self.bits >> self.count) & 1
    }

    pub(super) fn bits(&mut self, n: u32) -> i32 {
        let mut value = 0i32;
        for _ in 0..n.min(31) {
            value = (value << 1) | self.bit() as i32;
        }
        value
    }

    /// Decodes one Huffman-coded symbol.
    pub(super) fn huffman(&mut self, table: &HuffmanTable) -> Option<u8> {
        let mut code = 0i32;
        for length in 1..=16usize {
            code = (code << 1) | self.bit() as i32;
            let max = table.max_code.get(length).copied().unwrap_or(-1);
            if max >= 0 && code <= max {
                let offset = table.value_offset.get(length).copied().unwrap_or(0);
                let index = usize::try_from(offset + code).ok()?;
                return table.values.get(index).copied();
            }
            if self.exhausted {
                return None;
            }
        }
        None
    }

    /// Resets at a restart marker (T.81 F.2.1.3.1).
    pub(super) fn restart(&mut self) {
        self.count = 0;
        if self.crossed_restart {
            // The bit reader already stepped over it while filling its
            // accumulator; skipping another would drop an entire interval's
            // worth of blocks.
            self.crossed_restart = false;
            return;
        }

        // Skip to just past the next RSTn marker.
        while self.at + 1 < self.data.len() {
            if self.data.get(self.at) == Some(&0xFF) {
                if let Some(&m) = self.data.get(self.at + 1) {
                    if (0xD0..=0xD7).contains(&m) {
                        self.at += 2;
                        return;
                    }
                }
            }
            self.at += 1;
        }
    }
}
