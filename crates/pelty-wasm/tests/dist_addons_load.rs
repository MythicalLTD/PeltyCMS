//! Every packed addon in extensions/addons/dist (from `cargo xtask addons`) must load in the sandbox.
//! Skipped when the dist folder has not been built.
#[test]
fn all_dist_load() {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/addons/dist");
    if !d.is_dir() {
        eprintln!("skip: no {}", d.display());
        return;
    }
    let mut bad = vec![];
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let z = std::fs::read(e.path()).unwrap();
        if let Err(err) = pelty_wasm::WasmAddon::from_zip(&z, pelty_extensions::addon_pkg::Limits::default(), Default::default()) { bad.push(format!("{:?}: {err}", e.path().file_name())); }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
