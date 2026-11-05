# API keys and integrations

## API keys

Staff create personal API keys in Admin → Addons & integrations → **API keys**. A key is shown once, carries a subset of your own permissions (scopes) and can expire after 1 to 3650 days. Send it as `Authorization: Bearer pelty_…`.

- Scopes are permission names, so a key can be limited per resource and per action: `invoices.read` only reads invoices, `invoices.manage` reads and writes them.
- Keys work on the API routes their scopes allow, but never on `/auth/…`, `/account/…` or the API-key routes themselves: a key cannot create or revoke keys.
- Creating, listing and revoking keys needs an interactive sign-in.
- Failed key lookups are rate-limited per IP address. The last use time and IP are recorded.
- **Allowed IPs** (optional): up to 20 IP addresses or CIDR ranges (`203.0.113.7`, `10.0.0.0/8`, `2001:db8::/32`). A key with a list only works from those addresses; other calls get `403 error.api_key_ip_denied` and an `api_key.ip_denied` audit entry. Behind a reverse proxy, configure Trusted proxies so the real client IP is seen.
- The owner (or staff with `api_keys.manage` in the all-keys list) can change a key's allowed IPs and expiry later without rotating it. Scopes are fixed: create a new key to change them.
- Staff can revoke all keys of a user.

## Outgoing webhooks

Admin → Addons & integrations → **Webhooks** (permission `webhooks.manage`) sends domain events to your own HTTPS endpoints, for example to sync a CRM or accounting system.

- **Event filter per endpoint:** pick single events (`invoice.paid`), whole categories (`invoice.*`) or everything (`*`). The list comes from the event registry, so every domain event can be subscribed to.
- **Request:** `POST` with JSON `{"id": "<delivery id>", "event": "invoice.paid", "created_at": <unix seconds>, "data": {…event payload…}}` and headers `X-Pelty-Event`, `X-Pelty-Delivery`, `X-Pelty-Timestamp` and `X-Pelty-Signature`.
- **Signature:** `X-Pelty-Signature` is `sha256=` followed by the hex HMAC-SHA256 of `<timestamp>.<raw body>`, keyed with the endpoint's signing secret (`whsec_…`, shown once on create and on rotate). Recompute it over the exact bytes you received, compare in constant time, and reject old timestamps (for example older than 5 minutes) to stop replays. Use `X-Pelty-Delivery` to ignore duplicates.
- **Retries:** any 2xx counts as delivered. Network errors, 5xx, 408 and 429 are retried up to 8 times with exponential backoff (10 s, 20 s, 40 s, … up to about 40 minutes in total). Other 4xx answers fail at once. After 20 failed deliveries in a row the endpoint is disabled (audit entry `webhook_endpoint.auto_disabled`, event `webhook_endpoint.disabled`); saving it as enabled again resets the counter.
- **Delivery log:** every delivery with status, attempts, HTTP status or error, duration and the full payload, filterable by endpoint and status. **Redeliver** sends the same event and payload again as a new delivery linked to the original. **Send test** queues a `webhook.ping` event, even to a disabled endpoint.
- Endpoint URLs must use `https://`. Private, loopback and link-local addresses are refused at send time. Signing secrets are stored encrypted, so `PELTY_ENCRYPTION_KEY` must be set. Up to 50 endpoints.
- The same features are available over the API: `/api/v1/admin/webhook-endpoints` and `/api/v1/admin/webhook-deliveries`.

## OAuth clients and SAML

**OAuth clients** registers apps that sign users in with PeltyCMS (PeltyCMS as OAuth provider). Clients see the apps they approved under **Connected apps**. **SAML** configures SAML identity providers for single sign-on (Exclusive C14N, RSA/ECDSA-SHA256, HTTP-POST/Redirect only — no EncryptedAssertion or artifact binding; see [Security](admin-security.md)).

## The HTTP API

Every route is listed in the developer [HTTP API](developers/api.md) reference with its permission, parameters and responses, and as an OpenAPI document (`developers/api.json`). The [Developer docs](developers/index.md) cover events, webhooks and writing addons.
