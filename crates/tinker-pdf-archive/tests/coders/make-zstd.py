# Writes the Zstandard fixtures: bare streams (zstd-*.zst), one ZIP of
# method-93 entries (zstd-method-93.zip), and the seeds of the `zstd` fuzz
# target and the `zip_archive` target's `zstd-method-93`.
#
#   pip install --user zstandard
#   cd crates/tinker-pdf-archive/tests/coders && python3 make-zstd.py
#
# python-zstandard is libzstd's own encoder behind a CPython binding: the
# reference implementation, so these are the frames the format's authors
# write. Every input is a file make-inputs.py made, and the expected answer
# for every fixture is those files -- never another decoder's output
# (ruling 13).
#
# CPython 3.11's zipfile has no method 93 (3.14 added it), so the ZIP is
# written here field by field from APPNOTE 4.3, around frames libzstd wrote.
# Nothing here reads a clock; a rerun under the same zstandard and libzstd
# writes the same bytes. Not run by any test: the committed files are the
# record (ruling 13).

import os
import struct
import zlib

import zstandard

HERE = os.path.dirname(os.path.abspath(__file__))
SEEDS = os.path.join(HERE, "..", "..", "..", "..", "fuzz", "corpus")


def read(name):
    with open(os.path.join(HERE, "input", name), "rb") as f:
        return f.read()


PROSE, X86, RUNS, EMPTY, MODES = (
    read(n) for n in ("prose.txt", "x86.bin", "runs.bin", "empty.txt", "modes.bin")
)
# modes.bin's five sections, as make-inputs.py lays them out; the fifth is
# flushed as two blocks of twenty records.
SECTIONS = [
    MODES[:4096],
    MODES[4096:6596],
    MODES[6596:7396],
    MODES[7396:9396],
    MODES[9396:10136],
    MODES[10136:],
]


def frame(data, level, checksum=True, content_size=True, **params):
    """One frame, libzstd one-shot: the content size is known up front."""
    cp = zstandard.ZstdCompressionParameters.from_level(
        level,
        source_size=len(data),
        write_checksum=checksum,
        write_content_size=content_size,
        **params,
    )
    return zstandard.ZstdCompressor(compression_params=cp).compress(data)


def streamed(chunks, level, checksum=True):
    """One frame written as a stream: no content size, a window descriptor,
    and a block boundary at every flush."""
    c = zstandard.ZstdCompressor(level=level, write_checksum=checksum)
    o = c.compressobj()
    out = b""
    for chunk in chunks:
        out += o.compress(chunk)
        out += o.flush(zstandard.COMPRESSOBJ_FLUSH_BLOCK)
    return out + o.flush()


def skippable(nibble, payload):
    """RFC 8878 3.1.2: magic 0x184D2A5?, a four-byte size, anything."""
    return struct.pack("<II", 0x184D2A50 | nibble, len(payload)) + payload


def write(path, data):
    with open(path, "wb") as f:
        f.write(data)


# The bare streams, each named for what it is shaped to reach.
FIXTURES = {
    # Level 3 over 120 KB: one single-segment frame, Huffman literals in
    # four streams, FSE-described sequence tables.
    "zstd-prose-l3.zst": frame(PROSE, 3),
    # Level 19 (btultra): the optimal parser's longer matches and repeat
    # offsets; no checksum, so the content size is the only frame check.
    "zstd-prose-l19.zst": frame(PROSE, 19, checksum=False),
    # Level 22 over the x86 file: the incompressible kilobyte of soup
    # beside code.
    "zstd-x86-l22.zst": frame(X86, 22),
    # Level 1 over the runs: long matches at offset one.
    "zstd-runs-l1.zst": frame(RUNS, 1),
    # A 1 KiB window caps every block at 1 KiB: 118 blocks in one frame, so
    # the treeless literals, repeat-mode tables and repeat offsets that carry
    # across blocks are all reached, and offsets run up to the window.
    "zstd-prose-w10.zst": frame(PROSE, 7, window_log=10),
    # Four frames and a skippable one: prose then x86 streamed in 16 KiB
    # flushes, a skippable frame, runs in one shot, and an empty frame.
    "zstd-frames.zst": streamed(
        [PROSE[i : i + 16384] for i in range(0, len(PROSE), 16384)], 5
    )
    + streamed([X86], 12, checksum=False)
    + skippable(0xE, b"tinker-pdf: a skippable frame is skipped")
    + frame(RUNS, 2)
    + frame(EMPTY, 3),
    # modes.bin, one block per section, in three frames: the nibbles and the
    # records at level 3 (direct Huffman weights; RLE literals and RLE for
    # all three sequence tables), the prose and the run at level 1 (the
    # three predefined tables, which only a fast level picks, and an RLE
    # block, which libzstd never makes a frame's first), and the forty
    # records in two blocks at level 3 (a repeat offset one block leaves the
    # next).
    "zstd-modes.zst": streamed(SECTIONS[:2], 3)
    + streamed(SECTIONS[2:4], 1)
    + streamed(SECTIONS[4:], 3),
    # A Huffman tree outlives a block that did not replace it: prose in
    # 16 KiB flushes at level 5, with a block of text already seen -- all
    # match, its literals raw -- between two whose literals reuse the tree.
    "zstd-treeless.zst": streamed(
        [PROSE[:16384], PROSE[16384:32768], PROSE[:4000], PROSE[32768:49152]], 5
    ),
    # The empty input, as libzstd frames it.
    "zstd-empty.zst": frame(EMPTY, 3),
    # Eighteen checksummed frames of prose's first n bytes, for n either side
    # of every length XXH64 treats differently: under 4, 8 and 32 bytes (no
    # stripe; a four-byte lane; an eight-byte lane) and past a stripe.
    "zstd-checksums.zst": b"".join(
        frame(PROSE[:n], 1)
        for n in (0, 1, 3, 4, 5, 7, 8, 9, 15, 16, 31, 32, 33, 63, 64, 65, 100, 1000)
    ),
}

# A frame whose header names a trained dictionary: refused by name.
samples = [PROSE[i : i + 512] for i in range(0, len(PROSE), 512)]
trained = zstandard.train_dictionary(4096, samples, dict_id=0x7E57, threads=0)
FIXTURES["zstd-dictionary.zst"] = zstandard.ZstdCompressor(
    level=3, dict_data=trained, write_dict_id=True
).compress(PROSE[:4096])
# A frame that needs a raw-content dictionary and cannot name one (its ID is
# zero): its first match reaches back before the frame's first byte.
raw = zstandard.ZstdCompressionDict(PROSE[:8192], dict_type=zstandard.DICT_TYPE_RAWCONTENT)
FIXTURES["zstd-raw-dictionary.zst"] = zstandard.ZstdCompressor(
    level=3, dict_data=raw, write_dict_id=False
).compress(PROSE[:8192])

for name, data in FIXTURES.items():
    write(os.path.join(HERE, name), data)


# APPNOTE 4.3: local headers, a central directory, an end record. Method 93
# is "Zstandard (zstd) Compression" (4.4.5). 4.4.3.2 names no version needed
# for it; 6.3, the highest it names (LZMA's), is written.
def zip_archive(entries):
    body, central = b"", b""
    date = ((2026 - 1980) << 9) | (9 << 5) | 26
    for name, method, data, plain in entries:
        crc = zlib.crc32(plain)
        header = struct.pack(
            "<IHHHHHIIIHH", 0x04034B50, 63, 0, method, 0, date, crc, len(data), len(plain),
            len(name), 0,
        )
        central += struct.pack(
            "<IHHHHHHIIIHHHHHII", 0x02014B50, 63, 63, 0, method, 0, date, crc, len(data),
            len(plain), len(name), 0, 0, 0, 0, 0, len(body),
        ) + name.encode()
        body += header + name.encode() + data
    end = struct.pack(
        "<IHHHHIIH", 0x06054B50, 0, 0, len(entries), len(entries), len(central), len(body), 0
    )
    return body + central + end


write(
    os.path.join(HERE, "zstd-method-93.zip"),
    zip_archive(
        [
            ("prose.txt", 93, frame(PROSE, 3), PROSE),
            ("x86.bin", 93, frame(X86, 19), X86),
            ("runs.bin", 93, streamed([RUNS], 1), RUNS),
            ("empty.txt", 93, frame(EMPTY, 3), EMPTY),
        ]
    ),
)


# Seeds: small, so a fuzz iteration is quick, and each reaching a different
# part of the decoder. The `zstd` target reads one control byte first; 0xFF
# picks its roomiest ceiling.
def seed(target, name, data):
    os.makedirs(os.path.join(SEEDS, target), exist_ok=True)
    write(os.path.join(SEEDS, target, name), data)


seed("zstd", "prose-l3", b"\xff" + frame(PROSE[:3000], 3))
seed("zstd", "x86-l19", b"\xff" + frame(X86[:2048], 19))
seed("zstd", "runs-l1", b"\xff" + frame(RUNS[:4096], 1))
seed("zstd", "window-1k", b"\xff" + frame(PROSE[:6000], 7, window_log=10))
seed(
    "zstd",
    "frames",
    b"\xff"
    + streamed([PROSE[:1500], PROSE[1500:3000]], 5)
    + skippable(0x0, b"skip")
    + frame(RUNS[:600], 2)
    + frame(EMPTY, 3),
)
seed("zstd", "empty", b"\xff" + frame(EMPTY, 3))
seed(
    "zip_archive",
    "zstd-method-93",
    b"\xff" + zip_archive([("prose.txt", 93, frame(PROSE[:2000], 3), PROSE[:2000])]),
)
