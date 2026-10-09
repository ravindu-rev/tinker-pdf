package io.github.ravindu_rev.tinkerpdf;

import java.lang.foreign.AddressLayout;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.StructLayout;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;

/**
 * Every function in crates/tinker-pdf-ffi/include/tinker_pdf.h, as a downcall
 * handle, and the byte-moving helpers the public classes share.
 *
 * <p>The declarations are the header's, one line each, in the header's order:
 * enums cross as {@code int}, {@code size_t} and the 64-bit integers as
 * {@code long}, every pointer as an address, and {@code TpdfSourceVtable} by
 * value as the three-pointer struct it is. Nothing here decides anything.
 *
 * <p>The library is the one named by the {@code tinkerpdf.library} system
 * property, else by the {@code TINKER_PDF_LIB} environment variable, else
 * {@code tinker_pdf_ffi} on the platform's own library search path.
 */
final class Native {
    private Native() {}

    static final ValueLayout.OfInt INT = ValueLayout.JAVA_INT;
    static final ValueLayout.OfLong LONG = ValueLayout.JAVA_LONG;
    static final ValueLayout.OfShort SHORT = ValueLayout.JAVA_SHORT;
    static final ValueLayout.OfByte BYTE = ValueLayout.JAVA_BYTE;
    static final ValueLayout.OfDouble DOUBLE = ValueLayout.JAVA_DOUBLE;
    static final AddressLayout ADDRESS = ValueLayout.ADDRESS;

    /** {@code TpdfSourceVtable}: len, read, free; 24 bytes. */
    static final StructLayout VTABLE = MemoryLayout.structLayout(
            ADDRESS.withName("len"), ADDRESS.withName("read"), ADDRESS.withName("free"));

    static final Linker LINKER = Linker.nativeLinker();
    private static final SymbolLookup LIBRARY = load();

    private static SymbolLookup load() {
        String path = System.getProperty("tinkerpdf.library");
        if (path == null) {
            path = System.getenv("TINKER_PDF_LIB");
        }
        if (path != null) {
            return SymbolLookup.libraryLookup(Path.of(path), Arena.global());
        }
        return SymbolLookup.libraryLookup(System.mapLibraryName("tinker_pdf_ffi"), Arena.global());
    }

    private static MemorySegment symbol(String name) {
        return LIBRARY.find(name).orElseThrow(() -> new UnsatisfiedLinkError("the tinker-pdf library has no " + name));
    }

    static MethodHandle function(String name, MemoryLayout result, MemoryLayout... arguments) {
        return LINKER.downcallHandle(symbol(name), FunctionDescriptor.of(result, arguments));
    }

    static MethodHandle procedure(String name, MemoryLayout... arguments) {
        return LINKER.downcallHandle(symbol(name), FunctionDescriptor.ofVoid(arguments));
    }

    static final MethodHandle tpdf_last_error_message = function("tpdf_last_error_message", ADDRESS);
    static final MethodHandle tpdf_version = function("tpdf_version", ADDRESS);
    static final MethodHandle tpdf_document_open = function("tpdf_document_open", INT, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_document_open_streaming = function("tpdf_document_open_streaming", INT, VTABLE, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_document_is_streamed = function("tpdf_document_is_streamed", INT, ADDRESS);
    static final MethodHandle tpdf_document_free = procedure("tpdf_document_free", ADDRESS);
    static final MethodHandle tpdf_document_page_count = function("tpdf_document_page_count", INT, ADDRESS);
    static final MethodHandle tpdf_document_is_encrypted = function("tpdf_document_is_encrypted", INT, ADDRESS);
    static final MethodHandle tpdf_document_authenticate = function("tpdf_document_authenticate", INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_document_may_print = function("tpdf_document_may_print", INT, ADDRESS);
    static final MethodHandle tpdf_page_size = function("tpdf_page_size", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_page_text = function("tpdf_page_text", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_string_free = procedure("tpdf_string_free", ADDRESS);
    static final MethodHandle tpdf_document_set_fonts = function("tpdf_document_set_fonts", INT, ADDRESS, ADDRESS, LONG, ADDRESS, LONG, ADDRESS, LONG, ADDRESS, LONG);
    static final MethodHandle tpdf_page_render = function("tpdf_page_render", INT, ADDRESS, INT, DOUBLE, INT, ADDRESS);
    static final MethodHandle tpdf_bitmap_width = function("tpdf_bitmap_width", INT, ADDRESS);
    static final MethodHandle tpdf_bitmap_height = function("tpdf_bitmap_height", INT, ADDRESS);
    static final MethodHandle tpdf_bitmap_stride = function("tpdf_bitmap_stride", LONG, ADDRESS);
    static final MethodHandle tpdf_bitmap_data = function("tpdf_bitmap_data", ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_bitmap_free = procedure("tpdf_bitmap_free", ADDRESS);
    static final MethodHandle tpdf_document_validate = function("tpdf_document_validate", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_defects_count = function("tpdf_defects_count", INT, ADDRESS);
    static final MethodHandle tpdf_defect_rule = function("tpdf_defect_rule", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_defect_message = function("tpdf_defect_message", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_defects_free = procedure("tpdf_defects_free", ADDRESS);
    static final MethodHandle tpdf_document_signatures = function("tpdf_document_signatures", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_signatures_count = function("tpdf_signatures_count", INT, ADDRESS);
    static final MethodHandle tpdf_signatures_free = procedure("tpdf_signatures_free", ADDRESS);
    static final MethodHandle tpdf_signature_field_name = function("tpdf_signature_field_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_signature_sub_filter = function("tpdf_signature_sub_filter", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_signature_reason = function("tpdf_signature_reason", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_signature_location = function("tpdf_signature_location", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_signature_name = function("tpdf_signature_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_signature_coverage = function("tpdf_signature_coverage", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_signature_covers_whole_file = function("tpdf_signature_covers_whole_file", INT, ADDRESS, INT);
    static final MethodHandle tpdf_signature_is_usage_rights = function("tpdf_signature_is_usage_rights", INT, ADDRESS, INT);
    static final MethodHandle tpdf_signature_certification_level = function("tpdf_signature_certification_level", INT, ADDRESS, INT);
    static final MethodHandle tpdf_signature_span_count = function("tpdf_signature_span_count", INT, ADDRESS, INT);
    static final MethodHandle tpdf_signature_span = function("tpdf_signature_span", INT, ADDRESS, INT, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_trust_anchors_new = function("tpdf_trust_anchors_new", ADDRESS);
    static final MethodHandle tpdf_trust_anchors_add = function("tpdf_trust_anchors_add", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_trust_anchors_count = function("tpdf_trust_anchors_count", INT, ADDRESS);
    static final MethodHandle tpdf_trust_anchors_free = procedure("tpdf_trust_anchors_free", ADDRESS);
    static final MethodHandle tpdf_document_verify_signatures = function("tpdf_document_verify_signatures", INT, ADDRESS, ADDRESS, INT, LONG, ADDRESS);
    static final MethodHandle tpdf_verdicts_count = function("tpdf_verdicts_count", INT, ADDRESS);
    static final MethodHandle tpdf_verdicts_free = procedure("tpdf_verdicts_free", ADDRESS);
    static final MethodHandle tpdf_verdict_cms_state = function("tpdf_verdict_cms_state", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_verdict_document_digest = function("tpdf_verdict_document_digest", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_verdict_signature_check = function("tpdf_verdict_signature_check", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_verdict_chain = function("tpdf_verdict_chain", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_verdict_signer_subject = function("tpdf_verdict_signer_subject", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_verdict_signer_issuer = function("tpdf_verdict_signer_issuer", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_verdict_signer_validity = function("tpdf_verdict_signer_validity", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_verdict_weakness_count = function("tpdf_verdict_weakness_count", INT, ADDRESS, INT);
    static final MethodHandle tpdf_verdict_weakness = function("tpdf_verdict_weakness", INT, ADDRESS, INT, INT, ADDRESS);
    static final MethodHandle tpdf_write_options_init = function("tpdf_write_options_init", INT, ADDRESS);
    static final MethodHandle tpdf_document_editor = function("tpdf_document_editor", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_free = procedure("tpdf_editor_free", ADDRESS);
    static final MethodHandle tpdf_editor_is_dirty = function("tpdf_editor_is_dirty", INT, ADDRESS);
    static final MethodHandle tpdf_editor_page_count = function("tpdf_editor_page_count", INT, ADDRESS);
    static final MethodHandle tpdf_editor_delete_page = function("tpdf_editor_delete_page", INT, ADDRESS, INT);
    static final MethodHandle tpdf_editor_move_page = function("tpdf_editor_move_page", INT, ADDRESS, INT, INT);
    static final MethodHandle tpdf_editor_rotate_page = function("tpdf_editor_rotate_page", INT, ADDRESS, INT, LONG);
    static final MethodHandle tpdf_editor_insert_page = function("tpdf_editor_insert_page", INT, ADDRESS, INT, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_editor_set_crop_box = function("tpdf_editor_set_crop_box", INT, ADDRESS, INT, DOUBLE, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_editor_append_content = function("tpdf_editor_append_content", INT, ADDRESS, INT, ADDRESS, LONG);
    static final MethodHandle tpdf_editor_field_count = function("tpdf_editor_field_count", INT, ADDRESS);
    static final MethodHandle tpdf_editor_field_name = function("tpdf_editor_field_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_editor_field_value = function("tpdf_editor_field_value", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_editor_fill_field = function("tpdf_editor_fill_field", INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_set_checkbox = function("tpdf_editor_set_checkbox", INT, ADDRESS, ADDRESS, INT);
    static final MethodHandle tpdf_editor_select_radio = function("tpdf_editor_select_radio", INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_checkpoint = function("tpdf_editor_checkpoint", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_restore = function("tpdf_editor_restore", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_checkpoint_free = procedure("tpdf_checkpoint_free", ADDRESS);
    static final MethodHandle tpdf_editor_save = function("tpdf_editor_save", INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_buffer_data = function("tpdf_buffer_data", ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_buffer_len = function("tpdf_buffer_len", LONG, ADDRESS);
    static final MethodHandle tpdf_buffer_free = procedure("tpdf_buffer_free", ADDRESS);
    static final MethodHandle tpdf_fill_report_count = function("tpdf_fill_report_count", INT, ADDRESS);
    static final MethodHandle tpdf_fill_report_message = function("tpdf_fill_report_message", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_fill_report_widget = function("tpdf_fill_report_widget", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_fill_report_defect = function("tpdf_fill_report_defect", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_fill_report_free = procedure("tpdf_fill_report_free", ADDRESS);
    static final MethodHandle tpdf_destination_init_fit = function("tpdf_destination_init_fit", INT, ADDRESS);
    static final MethodHandle tpdf_builder_new = function("tpdf_builder_new", INT, ADDRESS);
    static final MethodHandle tpdf_builder_free = procedure("tpdf_builder_free", ADDRESS);
    static final MethodHandle tpdf_builder_add_base_font = function("tpdf_builder_add_base_font", INT, ADDRESS, ADDRESS, LONG, ADDRESS, LONG);
    static final MethodHandle tpdf_builder_add_embedded_font = function("tpdf_builder_add_embedded_font", INT, ADDRESS, ADDRESS, LONG, ADDRESS, LONG, ADDRESS, LONG);
    static final MethodHandle tpdf_builder_set_subset_fonts = function("tpdf_builder_set_subset_fonts", INT, ADDRESS, INT);
    static final MethodHandle tpdf_builder_add_image = function("tpdf_builder_add_image", INT, ADDRESS, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_builder_set_info = function("tpdf_builder_set_info", INT, ADDRESS, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_editor_recalculate = function("tpdf_editor_recalculate", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_free = procedure("tpdf_recalculation_free", ADDRESS);
    static final MethodHandle tpdf_recalculation_changed_count = function("tpdf_recalculation_changed_count", INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_changed_name = function("tpdf_recalculation_changed_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_changed_value = function("tpdf_recalculation_changed_value", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_skipped_count = function("tpdf_recalculation_skipped_count", INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_cascades_cut_count = function("tpdf_recalculation_cascades_cut_count", INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_cascades_cut_name = function("tpdf_recalculation_cascades_cut_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_refused_count = function("tpdf_recalculation_refused_count", INT, ADDRESS);
    static final MethodHandle tpdf_recalculation_refused_name = function("tpdf_recalculation_refused_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_editor_formatted_value = function("tpdf_editor_formatted_value", INT, ADDRESS, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_editor_keystroke = function("tpdf_editor_keystroke", INT, ADDRESS, ADDRESS, ADDRESS, LONG, LONG, INT, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_validate = function("tpdf_editor_validate", INT, ADDRESS, ADDRESS, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_builder_begin_page = function("tpdf_builder_begin_page", INT, ADDRESS, DOUBLE, DOUBLE, ADDRESS);
    static final MethodHandle tpdf_builder_push_page = function("tpdf_builder_push_page", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_page_builder_free = procedure("tpdf_page_builder_free", ADDRESS);
    static final MethodHandle tpdf_page_builder_text = function("tpdf_page_builder_text", INT, ADDRESS, ADDRESS, LONG, DOUBLE, DOUBLE, DOUBLE, ADDRESS);
    static final MethodHandle tpdf_page_builder_fill_rect = function("tpdf_page_builder_fill_rect", INT, ADDRESS, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_builder_image = function("tpdf_page_builder_image", INT, ADDRESS, ADDRESS, LONG, DOUBLE, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_builder_set_fill_rgb = function("tpdf_page_builder_set_fill_rgb", INT, ADDRESS, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_builder_set_stroke_rgb = function("tpdf_page_builder_set_stroke_rgb", INT, ADDRESS, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_builder_set_crop_box = function("tpdf_page_builder_set_crop_box", INT, ADDRESS, DOUBLE, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_builder_raw = function("tpdf_page_builder_raw", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_page_builder_link = function("tpdf_page_builder_link", INT, ADDRESS, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS);
    static final MethodHandle tpdf_outline_entry_new = function("tpdf_outline_entry_new", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_outline_entry_set_target = function("tpdf_outline_entry_set_target", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_outline_entry_set_open = function("tpdf_outline_entry_set_open", INT, ADDRESS, INT);
    static final MethodHandle tpdf_outline_entry_add_child = function("tpdf_outline_entry_add_child", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_outline_entry_free = procedure("tpdf_outline_entry_free", ADDRESS);
    static final MethodHandle tpdf_builder_set_outline = function("tpdf_builder_set_outline", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_builder_finish = function("tpdf_builder_finish", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_set_page_labels = function("tpdf_editor_set_page_labels", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_editor_attach_file = function("tpdf_editor_attach_file", INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_set_outline = function("tpdf_editor_set_outline", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_editor_set_info = function("tpdf_editor_set_info", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_set_info_date = function("tpdf_editor_set_info_date", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_set_trapped = function("tpdf_editor_set_trapped", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_editor_set_xmp_metadata = function("tpdf_editor_set_xmp_metadata", INT, ADDRESS, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_editor_set_page_boundary = function("tpdf_editor_set_page_boundary", INT, ADDRESS, INT, INT, DOUBLE, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_boundary = function("tpdf_page_boundary", INT, ADDRESS, INT, INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_sanitise = function("tpdf_editor_sanitise", INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_sanitise_report_count = function("tpdf_sanitise_report_count", INT, ADDRESS, INT);
    static final MethodHandle tpdf_sanitise_report_entry = function("tpdf_sanitise_report_entry", INT, ADDRESS, INT, INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_sanitise_report_action = function("tpdf_sanitise_report_action", INT, ADDRESS, INT, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_sanitise_report_path_count = function("tpdf_sanitise_report_path_count", INT, ADDRESS, INT);
    static final MethodHandle tpdf_sanitise_report_path_step = function("tpdf_sanitise_report_path_step", INT, ADDRESS, INT, INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_sanitise_report_free = procedure("tpdf_sanitise_report_free", ADDRESS);
    static final MethodHandle tpdf_editor_add_text_field = function("tpdf_editor_add_text_field", INT, ADDRESS, ADDRESS, INT, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS, INT, INT, LONG, DOUBLE, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_add_checkbox = function("tpdf_editor_add_checkbox", INT, ADDRESS, ADDRESS, INT, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS, INT, LONG, DOUBLE, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_add_radio_group = function("tpdf_editor_add_radio_group", INT, ADDRESS, ADDRESS, ADDRESS, LONG, ADDRESS, LONG, DOUBLE, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_editor_add_choice_field = function("tpdf_editor_add_choice_field", INT, ADDRESS, ADDRESS, INT, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS, LONG, INT, INT, ADDRESS, LONG, DOUBLE, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_document_form_data = function("tpdf_document_form_data", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_form_data_read_fdf = function("tpdf_form_data_read_fdf", INT, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_form_data_read_xfdf = function("tpdf_form_data_read_xfdf", INT, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_form_data_new = function("tpdf_form_data_new", INT, ADDRESS);
    static final MethodHandle tpdf_form_data_add_field = function("tpdf_form_data_add_field", INT, ADDRESS, ADDRESS, INT, ADDRESS, LONG);
    static final MethodHandle tpdf_form_data_set_source = function("tpdf_form_data_set_source", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_form_data_to_fdf = function("tpdf_form_data_to_fdf", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_form_data_to_xfdf = function("tpdf_form_data_to_xfdf", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_form_data_count = function("tpdf_form_data_count", INT, ADDRESS);
    static final MethodHandle tpdf_form_data_field_name = function("tpdf_form_data_field_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_form_data_field_value_kind = function("tpdf_form_data_field_value_kind", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_form_data_field_value_count = function("tpdf_form_data_field_value_count", INT, ADDRESS, INT);
    static final MethodHandle tpdf_form_data_field_value = function("tpdf_form_data_field_value", INT, ADDRESS, INT, INT, ADDRESS);
    static final MethodHandle tpdf_form_data_source = function("tpdf_form_data_source", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_form_data_warning_count = function("tpdf_form_data_warning_count", INT, ADDRESS);
    static final MethodHandle tpdf_form_data_warning = function("tpdf_form_data_warning", INT, ADDRESS, INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_form_data_free = procedure("tpdf_form_data_free", ADDRESS);
    static final MethodHandle tpdf_editor_apply_form_data = function("tpdf_editor_apply_form_data", INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_builder_new_with_version = function("tpdf_builder_new_with_version", INT, INT, INT, ADDRESS);
    static final MethodHandle tpdf_builder_clear_image_resources = function("tpdf_builder_clear_image_resources", INT, ADDRESS);
    static final MethodHandle tpdf_builder_add_named_font = function("tpdf_builder_add_named_font", INT, ADDRESS, ADDRESS, LONG, ADDRESS, LONG, INT, ADDRESS, LONG, ADDRESS, LONG);
    static final MethodHandle tpdf_ext_gstate_init = function("tpdf_ext_gstate_init", INT, ADDRESS);
    static final MethodHandle tpdf_builder_add_ext_gstate = function("tpdf_builder_add_ext_gstate", INT, ADDRESS, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_builder_add_form = function("tpdf_builder_add_form", INT, ADDRESS, ADDRESS, LONG, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_builder_add_tiling_pattern = function("tpdf_builder_add_tiling_pattern", INT, ADDRESS, ADDRESS, LONG, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS, INT, ADDRESS, LONG);
    static final MethodHandle tpdf_page_builder_set_bleed_box = function("tpdf_page_builder_set_bleed_box", INT, ADDRESS, DOUBLE, DOUBLE, DOUBLE, DOUBLE);
    static final MethodHandle tpdf_page_builder_encoded_text = function("tpdf_page_builder_encoded_text", INT, ADDRESS, ADDRESS, LONG, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_page_builder_set_ext_gstate = function("tpdf_page_builder_set_ext_gstate", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_page_builder_form = function("tpdf_page_builder_form", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_page_builder_set_fill_pattern = function("tpdf_page_builder_set_fill_pattern", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_page_builder_set_stroke_pattern = function("tpdf_page_builder_set_stroke_pattern", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_document_info = function("tpdf_document_info", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_document_trapped = function("tpdf_document_trapped", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_document_pdf_version = function("tpdf_document_pdf_version", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_document_page_labels = function("tpdf_document_page_labels", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_page_labels_count = function("tpdf_page_labels_count", INT, ADDRESS);
    static final MethodHandle tpdf_page_label_text = function("tpdf_page_label_text", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_page_labels_free = procedure("tpdf_page_labels_free", ADDRESS);
    static final MethodHandle tpdf_document_xmp_metadata = function("tpdf_document_xmp_metadata", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_document_outline = function("tpdf_document_outline", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_outline_count = function("tpdf_outline_count", INT, ADDRESS);
    static final MethodHandle tpdf_outline_item = function("tpdf_outline_item", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_outline_title = function("tpdf_outline_title", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_outline_destination = function("tpdf_outline_destination", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_outline_destination_bytes = function("tpdf_outline_destination_bytes", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_outline_free = procedure("tpdf_outline_free", ADDRESS);
    static final MethodHandle tpdf_page_links = function("tpdf_page_links", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_links_count = function("tpdf_links_count", INT, ADDRESS);
    static final MethodHandle tpdf_link_rect = function("tpdf_link_rect", INT, ADDRESS, INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_link_reference = function("tpdf_link_reference", INT, ADDRESS, INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_link_action = function("tpdf_link_action", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_link_action_bytes = function("tpdf_link_action_bytes", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_link_destination_bytes = function("tpdf_link_destination_bytes", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_links_free = procedure("tpdf_links_free", ADDRESS);
    static final MethodHandle tpdf_document_attachments = function("tpdf_document_attachments", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_attachments_count = function("tpdf_attachments_count", INT, ADDRESS);
    static final MethodHandle tpdf_attachment_name = function("tpdf_attachment_name", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_attachment_filename = function("tpdf_attachment_filename", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_attachment_description = function("tpdf_attachment_description", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_attachment_size = function("tpdf_attachment_size", INT, ADDRESS, INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_attachment_data = function("tpdf_attachment_data", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_attachments_free = procedure("tpdf_attachments_free", ADDRESS);
    static final MethodHandle tpdf_document_warnings = function("tpdf_document_warnings", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_warnings_count = function("tpdf_warnings_count", INT, ADDRESS);
    static final MethodHandle tpdf_warning_location = function("tpdf_warning_location", INT, ADDRESS, INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_warning_kind = function("tpdf_warning_kind", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_warning_message = function("tpdf_warning_message", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_warnings_free = procedure("tpdf_warnings_free", ADDRESS);
    static final MethodHandle tpdf_tag_new = function("tpdf_tag_new", INT, ADDRESS, LONG, ADDRESS);
    static final MethodHandle tpdf_tag_set_text = function("tpdf_tag_set_text", INT, ADDRESS, INT, ADDRESS);
    static final MethodHandle tpdf_tag_set_id = function("tpdf_tag_set_id", INT, ADDRESS, ADDRESS, LONG);
    static final MethodHandle tpdf_tag_set_key = function("tpdf_tag_set_key", INT, ADDRESS, LONG, LONG);
    static final MethodHandle tpdf_tag_keep_empty = function("tpdf_tag_keep_empty", INT, ADDRESS);
    static final MethodHandle tpdf_tag_free = procedure("tpdf_tag_free", ADDRESS);
    static final MethodHandle tpdf_page_builder_open_tag = function("tpdf_page_builder_open_tag", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_page_builder_close_tag = function("tpdf_page_builder_close_tag", INT, ADDRESS);
    static final MethodHandle tpdf_builder_set_language = function("tpdf_builder_set_language", INT, ADDRESS, ADDRESS);
    static final MethodHandle tpdf_builder_map_role = function("tpdf_builder_map_role", INT, ADDRESS, ADDRESS, LONG, ADDRESS, LONG);

    // ---- Helpers. They move bytes and raise on a status; that is all. ----

    /** One call, its checked exceptions folded into unchecked ones. */
    static Object call(MethodHandle handle, Object... arguments) {
        try {
            return handle.invokeWithArguments(arguments);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw new IllegalStateException(t);
        }
    }

    static int callInt(MethodHandle handle, Object... arguments) {
        return (int) call(handle, arguments);
    }

    static long callLong(MethodHandle handle, Object... arguments) {
        return (long) call(handle, arguments);
    }

    static MemorySegment callAddress(MethodHandle handle, Object... arguments) {
        return (MemorySegment) call(handle, arguments);
    }

    /**
     * Runs a call that returns a {@code TpdfStatus} and throws on anything but
     * Ok, with the engine's own message. The message is per thread and read
     * straight after, on the same thread: a platform thread is an OS thread,
     * and a virtual thread does not unmount between two calls with nothing
     * blocking between them.
     */
    static void check(MethodHandle handle, Object... arguments) {
        int status = callInt(handle, arguments);
        if (status != 0) {
            MemorySegment message = callAddress(tpdf_last_error_message);
            throw new TinkerPdfException(status,
                    isNull(message) ? "tinker-pdf error " + status : readCString(message));
        }
    }

    static boolean isNull(MemorySegment pointer) {
        return pointer.address() == 0;
    }

    /** A NUL-terminated UTF-8 copy for a {@code const char *}. */
    static MemorySegment cString(Arena arena, String text) {
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        MemorySegment segment = arena.allocate(bytes.length + 1L);
        MemorySegment.copy(bytes, 0, segment, BYTE, 0, bytes.length);
        segment.set(BYTE, bytes.length, (byte) 0);
        return segment;
    }

    /** A nullable {@code const char *}. */
    static MemorySegment cStringOrNull(Arena arena, String text) {
        return text == null ? MemorySegment.NULL : cString(arena, text);
    }

    /**
     * A copy of bytes for a pointer-and-length pair. Never null: the C ABI
     * refuses a null pointer even with a zero length, so an empty array points
     * at one byte that is never read.
     */
    static MemorySegment bytes(Arena arena, byte[] data) {
        MemorySegment segment = arena.allocate(Math.max(1, data.length));
        MemorySegment.copy(data, 0, segment, BYTE, 0, data.length);
        return segment;
    }

    static String readCString(MemorySegment pointer) {
        MemorySegment text = pointer.reinterpret(Long.MAX_VALUE);
        long length = 0;
        while (text.get(BYTE, length) != 0) {
            length++;
        }
        return new String(text.asSlice(0, length).toArray(BYTE), StandardCharsets.UTF_8);
    }

    /** An engine-allocated string from an out slot, copied and freed; null for null. */
    static String takeString(MemorySegment slot) {
        MemorySegment pointer = slot.get(ADDRESS, 0);
        if (isNull(pointer)) {
            return null;
        }
        String text = readCString(pointer);
        call(tpdf_string_free, pointer);
        return text;
    }

    /** Bytes the engine lends through a data slot and a length slot, copied; null for null. */
    static byte[] borrowed(MemorySegment dataSlot, MemorySegment lengthSlot) {
        MemorySegment pointer = dataSlot.get(ADDRESS, 0);
        if (isNull(pointer)) {
            return null;
        }
        return pointer.reinterpret(lengthSlot.get(LONG, 0)).toArray(BYTE);
    }

    /** A {@code TpdfBuffer}, copied and freed; null for a null buffer. */
    static byte[] takeBuffer(MemorySegment buffer) {
        if (isNull(buffer)) {
            return null;
        }
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment length = arena.allocate(LONG);
            MemorySegment data = callAddress(tpdf_buffer_data, buffer, length);
            return isNull(data) ? new byte[0] : data.reinterpret(callLong(tpdf_buffer_len, buffer)).toArray(BYTE);
        } finally {
            call(tpdf_buffer_free, buffer);
        }
    }

    /** A zeroed slot for one out-parameter. */
    static MemorySegment slot(Arena arena, MemoryLayout layout) {
        MemorySegment segment = arena.allocate(layout);
        segment.fill((byte) 0);
        return segment;
    }

    static MemorySegment slot(Arena arena, long size) {
        MemorySegment segment = arena.allocate(size, 8);
        segment.fill((byte) 0);
        return segment;
    }

    static int flag(boolean value) {
        return value ? 1 : 0;
    }

    /** NaN for null: the C ABI's spelling of a destination's {@code null}. */
    static double nanFor(Double value) {
        return value == null ? Double.NaN : value;
    }

    static Double nullable(double value) {
        return Double.isNaN(value) ? null : value;
    }
}
