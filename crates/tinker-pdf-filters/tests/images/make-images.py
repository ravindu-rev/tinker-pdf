# Writes the container-image fixtures in this directory from authored pixels.
#
# Ruling 13's rule for a lossless codec: the expected output is the
# *generator's input*, never another decoder's output. So every picture below
# is a formula, the same formula `tests/image_fixtures.rs` carries as
# `recipe::*`, and a third-party encoder turns it into bytes once. The test
# recomputes the pixels from the formula and compares them with what this
# repository's decoder makes of the bytes; nothing here ever decodes anything.
#
#   cd crates/tinker-pdf-filters/tests/images && python3 make-images.py
#
# Encoders, as run on 26 September 2026 (Linux x86_64, CPython 3.11.15):
#   Pillow 12.3.0        pip install --user pillow
#   imagecodecs 2026.3.6 pip install --user imagecodecs  (its BMP writer)
#   numpy 2.4.6          what imagecodecs takes its arrays as
#   tifffile 2026.3.3    pip install --user tifffile
#
# `make-gif.js` beside this writes the GIFs Pillow cannot: Pillow's GIF
# writer always codes with 256 roots, and a GIF's LZW root size is the thing
# most worth varying.
#
# Not run by any test: the committed files are the record.

import numpy as np
from PIL import Image
import imagecodecs


# ---- the recipe: keep in step with `recipe` in tests/image_fixtures.rs -------

def rgb(x, y):
    return ((x * 29 + y * 7) % 256, (x * 3 + y * 41 + 17) % 256, (x * y + 101) % 256)


def alpha(x, y):
    return (x * 13 + y * 19 + 5) % 256


def grey(x, y):
    return (x * 5 + y * 9) % 256


def index(x, y, n):
    return (x + 3 * y) % n


def palette(i):
    return ((i * 7) % 256, (i * 13 + 50) % 256, (255 - i) % 256)


def bit(x, y):
    return 1 if (x + y) % 3 == 0 else 0


def rgb_image(w, h):
    im = Image.new("RGB", (w, h))
    im.putdata([rgb(x, y) for y in range(h) for x in range(w)])
    return im


def rgba_array(w, h):
    a = np.zeros((h, w, 4), dtype=np.uint8)
    for y in range(h):
        for x in range(w):
            a[y, x] = rgb(x, y) + (alpha(x, y),)
    return a


def palette_image(w, h, n):
    im = Image.new("P", (w, h))
    flat = []
    for i in range(n):
        flat.extend(palette(i))
    im.putpalette(flat)
    im.putdata([index(x, y, n) for y in range(h) for x in range(w)])
    return im


# ---- BMP ---------------------------------------------------------------------

W, H = 13, 7

# Pillow's BMP writer: BITMAPINFOHEADER, BI_RGB, bottom-up, at 1, 8, 24 and 32
# bits. Its 32-bit file puts alpha in the byte BITMAPINFOHEADER says is unused.
bits1 = Image.new("1", (21, 5))
bits1.putdata([255 if bit(x, y) else 0 for y in range(5) for x in range(21)])
bits1.save("bmp/pillow-1bit-21x5.bmp")

grey8 = Image.new("L", (W, H))
grey8.putdata([grey(x, y) for y in range(H) for x in range(W)])
grey8.save("bmp/pillow-grey-13x7.bmp")

palette_image(W, H, 256).save("bmp/pillow-palette-13x7.bmp")
rgb_image(W, H).save("bmp/pillow-rgb-13x7.bmp")
Image.fromarray(rgba_array(W, H), "RGBA").save("bmp/pillow-rgba-13x7.bmp")

# imagecodecs' BMP writer: 32-bit BI_BITFIELDS under a BITMAPV4HEADER with an
# alpha mask, which is the one way a BMP states alpha.
open("bmp/imagecodecs-rgba-13x7.bmp", "wb").write(imagecodecs.bmp_encode(rgba_array(W, H)))


# ---- GIF ---------------------------------------------------------------------

# Pillow's GIF writer: GIF89a, a global table, LZW with 256 roots always, and
# interlace on by default once both sides reach 16.
palette_image(W, H, 256).save("gif/pillow-palette-13x7.gif")
palette_image(40, 24, 256).save("gif/pillow-interlaced-40x24.gif", interlace=True)
palette_image(W, H, 256).save("gif/pillow-transparent-13x7.gif", transparency=5)
palette_image(W, H, 256).save("gif/pillow-local-table-13x7.gif", include_color_table=True)
grey8.save("gif/pillow-grey-13x7.gif")

# Two frames: the recipe, then its mirror. The first is the picture.
second = palette_image(W, H, 256).transpose(Image.Transpose.FLIP_LEFT_RIGHT)
palette_image(W, H, 256).save(
    "gif/pillow-animated-13x7.gif", save_all=True, append_images=[second], duration=100, loop=0
)


# ---- TIFF --------------------------------------------------------------------
#
# tifffile 2026.3.3 over imagecodecs 2026.3.6's codecs (zlib, and JPEG 2000
# through OpenJPEG 2.5.4). Each file exercises one of the archive row's TIFF
# additions; `tests/image_fixtures.rs` reads the tag that says so before it
# reads a pixel.

import tifffile

TW, TH = 13, 7


def plane(f, dtype):
    return np.array([[f(x, y) for x in range(TW)] for y in range(TH)], dtype=dtype)


def cmyk_array():
    a = np.zeros((TH, TW, 4), dtype=np.uint8)
    for y in range(TH):
        for x in range(TW):
            a[y, x] = rgb(x, y) + (alpha(x, y),)
    return a


# PhotometricInterpretation 5, InkSet 1: uncompressed (decoded) and deflated
# (placed as its own bytes). Cyan, magenta, yellow are the RGB recipe and
# black is the alpha recipe.
tifffile.imwrite("tiff/tifffile-cmyk-13x7.tif", cmyk_array(), photometric="separated")
tifffile.imwrite(
    "tiff/tifffile-cmyk-deflate-13x7.tif", cmyk_array(), photometric="separated",
    compression="zlib",
)

# SampleFormat 2: signed samples, at 8 bits (the grey recipe moved down by
# 128), 16 bits big-endian, and 32 bits with horizontal differencing.
tifffile.imwrite("tiff/tifffile-int8-13x7.tif", plane(lambda x, y: grey(x, y) - 128, np.int8))
tifffile.imwrite(
    "tiff/tifffile-int16-13x7.tif",
    plane(lambda x, y: (x * 1000 + y * 7777) % 65536 - 32768, np.int16),
    byteorder=">",
)
tifffile.imwrite(
    "tiff/tifffile-int32-predictor-13x7.tif",
    plane(lambda x, y: (x * 123456789 + y * 987654321) % 4294967296 - 2147483648,
          np.int64).astype(np.int32),
    predictor=True, compression="zlib",
)


# SampleFormat 3: floats around [0, 1] and past both ends, at 32 bits with
# Predictor 3, at 16 bits big-endian, and at 64 bits. Every value is a whole
# number of 128ths below 2, which all three widths hold exactly, so no
# encoder's rounding stands between the recipe and the file.
def float_value(x, y):
    return (3 * grey(x, y) - 64) / 128


tifffile.imwrite(
    "tiff/tifffile-float32-predictor3-13x7.tif", plane(float_value, np.float32),
    predictor=True, compression="zlib",
)
tifffile.imwrite("tiff/tifffile-float16-13x7.tif", plane(float_value, np.float16), byteorder=">")
tifffile.imwrite("tiff/tifffile-float64-13x7.tif", plane(float_value, np.float64))

# BigTIFF, both byte orders.
rgb_array = np.array([[rgb(x, y) for x in range(TW)] for y in range(TH)], dtype=np.uint8)
tifffile.imwrite("tiff/tifffile-bigtiff-rgb-13x7.tif", rgb_array, bigtiff=True, photometric="rgb")
tifffile.imwrite(
    "tiff/tifffile-bigtiff-mm-rgb-13x7.tif", rgb_array, bigtiff=True, photometric="rgb",
    byteorder=">",
)

# Compression 34712: one strip (placed as /JPXDecode) and tiled (decoded),
# both lossless, which is what OpenJPEG's `level=0` asks for.
tifffile.imwrite(
    "tiff/tifffile-jpeg2000-rgb-13x7.tif", rgb_array, photometric="rgb",
    compression="jpeg2000", compressionargs={"level": 0},
)
big_rgb = np.array([[rgb(x, y) for x in range(40)] for y in range(24)], dtype=np.uint8)
tifffile.imwrite(
    "tiff/tifffile-jpeg2000-tiled-40x24.tif", big_rgb, photometric="rgb", tile=(16, 16),
    compression="jpeg2000", compressionargs={"level": 0},
)

# Four directories: grey, RGB, a reduced-resolution copy (NewSubfileType 1),
# and the grey recipe inverted. A comic pages the three that are pages.
with tifffile.TiffWriter("tiff/tifffile-multipage.tif") as tw:
    tw.write(plane(grey, np.uint8), photometric="minisblack")
    tw.write(rgb_array, photometric="rgb")
    tw.write(plane(grey, np.uint8)[::2, ::2], photometric="minisblack", subfiletype=1)
    tw.write(plane(lambda x, y: 255 - grey(x, y), np.uint8), photometric="minisblack")
