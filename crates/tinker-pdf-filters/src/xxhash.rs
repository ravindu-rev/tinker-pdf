//! XXH64, the hash behind a Zstandard frame's `Content_Checksum` (RFC 8878
//! §3.1.1: "the result of the xxh64() hash function digesting the original
//! (decoded) data as input, and a seed of zero"; the frame keeps its low
//! four bytes), and the one deduplication buckets streams by.
//!
//! Here rather than in the archive's Zstandard module because both callers
//! depend on this crate and neither on the other. Bytes in, a `u64` out.
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
#[must_use]
pub fn xxh64(data: &[u8]) -> u64 {
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

#[cfg(test)]
mod tests {
    use super::xxh64;

    /// The empty input, and lengths either side of every one the hash
    /// treats apart — the byte tail, the four-byte lane, the eight-byte
    /// lanes and the 32-byte stripes — hash to what the archive's copy
    /// returned before it moved here. The empty input's value is the one
    /// the specification publishes.
    #[test]
    fn xxh64_keeps_the_values_the_archive_copy_gave() {
        let pinned: [(usize, u64); 18] = [
            (0, 0xEF46_DB37_51D8_E999),
            (1, 0xF592_C0C7_639C_4CB6),
            (3, 0x22C0_8528_601D_4F27),
            (4, 0xFB1E_5CF2_F1AE_4D95),
            (5, 0x9D21_3ECA_1F68_F273),
            (7, 0x5613_AC51_0496_C04E),
            (8, 0x57CB_2B75_21F3_E21A),
            (9, 0xC3F1_D097_7525_7B66),
            (15, 0x90A9_714E_B00E_8D29),
            (16, 0xC684_3CEF_9972_1174),
            (31, 0xE4A0_E629_E519_A4AE),
            (32, 0xCC6B_8AAA_DA79_0B2D),
            (33, 0x35EC_4985_0475_A832),
            (63, 0xBF9F_0BA3_CF95_B28A),
            (64, 0x155C_CCE4_BF32_BEFC),
            (65, 0xDA5E_E441_A595_CFB8),
            (100, 0x4826_E367_566E_A023),
            (1000, 0x128D_A10C_FBDC_59D9),
        ];
        for (length, want) in pinned {
            let data: Vec<u8> = (0..length)
                .map(|at| (at as u8).wrapping_mul(37).wrapping_add(11))
                .collect();
            assert_eq!(xxh64(&data), want, "{length} bytes");
        }
    }
}
