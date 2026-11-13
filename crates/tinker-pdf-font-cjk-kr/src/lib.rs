//! Source Han Sans KR Regular in WOFF2: one of the CJK fallback faces
//! `tinker-pdf-font`'s `cjk-fonts` feature carries, the Korean region's face,
//! for Adobe-Korea1 and Adobe-KR (`docs/design/fallback-faces.md`).
//!
//! Feature documentation: `docs/features/fonts.md`.
//!
//! The face is release `2.005R` of Adobe's Source Han Sans,
//! `SubsetOTF/KR/SourceHanSansKR-Regular.otf`, made WOFF2 by the reference
//! encoder, `woff2_compress`, whose version `THIRDPARTY.md` records. Its font
//! data are the published face's apart from the compression, which
//! `crates/tinker-pdf/tests/cjk_faces.rs` holds table by table through the
//! engine's own WOFF2 reader. It is licensed under the SIL Open Font License
//! 1.1, with Reserved Font Name "Source", whose text is
//! `data/source-han-sans/LICENSE.txt`.
//!
//! Nothing here reads the bytes: `tinker-pdf-font` decodes them once a
//! process, on its first request for the region's face.

#![no_std]
#![forbid(unsafe_code)]

/// The face, as its WOFF2 file holds it.
///
/// A `static` rather than a `const`, so the bytes are carried once whatever
/// a caller's crate inlines (`tinker-pdf-font`'s `bundled.rs` says why).
pub static WOFF2: &[u8] = include_bytes!("../data/source-han-sans/SourceHanSansKR-Regular.woff2");
