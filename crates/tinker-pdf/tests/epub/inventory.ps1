# Regenerates INVENTORY.tsv from the books in this directory.
#
# What milestone 1 owes: every entry of every book, its media type as the
# package document declares it, the compression method the ZIP central
# directory records, where its local header sits, and both sizes. It is
# produced by reading the files rather than by remembering what was written,
# and `tests/epub.rs`'s `inventory_matches_the_books` recomputes name, method,
# header offset and both sizes through `tinker-pdf-zip` on every `cargo test`,
# so it cannot drift either. Two independent readers -- .NET's and ours -- have
# to agree about all of it.
#
# The central directory is walked by hand rather than through
# System.IO.Compression, because .NET's ZipArchiveEntry exposes neither the
# compression method nor the local header offset, and both are the point:
# OCF 3.3 section 4.3.2 is a statement about method and physical position.
#
#   pwsh -NoProfile -File crates\tinker-pdf\tests\epub\inventory.ps1

param([string]$Dir = $PSScriptRoot)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Get-OpfMediaTypes {
    # Follows META-INF/container.xml to the first rootfile, reads its manifest,
    # and resolves every item's href against the package document's own
    # directory -- which is section 4.2.5's rule and is why the two producers'
    # books, one with the OPF at the root and one with it under EPUB/, produce
    # the same kind of table.
    param($Zip)
    $types = @{}
    $container = $Zip.Entries | Where-Object { $_.FullName -eq 'META-INF/container.xml' }
    if (-not $container) { return $types }
    $reader = New-Object System.IO.StreamReader($container.Open())
    $xml = [xml]$reader.ReadToEnd()
    $reader.Dispose()
    $rootfile = $xml.GetElementsByTagName('rootfile') | Select-Object -First 1
    if (-not $rootfile) { return $types }
    $opfPath = $rootfile.GetAttribute('full-path')
    $types['#opf'] = $opfPath
    $opf = $Zip.Entries | Where-Object { $_.FullName -eq $opfPath }
    if (-not $opf) { return $types }
    $reader = New-Object System.IO.StreamReader($opf.Open())
    $package = [xml]$reader.ReadToEnd()
    $reader.Dispose()
    $base = Split-Path $opfPath -Parent
    $base = $base -replace '\\', '/'
    foreach ($item in $package.GetElementsByTagName('item')) {
        $href = $item.GetAttribute('href')
        if (-not $href) { continue }
        # No percent-decoding and no `..` resolution: neither appears in this
        # corpus, and inventing a canonical form is a claim milestone 3 makes
        # with the specification in front of it rather than one this makes here.
        $full = if ($base) { "$base/$href" } else { $href }
        $types[$full] = $item.GetAttribute('media-type')
    }
    return $types
}

$rows = @("book`tentry`tmedia_type`tmethod`theader_offset`tcompressed`tuncompressed")

foreach ($f in Get-ChildItem $Dir -Filter '*.epub' | Sort-Object Name) {
    $zip = [System.IO.Compression.ZipFile]::OpenRead($f.FullName)
    try { $types = Get-OpfMediaTypes $zip } finally { $zip.Dispose() }

    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    $i = 0
    while ($i -lt $bytes.Length - 3) {
        if ($bytes[$i] -eq 0x50 -and $bytes[$i + 1] -eq 0x4B -and $bytes[$i + 2] -eq 0x01 -and $bytes[$i + 3] -eq 0x02) {
            $m = [int]$bytes[$i + 10] + ([int]$bytes[$i + 11] -shl 8)
            $csize = [System.BitConverter]::ToUInt32($bytes, $i + 20)
            $usize = [System.BitConverter]::ToUInt32($bytes, $i + 24)
            $nlen = [int]$bytes[$i + 28] + ([int]$bytes[$i + 29] -shl 8)
            $elen = [int]$bytes[$i + 30] + ([int]$bytes[$i + 31] -shl 8)
            $clen = [int]$bytes[$i + 32] + ([int]$bytes[$i + 33] -shl 8)
            $offset = [System.BitConverter]::ToUInt32($bytes, $i + 42)
            $name = [System.Text.Encoding]::UTF8.GetString($bytes, $i + 46, $nlen)
            $method = switch ($m) { 0 { 'stored' } 8 { 'deflate' } default { "method-$m" } }
            $type = if ($name -eq $types['#opf']) { 'application/oebps-package+xml' }
                    elseif ($types.ContainsKey($name)) { $types[$name] }
                    else { '(none)' }
            $rows += "{0}`t{1}`t{2}`t{3}`t{4}`t{5}`t{6}" -f $f.Name, $name, $type, $method, $offset, $csize, $usize
            $i += 46 + $nlen + $elen + $clen
        }
        else { $i++ }
    }
    $zip = $null
}

$out = Join-Path $Dir 'INVENTORY.tsv'
# LF, no BOM: .gitattributes normalises this file like any other text.
[System.IO.File]::WriteAllText($out, ($rows -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
"wrote {0} ({1} rows)" -f $out, ($rows.Count - 1)
