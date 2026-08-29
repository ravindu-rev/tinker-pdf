# Regenerates INVENTORY.tsv from the ZIP archives in this directory.
#
# Every entry of every archive: the stored path, the compression method the
# central directory records, both sizes, and the CRC-32. It is produced by
# reading the files rather than by remembering what was written, so it cannot
# describe an archive that is no longer there -- and `cbz_real.rs`'s
# `the_inventory_matches_the_archives` recomputes the same table through
# `tinker-pdf-zip` on every `cargo test`, so it cannot drift either. Two
# independent readers, .NET's and ours, have to agree about every row.
#
# The three non-ZIP containers -- `.cb7`, `.cbt`, `.cbr` -- are not here.
# .NET reads none of them, and this build reads none of them either; README.md
# carries their sizes and hashes instead.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File crates\tinker-pdf\tests\cbz\inventory.ps1

param([string]$Dir = $PSScriptRoot)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

$rows = New-Object System.Collections.Generic.List[string]
$rows.Add("archive`tentry`tmethod`tcompressed`tuncompressed`tcrc32")

foreach ($file in Get-ChildItem -LiteralPath $Dir -Filter *.cbz | Sort-Object Name) {
    $zip = [System.IO.Compression.ZipFile]::OpenRead($file.FullName)
    try {
        foreach ($entry in $zip.Entries) {
            # .NET exposes no method code, so it is inferred the only way the
            # class allows: an entry whose compressed and uncompressed lengths
            # are equal was stored. That is a heuristic on .NET's side and an
            # exact reading on ours -- `tinker-pdf-zip` has the field -- which
            # is the asymmetry that makes the comparison worth running.
            $method = if ($entry.CompressedLength -eq $entry.Length) { 'stored' } else { 'deflate' }
            $rows.Add(("{0}`t{1}`t{2}`t{3}`t{4}`t{5:x8}" -f `
                $file.Name, $entry.FullName, $method, `
                $entry.CompressedLength, $entry.Length, $entry.Crc32))
        }
    }
    finally { $zip.Dispose() }
}

$out = Join-Path $Dir 'INVENTORY.tsv'
Set-Content -LiteralPath $out -Value $rows -Encoding UTF8
"wrote {0}  ({1} rows)" -f $out, ($rows.Count - 1)
