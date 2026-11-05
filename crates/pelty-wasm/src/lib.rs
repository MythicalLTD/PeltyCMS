//! Sandboxed WebAssembly addons (Component Model / WIT).
//!
//! An addon with `"engine": "wasm"` ships a `.wasm` component (any language
//! that targets the component model: Rust, C, C#, Go, Python, JS…). The host
//! runs it in wasmtime with a fuel budget, a memory cap, a wall-clock
//! deadline (epoch interruption) and a WASI context that grants nothing:
//! no filesystem, no sockets, no env vars, no inherited stdio. Opt-in
//! capabilities (approved by the admin) add a private data dir, database
//! access, exec and more; see `runtime::Capabilities`.
use pelty_extensions::addon_pkg::{self, Limits, Package};
use std::time::Duration;
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

pub mod gateway;
pub mod provisioning;
pub mod runtime;
pub use runtime::{
    abi_supported, data_dir, in_container, is_custom_event, is_private_ip, is_request_pattern,
    split_phase, CallCtx, Capabilities as AddonCapabilities, CronJob, DbCaps, EventBus, Filter,
    GuardedHttp, Hook, HostServices, Job, KvStore, MemoryKv, Mixin, MoneyCaps, NoBus, NoPower,
    PowerHost, Route, WasmAddon, PHASES, PRIORITY_MONITOR,
};

pub mod captcha_bindings {
    wasmtime::component::bindgen!({ path: "wit/captcha.wit", world: "captcha-addon" });
}
use captcha_bindings::exports::pelty::addon::captcha::{VerifyRequest, VerifyResult};
use captcha_bindings::pelty::addon::host::{self, Level};

/// WIT ABI version implemented by this host.
pub const ABI_VERSION: &str = "0.1.0";

#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("package: {0}")]
    Package(#[from] addon_pkg::PkgError),
    #[error("manifest: {0}")]
    Manifest(String),
    #[error("load: {0}")]
    Load(String),
    #[error("trap: {0}")]
    Trap(String),
    #[error("addon error: {0}")]
    Addon(String),
}

#[derive(Debug, Clone, Copy)]
pub struct Sandbox {
    pub fuel: u64,
    pub max_memory_bytes: usize,
    pub timeout: Duration,
}

impl Sandbox {
    /// Limits per addon kind: network-bound kinds get a longer deadline and
    /// more fuel (JSON parsing of provider responses), compute stays capped.
    pub fn for_kind(kind: &str) -> Self {
        match kind {
            "captcha" | "sso" => Sandbox {
                fuel: 2_000_000_000,
                max_memory_bytes: 64 * 1024 * 1024,
                timeout: Duration::from_secs(20),
            },
            _ => Sandbox {
                fuel: 10_000_000_000,
                max_memory_bytes: 64 * 1024 * 1024,
                timeout: Duration::from_secs(90),
            },
        }
    }
}

/// Hard ceilings for per-addon `limits` (the admin can lower them with
/// `PELTY_ADDON_MAX_MEMORY_MIB`, `PELTY_ADDON_MAX_TIMEOUT_SECS`,
/// `PELTY_ADDON_MAX_FUEL`, never raise them above these).
pub const LIMIT_MEMORY_MIB: u64 = 512;
pub const LIMIT_TIMEOUT_SECS: u64 = 120;
pub const LIMIT_FUEL: u64 = 200_000_000_000;

fn env_max(key: &str, hard: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v.min(hard))
        .unwrap_or(hard)
}

/// Admin maxima currently in force: (memory MiB, timeout s, fuel).
pub fn limit_maxima() -> (u64, u64, u64) {
    (
        // Lower the per-addon memory ceiling in MiB. Cannot exceed the built-in maximum.
        env_max("PELTY_ADDON_MAX_MEMORY_MIB", LIMIT_MEMORY_MIB),
        // Lower the per-addon call timeout in seconds. Cannot exceed the built-in maximum.
        env_max("PELTY_ADDON_MAX_TIMEOUT_SECS", LIMIT_TIMEOUT_SECS),
        // Lower the per-addon fuel budget. Cannot exceed the built-in maximum.
        env_max("PELTY_ADDON_MAX_FUEL", LIMIT_FUEL),
    )
}

impl Sandbox {
    /// Kind defaults, overridden by manifest `limits`
    /// (`{"memory_mib", "timeout_secs", "fuel"}`). A value above the admin
    /// maximum (or zero) is an error, not silently clamped.
    pub fn from_manifest(kind: &str, limits: Option<&serde_json::Value>) -> Result<Self, String> {
        let mut sb = Sandbox::for_kind(kind);
        let (mm, mt, mf) = limit_maxima();
        // Omitted fields must obey the same admin ceilings as explicit ones.
        sb.max_memory_bytes = sb.max_memory_bytes.min((mm * 1024 * 1024) as usize);
        sb.timeout = sb.timeout.min(Duration::from_secs(mt));
        sb.fuel = sb.fuel.min(mf);
        let Some(l) = limits.filter(|l| !l.is_null()) else {
            return Ok(sb);
        };
        let o = l.as_object().ok_or("limits must be an object")?;
        for (k, v) in o {
            let n = v
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("limits.{k} must be a positive integer"))?;
            match k.as_str() {
                "memory_mib" if n <= mm => sb.max_memory_bytes = (n * 1024 * 1024) as usize,
                "timeout_secs" if n <= mt => sb.timeout = Duration::from_secs(n),
                "fuel" if n <= mf => sb.fuel = n,
                "memory_mib" | "timeout_secs" | "fuel" => {
                    return Err(format!("limits.{k} = {n} is above the admin maximum"))
                }
                _ => return Err(format!("unknown limit {k}")),
            }
        }
        Ok(sb)
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Sandbox {
            fuel: 50_000_000,
            max_memory_bytes: 32 * 1024 * 1024,
            timeout: Duration::from_secs(2),
        }
    }
}

/// The memory cap is per linear memory. Bound their count and table growth too.
fn store_limits(sandbox: Sandbox, instances: usize) -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(sandbox.max_memory_bytes)
        .memories(8)
        .tables(8)
        .table_elements(65_536)
        .instances(instances)
        .build()
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    addon_id: String,
    logs: Vec<(String, String)>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl host::Host for State {
    fn log(&mut self, level: Level, message: String) {
        let msg: String = message.chars().take(2000).collect();
        let lv = format!("{level:?}").to_lowercase();
        tracing::info!(addon = %self.addon_id, level = %lv, "{msg}");
        if self.logs.len() < 200 {
            self.logs.push((lv, msg));
        }
    }
}

/// A compiled captcha addon, ready to be instantiated per call.
pub struct CaptchaAddon {
    engine: Engine,
    component: Component,
    linker: Linker<State>,
    sandbox: Sandbox,
    pub id: String,
}

pub(crate) fn engine() -> Result<Engine, WasmError> {
    let mut c = Config::new();
    c.consume_fuel(true)
        .epoch_interruption(true)
        .wasm_component_model(true);
    Engine::new(&c).map_err(|e| WasmError::Load(e.to_string()))
}

impl CaptchaAddon {
    pub fn from_component_bytes(
        id: &str,
        wasm: &[u8],
        sandbox: Sandbox,
    ) -> Result<Self, WasmError> {
        let engine = engine()?;
        let component =
            Component::new(&engine, wasm).map_err(|e| WasmError::Load(e.to_string()))?;
        let mut linker: Linker<State> = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| WasmError::Load(e.to_string()))?;
        captcha_bindings::CaptchaAddon::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
            .map_err(|e| WasmError::Load(e.to_string()))?;
        Ok(CaptchaAddon {
            engine,
            component,
            linker,
            sandbox,
            id: id.to_string(),
        })
    }

    /// Load from an addon zip: manifest must say `engine: "wasm"`, `kind: "captcha"`,
    /// `abi` compatible with [`ABI_VERSION`], and `wasm` must name a declared,
    /// sha256-verified file inside the package.
    pub fn from_zip(bytes: &[u8], sandbox: Sandbox) -> Result<Self, WasmError> {
        let pkg = addon_pkg::inspect(bytes, Limits::default())?;
        let wasm = wasm_entry(&pkg, "captcha")?;
        Self::from_component_bytes(&pkg.id, wasm, sandbox)
    }

    /// Run `verify` in a fresh store. Returns the result plus the addon's log lines.
    pub fn verify(
        &self,
        req: &VerifyRequest,
    ) -> Result<(VerifyResult, Vec<(String, String)>), WasmError> {
        let state = State {
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            limits: store_limits(self.sandbox, 4),
            addon_id: self.id.clone(),
            logs: Vec::new(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(self.sandbox.fuel)
            .map_err(|e| WasmError::Load(e.to_string()))?;
        let timeout = self.sandbox.timeout;
        let deadline = std::time::Instant::now() + timeout;
        store.set_epoch_deadline(1);
        // Concurrent calls share this engine, but not their deadlines.
        store.epoch_deadline_callback(move |_| {
            if std::time::Instant::now() >= deadline {
                Err(wasmtime::Error::msg("addon timed out"))
            } else {
                Ok(wasmtime::UpdateDeadline::Continue(1))
            }
        });
        let engine = self.engine.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if rx.recv_timeout(timeout).is_err() {
                engine.increment_epoch();
            }
        });
        let out = (|| {
            let inst = captcha_bindings::CaptchaAddon::instantiate(
                &mut store,
                &self.component,
                &self.linker,
            )
            .map_err(|e| WasmError::Trap(format!("{e:#}")))?;
            inst.pelty_addon_captcha()
                .call_verify(&mut store, req)
                .map_err(|e| WasmError::Trap(format!("{e:#}")))
        })();
        let _ = tx.send(());
        let _ = watchdog.join();
        let res = out?.map_err(WasmError::Addon)?;
        Ok((res, std::mem::take(&mut store.data_mut().logs)))
    }
}

/// Validate a wasm addon manifest and return the component bytes.
pub fn wasm_entry<'a>(pkg: &'a Package, kind: &str) -> Result<&'a [u8], WasmError> {
    let m = &pkg.manifest;
    if m["engine"] != "wasm" {
        return Err(WasmError::Manifest("engine must be \"wasm\"".into()));
    }
    if m["kind"] != kind {
        return Err(WasmError::Manifest(format!("kind must be \"{kind}\"")));
    }
    let abi = m["abi"].as_str().unwrap_or_default();
    if abi.split('.').take(2).ne(ABI_VERSION.split('.').take(2)) {
        return Err(WasmError::Manifest(format!(
            "unsupported abi {abi:?}, host implements {ABI_VERSION}"
        )));
    }
    let path = m["wasm"]
        .as_str()
        .ok_or_else(|| WasmError::Manifest("missing \"wasm\"".into()))?;
    pkg.files
        .get(path)
        .map(Vec::as_slice)
        .ok_or_else(|| WasmError::Manifest(format!("wasm file {path} not in package")))
}

pub use captcha_bindings::exports::pelty::addon::captcha::{
    VerifyRequest as CaptchaRequest, VerifyResult as CaptchaResult,
};

#[cfg(test)]
mod resource_tests {
    use super::*;

    #[test]
    fn memory_and_table_growth_are_bounded_per_store() {
        let engine = engine().unwrap();
        let sandbox = Sandbox {
            max_memory_bytes: 65_536,
            ..Sandbox::default()
        };
        for _ in 0..2 {
            let mut store = Store::new(&engine, store_limits(sandbox, 4));
            store.limiter(|limits| limits);
            let memory =
                wasmtime::Memory::new(&mut store, wasmtime::MemoryType::new(1, None)).unwrap();
            assert!(memory.grow(&mut store, 1).is_err());
            assert_eq!(memory.size(&store), 1);
            let table = wasmtime::Table::new(
                &mut store,
                wasmtime::TableType::new(wasmtime::RefType::FUNCREF, 0, None),
                wasmtime::Ref::Func(None),
            )
            .unwrap();
            assert!(table
                .grow(&mut store, 65_537, wasmtime::Ref::Func(None))
                .is_err());
            assert_eq!(table.size(&store), 0);
        }
    }
}
