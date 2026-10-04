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


def noise(x, y, k):
    return (((x * 73856093) ^ (y * 19349663) ^ (k * 83492791)) % 4294967296) >> 13 & 255


def mixed(x, y):
    """Tiles of the recipe repeated (back-references) beside noise (literals)."""
    if (x // 8 + y // 8) % 2 == 0:
        return rgb(x % 8, y % 8)
    return (noise(x, y, 1), noise(x, y, 2), noise(x, y, 3))


def diagonal(x, y):
    """Noise constant along each anti-diagonal: every pixel is its top-right
    neighbour, so VP8L's top-right predictor wins in every block."""
    return (noise(x + y, 0, 1), noise(x + y, 0, 2), noise(x + y, 0, 3))


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


# PhotometricInterpretation 8, CIE L*a*b* (TIFF 6.0 §23): L* unsigned, a* and
# b* signed two's complement. The recipe is the RGB one read as offset binary:
# L* is the red channel, and a* and b* are the green and blue channels less
# 128, stored as the signed bytes they are — the green and blue recipe with
# the top bit flipped. A decoder that flips it back hands back the RGB recipe
# exactly, and a PDF `/Lab` over `/Range [-128 127 -128 127]` reads those
# bytes as L* = 100 r / 255, a* = g - 128, b* = b - 128. Uncompressed and
# deflated at 8 bits, and uncompressed at 16, where each recipe byte is
# widened by 257 and the top bit of the sixteen is flipped the same way.
def cielab_array():
    a = np.zeros((TH, TW, 3), dtype=np.uint8)
    for y in range(TH):
        for x in range(TW):
            r, g, b = rgb(x, y)
            a[y, x] = (r, g ^ 0x80, b ^ 0x80)
    return a


def cielab16_array():
    a = np.zeros((TH, TW, 3), dtype=np.uint16)
    for y in range(TH):
        for x in range(TW):
            r, g, b = rgb(x, y)
            a[y, x] = (r * 257, (g * 257) ^ 0x8000, (b * 257) ^ 0x8000)
    return a


tifffile.imwrite("tiff/tifffile-cielab-13x7.tif", cielab_array(), photometric="cielab")
tifffile.imwrite(
    "tiff/tifffile-cielab-deflate-13x7.tif", cielab_array(), photometric="cielab",
    compression="zlib",
)
tifffile.imwrite("tiff/tifffile-cielab16-13x7.tif", cielab16_array(), photometric="cielab")


# ---- WebP --------------------------------------------------------------------
#
# Pillow 12.3.0's WebP plugin over its bundled libwebp, and imagecodecs'
# `webp_encode` over libwebp 1.6.0. Lossless throughout this block: the
# expected answer is the recipe, exactly. `exact=True` keeps the colour under a
# fully transparent pixel, which libwebp otherwise rewrites.


def webp_rgba(w, h):
    return Image.fromarray(rgba_array(w, h), "RGBA")


def few_colours(w, h, n):
    im = Image.new("RGB", (w, h))
    im.putdata([palette(index(x, y, n)) for y in range(h) for x in range(w)])
    return im


rgb_image(W, H).save("webp/pillow-lossless-rgb-13x7.webp", lossless=True)
webp_rgba(W, H).save("webp/pillow-lossless-rgba-13x7.webp", lossless=True, exact=True)
# Method 6 at quality 100 is libwebp's slowest and tries every transform; a
# larger picture gives the meta prefix codes blocks to differ across.
rgb_image(96, 64).save("webp/pillow-lossless-m6-96x64.webp", lossless=True, method=6, quality=100)
webp_rgba(96, 64).save("webp/pillow-lossless-rgba-m6-96x64.webp", lossless=True, method=6,
                       quality=100, exact=True)
# Repeated tiles beside noise, at a size where LZ77 distances, the colour
# cache and more than one prefix code group all earn their place.
mixed_image = Image.new("RGBA", (160, 96))
mixed_image.putdata([mixed(x, y) + (alpha(x, y) | 1,) for y in range(96) for x in range(160)])
mixed_image.save("webp/pillow-lossless-mixed-160x96.webp", lossless=True, method=6, quality=100,
                 exact=True)
# The top-right predictor chosen in the last column too, where §3.5.1 makes
# the top-right pixel the first of the row being predicted.
diagonal_image = Image.new("RGB", (64, 32))
diagonal_image.putdata([diagonal(x, y) for y in range(32) for x in range(64)])
diagonal_image.save("webp/pillow-lossless-diagonal-64x32.webp", lossless=True, method=6,
                    quality=100)
# Two, four and sixteen colours: the colour-indexing transform at each of its
# three bundling widths.
for n, w, h in ((2, 21, 5), (4, 13, 7), (16, 21, 9)):
    few_colours(w, h, n).save(f"webp/pillow-lossless-{n}colour-{w}x{h}.webp", lossless=True)
# Two frames, the second the first mirrored.
first = rgb_image(W, H)
first.save("webp/pillow-animated-lossless-13x7.webp", lossless=True, save_all=True,
           append_images=[first.transpose(Image.Transpose.FLIP_LEFT_RIGHT)], duration=100)
open("webp/imagecodecs-lossless-rgba-13x7.webp", "wb").write(
    imagecodecs.webp_encode(rgba_array(W, H), lossless=True))


# ---- WebP, lossy ---------------------------------------------------------------
#
# Run 2 October 2026, the same Pillow and imagecodecs.
#
# A lossy picture has no exact answer of its own, and its answer is still
# not another decoder's (ruling 13): the test holds each file's colour to the
# recipe within a stated error, and the VP8 decoder itself to the WebM
# project's published test-vector MD5s. The alpha of a lossy WebP is lossless
# at `alpha_quality=100`, so where a file has alpha the test holds it to the
# recipe exactly. `smooth` gives the encoder gradients to spend few bits on;
# `rgb`'s wrap-arounds give it edges.
#
# *Changed the same day, on review.* The first run also decoded each file
# through Pillow, over libwebp 1.6.0, and committed the picture as
# `<name>.libwebp.png`, which the test then took as its expected answer --
# an outside program's output adjudicating this decoder. Those PNGs are
# deleted and this script no longer writes them; the `.webp` files are the
# bytes that run encoded, unchanged.


def smooth(x, y):
    return ((x * 3 + y * 2) % 256, (200 - x - y) % 256, (x * y // 16 + 40) % 256)


def smooth_image(w, h):
    im = Image.new("RGB", (w, h))
    im.putdata([smooth(x, y) for y in range(h) for x in range(w)])
    return im


def lossy(im, name, **options):
    im.save(f"webp/{name}.webp", **options)


lossy(rgb_image(61, 45), "pillow-lossy-rgb-61x45", quality=80)
lossy(smooth_image(96, 64), "pillow-lossy-smooth-q10-m6-96x64", quality=10, method=6)
lossy(smooth_image(61, 45), "pillow-lossy-smooth-q100-61x45", quality=100)
# Quality 55 puts a segment's loop-filter level at exactly 15, where §15's
# high-edge-variance threshold steps from 0 to 1.
lossy(smooth_image(61, 45), "pillow-lossy-smooth-q55-61x45", quality=55)
lossy(webp_rgba(61, 45), "pillow-lossy-rgba-61x45", quality=80, alpha_quality=100, exact=True)
lossy(webp_rgba(61, 45), "pillow-lossy-rgba-m0-61x45", quality=80, alpha_quality=100, exact=True,
      method=0)
first = smooth_image(61, 45)
first.save("webp/pillow-animated-lossy-61x45.webp", quality=80, save_all=True,
           append_images=[first.transpose(Image.Transpose.FLIP_LEFT_RIGHT)], duration=100)
open("webp/imagecodecs-lossy-rgb-61x45.webp", "wb").write(
    imagecodecs.webp_encode(np.asarray(rgb_image(61, 45)), level=75, lossless=False))
