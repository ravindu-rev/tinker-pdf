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
#
#   cd crates/tinker-pdf-archive/tests/coders && python3 make-coders.py
#
# py7zr stamps every entry with the current time, so its clock is pinned to one
# instant for the run (the one line below that reaches into it); CPython's
# zipfile takes the stamp it is given. So a rerun under the same versions
# writes the same bytes, and README.md records the versions and the hashes.
# Not run by any test: the committed archives are the record (ruling 13).

import datetime
import io

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

sevenz("py7zr-bcj.7z", BCJ, ["x86.bin", "prose.txt"])

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
