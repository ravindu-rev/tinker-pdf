//! The decode group: compressed data and images decoded.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `decode` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
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

    // Inflate and LZW, each asserted to give its input back.
    let page_like = inputs::page_like();
    let deflated = deflate(&page_like);
    let noise = inputs::noise();
    let noisy = deflate(&noise);
    let ratio = noisy.len() as f64 / noise.len() as f64;
    assert!(
        (0.90..1.0).contains(&ratio),
        "the noise deflates to between 90 % and 100 %, so it is coded and not stored: {ratio}"
    );
    let limits = Limits::new(CEILING);
    assert!(inflate_raw(&deflated, &limits).data == page_like);
    assert!(inflate_raw(&noisy, &limits).data == noise);
    let lzw = inputs::lzw(&page_like);
    let unlzw = lzw_decode(&lzw, &limits, true, None).expect("the LZW decodes");
    assert!(
        unlzw.complete && unlzw.data == page_like,
        "LZW gives the page-like data back"
    );

    // The image codecs, each asserted to decode its whole picture.
    let baseline = inputs::baseline_jpeg();
    let progressive = inputs::committed_data("progressive-2480x3508.jpg");
    let irreversible = inputs::committed_data("irreversible-2480x3508.jp2");
    let reversible = inputs::committed_data("reversible-2480x3508.jp2");
    let image = jpeg_decode(&baseline, CEILING).expect("the JPEG decodes");
    assert_eq!((image.width, image.height), inputs::SCAN);
    assert_eq!(image.data.len(), 3 * pixels as usize);
    // The progressive file is the baseline's coefficients re-scanned, so the
    // two decode to the same samples.
    let rescanned = jpeg_decode(&progressive, CEILING).expect("the JPEG decodes");
    assert!(
        rescanned.data == image.data,
        "the progressive JPEG decodes to the baseline's samples exactly"
    );
    let image = jpx_decode(&irreversible, &limits, &mut Vec::new()).expect("it decodes");
    assert_eq!((image.width, image.height), inputs::SCAN);
    assert_eq!(image.samples.len(), 3 * pixels as usize);
    // The lossless codestream is the generator's own picture, exactly.
    let image = jpx_decode(&reversible, &limits, &mut Vec::new()).expect("it decodes");
    assert!(
        image.samples == inputs::photo(pixels_w, pixels_h, 0),
        "the reversible codestream decodes to photo(2480, 3508, 0) exactly"
    );

    // The scan, coded each bilevel way and decoded back to itself.
    let (width, height, bits) = inputs::scan();
    let scan_pixels = u64::from(width) * u64::from(height);
    let g4 = inputs::g4(width, height, &bits);
    let g4_params = CcittParams {
        k: -1,
        columns: width,
        rows: height,
        ..CcittParams::default()
    };
    let (rows, warnings) = ccitt_decode(&g4, &g4_params, CEILING);
    assert!(
        warnings.is_empty() && rows == bits[..],
        "G4 gives the scan back"
    );
    let jbig2 = inputs::jbig2(width, height, &bits);
    let jbig2_params = Jbig2Params {
        globals: &[],
        width,
        height,
        work_ceiling: None,
    };
    let page = jbig2_decode(&jbig2, &jbig2_params, CEILING, &mut Vec::new()).expect("decodes");
    let black_is_1: Vec<u8> = bits.iter().map(|b| !b).collect();
    assert!(
        page == black_is_1,
        "JBIG2 gives the scan back, in its own sense"
    );

    let mut group = c.benchmark_group("decode");
    group.sampling_mode(SamplingMode::Flat);

    group.throughput(Per::Bytes(2 * page_like.len() as u64).throughput());
    group.bench_function("inflate page-like data", |b| {
        b.iter(|| {
            for _ in 0..2 {
                black_box(inflate_raw(&deflated, &limits));
            }
        });
    });
    group.throughput(Per::Bytes(noise.len() as u64).throughput());
    group.bench_function("inflate near-incompressible data", |b| {
        b.iter(|| black_box(inflate_raw(&noisy, &limits)));
    });
    group.throughput(Per::Bytes(page_like.len() as u64).throughput());
    group.bench_function("LZW decode page-like data", |b| {
        b.iter(|| black_box(lzw_decode(&lzw, &limits, true, None)));
    });

    group.throughput(Per::Pixels(pixels).throughput());
    group.bench_function("baseline JPEG 2480x3508", |b| {
        b.iter(|| black_box(jpeg_decode(&baseline, CEILING)));
    });
    group.bench_function("progressive JPEG 2480x3508", |b| {
        b.iter(|| black_box(jpeg_decode(&progressive, CEILING)));
    });
    group.bench_function("irreversible JPEG 2000 2480x3508", |b| {
        b.iter(|| black_box(jpx_decode(&irreversible, &limits, &mut Vec::new())));
    });
    group.bench_function("reversible JPEG 2000 2480x3508", |b| {
        b.iter(|| black_box(jpx_decode(&reversible, &limits, &mut Vec::new())));
    });

    group.throughput(Per::Pixels(scan_pixels).throughput());
    group.bench_function("decode the JBIG2 generic page", |b| {
        b.iter(|| {
            black_box(jbig2_decode(
                &jbig2,
                &jbig2_params,
                CEILING,
                &mut Vec::new(),
            ))
        });
    });
    group.bench_function("decode the CCITT G4 page", |b| {
        b.iter(|| black_box(ccitt_decode(&g4, &g4_params, CEILING)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = decode
}
criterion_main!(benches);
