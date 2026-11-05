//! Decides which build outputs `embed-frontend` (web UI) and the opt-in
//! `embed-addons` (addon zips) compile in: each folder is embedded only when
//! it exists, so `cargo build` works without a UI build
//! (the binary then serves the API and any build found on disk).
use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(pelty_embed_frontend)");
    println!("cargo:rustc-check-cfg=cfg(pelty_embed_addons)");
    println!("cargo:rerun-if-changed=build.rs");
    // The update command picks the release archive built for this target.
    println!("cargo:rustc-env=PELTY_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    println!("cargo:rerun-if-changed=release-key.pub");
    if std::env::var_os("CARGO_FEATURE_EMBED_FRONTEND").is_none() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut dirs = vec![("frontend/dist", "index.html", "pelty_embed_frontend")];
    // Addon zips only with the opt-in `embed-addons` feature.
    if std::env::var_os("CARGO_FEATURE_EMBED_ADDONS").is_some() {
        dirs.push(("extensions/addons/dist", "", "pelty_embed_addons"));
    }
    for (dir, marker, cfg) in dirs {
        let d = root.join(dir);
        // A missing folder is watched too, so building it later re-embeds.
        println!("cargo:rerun-if-changed={}", d.display());
        let ok = if marker.is_empty() { d.is_dir() } else { d.join(marker).is_file() };
        if ok {
            println!("cargo:rustc-cfg={cfg}");
        } else {
            println!("cargo:warning={dir} not found: not embedded (run `cargo xtask build` for a single self-contained binary)");
        }
    }
}
