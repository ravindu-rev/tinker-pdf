# CFF fonts draw the wrong glyphs

A document embedding a Type 1 face as `/FontFile3 /Subtype /Type1C` — which is
how they have been embedded since PDF 1.2 — renders text made of the wrong
letters. Not missing, not blank: wrong, at the right widths, so it looks like
a font substitution rather than a bug. When this is done, a CFF font draws the
glyphs the file names. (L)

## What is wrong

`crates/tinker-pdf/src/resources.rs`, in `extract_outline`:

```rust
if let Some(cff) = Cff::parse(program) {
    let glyph = u16::try_from(code).ok()?;
    let outline = cff.outline(glyph)?;
```

The character code *is* the glyph index. Code 65 — `A` in every Latin
encoding — fetches glyph 65, which in a typical subset is somewhere in the
punctuation.

The parser cannot do better, because it does not read the tables that would
let it. `crates/tinker-pdf-font/src/cff.rs` is 795 lines and exposes exactly
`parse`, `glyph_count`, `outline(glyph: u16)`, `advance(glyph: u16)` and a
`font_matrix` field. Absent entirely: the **charset** (GID → SID for a simple
font, GID → CID for a CID-keyed one), the **CFF encoding** (code → GID), the
**string index** and the **391 standard strings** that a SID resolves through,
and `ROS` / `FDArray` / `FDSelect` for CID-keyed fonts.

The two branches beside it in the same function both resolve properly:
TrueType goes through `cmap`, and Type 1 through its own encoding. CFF is the
lone outlier.

## Scope

- Charset parsing, formats 0, 1 and 2 (14.1 in the CFF specification), plus
  the three predefined charsets `ISOAdobe`, `Expert` and `ExpertSubset`.
- The string index and the standard strings, so a SID resolves to a name.
- `Cff::gid_for_name(&str) -> Option<u16>` for simple fonts.
- CFF encoding, formats 0 and 1 with supplements, and the two predefined
  encodings — used only when the PDF font dictionary supplies no `/Encoding`,
  because 9.6.6 makes the PDF's encoding win.
- CID-keyed support: `ROS` detection, `FDArray`, `FDSelect` formats 0 and 3,
  per-FD private dicts and local subrs, and `Cff::gid_for_cid(u32)`.
- Per-FD font matrices, which a CID-keyed font may carry instead of a top-level
  one.
- Wire `extract_outline` to resolve a code through the font dictionary's
  encoding to a glyph *name*, then through the charset to a GID.

## Non-goals

- **Type 2 charstring changes.** The interpreter is correct and tested; this
  is entirely about which charstring gets run.
- **CFF2.** A different format with a different variation model, and no PDF
  version references it.
- **Writing CFF.** The subsetter emits TrueType; a CFF subsetter is separate
  work nobody has asked for.

## Design

**Where the name comes from.** For a simple font the PDF font dictionary's
`/Encoding` — base encoding plus `/Differences` — maps a code to a glyph name.
`crates/tinker-pdf-cos/src/font.rs` already models this for width lookup;
`extract_outline` needs the same name, so the accessor it needs is a glyph
*name* for a code rather than a character.

**Fallback order**, which matters because real files omit things:

1. The PDF `/Encoding` gives a name → charset gives the GID.
2. No PDF encoding → the CFF font's own encoding gives the GID directly.
3. Neither → the standard encoding's name for the code, through the charset.
4. Still nothing → `.notdef`, and count it in `missing_fonts` so the page
   reports rather than silently drawing the wrong thing.

Step 4 is the one that must not be skipped. The current behaviour is
effectively "always step 4, but pick a random glyph instead of `.notdef`".

**The standard strings** are 391 fixed names. They belong in `cff.rs` as a
`&[&str; 391]` — no build step, no data file, and small enough that the wasm
budget does not notice.

**CID-keyed fonts** invert the charset: it maps GID → CID, and the lookup
needs CID → GID. Build the reverse map once at parse time. Sizes are bounded
by `glyph_count`, which is already read.

## Where a half-implementation is worse than none

Shipping the charset without the encoding fallback chain. A font whose PDF
dictionary omits `/Encoding` would then resolve through *no* path and draw
`.notdef` everywhere — visibly broken, where today it is invisibly wrong. That
is arguably an improvement, but it will read as a regression to anyone whose
files happened to work, so land the chain whole.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | String index, standard strings, charset formats 0/1/2 | A hand-built CFF with a known charset returns the right GID for every name; the three predefined charsets resolve | M |
| 2 | `gid_for_name`, PDF-encoding wiring, fallback chain | A `/FontFile3 /Subtype /Type1C` fixture renders `A` as `A`; a fixture with no `/Encoding` still resolves through the font's own | M |
| 3 | CFF encoding formats 0/1 and supplements | A font relying on its built-in encoding resolves without a PDF `/Encoding` | S |
| 4 | `ROS`, `FDArray`, `FDSelect`, per-FD private dicts and matrices | A CID-keyed CFF renders correct glyphs; per-FD subrs are used for the right glyphs | M |
| 5 | Fuzz target for the new tables | `cargo fuzz run cff` survives a session with the charset and FDSelect paths reachable | S |

## Dependencies

**Needs first:** nothing. The charstring interpreter is done.

**Unblocks:** [02](02-cid-to-gid.md), which needs `gid_for_cid` to exist
before a CID can reach an outline. Real parity on any document embedding a
Type 1 face.

## Risks

| Risk | Mitigation |
| --- | --- |
| No CFF fixture exists in the tree — `testdata/` is four self-authored PDFs, none with an embedded CFF | Hand-build one in the test module, as the TrueType subsetting tests already do; the format is compact enough to author byte by byte, and a fixture whose every byte is known is what makes "the right GID" checkable by arithmetic |
| Charset format 2 uses 16-bit run lengths and format 1 uses 8-bit; confusing them silently shifts every glyph after the first run | A fixture per format, asserting the *same* name-to-GID map from all three |
| The reverse CID map is attacker-sized | Bound by `glyph_count`, which is already read and clamped; build it lazily and cap it |
