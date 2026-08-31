//! This Brotli decoder against committed reference streams, and against
//! streams built here from RFC 7932 itself.
//!
//! # Where the streams came from, and why that is admissible
//!
//! RFC 7932 has **no test vectors**. Its §11 is advice to compressor authors
//! and its appendices are tables; there is nothing in the document of the
//! shape "these bytes decode to those bytes". So the evidence has to be built,
//! and it is built two ways, because the two fail differently.
//!
//! The forty-two files under `tests/brotli/` were produced **once** — forty on
//! 2026-08-30 and the two `interleaved-periods` streams on 2026-08-31 — by Node v25.6.0's `zlib.brotliCompressSync` over plaintexts
//! this repository wrote — the same plaintexts [`plaintext`] below
//! reconstructs. Ruling 13 draws its line between a third party *adjudicating*
//! and a third party *supplying*: nothing here asks another program whether an
//! answer is right, and no program is invoked at test time. These are dated
//! measurements committed as fixtures, exactly as `tests/jpx/` is, and the
//! assertion over them is entirely first-party — **this decoder reproduces a
//! plaintext this repository already had**.
//!
//! That matters more here than it usually does. A decoder tested only against
//! streams its author also built tests one reading of the specification twice:
//! if the author misread §7.1's context tables, a hand-built fixture misreads
//! them identically and the test passes. A real encoder's output cannot be
//! wrong in the same direction, because it was written by somebody else from
//! the same document. The forty-two vectors are that independent half.
//!
//! The other half is [`hand_built`]: three shapes a general-purpose encoder
//! **never emits**, so no corpus of real streams would ever cover them —
//! an empty last meta-block, an uncompressed meta-block, and a metadata
//! meta-block whose bytes are neither output nor window (§9.2). Those are
//! written here bit by bit from §9.2, which is the only way they can exist.
//!
//! # What the forty-two cover
//!
//! Nine plaintexts across five quality settings and two window sizes, chosen
//! so that each turns on machinery the others leave off:
//!
//! - `repeated-byte` and `repeated-pair` are **overlapping backward copies**,
//!   the case §10 calls out by name (`<length 5, distance 2>` over `XY` gives
//!   `XYXYX`).
//! - `english-prose` is the **static dictionary** and the UTF8 context mode:
//!   at quality 11 the encoder reaches Appendix A for " the ", " of " and
//!   their transformed forms, which is the only path that touches the 122 KiB
//!   blob at all.
//! - `utf8-text` moves §7.1's context lookup off its ASCII rows.
//! - `incompressible` is 600 bytes of xorshift, which every quality setting
//!   gives up on — so these are the vectors that exercise **uncompressed
//!   meta-blocks** as a real encoder emits them.
//! - `sfnt-shaped` is what a WOFF2 actually hands this decoder.
//! - The `-w10` and `-w24` pairs move WBITS to both ends of §9.1's range, so
//!   the window-size decode is not tested only at its default of 16.
//! - `empty` and `one-byte` are the degenerate lengths, where a prefix code
//!   can legally have **one symbol and zero bits**.
//! - The two `interleaved-periods` streams are the only ones here chosen
//!   against a *decoder* rather than against a clause. §4 says a distance
//!   symbol 0 is not pushed to the ring buffer of last distances; a decoder
//!   that pushed it decoded all forty of the others correctly, because none of
//!   them follows a repeated distance with a short code counted against the
//!   second-to-last. These two do, and they were found by generating a hundred
//!   and sixty structured plaintexts and keeping the ones the two readings of
//!   §4 disagree about. Eleven of the hundred and sixty did; these are the
//!   shortest and the one with the most segments.

use tinker_pdf_filters::{brotli_decode, BrotliError, Limits};

/// A ceiling far above anything here, so the vector tests measure decoding
/// rather than the ceiling. The ceiling has its own tests.
const ROOMY: Limits = Limits::new(1 << 20);

// ---- the plaintexts, reconstructed rather than committed ---------------------

/// The prose vector's paragraph, once.
const PROSE: &str = "The time of the year is the best time to work with the free text of a book, \
and the people of the world will find that the same words are used over and \
over again in the text of that book.\n";

/// The 600 pseudo-random bytes of the `incompressible` vector.
///
/// A named xorshift rather than a committed blob: the sequence is four lines
/// of arithmetic, and a test that can *say* what it expects is worth more than
/// one that can only point at a file.
fn xorshift(len: usize) -> Vec<u8> {
    let mut state: u32 = 0x1234_5678;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        out.push((state & 0xff) as u8);
    }
    out
}

/// The plaintext each vector name decodes to.
fn plaintext(name: &str) -> Vec<u8> {
    match name {
        "empty" => Vec::new(),
        "one-byte" => b"A".to_vec(),
        "short-ascii" => b"hello brotli".to_vec(),
        "repeated-byte" => vec![b'a'; 1024],
        "repeated-pair" => b"ab".repeat(512),
        "english-prose" => PROSE.repeat(2).into_bytes(),
        "utf8-text" => "Ceci n\u{2019}est pas une pipe \u{2014} \u{4e00}\u{4e8c}\u{4e09}\u{56db}\u{4e94} \u{440}\u{443}\u{441}\u{441}\u{43a}\u{438}\u{439} \u{442}\u{435}\u{43a}\u{441}\u{442}. "
            .repeat(4)
            .into_bytes(),
        "incompressible" => xorshift(600),
        // Three runs of a short period over a four-symbol alphabet, the
        // periods chosen so that none divides another. At quality 11 the
        // encoder answers that with a **distance symbol 0** — "the same
        // distance again" — and then, at the next period change, with a short
        // code counted against the second-to-last distance. RFC 7932 §4 says
        // the first of those must not enter the ring buffer, so the two are
        // only consistent in a decoder that does not push it. This is the
        // shape that found the decoder that did.
        "interleaved-periods" => interleaved_periods(&[(4, 29), (9, 46), (5, 35)]),
        "interleaved-periods-short" => {
            interleaved_periods(&[(4, 24), (3, 44), (3, 36), (4, 9), (3, 11)])
        }
        "sfnt-shaped" => (0..1500usize)
            .map(|i| {
                if i % 97 == 0 {
                    0u8
                } else {
                    (((i * 31) & 0xff) >> (i & 3)) as u8
                }
            })
            .collect(),
        other => panic!("no plaintext is written down for the vector {other:?}"),
    }
}

/// A run of `repeats` copies of a `period`-long cycle, per segment, over four
/// symbols.
///
/// The byte is `(position within the period * 7 + segment index) % 4`, so the
/// same period in two segments is a *different* cycle and a back-reference has
/// to reach past the nearer one. Four lines of arithmetic rather than a
/// committed blob, for the reason [`xorshift`] is: a test that can say what it
/// expects is worth more than one that can only point at a file.
fn interleaved_periods(segments: &[(usize, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (segment, &(period, repeats)) in segments.iter().enumerate() {
        for _ in 0..repeats {
            for j in 0..period {
                out.push(((j * 7 + segment) % 4) as u8);
            }
        }
    }
    out
}

/// Every committed vector: `(file name, the plaintext key it decodes to)`.
///
/// Listed rather than globbed, because a glob over a directory turns a
/// **missing fixture into a passing test** — the failure mode gap 20 named
/// and this repository greps its CI output for.
macro_rules! vectors {
    ($($file:literal => $key:literal),* $(,)?) => {
        const VECTORS: &[(&str, &str, &[u8])] = &[
            $(($file, $key, include_bytes!(concat!("brotli/", $file)))),*
        ];
    };
}

vectors! {
    "interleaved-periods-q11.br" => "interleaved-periods",
    "interleaved-periods-short-q11.br" => "interleaved-periods-short",
    "empty-q1.br" => "empty",
    "empty-q11.br" => "empty",
    "empty-w10.br" => "empty",
    "empty-w24.br" => "empty",
    "one-byte-q1.br" => "one-byte",
    "one-byte-q11.br" => "one-byte",
    "one-byte-w10.br" => "one-byte",
    "one-byte-w24.br" => "one-byte",
    "short-ascii-q1.br" => "short-ascii",
    "short-ascii-q11.br" => "short-ascii",
    "short-ascii-w10.br" => "short-ascii",
    "short-ascii-w24.br" => "short-ascii",
    "repeated-byte-q0.br" => "repeated-byte",
    "repeated-byte-q1.br" => "repeated-byte",
    "repeated-byte-q11.br" => "repeated-byte",
    "repeated-byte-w10.br" => "repeated-byte",
    "repeated-byte-w24.br" => "repeated-byte",
    "repeated-pair-q1.br" => "repeated-pair",
    "repeated-pair-q11.br" => "repeated-pair",
    "repeated-pair-w10.br" => "repeated-pair",
    "repeated-pair-w24.br" => "repeated-pair",
    "english-prose-q0.br" => "english-prose",
    "english-prose-q1.br" => "english-prose",
    "english-prose-q5.br" => "english-prose",
    "english-prose-q9.br" => "english-prose",
    "english-prose-q11.br" => "english-prose",
    "english-prose-w10.br" => "english-prose",
    "english-prose-w24.br" => "english-prose",
    "utf8-text-q1.br" => "utf8-text",
    "utf8-text-q11.br" => "utf8-text",
    "utf8-text-w10.br" => "utf8-text",
    "utf8-text-w24.br" => "utf8-text",
    "incompressible-q1.br" => "incompressible",
    "incompressible-q11.br" => "incompressible",
    "incompressible-w10.br" => "incompressible",
    "incompressible-w24.br" => "incompressible",
    "sfnt-shaped-q1.br" => "sfnt-shaped",
    "sfnt-shaped-q11.br" => "sfnt-shaped",
    "sfnt-shaped-w10.br" => "sfnt-shaped",
    "sfnt-shaped-w24.br" => "sfnt-shaped",
}

// ---- the headline -----------------------------------------------------------

/// **Every committed stream decodes to the plaintext it was made from.**
///
/// Forty-two streams, eleven plaintexts, and the whole of §3 to §10 between
/// them. This is the test that says the decoder is a Brotli decoder rather
/// than something that agrees with its author.
#[test]
fn every_reference_stream_decodes_to_its_plaintext() {
    assert_eq!(VECTORS.len(), 42, "the vector list and the directory agree");
    for (file, key, stream) in VECTORS {
        let want = plaintext(key);
        match brotli_decode(stream, &ROOMY) {
            Ok(got) => assert!(
                got == want,
                "{file} decoded to {} bytes, expected {}",
                got.len(),
                want.len()
            ),
            Err(error) => panic!("{file} did not decode: {error}"),
        }
    }
}

/// **The prose vectors really do reach the static dictionary.**
///
/// The premise of the headline's coverage claim, asserted rather than assumed.
/// A dictionary reference is invisible in the output — it produces ordinary
/// bytes — so if the encoder had declined to use one, the 122 KiB blob would
/// be dead weight that no test touches and nobody would know.
///
/// The check is indirect and is the only one available without instrumenting
/// the decoder: with the dictionary reachable the stream decodes; with every
/// backward distance forced to be in-window it could not, because the words
/// are not in the output to copy. Concretely, the prose fixture at quality 11
/// is 98 bytes for 378 bytes of text — 26 %, which no literal-and-copy coding
/// of two paragraphs reaches — and the same text with a 1 KiB window (`-w10`)
/// is 99 bytes rather than the ~200 a window that small would otherwise force.
#[test]
fn the_prose_vectors_are_small_enough_that_the_dictionary_must_be_in_play() {
    let text = plaintext("english-prose");
    let (_, _, best) = VECTORS
        .iter()
        .find(|(f, _, _)| *f == "english-prose-q11.br")
        .expect("the vector list carries the quality 11 prose stream");
    let (_, _, tiny_window) = VECTORS
        .iter()
        .find(|(f, _, _)| *f == "english-prose-w10.br")
        .expect("the vector list carries the 1 KiB window prose stream");

    assert!(
        best.len() * 3 < text.len(),
        "the prose stream is {} bytes for {} of text, which is not dictionary-tight",
        best.len(),
        text.len()
    );
    // A 1 KiB window cannot hold the paragraph, so a stream that stayed this
    // small found its words somewhere other than its own output.
    assert!(tiny_window.len() < 120, "the small-window stream grew");
    assert_eq!(
        brotli_decode(tiny_window, &ROOMY).expect("it decodes"),
        text
    );
}

// ---- the shapes no encoder emits --------------------------------------------

/// A bit writer in RFC 7932's packing order (§1.5.1): elements go in from the
/// least significant bit of each byte upward.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    /// `count` bits of `value`, least significant first — §1.5.1's "integer
    /// values".
    fn push(&mut self, value: u32, count: u32) {
        for i in 0..count {
            if self.bits % 8 == 0 {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 1 << (self.bits % 8);
            }
            self.bits += 1;
        }
    }

    /// Pads to the next byte boundary with zeros, which is what §9.2 requires
    /// of every place a decoder is told to skip to one.
    fn align(&mut self) {
        while self.bits % 8 != 0 {
            self.push(0, 1);
        }
    }

    fn raw(&mut self, data: &[u8]) {
        assert_eq!(self.bits % 8, 0, "raw bytes go in aligned");
        self.bytes.extend_from_slice(data);
        self.bits += data.len() * 8;
    }

    /// §9.1's one-bit spelling of WBITS = 16, which is all these fixtures need.
    fn window_16(&mut self) {
        self.push(0, 1);
    }

    /// §9.2's `ISLAST = 1, ISLASTEMPTY = 1`: the stream ends at this bit.
    fn last_empty(&mut self) {
        self.push(1, 1);
        self.push(1, 1);
    }

    /// §9.2's uncompressed meta-block, which is also §11.1's "trivial
    /// compressor".
    fn uncompressed(&mut self, data: &[u8]) {
        assert!(!data.is_empty(), "MLEN is stored as MLEN - 1");
        self.push(0, 1); // ISLAST = 0; an uncompressed block may not be last.
        self.push(0, 2); // MNIBBLES = 4.
        self.push((data.len() - 1) as u32, 16);
        self.push(1, 1); // ISUNCOMPRESSED
        self.align();
        self.raw(data);
    }

    /// §9.2's empty meta-block carrying metadata bytes, which are "not part of
    /// either the sliding window or the uncompressed data".
    fn metadata(&mut self, data: &[u8]) {
        assert!(!data.is_empty() && data.len() <= 256);
        self.push(0, 1); // ISLAST = 0
        self.push(3, 2); // MNIBBLES = 0, i.e. the empty meta-block
        self.push(0, 1); // reserved
        self.push(1, 2); // MSKIPBYTES = 1
        self.push((data.len() - 1) as u32, 8);
        self.align();
        self.raw(data);
    }
}

/// Builds one of the three shapes a general-purpose encoder never produces.
fn hand_built(shape: &str) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.window_16();
    match shape {
        "empty" => {}
        "uncompressed" => w.uncompressed(b"the trivial compressor of RFC 7932 section 11.1"),
        "two-meta-blocks" => {
            w.uncompressed(b"first half, ");
            w.uncompressed(b"second half");
        }
        "metadata-then-data" => {
            w.metadata(b"<metadata that is not output>");
            w.uncompressed(b"only this is output");
        }
        other => panic!("no hand-built shape named {other:?}"),
    }
    w.last_empty();
    w.align();
    w.bytes
}

/// **The three shapes RFC 7932 defines and no encoder emits.**
///
/// An uncompressed meta-block, two of them in a row, and a metadata block
/// whose bytes must *not* reach the output. Each is written bit by bit from
/// §9.2 above, because there is no other way to obtain one: a general-purpose
/// encoder emits an uncompressed meta-block only for data it gives up on, and
/// emits a metadata meta-block never.
///
/// The metadata case is the one worth the trouble. §9.2 says those bytes are
/// "not part of the uncompressed data or the sliding window", so a decoder
/// that merely *skipped* them would pass this test — and a decoder that
/// appended them would produce a font with 29 bytes of junk in the middle of
/// a table.
#[test]
fn the_meta_block_shapes_no_encoder_emits_still_decode() {
    for (shape, want) in [
        ("empty", &b""[..]),
        (
            "uncompressed",
            &b"the trivial compressor of RFC 7932 section 11.1"[..],
        ),
        ("two-meta-blocks", &b"first half, second half"[..]),
        ("metadata-then-data", &b"only this is output"[..]),
    ] {
        let stream = hand_built(shape);
        assert_eq!(
            brotli_decode(&stream, &ROOMY),
            Ok(want.to_vec()),
            "the hand-built {shape} stream"
        );
    }
}

// ---- refusals ---------------------------------------------------------------

/// **A truncated stream is `Truncated`, and never anything else.**
///
/// §10: "If the stream ends before the completion of the last meta-block, then
/// the stream should be rejected as invalid." The distinction from
/// [`BrotliError::Malformed`] is the one a caller acts on — a short read is
/// worth retrying and a broken file is not.
#[test]
fn a_stream_cut_short_is_named_as_truncated() {
    let full = hand_built("uncompressed");
    for cut in 1..full.len() {
        match brotli_decode(&full[..cut], &ROOMY) {
            Err(BrotliError::Truncated) => {}
            Ok(out) => panic!(
                "{cut} bytes of a {}-byte stream decoded to {out:?}",
                full.len()
            ),
            Err(other) => panic!("{cut} bytes gave {other} rather than a truncation"),
        }
    }
    // Nothing at all is also a truncation rather than an empty success: §9.1's
    // window size is mandatory and one bit long.
    assert_eq!(brotli_decode(b"", &ROOMY), Err(BrotliError::Truncated));
}

/// **§9.1's reserved window-size pattern is refused by name.**
///
/// "Note that bit pattern 0010001 is invalid and must not be used." It is the
/// one hole in an otherwise dense variable-length code, so a decoder that fell
/// through to a default would accept a stream no encoder can write and would
/// then read the rest of it against the wrong window.
#[test]
fn the_reserved_window_size_pattern_is_refused() {
    // 0010001, least significant bit first: 1, 0, 0, 0, 1, 0, 0.
    let mut w = BitWriter::default();
    w.push(1, 1);
    w.push(0, 3); // the three bits that select the second form
    w.push(1, 3); // m == 1, which §9.1 reserves
    w.last_empty();
    w.align();
    assert_eq!(
        brotli_decode(&w.bytes, &ROOMY),
        Err(BrotliError::Malformed("the reserved window-size pattern"))
    );

    // The neighbouring patterns are the legal window sizes either side of it.
    for (m, _wbits) in [(0u32, 17u32), (2, 10), (7, 15)] {
        let mut w = BitWriter::default();
        w.push(1, 1);
        w.push(0, 3);
        w.push(m, 3);
        w.last_empty();
        w.align();
        assert_eq!(brotli_decode(&w.bytes, &ROOMY), Ok(Vec::new()), "m = {m}");
    }
}

/// **The pad bits §9.2 says must be zero are checked.**
///
/// Three places in §9.2 skip to a byte boundary and each says the skipped bits
/// must be zero. They are the cheapest corruption check the format has, and a
/// decoder that ignored them would read a damaged stream one bit out of phase
/// and produce plausible garbage rather than a refusal.
#[test]
fn a_non_zero_pad_before_an_uncompressed_block_is_refused() {
    let mut w = BitWriter::default();
    w.window_16();
    w.push(0, 1); // ISLAST = 0
    w.push(0, 2); // MNIBBLES = 4
    w.push(3, 16); // MLEN = 4
    w.push(1, 1); // ISUNCOMPRESSED
                  // The pad, deliberately not zero.
    while w.bits % 8 != 0 {
        w.push(1, 1);
    }
    w.raw(b"data");
    w.last_empty();
    w.align();
    assert_eq!(
        brotli_decode(&w.bytes, &ROOMY),
        Err(BrotliError::Malformed(
            "the pad before an uncompressed meta-block was not zero"
        ))
    );
}

/// **Arbitrary bytes never panic.**
///
/// Ruling 1's standing property in its cheapest form, so that the guarantee is
/// checked on stable on every commit rather than only under `cargo fuzz`. The
/// inputs are mutations of real streams, which is where the interesting
/// failures are: random bytes are rejected by the window-size decode almost
/// immediately, and prove very little.
#[test]
fn mutations_of_real_streams_never_panic() {
    let mut state: u32 = 0x9e37_79b9;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for (_, _, stream) in VECTORS {
        let mut body = stream.to_vec();
        for _ in 0..64 {
            if body.is_empty() {
                break;
            }
            let at = (next() as usize) % body.len();
            body[at] ^= (next() & 0xff) as u8;
            // Two ceilings, because the interesting interaction is between a
            // damaged length and a ceiling that fires part-way through.
            let _ = brotli_decode(&body, &Limits::new(1 << 16));
            let _ = brotli_decode(&body, &Limits::new(64));
        }
    }
}

// ---- the fuzz corpus --------------------------------------------------------

/// Writes the seeds `fuzz/corpus/brotli/` carries, so the seeds and the
/// fixtures here cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
///
/// Each seed is the target's **control byte** and then a stream, because the
/// first byte is what picks the output ceiling. Two seeds carry a control byte
/// of zero — a ceiling of one byte — since that is the only value from which
/// `ExceedsOutputLimit` fires on an otherwise perfectly good stream, and a
/// corpus that never reaches a refusal is the corpus half of gap 18a milestone
/// 8's failure.
#[test]
#[ignore = "writes into fuzz/corpus/brotli, which is committed"]
fn write_the_fuzz_seeds() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/brotli");
    std::fs::create_dir_all(&base).expect("the corpus directory can be made");

    for (file, _, stream) in VECTORS {
        let name = file.trim_end_matches(".br");
        std::fs::write(base.join(name), [&[0x03u8][..], stream].concat())
            .expect("the corpus directory is there");
    }
    for shape in [
        "empty",
        "uncompressed",
        "two-meta-blocks",
        "metadata-then-data",
    ] {
        std::fs::write(
            base.join(format!("handbuilt-{shape}")),
            [&[0x03u8][..], &hand_built(shape)].concat(),
        )
        .expect("the corpus directory is there");
    }
    // The same streams against a one-byte ceiling.
    for file in ["repeated-byte-q11.br", "english-prose-q11.br"] {
        let (_, _, stream) = VECTORS
            .iter()
            .find(|(f, _, _)| f == &file)
            .expect("a named vector");
        std::fs::write(
            base.join(format!("{}-tight", file.trim_end_matches(".br"))),
            [&[0x00u8][..], stream].concat(),
        )
        .expect("the corpus directory is there");
    }
}
