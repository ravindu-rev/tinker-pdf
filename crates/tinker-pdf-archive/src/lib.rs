//! The archive containers that are not ZIP: bytes in, entries out, with no PDF
//! vocabulary anywhere.
//!
//! Feature documentation: `docs/features/cbz.md`; the design is
//! `docs/design/comic-archives.md`.
//!
//! A CBR is a RAR, a CB7 is a 7z and a CBT is a tar, and each of the three is
//! a comic archive this engine recognised by name and refused. This crate is
//! the three readers. It is a leaf under ruling 8 — names and byte ranges out,
//! no COS types, no idea what an entry is *for* — and the deciding of what a
//! page is stays in the facade, exactly as it does for `tinker-pdf-zip`.
//!
//! # The rule this crate is organised by, written down because it is unusual
//!
//! **One crate, three container modules, three error enums, three entry types,
//! and no trait over them.**
//!
//! [`lzma`] is a fourth module and is not one of the three: it is the
//! compression [`sevenz`] carries rather than a container, it has no `Archive`
//! and no entries, and it is public because a `.7z`'s coder list names it and
//! a caller reading a refusal deserves to reach the error type that refusal
//! carries.
//!
//! What [`tar`], `sevenz` and `rar` have in common is a *negative* — they
//! are the archive containers that are not ZIP — which is a weaker binding
//! than `filters`' "decoders" and is honestly weaker. It is still real, and it
//! buys one node in the crate graph and one edge instead of three of each. The
//! part that is not negotiable is the second half: **no trait unifies them**,
//! and the reason is a measurement rather than a preference.
//!
//! `tinker_pdf_zip::Archive::read` returns a `Cow` and hands a **stored entry
//! back borrowed**, copied nowhere. `tinker-pdf-zip`'s own test suite pins
//! that, in as many words: *the moment this copies, a 3.6 GB peak comes back*
//! — the comic path places image bytes into a PDF stream verbatim, so a copy
//! per entry is a copy of the whole archive. [`tar::Archive::read`] keeps that
//! property and strengthens it: a tar entry is stored, contiguous and aligned,
//! so it hands back a plain `&[u8]` with no `Cow` at all.
//!
//! **7z cannot.** A solid block decodes many files from one LZMA stream, so
//! there is no byte range in the input that is any one file, and
//! `sevenz::Archive::read` must return owned bytes in the general case. A
//! trait over all three would have to return the *weakest* of the three
//! signatures — owned bytes — which would delete the exact property that ZIP
//! test exists to hold, in the crate that has it, to make three unrelated
//! readers share a name. So the three modules are three readers that a caller
//! matches on, and the caller is one `match` longer for it.
//!
//! The failure this avoids is visible one layer up and is worth naming, since
//! it is the same shape: `tinker_pdf::ArchiveRefusal` is a twenty-odd-variant
//! union of three formats' vocabularies of which only a third are reachable
//! from a comic archive, because three formats were given one enum to fail
//! through. Three enums here is that not repeated.
//!
//! # Untrusted bytes, and what that costs
//!
//! Every byte an archive supplies is untrusted (ruling 1). No `unwrap` on a
//! file-derived value, no unchecked indexing, checked or saturating arithmetic
//! throughout, `#![forbid(unsafe_code)]`, and an explicit budget in front of
//! every allocation — [`tar::Limits`] and its siblings
//! are each mandatory arguments rather than defaults a caller may forget.
//! Each module has a fuzz target with a committed seed corpus.
//!
//! # What adjudicates a decompressor here
//!
//! **The archive's own CRC-32.** 7z and RAR both record one per file in the
//! archive's header, so a wrong window, a wrong Huffman table or an ignored
//! solid flag fails the *format's* check rather than needing a second
//! implementation to disagree with — the format adjudicating the
//! decompression, which is first-party in the only sense ruling 13 cares
//! about. tar has no checksum over file data at all and only one over each
//! header, which is a real difference between the three and is why the tar
//! module's tests lean on the cross-container page identity instead.
//!
//! What a CRC does **not** cover is metadata: the `vint` encoding, filename
//! decoding, volume flags. A mis-decoded filename changes page order and fails
//! nothing, which is why that half is asserted by name against a committed
//! inventory rather than trusted to a checksum.

#![forbid(unsafe_code)]

pub mod lzma;
pub mod sevenz;
pub mod tar;
