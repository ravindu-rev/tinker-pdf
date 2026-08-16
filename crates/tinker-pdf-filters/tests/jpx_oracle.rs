//! `openjpeg` as an external arbiter for JPEG 2000 (gap 18a, ruling 9).
//!
//! The repository holds **zero JPX bytes**, and ISO/IEC 15444-4's conformance
//! codestreams are not freely redistributable, so they are not the answer and
//! no amount of wanting them makes them one. Gap 17 solved the same problem
//! for JBIG2 by transcribing T.88's Annex H by hand; T.800 publishes no
//! equivalent datastream, so this plan cannot copy that move.
//!
//! What `opj_compress` buys that a hand-built stream cannot is the axes: each
//! of LRCP, RLCP, RPCL, PCRL and CPRL, multiple tiles, multiple quality
//! layers, explicit precincts, SOP and EPH, code-block sizes from 4 x 4 to
//! 64 x 64, one to five decomposition levels, reversible and irreversible.
//! Every one of those is a flag here, and every one of them is a partition or
//! an order this decoder would otherwise be guessing about.
//!
//! # Ruling 9
//!
//! `opj_compress` and `opj_decompress` are subprocesses, never dependencies.
//! Nothing links them, no part of them is vendored, and their output is a
//! transient comparison reference read and dropped. Fixtures are written into
//! `CARGO_TARGET_TMPDIR` because the tools read files rather than pipes;
//! nothing there is committed.
//!
//! # Skipped, not silently passed
//!
//! Gap 20 found qpdf doing exactly the wrong thing here: a check that quietly
//! succeeds when its oracle is missing is a check that will one day be
//! missing everywhere, because `cargo test` with a filter that matches
//! nothing exits 0 and reads exactly like a pass. When `openjpeg` cannot be
//! found these tests print [`SKIPPED`] and do nothing, and the CI job greps
//! its own output for [`RAN`] — checking for the skip **first**, so that the
//! reason reaches the log rather than a silent red tick.
//!
//! Set `TINKER_OPENJPEG` to the directory holding the two executables, or to
//! `wsl:<distro>` to reach them through WSL — which is how they are reached
//! on Windows, since `cargo-fuzz` and `openjpeg` both live on the Linux side
//! of this machine.
//!
//! # What is asserted here, and why it is not weak
//!
//! Every stream below is refused, because the wavelet is milestone 4. What is
//! asserted is *which* refusal: `JpxStageNotBuilt` and nothing else. That
//! means every byte this build has code for was understood — a marker it did
//! not know, a tile grid it could not divide, a packet header that did not
//! land on a boundary would each replace that warning with a different one.
//! The assertion therefore strengthens on its own as the milestones land,
//! without being rewritten.

use std::path::{Path, PathBuf};
use std::process::Command;

use tinker_pdf_filters::{jpx_decode, Capability, FilterError, Limits, Warning};

/// Printed once per test that actually invoked openjpeg. CI greps for it.
const RAN: &str = "jpx-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "jpx-oracle: SKIPPED";

/// How to reach the two executables.
#[derive(Clone, Debug)]
struct OpenJpeg {
    /// `wsl -d <distro> --` in front of every command, for a Windows host
    /// whose openjpeg lives inside WSL.
    wsl: Option<String>,
    /// A directory to prefix, when `TINKER_OPENJPEG` named one.
    dir: Option<PathBuf>,
}

impl OpenJpeg {
    fn command(&self, tool: &str) -> Command {
        match &self.wsl {
            Some(distro) => {
                let mut c = Command::new("wsl");
                c.arg("-d").arg(distro).arg("--").arg(tool);
                c
            }
            None => Command::new(match &self.dir {
                Some(dir) => dir.join(tool),
                None => PathBuf::from(tool),
            }),
        }
    }

    /// A path as the tool will see it. WSL reads `/mnt/c/...` where Windows
    /// writes `C:\...`, and the fixtures are written by this process.
    fn path(&self, path: &Path) -> String {
        let text = path.display().to_string();
        if self.wsl.is_none() {
            return text;
        }
        let text = text.replace('\\', "/");
        match text.split_once(":/") {
            Some((drive, rest)) if drive.len() == 1 => {
                format!("/mnt/{}/{}", drive.to_ascii_lowercase(), rest)
            }
            _ => text,
        }
    }

    /// Runs one tool and returns its output joined, panicking only if the
    /// process could not be started at all.
    fn run(&self, tool: &str, args: &[String]) -> (bool, String) {
        let out = self
            .command(tool)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("{tool} did not run: {e}"));
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), text)
    }
}

/// Where openjpeg is, or `None`.
///
/// A usage query rather than a `which`: an executable that cannot run is not
/// an oracle. `opj_compress -h` exits **1** — it is a usage message, not a
/// success — so the check is on the text rather than on the status, which is
/// the trap this comment exists to keep the next reader out of.
fn openjpeg() -> Option<OpenJpeg> {
    let named = std::env::var("TINKER_OPENJPEG").ok();
    let mut candidates = Vec::new();
    match named.as_deref() {
        Some(v) if v.starts_with("wsl:") => candidates.push(OpenJpeg {
            wsl: Some(v[4..].to_string()),
            dir: None,
        }),
        Some(v) => candidates.push(OpenJpeg {
            wsl: None,
            dir: Some(PathBuf::from(v)),
        }),
        None => {
            candidates.push(OpenJpeg {
                wsl: None,
                dir: None,
            });
            if cfg!(windows) {
                // The local route on this machine, and the one three other
                // plans already record for `cargo-fuzz`: the Linux tooling
                // lives in WSL2 rather than on the Windows side.
                for distro in ["Ubuntu-24.04", "Ubuntu"] {
                    candidates.push(OpenJpeg {
                        wsl: Some(distro.to_string()),
                        dir: None,
                    });
                }
            }
        }
    }
    candidates.into_iter().find(|c| {
        c.command("opj_compress").arg("-h").output().is_ok_and(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned()
                + &String::from_utf8_lossy(&o.stderr);
            text.contains("openjp2")
        })
    })
}

/// The oracle, or a printed skip.
///
/// The banner is deliberately shouted: a skipped oracle in a green run is the
/// thing this whole file exists to make visible.
macro_rules! oracle {
    ($what:expr) => {
        match openjpeg() {
            Some(o) => {
                println!("{} {}", RAN, $what);
                o
            }
            None => {
                println!("{} {} (openjpeg not found)", SKIPPED, $what);
                return;
            }
        }
    };
}

/// A 32 x 32 greyscale ramp with a little high-frequency detail in it, as a
/// binary PGM. Our own image, so the codestreams `opj_compress` makes from it
/// are ours — which is what lets them seed the fuzz corpus without any
/// conformance material being redistributed.
fn grey_pgm() -> Vec<u8> {
    let (w, h) = (32usize, 32usize);
    let mut out = format!("P5\n{w} {h}\n255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            out.push(((x * 7 + y * 5 + ((x * y) & 15)) % 256) as u8);
        }
    }
    out
}

/// The same as a colour PPM, so the RCT and the ICT are reachable.
fn rgb_ppm() -> Vec<u8> {
    let (w, h) = (32usize, 32usize);
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            out.push((x * 8) as u8);
            out.push((y * 8) as u8);
            out.push(((x + y) * 4) as u8);
        }
    }
    out
}

fn tmp() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("jpx-oracle")
}

/// Encodes `image` with the given flags and returns the codestream.
///
/// `-n 2` is the default in every caller that does not override it, and it is
/// not decoration: `opj_compress` refuses a small image with its default
/// resolution count — "Number of resolutions is too high in comparison to the
/// size of tiles" — so a 32 x 32 fixture has to say how many it wants.
fn encode(o: &OpenJpeg, name: &str, image: &[u8], suffix: &str, flags: &[&str]) -> Vec<u8> {
    let dir = tmp();
    std::fs::create_dir_all(&dir).expect("a target tmpdir");
    let input = dir.join(format!(
        "{name}.{}",
        if image.starts_with(b"P6") {
            "ppm"
        } else {
            "pgm"
        }
    ));
    let output = dir.join(format!("{name}.{suffix}"));
    std::fs::write(&input, image).expect("write the fixture");
    let _ = std::fs::remove_file(&output);

    let mut args = vec![
        "-i".to_string(),
        o.path(&input),
        "-o".to_string(),
        o.path(&output),
    ];
    args.extend(flags.iter().map(|s| (*s).to_string()));
    let (ok, text) = o.run("opj_compress", &args);
    assert!(ok, "opj_compress {flags:?} failed:\n{text}");
    std::fs::read(&output).unwrap_or_else(|e| panic!("opj_compress wrote no {name}: {e}"))
}

/// Feeds a codestream to the decoder and returns the warnings it left.
fn refuse(bytes: &[u8]) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let got = jpx_decode(bytes, &Limits::new(1 << 22), &mut warnings);
    assert_eq!(
        got,
        Err(FilterError::Unsupported(Capability::Jpx)),
        "the wavelet is milestone 4; nothing may decode yet"
    );
    warnings
}

/// Every axis `opj_compress` can vary, parsed to the last stage this build
/// has, with the refusal naming that stage and nothing else.
///
/// A marker this build did not know would leave `JpxMarkerUnknown` here; a
/// tile grid it could not divide would leave `JpxStructureInvalid`; a
/// code-block style it does not implement would leave
/// `JpxFeatureUnsupported`. Each of those is a different warning and each
/// would fail this assertion, which is why "it was refused" is not what is
/// being checked.
#[test]
fn every_axis_openjpeg_can_encode_parses_to_the_last_built_stage() {
    let o = oracle!("codestream headers across every encoder axis");
    let grey = grey_pgm();
    let rgb = rgb_ppm();

    let cases: &[(&str, &[u8], &str, &[&str])] = &[
        ("lrcp", &grey, "j2k", &["-r", "1", "-n", "2"]),
        ("rlcp", &grey, "j2k", &["-r", "1", "-n", "2", "-p", "RLCP"]),
        ("rpcl", &grey, "j2k", &["-r", "1", "-n", "2", "-p", "RPCL"]),
        ("pcrl", &grey, "j2k", &["-r", "1", "-n", "2", "-p", "PCRL"]),
        ("cprl", &grey, "j2k", &["-r", "1", "-n", "2", "-p", "CPRL"]),
        ("jp2-boxed", &grey, "jp2", &["-r", "1", "-n", "2"]),
        ("rgb-rct", &rgb, "j2k", &["-r", "1", "-n", "2"]),
        ("rgb-ict-lossy", &rgb, "j2k", &["-r", "20", "-n", "2", "-I"]),
        ("layers", &grey, "j2k", &["-r", "20,10,1", "-n", "2"]),
        (
            "tiles",
            &grey,
            "j2k",
            &["-r", "1", "-n", "2", "-t", "16,16"],
        ),
        (
            "sop-eph",
            &grey,
            "j2k",
            &["-r", "1", "-n", "2", "-SOP", "-EPH"],
        ),
        (
            "precincts",
            &grey,
            "j2k",
            &["-r", "1", "-n", "2", "-c", "[128,128],[128,128]"],
        ),
        ("cb-4x4", &grey, "j2k", &["-r", "1", "-n", "2", "-b", "4,4"]),
        (
            "cb-64x64",
            &grey,
            "j2k",
            &["-r", "1", "-n", "2", "-b", "64,64"],
        ),
        ("levels-1", &grey, "j2k", &["-r", "1", "-n", "1"]),
        (
            "levels-5",
            &grey,
            "j2k",
            &["-r", "1", "-n", "5", "-t", "32,32"],
        ),
        ("segsym", &grey, "j2k", &["-r", "1", "-n", "2", "-M", "32"]),
    ];

    for (name, image, suffix, flags) in cases {
        let bytes = encode(&o, name, image, suffix, flags);
        assert!(!bytes.is_empty(), "{name} encoded to nothing");
        assert_eq!(
            refuse(&bytes),
            vec![Warning::JpxStageNotBuilt],
            "{name}: a stage this build does have failed to understand the stream"
        );
    }
}

/// The two shapes PDF permits (7.4.9) really do differ in the bytes, so a
/// decoder that only ever saw one of them would not know.
#[test]
fn a_boxed_jp2_and_a_bare_codestream_are_different_files() {
    let o = oracle!("the JP2 container against a bare codestream");
    let grey = grey_pgm();
    let bare = encode(&o, "shape-bare", &grey, "j2k", &["-r", "1", "-n", "2"]);
    let boxed = encode(&o, "shape-boxed", &grey, "jp2", &["-r", "1", "-n", "2"]);
    assert!(
        bare.starts_with(&[0xFF, 0x4F, 0xFF, 0x51]),
        "a bare codestream opens with SOC SIZ"
    );
    assert!(
        boxed.starts_with(&[0, 0, 0, 12, b'j', b'P']),
        "a JP2 opens with its signature box"
    );
    assert!(boxed.len() > bare.len(), "the boxes cost something");
    assert_eq!(refuse(&bare), vec![Warning::JpxStageNotBuilt]);
    assert_eq!(refuse(&boxed), vec![Warning::JpxStageNotBuilt]);
}

/// The round trip the oracle's own correctness rests on, and the second trap
/// worth writing down: `opj_decompress` puts a `#OpenJPEG-2.5.0` comment line
/// into the PGM header, so a whole-file comparison **fails on a correct
/// decode**. Compare the sample data, not the file.
#[test]
fn openjpeg_round_trips_our_fixture_losslessly() {
    let o = oracle!("openjpeg's own lossless round trip");
    let grey = grey_pgm();
    let encoded = encode(&o, "roundtrip", &grey, "j2k", &["-r", "1", "-n", "2"]);
    assert!(!encoded.is_empty());

    let dir = tmp();
    let back = dir.join("roundtrip-out.pgm");
    let _ = std::fs::remove_file(&back);
    let (ok, text) = o.run(
        "opj_decompress",
        &[
            "-i".to_string(),
            o.path(&dir.join("roundtrip.j2k")),
            "-o".to_string(),
            o.path(&back),
        ],
    );
    assert!(ok, "opj_decompress failed:\n{text}");
    let decoded = std::fs::read(&back).expect("opj_decompress wrote a PGM");
    assert_eq!(
        pgm_samples(&decoded),
        pgm_samples(&grey),
        "openjpeg did not round-trip its own lossless encode"
    );
}

/// The sample data of a binary PGM or PPM, past however many header lines and
/// comments the writer chose to put in front of it.
fn pgm_samples(data: &[u8]) -> &[u8] {
    let mut at = 0usize;
    let mut fields = 0;
    while fields < 4 && at < data.len() {
        while at < data.len() && data[at].is_ascii_whitespace() {
            at += 1;
        }
        if data.get(at) == Some(&b'#') {
            while at < data.len() && data[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        while at < data.len() && !data[at].is_ascii_whitespace() {
            at += 1;
        }
        fields += 1;
    }
    data.get(at + 1..).unwrap_or_default()
}
