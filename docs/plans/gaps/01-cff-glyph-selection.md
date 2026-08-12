# CFF fonts draw the wrong glyphs

A document embedding a Type 1 face as `/FontFile3 /Subtype /Type1C` — which is
how they have been embedded since PDF 1.2 — renders text made of the wrong
letters. Not missing, not blank: wrong, at the right widths, so it looks like
a font substitution rather than a bug. When this is done, a CFF font draws the
glyphs the file names. (L)

> **The paragraph above was half right, and the half it got wrong is the more
> common half.** It was written while `Cff::parse` still returned `None` for
> every real font program — the INDEX reader was off by a byte until
> `263fcf0` — so this branch had never been entered and the symptom could not
> have been observed. Measured afterwards, it takes two shapes and which one
> a file gets depends on its glyph count: a **subset** font, which is what a
> producer embeds, has too few glyphs for the code to land on one at all, so
> the page comes out **blank** where the text should be (and does report
> `UnreadableFont`, about the wrong thing); a **full** font draws the wrong
> letter at the right width, silently, which is the substitution described
> above. See `As built` at the end of this file.

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
| The reverse CID map is attacker-sized | Bound by `glyph_count`, which is already read and clamped ~~; build it lazily and cap it~~ — *amended August 2026:* built once at parse time, as the Design section above says and as this row contradicted. It is the same sorted array the name lookup uses, so building it lazily would mean two maps and a second bound to get right; 65535 glyphs is 256 KB, which is smaller than the charstrings that named them |
| A fixture per charset format that only checks itself passes with the 8-bit and 16-bit run lengths swapped | *Added August 2026.* The three formats have to describe **one** font, and the assertion is that the same name-to-GID map falls out of all three. The same is true of FDSelect's two formats, and the CID fixture is likewise built twice |

## As built

*August 2026.* All five milestones are done, in five commits plus this one.
Everything in Scope is implemented and every Non-goal held. Ten things the
plan did not say.

1. **The symptom was two symptoms, and the plan described the rarer one.**
   Corrected in the quoted block at the top of this file. The defect was
   measured before anything was changed, by building a fixture and rendering
   it: a four-glyph subset drew **zero** inked pixels for `A`, `B` and `C` and
   reported `UnreadableFont`; a seventy-glyph font drew glyph 65 — a real
   glyph, the wrong one, 100 pixels of ink where `A` should have put 830 —
   and reported nothing. Both are now regression tests named for what they
   were, because the two need different evidence: the first is a page that is
   blank, the second a page that is confidently wrong.

   This mattered for the fix as well as for the description. The `.notdef`
   path in step 4 of the fallback order looks like a no-op against the first
   symptom — both draw nothing — and the difference is entirely in what gets
   reported.

2. **Three sites used the code as an index, not one.** The plan quotes the
   bare `/FontFile3` branch. The `OTTO` branch four lines up had the same two
   lines, and now shares one function with it. The third is the TrueType
   fallback, and it is the interesting one: `.or_else(|| u16::try_from(code)
   .ok())` is **correct** for a composite font with an identity CID-to-GID
   map, where the code really is the glyph number, and it is the last resort
   every reader makes for a subset whose `cmap` was dropped. What was missing
   sat before it — 9.6.6.4 sends a symbolic font's code through the `cmap` *as
   a code*, into the F0xx private-use block for a (3,0) subtable, and only
   codes with a Unicode meaning were being tried, so a symbolic TrueType face
   skipped its own `cmap` entirely. Fixed; the last resort stays, with a
   comment saying which case it is for.

3. **`.notdef` is an outline, so the existing warning could not see it.** The
   renderer counts a glyph it was handed *nothing* for. Returning `.notdef` —
   which is what step 4 asks for, and what the specification asks for — hands
   it an outline, usually an empty one, which it reads as a space. So
   `render` now turns a non-empty `PageResources::missing_fonts` into
   `UnreadableFont` itself. The two uses of that list cannot contaminate each
   other, because text extraction never asks for an outline: on that path the
   list still holds only unresolvable font *names*, which is what
   `TextWarning::UnknownFont` reports.

4. **Step 1 falls through to step 2, which is not what 9.6.6 says literally.**
   The plan's order reads as "the PDF's encoding, *or* if there is none the
   font's". As built, a name the document supplies that reaches no glyph *in
   this font* also falls through. The clause is about which encoding is
   authoritative, not about refusing to look at a font that plainly disagrees
   with it, and a subset whose `/Differences` were written against the
   unsubsetted original is a file that exists. The precedence that matters is
   still pinned in both directions: one fixture's built-in encoding maps code
   65 to a different glyph than WinAnsi does, and it draws one glyph with an
   `/Encoding` in the dictionary and the other without.

5. **Annex D's StandardEncoding was missing its last eight names.** `Oslash`
   through `germandbls`. Nothing had noticed, because the codes they sit at
   have no character either — `base_char` returned `None` and extraction
   reported nothing. Glyph selection is what made it visible: step 3 of the
   fallback order turns a code into a name and the name into a glyph, so a
   missing name is a glyph that draws as `.notdef`.

6. **The reverse map is the forward map.** The plan treats the name lookup and
   the CID lookup as two structures. They are one: the charset inverted,
   sorted, deduplicated on the first glyph that claims a value, binary
   searched. A SID and a CID are the same sixteen bits in the same table, and
   which one it is depends only on whether `ROS` is present. The risk row
   above is amended accordingly.

7. **`gid_for_name` returns `None` for a CID-keyed font, deliberately.** CID
   34 and the SID of `A` are indistinguishable as numbers, so a name lookup
   against a CID table would answer confidently and wrongly. The same applies
   in reverse to `gid_for_cid`.

8. **The Expert data cannot be checked against reality here.** ISOAdobe is
   arithmetic; Expert and ExpertSubset are transcriptions of the
   specification's Appendix C, and no font in this repository — or in any
   corpus this repository has — uses either. What is checked instead is that
   the two transcriptions agree with each other: the expert character set is
   laid out in code order, so the Expert *encoding*'s SID column is the Expert
   *charset*, and a slip in one that is not also in the other fails
   `the_expert_tables_agree`. Recorded as a known limit rather than claimed as
   verified.

9. **Per-FD font matrices multiply.** The rule is not "whichever exists". With
   a matrix in both the Top DICT and a Font DICT, a glyph's space passes
   through the Font DICT's and then the Top DICT's; with only the Font DICT's,
   that one is the whole of it. All three cases are tested. The public
   `font_matrix` field keeps meaning the top-level one, so nothing that read
   it changed meaning, and `font_matrix_for(glyph)` is what the resolver uses.

10. **The fuzz session, and what it proves.** `cargo fuzz run cff
    -max_total_time=600 -timeout=25 -print_final_stats=1` on WSL2 /
    Ubuntu-24.04 under `rustc 1.99.0-nightly` and `cargo-fuzz 0.13.2` —
    libFuzzer does not build on this repository's development host, which
    [24](24-fuzz-execution.md)'s `As built` records. Seeded with the four committed corpus files and
    run on a *copy* of the directory, so the reviewable names survive
    (`fuzz/README.md`). **4,084,521 executions in 601 seconds**, 6,796 a
    second, 7,827 new units, slowest unit under a second, peak RSS 840 MB, and
    **no crash**: `artifacts/cff/` is empty.

    Reachability was measured rather than assumed, because a session that
    never enters the new code proves nothing. Replaying the corpus alone —
    `-runs=0`, nothing mutated — under `cargo fuzz coverage` executes
    `read_charset`, `read_charset_table`, `read_encoding`, `read_fd_select`,
    `read_fd_array`, `invert_charset`, `gid_for_sid`, `gid_for_cid`,
    `gid_for_code`, `sid_for_name` and `sid_name`: every reader this gap
    added, at 59% to 100% of regions each, before a single byte is mutated.
    The one seed that existed before reached 322 blocks; the four together
    reach 439.

    The target had to change to make that true. Asking a font only for
    outlines never touches any of these tables, so it now asks what each glyph
    is called and feeds the answer back in as a name — which is what reaches
    the *resolving* half with something the font actually holds. Arbitrary
    bytes almost never name a real glyph, and a target that only looked names
    up from its input would exercise the miss path and call it coverage. The
    session's own evidence that this worked is in the dictionary libFuzzer
    recommended at the end of it: `igrave`, which is one of the 391 standard
    strings, and `uniE001`, which is one byte away from the custom string in
    the `charset_and_encoding` seed. It got there by comparing names.

**Ruling 4:** no determinism fingerprint moved, on native Windows or on
`wasm32-wasip1` under wasmtime. Expected — that fixture embeds a TrueType face
and reaches none of this — and checked rather than assumed, because the
TrueType site changed too.

**For [gap 02](02-cid-to-gid.md):** `Cff::gid_for_cid` exists and is wired.
The CIDFontType0 half of that gap is therefore done — a composite font's code
becomes a CID through its encoding CMap and the CID reaches a glyph through
the charset, and a plain CFF inside a `CIDFontType0` uses the CID as a glyph
index (9.7.4.2), which is also written down now. What gap 02 still owns is the
CIDFontType2 half: `/CIDToGIDMap`, as a name or a stream, which nothing reads.

**Test count:** 988 -> 1011, plus one `#[ignore]`d writer that regenerates the
fuzz seeds from the fixtures so the two cannot drift. It is ignored because it
writes into a committed directory, and a run that rewrites the corpus is a diff
to look at rather than one to apply blindly.
