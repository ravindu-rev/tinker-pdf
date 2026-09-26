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

Obtained on Linux x86_64 with CPython 3.11.15, py7zr 1.1.3 and liblzma 5.4.5
(Ubuntu `5.6.1+really5.4.5-1ubuntu0.2`), on 26 September 2026:

```
pip install --user py7zr
cd crates/tinker-pdf-archive/tests/coders
python3 make-inputs.py
python3 make-coders.py
```

`make-coders.py` pins py7zr's clock to one instant, so a rerun under the same
versions writes the same bytes; every hash above was measured twice. It also
writes the real-writer seeds in `fuzz/corpus/` (`sevenz/bcj-lzma2`: the same
filters over `x86.bin`'s first 768 bytes).

### Which implementation did the coding

py7zr is a 7z *container* writer; the coding it hands to libraries, and which
one matters, because the coder is what these files adjudicate.

- **BCJ + LZMA2**: py7zr passes both to liblzma as one raw filter chain, so
  the x86 filter here is **xz's**, not 7-Zip's `Bra86.c` — a second
  implementation of the encoder the decoder must invert.

## Whether they may be committed

Yes, for the reason `crates/tinker-pdf/tests/cbz/README.md` gives: a coder's
licence does not reach the bytes it codes, the inputs are ours, and nothing of
any writer is vendored, linked or redistributed. py7zr is LGPL-2.1-or-later;
liblzma is public domain (0BSD from 5.6).
