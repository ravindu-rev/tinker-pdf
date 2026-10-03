"""The write-parity scripts, run through an installed wheel.

    python bindings/python/tests/write_parity.py testdata/form-fields.pdf

Run against a wheel that has been `pip install`ed, never against the source
tree -- the same rule `wheel_smoke.py` follows, and for the same reason: a
managed module that imports is not evidence that the engine came with it.

The scripts here are the *same* ones that
`crates/tinker-pdf/examples/write_parity.rs` runs against the facade,
`bindings/js/tests/write_parity.mjs` runs through wasm and
`bindings/dotnet/tests/Smoke` and the Go, Java and Ruby parity programs run
through the C ABI. `cargo xtask bindings-parity` requires every surface to
print the same SHA-256s. The third, read-surface, writes down everything the
read surface says about two documents in the text the facade example's module
documentation specifies byte for byte, and prints the hash of that. Ruling 11 is
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
import os
import pathlib
import struct
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


CREATED = (2026, 10, 3, 12, 0, 0, 0)
PACKET = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>"


def document_ops(outline_fixture: bytes) -> bytes:
    """The editor's document operations, one after another."""
    editor = tinker_pdf.Document(outline_fixture).editor()
    editor.set_page_labels([(0, "roman-lower", None, 1), (2, "decimal", "A-", 1)])
    editor.attach_file(
        "data.csv",
        "data.csv",
        b"a,b\n1,2\n",
        description="the numbers",
        mime_type="text/csv",
        created=CREATED,
    )
    assert editor.set_title("Document operations") == "alone"
    editor.set_author("tinker-pdf")
    editor.set_creation_date(CREATED)
    editor.set_trapped("false")
    assert editor.set_xmp_metadata(PACKET) == "other-half-unchanged"
    editor.set_trim_box(0, 10.0, 10.0, 585.0, 832.0)
    editor.set_bleed_box(1, 0.0, 0.0, 595.0, 842.0)
    editor.set_outline([tinker_pdf.OutlineEntry("Only entry", page=3, view="fith", top=700.0)])
    return editor.save()


# The options a save takes, every one the C ABI carries but encryption away
# from its default, after two edits that give them something to act on: the
# deleted page is what garbage collection drops, and the appended operators
# are the one stream nobody has encoded, which is what compression
# compresses. save-linearized is the same save linearized; the linearizer sets
# object streams and compression aside, so it is a second script.
def save_options(operated: bytes, linearize: bool) -> bytes:
    editor = tinker_pdf.Document(operated).editor()
    editor.delete_page(1)
    editor.append_content(0, b"0 0 m 100 100 l S")
    return editor.save(
        mode="rewrite",
        linearize=linearize,
        version=(2, 0),
        object_streams=True,
        compress=True,
        garbage_collect=True,
    )


def sanitise(operated: bytes):
    """Everything Sanitise::ALL names, taken out, with the report as text."""
    editor = tinker_pdf.Document(operated).editor()
    report = editor.sanitise(javascript=True, actions=True, embedded_files=True, metadata=True)
    lines = []
    for holder, path, what, action in report.removed:
        steps = "/".join(
            f"i:{step}" if isinstance(step, int) else f"k:{step.hex()}" for step in path
        )
        lines.append(
            f"removed {what} {'trailer' if holder is None else f'{holder[0]}.{holder[1]}'} "
            f"{steps} {_bytes(action)}"
        )
    for (number, generation), what, action in report.deleted:
        lines.append(f"deleted {what} {number}.{generation} {_bytes(action)}")
    return editor.save(), "".join(line + "\n" for line in lines)


def linked_document() -> bytes:
    """Read-surface's second document: links, an outline and /Info, built."""
    builder = tinker_pdf.DocumentBuilder()
    builder.add_base_font(b"F1", b"Helvetica")
    one = builder.begin_page(200.0, 200.0)
    one.text(b"F1", 12.0, 20.0, 170.0, "Links")
    one.link(10.0, 10.0, 60.0, 30.0, uri="https://example.org/parity")
    one.link(70.0, 10.0, 120.5, 30.25, page=1, view="xyz", left=10.0, zoom=1.5)
    builder.push_page(one)
    builder.push_page(builder.begin_page(200.0, 200.0))
    builder.set_info(b"Title", "Read surface \u2014 parity")
    builder.set_info(b"Author", "")
    chapter = tinker_pdf.OutlineEntry("Chapter one", page=1, view="fith", top=150.0)
    heading = tinker_pdf.OutlineEntry("Part one", open=True, children=[chapter])
    builder.set_outline([heading, tinker_pdf.OutlineEntry("Elsewhere", uri="https://example.org/")])
    return builder.finish()


def _text(value):
    return "-" if value is None else "s:" + value.encode("utf-8").hex()


def _bytes(value):
    return "-" if value is None else "b:" + bytes(value).hex()


def _number(value):
    return "-" if value is None else "f:" + struct.pack(">d", value).hex()


def _reference(value):
    return "-" if value is None else f"{value[0]}.{value[1]}"


def _digest(value):
    return "-" if value is None else hashlib.sha256(value).hexdigest()


def _view(view):
    kind = view.kind
    if kind == "xyz":
        return f"xyz {_number(view.left)} {_number(view.top)} {_number(view.zoom)}"
    if kind in ("fith", "fitbh"):
        return f"{kind} {_number(view.top)}"
    if kind in ("fitv", "fitbv"):
        return f"{kind} {_number(view.left)}"
    if kind == "fitr":
        return (
            f"fitr {_number(view.left)} {_number(view.bottom)} "
            f"{_number(view.right)} {_number(view.top)}"
        )
    return kind


def _destination(dest):
    if dest is None:
        return "-"
    if dest.kind == "explicit":
        page = "-" if dest.page_index is None else str(dest.page_index)
        return f"explicit {page} {_reference(dest.page_ref)} {_view(dest.view)}"
    if dest.kind == "named":
        return f"named {_bytes(dest.name)}"
    return f"uri {_bytes(dest.uri)}"


def _action(action):
    if action is None:
        return "-"
    kind = action.kind
    if kind == "goto":
        return f"goto {_destination(action.destination)}"
    if kind == "gotor":
        return f"gotor {_bytes(action.file)} {_destination(action.destination)}"
    if kind == "uri":
        return f"uri {_bytes(action.uri)}"
    if kind == "named":
        return f"named {_bytes(action.name)}"
    if kind == "launch":
        return f"launch {_bytes(action.file)}"
    return f"other {_bytes(action.subtype)}"


def _flatten(items, depth=0):
    for item in items:
        yield depth, item
        yield from _flatten(item.children, depth + 1)


def read_dump(name: str, document, out: list) -> None:
    """Everything the read surface says about one document, in the contract's order."""
    out.append(f"document {name}")
    out.append(f"version {_text(document.pdf_version)}")
    out.append(f"pages {document.page_count}")
    metadata = document.metadata
    for key, value in [
        ("title", metadata.title),
        ("author", metadata.author),
        ("subject", metadata.subject),
        ("keywords", metadata.keywords),
        ("creator", metadata.creator),
        ("producer", metadata.producer),
        ("creation-date", metadata.creation_date),
        ("modification-date", metadata.modification_date),
    ]:
        out.append(f"info {key} {_text(value)}")
    out.append(f"trapped {metadata.trapped or 'absent'}")
    for index, label in enumerate(document.page_labels()):
        out.append(f"label {index} {_text(label)}")
    for index in range(document.page_count):
        for boundary in ("media", "crop", "bleed", "trim", "art"):
            x0, y0, x1, y1 = document.page_box(index, boundary)
            out.append(
                f"box {index} {boundary} {_number(x0)} {_number(y0)} {_number(x1)} {_number(y1)}"
            )
    for depth, item in _flatten(document.outline()):
        out.append(
            f"outline {depth} {int(item.open)} {_text(item.title)} {_destination(item.destination)}"
        )
    for index in range(document.page_count):
        for link in document.links(index):
            x0, y0, x1, y1 = link.rect
            out.append(
                f"link {index} {_number(x0)} {_number(y0)} {_number(x1)} {_number(y1)} "
                f"{_reference(link.reference)} {_action(link.action)}"
            )
    for attachment in document.attachments():
        try:
            data = attachment.data()
        except ValueError:
            data = None
        size = "-" if attachment.size is None else str(attachment.size)
        out.append(
            f"attachment {_text(attachment.name)} {_text(attachment.filename)} "
            f"{_text(attachment.description)} {size} {_digest(data)}"
        )
    out.append(f"xmp {_digest(document.xmp_metadata())}")
    for warning in document.warnings():
        out.append(
            f"warning {warning.offset} {_reference(warning.object)} {warning.kind} "
            f"{_text(warning.message)}"
        )


def read_surface(outline_fixture: bytes, operated: bytes) -> str:
    """Script three: everything the read surface says about three documents."""
    lines = []
    read_dump("shifted", tinker_pdf.Document(b"JUNK\n" + outline_fixture), lines)
    read_dump("linked", tinker_pdf.Document(linked_document()), lines)
    read_dump("operated", tinker_pdf.Document(operated), lines)
    return "".join(line + "\n" for line in lines)


# The name written down, the fixture it is made from, and its root. The
# altered one is ecdsa-p256.pdf with its first `verdict path` changed to
# `verdict PATH`: only its digest moves, which is what tells the digest and
# the signature check apart.
SIGNED = [
    ("ecdsa-p256", "ecdsa-p256", "ecdsa-p256-root"),
    ("pkcs7-sha1", "pkcs7-sha1", "pkcs7-sha1-root"),
    ("document-timestamp", "document-timestamp", None),
    ("ecdsa-p256-altered", "ecdsa-p256", "ecdsa-p256-root"),
]


def signatures_dump(support: pathlib.Path) -> str:
    """Script four: every signature and both verdicts, in the contract's text."""
    lines = []
    for name, file, root in SIGNED:
        data = (support / f"{file}.pdf").read_bytes()
        if name != file:
            assert b"verdict path" in data
            data = data.replace(b"verdict path", b"verdict PATH", 1)
        document = tinker_pdf.Document(data)
        anchors = tinker_pdf.TrustAnchors()
        if root is not None:
            anchors.add((support / f"{root}.der").read_bytes())
        lines.append(f"document {name}")
        for index, signature in enumerate(document.signatures()):
            spans = ",".join(f"{start}:{end - start}" for start, end in signature.spans) or "-"
            lines.append(
                f"signature {index} {_text(signature.field)} {_text(signature.sub_filter_name)} "
                f"{_text(signature.reason)} {_text(signature.location)} {_text(signature.name)} "
                f"{signature.coverage} {int(signature.covers_whole_file)} "
                f"{int(signature.is_usage_rights)} {signature.certification_level or 0} {spans}"
            )
        for at in (None, 0):
            for index, verdict in enumerate(document.verify_signatures(anchors, at)):
                signer = verdict.signer
                validity = "- -" if signer is None else f"{signer.validity[0]} {signer.validity[1]}"
                weaknesses = ",".join(kind for kind, _ in verdict.weaknesses) or "-"
                lines.append(
                    f"verdict {'-' if at is None else at} {index} {verdict.cms} "
                    f"{verdict.document_digest} {verdict.signature} {verdict.chain} "
                    f"{_text(None if signer is None else signer.subject)} "
                    f"{_text(None if signer is None else signer.issuer)} {validity} {weaknesses}"
                )
    return "".join(line + "\n" for line in lines)


def signature_payloads_cross(support: pathlib.Path) -> None:
    """What the C ABI cannot carry and a Python object can: each arm's payload.

    Not part of the compared text, because only the facade-direct surfaces
    have it; asserted here instead, against what the fixture is known to be.
    """
    document = tinker_pdf.Document((support / "ecdsa-p256.pdf").read_bytes())
    anchors = tinker_pdf.TrustAnchors()
    anchors.add((support / "ecdsa-p256-root.der").read_bytes())
    try:
        anchors.add(b"not a certificate")
    except ValueError:
        pass
    else:
        raise AssertionError("bytes that are not a certificate must be refused")
    assert len(anchors) == 1, "a refused anchor is not kept"

    [signature] = document.signatures()
    assert signature.anchor == "field" and signature.anchor_name is None
    assert signature.sub_filter == "adbe.pkcs7.detached", signature.sub_filter
    assert signature.contents[:1] == b"\x30", "the stored /Contents is DER"
    assert signature.coverage_revision is None and signature.coverage_defect is None

    [verdict] = document.verify_signatures(anchors)
    assert verdict.cms_signers == 1, verdict.cms_signers
    assert verdict.chain_subject.endswith("CN=Tinker PDF ECDSA P256 Test Root"), verdict.chain_subject
    assert verdict.document_digest_reason is None and verdict.signature_reason is None
    assert verdict.is_trusted()

    [judged] = document.verify_signatures(anchors, 0)
    subjects = [detail for kind, detail in judged.weaknesses if kind == "outside-validity"]
    assert subjects and all(isinstance(subject, str) and subject for subject in subjects), (
        judged.weaknesses
    )

    [untrusted] = document.verify_signatures(tinker_pdf.TrustAnchors())
    assert untrusted.chain == "no-anchors" and untrusted.chain_subject is None
    assert not untrusted.is_trusted()
    print("PYTHON-PARITY: signature payloads cross")


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
    outline = pathlib.Path(fixture_path).with_name("outline-3level.pdf").read_bytes()
    report("fill-and-save", fill_and_save(fixture))
    report("build-a-document", build_a_document())
    operated = document_ops(outline)
    report("document-ops", operated)
    sanitised, removed = sanitise(operated)
    report("sanitise", sanitised)
    report("save-options", save_options(operated, False))
    report("save-linearized", save_options(operated, True))
    removed = removed.encode("utf-8")
    if os.environ.get("TINKER_PARITY_DUMP"):
        sys.stdout.write(removed.decode("utf-8"))
    print(
        f"READ sha256={hashlib.sha256(removed).hexdigest()} "
        f"surface=python script=sanitise-report bytes={len(removed)}"
    )

    dumped = read_surface(outline, operated).encode("utf-8")
    if os.environ.get("TINKER_PARITY_DUMP"):
        sys.stdout.write(dumped.decode("utf-8"))
    print(
        f"READ sha256={hashlib.sha256(dumped).hexdigest()} "
        f"surface=python script=read-surface bytes={len(dumped)}"
    )

    support = pathlib.Path(fixture_path).resolve().parent.parent / (
        "crates/tinker-pdf/tests/signature_support"
    )
    signed = signatures_dump(support).encode("utf-8")
    if os.environ.get("TINKER_PARITY_DUMP"):
        sys.stdout.write(signed.decode("utf-8"))
    print(
        f"READ sha256={hashlib.sha256(signed).hexdigest()} "
        f"surface=python script=signatures bytes={len(signed)}"
    )
    signature_payloads_cross(support)
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
