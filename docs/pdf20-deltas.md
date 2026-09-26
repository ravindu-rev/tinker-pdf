# PDF 2.0 deltas

ISO 32000-2 items tracked against the 1.7 baseline as they become relevant.
Full 2.0 conformance is not a current goal ([architecture.md](architecture.md)).

| Delta | Status | Doc |
| --- | --- | --- |
| AES-256 / R6 encryption (was Adobe extension, core in 2.0) | Committed day one | [encryption](features/encryption.md) |
| R5 deprecated | Read-only with warning | [encryption](features/encryption.md) |
| UTF-8 string type | **Read.** `decode_text_string` takes 7.9.2.2's `EF BB BF` as UTF-8, on ten live call sites; the writer still emits UTF-16BE only | [opening](features/opening.md) |
| 2.0 blend/dash clarifications | **Tracked, and not readable here.** What ISO 32000-2 changed in 11.3.5 and 8.4.3.6 against 1.7 needs the 2.0 text, and on 26 September 2026 none of it could be read: the PDF Association's sponsored copy (`pdfa.org/sponsored-standards/`) was refused by this environment's egress policy with a 403, and a refused host is not routed around. Two first-party PDF Association sources were reachable and say less than the question needs. **Its errata** (`github.com/pdf-association/pdf-issues`, `docs/32000-2-2020/`, at `b25fc23`, 16 September 2026) carry nothing under 8.4.3.6, and under clause 11 only a NOTE that Figures 72 and 73 are indicative (issue 345), a symbol fix in 11.4.8 (688), `/ID` and `/OPI` deprecated in Table 143 (619) and Table 145's `/CS` defaults resolved in the group XObject's own resources (134); they quote only the text an erratum touches, by ISO copyright. **The Arlington model** (`github.com/pdf-association/arlington-pdf-model`, `tsv/latest`, at `c48b363`, 17 September 2026) records the array form of `/BM` as deprecated in 2.0 and `/Compatible` since 1.4 — both of which this engine already reads — and nothing about dash semantics. Nothing was implemented from memory. One thing the errata's quotation does say is recorded in the [ROADMAP](ROADMAP.md) row rather than acted on: Table 145's `/CS` text excludes Lab from a transparency group's colour space, so the `/Lab` group [rendering](features/rendering.md) composites in Lab is a file the specification does not permit, and what a reader does with one is a decision | [rendering](features/rendering.md) |
| Deprecated features (XFA removed in 2.0, /ProcSet, ...) | Noted per doc; XFA is a named permanent non-goal ([ROADMAP.md](ROADMAP.md)) | — |
