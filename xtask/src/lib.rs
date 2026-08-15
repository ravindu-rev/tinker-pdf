//! The corpus machinery, as a library so that it can be tested.
//!
//! `xtask` was a binary and nothing else, which was right while its chores
//! were three checks over the repository's own files. The corpus work is not
//! that: it grows a lock parser, a runner and a ratchet, each of which is a
//! thing to be tested rather than a thing to be run and looked at. A binary
//! cannot be imported by a test, so there is a library.
//!
//! `main.rs` keeps the three checks — they have no such need — and dispatches
//! to this.

use std::path::{Path, PathBuf};

pub mod corpus;
pub mod lock;

/// The repository root, from the manifest directory of whichever of the two
/// targets is asking.
pub fn repo_root() -> PathBuf {
    // The manifest directory is `<root>/xtask`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
