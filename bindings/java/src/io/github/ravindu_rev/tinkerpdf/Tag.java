package io.github.ravindu_rev.tinkerpdf;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;

/** A structure element to open (14.7.2): its type and properties. The page holds its own copy. */
public final class Tag implements AutoCloseable {
    private MemorySegment pointer;

    /** An element of structure type {@code kind}. */
    public Tag(String kind) {
        byte[] k = kind.getBytes(StandardCharsets.UTF_8);
        try (Arena arena = Arena.ofConfined()) {
            pointer = Document.handle(Native.tpdf_tag_new, Native.bytes(arena, k), (long) k.length);
        }
    }

    MemorySegment pointer() {
        return pointer;
    }

    /** Sets /T, /Lang, /Alt, /ActualText or /E. */
    public void setText(TinkerPdf.TagText which, String text) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_tag_set_text, pointer, which.ordinal(), Native.cString(arena, text));
        }
    }

    /** Sets /ID. */
    public void setId(byte[] id) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_tag_set_id, pointer, Native.bytes(arena, id), (long) id.length);
        }
    }

    /** Names the element so its halves drawn apart are one element. */
    public void setKey(long key, long order) {
        Native.check(Native.tpdf_tag_set_key, pointer, key, order);
    }

    /** Writes the element even with nothing drawn inside it. */
    public void keepEmpty() {
        Native.check(Native.tpdf_tag_keep_empty, pointer);
    }

    /** Releases the tag; an element it opened stays open. */
    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_tag_free, pointer);
            pointer = null;
        }
    }
}
