# Produces the `gs-*.xps` half of the XPS corpus: a second producer, from a
# second vendor, on a machine that is still Windows but whose converter is not.
#
# Nothing in this repository writes a byte of the packages. What this repository
# writes is their **input**: `crates/tinker-pdf/tests/xps_corpus_source.rs`
# builds five one-page PDFs with `DocumentBuilder` and commits them under
# `source/`, and everything below hands one of them to Ghostscript's `xpswrite`
# device. Our content through their tool, which is the reading `fuzz/README.md`
# applies to the JPX seeds, `tests/epub/README.md` applies to pandoc and
# calibre, and `make-corpus.ps1` beside this file applies to WPF.
#
#   GPL Ghostscript 10.07.1, AGPL-3.0-or-later, from Artifex's own
#   `gs10071w64.exe` release, unpacked with 7-Zip rather than installed because
#   the NSIS installer wants elevation this session does not have. It is not
#   vendored, not linked, not redistributed and not a dependency of anything in
#   the workspace -- see README.md, "Whether they may be committed".
#
# Run it as:
#
#   powershell.exe -NoProfile -ExecutionPolicy Bypass `
#       -File crates\tinker-pdf\tests\xps\make-gs-corpus.ps1
#
# after regenerating the sources, which is a separate and deliberate step:
#
#   cargo test -p tinker-pdf --test xps_corpus_source -- --ignored --nocapture
#
# **This is how the files were obtained, not something CI runs.** No test spawns
# it, ruling 13 would not allow one to, and `cargo xtask oracles` holds that
# line with a build failure. `tests/epub/README.md` says the same of its own
# `make-corpus.ps1` in the same words, and for the same reason.
#
# Unlike every other corpus script in this tree, **re-running it does reproduce
# these bytes.** Ghostscript stamps 2012-02-16 09:15:00 on every ZIP entry,
# stores rather than deflates, and derives its one relationship `Id` from the
# content rather than from a GUID, so a second run over the same source PDF is
# the same file to the byte. That is why the SHA-256 of each output is printed:
# here it is a *check*, where in `make-corpus.ps1` and in `tests/epub`'s it
# could only ever have been a record.

param(
    [string]$OutDir = $PSScriptRoot,
    [string]$Ghostscript = (Join-Path $env:LOCALAPPDATA 'gs10071\bin\gswin64c.exe')
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Ghostscript)) {
    throw "no Ghostscript at $Ghostscript -- pass -Ghostscript with the path to gswin64c.exe"
}
$version = & $Ghostscript --version
"Ghostscript {0} at {1}" -f $version, $Ghostscript
if ($version -ne '10.07.1') {
    Write-Warning "README.md records 10.07.1; this is $version, so the hashes below will not match"
}

$SourceDir = Join-Path $OutDir 'source'

# (source document, package). One package per source, one thing demonstrated
# per package, which is the shape the six `wpf-` packages already have.
$Corpus = @(
    @{ Source = 'paths.pdf'; Package = 'gs-paths.xps' }
    @{ Source = 'gradients.pdf'; Package = 'gs-gradients.xps' }
    @{ Source = 'images.pdf'; Package = 'gs-images.xps' }
    @{ Source = 'embedded-font.pdf'; Package = 'gs-embedded-font.xps' }
    @{ Source = 'rasterised-text.pdf'; Package = 'gs-rasterised-text.xps' }
)

foreach ($item in $Corpus) {
    $in = Join-Path $SourceDir $item.Source
    if (-not (Test-Path $in)) {
        throw "no source at $in -- run the writer in tests/xps_corpus_source.rs first"
    }
    $out = Join-Path $OutDir $item.Package
    if (Test-Path $out) { Remove-Item $out -Force }

    # -dSAFER is the default from 9.50 on and is passed anyway, because a
    # command that reads a document should say what it is allowed to reach even
    # when the default already says it. -dNOPAUSE -dBATCH make it exit; -q keeps
    # the banner out of the output this script's own lines are read from.
    & $Ghostscript -dNOPAUSE -dBATCH -dSAFER -q -sDEVICE=xpswrite -o $out $in
    if ($LASTEXITCODE -ne 0) { throw "$($item.Source): Ghostscript exited $LASTEXITCODE" }

    $hash = (Get-FileHash -Algorithm SHA256 -Path $out).Hash.ToLowerInvariant()
    "wrote {0,-26} {1,8} bytes  {2}" -f $item.Package, (Get-Item $out).Length, $hash
}

"`nNow re-run inventory.ps1, then the conservation sweep, and update README.md:"
"  powershell -NoProfile -File crates\tinker-pdf\tests\xps\inventory.ps1"
"  cargo test -p tinker-pdf --test xps_conservation -- --nocapture"
