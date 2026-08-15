//! A corpus child that does what the file it is given tells it to.
//!
//! The runner's whole claim is that one pathological file cannot take a run
//! down. That claim is only tested by files that are actually pathological,
//! and there is no honest way to get one: a PDF that reliably aborts the
//! engine would be a bug to fix rather than a fixture to keep, and a PDF that
//! reliably hangs it would be the same bug with a longer fuse. Waiting for the
//! corpus to supply one means the isolation is untested until the day it
//! matters.
//!
//! So the misbehaviour lives in the *files*, and this is the child that obeys
//! them. A synthetic corpus file carries a marker in a PDF comment — legal
//! syntax, ignored by every reader, including this project's — and this
//! program aborts or hangs when it sees one. The files are real PDFs; what
//! makes them pathological is a line of text that says so.
//!
//! It is never spawned by a real run. `corpus-run` spawns `tpdf probe`; this
//! is what the isolation tests point `--child` at.

use std::io::Read;

/// The marker's prefix. `%` opens a comment in PDF (7.2.4), so a file
//  carrying one of these is a valid PDF that additionally says what a harness
//  reading it should do.
const MARKER: &str = "%%TINKER-CORPUS-STUB ";

fn main() {
    let file = std::env::args().next_back().unwrap_or_default();
    let mut bytes = Vec::new();
    if let Ok(mut handle) = std::fs::File::open(&file) {
        let _ = handle.read_to_end(&mut bytes);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let instruction = text
        .lines()
        .find_map(|line| line.trim().strip_prefix(MARKER))
        .map(|rest| rest.trim().to_string())
        .unwrap_or_else(|| "pass".to_string());

    match instruction.as_str() {
        // Not a panic: a panic unwinds, `main` returns, and the process exits
        // 0, which is the case the runner must not mistake for a pass. An
        // abort is the harder half — no unwinding, no flush, no exit code that
        // means anything portable — and it is what a real engine crash under
        // `panic = "abort"` looks like.
        "abort" => {
            eprintln!("corpus-stub: aborting on {file}");
            std::process::abort();
        }
        // Never returns, and writes nothing, so the only thing that ends it is
        // the runner's timeout.
        "hang" => loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        },
        // Dies mid-record: everything up to `rendered` and then nothing. This
        // is the one an exit code cannot catch, because the process exits 0.
        "truncate" => {
            println!("probe 1");
            println!("file {file}");
            println!("opened yes");
            println!("pages 9");
        }
        "unopenable" => {
            println!("probe 1");
            println!("file {file}");
            println!("opened no not a PDF: no indirect objects found");
            println!("ms 1");
            println!("done");
        }
        other => {
            println!("probe 1");
            println!("file {file}");
            println!("opened yes");
            println!("ladder Trust");
            println!("pages 2");
            println!("rendered 2");
            if other == "degraded" {
                println!("cap jbig2");
                println!("warn render:UnsupportedImage(JBIG2Decode) 1");
            }
            println!("ms 1");
            println!("done");
        }
    }
}
