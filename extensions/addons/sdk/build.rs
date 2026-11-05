// Rebuild the guest bindings whenever the host WIT changes; without this a
// cached SDK can miss new imports (e.g. `filters.apply`) after an ABI bump.
fn main() {
    println!("cargo:rerun-if-changed=../../../crates/pelty-wasm/wit");
    println!("cargo:rerun-if-changed=build.rs");
}
