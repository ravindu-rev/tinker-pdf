//! What a page with a large raster costs to synthesise (gap 30, milestone 9).
//!
//! # Why this file exists rather than an arithmetic claim
//!
//! Gap 29 closed with its peak-memory figure **unmeasured**: its milestone 6
//! recorded that the 3.6 GB the design turns on is arithmetic, that no 200-page
//! archive was ever opened, and that the claim is therefore that the design
//! *cannot* hold a raster per page rather than that anybody watched it not.
//! This gap inherits the same pass-through and the same claim, and milestone 1's
//! corpus cannot test it: every image Windows put in those eight packages is
//! 32 x 32, so a build that decoded every one of them would cost a few kilobytes
//! more and no measurement could tell.
//!
//! So the fixture here is **hand-built and large**, and the thing it measures is
//! memory rather than fidelity — which is the one claim a hand-built package can
//! carry honestly, because a raster's size does not depend on who wrote the
//! markup around it.
//!
//! # What is asserted
//!
//! `--ignored`, and it writes a package to `CARGO_TARGET_TMPDIR` for the
//! measurement recorded in the plan. The assertion that runs on every build is
//! the cheap half: the synthesised document is **smaller than the decoded
//! raster would be**, which is a statement about the bytes rather than about the
//! allocator, and it is what the pass-through actually promises.

mod xps_support;

use tinker_pdf::{Document, WriteMode, WriteOptions};
use xps_support::{archive, before_content_types, binary_part, one_page_package, rgb_png, with};

/// A page whose whole content is one large image.
const WIDE: u32 = 2000;
const TALL: u32 = 3000;

/// The package, with a `WIDE` x `TALL` PNG behind a `{StaticResource}`.
fn big_package() -> Vec<u8> {
    // A gradient with structure in it, which is what a scanned page or a
    // photograph looks like to a compressor: neighbouring pixels are close, so
    // PNG's row filters and DEFLATE both have something to work with.
    //
    // **Not noise.** The first version of this fixture used a linear
    // congruential generator and the assertion failed at 18 005 473 bytes
    // against 18 000 000 -- correctly. Incompressible data is the one case
    // where the pass-through and the decode cost the same, because the IDAT
    // *is* the raster. That is a true property of the claim rather than a
    // defect in it, and it is recorded here so the next reader does not
    // "fix" this fixture back into noise.
    let mut pixels = Vec::with_capacity((WIDE * TALL * 3) as usize);
    for y in 0..TALL {
        for x in 0..WIDE {
            pixels.push((x * 255 / WIDE) as u8);
            pixels.push((y * 255 / TALL) as u8);
            pixels.push(((x + y) * 255 / (WIDE + TALL)) as u8);
        }
    }
    let png = rgb_png(WIDE, TALL, &pixels);

    let ns = xps_support::XPS_NS;
    let body = r#"<Path Data="M0,0L816,0 816,1056 0,1056Z"><Path.Fill>
        <ImageBrush ImageSource="/Resources/big.png" ViewboxUnits="Absolute"
                    ViewportUnits="Absolute" Viewbox="0,0,2000,3000"
                    Viewport="0,0,816,1056" TileMode="None" />
        </Path.Fill></Path>"#;
    let markup = format!(r#"<FixedPage xmlns="{ns}" Width="816" Height="1056">{body}</FixedPage>"#);
    let parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
    archive(before_content_types(
        parts,
        binary_part("Resources/big.png", png),
    ))
}

/// The synthesised document is smaller than the decoded raster would be.
///
/// *w × h × 3* for this page is 18 MB. A build that decoded the PNG and embedded
/// samples would produce a document at least that large whatever else it did,
/// so a document under it is one that did not decode — which is the
/// pass-through, stated in bytes rather than in allocator behaviour.
///
/// This is the assertion gap 29 could have made and did not, and it is cheap
/// enough to run on every build.
#[test]
fn a_page_of_one_large_image_does_not_cost_its_decoded_size() {
    let package = big_package();
    let document = Document::open(package.clone()).expect("an XPS");
    let saved = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });

    let decoded = (WIDE as usize) * (TALL as usize) * 3;
    assert!(
        saved.len() < decoded,
        "the document is {} bytes and the decoded raster alone would be {decoded}",
        saved.len()
    );
    // And it is within a small multiple of the package, which is the positive
    // form of the same claim: the picture arrived as the bytes the part held.
    assert!(
        saved.len() < package.len() * 3,
        "the document is {} bytes over a {}-byte package",
        saved.len(),
        package.len()
    );
    // The margin is worth an eye rather than only a bound: a page of six
    // million samples that costs well under a tenth of them is one whose
    // picture was never expanded.
    assert!(
        saved.len() * 10 < decoded,
        "the document is {} bytes against a {decoded}-byte raster, which is not          the order of magnitude a pass-through gives",
        saved.len()
    );
}

/// Writes the package where the plan's peak-memory measurement can find it.
#[test]
#[ignore = "writes a fixture for the measurement recorded in gap 30's plan"]
fn write_the_large_package_for_measurement() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("xps-memory");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("big.xps");
    std::fs::write(&path, big_package()).expect("written");
    println!("xps-memory-fixture: {}", path.display());
}
