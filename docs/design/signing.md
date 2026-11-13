# Signing in the engine

The engine signs with a private key it is handed. Ruling 20 is the owner's
answer to H-09 and the rules this design implements: every operation on a
secret operand runs on a path whose branches and memory accesses do not
depend on the secret; entropy is the caller's; key material is zeroised when
it is dropped; the algorithms and key formats are tier SG's list; nonces are
RFC 6979's deterministic ones, hedged on request; a PSS salt is the caller's;
and keys cross the C ABI and the bindings as bytes and a passphrase, never
logged, with `RawSigner` and two-phase signing for a key held elsewhere.

This document designs what every later section stands on — the constant-time
path, entropy, key material and its zeroisation — and RSA signing on it in
full (ROADMAP row SG-01), which is also the private-key arithmetic
`docs/design/crypto-throughput.md` (P-13.1, for row P-13) speeds and does not
reimplement. ECDSA (SG-02), EdDSA (SG-03), key import (SG-05), CMS
`SignedData` built in the engine (SG-06) and the PAdES baseline levels
(SG-07) each add their section here in their own design step, on the frame
below; *The sections the later rows add* says what each takes from it.

[signatures](signatures.md) is the reading side and the caller-held signing
that exists: the verdict, `DocumentEditor::save_signed` and the `Signer`
callback that returns finished CMS bytes. That callback stays, beside the
`RawSigner` seam SG-06 adds, for keys the engine never sees.

## Scope

- **The constant-time path**: which values are secret, what the path may and
  may not do with one, and the operation-count test that holds it.
- **Entropy**: the one seam a signing operation draws from, and what it
  refuses without.
- **Key material**: the types that hold a secret, their zeroisation on drop,
  their redaction from every printed form, and how a key moves without
  leaving copies behind.
- **RSA signing** (SG-01): the private operation by the Chinese remainder
  theorem with blinding, a fault check before a signature leaves, fixed-window
  Montgomery exponentiation on the constant-time path, and RSASSA-PKCS1-v1_5
  and RSASSA-PSS signature generation over SHA-256, SHA-384, SHA-512,
  SHA-512/256 and, with SG-04, SHA3-256, SHA3-384 and SHA3-512, the digest the
  caller's choice.
- **The frame the later rows extend**: ECDSA, EdDSA, key import, CMS
  `SignedData` and PAdES, each named with the milestones it adds.

## Non-goals

- **Key generation and certificates.** Keys made here, self-signed
  certificates, PKCS#10 requests and PKCS#12 export are ROADMAP row SG-23.
  Every key this design signs with comes from a caller, by SG-05's import or
  as its numbers.
- **The C ABI, the bindings and two-phase signing.** Ruling 20 fixes what
  crosses — key bytes and a passphrase, never logged; `RawSigner` and
  two-phase signing for a key held elsewhere; one signing callback for a key
  the ABI does not hold — and SG-09 builds it, in
  [bindings-write](bindings-write.md). The engine API below is what it
  projects.
- **Providers that hold keys.** PKCS#11 (SG-27) and the operating systems' key
  stores (SG-28) reach a key through `RawSigner`; the engine never holds one.
- **Post-quantum signatures** are SG-25.
- **Revocation and long-term validation data.** SG-07's B-LT carries the
  revocation data SG-10 parses; nothing here fetches it.
- **A claim of resistance an outside review has not made.** The operation
  counts below show the path's shape in source; they cannot show the absence
  of a timing leak in the machine code a target runs. H-67, an independent
  cryptographic review, precedes shipping SG-01 to SG-09.

## Design

### Secret operands

A value is **secret** when it is key material or anything computed from key
material that is not published by the signature it makes:

- an RSA private exponent and the parts of its second representation (RFC
  8017 §3.2): the primes `p` and `q`, `dP`, `dQ` and `qInv`;
- an ECDSA or EdDSA private scalar, a per-signature nonce `k` and its
  inverse, and an EdDSA key's hash prefix;
- the blinding factor `r`, its inverse and every intermediate of a blinded
  operation before the unblinding;
- a passphrase and every key derived from it (SG-05).

Public values are everything a verifier sees: the modulus `n`, the public
exponent `e`, a public point, the digest, the signature. `bignum`'s existing
path — `Modulus::pow`, which branches on exponent bits and skips leading zeros
— stays the public-exponent path verification uses and touches no secret.

### The constant-time path

`tinker-pdf-crypto` gains a second path through `bignum` for secret
operands, and on it four rules hold:

1. **No branch on a secret.** No `if`, `match`, early return, `?` or loop
   bound depends on a secret value or a secret bit. A choice between two
   values is a mask: `mask = 0u64.wrapping_sub(bit)` and
   `(a & mask) | (b & !mask)`, the mask passed through `core::hint::black_box`
   so that the optimiser has no bit to branch on.
2. **No memory access indexed by a secret.** A table entry chosen by a secret
   window is read by reading **every** entry and keeping the wanted one by
   mask.
3. **Fixed lengths.** Every loop runs over the modulus's own limb count and
   every exponentiation over the bit length of the modulus it works under, so
   a secret's leading zeros take as long as its leading ones; nothing is
   normalised.
4. **Branch-free reduction.** Montgomery multiplication's final conditional
   subtraction computes `t - m` with its borrow and selects between `t` and
   `t - m` by mask, where the public path compares and subtracts.

**Fixed-window exponentiation.** A secret exponent is consumed four bits at a
time from the top, over a fixed number of windows — the modulus's bit length
over four, rounded up. A table of the sixteen powers `base^0` to `base^15` in
Montgomery form is built first; each window then squares four times and
multiplies by the table entry the window names, read by rule 2, the
multiplication made even for a window of zero (by `base^0`, one in
Montgomery form). Four bits keeps a window's full scan of the table at sixteen
reads against four squarings, for the halves of 1 024- to 4 096-bit moduli the
path serves.

**The operation-count test.** Under `cfg(test)` the path counts what it does:
each branch it takes, each table read and each limb product. The test runs the
exponentiation over exponents of every shape — every bit clear, every bit
set, alternating, and random draws — on random bases and moduli, and holds the
counts equal across all of them: a count that moved with the exponent is a
branch or a lookup that depends on it. It is SG-01.2's exit, P-13's
timing-independence exit, and P-13.8 extends it to the dedicated squaring.

### Entropy

**One seam, the caller's.** `tinker_pdf_crypto::EntropySource` — already
the seam encryption keys come from — supplies every unpredictable value a
signing operation needs: RSA blinding factors, PSS salts and the extra input
of a hedged ECDSA nonce. The engine links no random number generator. A
source that cannot fill its buffer is a typed refusal, and the operation
does not proceed: a private operation is never run unblinded because entropy
ran out.

**Determinism is kept where the scheme allows it** (ruling 4). An RSASSA-PKCS1-v1_5
signature is a function of the key and the digest, and blinding does not move
it; a PSS signature is a function of its salt, which is the caller's and which
a test fixes; an ECDSA signature takes RFC 6979's deterministic nonce unless
the caller asks for a hedged one. So a signed document's bytes are identical
on every target for a deterministic key and request (K-07).

### Key material

**Secret limbs live in `Secret` values.** `Secret<N>` holds an `N`-limb
number on the heap, in one allocation that moving the value does not copy:
Rust moves a stack value by copying its bytes and leaves the old ones in
place, so a secret is never a stack value that moves. It is neither `Copy`
nor `Clone`; a copy is an explicit call, and itself a `Secret`.

**Zeroised on drop.** `Secret`'s `Drop` writes zero to every limb, passes
the limbs through `core::hint::black_box` and issues a
`compiler_fence(SeqCst)`, so the stores are not elided as writes to memory
about to be freed. The constant-time path's scratch — the window table, the
accumulators and the blinded intermediates — is wiped the same way before the
operation returns. The crates forbid `unsafe`, so there is no volatile write;
the risks say what that leaves.

**Never printed.** `Debug` on a private key or a `Secret` writes
`[redacted]`, and nothing in the engine formats a secret into a warning, an
error or a log line. Passphrases (SG-05) are bytes in a `Secret` buffer
under the same rules.

**The RSA private key** is `RsaPrivateKey`: the public `n` and `e`, and
`d`, `p`, `q`, `dP`, `dQ` and `qInv` each a `Secret`. Building one checks
what can be checked without trusting it — `p · q = n`, both odd, the CRT
exponents below their primes, `qInv · q ≡ 1 (mod p)` — and refuses a key that
fails, by a typed error naming the check. A modulus is at most 4 096 bits,
`bignum::MAX_LIMBS`; a wider key is refused, as verification refuses a wider
modulus.

### RSA signing

**The private operation is the second representation's** (RFC 8017 §5.2.1,
step 2b): for a message representative `c`,

```text
m1 = c^dP mod p        m2 = c^dQ mod q
h  = qInv · (m1 - m2) mod p
s  = m2 + q · h
```

each exponentiation the fixed-window one at the half's width, which is
where the time goes and what P-13.8 speeds.

**Blinded by the caller's entropy.** Before the exponentiations `c` is
multiplied by `r^e mod n` and after them `s` by `r^-1 mod n`, for an `r`
uniform in `[1, n)` (Kocher, 1996), so the values the private
exponents meet are unrelated to the message an observer chose. The pair
`(r^e, r^-1)` is held with the key behind a lock: drawn from the caller's
source on the key's first private operation and on every thirty-second after
it, each draw paying one inverse — computed under each prime by Fermat's
little theorem, `r^(p-2) mod p` and `r^(q-2) mod q` on the constant-time
path, and joined by the CRT — and squared in between, `r^e` to `r^2e` and
`r^-1` to `r^-2`, which keeps a pair's inverse an inverse at the cost of two
multiplications a signature. A drawn `r` that shares a factor with `n` is
drawn again.

**A fault does not leave.** A miscomputed half reveals a prime to anyone
holding the faulty signature and a correct one (Boneh, DeMillo and Lipton
1997), so every signature is checked with the public exponent — `s^e mod n`
against `c` — before it is returned, and one that fails is a typed refusal
that carries nothing of it.

**The schemes.** `RsaPrivateKey::sign_pkcs1_v15` is RSASSA-PKCS1-V1_5-SIGN
(§8.2.1): EMSA-PKCS1-v1_5 (§9.2) builds `0x00 0x01 PS 0x00 T` with `T` the
`DigestInfo` of the caller's digest, from the same per-digest prefixes the
verifier compares whole. `RsaPrivateKey::sign_pss` is RSASSA-PSS-SIGN
(§8.1.1): EMSA-PSS-ENCODE (§9.1.1) with MGF1 (B.2.1) over the message digest
and the salt the caller passes — its length is the PSS salt length, and a
salt the encoding has no room for is refused. The digests are SHA-256,
SHA-384, SHA-512 and SHA-512/256, and SHA3-256, SHA3-384 and SHA3-512 when
SG-04's Keccak lands; the caller names one, and nothing picks it for them.

**What holds them** is published data, never the engine agreeing with
itself (ruling 13): NIST CAVP's signature-generation files for PKCS#1 v1.5
and for PSS, whose PSS records state their salts, and Wycheproof's RSA
signature files (Apache-2.0), vendored with their licence records (ruling 18)
and run whole. A signature the signer makes and the verifier accepts proves
only that the two agree.

### The sections the later rows add

Each is designed by its row's first step, which adds its section here and
names its steps' exit tests; each stands on the constant-time path, the
entropy seam and `Secret` above.

- **ECDSA** (SG-02.1): constant-time field arithmetic per curve — P-256,
  P-384, P-521 and Brainpool P256r1, P384r1 and P512r1 — on this path,
  complete addition formulas, a scalar multiplication with no branch or index
  on the scalar's bits, and nonces by RFC 6979 §3.2, hedged on request by
  its §3.6 extra input from the caller's source.
- **EdDSA** (SG-03.1): Curve25519's and Edwards448's arithmetic on this path,
  RFC 8032's Ed25519 and Ed448, and their carriage in CMS by RFC 8419.
- **Key import** (SG-05.1): PKCS#8 plain and encrypted (PBES2 with PBKDF2 and
  AES-CBC, and PBES1), PKCS#12 with its legacy schemes and MAC, PKCS#1
  `RSAPrivateKey`, SEC 1 `ECPrivateKey` and PEM armour, each yielding the key
  types above; passphrases as bytes in a `Secret`, never logged; and the
  provenance of the committed key files the tests read.
- **CMS `SignedData`** (SG-06.1): the signed attributes — content type,
  message digest, ESS signing-certificate-v2, an optional signing time — the
  chain and the unsigned timestamp, DER's canonical `SET OF` order, the
  `adbe.pkcs7.detached` and `ETSI.CAdES.detached` sub-filters, and the
  `RawSigner` seam beside today's `Signer`, through which a host returns a raw
  signature over the to-be-signed attributes.
- **PAdES** (SG-07.1): the baseline levels B-B, B-T, B-LT and B-LTA as
  orchestration over SG-06's `SignedData`, and the level detector's rules
  transcribed clause by clause from EN 319 142-1.

## Milestones

1. **SG-01.2, the constant-time path.** Masks, branch-free Montgomery
   reduction and fixed-window exponentiation in `bignum.rs`. Exit:
   `bignum.rs` `the_private_path_counts_alike_over_every_exponent`, the
   operation-count test above, its branch, table-read and limb-product counts
   equal over every exponent shape.
2. **SG-01.3, the vectors.** CAVP's `SigGen15` and `SigGenPSS` files and
   Wycheproof's RSA signature files under
   `crates/tinker-pdf-crypto/tests/data/`, with `THIRDPARTY.md`'s records and
   `.gitattributes` entries. Exit: `rsa_sign.rs`
   `the_signature_generation_vectors_load`, which reads every file and prints
   each one's count of keys and records.
3. **SG-01.4, the private operation.** `RsaPrivateKey`, `Secret`, the CRT
   operation, the blinding pair and the fault check. Exit: `rsa.rs`
   `crt_equals_the_plain_private_operation_on_every_vector_key`,
   `a_blinded_run_equals_an_unblinded_one`,
   `a_fault_in_a_half_is_refused_before_it_leaves` and
   `a_private_key_is_zeroised_when_dropped`, the last reading a dropped key's
   limbs through a test-only hook in `Secret`'s `Drop`.
4. **SG-01.5, RSASSA-PKCS1-v1_5.** Exit: `rsa_sign.rs`
   `cavp_pkcs1_v15_generation_vectors_pass` and
   `wycheproof_rsa_signature_vectors_pass`, for every SHA-2 digest the files
   cover.
5. **SG-01.6, RSASSA-PSS.** Exit: `rsa_sign.rs`
   `cavp_pss_generation_vectors_pass_with_their_salts`, for every SHA-2
   digest the files cover.
6. **SG-01.7, SHA-3.** Exit: `rsa_sign.rs` `the_sha3_pairs_pass`, for both
   schemes, once SG-04.1 lands.
7. **SG-02's section**, by SG-02.1: ECDSA as above. Its exits are SG-02.2 to
   SG-02.7's: field and scalar operation counts independent of operands,
   RFC 6979 A.2.5 to A.2.7 reproduced byte for byte, CAVP's and Wycheproof's
   ECDSA vectors, and the four speed targets.
8. **SG-03's section**, by SG-03.1: EdDSA. Its exits are SG-03.2 to SG-03.4's:
   RFC 8032's and Wycheproof's vectors, and signed documents verifying
   through the verdict.
9. **SG-05's section**, by SG-05.1: key import. Its exits are SG-05.2 to
   SG-05.5's: RFC 6070's PBKDF2 vectors, RFC 7292's structures, each committed
   key file held to its certificate's public key, and the `key_import` fuzz
   target.
10. **SG-06's section**, by SG-06.1: CMS `SignedData`. Its exits are SG-06.2
    to SG-06.6's: canonical DER, outputs verifying with every answer `Ok`,
    timestamps, `RawSigner` outputs byte-identical to in-engine signing, and
    RFC 5652's structures transcribed.
11. **SG-07's section**, by SG-07.1: PAdES. Its exits are SG-07.2 to
    SG-07.5's: each level's output classified by the detector, and every rule
    passing its fixture and failing its twin.

**What P-13 adds to this path.** `docs/design/crypto-throughput.md`
(P-13.1) speeds the private-key arithmetic designed here and does not
reimplement it. P-13.8 gives the path a Montgomery core at the CRT halves'
width with a dedicated squaring; its exit is SG-01's vectors passing
unchanged, `the_private_path_counts_alike_over_every_exponent` extended to the
squaring and still equal, and `bignum.rs`
`a_squaring_forms_each_cross_product_once`, which counts `n(n+1)/2` limb
products for a squaring of `n` limbs against `n²` for a product.

## Risks

| Risk | Mitigation |
| --- | --- |
| The optimiser turns a mask into a branch, or a target's code generation reintroduces one, where the source has none | Masks pass through `black_box`; the operation-count test holds the source's shape; H-67 reviews the machine code on every target, wasm's JIT among them, before signing ships |
| A zeroising store is elided as a write to memory about to be freed, without a volatile write the crates' `forbid(unsafe_code)` rules out | `black_box` over the wiped limbs and a sequentially consistent compiler fence after them; secrets on the heap so that a move copies no limb; `a_private_key_is_zeroised_when_dropped` reads the wiped limbs through a test hook; H-67 reviews what the compiler keeps |
| A blinding pair shared by threads signing with one key | The pair sits behind the key's lock; a poisoned lock is a typed refusal rather than an unblinded operation |
| A CRT fault — a miscomputed half, or a key whose parts disagree — leaks a prime through the signature | Every signature is checked with the public exponent before it leaves; key construction checks `p · q = n` and the CRT parts |
| The caller's source returns too little or predictable bytes | Too little is a refusal; predictable is the caller's, ruling 20 makes entropy theirs, and determinism never depends on it |
| The fixed window's table read costs sixteen loads a window, where a branching lookup costs one | It is the price of rule 2; P-13 measures the whole operation against its target and speeds the core rather than the rule |
| A vector file the licence rules do not admit | Ruling 18: CAVP's data is public and Wycheproof's is Apache-2.0; a file under any other licence is fetched for a local run and never committed |
