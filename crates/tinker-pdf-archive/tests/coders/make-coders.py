# Writes the archives in this directory from the files in input/.
#
# Each is one real third-party writer asked for one coder, over this
# repository's own bytes (make-inputs.py). Every coder here is lossless, so the
# expected answer to decoding any entry is the file that went in -- never
# another decoder's output (ruling 13) -- and the archive's own CRC-32 over the
# original bytes stands behind that inside the reader.
#
#   py7zr-bcj.7z        py7zr, filters [BCJ x86, LZMA2]: coder 03030103 fed by
#                       21 through a bind pair, one solid folder of x86.bin and
#                       prose.txt
#   py7zr-bzip2.7z      py7zr, filter [BZIP2]: coder 040202 over the three
#                       non-empty inputs in one solid folder
#   python-bzip2.zip    CPython zipfile, ZIP_BZIP2 (APPNOTE method 12)
#   py7zr-ppmd.7z       py7zr, filter [PPMD] order 6 in 16 MiB: coder 030401
#   py7zr-ppmd-tight.7z the same at order 32 in 64 KiB, which restarts
#
#   cd crates/tinker-pdf-archive/tests/coders && python3 make-coders.py
#
# py7zr stamps every entry with the current time, so its clock is pinned to one
# instant for the run (the one line below that reaches into it); CPython's
# zipfile takes the stamp it is given. So a rerun under the same versions
# writes the same bytes, and README.md records the versions and the hashes.
# Not run by any test: the committed archives are the record (ruling 13).

import bz2
import datetime
import io
import zipfile

import py7zr
import pyppmd
import py7zr.helpers

STAMP = (2026, 9, 26, 0, 0, 0)
INSTANT = datetime.datetime(*STAMP, tzinfo=datetime.timezone.utc).timestamp()
py7zr.helpers.ArchiveTimestamp.from_now = classmethod(lambda cls: cls.from_datetime(INSTANT))


def data(name):
    with open("input/" + name, "rb") as f:
        return f.read()


def sevenz(path, filters, names):
    with py7zr.SevenZipFile(path, "w", filters=filters) as z:
        for name in names:
            z.writestr(data(name), name)


BCJ = [{"id": py7zr.FILTER_X86}, {"id": py7zr.FILTER_LZMA2}]
BZIP2 = [{"id": py7zr.FILTER_BZIP2}]

sevenz("py7zr-bcj.7z", BCJ, ["x86.bin", "prose.txt"])
sevenz("py7zr-bzip2.7z", BZIP2, ["prose.txt", "runs.bin", "x86.bin"])
# PPMd at py7zr's own defaults, order 6 in 16 MiB (mem 24 is 2^24): room to
# spare, so the model never restarts.
PPMD = [{"id": py7zr.FILTER_PPMD, "order": 6, "mem": 24}]
sevenz("py7zr-ppmd.7z", PPMD, ["prose.txt", "runs.bin", "x86.bin"])
# And order 32 in 64 KiB (mem 16): the arena fills after a few kilobytes and
# the model is thrown away and rebuilt, over and over, across 148 KB -- the
# allocator's gluing, its borrowing from the text area and RestartModel all
# run, which a roomy arena never asks of them.
sevenz(
    "py7zr-ppmd-tight.7z",
    [{"id": py7zr.FILTER_PPMD, "order": 32, "mem": 16}],
    ["prose.txt", "runs.bin", "x86.bin"],
)

# APPNOTE method 12 at compresslevel 1, whose 100 000-byte blocks cut
# prose.txt's 120 KB in two; empty.txt is a stream with no block at all.
with zipfile.ZipFile("python-bzip2.zip", "w") as z:
    for name in ["prose.txt", "runs.bin", "x86.bin", "empty.txt"]:
        info = zipfile.ZipInfo(name, date_time=STAMP)
        info.compress_type = zipfile.ZIP_BZIP2
        # compresslevel on the writestr and not the ZipFile: a ZipInfo the
        # caller built carries no level, and the ZipFile's is not consulted.
        z.writestr(info, data(name), compresslevel=1)

# Fuzz seeds: the same writers over a slice small enough for a fuzzer to
# mutate usefully, each behind the target's control byte (0xFF, every bound
# at its roomiest). fuzz/corpus is committed; this is how the real-writer
# seeds in it were obtained.
SEEDS = "../../../../fuzz/corpus/"


def seed(target, name, body):
    with open(SEEDS + target + "/" + name, "wb") as f:
        f.write(b"\xff" + body)


def sevenz_bytes(filters, name, payload):
    buffer = io.BytesIO()
    with py7zr.SevenZipFile(buffer, "w", filters=filters) as z:
        z.writestr(payload, name)
    return buffer.getvalue()


seed("sevenz", "bcj-lzma2", sevenz_bytes(BCJ, "x86.bin", data("x86.bin")[:768]))
seed("sevenz", "bzip2", sevenz_bytes(BZIP2, "runs.bin", data("runs.bin")[:600]))

zipped = io.BytesIO()
with zipfile.ZipFile(zipped, "w") as z:
    info = zipfile.ZipInfo("page1.png", date_time=STAMP)
    info.compress_type = zipfile.ZIP_BZIP2
    z.writestr(info, data("prose.txt")[:400])
seed("zip_archive", "bzip2-method-12", zipped.getvalue())

# The bzip2 target's own seeds are bare streams: libbzip2 through CPython's
# bz2 module, at the level the name says.
seed("bzip2", "runs-l1", bz2.compress(data("runs.bin")[:1500], 1))
seed("bzip2", "prose-l9", bz2.compress(data("prose.txt")[:2000], 9))
seed("bzip2", "x86-l5", bz2.compress(data("x86.bin")[:1200], 5))
seed("bzip2", "empty", bz2.compress(b"", 9))
seed("bzip2", "two-streams", bz2.compress(b"first", 9) + bz2.compress(b"second", 9))

# A 1 MiB arena rather than py7zr's 16 MiB: the sevenz target's roomiest bound is
# 4 MiB, and a seed its own bounds refuse would never reach the model.
PPMD_SEED = [{"id": py7zr.FILTER_PPMD, "order": 8, "mem": 20}]
seed("sevenz", "ppmd", sevenz_bytes(PPMD_SEED, "prose.txt", data("prose.txt")[:700]))

# The ppmd target's seeds are a bare 7z-style PPMd stream behind the target's
# own three parameter bytes -- the model order less two, the arena as a power
# of two over 2 KiB, and the length in sixteens -- each coded by pyppmd, which
# is 7-Zip's Ppmd7Enc.c, with the order, arena and length its name says.


def ppmd_seed(name, order, arena_log2, payload):
    assert len(payload) % 16 == 0 and len(payload) // 16 < 256
    encoder = pyppmd.Ppmd7Encoder(order, 1 << arena_log2)
    stream = encoder.encode(payload)
    stream += encoder.flush()
    head = bytes([order - 2, arena_log2 - 11, len(payload) // 16])
    # No 0xFF in front: these three bytes are the whole control prefix.
    with open(SEEDS + "ppmd/" + name, "wb") as f:
        f.write(head + stream)


ppmd_seed("prose-o6-64k", 6, 16, data("prose.txt")[:1024])
ppmd_seed("x86-o2-2k", 2, 11, data("x86.bin")[:1024])
ppmd_seed("runs-o64-8k", 64, 13, data("runs.bin")[:2048])
ppmd_seed("prose-o16-1m", 16, 20, data("prose.txt")[:4080])
