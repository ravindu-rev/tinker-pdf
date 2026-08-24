# Determinism

Rendering the same bytes with the same options produces bit-identical
output on every supported target. Not "visually identical" — bit-identical.
This is ruling 4, and it is what makes single-golden CI possible: a
fingerprint is stored once and every target must reproduce it exactly, so a
perceptual diff never has to absorb platform noise, and a real regression
cannot hide inside a tolerance.

## What it does

Three sources of cross-target variance are removed structurally rather than
chased:

- **Fixed-point coverage accumulation** in the rasterizer
  ([rasterizer](rasterizer.md)): edge walking and coverage sums use integer
  fixed-point, so there is no float summation-order or FMA-contraction
  variance to leak into pixel values.
- **No platform libm on any pixel path.** `tinker-pdf-math` supplies `sin`,
  `cos`, `tan`, `atan`, `atan2`, `ln`, `exp`, `powf`, `cbrt`, `log2` and
  `log10`, built from nothing but operations IEEE 754 pins exactly, and it
  is `no_std` so `x.sin()` does not compile inside it. The boundary is
  precise: `sqrt`, `floor`, `ceil`, `round`, `trunc` and `abs` **are**
  correctly rounded by the standard and may be used freely; only the
  transcendental family diverges between libms, and last-ulp differences
  become visible pixels after quantization. `cargo xtask libm` fails the
  build if a pixel-path crate calls a platform transcendental.
- **No environment-dependent behavior**: no locale-sensitive parsing, no
  hash-map iteration order on any output path, no time or randomness in the
  render path. Where a step count or level count is computed from geometry,
  it branches on 16.16 fixed-point integers, because a count one different
  is a different image.

The contract has a second half the container formats added: a book's page
count is a function of the box the caller passes
([epub](epub.md)), so the same book at two page boxes must each be stable
on every target **and the two must differ** — a build that ignored the
argument would satisfy "stable" twice over and be exactly as wrong as one
that paginated at random.

## API

Nothing to call — determinism is a property of `Page::render` and
`DocumentEditor::save`, not an option. The contract is stated here and
enforced by the suite below.

## Verified

`crates/tinker-pdf/tests/determinism.rs` commits **15 render
fingerprints** — `text`, `curves`, `shading`, `pattern`, `optional`,
`image`, `jbig2`, `jpx`, `blend`, `transparency`, `tiling`, `mesh`, `cbz`,
`xps`, `epub` — each a hash of rendered pixels plus the page dimensions and
ink counts it is computed from. Every fixture asserts a minimum ink count
and the absence of `UnreadableFont` before it is hashed, so a fixture that
draws nothing fails on the day it is added rather than becoming a baseline.

Beside them, **three document byte-hashes** pin the writer as well as the
renderer: a synthesised PDF, a synthesised fixed-layout document and a
synthesised book are each hashed as *bytes*, which is where object
numbering, dictionary key order and stream framing are pinned — none of
which a rendered hash can see. The `epub` fixture asserts the two-box claim
above: seven pages at one box, six at another, both stable.

Three fixtures carry guarantees of different kinds: one is synthesised
rather than parsed, so it pins the writer and parser to each other; one is
a package a third-party serialiser wrote, so it can disagree with this
engine about the *format* rather than only about arithmetic; and one is a
reflowable book, whose pagination is a function of the open options.

### Targets: measured versus claimed (August 2026)

| Target | Status |
| --- | --- |
| `x86_64-pc-windows-msvc` | measured — the committed fingerprint table is this one |
| `wasm32-wasip1` | measured, under wasmtime — tests width: 64-bit against 32-bit, where a `usize` assumption would show |
| `x86_64-unknown-linux-gnu` | measured, full suite green — tests everything below the arithmetic: a different `std`, allocator and linker |
| `aarch64-apple-darwin` | measured — observed 23 August 2026, [run 32670809479](https://github.com/ravindu-rev/tinker-pdf/actions/runs/32670809479) at `b76e6f9` |

**All four are measured, and the fourth took some getting to.** The macOS
row said *claimed, from CI configuration only* for as long as it did because
nobody had watched a run, and watching one found out why: `cargo clippy
--workspace --all-targets -- -D warnings` runs before `cargo test` in that
job and had been failing on every operating system for at least five runs,
so the leg had never reached the fingerprints at all. Three separate lints
had to be fixed, each found only by pushing, because CI's stable was 1.98
and the checkout doing the fixing was on 1.93.

The run recorded above has `test (macos-14)` and `wasm-determinism` green
on the same commit, which is what this claim needed. The native legs now
grep their own logs for `rendering_is_stable_across_targets ... ok`, the way
the wasm leg always has: `cargo test --workspace` does run those tests, but
a green tick from it is not evidence that it did, and an `#[ignore]` added
one afternoon would look identical. That is
[verification.md](../verification.md)'s "a check that can be absent is red,
not green", applied to the last place it was missing.

One thing settled itself along the way. The suite, fingerprints included,
passes on rustc 1.98 as well as on 1.93 — so ruling 4's arithmetic survived
a compiler version bump on this target, which is the kind of property only
ever noticed when it fails.
