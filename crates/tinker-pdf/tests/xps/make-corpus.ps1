# Produces the XPS corpus in this directory. Nothing in this repository writes
# a byte of it: every package comes out of one of two Microsoft serialisers.
#
#   .xps   System.Windows.Xps.Packaging.XpsDocument -- WPF's ReachFramework,
#          which writes the XPS 1.0 dialect. No printer and no elevation.
#   .oxps  the "XPS Object Factory" coclass in %WINDIR%\System32\XpsServices.dll
#          -- the XPS Document API -- reading one of the .xps files above and
#          writing it back as OpenXPS. Also no printer and no elevation.
#
# The content -- four coloured quadrants under a white diagonal, three
# rectangles, two gradients, eight characters of text -- is authored here, so
# each package is our input through their tool, which is the reading
# `fuzz/README.md` already applies to the JPX seeds.
#
# Run it as:
#
#   powershell.exe -NoProfile -STA -ExecutionPolicy Bypass `
#       -File crates\tinker-pdf\tests\xps\make-corpus.ps1
#
# Windows PowerShell 5.1 and -STA, because WPF will not build a visual on an
# MTA thread. It is NOT reproducible byte for byte: both serialisers mint a
# fresh GUID for every resource part and a fresh Id for every relationship, so
# a second run produces different part names and different bytes. The committed
# files are the record; this script is how they were obtained, and README.md
# beside it says which run produced which file.

param([string]$OutDir = $PSScriptRoot)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName PresentationCore
Add-Type -AssemblyName PresentationFramework
Add-Type -AssemblyName WindowsBase
Add-Type -AssemblyName ReachFramework
Add-Type -AssemblyName System.Printing

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$W = 816.0   # US Letter, in XPS units of 1/96 inch
$H = 1056.0

# Cascadia Mono ships with Windows and is the only font on a stock Windows 11
# install whose own `name` table carries the SIL Open Font License grant --
# "use, study, copy, merge, embed, modify, redistribute", with "The requirement
# for fonts to remain under this license does not apply to any document created
# using the Font Software". Its fsType is Installable. That is why the text in
# this corpus is set in it and not in one of the Monotype faces beside it.
$FontPath = Join-Path $env:WINDIR 'Fonts\CascadiaMono.ttf'
$FontUri = New-Object System.Uri($FontPath)

function New-OurBitmap {
    param([int]$Size, [string]$Kind)
    $dv = New-Object System.Windows.Media.DrawingVisual
    $dc = $dv.RenderOpen()
    $s = [double]$Size
    $q = $s / 2.0
    $cols = @(
        [System.Windows.Media.Brushes]::Crimson,
        [System.Windows.Media.Brushes]::SteelBlue,
        [System.Windows.Media.Brushes]::Goldenrod,
        [System.Windows.Media.Brushes]::SeaGreen
    )
    $i = 0
    foreach ($y in 0, 1) {
        foreach ($x in 0, 1) {
            $dc.DrawRectangle($cols[$i], $null, (New-Object System.Windows.Rect(($x * $q), ($y * $q), $q, $q)))
            $i++
        }
    }
    $pen = New-Object System.Windows.Media.Pen([System.Windows.Media.Brushes]::White, ($s / 8.0))
    $dc.DrawLine($pen, (New-Object System.Windows.Point(0, 0)), (New-Object System.Windows.Point($s, $s)))
    $dc.Close()
    $rtb = New-Object System.Windows.Media.Imaging.RenderTargetBitmap(
        $Size, $Size, 96, 96, [System.Windows.Media.PixelFormats]::Pbgra32)
    $rtb.Render($dv)
    $enc = if ($Kind -eq 'jpeg') {
        $j = New-Object System.Windows.Media.Imaging.JpegBitmapEncoder
        $j.QualityLevel = 70
        $j
    } else {
        New-Object System.Windows.Media.Imaging.PngBitmapEncoder
    }
    $enc.Frames.Add([System.Windows.Media.Imaging.BitmapFrame]::Create($rtb))
    $ms = New-Object System.IO.MemoryStream
    $enc.Save($ms)
    $bytes = $ms.ToArray()
    $ms.Dispose()
    # Round-trip through a decoder so the page carries an encoded source of
    # exactly these bytes, which is what the serialiser copies into the part.
    $src = New-Object System.IO.MemoryStream(, $bytes)
    return [System.Windows.Media.Imaging.BitmapFrame]::Create($src,
        [System.Windows.Media.Imaging.BitmapCreateOptions]::PreservePixelFormat,
        [System.Windows.Media.Imaging.BitmapCacheOption]::OnLoad)
}

function New-Page {
    $p = New-Object System.Windows.Documents.FixedPage
    $p.Width = $W
    $p.Height = $H
    $p.Background = [System.Windows.Media.Brushes]::White
    return $p
}

function Add-Rect {
    param($Page, $Brush, [double]$X, [double]$Y, [double]$Wd, [double]$Ht, [double]$Alpha = 1.0)
    $r = New-Object System.Windows.Shapes.Rectangle
    $r.Width = $Wd
    $r.Height = $Ht
    $r.Fill = $Brush
    $r.Opacity = $Alpha
    [System.Windows.Documents.FixedPage]::SetLeft($r, $X)
    [System.Windows.Documents.FixedPage]::SetTop($r, $Y)
    $Page.Children.Add($r) | Out-Null
}

function Add-Glyphs {
    # A Glyphs element rather than a TextBlock, because Indices is set here
    # rather than computed: ",53" is 12.1.3's least obvious form, an *empty*
    # GlyphIndex followed by an advance width, and a monospaced font gives the
    # serialiser no reason to emit one on its own.
    param($Page, [string]$Text, [string]$Indices, [double]$X, [double]$Y, [double]$Size)
    $g = New-Object System.Windows.Documents.Glyphs
    $g.FontUri = $FontUri
    $g.FontRenderingEmSize = $Size
    $g.OriginX = $X
    $g.OriginY = $Y
    $g.UnicodeString = $Text
    if ($Indices) { $g.Indices = $Indices }
    $g.Fill = [System.Windows.Media.Brushes]::Black
    $Page.Children.Add($g) | Out-Null
}

function Write-Xps {
    param([string]$Name, [System.Collections.IEnumerable]$Pages)
    $path = Join-Path $OutDir $Name
    if (Test-Path $path) { Remove-Item $path -Force }
    $fd = New-Object System.Windows.Documents.FixedDocument
    foreach ($pg in $Pages) {
        $pg.Measure((New-Object System.Windows.Size($W, $H)))
        $pg.Arrange((New-Object System.Windows.Rect(0, 0, $W, $H)))
        $pg.UpdateLayout()
        $pc = New-Object System.Windows.Documents.PageContent
        ([System.Windows.Markup.IAddChild]$pc).AddChild($pg)
        $fd.Pages.Add($pc) | Out-Null
    }
    $doc = New-Object System.Windows.Xps.Packaging.XpsDocument($path, [System.IO.FileAccess]::ReadWrite)
    [System.Windows.Xps.Packaging.XpsDocument]::CreateXpsDocumentWriter($doc).Write($fd)
    $doc.Close()
    "wrote {0,-30} {1,8} bytes" -f $Name, (Get-Item $path).Length
}

# --- 1. a raster resource and a line of text in an obfuscated font ----------
# The package the two pinned failures are measured on: one PNG resource, so
# the comic path finds exactly one image entry and pages it.
$p = New-Page
$img = New-Object System.Windows.Controls.Image
$img.Source = (New-OurBitmap -Size 32 -Kind 'png')
$img.Width = 200
$img.Height = 200
[System.Windows.Documents.FixedPage]::SetLeft($img, 100)
[System.Windows.Documents.FixedPage]::SetTop($img, 100)
$p.Children.Add($img) | Out-Null
Add-Glyphs -Page $p -Text 'Page one' -Indices ',53' -X 100 -Y 400 -Size 24
Write-Xps -Name 'wpf-image-and-text.xps' -Pages @($p)

# --- 2. no raster part anywhere, so the comic path finds nothing to page ----
$p = New-Page
Add-Rect -Page $p -Brush ([System.Windows.Media.Brushes]::Black)    -X 100 -Y 120 -Wd 300 -Ht 4
Add-Rect -Page $p -Brush ([System.Windows.Media.Brushes]::Crimson)  -X 100 -Y 200 -Wd 200 -Ht 160
Add-Rect -Page $p -Brush ([System.Windows.Media.Brushes]::SeaGreen) -X 340 -Y 200 -Wd 160 -Ht 160
Write-Xps -Name 'wpf-shapes-only.xps' -Pages @($p)

# --- 3. three pages, so markup order can be told from filename order --------
$pages = @()
foreach ($n in 1, 2, 3) {
    $pg = New-Page
    Add-Rect -Page $pg -Brush ([System.Windows.Media.Brushes]::SteelBlue) -X 100 -Y (100 + 60 * $n) -Wd (120 * $n) -Ht 60
    $pages += $pg
}
Write-Xps -Name 'wpf-three-pages.xps' -Pages $pages

# --- 4. a linear and a radial gradient brush --------------------------------
$p = New-Page
$lg = New-Object System.Windows.Media.LinearGradientBrush
$lg.StartPoint = New-Object System.Windows.Point(0, 0)
$lg.EndPoint = New-Object System.Windows.Point(1, 1)
$lg.GradientStops.Add((New-Object System.Windows.Media.GradientStop([System.Windows.Media.Colors]::Crimson, 0.0)))
$lg.GradientStops.Add((New-Object System.Windows.Media.GradientStop([System.Windows.Media.Colors]::Gold, 0.5)))
$lg.GradientStops.Add((New-Object System.Windows.Media.GradientStop([System.Windows.Media.Colors]::SeaGreen, 1.0)))
Add-Rect -Page $p -Brush $lg -X 100 -Y 100 -Wd 400 -Ht 200
$rg = New-Object System.Windows.Media.RadialGradientBrush
$rg.GradientOrigin = New-Object System.Windows.Point(0.4, 0.4)
$rg.GradientStops.Add((New-Object System.Windows.Media.GradientStop([System.Windows.Media.Colors]::White, 0.0)))
$rg.GradientStops.Add((New-Object System.Windows.Media.GradientStop([System.Windows.Media.Colors]::MidnightBlue, 1.0)))
Add-Rect -Page $p -Brush $rg -X 100 -Y 360 -Wd 300 -Ht 300
Write-Xps -Name 'wpf-gradients.xps' -Pages @($p)

# --- 5. tiling image brushes and an opacity ---------------------------------
$p = New-Page
$ib = New-Object System.Windows.Media.ImageBrush((New-OurBitmap -Size 32 -Kind 'png'))
$ib.TileMode = [System.Windows.Media.TileMode]::Tile
$ib.Viewport = New-Object System.Windows.Rect(0, 0, 0.25, 0.25)
$ib.ViewportUnits = [System.Windows.Media.BrushMappingMode]::RelativeToBoundingBox
Add-Rect -Page $p -Brush $ib -X 100 -Y 100 -Wd 400 -Ht 400
$ib2 = New-Object System.Windows.Media.ImageBrush((New-OurBitmap -Size 32 -Kind 'png'))
$ib2.TileMode = [System.Windows.Media.TileMode]::FlipXY
$ib2.Viewport = New-Object System.Windows.Rect(0, 0, 0.5, 0.5)
$ib2.ViewportUnits = [System.Windows.Media.BrushMappingMode]::RelativeToBoundingBox
Add-Rect -Page $p -Brush $ib2 -X 100 -Y 560 -Wd 400 -Ht 300 -Alpha 0.5
Write-Xps -Name 'wpf-tiled-brush.xps' -Pages @($p)

# --- 6. a JPEG image part ---------------------------------------------------
$p = New-Page
$img = New-Object System.Windows.Controls.Image
$img.Source = (New-OurBitmap -Size 48 -Kind 'jpeg')
$img.Width = 240
$img.Height = 240
[System.Windows.Documents.FixedPage]::SetLeft($img, 100)
[System.Windows.Documents.FixedPage]::SetTop($img, 100)
$p.Children.Add($img) | Out-Null
Write-Xps -Name 'wpf-jpeg-image.xps' -Pages @($p)

# --- 7 and 8. the same material as OpenXPS ----------------------------------
& (Join-Path $PSScriptRoot 'to-openxps.ps1') `
    -In (Join-Path $OutDir 'wpf-image-and-text.xps') `
    -Out (Join-Path $OutDir 'xpsom-image-and-text.oxps')
& (Join-Path $PSScriptRoot 'to-openxps.ps1') `
    -In (Join-Path $OutDir 'wpf-gradients.xps') `
    -Out (Join-Path $OutDir 'xpsom-gradients.oxps')
