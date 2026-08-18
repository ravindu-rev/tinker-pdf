//! mutool reads the XPS itself, and agrees with what this engine made of it
//! (gap 30 milestone 9, ruling 9).
//!
//! **The irony is the point, and gap 30's design section writes it down rather
//! than leaving it to be noticed.** MuPDF reads XPS — `mutool draw` lists
//! `pdf`, `xps`, `cbz` and `epub` as its input formats — and mutool is already
//! one of ruling 9's four named oracles. So *the library gap 28's decision
//! removes from Tinker's shipped tree is the best available oracle for the
//! format it is being removed for*, and ruling 9 permits exactly that, because
//! an oracle is invoked and never linked.
//!
//! `tests/xps_qpdf.rs` is the other half of this gap's oracle story and it
//! answers a different question. qpdf reads the **PDF this engine wrote** and
//! says whether it is a well-formed PDF; it has never seen an XPS and cannot
//! say whether the document means what the package meant. mutool reads
//! **both** — the package and the document — so the comparison here is between
//! two readings of the same source by two independent programs, which is the
//! only thing in this repository that can catch a mistake this engine makes
//! consistently in both directions.
//!
//! # What is compared, and what deliberately is not
//!
//! `mutool draw -F trace` emits a **device-call trace** as XML: which paths
//! were filled, in what order, in what colour, under what matrix, and which
//! glyph of which font landed at which point. That is a *structural* oracle and
//! it can be made exact. A rendered PNG cannot: two independent rasterisers are
//! not byte-equal, this plan never promised they would be, and gap 18a
//! pre-argued the same point for a fixed-point wavelet against a float
//! reference. So nothing here compares pixels.
//!
//! Numbers are compared with a tolerance because the two traces round
//! differently — mutool prints a PDF's `.2745` and an XPS's `.27450983` for the
//! same colour, because the PDF this engine writes carries four decimal places
//! and the markup carries a byte. The tolerance is stated at each site with the
//! reason for its size.
//!
//! # And the caveat that must be stated rather than discovered
//!
//! **It is the same engine Tinker is leaving.** Where MuPDF is wrong about XPS
//! this engine will agree with it and both will be wrong. An oracle bounds the
//! space of disagreements; it does not certify the answer.
//!
//! # Skipped, not silently passed
//!
//! Gap 20 found that a skipped oracle exits 0 and reads exactly like a pass.
//! These tests print [`RAN`] or [`SKIPPED`] and the CI job greps its own output
//! for the first and **fails on the second**, which is row 9's own words: the
//! job is red when the tool is missing. `mupdf-tools` is an apt package, so
//! `ubuntu-latest` installs it beside qpdf. Set `TINKER_MUTOOL` to an absolute
//! path when mutool is installed somewhere `PATH` does not reach.

use std::path::{Path, PathBuf};
use std::process::Command;

use tinker_pdf::{cbz, xps};

/// Printed once per test that actually invoked mutool. CI greps for it.
const RAN: &str = "mutool-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "mutool-oracle: SKIPPED";

/// Where mutool is, or `None`.
///
/// A version query rather than a `which`, for `xps_qpdf.rs`'s reason: an
/// executable that cannot run is not an oracle, and this is the one call whose
/// failure means *skip* rather than *fail*. `mutool -v` writes its banner to
/// stderr and exits 0.
fn mutool() -> Option<PathBuf> {
    let named =
        std::env::var_os("TINKER_MUTOOL").map_or_else(|| PathBuf::from("mutool"), PathBuf::from);
    let output = Command::new(&named).arg("-v").output().ok()?;
    output.status.success().then_some(named)
}

macro_rules! oracle {
    ($what:expr) => {
        match mutool() {
            Some(path) => {
                println!("{} {} ({})", RAN, $what, path.display());
                path
            }
            None => {
                println!(
                    "{} {} -- mutool is not on PATH and TINKER_MUTOOL is unset",
                    SKIPPED, $what
                );
                return;
            }
        }
    };
}

/// A directory per test, because cargo runs them on threads and two of them
/// would otherwise rewrite a file the other has open — which on Windows fails
/// outright and elsewhere fails worse, with the tool reading half a file and
/// reporting a difference that is not there.
fn scratch(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("xps-mutool")
        .join(test);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// One of milestone 1's real packages, unmodified, written where mutool can
/// reach it.
///
/// Copied into `CARGO_TARGET_TMPDIR` rather than handed to mutool where it
/// lives, so that both sides of every comparison below are read from the same
/// directory by the same invocation shape. Ruling 9: the tool's output is a
/// transient comparison reference and never leaves `target/`.
fn package(dir: &Path, name: &str) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    let bytes = std::fs::read(&source).unwrap_or_else(|e| panic!("reading {source:?}: {e}"));
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the package is written");
    path
}

/// The document this engine synthesises from that package, beside it.
fn synthesised(dir: &Path, name: &str) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    let bytes = std::fs::read(&source).unwrap_or_else(|e| panic!("reading {source:?}: {e}"));
    let archive =
        cbz::open_archive(&bytes, &tinker_pdf_zip::Limits::DEFAULT).expect("a readable ZIP");
    let pdf = match xps::route(archive, &xps::Limits::DEFAULT) {
        xps::Routing::Document(pdf, _) => pdf,
        xps::Routing::Refused(why) => panic!("{name} was refused: {why}"),
        xps::Routing::NotXps(_) => panic!("{name} is an XPS"),
    };
    let path = dir.join(format!("{name}.pdf"));
    std::fs::write(&path, pdf).expect("the document is written");
    path
}

/// `mutool draw -F trace`, as a string.
///
/// The trace goes to a file rather than to stdout because mutool writes its
/// per-page progress and its ICC warning to the same streams, and a comparison
/// that had to strip those would be comparing this test's filter rather than
/// the tool's answer.
fn trace(mutool: &Path, input: &Path) -> String {
    let out = input.with_extension("trace.xml");
    let output = Command::new(mutool)
        .arg("draw")
        .args(["-F", "trace", "-o"])
        .arg(&out)
        .arg(input)
        .output()
        .unwrap_or_else(|e| panic!("mutool draw on {input:?} did not run: {e}"));
    assert!(
        output.status.success(),
        "mutool draw on {input:?} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    std::fs::read_to_string(&out).unwrap_or_else(|e| panic!("reading {out:?}: {e}"))
}

/// Every value of one attribute, in document order.
///
/// A hand-rolled scan rather than a parser, and the reason is ruling 8 in
/// miniature: pulling an XML crate in to read a test oracle's output would put
/// this repository's own parser between the tool and the assertion, which is
/// the one thing an oracle must not have.
///
/// **The leading space is load-bearing**, and the first draft of this file did
/// not have it: `end="400 200"` is a substring of `extend="1 1"`, so a shading's
/// axis endpoint matched twice and the two values compared were the wrong pair.
/// It failed loudly, which is luck — a name that is a suffix of another and
/// carries a *similar* value would have compared two plausible numbers.
fn attributes<'a>(trace: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!(" {name}=\"");
    let mut found = Vec::new();
    let mut rest = trace;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let end = rest.find('"').expect("a closing quote");
        found.push(&rest[..end]);
        rest = &rest[end..];
    }
    found
}

/// Every element with this tag, one string per element, in document order.
fn elements<'a>(trace: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag} ");
    trace
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with(&open))
        .collect()
}

/// One element's attribute.
fn attribute<'a>(element: &'a str, name: &str) -> &'a str {
    let values = attributes(element, name);
    assert_eq!(
        values.len(),
        1,
        "`{name}` appears {} times in `{element}`",
        values.len()
    );
    values[0]
}

/// The numbers in a space-separated attribute — a matrix, a colour, a point.
fn numbers(value: &str) -> Vec<f64> {
    value
        .split_whitespace()
        .map(|n| n.parse::<f64>().unwrap_or_else(|_| panic!("a number: {n}")))
        .collect()
}

/// Two number lists agree to `tolerance`, with the whole of both printed when
/// they do not — a message naming only the first difference makes a
/// disagreement about *order* read as a disagreement about a value.
#[track_caller]
fn agree(what: &str, package: &str, document: &str, tolerance: f64) {
    let (left, right) = (numbers(package), numbers(document));
    assert_eq!(
        left.len(),
        right.len(),
        "{what}: mutool reads {} numbers from the package (`{package}`) and {} \
         from the document (`{document}`)",
        left.len(),
        right.len(),
    );
    for (a, b) in left.iter().zip(&right) {
        assert!(
            (a - b).abs() <= tolerance,
            "{what}: the package says `{package}` and the document says \
             `{document}`, which differ by more than {tolerance}",
        );
    }
}

// ---- the page ------------------------------------------------------------

/// mutool reads the same page count and the same page sizes from the package
/// and from the document this engine made of it.
///
/// **This is the original defect, asserted from outside.** Before milestone 3
/// `wpf-image-and-text.xps` opened as a one-page *comic* whose single page was
/// 32 × 32 points — the 4 × 4 pt measurement in the plan's opening section is
/// the same failure at a smaller resolution — and every test that could see it
/// was this engine reading its own output. mutool reads the package by a route
/// that shares no code with this one and states the page box in points, so a
/// build that went back to paging resources disagrees here about a number a
/// third party computed.
///
/// Five packages, spanning both dialects and both producers, because milestone
/// 4's criterion is that page geometry comes from `Width × 0.75` by
/// `Height × 0.75` and one Letter-sized fixture cannot tell that from a
/// hardcoded 612 × 792.
#[test]
fn mutool_reads_the_same_pages_at_the_same_sizes_from_the_package_and_from_the_document() {
    let mutool = oracle!("page sizes, package against document");
    let dir = scratch("pages");

    for name in [
        "wpf-image-and-text.xps",
        "wpf-shapes-only.xps",
        "wpf-three-pages.xps",
        "wpf-tiled-brush.xps",
        "xpsom-gradients.oxps",
    ] {
        let from_package = trace(&mutool, &package(&dir, name));
        let from_document = trace(&mutool, &synthesised(&dir, name));

        let boxes = attributes(&from_package, "mediabox");
        assert!(!boxes.is_empty(), "{name}: mutool found no page at all");
        assert_eq!(
            boxes,
            attributes(&from_document, "mediabox"),
            "{name}: mutool reads different pages from the package and from \
             the document"
        );
        // And the sizes are the ones ECMA-388 18.1 gives, rather than merely
        // equal to each other: two wrong readings that agree are the failure
        // an oracle exists to prevent, and this corpus is entirely Letter.
        for page in &boxes {
            agree(&format!("{name}: the page box"), "0 0 612 792", page, 0.001);
        }
    }

    // The multi-page package really is multi-page, so the loop above is not
    // comparing one page with itself five times.
    let from_package = trace(&mutool, &package(&dir, "wpf-three-pages.xps"));
    assert_eq!(
        attributes(&from_package, "mediabox").len(),
        3,
        "the three-page package has three pages"
    );
}

// ---- what is on it -------------------------------------------------------

/// mutool fills the same paths, in the same order, in the same colours, under
/// the same matrices.
///
/// `wpf-shapes-only.xps` is the package milestone 1 committed for the second of
/// its two pinned failures — it holds no image part at all, which is what used
/// to make it `NoImages` — and it is three filled `<Path>` elements and nothing
/// else. So a disagreement here is about geometry rather than about anything
/// downstream of it: 11.2.3's abbreviated `Data` grammar, the transform
/// composition order, and 18.1's flip, each read by a program that shares no
/// line of code with this one.
///
/// The colour tolerance is 1/10 000 because this engine writes four decimal
/// places into the PDF and the markup carries a byte: mutool prints
/// `.27450983` for the package and `.2745` for the document, and those are the
/// same colour.
#[test]
fn mutool_fills_the_same_paths_in_the_same_colours_under_the_same_matrices() {
    let mutool = oracle!("fill_path, package against document");
    let dir = scratch("fills");

    for name in ["wpf-shapes-only.xps", "wpf-three-pages.xps"] {
        let (package_trace, document_trace) = (
            trace(&mutool, &package(&dir, name)),
            trace(&mutool, &synthesised(&dir, name)),
        );
        let from_package = elements(&package_trace, "fill_path");
        let from_document = elements(&document_trace, "fill_path");

        assert!(
            !from_package.is_empty(),
            "{name}: mutool filled nothing at all, so this test is measuring \
             its own emptiness"
        );
        assert_eq!(
            from_package.len(),
            from_document.len(),
            "{name}: mutool fills {} paths from the package and {} from the \
             document",
            from_package.len(),
            from_document.len(),
        );

        for (index, (package_fill, document_fill)) in
            from_package.iter().zip(&from_document).enumerate()
        {
            agree(
                &format!("{name}: fill {index}'s colour"),
                attribute(package_fill, "color"),
                attribute(document_fill, "color"),
                0.000_1,
            );
            // Sub-hundredth of a point: the numbers here are exact in both
            // traces and the tolerance exists only so that a future writer
            // rounding a matrix differently is not a failure.
            agree(
                &format!("{name}: fill {index}'s matrix"),
                attribute(package_fill, "transform"),
                attribute(document_fill, "transform"),
                0.005,
            );
            assert_eq!(
                attribute(package_fill, "colorspace"),
                attribute(document_fill, "colorspace"),
                "{name}: fill {index} is in a different space"
            );
        }
    }
}

/// mutool reads the same two gradients from the package and from the document.
///
/// The **OpenXPS** package, deliberately: `xpsom-gradients.oxps` was written by
/// the XPS Object Factory rather than by WPF, so this is the one comparison
/// here whose input came from a second Microsoft component, and decision 3's
/// claim — that both dialects are read — is being checked by a third party.
///
/// What makes this the sharpest assertion in the file is that mutool prints a
/// shading's *own parameters*: `type`, the axis endpoints for a linear one and
/// both circles for a radial one. Milestone 5 wrote `/Shading` types 2 and 3
/// and milestone 6 turned `LinearGradientBrush` and `RadialGradientBrush` into
/// them; if the mapping between the two vocabularies were wrong — an axis
/// reversed, a radius taken from the wrong stop, `MappingMode="Absolute"`
/// treated as relative — the page would still be a gradient and would still
/// look plausible. These numbers are the thing that is not plausible-looking.
#[test]
fn mutool_reads_the_same_gradients_from_the_package_and_from_the_document() {
    let mutool = oracle!("fill_shade, package against document");
    let dir = scratch("gradients");

    for name in ["xpsom-gradients.oxps", "wpf-gradients.xps"] {
        let (package_trace, document_trace) = (
            trace(&mutool, &package(&dir, name)),
            trace(&mutool, &synthesised(&dir, name)),
        );
        let from_package = elements(&package_trace, "fill_shade");
        let from_document = elements(&document_trace, "fill_shade");

        assert_eq!(
            from_package.len(),
            2,
            "{name}: mutool reads two gradients from the package"
        );
        assert_eq!(
            from_document.len(),
            2,
            "{name}: mutool reads two gradients from the document"
        );

        for (index, (package_shade, document_shade)) in
            from_package.iter().zip(&from_document).enumerate()
        {
            let kind = attribute(package_shade, "type");
            assert_eq!(
                kind,
                attribute(document_shade, "type"),
                "{name}: shading {index} is a different kind of gradient"
            );
            // The parameters that name the geometry, per kind. A linear
            // shading has an axis; a radial one has two circles. Both are in
            // the shading's own space, which is what the pattern matrix beside
            // them is for.
            let geometry: &[&str] = match kind {
                "linear" => &["start", "end", "extend"],
                "radial" => &["inner", "outer", "extend"],
                other => panic!("{name}: mutool calls shading {index} a `{other}`"),
            };
            for key in geometry {
                agree(
                    &format!("{name}: shading {index}'s `{key}`"),
                    attribute(package_shade, key),
                    attribute(document_shade, key),
                    0.005,
                );
            }
            agree(
                &format!("{name}: shading {index}'s matrix"),
                attribute(package_shade, "transform"),
                attribute(document_shade, "transform"),
                0.005,
            );
        }
    }
}

/// mutool addresses the **same glyphs** at the same places from the package and
/// from the document.
///
/// This is the assertion milestone 7 could not make and milestone 5 exists for.
/// `Indices=",53"` names a glyph *by index* in an obfuscated Cascadia Mono, and
/// the only way a PDF can address a glyph by index is 9.7's Type0 font with
/// `/Encoding /Identity-H` and `/CIDToGIDMap /Identity`. mutool reads the
/// package's own ODTTF and the document's own subset — two different fonts, by
/// two different routes — and if this engine's `Indices` arithmetic, its
/// de-obfuscation key or its `/CIDToGIDMap` were wrong, the glyph *numbers*
/// here would differ while both pages still drew letters.
///
/// The `unicode` attribute is compared too, and it is a second claim rather
/// than a restatement of the first: on the document's side it comes from the
/// `/ToUnicode` this engine wrote, which is what `Page::text()` reads, so the
/// pair together says the glyph is right **and** that the text under it is.
///
/// The font *name* is deliberately not compared: the document's is
/// `WZKFDK+XpsFont`, a 9.6.4 subset tag over a face this engine subsetted, and
/// the package's is `Cascadia Mono Roman`. A test that asserted those were
/// equal would be asserting that the subsetter does not run.
#[test]
fn mutool_addresses_the_same_glyphs_at_the_same_points_from_both() {
    let mutool = oracle!("fill_text, package against document");
    let dir = scratch("glyphs");
    let name = "wpf-image-and-text.xps";

    let from_package = trace(&mutool, &package(&dir, name));
    let from_document = trace(&mutool, &synthesised(&dir, name));

    let (package_glyphs, document_glyphs) =
        (elements(&from_package, "g"), elements(&from_document, "g"));
    assert_eq!(
        package_glyphs.len(),
        8,
        "the run is the eight characters of `Page one`: {package_glyphs:?}"
    );
    assert_eq!(
        package_glyphs.len(),
        document_glyphs.len(),
        "mutool draws {} glyphs from the package and {} from the document",
        package_glyphs.len(),
        document_glyphs.len(),
    );

    for (index, (package_glyph, document_glyph)) in
        package_glyphs.iter().zip(&document_glyphs).enumerate()
    {
        assert_eq!(
            attribute(package_glyph, "glyph"),
            attribute(document_glyph, "glyph"),
            "glyph {index} is a different glyph of the face"
        );
        assert_eq!(
            attribute(package_glyph, "unicode"),
            attribute(document_glyph, "unicode"),
            "glyph {index} maps back to a different character"
        );
        // A twentieth of a point. The advance that puts the next glyph here is
        // read from the face's own `hmtx` on one side and from the `/W` array
        // this engine wrote from that same table on the other, so a drift
        // would accumulate along the run rather than appear at one glyph.
        for key in ["x", "y"] {
            agree(
                &format!("glyph {index}'s `{key}`"),
                attribute(package_glyph, key),
                attribute(document_glyph, key),
                0.05,
            );
        }
    }

    // And the text matrix, which is where the em size lands: `trm="24 0 0 -24"`
    // on both sides is 12.1's `RenderTransform` and `FontRenderingEmSize`
    // arriving through two unrelated readers.
    let spans = |trace: &str| {
        elements(trace, "span")
            .iter()
            .map(|span| attribute(span, "trm").to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        spans(&from_package),
        spans(&from_document),
        "the run is set at a different size"
    );
}

/// mutool puts the image in the same place, at the same size, from both.
///
/// Milestone 8's own defect, from outside. The `ImageBrush`'s pattern matrix
/// was composed outermost-first and put the picture 4 336 points down a
/// 792-point page — off the sheet, with the text still on it, which reads
/// exactly like "the image brush is not implemented yet". mutool computes the
/// image's placement from the package by its own route, so a composition-order
/// mistake is a disagreement about a rectangle rather than a page that looks
/// unfinished.
///
/// **The comparison is the device-space box, not the matrix**, and the
/// difference matters: mutool traces the package's image at
/// `149.979 0 0 149.979 75 75` and the document's at
/// `150.0 0 0 -150.0 75 225.0`, which are the same rectangle drawn from
/// opposite corners. PDF's image space is the unit square with its origin at
/// the bottom left and XPS's is not, so the `/Matrix` this engine writes must
/// carry a flip that the package's does not — asserting the six numbers were
/// equal would be asserting the flip is absent.
#[test]
fn mutool_paints_the_image_over_the_same_rectangle_from_both() {
    let mutool = oracle!("fill_image, package against document");
    let dir = scratch("image");

    for name in ["wpf-image-and-text.xps", "wpf-jpeg-image.xps"] {
        let (package_trace, document_trace) = (
            trace(&mutool, &package(&dir, name)),
            trace(&mutool, &synthesised(&dir, name)),
        );
        let from_package = elements(&package_trace, "fill_image");
        let from_document = elements(&document_trace, "fill_image");

        assert!(
            !from_package.is_empty(),
            "{name}: mutool painted no image at all"
        );
        assert_eq!(
            from_package.len(),
            from_document.len(),
            "{name}: mutool paints {} images from the package and {} from the \
             document",
            from_package.len(),
            from_document.len(),
        );

        for (index, (package_image, document_image)) in
            from_package.iter().zip(&from_document).enumerate()
        {
            // The picture's own pixels, which is the pass-through's claim: the
            // part reaches the page at the size it already was, never resampled
            // to the page.
            for key in ["width", "height"] {
                assert_eq!(
                    attribute(package_image, key),
                    attribute(document_image, key),
                    "{name}: image {index} is a different number of pixels {key}"
                );
            }

            // A quarter of a point over a 150-point square, which is 1 part in
            // 600. The two traces disagree by 0.02 points on this corpus and
            // the reason is 13.4.1: the viewbox is stated in the image's own
            // units and this engine derives those from the part's `pHYs`,
            // where mutool derives them from its own default. Neither is
            // wrong and a tolerance is the honest way to say so.
            let box_of = |image: &str| {
                let m = numbers(attribute(image, "transform"));
                let (x0, y0) = (m[4], m[5]);
                let (x1, y1) = (m[4] + m[0] + m[2], m[5] + m[1] + m[3]);
                vec![x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
            };
            let (package_box, document_box) = (box_of(package_image), box_of(document_image));
            for (a, b) in package_box.iter().zip(&document_box) {
                assert!(
                    (a - b).abs() <= 0.25,
                    "{name}: image {index} covers {package_box:?} in the \
                     package and {document_box:?} in the document"
                );
            }
        }
    }
}
