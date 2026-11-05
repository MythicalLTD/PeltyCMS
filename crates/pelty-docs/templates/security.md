# Security model

- Addons run in a wasmtime sandbox. Memory, fuel and time are limited, and there is no ambient file, network or process access.
- Everything an addon can reach is declared in `addon.json` under `capabilities` and approved by an admin at install. The host checks each call against what was approved.
- Some capabilities are marked **dangerous**. The installer shows them prominently.
- Iframe UI needs a separate explicit approval. Frames are sandboxed and can only call the API paths listed in `capabilities.bridge`.
- Outbound HTTP only reaches the hosts listed in `capabilities.http`. Link-local and cloud metadata addresses are always refused.
- Hook payloads redact password fields, and the host restores any redacted value a hook tries to overwrite. Money fields can only change with `money.modify`.
- Package files are hashed (`files` map). Use `pelty-cli addon hash` after every change.

## Addon release review

- Use the [addon security review checklist](addon-security-review.md) to record least-privilege, secret-handling and idempotency evidence.
- Follow [signed addon releases](addon-signing.md) for package verification, operator trust and signing-key rotation; signatures do not grant capabilities.
- Review [addon upgrades](addon-upgrades.md) for migration safety, activation ordering and recovery.
- Check [SDK and ABI compatibility](addon-compatibility.md) against the deployed host and run [local addon tests](addon-testing.md). Passing fixtures are regression evidence, not a trust decision or provider certification.

## Capabilities

{{capabilities}}

## Package permissions

{{permissions}}
