# Linearization is dropped when encryption is on

Ask for both and you get an encrypted file with an ordinary layout, no error
and no warning. The option documents this as one of three cases where
linearization is quietly dropped, which is honest — but it is the only one of
the three that is a limitation rather than a definition. When this is done,
an encrypted file can also be linearized. (M)

## What is wrong

`rewrite` guards the linearized path with `options.encryption.is_none()`, so
encryption wins and the layout request is discarded. Both halves work
independently: `linearize` emits a complete Annex F file, and encrypt-on-save
round-trips through the engine's own reader.

## The crux

The linearized writer's whole design is that **no size depends on an offset**.
That is what lets it serialise every object, measure, derive every offset, and
emit once — with no patching pass, and therefore no chance of a patch missing
a field.

Encryption breaks the premise. AES-256-CBC pads to the block size and prefixes
a 16-byte initialisation vector, so an encrypted stream is longer than its
plaintext by an amount that depends on the plaintext's length. Encrypt after
measuring and every offset is wrong; measure after encrypting and the
measurement is of the right thing.

So the resolution is simply **encrypt first, then measure**. The object bytes
are serialised and encrypted in one pass, and the layout is computed from the
encrypted lengths. Nothing else about the design changes, and the no-patching
property survives.

Three objects stay in the clear (7.6.1), and each is already handled somewhere
in the writer:

- The `/Encrypt` dictionary itself, since a reader needs it before it can
  decrypt anything.
- Both cross-reference tables — a reader finds them before it knows there is
  an `/Encrypt` dictionary to look for.
- The linearization parameter dictionary. It is a plain dictionary object and
  7.6.1 does not exempt it, but a reader consults it *before* authenticating.
  This is the one genuinely awkward point: strings inside it would be
  encrypted, and it contains none — every value is an integer or an array of
  integers, so there is nothing to encrypt and the question is moot. Assert
  that rather than assuming it.

The hint stream **is** encrypted. It is an ordinary stream object; `/H` gives
its offset and length in the file, which is the encrypted length.

## Scope

- Remove the guard; thread the cipher into the linearized writer.
- Encrypt object bytes during serialisation, before lengths are taken.
- Keep the parameter dictionary, both tables and the `/Encrypt` dictionary in
  the clear.
- Encrypt the hint stream, and measure `/H` from the encrypted bytes.
- Per-object initialisation vectors, as the ordinary writer already does.

## Non-goals

- **Encrypting an incremental update.** Structurally impossible without the
  original file's key, and already reported rather than pretended.
- **A new encryption handler.** R6 is what the writer emits.

## Where a half-implementation is worse than none

Measuring before encrypting. Every offset after the first stream would be
wrong by the accumulated padding, and the file would open in this engine —
whose reader walks subsection headers — while failing in any reader that
trusts `/L`, `/H` or `/T`. Which is precisely the population the feature
exists for.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Cipher threaded in; objects encrypted before measuring | An encrypted linearized file opens, authenticates and reads its pages | S |
| 2 | Clear-text objects kept clear | The parameter dictionary and both tables are readable in the raw bytes; `/Encrypt` is too | S |
| 3 | Offsets verified against the encrypted bytes | `/L` equals the file length; `/H` points at the hint stream; `/E` and `/T` are ordered — the same assertions the unencrypted tests make, on an encrypted file | S |
| 4 | The parameter dictionary carries no strings | Asserted, so a future field that does carry one fails rather than leaking | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** nothing. This has **no parity value** — nothing in Tinker's
eleven-function seam writes a file. It ranks where it does only because the
option currently promises something it does not do.

## Risks

| Risk | Mitigation |
| --- | --- |
| An encrypted file whose offsets are wrong opens here and fails elsewhere | Milestone 3 asserts the offsets against the bytes, exactly as the unencrypted tests do; [20](20-linearization-validation.md) is what would catch the rest |
| Padding makes the layout non-deterministic if the IV is random | The entropy is caller-supplied and already deterministic in tests; the same fixture must produce the same file twice |
