//! RSA signature generation held to published vectors
//! (`docs/design/signing.md`, *RSA signing*): NIST CAVP's `SigGen15` and
//! `SigGenPSS` files, their SHA-512/224 and SHA-512/256 companions among
//! them, and Wycheproof's RSASSA-PKCS1-v1_5 generation files, all under
//! `tests/data/` with their provenance in `THIRDPARTY.md`.
//!
//! What holds a signer is published data, never the engine agreeing with
//! itself (ruling 13): a signature the signer makes and the verifier accepts
//! proves only that the two agree. Every count below is asserted as well as
//! printed, because a reader that silently matched no line would otherwise
//! pass.

use std::collections::BTreeMap;

use tinker_pdf_crypto::bignum::{Modulus, Uint};
use tinker_pdf_crypto::rsa::RSA_LIMBS;
use tinker_pdf_crypto::sha2::sha256;
use tinker_pdf_crypto::{
    DigestAlgorithm, EntropySource, PssParameters, RsaPrivateComponents, RsaPrivateKey, RsaRefusal,
};

fn unhex(text: &str) -> Vec<u8> {
    let text = text.trim();
    assert!(text.len() % 2 == 0, "an odd number of hex digits: {text}");
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("ASCII");
            u8::from_str_radix(digits, 16).expect("a hex digit pair")
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// ---- NIST CAVP -------------------------------------------------------------

/// The four CAVP signature-generation files, and the SHA-256 of each one's
/// archive member as its header records it.
const CAVP: [(&str, &str, &str); 4] = [
    (
        "rsa_siggen15.rsp",
        include_str!("data/cavp/rsa_siggen15.rsp"),
        "19c49b8fc7102cc12476e215854cd228f1f8c0d316f0ea2df6c183f167e9724a",
    ),
    (
        "rsa_siggen15_truncated.rsp",
        include_str!("data/cavp/rsa_siggen15_truncated.rsp"),
        "d02f487118dc9d2f761bbbe87d52406c90a4701638eac0e11219892ff923c709",
    ),
    (
        "rsa_siggen_pss.rsp",
        include_str!("data/cavp/rsa_siggen_pss.rsp"),
        "46de7f2fccb5f89516e64b5bdb1c4fd456a09c9cc680dd64e0756e62ff5000ec",
    ),
    (
        "rsa_siggen_pss_truncated.rsp",
        include_str!("data/cavp/rsa_siggen_pss_truncated.rsp"),
        "a2d2caf6bb059f4dec9a1a2fdfc7219b6b65afbc51969d67373d108ccf511dae",
    ),
];

/// One key of a CAVP signature-generation file and what was signed with it.
struct CavpKey {
    /// The `[mod = …]` the key sits under.
    bits: usize,
    n: Vec<u8>,
    e: Vec<u8>,
    d: Vec<u8>,
    records: Vec<CavpRecord>,
}

/// One signature: the digest it was made over, by its FIPS 180-4 name, the
/// message, the signature, and — in a PSS file — the salt as printed.
struct CavpRecord {
    digest: &'static str,
    message: Vec<u8>,
    signature: Vec<u8>,
    salt: Option<String>,
}

/// CAVP's spelling of a digest, as FIPS 180-4 names it.
fn cavp_digest(name: &str) -> &'static str {
    match name {
        "SHA224" => "SHA-224",
        "SHA256" => "SHA-256",
        "SHA384" => "SHA-384",
        "SHA512" => "SHA-512",
        "SHA512224" => "SHA-512/224",
        "SHA512256" => "SHA-512/256",
        other => panic!("a CAVP digest this file was not expected to name: {other}"),
    }
}

/// Reads a CAVP signature-generation file: `[mod = …]`, then each key's `n`,
/// `e` and `d`, then `SHAAlg`, `Msg` and `S` per signature — and, after `S`
/// in a PSS file, the `SaltVal` it was made with.
fn cavp_keys(text: &str) -> Vec<CavpKey> {
    let mut keys: Vec<CavpKey> = Vec::new();
    let mut bits = 0usize;
    let mut digest = None;
    let mut message = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(size) = line
            .strip_prefix("[mod = ")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            bits = size.parse().expect("a modulus size");
            continue;
        }
        let Some((name, value)) = line.split_once(" = ") else {
            panic!("a line that is not a CAVP assignment: {line}");
        };
        match name {
            "n" => keys.push(CavpKey {
                bits,
                n: unhex(value),
                e: Vec::new(),
                d: Vec::new(),
                records: Vec::new(),
            }),
            "e" => keys.last_mut().expect("n precedes e").e = unhex(value),
            "d" => keys.last_mut().expect("n precedes d").d = unhex(value),
            "SHAAlg" => digest = Some(cavp_digest(value)),
            "Msg" => message = unhex(value),
            "S" => keys
                .last_mut()
                .expect("a key precedes every signature")
                .records
                .push(CavpRecord {
                    digest: digest.expect("SHAAlg precedes every signature"),
                    message: std::mem::take(&mut message),
                    signature: unhex(value),
                    salt: None,
                }),
            "SaltVal" => {
                let record = keys
                    .last_mut()
                    .and_then(|key| key.records.last_mut())
                    .expect("SaltVal follows its signature");
                record.salt = Some(value.to_string());
            }
            other => panic!("a CAVP field this reader does not know: {other}"),
        }
    }
    keys
}

/// The salt lengths a PSS file's CAVS header declares, per modulus size and
/// digest: `Mod Size 2048 with SHA-224(Salt len: 15); SHA-256(Salt len: 20);`.
fn declared_salt_lengths(text: &str) -> Vec<(usize, &'static str, usize)> {
    let line = text
        .lines()
        .find(|line| line.starts_with("# Combinations selected:"))
        .expect("a CAVS combinations line");
    let mut declared = Vec::new();
    for part in line.split("Mod Size ").skip(1) {
        let (size, rest) = part.split_once(" with ").expect("a size and its digests");
        let size: usize = size.trim().parse().expect("a modulus size");
        for entry in rest.split(';') {
            let Some((name, length)) = entry.trim().split_once("(Salt len: ") else {
                continue;
            };
            let length = length.trim_end_matches(')').parse().expect("a salt length");
            let name = match name.trim() {
                "SHA-224" => "SHA-224",
                "SHA-256" => "SHA-256",
                "SHA-384" => "SHA-384",
                "SHA-512" => "SHA-512",
                "SHA-512/224" => "SHA-512/224",
                "SHA-512/256" => "SHA-512/256",
                other => panic!("a declared digest this file was not expected to name: {other}"),
            };
            declared.push((size, name, length));
        }
    }
    declared
}

/// A record's salt, read the way its file's header declares it: CAVP prints
/// an empty salt as `SaltVal = 00`, as it prints an empty message, so a
/// declared length of zero is what tells that from a salt of one zero octet.
fn salt_of(record: &CavpRecord, bits: usize, declared: &[(usize, &'static str, usize)]) -> Vec<u8> {
    let printed = record
        .salt
        .as_deref()
        .expect("a PSS record carries its salt");
    let length = declared
        .iter()
        .find(|&&(size, name, _)| size == bits && name == record.digest)
        .map(|&(_, _, length)| length)
        .expect("the header declares a salt length for every digest it signs with");
    if length == 0 {
        assert_eq!(printed, "00", "an empty salt is printed as 00");
        Vec::new()
    } else {
        let salt = unhex(printed);
        assert_eq!(
            salt.len(),
            length,
            "the salt is the length the header declares"
        );
        salt
    }
}

/// Holds a CAVP file's body to the SHA-256 its header records for the archive
/// member: the body starts at the member's own first line, `# CAVS`, and the
/// member's line endings were CRLF.
fn assert_cavp_provenance(name: &str, text: &str, member_sha256: &str) {
    assert!(
        text.contains(member_sha256),
        "{name}: the header records the member's SHA-256"
    );
    let start = text.find("\n# CAVS").expect("the member's own header") + 1;
    let member = text.get(start..).expect("in bounds").replace('\n', "\r\n");
    assert_eq!(
        hex(&sha256(member.as_bytes())),
        member_sha256,
        "{name}: the body is the archive member, byte for byte"
    );
}

// ---- Wycheproof ------------------------------------------------------------

/// The five Wycheproof files and the SHA-256 each was fetched with
/// (`THIRDPARTY.md`).
const WYCHEPROOF: [(&str, &[u8], &str); 5] = [
    (
        "rsa_pkcs1_1024_sig_gen_test.json",
        include_bytes!("data/wycheproof/rsa_pkcs1_1024_sig_gen_test.json"),
        "d29bb1d43016e8932937554291e997dc509455373b0d3cfe89ebffaee80ec9cd",
    ),
    (
        "rsa_pkcs1_1536_sig_gen_test.json",
        include_bytes!("data/wycheproof/rsa_pkcs1_1536_sig_gen_test.json"),
        "251da3f00f11cba07f8e89c94363e25ea16ef4b35aaa270063e4f763c283a8bd",
    ),
    (
        "rsa_pkcs1_2048_sig_gen_test.json",
        include_bytes!("data/wycheproof/rsa_pkcs1_2048_sig_gen_test.json"),
        "587b3d27d3429f2118cb02818da4d08eb3f36f50c747939eb550fae4337ccbf0",
    ),
    (
        "rsa_pkcs1_3072_sig_gen_test.json",
        include_bytes!("data/wycheproof/rsa_pkcs1_3072_sig_gen_test.json"),
        "45153219d486fd135d35aa968a957a0c015c7829e8dc59bea5f87e6c30912677",
    ),
    (
        "rsa_pkcs1_4096_sig_gen_test.json",
        include_bytes!("data/wycheproof/rsa_pkcs1_4096_sig_gen_test.json"),
        "f13eadbec6c45291cb33e6b2fa0a0c2b862d0bd2eaec29f930d3b78067cd48c7",
    ),
];

const WYCHEPROOF_LICENSE: (&[u8], &str) = (
    include_bytes!("data/wycheproof/LICENSE"),
    "58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd",
);

/// A JSON value, as much of RFC 8259 as the vector files use: no number is
/// read as anything but its digits, which is all a test id or a size needs.
#[derive(Debug)]
enum Json {
    Null,
    True,
    False,
    Number(String),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn text(&self) -> &str {
        match self {
            Json::Text(text) => text,
            other => panic!("not a string: {other:?}"),
        }
    }

    fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("not an array: {other:?}"),
        }
    }

    fn number(&self) -> u64 {
        match self {
            Json::Number(digits) => digits.parse().expect("a whole number"),
            other => panic!("not a number: {other:?}"),
        }
    }

    fn field(&self, key: &str) -> &Json {
        self.get(key)
            .unwrap_or_else(|| panic!("no field `{key}` in {self:?}"))
    }
}

/// A recursive-descent reader over the bytes of one document.
struct JsonReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl JsonReader<'_> {
    fn document(bytes: &[u8]) -> Json {
        let mut reader = JsonReader { bytes, at: 0 };
        let value = reader.value();
        reader.space();
        assert_eq!(reader.at, bytes.len(), "nothing follows the document");
        value
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn take_byte(&mut self) -> u8 {
        let byte = self.peek().expect("the document does not end mid-value");
        self.at += 1;
        byte
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, word: &[u8]) {
        for &expected in word {
            assert_eq!(self.take_byte(), expected, "malformed JSON at {}", self.at);
        }
    }

    fn value(&mut self) -> Json {
        self.space();
        match self.peek().expect("a value") {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Json::Object(members);
                }
                loop {
                    self.space();
                    let Json::Text(name) = self.value() else {
                        panic!("an object key that is not a string at {}", self.at);
                    };
                    self.space();
                    self.expect(b":");
                    let value = self.value();
                    members.push((name, value));
                    self.space();
                    match self.take_byte() {
                        b',' => continue,
                        b'}' => return Json::Object(members),
                        other => panic!("unexpected {:?} in an object", other as char),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Json::Array(items);
                }
                loop {
                    items.push(self.value());
                    self.space();
                    match self.take_byte() {
                        b',' => continue,
                        b']' => return Json::Array(items),
                        other => panic!("unexpected {:?} in an array", other as char),
                    }
                }
            }
            b'"' => {
                self.at += 1;
                let mut text = String::new();
                loop {
                    match self.take_byte() {
                        b'"' => return Json::Text(text),
                        b'\\' => match self.take_byte() {
                            b'"' => text.push('"'),
                            b'\\' => text.push('\\'),
                            b'/' => text.push('/'),
                            b'b' => text.push('\u{8}'),
                            b'f' => text.push('\u{c}'),
                            b'n' => text.push('\n'),
                            b'r' => text.push('\r'),
                            b't' => text.push('\t'),
                            b'u' => {
                                let digits: Vec<u8> = (0..4).map(|_| self.take_byte()).collect();
                                let digits = std::str::from_utf8(&digits).expect("ASCII");
                                let code =
                                    u32::from_str_radix(digits, 16).expect("four hex digits");
                                text.push(char::from_u32(code).expect("a scalar value"));
                            }
                            other => panic!("unknown escape \\{}", other as char),
                        },
                        byte if byte < 0x80 => text.push(char::from(byte)),
                        lead => {
                            // A multi-byte UTF-8 sequence, copied whole.
                            let start = self.at - 1;
                            let width = match lead {
                                0xc0..=0xdf => 2,
                                0xe0..=0xef => 3,
                                _ => 4,
                            };
                            self.at = start + width;
                            let sequence =
                                self.bytes.get(start..self.at).expect("a whole sequence");
                            text.push_str(std::str::from_utf8(sequence).expect("UTF-8"));
                        }
                    }
                }
            }
            b't' => {
                self.expect(b"true");
                Json::True
            }
            b'f' => {
                self.expect(b"false");
                Json::False
            }
            b'n' => {
                self.expect(b"null");
                Json::Null
            }
            _ => {
                let start = self.at;
                while matches!(
                    self.peek(),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.at += 1;
                }
                let digits = self.bytes.get(start..self.at).expect("in bounds");
                assert!(!digits.is_empty(), "a value at {start}");
                Json::Number(String::from_utf8(digits.to_vec()).expect("ASCII"))
            }
        }
    }
}

/// One DER TLV: its tag, its contents and what follows it. Definite lengths
/// only, which is all DER has.
fn tlv(input: &[u8]) -> (u8, &[u8], &[u8]) {
    let (&tag, rest) = input.split_first().expect("a tag");
    let (&first, rest) = rest.split_first().expect("a length");
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        assert!(
            (1..=4).contains(&count),
            "a definite length of 1 to 4 octets"
        );
        let (octets, rest) = rest.split_at(count);
        let length = octets
            .iter()
            .fold(0usize, |length, &octet| (length << 8) | usize::from(octet));
        (length, rest)
    };
    assert!(length <= rest.len(), "the contents fit");
    let (contents, rest) = rest.split_at(length);
    (tag, contents, rest)
}

/// An RSA private key's parts, as RFC 8017 A.1.2's `RSAPrivateKey` lists
/// them.
struct RsaParts {
    n: Vec<u8>,
    e: Vec<u8>,
    d: Vec<u8>,
    p: Vec<u8>,
    q: Vec<u8>,
    dp: Vec<u8>,
    dq: Vec<u8>,
    q_inv: Vec<u8>,
}

/// The `RSAPrivateKey` inside a PKCS#8 `PrivateKeyInfo` (RFC 5208 §5): a
/// version, `rsaEncryption`, and the key as an `OCTET STRING`. A two-prime
/// key is version 0 with exactly nine `INTEGER`s.
fn pkcs8_rsa_parts(der: &[u8]) -> RsaParts {
    let (tag, info, rest) = tlv(der);
    assert_eq!(
        (tag, rest.len()),
        (0x30, 0),
        "PrivateKeyInfo is one SEQUENCE"
    );
    let (tag, version, info) = tlv(info);
    assert_eq!(
        (tag, version),
        (0x02, &[0u8][..]),
        "PrivateKeyInfo version 0"
    );
    let (tag, algorithm, info) = tlv(info);
    assert_eq!(tag, 0x30, "an AlgorithmIdentifier");
    let (tag, oid, parameters) = tlv(algorithm);
    assert_eq!(
        (tag, oid),
        (
            0x06,
            &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01][..]
        ),
        "rsaEncryption"
    );
    assert_eq!(parameters, [0x05, 0x00], "with NULL parameters");
    let (tag, key, _) = tlv(info);
    assert_eq!(tag, 0x04, "the key is an OCTET STRING");
    let (tag, mut fields, rest) = tlv(key);
    assert_eq!(
        (tag, rest.len()),
        (0x30, 0),
        "RSAPrivateKey is one SEQUENCE"
    );
    let mut integers = Vec::new();
    while !fields.is_empty() {
        let (tag, value, rest) = tlv(fields);
        assert_eq!(tag, 0x02, "every RSAPrivateKey field is an INTEGER");
        integers.push(value.to_vec());
        fields = rest;
    }
    let [version, n, e, d, p, q, dp, dq, q_inv]: [Vec<u8>; 9] = integers
        .try_into()
        .expect("a two-prime key has nine INTEGERs");
    assert_eq!(version, [0], "two-prime");
    RsaParts {
        n,
        e,
        d,
        p,
        q,
        dp,
        dq,
        q_inv,
    }
}

/// A big-endian integer without the sign octets DER and JSON put in front.
fn magnitude(bytes: &[u8]) -> &[u8] {
    let zeros = bytes.iter().take_while(|&&byte| byte == 0).count();
    bytes.get(zeros..).unwrap_or(&[])
}

/// One Wycheproof test group: a key and what it signs.
struct WycheproofGroup {
    sha: String,
    key_size: u64,
    parts: RsaParts,
    tests: Vec<WycheproofTest>,
}

struct WycheproofTest {
    id: u64,
    message: Vec<u8>,
    signature: Vec<u8>,
    result: String,
    flags: Vec<String>,
}

/// Reads a Wycheproof `RsassaPkcs1Generate` file: its declared test count,
/// and each group with the private key its PKCS#8 carries, held to the
/// group's own `privateKey` numbers.
fn wycheproof_groups(bytes: &[u8]) -> (u64, Vec<WycheproofGroup>) {
    let document = JsonReader::document(bytes);
    assert_eq!(document.field("algorithm").text(), "RSASSA-PKCS1-v1_5");
    let declared = document.field("numberOfTests").number();
    let mut groups = Vec::new();
    for group in document.field("testGroups").items() {
        assert_eq!(group.field("type").text(), "RsassaPkcs1Generate");
        let parts = pkcs8_rsa_parts(&unhex(group.field("privateKeyPkcs8").text()));
        let numbers = group.field("privateKey");
        assert_eq!(
            magnitude(&parts.n),
            magnitude(&unhex(numbers.field("modulus").text()))
        );
        assert_eq!(
            magnitude(&parts.e),
            magnitude(&unhex(numbers.field("publicExponent").text()))
        );
        assert_eq!(
            magnitude(&parts.d),
            magnitude(&unhex(numbers.field("privateExponent").text()))
        );
        let tests = group
            .field("tests")
            .items()
            .iter()
            .map(|test| WycheproofTest {
                id: test.field("tcId").number(),
                message: unhex(test.field("msg").text()),
                signature: unhex(test.field("sig").text()),
                result: test.field("result").text().to_string(),
                flags: test
                    .field("flags")
                    .items()
                    .iter()
                    .map(|flag| flag.text().to_string())
                    .collect(),
            })
            .collect();
        groups.push(WycheproofGroup {
            sha: group.field("sha").text().to_string(),
            key_size: group.field("keySize").number(),
            parts,
            tests,
        });
    }
    (declared, groups)
}

// ---- The vectors load (SG-01.3) -------------------------------------------

/// Every file reads, every count is what the file says it is, and every byte
/// is the byte that was fetched.
///
/// CAVP: two keys a file, ten signatures for every digest the file names,
/// every key carrying its private exponent, every PSS salt the length its
/// header declares. Wycheproof: the test count each file declares, every
/// group's PKCS#8 a two-prime key whose numbers are the group's own, and
/// `result` one of the two a generation vector can carry.
#[test]
fn the_signature_generation_vectors_load() {
    let expected_digests: [&[&str]; 4] = [
        &["SHA-224", "SHA-256", "SHA-384", "SHA-512"],
        &["SHA-512/224", "SHA-512/256"],
        &["SHA-224", "SHA-256", "SHA-384", "SHA-512"],
        &["SHA-512/224", "SHA-512/256"],
    ];
    let mut cavp_records = 0usize;
    for ((name, text, member_sha256), digests) in CAVP.iter().zip(expected_digests) {
        assert_cavp_provenance(name, text, member_sha256);
        let keys = cavp_keys(text);
        let records: usize = keys.iter().map(|key| key.records.len()).sum();
        println!("RAN {name}: {} keys, {records} signatures", keys.len());
        assert_eq!(
            keys.iter().map(|key| key.bits).collect::<Vec<_>>(),
            [2048, 3072],
            "{name}"
        );
        let pss = name.contains("pss");
        let declared = if pss {
            declared_salt_lengths(text)
        } else {
            Vec::new()
        };
        for key in &keys {
            assert!(
                !key.n.is_empty() && !key.e.is_empty() && !key.d.is_empty(),
                "{name}: every key has n, e and d"
            );
            assert_eq!(
                magnitude(&key.n).len() * 8,
                key.bits,
                "{name}: n fills its size"
            );
            assert_eq!(key.records.len(), 10 * digests.len(), "{name}");
            for digest in digests {
                assert_eq!(
                    key.records
                        .iter()
                        .filter(|record| record.digest == *digest)
                        .count(),
                    10,
                    "{name}: {digest} under the {}-bit key",
                    key.bits
                );
            }
            for record in &key.records {
                assert_eq!(
                    record.message.len(),
                    128,
                    "{name}: SigGen's 1024-bit messages"
                );
                assert_eq!(record.signature.len() * 8, key.bits, "{name}: k octets");
                assert_eq!(
                    record.salt.is_some(),
                    pss,
                    "{name}: a salt where PSS needs one"
                );
                if pss {
                    let _ = salt_of(record, key.bits, &declared);
                }
            }
        }
        cavp_records += records;
    }
    assert_eq!(cavp_records, 240, "80 + 40 + 80 + 40");

    let (license, license_sha256) = WYCHEPROOF_LICENSE;
    assert_eq!(
        hex(&sha256(license)),
        license_sha256,
        "Wycheproof's LICENSE"
    );
    assert!(
        std::str::from_utf8(license)
            .expect("text")
            .contains("Apache License"),
        "the licence the vectors are published under"
    );

    let mut wycheproof_tests = 0u64;
    for (name, bytes, fetched_sha256) in WYCHEPROOF {
        assert_eq!(
            hex(&sha256(bytes)),
            fetched_sha256,
            "{name} is the file that was fetched"
        );
        let (declared, groups) = wycheproof_groups(bytes);
        let tests: u64 = groups.iter().map(|group| group.tests.len() as u64).sum();
        let mut moduli: Vec<&[u8]> = groups
            .iter()
            .map(|group| magnitude(&group.parts.n))
            .collect();
        moduli.sort_unstable();
        moduli.dedup();
        println!(
            "RAN {name}: {} keys, {} groups, {tests} signatures",
            moduli.len(),
            groups.len()
        );
        assert_eq!(tests, declared, "{name}: numberOfTests");
        let mut empty_messages = 0usize;
        for group in &groups {
            let parts = &group.parts;
            let n_len = magnitude(&parts.n).len();
            assert_eq!(
                n_len as u64 * 8,
                group.key_size,
                "{name}: the key is the size the group says"
            );
            let (p_len, q_len) = (magnitude(&parts.p).len(), magnitude(&parts.q).len());
            assert!(
                p_len + q_len == n_len || p_len + q_len == n_len + 1,
                "{name}: two primes whose lengths make the modulus's"
            );
            assert!(
                magnitude(&parts.dp).len() <= p_len
                    && magnitude(&parts.dq).len() <= q_len
                    && magnitude(&parts.q_inv).len() <= p_len,
                "{name}: the CRT exponents and the coefficient no longer than their primes"
            );
            assert!(
                ["SHA-1", "SHA-224", "SHA-256", "SHA-384", "SHA-512"].contains(&group.sha.as_str()),
                "{name}: {}",
                group.sha
            );
            let small_exponent = magnitude(&parts.e) == [3];
            for test in &group.tests {
                assert!(
                    test.result == "valid" || test.result == "acceptable",
                    "{name} #{}: a generation vector is a signature to make",
                    test.id
                );
                assert_eq!(
                    test.signature.len() as u64 * 8,
                    group.key_size,
                    "{name} #{}: k octets",
                    test.id
                );
                // The file's own notes say why a signature is only
                // acceptable, and each note follows from the group.
                let flagged = |note: &str| test.flags.iter().any(|flag| flag == note);
                assert_eq!(
                    flagged("WeakHash"),
                    group.sha == "SHA-1",
                    "{name} #{}",
                    test.id
                );
                assert_eq!(
                    flagged("SmallModulus"),
                    group.key_size < 2048,
                    "{name} #{}",
                    test.id
                );
                assert_eq!(
                    flagged("SmallPublicKey"),
                    small_exponent,
                    "{name} #{}",
                    test.id
                );
                if test.message.is_empty() {
                    empty_messages += 1;
                }
            }
        }
        assert!(
            empty_messages > 0,
            "{name}: the empty message among the rest"
        );
        wycheproof_tests += tests;
    }
    assert_eq!(wycheproof_tests, 33 + 32 + 43 + 26 + 24);
}

// ---- Keys from their published numbers ---------------------------------------

type Big = Uint<RSA_LIMBS>;

fn big(bytes: &[u8]) -> Big {
    Big::from_be_bytes(bytes).expect("fits 4096 bits")
}

/// A value's big-endian octets, without leading zeros.
fn octets(value: &Big) -> Vec<u8> {
    let mut bytes = vec![0u8; RSA_LIMBS * 8];
    assert!(value.to_be_bytes(&mut bytes));
    let zeros = bytes.iter().take_while(|&&byte| byte == 0).count();
    bytes.split_off(zeros.min(bytes.len() - 1))
}

/// `value >> 1`, through the value's octets: the public `Uint` has no shift
/// right, and this is test arithmetic on published numbers.
fn halve(value: &Big) -> Big {
    let mut bytes = vec![0u8; RSA_LIMBS * 8];
    assert!(value.to_be_bytes(&mut bytes));
    let mut carry = 0u8;
    for byte in &mut bytes {
        let low = *byte & 1;
        *byte = (*byte >> 1) | (carry << 7);
        carry = low;
    }
    big(&bytes)
}

/// `value · small`, by shift and add over the multiplier's bits.
fn times(value: &Big, small: u64) -> Big {
    let mut product = Big::zero();
    let mut addend = *value;
    for bit in 0..64 {
        if small >> bit & 1 == 1 {
            let (sum, carried) = product.add_carry(&addend);
            assert!(!carried, "the product fits 4096 bits");
            product = sum;
        }
        addend = addend.shl_small(1);
    }
    product
}

/// `gcd(a, b)` for an odd `b`, by Stein's binary algorithm: factors of two
/// leave `a` freely, because the answer is odd.
fn gcd(mut a: Big, mut b: Big) -> Big {
    if a.is_zero() {
        return b;
    }
    while !a.bit(0) {
        a = halve(&a);
    }
    loop {
        while !b.bit(0) {
            b = halve(&b);
        }
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        b = b.sub_borrow(&a).0;
        if b.is_zero() {
            return a;
        }
    }
}

/// `value mod modulus`, one bit at a time, for any modulus: `d` is reduced
/// modulo `p - 1`, which Montgomery cannot take.
fn remainder(value: &Big, modulus: &Big) -> Big {
    let mut remainder = Big::zero();
    for i in (0..value.bits()).rev() {
        let (doubled, carried) = remainder.add_carry(&remainder);
        let mut next = doubled;
        if value.bit(i) {
            next = next.add_carry(&Big::from_u64(1)).0;
        }
        remainder = if carried || next >= *modulus {
            next.sub_borrow(modulus).0
        } else {
            next
        };
    }
    remainder
}

/// The primes of `n` from `e` and `d`, by the probabilistic factoring of NIST
/// SP 800-56B Rev. 2 appendix C.2: `e·d - 1 = 2^t · r` with `r` odd is a
/// multiple of `λ(n)`, so for most `g` some `y = g^(2^i · r)` is a square root
/// of one that is neither 1 nor `n - 1`, and `gcd(y - 1, n)` and
/// `gcd(y + 1, n)` are the primes.
///
/// CAVP's signature-generation files publish `n`, `e` and `d` and not the
/// primes; this recovers them so that the key the engine signs with is the
/// published key, whose published signatures it must then reproduce. A wrong
/// recovery would be refused by the key's own checks or by its signatures.
fn primes_of(n: &Big, e: &Big, d: &Big) -> (Big, Big) {
    let modulus = Modulus::new(*n).expect("an odd modulus");
    let one = Big::from_u64(1);
    let (n_minus_one, _) = n.sub_borrow(&one);
    assert!(e.bits() <= 64, "a public exponent of at most 64 bits");
    let (k, _) = times(d, e.limb(0)).sub_borrow(&one);
    let mut r = k;
    let mut t = 0usize;
    while !r.bit(0) {
        r = halve(&r);
        t += 1;
    }
    for g in 2u64..100 {
        let mut y = modulus.pow(&Big::from_u64(g), &r);
        if y == one || y == n_minus_one {
            continue;
        }
        for _ in 0..t {
            let x = modulus.mul_mod(&y, &y);
            if x == one {
                let p = gcd(y.sub_borrow(&one).0, *n);
                let q = gcd(y.add_carry(&one).0, *n);
                return (p, q);
            }
            if x == n_minus_one {
                break;
            }
            y = x;
        }
    }
    panic!("no square root of one gave n's factors");
}

/// A key's eight parts, from the three a CAVP file publishes: the primes
/// recovered, `dP` and `dQ` reduced from `d`, `qInv` by Fermat under `p`.
fn cavp_parts(key: &CavpKey) -> [Vec<u8>; 8] {
    let (n, e, d) = (big(&key.n), big(&key.e), big(&key.d));
    let (p, q) = primes_of(&n, &e, &d);
    let one = Big::from_u64(1);
    let dp = remainder(&d, &p.sub_borrow(&one).0);
    let dq = remainder(&d, &q.sub_borrow(&one).0);
    let q_inv = Modulus::new(p)
        .and_then(|modulus| modulus.inverse_prime(&q))
        .expect("q is invertible under the prime p");
    [
        key.n.clone(),
        key.e.clone(),
        key.d.clone(),
        octets(&p),
        octets(&q),
        octets(&dp),
        octets(&dq),
        octets(&q_inv),
    ]
}

fn wycheproof_parts(parts: &RsaParts) -> [Vec<u8>; 8] {
    [
        parts.n.clone(),
        parts.e.clone(),
        parts.d.clone(),
        parts.p.clone(),
        parts.q.clone(),
        parts.dp.clone(),
        parts.dq.clone(),
        parts.q_inv.clone(),
    ]
}

fn private_key(parts: &[Vec<u8>; 8]) -> RsaPrivateKey {
    let [n, e, d, p, q, dp, dq, q_inv] = parts;
    RsaPrivateKey::new(&RsaPrivateComponents {
        modulus: n,
        public_exponent: e,
        private_exponent: d,
        prime1: p,
        prime2: q,
        exponent1: dp,
        exponent2: dq,
        coefficient: q_inv,
    })
    .expect("a published key is a key")
}

/// The digest a vector file names, by FIPS 180-4's name.
fn digest_named(name: &str) -> DigestAlgorithm {
    match name {
        "SHA-1" => DigestAlgorithm::Sha1,
        "SHA-224" => DigestAlgorithm::Sha224,
        "SHA-256" => DigestAlgorithm::Sha256,
        "SHA-384" => DigestAlgorithm::Sha384,
        "SHA-512" => DigestAlgorithm::Sha512,
        "SHA-512/224" => DigestAlgorithm::Sha512_224,
        "SHA-512/256" => DigestAlgorithm::Sha512_256,
        other => panic!("a digest no file was expected to name: {other}"),
    }
}

/// A deterministic stand-in for a host's source: SHA-256 over a seed and a
/// counter. Any source serves — blinding moves no signature — and a fixed one
/// makes a failure repeatable.
struct Stream {
    seed: u64,
    counter: u64,
}

impl Stream {
    fn new(seed: u64) -> Self {
        Self { seed, counter: 0 }
    }
}

impl EntropySource for Stream {
    fn fill(&mut self, out: &mut [u8]) -> bool {
        for chunk in out.chunks_mut(32) {
            let mut input = self.seed.to_be_bytes().to_vec();
            input.extend_from_slice(&self.counter.to_be_bytes());
            let block = sha256(&input);
            chunk.copy_from_slice(block.get(..chunk.len()).expect("a block is 32 octets"));
            self.counter += 1;
        }
        true
    }
}

// ---- RSASSA-PKCS1-v1_5 (SG-01.5) ---------------------------------------------

/// SG-01.5's exit, CAVP's half: all 120 of the `SigGen15` files' signatures,
/// made by the engine from each message under its published key, octet for
/// octet — ten for each of SHA-224, SHA-256, SHA-384 and SHA-512 and of
/// SHA-512/224 and SHA-512/256 under a 2048- and a 3072-bit key. Each
/// published signature also verifies under the key's public half, which is
/// the verifier's side of the `DigestInfo` prefixes the two share.
#[test]
fn cavp_pkcs1_v15_generation_vectors_pass() {
    let mut entropy = Stream::new(0x5347_2d30_312e_35);
    let mut by_digest = BTreeMap::<&str, usize>::new();
    for (name, text, _) in [CAVP[0], CAVP[1]] {
        for vector in cavp_keys(text) {
            let key = private_key(&cavp_parts(&vector));
            assert_eq!(key.modulus_bits(), vector.bits, "{name}");
            for record in &vector.records {
                let algorithm = digest_named(record.digest);
                assert_eq!(
                    key.sign_pkcs1_v15_message(algorithm, &record.message, &mut entropy)
                        .as_ref(),
                    Ok(&record.signature),
                    "{name}: {} under the {}-bit key",
                    record.digest,
                    vector.bits
                );
                assert_eq!(
                    key.public_key().verify_pkcs1_v15_message(
                        algorithm,
                        &record.message,
                        &record.signature
                    ),
                    Ok(()),
                    "{name}: {} verifies",
                    record.digest
                );
                *by_digest.entry(record.digest).or_default() += 1;
            }
        }
    }
    println!("RAN CAVP SigGen15: {by_digest:?}");
    let expected: BTreeMap<&str, usize> = [
        ("SHA-224", 20),
        ("SHA-256", 20),
        ("SHA-384", 20),
        ("SHA-512", 20),
        ("SHA-512/224", 20),
        ("SHA-512/256", 20),
    ]
    .into_iter()
    .collect();
    assert_eq!(by_digest, expected);
}

/// SG-01.5's exit, Wycheproof's half: every signature of the five
/// generation files, 1 024 to 4 096 bits, made under each group's own key —
/// the acceptable ones among them, a three-exponent key's signatures with
/// leading zero octets and one close to `n`, which a signer that trimmed its
/// output or reduced it wrongly would miss — for every SHA-2 digest the files
/// cover. Their SHA-1 groups are refused by name, the published signatures
/// still verifying: SHA-1 is read and never written.
#[test]
fn wycheproof_rsa_signature_vectors_pass() {
    let mut entropy = Stream::new(0x7779_6368);
    let mut signed = BTreeMap::<String, usize>::new();
    let mut refused = 0usize;
    for (name, bytes, _) in WYCHEPROOF {
        let (_, groups) = wycheproof_groups(bytes);
        for group in &groups {
            let key = private_key(&wycheproof_parts(&group.parts));
            let algorithm = digest_named(&group.sha);
            for test in &group.tests {
                let made = key.sign_pkcs1_v15_message(algorithm, &test.message, &mut entropy);
                if algorithm.signs() {
                    assert_eq!(
                        made.as_ref(),
                        Ok(&test.signature),
                        "{name} #{} ({}, {})",
                        test.id,
                        group.sha,
                        test.result
                    );
                    *signed.entry(group.sha.clone()).or_default() += 1;
                } else {
                    assert_eq!(
                        made,
                        Err(RsaRefusal::DigestNotForSigning),
                        "{name} #{}",
                        test.id
                    );
                    refused += 1;
                }
                assert_eq!(
                    key.public_key().verify_pkcs1_v15_message(
                        algorithm,
                        &test.message,
                        &test.signature
                    ),
                    Ok(()),
                    "{name} #{} verifies",
                    test.id
                );
            }
        }
    }
    println!("RAN Wycheproof RsassaPkcs1Generate: {signed:?}, {refused} SHA-1 refused");
    let expected: BTreeMap<String, usize> = [
        ("SHA-224", 24),
        ("SHA-256", 44),
        ("SHA-384", 40),
        ("SHA-512", 34),
    ]
    .into_iter()
    .map(|(name, count)| (name.to_string(), count))
    .collect();
    assert_eq!(signed, expected);
    assert_eq!(
        refused, 16,
        "the SHA-1 groups of the 1024- and 2048-bit files"
    );
}

// ---- RSASSA-PSS (SG-01.6) ------------------------------------------------------

/// SG-01.6's exit: all 120 of the `SigGenPSS` files' signatures, made by the
/// engine from each message under its published key with the salt the file
/// prints, octet for octet — ten for each of SHA-224, SHA-256, SHA-384 and
/// SHA-512 under a 2048- and a 3072-bit key, with salts of the fifteen to
/// sixty-two octets the CAVS header gives each, and ten for each of
/// SHA-512/224 and SHA-512/256 with the empty salt. MGF1 runs over the
/// message digest, as every record of the files does. Each published
/// signature also verifies under the same parameters.
#[test]
fn cavp_pss_generation_vectors_pass_with_their_salts() {
    let mut entropy = Stream::new(0x5347_2d30_312e_36);
    let mut by_digest = BTreeMap::<&str, usize>::new();
    let mut empty_salts = 0usize;
    for (name, text, _) in [CAVP[2], CAVP[3]] {
        let declared = declared_salt_lengths(text);
        for vector in cavp_keys(text) {
            let key = private_key(&cavp_parts(&vector));
            assert_eq!(key.modulus_bits(), vector.bits, "{name}");
            for record in &vector.records {
                let salt = salt_of(record, vector.bits, &declared);
                let hash = digest_named(record.digest);
                let parameters = PssParameters {
                    hash,
                    mask_hash: hash,
                    salt_length: salt.len(),
                };
                assert_eq!(
                    key.sign_pss_message(parameters, &record.message, &salt, &mut entropy)
                        .as_ref(),
                    Ok(&record.signature),
                    "{name}: {} with a {}-octet salt under the {}-bit key",
                    record.digest,
                    salt.len(),
                    vector.bits
                );
                assert_eq!(
                    key.public_key().verify_pss_message(
                        parameters,
                        &record.message,
                        &record.signature
                    ),
                    Ok(()),
                    "{name}: {} verifies",
                    record.digest
                );
                if salt.is_empty() {
                    empty_salts += 1;
                }
                *by_digest.entry(record.digest).or_default() += 1;
            }
        }
    }
    println!("RAN CAVP SigGenPSS: {by_digest:?}, {empty_salts} with the empty salt");
    let expected: BTreeMap<&str, usize> = [
        ("SHA-224", 20),
        ("SHA-256", 20),
        ("SHA-384", 20),
        ("SHA-512", 20),
        ("SHA-512/224", 20),
        ("SHA-512/256", 20),
    ]
    .into_iter()
    .collect();
    assert_eq!(by_digest, expected);
    assert_eq!(empty_salts, 40, "every SHA-512/224 and SHA-512/256 record");
}
