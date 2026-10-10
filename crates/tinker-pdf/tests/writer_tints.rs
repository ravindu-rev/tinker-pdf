//! `/Separation` and `/DeviceN` on write, drawn by this repository's own
//! renderer and held to the arithmetic of their tint transforms.
//!
//! A tint transform is a formula the writer was handed, so the right colour at
//! any tint is something the test can compute rather than something it has to
//! take from a picture: 7.10.3's `c0 + t (c1 - c0)` for a type 2 function, the
//! caller's own program for a type 4 one, then the alternate space's
//! conversion, and a component reaches the page as `round(v × 255)`
//! (`render_analytic.rs` measured that convention and says so). Every pixel
//! compared below is inside a filled shape, fully covered and opaque, so the
//! comparison is **exact** — there is no coverage and no constant alpha for a
//! tolerance to hide in.
//!
//! What this cannot reach is whether a spot colour is the colour a press would
//! print. Nothing here manages ink; a reader without the ink paints the
//! alternate, and the alternate is what is measured.

use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, ArchivalRefusal, CalculatorOp, CompressedImage,
    DeviceNAttributes, DeviceSpace, Document, DocumentBuilder, Function, ImageColorSpace,
    ImageData,
};

mod pdfa_support;
mod render_support;
use render_support::{byte, ink, pixel, render};

/// The page every fixture draws on, in points and — at the default 72 dpi —
/// in pixels.
const PAGE: f64 = 60.0;

/// The pixel whose centre is at `(x, y)` in user space.
fn at(bitmap: &tinker_pdf::Bitmap, x: f64, y: f64) -> (u8, u8, u8) {
    pixel(bitmap, x as u32, (PAGE - y) as u32)
}

/// The three components a DeviceRGB value reaches the page as.
fn rgb(value: [f64; 3]) -> (u8, u8, u8) {
    (byte(value[0]), byte(value[1]), byte(value[2]))
}

/// The fewest pixels any fixture here paints: one 10 x 20 square of ink.
const FLOOR: usize = 200;

/// The spot colour's ramp: white at no ink, a blue at full strength.
const C0: [f64; 3] = [1.0, 1.0, 1.0];
const C1: [f64; 3] = [0.2, 0.4, 0.9];

fn ramp() -> Function {
    Function::Exponential {
        domain: [0.0, 1.0],
        c0: C0.to_vec(),
        c1: C1.to_vec(),
        n: 1.0,
    }
}

/// 7.10.3 with `N = 1`, the way the clause writes it.
fn ramp_at(t: f64) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (i, value) in out.iter_mut().enumerate() {
        *value = C0[i] + t * (C1[i] - C0[i]);
    }
    out
}

/// Two inks into RGB: `R = 1 - a`, `G = 1 - b`, `B = 1 - (a + b) / 2`.
fn two_inks() -> Function {
    use CalculatorOp::{Number as N, Operator as Op};
    Function::Calculator {
        domain: vec![[0.0, 1.0], [0.0, 1.0]],
        range: vec![[0.0, 1.0], [0.0, 1.0], [0.0, 1.0]],
        program: vec![
            N(1.0),
            Op("index"),
            N(1.0),
            Op("exch"),
            Op("sub"),
            N(1.0),
            Op("index"),
            N(1.0),
            Op("exch"),
            Op("sub"),
            N(3.0),
            Op("index"),
            N(3.0),
            Op("index"),
            Op("add"),
            N(0.5),
            Op("mul"),
            N(1.0),
            Op("exch"),
            Op("sub"),
            N(5.0),
            N(3.0),
            Op("roll"),
            Op("pop"),
            Op("pop"),
        ],
    }
}

/// The program above, as arithmetic.
fn two_inks_at(a: f64, b: f64) -> [f64; 3] {
    [1.0 - a, 1.0 - b, 1.0 - (a + b) * 0.5]
}

/// Opens a built document, asserts the strict validator has nothing to say
/// about it, and renders its first page.
///
/// `least` is the ink floor: every fixture here paints at least one full-ink
/// shape, and a page that drew nothing would compare equal to nothing.
#[track_caller]
fn drawn(bytes: Vec<u8>, least: usize) -> tinker_pdf::Bitmap {
    let document = Document::open(bytes.clone()).expect("the built document opens");
    let defects = document.validate();
    assert!(defects.is_empty(), "the strict validator: {defects:?}");
    let bitmap = render(bytes);
    let painted = ink(&bitmap);
    assert!(
        painted >= least,
        "the fixture painted {painted} pixels, fewer than {least}"
    );
    bitmap
}

/// Five tints of one spot colour, each a filled square, each the ramp's value
/// at that tint.
#[test]
fn a_separation_fill_is_its_tint_transform_at_every_tint() {
    let tints = [0.0, 0.25, 0.5, 0.8, 1.0];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_separation_color_space(b"CS0", b"Spot Blue", DeviceSpace::Rgb, &ramp()));
    builder.add_page(PAGE, PAGE, |page| {
        for (i, t) in tints.iter().enumerate() {
            assert!(page.set_fill_tint(b"CS0", &[*t]));
            page.raw(format!("{} 20 10 20 re f", 5 + 10 * i).as_bytes());
        }
    });
    let bitmap = drawn(builder.finish(), FLOOR);
    for (i, t) in tints.iter().enumerate() {
        let x = 10.0 + 10.0 * i as f64;
        assert_eq!(
            at(&bitmap, x, 30.0),
            rgb(ramp_at(*t)),
            "tint {t}, at x = {x}"
        );
    }
}

/// The stroking half: `CS` and `SCN`, into a different parameter of the
/// graphics state from the fill, so a fill set alongside must not leak in.
#[test]
fn a_separation_stroke_is_its_tint_transform_and_not_the_fill() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_separation_color_space(b"CS0", b"Spot Blue", DeviceSpace::Rgb, &ramp()));
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.set_fill_tint(b"CS0", &[1.0]));
        assert!(page.set_stroke_tint(b"CS0", &[0.35]));
        page.raw(b"10 w 5 45 m 55 45 l S");
        page.raw(b"5 5 50 20 re f");
    });
    let bitmap = drawn(builder.finish(), FLOOR);
    assert_eq!(at(&bitmap, 30.0, 45.0), rgb(ramp_at(0.35)), "the stroke");
    assert_eq!(at(&bitmap, 30.0, 15.0), rgb(ramp_at(1.0)), "the fill");
}

/// A two-ink `/DeviceN` fill is the caller's calculator at every pair of
/// tints, the pairs chosen so no two share a component.
#[test]
fn a_device_n_fill_is_its_calculator_at_every_pair() {
    let pairs = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.3, 0.6), (0.9, 0.15)];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_device_n_color_space(
        b"CS1",
        &[b"Spot A", b"Spot B"],
        DeviceSpace::Rgb,
        &two_inks(),
        None,
    ));
    builder.add_page(PAGE, PAGE, |page| {
        for (i, (a, b)) in pairs.iter().enumerate() {
            assert!(page.set_fill_tint(b"CS1", &[*a, *b]));
            page.raw(format!("{} 20 10 20 re f", 5 + 10 * i).as_bytes());
        }
    });
    let bitmap = drawn(builder.finish(), FLOOR);
    for (i, (a, b)) in pairs.iter().enumerate() {
        let x = 10.0 + 10.0 * i as f64;
        assert_eq!(
            at(&bitmap, x, 30.0),
            rgb(two_inks_at(*a, *b)),
            "tints ({a}, {b})"
        );
    }
}

/// A `/Separation` whose transform is a calculator draws the same arithmetic
/// as one whose transform is a ramp, when the two compute the same thing —
/// the second function type through the same space.
#[test]
fn a_separation_through_a_calculator_is_the_same_arithmetic() {
    use CalculatorOp::{Number as N, Operator as Op};
    // `t -> [1 - 0.8 t, 1 - 0.6 t, 1 - 0.1 t]`, which is `ramp_at` spelled as
    // a program.
    let mut program = Vec::new();
    for (i, slope) in [0.8, 0.6, 0.1].iter().enumerate() {
        program.extend([
            N(i as f64),
            Op("index"),
            N(-slope),
            Op("mul"),
            N(1.0),
            Op("add"),
        ]);
    }
    program.extend([N(4.0), N(3.0), Op("roll"), Op("pop")]);
    let calculator = Function::Calculator {
        domain: vec![[0.0, 1.0]],
        range: vec![[0.0, 1.0]; 3],
        program,
    };
    let tints = [0.0, 0.5, 1.0];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_separation_color_space(
        b"CS0",
        b"Spot Blue",
        DeviceSpace::Rgb,
        &calculator
    ));
    builder.add_page(PAGE, PAGE, |page| {
        for (i, t) in tints.iter().enumerate() {
            assert!(page.set_fill_tint(b"CS0", &[*t]));
            page.raw(format!("{} 20 10 20 re f", 5 + 10 * i).as_bytes());
        }
    });
    let bitmap = drawn(builder.finish(), FLOOR);
    for (i, t) in tints.iter().enumerate() {
        let x = 10.0 + 10.0 * i as f64;
        let expected = [1.0 - 0.8 * t, 1.0 - 0.6 * t, 1.0 - 0.1 * t];
        assert_eq!(at(&bitmap, x, 30.0), rgb(expected), "tint {t}");
    }
}

/// An image in a `/Separation` space: each 8-bit sample is a tint, `s / 255`,
/// and each is drawn as the ramp at that tint.
#[test]
fn a_separation_image_is_its_tint_transform_per_sample() {
    let samples = [0u8, 85, 170, 255];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_separation_color_space(b"CS0", b"Spot Blue", DeviceSpace::Rgb, &ramp()));
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Compressed(CompressedImage {
            width: 4,
            height: 1,
            bits_per_component: 8,
            color_space: ImageColorSpace::Tint {
                resource: b"CS0",
                components: 1,
            },
            filter: None,
            data: &samples,
            color_key_mask: None,
            soft_mask: None,
        }),
    ));
    builder.add_page(PAGE, PAGE, |page| {
        page.image(b"Im0", 10.0, 20.0, 40.0, 20.0)
    });
    let bitmap = drawn(builder.finish(), FLOOR);
    for (i, sample) in samples.iter().enumerate() {
        let x = 15.0 + 10.0 * i as f64;
        let t = f64::from(*sample) / 255.0;
        assert_eq!(at(&bitmap, x, 30.0), rgb(ramp_at(t)), "sample {sample}");
    }
}

/// An image in a two-ink `/DeviceN` space: two samples a pixel, through the
/// calculator.
#[test]
fn a_device_n_image_is_its_calculator_per_sample() {
    let samples = [0u8, 0, 255, 51, 102, 204];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_device_n_color_space(
        b"CS1",
        &[b"Spot A", b"Spot B"],
        DeviceSpace::Rgb,
        &two_inks(),
        None,
    ));
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Compressed(CompressedImage {
            width: 3,
            height: 1,
            bits_per_component: 8,
            color_space: ImageColorSpace::Tint {
                resource: b"CS1",
                components: 2,
            },
            filter: None,
            data: &samples,
            color_key_mask: None,
            soft_mask: None,
        }),
    ));
    builder.add_page(PAGE, PAGE, |page| page.image(b"Im0", 0.0, 20.0, 60.0, 20.0));
    let bitmap = drawn(builder.finish(), FLOOR);
    for (i, pair) in samples.chunks_exact(2).enumerate() {
        let x = 10.0 + 20.0 * i as f64;
        let (a, b) = (f64::from(pair[0]) / 255.0, f64::from(pair[1]) / 255.0);
        assert_eq!(at(&bitmap, x, 30.0), rgb(two_inks_at(a, b)), "{pair:?}");
    }
}

/// Under ISO 19005 the alternate is a device colour, and the destination
/// profile decides whether it may be painted: a spot colour whose alternate
/// the output intent cannot reproduce is refused by the clause.
#[test]
fn an_archival_document_refuses_an_alternate_its_intent_cannot_reproduce() {
    let mut builder = DocumentBuilder::archival(ArchivalProfile {
        part: ArchivalPart::Two,
        level: Some(ArchivalLevel::B),
        destination_profile: pdfa_support::cmyk_like(),
        destination_space: DeviceSpace::Cmyk,
        output_condition: "a CMYK press".to_string(),
        language: None,
    });
    assert!(!builder.add_separation_color_space(b"CS0", b"Spot", DeviceSpace::Rgb, &ramp()));
    assert_eq!(
        builder.refusals(),
        &[ArchivalRefusal::DeviceColour {
            space: DeviceSpace::Rgb
        }]
    );
    assert!(builder.add_separation_color_space(
        b"CS0",
        b"Spot",
        DeviceSpace::Cmyk,
        &Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0; 4],
            c1: vec![1.0, 0.5, 0.0, 0.1],
            n: 1.0,
        },
    ));
}

/// A level B builder for `part` with a CMYK output intent, which admits a
/// CMYK alternate.
fn archival_cmyk(part: ArchivalPart) -> DocumentBuilder {
    DocumentBuilder::archival(ArchivalProfile {
        part,
        level: Some(ArchivalLevel::B),
        destination_profile: pdfa_support::cmyk_like(),
        destination_space: DeviceSpace::Cmyk,
        output_condition: "a CMYK press".to_string(),
        language: None,
    })
}

/// A ramp from no ink to `c1` in CMYK.
fn cmyk_ramp(c1: [f64; 4]) -> Function {
    Function::Exponential {
        domain: [0.0, 1.0],
        c0: vec![0.0; 4],
        c1: c1.to_vec(),
        n: 1.0,
    }
}

/// Two inks into CMYK: `(a, b) -> (a, b, 0, 0)`.
fn two_inks_cmyk() -> Function {
    Function::Calculator {
        domain: vec![[0.0, 1.0], [0.0, 1.0]],
        range: vec![[0.0, 1.0]; 4],
        program: vec![CalculatorOp::Number(0.0), CalculatorOp::Number(0.0)],
    }
}

/// ISO 19005-2 6.2.4.4, its first sentence: every spot colour a `/DeviceN`
/// names has an entry in the space's `/Colorants`. A space with no
/// attributes at all, and one whose attributes describe one of its two inks,
/// are refused by the clause; `/None` and DeviceCMYK's four process
/// colorants are not spot colours and need no entry; the space describing
/// both inks is written, and the finished document is one the profile is
/// satisfied with.
#[test]
fn an_archival_device_n_describes_every_spot_colour_it_names() {
    let spots: [&[u8]; 2] = [b"Spot A", b"Spot B"];
    let mut builder = archival_cmyk(ArchivalPart::Two);
    assert!(!builder.add_device_n_color_space(
        b"CS1",
        &spots,
        DeviceSpace::Cmyk,
        &two_inks_cmyk(),
        None,
    ));
    assert!(builder.add_separation_color_space(
        b"SA",
        b"Spot A",
        DeviceSpace::Cmyk,
        &cmyk_ramp([1.0, 0.5, 0.0, 0.1]),
    ));
    assert!(!builder.add_device_n_color_space(
        b"CS1",
        &spots,
        DeviceSpace::Cmyk,
        &two_inks_cmyk(),
        Some(&DeviceNAttributes {
            colorants: &[b"SA"]
        }),
    ));
    assert_eq!(
        builder.refusals(),
        &[
            ArchivalRefusal::UndescribedColorant {
                colorant: b"Spot A".to_vec()
            },
            ArchivalRefusal::UndescribedColorant {
                colorant: b"Spot B".to_vec()
            },
        ]
    );
    for refusal in builder.refusals() {
        assert_eq!(refusal.clause(), "6.2.4.4");
    }
    assert!(builder.add_device_n_color_space(
        b"CS2",
        &[b"Cyan", b"None"],
        DeviceSpace::Cmyk,
        &two_inks_cmyk(),
        None,
    ));
    assert!(builder.add_separation_color_space(
        b"SB",
        b"Spot B",
        DeviceSpace::Cmyk,
        &cmyk_ramp([0.0, 0.3, 1.0, 0.0]),
    ));
    assert!(builder.add_device_n_color_space(
        b"CS1",
        &spots,
        DeviceSpace::Cmyk,
        &two_inks_cmyk(),
        Some(&DeviceNAttributes {
            colorants: &[b"SA", b"SB"]
        }),
    ));
    assert_eq!(builder.refusals().len(), 2, "nothing more was refused");
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.set_fill_tint(b"CS1", &[0.5, 0.5]));
        page.raw(b"10 10 20 20 re f");
        assert!(page.set_fill_tint(b"CS2", &[0.5, 0.0]));
        page.raw(b"30 10 20 20 re f");
    });
    builder.finish_archival().expect("the profile is satisfied");

    // Without a profile the clause does not apply.
    let mut plain = DocumentBuilder::new();
    assert!(plain.add_device_n_color_space(
        b"CS1",
        &spots,
        DeviceSpace::Cmyk,
        &two_inks_cmyk(),
        None,
    ));
}

/// ISO 19005-2 6.2.4.4, its second sentence: every `/Separation` array of one
/// colorant name has the same tint transform and the same alternate. A
/// second space for an ink already registered is refused when either
/// differs — re-registered under the same resource name too, since a page
/// begun before still names the first — and written when both agree, under
/// any resource name.
#[test]
fn an_archival_document_refuses_a_second_separation_of_one_ink_that_disagrees() {
    let ramp = cmyk_ramp([1.0, 0.5, 0.0, 0.1]);
    let other = cmyk_ramp([0.0, 1.0, 0.0, 0.0]);
    let grey = Function::Exponential {
        domain: [0.0, 1.0],
        c0: vec![0.0],
        c1: vec![1.0],
        n: 1.0,
    };
    let mut builder = archival_cmyk(ArchivalPart::Two);
    assert!(builder.add_separation_color_space(b"S1", b"Spot A", DeviceSpace::Cmyk, &ramp));
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.set_fill_tint(b"S1", &[1.0]));
        page.raw(b"10 10 20 20 re f");
    });
    assert!(!builder.add_separation_color_space(b"S2", b"Spot A", DeviceSpace::Cmyk, &other));
    assert!(!builder.add_separation_color_space(b"S1", b"Spot A", DeviceSpace::Cmyk, &other));
    assert!(!builder.add_separation_color_space(b"S3", b"Spot A", DeviceSpace::Gray, &grey));
    let refused = ArchivalRefusal::InconsistentSeparation {
        colorant: b"Spot A".to_vec(),
    };
    assert_eq!(
        builder.refusals(),
        &[refused.clone(), refused.clone(), refused]
    );
    assert_eq!(builder.refusals()[0].clause(), "6.2.4.4");
    assert!(builder.add_separation_color_space(b"S2", b"Spot A", DeviceSpace::Cmyk, &ramp));
    assert!(builder.add_separation_color_space(b"S4", b"Spot B", DeviceSpace::Gray, &grey));
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.set_fill_tint(b"S2", &[1.0]));
        page.raw(b"10 10 20 20 re f");
        assert!(page.set_fill_tint(b"S4", &[1.0]));
        page.raw(b"30 10 20 20 re f");
    });
    builder.finish_archival().expect("the profile is satisfied");

    // Part 1 has no such clause, and no profile has no clauses: the second
    // spelling is written by both.
    for mut unbound in [archival_cmyk(ArchivalPart::One), DocumentBuilder::new()] {
        assert!(unbound.add_separation_color_space(b"S1", b"Spot A", DeviceSpace::Cmyk, &ramp));
        assert!(unbound.add_separation_color_space(b"S2", b"Spot A", DeviceSpace::Cmyk, &other));
    }
}
