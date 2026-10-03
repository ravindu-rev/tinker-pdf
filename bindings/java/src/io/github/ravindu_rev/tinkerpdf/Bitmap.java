package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.BYTE;
import static io.github.ravindu_rev.tinkerpdf.Native.LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;

/** A rendered page. */
public final class Bitmap implements AutoCloseable {
    private MemorySegment pointer;

    Bitmap(MemorySegment pointer) {
        this.pointer = pointer;
    }

    public int width() {
        return Native.callInt(Native.tpdf_bitmap_width, pointer);
    }

    public int height() {
        return Native.callInt(Native.tpdf_bitmap_height, pointer);
    }

    /** Bytes per row. */
    public long stride() {
        return Native.callLong(Native.tpdf_bitmap_stride, pointer);
    }

    /** A copy of the pixels. */
    public byte[] pixels() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment length = Native.slot(arena, LONG);
            MemorySegment data = Native.callAddress(Native.tpdf_bitmap_data, pointer, length);
            return Native.isNull(data) ? new byte[0] : data.reinterpret(length.get(LONG, 0)).toArray(BYTE);
        }
    }

    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_bitmap_free, pointer);
            pointer = null;
        }
    }
}
