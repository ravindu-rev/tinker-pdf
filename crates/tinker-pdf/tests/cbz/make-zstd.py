# Writes python-zstd.cbz: the five pages in source/, every entry ZIP method 93.
#
# APPNOTE 4.4.5's method 93 is Zstandard, and no ZIP writer on hand makes it:
# CPython's zipfile learned it in 3.14 and this machine has 3.11. So the
# frames are libzstd's -- python-zstandard's ZstdCompressor, the reference
# encoder, at level 3 with the content checksum on -- and the ZIP around them
# is written here field by field from APPNOTE 4.3, the part that is ZIP's
# rather than the coder's. The pages are the corpus' own, committed in
# source/, so the decoded entries have an exact expected answer that no
# decoder produced: the files that went in.
#
#   pip install --user zstandard
#   cd crates/tinker-pdf/tests/cbz && python3 make-zstd.py
#
# One fixed timestamp for every entry and nothing read from a clock, so a
# rerun under the same zstandard and libzstd writes the same bytes; README.md
# records both versions and the hash. Not run by any test: the committed
# archive is the record (ruling 13).

import struct
import zlib

import zstandard

# The order make-corpus.ps1 packs in, which is deliberately not reading order.
ORDER = ["page1.png", "page10.png", "page11.png", "page2.png", "page3.jpg"]
# 26 September 2026, 00:00:00, as a DOS date and time.
DATE, TIME = ((2026 - 1980) << 9) | (9 << 5) | 26, 0

body, central = b"", b""
for name in ORDER:
    with open("source/" + name, "rb") as f:
        plain = f.read()
    data = zstandard.ZstdCompressor(level=3, write_checksum=True).compress(plain)
    crc = zlib.crc32(plain)
    # Version needed 6.3: 4.4.3.2 names none for method 93, and 6.3 is the
    # highest it names.
    local = struct.pack(
        "<IHHHHHIIIHH", 0x04034B50, 63, 0, 93, TIME, DATE, crc, len(data), len(plain),
        len(name), 0,
    )
    central += struct.pack(
        "<IHHHHHHIIIHHHHHII", 0x02014B50, 63, 63, 0, 93, TIME, DATE, crc, len(data),
        len(plain), len(name), 0, 0, 0, 0, 0, len(body),
    ) + name.encode()
    body += local + name.encode() + data
end = struct.pack(
    "<IHHHHIIH", 0x06054B50, 0, 0, len(ORDER), len(ORDER), len(central), len(body), 0
)
with open("python-zstd.cbz", "wb") as f:
    f.write(body + central + end)
