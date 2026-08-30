# Produces the committed EPUB corpus in this directory. Nothing in this
# repository writes a byte of any `.epub` here: every book comes out of one of
# three real producers, run over text authored in `source/`.
#
#   pandoc         jgm/pandoc, GPL-2.0-or-later, the portable Windows zip.
#                  Writes EPUB 3 by default and EPUB 2 under `-t epub2`.
#   ebook-convert  calibre's converter, kovidgoyal/calibre, GPL-3.0-only.
#                  Writes EPUB 2 or 3 under `--epub-version`.
#   kcc-c2e        Kindle Comic Converter's comic-to-ebook converter,
#                  ciromattia/kcc, ISC. The only producer here that writes a
#                  fixed-layout book, and see README.md for the three that
#                  cannot.
#
# The text, the tables, the code block, the Japanese line and the four PNGs are
# all authored here, so each book is *our input through their tool* -- the
# reading `fuzz/README.md` already applies to the JPX seed corpus ("codestreams
# `opj_compress` made from our own 32 x 32 images") and gap 30's milestone 1
# applied to Windows' XPS serialisers. See README.md beside this file for why
# the alternative sources -- Project Gutenberg and `epub3-samples` -- cannot be
# committed at all.
#
# The one thing here that is not ours is the **face** the two font books
# embed, and it is deliberately the one already in this tree:
# `crates/tinker-pdf-font/data/liberation`, OFL-1.1, already on `deny.toml`'s
# allowlist and already in `THIRDPARTY.md`. Both producers embed it
# **unmodified**, which README.md explains is a licence constraint rather than
# a preference.
#
# Run it as:
#
#   pwsh -NoProfile -File crates\tinker-pdf\tests\epub\make-corpus.ps1 `
#       -Pandoc C:\tools\pandoc\pandoc.exe `
#       -EbookConvert 'C:\Program Files\Calibre2\ebook-convert.exe' `
#       -KccC2e C:\Users\you\AppData\Local\kcc_c2e.exe
#
# It is NOT reproducible byte for byte. All three producers mint a fresh UUID
# for the package document's `dc:identifier` on every run, and calibre and KCC
# stamp a `dcterms:modified` timestamp. A second run is a different file. The
# committed books are the record; this script is how they were obtained, and
# README.md records which version of which producer wrote which file, with a
# hash.

param(
    [string]$OutDir = $PSScriptRoot,
    [string]$Pandoc = $(if ($env:TINKER_PANDOC) { $env:TINKER_PANDOC } else { 'pandoc' }),
    [string]$EbookConvert = $(if ($env:TINKER_EBOOK_CONVERT) { $env:TINKER_EBOOK_CONVERT } else { 'ebook-convert' }),
    [string]$KccC2e = $(if ($env:TINKER_KCC_C2E) { $env:TINKER_KCC_C2E } else { 'kcc-c2e' })
)

$ErrorActionPreference = 'Stop'
$src = Join-Path $PSScriptRoot 'source'
$figures = Join-Path $src 'figures'
New-Item -ItemType Directory -Force -Path $OutDir, $figures | Out-Null

# ---- the pictures -----------------------------------------------------------
#
# Written byte by byte from the PNG specification rather than through GDI+, for
# two reasons: the bytes are then the same on every machine, and there is no
# question about whose image it is. Truecolour, 8 bits, no interlacing, one
# IDAT. Four different pixel sizes, because today's defect reports a page whose
# size is the picture's pixel count at one pixel to the point -- and four
# distinct sizes make that assertion say which picture.

# An eight-hex-digit literal is an Int32 in PowerShell, so `0xEDB88320` is a
# negative number and `0xFFFFFFFF` is -1. The `L` suffix makes them Int64 first,
# which is the only reason these two casts are here.
$POLY = [uint32]0xEDB88320L
$ONES = [uint32]0xFFFFFFFFL

$crcTable = New-Object 'uint32[]' 256
for ($n = 0; $n -lt 256; $n++) {
    $c = [uint32]$n
    for ($k = 0; $k -lt 8; $k++) {
        if ($c -band 1) { $c = [uint32]($POLY -bxor ($c -shr 1)) } else { $c = [uint32]($c -shr 1) }
    }
    $crcTable[$n] = $c
}

function Get-Crc32 {
    param([byte[]]$Bytes)
    $c = $ONES
    foreach ($b in $Bytes) { $c = [uint32]($crcTable[($c -bxor $b) -band 0xFF] -bxor ($c -shr 8)) }
    return [uint32]($c -bxor $ONES)
}

function Get-Adler32 {
    param([byte[]]$Bytes)
    $a = [uint32]1; $b = [uint32]0
    foreach ($x in $Bytes) { $a = ($a + $x) % 65521; $b = ($b + $a) % 65521 }
    return [uint32](($b -shl 16) -bor $a)
}

function ConvertTo-Be32 {
    param([uint32]$Value)
    # Every element is parenthesised: `,` binds tighter than `-band`, so
    # `$x -band 0xFF, $y` is `$x -band (0xFF, $y)` and does not compile.
    return [byte[]]@(
        (($Value -shr 24) -band 0xFF),
        (($Value -shr 16) -band 0xFF),
        (($Value -shr 8) -band 0xFF),
        ($Value -band 0xFF)
    )
}

function New-PngChunk {
    param([string]$Type, [byte[]]$Data)
    $t = [System.Text.Encoding]::ASCII.GetBytes($Type)
    $body = $t + $Data
    return (ConvertTo-Be32 ([uint32]$Data.Length)) + $body + (ConvertTo-Be32 (Get-Crc32 $body))
}

function New-OurPng {
    # A plain two-tone diagonal. The picture does not matter; its dimensions do.
    param([string]$Path, [int]$Width, [int]$Height, [byte]$R, [byte]$G, [byte]$B)
    $raw = New-Object System.Collections.Generic.List[byte]
    for ($y = 0; $y -lt $Height; $y++) {
        $raw.Add(0) | Out-Null   # filter type 0, None
        for ($x = 0; $x -lt $Width; $x++) {
            if ((($x * $Height) / $Width) -lt $y) { $raw.AddRange([byte[]]@($R, $G, $B)) }
            else { $raw.AddRange([byte[]]@(0xFF, 0xFF, 0xFF)) }
        }
    }
    $rawBytes = $raw.ToArray()
    $ms = New-Object System.IO.MemoryStream
    $ds = New-Object System.IO.Compression.DeflateStream($ms, [System.IO.Compression.CompressionLevel]::Optimal, $true)
    $ds.Write($rawBytes, 0, $rawBytes.Length)
    $ds.Dispose()
    # zlib wrapper: CM 8 / CINFO 7, FLEVEL 2, then the raw deflate, then Adler.
    $idat = [byte[]]@(0x78, 0x9C) + $ms.ToArray() + (ConvertTo-Be32 (Get-Adler32 $rawBytes))
    $ihdr = (ConvertTo-Be32 ([uint32]$Width)) + (ConvertTo-Be32 ([uint32]$Height)) + [byte[]]@(8, 2, 0, 0, 0)
    $png = [byte[]]@(0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A) +
           (New-PngChunk 'IHDR' $ihdr) + (New-PngChunk 'IDAT' $idat) + (New-PngChunk 'IEND' @())
    [System.IO.File]::WriteAllBytes($Path, $png)
    "  {0}  {1} x {2}  {3} bytes" -f (Split-Path $Path -Leaf), $Width, $Height, $png.Length
}

'pictures:'
New-OurPng (Join-Path $figures 'cover.png')   120 180 0xC0 0x30 0x40
New-OurPng (Join-Path $figures 'plate-a.png')  60  40 0x20 0x60 0xA0
New-OurPng (Join-Path $figures 'plate-b.png')  80  50 0xA0 0x80 0x20
New-OurPng (Join-Path $figures 'plate-c.png')  40  90 0x30 0x90 0x50

# ---- pandoc -----------------------------------------------------------------

$book = Join-Path $src 'book.md'
$plates = Join-Path $src 'figures.md'
$cover = Join-Path $figures 'cover.png'

& $Pandoc --version | Select-Object -First 1
$common = @('--standalone', '--toc', '--split-level=1', "--resource-path=$src")

& $Pandoc $book @common -o (Join-Path $OutDir 'pandoc-book-nocover.epub')
& $Pandoc $book @common "--epub-cover-image=$cover" -o (Join-Path $OutDir 'pandoc-book-cover.epub')
& $Pandoc $book @common '-t' 'epub2' "--epub-cover-image=$cover" -o (Join-Path $OutDir 'pandoc-book-epub2.epub')
& $Pandoc $plates @common -o (Join-Path $OutDir 'pandoc-plates.epub')

# ---- calibre ----------------------------------------------------------------
#
# calibre's TXT input plugin reads Markdown, but it renders a YAML metadata
# block as literal text rather than as metadata, so the block is stripped into
# a temporary copy and the same three fields are passed on the command line.
# The prose, the tables and the pictures are untouched.

& $EbookConvert --version | Select-Object -First 1
$stripped = Join-Path ([System.IO.Path]::GetTempPath()) 'tinker-epub-book.md'
$text = [System.IO.File]::ReadAllText($book)
$text = [System.Text.RegularExpressions.Regex]::Replace($text, '(?s)\A---\r?\n.*?\r?\n---\r?\n', '')
[System.IO.File]::WriteAllText($stripped, $text, (New-Object System.Text.UTF8Encoding($false)))

$meta = @(
    '--title', 'A Short Account of Containers',
    '--authors', 'The tinker-pdf authors',
    '--language', 'en',
    '--txt-in-remove-indents'
)

& $EbookConvert $stripped (Join-Path $OutDir 'calibre-book-cover.epub') @meta `
    '--epub-version' '3' '--cover' $cover
& $EbookConvert $stripped (Join-Path $OutDir 'calibre-book-nocover.epub') @meta `
    '--epub-version' '2' '--no-default-epub-cover'

Remove-Item $stripped -ErrorAction SilentlyContinue

# ---- the two books that carry a face -----------------------------------------
#
# `source/typeface.md` is short and has no code block and no table on purpose:
# a monospace run or a table caption would pull a second family in, and every
# family a producer embeds is another third of a megabyte in a corpus that was
# fifty kilobytes.
#
# The face is `LiberationSerif-Regular.ttf` and its bold, out of
# `crates/tinker-pdf-font/data/liberation` -- OFL-1.1, release 2.1.5, already
# vendored, already on `deny.toml`'s allowlist. calibre ships **the same
# release**, byte for byte, in `app/resources/fonts/liberation`, which is where
# it finds the family: this machine has no Liberation in `C:\Windows\Fonts`.
#
# `--embed-all-fonts` rather than `--embed-font-family`, and the difference is
# measured rather than assumed. `--embed-font-family "Liberation Serif"`
# embeds all four faces of the family whether or not the book uses them --
# 863 KB -- and writes a `@font-face` rule per face carrying only the
# descriptors that differ. `--embed-all-fonts` embeds the two faces the
# document actually reaches, 423 KB, and writes `font-weight`, `font-style`
# **and** `font-stretch` on every rule. Half the bytes and more descriptors, so
# it is the one committed; the other is recorded here because a reader of this
# script should not have to rediscover what it costs.
#
# **Neither is subsetted, and that is a licence constraint.**
# `--subset-embedded-fonts` takes the book to 11 KB and produces a file this
# repository may not redistribute: OFL-1.1 clause 3 forbids a Modified Version
# from using a Reserved Font Name, `AUTHORS` beside the vendored faces reads
# "with Reserved Font Name Liberation", and calibre's subsetter keeps
# `name` ID 1 as "Liberation Serif" while dropping IDs 7 to 14 -- including
# ID 13, the OFL grant clause 2 requires every redistributed copy to carry.
# Two independent breaches in one 14 KB file. README.md records it.

$typefaceStripped = Join-Path ([System.IO.Path]::GetTempPath()) 'tinker-epub-typeface.md'
$typeface = Join-Path $src 'typeface.md'
$text = [System.IO.File]::ReadAllText($typeface)
$text = [System.Text.RegularExpressions.Regex]::Replace($text, '(?s)\A---\r?\n.*?\r?\n---\r?\n', '')
[System.IO.File]::WriteAllText($typefaceStripped, $text, (New-Object System.Text.UTF8Encoding($false)))

& $EbookConvert $typefaceStripped (Join-Path $OutDir 'calibre-embedded-font.epub') `
    '--title' 'A Book That Brought Its Own Face' `
    '--authors' 'The tinker-pdf authors' `
    '--language' 'en' '--txt-in-remove-indents' `
    '--epub-version' '3' '--no-default-epub-cover' `
    '--embed-all-fonts' `
    '--extra-css' 'body { font-family: "Liberation Serif", serif; }'

Remove-Item $typefaceStripped -ErrorAction SilentlyContinue

# pandoc embeds the file named and writes its manifest entry; its own
# documentation makes the `@font-face` rule the author's, so `source/embedded.css`
# is ours and everything around it is pandoc's. Two producers rather than one,
# for the reason README.md gives about producer counts -- and here the second
# producer earns its place twice over, because pandoc does **not** rewrite the
# author's `url()` and puts the stylesheet and the face in two different
# directories, so the reference resolves against the sheet that holds it.

$face = Join-Path $PSScriptRoot '..\..\..\tinker-pdf-font\data\liberation\LiberationSerif-Regular.ttf'
$face = [System.IO.Path]::GetFullPath($face)
& $Pandoc $typeface @common `
    "--css=$(Join-Path $src 'embedded.css')" `
    "--epub-embed-font=$face" `
    -o (Join-Path $OutDir 'pandoc-embedded-font.epub')

# ---- the fixed-layout book ---------------------------------------------------
#
# KCC is a comic converter and takes a directory of pictures, so it is fed the
# same four PNGs written above rather than anything new: four pages at four
# different pixel sizes, which is what makes a per-item viewport visible.
#
# `--nokepub` because the default extension is `.kepub.epub`, which is Kobo's
# and not a name this corpus should carry. `-p KV` is KCC's default device
# profile; every picture here is smaller than its 1072 x 1448 screen and KCC
# does not upscale unless asked, so each page keeps its own dimensions.
#
# calibre cannot do this and it was tried: `ebook-convert` writes no
# `rendition:` metadata at all, in either EPUB version, and its help lists no
# fixed-layout, viewport or pre-paginated option. README.md records the route.

& $KccC2e -p KV -f EPUB --nokepub `
    -t 'Four Plates, Pre-Paginated' `
    -a 'The tinker-pdf authors' `
    --language en `
    -o (Join-Path $OutDir 'kcc-fixed-layout.epub') `
    $figures

'books:'
Get-ChildItem $OutDir -Filter '*.epub' | Sort-Object Name | ForEach-Object {
    "  {0}  {1} bytes  sha256 {2}" -f $_.Name, $_.Length, (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
}
