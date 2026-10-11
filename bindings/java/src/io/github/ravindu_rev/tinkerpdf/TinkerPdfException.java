package io.github.ravindu_rev.tinkerpdf;

/**
 * A failure the engine reported: its status, which a caller branches on, and
 * the engine's own sentence, which names the call and the argument.
 */
public final class TinkerPdfException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    private final int code;

    TinkerPdfException(int code, String message) {
        super(message + " (status " + code + ")");
        this.code = code;
    }

    /** The {@code TpdfStatus} number, which is the ABI. */
    public int code() {
        return code;
    }

    /** The status, or null for a number this binding was transcribed before. */
    public TinkerPdf.Status status() {
        TinkerPdf.Status[] statuses = TinkerPdf.Status.values();
        return code >= 0 && code < statuses.length ? statuses[code] : null;
    }
}
