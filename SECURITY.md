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
- **Correctness is gated on published vectors**: NIST CAVP for AES and SHA-2,
  RFC 6229 for RC4, RFC 1321 for MD5. These are merge gates, not aspirations —
  the implementations do not land without them passing.
- **Password comparison is constant-time**; decrypt paths are fuzzed.
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
