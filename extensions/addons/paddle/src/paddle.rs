//! Paddle Billing-style gateway: non-catalog transactions with a hosted
//! checkout URL; webhooks signed with `Paddle-Signature: ts=…;h1=…` where
//! h1 = hex(HMAC-SHA256("{ts}:{body}", secret)). Refunds are adjustments.
//! Mock-tested only.
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

pub const API_BASE: &str = "https://api.paddle.com";
pub const SANDBOX_BASE: &str = "https://sandbox-api.paddle.com";
pub const TOLERANCE: i64 = 300;

pub struct PaddleGateway {
    key: String,
    secret: String,
    base: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[
    SettingField::of("live", "bool"),
    SettingField::of("api_base", "url"),
];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "paddle",
        name: "Paddle",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: true,
            recurring: false,
        },
        secrets: &["api_key", "webhook_secret"],
        settings: SETTINGS,
        currencies: &[],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    let default = if cfg.bool("live") {
        API_BASE
    } else {
        SANDBOX_BASE
    };
    Ok(Box::new(PaddleGateway {
        key: cfg.secret("api_key")?.into(),
        secret: cfg.secret("webhook_secret")?.into(),
        base: api_base(&cfg.config, default)?,
        t: cfg.transport.clone(),
    }))
}

impl PaddleGateway {
    fn post(&self, path: &str, body: serde_json::Value) -> HttpRequestSpec {
        HttpRequestSpec {
            method: "POST",
            url: format!("{}{path}", self.base),
            headers: vec![
                ("Authorization".into(), format!("Bearer {}", self.key)),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: body.to_string(),
        }
    }
    pub fn verify(&self, h: &Headers, body: &[u8], now: i64) -> Result<(), GatewayError> {
        let hv = header(h, "paddle-signature").ok_or(GatewayError::InvalidSignature)?;
        let mut ts = None;
        let mut sigs = vec![];
        for p in hv.split(';') {
            match p.trim().split_once('=') {
                Some(("ts", v)) => ts = v.parse::<i64>().ok(),
                Some(("h1", v)) => sigs.push(v.to_string()),
                _ => {}
            }
        }
        let ts = ts.ok_or(GatewayError::InvalidSignature)?;
        if (now - ts).abs() > TOLERANCE {
            return Err(GatewayError::InvalidSignature);
        }
        let mut msg = format!("{ts}:").into_bytes();
        msg.extend_from_slice(body);
        let want = hmac_sha256(self.secret.as_bytes(), &msg);
        if sigs
            .iter()
            .any(|s| hex::decode(s).is_ok_and(|b| ct_eq(&b, &want)))
        {
            Ok(())
        } else {
            Err(GatewayError::InvalidSignature)
        }
    }
}

impl PaymentGateway for PaddleGateway {
    fn id(&self) -> &'static str {
        "paddle"
    }
    fn display_name(&self) -> &'static str {
        "Paddle"
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
                "items": [{"quantity": 1, "price": {
                    "description": r.description,
                    "name": r.reference,
                    "unit_price": {"amount": minor_units(r.amount)?.to_string(), "currency_code": r.amount.currency.as_str()},
                    "product": {"name": r.description, "tax_category": "standard"},
                }}],
                "currency_code": r.amount.currency.as_str(),
                "custom_data": {"pelty_payment_id": r.payment_id},
                "checkout": {"url": r.success_url},
            });
            let v = send_json(self.t.as_ref(), self.post("/transactions", body)).await?;
            match (obj_str(&v, "data.id"), obj_str(&v, "data.checkout.url")) {
                (Some(id), Some(url)) => Ok(PaymentInit::Redirect {
                    url,
                    provider_ref: id,
                }),
                _ => Err(GatewayError::Provider(
                    "missing data.id/checkout.url".into(),
                )),
            }
        })
    }
    fn parse_webhook(
        &self,
        h: &Headers,
        body: &[u8],
        now: i64,
    ) -> Result<WebhookEvent, GatewayError> {
        self.verify(h, body, now)?;
        let v: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let id = obj_str(&v, "event_id").ok_or(GatewayError::MalformedPayload)?;
        let ty = obj_str(&v, "event_type").ok_or(GatewayError::MalformedPayload)?;
        let (kind, r) = match ty.as_str() {
            "transaction.completed" | "transaction.paid" => {
                (WebhookKind::PaymentSucceeded, obj_str(&v, "data.id"))
            }
            "transaction.payment_failed" | "transaction.canceled" => {
                (WebhookKind::PaymentFailed, obj_str(&v, "data.id"))
            }
            "adjustment.updated"
                if obj_str(&v, "data.status").as_deref() == Some("approved")
                    && obj_str(&v, "data.type").as_deref() == Some("full") =>
            {
                (WebhookKind::Refunded, obj_str(&v, "data.transaction_id"))
            }
            "adjustment.created" if obj_str(&v, "data.action").as_deref() == Some("chargeback") => {
                (WebhookKind::Disputed, obj_str(&v, "data.transaction_id"))
            }
            _ => (WebhookKind::Ignored, obj_str(&v, "data.id")),
        };
        let mut e = WebhookEvent::new(id, ty, kind, r);
        e.payment_id = obj_str(&v, "data.custom_data.pelty_payment_id");
        Ok(e)
    }
    fn refund<'a>(
        &'a self,
        txn: &'a str,
        amount: Money,
        idem: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            // Paddle adjustments are per line item; our transactions have one.
            let tx = send_json(
                self.t.as_ref(),
                HttpRequestSpec {
                    method: "GET",
                    url: format!(
                        "{}/transactions/{}",
                        self.base,
                        pelty_payments::http::url_encode(txn)
                    ),
                    headers: vec![("Authorization".into(), format!("Bearer {}", self.key))],
                    body: String::new(),
                },
            )
            .await?;
            let item_id = obj_str(&tx, "data.details.line_items.0.id")
                .ok_or_else(|| GatewayError::Provider("transaction has no line item".into()))?;
            let body = serde_json::json!({
                "action": "refund",
                "transaction_id": txn,
                "reason": format!("PeltyCMS refund {idem}"),
                "type": "partial",
                "items": [{"item_id": item_id, "type": "partial", "amount": minor_units(amount)?.to_string()}],
            });
            let v = send_json(self.t.as_ref(), self.post("/adjustments", body)).await?;
            let status = match obj_str(&v, "data.status").as_deref() {
                Some("approved") => "succeeded",
                Some("rejected") => "failed",
                _ => "pending",
            };
            Ok(RefundResult {
                provider_ref: obj_str(&v, "data.id"),
                status: status.into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_payments::test_util::*;

    #[tokio::test]
    async fn transaction_and_webhook() {
        let t = MockTransport::new();
        let gw = factory(&cfg(
            "paddle",
            serde_json::json!({}),
            &[("api_key", "pdl"), ("webhook_secret", "pdl_ntfset")],
            &t,
        ))
        .unwrap();
        t.push(
            201,
            r#"{"data":{"id":"txn_1","checkout":{"url":"https://pay.example/?_ptxn=txn_1"}}}"#,
        );
        assert_eq!(
            gw.create_payment(&req("9.99", "EUR")).await.unwrap(),
            PaymentInit::Redirect {
                url: "https://pay.example/?_ptxn=txn_1".into(),
                provider_ref: "txn_1".into()
            }
        );
        assert!(
            t.requests()[0].url.starts_with(SANDBOX_BASE),
            "sandbox by default"
        );
        let v: serde_json::Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(v["items"][0]["price"]["unit_price"]["amount"], "999");

        let body = r#"{"event_id":"evt_1","event_type":"transaction.completed","data":{"id":"txn_1","custom_data":{"pelty_payment_id":"p1"}}}"#;
        // python3: hmac.new(b"pdl_ntfset", b"1700000000:"+body, sha256).hexdigest()
        let sig = "37409467f48c8561efc31ceb9e60f9b741c1ce4a04437230a52a5a2e7803e7d0";
        let h = hdrs(&[("paddle-signature", &format!("ts=1700000000;h1={sig}"))]);
        let e = gw
            .parse_webhook(&h, body.as_bytes(), 1_700_000_010)
            .unwrap();
        assert_eq!(
            (e.kind, e.provider_ref.as_deref(), e.payment_id.as_deref()),
            (WebhookKind::PaymentSucceeded, Some("txn_1"), Some("p1"))
        );
        assert!(
            gw.parse_webhook(&h, body.as_bytes(), 1_700_000_400)
                .is_err(),
            "stale"
        );
        assert!(gw
            .parse_webhook(&h, body.replace("txn_1", "txn_2").as_bytes(), 1_700_000_000)
            .is_err());

        t.push(
            200,
            r#"{"data":{"id":"txn_1","details":{"line_items":[{"id":"txnitm_1"}]}}}"#,
        )
        .push(
            201,
            r#"{"data":{"id":"adj_1","status":"pending_approval"}}"#,
        );
        let r = gw.refund("txn_1", money("1.00", "EUR"), "k").await.unwrap();
        assert_eq!(
            r,
            RefundResult {
                provider_ref: Some("adj_1".into()),
                status: "pending".into()
            }
        );
        let v: serde_json::Value =
            serde_json::from_str(&t.requests().last().unwrap().body).unwrap();
        assert_eq!(
            (
                v["items"][0]["item_id"].as_str(),
                v["items"][0]["amount"].as_str()
            ),
            (Some("txnitm_1"), Some("100"))
        );
    }
}
