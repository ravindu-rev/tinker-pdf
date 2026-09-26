"""The write-parity scripts, run through an installed wheel.

    python bindings/python/tests/write_parity.py testdata/form-fields.pdf

Run against a wheel that has been `pip install`ed, never against the source
tree -- the same rule `wheel_smoke.py` follows, and for the same reason: a
managed module that imports is not evidence that the engine came with it.

The two scripts here are the *same two* that
`crates/tinker-pdf/examples/write_parity.rs` runs against the facade,
`bindings/js/tests/write_parity.mjs` runs through wasm and
`bindings/dotnet/tests/Smoke` runs through the C ABI. `cargo xtask
bindings-parity` requires all four to print the same SHA-256s. Ruling 11 is
what makes that the right test: a binding projects the facade 1:1 and adds no
logic of its own, so four surfaces disagreeing means one of them added
something.

Every artefact is put through the engine's own strict structural validator
before its hash is printed, because four byte-identical outputs agreeing tells
you nothing if all four are wrong. Under ruling 13 that validator is
first-party, which is exactly why it can be a gate here instead of an external
step somebody might skip.
"""

import hashlib
import pathlib
import sys

import tinker_pdf

# The eight-by-eight grey image every surface builds, from the same formula.
# A formula rather than a fixture file on purpose: a parity suite whose four
# surfaces read the same image *file* proves they can read a file.
PARITY_IMAGE = bytes((i * 7) % 256 for i in range(64))


def fill_and_save(fixture: bytes) -> bytes:
    """Open a form, fill it, save incrementally (7.5.6)."""
    document = tinker_pdf.Document(fixture)
    editor = document.editor()

    skipped = editor.fill_field("name", "Ada Lovelace")
    assert len(skipped) == 1, (
        f"the fixture's /Rect-less widget must be reported, not swallowed: {skipped}"
    )
    assert str(skipped[0]) == "7 0 R: no usable /Rect (12.5.2)", str(skipped[0])
    assert skipped[0].object_number == 7
    assert skipped[0].generation == 0
    assert skipped[0].reason == "rect-missing"

    clean = editor.fill_field("notes", "every surface writes this")
    assert clean == [], "the control field is well formed, so nothing is skipped"

    editor.set_checkbox("agree", True)
    editor.select_radio("colour", "red")
    return editor.save(mode="incremental")


def build_a_document() -> bytes:
    """Build a document from pages, a font and an image."""
    builder = tinker_pdf.DocumentBuilder()
    builder.add_base_font(b"F1", b"Helvetica")
    builder.add_image(b"Im1", PARITY_IMAGE, "gray8", width=8, height=8)

    one = builder.begin_page(200.0, 200.0)
    one.text(b"F1", 14.0, 20.0, 170.0, "Page one")
    one.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25)
    one.image(b"Im1", 100.0, 40.0, 60.0, 60.0)
    builder.push_page(one)

    two = builder.begin_page(200.0, 200.0)
    two.text(b"F1", 14.0, 20.0, 170.0, "Page two")
    builder.push_page(two)

    builder.set_info(b"Title", "tinker-pdf write parity")
    builder.set_outline(
        [
            tinker_pdf.OutlineEntry("Page one", page=0),
            tinker_pdf.OutlineEntry("Page two", page=1),
        ]
    )
    return builder.finish()


def transaction_rolls_back_on_an_exception(fixture: bytes) -> None:
    """The context manager is checkpoint, `with`, restore -- and nothing else.

    A body that raises must leave the editor exactly as it was, which is
    asserted the only way that cannot be faked: by saving before and after and
    comparing the bytes. And the exception must still escape -- a rollback that
    also hid the reason would be the worst of both.
    """
    document = tinker_pdf.Document(fixture)
    editor = document.editor()
    editor.set_checkbox("agree", True)
    before = hashlib.sha256(editor.save(mode="incremental")).hexdigest()

    class Deliberate(Exception):
        pass

    raised = False
    try:
        with editor.transaction():
            editor.select_radio("colour", "blue")
            editor.fill_field("notes", "this must not survive")
            assert (
                hashlib.sha256(editor.save(mode="incremental")).hexdigest() != before
            ), "the body really did change the document"
            raise Deliberate("the body fails")
    except Deliberate:
        raised = True

    assert raised, "the exception was swallowed, which a rollback must never do"
    after = hashlib.sha256(editor.save(mode="incremental")).hexdigest()
    assert after == before, f"the editor was not restored: {before} -> {after}"

    # And the committing leg: a body that returns normally keeps everything.
    with editor.transaction():
        editor.select_radio("colour", "red")
    kept = hashlib.sha256(editor.save(mode="incremental")).hexdigest()
    assert kept != before, "a body that does not raise commits"

    print("PYTHON-PARITY: transaction rolls back on an exception and commits without one")


def report(script: str, data: bytes) -> None:
    """Validate, then print the line `cargo xtask bindings-parity` reads."""
    defects = tinker_pdf.Document(data).validate()
    assert defects == [], f"{script}: the artefact does not pass the strict validator: {defects}"
    print(
        f"WROTE sha256={hashlib.sha256(data).hexdigest()} "
        f"surface=python script={script} bytes={len(data)}"
    )


def main(fixture_path: str) -> None:
    fixture = pathlib.Path(fixture_path).read_bytes()
    report("fill-and-save", fill_and_save(fixture))
    report("build-a-document", build_a_document())
    transaction_rolls_back_on_an_exception(fixture)

    # A consumed handle refuses rather than producing a second document, which
    # is the Python spelling of the C ABI's SpentHandle.
    builder = tinker_pdf.DocumentBuilder()
    builder.add_base_font(b"F1", b"Helvetica")
    builder.finish()
    try:
        builder.finish()
    except ValueError as error:
        assert "already finished" in str(error), str(error)
    else:
        raise AssertionError("a second finish must be refused")

    print("PYTHON-PARITY: RAN")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__)
        raise SystemExit(2)
    main(sys.argv[1])
