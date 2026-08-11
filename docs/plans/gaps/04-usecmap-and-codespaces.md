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
