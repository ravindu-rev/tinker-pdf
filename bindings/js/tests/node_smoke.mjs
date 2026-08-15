// Proves an *installed* npm package is the engine, and demonstrates the one
// footgun plan 13 requires be documented.
//
//   cd <a directory where `npm install <tarball>` has run>
//   node <this file> <path to a PDF> <path to a TrueType face>
//
// Two things are asserted that a weaker smoke test would miss.
//
// **Ink, not dimensions.** `testdata/simple-text.pdf` names Helvetica and
// embeds no font program; the engine bundles no faces and a browser has no
// font directory, so rendering it without `setFonts` returns a correctly
// sized, entirely blank bitmap. "A bitmap of the right size came back" passes
// on a package whose renderer does nothing. So the render is asserted twice,
// blank and then inked, which also exercises `setFonts` — the call every
// browser host must make.
//
// **The view really is invalidated.** `viewUnsafeUntilNextAllocation()`
// returns a `Uint8Array` aliasing wasm linear memory. Growing that memory
// *detaches* the JavaScript ArrayBuffer that was wrapping it, and every view
// into it becomes zero length — silently, at whatever later moment an
// allocation happens to cross a page boundary. This test makes that happen on
// purpose and asserts it, so the documentation is describing observed
// behaviour rather than a warning somebody wrote from memory.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import process from 'node:process';

const [pdfPath, facePath] = process.argv.slice(2);
if (!pdfPath || !facePath) {
  console.error('usage: node node_smoke.mjs <file.pdf> <face.ttf>');
  process.exit(2);
}

// Resolved by package name from the *current directory*, not by a path
// relative to this file: that is what makes this a test of the install rather
// than of the build directory. `import.meta.resolve` would resolve against
// this script's own location, which is inside the repository and has no
// `node_modules` — so the resolution is anchored to the cwd deliberately.
const require = createRequire(pathToFileURL(path.join(process.cwd(), 'package.json')));
const entry = pathToFileURL(require.resolve('tinker-pdf-js')).href;
console.log(`resolved tinker-pdf-js to ${entry}`);
const module_ = await import(entry);
const init = module_.default;
const { PdfDocument, version } = module_;

// The `web` target's default init fetches `tinker_pdf_js_bg.wasm` relative to
// its own URL. Node's fetch does not do file: URLs, so the bytes are handed
// over directly — the same entry point, one argument different. This is why
// the package needs no second CommonJS build: see bindings/js/README.md.
const wasmUrl = new URL('tinker_pdf_js_bg.wasm', entry);
await init({ module_or_path: readFileSync(fileURLToPath(wasmUrl)) });

const ink = (bitmap, bytes) => {
  let painted = 0;
  for (let i = 0; i < bytes.length; i += 4) {
    if (bytes[i] !== 0xff) painted += 1;
  }
  return painted;
};

console.log(`engine version ${version()}`);

const pdf = readFileSync(pdfPath);
const face = readFileSync(facePath);
const doc = new PdfDocument(pdf);
console.log(
  `pages=${doc.pageCount} encrypted=${doc.isEncrypted} ` +
    `size=${doc.pageWidth(0)}x${doc.pageHeight(0)}`,
);
if (doc.pageCount !== 3) throw new Error(`pageCount ${doc.pageCount}`);

const text = doc.pageText(0);
if (!text.includes('Tinker fixture')) throw new Error(`text ${JSON.stringify(text)}`);
console.log(`text=${JSON.stringify(text.trim().slice(0, 40))}`);

const bare = doc.renderPage(0, 1.0);
if (ink(bare, bare.data()) !== 0) {
  throw new Error('this fixture embeds no font, so it must draw nothing yet');
}

doc.setFonts(face, undefined, undefined, undefined);
const drawn = doc.renderPage(0, 1.0);
const copied = drawn.data();
const painted = ink(drawn, copied);
console.log(`bitmap ${drawn.width}x${drawn.height} bytes=${copied.length} ink=${painted}`);
if (painted < 100) throw new Error(`only ${painted} pixels of ink with a face supplied`);
if (copied.length !== drawn.width * drawn.height * 4) {
  throw new Error(`pixel buffer is ${copied.length}`);
}

// The documented footgun, demonstrated rather than asserted from memory.
const view = drawn.viewUnsafeUntilNextAllocation();
if (view.length !== copied.length) {
  throw new Error(`the view is ${view.length} bytes and the copy is ${copied.length}`);
}
if (view[0] !== copied[0]) throw new Error('the view and the copy disagree');

// Force the wasm heap to grow. Rendering the same page four times larger
// allocates roughly sixteen times the pixels, which crosses a page boundary on
// any heap this engine starts with.
doc.renderPage(0, 4.0);
console.log(`after an allocation, the view is ${view.length} bytes`);
if (view.length !== 0) {
  throw new Error(
    'the view survived an allocation that grew wasm memory. Either the heap did ' +
      'not grow — in which case this test no longer demonstrates anything and ' +
      'must be made to allocate more — or the aliasing contract has changed and ' +
      'the documentation in src/lib.rs and README.md is now wrong.',
  );
}
// And the copy is untouched, which is the whole reason `data()` is the short
// name and the view has the long one.
if (copied.length === 0 || ink(drawn, copied) !== painted) {
  throw new Error('the copy was disturbed by an allocation');
}

console.log('NODE-SMOKE: RAN, rendered, inked, and the view was invalidated');
