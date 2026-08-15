//! The corpus sub-commands' command lines.

use std::path::Path;

use crate::lock;

/// `cargo xtask corpus-licences` — plan 14's table, generated from the lock.
///
/// `--check` compares it against the committed `corpus/README.md` and fails on
/// a difference, so a corpus cannot be added without its licence reaching the
/// file a person reads.
pub fn licences(root: &Path, args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");
    let corpora = lock::read(root)?;
    let table = lock::licence_table(&corpora);

    if !check {
        print!("{table}");
        return Ok(());
    }

    let readme_path = root.join("corpus/README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .map_err(|e| format!("{}: {e}", readme_path.display()))?;
    let missing: Vec<&str> = table
        .lines()
        .filter(|line| !readme.contains(*line))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "corpus/README.md has drifted from the lock. It is missing:\n{}\n\
         Run `cargo xtask corpus-licences` and paste the table in.",
        missing.join("\n")
    ))
}
