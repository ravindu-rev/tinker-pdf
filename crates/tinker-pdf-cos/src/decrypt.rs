//! The encryption seam (7.6).
//!
//! This module owns the call sites and their exemptions, not a single cipher.
//! Strings decrypt when their containing indirect object loads, streams
//! decrypt in [`crate::CosDocument::stream_raw`] and
//! [`crate::CosDocument::stream_decoded`], and three things are never handed
//! to a decryptor at all (7.6.2): the `/Encrypt` dictionary itself,
//! cross-reference streams, and strings inside object streams — the object
//! stream was decrypted as a whole, so its contents already are.
//!
//! [`EncryptParams`] is the handoff to `docs/plans/03-encryption.md`: plain
//! integers, booleans and byte strings, no COS types, because
//! `tinker-pdf-crypto` is a leaf crate.

use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};

/// What decrypts a document's strings and streams.
///
/// Implementations are supplied by the security handler; this crate ships
/// only [`IdentityDecryptor`] and the wiring. A decryptor is called once per
/// string with the *containing indirect object's* reference, because the
/// standard security handler salts the object key with it (7.6.2).
pub trait Decryptor: Send + Sync {
    /// Decrypts one string, given the indirect object it was found in.
    fn decrypt_string(&self, containing: ObjRef, data: &[u8]) -> Vec<u8>;

    /// Decrypts one stream's raw bytes.
    fn decrypt_stream(&self, r: ObjRef, data: &[u8]) -> Vec<u8>;
}

/// The decryptor of an unencrypted document: bytes through unchanged.
///
/// Also the default for an encrypted document until a security handler is
/// installed, which is why every tier of the stream API works — and returns
/// ciphertext — before a password is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdentityDecryptor;

impl Decryptor for IdentityDecryptor {
    fn decrypt_string(&self, _containing: ObjRef, data: &[u8]) -> Vec<u8> {
        data.to_vec()
    }

    fn decrypt_stream(&self, _r: ObjRef, data: &[u8]) -> Vec<u8> {
        data.to_vec()
    }
}

/// One entry of the `/CF` crypt-filter dictionary (7.6.5), as plain values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CryptFilterParams {
    /// The filter's name in `/CF`, e.g. `StdCF`.
    pub name: Vec<u8>,
    /// `/CFM`: `None`, `V2`, `AESV2`, `AESV3`.
    pub cfm: Option<Vec<u8>>,
    /// `/AuthEvent`: `DocOpen` or `EFOpen`.
    pub auth_event: Option<Vec<u8>>,
    /// `/Length`, in bytes or bits depending on the producer — 7.6.5 is
    /// ambiguous and both spellings occur, so the raw number is passed on.
    pub length: Option<i64>,
}

/// Everything `/Encrypt` and `/ID` say, flattened to values a crate with no
/// PDF types can consume.
///
/// Absent entries stay `None`: applying the spec's defaults is the security
/// handler's job, because the defaults differ by `/V` and `/R`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncryptParams {
    /// The indirect object the `/Encrypt` dictionary lives in, when it is
    /// indirect. Its own strings are never decrypted (7.6.2).
    pub encrypt_ref: Option<ObjRef>,
    /// `/Filter`: the security handler's name, e.g. `Standard`.
    pub filter: Option<Vec<u8>>,
    /// `/SubFilter`.
    pub sub_filter: Option<Vec<u8>>,
    /// `/V`, the algorithm selector (7.6.1 Table 20).
    pub v: Option<i64>,
    /// `/R`, the standard handler's revision (7.6.4.4 Table 21).
    pub r: Option<i64>,
    /// `/Length`, the key length in bits.
    pub length: Option<i64>,
    /// `/P`, the permission flags, as the signed integer the file stores.
    pub p: Option<i64>,
    /// `/O`, the owner password entry.
    pub o: Option<Vec<u8>>,
    /// `/U`, the user password entry.
    pub u: Option<Vec<u8>>,
    /// `/OE` (revision 6).
    pub oe: Option<Vec<u8>>,
    /// `/UE` (revision 6).
    pub ue: Option<Vec<u8>>,
    /// `/Perms` (revision 6).
    pub perms: Option<Vec<u8>>,
    /// `/EncryptMetadata`, whose default is true.
    pub encrypt_metadata: bool,
    /// `/StmF`, the crypt filter for streams.
    pub stm_f: Option<Vec<u8>>,
    /// `/StrF`, the crypt filter for strings.
    pub str_f: Option<Vec<u8>>,
    /// `/EFF`, the crypt filter for embedded file streams.
    pub eff: Option<Vec<u8>>,
    /// `/CF`, in file order.
    pub crypt_filters: Vec<CryptFilterParams>,
    /// The first element of the trailer's `/ID` array, which every key
    /// derivation before revision 6 mixes in (7.6.3.3).
    pub id_first: Option<Vec<u8>>,
}

/// Pulls the scalars out of an already-resolved `/Encrypt` dictionary.
///
/// `names` supplies the symbols; `trailer` supplies `/ID`. Values are read
/// direct-only: an indirect `/V` is not a thing any producer emits, and
/// resolving one here would mean decrypting objects to learn how to decrypt
/// objects.
pub(crate) fn extract(
    encrypt: &Dict,
    encrypt_ref: Option<ObjRef>,
    trailer: &Dict,
    names: &crate::doc::DocNames,
) -> EncryptParams {
    let bytes = |d: &Dict, key: Name| d.get_string(key).map(|s| s.bytes.clone());
    let name_bytes = |d: &Dict, key: Name| -> Option<Vec<u8>> {
        match d.get(key) {
            // /Filter and /StmF are names; a few producers write them as
            // strings. Both spellings decode to the same bytes.
            Some(Object::Name(n)) => names.table.bytes(*n).map(|b| b.to_vec()),
            Some(Object::String(s)) => Some(s.bytes.clone()),
            _ => None,
        }
    };

    let mut crypt_filters = Vec::new();
    if let Some(cf) = encrypt.get_dict(names.cf) {
        for (key, value) in cf.iter() {
            let Some(entry) = value.as_dict() else {
                continue;
            };
            crypt_filters.push(CryptFilterParams {
                name: names
                    .table
                    .bytes(*key)
                    .map(|b| b.to_vec())
                    .unwrap_or_default(),
                cfm: name_bytes(entry, names.cfm),
                auth_event: name_bytes(entry, names.auth_event),
                length: entry.get_int(Name::LENGTH),
            });
        }
    }

    let id_first = match trailer.get_array(Name::ID).and_then(<[Object]>::first) {
        Some(Object::String(s)) => Some(s.bytes.clone()),
        _ => None,
    };

    EncryptParams {
        encrypt_ref,
        filter: name_bytes(encrypt, Name::FILTER),
        sub_filter: name_bytes(encrypt, names.sub_filter),
        v: encrypt.get_int(names.v),
        r: encrypt.get_int(names.r),
        length: encrypt.get_int(Name::LENGTH),
        p: encrypt.get_int(names.p),
        o: bytes(encrypt, names.o),
        u: bytes(encrypt, names.u),
        oe: bytes(encrypt, names.oe),
        ue: bytes(encrypt, names.ue),
        perms: bytes(encrypt, names.perms),
        // 7.6.4.4 Table 21: the default is true.
        encrypt_metadata: encrypt.get_bool(names.encrypt_metadata).unwrap_or(true),
        stm_f: name_bytes(encrypt, names.stm_f),
        str_f: name_bytes(encrypt, names.str_f),
        eff: name_bytes(encrypt, names.eff),
        crypt_filters,
        id_first,
    }
}

/// Rewrites every string inside `object` in place (7.6.2).
///
/// Recursion is bounded by [`crate::limits::MAX_NEST_DEPTH`], which the object
/// parser already enforced when it built this value, so the bound is a belt
/// on top of braces rather than the only guard.
pub(crate) fn decrypt_strings(object: &mut Object, containing: ObjRef, d: &dyn Decryptor) {
    decrypt_strings_at(object, containing, d, 0);
}

fn decrypt_strings_at(object: &mut Object, containing: ObjRef, d: &dyn Decryptor, depth: u32) {
    if depth > crate::limits::MAX_NEST_DEPTH {
        return;
    }
    match object {
        Object::String(s) => s.bytes = d.decrypt_string(containing, &s.bytes),
        Object::Array(items) => {
            for item in items {
                decrypt_strings_at(item, containing, d, depth + 1);
            }
        }
        Object::Dict(dict) => {
            for value in dict.values_mut() {
                decrypt_strings_at(value, containing, d, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for value in stream.dict.values_mut() {
                decrypt_strings_at(value, containing, d, depth + 1);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PdfString;

    struct Upper;

    impl Decryptor for Upper {
        fn decrypt_string(&self, _containing: ObjRef, data: &[u8]) -> Vec<u8> {
            data.to_ascii_uppercase()
        }
        fn decrypt_stream(&self, _r: ObjRef, data: &[u8]) -> Vec<u8> {
            data.to_ascii_uppercase()
        }
    }

    #[test]
    fn identity_is_the_identity() {
        let d = IdentityDecryptor;
        assert_eq!(d.decrypt_string(ObjRef::new(1, 0), b"abc"), b"abc");
        assert_eq!(d.decrypt_stream(ObjRef::new(1, 0), b"abc"), b"abc");
    }

    #[test]
    fn strings_decrypt_at_every_depth() {
        let mut inner = Dict::new();
        inner.insert(
            Name::TYPE,
            Object::String(PdfString::literal(b"deep".to_vec())),
        );
        let mut object = Object::Array(vec![
            Object::String(PdfString::literal(b"top".to_vec())),
            Object::Dict(inner),
            Object::Int(3),
        ]);
        decrypt_strings(&mut object, ObjRef::new(4, 0), &Upper);
        let items = object.as_array().expect("an array");
        assert_eq!(
            items[0].as_string().map(|s| s.bytes.clone()),
            Some(b"TOP".to_vec())
        );
        assert_eq!(
            items[1]
                .as_dict()
                .and_then(|d| d.get_string(Name::TYPE))
                .map(|s| s.bytes.clone()),
            Some(b"DEEP".to_vec())
        );
        assert_eq!(items[2], Object::Int(3));
    }
}
