# An embedded CMap cannot inherit from its parent

`usecmap` is how a CMap says "everything the parent defines, plus these
changes". A file using one gets only the changes: no codespace ranges, and
whichever CID ranges it happened to restate. Strings split at the wrong
boundaries and most codes map to nothing. When this is done, an embedded CMap
that inherits behaves as though it had been written out in full. (S)

## What is wrong

`cmap::parse` (`crates/tinker-pdf-font/src/cmap.rs`) is a token scanner that
handles `begincodespacerange`, `beginbfchar`, `beginbfrange`, `begincidchar`,
`begincidrange` and `/WMode`. `usecmap` appears nowhere in the repository —
not in code, not in docs, not in a test.

A CMap that opens

```postscript
/90ms-RKSJ-H usecmap
```

therefore starts empty. Since the parent supplies the codespaces in the common
case, the child ends up with none, and `Font::decode` falls back to splitting
one byte per code.

Two smaller things sit alongside it. The CMap stream *dictionary* keys are
never read — `/UseCMap` there is an alternative spelling of the same
inheritance — and the `end*` operators are not matched by name: each section
loop terminates on the first non-hex token, which is why a truncated section
ends quietly rather than being noticed.

## Scope

- `usecmap` in the CMap body: resolve the named parent, inherit its
  codespaces, CID ranges and bf ranges, then apply the child's own on top.
- `/UseCMap` in the CMap stream dictionary, which may be a name or a stream.
- A recursion cap and a cycle guard — a CMap that uses itself, directly or
  through a chain, must terminate.
- Match `endcodespacerange` and friends by name, so a malformed section is
  noticed rather than absorbed.

## Non-goals

- **Making the parents correct.** If the parent is `90ms-RKSJ-H` the
  inheritance is only as good as [03](03-predefined-cmaps.md). Inheriting from
  a stub is still better than inheriting from nothing, because at least the
  codespaces will be right once 03 lands.

## Design

Inheritance is a merge, not a replacement: the child's ranges override the
parent's where they overlap, and the parent's survive where they do not. The
child's `/WMode` wins if it declares one.

The parent lookup goes through `CMap::predefined` for a name, and through the
document for a stream. That second path means `cmap.rs` cannot resolve it
alone — it is a leaf crate and knows no PDF types (ruling 8) — so the resolver
is passed in as a closure by `crates/tinker-pdf-cos/src/font.rs`, which is
where the stream can be fetched. The same shape the filter chain already uses
for `/DecodeParms` resolution.

*Amended August 2026.* The closure is asked two different questions, not one,
because the dictionary form has to reach the chain as well: `ParentRef::Named`
is a name the predefined set did not recognise, and `ParentRef::Dictionary` is
"the `/UseCMap` entry of the stream whose source these bytes are" — the only
way a leaf crate can ask for something it cannot address. The answer is a
source or another name, so both spellings walk one chain under one cap and one
cycle guard.

Cap the chain at four. Real CMaps nest one deep, occasionally two.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `usecmap` with a name parent, merge semantics, cycle guard | A child declaring only one `cidrange` inherits the parent's codespaces; a self-referential CMap terminates | S |
| 2 | `/UseCMap` from the stream dictionary, name and stream forms | A fixture inheriting through the dictionary resolves identically to one inheriting in the body | S |
| 3 | `end*` matched by name | A truncated section produces a warning rather than silently ending | S |

## Dependencies

**Needs first:** nothing, though it is worth little until
[03](03-predefined-cmaps.md) makes the parents real.

**Unblocks:** documents with a customised registry encoding — common in
Japanese publishing workflows.

## Risks

| Risk | Mitigation |
| --- | --- |
| The resolver closure re-enters the document while a font is being built | The same guard the filter-parameter resolver uses; cap the chain and refuse a cycle rather than recursing |
| Merge order silently wrong — parent overriding child | A fixture where both define the same range with different CIDs, asserting the child's wins |
| A fixture whose codes are below 0x100 cannot tell a two-byte read from a one-byte one | *Added August 2026.* Both compute the same number, so the test passes on both sides of the change. Every fixture's codespace is Shift-JIS-shaped and starts at 0x81, where a byte-at-a-time split yields codes nothing maps |

## As built

*August 2026.* All three milestones are done, in two commits. Everything in
Scope is implemented and the Non-goal held. Six things the plan did not say.

1. **`usecmap` did appear in the repository, in one place the plan could not
   have known about.** "Appears nowhere — not in code, not in docs, not in a
   test" was true when written. [PRE-C](00-execution-order.md) then landed a
   `usecmap` seed in `fuzz/corpus/cmap` and a line about it in
   `fuzz/README.md`. Nothing in code, which was the part that mattered, and
   the other two defects were live exactly as described: the CMap stream
   dictionary's keys were never read at all, and each `read_*` loop
   terminated on the first token of an unexpected *shape*.

2. **The `end*` defect was worse than "a truncated section ends quietly".**
   It also *consumed* the token that ended it. So a truncated `beginbfchar`
   followed by `begincidrange` lost the whole `cidrange` as well, and
   everything after it read as loose tokens. Sections now close on their own
   operator by name; anything else leaves the keyword unread for the outer
   scan, which is a second test
   (`a_truncated_section_does_not_swallow_the_next_one`) and not only a
   warning.

3. **`CMap::cid` had to be reordered before the merge could work.** It tested
   the `identity` flag first and returned the code, short-circuiting the
   CMap's own entries. That is invisible while nothing merges — an identity
   CMap has no entries — and destroys a child the moment one does, because
   every CJK stub `CMap::predefined` returns *is* an identity, so
   `/90ms-RKSJ-H usecmap` would have thrown the child's own `cidrange` away
   entirely. Entries now win over the fallthrough. No existing behaviour
   moves.

4. **Merge order needed three rules, not one.** Appending the parent's ranges
   behind the child's is necessary and not sufficient: a *single* is
   consulted before any range, so a parent `cidchar` inside a child
   `cidrange` would still outrank it. Parent singles the child already
   answers are dropped during the merge. The third rule is 9.7.5.1's, which
   needs `wmode_declared` as a separate state, because "declared horizontal"
   and "said nothing" are different and `vertical` alone cannot tell them
   apart.

5. **The cycle guard compares sources, not names.** A name-keyed guard misses
   two CMaps that use each other under different names, and misses one stream
   reached by two spellings. It also could not have covered both the body
   form, whose links are names, and the dictionary form, whose links are
   object references. Comparing the decoded source bytes covers both with one
   guard. Following a name to another name does not spend a link — it has not
   moved down the chain, only been spelled again — so the cap of four means
   four parents rather than two.

6. **A `/UseCMap` that cannot be fetched is reported from the COS side.** The
   resolver closure answers `None` both for "there is no `/UseCMap`" and for
   "there is one and it is a dangling reference", and the leaf crate cannot
   tell those apart. `read_cmap` records the refusal itself before parsing.
   7.3.9's rule that a null value is an absent key keeps `/UseCMap null`
   quiet.

**Why the body form still resolves only predefined names.** A `usecmap`
operator names a CMap, and a name has no address in a PDF: 9.7.5.2's
predefined set is the only place one can come from, so
`tinker-pdf-cos`'s resolver answers every `ParentRef::Named` with `None` and
`CMap::predefined` answers it instead. That is not a hole — it is where gap 03
plugs in.

**Ruling 4:** no determinism fingerprint moved, on native Windows or on
`wasm32-wasip1` under wasmtime 47.0.3. Expected — that fixture's `text` page
embeds a simple TrueType font and reaches no CMap — and checked, because
`font::read` now runs a warning sink on every font it reads.

**The parents in every test are hand-built, and prove nothing about a real
predefined one.** This is stated in the fixtures' own text as well as here,
so a passing test cannot later read as evidence that `90ms-RKSJ-H`
inheritance works. Until [03](03-predefined-cmaps.md) lands, that name
resolves to a two-byte identity stub; a fixture inheriting from it would be
measuring the stub, and would keep passing when the stub was replaced by real
data with different contents.

**For [03](03-predefined-cmaps.md):** everything it needs is already wired.
To make these parents real it must (a) put the compiled tables behind
`CMap::predefined`, which is the single function this chain calls for a name
and the first thing it tries, ahead of the resolver, so nothing else has to
change; and (b) clear `approximate` on the collections it ships. The merge
propagates `approximate` deliberately — a child inheriting from a stub
reports as approximate today, which is what
`inheriting_from_an_approximate_parent_stays_approximate` pins — so that flag
stops being set on its own as soon as the stub goes. 03 M5's "`is_approximate`
has a caller" is still owed; nothing here calls it outside a test.

**Test count:** 1027 -> 1060.
