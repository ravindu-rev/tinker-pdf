# Python binding

PyO3 directly over the `tinker-pdf` facade — not through the C ABI, which
would only add a second error translation. Scope, design and packaging:
[`docs/features/bindings.md`](../../docs/features/bindings.md).

```python
import tinker_pdf

doc = tinker_pdf.Document(open("file.pdf", "rb").read())
print(doc.page_count, doc.page_text(0))

# The engine bundles no font faces and reads no font directories, so a
# document that embeds none extracts its text perfectly and draws none of it.
# This is the call that fixes that, and it is the same seam in all four
# languages.
doc.set_fonts(open("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "rb").read())

bitmap = doc.render(0, dpi=150.0)
memoryview(bitmap.data)  # zero-copy into numpy or Pillow
```

`render` and `page_text` release the GIL, so a thread pool over pages is
actually parallel.

## Building a wheel

```bash
maturin build --manifest-path bindings/python/Cargo.toml --release --out dist
```

**One wheel per platform, not one per interpreter.** `Cargo.toml` asks pyo3
for `abi3-py39`, so maturin emits a single `cp39-abi3` wheel that pip installs
on every CPython from 3.9 up. The name says so:
`tinker_pdf-0.0.1-cp39-abi3-win_amd64.whl`. That is worth checking rather than
assuming — dropping the `abi3-py39` feature still builds, still installs, and
silently needs one wheel per interpreter version — so
[`.github/workflows/release.yml`](../../.github/workflows/release.yml) asserts
the `abi3` tag is in the filename and refuses a build that emits more than one
wheel.

## Proving an installed wheel works

```bash
python bindings/python/tests/wheel_smoke.py testdata/simple-text.pdf auto
```

Run it against a `pip install`ed wheel, never against the source tree. It
asserts the render **twice** — blank without a face, inked with one — because
`testdata/simple-text.pdf` embeds no font program and this engine bundles no
faces, so "a bitmap of the right size came back" passes on a build whose
renderer does nothing at all. That is PRE-A's failure, one ecosystem out.

## Writing, and proving it is the same engine

```bash
python bindings/python/tests/write_parity.py testdata/form-fields.pdf
```

Two scripts with every input pinned — fill a form and save incrementally, and
build a document from pages, a font and an image — printing one
`WROTE sha256=<hex>` line each. The same two run against the facade in Rust,
through the npm package and through the NuGet package, and
`cargo xtask bindings-parity` requires all four to be byte-identical. Ruling 11
is what makes that the right test: a binding projects the facade 1:1 and adds
no logic of its own, so four surfaces disagreeing means one of them added
something.

The API is the facade's, closure-free where a closure could not cross:

```python
editor = doc.editor()
for widget in editor.fill_field("name", "Ada Lovelace"):
    print(f"not drawn: {widget}")     # written, and not wholly drawable

with editor.transaction():            # checkpoint, `with`, restore
    editor.set_checkbox("agree", True)

data = editor.save(mode="incremental")
assert tinker_pdf.Document(data).validate() == []
```

`fill_field` has **three** outcomes and not two: it raises when nothing was
written, returns an empty list when the value was written and every widget
drawn, and returns a non-empty one when the value was written and those widgets
were left showing what they showed before. `save` takes no defaults of its own
— every argument it does not receive is the engine's own — and `entropy` for
an encrypted save is 48 caller-supplied bytes, because this binding does not
invent randomness.

## Nothing has been published

`pip install tinker-pdf` does not work and is not meant to yet. The pipeline
exists and has been exercised as a dry run; the facade is not frozen until
0.1.0 ([`docs/architecture.md`](../../docs/architecture.md)), and until then the
version number says what the API is worth depending on.
