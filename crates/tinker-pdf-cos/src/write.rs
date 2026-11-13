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

use tinker_pdf_pool::Parallelism;

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
    ///
    /// **Empty means none, and then the user password is the owner's too**
    /// (Algorithm 3 step (a)'s rule for R2 to R4, taken for R6). It is not
    /// written as the empty string: a reader tries the owner password first
    /// and the empty password before any other, so an empty owner password
    /// would open the file with every permission for anybody, and the user
    /// password would protect nothing. The cost of the rule is that whoever
    /// has the user password has the owner's authority, so `permissions`
    /// restrict nobody unless an owner password is given.
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
///
/// Nine fields: eight about **bytes on disk** — layout, declared version,
/// containers, compression, encryption, what is dropped on the way and what
/// is written once instead of twice — and one about the threads the writer
/// may spend making them, which changes no byte. That is deliberate and there
/// is one absence worth naming, because it is the field the next person will
/// reach for.
///
/// **There is no font-subsetting switch here, and there must not be.**
/// Subsetting a rewrite's embedded font programs means walking every content
/// stream the document draws — pages, form XObjects at any depth, every state
/// of every `/AP`, Type 3 glyph procedures — through the interpreter, so that
/// the glyphs kept are the glyphs the engine actually shows. That walk lives
/// in the facade (`tinker_pdf::subset`), which depends on this crate and not
/// the other way round; `cargo xtask dag` holds that direction against a
/// declared graph. A `subset_fonts: bool` on this struct would therefore be a
/// flag the crate carrying it cannot act on, and the caller most likely to set
/// it is a caller redacting a document — who would get a file that still
/// carries the removed letters' outlines and nothing saying so.
///
/// The switch is on `tinker_pdf::write::SaveOptions` instead, where the type
/// that carries it is the one that can act on it. This door writes every
/// embedded program through exactly as it arrived, always has, and does not
/// offer to do anything else.
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
    /// Merge identical streams into one object, on a rewrite.
    ///
    /// Identical means identical: equal dictionaries (`/Length` aside) and
    /// equal content, compared byte for byte — XXH64 only chooses which
    /// streams to compare. Byte-identical encodings merge without a decode;
    /// differing encodings under one dictionary are compared decoded, each
    /// once a save, so two encodings of one content merge too (ruling 23). A
    /// wrong merge silently draws one font with another's program, so nothing
    /// short of equality merges. Only streams are merged, never the
    /// dictionaries whose object number carries identity (pages, annotations,
    /// groups, structure elements, signatures), nor a stream an `/OBJR` names
    /// or a cross-reference or object stream. Runs after
    /// [`WriteOptions::garbage_collect`].
    ///
    /// Off by default: it decodes each stream whose dictionary another shares
    /// under a different encoding to compare it, and a file whose streams all
    /// differ pays that for nothing. Ignored on an incremental update, which
    /// appends and must not rewrite what a signature's revision covers.
    /// Unlike the font-subsetting switch this type's documentation refuses,
    /// this one is acted on here: the pass needs the object set and the
    /// filters, and this crate has both.
    pub deduplicate_streams: bool,
    /// The threads the writer may spend compressing. [`Parallelism::Serial`]
    /// by default.
    ///
    /// With `Threads(n)` and [`WriteOptions::compress`] on, the streams an
    /// output carries are compressed on the engine's own pool a window at a
    /// time — a window's streams side by side, each into a pre-sized slot of
    /// its own, then its objects serialised in their order — for a rewrite,
    /// an incremental update, the object-stream container and the linearized
    /// layout, which compresses every stream before it lays the file out.
    /// Each stream deflates alone and encryption stays after compression and
    /// in object order, so every byte, every IV and every ciphertext is the
    /// serial writer's at any thread count; the extra memory is one window's
    /// compressed streams. `Serial`, and a write that compresses nothing,
    /// write each object in turn.
    pub parallelism: Parallelism,
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
            deduplicate_streams: false,
            parallelism: Parallelism::Serial,
        }
    }
}

/// Escapes and writes a literal string (7.3.4.2).
pub(crate) fn write_string(out: &mut Vec<u8>, s: &PdfString) {
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
pub(crate) fn write_name(out: &mut Vec<u8>, bytes: &[u8]) {
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
    // 7.3.3: digits with no decimal point are an integer, and one past what an
    // integer holds is a token a reader clamps. Such a value keeps its point,
    // which still makes it a real ("4." is one).
    if value.abs() >= INTEGER_TOKEN_LIMIT && !trimmed.contains('.') {
        out.push(b'.');
    }
}

/// 2^63: the first magnitude whose digits an `i64` cannot hold, which is
/// where an integer token stops reading back as the value written.
const INTEGER_TOKEN_LIMIT: f64 = 9_223_372_036_854_775_808.0;

/// The number a reader takes from what [`write_real`] writes for `value`:
/// `value` to six decimal places, or 0 where it is not finite.
///
/// For a caller that must hold a value to a rule **as the file will state
/// it** — a white point's `X` of a ten-millionth is positive, and is written
/// as `0`, which Table 63 forbids.
pub(crate) fn written_real(value: f64) -> f64 {
    let mut text = Vec::new();
    write_real(&mut text, value);
    std::str::from_utf8(&text)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0.0)
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

    fn seal_stream(&self, data: &[u8], num: u32) -> Vec<u8> {
        tinker_pdf_crypto::handler::encrypt_aes256(&self.key, &self.iv_for(num, 0), data)
    }
}

/// What a writer needs of an encryption scheme, whichever one it is.
///
/// Two implement it and they exist for opposite reasons. [`StreamCipher`] is a
/// *fresh* R6 key a full save invents, and it is the only scheme this engine
/// writes when it gets to choose. [`InheritedCipher`] reproduces the
/// encryption of a file that already exists, because an incremental update
/// appends into one whose `/Encrypt` still stands and does not get to choose
/// at all — it must speak RC4 to an RC4 file (7.6.2).
pub trait ObjectCipher {
    /// Encrypts a stream belonging to object `num`.
    fn encrypt_stream(&self, data: &[u8], num: u32) -> Vec<u8>;
    /// A copy of `object` with every string inside it encrypted.
    fn encrypt_strings(&self, object: &Object, num: u32) -> Object;
}

impl ObjectCipher for StreamCipher {
    fn encrypt_stream(&self, data: &[u8], num: u32) -> Vec<u8> {
        self.seal_stream(data, num)
    }

    fn encrypt_strings(&self, object: &Object, num: u32) -> Object {
        let mut counter = 1u32;
        walk_strings(object, &mut counter, &mut |bytes, nonce| {
            tinker_pdf_crypto::handler::encrypt_aes256(&self.key, &self.iv_for(num, nonce), bytes)
        })
    }
}

/// The encryption of the file an incremental update is appending to.
pub struct InheritedCipher<'a> {
    /// The key the document authenticated with, and the methods it names.
    pub key: &'a tinker_pdf_crypto::FileKey,
}

impl ObjectCipher for InheritedCipher<'_> {
    fn encrypt_stream(&self, data: &[u8], num: u32) -> Vec<u8> {
        // Generation zero: `write_entry` writes every object as `num 0 obj`,
        // and 7.6.2's per-object key is salted with the generation it is
        // written under.
        self.key.encrypt_stream(num, 0, data)
    }

    fn encrypt_strings(&self, object: &Object, num: u32) -> Object {
        let mut counter = 1u32;
        walk_strings(object, &mut counter, &mut |bytes, nonce| {
            self.key.encrypt_string(num, 0, nonce, bytes)
        })
    }
}

/// Rebuilds `object` with every string in it passed through `seal`.
///
/// Shared by both ciphers so the *traversal* cannot differ between them — only
/// the arithmetic does. `counter` distinguishes several strings inside one
/// object, which would otherwise share an initialisation vector.
fn walk_strings(
    object: &Object,
    counter: &mut u32,
    seal: &mut dyn FnMut(&[u8], u32) -> Vec<u8>,
) -> Object {
    match object {
        Object::String(s) => {
            let nonce = *counter;
            *counter = counter.saturating_add(1);
            // Hex, because ciphertext is arbitrary bytes and a literal string
            // would need every one of them escaped.
            Object::String(PdfString::hex(seal(&s.bytes, nonce)))
        }
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|item| walk_strings(item, counter, seal))
                .collect(),
        ),
        Object::Dict(dict) => {
            let mut out = Dict::with_capacity(dict.len());
            for (key, value) in dict.iter() {
                out.insert(*key, walk_strings(value, counter, seal));
            }
            Object::Dict(out)
        }
        other => other.clone(),
    }
}

/// Which security handler a rewrite seals with: the standard one's password
/// ([`WriteOptions::encryption`]) or the public-key one's recipients
/// ([`crate::DocumentEditor::save_sealed`]). Crate-private, so that neither
/// public type had to change to carry the other.
#[derive(Clone, Copy)]
pub(crate) enum Sealing<'a> {
    /// 7.6.4's standard handler at R6.
    Password(&'a Encryption),
    /// 7.6.5's `/Adobe.PubSec`, its file key already sealed.
    PublicKey(&'a crate::pubsec::PublicKeyEncryption),
}

impl<'a> Sealing<'a> {
    /// The scheme `options` asks for on its own.
    pub(crate) fn from_options(options: &'a WriteOptions) -> Option<Sealing<'a>> {
        options.encryption.as_ref().map(Sealing::Password)
    }

    /// The caller's entropy the file identifier is mixed with, which is what
    /// keeps an encrypted file's `/ID` from confirming a guess at its
    /// plaintext (see [`with_identifier`]).
    fn identifier_entropy(self) -> &'a [u8; 48] {
        match self {
            Sealing::Password(encryption) => &encryption.entropy,
            Sealing::PublicKey(sealed) => sealed.identifier_entropy(),
        }
    }
}

/// The `/Encrypt` dictionary and the cipher for either handler.
pub(crate) fn build_sealing(
    sealing: Sealing<'_>,
    names: &NameTable,
) -> Option<(Dict, StreamCipher)> {
    match sealing {
        Sealing::Password(encryption) => build_encryption(encryption, names),
        Sealing::PublicKey(sealed) => Some(build_public_key(sealed, names)),
    }
}

/// 7.6.5's `/Encrypt` for a public-key handler at AES-256: `/Filter
/// /Adobe.PubSec`, `/SubFilter /adbe.pkcs7.s5` (crypt filters carry the
/// recipients), `/V 5`, and one crypt filter — `/AESV3`, its `/Recipients`
/// the sealed envelopes in the order the key was derived over.
///
/// From the file key on, everything is the standard handler's R6: AESV3
/// takes the file key as it is, so the cipher is the same [`StreamCipher`]
/// a password-encrypted rewrite uses, and only this dictionary differs.
fn build_public_key(
    sealed: &crate::pubsec::PublicKeyEncryption,
    names: &NameTable,
) -> (Dict, StreamCipher) {
    let filter_name = names.intern(b"DefaultCryptFilter");
    let mut filter = Dict::new();
    filter.insert(Name::TYPE, Object::Name(names.intern(b"CryptFilter")));
    filter.insert(names.intern(b"CFM"), Object::Name(names.intern(b"AESV3")));
    filter.insert(
        names.intern(b"AuthEvent"),
        Object::Name(names.intern(b"DocOpen")),
    );
    // ISO 32000-1 Table 25: the standard handler gives a crypt filter's
    // `/Length` in bytes, "public-key security handlers express it as is" —
    // in bits. So a public-key AESV3 filter says 256 where `build_encryption`
    // says 32. This tree's reader takes the key length from `/V` and ignores
    // the filter's, so nothing here would notice either; another reader might.
    filter.insert(names.intern(b"Length"), Object::Int(256));
    filter.insert(
        names.intern(b"Recipients"),
        Object::Array(
            sealed
                .recipients()
                .iter()
                .map(|envelope| Object::String(PdfString::hex(envelope.clone())))
                .collect(),
        ),
    );
    // No `/EncryptMetadata`: Table 27 puts the public-key handler's in the
    // crypt filter and this tree's reader looks for it on `/Encrypt` itself,
    // so the one value every reader agrees on is the default both places
    // share — true, which is what this writer does.
    let mut cf = Dict::new();
    cf.insert(filter_name, Object::Dict(filter));

    let mut dict = Dict::new();
    dict.insert(Name::FILTER, Object::Name(names.intern(b"Adobe.PubSec")));
    dict.insert(
        names.intern(b"SubFilter"),
        Object::Name(names.intern(b"adbe.pkcs7.s5")),
    );
    dict.insert(names.intern(b"V"), Object::Int(5));
    dict.insert(names.intern(b"Length"), Object::Int(256));
    dict.insert(
        names.intern(b"P"),
        Object::Int(i64::from(sealed.permissions())),
    );
    dict.insert(names.intern(b"CF"), Object::Dict(cf));
    dict.insert(names.intern(b"StmF"), Object::Name(filter_name));
    dict.insert(names.intern(b"StrF"), Object::Name(filter_name));
    (dict, StreamCipher::new(sealed.file_key()))
}

/// Builds the `/Encrypt` dictionary and the cipher that goes with it.
pub(crate) fn build_encryption(
    encryption: &Encryption,
    names: &NameTable,
) -> Option<(Dict, StreamCipher)> {
    // Algorithm 2.A tries the owner password first and every reader tries
    // the empty one first, so a `/O` derived from "" would hand the owner's
    // authority, and the file key, to anybody whatever the user password
    // was. Algorithm 3 step (a)'s rule for R2 to R4 — "if there is no owner
    // password, use the user password instead" — is taken for R6 as well.
    let owner = match encryption.owner_password.is_empty() {
        true => &encryption.user_password,
        false => &encryption.owner_password,
    };
    let built = tinker_pdf_crypto::handler::build_r6(
        encryption.user_password.as_bytes(),
        owner.as_bytes(),
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
///
/// # The first of those is a contract, not an optimisation
///
/// [`crate::ImageData::Compressed`] depends on it. That variant's whole point
/// is that a PNG's IDAT reaches a page **unmodified**, as `/FlateDecode` with
/// `/Predictor 15`; a writer that re-deflated it would produce a stream whose
/// declared filter describes the inner encoding and whose bytes are the outer
/// one, which no reader can undo. The dictionary is the only signal available
/// here — the bytes cannot be asked whether they are encoded — so the rule is
/// exactly "a `/Filter` key means hands off", and
/// `a_stream_that_already_declares_a_filter_is_handed_through_untouched`
/// asserts it rather than leaving it to be re-derived from this comment.
pub(crate) fn maybe_compress(
    data: &[u8],
    dict: &mut Dict,
    names: &NameTable,
    compress: bool,
) -> Vec<u8> {
    if !compress || data.is_empty() || dict.contains_key(Name::FILTER) {
        return data.to_vec();
    }
    // 14.3.2: a metadata stream is written for tools that read XMP without
    // reading PDF — a packet scanner looks for `<?xpacket` in the raw bytes —
    // and ISO 19005 forbids a `/Filter` on one outright. So compression never
    // reaches it, whatever the caller asked for.
    if dict.get_name(Name::TYPE) == Some(names.intern(b"Metadata")) {
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

/// One numbered object as it is serialised: an object as it stands, or a
/// stream's dictionary and bytes once [`maybe_compress`] has made of them what
/// the write asks.
pub(crate) enum Ready<'a> {
    /// An object, written as it stands.
    Object(&'a Object),
    /// A stream's dictionary and its bytes, compressed as asked, with no
    /// `/Length` yet.
    Stream(Dict, Vec<u8>),
}

/// `entry` made ready to serialise: a stream compressed as [`maybe_compress`]
/// compresses it.
pub(crate) fn ready<'a>(entry: &'a Written, names: &NameTable, compress: bool) -> Ready<'a> {
    match entry {
        Written::Object(object) => Ready::Object(object),
        Written::Stream(stream) => {
            let mut dict = stream.dict.clone();
            let data = maybe_compress(&stream.data, &mut dict, names, compress);
            Ready::Stream(dict, data)
        }
    }
}

#[cfg(test)]
pub(crate) static COMPRESSED: std::sync::Mutex<Vec<(std::thread::ThreadId, bool)>> =
    std::sync::Mutex::new(Vec::new());

/// Records, under `cfg(test)` only, that a stream of a pooled write called on
/// `caller`'s thread was made ready on a worker or not: what
/// `compression_on_four_threads_runs_on_no_caller_thread` reads, its own
/// thread's entries apart from every other test's.
#[cfg(test)]
fn compressed_on(caller: std::thread::ThreadId) {
    if let Ok(mut compressed) = COMPRESSED.lock() {
        compressed.push((caller, tinker_pdf_pool::on_worker()));
    }
}

/// Nothing is recorded outside a test.
#[cfg(not(test))]
fn compressed_on(_caller: std::thread::ThreadId) {}

/// Every stream of `entries` made ready ([`ready`]) on the threads
/// `parallelism` names, each into a pre-sized slot of its own, in entry
/// order; `None` for an entry that is no stream.
pub(crate) fn ready_all<'a>(
    entries: &[&'a Written],
    names: &NameTable,
    compress: bool,
    parallelism: Parallelism,
) -> Vec<Option<Ready<'a>>> {
    let streams: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| matches!(entry, Written::Stream(_)))
        .map(|(at, _)| at)
        .collect();
    let caller = std::thread::current().id();
    let made = tinker_pdf_pool::map(&parallelism, streams.len(), |slot| {
        compressed_on(caller);
        streams
            .get(slot)
            .and_then(|at| entries.get(*at))
            .map(|entry| ready(entry, names, compress))
    });
    let mut out: Vec<Option<Ready<'a>>> = entries.iter().map(|_| None).collect();
    for (at, made) in streams.into_iter().zip(made) {
        if let Some(slot) = out.get_mut(at) {
            *slot = made;
        }
    }
    out
}

/// Whether a write compresses on the pool: it compresses at all, and was
/// asked for threads.
pub(crate) fn pooled(compress: bool, parallelism: Parallelism) -> bool {
    compress && matches!(parallelism, Parallelism::Threads(threads) if threads > 0)
}

/// How many streams a window of a pooled write holds for each worker it may
/// start: enough that one slow stream does not leave the rest idle for long,
/// few enough that a window's compressed bytes are a small part of the file.
const WINDOW_PER_WORKER: usize = 8;

/// Writes `entries` into `out` in their order, recording where each begins in
/// `offsets`.
///
/// Serially, and for a write that compresses nothing, each object is made
/// ready and written in turn, as [`write_entry`] writes it. On the pool
/// ([`pooled`]) the streams are made ready a window at a time — the window's
/// streams side by side, each into a slot of its own ([`ready_all`]), then its
/// objects serialised in their order — so the output is the serial writer's
/// and the extra memory one window's compressed streams. Encryption is applied
/// as each object is serialised, in object order, whichever way.
fn write_entries(
    out: &mut Vec<u8>,
    offsets: &mut Vec<(u32, u64)>,
    entries: &[(u32, &Written)],
    names: &NameTable,
    compress: bool,
    crypt: Option<&dyn ObjectCipher>,
    parallelism: Parallelism,
) {
    if !pooled(compress, parallelism) {
        for (num, entry) in entries {
            offsets.push((*num, out.len() as u64));
            write_entry(out, *num, entry, names, compress, crypt);
        }
        return;
    }
    let window = parallelism
        .workers(entries.len())
        .max(1)
        .saturating_mul(WINDOW_PER_WORKER);
    let mut rest = entries;
    while !rest.is_empty() {
        // The window: entries up to and including its `window`th stream.
        let mut streams = 0;
        let mut len = 0;
        for (_, entry) in rest {
            len += 1;
            if matches!(entry, Written::Stream(_)) {
                streams += 1;
                if streams == window {
                    break;
                }
            }
        }
        let (now, later) = rest.split_at(len.min(rest.len()));
        let inner: Vec<&Written> = now.iter().map(|(_, entry)| *entry).collect();
        let made = ready_all(&inner, names, compress, parallelism);
        for ((num, entry), made) in now.iter().zip(made) {
            offsets.push((*num, out.len() as u64));
            let made = made.unwrap_or_else(|| ready(entry, names, compress));
            write_ready(out, *num, made, names, crypt);
        }
        rest = later;
    }
}

/// Writes one numbered object, whichever kind it is.
fn write_entry(
    out: &mut Vec<u8>,
    num: u32,
    entry: &Written,
    names: &NameTable,
    compress: bool,
    crypt: Option<&dyn ObjectCipher>,
) {
    write_ready(out, num, ready(entry, names, compress), names, crypt);
}

/// Writes one numbered object made ready ([`ready`]), encrypting it with
/// `crypt` as it goes.
fn write_ready(
    out: &mut Vec<u8>,
    num: u32,
    entry: Ready<'_>,
    names: &NameTable,
    crypt: Option<&dyn ObjectCipher>,
) {
    out.extend_from_slice(
        format!(
            "{num} 0 obj
"
        )
        .as_bytes(),
    );
    match entry {
        Ready::Object(object) => match crypt {
            // 7.6.2: every string in the file is encrypted too, not only the
            // streams. Leaving them in the clear puts titles, form values and
            // annotation contents in plain sight inside a file that claims to
            // be encrypted.
            Some(cipher) => write_object(out, &cipher.encrypt_strings(object, num), names),
            None => write_object(out, object, names),
        },
        Ready::Stream(mut dict, data) => {
            // 7.3.8.2: /Length must agree with the bytes that follow, so the
            // writer computes it rather than trusting whatever was there.
            // Encryption is the last thing applied and the first thing undone,
            // so it wraps the compressed bytes rather than the other way round.
            let data = match crypt {
                Some(cipher) => cipher.encrypt_stream(&data, num),
                None => data,
            };
            dict.insert(Name::LENGTH, Object::Int(data.len() as i64));
            match crypt {
                // 7.6.2 again: a stream's dictionary holds strings as any
                // object does — an embedded file's `/Params` `/CheckSum` and
                // `/ModDate`, a form's `/PieceInfo` — and the reader decrypts
                // them there. Writing it as given put those in the clear in a
                // file sealed everywhere else, and a reader then decrypted the
                // clear bytes into garbage. One walk over the whole dictionary,
                // so its strings take the object's string nonces, which start
                // above the stream's own.
                Some(cipher) => write_object(
                    out,
                    &cipher.encrypt_strings(&Object::Dict(dict), num),
                    names,
                ),
                None => write_dict(out, &dict, names, 0),
            }
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

/// What an update written after a document needs to know of it: its length,
/// and whether its last byte is a newline — never its bytes.
///
/// An update's offsets are the document's length plus their place in the
/// update, and 7.5.6's update begins on a new line, so these two are all
/// [`update_section`] reads of what it is written after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// How many bytes the document has.
    pub len: u64,
    /// Whether its last byte is `\n`. False for a document with no bytes.
    pub ends_in_newline: bool,
}

impl Prefix {
    /// The prefix `bytes` make.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Prefix {
        Prefix {
            len: bytes.len() as u64,
            ends_in_newline: bytes.last() == Some(&b'\n'),
        }
    }
}

/// Appends an incremental update to `original`.
///
/// The result **starts with `original`, byte for byte**. That invariant is
/// what lets a signed document be updated without breaking its signature, and
/// it is asserted by tests rather than assumed.
///
/// `trailer` supplies `/Root`, `/Info` and `/ID`; `/Prev` and `/Size` are
/// written for it.
///
/// The original followed by the section [`update_section`] writes after it,
/// joined once: the one copy of the original a whole-file save makes.
#[must_use]
pub fn incremental_update(
    original: &[u8],
    changed: &ObjectSet,
    trailer: &Dict,
    previous_startxref: u64,
    names: &NameTable,
    compress: bool,
    crypt: Option<&dyn ObjectCipher>,
) -> Vec<u8> {
    incremental_update_reserving(
        original,
        changed,
        trailer,
        previous_startxref,
        &UpdatePlan {
            names,
            compress,
            crypt,
            reserved: None,
            parallelism: Parallelism::Serial,
        },
    )
    .0
}

/// The incremental update [`incremental_update`] appends to a document,
/// alone: the bytes that follow `prefix`'s document in the file it writes.
///
/// A newline first when the document does not end in one (7.5.6), then each
/// changed object, the cross-reference section of their rows and object
/// zero's free head, and the trailer with its `/Size`, `/Prev` and `/ID` —
/// every offset in it the document's length plus its place here, so the
/// document's bytes followed by these are [`incremental_update`]'s file, byte
/// for byte. Nothing of the document's own bytes is read or copied.
#[must_use]
pub fn update_section(
    prefix: Prefix,
    changed: &ObjectSet,
    trailer: &Dict,
    previous_startxref: u64,
    names: &NameTable,
    compress: bool,
    crypt: Option<&dyn ObjectCipher>,
) -> Vec<u8> {
    update_section_reserving(
        prefix,
        changed,
        trailer,
        previous_startxref,
        &UpdatePlan {
            names,
            compress,
            crypt,
            reserved: None,
            parallelism: Parallelism::Serial,
        },
    )
    .0
}

/// `original` followed by `section`, in one buffer of their sum's length.
fn joined(original: &[u8], section: &[u8]) -> Vec<u8> {
    count::copied(original.len());
    let mut out = Vec::with_capacity(original.len().saturating_add(section.len()));
    out.extend_from_slice(original);
    out.extend_from_slice(section);
    out
}

/// What the writer copies out of a document it writes after, counted for
/// the update section's guard: compiled only under `cfg(test)` and kept per
/// thread.
#[cfg(test)]
pub(crate) mod count {
    use std::cell::Cell;

    std::thread_local! {
        static COPIED: Cell<u64> = const { Cell::new(0) };
    }

    /// `bytes` of an original copied into the output.
    pub(crate) fn copied(bytes: usize) {
        COPIED.with(|count| count.set(count.get().saturating_add(bytes as u64)));
    }

    /// The bytes copied on this thread since the last call, which starts the
    /// count again from zero.
    pub(crate) fn take() -> u64 {
        COPIED.with(|count| count.replace(0))
    }
}

/// The counter outside tests: nothing.
#[cfg(not(test))]
mod count {
    pub(crate) fn copied(_bytes: usize) {}
}

/// How an incremental update is to be written, beyond what it is written over.
///
/// A struct rather than four more parameters: the function had reached the
/// seven the lint allows, and the four that vary together are exactly these.
pub(crate) struct UpdatePlan<'a> {
    pub(crate) names: &'a NameTable,
    pub(crate) compress: bool,
    pub(crate) crypt: Option<&'a dyn ObjectCipher>,
    /// One object serialised by the caller, appended verbatim.
    pub(crate) reserved: Option<(u32, &'a crate::sign::Reserved)>,
    /// The threads the update's compression may spend
    /// ([`WriteOptions::parallelism`]).
    pub(crate) parallelism: Parallelism,
}

/// [`incremental_update`], optionally appending one object whose bytes the
/// caller has already serialised.
///
/// The object is the signature dictionary (12.8.1), and it is written this way
/// for two reasons that are both about *where* rather than *what*. It has to
/// be patched after the file is finished, so its two reserved fields need
/// absolute offsets, and no writer that formats a dictionary can report where
/// inside its output a particular value landed. And 7.6.2 exempts a signature
/// dictionary's `/Contents` from encryption, so it must not go through
/// `crypt` — writing it by hand makes that a decision rather than an omission.
///
/// Returns the file and, when an object was appended, where its two reserved
/// fields ended up.
pub(crate) fn incremental_update_reserving(
    original: &[u8],
    changed: &ObjectSet,
    trailer: &Dict,
    previous_startxref: u64,
    plan: &UpdatePlan<'_>,
) -> (Vec<u8>, Option<SignaturePlaceholder>) {
    let (section, placeholder) = update_section_reserving(
        Prefix::of(original),
        changed,
        trailer,
        previous_startxref,
        plan,
    );
    (joined(original, &section), placeholder)
}

/// [`update_section`], optionally appending one object whose bytes the caller
/// has already serialised, as [`incremental_update_reserving`] does — the
/// placeholder's fields at their offsets in the **whole file**, the document's
/// length plus their place in the section, since that is the file a signature
/// is patched and digested in.
pub(crate) fn update_section_reserving(
    prefix: Prefix,
    changed: &ObjectSet,
    trailer: &Dict,
    previous_startxref: u64,
    plan: &UpdatePlan<'_>,
) -> (Vec<u8>, Option<SignaturePlaceholder>) {
    let (names, compress, crypt, reserved) = (plan.names, plan.compress, plan.crypt, plan.reserved);
    let mut out = Vec::new();

    // 7.5.6: an update begins on a new line so the appended section cannot be
    // mistaken for a continuation of whatever the original ended with.
    if !prefix.ends_in_newline {
        out.push(b'\n');
    }

    // Where a byte of the section stands in the whole file.
    let base = prefix.len;
    let at = |out: &[u8]| base.saturating_add(out.len() as u64);
    let in_file = |local: usize| {
        usize::try_from(base)
            .unwrap_or(usize::MAX)
            .saturating_add(local)
    };

    let mut offsets: Vec<(u32, u64)> = Vec::with_capacity(changed.entries.len());
    // 7.6.2: an update inherits the original file's encryption, so the
    // appended objects are sealed with *that* file's key and methods — not
    // with a scheme of this writer's choosing. Appending plaintext under a
    // trailer that still carries `/Encrypt` produces a file every conforming
    // reader decrypts into garbage.
    let entries: Vec<(u32, &Written)> = changed
        .entries
        .iter()
        .map(|(num, entry)| (*num, entry))
        .collect();
    write_entries(
        &mut out,
        &mut offsets,
        &entries,
        names,
        compress,
        crypt,
        plan.parallelism,
    );
    // `write_entries` places each object in the section; the table states
    // where it stands in the whole file.
    for (_, offset) in &mut offsets {
        *offset = base.saturating_add(*offset);
    }

    // The signature object, if there is one: written verbatim, uncompressed
    // and unencrypted, with its reserved fields' offsets carried out.
    let mut placeholder = None;
    if let Some((num, reserved)) = reserved {
        offsets.push((num, at(&out)));
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        let body_at = in_file(out.len());
        out.extend_from_slice(&reserved.bytes);
        out.extend_from_slice(b"\nendobj\n");
        placeholder = Some(SignaturePlaceholder {
            contents_at: body_at.saturating_add(reserved.contents_at),
            contents_len: reserved.contents_len,
            byte_range_at: body_at.saturating_add(reserved.byte_range_at),
            byte_range_len: reserved.byte_range_len,
        });
    }

    let xref_at = at(&out);
    write_classic_xref(&mut out, &offsets);

    // 14.4: an update keeps the document's permanent identifier and moves the
    // second string, so a reader can tell two revisions of one document from
    // two documents. Derived from the objects this revision actually writes,
    // which is what makes it move at all.
    let mut trailer = with_identifier(trailer, changed, names, None);
    // The *changed* set's highest number is not the file's. Writing it
    // unqualified clobbered the correct value the caller passes in, and a
    // conforming reader must then ignore every object above it — including
    // the edited page's own `/Contents`. It can only ever go up.
    let existing = trailer
        .get(Name::SIZE)
        .and_then(Object::as_int)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let size = changed
        .max_number()
        .saturating_add(1)
        .max(existing)
        // The signature object is not in `changed` — it was serialised by the
        // caller — so its number has to be counted here or a reader would be
        // required to ignore the one object the file exists to carry.
        .max(reserved.map_or(0, |(num, _)| num.saturating_add(1)));
    trailer.insert(Name::SIZE, Object::Int(i64::from(size)));
    // 7.5.5 Table 15: /Prev names the previous cross-reference *section*. A
    // document with none — one the repair scanner rebuilt — gets no /Prev at
    // all, rather than a `/Prev 0` that sends every reader to byte zero.
    if previous_startxref > 0 {
        trailer.insert(
            Name::PREV,
            Object::Int(i64::try_from(previous_startxref).unwrap_or(0)),
        );
    } else {
        trailer = crate::edit::without(&trailer, Name::PREV);
    }

    out.extend_from_slice(b"trailer\n");
    write_dict(&mut out, &trailer, names, 0);
    out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());

    (out, placeholder)
}

/// The trailer with the `/ID` 7.5.5 Table 15 asks for.
///
/// **Required whenever `/Encrypt` is present**, and strongly recommended
/// otherwise — and until this existed no file this engine wrote carried one on
/// any path. An outside reader is what found that; the strict validator is
/// what refuses it now.
///
/// # Where the bytes come from
///
/// A hash of the document, not of the moment. 14.4 suggests deriving the
/// identifier from the time, the path and the file's size; a clock is banned
/// from this engine's output by ruling 4, and a path is not a thing a byte
/// slice has. So the identifier is SHA-256 over the objects as they serialise
/// and over the trailer that names them, truncated to the sixteen bytes
/// everybody writes. Same document in, same identifier out, on every target —
/// which is the property ruling 4 needs and a timestamp cannot give.
///
/// Serialised uncompressed and unencrypted deliberately: the identifier is a
/// property of the *document*, so saving it twice with different options gives
/// the same `/ID[0]`, which is what 14.4 means by a permanent identifier.
///
/// # Why the entropy goes in when there is any
///
/// A plaintext-derived identifier on an encrypted file is a confirmation
/// oracle: anybody holding a candidate document can hash it and check the
/// `/ID` in the clear, without the password. Mixing in the caller's entropy —
/// which is already secret, already fixed for the call, and already what the
/// file key is built from — makes the identifier say "this file" rather than
/// "this content".
///
/// # `/ID[0]` survives, except where keeping it would leak
///
/// 14.4: the first string is the document's permanent identifier and the
/// second changes with each revision. A trailer that already carries a usable
/// first string keeps it, so a rewrite is recognisably the same document it
/// was read from, and only the second half moves.
///
/// **An encrypted write derives both halves.** Inheriting the first would
/// carry a plaintext-derived identifier into the sealed file, and in the
/// ordinary create-then-encrypt flow that plaintext never existed anywhere a
/// reader could see it — so the inherited string would be the one thing in the
/// file that answers "is this that document?" for free. The linkage to the
/// source's identifier is lost, which is a real cost and the smaller one; 14.4
/// suggests a derivation rather than requiring one, and Table 15 requires only
/// that the two strings be there.
fn with_identifier(
    trailer: &Dict,
    objects: &ObjectSet,
    names: &NameTable,
    entropy: Option<&[u8; 48]>,
) -> Dict {
    let mut hasher = tinker_pdf_crypto::sha2::Sha256::new();
    let mut scratch = Vec::new();
    for (num, entry) in objects.iter() {
        scratch.clear();
        write_entry(&mut scratch, *num, entry, names, false, None);
        hasher.update(&scratch);
    }
    // The trailer as well: two documents with the same objects and different
    // /Root are different documents.
    scratch.clear();
    write_dict(&mut scratch, trailer, names, 0);
    hasher.update(&scratch);
    if let Some(entropy) = entropy {
        hasher.update(entropy);
    }
    let digest = hasher.finish();
    let derived = PdfString::hex(digest.get(..16).unwrap_or_default().to_vec());

    let inherited = trailer
        .get_array(Name::ID)
        .and_then(<[Object]>::first)
        .and_then(Object::as_string)
        .filter(|first| !first.bytes.is_empty());
    let permanent = match (inherited, entropy) {
        (Some(first), None) => first.clone(),
        _ => derived.clone(),
    };

    let mut out = trailer.clone();
    out.insert(
        Name::ID,
        Object::Array(vec![Object::String(permanent), Object::String(derived)]),
    );
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
    rewrite_sealed(
        objects,
        trailer,
        options,
        names,
        Sealing::from_options(options),
    )
}

/// [`rewrite`], sealed with whichever handler `sealing` names.
pub(crate) fn rewrite_sealed(
    objects: &ObjectSet,
    trailer: &Dict,
    options: &WriteOptions,
    names: &NameTable,
    sealing: Option<Sealing<'_>>,
) -> Vec<u8> {
    // Before the layout is chosen, so the linearized writer receives the same
    // trailer the ordinary one would.
    let identified = with_identifier(
        trailer,
        objects,
        names,
        sealing.map(Sealing::identifier_entropy),
    );
    // 7.5.6: a rewrite is one revision, so it has no earlier section to chain
    // to. Both keys arrive from the *source* document's trailer — every file
    // that was ever incrementally updated carries a `/Prev`, and a hybrid one
    // carries an `/XRefStm` — and both name offsets in a file that no longer
    // exists. The cross-reference-stream path already dropped them; the
    // classic path did not, so a rewrite of any updated document sent every
    // reader to an offset in the middle of the new file's objects. Ours
    // repaired it and opened anyway, which is why nothing here noticed until
    // the strict validator read the sections as written.
    let identified = crate::edit::without(&identified, Name::PREV);
    let identified = crate::edit::without(&identified, Name::XREF_STM);
    let trailer = &identified;

    if options.linearize {
        // A document with no catalog or no pages has no first page to put
        // first; the ordinary layout is then the only honest one, rather than
        // a file claiming /Linearized and not being.
        //
        // Encryption used to be a second reason to fall through here, and the
        // caller was told nothing: they asked for both and got an encrypted
        // file with an ordinary layout. `linearize` owns the cipher now.
        if let Some(bytes) =
            crate::linearize::linearize_sealed(objects, trailer, options, names, sealing)
        {
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
    let encryption = sealing.and_then(|sealing| build_sealing(sealing, names));
    let crypt: Option<&dyn ObjectCipher> = encryption
        .as_ref()
        .map(|(_, cipher)| cipher as &dyn ObjectCipher);

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

    // F.3.3: the linearization parameter dictionary describes *this file's*
    // layout, and an ordinary rewrite is not that layout. It arrives here from
    // any source that was linearized, nothing references it, and carrying it
    // through makes the new file claim a fast-web-view layout whose hint
    // stream and first-page section are gone. Emptied rather than dropped, so
    // the numbering the rest of the file uses does not move.
    let linearized = names.intern(b"Linearized");
    let hollow = Written::Object(Object::Dict(Dict::new()));

    let mut entries: Vec<(u32, &Written)> = Vec::with_capacity(objects.entries.len() + 1);
    for (num, object) in &objects.entries {
        if packed.contains(num) {
            continue;
        }
        let stale = matches!(
            object,
            Written::Object(Object::Dict(dict)) if dict.contains_key(linearized)
        );
        entries.push((*num, if stale { &hollow } else { object }));
    }

    // The container is built before anything is written, so that its stream
    // is compressed beside the loose objects' streams; it is written after
    // them all, where it always stands.
    let container = objects.max_number().saturating_add(1);
    let container_stream = (!packed.is_empty()).then(|| {
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
        Written::Stream(StreamData { dict, data })
    });
    if let Some(stream) = &container_stream {
        entries.push((container, stream));
    }
    // The container is compressed whenever compression is on at all: packing
    // many small objects together and then leaving the result uncompressed
    // makes the file *larger* than not packing it, which is the opposite of
    // the reason object streams exist.
    write_entries(
        &mut out,
        &mut offsets,
        &entries,
        names,
        options.compress,
        crypt,
        options.parallelism,
    );

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

/// One row of a cross-reference stream, before it is packed into `/W` fields.
///
/// Named rather than written as a tuple because the three forms are 7.5.8.2's
/// three types and the middle field means something different in each: a byte
/// offset in type 1 and a container's object number in type 2.
enum Row {
    /// Type 0: free. Object zero always is; nothing else here ever is.
    Free,
    /// Type 1: at this byte offset in the file.
    AtOffset(u64),
    /// Type 2: the k-th object of the container.
    InStream(usize),
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
    // Every object that has an offset, not just the container. `/Encrypt` is
    // numbered above it, and sizing from the container alone left that object
    // with no entry at all: the file reopened as unencrypted with zero pages
    // while its content was genuinely ciphertext, which is unrecoverable.
    let highest_written = offsets
        .iter()
        .map(|(num, _)| *num)
        .max()
        .unwrap_or(container)
        .max(container);
    // The stream is an object as well, and it takes the next number after
    // everything already written — not `container + 1`, which is the number
    // `/Encrypt` takes on an encrypted rewrite. Those two collided, so such a
    // file carried **two objects numbered the same**, and the reason nothing
    // noticed is that neither of them is found by number: a reader reaches the
    // stream through `startxref` and `/Encrypt` through the trailer. The dense
    // table hid it further, by resolving the collision in favour of whichever
    // branch it tested first.
    let stream_number = highest_written.saturating_add(1);
    let size = stream_number.saturating_add(1);

    // One row per object that exists, in a map, and then `/Index` subsections
    // over the runs — the same device `write_classic_xref` has always used,
    // spelled the way 7.5.8.2 Table 17 spells it.
    //
    // This used to write `0..size` densely and look up each number by scanning
    // `offsets` and `packed`. Two defects in one loop. The scans made it
    // quadratic in the object count, which is invisible until a file is large.
    // The density made it unbounded in the highest object *number*, which is a
    // different thing entirely: `pdfjs/test/pdfs/bug1980958.pdf` holds four
    // objects and numbers the last of them `i32::MAX`, so the loop ran two
    // thousand million times and `Vec::with_capacity(size * 8)` asked for
    // seventeen gigabytes to hold the result.
    //
    // A reader that ignores `/Index` reads the default `[0 Size]` and gets the
    // wrong rows, so this is not a free choice — but `/Index` is not optional
    // to support either, and this engine's own reader has honoured it since
    // the cross-reference stream reader was written.
    let mut rows: BTreeMap<u32, Row> = BTreeMap::new();
    // Object zero heads the free list, and 7.5.8.2 keeps that requirement from
    // the classic table.
    rows.insert(0, Row::Free);
    for (num, offset) in offsets {
        rows.insert(*num, Row::AtOffset(*offset));
    }
    for (index, num) in packed.iter().enumerate() {
        // A packed object is only in a container if it has no offset of its
        // own; an object written twice would be a defect upstream of here, and
        // the offset is the entry a reader can act on.
        rows.entry(*num).or_insert(Row::InStream(index));
    }
    rows.insert(stream_number, Row::AtOffset(out.len() as u64));

    // Three bytes of offset covers 16 MB; four covers 4 GB, which is past
    // what any single PDF should be.
    let mut data = Vec::with_capacity(rows.len() * 8);
    let mut index_pairs: Vec<(u32, u32)> = Vec::new();
    for (num, row) in &rows {
        match index_pairs.last_mut() {
            Some((first, count)) if *first + *count == *num => *count += 1,
            _ => index_pairs.push((*num, 1)),
        }
        match row {
            Row::Free => {
                data.push(0);
                data.extend_from_slice(&[0, 0, 0, 0]);
                data.extend_from_slice(&[255, 255]);
            }
            Row::AtOffset(offset) => {
                data.push(1);
                data.extend_from_slice(&(*offset as u32).to_be_bytes());
                data.extend_from_slice(&0u16.to_be_bytes());
            }
            Row::InStream(index) => {
                data.push(2);
                data.extend_from_slice(&container.to_be_bytes());
                data.extend_from_slice(&(*index as u16).to_be_bytes());
            }
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
    // 7.5.8.2 Table 17: which object numbers the rows above describe. Its
    // default is `[0 Size]`, which is what this stream used to be — and what
    // made its cost a function of the highest object number rather than of the
    // object count.
    dict.insert(
        Name::INDEX,
        Object::Array(
            index_pairs
                .iter()
                .flat_map(|(first, count)| {
                    [
                        Object::Int(i64::from(*first)),
                        Object::Int(i64::from(*count)),
                    ]
                })
                .collect(),
        ),
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

    // 7.5.4: object zero is the head of the free list, and it is not optional.
    // The run-merging below writes it as part of a subsection that starts at
    // one; a document whose lowest object number is two or more — which is
    // every rewrite of a file that had a free slot low down — used to get no
    // free head at all. Seventy-two corpus files produced such a table, and
    // this engine's own reader never looked: the strict validator is what
    // found it.
    if offsets.first().is_some_and(|(num, _)| *num > 1) {
        out.extend_from_slice(
            b"0 1
0000000000 65535 f 
",
        );
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
        // Past 2^63 the digits would read back as an integer the reader
        // clamps, so the point stays; below it nothing changes.
        assert_eq!(rendered(&Object::Real(1e20)), "100000000000000000000.");
        assert_eq!(rendered(&Object::Real(-1e20)), "-100000000000000000000.");
        assert_eq!(rendered(&Object::Real(1e18)), "1000000000000000000");
        // Non-finite values cannot be written at all, so they become zero.
        assert_eq!(rendered(&Object::Real(f64::NAN)), "0");
        assert_eq!(rendered(&Object::Real(f64::INFINITY)), "0");
    }

    /// What is written for a real reads back as that real with no repair, at
    /// every magnitude: a hostile file's huge coordinate, rewritten, must not
    /// read back as an integer clamped to `i64::MAX`, and the largest real,
    /// which is what the reader clamps an overflowing one to, must not read
    /// back as an overflow clamped again.
    #[test]
    fn a_real_past_an_integers_reach_reads_back_as_itself() {
        for value in [1e20, -1e20, 1e308, 2f64.powi(63), 1e18, f64::MAX, -f64::MAX] {
            let text = rendered(&Object::Real(value));
            let mut sink = crate::warn::WarningSink::new();
            let parsed = crate::parse::parse_object_at(text.as_bytes(), 0, &names(), &mut sink);
            assert!(sink.take().is_empty(), "{text} needed a repair");
            let read = match parsed.object {
                Object::Real(read) => read,
                Object::Int(read) => read as f64,
                other => panic!("{text} read back as {other:?}"),
            };
            // Within an ulp, which is what the lexer promises past 10^22, and
            // not `i64::MAX`, which is what a clamped integer would be.
            assert!(((read - value) / value).abs() <= 1e-15, "{text}: {read}");
        }
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
        let base = crate::CosDocument::open(original.clone()).expect("the base opens");

        let mut changed = ObjectSet::new();
        changed.insert(2, Object::Int(42));
        let updated = incremental_update(
            &original,
            &changed,
            &trailer,
            base.last_startxref(),
            &table,
            false,
            None,
        );

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
        // And the object the update did *not* carry, which is what a broken
        // chain loses: reached through the table rather than by rescanning.
        assert_eq!(doc.ladder_level(), crate::LadderLevel::Trust);
        assert!(doc.catalog().is_some(), "the earlier revision is reachable");
    }

    /// A document the repair scanner rebuilt has no section to chain to, and
    /// `/Prev 0` would send a reader to byte zero rather than saying so.
    #[test]
    fn an_update_over_an_unchainable_document_writes_no_prev() {
        let table = names();
        let original =
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let mut changed = ObjectSet::new();
        changed.insert(2, Object::Int(42));
        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));

        let updated = incremental_update(&original[..], &changed, &trailer, 0, &table, false, None);
        let text = String::from_utf8_lossy(&updated);
        assert!(!text.contains("/Prev"), "no section to name: {text}");
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

    /// The contract [`ImageData::Compressed`] rests on, asserted rather than
    /// inferred from `maybe_compress`'s shape.
    ///
    /// The bytes below are highly compressible on purpose — a run of one value,
    /// which `zlib_compress` shortens by an order of magnitude — so "no smaller
    /// than the input" cannot be what spares them. Only the `/Filter` key can.
    /// Both directions are checked, because a test that only pinned the
    /// declining half would pass on a `maybe_compress` that had stopped
    /// compressing anything at all.
    #[test]
    fn a_stream_that_already_declares_a_filter_is_handed_through_untouched() {
        let table = names();
        let data = vec![0x5Au8; 4096];

        let mut bare = Dict::new();
        let packed = maybe_compress(&data, &mut bare, &table, true);
        assert!(
            packed.len() < data.len() / 4,
            "an unfiltered stream is compressed: {} bytes",
            packed.len()
        );
        assert!(
            bare.contains_key(Name::FILTER),
            "and the writer records that it did"
        );

        let mut declared = Dict::new();
        declared.insert(Name::FILTER, Object::Name(table.intern(b"FlateDecode")));
        declared.insert(Name::DECODE_PARMS, {
            let mut parms = Dict::new();
            parms.insert(table.intern(b"Predictor"), Object::Int(15));
            Object::Dict(parms)
        });
        let before = declared.clone();
        let through = maybe_compress(&data, &mut declared, &table, true);

        assert_eq!(through, data, "the bytes are the bytes that were handed in");
        assert_eq!(
            declared, before,
            "and the dictionary is untouched, /DecodeParms included"
        );

        // The same holds for a filter that is not this writer's own, since the
        // rule is the key's presence and never which codec it names.
        let mut dct = Dict::new();
        dct.insert(Name::FILTER, Object::Name(table.intern(b"DCTDecode")));
        assert_eq!(maybe_compress(&data, &mut dct, &table, true), data);
    }

    /// A catalog, and `streams` streams of text that deflates, each its own,
    /// with a dictionary between every pair of them.
    fn a_set_of_streams(streams: u32, table: &NameTable) -> (ObjectSet, Dict) {
        let mut objects = ObjectSet::new();
        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(table.intern(b"Catalog")));
        objects.insert(1, Object::Dict(catalog));
        for index in 0..streams {
            let text: String = (0..60)
                .map(|line| format!("({index} {line}) Tj T* "))
                .collect();
            objects.insert_stream(
                2 + 2 * index,
                StreamData {
                    dict: Dict::new(),
                    data: text.into_bytes(),
                },
            );
            objects.insert(3 + 2 * index, Object::Int(i64::from(index)));
        }
        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));
        (objects, trailer)
    }

    /// This thread's records of where a pooled write made its streams ready,
    /// taken out of the shared record.
    fn taken() -> Vec<bool> {
        let me = std::thread::current().id();
        let mut taken = Vec::new();
        if let Ok(mut compressed) = COMPRESSED.lock() {
            compressed.retain(|(caller, on_worker)| {
                if *caller == me {
                    taken.push(*on_worker);
                    false
                } else {
                    true
                }
            });
        }
        taken
    }

    /// **Compression on four threads runs on no caller thread.** A rewrite of
    /// forty streams compressing on four threads — two windows, of thirty-two
    /// streams and of eight — makes every stream ready on a worker and none
    /// on the calling thread; on `wasm32-wasip1`, where no thread starts, all
    /// forty on the caller. The same rewrite serially, and one on four
    /// threads that compresses nothing, put no stream on the pool at all.
    #[test]
    fn compression_on_four_threads_runs_on_no_caller_thread() {
        let table = names();
        let (objects, trailer) = a_set_of_streams(40, &table);
        let _ = taken();

        let four = WriteOptions {
            compress: true,
            parallelism: Parallelism::Threads(4),
            ..WriteOptions::default()
        };
        let pooled = rewrite(&objects, &trailer, &four, &table);
        let ran = taken();
        assert_eq!(ran.len(), 40, "every stream was made ready on the pool");
        if cfg!(target_family = "wasm") {
            assert!(ran.iter().all(|on_worker| !on_worker), "the caller ran all");
        } else {
            assert!(
                ran.iter().all(|on_worker| *on_worker),
                "a stream was compressed on the calling thread"
            );
        }

        let serial = rewrite(
            &objects,
            &trailer,
            &WriteOptions {
                compress: true,
                ..WriteOptions::default()
            },
            &table,
        );
        assert!(taken().is_empty(), "a serial write uses no pool");
        assert!(pooled == serial, "and writes the pooled write's bytes");

        let _ = rewrite(
            &objects,
            &trailer,
            &WriteOptions {
                parallelism: Parallelism::Threads(4),
                ..WriteOptions::default()
            },
            &table,
        );
        assert!(taken().is_empty(), "nothing to compress, nothing pooled");
    }

    /// **A pooled write is the serial write at every window's edge.** Sets of
    /// one, two, seven, eight, nine, thirty-one to thirty-three and sixty-five
    /// streams — windows exactly full, one short, one over, and a window of a
    /// single stream — rewritten and appended as an update on 1, 2, 3, 4 and 7
    /// threads, each byte for byte the serial write.
    #[test]
    fn a_pooled_write_is_the_serial_write_at_every_window_edge() {
        let table = names();
        for streams in [1u32, 2, 7, 8, 9, 31, 32, 33, 65] {
            let (objects, trailer) = a_set_of_streams(streams, &table);
            let options = |parallelism| WriteOptions {
                compress: true,
                parallelism,
                ..WriteOptions::default()
            };
            let serial = rewrite(&objects, &trailer, &options(Parallelism::Serial), &table);
            let original = rewrite(&objects, &trailer, &WriteOptions::default(), &table);
            let update = |parallelism| {
                incremental_update_reserving(
                    &original,
                    &objects,
                    &trailer,
                    0,
                    &UpdatePlan {
                        names: &table,
                        compress: true,
                        crypt: None,
                        reserved: None,
                        parallelism,
                    },
                )
                .0
            };
            let serial_update = update(Parallelism::Serial);
            for threads in [1usize, 2, 3, 4, 7] {
                let pooled = rewrite(
                    &objects,
                    &trailer,
                    &options(Parallelism::Threads(threads)),
                    &table,
                );
                assert!(
                    pooled == serial,
                    "{streams} streams on {threads} threads: the rewrite differs"
                );
                assert!(
                    update(Parallelism::Threads(threads)) == serial_update,
                    "{streams} streams on {threads} threads: the update differs"
                );
            }
        }
        let _ = taken();
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
