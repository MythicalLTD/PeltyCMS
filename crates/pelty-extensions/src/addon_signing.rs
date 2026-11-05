//! Detached Ed25519 release signatures. Trust comes only from operator keys.
use crate::addon_pkg::{sha256_hex, Package};
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::{
    rand::SystemRandom,
    signature::{self, Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SIGNATURE_FILE: &str = "addon.sig";
pub const MAX_SIGNATURE_BYTES: usize = 4096;
const DOMAIN: &[u8] = b"PeltyCMS addon release\0v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSignature {
    pub version: u32,
    pub algorithm: String,
    pub key_id: String,
    /// Standard padded base64, exactly 64 decoded bytes.
    pub signature: String,
}

/// Loaded from operator configuration, never from the package.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedKey {
    /// Standard padded base64, exactly 32 decoded bytes.
    pub public_key: String,
    #[serde(default)]
    pub revoked: bool,
}

pub type TrustedKeys = BTreeMap<String, TrustedKey>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SigningError {
    #[error("malformed addon.sig (expected v1 Ed25519 signature)")]
    MalformedSignature,
    #[error("a signed addon release is required")]
    Required,
    #[error("unknown addon signing key: {0}")]
    UnknownKey(String),
    #[error("revoked addon signing key: {0}")]
    RevokedKey(String),
    #[error("invalid trusted Ed25519 public key: {0}")]
    InvalidPublicKey(String),
    #[error("addon release signature verification failed")]
    InvalidSignature,
    #[error("invalid Ed25519 PKCS#8 private key")]
    InvalidPrivateKey,
    #[error("invalid signing key id")]
    InvalidKeyId,
    #[error("package content is inconsistent with its raw manifest")]
    InvalidContent,
    #[error("operating system random generator failed")]
    Random,
}

pub fn key_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

impl ReleaseSignature {
    pub fn parse(raw: &[u8]) -> Result<Self, SigningError> {
        if raw.len() > MAX_SIGNATURE_BYTES {
            return Err(SigningError::MalformedSignature);
        }
        let sig: Self =
            serde_json::from_slice(raw).map_err(|_| SigningError::MalformedSignature)?;
        sig.validate()?;
        Ok(sig)
    }

    fn validate(&self) -> Result<Vec<u8>, SigningError> {
        if self.version != 1
            || self.algorithm != "Ed25519"
            || !key_id_ok(&self.key_id)
            || self.signature.len() != 88
        {
            return Err(SigningError::MalformedSignature);
        }
        let bytes = STANDARD
            .decode(&self.signature)
            .map_err(|_| SigningError::MalformedSignature)?;
        if bytes.len() != 64 {
            return Err(SigningError::MalformedSignature);
        }
        Ok(bytes)
    }
}

fn field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Revalidate even a manually constructed/mutated Package before signing it.
/// Encoding: domain, length-prefixed key id and raw manifest, u64 file count,
/// then sorted (length-prefixed UTF-8 path, length-prefixed lowercase hex hash).
fn payload(pkg: &Package, key_id: &str) -> Result<Vec<u8>, SigningError> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&pkg.manifest_raw).map_err(|_| SigningError::InvalidContent)?;
    if manifest != pkg.manifest
        || manifest.get("id").and_then(serde_json::Value::as_str) != Some(pkg.id.as_str())
    {
        return Err(SigningError::InvalidContent);
    }
    let declared = manifest.get("files").and_then(serde_json::Value::as_object);
    if declared.map_or(0, |f| f.len()) != pkg.files.len() {
        return Err(SigningError::InvalidContent);
    }
    let mut out = DOMAIN.to_vec();
    field(&mut out, key_id.as_bytes());
    field(&mut out, &pkg.manifest_raw);
    out.extend_from_slice(&(pkg.files.len() as u64).to_be_bytes());
    for (path, contents) in &pkg.files {
        let hash = sha256_hex(contents);
        if !crate::addon_pkg::safe_rel(path)
            || path == "addon.json"
            || path == SIGNATURE_FILE
            || !declared
                .and_then(|f| f.get(path))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|h| h.eq_ignore_ascii_case(&hash))
        {
            return Err(SigningError::InvalidContent);
        }
        field(&mut out, path.as_bytes());
        field(&mut out, hash.as_bytes());
    }
    Ok(out)
}

/// Unsigned is accepted only when `require_signed` is false. An existing
/// signature is always verified, including in permissive mode. Returns signer id.
pub fn verify(
    pkg: &Package,
    trusted_keys: &TrustedKeys,
    require_signed: bool,
) -> Result<Option<String>, SigningError> {
    let Some(sig) = &pkg.signature else {
        return if require_signed {
            Err(SigningError::Required)
        } else {
            Ok(None)
        };
    };
    let bytes = sig.validate()?;
    let key = trusted_keys
        .get(&sig.key_id)
        .ok_or_else(|| SigningError::UnknownKey(sig.key_id.clone()))?;
    if key.revoked {
        return Err(SigningError::RevokedKey(sig.key_id.clone()));
    }
    let public_key = decode_public_key(&key.public_key)
        .map_err(|_| SigningError::InvalidPublicKey(sig.key_id.clone()))?;
    signature::UnparsedPublicKey::new(&signature::ED25519, public_key)
        .verify(&payload(pkg, &sig.key_id)?, &bytes)
        .map_err(|_| SigningError::InvalidSignature)?;
    Ok(Some(sig.key_id.clone()))
}

pub fn decode_public_key(encoded: &str) -> Result<Vec<u8>, SigningError> {
    if encoded.len() != 44 {
        return Err(SigningError::InvalidPublicKey(String::new()));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| SigningError::InvalidPublicKey(String::new()))?;
    if bytes.len() != 32 {
        return Err(SigningError::InvalidPublicKey(String::new()));
    }
    Ok(bytes)
}

pub fn sign(pkg: &Package, key_id: &str, pkcs8: &[u8]) -> Result<ReleaseSignature, SigningError> {
    if !key_id_ok(key_id) {
        return Err(SigningError::InvalidKeyId);
    }
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| SigningError::InvalidPrivateKey)?;
    let signed = pair.sign(&payload(pkg, key_id)?);
    Ok(ReleaseSignature {
        version: 1,
        algorithm: "Ed25519".into(),
        key_id: key_id.into(),
        signature: STANDARD.encode(signed.as_ref()),
    })
}

/// Returns binary PKCS#8 private material and a base64 public key. The caller
/// must store the private bytes securely and must never log them.
pub fn generate_keypair() -> Result<(Vec<u8>, String), SigningError> {
    let document =
        Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| SigningError::Random)?;
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref())
        .map_err(|_| SigningError::InvalidPrivateKey)?;
    Ok((
        document.as_ref().to_vec(),
        STANDARD.encode(pair.public_key().as_ref()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addon_pkg::{inspect, Limits};
    use std::io::{Cursor, Write};

    fn archive(manifest: &[u8], content: &[u8], sig: Option<&[u8]>, nested: bool) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut entries = vec![("addon.json", manifest), ("file.txt", content)];
        if let Some(s) = sig {
            entries.push((SIGNATURE_FILE, s));
        }
        for (name, bytes) in entries {
            zip.start_file(
                format!("{}{name}", if nested { "demo/" } else { "" }),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn trust_tamper_and_unsigned_policy() {
        let content = b"hello";
        let raw = serde_json::to_vec(&serde_json::json!({"id":"demo-addon", "version":"1", "files":{"file.txt":sha256_hex(content)}})).unwrap();
        let mut pkg = inspect(&archive(&raw, content, None, false), Limits::default()).unwrap();
        let (private, public_key) = generate_keypair().unwrap();
        let mut keys = TrustedKeys::from([(
            "release-1".into(),
            TrustedKey {
                public_key,
                revoked: false,
            },
        )]);
        assert_eq!(verify(&pkg, &keys, false), Ok(None));
        assert_eq!(verify(&pkg, &keys, true), Err(SigningError::Required));
        let sig = sign(&pkg, "release-1", &private).unwrap();
        assert_eq!(sign(&pkg, "release-1", &private).unwrap(), sig);
        let sig_raw = serde_json::to_vec(&sig).unwrap();
        pkg = inspect(
            &archive(&raw, content, Some(&sig_raw), true),
            Limits::default(),
        )
        .unwrap();
        assert!(!pkg.files.contains_key(SIGNATURE_FILE));
        let original_sig = pkg.signature.clone().unwrap();
        let mut corrupted = STANDARD.decode(&original_sig.signature).unwrap();
        corrupted[0] ^= 1;
        pkg.signature.as_mut().unwrap().signature = STANDARD.encode(corrupted);
        assert_eq!(
            verify(&pkg, &keys, false),
            Err(SigningError::InvalidSignature)
        );
        pkg.signature = Some(original_sig);
        assert_eq!(verify(&pkg, &keys, true), Ok(Some("release-1".into())));
        assert_eq!(
            verify(&pkg, &TrustedKeys::new(), false),
            Err(SigningError::UnknownKey("release-1".into()))
        );
        keys.get_mut("release-1").unwrap().revoked = true;
        assert_eq!(
            verify(&pkg, &keys, false),
            Err(SigningError::RevokedKey("release-1".into()))
        );
        keys.get_mut("release-1").unwrap().revoked = false;
        let (_, other_public) = generate_keypair().unwrap();
        let other_keys = TrustedKeys::from([(
            "release-1".into(),
            TrustedKey {
                public_key: other_public,
                revoked: false,
            },
        )]);
        assert_eq!(
            verify(&pkg, &other_keys, false),
            Err(SigningError::InvalidSignature)
        );
        // Whitespace is signed, not just the parsed JSON.
        let changed = [raw.clone(), b"\n".to_vec()].concat();
        let changed_pkg = inspect(
            &archive(&changed, content, Some(&sig_raw), false),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(
            verify(&changed_pkg, &keys, false),
            Err(SigningError::InvalidSignature)
        );
        // Rehashing tampered content cannot preserve the release signature.
        let changed_raw = serde_json::to_vec(&serde_json::json!({"id":"demo-addon", "version":"1", "files":{"file.txt":sha256_hex(b"evil")}})).unwrap();
        let changed_pkg = inspect(
            &archive(&changed_raw, b"evil", Some(&sig_raw), false),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(
            verify(&changed_pkg, &keys, false),
            Err(SigningError::InvalidSignature)
        );
        pkg.files.insert("file.txt".into(), b"evil".to_vec());
        assert_eq!(
            verify(&pkg, &keys, false),
            Err(SigningError::InvalidContent)
        );
    }

    #[test]
    fn malformed_and_oversized_signatures_are_never_unsigned() {
        let raw = serde_json::to_vec(
            &serde_json::json!({"id":"demo-addon", "files":{"file.txt":sha256_hex(b"x")}}),
        )
        .unwrap();
        for sig in [
            b"{}".to_vec(),
            b"null".to_vec(),
            vec![b' '; MAX_SIGNATURE_BYTES + 1],
        ] {
            assert!(inspect(&archive(&raw, b"x", Some(&sig), false), Limits::default()).is_err());
        }
        let (private, _) = generate_keypair().unwrap();
        let pkg = inspect(&archive(&raw, b"x", None, false), Limits::default()).unwrap();
        let mut sig = serde_json::to_value(sign(&pkg, "key", &private).unwrap()).unwrap();
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("algorithm", serde_json::json!("ed25519")),
            ("key_id", serde_json::json!("../bad")),
            ("signature", serde_json::json!("not-base64")),
        ] {
            let mut invalid = sig.clone();
            invalid[field] = value;
            assert!(ReleaseSignature::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        sig["public_key"] = "embedded key must not be accepted".into();
        assert!(ReleaseSignature::parse(&serde_json::to_vec(&sig).unwrap()).is_err());
        assert!(ReleaseSignature::parse(
            br#"{"version":1,"version":1,"algorithm":"Ed25519","key_id":"key","signature":""}"#
        )
        .is_err());
    }
}
