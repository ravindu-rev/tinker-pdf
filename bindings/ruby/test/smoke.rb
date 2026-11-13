# frozen_string_literal: true

# Proves the Ruby binding is the engine, not merely a file that loads.
#
#   ruby -Ilib test/smoke.rb <testdata/simple-text.pdf> <face.ttf>
#
# The trap is the one every smoke test here is written against.
# testdata/simple-text.pdf names Helvetica and embeds no font program, and the
# engine bundles no faces and reads no font directories — so a render of it is
# a correctly sized, entirely blank bitmap, and "a bitmap of the right size
# came back" passes on a library whose renderer does nothing at all. So the
# render is asserted twice: blank without a face, inked with one. The script
# prints RUBY-SMOKE: RAN on success, which CI greps for, because a script that
# exits 0 having checked nothing looks exactly like a pass.

require 'tinker_pdf'

def fail!(message)
  warn "RUBY-SMOKE: FAILED: #{message}"
  exit 1
end

# Pixels that are not white, one byte per component of RGB.
def ink(pixels) = pixels.unpack('C*').each_slice(3).count { |red, _, _| red != 0xFF }

if ARGV.size != 2
  warn 'usage: smoke.rb <file.pdf> <face.ttf>'
  exit 2
end
puts "engine version #{TinkerPdf.version}"
data = File.binread(ARGV[0])
face = File.binread(ARGV[1])

document = TinkerPdf::Document.open(data)
fail!("page_count #{document.page_count}") unless document.page_count == 3
text = document.page_text(0)
fail!("text #{text.inspect}") unless text.include?('Tinker fixture')
puts "text=#{text.strip.inspect}"

bare = document.render(0)
painted = ink(bare.pixels)
fail!("this fixture embeds no font, so it must draw nothing yet; it drew #{painted}") unless painted.zero?
bare.close

document.set_fonts(face)
drawn = document.render(0)
pixels = drawn.pixels
painted = ink(pixels)
puts "bitmap #{drawn.width}x#{drawn.height} stride=#{drawn.stride} bytes=#{pixels.bytesize} ink=#{painted}"
fail!("only #{painted} pixels of ink with a face supplied") if painted < 100
fail!("the pixel string is #{pixels.bytesize} bytes") unless pixels.bytesize == drawn.stride * drawn.height
drawn.close

# A page past the end is the engine's refusal, with its status, rather than a
# crash or an empty answer.
begin
  document.page_text(99)
  fail!('a page past the end must be refused')
rescue TinkerPdf::Error => e
  fail!("a page past the end is #{e.message}, not NO_SUCH_PAGE") unless e.status == TinkerPdf::Status::NO_SUCH_PAGE
end
# So is a number its enum does not declare: an Integer crosses as a C int, and
# the engine checks it, where it once read past a jump table and killed the
# process (review of lane 7C).
[[8, -> { document.info(8) }], [4, -> { document.render(0, format: 4) }]].each do |number, call|
  call.call
  fail!("#{number} is not an enum number and must be refused")
rescue TinkerPdf::Error => e
  fail!("an enum number of #{number} is #{e.message}") unless e.status == TinkerPdf::Status::BAD_ARGUMENT
end
fail!('an unencrypted document is not encrypted') if document.encrypted?
fail!('an unencrypted document may be printed') unless document.may_print?
fail!('a document opened from bytes is not streamed') if document.streamed?
fail!('a document opened from bytes answers as its bytes do') unless document.answers_as_buffered?
size = document.page_size(0)
fail!("page size #{size}") unless size == [595.0, 842.0]
begin
  document.authenticate('anything')
  fail!('authenticating an unencrypted document must say there is nothing to authenticate')
rescue TinkerPdf::Error => e
  fail!("authenticate is #{e.message}") unless e.status == TinkerPdf::Status::NOT_ENCRYPTED
end
document.close

# The rest of the header, each call once, against the fixtures that answer
# it. The parity script covers what it scripts; this covers what it does not,
# so a declaration transcribed wrongly fails here rather than in a caller.
fixtures = File.dirname(File.expand_path(ARGV[0]))
locked = TinkerPdf::Document.open(File.binread(File.join(fixtures, 'encrypted-aes256.pdf')))
fail!('encrypted-aes256.pdf is encrypted') unless locked.encrypted?
begin
  locked.authenticate('not it')
  fail!('a wrong password must be refused')
rescue TinkerPdf::Error => e
  fail!("a wrong password is #{e.message}") unless e.status == TinkerPdf::Status::WRONG_PASSWORD
end
level = locked.authenticate('open-sesame')
fail!("the user password reached level #{level}") unless level == TinkerPdf::AuthLevel::USER
locked.close
restricted = TinkerPdf::Document.open(File.binread(File.join(fixtures, 'permissions-noprint.pdf')))
restricted.authenticate('user')
fail!('permissions-noprint.pdf denies printing to its user') if restricted.may_print?
restricted.close

form = TinkerPdf::Document.open(File.binread(File.join(fixtures, 'form-fields.pdf')))
editor = form.editor
form.close
names = (0...editor.field_count).map { |i| editor.field_name(i) }
fail!("fields #{names}") unless names.sort == %w[agree colour name notes]
notes = names.index('notes')
fail!('nothing is edited yet') if editor.dirty?
checkpoint = editor.checkpoint
skipped = editor.fill_field('name', 'Ada')
fail!("the skipped widget's defect is #{skipped[0].defect}") unless
  skipped[0].defect == TinkerPdf::WidgetDefect::RECT_MISSING
editor.fill_field('notes', 'kept for a moment')
fail!("notes is #{editor.field_value(notes).inspect}") unless editor.field_value(notes) == 'kept for a moment'
fail!('a fill is an edit') unless editor.dirty?
editor.restore(checkpoint)
editor.restore(checkpoint)
checkpoint.close
fail!("restored notes is #{editor.field_value(notes).inspect}") unless editor.field_value(notes) == ''
recalculation = editor.recalculate(TinkerPdf::Script::DEFAULT)
fail!("a form with no calculations changed #{recalculation}") unless
  recalculation.changed.empty? && recalculation.skipped.zero? && recalculation.refused.empty?
fail!('notes carries no format action') unless editor.formatted_value('notes').nil?
accepted, change = editor.keystroke('notes', 'x', 0, 0, false, TinkerPdf::Script::KEYSTROKE)
fail!("a field with no keystroke action took #{[accepted, change]}") unless accepted && change == 'x'
accepted, value = editor.validate('notes', 'v', TinkerPdf::Script::VALIDATE)
fail!("a field with no validate action took #{[accepted, value]}") unless accepted && value == 'v'

editor.insert_page(1, 200, 300)
editor.rotate_page(0, 90)
editor.move_page(1, 0)
editor.set_crop_box(0, 0, 0, 100, 100)
editor.append_content(0, '0 0 m 10 10 l S')
begin
  editor.rotate_page(0, 45)
  fail!('a turn that is not a quarter-turn multiple must be refused')
rescue TinkerPdf::Error => e
  fail!("rotating by 45 is #{e.message}") unless e.status == TinkerPdf::Status::EDIT_REFUSED
end
fail!("the editor sees #{editor.page_count} pages") unless editor.page_count == 2
editor.delete_page(1)
options = TinkerPdf.write_options
options.encryption = TinkerPdf::Encryption.new(user_password: 'u', owner_password: 'o', permissions: -4,
                                               entropy: (0...TinkerPdf::ENTROPY_LEN).to_a.pack('C*'))
saved = editor.save(options)
editor.close
reopened = TinkerPdf::Document.open(saved)
fail!('the save asked for encryption') unless reopened.encrypted?
fail!('the user password opens it') unless reopened.authenticate('u') == TinkerPdf::AuthLevel::USER
media = reopened.page_box(0, TinkerPdf::PageBoundary::MEDIA_BOX)
fail!("the inserted page is not first: #{media}") unless media == [0.0, 0.0, 200.0, 300.0]
fail!("the crop box is the page's size, #{reopened.page_size(0)}") unless reopened.page_size(0) == [100.0, 100.0]
reopened.close

builder = TinkerPdf::Builder.new
builder.add_embedded_font('F2', 'DejaVuSans', face)
builder.subset_fonts = true
page = builder.begin_page(200, 200)
page.set_fill_rgb(0.2, 0.4, 0.6)
page.set_stroke_rgb(0.6, 0.4, 0.2)
page.set_crop_box(0, 0, 150, 150)
page.raw('10 10 m 140 140 l S')
page.text('F2', 18, 20, 100, 'Embedded')
builder.push_page(page)
page.close
built = TinkerPdf::Document.open(builder.finish)
builder.close
defects = built.validate
fail!("the built document has defects #{defects}") unless defects.empty?
fail!("the built text is #{built.page_text(0).inspect}") unless built.page_text(0).include?('Embedded')
fail!("the crop box is #{built.page_box(0, TinkerPdf::PageBoundary::CROP_BOX)}") unless
  built.page_box(0, TinkerPdf::PageBoundary::CROP_BOX) == [0.0, 0.0, 150.0, 150.0]
fail!('the embedded face draws') unless ink(built.render(0).pixels).positive?
built.close
fail!('the engine fits by default') unless TinkerPdf::View.fit.kind == TinkerPdf::DestKind::FIT

# ---- the command stream -------------------------------------------------------
#
# The stream of a page holding every kind of record -- the C ABI's test page
# (crates/tinker-pdf-ffi/src/commands/tests.rs, every_kind_page), built again
# from the face and the profile the checkout holds -- decoded through the
# binding's own types and written back to the facade's bytes, beside a stream
# of the three kinds a page reaches only past a budget or through a recording
# that composites colorants, pushed by hand. Between the two, every kind but
# the end. A recording device hears every kind in the stream's order, the
# tables' accessors are their records' fields, the replay through the engine
# draws what the page's render draws, and bytes that are no stream are
# refused with the reader's sentence.

def check!(ok, message)
  fail!("command stream: #{message}") unless ok
end

# The block raises Error with `status`.
def expect_status(status, what)
  yield
  fail!("command stream: #{what} must be refused with status #{status}")
rescue TinkerPdf::Error => e
  fail!("command stream: #{what}: #{e.message}, not status #{status}") unless e.status == status
end

# A stream object of `dictionary` holding `data`.
def stream_object(dictionary, data)
  "<< #{dictionary} /Length #{data.bytesize} >>\nstream\n".b + data.b + "\nendstream".b
end

# A document of `objects`, numbered from one, the first its catalog, with
# its cross-reference table.
def pdf(objects)
  out = "%PDF-1.7\n".b
  offsets = objects.each_with_index.map do |object, i|
    offset = out.bytesize
    out << "#{i + 1} 0 obj\n".b << object.b << "\nendobj\n".b
    offset
  end
  xref = out.bytesize
  out << "xref\n0 #{objects.size + 1}\n0000000000 65535 f \n".b
  offsets.each { |offset| out << format("%010d 00000 n \n", offset).b }
  out << "trailer\n<< /Size #{objects.size + 1} /Root 1 0 R >>\nstartxref\n#{xref}\n%%EOF\n".b
end

# The 200 x 160 page whose stream holds every kind a page's stream is written
# with: a state with stored parameters, a black generation, a transfer
# function and a halftone; a clip and an axial shading in an ICC space; a fill
# in a /Separation space and one in a Lab space whose range is written
# backwards; text in an embedded face and in a font no name resolves; an image
# XObject and an inline image; marked content, a group under a luminosity soft
# mask then cleared; a tiling pattern, a form with resources of its own, a
# form that draws itself, and a square annotation with an appearance.
def every_kind_page(serif, profile)
  widths = (['500'] * 95).join(' ')
  content = "q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n" \
            "q 10 40 40 30 re W n /Sh0 sh Q\n" \
            "q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n" \
            "q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n" \
            "BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n" \
            "BT /Nope 12 Tf 10 140 Td (x) Tj ET\n" \
            "q 20 0 0 20 120 20 cm /Im0 Do Q\n" \
            'q 10 0 0 10 150 20 cm BI /W 2 /H 2 /CS /G /BPC 8 ID '.b +
            [0x00, 0x55, 0xAA, 0xFF].pack('C*') +
            " EI Q\n" \
            "/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n" \
            "q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n" \
            "q /Pattern cs /P0 scn 150 60 30 30 re f Q\n" \
            "q 1 0 0 1 150 100 cm /Fm0 Do Q\n" \
            "/Rec Do\n"
  rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0].pack('C*')
  pdf([
        '<< /Type /Catalog /Pages 2 0 R >>',
        '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R ' \
        '/Contents 5 0 R /Annots [17 0 R] >>',
        '<< /Font << /F0 6 0 R >> ' \
        '/XObject << /Im0 9 0 R /G 10 0 R /Fm0 11 0 R /Rec 12 0 R >> ' \
        '/Shading << /Sh0 13 0 R >> /Pattern << /P0 15 0 R >> ' \
        '/ColorSpace << ' \
        '/CS0 [/Separation /Spot /DeviceRGB ' \
        '<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] ' \
        '/Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] /Range [100 -100 -100 100] >>] >> ' \
        '/ExtGState << ' \
        '/Fancy << /RI /Saturation ' \
        '/BG << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> ' \
        '/TR << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >> ' \
        '/HT << /Type /Halftone /HalftoneType 1 /Frequency 60 /Angle 45 ' \
        '/SpotFunction /Round >> >> ' \
        '/Masked << /SMask << /Type /Mask /S /Luminosity /G 16 0 R >> >> ' \
        '/NoMask << /SMask /None >> >> >>',
        stream_object('', content),
        '<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 ' \
        "/LastChar 126 /Widths [#{widths}] /FontDescriptor 7 0 R " \
        '/Encoding /WinAnsiEncoding >>',
        '<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 ' \
        '/FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 ' \
        '/CapHeight 700 /StemV 80 /FontFile2 8 0 R >>',
        stream_object("/Length1 #{serif.bytesize}", serif),
        stream_object('/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB ' \
                      '/BitsPerComponent 8', rgb),
        stream_object('/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>',
                      '0 0.8 0 rg 0 0 20 20 re f'),
        stream_object('/Type /XObject /Subtype /Form /BBox [0 0 40 40] ' \
                      '/Resources << /XObject << /ImF 9 0 R >> >>', 'q 10 0 0 10 0 0 cm /ImF Do Q'),
        stream_object('/Type /XObject /Subtype /Form /BBox [0 0 10 10] ' \
                      '/Resources << /XObject << /Rec 12 0 R >> >>', '0 0 1 rg 0 0 2 2 re f /Rec Do'),
        '<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] ' \
        '/Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> ' \
        '/Extend [true true] >>',
        stream_object('/N 3', profile),
        stream_object('/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 ' \
                      '/YStep 10 /Resources << >>', '1 0 0 rg 0 0 5 5 re f'),
        stream_object('/Type /XObject /Subtype /Form /BBox [0 0 200 160] ' \
                      '/Group << /S /Transparency /CS /DeviceGray >>', '0.5 g 0 0 200 160 re f'),
        '<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>',
        stream_object('/Type /XObject /Subtype /Form /BBox [0 0 40 30]', '0 0 1 RG 2 w 1 1 38 28 re S')
      ])
end

# `fields`, each made again from its tag and value through the binding's
# constructors, and held equal to what it was made from.
def rebuilt(fields)
  tags = TinkerPdf::CommandFieldTag
  fields.map do |field|
    value = field.value
    again = case field.tag
            when tags::NOTHING then TinkerPdf::CommandField.nothing
            when tags::UNSIGNED then TinkerPdf::CommandField.unsigned(value)
            when tags::SIGNED then TinkerPdf::CommandField.signed(value)
            when tags::REAL then TinkerPdf::CommandField.real(value)
            when tags::BYTES then TinkerPdf::CommandField.bytes(value)
            when tags::REALS then TinkerPdf::CommandField.reals(value)
            when tags::NATURALS then TinkerPdf::CommandField.naturals(value)
            when tags::STRINGS then TinkerPdf::CommandField.strings(value)
            else fail!("command stream: a field of tag #{field.tag}")
            end
    check!(again == field && again.hash == field.hash, "a field rebuilds: #{field.inspect}")
    again
  end
end

# Field `at`'s unsigned value.
def natural(record, at)
  field = record.fields[at]
  check!(field.tag == TinkerPdf::CommandFieldTag::UNSIGNED, "kind #{record.kind}'s field #{at} is #{field.inspect}")
  field.value
end

# Field `at`'s bytes.
def bytes_of(record, at)
  field = record.fields[at]
  check!(field.tag == TinkerPdf::CommandFieldTag::BYTES, "kind #{record.kind}'s field #{at} is #{field.inspect}")
  field.value
end

# Every kind this build knows, by number.
def every_kind = TinkerPdf::CommandKind.constants.map { |name| TinkerPdf::CommandKind.const_get(name) }

# A device that notes every record it is handed, each by the method its kind
# names: the command stream leg's recording device.
class Recorder
  include TinkerPdf::CommandDevice

  attr_reader :seen

  def initialize
    @seen = []
  end

  TinkerPdf::COMMAND_METHODS.each_with_index do |name, kind|
    next if name.nil?

    define_method(name) do |record|
      fail!("command stream: the method for #{name} was handed kind #{record.kind}") unless record.kind == kind
      @seen << kind
    end
  end

  def unknown(record)
    fail!("command stream: a record of kind #{record.number}, which this build does not know")
  end
end

# A device naming one method and including nothing, which hears only that
# kind: a device that does not respond to a kind's method is passed over.
class GlyphRuns
  attr_reader :runs

  def initialize
    @runs = 0
  end

  def glyph_run(record)
    fail!("command stream: glyph_run was handed kind #{record.kind}") unless
      record.kind == TinkerPdf::CommandKind::GLYPH_RUN
    @runs += 1
  end
end

# A device whose exception ends the replay.
class Refusing
  include TinkerPdf::CommandDevice

  def begin_text(_record)
    raise ArgumentError, 'the device refuses'
  end
end

def command_streams(root)
  kinds = TinkerPdf::CommandKind
  serif = File.binread(File.join(root, 'crates', 'tinker-pdf-font', 'data', 'liberation',
                                 'LiberationSerif-Regular.ttf'))
  profile = File.binread(File.join(root, 'fuzz', 'corpus', 'icc_profile', 'rgb-gamma-curve.icc'))
  page = TinkerPdf::Document.open(every_kind_page(serif, profile))
  written = page.command_stream(0)
  page_size = page.page_size(0)
  bitmap = page.render(0)
  direct = [bitmap.width, bitmap.height, bitmap.pixels]
  bitmap.close
  expect_status(TinkerPdf::Status::NO_SUCH_PAGE, 'a stream of a page past the end') { page.command_stream(1) }
  page.close

  stream = TinkerPdf::CommandStream.parse(written)
  check!(stream.to_bytes == written, 'the stream writes back its own bytes')
  check!(stream.page_index.zero?, "the stream is page 0's, not #{stream.page_index}")
  check!(stream.size == page_size, "the stream's page is the page's size")

  records = stream.records
  check!(records.size == stream.count, 'every record, counted')
  check!(records[0].kind == kinds::PAGE, "the page record leads: #{records[0].kind}")
  records.each do |record|
    command = stream.next
    check!(!command.kind.nil?, "kind #{record.number} is one this build knows")
    check!(command == stream.command_at(record.index), "the cursor reads record #{record.index} as its place does")
    check!(command.kind == record.kind && command.number == record.number &&
           command.field_count == record.fields.size, "record #{record.index}'s command")
  end
  2.times do
    last = stream.next
    check!(last.kind == kinds::END_ && last.number.zero? && last.index == records.size && last.field_count.zero?,
           "the end, after the last record: #{last}")
  end
  stream.rewind
  check!(stream.next == stream.command_at(0), 'a rewind puts the cursor back on the page record')
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'a record past the end') { stream.command_at(records.size) }
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, "a field past the page record's last") do
    stream.field_at(0, records[0].fields.size)
  end

  encoder = TinkerPdf::CommandEncoder.new
  records.each { |record| encoder.push(record.kind, rebuilt(record.fields)) }
  check!(encoder.finish == written,
         "the stream decoded and written back through the binding's types is the facade's bytes")
  encoder.close

  on_the_page = records.map(&:kind).uniq
  by_hand = [kinds::COLORANTS, kinds::GROUP_BUDGET_SPENT, kinds::NESTED_BUDGET_SPENT]
  (every_kind - [kinds::END_] - by_hand).each do |kind|
    check!(on_the_page.include?(kind), "the page's stream holds a record of kind #{kind}")
  end

  field = TinkerPdf::CommandField
  colorants = [
    field.unsigned(1), field.strings(%w[Spot Other]), field.bytes([128, 64].pack('C*')), field.naturals([1]),
    field.bytes([0, 0, 0, 255].pack('C*')), field.nothing, field.bytes([10, 20, 30, 40].pack('C*'))
  ]
  hands = TinkerPdf::CommandEncoder.new
  hands.push_record(records[0])
  hands.push(kinds::COLORANTS, colorants)
  hands.push(kinds::GROUP_BUDGET_SPENT, [])
  hands.push(kinds::NESTED_BUDGET_SPENT, [])
  hand = hands.finish
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'the end pushed') { hands.push(kinds::END_, []) }
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'a kind the C ABI does not declare') { hands.push(99, []) }
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'a tag the C ABI does not declare') do
    hands.push(kinds::FUNCTION, [field.send(:new, 9, nil)])
  end
  check!(hands.finish == hand, 'an encoder keeps its records, and a refused push adds none')
  hands.close
  lonely = TinkerPdf::CommandEncoder.new
  lonely.push(kinds::GROUP_BUDGET_SPENT, [])
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'records with no page record') { lonely.finish }
  lonely.close

  heard = []
  reread = TinkerPdf::CommandStream.parse(hand)
  back = reread.records
  check!(back.map(&:kind) == [kinds::PAGE, kinds::COLORANTS, kinds::GROUP_BUDGET_SPENT, kinds::NESTED_BUDGET_SPENT],
         "the records pushed by hand read back in order: #{back.map(&:kind)}")
  check!(back[0].fields == records[0].fields && back[1].fields == colorants && back[2].fields.empty? &&
         back[3].fields.empty?, 'the records pushed by hand read back as pushed')
  check!(reread.version == stream.version, "the page's stream is at the version the encoder writes, this build's")
  writer = TinkerPdf::CommandEncoder.new
  back.each { |record| writer.push(record.kind, rebuilt(record.fields)) }
  check!(writer.finish == hand, "and write back through the binding's types")
  writer.close
  [stream, reread].each do |each|
    recorder = Recorder.new
    each.replay(recorder)
    check!(recorder.seen == each.records.map(&:kind), "the device receives every record in the decoder's order")
    heard.concat(recorder.seen)
  end
  (every_kind - [kinds::END_]).each do |kind|
    check!(heard.include?(kind), "the device heard a record of kind #{kind}")
  end
  reread.close

  glyphs = GlyphRuns.new
  stream.replay(glyphs)
  check!(glyphs.runs == records.count { |record| record.kind == kinds::GLYPH_RUN },
         'a device naming one method hears only that kind')
  begin
    stream.replay(Refusing.new)
    fail!("command stream: a device's exception ends the replay")
  rescue ArgumentError => e
    check!(e.message == 'the device refuses', "the device's exception is raised: #{e.message}")
  end

  outlines = 0
  decodes = 0
  records.each do |record|
    case record.kind
    when kinds::OUTLINE
      outline = stream.outline_at(record.index)
      reals = record.fields[5].value
      check!(outline.scope == natural(record, 0) && outline.font_id == natural(record, 1) &&
             outline.code == natural(record, 2) && outline.found == (natural(record, 3) == 1) &&
             outline.operators == bytes_of(record, 4) && outline.numbers.pack('G*') == reals.pack('G*'),
             "outline record #{record.index} is its fields")
      outlines += 1
    when kinds::IMAGE_DECODE
      image = stream.image_at(record.index)
      check!(image.image == natural(record, 0) && image.reduction == natural(record, 1) &&
             image.answer == natural(record, 2),
             "image decode record #{record.index} is its image, reduction and answer")
      case image.answer
      when TinkerPdf::ImageAnswer::IMAGE
        held = record.fields[8].tag == TinkerPdf::CommandFieldTag::BYTES
        two_colour = held ? bytes_of(record, 8) : ''.b
        colours = held ? bytes_of(record, 9).unpack('C*') : [0] * 8
        check!(image.width == natural(record, 4) && image.height == natural(record, 5) &&
               image.rgb == bytes_of(record, 6) && image.alpha == bytes_of(record, 7) &&
               image.two_colour == two_colour && image.colours == colours &&
               image.stencil == (natural(record, 10) == 1) && image.interpolate == (natural(record, 11) == 1),
               "image decode record #{record.index} is its samples and colours")
        window = record.fields[15]
        if window.tag == TinkerPdf::CommandFieldTag::NATURALS
          check!(image.window == window.value, "image decode record #{record.index}'s window")
        else
          check!(image.window.nil?, "image decode record #{record.index} is all of its decode")
        end
      when TinkerPdf::ImageAnswer::REFUSED
        check!(image.codec == bytes_of(record, 3).force_encoding(Encoding::UTF_8),
               'a refused decode names its codec')
      end
      decodes += 1
    end
  end
  check!(outlines.positive? && decodes.positive?, "#{outlines} outlines and #{decodes} decodes")
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'an outline asked of the page record') { stream.outline_at(0) }
  expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'a decode asked of the page record') { stream.image_at(0) }

  replayed = stream.render
  check!([replayed.width, replayed.height, replayed.pixels] == direct,
         'the replay draws what a render of the page draws')
  replayed.close

  [''.b, 'not a command stream'.b, written[0...-1]].each do |refused|
    expect_status(TinkerPdf::Status::BAD_ARGUMENT, 'bytes that are no stream') do
      TinkerPdf::CommandStream.parse(refused).close
    end
  end
  begin
    TinkerPdf::CommandStream.parse('plainly not one'.b).close
    fail!('command stream: plainly not a stream, and read')
  rescue TinkerPdf::Error => e
    check!(e.message.include?('not a command stream'), "the reader's sentence: #{e.message}")
  end
  stream.close
  puts "command stream: #{records.size} records, every kind, written back to the facade's bytes"
end

command_streams(File.dirname(fixtures))
puts 'RUBY-SMOKE: RAN, rendered and inked, every declaration called, the command stream written back'
