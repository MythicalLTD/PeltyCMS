//! Mock registrar (testing and demos). Each call runs in a fresh instance,
//! so the domain table lives in the addon's host kv store.
#[cfg(all(target_family = "wasm", feature = "export"))]
use pelty_addon_sdk::host;
pub mod mock;
#[cfg(all(target_family = "wasm", feature = "export"))]
use mock::MockRegistrar;

#[cfg(all(target_family = "wasm", feature = "export"))]
const KEY: &str = "domains";

#[cfg(all(target_family = "wasm", feature = "export"))]
struct PeltyAddon;
#[cfg(all(target_family = "wasm", feature = "export"))]
impl pelty_addon_sdk::Guest for PeltyAddon {
    fn describe() -> String {
        pelty_addon_sdk::provisioning::describe_registrar(&MockRegistrar::default(), pelty_addon_sdk::serde_json::json!([]))
    }
    fn call(op: String, input: String) -> Result<String, String> {
        let m = MockRegistrar::from_snapshot(&host::kv_get(KEY).unwrap_or_default());
        let out = pelty_addon_sdk::provisioning::call_registrar(&m, &op, &input);
        host::kv_set(KEY, &m.snapshot())?;
        out
    }
    fn on_event(_name: String, _payload: String) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::export!(PeltyAddon with_types_in pelty_addon_sdk);
