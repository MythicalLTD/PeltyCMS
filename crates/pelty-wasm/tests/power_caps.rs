//! Opt-in capabilities, end to end through a real WASM addon
//! (`extensions/addons/fixture-power`): each power works when granted and is
//! refused without its capability.
mod common;
use pelty_payments::{BoxFuture, HttpRequestSpec, HttpTransport};
use pelty_wasm::{AddonCapabilities, CallCtx, HostServices, PowerHost, Sandbox, WasmAddon};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecPower(Mutex<Vec<String>>);
impl PowerHost for RecPower {
    fn db_read<'a>(
        &'a self,
        addon: &'a str,
        entity: &'a str,
        _q: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        self.0
            .lock()
            .unwrap()
            .push(format!("read {addon} {entity}"));
        Box::pin(async { Ok(json!([{ "id": "c1", "email": "a@b.test" }])) })
    }
    fn db_write<'a>(
        &'a self,
        _a: &'a str,
        op: &'a str,
        _i: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        self.0.lock().unwrap().push(format!("write {op}"));
        Box::pin(async { Ok(json!({ "ok": true })) })
    }
    fn sql<'a>(
        &'a self,
        _a: &'a str,
        st: &'a str,
        _p: Vec<Value>,
    ) -> BoxFuture<'a, Result<Value, String>> {
        self.0.lock().unwrap().push(format!("sql {st}"));
        Box::pin(async { Ok(json!([{ "n": 1 }])) })
    }
    fn enqueue<'a>(
        &'a self,
        _a: &'a str,
        op: &'a str,
        _i: Value,
        _d: u32,
    ) -> BoxFuture<'a, Result<(), String>> {
        self.0.lock().unwrap().push(format!("job {op}"));
        Box::pin(async { Ok(()) })
    }
}

struct Ok200;
impl HttpTransport for Ok200 {
    fn send(&self, _r: HttpRequestSpec) -> BoxFuture<'_, Result<(u16, String), String>> {
        Box::pin(async { Ok((200, String::new())) })
    }
}

fn manifest() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(common::addons_root().join("fixture-power/addon.json")).unwrap(),
    )
    .unwrap()
}

fn wasm() -> Vec<u8> {
    let zip = common::addon_zip("fixture-power", "pelty-addon-fixture-power");
    let pkg = pelty_extensions::addon_pkg::inspect(&zip, Default::default()).unwrap();
    pkg.files["addon.wasm"].clone()
}

fn load(caps: AddonCapabilities, power: Arc<RecPower>) -> WasmAddon {
    let services = HostServices {
        http: Arc::new(Ok200),
        power,
        ..Default::default()
    };
    WasmAddon::from_bytes(
        "integration-powerdemo",
        "integration",
        "0.1.0",
        &wasm(),
        caps,
        manifest(),
        Sandbox::for_kind("integration"),
        services,
    )
    .unwrap()
}

fn try_(a: &WasmAddon, v: Value) -> Value {
    a.call_json(&CallCtx::default(), "try", &v).unwrap()
}

fn full_caps() -> AddonCapabilities {
    serde_json::from_value(manifest()["capabilities"].clone()).unwrap()
}

#[test]
fn granted_powers_work() {
    let dir = std::env::temp_dir().join(format!("pelty-addon-data-{}", std::process::id()));
    std::env::set_var("PELTY_ADDON_DATA_DIR", &dir);
    let rec = Arc::new(RecPower::default());
    let a = load(full_caps(), rec.clone());
    assert_eq!(
        try_(
            &a,
            json!({"power": "db_read", "entity": "clients", "query": {}})
        )["ok"][0]["id"],
        "c1"
    );
    assert_eq!(
        try_(
            &a,
            json!({"power": "db_write", "op": "client.note", "input": {"text": "x"}})
        )["ok"]["ok"],
        true
    );
    assert_eq!(
        try_(
            &a,
            json!({"power": "sql", "statement": "SELECT 1 AS n", "params": []})
        )["ok"][0]["n"],
        1
    );
    assert_eq!(try_(&a, json!({"power": "job", "op": "tick"}))["ok"], true);
    assert_eq!(
        *rec.0.lock().unwrap(),
        vec![
            "read integration-powerdemo clients",
            "write client.note",
            "sql SELECT 1 AS n",
            "job tick"
        ]
    );
    // Private data dir at /data, on the host under <root>/<id>.
    assert_eq!(
        try_(&a, json!({"power": "fs", "path": "/data/note.txt"}))["ok"],
        "hello"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("integration-powerdemo/note.txt")).unwrap(),
        "hello"
    );
    // ...and nothing outside it.
    for p in ["/etc/passwd", "/data/../escape.txt", "/tmp/x"] {
        assert!(
            try_(&a, json!({"power": "fs", "path": p}))
                .get("err")
                .is_some(),
            "{p}"
        );
    }
    assert!(!dir.join("escape.txt").exists());
    // exec: allowlisted program only, no shell.
    if cfg!(unix) && !pelty_wasm::in_container() {
        let r = try_(
            &a,
            json!({"power": "exec", "program": "/bin/echo", "args": ["hi", "$HOME"]}),
        );
        assert_eq!(r["ok"]["stdout"], "hi $HOME\n", "{r}");
    }
    let r = try_(
        &a,
        json!({"power": "exec", "program": "/bin/sh", "args": ["-c", "id"]}),
    );
    assert!(
        r["err"]
            .as_str()
            .unwrap()
            .contains("capability not granted"),
        "{r}"
    );
    // Scopes: an entity / op not listed is refused.
    assert!(try_(
        &a,
        json!({"power": "db_read", "entity": "tickets", "query": {}})
    )["err"]
        .as_str()
        .unwrap()
        .contains("db.read:tickets"));
    assert!(try_(
        &a,
        json!({"power": "db_write", "op": "invoice.delete", "input": {}})
    )["err"]
        .as_str()
        .unwrap()
        .contains("not granted"));
    assert!(try_(&a, json!({"power": "job", "op": "other"}))["err"]
        .as_str()
        .unwrap()
        .contains("not granted"));
    // http "*": public hosts yes, private/loopback/metadata no (http_private off).
    assert_eq!(
        try_(
            &a,
            json!({"power": "fetch", "url": "https://93.184.215.14/"})
        )["ok"],
        200
    );
    for u in [
        "http://127.0.0.1:8080/",
        "http://10.0.0.5/",
        "http://[::1]/",
        "http://localhost/",
        "http://169.254.169.254/latest/meta-data",
        "http://192.168.1.1/",
    ] {
        assert!(
            try_(&a, json!({"power": "fetch", "url": u}))["err"]
                .as_str()
                .unwrap()
                .contains("private"),
            "{u}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_private_is_a_separate_opt_in() {
    let mut caps = full_caps();
    caps.http_private = true;
    let a = load(caps, Default::default());
    assert_eq!(
        try_(&a, json!({"power": "fetch", "url": "http://10.0.0.5/"}))["ok"],
        200
    );
}

#[test]
fn without_capabilities_everything_is_refused() {
    let rec = Arc::new(RecPower::default());
    let a = load(AddonCapabilities::default(), rec.clone());
    for (v, cap) in [
        (
            json!({"power": "db_read", "entity": "clients", "query": {}}),
            "db.read",
        ),
        (
            json!({"power": "db_write", "op": "client.note", "input": {}}),
            "db.write",
        ),
        (
            json!({"power": "sql", "statement": "SELECT 1", "params": []}),
            "db.sql",
        ),
        (
            json!({"power": "exec", "program": "/bin/echo", "args": []}),
            "exec",
        ),
        (json!({"power": "job", "op": "tick"}), "jobs"),
    ] {
        let r = try_(&a, v);
        assert!(
            r["err"]
                .as_str()
                .unwrap()
                .contains(&format!("capability not granted: {cap}")),
            "{r}"
        );
    }
    assert!(try_(&a, json!({"power": "fs", "path": "/data/x"}))
        .get("err")
        .is_some());
    assert!(try_(
        &a,
        json!({"power": "fetch", "url": "https://93.184.215.14/"})
    )["err"]
        .as_str()
        .unwrap()
        .contains("not allowed"));
    assert!(
        rec.0.lock().unwrap().is_empty(),
        "host services never reached"
    );
}

#[test]
fn limits_from_manifest() {
    struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let keys = [
        "PELTY_ADDON_MAX_MEMORY_MIB",
        "PELTY_ADDON_MAX_TIMEOUT_SECS",
        "PELTY_ADDON_MAX_FUEL",
    ];
    let _restore = RestoreEnv(
        keys.iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect(),
    );
    for key in keys {
        std::env::remove_var(key);
    }
    let sb = Sandbox::from_manifest("integration", manifest().get("limits")).unwrap();
    assert_eq!(
        (sb.max_memory_bytes, sb.timeout.as_secs()),
        (128 * 1024 * 1024, 10)
    );
    assert_eq!(
        Sandbox::from_manifest(
            "integration",
            Some(&json!({"memory_mib": 512, "timeout_secs": 120}))
        )
        .unwrap()
        .timeout
        .as_secs(),
        120
    );
    for bad in [
        json!({"memory_mib": 513}),
        json!({"timeout_secs": 121}),
        json!({"fuel": 0}),
        json!({"cpu": 1}),
        json!({"memory_mib": "1"}),
    ] {
        assert!(
            Sandbox::from_manifest("integration", Some(&bad)).is_err(),
            "{bad}"
        );
    }
    // Admin maximum lowers the ceiling.
    std::env::set_var("PELTY_ADDON_MAX_TIMEOUT_SECS", "30");
    assert!(Sandbox::from_manifest("integration", Some(&json!({"timeout_secs": 31}))).is_err());
    std::env::set_var("PELTY_ADDON_MAX_MEMORY_MIB", "32");
    std::env::set_var("PELTY_ADDON_MAX_FUEL", "1");
    for limits in [
        None,
        Some(Value::Null),
        Some(json!({})),
        Some(json!({"fuel": 1})),
    ] {
        let sb = Sandbox::from_manifest("integration", limits.as_ref()).unwrap();
        assert_eq!(
            (sb.max_memory_bytes, sb.timeout.as_secs(), sb.fuel),
            (32 * 1024 * 1024, 30, 1),
            "omitted fields must respect admin ceilings"
        );
    }
    std::env::set_var("PELTY_ADDON_MAX_TIMEOUT_SECS", "9999");
    assert!(
        Sandbox::from_manifest("integration", Some(&json!({"timeout_secs": 121}))).is_err(),
        "hard cap 120 s"
    );
    assert_eq!(
        Sandbox::from_manifest("integration", None).unwrap().timeout,
        Sandbox::for_kind("integration").timeout,
        "higher ceilings must not raise defaults"
    );
}

#[test]
fn dangerous_list() {
    let d = full_caps().dangerous();
    for k in [
        "http.any",
        "money.modify",
        "db.read",
        "db.write",
        "db.sql",
        "fs",
        "exec",
        "auth.provider",
        "hooks.any",
    ] {
        assert!(d.contains(&k), "{k}");
    }
    assert!(!d.contains(&"http.private"));
    assert!(AddonCapabilities::default().dangerous().is_empty());
}
