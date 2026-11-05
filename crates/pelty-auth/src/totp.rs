//! RFC 6238 TOTP (SHA-1, 6 digits, 30 s) plus backup codes.
use hmac::{Hmac, Mac};
use rand::{Rng, RngCore};
use sha1::Sha1;

pub const STEP: i64 = 30;
pub const DIGITS: u32 = 6;
/// Accept codes from one step before/after (clock drift).
pub const SKEW: i64 = 1;
pub const BACKUP_CODES: usize = 10;

pub fn new_secret() -> Vec<u8> {
    let mut s = vec![0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut s);
    s
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in data {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = (h[19] & 0x0f) as usize;
    let bin = (u32::from(h[off]) & 0x7f) << 24
        | u32::from(h[off + 1]) << 16
        | u32::from(h[off + 2]) << 8
        | u32::from(h[off + 3]);
    bin % 10u32.pow(DIGITS)
}

pub fn step_at(unix: i64) -> i64 {
    unix.div_euclid(STEP)
}

pub fn code_at(secret: &[u8], unix: i64) -> String {
    format!(
        "{:0width$}",
        hotp(secret, step_at(unix) as u64),
        width = DIGITS as usize
    )
}

/// Returns the matched time step if `code` is valid at `unix` (±SKEW) and
/// newer than `last_step` (replay protection).
pub fn verify(secret: &[u8], code: &str, unix: i64, last_step: Option<i64>) -> Option<i64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let now = step_at(unix);
    (now - SKEW..=now + SKEW).find(|&st| {
        last_step.is_none_or(|l| st > l)
            && crate::token::constant_eq(
                &format!(
                    "{:0width$}",
                    hotp(secret, st as u64),
                    width = DIGITS as usize
                ),
                &code,
            )
    })
}

pub fn otpauth_url(issuer: &str, account: &str, secret: &[u8]) -> String {
    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~@".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP}",
        enc(issuer),
        enc(account),
        base32(secret),
        enc(issuer)
    )
}

/// `xxxxx-xxxxx` from an unambiguous alphabet (~50 bits).
pub fn backup_code() -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut r = rand::rngs::OsRng;
    let mut s: String = (0..10)
        .map(|_| A[r.gen_range(0..A.len())] as char)
        .collect();
    s.insert(5, '-');
    s
}

/// Canonical form for hashing: lowercase, no separators/spaces.
pub fn normalize_backup(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    // RFC 6238 Appendix B, SHA-1 seed.
    const SEED: &[u8] = b"12345678901234567890";
    #[test]
    fn rfc6238_vectors() {
        for (t, want) in [
            (59, 94287082u32),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
        ] {
            assert_eq!(hotp(SEED, step_at(t) as u64), want % 1_000_000, "t={t}");
        }
    }
    #[test]
    fn verify_skew_and_replay() {
        let t = 1_700_000_000;
        let c = code_at(SEED, t);
        let st = verify(SEED, &c, t, None).unwrap();
        assert_eq!(st, step_at(t));
        assert!(
            verify(SEED, &c, t + 30, None).is_some(),
            "previous step accepted"
        );
        assert!(verify(SEED, &c, t + 90, None).is_none(), "too old");
        assert!(verify(SEED, &c, t, Some(st)).is_none(), "replay rejected");
        assert!(verify(SEED, "12345", t, None).is_none());
        assert!(verify(SEED, "abcdef", t, None).is_none());
        assert_eq!(
            verify(SEED, &format!("{} {}", &c[..3], &c[3..]), t, None),
            Some(st)
        );
    }
    #[test]
    fn base32_and_url() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(SEED), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        let u = otpauth_url("Pelty CMS", "ana@x.ro", SEED);
        assert!(u.starts_with("otpauth://totp/Pelty%20CMS:ana@x.ro?secret=GEZD"));
    }
    #[test]
    fn backup_codes() {
        let c = backup_code();
        assert_eq!(c.len(), 11);
        assert_eq!(normalize_backup(&c.to_uppercase()).len(), 10);
        assert_ne!(c, backup_code());
    }
}
