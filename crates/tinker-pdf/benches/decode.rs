//! The decode group: compressed data and images decoded.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `decode` list names these operations
//! in this order.

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

fn decode(c: &mut Criterion) {
    let (pixels_w, pixels_h) = inputs::SCAN;
    let pixels = u64::from(pixels_w) * u64::from(pixels_h);
    let limits = Limits::new(CEILING);

    // Inflate and LZW, each asserted to give its input back.
    let page_like = LazyCell::new(inputs::page_like);
    let deflated = LazyCell::new(|| {
        let deflated = deflate(&page_like);
        assert!(inflate_raw(&deflated, &limits).data == *page_like);
        deflated
    });
    let noise = LazyCell::new(|| {
        let noise = inputs::noise();
        let noisy = deflate(&noise);
        let ratio = noisy.len() as f64 / noise.len() as f64;
        assert!(
            (0.90..1.0).contains(&ratio),
            "the noise deflates to between 90 % and 100 %, so it is coded and not stored: {ratio}"
        );
        assert!(inflate_raw(&noisy, &limits).data == noise);
        (noise, noisy)
    });
    let lzw = LazyCell::new(|| {
        let lzw = inputs::lzw(&page_like);
        let unlzw = lzw_decode(&lzw, &limits, true, None).expect("the LZW decodes");
        assert!(
            unlzw.complete && unlzw.data == *page_like,
            "LZW gives the page-like data back"
        );
        lzw
    });

    // The image codecs, each asserted to decode its whole picture.
    let baseline = LazyCell::new(|| {
        let baseline = inputs::baseline_jpeg();
        let image = jpeg_decode(&baseline, CEILING).expect("the JPEG decodes");
        assert_eq!((image.width, image.height), inputs::SCAN);
        assert_eq!(image.data.len(), 3 * pixels as usize);
        (baseline, image.data)
    });
    // The progressive file is the baseline's coefficients re-scanned, so the
    // two decode to the same samples.
    let progressive = LazyCell::new(|| {
        let progressive = inputs::committed_data("progressive-2480x3508.jpg");
        let rescanned = jpeg_decode(&progressive, CEILING).expect("the JPEG decodes");
        assert!(
            rescanned.data == baseline.1,
            "the progressive JPEG decodes to the baseline's samples exactly"
        );
        progressive
    });
    let irreversible = LazyCell::new(|| {
        let irreversible = inputs::committed_data("irreversible-2480x3508.jp2");
        let image = jpx_decode(&irreversible, &limits, &mut Vec::new()).expect("it decodes");
        assert_eq!((image.width, image.height), inputs::SCAN);
        assert_eq!(image.samples.len(), 3 * pixels as usize);
        irreversible
    });
    // The lossless codestream is the generator's own picture, exactly.
    let reversible = LazyCell::new(|| {
        let reversible = inputs::committed_data("reversible-2480x3508.jp2");
        let image = jpx_decode(&reversible, &limits, &mut Vec::new()).expect("it decodes");
        assert!(
            image.samples == inputs::photo(pixels_w, pixels_h, 0),
            "the reversible codestream decodes to photo(2480, 3508, 0) exactly"
        );
        reversible
    });

    // The scan, coded each bilevel way and decoded back to itself.
    let scan = LazyCell::new(inputs::scan);
    let scan_pixels = || {
        let (width, height, _) = &*scan;
        u64::from(*width) * u64::from(*height)
    };
    let g4 = LazyCell::new(|| {
        let (width, height, bits) = &*scan;
        let g4 = inputs::g4(*width, *height, bits);
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
        let jbig2 = inputs::jbig2(*width, *height, bits);
        let params = Jbig2Params {
            globals: &[],
            width: *width,
            height: *height,
            work_ceiling: None,
        };
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
        op.per(Per::Bytes(2 * page_like.len() as u64));
        op.iter(|| {
            for _ in 0..2 {
                black_box(inflate_raw(&deflated, &limits));
            }
        });
    });
    group.operation("inflate near-incompressible data", |op| {
        let (noise, noisy) = &*noise;
        op.per(Per::Bytes(noise.len() as u64));
        op.iter(|| black_box(inflate_raw(noisy, &limits)));
    });
    group.operation("LZW decode page-like data", |op| {
        op.per(Per::Bytes(page_like.len() as u64));
        op.iter(|| black_box(lzw_decode(&lzw, &limits, true, None)));
    });

    group.operation("baseline JPEG 2480x3508", |op| {
        let (baseline, _) = &*baseline;
        op.per(Per::Pixels(pixels));
        op.iter(|| black_box(jpeg_decode(baseline, CEILING)));
    });
    group.operation("progressive JPEG 2480x3508", |op| {
        op.per(Per::Pixels(pixels));
        op.iter(|| black_box(jpeg_decode(&progressive, CEILING)));
    });
    group.operation("irreversible JPEG 2000 2480x3508", |op| {
        op.per(Per::Pixels(pixels));
        op.iter(|| black_box(jpx_decode(&irreversible, &limits, &mut Vec::new())));
    });
    group.operation("reversible JPEG 2000 2480x3508", |op| {
        op.per(Per::Pixels(pixels));
        op.iter(|| black_box(jpx_decode(&reversible, &limits, &mut Vec::new())));
    });

    group.operation("decode the JBIG2 generic page", |op| {
        let (jbig2, params) = &*jbig2;
        op.per(Per::Pixels(scan_pixels()));
        op.iter(|| black_box(jbig2_decode(jbig2, params, CEILING, &mut Vec::new())));
    });
    group.operation("decode the CCITT G4 page", |op| {
        let (g4, params) = &*g4;
        op.per(Per::Pixels(scan_pixels()));
        op.iter(|| black_box(ccitt_decode(g4, params, CEILING)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = decode
}
criterion_main!(benches);
