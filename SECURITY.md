# Security

## Reporting a vulnerability

Report privately through GitHub's **Report a vulnerability** button on the
Security tab of this repository. Please do not open a public issue for
anything exploitable. Include the input file or byte sequence that triggers
the behavior if you can share it — a reproducer is worth more than a
description, and PDF bugs are almost always input-shaped.

Expect an acknowledgement within a few days. Fixes ship in a patch release
with credit unless you prefer otherwise.

## What counts

tinker-pdf parses hostile input by design: a PDF is an attacker-controlled
byte stream with an object graph, embedded programs, compression bombs and
thirty years of malformed producers behind it. So the following are all
security issues here, not merely bugs:

- **Any panic on untrusted input.** Ruling 1 of the project's consistency
  rules makes never-panicking a contract, and the per-format fuzzers enforce
  it. A crash reachable from document bytes is a denial of service in every
  embedder.
- **Unbounded memory or time** from a small input — decompression bombs, xref
  or object-stream recursion, hostile `/Size`, degenerate shadings. Every
  limit that exists is a named constant; a path that escapes those limits is
  a defect.
- **Incorrect security semantics**: reporting a document as unrestricted when
  its `/P` restricts, accepting a wrong password, confusing owner
  authentication with user authentication, or decrypting with a key derived
  incorrectly.
- **Reading outside the document buffer**, in any form.

The engine contains no `unsafe` in its parsing and rendering crates, so
memory-safety issues should be impossible by construction. If you find one,
that is a very interesting report.

## The hand-rolled crypto, stated plainly

This project implements MD5, RC4, SHA-2, AES-CBC and DES-EDE3-CBC itself, along
with the PDF standard security handler and — for signature verification —
big-integer modular arithmetic, RSASSA-PKCS1-v1_5 and ECDSA over P-256 and
P-384, because
the project's design mandate is that no third-party crate implements engine
functionality. That is an unusual choice and it deserves an unusual amount of
scrutiny, so here is the honest framing:

- **Scope is document decryption, encryption-on-save, and signature
  verification.** No TLS, no key exchange, no long-term secret storage, no
  protocol implementation.
- **No private-key operation exists.** The signature code verifies; a
  signature written on save is the CMS blob a caller-supplied `Signer`
  returns over the digest. There is no key generation and no PKCS#8 or
  PKCS#12 parsing. That is what lets `bignum` skip constant-time discipline —
  every value it handles is published in the document being checked. Signing
  with a private key in the engine is [ROADMAP](docs/ROADMAP.md) rows SG-01
  to SG-09.
- **Correctness is checked against published vectors** — RFC 1321 for MD5,
  RFC 6229 for RC4, worked examples for SHA-2, and known-answer tests for AES.

  Stated precisely: for *most* of the cipher and hash primitives the full
  NIST CAVP suites are **not** wired in, and there is no CBC-AES-256 vector
  even though that is the mode the fixtures use. AES-256 in CBC is exercised
  end to end by decrypting a mutool-written R6 fixture, which is real
  evidence and is not the same thing as CAVP. Committing the AES and SHA-256
  suites is [ROADMAP](docs/ROADMAP.md) row P-13.

  **Triple DES is the one cipher that is gated on CAVP**: its known answers
  are committed files, and the FIPS 46-3 tables are read twice.

  The *signature* primitives are the other exception, and they are gated on
  CAVP itself: `SigVer15` vectors for RSA (moduli of 1024 to 4096 bits
  crossed with SHA-1/256/384/512, valid ones and ones that must be refused,
  forged paddings among them), `SigVer` vectors and `PKV` points for ECDSA,
  and RFC 6979's appendix A.2 signatures. The vector files are committed
  under `crates/tinker-pdf-crypto/tests/data/cavp/` with their provenance in
  each file's header.
- **Password comparison is constant-time.**

  Decrypt paths **are** fuzzed: `crypt` drives the security handler and
  `crypt_ciphers` the ciphers themselves, both with committed seed corpora.
  The `cos_document` target additionally reaches the decryptor for inputs
  that carry an `/Encrypt` dictionary, and the stable hostile-input sweep
  authenticates with several passwords against every mutated fixture.
- **PDF encryption is weak by design in its older revisions** (RC4-40 exists
  because the spec has it, not because it protects anything), and the format's
  permission flags are advisory — a document that says "printing denied" is
  asking, not enforcing. Nothing this engine does changes that, and no
  embedder should treat PDF permissions as a security boundary.
- **Triple DES is here to read old documents and nothing else.** Its 64-bit
  block is Sweet32's whole premise and two-key EDE is below 112 bits of
  security. It exists because `openssl cms -encrypt` still chooses
  `des-ede3-cbc` by default for older recipients, so public-key-encrypted PDFs
  in the wild carry it. The module decrypts only: there is no way to reach a
  DES *encryption* from any public API, and nothing in this engine will ever
  write such an envelope.

Review of the crypto crate is actively welcomed, and a finding there is worth
reporting even if you cannot demonstrate an exploit.

## Supported versions

Pre-1.0: fixes land on `main` and in the next release. There are no
maintained release branches yet.
