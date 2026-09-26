# Writes input/: the files every archive in this directory was made from.
#
# They are this repository's own bytes, made by arithmetic rather than taken
# from anywhere, and each is shaped to reach one part of a coder that the five
# comic pages in crates/tinker-pdf/tests/cbz/source/ cannot:
#
#   x86.bin    16 384 bytes for the x86 filters: thirty-six operands built
#              so BCJ converts them twice, a kilobyte of E8/E9/00/FF soup for
#              its previous-byte mask, then x86-shaped code -- E8 calls and E9
#              jumps whose rel32 targets land inside the file (so the high byte
#              is 00 or FF and a BCJ encoder converts them), 0F 8x conditional
#              jumps (which BCJ2 converts and BCJ does not), E8s whose fourth
#              operand byte is neither 00 nor FF (which neither converts) --
#              and an E8 in the last four bytes, which no encoder may touch.
#   prose.txt  About 120 KB of word-salad English over a fixed vocabulary:
#              long enough that bzip2 at level 1 cuts it into two blocks, and
#              repetitive enough that PPMd reaches its high orders.
#   runs.bin   Runs of one byte, of lengths chosen to sit either side of every
#              threshold a run coder has: 1 to 5 (bzip2's RLE1 starts at 4),
#              255 to 260 (its run byte tops out at 251 past the four), and
#              long ones; then every byte value once.
#   empty.txt  Nothing, which a ZIP writer still compresses into a stream.
#
#   cd crates/tinker-pdf-archive/tests/coders && python3 make-inputs.py
#
# Deterministic: a 64-bit LCG with fixed seeds and no clock, so a rerun writes
# the same bytes on any Python 3. The committed files are the record, and the
# tests hold every decoded entry to them byte for byte.

import os

M64 = (1 << 64) - 1


def lcg(seed):
    state = seed & M64
    while True:
        state = (state * 6364136223846793005 + 1442695040888963407) & M64
        yield state >> 33


def x86(size=16384):
    r = lcg(0x86)
    out = bytearray()
    # First, the three shapes that make BCJ convert an operand *twice*: a
    # candidate one, two or three bytes after an E8 that was skipped, whose
    # converted address then has 00 or FF in the byte that skipped E8's operand
    # would have ended on. Each depends on the address it sits at, so each is
    # built for its own offset. Nothing random reaches this often enough.
    for _ in range(12):
        out += b"\x90" * 4
        out += bytes([0xE8, 0xE8, 0xFF, 0xFF, 0xFE, 0x00])
        out += b"\x90" * 4
        out += bytes([0xE8, 0x90, 0xE8, 0xFF, 0xFE, next(r) & 0x7F, 0x00])
        out += b"\x90" * 4
        while (len(out) + 3 + 5) & 0xFF == 0:
            out.append(0x90)
        low = (len(out) + 3 + 5) & 0xFF
        first = (0xFF - low) & 0xFF
        if first in (0x00, 0xFF):
            first = (0x100 - low) & 0xFF
        out += bytes([0xE8, 0x90, 0x90, 0xE8, first, next(r) & 0x7F, next(r) & 0x7F, 0x00])
    # Then 1 KiB of branch-byte soup: E8, E9, 00 and FF at a density no
    # compiler emits, so candidates sit one to three bytes apart and BCJ's
    # previous-byte mask is non-zero at conversions. Without it the mask's
    # ageing and its second conversion are reached by nothing a real encoder
    # wrote (an injection campaign measured both at zero).
    soup = [0xE8, 0xE9, 0x00, 0xFF]
    for _ in range(1024):
        pick = next(r) % 6
        out.append(soup[pick] if pick < 4 else next(r) & 0xFF)
    filler = [0x48, 0x89, 0x8B, 0x83, 0xC3, 0x55, 0x5D, 0x90, 0x31, 0xC0,
              0x45, 0x85, 0xF6, 0x74, 0x75, 0x4C, 0x8D, 0x0F, 0xB6, 0x41]

    def rel32(pos, length):
        # A target inside the file, so the displacement is small and its top
        # byte is 00 or FF: what a real call looks like and what BCJ converts.
        target = next(r) % size
        return ((target - (pos + length)) & 0xFFFFFFFF).to_bytes(4, "little")

    while len(out) < size - 16:
        pos = len(out)
        kind = next(r) % 16
        if kind < 7:
            for _ in range(1 + next(r) % 6):
                out.append(filler[next(r) % len(filler)])
        elif kind < 10:
            out.append(0xE8)
            out += rel32(pos, 5)
        elif kind < 11:
            out.append(0xE9)
            out += rel32(pos, 5)
        elif kind < 12:
            out += bytes([0x0F, 0x80 | (next(r) % 16)])
            out += rel32(pos, 6)
        elif kind < 13:
            # An E8 whose operand is not an address: the top byte is neither
            # 00 nor FF, so no x86 filter converts it.
            out.append(0xE8)
            out += bytes([next(r) & 0xFF, next(r) & 0xFF, next(r) & 0xFF, 0x12 + next(r) % 0x80])
        elif kind < 14:
            # E8 and E9 bytes close together: the previous-byte mask.
            for _ in range(2 + next(r) % 3):
                out.append(0xE8 + next(r) % 2)
            out += bytes([0x00, 0x00, 0x00])
        elif kind < 15:
            out += bytes([0xEB if next(r) % 2 else 0x70 + next(r) % 16, next(r) & 0xFF])
        else:
            out += bytes(next(r) & 0xFF for _ in range(1 + next(r) % 8))
    while len(out) < size - 3:
        out.append(0x90)
    # An E8 inside the last four bytes, which an encoder leaves alone because
    # the operand runs off the end.
    out += bytes([0xE8, 0x00, 0x00])
    assert len(out) == size
    return bytes(out)


WORDS = """
the a an of to in and or but for with on at by from into over under between
page panel comic archive reader writer block stream folder coder filter frame
window table symbol context order model range probability escape binary tree
bit byte length distance literal match offset header footer entry name size
check sum value number count limit bound cap ceiling floor edge corner margin
quiet brown fox jumps lazy dog river mountain valley city harbour lantern
morning evening winter summer autumn spring north south east west inside out
read write copy move keep hold drop take give find lose open close begin end
first second third last next previous early late long short wide narrow deep
is was are were be been being has had have do does did will would can could
""".split()


def prose(target=120_000):
    r = lcg(0x7E57)
    out = []
    line = []
    width = 0
    total = 0
    while total < target:
        words = 4 + next(r) % 14
        sentence = [WORDS[next(r) % len(WORDS)] for _ in range(words)]
        sentence[0] = sentence[0].capitalize()
        sentence[-1] += "." if next(r) % 5 else ","
        for word in sentence:
            if width + len(word) + 1 > 72:
                text = " ".join(line) + "\n"
                out.append(text)
                total += len(text)
                line, width = [], 0
            line.append(word)
            width += len(word) + 1
        if next(r) % 9 == 0:
            text = " ".join(line) + "\n\n"
            out.append(text)
            total += len(text)
            line, width = [], 0
    out.append(" ".join(line) + "\n")
    return "".join(out).encode("ascii")


def runs():
    r = lcg(0x4E5)
    out = bytearray()
    lengths = [1, 2, 3, 4, 5, 6, 250, 251, 254, 255, 256, 257, 258, 259, 260, 261, 1000, 4096]
    for length in lengths:
        out += bytes([next(r) & 0xFF]) * length
    for _ in range(600):
        out += bytes([next(r) & 0xFF]) * (1 + next(r) % 12)
    out += bytes(range(256))
    return bytes(out)


os.makedirs("input", exist_ok=True)
for name, data in [
    ("x86.bin", x86()),
    ("prose.txt", prose()),
    ("runs.bin", runs()),
    ("empty.txt", b""),
]:
    with open(os.path.join("input", name), "wb") as f:
        f.write(data)
    print(f"{name:10} {len(data):7} bytes")
