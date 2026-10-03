# frozen_string_literal: true

# The Ruby binding: Fiddle, Ruby's own standard library, over the C ABI in
# crates/tinker-pdf-ffi, whose header crates/tinker-pdf-ffi/include/tinker_pdf.h
# every declaration below is transcribed from.
#
# Ruling 11 is the whole design: the facade is the only public surface, and a
# binding projects it and adds no logic, caching or defaults of its own. Every
# method here is one C call, or a loop of them over a list the engine hands
# back, and nothing else. Scope and packaging: docs/features/bindings.md.
#
# The library is loaded from TINKER_PDF_LIB when that is set, and otherwise
# from target/release in this checkout (cargo build -p tinker-pdf-ffi
# --release). Structs cross as packed strings whose layouts are the ones the C
# crate pins by offset in its own tests, so a field moved there fails there.
#
# Ownership is the C ABI's: every handle the engine allocates is released by
# its #close, which is safe to call twice, and every String handed back is a
# Ruby copy that outlives its handle.

require 'fiddle'
require 'fiddle/import'
require 'rbconfig'

module TinkerPdf
  # A failure the engine reported: its status, which a caller branches on,
  # and the engine's own sentence, which names the call and the argument.
  class Error < StandardError
    attr_reader :status

    def initialize(status, message)
      @status = status
      super("#{message} (status #{status})")
    end
  end

  # TpdfStatus, transcribed. Append only; the C crate pins every number.
  module Status
    OK = 0
    BAD_ARGUMENT = 1
    NOT_A_PDF = 2
    NEEDS_PASSWORD = 3
    WRONG_PASSWORD = 4
    NO_SUCH_PAGE = 5
    NOT_ENCRYPTED = 6
    UNSUPPORTED_HANDLER = 7
    NO_SUCH_SIGNATURE = 8
    NO_SUCH_FIELD = 9
    VALUE_REFUSED = 10
    FIELD_UNREADABLE = 11
    SPENT_HANDLE = 12
    EDIT_REFUSED = 13
    SOURCE_MISS = 14
    SCRIPT_REFUSED = 15
    STREAM_UNREADABLE = 16
  end

  # The C declarations, one per line, as the header spells them with enums as
  # `int` and handles as `void *`.
  module Native
    extend Fiddle::Importer

    def self.library_path
      return ENV['TINKER_PDF_LIB'] if ENV['TINKER_PDF_LIB']

      name = case RbConfig::CONFIG['host_os']
             when /darwin/ then 'libtinker_pdf_ffi.dylib'
             when /mswin|mingw/ then 'tinker_pdf_ffi.dll'
             else 'libtinker_pdf_ffi.so'
             end
      File.expand_path("../../../target/release/#{name}", __dir__)
    end

    dlload library_path

    extern 'const char *tpdf_last_error_message(void)'
    extern 'const char *tpdf_version(void)'
    extern 'void tpdf_string_free(char *text)'
    extern 'int tpdf_document_open(const uint8_t *bytes, size_t len, void **out)'
    extern 'void tpdf_document_free(void *doc)'
    extern 'uint32_t tpdf_document_page_count(const void *doc)'
    extern 'int tpdf_page_text(const void *doc, uint32_t index, char **out)'
    extern 'int tpdf_document_set_fonts(void *doc, const uint8_t *regular, size_t regular_len, ' \
           'const uint8_t *bold, size_t bold_len, const uint8_t *italic, size_t italic_len, ' \
           'const uint8_t *bold_italic, size_t bold_italic_len)'
    extern 'int tpdf_page_render(const void *doc, uint32_t index, double scale, int format, void **out)'
    extern 'uint32_t tpdf_bitmap_width(const void *bitmap)'
    extern 'uint32_t tpdf_bitmap_height(const void *bitmap)'
    extern 'size_t tpdf_bitmap_stride(const void *bitmap)'
    extern 'const uint8_t *tpdf_bitmap_data(const void *bitmap, size_t *out_len)'
    extern 'void tpdf_bitmap_free(void *bitmap)'
    extern 'int tpdf_document_validate(const void *doc, void **out)'
    extern 'uint32_t tpdf_defects_count(const void *defects)'
    extern 'int tpdf_defect_rule(const void *defects, uint32_t index, char **out)'
    extern 'void tpdf_defects_free(void *defects)'

    extern 'int tpdf_write_options_init(void *out)'
    extern 'int tpdf_document_editor(const void *doc, void **out)'
    extern 'void tpdf_editor_free(void *editor)'
    extern 'int tpdf_editor_fill_field(void *editor, const char *name, const char *value, void **out_report)'
    extern 'int tpdf_editor_set_checkbox(void *editor, const char *name, int on)'
    extern 'int tpdf_editor_select_radio(void *editor, const char *name, const char *option)'
    extern 'int tpdf_editor_save(const void *editor, const void *options, void **out)'
    extern 'const uint8_t *tpdf_buffer_data(const void *buffer, size_t *out_len)'
    extern 'void tpdf_buffer_free(void *buffer)'
    extern 'uint32_t tpdf_fill_report_count(const void *report)'
    extern 'int tpdf_fill_report_message(const void *report, uint32_t index, char **out)'
    extern 'int tpdf_fill_report_widget(const void *report, uint32_t index, uint32_t *out_number, uint16_t *out_generation)'
    extern 'void tpdf_fill_report_free(void *report)'

    extern 'int tpdf_builder_new(void **out)'
    extern 'void tpdf_builder_free(void *builder)'
    extern 'int tpdf_builder_add_base_font(void *builder, const uint8_t *resource, size_t resource_len, ' \
           'const uint8_t *base_font, size_t base_font_len)'
    extern 'int tpdf_builder_add_image(void *builder, const uint8_t *resource, size_t resource_len, const void *image)'
    extern 'int tpdf_builder_set_info(void *builder, const uint8_t *key, size_t key_len, const char *value)'
    extern 'int tpdf_builder_begin_page(void *builder, double width, double height, void **out)'
    extern 'int tpdf_builder_push_page(void *builder, void *page)'
    extern 'int tpdf_builder_set_outline(void *builder, void *entries, size_t count)'
    extern 'int tpdf_builder_finish(void *builder, void **out)'
    extern 'void tpdf_page_builder_free(void *page)'
    extern 'int tpdf_page_builder_text(void *page, const uint8_t *font, size_t font_len, double size, ' \
           'double x, double y, const char *text)'
    extern 'int tpdf_page_builder_fill_rect(void *page, double x, double y, double w, double h, double grey)'
    extern 'int tpdf_page_builder_image(void *page, const uint8_t *resource, size_t resource_len, ' \
           'double x, double y, double w, double h)'
    extern 'int tpdf_page_builder_link(void *page, double x0, double y0, double x1, double y1, const void *target)'
    extern 'int tpdf_outline_entry_new(const char *title, void **out)'
    extern 'int tpdf_outline_entry_set_target(void *entry, const void *target)'
    extern 'int tpdf_outline_entry_set_open(void *entry, int open)'
    extern 'int tpdf_outline_entry_add_child(void *parent, void *child)'
    extern 'void tpdf_outline_entry_free(void *entry)'

    extern 'int tpdf_editor_set_page_labels(void *editor, const void *ranges, size_t count)'
    extern 'int tpdf_editor_attach_file(void *editor, const void *file, uint32_t *out_object, uint16_t *out_generation)'
    extern 'int tpdf_editor_set_outline(void *editor, void *entries, size_t count)'
    extern 'int tpdf_editor_set_info(void *editor, int key, const char *value, int *out_sync)'
    extern 'int tpdf_editor_set_info_date(void *editor, int key, const void *date, int *out_sync)'
    extern 'int tpdf_editor_set_trapped(void *editor, int trapped, int *out_sync)'
    extern 'int tpdf_editor_set_xmp_metadata(void *editor, const uint8_t *data, size_t len, int *out_sync)'
    extern 'int tpdf_editor_set_page_boundary(void *editor, uint32_t index, int boundary, ' \
           'double x0, double y0, double x1, double y1)'
    extern 'int tpdf_page_boundary(const void *doc, uint32_t index, int boundary, ' \
           'double *x0, double *y0, double *x1, double *y1)'
    extern 'int tpdf_editor_sanitise(void *editor, const void *what, void **out)'
    extern 'uint32_t tpdf_sanitise_report_count(const void *report, int list)'
    extern 'int tpdf_sanitise_report_entry(const void *report, int list, uint32_t index, int *out_what, ' \
           'int *out_has_object, uint32_t *out_object, uint16_t *out_generation)'
    extern 'int tpdf_sanitise_report_action(const void *report, int list, uint32_t index, void **out_data, size_t *out_len)'
    extern 'uint32_t tpdf_sanitise_report_path_count(const void *report, uint32_t index)'
    extern 'int tpdf_sanitise_report_path_step(const void *report, uint32_t index, uint32_t step, int *out_is_index, ' \
           'uint64_t *out_position, void **out_key_data, size_t *out_key_len)'
    extern 'void tpdf_sanitise_report_free(void *report)'

    extern 'int tpdf_document_info(const void *doc, int key, char **out)'
    extern 'int tpdf_document_trapped(const void *doc, int *out)'
    extern 'int tpdf_document_pdf_version(const void *doc, char **out)'
    extern 'int tpdf_document_page_label(const void *doc, uint32_t index, char **out)'
    extern 'int tpdf_document_xmp_metadata(const void *doc, void **out)'
    extern 'int tpdf_document_outline(const void *doc, void **out)'
    extern 'uint32_t tpdf_outline_count(const void *outline)'
    extern 'int tpdf_outline_item(const void *outline, uint32_t index, uint32_t *out_depth, int *out_open)'
    extern 'int tpdf_outline_title(const void *outline, uint32_t index, char **out)'
    extern 'int tpdf_outline_destination(const void *outline, uint32_t index, void *out)'
    extern 'int tpdf_outline_destination_bytes(const void *outline, uint32_t index, void **out_data, size_t *out_len)'
    extern 'void tpdf_outline_free(void *outline)'
    extern 'int tpdf_page_links(const void *doc, uint32_t index, void **out)'
    extern 'uint32_t tpdf_links_count(const void *links)'
    extern 'int tpdf_link_rect(const void *links, uint32_t index, double *x0, double *y0, double *x1, double *y1)'
    extern 'int tpdf_link_reference(const void *links, uint32_t index, int *out_present, ' \
           'uint32_t *out_object, uint16_t *out_generation)'
    extern 'int tpdf_link_action(const void *links, uint32_t index, int *out_kind, void *out_destination)'
    extern 'int tpdf_link_action_bytes(const void *links, uint32_t index, void **out_data, size_t *out_len)'
    extern 'int tpdf_link_destination_bytes(const void *links, uint32_t index, void **out_data, size_t *out_len)'
    extern 'void tpdf_links_free(void *links)'
    extern 'int tpdf_document_attachments(const void *doc, void **out)'
    extern 'uint32_t tpdf_attachments_count(const void *attachments)'
    extern 'int tpdf_attachment_name(const void *attachments, uint32_t index, char **out)'
    extern 'int tpdf_attachment_filename(const void *attachments, uint32_t index, char **out)'
    extern 'int tpdf_attachment_description(const void *attachments, uint32_t index, char **out)'
    extern 'int tpdf_attachment_size(const void *attachments, uint32_t index, int *out_present, int64_t *out_size)'
    extern 'int tpdf_attachment_data(const void *attachments, uint32_t index, void **out)'
    extern 'void tpdf_attachments_free(void *attachments)'
    extern 'int tpdf_document_warnings(const void *doc, void **out)'
    extern 'uint32_t tpdf_warnings_count(const void *warnings)'
    extern 'int tpdf_warning_location(const void *warnings, uint32_t index, uint64_t *out_offset, ' \
           'int *out_has_object, uint32_t *out_object, uint16_t *out_generation)'
    extern 'int tpdf_warning_kind(const void *warnings, uint32_t index, char **out)'
    extern 'int tpdf_warning_message(const void *warnings, uint32_t index, char **out)'
    extern 'void tpdf_warnings_free(void *warnings)'

    extern 'int tpdf_document_signatures(const void *doc, void **out)'
    extern 'uint32_t tpdf_signatures_count(const void *signatures)'
    extern 'void tpdf_signatures_free(void *signatures)'
    extern 'int tpdf_signature_field_name(const void *signatures, uint32_t index, char **out)'
    extern 'int tpdf_signature_sub_filter(const void *signatures, uint32_t index, char **out)'
    extern 'int tpdf_signature_reason(const void *signatures, uint32_t index, char **out)'
    extern 'int tpdf_signature_location(const void *signatures, uint32_t index, char **out)'
    extern 'int tpdf_signature_name(const void *signatures, uint32_t index, char **out)'
    extern 'int tpdf_signature_coverage(const void *signatures, uint32_t index, int *out)'
    extern 'int tpdf_signature_covers_whole_file(const void *signatures, uint32_t index)'
    extern 'int tpdf_signature_is_usage_rights(const void *signatures, uint32_t index)'
    extern 'uint32_t tpdf_signature_certification_level(const void *signatures, uint32_t index)'
    extern 'uint32_t tpdf_signature_span_count(const void *signatures, uint32_t index)'
    extern 'int tpdf_signature_span(const void *signatures, uint32_t index, uint32_t span, ' \
           'uint64_t *out_start, uint64_t *out_length)'
    extern 'void *tpdf_trust_anchors_new(void)'
    extern 'int tpdf_trust_anchors_add(void *anchors, const uint8_t *der, size_t len)'
    extern 'uint32_t tpdf_trust_anchors_count(const void *anchors)'
    extern 'void tpdf_trust_anchors_free(void *anchors)'
    extern 'int tpdf_document_verify_signatures(const void *doc, const void *anchors, int judge_validity, ' \
           'int64_t at, void **out)'
    extern 'uint32_t tpdf_verdicts_count(const void *verdicts)'
    extern 'void tpdf_verdicts_free(void *verdicts)'
    extern 'int tpdf_verdict_cms_state(const void *verdicts, uint32_t index, int *out)'
    extern 'int tpdf_verdict_document_digest(const void *verdicts, uint32_t index, int *out)'
    extern 'int tpdf_verdict_signature_check(const void *verdicts, uint32_t index, int *out)'
    extern 'int tpdf_verdict_chain(const void *verdicts, uint32_t index, int *out)'
    extern 'int tpdf_verdict_signer_subject(const void *verdicts, uint32_t index, char **out)'
    extern 'int tpdf_verdict_signer_issuer(const void *verdicts, uint32_t index, char **out)'
    extern 'int tpdf_verdict_signer_validity(const void *verdicts, uint32_t index, int64_t *out_not_before, ' \
           'int64_t *out_not_after)'
    extern 'uint32_t tpdf_verdict_weakness_count(const void *verdicts, uint32_t index)'
    extern 'int tpdf_verdict_weakness(const void *verdicts, uint32_t index, uint32_t weakness, int *out)'

    extern 'int tpdf_document_is_streamed(const void *doc)'
    extern 'int tpdf_document_is_encrypted(const void *doc)'
    extern 'int tpdf_document_authenticate(void *doc, const char *password, int *out_level)'
    extern 'int tpdf_document_may_print(const void *doc)'
    extern 'int tpdf_page_size(const void *doc, uint32_t index, double *out_width, double *out_height)'
    extern 'int tpdf_defect_message(const void *defects, uint32_t index, char **out)'
    extern 'int tpdf_editor_is_dirty(const void *editor)'
    extern 'uint32_t tpdf_editor_page_count(const void *editor)'
    extern 'int tpdf_editor_delete_page(void *editor, uint32_t index)'
    extern 'int tpdf_editor_move_page(void *editor, uint32_t from, uint32_t to)'
    extern 'int tpdf_editor_rotate_page(void *editor, uint32_t index, int64_t degrees)'
    extern 'int tpdf_editor_insert_page(void *editor, uint32_t index, double width, double height)'
    extern 'int tpdf_editor_set_crop_box(void *editor, uint32_t index, double x0, double y0, double x1, double y1)'
    extern 'int tpdf_editor_append_content(void *editor, uint32_t page, const uint8_t *operators, size_t len)'
    extern 'uint32_t tpdf_editor_field_count(const void *editor)'
    extern 'int tpdf_editor_field_name(const void *editor, uint32_t index, char **out)'
    extern 'int tpdf_editor_field_value(const void *editor, uint32_t index, char **out)'
    extern 'int tpdf_editor_checkpoint(const void *editor, void **out)'
    extern 'int tpdf_editor_restore(void *editor, const void *checkpoint)'
    extern 'void tpdf_checkpoint_free(void *checkpoint)'
    extern 'size_t tpdf_buffer_len(const void *buffer)'
    extern 'int tpdf_fill_report_defect(const void *report, uint32_t index, int *out)'
    extern 'int tpdf_destination_init_fit(void *out)'
    extern 'int tpdf_builder_add_embedded_font(void *builder, const uint8_t *resource, size_t resource_len, ' \
           'const uint8_t *base_font, size_t base_font_len, const uint8_t *program, size_t program_len)'
    extern 'int tpdf_builder_set_subset_fonts(void *builder, int subset)'
    extern 'int tpdf_editor_recalculate(void *editor, uint32_t policy, void **out_report)'
    extern 'void tpdf_recalculation_free(void *report)'
    extern 'uint32_t tpdf_recalculation_changed_count(const void *report)'
    extern 'int tpdf_recalculation_changed_name(const void *report, uint32_t index, char **out)'
    extern 'int tpdf_recalculation_changed_value(const void *report, uint32_t index, char **out)'
    extern 'uint32_t tpdf_recalculation_skipped_count(const void *report)'
    extern 'uint32_t tpdf_recalculation_cascades_cut_count(const void *report)'
    extern 'int tpdf_recalculation_cascades_cut_name(const void *report, uint32_t index, char **out)'
    extern 'uint32_t tpdf_recalculation_refused_count(const void *report)'
    extern 'int tpdf_recalculation_refused_name(const void *report, uint32_t index, char **out)'
    extern 'int tpdf_editor_formatted_value(const void *editor, const char *name, uint32_t policy, char **out)'
    extern 'int tpdf_editor_keystroke(const void *editor, const char *name, const char *change, int64_t sel_start, ' \
           'int64_t sel_end, int will_commit, uint32_t policy, int *out_accepted, char **out_change)'
    extern 'int tpdf_editor_validate(const void *editor, const char *name, const char *value, uint32_t policy, ' \
           'int *out_accepted, char **out_value)'
    extern 'int tpdf_page_builder_set_fill_rgb(void *page, double r, double g, double b)'
    extern 'int tpdf_page_builder_set_stroke_rgb(void *page, double r, double g, double b)'
    extern 'int tpdf_page_builder_set_crop_box(void *page, double x0, double y0, double x1, double y1)'
    extern 'int tpdf_page_builder_raw(void *page, const uint8_t *operators, size_t len)'
  end

  # The TPDF_SCRIPT_* policy bits and TPDF_ENTROPY_LEN, transcribed.
  module Script
    CALCULATE = 1
    FORMAT = 1 << 1
    KEYSTROKE = 1 << 2
    VALIDATE = 1 << 3
    DOCUMENT = 1 << 4
    CATALOG = 1 << 5
    DEFAULT = CALCULATE | FORMAT
  end

  ENTROPY_LEN = 48

  # Byte-level helpers. Nothing here decides anything; it moves bytes.
  module Raw
    module_function

    def slot(size = 8)
      Fiddle::Pointer.malloc(size, Fiddle::RUBY_FREE)
    end

    def u16(pointer) = pointer[0, 2].unpack1('S')
    def u32(pointer) = pointer[0, 4].unpack1('L')
    def i32(pointer) = pointer[0, 4].unpack1('l')
    def u64(pointer) = pointer[0, 8].unpack1('Q')
    def i64(pointer) = pointer[0, 8].unpack1('q')
    def f64(pointer) = pointer[0, 8].unpack1('d')

    # Runs a call and raises Error on a non-Ok status, with the engine's own
    # message, read on the same thread before anything else can replace it.
    def check(status)
      return if status.zero?

      pointer = Native.tpdf_last_error_message
      message = pointer.null? ? "tinker-pdf error #{status}" : pointer.to_s.force_encoding(Encoding::UTF_8)
      raise Error.new(status, message)
    end

    # A NUL-terminated UTF-8 copy for a `const char *`.
    def cstr(text)
      "#{text.encode(Encoding::UTF_8)}\0".b
    end

    # An engine-allocated string, copied and freed; nil for null.
    def take_string(slot)
      pointer = slot.ptr
      return nil if pointer.null?

      text = pointer.to_s.force_encoding(Encoding::UTF_8)
      Native.tpdf_string_free(pointer)
      text
    end

    # Bytes the engine lends, copied; nil for null.
    def borrowed(data_slot, length_slot)
      pointer = data_slot.ptr
      return nil if pointer.null?

      pointer[0, u64(length_slot)]
    end

    # A TpdfBuffer, copied and freed; nil for a null buffer.
    def take_buffer(buffer)
      return nil if buffer.null?

      length = slot
      data = Native.tpdf_buffer_data(buffer, length)
      bytes = data.null? ? ''.b : data[0, Native.tpdf_buffer_len(buffer)]
      Native.tpdf_buffer_free(buffer)
      bytes
    end

    # An out-pointer call: the handle the engine wrote.
    def handle
      out = slot
      check(yield(out))
      out.ptr
    end

    # A string the engine wrote, or nil for "the document does not say".
    def text
      out = slot
      check(yield(out))
      take_string(out)
    end

    # The address of a String's bytes for a struct field. The caller keeps the
    # String alive for the length of the call.
    def address(bytes)
      bytes.nil? ? 0 : Fiddle::Pointer[bytes].to_i
    end

    # A NaN-for-null number, the C ABI's spelling of `null` in a view.
    def nan_for(value)
      value.nil? ? Float::NAN : value.to_f
    end

    def nullable(value)
      value.nan? ? nil : value
    end
  end

  # The pixel formats, transcribed from TpdfPixelFormat.
  module PixelFormat
    GRAY8 = 0
    GRAY_A8 = 1
    RGB8 = 2
    RGBA8 = 3
  end

  # The destination kinds, transcribed from TpdfDestKind (12.3.2.2).
  module DestKind
    XYZ = 0
    FIT = 1
    FIT_H = 2
    FIT_V = 3
    FIT_R = 4
    FIT_B = 5
    FIT_BH = 6
    FIT_BV = 7
  end

  # The remaining enums, transcribed from the header in its order. Each is
  # the C numbering, which the C crate pins, and nothing else.
  module AuthLevel
    NONE = 0
    USER = 1
    OWNER = 2
  end

  module Coverage
    WHOLE_FILE = 0
    REVISION = 1
    SUSPICIOUS = 2
  end

  module CmsState
    READ = 0
    ABSENT = 1
    UNREADABLE = 2
  end

  module DocumentDigest
    MATCHES = 0
    DIFFERS = 1
    NOT_CHECKED = 2
  end

  module SignatureCheck
    VERIFIED = 0
    FAILED = 1
    NOT_CHECKED = 2
  end

  module Chain
    ANCHORED_TO = 0
    SELF_SIGNED = 1
    INCOMPLETE = 2
    BROKEN = 3
    NO_ANCHORS = 4
    NO_SIGNER_CERTIFICATE = 5
  end

  module Weakness
    SHA1_DIGEST = 0
    SHA1_SIGNATURE = 1
    SHORT_RSA_KEY = 2
    COVERS_ONLY_A_REVISION = 3
    COVERAGE_SUSPICIOUS = 4
    OUTSIDE_VALIDITY = 5
  end

  module WidgetDefect
    RECT_MISSING = 0
  end

  module LabelStyle
    DECIMAL = 0
    ROMAN_UPPER = 1
    ROMAN_LOWER = 2
    LETTERS_UPPER = 3
    LETTERS_LOWER = 4
    NONE = 5
  end

  module InfoKey
    TITLE = 0
    AUTHOR = 1
    SUBJECT = 2
    KEYWORDS = 3
    CREATOR = 4
    PRODUCER = 5
    CREATION_DATE = 6
    MODIFICATION_DATE = 7
  end

  module MetadataSync
    ALONE = 0
    OTHER_HALF_UNCHANGED = 1
  end

  module Trapped
    ABSENT = 0
    TRUE = 1
    FALSE = 2
    UNKNOWN = 3
  end

  module PageBoundary
    MEDIA_BOX = 0
    CROP_BOX = 1
    BLEED_BOX = 2
    TRIM_BOX = 3
    ART_BOX = 4
  end

  module Removal
    JAVA_SCRIPT = 0
    DOCUMENT_JAVA_SCRIPT = 1
    CALCULATION_ORDER = 2
    XFA_FORM = 3
    ACTION = 4
    EMBEDDED_FILE_TREE = 5
    EMBEDDED_FILE = 6
    INFO = 7
    METADATA = 8
  end

  module DestinationKind
    ABSENT = 0
    EXPLICIT = 1
    NAMED = 2
    URI = 3
  end

  module ActionKind
    ABSENT = 0
    GO_TO = 1
    GO_TO_R = 2
    URI = 3
    NAMED = 4
    LAUNCH = 5
    OTHER = 6
  end

  # A view; a nil number is the file's null, "retain the current value".
  View = Struct.new(:kind, :left, :bottom, :right, :top, :zoom, keyword_init: true) do
    def pack
      [kind, Raw.nan_for(left), Raw.nan_for(bottom), Raw.nan_for(right), Raw.nan_for(top), Raw.nan_for(zoom)]
    end

    # The engine's own `/Fit` view, from tpdf_destination_init_fit.
    # TpdfDestination: kind i32 @0, then five doubles from @8; 48 bytes.
    def self.fit
      raw = Raw.slot(48)
      Raw.check(Native.tpdf_destination_init_fit(raw))
      kind, *numbers = raw[0, 48].unpack('lx4d5')
      new(kind: kind, left: Raw.nullable(numbers[0]), bottom: Raw.nullable(numbers[1]),
          right: Raw.nullable(numbers[2]), top: Raw.nullable(numbers[3]), zoom: Raw.nullable(numbers[4]))
    end
  end

  # How to encrypt on save. entropy is exactly ENTROPY_LEN caller-supplied
  # bytes; there is no default, because the engine has no opinion about
  # where randomness comes from.
  Encryption = Struct.new(:user_password, :owner_password, :permissions, :entropy, keyword_init: true)

  # How to save, field for field TpdfWriteOptions. Start from
  # TinkerPdf.write_options, the engine's defaults, and change what you mean.
  WriteOptions = Struct.new(:mode, :linearize, :version_major, :version_minor, :object_streams, :compress,
                            :garbage_collect, :encryption, keyword_init: true) do
    # TpdfWriteOptions: mode @0, linearize @4, version_major @8,
    # version_minor @12, object_streams @16, compress @20,
    # garbage_collect @24, encryption pointer @32; 40 bytes. TpdfEncryption:
    # user_password @0, owner_password @8, permissions @16, entropy @24,
    # entropy_len @32; 40 bytes. Every String packed is kept alive in the
    # block's frame for the length of the call.
    def with_raw
      kept = []
      if encryption
        kept = [encryption.user_password && Raw.cstr(encryption.user_password),
                encryption.owner_password && Raw.cstr(encryption.owner_password), encryption.entropy&.b]
        kept << [Raw.address(kept[0]), Raw.address(kept[1]), encryption.permissions || 0, Raw.address(kept[2]),
                 kept[2]&.bytesize || 0].pack('QQlx4QQ')
      end
      raw = [mode, flag(linearize), version_major, version_minor, flag(object_streams), flag(compress),
             flag(garbage_collect), encryption ? Raw.address(kept.last) : 0].pack('llLLlllx4Q')
      yield raw
    end

    private

    def flag(value) = value ? 1 : 0
  end

  def self.write_options
    raw = Raw.slot(40)
    Raw.check(Native.tpdf_write_options_init(raw))
    mode, linearize, major, minor, object_streams, compress, garbage_collect = raw[0, 28].unpack('llLLlll')
    WriteOptions.new(mode: mode, linearize: !linearize.zero?, version_major: major, version_minor: minor,
                     object_streams: !object_streams.zero?, compress: !compress.zero?,
                     garbage_collect: !garbage_collect.zero?, encryption: nil)
  end

  # Where a link or outline entry goes: a page with a view, or a URI.
  Target = Struct.new(:page, :view, :uri, keyword_init: true) do
    # TpdfTarget: kind i32 @0, page_index u32 @4, view @8 (kind i32, pad,
    # five doubles), uri pointer @56; 64 bytes.
    def with_raw
      uri_bytes = uri && Raw.cstr(uri)
      raw = [uri ? 1 : 0, page || 0, *(view || View.fit).pack, Raw.address(uri_bytes)]
            .pack('lLlx4d5Q')
      yield raw
    end
  end

  # A rendered page.
  class Bitmap
    def initialize(pointer)
      @pointer = pointer
    end

    def width = Native.tpdf_bitmap_width(@pointer)
    def height = Native.tpdf_bitmap_height(@pointer)
    def stride = Native.tpdf_bitmap_stride(@pointer)

    # A copy of the pixels.
    def pixels
      length = Raw.slot
      data = Native.tpdf_bitmap_data(@pointer, length)
      data.null? ? ''.b : data[0, Raw.u64(length)]
    end

    def close
      Native.tpdf_bitmap_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end
  end

  OutlineItem = Struct.new(:depth, :open, :title, :destination, keyword_init: true)
  Destination = Struct.new(:kind, :page_index, :page_ref, :view, :bytes, keyword_init: true)
  Link = Struct.new(:rect, :reference, :action, :destination, :action_bytes, keyword_init: true)
  Warning = Struct.new(:offset, :object, :kind, :message, keyword_init: true)
  SkippedWidget = Struct.new(:object, :generation, :defect, :message, keyword_init: true)
  Defect = Struct.new(:rule, :message, keyword_init: true)
  Recalculation = Struct.new(:changed, :skipped, :cascades_cut, :refused, keyword_init: true)
  Signature = Struct.new(:field_name, :sub_filter, :reason, :location, :name, :coverage,
                         :covers_whole_file, :usage_rights, :certification_level, :spans, keyword_init: true)
  Verdict = Struct.new(:cms, :document_digest, :signature, :chain, :signer_subject, :signer_issuer,
                       :signer_validity, :weaknesses, keyword_init: true)
  RemovedEntry = Struct.new(:holder, :path, :what, :action, keyword_init: true)
  DeletedObject = Struct.new(:object, :what, :action, keyword_init: true)
  SanitiseReport = Struct.new(:removed, :deleted, keyword_init: true)

  # TpdfDestinationRead: kind @0, has_page_index @4, page_index @8,
  # has_page_ref @12, page_object @16, page_generation @20, view @24; 72 bytes.
  def self.destination(raw, bytes)
    kind, has_index, index, has_ref, object, generation, view_kind, *numbers = raw.unpack('llLlLSx2lx4d5')
    return nil if kind.zero?

    Destination.new(
      kind: kind,
      page_index: has_index.zero? ? nil : index,
      page_ref: has_ref.zero? ? nil : [object, generation],
      view: View.new(kind: view_kind, left: Raw.nullable(numbers[0]), bottom: Raw.nullable(numbers[1]),
                     right: Raw.nullable(numbers[2]), top: Raw.nullable(numbers[3]),
                     zoom: Raw.nullable(numbers[4])),
      bytes: bytes
    )
  end

  # An open PDF.
  class Document
    attr_reader :pointer

    def self.open(bytes)
      new(Raw.handle { |out| Native.tpdf_document_open(bytes, bytes.bytesize, out) })
    end

    def initialize(pointer)
      @pointer = pointer
    end

    def close
      Native.tpdf_document_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end

    def page_count = Native.tpdf_document_page_count(@pointer)

    def page_text(index)
      Raw.text { |out| Native.tpdf_page_text(@pointer, index, out) }
    end

    def set_fonts(regular)
      Raw.check(Native.tpdf_document_set_fonts(@pointer, regular, regular.bytesize, nil, 0, nil, 0, nil, 0))
    end

    def render(index, scale: 1.0, format: PixelFormat::RGB8)
      Bitmap.new(Raw.handle { |out| Native.tpdf_page_render(@pointer, index, scale, format, out) })
    end

    # The strict structural validator's defects, each a rule and the
    # engine's sentence about it; empty is clean.
    def validate
      defects = Raw.handle { |out| Native.tpdf_document_validate(@pointer, out) }
      (0...Native.tpdf_defects_count(defects)).map do |i|
        Defect.new(rule: Raw.text { |out| Native.tpdf_defect_rule(defects, i, out) },
                   message: Raw.text { |out| Native.tpdf_defect_message(defects, i, out) })
      end
    ensure
      Native.tpdf_defects_free(defects) if defects
    end

    def streamed? = !Native.tpdf_document_is_streamed(@pointer).zero?
    def encrypted? = !Native.tpdf_document_is_encrypted(@pointer).zero?
    def may_print? = !Native.tpdf_document_may_print(@pointer).zero?

    # Tries a password; the AuthLevel it reached. A wrong one raises with
    # WRONG_PASSWORD, and an unencrypted document with NOT_ENCRYPTED.
    def authenticate(password)
      out = Raw.slot(4)
      Raw.check(Native.tpdf_document_authenticate(@pointer, Raw.cstr(password), out))
      Raw.i32(out)
    end

    # [width, height] in points.
    def page_size(index)
      width, height = Raw.slot, Raw.slot
      Raw.check(Native.tpdf_page_size(@pointer, index, width, height))
      [Raw.f64(width), Raw.f64(height)]
    end

    def editor
      Editor.new(Raw.handle { |out| Native.tpdf_document_editor(@pointer, out) })
    end

    # One /Info text entry; nil when absent, "" when empty.
    def info(key)
      Raw.text { |out| Native.tpdf_document_info(@pointer, key, out) }
    end

    def trapped
      out = Raw.slot(4)
      Raw.check(Native.tpdf_document_trapped(@pointer, out))
      Raw.i32(out)
    end

    def pdf_version
      Raw.text { |out| Native.tpdf_document_pdf_version(@pointer, out) }
    end

    def page_label(index)
      Raw.text { |out| Native.tpdf_document_page_label(@pointer, index, out) }
    end

    def xmp_metadata
      out = Raw.slot
      Raw.check(Native.tpdf_document_xmp_metadata(@pointer, out))
      Raw.take_buffer(out.ptr)
    end

    # The outline flattened to reading order, each item with its depth.
    def outline
      handle = Raw.handle { |out| Native.tpdf_document_outline(@pointer, out) }
      (0...Native.tpdf_outline_count(handle)).map do |i|
        depth = Raw.slot(4)
        open = Raw.slot(4)
        Raw.check(Native.tpdf_outline_item(handle, i, depth, open))
        title = Raw.text { |out| Native.tpdf_outline_title(handle, i, out) }
        raw = Raw.slot(72)
        Raw.check(Native.tpdf_outline_destination(handle, i, raw))
        data = Raw.slot
        length = Raw.slot
        Raw.check(Native.tpdf_outline_destination_bytes(handle, i, data, length))
        OutlineItem.new(depth: Raw.u32(depth), open: !Raw.i32(open).zero?, title: title,
                        destination: TinkerPdf.destination(raw[0, 72], Raw.borrowed(data, length)))
      end
    ensure
      Native.tpdf_outline_free(handle) if handle
    end

    def links(page)
      handle = Raw.handle { |out| Native.tpdf_page_links(@pointer, page, out) }
      (0...Native.tpdf_links_count(handle)).map do |i|
        x0, y0, x1, y1 = Array.new(4) { Raw.slot }
        Raw.check(Native.tpdf_link_rect(handle, i, x0, y0, x1, y1))
        present, object, generation = Raw.slot(4), Raw.slot(4), Raw.slot(2)
        Raw.check(Native.tpdf_link_reference(handle, i, present, object, generation))
        kind = Raw.slot(4)
        raw = Raw.slot(72)
        Raw.check(Native.tpdf_link_action(handle, i, kind, raw))
        action_data, action_length, dest_data, dest_length = Array.new(4) { Raw.slot }
        Raw.check(Native.tpdf_link_action_bytes(handle, i, action_data, action_length))
        Raw.check(Native.tpdf_link_destination_bytes(handle, i, dest_data, dest_length))
        Link.new(rect: [x0, y0, x1, y1].map { |p| Raw.f64(p) },
                 reference: Raw.i32(present).zero? ? nil : [Raw.u32(object), Raw.u16(generation)],
                 action: Raw.i32(kind),
                 destination: TinkerPdf.destination(raw[0, 72], Raw.borrowed(dest_data, dest_length)),
                 action_bytes: Raw.borrowed(action_data, action_length))
      end
    ensure
      Native.tpdf_links_free(handle) if handle
    end

    def attachments
      Attachments.new(Raw.handle { |out| Native.tpdf_document_attachments(@pointer, out) })
    end

    def warnings
      handle = Raw.handle { |out| Native.tpdf_document_warnings(@pointer, out) }
      (0...Native.tpdf_warnings_count(handle)).map do |i|
        offset, has, object, generation = Raw.slot, Raw.slot(4), Raw.slot(4), Raw.slot(2)
        Raw.check(Native.tpdf_warning_location(handle, i, offset, has, object, generation))
        Warning.new(offset: Raw.u64(offset),
                    object: Raw.i32(has).zero? ? nil : [Raw.u32(object), Raw.u16(generation)],
                    kind: Raw.text { |out| Native.tpdf_warning_kind(handle, i, out) },
                    message: Raw.text { |out| Native.tpdf_warning_message(handle, i, out) })
      end
    ensure
      Native.tpdf_warnings_free(handle) if handle
    end

    def page_box(index, boundary)
      x0, y0, x1, y1 = Array.new(4) { Raw.slot }
      Raw.check(Native.tpdf_page_boundary(@pointer, index, boundary, x0, y0, x1, y1))
      [x0, y0, x1, y1].map { |p| Raw.f64(p) }
    end

    def signatures
      handle = Raw.handle { |out| Native.tpdf_document_signatures(@pointer, out) }
      (0...Native.tpdf_signatures_count(handle)).map do |i|
        coverage = Raw.slot(4)
        Raw.check(Native.tpdf_signature_coverage(handle, i, coverage))
        spans = (0...Native.tpdf_signature_span_count(handle, i)).map do |s|
          start, length = Raw.slot, Raw.slot
          Raw.check(Native.tpdf_signature_span(handle, i, s, start, length))
          [Raw.u64(start), Raw.u64(length)]
        end
        Signature.new(
          field_name: Raw.text { |out| Native.tpdf_signature_field_name(handle, i, out) },
          sub_filter: Raw.text { |out| Native.tpdf_signature_sub_filter(handle, i, out) },
          reason: Raw.text { |out| Native.tpdf_signature_reason(handle, i, out) },
          location: Raw.text { |out| Native.tpdf_signature_location(handle, i, out) },
          name: Raw.text { |out| Native.tpdf_signature_name(handle, i, out) },
          coverage: Raw.i32(coverage),
          covers_whole_file: !Native.tpdf_signature_covers_whole_file(handle, i).zero?,
          usage_rights: !Native.tpdf_signature_is_usage_rights(handle, i).zero?,
          certification_level: Native.tpdf_signature_certification_level(handle, i),
          spans: spans
        )
      end
    ensure
      Native.tpdf_signatures_free(handle) if handle
    end

    # at: seconds since the Unix epoch to judge validity at, or nil to judge
    # nothing — "expired" is a claim about a moment the caller names.
    def verify_signatures(anchors, at = nil)
      handle = Raw.handle do |out|
        Native.tpdf_document_verify_signatures(@pointer, anchors.pointer, at.nil? ? 0 : 1, at || 0, out)
      end
      (0...Native.tpdf_verdicts_count(handle)).map do |i|
        answers = %i[tpdf_verdict_cms_state tpdf_verdict_document_digest tpdf_verdict_signature_check
                     tpdf_verdict_chain].map do |accessor|
          out = Raw.slot(4)
          Raw.check(Native.send(accessor, handle, i, out))
          Raw.i32(out)
        end
        not_before, not_after = Raw.slot, Raw.slot
        validity = Native.tpdf_verdict_signer_validity(handle, i, not_before, not_after)
        weaknesses = (0...Native.tpdf_verdict_weakness_count(handle, i)).map do |w|
          out = Raw.slot(4)
          Raw.check(Native.tpdf_verdict_weakness(handle, i, w, out))
          Raw.i32(out)
        end
        Verdict.new(cms: answers[0], document_digest: answers[1], signature: answers[2], chain: answers[3],
                    signer_subject: Raw.text { |out| Native.tpdf_verdict_signer_subject(handle, i, out) },
                    signer_issuer: Raw.text { |out| Native.tpdf_verdict_signer_issuer(handle, i, out) },
                    signer_validity: validity.zero? ? nil : [Raw.i64(not_before), Raw.i64(not_after)],
                    weaknesses: weaknesses)
      end
    ensure
      Native.tpdf_verdicts_free(handle) if handle
    end
  end

  # Every file attached to a document (7.11.4). Holds its own document.
  class Attachments
    def initialize(pointer)
      @pointer = pointer
    end

    def count = Native.tpdf_attachments_count(@pointer)
    def name(index) = Raw.text { |out| Native.tpdf_attachment_name(@pointer, index, out) }
    def filename(index) = Raw.text { |out| Native.tpdf_attachment_filename(@pointer, index, out) }
    def description(index) = Raw.text { |out| Native.tpdf_attachment_description(@pointer, index, out) }

    def size(index)
      present, size = Raw.slot(4), Raw.slot
      Raw.check(Native.tpdf_attachment_size(@pointer, index, present, size))
      Raw.i32(present).zero? ? nil : Raw.i64(size)
    end

    # The bytes, decoded; nil when no stream is named. A named stream that
    # does not read raises with STREAM_UNREADABLE.
    def data(index)
      out = Raw.slot
      Raw.check(Native.tpdf_attachment_data(@pointer, index, out))
      Raw.take_buffer(out.ptr)
    end

    def close
      Native.tpdf_attachments_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end
  end

  # Certificates the caller trusts, as DER; there is no default root store.
  class TrustAnchors
    attr_reader :pointer

    def initialize
      @pointer = Native.tpdf_trust_anchors_new
    end

    def add(der) = Raw.check(Native.tpdf_trust_anchors_add(@pointer, der, der.bytesize))
    def count = Native.tpdf_trust_anchors_count(@pointer)

    def close
      Native.tpdf_trust_anchors_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end
  end

  # One outline entry under construction; add_child and set_outline consume.
  class OutlineEntry
    attr_reader :pointer

    def initialize(title)
      @pointer = Raw.handle { |out| Native.tpdf_outline_entry_new(Raw.cstr(title), out) }
    end

    def target=(target)
      target.with_raw { |raw| Raw.check(Native.tpdf_outline_entry_set_target(@pointer, raw)) }
    end

    def open=(open)
      Raw.check(Native.tpdf_outline_entry_set_open(@pointer, open ? 1 : 0))
    end

    def add_child(child) = Raw.check(Native.tpdf_outline_entry_add_child(@pointer, child.pointer))

    def close
      Native.tpdf_outline_entry_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end

    def self.array(entries)
      entries.map { |entry| entry.pointer.to_i }.pack('Q*')
    end
  end

  # The write modes, transcribed from TpdfWriteMode.
  module WriteMode
    REWRITE = 0
    INCREMENTAL = 1
  end

  # An editor over a document; it holds its own reference to the object store.
  class Editor
    def initialize(pointer)
      @pointer = pointer
    end

    def close
      Native.tpdf_editor_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end

    # Raises when nothing was written; returns the widgets it could not draw.
    def fill_field(name, value)
      report = Raw.handle { |out| Native.tpdf_editor_fill_field(@pointer, Raw.cstr(name), Raw.cstr(value), out) }
      (0...Native.tpdf_fill_report_count(report)).map do |i|
        number, generation = Raw.slot(4), Raw.slot(2)
        Raw.check(Native.tpdf_fill_report_widget(report, i, number, generation))
        defect = Raw.slot(4)
        Raw.check(Native.tpdf_fill_report_defect(report, i, defect))
        SkippedWidget.new(object: Raw.u32(number), generation: Raw.u16(generation), defect: Raw.i32(defect),
                          message: Raw.text { |out| Native.tpdf_fill_report_message(report, i, out) })
      end
    ensure
      Native.tpdf_fill_report_free(report) if report
    end

    def set_checkbox(name, on) = Raw.check(Native.tpdf_editor_set_checkbox(@pointer, Raw.cstr(name), on ? 1 : 0))

    def select_radio(name, option)
      Raw.check(Native.tpdf_editor_select_radio(@pointer, Raw.cstr(name), Raw.cstr(option)))
    end

    # Saves under options, which start from TinkerPdf.write_options.
    def save(options = TinkerPdf.write_options)
      options.with_raw do |raw|
        Raw.take_buffer(Raw.handle { |out| Native.tpdf_editor_save(@pointer, raw, out) })
      end
    end

    def dirty? = !Native.tpdf_editor_is_dirty(@pointer).zero?
    def page_count = Native.tpdf_editor_page_count(@pointer)
    def delete_page(index) = Raw.check(Native.tpdf_editor_delete_page(@pointer, index))
    def move_page(from, to) = Raw.check(Native.tpdf_editor_move_page(@pointer, from, to))
    def rotate_page(index, degrees) = Raw.check(Native.tpdf_editor_rotate_page(@pointer, index, degrees))
    def insert_page(index, width, height) = Raw.check(Native.tpdf_editor_insert_page(@pointer, index, width, height))

    def set_crop_box(index, x0, y0, x1, y1)
      Raw.check(Native.tpdf_editor_set_crop_box(@pointer, index, x0, y0, x1, y1))
    end

    def append_content(page, operators)
      Raw.check(Native.tpdf_editor_append_content(@pointer, page, operators, operators.bytesize))
    end

    def field_count = Native.tpdf_editor_field_count(@pointer)
    def field_name(index) = Raw.text { |out| Native.tpdf_editor_field_name(@pointer, index, out) }
    def field_value(index) = Raw.text { |out| Native.tpdf_editor_field_value(@pointer, index, out) }

    # The editor's state as a value, for #restore to put back.
    def checkpoint
      Checkpoint.new(Raw.handle { |out| Native.tpdf_editor_checkpoint(@pointer, out) })
    end

    def restore(checkpoint) = Raw.check(Native.tpdf_editor_restore(@pointer, checkpoint.pointer))

    # Runs the form's calculate actions under policy, all or nothing.
    def recalculate(policy = Script::DEFAULT)
      report = Raw.handle { |out| Native.tpdf_editor_recalculate(@pointer, policy, out) }
      names = lambda do |count, accessor|
        (0...Native.send(count, report)).map { |i| Raw.text { |out| Native.send(accessor, report, i, out) } }
      end
      changed = (0...Native.tpdf_recalculation_changed_count(report)).map do |i|
        [Raw.text { |out| Native.tpdf_recalculation_changed_name(report, i, out) },
         Raw.text { |out| Native.tpdf_recalculation_changed_value(report, i, out) }]
      end
      Recalculation.new(changed: changed, skipped: Native.tpdf_recalculation_skipped_count(report),
                        cascades_cut: names.call(:tpdf_recalculation_cascades_cut_count,
                                                 :tpdf_recalculation_cascades_cut_name),
                        refused: names.call(:tpdf_recalculation_refused_count, :tpdf_recalculation_refused_name))
    ensure
      Native.tpdf_recalculation_free(report) if report
    end

    # What the field's format action displays; nil when it has none.
    def formatted_value(name, policy = Script::DEFAULT)
      Raw.text { |out| Native.tpdf_editor_formatted_value(@pointer, Raw.cstr(name), policy, out) }
    end

    # [accepted, change]: change is nil when the action refused. A refusal
    # is the form working, not an error.
    def keystroke(name, change, sel_start, sel_end, will_commit, policy)
      accepted = Raw.slot(4)
      out = Raw.slot
      Raw.check(Native.tpdf_editor_keystroke(@pointer, Raw.cstr(name), Raw.cstr(change), sel_start, sel_end,
                                             will_commit ? 1 : 0, policy, accepted, out))
      [!Raw.i32(accepted).zero?, Raw.take_string(out)]
    end

    # [accepted, value], as #keystroke; nothing is written either way.
    def validate(name, value, policy)
      accepted = Raw.slot(4)
      out = Raw.slot
      Raw.check(Native.tpdf_editor_validate(@pointer, Raw.cstr(name), Raw.cstr(value), policy, accepted, out))
      [!Raw.i32(accepted).zero?, Raw.take_string(out)]
    end

    # ranges: [first_page, style, prefix or nil, start]; TpdfPageLabelRange is
    # first_page u32 @0, style i32 @4, prefix pointer @8, start u32 @16.
    def set_page_labels(ranges)
      prefixes = ranges.map { |range| range[2] && Raw.cstr(range[2]) }
      raw = ranges.each_with_index.map do |(first, style, _prefix, start), i|
        [first, style, Raw.address(prefixes[i]), start].pack('LlQLx4')
      end.join
      Raw.check(Native.tpdf_editor_set_page_labels(@pointer, raw, ranges.size))
    end

    # A date is [year, month, day, hour, minute, second, offset-or-nil].
    def self.date(date)
      *fields, offset = date
      (fields + [offset.nil? ? 0 : 1, offset || 0]).pack('l8')
    end

    # Returns the file specification's [object, generation].
    def attach_file(name:, filename:, data:, description: nil, mime_type: nil, created: nil, modified: nil)
      kept = [Raw.cstr(name), Raw.cstr(filename), description && Raw.cstr(description),
              mime_type && Raw.cstr(mime_type), created && Editor.date(created), modified && Editor.date(modified),
              data.empty? ? nil : data.b]
      raw = (kept.map { |field| Raw.address(field) } + [data.bytesize]).pack('Q8')
      object, generation = Raw.slot(4), Raw.slot(2)
      Raw.check(Native.tpdf_editor_attach_file(@pointer, raw, object, generation))
      [Raw.u32(object), Raw.u16(generation)]
    end

    def set_outline(entries)
      Raw.check(Native.tpdf_editor_set_outline(@pointer, OutlineEntry.array(entries), entries.size))
    end

    def sync
      out = Raw.slot(4)
      Raw.check(yield(out))
      Raw.i32(out)
    end
    private :sync

    def set_info(key, value) = sync { |out| Native.tpdf_editor_set_info(@pointer, key, Raw.cstr(value), out) }
    def set_info_date(key, date) = sync { |out| Native.tpdf_editor_set_info_date(@pointer, key, Editor.date(date), out) }
    def set_trapped(trapped) = sync { |out| Native.tpdf_editor_set_trapped(@pointer, trapped, out) }

    def set_xmp_metadata(packet)
      sync { |out| Native.tpdf_editor_set_xmp_metadata(@pointer, packet, packet.bytesize, out) }
    end

    def set_page_boundary(index, boundary, x0, y0, x1, y1)
      Raw.check(Native.tpdf_editor_set_page_boundary(@pointer, index, boundary, x0, y0, x1, y1))
    end

    def sanitise(javascript: false, actions: false, embedded_files: false, metadata: false)
      what = [javascript, actions, embedded_files, metadata].map { |flag| flag ? 1 : 0 }.pack('l4')
      report = Raw.handle { |out| Native.tpdf_editor_sanitise(@pointer, what, out) }
      lists = [0, 1].map do |list|
        (0...Native.tpdf_sanitise_report_count(report, list)).map do |i|
          what_slot, has, object, generation = Raw.slot(4), Raw.slot(4), Raw.slot(4), Raw.slot(2)
          Raw.check(Native.tpdf_sanitise_report_entry(report, list, i, what_slot, has, object, generation))
          data, length = Raw.slot, Raw.slot
          Raw.check(Native.tpdf_sanitise_report_action(report, list, i, data, length))
          ref = [Raw.u32(object), Raw.u16(generation)]
          action = Raw.borrowed(data, length)
          next DeletedObject.new(object: ref, what: Raw.i32(what_slot), action: action) if list == 1

          path = (0...Native.tpdf_sanitise_report_path_count(report, i)).map do |s|
            is_index, position, key, key_length = Raw.slot(4), Raw.slot, Raw.slot, Raw.slot
            Raw.check(Native.tpdf_sanitise_report_path_step(report, i, s, is_index, position, key, key_length))
            Raw.i32(is_index).zero? ? Raw.borrowed(key, key_length) : Raw.u64(position)
          end
          RemovedEntry.new(holder: Raw.i32(has).zero? ? nil : ref, path: path, what: Raw.i32(what_slot),
                           action: action)
        end
      end
      SanitiseReport.new(removed: lists[0], deleted: lists[1])
    ensure
      Native.tpdf_sanitise_report_free(report) if report
    end
  end

  # An editor's state, borrowed by Editor#restore as often as it is needed.
  class Checkpoint
    attr_reader :pointer

    def initialize(pointer)
      @pointer = pointer
    end

    def close
      Native.tpdf_checkpoint_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end
  end

  # A page being drawn, owned until it is pushed.
  class PageBuilder
    attr_reader :pointer

    def initialize(pointer)
      @pointer = pointer
    end

    def text(font, size, x, y, text)
      Raw.check(Native.tpdf_page_builder_text(@pointer, font, font.bytesize, size, x, y, Raw.cstr(text)))
    end

    def fill_rect(x, y, w, h, grey) = Raw.check(Native.tpdf_page_builder_fill_rect(@pointer, x, y, w, h, grey))

    def image(resource, x, y, w, h)
      Raw.check(Native.tpdf_page_builder_image(@pointer, resource, resource.bytesize, x, y, w, h))
    end

    def link(x0, y0, x1, y1, target)
      target.with_raw { |raw| Raw.check(Native.tpdf_page_builder_link(@pointer, x0, y0, x1, y1, raw)) }
    end

    def set_fill_rgb(red, green, blue) = Raw.check(Native.tpdf_page_builder_set_fill_rgb(@pointer, red, green, blue))

    def set_stroke_rgb(red, green, blue)
      Raw.check(Native.tpdf_page_builder_set_stroke_rgb(@pointer, red, green, blue))
    end

    def set_crop_box(x0, y0, x1, y1) = Raw.check(Native.tpdf_page_builder_set_crop_box(@pointer, x0, y0, x1, y1))

    # Content-stream operators written as they are.
    def raw(operators) = Raw.check(Native.tpdf_page_builder_raw(@pointer, operators, operators.bytesize))

    def close
      Native.tpdf_page_builder_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end
  end

  # The image kinds, transcribed from TpdfImageKind.
  module ImageKind
    JPEG = 0
    RGB8 = 1
    GRAY8 = 2
  end

  # Assembles a document from pages, fonts and images.
  class Builder
    def initialize
      @pointer = Raw.handle { |out| Native.tpdf_builder_new(out) }
    end

    def add_base_font(resource, base_font)
      Raw.check(Native.tpdf_builder_add_base_font(@pointer, resource, resource.bytesize, base_font,
                                                  base_font.bytesize))
    end

    # TpdfImage: kind i32 @0, width @4, height @8, data pointer @16, len @24.
    def add_image(resource, kind, width, height, data)
      bytes = data.b
      raw = [kind, width, height, Raw.address(bytes), bytes.bytesize].pack('lLLx4QQ')
      Raw.check(Native.tpdf_builder_add_image(@pointer, resource, resource.bytesize, raw))
    end

    # A TrueType or CFF program, embedded under a resource name.
    def add_embedded_font(resource, base_font, program)
      Raw.check(Native.tpdf_builder_add_embedded_font(@pointer, resource, resource.bytesize, base_font,
                                                      base_font.bytesize, program, program.bytesize))
    end

    def subset_fonts=(subset)
      Raw.check(Native.tpdf_builder_set_subset_fonts(@pointer, subset ? 1 : 0))
    end

    def set_info(key, value) = Raw.check(Native.tpdf_builder_set_info(@pointer, key, key.bytesize, Raw.cstr(value)))

    def begin_page(width, height)
      PageBuilder.new(Raw.handle { |out| Native.tpdf_builder_begin_page(@pointer, width, height, out) })
    end

    def push_page(page) = Raw.check(Native.tpdf_builder_push_page(@pointer, page.pointer))

    def set_outline(entries)
      Raw.check(Native.tpdf_builder_set_outline(@pointer, OutlineEntry.array(entries), entries.size))
    end

    def finish
      Raw.take_buffer(Raw.handle { |out| Native.tpdf_builder_finish(@pointer, out) })
    end

    def close
      Native.tpdf_builder_free(@pointer) unless @pointer.nil?
      @pointer = nil
    end
  end

  def self.version = Native.tpdf_version.to_s
end
