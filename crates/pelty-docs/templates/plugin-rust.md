# Write a plugin in Rust

This walk-through uses `extensions/examples/addon-guide`. It is a real addon in the addons workspace. `crates/pelty-api/tests/integration/addon_guide.rs` installs it into a test server and checks every feature shown here, so the samples below always compile and work.

## 1. Scaffold

```sh
pelty-cli addon wasm-scaffold integration-hello --kind integration --lang rust --dir extensions/addons
```

This writes `addon.json`, `Cargo.toml` and `src/lib.rs`. An addon is a WebAssembly component exporting the `addon` interface from [the WIT world](wit.md): `describe`, `call(op, input)` and `on-event(name, payload)`. The Rust scaffold targets ABI 0.5: it enables the SDK's `cron` feature and also exports `on-cron(job)` with an example `nightly` job; declare jobs under `capabilities.cron` in `addon.json` to schedule them (see [Events and hooks](events-guide.md), "Cron jobs"). `cargo xtask addon <id> --kind <kind>` runs the same scaffold inside the addons workspace.

## 2. The entry point

All host calls arrive at `call` with an `op` name and a JSON input. The ops the host uses are `hook`, `mixin` and `http`, plus your own job ops.

{{snippet:guest}}

## 3. The manifest

`addon.json` declares what the addon may do. The admin sees and approves these capabilities at install, and the host refuses everything else. Every key is listed in the [manifest reference](manifest.md) and the [capabilities reference](capabilities.md).

{{file:extensions/examples/addon-guide/addon.json:json}}

## 4. Listen to events

{{snippet:listen}}

The available events and their payloads are in the [event registry](events.md).

## 5. Cancel or modify with hooks

{{snippet:hooks}}

A cancelled pre-event makes the API answer `409` with `error.addon_hook_cancelled`. Priorities, `fail_open` and `timeout_ms` are described in [hooks and mixins](hooks.md).

## 6. Emit your own event

{{snippet:emit}}

## 7. Wrap a core route with a mixin

{{snippet:mixin}}

## 8. Your own HTTP routes

{{snippet:routes}}

## 9. Settings, secrets and storage

{{snippet:settings}}

{{snippet:kv}}

## 10. Outbound HTTP

{{snippet:http}}

## 11. Background jobs

{{snippet:jobs}}

## 12. Build, pack and install

```sh
cd extensions/addons
cargo build --release --target wasm32-wasip2 -p pelty-addon-guide
cp target/wasm32-wasip2/release/pelty_addon_guide.wasm ../examples/addon-guide/addon.wasm
pelty-cli addon hash ../examples/addon-guide
pelty-cli addon pack ../examples/addon-guide -o guide.zip
```

Upload the zip under **Admin → Addons**, review the capabilities, then install and enable it.
