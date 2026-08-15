"""Proves an installed wheel is the engine, not merely an importable module.

Run against a wheel that has been `pip install`ed, never against the source
tree:

    python bindings/python/tests/wheel_smoke.py testdata/simple-text.pdf FACE.ttf

The trap here is PRE-A's, one ecosystem out. `testdata/simple-text.pdf` names
Helvetica and embeds no font program, and this engine bundles no faces and
reads no font directories -- so a render of it returns a correctly sized,
entirely blank bitmap. Every assertion of the shape "a bitmap came back with
the right dimensions" passes on that, and it would pass equally on a wheel
whose renderer had been replaced with a memset.

So the render is asserted twice: **blank without a face, inked with one.**
The pair is what says the pixels came from the engine, and it exercises the
`set_fonts` seam at the same time -- which is the call a Python host must
make for any document that does not embed its fonts, and therefore the one
most worth proving crosses the binding.

The script prints `WHEEL-SMOKE: RAN` on success. CI greps for that, because a
Python script that imports nothing and exits 0 looks exactly like a passing
one.
"""

import pathlib
import sys

import tinker_pdf


"""One face per platform the release matrix builds on.

`auto` resolves through this list. It is a list rather than a fallback to
"render without a face" on purpose: a missing face would otherwise turn the
inked half of this test into a skip, and a skip exits 0 and reads exactly like
a pass -- the failure mode `qpdf-oracle` and `wasm-determinism` are both
written to avoid.
"""
FACES = [
    # windows-latest
    "C:/Windows/Fonts/arial.ttf",
    # macos runners
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    # ubuntu-latest, after `apt-get install fonts-dejavu-core`
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
]


def resolve_face(argument):
    if argument != "auto":
        return pathlib.Path(argument)
    for candidate in FACES:
        path = pathlib.Path(candidate)
        if path.is_file():
            return path
    raise SystemExit(
        "no font face found. This test needs one: the engine bundles no faces, "
        "so without one the inked half cannot run, and a version of it that "
        "quietly skipped would pass on a wheel that renders nothing at all.\n"
        "Looked in:\n  " + "\n  ".join(FACES)
    )


def ink(bitmap):
    """Pixels whose first channel is not the white background."""
    pixels = bitmap.data
    return sum(1 for i in range(0, len(pixels), bitmap.components) if pixels[i] != 0xFF)


def main(pdf_path, face_path):
    pdf = pathlib.Path(pdf_path).read_bytes()
    face_file = resolve_face(face_path)
    face = face_file.read_bytes()
    print(f"face {face_file}")

    print(
        f"interpreter {sys.version_info.major}.{sys.version_info.minor}, "
        f"tinker_pdf {tinker_pdf.__version__}"
    )

    document = tinker_pdf.Document(pdf)
    print(
        f"pages={document.page_count} encrypted={document.is_encrypted} "
        f"size={document.page_size(0)}"
    )
    assert document.page_count == 3, document.page_count
    assert not document.is_encrypted

    text = document.page_text(0)
    assert "Tinker fixture" in text, text
    print(f"text={text.strip()[:40]!r}")

    bare = document.render(0, dpi=72.0)
    assert ink(bare) == 0, (
        "this fixture embeds no font program, so a build that bundles no faces "
        "must draw nothing -- if it drew something, this test is measuring the "
        "wrong thing"
    )

    document.set_fonts(face)
    drawn = document.render(0, dpi=72.0)
    painted = ink(drawn)
    print(
        f"bitmap {drawn.width}x{drawn.height} stride={drawn.stride} "
        f"comps={drawn.components} bytes={len(drawn.data)} ink={painted}"
    )
    assert painted > 100, f"only {painted} pixels of ink with a face supplied"
    assert len(drawn.data) == drawn.stride * drawn.height, "the pixel buffer is the wrong size"

    # The buffer protocol plan 13 names, which is how numpy and Pillow ingest
    # pixels without a copy.
    view = memoryview(drawn.data)
    assert view.nbytes == len(drawn.data)

    print("WHEEL-SMOKE: RAN, rendered and inked")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__)
        raise SystemExit(2)
    main(sys.argv[1], sys.argv[2])
