# Form data fixtures

Four files, **written by hand** on 26 September 2026 for
`crates/tinker-pdf/tests/form_data.rs`. No program wrote any of them, and
none of them was written by this repository's own FDF or XFDF writer: the
point of a hand-written fixture is that it pins the *reader* to the format
rather than to the writer beside it.

- `form-fields.fdf` and `form-fields.xfdf` carry values for the four fields of
  `testdata/form-fields.pdf` — `name`, `agree` (whose on state is `/On`),
  `colour` (a radio pair) and `notes` — so the test can import them into the
  form they were written for.
- `hierarchy.fdf` and `hierarchy.xfdf` carry the shapes the flat pair does
  not: `/Kids` and nested `<field>` two deep, a flat dotted name, a multiple
  selection, escapes, a field with no value, a field reached through an
  indirect reference, a file specification dictionary, and keys and elements
  the reader does not read, which it must name.

**What they were written from, and what they were not.** The FDF pair
follows the structure ISO 32000-1 12.7.8 describes — a `%FDF-1.2` header, one
indirect object whose `/FDF` dictionary holds `/Fields` and `/F`, a trailer
naming it as `/Root`, and no cross-reference table, which the clause makes
optional — from this author's knowledge of the clause and its example. **The
text of ISO 32000-1 was not available in the container these were written
in**, so they are not the specification's own example bytes, and a detail of
the clause they get wrong would be wrong in the reader too.

The XFDF pair uses the commonly documented core of Adobe's *XML Forms Data
Format Specification* (standardised as ISO 19444-1): the
`http://ns.adobe.com/xfdf/` namespace, `<f href>`, `<fields>`, nested
`<field name>`, repeated `<value>`, and `<ids>`, `<annots>` and
`<value-richtext>` as elements a reader may meet and not read. **Neither the
Adobe specification nor ISO 19444-1 was available** to write them from, and
`docs/features/forms.md` names that limit where it names what the reader
reads.

The same four files are four of the six seeds in `fuzz/corpus/form_data/`.
