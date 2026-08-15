// Drives `demo/index.html` in a real browser and asserts that pixels arrived.
//
//   wasm-pack build --release --target web --out-dir pkg bindings/js
//   cd bindings/js/demo && npm install && node verify.mjs
//
// Plan 13 names the browser demo as the JavaScript binding's exit criterion:
// "a page that renders an uploaded PDF". This is that sentence, executed.
//
// ## Why a browser and not the Node smoke test
//
// `tests/node_smoke.mjs` already proves the engine renders through the
// installed package. It cannot prove the two things this file exists for,
// because neither exists in Node: the module fetching its own `.wasm` over
// HTTP (the `web` target's default path, and the one a page actually takes),
// and the pixels surviving the trip through `ImageData` onto a `<canvas>`.
//
// ## Why the ink is read back off the canvas
//
// Not from the bitmap, and not from the status line. A canvas that was created
// at the right size and never painted looks identical in an accessibility
// snapshot and in a screenshot thumbnail. So `getImageData` reads the canvas
// the browser actually holds, counts non-white pixels, and computes their
// bounding box — the box is what tells a line of text from a full-page smear,
// which is the assertion gap 01 found to be the only one that distinguishes a
// right glyph from a wrong one.
//
// ## Why it renders twice
//
// `testdata/simple-text.pdf` embeds no font program and this engine bundles no
// faces, so the first render must be **blank** and the second, after a face is
// uploaded, must be inked. A test that only checked the second would pass on a
// page that painted something arbitrary.

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { chromium } from 'playwright';

const here = path.dirname(fileURLToPath(import.meta.url));
const jsRoot = path.resolve(here, '..');
const repoRoot = path.resolve(jsRoot, '../..');

const PDF = process.argv[2] ?? path.join(repoRoot, 'testdata/simple-text.pdf');
const FACE = process.argv[3];
if (!FACE) {
  console.error('usage: node verify.mjs [file.pdf] <face.ttf>');
  console.error('a face is required: without one this document draws nothing, by design');
  process.exit(2);
}

const TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.ts': 'text/plain',
};

// A file: URL cannot be used: the module fetches its own .wasm and browsers
// refuse cross-origin fetches from file:. So the demo is served, which is also
// how anybody would actually run it.
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  let file = path.join(jsRoot, decodeURIComponent(url.pathname));
  if (url.pathname.endsWith('/')) file = path.join(file, 'index.html');
  // No traversal outside the served root, which matters because this serves
  // from inside a checkout.
  if (!path.resolve(file).startsWith(jsRoot)) {
    response.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, { 'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});

await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const { port } = server.address();
const url = `http://127.0.0.1:${port}/demo/`;
console.log(`serving ${jsRoot} at ${url}`);

const browser = await chromium.launch();
let failure = null;
try {
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(String(error)));
  page.on('console', (message) => {
    if (message.type() === 'error' && !message.text().includes('favicon')) {
      errors.push(message.text());
    }
  });

  await page.goto(url);
  await page.waitForFunction(
    () => document.getElementById('status').textContent.includes('ready'),
  );
  console.log(await page.textContent('#status'));

  // Blank first: no embedded font, no uploaded face.
  await page.setInputFiles('#pdf', PDF);
  await page.waitForFunction(
    () => document.querySelectorAll('#pages canvas').length > 0,
  );
  const blank = await measure(page);
  console.log(`without a face: ${JSON.stringify(blank)}`);
  if (blank.ink !== 0) {
    throw new Error(
      `${blank.ink} pixels of ink before a face was supplied. Either this fixture ` +
        'gained an embedded font, or this test is no longer measuring what it thinks.',
    );
  }

  // Inked second.
  await page.setInputFiles('#face', FACE);
  await page.waitForFunction(
    () => !document.getElementById('status').textContent.includes('Nothing was drawn'),
  );
  const drawn = await measure(page);
  console.log(`with a face:    ${JSON.stringify(drawn)}`);
  console.log(await page.textContent('#status'));

  if (drawn.pages !== 3) throw new Error(`${drawn.pages} canvases, expected 3`);
  if (drawn.ink < 100) throw new Error(`only ${drawn.ink} pixels of ink on the canvas`);
  // One line of text near the top of a 1263-pixel-tall page. A full-page smear
  // or a single stray pixel both have plenty of ink and neither has this shape.
  const [minX, minY, maxX, maxY] = drawn.box;
  const width = maxX - minX;
  const height = maxY - minY;
  if (width < 100 || width > drawn.canvas[0] * 0.8) {
    throw new Error(`the ink is ${width} wide on a ${drawn.canvas[0]} canvas; that is not a line of text`);
  }
  if (height < 5 || height > drawn.canvas[1] * 0.1) {
    throw new Error(`the ink is ${height} tall on a ${drawn.canvas[1]} canvas; that is not a line of text`);
  }
  if (errors.length) throw new Error(`the page reported: ${errors.join('; ')}`);

  console.log('DEMO-RENDER: RAN, and the canvas holds a rendered page');
} catch (error) {
  failure = error;
} finally {
  await browser.close();
  server.close();
}

if (failure) {
  console.error(String(failure));
  process.exit(1);
}

// Reads the first canvas the browser is actually holding.
function measure(page) {
  return page.evaluate(() => {
    const canvases = [...document.querySelectorAll('#pages canvas')];
    const canvas = canvases[0];
    const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
    let ink = 0;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -1;
    let maxY = -1;
    for (let y = 0; y < canvas.height; y += 1) {
      for (let x = 0; x < canvas.width; x += 1) {
        if (data[(y * canvas.width + x) * 4] !== 255) {
          ink += 1;
          if (x < minX) minX = x;
          if (x > maxX) maxX = x;
          if (y < minY) minY = y;
          if (y > maxY) maxY = y;
        }
      }
    }
    return { pages: canvases.length, canvas: [canvas.width, canvas.height], ink, box: [minX, minY, maxX, maxY] };
  });
}
