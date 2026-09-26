//! The `RenderOptions` fields that change what a page's bytes *are* rather than
//! which page or how large: ink instead of light, and what a pixel means when
//! it is not fully painted.
//!
//! # Why a file of its own, and why these hashes are not in `determinism.rs`
//!
//! `determinism.rs` pins nineteen pages rendered with `RenderOptions::default()`
//! and nothing else: its `Fixture` has a builder, a way to open and an ink floor,
//! and no options, and the table it checks is the one thing in the repository a
//! determinism bug would show up in. Growing an options column there would
//! change that file's shape to add a row, and a row that means "this page, but
//! differently" beside nineteen that mean "this page" is how a reader of that
//! table gets the wrong idea about what moved.
//!
//! So each option carries its own pinned hash here, computed the way that file
//! computes one — SHA-256 over the width and height, big-endian, and the pixel
//! bytes — over pages `render_support` builds, which are the same bytes that
//! file's `analytic_*` rows hash. And each option has the claim that matters
//! most beside it: **the default is unchanged**. That is held two ways: the
//! default render of the blend grid still hashes to the value `determinism.rs`
//! pins for `analytic_blend`, copied here verbatim, and the option spelled out
//! at its default renders the same bytes as not mentioning it.
//!
//! A mismatch prints the replacement line, as `determinism.rs` does. The same
//! rule applies: two targets disagreeing is a determinism bug and the table is
//! not to be edited to make it go away.

mod render_support;

use render_support::{blend_grid_page, curvy_font};
use tinker_pdf::{
    Bitmap, BlendMode, Document, DocumentBuilder, ExtGState, ImageData, PixelFormat, RenderOptions,
};

/// `determinism.rs`'s `analytic_blend` row, copied: the blend grid rendered
/// with the default options. If this and that file ever disagree, one of them
/// was edited without the other.
const ANALYTIC_BLEND_DEFAULT: &str =
    "e1d056a15ae8cb7f18fd1524dfcc419893f4a5283c2f56b3f7307380aee43560";

/// The hash `determinism.rs` takes: dimensions, then bytes.
fn fingerprint(bitmap: &Bitmap) -> String {
    let mut input = Vec::with_capacity(bitmap.data.len() + 8);
    input.extend_from_slice(&bitmap.width.to_be_bytes());
    input.extend_from_slice(&bitmap.height.to_be_bytes());
    input.extend_from_slice(&bitmap.data);
    tinker_pdf_crypto::sha2::sha256(&input)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn render(bytes: Vec<u8>, options: &RenderOptions) -> Bitmap {
    Document::open(bytes)
        .expect("the document opens")
        .page(0)
        .expect("a page")
        .render(options)
}

/// Pixels something was painted on: any colour channel off the background,
/// which is no ink for `CmykA8` and white for every other format.
fn painted(bitmap: &Bitmap) -> usize {
    let components = bitmap.components();
    bitmap
        .data
        .chunks_exact(components)
        .filter(|pixel| match bitmap.format {
            PixelFormat::CmykA8 => pixel[..4].iter().any(|v| *v != 0),
            PixelFormat::Gray8 | PixelFormat::Rgb8 => pixel.iter().any(|v| *v != 255),
            _ => pixel[..components - 1].iter().any(|v| *v != 255),
        })
        .count()
}

/// Asserts a pinned hash, printing the line to paste if it moved — after
/// asserting the page painted at least `least` pixels, because a hash of a
/// page that stopped drawing is as stable as any other (`determinism.rs`'s
/// header tells the story of the fixture that was blank for months).
fn pinned(name: &str, bitmap: &Bitmap, least: usize, want: &str) {
    let drawn = painted(bitmap);
    assert!(
        drawn >= least,
        "the `{name}` render painted {drawn} pixels, fewer than {least}: it is \
         measuring less than it claims. Warnings: {:?}",
        bitmap.warnings
    );
    let actual = fingerprint(bitmap);
    assert_eq!(
        actual, want,
        "the `{name}` render moved.\n\n\
         If two targets disagree, this is a determinism bug: find the \
         arithmetic that is not target-stable and do not update the value.\n\
         If this is a deliberate rendering change, say what moved in the \
         commit and paste:\n\n    \"{actual}\""
    );
}

/// Text in an embedded face, a DeviceCMYK fill, a rich black, a pure-K fill,
/// and a translucent `/Multiply` band laid over all three: the page whose ink
/// is worth looking at.
fn ink_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    assert!(builder.add_ext_gstate(
        b"G0",
        &ExtGState {
            fill_alpha: Some(0.5),
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    builder.add_page(96.0, 48.0, |page| {
        page.text(b"F0", 12.0, 4.0, 30.0, "ink 0123");
        page.raw(b"1 0 0 0 k 4 4 20 20 re f");
        page.raw(b"1 1 1 1 k 28 4 20 20 re f");
        page.raw(b"0 0 0 1 k 52 4 20 20 re f");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"G0"));
        page.raw(b"0 1 0 0 k 14 10 50 14 re f");
        page.raw(b"Q");
    });
    builder.finish()
}

// ---- CMYK page output --------------------------------------------------------

/// Asking for ink and saying so gets ink: five bytes a pixel, the canvas
/// starting with none, and each fill's components named — `1 0 0 0 k` is
/// exactly cyan because 8.6.4.4 relates it to `(0, 255, 255)` and maximum
/// undercolour removal relates that straight back.
#[test]
fn a_page_asked_for_in_ink_with_the_opt_in_comes_back_in_ink() {
    let ink = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            ..RenderOptions::default()
        },
    );
    assert_eq!(ink.format, PixelFormat::CmykA8);
    assert_eq!(ink.components(), 5);
    assert_eq!(ink.stride, ink.width as usize * 5);

    let at = |x: u32, y: u32| -> [u8; 5] {
        let i = y as usize * ink.stride + x as usize * 5;
        ink.data[i..i + 5].try_into().expect("five bytes")
    };
    // Page space is 48 points tall at one pixel a point, so y flips.
    assert_eq!(
        at(1, 1),
        [0, 0, 0, 0, 255],
        "no ink where nothing is painted"
    );
    assert_eq!(at(14, 40), [255, 0, 0, 0, 255], "cyan is cyan");
    assert_eq!(at(62, 40), [0, 0, 0, 255, 255], "black is K");
    // **The limitation, pinned so it cannot be forgotten**: a rich black is
    // light converted back, and comes back as the pure K of the same colour.
    assert_eq!(
        at(38, 40),
        [0, 0, 0, 255, 255],
        "a rich black arrives as pure K: the ink is light turned back into \
         ink, not the file's own components"
    );
}

/// Without the opt-in, ink is still light — which is what every caller that
/// asked for `CmykA8` before this switch existed got, and still gets.
#[test]
fn without_the_opt_in_a_page_asked_for_in_ink_is_still_light() {
    let light = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            ..RenderOptions::default()
        },
    );
    assert_eq!(light.format, PixelFormat::Rgba8);
}

/// The opt-in changes nothing for any format but ink.
#[test]
fn the_opt_in_changes_nothing_for_a_format_that_is_not_ink() {
    for format in [
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
        PixelFormat::LabA8,
    ] {
        let plain = render(
            ink_page(),
            &RenderOptions {
                format,
                ..RenderOptions::default()
            },
        );
        let opted = render(
            ink_page(),
            &RenderOptions {
                format,
                allow_cmyk: true,
                ..RenderOptions::default()
            },
        );
        assert_eq!(opted.format, plain.format, "{format:?}");
        assert_eq!(opted.data, plain.data, "{format:?}");
    }
}

/// `to_png` writes an ink page as the light it stands for, and that light is
/// byte for byte what the same render returns without the switch: one
/// conversion, 8.6.4.4's, reached two ways.
#[test]
fn an_ink_page_written_as_png_is_the_light_the_switch_would_have_returned() {
    for page in [ink_page(), blend_grid_page()] {
        let ink = render(
            page.clone(),
            &RenderOptions {
                format: PixelFormat::CmykA8,
                allow_cmyk: true,
                ..RenderOptions::default()
            },
        );
        let light = render(
            page,
            &RenderOptions {
                format: PixelFormat::CmykA8,
                ..RenderOptions::default()
            },
        );
        let read = Bitmap::from_png(&ink.to_png().expect("a picture")).expect("it reads");
        assert_eq!(read.format, PixelFormat::Rgba8, "PNG has no CMYK");
        assert_eq!(read.data, light.data);
    }
}

/// **The fingerprint.** Two pages in ink: the blend grid, whose twelve modes
/// are 11.3.5's formulas over complemented components here rather than over
/// light, and the ink page.
#[test]
fn ink_output_is_pinned() {
    let options = RenderOptions {
        format: PixelFormat::CmykA8,
        allow_cmyk: true,
        ..RenderOptions::default()
    };
    // 1 656 of 1 728 pixels and 1 562 of 4 608 today; each floor is about
    // half, as `determinism.rs`'s are.
    pinned(
        "blend grid, in ink",
        &render(blend_grid_page(), &options),
        800,
        "a8c768d45da8dbf2a9f9457f6c10e3f112576def13c28502a3a0b5fa6c03a418",
    );
    pinned(
        "ink page, in ink",
        &render(ink_page(), &options),
        780,
        "cd3c60cc88514aaa0ac5a0ce2eea1d7b26cc8b0d6412f2c9fc824c0a42e8788b",
    );
}

/// **The default is unchanged.** The blend grid rendered with the default
/// options is still `determinism.rs`'s `analytic_blend`; the hard-edge page's
/// default render — every kind of edge this file hardens, anti-aliased — is
/// pinned beside it; and every switch spelled out at its default renders the
/// same bytes as not mentioning it.
///
/// The nineteen fingerprints in `determinism.rs` are the rest of this claim,
/// and the stronger half: they render with the defaults and did not move.
#[test]
fn the_default_is_unchanged() {
    let spelled = RenderOptions {
        allow_cmyk: false,
        antialias: true,
        ..RenderOptions::default()
    };
    for (name, page, least, want) in [
        ("blend grid", blend_grid_page(), 800, ANALYTIC_BLEND_DEFAULT),
        (
            "hard-edge page",
            hard_edge_page(),
            1100,
            "03dbe46995a5a1bdf69376e57a5e67de9ea24c8be8f3d19b41b2265bb679812c",
        ),
    ] {
        let default = render(page.clone(), &RenderOptions::default());
        pinned(&format!("{name}, default"), &default, least, want);
        let explicit = render(page, &spelled);
        assert_eq!(explicit.data, default.data, "{name}");
        assert_eq!(explicit.format, PixelFormat::Rgb8, "{name}");
    }
}

// ---- the anti-aliasing switch -------------------------------------------------

/// The image the hard-edge page places: two by two samples of one green, so
/// every pixel it covers is exactly that green whatever the sampler does
/// inside it, and a pixel of any other colour is an edge.
const GREEN: [u8; 3] = [0, 160, 40];

/// Text, a diagonal fill, a stroked curve, a hairline and a rotated image's
/// edge, each in its own opaque colour and none touching another — the
/// fixture the switch's exit criterion names.
///
/// Every colour is pure and every element opaque, so on a hard-edged render a
/// pixel has exactly six possible values: the page's white or one element's
/// colour. Anything else is a pixel something covered partly.
fn hard_edge_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    let samples = GREEN.repeat(4);
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &samples,
        }
    ));
    builder.add_page(160.0, 100.0, |page| {
        // Text, in black: glyph coverage, the densest source of partial
        // pixels there is.
        page.text(b"F0", 14.0, 4.0, 78.0, "fox 0123");
        // A diagonal fill, in red. Closed with `h` rather than left open:
        // this engine does not yet close an open subpath before filling or
        // clipping (8.5.3.3), and a fixture about edges should not lean on it.
        page.raw(b"1 0 0 rg 100 45 m 150 75 l 140 40 l h f");
        // A stroked curve, in blue.
        page.raw(b"0 0 1 RG 2.3 w 10 10 m 30 45 60 5 80 30 c S");
        // A hairline at a shallow angle, in black: the thinnest stroke there
        // is, which a hard edge must not lose.
        page.raw(b"0 0 0 RG 0 w 5 97 m 155 94 l S");
        // An image's edge, rotated so none of it is axis-aligned, in green.
        page.raw(b"q 20 12 -12 20 95 5 cm /Im0 Do Q");
    });
    builder.finish()
}

/// The colours a hard-edged render of [`hard_edge_page`] may hold.
const HARD_COLOURS: [[u8; 3]; 5] = [[255, 255, 255], [0, 0, 0], [255, 0, 0], [0, 0, 255], GREEN];

/// Pixels whose colour is not one of `HARD_COLOURS` — each one a pixel
/// something covered partly.
fn partial_pixels(bitmap: &Bitmap) -> Vec<(u32, u32, [u8; 3])> {
    let mut out = Vec::new();
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let i = y as usize * bitmap.stride + x as usize * 3;
            let pixel = [bitmap.data[i], bitmap.data[i + 1], bitmap.data[i + 2]];
            if !HARD_COLOURS.contains(&pixel) {
                out.push((x, y, pixel));
            }
        }
    }
    out
}

fn count(bitmap: &Bitmap, colour: [u8; 3]) -> usize {
    bitmap
        .data
        .chunks_exact(3)
        .filter(|pixel| **pixel == colour)
        .count()
}

/// **The exit criterion.** Every pixel of an anti-aliasing-off render of text,
/// a diagonal fill, a stroked curve, a hairline and an image edge is fully
/// covered or fully uncovered — and the same page with anti-aliasing on is
/// not, so the fixture has edges in it to harden.
///
/// Each element is also required to be *present*, by its own colour, so that
/// a switch that hardened by painting nothing would fail here rather than
/// pass: an empty page is fully uncovered everywhere.
#[test]
fn with_anti_aliasing_off_every_pixel_is_wholly_covered_or_not_at_all() {
    for scale in [1.0, 1.7, 3.0] {
        let soft = render(
            hard_edge_page(),
            &RenderOptions {
                scale,
                ..RenderOptions::default()
            },
        );
        let hard = render(
            hard_edge_page(),
            &RenderOptions {
                scale,
                antialias: false,
                ..RenderOptions::default()
            },
        );
        assert!(
            soft.warnings.is_empty() && hard.warnings.is_empty(),
            "{:?} {:?}",
            soft.warnings,
            hard.warnings
        );
        assert!(
            partial_pixels(&soft).len() > 100,
            "at {scale}x the anti-aliased page has partial pixels to take away"
        );
        let partial = partial_pixels(&hard);
        assert!(
            partial.is_empty(),
            "at {scale}x, {} pixels of the hard-edged render are partly \
             covered; the first few: {:?}",
            partial.len(),
            &partial[..partial.len().min(8)]
        );
        // Each element painted, by its own colour. The floors are about half
        // of what each paints at 1x, scaled by area.
        let area = scale * scale;
        for (what, colour, least) in [
            ("text and hairline", [0, 0, 0], 180.0),
            ("the diagonal fill", [255, 0, 0], 300.0),
            ("the stroked curve", [0, 0, 255], 90.0),
            ("the image", GREEN, 270.0),
        ] {
            let painted = count(&hard, colour);
            assert!(
                painted as f64 >= least * area,
                "at {scale}x {what} painted {painted} pixels"
            );
        }
    }
}

/// The hairline survives: a zero-width stroke is at least a pixel wide with
/// anti-aliasing off, so a shallow line across the page leaves no column it
/// crosses empty — which a line eight tenths of a pixel wide, split across two
/// rows, would.
#[test]
fn a_hard_edged_hairline_leaves_no_column_it_crosses_empty() {
    // Slopes from level to one in one, each starting on a different phase.
    for (index, rise) in [0.0, 0.37, 1.5, 3.0, 11.0, 37.0, 60.0, 99.5]
        .iter()
        .enumerate()
    {
        let y0 = 20.3 + index as f64 * 0.13;
        let content = format!("0 w 10 {y0} m 110 {} l S", y0 + rise);
        let page = format!(
            "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
             2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
             3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 140] \
             /Contents 4 0 R >>\nendobj\n\
             4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
             trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
            content.len()
        );
        let hard = render(
            page.into_bytes(),
            &RenderOptions {
                antialias: false,
                ..RenderOptions::default()
            },
        );
        for x in 11..109u32 {
            let inked = (0..hard.height).any(|y| {
                let i = y as usize * hard.stride + x as usize * 3;
                hard.data[i] == 0
            });
            assert!(inked, "rise {rise}: column {x} of the hairline is empty");
        }
    }
}

/// Ruling 5 holds with the switch off: a tile of a hard-edged page is the
/// hard-edged page under it, byte for byte, because the threshold is a
/// function of one pixel's coverage and the coverage already agreed.
#[test]
fn a_hard_edged_tile_is_the_hard_edged_page_under_it() {
    let doc = Document::open(hard_edge_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions {
        antialias: false,
        ..RenderOptions::default()
    };
    let whole = page.render(&options);
    for (x, y, w, h) in [
        (0, 0, 37, 23),
        (37, 23, 37, 23),
        (90, 60, 70, 40),
        (3, 71, 64, 29),
    ] {
        let tile = page.render(&RenderOptions {
            region: Some(tinker_pdf::PixelRegion::new(x, y, w, h)),
            ..options.clone()
        });
        for row in 0..h {
            let from = (y + row) as usize * whole.stride + x as usize * 3;
            let want = &whole.data[from..from + w as usize * 3];
            let got = &tile.data[row as usize * tile.stride..][..w as usize * 3];
            assert_eq!(got, want, "tile ({x}, {y}) row {row}");
        }
    }
}

/// The shading paths and the tiling one, each in one flat colour so that any
/// pixel of any other colour is an edge: a type 4 mesh whose vertices all
/// carry the same colour, `sh` of an axial shading whose two ends agree
/// painted through a diagonal clip, the same shading as a pattern filling a
/// curved shape, and a tiling pattern whose cell is a diagonal band.
///
/// A mesh is rasterised by its own path (`draw_mesh`, one coverage buffer
/// for the whole mesh), `sh` paints whatever the clip covers, a shading
/// pattern paints whatever its shape covers, and a tiling cell is drawn by a
/// renderer of its own before it is composited — four different places a
/// coverage value is produced, which is why each gets a flat colour here
/// rather than the gradient that would hide an edge inside a ramp.
fn hard_edge_shadings_page() -> Vec<u8> {
    // One vertex a line: flag, x, y, r, g, b, eight bits each, so the stream
    // is byte-aligned and needs no packing. `/Decode` maps the coordinates
    // onto the page and the colour straight through.
    let vertices: [(u8, u8); 3] = [(15, 20), (110, 45), (45, 150)];
    let mut mesh = String::new();
    for (x, y) in vertices {
        for byte in [0u8, x, y, 0, 128, 255] {
            mesh.push_str(&format!("{byte:02x}"));
        }
    }
    mesh.push('>');
    let axial = "/ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 160 100] \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0.5 0] \
                 /C1 [1 0.5 0] /N 1 >>";
    let content = "q 90 10 m 150 30 l 110 60 l h W n /Sh1 sh Q\n\
                   /Pattern cs /P0 scn 95 75 m 115 99 145 90 150 70 c 120 62 l h f\n\
                   /P1 scn 5 65 65 30 re f";
    let cell = "0.5 0 0.5 rg 0 0 m 10 6 l 10 10 l 4 10 l h f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 100]\n\
   /Resources << /Shading << /Sh0 5 0 R /Sh1 << {axial} >> >>\n\
   /Pattern << /P0 << /PatternType 2 /Shading << {axial} >> >> /P1 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n/Sh0 sh\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
/BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 160 0 100 0 1 0 1 0 1] \
/Filter /ASCIIHexDecode /Length {} >>\nstream\n{mesh}\nendstream\nendobj\n\
6 0 obj\n<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 \
/YStep 10 /Resources << >> /Length {} >>\nstream\n{cell}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 8,
        mesh.len() + 1,
        cell.len() + 1,
    )
    .into_bytes()
}

/// The distinct colours a render holds, with how many pixels each.
fn palette(bitmap: &Bitmap) -> Vec<([u8; 3], usize)> {
    let mut out: Vec<([u8; 3], usize)> = Vec::new();
    for pixel in bitmap.data.chunks_exact(3) {
        let colour = [pixel[0], pixel[1], pixel[2]];
        match out.iter_mut().find(|(c, _)| *c == colour) {
            Some((_, n)) => *n += 1,
            None => out.push((colour, 1)),
        }
    }
    out
}

/// **The switch reaches every shading path, and the tiling one**: with
/// anti-aliasing off, the mesh, the clipped `sh`, the shading pattern and the
/// tiling pattern leave exactly four colours on the page — the white and three
/// flat colours — where the same page anti-aliased has dozens along the
/// silhouettes and inside the tiling cells.
#[test]
fn with_anti_aliasing_off_a_shading_is_wholly_covered_or_not_at_all() {
    for scale in [1.0, 2.3] {
        let soft = render(
            hard_edge_shadings_page(),
            &RenderOptions {
                scale,
                ..RenderOptions::default()
            },
        );
        let hard = render(
            hard_edge_shadings_page(),
            &RenderOptions {
                scale,
                antialias: false,
                ..RenderOptions::default()
            },
        );
        assert!(
            soft.warnings.is_empty() && hard.warnings.is_empty(),
            "{:?} {:?}",
            soft.warnings,
            hard.warnings
        );
        assert!(
            palette(&soft).len() > 20,
            "at {scale}x the anti-aliased silhouettes have partial pixels"
        );
        let colours = palette(&hard);
        assert_eq!(
            colours.len(),
            4,
            "at {scale}x a hard-edged page of three flat paints holds white \
             and three colours, not {colours:?}"
        );
        // Every paint, and each with real area behind it: at 1x the mesh is
        // 1 416 pixels, the clip and the shading pattern together 2 274 and
        // the tiling pattern 1 002, and each floor is about half.
        let area = scale * scale;
        for (colour, least) in [
            ([0u8, 128, 255], 700.0),
            ([255, 128, 0], 1100.0),
            ([128, 0, 128], 500.0),
        ] {
            let painted = colours
                .iter()
                .find(|(c, _)| *c == colour)
                .map_or(0, |(_, n)| *n);
            assert!(
                painted as f64 >= least * area,
                "at {scale}x {colour:?} painted {painted} pixels: {colours:?}"
            );
        }
    }
}

/// **The fingerprint.** The hard-edge page with anti-aliasing off, at 1x and
/// at 1.7x, where every edge lands on a different phase of the pixel grid.
#[test]
fn hard_edged_output_is_pinned() {
    // 1 780 and 4 983 pixels painted today; the floors are about half.
    for (scale, least, want) in [
        (
            1.0,
            890,
            "f407710585c1a5218877c3fb1f8689204f24d8835340c72ef5f313ea874c9561",
        ),
        (
            1.7,
            2490,
            "39d5aba7acb9e32ea92f05d49797e42499118e63cd66e9a7a6a19152b222401c",
        ),
    ] {
        pinned(
            &format!("hard-edge page at {scale}x, anti-aliasing off"),
            &render(
                hard_edge_page(),
                &RenderOptions {
                    scale,
                    antialias: false,
                    ..RenderOptions::default()
                },
            ),
            least,
            want,
        );
    }
}
