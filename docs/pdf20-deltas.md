# PDF 2.0 deltas

ISO 32000-2 items tracked against the 1.7 baseline as they become relevant.
Full 2.0 conformance is not a current goal ([architecture.md](architecture.md)).

| Delta | Status | Doc |
| --- | --- | --- |
| AES-256 / R6 encryption (was Adobe extension, core in 2.0) | Committed day one | [encryption](features/encryption.md) |
| R5 deprecated | Read-only with warning | [encryption](features/encryption.md) |
| UTF-8 string type | **Read.** `decode_text_string` takes 7.9.2.2's `EF BB BF` as UTF-8, on ten live call sites; the writer still emits UTF-16BE only | [opening](features/opening.md) |
| 2.0 blend/dash clarifications | Tracked | [rendering](features/rendering.md) |
| Deprecated features (XFA removed in 2.0, /ProcSet, ...) | Noted per doc; XFA is a named permanent non-goal ([ROADMAP.md](ROADMAP.md)) | — |
