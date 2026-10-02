# Container images somebody else encoded

The decoders the tier-4 archive row added — BMP, GIF and WebP — and the TIFF
decoder's additions from its TIFF row are held here to
pictures this repository **authored** and a **third-party encoder** wrote.
Ruling 13's rule for a lossless codec is that the expected output is the
generator's input, never another decoder's output, so the device is the one
`tests/cbz/README.md` and `tests/epub/README.md` use for containers, applied to
pixels: author the picture as a formula, let somebody else's encoder turn it
into bytes once, commit the bytes, and recompute the formula in the test.

`make-images.py` and `make-gif.js` are the generators and
`tests/image_fixtures.rs` is the test. All three carry the same functions —
`rgb`, `alpha`, `grey`, `index`, `palette`, `bit`, or the subset a script
needs — and a change to one without the others fails every picture.
No test runs the script and no test spawns a program (`cargo xtask oracles`);
the committed files are the record.

## What produced them

| Directory | Producer | Command | Date |
| --- | --- | --- | --- |
| `bmp/pillow-*.bmp` | **Pillow 12.3.0** (`PIL.BmpImagePlugin`), CPython 3.11.15, Linux x86_64 | `python3 make-images.py` | 26 September 2026 |
| `bmp/imagecodecs-*.bmp` | **imagecodecs 2026.3.6** (`imagecodecs.bmp_encode`), numpy 2.4.6 | the same script | 26 September 2026 |
| `bmpsuite/*.bmp` | **bmpsuite 2.8**, Jason Summers' generator, at [`jsummers/bmpsuite`](https://github.com/jsummers/bmpsuite) `555e43a` (2023-11-28) | `make` with gcc 13.3.0, then `make check`, which verified every generated file against upstream's own `checksums` | 26 September 2026 |
| `gif/pillow-*.gif` | **Pillow 12.3.0** (`PIL.GifImagePlugin`) | `python3 make-images.py` | 26 September 2026 |
| `gif/omggif-*.gif` | **omggif 1.0.10** (Dean McNamee, MIT, from npm), Node v22.22.2 | `npm install omggif@1.0.10`, then `node make-gif.js` | 26 September 2026 |
| `webp/pillow-*.webp` | **Pillow 12.3.0** (`PIL.WebPImagePlugin`) over its bundled libwebp 1.6.0 | `python3 make-images.py` | 26 September 2026 (lossless), 2 October 2026 (lossy) |
| `webp/imagecodecs-*.webp` | **imagecodecs 2026.3.6** (`imagecodecs.webp_encode`) over libwebp 1.6.0 | the same script | the same two days |
| `webp/*.libwebp.png` | **libwebp 1.6.0's decode** of the lossy file beside it, through Pillow 12.3.0, written as PNG by Pillow | the same script | 2 October 2026 |
| `tiff/tifffile-*.tif` | **tifffile 2026.3.3** over imagecodecs 2026.3.6 (zlib; JPEG 2000 through OpenJPEG 2.5.4, `level=0`, lossless) | `python3 make-images.py` | 26 September 2026 |

Pillow's BMP writer produces `BITMAPINFOHEADER`, `BI_RGB`, bottom-up files at
1, 8, 24 and 32 bits — and at 32 it puts an RGBA image's alpha into the byte
the header documentation says is unused, which is why
`a_32_bit_bi_rgb_file_is_its_colour_and_opaque` exists. imagecodecs' writer is
the one on this machine that states alpha the way BMP can: `BI_BITFIELDS`
under a `BITMAPV4HEADER` with an alpha mask.

**Neither writes RLE4, RLE8, 2 or 4 bits a pixel, 16-bit bit fields, a
top-down file or an OS/2 header**, and nothing else installable here does.
That is what bmpsuite is for.

Pillow's GIF writer is GIF89a over a global table and **always codes LZW with
256 roots**, whatever the palette, so it cannot exercise the one thing about
GIF's LZW that is not `/LZWDecode`'s. It writes the palette, grey, interlaced
(Pillow's default once both sides reach 16), local-table
(`include_color_table=True`), transparent (`transparency=5`) and two-frame
files. omggif sizes the root set from the palette — two bits for two or four
colours, four for sixteen — and writes a first image smaller than its logical
screen, over the global table and over a local one with a transparent index.
It does not interlace, which is why the interlaced file is Pillow's. Every
test that depends on one of those features reads the descriptor byte that
says the fixture has it, rather than trusting either encoder's documentation.

tifffile writes each TIFF shape the TIFF row added: CMYK (uncompressed and
deflated), signed samples at 8, 16 (big-endian) and 32 bits (with
`Predictor` 2), IEEE floats at 16 (big-endian), 32 (with `Predictor` 3) and
64 bits, BigTIFF in both byte orders, JPEG 2000 as one strip and as a padded
16 x 16 tile grid, and four directories with a reduced-resolution copy among
them. The float recipe is whole 128ths, which every width holds exactly, so
no encoder's rounding stands between the recipe and the file; a signed or
float sample's expected intensity is the recipe through the mapping
`tiff.rs`'s module note states, recomputed in the test. For the lossless JPEG
2000 files the expected answer is the generator's input, as for every other
file here.

Every lossless WebP here — `*-lossless-*` and the imagecodecs RGBA file — has
the recipe as its expected answer, exactly. Pillow writes RGB; RGBA with `exact=True`, which keeps the colour
under a fully transparent pixel where libwebp would otherwise rewrite it;
method 6 at quality 100, libwebp's exhaustive search, at 96 x 64 so the
transforms have blocks to differ across; two, four and sixteen colours, the
colour-indexing transform at each of its three bundling widths; a 160 x 96
picture of repeated tiles beside hash noise (`mixed`), so back-references,
the colour cache and literals all carry pixels; a 64 x 32 picture whose
every pixel is its top-right neighbour (`diagonal`), so libwebp picks the
top-right predictor in the last column too, where §3.5.1 makes that
neighbour the first pixel of the row being predicted; and a two-frame
animation.
imagecodecs writes RGBA through a second binding. The tests that claim a
transform read the VP8L header bits that say the fixture has one.

**A lossy WebP has no exact answer of its own**, so its answer is the
reference decoder's: `make-images.py` decodes each lossy file once through the
same Pillow, over libwebp 1.6.0, and commits that picture beside it as
`<name>.libwebp.png`; `image_fixtures.rs` reads the PNG with this crate's own
PNG decoder and holds every pixel to it. That is the standing the WebM
project's MD5s have for VP8 itself — `src/webp/vp8/tests.rs` — and it is what
makes the chroma upsampling and colour conversion checkable at all, since
RFC 6386 ends at the Y, U and V planes. The files: quality 80 over the `rgb`
recipe, quality 10 at method 6, 55 and 100 over `smooth` (gradients the
encoder spends few bits on; at 55 one segment's loop-filter level is exactly
15, where the high-edge-variance threshold steps), RGBA at `alpha_quality=100` — lossless alpha, so
the alpha is also held to the recipe — once unfiltered and once at method 0,
where libwebp chose the horizontal filter, a two-frame animation, and
imagecodecs' `webp_encode(lossless=False)` over `rgb`. Sizes of 61 x 45 crop
inside a macroblock and inside a chroma sample.

## bmpsuite, and what it can prove

The suite's generator writes one picture in several dozen ways. Its README:

> Image files generated by this program are not covered by this license, and
> are in the public domain (except for the embedded ICC profiles).

None of the committed files embeds a profile. The generator itself is
GPL-3.0-or-later and is **not** committed, nor is anything derived from its
source: the files are its output, run once, which is the standing a fixture
from any other tool has. They are renamed `g-*` and `q-*` after the suite's
"good" and "questionable" directories.

The suite ships no expected pixels a test could use without trusting some
decoder, so what is asserted is **relations**: files that describe one picture
must decode to one picture. `pal8rle` must be `pal8`; `pal4rle` must be
`pal4`; top-down, OS/2 1.x and 2.x, V5 and an oversized palette must all be
`pal8`; 32-bit `BI_RGB`, shuffled bit fields and garbage in the unused byte
must all be `rgb24`; three alpha files with three mask layouts must agree. The
chain ends at an expected answer nobody decoded, because the uncompressed
8-bit path those relations lean on is the one Pillow's authored palette pins.

SHA-256 of what was committed:

| File | SHA-256 |
| --- | --- |
| `g-pal1.bmp` | `c631861fa4c4e959d2abc6e0db0106253989b0101223cb08020e116e19c97f77` |
| `g-pal4.bmp` | `01f7bfaaf5110a404fb68451a860970750c155e5c46e1721fa6b9c500be0ca2c` |
| `g-pal4rle.bmp` | `3c26b6bdd22311e03b1ac0713b0a3b6af5e579bef16694cbea74ae0c4a2599e4` |
| `g-pal8.bmp` | `f9e38f114b12b6ba97cb9bff1b4f931cc58796439b8b722b38947d864ff5fcd9` |
| `g-pal8os2.bmp` | `2bfe739377020722872f7fa3ba3ff3ff0daa17f2f202c80e25cf66c4ec30e506` |
| `g-pal8rle.bmp` | `809938ab0e821710b5f566d85ba735870e5b8ef690eaca7f54a81cf1729fe10a` |
| `g-pal8topdown.bmp` | `e06cf94cc7fb87a841438f304dd90c902763ccea50994ec87bde08fcf5e69d63` |
| `g-pal8v5.bmp` | `65ac4a579189d2fc30b3499b8246f30127e032f35d6cde90335dafe87a1b23c3` |
| `g-pal8w126.bmp` | `1ade7bd93a15ed45be63e883190e30aaf9e965fd3b839d9ac4aa646ac6e0f946` |
| `g-rgb16-565.bmp` | `c2ffadac9c1239fb397834415c7b5f85d5c6044bd9c31fa66e23056a19b82b1d` |
| `g-rgb16.bmp` | `fcecd482048c853d08255430c5bac462821522683f4429626442580058aa5e5e` |
| `g-rgb16bfdef.bmp` | `3c5d1d67482b769d875b552e6012b80434880f763c9d56a45a94d07bbbe16fba` |
| `g-rgb24.bmp` | `a9c4fbfbf8cb6df8d2d9d1484359d037aebd25078b21137bfd6c69739fcbe2e1` |
| `g-rgb32.bmp` | `b4c79cb8ffd2f1c096af27f9d82b5feaa0aa2cb049c791e0f6251de0435066e5` |
| `g-rgb32bf.bmp` | `4c1189e3a039b1e5aae6347024684937c495a5736a3c309dc7c2e3c161f89751` |
| `q-pal2.bmp` | `bac6eec4100831e635fcd34a9e0e34a8a9082abdec132ac327aa1bfc7137d40f` |
| `q-pal4rletrns.bmp` | `2fcbaa0f387c57ba678ced91dfb2db5b2e544e2ed28a7875459e551b514daf84` |
| `q-pal8os2v2.bmp` | `e2597b53091d734900068b42cddaed7430261cc5eade32b4f19fd6c2c4568a52` |
| `q-pal8oversizepal.bmp` | `51c89919fc6b85cc11850054ff16bb947c11dead620e35a92e46d7b0fde79faf` |
| `q-pal8rletrns.bmp` | `445b856331c5d03054887d6555f56a8882f0aabbe0d59e322d9ff0be7f0eee94` |
| `q-rgb32fakealpha.bmp` | `f9f583be33545dbc4d2332f84d8b8bb18efd2730b81c83bbeffffeffad67aa99` |
| `q-rgba32-1.bmp` | `6208cb30edd75b0ad23a54b9d64fea29aa264aedeec61b2bed6d4a0c75e86563` |
| `q-rgba32-2.bmp` | `257e0127fa0ccdb6eb8381338dda0fb692b2ea4d38775f1aed56dfdccaed2ab4` |
| `q-rgba32abf.bmp` | `1d2183466e05c768b5a9d2ce5d132d72db2272bb331c7b27560d24f197f4165e` |

## What this still does not buy

- **No expected pixels for RLE, 16-bit or the OS/2 headers** — only the
  relation to a twin. A decoder wrong in the same way on both halves of a
  relation passes it; the twins were chosen so that the uncompressed half is
  the simplest path in the decoder.
- **bmpsuite's RLE "delta" files leave pixels undefined by design**, and
  `bmpsuite_rle_deltas_define_what_they_define_and_warn_about_the_rest` holds
  only the defined ones to the twin. What an undefined pixel *is* — index 0
  here — is a decision `bmp.rs` states, not a fact any fixture can check.
