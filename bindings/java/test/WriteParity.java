import io.github.ravindu_rev.tinkerpdf.Attachments;
import io.github.ravindu_rev.tinkerpdf.Builder;
import io.github.ravindu_rev.tinkerpdf.Document;
import io.github.ravindu_rev.tinkerpdf.Editor;
import io.github.ravindu_rev.tinkerpdf.FormData;
import io.github.ravindu_rev.tinkerpdf.OutlineEntry;
import io.github.ravindu_rev.tinkerpdf.PageBuilder;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.DestKind;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Destination;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Link;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Target;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.View;
import io.github.ravindu_rev.tinkerpdf.TinkerPdfException;
import io.github.ravindu_rev.tinkerpdf.TrustAnchors;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;

/**
 * The parity scripts, run through the Java binding.
 *
 * <pre>java --enable-preview --enable-native-access=ALL-UNNAMED WriteParity testdata/form-fields.pdf</pre>
 *
 * <p>The same scripts crates/tinker-pdf/examples/write_parity.rs runs against
 * the facade and every other binding runs through its own surface; {@code cargo
 * xtask bindings-parity} requires every surface to print the same SHA-256s.
 * Ruling 11 is what makes that the right test: a binding projects the facade
 * 1:1 and adds no logic of its own, so surfaces disagreeing means one of them
 * added something. The read texts are specified byte for byte in the facade
 * example's module documentation.
 *
 * <p>Every written artefact goes through the engine's own strict structural
 * validator before its hash is printed, because byte-identical outputs
 * agreeing tells you nothing if all of them are wrong.
 */
public final class WriteParity {
    private WriteParity() {}

    private static final HexFormat HEX = HexFormat.of();

    static void check(boolean ok, String message) {
        if (!ok) {
            System.err.println("JAVA-PARITY: FAILED: " + message);
            System.exit(1);
        }
    }

    static String sha(byte[] data) {
        try {
            return HEX.formatHex(MessageDigest.getInstance("SHA-256").digest(data));
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }

    static byte[] utf8(String text) {
        return text.getBytes(StandardCharsets.UTF_8);
    }

    /** Validates an artefact, then prints the line bindings-parity reads. */
    static void report(String script, byte[] data) {
        try (Document document = Document.open(data)) {
            var defects = document.validate();
            check(defects.isEmpty(), script + ": the artefact does not pass the strict validator: " + defects);
        }
        System.out.println("WROTE sha256=" + sha(data) + " surface=java script=" + script + " bytes=" + data.length);
    }

    static void reportRead(String script, String text) {
        byte[] bytes = utf8(text);
        String dump = System.getenv("TINKER_PARITY_DUMP");
        if (dump != null && !dump.isEmpty()) {
            System.out.print(text);
        }
        System.out.println("READ sha256=" + sha(bytes) + " surface=java script=" + script + " bytes=" + bytes.length);
    }

    /** The eight-by-eight grey image every surface builds, from the same formula. */
    static byte[] parityImage() {
        byte[] image = new byte[64];
        for (int i = 0; i < image.length; i++) {
            image[i] = (byte) ((i * 7) % 256);
        }
        return image;
    }

    /** The engine's default options but the mode, which is the script's choice. */
    static TinkerPdf.WriteOptions options(TinkerPdf.WriteMode mode) {
        TinkerPdf.WriteOptions options = TinkerPdf.writeOptions();
        options.mode = mode;
        return options;
    }

    static byte[] fillAndSave(byte[] fixture) {
        Editor editor;
        try (Document document = Document.open(fixture)) {
            // The editor holds its own reference to the object store.
            editor = document.editor();
        }
        try (editor) {
            var skipped = editor.fillField("name", "Ada Lovelace");
            check(skipped.size() == 1, "the /Rect-less widget must be reported, not swallowed");
            check(skipped.get(0).message().equals("7 0 R: no usable /Rect (12.5.2)"),
                    "unexpected report: " + skipped.get(0).message());
            check(skipped.get(0).object() == 7 && skipped.get(0).generation() == 0,
                    "the report lost the widget it names");
            check(editor.fillField("notes", "every surface writes this").isEmpty(),
                    "the control field is well formed, so nothing is skipped");
            editor.setCheckbox("agree", true);
            editor.selectRadio("colour", "red");
            return editor.save(options(TinkerPdf.WriteMode.INCREMENTAL));
        }
    }

    static Target pageTarget(int page, View view) {
        return Target.page(page, view);
    }

    static byte[] buildADocument() {
        try (Builder builder = new Builder()) {
            builder.addBaseFont("F1", "Helvetica");
            builder.addImage("Im1", TinkerPdf.ImageKind.GRAY8, 8, 8, parityImage());
            try (PageBuilder one = builder.beginPage(200, 200)) {
                one.text("F1", 14, 20, 170, "Page one");
                one.fillRect(20, 40, 60, 60, 0.25);
                one.image("Im1", 100, 40, 60, 60);
                builder.pushPage(one);
            }
            try (PageBuilder two = builder.beginPage(200, 200)) {
                two.text("F1", 14, 20, 170, "Page two");
                builder.pushPage(two);
            }
            builder.setInfo("Title", "tinker-pdf write parity");
            try (OutlineEntry first = new OutlineEntry("Page one");
                    OutlineEntry second = new OutlineEntry("Page two")) {
                first.setTarget(pageTarget(0, View.of(DestKind.FIT)));
                second.setTarget(pageTarget(1, View.of(DestKind.FIT)));
                builder.setOutline(List.of(first, second));
            }
            return builder.finish();
        }
    }

    static final TinkerPdf.Date CREATED = new TinkerPdf.Date(2026, 10, 3, 12, 0, 0, 0);
    static final byte[] PACKET = utf8("<x:xmpmeta xmlns:x='adobe:ns:meta/'/>");

    static byte[] documentOps(byte[] outline) {
        Editor editor;
        try (Document document = Document.open(outline)) {
            editor = document.editor();
        }
        try (editor) {
            editor.setPageLabels(List.of(
                    new TinkerPdf.PageLabelRange(0, TinkerPdf.LabelStyle.ROMAN_LOWER, null, 1),
                    new TinkerPdf.PageLabelRange(2, TinkerPdf.LabelStyle.DECIMAL, "A-", 1)));
            editor.attachFile(new TinkerPdf.EmbeddedFile("data.csv", "data.csv", "the numbers", "text/csv", CREATED,
                    null, utf8("a,b\n1,2\n")));
            check(editor.setInfo(TinkerPdf.InfoKey.TITLE, "Document operations") == TinkerPdf.MetadataSync.ALONE,
                    "no XMP packet yet, so the title is alone");
            editor.setInfo(TinkerPdf.InfoKey.AUTHOR, "tinker-pdf");
            editor.setInfoDate(TinkerPdf.InfoKey.CREATION_DATE, CREATED);
            editor.setTrapped(TinkerPdf.Trapped.FALSE);
            check(editor.setXmpMetadata(PACKET) == TinkerPdf.MetadataSync.OTHER_HALF_UNCHANGED,
                    "/Info has entries the packet was not checked against");
            editor.setPageBoundary(0, TinkerPdf.PageBoundary.TRIM_BOX, 10, 10, 585, 832);
            editor.setPageBoundary(1, TinkerPdf.PageBoundary.BLEED_BOX, 0, 0, 595, 842);
            try (OutlineEntry only = new OutlineEntry("Only entry")) {
                only.setTarget(pageTarget(3, new View(DestKind.FIT_H, null, null, null, 700.0, null)));
                editor.setOutline(List.of(only));
            }
            return editor.save(options(TinkerPdf.WriteMode.REWRITE));
        }
    }

    /**
     * The options a save takes, every one the C ABI carries but encryption
     * away from its default, after two edits that give them something to act
     * on: the deleted page is what garbage collection drops, and the appended
     * operators are the one stream nobody has encoded, which is what
     * compression compresses. save-linearized is the same save linearized; the
     * linearizer sets object streams and compression aside, so it is a second
     * script.
     */
    static byte[] saveOptions(byte[] operated, boolean linearize) {
        Editor editor;
        try (Document document = Document.open(operated)) {
            editor = document.editor();
        }
        try (editor) {
            editor.deletePage(1);
            editor.appendContent(0, utf8("0 0 m 100 100 l S"));
            TinkerPdf.WriteOptions chosen = options(TinkerPdf.WriteMode.REWRITE);
            chosen.linearize = linearize;
            chosen.versionMajor = 2;
            chosen.versionMinor = 0;
            chosen.objectStreams = true;
            chosen.compress = true;
            chosen.garbageCollect = true;
            return editor.save(chosen);
        }
    }

    static final Map<TinkerPdf.Removal, String> REMOVALS = Map.of(
            TinkerPdf.Removal.JAVA_SCRIPT, "javascript", TinkerPdf.Removal.DOCUMENT_JAVA_SCRIPT, "document-javascript",
            TinkerPdf.Removal.CALCULATION_ORDER, "calculation-order", TinkerPdf.Removal.XFA_FORM, "xfa-form",
            TinkerPdf.Removal.ACTION, "action", TinkerPdf.Removal.EMBEDDED_FILE_TREE, "embedded-file-tree",
            TinkerPdf.Removal.EMBEDDED_FILE, "embedded-file", TinkerPdf.Removal.INFO, "info",
            TinkerPdf.Removal.METADATA, "metadata");

    record Sanitised(byte[] saved, String report) {}

    static Sanitised sanitise(byte[] operated) {
        Editor editor;
        try (Document document = Document.open(operated)) {
            editor = document.editor();
        }
        try (editor) {
            var report = editor.sanitise(new TinkerPdf.Sanitise(true, true, true, true));
            StringBuilder text = new StringBuilder();
            for (var entry : report.removed()) {
                String holder = entry.holder() == null ? "trailer" : reference(entry.holder());
                List<String> steps = new ArrayList<>();
                for (var step : entry.path()) {
                    steps.add(step.index() != null ? "i:" + step.index() : "k:" + HEX.formatHex(step.key()));
                }
                text.append("removed ").append(REMOVALS.get(entry.what())).append(' ').append(holder).append(' ')
                        .append(String.join("/", steps)).append(' ').append(bytesToken(entry.action())).append('\n');
            }
            for (var entry : report.deleted()) {
                text.append("deleted ").append(REMOVALS.get(entry.what())).append(' ')
                        .append(reference(entry.object())).append(' ').append(bytesToken(entry.action()))
                        .append('\n');
            }
            return new Sanitised(editor.save(options(TinkerPdf.WriteMode.REWRITE)), text.toString());
        }
    }

    static byte[] linkedDocument() {
        try (Builder builder = new Builder()) {
            builder.addBaseFont("F1", "Helvetica");
            try (PageBuilder one = builder.beginPage(200, 200)) {
                one.text("F1", 12, 20, 170, "Links");
                one.link(10, 10, 60, 30, Target.uri("https://example.org/parity"));
                one.link(70, 10, 120.5, 30.25, pageTarget(1, new View(DestKind.XYZ, 10.0, null, null, null, 1.5)));
                builder.pushPage(one);
            }
            try (PageBuilder two = builder.beginPage(200, 200)) {
                builder.pushPage(two);
            }
            builder.setInfo("Title", "Read surface — parity");
            builder.setInfo("Author", "");
            try (OutlineEntry heading = new OutlineEntry("Part one");
                    OutlineEntry chapter = new OutlineEntry("Chapter one");
                    OutlineEntry elsewhere = new OutlineEntry("Elsewhere")) {
                heading.setOpen(true);
                chapter.setTarget(pageTarget(1, new View(DestKind.FIT_H, null, null, null, 150.0, null)));
                heading.addChild(chapter);
                elsewhere.setTarget(Target.uri("https://example.org/"));
                builder.setOutline(List.of(heading, elsewhere));
            }
            return builder.finish();
        }
    }

    // The contract's tokens.
    static String textToken(String value) {
        return value == null ? "-" : "s:" + HEX.formatHex(utf8(value));
    }

    static String bytesToken(byte[] value) {
        return value == null ? "-" : "b:" + HEX.formatHex(value);
    }

    static String number(Double value) {
        return value == null ? "-" : String.format("f:%016x", Double.doubleToRawLongBits(value));
    }

    static String reference(TinkerPdf.Ref value) {
        return value == null ? "-" : value.object() + "." + value.generation();
    }

    static String digest(byte[] value) {
        return value == null ? "-" : sha(value);
    }

    static String viewToken(View view) {
        return switch (view.kind()) {
            case XYZ -> "xyz " + number(view.left()) + " " + number(view.top()) + " " + number(view.zoom());
            case FIT_H -> "fith " + number(view.top());
            case FIT_V -> "fitv " + number(view.left());
            case FIT_R -> "fitr " + number(view.left()) + " " + number(view.bottom()) + " " + number(view.right())
                    + " " + number(view.top());
            case FIT_B -> "fitb";
            case FIT_BH -> "fitbh " + number(view.top());
            case FIT_BV -> "fitbv " + number(view.left());
            default -> "fit";
        };
    }

    static String destinationToken(Destination destination) {
        if (destination == null) {
            return "-";
        }
        return switch (destination.kind()) {
            case EXPLICIT -> "explicit " + (destination.pageIndex() == null ? "-" : destination.pageIndex()) + " "
                    + reference(destination.pageRef()) + " " + viewToken(destination.view());
            case NAMED -> "named " + bytesToken(destination.bytes());
            default -> "uri " + bytesToken(destination.bytes());
        };
    }

    static String actionToken(Link link) {
        return switch (link.action()) {
            case ABSENT -> "-";
            case GO_TO -> "goto " + destinationToken(link.destination());
            case GO_TO_R -> "gotor " + bytesToken(link.actionBytes()) + " " + destinationToken(link.destination());
            case URI -> "uri " + bytesToken(link.actionBytes());
            case NAMED -> "named " + bytesToken(link.actionBytes());
            case LAUNCH -> "launch " + bytesToken(link.actionBytes());
            default -> "other " + bytesToken(link.actionBytes());
        };
    }

    static final String[] INFO = {"title", "author", "subject", "keywords", "creator", "producer", "creation-date",
        "modification-date"};
    static final String[] TRAPPED = {"absent", "true", "false", "unknown"};
    static final String[] BOXES = {"media", "crop", "bleed", "trim", "art"};

    static void readDump(String name, Document document, StringBuilder out) {
        out.append("document ").append(name).append('\n');
        out.append("version ").append(textToken(document.pdfVersion())).append('\n');
        out.append("pages ").append(document.pageCount()).append('\n');
        for (TinkerPdf.InfoKey key : TinkerPdf.InfoKey.values()) {
            out.append("info ").append(INFO[key.ordinal()]).append(' ').append(textToken(document.info(key)))
                    .append('\n');
        }
        out.append("trapped ").append(TRAPPED[document.trapped().ordinal()]).append('\n');
        for (int index = 0; index < document.pageCount(); index++) {
            String label = document.pageLabel(index);
            if (label == null) {
                break;
            }
            out.append("label ").append(index).append(' ').append(textToken(label)).append('\n');
        }
        for (int index = 0; index < document.pageCount(); index++) {
            for (TinkerPdf.PageBoundary boundary : TinkerPdf.PageBoundary.values()) {
                TinkerPdf.Rect box = document.pageBox(index, boundary);
                out.append("box ").append(index).append(' ').append(BOXES[boundary.ordinal()]).append(' ')
                        .append(number(box.x0())).append(' ').append(number(box.y0())).append(' ')
                        .append(number(box.x1())).append(' ').append(number(box.y1())).append('\n');
            }
        }
        for (var item : document.outline()) {
            out.append("outline ").append(item.depth()).append(' ').append(item.open() ? 1 : 0).append(' ')
                    .append(textToken(item.title())).append(' ').append(destinationToken(item.destination()))
                    .append('\n');
        }
        for (int index = 0; index < document.pageCount(); index++) {
            for (Link link : document.links(index)) {
                out.append("link ").append(index).append(' ').append(number(link.x0())).append(' ')
                        .append(number(link.y0())).append(' ').append(number(link.x1())).append(' ')
                        .append(number(link.y1())).append(' ').append(reference(link.reference())).append(' ')
                        .append(actionToken(link)).append('\n');
            }
        }
        try (Attachments attachments = document.attachments()) {
            for (int i = 0; i < attachments.count(); i++) {
                byte[] data;
                try {
                    data = attachments.data(i);
                } catch (TinkerPdfException e) {
                    data = null;
                }
                Long size = attachments.size(i);
                out.append("attachment ").append(textToken(attachments.name(i))).append(' ')
                        .append(textToken(attachments.filename(i))).append(' ')
                        .append(textToken(attachments.description(i))).append(' ')
                        .append(size == null ? "-" : size.toString()).append(' ').append(digest(data)).append('\n');
            }
        }
        out.append("xmp ").append(digest(document.xmpMetadata())).append('\n');
        for (var warning : document.warnings()) {
            out.append("warning ").append(warning.offset()).append(' ').append(reference(warning.object()))
                    .append(' ').append(warning.kind()).append(' ').append(textToken(warning.message())).append('\n');
        }
    }

    static String readSurface(byte[] outline, byte[] operated) {
        StringBuilder out = new StringBuilder();
        ByteArrayOutputStream shifted = new ByteArrayOutputStream();
        shifted.writeBytes(utf8("JUNK\n"));
        shifted.writeBytes(outline);
        String[] names = {"shifted", "linked", "operated"};
        byte[][] documents = {shifted.toByteArray(), linkedDocument(), operated};
        for (int i = 0; i < names.length; i++) {
            try (Document document = Document.open(documents[i])) {
                readDump(names[i], document, out);
            }
        }
        return out.toString();
    }

    static final String[] COVERAGE = {"whole-file", "revision", "suspicious"};
    static final String[] CHAIN = {"anchored-to", "self-signed", "incomplete", "broken", "no-anchors",
        "no-signer-certificate"};
    static final String[] WEAKNESS = {"sha1-digest", "sha1-signature", "short-rsa-key", "covers-only-a-revision",
        "coverage-suspicious", "outside-validity"};

    static byte[] altered(byte[] data) {
        byte[] needle = utf8("verdict path");
        for (int at = 0; at + needle.length <= data.length; at++) {
            if (Arrays.equals(data, at, at + needle.length, needle, 0, needle.length)) {
                byte[] copy = data.clone();
                System.arraycopy(utf8("PATH"), 0, copy, at + "verdict ".length(), 4);
                return copy;
            }
        }
        check(false, "ecdsa-p256.pdf carries the reason the alteration changes");
        return data;
    }

    static String signatures(Path support) throws IOException {
        StringBuilder out = new StringBuilder();
        // The name written down, the fixture it is made from, and its root.
        // The altered one is ecdsa-p256.pdf with its first `verdict path`
        // changed to `verdict PATH`: only its digest moves, which is what
        // tells the digest and the signature check apart.
        String[][] signed = {{"ecdsa-p256", "ecdsa-p256", "ecdsa-p256-root"},
            {"pkcs7-sha1", "pkcs7-sha1", "pkcs7-sha1-root"}, {"document-timestamp", "document-timestamp", null},
            {"ecdsa-p256-altered", "ecdsa-p256", "ecdsa-p256-root"}};
        for (String[] entry : signed) {
            String[] pair = {entry[0], entry[2]};
            byte[] data = Files.readAllBytes(support.resolve(entry[1] + ".pdf"));
            if (!entry[0].equals(entry[1])) {
                data = altered(data);
            }
            try (Document document = Document.open(data); TrustAnchors anchors = new TrustAnchors()) {
                if (pair[1] != null) {
                    anchors.add(Files.readAllBytes(support.resolve(pair[1] + ".der")));
                }
                check(anchors.count() == (pair[1] == null ? 0 : 1), "the anchors hold what was added");
                out.append("document ").append(pair[0]).append('\n');
                int index = 0;
                for (var s : document.signatures()) {
                    List<String> spans = new ArrayList<>();
                    for (var span : s.spans()) {
                        spans.add(span.start() + ":" + span.length());
                    }
                    out.append("signature ").append(index++).append(' ').append(textToken(s.fieldName())).append(' ')
                            .append(textToken(s.subFilter())).append(' ').append(textToken(s.reason())).append(' ')
                            .append(textToken(s.location())).append(' ').append(textToken(s.name())).append(' ')
                            .append(COVERAGE[s.coverage().ordinal()]).append(' ').append(s.coversWholeFile() ? 1 : 0)
                            .append(' ').append(s.usageRights() ? 1 : 0).append(' ').append(s.certificationLevel())
                            .append(' ').append(spans.isEmpty() ? "-" : String.join(",", spans)).append('\n');
                }
                for (Long at : new Long[] {null, 0L}) {
                    int v = 0;
                    for (var verdict : document.verifySignatures(anchors, at)) {
                        List<String> weaknesses = new ArrayList<>();
                        for (var weakness : verdict.weaknesses()) {
                            weaknesses.add(WEAKNESS[weakness.ordinal()]);
                        }
                        long[] validity = verdict.signerValidity();
                        out.append("verdict ").append(at == null ? "-" : at.toString()).append(' ').append(v++)
                                .append(' ').append(new String[] {"read", "absent", "unreadable"}[verdict.cms()
                                        .ordinal()])
                                .append(' ').append(new String[] {"matches", "differs", "not-checked"}[verdict
                                        .documentDigest().ordinal()])
                                .append(' ').append(new String[] {"verified", "failed", "not-checked"}[verdict
                                        .signature().ordinal()])
                                .append(' ').append(CHAIN[verdict.chain().ordinal()]).append(' ')
                                .append(textToken(verdict.signerSubject())).append(' ')
                                .append(textToken(verdict.signerIssuer())).append(' ')
                                .append(validity == null ? "- -" : validity[0] + " " + validity[1]).append(' ')
                                .append(weaknesses.isEmpty() ? "-" : String.join(",", weaknesses)).append('\n');
                    }
                }
            }
        }
        return out.toString();
    }

    record Formed(byte[] saved, List<String> lines) {}

    /** Creates a field of every kind, applies an XFDF fixture and saves. */
    static Formed forms(byte[] fixture, Path formDataDir) throws IOException {
        Editor editor;
        try (Document document = Document.open(fixture)) {
            editor = document.editor();
        }
        try (editor) {
            List<String> lines = new ArrayList<>();
            java.util.function.BiConsumer<String, TinkerPdf.Ref> added = (name, ref) ->
                    lines.add("added " + textToken(name) + " " + ref.object() + "." + ref.generation());
            added.accept("person.given", editor.addTextField("person.given", 0, 300, 700, 500, 720, "Ada", 20, 0, 0));
            added.accept("subscribe", editor.addCheckbox("subscribe", 0, 300, 660, 320, 680, "Yes", true, 2, 0));
            added.accept("size", editor.addRadioGroup("size", List.of(
                    new TinkerPdf.RadioButton("S", 0, 300, 620, 320, 640),
                    new TinkerPdf.RadioButton("M", 0, 330, 620, 350, 640)), "M", 0, 0));
            added.accept("country", editor.addChoiceField("country", 0, 300, 580, 400, 600,
                    List.of("NZ", "LK", "UK"), true, false, "LK", 0, 10));
            added.accept("languages", editor.addChoiceField("languages", 0, 300, 500, 400, 560,
                    List.of("en", "fr"), false, false, null, 0, 0));
            List<String> widgets = new ArrayList<>();
            try (FormData data = FormData.readXfdf(Files.readAllBytes(formDataDir.resolve("form-fields.xfdf")))) {
                for (var widget : editor.applyFormData(data)) {
                    widgets.add(widget.object() + "." + widget.generation());
                }
            }
            lines.add("applied " + (widgets.isEmpty() ? "-" : String.join(",", widgets)));
            return new Formed(editor.save(options(TinkerPdf.WriteMode.REWRITE)), lines);
        }
    }

    static final String[] VALUE_KINDS = {"none", "text", "state", "many"};
    static final String[] FORM_WARNINGS = {"not-read", "value-unreadable", "tree-cut", "unnamed"};

    static void formDataDump(String label, FormData data, List<String> lines) {
        lines.add("data " + label);
        lines.add("source " + textToken(data.source()));
        for (int i = 0; i < data.count(); i++) {
            StringBuilder line = new StringBuilder("field " + textToken(data.fieldName(i)) + " "
                    + VALUE_KINDS[data.valueKind(i).ordinal()]);
            for (String value : data.values(i)) {
                line.append(' ').append(textToken(value));
            }
            lines.add(line.toString());
        }
        for (var warning : data.warnings()) {
            lines.add("warning " + FORM_WARNINGS[warning.kind().ordinal()] + " " + textToken(warning.what()) + " "
                    + textToken(warning.field()));
        }
        lines.add("fdf " + sha(data.toFdf()));
        String xfdf;
        try {
            xfdf = sha(data.toXfdf());
        } catch (TinkerPdfException e) {
            check(e.status() == TinkerPdf.Status.FORM_DATA_REFUSED, "toXfdf: " + e.getMessage());
            xfdf = "refused";
        }
        lines.add("xfdf " + xfdf);
    }

    /** hierarchy.fdf altered three ways: the warnings the fixtures never reach (the facade example says why). */
    static byte[] hostile(byte[] raw) {
        String text = new String(raw, StandardCharsets.ISO_8859_1);
        String[][] pairs = {
            {"/V (plain)", "/V 12345"},
            {"/T (untouched)", "/X (untouched)"},
            {"/V (through a reference)", "/Kids [ 2 0 R ]"},
        };
        for (String[] pair : pairs) {
            int at = text.indexOf(pair[0]);
            check(at >= 0, "hierarchy.fdf carries what the alteration changes");
            text = text.substring(0, at) + pair[1] + text.substring(at + pair[0].length());
        }
        return text.getBytes(StandardCharsets.ISO_8859_1);
    }

    static String formDataText(byte[] formed, List<String> lines, Path formDataDir) throws IOException {
        try (Document document = Document.open(formed); FormData own = document.formData()) {
            formDataDump("document", own, lines);
        }
        for (String file : List.of("form-fields.fdf", "hierarchy.fdf", "form-fields.xfdf", "hierarchy.xfdf")) {
            byte[] raw = Files.readAllBytes(formDataDir.resolve(file));
            try (FormData data = file.endsWith(".xfdf") ? FormData.readXfdf(raw) : FormData.readFdf(raw)) {
                formDataDump(file, data, lines);
            }
        }
        try (FormData data = FormData.readFdf(hostile(Files.readAllBytes(formDataDir.resolve("hierarchy.fdf"))))) {
            formDataDump("hostile.fdf", data, lines);
        }
        try (FormData built = FormData.empty()) {
            built.setSource("built.pdf");
            built.addField("a.b", TinkerPdf.FieldValueKind.TEXT, "x é");
            built.addField("a.c", TinkerPdf.FieldValueKind.STATE, "On");
            built.addField("list", TinkerPdf.FieldValueKind.MANY, "1", "2");
            built.addField("nothing", TinkerPdf.FieldValueKind.MANY);
            built.addField("empty", TinkerPdf.FieldValueKind.NONE);
            formDataDump("built", built, lines);
        }
        try (FormData unrepresentable = FormData.empty()) {
            unrepresentable.addField("bell", TinkerPdf.FieldValueKind.TEXT, "\u0007");
            formDataDump("unrepresentable", unrepresentable, lines);
        }
        for (String label : List.of("read-fdf", "read-xfdf")) {
            boolean xml = label.equals("read-xfdf");
            byte[] raw = utf8(xml ? "<root/>" : "not form data");
            try {
                (xml ? FormData.readXfdf(raw) : FormData.readFdf(raw)).close();
                lines.add(label + " accepted");
            } catch (TinkerPdfException e) {
                check(e.status() == TinkerPdf.Status.FORM_DATA_REFUSED, label + ": " + e.getMessage());
                lines.add(label + " refused");
            }
        }
        StringBuilder out = new StringBuilder();
        for (String line : lines) {
            out.append(line).append('\n');
        }
        return out.toString();
    }

    public static void main(String[] args) throws IOException {
        if (args.length != 1) {
            System.err.println("usage: WriteParity <form-fields.pdf>");
            System.exit(2);
        }
        Path fixturePath = Path.of(args[0]).toAbsolutePath();
        byte[] fixture = Files.readAllBytes(fixturePath);
        byte[] outline = Files.readAllBytes(fixturePath.getParent().resolve("outline-3level.pdf"));

        report("fill-and-save", fillAndSave(fixture));
        report("build-a-document", buildADocument());
        byte[] operated = documentOps(outline);
        report("document-ops", operated);
        Sanitised sanitised = sanitise(operated);
        report("sanitise", sanitised.saved());
        report("save-options", saveOptions(operated, false));
        report("save-linearized", saveOptions(operated, true));
        reportRead("sanitise-report", sanitised.report());
        reportRead("read-surface", readSurface(outline, operated));
        Path support = fixturePath.getParent().getParent().resolve("crates").resolve("tinker-pdf").resolve("tests")
                .resolve("signature_support");
        reportRead("signatures", signatures(support));
        Path formDataDir = support.getParent().resolve("form_data");
        Formed formed = forms(fixture, formDataDir);
        report("forms", formed.saved());
        reportRead("form-data", formDataText(formed.saved(), formed.lines(), formDataDir));
        System.out.println("JAVA-PARITY: RAN");
    }
}
