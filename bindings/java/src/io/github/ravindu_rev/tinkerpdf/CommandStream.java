package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;
import static io.github.ravindu_rev.tinkerpdf.Native.BYTE;
import static io.github.ravindu_rev.tinkerpdf.Native.DOUBLE;
import static io.github.ravindu_rev.tinkerpdf.Native.INT;
import static io.github.ravindu_rev.tinkerpdf.Native.LONG;
import static io.github.ravindu_rev.tinkerpdf.Native.SHORT;

import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Command;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandField;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandFieldTag;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandImage;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandKind;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandOutline;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.CommandRecord;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.ImageAnswer;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.PixelFormat;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/**
 * A command stream read (docs/design/command-stream.md): a page's drawing as
 * versioned bytes, which {@link Document#commandStream(int)} hands back, held
 * by the engine as {@code TpdfCommandStream}.
 *
 * <p>Every method is a call of crates/tinker-pdf-ffi's {@code src/commands.rs},
 * or a loop of them: the cursor ({@link #next()}, {@link #rewind()}), a record
 * read by its place ({@link #command(int)}, {@link #field(int, int)}), the two
 * tables a host draws from ({@link #outline(int)}, {@link #image(int)}), the
 * stream's own facts and its render, and {@link Encoder}, which writes records
 * back as bytes. {@link #replay(Device)} hands each record, in the stream's
 * order, to the {@link Device} method named for its kind. A replay to a 2D
 * canvas is a device written over this, never part of it:
 * bindings/java/test/Smoke.java carries one to {@code java.awt.Graphics2D}.
 *
 * <p>Bytes the reader refuses throw {@link TinkerPdfException} with
 * {@code BAD_ARGUMENT} and the reader's own sentence. What a read hands back
 * is a Java copy that outlives the handle.
 */
public final class CommandStream implements AutoCloseable {
    /** {@code TpdfCommand}: kind @0, number @4, index @8, field_count @12. */
    private static final long COMMAND = 16;
    /**
     * {@code TpdfCommandField}: tag @0, natural @8, integer @16, real @24,
     * data @32, count @40.
     */
    private static final long FIELD = 48;
    /** {@code TpdfCommandBytes}: data @0, len @8. */
    private static final long BYTES = 16;
    /**
     * {@code TpdfCommandOutline}: scope @0, font_id @8, code @16, found @20,
     * operators @24, numbers @40, number_count @48.
     */
    private static final long OUTLINE = 56;
    /**
     * {@code TpdfCommandImage}: image @0, reduction @8, answer @12, width @16,
     * height @20, has_window @24, window_x @28, window_y @32, window_width @36,
     * window_height @40, stencil @44, interpolate @48, codec @56, rgb @72,
     * alpha @88, two_colour @104, colours @120.
     */
    private static final long IMAGE = 128;

    private MemorySegment pointer;

    private CommandStream(MemorySegment pointer) {
        this.pointer = pointer;
    }

    /** Reads a stream's bytes, which are read and not kept. */
    public static CommandStream parse(byte[] bytes) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Native.check(Native.tpdf_command_stream_parse, Native.bytes(arena, bytes), (long) bytes.length, out);
            return new CommandStream(out.get(ADDRESS, 0));
        }
    }

    /** Releases the stream. What was read from it stays valid. */
    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_command_stream_free, pointer);
            pointer = null;
        }
    }

    /** How many records the stream holds before its end. */
    public int count() {
        return Native.callInt(Native.tpdf_command_stream_count, pointer);
    }

    /**
     * The cursor: the next record's command, moving past it. After the last
     * record it reads {@code END}, at the number of records, until
     * {@link #rewind()}.
     */
    public Command next() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, COMMAND);
            Native.check(Native.tpdf_command_stream_next, pointer, out);
            return command(out);
        }
    }

    /** Puts the cursor back on the stream's first record. */
    public void rewind() {
        Native.check(Native.tpdf_command_stream_rewind, pointer);
    }

    /** Record {@code index}'s command, wherever the cursor stands. */
    public Command command(int index) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, COMMAND);
            Native.check(Native.tpdf_command_stream_record, pointer, index, out);
            return command(out);
        }
    }

    /** Field {@code field} of record {@code index}, copied. */
    public CommandField field(int index, int field) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, FIELD);
            Native.check(Native.tpdf_command_stream_field, pointer, index, field, out);
            return field(out);
        }
    }

    /** Record {@code index}: its command and every one of its fields. */
    public CommandRecord record(int index) {
        Command command = command(index);
        List<CommandField> fields = new ArrayList<>(command.fieldCount());
        for (int at = 0; at < command.fieldCount(); at++) {
            fields.add(field(index, at));
        }
        return new CommandRecord(command.kind(), command.number(), command.index(), List.copyOf(fields));
    }

    /** Every record, in the stream's order. */
    public List<CommandRecord> records() {
        int count = count();
        List<CommandRecord> records = new ArrayList<>(count);
        for (int index = 0; index < count; index++) {
            records.add(record(index));
        }
        return records;
    }

    /** Outline record {@code index}'s glyph. A record of another kind is refused, naming both. */
    public CommandOutline outline(int index) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, OUTLINE);
            Native.check(Native.tpdf_command_stream_outline, pointer, index, out);
            return new CommandOutline(out.get(LONG, 0), out.get(LONG, 8), out.get(INT, 16), out.get(INT, 20) != 0,
                    borrowedBytes(out, 24), borrowed(out.get(ADDRESS, 40), out.get(LONG, 48) * 8).toArray(DOUBLE));
        }
    }

    /** Image decode record {@code index}. A record of another kind is refused, naming both. */
    public CommandImage image(int index) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, IMAGE);
            Native.check(Native.tpdf_command_stream_image, pointer, index, out);
            int[] window = out.get(INT, 24) == 0 ? null
                    : new int[] {out.get(INT, 28), out.get(INT, 32), out.get(INT, 36), out.get(INT, 40)};
            return new CommandImage(out.get(LONG, 0), out.get(INT, 8), ImageAnswer.values()[out.get(INT, 12)],
                    out.get(INT, 16), out.get(INT, 20), window, out.get(INT, 44) != 0, out.get(INT, 48) != 0,
                    new String(borrowedBytes(out, 56), StandardCharsets.UTF_8), borrowedBytes(out, 72),
                    borrowedBytes(out, 88), borrowedBytes(out, 104), out.asSlice(120, 8).toArray(BYTE));
        }
    }

    /** The stream's bytes, written back at its own version. */
    public byte[] toBytes() {
        return Native.takeBuffer(Document.handle(Native.tpdf_command_stream_to_bytes, pointer));
    }

    /** {major, minor}. */
    public int[] version() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment major = Native.slot(arena, SHORT);
            MemorySegment minor = Native.slot(arena, SHORT);
            Native.check(Native.tpdf_command_stream_version, pointer, major, minor);
            return new int[] {Short.toUnsignedInt(major.get(SHORT, 0)), Short.toUnsignedInt(minor.get(SHORT, 0))};
        }
    }

    /** Which page of its document the stream is of. */
    public int pageIndex() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, INT);
            Native.check(Native.tpdf_command_stream_page_index, pointer, out);
            return out.get(INT, 0);
        }
    }

    /** {width, height}: the page's size in points after rotation. */
    public double[] size() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment width = Native.slot(arena, DOUBLE);
            MemorySegment height = Native.slot(arena, DOUBLE);
            Native.check(Native.tpdf_command_stream_size, pointer, width, height);
            return new double[] {width.get(DOUBLE, 0), height.get(DOUBLE, 0)};
        }
    }

    /** Draws the stream as a render draws its page, through the engine's renderer. */
    public Bitmap render(double scale, PixelFormat format) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Native.check(Native.tpdf_command_stream_render, pointer, scale, format.ordinal(), out);
            return new Bitmap(out.get(ADDRESS, 0));
        }
    }

    /**
     * Hands every record, in the stream's order, to the method of
     * {@code device} named for its kind, and a record of a kind this build
     * does not know to {@link Device#unknown}. An exception a method throws
     * ends the replay and is thrown.
     */
    public void replay(Device device) {
        int count = count();
        for (int index = 0; index < count; index++) {
            CommandRecord record = record(index);
            if (record.kind() == null) {
                device.unknown(record);
                continue;
            }
            switch (record.kind()) {
                // The end is where a stream stops: no record is of its kind.
                case END -> { }
                case FUNCTION -> device.function(record);
                case HALFTONE -> device.halftone(record);
                case STORED -> device.stored(record);
                case COLORANTS -> device.colorants(record);
                case SOURCE -> device.source(record);
                case STATE -> device.state(record);
                case BEGIN_TEXT -> device.beginText(record);
                case GLYPH_RUN -> device.glyphRun(record);
                case END_TEXT -> device.endText(record);
                case FILL_PATH -> device.fillPath(record);
                case STROKE_PATH -> device.strokePath(record);
                case CLIP_PATH -> device.clipPath(record);
                case SAVE_STATE -> device.saveState(record);
                case RESTORE_STATE -> device.restoreState(record);
                case DRAW_IMAGE -> device.drawImage(record);
                case DRAW_SHADING -> device.drawShading(record);
                case BEGIN_MARKED_CONTENT -> device.beginMarkedContent(record);
                case END_MARKED_CONTENT -> device.endMarkedContent(record);
                case BEGIN_FORM -> device.beginForm(record);
                case END_FORM -> device.endForm(record);
                case BEGIN_GROUP -> device.beginGroup(record);
                case END_GROUP -> device.endGroup(record);
                case BEGIN_SOFT_MASK -> device.beginSoftMask(record);
                case END_SOFT_MASK -> device.endSoftMask(record);
                case CLEAR_SOFT_MASK -> device.clearSoftMask(record);
                case GROUP_BUDGET_SPENT -> device.groupBudgetSpent(record);
                case RECURSION_REFUSED -> device.recursionRefused(record);
                case NESTED_BUDGET_SPENT -> device.nestedBudgetSpent(record);
                case PAGE -> device.page(record);
                case ANNOTATION -> device.annotation(record);
                case MISSING_FONT -> device.missingFont(record);
                case REPAIRED_SPACE -> device.repairedSpace(record);
                case SCOPE -> device.scope(record);
                case OUTLINE -> device.outline(record);
                case FONT_IDENTITY -> device.fontIdentity(record);
                case PROFILE -> device.profile(record);
                case COLOR_SPACE -> device.colorSpace(record);
                case IMAGE -> device.image(record);
                case INLINE_IMAGE -> device.inlineImage(record);
                case IMAGE_DECODE -> device.imageDecode(record);
                case SHADING_DEFINITION -> device.shadingDefinition(record);
                case SHADING -> device.shading(record);
                case PATTERN -> device.pattern(record);
                case CELL -> device.cell(record);
            }
        }
    }

    private static Command command(MemorySegment raw) {
        int kind = raw.get(INT, 0);
        return new Command(kind < 0 ? null : CommandKind.values()[kind], raw.get(INT, 4), raw.get(INT, 8),
                raw.get(INT, 12));
    }

    private static CommandField field(MemorySegment raw) {
        MemorySegment data = raw.get(ADDRESS, 32);
        long count = raw.get(LONG, 40);
        return switch (CommandFieldTag.values()[raw.get(INT, 0)]) {
            case NOTHING -> CommandField.nothing();
            case UNSIGNED -> CommandField.unsigned(raw.get(LONG, 8));
            case SIGNED -> CommandField.signed(raw.get(LONG, 16));
            case REAL -> CommandField.real(raw.get(DOUBLE, 24));
            case BYTES -> CommandField.bytes(borrowed(data, count).toArray(BYTE));
            case REALS -> CommandField.reals(borrowed(data, count * 8).toArray(DOUBLE));
            case NATURALS -> CommandField.naturals(borrowed(data, count * 8).toArray(LONG));
            case STRINGS -> {
                MemorySegment list = borrowed(data, count * BYTES);
                List<byte[]> strings = new ArrayList<>((int) count);
                for (long at = 0; at < count; at++) {
                    strings.add(borrowedBytes(list, at * BYTES));
                }
                yield CommandField.strings(strings);
            }
        };
    }

    /** {@code size} bytes the stream lends at {@code data}; none for null. */
    private static MemorySegment borrowed(MemorySegment data, long size) {
        return Native.isNull(data) || size == 0 ? MemorySegment.NULL : data.reinterpret(size);
    }

    /** The {@code TpdfCommandBytes} at {@code offset} of {@code raw}, copied. */
    private static byte[] borrowedBytes(MemorySegment raw, long offset) {
        return borrowed(raw.get(ADDRESS, offset), raw.get(LONG, offset + 8)).toArray(BYTE);
    }

    /**
     * A device a replay hands records to: one method a kind, each handed the
     * record, and {@link #unknown} for a kind this build does not know. Every
     * method does nothing unless a device overrides it, so a device names the
     * kinds it draws and passes over the rest. A record's tables (its glyphs'
     * outlines and its images' decodes) are the stream's
     * {@link CommandStream#outline(int)} and {@link CommandStream#image(int)},
     * which a device that draws them holds the stream for.
     */
    public interface Device {
        default void function(CommandRecord record) {}

        default void halftone(CommandRecord record) {}

        default void stored(CommandRecord record) {}

        default void colorants(CommandRecord record) {}

        default void source(CommandRecord record) {}

        default void state(CommandRecord record) {}

        default void beginText(CommandRecord record) {}

        default void glyphRun(CommandRecord record) {}

        default void endText(CommandRecord record) {}

        default void fillPath(CommandRecord record) {}

        default void strokePath(CommandRecord record) {}

        default void clipPath(CommandRecord record) {}

        default void saveState(CommandRecord record) {}

        default void restoreState(CommandRecord record) {}

        default void drawImage(CommandRecord record) {}

        default void drawShading(CommandRecord record) {}

        default void beginMarkedContent(CommandRecord record) {}

        default void endMarkedContent(CommandRecord record) {}

        default void beginForm(CommandRecord record) {}

        default void endForm(CommandRecord record) {}

        default void beginGroup(CommandRecord record) {}

        default void endGroup(CommandRecord record) {}

        default void beginSoftMask(CommandRecord record) {}

        default void endSoftMask(CommandRecord record) {}

        default void clearSoftMask(CommandRecord record) {}

        default void groupBudgetSpent(CommandRecord record) {}

        default void recursionRefused(CommandRecord record) {}

        default void nestedBudgetSpent(CommandRecord record) {}

        default void page(CommandRecord record) {}

        default void annotation(CommandRecord record) {}

        default void missingFont(CommandRecord record) {}

        default void repairedSpace(CommandRecord record) {}

        default void scope(CommandRecord record) {}

        default void outline(CommandRecord record) {}

        default void fontIdentity(CommandRecord record) {}

        default void profile(CommandRecord record) {}

        default void colorSpace(CommandRecord record) {}

        default void image(CommandRecord record) {}

        default void inlineImage(CommandRecord record) {}

        default void imageDecode(CommandRecord record) {}

        default void shadingDefinition(CommandRecord record) {}

        default void shading(CommandRecord record) {}

        default void pattern(CommandRecord record) {}

        default void cell(CommandRecord record) {}

        default void unknown(CommandRecord record) {}
    }

    /**
     * {@code TpdfCommandEncoder}: records pushed by kind and fields, copied,
     * and finished into a stream's bytes at this build's version, read back as
     * {@link #parse} reads them. A kind or a tag the C ABI does not declare,
     * the end pushed, and records whose stream the reader refuses throw
     * {@code BAD_ARGUMENT} naming them. The encoder keeps its records, so it
     * finishes again.
     */
    public static final class Encoder implements AutoCloseable {
        private MemorySegment pointer;

        public Encoder() {
            pointer = Document.handle(Native.tpdf_command_encoder_new);
        }

        /** Appends a record of {@code kind}; null is the C ABI's -1, a kind it does not declare. */
        public void push(CommandKind kind, List<CommandField> fields) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment array = Native.slot(arena, Math.max(1, fields.size()) * FIELD);
                for (int at = 0; at < fields.size(); at++) {
                    write(arena, array.asSlice(at * FIELD, FIELD), fields.get(at));
                }
                Native.check(Native.tpdf_command_encoder_push, pointer, kind == null ? -1 : kind.ordinal(), array,
                        (long) fields.size());
            }
        }

        /** Appends {@code record}'s kind and fields. */
        public void push(CommandRecord record) {
            push(record.kind(), record.fields());
        }

        /** The records pushed so far as a stream's bytes. */
        public byte[] finish() {
            return Native.takeBuffer(Document.handle(Native.tpdf_command_encoder_finish, pointer));
        }

        @Override
        public void close() {
            if (pointer != null) {
                Native.call(Native.tpdf_command_encoder_free, pointer);
                pointer = null;
            }
        }

        /** {@code field} as a {@code TpdfCommandField} in {@code slot}, its arrays in {@code arena}. */
        private static void write(Arena arena, MemorySegment slot, CommandField field) {
            slot.set(INT, 0, field.tag().ordinal());
            Object value = field.value();
            switch (field.tag()) {
                case NOTHING -> { }
                case UNSIGNED -> slot.set(LONG, 8, (Long) value);
                case SIGNED -> slot.set(LONG, 16, (Long) value);
                case REAL -> slot.set(DOUBLE, 24, (Double) value);
                case BYTES -> {
                    byte[] bytes = (byte[]) value;
                    array(slot, Native.bytes(arena, bytes), bytes.length);
                }
                case REALS -> {
                    double[] reals = (double[]) value;
                    MemorySegment data = Native.slot(arena, Math.max(1, reals.length) * 8L);
                    MemorySegment.copy(reals, 0, data, DOUBLE, 0, reals.length);
                    array(slot, data, reals.length);
                }
                case NATURALS -> {
                    long[] naturals = (long[]) value;
                    MemorySegment data = Native.slot(arena, Math.max(1, naturals.length) * 8L);
                    MemorySegment.copy(naturals, 0, data, LONG, 0, naturals.length);
                    array(slot, data, naturals.length);
                }
                case STRINGS -> {
                    @SuppressWarnings("unchecked")
                    List<byte[]> strings = (List<byte[]>) value;
                    MemorySegment list = Native.slot(arena, Math.max(1, strings.size()) * BYTES);
                    for (int at = 0; at < strings.size(); at++) {
                        byte[] string = strings.get(at);
                        list.set(ADDRESS, at * BYTES, Native.bytes(arena, string));
                        list.set(LONG, at * BYTES + 8, string.length);
                    }
                    array(slot, list, strings.size());
                }
            }
        }

        private static void array(MemorySegment slot, MemorySegment data, long count) {
            slot.set(ADDRESS, 32, data);
            slot.set(LONG, 40, count);
        }
    }
}
