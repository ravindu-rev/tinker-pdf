// Writes the GIFs Pillow cannot, from the same authored pixels.
//
// Pillow's GIF writer always codes with 256 roots. omggif sizes the LZW root
// set from the palette, as GIF89a Appendix F allows — 2 bits for a 2- or
// 4-colour table, 4 for 16 — and writes a first image smaller than its
// logical screen, with a local table, which is the one shape of the format
// that cannot keep a single index space.
//
//   cd crates/tinker-pdf-filters/tests/images
//   npm install omggif@1.0.10    (anywhere on NODE_PATH; MIT, Dean McNamee)
//   node make-gif.js
//
// Run once, 26 September 2026, Node v22.22.2, Linux x86_64. Not run by any
// test: the committed files are the record. The recipe is `make-images.py`'s
// and `tests/image_fixtures.rs`'s, restated.

"use strict";
const fs = require("fs");
const { GifWriter } = require("omggif");

const index = (x, y, n) => (x + 3 * y) % n;
const palette = (i) => [(i * 7) % 256, (i * 13 + 50) % 256, (255 - i) % 256];
const packed = ([r, g, b]) => (r << 16) | (g << 8) | b;
const table = (n, offset) =>
  Array.from({ length: n }, (_, i) => packed(palette(i + offset)));

function indices(w, h, n) {
  const out = [];
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) out.push(index(x, y, n));
  return out;
}

function write(name, w, h, gopts, frames) {
  const buf = Buffer.alloc(65536);
  const gw = new GifWriter(buf, w, h, gopts);
  for (const f of frames) gw.addFrame(f.x, f.y, f.w, f.h, f.pixels, f.opts);
  fs.writeFileSync("gif/" + name, buf.subarray(0, gw.end()));
}

// A whole screen at 2, 1 and 4 bits of roots (omggif floors the code size at
// 2, so the two-colour file codes with four roots and uses two).
for (const [n, w, h] of [[4, 13, 7], [2, 21, 5], [16, 21, 9]]) {
  write(`omggif-${n}colour-${w}x${h}.gif`, w, h, { palette: table(n, 0) }, [
    { x: 0, y: 0, w, h, pixels: indices(w, h, n), opts: {} },
  ]);
}

// A 6 x 4 image at (3, 2) on a 13 x 7 screen, carrying an eight-entry local
// table (the recipe's palette from entry 100) and transparent index 2. The
// global table is four entries and the background is its entry 1.
write("omggif-local-offset-13x7.gif", 13, 7, { palette: table(4, 0), background: 1 }, [
  { x: 3, y: 2, w: 6, h: 4, pixels: indices(6, 4, 8), opts: { palette: table(8, 100), transparent: 2 } },
]);

// An 8 x 5 image at (2, 1) using the sixteen-entry global table, background 7.
write("omggif-global-offset-13x7.gif", 13, 7, { palette: table(16, 0), background: 7 }, [
  { x: 2, y: 1, w: 8, h: 5, pixels: indices(8, 5, 16), opts: {} },
]);
