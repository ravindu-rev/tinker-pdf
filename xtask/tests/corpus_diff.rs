//! `cargo xtask corpus-diff --held-to-broke` is read by `corpus.yml` row by
//! row, each row a corpus, a path and the file on disk, so what it prints on
//! standard output is those rows and nothing else.

use std::path::PathBuf;
use std::process::Command;

/// A per-file report with one `pdfjs` file whose `dpi` relation ended as
/// `verdict`.
fn report(verdict: &str) -> String {
    format!(
        r#"{{"schema": 2, "complete": true, "limits": [],
           "settings": {{"timeout_seconds": 180, "dpi": 72, "fonts": "none", "jobs": 4}},
           "corpora": [{{"name": "pdfjs", "files": [
             {{"path": "a.pdf", "outcome": "passed",
               "metamorphic": {{"dpi": {{"verdict": "{verdict}", "detail": ""}}}}}},
             {{"path": "b.pdf", "outcome": "passed",
               "metamorphic": {{"dpi": {{"verdict": "held", "detail": ""}}}}}}]}}]}}"#
    )
}

/// A directory of this test's own, so a parallel run writes nowhere another
/// reads.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn held_to_broke_prints_its_rows_and_nothing_after_them() {
    let dir = scratch("corpus-diff");
    let before = dir.join("before.json");
    let after = dir.join("after.json");
    std::fs::write(&before, report("held")).expect("the earlier report");
    std::fs::write(&after, report("broke")).expect("the later report");
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("corpus-diff")
        .arg(&before)
        .arg(&after)
        .args(["--held-to-broke", "dpi"])
        .output()
        .expect("xtask runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let rows: Vec<Vec<&str>> = stdout
        .lines()
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "one file went from held to broke: {stdout:?}"
    );
    assert_eq!(rows[0].len(), 3, "a corpus, a path and a file: {stdout:?}");
    assert_eq!(rows[0][..2], ["pdfjs", "a.pdf"]);
    assert!(rows[0][2].ends_with("a.pdf"), "{stdout:?}");
}
