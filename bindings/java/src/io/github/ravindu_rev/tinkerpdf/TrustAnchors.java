package io.github.ravindu_rev.tinkerpdf;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;

/** Certificates the caller trusts, as DER. There is no default root store. */
public final class TrustAnchors implements AutoCloseable {
    private MemorySegment pointer;

    public TrustAnchors() {
        pointer = Native.callAddress(Native.tpdf_trust_anchors_new);
    }

    MemorySegment pointer() {
        return pointer;
    }

    public void add(byte[] der) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_trust_anchors_add, pointer, Native.bytes(arena, der), (long) der.length);
        }
    }

    public int count() {
        return Native.callInt(Native.tpdf_trust_anchors_count, pointer);
    }

    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_trust_anchors_free, pointer);
            pointer = null;
        }
    }
}
