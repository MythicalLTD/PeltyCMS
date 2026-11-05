# Local addon conformance testing

This is an offline **wire-contract regression harness**, not provider certification.
It runs packaged WASM components in the production `pelty-wasm::WasmAddon` runtime.
No server, database, account, API credential, or listening HTTP server is needed.
Use synthetic secrets only. Never record production HTTP traffic verbatim.

## Run the representative suites

From the repository root, with a consistent Rust toolchain on PATH:

```sh
rustup target add wasm32-wasip2
cargo xtask addons stripe prov-digitalocean
cargo run --offline -p xtask --features addon-test -- addon-test extensions/addons/dist/gateway-stripe.zip xtask/tests/fixtures/addons/stripe.json --report target/stripe-conformance.json
cargo run --offline -p xtask --features addon-test -- addon-test extensions/addons/dist/provisioning-digitalocean.zip xtask/tests/fixtures/addons/digitalocean.json --report target/digitalocean-conformance.json
cargo test --offline -p xtask --features addon-test addon_test::tests
```

The build step may need dependency downloads on first use; install/cache the
WASI target and Cargo dependencies before disconnecting. To require an offline
build too, set `CARGO_NET_OFFLINE=true` in your shell before `cargo xtask addons`.
`--offline` controls Cargo, not the sandbox; harness execution itself always uses
an in-memory transport, regardless of Cargo's network setting. Wasmtime is an
opt-in xtask feature so normal developer commands do not link it.

If `rustup target list --installed` lists WASI but compilation cannot find `core`,
check `rustc -Vv` and the executable selected by PATH. A standalone Rust install
can shadow rustup's compiler. Select the matching rustup compiler/toolchain for
both Cargo and rustc; installing a target for a different compiler does not help.
Run Cargo jobs sequentially, including the addon build and harness build.

`addon-test` accepts exactly a package ZIP, a fixture JSON, and optionally
`--report PATH`. Relative paths are relative to the working directory. It does
not build, rehash, sign, install, or modify the addon. The report's parent directory
must exist. stdout contains one JSON report; the optional file contains the same
JSON. Failures return exit status 1 and a short stderr diagnostic. Invalid CLI
arguments also fail, but do not produce a report. Report-write failure fails the
command even if test cases passed.

## What the shipped cases cover

- Stripe: eager configuration validation, checkout minor units/metadata and
  idempotency header, session-to-intent refund resolution, provider failure,
  valid signed webhook, forged signature and expired signature. Webhook time is
  explicitly supplied as `now`, not taken from the machine clock.
- DigitalOcean: exact create request and extracted remote ID, replay with an
  already-persisted ID and zero HTTP, suspend (`power_off`), resume (`unsuspend`
  wire operation, `power_on` request), and rejection of path-like remote IDs.

These are representative existing addons using realistic synthetic responses,
not live API tests. A successful create/action response does not prove that an
asynchronous cloud operation completed. Replay with a persisted ID does not prove
exactly-once creation after a crash before that ID was saved.

## Fixture schema 1

The complete executable examples are in `xtask/tests/fixtures/addons/`.
Top-level fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Must be `1` |
| `addon_id`, `kind` | Must match the package; gateway or provisioning only |
| `settings` | Object passed as `CallCtx.settings`; defaults to `{}` |
| `secrets` | Name/value map passed through host secret imports; defaults empty; each synthetic value must be at least six bytes |
| `server_url` | Optional URL for the runtime's `$server` allowlist |
| `cases` | Nonempty ordered array of independent calls |

Each case requires `name`, `op`, `input`, `expect`, and `http`. `name` is a human
label in the fixture; report `index` is its zero-based position. Inputs are the
existing gateway/provisioning adapter wire envelopes, **not** REST API requests.
No hidden defaults inject secrets into those envelopes: gateway inputs include
`id`, `config`, and `secret_names`; provisioning inputs include `server` (base URL
and config, no key) and `svc`. Supply settings/config consistently yourself.
Money uses decimal strings plus currency, never JSON floats. See
`crates/pelty-payments/src/wire.rs` and
`crates/pelty-provisioning/src/wire.rs` for the shared guest/host serialization.

`expect` is exactly one externally tagged variant: `{"value": <JSON>}` or
`{"error": "exact wire error"}`. Successful outputs are compared as complete
JSON values (object key order ignored, arrays ordered). Error expectations match
only `WasmError::Addon` exactly, including the wire prefix such as
`invalid_signature:` or `config:`. A runtime trap never counts as an expected
provider error.

Each `http` exchange requires:

```json
{
  "request": {
    "method": "POST",
    "url": "https://panel.example.test/v2/droplets/42/actions",
    "headers": [["accept", "application/json"], ["authorization", "Bearer do_fixture_only"], ["content-type", "application/json"]],
    "body": "{\"type\":\"power_off\"}"
  },
  "response": {"status": 201, "body": "{\"action\":{\"id\":71}}"}
}
```

Matching is ordered and exact: method, full URL including query, complete header
multiset (case-insensitive names, order ignored, exact values), and body bytes as
a UTF-8 string. Whitespace/form encoding changes are meaningful. Responses are
raw strings/status codes, not inferred from request paths. The current host HTTP
ABI does not expose response headers. No matchers, URL prefixes, fallback routes,
redirect following, or real transport are used. Unexpected calls, mismatches,
and unconsumed exchanges fail even when the addon catches the transport error.
Use `http: []` to assert no outbound calls. Cases do not automatically feed output
into later inputs or maintain remote provider state; write explicit replay inputs.
Unknown fixture struct fields are rejected to catch misspellings.

## Reports and safety boundaries

Reports include `schema_version`, `passed`, runtime ABI, SHA-256 of the exact ZIP,
and per-case index, pass/fail, expected/actual call counts and safe diagnostics
such as `http[0] mismatch: headers, body`. Setup errors return `passed: false`,
`setup_error`, and an empty case list. Reports deliberately omit fixture names,
request/response payloads, logs, provider error text and actual outputs to avoid
accidentally publishing credentials. Use the index to inspect your local fixture.

Before instantiation, package inspection validates structure/hashes. The harness
permits only `http` and `secrets` capability keys, rejects `http: ["*"]` (the
runtime may resolve DNS before invoking a wildcard transport), and refuses all
power/event/KV capabilities, even if configured empty. Explicit hosts, `$server`,
`$setting:...`, and explicit suffix allowlists still use runtime authorization;
no manifest capability is broadened. `describe` must not make HTTP calls.

Host log events are observed during describe, successful calls, errors, and traps.
A visible synthetic secret or runtime `[redacted]` marker causes failure. The
marker deliberately treats an attempted leak as a failure even when the runtime
redacts it. Runtime truncation/redaction occurs before observation; short,
encoded, split, transformed, or omitted secret sentinels are not exhaustively
detected. A literal harmless `[redacted]` message can be a false positive. This
is a useful regression guard, not data-flow analysis or proof of no exfiltration.

The harness does not call the application payment/provisioning adapters: it uses
their wire inputs directly so it can inspect call logs and replace provisioning
HTTP without changing its adapter (which selects `GuardedHttp`). Consequently it
does not cover registry wiring, application authorization, webhook deduplication,
billing, retries/queues, persistence, migration/upgrade transactionality, package
signature trust, DNS/SSRF enforcement by the real HTTP transport, or arbitrary
providers/kinds. Runtime fuel, memory and timeout enforcement still applies; time
and randomness imports are not virtualized. Keep generated timestamps/randomness
out of exact fixture requests unless the addon accepts deterministic inputs.

Review [compatibility](addon-compatibility.md),
[security](addon-security-review.md), and [signing](addon-signing.md) separately.
