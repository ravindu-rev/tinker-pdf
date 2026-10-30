//! The sign group: the RSA-2048 private-key operation, a signature verified
//! over 10 MB, and SHA-256 of the same 10 MB.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `sign` list names these operations
//! in this order. The key is `pss-vect.txt`'s Example 10 and the signature is
//! made here with it, so the setup asserts the signature verifies before
//! anything is timed.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf_crypto::rsa::{DigestAlgorithm, RsaPublicKey};
use tinker_pdf_crypto::sha2::sha256;

fn sign(c: &mut Criterion) {
    let key = LazyCell::new(inputs::example_10);
    let public = LazyCell::new(|| {
        let public =
            RsaPublicKey::new(&key.modulus, &key.public_exponent).expect("Example 10's public key");
        assert_eq!(public.modulus_bits(), 2048, "Example 10 is a 2048-bit key");
        public
    });
    let signed = LazyCell::new(|| {
        let (message, signature) = inputs::message();
        assert_eq!(message.len(), inputs::MESSAGE_BYTES);
        assert!(
            public
                .verify_pkcs1_v15_message(DigestAlgorithm::Sha256, &message, &signature)
                .is_ok(),
            "the signature made with d verifies under e"
        );
        (message, signature)
    });

    let mut group = Group::new(c, "sign");

    // First, because it has no unit: a throughput set on a group stays on it
    // for every operation after, and a private-key operation processes no
    // pages, pixels or bytes.
    group.operation("RSA-2048 private-key operation", |op| {
        let (message, _) = &*signed;
        let (modulus, d, representative) =
            inputs::private_operands(&key, &inputs::pkcs1_encoded(&key, message));
        op.iter(|| {
            for _ in 0..13 {
                black_box(modulus.pow(black_box(&representative), &d));
            }
        });
    });
    group.operation("verify an RSA-2048 signature over 10 MB", |op| {
        let (message, signature) = &*signed;
        op.per(Per::Bytes(message.len() as u64));
        op.iter(|| {
            black_box(public.verify_pkcs1_v15_message(DigestAlgorithm::Sha256, message, signature))
        });
    });
    group.operation("SHA-256 of 10 MB", |op| {
        let (message, _) = &*signed;
        op.per(Per::Bytes(message.len() as u64));
        op.iter(|| black_box(sha256(message)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::fine_config();
    targets = sign
}
criterion_main!(benches);
