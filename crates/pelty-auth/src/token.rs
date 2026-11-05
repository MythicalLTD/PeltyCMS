//! Random tokens and their SHA-256 hashes for sessions and links.
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 256-bit random token, URL-safe base64.
pub fn generate() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

/// SHA-256 of the token; only this is stored.
pub fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub fn constant_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens() {
        let t = super::generate();
        assert_eq!(t.len(), 43);
        assert_ne!(t, super::generate());
        assert_eq!(super::hash(&t), super::hash(&t));
        assert!(super::constant_eq("abc", "abc"));
        assert!(!super::constant_eq("abc", "abd"));
    }
}
