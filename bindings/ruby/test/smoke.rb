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
fail!('an unencrypted document is not encrypted') if document.encrypted?
fail!('an unencrypted document may be printed') unless document.may_print?
fail!('a document opened from bytes is not streamed') if document.streamed?
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
puts 'RUBY-SMOKE: RAN, rendered and inked, every declaration called'
