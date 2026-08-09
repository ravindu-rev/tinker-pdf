//! The standing properties: no panic on arbitrary bytes, and no decoder ever
//! produces more than `Limits::max_output` (ruling 1, and the zip-bomb row of
//! the plan's risk table).

use proptest::prelude::*;
use tinker_pdf_filters::{
    apply_chain, ascii85_decode, ascii_hex_decode, flate_decode, lzw_decode, predictor_decode,
    run_length_decode, ChainOutput, Filter, FilterSpec, Limits, PredictorParams,
};

fn params_strategy() -> impl Strategy<Value = PredictorParams> {
    (
        // Valid predictors, an undefined one, and a wild one.
        prop::sample::select(vec![1i32, 2, 10, 11, 12, 13, 14, 15, 7, 999, -3]),
        0u32..6,
        prop::sample::select(vec![1u32, 2, 4, 8, 16, 3, 0]),
        0u32..24,
    )
        .prop_map(
            |(predictor, colors, bits_per_component, columns)| PredictorParams {
                predictor,
                colors,
                bits_per_component,
                columns,
            },
        )
}

fn filter_strategy() -> impl Strategy<Value = FilterSpec> {
    (
        prop::sample::select(vec![
            Filter::Flate,
            Filter::Lzw,
            Filter::AsciiHex,
            Filter::Ascii85,
            Filter::RunLength,
            Filter::Dct,
            Filter::Jbig2,
        ]),
        prop::option::of(params_strategy()),
        any::<bool>(),
    )
        .prop_map(|(filter, predictor, early_change)| FilterSpec {
            filter,
            predictor,
            early_change,
        })
}

/// A valid raw-deflate stream of stored blocks, so arbitrary data can be fed
/// through the real block machinery without an encoder.
fn stored_blocks(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunks = data.chunks(7).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
        return out;
    }
    while let Some(c) = chunks.next() {
        let last = u8::from(chunks.peek().is_none());
        let len = c.len() as u16;
        out.push(last);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(c);
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn flate_never_panics_and_respects_the_cap(data in prop::collection::vec(any::<u8>(), 0..512), cap in 0usize..4096) {
        let limits = Limits::new(cap);
        if let Ok(d) = flate_decode(&data, &limits, None) {
            prop_assert!(d.data.len() <= cap);
        }
    }

    #[test]
    fn flate_with_a_predictor_never_panics(
        data in prop::collection::vec(any::<u8>(), 0..512),
        p in params_strategy(),
    ) {
        let limits = Limits::new(4096);
        if let Ok(d) = flate_decode(&data, &limits, Some(&p)) {
            prop_assert!(d.data.len() <= 4096);
        }
    }

    #[test]
    fn predictors_never_panic(
        data in prop::collection::vec(any::<u8>(), 0..512),
        p in params_strategy(),
        cap in 0usize..1024,
    ) {
        let limits = Limits::new(cap);
        if let Ok(d) = predictor_decode(&data, &p, &limits) {
            prop_assert!(d.data.len() <= cap);
        }
    }

    #[test]
    fn every_decoder_respects_max_output(
        data in prop::collection::vec(any::<u8>(), 0..512),
        cap in 0usize..600,
        early in any::<bool>(),
    ) {
        let limits = Limits::new(cap);
        prop_assert!(ascii_hex_decode(&data, &limits).data.len() <= cap);
        prop_assert!(ascii85_decode(&data, &limits).data.len() <= cap);
        prop_assert!(run_length_decode(&data, &limits).data.len() <= cap);
        if let Ok(d) = flate_decode(&data, &limits, None) {
            prop_assert!(d.data.len() <= cap);
        }
        if let Ok(d) = lzw_decode(&data, &limits, early, None) {
            prop_assert!(d.data.len() <= cap);
        }
    }

    #[test]
    fn chains_never_panic(
        data in prop::collection::vec(any::<u8>(), 0..256),
        chain in prop::collection::vec(filter_strategy(), 0..4),
        cap in 0usize..2048,
    ) {
        let limits = Limits::new(cap);
        match apply_chain(&data, &chain, &limits) {
            Ok(ChainOutput::Bytes(d)) => prop_assert!(d.data.len() <= cap),
            // Even a codec's still-encoded payload stays inside the ceiling.
            Ok(ChainOutput::EncodedImage { data, .. }) => prop_assert!(data.len() <= cap),
            Err(_) => {}
        }
    }

    #[test]
    fn stored_blocks_round_trip(data in prop::collection::vec(any::<u8>(), 0..300)) {
        let stream = stored_blocks(&data);
        let d = flate_decode(&stream, &Limits::new(1 << 16), None).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&d.data, &data);
        prop_assert!(d.complete);
    }

    #[test]
    fn any_prefix_of_a_valid_stream_decodes_to_a_prefix(
        data in prop::collection::vec(any::<u8>(), 1..300),
        cut in 0usize..64,
    ) {
        let stream = stored_blocks(&data);
        let n = stream.len().saturating_sub(cut);
        let d = flate_decode(&stream[..n], &Limits::new(1 << 16), None)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(data.starts_with(&d.data), "output is not a prefix of the input");
    }

    #[test]
    fn ascii_hex_round_trips(data in prop::collection::vec(any::<u8>(), 0..300)) {
        let mut enc = Vec::new();
        for b in &data {
            enc.extend_from_slice(format!("{b:02X}").as_bytes());
        }
        enc.push(b'>');
        let d = ascii_hex_decode(&enc, &Limits::new(1 << 16));
        prop_assert_eq!(&d.data, &data);
        prop_assert!(d.complete);
        prop_assert!(d.warnings.is_empty());
    }

    #[test]
    fn ascii85_round_trips(data in prop::collection::vec(any::<u8>(), 0..300)) {
        let mut enc = Vec::new();
        for chunk in data.chunks(4) {
            let mut buf = [0u8; 4];
            buf[..chunk.len()].copy_from_slice(chunk);
            let mut v = u32::from_be_bytes(buf);
            let mut group = [0u8; 5];
            for slot in group.iter_mut().rev() {
                *slot = b'!' + (v % 85) as u8;
                v /= 85;
            }
            enc.extend_from_slice(&group[..chunk.len() + 1]);
        }
        enc.extend_from_slice(b"~>");
        let d = ascii85_decode(&enc, &Limits::new(1 << 16));
        prop_assert_eq!(&d.data, &data);
        prop_assert!(d.complete);
    }

    #[test]
    fn run_length_round_trips(data in prop::collection::vec(any::<u8>(), 0..300)) {
        let mut enc = Vec::new();
        for chunk in data.chunks(128) {
            enc.push((chunk.len() - 1) as u8);
            enc.extend_from_slice(chunk);
        }
        enc.push(128);
        let d = run_length_decode(&enc, &Limits::new(1 << 16));
        prop_assert_eq!(&d.data, &data);
        prop_assert!(d.complete);
    }
}
