# Writes python-bzip2.cbz: the five pages in source/, every entry ZIP method 12.
#
# CPython's zipfile writes APPNOTE method 12 (bzip2) through libbzip2, and it is
# the writer on hand that does -- the same one make-lzma.py asked for method 14.
# The pages are the corpus' own, committed in source/, so the decoded entries
# have an exact expected answer that no decoder produced: the files that went
# in.
#
#   cd crates/tinker-pdf/tests/cbz && python3 make-bzip2.py
#
# One fixed timestamp for every entry, so a rerun under the same CPython and
# libbzip2 writes the same bytes; README.md records both versions and the hash.
# Not run by any test: the committed archive is the record (ruling 13).

import zipfile

# The order make-corpus.ps1 packs in, which is deliberately not reading order.
ORDER = ["page1.png", "page10.png", "page11.png", "page2.png", "page3.jpg"]
STAMP = (2026, 9, 26, 0, 0, 0)

with zipfile.ZipFile("python-bzip2.cbz", "w") as z:
    for name in ORDER:
        with open("source/" + name, "rb") as f:
            data = f.read()
        info = zipfile.ZipInfo(name, date_time=STAMP)
        info.compress_type = zipfile.ZIP_BZIP2
        z.writestr(info, data)
