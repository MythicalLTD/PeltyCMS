# Addon security review checklist

Record reviewer, addon/package hash, source revision, SDK/ABI, host version,
provider API version and evidence for each relevant item. Mark exclusions and
residual risks explicitly. Passing the [local harness](addon-testing.md) is
regression evidence, not approval, certification or a trust decision.

## Integration admission and maintenance

Do not treat addon count as the success metric. Before adding or promoting an integration, record:

- Named customer requests or anonymized demand evidence, required workflows and regional/payment requirements.
- Provider sandbox availability, API stability, rate limits, commercial/access requirements and deprecation policy.
- A maintainer, supported provider API versions, fixture coverage, a real-sandbox validation date and an incident/credential-rotation owner.
- The minimum privileges needed and whether an existing maintained integration already satisfies the workflow.
- Upgrade, reconciliation, webhook replay and operator recovery evidence appropriate to the integration's risk.

Prioritize blockers to actual customer migrations and frequently used revenue/provisioning workflows. Label packages as experimental, fixture-tested or sandbox-validated according to evidence; do not call an integration certified merely because it compiles or passes local fixtures. Stripe and DigitalOcean are representative fixture examples, not a claim that they are the highest-demand providers or production-certified. Review maintenance cost and unresolved incidents before admitting more integrations.

## Trust and release provenance

- [ ] Review source and dependency changes, reproducible build inputs, lockfiles,
  licenses, vulnerabilities, and the exact WASM/package selected for deployment.
- [ ] Validate package structure, sizes, paths and hashes before extraction.
  Verify signatures against **operator-owned** trusted keys before installation,
  migration, upgrade or activation; never trust a key bundled with an upload.
- [ ] Keep signing private keys out of source/packages/logs and backups accessible
  to addon code. Restrict filesystem permissions/Windows ACLs and signing access.
- [ ] Follow [signed addon releases](addon-signing.md). Record the signer/key ID,
  distribute new public keys over an authenticated channel, define the overlap
  period, revoke retired/compromised keys, and test unknown/revoked/invalid keys.
- [ ] Define emergency response for already-installed code after revocation.
  Signature validity alone does not prevent rollback or revoke a running addon.
  Document unsigned-package policy and downgrade controls in the application.

## Least privilege and isolation

- [ ] Request only required permissions and capabilities; review changes against
  the previously approved version. Approval is an application/operator decision,
  not something the SDK constants or harness grant.
- [ ] Prefer exact outbound hosts. Review `$server`, `$setting:...`, suffix
  allowlists, schemes, ports, redirects, private networks and cloud metadata
  access. Keep `http_private` and wildcard access off unless explicitly justified.
  Exercise real SSRF/DNS-rebinding controls outside the offline harness.
- [ ] Minimize declared secret names. Read secrets through host imports, not
  ordinary settings or JSON wire input. Never log auth headers, webhook signing
  material, passwords, tokens, private keys or complete customer/provider payloads.
- [ ] Test missing/revoked/rotated credentials and redaction. Do not rely on host
  redaction as permission to log secrets; transformed/split/short values can leak.
- [ ] Avoid fs, db/sql, exec, bridge, jobs and broad UI powers where possible.
  If necessary, review each path/table/program/argument/quota and runtime/operator
  allowlist; prefer structured commands over shell interpolation. Check symlinks,
  traversal, subprocess output, untrusted configuration and data exfiltration.
- [ ] Bound fuel, memory, wall time, HTTP payload sizes, call counts, queues and
  retry rates. Verify failure/cancellation does not leave a partial privileged
  action silently accepted. Check hooks/mixins/routes for auth bypass and recursion.

## Gateway and webhook behavior

- [ ] Check exact decimal/currency handling and minor-unit rounding; use stable
  payment/reference metadata and idempotency keys, including refund keys.
- [ ] Verify webhook signatures over original bytes with constant-time checks,
  enforce timestamp tolerance where the provider supports it, and test malformed,
  forged, expired and rotated-secret signatures. Do not authenticate by source
  URL or an unverified event field alone.
- [ ] Deduplicate verified event IDs durably in the application; handle reordered
  events, duplicate deliveries and concurrent callbacks. A parser returning the
  same event twice is not application deduplication.
- [ ] Reconcile amount, currency, provider account and local payment ownership
  before applying financial effects. Verify refunds, partial refunds and unknown
  statuses do not accidentally settle a payment. Limit sensitive error detail.
- [ ] Classify provider/network failures accurately; retry only safe operations
  with bounded backoff and reconciliation after ambiguous outcomes.

## Provisioning lifecycle and idempotency

- [ ] Validate remote IDs and template fields before constructing paths, queries,
  commands or JSON. Encode correctly and bind remote resources to the right
  customer/service/server; reject cross-tenant IDs.
- [ ] Persist the created remote ID/module data and reuse them for retries,
  suspend/resume and terminate. Test create replay with the saved ID, plus the
  harder crash-after-provider-success/before-local-commit window separately.
- [ ] Use provider idempotency support or explicit reconciliation for ambiguous
  creates. Do not advertise exactly-once creation from a local saved-ID shortcut.
- [ ] Document asynchronous create/action completion, polling, failed actions,
  suspend/resume ordering, already-absent termination, timeout and retry behavior.
  Do not interpret an accepted action as a completed one.

## Migrations, upgrades and operator recovery

These are review requirements for the application upgrade owner, not behavior
implemented by the SDK or local harness.

- [ ] Inventory schema/data changes and enforce addon ownership/namespacing.
  Review raw SQL, grants, indexes, locks, destructive steps and access to core data.
- [ ] Verify trust before executing migration code. Reapproval must cover expanded
  capabilities; check every install/upgrade entry point, not only a CLI path.
- [ ] Define ordered migration history/checksums and retry/idempotency behavior.
  Test duplicate execution, partial failure, concurrent upgrades and interrupted
  activation. Where transactional DDL is unavailable, specify recovery steps.
- [ ] Test an upgrade from a real prior package/data snapshot, not only a clean
  install. Preserve customer/service IDs, money, provider references and secrets.
- [ ] Back up and test restore. Declare whether rollback is supported after each
  migration and whether old code can read new data; do not assume downgrading
  signed WASM reverses a schema migration.
- [ ] Separate package verification, extraction/staging, migration and activation.
  Document their ordering/atomicity and prevent requests/jobs using incompatible
  code/schema during cutover. Include credential/key rotation during upgrade.
- [ ] Record compatibility evidence and known limitations using
  [the compatibility contract](addon-compatibility.md). Keep deployment approval,
  residual-risk acceptance and operational monitoring separate from test output.
