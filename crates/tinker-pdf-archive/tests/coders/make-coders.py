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
