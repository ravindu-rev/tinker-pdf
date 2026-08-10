//! The content-stream tokenizer, and the interpreter over it.
//!
//! A page's operators come from a decompressed stream, so they are attacker
//! controlled even in a file whose structure is sound.
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
