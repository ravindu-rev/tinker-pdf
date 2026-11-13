//! AES-128 and AES-256 in CBC mode (FIPS 197, NIST SP 800-38A).
//!
//! Two padding disciplines, because PDF needs both: encrypted strings and
//! streams carry a leading 16-byte IV and PKCS#7 padding (7.6.2), while
//! Algorithm 2.B's hardened hash runs CBC with no padding at all and an
//! explicit IV (7.6.4.3.4).
//!
//! **This is a claim of constant-time execution, in the source.** Both
//! directions run on a sliced core (`docs/design/crypto-throughput.md`): the
//! state is eight planes of 64-bit words, plane `b` holding bit `b` of every
//! byte of the blocks a pass carries, four blocks a word; SubBytes is a
//! boolean circuit over the planes and its inverse the same circuit between
//! two inverse affine maps, and the linear layers are shifts and XORs, so no
//! table is read at an index a state byte or a key byte chooses, and nothing
//! branches on either. The key schedule runs SubWord through the same
//! circuit, and a decryption's padding check reads its last block whole and
//! decides by masks. A lone chain is a pass of one word a block. CBC
//! decryption, whose blocks are each the inverse cipher of a ciphertext block
//! the input already holds, decrypts eight blocks a pass of two words, a
//! shorter tail through the same pass with its other lanes empty. CBC
//! encryption is sequential within a chain, so several chains — a save's
//! streams, each under its own IV — are sealed side by side, a chain a lane,
//! each to the bytes it seals to alone. The core
//! records, in test builds, every table it reads and every condition it
//! tests, and two tests hold that record equal over random keys and blocks at
//! both widths (`the_core_reads_no_secret_indexed_table_and_branches_alike`
//! and `the_inverse_core_reads_no_secret_indexed_table_and_branches_alike`).
//! The circuits and the cipher are held to FIPS 197 at both widths when the
//! crate compiles: the `const` items below this module's core fail the build
//! if they disagree. What the source cannot show is the machine code a target
//! runs, which H-67 reviews.

/// FIPS 197's S-box (5.1.1). Nothing reads it but the `const` item that
/// holds the circuit in [`sub_bytes`] to it at every input, and its inverse
/// to [`inv_sub_bytes`], at both widths of a pass, when the crate compiles.
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const RCON: [u8; 11] = [
    0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36,
];

// ---------------------------------------------------------------------------
// The sliced core (`docs/design/crypto-throughput.md`).
// ---------------------------------------------------------------------------

/// One word of the sliced state: eight planes, plane `b` holding bit `b` of
/// every byte of four lanes, a lane a block. State byte `(r, c)` (FIPS 197
/// 3.4: input byte `r + 4c`) of lane `l` is bit `16r + 4c + l` of a plane —
/// rows outermost, then columns, the lanes innermost — so the state a row
/// down is each plane rotated by 16, and a column across each row's sixteen
/// bits rotated by four. A round key is one word, sliced into every lane.
type Word = [u64; 8];

/// The lanes a word holds.
const LANES: usize = 4;

/// A pass of the sliced state: `W` words abreast. Plane `b` of the pass is
/// the `W` words `[b]`, and lane `l` of word `w` is the pass's lane
/// `LANES·w + l`, a block of its own. Every operation of a pass is the same
/// on each word of a plane, so a pass of two words is a pair the vectoriser
/// can fuse, and a pass costs what it costs whatever lanes it fills.
type Planes<const W: usize> = [[u64; W]; 8];

/// The blocks a pass carries, word by word: `[w][l]` is lane `l` of word `w`.
type Blocks<const W: usize> = [[[u8; 16]; LANES]; W];

/// The words of the batched pass, which carries eight lanes; a lone chain's
/// pass is one word.
const BATCH_WORDS: usize = 2;

/// The lanes the batched pass carries.
const BATCH: usize = LANES * BATCH_WORDS;

/// The rounds of the longer key, which has one round key more.
const MAX_ROUNDS: usize = 14;

/// What the core records of itself in test builds: each table it reads, as
/// the table and the index, and each condition it tests, as the site and the
/// outcome, folded into one digest beside a count of each. Outside test
/// builds it holds and records nothing, so passing one costs nothing.
///
/// Two runs over different keys and blocks that end with one digest read the
/// same tables at the same indices and took the same branches: none of their
/// reads or branches moved with a secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Trace {
    #[cfg(test)]
    digest: u64,
    #[cfg(test)]
    reads: u64,
    #[cfg(test)]
    branches: u64,
}

impl Trace {
    /// A trace with nothing recorded.
    const fn new() -> Trace {
        Trace {
            #[cfg(test)]
            digest: 0xcbf2_9ce4_8422_2325,
            #[cfg(test)]
            reads: 0,
            #[cfg(test)]
            branches: 0,
        }
    }

    /// A read of `table` at `index`.
    #[inline(always)]
    const fn read(&mut self, table: u64, index: usize) {
        #[cfg(test)]
        {
            self.reads += 1;
            self.fold(1, table, index as u64);
        }
        #[cfg(not(test))]
        {
            let _ = (table, index);
        }
    }

    /// A condition tested at `site`, which comes out `taken`, as it returns.
    #[inline(always)]
    const fn branch(&mut self, site: u64, taken: bool) -> bool {
        #[cfg(test)]
        {
            self.branches += 1;
            self.fold(2, site, taken as u64);
        }
        #[cfg(not(test))]
        {
            let _ = site;
        }
        taken
    }

    /// A choice made at `site` among several ways, `way` the one taken.
    #[inline(always)]
    const fn choice(&mut self, site: u64, way: usize) {
        #[cfg(test)]
        {
            self.branches += 1;
            self.fold(3, site, way as u64);
        }
        #[cfg(not(test))]
        {
            let _ = (site, way);
        }
    }

    /// FNV-1a over an event's kind and its two words.
    #[cfg(test)]
    const fn fold(&mut self, kind: u64, first: u64, second: u64) {
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        self.digest = (self.digest ^ kind).wrapping_mul(PRIME);
        self.digest = (self.digest ^ first).wrapping_mul(PRIME);
        self.digest = (self.digest ^ second).wrapping_mul(PRIME);
    }
}

// The sites the core's conditions are recorded at, and its tables.
const SITE_LANE: u64 = 1;
const SITE_BYTE: u64 = 2;
const SITE_EXCHANGE: u64 = 3;
const SITE_PLANE: u64 = 4;
const SITE_ROUND: u64 = 5;
const SITE_DRIFT: u64 = 6;
const SITE_KEY_LENGTH: u64 = 7;
const SITE_KEY_WORD: u64 = 8;
const SITE_KEY_ROTATE: u64 = 9;
const SITE_KEY_SUBSTITUTE: u64 = 10;
const SITE_KEY_ROUND: u64 = 11;
const SITE_KEY_MOVE: u64 = 12;
const SITE_WORD: u64 = 13;
const TABLE_INTERLEAVE: u64 = 101;
const TABLE_RCON: u64 = 102;
const TABLE_ROUND_KEY: u64 = 103;

/// The byte of a block that each byte of the two 64-bit words a lane is
/// loaded as takes, low byte first: the first word the bytes of columns 0
/// and 2, the second those of columns 1 and 3, row by row, so byte `k` of the
/// first is `(r, c) = (k / 2, 2(k mod 2))` and of the second one column on.
///
/// Loaded so, the bit of weight `2^b` in byte `k` of load word `v` — `v` the
/// lane plus four for the second word, a row of the pass's word before the
/// planes are made — is at position `8k + b`, and [`exchange`], which swaps
/// the three bits of `b` with the three bits of `v`, puts it at bit
/// `16r + 4c + l` of plane `b`.
const INTERLEAVE: [usize; 16] = [0, 8, 1, 9, 2, 10, 3, 11, 4, 12, 5, 13, 6, 14, 7, 15];

/// Swaps, in each word of a pass, the bits of row `a` at `mask << shift` with
/// those of row `b` at `mask`.
#[inline(always)]
const fn swap_move<const W: usize>(
    words: &mut Planes<W>,
    a: usize,
    b: usize,
    mask: u64,
    shift: u32,
    trace: &mut Trace,
) {
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let t = ((words[a][w] >> shift) ^ words[b][w]) & mask;
        words[b][w] ^= t;
        words[a][w] ^= t << shift;
        w += 1;
    }
}

/// Over the eight rows of each word of a pass — its load words before, its
/// planes after — swaps each of the three bits of a bit's position, of weight
/// 1, 2 and 4, with the bit of the same weight of its row's index: three
/// layers of four swap-moves, each its own inverse and independent of the
/// others, so this is its own inverse.
const fn exchange<const W: usize>(words: &mut Planes<W>, trace: &mut Trace) {
    let mut i = 0;
    while trace.branch(SITE_EXCHANGE, i < 4) {
        swap_move(words, 2 * i, 2 * i + 1, 0x5555_5555_5555_5555, 1, trace);
        i += 1;
    }
    i = 0;
    while trace.branch(SITE_EXCHANGE, i < 4) {
        let a = (i & 1) + 4 * (i >> 1);
        swap_move(words, a, a + 2, 0x3333_3333_3333_3333, 2, trace);
        i += 1;
    }
    i = 0;
    while trace.branch(SITE_EXCHANGE, i < 4) {
        swap_move(words, i, i + 4, 0x0f0f_0f0f_0f0f_0f0f, 4, trace);
        i += 1;
    }
}

/// A pass's blocks, one a lane, into its planes.
const fn slice<const W: usize>(blocks: &Blocks<W>, trace: &mut Trace) -> Planes<W> {
    let mut words = [[0u64; W]; 8];
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let mut lane = 0;
        while trace.branch(SITE_LANE, lane < LANES) {
            let mut k = 0;
            while trace.branch(SITE_BYTE, k < 8) {
                trace.read(TABLE_INTERLEAVE, k);
                trace.read(TABLE_INTERLEAVE, k + 8);
                words[lane][w] |= (blocks[w][lane][INTERLEAVE[k]] as u64) << (8 * k);
                words[LANES + lane][w] |= (blocks[w][lane][INTERLEAVE[k + 8]] as u64) << (8 * k);
                k += 1;
            }
            lane += 1;
        }
        w += 1;
    }
    exchange(&mut words, trace);
    words
}

/// A pass's planes back into its blocks.
const fn unslice<const W: usize>(planes: &Planes<W>, trace: &mut Trace) -> Blocks<W> {
    let mut words = *planes;
    exchange(&mut words, trace);
    let mut blocks = [[[0u8; 16]; LANES]; W];
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let mut lane = 0;
        while trace.branch(SITE_LANE, lane < LANES) {
            let mut k = 0;
            while trace.branch(SITE_BYTE, k < 8) {
                trace.read(TABLE_INTERLEAVE, k);
                trace.read(TABLE_INTERLEAVE, k + 8);
                blocks[w][lane][INTERLEAVE[k]] = (words[lane][w] >> (8 * k)) as u8;
                blocks[w][lane][INTERLEAVE[k + 8]] = (words[LANES + lane][w] >> (8 * k)) as u8;
                k += 1;
            }
            lane += 1;
        }
        w += 1;
    }
    blocks
}

/// SubBytes (FIPS 197 5.1.1) over every byte of a pass, a word at a time: the
/// depth-16 circuit of 128 gates Boyar and Peralta give for the AES S-box
/// (2012) — a linear layer of 27 XORs, a non-linear middle of 34 ANDs and 29
/// XORs computing the inverse in a tower field, and a linear layer of 30 XORs
/// and 8 more for the output, four of them XNORs that add the constant 0x63.
/// `u0` and `s0` are the most significant bits. No table, no branch.
const fn sub_bytes<const W: usize>(x: &mut Planes<W>, trace: &mut Trace) {
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let (u0, u1, u2, u3) = (x[7][w], x[6][w], x[5][w], x[4][w]);
        let (u4, u5, u6, u7) = (x[3][w], x[2][w], x[1][w], x[0][w]);

        let t1 = u0 ^ u3;
        let t2 = u0 ^ u5;
        let t3 = u0 ^ u6;
        let t4 = u3 ^ u5;
        let t5 = u4 ^ u6;
        let t6 = t1 ^ t5;
        let t7 = u1 ^ u2;
        let t8 = u7 ^ t6;
        let t9 = u7 ^ t7;
        let t10 = t6 ^ t7;
        let t11 = u1 ^ u5;
        let t12 = u2 ^ u5;
        let t13 = t3 ^ t4;
        let t14 = t6 ^ t11;
        let t15 = t5 ^ t11;
        let t16 = t5 ^ t12;
        let t17 = t9 ^ t16;
        let t18 = u3 ^ u7;
        let t19 = t7 ^ t18;
        let t20 = t1 ^ t19;
        let t21 = u6 ^ u7;
        let t22 = t7 ^ t21;
        let t23 = t2 ^ t22;
        let t24 = t2 ^ t10;
        let t25 = t20 ^ t17;
        let t26 = t3 ^ t16;
        let t27 = t1 ^ t12;

        let m1 = t13 & t6;
        let m2 = t23 & t8;
        let m3 = t14 ^ m1;
        let m4 = t19 & u7;
        let m5 = m4 ^ m1;
        let m6 = t3 & t16;
        let m7 = t22 & t9;
        let m8 = t26 ^ m6;
        let m9 = t20 & t17;
        let m10 = m9 ^ m6;
        let m11 = t1 & t15;
        let m12 = t4 & t27;
        let m13 = m12 ^ m11;
        let m14 = t2 & t10;
        let m15 = m14 ^ m11;
        let m16 = m3 ^ m2;
        let m17 = m5 ^ t24;
        let m18 = m8 ^ m7;
        let m19 = m10 ^ m15;
        let m20 = m16 ^ m13;
        let m21 = m17 ^ m15;
        let m22 = m18 ^ m13;
        let m23 = m19 ^ t25;
        let m24 = m22 ^ m23;
        let m25 = m22 & m20;
        let m26 = m21 ^ m25;
        let m27 = m20 ^ m21;
        let m28 = m23 ^ m25;
        let m29 = m28 & m27;
        let m30 = m26 & m24;
        let m31 = m20 & m23;
        let m32 = m27 & m31;
        let m33 = m27 ^ m25;
        let m34 = m21 & m22;
        let m35 = m24 & m34;
        let m36 = m24 ^ m25;
        let m37 = m21 ^ m29;
        let m38 = m32 ^ m33;
        let m39 = m23 ^ m30;
        let m40 = m35 ^ m36;
        let m41 = m38 ^ m40;
        let m42 = m37 ^ m39;
        let m43 = m37 ^ m38;
        let m44 = m39 ^ m40;
        let m45 = m42 ^ m41;
        let m46 = m44 & t6;
        let m47 = m40 & t8;
        let m48 = m39 & u7;
        let m49 = m43 & t16;
        let m50 = m38 & t9;
        let m51 = m37 & t17;
        let m52 = m42 & t15;
        let m53 = m45 & t27;
        let m54 = m41 & t10;
        let m55 = m44 & t13;
        let m56 = m40 & t23;
        let m57 = m39 & t19;
        let m58 = m43 & t3;
        let m59 = m38 & t22;
        let m60 = m37 & t20;
        let m61 = m42 & t1;
        let m62 = m45 & t4;
        let m63 = m41 & t2;

        let l0 = m61 ^ m62;
        let l1 = m50 ^ m56;
        let l2 = m46 ^ m48;
        let l3 = m47 ^ m55;
        let l4 = m54 ^ m58;
        let l5 = m49 ^ m61;
        let l6 = m62 ^ l5;
        let l7 = m46 ^ l3;
        let l8 = m51 ^ m59;
        let l9 = m52 ^ m53;
        let l10 = m53 ^ l4;
        let l11 = m60 ^ l2;
        let l12 = m48 ^ m51;
        let l13 = m50 ^ l0;
        let l14 = m52 ^ m61;
        let l15 = m55 ^ l1;
        let l16 = m56 ^ l0;
        let l17 = m57 ^ l1;
        let l18 = m58 ^ l8;
        let l19 = m63 ^ l4;
        let l20 = l0 ^ l1;
        let l21 = l1 ^ l7;
        let l22 = l3 ^ l12;
        let l23 = l18 ^ l2;
        let l24 = l15 ^ l9;
        let l25 = l6 ^ l10;
        let l26 = l7 ^ l9;
        let l27 = l8 ^ l10;
        let l28 = l11 ^ l14;
        let l29 = l11 ^ l17;

        x[7][w] = l6 ^ l24;
        x[6][w] = !(l16 ^ l26);
        x[5][w] = !(l19 ^ l28);
        x[4][w] = l6 ^ l21;
        x[3][w] = l20 ^ l22;
        x[2][w] = l25 ^ l29;
        x[1][w] = !(l13 ^ l27);
        x[0][w] = !(l6 ^ l23);
        w += 1;
    }
}

/// FIPS 197 5.3.2's inverse of the affine map, over planes:
/// `b'_i = b_(i+2) ⊕ b_(i+5) ⊕ b_(i+7) ⊕ d_i` with `d` = 0x05, which sets
/// bits 0 and 2.
#[inline(always)]
const fn inv_affine<const W: usize>(x: &mut Planes<W>, trace: &mut Trace) {
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let v = [
            x[0][w], x[1][w], x[2][w], x[3][w], x[4][w], x[5][w], x[6][w], x[7][w],
        ];
        x[0][w] = !(v[2] ^ v[5] ^ v[7]);
        x[1][w] = v[3] ^ v[6] ^ v[0];
        x[2][w] = !(v[4] ^ v[7] ^ v[1]);
        x[3][w] = v[5] ^ v[0] ^ v[2];
        x[4][w] = v[6] ^ v[1] ^ v[3];
        x[5][w] = v[7] ^ v[2] ^ v[4];
        x[6][w] = v[0] ^ v[3] ^ v[5];
        x[7][w] = v[1] ^ v[4] ^ v[6];
        w += 1;
    }
}

/// InvSubBytes (5.3.2): the S-box is the affine map of the inverse in
/// GF(2^8), so the inverse S-box is the inverse affine map of the S-box of
/// the inverse affine map — the forward circuit between two of them, and no
/// table.
#[inline(always)]
const fn inv_sub_bytes<const W: usize>(x: &mut Planes<W>, trace: &mut Trace) {
    inv_affine(x, trace);
    sub_bytes(x, trace);
    inv_affine(x, trace);
}

/// Each sixteen-bit row of `x` rotated right by `shift`, a multiple of four
/// under sixteen: column `c + shift / 4` of each row brought to column `c`.
#[inline(always)]
const fn across(x: u64, shift: u32) -> u64 {
    const ROWS: u64 = 0x0001_0001_0001_0001;
    let low = (0xffff_u64 >> shift) * ROWS;
    let high = ((0xffff_u64 << (16 - shift)) & 0xffff) * ROWS;
    ((x >> shift) & low) | ((x << (16 - shift)) & high)
}

/// `x` moved `down` rows and `right` columns: the byte at
/// `(r + down, c + right)` brought to `(r, c)`, both modulo 4.
#[inline(always)]
const fn moved(x: u64, down: u32, right: u32) -> u64 {
    across(x.rotate_right(16 * down), (4 * right) % 16)
}

/// `MC_(J) = SR^-J ∘ MixColumns ∘ SR^J` (FIPS 197 5.1.3 conjugated), a word
/// of the pass at a time: the mixing of the bytes `(r + i, c + J·i)`, the
/// column of the state the fixsliced rounds hold read along a diagonal. With
/// `y` the state XORed with itself moved a row down and `J` columns across,
/// and `s` the sum of the four bytes of a diagonal — `y` XORed with itself
/// moved two rows and `2J` columns — the row `2a_0 ⊕ 3a_1 ⊕ a_2 ⊕ a_3` is
/// `xtime(y) ⊕ s ⊕ a_0`, and `xtime` on planes is a renaming of them and
/// three XORs, the reduction by 0x1B.
#[inline(always)]
const fn mix<const J: u32, const W: usize>(x: &mut Planes<W>, trace: &mut Trace) {
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let mut y = [0u64; 8];
        let mut b = 0;
        while trace.branch(SITE_PLANE, b < 8) {
            y[b] = x[b][w] ^ moved(x[b][w], 1, J);
            b += 1;
        }
        let top = y[7];
        let doubled = [
            top,
            y[0] ^ top,
            y[1],
            y[2] ^ top,
            y[3] ^ top,
            y[4],
            y[5],
            y[6],
        ];
        b = 0;
        while trace.branch(SITE_PLANE, b < 8) {
            let s = y[b] ^ moved(y[b], 2, 2 * J);
            x[b][w] ^= doubled[b] ^ s;
            b += 1;
        }
        w += 1;
    }
}

/// Round `round`'s mixing: `MC_(round mod 4)`, the round's index public.
#[inline(always)]
const fn mix_columns<const W: usize>(x: &mut Planes<W>, round: usize, trace: &mut Trace) {
    let drift = round % 4;
    trace.choice(SITE_DRIFT, drift);
    match drift {
        0 => mix::<0, W>(x, trace),
        1 => mix::<1, W>(x, trace),
        2 => mix::<2, W>(x, trace),
        _ => mix::<3, W>(x, trace),
    }
}

/// `SR^-J ∘ InvMixColumns ∘ SR^J` (5.3.3 conjugated). InvMixColumns' row
/// `[0E 0B 0D 09]` is MixColumns' `[02 03 01 01]` times `[05 00 04 00]` as
/// circulants, so it is `D` and then MixColumns, `D(a)_r = a_r ⊕ 4(a_r ⊕
/// a_(r+2))`; conjugated alike, `D`'s neighbour is two rows down and `2J`
/// columns across, and `4·` on planes is a renaming of them and six XORs.
#[inline(always)]
const fn unmix<const J: u32, const W: usize>(x: &mut Planes<W>, trace: &mut Trace) {
    let mut w = 0;
    while trace.branch(SITE_WORD, w < W) {
        let mut y = [0u64; 8];
        let mut b = 0;
        while trace.branch(SITE_PLANE, b < 8) {
            y[b] = x[b][w] ^ moved(x[b][w], 2, 2 * J);
            b += 1;
        }
        let quadrupled = [
            y[6],
            y[6] ^ y[7],
            y[0] ^ y[7],
            y[1] ^ y[6],
            y[2] ^ y[6] ^ y[7],
            y[3] ^ y[7],
            y[4],
            y[5],
        ];
        b = 0;
        while trace.branch(SITE_PLANE, b < 8) {
            x[b][w] ^= quadrupled[b];
            b += 1;
        }
        w += 1;
    }
    mix::<J, W>(x, trace);
}

/// The inverse cipher's mixing for a state held as `SR^moved` of the
/// cipher's: `SR^moved ∘ InvMixColumns ∘ SR^-moved`, which is [`unmix`] at
/// the drift `−moved` modulo 4, the count public.
#[inline(always)]
const fn unmix_columns<const W: usize>(x: &mut Planes<W>, moved: usize, trace: &mut Trace) {
    let drift = (4 - moved % 4) % 4;
    trace.choice(SITE_DRIFT, drift);
    match drift {
        0 => unmix::<0, W>(x, trace),
        1 => unmix::<1, W>(x, trace),
        2 => unmix::<2, W>(x, trace),
        _ => unmix::<3, W>(x, trace),
    }
}

/// `SR²`, ShiftRows twice: rows 1 and 3 moved two columns across, rows 0 and
/// 2 as they stand.
#[inline(always)]
const fn shift_rows_twice<const W: usize>(x: &mut Planes<W>, trace: &mut Trace) {
    const ROWS_0_2: u64 = 0x0000_ffff_0000_ffff;
    let mut b = 0;
    while trace.branch(SITE_PLANE, b < 8) {
        let mut w = 0;
        while trace.branch(SITE_WORD, w < W) {
            x[b][w] = (x[b][w] & ROWS_0_2) | (across(x[b][w], 8) & !ROWS_0_2);
            w += 1;
        }
        b += 1;
    }
}

/// AddRoundKey (5.1.4) with a key sliced into every lane, into every word of
/// a pass.
#[inline(always)]
const fn add_round_key<const W: usize>(x: &mut Planes<W>, key: &Word, trace: &mut Trace) {
    let mut b = 0;
    while trace.branch(SITE_PLANE, b < 8) {
        let mut w = 0;
        while trace.branch(SITE_WORD, w < W) {
            x[b][w] ^= key[b];
            w += 1;
        }
        b += 1;
    }
}

/// An expanded key: its round keys each sliced into every lane of a word, as
/// the fixsliced cipher and inverse cipher take them — round key `i` moved by
/// `SR^-i` for the cipher and by `SR^(rounds − i)` for the inverse cipher,
/// for `0 < i < rounds`, and the first and the last as they are for both.
#[derive(Clone, Copy)]
struct Expanded {
    /// 10 for a 16-byte key, 14 for a 32-byte one.
    rounds: usize,
    /// The cipher's round keys, sliced.
    enc: [Word; MAX_ROUNDS + 1],
    /// The inverse cipher's round keys, sliced.
    dec: [Word; MAX_ROUNDS + 1],
}

/// A round key sliced into every lane of a word.
const fn key_word(key: [u8; 16], trace: &mut Trace) -> Word {
    let planes = slice::<1>(&[[key; LANES]], trace);
    let mut word = [0u64; 8];
    let mut b = 0;
    while trace.branch(SITE_PLANE, b < 8) {
        word[b] = planes[b][0];
        b += 1;
    }
    word
}

/// FIPS 197 5.2's SubWord through the circuit: the word in lane 0's first
/// column of a one-word pass, sliced, substituted and unsliced.
const fn sub_word(word: [u8; 4], trace: &mut Trace) -> [u8; 4] {
    let mut block = [0u8; 16];
    block[0] = word[0];
    block[1] = word[1];
    block[2] = word[2];
    block[3] = word[3];
    let mut planes = slice::<1>(&[[block, [0; 16], [0; 16], [0; 16]]], trace);
    sub_bytes(&mut planes, trace);
    let out = unslice(&planes, trace);
    [out[0][0][0], out[0][0][1], out[0][0][2], out[0][0][3]]
}

/// FIPS 197 5.2's key expansion of a 16- or 32-byte key, SubWord run through
/// the circuit, and the round keys sliced; `None` for any other length.
///
/// Every branch is on a public count: the key's length, a word's index in the
/// schedule and a round's. The round constants are read at a round's index.
const fn expand(key: &[u8], trace: &mut Trace) -> Option<Expanded> {
    trace.choice(SITE_KEY_LENGTH, key.len());
    let (nk, rounds) = match key.len() {
        16 => (4, 10),
        32 => (8, 14),
        _ => return None,
    };
    let total = 4 * (rounds + 1);
    let mut words = [[0u8; 4]; 4 * (MAX_ROUNDS + 1)];
    let mut i = 0;
    while trace.branch(SITE_KEY_WORD, i < nk) {
        words[i] = [key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]];
        i += 1;
    }
    while trace.branch(SITE_KEY_WORD, i < total) {
        let mut temp = words[i - 1];
        if trace.branch(SITE_KEY_ROTATE, i % nk == 0) {
            temp = sub_word([temp[1], temp[2], temp[3], temp[0]], trace);
            trace.read(TABLE_RCON, i / nk);
            temp[0] ^= RCON[i / nk];
        } else if trace.branch(SITE_KEY_SUBSTITUTE, nk > 6 && i % nk == 4) {
            // 5.2: AES-256 substitutes the fourth word of each group as well.
            temp = sub_word(temp, trace);
        }
        let before = words[i - nk];
        words[i] = [
            before[0] ^ temp[0],
            before[1] ^ temp[1],
            before[2] ^ temp[2],
            before[3] ^ temp[3],
        ];
        i += 1;
    }

    let mut enc = [[0u64; 8]; MAX_ROUNDS + 1];
    let mut dec = [[0u64; 8]; MAX_ROUNDS + 1];
    let mut round = 0;
    while trace.branch(SITE_KEY_ROUND, round <= rounds) {
        let ends = round == 0 || round == rounds;
        let (back, on) = if trace.branch(SITE_KEY_MOVE, ends) {
            (0, 0)
        } else {
            (round % 4, (rounds - round) % 4)
        };
        // The cipher's `SR^-back`: byte (r, c) takes the round key's
        // (r, c − back·r). The inverse cipher's `SR^on`: (r, c + on·r).
        let mut forward = [0u8; 16];
        let mut inverse = [0u8; 16];
        let mut c = 0;
        while trace.branch(SITE_BYTE, c < 4) {
            let mut r = 0;
            while trace.branch(SITE_BYTE, r < 4) {
                forward[r + 4 * c] = words[4 * round + (c + 4 - (back * r) % 4) % 4][r];
                inverse[r + 4 * c] = words[4 * round + (c + on * r) % 4][r];
                r += 1;
            }
            c += 1;
        }
        enc[round] = key_word(forward, trace);
        dec[round] = key_word(inverse, trace);
        round += 1;
    }
    Some(Expanded { rounds, enc, dec })
}

/// FIPS 197 5.1's cipher, fixsliced, over every lane of a pass: the state
/// after round `i` held as `SR^-i` of the cipher's, so that round `i` is
/// SubBytes, `MC_(i mod 4)` and its moved round key, and ShiftRows runs once,
/// as `SR²`, before the last round key — the round count being 2 modulo 4 for
/// both key sizes.
const fn encrypt_pass<const W: usize>(keys: &Expanded, x: &mut Planes<W>, trace: &mut Trace) {
    trace.read(TABLE_ROUND_KEY, 0);
    add_round_key(x, &keys.enc[0], trace);
    let mut round = 1;
    while trace.branch(SITE_ROUND, round < keys.rounds) {
        sub_bytes(x, trace);
        mix_columns(x, round, trace);
        trace.read(TABLE_ROUND_KEY, round);
        add_round_key(x, &keys.enc[round], trace);
        round += 1;
    }
    sub_bytes(x, trace);
    shift_rows_twice(x, trace);
    trace.read(TABLE_ROUND_KEY, keys.rounds);
    add_round_key(x, &keys.enc[keys.rounds], trace);
}

/// A block through the cipher alone in lane 0 of a one-word pass.
const fn encrypt_alone(keys: &Expanded, block: [u8; 16], trace: &mut Trace) -> [u8; 16] {
    let mut planes = slice::<1>(&[[block, [0; 16], [0; 16], [0; 16]]], trace);
    encrypt_pass(keys, &mut planes, trace);
    unslice(&planes, trace)[0][0]
}

/// FIPS 197 5.3's inverse cipher, fixsliced the other way, over every lane of
/// a pass: the state before round `i` is undone held as `SR^(rounds − i)` of
/// the inverse cipher's, one more each round, so that InvShiftRows never runs
/// between rounds — a round is its moved round key, [`unmix_columns`] and
/// InvSubBytes — and the plaintext is `SR²` of the last state, the round
/// count being 2 modulo 4, before the first round key.
const fn decrypt_pass<const W: usize>(keys: &Expanded, x: &mut Planes<W>, trace: &mut Trace) {
    trace.read(TABLE_ROUND_KEY, keys.rounds);
    add_round_key(x, &keys.dec[keys.rounds], trace);
    inv_sub_bytes(x, trace);
    let mut round = keys.rounds - 1;
    while trace.branch(SITE_ROUND, round > 0) {
        trace.read(TABLE_ROUND_KEY, round);
        add_round_key(x, &keys.dec[round], trace);
        unmix_columns(x, keys.rounds - round, trace);
        inv_sub_bytes(x, trace);
        round -= 1;
    }
    shift_rows_twice(x, trace);
    trace.read(TABLE_ROUND_KEY, 0);
    add_round_key(x, &keys.dec[0], trace);
}

/// A block through the inverse cipher alone in lane 0 of a one-word pass.
const fn decrypt_alone(keys: &Expanded, block: [u8; 16], trace: &mut Trace) -> [u8; 16] {
    let mut planes = slice::<1>(&[[block, [0; 16], [0; 16], [0; 16]]], trace);
    decrypt_pass(keys, &mut planes, trace);
    unslice(&planes, trace)[0][0]
}

// ---------------------------------------------------------------------------
// The core held to FIPS 197 when the crate compiles.
// ---------------------------------------------------------------------------

const fn same(a: &[u8; 16], b: &[u8; 16]) -> bool {
    let mut at = 0;
    while at < 16 {
        if a[at] != b[at] {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether the circuit is FIPS 197's S-box at all 256 inputs, in passes of
/// `W` words: the `64W` inputs of each pass sliced, substituted and unsliced.
const fn circuit_is_the_table<const W: usize>() -> bool {
    let carried = 16 * LANES * W;
    let mut pass = 0;
    while pass * carried < 256 {
        let mut blocks = [[[0u8; 16]; LANES]; W];
        let mut at = 0;
        while at < carried {
            blocks[at / 64][at / 16 % LANES][at % 16] = (carried * pass + at) as u8;
            at += 1;
        }
        let mut trace = Trace::new();
        let mut planes = slice(&blocks, &mut trace);
        sub_bytes(&mut planes, &mut trace);
        let out = unslice(&planes, &mut trace);
        at = 0;
        while at < carried {
            if out[at / 64][at / 16 % LANES][at % 16] != SBOX[carried * pass + at] {
                return false;
            }
            at += 1;
        }
        pass += 1;
    }
    true
}

/// Whether the inverse circuit is the inverse of FIPS 197's S-box at all 256
/// inputs, in passes of `W` words, as [`circuit_is_the_table`] holds the
/// circuit to the table.
const fn inverse_is_the_tables_inverse<const W: usize>() -> bool {
    let mut inverse = [0u8; 256];
    let mut at = 0;
    while at < 256 {
        inverse[SBOX[at] as usize] = at as u8;
        at += 1;
    }
    let carried = 16 * LANES * W;
    let mut pass = 0;
    while pass * carried < 256 {
        let mut blocks = [[[0u8; 16]; LANES]; W];
        at = 0;
        while at < carried {
            blocks[at / 64][at / 16 % LANES][at % 16] = (carried * pass + at) as u8;
            at += 1;
        }
        let mut trace = Trace::new();
        let mut planes = slice(&blocks, &mut trace);
        inv_sub_bytes(&mut planes, &mut trace);
        let out = unslice(&planes, &mut trace);
        at = 0;
        while at < carried {
            if out[at / 64][at / 16 % LANES][at % 16] != inverse[carried * pass + at] {
                return false;
            }
            at += 1;
        }
        pass += 1;
    }
    true
}

/// Whether slicing a pass of `W` words puts byte `(r, c)` of lane `l` of word
/// `w` at bit `16r + 4c + l` of word `w` of every plane and nowhere else, and
/// unslicing gives every block back.
const fn slicing_is_the_layout<const W: usize>() -> bool {
    let mut n = 0;
    while n < 16 {
        let mut word = 0;
        while word < W {
            let mut lane = 0;
            while lane < LANES {
                let mut blocks = [[[0u8; 16]; LANES]; W];
                blocks[word][lane][n] = 0xff;
                let mut trace = Trace::new();
                let planes = slice(&blocks, &mut trace);
                let bit = 1u64 << (16 * (n % 4) + 4 * (n / 4) + lane);
                let mut b = 0;
                while b < 8 {
                    let mut w = 0;
                    while w < W {
                        let expected = if w == word { bit } else { 0 };
                        if planes[b][w] != expected {
                            return false;
                        }
                        w += 1;
                    }
                    b += 1;
                }
                let back = unslice(&planes, &mut trace);
                let mut w = 0;
                while w < W {
                    let mut l = 0;
                    while l < LANES {
                        if !same(&back[w][l], &blocks[w][l]) {
                            return false;
                        }
                        l += 1;
                    }
                    w += 1;
                }
                lane += 1;
            }
            word += 1;
        }
        n += 1;
    }
    true
}

/// Whether, in a pass of `W` words, `plain` in every lane encrypts to
/// `cipher` and `cipher` in every lane decrypts to `plain`, and `4W`
/// different blocks abreast come out, either way, as each does alone.
const fn abreast<const W: usize>(keys: &Expanded, plain: [u8; 16], cipher: [u8; 16]) -> bool {
    let mut trace = Trace::new();
    let mut planes = slice(&[[plain; LANES]; W], &mut trace);
    encrypt_pass(keys, &mut planes, &mut trace);
    let sealed = unslice(&planes, &mut trace);
    let mut planes = slice(&[[cipher; LANES]; W], &mut trace);
    decrypt_pass(keys, &mut planes, &mut trace);
    let opened = unslice(&planes, &mut trace);
    let mut blocks = [[[0u8; 16]; LANES]; W];
    let mut w = 0;
    while w < W {
        let mut lane = 0;
        while lane < LANES {
            if !same(&sealed[w][lane], &cipher) || !same(&opened[w][lane], &plain) {
                return false;
            }
            let mut at = 0;
            while at < 16 {
                blocks[w][lane][at] = plain[at] ^ ((17 * at + 61 * (LANES * w + lane) + 5) as u8);
                at += 1;
            }
            lane += 1;
        }
        w += 1;
    }
    let mut planes = slice(&blocks, &mut trace);
    encrypt_pass(keys, &mut planes, &mut trace);
    let sealed = unslice(&planes, &mut trace);
    let mut planes = slice(&blocks, &mut trace);
    decrypt_pass(keys, &mut planes, &mut trace);
    let opened = unslice(&planes, &mut trace);
    w = 0;
    while w < W {
        let mut lane = 0;
        while lane < LANES {
            if !same(
                &sealed[w][lane],
                &encrypt_alone(keys, blocks[w][lane], &mut trace),
            ) || !same(
                &opened[w][lane],
                &decrypt_alone(keys, blocks[w][lane], &mut trace),
            ) {
                return false;
            }
            lane += 1;
        }
        w += 1;
    }
    true
}

/// Whether `key` takes `plain` to `cipher` and `cipher` back to `plain`
/// alone in lane 0, and in every lane and different blocks abreast in a pass
/// of one word and in a pass of two.
const fn cipher_vector(key: &[u8], plain: [u8; 16], cipher: [u8; 16]) -> bool {
    let mut trace = Trace::new();
    let Some(keys) = expand(key, &mut trace) else {
        return false;
    };
    same(&encrypt_alone(&keys, plain, &mut trace), &cipher)
        && same(&decrypt_alone(&keys, cipher, &mut trace), &plain)
        && abreast::<1>(&keys, plain, cipher)
        && abreast::<BATCH_WORDS>(&keys, plain, cipher)
}

/// The S-box circuit and its inverse held to FIPS 197's table and its
/// inverse at all 256 inputs, in passes of one word and of two, or the crate
/// does not compile.
const _: () = {
    assert!(
        circuit_is_the_table::<1>(),
        "the S-box circuit is FIPS 197's table"
    );
    assert!(
        circuit_is_the_table::<BATCH_WORDS>(),
        "the S-box circuit is FIPS 197's table two words abreast"
    );
    assert!(
        inverse_is_the_tables_inverse::<1>(),
        "the inverse circuit is the table's inverse"
    );
    assert!(
        inverse_is_the_tables_inverse::<BATCH_WORDS>(),
        "the inverse circuit is the table's inverse two words abreast"
    );
};

/// The slicing of a pass of one word held to the layout, or the crate does
/// not compile.
const _: () = assert!(
    slicing_is_the_layout::<1>(),
    "slicing puts each byte at its row, column and lane"
);

/// The slicing of a pass of two words held to the layout, or the crate does
/// not compile.
const _: () = assert!(
    slicing_is_the_layout::<BATCH_WORDS>(),
    "slicing puts each byte at its row, column, lane and word"
);

/// The cipher and the inverse cipher held to FIPS 197 appendix B's AES-128
/// block both ways, alone and abreast at both widths, or the crate does not
/// compile.
const _: () = assert!(
    cipher_vector(
        &[
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ],
        [
            0x32, 0x43, 0xf6, 0xa8, 0x88, 0x5a, 0x30, 0x8d, 0x31, 0x31, 0x98, 0xa2, 0xe0, 0x37,
            0x07, 0x34,
        ],
        [
            0x39, 0x25, 0x84, 0x1d, 0x02, 0xdc, 0x09, 0xfb, 0xdc, 0x11, 0x85, 0x97, 0x19, 0x6a,
            0x0b, 0x32,
        ],
    ),
    "FIPS 197 appendix B"
);

/// The cipher and the inverse cipher held to FIPS 197 C.1's AES-128 block,
/// as appendix B's.
const _: () = assert!(
    cipher_vector(
        &[
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ],
        [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ],
        [
            0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
            0xc5, 0x5a,
        ],
    ),
    "FIPS 197 C.1"
);

/// The cipher and the inverse cipher held to FIPS 197 C.3's AES-256 block,
/// as appendix B's.
const _: () = assert!(
    cipher_vector(
        &[
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ],
        [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ],
        [
            0x8e, 0xa2, 0xb7, 0xca, 0x51, 0x67, 0x45, 0xbf, 0xea, 0xfc, 0x49, 0x90, 0x4b, 0x49,
            0x60, 0x89,
        ],
    ),
    "FIPS 197 C.3"
);

/// Key expansions made, counted for `the_aesv3_key_is_expanded_once_per_file`:
/// compiled only under `cfg(test)` and kept per thread, so tests running side
/// by side cannot add into each other's.
#[cfg(test)]
pub(crate) mod expansions {
    use std::cell::Cell;

    std::thread_local! {
        static MADE: Cell<u64> = const { Cell::new(0) };
    }

    /// One expansion.
    pub(crate) fn counted() {
        MADE.with(|made| made.set(made.get() + 1));
    }

    /// The expansions counted on this thread since the last call, which
    /// starts the count again from zero.
    pub(crate) fn take() -> u64 {
        MADE.with(|made| made.replace(0))
    }
}

/// Passes of the core run, and the words they ran, counted for
/// `a_decrypted_block_costs_a_lane_not_a_pass` and
/// `a_chain_sealed_beside_others_costs_a_lane_not_a_pass`: compiled only
/// under `cfg(test)` and kept per thread, as [`expansions`] is. A pass counts
/// one however many of its lanes it filled, and its width in words.
#[cfg(test)]
pub(crate) mod passes {
    use std::cell::Cell;

    std::thread_local! {
        static RUN: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    }

    /// One pass of `words` words.
    pub(crate) fn counted(words: usize) {
        RUN.with(|run| {
            let (passes, ran) = run.get();
            run.set((passes + 1, ran + words as u64));
        });
    }

    /// The passes counted on this thread since the last call to this or to
    /// [`take_words`], which starts both counts again from zero.
    pub(crate) fn take() -> u64 {
        take_words().0
    }

    /// The passes counted on this thread since the last call to this or to
    /// [`take`], and the words they ran, which starts both again from zero.
    pub(crate) fn take_words() -> (u64, u64) {
        RUN.with(|run| run.replace((0, 0)))
    }
}

/// The most chains [`Aes::cbc_encrypt_side_by_side`] carries in one pass of
/// the core: the lanes of the batched pass.
pub const SIDE_BY_SIDE: usize = BATCH;

/// Chains sealed side by side ([`Aes::cbc_encrypt_side_by_side`]): each
/// one's bytes, in the order the chains were given, and the passes of the
/// core the seal ran.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SideBySide {
    /// Each chain's ciphertext with its IV prefixed, byte for byte what
    /// [`Aes::cbc_encrypt_with_iv_prefix`] writes for that IV and plaintext.
    pub chains: Vec<Vec<u8>>,
    /// The passes of the core the seal took, each one whatever its width and
    /// however many lanes it filled.
    pub passes: u64,
}

/// A chain a lane carries while chains are sealed side by side: the
/// plaintext it has still to seal, the ciphertext block before its next one,
/// and its output.
struct Carried<'a> {
    rest: &'a [u8],
    before: [u8; 16],
    out: &'a mut Vec<u8>,
}

impl Carried<'_> {
    /// The chain's next block XORed with the ciphertext block before it, and
    /// whether it is the chain's last: a whole block of what is left, or —
    /// once fewer than sixteen bytes are — what is left and its PKCS#7
    /// padding (7.6.2), the block [`Aes::cbc_encrypt_with_iv_prefix`] ends a
    /// chain with. Which it is turns on the length left alone.
    fn next_block(&mut self) -> ([u8; 16], bool) {
        let (mut block, last) = match self.rest.split_first_chunk::<16>() {
            Some((head, tail)) => {
                self.rest = tail;
                (*head, false)
            }
            None => {
                let pad = 16 - self.rest.len();
                let mut padded = [pad as u8; 16];
                if let Some(head) = padded.get_mut(..self.rest.len()) {
                    head.copy_from_slice(self.rest);
                }
                self.rest = &[];
                (padded, true)
            }
        };
        for (byte, b) in block.iter_mut().zip(self.before) {
            *byte ^= b;
        }
        (block, last)
    }
}

/// An expanded AES key. 128- and 256-bit only: PDF has no use for AES-192.
///
/// Its round keys are written to zero when it is dropped, and its `Debug`
/// prints none of them.
#[derive(Clone)]
pub struct Aes {
    keys: Expanded,
}

impl core::fmt::Debug for Aes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Aes([redacted])")
    }
}

impl Drop for Aes {
    /// The round keys wiped, the stores passed through `black_box` and a
    /// fence so that they are not dropped as writes to memory about to be
    /// freed; the crate forbids `unsafe`, so no volatile write.
    fn drop(&mut self) {
        let Expanded { enc, dec, .. } = &mut self.keys;
        for planes in enc.iter_mut().chain(dec.iter_mut()) {
            *planes = [0; 8];
        }
        let _ = core::hint::black_box(&mut self.keys);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

impl Aes {
    /// Expands a 16- or 32-byte key. Any other length is rejected: unlike
    /// document data, a wrong-sized key is a caller error the security handler
    /// must decide about.
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        let keys = expand(key, &mut Trace::new())?;
        #[cfg(test)]
        expansions::counted();
        Some(Self { keys })
    }

    /// One block through the cipher, alone in lane 0 of a one-word pass: the
    /// single-chain path.
    fn encrypt_block(&self, block: &mut [u8; 16]) {
        #[cfg(test)]
        passes::counted(1);
        *block = encrypt_alone(&self.keys, *block, &mut Trace::new());
    }

    /// One block through the inverse cipher, alone in lane 0 of a one-word
    /// pass: decryption block by block, which the batched path is held to.
    #[cfg(test)]
    fn decrypt_block(&self, block: &mut [u8; 16]) {
        passes::counted(1);
        *block = decrypt_alone(&self.keys, *block, &mut Trace::new());
    }

    /// The eight lanes of a pass of two words through the inverse cipher at
    /// once, a block a lane.
    fn decrypt_lanes(&self, blocks: &Blocks<BATCH_WORDS>) -> Blocks<BATCH_WORDS> {
        #[cfg(test)]
        passes::counted(BATCH_WORDS);
        let mut trace = Trace::new();
        let mut planes = slice(blocks, &mut trace);
        decrypt_pass(&self.keys, &mut planes, &mut trace);
        unslice(&planes, &mut trace)
    }

    /// CBC decryption of the whole blocks of `whole` under `iv`, appended to
    /// `out`. Each plaintext block is `D(C_n) ⊕ C_(n−1)`, and every `C_n` is
    /// in the input, so the `D(C_n)` are independent: a pass of two words
    /// decrypts eight blocks, and a tail of fewer takes the same pass with its
    /// other lanes empty. A partial block at the end is left out. The lengths
    /// decide the passes, and nothing a pass gives decides anything.
    fn decrypt_chain(&self, iv: &[u8; 16], whole: &[u8], out: &mut Vec<u8>) {
        let mut before = *iv;
        let mut passes = whole.chunks_exact(16 * BATCH);
        for pass in &mut passes {
            self.open_pass(pass, &mut before, out);
        }
        let tail = passes.remainder();
        if tail.len() >= 16 {
            self.open_pass(tail, &mut before, out);
        }
    }

    /// The whole blocks of `pass`, eight at most, through one pass of the
    /// inverse cipher, a block a lane, each XORed with the ciphertext block
    /// before it — `before` for the first, which is left holding the last —
    /// and appended to `out`.
    fn open_pass(&self, pass: &[u8], before: &mut [u8; 16], out: &mut Vec<u8>) {
        let mut lanes = [[[0u8; 16]; LANES]; BATCH_WORDS];
        for (lane, block) in lanes.iter_mut().flatten().zip(pass.chunks_exact(16)) {
            lane.copy_from_slice(block);
        }
        let opened = self.decrypt_lanes(&lanes);
        for (plain, block) in opened.iter().flatten().zip(pass.chunks_exact(16)) {
            let mut plain = *plain;
            for (byte, b) in plain.iter_mut().zip(before.iter()) {
                *byte ^= b;
            }
            out.extend_from_slice(&plain);
            before.copy_from_slice(block);
        }
    }

    /// The first `LANES·W` of `blocks` through the cipher at once in a pass of
    /// `W` words, a block a lane, and back in their places; a block the pass
    /// does not carry comes back zero.
    fn encrypt_lanes<const W: usize>(&self, blocks: &[[u8; 16]; BATCH]) -> [[u8; 16]; BATCH] {
        #[cfg(test)]
        passes::counted(W);
        let mut lanes = [[[0u8; 16]; LANES]; W];
        for (lane, block) in lanes.iter_mut().flatten().zip(blocks) {
            *lane = *block;
        }
        let mut trace = Trace::new();
        let mut planes = slice(&lanes, &mut trace);
        encrypt_pass(&self.keys, &mut planes, &mut trace);
        let sealed = unslice(&planes, &mut trace);
        let mut out = [[0u8; 16]; BATCH];
        for (slot, block) in out.iter_mut().zip(sealed.iter().flatten()) {
            *slot = *block;
        }
        out
    }

    /// Several CBC chains with PKCS#7 padding, each with its IV prefixed,
    /// sealed side by side: each chain's bytes are what
    /// [`Aes::cbc_encrypt_with_iv_prefix`] writes for its IV and plaintext,
    /// in the order the chains were given, and the passes of the core the
    /// seal ran come back beside them.
    ///
    /// A chain is sequential — each block enters the cipher XORed with the
    /// ciphertext of the block before — so chains, not blocks, are the lanes:
    /// a pass takes the next block of each chain it carries, at most
    /// [`SIDE_BY_SIDE`] of them, and a chain that ends gives its lane to the
    /// next chain waiting. A pass is one word while four chains or fewer are
    /// carried and two while more are, so a lone chain is the single-chain
    /// path's pass of one word a block. What a pass carries is decided by the
    /// chains' lengths, which are public, and nothing a pass gives decides
    /// anything.
    #[must_use]
    pub fn cbc_encrypt_side_by_side(&self, chains: &[([u8; 16], &[u8])]) -> SideBySide {
        let mut sealed: Vec<Vec<u8>> = chains
            .iter()
            .map(|(iv, plain)| {
                let mut out = Vec::with_capacity(32 + plain.len() / 16 * 16);
                out.extend_from_slice(iv);
                out
            })
            .collect();
        let mut waiting = chains
            .iter()
            .zip(sealed.iter_mut())
            .map(|((iv, plain), out)| Carried {
                rest: plain,
                before: *iv,
                out,
            });
        let mut lanes: [Option<Carried<'_>>; BATCH] = core::array::from_fn(|_| None);
        let mut passes = 0u64;
        loop {
            for lane in lanes.iter_mut().filter(|lane| lane.is_none()) {
                *lane = waiting.next();
            }
            let carried = lanes.iter().filter(|lane| lane.is_some()).count();
            if carried == 0 {
                break;
            }
            let mut blocks = [[0u8; 16]; BATCH];
            let mut ends = [false; BATCH];
            for ((block, end), chain) in blocks
                .iter_mut()
                .zip(ends.iter_mut())
                .zip(lanes.iter_mut().flatten())
            {
                (*block, *end) = chain.next_block();
            }
            let out = if carried <= LANES {
                self.encrypt_lanes::<1>(&blocks)
            } else {
                self.encrypt_lanes::<BATCH_WORDS>(&blocks)
            };
            passes += 1;
            for ((block, end), lane) in out
                .iter()
                .zip(ends)
                .zip(lanes.iter_mut().filter(|lane| lane.is_some()))
            {
                if let Some(chain) = lane.as_mut() {
                    chain.out.extend_from_slice(block);
                    chain.before = *block;
                }
                if end {
                    *lane = None;
                }
            }
        }
        SideBySide {
            chains: sealed,
            passes,
        }
    }

    /// CBC with PKCS#7 padding, the IV prefixed — the layout
    /// [`Aes::cbc_decrypt_with_iv_prefix`] expects. The whole blocks are read
    /// where they lie, and only the last, which takes the padding, is built.
    #[must_use]
    pub fn cbc_encrypt_with_iv_prefix(&self, iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + plain.len() + 16);
        out.extend_from_slice(iv);
        let mut prev = *iv;
        let mut chunks = plain.chunks_exact(16);
        for chunk in &mut chunks {
            for (byte, p) in prev.iter_mut().zip(chunk) {
                *byte ^= p;
            }
            self.encrypt_block(&mut prev);
            out.extend_from_slice(&prev);
        }
        let rest = chunks.remainder();
        let pad = 16 - rest.len();
        let mut last = [pad as u8; 16];
        if let Some(head) = last.get_mut(..rest.len()) {
            head.copy_from_slice(rest);
        }
        for (byte, p) in prev.iter_mut().zip(last) {
            *byte ^= p;
        }
        self.encrypt_block(&mut prev);
        out.extend_from_slice(&prev);
        out
    }

    /// Decrypts CBC ciphertext whose first 16 bytes are the IV, stripping
    /// PKCS#7 padding (7.6.2): eight blocks a pass, into output reserved
    /// once.
    ///
    /// Never fails: damage returns the plaintext recovered so far with a
    /// note, per the leniency policy every reading path in this engine
    /// follows. The notes are decided by the lengths before any block is
    /// decrypted, and by the padding of the last block, whichever pass gave
    /// it.
    #[must_use]
    pub fn cbc_decrypt_with_iv_prefix(&self, data: &[u8]) -> (Vec<u8>, Vec<AesNote>) {
        let mut notes = Vec::new();
        let Some((iv, body)) = data.split_at_checked(16) else {
            return (Vec::new(), vec![AesNote::TooShort]);
        };
        if body.len() < 16 {
            return (Vec::new(), vec![AesNote::TooShort]);
        }
        if body.len() % 16 != 0 {
            notes.push(AesNote::RaggedTail);
        }

        let mut before = [0u8; 16];
        before.copy_from_slice(iv);
        let whole = whole_blocks(body);
        let mut out = Vec::with_capacity(whole.len());
        self.decrypt_chain(&before, whole, &mut out);

        match padding(&last_block(&out)) {
            Some(pad) => out.truncate(out.len().saturating_sub(pad)),
            None => notes.push(AesNote::BadPadding),
        }

        (out, notes)
    }

    /// CBC encryption with an explicit IV and no padding: Algorithm 2.B's
    /// inner loop (7.6.4.3.4), where the input is always a whole number of
    /// blocks; a ragged tail is left out.
    #[must_use]
    pub fn cbc_encrypt_no_padding(&self, iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let mut prev = *iv;
        for chunk in data.chunks_exact(16) {
            for (byte, p) in prev.iter_mut().zip(chunk) {
                *byte ^= p;
            }
            self.encrypt_block(&mut prev);
            out.extend_from_slice(&prev);
        }
        out
    }

    /// CBC decryption with an explicit IV and no padding: how revision 6
    /// unwraps `/UE` and `/OE` (7.6.4.3.3); eight blocks a pass, into output
    /// reserved once, and a ragged tail left out.
    #[must_use]
    pub fn cbc_decrypt_no_padding(&self, iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
        let whole = whole_blocks(data);
        let mut out = Vec::with_capacity(whole.len());
        self.decrypt_chain(iv, whole, &mut out);
        out
    }
}

/// `data` up to the end of its last whole block: a ragged tail left out.
fn whole_blocks(data: &[u8]) -> &[u8] {
    data.get(..data.len() - data.len() % 16).unwrap_or_default()
}

/// The last block of a decryption's output, which holds its padding; the
/// output is whole blocks and at least one, so there is one.
fn last_block(out: &[u8]) -> [u8; 16] {
    let mut last = [0u8; 16];
    if let Some(tail) = out.len().checked_sub(16).and_then(|at| out.get(at..)) {
        last.copy_from_slice(tail);
    }
    last
}

/// The PKCS#7 padding length a last block states, where it is well formed:
/// its last byte `n` in 1..=16 and the `n` bytes before the block's end all
/// `n`; `None` otherwise.
///
/// **Decided by masks over the whole block**, so that the time the check
/// takes says nothing of the plaintext: every byte is read whatever `n` is, a
/// byte counts against the padding through a mask that is all ones exactly
/// where it lies within `n` of the end, and the range of `n` is a mask too.
/// The answer is the one the check a byte at a time gives, and is the one
/// thing it tells: whether `AesNote::BadPadding` is reported, and how much
/// is stripped, which the plaintext's length shows anyway.
fn padding(last: &[u8; 16]) -> Option<usize> {
    let pad = last[15];
    // Zero exactly when `pad` is in 1..=16.
    let mut wrong = pad.wrapping_sub(1) >> 4;
    for (at, &byte) in last.iter().enumerate() {
        let from_end = 16 - at as u8;
        // All ones where `from_end <= pad`: `pad - from_end` borrows otherwise.
        let within = !((u16::from(pad).wrapping_sub(u16::from(from_end)) >> 8) as u8);
        wrong |= within & (byte ^ pad);
    }
    if core::hint::black_box(wrong) == 0 {
        Some(usize::from(pad))
    } else {
        None
    }
}

/// What a decryption tolerated. PDF strings in the wild are truncated,
/// mis-padded, and occasionally not encrypted at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AesNote {
    /// Fewer than 16 bytes, or no whole block after the IV: nothing to do.
    TooShort,
    /// Trailing bytes that are not a whole block; they are ignored.
    RaggedTail,
    /// The PKCS#7 padding was not well formed, so nothing was stripped.
    BadPadding,
}

/// Decrypts CBC ciphertext whose first 16 bytes are the IV, stripping PKCS#7
/// padding (7.6.2): [`Aes::cbc_decrypt_with_iv_prefix`] under `key`, expanded
/// for this call.
///
/// Never fails: damage returns the plaintext recovered so far with a note, per
/// the leniency policy every reading path in this engine follows; a key AES
/// does not take returns nothing and [`AesNote::TooShort`].
#[must_use]
pub fn cbc_decrypt_with_iv_prefix(key: &[u8], data: &[u8]) -> (Vec<u8>, Vec<AesNote>) {
    let Some(aes) = Aes::new(key) else {
        return (Vec::new(), vec![AesNote::TooShort]);
    };
    aes.cbc_decrypt_with_iv_prefix(data)
}

/// Encrypts with CBC and PKCS#7 padding, prefixing the given IV — the layout
/// [`cbc_decrypt_with_iv_prefix`] expects: [`Aes::cbc_encrypt_with_iv_prefix`]
/// under `key`, expanded for this call.
#[must_use]
pub fn cbc_encrypt_with_iv_prefix(key: &[u8], iv: &[u8; 16], plain: &[u8]) -> Option<Vec<u8>> {
    Some(Aes::new(key)?.cbc_encrypt_with_iv_prefix(iv, plain))
}

/// CBC encryption with an explicit IV and no padding: Algorithm 2.B's inner
/// loop (7.6.4.3.4), where the input is always a whole number of blocks.
#[must_use]
pub fn cbc_encrypt_no_padding(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    Some(Aes::new(key)?.cbc_encrypt_no_padding(iv, data))
}

/// CBC decryption with an explicit IV and no padding: how revision 6 unwraps
/// `/UE` and `/OE` (7.6.4.3.3).
#[must_use]
pub fn cbc_decrypt_no_padding(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    Some(Aes::new(key)?.cbc_decrypt_no_padding(iv, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| {
                let text = std::str::from_utf8(pair).ok()?;
                u8::from_str_radix(text, 16).ok()
            })
            .collect()
    }

    /// FIPS 197 appendix C.1: AES-128 known-answer block.
    #[test]
    fn fips197_aes128_block() {
        let key = unhex("000102030405060708090a0b0c0d0e0f");
        let plain = unhex("00112233445566778899aabbccddeeff");
        let aes = Aes::new(&key).expect("a 16-byte key expands");

        let mut block = [0u8; 16];
        block.copy_from_slice(&plain);
        aes.encrypt_block(&mut block);
        assert_eq!(hex(&block), "69c4e0d86a7b0430d8cdb78070b4c55a");

        aes.decrypt_block(&mut block);
        assert_eq!(hex(&block), hex(&plain));
    }

    /// FIPS 197 appendix C.3: AES-256 known-answer block.
    #[test]
    fn fips197_aes256_block() {
        let key = unhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let plain = unhex("00112233445566778899aabbccddeeff");
        let aes = Aes::new(&key).expect("a 32-byte key expands");

        let mut block = [0u8; 16];
        block.copy_from_slice(&plain);
        aes.encrypt_block(&mut block);
        assert_eq!(hex(&block), "8ea2b7ca516745bfeafc49904b496089");

        aes.decrypt_block(&mut block);
        assert_eq!(hex(&block), hex(&plain));
    }

    /// NIST SP 800-38A F.2.1/F.2.2: AES-128-CBC, first two blocks.
    #[test]
    fn sp800_38a_cbc_aes128() {
        let key = unhex("2b7e151628aed2a6abf7158809cf4f3c");
        let iv = unhex("000102030405060708090a0b0c0d0e0f");
        let plain = unhex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");

        let mut iv16 = [0u8; 16];
        iv16.copy_from_slice(&iv);
        let cipher = cbc_encrypt_no_padding(&key, &iv16, &plain).expect("valid key");
        // Both blocks are published answers; the second is what proves the
        // chaining, since a broken CBC still gets the first block right.
        assert_eq!(
            hex(cipher.get(..16).unwrap_or_default()),
            "7649abac8119b246cee98e9b12e9197d"
        );
        assert_eq!(
            hex(cipher.get(16..32).unwrap_or_default()),
            "5086cb9b507219ee95db113a917678b2"
        );

        let back = cbc_decrypt_no_padding(&key, &iv16, &cipher).expect("valid key");
        assert_eq!(hex(&back), hex(&plain));
    }

    #[test]
    fn iv_prefixed_round_trip_strips_padding() {
        let key = [7u8; 32];
        let iv = [3u8; 16];
        for len in [0usize, 1, 15, 16, 17, 64] {
            let plain = vec![0xabu8; len];
            let cipher = cbc_encrypt_with_iv_prefix(&key, &iv, &plain).expect("valid key");
            let (back, notes) = cbc_decrypt_with_iv_prefix(&key, &cipher);
            assert_eq!(back, plain, "length {len}");
            assert!(notes.is_empty(), "length {len} produced {notes:?}");
        }
    }

    /// A field of an AESAVS file, hex to bytes, refusing anything else: a
    /// vector that does not parse is a failure, never a skipped one.
    fn field(value: &str) -> Vec<u8> {
        let digits = value.trim();
        assert!(
            digits.len() % 2 == 0,
            "an even count of hex digits: {digits}"
        );
        (0..digits.len() / 2)
            .map(|at| {
                let pair = digits.get(2 * at..2 * at + 2).expect("a pair of digits");
                u8::from_str_radix(pair, 16).expect("a hex byte")
            })
            .collect()
    }

    /// One record of an AESAVS response file: its section and its fields.
    struct Record {
        encrypt: bool,
        count: usize,
        key: Vec<u8>,
        iv: [u8; 16],
        plaintext: Vec<u8>,
        ciphertext: Vec<u8>,
    }

    /// Every record of an AESAVS response file, in the file's order. Lines of
    /// the header and the file's own comments start with `#` and are passed
    /// over; a record is its `COUNT` and the fields after it.
    fn records(source: &str) -> Vec<Record> {
        let mut out: Vec<Record> = Vec::new();
        let mut encrypt = None;
        for line in source.lines().map(str::trim) {
            if line.starts_with('#') {
                continue;
            }
            match line {
                "[ENCRYPT]" => encrypt = Some(true),
                "[DECRYPT]" => encrypt = Some(false),
                _ => {}
            }
            let Some((name, value)) = line.split_once(" = ") else {
                continue;
            };
            if name == "COUNT" {
                out.push(Record {
                    encrypt: encrypt.expect("every record is in a section"),
                    count: value.trim().parse().expect("a count"),
                    key: Vec::new(),
                    iv: [0; 16],
                    plaintext: Vec::new(),
                    ciphertext: Vec::new(),
                });
                continue;
            }
            let record = out.last_mut().expect("every field follows its COUNT");
            match name {
                "KEY" => record.key = field(value),
                "IV" => record.iv = field(value).try_into().expect("a 16-byte IV"),
                "PLAINTEXT" => record.plaintext = field(value),
                "CIPHERTEXT" => record.ciphertext = field(value),
                other => panic!("an AESAVS field this test does not know: {other}"),
            }
        }
        out
    }

    /// The records a file holds in each section, counted from its lines
    /// alone, so that a parser that matched nothing cannot agree with it.
    fn held(source: &str) -> (usize, usize) {
        let (mut encrypt, mut decrypt, mut section) = (0, 0, None);
        for line in source.lines().map(str::trim) {
            match line {
                "[ENCRYPT]" => section = Some(true),
                "[DECRYPT]" => section = Some(false),
                _ if line.starts_with("COUNT = ") => match section {
                    Some(true) => encrypt += 1,
                    Some(false) => decrypt += 1,
                    None => panic!("a COUNT outside a section"),
                },
                _ => {}
            }
        }
        (encrypt, decrypt)
    }

    /// A known-answer or multi-block message file: each record's plaintext
    /// encrypts to its ciphertext and its ciphertext decrypts to its
    /// plaintext, CBC with the record's IV and no padding, whichever section
    /// it is in; an encryption record's ciphertext is also what the
    /// IV-prefixed encryption writes after the IV, before the block of
    /// padding a whole number of blocks gains. Returns the records each
    /// section ran.
    fn run_messages(name: &str, source: &str) -> (usize, usize) {
        let (mut encrypt, mut decrypt) = (0, 0);
        for record in records(source) {
            let label = format!(
                "{name}, {} record {}",
                if record.encrypt { "encrypt" } else { "decrypt" },
                record.count
            );
            assert_eq!(record.plaintext.len(), record.ciphertext.len(), "{label}");
            assert!(
                !record.plaintext.is_empty() && record.plaintext.len() % 16 == 0,
                "{label}: whole blocks"
            );
            let sealed = cbc_encrypt_no_padding(&record.key, &record.iv, &record.plaintext)
                .expect("the record's key expands");
            assert_eq!(hex(&sealed), hex(&record.ciphertext), "{label}: encrypt");
            let opened = cbc_decrypt_no_padding(&record.key, &record.iv, &record.ciphertext)
                .expect("the record's key expands");
            assert_eq!(hex(&opened), hex(&record.plaintext), "{label}: decrypt");
            if record.encrypt {
                let prefixed =
                    cbc_encrypt_with_iv_prefix(&record.key, &record.iv, &record.plaintext)
                        .expect("the record's key expands");
                let length = record.ciphertext.len();
                assert_eq!(prefixed.len(), 16 + length + 16, "{label}: prefixed length");
                assert_eq!(prefixed.get(..16), Some(&record.iv[..]), "{label}: the IV");
                assert_eq!(
                    prefixed.get(16..16 + length),
                    Some(&record.ciphertext[..]),
                    "{label}: prefixed"
                );
                encrypt += 1;
            } else {
                decrypt += 1;
            }
        }
        (encrypt, decrypt)
    }

    /// One block through an expanded key, forward.
    fn forward(aes: &Aes, block: [u8; 16]) -> [u8; 16] {
        let mut block = block;
        aes.encrypt_block(&mut block);
        block
    }

    /// One block through an expanded key, back.
    fn backward(aes: &Aes, block: [u8; 16]) -> [u8; 16] {
        let mut block = block;
        aes.decrypt_block(&mut block);
        block
    }

    fn xor16(a: [u8; 16], b: [u8; 16]) -> [u8; 16] {
        let mut out = a;
        for (byte, other) in out.iter_mut().zip(b) {
            *byte ^= other;
        }
        out
    }

    /// AESAVS 6.4.2's Monte Carlo test for CBC, both sections of a file.
    ///
    /// From each section's first record, a text is a thousand blocks under
    /// one key in one CBC chain from the text's IV, the first block the
    /// record's input and each later one the IV (the second) or the output
    /// two blocks back (every other); the record's output is the thousandth.
    /// The next record's key is this one's XORed with the last output (a
    /// 128-bit key) or the last two (a 256-bit key), its IV the last output
    /// and its input the one before. Decryption is the same with the roles
    /// of plaintext and ciphertext exchanged (6.4). Every record's key, IV
    /// and input are held to the chain's and its output to the chain's, so
    /// one wrong block fails the record it lands in. Returns the records
    /// each section ran.
    fn run_monte_carlo(name: &str, source: &str) -> (usize, usize) {
        let all = records(source);
        let mut ran = (0, 0);
        for encrypt in [true, false] {
            let section: Vec<&Record> = all.iter().filter(|r| r.encrypt == encrypt).collect();
            let first = section.first().expect("a section of checkpoints");
            let input_of = |r: &Record| -> [u8; 16] {
                let bytes = if encrypt { &r.plaintext } else { &r.ciphertext };
                bytes.as_slice().try_into().expect("one block")
            };
            let output_of = |r: &Record| -> [u8; 16] {
                let bytes = if encrypt { &r.ciphertext } else { &r.plaintext };
                bytes.as_slice().try_into().expect("one block")
            };
            let (mut key, mut iv, mut input) = (first.key.clone(), first.iv, input_of(first));
            for (at, record) in section.iter().enumerate() {
                let label = format!(
                    "{name}, {} checkpoint {at}",
                    if encrypt { "encrypt" } else { "decrypt" }
                );
                assert_eq!(record.count, at, "{label}: in order");
                assert_eq!(hex(&record.key), hex(&key), "{label}: key");
                assert_eq!(hex(&record.iv), hex(&iv), "{label}: IV");
                assert_eq!(hex(&input_of(record)), hex(&input), "{label}: input");
                let aes = Aes::new(&key).expect("the checkpoint's key expands");
                let (mut chain, mut block) = (iv, input);
                let (mut before, mut last) = ([0u8; 16], [0u8; 16]);
                for step in 0..1000 {
                    let out = if encrypt {
                        forward(&aes, xor16(block, chain))
                    } else {
                        xor16(backward(&aes, block), chain)
                    };
                    let next = if step == 0 { iv } else { last };
                    before = last;
                    last = out;
                    chain = if encrypt { out } else { block };
                    block = next;
                }
                assert_eq!(hex(&last), hex(&output_of(record)), "{label}: output");
                match key.len() {
                    16 => {
                        for (byte, out) in key.iter_mut().zip(last) {
                            *byte ^= out;
                        }
                    }
                    32 => {
                        for (byte, out) in key.iter_mut().zip(before.iter().chain(&last)) {
                            *byte ^= out;
                        }
                    }
                    other => panic!("{label}: a {other}-byte key"),
                }
                iv = last;
                input = before;
                if encrypt {
                    ran.0 += 1;
                } else {
                    ran.1 += 1;
                }
            }
        }
        ran
    }

    /// **NIST CAVP's AESAVS files for CBC under 128- and 256-bit keys pass**
    /// (P-13.2): the four known-answer tests (GFSbox, KeySbox, VarKey,
    /// VarTxt; AESAVS 6.2), the multi-block messages of one to ten blocks
    /// (6.3) and the Monte Carlo test's hundred checkpoints of a thousand
    /// chained blocks (6.4.2), each file's encrypt and decrypt sections both.
    /// Every record runs, the count each section ran is the count its lines
    /// hold, and both are the files' published inventory. Read through
    /// `include_str!`, so a missing file fails the build rather than skips.
    #[test]
    fn aesavs_known_answer_and_cbc_vectors_pass() {
        let messages: [(&str, &str, usize); 10] = [
            (
                "CBCGFSbox128",
                include_str!("../tests/data/cavp/aes_cbc_gfsbox128.rsp"),
                7,
            ),
            (
                "CBCGFSbox256",
                include_str!("../tests/data/cavp/aes_cbc_gfsbox256.rsp"),
                5,
            ),
            (
                "CBCKeySbox128",
                include_str!("../tests/data/cavp/aes_cbc_keysbox128.rsp"),
                21,
            ),
            (
                "CBCKeySbox256",
                include_str!("../tests/data/cavp/aes_cbc_keysbox256.rsp"),
                16,
            ),
            (
                "CBCVarKey128",
                include_str!("../tests/data/cavp/aes_cbc_varkey128.rsp"),
                128,
            ),
            (
                "CBCVarKey256",
                include_str!("../tests/data/cavp/aes_cbc_varkey256.rsp"),
                256,
            ),
            (
                "CBCVarTxt128",
                include_str!("../tests/data/cavp/aes_cbc_vartxt128.rsp"),
                128,
            ),
            (
                "CBCVarTxt256",
                include_str!("../tests/data/cavp/aes_cbc_vartxt256.rsp"),
                128,
            ),
            (
                "CBCMMT128",
                include_str!("../tests/data/cavp/aes_cbc_mmt128.rsp"),
                10,
            ),
            (
                "CBCMMT256",
                include_str!("../tests/data/cavp/aes_cbc_mmt256.rsp"),
                10,
            ),
        ];
        let mut total = 0;
        for (name, source, each) in messages {
            let ran = run_messages(name, source);
            assert_eq!(ran, held(source), "{name}: every record its lines hold");
            assert_eq!(ran, (each, each), "{name}: the published inventory");
            total += ran.0 + ran.1;
        }
        assert_eq!(
            total,
            2 * (7 + 5 + 21 + 16 + 128 + 256 + 128 + 128) + 4 * 10
        );

        let monte_carlo = [
            (
                "CBCMCT128",
                include_str!("../tests/data/cavp/aes_cbc_mct128.rsp"),
            ),
            (
                "CBCMCT256",
                include_str!("../tests/data/cavp/aes_cbc_mct256.rsp"),
            ),
        ];
        for (name, source) in monte_carlo {
            let ran = run_monte_carlo(name, source);
            assert_eq!(ran, held(source), "{name}: every checkpoint its lines hold");
            assert_eq!(ran, (100, 100), "{name}: the published inventory");
        }
    }

    #[test]
    fn damage_returns_notes_not_panics() {
        let key = [1u8; 16];
        for data in [
            vec![],
            vec![0u8; 8],
            vec![0u8; 16],
            vec![0u8; 20],
            vec![0xffu8; 48],
        ] {
            let _ = cbc_decrypt_with_iv_prefix(&key, &data);
        }
        assert!(Aes::new(&[0u8; 24]).is_none(), "AES-192 is not offered");
    }

    /// A stream of bytes for the trace tests: xorshift, deterministic.
    struct Bytes(u64);

    impl Bytes {
        fn next(&mut self) -> u8 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 24) as u8
        }

        fn fill(&mut self, bytes: &mut [u8]) {
            for byte in bytes {
                *byte = self.next();
            }
        }
    }

    /// The traces of 300 trials at a pass of `W` words under random keys of
    /// `length` bytes, random blocks in one lane — as a lone chain fills it —
    /// and in every lane by turns: the key expansion's one trace and the
    /// cipher's one trace, each the same in every trial, which it returns.
    fn forward_traces<const W: usize>(bytes: &mut Bytes, length: usize) -> (Trace, Trace) {
        let mut first: Option<(Trace, Trace)> = None;
        for trial in 0..300 {
            let mut key = vec![0u8; length];
            bytes.fill(&mut key);
            let mut blocks = [[[0u8; 16]; LANES]; W];
            let filled = if trial % 2 == 0 { 1 } else { LANES * W };
            for block in blocks.iter_mut().flatten().take(filled) {
                bytes.fill(block);
            }

            let mut schedule = Trace::new();
            let keys = expand(&key, &mut schedule).expect("the key expands");
            let mut cipher = Trace::new();
            let mut planes = slice(&blocks, &mut cipher);
            encrypt_pass(&keys, &mut planes, &mut cipher);
            let out = unslice(&planes, &mut cipher);
            assert_ne!(out, blocks, "the cipher ran");

            assert!(
                schedule.reads > 0 && schedule.branches > 0,
                "the schedule recorded"
            );
            assert!(
                cipher.reads > 0 && cipher.branches > 0,
                "the cipher recorded"
            );
            match &first {
                Some((first_schedule, first_cipher)) => {
                    assert_eq!(
                        schedule, *first_schedule,
                        "{length}-byte key, {W} words, trial {trial}: the schedule's reads and branches"
                    );
                    assert_eq!(
                        cipher, *first_cipher,
                        "{length}-byte key, {W} words, trial {trial}: the cipher's reads and branches"
                    );
                }
                None => first = Some((schedule, cipher)),
            }
        }
        first.expect("a trial ran")
    }

    /// **The encryption core reads no secret-indexed table and branches
    /// alike** (P-13.4).
    ///
    /// The core records in test builds every table it reads, as the table and
    /// the index, and every condition it tests, as the site and the outcome.
    /// Over random keys of both lengths, and random blocks in every lane —
    /// one lane filled, as a lone chain fills it, and all of them — in a pass
    /// of one word and in a pass of two, the key expansion leaves one trace
    /// for each key length and the cipher one for each length and width, so
    /// no read is indexed by a key or a state byte and no branch turns on
    /// one; the traces are not empty, and they do tell two lengths apart,
    /// whose round counts are public, and two widths, which are public too.
    /// A read made at a key byte's index, through the same trace, comes out
    /// differently for two keys: the trace sees an index that moves.
    #[test]
    fn the_core_reads_no_secret_indexed_table_and_branches_alike() {
        let mut bytes = Bytes(0x7472_6163_6573_6564);
        let mut seen = Vec::new();
        for length in [16usize, 32] {
            let one = forward_traces::<1>(&mut bytes, length);
            let two = forward_traces::<BATCH_WORDS>(&mut bytes, length);
            assert_eq!(
                one.0, two.0,
                "{length}-byte key: one schedule for both widths"
            );
            assert_ne!(
                one.1, two.1,
                "{length}-byte key: the two widths, public, are told apart"
            );
            seen.push((one, two));
        }
        assert_ne!(
            seen[0], seen[1],
            "the two key lengths, public, are told apart"
        );

        let (mut one, mut two) = (Trace::new(), Trace::new());
        one.read(TABLE_ROUND_KEY, usize::from(0x17u8));
        two.read(TABLE_ROUND_KEY, usize::from(0xc4u8));
        assert_ne!(
            one, two,
            "a read at a moving index leaves a different trace"
        );
    }

    /// The inverse cipher's one trace over 300 trials at a pass of `W` words,
    /// under random keys of `length` bytes and random ciphertext in one lane
    /// and in every lane by turns; every block it gives back is the one the
    /// cipher takes back to the ciphertext.
    fn inverse_traces<const W: usize>(bytes: &mut Bytes, length: usize) -> Trace {
        let mut first: Option<Trace> = None;
        for trial in 0..300 {
            let mut key = vec![0u8; length];
            bytes.fill(&mut key);
            let keys = expand(&key, &mut Trace::new()).expect("the key expands");
            let mut blocks = [[[0u8; 16]; LANES]; W];
            let filled = if trial % 2 == 0 { 1 } else { LANES * W };
            for block in blocks.iter_mut().flatten().take(filled) {
                bytes.fill(block);
            }

            let mut inverse = Trace::new();
            let mut planes = slice(&blocks, &mut inverse);
            decrypt_pass(&keys, &mut planes, &mut inverse);
            let opened = unslice(&planes, &mut inverse);
            assert!(
                inverse.reads > 0 && inverse.branches > 0,
                "the inverse recorded"
            );
            match &first {
                Some(first) => assert_eq!(
                    inverse, *first,
                    "{length}-byte key, {W} words, trial {trial}: the inverse cipher's reads and branches"
                ),
                None => first = Some(inverse),
            }

            let mut planes = slice(&opened, &mut Trace::new());
            encrypt_pass(&keys, &mut planes, &mut Trace::new());
            assert_eq!(
                unslice(&planes, &mut Trace::new()),
                blocks,
                "{length}-byte key, {W} words, trial {trial}: the cipher takes back what the inverse gave"
            );
        }
        first.expect("a trial ran")
    }

    /// **The inverse core reads no secret-indexed table and branches alike**
    /// (P-13.5), in the batched pass CBC decryption runs as well (P-13.6).
    ///
    /// The same record as the encryption core's, of the inverse cipher: over
    /// random keys of both lengths and random ciphertext in one lane and in
    /// all of them, in a pass of one word and in the batched pass of two, one
    /// trace for each key length and width, and every block the inverse
    /// cipher gives back is the one the cipher took. InvSubBytes reads no
    /// table — it is the forward circuit between two inverse affine maps —
    /// and InvMixColumns multiplies by no loop, so nothing in the record moves
    /// with a key or a state byte; the record of a different length differs,
    /// and so does that of a different width.
    #[test]
    fn the_inverse_core_reads_no_secret_indexed_table_and_branches_alike() {
        let mut bytes = Bytes(0x696e_7665_7273_6573);
        let mut seen = Vec::new();
        for length in [16usize, 32] {
            let one = inverse_traces::<1>(&mut bytes, length);
            let two = inverse_traces::<BATCH_WORDS>(&mut bytes, length);
            assert_ne!(
                one, two,
                "{length}-byte key: the two widths, public, are told apart"
            );
            seen.push((one, two));
        }
        assert_ne!(
            seen[0], seen[1],
            "the two key lengths, public, are told apart"
        );
    }

    /// The padding check by masks gives the answer the check a byte at a
    /// time gives, for every last byte and every last block a corruption of
    /// a well-formed one can make: a length in 1..=16 whose bytes all agree
    /// strips that many, and anything else — a length of 0 or past 16, or a
    /// byte inside the padding that differs — strips nothing.
    #[test]
    fn the_padding_check_by_masks_is_the_check_byte_by_byte() {
        let by_bytes = |last: &[u8; 16]| -> Option<usize> {
            let pad = usize::from(last[15]);
            if (1..=16).contains(&pad) && last[16 - pad..].iter().all(|&b| usize::from(b) == pad) {
                Some(pad)
            } else {
                None
            }
        };
        let mut checked = 0;
        for pad in 0..=255u8 {
            for fill in [0u8, 0x5a, 0xff] {
                let mut last = [fill; 16];
                let span = usize::from(pad).clamp(1, 16);
                last[16 - span..].fill(pad);
                assert_eq!(padding(&last), by_bytes(&last), "pad {pad}, fill {fill}");
                checked += 1;
                for flip in 0..16 {
                    let mut wrong = last;
                    wrong[flip] ^= 0x01;
                    assert_eq!(
                        padding(&wrong),
                        by_bytes(&wrong),
                        "pad {pad}, byte {flip} flipped"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 256 * 3 * 17);
        assert_eq!(padding(&[16; 16]), Some(16), "a whole block of padding");
        assert_eq!(padding(&[0; 16]), None, "a length of zero");
    }

    /// CBC decryption of `body`'s whole blocks under `iv`, a block a pass of
    /// one word: decryption as it is with no batching.
    fn unpadded_block_by_block(aes: &Aes, iv: [u8; 16], body: &[u8]) -> Vec<u8> {
        let mut before = iv;
        let mut out = Vec::new();
        for chunk in body.chunks_exact(16) {
            let cipher: [u8; 16] = chunk.try_into().expect("a block");
            let mut block = cipher;
            aes.decrypt_block(&mut block);
            out.extend(block.iter().zip(before).map(|(byte, b)| byte ^ b));
            before = cipher;
        }
        out
    }

    /// CBC decryption of IV-prefixed `data` a block a pass, with the notes
    /// 7.6.2's reading gives: `TooShort` where there is no IV or no whole
    /// block after it, `RaggedTail` where bytes trail the last whole block,
    /// and the padding checked a byte at a time, stripped where well formed
    /// and `BadPadding` otherwise. What the batched path is held to.
    fn block_by_block(aes: &Aes, data: &[u8]) -> (Vec<u8>, Vec<AesNote>) {
        if data.len() < 32 {
            return (Vec::new(), vec![AesNote::TooShort]);
        }
        let (iv, body) = data.split_at(16);
        let mut notes = Vec::new();
        if body.len() % 16 != 0 {
            notes.push(AesNote::RaggedTail);
        }
        let mut out = unpadded_block_by_block(aes, iv.try_into().expect("an IV"), body);
        let last = &out[out.len() - 16..];
        let pad = usize::from(last[15]);
        if (1..=16).contains(&pad) && last[16 - pad..].iter().all(|&b| usize::from(b) == pad) {
            out.truncate(out.len() - pad);
        } else {
            notes.push(AesNote::BadPadding);
        }
        (out, notes)
    }

    /// Asserts that the batched decryptions of `data` — IV prefixed, and the
    /// IV taken apart and no padding — give block by block's plaintext and
    /// notes.
    fn holds(aes: &Aes, data: &[u8], label: &str) {
        assert_eq!(
            aes.cbc_decrypt_with_iv_prefix(data),
            block_by_block(aes, data),
            "{label}: IV prefixed"
        );
        if let Some((iv, body)) = data.split_at_checked(16) {
            let iv: [u8; 16] = iv.try_into().expect("an IV");
            assert_eq!(
                aes.cbc_decrypt_no_padding(&iv, body),
                unpadded_block_by_block(aes, iv, body),
                "{label}: the IV apart, no padding"
            );
        }
    }

    /// **Batched decryption is block-by-block decryption at every length**
    /// (P-13.6).
    ///
    /// Under a key of each length, held to decryption a block a pass with the
    /// padding checked a byte at a time: every plaintext length up to three
    /// passes of blocks, sealed and opened with its padding well formed; the
    /// same ciphertext with the last block's padding made bad — its length
    /// byte turned to 0 and to 17, and the padding byte furthest from the end
    /// changed — by a bit flipped in the block before it, which flips the
    /// same bit of the plaintext; the same ciphertext with a ragged tail of 1,
    /// 8 and 15 bytes; random ciphertext of every whole-block count to past
    /// three passes, with and without a ragged tail, its padding whatever it
    /// comes to; and every length under two blocks. The plaintext and the
    /// notes are block by block's, read with the IV prefixed and with it
    /// apart.
    #[test]
    fn batched_decryption_is_block_by_block_decryption_at_every_length() {
        let mut bytes = Bytes(0x6261_7463_6865_6421);
        for length in [16usize, 32] {
            let mut key = vec![0u8; length];
            bytes.fill(&mut key);
            let aes = Aes::new(&key).expect("the key expands");
            let mut iv = [0u8; 16];
            bytes.fill(&mut iv);

            for len in 0..=3 * 16 * BATCH {
                let label = format!("{length}-byte key, {len} bytes of plaintext");
                let mut plain = vec![0u8; len];
                bytes.fill(&mut plain);
                let sealed = aes.cbc_encrypt_with_iv_prefix(&iv, &plain);
                assert_eq!(
                    aes.cbc_decrypt_with_iv_prefix(&sealed),
                    (plain.clone(), Vec::new()),
                    "{label}: opened"
                );
                holds(&aes, &sealed, &label);

                let pad = 16 - len % 16;
                let before_last = sealed.len() - 32;
                for (at, flip) in [(15, pad as u8), (15, pad as u8 ^ 17), (16 - pad, 0x40)] {
                    let mut corrupt = sealed.clone();
                    corrupt[before_last + at] ^= flip;
                    let (_, notes) = aes.cbc_decrypt_with_iv_prefix(&corrupt);
                    assert_eq!(
                        notes,
                        vec![AesNote::BadPadding],
                        "{label}, padding byte {at} flipped by {flip:#04x}"
                    );
                    holds(
                        &aes,
                        &corrupt,
                        &format!("{label}, padding byte {at} flipped by {flip:#04x}"),
                    );
                }

                for extra in [1usize, 8, 15] {
                    let mut ragged = sealed.clone();
                    ragged.extend(std::iter::repeat_n(0x5a, extra));
                    holds(&aes, &ragged, &format!("{label}, {extra} ragged bytes"));
                }
            }

            for blocks in 0..=3 * BATCH + 2 {
                for extra in [0usize, 1, 15] {
                    let mut data = vec![0u8; 16 + 16 * blocks + extra];
                    bytes.fill(&mut data);
                    holds(
                        &aes,
                        &data,
                        &format!("{length}-byte key, {blocks} random blocks and {extra} bytes"),
                    );
                }
            }

            for len in 0..32 {
                let mut data = vec![0u8; len];
                bytes.fill(&mut data);
                holds(&aes, &data, &format!("{length}-byte key, {len} bytes"));
            }
        }
    }

    /// **A decrypted block costs a lane, not a pass** (P-13.6).
    ///
    /// Passes of the core are counted. Decrypting `n` whole blocks, with the
    /// IV prefixed or apart, runs `n / 8` passes rounded up — the batched
    /// pass's eight lanes — at every count from none to past three passes,
    /// and a ragged tail costs nothing more; decrypting the same blocks one
    /// at a time runs a pass a block, and so does sealing them, which is one
    /// chain, its block of padding included. The free function, which expands
    /// its key for the call, decrypts as the method does.
    #[test]
    fn a_decrypted_block_costs_a_lane_not_a_pass() {
        let aes = Aes::new(&[0x5c; 32]).expect("a 32-byte key expands");
        let iv = [0x3a; 16];
        passes::take();
        for blocks in 0..=3 * BATCH + 2 {
            let body: Vec<u8> = (0..16 * blocks).map(|at| (at * 37 + 11) as u8).collect();
            let mut data = iv.to_vec();
            data.extend_from_slice(&body);
            let batched = blocks.div_ceil(BATCH) as u64;

            let _ = aes.cbc_decrypt_with_iv_prefix(&data);
            assert_eq!(passes::take(), batched, "{blocks} blocks, the IV prefixed");
            let _ = aes.cbc_decrypt_no_padding(&iv, &body);
            assert_eq!(passes::take(), batched, "{blocks} blocks, the IV apart");
            data.extend_from_slice(&[0x77; 9]);
            let _ = aes.cbc_decrypt_with_iv_prefix(&data);
            assert_eq!(passes::take(), batched, "{blocks} blocks and a ragged tail");

            let _ = unpadded_block_by_block(&aes, iv, &body);
            assert_eq!(
                passes::take(),
                blocks as u64,
                "{blocks} blocks, one at a time"
            );
            let _ = aes.cbc_encrypt_with_iv_prefix(&iv, &body);
            assert_eq!(
                passes::take(),
                blocks as u64 + 1,
                "{blocks} blocks sealed, and their block of padding"
            );
        }

        let mut data = iv.to_vec();
        data.extend_from_slice(&[0x42; 16 * 17]);
        let _ = cbc_decrypt_with_iv_prefix(&[0x5c; 32], &data);
        assert_eq!(
            passes::take(),
            3,
            "seventeen blocks: two passes of eight and one of one"
        );
    }

    /// The lengths `chains_sealed_side_by_side_are_each_chain_sealed_alone`
    /// cycles through, each a block's edge or a few blocks.
    const CHAIN_LENGTHS: [usize; 14] = [0, 1, 15, 16, 17, 31, 32, 33, 47, 48, 64, 100, 129, 200];

    /// **Chains sealed side by side are each chain sealed alone** (P-13.7).
    ///
    /// Under a key of each length, every count of chains from none to past
    /// two passes' lanes — so that lanes refill as chains end, and a pass
    /// narrows to a word as they thin — of lengths that cycle through the
    /// block edges and a few blocks, a random byte longer or not, and the
    /// same counts of chains all one length: each chain's bytes are what
    /// sealing it alone with its IV writes, the chains come back in the order
    /// given, and each opens back to its plaintext.
    #[test]
    fn chains_sealed_side_by_side_are_each_chain_sealed_alone() {
        let mut bytes = Bytes(0x7369_6465_6279_7369);
        for length in [16usize, 32] {
            let mut key = vec![0u8; length];
            bytes.fill(&mut key);
            let aes = Aes::new(&key).expect("the key expands");
            for count in 0..=2 * SIDE_BY_SIDE + 3 {
                for equal in [false, true] {
                    let label = format!(
                        "{length}-byte key, {count} chains{}",
                        if equal { " of one length" } else { "" }
                    );
                    let plains: Vec<Vec<u8>> = (0..count)
                        .map(|at| {
                            let len = if equal {
                                45
                            } else {
                                CHAIN_LENGTHS[at % CHAIN_LENGTHS.len()]
                                    + usize::from(bytes.next() % 2)
                            };
                            let mut plain = vec![0u8; len];
                            bytes.fill(&mut plain);
                            plain
                        })
                        .collect();
                    let ivs: Vec<[u8; 16]> = (0..count)
                        .map(|_| {
                            let mut iv = [0u8; 16];
                            bytes.fill(&mut iv);
                            iv
                        })
                        .collect();
                    let chains: Vec<([u8; 16], &[u8])> = ivs
                        .iter()
                        .copied()
                        .zip(plains.iter().map(Vec::as_slice))
                        .collect();
                    let sealed = aes.cbc_encrypt_side_by_side(&chains);
                    assert_eq!(sealed.chains.len(), count, "{label}: a chain each");
                    for (at, ((iv, plain), out)) in chains.iter().zip(&sealed.chains).enumerate() {
                        assert_eq!(
                            out,
                            &aes.cbc_encrypt_with_iv_prefix(iv, plain),
                            "{label}: chain {at} is its bytes sealed alone"
                        );
                        assert_eq!(
                            aes.cbc_decrypt_with_iv_prefix(out),
                            (plain.to_vec(), Vec::new()),
                            "{label}: chain {at} opens"
                        );
                    }
                }
            }
        }
    }

    /// **A chain sealed beside others costs a lane, not a pass** (P-13.7).
    ///
    /// Passes and the words they run are counted. No chains take no pass.
    /// `k` chains of `n` blocks each take `n` passes for each eight of them,
    /// rounded up, at every `k` to past two passes' lanes and `n` from one
    /// block to three, and the passes the seal hands back are the passes the
    /// core ran; a pass carrying four chains or fewer is one word and one
    /// carrying more is two, so a group of eight runs two words a pass and a
    /// last group of four or fewer one. Chains of many lengths share passes —
    /// no fewer than the longest has blocks, nor than their blocks over
    /// eight, and fewer than their blocks. A lone chain takes a pass of one
    /// word a block, as sealing it alone does.
    #[test]
    fn a_chain_sealed_beside_others_costs_a_lane_not_a_pass() {
        let aes = Aes::new(&[0x2e; 32]).expect("a 32-byte key expands");
        passes::take();
        assert_eq!(aes.cbc_encrypt_side_by_side(&[]), SideBySide::default());
        assert_eq!(passes::take(), 0, "no chains, no pass");

        for n in 1..=3usize {
            let plain = vec![0x61u8; 16 * n - 1];
            for k in 1..=2 * SIDE_BY_SIDE + 3 {
                let chains: Vec<([u8; 16], &[u8])> = (0..k)
                    .map(|at| ([at as u8; 16], plain.as_slice()))
                    .collect();
                let sealed = aes.cbc_encrypt_side_by_side(&chains);
                let expected = (n * k.div_ceil(SIDE_BY_SIDE)) as u64;
                let last = match k % SIDE_BY_SIDE {
                    0 => 0,
                    1..=LANES => 1,
                    _ => BATCH_WORDS,
                };
                let words = (n * (BATCH_WORDS * (k / SIDE_BY_SIDE) + last)) as u64;
                assert_eq!(sealed.passes, expected, "{k} chains of {n} blocks");
                assert_eq!(
                    passes::take_words(),
                    (expected, words),
                    "{k} chains of {n} blocks: the passes the core ran, and their words"
                );
            }
        }

        let plains: Vec<Vec<u8>> = (0..23).map(|at| vec![0x33u8; 16 * (at % 12)]).collect();
        let chains: Vec<([u8; 16], &[u8])> = plains
            .iter()
            .enumerate()
            .map(|(at, plain)| ([at as u8; 16], plain.as_slice()))
            .collect();
        let sealed = aes.cbc_encrypt_side_by_side(&chains);
        let blocks: Vec<u64> = plains.iter().map(|p| (p.len() / 16 + 1) as u64).collect();
        let total: u64 = blocks.iter().sum();
        let longest = blocks.iter().copied().max().unwrap_or(0);
        assert!(
            sealed.passes >= longest,
            "no fewer than the longest chain's blocks"
        );
        assert!(
            sealed.passes >= total.div_ceil(SIDE_BY_SIDE as u64),
            "no fewer than the blocks over the lanes"
        );
        assert!(sealed.passes < total, "the chains shared passes");
        assert_eq!(passes::take(), sealed.passes);

        let plain = vec![0x7eu8; 16 * 5 + 3];
        let sealed = aes.cbc_encrypt_side_by_side(&[([1; 16], plain.as_slice())]);
        assert_eq!(sealed.passes, 6, "a lone chain, a pass a block");
        assert_eq!(passes::take_words(), (6, 6), "each pass one word");
        let _ = aes.cbc_encrypt_with_iv_prefix(&[1; 16], &plain);
        assert_eq!(passes::take_words(), (6, 6), "as sealing it alone runs");
    }
}
