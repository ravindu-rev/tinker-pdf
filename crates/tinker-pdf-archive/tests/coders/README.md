# The coders, held to archives real writers made

`crates/tinker-pdf/tests/cbz/` holds archives of five comic pages, and those
are what the comic path is held to. They are the wrong input for a coder: five
PNGs and a JPEG are already compressed, carry one BCJ candidate between them,
and never repeat a byte four times. This directory is the other half — **one
real third-party writer asked for one coder, over bytes shaped for that
coder** — and `tests/coders.rs` is what reads it.

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

## The archives

| File | Bytes | Writer | Coder | sha256 |
| --- | ---: | --- | --- | --- |
| `py7zr-bcj.7z` | 47 656 | py7zr 1.1.3, `[FILTER_X86, FILTER_LZMA2]` | 7z `03030103` (BCJ) fed by `21` (LZMA2) through a bind pair; `x86.bin` and `prose.txt` in one solid folder | `1fdf9888563556d9` |
| `py7zr-bzip2.7z` | 41 604 | py7zr 1.1.3, `[FILTER_BZIP2]` | 7z `040202`; `prose.txt`, `runs.bin` and `x86.bin` in one solid folder, one level-9 block | `091fb3303fa690e5` |
| `py7zr-ppmd.7z` | 37 965 | py7zr 1.1.3, `[FILTER_PPMD]` order 6, `mem` 24 | 7z `030401` in a 16 MiB arena the model never fills | `a7b31f4828f5d068` |
| `py7zr-ppmd-tight.7z` | 56 394 | py7zr 1.1.3, `[FILTER_PPMD]` order 32, `mem` 16 | The same coder in **64 KiB**: the arena fills and the model restarts again and again over 148 KB — counted by `the_ppmd_fixtures_run_in_the_arenas_they_are_named_for` | `31de4d89ffcd0f68` |
| `7zz-bcj2.7z` | 47 068 | 7-Zip 26.02 for Linux (`7zz`), `-m0=BCJ2 -m1=LZMA:d20 -m2=LZMA:d20 -m3=LZMA:d20 -mb0:1 -mb0s1:2 -mb0s2:3` | 7z `0303011B`: BCJ2's main, call and jump streams each out of an LZMA coder, its decisions packed as they are — four coders, four pack streams, one output; `prose.txt` and `x86.bin` | `a18b64c4f11c5646` |
| `python-bzip2.zip` | 41 656 | CPython 3.11.15 `zipfile`, `ZIP_BZIP2`, `compresslevel=1` | APPNOTE method 12 on all four inputs: `prose.txt` is **two** blocks at level 1's 100 000-byte limit, and `empty.txt` a stream with **no** block | `8ca8493ac60b6287` |

Obtained on Linux x86_64 with CPython 3.11.15, py7zr 1.1.3, liblzma 5.4.5
(Ubuntu `5.6.1+really5.4.5-1ubuntu0.2`), libbzip2 1.0.8 (Ubuntu
`1.0.8-5.1build0.1`) and pyppmd 1.3.1, on 26 September 2026:

```
pip install --user py7zr
cd crates/tinker-pdf-archive/tests/coders
python3 make-inputs.py
python3 make-coders.py
sh make-bcj2.sh
```

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

## Whether they may be committed

Yes, for the reason `crates/tinker-pdf/tests/cbz/README.md` gives: a coder's
licence does not reach the bytes it codes, the inputs are ours, and nothing of
any writer is vendored, linked or redistributed. py7zr is LGPL-2.1-or-later;
liblzma is public domain (0BSD from 5.6); libbzip2 is under its own BSD-style
licence; 7-Zip is LGPL-2.1-or-later with an unRAR restriction, none of which
reaches an archive of our bytes.
