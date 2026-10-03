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

import struct
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


def write(out, body, extra_parts=(), extra_types="", extra_rels=""):
    source = zipfile.ZipFile(SOURCE)
    with zipfile.ZipFile(out, "w") as package:
        for info in source.infolist():
            data = source.read(info.filename)
            if info.filename == PAGE:
                data = (HEAD + body + "</FixedPage>").encode("utf-8")
            if info.filename == "[Content_Types].xml" and extra_types:
                text = data.decode("utf-8")
                data = text.replace("</Types>", extra_types + "</Types>").encode("utf-8")
            if info.filename == "Documents/1/Pages/_rels/1.fpage.rels" and extra_rels:
                text = data.decode("utf-8")
                data = text.replace(
                    "</Relationships>", extra_rels + "</Relationships>"
                ).encode("utf-8")
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

def n_channel_lut(n):
    """An nCLR profile of n channels, one mft2 at A2B0, XYZ connection space.

    The same profile `xps_context_colour.rs` builds as `n_channel_lut`: a
    two-point grid, a fifth of the D50 white with no ink on the first channel
    and black with it full, whatever the other channels say.
    """
    table = b"mft2\0\0\0\0" + bytes([n, 3, 2, 0])
    for row in range(3):
        for column in range(3):
            table += struct.pack(">i", 0x10000 if row == column else 0)
    table += struct.pack(">HH", 2, 2)
    table += b"\x00\x00\xff\xff" * n
    fifth = (31595 // 5, 32768 // 5, 27030 // 5)
    for corner in range(1 << n):
        inked = (corner >> (n - 1)) & 1 == 1
        table += struct.pack(">HHH", *((0, 0, 0) if inked else fifth))
    table += b"\x00\x00\xff\xff" * 3
    header = bytearray(128)
    header[8:12] = bytes([2, 0x10, 0, 0])
    header[12:16] = b"prtr"
    header[16:20] = ("%XCLR" % n).encode("ascii")
    header[20:24] = b"XYZ "
    header[36:40] = b"acsp"
    profile = bytes(header) + struct.pack(">I", 1) + b"A2B0" + struct.pack(">II", 144, len(table)) + table
    return struct.pack(">I", len(profile)) + profile[4:]


# 15.2.5's n-channel colour: a six-channel profile, placed as a `/DeviceN`,
# three fills in it and one through a `SolidColorBrush`.
def ncl(components):
    return "ContextColor /Resources/n.icc 1.0,%s" % components


write(
    "wpf-n-channel.xps",
    "<FixedPage.Resources><ResourceDictionary>"
    '<SolidColorBrush x:Key="b" Color="%s" />' % ncl("0,0,0.5,0,0,1")
    + "</ResourceDictionary></FixedPage.Resources>"
    + '<Path Fill="%s" Data="M100,100L300,100 300,200 100,200Z" />' % ncl("0,0,0,0,0,0")
    + '<Path Fill="%s" Data="M100,300L300,300 300,400 100,400Z" />' % ncl("1,0,0,0,0,0")
    + '<Path Fill="%s" Data="M100,500L300,500 300,600 100,600Z" />' % ncl("0.5,0.25,0,0,0,1")
    + '<Path Fill="{StaticResource b}" Data="M100,700L300,700 300,800 100,800Z" />',
    extra_parts=[("Resources/n.icc", n_channel_lut(6))],
    extra_types='<Default Extension="icc" ContentType="application/vnd.ms-color.iccprofile" />',
    extra_rels='<Relationship Type="http://schemas.microsoft.com/xps/2005/06/required-resource" '
    'Target="/Resources/n.icc" Id="Rn" />',
)

# 18.3.1.2's two interpolation modes over the same stops: sRGB stated, then
# scRGB over three stops, a hard edge, a radial and fading alpha.
SCRGB = 'ColorInterpolationMode="ScRgbLinearInterpolation"'
THREE = stop("#FFDC143C", "0") + stop("#FFFFD700", "0.5") + stop("#FF2E8B57", "1")
write(
    "wpf-colour-interpolation.xps",
    "<FixedPage.Resources><ResourceDictionary>"
    + linear("s", THREE, 'ColorInterpolationMode="SRgbLinearInterpolation"')
    + linear("l", THREE, SCRGB)
    + linear(
        "h",
        stop("#FF000000", "0") + stop("#FF000000", "0.5") + stop("#FFFFFFFF", "0.5")
        + stop("#FF191970", "1"),
        SCRGB,
    )
    + '<RadialGradientBrush x:Key="r" MappingMode="Absolute" SpreadMethod="Pad" %s '
    'Center="150,150" RadiusX="150" RadiusY="150" GradientOrigin="120,120">'
    "<RadialGradientBrush.GradientStops>%s%s</RadialGradientBrush.GradientStops>"
    "</RadialGradientBrush>" % (SCRGB, stop("#FFFFFFFF", "0.25"), stop("#FF191970", "0.75"))
    + linear("a", stop("#FFDC143C", "0") + stop("#002E8B57", "1"), SCRGB)
    + "</ResourceDictionary></FixedPage.Resources>"
    + path("s", 40)
    + path("l", 160)
    + path("h", 280)
    + '<Path Fill="{StaticResource r}" RenderTransform="1,0,0,1,100,400" '
    'Data="M0,0L300,0 300,300 0,300Z" />'
    + path("a", 740),
)
