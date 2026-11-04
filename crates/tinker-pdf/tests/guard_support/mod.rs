//! What the guard suite's binaries share (`docs/design/performance.md`,
//! "The guard suite"): the documents more than one of them reads, written
//! object by object, and the allocator that counts what an operation
//! allocates.
//!
//! `guards.rs` holds the guards over the tally a door hands back and the
//! inputs only it reads; `guard_allocations.rs` counts the one thing no tally
//! sees, the allocations an operation makes, over A2's documents.
//!
//! # Why the allow
//!
//! Each binary compiles its own copy of this module and uses a different part
//! of it: `guards.rs` installs no allocator, and `guard_allocations.rs` writes
//! no document but A2's.

#![allow(
    dead_code,
    reason = "shared by the guard suite's binaries; each uses a different part"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use crate::render_support;

// ---- Writing a document ----------------------------------------------------

/// A PDF of `objects`, numbered from 1 in order, the first its catalog, with
/// a cross-reference table so that it opens without repair.
pub fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A stream object: `dict`'s entries, its `/Length`, and `data`.
pub fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// `data` deflated, as a stream with `dict`'s entries.
pub fn deflated(dict: &str, data: &[u8]) -> Vec<u8> {
    stream(
        &format!("{dict} /Filter /FlateDecode"),
        &tinker_pdf_filters::zlib_compress(data),
    )
}

pub fn object(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// The embedded face as three objects numbered from `at`: the font, its
/// descriptor at `at + 1`, and its program at `at + 2`, deflated. The face
/// draws an outline for every printable code but the space.
pub fn face(at: usize) -> [Vec<u8>; 3] {
    [
        object(&format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Curvy /FirstChar 32 /LastChar 126\n\
             /Widths [320{}] /Encoding /WinAnsiEncoding /FontDescriptor {} 0 R >>",
            " 640".repeat(94),
            at + 1
        )),
        object(&format!(
            "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 /FontBBox [0 0 1000 1000]\n\
             /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 {} 0 R >>",
            at + 2
        )),
        deflated("", &render_support::curvy_font()),
    ]
}

/// A packet claiming PDF/A-2b, so that a validation judges what claims it.
pub const PDFA_PACKET: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="2" pdfaid:conformance="B"/>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

// ---- A2's documents ----------------------------------------------------------

/// The two sizes A2 compares: the work at 4 000 pages at most 4.4 times the
/// work at 1 000.
pub const A2_PAGES: [usize; 2] = [1_000, 4_000];

/// The line page `page` of part `part` shows.
pub fn line(part: usize, page: usize) -> String {
    format!("Part {part}, page {}", page + 1)
}

/// A document of `pages` pages, part `part` of a merge: each page one line of
/// text in the embedded face from a content stream of its own, every stream
/// deflated, under one flat page tree, and a packet claiming PDF/A-2b.
///
/// It is the least that exercises every A2 leg: text, a render, subsetting
/// and redaction have glyphs to work on; a rewrite, a rotation, a split and a
/// merge have pages; and a validation has a claim to judge. Each part draws
/// its own lines, so two parts' contents differ while their faces are the same
/// encoded bytes: a merge's deduplication meets one collision a part, the
/// face, and none among the pages.
pub fn document(part: usize, pages: usize) -> Vec<u8> {
    // 1 the catalog, 2 the page tree, 3 to 5 the face, 6 the packet, then each
    // page and its content from 7.
    let kids: Vec<String> = (0..pages).map(|at| format!("{} 0 R", 7 + 2 * at)).collect();
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R /Metadata 6 0 R >>"),
        object(&format!(
            "<< /Type /Pages /Count {pages} /Kids [{}] >>",
            kids.join(" ")
        )),
    ];
    objects.extend(face(3));
    objects.push(stream(
        "/Type /Metadata /Subtype /XML",
        PDFA_PACKET.as_bytes(),
    ));
    for at in 0..pages {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            8 + 2 * at
        )));
        objects.push(deflated(
            "",
            format!("BT /F1 12 Tf 10 50 Td ({}) Tj ET", line(part, at)).as_bytes(),
        ));
    }
    pdf(&objects)
}

// ---- Counting allocations ----------------------------------------------------

/// `System`, counting each allocation it hands out against the thread that
/// asked for it: an allocation, a zeroed allocation and a reallocation are one
/// each, and a release is none.
///
/// Counted a thread at a time, so a test reads its own thread's allocations
/// and nothing another test makes beside it: the binary that installs it can
/// hold several. An operation that handed work to other threads would count
/// only its caller's share; none of the operations counted here does.
pub struct Counting;

thread_local! {
    /// This thread's allocations so far. A `const` `Cell` of a number, so
    /// reading it neither allocates nor registers a destructor, and the
    /// allocator can count from inside an allocation.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// Counts one allocation on this thread. One made while the thread's locals
/// are being torn down counts nothing rather than failing.
fn count() {
    let _ = ALLOCATIONS.try_with(|n| n.set(n.get().saturating_add(1)));
}

// SAFETY: every method forwards its arguments unchanged to `System`, whose
// contract is the trait's; the count is a thread-local number, which neither
// allocates nor reads anything a caller handed in.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `alloc` is `System::alloc`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        // SAFETY: `at` came from this allocator, which is `System`.
        unsafe { System.dealloc(at, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract is `System::alloc_zeroed`'s.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: `at` came from this allocator with `layout`, and the
        // caller's contract for `size` is `System::realloc`'s.
        unsafe { System.realloc(at, layout, size) }
    }
}

/// The allocations this thread has made so far, in a binary that installs
/// [`Counting`] as its global allocator.
pub fn allocations() -> u64 {
    ALLOCATIONS.with(Cell::get)
}

/// What `run` allocates on this thread.
pub fn allocations_of(run: impl FnOnce()) -> u64 {
    let before = allocations();
    run();
    allocations() - before
}
