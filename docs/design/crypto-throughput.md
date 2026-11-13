# Crypto throughput, constant-time

*For [ROADMAP](../ROADMAP.md) row P-13.* `crates/tinker-pdf-crypto/src/aes.rs`
encrypts a block a byte at a time: SubBytes reads a 256-byte S-box at each
state byte, MixColumns multiplies in GF(2^8) by a loop that branches on the
multiplier's bits and `xtime` branches on the top bit, and decryption rebuilds
its inverse table on every call and multiplies by 9, 11, 13 and 14 through the
same loop. Every CBC function expands its key on every call, and `FileKey`
hands the AESV3 file key to them once a string or a stream. `sha2.rs` copies
each 64-byte block into an array, builds a 64-word schedule before its rounds
and reads both through bounds-checked `get`. The private-key arithmetic is
SG-01's: [signing](signing.md) designs it, a constant-time fixed-window
Montgomery exponentiation with CRT and blinding over `bignum.rs`'s CIOS
multiplication, and this document speeds it and does not reimplement it.

This document fixes the threat model that makes AES and the private-key path
constant-time and the trace tests that show it; the sliced AES core — its
lanes, its layout, the circuit that is its S-box, the linear layers, the key
schedule — and encryption and decryption through it; CBC decryption batched
across the core's lanes; several streams' CBC chains sealed side by side on
save; the expanded key held once a file; SHA-256's schedule; the Montgomery
work on SG-01's path; and the budget of the CLI encrypt, with what this design
cannot bring within it.

## Scope

- **The threat model**: which values are secret, what a path that touches one
  may do, and the two trace tests and the operation-count test that hold it.
- **The sliced core** (P-13.4, P-13.5): its words and lanes, its bit layout, the
  S-box circuit, the fixsliced linear layers, the key schedule through the
  circuit, encryption and the inverse cipher.
- **CBC through the core**: one chain a lane; decryption batched across the
  lanes (P-13.6); streams sealed side by side (P-13.7).
- **The expanded key held once**: `Aes` and `FileKey` (P-13.4).
- **SHA-256's schedule** (P-13.3).
- **The Montgomery core at the CRT halves' width** and its squaring (P-13.8).
- **The CLI encrypt's budget**, phase by phase, and the row that carries each
  phase that is not sealing.

## Non-goals

- **AES and SHA instructions.** They need `unsafe` intrinsics or a CPU
  baseline above the one distributed binaries are built for; H-16 keeps
  `forbid(unsafe_code)` and H-43 the baseline build. The core is safe integer
  code shaped for the autovectoriser.
- **AES-192**, which PDF never uses; `Aes::new` refuses a 24-byte key.
- **The private-key arithmetic itself**: [signing](signing.md)'s path, its
  rules and its operation-count test are SG-01's.
- **RC4 and MD5.** RC4's state is a permutation its key builds and its own
  bytes index, so no constant-time RC4 exists short of another cipher; it and
  the MD5 of Algorithm 1 serve the revisions H-18 marks weak, and keep their
  code. DES keeps its code for the public-key handler's legacy recipients.
- **Power, electromagnetic and fault channels** past the fault check SG-01
  makes. The model below is timing: what a branch or a memory access reveals.
- **A claim the source cannot back.** The trace tests show the source's
  shape; they cannot show the machine code a target runs. H-67 reviews it.

## Design

### The threat model

**What is secret.** A file encryption key and every key derived from it — an
AESV2 object key, Algorithm 2.B's per-round AES-128 keys, the intermediate
key that unwraps `/UE` and `/OE` — and every state of a cipher run under one,
plaintext or ciphertext, between its first round key and its last. The
plaintext a decryption recovers is secret; the ciphertext it reads and the
lengths of both are not, since the document carries them. A password, and
every hash Algorithm 2.B computes from it, is secret. The private-key
operands are [signing](signing.md)'s list.

**The adversary** shares the machine, or measures from outside it how long
an operation takes, while the engine works under a key the adversary does not
hold — a server decrypting and signing other people's documents is the case
ruling 20 is written for. What reaches them is the time an operation takes and
the cache lines and branch predictor state it disturbs. So a path that touches
a secret keeps two rules, ruling 20's:

1. **No memory access indexed by a secret.** No table is read at an index a
   state byte, a key byte or a password byte chooses. The S-box is a circuit,
   never an array.
2. **No branch on a secret.** No `if`, `match`, early return or loop bound
   depends on a secret value. A loop runs over a public count — the rounds a
   key's length gives, the blocks a length gives — and a choice between two
   values is a mask.

Public values are everything a document or a verifier holds: lengths,
counts, ciphertext, a public key, a signature. The round count and the lanes a
pass fills are public.

**What holds it.** The core records in test builds every table it reads and
every branch it takes, into a trace (*The trace*, below); a test runs it over
random keys and blocks and holds the trace equal on every run, so that no read
and no branch moved with a secret. The encryption core's test is P-13.4's, the
inverse core's P-13.5's. The private-key path's is SG-01.2's operation-count
test, which P-13.8 extends to its squaring.

**The padding check** of a decryption (P-13.5) reads the last block's bytes:
it reads all sixteen whatever the padding length, and computes its verdict by
masks, so that how long it takes says nothing of the plaintext. Its answer is
the one the check a byte at a time gives — strip the padding or report
`AesNote::BadPadding` — and the note is the one thing it reveals, since ruling
10 has it reported.

### The sliced core: lanes and layout

**A word holds four lanes.** The state is eight **planes**, plane `b`
holding bit `b` of every byte of the pass's blocks. A plane is `W` 64-bit
words, and each word holds four **lanes** — four blocks, sixteen bits a lane —
so a pass of `W` words is `4W` lanes, a lane a block of its own chain. One pass
runs every lane through every round at once, and costs what it costs whatever
lanes it fills. Every operation of a pass is the same on each word of a plane,
so a pass of two words is a pair the vectoriser fuses into one 128-bit
instruction (SSE2 on x86-64, `+simd128` on wasm), and a build that does not
fuse them does two instructions for twice the lanes. **The single chain's pass
is one word, four lanes; the batched paths' (P-13.6, P-13.7) is two, eight
lanes.** In sixty-four bits every plane operation is one integer instruction on
every target.

**The layout** of a word's sixty-four bits puts state byte `(r, c)` (FIPS 197
3.4: input byte `r + 4c`) of lane `l` at bit `16r + 4c + l`: rows outermost,
then columns, the lanes innermost. Two moves are then cheap:

- **a row down**, `(r + 1, c)` brought to `(r, c)`, is the whole word rotated
  right by 16 — one instruction;
- **a column across** inside a row, `(r, c + k)` to `(r, c)`, is each 16-bit
  row segment rotated right by `4k`: two shifts, two masks and an OR.

**Slicing a word** — four blocks into eight planes' words and back — is three
layers of swap-moves over eight words, after each block is loaded as two words whose
bytes are interleaved so that the swap-moves land each bit where the layout
puts it: the bytes of columns 0 and 2 in one word and of columns 1 and 3 in
the other, row by row. The three layers exchange the three bits of a bit's
index in its byte with the three bits of its word's index, so each is its own
inverse and unslicing runs them again before the bytes are de-interleaved. A
lane a pass leaves empty is zero, and costs only the instructions it is folded
out of.

### SubBytes: a circuit

The S-box is the depth-16 circuit of 128 gates — 34 AND and 94 XOR or XNOR —
that Boyar and Peralta give for the AES S-box (2012): a linear layer of
twenty-seven XORs, a non-linear middle computing the inverse in a tower field,
and a linear layer that includes the affine map, its four XNORs FIPS 197's
constant 0x63. It runs on the eight planes as they stand, so a word of it is
sixty-four S-boxes and reads no table.

**The inverse S-box** is the same circuit between two copies of FIPS 197's
inverse affine map, 5.3.2's `b'_i = b_{i+2} ⊕ b_{i+5} ⊕ b_{i+7} ⊕ d_i`: the
S-box is the affine map of the inverse, so the inverse is the inverse affine
map of the S-box of the inverse affine map. Sixteen XORs and two NOTs each
side; it costs more than a dedicated circuit and is the forward circuit's
correctness and nothing else's.

**The circuit is held to the table when the crate compiles.** The core is
`const fn`, and a `const` item runs the circuit over all 256 inputs and
FIPS 197's S-box table, the inverse over its inverse, and the whole cipher
over appendix C's AES-128 and AES-256 vectors in both directions, in every
lane; a gate written wrong is a build that does not compile, not a test that
fails later. The table stays in the source for that item and nothing else
reads it.

### The linear layers: fixsliced

ShiftRows is the one step the layout makes dear: each row segment rotated by
its own amount. So it is never run between rounds. Write `SR` for ShiftRows,
`MC` for MixColumns, `SB` for SubBytes; `SB` commutes with `SR`, being bytewise.
The state held after round `i` is `SR^-i` of the cipher's own, and round `i`
becomes

```text
y_i = MC_(i mod 4)(SB(y_(i-1))) ⊕ SR^-i(k_i)        MC_(j) = SR^-j ∘ MC ∘ SR^j
```

with the round keys moved by `SR^-i` once, in the key schedule, and the last
round `SR^Nr(SB(y)) ⊕ k_Nr` — `SR²` for both key sizes, since 10 and 14 are 2
modulo 4 (the fixslicing of Adomnicai and Peyrin, 2021). `MC_(j)` mixes the
bytes `(r + i, c + j·i)`: the column read along a diagonal. With `N(x)` the
state moved a row down and `j` columns across, and `N²` twice that,

```text
y = x ⊕ N(x)        s = y ⊕ N²(y)        MC_(j)(x) = xtime(y) ⊕ s ⊕ x
```

since MixColumns' row is `2a_0 ⊕ 3a_1 ⊕ a_2 ⊕ a_3 = 2(a_0 ⊕ a_1) ⊕ s ⊕ a_0` for
`s` the sum of the four. `xtime` on planes is a renaming of planes and three
XORs, the reduction by 0x1B. For `j = 0` the moves are rotations by 16 and 32;
for `j = 2`, `N²` is the rotation by 32 alone; for 1 and 3 each move adds a
row-segment rotation. No GF(2^8) multiplication branches, because none is
written as one.

**The inverse cipher** is fixsliced the other way: holding `SR^a` of the
state, `a` one more each round, absorbs InvShiftRows, and InvMixColumns is
`MC ∘ D` for `D(a)_r = a_r ⊕ 4(a_r ⊕ a_(r+2))` — `[0E 0B 0D 09]` is
`[02 03 01 01]` times `[05 00 04 00]` as circulants — conjugated alike, so it
is the forward mixing after one more move and two `xtime`s. The round keys
at either end are taken plain and those between moved by `SR^a`, and the
plaintext is `SR²` of the last state before the first round key is added, the
key sizes again agreeing.

### The key schedule

FIPS 197 5.2's expansion, with SubWord run through the circuit: the word's
four bytes in a block of lane 0, sliced, through the S-box, unsliced. The
round constants are a table read at the round's index, which is public. Each
round key between the first and the last is moved by its round's `SR^-i` — the
inverse cipher's by `SR^a` — and every round key is sliced into every lane, so
that a pass's lanes run under one key. `Aes::new` expands once, and an `Aes`
is the expanded key: its round keys are wiped when it is dropped, with
`core::hint::black_box` and a compiler fence over the stores (the crates
forbid `unsafe`, so no volatile write), and its `Debug` prints `[redacted]`.

### The trace

The core takes a trace, a value it records into in test builds and that is
empty and costs nothing in every other: each table read as the table and the
index, each branch as its site and its outcome, folded into a digest beside a
count of each. Every table the core reads and every condition it tests goes
through it. A test that runs the core over random keys and blocks and finds one
digest has shown that every read and every branch is the same whatever the key
and the block were — that none is indexed by, or turns on, a secret.

### CBC through the core

**One chain is one lane.** CBC encryption is sequential: block `n` enters the
cipher XORed with block `n − 1`'s ciphertext. A lone chain — a string, a lone
stream, Algorithm 2.B's inner loop, the bench's 2.5 MB — is a pass of one word
a block, with one lane filled. The other three lanes cost the instructions
slicing an empty lane folds out of; the circuit and the linear layers cost a
whole word.

**Decryption batches inside a chain** (P-13.6). Each plaintext block is
`D(C_n) ⊕ C_(n−1)`, and the `D(C_n)` are independent, so
`cbc_decrypt_with_iv_prefix` and `cbc_decrypt_no_padding` decrypt eight
ciphertext blocks a pass, XOR each with the block before it, take a short
tail through the same path with its unused lanes empty, and write into output
reserved once. `AesNote` comes out as it does: `TooShort` and `RaggedTail` are
decided from lengths before the first pass, and the padding check reads the
last block whatever pass produced it.

**Encryption batches across chains** (P-13.7). A save's streams are
independent chains, each with its own IV, which `write.rs`'s `iv_for` derives
from the file key and the object rather than draws (ruling 4). The writer
gathers the payloads of the streams it is about to seal, runs eight chains a
pass — lane `l` the next block of the `l`-th chain, a chain that has ended
leaving its lane empty — and gives each stream its ciphertext. Every byte is
the single-chain path's for the same IV and payload, so the output of a save is
what sealing each stream alone writes. The expanded key is held once, by
`StreamCipher`. Strings, a lone stream, and the inherited RC4 and AESV2
ciphers keep their single-chain path. A save of one large stream and nothing
else — the bench's 2.5 MB file is one image — has one chain and no lane to
fill.

### The expanded key held once

`Aes` gains the CBC operations over its own round keys, and the functions that
take a key's bytes expand once and call them. `FileKey` expands its AESV3 key
once, when it is made — by `authenticate` and by `from_derived` — and seals
and opens every string and stream with it; a key AES does not take is held as
no key, and those strings and streams pass through as they do. AESV2 still
derives a key an object (7.6.2), so it still expands one an object. A counter
of expansions, compiled only under `cfg(test)`, shows a file's strings and
streams costing one.

### SHA-256's schedule

`compress` takes each whole block where it lies in the input — a 64-byte
slice of what `update` was handed — and only a block that straddles two calls
is gathered into the buffer. The schedule is a ring of sixteen words: round `t`
reads `W[t mod 16]`, and from round 16 replaces it with
`σ1(W[t−2]) + W[t−7] + σ0(W[t−15]) + W[t−16]` from the ring before using it,
so no 64-word array is built ahead of the rounds and every index is a constant
or a mask of the round counter; no `get` is bounds-checked inside either loop.
SHA-384 and SHA-512 keep their code. A counter of bytes copied into the block
buffer, compiled only under `cfg(test)`, shows whole blocks copied nowhere.
SHA-256 has no secret-dependent branch or index in either form, so the
rewrite is speed alone; it also sets most of a 10 MB signature verification's
time.

### The Montgomery core on SG-01's path

[signing](signing.md)'s fixed-window exponentiation spends its time in
Montgomery products at the CRT halves' width — a 1 024-bit half is sixteen
64-bit limbs. P-13.8 gives that path a core of its own:

- **CIOS over the modulus's own limb count**, each operand's fixed array cut
  once to that count and the loops walking the slices, so there is no `read`
  or `write` call a limb and no bounds check inside the loops; the two limbs
  above the width held in registers between rounds; the final subtraction by
  mask, as [signing](signing.md)'s rule 4 has it.
- **A dedicated squaring**: the cross products `a_i·a_j` for `i < j` formed
  once and doubled, the squares `a_i²` added, then the same reduction — `n(n+1)/2`
  limb products in the multiplication half against `n²` for a product — used
  for the four squarings of every window, the fault check's squarings under
  `e` and the blinding pair's.
- **Still branch-free on every secret**: the squaring's loops run over the
  limb count, and its doubling carries by arithmetic.

Verification keeps `Modulus::pow`'s public-exponent path. SG-01's
operation-count test, extended to the squaring, holds the counts equal over
random keys and messages; it is the row's timing-independence exit as well as
SG-01's.

### The CLI encrypt's budget

`write/encrypt the 2.5 MB image file, CLI` runs `tpdf encrypt` with both
passwords empty over the bench's 2.5 MB file (`inputs::small_image_file`): one
page whose content is one baseline JPEG image stream, which holds nearly all of
the file's bytes. Tier P's target is 16.1 ms, rescaled from the 23 MB file's
150 ms. Its phases, the share of 16.1 ms each may take, and the
row that carries each:

| Phase | Share | Row |
| --- | --- | --- |
| The process: start, arguments, the 48-byte entropy file | 1.0 ms | P-05, held by P-21's small-file open |
| Reading and opening the input: the tail, the cross-reference data, the objects on demand, no second copy | 1.5 ms | P-05 |
| The R6 handler: Algorithms 8, 9 and 10, four Algorithm 2.B hashes and `/Perms` | 4.0 ms | P-13 |
| The rewrite: every object serialised, the output written | 1.6 ms | P-01, P-02, held by P-21 |
| Sealing: each object's IV derived and every string and stream encrypted | 8.0 ms | P-13 |

**What this design cannot bring within it.** Two of those phases are a single
CBC chain, and the budget does not hold either at the row's own single-chain
target of 80 MB/s:

- **Sealing** this file is one chain: its image stream is nearly all of its
  2.5 MB in one object, and no other stream is large enough to fill a lane
  beside it. At 80 MB/s the chain alone takes 31 ms — twice the operation's
  whole budget. Side-by-side sealing (P-13.7) is what brings a save of many
  streams, the 23 MB file's images among them, to the batched rate; it has
  nothing to batch here.
- **The handler** is fixed by 7.6.4.3.4 whatever the file's size: each of the
  four Algorithm 2.B hashes runs at least 64 rounds, each an AES-128 CBC chain
  over 64 copies of the password, the running hash and the user data — at
  least 2 048 bytes a round for `/U` and its key, at least 5 120 for `/O` and
  its key, which hash `/U` as the user data — and a SHA-2 hash of the same.
  That is at least 917 504 bytes of single-chain AES-128 and as much SHA-2 for
  a file of any size. The rescale from 23 MB that set 16.1 ms scaled this fixed
  cost with the file.

So the operation's 5.13x is not reachable on this input by any design of this
row, and H-167 puts it to the owner with P-13.9's paired figures in hand
(H-77: a target gates as written until the owner re-states it). The other two
phases of P-13's are what this design does reach: decryption at the batched
rate inside every chain, a save of several streams at the batched rate across
them, and the expanded key held once.

## Milestones

1. **P-13.2, the vectors**, before either primitive changes. NIST CAVP's
   AESAVS files for CBC under 128- and 256-bit keys — the known-answer files
   (GFSbox, KeySbox, VarKey, VarTxt), the multi-block messages and the Monte
   Carlo test — and SHAVS's SHA-256 short-message, long-message and Monte Carlo
   files, each untrimmed with its provenance in its header, read through
   `include_str!`. Exit: `aes.rs` `aesavs_known_answer_and_cbc_vectors_pass`
   and `sha2.rs` `shavs_sha256_vectors_pass` run every vector, both
   directions, count them against the files and pass on today's code.
2. **P-13.3, SHA-256's schedule.** Exit: P-13.2's SHAVS vectors,
   `sha256_known_answers`, `streaming_matches_one_shot` and
   `block_boundary_lengths_are_exact` pass; `a_whole_block_is_compressed_where_it_lies`
   counts the bytes copied into the block buffer while whole blocks are hashed,
   none; revision 6's Algorithm 2.B tests and every `encrypt_on_save.rs` test
   unchanged.
3. **P-13.4, the encryption core.** Exit: P-13.2's AESAVS encrypt-direction
   vectors, `fips197_aes128_block`, `fips197_aes256_block` and
   `sp800_38a_cbc_aes128` pass; `aes.rs`
   `the_core_reads_no_secret_indexed_table_and_branches_alike` finds one trace
   over random keys and blocks and no read indexed by either;
   `handler.rs` `the_aesv3_key_is_expanded_once_per_file` counts one expansion
   over a file's strings and streams; `hash_2b_terminates_and_is_deterministic`
   unchanged. The module note keeps its disclaimer: decryption is still
   today's.
4. **P-13.5, the inverse core.** Exit: P-13.2's AESAVS decrypt-direction
   vectors, the FIPS 197 round trips, `sp800_38a_cbc_aes128`,
   `iv_prefixed_round_trip_strips_padding` and `damage_returns_notes_not_panics`
   pass; `aes.rs` `the_inverse_core_reads_no_secret_indexed_table_and_branches_alike`;
   every revision-6 open in `crates/tinker-pdf-cos/tests/encryption.rs`
   unchanged; the padding check reads its block by masks, with the notes it
   gave; the module note becomes the claim of constant-time execution, and
   [encryption](../features/encryption.md) says so.
5. **P-13.6, batched decryption.** Exit: `aes.rs`
   `batched_decryption_is_block_by_block_decryption_at_every_length` — lengths
   up to three passes of blocks, ragged tails and bad padding, the same
   plaintext and notes — and `a_decrypted_block_costs_a_lane_not_a_pass`,
   counting passes; P-13.2's CBC decrypt vectors,
   `crates/tinker-pdf-cos/tests/encryption.rs` and the AES-256 fixtures'
   reads unchanged.
6. **P-13.7, streams sealed side by side.** Exit: `encrypt_on_save.rs`
   `streams_sealed_together_are_the_bytes_each_seals_alone` and
   `a_save_seals_its_streams_a_lane_each`, counting passes over a save of
   equal streams — at most the blocks of one stream times the streams over the
   lanes, rounded up; `identical_content_encrypts_differently`, every
   `encrypt_on_save.rs` test and `linearized.rs`
   `an_encrypted_linearized_file_is_byte_for_byte_reproducible` unchanged.
7. **P-13.8, the Montgomery core.** Exit: SG-01's signature-generation vectors
   pass unchanged; SG-01's operation-count test, extended to the squaring,
   shows the same counts over random keys and messages; `bignum.rs`
   `a_squaring_forms_each_cross_product_once` counts `n(n+1)/2` limb products
   in its multiplication half for `n = 16` against `n²` for a product, and the
   squaring equals `mul(a, a)` on random operands;
   `montgomery_products_agree_with_u128` unchanged.
8. **P-13.9, the row's close** in MS-02's paired run: the floors raised to
   AES-256-CBC decrypt 4.68x and encrypt 1.4x, SHA-256 1.33x, the 10 MB
   verification 1.26x, the RSA-2048 private-key signature 3x against the
   baseline's private-exponent `pow`, and the CLI encrypt 5.13x, or to what
   H-167's answer re-states for the encrypt and the CLI encrypt; the
   private-key operation-count test and the two AES trace tests green in
   `cargo test` on Linux and Windows and under `--target wasm32-wasip1`.

## Risks

| Risk | Mitigation |
| --- | --- |
| A gate of the circuit, a plane move or a round-key move is wrong | The core is `const fn` and the crate does not compile unless the circuit is FIPS 197's table at all 256 inputs, the inverse its inverse, and the cipher appendix C's vectors both ways in every lane; P-13.2's AESAVS files then run every vector and checkpoint at test time |
| One chain through the sliced core is slower than the table-driven block it replaces | A lone chain pays a whole pass a block: 128 gates of S-box and the fixsliced mixing for sixteen bytes, where the code it replaces does sixteen table reads and a few shifts and XORs — and that code reads a secret-indexed table, which ruling 20 rules out. The single-chain path is made as cheap as a pass can be: one lane sliced, the round keys sliced once a key, `ShiftRows` never run between rounds, every operation one integer instruction. Whether it reaches 80 MB/s, or holds today's rate, is the timing phase's; every operation whose time is one chain — the encrypt, the CLI encrypt, opening an R6 file through Algorithm 2.B — is named in H-167, never eased by a lowered floor without the owner (H-78) |
| The CLI encrypt's target is out of reach on its input | Its sealing is one chain and its handler a fixed cost (*The CLI encrypt's budget*); H-167 |
| The optimiser turns a mask into a branch, or a table appears where the source has none | The trace tests hold the source; H-67 reviews the machine code on every target, wasm's among them |
| A batched pass or a side-by-side save writes other bytes than the single chain | `batched_decryption_is_block_by_block_decryption_at_every_length` and `streams_sealed_together_are_the_bytes_each_seals_alone`; the IVs are derived, so the bytes are a function of the file key and the object (ruling 4) |
| `AesNote` moves | Lengths decide `TooShort` and `RaggedTail` before any pass; the padding check reads the last block as before, by masks; `damage_returns_notes_not_panics` and the encrypted fixtures' reads unchanged |
| An expanded key outlives its use | `Aes` wipes its round keys on drop and never prints them; `FileKey` holds one for the file's life, as it holds the key's bytes |
| SHA-256's ring reads a word before it is written | The ring is written at round `t` before round `t` reads it, from words at most 16 rounds old; the SHAVS files and the block-boundary tests run every message length from 0 to 64 bytes and 64 long ones |
| The squaring forms a product the multiplication does not | It equals `mul(a, a)` on random operands, and SG-01's vectors pass through it |
