//! Authorize.net Accept Hosted: getHostedPaymentPageRequest, webhook, refund.
use pelty_payments::http::{api_base, send_json, HttpRequestSpec};
use pelty_core::money::Money;
use pelty_payments::*;
use serde_json::{json, Value};
use std::sync::Arc;

pub const API_LIVE: &str = "https://api.authorize.net/xml/v1/request.api";
pub const API_SANDBOX: &str = "https://apitest.authorize.net/xml/v1/request.api";
pub const PAY_LIVE: &str = "https://accept.authorize.net/payment/payment";
pub const PAY_SANDBOX: &str = "https://test.authorize.net/payment/payment";

pub struct AuthorizeNetGateway {
    login: String,
    key: String,
    api: String,
    pay: String,
    t: Arc<dyn HttpTransport>,
}

const SETTINGS: &[SettingField] = &[
    SettingField::of("live", "bool"),
    SettingField::of("api_base", "url"),
];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "authorizenet",
        name: "Authorize.net",
        capabilities: Capabilities {
            checkout: true,
            webhooks: true,
            refunds: true,
            recurring: false,
        },
        secrets: &["api_login_id", "transaction_key", "signature_key"],
        settings: SETTINGS,
        currencies: &["USD", "CAD", "GBP", "EUR", "AUD"],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    let live = cfg.bool("live");
    let default_api = if live { API_LIVE } else { API_SANDBOX };
    Ok(Box::new(AuthorizeNetGateway {
        login: cfg.secret("api_login_id")?.into(),
        key: cfg.secret("transaction_key")?.into(),
        api: api_base(&cfg.config, default_api)?,
        pay: if live { PAY_LIVE } else { PAY_SANDBOX }.into(),
        t: cfg.transport.clone(),
    }))
}

impl AuthorizeNetGateway {
    fn auth(&self) -> Value {
        json!({ "name": self.login, "transactionKey": self.key })
    }
    fn post(&self, body: Value) -> HttpRequestSpec {
        HttpRequestSpec {
            method: "POST",
            url: self.api.clone(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string(),
        }
    }
}

impl PaymentGateway for AuthorizeNetGateway {
    fn id(&self) -> &'static str {
        "authorizenet"
    }
    fn display_name(&self) -> &'static str {
        "Authorize.net"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }

    fn create_payment<'a>(
        &'a self,
        req: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            let amount = decimal_str(req.amount)?;
            let body = json!({
                "getHostedPaymentPageRequest": {
                    "merchantAuthentication": self.auth(),
                    "transactionRequest": {
                        "transactionType": "authCaptureTransaction",
                        "amount": amount,
                        "order": {
                            "invoiceNumber": req.reference.chars().take(20).collect::<String>(),
                            "description": req.description.chars().take(255).collect::<String>(),
                        },
                        "customer": { "email": req.customer_email },
                    },
                    "hostedPaymentSettings": {
                        "setting": [
                            { "settingName": "hostedPaymentReturnOptions",
                              "settingValue": format!(
                                "{{\"showReceipt\": false,\"url\":\"{}\",\"cancelUrl\":\"{}\"}}",
                                req.success_url, req.cancel_url
                              ) },
                            { "settingName": "hostedPaymentButtonOptions",
                              "settingValue": "{\"text\":\"Pay\"}" },
                        ]
                    }
                }
            });
            let resp = send_json(self.t.as_ref(), self.post(body)).await?;
            let token = resp
                .pointer("/token")
                .or_else(|| resp.pointer("/getHostedPaymentPageResponse/token"))
                .and_then(|t| t.as_str())
                .ok_or_else(|| {
                    GatewayError::Provider(
                        resp.pointer("/messages/message/0/text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("Authorize.net did not return a token")
                            .into(),
                    )
                })?;
            Ok(PaymentInit::Redirect {
                url: format!("{}?token={}", self.pay, token),
                provider_ref: req.payment_id.clone(),
            })
        })
    }

    fn parse_webhook(
        &self,
        _h: &Headers,
        body: &[u8],
        _now: i64,
    ) -> Result<WebhookEvent, GatewayError> {
        let v: Value =
            serde_json::from_slice(body).map_err(|_| GatewayError::MalformedPayload)?;
        let event = v["eventType"].as_str().unwrap_or("unknown").to_string();
        let payload = &v["payload"];
        let id = payload["id"]
            .as_str()
            .or_else(|| payload["merchantReferenceId"].as_str())
            .unwrap_or(&event)
            .to_string();
        let kind = match event.as_str() {
            "net.authorize.payment.authcapture.created"
            | "net.authorize.payment.capture.created"
            | "net.authorize.payment.fraud.approved" => WebhookKind::PaymentSucceeded,
            "net.authorize.payment.void.created" | "net.authorize.payment.refund.created" => {
                WebhookKind::Refunded
            }
            _ => WebhookKind::Ignored,
        };
        let pref = payload["id"].as_str().map(str::to_string);
        Ok(WebhookEvent::new(id, event, kind, pref))
    }

    fn refund<'a>(
        &'a self,
        provider_ref: &'a str,
        amount: Money,
        _: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            let body = json!({
                "createTransactionRequest": {
                    "merchantAuthentication": self.auth(),
                    "transactionRequest": {
                        "transactionType": "refundTransaction",
                        "amount": decimal_str(amount)?,
                        "refTransId": provider_ref,
                    }
                }
            });
            let resp = send_json(self.t.as_ref(), self.post(body)).await?;
            let id = resp
                .pointer("/transactionResponse/transId")
                .and_then(|t| t.as_str())
                .unwrap_or(provider_ref);
            Ok(RefundResult {
                provider_ref: Some(id.into()),
                status: "refunded".into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_core::money::{Currency, Money};
    use pelty_payments::MockTransport;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn hosted_page_redirect() {
        let t = MockTransport::new();
        t.push(200, r#"{"token":"tok-abc"}"#);
        let g = AuthorizeNetGateway {
            login: "login".into(),
            key: "key".into(),
            api: API_SANDBOX.into(),
            pay: PAY_SANDBOX.into(),
            t: Arc::new(t),
        };
        let req = PaymentRequest {
            payment_id: "pay_1".into(),
            reference: "INV-1".into(),
            amount: Money::new(Decimal::new(1999, 2), Currency::parse("USD").unwrap()),
            description: "VPS".into(),
            customer_email: Some("a@b.c".into()),
            webhook_url: String::new(),
            success_url: "https://ex/ok".into(),
            cancel_url: "https://ex/no".into(),
            idempotency_key: "k".into(),
        };
        match g.create_payment(&req).await.unwrap() {
            PaymentInit::Redirect { url, .. } => {
                assert!(url.contains("token=tok-abc"));
                assert!(url.starts_with(PAY_SANDBOX));
            }
            other => panic!("{other:?}"),
        }
    }
}