# Plugins in other languages

Any language that can build a **WASI preview 2 component** for the [WIT world](wit.md) can be used to write an addon. Rust is the language we test. `pelty-cli addon wasm-scaffold --lang c|tinygo` writes starter templates for C and TinyGo. These templates are **not compiled in CI**. The scaffold labels them as untested, so treat them as a starting point.

## C (wasi-sdk + wit-bindgen)

{{rawconst:crates/pelty-cli/src/wasm_scaffold.rs:C_MAIN:c}}

## TinyGo

{{rawconst:crates/pelty-cli/src/wasm_scaffold.rs:GO_MAIN:go}}

## Rust template

{{rawconst:crates/pelty-cli/src/wasm_scaffold.rs:RUST_LIB:rust}}
