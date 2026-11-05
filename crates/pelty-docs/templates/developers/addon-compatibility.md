# Addon SDK and ABI compatibility

Compatibility has several independent layers. Do not use an SDK version, addon
version, signature, or passing fixture as a substitute for host compatibility.

Related guides: [local testing](addon-testing.md),
[security review](addon-security-review.md), and [signed releases](addon-signing.md).

## Public SDK version identifiers

`pelty-addon-sdk` exposes these `&str` constants:

| Constant | Contract |
| --- | --- |
| `SDK_VERSION` | `env!("CARGO_PKG_VERSION")`: the SDK Rust crate's version inherited from the addon workspace; currently `0.1.0` |
| `WIT_PACKAGE_VERSION` | `0.2.0`: the generic bindings' `pelty:addon` WIT package identity |
| `ABI_VERSION` | `0.5.0`: latest manifest ABI feature level documented by this SDK, **not** an automatic minimum host or a negotiation result |
| `BASE_ABI_VERSION` | `0.2.0`: baseline generic addon manifest ABI |
| `CRON_ABI_VERSION` | `0.5.0`, available with SDK feature `cron`: minimum manifest feature level for the scheduled export |

These constants are informational Rust API. They do not update `addon.json`,
embed capability approval, generate a compatibility range, or add runtime
negotiation. There is no promise that every SDK feature works with a host merely
because the manifest ABI was accepted. `pelty-wasm::ABI_VERSION` refers to the
separate legacy captcha world (`0.1.0`); the generic runtime's identifier is
`pelty_wasm::runtime::ABI` (`0.5.0`). Do not confuse them.

## Current generic host behavior

The generic runtime parses the manifest `abi` with strict
`semver::Version::parse` and applies this compatibility policy:

- Stable `0.2.x`, `0.3.x`, and `0.4.x` versions are accepted for any valid patch
  number.
- For the current `0.5` line, only patch `0` (`0.5.0`) is accepted. Future
  patches such as `0.5.1` are rejected until explicitly supported by the host.
- All prereleases are rejected, including `0.4.0-rc.1` and `0.5.0-rc.1`.
- Build metadata is allowed and does not change the compatibility verdict:
  `0.4.7+build.12` and `0.5.0+build.12` are accepted, while `0.5.1+build.12`
  remains rejected.
- Incomplete versions (`0.5`), malformed versions (`0.5.00`), leading/trailing
  whitespace (` 0.5.0` or `0.5.0 `), and unsupported major/minor lines (`0.6.0`,
  `1.0.0`) are rejected. The parser does not trim or normalize invalid input.

Publish complete `major.minor.patch` strings, optionally with valid build
metadata. ABI acceptance is followed by component linking, descriptor parsing
and capability/limit checks, any of which can still fail.

| Manifest feature level | Additions |
| --- | --- |
| `0.2` | Generic `describe`, JSON `call`, `on-event`, host services and kind-specific wire adapters |
| `0.3` | Lifecycle phases, custom events and mixins |
| `0.4` | Call context, filters and core-function mixins |
| `0.5` | Optional `scheduled` / `on-cron` export and cron jobs |

The additive generations retain WIT package identity `pelty:addon@0.2.0`.
The current host is intended to keep older components linking, rather than force
recompilation of all old releases. The inverse is not guaranteed: rebuilding an
old manifest with a new SDK can introduce imports unavailable on an older host.
The `cron` feature selects the `pelty-addon-cron` world instead of the baseline
`pelty-addon` world. Test the actual built component against the oldest host you
intend to support and declare its required manifest feature level honestly.

Gateway/provisioning JSON is part of the wire contract as well as WIT. Use the
shared `pelty-payments::wire` / `pelty-provisioning::wire` serializers, exact money
strings, and structured error prefixes. Do not reinterpret unknown operations as
success. Some optional provisioning operations (`list_accounts`, `catalog`) have
explicit older-addon fallbacks; this is not a universal unknown-op policy.

## Semver policy and limits of the guarantee

The SDK is currently pre-1.0 and workspace-versioned, not a separately released
stable platform. Use Cargo's pre-1.0 interpretation: compatible `0.1.x` patch
updates must preserve documented public Rust signatures, existing macro use,
existing supported wire meanings and baseline exports; incompatible changes
require the next minor (`0.2.0`), not a patch. After 1.0, those incompatible
changes require a major. Additive optional APIs and bug fixes may be compatible,
but new imports/exports must declare and test a suitable host feature level.
Do not claim long-term support or a fixed support window that has not been
published. Pin the SDK and shared dependency revisions/lockfile for reproducible
builds; source compatibility does not guarantee identical WASM bytes.

This is the maintenance rule for the SDK's documented public surface, not a
retroactive blanket stability claim for all internal code or generated
wit-bindgen implementation details. Separate version changes are required for a
breaking manifest/WIT/wire contract; changing the Rust crate version alone does
not make an old host understand it. Conversely, retaining an ABI string does not
justify changing the meaning of old fields or error codes incompatibly.

No SDK compatibility promise covers provider REST changes, addon business logic,
credentials, operator approvals/limits, schema/data migrations, all CPU/OS/compiler
combinations, or application API compatibility. Package signatures establish
identity/integrity, not compatibility or an anti-rollback policy.

## Deprecation and removal rules

1. Identify the public symbol/operation/field, reason, replacement, affected SDK
   and ABI generations, and migration example in release notes and this contract.
   Use Rust `#[deprecated(since = "...", note = "...")]` where applicable.
2. Keep the old behavior available through the remainder of its compatible SDK
   line. Do not remove or silently repurpose it in a patch, and do not use a new
   default to bypass this rule. Retain regression coverage for old inputs.
3. Remove a Rust API only at the next allowed breaking SDK boundary (pre-1.0
   minor, post-1.0 major), after publishing the replacement and removal release.
   No calendar grace period is implied. If no removal release is decided, say so.
4. Removing a WIT import/export or breaking JSON behavior also requires an
   explicit incompatible ABI strategy and host compatibility documentation.
   Old component support may be dropped only in a deliberately documented host
   breaking release, not as an incidental SDK cleanup. Re-test retained ABI lines.
5. Urgent security fixes may restrict unsafe behavior sooner. Publish the risk,
   affected versions and required operator action explicitly; do not describe
   security tightening as an unconditional backwards-compatibility guarantee.

For every release, record the SDK revision, manifest ABI, oldest/newest tested
hosts and compiler target, retain package hashes, run the relevant
[offline fixtures](addon-testing.md), and test real upgrade/retry paths separately.
Parent application owners define trust/activation and migration upgrade policy;
this SDK document does not implement or assert those controls.
