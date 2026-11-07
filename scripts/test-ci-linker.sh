#!/usr/bin/env bash
# A real wasm link catches native RUSTFLAGS leaking into nested addon builds.
# Inherit the job environment unchanged: clearing RUSTFLAGS here would hide the
# regression rather than testing the configuration used by `cargo xtask addons`.
set -euo pipefail
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/src"
cat > "$root/Cargo.toml" <<'TOML'
[package]
name = "pelty-ci-linker-smoke"
version = "0.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[workspace]
TOML
cat > "$root/src/lib.rs" <<'RS'
#[no_mangle]
pub extern "C" fn pelty_ci_linker_smoke() -> u32 {
    42
}
RS
# No dependencies, network access, or warm target cache can mask this link.
cargo generate-lockfile --manifest-path "$root/Cargo.toml" --offline
cargo build --manifest-path "$root/Cargo.toml" --target-dir "$root/target" \
    --target wasm32-wasip2 --release --locked --offline
test -s "$root/target/wasm32-wasip2/release/pelty_ci_linker_smoke.wasm"
