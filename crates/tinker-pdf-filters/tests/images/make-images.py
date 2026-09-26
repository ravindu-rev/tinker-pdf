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
