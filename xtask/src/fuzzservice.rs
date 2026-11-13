//! What a continuous-fuzzing service's build wrote, checked here.
//!
//! `fuzz/service/build.sh` is the build a public fuzzing service runs
//! (`docs/design/continuous-fuzzing.md`): for every target in
//! `fuzz/fuzz_targets/` it writes the binary, a seed archive packed from
//! `fuzz/corpus/<target>/` and an options file into one output directory.
//! The service's own tooling checks such a directory by running the targets,
//! and ruling 13 lets no outside program decide anything here, so this is the
//! check in its place: `cargo xtask fuzz --service-out DIR` reads the
//! directory and names every target whose binary is missing or is not an ELF
//! file, whose seed archive does not read back through `tinker_pdf_zip` to
//! exactly the files of its corpus, byte for byte, or whose options file does
//! not hold the local sessions' timeout. It reads files and spawns nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::fuzzaudit;

/// The per-input timeout of the local fuzz sessions, in seconds: the
/// `-timeout=20` `docs/verification.md` runs every session with. The service
/// build writes it into every target's options file, so an input the service
/// reports as a timeout is one a session here times out on too.
pub const SESSION_TIMEOUT_SECONDS: u32 = 20;

/// The four bytes every ELF file starts with (`EI_MAG0` to `EI_MAG3`).
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

/// The seed archive's name beside a target's binary, as the service reads it.
#[must_use]
pub fn seed_archive(name: &str) -> String {
    format!("{name}_seed_corpus.zip")
}

/// The options file's name beside a target's binary, as the service reads it.
#[must_use]
pub fn options_file(name: &str) -> String {
    format!("{name}.options")
}

/// `cargo xtask fuzz --service-out DIR`.
pub fn run(root: &Path, args: &[String]) -> Result<(), Vec<String>> {
    let out = match args {
        [flag, dir] if flag == "--service-out" => PathBuf::from(dir),
        _ => {
            return Err(vec![
                "usage: cargo xtask fuzz [--service-out DIR]".to_string()
            ])
        }
    };
    check(root, &out)?;
    println!(
        "fuzz: {} holds every target's binary, seeds and options",
        out.display()
    );
    Ok(())
}

/// Every problem with the service build in `out`, for the targets of the
/// checkout at `root`.
pub fn check(root: &Path, out: &Path) -> Result<(), Vec<String>> {
    let roster = fuzzaudit::roster(root).map_err(|e| vec![e])?;
    if roster.is_empty() {
        return Err(vec![
            "fuzz/fuzz_targets holds no target, so there is nothing to check".to_string(),
        ]);
    }
    if !out.is_dir() {
        return Err(vec![format!(
            "{}: not a directory, so no build output was read",
            out.display()
        )]);
    }
    let mut problems = Vec::new();
    for name in &roster {
        problems.extend(binary(out, name));
        problems.extend(seeds(root, out, name));
        problems.extend(options(out, name));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The target's binary: there, and an ELF file.
fn binary(out: &Path, name: &str) -> Option<String> {
    let path = out.join(name);
    match std::fs::read(&path) {
        Err(_) => Some(format!("{name}: no binary at {}", path.display())),
        Ok(bytes) if !bytes.starts_with(ELF_MAGIC) => Some(format!(
            "{name}: {} is not an ELF file, so the service has no target to run",
            path.display()
        )),
        Ok(_) => None,
    }
}

/// The target's seed archive: it opens, and its files are exactly the
/// corpus's, by name and byte for byte.
fn seeds(root: &Path, out: &Path, name: &str) -> Vec<String> {
    let path = out.join(seed_archive(name));
    let Ok(bytes) = std::fs::read(&path) else {
        return vec![format!("{name}: no seed archive at {}", path.display())];
    };
    let mut archive = match tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
    {
        Ok(archive) => archive,
        Err(e) => {
            return vec![format!(
                "{name}: {} does not open as a ZIP archive ({e})",
                path.display()
            )]
        }
    };

    let mut problems = Vec::new();
    let mut packed: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let entries: Vec<(usize, String, bool)> = archive
        .entries()
        .iter()
        .map(|entry| (entry.index, entry.name.clone(), entry.is_directory()))
        .collect();
    for (index, entry, directory) in entries {
        if directory {
            continue;
        }
        match archive.read(index) {
            Ok(data) => {
                if packed.insert(entry.clone(), data.into_owned()).is_some() {
                    problems.push(format!("{name}: the seed archive holds `{entry}` twice"));
                }
            }
            Err(e) => problems.push(format!(
                "{name}: the seed archive's `{entry}` does not read back ({e:?})"
            )),
        }
    }

    let corpus = root.join("fuzz/corpus").join(name);
    let mut held: BTreeMap<String, PathBuf> = BTreeMap::new();
    for file in fuzzaudit::seeds_in(&corpus) {
        if let Some(seed) = file.file_name().and_then(|n| n.to_str()) {
            held.insert(seed.to_string(), file.clone());
        }
    }
    for (seed, file) in &held {
        match (packed.get(seed), std::fs::read(file)) {
            (None, _) => problems.push(format!(
                "{name}: the seed archive lacks `{seed}`, which fuzz/corpus/{name}/ holds"
            )),
            (Some(_), Err(e)) => problems.push(format!(
                "{name}: fuzz/corpus/{name}/{seed} cannot be read ({e})"
            )),
            (Some(data), Ok(seeded)) if *data != seeded => problems.push(format!(
                "{name}: the seed archive's `{seed}` is not the bytes of fuzz/corpus/{name}/{seed}"
            )),
            (Some(_), Ok(_)) => {}
        }
    }
    for entry in packed.keys() {
        if !held.contains_key(entry) {
            problems.push(format!(
                "{name}: the seed archive holds `{entry}`, which fuzz/corpus/{name}/ does not"
            ));
        }
    }
    problems
}

/// The target's options file: a `[libfuzzer]` section setting `timeout` to
/// the local sessions' [`SESSION_TIMEOUT_SECONDS`].
fn options(out: &Path, name: &str) -> Option<String> {
    let path = out.join(options_file(name));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Some(format!("{name}: no options file at {}", path.display()));
    };
    if libfuzzer_timeout(&text) == Some(SESSION_TIMEOUT_SECONDS) {
        None
    } else {
        Some(format!(
            "{name}: {} sets no `timeout = {SESSION_TIMEOUT_SECONDS}` in a [libfuzzer] \
             section, the local sessions' -timeout={SESSION_TIMEOUT_SECONDS}",
            path.display()
        ))
    }
}

/// The `timeout` an options file's `[libfuzzer]` section sets, if it sets
/// one that is a whole number of seconds.
///
/// The file is INI as the service reads it: `[section]` headers, `key =
/// value` or `key: value` lines, keys in any case, `#` and `;` comment lines.
/// The last `timeout` in the section is the one that holds.
#[must_use]
pub fn libfuzzer_timeout(text: &str) -> Option<u32> {
    let mut section = String::new();
    let mut timeout = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = header.trim().to_string();
            continue;
        }
        if section != "libfuzzer" {
            continue;
        }
        let Some((key, value)) = line.split_once(['=', ':']) else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("timeout") {
            timeout = value.trim().parse::<u32>().ok();
        }
    }
    timeout
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZIP of stored entries, each with the CRC-32 the reader checks.
    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        fn u16le(out: &mut Vec<u8>, value: usize) {
            out.extend_from_slice(&u16::try_from(value).expect("fits").to_le_bytes());
        }
        fn u32le(out: &mut Vec<u8>, value: usize) {
            out.extend_from_slice(&u32::try_from(value).expect("fits").to_le_bytes());
        }
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let offset = out.len();
            let crc = tinker_pdf_filters::crc32(data) as usize;
            // The local file header (APPNOTE 4.3.7): stored, UTF-8 names.
            out.extend_from_slice(b"PK\x03\x04");
            u16le(&mut out, 20);
            u16le(&mut out, 1 << 11);
            u16le(&mut out, 0);
            u16le(&mut out, 0);
            u16le(&mut out, 0x21);
            u32le(&mut out, crc);
            u32le(&mut out, data.len());
            u32le(&mut out, data.len());
            u16le(&mut out, name.len());
            u16le(&mut out, 0);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            // Its central directory header (4.3.12).
            central.extend_from_slice(b"PK\x01\x02");
            u16le(&mut central, 20);
            u16le(&mut central, 20);
            u16le(&mut central, 1 << 11);
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u16le(&mut central, 0x21);
            u32le(&mut central, crc);
            u32le(&mut central, data.len());
            u32le(&mut central, data.len());
            u16le(&mut central, name.len());
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u16le(&mut central, 0);
            u32le(&mut central, 0);
            u32le(&mut central, offset);
            central.extend_from_slice(name.as_bytes());
        }
        let directory = out.len();
        out.extend_from_slice(&central);
        // The end of central directory record (4.3.16).
        out.extend_from_slice(b"PK\x05\x06");
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, files.len());
        u16le(&mut out, files.len());
        u32le(&mut out, central.len());
        u32le(&mut out, directory);
        u16le(&mut out, 0);
        out
    }

    /// A checkout of two targets, `alpha` with two seeds and `beta` with one,
    /// and a service build of it that is complete, under a root of its own
    /// named for `tag` and this process, so two checkouts testing at once
    /// never share one.
    struct Staged {
        root: PathBuf,
        out: PathBuf,
    }

    const SEEDS: &[(&str, &str, &[u8])] = &[
        ("alpha", "first", b"%PDF-1.7 the first seed"),
        ("alpha", "second.bin", b"\x00\x01\x02\xff"),
        ("beta", "only", b"<svg/>"),
    ];

    impl Staged {
        fn new(tag: &str) -> Staged {
            let root = std::env::temp_dir()
                .join(format!("tinker-fuzz-service-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let out = root.join("out");
            std::fs::create_dir_all(root.join("fuzz/fuzz_targets")).expect("targets");
            std::fs::create_dir_all(&out).expect("out");
            for name in ["alpha", "beta"] {
                std::fs::write(
                    root.join(format!("fuzz/fuzz_targets/{name}.rs")),
                    "#![no_main]\n",
                )
                .expect("target");
                std::fs::create_dir_all(root.join("fuzz/corpus").join(name)).expect("corpus");
                let mut binary = ELF_MAGIC.to_vec();
                binary.extend_from_slice(b"\x02\x01\x01 the rest of a binary");
                std::fs::write(out.join(name), binary).expect("binary");
                std::fs::write(
                    out.join(options_file(name)),
                    format!("[libfuzzer]\ntimeout = {SESSION_TIMEOUT_SECONDS}\n"),
                )
                .expect("options");
            }
            for (target, seed, data) in SEEDS {
                std::fs::write(root.join("fuzz/corpus").join(target).join(seed), data)
                    .expect("seed");
            }
            let staged = Staged { root, out };
            for name in ["alpha", "beta"] {
                let files: Vec<(&str, &[u8])> = SEEDS
                    .iter()
                    .filter(|(target, _, _)| *target == name)
                    .map(|(_, seed, data)| (*seed, *data))
                    .collect();
                staged.archive(name, &files);
            }
            staged
        }

        fn archive(&self, name: &str, files: &[(&str, &[u8])]) {
            std::fs::write(self.out.join(seed_archive(name)), zip_of(files)).expect("archive");
        }

        fn problems(&self) -> Vec<String> {
            check(&self.root, &self.out).err().unwrap_or_default()
        }
    }

    impl Drop for Staged {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// The one problem `problems` holds, which must name `target`.
    fn only_problem(problems: &[String], target: &str, says: &str) {
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(
            problems[0].starts_with(&format!("{target}: ")),
            "{problems:#?}"
        );
        assert!(problems[0].contains(says), "{problems:#?}");
    }

    #[test]
    fn a_complete_service_output_passes() {
        let staged = Staged::new("complete");
        assert_eq!(staged.problems(), Vec::<String>::new());
    }

    #[test]
    fn a_target_without_its_binary_is_named() {
        let staged = Staged::new("no-binary");
        std::fs::remove_file(staged.out.join("beta")).expect("removed");
        only_problem(&staged.problems(), "beta", "no binary");
    }

    #[test]
    fn a_binary_that_is_not_elf_is_named() {
        let staged = Staged::new("not-elf");
        // A Windows executable's `MZ`, which is the build this would catch.
        std::fs::write(staged.out.join("alpha"), b"MZ\x90\x00 not an ELF file").expect("written");
        only_problem(&staged.problems(), "alpha", "not an ELF file");
    }

    #[test]
    fn a_seed_archive_missing_a_seed_is_named() {
        let staged = Staged::new("missing-seed");
        staged.archive("alpha", &[("first", b"%PDF-1.7 the first seed")]);
        only_problem(&staged.problems(), "alpha", "lacks `second.bin`");
    }

    #[test]
    fn a_seed_archive_with_a_stray_entry_is_named() {
        let staged = Staged::new("stray-entry");
        staged.archive("beta", &[("only", b"<svg/>"), ("stray", b"not a seed")]);
        only_problem(&staged.problems(), "beta", "holds `stray`");
    }

    #[test]
    fn a_seed_archive_with_a_changed_byte_is_named() {
        let staged = Staged::new("changed-byte");
        staged.archive(
            "alpha",
            &[
                ("first", b"%PDF-1.7 the first seed"),
                ("second.bin", b"\x00\x01\x02\xfe"),
            ],
        );
        only_problem(&staged.problems(), "alpha", "`second.bin` is not the bytes");
    }

    #[test]
    fn an_options_file_without_the_session_timeout_is_named() {
        let staged = Staged::new("no-timeout");
        for text in [
            "[libfuzzer]\nmax_len = 4096\n".to_string(),
            "[libfuzzer]\ntimeout = 25\n".to_string(),
            format!("[asan]\ntimeout = {SESSION_TIMEOUT_SECONDS}\n"),
            String::new(),
        ] {
            std::fs::write(staged.out.join(options_file("beta")), &text).expect("written");
            only_problem(&staged.problems(), "beta", "sets no `timeout = 20`");
        }
        std::fs::remove_file(staged.out.join(options_file("beta"))).expect("removed");
        only_problem(&staged.problems(), "beta", "no options file");
    }

    #[test]
    fn a_seed_archive_that_is_not_a_zip_is_named() {
        let staged = Staged::new("not-a-zip");
        std::fs::write(staged.out.join(seed_archive("beta")), b"only some bytes").expect("w");
        only_problem(&staged.problems(), "beta", "does not open as a ZIP archive");
        std::fs::remove_file(staged.out.join(seed_archive("beta"))).expect("removed");
        only_problem(&staged.problems(), "beta", "no seed archive");
    }

    #[test]
    fn the_options_file_is_read_as_ini() {
        assert_eq!(libfuzzer_timeout("[libfuzzer]\ntimeout = 20\n"), Some(20));
        assert_eq!(
            libfuzzer_timeout("# a comment\n[libfuzzer]\n; another\nmax_len = 1\nTimeout: 20\n"),
            Some(20)
        );
        assert_eq!(
            libfuzzer_timeout("[libfuzzer]\ntimeout = 20\ntimeout = 25\n"),
            Some(25)
        );
        assert_eq!(libfuzzer_timeout("[libfuzzer]\ntimeout = twenty\n"), None);
        assert_eq!(
            libfuzzer_timeout("[libfuzzer]\n[afl]\ntimeout = 20\n"),
            None
        );
    }
}
