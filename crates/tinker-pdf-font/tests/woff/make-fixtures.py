#!/usr/bin/env python3
"""Packs this repository's own face into WOFF 1.0 and WOFF 2.0.

Run from the repository root:

    cargo run -p xtask -- synth-face
    python crates/tinker-pdf-font/tests/woff/make-fixtures.py

Nothing here runs at test time. This writes seven files once, they are
committed, and `tests/woff_fixtures.rs` reads them with no interpreter
anywhere near it.

Why this script exists at all
============================

`docs/features/epub.md` carried, for as long as the WOFF row was open, the
sentence "No WOFF or WOFF2 file is committed: no producer available emits
one". Two things were true behind it. No producer in the corpus tooling emits
a web font -- pandoc, calibre and KCC all embed the sfnt they were handed. And
repacking a *vendored* face is barred: OFL-1.1 reserves the font name, and a
`Liberation Serif` that had been through a re-encoder is a modified version
under a reserved name.

Neither applies to a face this repository wrote. `cargo xtask synth-face`
writes `target/fonts/tinker-synthetic-2.ttf` -- every glyph from 32 up the
same filled box, three tables, no licence on it but this project's. That face
can be packed, and this packs it.

Ruling 13, and which half of it fontTools is
============================================

Ruling 13 says no program outside this repository may **adjudicate** a
document. It has never said one may not **supply** one: `tests/brotli/` is
forty streams Node's zlib emitted on a named day, `tests/epub/` is nine books
three real producers wrote, and both are read by first-party assertions.

fontTools is here on the supplying side and only there. It packs bytes; it is
never asked whether an answer is right. The test that reads these files
compares this decoder's output against **this repository's own source face**,
which is committed beside them -- no interpreter is spawned at test time, no
reference decoder is consulted, and if fontTools and this build disagree about
what the container holds, the source face settles it and fontTools gets no
vote.

Two producers per format, on purpose
====================================

One encoder's output tests one encoder's reading of the specification. So each
format is written twice, by implementations with no code in common:

* WOFF 1.0 -- fontTools 4.63.0 (`flavor="woff"`, Python) and `ttf2woff` 3.0.0
  (JavaScript, over pako).
* WOFF 2.0 -- fontTools 4.63.0 (`flavor="woff2"`, a pure-Python re-encoder)
  and `wawoff2` 2.0.1, which is Google's reference `woff2` C++ encoder
  compiled to WebAssembly.

They differ in ways a decoder has to survive: the two disagree about the
physical order of the tables, and about whether the `hmtx` transform applies.

The face this builds, and why it is not just the square
=======================================================

`synth-face`'s glyf is one contour of four on-curve points, repeated. Packed
through WOFF2's `glyf` transform that exercises almost nothing: no off-curve
flag, no composite, no instruction, no bounding box that has to be stored, and
every triplet in the one-byte form.

So the square is kept for glyphs 32..255 -- this is still that face, and the
test checks the square is still a square -- and seven glyphs are appended that
reach the rest of the transform:

  tp.twocontours  two contours, and the only glyph carrying instructions
  tp.curved       off-curve points, so the flag bit is set
  tp.wide         deltas past the one-byte triplet forms, both signs
  tp.offcurve     consecutive off-curve points, an implied on-curve midpoint
  tp.empty        nContours == 0
  tp.composite    two components, ARGS_ARE_XY_VALUES with word arguments
  tp.composite2   one scaled component, a wider component record

`hmtx` is written so most glyphs have `lsb == xMin` and one in seven does not,
which is what decides whether an encoder may apply transform 1 -- so neither
encoder applies it, and the reverse would have no fixture at all. Hence the
seventh file: `synthetic-2-aligned.ttf` is the same face with the bearings
aligned, and `synthetic-2-aligned-hmtx.woff2` is fontTools packing it with
`woff2TransformedTableTags` widened to include `hmtx`. The setting is unusual
and the bytes are still a real encoder's.

The square's *outline* is what survives, not its bytes. `synth-face` writes
every delta as a word; fontTools' compiler re-encodes the small ones as bytes,
so `synthetic-2.ttf` is smaller than the face it came from and holds the same
shape. That is the property the test asserts about it, and the same property
it asserts about both containers.

Determinism
===========

`head.created` and `head.modified` are pinned to zero, so re-running this
writes the same bytes. Nothing here reads a clock, an environment variable
that affects the output, or a hash-map iteration order.
"""

import os
import subprocess
import sys

from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable, woff2
from fontTools.ttLib.tables import ttProgram
from fontTools.ttLib.tables.O_S_2f_2 import Panose
from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
SOURCE = os.path.join(ROOT, "target", "fonts", "tinker-synthetic-2.ttf")

# ---- the synthetic face --------------------------------------------------


def source_face():
    """`synth-face`'s bytes, opened directly.

    Until 2026-08-31 this had to bolt a `maxp` onto the face before fontTools
    would open it: `synthetic-1` had three tables and none of them carried
    `numGlyphs`, so `loca` could not be interpreted and fontTools declined the
    file. `xtask::face` was fixed rather than worked around, and the twenty
    lines that did the bolting are gone with it.
    """
    if not os.path.exists(SOURCE):
        sys.exit("run `cargo run -p xtask -- synth-face` first: %s" % SOURCE)
    font = TTFont(SOURCE, recalcTimestamp=False)
    return font["maxp"].numGlyphs


# ---- the seven glyphs that reach the rest of the transform ------------------


def drawn(draw):
    pen = TTGlyphPen(None)
    draw(pen)
    return pen.glyph()


def two_contours(p):
    p.moveTo((0, 0))
    p.lineTo((600, 0))
    p.lineTo((600, 600))
    p.lineTo((0, 600))
    p.closePath()
    p.moveTo((150, 150))
    p.lineTo((150, 450))
    p.lineTo((450, 450))
    p.lineTo((450, 150))
    p.closePath()


def curved(p):
    p.moveTo((0, 0))
    p.qCurveTo((300, 400), (600, 0))
    p.qCurveTo((700, -300), (0, 0))
    p.closePath()


def wide(p):
    p.moveTo((-2000, -1500))
    p.lineTo((2000, -1500))
    p.lineTo((2000, 1500))
    p.lineTo((-2000, 1500))
    p.closePath()


def all_offcurve(p):
    p.qCurveTo((0, 500), (500, 500), (500, 0), None)
    p.closePath()


def build(num_glyphs, out):
    # `recalcTimestamp=False` or the whole exercise is not reproducible:
    # fontTools stamps `head.modified` with the clock on every compile, which
    # moves the table, moves the Brotli stream, and gives a different file
    # every run. Pinning `head.created`/`head.modified` below is necessary and
    # on its own not sufficient, because the stamp happens after that.
    font = TTFont(SOURCE, recalcTimestamp=False)
    glyf = font["glyf"]
    order = list(font.getGlyphOrder())

    extra = {
        "tp.twocontours": drawn(two_contours),
        "tp.curved": drawn(curved),
        "tp.wide": drawn(wide),
        "tp.offcurve": drawn(all_offcurve),
    }
    empty = Glyph()
    empty.numberOfContours = 0
    extra["tp.empty"] = empty

    for name in sorted(extra):
        glyf.glyphs[name] = extra[name]
        order.append(name)

    composite = Glyph()
    composite.numberOfContours = -1
    composite.components = []
    for dx, dy, base in ((0, 0, "tp.curved"), (700, 200, "tp.twocontours")):
        part = GlyphComponent()
        part.glyphName = base
        part.x, part.y = dx, dy
        # ARG_1_AND_2_ARE_WORDS | ARGS_ARE_XY_VALUES.
        part.flags = 0x0001 | 0x0002
        composite.components.append(part)
    glyf.glyphs["tp.composite"] = composite
    order.append("tp.composite")

    scaled = Glyph()
    scaled.numberOfContours = -1
    part = GlyphComponent()
    part.glyphName = "tp.wide"
    part.x, part.y = -100, 50
    part.flags = 0x0001 | 0x0002
    part.scale = (0.5,)
    scaled.components = [part]
    glyf.glyphs["tp.composite2"] = scaled
    order.append("tp.composite2")

    # Instructions on one simple glyph, so `instructionStream` is not empty.
    # Nothing in this tree executes them; five real opcodes rather than noise,
    # so a reader who disassembles them finds a program.
    program = ttProgram.Program()
    program.fromBytecode(bytes([0xB0, 0x00, 0xB0, 0x01, 0x2F]))
    glyf.glyphs["tp.twocontours"].program = program  # noqa: set before expand

    font.setGlyphOrder(order)
    glyf.glyphOrder = order
    # Through `glyf[name]` rather than `glyf.glyphs[name]`: the former expands
    # a glyph still held as the bytes it was read from, and the square arrives
    # that way for all 256 of the glyphs that came out of `synth-face`.
    for name in order:
        glyf[name].recalcBounds(glyf)

    count = len(order)

    maxp = newTable("maxp")
    maxp.tableVersion = 0x00010000
    maxp.numGlyphs = count
    maxp.maxPoints = 64
    maxp.maxContours = 8
    maxp.maxCompositePoints = 64
    maxp.maxCompositeContours = 8
    maxp.maxZones = 2
    maxp.maxTwilightPoints = 16
    maxp.maxStorage = 16
    maxp.maxFunctionDefs = 16
    maxp.maxInstructionDefs = 0
    maxp.maxStackElements = 64
    maxp.maxSizeOfInstructions = 16
    maxp.maxComponentElements = 2
    maxp.maxComponentDepth = 2
    font["maxp"] = maxp

    # One glyph in seven is given an lsb that is not its xMin. WOFF2 lets an
    # encoder drop the left side bearings only when every one of them equals
    # the glyph's xMin, so this is what decides whether `hmtx` transform 1 is
    # legal -- and the two encoders here decide it differently.
    hmtx = newTable("hmtx")
    hmtx.metrics = {}
    for index, name in enumerate(order):
        x_min = getattr(glyf[name], "xMin", 0)
        hmtx.metrics[name] = (900, x_min if index % 7 else x_min + 3)
    font["hmtx"] = hmtx

    hhea = newTable("hhea")
    hhea.tableVersion = 0x00010000
    hhea.ascent, hhea.descent, hhea.lineGap = 800, -200, 0
    hhea.advanceWidthMax = 900
    hhea.minLeftSideBearing = -2000
    hhea.minRightSideBearing = -1100
    hhea.xMaxExtent = 2000
    hhea.caretSlopeRise, hhea.caretSlopeRun, hhea.caretOffset = 1, 0, 0
    hhea.reserved0 = hhea.reserved1 = hhea.reserved2 = hhea.reserved3 = 0
    hhea.metricDataFormat = 0
    hhea.numberOfHMetrics = count
    font["hhea"] = hhea

    mapping = {code: order[code] for code in range(32, num_glyphs)}
    appended = sorted(extra) + ["tp.composite", "tp.composite2"]
    for offset, name in enumerate(appended):
        mapping[0x2400 + offset] = name
    subtable = CmapSubtable.newSubtable(4)
    subtable.platformID, subtable.platEncID, subtable.language = 3, 1, 0
    subtable.cmap = mapping
    cmap = newTable("cmap")
    cmap.tableVersion = 0
    cmap.tables = [subtable]
    font["cmap"] = cmap

    name_table = newTable("name")
    name_table.names = []
    for name_id, text in ((1, "Tinker Synthetic"), (2, "Regular"),
                          (3, "tinker-pdf synthetic-2"),
                          (4, "Tinker Synthetic"),
                          (6, "TinkerSynthetic-Regular")):
        name_table.setName(text, name_id, 3, 1, 0x409)
    font["name"] = name_table

    os2 = newTable("OS/2")
    os2.version = 4
    os2.xAvgCharWidth = 900
    os2.usWeightClass, os2.usWidthClass, os2.fsType = 400, 5, 0
    os2.ySubscriptXSize = os2.ySubscriptYSize = 650
    os2.ySubscriptXOffset, os2.ySubscriptYOffset = 0, 140
    os2.ySuperscriptXSize = os2.ySuperscriptYSize = 650
    os2.ySuperscriptXOffset, os2.ySuperscriptYOffset = 0, 480
    os2.yStrikeoutSize, os2.yStrikeoutPosition = 50, 250
    os2.sFamilyClass = 0
    os2.panose = Panose()
    os2.ulUnicodeRange1 = os2.ulUnicodeRange2 = 0
    os2.ulUnicodeRange3 = os2.ulUnicodeRange4 = 0
    os2.achVendID = "TPDF"
    os2.fsSelection = 0x40
    os2.usFirstCharIndex, os2.usLastCharIndex = 32, 0x2408
    os2.sTypoAscender, os2.sTypoDescender, os2.sTypoLineGap = 800, -200, 0
    os2.usWinAscent, os2.usWinDescent = 800, 200
    os2.ulCodePageRange1, os2.ulCodePageRange2 = 1, 0
    os2.sxHeight, os2.sCapHeight = 500, 700
    os2.usDefaultChar, os2.usBreakChar, os2.usMaxContext = 0, 32, 1
    font["OS/2"] = os2

    post = newTable("post")
    post.formatType = 3.0
    post.italicAngle = 0
    post.underlinePosition, post.underlineThickness = -100, 50
    post.isFixedPitch = 0
    post.minMemType42 = post.maxMemType42 = 0
    post.minMemType1 = post.maxMemType1 = 0
    font["post"] = post

    head = font["head"]
    # `synth-face` writes `head` as 54 zero bytes with two fields poked into
    # it, so its `tableVersion` and `magicNumber` are zero -- which no reader
    # in this tree checks and which makes the face invalid all the same. Both
    # are set here, and `tableVersion` matters twice: `ttf2woff` takes the WOFF
    # `flavor` from the first four bytes of `head` rather than of the font, so
    # a zero there becomes a WOFF announcing a font format that does not exist.
    head.tableVersion = 1.0
    head.unitsPerEm = 1000
    # Pinned, so re-running writes the same bytes.
    head.created = head.modified = 0
    head.flags, head.macStyle, head.lowestRecPPEM = 3, 0, 8
    head.fontDirectionHint = 2
    head.indexToLocFormat = 1
    head.glyphDataFormat = 0
    head.magicNumber = 0x5F0F3CF5
    head.fontRevision = 1.0

    font.save(out)
    return count


INDEPENDENT_JS = r"""// Written and deleted by make-fixtures.py; not committed.
const fs = require("fs");
const path = require("path");
const ttf2woff = require("ttf2woff");
const wawoff2 = require("wawoff2");

const source = fs.readFileSync(process.argv[2]);
const out = process.argv[3];

const woff = Buffer.from(ttf2woff(new Uint8Array(source)).buffer);
fs.writeFileSync(path.join(out, "synthetic-2-ttf2woff.woff"), woff);
console.log("ttf2woff  woff  -> synthetic-2-ttf2woff.woff (" + woff.length + ")");

wawoff2.compress(source).then((packed) => {
  const buf = Buffer.from(packed);
  fs.writeFileSync(path.join(out, "synthetic-2-wawoff2.woff2"), buf);
  console.log("wawoff2   woff2 -> synthetic-2-wawoff2.woff2 (" + buf.length + ")");
});
"""


PROVENANCE_HEADER = "file\tbytes\tproducer\tversion\tfrom\twritten\trole\n"

# One row per committed file. `tests/woff_fixtures.rs` asserts that this names
# every file in the directory and that every row names a file of that size, so
# a fixture that is regenerated and a record that is not cannot both survive.
PROVENANCE_ROWS = [
    ("synthetic-2.ttf", "fontTools", "4.63.0", "cargo xtask synth-face",
     "the source face: synth-face's 256 glyphs plus seven that reach the rest "
     "of WOFF2's glyf transform"),
    ("synthetic-2.woff", "fontTools", "4.63.0", "synthetic-2.ttf",
     "WOFF 1.0, table order preserved, so the round trip is byte identity"),
    ("synthetic-2-ttf2woff.woff", "ttf2woff", "3.0.0", "synthetic-2.ttf",
     "WOFF 1.0 from a second encoder, which sorts its tables"),
    ("synthetic-2.woff2", "fontTools", "4.63.0", "synthetic-2.ttf",
     "WOFF 2.0; loca sits four entries after glyf, which 5.5 permits"),
    ("synthetic-2-wawoff2.woff2", "wawoff2", "2.0.1", "synthetic-2.ttf",
     "WOFF 2.0 from Google's reference C++ encoder built to WebAssembly"),
    ("synthetic-2-aligned.ttf", "fontTools", "4.63.0", "synthetic-2.ttf",
     "the same outlines with every lsb equal to its xMin"),
    ("synthetic-2-aligned-hmtx.woff2", "fontTools", "4.63.0",
     "synthetic-2-aligned.ttf",
     "WOFF 2.0 with hmtx transform 1, which no encoder applies unasked"),
]

# Ruling 13, restated where a reader of the record will see it rather than only
# in this file's header.
PROVENANCE_NOTE = (
    "# Written by make-fixtures.py. Every producer named here GENERATED a\n"
    "# file and none of them ADJUDICATES one (ruling 13): no program runs at\n"
    "# test time, and tests/woff_fixtures.rs compares this build against\n"
    "# synthetic-2.ttf, which is committed beside the containers.\n")


def write_provenance(written):
    """The record, regenerated with the files so the two cannot disagree."""
    path = os.path.join(HERE, "PROVENANCE.tsv")
    with open(path, "w", encoding="utf-8", newline="\n") as out:
        out.write(PROVENANCE_NOTE)
        out.write(PROVENANCE_HEADER)
        for name, producer, version, source, role in PROVENANCE_ROWS:
            full = os.path.join(HERE, name)
            if not os.path.exists(full):
                sys.exit("PROVENANCE_ROWS names %s and it was not written"
                         % name)
            out.write("%s\t%d\t%s\t%s\t%s\t%s\t%s\n"
                      % (name, os.path.getsize(full), producer, version,
                         source, written, role))
    print("provenance: PROVENANCE.tsv (%d rows)" % len(PROVENANCE_ROWS))


def main():
    num_glyphs = source_face()
    ttf = os.path.join(HERE, "synthetic-2.ttf")
    count = build(num_glyphs, ttf)
    print("source face: synthetic-2.ttf (%d glyphs, %d bytes)"
          % (count, os.path.getsize(ttf)))

    for flavor, name in (("woff", "synthetic-2.woff"),
                         ("woff2", "synthetic-2.woff2")):
        font = TTFont(ttf, recalcTimestamp=False)
        font.flavor = flavor
        out = os.path.join(HERE, name)
        font.save(out)
        print("fontTools %-5s -> %s (%d bytes)"
              % (flavor, name, os.path.getsize(out)))

    # ---- the hmtx transform, which needs a face that qualifies for it -------
    #
    # WOFF2 §5.4 lets an encoder delete the left side bearings only when every
    # one of them equals the glyph's xMin, and `synthetic-2.ttf` is written so
    # one in seven does not. So neither encoder applies the transform to it,
    # and a decoder with no fixture behind that branch has a reverse nobody has
    # ever run.
    #
    # This writes the same face with the bearings aligned -- identical
    # outlines, identical advances, `lsb == xMin` throughout -- and asks
    # fontTools for the transform explicitly. Even then it has to be asked:
    # `woff2TransformedTableTags` defaults to ('glyf', 'loca'), and the
    # reference encoder declines this face too. A real producer wrote the
    # bytes; what is unusual is the setting, not the file.
    aligned = os.path.join(HERE, "synthetic-2-aligned.ttf")
    font = TTFont(ttf, recalcTimestamp=False)
    glyf, hmtx = font["glyf"], font["hmtx"]
    for name in font.getGlyphOrder():
        advance, _lsb = hmtx.metrics[name]
        hmtx.metrics[name] = (advance, getattr(glyf[name], "xMin", 0))
    font.save(aligned)
    print("aligned face: synthetic-2-aligned.ttf (%d bytes)"
          % os.path.getsize(aligned))

    woff2.woff2TransformedTableTags = ("glyf", "loca", "hmtx")
    font = TTFont(aligned, recalcTimestamp=False)
    font.flavor = "woff2"
    out = os.path.join(HERE, "synthetic-2-aligned-hmtx.woff2")
    font.save(out)
    woff2.woff2TransformedTableTags = ("glyf", "loca")
    print("fontTools woff2 -> synthetic-2-aligned-hmtx.woff2 (%d bytes, hmtx "
          "transform 1)" % os.path.getsize(out))

    # The two independent encoders live in a node_modules this repository does
    # not carry; point at one with WOFF_NODE_MODULES to rewrite their files.
    # Without it this leaves the committed pair alone rather than deleting it.
    node = os.environ.get("WOFF_NODE_MODULES")
    if not node:
        print("WOFF_NODE_MODULES unset: the two independent files are left as "
              "committed")
        return
    # Beside the `node_modules` and not beside this file: node resolves
    # `require` from the script's own directory, so a script left here would
    # find neither package.
    script = os.path.join(node, "_independent.js")
    open(script, "w", encoding="utf-8", newline="\n").write(INDEPENDENT_JS)
    subprocess.run(["node", script, ttf, HERE], cwd=node, check=True)
    os.remove(script)

    # Last, so it records the sizes of the files that were just written. The
    # date is an argument rather than a clock: re-running this on a later day
    # must not silently restate when the committed bytes were made.
    write_provenance(os.environ.get("WOFF_WRITTEN", "2026-08-31"))


if __name__ == "__main__":
    main()
