# frozen_string_literal: true

# The parity scripts, run through the Ruby binding.
#
#   ruby -Ilib test/write_parity.rb <path to testdata/form-fields.pdf>
#
# The same scripts crates/tinker-pdf/examples/write_parity.rs runs against the
# facade and every other binding runs through its own surface; `cargo xtask
# bindings-parity` requires every surface to print the same SHA-256s. Ruling 11
# is what makes that the right test: a binding projects the facade 1:1 and adds
# no logic of its own, so surfaces disagreeing means one of them added
# something. The read texts are specified byte for byte in the facade
# example's module documentation.
#
# Every written artefact goes through the engine's own strict structural
# validator before its hash is printed, because byte-identical outputs
# agreeing tells you nothing if all of them are wrong.

require 'digest'
require 'tinker_pdf'

def fail!(message)
  warn "RUBY-PARITY: FAILED: #{message}"
  exit 1
end

def check(ok, message)
  fail!(message) unless ok
end

def sha(data) = Digest::SHA256.hexdigest(data)

# Validates an artefact, then prints the line bindings-parity reads.
def report(script, data)
  document = TinkerPdf::Document.open(data)
  defects = document.validate
  document.close
  check(defects.empty?, "#{script}: the artefact does not pass the strict validator: #{defects.map(&:rule)}")
  puts "WROTE sha256=#{sha(data)} surface=ruby script=#{script} bytes=#{data.bytesize}"
end

def report_read(script, text)
  $stdout.write(text) unless ENV.fetch('TINKER_PARITY_DUMP', '').empty?
  puts "READ sha256=#{sha(text)} surface=ruby script=#{script} bytes=#{text.bytesize}"
end

# The eight-by-eight grey image every surface builds, from the same formula.
def parity_image = (0...64).map { |i| (i * 7) % 256 }.pack('C*')

# The engine's default options but the mode, which is the script's choice.
def options(mode)
  options = TinkerPdf.write_options
  options.mode = mode
  options
end

def fill_and_save(fixture)
  document = TinkerPdf::Document.open(fixture)
  editor = document.editor
  # The editor holds its own reference to the object store.
  document.close
  skipped = editor.fill_field('name', 'Ada Lovelace')
  check(skipped.size == 1, 'the /Rect-less widget must be reported, not swallowed')
  check(skipped[0].message == '7 0 R: no usable /Rect (12.5.2)', "unexpected report: #{skipped[0].message}")
  check(skipped[0].object == 7 && skipped[0].generation.zero?, 'the report lost the widget it names')
  check(editor.fill_field('notes', 'every surface writes this').empty?,
        'the control field is well formed, so nothing is skipped')
  editor.set_checkbox('agree', true)
  editor.select_radio('colour', 'red')
  editor.save(options(TinkerPdf::WriteMode::INCREMENTAL))
ensure
  editor&.close
end

def page_target(page, kind, **numbers)
  TinkerPdf::Target.new(page: page, view: TinkerPdf::View.new(kind: kind, **numbers))
end

def build_a_document
  builder = TinkerPdf::Builder.new
  builder.add_base_font('F1', 'Helvetica')
  builder.add_image('Im1', TinkerPdf::ImageKind::GRAY8, 8, 8, parity_image)

  one = builder.begin_page(200, 200)
  one.text('F1', 14, 20, 170, 'Page one')
  one.fill_rect(20, 40, 60, 60, 0.25)
  one.image('Im1', 100, 40, 60, 60)
  builder.push_page(one)
  one.close

  two = builder.begin_page(200, 200)
  two.text('F1', 14, 20, 170, 'Page two')
  builder.push_page(two)
  two.close

  builder.set_info('Title', 'tinker-pdf write parity')
  first = TinkerPdf::OutlineEntry.new('Page one')
  first.target = page_target(0, TinkerPdf::DestKind::FIT)
  second = TinkerPdf::OutlineEntry.new('Page two')
  second.target = page_target(1, TinkerPdf::DestKind::FIT)
  builder.set_outline([first, second])
  first.close
  second.close
  builder.finish
ensure
  builder&.close
end

CREATED = [2026, 10, 3, 12, 0, 0, 0].freeze
PACKET = "<x:xmpmeta xmlns:x='adobe:ns:meta/'/>"

INFO = %w[title author subject keywords creator producer creation-date modification-date].freeze
TRAPPED = %w[absent true false unknown].freeze
BOXES = %w[media crop bleed trim art].freeze

def document_ops(outline)
  document = TinkerPdf::Document.open(outline)
  editor = document.editor
  document.close
  editor.set_page_labels([[0, TinkerPdf::LabelStyle::ROMAN_LOWER, nil, 1],
                          [2, TinkerPdf::LabelStyle::DECIMAL, 'A-', 1]])
  editor.attach_file(name: 'data.csv', filename: 'data.csv', description: 'the numbers', mime_type: 'text/csv',
                     created: CREATED, data: "a,b\n1,2\n")
  check(editor.set_info(TinkerPdf::InfoKey::TITLE, 'Document operations') == TinkerPdf::MetadataSync::ALONE,
        'no XMP packet yet, so the title is alone')
  editor.set_info(TinkerPdf::InfoKey::AUTHOR, 'tinker-pdf')
  editor.set_info_date(TinkerPdf::InfoKey::CREATION_DATE, CREATED)
  editor.set_trapped(TinkerPdf::Trapped::FALSE)
  check(editor.set_xmp_metadata(PACKET) == TinkerPdf::MetadataSync::OTHER_HALF_UNCHANGED,
        '/Info has entries the packet was not checked against')
  editor.set_page_boundary(0, TinkerPdf::PageBoundary::TRIM_BOX, 10, 10, 585, 832)
  editor.set_page_boundary(1, TinkerPdf::PageBoundary::BLEED_BOX, 0, 0, 595, 842)
  only = TinkerPdf::OutlineEntry.new('Only entry')
  only.target = page_target(3, TinkerPdf::DestKind::FIT_H, top: 700)
  editor.set_outline([only])
  only.close
  editor.save(options(TinkerPdf::WriteMode::REWRITE))
ensure
  editor&.close
end

# The options a save takes, every one the C ABI carries but encryption away
# from its default, after two edits that give them something to act on: the
# deleted page is what garbage collection drops, and the appended operators
# are the one stream nobody has encoded, which is what compression
# compresses. save-linearized is the same save linearized; the linearizer sets
# object streams and compression aside, so it is a second script.
def save_options(operated, linearize)
  document = TinkerPdf::Document.open(operated)
  editor = document.editor
  document.close
  editor.delete_page(1)
  editor.append_content(0, '0 0 m 100 100 l S')
  chosen = options(TinkerPdf::WriteMode::REWRITE)
  chosen.linearize = linearize
  chosen.version_major = 2
  chosen.version_minor = 0
  chosen.object_streams = true
  chosen.compress = true
  chosen.garbage_collect = true
  editor.save(chosen)
ensure
  editor&.close
end

REMOVALS = %w[javascript document-javascript calculation-order xfa-form action embedded-file-tree embedded-file
              info metadata].freeze

def sanitise(operated)
  document = TinkerPdf::Document.open(operated)
  editor = document.editor
  document.close
  result = editor.sanitise(javascript: true, actions: true, embedded_files: true, metadata: true)
  text = +''
  result.removed.each do |entry|
    holder = entry.holder ? entry.holder.join('.') : 'trailer'
    steps = entry.path.map { |step| step.is_a?(Integer) ? "i:#{step}" : "k:#{step.unpack1('H*')}" }
    text << "removed #{REMOVALS[entry.what]} #{holder} #{steps.join('/')} #{bytes_token(entry.action)}\n"
  end
  result.deleted.each do |entry|
    text << "deleted #{REMOVALS[entry.what]} #{entry.object.join('.')} #{bytes_token(entry.action)}\n"
  end
  [editor.save(options(TinkerPdf::WriteMode::REWRITE)), text]
ensure
  editor&.close
end

def linked_document
  builder = TinkerPdf::Builder.new
  builder.add_base_font('F1', 'Helvetica')
  one = builder.begin_page(200, 200)
  one.text('F1', 12, 20, 170, 'Links')
  one.link(10, 10, 60, 30, TinkerPdf::Target.new(uri: 'https://example.org/parity'))
  one.link(70, 10, 120.5, 30.25, page_target(1, TinkerPdf::DestKind::XYZ, left: 10, zoom: 1.5))
  builder.push_page(one)
  one.close
  two = builder.begin_page(200, 200)
  builder.push_page(two)
  two.close
  builder.set_info('Title', 'Read surface — parity')
  builder.set_info('Author', '')
  heading = TinkerPdf::OutlineEntry.new('Part one')
  heading.open = true
  chapter = TinkerPdf::OutlineEntry.new('Chapter one')
  chapter.target = page_target(1, TinkerPdf::DestKind::FIT_H, top: 150)
  heading.add_child(chapter)
  chapter.close
  elsewhere = TinkerPdf::OutlineEntry.new('Elsewhere')
  elsewhere.target = TinkerPdf::Target.new(uri: 'https://example.org/')
  builder.set_outline([heading, elsewhere])
  heading.close
  elsewhere.close
  builder.finish
ensure
  builder&.close
end

# The contract's tokens.
def text_token(value) = value.nil? ? '-' : "s:#{value.b.unpack1('H*')}"
def bytes_token(value) = value.nil? ? '-' : "b:#{value.unpack1('H*')}"
def number(value) = value.nil? ? '-' : format('f:%016x', [value.to_f].pack('G').unpack1('Q>'))
def reference(value) = value.nil? ? '-' : value.join('.')
def digest(value) = value.nil? ? '-' : sha(value)

def view_token(view)
  case view.kind
  when TinkerPdf::DestKind::XYZ then "xyz #{number(view.left)} #{number(view.top)} #{number(view.zoom)}"
  when TinkerPdf::DestKind::FIT_H then "fith #{number(view.top)}"
  when TinkerPdf::DestKind::FIT_V then "fitv #{number(view.left)}"
  when TinkerPdf::DestKind::FIT_R
    "fitr #{number(view.left)} #{number(view.bottom)} #{number(view.right)} #{number(view.top)}"
  when TinkerPdf::DestKind::FIT_B then 'fitb'
  when TinkerPdf::DestKind::FIT_BH then "fitbh #{number(view.top)}"
  when TinkerPdf::DestKind::FIT_BV then "fitbv #{number(view.left)}"
  else 'fit'
  end
end

def destination_token(destination)
  return '-' if destination.nil?

  case destination.kind
  when TinkerPdf::DestinationKind::EXPLICIT
    page = destination.page_index.nil? ? '-' : destination.page_index.to_s
    "explicit #{page} #{reference(destination.page_ref)} #{view_token(destination.view)}"
  when TinkerPdf::DestinationKind::NAMED then "named #{bytes_token(destination.bytes)}"
  else "uri #{bytes_token(destination.bytes)}"
  end
end

def action_token(link)
  case link.action
  when TinkerPdf::ActionKind::ABSENT then '-'
  when TinkerPdf::ActionKind::GO_TO then "goto #{destination_token(link.destination)}"
  when TinkerPdf::ActionKind::GO_TO_R then "gotor #{bytes_token(link.action_bytes)} #{destination_token(link.destination)}"
  when TinkerPdf::ActionKind::URI then "uri #{bytes_token(link.action_bytes)}"
  when TinkerPdf::ActionKind::NAMED then "named #{bytes_token(link.action_bytes)}"
  when TinkerPdf::ActionKind::LAUNCH then "launch #{bytes_token(link.action_bytes)}"
  else "other #{bytes_token(link.action_bytes)}"
  end
end

def read_dump(name, document, out)
  out << "document #{name}\n"
  out << "version #{text_token(document.pdf_version)}\n"
  out << "pages #{document.page_count}\n"
  INFO.each_with_index { |key, i| out << "info #{key} #{text_token(document.info(i))}\n" }
  out << "trapped #{TRAPPED[document.trapped]}\n"
  document.page_count.times do |index|
    label = document.page_label(index)
    break if label.nil?

    out << "label #{index} #{text_token(label)}\n"
  end
  document.page_count.times do |index|
    BOXES.each_with_index do |box, boundary|
      out << "box #{index} #{box} #{document.page_box(index, boundary).map { |v| number(v) }.join(' ')}\n"
    end
  end
  document.outline.each do |item|
    out << "outline #{item.depth} #{item.open ? 1 : 0} #{text_token(item.title)} " \
           "#{destination_token(item.destination)}\n"
  end
  document.page_count.times do |index|
    document.links(index).each do |link|
      out << "link #{index} #{link.rect.map { |v| number(v) }.join(' ')} #{reference(link.reference)} " \
             "#{action_token(link)}\n"
    end
  end
  attachments = document.attachments
  attachments.count.times do |i|
    data = begin
      attachments.data(i)
    rescue TinkerPdf::Error
      nil
    end
    size = attachments.size(i)
    out << "attachment #{text_token(attachments.name(i))} #{text_token(attachments.filename(i))} " \
           "#{text_token(attachments.description(i))} #{size.nil? ? '-' : size} #{digest(data)}\n"
  end
  attachments.close
  out << "xmp #{digest(document.xmp_metadata)}\n"
  document.warnings.each do |warning|
    out << "warning #{warning.offset} #{reference(warning.object)} #{warning.kind} #{text_token(warning.message)}\n"
  end
end

def read_surface(outline, operated)
  out = +''
  [['shifted', "JUNK\n".b + outline], ['linked', linked_document], ['operated', operated]].each do |name, bytes|
    document = TinkerPdf::Document.open(bytes)
    read_dump(name, document, out)
    document.close
  end
  out
end

COVERAGE = %w[whole-file revision suspicious].freeze
CHAIN = %w[anchored-to self-signed incomplete broken no-anchors no-signer-certificate].freeze
WEAKNESS = %w[sha1-digest sha1-signature short-rsa-key covers-only-a-revision coverage-suspicious
              outside-validity].freeze

def signatures(support)
  out = +''
  # The name written down, the fixture it is made from, and its root. The
  # altered one is ecdsa-p256.pdf with its first `verdict path` changed to
  # `verdict PATH`: only its digest moves, which is what tells the digest and
  # the signature check apart.
  [%w[ecdsa-p256 ecdsa-p256 ecdsa-p256-root], %w[pkcs7-sha1 pkcs7-sha1 pkcs7-sha1-root],
   ['document-timestamp', 'document-timestamp', nil],
   %w[ecdsa-p256-altered ecdsa-p256 ecdsa-p256-root]].each do |name, file, root|
    data = File.binread(File.join(support, "#{file}.pdf"))
    if name != file
      check(data.include?('verdict path'), 'ecdsa-p256.pdf carries the reason the alteration changes')
      data = data.sub('verdict path', 'verdict PATH')
    end
    document = TinkerPdf::Document.open(data)
    anchors = TinkerPdf::TrustAnchors.new
    anchors.add(File.binread(File.join(support, "#{root}.der"))) if root
    out << "document #{name}\n"
    document.signatures.each_with_index do |s, index|
      spans = s.spans.empty? ? '-' : s.spans.map { |start, length| "#{start}:#{length}" }.join(',')
      out << "signature #{index} #{text_token(s.field_name)} #{text_token(s.sub_filter)} " \
             "#{text_token(s.reason)} #{text_token(s.location)} #{text_token(s.name)} #{COVERAGE[s.coverage]} " \
             "#{s.covers_whole_file ? 1 : 0} #{s.usage_rights ? 1 : 0} #{s.certification_level} #{spans}\n"
    end
    [nil, 0].each do |at|
      document.verify_signatures(anchors, at).each_with_index do |v, index|
        validity = v.signer_validity.nil? ? '- -' : v.signer_validity.join(' ')
        weaknesses = v.weaknesses.empty? ? '-' : v.weaknesses.map { |w| WEAKNESS[w] }.join(',')
        out << "verdict #{at.nil? ? '-' : at} #{index} #{%w[read absent unreadable][v.cms]} " \
               "#{%w[matches differs not-checked][v.document_digest]} " \
               "#{%w[verified failed not-checked][v.signature]} #{CHAIN[v.chain]} " \
               "#{text_token(v.signer_subject)} #{text_token(v.signer_issuer)} #{validity} #{weaknesses}\n"
      end
    end
    anchors.close
    document.close
  end
  out
end

# Creates a field of every kind, applies an XFDF fixture and saves; returns
# the artefact and the form-data text's first lines.
def forms(fixture, form_data_dir)
  document = TinkerPdf::Document.open(fixture)
  editor = document.editor
  document.close
  added = [
    ['person.given', editor.add_text_field('person.given', 0, [300.0, 700.0, 500.0, 720.0], value: 'Ada', max_len: 20)],
    ['subscribe', editor.add_checkbox('subscribe', 0, [300.0, 660.0, 320.0, 680.0], 'Yes', true, flags: 2)],
    ['size', editor.add_radio_group('size', [['S', 0, [300.0, 620.0, 320.0, 640.0]],
                                             ['M', 0, [330.0, 620.0, 350.0, 640.0]]], selected: 'M')],
    ['country', editor.add_choice_field('country', 0, [300.0, 580.0, 400.0, 600.0], %w[NZ LK UK], true,
                                        value: 'LK', font_size: 10.0)],
    ['languages', editor.add_choice_field('languages', 0, [300.0, 500.0, 400.0, 560.0], %w[en fr], false)]
  ]
  lines = added.map { |name, ref| "added #{text_token(name)} #{ref.join('.')}" }
  data = TinkerPdf::FormData.read_xfdf(File.binread(File.join(form_data_dir, 'form-fields.xfdf')))
  skipped = editor.apply_form_data(data)
  data.close
  widgets = skipped.map { |w| "#{w.object}.#{w.generation}" }
  lines << "applied #{widgets.empty? ? '-' : widgets.join(',')}"
  saved = editor.save(options(TinkerPdf::WriteMode::REWRITE))
  editor.close
  [saved, lines]
end

VALUE_KINDS = %w[none text state many].freeze
FORM_WARNINGS = %w[not-read value-unreadable tree-cut unnamed].freeze

# hierarchy.fdf altered three ways, each the first occurrence replaced: the
# three warnings the fixtures never reach (the facade example says why).
HOSTILE = [['/V (plain)', '/V 12345'], ['/T (untouched)', '/X (untouched)'],
           ['/V (through a reference)', '/Kids [ 2 0 R ]']].freeze

def hostile(raw)
  HOSTILE.reduce(raw.b) do |bytes, (from, to)|
    check(bytes.include?(from), 'hierarchy.fdf carries what the alteration changes')
    bytes.sub(from, to)
  end
end

def form_data_dump(label, data, lines)
  lines << "data #{label}"
  lines << "source #{text_token(data.source)}"
  (0...data.count).each do |i|
    values = data.values(i).map { |value| " #{text_token(value)}" }.join
    lines << "field #{text_token(data.field_name(i))} #{VALUE_KINDS.fetch(data.value_kind(i))}#{values}"
  end
  data.warnings.each do |w|
    lines << "warning #{FORM_WARNINGS.fetch(w.kind)} #{text_token(w.what)} #{text_token(w.field)}"
  end
  lines << "fdf #{sha(data.to_fdf)}"
  xfdf = begin
    sha(data.to_xfdf)
  rescue TinkerPdf::Error => e
    raise unless e.status == TinkerPdf::Status::FORM_DATA_REFUSED

    'refused'
  end
  lines << "xfdf #{xfdf}"
end

def form_data_text(formed, lines, form_data_dir)
  document = TinkerPdf::Document.open(formed)
  own = document.form_data
  form_data_dump('document', own, lines)
  own.close
  document.close
  %w[form-fields.fdf hierarchy.fdf form-fields.xfdf hierarchy.xfdf].each do |file|
    raw = File.binread(File.join(form_data_dir, file))
    data = file.end_with?('.xfdf') ? TinkerPdf::FormData.read_xfdf(raw) : TinkerPdf::FormData.read_fdf(raw)
    form_data_dump(file, data, lines)
    data.close
  end
  altered = TinkerPdf::FormData.read_fdf(hostile(File.binread(File.join(form_data_dir, 'hierarchy.fdf'))))
  form_data_dump('hostile.fdf', altered, lines)
  altered.close

  built = TinkerPdf::FormData.empty
  built.source = 'built.pdf'
  built.add_field('a.b', TinkerPdf::FieldValueKind::TEXT, ["x é"])
  built.add_field('a.c', TinkerPdf::FieldValueKind::STATE, ['On'])
  built.add_field('list', TinkerPdf::FieldValueKind::MANY, %w[1 2])
  built.add_field('nothing', TinkerPdf::FieldValueKind::MANY, [])
  built.add_field('empty', TinkerPdf::FieldValueKind::NONE, [])
  form_data_dump('built', built, lines)
  built.close

  unrepresentable = TinkerPdf::FormData.empty
  unrepresentable.add_field('bell', TinkerPdf::FieldValueKind::TEXT, ["\u0007"])
  form_data_dump('unrepresentable', unrepresentable, lines)
  unrepresentable.close

  [['read-fdf', :read_fdf, 'not form data'], ['read-xfdf', :read_xfdf, '<root/>']].each do |label, reader, raw|
    TinkerPdf::FormData.public_send(reader, raw).close
    lines << "#{label} accepted"
  rescue TinkerPdf::Error => e
    raise unless e.status == TinkerPdf::Status::FORM_DATA_REFUSED

    lines << "#{label} refused"
  end
  lines.map { |line| "#{line}\n" }.join
end

if ARGV.size != 1
  warn 'usage: write_parity.rb <form-fields.pdf>'
  exit 2
end
fixture_path = File.expand_path(ARGV[0])
fixture = File.binread(fixture_path)
outline = File.binread(File.join(File.dirname(fixture_path), 'outline-3level.pdf'))

report('fill-and-save', fill_and_save(fixture))
report('build-a-document', build_a_document)
operated = document_ops(outline)
report('document-ops', operated)
sanitised, removed = sanitise(operated)
report('sanitise', sanitised)
report('save-options', save_options(operated, false))
report('save-linearized', save_options(operated, true))
report_read('sanitise-report', removed)
report_read('read-surface', read_surface(outline, operated))
support = File.join(File.dirname(File.dirname(fixture_path)), 'crates', 'tinker-pdf', 'tests', 'signature_support')
report_read('signatures', signatures(support))
form_data_dir = File.join(File.dirname(support), 'form_data')
formed, form_lines = forms(fixture, form_data_dir)
report('forms', formed)
report_read('form-data', form_data_text(formed, form_lines, form_data_dir))
puts 'RUBY-PARITY: RAN'
