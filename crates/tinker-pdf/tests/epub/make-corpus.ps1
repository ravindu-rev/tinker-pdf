# Produces the committed EPUB corpus in this directory. Nothing in this
# repository writes a byte of any `.epub` here: every book comes out of one of
# two real producers, run over text authored in `source/`.
#
#   pandoc         jgm/pandoc, GPL-2.0-or-later, the portable Windows zip.
#                  Writes EPUB 3 by default and EPUB 2 under `-t epub2`.
#   ebook-convert  calibre's converter, kovidgoyal/calibre, GPL-3.0-only.
#                  Writes EPUB 2 or 3 under `--epub-version`.
#
# The text, the tables, the code block, the Japanese line and the four PNGs are
# all authored here, so each book is *our input through their tool* -- the
# reading `fuzz/README.md` already applies to the JPX seed corpus ("codestreams
# `opj_compress` made from our own 32 x 32 images") and gap 30's milestone 1
# applied to Windows' XPS serialisers. See README.md beside this file for why
# the alternative sources -- Project Gutenberg and `epub3-samples` -- cannot be
# committed at all.
#
# Run it as:
#
#   pwsh -NoProfile -File crates\tinker-pdf\tests\epub\make-corpus.ps1 `
#       -Pandoc C:\tools\pandoc\pandoc.exe `
#       -EbookConvert 'C:\Program Files\Calibre2\ebook-convert.exe'
#
# It is NOT reproducible byte for byte. Both producers mint a fresh UUID for
# the package document's `dc:identifier` on every run, and calibre stamps a
# `dcterms:modified` timestamp. A second run is a different file. The committed
# books are the record; this script is how they were obtained, and README.md
# records which version of which producer wrote which file, with a hash.

param(
    [string]$OutDir = $PSScriptRoot,
    [string]$Pandoc = $(if ($env:TINKER_PANDOC) { $env:TINKER_PANDOC } else { 'pandoc' }),
    [string]$EbookConvert = $(if ($env:TINKER_EBOOK_CONVERT) { $env:TINKER_EBOOK_CONVERT } else { 'ebook-convert' })
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

'books:'
Get-ChildItem $OutDir -Filter '*.epub' | Sort-Object Name | ForEach-Object {
    "  {0}  {1} bytes  sha256 {2}" -f $_.Name, $_.Length, (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
}
