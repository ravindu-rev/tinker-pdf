# Encodes this repository's own rasters as JPEG XR, using the encoder that
# ships with Windows.
#
# Read `crates/tinker-pdf-filters/tests/jxr_fixtures.rs` first: it explains
# why the fixtures are shaped this way and what they are evidence of. In one
# sentence — the rasters are authored in this repository, WIC only encodes
# them, and the decoder must give them back bit for bit, so nothing outside
# this tree ever says whether a decode is right (ruling 13).
#
# Run order, from the repository root:
#
#   cargo test -p tinker-pdf-filters --test jxr_fixtures -- --ignored write_source_rasters
#   pwsh -File crates/tinker-pdf-filters/tests/jxr/make-fixtures.ps1
#
# The first writes the `.raw` sources and `manifest.txt` from the RASTERS
# table; this script reads both and writes the `.jxr` files beside them. It
# prints RAN or exits non-zero: a script that quietly produced nothing reads
# exactly like one that produced the right thing, which is the failure mode
# `docs/verification.md` keeps the RAN / SKIPPED discipline for.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

Add-Type -AssemblyName PresentationCore, WindowsBase

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$manifestPath = Join-Path $here 'manifest.txt'

if (-not (Test-Path $manifestPath)) {
    Write-Error "no manifest.txt in $here - run the write_source_rasters test first"
}

# System.Windows.Media.PixelFormats member -> (bytes per pixel, member).
# The member is looked up by name so that a typo in the manifest fails here
# rather than silently encoding the wrong layout.
function Get-WpfFormat([string]$name) {
    $prop = [System.Windows.Media.PixelFormats].GetProperty($name)
    if ($null -eq $prop) { Write-Error "no PixelFormats member named '$name'" }
    return $prop.GetValue($null)
}

$rows = @()
foreach ($line in Get-Content $manifestPath) {
    $t = $line.Trim()
    if ($t -eq '' -or $t.StartsWith('#')) { continue }
    $p = $t -split '\s+'
    if ($p.Count -ne 10) { Write-Error "malformed manifest row: $t" }
    $rows += [pscustomobject]@{
        Name      = $p[0]
        Width     = [int]$p[1]
        Height    = [int]$p[2]
        WpfFormat = $p[3]
        Lossless  = [int]$p[4] -ne 0
        Overlap   = [byte]$p[5]
        HTiles    = [int16]$p[6]
        VTiles    = [int16]$p[7]
        Frequency = [int]$p[8] -ne 0
        Quant     = [byte]$p[9]
    }
}

$written = 0
foreach ($row in $rows) {
    $rawPath = Join-Path $here "$($row.Name).raw"
    if (-not (Test-Path $rawPath)) {
        Write-Error "no $($row.Name).raw - run the write_source_rasters test first"
    }
    $bytes = [System.IO.File]::ReadAllBytes($rawPath)

    $fmt = Get-WpfFormat $row.WpfFormat
    # Integer division: PowerShell's `/` is rational and `[int]` rounds it,
    # which turns a 48-byte stride into 49 and every raster into the wrong
    # length.
    $stride = [int][math]::Floor(($row.Width * $fmt.BitsPerPixel + 7) / 8)
    $expected = $stride * $row.Height
    if ($bytes.Length -ne $expected) {
        Write-Error "$($row.Name).raw is $($bytes.Length) bytes, expected $expected for $($row.WpfFormat) at $($row.Width)x$($row.Height)"
    }

    $source = [System.Windows.Media.Imaging.BitmapSource]::Create(
        $row.Width, $row.Height, 96, 96, $fmt, $null, $bytes, $stride)

    $enc = New-Object System.Windows.Media.Imaging.WmpBitmapEncoder
    # UseCodecOptions is what makes the codec honour the knobs below rather
    # than the single ImageQualityLevel dial.
    $enc.UseCodecOptions = $true
    # QualityLevel is the codec's own quantization parameter, and 1 is what
    # actually produces a lossless file. It is set for EVERY row, including
    # the lossless ones, because `Lossless` on its own does nothing.
    #
    # That is the third silently-ignored knob this fixture set has found, and
    # it cost the most: with `Lossless = $true` and no QualityLevel the codec
    # encodes at its default QP of 10, so the files this script used to write
    # were quantized while claiming to be lossless — and the lossless
    # identity, which is the primary evidence leg of the whole decoder, was
    # comparing against an encode that could never match. Measured, not
    # assumed: the same raster at `Lossless` alone is 1016 bytes and decodes
    # to within +/-3, and at `QualityLevel = 1` is 1378 bytes and decodes
    # bit-exact.
    #
    # `Lossless` is still set, because the codec may use it to choose among
    # otherwise equivalent encodings, and because a reader comparing this
    # script with the manifest should see the column honoured.
    $enc.Lossless = $row.Lossless
    $enc.QualityLevel = if ($row.Lossless) { 1 } else { $row.Quant }
    $enc.OverlapLevel = $row.Overlap
    $enc.HorizontalTileSlices = $row.HTiles
    $enc.VerticalTileSlices = $row.VTiles
    $enc.FrequencyOrder = $row.Frequency
    # No chroma subsampling: 4:4:4 keeps INTERNAL_CLR_FMT at YUV444, which is
    # the internal layout this decoder implements. A subsampled fixture would
    # be refused by name rather than decoded, and would prove nothing.
    $enc.SubsamplingLevel = 3

    $enc.Frames.Add([System.Windows.Media.Imaging.BitmapFrame]::Create($source))

    $outPath = Join-Path $here "$($row.Name).jxr"
    $fs = [System.IO.File]::Create($outPath)
    try { $enc.Save($fs) } finally { $fs.Close() }

    $out = [System.IO.File]::ReadAllBytes($outPath)
    if ($out.Length -lt 4 -or $out[0] -ne 0x49 -or $out[1] -ne 0x49 -or $out[2] -ne 0xBC) {
        Write-Error "$($row.Name).jxr does not start with Annex A's file header"
    }
    $written++
    "  {0,-18} {1,5} bytes  {2}x{3} {4} overlap={5} tiles={6}x{7} freq={8} lossless={9} quant={10}" -f `
        $row.Name, $out.Length, $row.Width, $row.Height, $row.WpfFormat, `
        $row.Overlap, $row.HTiles, $row.VTiles, $row.Frequency, $row.Lossless, $row.Quant
}

$os = [System.Environment]::OSVersion.Version
"RAN make-fixtures.ps1: $written fixtures written on Windows $os"
if ($written -eq 0) { exit 1 }
