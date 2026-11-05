//! Application-level encryption for small secrets at rest (TOTP seeds).
//! AES-256-GCM with a random 96-bit nonce; key from `PELTY_ENCRYPTION_KEY`
//! (base64, 32 bytes). There is no built-in default key.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand::RngCore;

#[derive(Clone)]
pub struct SecretBox {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for SecretBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBox(<key>)")
    }
}

impl SecretBox {
    pub fn from_base64(key: &str) -> Result<Self, String> {
        let k = STANDARD
            .decode(key.trim())
            .map_err(|_| "PELTY_ENCRYPTION_KEY is not valid base64".to_string())?;
        if k.len() != 32 {
            return Err("PELTY_ENCRYPTION_KEY must decode to 32 bytes".into());
        }
        Ok(SecretBox {
            cipher: Aes256Gcm::new_from_slice(&k).map_err(|e| e.to_string())?,
        })
    }

    /// nonce || ciphertext+tag
    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        let mut n = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut n);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&n), plain)
            .expect("AES-GCM encryption cannot fail for small inputs");
        let mut out = n.to_vec();
        out.extend(ct);
        out
    }

    pub fn open(&self, sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < 12 + 16 {
            return None;
        }
        let (n, ct) = sealed.split_at(12);
        self.cipher.decrypt(Nonce::from_slice(n), ct).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_tamper() {
        let key = STANDARD.encode([7u8; 32]);
        let b = SecretBox::from_base64(&key).unwrap();
        let s = b.seal(b"seed");
        assert_ne!(b.seal(b"seed"), s, "random nonce");
        assert_eq!(b.open(&s).unwrap(), b"seed");
        let mut t = s.clone();
        *t.last_mut().unwrap() ^= 1;
        assert!(b.open(&t).is_none());
        let other = SecretBox::from_base64(&STANDARD.encode([8u8; 32])).unwrap();
        assert!(other.open(&s).is_none());
        assert!(SecretBox::from_base64("short").is_err());
        assert!(format!("{b:?}").contains("<key>"));
    }
}
