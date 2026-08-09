//! Every hardening cap the COS layer enforces, in one place.
//!
//! These are hardening limits, not conformance limits: ISO 32000-1 Annex C
//! "Implementation limits" is advisory and real files exceed it routinely, so
//! each constant here is set far above what honest documents use and exists
//! only so that a hostile file cannot turn bounded input into unbounded work.
//! The fuzzer's job is to prove these are the only limits that exist.
//!
//! There is deliberately no `MAX_STRING_LEN`. Both string forms shrink or hold
//! steady under decoding — a literal string's escapes never expand (7.3.4.2)
//! and a hex string yields one byte per two digits (7.3.4.3) — so string
//! memory is already bounded by the input length, and truncating would corrupt
//! content to buy nothing.

/// Maximum array/dictionary nesting the object parser will build (7.3.6,
/// 7.3.7). Deeper containers are skipped iteratively and read as null, so the
/// parser's own recursion is bounded by this number regardless of input.
pub const MAX_NEST_DEPTH: u32 = 256;

/// Longest name kept, in decoded bytes (7.3.5). The spec recommends 127 bytes;
/// this is thirty times that, and a longer name is truncated rather than
/// dropped so the dictionary key it belongs to survives.
pub const MAX_NAME_LEN: usize = 4096;

/// Maximum elements retained in one array. Further elements are dropped with a
/// warning; the array itself still parses to its closing bracket.
pub const MAX_ARRAY_LEN: usize = 1 << 20;

/// Maximum entries retained in one dictionary, well above the largest
/// real-world dictionaries (merged-document resource dictionaries are the
/// biggest, in the hundreds). It also bounds insertion cost: keys are unique,
/// so adding an entry scans the ones already there, and only a cap keeps that
/// scan from turning a hostile dictionary into quadratic work.
pub const MAX_DICT_ENTRIES: usize = 4096;

/// Maximum warnings a [`crate::warn::WarningSink`] retains. A pathological
/// file cannot flood memory with its own diagnostics; the cap itself is
/// reported once, as [`crate::warn::WarningKind::WarningCapReached`].
pub const MAX_WARNINGS: usize = 10_000;

/// Maximum blank bytes tolerated between the `stream` keyword and its
/// end-of-line marker (7.3.8.1). Only blanks are skipped, never arbitrary
/// bytes: stream data itself frequently contains `0x0A`, and scanning past
/// real data looking for an EOL would silently truncate it.
pub const MAX_STREAM_EOL_SKIP: usize = 32;

/// Dense cross-reference slots. `/Size` is a claim, not a fact (7.5.5): the
/// merged table is dense up to this many object numbers and spills to a map
/// above it, so a hostile `/Size` costs a map entry per real entry rather than
/// gigabytes of `Vec`.
pub const MAX_XREF_SLOTS: usize = 1 << 20;

/// Maximum `/Prev` links followed from the first cross-reference section
/// (7.5.6). A cycle is caught by the visited-offset set; this bounds the
/// acyclic-but-absurd chain as well.
pub const MAX_XREF_CHAIN: u32 = 64;

/// Maximum `Ref → Ref → Ref` hops a resolve helper follows before giving up.
/// Indirect references to indirect references are legal but never deep.
pub const MAX_RESOLVE_DEPTH: u32 = 32;

/// Maximum indirect objects on the loader's in-progress stack. Nested loads
/// happen for an indirect `/Length`, an object stream's container, and the
/// `/Encrypt` dictionary; nothing legitimate stacks deeper than a handful.
pub const MAX_LOAD_DEPTH: usize = 64;

/// Ceiling on the bytes one `stream_decoded` call produces. A 1 KB flate
/// stream can legally expand without bound, so the cap is what keeps a
/// decompression bomb costing bounded memory.
pub const MAX_DECODED_STREAM: usize = 128 << 20;

/// Bytes at the end of the buffer searched for `startxref` first (7.5.5).
pub const STARTXREF_SCAN: usize = 1024;

/// Bytes at the end of the buffer searched for `startxref` before giving up
/// and dropping to a full rescan. Trailing junk after `%%EOF` is routine.
pub const STARTXREF_SCAN_MAX: usize = 64 * 1024;

/// Bytes at the start of the buffer searched for the `%PDF-` header (7.5.2).
/// A header found past byte 0 means every stored offset is short by exactly
/// that much, which is the most common corruption in the wild.
pub const MAX_HEADER_SCAN: usize = 4096;

/// Maximum object-number/offset pairs read from one object stream's prologue
/// (7.5.7). The pairs are also bounded by the decompressed stream itself.
pub const MAX_OBJSTM_ENTRIES: usize = 1 << 20;

/// Fewest per-object validation failures that can send the whole document to
/// ladder level 3. Below this, a majority of bad offsets is still cheaper to
/// patch entry by entry than to rescan.
pub const LADDER_RESCAN_MIN_FAILURES: usize = 4;

/// The most pages one document may report.
///
/// A hostile `/Kids` graph can describe far more pages than a file could hold;
/// this bounds the walk without bounding any real document, the largest of
/// which run to a few hundred thousand pages.
pub const MAX_PAGES: usize = 1 << 21;

/// The most entries one name or number tree may yield.
///
/// Bounds a hostile or cyclic tree without bounding any real one: documents
/// carry destinations and labels in the hundreds, not the millions.
pub const MAX_TREE_ENTRIES: usize = 1 << 18;
