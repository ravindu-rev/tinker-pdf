# JavaScript and WebAssembly binding

wasm-bindgen directly over the `tinker-pdf` facade — not through the C ABI,
which would add a second error translation for nothing. Scope, design and
packaging: [`docs/features/bindings.md`](../../docs/features/bindings.md).

```bash
wasm-pack build --release --target web --out-dir pkg bindings/js
```

## ESM only, one build, `--target web`

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

The `web` target is used rather than `bundler` for the same reason the demo
exists: it loads from a plain `<script type="module">` with no build step at
all, which is what a page renders a PDF from. `bundler` needs webpack or vite
in front of it.

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

The package is named **`tinker-pdf-js`**, after the crate. wasm-pack derives
the npm name and version from `Cargo.toml`, so taking the derived one means the
npm version cannot drift from the workspace — a hand-edited `package.json`
would be one more manifest for `cargo xtask versions` to police, and the file
is regenerated on every build anyway.

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

`node_smoke.mjs` renders a page, takes a view, renders the same page at four
times the scale, and asserts the view's length has become 0. If wasm-bindgen or
the engine ever changes that, the test fails and this paragraph is what needs
correcting.

So: **draw from the view immediately and drop it.** If the pixels must outlive
the next engine call, use `data()`. The safe call has the short name; the
dangerous one has the warning in its name, its doc comment and here.

## Opening by ranges

A host that has the whole file hands it over. A host that has to fetch it —
over HTTP range requests, out of a `File` it does not want to read whole —
opens the document by feeding the ranges the engine asks for:

```js
const source = new PdfSource(file.size);
let doc = null;
while (doc === null) {
  try {
    doc = PdfDocument.openStreaming(source);
  } catch (e) {
    const needed = source.takeNeeded();      // [start, end, start, end, ...]
    for (let i = 0; i < needed.length; i += 2) {
      source.feed(needed[i], await fetchRange(needed[i], needed[i + 1]));
    }
  }
}
```

**The engine performs no transport.** It never fetches, never blocks and has
no runtime; it asks for a range and, if the host has not fed it, refuses with
that range named. The loop terminates because every refusal names a range, the
host feeds exactly that, and the engine's caches keep what they have already
parsed — so each turn strictly increases what is readable and no work is
repeated. There is no async here and there will not be: it would need a
runtime this workspace does not have, would colour every function down to the
device, and would make output depend on when bytes arrived
(`docs/rulings.md`, ruling 4).

The document that comes out is the document the whole-buffer open would have
produced. `node_smoke.mjs` drives the loop, caps the turns so a loop that could
not finish fails rather than hangs, and asserts the streamed render is
**byte-identical** to the buffered one.

A linearized file (ISO 32000-1 Annex F) pays for this the least: its first page
is at the front, and page one renders without a byte of the tail being fetched.

## The browser demo

A page that renders an uploaded PDF.

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
a full-page smear or one stray pixel.

Playwright is build tooling (rule 1's documented exception) and is installed on
demand; `demo/package.json` is `"private": true` and is never published.

## Size

`cmap-predefined` is on by default, and it carries all of Adobe's predefined
CMaps. `release.yml` gates the gzipped `.wasm` at 2.5 MiB. Turning the feature
off is `--no-default-features`, and it is the switch a host that renders no CJK
reaches for.

## Writing, and proving it is the same engine

```bash
cd <a directory where the tarball has been npm install'ed>
node <repo>/bindings/js/tests/write_parity.mjs <repo>/testdata/form-fields.pdf
```

Scripts with every input pinned — those that write a document (fill a
form and save incrementally; build a document from pages, a font and an image;
the document operations; a sanitise; two saves with every write option set;
form fields; the builder's graphics resources; tagged writing) and those that
write down what a sanitise reported, what the read surface and the signature
verdicts say, and what form data says, all listed in
[docs/features/bindings.md](../../docs/features/bindings.md) —
print one
`WROTE sha256=<hex>` or `READ sha256=<hex>` line each. The same scripts run against the facade in Rust,
through the wheel, the NuGet package and the Go, Ruby and Java bindings, and
`cargo xtask bindings-parity` requires every one to be byte-identical.

**There is no `editor.transaction(callback)`**, and the reason is mechanical
rather than a matter of taste. An exported wasm-bindgen method borrows its
`this` for the whole call, so JavaScript running inside one that touched the
same editor would hit *"recursive use of an object detected which would lead to
unsafe aliasing in Rust"* — a panic, from the one shape a caller would most
want. What the engine's design asks for is checkpoint, host-language control
flow, restore, and in JavaScript that is three lines you write:

```js
const mark = editor.checkpoint();
try { editor.fillField('name', 'Ada Lovelace'); }
catch (e) { editor.restore(mark); throw e; }
finally { mark.free(); }
```

There is no shipped helper around those three lines: it would be the first
logic this binding carries, and ruling 11's whole point is that there is none
to diverge with. `restore` is idempotent, so a `finally` that runs after its
own `catch` is safe.

`save` returns a **copy**. `viewUnsafeUntilNextAllocation` is the only aliasing
view on this surface, and a write API that handed one back would hand it back
at exactly the moment the caller is about to allocate again.

## The command stream

`PdfDocument.commandStream(index)` hands back a page's drawing as versioned
bytes ([`docs/design/command-stream.md`](../../docs/design/command-stream.md)),
and `PdfCommandStream.parse` reads them: `records()` is every record in order,
a `PdfCommandRecord` of a kind number and `PdfCommandField`s, each its tag
beside a value — a `BigInt`, a `number`, a `Uint8Array`, a `Float64Array`, a
`BigUint64Array` or an array of `Uint8Array`s — and `field(name)` reads one by
the name its kind gives the place (`PdfCommandKind.fields`).
`PdfCommandStream.fromRecords` writes records back, `toBytes` hands over the
bytes, and `render(scale)` replays the stream through the engine. A record
handed to `fromRecords` or to `new PdfCommandRecord` moves into the engine, as
wasm-bindgen moves every object a `Vec` takes, so make each list for the call.

`replay(device)` hands each record, in the stream's order, to the device's
method named for its kind (`glyphRun`, `fillPath`, `beginGroup`; a kind the
build does not know goes to `unknown`), and skips a kind the device has no
method for. A replay to a `CanvasRenderingContext2D` is a device over that
projection, written by the host and not shipped: here, fills, strokes and
clips. A command's state is a reference to the `State` records before it, in
their order; its path is in the page's space, points with `y` up, the
transformation already applied.

```js
import init, { PdfDocument, PdfCommandStream } from 'tinker-pdf-js';

await init();
const pdf = new PdfDocument(new Uint8Array(await (await fetch('page.pdf')).arrayBuffer()));
const stream = PdfCommandStream.parse(pdf.commandStream(0));
const canvas = document.querySelector('canvas');
const ctx = canvas.getContext('2d');
const [width, height] = stream.size();
const scale = canvas.width / width;
ctx.setTransform(scale, 0, 0, -scale, 0, height * scale);

const states = [];
const stateOf = (record) => states[Number(record.field('state').value)];
const colour = (rgb, alpha) => `rgb(${rgb[0]} ${rgb[1]} ${rgb[2]} / ${alpha})`;
const rule = (record) => (record.field('even-odd').value === 1n ? 'evenodd' : 'nonzero');
const path = (record) => {
  const operators = record.field('operators').value;
  const n = record.field('numbers').value;
  const out = new Path2D();
  let at = 0;
  for (const operator of operators) {
    if (operator === 0) { out.moveTo(n[at], n[at + 1]); at += 2; }
    else if (operator === 1) { out.lineTo(n[at], n[at + 1]); at += 2; }
    else if (operator === 2) { out.bezierCurveTo(n[at], n[at + 1], n[at + 2], n[at + 3], n[at + 4], n[at + 5]); at += 6; }
    else { out.closePath(); }
  }
  return out;
};

stream.replay({
  state(record) { states.push(record); },
  saveState() { ctx.save(); },
  restoreState() { ctx.restore(); },
  fillPath(record) {
    const state = stateOf(record);
    ctx.fillStyle = colour(state.field('fill colour').value, state.field('fill alpha').value);
    ctx.fill(path(record), rule(record));
  },
  strokePath(record) {
    const state = stateOf(record);
    const [a, b, c, d] = state.field('ctm').value;
    ctx.strokeStyle = colour(state.field('stroke colour').value, state.field('stroke alpha').value);
    ctx.lineWidth = state.field('line width').value * Math.sqrt(Math.abs(a * d - b * c));
    ctx.stroke(path(record));
  },
  clipPath(record) { ctx.clip(path(record), rule(record)); },
});
```

A glyph run's outlines are the stream's `outline` records, an image's samples
its `imageDecode` records, a shading's its `shadingDefinition`; a device that
draws them reads those as it reads the paths above.
[`tests/command_stream.mjs`](tests/command_stream.mjs) decodes a page holding
every kind of record and writes it back to the facade's bytes, and drives a
device that hears every kind in the stream's order:

```bash
cd <a directory where the tarball has been npm install'ed>
node <repo>/bindings/js/tests/command_stream.mjs
```

## Nothing is published

`npm install tinker-pdf-js` does not work: nothing is published.
[`release.yml`](../../.github/workflows/release.yml) publishes the package to
npm when the owner merges `develop` into `main` at a workspace version that has
no `v<version>` tag, and a merge that leaves the version as it is publishes
nothing; the facade does not freeze before 0.1.0
([`docs/architecture.md`](../../docs/architecture.md)).
