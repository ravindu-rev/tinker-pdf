# Python binding

PyO3 directly over the `tinker-pdf` facade — not through the C ABI, which
would only add a second error translation. Scope and design:
[`docs/plans/13-bindings.md`](../../docs/plans/13-bindings.md); packaging:
[gap 26](../../docs/plans/gaps/26-binding-packaging.md).

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

## Nothing has been published

`pip install tinker-pdf` does not work and is not meant to yet. The pipeline
exists and has been exercised as a dry run; the facade is not frozen until
0.1.0 ([plan 00](../../docs/plans/00-architecture.md)), and until then the
version number says what the API is worth depending on.
