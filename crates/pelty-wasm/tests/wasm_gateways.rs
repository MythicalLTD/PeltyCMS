//! WASM ports of the native gateway behaviour tests (`pelty-payments/src/<gw>.rs`).
//! Each module's tests run unchanged except that `factory` builds the
//! gateway from its compiled addon, with the host HTTP transport mocked.
#![allow(unused_imports, dead_code)]
mod common;

pub(crate) mod test_util {
    use pelty_core::money::{Currency, Money};
    use pelty_payments::*;
    use std::str::FromStr;
    use std::sync::Arc;
    pub fn money(v: &str, c: &str) -> Money {
        Money::new(
            rust_decimal::Decimal::from_str(v).unwrap(),
            Currency::parse(c).unwrap(),
        )
    }
    pub fn req(v: &str, c: &str) -> PaymentRequest {
        PaymentRequest {
            payment_id: "pay_1".into(),
            reference: "INV-7".into(),
            amount: money(v, c),
            description: "Invoice INV-7".into(),
            customer_email: Some("a@example.test".into()),
            success_url: "https://panel.example/ok".into(),
            cancel_url: "https://panel.example/cancel".into(),
            webhook_url: "https://panel.example/api/v1/webhooks/x".into(),
            idempotency_key: "idem-1".into(),
        }
    }
    pub fn cfg(
        id: &str,
        config: serde_json::Value,
        secrets: &[(&str, &str)],
        t: &MockTransport,
    ) -> GatewayConfig {
        GatewayConfig {
            id: id.into(),
            config,
            secrets: secrets
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            transport: Arc::new(t.clone()),
        }
    }
    pub fn hdrs(p: &[(&str, &str)]) -> Headers {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    /// Build gateway `id` from addon `extensions/addons/<dir>`, HTTP through `c.transport`.
    pub fn wasm_factory(
        dir: &str,
        c: &GatewayConfig,
    ) -> Result<Box<dyn PaymentGateway>, GatewayError> {
        let zip = crate::common::addon_zip(dir, &format!("pelty-addon-gateway-{dir}"));
        let services = pelty_wasm::HostServices {
            http: c.transport.clone(),
            ..Default::default()
        };
        let addon = pelty_wasm::WasmAddon::from_zip(
            &zip,
            pelty_extensions::addon_pkg::Limits::default(),
            services,
        )
        .expect("load addon");
        let reg = GatewayRegistry::empty(c.transport.clone());
        let d = pelty_wasm::gateway::register(&reg, std::sync::Arc::new(addon)).expect("register");
        reg.build(d.id, c)
    }
}

mod mollie {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("mollie", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_mollie::mollie::*;
    use pelty_payments::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn g(t: &MockTransport) -> Box<dyn PaymentGateway> {
        factory(&cfg(
            "mollie",
            serde_json::json!({}),
            &[("api_key", "test_abc")],
            t,
        ))
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_webhook_refund() {
        let t = MockTransport::new();
        t.push(201, r#"{"id":"tr_1","status":"open","_links":{"checkout":{"href":"https://www.mollie.com/checkout/tr_1"}}}"#);
        let gw = g(&t);
        let init = gw.create_payment(&req("10", "EUR")).await.unwrap();
        assert_eq!(
            init,
            PaymentInit::Redirect {
                url: "https://www.mollie.com/checkout/tr_1".into(),
                provider_ref: "tr_1".into()
            }
        );
        let s = &t.requests()[0];
        let v: serde_json::Value = serde_json::from_str(&s.body).unwrap();
        assert_eq!(v["amount"]["value"], "10.00");
        assert_eq!(v["webhookUrl"], "https://panel.example/api/v1/webhooks/x");
        assert_eq!(s.header("authorization"), Some("Bearer test_abc"));

        t.push(200, r#"{"id":"tr_1","status":"paid","amount":{"value":"10.00"},"amountRefunded":{"value":"0.00"},"metadata":{"pelty_payment_id":"p1"}}"#);
        let e = gw
            .handle_webhook(&Headers::new(), b"id=tr_1", 0)
            .await
            .unwrap();
        assert_eq!(
            (e.kind, e.provider_ref.as_deref(), e.payment_id.as_deref()),
            (WebhookKind::PaymentSucceeded, Some("tr_1"), Some("p1"))
        );
        assert_eq!(e.event_id, "tr_1:paid:0.00");

        // Unknown id (forged webhook) → treated as a bad signature.
        t.push(404, r#"{"status":404,"detail":"No payment exists"}"#);
        assert_eq!(
            gw.handle_webhook(&Headers::new(), b"id=tr_forged", 0)
                .await
                .unwrap_err(),
            GatewayError::InvalidSignature
        );
        assert_eq!(
            gw.handle_webhook(&Headers::new(), b"foo=bar", 0)
                .await
                .unwrap_err(),
            GatewayError::MalformedPayload
        );

        t.push(201, r#"{"id":"re_1","status":"pending"}"#);
        let r = gw.refund("tr_1", money("4.00", "EUR"), "rk").await.unwrap();
        assert_eq!(r.status, "pending");
        assert!(t
            .requests()
            .last()
            .unwrap()
            .url
            .ends_with("/payments/tr_1/refunds"));

        t.push(200, r#"{"id":"tr_1","status":"paid","amount":{"value":"10.00"},"amountRefunded":{"value":"10.00"}}"#);
        assert_eq!(
            gw.handle_webhook(&Headers::new(), b"id=tr_1", 0)
                .await
                .unwrap()
                .kind,
            WebhookKind::Refunded
        );
    }
}

mod razorpay {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("razorpay", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_razorpay::razorpay::*;
    use pelty_payments::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

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

    #[tokio::test(flavor = "multi_thread")]
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

mod gocardless {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("gocardless", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_gocardless::gocardless::classify;
    use pelty_payments::*;
    use std::collections::HashMap;

    fn g(t: &MockTransport, sandbox: bool) -> Box<dyn PaymentGateway> {
        factory(&cfg(
            "gocardless",
            serde_json::json!({"sandbox": sandbox}),
            &[("access_token", "sandbox_tok"), ("webhook_secret", "whs")],
            t,
        ))
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn billing_request_flow() {
        let t = MockTransport::new();
        t.push(201, r#"{"billing_requests":{"id":"BRQ1","status":"pending"}}"#).push(201, r#"{"billing_request_flows":{"id":"BRF1","authorisation_url":"https://pay-sandbox.gocardless.com/flow/BRF1"}}"#);
        let gw = g(&t, true);
        assert_eq!(
            gw.create_payment(&req("12.34", "EUR")).await.unwrap(),
            PaymentInit::Redirect {
                url: "https://pay-sandbox.gocardless.com/flow/BRF1".into(),
                provider_ref: "BRQ1".into()
            }
        );
        let rq = t.requests();
        assert_eq!(
            rq[0].url,
            "https://api-sandbox.gocardless.com/billing_requests"
        );
        assert_eq!(rq[0].header("GoCardless-Version"), Some("2015-07-06"));
        assert_eq!(rq[0].header("Idempotency-Key"), Some("idem-1"));
        let b: serde_json::Value = serde_json::from_str(&rq[0].body).unwrap();
        assert_eq!(b["billing_requests"]["payment_request"]["amount"], 1234);
        assert_eq!(b["billing_requests"]["payment_request"]["currency"], "EUR");
        assert!(b["billing_requests"]["mandate_request"].is_object());
        let f: serde_json::Value = serde_json::from_str(&rq[1].body).unwrap();
        assert_eq!(
            f["billing_request_flows"]["links"]["billing_request"],
            "BRQ1"
        );
        assert_eq!(
            f["billing_request_flows"]["prefilled_customer"]["email"],
            "a@example.test"
        );
        // Live base when sandbox is off.
        let t2 = MockTransport::new();
        t2.push(201, r#"{"billing_requests":{"id":"BRQ2"}}"#).push(
            201,
            r#"{"billing_request_flows":{"authorisation_url":"https://pay.gocardless.com/x"}}"#,
        );
        g(&t2, false)
            .create_payment(&req("1", "GBP"))
            .await
            .unwrap();
        assert!(t2.requests()[0]
            .url
            .starts_with("https://api.gocardless.com/"));
        // Return: fulfilled is still pending (DD clears later); cancelled fails.
        t.push(200, r#"{"billing_requests":{"status":"fulfilled"}}"#)
            .push(200, r#"{"billing_requests":{"status":"cancelled"}}"#);
        assert_eq!(
            gw.complete_payment("BRQ1", &HashMap::new()).await.unwrap(),
            CompleteStatus::Pending
        );
        assert_eq!(
            gw.complete_payment("BRQ1", &HashMap::new()).await.unwrap(),
            CompleteStatus::Failed
        );
    }

    // Signatures computed independently:
    // python3 -c 'import hmac,hashlib,sys;print(hmac.new(b"whs",open(f).read().encode(),hashlib.sha256).hexdigest())'
    pub const BATCH: &str = r#"{"events":[{"id":"EV2","resource_type":"mandates","action":"active","links":{"mandate":"MD1"}},{"id":"EV1","resource_type":"payments","action":"confirmed","links":{"payment":"PM1"},"resource_metadata":{"pelty_payment_id":"pay_1"}}]}"#;
    pub const BATCH_SIG: &str = "65c85a327fae318460b942de378c16b0ff55ac3f271250c6a803cad4abb0f538";
    pub const FAIL: &str = r#"{"events":[{"id":"EV3","resource_type":"payments","action":"confirmed","links":{"payment":"PM1"}},{"id":"EV4","resource_type":"payments","action":"charged_back","links":{"payment":"PM1"}}]}"#;
    pub const FAIL_SIG: &str = "d47a8708b3dcda5aa7d733b685dcebf094776e242da59a78421cce5dfc3915b4";

    #[test]
    fn webhooks() {
        let t = MockTransport::new();
        let gw = g(&t, true);
        let e = gw
            .parse_webhook(
                &hdrs(&[("webhook-signature", BATCH_SIG)]),
                BATCH.as_bytes(),
                0,
            )
            .unwrap();
        assert_eq!(
            (
                e.kind.clone(),
                e.provider_ref.as_deref(),
                e.payment_id.as_deref(),
                e.event_id.as_str(),
                e.event_type.as_str()
            ),
            (
                WebhookKind::PaymentSucceeded,
                Some("PM1"),
                Some("pay_1"),
                "EV1,EV2",
                "payments.confirmed"
            )
        );
        let e = gw
            .parse_webhook(
                &hdrs(&[("webhook-signature", FAIL_SIG)]),
                FAIL.as_bytes(),
                0,
            )
            .unwrap();
        assert_eq!(e.kind, WebhookKind::Disputed);
        assert_eq!(
            gw.parse_webhook(
                &hdrs(&[("webhook-signature", BATCH_SIG)]),
                BATCH.replace("PM1", "PM9").as_bytes(),
                0
            )
            .unwrap_err(),
            GatewayError::InvalidSignature
        );
        assert_eq!(
            gw.parse_webhook(&hdrs(&[]), BATCH.as_bytes(), 0)
                .unwrap_err(),
            GatewayError::InvalidSignature
        );
        assert_eq!(classify("mandates", "cancelled").0, WebhookKind::Ignored);
        assert_eq!(classify("payments", "failed").0, WebhookKind::PaymentFailed);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refund_and_mandate_charges() {
        let t = MockTransport::new();
        let gw = g(&t, true);
        t.push(
            200,
            r#"{"billing_requests":{"links":{"payment_request_payment":"PM7"}}}"#,
        )
        .push(201, r#"{"refunds":{"id":"RF1","status":"created"}}"#);
        let r = gw
            .refund("BRQ1", money("5.00", "EUR"), "refund-key")
            .await
            .unwrap();
        assert_eq!(
            (r.status.as_str(), r.provider_ref.as_deref()),
            ("pending", Some("RF1"))
        );
        let rq = t.requests();
        let b: serde_json::Value = serde_json::from_str(&rq[1].body).unwrap();
        assert_eq!(
            (
                b["refunds"]["amount"].as_i64(),
                b["refunds"]["links"]["payment"].as_str()
            ),
            (Some(500), Some("PM7"))
        );

        assert!(gw.save_method("pm_card", "a@x", "k").await.is_err());
        t.push(
            200,
            r#"{"mandates":{"id":"MD1","status":"active","scheme":"sepa_core"}}"#,
        );
        assert_eq!(
            gw.save_method("MD1", "a@x", "k").await.unwrap().brand,
            "sepa_core"
        );
        t.push(200, r#"{"mandates":{"id":"MD2","status":"cancelled"}}"#);
        assert!(gw.save_method("MD2", "a@x", "k").await.is_err());

        t.push(
            201,
            r#"{"payments":{"id":"PM9","status":"pending_submission"}}"#,
        );
        assert_eq!(
            gw.charge_saved("MD1", &req("9.99", "EUR")).await.unwrap(),
            ChargeOutcome::Pending {
                provider_ref: "PM9".into()
            }
        );
        let b: serde_json::Value =
            serde_json::from_str(&t.requests().last().unwrap().body).unwrap();
        assert_eq!(
            (
                b["payments"]["amount"].as_i64(),
                b["payments"]["links"]["mandate"].as_str()
            ),
            (Some(999), Some("MD1"))
        );
        t.push(422, r#"{"error":{"message":"Mandate is cancelled","errors":[{"reason":"mandate_is_inactive"}]}}"#);
        assert_eq!(
            gw.charge_saved("MD1", &req("9.99", "EUR")).await.unwrap(),
            ChargeOutcome::Declined {
                reason: "mandate_is_inactive".into()
            }
        );
        t.push(500, "{}");
        assert!(gw.charge_saved("MD1", &req("9.99", "EUR")).await.is_err());
    }
}

mod crypto {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("crypto", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_crypto::crypto::*;
    use pelty_payments::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test(flavor = "multi_thread")]
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

mod paddle {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("paddle", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_paddle::paddle::*;
    use pelty_payments::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test(flavor = "multi_thread")]
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

mod adyen {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("adyen", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_adyen::adyen::*;
    use pelty_payments::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    const KEY: &str = "44782DEF547AAA06C910C43932B1EB0C71FC68D9D0C057550C48EC2ACF6BA056";

    #[tokio::test(flavor = "multi_thread")]
    async fn link_and_hmac_notification() {
        let t = MockTransport::new();
        let gw = factory(&cfg(
            "adyen",
            serde_json::json!({"merchant_account": "TestMerchant"}),
            &[("api_key", "AQE"), ("hmac_key", KEY)],
            &t,
        ))
        .unwrap();
        t.push(201, r#"{"id":"PL1","url":"https://test.adyen.link/PL1"}"#);
        assert_eq!(
            gw.create_payment(&req("10.5", "EUR")).await.unwrap(),
            PaymentInit::Redirect {
                url: "https://test.adyen.link/PL1".into(),
                provider_ref: "pay_1".into()
            }
        );
        let v: serde_json::Value = serde_json::from_str(&t.requests()[0].body).unwrap();
        assert_eq!(v["amount"]["value"], 1050);
        assert!(t.requests()[0]
            .url
            .starts_with("https://checkout-test.adyen.com"));

        // Signature computed independently in Python:
        // base64(hmac(bytes.fromhex(KEY), b"7914073381342284::TestMerchant:pay_1:1050:EUR:AUTHORISATION:true", sha256))
        let sig = "6JAYqENwSbDKqOxEv30u+mCNiKBur7xF1Et9vViwkK4=";
        let body = format!(
            r#"{{"live":"false","notificationItems":[{{"NotificationRequestItem":{{"additionalData":{{"hmacSignature":"{sig}"}},"amount":{{"currency":"EUR","value":1050}},"eventCode":"AUTHORISATION","merchantAccountCode":"TestMerchant","merchantReference":"pay_1","pspReference":"7914073381342284","success":"true"}}}}]}}"#
        );
        let e = gw
            .parse_webhook(&Headers::new(), body.as_bytes(), 0)
            .unwrap();
        assert_eq!(
            (e.kind, e.provider_ref.as_deref()),
            (WebhookKind::PaymentSucceeded, Some("pay_1"))
        );
        assert_eq!(
            gw.parse_webhook(&Headers::new(), body.replace("1050", "1").as_bytes(), 0)
                .unwrap_err(),
            GatewayError::InvalidSignature
        );
    }

    #[test]
    fn escapes_colons_and_backslashes() {
        let i = serde_json::json!({"pspReference": "a:b", "merchantReference": "c\\d", "amount": {"value": 1, "currency": "EUR"}, "eventCode": "X", "success": "true"});
        assert_eq!(signing_string(&i), "a\\:b:::c\\\\d:1:EUR:X:true");
    }
}

mod manual {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("manual", c)
    }

    use pelty_addon_gateway_manual::manual::*;
    use pelty_core::money::{Currency, Money};
    use pelty_payments::*;
    use rust_decimal::Decimal;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test(flavor = "multi_thread")]
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

mod custom {
    fn factory(
        c: &pelty_payments::GatewayConfig,
    ) -> Result<Box<dyn pelty_payments::PaymentGateway>, pelty_payments::GatewayError> {
        crate::test_util::wasm_factory("custom", c)
    }

    use crate::test_util::*;
    use pelty_addon_gateway_custom::custom::*;
    use pelty_payments::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn conf() -> serde_json::Value {
        serde_json::json!({
            "name": "Acme Pay",
            "create": {
                "url": "https://api.acme.example/v1/pay?ref={{reference}}",
                "headers": {"Authorization": "Bearer {{secret.api_key}}"},
                "body": "{\"amount\":\"{{amount}}\",\"currency\":\"{{currency}}\",\"desc\":\"{{description}}\",\"order\":\"{{payment_id}}\"}",
                "redirect_url_path": "data.checkout_url",
                "ref_path": "data.id"
            },
            "webhook": {
                "signature_header": "X-Acme-Signature", "prefix": "sha256=",
                "timestamp_header": "X-Acme-Timestamp", "signed_payload": "{timestamp}.{body}",
                "event_id_path": "id", "event_type_path": "type", "ref_path": "data.id",
                "success_types": ["payment.paid"], "failed_types": ["payment.failed"]
            },
            "refund": {
                "url": "https://api.acme.example/v1/pay/{{provider_ref}}/refund",
                "body": "{\"amount_minor\":{{amount_minor}}}",
                "ref_path": "id", "status_path": "state", "success_values": ["done"]
            }
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn templated_create_webhook_refund() {
        let t = MockTransport::new();
        let mut reg = GatewayRegistry::with_transport(Arc::new(t.clone()));
        reg.register(descriptor(), factory);
        let c = GatewayConfig {
            config: conf(),
            secrets: [
                ("api_key".to_string(), "k1".to_string()),
                ("webhook_secret".to_string(), "whs".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        let gw = reg.build("custom-acme", &c).unwrap();
        t.push(
            200,
            r#"{"data":{"id":"A1","checkout_url":"https://pay.acme.example/A1"}}"#,
        );
        let mut rq = req("7", "EUR");
        rq.description = "VPS \"S\" & more".into();
        rq.reference = "INV 7/a".into();
        let init = gw.create_payment(&rq).await.unwrap();
        assert_eq!(
            init,
            PaymentInit::Redirect {
                url: "https://pay.acme.example/A1".into(),
                provider_ref: "A1".into()
            }
        );
        let s = &t.requests()[0];
        assert_eq!(s.url, "https://api.acme.example/v1/pay?ref=INV+7%2Fa");
        assert_eq!(s.header("authorization"), Some("Bearer k1"));
        let b: serde_json::Value = serde_json::from_str(&s.body).unwrap();
        assert_eq!(
            (b["amount"].as_str(), b["desc"].as_str()),
            (Some("7.00"), Some("VPS \"S\" & more"))
        );

        let body = r#"{"id":"ev1","type":"payment.paid","data":{"id":"A1"}}"#;
        // python3: hmac.new(b"whs", b"1700000000."+body, sha256).hexdigest()
        let real = "64f0f7aef9024e9f92d213a7d9221cb74403c245b556f8f5423c1dd150b1500b";
        let h = hdrs(&[
            ("x-acme-signature", &format!("sha256={real}")),
            ("x-acme-timestamp", "1700000000"),
        ]);
        let e = gw
            .parse_webhook(&h, body.as_bytes(), 1_700_000_001)
            .unwrap();
        assert_eq!(
            (e.kind, e.provider_ref.as_deref()),
            (WebhookKind::PaymentSucceeded, Some("A1"))
        );
        assert!(
            gw.parse_webhook(&h, body.as_bytes(), 1_700_001_000)
                .is_err(),
            "stale"
        );
        let bad = hdrs(&[
            ("x-acme-signature", real),
            ("x-acme-timestamp", "1700000000"),
        ]);
        assert!(
            gw.parse_webhook(&bad, body.as_bytes(), 1_700_000_001)
                .is_err(),
            "prefix required"
        );

        t.push(200, r#"{"id":"R1","state":"done"}"#);
        let r = gw.refund("A1", money("2", "EUR"), "k").await.unwrap();
        assert_eq!(
            r,
            RefundResult {
                provider_ref: Some("R1".into()),
                status: "succeeded".into()
            }
        );
        let last = t.requests().last().unwrap().clone();
        assert_eq!(
            (last.url.as_str(), last.body.as_str()),
            (
                "https://api.acme.example/v1/pay/A1/refund",
                r#"{"amount_minor":200}"#
            )
        );
    }

    // `validation_rejects_unsafe_configs` checks private parser internals and
    // stays a native unit test in pelty-payments.
}

mod paypal {
    use crate::test_util::*;
    use pelty_core::money::{Currency, Money};
    use pelty_payments::*;
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    const SANDBOX_BASE: &str = "https://api-m.sandbox.paypal.com";

    /// The native constructor's arguments, built through the addon factory.
    fn pp(
        cid: &str,
        sec: &str,
        wh: &str,
        pem: &str,
        live: bool,
        t: Arc<dyn HttpTransport>,
    ) -> Result<Box<dyn PaymentGateway>, GatewayError> {
        let c = GatewayConfig {
            id: "paypal".into(),
            config: serde_json::json!({ "live": live }),
            secrets: [
                ("client_id", cid),
                ("client_secret", sec),
                ("webhook_id", wh),
                ("webhook_public_key", pem),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            transport: t,
        };
        wasm_factory("paypal", &c)
    }

    // Offline test vector: key pair generated locally with openssl, NOT a
    // PayPal key. Signed message: "TX-1|2026-10-29T10:00:00Z|WH-ID-1|604322261".
    const PUB: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA6k6vIONntvAnjDOo9YHo
CacY6EXBL3JB9fUDHB+dxl2smxxmmheJighETcvUdeKL1p7JvRy7ksuqzuGbk5j9
Q3SU1k7uklFH6s20URd3AwgV4F9480X6twPtAkgLQGr8ebGVLT+ZswtGTKb+33wa
jKwTQKAq4K8cpMYBsOkU1zc8Kb+x2E2yNcbIMxAs+Q0EGHxxmTq7DEsBhmKwf2MW
HkvHk3N9s4Y5dA/0mBUuRLUHMuoxq8nzVomTkicK+a2fccNAlqYax66y4X0SrWsg
LMnhc67h+hqrhTQcTy8cApJF+Yeu/M37RRg/zmOSu5z5JkK8EgRzVNsgwO6eOpOE
rQIDAQAB
-----END PUBLIC KEY-----";
    const SIG: &str = "zTz+HRhrBYgqczHLaUIijQ0V8QqeV/T2EENWIQeQUeyvD2T9sM0xDBqnmyw0E9H0YyxEv8M0qmHhchW0im0i6xQEewkMD5VFBx6Fhqof04764gJt92C743T41XQHjUqRtdRYgE97BX/RhMezMxK9VkHbouaZPPE7XLgaQQcECTnH1uNHOaaiOH+vYp3HinznGb1mf7PnVXta4lAOlRbZhBtGkheHeO/pMYN2yJN2b+D9a/zRC/PGqupLfPsO+JQMtUfhQfBAI/kzwZIidp4VftKQJKjM0HHmcQ9yQHTExM7jNcWo5PcHnrK7DV+NX5AmiBcRC0qGtjBxi+qcLiYQ8w==";
    const BODY: &str = r#"{"id":"WH-TEST-1","event_type":"PAYMENT.CAPTURE.COMPLETED","resource":{"id":"CAP-1","supplementary_data":{"related_ids":{"order_id":"ORDER-1"}}}}"#;
    const NOW: i64 = 1793268000;

    fn gw() -> Box<dyn PaymentGateway> {
        pp("cid", "csec", "WH-ID-1", PUB, false, Arc::new(NoTransport)).unwrap()
    }
    fn hdrs(sig: &str, time: &str) -> Headers {
        [
            ("paypal-transmission-id", "TX-1"),
            ("paypal-transmission-time", time),
            ("paypal-transmission-sig", sig),
            ("paypal-auth-algo", "SHA256withRSA"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn known_vector_passes_and_parses() {
        let e = gw()
            .parse_webhook(
                &hdrs(SIG, "2026-10-29T10:00:00Z"),
                BODY.as_bytes(),
                NOW + 10,
            )
            .unwrap();
        assert_eq!(e.event_id, "WH-TEST-1");
        assert_eq!(e.kind, WebhookKind::PaymentSucceeded);
        assert_eq!(e.provider_ref.as_deref(), Some("ORDER-1"));
    }

    #[test]
    fn tampering_replay_and_wrong_webhook_id_rejected() {
        let g = gw();
        let h = hdrs(SIG, "2026-10-29T10:00:00Z");
        let tampered = BODY.replace("CAP-1", "CAP-2");
        assert_eq!(
            g.parse_webhook(&h, tampered.as_bytes(), NOW),
            Err(GatewayError::InvalidSignature)
        );
        assert_eq!(
            g.parse_webhook(&h, BODY.as_bytes(), NOW + 301),
            Err(GatewayError::InvalidSignature)
        );
        let other = pp("c", "s", "WH-OTHER", PUB, false, Arc::new(NoTransport)).unwrap();
        assert_eq!(
            other.parse_webhook(&h, BODY.as_bytes(), NOW),
            Err(GatewayError::InvalidSignature)
        );
        let mut bad_algo = h.clone();
        bad_algo.insert("paypal-auth-algo".into(), "SHA1withRSA".into());
        assert!(g.parse_webhook(&bad_algo, BODY.as_bytes(), NOW).is_err());
        let mut missing = h;
        missing.remove("paypal-transmission-sig");
        assert!(g.parse_webhook(&missing, BODY.as_bytes(), NOW).is_err());
    }

    #[test]
    fn invalid_key_is_config_error() {
        assert!(matches!(
            pp("c", "s", "w", "nope", false, Arc::new(NoTransport)),
            Err(GatewayError::Config(_))
        ));
    }

    fn g(t: &MockTransport) -> Box<dyn PaymentGateway> {
        pp("cid", "csec", "WH", tests_pub(), false, Arc::new(t.clone())).unwrap()
    }
    fn tests_pub() -> &'static str {
        PUB
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_capture_refund_flow() {
        let t = MockTransport::new();
        let tok = r#"{"access_token":"A21","expires_in":300}"#;
        t.push(200, tok).push(201, r#"{"id":"ORDER-9","status":"PAYER_ACTION_REQUIRED","links":[{"rel":"self","href":"x"},{"rel":"payer-action","href":"https://www.sandbox.paypal.com/checkoutnow?token=ORDER-9"}]}"#);
        let gw = g(&t);
        let init = gw.create_payment(&req("12.50", "EUR")).await.unwrap();
        assert_eq!(
            init,
            PaymentInit::Redirect {
                url: "https://www.sandbox.paypal.com/checkoutnow?token=ORDER-9".into(),
                provider_ref: "ORDER-9".into()
            }
        );

        t.push(200, tok).push(201, r#"{"id":"ORDER-9","status":"COMPLETED","purchase_units":[{"payments":{"captures":[{"id":"CAP-7","status":"COMPLETED"}]}}]}"#);
        assert_eq!(
            gw.complete_payment("ORDER-9", &HashMap::new())
                .await
                .unwrap(),
            CompleteStatus::Succeeded
        );

        // Retried capture: 422 → read the order.
        t.push(200, tok).push(422, r#"{"name":"UNPROCESSABLE_ENTITY","message":"ORDER_ALREADY_CAPTURED"}"#)
            .push(200, r#"{"id":"ORDER-9","status":"COMPLETED","purchase_units":[{"payments":{"captures":[{"id":"CAP-7","status":"COMPLETED"}]}}]}"#);
        assert_eq!(
            gw.complete_payment("ORDER-9", &HashMap::new())
                .await
                .unwrap(),
            CompleteStatus::Succeeded
        );

        t.push(200, tok).push(200, r#"{"id":"ORDER-9","purchase_units":[{"payments":{"captures":[{"id":"CAP-7","status":"COMPLETED"}]}}]}"#)
            .push(201, r#"{"id":"RF-1","status":"COMPLETED"}"#);
        let r = gw
            .refund("ORDER-9", money("2.5", "EUR"), "rk")
            .await
            .unwrap();
        assert_eq!(
            r,
            RefundResult {
                provider_ref: Some("RF-1".into()),
                status: "succeeded".into()
            }
        );

        let sent = t.requests();
        assert!(
            sent.iter().all(|r| r.url.starts_with(SANDBOX_BASE)),
            "never live"
        );
        assert_eq!(sent[0].header("authorization"), Some("Basic Y2lkOmNzZWM="));
        assert!(sent[3].url.ends_with("/v2/checkout/orders/ORDER-9/capture"));
        let last = sent.last().unwrap();
        assert!(last.url.ends_with("/v2/payments/captures/CAP-7/refund"));
        assert_eq!(last.header("paypal-request-id"), Some("rk"));
        let v: serde_json::Value = serde_json::from_str(&last.body).unwrap();
        assert_eq!(v["amount"]["value"], "2.50");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn declined_capture_is_failed() {
        let t = MockTransport::new();
        t.push(200, r#"{"access_token":"A"}"#).push(201, r#"{"status":"COMPLETED","purchase_units":[{"payments":{"captures":[{"id":"CAP-1","status":"DECLINED"}]}}]}"#);
        assert_eq!(
            g(&t).complete_payment("O", &HashMap::new()).await.unwrap(),
            CompleteStatus::Failed
        );
    }
}
