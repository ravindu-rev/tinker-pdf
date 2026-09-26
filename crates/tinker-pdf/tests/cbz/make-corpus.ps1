# Produces the committed comic-archive corpus in this directory.
#
# Nothing in this repository writes a byte of any archive here. The *pictures*
# are ours -- `cbz_real.rs`'s `write_the_source_pages` emits them into
# `source/` through `cbz_support`'s own PNG and JPEG writers, which are those
# two specifications transcribed -- and every archive around them was packed by
# somebody else's implementation. That is the shape `tests/epub/README.md` and
# `tests/xps/README.md` already argue for: our input through their tool.
#
#   7-Zip 26.02      ip7z/7zip, LGPL-2.1-or-later with an unRAR restriction.
#                    Writes ZIP (deflated and stored), 7z and tar.
#   WinRAR 7.20      RARLAB, proprietary, trial. Writes ZIP and RAR5.
#   Compress-Archive PowerShell over .NET's System.IO.Compression.ZipFile.
#   CPython 3.12     the `zipfile` module.
#
# Five independent ZIP writers, because one proves the reader can open one real
# archive and five prove the stronger relation `cbz_real.rs` asserts. See
# README.md beside this file for what each demonstrates and what each producer
# did that the hand-built fixtures never did.
#
# Run it as:
#
#   pwsh -NoProfile -File crates\tinker-pdf\tests\cbz\make-corpus.ps1
#
# `-Only` writes a subset by file name, and it exists because a whole run
# rewrites every archive here: each producer stamps its entries with the source
# files' modification times, so re-running the script for one new fixture would
# change the bytes and the hash of every older one for no reason. The two
# `.cb7`s added after the first run were obtained with
#
#   pwsh -NoProfile -File ...\make-corpus.ps1 -Only 7z-nonsolid.cb7,7z-dictreset.cb7
#
# It is NOT reproducible byte for byte: every ZIP writer here stamps each entry
# with the source file's modification time, and 7-Zip and WinRAR record it to
# different precisions. The committed archives are the record; this script is
# how they were obtained, and README.md carries a hash for each.

param(
    [string]$OutDir = $PSScriptRoot,
    [string[]]$Only = @(),
    [string]$SevenZip = $(if ($env:TINKER_7ZIP) { $env:TINKER_7ZIP } else { 'C:\Program Files\7-Zip\7z.exe' }),
    [string]$WinRar = $(if ($env:TINKER_WINRAR) { $env:TINKER_WINRAR } else { 'C:\Program Files\WinRAR\WinRAR.exe' }),
    [string]$Rar = $(if ($env:TINKER_RAR) { $env:TINKER_RAR } else { 'C:\Program Files\WinRAR\Rar.exe' }),
    [string]$Python = $(if ($env:TINKER_PYTHON) { $env:TINKER_PYTHON } else { 'python' })
)

$ErrorActionPreference = 'Stop'
$src = Join-Path $PSScriptRoot 'source'
if (-not (Test-Path $src)) {
    throw "$src does not exist. Run the generator first:`n" +
          "  cargo test -p tinker-pdf --test cbz_real -- --ignored write_the_source_pages"
}

# The order here is the order the archives store the entries in, and it is
# deliberately NOT the reading order: `page10` and `page11` are packed between
# `page1` and `page2` so that an archive whose stored order was trusted would
# page the comic wrongly. Natural order has to come from the names.
$pages = @('page1.png', 'page10.png', 'page11.png', 'page2.png', 'page3.jpg')
$paths = $pages | ForEach-Object { Join-Path $src $_ }
foreach ($p in $paths) { if (-not (Test-Path $p)) { throw "missing source page: $p" } }

function Remove-IfPresent { param([string]$Path) if (Test-Path $Path) { Remove-Item $Path -Force } }

# `pwsh -File` hands every argument over as one literal string, so `-Only a,b`
# arrives as the single element "a,b" rather than as two. Split it here rather
# than telling the reader to invoke the script a different way.
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })

# Whether this run is meant to write a given archive. An empty `-Only` means
# all of them, which is what a first run does.
function Test-Wanted { param([string]$Name) return ($Only.Count -eq 0) -or ($Only -contains $Name) }

function Show-Result {
    param([string]$Path, [string]$Producer)
    $bytes = (Get-Item $Path).Length
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLower()
    "  {0,-22} {1,7} bytes  {2}  {3}" -f (Split-Path $Path -Leaf), $bytes, $hash.Substring(0, 16), $Producer
}

Push-Location $src
try {
    # ---- 7-Zip ---------------------------------------------------------------
    #
    # Two ZIPs from one writer, because the difference between them is the one
    # the reader's pass-through path turns on: `-mx0` stores every entry and
    # `-mx9` deflates it, and a stored entry is handed back borrowed where a
    # deflated one is inflated into an owned buffer.
    #
    # Three `.cb7`s, for the same reason and one level down. `-m0=LZMA2` alone
    # writes what a desktop archiver writes: ONE folder holding one LZMA2 chunk,
    # which leaves the folder walk and the chunk loop each running exactly once.
    # The other two ask 7-Zip for the two shapes that make those loops run more
    # than once, and **the flags were checked against what came out** rather
    # than believed -- `the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`
    # in `tinker-pdf-archive`'s `sevenz/tests.rs` asserts the folder count and
    # the chunk count, so a regeneration that quietly lost either fails a test:
    #
    #   -ms=off        turns solid mode off: one folder per file, five folders,
    #                  so the folder walk runs past folder 0 and the per-folder
    #                  coder setup happens five times instead of once.
    #   d8k:c8k        an 8 KiB dictionary and an 8 KiB LZMA2 block, over
    #                  18 483 bytes of pages: three LZMA2 chunks, each opening
    #                  with a dictionary reset, two of them mid-stream.
    #
    # `d64k` on its own does nothing here and that is worth recording, because
    # it is the obvious flag to reach for: these five pages are 18 483 bytes,
    # a solid LZMA2 block is not split by dictionary size, and a run with
    # `-m0=LZMA2:d64k` measured **one** chunk. It is `c`, the block size, that
    # makes the encoder start a new block -- and a new block is what carries the
    # dictionary reset. `d8k` is set alongside it only so the dictionary cannot
    # outlive the block it belongs to.
    #
    # There is deliberately **no BCJ case**. `-mf=BCJ` writes coder id `03030103`,
    # which is outside the allow-list in `sevenz.rs` and would be refused at
    # open; a fixture for it belongs with the work that implements it, not with
    # coverage of the decoder that exists. See `docs/design/comic-archives.md`.
    foreach ($case in @(@{ name = '7z-deflate.cbz';   args = @('-tzip', '-mx9') },
                        @{ name = '7z-store.cbz';     args = @('-tzip', '-mx0') },
                        @{ name = '7z-lzma2.cb7';     args = @('-t7z', '-m0=LZMA2') },
                        @{ name = '7z-nonsolid.cb7';  args = @('-t7z', '-m0=LZMA2', '-ms=off') },
                        @{ name = '7z-dictreset.cb7'; args = @('-t7z', '-m0=LZMA2:d8k:c8k') },
                        @{ name = '7z-tar.cbt';       args = @('-ttar') })) {
        if (-not (Test-Wanted $case.name)) { continue }
        $out = Join-Path $OutDir $case.name
        Remove-IfPresent $out
        & $SevenZip a @($case.args) $out @pages | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "7-Zip failed on $($case.name): $LASTEXITCODE" }
        Show-Result $out '7-Zip 26.02'
    }

    # ---- WinRAR --------------------------------------------------------------
    #
    # `Rar.exe` writes RAR only; the ZIP comes from `WinRAR.exe -afzip`.
    #
    # **RAR 4 cannot be produced on this machine and that is recorded rather
    # than worked around.** RAR 7.20's `rar.exe` has no `-ma` switch at all --
    # `-ma4` answers "Unknown option: ma4" and the help lists no format-version
    # switch -- so this release writes RAR 5 and nothing else. README.md carries
    # what that costs.
    if (Test-Wanted 'winrar.cbz') {
        $out = Join-Path $OutDir 'winrar.cbz'
        Remove-IfPresent $out
        & $WinRar a -afzip -ibck -y $out @pages | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "WinRAR failed on winrar.cbz: $LASTEXITCODE" }
        Show-Result $out 'WinRAR 7.20'
    }

    if (Test-Wanted 'winrar-rar5.cbr') {
        $out = Join-Path $OutDir 'winrar-rar5.cbr'
        Remove-IfPresent $out
        & $Rar a -y -idq $out @pages | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Rar failed on winrar-rar5.cbr: $LASTEXITCODE" }
        Show-Result $out 'WinRAR 7.20 (RAR 5)'
    }

    # ---- .NET, through PowerShell -------------------------------------------
    #
    # `Compress-Archive` refuses any destination whose extension is not `.zip`
    # -- `NotSupportedArchiveFileExtension` -- so it writes one and the file is
    # renamed afterwards. The bytes are untouched by the rename, which is the
    # point: a comic archive is a ZIP whatever it is called, and this reader
    # decides by the bytes at offset zero rather than by the name.
    if (Test-Wanted 'pwsh.cbz') {
        $tmp = Join-Path $OutDir 'pwsh.zip'
        $out = Join-Path $OutDir 'pwsh.cbz'
        Remove-IfPresent $tmp
        Remove-IfPresent $out
        Compress-Archive -LiteralPath $paths -DestinationPath $tmp -Force
        Move-Item $tmp $out
        Show-Result $out '.NET System.IO.Compression'
    }

    # ---- CPython -------------------------------------------------------------
    if (Test-Wanted 'python.cbz') {
        $out = Join-Path $OutDir 'python.cbz'
        Remove-IfPresent $out
        $script = @'
import sys, zipfile
out, names = sys.argv[1], sys.argv[2:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for n in names:
        z.write(n)
'@
        $scriptPath = Join-Path $env:TEMP 'tinker-cbz-corpus.py'
        Set-Content -LiteralPath $scriptPath -Value $script -Encoding ASCII
        & $Python $scriptPath $out @pages
        if ($LASTEXITCODE -ne 0) { throw "python failed on python.cbz: $LASTEXITCODE" }
        Remove-Item $scriptPath -Force
        Show-Result $out 'CPython 3.12 zipfile'
    }
}
finally {
    Pop-Location
}

""
"Regenerate INVENTORY.tsv with:  pwsh -NoProfile -File $(Join-Path $PSScriptRoot 'inventory.ps1')"
