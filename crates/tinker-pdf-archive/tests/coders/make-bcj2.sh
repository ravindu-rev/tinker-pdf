#!/bin/sh
# Writes 7zz-bcj2.7z: x86.bin and prose.txt through BCJ2, the four-stream x86
# converter, in 7-Zip's own Linux build -- the only writer of BCJ2 there is --
# and the `sevenz` fuzz target's `bcj2` seed, the same chain over 768 bytes
# of branches written for it (below).
#
# BCJ2 splits its input into four streams: s0 the main bytes, s1 the CALL
# targets, s2 the JMP and Jcc targets, and s3 a range-coded stream of the
# decisions it made. The chain below is the one 7-Zip's manual gives: s0, s1
# and s2 each LZMA-compressed (bound to coders 1, 2 and 3), s3 packed as it is.
#
#   cd crates/tinker-pdf-archive/tests/coders && sh make-bcj2.sh
#
# 7-Zip 26.02 (x64) for Linux, `7z2602-linux-x64.tar.xz` from the ip7z/7zip
# GitHub release 26.02, SHA-256
# 41aaba7b1235304ab5aa0624530c67ae829496cd29e875925271efdccc28c03e, run as
# `7zz`. Timestamps are left out of the archive (-mtm=off -mtc=off -mta=off)
# and the work is single-threaded, so a rerun writes the same bytes. Not run
# by any test: the committed archive is the record (ruling 13).
set -e
CHAIN="-m0=BCJ2 -m1=LZMA:d20 -m2=LZMA:d20 -m3=LZMA:d20 -mb0:1 -mb0s1:2 -mb0s2:3"
FLAGS="-t7z -mtm=off -mtc=off -mta=off -mmt=off"
rm -f 7zz-bcj2.7z
(cd input && 7zz a $FLAGS $CHAIN ../7zz-bcj2.7z x86.bin prose.txt >/dev/null)

seed=$(mktemp -d)
# 7-Zip's BCJ2 converts a branch only when its target lands inside the file
# it is coding, and a 768-byte slice of x86.bin has few whose do; so the seed
# is 768 bytes of calls, jumps and conditional jumps written for it, each
# target inside those 768 bytes, between filler bytes.
python3 - "$seed/x86.bin" <<'EOF'
import sys
out, state = bytearray(), 0x5EED
def rand():
    global state
    state = (state * 6364136223846793005 + 1442695040888963407) & (2**64 - 1)
    return state >> 33
while len(out) < 740:
    kind = rand() % 4
    opcode = [b"\xE8", b"\xE9", bytes([0x0F, 0x80 | rand() % 16]), b""][kind]
    if opcode:
        target = rand() % 768
        out += opcode + ((target - (len(out) + len(opcode) + 4)) & 0xFFFFFFFF).to_bytes(4, "little")
    else:
        out += bytes([0x48, 0x89, 0xE5, 0x90, 0xC3][rand() % 5] for _ in range(1 + rand() % 4))
out += b"\x90" * (768 - len(out))
open(sys.argv[1], "wb").write(bytes(out[:768]))
EOF
(cd "$seed" && 7zz a $FLAGS $CHAIN seed.7z x86.bin >/dev/null)
{ printf '\377'; cat "$seed/seed.7z"; } > ../../../../fuzz/corpus/sevenz/bcj2
rm -rf "$seed"
