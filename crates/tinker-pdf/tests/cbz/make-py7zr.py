# Writes the .cb7s py7zr made: the five pages in source/, each archive one 7z
# coder this directory had no real writer's archive of.
#
#   py7zr-bcj.cb7    filters [BCJ x86, LZMA2]: coder 03030103 in front of 21
#
# py7zr is the second 7z writer this directory never had -- every .cb7 before
# these was 7-Zip 26.02 -- and it is a separate implementation of the container
# (its own header writer, and it lists a folder's coders in the opposite order
# from 7-Zip's), so these archives also answer tests/cbz/README.md's "a second
# real archiver's 7z".
#
#   cd crates/tinker-pdf/tests/cbz && python3 make-py7zr.py
#
# py7zr stamps every entry with the current time, so its clock is pinned to one
# instant for the run; a rerun under the same py7zr writes the same bytes, and
# README.md records the version and the hashes. Packed in make-corpus.ps1's
# order, not reading order. Not run by any test: the committed archives are the
# record (ruling 13).

import datetime

import py7zr
import py7zr.helpers

ORDER = ["page1.png", "page10.png", "page11.png", "page2.png", "page3.jpg"]
INSTANT = datetime.datetime(2026, 9, 26, tzinfo=datetime.timezone.utc).timestamp()
py7zr.helpers.ArchiveTimestamp.from_now = classmethod(lambda cls: cls.from_datetime(INSTANT))


def write(path, filters):
    with py7zr.SevenZipFile(path, "w", filters=filters) as z:
        for name in ORDER:
            with open("source/" + name, "rb") as f:
                z.writestr(f.read(), name)


write("py7zr-bcj.cb7", [{"id": py7zr.FILTER_X86}, {"id": py7zr.FILTER_LZMA2}])
