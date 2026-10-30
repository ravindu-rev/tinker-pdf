//! The one spawner: this revision's `tpdf`, run as a child.
//!
//! Tier P marks some operations CLI, and their figures include the process.
//! `tpdf` has no library half, so each of them is timed by running it. The
//! child is this repository's own binary, built from the same checkout, so
//! this file is the row in `xtask/src/main.rs`'s `SPAWNERS` that says it
//! hosts that binary and adjudicates nothing — and the only file under
//! `benches/` that starts a program.
//!
//! Every call is one child. Its standard output and error go to files in its
//! operation's own directory, and its exit status is checked every time, so a
//! child that fails panics the bench and is never timed. What an operation
//! asserts after it ran reads those files and what the child wrote, so the
//! run that is timed and the run that is checked are the same run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `tpdf` beside this bench binary's profile directory, or a panic naming
/// the command that builds it.
///
/// A bench binary is `target/<profile>/deps/<name>-<hash>` and a workspace
/// binary is `target/<profile>/<name>`, the layout `xtask/src/corpus.rs`
/// relies on for its own child: `cargo bench` runs beside a release `tpdf`
/// and `cargo test --benches` beside a debug one.
pub fn tpdf() -> PathBuf {
    let exe = std::env::current_exe().expect("this bench binary's own path");
    let profile = exe
        .parent()
        .and_then(Path::parent)
        .expect("a bench binary sits in target/<profile>/deps");
    let tpdf = profile.join(format!("tpdf{}", std::env::consts::EXE_SUFFIX));
    let release = profile.file_name().is_some_and(|name| name == "release");
    assert!(
        tpdf.is_file(),
        "{} is not built; the CLI operations run it, so build it first with \
         `cargo build{} -p tpdf`",
        tpdf.display(),
        if release { " --release" } else { "" }
    );
    tpdf
}

/// The directory a CLI operation's input files are written into: the one
/// `TINKER_BENCH_FIXTURES` names, which is how a paired job hands two builds
/// the same bytes, or `bench-fixtures/` under the directory cargo gives every
/// bench target for its own files.
fn fixtures() -> (PathBuf, bool) {
    let (dir, shared) = match std::env::var_os("TINKER_BENCH_FIXTURES") {
        Some(dir) => (PathBuf::from(dir), true),
        None => (
            Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench-fixtures"),
            false,
        ),
    };
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("the fixture directory {}: {error}", dir.display()));
    (dir, shared)
}

/// The input file `name`, holding `bytes`, written once a process, and its
/// path. In a directory `TINKER_BENCH_FIXTURES` names a file already there is
/// kept, so the first process to need an input writes it for every build
/// timed on it.
pub fn fixture(name: &str, bytes: impl FnOnce() -> Vec<u8>) -> PathBuf {
    let (dir, shared) = fixtures();
    let path = dir.join(name);
    if !(shared && path.is_file()) {
        std::fs::write(&path, bytes())
            .unwrap_or_else(|error| panic!("the fixture {}: {error}", path.display()));
    }
    path
}

/// One CLI operation's runner: the binary, and the operation's own directory,
/// where its children's output files and everything they write go.
pub struct Cli {
    tpdf: PathBuf,
    dir: PathBuf,
}

impl Cli {
    /// The runner for the operation `slug` names, its directory emptied.
    pub fn new(slug: &str) -> Cli {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("bench-runs")
            .join(slug);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .unwrap_or_else(|error| panic!("emptying {}: {error}", dir.display()));
        }
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|error| panic!("the run directory {}: {error}", dir.display()));
        Cli { tpdf: tpdf(), dir }
    }

    /// `name` inside this operation's directory: where a command is told to
    /// write.
    pub fn out(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// One child, `tpdf` with `args`: its standard output and error written
    /// to `stdout` and `stderr` here, and its exit status checked.
    pub fn run(&self, args: &[&dyn AsRef<std::ffi::OsStr>]) {
        let file = |name: &str| {
            std::fs::File::create(self.dir.join(name))
                .unwrap_or_else(|error| panic!("{name} in {}: {error}", self.dir.display()))
        };
        let status = Command::new(&self.tpdf)
            .args(args.iter().map(|arg| arg.as_ref()))
            .stdin(Stdio::null())
            .stdout(file("stdout"))
            .stderr(file("stderr"))
            .status()
            .unwrap_or_else(|error| panic!("{} did not start: {error}", self.tpdf.display()));
        assert!(
            status.success(),
            "tpdf {:?} failed with {status}: {}",
            args.iter().map(|arg| arg.as_ref()).collect::<Vec<_>>(),
            std::fs::read_to_string(self.dir.join("stderr")).unwrap_or_default()
        );
    }

    /// What the last child wrote to its standard output.
    pub fn stdout(&self) -> String {
        std::fs::read_to_string(self.dir.join("stdout"))
            .unwrap_or_else(|error| panic!("the last run's stdout: {error}"))
    }

    /// The PNGs a render wrote into `out`, by name, each with its width and
    /// height read from its `IHDR`.
    pub fn pngs(&self, out: &Path) -> Vec<(String, u32, u32)> {
        let mut found: Vec<(String, u32, u32)> = std::fs::read_dir(out)
            .unwrap_or_else(|error| panic!("the render directory {}: {error}", out.display()))
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|e| e == "png"))
            .map(|path| {
                let bytes = std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert_eq!(
                    &bytes[..8],
                    b"\x89PNG\r\n\x1a\n",
                    "{} is a PNG",
                    path.display()
                );
                assert_eq!(
                    &bytes[12..16],
                    b"IHDR",
                    "{} begins with IHDR",
                    path.display()
                );
                let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
                let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("a UTF-8 file name")
                    .to_string();
                (name, width, height)
            })
            .collect();
        found.sort();
        found
    }
}
