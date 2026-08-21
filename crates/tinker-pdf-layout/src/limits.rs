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
//! layout or not at all"*. Floats are that layout, and milestone 11's tables
//! are the other half of the same sentence: §17.5.2.2 is two passes, a cell is
//! laid out three times, and a nested table multiplies all of it. It is spent
//! across a whole book for the same reason as the other two.
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
/// | The most any fixture in this repository spends | 2 097 153 (the tree built *past* this cap; the largest real content document in the committed corpus generates 138) |
/// | A 400-page novel | ~36 000, at 12 000 elements and about three boxes each |
/// | **The densest real book in the fetched corpus** | **993 349** — `sample-linear-algebra.epub`, 94 content documents of MathML |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **2 097 152** |
///
/// **A work cap.** Boxes are not elements and the difference is the reason
/// this is not `MAX_DOM_NODES` under another name: anonymous block generation
/// (CSS 2.2 §9.2.1.1), `::before`/`::after` and table-structure fixup (§17.2.1)
/// each create boxes the document did not write, and a book chooses how many
/// documents it has.
///
/// # Raised eightfold by milestone 13, and the reason is the third yardstick
///
/// This was 262 144 from milestone 7 until milestone 13, and the row above that
/// made it look safe was *"the largest real content document in the committed
/// corpus generates 138"*. That figure is true and it is about **six books this
/// repository commissioned**. Milestone 13 added `bounds_ledger.rs`'s third
/// yardstick — a book, beside gap 29's comic and gap 30's fixed document — and
/// the first thing it did was measure the twenty fetched books through
/// [`crate::Budget`] rather than estimate them:
/// `sample-linear-algebra.epub` needs **993 349** boxes, which is 3.8 times
/// what this cap allowed.
///
/// It was not refused loudly. The budget is spent across a whole book, so the
/// chapter that crossed the cap and **every chapter after it** became grey
/// placeholder pages — a 94-chapter W3C sample book that opened, paginated to
/// its spine, and silently lost two thirds of its text. That is the failure
/// `bounds_ledger.rs` exists for, arriving from the direction it always warned
/// about: *a cap that refuses the thing the format is for is not a bound, it is
/// a missing feature*. Three boxes per element is right for prose and wrong for
/// MathML, where `<mi>`, `<mo>` and `<mrow>` put an inline box on every symbol.
///
/// Two point one million rather than one: a cap set at 1.055 times the densest
/// book anybody has measured is the same mistake with a bigger number in it.
pub const MAX_BOX_TREE_NODES: usize = 2_097_152;

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

/// Float examinations and table slots across a whole book.
///
/// | | Examinations |
/// | --- | --- |
/// | The most any fixture in this repository spends | 16 000 001 (the book built *past* this cap; the largest real book in the **committed** corpus spends 39) |
/// | **The densest real book in the fetched corpus** | **4 233 567** — `sample-epub30-spec.epub`, the EPUB 3.0 specification as a book |
/// | A 400-page novel | ~24 000 (one figure per spine item, 12 000 line boxes, each asking the figures beside it for its measure) |
/// | A 400-page novel with a figure on every fourth page | ~240 000 |
/// | A 400-page novel with a twenty-row table in every tenth chapter | ~1 000 (four tables, sixty slots, three columns and three widths each) |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **16 000 000** |
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
///
/// # Milestone 11 added the other half of the sentence, and it is table slots
///
/// The row that predicted this cap named *two* multipliers: *"automatic table
/// layout is two passes (§17.5.2.2), float placement re-flows a line,
/// shrink-to-fit measures twice, and **a nested table multiplies all three**"*.
/// Tables charge it in three places, and the three are three different
/// quantities rather than one quantity counted thrice — which is what a firing
/// fixture for each of them says:
///
/// | Charged | The quantity | Why the other two do not bound it |
/// | --- | --- | --- |
/// | [`crate::table::Grid::place`] | slots occupied, `colspan` × `rowspan` per cell | Five boxes can claim five million slots: a `colspan` is a number in the file and neither the box cap nor the break cap can see it |
/// | The occupancy map | grid rows × grid columns | Two thousand rows whose first one spans two thousand columns is four thousand slots to *place* and four million to *hold* |
/// | §17.5.2.2's distribution | columns + every spanning cell's span | One row of 1 200 000 columns is under the total on the first two and past it with the third |
///
/// **And a nested table multiplies every one of them.** An outer cell is laid
/// out three times — twice to measure its two content widths and once to set it
/// — so a table inside a cell pays its whole bill three times over, and the
/// same table alone is under the total while nested it is not. That pair of
/// fixtures is `a_nested_table_multiplies_the_work_total`, and it is the
/// clearest statement of what this constant is for that this crate has.
/// # Raised fourfold by milestone 13, for [`MAX_BOX_TREE_NODES`]'s reason
///
/// The row above said *"the largest real book here spends under a thousand"*,
/// and it meant the six books this repository commissioned; the committed
/// corpus's own maximum is 39. Milestone 13's third yardstick measured the
/// twenty **fetched** books instead, and `sample-epub30-spec.epub` — the EPUB
/// 3.0 specification, published as an EPUB — spends **4 233 567**.
///
/// It crossed the cap on page 601 of 777 and every page after it was a grey
/// placeholder. Nothing was silent about it: `epub_fetched.rs`'s placeholder
/// sweep says `NotFragmented` by name, which is what a refusal is for — but a
/// refusal aimed at a book W3C publishes is a missing feature wearing a `MAX_`
/// prefix, which is the other half of what `bounds_ledger.rs` checks.
///
/// Sixteen million is 3.8 times the measured book, the same margin the box cap
/// above took, and it stays five thousand times under the square this row's
/// argument rests on.
///
/// # What this cap costs to reach, measured
///
/// *Added by milestone 13's campaign, which is the first session the `layout`
/// fuzz target has ever run.* A **107-byte** input built a tree of about twenty
/// elements, nested twelve deep, and spent **443 137 boxes, 3 899 421 break
/// evaluations and 354 618 float examinations** producing **one page** — in
/// twenty-two seconds on the machine that ran it.
///
/// The mechanism is the one the table above names, at every level rather than
/// at one: a shrink-to-fit container lays its contents out **three times**,
/// twice to measure and once to set, so nesting multiplies by three per level
/// and twelve levels is 3^12. Nothing here is unbounded — every one of the
/// three totals refuses that input by name if it is lowered — and that is the
/// point worth stating plainly: **the totals are all that stands between a few
/// hundred bytes of nested markup and an engine that does not come back.**
/// [`MAX_BOX_DEPTH`] is 256, so the multiplier's ceiling is far above anything
/// arithmetic would forgive, and the budget is what bounds it.
///
/// Two consequences follow and both are honest costs of the raise above:
/// reaching this cap now takes about four times as long as it did at 4 000 000,
/// and the `layout` fuzz target's generator was lowered from twelve levels to
/// nine so that a session explores rather than spending itself on one input.
/// **Memoising a sublayout's measurement is the fix**, it would remove the
/// multiplier rather than charge for it, and it is not this milestone's.
pub const MAX_LAYOUT_WORK: usize = 16_000_000;

/// The relations, in a `const` block so a build that broke one **does not
/// compile**.
///
/// Gap 29's device. Both are written the ordinary way round — a cap below what
/// stands in front of it — and both exist because the alternative is a constant
/// that can never fire, which behaves exactly like one that is never
/// approached.
///
/// # The square is formed in `u128`, and that is not tidiness
///
/// *Corrected by milestone 13.* The second relation was written
/// `MAX_LAYOUT_WORK < MAX_BOX_TREE_NODES * MAX_BOX_TREE_NODES` in `usize`, and
/// 262 144 squared is 6.9e10 — which fits a 64-bit `usize` and **does not fit a
/// 32-bit one**. So this block did not evaluate on `wasm32`, and a `const`
/// assertion that does not evaluate is a build failure rather than a silent
/// pass: `cargo test --target wasm32-wasip1` has not compiled this crate since
/// milestone 10 wrote the row. A relation about a product has to be formed in a
/// width that holds the product, on **every** target the engine claims — which
/// is ruling 4's own argument arriving somewhere nobody was looking.
const _: () = {
    assert!(
        MAX_BOX_DEPTH < MAX_BOX_TREE_NODES,
        "a tree of depth MAX_BOX_DEPTH has at least that many boxes in it, so a depth cap at \
         or above the box cap could never fire"
    );
    assert!(
        (MAX_LAYOUT_WORK as u128) < (MAX_BOX_TREE_NODES as u128) * (MAX_BOX_TREE_NODES as u128),
        "placing the last of MAX_BOX_TREE_NODES floats examines the others, so a float-work \
         cap at or above the square of the box cap could never fire"
    );
    assert!(
        MAX_LAYOUT_PAGES < MAX_LINE_BREAK_WORK,
        "a page needs at least one line box and a line box needs at least one character, so \
         a page cap at or above the break total could never fire"
    );
};
