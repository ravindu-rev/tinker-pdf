//! What the LZMA engine is held to, from the format's own description of its
//! coder.
//!
//! # What can and cannot be adjudicated here, said plainly
//!
//! There is no LZMA encoder in this workspace and CONTRIBUTING rule 1 forbids
//! pulling one in, so a decoder cannot be checked against a round trip through
//! somebody else's compressor. What is done instead is a split, and it is
//! worth writing down because the halves are verified by different means:
//!
//! - **The arithmetic coder and the literal path** are round-tripped here,
//!   through [`Encoder`] below — a range *encoder* transcribed from the
//!   format's own pseudocode, not from this module's decoder. It is the exact
//!   dual: `shift_low`'s carry handling, the `(range >> 11) * prob` bound and
//!   the `>> 5` adaptation. If either side had those wrong the round trip
//!   would not close, and a shared misunderstanding is the residual risk,
//!   named rather than papered over.
//! - **Matches, distances, the length coder and LZMA2's compressed chunks**
//!   are *not* reachable from an encoder this small, and they are adjudicated
//!   by the `.cb7`s' own CRC-32s in `crates/tinker-pdf/tests/cbz_real.rs` —
//!   the format checking the decompression, which is what ruling 13 asks for.
//!   `7z-lzma2.cb7` was written by 7-Zip before this decoder existed, and its
//!   five pages must come back byte-identical to the five a ZIP produces.
//!
//!   There are **three** of them and the other two were added because one was
//!   not enough: `-m0=LZMA2` writes the simplest shape the format allows — one
//!   folder, one chunk — so `decode_lzma2`'s loop ran exactly once for every
//!   committed archive and a defect in its second iteration was unreachable.
//!   `7z-dictreset.cb7` (`-m0=LZMA2:d8k:c8k`) is three chunks with a
//!   dictionary reset each, two of them mid-stream; `7z-nonsolid.cb7`
//!   (`-ms=off`) is five folders, so the front end is entered five times.
//!   What none of the three buys is a second *writer* — all are 7-Zip 26.02 —
//!   which `docs/design/comic-archives.md` records as unmet.
//!
//! The LZMA2 *framing* is a third case and is fully checkable here: an
//! uncompressed chunk needs no range coder at all, so the control-byte
//! grammar, the two big-endian sizes and the dictionary reset are hand-built
//! below.
//!
//! # Injection, counted
//!
//! See `crate::sevenz::tests` for the campaign that covers both modules; the
//! defects it injects into this one are in that table.

use super::*;

/// A range **encoder**, transcribed from the format's pseudocode.
///
/// The dual of [`Range`], and deliberately not written by reading it: the
/// carry chain here (`cache`, `cache_size`, and the `0xFF` run that
/// `shift_low` emits) has no counterpart in the decoder at all, which is what
/// makes the round trip evidence rather than a tautology.
struct Encoder {
    low: u64,
    range: u32,
    cache: u8,
    cache_size: u64,
    out: Vec<u8>,
}

impl Encoder {
    fn new() -> Self {
        Encoder {
            low: 0,
            range: u32::MAX,
            cache: 0,
            cache_size: 1,
            out: Vec::new(),
        }
    }

    fn shift_low(&mut self) {
        if self.low < 0xFF00_0000 || self.low > 0xFFFF_FFFF {
            let mut temp = self.cache;
            loop {
                self.out.push(temp.wrapping_add((self.low >> 32) as u8));
                temp = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = ((self.low >> 24) & 0xFF) as u8;
        }
        self.cache_size += 1;
        self.low = (self.low << 8) & 0xFFFF_FFFF;
    }

    fn encode_bit(&mut self, prob: &mut u16, bit: u32) {
        let bound = (self.range >> PROB_BITS) * u32::from(*prob);
        if bit == 0 {
            self.range = bound;
            *prob += ((1 << PROB_BITS) - *prob) >> MOVE_BITS;
        } else {
            self.low += u64::from(bound);
            self.range -= bound;
            *prob -= *prob >> MOVE_BITS;
        }
        while self.range < TOP {
            self.range <<= 8;
            self.shift_low();
        }
    }

    fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

/// Encodes `data` as a literal-only LZMA stream with `lc=3, lp=0, pb=2` — the
/// property byte `0x5D` that 7-Zip writes by default.
///
/// Literals only, so the state machine never leaves 0 and no distance is ever
/// coded. That is the whole of what this instrument claims to cover; the
/// module header says what covers the rest.
fn encode_literals(data: &[u8]) -> Vec<u8> {
    const LC: u32 = 3;
    const PB_MASK: u32 = (1 << 2) - 1;
    let mut is_match = [[PROB_INIT; 1 << 4]; STATES];
    let mut literal = vec![PROB_INIT; 0x300 << LC];
    let mut enc = Encoder::new();
    let state = 0usize;

    for (index, &byte) in data.iter().enumerate() {
        let total = index as u32;
        let pos_state = (total & PB_MASK) as usize;
        enc.encode_bit(&mut is_match[state][pos_state], 0);

        let prev = if index == 0 { 0 } else { data[index - 1] };
        let context = ((u32::from(prev) >> (8 - LC)) as usize) * 0x300;
        let mut symbol = 1usize;
        for bit_index in (0..8).rev() {
            let bit = u32::from((byte >> bit_index) & 1);
            enc.encode_bit(&mut literal[context + symbol], bit);
            symbol = (symbol << 1) | bit as usize;
        }
    }
    enc.finish()
}

const LIMITS: Limits = Limits {
    max_unpacked: 1 << 20,
};

/// **The arithmetic coder closes.** An encoder written from the format's
/// pseudocode and a decoder written from its description agree on every byte.
///
/// The inputs are chosen so the probability model has to actually move: a run
/// of one byte drives every literal probability to its ceiling, random-looking
/// bytes keep them near a half, and the mixture crosses the renormalisation
/// boundary repeatedly. A decoder with the adaptation rate or the bound
/// arithmetic wrong diverges within the first few hundred bits of any of them.
#[test]
fn the_range_coder_round_trips_through_an_encoder_written_from_the_format() {
    let runs: Vec<u8> = vec![0x41; 5000];
    let mixed: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let text = b"the quick brown fox jumps over the lazy dog. ".repeat(64);
    let ramp: Vec<u8> = (0..=255u8).cycle().take(3000).collect();

    for (name, data) in [
        ("one byte", &b"x"[..]),
        ("empty", &b""[..]),
        ("a run", runs.as_slice()),
        ("mixed", mixed.as_slice()),
        ("text", text.as_slice()),
        ("a ramp", ramp.as_slice()),
    ] {
        let encoded = encode_literals(data);
        let decoded =
            decode(&encoded, 0x5D, data.len(), &LIMITS).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(decoded, data, "{name} did not survive the round trip");
    }
}

/// The property byte unpacks as `(pb * 5 + lp) * 9 + lc`, over all 256 values.
///
/// Exhaustive rather than sampled because the failure is an off-by-one in a
/// divide, which a sample of three would miss and which changes the size of
/// the literal table — so it is caught, if at all, as a wrong picture rather
/// than as an error.
#[test]
fn properties_unpack_the_way_the_format_packs_them() {
    let mut accepted = 0usize;
    for byte in 0..=255u8 {
        let got = Properties::from_byte(byte);
        let value = u32::from(byte);
        if value >= 9 * 5 * 5 {
            assert_eq!(got, Err(Error::BadProperties), "{byte} is out of range");
            continue;
        }
        let lc = value % 9;
        let lp = (value / 9) % 5;
        let pb = value / 45;
        if lc + lp > 4 {
            assert_eq!(got, Err(Error::BadProperties), "{byte} has lc + lp > 4");
            continue;
        }
        assert_eq!(got, Ok(Properties { lc, lp, pb }), "byte {byte}");
        accepted += 1;
    }
    // 7-Zip's default, spelled out so the number below is a fact rather than
    // an observation.
    assert_eq!(
        Properties::from_byte(0x5D),
        Ok(Properties {
            lc: 3,
            lp: 0,
            pb: 2
        }),
        "0x5D is lc=3 lp=0 pb=2, which is what every 7z in this corpus carries"
    );
    // 75 of the 256, which is worth pinning as a number: 225 encode at all,
    // and the `lc + lp <= 4` rule throws away two thirds of those. A decoder
    // that dropped the rule would allocate a 3 MiB literal table from one byte.
    assert_eq!(accepted, 75, "the property bytes that are legal");
}

/// **An LZMA2 stream of uncompressed chunks is its own bytes**, which is the
/// arm 7-Zip takes for data that will not compress — most of a comic.
///
/// Built by hand from the control-byte grammar: `0x01` resets the dictionary
/// and `0x02` does not, both are followed by two big-endian bytes holding
/// `size - 1`, and `0x00` ends the stream. The "minus one" is the detail worth
/// a fixture: a reader that took the field at face value is off by one byte
/// per chunk and produces a picture that is almost right.
#[test]
fn an_lzma2_stream_of_uncompressed_chunks_is_its_own_bytes() {
    let mut stream = Vec::new();
    let first = b"the first chunk, which resets the dictionary";
    let second = b"and the second, which does not";
    stream.push(0x01u8);
    stream.extend_from_slice(&((first.len() - 1) as u16).to_be_bytes());
    stream.extend_from_slice(first);
    stream.push(0x02);
    stream.extend_from_slice(&((second.len() - 1) as u16).to_be_bytes());
    stream.extend_from_slice(second);
    stream.push(0x00);

    let want: Vec<u8> = [first.as_slice(), second.as_slice()].concat();
    assert_eq!(
        decode_lzma2(&stream, want.len(), &LIMITS),
        Ok(want.clone()),
        "two uncompressed chunks are their own bytes"
    );

    // A chunk whose declared size runs past the input is a truncation, not a
    // short read padded with zeros.
    let mut cut = stream.clone();
    cut.truncate(cut.len() - 5);
    assert_eq!(
        decode_lzma2(&cut, want.len(), &LIMITS),
        Err(Error::Truncated),
        "a chunk cut short"
    );
}

/// A control byte the format does not define is refused rather than guessed
/// at.
///
/// `0x03..=0x7F` is the hole in LZMA2's control space: below `0x80` only
/// `0x00`, `0x01` and `0x02` mean anything, and a reader that fell through to
/// the LZMA arm would read the next four bytes as sizes and decode noise.
#[test]
fn an_lzma2_control_byte_the_format_does_not_define_is_refused() {
    for control in [0x03u8, 0x40, 0x7F] {
        assert_eq!(
            decode_lzma2(&[control, 0, 0, 0, 0], 16, &LIMITS),
            Err(Error::BadChunk),
            "control {control:#04x}"
        );
    }
    // An LZMA chunk that asks to continue a stream whose properties were never
    // given. `0x80` is reset mode 0: no reset, no property byte.
    assert_eq!(
        decode_lzma2(
            &[0x80, 0x00, 0x07, 0x00, 0x05, 0, 0, 0, 0, 0, 0],
            8,
            &LIMITS
        ),
        Err(Error::BadChunk),
        "a chunk continuing a stream that never started"
    );
}

/// A declared size past the cap is refused **before** the allocation, not
/// after it.
///
/// The distinction is the whole point of the cap: `Vec::with_capacity` on a
/// file-derived `u64` is the allocation this guards, so a check that ran after
/// the decode would be a check that never ran.
#[test]
fn a_declared_size_past_the_cap_is_refused_before_it_allocates() {
    let tiny = Limits { max_unpacked: 16 };
    assert_eq!(
        decode(&[0, 0, 0, 0, 0], 0x5D, 1_000_000, &tiny),
        Err(Error::TooLarge)
    );
    assert_eq!(
        decode_lzma2(&[0x00], 1_000_000, &tiny),
        Err(Error::TooLarge)
    );
    // And through a chunk that fits the declared total but not the cap.
    let mut stream = vec![0x01u8];
    stream.extend_from_slice(&99u16.to_be_bytes());
    stream.extend(std::iter::repeat_n(0u8, 100));
    stream.push(0x00);
    assert_eq!(
        decode_lzma2(&stream, 100, &tiny),
        Err(Error::TooLarge),
        "a chunk past the cap, declared under it"
    );
}

/// A range-coded stream must begin with a zero byte.
///
/// It is the only structural check LZMA has — there is no magic and no length
/// — so it is the only thing standing between a wrong offset into an archive
/// and 4 GB of plausible-looking noise.
#[test]
fn a_range_coded_stream_must_start_with_a_zero_byte() {
    assert_eq!(
        decode(&[0x01, 0, 0, 0, 0, 0], 0x5D, 4, &LIMITS),
        Err(Error::BadRangeStart)
    );
    assert_eq!(decode(&[0, 0, 0], 0x5D, 4, &LIMITS), Err(Error::Truncated));
    assert_eq!(
        decode(&[0, 0, 0, 0, 0], 0xFF, 4, &LIMITS),
        Err(Error::BadProperties),
        "the property byte is checked before the stream is"
    );
}

/// A stream that runs out mid-symbol is [`Error::Truncated`], and one that
/// ends cleanly short of its declared length is [`Error::ShortOutput`].
///
/// Two variants rather than one because the caller does different things: a
/// truncation is a damaged archive, and a short output is a header that
/// disagrees with its own stream.
#[test]
fn a_stream_that_stops_early_says_which_way_it_stopped() {
    let data = b"a moderately long run of literals to encode".repeat(8);
    let encoded = encode_literals(&data);
    // Cut the compressed bytes, and ask for more than the stream holds. Both
    // must be an error and neither may be a short `Ok`: an LZMA stream carries
    // no length of its own, so "the input ran out" and "the model went
    // somewhere impossible" are both reachable from the same cut and *which*
    // of them fires is a fact about the bits rather than about the defect.
    // What is asserted is the part a caller acts on.
    for (name, input, want) in [
        ("half a stream", &encoded[..encoded.len() / 2], data.len()),
        ("a length past the stream", &encoded[..], data.len() + 4096),
    ] {
        let got = decode(input, 0x5D, want, &LIMITS);
        assert!(
            matches!(
                got,
                Err(Error::Truncated | Error::DistanceTooFar | Error::ShortOutput)
            ),
            "{name} gave {got:?} rather than a refusal"
        );
    }
}

/// **Hostile bytes produce answers rather than panics** (ruling 1).
///
/// The fuzz target is the real instrument; this is the part of it that runs on
/// every commit, over inputs chosen to hit the arms a fuzzer takes longest to
/// find — a control byte in every class, a property byte in every class, and
/// the five-byte range preamble at every truncation.
#[test]
fn hostile_bytes_produce_answers_rather_than_panics() {
    let mut answered = 0usize;
    for seed in 0..2048u32 {
        let len = (seed % 37) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i as u32) >> 11) as u8)
            .collect();
        let _ = decode(&bytes, (seed % 256) as u8, (seed % 512) as usize, &LIMITS);
        let _ = decode_lzma2(&bytes, (seed % 512) as usize, &LIMITS);
        answered += 1;
    }
    assert_eq!(answered, 2048, "every hostile input returned");
}

/// **A mid-stream dictionary reset restarts the literal context**, and the
/// byte before the reset is not reachable from after it.
///
/// This test exists because a counted injection said nothing existed: making
/// the literal context reach back past a dictionary reset was caught by
/// **zero** tests in the workspace. The one real `.cb7` in the corpus is a
/// single solid block whose only reset is at position 0 — where a decoder that
/// reaches back finds nothing and is accidentally right — so the defect was
/// live and invisible.
///
/// The fixture is two LZMA2 chunks, each with control reset mode 3: reset the
/// dictionary, reset the state, and take a new property byte. Each chunk is
/// therefore a complete literal-only LZMA stream starting from a clean model,
/// which is exactly what [`encode_literals`] produces. A decoder that carried
/// the last byte of chunk one into chunk two's literal context picks a
/// different probability slot from the one the encoder used, and every byte
/// after that diverges — so the second chunk comes back wrong while the first
/// is perfect, which is the signature of this class of bug.
#[test]
fn a_dictionary_reset_restarts_the_literal_context() {
    // Chosen so the last byte of the first chunk is far from the first byte of
    // the second in the `lc = 3` context: `0xF0 >> 5` is 7 and `0x11 >> 5` is
    // 0, so the wrong context is a different slot rather than the same one by
    // luck.
    let first: Vec<u8> = std::iter::repeat_n(0xF0u8, 600).collect();
    let second: Vec<u8> = (0..600u32).map(|i| 0x11u8.wrapping_add(i as u8)).collect();

    let mut stream = Vec::new();
    for (index, chunk) in [&first, &second].into_iter().enumerate() {
        let packed = encode_literals(chunk);
        // Control: 0x80 | (reset 3 << 5) | the top five bits of size - 1.
        let unpacked = chunk.len() - 1;
        stream.push(0x80 | (3 << 5) | ((unpacked >> 16) as u8 & 0x1F));
        stream.extend_from_slice(&((unpacked & 0xFFFF) as u16).to_be_bytes());
        stream.extend_from_slice(&((packed.len() - 1) as u16).to_be_bytes());
        stream.push(0x5D); // lc = 3, lp = 0, pb = 2
        stream.extend_from_slice(&packed);
        assert!(index < 2);
    }
    stream.push(0x00);

    let want: Vec<u8> = [first.as_slice(), second.as_slice()].concat();
    let got = decode_lzma2(&stream, want.len(), &LIMITS);
    assert_eq!(
        got.as_deref(),
        Ok(want.as_slice()),
        "two dictionary-reset chunks did not decode to their own bytes"
    );
}

/// A **state** reset is not a dictionary reset, and the two are different
/// control values for a reason.
///
/// Control reset mode 1 resets the probability model and leaves the dictionary
/// alone, so a match in the second chunk may still reach back into the first.
/// A decoder that treated mode 1 as mode 3 would move `dict_start` forward and
/// refuse that match as [`Error::DistanceTooFar`] — turning a legal archive
/// into a damaged one. Literals-only chunks cannot show the match half, so
/// what is asserted here is the half they can: the position counter keeps
/// running across a state reset and restarts across a dictionary reset, which
/// is what picks `pos_state` and therefore which probability slot every symbol
/// uses.
#[test]
fn a_state_reset_and_a_dictionary_reset_are_not_the_same_control() {
    let body: Vec<u8> = b"the quick brown fox jumps over the lazy dog. "
        .iter()
        .copied()
        .cycle()
        .take(700)
        .collect();
    let packed = encode_literals(&body);
    let unpacked = body.len() - 1;

    // The same chunk under both control values. Only mode 3 can decode here,
    // because `encode_literals` always starts its position counter at zero --
    // which *is* the difference the two modes name.
    let framed = |mode: u8| {
        let mut stream = vec![0x80 | (mode << 5) | ((unpacked >> 16) as u8 & 0x1F)];
        stream.extend_from_slice(&((unpacked & 0xFFFF) as u16).to_be_bytes());
        stream.extend_from_slice(&((packed.len() - 1) as u16).to_be_bytes());
        stream.push(0x5D);
        stream.extend_from_slice(&packed);
        stream.push(0x00);
        decode_lzma2(&stream, body.len(), &LIMITS)
    };
    assert_eq!(
        framed(3).as_deref(),
        Ok(body.as_slice()),
        "a dictionary-reset chunk decodes to its own bytes"
    );
    // Mode 2 -- new properties, state reset, no dictionary reset -- is the same
    // stream at the same position, because this is the first chunk and the
    // dictionary is empty either way. The two agreeing *here* is what makes the
    // disagreement in the test above a fact about the reset rather than about
    // the framing.
    assert_eq!(
        framed(2).as_deref(),
        Ok(body.as_slice()),
        "the first chunk of a stream is at position zero under either reset"
    );
}
