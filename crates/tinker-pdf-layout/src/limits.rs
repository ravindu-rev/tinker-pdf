//! Every bound this crate enforces, in one place, with the numbers that set it.
//!
//! The form is `tinker-pdf-css`'s `limits.rs`, which is `tinker-pdf-zip`'s, and
//! it is that shape for the two scars those files record. `5adf502` found an
//! 1 851-byte page that took 19.3 seconds to render with `MAX_GROUP_DEPTH` in
//! place the whole time — *depth is not work once the recursion branches*. Gap
//! 18a's milestone 8 found the opposite failure in a constant written to avoid
//! the first: `MAX_JPX_WORK` sat **above** the most its own inputs could ask
//! for, so it could never fire.
//!
//! Each carries three numbers — the most any fixture in this repository spends,
//! the most a plausible real book spends, and the constant — and each is proved
//! to fire in a test **by its own refusal, never by a clock**. The yardstick
//! for the second number is gap 31's own: **a 400-page novel of 120 000 words
//! in 40 spine items, with four stylesheets totalling 40 KB and two embedded
//! faces**.
//!
//! # Two are work caps and two are not
//!
//! [`MAX_BOX_TREE_NODES`] and [`MAX_LINE_BREAK_WORK`] are spent across a
//! **whole book** and never refunded, because a book chooses how many content
//! documents it has: a per-document cap times a file-chosen document count
//! bounds nothing. They live in [`crate::Budget`], which is one object
//! threaded through every spine item rather than a convention.
//!
//! [`MAX_LAYOUT_WORK`] is the **third**, and it arrived at milestone 10 exactly
//! where milestone 7 said it would: *"the bound arrives with the multi-pass
//! layout or not at all"*. Floats are that layout. It is spent across a whole
//! book for the same reason as the other two.
//!
//! [`MAX_BOX_DEPTH`] is per-item and bounds a recursion rather than a total.
//! [`MAX_LAYOUT_PAGES`] is per-book and bounds an output.
//!
//! # `MAX_BOX_DEPTH` exists where gap 31's plan said a depth cap would not
//!
//! That plan's *"four deliberately absent"* list says there is **nothing on DOM
//! depth**, because `tinker_pdf_xml::limits::MAX_XML_DEPTH` is 256 and stands
//! in front of every content document, so a second constant could never fire.
//! That argument is right about the facade and wrong here, and the difference
//! is what this crate is: **its input is a caller-built tree, not a parsed
//! document.** Nothing stands in front of [`crate::BoxNode`] — the fuzz target
//! builds one directly from a structured generator, and layout recurses over
//! it. A cap that could never fire in the facade fires on the twenty-fourth
//! target's first deep input.
//!
//! # `MAX_LAYOUT_PAGES` is gap 31's `MAX_EPUB_PAGES`, in the crate that
//! actually fragments
//!
//! The plan's bounds table names `MAX_EPUB_PAGES` and milestone 4 amended the
//! row in place: *"it arrives with milestone 7's fragmentation and not
//! before"*, because a build that puts one page on each `<itemref>` has a page
//! count the spine cap already bounds. This is that milestone, and the constant
//! is **here** rather than in `epub.rs` for `MAX_DOM_NODES`'s reason one
//! milestone earlier: the cap has to be where the thing it bounds is decided,
//! and pages are decided by [`crate::fragment`]. The facade's ledger row
//! carries the argument.

/// How deep a box tree may nest.
///
/// | | Levels |
/// | --- | --- |
/// | The most any fixture in this repository spends | 257 (the tree built *past* this cap; the deepest real content document in the committed corpus is 6) |
/// | A 400-page novel | 8 |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **256** |
///
/// A per-item cap on a **recursion**, not a budget, which is
/// `MAX_XML_DEPTH`'s number taken for `MAX_XML_DEPTH`'s reason: past a few
/// hundred levels the alternative is a stack overflow, which is a crash rather
/// than a refusal.
///
/// The ceiling in front of it is unbounded, and that is the whole argument for
/// its existence — see the module header.
pub const MAX_BOX_DEPTH: usize = 256;

/// Boxes generated across a whole book.
///
/// | | Boxes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 262 145 (the tree built *past* this cap; the largest real content document in the committed corpus generates 138) |
/// | A 400-page novel | ~36 000, at 12 000 elements and about three boxes each |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **262 144** |
///
/// **A work cap.** Boxes are not elements and the difference is the reason
/// this is not `MAX_DOM_NODES` under another name: anonymous block generation
/// (CSS 2.2 §9.2.1.1), `::before`/`::after` and table-structure fixup (§17.2.1)
/// each create boxes the document did not write, and a book chooses how many
/// documents it has.
pub const MAX_BOX_TREE_NODES: usize = 262_144;

// **`MAX_LAYOUT_WORK` is deliberately absent, and this is its argument.**
//
// Gap 31's bounds table has a row for it -- *"box-layout operations across the
// book"* -- and the reason it gives is right about the build it describes: *"a
// per-box cap is not a total once the file chooses the box count **and** the
// pass count: automatic table layout is two passes (§17.5.2.2), float
// placement re-flows a line, shrink-to-fit measures twice, and a nested table
// multiplies all three."* Every one of those is a milestone this is not.
//
// In **this** build there is no second pass. Every unit of layout work is one
// box or one line box, and both are already bounded: boxes by
// [`MAX_BOX_TREE_NODES`], and line boxes by [`MAX_LINE_BREAK_WORK`], because a
// line box needs at least one character and every character is charged before
// the breaker is entered. So a work cap here would sit either **above** what
// its own inputs can ask for -- gap 18a milestone 8's failure, a constant that
// can never fire -- or below the box cap, where it would be the box cap
// wearing another name. It was written, its firing test was attempted, and it
// could not be made to fire without lowering itself; that is the finding.
//
// The absence is not free and it cost three fixes to earn, because *"depth is
// not work once the recursion branches"* has a loop-shaped twin. Three places
// in [`crate::flow`] were quadratic and each is now linear, and each is
// commented where it was fixed: the line filler restarted its scan of the
// break opportunities at zero for every line, `piece_at` scanned the span list
// once per boundary, and the list-item ordinal counted from the first child
// for every item. A work cap would have *charged* for all three rather than
// removing them.
//
// **The bound arrives with the multi-pass layout or not at all**, which is
// milestones 10 and 11 -- the same sentence `bounds_ledger.rs` already carries
// for `MAX_XPS_VISUAL_DEPTH`'s absence and for `MAX_EPUB_PAGES`'s at milestone
// 4, and the second time this plan has had one of its own rows amended in
// place by the milestone that tried to build it.

/// Break opportunities evaluated across a whole book.
///
/// | | Opportunities |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 000 001 (the paragraph built *past* this cap, at the real constant rather than a lowered copy of it; the longest real content document in the committed corpus spends about 3 000) |
/// | A 400-page novel | ~700 000, at 120 000 words of about six characters |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **4 000 000** |
///
/// **A work cap**, and the one that stands in front of every line box this
/// build makes. UAX #14 evaluates a boundary between every pair of characters,
/// so this is charged per character
/// rather than per word -- and a book's character count is bounded only by
/// `MAX_ZIP_INFLATED`, a gigabyte, which is two hundred and fifty times this
/// cap.
///
/// It is charged **before** the breaker is entered rather than as the pair table
/// walks, which is `tinker-pdf-zip`'s posture -- a permit is what has been
/// promised, not what happened to arrive -- and here it is also what makes the
/// cap cheap to fire: a paragraph past it is refused before the class table
/// allocates a unit per character, so the firing fixture costs four megabytes of
/// text rather than a hundred and fifty of vectors.
pub const MAX_LINE_BREAK_WORK: usize = 4_000_000;

/// Pages fragmented out of one book.
///
/// | | Pages |
/// | --- | --- |
/// | The most any fixture in this repository spends | 65 537 (the flow built *past* this cap; the largest real book here paginates to about 400) |
/// | A 400-page novel | 400 |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **65 536** |
///
/// **Not bounded by the spine item count**, and gap 31's bounds table says so
/// in as many words: *"one spine item of 128 MiB of text fragments into as many
/// pages as its length divided by the page height"*. At a page of 648 points
/// and a line of 14, a page holds about forty lines, so 128 MiB of text at six
/// bytes a word and ten words a line is roughly fifty thousand pages **from one
/// entry** — which is why the cap is where it is and why it is not a multiple
/// of anything to do with the spine.
pub const MAX_LAYOUT_PAGES: usize = 65_536;

/// Float examinations across a whole book.
///
/// | | Examinations |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 000 001 (the book built *past* this cap; the largest real book here spends under a thousand) |
/// | A 400-page novel | ~24 000 (one figure per spine item, 12 000 line boxes, each asking the figures beside it for its measure) |
/// | A 400-page novel with a figure on every fourth page | ~240 000 |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **4 000 000** |
///
/// **This is the cap milestone 7 argued out of existence, arriving where that
/// argument said it would.** Its words were: *"in a build with no float
/// re-flow, no two-pass table layout and no shrink-to-fit, every unit of layout
/// work is one box or one line box"*, and it ended *"the bound arrives with the
/// multi-pass layout or not at all"*. Milestone 10 is floats, and floats are
/// the multi-pass layout: CSS 2.2 §9.5.1 places a float against **every float
/// already placed**, and §9.5's line boxes ask all of them for their measure.
///
/// The quadratic behind it is `5adf502`'s finding — *depth is not work once the
/// recursion branches* — in its loop-shaped form. [`MAX_BOX_TREE_NODES`] lets a
/// book float 262 144 boxes; placing the last of them examines the other
/// 262 143, and the total is 6.9e10 examinations with every other cap
/// satisfied. Neither of the two work caps bounds it: boxes bound how many
/// floats there are and characters bound how many lines, and the work here is
/// the **product**.
///
/// A unit is one float examined for one question — is it beside this height, is
/// its inner edge in the way, is its bottom the next one down. It is charged
/// where the loop is entered rather than inside it, so the cost of a scan is
/// known before the scan happens and a book past the cap is refused rather than
/// swept.
pub const MAX_LAYOUT_WORK: usize = 4_000_000;

/// The relations, in a `const` block so a build that broke one **does not
/// compile**.
///
/// Gap 29's device. Both are written the ordinary way round — a cap below what
/// stands in front of it — and both exist because the alternative is a constant
/// that can never fire, which behaves exactly like one that is never
/// approached.
const _: () = {
    assert!(
        MAX_BOX_DEPTH < MAX_BOX_TREE_NODES,
        "a tree of depth MAX_BOX_DEPTH has at least that many boxes in it, so a depth cap at \
         or above the box cap could never fire"
    );
    assert!(
        MAX_LAYOUT_WORK < MAX_BOX_TREE_NODES * MAX_BOX_TREE_NODES,
        "placing the last of MAX_BOX_TREE_NODES floats examines the others, so a float-work \
         cap at or above the square of the box cap could never fire"
    );
    assert!(
        MAX_LAYOUT_PAGES < MAX_LINE_BREAK_WORK,
        "a page needs at least one line box and a line box needs at least one character, so \
         a page cap at or above the break total could never fire"
    );
};
