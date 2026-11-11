//! Game-store sign-in that is not OAuth 2.0: FiveM / RedM players through
//! their Cfx.re forum account (Discourse User API keys, `sso-cfx`) and Steam
//! (OpenID 2.0, `sso-steam`). Account linking, sessions, MFA and the
//! register/link rules are shared with OAuth in [`crate::oauth`]; this
//! module only owns the two protocols.
//!
//! Neither is Rockstar SSO: Rockstar's sign-in is first-party only, and the
//! Cfx.re IDMS that Tebex and txAdmin use is private. The forum key flow is
//! the public path Cfx.re staff recommend, and it identifies the forum
//! account, not a Rockstar licence (see `make docs`, admin security).
//!
//! Security: every attempt has a one-time state row (10 minutes) bound to
//! the browser by a short-lived HttpOnly cookie, so a callback link sent to
//! someone else does nothing. Cfx: the payload must decrypt with this
//! store's private key and carry the attempt's nonce; the key is revoked
//! right after reading the profile. Steam: `openid.return_to` must be
//! exactly the URL this attempt issued, and Steam itself must confirm the
//! signature (`check_authentication`).
use crate::AppState;
use aws_lc_rs::encoding::AsDer;
use aws_lc_rs::rsa::{KeySize, Pkcs1PrivateDecryptingKey, PrivateDecryptingKey};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pelty_core::{AppError, AppResult};
use serde_json::{json, Value};

/// Addon `protocol` of the Cfx.re forum (Discourse User API keys).
pub const DISCOURSE: &str = "discourse_user_api";
/// Addon `protocol` of Steam (OpenID 2.0).
pub const OPENID2: &str = "openid2";

/// Short-lived cookie binding a sign-in attempt to the browser that started it.
pub const BIND_COOKIE: &str = "pelty_sso_bind";

const OPENID_NS: &str = "http://specs.openid.net/auth/2.0";
const IDENTIFIER_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";
/// addon kv key of the forum key pair.
const KEYS_KV: &str = "user_api_rsa";

pub fn is_game(protocol: &str) -> bool {
    protocol == DISCOURSE || protocol == OPENID2
}

/// A same-site path to land on after sign-in, or `None`. Rejects absolute
/// and protocol-relative URLs (no open redirect).
pub fn safe_return_to(v: Option<&str>) -> Option<String> {
    let v = v?.trim();
    let ok = v.starts_with('/')
        && !v.starts_with("//")
        && !v.starts_with("/\\")
        && v.len() <= 512
        && !v.contains("://")
        && !v.chars().any(|c| c.is_control() || c == '\\');
    ok.then(|| v.to_string())
}

/// Stored key pair of a forum addon.
struct Keys {
    client_id: String,
    private: PrivateDecryptingKey,
}

fn pem(der: &[u8]) -> String {
    let b = STANDARD.encode(der);
    let mut out = String::from("-----BEGIN PUBLIC KEY-----\n");
    for chunk in b.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push('\n');
    }
    out.push_str("-----END PUBLIC KEY-----\n");
    out
}

fn parse_keys(raw: &str) -> Option<Keys> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let der = STANDARD.decode(v["pkcs8"].as_str()?).ok()?;
    Some(Keys {
        client_id: v["client_id"].as_str()?.to_string(),
        private: PrivateDecryptingKey::from_pkcs8(&der).ok()?,
    })
}

/// The forum client id and RSA key pair of addon `id`, created on first
/// use and kept in the addon's kv store (sealed with the app key). The
/// private key never leaves the host.
async fn keys(s: &AppState, id: &str) -> AppResult<Keys> {
    static GEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    if let Some(k) = s
        .addon_kv
        .get(id, KEYS_KV)
        .await
        .as_deref()
        .and_then(parse_keys)
    {
        return Ok(k);
    }
    let _g = GEN.lock().await;
    if let Some(k) = s
        .addon_kv
        .get(id, KEYS_KV)
        .await
        .as_deref()
        .and_then(parse_keys)
    {
        return Ok(k);
    }
    let private = tokio::task::spawn_blocking(|| PrivateDecryptingKey::generate(KeySize::Rsa2048))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|_| AppError::Internal("rsa keygen".into()))?;
    let der = private
        .as_der()
        .map_err(|_| AppError::Internal("rsa export".into()))?;
    let client_id = format!("pelty-{}", pelty_auth::token::generate());
    let raw = json!({ "client_id": client_id, "pkcs8": STANDARD.encode(der.as_ref()) }).to_string();
    s.addon_kv
        .set(id, KEYS_KV, &raw)
        .await
        .map_err(AppError::Internal)?;
    Ok(Keys { client_id, private })
}

fn public_pem(k: &Keys) -> AppResult<String> {
    let der = k
        .private
        .public_key()
        .as_der()
        .map_err(|_| AppError::Internal("rsa export".into()))?;
    Ok(pem(der.as_ref()))
}

/// Inputs of a sign-in start URL.
pub struct StartInput<'a> {
    pub addon_id: &'a str,
    pub authorize: &'a str,
    pub cfg: &'a Value,
    /// Callback URL including `?state=…`.
    pub callback: &'a str,
    pub nonce: &'a str,
}

fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    let mut q = form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        q.append_pair(k, v);
    }
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}{}", q.finish())
}

/// Where to send the browser for a Cfx.re forum key.
pub async fn cfx_url(s: &AppState, i: &StartInput<'_>) -> AppResult<String> {
    let k = keys(s, i.addon_id).await?;
    let app = match i.cfg["application_name"].as_str().map(str::trim) {
        Some(v) if !v.is_empty() => v.chars().take(80).collect(),
        _ => crate::routes::site::load_config(s)
            .await
            .map(|c| c.site_name)
            .ok()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "PeltyCMS store".into()),
    };
    Ok(with_query(
        i.authorize,
        &[
            ("application_name", &app),
            ("client_id", &k.client_id),
            ("scopes", "session_info"),
            ("public_key", &public_pem(&k)?),
            ("nonce", i.nonce),
            ("auth_redirect", i.callback),
        ],
    ))
}

/// Where to send the browser for Steam OpenID 2.0.
pub fn steam_url(s: &AppState, i: &StartInput<'_>) -> String {
    with_query(
        i.authorize,
        &[
            ("openid.ns", OPENID_NS),
            ("openid.mode", "checkid_setup"),
            ("openid.return_to", i.callback),
            ("openid.realm", &realm(s)),
            ("openid.identity", IDENTIFIER_SELECT),
            ("openid.claimed_id", IDENTIFIER_SELECT),
        ],
    )
}

fn realm(s: &AppState) -> String {
    let u = s.cfg.public_url.trim_end_matches('/');
    format!("{u}/")
}

/// Decrypt a forum `payload` (base64, RSA PKCS#1 v1.5) and return the user
/// API key when its nonce is `nonce`.
async fn cfx_key(
    s: &AppState,
    id: &str,
    payload: &str,
    nonce: &str,
) -> AppResult<(String, String)> {
    let bad = || AppError::Validation("error.oauth_payload");
    let clean: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    let ct = STANDARD.decode(clean.as_bytes()).map_err(|_| bad())?;
    let k = keys(s, id).await?;
    let dec = Pkcs1PrivateDecryptingKey::new(k.private.clone()).map_err(|_| bad())?;
    if ct.len() != dec.key_size_bytes() {
        return Err(bad());
    }
    let mut out = vec![0u8; dec.min_output_size()];
    let plain = dec.decrypt(&ct, &mut out).map_err(|_| bad())?;
    let v: Value = serde_json::from_slice(plain).map_err(|_| bad())?;
    let got = v["nonce"].as_str().unwrap_or("");
    let same: bool = got.len() == nonce.len()
        && got
            .bytes()
            .zip(nonce.bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            == 0;
    if nonce.is_empty() || !same {
        return Err(AppError::Validation("error.oauth_state"));
    }
    let key = v["key"]
        .as_str()
        .filter(|k| !k.is_empty())
        .ok_or_else(bad)?;
    Ok((key.to_string(), k.client_id))
}

/// Verified profile of a callback. `query` is the whole callback query;
/// `callback` is the exact URL (with `?state=…`) this attempt issued.
#[allow(clippy::too_many_arguments)]
pub async fn profile(
    s: &AppState,
    protocol: &str,
    addon_id: &str,
    addon: &std::sync::Arc<pelty_wasm::WasmAddon>,
    cfg: &Value,
    query: &[(String, String)],
    callback: &str,
    nonce: &str,
) -> AppResult<super::oauth::Profile> {
    let fail = |e: pelty_wasm::WasmError| {
        tracing::warn!(error = %e, provider = %addon_id, "game sign-in failed");
        AppError::Validation("error.oauth_failed")
    };
    let get = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let ctx = pelty_wasm::CallCtx {
        settings: cfg.clone(),
        ..Default::default()
    };
    let v = match protocol {
        DISCOURSE => {
            let payload = get("payload").ok_or(AppError::Validation("error.oauth_cancelled"))?;
            let (key, client_id) = cfx_key(s, addon_id, payload, nonce).await?;
            addon
                .call_async(
                    ctx,
                    "profile".into(),
                    json!({ "key": key, "client_id": client_id }),
                )
                .await
                .map_err(fail)?
        }
        OPENID2 => {
            match get("openid.mode") {
                Some("id_res") => {}
                Some("cancel") | None => return Err(AppError::Validation("error.oauth_cancelled")),
                Some(_) => return Err(AppError::Validation("error.oauth_failed")),
            }
            if get("openid.return_to") != Some(callback) {
                return Err(AppError::Validation("error.oauth_state"));
            }
            let params: serde_json::Map<String, Value> = query
                .iter()
                .filter(|(k, _)| k.starts_with("openid."))
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            addon
                .call_async(
                    ctx,
                    "verify".into(),
                    json!({ "params": params, "settings": cfg }),
                )
                .await
                .map_err(fail)?
        }
        _ => return Err(AppError::NotFound),
    };
    let g = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let prof = super::oauth::Profile {
        subject: g("subject"),
        email: g("email"),
        email_verified: false,
        name: g("name"),
        username: g("username"),
    };
    if prof.subject.is_empty() {
        return Err(AppError::Validation("error.oauth_failed"));
    }
    Ok(prof)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_is_same_site_only() {
        assert_eq!(
            safe_return_to(Some("/checkout")).as_deref(),
            Some("/checkout")
        );
        assert_eq!(
            safe_return_to(Some("/store/x?a=1")).as_deref(),
            Some("/store/x?a=1")
        );
        for bad in [
            "https://evil.test/",
            "//evil.test",
            "/\\evil.test",
            "javascript:alert(1)",
            "/x\nSet-Cookie: a",
            "checkout",
            "/redirect?u=https://evil.test",
        ] {
            assert!(safe_return_to(Some(bad)).is_none(), "{bad}");
        }
        assert!(safe_return_to(None).is_none());
    }

    #[test]
    fn pem_wraps_at_64() {
        let p = pem(&[7u8; 100]);
        assert!(p.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(p.lines().all(|l| l.len() <= 64));
    }
}
