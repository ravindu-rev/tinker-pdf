// The write-parity scripts, run through an *installed* npm package.
//
//   cd <a directory where `npm install <tarball>` has run>
//   node <this file> <path to testdata/form-fields.pdf>
//
// The scripts here are the same ones that
// `crates/tinker-pdf/examples/write_parity.rs` runs against the facade,
// `bindings/python/tests/write_parity.py` runs through the wheel and
// `bindings/dotnet/tests/Smoke` and the Go, Java and Ruby parity programs run
// through the C ABI. `cargo xtask bindings-parity` requires every surface to
// print the same SHA-256s. Ruling 11 is what makes that the right test: a
// binding projects the facade 1:1 and adds no logic of its own, so surfaces
// disagreeing means one of them added something. The third script,
// read-surface, writes down everything the read surface says about two
// documents in the text the facade example's module documentation specifies
// byte for byte, and prints the hash of that.
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
const { PdfDocument, PdfBuilder, PdfWriteOptions, PdfOutlineEntry, PdfView, PdfTrustAnchors } =
  module_;

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

// Read-surface's second document: links, an outline and /Info, built.
function linkedDocument() {
  const builder = new PdfBuilder();
  builder.addBaseFont(name('F1'), name('Helvetica'));
  const one = builder.beginPage(200.0, 200.0);
  one.text(name('F1'), 12.0, 20.0, 170.0, 'Links');
  one.linkToUri(10.0, 10.0, 60.0, 30.0, 'https://example.org/parity');
  const xyz = PdfView.xyz(10.0, undefined, 1.5);
  one.linkToPageView(70.0, 10.0, 120.5, 30.25, 1, xyz);
  xyz.free();
  builder.pushPage(one);
  one.free();
  const two = builder.beginPage(200.0, 200.0);
  builder.pushPage(two);
  two.free();
  builder.setInfo(name('Title'), 'Read surface \u2014 parity');
  builder.setInfo(name('Author'), '');
  const heading = new PdfOutlineEntry('Part one');
  heading.setOpen(true);
  const chapter = new PdfOutlineEntry('Chapter one');
  const fitH = PdfView.fitH(150.0);
  chapter.setPageTargetView(1, fitH);
  fitH.free();
  heading.addChild(chapter);
  chapter.free();
  const elsewhere = new PdfOutlineEntry('Elsewhere');
  elsewhere.setUriTarget('https://example.org/');
  builder.setOutline([heading, elsewhere]);
  const bytes = builder.finish();
  builder.free();
  return bytes;
}

const hex = (bytes) => Buffer.from(bytes).toString('hex');
const text = (value) => (value === undefined ? '-' : `s:${hex(encoder.encode(value))}`);
const byteString = (value) => (value === undefined ? '-' : `b:${hex(value)}`);
const number = (value) => {
  if (value === undefined) return '-';
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, value);
  return `f:${hex(new Uint8Array(view.buffer))}`;
};
const reference = (value) => (value === undefined ? '-' : `${value[0]}.${value[1]}`);
const digest = (value) => (value === undefined ? '-' : sha256(value));

function viewText(view) {
  switch (view.kind) {
    case 'xyz':
      return `xyz ${number(view.left)} ${number(view.top)} ${number(view.zoom)}`;
    case 'fith':
    case 'fitbh':
      return `${view.kind} ${number(view.top)}`;
    case 'fitv':
    case 'fitbv':
      return `${view.kind} ${number(view.left)}`;
    case 'fitr':
      return `fitr ${number(view.left)} ${number(view.bottom)} ${number(view.right)} ${number(view.top)}`;
    default:
      return view.kind;
  }
}

function destinationText(dest) {
  if (dest === undefined) return '-';
  if (dest.kind === 'explicit') {
    const page = dest.pageIndex === undefined ? '-' : String(dest.pageIndex);
    return `explicit ${page} ${reference(dest.pageRef)} ${viewText(dest.view)}`;
  }
  if (dest.kind === 'named') return `named ${byteString(dest.name)}`;
  return `uri ${byteString(dest.uri)}`;
}

function actionText(action) {
  if (action === undefined) return '-';
  switch (action.kind) {
    case 'goto':
      return `goto ${destinationText(action.destination)}`;
    case 'gotor':
      return `gotor ${byteString(action.file)} ${destinationText(action.destination)}`;
    case 'uri':
      return `uri ${byteString(action.uri)}`;
    case 'named':
      return `named ${byteString(action.name)}`;
    case 'launch':
      return `launch ${byteString(action.file)}`;
    default:
      return `other ${byteString(action.subtype)}`;
  }
}

function* flatten(items, depth = 0) {
  for (const item of items) {
    yield [depth, item];
    yield* flatten(item.children, depth + 1);
  }
}

// Everything the read surface says about one document, in the contract's order.
function readDump(label, document_, out) {
  out.push(`document ${label}`);
  out.push(`version ${text(document_.pdfVersion)}`);
  out.push(`pages ${document_.pageCount}`);
  const metadata = document_.metadata;
  for (const [key, value] of [
    ['title', metadata.title],
    ['author', metadata.author],
    ['subject', metadata.subject],
    ['keywords', metadata.keywords],
    ['creator', metadata.creator],
    ['producer', metadata.producer],
    ['creation-date', metadata.creationDate],
    ['modification-date', metadata.modificationDate],
  ]) {
    out.push(`info ${key} ${text(value)}`);
  }
  out.push(`trapped ${metadata.trapped ?? 'absent'}`);
  document_.pageLabels().forEach((label_, index) => out.push(`label ${index} ${text(label_)}`));
  for (const [depth, item] of flatten(document_.outline())) {
    out.push(`outline ${depth} ${item.open ? 1 : 0} ${text(item.title)} ${destinationText(item.destination)}`);
  }
  for (let index = 0; index < document_.pageCount; index += 1) {
    for (const link of document_.links(index)) {
      const [x0, y0, x1, y1] = link.rect;
      out.push(
        `link ${index} ${number(x0)} ${number(y0)} ${number(x1)} ${number(y1)} ` +
          `${reference(link.reference)} ${actionText(link.action)}`,
      );
    }
  }
  for (const attachment of document_.attachments()) {
    let data;
    try {
      data = attachment.data();
    } catch {
      data = undefined;
    }
    const size = attachment.size === undefined ? '-' : String(attachment.size);
    out.push(
      `attachment ${text(attachment.name)} ${text(attachment.filename)} ` +
        `${text(attachment.description)} ${size} ${digest(data)}`,
    );
  }
  out.push(`xmp ${digest(document_.xmpMetadata())}`);
  for (const warning of document_.warnings()) {
    out.push(`warning ${warning.offset} ${reference(warning.object)} ${warning.kind} ${text(warning.message)}`);
  }
}

// Script three: everything the read surface says about two documents.
function readSurface(outlineFixture) {
  const lines = [];
  const shifted = new Uint8Array(outlineFixture.length + 5);
  shifted.set(encoder.encode('JUNK\n'), 0);
  shifted.set(outlineFixture, 5);
  const first = new PdfDocument(shifted);
  readDump('shifted', first, lines);
  first.free();
  const second = new PdfDocument(linkedDocument());
  readDump('linked', second, lines);
  second.free();
  return lines.map((line) => `${line}\n`).join('');
}

const SIGNED = [
  ['ecdsa-p256', 'ecdsa-p256-root'],
  ['pkcs7-sha1', 'pkcs7-sha1-root'],
  ['document-timestamp', null],
];

// Script four: every signature and both verdicts, in the contract's text.
function signaturesDump(support) {
  const lines = [];
  for (const [label, root] of SIGNED) {
    const document_ = new PdfDocument(readFileSync(path.join(support, `${label}.pdf`)));
    const anchors = new PdfTrustAnchors();
    if (root !== null) anchors.add(readFileSync(path.join(support, `${root}.der`)));
    lines.push(`document ${label}`);
    document_.signatures().forEach((signature, index) => {
      const flat = signature.spans;
      const spans = [];
      for (let i = 0; i < flat.length; i += 2) spans.push(`${flat[i]}:${flat[i + 1] - flat[i]}`);
      lines.push(
        `signature ${index} ${text(signature.field)} ${text(signature.subFilterName)} ` +
          `${text(signature.reason)} ${text(signature.location)} ${text(signature.name)} ` +
          `${signature.coverage} ${signature.coversWholeFile ? 1 : 0} ` +
          `${signature.isUsageRights ? 1 : 0} ${signature.certificationLevel ?? 0} ` +
          `${spans.length === 0 ? '-' : spans.join(',')}`,
      );
    });
    for (const at of [undefined, 0]) {
      document_.verifySignatures(anchors, at).forEach((verdict, index) => {
        const validity = verdict.signerValidity;
        const weaknesses = verdict.weaknesses;
        lines.push(
          `verdict ${at === undefined ? '-' : at} ${index} ${verdict.cms} ` +
            `${verdict.documentDigest} ${verdict.signature} ${verdict.chain} ` +
            `${text(verdict.signerSubject)} ${text(verdict.signerIssuer)} ` +
            `${validity === undefined ? '- -' : `${validity[0]} ${validity[1]}`} ` +
            `${weaknesses.length === 0 ? '-' : weaknesses.join(',')}`,
        );
      });
    }
    anchors.free();
    document_.free();
  }
  return lines.map((line) => `${line}\n`).join('');
}

// What the C ABI cannot carry and a JavaScript object can: each arm's payload.
// Not part of the compared text, because only the facade-direct surfaces have
// it; asserted here against what the fixture is known to be.
function signaturePayloadsCross(support) {
  const check = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  const document_ = new PdfDocument(readFileSync(path.join(support, 'ecdsa-p256.pdf')));
  const anchors = new PdfTrustAnchors();
  anchors.add(readFileSync(path.join(support, 'ecdsa-p256-root.der')));
  let refused = false;
  try {
    anchors.add(encoder.encode('not a certificate'));
  } catch {
    refused = true;
  }
  check(refused, 'bytes that are not a certificate must be refused');
  check(anchors.length === 1, 'a refused anchor is not kept');

  const [signature] = document_.signatures();
  check(signature.anchor === 'field' && signature.anchorName === undefined, 'anchor');
  check(signature.subFilter === 'adbe.pkcs7.detached', `subFilter ${signature.subFilter}`);
  check(signature.contents[0] === 0x30, 'the stored /Contents is DER');

  const [verdict] = document_.verifySignatures(anchors);
  check(verdict.cmsSigners === 1, `cmsSigners ${verdict.cmsSigners}`);
  check(
    verdict.chainSubject.endsWith('CN=Tinker PDF ECDSA P256 Test Root'),
    `chainSubject ${verdict.chainSubject}`,
  );
  check(verdict.isTrusted(), 'the anchored ECDSA signature is trusted');

  const [judged] = document_.verifySignatures(anchors, 0);
  const details = judged.weaknessDetails.filter(
    (_, i) => judged.weaknesses[i] === 'outside-validity',
  );
  check(details.length > 0 && details.every((d) => d.length > 0), 'outside-validity names a subject');

  const none = new PdfTrustAnchors();
  const [untrusted] = document_.verifySignatures(none);
  check(untrusted.chain === 'no-anchors' && untrusted.chainSubject === undefined, 'no anchors');
  check(!untrusted.isTrusted(), 'nothing trusted, nothing trusted');
  none.free();
  anchors.free();
  document_.free();
  console.log('JS-PARITY: signature payloads cross');
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

const outlineFixture = readFileSync(path.join(path.dirname(fixturePath), 'outline-3level.pdf'));
const dumped = encoder.encode(readSurface(outlineFixture));
if (process.env.TINKER_PARITY_DUMP) process.stdout.write(Buffer.from(dumped));
console.log(`READ sha256=${sha256(dumped)} surface=js script=read-surface bytes=${dumped.length}`);

const support = path.join(path.dirname(path.resolve(fixturePath)), '..', 'crates/tinker-pdf/tests/signature_support');
const signed = encoder.encode(signaturesDump(support));
if (process.env.TINKER_PARITY_DUMP) process.stdout.write(Buffer.from(signed));
console.log(`READ sha256=${sha256(signed)} surface=js script=signatures bytes=${signed.length}`);
signaturePayloadsCross(support);
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
