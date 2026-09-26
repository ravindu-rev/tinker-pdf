# Writes python-lzma.cbz: the five pages in source/, every entry ZIP method 14.
#
# CPython's zipfile is the one ZIP writer on hand that emits APPNOTE method 14
# (LZMA) at all -- 7-Zip can and is not installed here, WinRAR and .NET cannot --
# so it is the producer for the archive that holds `tinker-pdf-zip`'s method-14
# path and the LZMA decoder behind it to a real writer's bytes. The pages are
# the corpus' own, committed in source/, so the decoded entries have an exact
# expected answer that no decoder produced: the files that went in.
#
#   cd crates/tinker-pdf/tests/cbz && python3 make-lzma.py
#
# Every entry carries one fixed timestamp rather than its file's modification
# time, which is what makes a rerun under the same CPython and liblzma write the
# same bytes; README.md records both versions and the hash. Not run by any test:
# the committed archive is the record (ruling 13).

import zipfile

# The order make-corpus.ps1 packs in, which is deliberately not reading order.
ORDER = ["page1.png", "page10.png", "page11.png", "page2.png", "page3.jpg"]
STAMP = (2026, 9, 26, 0, 0, 0)

with zipfile.ZipFile("python-lzma.cbz", "w") as z:
    for name in ORDER:
        with open("source/" + name, "rb") as f:
            data = f.read()
        info = zipfile.ZipInfo(name, date_time=STAMP)
        info.compress_type = zipfile.ZIP_LZMA
        z.writestr(info, data)
