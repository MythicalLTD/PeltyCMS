//! Real end-to-end test: compiles `extensions/examples/wasm-captcha-rust` to a
//! wasm32-wasip2 component with cargo, packs it into an addon zip and runs it
//! through the sandbox. Requires `rustup target add wasm32-wasip2`.
use pelty_wasm::{CaptchaAddon, CaptchaRequest, Sandbox, WasmError};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn build_zip() -> &'static Vec<u8> {
    static ZIP: OnceLock<Vec<u8>> = OnceLock::new();
    ZIP.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extensions/examples/wasm-captcha-rust");
        let target = std::env::temp_dir().join("pelty-wasm-example-target");
        let st = Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2", "--target-dir"])
            .arg(&target)
            .current_dir(&root)
            .status()
            .expect("run cargo");
        assert!(st.success(), "guest build failed (is wasm32-wasip2 installed?)");
        let wasm = std::fs::read(target.join("wasm32-wasip2/release/pelty_addon_captcha_example.wasm")).unwrap();
        let dir = std::env::temp_dir().join(format!("pelty-wasm-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("addon.wasm"), wasm).unwrap();
        std::fs::write(
            dir.join("addon.json"),
            r#"{"id":"captcha-example","name":"Example captcha","version":"0.1.0","kind":"captcha","engine":"wasm","abi":"0.1.0","wasm":"addon.wasm","capabilities":{}}"#,
        )
        .unwrap();
        pelty_extensions::addon_pkg::rehash(&dir).unwrap();
        pelty_extensions::addon_pkg::pack(&dir).unwrap()
    })
}

fn req(token: &str, secret: &str) -> CaptchaRequest {
    CaptchaRequest { token: token.into(), remote_ip: Some("203.0.113.7".into()), secret: secret.into() }
}

#[test]
fn verifies_valid_and_invalid_tokens() {
    let a = CaptchaAddon::from_zip(build_zip(), Sandbox::default()).unwrap();
    assert_eq!(a.id, "captcha-example");
    let (ok, logs) = a.verify(&req("ok-s3cret", "s3cret")).unwrap();
    assert!(ok.success && ok.error_codes.is_empty());
    assert_eq!(logs.len(), 1);
    assert!(logs[0].1.contains("ok=true"));
    let (bad, _) = a.verify(&req("ok-wrong", "s3cret")).unwrap();
    assert!(!bad.success);
    assert_eq!(bad.error_codes, vec!["invalid-input-response".to_string()]);
}

#[test]
fn addon_error_is_returned() {
    let a = CaptchaAddon::from_zip(build_zip(), Sandbox::default()).unwrap();
    assert!(matches!(a.verify(&req("x", "")), Err(WasmError::Addon(m)) if m.contains("secret")));
}

#[test]
fn infinite_loop_is_stopped_by_fuel() {
    let a = CaptchaAddon::from_zip(build_zip(), Sandbox { fuel: 5_000_000, timeout: Duration::from_secs(30), ..Sandbox::default() }).unwrap();
    let t = Instant::now();
    assert!(matches!(a.verify(&req("loop", "s")), Err(WasmError::Trap(_))));
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[test]
fn infinite_loop_is_stopped_by_timeout() {
    let a = CaptchaAddon::from_zip(build_zip(), Sandbox { fuel: u64::MAX, timeout: Duration::from_millis(300), ..Sandbox::default() }).unwrap();
    let t = Instant::now();
    assert!(matches!(a.verify(&req("loop", "s")), Err(WasmError::Trap(_))));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn rejects_wrong_engine_and_abi() {
    let pkg = pelty_extensions::addon_pkg::inspect(build_zip(), Default::default()).unwrap();
    assert!(pelty_wasm::wasm_entry(&pkg, "gateway").is_err());
    let mut p2 = pkg;
    p2.manifest["abi"] = "1.0.0".into();
    assert!(pelty_wasm::wasm_entry(&p2, "captcha").is_err());
}

#[test]
fn truncated_component_rejected() {
    let zip = build_zip();
    let pkg = pelty_extensions::addon_pkg::inspect(zip, Default::default()).unwrap();
    let w = pkg.files.get("addon.wasm").unwrap();
    assert!(CaptchaAddon::from_component_bytes("t", &w[..100], Sandbox::default()).is_err());
}
