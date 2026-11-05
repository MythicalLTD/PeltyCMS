//! Per-call overhead of a WASM addon (store + instantiate + one export).
//! Ignored by default: `cargo test -p pelty-wasm --release --test call_latency -- --ignored --nocapture`.
mod common;
use pelty_wasm::{CallCtx, HostServices, WasmAddon};
use serde_json::json;

#[test]
#[ignore]
fn call_latency() {
    let zip = common::addon_zip("fixture-power", "pelty-addon-fixture-power");
    let a = WasmAddon::from_zip(&zip, Default::default(), HostServices::default()).unwrap();
    let ctx = CallCtx::default();
    for _ in 0..50 {
        let _ = a.on_event(&ctx, "bench.noop", &json!({}));
    }
    let n = 2000;
    let mut v: Vec<u128> = Vec::with_capacity(n);
    for _ in 0..n {
        let t = std::time::Instant::now();
        let _ = a.on_event(&ctx, "bench.noop", &json!({}));
        v.push(t.elapsed().as_micros());
    }
    v.sort_unstable();
    let mean = v.iter().sum::<u128>() as f64 / n as f64;
    println!(
        "CALL_LATENCY n={n} mean={mean:.1}us p50={}us p95={}us p99={}us",
        v[n / 2],
        v[n * 95 / 100],
        v[n * 99 / 100]
    );
}
