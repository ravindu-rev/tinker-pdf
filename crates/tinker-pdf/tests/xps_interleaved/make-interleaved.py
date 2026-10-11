# Writes wpf-image-and-text-pieces.xps: a committed WPF package with six of its
# eight items split into OPC interleaved pieces (ECMA-376 Part 2, 7.2.4).
#
# No producer on hand writes an interleaved package -- WPF, the XPS object
# model and Ghostscript all write whole parts, and so does every package in
# ../xps -- so this one is derived: the bytes of every part are exactly
# ../xps/wpf-image-and-text.xps's, and only the container changes. Each split
# item becomes `<item>/[0].piece` ... `<item>/[n].last.piece` in equal-sized
# chunks, and the pieces of different items are written round-robin, so the
# archive order interleaves six parts at once. `[Content_Types].xml` is split
# too, since 7.2.4 lets it be. Pieces keep their source item's method: the
# PNG's are stored, everything else is deflated.
#
#   cd crates/tinker-pdf/tests/xps_interleaved && python3 make-interleaved.py
#
# One fixed timestamp, so a rerun under the same CPython writes the same bytes.
# Not run by any test: the committed package is the record (ruling 13).

import zipfile

SOURCE = "../xps/wpf-image-and-text.xps"
OUT = "wpf-image-and-text-pieces.xps"
STAMP = (2026, 9, 26, 0, 0, 0)
SPLIT = {
    "[Content_Types].xml": 3,
    "_rels/.rels": 2,
    "Documents/1/Pages/1.fpage": 4,
    "Documents/1/Pages/_rels/1.fpage.rels": 2,
    "Resources/234b4a6b-25d1-4c77-bd16-cfcaa91dbca9.png": 2,
    "Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF": 5,
}

source = zipfile.ZipFile(SOURCE)
queues = []
for info in source.infolist():
    data = source.read(info.filename)
    count = SPLIT.get(info.filename, 1)
    if count == 1:
        queues.append([(info.filename, data, info.compress_type)])
        continue
    size = -(-len(data) // count)
    pieces = []
    for k in range(count):
        suffix = "[%d].last.piece" % k if k == count - 1 else "[%d].piece" % k
        chunk = data[k * size : (k + 1) * size]
        assert chunk, "every piece holds bytes"
        pieces.append((info.filename + "/" + suffix, chunk, info.compress_type))
    queues.append(pieces)

with zipfile.ZipFile(OUT, "w") as out:
    while any(queues):
        for queue in queues:
            if queue:
                name, data, method = queue.pop(0)
                entry = zipfile.ZipInfo(name, date_time=STAMP)
                entry.compress_type = method
                out.writestr(entry, data)
