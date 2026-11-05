//! Generic hosted crypto checkout, Coinbase Commerce-style: `POST /charges`
//! (`X-CC-Api-Key`) returns `data.hosted_url`; webhooks are signed with
//! `X-CC-Webhook-Signature` = hex(HMAC-SHA256(body, shared_secret)).
//! Crypto payments are not refundable through the API. Mock-tested only.
use pelty_payments::http::{api_base, ct_eq, hmac_sha256, send_json};
use pelty_payments::*;
#[allow(unused_imports)]
use {
    pelty_core::money::Money,
    serde::{Deserialize, Serialize},
    std::collections::{BTreeMap, HashMap},
    std::future::Future,
    std::pin::Pin,
    std::sync::Arc,
};

pub const API_BASE: &str = "https://api.commerce.coinbase.com";

pub struct CryptoGateway {
    key: String,
    secret: String,
    base: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[SettingField::of("api_base", "url")];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "crypto",
        name: "Crypto (Coinbase Commerce-style)",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: false,
            recurring: false,
        },
        secrets: &["api_key", "webhook_secret"],
        settings: SETTINGS,
        currencies: &[],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    Ok(Box::new(CryptoGateway {
        key: cfg.secret("api_key")?.into(),
        secret: cfg.secret("webhook_secret")?.into(),
        base: api_base(&cfg.config, API_BASE)?,
        t: cfg.transport.clone(),
    }))
}

impl PaymentGateway for CryptoGateway {
    fn id(&self) -> &'static str {
        "crypto"
    }
    fn display_name(&self) -> &'static str {
        "Crypto"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(
        &'a self,
        r: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let body = serde_json::json!({
                "name": r.reference,
                "description": r.description,
                "pricing_type": "fixed_price",
                "local_price": {"amount": decimal_str(r.amount)?, "currency": r.amount.currency.as_str()},
                "metadata": {"pelty_payment_id": r.payment_id},
                "redirect_url": r.success_url,
                "cancel_url": r.cancel_url,
            });
            let spec = HttpRequestSpec {
                method: "POST",
                url: format!("{}/charges", self.base),
                headers: vec![
                    ("X-CC-Api-Key".into(), self.key.clone()),
                    ("X-CC-Version".into(), "2018-03-22".into()),
                    ("Content-Type".into(), "application/json".into()),
                ],
                body: body.to_string(),
            };
            let v = send_json(self.t.as_ref(), spec).await?;
            match (obj_str(&v, "data.code"), obj_str(&v, "data.hosted_url")) {
                (Some(code), Some(url)) => Ok(PaymentInit::Redirect {
                    url,
                    provider_ref: code,
                }),
                _ => Err(GatewayError::Provider(
                    "missing data.code/hosted_url".into(),
                )),
            }
        })
    }
    fn parse_webhook(
        &self,
        h: &Headers,
        body: &[u8],
        _now: i64,
    ) -> Result<WebhookEvent, GatewayError> {
        let sig = header(h, "x-cc-webhook-signature").ok_or(GatewayError::InvalidSignature)?;
        let want = hmac_sha256(self.secret.as_bytes(), body);
        if !hex::decode(sig.trim()).is_ok_and(|b| ct_eq(&b, &want)) {
            return Err(GatewayError::InvalidSignature);
        }
        let v: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let ev = &v["event"];
        let id = obj_str(ev, "id").ok_or(GatewayError::MalformedPayload)?;
        let ty = obj_str(ev, "type").ok_or(GatewayError::MalformedPayload)?;
        let kind = match ty.as_str() {
            "charge:confirmed" | "charge:resolved" => WebhookKind::PaymentSucceeded,
            "charge:failed" => WebhookKind::PaymentFailed,
            _ => WebhookKind::Ignored,
        };
        let mut e = WebhookEvent::new(id, ty, kind, obj_str(ev, "data.code"));
        e.payment_id = obj_str(ev, "data.metadata.pelty_payment_id");
        Ok(e)
    }
    fn refund<'a>(
        &'a self,
        _: &'a str,
        _: Money,
        _: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async { Err(GatewayError::Unsupported) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_payments::test_util::*;

    #[tokio::test]
    async fn charge_and_signed_webhook() {
        let t = MockTransport::new();
        let gw = factory(&cfg(
            "crypto",
            serde_json::json!({}),
            &[("api_key", "k"), ("webhook_secret", "whs")],
            &t,
        ))
        .unwrap();
        t.push(201, r#"{"data":{"code":"ABCD1234","hosted_url":"https://commerce.coinbase.com/charges/ABCD1234"}}"#);
        let init = gw.create_payment(&req("25", "USD")).await.unwrap();
        assert_eq!(
            init,
            PaymentInit::Redirect {
                url: "https://commerce.coinbase.com/charges/ABCD1234".into(),
                provider_ref: "ABCD1234".into()
            }
        );
        let v: serde_json::Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(v["local_price"]["amount"], "25.00");

        let body = r#"{"event":{"id":"ev1","type":"charge:confirmed","data":{"code":"ABCD1234","metadata":{"pelty_payment_id":"p1"}}}}"#;
        let sig = "c78e5c9730735f7e9d7bb2287b5f6abcee80e882d2e35a26c576d9d2e52730df";
        let e = gw
            .parse_webhook(
                &hdrs(&[("x-cc-webhook-signature", sig)]),
                body.as_bytes(),
                0,
            )
            .unwrap();
        assert_eq!(
            (e.kind, e.provider_ref.as_deref()),
            (WebhookKind::PaymentSucceeded, Some("ABCD1234"))
        );
        assert!(gw
            .parse_webhook(
                &hdrs(&[("x-cc-webhook-signature", "00")]),
                body.as_bytes(),
                0
            )
            .is_err());
        assert_eq!(
            gw.refund("x", money("1", "USD"), "k").await.unwrap_err(),
            GatewayError::Unsupported
        );
    }
}
