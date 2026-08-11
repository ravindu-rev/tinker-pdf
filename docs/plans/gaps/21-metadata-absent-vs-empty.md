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
