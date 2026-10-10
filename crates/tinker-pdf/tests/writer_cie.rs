//! `/CalGray`, `/CalRGB` and `/Lab` on write, read back through this
//! repository's own reader and drawn by its own renderer, and held to ISO
//! 32000-1's arithmetic for each (8.6.5.2, 8.6.5.3, 8.6.5.4).
//!
//! The clauses take a colour to CIE XYZ relative to the space's white point,
//! and every expectation below is computed that way, here, from the clause's
//! formulas. What the clauses do not say is how XYZ becomes a pixel, and the
//! fixtures are chosen so that step needs as little as possible:
//!
//! - a colour **neutral to its own white point** — every `/CalGray` colour,
//!   and a `/CalRGB` whose matrix columns are multiples of the white — is a
//!   grey of luminance `Y` on any display that maps white to white, so the
//!   pixel is IEC 61966-2-1's transfer function of `Y` on every channel, and
//!   nothing else;
//! - a chromatic `/Lab` colour needs XYZ at D50 taken to sRGB, and that is
//!   the published Bradford-adapted sRGB matrix for a D50 white (Lindbloom's
//!   table, the ICC v4 sRGB profile's own primaries) — quoted here, not
//!   copied from the renderer, whose constants differ from it in the fourth
//!   decimal; so those comparisons allow one level.
//!
//! Every pixel compared is inside a filled shape or a sample's own square,
//! fully covered and opaque.

use tinker_pdf::{
    CieSpace, CompressedImage, Document, DocumentBuilder, ImageColorSpace, ImageData, ImageSpace,
};

mod render_support;
use render_support::{pixel, render};

/// The page every fixture draws on, in points and pixels.
const PAGE: f64 = 60.0;

/// The pixel whose centre is at `(x, y)` in user space.
fn at(bitmap: &tinker_pdf::Bitmap, x: f64, y: f64) -> (u8, u8, u8) {
    pixel(bitmap, x as u32, (PAGE - y) as u32)
}

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const D65: [f64; 3] = [0.9505, 1.0, 1.089];

/// IEC 61966-2-1's transfer function: linear light to an sRGB byte.
fn srgb(linear: f64) -> u8 {
    let v = linear.clamp(0.0, 1.0);
    let encoded = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// A grey of luminance `y`, as every channel carries it.
fn grey(y: f64) -> (u8, u8, u8) {
    let v = srgb(y);
    (v, v, v)
}

/// XYZ relative to a D50 white, as sRGB: the Bradford-adapted sRGB matrix
/// for a D50 reference white, as published, then the transfer function.
fn d50_to_srgb(xyz: [f64; 3]) -> (u8, u8, u8) {
    const M: [[f64; 3]; 3] = [
        [3.133_856_1, -1.616_866_7, -0.490_614_6],
        [-0.978_768_4, 1.916_141_5, 0.033_454_0],
        [0.071_945_3, -0.228_991_4, 1.405_242_7],
    ];
    let row = |r: [f64; 3]| r[0] * xyz[0] + r[1] * xyz[1] + r[2] * xyz[2];
    (srgb(row(M[0])), srgb(row(M[1])), srgb(row(M[2])))
}

/// 8.6.5.4: `L*a*b*` to XYZ relative to `white`.
fn lab_xyz(l: f64, a: f64, b: f64, white: [f64; 3]) -> [f64; 3] {
    let g = |x: f64| {
        if x >= 6.0 / 29.0 {
            x * x * x
        } else {
            108.0 / 841.0 * (x - 4.0 / 29.0)
        }
    };
    let m = (l + 16.0) / 116.0;
    [
        white[0] * g(m + a / 500.0),
        white[1] * g(m),
        white[2] * g(m - b / 200.0),
    ]
}

/// Within one level per channel.
#[track_caller]
fn near(got: (u8, u8, u8), want: (u8, u8, u8), what: &str) {
    let close = |a: u8, b: u8| a.abs_diff(b) <= 1;
    assert!(
        close(got.0, want.0) && close(got.1, want.1) && close(got.2, want.2),
        "{what}: drawn {got:?}, the clause gives {want:?}"
    );
}

/// The three spaces every test registers.
///
/// The gray's gamma is 1.8 and not the commoner 2.2 on purpose: sRGB's
/// transfer function is close to a 2.2 power, so a `/CalGray` of gamma 2.2
/// misread as `/DeviceGray` lands on the same byte at 0.5 and a test of it
/// proves nothing — which is how the resource name `/G` below was found to
/// be read as the inline-image abbreviation for `/DeviceGray`.
fn cal_gray() -> CieSpace {
    CieSpace::CalGray {
        white: D65,
        black: [0.0; 3],
        gamma: 1.8,
    }
}

/// Columns that are multiples of the white point — `0.5 W`, `0.3 W`, `0.2 W`
/// — so `X : Y : Z` is always the white's and the colour always a grey of
/// `Y = 0.5 A^1 + 0.3 B^2 + 0.2 C^3`: three gammas and the matrix read column
/// by column, all in one number. A matrix read row by row puts `0.5 XW`,
/// `0.5` and `0.5 ZW` on the first row, which is not neutral.
///
/// Written out as decimals, `0.5 × 0.9642 = 0.4821` and so on, so the file
/// holds exactly the numbers the read-back is compared with.
fn cal_rgb() -> CieSpace {
    CieSpace::CalRgb {
        white: D50,
        black: [0.0; 3],
        gamma: [1.0, 2.0, 3.0],
        matrix: [
            0.4821, 0.5, 0.41245, // 0.5 W
            0.28926, 0.3, 0.24747, // 0.3 W
            0.19284, 0.2, 0.16498, // 0.2 W
        ],
    }
}

fn lab() -> CieSpace {
    CieSpace::Lab {
        white: D50,
        black: [0.0; 3],
        range: [-128.0, 127.0, -128.0, 127.0],
    }
}

/// **Fills and strokes in each space** (8.6.5.2–8.6.5.4), through
/// `set_fill_cie` and `set_stroke_cie`, drawn to the clause's arithmetic.
#[test]
fn a_cie_colour_fills_and_strokes_to_its_clauses_arithmetic() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cie_color_space(b"G", &cal_gray()));
    assert!(builder.add_cie_color_space(b"R", &cal_rgb()));
    assert!(builder.add_cie_color_space(b"L", &lab()));
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.set_fill_cie(b"G", &[0.5]));
        page.raw(b"0 40 20 20 re f");
        assert!(page.set_fill_cie(b"R", &[0.4, 0.6, 0.9]));
        page.raw(b"20 40 20 20 re f");
        assert!(page.set_fill_cie(b"L", &[50.0, 40.0, -30.0]));
        page.raw(b"40 40 20 20 re f");
        assert!(page.set_fill_cie(b"L", &[60.0, 0.0, 0.0]));
        page.raw(b"0 20 20 20 re f");
        // A stroke, ten wide, along y = 10.
        assert!(page.set_stroke_cie(b"G", &[0.8]));
        page.raw(b"10 w 0 10 m 60 10 l S");
        // And one in each three-component space, ten wide along y = 30 with
        // butt caps, so each covers its own 20-point run and no fill. The
        // /Lab one has a negative a*, which a 0..1 clamp would make zero.
        assert!(page.set_stroke_cie(b"R", &[0.7, 0.2, 0.5]));
        page.raw(b"20 30 m 40 30 l S");
        assert!(page.set_stroke_cie(b"L", &[65.0, -30.0, 40.0]));
        page.raw(b"40 30 m 60 30 l S");
    });
    let bitmap = render(builder.finish());

    // 8.6.5.2: X = XW A^G and so on, so `A = 0.5` at G = 1.8 is a grey of
    // Y = 0.5^1.8 relative to the D65 white.
    near(
        at(&bitmap, 10.0, 50.0),
        grey(0.5_f64.powf(1.8)),
        "CalGray fill",
    );
    near(
        at(&bitmap, 30.0, 10.0),
        grey(0.8_f64.powf(1.8)),
        "CalGray stroke",
    );
    // 8.6.5.3: Y = 0.5 (0.4) + 0.3 (0.6)^2 + 0.2 (0.9)^3.
    let y = 0.5 * 0.4 + 0.3 * 0.6_f64.powi(2) + 0.2 * 0.9_f64.powi(3);
    near(at(&bitmap, 30.0, 50.0), grey(y), "CalRGB fill");
    // 8.6.5.4.
    near(
        at(&bitmap, 50.0, 50.0),
        d50_to_srgb(lab_xyz(50.0, 40.0, -30.0, D50)),
        "Lab fill",
    );
    // No chroma is a grey of g((L + 16) / 116), with no matrix needed.
    near(
        at(&bitmap, 10.0, 30.0),
        grey(lab_xyz(60.0, 0.0, 0.0, D50)[1]),
        "Lab grey",
    );
    // The strokes, through `CS` and `SC`, to the same arithmetic.
    let y = 0.5 * 0.7 + 0.3 * 0.2_f64.powi(2) + 0.2 * 0.5_f64.powi(3);
    near(at(&bitmap, 30.0, 30.0), grey(y), "CalRGB stroke");
    near(
        at(&bitmap, 50.0, 30.0),
        d50_to_srgb(lab_xyz(65.0, -30.0, 40.0, D50)),
        "Lab stroke",
    );
}

/// **Images in each space** through `add_image`, read back as the spaces they
/// were written in and drawn to the clause's arithmetic. With no `/Decode`,
/// Table 90 maps a `/CalGray` or `/CalRGB` sample over 0..1 and a `/Lab` one
/// over 0..100 for `L*` and the space's `/Range` for `a*` and `b*` — so an
/// 8-bit `/Lab` sample `s` is `100 s / 255`, and under `[-128 127]` an `a*`
/// sample `s` is exactly `s - 128`.
#[test]
fn a_cie_image_is_read_back_in_its_space_and_drawn_to_its_clauses_arithmetic() {
    let grey_samples: &'static [u8] = &[64, 191];
    let rgb_samples: &'static [u8] = &[102, 153, 230, 255, 0, 51];
    let lab_samples: &'static [u8] = &[128, 168, 98, 153, 128, 128];
    let raw = |space: ImageColorSpace<'static>, data: &'static [u8]| {
        ImageData::Compressed(CompressedImage {
            width: 2,
            height: 1,
            bits_per_component: 8,
            color_space: space,
            filter: None,
            data,
            color_key_mask: None,
            soft_mask: None,
        })
    };
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cie_color_space(b"G", &cal_gray()));
    assert!(builder.add_cie_color_space(b"R", &cal_rgb()));
    assert!(builder.add_cie_color_space(b"L", &lab()));
    let image = |resource: &'static [u8], components: u8| ImageColorSpace::Cie {
        resource,
        components,
    };
    assert!(builder.add_image(b"ImG", &raw(image(b"G", 1), grey_samples)));
    assert!(builder.add_image(b"ImR", &raw(image(b"R", 3), rgb_samples)));
    assert!(builder.add_image(b"ImL", &raw(image(b"L", 3), lab_samples)));
    // Each image's two samples are 20 x 20 squares side by side.
    builder.add_page(PAGE, PAGE, |page| {
        page.image(b"ImG", 0.0, 40.0, 40.0, 20.0);
        page.image(b"ImR", 0.0, 20.0, 40.0, 20.0);
        page.image(b"ImL", 0.0, 0.0, 40.0, 20.0);
    });
    let bytes = builder.finish();

    let document = Document::open(bytes.clone()).expect("it opens");
    let spaces: Vec<Option<ImageSpace>> = document
        .page(0)
        .expect("a page")
        .images()
        .into_iter()
        .map(|image| image.color_space)
        .collect();
    let CieSpace::CalRgb { matrix, .. } = cal_rgb() else {
        unreachable!()
    };
    assert_eq!(
        spaces,
        vec![
            Some(ImageSpace::CalGray {
                white: D65,
                gamma: 1.8
            }),
            Some(ImageSpace::CalRgb {
                white: D50,
                gamma: [1.0, 2.0, 3.0],
                matrix,
            }),
            Some(ImageSpace::Lab {
                white: D50,
                range: [-128.0, 127.0, -128.0, 127.0],
            }),
        ],
        "each image names its space, read back as written"
    );

    let bitmap = render(bytes);
    let s = |v: u8| f64::from(v) / 255.0;
    for (index, sample) in grey_samples.iter().enumerate() {
        near(
            at(&bitmap, 10.0 + 20.0 * index as f64, 50.0),
            grey(s(*sample).powf(1.8)),
            "CalGray sample",
        );
    }
    for (index, rgb) in rgb_samples.chunks_exact(3).enumerate() {
        let y = 0.5 * s(rgb[0]) + 0.3 * s(rgb[1]).powi(2) + 0.2 * s(rgb[2]).powi(3);
        near(
            at(&bitmap, 10.0 + 20.0 * index as f64, 30.0),
            grey(y),
            "CalRGB sample",
        );
    }
    for (index, lab) in lab_samples.chunks_exact(3).enumerate() {
        let l = 100.0 * s(lab[0]);
        let a = f64::from(lab[1]) - 128.0;
        let b = f64::from(lab[2]) - 128.0;
        near(
            at(&bitmap, 10.0 + 20.0 * index as f64, 10.0),
            d50_to_srgb(lab_xyz(l, a, b, D50)),
            "Lab sample",
        );
    }
}

/// **What the writer refuses**, by Tables 63–65: a white point whose `Y` is
/// not 1 or whose `X` or `Z` is not positive, a negative black point, a gamma
/// that is not positive, a range whose minimum is not below its maximum —
/// and a setter or an image naming no CIE space, or the wrong component
/// count for one.
#[test]
fn the_writer_refuses_what_the_tables_forbid() {
    let mut builder = DocumentBuilder::new();
    let gray = |white: [f64; 3], black: [f64; 3], gamma: f64| CieSpace::CalGray {
        white,
        black,
        gamma,
    };
    assert!(!builder.add_cie_color_space(b"X", &gray([0.95, 0.9, 1.09], [0.0; 3], 1.0)));
    assert!(!builder.add_cie_color_space(b"X", &gray([0.0, 1.0, 1.09], [0.0; 3], 1.0)));
    assert!(!builder.add_cie_color_space(b"X", &gray(D65, [-0.1, 0.0, 0.0], 1.0)));
    assert!(!builder.add_cie_color_space(b"X", &gray(D65, [0.0; 3], 0.0)));
    assert!(!builder.add_cie_color_space(b"X", &gray(D65, [0.0; 3], f64::NAN)));
    assert!(!builder.add_cie_color_space(
        b"X",
        &CieSpace::Lab {
            white: D50,
            black: [0.0; 3],
            range: [10.0, -10.0, -100.0, 100.0],
        }
    ));
    assert!(builder.add_cie_color_space(b"G", &cal_gray()));
    assert!(!builder.add_image(
        b"Im",
        &ImageData::Compressed(CompressedImage {
            width: 1,
            height: 1,
            bits_per_component: 8,
            color_space: ImageColorSpace::Cie {
                resource: b"G",
                components: 3,
            },
            filter: None,
            data: &[0, 0, 0],
            color_key_mask: None,
            soft_mask: None,
        })
    ));
    builder.add_page(PAGE, PAGE, |page| {
        assert!(!page.set_fill_cie(b"X", &[0.5]), "nothing registered");
        assert!(page.set_fill_cie(b"G", &[0.5]));
    });
}
