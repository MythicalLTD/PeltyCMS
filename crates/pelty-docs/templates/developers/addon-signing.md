# Signed addon releases

Addon signatures authenticate a release to an **operator-configured** Ed25519 key.
A signature never grants permissions, makes an addon safe to execute, or prevents
rollback to an older valid release. Version/upgrade policy and permission checks
remain separate server responsibilities.

## Release workflow

Store private keys **outside addon source/package directories** and outside Git.
Use a private directory with restrictive permissions. Key generation creates files
exclusively (no overwrite), sets mode `0600` on Unix, and never prints private key
material. On Windows files inherit the parent directory ACL: restrict that ACL to
the signing account before generating keys. Private keys are binary PKCS#8, not
PEM, and are not encrypted; protect them with OS access controls or offline storage.

```sh
pelty-cli addon keygen /secure/release.pk8 --key-id release-2026 --public-out /secure/trusted-keys.json
pelty-cli addon hash ./my-addon
pelty-cli addon validate ./my-addon
pelty-cli addon sign ./my-addon --key /secure/release.pk8 --key-id release-2026
pelty-cli addon pack ./my-addon -o my-addon.zip
pelty-cli addon verify my-addon.zip --trusted-keys /secure/trusted-keys.json --require-signed
```

`sign` operates on an addon directory after hashes are finalized. It creates
`addon.sig` exclusively, refusing an existing signature; explicitly remove an old
signature before re-signing. `pack` includes it, and extraction preserves it.
`hash` excludes `addon.sig` from the manifest files map, but does **not** remove an
old signature or silently bless changed content. Rehashing or editing `addon.json`
(including whitespace) invalidates the old signature. Re-sign after any change.
Keep public trust files outside the addon too; they are operator inputs, not
package contents. `validate`/`pack` check structure and integrity, **not trust**.

## Operator trust and revocation

The public output from `keygen` is a JSON map suitable for `--trusted-keys`:

```json
{
  "release-2026": {
    "public_key": "<standard padded base64 of the 32-byte Ed25519 public key>",
    "revoked": false
  }
}
```

Distribute public keys through an authenticated administrative channel. Never
populate trust from uploaded packages, manifests, registry metadata, or a key
included alongside a downloaded ZIP. Merge approved keys into the operator trust
map during rotation. Set `revoked: true` to reject an old key explicitly; removing
it also rejects it as unknown. Revocation affects future verifications; already
installed/running addons require separate operator action.

Without `--require-signed`, unsigned packages remain backward compatible, and CLI
output explicitly identifies them as unauthenticated. An **existing** signature
is always checked: malformed, unknown-key, revoked-key, and invalid signatures
fail even in permissive mode. An unsigned package is not distinguishable from a
signed package whose signature was removed, so require signatures for production
trust enforcement.

## Server enforcement

Set `PELTY_ADDON_TRUSTED_KEYS` to the path of the operator-owned JSON trust map and `PELTY_ADDON_REQUIRE_SIGNATURES=true` to refuse unsigned disk addons. The server checks the trust file during initialization, rejects malformed configuration, and reloads it for each upload, installation, enable, startup activation and cluster resynchronization. Trust maps are limited to 64 KiB and 256 keys. The built-in host implementations are trusted application code, not downloaded packages.

Docker Compose forwards both variables; mount the public-key JSON read-only and use its **container** path. Never put private signing keys in the server container. With enforcement enabled, first-party disk/bundled WASM packages also need trusted signatures. There is no filename-based or registry-based trust exemption. Without enforcement, legacy unsigned packages remain usable, but any signature that is present must verify against an explicitly trusted, non-revoked key.

Revocation does not interrupt an invocation already in progress and the server does not watch trust-file changes. Disable affected addons immediately and restart or resynchronize every instance after updating trust. Failed verification during resynchronization unloads the affected WASM engine instead of retaining a previously loaded copy. This does not undo database changes or provider actions already performed by an addon.

The API returns a generic localized signature error; server diagnostics identify the addon and verification category without exposing package contents or secrets. Existing admin upload/install/enable actions remain permission-protected and audited. Signatures do not approve capabilities or replace the [security review](addon-security-review.md) and [upgrade checks](addon-upgrades.md).

## Library integration contract

```rust,ignore
use pelty_extensions::{addon_pkg, addon_signing};

// Parse once and verify BEFORE extraction, migration, upgrade, or activation.
let pkg = addon_pkg::inspect(&zip_bytes, addon_pkg::Limits::default())?;
let keys: addon_signing::TrustedKeys = serde_json::from_str(&operator_keys_json)?;
let signer: Option<String> = addon_signing::verify(&pkg, &keys, require_signed)?;
// None: unsigned and allowed; Some(id): authenticated against operator trust.
let installed_path = addon_pkg::extract(&pkg, &extensions_root, false)?;
```

Exact public API (module `pelty_extensions::addon_signing`):

- `TrustedKeys = BTreeMap<String, TrustedKey>`
- `TrustedKey { public_key: String, revoked: bool }` (`revoked` defaults false
  when deserialized; the key is standard padded base64, exactly 32 decoded bytes).
- `verify(&Package, &TrustedKeys, require_signed: bool)
  -> Result<Option<String>, SigningError>`
- `sign(&Package, key_id: &str, pkcs8: &[u8])
  -> Result<ReleaseSignature, SigningError>`
- `generate_keypair() -> Result<(Vec<u8>, String), SigningError>` returns private
  PKCS#8 bytes and the public key's base64 representation; never log the first.
- `ReleaseSignature::parse(&[u8]) -> Result<ReleaseSignature, SigningError>`
- `decode_public_key(&str) -> Result<Vec<u8>, SigningError>` for config validation.
- `key_id_ok(&str) -> bool`, `SIGNATURE_FILE`, `MAX_SIGNATURE_BYTES`.

`Package.signature: Option<ReleaseSignature>` contains parsed metadata, not a
trust verdict. `Package.files` excludes both `addon.json` and `addon.sig`.
`inspect()` rejects malformed signatures as `PkgError::Signature(String)`.
`verify()` distinguishes `Required`, `UnknownKey(String)`, `RevokedKey(String)`,
`InvalidPublicKey(String)`, `InvalidSignature`, `MalformedSignature`, and
`InvalidContent`. Private-key operations also expose `InvalidPrivateKey`,
`InvalidKeyId`, and `Random` errors. No API/config changes are implicit in this
library: all installation and upgrade entry points must invoke verification.

## Signature format v1

`addon.sig` is a reserved root file (also supported under the archive's single
common wrapper directory). It must not be declared in `addon.json.files` or used
as a migration. It is a strict JSON object, capped at 4096 decompressed bytes:

```json
{
  "version": 1,
  "algorithm": "Ed25519",
  "key_id": "release-2026",
  "signature": "<standard padded base64 of the 64-byte signature>"
}
```

Unknown fields (including embedded public keys), duplicate fields, unsupported
versions/algorithms, invalid key IDs, and invalid base64/lengths are rejected.
Key IDs are 1-128 ASCII letters, digits, hyphens, underscores, or periods.

Ed25519 signs this unambiguous deterministic binary payload, not ZIP metadata or
re-serialized JSON:

1. Domain bytes `PeltyCMS addon release\0v1\0` (the `\0` denotes a zero byte).
2. Length-prefixed UTF-8 key ID.
3. Length-prefixed **entire original byte sequence** of `addon.json`.
4. File count as an unsigned 64-bit big-endian integer.
5. For every declared content file in ascending Rust `BTreeMap<String, _>` path
   order: length-prefixed UTF-8 relative path, then length-prefixed lowercase
   ASCII SHA-256 hex digest of the actual file bytes.

Every length prefix is an unsigned 64-bit big-endian byte count. Actual hashes
must match the manifest's declarations (case-insensitive hex comparison), with
no missing or undeclared files. Signing/verifying rechecks that the public
`Package` fields have not diverged from its raw manifest. The detached signature
is excluded to avoid a circular hash. Repacking/compression or changing a common
ZIP wrapper directory does not change the authenticated content.
