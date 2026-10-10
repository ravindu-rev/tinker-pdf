import io.github.ravindu_rev.tinkerpdf.Bitmap;
import io.github.ravindu_rev.tinkerpdf.Builder;
import io.github.ravindu_rev.tinkerpdf.Checkpoint;
import io.github.ravindu_rev.tinkerpdf.Document;
import io.github.ravindu_rev.tinkerpdf.Editor;
import io.github.ravindu_rev.tinkerpdf.PageBuilder;
import io.github.ravindu_rev.tinkerpdf.Source;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf;
import io.github.ravindu_rev.tinkerpdf.TinkerPdfException;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;

/**
 * Proves the Java binding is the engine, not merely classes that compile.
 *
 * <pre>java --enable-preview --enable-native-access=ALL-UNNAMED Smoke testdata/simple-text.pdf FACE.ttf</pre>
 *
 * <p>The trap is the one every smoke test here is written against.
 * testdata/simple-text.pdf names Helvetica and embeds no font program, and the
 * engine bundles no faces and reads no font directories — so a render of it is
 * a correctly sized, entirely blank bitmap, and "a bitmap of the right size
 * came back" passes on a library whose renderer does nothing at all. So the
 * render is asserted twice: blank without a face, inked with one. Then every
 * declaration the parity program does not reach is called once, against the
 * fixture that answers it, so a declaration transcribed wrongly fails here
 * rather than in a caller. The program prints JAVA-SMOKE: RAN on success,
 * which CI greps for, because a program that exits 0 having checked nothing
 * looks exactly like a pass.
 */
public final class Smoke {
    private Smoke() {}

    static void check(boolean ok, String message) {
        if (!ok) {
            System.err.println("JAVA-SMOKE: FAILED: " + message);
            System.exit(1);
        }
    }

    interface Call {
        void run();
    }

    static void expectStatus(Call call, TinkerPdf.Status status, String what) {
        try {
            call.run();
        } catch (TinkerPdfException e) {
            check(e.status() == status, what + ": " + e.getMessage() + ", not " + status);
            return;
        }
        check(false, what + " must be refused with " + status);
    }

    /** Pixels that are not white, one byte per component of RGB. */
    static int ink(byte[] pixels) {
        int painted = 0;
        for (int i = 0; i + 2 < pixels.length; i += 3) {
            if (pixels[i] != (byte) 0xFF) {
                painted++;
            }
        }
        return painted;
    }

    /** A source over bytes in hand, which can be told to withhold them. */
    record Memory(byte[] data, boolean withhold) implements Source {
        @Override
        public long length() {
            return data.length;
        }

        @Override
        public int readAt(long offset, byte[] into) {
            if (withhold) {
                return -1;
            }
            int count = (int) Math.min(into.length, data.length - offset);
            System.arraycopy(data, (int) offset, into, 0, count);
            return count;
        }
    }

    public static void main(String[] args) throws IOException {
        if (args.length != 2) {
            System.err.println("usage: Smoke <file.pdf> <face.ttf>");
            System.exit(2);
        }
        System.out.println("engine version " + TinkerPdf.version());
        byte[] data = Files.readAllBytes(Path.of(args[0]));
        byte[] face = Files.readAllBytes(Path.of(args[1]));

        try (Document document = Document.open(data)) {
            check(document.pageCount() == 3, "pageCount " + document.pageCount());
            String text = document.pageText(0);
            check(text.contains("Tinker fixture"), "text " + text);
            System.out.println("text=\"" + text.strip() + "\"");
            try (Bitmap bare = document.render(0, 1.0, TinkerPdf.PixelFormat.RGB8)) {
                int painted = ink(bare.pixels());
                check(painted == 0, "this fixture embeds no font, so it must draw nothing yet; it drew " + painted);
            }
            document.setFonts(face);
            try (Bitmap drawn = document.render(0, 1.0, TinkerPdf.PixelFormat.RGB8)) {
                byte[] pixels = drawn.pixels();
                int painted = ink(pixels);
                System.out.println("bitmap " + drawn.width() + "x" + drawn.height() + " stride=" + drawn.stride()
                        + " bytes=" + pixels.length + " ink=" + painted);
                check(painted >= 100, "only " + painted + " pixels of ink with a face supplied");
                check(pixels.length == drawn.stride() * drawn.height(), "the pixel array is " + pixels.length);
            }
            // A page past the end is the engine's refusal, with its status.
            expectStatus(() -> document.pageText(99), TinkerPdf.Status.NO_SUCH_PAGE, "a page past the end");
            check(!document.isEncrypted() && document.mayPrint() && !document.isStreamed(),
                    "simple-text.pdf is unencrypted, printable and opened from bytes");
            check(Arrays.equals(document.pageSize(0), new double[] {595, 842}), "page size");
            expectStatus(() -> document.authenticate("anything"), TinkerPdf.Status.NOT_ENCRYPTED,
                    "authenticating an unencrypted document");
        }

        Path fixtures = Path.of(args[0]).toAbsolutePath().getParent();
        streamed(data);
        locks(fixtures);
        edits(fixtures);
        builds(face);
        System.out.println("JAVA-SMOKE: RAN, rendered and inked, every declaration called");
    }

    static void streamed(byte[] data) {
        try (Document document = Document.openStreaming(new Memory(data, false))) {
            check(document.isStreamed() && document.pageCount() == 3, "a streamed open is streamed, three pages");
            check(document.pageText(0).contains("Tinker fixture"), "streamed text");
        }
        expectStatus(() -> Document.openStreaming(new Memory(data, true)), TinkerPdf.Status.SOURCE_MISS,
                "a source that has none of the bytes");
    }

    static void locks(Path fixtures) throws IOException {
        try (Document locked = Document.open(Files.readAllBytes(fixtures.resolve("encrypted-aes256.pdf")))) {
            check(locked.isEncrypted(), "encrypted-aes256.pdf is encrypted");
            expectStatus(() -> locked.authenticate("not it"), TinkerPdf.Status.WRONG_PASSWORD, "a wrong password");
            check(locked.authenticate("open-sesame") == TinkerPdf.AuthLevel.USER, "the user password");
        }
        try (Document restricted = Document.open(Files.readAllBytes(fixtures.resolve("permissions-noprint.pdf")))) {
            restricted.authenticate("user");
            check(!restricted.mayPrint(), "permissions-noprint.pdf denies printing to its user");
        }
    }

    static void edits(Path fixtures) throws IOException {
        Editor editor;
        try (Document form = Document.open(Files.readAllBytes(fixtures.resolve("form-fields.pdf")))) {
            editor = form.editor();
        }
        try (editor) {
            List<String> names = new ArrayList<>();
            int notes = -1;
            for (int i = 0; i < editor.fieldCount(); i++) {
                names.add(editor.fieldName(i));
                if (names.get(i).equals("notes")) {
                    notes = i;
                }
            }
            Collections.sort(names);
            check(names.equals(List.of("agree", "colour", "name", "notes")), "fields " + names);
            check(!editor.isDirty(), "nothing is edited yet");
            try (Checkpoint checkpoint = editor.checkpoint()) {
                var skipped = editor.fillField("name", "Ada");
                check(skipped.get(0).defect() == TinkerPdf.WidgetDefect.RECT_MISSING, "the skipped widget's defect");
                editor.fillField("notes", "kept for a moment");
                check(editor.fieldValue(notes).equals("kept for a moment") && editor.isDirty(), "notes was filled");
                editor.restore(checkpoint);
                editor.restore(checkpoint);
            }
            check(editor.fieldValue(notes).isEmpty(), "restored notes is " + editor.fieldValue(notes));
            var recalculation = editor.recalculate(TinkerPdf.SCRIPT_DEFAULT);
            check(recalculation.changed().isEmpty() && recalculation.skipped() == 0
                    && recalculation.refused().isEmpty(), "a form with no calculations changed " + recalculation);
            check(editor.formattedValue("notes", TinkerPdf.SCRIPT_DEFAULT) == null, "notes carries no format action");
            var keystroke = editor.keystroke("notes", "x", 0, 0, false, TinkerPdf.SCRIPT_KEYSTROKE);
            check(keystroke.accepted() && "x".equals(keystroke.value()), "a field with no keystroke action " + keystroke);
            var validated = editor.validate("notes", "v", TinkerPdf.SCRIPT_VALIDATE);
            check(validated.accepted() && "v".equals(validated.value()), "a field with no validate action " + validated);

            editor.insertPage(1, 200, 300);
            editor.rotatePage(0, 90);
            editor.movePage(1, 0);
            editor.setCropBox(0, 0, 0, 100, 100);
            editor.appendContent(0, "0 0 m 10 10 l S".getBytes(StandardCharsets.US_ASCII));
            expectStatus(() -> editor.rotatePage(0, 45), TinkerPdf.Status.EDIT_REFUSED,
                    "a turn that is not a quarter-turn multiple");
            check(editor.pageCount() == 2, "the editor sees " + editor.pageCount() + " pages");
            editor.deletePage(1);
            TinkerPdf.WriteOptions options = TinkerPdf.writeOptions();
            byte[] entropy = new byte[TinkerPdf.ENTROPY_LEN];
            for (int i = 0; i < entropy.length; i++) {
                entropy[i] = (byte) i;
            }
            options.encryption = new TinkerPdf.Encryption("u", "o", -4, entropy);
            try (Document reopened = Document.open(editor.save(options))) {
                check(reopened.isEncrypted(), "the save asked for encryption");
                check(reopened.authenticate("u") == TinkerPdf.AuthLevel.USER, "the user password opens it");
                check(reopened.pageBox(0, TinkerPdf.PageBoundary.MEDIA_BOX).equals(new TinkerPdf.Rect(0, 0, 200, 300)),
                        "the inserted page is not first");
                check(Arrays.equals(reopened.pageSize(0), new double[] {100, 100}), "the crop box is the page's size");
            }
        }
    }

    static void builds(byte[] face) {
        byte[] bytes;
        try (Builder builder = new Builder()) {
            builder.addEmbeddedFont("F2", "DejaVuSans", face);
            builder.setSubsetFonts(true);
            try (PageBuilder page = builder.beginPage(200, 200)) {
                page.setFillRgb(0.2, 0.4, 0.6);
                page.setStrokeRgb(0.6, 0.4, 0.2);
                page.setCropBox(0, 0, 150, 150);
                page.raw("10 10 m 140 140 l S".getBytes(StandardCharsets.US_ASCII));
                page.text("F2", 18, 20, 100, "Embedded");
                builder.pushPage(page);
            }
            bytes = builder.finish();
        }
        try (Document built = Document.open(bytes)) {
            check(built.validate().isEmpty(), "the built document has defects " + built.validate());
            check(built.pageText(0).contains("Embedded"), "the built text is " + built.pageText(0));
            check(built.pageBox(0, TinkerPdf.PageBoundary.CROP_BOX).equals(new TinkerPdf.Rect(0, 0, 150, 150)),
                    "the crop box");
            try (Bitmap bitmap = built.render(0, 1.0, TinkerPdf.PixelFormat.RGB8)) {
                check(ink(bitmap.pixels()) > 0, "the embedded face draws");
            }
        }
        check(TinkerPdf.fitView().kind() == TinkerPdf.DestKind.FIT, "the engine fits by default");
    }
}
