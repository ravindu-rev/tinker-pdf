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
//! their unit counting every call. The quarter-scale JPEG decode counts the
//! stream's pixels, as the full-size decode the planning baseline runs in its
//! place does. The JPEG 2000 decodes with two levels discarded take four and
//! eight calls, which bring tier P's targets for them above the window's
//! 20 ms, the decodes of each codestream's centre quarter one call each, and
//! the 9/7 codestream's decode on four threads one call; no run has timed
//! those five yet, so the design's decode table gives their window as
//! untimed.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf::Document;
use tinker_pdf_filters::{
    ccitt_decode, deflate, inflate_raw, jbig2_decode, jpeg_decode, jpx_decode, lzw_decode,
    CcittParams, FilterError, Jbig2Params, JpxImage, Limits,
};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf_filters::{jpeg_decode_strips, JpegError, JpegLayout, JpegScale, JpegSink};

/// The ceiling every decode here is handed: well above the largest output,
/// an RGB page at 300 dpi.
const CEILING: usize = 1 << 28;

/// The decodes an iteration of the 9/7 codestream with two levels discarded
/// makes: tier P's 6.3 ms target a call, four times, is above 20 ms.
const IRREVERSIBLE_REDUCED_CALLS: u64 = 4;

/// The decodes an iteration of the 5/3 codestream with two levels discarded
/// makes: tier P's 2.8 ms target a call, eight times, is above 20 ms.
const REVERSIBLE_REDUCED_CALLS: u64 = 8;

/// Width times height.
fn area((width, height): (u32, u32)) -> u64 {
    u64::from(width) * u64::from(height)
}

/// An image of `size` with its top two resolution levels discarded: on the
/// reference grid reduced by four, each dimension `ceil(x / 4)` (T.800 B.5),
/// the committed codestreams' images standing at the grid's origin.
fn two_levels_fewer((width, height): (u32, u32)) -> (u32, u32) {
    (width.div_ceil(4), height.div_ceil(4))
}

/// The centre quarter of an image of `size`, as `(x, y, width, height)` from
/// its top-left corner: half its width and half its height, a quarter of
/// each dimension in.
fn centre((width, height): (u32, u32)) -> (u32, u32, u32, u32) {
    (width / 4, height / 4, width / 2, height / 2)
}

/// The rectangle `(x, y, width, height)` of an image `width` pixels wide,
/// three 8-bit samples a pixel.
#[cfg(not(tinker_planning_baseline))]
fn crop(samples: &[u8], width: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
    let stride = 3 * width as usize;
    let (left, right) = (3 * x as usize, 3 * (x + w) as usize);
    (y..y + h)
        .flat_map(|row| {
            let start = row as usize * stride;
            samples[start + left..start + right].iter().copied()
        })
        .collect()
}

/// `input` decoded with its top two resolution levels discarded.
#[cfg(not(tinker_planning_baseline))]
fn jpx_reduced(input: &[u8], limits: &Limits) -> Result<JpxImage, FilterError> {
    let request = tinker_pdf_filters::JpxRequest {
        discard: 2,
        window: None,
    };
    tinker_pdf_filters::jpx_decode_request(input, limits, &request, &mut Vec::new())
        .map(|(image, _)| image)
}

/// The planning baseline has no decode request: its arm runs the full
/// decode, the operation the gate pairs this one with (`baseline.json`'s
/// `paired_with`).
#[cfg(tinker_planning_baseline)]
fn jpx_reduced(input: &[u8], limits: &Limits) -> Result<JpxImage, FilterError> {
    jpx_decode(input, limits, &mut Vec::new())
}

/// The rectangle `(x, y, width, height)` of `input`'s image decoded at the
/// full size, and nothing outside it.
#[cfg(not(tinker_planning_baseline))]
fn jpx_window(
    input: &[u8],
    limits: &Limits,
    (x, y, width, height): (u32, u32, u32, u32),
) -> Result<JpxImage, FilterError> {
    let request = tinker_pdf_filters::JpxRequest {
        discard: 0,
        window: Some(tinker_pdf_filters::JpxWindow {
            x,
            y,
            width,
            height,
        }),
    };
    tinker_pdf_filters::jpx_decode_request(input, limits, &request, &mut Vec::new())
        .map(|(image, _)| image)
}

/// The planning baseline has no window: its arm runs the full decode, the
/// operation the gate pairs this one with (`baseline.json`'s `paired_with`).
#[cfg(tinker_planning_baseline)]
fn jpx_window(
    input: &[u8],
    limits: &Limits,
    _window: (u32, u32, u32, u32),
) -> Result<JpxImage, FilterError> {
    jpx_decode(input, limits, &mut Vec::new())
}

/// `input` decoded whole on four threads of the engine's own pool: its
/// code-blocks, tiles, components and wavelet levels shared out
/// (`jpx_decode_on` with `Threads(4)`).
#[cfg(not(tinker_planning_baseline))]
fn jpx_on_four(input: &[u8], limits: &Limits) -> Result<JpxImage, FilterError> {
    tinker_pdf_filters::jpx_decode_on(
        input,
        limits,
        &tinker_pdf_filters::JpxRequest::default(),
        &tinker_pdf::Parallelism::Threads(4),
        &mut Vec::new(),
    )
    .map(|(image, _)| image)
}

/// The planning baseline has no pool: its arm runs the one-thread full
/// decode, the operation the gate pairs this one with (`baseline.json`'s
/// `paired_with`).
#[cfg(tinker_planning_baseline)]
fn jpx_on_four(input: &[u8], limits: &Limits) -> Result<JpxImage, FilterError> {
    jpx_decode(input, limits, &mut Vec::new())
}

/// What a decode of an image of `size` with two levels discarded gives: the
/// discard asked for, applied, at [`two_levels_fewer`]'s size, every tile of
/// it decoded.
#[cfg(not(tinker_planning_baseline))]
fn check_reduced(image: &JpxImage, size: (u32, u32)) {
    let reduced = two_levels_fewer(size);
    assert_eq!(image.discard, 2, "both levels are discarded");
    assert_eq!((image.width, image.height), reduced);
    assert_eq!(image.samples.len(), 3 * area(reduced) as usize);
    assert!(!image.truncated, "every tile of the reduced image decodes");
}

/// The planning baseline's arm decodes the whole image, which the full
/// decode's own operation checks.
#[cfg(tinker_planning_baseline)]
fn check_reduced(_image: &JpxImage, _size: (u32, u32)) {}

/// What a window decode gives: exactly the same rectangle of `whole`, the
/// samples of the full decode of an image `width` pixels wide (P-10.3's
/// property, `jpx/tests/window.rs`).
#[cfg(not(tinker_planning_baseline))]
fn check_window(image: &JpxImage, whole: &[u8], width: u32, window: (u32, u32, u32, u32)) {
    let (_, _, w, h) = window;
    assert_eq!((image.width, image.height), (w, h), "the window's size");
    assert!(
        image.samples == crop(whole, width, window),
        "the window decode is its rectangle of the full decode, sample for sample"
    );
}

/// The planning baseline's arm decodes the whole image, which the full
/// decode's own operation checks.
#[cfg(tinker_planning_baseline)]
fn check_window(_image: &JpxImage, _whole: &[u8], _width: u32, _window: (u32, u32, u32, u32)) {}

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

/// The images the first page of the document `bytes` holds draws, their
/// samples as decoded and unconverted.
fn extracted(bytes: &Arc<[u8]>) -> Vec<tinker_pdf::PageImage> {
    Document::open(Arc::clone(bytes))
        .expect("the page opens")
        .page(0)
        .expect("the page")
        .images()
}

/// A strip decode's image gathered whole, as a caller that holds the
/// reduced image gathers it.
#[cfg(not(tinker_planning_baseline))]
#[derive(Default)]
struct Gathered {
    width: u32,
    height: u32,
    samples: Vec<u8>,
}

#[cfg(not(tinker_planning_baseline))]
impl JpegSink for Gathered {
    fn begin(&mut self, layout: JpegLayout) {
        self.width = layout.width;
        self.height = layout.height;
        let samples = layout.width as usize * layout.height as usize * layout.color.components();
        self.samples = Vec::with_capacity(samples);
    }

    fn strip(&mut self, _: u32, _: u32, data: &[u8]) {
        self.samples.extend_from_slice(data);
    }
}

/// `jpeg` decoded at a quarter of its size each way, gathered into one
/// image: its width, height and samples.
#[cfg(not(tinker_planning_baseline))]
fn jpeg_at_a_quarter(jpeg: &[u8]) -> Result<(u32, u32, Vec<u8>), JpegError> {
    let mut gathered = Gathered::default();
    jpeg_decode_strips(jpeg, CEILING, JpegScale::Quarter, &mut gathered)?;
    Ok((gathered.width, gathered.height, gathered.samples))
}

/// The planning baseline has no reduced decode: its arm runs the full-size
/// decode the gate pairs the operation with (`baseline.json`'s
/// `paired_with`).
#[cfg(tinker_planning_baseline)]
fn jpeg_at_a_quarter(jpeg: &[u8]) -> Result<(u32, u32, Vec<u8>), tinker_pdf_filters::JpegError> {
    jpeg_decode(jpeg, CEILING).map(|image| (image.width, image.height, image.data))
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
    group.operation("JPEG decode at quarter scale", |op| {
        let (baseline, _) = &*baseline;
        let (width, height, samples) =
            jpeg_at_a_quarter(baseline).expect("the JPEG decodes at a quarter");
        assert_eq!(samples.len(), 3 * width as usize * height as usize);
        if !cfg!(tinker_planning_baseline) {
            // A quarter of the stream each way, rounded up (P-09.1's
            // geometry): 310 x 439.
            assert_eq!(
                (width, height),
                (jpeg_size.0.div_ceil(4), jpeg_size.1.div_ceil(4)),
                "the quarter-scale image is a quarter of the stream each way"
            );
        }
        op.per(Per::Pixels(2 * area(jpeg_size)));
        op.iter(move || {
            for _ in 0..2 {
                let _ = black_box(jpeg_at_a_quarter(baseline));
            }
        });
    });
    group.operation("irreversible JPEG 2000 620x877", |op| {
        op.per(Per::Pixels(area(irreversible_size)));
        op.iter(|| black_box(jpx_decode(&irreversible, &limits, &mut Vec::new())));
    });
    group.operation("reversible JPEG 2000 310x438", |op| {
        op.per(Per::Pixels(area(reversible_size)));
        op.iter(|| black_box(jpx_decode(&reversible, &limits, &mut Vec::new())));
    });
    // The same two codestreams asked for less: two levels discarded, and the
    // centre quarter at the full size. The planning baseline can express
    // neither, so its arm of each runs the full decode it pairs with.
    group.operation(
        "irreversible JPEG 2000 620x877, two levels discarded",
        |op| {
            let irreversible: &[u8] = &irreversible;
            let limits = &limits;
            let checked = jpx_reduced(irreversible, limits).expect("it decodes");
            check_reduced(&checked, irreversible_size);
            op.per(Per::Pixels(
                IRREVERSIBLE_REDUCED_CALLS * area(two_levels_fewer(irreversible_size)),
            ));
            op.iter(move || {
                for _ in 0..IRREVERSIBLE_REDUCED_CALLS {
                    let _ = black_box(jpx_reduced(irreversible, limits));
                }
            });
        },
    );
    group.operation("reversible JPEG 2000 310x438, two levels discarded", |op| {
        let reversible: &[u8] = &reversible;
        let limits = &limits;
        let checked = jpx_reduced(reversible, limits).expect("it decodes");
        check_reduced(&checked, reversible_size);
        op.per(Per::Pixels(
            REVERSIBLE_REDUCED_CALLS * area(two_levels_fewer(reversible_size)),
        ));
        op.iter(move || {
            for _ in 0..REVERSIBLE_REDUCED_CALLS {
                let _ = black_box(jpx_reduced(reversible, limits));
            }
        });
    });
    group.operation("irreversible JPEG 2000 620x877, its 310x438 centre", |op| {
        let irreversible: &[u8] = &irreversible;
        let limits = &limits;
        let window = centre(irreversible_size);
        let whole = jpx_decode(irreversible, limits, &mut Vec::new()).expect("it decodes");
        let checked = jpx_window(irreversible, limits, window).expect("it decodes");
        check_window(&checked, &whole.samples, irreversible_size.0, window);
        op.per(Per::Pixels(area((window.2, window.3))));
        op.iter(move || black_box(jpx_window(irreversible, limits, window)));
    });
    // The lossless codestream's full decode is the generator's picture, so
    // its window is held to the picture's own rectangle.
    group.operation("reversible JPEG 2000 310x438, its 155x219 centre", |op| {
        let reversible: &[u8] = &reversible;
        let limits = &limits;
        let window = centre(reversible_size);
        let (width, height) = reversible_size;
        let checked = jpx_window(reversible, limits, window).expect("it decodes");
        check_window(&checked, &inputs::photo(width, height, 0), width, window);
        op.per(Per::Pixels(area((window.2, window.3))));
        op.iter(move || black_box(jpx_window(reversible, limits, window)));
    });
    // The 9/7 codestream whole on four threads of the engine's pool, the
    // image the one-thread decode's sample for sample. The planning baseline
    // has no pool, so its arm runs the one-thread decode the gate pairs this
    // one with.
    group.operation("irreversible JPEG 2000 620x877 on 4 threads", |op| {
        let irreversible: &[u8] = &irreversible;
        let limits = &limits;
        let whole = jpx_decode(irreversible, limits, &mut Vec::new()).expect("it decodes");
        let pooled = jpx_on_four(irreversible, limits).expect("it decodes");
        assert!(
            (pooled.width, pooled.height) == (whole.width, whole.height)
                && pooled.samples == whole.samples,
            "the four-thread decode is the one-thread decode, sample for sample"
        );
        op.per(Per::Pixels(area(irreversible_size)));
        op.iter(move || black_box(jpx_on_four(irreversible, limits)));
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

    // The page's image extracted raw through `Page::images`, the document
    // opened from its bytes each iteration: the Flate stream inflated and
    // its PNG predictor rows unfiltered, nothing converted.
    group.operation("extract a Flate image with predictors", |op| {
        let page: Arc<[u8]> = store
            .bytes("predicted-1240x1754.pdf", || inputs::predicted_page(2))
            .into();
        let (width, height) = jpeg_size;
        let images = extracted(&page);
        let [image] = &images[..] else {
            panic!("the page draws one image, not {}", images.len());
        };
        assert_eq!((image.width, image.height), (width, height));
        assert_eq!((image.bits_per_component, image.components), (8, 3));
        assert!(
            image.samples == inputs::photo(width, height, 2),
            "the extracted samples are the predicted rows unfiltered, exactly"
        );
        op.per(Per::Pixels(area(jpeg_size)));
        op.iter(move || black_box(extracted(&page)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = decode
}
criterion_main!(benches);
