//! Argon2 password hashing and verification.
use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use pelty_core::{AppError, AppResult};
use std::sync::OnceLock;

fn hasher() -> Argon2<'static> {
    // OWASP: m=19 MiB, t=2, p=1.
    let params = Params::new(19 * 1024, 2, 1, None).unwrap_or_default();
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash(password: &str) -> AppResult<String> {
    hasher()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(AppError::internal)
}

pub fn verify(password: &str, encoded: &str) -> bool {
    if let Some(h) = encoded.strip_prefix(LEGACY_WHMCS) {
        return verify_whmcs(password, h);
    }
    match PasswordHash::new(encoded) {
        Ok(h) => hasher().verify_password(password.as_bytes(), &h).is_ok(),
        Err(_) => false,
    }
}

/// Prefix of a login hash carried over by the WHMCS importer.
pub const LEGACY_WHMCS: &str = "whmcs:";

/// True when `encoded` should be replaced by an Argon2id hash after the
/// next successful login (imported WHMCS hashes).
pub fn needs_rehash(encoded: &str) -> bool {
    encoded.starts_with(LEGACY_WHMCS)
}

/// WHMCS hashes: bcrypt (`$2y$`, PHP password_hash) or the pre-5.3 format
/// `md5(salt . password):salt`.
fn verify_whmcs(password: &str, h: &str) -> bool {
    if h.starts_with("$2") {
        return bcrypt::verify(password, h).unwrap_or(false);
    }
    let Some((md5_hex, salt)) = h.split_once(':') else {
        return false;
    };
    let got: String = md5::compute(format!("{salt}{password}"))
        .0
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    use subtle::ConstantTimeEq;
    got.as_bytes()
        .ct_eq(md5_hex.to_ascii_lowercase().as_bytes())
        .into()
}

/// Burn the same CPU as a real verification when the user does not exist.
pub fn dummy_verify(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let h = DUMMY.get_or_init(|| hash("dummy-password-for-timing").unwrap_or_default());
    let _ = verify(password, h);
}

/// Password policy: 12..=128 chars, not all one character class repeated, not trivially weak.
pub fn check_policy(password: &str, email: &str) -> AppResult<()> {
    check_policy_min(password, email, 12)
}

/// Same as [`check_policy`] with a configurable minimum length (8..=128).
pub fn check_policy_min(password: &str, email: &str, min: usize) -> AppResult<()> {
    let n = password.chars().count();
    if n < min.clamp(8, 128) {
        return Err(AppError::Validation("error.password_too_short"));
    }
    if n > 128 {
        return Err(AppError::Validation("error.password_too_long"));
    }
    let lower = password.to_lowercase();
    let local = email.split('@').next().unwrap_or("").to_lowercase();
    let distinct = {
        let mut c: Vec<char> = password.chars().collect();
        c.sort_unstable();
        c.dedup();
        c.len()
    };
    if is_breached(&lower) {
        return Err(AppError::Validation("error.password_breached"));
    }
    if distinct < 5 || (local.len() >= 4 && lower.contains(&local)) || lower.contains("password") {
        return Err(AppError::Validation("error.password_weak"));
    }
    Ok(())
}

const BREACHED: &str = include_str!("breached.txt");

/// Offline check against the bundled list: exact match, or a short common
/// base word followed only by digits/symbols ("Summer2024!").
pub fn is_breached(lower: &str) -> bool {
    let base = lower.trim_end_matches(|c: char| c.is_ascii_digit() || c.is_ascii_punctuation());
    BREACHED
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .any(|w| w == lower || (w.len() < 12 && w == base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whmcs_hashes_verify_and_need_rehash() {
        // PHP password_hash() writes $2y$; same algorithm as $2b$.
        let b = bcrypt::hash("Client-Pass-1", 4)
            .unwrap()
            .replacen("$2b$", "$2y$", 1);
        let enc = format!("{LEGACY_WHMCS}{b}");
        assert!(verify("Client-Pass-1", &enc) && !verify("client-pass-1", &enc));
        assert!(needs_rehash(&enc) && !needs_rehash(&hash("x-long-password").unwrap()));
        // Legacy WHMCS: md5(salt . password):salt.
        let md5: String = md5::compute("ab#1Csecret")
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let old = format!("{LEGACY_WHMCS}{md5}:ab#1C");
        assert!(verify("secret", &old) && !verify("Secret", &old));
        assert!(!verify("x", "whmcs:") && !verify("x", "!whmcs-import:password-reset-required"));
    }
    #[test]
    fn breached_list() {
        assert!(is_breached("qwertyuiop123"));
        assert!(is_breached("football2024!!"));
        assert!(!is_breached("violet-kettle-orbit-42"));
        assert!(matches!(
            check_policy("Football2024!!", "a@b.c"),
            Err(AppError::Validation("error.password_breached"))
        ));
        assert!(check_policy("violet-kettle-orbit-42", "a@b.c").is_ok());
    }
    #[test]
    fn roundtrip() {
        let h = hash("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correct horse battery", &h));
        assert!(!verify("wrong", &h));
        assert!(!verify("x", "garbage"));
    }
    #[test]
    fn policy() {
        assert!(check_policy("short", "a@b.c").is_err());
        assert!(check_policy("aaaaaaaaaaaaaaa", "a@b.c").is_err());
        assert!(check_policy("mypassword123456", "a@b.c").is_err());
        assert!(check_policy("johnsmith-2026!", "johnsmith@x.io").is_err());
        assert!(check_policy("Tr0ub4dor&3-horse", "a@b.c").is_ok());
    }
}
