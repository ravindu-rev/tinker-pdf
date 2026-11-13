# System fonts

A document that embeds no program for a font is drawn with a face the host
supplies through `FontProvider`, or with a face the build bundles; the engine
reads no font directory (`docs/features/fonts.md`). This document describes a
provider that answers from the faces a machine already has installed: the
crate it lives in, `tools/sysfonts`, outside the engine crates; the pure
policy that picks one face for a request; the directories it reads on each
operating system; how it describes a face through the facade; and how the CLI
and the native bindings take it (ROADMAP FT-13). The owner allowed it (H-24):
a host provider in `tools/sysfonts`, outside the engine crates, which ruling
11 admits.

## Scope

- **The crate**: `tools/sysfonts`, package `tinker-pdf-sysfonts`, a library
  depending on the facade alone, with its edges declared to `cargo xtask dag`
  and its place in the published graph.
- **The policy**, pure: name normalisation (subset prefixes, `,Bold`,
  `-BoldMT` and their kin), the family and style match, PANOSE and
  `sFamilyClass` scoring, the CJK defaults by a composite font's character
  collection, and the per-script and emoji chains; its inputs and its one
  answer, a face or none.
- **The provider**: the font directories it reads on each operating system,
  or the roots a caller names; what it caches; what it skips.
- **A face described through the facade alone**: what the facade hands back
  about each face of a font file, a collection's members included, and a
  member written out as a face of its own.
- **Its surfaces**: `tpdf --fonts system`, and one call in the C ABI that
  each native binding projects. How ruling 11 reads for it.

## Non-goals

- **The wasm binding.** `wasm32-unknown-unknown` has no file system and no
  font directory (NG-08); the JavaScript binding keeps `set_fonts`, the
  host's own faces, and gains no call here.
- **A platform's font service.** No fontconfig configuration (`fonts.conf`,
  its aliases and rules), no registry substitution table, no Core Text or
  DirectWrite query: each is a C interface, every crate here forbids
  `unsafe`, and the policy is the repository's own, held to its table. The
  provider reads files.
- **Fonts a machine does not hold**: on-demand downloads, a network source,
  a font a document names by URL. The provider never reads a document; it
  reads font files the host already has.
- **A persistent index.** What the provider reads it keeps in memory for its
  own life; nothing is written to disk, and a face installed after the
  provider first answers is not seen until a new one is made.
- **Embedding a system face.** The provider answers the renderer's request
  for a face to draw an unembedded font with. No producing path consults a
  provider — a book's pagination and drawing do not depend on one
  (`epub/paint.rs`) — so no system face is written into a document, and
  `OS/2` `fsType`'s embedding permissions do not bind what it hands back.
- **Metric-matched substitution.** Drawing `Helvetica` with a face built to
  its metrics is FT-14's, behind H-147's switch; this policy matches what a
  face says about itself.
- **A style the faces lack.** A bold or italic request with no such face is
  answered with the family's nearest face, as `SimpleFontProvider` fills a
  missing style; nothing is emboldened or slanted here.
- **How a substituted face is read.** `docs/design/fallback-faces.md`'s
  routes stand: the provider chooses a face, and the engine reads it as it
  reads any host's.

## Design

### The crate and its edges

`tools/sysfonts` is a library beside `tools/tpdf` and `tools/pdfcmp`, under
`#![forbid(unsafe_code)]`, its package `tinker-pdf-sysfonts` with
`version.workspace = true`. It holds two modules: `policy`, pure, and
`provider`, which reads the file system.

**The edges `cargo xtask dag` declares.** The crate joins `xtask`'s `TOOLS`
table as `("tools/sysfonts", &["tinker-pdf"])`: like every tool it depends
on the facade alone, so it describes a face through what a user of the facade
gets and never reaches past it into `tinker-pdf-font`. Two crates gain an
edge to it, each declared where it lands: `tools/tpdf`'s `TOOLS` row
(FT-13.4), for `--fonts system`, and `tinker-pdf-ffi`'s row in the crates'
table (FT-13.5), for the C ABI's call. Nothing under `crates/` but the C ABI
reaches it, so the engine crates stay free of I/O.

**Published.** The C ABI is in `cargo xtask release`'s published graph, and
a published crate depends only on published crates, so `tinker-pdf-sysfonts`
is published too: it takes a `[workspace.dependencies]` entry beside the
engine crates', is no row of `NEVER_PUBLISHED`, and stands after
`tinker-pdf` and before `tinker-pdf-ffi` in the publish order.
`cargo xtask versions` holds its version to the workspace's (FT-13.3).

**Ruling 11, for a crate beside the facade.** Ruling 11 has the bindings and
the C ABI project `tinker-pdf` 1:1, with no logic, caching or default of their
own, and names one other crate a binding may project the same way:
`tools/sysfonts` (ruling 23, H-24). So the C ABI gains exactly one call, which
constructs the crate's provider and installs it on a document, and each
binding projects that call as it projects `tpdf_document_set_fonts`. Which
directories are read, the order of evidence, what is cached and what is
skipped are the crate's and are decided once, there; a binding passes the
roots its caller names and nothing else. A behaviour a binding wants grows in
the crate first.

### A face, described through the facade

The provider parses nothing itself. The facade grows a pure description of
a font file (FT-13.2): bytes in, values out, no path, as every facade door
takes bytes.

- **`describe_faces(bytes)`** gives one description for each face the file
  holds: the one face of an sfnt, each member of a TrueType or OpenType
  collection, a WOFF or WOFF2 unpacked first. Each carries its member index;
  its family, subfamily, full and PostScript names, the typographic family
  and subfamily (`name` IDs 16 and 17) preferred where the face has them;
  `OS/2`'s `usWeightClass`, PANOSE, `sFamilyClass`, code-page and Unicode
  ranges; whether it is italic (`fsSelection` bit 0, or `head`'s `macStyle`
  where `OS/2` is absent) and fixed-pitch (`post`'s `isFixedPitch`, or PANOSE
  proportion 9); and the characters it **draws**, as ranges: those its
  `cmap` maps to a glyph other than `.notdef` that has an outline in `glyf`,
  `CFF ` or `CFF2`. Drawing is what the engine does with a substituted face
  — it never draws a substituted face's colour tables, as it never draws an
  embedded one's (`docs/design/colour-fonts.md`) — so a face whose emoji
  live only in `sbix` or `CBDT` draws none of them. A table that is absent or
  will not read gives no value for what it holds, never a guess, and a face
  whose table directory will not read gives no description.
- **`face_of_member(bytes, n)`** writes member `n` of a collection out as a
  face of its own — its table directory and the tables it names — so the
  engine, whose `Sfnt::parse` takes a collection's first member as a
  document's embedded collection needs, reads the member chosen. `Sfnt` gains
  a parse of member `n` for both.

### The policy

`policy::resolve(query, faces)` answers one index into `faces`, the
descriptions the provider holds, or none. It reads no file, clock or
environment, so its table (FT-13.3) runs on every target `cargo test` runs on
and answers alike on each.

**Its inputs.** The query is what the engine's `FontRequest` says about the
font, and nothing the provider guesses: the base font's name; the descriptor's
bold, italic, serif, fixed-pitch and symbolic flags; a composite font's
character collection (`/CIDSystemInfo`, already in the request); its PANOSE
and family class, which FT-12 adds to the request; and the Unicode scripts its
mapped text covers, which FT-13.6 adds, with whether that text holds emoji.
Each face is a description above, and the faces are ordered by normalised
family, then path, then member, so that a tie breaks the same way on every
run and every machine.

**Name normalisation.** A requested name is reduced to a family and a style:

1. A subset prefix, six capital letters and `+` (9.6.4), is dropped.
2. A style after a comma — `,Bold`, `,Italic`, `,BoldItalic`, `,Regular` —
   sets the style and is dropped, as Windows producers write `Arial,Bold`.
3. A vendor suffix — `MT`, `PS`, `PSMT` — is dropped where it ends the name
   or the part before a hyphen: `ArialMT`, `Arial-BoldMT`,
   `TimesNewRomanPSMT`.
4. A style after a hyphen — `-Bold`, `-Italic`, `-BoldItalic`, `-Oblique`,
   `-BoldOblique`, `-Roman`, `-Regular`, and the weights `-Thin` to
   `-Black` — sets the style and the weight and is dropped.
5. What remains is compared with each face's family, full name and
   PostScript name with spaces, hyphens and underscores removed and ASCII case
   folded, so `TimesNewRoman` is `Times New Roman`. The flags set the style
   where the name says none.

**The order of evidence**, decided once. The first step that yields a face
answers, and a step yields only faces that draw the request's characters
where FT-13.6 has said which they are:

1. **The family the name names**, then the style within it: the face whose
   weight and slope are the request's, else the nearest weight of the
   request's slope, else the family's regular face.
2. **A composite font's collection** (`docs/design/fallback-faces.md`): the
   collection's default families below for the request's serif flag, in their
   order, and then any face whose `OS/2` code-page range names the
   collection's region; each matched for style as in step 1. A face for the
   wrong region draws the wrong forms of a shared character, so a face is
   never taken for another collection's region.
3. **The script chain**: for each script the text covers, that script's
   families in their order, and for emoji the emoji chain; then any face that
   draws every character the request names.
4. **Likeness**: every face but ornamental, script and symbol ones
   (`sFamilyClass` 9, 10 and 12, which only step 1 takes), ranked by the
   PANOSE distance where the request has a PANOSE — the digits that differ,
   family kind, serif style, weight and proportion counting double, a digit of
   0 or 1 (*any*, *no fit*) differing from none — and otherwise by
   `sFamilyClass` against the serif flag, classes 1 to 5 and 7 serif and 8
   sans serif; then by fixed pitch, weight and slope.
5. **None.** A symbolic simple font is answered only at step 1, since a text
   face drawn by a symbol font's codes shows the wrong glyphs; and a request
   no step answers is declined, which leaves the page reporting
   `UnreadableFont` as it would with no provider.

**A composite request is answered only with a face the engine selects
correctly under its collection.** Ruling 23's answer to FT-01.3 reads a
substituted face for a composite font through its registry collection or its
`/ToUnicode`, never by CID as a glyph index. So a request in one of the
registry's five collections — Adobe-Japan1, -GB1, -CNS1, -Korea1 and -KR — is
answered with a face of that collection's region, which the inverted CMaps
reach; a request in `Adobe-Identity` or a collection the registry does not
hold is answered only where its mapped text names its characters, by step 1,
3 or 4, since `/ToUnicode` is then the engine's one route; and one with
neither is declined, as the engine would draw `.notdef` whatever face it was
handed.

**The CJK defaults**, the policy's first table, by collection and the serif
flag, each family taken where a machine has it:

| Collection | Sans serif | Serif |
| --- | --- | --- |
| Adobe-Japan1 | Noto Sans CJK JP, Source Han Sans, Yu Gothic, Hiragino Sans, Hiragino Kaku Gothic ProN, MS Gothic, IPAexGothic, IPAGothic | Noto Serif CJK JP, Source Han Serif, Yu Mincho, Hiragino Mincho ProN, MS Mincho, IPAexMincho, IPAMincho |
| Adobe-GB1 | Noto Sans CJK SC, Source Han Sans SC, Microsoft YaHei, PingFang SC, SimHei, WenQuanYi Zen Hei | Noto Serif CJK SC, Source Han Serif SC, SimSun, Songti SC, AR PL UMing CN |
| Adobe-CNS1 | Noto Sans CJK TC, Source Han Sans TC, Microsoft JhengHei, PingFang TC | Noto Serif CJK TC, Source Han Serif TC, PMingLiU, Songti TC, AR PL UMing TW |
| Adobe-Korea1, Adobe-KR | Noto Sans CJK KR, Source Han Sans K, Malgun Gothic, Apple SD Gothic Neo, NanumGothic | Noto Serif CJK KR, Source Han Serif K, Batang, AppleMyungjo, NanumMyeongjo |

The code-page bits that name each region are `ulCodePageRange1`'s 17
(Japan), 18 (Simplified Chinese), 20 (Traditional Chinese), and 19 and 21
(Korea).

**The script chains**, the second table: for each script FT-13.6 can name,
the families that cover it on the systems above — `Noto Sans` and
`Noto Serif` of that script first, then each system's own (for Arabic,
`Noto Naskh Arabic`, `Noto Sans Arabic`, `Segoe UI`, `Geeza Pro`; for
Devanagari, `Noto Sans Devanagari`, `Nirmala UI`, `Kohinoor Devanagari`,
`Lohit Devanagari`; and so on, script by script in the policy's table) — and
the **emoji chain**, `Noto Emoji`, `Segoe UI Emoji`, `Segoe UI Symbol`,
`DejaVu Sans`, each taken only where it draws the characters asked: a colour
emoji face with no outlines draws nothing here.

### The provider

`SystemFontProvider::new(roots)` is the crate's one constructor: the roots to
read, or, given none, the running system's font directories. It implements
`FontProvider`, so it is installed as any host provider is
(`OpenOptions::with_fonts`, `Document::with_fonts`), and the bundled faces,
where the build carries them, are still asked after it.

**The directories read**, each searched below it:

| System | Directories |
| --- | --- |
| Windows | `%SystemRoot%\Fonts` (`C:\Windows\Fonts` where the variable is unset), and the user's `%LOCALAPPDATA%\Microsoft\Windows\Fonts` |
| macOS | `/System/Library/Fonts`, `/Library/Fonts`, and `~/Library/Fonts` |
| Linux and other freedesktop systems | `$XDG_DATA_HOME/fonts` (`~/.local/share/fonts` where unset), `~/.fonts`, and `fonts` under each of `$XDG_DATA_DIRS` (`/usr/local/share` and `/usr/share` where unset) |
| Android | `/system/fonts` |
| Any other, `wasm32-wasip1` among them | none: only the roots a caller names |

A file is read where its extension is `.ttf`, `.otf`, `.ttc`, `.otc`,
`.woff` or `.woff2`, in any case, and is then described by its bytes, not its
name. The search follows a symbolic link once — a directory reached twice by
its canonical path is read once — and is bounded (ruling 1): directories
eight deep, 65 536 font files, 256 MiB a file; what passes a bound is skipped
and named, as is a file that will not read or describes no face.

**When it reads.** Nothing at construction. The first request lists the
directories and describes every face once, behind a `OnceLock`, so a process
that never asks reads nothing; each chosen face's bytes are read when it is
first chosen and kept, a collection member written out whole once
(`face_of_member`), and each distinct query's answer is kept beside them, so
a document asking for `Arial,Bold` on every page reads one file once.
`skipped()` hands back each file passed over and why. The provider is
`Send + Sync`, as `FontProvider` requires: its index and its caches sit
behind the lock and a mutex.

### Its surfaces

- **The CLI.** `tpdf --fonts system` installs `SystemFontProvider::new` over
  no roots, beside `--fonts PATH` and `--fonts bundled` (FT-13.4); a build
  with `bundled-fonts` still asks its bundled faces after the system's.
- **The C ABI** gains `tpdf_document_use_system_fonts`, taking the roots as
  the ABI passes every string, UTF-8 bytes and a length each, and installing
  the provider over them (none naming the system's directories); its header
  entry is generated as every other's. Python, .NET, Go, Ruby and Java each
  project it beside `set_fonts`, the roots as their own strings (FT-13.5).
  The JavaScript binding has no such call (NG-08), and
  `docs/features/bindings.md` names it excluded.

## Milestones

1. **FT-13.1, this document.**
2. **FT-13.2, a face described through the facade.** `describe_faces` and
   `face_of_member`, and `Sfnt`'s parse of member `n`. Exit: `sfnt.rs`'s
   `every_member_of_a_collection_parses` and `fonts.rs`'s
   `a_face_is_described_from_its_own_tables` and
   `a_member_written_out_is_the_face_it_was`, over synthesised faces and a
   synthesised two-member collection.
3. **FT-13.3, the policy.** `tools/sysfonts` with `policy`, declared in
   `TOOLS` and published. Exit: `tools/sysfonts/tests/policy_table.rs`'s
   `every_name_resolves_to_its_face`, a committed table of requests —
   subset-prefixed, `,Bold` and `-BoldMT` names, PANOSE-only matches, each
   collection's CJK default, each script chain and the emoji chain — each
   resolved against a fixed list of described faces to the face its row
   names; `cargo xtask dag` passing with the crate declared.
4. **FT-13.4, the provider and `tpdf --fonts system`.** Exit:
   `tools/sysfonts/tests/provider.rs`'s
   `a_root_of_faces_answers_as_the_policy_table_does`, over a temporary root
   of synthesised faces, a collection and a damaged file, which it names; and
   `tpdf`'s `fonts_system_is_a_provider_not_a_path`.
5. **FT-13.5, the native bindings.** Exit:
   `crates/tinker-pdf-ffi/tests/system_fonts.rs`'s
   `a_document_can_use_system_fonts`: a page drawn blank without a face and
   inked through the provider over a temporary root; the header agreeing with
   its regeneration; `cargo xtask dag` with the declared edge; a local
   `cargo xtask bindings-parity --require-all` green.
6. **FT-13.6, a document's own fonts reach the policy.** `FontRequest` gains
   the scripts a font's mapped text covers. Exit: `substitute_fonts.rs`'s
   `the_provider_is_told_the_collection_and_the_scripts` and `provider.rs`'s
   `a_document_font_resolves_through_its_collection_script_and_panose`; a
   composite request the engine would index by CID declined.
7. **FT-13.7, the speed row.** Describing every face of a generated
   many-member collection and writing one member out, in MS-04's fonts
   group; the provider's directory search is I/O on a host's disk and has no
   row. Exit: `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` holding the operation.

## Risks

| Risk | Mitigation |
| --- | --- |
| The first request on a machine with thousands of faces is slow | Nothing is read until a request; every face is described once a provider; FT-13.7 times the facade's description of a many-member collection, the part the engine owns |
| A damaged or hostile file in a font directory | Each is described by the facade's bounded readers; one that will not read is skipped and named, never fatal |
| A name normalised to the wrong family | The committed policy table pins every normalisation rule to the face it must reach, and a rule added later adds its rows |
| A Han character drawn in another region's forms | Faces are taken for a collection by region alone, by its default families and then its code-page bit; a composite request with no region and no mapped text is declined |
| An emoji face that draws nothing | A face covers only the characters it draws an outline for, so a bitmap-only emoji face is never chosen |
| The same request answered differently on two runs | The faces are ordered by family, path and member before the policy reads them, so ties break alike |
| A root that is not UTF-8 crosses the C ABI | Roots cross as UTF-8, as every path the ABI takes; a root that is not cannot be named through the bindings |
| A face installed while a process runs | Not seen until a new provider is made; the non-goals say so |
