//! Manual bank transfer: shows instructions; staff mark payments as received.
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

pub struct ManualGateway {
    instructions: String,
}

const SETTINGS: &[SettingField] = &[SettingField::of("instructions", "textarea")];

pub fn descriptor() -> GatewayDescriptor {
    GatewayDescriptor {
        id: "manual",
        name: "Manual bank transfer",
        capabilities: Capabilities {
            checkout: true,
            webhooks: false,
            refunds: true,
            recurring: false,
        },
        secrets: &[],
        settings: SETTINGS,
        currencies: &[],
    }
}

pub fn factory(cfg: &GatewayConfig) -> Result<Box<dyn PaymentGateway>, GatewayError> {
    let instructions = cfg.str("instructions").unwrap_or("").trim().to_string();
    Ok(Box::new(ManualGateway { instructions }))
}

impl PaymentGateway for ManualGateway {
    fn id(&self) -> &'static str {
        "manual"
    }
    fn display_name(&self) -> &'static str {
        "Manual bank transfer"
    }
    fn capabilities(&self) -> Capabilities {
        descriptor().capabilities
    }
    fn create_payment<'a>(
        &'a self,
        req: &'a PaymentRequest,
    ) -> BoxFuture<'a, Result<PaymentInit, GatewayError>> {
        Box::pin(async move {
            Ok(PaymentInit::Instructions {
                text: self.instructions.clone(),
                reference: req.reference.clone(),
                amount: req.amount.rounded().amount.to_string(),
                currency: req.amount.currency.as_str().to_string(),
            })
        })
    }
    fn parse_webhook(&self, _: &Headers, _: &[u8], _: i64) -> Result<WebhookEvent, GatewayError> {
        Err(GatewayError::Unsupported)
    }
    fn refund<'a>(
        &'a self,
        provider_ref: &'a str,
        _: Money,
        _: &'a str,
    ) -> BoxFuture<'a, Result<RefundResult, GatewayError>> {
        Box::pin(async move {
            Ok(RefundResult {
                provider_ref: Some(provider_ref.to_string()),
                status: "manual_pending".into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pelty_core::money::{Currency, Money};
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn manual_instructions() {
        let g = factory(&GatewayConfig {
            config: serde_json::json!({"instructions": "IBAN RO00"}),
            ..Default::default()
        })
        .unwrap();
        let req = PaymentRequest {
            payment_id: "pay_1".into(),
            reference: "INV-000007".into(),
            amount: Money::new(Decimal::new(1999, 2), Currency::parse("EUR").unwrap()),
            description: "VPS".into(),
            customer_email: None,
            webhook_url: String::new(),
            success_url: String::new(),
            cancel_url: String::new(),
            idempotency_key: "k".into(),
        };
        match g.create_payment(&req).await.unwrap() {
            PaymentInit::Instructions {
                text,
                reference,
                amount,
                currency,
            } => {
                assert_eq!(text, "IBAN RO00");
                assert_eq!(reference, "INV-000007");
                assert_eq!((amount.as_str(), currency.as_str()), ("19.99", "EUR"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            g.parse_webhook(&Headers::new(), b"", 0),
            Err(GatewayError::Unsupported)
        );
    }
}
