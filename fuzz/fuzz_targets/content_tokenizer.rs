//! The content-stream tokenizer, and the interpreter over it.
//!
//! A page's operators come from a decompressed stream, so they are attacker
//! controlled even in a file whose structure is sound.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** The loop
//! below walks every token and adds its length to a counter that is never
//! read. Nothing checks that the tokens are the right ones, that a string
//! ended where it should, or that an operator was split from its operands
//! correctly. So a run that returned the *wrong* answer passes this target
//! exactly as a correct one does, and a green `cargo fuzz` here is evidence
//! about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-content`'s own tests and, end to end, in the
//! rendered-page comparisons — a tokenizer that silently merged two operators
//! would draw a different page and would pass here.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_content::{Token, Tokenizer};

fuzz_target!(|data: &[u8]| {
    let mut tokens = Tokenizer::new(data);
    let mut seen = 0usize;
    while let Some(token) = tokens.next_token() {
        // Touching each token, so a lazily built one is actually built.
        match &token {
            Token::String(s) => seen += s.len(),
            Token::Name(n) => seen += n.len(),
            Token::Operator(o) => seen += o.len(),
            _ => seen += 1,
        }
        if seen > 1 << 22 {
            break;
        }
    }
});
