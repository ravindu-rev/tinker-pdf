//! The decode group: compressed data and images decoded.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `decode` list names these operations
//! in this order.
//!
//! Each operation is sized into the set's 20-300 ms window on the hosted
//! runner (`docs/design/performance.md`): the JPEG decodes at a quarter of a
//! scan's pixels, the 9/7 codestream at a sixteenth and the lossless 5/3 one
//! at a sixty-fourth, and the shorter decodes repeated inside an iteration,
//! their unit counting every call.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf_filters::{
    ccitt_decode, deflate, inflate_raw, jbig2_decode, jpeg_decode, jpx_decode, lzw_decode,
    CcittParams, Jbig2Params, Limits,
};

/// The ceiling every decode here is handed: well above the largest output,
/// an RGB page at 300 dpi.
const CEILING: usize = 1 << 28;

/// Width times height.
fn area((width, height): (u32, u32)) -> u64 {
    u64::from(width) * u64::from(height)
}

/// The generic page's parameters: no globals, and the work its decode may
/// do measured against its output ceiling.
#[cfg(not(tinker_planning_baseline))]
fn jbig2_params(width: u32, height: u32) -> Jbig2Params<'static> {
    Jbig2Params {
        globals: &[],
        width,
        height,
        work_ceiling: None,
    }
}

/// The same parameters on the planning baseline, whose `Jbig2Params` has no
/// work ceiling of its own.
#[cfg(tinker_planning_baseline)]
fn jbig2_params(width: u32, height: u32) -> Jbig2Params<'static> {
    Jbig2Params {
        globals: &[],
        width,
        height,
    }
}

fn decode(c: &mut Criterion) {
    let jpeg_size = inputs::scan_part(2);
    let irreversible_size = inputs::scan_part(4);
    let reversible_size = inputs::scan_part(8);
    let limits = Limits::new(CEILING);

    // Inflate and LZW, each asserted to give its input back. The coded
    // streams are written by the encoders, so they cross a paired run too.
    let store = inputs::store();
    let page_like = LazyCell::new(|| store.bytes("page-like.rgb", inputs::page_like));
    let deflated = LazyCell::new(|| {
        let deflated = store.bytes("page-like.deflate", || deflate(&page_like));
        assert!(inflate_raw(&deflated, &limits).data == *page_like);
        deflated
    });
    let noise = LazyCell::new(|| {
        let noise = store.bytes("noise.bin", inputs::noise);
        let noisy = store.bytes("noise.deflate", || deflate(&noise));
        let ratio = noisy.len() as f64 / noise.len() as f64;
        assert!(
            (0.90..1.0).contains(&ratio),
            "the noise deflates to between 90 % and 100 %, so it is coded and not stored: {ratio}"
        );
        assert!(inflate_raw(&noisy, &limits).data == noise);
        (noise, noisy)
    });
    let lzw = LazyCell::new(|| {
        let lzw = store.bytes("page-like.lzw", || inputs::lzw(&page_like));
        let unlzw = lzw_decode(&lzw, &limits, true, None).expect("the LZW decodes");
        assert!(
            unlzw.complete && unlzw.data == *page_like,
            "LZW gives the page-like data back"
        );
        lzw
    });

    // The image codecs, each asserted to decode its whole picture.
    let baseline = LazyCell::new(|| {
        let baseline = store.bytes("jpeg-1240x1754.jpg", || inputs::baseline_jpeg_part(2));
        let image = jpeg_decode(&baseline, CEILING).expect("the JPEG decodes");
        assert_eq!((image.width, image.height), jpeg_size);
        assert_eq!(image.data.len(), 3 * area(jpeg_size) as usize);
        (baseline, image.data)
    });
    // The progressive file is the baseline's coefficients re-scanned, so the
    // two decode to the same samples.
    let progressive = LazyCell::new(|| {
        let name = "progressive-1240x1754.jpg";
        let progressive = store.bytes(name, || inputs::committed_data(name));
        let rescanned = jpeg_decode(&progressive, CEILING).expect("the JPEG decodes");
        assert!(
            rescanned.data == baseline.1,
            "the progressive JPEG decodes to the baseline's samples exactly"
        );
        progressive
    });
    let irreversible = LazyCell::new(|| {
        let name = "irreversible-620x877.jp2";
        let irreversible = store.bytes(name, || inputs::committed_data(name));
        let image = jpx_decode(&irreversible, &limits, &mut Vec::new()).expect("it decodes");
        assert_eq!((image.width, image.height), irreversible_size);
        assert_eq!(image.samples.len(), 3 * area(irreversible_size) as usize);
        irreversible
    });
    // The lossless codestream is the generator's own picture, exactly.
    let reversible = LazyCell::new(|| {
        let name = "reversible-310x438.jp2";
        let reversible = store.bytes(name, || inputs::committed_data(name));
        let image = jpx_decode(&reversible, &limits, &mut Vec::new()).expect("it decodes");
        let (width, height) = reversible_size;
        assert!(
            image.samples == inputs::photo(width, height, 0),
            "the reversible codestream decodes to photo(310, 438, 0) exactly"
        );
        reversible
    });

    // The scan, a render, coded each bilevel way and decoded back to itself.
    let scan = LazyCell::new(|| store.scan("scan-300.bits", inputs::scan));
    let scan_pixels = || {
        let (width, height, _) = &*scan;
        u64::from(*width) * u64::from(*height)
    };
    let g4 = LazyCell::new(|| {
        let (width, height, bits) = &*scan;
        let g4 = store.bytes("scan-300.g4", || inputs::g4(*width, *height, bits));
        let params = CcittParams {
            k: -1,
            columns: *width,
            rows: *height,
            ..CcittParams::default()
        };
        let (rows, warnings) = ccitt_decode(&g4, &params, CEILING);
        assert!(
            warnings.is_empty() && rows == bits[..],
            "G4 gives the scan back"
        );
        (g4, params)
    });
    let jbig2 = LazyCell::new(|| {
        let (width, height, bits) = &*scan;
        let jbig2 = store.bytes("scan-300.jbig2", || inputs::jbig2(*width, *height, bits));
        let params = jbig2_params(*width, *height);
        let page = jbig2_decode(&jbig2, &params, CEILING, &mut Vec::new()).expect("decodes");
        let black_is_1: Vec<u8> = bits.iter().map(|b| !b).collect();
        assert!(
            page == black_is_1,
            "JBIG2 gives the scan back, in its own sense"
        );
        (jbig2, params)
    });

    let mut group = Group::new(c, "decode");

    group.operation("inflate page-like data", |op| {
        op.per(Per::Bytes(6 * page_like.len() as u64));
        op.iter(|| {
            for _ in 0..6 {
                black_box(inflate_raw(&deflated, &limits));
            }
        });
    });
    group.operation("inflate near-incompressible data", |op| {
        let (noise, noisy) = &*noise;
        let limits = &limits;
        op.per(Per::Bytes(3 * noise.len() as u64));
        op.iter(move || {
            for _ in 0..3 {
                black_box(inflate_raw(noisy, limits));
            }
        });
    });
    group.calibrated("LZW decode page-like data", |op| {
        op.per(Per::Bytes(5 * page_like.len() as u64));
        op.iter(|| {
            for _ in 0..5 {
                let _ = black_box(lzw_decode(&lzw, &limits, true, None));
            }
        });
    });

    group.operation("baseline JPEG 1240x1754", |op| {
        let (baseline, _) = &*baseline;
        op.per(Per::Pixels(area(jpeg_size)));
        op.iter(move || black_box(jpeg_decode(baseline, CEILING)));
    });
    group.operation("progressive JPEG 1240x1754", |op| {
        op.per(Per::Pixels(area(jpeg_size)));
        op.iter(|| black_box(jpeg_decode(&progressive, CEILING)));
    });
    group.operation("irreversible JPEG 2000 620x877", |op| {
        op.per(Per::Pixels(area(irreversible_size)));
        op.iter(|| black_box(jpx_decode(&irreversible, &limits, &mut Vec::new())));
    });
    group.operation("reversible JPEG 2000 310x438", |op| {
        op.per(Per::Pixels(area(reversible_size)));
        op.iter(|| black_box(jpx_decode(&reversible, &limits, &mut Vec::new())));
    });

    group.operation("decode the JBIG2 generic page", |op| {
        let (jbig2, params) = &*jbig2;
        op.per(Per::Pixels(scan_pixels()));
        op.iter(move || black_box(jbig2_decode(jbig2, params, CEILING, &mut Vec::new())));
    });
    group.operation("decode the CCITT G4 page", |op| {
        let (g4, params) = &*g4;
        op.per(Per::Pixels(2 * scan_pixels()));
        op.iter(move || {
            for _ in 0..2 {
                black_box(ccitt_decode(g4, params, CEILING));
            }
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = decode
}
criterion_main!(benches);
