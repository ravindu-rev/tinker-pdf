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

/// Ceiling on the bytes one `stream_decoded` call produces. A flate stream
/// expands by as much as 1 032 to 1, so what a decode costs is a property of
/// the file's arithmetic rather than of its length, and this cap is what keeps
/// a decompression bomb costing bounded memory.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 134 217 728 |
/// | A 200-page comic, whose largest page is a 2000 x 3000 16-bit RGBA scan | 48 000 000 |
/// | A dense 200-page fixed document, whose largest stream is a full-page 300 dpi RGBA image | 33 660 000 |
/// | A 300-page reflowable book, whose largest stream is the same plate | 33 660 000 |
/// | **This cap** | **128 MiB** |
///
/// The fixture figure is the cap itself and is allowed:
/// `an_inline_image_decodes_under_the_shared_ceiling` builds a zlib bomb of
/// `cap + 1024` and the decode stops at `cap` with
/// `filters::Warning::OutputCapHit`, so the most any fixture *produces* is
/// this number. The three yardsticks are one stream each rather than a
/// document's worth, because that is what this cap bounds — every one of the
/// three formats is synthesised into a PDF and read back through this crate,
/// and the largest single stream each of them holds is an image.
///
/// Reachable, and this is the row that needed the number written down. The
/// sentence above used to read *"a 1 KB flate stream can legally expand
/// without bound"*, which is a cap with no ceiling in front of it to be
/// compared against — gap 18a milestone 8's failure, and the thing
/// `bounds_ledger.rs`'s `every_bound_can_fire` exists to catch. DEFLATE's
/// expansion **is** bounded, at **1 032:1**: RFC 1951's longest match is 258
/// bytes, and under a degenerate dynamic Huffman tree — one literal/length
/// code and one distance code, one bit each — that match costs two bits, so
/// `258 * 8 / 2` is 1 032 bytes out per byte in. The input is a subslice of
/// the document buffer, which on the 32-bit targets this engine builds for
/// (`wasm32-unknown-unknown` and `wasm32-wasip1`) is at most `u32::MAX` bytes.
/// The narrowest target is taken deliberately: a cap that fires under the
/// tightest ceiling fires under every looser one. `4 294 967 295 * 1 032` is
/// 4 432 406 248 440 bytes, thirty-three thousand times this cap.
pub const MAX_DECODED_STREAM: usize = 128 << 20;

/// Bytes at the end of the buffer searched for `startxref` first (7.5.5).
pub const STARTXREF_SCAN: usize = 1024;

/// Bytes at the end of the buffer searched for `startxref` before giving up
/// and dropping to a full rescan. Trailing junk after `%%EOF` is routine.
pub const STARTXREF_SCAN_MAX: usize = 64 * 1024;

/// Bytes fetched for one indirect object before the window doubles.
///
/// Only a streamed document pays this. The window grows until the object
/// demonstrably ends inside it, because a window that cut an object short
/// would parse to a different value than the same bytes in one buffer, and
/// ruling 4 does not allow the two to differ.
///
/// A quarter of a chunk rather than a whole one, so that reading an object
/// near the end of the head of a small linearized file does not pull the
/// chunk after it. Doubling costs no transport -- the chunk cache holds what
/// the shorter attempt read -- so the small first window is close to free,
/// and a stream's data is fetched by its declared length rather than by
/// doubling anyway.
pub const OBJECT_WINDOW: u64 = 1024;

/// How far past a cross-reference section a streamed reader looks for the
/// `%%EOF` that ends its revision (7.5.5).
///
/// A generic file puts them within a few hundred bytes of each other. A
/// linearized one does not: its first-page section is at the front and the
/// only `%%EOF` is at the very end (Annex F part 11), so an unbounded forward
/// search would read the whole document to answer a question the head-only
/// open exists to avoid asking. Past this the revision is taken to end at the
/// document, which is where it ends -- give or take the bytes after the
/// marker, and `Revision::byte_range` on a streamed document says so.
pub const REVISION_END_SCAN: u64 = 8192;

/// Bytes fetched for the first-page cross-reference section of a linearized
/// file before the window doubles (Annex F part 3).
///
/// Smaller than a generic section's window on purpose. That table describes
/// one page, so it is short; and a linearized file can be small enough that
/// `/E` is under a single chunk, where an over-eager first window would read
/// the tail of the file to parse the front of it.
pub const HEAD_SECTION_WINDOW: u64 = 1024;

/// Bytes fetched for one cross-reference section before the window doubles.
///
/// Only a streamed document pays this: a document opened from a buffer hands
/// the walker the whole buffer for every window, so the constant is what a
/// *fetch* costs rather than what a section may be. It doubles until the
/// section parses, and doubling is free in fetched bytes because the chunk
/// cache already holds what the shorter attempt read.
pub const XREF_SECTION_WINDOW: u64 = 8192;

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

/// Longest `/AA` or `/Names /JavaScript` source surfaced, in decoded bytes
/// (12.6.4.16).
///
/// A script past this is reported as present and oversized rather than
/// truncated: a truncated script is source text that means something different
/// from what the file says, which is worse than saying there is one and it is
/// too big.
pub const MAX_SCRIPT_LEN: usize = 64 << 10;

/// Total script source one document may surface, in decoded bytes.
///
/// [`MAX_SCRIPT_LEN`] bounds one script and [`MAX_PAGES`] bounds the field
/// count, and their product is not a bound anybody would want to allocate. A
/// per-item cap is not a total cap when the item count is document-controlled,
/// which is the same lesson `MAX_TILE_WORK` exists for.
pub const MAX_SCRIPT_TOTAL: usize = 4 << 20;

/// Deepest expression or statement nesting the script parser will build.
///
/// The evaluator recurses over the tree the parser produced, so this is what
/// bounds its stack — there is no second cap in the evaluator, because a
/// second cap is a second thing to get wrong.
pub const MAX_SCRIPT_DEPTH: u32 = 32;

/// Tokens one script may hold. Bounds the parser's own allocation before any
/// evaluation begins.
pub const MAX_SCRIPT_TOKENS: usize = 16_384;

/// Evaluation steps one script may take.
///
/// A depth cap is not a work cap. `if` nests to 32 and costs nothing; a
/// `while` loop is depth 1 and costs everything, and so does a chain of
/// concatenations. Every statement executed and every expression node
/// evaluated charges one step, so a script that does not terminate cheaply
/// stops here rather than running.
pub const MAX_SCRIPT_STEPS: u32 = 20_000;

/// Evaluation steps a whole recalculation pass may take, across every field's
/// script.
///
/// [`MAX_SCRIPT_STEPS`] bounds one script; a document chooses how many
/// scripts there are, so the pass needs its own total.
pub const MAX_CALC_STEPS: u32 = 200_000;

/// Longest string one script may build. Repeated concatenation doubles, and
/// twenty thousand steps of doubling is not a length anything can hold.
pub const MAX_SCRIPT_STRING: usize = 8192;

/// Longest array literal one script may build.
pub const MAX_SCRIPT_ARRAY: usize = 1024;

/// Variables one script may declare.
pub const MAX_SCRIPT_VARS: usize = 256;

/// Fields whose calculation action one recalculation pass will run.
pub const MAX_CALC_FIELDS: usize = 4096;

/// Distinct functions one document's `/Names /JavaScript` scripts may define
/// for its field scripts to call (7.7.4).
///
/// The same number as [`MAX_SCRIPT_VARS`] and for the same reason: a name
/// table is a lookup the interpreter scans per unknown name, so a
/// document-controlled count of them is document-controlled work. Real forms
/// define a handful; the largest generated ones a corpus shows define tens.
pub const MAX_SCRIPT_FUNCTIONS: usize = 256;
