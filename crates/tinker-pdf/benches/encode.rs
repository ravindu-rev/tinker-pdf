//! The encode group: bitmaps and data compressed.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `encode` list names these operations
//! in this order.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf::{Bitmap, Document, RenderOptions};
use tinker_pdf_filters::{inflate_raw, png_decode, Limits};

/// The first page of `bytes` at 150 dpi.
fn at_150(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(150.0))
}

/// `bitmap` as PNG, decoded back to its own size.
fn png_round_trip(bitmap: &Bitmap) -> Vec<u8> {
    let png = bitmap.to_png().expect("the bitmap encodes");
    let decoded = png_decode(&png, &Limits::new(1 << 28)).expect("the PNG decodes");
    assert_eq!(
        (decoded.width, decoded.height),
        (bitmap.width, bitmap.height),
        "the PNG is the bitmap's size"
    );
    png
}

/// `data` deflated by `code`, inflated back to itself.
fn deflate_round_trip(data: &[u8], code: fn(&[u8]) -> Vec<u8>) {
    let coded = code(data);
    let back = inflate_raw(&coded, &Limits::new(data.len() * 2));
    assert!(
        back.complete && back.data == data,
        "inflate gives the input back"
    );
}

/// `data` deflated at the ratio level.
#[cfg(not(tinker_planning_baseline))]
fn deflate_ratio(data: &[u8]) -> Vec<u8> {
    tinker_pdf_filters::deflate_at(data, tinker_pdf_filters::DeflateLevel::Ratio)
}

/// The planning baseline has no levels: its arm runs its own deflate, the
/// encoder the ratio level is held against.
#[cfg(tinker_planning_baseline)]
fn deflate_ratio(data: &[u8]) -> Vec<u8> {
    tinker_pdf_filters::deflate(data)
}

/// `data` deflated at the fast level.
#[cfg(not(tinker_planning_baseline))]
fn deflate_fast(data: &[u8]) -> Vec<u8> {
    tinker_pdf_filters::deflate_at(data, tinker_pdf_filters::DeflateLevel::Fast)
}

/// The planning baseline has no fast level: its arm runs the deflate the
/// gate pairs the operation with (`baseline.json`'s `paired_with`).
#[cfg(tinker_planning_baseline)]
fn deflate_fast(data: &[u8]) -> Vec<u8> {
    tinker_pdf_filters::deflate(data)
}

fn encode(c: &mut Criterion) {
    // A render's bitmap, which crosses a paired run as its pixels.
    let text = LazyCell::new(|| {
        let text = inputs::store().bitmap("dense-1-at-150.pixels", || at_150(inputs::dense(1, 0)));
        png_round_trip(&text);
        text
    });
    let blank = LazyCell::new(|| {
        let blank =
            inputs::store().bitmap("near-blank-at-150.pixels", || at_150(inputs::near_blank()));
        png_round_trip(&blank);
        blank
    });
    let page_like = LazyCell::new(|| {
        let page_like = inputs::store().bytes("page-like.rgb", inputs::page_like);
        assert_eq!(page_like.len(), inputs::PAGE_BYTES);
        let inked = page_like.chunks(3).filter(|pixel| pixel[0] < 128).count();
        assert!(
            (1240 * 1755 / 20..1240 * 1755 / 6).contains(&inked),
            "about a tenth of the page-like pixels are inked: {inked}"
        );
        deflate_round_trip(&page_like, deflate_ratio);
        page_like
    });
    let photo_like = LazyCell::new(|| {
        let photo_like = inputs::store().bytes("photo-like.rgb", inputs::photo_like);
        assert_eq!(photo_like.len(), inputs::PAGE_BYTES);
        deflate_round_trip(&photo_like, deflate_ratio);
        photo_like
    });

    let mut group = Group::new(c, "encode");

    group.operation("PNG of the 1240x1755 text page", |op| {
        op.per(support::pixels(&text));
        op.iter(|| black_box(text.to_png()));
    });
    group.operation("PNG of the near-blank page", |op| {
        op.per(Per::Pixels(
            2 * u64::from(blank.width) * u64::from(blank.height),
        ));
        op.iter(|| {
            for _ in 0..2 {
                black_box(blank.to_png());
            }
        });
    });
    group.operation("deflate page-like data", |op| {
        op.per(Per::Bytes(page_like.len() as u64));
        op.iter(|| black_box(deflate_ratio(&page_like)));
    });
    group.operation("deflate photo-like data", |op| {
        op.per(Per::Bytes(photo_like.len() as u64));
        op.iter(|| black_box(deflate_ratio(&photo_like)));
    });
    group.operation("deflate page-like data, fast level", |op| {
        deflate_round_trip(&page_like, deflate_fast);
        op.per(Per::Bytes(page_like.len() as u64));
        op.iter(|| black_box(deflate_fast(&page_like)));
    });
    group.operation("deflate photo-like data, fast level", |op| {
        deflate_round_trip(&photo_like, deflate_fast);
        op.per(Per::Bytes(photo_like.len() as u64));
        op.iter(|| black_box(deflate_fast(&photo_like)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = encode
}
criterion_main!(benches);
