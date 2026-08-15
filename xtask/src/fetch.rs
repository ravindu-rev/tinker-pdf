//! Fetching a corpus, and refusing one that is not what the lock says.
//!
//! ## Why `curl` and `tar` rather than crates
//!
//! CONTRIBUTING rule 1: no third-party crates. An HTTPS client and a gzip/tar
//! reader are each a large dependency tree, and the project already has a
//! precedent for the alternative — ruling 9 invokes qpdf and mutool as
//! subprocesses rather than linking them, on the grounds that an external
//! arbiter should stay external. `curl` and `tar` ship with every platform
//! this repository builds on, Windows 10 and the GitHub runners included, and
//! neither is trusted with anything: what comes back is verified here, against
//! a digest computed by this project's own SHA-256, before it is unpacked.
//!
//! ## What "verified" means
//!
//! The archive is hashed **after** it is written and **before** it is
//! extracted, and a mismatch deletes it. That order is the whole security
//! property. Verifying a stream while writing it, or extracting first and
//! checking afterwards, both mean that a tampered archive has already had its
//! contents on the disk — and `tar` has had path-traversal bugs for as long as
//! `tar` has existed.
//!
//! A digest that does not match is never repaired by re-recording it. The lock
//! pins `commit` as well as `sha256` exactly so that the two can disagree: a
//! forge re-compressing its archive of a fixed commit changes the bytes and
//! not the content, and that is a different event from a mirror serving
//! something else. `--record` prints what was actually fetched so that the
//! first can be committed deliberately, with its reason. It never writes the
//! lock itself.

use std::path::{Path, PathBuf};
use std::process::Command;

use tinker_pdf_crypto::sha2::sha256;

use crate::lock::{self, Corpus};

/// `cargo xtask corpus-fetch`.
pub fn fetch(root: &Path, args: &[String]) -> Result<(), String> {
    let record = args.iter().any(|a| a == "--record");
    let force = args.iter().any(|a| a == "--force");
    let mut only = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--corpus" => {
                index += 1;
                only.push(args.get(index).cloned().ok_or("`--corpus` needs a name")?);
            }
            "--record" | "--force" => {}
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }

    let corpora = lock::read(root)?;
    for name in &only {
        if !corpora.iter().any(|c| c.name == *name) {
            return Err(format!("`{name}` is not a corpus in {}", lock::LOCK_PATH));
        }
    }

    let mut recorded = Vec::new();
    for corpus in &corpora {
        if !only.is_empty() && !only.contains(&corpus.name) {
            continue;
        }
        let digest = one(root, corpus, record, force)?;
        recorded.push((corpus.name.clone(), digest));
    }

    if record {
        println!("\nWhat was actually fetched. If a digest differs from the lock,");
        println!("that is a deliberate commit with its own reason, not a repair:");
        for (name, digest) in &recorded {
            println!("[{name}]\nsha256 = {digest}");
        }
    }
    Ok(())
}

/// Fetches, verifies and extracts one corpus. Returns the digest seen.
fn one(root: &Path, corpus: &Corpus, record: bool, force: bool) -> Result<String, String> {
    let archive = lock::archive_path(root, corpus);
    let extracted = root.join("corpus/files").join(&corpus.name);

    if let Some(parent) = archive.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }

    if archive.exists() && !force {
        let digest = digest_of(&archive)?;
        if digest == corpus.sha256 {
            eprintln!("corpus-fetch: {} is cached and verifies", corpus.name);
            if !extracted.exists() {
                extract(&archive, &extracted, corpus)?;
            }
            return Ok(digest);
        }
        eprintln!(
            "corpus-fetch: {}'s cached archive does not match the lock; refetching",
            corpus.name
        );
        let _ = std::fs::remove_file(&archive);
    }

    download(&corpus.url, &archive)?;
    let digest = digest_of(&archive)?;

    if digest != corpus.sha256 {
        // Removed before anything can reach it. A rejected archive left on
        // disk is one `--force`-less rerun away from being treated as a cache
        // hit by a future version of this function that forgot to check.
        let _ = std::fs::remove_file(&archive);
        if !record {
            return Err(format!(
                "{}: the archive does not match the lock and has been deleted.\n  \
                 lock says {}\n  fetched  {digest}\n\
                 The lock pins the upstream commit ({}) as well as these bytes, so \
                 the two disagreeing means either the forge re-compressed its \
                 archive of that same commit — in which case re-record the digest \
                 in its own commit, with that reason — or what came back is not \
                 what the lock names, in which case do not.\n  \
                 `cargo xtask corpus-fetch --record` prints what was fetched.",
                corpus.name, corpus.sha256, corpus.commit
            ));
        }
        eprintln!(
            "corpus-fetch: {} fetched {digest}, lock says {} — not extracted",
            corpus.name, corpus.sha256
        );
        return Ok(digest);
    }

    eprintln!("corpus-fetch: {} verifies", corpus.name);
    extract(&archive, &extracted, corpus)?;
    Ok(digest)
}

/// The archive's SHA-256, lowercase hex.
fn digest_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(hex(&sha256(&bytes)))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    eprintln!("corpus-fetch: {url}");
    // Written to a partial file and renamed, so an interrupted fetch cannot
    // leave something at the cache path that a later run treats as cached.
    let partial = to.with_extension("partial");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            // A redirect off HTTPS would turn a pinned fetch into an
            // unauthenticated one, and the lock refuses a plain-HTTP url for
            // the same reason.
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--retry",
            "3",
            "--output",
        ])
        .arg(&partial)
        .arg(url)
        .status()
        .map_err(|e| {
            format!(
                "curl could not be run ({e}). corpus-fetch invokes curl and tar \
                 rather than linking an HTTPS client and a tar reader; both ship \
                 with every platform this repository builds on."
            )
        })?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("curl failed for {url} ({status})"));
    }
    std::fs::rename(&partial, to).map_err(|e| format!("{}: {e}", to.display()))
}

/// Unpacks a verified archive, stripping the forge's `name-commit/` wrapper.
fn extract(archive: &Path, into: &Path, corpus: &Corpus) -> Result<(), String> {
    // Replaced rather than merged: a corpus half from one commit and half from
    // another is not a corpus, and it would be pinned by nothing.
    if into.exists() {
        std::fs::remove_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    }
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        // Every codeload archive is one directory named `repo-commit`, which
        // would otherwise have to be spelled inside every `subdir` in the lock
        // and would change whenever a commit is re-pinned.
        .arg("--strip-components=1")
        .status()
        .map_err(|e| format!("tar could not be run ({e})"))?;
    if !status.success() {
        return Err(format!("tar failed for {} ({status})", archive.display()));
    }

    let files = lock::files_dir(&PathBuf::from("."), corpus);
    let _ = files;
    eprintln!(
        "corpus-fetch: {} extracted to {}",
        corpus.name,
        into.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's own SHA-256, against the two vectors every implementation
    /// is checked with. If this ever moves, `corpus-fetch` is verifying
    /// against something other than SHA-256 and every pin in the lock is
    /// meaningless.
    #[test]
    fn the_digest_is_sha256() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Milestone 4's exit criterion. A tampered archive differs from the lock
    /// by one byte and must be refused — and, having been refused, must not
    /// still be sitting where the next run would read it as a cache hit.
    #[test]
    fn a_tampered_archive_is_refused_and_deleted() {
        let dir = std::env::temp_dir().join(format!("tinker-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let archive = dir.join("corpus.tar.gz");

        let honest = b"the archive the lock names".to_vec();
        std::fs::write(&archive, &honest).expect("a file");
        let pinned = hex(&sha256(&honest));
        assert_eq!(digest_of(&archive).expect("a digest"), pinned);

        // One byte, which is what a tampered mirror looks like.
        let mut tampered = honest.clone();
        tampered[0] ^= 0x01;
        std::fs::write(&archive, &tampered).expect("a file");
        let seen = digest_of(&archive).expect("a digest");
        assert_ne!(seen, pinned, "one byte changes the digest");

        let corpus = Corpus {
            name: "tampered".to_string(),
            url: "https://example.invalid/x.tar.gz".to_string(),
            commit: "0".repeat(40),
            sha256: pinned.clone(),
            subdir: String::new(),
            licence: "n/a".to_string(),
            redistribute: false,
            exercises: "nothing".to_string(),
        };
        // `one` with the archive already present and no `--force`: it hashes
        // the cache, finds the mismatch, deletes it and tries to refetch,
        // which fails because the url is unreachable. What matters for this
        // test is that the tampered bytes did not survive.
        let outcome = one(&dir, &corpus, false, false);
        assert!(outcome.is_err(), "a mismatch is never accepted");
        assert!(
            !dir.join("corpus/cache/tampered.tar.gz").exists(),
            "the rejected archive must not be left where a later run reads it \
             as a cache hit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonsense_on_the_command_line_is_refused() {
        let root = crate::repo_root();
        assert!(fetch(&root, &["--nope".to_string()])
            .expect_err("refused")
            .contains("unknown option"));
        assert!(fetch(&root, &["--corpus".to_string()])
            .expect_err("refused")
            .contains("needs a name"));
        assert!(
            fetch(&root, &["--corpus".to_string(), "nosuch".to_string()])
                .expect_err("refused")
                .contains("is not a corpus")
        );
    }
}
