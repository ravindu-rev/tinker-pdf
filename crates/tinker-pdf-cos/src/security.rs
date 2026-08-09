//! Wiring the standard security handler to the document (7.6).
//!
//! [`crate::decrypt`] owns the call sites and their exemptions; this module
//! owns the translation between `/Encrypt`'s vocabulary and the plain values
//! `tinker-pdf-crypto` consumes, and the decryptor that results.

use std::sync::Arc;

use tinker_pdf_crypto::handler::{self, CryptMethod, FileKey, HandlerParams};
use tinker_pdf_crypto::{AuthOutcome, Permissions};

use crate::decrypt::{Decryptor, EncryptParams};
use crate::object::ObjRef;

/// How far a password got.
///
/// Distinguishing the two levels is the point: a document opened with the
/// owner password is unrestricted, and an engine that only knows "a password
/// worked" cannot say so. Ordered, so `>=` expresses "at least this much
/// authority".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthLevel {
    /// The document is not encrypted, or no password has been accepted.
    #[default]
    None,
    /// The user password matched; the document's permissions apply.
    User,
    /// The owner password matched; restrictions are lifted.
    Owner,
}

/// Why authentication did not succeed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthError {
    /// The document is not encrypted, so there is nothing to authenticate.
    NotEncrypted,
    /// `/Filter` names a handler this engine does not implement — public-key
    /// security, or a vendor's own.
    UnsupportedHandler,
    /// Neither the user nor the owner password matched.
    WrongPassword,
}

/// The decryptor built from an authenticated [`FileKey`].
pub struct StandardDecryptor {
    key: FileKey,
}

impl StandardDecryptor {
    /// How far the password that produced this decryptor got.
    #[must_use]
    pub fn auth_level(&self) -> AuthLevel {
        match self.key.outcome() {
            AuthOutcome::Owner => AuthLevel::Owner,
            AuthOutcome::User => AuthLevel::User,
            AuthOutcome::Failed => AuthLevel::None,
        }
    }
}

impl Decryptor for StandardDecryptor {
    fn decrypt_string(&self, containing: ObjRef, data: &[u8]) -> Vec<u8> {
        self.key
            .decrypt_string(containing.num, containing.gen, data)
    }

    fn decrypt_stream(&self, r: ObjRef, data: &[u8]) -> Vec<u8> {
        self.key.decrypt_stream(r.num, r.gen, data)
    }
}

/// Resolves `/StmF` or `/StrF` through `/CF` into a concrete method (7.6.5).
fn method_for(params: &EncryptParams, filter_name: Option<&[u8]>) -> CryptMethod {
    // 7.6.5 Table 24: both default to /Identity, and /Identity is never
    // looked up in /CF.
    let Some(name) = filter_name else {
        return CryptMethod::Identity;
    };
    if name == b"Identity" {
        return CryptMethod::Identity;
    }

    let Some(entry) = params.crypt_filters.iter().find(|f| f.name == name) else {
        return CryptMethod::Identity;
    };

    match entry.cfm.as_deref() {
        Some(b"V2") => CryptMethod::Rc4,
        Some(b"AESV2") => CryptMethod::AesV2,
        Some(b"AESV3") => CryptMethod::AesV3,
        // /None, absent, or a name from a future revision.
        _ => CryptMethod::Identity,
    }
}

/// Flattens `/Encrypt` into the handler's inputs.
fn to_handler_params(params: &EncryptParams) -> HandlerParams {
    let v = params.v.unwrap_or(0);
    let r = params.r.unwrap_or(0);

    // 7.6.5: crypt filters exist only from /V 4. Before that, everything is
    // RC4 with the file key.
    let (stream_method, string_method) = if v >= 4 {
        (
            method_for(params, params.stm_f.as_deref()),
            method_for(params, params.str_f.as_deref()),
        )
    } else {
        (CryptMethod::Rc4, CryptMethod::Rc4)
    };

    HandlerParams {
        v,
        r,
        length_bits: params.length.unwrap_or(0),
        o: params.o.clone().unwrap_or_default(),
        u: params.u.clone().unwrap_or_default(),
        oe: params.oe.clone().unwrap_or_default(),
        ue: params.ue.clone().unwrap_or_default(),
        perms: params.perms.clone().unwrap_or_default(),
        // /P is a 32-bit signed value the file may store as any integer.
        p: params.p.unwrap_or(-1) as i32,
        id_first: params.id_first.clone().unwrap_or_default(),
        encrypt_metadata: params.encrypt_metadata,
        stream_method,
        string_method,
    }
}

/// A successful authentication: what to install, how far it got, and what the
/// handler tolerated on the way.
pub struct Authenticated {
    /// The decryptor to install on the document.
    pub decryptor: Arc<dyn Decryptor>,
    /// Which password matched.
    pub level: AuthLevel,
    /// Leniency the handler applied, for the document's warning list.
    pub notes: Vec<handler::HandlerNote>,
}

/// Attempts `password` and, on success, returns the decryptor to install.
///
/// The owner password is tried first, so a document whose passwords are equal
/// authenticates at the higher level.
pub fn authenticate(params: &EncryptParams, password: &str) -> Result<Authenticated, AuthError> {
    match params.filter.as_deref() {
        // An absent /Filter is a malformed file; the standard handler is the
        // only plausible reading and costs nothing to try.
        None | Some(b"Standard") => {}
        Some(_) => return Err(AuthError::UnsupportedHandler),
    }

    let hp = to_handler_params(params);
    let key = handler::authenticate(&hp, password.as_bytes()).ok_or(AuthError::WrongPassword)?;

    let notes = key.notes().to_vec();
    let decryptor = StandardDecryptor { key };
    let level = decryptor.auth_level();
    Ok(Authenticated {
        decryptor: Arc::new(decryptor),
        level,
        notes,
    })
}

/// The document's permission flags, given how far authentication got.
///
/// An unencrypted document permits everything, and so does one opened with the
/// owner password: the flags remain in the file but no longer restrict.
#[must_use]
pub fn permissions(params: Option<&EncryptParams>, level: AuthLevel) -> Permissions {
    match (params, level) {
        (Some(p), AuthLevel::User) => Permissions::from_raw(p.p.unwrap_or(-1) as i32),
        _ => Permissions::all(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decrypt::CryptFilterParams;

    fn params_with_filters() -> EncryptParams {
        EncryptParams {
            v: Some(4),
            r: Some(4),
            stm_f: Some(b"StdCF".to_vec()),
            str_f: Some(b"StdCF".to_vec()),
            crypt_filters: vec![CryptFilterParams {
                name: b"StdCF".to_vec(),
                cfm: Some(b"AESV2".to_vec()),
                ..CryptFilterParams::default()
            }],
            ..EncryptParams::default()
        }
    }

    #[test]
    fn crypt_filters_resolve_through_cf() {
        let hp = to_handler_params(&params_with_filters());
        assert_eq!(hp.stream_method, CryptMethod::AesV2);
        assert_eq!(hp.string_method, CryptMethod::AesV2);
    }

    #[test]
    fn identity_is_never_looked_up() {
        let mut p = params_with_filters();
        p.stm_f = Some(b"Identity".to_vec());
        let hp = to_handler_params(&p);
        assert_eq!(hp.stream_method, CryptMethod::Identity);
        assert_eq!(hp.string_method, CryptMethod::AesV2, "/StrF is unaffected");
    }

    #[test]
    fn a_missing_filter_name_defaults_to_identity() {
        let mut p = params_with_filters();
        p.stm_f = None;
        p.str_f = Some(b"NoSuchFilter".to_vec());
        let hp = to_handler_params(&p);
        assert_eq!(hp.stream_method, CryptMethod::Identity);
        assert_eq!(hp.string_method, CryptMethod::Identity);
    }

    #[test]
    fn before_v4_everything_is_rc4() {
        let p = EncryptParams {
            v: Some(2),
            r: Some(3),
            ..EncryptParams::default()
        };
        let hp = to_handler_params(&p);
        assert_eq!(hp.stream_method, CryptMethod::Rc4);
        assert_eq!(hp.string_method, CryptMethod::Rc4);
    }

    #[test]
    fn a_foreign_handler_is_refused_rather_than_guessed() {
        let p = EncryptParams {
            filter: Some(b"Adobe.PubSec".to_vec()),
            ..EncryptParams::default()
        };
        assert_eq!(
            authenticate(&p, "anything").err(),
            Some(AuthError::UnsupportedHandler)
        );
    }

    #[test]
    fn permissions_apply_only_under_user_authentication() {
        let p = EncryptParams {
            p: Some(-2056),
            ..EncryptParams::default()
        };
        assert!(!permissions(Some(&p), AuthLevel::User).print());
        assert!(
            permissions(Some(&p), AuthLevel::Owner).print(),
            "the owner password lifts restrictions"
        );
        assert!(permissions(None, AuthLevel::None).print());
    }
}
