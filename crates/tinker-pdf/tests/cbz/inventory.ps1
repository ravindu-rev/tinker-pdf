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
# The five non-ZIP containers -- three `.cb7`, one `.cbt`, one `.cbr` -- are not
# here. .NET reads none of them, so there is no second reader to disagree with;
# README.md carries their sizes and hashes instead, and what this build makes of
# them is asserted in `cbz_real.rs` and in `tinker-pdf-archive`'s own suite.
#
#   pwsh -NoProfile -ExecutionPolicy Bypass -File crates\tinker-pdf\tests\cbz\inventory.ps1
#
# `pwsh` 7 and not Windows PowerShell 5.1: `ZipArchiveEntry.Crc32` arrived in
# .NET 7, and 5.1 leaves that column empty rather than failing.

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
# LF and no byte-order mark, written the way `tests/xps/inventory.ps1` writes
# its own: `Set-Content` would give CRLF on Windows, `.gitattributes` would
# normalise it back to LF on commit, and every regeneration would then show a
# whole-file diff that changes nothing.
[System.IO.File]::WriteAllText($out, ($rows -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
"wrote {0}  ({1} rows)" -f $out, ($rows.Count - 1)
