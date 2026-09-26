// The write-parity scripts, run through an *installed* npm package.
//
//   cd <a directory where `npm install <tarball>` has run>
//   node <this file> <path to testdata/form-fields.pdf>
//
// The two scripts here are the same two that
// `crates/tinker-pdf/examples/write_parity.rs` runs against the facade,
// `bindings/python/tests/write_parity.py` runs through the wheel and
// `bindings/dotnet/tests/Smoke` runs through the C ABI. `cargo xtask
// bindings-parity` requires all four to print the same SHA-256s. Ruling 11 is
// what makes that the right test: a binding projects the facade 1:1 and adds
// no logic of its own, so four surfaces disagreeing means one of them added
// something.
//
// **There is no `editor.transaction(callback)` in this binding**, and the
// reason is mechanical rather than a matter of taste: an exported wasm-bindgen
// method borrows its `this` for the whole call, so JavaScript running inside
// one that touched the same editor would hit "recursive use of an object
// detected which would lead to unsafe aliasing in Rust". What the design asks
// for -- checkpoint, host-language control flow, restore -- is in JavaScript
// three lines the caller writes, and `transaction` below is those three lines,
// written here in the test rather than shipped in the package. Shipping them
// would be the first logic any binding here carries.
//
// Every artefact goes through the engine's own strict structural validator
// before its hash is printed, because four byte-identical outputs agreeing
// tells you nothing if all four are wrong.

import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import process from 'node:process';

const [fixturePath] = process.argv.slice(2);
if (!fixturePath) {
  console.error('usage: node write_parity.mjs <form-fields.pdf>');
  process.exit(2);
}

// Resolved by package name from the *current directory*, so this tests the
// install rather than the build directory — the same anchoring node_smoke.mjs
// uses and for the same reason.
const require = createRequire(pathToFileURL(path.join(process.cwd(), 'package.json')));
const entry = pathToFileURL(require.resolve('tinker-pdf-js')).href;
const module_ = await import(entry);
const init = module_.default;
const { PdfDocument, PdfBuilder, PdfWriteOptions, PdfOutlineEntry } = module_;

const wasmUrl = new URL('tinker_pdf_js_bg.wasm', entry);
await init({ module_or_path: readFileSync(fileURLToPath(wasmUrl)) });

// The eight-by-eight grey image every surface builds, from the same formula.
// A formula rather than a fixture file: a parity suite whose four surfaces
// read the same image *file* proves they can read a file.
const PARITY_IMAGE = new Uint8Array(64);
for (let i = 0; i < 64; i += 1) PARITY_IMAGE[i] = (i * 7) % 256;

const encoder = new TextEncoder();
const name = (text) => encoder.encode(text);
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/// Checkpoint, host-language control flow, restore. Three lines, in the host.
function transaction(editor, body) {
  const mark = editor.checkpoint();
  try {
    return body();
  } catch (error) {
    editor.restore(mark);
    throw error;
  } finally {
    mark.free();
  }
}

function fillAndSave(fixture) {
  const document_ = new PdfDocument(fixture);
  const editor = document_.editor();

  const skipped = editor.fillField('name', 'Ada Lovelace');
  if (skipped.length !== 1) {
    throw new Error(`the /Rect-less widget must be reported, got ${skipped.length}`);
  }
  if (skipped[0].toString() !== '7 0 R: no usable /Rect (12.5.2)') {
    throw new Error(`unexpected report: ${skipped[0].toString()}`);
  }
  if (skipped[0].objectNumber !== 7 || skipped[0].reason !== 'rect-missing') {
    throw new Error('the report lost the widget it names');
  }

  const clean = editor.fillField('notes', 'every surface writes this');
  if (clean.length !== 0) {
    throw new Error('the control field is well formed, so nothing is skipped');
  }

  editor.setCheckbox('agree', true);
  editor.selectRadio('colour', 'red');

  const options = new PdfWriteOptions();
  options.setMode('incremental');
  const bytes = editor.save(options);
  options.free();
  editor.free();
  document_.free();
  return bytes;
}

function buildADocument() {
  const builder = new PdfBuilder();
  builder.addBaseFont(name('F1'), name('Helvetica'));
  builder.addImage(name('Im1'), PARITY_IMAGE, 'gray8', 8, 8);

  const one = builder.beginPage(200.0, 200.0);
  one.text(name('F1'), 14.0, 20.0, 170.0, 'Page one');
  one.fillRect(20.0, 40.0, 60.0, 60.0, 0.25);
  one.image(name('Im1'), 100.0, 40.0, 60.0, 60.0);
  builder.pushPage(one);
  one.free();

  const two = builder.beginPage(200.0, 200.0);
  two.text(name('F1'), 14.0, 20.0, 170.0, 'Page two');
  builder.pushPage(two);
  two.free();

  builder.setInfo(name('Title'), 'tinker-pdf write parity');

  const first = new PdfOutlineEntry('Page one');
  first.setPageTarget(0);
  const second = new PdfOutlineEntry('Page two');
  second.setPageTarget(1);
  builder.setOutline([first, second]);

  const bytes = builder.finish();
  builder.free();
  return bytes;
}

// The throwing-callback leg: the editor must be exactly as it was, asserted by
// re-saving and hashing — the only way that cannot be faked — and the error
// must still escape, because a rollback that also hid the reason would be the
// worst of both.
function transactionRollsBackOnAThrow(fixture) {
  const document_ = new PdfDocument(fixture);
  const editor = document_.editor();
  const options = new PdfWriteOptions();
  options.setMode('incremental');
  const save = () => sha256(editor.save(options));

  editor.setCheckbox('agree', true);
  const before = save();

  let threw = false;
  try {
    transaction(editor, () => {
      editor.selectRadio('colour', 'blue');
      editor.fillField('notes', 'this must not survive');
      if (save() === before) throw new Error('the body did not change anything');
      throw new Error('deliberate');
    });
  } catch (error) {
    if (error.message !== 'deliberate') throw error;
    threw = true;
  }
  if (!threw) throw new Error('the throw was swallowed, which a rollback must never do');

  const after = save();
  if (after !== before) throw new Error(`the editor was not restored: ${before} -> ${after}`);

  // And the committing leg: a body that returns normally keeps everything.
  transaction(editor, () => editor.selectRadio('colour', 'red'));
  if (save() === before) throw new Error('a body that does not throw commits');

  options.free();
  editor.free();
  document_.free();
  console.log('JS-PARITY: transaction rolls back on a throw and commits without one');
}

function report(script, bytes) {
  const document_ = new PdfDocument(bytes);
  const defects = document_.validate();
  document_.free();
  if (defects.length !== 0) {
    throw new Error(`${script}: the artefact does not pass the strict validator: ${defects}`);
  }
  console.log(
    `WROTE sha256=${sha256(bytes)} surface=js script=${script} bytes=${bytes.length}`,
  );
}

const fixture = readFileSync(fixturePath);
report('fill-and-save', fillAndSave(fixture));
report('build-a-document', buildADocument());
transactionRollsBackOnAThrow(fixture);

// A consumed handle refuses rather than producing a second document, which is
// the JavaScript spelling of the C ABI's SpentHandle.
const spent = new PdfBuilder();
spent.addBaseFont(name('F1'), name('Helvetica'));
spent.finish();
let refused = false;
try {
  spent.finish();
} catch (error) {
  refused = String(error).includes('already finished');
}
spent.free();
if (!refused) throw new Error('a second finish must be refused');

console.log('JS-PARITY: RAN');
