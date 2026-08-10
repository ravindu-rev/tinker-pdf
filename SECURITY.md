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

This project implements MD5, RC4, SHA-2 and AES-CBC itself, along with the PDF
standard security handler, because the project's design mandate is that no
third-party crate implements engine functionality. That is an unusual choice
and it deserves an unusual amount of scrutiny, so here is the honest framing:

- **Scope is document decryption and encryption-on-save.** No TLS, no key
  exchange, no long-term secret storage, no protocol implementation.
- **Correctness is checked against published vectors** — RFC 1321 for MD5,
  RFC 6229 for RC4, worked examples for SHA-2, and known-answer tests for AES.

  Stated precisely, because the previous wording here claimed more than was
  true: the full NIST CAVP suites are **not** wired in, and there is no
  CBC-AES-256 vector even though that is the mode the fixtures use. AES-256 in
  CBC is exercised end to end by decrypting a MuPDF-produced R6 file and by an
  independent Python reimplementation of Algorithm 2.B in the test suite, which
  is real evidence and is not the same thing as CAVP. Wiring the suites is
  tracked in [plan 03](docs/plans/03-encryption.md).
- **Password comparison is constant-time.**

  Decrypt paths are **not** fuzzed today. A `tinker-pdf-crypto` fuzz target
  does not exist, and the `cos_document` target reaches the decryptor only for
  inputs that happen to carry an `/Encrypt` dictionary. The stable hostile-input
  sweep authenticates with several passwords against every mutated fixture,
  which exercises the failure paths but is far shallower than a fuzzer.
- **PDF encryption is weak by design in its older revisions** (RC4-40 exists
  because the spec has it, not because it protects anything), and the format's
  permission flags are advisory — a document that says "printing denied" is
  asking, not enforcing. Nothing this engine does changes that, and no
  embedder should treat PDF permissions as a security boundary.

Review of the crypto crate is actively welcomed, and a finding there is worth
reporting even if you cannot demonstrate an exploit.

## Supported versions

Pre-1.0: fixes land on `main` and in the next release. There are no
maintained release branches yet.
