//! Builds real addons from `extensions/addons/<dir>` to wasm32-wasip2 and packs them.
#![allow(dead_code)]
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub fn addons_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extensions/addons")
}

fn target() -> PathBuf {
    std::env::var("PELTY_ADDONS_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pelty-addons-target"))
}

/// Build package `crate_name` (release, wasm32-wasip2) and pack `extensions/addons/<dir>`
/// with its addon.json into a zip. Cached per process.
pub fn addon_zip(dir: &str, crate_name: &str) -> Vec<u8> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, Vec<u8>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(z) = cache.lock().unwrap().get(dir) {
        return z.clone();
    }
    let st = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-p",
            crate_name,
            "--target-dir",
        ])
        .arg(target())
        .current_dir(addons_root())
        .status()
        .expect("run cargo");
    assert!(st.success(), "addon build failed for {crate_name}");
    let wasm = std::fs::read(target().join(format!(
        "wasm32-wasip2/release/{}.wasm",
        crate_name.replace('-', "_")
    )))
    .unwrap();
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pkg =
        std::env::temp_dir().join(format!("pelty-addon-pkg-{dir}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&pkg);
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("addon.wasm"), wasm).unwrap();
    std::fs::copy(
        addons_root().join(dir).join("addon.json"),
        pkg.join("addon.json"),
    )
    .unwrap();
    pelty_extensions::addon_pkg::rehash(&pkg).unwrap();
    let z = pelty_extensions::addon_pkg::pack(&pkg).unwrap();
    let _ = std::fs::remove_dir_all(&pkg);
    cache.lock().unwrap().insert(dir.into(), z.clone());
    z
}

/// Loads provisioning addons on demand (`get(id)`), wired to the real
/// SSRF-guarded host HTTP so tests hit in-process mock panels.
#[derive(Clone, Default)]
pub struct WasmRegistry(pelty_provisioning::Registry);

impl WasmRegistry {
    pub fn get(
        &self,
        id: &str,
    ) -> Option<std::sync::Arc<dyn pelty_provisioning::ProvisioningModule>> {
        if let Some(m) = self.0.get(id) {
            return Some(m);
        }
        let dash = id.replace('_', "-");
        let zip = addon_zip(
            &format!("prov-{dash}"),
            &format!("pelty-addon-provisioning-{dash}"),
        );
        let services = pelty_wasm::HostServices {
            http: std::sync::Arc::new(pelty_wasm::GuardedHttp),
            ..Default::default()
        };
        let addon = pelty_wasm::WasmAddon::from_zip(
            &zip,
            pelty_extensions::addon_pkg::Limits::default(),
            services,
        )
        .expect("load addon");
        let got = pelty_wasm::provisioning::register(&self.0, std::sync::Arc::new(addon))
            .expect("register");
        assert_eq!(got, id);
        self.0.get(id)
    }
}

impl WasmRegistry {
    /// Module infos of the addons loaded so far.
    pub fn list(&self) -> Vec<pelty_provisioning::ModuleInfo> {
        self.0.list()
    }
}

pub fn wasm_registry() -> WasmRegistry {
    WasmRegistry::default()
}
