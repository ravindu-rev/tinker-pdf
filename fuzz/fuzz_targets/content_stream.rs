//! Content streams, generated: a page whose content is every operator of
//! Annex A, rendered directly and replayed from its display list.
//!
//! `content_tokenizer` and `render_page` hand mutated bytes to the reader,
//! and a mutation that breaks a file's syntax stops at the parser. This target
//! reads its input as choices instead (`tinker_pdf_fuzz::Choices`) and builds
//! a page out of them with `tinker_pdf_fuzz::content`: operators with operand
//! lists of the shape each takes or of a wrong one — too few, too many, of the
//! wrong type — `q`/`Q`, `BT`/`ET`, `BX`/`EX` and marked content balanced and
//! not, inline images of every colour space and a stencil mask, and `Do` of
//! form XObjects whose content is generated too, over resources generated with
//! the page: standard 14 fonts, a Type 3 font whose glyph procedures are
//! generated content opening with `d0` or `d1`, image XObjects in the device
//! spaces, an indexed space and as stencil masks, graphics states with line,
//! dash, alpha, blend and soft-mask entries, axial and radial shadings, tiling
//! and shading patterns, colour spaces from indexed to Lab, property lists, and
//! an annotation with an appearance and one without. The render's options —
//! scale, region, pixel format, anti-aliasing, annotations, a transparent and
//! a premultiplied page — are chosen with it. The document is written by
//! `tinker_pdf_fuzz::cos`, every `/Length` honest.
//!
//! The caps — 32 operators a stream, 192 in the document, forms and patterns
//! one level below the page and glyph procedures below them, images of eight
//! samples a side, a page of 72 points a side — are
//! `tinker_pdf_fuzz::content`'s constants. Every page's content opens with a
//! filled rectangle, so a page draws something unless its own content paints
//! it over.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **The display list replays the page.** `Page::display_list` interprets
//!   the page once and `DisplayList::render` draws what it recorded, and for
//!   the generated options it returns the bitmap `Page::render` returns: the
//!   same width, height, stride, format and alpha convention, every byte, and
//!   the same warnings. That is the display list's documented contract
//!   (`crates/tinker-pdf/src/display.rs`), and a replay that drew one call in
//!   the wrong state, or reported one refusal the direct render did not, fails
//!   here.
//! - **A generated page opens and has its page.** The file is written clean,
//!   so a page that did not open, or opened without its one page, is the
//!   writer's defect or the reader's.
//!
//! # What this target cannot find, and what covers it instead
//!
//! The relation compares two renders of one page by one engine, so a defect
//! both share — an operator drawn wrongly by the interpreter, a colour
//! converted wrongly by the renderer — draws the same wrong pixels twice and
//! passes. Whether a page draws what its content says is the analytic tier's
//! (`crates/tinker-pdf/tests/render_analytic.rs`) and the reviewed goldens',
//! and whether it draws what the world draws is the corpus ratchet's.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::{Bitmap, Document};
use tinker_pdf_fuzz::{content, Choices};

fuzz_target!(|data: &[u8]| {
    let page = content::page(&mut Choices::new(data));
    let doc = Document::open(page.bytes)
        .unwrap_or_else(|error| panic!("a generated page did not open: {error}"));
    let Some(drawn) = doc.page(0) else {
        panic!("a generated document has no page");
    };
    let direct = drawn.render(&page.options);
    let replayed = drawn.display_list().render(&page.options);
    same(&direct, &replayed);
});

/// The replayed bitmap is the direct one: its shape, every byte, and its
/// warnings.
fn same(direct: &Bitmap, replayed: &Bitmap) {
    assert_eq!(
        (
            direct.width,
            direct.height,
            direct.stride,
            direct.format,
            direct.premultiplied
        ),
        (
            replayed.width,
            replayed.height,
            replayed.stride,
            replayed.format,
            replayed.premultiplied
        ),
        "the display list drew a bitmap of another shape"
    );
    if direct.data != replayed.data {
        let at = direct
            .data
            .iter()
            .zip(&replayed.data)
            .position(|(a, b)| a != b)
            .unwrap_or(direct.data.len().min(replayed.data.len()));
        let stride = direct.stride.max(1);
        panic!(
            "the display list drew byte {at} (row {}, column byte {}) as {:?}, the page as {:?}",
            at / stride,
            at % stride,
            replayed.data.get(at),
            direct.data.get(at)
        );
    }
    assert_eq!(
        direct.warnings, replayed.warnings,
        "the display list warned otherwise"
    );
}
