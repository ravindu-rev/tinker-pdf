//! PDF file syntax: lexer, object model, cross-reference tables in every flavor, object streams, the repair scanner, and the serializers.
//!
//! Scope, design and exit criteria: `docs/plans/01-cos-and-object-model.md`.
//!
//! Two layers live here. The bottom one is pure syntax — the token grammar of
//! 7.2 and the object grammar of 7.3 — and knows nothing beyond the bytes it
//! is pointed at: [`parse_object_at`] takes a buffer and an offset and returns
//! an object plus typed warnings, never a failure. The top one is
//! [`CosDocument`], which owns a buffer, merges the cross-reference tables of
//! every revision over it, and hands out `Arc<Object>` lazily and safely from
//! any number of threads.
//!
//! Neither layer panics on untrusted input, and neither reads a file: bytes go
//! in, values come out, because wasm32-unknown-unknown has no filesystem and
//! is a first-class target.
//!
//! ```
//! use tinker_pdf_cos::{CosDocument, LadderLevel, Name};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let bytes = b"%PDF-1.7\n\
//! 1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
//! 2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
//! xref\n0 3\n\
//! 0000000000 65535 f \n\
//! 0000000009 00000 n \n\
//! 0000000058 00000 n \n\
//! trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n110\n%%EOF\n";
//!
//! let doc = CosDocument::open(&bytes[..])?;
//! assert_eq!(doc.ladder_level(), LadderLevel::Trust);
//! assert!(doc.warnings().is_empty());
//!
//! let root = doc.resolve_key(doc.trailer(), Name::ROOT);
//! let pages = doc.resolve_key(root.as_dict().expect("a catalog"), Name::PAGES);
//! assert_eq!(pages.as_dict().and_then(|d| d.get_int(Name::COUNT)), Some(0));
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod decrypt;
pub mod dest;
pub mod doc;
pub mod font;
pub mod lexer;
pub mod limits;
pub mod name;
pub mod object;
pub mod outline;
pub mod pages;
pub mod parse;
pub mod security;
pub mod text_string;
pub mod trees;
pub mod warn;
pub mod xref;

// The three-tier stream API lives on `CosDocument`, and the scanner, the slot
// store and the object-stream cache are implementation of the ladder rather
// than surface for callers.
mod objstm;
mod repair;
mod store;
mod streams;

pub use decrypt::{CryptFilterParams, Decryptor, EncryptParams, IdentityDecryptor};
pub use dest::{Action, DestKind, Destination, Resolver};
pub use doc::{CosDocument, CosError, LadderLevel, OpenError};
pub use font::{DecodedCode, Font, FontKind};
pub use lexer::{Keyword, Lexer, Token, TokenKind};
pub use name::{Name, NameTable, NAMES};
pub use object::{Dict, ObjRef, Object, PdfString, StreamObj};
pub use outline::{metadata, outline, page_labels, LabelStyle, Metadata, OutlineItem};
pub use pages::{Page, Rect};
pub use parse::{parse_indirect_at, parse_object_at, ParsedIndirect, ParsedObject};
pub use security::{AuthError, AuthLevel, Authenticated, StandardDecryptor};
pub use text_string::{decode_text_string, parse_date, Date};
pub use trees::{name_tree, number_tree};
pub use warn::{Warning, WarningKind, WarningSink};
pub use xref::{Revision, XrefEntry, XrefTable};
