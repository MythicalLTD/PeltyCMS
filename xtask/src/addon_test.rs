//! Offline wire-contract tests using the production component runtime, not a second VM.
use crate::util::R;
use pelty_extensions::addon_pkg::{self, Limits};
use pelty_payments::{BoxFuture, HttpRequestSpec, HttpTransport};
use pelty_wasm::{CallCtx, HostServices, WasmAddon};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing_subscriber::prelude::*;

// The runtime returns logs only on success. Observe its tracing events too,
// so a guest cannot hide a logged secret by returning an error or trapping.
struct LogGuard {
    secrets: HashMap<String, String>,
    leaked: Arc<AtomicBool>,
}
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogGuard {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Visitor<'a>(&'a LogGuard);
        impl tracing::field::Visit for Visitor<'_> {
            fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let text = format!("{value:?}");
                if text.contains("[redacted]") || self.0.secrets.values().any(|s| text.contains(s))
                {
                    self.0.leaked.store(true, Ordering::Relaxed);
                }
            }
        }
        event.record(&mut Visitor(self));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    schema_version: u32,
    addon_id: String,
    kind: String,
    #[serde(default = "empty_object")]
    settings: Value,
    #[serde(default)]
    secrets: HashMap<String, String>,
    server_url: Option<String>,
    cases: Vec<Case>,
}
fn empty_object() -> Value {
    json!({})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    op: String,
    input: Value,
    expect: Expected,
    http: Vec<Exchange>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Expected {
    Value(Value),
    Error(String),
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    request: Request,
    response: Response,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    status: u16,
    body: String,
}

#[derive(Default)]
struct Tape {
    exchanges: Vec<Exchange>,
    calls: usize,
    failures: Vec<String>,
}
#[derive(Clone, Default)]
struct StrictHttp(Arc<Mutex<Tape>>);

fn headers(mut h: Vec<(String, String)>) -> Vec<(String, String)> {
    for (name, _) in &mut h {
        name.make_ascii_lowercase();
    }
    h.sort();
    h
}
impl HttpTransport for StrictHttp {
    fn send(&self, req: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        let mut tape = self.0.lock().unwrap();
        let index = tape.calls;
        tape.calls += 1;
        let result = if let Some(e) = tape.exchanges.get(index).cloned() {
            let mut fields = Vec::new();
            if req.method != e.request.method {
                fields.push("method");
            }
            if req.url != e.request.url {
                fields.push("url");
            }
            if headers(req.headers) != headers(e.request.headers) {
                fields.push("headers");
            }
            if req.body != e.request.body {
                fields.push("body");
            }
            if fields.is_empty() {
                Ok((e.response.status, e.response.body))
            } else {
                tape.failures
                    .push(format!("http[{index}] mismatch: {}", fields.join(", ")));
                Err("fixture request mismatch".into())
            }
        } else {
            tape.failures.push(format!("http[{index}] unexpected call"));
            Err("unexpected fixture request".into())
        };
        Box::pin(std::future::ready(result))
    }
}

fn read(path: &Path, max: usize) -> R<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "cannot open input")?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read input")?;
    if bytes.len() > max {
        return Err("input exceeds harness size limit".into());
    }
    Ok(bytes)
}

fn execute(package: &Path, fixture: &Path) -> R<Value> {
    let suite: Suite = serde_json::from_slice(&read(fixture, 4 * 1024 * 1024)?)
        .map_err(|_| "invalid fixture JSON or schema")?;
    if suite.schema_version != 1 || suite.cases.is_empty() || !suite.settings.is_object() {
        return Err("expected schema_version 1, object settings and nonempty cases".into());
    }
    if suite.secrets.values().any(|s| s.len() < 6) {
        return Err("use synthetic secret sentinels of at least six bytes".into());
    }
    let bytes = read(package, 64 * 1024 * 1024)?;
    let pkg = addon_pkg::inspect(&bytes, Limits::default()).map_err(|_| "invalid addon package")?;
    if pkg.id != suite.addon_id || pkg.manifest["kind"] != suite.kind {
        return Err("fixture addon identity/kind does not match package".into());
    }
    if !["gateway", "provisioning"].contains(&suite.kind.as_str()) {
        return Err("harness supports gateway and provisioning wire calls only".into());
    }
    // Inspect BEFORE instantiate/describe: no filesystem, process, jobs or DNS.
    if let Some(caps) = pkg.manifest.get("capabilities") {
        let caps = caps.as_object().ok_or("invalid capabilities")?;
        if caps
            .keys()
            .any(|k| !["http", "secrets"].contains(&k.as_str()))
        {
            return Err("offline harness permits only http and secrets capabilities".into());
        }
        if caps
            .get("http")
            .and_then(Value::as_array)
            .is_some_and(|h| h.iter().any(|v| v == "*"))
        {
            return Err("offline harness refuses wildcard HTTP (runtime may resolve DNS)".into());
        }
    }
    let http = StrictHttp::default();
    let services = HostServices {
        http: Arc::new(http.clone()),
        ..Default::default()
    };
    let leaked = Arc::new(AtomicBool::new(false));
    let subscriber = tracing_subscriber::registry().with(LogGuard {
        secrets: suite.secrets.clone(),
        leaked: leaked.clone(),
    });
    let _guard = tracing::subscriber::set_default(subscriber);
    let loaded = WasmAddon::from_package(&pkg, services);
    if leaked.load(Ordering::Relaxed) {
        return Err("secret logging detected during describe".into());
    }
    let addon = loaded.map_err(|_| "component load/describe failed")?;
    if http.0.lock().unwrap().calls != 0 {
        return Err("describe attempted HTTP".into());
    }
    let ctx = CallCtx {
        settings: suite.settings,
        secrets: suite.secrets.clone(),
        server_url: suite.server_url,
        http: Some(Arc::new(http.clone())),
        ..Default::default()
    };
    let mut cases = Vec::new();
    for (index, case) in suite.cases.into_iter().enumerate() {
        if case.name.is_empty() || case.op.is_empty() {
            return Err("case name/op must be nonempty".into());
        }
        let expected_calls = case.http.len();
        *http.0.lock().unwrap() = Tape {
            exchanges: case.http,
            ..Default::default()
        };
        leaked.store(false, Ordering::Relaxed);
        let result = addon.call(&ctx, &case.op, &case.input);
        let mut failures = Vec::new();
        match (&case.expect, result) {
            (Expected::Value(expected), Ok(out)) => {
                if !logs_safe(&out.logs, &suite.secrets) {
                    failures.push("secret logging detected".into());
                }
                match serde_json::from_str::<Value>(&out.value) {
                    Ok(actual) if &actual == expected => (),
                    _ => failures.push("output JSON mismatch".into()),
                }
            }
            (Expected::Error(expected), Err(pelty_wasm::WasmError::Addon(actual)))
                if &actual == expected => {}
            (_, Ok(out)) => {
                if !logs_safe(&out.logs, &suite.secrets) {
                    failures.push("secret logging detected".into());
                }
                failures.push("expected addon error, got success".into());
            }
            _ => failures.push("unexpected addon error or runtime trap".into()),
        }
        if leaked.load(Ordering::Relaxed)
            && !failures.iter().any(|f| f == "secret logging detected")
        {
            failures.push("secret logging detected".into());
        }
        let tape = http.0.lock().unwrap();
        failures.extend(tape.failures.clone());
        if tape.calls < expected_calls {
            failures.push(format!(
                "{} unconsumed HTTP exchanges",
                expected_calls - tape.calls
            ));
        }
        // Do not echo fixture names, inputs, logs, outputs or provider errors: they can contain credentials.
        cases.push(
            json!({"index": index, "passed": failures.is_empty(), "http_calls": tape.calls,
            "expected_http_calls": expected_calls, "failures": failures}),
        );
    }
    let passed = cases.iter().all(|c| c["passed"] == true);
    Ok(
        json!({"schema_version": 1, "passed": passed, "package_sha256": addon_pkg::sha256_hex(&bytes),
        "runtime_abi": pelty_wasm::runtime::ABI, "cases": cases,
        "limitations": ["wire calls, not application workflows", "log checks observe runtime-redacted/truncated host logs; encoded or fragmented secrets are not detected", "no provider certification or signature trust decision"]}),
    )
}
fn logs_safe(logs: &[(String, String)], secrets: &HashMap<String, String>) -> bool {
    logs.iter()
        .all(|(_, msg)| !msg.contains("[redacted]") && !secrets.values().any(|s| msg.contains(s)))
}

pub fn run(args: &[String]) -> R {
    let (package, fixture, report) = match args {
        [p, f] => (p, f, None),
        [p, f, flag, r] if flag == "--report" => (p, f, Some(r)),
        _ => {
            return Err(
                "usage: addon-test <package.zip> <fixture.json> [--report report.json]".into(),
            )
        }
    };
    let report_value = execute(Path::new(package), Path::new(fixture)).unwrap_or_else(
        |e| json!({"schema_version": 1, "passed": false, "setup_error": e, "cases": []}),
    );
    let text =
        serde_json::to_string_pretty(&report_value).map_err(|_| "cannot serialize report")?;
    println!("{text}");
    if let Some(path) = report {
        std::fs::write(path, &text).map_err(|_| "cannot write report")?;
    }
    if report_value["passed"] == true {
        Ok(())
    } else {
        Err("addon conformance failed (see JSON report)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shipped_fixtures_have_strict_schema() {
        for bytes in [
            include_str!("../tests/fixtures/addons/stripe.json"),
            include_str!("../tests/fixtures/addons/digitalocean.json"),
        ] {
            let suite: Suite = serde_json::from_str(bytes).unwrap();
            assert_eq!(suite.schema_version, 1);
            assert!(!suite.cases.is_empty());
        }
        assert!(
            serde_json::from_str::<Response>(r#"{"status":200,"body":"","typo":true}"#).is_err()
        );
    }
    #[test]
    fn tracing_observer_catches_logs_without_call_output() {
        let leaked = Arc::new(AtomicBool::new(false));
        let subscriber = tracing_subscriber::registry().with(LogGuard {
            secrets: HashMap::from([("key".into(), "synthetic-secret".into())]),
            leaked: leaked.clone(),
        });
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("safe message");
            assert!(!leaked.load(Ordering::Relaxed));
            tracing::info!("token=[redacted]");
            assert!(leaked.swap(false, Ordering::Relaxed));
            tracing::error!("synthetic-secret");
            assert!(leaked.load(Ordering::Relaxed));
        });
    }
    fn req() -> HttpRequestSpec {
        HttpRequestSpec {
            method: "POST",
            url: "https://fixture.test/x".into(),
            headers: vec![("Authorization".into(), "Bearer synthetic".into())],
            body: "{}".into(),
        }
    }
    fn exchange() -> Exchange {
        Exchange {
            request: Request {
                method: "POST".into(),
                url: req().url,
                headers: req().headers,
                body: "{}".into(),
            },
            response: Response {
                status: 201,
                body: "{}".into(),
            },
        }
    }
    fn send(t: &StrictHttp, req: HttpRequestSpec) -> Result<(u16, String), String> {
        let mut f = t.send(req);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(r) => r,
            _ => panic!("transport must be ready"),
        }
    }
    #[test]
    fn strict_transport_never_falls_back_and_keeps_failures() {
        let t = StrictHttp::default();
        t.0.lock().unwrap().exchanges.push(exchange());
        assert_eq!(send(&t, req()).unwrap().0, 201);
        assert!(send(&t, req()).is_err());
        assert_eq!(t.0.lock().unwrap().failures.len(), 1);
    }
    #[test]
    fn request_matching_is_exact_except_header_order_and_case() {
        let t = StrictHttp::default();
        t.0.lock().unwrap().exchanges = vec![exchange(), exchange(), exchange(), exchange()];
        let mut r = req();
        r.headers[0].0 = "authorization".into();
        assert!(send(&t, r).is_ok());
        let mut r = req();
        r.body = "{ }".into();
        assert!(send(&t, r).is_err());
        let mut r = req();
        r.url.push_str("?extra=1");
        assert!(send(&t, r).is_err());
        let mut r = req();
        r.headers.push(("extra".into(), "x".into()));
        assert!(send(&t, r).is_err());
        assert_eq!(t.0.lock().unwrap().failures.len(), 3);
    }
    #[test]
    fn reject_secret_or_runtime_redaction_without_echoing_it() {
        let secrets = HashMap::from([("key".into(), "synthetic-secret".into())]);
        assert!(!logs_safe(
            &[("info".into(), "synthetic-secret".into())],
            &secrets
        ));
        assert!(!logs_safe(
            &[("info".into(), "token=[redacted]".into())],
            &secrets
        ));
        assert!(logs_safe(
            &[("info".into(), "created resource".into())],
            &secrets
        ));
    }
}
