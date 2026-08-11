# A blank title reads as no title

`/Title ()` and no `/Title` at all produce the same answer: `None`. A user
interface cannot tell "the producer wrote an empty title" from "there is no
title", and the distinction is unrecoverable once it reaches a caller. The
struct's own doc comment says these are different facts. When this is done,
they are. (S)

## What is wrong

`crates/tinker-pdf-cos/src/outline.rs`, in `metadata`:

```rust
let text = value.as_string().map(|s| decode_text_string(&s.bytes))?;
// Absent beats empty once this reaches a caller.
(!text.trim().is_empty()).then_some(text)
```

The last line is the rule, and it is backwards. A field that is present and
empty — or present and entirely whitespace — becomes `None`.

`docs/plans/04-document-semantics.md` is explicit about the opposite:

> Absent-not-empty is a contract: a key missing from `/Info` (or `/Info`
> missing entirely) is `None`; **a writer that emitted an empty string gets
> `Some("")`**. Tinker's UI distinguishes "no title" from "blank title", and
> collapsing the two on read makes the distinction unrecoverable.

And the `Metadata` struct's own doc comment, twenty lines above the offending
closure, says the same thing: "a blank title and no title are different
facts, and only one of them should reach a user interface."

So the code contradicts its own documentation twice, and the comment on the
line itself asserts the wrong rule as though it were the intended one.

## Scope

- Return `Some(text)` whenever the key is present and holds a string,
  whatever the string contains.
- Keep `None` for: key absent, `/Info` absent, or a value that is not a
  string.
- Fix the comment, which currently states the wrong rule.
- Same treatment for every field the closure serves — title, author, subject,
  keywords, creator, producer.

## Non-goals

- **Trimming whitespace.** The value is returned untrimmed today and should
  stay that way; a title with deliberate leading spaces is the producer's
  choice.
- **`/Trapped`.** [22](22-pdf-version-and-trapped.md), because it is a name
  rather than a string and this closure cannot carry it.

## Design

Two lines: drop the emptiness test, correct the comment. The whole difficulty
is that it looks like a deliberate nicety and reads as one — which is why it
survived. The test is what makes it stay fixed.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | The rule inverted, comment corrected | `/Title ()` gives `Some("")`; `/Title (   )` gives `Some("   ")`; no `/Title` gives `None`; no `/Info` gives `None`; `/Title /NotAString` gives `None` | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** a Tinker UI that can show "(untitled)" for a missing title and
an empty field for a blank one — which is what plan 04 says the distinction is
for.

## Risks

| Risk | Mitigation |
| --- | --- |
| A caller somewhere treats `Some` as "has a useful title" and now shows an empty string where it used to show a fallback | That is the intended change, and it is the caller's decision to make. Search the facade and the bindings for consumers before landing |
| The same collapse exists elsewhere for other fields | The closure serves all six `/Info` string fields, so one fix covers them; check the XMP path separately |

## As built

*August 2026.* Milestone 1 is done, and the two lines the Design predicted are
the two lines that changed. Five things the plan did not say:

1. **The closure serves eight fields, not six.** `/CreationDate` and
   `/ModDate` go through it too — they are text strings held raw for the
   parity tests, and `Metadata::created` parses them on demand. So a document
   with `/ModDate ()` used to claim it had never been modified. The date
   parser returns `None` for an empty string either way, which is why nothing
   downstream breaks and why nothing downstream noticed.

2. **Nothing consumes it that had to change.** `tinker-pdf` re-exports
   `Metadata` and `Document::metadata` verbatim (ruling 11); the C ABI, Python,
   JS and .NET bindings expose no metadata surface at all, so there was nothing
   to project the new answer through. The one live consumer is `tpdf info`,
   which prints a field only when it is `Some` — it now prints a label with
   nothing after it for a blank field, which is the honest rendering and
   exactly the change the Risks table anticipated.

3. **The XMP path was already right.** `xmp_metadata` returns
   `stream_decoded(...).ok()`, so a zero-length `/Metadata` stream was always
   `Some(&[])` and only a catalog naming no stream was `None`. Checked, not
   assumed, and now pinned by a test — an archival profile that requires the
   stream to exist cares which of the two it has.

4. **The `Metadata` doc comment was ambiguous, not wrong.** "Absent rather
   than empty" reads as a description of either rule depending on what you
   already believe. It now names both answers: `None` for a missing key,
   `Some("")` for a key holding `()`.

5. **A test for this already existed and passed throughout.**
   `metadata_reports_absent_rather_than_empty` asserted "a present field is
   never blank" over the title, author, subject and keywords of
   `simple-text.pdf` — whose `/Info` is `<< /Producer (MuPDF 1.27.2) >>` and
   nothing else. The loop body never executed. It asserted the inverted rule,
   it could not have failed, and it is renamed to
   `metadata_reports_absent_for_keys_the_file_omits` and now checks those four
   fields are `None`, which is the half of the contract a MuPDF fixture can
   pin. The other half needs a document no producer writes, so the new tests
   are hand-built byte by byte beside the code in `outline.rs`: `/Title ()` →
   `Some("")`, `/Title (   )` → `Some("   ")`, `/Title <FEFF>` → `Some("")`
   after 7.9.2.2 decoding, all eight fields together, `/Title` as a name, an
   integer, a null and an array → `None`, and no `/Info` at all → every field
   `None`.

`docs/plans/04-document-semantics.md` needed no amendment: its absent-not-empty
paragraph is what the code now does. Its `Info` sketch still differs from the
shipped `Metadata` in the `trapped` field and in two names, which
[22](22-pdf-version-and-trapped.md) owns and reconciles.

Seven tests added, 971 → 978. Whitespace is still untrimmed and `/Trapped` is
still absent, both deliberately.
