//! Tests for the JPEG 2000 decoder.
//!
//! Four sources of test material, because the repository holds **zero JPX
//! bytes** and ISO/IEC 15444-4's conformance codestreams are not freely
//! redistributable, so no amount of wanting them makes them the answer:
//!
//! a. [`writer`] — a test-only encoder for the container and the codestream
//!    headers, the same shape and the same justification as `mq::encoder`.
//!    Nothing in the shipped surface writes JPEG 2000.
//! b. The Annex D context tables, transcribed and asserted **one entry at a
//!    time**. Gap 17 proved why this cannot be replaced by a round-trip: a
//!    bijective relabelling of a context array is invisible to one, because
//!    every adaptive state starts identical and an encoder's slot histories
//!    and a decoder's stay in step under *any* permutation. Gap 17 transposed
//!    two of template 0's context bits and T.88 Annex H.1 still decoded to
//!    its published picture byte for byte.
//! c. `openjpeg` as a subprocess oracle, under ruling 9 — invoked, never
//!    vendored, in `tests/jpx_oracle.rs`.
//! d. Gap 23's nineteen real files, as milestone 8's acceptance number.

mod headers;
mod tier1;
mod tier2;
mod wavelet;
pub(crate) mod writer;
