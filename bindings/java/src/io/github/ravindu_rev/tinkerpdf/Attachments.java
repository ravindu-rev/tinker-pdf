package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.INT;
import static io.github.ravindu_rev.tinkerpdf.Native.LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;

/** Every file attached to a document (7.11.4). Holds its own document. */
public final class Attachments implements AutoCloseable {
    private MemorySegment pointer;

    Attachments(MemorySegment pointer) {
        this.pointer = pointer;
    }

    public int count() {
        return Native.callInt(Native.tpdf_attachments_count, pointer);
    }

    public String name(int index) {
        return Document.text(Native.tpdf_attachment_name, pointer, index);
    }

    public String filename(int index) {
        return Document.text(Native.tpdf_attachment_filename, pointer, index);
    }

    public String description(int index) {
        return Document.text(Native.tpdf_attachment_description, pointer, index);
    }

    /** The size the file specification states; null when it states none. */
    public Long size(int index) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment present = Native.slot(arena, INT);
            MemorySegment size = Native.slot(arena, LONG);
            Native.check(Native.tpdf_attachment_size, pointer, index, present, size);
            return present.get(INT, 0) == 0 ? null : size.get(LONG, 0);
        }
    }

    /**
     * The bytes, decoded; null when no stream is named. A named stream that
     * does not read throws with {@code STREAM_UNREADABLE}.
     */
    public byte[] data(int index) {
        return Native.takeBuffer(Document.handle(Native.tpdf_attachment_data, pointer, index));
    }

    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_attachments_free, pointer);
            pointer = null;
        }
    }
}
