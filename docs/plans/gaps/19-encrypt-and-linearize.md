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

## As built — 15 August 2026

Four commits, one per milestone. 1 304 tests to 1 316; the full gate green on
each, plus the wasm determinism leg, and none of the nine rendering
fingerprints moved on either target — this changes the writer, not the
rasteriser.

The defect was live and exactly as described. Asking for both produced a file
with `/Encrypt` and no `/Linearized`, no error and no warning; asking for
either alone worked.

**The plan's resolution is the whole design and it needed nothing else.**
Objects are serialised and encrypted in one pass in `Plan::build`, `/Length`
comes from the ciphertext, and every offset is derived from those lengths. The
no-patching property survives untouched. What the plan does not say is that
plan 09 claimed the writer patches — "two-pass with patch-after-layout" — and
it never has; that paragraph is now amended, because the encryption argument
only makes sense against the design that is actually there.

**Three things the plan did not anticipate.**

*The `/Encrypt` dictionary's object number is load-bearing.* The unlinearized
writer numbers it directly above the object-stream container, and copying that
here would have produced a file that opens nowhere. The first-page
cross-reference table declares a single subsection running from zero to its
highest entry and marks every number in that range it does not carry as
**free**; a free entry in the newer table overrides the main table reached
through `/Prev`. A high `/Encrypt` number stretches the front table's range
across the whole file and frees everything behind it. `ENCRYPT_OBJECT` is 3,
beside the parameter dictionary and the hint stream, and the dictionary leads
part 4 — which is also where a reader wants it, since it is the first thing
needed and this layout exists to put the first thing needed first.

*The clear-text set is larger than 7.6.1's.* 7.6.1 exempts cross-reference
*streams*; a linearized file has two **classic** tables and the same reasoning
covers them. And the parameter dictionary is outside the specification's
exemptions entirely — it is clear because a reader consults it before
authenticating, which is only sound because it holds no strings. Milestone 4
asserts that from both sides: that part 2 contains no string tokens, and that
part 2's bytes are identical whether a cipher exists or not. Plans 03 and 09
are amended with both.

*A `/H` measured from the plaintext was caught by nothing.* The existing test
read only `/H`'s first element and checked that an indirect object began there,
which is true whichever length is written. The assertion now measures to
`offset + length` and requires `endobj`. Injecting that defect failed that one
assertion and 1 306 other tests passed.

**How encrypt-then-measure was proved, rather than asserted.** The fixture has
eight streams and every one of them is at least seventeen bytes longer
encrypted — sixteen for the initialisation vector and at least one for padding,
which CBC never leaves empty — asserted as a number, along with the stream
count and a total growth over 150 bytes, so the fixture cannot quietly shrink
below the point where a drift would show. Then measuring-before-encrypting was
injected faithfully, by serialising each object twice and writing the
ciphertext while measuring the plaintext. `/L` came out 3 072 against a real
length of 3 288. Three tests caught it — and **the round trip did not**, which
is the plan's point stated as a measurement: this engine's reader walks the
subsection headers and recovers. qpdf does not, and says so.

**qpdf, run although it is gap 20's job formally.** On the correct output:
`R = 6`, `AESv3` for streams, strings and the file, `Supplied password is user
password`, `File is linearized`. `qpdf --decrypt` produces a copy that
`qpdf --check` calls free of syntax and stream encoding errors, which means
every object offset resolved and every stream decrypted. On the
measure-before-encrypt build the same command says `file is damaged`,
`xref not found`, `Attempting to reconstruct cross-reference table` and
`File is not linearized`.

### What gap 20 should point qpdf at, and what it will find

Two things, and neither is an encryption defect.

**The hint tables do not parse.** `qpdf --show-linearization` fails with
`overflow reading bit stream: wanted = 1; available = 0` on the linearized
output — **identically with and without encryption**, which is how it is known
to be a hint-table defect rather than an offset one. `--check` reports it as
`error encountered while checking linearization data` and otherwise passes.
This is gap 20's milestone 1 and 3, precisely: the writer emits the tables and
nothing in this repository reads them. Milestone 4 should use the encrypted
fixture from `crates/tinker-pdf-cos/tests/linearized.rs`
(`encrypted_linearized(6)`, password `open-me`, the deterministic
`entropy()`), and should expect the *same* error on it as on the unencrypted
one — if it differs, that is an encryption defect and this gap's.

**No encrypted file carries an `/ID`.** 7.5.5 Table 15 requires one whenever
`/Encrypt` is present. Nothing in this engine writes one on any path, so qpdf
reports `invalid /ID in trailer dictionary` on every encrypted file it writes,
linearized or not — this is a defect of encrypt-on-save, found here only
because gap 19 was the first thing to point qpdf at an encrypted output. R6
does not mix `/ID` into key derivation, so nothing fails to decrypt, and it is
recorded in `audit-2026-08.md` rather than fixed here: all 48 entropy bytes are
already spoken for and choosing how to derive an identifier is a design
decision, not a patch.

### Deliberately not done

- **Encrypting an incremental update**, per the non-goals: structurally
  impossible without the original file's key, and already refused rather than
  pretended.
- **A new encryption handler.** R6 is what the writer emits.
- **Strings inside a stream's *dictionary* are not encrypted**, on either
  writer path. The reader decrypts them (`decrypt_strings` walks
  `Object::Stream`'s dictionary), so the two disagree. It is pre-existing,
  it is unreachable for anything this engine currently writes — no stream
  dictionary it emits carries a string — and the linearized writer was made to
  mirror `write_entry` exactly rather than diverge from it. Worth a row
  somewhere; it is not this gap's to fix quietly.
