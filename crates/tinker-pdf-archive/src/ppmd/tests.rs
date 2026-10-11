//! What the PPMd decoder is held to beside the committed archives.
//!
//! The streams below are pyppmd 1.3.1's `Ppmd7Encoder` — 7-Zip's own
//! `Ppmd7Enc.c` compiled for CPython, the encoder py7zr's `FILTER_PPMD` runs —
//! over inputs written here, with the order and arena size each name says.
//! Their expected answer is the input, never another decoder's output
//! (ruling 13). Two of them run in the **smallest arena 7-Zip accepts**,
//! 2 KiB, where the model fills after a few dozen bytes: every restart, every
//! glue of the free lists and every allocation from the text area is reached
//! within the first kilobyte, which is what a roomy default never does.

use super::*;

const ABRA: &[&str] = &["0061037c3b105fee1033f6d0cf0000"];
const ABRA_END: &[&str] = &["0061037c3b105fee1033f6dfd8c5c28000"];
const FOX_2K: &[&str] = &[
    "0054163bb6bc00c0541692710e586ff4e03c4f0e407b81e77d99bf58b26e386e59e53c822b878e23c53a369c",
    "40add987f25e1f5727ad1eba708352fe2dfe3eb7bd833f3d5ecd57f9b6a11198f8f78e46b21edc5f2390e7c5",
    "05441eef0e4ce95843540cad8ff7ec9dea7304529c9139ee2e2582cc2946725b8eeeb8da1359f548da4ab28d",
    "ee651f0274d2ced98393b86b3fcd893638e63866b1300ec23fb9c4b3f22e99dbd826170952928c529170a881",
    "26589fcbc66cc137517b7707d2bb40c00e762064c32a5cc274f2a4c1d9e1e06e0128a138d2917e22f633d751",
    "696aa6504c2b1127e8240ea8b8946108225cc97ef0fdedec5aff425a60214be355cbb780852a99ccc5929814",
    "1ce227347b00",
];
const BYTES_2K: &[&str] = &[
    "000004ec15c5e70501fd6b52b0561754c06d27eae7689d46c5680a19c98909764b8ea3883bebb453b311aa6a",
    "7fd925896f36687e5d9fc291a708b47f79f48343ab9ad87dc955ebd1b920586b58715b5f106ec26aa8e136b9",
    "d42bc52bc636535f3f0df9a6849b5b08ebf32283b1bf607b32acd65aec901808331103d672de892111a8c3bf",
    "8606f3c61c861036d1ccba13ee4c105a9610b1894ce146e28d87f909f97038261dcb08a060e18d56f8b4c5c8",
    "791800c3ed1aa5e9054e1685fff96ffd2eedb9c51f01c6d55171f1b4698a676f7ed7a69a8ebb86b1e36e718c",
    "158cf60295125d6bd0275e1715c2ed5decd1c96d75c3a5eb870239d1ccc6ac8d86107736be209fd55c4eccd8",
    "509df579a67cd5371c96a65d758e44e28ce5c518282f4882146b9a6f3a7007f5b98e56d9e36849e0afeefaae",
    "6afde82e10764553ea76bb82ae4209d173001c3e03c87874965a53be8861c9daede1e7d13ef5c6d3e6f1fb39",
    "2a7a6f1ef8821597cbe63170047aaeedb6464cebdbf3fc2ebb7695dc9af1c1ffc88adeeacea43397054c1de1",
    "d01612a51779b994279c772d10caa26964438c68783999a7d8163cbfd0e52fce4839671fc5ffe6eff3ff94dc",
    "5f14c80642cdfdc19541331a33cb0018fc417dfda9a65e5de7291b8883ca69a71440e6793639da3ceaed235e",
    "8bc1dbfb81144acc3fe59ca726605e27fab993f8381f8f78915c770163ff7dcc78a582222c3bdddddfaf4173",
    "88452da6f907da9c765f4178fa0d03c881b5e19cc265e5caa8ed9239ff0376dc4ae86a2a7ff86246881e3489",
    "99432f462be8ef182be8afbb6a215d827163c4d6059f1b44afb5ef33b3f20000",
];

fn hex(parts: &[&str]) -> Vec<u8> {
    let text: String = parts.concat();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn props(order: u8, memory: u32) -> Vec<u8> {
    let mut p = vec![order];
    p.extend_from_slice(&memory.to_le_bytes());
    p
}

const ROOMY: Limits = Limits {
    max_unpacked: 1 << 20,
    max_memory: 1 << 24,
};

#[test]
fn small_streams_from_7_zip_s_encoder_decode_to_what_went_in() {
    let abra = b"abracadabra abracadabra abracadabra";
    assert_eq!(
        decode(&hex(ABRA), &props(6, 1 << 16), abra.len(), &ROOMY).as_deref(),
        Ok(&abra[..])
    );
    // The same stream with the end-of-data escape after it: the declared
    // length is where the decode stops, and the marker is never reached.
    assert_eq!(
        decode(&hex(ABRA_END), &props(6, 1 << 16), abra.len(), &ROOMY).as_deref(),
        Ok(&abra[..])
    );
    // Asking for one byte more than was coded reaches the marker, which is a
    // stream shorter than its declaration.
    assert_eq!(
        decode(&hex(ABRA_END), &props(6, 1 << 16), abra.len() + 1, &ROOMY),
        Err(Error::Corrupt)
    );
}

/// The smallest arena, 2 KiB: 1 536 bytes of it are the order-0 context's
/// 256 states, so the model restarts every few dozen symbols, and the order-64
/// stream over every byte value twice cannot keep a context longer than one.
#[test]
fn the_smallest_arena_restarts_and_still_decodes() {
    let fox = b"The quick brown fox jumps over the lazy dog. ".repeat(8);
    assert_eq!(
        decode(&hex(FOX_2K), &props(2, 1 << 11), fox.len(), &ROOMY),
        Ok(fox)
    );
    let bytes: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
    assert_eq!(
        decode(&hex(BYTES_2K), &props(64, 1 << 11), bytes.len(), &ROOMY),
        Ok(bytes)
    );
}

#[test]
fn properties_outside_7_zip_s_ranges_are_refused() {
    let stream = hex(ABRA);
    for bad in [
        vec![],
        vec![6, 0, 0, 1],
        props(1, 1 << 16),
        props(65, 1 << 16),
        props(6, (1 << 11) - 1),
        props(6, u32::MAX - 35),
    ] {
        assert_eq!(
            decode(&stream, &bad, 1, &ROOMY),
            Err(Error::BadProperties),
            "{bad:?}"
        );
    }
    // py7zr writes seven bytes; the five that mean something are read.
    let mut seven = props(6, 1 << 16);
    seven.extend_from_slice(&[0, 0]);
    assert!(decode(&stream, &seven, 35, &ROOMY).is_ok());
}

#[test]
fn the_arena_and_the_output_are_bounded_before_anything_is_allocated() {
    let stream = hex(ABRA);
    let tight = Limits {
        max_unpacked: 35,
        max_memory: 1 << 15,
    };
    assert_eq!(
        decode(&stream, &props(6, 1 << 16), 35, &tight),
        Err(Error::TooLarge),
        "a 64 KiB arena against a 32 KiB cap"
    );
    assert_eq!(
        decode(&stream, &props(6, 1 << 15), 36, &tight),
        Err(Error::TooLarge),
        "36 bytes against a 35-byte cap"
    );
}

#[test]
fn a_bad_range_start_and_a_short_stream_are_refused_by_name() {
    let mut stream = hex(ABRA);
    stream[0] = 1;
    assert_eq!(
        decode(&stream, &props(6, 1 << 16), 35, &ROOMY),
        Err(Error::BadRangeStart)
    );
    assert_eq!(
        decode(&[0, 0xFF, 0xFF, 0xFF, 0xFF], &props(6, 1 << 16), 1, &ROOMY),
        Err(Error::BadRangeStart)
    );
    let stream = hex(FOX_2K);
    let fox = 45 * 8;
    for cut in [0, 4, 5, stream.len() / 2, stream.len() - 3] {
        assert!(
            decode(&stream[..cut], &props(2, 1 << 11), fox, &ROOMY).is_err(),
            "cut at {cut}"
        );
    }
}

/// Both end bits of every byte of every small stream flipped, and every
/// cut: each answers
/// something, within its cap, without a panic or a hang.
#[test]
fn every_bit_flip_and_every_cut_answers_rather_than_panics() {
    for (stream, order, memory, len) in [
        (hex(ABRA_END), 6u8, 1u32 << 16, 35usize),
        (hex(FOX_2K), 2, 1 << 11, 360),
        (hex(BYTES_2K), 64, 1 << 11, 512),
    ] {
        for at in 0..stream.len() {
            for bit in [0, 7] {
                let mut damaged = stream.clone();
                damaged[at] ^= 1 << bit;
                if let Ok(out) = decode(&damaged, &props(order, memory), len, &ROOMY) {
                    assert_eq!(out.len(), len);
                }
            }
        }
        for cut in 0..stream.len() {
            let _ = decode(&stream[..cut], &props(order, memory), len, &ROOMY);
        }
    }
}
