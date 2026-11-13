import io.github.ravindu_rev.tinkerpdf.Bitmap;
import io.github.ravindu_rev.tinkerpdf.Builder;
import io.github.ravindu_rev.tinkerpdf.Checkpoint;
import io.github.ravindu_rev.tinkerpdf.CommandStream;
import io.github.ravindu_rev.tinkerpdf.Document;
import io.github.ravindu_rev.tinkerpdf.Editor;
import io.github.ravindu_rev.tinkerpdf.PageBuilder;
import io.github.ravindu_rev.tinkerpdf.Source;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Command;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandField;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandFieldTag;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandImage;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandKind;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandOutline;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandRecord;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.ImageAnswer;
import io.github.ravindu_rev.tinkerpdf.TinkerPdfException;
import java.awt.BasicStroke;
import java.awt.Color;
import java.awt.Graphics2D;
import java.awt.RenderingHints;
import java.awt.geom.AffineTransform;
import java.awt.geom.Path2D;
import java.awt.image.BufferedImage;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Deque;
import java.util.EnumSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

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
 * rather than in a caller. The command stream's leg decodes the stream of a
 * page holding every kind of record through the binding's own types and
 * writes it back to the facade's bytes, drives a recording device that hears
 * every kind in the stream's order, and replays the stream to a
 * {@code Graphics2D}, the example a host's canvas device follows. The program
 * prints JAVA-SMOKE: RAN on success,
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
        commandStream(fixtures.getParent());
        System.out.println("JAVA-SMOKE: RAN, rendered and inked, every declaration called");
    }

    static void streamed(byte[] data) {
        try (Document document = Document.openStreaming(new Memory(data, false))) {
            check(document.isStreamed() && document.pageCount() == 3, "a streamed open is streamed, three pages");
            check(document.answersAsBuffered(), "a streamed open of a clean file answers as its bytes do");
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

    // ---- the command stream ------------------------------------------------

    static byte[] ascii(String text) {
        return text.getBytes(StandardCharsets.US_ASCII);
    }

    /** A stream object of {@code dictionary} holding {@code data}. */
    static byte[] streamObject(String dictionary, byte[] data) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        out.writeBytes(ascii("<< " + dictionary + " /Length " + data.length + " >>\nstream\n"));
        out.writeBytes(data);
        out.writeBytes(ascii("\nendstream"));
        return out.toByteArray();
    }

    /** A file of {@code objects}, numbered from 1, the first the catalog, with a classic cross-reference table. */
    static byte[] pdf(List<byte[]> objects) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        out.writeBytes(ascii("%PDF-1.7\n"));
        List<Integer> offsets = new ArrayList<>();
        for (int number = 1; number <= objects.size(); number++) {
            offsets.add(out.size());
            out.writeBytes(ascii(number + " 0 obj\n"));
            out.writeBytes(objects.get(number - 1));
            out.writeBytes(ascii("\nendobj\n"));
        }
        int xref = out.size();
        out.writeBytes(ascii("xref\n0 " + (objects.size() + 1) + "\n0000000000 65535 f \n"));
        for (int offset : offsets) {
            out.writeBytes(ascii(String.format(Locale.ROOT, "%010d 00000 n \n", offset)));
        }
        out.writeBytes(ascii("trailer\n<< /Size " + (objects.size() + 1) + " /Root 1 0 R >>\nstartxref\n" + xref
                + "\n%%EOF\n"));
        return out.toByteArray();
    }

    /**
     * The 200 x 160 page whose stream holds every kind a page's stream is
     * written with, the C ABI's test page
     * (crates/tinker-pdf-ffi/src/commands/tests.rs, every_kind_page) built
     * again: a state with stored parameters, a black generation, a transfer
     * function and a halftone; a clip and an axial shading in an ICC space; a
     * fill in a /Separation space and one in a Lab space whose range is written
     * backwards; text in an embedded face and in a font no name resolves; an
     * image XObject and an inline image; marked content, a group under a
     * luminosity soft mask then cleared; a tiling pattern, a form with
     * resources of its own, a form that draws itself, and a square annotation
     * with an appearance.
     */
    static byte[] everyKindPage(byte[] face, byte[] profile) {
        StringBuilder widths = new StringBuilder("500");
        for (int at = 1; at < 95; at++) {
            widths.append(" 500");
        }
        ByteArrayOutputStream content = new ByteArrayOutputStream();
        content.writeBytes(ascii("q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n"
                + "q 10 40 40 30 re W n /Sh0 sh Q\n"
                + "q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n"
                + "q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n"
                + "BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n"
                + "BT /Nope 12 Tf 10 140 Td (x) Tj ET\n"
                + "q 20 0 0 20 120 20 cm /Im0 Do Q\n"
                + "q 10 0 0 10 150 20 cm BI /W 2 /H 2 /CS /G /BPC 8 ID "));
        content.writeBytes(new byte[] {0x00, 0x55, (byte) 0xAA, (byte) 0xFF});
        content.writeBytes(ascii(" EI Q\n"
                + "/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n"
                + "q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n"
                + "q /Pattern cs /P0 scn 150 60 30 30 re f Q\n"
                + "q 1 0 0 1 150 100 cm /Fm0 Do Q\n"
                + "/Rec Do\n"));
        byte[] rgb = {(byte) 255, 0, 0, 0, (byte) 255, 0, 0, 0, (byte) 255, (byte) 255, (byte) 255, 0};
        return pdf(List.of(
                ascii("<< /Type /Catalog /Pages 2 0 R >>"),
                ascii("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
                ascii("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R "
                        + "/Contents 5 0 R /Annots [17 0 R] >>"),
                ascii("<< /Font << /F0 6 0 R >> "
                        + "/XObject << /Im0 9 0 R /G 10 0 R /Fm0 11 0 R /Rec 12 0 R >> "
                        + "/Shading << /Sh0 13 0 R >> /Pattern << /P0 15 0 R >> "
                        + "/ColorSpace << "
                        + "/CS0 [/Separation /Spot /DeviceRGB "
                        + "<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] "
                        + "/Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] /Range [100 -100 -100 100] >>] >> "
                        + "/ExtGState << "
                        + "/Fancy << /RI /Saturation "
                        + "/BG << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> "
                        + "/TR << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >> "
                        + "/HT << /Type /Halftone /HalftoneType 1 /Frequency 60 /Angle 45 "
                        + "/SpotFunction /Round >> >> "
                        + "/Masked << /SMask << /Type /Mask /S /Luminosity /G 16 0 R >> >> "
                        + "/NoMask << /SMask /None >> >> >>"),
                streamObject("", content.toByteArray()),
                ascii("<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 "
                        + "/LastChar 126 /Widths [" + widths + "] /FontDescriptor 7 0 R "
                        + "/Encoding /WinAnsiEncoding >>"),
                ascii("<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 "
                        + "/FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 "
                        + "/CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"),
                streamObject("/Length1 " + face.length, face),
                streamObject("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB "
                        + "/BitsPerComponent 8", rgb),
                streamObject("/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>",
                        ascii("0 0.8 0 rg 0 0 20 20 re f")),
                streamObject("/Type /XObject /Subtype /Form /BBox [0 0 40 40] "
                        + "/Resources << /XObject << /ImF 9 0 R >> >>", ascii("q 10 0 0 10 0 0 cm /ImF Do Q")),
                streamObject("/Type /XObject /Subtype /Form /BBox [0 0 10 10] "
                        + "/Resources << /XObject << /Rec 12 0 R >> >>", ascii("0 0 1 rg 0 0 2 2 re f /Rec Do")),
                ascii("<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] "
                        + "/Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> "
                        + "/Extend [true true] >>"),
                streamObject("/N 3", profile),
                streamObject("/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 "
                        + "/YStep 10 /Resources << >>", ascii("1 0 0 rg 0 0 5 5 re f")),
                streamObject("/Type /XObject /Subtype /Form /BBox [0 0 200 160] "
                        + "/Group << /S /Transparency /CS /DeviceGray >>", ascii("0.5 g 0 0 200 160 re f")),
                ascii("<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>"),
                streamObject("/Type /XObject /Subtype /Form /BBox [0 0 40 30]",
                        ascii("0 0 1 RG 2 w 1 1 38 28 re S"))));
    }

    @SuppressWarnings("unchecked")
    static List<byte[]> strings(Object value) {
        return (List<byte[]>) value;
    }

    /** {@code fields}, each made again from its tag and value through the binding's constructors. */
    static List<CommandField> rebuilt(List<CommandField> fields) {
        List<CommandField> out = new ArrayList<>(fields.size());
        for (CommandField field : fields) {
            Object value = field.value();
            CommandField again = switch (field.tag()) {
                case NOTHING -> CommandField.nothing();
                case UNSIGNED -> CommandField.unsigned((Long) value);
                case SIGNED -> CommandField.signed((Long) value);
                case REAL -> CommandField.real((Double) value);
                case BYTES -> CommandField.bytes((byte[]) value);
                case REALS -> CommandField.reals((double[]) value);
                case NATURALS -> CommandField.naturals((long[]) value);
                case STRINGS -> CommandField.strings(strings(value));
            };
            check(again.equals(field) && again.hashCode() == field.hashCode(), "a field rebuilds: " + field);
            out.add(again);
        }
        return out;
    }

    /** Field {@code at}'s unsigned value. */
    static long natural(CommandRecord record, int at) {
        CommandField field = record.fields().get(at);
        check(field.tag() == CommandFieldTag.UNSIGNED, record.kind() + "'s field " + at + " is " + field);
        return (Long) field.value();
    }

    /** Field {@code at}'s bytes. */
    static byte[] bytesOf(CommandRecord record, int at) {
        CommandField field = record.fields().get(at);
        check(field.tag() == CommandFieldTag.BYTES, record.kind() + "'s field " + at + " is " + field);
        return (byte[]) field.value();
    }

    /** A device that notes every record it is handed, each by the method its kind names. */
    static final class Recorder implements CommandStream.Device {
        final List<CommandKind> seen = new ArrayList<>();

        private void note(CommandKind kind, CommandRecord record) {
            check(record.kind() == kind, "the method for " + kind + " was handed a " + record.kind());
            seen.add(kind);
        }

        @Override public void function(CommandRecord record) { note(CommandKind.FUNCTION, record); }
        @Override public void halftone(CommandRecord record) { note(CommandKind.HALFTONE, record); }
        @Override public void stored(CommandRecord record) { note(CommandKind.STORED, record); }
        @Override public void colorants(CommandRecord record) { note(CommandKind.COLORANTS, record); }
        @Override public void source(CommandRecord record) { note(CommandKind.SOURCE, record); }
        @Override public void state(CommandRecord record) { note(CommandKind.STATE, record); }
        @Override public void beginText(CommandRecord record) { note(CommandKind.BEGIN_TEXT, record); }
        @Override public void glyphRun(CommandRecord record) { note(CommandKind.GLYPH_RUN, record); }
        @Override public void endText(CommandRecord record) { note(CommandKind.END_TEXT, record); }
        @Override public void fillPath(CommandRecord record) { note(CommandKind.FILL_PATH, record); }
        @Override public void strokePath(CommandRecord record) { note(CommandKind.STROKE_PATH, record); }
        @Override public void clipPath(CommandRecord record) { note(CommandKind.CLIP_PATH, record); }
        @Override public void saveState(CommandRecord record) { note(CommandKind.SAVE_STATE, record); }
        @Override public void restoreState(CommandRecord record) { note(CommandKind.RESTORE_STATE, record); }
        @Override public void drawImage(CommandRecord record) { note(CommandKind.DRAW_IMAGE, record); }
        @Override public void drawShading(CommandRecord record) { note(CommandKind.DRAW_SHADING, record); }
        @Override public void beginMarkedContent(CommandRecord record) { note(CommandKind.BEGIN_MARKED_CONTENT, record); }
        @Override public void endMarkedContent(CommandRecord record) { note(CommandKind.END_MARKED_CONTENT, record); }
        @Override public void beginForm(CommandRecord record) { note(CommandKind.BEGIN_FORM, record); }
        @Override public void endForm(CommandRecord record) { note(CommandKind.END_FORM, record); }
        @Override public void beginGroup(CommandRecord record) { note(CommandKind.BEGIN_GROUP, record); }
        @Override public void endGroup(CommandRecord record) { note(CommandKind.END_GROUP, record); }
        @Override public void beginSoftMask(CommandRecord record) { note(CommandKind.BEGIN_SOFT_MASK, record); }
        @Override public void endSoftMask(CommandRecord record) { note(CommandKind.END_SOFT_MASK, record); }
        @Override public void clearSoftMask(CommandRecord record) { note(CommandKind.CLEAR_SOFT_MASK, record); }
        @Override public void groupBudgetSpent(CommandRecord record) { note(CommandKind.GROUP_BUDGET_SPENT, record); }
        @Override public void recursionRefused(CommandRecord record) { note(CommandKind.RECURSION_REFUSED, record); }
        @Override public void nestedBudgetSpent(CommandRecord record) { note(CommandKind.NESTED_BUDGET_SPENT, record); }
        @Override public void page(CommandRecord record) { note(CommandKind.PAGE, record); }
        @Override public void annotation(CommandRecord record) { note(CommandKind.ANNOTATION, record); }
        @Override public void missingFont(CommandRecord record) { note(CommandKind.MISSING_FONT, record); }
        @Override public void repairedSpace(CommandRecord record) { note(CommandKind.REPAIRED_SPACE, record); }
        @Override public void scope(CommandRecord record) { note(CommandKind.SCOPE, record); }
        @Override public void outline(CommandRecord record) { note(CommandKind.OUTLINE, record); }
        @Override public void fontIdentity(CommandRecord record) { note(CommandKind.FONT_IDENTITY, record); }
        @Override public void profile(CommandRecord record) { note(CommandKind.PROFILE, record); }
        @Override public void colorSpace(CommandRecord record) { note(CommandKind.COLOR_SPACE, record); }
        @Override public void image(CommandRecord record) { note(CommandKind.IMAGE, record); }
        @Override public void inlineImage(CommandRecord record) { note(CommandKind.INLINE_IMAGE, record); }
        @Override public void imageDecode(CommandRecord record) { note(CommandKind.IMAGE_DECODE, record); }
        @Override public void shadingDefinition(CommandRecord record) { note(CommandKind.SHADING_DEFINITION, record); }
        @Override public void shading(CommandRecord record) { note(CommandKind.SHADING, record); }
        @Override public void pattern(CommandRecord record) { note(CommandKind.PATTERN, record); }
        @Override public void cell(CommandRecord record) { note(CommandKind.CELL, record); }

        @Override
        public void unknown(CommandRecord record) {
            check(false, "a record of kind " + record.number() + ", which this build does not know");
        }
    }

    /**
     * A replay to {@code java.awt.Graphics2D}: fills, strokes and clips, a
     * device over the projection and no part of it. A command's state is its
     * field 0, a reference to the {@code STATE} records before it in their
     * order, or nothing for a command drawn with none; its path is its
     * operators (a byte a segment: 0 a move, 1 a line, 2 a curve, 3 a close)
     * and its numbers, in the page's space with y up and the transformation
     * applied. A state's ctm is its field 0, its fill and stroke alpha its
     * fields 4 and 5, its fill and stroke colour its fields 6 and 7, and its
     * line width its field 15 (docs/design/command-stream.md, "The records").
     * Glyphs and images are the stream's outline and image decode records,
     * which a device that draws them reads as it reads the paths here.
     */
    static final class Canvas implements CommandStream.Device {
        private final List<CommandRecord> states = new ArrayList<>();
        private final Deque<Graphics2D> saved = new ArrayDeque<>();
        private Graphics2D graphics;

        Canvas(Graphics2D graphics, double scale, double height) {
            this.graphics = graphics;
            graphics.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON);
            graphics.setTransform(new AffineTransform(scale, 0, 0, -scale, 0, height * scale));
        }

        private CommandRecord stateOf(CommandRecord record) {
            CommandField state = record.fields().get(0);
            return state.tag() == CommandFieldTag.UNSIGNED ? states.get(((Long) state.value()).intValue()) : null;
        }

        private static Color colour(CommandRecord state, int colour, int alpha) {
            byte[] rgb = (byte[]) state.fields().get(colour).value();
            double opacity = (Double) state.fields().get(alpha).value();
            return new Color(rgb[0] & 0xFF, rgb[1] & 0xFF, rgb[2] & 0xFF, (int) Math.round(opacity * 255));
        }

        private static Path2D path(CommandRecord record, int operators, boolean evenOdd) {
            byte[] segments = (byte[]) record.fields().get(operators).value();
            double[] n = (double[]) record.fields().get(operators + 1).value();
            Path2D.Double path = new Path2D.Double(evenOdd ? Path2D.WIND_EVEN_ODD : Path2D.WIND_NON_ZERO);
            int at = 0;
            for (byte segment : segments) {
                switch (segment) {
                    case 0 -> {
                        path.moveTo(n[at], n[at + 1]);
                        at += 2;
                    }
                    case 1 -> {
                        path.lineTo(n[at], n[at + 1]);
                        at += 2;
                    }
                    case 2 -> {
                        path.curveTo(n[at], n[at + 1], n[at + 2], n[at + 3], n[at + 4], n[at + 5]);
                        at += 6;
                    }
                    default -> path.closePath();
                }
            }
            return path;
        }

        private static boolean evenOdd(CommandRecord record) {
            return (Long) record.fields().get(1).value() == 1;
        }

        @Override
        public void state(CommandRecord record) {
            states.add(record);
        }

        @Override
        public void saveState(CommandRecord record) {
            saved.push(graphics);
            graphics = (Graphics2D) graphics.create();
        }

        @Override
        public void restoreState(CommandRecord record) {
            if (!saved.isEmpty()) {
                graphics.dispose();
                graphics = saved.pop();
            }
        }

        @Override
        public void fillPath(CommandRecord record) {
            CommandRecord state = stateOf(record);
            if (state != null) {
                graphics.setColor(colour(state, 6, 4));
                graphics.fill(path(record, 2, evenOdd(record)));
            }
        }

        @Override
        public void strokePath(CommandRecord record) {
            CommandRecord state = stateOf(record);
            if (state != null) {
                double[] ctm = (double[]) state.fields().get(0).value();
                double width = (Double) state.fields().get(15).value()
                        * Math.sqrt(Math.abs(ctm[0] * ctm[3] - ctm[1] * ctm[2]));
                graphics.setColor(colour(state, 7, 5));
                graphics.setStroke(new BasicStroke((float) Math.max(0, width)));
                graphics.draw(path(record, 1, false));
            }
        }

        @Override
        public void clipPath(CommandRecord record) {
            graphics.clip(path(record, 2, evenOdd(record)));
        }
    }

    /**
     * The command stream through the binding's own types: the every-kind
     * page's stream decoded and written back to the facade's bytes, beside a
     * stream of the three kinds a page reaches only past a budget or through
     * a recording that composites colorants, pushed by hand; a recording
     * device that hears every kind in the stream's order; the tables'
     * accessors against their records' fields; the replay through the engine
     * against the page's render; the reader's refusals; and a replay to
     * {@code Graphics2D}.
     */
    static void commandStream(Path root) throws IOException {
        byte[] face = Files.readAllBytes(root.resolve("crates/tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf"));
        byte[] profile = Files.readAllBytes(root.resolve("fuzz/corpus/icc_profile/rgb-gamma-curve.icc"));
        byte[] written;
        double[] pageSize;
        byte[] direct;
        int[] directSize;
        try (Document document = Document.open(everyKindPage(face, profile))) {
            written = document.commandStream(0);
            pageSize = document.pageSize(0);
            try (Bitmap bitmap = document.render(0, 1.0, TinkerPdf.PixelFormat.RGB8)) {
                direct = bitmap.pixels();
                directSize = new int[] {bitmap.width(), bitmap.height()};
            }
            expectStatus(() -> document.commandStream(1), TinkerPdf.Status.NO_SUCH_PAGE, "a stream of a page past the end");
        }

        try (CommandStream stream = CommandStream.parse(written)) {
            check(Arrays.equals(stream.toBytes(), written), "the stream writes back its own bytes");
            check(stream.pageIndex() == 0, "the stream is page 0's, not " + stream.pageIndex());
            check(Arrays.equals(stream.size(), pageSize), "the stream's page is the page's size");

            List<CommandRecord> records = stream.records();
            check(records.size() == stream.count(), "every record, counted");
            check(records.get(0).kind() == CommandKind.PAGE, "the page record leads: " + records.get(0).kind());
            for (CommandRecord record : records) {
                Command command = stream.next();
                check(command.kind() != null, "kind " + record.number() + " is one this build knows");
                check(command.equals(stream.command(record.index())), "the cursor reads record " + record.index()
                        + " as its place does");
                check(command.kind() == record.kind() && command.number() == record.number()
                        && command.fieldCount() == record.fields().size(), "record " + record.index() + "'s command");
            }
            for (int twice = 0; twice < 2; twice++) {
                Command end = stream.next();
                check(end.kind() == CommandKind.END && end.number() == 0 && end.index() == records.size()
                        && end.fieldCount() == 0, "the end, after the last record: " + end);
            }
            stream.rewind();
            check(stream.next().equals(stream.command(0)), "a rewind puts the cursor back on the page record");
            expectStatus(() -> stream.command(records.size()), TinkerPdf.Status.BAD_ARGUMENT, "a record past the end");
            expectStatus(() -> stream.field(0, records.get(0).fields().size()), TinkerPdf.Status.BAD_ARGUMENT,
                    "a field past the page record's last");

            try (CommandStream.Encoder encoder = new CommandStream.Encoder()) {
                for (CommandRecord record : records) {
                    encoder.push(record.kind(), rebuilt(record.fields()));
                }
                check(Arrays.equals(encoder.finish(), written),
                        "the stream decoded and written back through the binding's types is the facade's bytes");
            }

            Set<CommandKind> onThePage = EnumSet.noneOf(CommandKind.class);
            for (CommandRecord record : records) {
                onThePage.add(record.kind());
            }
            Set<CommandKind> byHand =
                    EnumSet.of(CommandKind.COLORANTS, CommandKind.GROUP_BUDGET_SPENT, CommandKind.NESTED_BUDGET_SPENT);
            for (CommandKind kind : CommandKind.values()) {
                if (kind != CommandKind.END && !byHand.contains(kind)) {
                    check(onThePage.contains(kind), "the page's stream holds a " + kind + " record");
                }
            }

            List<CommandField> colorants = List.of(CommandField.unsigned(1),
                    CommandField.strings(List.of(ascii("Spot"), ascii("Other"))),
                    CommandField.bytes(new byte[] {(byte) 128, 64}), CommandField.naturals(new long[] {1}),
                    CommandField.bytes(new byte[] {0, 0, 0, (byte) 255}), CommandField.nothing(),
                    CommandField.bytes(new byte[] {10, 20, 30, 40}));
            byte[] hand;
            try (CommandStream.Encoder encoder = new CommandStream.Encoder()) {
                encoder.push(records.get(0));
                encoder.push(CommandKind.COLORANTS, colorants);
                encoder.push(CommandKind.GROUP_BUDGET_SPENT, List.of());
                encoder.push(CommandKind.NESTED_BUDGET_SPENT, List.of());
                hand = encoder.finish();
                expectStatus(() -> encoder.push(CommandKind.END, List.of()), TinkerPdf.Status.BAD_ARGUMENT,
                        "the end pushed");
                check(Arrays.equals(encoder.finish(), hand), "an encoder keeps its records, and a refused push adds none");
            }
            try (CommandStream.Encoder encoder = new CommandStream.Encoder()) {
                encoder.push(CommandKind.GROUP_BUDGET_SPENT, List.of());
                expectStatus(encoder::finish, TinkerPdf.Status.BAD_ARGUMENT, "records with no page record");
            }

            Set<CommandKind> heard = EnumSet.noneOf(CommandKind.class);
            try (CommandStream reread = CommandStream.parse(hand)) {
                List<CommandRecord> back = reread.records();
                List<CommandKind> kinds = new ArrayList<>();
                for (CommandRecord record : back) {
                    kinds.add(record.kind());
                }
                check(kinds.equals(List.of(CommandKind.PAGE, CommandKind.COLORANTS, CommandKind.GROUP_BUDGET_SPENT,
                        CommandKind.NESTED_BUDGET_SPENT)), "the records pushed by hand read back in order: " + kinds);
                check(back.get(0).fields().equals(records.get(0).fields()) && back.get(1).fields().equals(colorants)
                        && back.get(2).fields().isEmpty() && back.get(3).fields().isEmpty(),
                        "the records pushed by hand read back as pushed");
                check(Arrays.equals(reread.version(), stream.version()),
                        "the page's stream is at the version the encoder writes, this build's");
                try (CommandStream.Encoder encoder = new CommandStream.Encoder()) {
                    for (CommandRecord record : back) {
                        encoder.push(record.kind(), rebuilt(record.fields()));
                    }
                    check(Arrays.equals(encoder.finish(), hand), "and write back through the binding's types");
                }
                for (CommandStream each : List.of(stream, reread)) {
                    Recorder recorder = new Recorder();
                    each.replay(recorder);
                    List<CommandKind> order = new ArrayList<>();
                    for (CommandRecord record : each.records()) {
                        order.add(record.kind());
                    }
                    check(recorder.seen.equals(order), "the device receives every record in the decoder's order");
                    heard.addAll(recorder.seen);
                }
            }
            for (CommandKind kind : CommandKind.values()) {
                if (kind != CommandKind.END) {
                    check(heard.contains(kind), "the device heard a " + kind + " record");
                }
            }

            int[] runs = {0};
            stream.replay(new CommandStream.Device() {
                @Override
                public void glyphRun(CommandRecord record) {
                    check(record.kind() == CommandKind.GLYPH_RUN, "glyphRun is handed glyph runs");
                    runs[0]++;
                }
            });
            check(runs[0] == records.stream().filter(record -> record.kind() == CommandKind.GLYPH_RUN).count(),
                    "a device naming one method hears only that kind");
            try {
                stream.replay(new CommandStream.Device() {
                    @Override
                    public void beginText(CommandRecord record) {
                        throw new IllegalStateException("the device refuses");
                    }
                });
                check(false, "a device's exception ends the replay");
            } catch (IllegalStateException e) {
                check("the device refuses".equals(e.getMessage()), "the device's exception is thrown: " + e.getMessage());
            }

            int outlines = 0;
            int decodes = 0;
            for (CommandRecord record : records) {
                if (record.kind() == CommandKind.OUTLINE) {
                    CommandOutline outline = stream.outline(record.index());
                    check(outline.scope() == natural(record, 0) && outline.fontId() == natural(record, 1)
                            && outline.code() == natural(record, 2) && outline.found() == (natural(record, 3) == 1)
                            && Arrays.equals(outline.operators(), bytesOf(record, 4))
                            && Arrays.equals(outline.numbers(), (double[]) record.fields().get(5).value()),
                            "outline record " + record.index() + " is its fields");
                    outlines++;
                } else if (record.kind() == CommandKind.IMAGE_DECODE) {
                    CommandImage image = stream.image(record.index());
                    check(image.image() == natural(record, 0) && image.reduction() == natural(record, 1)
                            && image.answer().ordinal() == natural(record, 2),
                            "image decode record " + record.index() + " is its image, reduction and answer");
                    if (image.answer() == ImageAnswer.IMAGE) {
                        CommandField twoColour = record.fields().get(8);
                        CommandField window = record.fields().get(15);
                        boolean held = twoColour.tag() == CommandFieldTag.BYTES;
                        check(image.width() == natural(record, 4) && image.height() == natural(record, 5)
                                && Arrays.equals(image.rgb(), bytesOf(record, 6))
                                && Arrays.equals(image.alpha(), bytesOf(record, 7))
                                && Arrays.equals(image.twoColour(), held ? bytesOf(record, 8) : new byte[0])
                                && Arrays.equals(image.colours(), held ? bytesOf(record, 9) : new byte[8])
                                && image.stencil() == (natural(record, 10) == 1)
                                && image.interpolate() == (natural(record, 11) == 1),
                                "image decode record " + record.index() + " is its samples and colours");
                        if (window.tag() == CommandFieldTag.NATURALS) {
                            long[] edges = (long[]) window.value();
                            check(image.window() != null && image.window()[0] == edges[0]
                                    && image.window()[1] == edges[1] && image.window()[2] == edges[2]
                                    && image.window()[3] == edges[3], "image decode " + record.index() + "'s window");
                        } else {
                            check(image.window() == null, "image decode " + record.index() + " is all of its decode");
                        }
                    } else if (image.answer() == ImageAnswer.REFUSED) {
                        check(image.codec().equals(new String(bytesOf(record, 3), StandardCharsets.UTF_8)),
                                "a refused decode names its codec");
                    }
                    decodes++;
                }
            }
            check(outlines > 0 && decodes > 0, outlines + " outlines and " + decodes + " decodes");
            expectStatus(() -> stream.outline(0), TinkerPdf.Status.BAD_ARGUMENT, "an outline asked of the page record");
            expectStatus(() -> stream.image(0), TinkerPdf.Status.BAD_ARGUMENT, "a decode asked of the page record");

            try (Bitmap drawn = stream.render(1.0, TinkerPdf.PixelFormat.RGB8)) {
                check(drawn.width() == directSize[0] && drawn.height() == directSize[1]
                        && Arrays.equals(drawn.pixels(), direct), "the replay draws what a render of the page draws");
            }

            double scale = 2;
            double[] size = stream.size();
            BufferedImage canvas = new BufferedImage((int) Math.ceil(size[0] * scale), (int) Math.ceil(size[1] * scale),
                    BufferedImage.TYPE_INT_RGB);
            Graphics2D graphics = canvas.createGraphics();
            graphics.setColor(Color.WHITE);
            graphics.fillRect(0, 0, canvas.getWidth(), canvas.getHeight());
            stream.replay(new Canvas(graphics, scale, size[1]));
            graphics.dispose();
            int inked = 0;
            for (int y = 0; y < canvas.getHeight(); y++) {
                for (int x = 0; x < canvas.getWidth(); x++) {
                    if ((canvas.getRGB(x, y) & 0xFFFFFF) != 0xFFFFFF) {
                        inked++;
                    }
                }
            }
            check(inked > 0, "the replay to Graphics2D drew nothing");
            System.out.println("command stream: " + records.size() + " records, every kind, written back to the "
                    + "facade's bytes; Graphics2D inked " + inked + " pixels");
        }

        for (byte[] refused : List.of(new byte[0], ascii("not a command stream"),
                Arrays.copyOf(written, written.length - 1))) {
            expectStatus(() -> CommandStream.parse(refused).close(), TinkerPdf.Status.BAD_ARGUMENT,
                    "bytes that are no stream");
        }
        try (CommandStream never = CommandStream.parse(ascii("plainly not one"))) {
            check(false, "plainly not a stream, and read");
        } catch (TinkerPdfException e) {
            check(e.getMessage().contains("not a command stream"), "the reader's sentence: " + e.getMessage());
        }
    }
}
