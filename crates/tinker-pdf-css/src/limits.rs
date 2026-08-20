//! Every bound this crate enforces, in one place, with the numbers that set it.
//!
//! The form is `tinker-pdf-zip`'s `limits.rs` and `tinker-pdf-xml`'s, and it is
//! that shape for the two scars those files record. `5adf502` found an
//! 1 851-byte page that took 19.3 seconds to render with `MAX_GROUP_DEPTH` in
//! place the entire time — *depth is not work once the recursion branches*. Gap
//! 18a's milestone 8 found the opposite failure in a constant written to avoid
//! the first: `MAX_JPX_WORK` sat *above* the most its own inputs could ask for,
//! so it could never fire. **A cap that cannot fire is not a cap.**
//!
//! Every constant here carries three numbers — the most any fixture in this
//! repository spends, the most a plausible real book spends, and the constant —
//! and each is proved to fire in a test **by its own refusal or warning, never
//! by a clock**. The yardstick for the second number is gap 31's own, named in
//! its bounds section: **a 400-page novel of 120 000 words in 40 spine items,
//! with four stylesheets totalling 40 KB and two embedded faces**.
//!
//! # Which of these are totals, and which are not
//!
//! Four are **work caps**, spent across a whole book and never refunded:
//! [`MAX_CSS_TOKENS`], [`MAX_CSS_RULES`], [`MAX_CSS_DECLARATIONS`] and
//! [`MAX_SELECTOR_MATCHES`]. A stylesheet count is chosen by the file — an
//! EPUB's manifest may name four thousand of them — so a per-sheet cap times a
//! file-chosen sheet count is not a bound on anything. They live in [`Budget`],
//! which is threaded through parsing and through the cascade, so that the total
//! is one object rather than a convention.
//!
//! The rest are per-item and each says so in its own doc comment, in the
//! register `MAX_SCRIPT_STEPS`, `MAX_MESH_TRIANGLES` and gap 30's XPS constants
//! already use.
//!
//! # The one that is a product, and the `const` block that proves it
//!
//! [`MAX_SELECTOR_MATCHES`] is the number gap 31's bounds table calls *"the
//! single most important constant in this plan"*, because selector matching is
//! the one cost in this engine whose reachable ceiling is a **multiplication**:
//! rules times elements. Neither factor bounds the other, so the relation is
//! asserted at compile time in the opposite direction from the ordinary ones —
//! the product must **exceed** the cap, or the cap could never fire. That is
//! gap 29's `const`-block device pointed at the number this plan is most likely
//! to get wrong.
//!
//! # What it counts is compounds, not selectors, and that is an amendment
//!
//! Gap 31's bounds table writes [`MAX_SELECTOR_MATCHES`] as *"selector-against-
//! element attempts"*. Milestone 6 charges it per **compound**-against-element
//! test instead, and the change is not bookkeeping. Matching `a b c d` against
//! an element walks the ancestor chain with backtracking, so one
//! selector-against-element attempt is `O(depth^parts)` compound tests, and a
//! cap on the attempts would bound a number that is not the work. It is
//! `5adf502`'s sentence one level further down: *the outer count is not the
//! work once the inner loop branches.* [`MAX_CSS_SELECTOR_PARTS`] bounds one
//! attempt's shape and this bounds its cost, and the two are different claims.

/// The most bytes one stylesheet's source may be.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 8 388 608 (the sheet built exactly *at* this cap, which is read; the one built past it is a byte longer and refused. The largest stylesheet in the committed corpus is **5 009**, measured by `epub_css.rs`) |
/// | A 400-page novel | 40 960 across four sheets, the largest of them ~20 000 — four times the largest a real producer wrote here |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **8 388 608** |
///
/// A per-item cap and **not** a work cap: it bounds one sheet, and a book may
/// hold as many sheets as its manifest names. [`MAX_CSS_TOKENS`] is the total
/// and this is not a substitute for it.
///
/// The ceiling in front of it is `tinker_pdf_zip::limits::MAX_ZIP_ENTRY_BYTES`,
/// 128 MiB, because every stylesheet in an EPUB is a ZIP entry — sixteen times
/// this cap, so the cap is a cap.
pub const MAX_CSS_BYTES: usize = 8 << 20;

/// Tokens produced across **every** stylesheet in one book.
///
/// | | Tokens |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 000 000 (the fixture built *past* this cap produces one more and is refused; the largest stylesheet in the committed corpus produces **1 392**, measured by `epub_css.rs`) |
/// | A 400-page novel | ~11 400, at 40 960 bytes and the 0.28 tokens a byte the committed corpus measures |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **4 000 000** |
///
/// **A work cap**, and the reason it is one is the sentence at the top of this
/// file: an EPUB's manifest chooses how many stylesheets there are, so a
/// per-sheet token cap times a file-chosen sheet count bounds nothing.
///
/// A token is at least one byte of source, so one sheet at [`MAX_CSS_BYTES`]
/// can produce 8 388 608 of them — twice this cap — which is what makes it
/// reachable from a single entry rather than only from a hostile manifest.
pub const MAX_CSS_TOKENS: usize = 4_000_000;

/// Qualified rules admitted across the whole book.
///
/// | | Rules |
/// | --- | --- |
/// | The most any fixture in this repository spends | 20 000 (the sheet built exactly *at* this cap, which is read; the one past it declares 20 001 and is refused. The largest stylesheet in the committed corpus has **45**, measured by `epub_css.rs`) |
/// | A 400-page novel | ~370, at 40 960 bytes and the one rule per 111 bytes the committed corpus measures |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **20 000** |
///
/// **A work cap**, for [`MAX_CSS_TOKENS`]'s reason, and it is the left-hand
/// factor of the product in the `const` block below.
///
/// `a{}` is three bytes, so one sheet at [`MAX_CSS_BYTES`] can declare
/// 2 796 202 rules — a hundred and thirty times this cap.
pub const MAX_CSS_RULES: usize = 20_000;

/// Declarations admitted across the whole book.
///
/// | | Declarations |
/// | --- | --- |
/// | The most any fixture in this repository spends | 100 000 (the fixture built *past* this cap declares one more and is refused; the largest stylesheet in the committed corpus has **99**, measured by `epub_css.rs`) |
/// | A 400-page novel | ~810, at 40 960 bytes and the one declaration per 51 bytes the committed corpus measures |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **100 000** |
///
/// **A work cap**, for [`MAX_CSS_TOKENS`]'s reason. `a:b;` is four bytes, so
/// one sheet at [`MAX_CSS_BYTES`] can declare 2 097 152 of them.
pub const MAX_CSS_DECLARATIONS: usize = 100_000;

/// Compound selectors in one complex selector — `a b c` is three.
///
/// | | Compounds |
/// | --- | --- |
/// | The most any fixture in this repository spends | 64 (the selector built exactly *at* this cap, which parses; the one past it has 65 compounds and is dropped. The longest selector in the committed corpus has **5**, measured by `epub_css.rs`) |
/// | A 400-page novel | 5, which is what the committed corpus already writes |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **64** |
///
/// A per-item cap and **not** a work cap. It bounds one match attempt's
/// *shape*; [`MAX_SELECTOR_MATCHES`] bounds the cost of all of them together,
/// and the two are different claims — a selector of four compounds costs
/// `O(depth^3)` compound tests against a deep tree, so bounding the parts does
/// not bound the work and bounding the work does not bound the parts.
///
/// A selector past it is **dropped with a counted warning** rather than
/// refusing the sheet: `css-syntax-3`'s error recovery is normative and ruling
/// 2 degrades rather than fails, so the rest of the stylesheet is still read.
///
/// One compound is at least one byte of source (`a`) plus one for the
/// combinator, so one sheet at [`MAX_CSS_BYTES`] can write a selector of
/// 4 194 304 compounds.
pub const MAX_CSS_SELECTOR_PARTS: usize = 64;

/// How deep `@import` may nest.
///
/// | | Levels |
/// | --- | --- |
/// | The most any fixture in this repository spends | 8 (the chain is read to exactly this depth and warns at the ninth; no book in the committed corpus uses `@import` at all, which `epub_css.rs` asserts rather than assumes) |
/// | A 400-page novel | 1 |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **8** |
///
/// A per-item cap on the recursion, and it is **not** the whole defence: a
/// cycle is two lines of CSS, and a depth cap without a cycle guard means the
/// same two files are read eight times over rather than for ever. Both are
/// here, and [`crate::Warning::ImportCycle`] is a different name from
/// [`crate::Warning::ImportTooDeep`] because they are different facts about a
/// book.
///
/// Nothing bounds the ceiling in front of it except the resolver, which is the
/// caller's: an `@import` chain is as deep as the container has entries, and a
/// self-importing sheet is infinitely deep. That is why the cap is not a large
/// number — it is not standing in front of an attacker's budget, it is standing
/// in front of an unbounded one.
pub const MAX_CSS_IMPORT_DEPTH: usize = 8;

/// Elements one cascade pass may be handed.
///
/// | | Elements |
/// | --- | --- |
/// | The most any fixture in this repository spends | 65 536 (the tree built *past* this cap holds one more and is refused; the largest content document in the committed corpus has **69** elements) |
/// | A 400-page novel | ~12 000 across 40 spine items, at ~300 an item — the committed corpus's chapters are a few hundred words each and hold 69, so this is an extrapolation to a 3 000-word chapter and not a measurement |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **65 536** |
///
/// A per-item cap: one content document is one cascade pass, and a book has as
/// many passes as its spine has items.
///
/// **It is declared here rather than with the element tree, and that is
/// deliberate.** Gap 31's bounds table calls this *"elements admitted from one
/// content document"* and the tree that admits them is milestone 8's. It lives
/// in this crate because it is the right-hand factor of the `const` block
/// below, and a compile-time relation can only see constants its own crate can
/// name — this crate's allow-list is empty, so it cannot reach
/// `tinker_pdf_xml::limits::MAX_XML_TOKENS`. The other half of the relation,
/// `MAX_DOM_NODES < MAX_XML_TOKENS`, is therefore owed by the facade, which
/// depends on both; it is recorded in the ledger's `reachable` column in the
/// meantime, where `MAX_XML_TOKENS` is 1 048 576 — sixteen times this cap, so
/// the cap fires long before the parser in front of it does.
pub const MAX_DOM_NODES: usize = 65_536;

/// Compound-selector-against-element tests across the whole book.
///
/// | | Tests |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 000 000 (the pair built *past* this cap would spend 4 002 000 and is refused at 4 000 001; the thousand-element document `an_ordinary_book_is_far_under_the_match_budget` cascades spends under 80 000) |
/// | A 400-page novel | ~480 000, at 12 000 elements against ~20 candidate rules of ~2 compounds each. An extrapolation, like the row above: nothing in this repository cascades a real book until milestone 8 |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **4 000 000** |
///
/// **The work cap of the cascade, and the most important constant this crate
/// has.** [`MAX_CSS_RULES`] bounds the stylesheet and [`MAX_DOM_NODES`] bounds
/// the document, and **neither bounds the other**: their product is what the
/// cascade actually spends, and a file chooses both factors independently.
/// That is `5adf502`'s finding in its purest form.
///
/// The matcher indexes rules by their rightmost compound's most selective key —
/// id, then class, then type, then the universal bucket — so an ordinary book
/// tests each element against a handful of rules rather than against all of
/// them. **The index is why the number is small and is not why the cap exists.**
/// A stylesheet whose every rule names the same class defeats the index
/// completely and gets the full product, which is exactly the input a hostile
/// book would write and exactly what
/// `the_match_budget_refuses_a_stylesheet_that_defeats_the_index` builds.
pub const MAX_SELECTOR_MATCHES: usize = 4_000_000;

/// The relations, checked at compile time so a bad one **does not build**.
///
/// Gap 29's device. The one that matters is written the opposite way round
/// from an ordinary ordering: it asserts the product is **larger** than the
/// cap, which is `every_bound_can_fire`'s check promoted to compile time for
/// the one constant whose reachable ceiling is a multiplication rather than a
/// field width.
const _: () = {
    // Gap 18a milestone 8's failure, at compile time: if the most the cascade's
    // own inputs can ask for were below the cap, the cap could never fire and
    // would be decoration.
    assert!(
        (MAX_CSS_RULES as u128) * (MAX_DOM_NODES as u128) > MAX_SELECTOR_MATCHES as u128,
        "MAX_SELECTOR_MATCHES is above the product of its own two factors, so it can never fire"
    );
    // A token is at least one byte, so a sheet at the byte cap must be able to
    // cross the token cap — otherwise the total is unreachable from one entry.
    assert!(
        MAX_CSS_BYTES > MAX_CSS_TOKENS,
        "one stylesheet at MAX_CSS_BYTES cannot reach MAX_CSS_TOKENS, so the total is \
         reachable only from a hostile manifest"
    );
    // A rule carries at least one declaration's worth of room, and a book with
    // more rules than declarations is not a book this ordering describes.
    assert!(
        MAX_CSS_RULES < MAX_CSS_DECLARATIONS,
        "MAX_CSS_RULES is at or above MAX_CSS_DECLARATIONS, so the declaration cap fires \
         first and the rule cap can never fire"
    );
};
