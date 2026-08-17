# Regenerates INVENTORY.tsv from the packages in this directory.
#
# The inventory is what milestone 1 owes: every item of every package, its
# media type as `[Content_Types].xml` resolves it, the compression method the
# ZIP central directory records, and both sizes. It is produced by reading the
# files rather than by remembering what was written, so it cannot describe a
# package that is no longer there -- and `tests/xps.rs`'s
# `inventory_matches_the_packages` recomputes the same table through
# `tinker-pdf-zip` on every `cargo test`, so it cannot drift either. Two
# independent readers, .NET's and ours, have to agree.
#
#   powershell -NoProfile -File crates\tinker-pdf\tests\xps\inventory.ps1

param([string]$Dir = $PSScriptRoot)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Resolve-MediaType {
    # OPC 7.2.3.5, in order: an Override whose PartName matches (ASCII
    # case-insensitive), else a Default whose Extension matches the substring
    # right of the rightmost dot of the rightmost segment (also ASCII
    # case-insensitive), else nothing.
    param([string]$Item, $Overrides, $Defaults)
    $part = '/' + $Item
    foreach ($k in $Overrides.Keys) {
        if ([string]::Equals($k, $part, 'OrdinalIgnoreCase')) { return $Overrides[$k] }
    }
    $seg = $Item.Split('/')[-1]
    $dot = $seg.LastIndexOf('.')
    if ($dot -lt 0) { return '(none)' }
    $ext = $seg.Substring($dot + 1)
    foreach ($k in $Defaults.Keys) {
        if ([string]::Equals($k, $ext, 'OrdinalIgnoreCase')) { return $Defaults[$k] }
    }
    return '(none)'
}

$rows = @("package`titem`tmedia_type`tmethod`tcompressed`tuncompressed")

foreach ($f in Get-ChildItem $Dir -Include *.xps, *.oxps -Recurse | Sort-Object Name) {
    $z = [System.IO.Compression.ZipFile]::OpenRead($f.FullName)
    try {
        $ct = $z.Entries | Where-Object { $_.FullName -eq '[Content_Types].xml' }
        $defaults = @{}
        $overrides = @{}
        if ($ct) {
            $r = New-Object System.IO.StreamReader($ct.Open())
            $xml = [xml]$r.ReadToEnd()
            $r.Dispose()
            foreach ($n in $xml.Types.ChildNodes) {
                if ($n.LocalName -eq 'Default') { $defaults[$n.Extension] = $n.ContentType }
                elseif ($n.LocalName -eq 'Override') { $overrides[$n.PartName] = $n.ContentType }
            }
        }
        # The ZIP general-purpose method, read from the archive rather than
        # inferred: a stored item and a deflated one are different inputs to
        # every reader downstream of this.
        $methods = @{}
        $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
        for ($i = 0; $i -lt $bytes.Length - 3; $i++) {
            if ($bytes[$i] -eq 0x50 -and $bytes[$i+1] -eq 0x4B -and $bytes[$i+2] -eq 0x01 -and $bytes[$i+3] -eq 0x02) {
                $m = [int]$bytes[$i+10] + ([int]$bytes[$i+11] -shl 8)
                $nlen = [int]$bytes[$i+28] + ([int]$bytes[$i+29] -shl 8)
                $elen = [int]$bytes[$i+30] + ([int]$bytes[$i+31] -shl 8)
                $clen = [int]$bytes[$i+32] + ([int]$bytes[$i+33] -shl 8)
                $name = [System.Text.Encoding]::UTF8.GetString($bytes, $i + 46, $nlen)
                $methods[$name] = switch ($m) { 0 { 'stored' } 8 { 'deflate' } default { "method-$m" } }
                $i += 45 + $nlen + $elen + $clen
            }
        }
        foreach ($e in $z.Entries) {
            $rows += "{0}`t{1}`t{2}`t{3}`t{4}`t{5}" -f $f.Name, $e.FullName,
                (Resolve-MediaType $e.FullName $overrides $defaults),
                $methods[$e.FullName], $e.CompressedLength, $e.Length
        }
    } finally { $z.Dispose() }
}

$out = Join-Path $Dir 'INVENTORY.tsv'
# LF, no BOM: .gitattributes normalises this file like any other text.
[System.IO.File]::WriteAllText($out, ($rows -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
"wrote {0} ({1} rows)" -f $out, ($rows.Count - 1)
