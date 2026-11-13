//! One reviewed golden per rendered surface the engine synthesises
//! (ROADMAP row K-08): appearances it draws for annotations and signatures,
//! pages it makes from other formats, the SVG it writes.
//!
//! `render_goldens.rs` holds one golden per operator family; this file holds
//! the surfaces by the same mechanism, `golden_support`'s — the header each
//! golden carries, its refusal of a field absent, blank or white space, the
//! byte-for-byte comparison and the writer — with a directory and ceilings of
//! its own. A surface is a whole synthesised page rather than one operator,
//! so its golden is larger than an operator's, and the operator families'
//! ceilings do not move for it. Every surface with text sets it in the test's
//! own face (`curvy_font`), never a face a build resolves by name, so one
//! golden serves the default and `bundled-fonts` builds and both run this file.
//!
//! A raster golden is a PPM with its header in `#` comment lines; an SVG
//! golden is the writer's output with its header in an XML comment after the
//! XML declaration, compared past that comment byte for byte.

use std::path::{Path, PathBuf};

use tinker_pdf::Bitmap;

mod golden_support;
use golden_support::{
    assert_small_enough, goldens_in, ppm_mismatch, read_header, read_svg_header, reviewer_and_date,
    svg_body, svg_mismatch, to_ppm, to_svg_golden, Golden, Header, HeaderError, UNREVIEWED_MARK,
};

/// The most bytes one surface golden may be: a page of about 145 points
/// square at 72 dpi. A reviewer reads the whole picture against its clause.
const PER_SURFACE: u64 = 64 * 1024;

/// The most bytes the surface goldens may be altogether.
const ALL_SURFACES: u64 = 512 * 1024;

/// A surface and the clause its golden draws.
struct Surface {
    /// The family, which is the golden's file name.
    name: &'static str,
    /// The clause the picture is read against.
    clause: &'static str,
    /// The surface, drawn by this build.
    draw: fn() -> Bitmap,
    /// The fewest non-background pixels it may paint: a blank golden matches
    /// a blank render forever.
    least_ink: usize,
}

/// The surfaces, in the order their goldens are listed and reviewed.
///
/// Order-sensitive on purpose, as `render_goldens.rs`'s families are: a
/// surface dropped from the list is the failure this exists to make visible.
const SURFACES: &[Surface] = &[];

/// The surfaces no person has read yet: exactly those whose golden's header
/// reads `reviewer: unreviewed`. A surface leaves this list when a person has
/// read its golden against its clause and put their name in it (H-73).
const UNREVIEWED: &[&str] = &[];

fn surfaces_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("surfaces")
}

fn golden_path(surface: &Surface) -> PathBuf {
    surfaces_dir().join(format!("{}.ppm", surface.name))
}

/// Pixels that are not the white the page started as.
fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count()
}

// ---- the checks -------------------------------------------------------------

/// Every surface has a golden whose header names it and its clause, the
/// surfaces left unreviewed are exactly those `UNREVIEWED` lists, and the
/// directory holds nothing the list does not name.
#[test]
fn every_surface_family_has_a_golden() {
    let mut unreviewed = Vec::new();
    for surface in SURFACES {
        let path = golden_path(surface);
        assert!(
            path.exists(),
            "{} has no golden; run the writer below with --ignored",
            surface.name
        );
        let bytes = std::fs::read(&path).expect("the golden reads");
        let header = read_header(&bytes)
            .unwrap_or_else(|e| panic!("{}: its header is refused: {e:?}", surface.name));
        assert_eq!(header.family, surface.name, "the header names its family");
        assert_eq!(header.clause, surface.clause, "the header names its clause");
        if header.reviewer == UNREVIEWED_MARK {
            unreviewed.push(surface.name);
        }
    }
    let mut listed = UNREVIEWED.to_vec();
    listed.sort_unstable();
    unreviewed.sort_unstable();
    assert_eq!(
        listed, unreviewed,
        "UNREVIEWED is the surfaces whose goldens read `reviewer: unreviewed`"
    );

    // A golden left behind by a renamed surface reads exactly like a
    // reviewed one.
    let mut wanted: Vec<String> = SURFACES.iter().map(|s| s.name.to_string()).collect();
    wanted.sort();
    assert_eq!(
        goldens_in(&surfaces_dir(), &["ppm", "svg"]),
        wanted,
        "the surface goldens on disk are the surfaces listed"
    );
}

/// A header with a field absent, blank or white space is refused, in the PPM
/// form and in the SVG comment form alike, and the undamaged twin of each is
/// read; a golden made from the writer's output gives that output back past
/// its header comment, and one changed byte is a difference.
#[test]
fn a_surface_golden_without_a_reviewer_is_refused() {
    let ppm = |reviewer: &str, reviewed: &str| {
        format!(
            "P6\n# tinker-pdf reviewed golden\n# family: signature\n# clause: 12.7.4.5\n\
             {reviewer}{reviewed}# built-by: surface_goldens.rs::signature\n1 1\n255\n\0\0\0"
        )
        .into_bytes()
    };
    let read = read_header(&ppm("# reviewer: A Person\n", "# reviewed: 2026-11-13\n"));
    assert_eq!(
        read,
        Ok(Header {
            family: "signature".into(),
            clause: "12.7.4.5".into(),
            reviewer: "A Person".into(),
            reviewed: "2026-11-13".into(),
            built_by: "surface_goldens.rs::signature".into(),
        }),
        "the undamaged twin is read"
    );
    let reviewed = "# reviewed: 2026-11-13\n";
    assert_eq!(
        read_header(&ppm("", reviewed)),
        Err(HeaderError::Missing("reviewer"))
    );
    assert_eq!(
        read_header(&ppm("# reviewer:\n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_header(&ppm("# reviewer: \t  \n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_header(&ppm("# reviewer: A Person\n", "# reviewed:  \n")),
        Err(HeaderError::Empty("reviewed")),
        "a later field is read as closely as the first"
    );

    let svg = |reviewer: &str, reviewed: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!--\ntinker-pdf reviewed golden\n\
             family: to-svg\nclause: SVG 1.1 8.3\n{reviewer}{reviewed}\
             built-by: surface_goldens.rs::to-svg\n-->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n"
        )
    };
    assert!(
        read_svg_header(&svg("reviewer: A Person\n", "reviewed: 2026-11-13\n")).is_ok(),
        "the undamaged twin is read"
    );
    let reviewed = "reviewed: 2026-11-13\n";
    assert_eq!(
        read_svg_header(&svg("", reviewed)),
        Err(HeaderError::Missing("reviewer"))
    );
    assert_eq!(
        read_svg_header(&svg("reviewer:\n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_svg_header(&svg("reviewer: \t  \n", reviewed)),
        Err(HeaderError::Empty("reviewer"))
    );
    assert_eq!(
        read_svg_header(&svg("reviewer: A Person\n", "reviewed:  \n")),
        Err(HeaderError::Empty("reviewed"))
    );
    assert_eq!(
        read_svg_header("<?xml version=\"1.0\"?>\n<svg/>\n"),
        Err(HeaderError::NotAnSvgGolden),
        "an SVG with no header comment is no golden"
    );
    assert_eq!(read_header(b"P5\n# family: x\n"), Err(HeaderError::NotAPpm));

    // The two forms, written and read back.
    let golden = Golden {
        family: "to-svg",
        clause: "SVG 1.1 8.3",
        built_by: "surface_goldens.rs::to-svg",
    };
    let output = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg width=\"1pt\"/>\n";
    let committed = to_svg_golden(output, golden, UNREVIEWED_MARK, UNREVIEWED_MARK);
    assert_eq!(svg_body(&committed).as_deref(), Some(output));
    let header = read_svg_header(&committed).expect("a written header reads");
    assert_eq!(
        (header.family.as_str(), header.reviewer.as_str()),
        ("to-svg", UNREVIEWED_MARK)
    );
    assert_eq!(svg_mismatch("to-svg", &committed, output), None);
    assert!(svg_mismatch("to-svg", &committed, &output.replace("1pt", "2pt")).is_some());

    let bitmap = Bitmap {
        width: 1,
        height: 1,
        format: tinker_pdf::PixelFormat::Rgb8,
        stride: 3,
        data: vec![10, 20, 30],
        warnings: Vec::new(),
        premultiplied: false,
        stats: None,
    };
    let committed = to_ppm(&bitmap, golden, UNREVIEWED_MARK, UNREVIEWED_MARK);
    assert!(read_header(&committed).is_ok(), "a written header reads");
    assert_eq!(ppm_mismatch("to-svg", &committed, &bitmap), None);
    let mut changed = bitmap.clone();
    changed.data[1] = 21;
    assert!(ppm_mismatch("to-svg", &committed, &changed).is_some());
}

/// A surface golden stays small enough that reviewing one is a real act, under
/// ceilings of the surfaces' own.
#[test]
fn a_surface_golden_stays_small_enough_to_review() {
    let goldens: Vec<(String, PathBuf)> = SURFACES
        .iter()
        .map(|surface| (surface.name.to_string(), golden_path(surface)))
        .collect();
    assert_small_enough(&goldens, PER_SURFACE, ALL_SURFACES);
}

/// Writes the surface goldens. Run with `--ignored` when a picture is meant to
/// change, and have the result reviewed before it is committed.
#[test]
#[ignore = "writes the committed goldens"]
fn write_the_surface_goldens() {
    let (reviewer, reviewed) = reviewer_and_date();
    std::fs::create_dir_all(surfaces_dir()).expect("the surfaces directory");
    for surface in SURFACES {
        let bitmap = (surface.draw)();
        let drawn = ink(&bitmap);
        assert!(
            drawn >= surface.least_ink,
            "{}: the page painted {drawn} pixels, fewer than the {} it is              supposed to -- a blank golden is not written",
            surface.name,
            surface.least_ink
        );
        let built_by = format!("surface_goldens.rs::{}", surface.name);
        let golden = Golden {
            family: surface.name,
            clause: surface.clause,
            built_by: &built_by,
        };
        let bytes = to_ppm(&bitmap, golden, &reviewer, &reviewed);
        std::fs::write(golden_path(surface), &bytes).expect("the golden writes");
        println!(
            "  wrote {} ({} bytes, {drawn} pixels of ink)",
            surface.name,
            bytes.len()
        );
    }
}
