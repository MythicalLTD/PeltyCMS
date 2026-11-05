//! Netopia Payments (mobilPay) card start API: redirect checkout + IPN webhook.
//! Mock-tested only; live API shapes vary by merchant contract.
use pelty_payments::http::{api_base, ct_eq, hmac_sha256, send_json, HttpRequestSpec};
use pelty_core::money::Money;
use pelty_payments::*;
use serde_json::json;
use std::sync::Arc;

pub const API_LIVE: &str = "https://secure.mobilpay.ro";
pub const API_SANDBOX: &str = "https://secure.sandbox.netopia-payments.ro";

pub struct NetopiaGateway {
    pos: String,
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
        id: "netopia",
        name: "Netopia Payments",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: false,
            recurring: false,
        },
        secrets: &["pos_signature", "signature_key"],
        settings: SETTINGS,
        currencies: &["RON", "EUR", "USD"],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    let live = cfg.bool("live");
    let default = if live { API_LIVE } else { API_SANDBOX };
    Ok(Box::new(NetopiaGateway {
        pos: cfg.secret("pos_signature")?.into(),
        secret: cfg.secret("signature_key")?.into(),
        base: api_base(&cfg.config, default)?,
        t: cfg.transport.clone(),
    }))
}

impl NetopiaGateway {
    fn verify(&self, h: &Headers, body: &[u8]) -> Result<(), GatewayError> {
        let sig = header(h, "x-netopia-signature")
            .or_else(|| header(h, "x-signature"))
            .ok_or(GatewayError::InvalidSignature)?;
        let want = hmac_sha256(self.secret.as_bytes(), body);
        let got = hex::decode(sig).map_err(|_| GatewayError::InvalidSignature)?;
        if ct_eq(&got, &want) {
            Ok(())
        } else {
            Err(GatewayError::InvalidSignature)
        }
    }
}

impl PaymentGateway for NetopiaGateway {
    fn id(&self) -> &'static str {
        "netopia"
    }
    fn display_name(&self) -> &'static str {
        "Netopia Payments"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }

    fn create_payment<'a>(
        &'a self,
        req: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let body = json!({
                "config": { "emailTemplate": "invoice", "notifyUrl": req.webhook_url, "redirectUrl": req.success_url },
                "payment": {
                    "options": { "installments": 0, "bonus": 0 },
                    "instrument": { "type": "card", "account": "" },
                    "data": { "posSignature": self.pos }
                },
                "order": {
                    "ntpID": "",
                    "posSignature": self.pos,
                    "dateTime": "",
                    "description": req.description,
                    "orderID": req.reference,
                    "amount": decimal_str(req.amount)?,
                    "currency": req.amount.currency.as_str(),
                    "billing": {
                        "email": req.customer_email.clone().unwrap_or_default(),
                        "type": "person"
                    }
                }
            });
            let resp = send_json(
                self.t.as_ref(),
                HttpRequestSpec {
                    method: "POST",
                    url: format!("{}/payment/card/start", self.base.trim_end_matches('/')),
                    headers: vec![
                        ("Content-Type".into(), "application/json".into()),
                        ("Authorization".into(), self.pos.clone()),
                    ],
                    body: body.to_string(),
                },
            )
            .await?;
            let url = obj_str(&resp, "payment.paymentURL")
                .or_else(|| obj_str(&resp, "paymentURL"))
                .ok_or_else(|| GatewayError::Provider("missing paymentURL".into()))?;
            let pref = obj_str(&resp, "payment.ntpID")
                .or_else(|| obj_str(&resp, "ntpID"))
                .unwrap_or_else(|| req.payment_id.clone());
            Ok(PaymentInit::Redirect {
                url,
                provider_ref: pref,
            })
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
        let status = obj_str(&v, "payment.status")
            .or_else(|| obj_str(&v, "status"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let id = obj_str(&v, "payment.ntpID")
            .or_else(|| obj_str(&v, "ntpID"))
            .or_else(|| obj_str(&v, "order.orderID"))
            .unwrap_or_else(|| "netopia".into());
        let kind = match status.as_str() {
            "confirmed" | "paid" | "3" => WebhookKind::PaymentSucceeded,
            "canceled" | "rejected" | "failed" => WebhookKind::PaymentFailed,
            "refunded" => WebhookKind::Refunded,
            _ => WebhookKind::Ignored,
        };
        let mut e = WebhookEvent::new(id.clone(), status, kind, Some(id));
        e.payment_id = obj_str(&v, "order.orderID");
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
    use pelty_core::money::{Currency, Money};
    use pelty_payments::MockTransport;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn start_redirect() {
        let t = MockTransport::new();
        t.push(200, r#"{"payment":{"paymentURL":"https://pay.example/x","ntpID":"ntp1"}}"#);
        let g = NetopiaGateway {
            pos: "POS".into(),
            secret: "sec".into(),
            base: API_SANDBOX.into(),
            t: Arc::new(t),
        };
        let req = PaymentRequest {
            payment_id: "pay_1".into(),
            reference: "INV-9".into(),
            amount: Money::new(Decimal::new(5000, 2), Currency::parse("RON").unwrap()),
            description: "Hosting".into(),
            customer_email: Some("a@b.c".into()),
            webhook_url: "https://ex/hook".into(),
            success_url: "https://ex/ok".into(),
            cancel_url: "https://ex/no".into(),
            idempotency_key: "k".into(),
        };
        match g.create_payment(&req).await.unwrap() {
            PaymentInit::Redirect { url, provider_ref } => {
                assert_eq!(url, "https://pay.example/x");
                assert_eq!(provider_ref, "ntp1");
            }
            other => panic!("{other:?}"),
        }
    }
}