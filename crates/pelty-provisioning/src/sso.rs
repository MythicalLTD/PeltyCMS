//! Client single sign-on into a provisioned panel.
//!
//! PeltyCMS issues a short-lived token signed with HMAC-SHA256 under the
//! server's API key (a secret both sides already share). The panel checks the
//! signature and expiry offline, then calls `POST /api/v1/sso/consume` once to
//! burn the nonce, so a token can never be replayed.
use crate::{ServerConn, ServiceCtx};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub sid: String,
    pub rid: Option<String>,
    pub email: String,
    /// Unix seconds.
    pub exp: i64,
    pub nonce: String,
}

fn mac(key: &str) -> HmacSha256 {
    // HMAC accepts keys of any length.
    HmacSha256::new_from_slice(key.as_bytes()).unwrap_or_else(|_| unreachable!())
}

/// `hex(json(claims)).hex(hmac)`.
pub fn sign(key: &str, c: &Claims) -> String {
    let body = hex::encode(serde_json::to_vec(c).unwrap_or_default());
    let mut m = mac(key);
    m.update(body.as_bytes());
    format!("{body}.{}", hex::encode(m.finalize().into_bytes()))
}

/// Claims without checking the signature (to find which server key to use).
pub fn peek(token: &str) -> Option<Claims> {
    let (body, _) = token.split_once('.')?;
    serde_json::from_slice(&hex::decode(body).ok()?).ok()
}

/// Verified, unexpired claims.
pub fn verify(key: &str, token: &str, now: i64) -> Option<Claims> {
    let (body, sig) = token.split_once('.')?;
    let mut m = mac(key);
    m.update(body.as_bytes());
    m.verify_slice(&hex::decode(sig).ok()?).ok()?;
    let c: Claims = serde_json::from_slice(&hex::decode(body).ok()?).ok()?;
    (c.exp >= now).then_some(c)
}

/// Default redirect: the server's `sso_url` setting, a template with
/// `{base_url}`, `{remote_id}` and `{token}`. None when not configured.
pub fn template_url(server: &ServerConn, svc: &ServiceCtx, token: &str) -> Option<String> {
    let t = server.config.get("sso_url")?.as_str()?.trim();
    if t.is_empty() || !t.contains("{token}") {
        return None;
    }
    let url = t
        .replace("{base_url}", server.base_url.trim_end_matches('/'))
        .replace("{remote_id}", svc.remote_id.as_deref().unwrap_or(""))
        .replace("{token}", token);
    (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claims(exp: i64) -> Claims {
        Claims { sid: "s".into(), rid: Some("42".into()), email: "a@b.c".into(), exp, nonce: "n1".into() }
    }

    #[test]
    fn sign_and_verify() {
        let t = sign("k", &claims(100));
        assert_eq!(verify("k", &t, 50), Some(claims(100)));
        assert_eq!(verify("other", &t, 50), None);
        assert_eq!(verify("k", &t, 101), None);
        assert_eq!(peek(&t).unwrap().nonce, "n1");
        let (b, s) = t.split_once('.').unwrap();
        let forged = sign("k", &Claims { email: "x@y.z".into(), ..claims(100) });
        let (fb, _) = forged.split_once('.').unwrap();
        assert_eq!(verify("k", &format!("{fb}.{s}"), 50), None);
        assert_eq!(verify("k", b, 50), None);
    }

    #[test]
    fn url_template() {
        let svc = ServiceCtx { service_id: "s".into(), customer_email: "a@b.c".into(), customer_first_name: String::new(), customer_last_name: String::new(), product_config: json!({}), custom_fields: json!({}), remote_id: Some("42".into()), module_data: json!({}) };
        let mut srv = ServerConn { base_url: "https://panel.test/".into(), api_key: "k".into(), config: json!({}) };
        assert_eq!(template_url(&srv, &svc, "T"), None);
        srv.config = json!({ "sso_url": "{base_url}/sso/{remote_id}?token={token}" });
        assert_eq!(template_url(&srv, &svc, "T").as_deref(), Some("https://panel.test/sso/42?token=T"));
        srv.config = json!({ "sso_url": "javascript:{token}" });
        assert_eq!(template_url(&srv, &svc, "T"), None);
    }
}
