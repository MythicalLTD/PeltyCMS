# Safe addon package upgrades

An installed package replacement is a schema-frozen update, not a migration
engine or a new approval flow. Disable the addon, upload a ZIP with
`POST /api/v1/admin/addons/upload?replace=true`, then enable it after reviewing
the release. Registry downloads and bundled restoration use the same package
acceptance path. A first upload without an installed registry row is unaffected.

## Replacement contract

Before extracting any files or updating the installed row, the host checks:

- The addon is disabled, except for trusted bundled startup restoration.
- Registry kind and effective engine still match. The old manifest's kind,
  engine implementation (`wasm` versus native), and `provides` identity cannot
  change either.
- Every recorded applied migration is still declared, with its recorded SHA-256
  checksum. No new migrations are allowed. The declaration list must remain
  unchanged, including ordering. **Schema is frozen in place**: no upgrade SQL
  runs, and no schema/data migration engine is introduced.
- The previous `addon.json` is readable, valid, hashes to the installed registry
  hash, and agrees with its identity, version, and permissions. Missing,
  unreadable, invalid or edited manifests fail closed; there is no catalog
  fallback that silently treats an installed addon as a first install.
- Permissions, capabilities, the full settings specification, UI (including
  iframe targets), sandbox limits, ABI, and WASM entry path are identical.
  This deliberately rejects narrowing as well as widening, settings additions
  as well as removals, and even settings label/default changes. Installed
  configuration and approvals are not silently reinterpreted.
- Old and incoming versions parse with SemVer 1.x rules. Downgrades are refused;
  prerelease ordering follows SemVer. Build metadata alone is not a version
  bump. Equal-precedence versions are allowed only when the complete parsed
  manifest is identical, including file hashes and descriptive fields. This
  permits identical retries and current bundled seeds, not same-version code
  changes. JSON formatting/key ordering is not part of the parsed contract.

A higher version may update ordinary file contents, display metadata and i18n,
but not the frozen fields above. The upload inspector verifies actual package
bytes against the incoming file hashes. Successful replacement preserves
settings, permissions and enabled state; it updates the version and manifest
hash. Rejected checks do not alter package files, the installed row, or migration
records. This is a preflight guarantee, not a new cross-filesystem/database
transaction. Concurrent lifecycle operations are excluded by the advisory lock
below.

For schema or contract changes, use an explicitly reviewed reinstall procedure.
Back up addon configuration and data first, plan data export/import and new
approvals, and follow the addon's uninstall/data-retention rules. Reinstall is
not an automatic destructive migration or a promise that existing SQL tables
will accept the new install migrations.

## Trusted restoration and legacy adoption

`restore` is an internal flag for first-party bundled startup packages, never an
HTTP option. For normal installed packages it bypasses only the disabled-state
requirement, not migration, identity, contract, integrity or version checks.

Legacy rows whose source **and** manifest hash are `builtin` have no previous
package contract to compare. Only trusted restoration may adopt one, and only
when the incoming package is WASM, has the same kind/effective engine, has the
canonical `builtin_id(kind, engine)`, and has no applied or incoming migrations.
The legacy version/contract comparison is intentionally unavailable for this
one-time native-to-package transition. Ordinary uploads cannot use it. Subsequent
package-backed replacements must pass the full disk-manifest policy.

## Lifecycle concurrency

`addon_upgrade::lock(pool, id)` acquires a transaction-scoped PostgreSQL advisory
lock using `pg_try_advisory_xact_lock(hashtextextended($1, 0))` and the namespaced
key `pelty:addon:<id>`. Another lifecycle operation for the same addon fails fast
with HTTP 409 `error.addon_busy`; it does not wait for the advisory lock.
Operations for different addon IDs can proceed independently. Retry a busy
operation after the active lifecycle completes.

The returned `LifecycleGuard` owns a temporary control connection opened using
`PgConnection::connect_with` and the pool's connection options. It executes
`BEGIN` before attempting the transaction-scoped lock, without checking out any
application pool slot. Registry queries therefore remain usable even with a
one-connection pool. Each active administrative lifecycle operation needs one
additional PostgreSQL connection outside the pool's configured connection limit;
server connection capacity must account for this. A failed acquisition, error or
cancellation drops its owned connection rather than returning it to the pool.

Keep the guard alive throughout validation, checks, extraction, registry writes
and runtime changes. Dropping it closes the control connection; PostgreSQL rolls
back its transaction and releases the lock when it processes the disconnect.
A retry immediately after drop may briefly still report busy. Call
`guard.release().await?` when deterministic release is needed: it awaits the
`ROLLBACK` response and closes the connection. Ordinary lifecycle callers may
keep using `_lock` and automatic drop. The control transaction changes no addon
data and does not make filesystem and registry updates one transaction.

Acquire the guard in `accept_package_with` after parsing the addon ID and before
validation or registry checks, and at the start of `install_addon`,
`set_addon_enabled` and `uninstall_addon`. Never reacquire in nested
`install_entry`, since that would conflict with the outer lifecycle guard.

```rust
let _lock = crate::addon_upgrade::lock(&s.pool, id).await?;
```

## Integration hook

In `crates/pelty-api/src/addon_host/addons.rs`, inside `accept_package_with`, after the
existing `reg::get`/replacement checks and before `let root = addons_dir()` and
any extraction, call:

```rust
crate::addon_upgrade::check(s, &m, restore).await?;
```

The checker is exported from `pelty-api`; its version comparison requires the
API crate's `semver = "1"` dependency. No routes or database migrations are added.

## Error locale keys

Policy failures use HTTP 409 conflicts with these stable keys. Locale entries
must be supplied by the parent integration; this module does not edit locales.

| Key | Meaning |
| --- | --- |
| `error.addon_busy` | Another lifecycle operation holds this addon's lock; retry after it completes |
| `error.addon_disable_first` | Disable the addon before ordinary replacement (existing key) |
| `error.addon_upgrade_migrations` | Applied migration set/checksums or declaration list changed (existing key) |
| `error.addon_upgrade_identity` | Kind, engine or provided identity changed |
| `error.addon_upgrade_manifest` | Installed manifest unavailable, untrusted, inconsistent, or legacy adoption not permitted |
| `error.addon_upgrade_contract` | Frozen permissions, capabilities, settings, UI, limits, ABI or WASM path changed; reviewed reinstall required |
| `error.addon_upgrade_version` | Invalid SemVer, downgrade, or changed manifest without a version-precedence increase |

The generated-package integration suite is registered as
`addon_upgrade_flow` in the existing single integration test binary. It covers
successful schema-frozen replacement, same-version retries, rejection atomicity,
checksum changes/removals/additions, identity, contract, missing/corrupt/edited
manifests, trusted restoration, same-ID lock contention, different-ID independence,
a usable single-connection application pool while the guard is held, and
reacquisition after awaited guard release. The suite itself compiles no WASM fixtures;
the common application harness retains its existing bundled-addon prerequisites.
