//! Razorpay (Payment Links API). Webhooks: `X-Razorpay-Signature` =
//! hex(HMAC-SHA256(body, webhook_secret)). Verified with mocked HTTP only.
use base64::Engine;
use pelty_payments::http::{api_base, ct_eq, hmac_sha256, send_json, url_encode};
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

pub const API_BASE: &str = "https://api.razorpay.com/v1";

pub struct RazorpayGateway {
    key_id: String,
    key_secret: String,
    webhook_secret: String,
    base: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[SettingField::of("api_base", "url")];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "razorpay",
        name: "Razorpay",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: true,
            recurring: false,
        },
        secrets: &["key_id", "key_secret", "webhook_secret"],
        settings: SETTINGS,
        currencies: &[],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    Ok(Box::new(RazorpayGateway {
        key_id: cfg.secret("key_id")?.into(),
        key_secret: cfg.secret("key_secret")?.into(),
        webhook_secret: cfg.secret("webhook_secret")?.into(),
        base: api_base(&cfg.config, API_BASE)?,
        t: cfg.transport.clone(),
    }))
}

impl RazorpayGateway {
    fn spec(
        &self,
        method: &'static str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> HttpRequestSpec {
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.key_id, self.key_secret));
        let mut headers = vec![("Authorization".into(), format!("Basic {basic}"))];
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        HttpRequestSpec {
            method,
            url: format!("{}{path}", self.base),
            headers,
            body: body.map(|b| b.to_string()).unwrap_or_default(),
        }
    }
    pub fn verify(&self, h: &Headers, body: &[u8]) -> Result<(), GatewayError> {
        let sig = header(h, "x-razorpay-signature").ok_or(GatewayError::InvalidSignature)?;
        let want = hmac_sha256(self.webhook_secret.as_bytes(), body);
        match hex::decode(sig.trim()) {
            Ok(b) if ct_eq(&b, &want) => Ok(()),
            _ => Err(GatewayError::InvalidSignature),
        }
    }
}

impl PaymentGateway for RazorpayGateway {
    fn id(&self) -> &'static str {
        "razorpay"
    }
    fn display_name(&self) -> &'static str {
        "Razorpay"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(
        &'a self,
        r: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let mut body = serde_json::json!({
                "amount": minor_units(r.amount)?,
                "currency": r.amount.currency.as_str(),
                "reference_id": r.payment_id,
                "description": r.description,
                "callback_url": r.success_url,
                "callback_method": "get",
                "notes": {"pelty_payment_id": r.payment_id, "invoice": r.reference},
            });
            if let Some(e) = &r.customer_email {
                body["customer"] = serde_json::json!({"email": e});
            }
            let v = send_json(
                self.t.as_ref(),
                self.spec("POST", "/payment_links", Some(body)),
            )
            .await?;
            match (v["id"].as_str(), v["short_url"].as_str()) {
                (Some(id), Some(url)) => Ok(PaymentInit::Redirect {
                    url: url.into(),
                    provider_ref: id.into(),
                }),
                _ => Err(GatewayError::Provider("missing id/short_url".into())),
            }
        })
    }
    fn parse_webhook(
        &self,
        h: &Headers,
        body: &[u8],
        _now: i64,
    ) -> Result<WebhookEvent, GatewayError> {
        self.verify(h, body)?;
        let v: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let ev = v["event"]
            .as_str()
            .ok_or(GatewayError::MalformedPayload)?
            .to_string();
        let p = &v["payload"];
        let (kind, r) = match ev.as_str() {
            "payment_link.paid" => (
                WebhookKind::PaymentSucceeded,
                obj_str(p, "payment_link.entity.id"),
            ),
            "payment_link.expired" | "payment_link.cancelled" => (
                WebhookKind::PaymentFailed,
                obj_str(p, "payment_link.entity.id"),
            ),
            "payment.dispute.created" => (WebhookKind::Disputed, obj_str(p, "payment.entity.id")),
            _ => (WebhookKind::Ignored, None),
        };
        // Razorpay sends a unique event id header; fall back to entity+event.
        let id = header(h, "x-razorpay-event-id")
            .map(str::to_string)
            .or_else(|| r.clone().map(|r| format!("{r}:{ev}")))
            .ok_or(GatewayError::MalformedPayload)?;
        let mut e = WebhookEvent::new(id, ev, kind, r);
        e.payment_id = obj_str(p, "payment_link.entity.reference_id");
        Ok(e)
    }
    fn refund<'a>(
        &'a self,
        r: &'a str,
        amount: Money,
        idem: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            // provider_ref is the payment link; refunds target its payment.
            let pay_id = if r.starts_with("pay_") {
                r.to_string()
            } else {
                let l = send_json(
                    self.t.as_ref(),
                    self.spec("GET", &format!("/payment_links/{}", url_encode(r)), None),
                )
                .await?;
                obj_str(&l, "payments.0.payment_id")
                    .ok_or_else(|| GatewayError::Provider("link has no payment".into()))?
            };
            let body = serde_json::json!({"amount": minor_units(amount)?, "receipt": idem, "notes": {"idempotency_key": idem}});
            let v = send_json(
                self.t.as_ref(),
                self.spec(
                    "POST",
                    &format!("/payments/{}/refund", url_encode(&pay_id)),
                    Some(body),
                ),
            )
            .await?;
            let status = match v["status"].as_str() {
                Some("processed") => "succeeded",
                Some("failed") => "failed",
                _ => "pending",
            };
            Ok(RefundResult {
                provider_ref: v["id"].as_str().map(str::to_string),
                status: status.into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_payments::test_util::*;

    fn g(t: &MockTransport) -> Box<dyn PaymentGateway> {
        factory(&cfg(
            "razorpay",
            serde_json::json!({}),
            &[
                ("key_id", "rzp_test_1"),
                ("key_secret", "s"),
                ("webhook_secret", "whs"),
            ],
            t,
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn link_webhook_refund() {
        let t = MockTransport::new();
        t.push(200, r#"{"id":"plink_1","short_url":"https://rzp.io/i/x"}"#);
        let gw = g(&t);
        assert_eq!(
            gw.create_payment(&req("499", "INR")).await.unwrap(),
            PaymentInit::Redirect {
                url: "https://rzp.io/i/x".into(),
                provider_ref: "plink_1".into()
            }
        );
        let v: serde_json::Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(v["amount"], 49900);

        let body = r#"{"event":"payment_link.paid","payload":{"payment_link":{"entity":{"id":"plink_1","reference_id":"pay_1"}}}}"#;
        // Signature computed independently: python3 hmac.new(b"whs", body, sha256).hexdigest()
        let sig = "80ce137db3540c734b80c65bb5310f52d458b41e72ccaa07c75c0de38c979f41";
        let h = hdrs(&[
            ("x-razorpay-signature", sig),
            ("x-razorpay-event-id", "evt_1"),
        ]);
        let e = gw.parse_webhook(&h, body.as_bytes(), 0).unwrap();
        assert_eq!(
            (
                e.kind.clone(),
                e.provider_ref.as_deref(),
                e.event_id.as_str()
            ),
            (WebhookKind::PaymentSucceeded, Some("plink_1"), "evt_1")
        );
        assert_eq!(
            gw.parse_webhook(&h, body.replace("plink_1", "plink_2").as_bytes(), 0)
                .unwrap_err(),
            GatewayError::InvalidSignature
        );

        t.push(
            200,
            r#"{"id":"plink_1","payments":[{"payment_id":"pay_X"}]}"#,
        )
        .push(200, r#"{"id":"rfnd_1","status":"processed"}"#);
        let r = gw
            .refund("plink_1", money("100", "INR"), "k")
            .await
            .unwrap();
        assert_eq!(r.status, "succeeded");
        assert!(t
            .requests()
            .last()
            .unwrap()
            .url
            .ends_with("/payments/pay_X/refund"));
    }
}
