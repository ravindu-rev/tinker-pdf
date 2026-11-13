//! The fuzz crate's library: the choice reader every structure-aware target
//! reads its input through, and the generators more than one target builds
//! on. `docs/design/structured-fuzzing.md` is the design.
//!
//! Dev tooling only. Nothing here ships in the engine; the targets in
//! `fuzz_targets/` link it, and `cargo test --manifest-path fuzz/Cargo.toml
//! --lib` runs its tests on stable.

use std::path::PathBuf;

pub mod cff;
pub mod content;
pub mod cos;
pub mod css;
pub mod document;
pub mod glyf;
pub mod jpx;
pub mod markup;

/// A target's input, read front to back as a sequence of decisions.
///
/// An input that has run out reads as zeros, and zero is the smallest choice
/// wherever a generator asks one, so every generator ends on every input and
/// a short input is a small structure. A choice of up to 256 options takes
/// one byte, so a mutation of one byte changes one decision.
#[derive(Clone, Debug)]
pub struct Choices<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Choices<'a> {
    /// A reader over `data`, at its first byte.
    pub fn new(data: &'a [u8]) -> Choices<'a> {
        Choices { data, at: 0 }
    }

    /// The next byte, or zero once the input has run out.
    pub fn byte(&mut self) -> u8 {
        let byte = self.data.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        byte
    }

    /// One byte's low bit.
    pub fn flag(&mut self) -> bool {
        self.byte() & 1 == 1
    }

    /// A value in `0..n`: one byte for `n` up to 256, two up to 65 536, four
    /// above, and none at all, giving 0, for `n` of 0 or 1.
    pub fn below(&mut self, n: usize) -> usize {
        match n {
            0 | 1 => 0,
            2..=0x100 => usize::from(self.byte()) % n,
            0x101..=0x1_0000 => usize::from(u16::from_be_bytes([self.byte(), self.byte()])) % n,
            _ => {
                let value =
                    u32::from_be_bytes([self.byte(), self.byte(), self.byte(), self.byte()]);
                // A `u32` is below any `n` past it, so the remainder is too.
                usize::try_from(value).unwrap_or(usize::MAX) % n
            }
        }
    }

    /// A value in `lo..=hi`, read as [`Choices::below`] reads the span; eight
    /// bytes when the span is wider than a `u32`. `lo` when `hi < lo`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (i128::from(hi) - i128::from(lo) + 1) as u128;
        let pick = if span <= 1 << 32 {
            self.below(span as usize) as u128
        } else {
            let mut bytes = [0u8; 8];
            for byte in &mut bytes {
                *byte = self.byte();
            }
            u128::from(u64::from_be_bytes(bytes)) % span
        };
        (i128::from(lo) + pick as i128) as i64
    }

    /// One of `items`, read as [`Choices::below`] over their count.
    ///
    /// # Panics
    /// On an empty slice, which no generator passes: every call site names a
    /// constant list.
    pub fn pick<'t, T>(&mut self, items: &'t [T]) -> &'t T {
        let at = self.below(items.len());
        &items[at]
    }

    /// A run of the input itself: a length read as `below(max + 1)`, then that
    /// many bytes, zeros where the input runs out first.
    pub fn bytes(&mut self, max: usize) -> Vec<u8> {
        let len = self.below(max.saturating_add(1));
        let start = self.at.min(self.data.len());
        let end = start.saturating_add(len).min(self.data.len());
        let mut run = self.data[start..end].to_vec();
        run.resize(len, 0);
        self.at = self.at.saturating_add(len);
        run
    }

    /// Whether every byte of the input has been read.
    pub fn spent(&self) -> bool {
        self.at >= self.data.len()
    }

    /// How many bytes of the input the decisions so far have read.
    pub fn consumed(&self) -> usize {
        self.at.min(self.data.len())
    }
}

/// The first input of a fixed pseudo-random sequence that `wanted` accepts,
/// cut to the bytes `read` says the generator read of it and stripped of the
/// zeros at its end, which read the same as no bytes at all.
///
/// The seed writers' search: `wanted` builds the structure and says whether
/// it has the feature a seed is named for, and `read` how much of the input
/// building it consumed. An input that cuts to one of the `taken` seeds is
/// passed over, so no two seeds of a corpus are one input under two names.
/// The sequence is xorshift64* from a fixed state, so a writer run again finds
/// the same seeds while the generator reads its choices in the same order.
pub fn first_input(
    len: usize,
    tries: usize,
    wanted: impl Fn(&[u8]) -> bool,
    read: impl Fn(&[u8]) -> usize,
    taken: &[Vec<u8>],
) -> Option<Vec<u8>> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut input = vec![0u8; len];
    for _ in 0..tries {
        for byte in &mut input {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            *byte = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
        }
        if wanted(&input) {
            let mut seed = input.clone();
            seed.truncate(read(&input));
            while seed.last() == Some(&0) {
                seed.pop();
            }
            if !taken.contains(&seed) {
                return Some(seed);
            }
        }
    }
    None
}

/// Writes `seeds` into `fuzz/corpus/<target>/`, each under its name.
///
/// # Panics
/// When a seed cannot be written, which is a seed writer's failure to report.
pub fn write_seeds(target: &str, seeds: &[(&str, Vec<u8>)]) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(target);
    std::fs::create_dir_all(&dir).expect("the corpus directory");
    for (name, bytes) in seeds {
        std::fs::write(dir.join(name), bytes).expect("a seed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_that_has_run_out_reads_as_zeros() {
        let mut c = Choices::new(&[7]);
        assert_eq!(c.byte(), 7);
        assert!(c.spent());
        assert_eq!(c.byte(), 0);
        assert_eq!(c.below(100), 0);
        assert!(!c.flag());
        assert_eq!(c.range(-5, 5), -5);
        assert!(c.bytes(10).is_empty());
        assert_eq!(c.consumed(), 1);
    }

    #[test]
    fn a_choice_takes_the_bytes_its_span_needs() {
        let mut c = Choices::new(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(c.below(1), 0, "one option reads nothing");
        assert_eq!(c.consumed(), 0);
        assert_eq!(c.below(256), 1, "one byte up to 256");
        assert_eq!(c.below(65_536), 0x0203, "two up to 65 536");
        assert_eq!(c.below(1 << 20), 0x0405_0607 % (1 << 20), "four above");
        assert_eq!(c.consumed(), 7);
    }

    #[test]
    fn a_range_holds_its_ends() {
        for byte in 0..=255u8 {
            let input = [byte];
            let mut c = Choices::new(&input);
            let value = c.range(-3, 3);
            assert!((-3..=3).contains(&value));
        }
        let mut wide = Choices::new(&[0xff; 8]);
        let value = wide.range(i64::MIN, i64::MAX);
        assert_eq!(wide.consumed(), 8);
        assert!(value >= i64::MIN);
        assert_eq!(Choices::new(&[]).range(4, 2), 4);
    }

    #[test]
    fn a_run_of_bytes_is_the_input_itself() {
        let mut c = Choices::new(&[3, b'a', b'b', b'c', b'd']);
        assert_eq!(c.bytes(8), b"abc");
        assert_eq!(c.byte(), b'd');
        let mut short = Choices::new(&[5, b'x']);
        assert_eq!(short.bytes(8), b"x\0\0\0\0", "zeros where the input ends");
        assert_eq!(short.byte(), 0);
    }

    #[test]
    fn the_same_input_makes_the_same_choices() {
        let input = [9u8, 200, 13, 77, 0, 255];
        let read = |data: &[u8]| {
            let mut c = Choices::new(data);
            (c.below(10), c.flag(), c.range(-100, 100), c.bytes(3))
        };
        assert_eq!(read(&input), read(&input));
    }

    #[test]
    fn a_found_input_is_cut_to_what_was_read() {
        let wanted = |data: &[u8]| Choices::new(data).below(4) == 3;
        let seed = first_input(64, 1000, wanted, |_| 2, &[]).expect("one in four inputs has it");
        assert!(seed.len() <= 2);
        assert_eq!(Choices::new(&seed).below(4), 3);
        assert!(seed.last() != Some(&0));
        // A seed already taken is passed over for the next input that has it.
        let next = first_input(64, 1000, wanted, |_| 2, &[seed.clone()]).expect("another");
        assert_ne!(next, seed);
        assert_eq!(Choices::new(&next).below(4), 3);
    }
}
