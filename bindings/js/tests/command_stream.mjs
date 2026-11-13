// The command stream through the JavaScript binding (RC-18.6).
//
//   cd <a directory where `npm install <tarball>` has run>
//   node <this file>
//
// The package is resolved by name from the current directory, as
// node_smoke.mjs resolves it, so this is a test of the install; the face and
// the profile the page embeds are read from the checkout this file is in.
//
// The page holds every kind of record a page's stream is written with -- the
// C ABI's test page (`crates/tinker-pdf-ffi/src/commands/tests.rs`,
// `every_kind_page`), built again here -- and a stream pushed by hand holds the
// three a page reaches only past a budget or through a recording that
// composites colorants. Between the two, every kind but the end.
//
// What is held:
//
// - the stream decoded into `PdfCommandRecord`s, each field a
//   `PdfCommandField` of its tag, and written back through the binding's own
//   types -- each field rebuilt from its tag and its JavaScript value, each
//   record from its kind and fields -- is the facade's bytes, byte for byte;
// - a recording device receives every record, every kind among them, in the
//   order the facade's decoder gives them, and a device that names only some
//   methods hears only those kinds;
// - the replay through the engine draws what a render of the page draws;
// - bytes that are no stream are refused with the reader's sentence.
//
// The script prints `COMMAND-STREAM: RAN` on success.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import process from 'node:process';

const require = createRequire(pathToFileURL(path.join(process.cwd(), 'package.json')));
const entry = pathToFileURL(require.resolve('tinker-pdf-js')).href;
const module_ = await import(entry);
const init = module_.default;
const {
  PdfDocument,
  PdfCommandStream,
  PdfCommandRecord,
  PdfCommandField,
  PdfCommandKind,
  PdfCommandBudgets,
} = module_;
const wasmUrl = new URL('tinker_pdf_js_bg.wasm', entry);
await init({ module_or_path: readFileSync(fileURLToPath(wasmUrl)) });

const root = new URL('../../../', import.meta.url);
const serif = readFileSync(
  new URL('crates/tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf', root),
);
const rgbProfile = readFileSync(new URL('fuzz/corpus/icc_profile/rgb-gamma-curve.icc', root));

// The tags in their numbers' order on the wire (`docs/design/command-stream.md`,
// "The form").
const TAGS = ['nothing', 'unsigned', 'signed', 'real', 'bytes', 'reals', 'naturals', 'strings'];

const check = (condition, what) => {
  if (!condition) throw new Error(`FAILED: ${what}`);
};

const ascii = (text) => new TextEncoder().encode(text);

const concat = (parts) => {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
};

const sameBytes = (a, b) => a.length === b.length && a.every((byte, at) => byte === b[at]);

/** A stream object of `dictionary` holding `data`. */
const streamObject = (dictionary, data) =>
  concat([ascii(`<< ${dictionary} /Length ${data.length} >>\nstream\n`), data, ascii('\nendstream')]);

/** A file of `objects`, numbered from 1, the first the catalog, with a classic cross-reference table. */
const pdf = (objects) => {
  const parts = [ascii('%PDF-1.7\n')];
  let length = parts[0].length;
  const offsets = [];
  objects.forEach((body, at) => {
    offsets.push(length);
    const object = concat([ascii(`${at + 1} 0 obj\n`), body, ascii('\nendobj\n')]);
    parts.push(object);
    length += object.length;
  });
  const lines = [`xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`];
  for (const offset of offsets) lines.push(`${String(offset).padStart(10, '0')} 00000 n \n`);
  lines.push(`trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${length}\n%%EOF\n`);
  parts.push(ascii(lines.join('')));
  return concat(parts);
};

/** The 200 x 160 page whose stream holds every kind a page's stream is written with. */
const everyKindPage = () => {
  const widths = Array(95).fill('500').join(' ');
  const content = concat([
    ascii(
      'q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n' +
        'q 10 40 40 30 re W n /Sh0 sh Q\n' +
        'q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n' +
        'q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n' +
        'BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n' +
        'BT /Nope 12 Tf 10 140 Td (x) Tj ET\n' +
        'q 20 0 0 20 120 20 cm /Im0 Do Q\n' +
        'q 10 0 0 10 150 20 cm BI /W 2 /H 2 /CS /G /BPC 8 ID ',
    ),
    new Uint8Array([0x00, 0x55, 0xaa, 0xff]),
    ascii(
      ' EI Q\n' +
        '/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n' +
        'q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n' +
        'q /Pattern cs /P0 scn 150 60 30 30 re f Q\n' +
        'q 1 0 0 1 150 100 cm /Fm0 Do Q\n' +
        '/Rec Do\n',
    ),
  ]);
  const rgb = new Uint8Array([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]);
  return pdf([
    ascii('<< /Type /Catalog /Pages 2 0 R >>'),
    ascii('<< /Type /Pages /Kids [3 0 R] /Count 1 >>'),
    ascii(
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R ' +
        '/Contents 5 0 R /Annots [17 0 R] >>',
    ),
    ascii(
      '<< /Font << /F0 6 0 R >> ' +
        '/XObject << /Im0 9 0 R /G 10 0 R /Fm0 11 0 R /Rec 12 0 R >> ' +
        '/Shading << /Sh0 13 0 R >> /Pattern << /P0 15 0 R >> ' +
        '/ColorSpace << ' +
        '/CS0 [/Separation /Spot /DeviceRGB ' +
        '<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] ' +
        '/Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] /Range [100 -100 -100 100] >>] >> ' +
        '/ExtGState << ' +
        '/Fancy << /RI /Saturation ' +
        '/BG << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> ' +
        '/TR << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >> ' +
        '/HT << /Type /Halftone /HalftoneType 1 /Frequency 60 /Angle 45 ' +
        '/SpotFunction /Round >> >> ' +
        '/Masked << /SMask << /Type /Mask /S /Luminosity /G 16 0 R >> >> ' +
        '/NoMask << /SMask /None >> >> >>',
    ),
    streamObject('', content),
    ascii(
      '<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 ' +
        `/LastChar 126 /Widths [${widths}] /FontDescriptor 7 0 R ` +
        '/Encoding /WinAnsiEncoding >>',
    ),
    ascii(
      '<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 ' +
        '/FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 ' +
        '/CapHeight 700 /StemV 80 /FontFile2 8 0 R >>',
    ),
    streamObject(`/Length1 ${serif.length}`, serif),
    streamObject(
      '/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB ' +
        '/BitsPerComponent 8',
      rgb,
    ),
    streamObject(
      '/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>',
      ascii('0 0.8 0 rg 0 0 20 20 re f'),
    ),
    streamObject(
      '/Type /XObject /Subtype /Form /BBox [0 0 40 40] ' +
        '/Resources << /XObject << /ImF 9 0 R >> >>',
      ascii('q 10 0 0 10 0 0 cm /ImF Do Q'),
    ),
    streamObject(
      '/Type /XObject /Subtype /Form /BBox [0 0 10 10] ' +
        '/Resources << /XObject << /Rec 12 0 R >> >>',
      ascii('0 0 1 rg 0 0 2 2 re f /Rec Do'),
    ),
    ascii(
      '<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] ' +
        '/Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> ' +
        '/Extend [true true] >>',
    ),
    streamObject('/N 3', rgbProfile),
    streamObject(
      '/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 ' +
        '/YStep 10 /Resources << >>',
      ascii('1 0 0 rg 0 0 5 5 re f'),
    ),
    streamObject(
      '/Type /XObject /Subtype /Form /BBox [0 0 200 160] ' +
        '/Group << /S /Transparency /CS /DeviceGray >>',
      ascii('0.5 g 0 0 200 160 re f'),
    ),
    ascii('<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>'),
    streamObject('/Type /XObject /Subtype /Form /BBox [0 0 40 30]', ascii('0 0 1 RG 2 w 1 1 38 28 re S')),
  ]);
};

/** The field, made again from its tag and its JavaScript value through the binding's constructors. */
const rebuilt = (field) => {
  switch (field.tagName) {
    case 'nothing':
      return PdfCommandField.nothing();
    case 'unsigned':
      return PdfCommandField.unsigned(field.value);
    case 'signed':
      return PdfCommandField.signed(field.value);
    case 'real':
      return PdfCommandField.real(field.value);
    case 'bytes':
      return PdfCommandField.bytes(field.value);
    case 'reals':
      return PdfCommandField.reals(field.value);
    case 'naturals':
      return PdfCommandField.naturals(field.value);
    case 'strings':
      return PdfCommandField.strings(field.value);
    default:
      throw new Error(`FAILED: a field of tag ${field.tagName}`);
  }
};

/** `records`, each made again from its kind and its fields' values. */
const throughJs = (records) =>
  records.map((record) => new PdfCommandRecord(record.kind, record.fields.map(rebuilt)));

/** A device with a method for every kind this build knows and for an unknown one, each noting the kind's number it was handed. */
const recorder = () => {
  const device = { seen: [] };
  for (const kind of PdfCommandKind.all()) {
    const number = kind.number;
    device[kind.method] = (record) => {
      check(record.kind === number, `the method for kind ${number} was handed a ${record.kind}`);
      device.seen.push(record.kind);
    };
  }
  device.unknown = (record) => device.seen.push(`unknown ${record.kind}`);
  return device;
};

const page = everyKindPage();
const document = new PdfDocument(page);
const written = document.commandStream(0);
const stream = PdfCommandStream.parse(written);
check(sameBytes(stream.toBytes(), written), 'the stream writes back its own bytes');
check(stream.pageIndex === 0, 'the stream is page 0s');
const [width, height] = stream.size();
check(
  width === document.pageWidth(0) && height === document.pageHeight(0),
  "the stream's page is the page's size",
);
check(stream.usage === undefined, 'the stream is of no usage');
check(stream.version.length === 2, 'a major and a minor version');

const records = stream.records();
check(records.length === stream.length, 'every record, counted');
check(records[0].name === 'Page', `the page record leads: ${records[0].name}`);
for (const record of records) {
  const kind = record.commandKind;
  check(kind !== undefined, `kind ${record.kind} is one this build knows`);
  check(record.name === kind.name, `${record.name} names its kind`);
  const names = record.fieldNames;
  const fields = record.fields;
  check(fields.length <= names.length, `${record.name} holds no more fields than its kind names`);
  fields.forEach((field, at) => {
    check(field.equals(rebuilt(field)), `${record.name}'s field ${at} rebuilds`);
    check(field.tag === TAGS.indexOf(field.tagName), `${record.name}'s field ${at}'s tag`);
    if (at < names.length) {
      const named = record.field(names[at]);
      check(named !== undefined && named.equals(field), `${record.name}'s field ${at} by its name`);
    }
  });
}
check(
  sameBytes(PdfCommandStream.fromRecords(throughJs(records)).toBytes(), written),
  "the stream decoded and written back through the binding's types is the facade's bytes",
);

const onThePage = new Set(records.map((record) => record.kind));
const byHand = new Set(['Colorants', 'GroupBudgetSpent', 'NestedBudgetSpent']);
const kinds = new Map(PdfCommandKind.all().map((kind) => [kind.name, kind]));
for (const [name, kind] of kinds) {
  if (name !== 'End' && !byHand.has(name)) {
    check(onThePage.has(kind.number), `the page's stream holds a ${name} record`);
  }
}

// A record handed to `fromRecords` or a `PdfCommandRecord` is moved into the
// engine, as wasm-bindgen moves every exported object a `Vec` takes, so each
// list is made afresh for the call it is handed to.
const F = PdfCommandField;
const handRecords = () => [
  throughJs([records[0]])[0],
  new PdfCommandRecord(kinds.get('Colorants').number, [
    F.unsigned(1n),
    F.strings([ascii('Spot'), ascii('Other')]),
    F.bytes(new Uint8Array([128, 64])),
    F.naturals(new BigUint64Array([1n])),
    F.bytes(new Uint8Array([0, 0, 0, 255])),
    F.nothing(),
    F.bytes(new Uint8Array([10, 20, 30, 40])),
  ]),
  new PdfCommandRecord(kinds.get('GroupBudgetSpent').number, []),
  new PdfCommandRecord(kinds.get('NestedBudgetSpent').number, []),
];
const handBytes = PdfCommandStream.fromRecords(handRecords()).toBytes();
const reread = PdfCommandStream.parse(handBytes);
const readBack = reread.records();
const hand = handRecords();
check(readBack.length === hand.length, 'the records pushed by hand, every one');
readBack.forEach((record, at) => check(record.equals(hand[at]), `pushed record ${at} reads back`));
check(
  sameBytes(PdfCommandStream.fromRecords(throughJs(readBack)).toBytes(), handBytes),
  "and writes back through the binding's types",
);

const heard = new Set();
for (const each of [stream, reread]) {
  const device = recorder();
  each.replay(device);
  const order = each.records().map((record) => record.kind);
  check(
    device.seen.length === order.length && device.seen.every((kind, at) => kind === order[at]),
    "the device receives every record in the decoder's order",
  );
  for (const kind of device.seen) heard.add(kind);
}
for (const [name, kind] of kinds) {
  if (name !== 'End') check(heard.has(kind.number), `the device received a ${name} record`);
}

let runs = 0;
stream.replay({
  glyphRun(record) {
    check(record.name === 'GlyphRun', 'glyphRun is handed glyph runs');
    check(record.field('font id').tagName === 'unsigned', 'a run names its font');
    runs += 1;
  },
});
check(
  runs === records.filter((record) => record.name === 'GlyphRun').length,
  'a device naming one method hears only that kind',
);

let thrown = false;
try {
  stream.replay({
    beginText() {
      throw new Error('the device refuses');
    },
  });
} catch (error) {
  thrown = String(error).includes('refuses');
}
check(thrown, "the device's exception ends the replay and is thrown");

const drawn = stream.render(1.0);
const direct = document.renderPage(0, 1.0);
check(
  drawn.width === direct.width &&
    drawn.height === direct.height &&
    sameBytes(drawn.data(), direct.data()),
  'the replay draws what a render of the page draws',
);
const [pixelWidth, pixelHeight] = stream.pixelSize(1.0);
check(pixelWidth === drawn.width && pixelHeight === drawn.height, "the replay's size");

const budgets = new PdfCommandBudgets();
budgets.records = 4n;
let named = '';
try {
  PdfCommandStream.parseWith(written, budgets);
} catch (error) {
  named = String(error);
}
check(named.includes('records'), `a stream past a budget names it: ${named}`);
check(
  sameBytes(PdfCommandStream.parseWith(written, new PdfCommandBudgets()).toBytes(), written),
  'the default budgets read the stream',
);
for (const refused of [new Uint8Array(0), ascii('not a command stream'), written.slice(0, -1)]) {
  let read = true;
  try {
    PdfCommandStream.parse(refused);
  } catch {
    read = false;
  }
  check(!read, 'bytes that are no stream are refused');
}
let sentence = '';
try {
  PdfCommandStream.parse(ascii('plainly not one'));
} catch (error) {
  sentence = String(error);
}
check(sentence.includes('not a command stream'), `the reader's sentence: ${sentence}`);
let badString = false;
try {
  PdfCommandField.strings([ascii('fine'), 'not bytes']);
} catch {
  badString = true;
}
check(badString, 'a strings field of something that is no Uint8Array is refused');

const glyphRun = PdfCommandKind.fromNumber(17);
check(glyphRun !== undefined && glyphRun.name === 'GlyphRun', 'kind 17 is a glyph run');
check(glyphRun.method === 'glyphRun' && glyphRun.isCommand, 'and a command');
check(PdfCommandKind.fromNumber(63) === undefined, 'kind 63 is none this build knows');
check(!kinds.get('Page').isCodec && kinds.get('State').isCodec, "the codec's kinds");

console.log(`records=${records.length} hand=${hand.length} kinds=${kinds.size}`);
console.log('COMMAND-STREAM: RAN');
