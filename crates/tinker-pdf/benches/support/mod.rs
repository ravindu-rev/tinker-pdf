//! Wall-clock benchmarks, deliberately outside the test suite: the harness
//! every group file beside this one shares.
//!
//! Clocks are banned from assertions in this repository — `bounds_ledger.rs`
//! bans `Instant::now` from itself, so that every bound is a property of an
//! input rather than of a machine. The cost of that rule is that every
//! performance number in `docs/` has been a one-time measurement with a date
//! beside it and nothing to stop it regressing.
//!
//! This is where the clocks live instead. Criterion is exempt tooling by name
//! (`Cargo.toml`'s header rule, `CONTRIBUTING.md`): it measures the engine and
//! is not part of it, nothing here ships, and `default-features = false` keeps
//! its plotting stack out of the tree. `cargo bench` runs on a schedule rather
//! than on the path between a push and a review, because a benchmark that
//! gates a pull request measures the runner it happened to land on.
//!
//! Regression comparison is the point, and it is criterion's to do:
//!
//! ```sh
//! cargo bench -p tinker-pdf -- --save-baseline before
//! # ... change something ...
//! cargo bench -p tinker-pdf -- --baseline before
//! ```
//!
//! # One file a group
//!
//! Each group of the benchmark set (`docs/design/performance.md`, MS-04) is
//! one file, `benches/<group>.rs`, and one `[[bench]]` in `Cargo.toml`, so
//! `cargo bench -p tinker-pdf --bench render` measures one group and
//! `bench.yml` runs one job a group. A group file opens one criterion group,
//! named for its group, so every id is `<group>/<name>`, and declares what
//! each iteration processes through criterion's `Throughput` ([`Per`]).
//! `baseline.json` lists the same names in the same order, each with its unit,
//! what an iteration covers (`per_iteration`) and how many calls an iteration
//! makes (`calls`). `cargo xtask baseline` holds the `[[bench]]` entries, each
//! file's names, its list and `bench.yml`'s count for the group to each other.
//!
//! # Names
//!
//! Criterion 0.5.1 records an operation under a directory made of its group
//! and its name, and makes each part filename-safe: it replaces `?`, `"`, `/`,
//! `\`, `*`, `<`, `>`, `:`, `|` and `^` with `_` and cuts the part at 64
//! bytes. A name either rule changes is recorded under a key the baseline does
//! not hold, reported as new and never held to its band. So every name is
//! printable ASCII, at most 64 bytes and free of those ten characters
//! (`PDF-A`, not `PDF/A`), and `cargo xtask baseline` refuses one that is not.
//!
//! # The seven
//!
//! Seven operations, chosen because they are the ones the docs quote numbers
//! for: opening, rendering text, rendering a shading, extracting text, filling
//! anti-aliased paths, rewriting a document, and paginating a book. They were
//! one file, `benches/engine.rs`, until MS-04.2 moved each into its group's
//! file with its input and its call unchanged, the inputs into [`inputs`];
//! `baseline.json` keeps their figures under their group ids. The fill is
//! newer than the rest and was added for a reason worth keeping: **six of
//! these do not enter the scanline rasteriser**, so the roadmap's
//! vectorisation row — whose exit criterion named "the six benchmarks" — was
//! unmeasurable as written. See [`inputs::triangles`].
//!
//! # Why the allow
//!
//! Every group file includes this module and compiles its own copy, and each
//! uses a different subset of it — the open group needs no render, the render
//! group no committed book — so each reports the others' as dead.

#![allow(
    dead_code,
    reason = "shared by every group file; each uses a different subset"
)]

pub mod inputs;

use std::time::Duration;

use criterion::{Criterion, Throughput};
use tinker_pdf::Bitmap;

/// The sampling of every group but sign, fonts and scripts: ten samples, the
/// fewest criterion takes, after a second's warm-up, over three seconds.
///
/// Set in `criterion_group!`'s `config` and never on a group, because
/// criterion's command line overrides a config and a group's own settings
/// override the command line, so a run can sample differently without
/// editing a file. Flat sampling is the exception: criterion sets it only on
/// a group and no flag sets it, so each group file sets it on its group.
pub fn config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
}

/// The sampling of the sign, fonts and scripts groups: thirty samples over
/// [`config`]'s times. Their operations are sized near 25 ms by their
/// targets, and at 100 ms or less thirty samples fit the three seconds ten
/// would take, so the closer reading costs nothing.
pub fn fine_config() -> Criterion {
    config().sample_size(30)
}

/// What one iteration of an operation processes, in the unit its list in
/// `baseline.json` gives it.
///
/// An operation whose input is none of these — a private-key operation, a
/// shaped paragraph — declares nothing and reports milliseconds alone.
#[derive(Clone, Copy, Debug)]
pub enum Per {
    /// Pages read, written or laid out.
    Pages(u64),
    /// Pixels: a render's bitmap's, or a decoded image's.
    Pixels(u64),
    /// Bytes, reported in decimal megabytes.
    Bytes(u64),
    /// Glyph outlines.
    Outlines(u64),
}

impl Per {
    /// The figure as criterion reports and records it.
    pub fn throughput(self) -> Throughput {
        match self {
            Per::Pages(n) | Per::Pixels(n) | Per::Outlines(n) => Throughput::Elements(n),
            Per::Bytes(n) => Throughput::BytesDecimal(n),
        }
    }
}

/// A render's unit: the pixels of the bitmap it draws.
pub fn pixels(bitmap: &Bitmap) -> Per {
    Per::Pixels(u64::from(bitmap.width) * u64::from(bitmap.height))
}
