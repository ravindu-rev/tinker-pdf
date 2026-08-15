# JavaScript and WebAssembly binding

wasm-bindgen directly over the `tinker-pdf` facade — not through the C ABI,
which would add a second error translation for nothing. Scope and design:
[`docs/plans/13-bindings.md`](../../docs/plans/13-bindings.md); packaging:
[gap 26](../../docs/plans/gaps/26-binding-packaging.md).

```bash
wasm-pack build --release --target web --out-dir pkg bindings/js
```

## ESM only, one build, `--target web`

Gap 26 lists "no decision recorded about the ESM/CJS split" as one of its
absences. This is the decision.

**The package is ESM and there is no CommonJS build.** `package.json` carries
`"type": "module"` and one entry point, built with wasm-pack's `web` target.

A dual package is not two wrappers around one artefact — it is two artefacts.
wasm-pack's `nodejs` target emits CommonJS that loads the `.wasm` with a
*synchronous* `fs.readFileSync` at module scope, which is exactly what a
browser cannot do; its `web` target emits ESM that fetches the `.wasm`. The
loader is the difference, so shipping both means shipping two builds of the
engine that can diverge, and ruling 11's whole point is that a binding has no
behaviour of its own to diverge *with*. One of them would get less use and
would break quietly.

The `web` target was chosen over `bundler` for the same reason the demo exists:
it loads from a plain `<script type="module">` with no build step at all, which
is what a page renders a PDF from. `bundler` needs webpack or vite in front of
it.

**Node runs the same file.** Node ≥ 18 executes ESM natively, and the one thing
the `web` target does that Node cannot follow — `fetch` on the `file:` URL of
its own `.wasm` — is avoided by handing `init` the bytes instead:

```js
import init, { PdfDocument } from 'tinker-pdf-js';
import { readFileSync } from 'node:fs';

await init({ module_or_path: readFileSync('node_modules/tinker-pdf-js/tinker_pdf_js_bg.wasm') });
```

One argument, not a second build. `bindings/js/tests/node_smoke.mjs` is that
call, run against an actually-`npm install`ed tarball.

The package is named **`tinker-pdf-js`**, after the crate. Plan 13 sketched
`@tinker/pdf`; wasm-pack derives the npm name and version from `Cargo.toml`, so
taking the derived one means the npm version cannot drift from the workspace —
a hand-edited `package.json` would be a fifth manifest for `cargo xtask
versions` to police, and the file is regenerated on every build anyway. Plan 13
is amended.

## The one footgun, and why it has the long name

```js
const bitmap = doc.renderPage(0, 1.0);

const pixels = bitmap.data();                       // a copy. Safe to keep.
const view   = bitmap.viewUnsafeUntilNextAllocation(); // aliases wasm memory.
```

`view()` returns a `Uint8Array` pointing **into wasm linear memory**. Any later
allocation may grow that memory, and growing it *detaches* the ArrayBuffer the
view was wrapping: the view silently becomes **zero length**, and so does every
other view anybody is holding. It does not throw, and it does not happen on
every call — only when an allocation crosses a page boundary — so a page that
holds a view across one render will work for months and then not.

This is observed behaviour, not a warning written from memory.
`node_smoke.mjs` renders a page, takes a view, renders the same page at four
times the scale, and asserts the view's length has become 0. If wasm-bindgen or
the engine ever changes that, the test fails and this paragraph is what needs
correcting.

So: **draw from the view immediately and drop it.** If the pixels must outlive
the next engine call, use `data()`. The safe call has the short name; the
dangerous one has the warning in its name, its doc comment and here.

## The browser demo

Plan 13's exit criterion for this binding: a page that renders an uploaded PDF.

```bash
wasm-pack build --release --target web --out-dir pkg bindings/js
python -m http.server -d bindings/js 8080     # then open /demo/
```

It is one HTML file with no build step, which is the `web` target paying off.
A `file://` URL will not work — the module fetches its own `.wasm` and browsers
refuse cross-origin fetches from `file:`.

The page reports a **non-white pixel count** beside the canvases, and that is
not decoration. A canvas created at the right size and never painted is
indistinguishable from a rendered one at a glance, and `testdata/simple-text.pdf`
produces exactly that until a font face is uploaded. The number is what tells
the two apart, so the page says it rather than leaving it to be noticed.

`demo/verify.mjs` drives the page in headless Chromium and asserts it:

```bash
cd bindings/js/demo && npm install && npx playwright install chromium
node verify.mjs ../../../testdata/simple-text.pdf /path/to/face.ttf
```

It renders **twice** — blank without a face, inked with one — reads the ink
back off the canvas with `getImageData` rather than trusting the status line,
and checks the ink's *bounding box* is the shape of a line of text rather than
a full-page smear or one stray pixel. Observed on Windows: 2385 non-white
pixels in a box of 108,130–412,155 on an 893×1263 canvas.

Playwright is build tooling (rule 1's documented exception) and is installed on
demand; `demo/package.json` is `"private": true` and is never published.

## Size

The `.wasm` is 2.03 MB, **1.40 MB gzipped**, with `cmap-predefined` on — that
is the default, and it carries all 202 of Adobe's predefined CMaps. Plan 13's
budget is 2.5 MB gzipped. Turning the feature off is
`--no-default-features`, and it is the switch a host that renders no CJK
reaches for.

## Nothing has been published

`npm install tinker-pdf-js` does not work and is not meant to yet. The pipeline
exists and has been exercised as a dry run; the facade is not frozen until
0.1.0 ([plan 00](../../docs/plans/00-architecture.md)).
