//! Recovery and capability boundaries using the existing real components.
mod common;

use pelty_payments::{BoxFuture, HttpRequestSpec, HttpTransport, MockTransport};
use pelty_wasm::{AddonCapabilities, CallCtx, HostServices, Sandbox, WasmAddon, WasmError};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

fn power(services: HostServices) -> WasmAddon {
    let zip = common::addon_zip("fixture-power", "pelty-addon-fixture-power");
    let pkg = pelty_extensions::addon_pkg::inspect(&zip, Default::default()).unwrap();
    WasmAddon::from_bytes(
        &pkg.id,
        "integration",
        "0.1.0",
        &pkg.files["addon.wasm"],
        AddonCapabilities::default(),
        pkg.manifest.clone(),
        Sandbox::for_kind("integration"),
        services,
    )
    .unwrap()
}

fn healthy(a: &WasmAddon) {
    assert_eq!(
        a.call_json(&CallCtx::default(), "tick", &json!({}))
            .unwrap(),
        json!({"ticked": true})
    );
}

#[test]
fn trap_fuel_and_memory_failures_do_not_poison_later_calls() {
    let mut a = power(HostServices::default());
    let other = power(HostServices::default());
    // The fixture deliberately indexes a nonexistent line on its bad-input path.
    let bad = json!({"payload": {"query": "pd=1&bad=1", "body": {"items": []}}});
    assert!(matches!(
        a.call(&CallCtx::default(), "hook", &bad),
        Err(WasmError::Trap(_))
    ));
    healthy(&a);
    healthy(&other);

    let ctx = CallCtx {
        fuel: Some(0),
        ..Default::default()
    };
    let err = a.call(&ctx, "tick", &json!({})).unwrap_err();
    assert!(matches!(err, WasmError::Trap(_)), "{err}");
    assert!(err.to_string().contains("fuel"), "{err}");
    healthy(&a);
    healthy(&other);

    let original = a.sandbox;
    // Fail during instantiation: even the component's initial memory is refused.
    a.sandbox.max_memory_bytes = 1;
    assert!(matches!(
        a.call(&CallCtx::default(), "tick", &json!({})),
        Err(WasmError::Trap(_))
    ));
    a.sandbox = original;
    healthy(&a);
    healthy(&other);
}

#[test]
fn unknown_and_malformed_capabilities_fail_before_compilation() {
    let zip = common::addon_zip("fixture-power", "pelty-addon-fixture-power");
    let mut pkg = pelty_extensions::addon_pkg::inspect(&zip, Default::default()).unwrap();
    for bad in [
        json!({"http_privte": true}),
        json!({"http": true}),
        json!({"db": {"sqli": true}}),
        json!({"money": {"modfy": true}}),
        json!([]),
        json!([[]]),
        json!(false),
        json!(1),
        json!("capabilities"),
    ] {
        pkg.manifest["capabilities"] = bad.clone();
        let result = WasmAddon::from_package(&pkg, HostServices::default());
        assert!(
            matches!(result, Err(WasmError::Manifest(ref e)) if e.contains("capabilities")),
            "{bad}"
        );
    }
    // Older manifests may omit capabilities or explicitly use null.
    for value in [Value::Null, json!({})] {
        pkg.manifest["capabilities"] = value;
        assert!(WasmAddon::from_package(&pkg, HostServices::default()).is_ok());
    }
    pkg.manifest.as_object_mut().unwrap().remove("capabilities");
    assert!(WasmAddon::from_package(&pkg, HostServices::default()).is_ok());
}

#[test]
fn secrets_require_explicit_grants_even_when_present_in_context() {
    let zip = common::addon_zip("stripe", "pelty-addon-gateway-stripe");
    let mut a = WasmAddon::from_zip(&zip, Default::default(), HostServices::default()).unwrap();
    let ctx = CallCtx {
        secrets: [
            ("secret_key".into(), "sk_test_secret".into()),
            ("webhook_secret".into(), "whsec_test_secret".into()),
        ]
        .into(),
        ..Default::default()
    };
    let input =
        json!({"id": "stripe", "config": {}, "secret_names": ["secret_key", "webhook_secret"]});
    for allowed in [
        vec![],
        vec!["secret_key".to_string()],
        vec!["unrelated".to_string()],
    ] {
        a.caps.secrets = allowed;
        assert!(a.call_json(&ctx, "validate", &input).is_err());
    }
    a.caps.secrets = vec!["secret_key".into(), "webhook_secret".into()];
    assert!(a.call_json(&ctx, "validate", &input).is_ok());
    a.caps.secrets = vec!["*".into()];
    assert!(a.call_json(&ctx, "validate", &input).is_ok());
}

#[test]
fn explicit_network_grants_match_only_declared_hosts() {
    let transport = MockTransport::new();
    let mut a = power(HostServices {
        http: Arc::new(transport.clone()),
        ..Default::default()
    });
    a.caps.http = vec![
        "api.test".into(),
        "*.example.test".into(),
        "$server".into(),
        "$setting:panel".into(),
    ];
    let ctx = CallCtx {
        server_url: Some("https://server.test/base".into()),
        settings: json!({"panel": "https://panel.test/api"}),
        ..Default::default()
    };
    for url in [
        "https://api.test/",
        "https://one.example.test/",
        "https://server.test/",
        "https://panel.test/",
    ] {
        transport.push(200, "ok");
        assert_eq!(
            a.call_json(&ctx, "try", &json!({"power": "fetch", "url": url}))
                .unwrap()["ok"],
            200,
            "{url}"
        );
    }
    for url in [
        "https://other.test/",
        "https://api.test.attacker.test/",
        "https://example.test/",
        "https://notexample.test/",
        "file://api.test/private",
    ] {
        assert!(
            a.call_json(&ctx, "try", &json!({"power": "fetch", "url": url}))
                .unwrap()["err"]
                .is_string(),
            "{url}"
        );
    }
    assert_eq!(
        transport.requests().len(),
        4,
        "denied requests never reach transport"
    );
}

struct PendingHttp(Arc<AtomicBool>);
struct Cancelled(Arc<AtomicBool>);
impl Drop for Cancelled {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl HttpTransport for PendingHttp {
    fn send(&self, _: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        let guard = Cancelled(self.0.clone());
        Box::pin(async move {
            let _guard = guard;
            std::future::pending().await
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn host_network_wait_obeys_call_deadline_and_recovers() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut a = power(HostServices {
        http: Arc::new(PendingHttp(cancelled.clone())),
        ..Default::default()
    });
    // No grant: the transport is never reached, even with a configured URL.
    let request = json!({"power": "fetch", "url": "https://allowed.test/"});
    assert!(
        a.call_json(&CallCtx::default(), "try", &request).unwrap()["err"]
            .as_str()
            .unwrap()
            .contains("not allowed")
    );
    assert!(!cancelled.load(Ordering::SeqCst));
    a.caps.http = vec!["allowed.test".into()];
    let a = Arc::new(a);
    let start = Instant::now();
    let result = a
        .call_async(
            CallCtx {
                timeout: Some(Duration::from_millis(100)),
                ..Default::default()
            },
            "try".into(),
            request,
        )
        .await;
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "host wait escaped deadline"
    );
    assert!(
        cancelled.load(Ordering::SeqCst),
        "timed-out transport future was not dropped"
    );
    match result {
        Err(WasmError::Trap(_)) => {}
        Ok(v) => assert!(v["err"].is_string(), "{v}"),
        other => panic!("unexpected result: {other:?}"),
    }
    healthy(&a);
}

#[test]
fn cpu_deadline_interrupts_spin_and_recovers() {
    let zip = common::addon_zip("fixture-hooks", "pelty-addon-fixture-hooks");
    let a = WasmAddon::from_zip(&zip, Default::default(), HostServices::default()).unwrap();
    let ctx = CallCtx {
        timeout: Some(Duration::from_millis(10)),
        ..Default::default()
    };
    let start = Instant::now();
    let err = a
        .call(
            &ctx,
            "hook",
            &json!({"event": "client.before_update", "payload": {"body": {"city": "Spin"}}}),
        )
        .unwrap_err();
    assert!(matches!(err, WasmError::Trap(_)), "{err}");
    assert!(err.to_string().contains("timed out"), "{err}");
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(a
        .call_json(
            &CallCtx::default(),
            "hook",
            &json!({"event": "client.before_update", "payload": {}})
        )
        .is_ok());
}
