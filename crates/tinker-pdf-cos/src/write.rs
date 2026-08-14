//! Serializing objects and documents (7.5).
//!
//! Two shapes of output, and the difference matters more than it looks.
//!
//! A **full rewrite** renumbers and emits everything, which is how a document
//! is compacted or garbage-collected. An **incremental update** appends: the
//! original bytes stay byte-for-byte identical as a prefix and only changed
//! objects are written after them, with a new cross-reference section chained
//! to the old one. That prefix invariance is not a nicety — a digital
//! signature covers a byte range of the original file, so a signed document
//! can only be updated this way, and phase 10's signing support is built on
//! exactly this.

use std::collections::BTreeMap;

use crate::name::{Name, NameTable};
use crate::object::{Dict, Object, PdfString};

/// How a document should be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Emit every object afresh, renumbering from one.
    Rewrite,
    /// Append changed objects to the original bytes.
    Incremental,
}

/// How a document should be encrypted on save.
#[derive(Clone, Debug)]
pub struct Encryption {
    /// The password a reader needs to open the document. Empty means none.
    pub user_password: String,
    /// The password that lifts the document's restrictions.
    pub owner_password: String,
    /// The permission bits, as `/P` stores them.
    pub permissions: i32,
    /// Forty-eight bytes of randomness: the 32-byte file key and two 8-byte
    /// salts.
    ///
    /// Supplied by the caller because this crate has no opinion about where
    /// randomness comes from, and `wasm32-unknown-unknown` has no source of it
    /// at all. Passing predictable bytes produces a predictably weak document
    /// — which is the caller's decision, and honest, where pretending to have
    /// entropy would not be.
    pub entropy: [u8; 48],
}

/// Options for writing.
#[derive(Clone, Debug)]
pub struct WriteOptions {
    /// Which shape of output.
    pub mode: WriteMode,
    /// Lay the file out for the first page to arrive first (Annex F).
    ///
    /// Off by default. Hints are advisory and most viewers ignore them; the
    /// practical win is HTTP range serving and the workflows that check "fast
    /// web view" as a box.
    ///
    /// It is a request, not a guarantee, and it is quietly dropped in two
    /// cases — each because the alternative is a file that claims
    /// `/Linearized` and is not, which is worse than an ordinary file because
    /// a reader will believe it:
    ///
    /// - **An incremental update**, which by its nature appends to whatever
    ///   layout the original had. (An incremental update to a linearized file
    ///   also de-linearizes it, by nature rather than by bug: the parameter
    ///   dictionary goes stale and validators say so.)
    /// - **A document with no catalog or no pages**, which has no first page
    ///   to put first.
    ///
    /// Both are definitions rather than limitations. Encryption used to be a
    /// third case and is not one any more: the linearized writer encrypts each
    /// object as it serialises it and measures the layout from the ciphertext,
    /// so the padding AES-256-CBC adds is inside every offset the file
    /// declares.
    ///
    /// `object_streams` is ignored when this succeeds: packing objects into a
    /// container would put the first page's objects in the same blob as
    /// everything else, which is the opposite of the point.
    pub linearize: bool,
    /// The PDF version to declare in the header, on a rewrite.
    pub version: (u8, u8),
    /// Pack eligible objects into object streams (7.5.7).
    ///
    /// Smaller files, because the packed objects compress together and their
    /// cross-reference entries shrink. Streams, and anything a reader must
    /// find before decryption, cannot go in one.
    pub object_streams: bool,
    /// Compress content streams the caller has not already encoded.
    pub compress: bool,
    /// Encrypt on save. Requires an entropy source at the call site.
    pub encryption: Option<Encryption>,
    /// Drop objects nothing reaches from the trailer, on a rewrite.
    ///
    /// Off by default, because "rewrite" has meant "serialize everything"
    /// since the writer was written and some callers rely on it — redaction
    /// overwrites a content stream in place precisely *because* an
    /// unreferenced object would otherwise survive with the original text in
    /// it, and that is a guarantee it should keep making for itself rather
    /// than inheriting from a flag.
    pub garbage_collect: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            mode: WriteMode::Rewrite,
            linearize: false,
            version: (1, 7),
            object_streams: false,
            compress: false,
            encryption: None,
            garbage_collect: false,
        }
    }
}

/// Escapes and writes a literal string (7.3.4.2).
fn write_string(out: &mut Vec<u8>, s: &PdfString) {
    if s.hex {
        out.push(b'<');
        for byte in &s.bytes {
            out.extend_from_slice(format!("{byte:02X}").as_bytes());
        }
        out.push(b'>');
        return;
    }

    out.push(b'(');
    for &byte in &s.bytes {
        match byte {
            // Only these three need escaping in a literal string; escaping
            // more is legal but makes files bigger and diffs noisier.
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(byte);
            }
            b'\r' => out.extend_from_slice(b"\\r"),
            _ => out.push(byte),
        }
    }
    out.push(b')');
}

/// Writes a name, escaping what 7.3.5 requires.
fn write_name(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'/');
    for &byte in bytes {
        // Delimiters, whitespace, '#' itself and anything outside the
        // printable range must be written as #xx.
        let needs_escape = byte <= b' '
            || byte >= 0x7F
            || matches!(
                byte,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
            );
        if needs_escape {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
    }
}

/// Formats a real number without an exponent, which PDF does not allow.
fn write_real(out: &mut Vec<u8>, value: f64) {
    if !value.is_finite() {
        out.extend_from_slice(b"0");
        return;
    }
    if value == value.trunc() && value.abs() < 1e15 {
        out.extend_from_slice(format!("{}", value as i64).as_bytes());
        return;
    }
    // Six decimals is more precision than any coordinate needs, and trailing
    // zeros are trimmed so the output stays compact.
    let text = format!("{value:.6}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(if trimmed.is_empty() { "0" } else { trimmed }.as_bytes());
}

/// Writes one object.
pub fn write_object(out: &mut Vec<u8>, object: &Object, names: &NameTable) {
    write_object_at(out, object, names, 0);
}

fn write_object_at(out: &mut Vec<u8>, object: &Object, names: &NameTable, depth: u32) {
    if depth > crate::limits::MAX_NEST_DEPTH {
        out.extend_from_slice(b"null");
        return;
    }

    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Bool(true) => out.extend_from_slice(b"true"),
        Object::Bool(false) => out.extend_from_slice(b"false"),
        Object::Int(v) => out.extend_from_slice(v.to_string().as_bytes()),
        Object::Real(v) => write_real(out, *v),
        Object::String(s) => write_string(out, s),
        Object::Name(n) => {
            let bytes = names.bytes(*n).unwrap_or_default();
            write_name(out, &bytes);
        }
        Object::Ref(r) => {
            out.extend_from_slice(format!("{} {} R", r.num, r.gen).as_bytes());
        }
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object_at(out, item, names, depth + 1);
            }
            out.push(b']');
        }
        Object::Dict(dict) => write_dict(out, dict, names, depth),
        // A parsed stream refers to bytes in the document it came from, so it
        // cannot be written from the object alone. Emitting one needs its
        // data, which is what `Written::Stream` carries; here only the
        // dictionary is available and only that is written.
        Object::Stream(stream) => write_dict(out, &stream.dict, names, depth),
    }
}

fn write_dict(out: &mut Vec<u8>, dict: &Dict, names: &NameTable, depth: u32) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        let bytes = names.bytes(*key).unwrap_or_default();
        write_name(out, &bytes);
        out.push(b' ');
        write_object_at(out, value, names, depth + 1);
    }
    out.extend_from_slice(b">>");
}

/// A stream ready to be written: a dictionary and its already-encoded bytes.
#[derive(Clone, Debug, Default)]
pub struct StreamData {
    /// The stream dictionary, without `/Length`, which is written for it.
    pub dict: Dict,
    /// The encoded bytes.
    pub data: Vec<u8>,
}

/// One thing a writer emits.
#[derive(Clone, Debug)]
pub enum Written {
    /// A plain object.
    Object(Object),
    /// A stream, whose data the writer owns and whose `/Length` it computes.
    Stream(StreamData),
}

/// What a writer emits: object numbers to objects.
#[derive(Clone, Debug, Default)]
pub struct ObjectSet {
    entries: BTreeMap<u32, Written>,
    /// Trailer entries a cross-reference stream must carry in its own
    /// dictionary, since such a file has no `trailer` keyword (7.5.8.2).
    trailer_hints: Dict,
}

impl ObjectSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> ObjectSet {
        ObjectSet::default()
    }

    /// Adds or replaces an object.
    pub fn insert(&mut self, num: u32, object: Object) {
        self.entries.insert(num, Written::Object(object));
    }

    /// Adds or replaces a stream.
    pub fn insert_stream(&mut self, num: u32, stream: StreamData) {
        self.entries.insert(num, Written::Stream(stream));
    }

    /// Records a trailer entry for a cross-reference stream to carry.
    pub fn set_trailer_hint(&mut self, key: Name, value: Object) {
        self.trailer_hints.insert(key, value);
    }

    /// How many objects the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every object number in the set, ascending.
    #[must_use]
    pub fn numbers(&self) -> Vec<u32> {
        self.entries.keys().copied().collect()
    }

    /// One object, by number.
    #[must_use]
    pub fn get(&self, num: u32) -> Option<&Written> {
        self.entries.get(&num)
    }

    /// Every object, in ascending number order.
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &Written)> {
        self.entries.iter()
    }

    /// The highest object number in the set, or zero when it is empty.
    #[must_use]
    pub fn max_number(&self) -> u32 {
        self.entries.keys().next_back().copied().unwrap_or(0)
    }
}

/// Encrypts strings and streams on the way out (7.6.2).
///
/// R6 only. Earlier revisions derive a key per object; R6 uses the file key
/// directly, which is why nothing here needs the generation number.
pub(crate) struct StreamCipher {
    key: [u8; 32],
    /// Seed for the per-object initialisation vectors.
    ///
    /// CBC needs a different IV per message or two identical plaintexts
    /// encrypt identically and the fact leaks. Derived from the file key and
    /// the object number rather than asked for separately: it must be
    /// unpredictable to an attacker without the key, and it is.
    seed: [u8; 32],
}

impl StreamCipher {
    fn new(key: [u8; 32]) -> StreamCipher {
        let mut seed_input = Vec::with_capacity(40);
        seed_input.extend_from_slice(&key);
        seed_input.extend_from_slice(b"tpdf-iv");
        StreamCipher {
            key,
            seed: tinker_pdf_crypto::sha2::sha256(&seed_input),
        }
    }

    fn iv_for(&self, num: u32, counter: u32) -> [u8; 16] {
        let mut input = Vec::with_capacity(40);
        input.extend_from_slice(&self.seed);
        input.extend_from_slice(&num.to_be_bytes());
        input.extend_from_slice(&counter.to_be_bytes());
        let digest = tinker_pdf_crypto::sha2::sha256(&input);
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&digest[..16]);
        iv
    }

    pub(crate) fn encrypt_stream(&self, data: &[u8], num: u32) -> Vec<u8> {
        tinker_pdf_crypto::handler::encrypt_aes256(&self.key, &self.iv_for(num, 0), data)
    }

    /// A copy of `object` with every string inside it encrypted.
    pub(crate) fn encrypt_strings(&self, object: &Object, num: u32) -> Object {
        let mut counter = 1u32;
        self.walk(object, num, &mut counter)
    }

    fn walk(&self, object: &Object, num: u32, counter: &mut u32) -> Object {
        match object {
            Object::String(s) => {
                let iv = self.iv_for(num, *counter);
                *counter = counter.saturating_add(1);
                let bytes = tinker_pdf_crypto::handler::encrypt_aes256(&self.key, &iv, &s.bytes);
                // Hex, because ciphertext is arbitrary bytes and a literal
                // string would need every one of them escaped.
                Object::String(PdfString::hex(bytes))
            }
            Object::Array(items) => Object::Array(
                items
                    .iter()
                    .map(|item| self.walk(item, num, counter))
                    .collect(),
            ),
            Object::Dict(dict) => {
                let mut out = Dict::with_capacity(dict.len());
                for (key, value) in dict.iter() {
                    out.insert(*key, self.walk(value, num, counter));
                }
                Object::Dict(out)
            }
            other => other.clone(),
        }
    }
}

/// Builds the `/Encrypt` dictionary and the cipher that goes with it.
pub(crate) fn build_encryption(
    encryption: &Encryption,
    names: &NameTable,
) -> Option<(Dict, StreamCipher)> {
    let built = tinker_pdf_crypto::handler::build_r6(
        encryption.user_password.as_bytes(),
        encryption.owner_password.as_bytes(),
        encryption.permissions,
        true,
        &encryption.entropy,
    )?;

    let mut dict = Dict::new();
    dict.insert(Name::FILTER, Object::Name(names.intern(b"Standard")));
    dict.insert(names.intern(b"V"), Object::Int(5));
    dict.insert(names.intern(b"R"), Object::Int(6));
    dict.insert(names.intern(b"Length"), Object::Int(256));
    dict.insert(
        names.intern(b"P"),
        Object::Int(i64::from(encryption.permissions)),
    );
    dict.insert(names.intern(b"U"), Object::String(PdfString::hex(built.u)));
    dict.insert(
        names.intern(b"UE"),
        Object::String(PdfString::hex(built.ue)),
    );
    dict.insert(names.intern(b"O"), Object::String(PdfString::hex(built.o)));
    dict.insert(
        names.intern(b"OE"),
        Object::String(PdfString::hex(built.oe)),
    );
    dict.insert(
        names.intern(b"Perms"),
        Object::String(PdfString::hex(built.perms)),
    );
    dict.insert(names.intern(b"EncryptMetadata"), Object::Bool(true));

    // 7.6.5: V5 names its crypt filters, and /StmF and /StrF say which applies
    // to streams and strings. Without them a reader falls back to /Identity
    // and reads ciphertext as content.
    let mut std_cf = Dict::new();
    std_cf.insert(names.intern(b"CFM"), Object::Name(names.intern(b"AESV3")));
    std_cf.insert(
        names.intern(b"AuthEvent"),
        Object::Name(names.intern(b"DocOpen")),
    );
    std_cf.insert(names.intern(b"Length"), Object::Int(32));
    let mut cf = Dict::new();
    cf.insert(names.intern(b"StdCF"), Object::Dict(std_cf));
    dict.insert(names.intern(b"CF"), Object::Dict(cf));
    dict.insert(names.intern(b"StmF"), Object::Name(names.intern(b"StdCF")));
    dict.insert(names.intern(b"StrF"), Object::Name(names.intern(b"StdCF")));

    Some((dict, StreamCipher::new(built.file_key)))
}

/// Compresses a stream's data and records the filter, when asked and when it
/// helps.
///
/// A stream that already declares a `/Filter` is left alone: its bytes are
/// already encoded, and wrapping them again would need the filter array
/// rewritten and would usually make them larger. A result no smaller than the
/// input is discarded for the same reason — an already-compressed image does
/// not compress twice.
pub(crate) fn maybe_compress(
    data: &[u8],
    dict: &mut Dict,
    names: &NameTable,
    compress: bool,
) -> Vec<u8> {
    if !compress || data.is_empty() || dict.contains_key(Name::FILTER) {
        return data.to_vec();
    }

    let packed = tinker_pdf_filters::zlib_compress(data);
    if packed.len() >= data.len() {
        return data.to_vec();
    }

    let flate = names.intern(b"FlateDecode");
    dict.insert(Name::FILTER, Object::Name(flate));
    packed
}

/// Writes one numbered object, whichever kind it is.
fn write_entry(
    out: &mut Vec<u8>,
    num: u32,
    entry: &Written,
    names: &NameTable,
    compress: bool,
    crypt: Option<&StreamCipher>,
) {
    out.extend_from_slice(
        format!(
            "{num} 0 obj
"
        )
        .as_bytes(),
    );
    match entry {
        Written::Object(object) => match crypt {
            // 7.6.2: every string in the file is encrypted too, not only the
            // streams. Leaving them in the clear puts titles, form values and
            // annotation contents in plain sight inside a file that claims to
            // be encrypted.
            Some(cipher) => write_object(out, &cipher.encrypt_strings(object, num), names),
            None => write_object(out, object, names),
        },
        Written::Stream(stream) => {
            // 7.3.8.2: /Length must agree with the bytes that follow, so the
            // writer computes it rather than trusting whatever was there.
            let mut dict = stream.dict.clone();
            let data = maybe_compress(&stream.data, &mut dict, names, compress);
            // Encryption is the last thing applied and the first thing undone,
            // so it wraps the compressed bytes rather than the other way round.
            let data = match crypt {
                Some(cipher) => cipher.encrypt_stream(&data, num),
                None => data,
            };
            dict.insert(Name::LENGTH, Object::Int(data.len() as i64));
            write_dict(out, &dict, names, 0);
            out.extend_from_slice(
                b"
stream
",
            );
            out.extend_from_slice(&data);
            out.extend_from_slice(
                b"
endstream",
            );
        }
    }
    out.extend_from_slice(
        b"
endobj
",
    );
}

/// Appends an incremental update to `original`.
///
/// The result **starts with `original`, byte for byte**. That invariant is
/// what lets a signed document be updated without breaking its signature, and
/// it is asserted by tests rather than assumed.
///
/// `trailer` supplies `/Root`, `/Info` and `/ID`; `/Prev` and `/Size` are
/// written for it.
#[must_use]
pub fn incremental_update(
    original: &[u8],
    changed: &ObjectSet,
    trailer: &Dict,
    previous_startxref: u64,
    names: &NameTable,
    compress: bool,
) -> Vec<u8> {
    let mut out = original.to_vec();

    // 7.5.6: an update begins on a new line so the appended section cannot be
    // mistaken for a continuation of whatever the original ended with.
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    let mut offsets: Vec<(u32, u64)> = Vec::with_capacity(changed.entries.len());
    for (num, object) in &changed.entries {
        offsets.push((*num, out.len() as u64));
        // An incremental update inherits the original file's encryption,
        // which this build cannot reproduce without its key — so it refuses
        // rather than appending plaintext into a ciphertext file.
        write_entry(&mut out, *num, object, names, compress, None);
    }

    let xref_at = out.len() as u64;
    write_classic_xref(&mut out, &offsets);

    let mut trailer = trailer.clone();
    // The *changed* set's highest number is not the file's. Writing it
    // unqualified clobbered the correct value the caller passes in, and a
    // conforming reader must then ignore every object above it — including
    // the edited page's own `/Contents`. It can only ever go up.
    let existing = trailer
        .get(Name::SIZE)
        .and_then(Object::as_int)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let size = changed.max_number().saturating_add(1).max(existing);
    trailer.insert(Name::SIZE, Object::Int(i64::from(size)));
    trailer.insert(
        Name::PREV,
        Object::Int(i64::try_from(previous_startxref).unwrap_or(0)),
    );

    out.extend_from_slice(b"trailer\n");
    write_dict(&mut out, &trailer, names, 0);
    out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());

    out
}

/// Writes a whole document afresh.
#[must_use]
pub fn rewrite(
    objects: &ObjectSet,
    trailer: &Dict,
    options: &WriteOptions,
    names: &NameTable,
) -> Vec<u8> {
    if options.linearize {
        // A document with no catalog or no pages has no first page to put
        // first; the ordinary layout is then the only honest one, rather than
        // a file claiming /Linearized and not being.
        //
        // Encryption used to be a second reason to fall through here, and the
        // caller was told nothing: they asked for both and got an encrypted
        // file with an ordinary layout. `linearize` owns the cipher now.
        if let Some(bytes) = crate::linearize::linearize(objects, trailer, options, names) {
            return bytes;
        }
    }

    let (major, minor) = options.version;
    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(format!("%PDF-{major}.{minor}\n").as_bytes());
    // 7.5.2: four bytes above 127 tell transfer software the file is binary.
    out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

    // 7.6.1: the /Encrypt dictionary is written in the clear and everything
    // else is not. Built first so the cipher exists before the first object.
    let encryption = options
        .encryption
        .as_ref()
        .and_then(|e| build_encryption(e, names));
    let crypt = encryption.as_ref().map(|(_, cipher)| cipher);

    let mut offsets: Vec<(u32, u64)> = Vec::with_capacity(objects.entries.len());

    // 7.5.7: an object stream holds objects that are not themselves streams.
    // Packing them costs one container and saves each object's own header,
    // and the container compresses them together.
    let packed: Vec<u32> = if options.object_streams {
        objects
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, Written::Object(o) if packable(o)))
            .map(|(num, _)| *num)
            .collect()
    } else {
        Vec::new()
    };

    for (num, object) in &objects.entries {
        if packed.contains(num) {
            continue;
        }
        offsets.push((*num, out.len() as u64));
        write_entry(&mut out, *num, object, names, options.compress, crypt);
    }

    let container = objects.max_number().saturating_add(1);
    if !packed.is_empty() {
        let mut prologue = Vec::new();
        let mut body = Vec::new();
        for num in &packed {
            if let Some(Written::Object(object)) = objects.entries.get(num) {
                // The prologue pairs each object number with where its value
                // begins, measured from /First.
                prologue.extend_from_slice(format!("{num} {} ", body.len()).as_bytes());
                write_object(&mut body, object, names);
                body.push(b' ');
            }
        }

        let first = prologue.len();
        let mut data = prologue;
        data.extend_from_slice(&body);

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(names.intern(b"ObjStm")));
        dict.insert(Name::N, Object::Int(packed.len() as i64));
        dict.insert(Name::FIRST, Object::Int(first as i64));

        offsets.push((container, out.len() as u64));
        write_entry(
            &mut out,
            container,
            &Written::Stream(StreamData { dict, data }),
            names,
            // The container is compressed whenever compression is on at all:
            // packing many small objects together and then leaving the result
            // uncompressed makes the file *larger* than not packing it, which
            // is the opposite of the reason object streams exist.
            options.compress,
            crypt,
        );
    }

    // 7.6.1: the /Encrypt dictionary is the one object never encrypted, since
    // a reader needs it to decrypt everything else.
    let mut trailer = trailer.clone();
    if let Some((dict, _)) = &encryption {
        // Directly above the object-stream container, which is `max + 1`.
        // A gap here is not merely untidy: the cross-reference table is sized
        // from the numbers actually used, and a number nobody claims is a
        // free entry a reader will honour.
        let encrypt_num = objects.max_number().saturating_add(2);
        offsets.push((encrypt_num, out.len() as u64));
        write_entry(
            &mut out,
            encrypt_num,
            &Written::Object(Object::Dict(dict.clone())),
            names,
            false,
            None,
        );
        trailer.insert(
            Name::ENCRYPT,
            Object::Ref(crate::object::ObjRef::new(encrypt_num, 0)),
        );
    }
    let trailer = &trailer;

    let xref_at = out.len() as u64;
    // Objects inside a container need a cross-reference stream to be found:
    // a classic table can say "free" or "at this offset" and nothing else.
    if packed.is_empty() {
        write_classic_xref(&mut out, &offsets);
    } else {
        write_xref_stream(
            &mut out, &offsets, &packed, container, trailer, objects, names,
        );
    }

    // A rewrite has no earlier revision, so /Prev is simply not written; the
    // caller's trailer is not expected to carry one.
    let mut trailer = trailer.clone();
    // 7.5.5 Table 15: one more than the highest object number *in the file*,
    // which includes the container and the `/Encrypt` dictionary. Taking it
    // from the object set alone understated it on every encrypted file.
    let highest = offsets
        .iter()
        .map(|(num, _)| *num)
        .max()
        .unwrap_or_else(|| objects.max_number());
    trailer.insert(
        Name::SIZE,
        Object::Int(i64::from(highest.saturating_add(1))),
    );

    if packed.is_empty() {
        out.extend_from_slice(b"trailer\n");
        write_dict(&mut out, &trailer, names, 0);
    }
    out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());

    out
}

/// Whether an object may live inside an object stream (7.5.7).
///
/// A stream cannot, because its data lies outside the object. Nor may
/// anything a reader must read before decryption — but this writer packs only
/// what the caller handed it, and the encryption dictionary is never in that
/// set.
fn packable(object: &Object) -> bool {
    !matches!(object, Object::Stream(_))
}

/// Writes a cross-reference stream (7.5.8).
///
/// Needed rather than preferred once objects live in containers: a classic
/// table has only "free" and "at this offset", and no way to say "object N is
/// the k-th in stream M".
#[allow(clippy::too_many_arguments)]
fn write_xref_stream(
    out: &mut Vec<u8>,
    offsets: &[(u32, u64)],
    packed: &[u32],
    container: u32,
    trailer: &Dict,
    objects: &ObjectSet,
    names: &NameTable,
) {
    let stream_number = container.saturating_add(1);
    // Every object that has an offset, not just the container. `/Encrypt` is
    // numbered above it, and sizing from the container alone left that object
    // with no entry at all: the file reopened as unencrypted with zero pages
    // while its content was genuinely ciphertext, which is unrecoverable.
    let highest = offsets
        .iter()
        .map(|(num, _)| *num)
        .max()
        .unwrap_or(stream_number)
        .max(stream_number);
    let size = highest.saturating_add(1);

    // Three bytes of offset covers 16 MB; four covers 4 GB, which is past
    // what any single PDF should be.
    let mut data = Vec::with_capacity((size as usize) * 8);
    for num in 0..size {
        if num == 0 {
            // Object zero heads the free list.
            data.push(0);
            data.extend_from_slice(&[0, 0, 0, 0]);
            data.extend_from_slice(&[255, 255]);
            continue;
        }
        if let Some((_, offset)) = offsets.iter().find(|(n, _)| *n == num) {
            data.push(1);
            data.extend_from_slice(&(*offset as u32).to_be_bytes());
            data.extend_from_slice(&0u16.to_be_bytes());
        } else if let Some(index) = packed.iter().position(|n| *n == num) {
            data.push(2);
            data.extend_from_slice(&container.to_be_bytes());
            data.extend_from_slice(&(index as u16).to_be_bytes());
        } else if num == stream_number {
            data.push(1);
            data.extend_from_slice(&(out.len() as u32).to_be_bytes());
            data.extend_from_slice(&0u16.to_be_bytes());
        } else {
            data.push(0);
            data.extend_from_slice(&[0, 0, 0, 0]);
            data.extend_from_slice(&[255, 255]);
        }
    }

    let mut dict = Dict::new();

    // 7.5.8.2: a file with a cross-reference stream has no `trailer` keyword,
    // so /Root and the rest live in the stream's own dictionary.
    //
    // The caller's trailer goes in *first*. It is the trailer of the document
    // this one was built from, and it carries that document's /Size — which
    // is smaller than this one's, because the container and this stream are
    // both new objects. Merging it afterwards silently overwrote the correct
    // /Size with the stale one, and a reader that trusts /Size then drops
    // every entry past it: the container became unreachable, every packed
    // object read as null, and the file opened with no pages at all.
    for (key, value) in trailer.iter() {
        dict.insert(*key, value.clone());
    }
    for (key, value) in objects.trailer_hints.iter() {
        dict.insert(*key, value.clone());
    }

    // The three the writer owns, and which no inherited value may override.
    dict.insert(Name::TYPE, Object::Name(names.intern(b"XRef")));
    dict.insert(Name::SIZE, Object::Int(i64::from(size)));
    dict.insert(
        Name::W,
        Object::Array(vec![Object::Int(1), Object::Int(4), Object::Int(2)]),
    );
    // A stale /Prev would point at a revision this file does not contain.
    let dict = crate::edit::without(&dict, Name::PREV);

    // Not compressed: a cross-reference stream is what a reader finds *first*,
    // by offset, before it knows anything about filters, and its entries are
    // already packed into fixed-width fields.
    write_entry(
        out,
        stream_number,
        &Written::Stream(StreamData { dict, data }),
        names,
        false,
        // 7.6.2: a cross-reference stream is never encrypted — a reader finds
        // it before it knows there is an /Encrypt dictionary to look for.
        None,
    );
}

/// Writes a classic cross-reference table (7.5.4).
///
/// Entries are grouped into contiguous subsections, which is what makes an
/// incremental update's table small: only the objects that changed appear.
fn write_classic_xref(out: &mut Vec<u8>, offsets: &[(u32, u64)]) {
    out.extend_from_slice(b"xref\n");

    if offsets.is_empty() {
        // A table with only the free head; the specification requires object
        // zero to exist and be free.
        out.extend_from_slice(b"0 1\n0000000000 65535 f \n");
        return;
    }

    let mut index = 0usize;
    while index < offsets.len() {
        let start = match offsets.get(index) {
            Some((num, _)) => *num,
            None => break,
        };

        // Find how far the run of consecutive numbers extends.
        let mut end = index;
        while let (Some((a, _)), Some((b, _))) = (offsets.get(end), offsets.get(end + 1)) {
            if b.saturating_sub(*a) == 1 {
                end += 1;
            } else {
                break;
            }
        }

        let count = end - index + 1;
        // Object zero heads the free list, so a run starting at 1 must
        // include it or readers reject the table.
        if start == 1 {
            out.extend_from_slice(format!("0 {}\n", count + 1).as_bytes());
            out.extend_from_slice(b"0000000000 65535 f \n");
        } else {
            out.extend_from_slice(format!("{start} {count}\n").as_bytes());
        }

        for (_, offset) in offsets.get(index..=end).unwrap_or_default() {
            // 7.5.4: exactly twenty bytes per entry, including the trailing
            // two-character end-of-line.
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        index = end + 1;
    }
}

/// Where a signature's byte range would go, reserved so it can be patched.
///
/// 12.8.1 needs a signature's `/Contents` to be a hex string large enough for
/// the eventual signature, and its `/ByteRange` to name the spans of the file
/// the signature covers — which cannot be known until the file is laid out.
/// Reserving both and patching afterwards is the only way round the circle,
/// and this records where to patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignaturePlaceholder {
    /// Byte offset of the `/Contents` hex string, at its opening angle.
    pub contents_at: usize,
    /// How many bytes the placeholder occupies, brackets included.
    pub contents_len: usize,
    /// Byte offset of the `/ByteRange` array.
    pub byte_range_at: usize,
    /// How many bytes that array occupies.
    pub byte_range_len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjRef;

    fn names() -> NameTable {
        NameTable::new()
    }

    fn rendered(object: &Object) -> String {
        let mut out = Vec::new();
        write_object(&mut out, object, &names());
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn scalars_write_as_the_syntax_says() {
        assert_eq!(rendered(&Object::Null), "null");
        assert_eq!(rendered(&Object::Bool(true)), "true");
        assert_eq!(rendered(&Object::Int(-42)), "-42");
        assert_eq!(rendered(&Object::Ref(ObjRef::new(3, 0))), "3 0 R");
    }

    #[test]
    fn reals_never_use_an_exponent() {
        // PDF has no exponent notation, so 1e-7 must not be written as such.
        assert_eq!(rendered(&Object::Real(0.5)), "0.5");
        assert_eq!(rendered(&Object::Real(2.0)), "2");
        assert_eq!(rendered(&Object::Real(-0.25)), "-0.25");
        assert!(!rendered(&Object::Real(1e-7)).contains('e'));
        assert!(!rendered(&Object::Real(1e20)).contains('e'));
        // Non-finite values cannot be written at all, so they become zero.
        assert_eq!(rendered(&Object::Real(f64::NAN)), "0");
        assert_eq!(rendered(&Object::Real(f64::INFINITY)), "0");
    }

    #[test]
    fn strings_escape_only_what_they_must() {
        assert_eq!(
            rendered(&Object::String(PdfString::literal(b"plain".to_vec()))),
            "(plain)"
        );
        assert_eq!(
            rendered(&Object::String(PdfString::literal(b"a(b)c\\d".to_vec()))),
            r"(a\(b\)c\\d)"
        );
    }

    #[test]
    fn names_escape_delimiters_and_whitespace() {
        let table = names();
        let mut out = Vec::new();
        write_object(&mut out, &Object::Name(table.intern(b"A B")), &table);
        assert_eq!(String::from_utf8_lossy(&out), "/A#20B");

        let mut out = Vec::new();
        write_object(&mut out, &Object::Name(table.intern(b"Plain")), &table);
        assert_eq!(String::from_utf8_lossy(&out), "/Plain");
    }

    #[test]
    fn a_streams_length_is_written_for_it() {
        let table = names();
        let mut out = Vec::new();
        write_entry(
            &mut out,
            4,
            &Written::Stream(StreamData {
                dict: Dict::new(),
                data: b"hello".to_vec(),
            }),
            &table,
            false,
            None,
        );
        let text = String::from_utf8_lossy(&out);

        assert!(text.contains("/Length 5"), "got {text}");
        assert!(text.contains("stream\nhello\nendstream"), "got {text}");
    }

    #[test]
    fn a_rewrite_produces_a_readable_document() {
        let table = names();
        let mut objects = ObjectSet::new();

        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(table.intern(b"Catalog")));
        objects.insert(1, Object::Dict(catalog));

        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));

        let bytes = rewrite(&objects, &trailer, &WriteOptions::default(), &table);
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.starts_with("%PDF-1.7"), "a header");
        assert!(text.contains("1 0 obj"), "the object");
        assert!(text.contains("xref"), "a table");
        assert!(text.contains("/Root 1 0 R"), "a trailer");
        assert!(text.ends_with("%%EOF\n"), "and an end marker");

        // And it reopens.
        let doc = crate::CosDocument::open(bytes.clone()).expect("the output reopens");
        assert!(doc.catalog().is_some(), "with a catalog");
    }

    /// The invariant phase 10's signing depends on.
    #[test]
    fn an_incremental_update_preserves_the_original_bytes_exactly() {
        let table = names();
        let mut objects = ObjectSet::new();
        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(table.intern(b"Catalog")));
        objects.insert(1, Object::Dict(catalog));
        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));

        let original = rewrite(&objects, &trailer, &WriteOptions::default(), &table);

        let mut changed = ObjectSet::new();
        changed.insert(2, Object::Int(42));
        let updated = incremental_update(&original, &changed, &trailer, 0, &table, false);

        assert!(
            updated.starts_with(&original),
            "the original must survive byte for byte"
        );
        assert!(updated.len() > original.len(), "and something was appended");

        let text = String::from_utf8_lossy(&updated);
        assert!(text.contains("/Prev"), "the update chains to the old table");
        assert!(text.matches("%%EOF").count() >= 2, "two revisions");

        // The updated file still opens, and sees the new object.
        let doc = crate::CosDocument::open(updated).expect("the update reopens");
        assert_eq!(
            doc.get(ObjRef::new(2, 0)).ok().as_deref(),
            Some(&Object::Int(42))
        );
    }

    #[test]
    fn the_cross_reference_table_always_has_its_free_head() {
        let mut out = Vec::new();
        write_classic_xref(&mut out, &[(1, 100), (2, 200)]);
        let text = String::from_utf8_lossy(&out);

        assert!(text.starts_with("xref\n0 3\n"), "got {text}");
        assert!(text.contains("0000000000 65535 f \n"), "the free head");
        assert!(text.contains("0000000100 00000 n \n"), "object one");

        // Every entry is exactly twenty bytes.
        for line in text.lines().skip(2) {
            if line.ends_with(" n ") || line.ends_with(" f ") {
                assert_eq!(line.len() + 1, 20, "entry {line:?} is not 20 bytes");
            }
        }
    }

    #[test]
    fn non_consecutive_objects_become_separate_subsections() {
        let mut out = Vec::new();
        write_classic_xref(&mut out, &[(1, 10), (2, 20), (7, 70)]);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("0 3\n"), "the first run, with the free head");
        assert!(text.contains("7 1\n"), "and a second subsection: {text}");
    }

    #[test]
    fn deeply_nested_objects_do_not_overflow_the_stack() {
        let mut object = Object::Int(0);
        for _ in 0..(crate::limits::MAX_NEST_DEPTH + 100) {
            object = Object::Array(vec![object]);
        }
        let mut out = Vec::new();
        write_object(&mut out, &object, &names());
        assert!(!out.is_empty());
    }
}

#[cfg(test)]
mod objstm_tests {
    use super::*;
    use crate::object::ObjRef;
    use crate::CosDocument;

    /// Objects packed into a container must still be findable, which needs a
    /// cross-reference stream — a classic table cannot express "object N is
    /// inside stream M".
    #[test]
    fn objects_packed_into_a_stream_are_still_readable() {
        let table = NameTable::new();
        let mut objects = ObjectSet::new();

        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(table.intern(b"Catalog")));
        catalog.insert(Name::PAGES, Object::Ref(ObjRef::new(2, 0)));
        objects.insert(1, Object::Dict(catalog));

        let mut pages = Dict::new();
        pages.insert(Name::TYPE, Object::Name(table.intern(b"Pages")));
        pages.insert(Name::COUNT, Object::Int(0));
        pages.insert(Name::KIDS, Object::Array(Vec::new()));
        objects.insert(2, Object::Dict(pages));
        objects.insert(3, Object::Int(1234));

        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));

        let options = WriteOptions {
            object_streams: true,
            ..WriteOptions::default()
        };
        let bytes = rewrite(&objects, &trailer, &options, &table);

        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/ObjStm"), "a container was written");
        assert!(text.contains("/XRef"), "and a cross-reference stream");
        assert!(
            !text.contains("\ntrailer\n"),
            "which replaces the trailer keyword entirely"
        );

        let doc = CosDocument::open(bytes).expect("the packed document opens");
        assert!(doc.catalog().is_some(), "the catalog is reachable");
        assert_eq!(
            doc.get(ObjRef::new(3, 0)).ok().as_deref(),
            Some(&Object::Int(1234)),
            "and so is an object that only exists inside the container"
        );
    }

    #[test]
    fn streams_are_never_packed() {
        let table = NameTable::new();
        let mut objects = ObjectSet::new();
        objects.insert(1, Object::Int(7));
        objects.insert_stream(
            2,
            StreamData {
                dict: Dict::new(),
                data: b"content".to_vec(),
            },
        );

        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));
        let options = WriteOptions {
            object_streams: true,
            ..WriteOptions::default()
        };
        let bytes = rewrite(&objects, &trailer, &options, &table);

        // A stream's data lies outside its object, so it cannot live in a
        // container; it must still be written at its own offset.
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("2 0 obj"), "the stream kept its own header");
        assert!(text.contains("content"), "and its data");

        let doc = CosDocument::open(bytes).expect("it opens");
        assert_eq!(
            doc.stream_decoded(ObjRef::new(2, 0)).ok(),
            Some(b"content".to_vec())
        );
    }

    #[test]
    fn packing_is_off_by_default() {
        let table = NameTable::new();
        let mut objects = ObjectSet::new();
        objects.insert(1, Object::Int(1));
        let trailer = Dict::new();

        let bytes = rewrite(&objects, &trailer, &WriteOptions::default(), &table);
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("/ObjStm"));
        assert!(text.contains("\ntrailer\n"), "a classic table and trailer");
    }
}
