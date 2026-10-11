# The coders, held to archives real writers made

`crates/tinker-pdf/tests/cbz/` holds archives of five comic pages, and those
are what the comic path is held to. They are the wrong input for a coder: five
PNGs and a JPEG are already compressed, carry one BCJ candidate between them,
and never repeat a byte four times. This directory is the other half — **one
real third-party writer asked for one coder, over bytes shaped for that
coder** — and `tests/coders.rs` is what reads it. Zstandard, which ZIP carries
as whole frames, is here as bare streams as well as in a ZIP, and
`src/zstd/tests.rs` reads those too.

The device is the comic corpus's: *author the input here, have somebody else's
tool code it, commit the result.* Every coder here is lossless, so the expected
answer for an entry is **the file that went in**, byte for byte — never another
decoder's output (ruling 13). The archive's own CRC-32 over the original bytes
stands behind that inside the reader, which is what adjudicates a hand-rolled
decoder in this crate; the byte comparison is the stronger claim, made from
outside.

## The inputs

`make-inputs.py` writes `input/` by arithmetic — a 64-bit LCG with fixed seeds
and no clock — so the files are this repository's own and a rerun writes the
same bytes on any Python 3.

| File | Bytes | Shaped to reach | sha256 |
| --- | ---: | --- | --- |
| `x86.bin` | 16 384 | BCJ: thirty-six operands built at their own offsets so the filter converts them *twice* (a candidate one, two or three bytes after a skipped `E8`), a kilobyte of `E8`/`E9`/`00`/`FF` soup for the previous-byte mask, then code-shaped bytes — `E8` calls and `E9` jumps whose targets land inside the file, `0F 8x` conditional jumps (BCJ2's, not BCJ's), `E8`s whose top operand byte is neither `00` nor `FF` — and an `E8` in the last four bytes | `f47373d05f73b12c` |
| `prose.txt` | 120 021 | Long enough that bzip2 at level 1 cuts it into two blocks; repetitive enough that PPMd reaches high orders | `6b1e444d13a5c92d` |
| `runs.bin` | 11 781 | Runs of one byte either side of every run-coder threshold (1–6, 250–261, 1 000, 4 096), then every byte value once | `37b63d50e7cdf6a7` |
| `empty.txt` | 0 | A stream with nothing in it | `e3b0c44298fc1c14` |
| `modes.bin` | 10 876 | Zstandard's rarer codings, a block or two per section: 4 096 random nibbles (sixteen equally likely literals, whose Huffman weights libzstd writes four bits each rather than FSE-coded); 100 records of `Q` and 24 bytes copied from 3 993 to 4 092 bytes back (every literal one byte and every sequence one set of codes: RLE literals and RLE for all three sequence tables); 800 bytes of prose (too few sequences for a fast level to describe a table: the predefined ones); 2 000 bytes of `Z` (an RLE block); and 40 records of a counter byte and the same 36 bytes, in two blocks, so the second block's first match repeats an offset the first block left | `340e83f7ec448581` |

## The archives

| File | Bytes | Writer | Coder | sha256 |
| --- | ---: | --- | --- | --- |
| `py7zr-bcj.7z` | 47 656 | py7zr 1.1.3, `[FILTER_X86, FILTER_LZMA2]` | 7z `03030103` (BCJ) fed by `21` (LZMA2) through a bind pair; `x86.bin` and `prose.txt` in one solid folder | `1fdf9888563556d9` |
| `py7zr-bzip2.7z` | 41 604 | py7zr 1.1.3, `[FILTER_BZIP2]` | 7z `040202`; `prose.txt`, `runs.bin` and `x86.bin` in one solid folder, one level-9 block | `091fb3303fa690e5` |
| `py7zr-ppmd.7z` | 37 965 | py7zr 1.1.3, `[FILTER_PPMD]` order 6, `mem` 24 | 7z `030401` in a 16 MiB arena the model never fills | `a7b31f4828f5d068` |
| `py7zr-ppmd-tight.7z` | 56 394 | py7zr 1.1.3, `[FILTER_PPMD]` order 32, `mem` 16 | The same coder in **64 KiB**: the arena fills and the model restarts again and again over 148 KB — counted by `the_ppmd_fixtures_run_in_the_arenas_they_are_named_for` | `31de4d89ffcd0f68` |
| `7zz-bcj2.7z` | 47 068 | 7-Zip 26.02 for Linux (`7zz`), `-m0=BCJ2 -m1=LZMA:d20 -m2=LZMA:d20 -m3=LZMA:d20 -mb0:1 -mb0s1:2 -mb0s2:3` | 7z `0303011B`: BCJ2's main, call and jump streams each out of an LZMA coder, its decisions packed as they are — four coders, four pack streams, one output; `prose.txt` and `x86.bin` | `a18b64c4f11c5646` |
| `python-bzip2.zip` | 41 656 | CPython 3.11.15 `zipfile`, `ZIP_BZIP2`, `compresslevel=1` | APPNOTE method 12 on all four inputs: `prose.txt` is **two** blocks at level 1's 100 000-byte limit, and `empty.txt` a stream with **no** block | `8ca8493ac60b6287` |
| `zstd-method-93.zip` | 54 553 | libzstd 1.5.7's frames (python-zstandard 0.25.0), in a ZIP `make-zstd.py` writes from APPNOTE 4.3 | APPNOTE method 93 on the first four inputs: `prose.txt` at level 3, `x86.bin` at level 19, `runs.bin` **streamed** (no content size) and `empty.txt` | `e660de472c478165` |

### The Zstandard streams

Bare frames, as libzstd writes them and as a method-93 entry holds them. What
each is shaped to reach is in `make-zstd.py`; that together they reach every
coding RFC 8878 gives a block — raw, RLE and compressed blocks; raw, RLE,
Huffman and treeless literals in one and four streams; FSE-coded and direct
Huffman weights; each of the four modes for each of the three sequence
tables; all four repeat offsets, and one carried from block to block; a
Huffman tree kept across a block whose literals were raw; single-segment and
windowed frames; frames with and without a checksum; a skippable frame — is
what
`the_fixtures_reach_every_part_of_the_format` asserts.

| File | Bytes | libzstd asked for | Decodes to | sha256 |
| --- | ---: | --- | --- | --- |
| `zstd-prose-l3.zst` | 40 430 | level 3, checksum, content size | `prose.txt` | `16644d22c7d9a245` |
| `zstd-prose-l19.zst` | 35 223 | level 19, no checksum | `prose.txt` | `11cb2f1a264fbfa9` |
| `zstd-x86-l22.zst` | 12 010 | level 22 | `x86.bin` | `9e28ab02946e472d` |
| `zstd-runs-l1.zst` | 1 684 | level 1 | `runs.bin` | `2f15e7075cc61ac8` |
| `zstd-prose-w10.zst` | 54 281 | level 7, `window_log=10`: 118 blocks of at most 1 KiB | `prose.txt` | `92c1f6b8da1470cb` |
| `zstd-frames.zst` | 53 919 | four frames and a skippable one: `prose.txt` streamed in 16 KiB flushes, `x86.bin` streamed, the skippable frame, `runs.bin`, and an empty frame | `prose.txt`, `x86.bin`, `runs.bin` | `12f953ec609c159d` |
| `zstd-modes.zst` | 2 814 | `modes.bin` a block per section, in three frames (level 3, level 1, level 3) | `modes.bin` | `4abdfb7cdcc31246` |
| `zstd-treeless.zst` | 16 865 | `prose.txt` in 16 KiB flushes at level 5, with a block of text already seen — all match, literals raw — between two whose literals reuse the last tree | `prose.txt`'s first 32 768 bytes, its first 4 000, then the next 16 384 | `4a5bc7343703a692` |
| `zstd-empty.zst` | 13 | the empty input | nothing | `f96deff1816083fd` |
| `zstd-checksums.zst` | 1 214 | eighteen checksummed frames of `prose.txt`'s first 0 to 1 000 bytes, either side of every length XXH64 treats differently | those prefixes | `0108cc45d5c344ed` |
| `zstd-dictionary.zst` | 1 341 | a frame naming a dictionary trained on `prose.txt` (ID `0x7E57`) | refused: `NeedsDictionary` | `4d4b0122a8c7901b` |
| `zstd-raw-dictionary.zst` | 19 | a frame needing a raw-content dictionary it cannot name | refused: `BadOffset` | `6388fcb574a660ac` |

Obtained on Linux x86_64 with CPython 3.11.15, py7zr 1.1.3, liblzma 5.4.5
(Ubuntu `5.6.1+really5.4.5-1ubuntu0.2`), libbzip2 1.0.8 (Ubuntu
`1.0.8-5.1build0.1`), pyppmd 1.3.1 and python-zstandard 0.25.0 (its bundled
libzstd 1.5.7), on 26 September 2026:

```
pip install --user py7zr zstandard
cd crates/tinker-pdf-archive/tests/coders
python3 make-inputs.py
python3 make-coders.py
sh make-bcj2.sh
python3 make-zstd.py
```

`modes.bin` arrived after the other four inputs, from its own seed and its
own arithmetic, and adding it left their bytes unchanged.

`make-bcj2.sh` needs 7-Zip 26.02's Linux build on the path as `7zz`: the
`7z2602-linux-x64.tar.xz` asset of the ip7z/7zip GitHub release 26.02,
SHA-256 `41aaba7b1235304ab5aa0624530c67ae829496cd29e875925271efdccc28c03e`,
unpacked as it comes. It leaves timestamps out of the archive and runs
single-threaded, so a rerun writes the same bytes, and it also writes the
`sevenz/bcj2` seed.

`make-coders.py` pins py7zr's clock to one instant, so a rerun under the same
versions writes the same bytes; every hash above was measured twice. It also
writes the real-writer seeds in `fuzz/corpus/`: `sevenz/bcj-lzma2` (the same
filters over `x86.bin`'s first 768 bytes), `sevenz/bzip2`,
`zip_archive/bzip2-method-12`, and the five bare streams of `bzip2/` —
libbzip2 through CPython's `bz2` at three levels, an empty stream, and two
streams end to end — `sevenz/ppmd` (order 8 in 1 MiB, inside the `sevenz`
target's roomiest bound), and the four bare streams of `ppmd/`, pyppmd's,
behind the three parameter bytes that target reads: order 2 in 2 KiB,
order 6 in 64 KiB, order 16 in 1 MiB and order 64 in 8 KiB.

`make-zstd.py` reads no clock either, so a rerun under the same zstandard and
libzstd writes the same bytes; every hash above was measured twice. It writes
the six `zstd/` seeds — libzstd's frames over slices of the inputs, behind
the target's `0xFF` control byte: level 3, level 19, level 1, a 1 KiB window,
two frames with a skippable one between and an empty one after, and the empty
input — and `zip_archive/zstd-method-93`, one method-93 entry in the same
hand-written ZIP.

### Which implementation did the coding

py7zr is a 7z *container* writer; the coding it hands to libraries, and which
one matters, because the coder is what these files adjudicate.

- **BCJ + LZMA2**: py7zr passes both to liblzma as one raw filter chain, so
  the x86 filter here is **xz's**, not 7-Zip's `Bra86.c` — a second
  implementation of the encoder the decoder must invert.
- **bzip2**: py7zr and CPython's `zipfile` both call CPython's `bz2`, which is
  libbzip2 1.0.8 — the reference implementation, so the one whose reading of
  the format is the format.
- **BCJ2**: 7-Zip is the only writer of it, so `7zz-bcj2.7z` is the same
  program as `tests/cbz/`'s Windows `.cb7`s, on another platform. Its folder
  lists BCJ2 last of the four coders, where the command line numbers it
  first — the listing order is the writer's, and the bind pairs are what
  the reader follows.
- **PPMd**: py7zr calls pyppmd 1.3.1, which is 7-Zip's own `Ppmd7.c` and
  `Ppmd7Enc.c` compiled for CPython — the same public-domain code this
  crate's decoder was transcribed from. So these archives are **not** a
  second implementation of the model: an error shared between 7-Zip's
  encoder and a faithful reading of 7-Zip's decoder would pass. What they
  are is the encoder every PPMd 7z in the world was written by, and the
  CRC-32 over the original bytes is what says this decoder inverts it.

- **Zstandard**: python-zstandard is a CPython binding over libzstd itself,
  the reference implementation, so these are the frames the format's
  authors' encoder writes. The ZIP around the method-93 entries is not
  libzstd's and is not a ZIP writer's either: CPython's `zipfile` learned
  method 93 in 3.14, so `make-zstd.py` writes the local headers, central
  directory and end record itself, from APPNOTE 4.3 — the part of the file
  that is ZIP's, which `tinker-pdf-zip` is already held to by other writers.

## Whether they may be committed

Yes, for the reason `crates/tinker-pdf/tests/cbz/README.md` gives: a coder's
licence does not reach the bytes it codes, the inputs are ours, and nothing of
any writer is vendored, linked or redistributed. py7zr is LGPL-2.1-or-later;
liblzma is public domain (0BSD from 5.6); libbzip2 is under its own BSD-style
licence; 7-Zip is LGPL-2.1-or-later with an unRAR restriction; python-zstandard
and libzstd are BSD-3-Clause (libzstd dual with GPL-2.0), none of which
reaches an archive of our bytes.
