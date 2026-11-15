//! The paths a release build would write into its artefact, remapped, so
//! the artefact names no machine.
//!
//! # What writes a path
//!
//! A release build carries no debug information, but every panic location
//! is a source file's name, kept for the message, and rustc writes the name
//! as the file reached it:
//!
//! - **The engine's crates**, by their absolute paths when `bindings/js` or
//!   `bindings/python` builds them. Each binding is a workspace root of its
//!   own, so the engine is a path dependency outside it, and cargo hands
//!   rustc a file outside the building workspace by its full path. Code
//!   generated into `OUT_DIR` is named under the target directory, which a
//!   release build keeps in the checkout.
//! - **Every crates.io dependency**, under `CARGO_HOME`'s `registry/src`.
//! - **The standard library's generic code** that the build instantiates:
//!   rustc names its files `/rustc/<commit>/library/...`, unless the
//!   toolchain carries the `rust-src` component, and then by the
//!   component's own path under the toolchain's `lib/rustlib/src/rust`.
//!
//! # What each becomes
//!
//! `--remap-path-prefix`, a stable flag, rewrites each: the checkout's root
//! to nothing, so an engine file reads `crates/<crate>/src/<file>.rs` as the
//! root workspace's own builds name it; `CARGO_HOME` to `/cargo`; and the
//! toolchain's `lib/rustlib/src/rust` to `/rustc/<commit>/`, the name the
//! standard library carries without `rust-src`, so a toolchain with the
//! component builds what one without it builds. The trailing `/` is what
//! keeps that name whole on Windows, where rustc would otherwise join the
//! rest of the path on with a `\`. rustc applies the last flag whose prefix
//! matches, so the flags run from the shortest prefix to the longest, and a
//! `CARGO_HOME` inside the checkout keeps its own name.
//!
//! # What a remap does not reach
//!
//! cargo hashes each package's source into the `-C metadata` that names its
//! symbols, and it hashes a path dependency outside the building workspace by
//! its absolute path. Symbol names order the code and data the build lays
//! out, so the wasm and the wheels, built from their bindings' workspaces,
//! still differ from one checkout's location to another's with every path
//! remapped. The root workspace's own builds, the C ABI's among them, hash
//! their members by their paths relative to the root, and do not.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One `--remap-path-prefix`: the prefix a build would write, and what it
/// writes instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remap {
    /// The machine's path.
    pub from: PathBuf,
    /// What the artefact carries in its place.
    pub to: String,
}

impl Remap {
    /// The flag, as rustc takes it.
    pub fn flag(&self) -> String {
        format!("--remap-path-prefix={}={}", self.from.display(), self.to)
    }
}

/// The remaps for a build of the checkout at `root`, with cargo's home at
/// `cargo_home` and the toolchain at `sysroot`, whose rustc was built from
/// `commit`: shortest prefix first, since rustc applies the last that
/// matches.
pub fn remaps(root: &Path, cargo_home: &Path, sysroot: &Path, commit: &str) -> Vec<Remap> {
    let sources = sysroot.join("lib").join("rustlib").join("src").join("rust");
    let mut remaps = vec![
        Remap {
            from: root.to_path_buf(),
            to: String::new(),
        },
        Remap {
            from: cargo_home.to_path_buf(),
            to: "/cargo".to_string(),
        },
        Remap {
            from: sources,
            to: format!("/rustc/{commit}/"),
        },
    ];
    remaps.sort_by_key(|remap| remap.from.components().count());
    remaps
}

/// What a build on this machine starts from: cargo's home, from
/// `CARGO_HOME` or the user's home, and the toolchain's sysroot and commit,
/// from the `rustc` cargo would run.
struct Machine {
    cargo_home: PathBuf,
    sysroot: PathBuf,
    commit: String,
}

fn machine() -> Result<Machine, String> {
    let cargo_home = cargo_home()?;
    let sysroot = PathBuf::from(rustc(&["--print", "sysroot"])?.trim());
    let version = rustc(&["-vV"])?;
    let commit = version
        .lines()
        .find_map(|line| line.strip_prefix("commit-hash:"))
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .ok_or_else(|| format!("`rustc -vV` names no commit-hash:\n{version}"))?
        .to_string();
    Ok(Machine {
        cargo_home,
        sysroot,
        commit,
    })
}

/// This machine's remaps for the checkout at `root`.
///
/// # Errors
///
/// When cargo's home cannot be found or `rustc` does not answer.
pub fn for_this_machine(root: &Path) -> Result<Vec<Remap>, String> {
    let machine = machine()?;
    Ok(remaps(
        root,
        &machine.cargo_home,
        &machine.sysroot,
        &machine.commit,
    ))
}

/// `remaps`, refused when a prefix holds whitespace: `RUSTFLAGS` cannot
/// carry it, since cargo splits the variable there.
///
/// # Errors
///
/// The first prefix with whitespace in it.
pub fn carried(remaps: Vec<Remap>) -> Result<Vec<Remap>, String> {
    match remaps.iter().find(|remap| {
        remap
            .from
            .to_string_lossy()
            .chars()
            .any(char::is_whitespace)
    }) {
        Some(remap) => Err(format!(
            "{} holds whitespace, which RUSTFLAGS cannot carry: cargo splits it there",
            remap.from.display()
        )),
        None => Ok(remaps),
    }
}

/// The variable and the value that hand `flags` to cargo beside the flags a
/// process was given, `encoded` its `CARGO_ENCODED_RUSTFLAGS` and `plain` its
/// `RUSTFLAGS`: the encoded form when that is set, since cargo then reads no
/// other, or when a flag holds whitespace, which only it can carry; the plain
/// one otherwise.
pub fn joined(
    encoded: Option<&str>,
    plain: Option<&str>,
    flags: &[String],
) -> (&'static str, String) {
    let mut all: Vec<&str> = match encoded {
        Some(encoded) => encoded
            .split('\x1f')
            .filter(|flag| !flag.is_empty())
            .collect(),
        None => plain.unwrap_or_default().split_whitespace().collect(),
    };
    all.extend(flags.iter().map(String::as_str));
    if encoded.is_some()
        || flags
            .iter()
            .any(|flag| flag.chars().any(char::is_whitespace))
    {
        ("CARGO_ENCODED_RUSTFLAGS", all.join("\x1f"))
    } else {
        ("RUSTFLAGS", all.join(" "))
    }
}

/// The paths of this machine an artefact must not carry: the three a build
/// writes, the checkout, cargo's home and the toolchain, the toolchain's
/// whole sysroot rather than its sources alone.
///
/// Not the user's home: the standard library a toolchain ships was built on
/// a hosted runner too, with that runner's home, and C code compiled into it
/// can name its own sources there.
///
/// # Errors
///
/// When cargo's home cannot be found or `rustc` does not answer.
pub fn machine_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let machine = machine()?;
    let mut paths = vec![root.to_path_buf(), machine.cargo_home, machine.sysroot];
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Each of `paths` that `bytes` holds, with how often, matched with either
/// separator and in either case: a path reaches a binary with whichever
/// separators the tool that wrote it joined, and on Windows in whatever
/// case the shell spelled it.
pub fn leaks(bytes: &[u8], paths: &[PathBuf]) -> Vec<(String, usize)> {
    let folded: Vec<u8> = bytes.iter().map(|&byte| fold(byte)).collect();
    let mut found = Vec::new();
    for path in paths {
        let needle: Vec<u8> = path.to_string_lossy().bytes().map(fold).collect();
        let needle = needle.strip_suffix(b"/").unwrap_or(&needle);
        let Some(&first) = needle.first() else {
            continue;
        };
        let count = folded
            .windows(needle.len())
            .filter(|window| window[0] == first && *window == needle)
            .count();
        if count > 0 {
            found.push((path.display().to_string(), count));
        }
    }
    found
}

/// A byte as [`leaks`] compares it: `\` as `/`, ASCII in lower case.
fn fold(byte: u8) -> u8 {
    if byte == b'\\' {
        b'/'
    } else {
        byte.to_ascii_lowercase()
    }
}

/// What a `--check` reads: the file's own bytes, or each entry's when the
/// file is a ZIP archive (a wheel), since a deflated entry hides its text.
fn contents(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok(vec![(path.display().to_string(), bytes)]);
    }
    // A wheel's extension module is tens of megabytes: the engine's bounds
    // for a document's archive are not the bounds for an artefact this
    // repository built.
    let limits = tinker_pdf_zip::Limits {
        max_entries: 1 << 16,
        max_entry_bytes: 1 << 30,
        max_inflated_total: 1 << 31,
        max_name_len: 1 << 12,
    };
    let mut archive = tinker_pdf_zip::Archive::open(&bytes, &limits)
        .map_err(|error| format!("{} does not open as a ZIP archive: {error}", path.display()))?;
    let entries: Vec<(usize, String)> = archive
        .entries()
        .iter()
        .filter(|entry| !entry.is_directory())
        .map(|entry| (entry.index, entry.name.clone()))
        .collect();
    let mut read = Vec::new();
    for (index, name) in entries {
        let data = archive
            .read(index)
            .map_err(|error| format!("{}: {name} does not read: {error}", path.display()))?;
        read.push((format!("{}!{name}", path.display()), data.into_owned()));
    }
    Ok(read)
}

/// `cargo xtask remap-paths`: this machine's flags, on one line, for a
/// release build's `RUSTFLAGS`.
///
/// # Errors
///
/// As [`for_this_machine`] and [`carried`].
pub fn print(root: &Path) -> Result<(), String> {
    let flags: Vec<String> = carried(for_this_machine(root)?)?
        .iter()
        .map(Remap::flag)
        .collect();
    println!("{}", flags.join(" "));
    Ok(())
}

/// `cargo xtask remap-paths --check FILE...`: fail if a built artefact still
/// carries a path of this machine.
///
/// # Errors
///
/// A file that cannot be read, or one that carries such a path.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let Some((first, files)) = args.split_first() else {
        return print(root);
    };
    if first != "--check" {
        return Err(format!("unknown option `{first}`"));
    }
    if files.is_empty() {
        return Err("`--check` needs the artefacts to read".to_string());
    }
    let paths = machine_paths(root)?;
    let mut problems = Vec::new();
    for file in files {
        for (name, bytes) in contents(Path::new(file))? {
            let found = leaks(&bytes, &paths);
            if found.is_empty() {
                println!(
                    "remap-paths: {name}: {} bytes, no path of this machine",
                    bytes.len()
                );
            }
            for (path, count) in found {
                problems.push(format!("{name} carries {path}, {count} time(s)"));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}. A build with `cargo xtask remap-paths` in its RUSTFLAGS writes none",
            problems.join("; ")
        ))
    }
}

/// Cargo's home, as cargo finds it: `CARGO_HOME`, or `.cargo` in the user's
/// home.
fn cargo_home() -> Result<PathBuf, String> {
    match std::env::var_os("CARGO_HOME").filter(|home| !home.is_empty()) {
        Some(home) => {
            let home = PathBuf::from(home);
            if home.is_absolute() {
                Ok(home)
            } else {
                std::env::current_dir()
                    .map(|directory| directory.join(home))
                    .map_err(|error| format!("cannot read the working directory: {error}"))
            }
        }
        None => user_home().map(|home| home.join(".cargo")),
    }
}

/// The user's home: `USERPROFILE` on Windows, `HOME` elsewhere.
fn user_home() -> Result<PathBuf, String> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("`{name}` is not set, so the user's home is unknown"))
}

/// What the `rustc` cargo would run prints for `args`: `RUSTC`'s, or the
/// one on `PATH`.
fn rustc(args: &[&str]) -> Result<String, String> {
    let program = std::env::var_os("RUSTC")
        .filter(|program| !program.is_empty())
        .unwrap_or_else(|| OsString::from("rustc"));
    let shown = format!("{} {}", program.to_string_lossy(), args.join(" "));
    let output = Command::new(&program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run `{shown}`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{shown}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("`{shown}` printed something not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(parts: &[&str]) -> PathBuf {
        let mut path = PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" });
        for part in parts {
            path.push(part);
        }
        path
    }

    fn checkout() -> PathBuf {
        path(&["work", "tinker-pdf", "tinker-pdf"])
    }

    fn cargo() -> PathBuf {
        path(&["home", "runner", ".cargo"])
    }

    fn toolchain() -> PathBuf {
        path(&["home", "runner", ".rustup", "toolchains", "stable"])
    }

    #[test]
    fn each_root_becomes_its_virtual_name_shortest_prefix_first() {
        let flags: Vec<String> = remaps(&checkout(), &cargo(), &toolchain(), "abc123")
            .iter()
            .map(Remap::flag)
            .collect();
        let sources = toolchain()
            .join("lib")
            .join("rustlib")
            .join("src")
            .join("rust");
        assert_eq!(
            flags,
            [
                format!("--remap-path-prefix={}=", checkout().display()),
                format!("--remap-path-prefix={}=/cargo", cargo().display()),
                format!("--remap-path-prefix={}=/rustc/abc123/", sources.display()),
            ]
        );
    }

    #[test]
    fn a_cargo_home_inside_the_checkout_keeps_its_own_name() {
        let inside = checkout().join(".cargo");
        let order: Vec<String> = remaps(&checkout(), &inside, &toolchain(), "abc123")
            .into_iter()
            .map(|remap| remap.to)
            .collect();
        // rustc applies the last flag that matches, so the deeper prefix
        // comes after the checkout's.
        assert_eq!(order, ["", "/cargo", "/rustc/abc123/"]);
    }

    #[test]
    fn rustflags_join_the_ones_a_process_was_given() {
        let flags = vec!["--remap-path-prefix=/a=".to_string()];
        assert_eq!(
            joined(None, Some(" -D  warnings "), &flags),
            (
                "RUSTFLAGS",
                "-D warnings --remap-path-prefix=/a=".to_string()
            )
        );
        assert_eq!(
            joined(None, None, &flags),
            ("RUSTFLAGS", "--remap-path-prefix=/a=".to_string())
        );
        // When the encoded form is set, cargo reads it alone, so the flags
        // join it.
        assert_eq!(
            joined(Some("-D\x1fwarnings"), Some("-C opt-level=0"), &flags),
            (
                "CARGO_ENCODED_RUSTFLAGS",
                "-D\x1fwarnings\x1f--remap-path-prefix=/a=".to_string()
            )
        );
        assert_eq!(
            joined(Some(""), Some("-D warnings"), &flags),
            (
                "CARGO_ENCODED_RUSTFLAGS",
                "--remap-path-prefix=/a=".to_string()
            )
        );
        // And a path with a space in it takes the encoded form, the one
        // that can carry it, with the plain flags moved across.
        let spaced = vec!["--remap-path-prefix=/a b=".to_string()];
        assert_eq!(
            joined(None, Some("-D warnings"), &spaced),
            (
                "CARGO_ENCODED_RUSTFLAGS",
                "-D\x1fwarnings\x1f--remap-path-prefix=/a b=".to_string()
            )
        );
    }

    #[test]
    fn a_leak_is_found_with_either_separator_and_in_either_case() {
        let paths = [
            PathBuf::from("C:\\Users\\Runner\\.cargo"),
            PathBuf::from("/home/runner"),
        ];
        let bytes = b"panicked at C:\\Users\\Runner\\.cargo\\registry\\src\\a.rs\0\
                      c:/users/runner/.CARGO/registry/b.rs\0\
                      C:\\Users\\Runner\\.cargo/lib/rustlib/src/rust\\library/c.rs\0\
                      /home/runner/work/d.rs";
        assert_eq!(
            leaks(bytes, &paths),
            [
                ("C:\\Users\\Runner\\.cargo".to_string(), 3),
                ("/home/runner".to_string(), 1)
            ]
        );
        assert!(leaks(
            b"crates/tinker-pdf/src/lib.rs\0/cargo/registry/src/a.rs",
            &paths
        )
        .is_empty());
    }

    /// A ZIP of deflated entries, each with the CRC-32 the reader checks.
    fn deflated_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
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
            let packed = tinker_pdf_filters::deflate(data);
            // The local file header (APPNOTE 4.3.7): deflated, UTF-8 names.
            out.extend_from_slice(b"PK\x03\x04");
            u16le(&mut out, 20);
            u16le(&mut out, 1 << 11);
            u16le(&mut out, 8);
            u16le(&mut out, 0);
            u16le(&mut out, 0x21);
            u32le(&mut out, crc);
            u32le(&mut out, packed.len());
            u32le(&mut out, data.len());
            u16le(&mut out, name.len());
            u16le(&mut out, 0);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);
            // Its central directory header (4.3.12).
            central.extend_from_slice(b"PK\x01\x02");
            u16le(&mut central, 20);
            u16le(&mut central, 20);
            u16le(&mut central, 1 << 11);
            u16le(&mut central, 8);
            u16le(&mut central, 0);
            u16le(&mut central, 0x21);
            u32le(&mut central, crc);
            u32le(&mut central, packed.len());
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

    #[test]
    fn a_wheel_is_read_entry_by_entry() {
        let module =
            b"/home/runner/work/tinker-pdf/tinker-pdf/crates/tinker-pdf/src/lib.rs".repeat(64);
        let wheel = deflated_zip(&[
            ("tinker_pdf/__init__.py", b"from .tinker_pdf import *"),
            ("tinker_pdf/tinker_pdf.abi3.so", &module),
        ]);
        let paths = [PathBuf::from("/home/runner/work/tinker-pdf/tinker-pdf")];
        // Deflated, the module's text is not in the archive's bytes.
        assert!(leaks(&wheel, &paths).is_empty());
        let file = std::env::temp_dir().join(format!("remap-wheel-{}.whl", std::process::id()));
        std::fs::write(&file, &wheel).expect("the wheel is written");
        let read = contents(&file);
        std::fs::remove_file(&file).expect("the wheel is removed");
        let read = read.expect("the wheel opens");
        let names: Vec<&str> = read
            .iter()
            .map(|(name, _)| name.rsplit('!').next().unwrap_or_default())
            .collect();
        assert_eq!(
            names,
            ["tinker_pdf/__init__.py", "tinker_pdf/tinker_pdf.abi3.so"]
        );
        assert_eq!(
            leaks(&read[1].1, &paths),
            [("/home/runner/work/tinker-pdf/tinker-pdf".to_string(), 64)]
        );
        assert!(leaks(&read[0].1, &paths).is_empty());
    }

    #[test]
    fn a_prefix_with_whitespace_is_refused() {
        let spaced = path(&["Users", "A Name", ".cargo"]);
        let refused = carried(remaps(&checkout(), &spaced, &toolchain(), "abc123"));
        assert_eq!(
            refused,
            Err(format!(
                "{} holds whitespace, which RUSTFLAGS cannot carry: cargo splits it there",
                spaced.display()
            ))
        );
        assert!(carried(remaps(&checkout(), &cargo(), &toolchain(), "abc123")).is_ok());
    }

    #[test]
    fn this_machine_names_its_toolchain_and_cargo() {
        let machine = machine().expect("rustc answers");
        assert!(machine.sysroot.join("lib").join("rustlib").is_dir());
        assert!(machine.cargo_home.is_absolute());
        assert!(!machine.commit.is_empty());
        let paths = machine_paths(&crate::repo_root()).expect("the machine's paths");
        assert!(paths.contains(&crate::repo_root()));
        assert!(paths.contains(&machine.sysroot));
    }
}
