//! XXH64, the hash behind a Zstandard frame's `Content_Checksum` (RFC 8878
//! §3.1.1: "the result of the xxh64() hash function digesting the original
//! (decoded) data as input, and a seed of zero"; the frame keeps its low
//! four bytes).
//!
//! Written from the xxHash specification, `doc/xxhash_spec.md` version 0.2.0
//! in `Cyan4973/xxHash`, read on 26 September 2026: its "XXH64 Algorithm
//! Description", steps 1 to 6. The document grants copying and distribution
//! of itself; nothing of it is copied here but the five primes and the
//! arithmetic it describes.

const PRIME64_1: u64 = 0x9E37_79B1_85EB_CA87;
const PRIME64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const PRIME64_3: u64 = 0x1656_67B1_9E37_79F9;
const PRIME64_4: u64 = 0x85EB_CA77_C2B2_AE63;
const PRIME64_5: u64 = 0x27D4_EB2F_1656_67C5;

fn round(acc: u64, lane: u64) -> u64 {
    acc.wrapping_add(lane.wrapping_mul(PRIME64_2))
        .rotate_left(31)
        .wrapping_mul(PRIME64_1)
}

fn merge(acc: u64, lane_acc: u64) -> u64 {
    (acc ^ round(0, lane_acc))
        .wrapping_mul(PRIME64_1)
        .wrapping_add(PRIME64_4)
}

fn le64(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    for (slot, &b) in buf.iter_mut().zip(bytes) {
        *slot = b;
    }
    u64::from_le_bytes(buf)
}

/// XXH64 of `data` with seed 0.
pub(super) fn xxh64(data: &[u8]) -> u64 {
    let mut stripes = data.chunks_exact(32);
    let mut acc = if data.len() >= 32 {
        // Step 1: four accumulators; step 2: 32-byte stripes, a lane each.
        let mut v = [
            PRIME64_1.wrapping_add(PRIME64_2),
            PRIME64_2,
            0,
            0u64.wrapping_sub(PRIME64_1),
        ];
        for stripe in stripes.by_ref() {
            for (acc, lane) in v.iter_mut().zip(stripe.chunks_exact(8)) {
                *acc = round(*acc, le64(lane));
            }
        }
        // Step 3: convergence.
        let [v1, v2, v3, v4] = v;
        let acc = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        merge(merge(merge(merge(acc, v1), v2), v3), v4)
    } else {
        PRIME64_5
    };
    // Step 4: the length.
    acc = acc.wrapping_add(data.len() as u64);

    // Step 5: what no stripe took, eight, then four, then one at a time.
    let mut rest = stripes.remainder();
    while let Some((lane, tail)) = rest.split_first_chunk::<8>() {
        acc = (acc ^ round(0, u64::from_le_bytes(*lane)))
            .rotate_left(27)
            .wrapping_mul(PRIME64_1)
            .wrapping_add(PRIME64_4);
        rest = tail;
    }
    if let Some((lane, tail)) = rest.split_first_chunk::<4>() {
        acc = (acc ^ u64::from(u32::from_le_bytes(*lane)).wrapping_mul(PRIME64_1))
            .rotate_left(23)
            .wrapping_mul(PRIME64_2)
            .wrapping_add(PRIME64_3);
        rest = tail;
    }
    for &byte in rest {
        acc = (acc ^ u64::from(byte).wrapping_mul(PRIME64_5))
            .rotate_left(11)
            .wrapping_mul(PRIME64_1);
    }

    // Step 6: the avalanche.
    acc ^= acc >> 33;
    acc = acc.wrapping_mul(PRIME64_2);
    acc ^= acc >> 29;
    acc = acc.wrapping_mul(PRIME64_3);
    acc ^ (acc >> 32)
}
