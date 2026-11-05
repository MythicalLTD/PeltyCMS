//! WHMCS secrets.
//!
//! * Server passwords (`tblservers.password`, also hosting passwords) are
//!   encrypted with WHMCS' legacy `encrypt()` keyed by `$cc_encryption_hash`
//!   from WHMCS' `configuration.php`: a SHA-1 keystream (20-byte blocks,
//!   re-keyed from the previous plaintext block) behind a random 20-byte IV,
//!   base64-encoded. [`decrypt`] reverses it; the key never leaves memory.
//! * Card data is never read: no `tblclients.cardnum` / `tblcreditcards` /
//!   `tblpaymethods` query exists in this crate.
//! * Client login hashes are carried over as `whmcs:<hash>` (see
//!   [`login_hash`]); `pelty_auth::password::verify` checks them (bcrypt, or
//!   the old `md5(salt . password):salt`) and the login rehashes to Argon2id.
use sha1::{Digest, Sha1};

const BLOCK: usize = 20;

fn md5_hex(b: &[u8]) -> String {
    md5::compute(b).0.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha1(b: &[u8]) -> [u8; BLOCK] {
    Sha1::digest(b).into()
}

fn hash_key(cc: &str) -> [u8; BLOCK] {
    let m = md5_hex(cc.as_bytes());
    let key = format!("{}{}", md5_hex(m.as_bytes()), m);
    sha1(key.as_bytes())
}

/// Decrypt a value written by WHMCS `encrypt()`. None when it is not valid
/// base64 or too short (wrong field), a wrong key yields garbage, which the
/// caller detects as non-UTF-8 / non-printable.
pub fn decrypt(cc_encryption_hash: &str, value: &str) -> Option<String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(value.trim()).ok()?;
    if raw.len() < BLOCK {
        return None;
    }
    let hk = hash_key(cc_encryption_hash);
    let (tmp_iv, body) = raw.split_at(BLOCK);
    let mut key: Vec<u8> = tmp_iv.iter().zip(hk.iter()).map(|(a, b)| a ^ b).collect();
    let mut out: Vec<u8> = Vec::with_capacity(body.len());
    for (c, x) in body.iter().enumerate() {
        if c != 0 && c % BLOCK == 0 {
            let mut k = key.clone();
            k.extend_from_slice(&out[c - BLOCK..c]);
            key = sha1(&k).to_vec();
        }
        out.push(key[c % BLOCK] ^ x);
    }
    let s = String::from_utf8(out).ok()?;
    s.chars().all(|c| !c.is_control() || c == '\t').then_some(s)
}

/// WHMCS `encrypt()` (tests and fixtures; `iv` is normally random).
pub fn encrypt(cc_encryption_hash: &str, plain: &str, iv: [u8; BLOCK]) -> String {
    use base64::Engine;
    let hk = hash_key(cc_encryption_hash);
    let mut out: Vec<u8> = iv.iter().zip(hk.iter()).map(|(a, b)| a ^ b).collect();
    let p = plain.as_bytes();
    let mut key = iv.to_vec();
    for (c, x) in p.iter().enumerate() {
        if c != 0 && c % BLOCK == 0 {
            let mut k = key.clone();
            k.extend_from_slice(&p[c - BLOCK..c]);
            key = sha1(&k).to_vec();
        }
        out.push(key[c % BLOCK] ^ x);
    }
    base64::engine::general_purpose::STANDARD.encode(out)
}

/// The `users.password_hash` value for a WHMCS login hash: `whmcs:<hash>`
/// for bcrypt (`$2y$`/`$2a$`/`$2b$`) and legacy `md5hex:salt` hashes, None
/// (forced reset) for anything else, including an empty password.
pub fn login_hash(h: &str) -> Option<String> {
    let h = h.trim();
    let bcrypt = h.len() == 60 && ["$2y$", "$2a$", "$2b$"].iter().any(|p| h.starts_with(p));
    let legacy = h.split_once(':').is_some_and(|(m, salt)| m.len() == 32 && m.bytes().all(|b| b.is_ascii_hexdigit()) && !salt.is_empty() && salt.len() <= 64);
    (bcrypt || legacy).then(|| format!("whmcs:{h}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cipher_round_trips_and_wrong_keys_fail() {
        let cc = "s3cr3tCcHashFromConfigurationPhp";
        for pw in ["", "x", "a much longer server password spanning several 20-byte blocks!"] {
            let enc = encrypt(cc, pw, [7; BLOCK]);
            assert_eq!(decrypt(cc, &enc).as_deref(), Some(pw));
        }
        let enc = encrypt(cc, "rootpass-123", [42; BLOCK]);
        assert_ne!(decrypt("other", &enc).as_deref(), Some("rootpass-123"));
        assert_eq!(decrypt(cc, "not base64!"), None);
    }

    #[test]
    fn login_hashes() {
        let b = "$2y$10$abcdefghijklmnopqrstuuMRK7cL0nL3oQwXzu3m1aW0eP4yS6J2y";
        assert_eq!(b.len(), 60);
        assert_eq!(login_hash(b).as_deref(), Some(format!("whmcs:{b}").as_str()));
        assert!(login_hash("5f4dcc3b5aa765d61d8327deb882cf99:abcde").is_some());
        assert_eq!(login_hash(""), None);
        assert_eq!(login_hash("plaintext"), None);
    }
}
