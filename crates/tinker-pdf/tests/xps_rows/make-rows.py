# Writes the packages in this directory: one per XpsElementDefect row closed
# since tier 4, each a conservation-census fixture for that row.
#
# Every package is ../xps/wpf-image-and-text.xps with its fixed page replaced:
# the font, the picture, the relationships and the containers are WPF's bytes,
# and only the page markup -- and, where a row needs one, a profile part -- is
# this repository's. No producer on hand writes any of these features, which
# is why they are derived rather than produced (see README.md).
#
#   cd crates/tinker-pdf/tests/xps_rows && python3 make-rows.py
#
# One fixed timestamp, so a rerun under the same CPython writes the same bytes.
# Not run by any test: the committed packages are the record (ruling 13).

import zipfile

SOURCE = "../xps/wpf-image-and-text.xps"
STAMP = (2026, 10, 3, 0, 0, 0)
PAGE = "Documents/1/Pages/1.fpage"
FONT = "/Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF"
HEAD = (
    '<FixedPage xmlns="http://schemas.microsoft.com/xps/2005/06" '
    'xmlns:x="http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key" '
    'xml:lang="en-us" Width="816" Height="1056">'
)


def glyphs(y, extra):
    return (
        '<Glyphs OriginX="100" OriginY="%d" FontRenderingEmSize="48" FontUri="%s" '
        'UnicodeString="Page one" Indices=",53" Fill="#FF000000" %s/>' % (y, FONT, extra)
    )


def write(out, body, extra_parts=(), extra_types=""):
    source = zipfile.ZipFile(SOURCE)
    with zipfile.ZipFile(out, "w") as package:
        for info in source.infolist():
            data = source.read(info.filename)
            if info.filename == PAGE:
                data = (HEAD + body + "</FixedPage>").encode("utf-8")
            if info.filename == "[Content_Types].xml" and extra_types:
                text = data.decode("utf-8")
                data = text.replace("</Types>", extra_types + "</Types>").encode("utf-8")
            entry = zipfile.ZipInfo(info.filename, date_time=STAMP)
            entry.compress_type = info.compress_type
            package.writestr(entry, data)
        for name, data in extra_parts:
            entry = zipfile.ZipInfo(name, date_time=STAMP)
            entry.compress_type = zipfile.ZIP_DEFLATED
            package.writestr(entry, data)


def linear(key, stops, extra=""):
    return (
        '<LinearGradientBrush x:Key="%s" StartPoint="0,0" EndPoint="400,0" '
        'MappingMode="Absolute" SpreadMethod="Pad" %s>'
        "<LinearGradientBrush.GradientStops>%s</LinearGradientBrush.GradientStops>"
        "</LinearGradientBrush>" % (key, extra, stops)
    )


def stop(colour, offset):
    return '<GradientStop Color="%s" Offset="%s" />' % (colour, offset)


def path(key, y):
    return (
        '<Path Fill="{StaticResource %s}" RenderTransform="1,0,0,1,100,%d" '
        'Data="M0,0L400,0 400,100 0,100Z" />' % (key, y)
    )


# 12.1.5's four values, one run each, a hundred units apart.
write(
    "wpf-style-simulations.xps",
    glyphs(200, "")
    + glyphs(300, 'StyleSimulations="BoldSimulation" ')
    + glyphs(400, 'StyleSimulations="ItalicSimulation" ')
    + glyphs(500, 'StyleSimulations="BoldItalicSimulation" '),
)

# 18.3.2's alpha, interpolated between stops: three stops fading from opaque
# through half to clear; a radial one fading outward; and one alpha shared by
# every stop, which is a constant alpha and no ramp at all.
RADIAL = (
    '<RadialGradientBrush x:Key="r" MappingMode="Absolute" SpreadMethod="Pad" '
    'Center="150,150" RadiusX="150" RadiusY="150" GradientOrigin="150,150">'
    "<RadialGradientBrush.GradientStops>%s%s</RadialGradientBrush.GradientStops>"
    "</RadialGradientBrush>" % (stop("#FF191970", "0"), stop("#00191970", "1"))
)
write(
    "wpf-stop-alphas.xps",
    "<FixedPage.Resources><ResourceDictionary>"
    + linear("a", stop("#FFDC143C", "0") + stop("#80FFD700", "0.5") + stop("#002E8B57", "1"))
    + RADIAL
    + linear("u", stop("#80DC143C", "0") + stop("#802E8B57", "1"))
    + "</ResourceDictionary></FixedPage.Resources>"
    + path("a", 100)
    + '<Path Fill="{StaticResource r}" RenderTransform="1,0,0,1,100,260" '
    'Data="M0,0L300,0 300,300 0,300Z" />'
    + path("u", 600),
)
