#!/bin/sh
# Writes 7zz-bcj2.cb7: the five pages in source/ through BCJ2 and three LZMA
# coders, the four-stream folder the chain-only reader refused as NotAChain.
#
#   cd crates/tinker-pdf/tests/cbz && sh make-bcj2.sh
#
# 7-Zip 26.02 (x64) for Linux, `7z2602-linux-x64.tar.xz` from the ip7z/7zip
# GitHub release 26.02, SHA-256
# 41aaba7b1235304ab5aa0624530c67ae829496cd29e875925271efdccc28c03e, run as
# `7zz`, on Linux x86_64, 26 September 2026 -- the same program as this
# directory's Windows `.cb7`s, on another platform. Timestamps are left out
# (-mtm=off -mtc=off -mta=off) and the work is single-threaded, so a rerun
# writes the same bytes. Packed in make-corpus.ps1's order, which 7-Zip then
# regroups by extension within the solid block. Not run by any test: the
# committed archive is the record (ruling 13).
set -e
rm -f 7zz-bcj2.cb7
cd source
7zz a -t7z -mtm=off -mtc=off -mta=off -mmt=off \
  -m0=BCJ2 -m1=LZMA:d20 -m2=LZMA:d20 -m3=LZMA:d20 -mb0:1 -mb0s1:2 -mb0s2:3 \
  ../7zz-bcj2.cb7 page1.png page10.png page11.png page2.png page3.jpg >/dev/null
