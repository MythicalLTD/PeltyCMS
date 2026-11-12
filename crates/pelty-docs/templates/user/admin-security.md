# Staff security

{{nav:admin:security}}

## Roles and permissions

Staff accounts get roles in Admin → Security → **Staff**; each role is a set of permissions such as `tickets.manage` or `gateways.manage`. Every admin API call checks the permission it needs.

Core resources have two levels: `<resource>.read` and `<resource>.manage` (customers, catalog, orders, invoices, services, tickets, coupons, affiliates, content, quotes, downloads, client groups, campaigns). `.manage` includes `.read`. A role (or an API key) with only `.read` can open every list and detail page of that resource (GET requests) but cannot create, change or delete anything there. Build read-only roles such as "Accounting viewer" from `admin.access` plus the `.read` permissions it needs.

## Two-factor authentication

Every account can enable TOTP two-factor authentication (an authenticator app) with one-time backup codes, and passkeys. When **Require 2FA for staff** is on (Admin → Security → **Staff**, stored as `security.staff_require_2fa`), staff without 2FA keep client access only until they enrol.

- **TOTP**: scan the QR code, confirm a code, store the backup codes. A backup code works once in place of a TOTP code.
- **Passkeys** (WebAuthn): register a passkey on the Sessions page (`/sessions`); it can then be used to sign in without a password and for step-up. If a passkey is lost, sign in with your password plus TOTP or a backup code. Staff can revoke a user's passkeys.
- **Passkeys as the second factor**: with TOTP on, the password step offers a passkey as an alternative to the code. Without TOTP, a user can turn on *Use passkeys for two-step verification* on `/sessions` (password re-entry required; `PUT /api/v1/auth/mfa/passkey`). The password login then answers `{"mfa_required": true, "methods": ["passkey"]}` and is finished with `POST /api/v1/auth/mfa/passkey/start` and `/finish`. This counts as 2FA for **Require 2FA for staff**. The last passkey can't be removed while it is the only factor, and staff can't turn it off without TOTP while the policy is on. Revoking a user's passkeys from admin also clears the setting, which is the recovery path for a lost device.
- **Duo**: Duo Security's Universal Prompt can be required after the password, for staff only or for everyone. Configure it in Admin → Security → **Security** with the Duo client id, client secret and API host:

{{fields:crates/pelty-api/src/access/duo.rs:DuoConfig}}

The client secret is stored encrypted and never returned. Duo redirects back to `{public URL}/auth/duo/callback`, so `PELTY_PUBLIC_URL` must be right.

## Security policy

Admin → Security → **Security** sets the policy below. Empty values mean "off".

{{fields:crates/pelty-auth/src/policy.rs:SecurityPolicy}}

## Step-up (sudo mode)

With `admin_reauth_minutes` set, staff must re-enter their password (or use a passkey) before a change guarded by one of these permissions when their last full sign-in is older than that:

{{sensitive}}

Reading is never blocked; only writes (`POST`, `PUT`, `PATCH`, `DELETE`) ask for it. API keys are not affected.

## IP rules

Admin → Security → **Security** keeps two kinds of IP rule (single addresses or CIDR ranges, optionally expiring):

- `ban`: refuse every request from the network.
- `admin_allow`: once at least one exists, the admin area (and admin-only addon routes) only answers from allowed networks. PeltyCMS refuses to add a rule that would lock out your own address.

Behind a reverse proxy, make sure the real client address reaches PeltyCMS, or every request looks like it comes from the proxy.

## Sessions, sign-in history and audit

**Sessions** lists and revokes active sessions, **Login history** shows sign-ins, and Admin → System & settings → **Audit** records every staff change. With `login_alerts` on, a staff sign-in from a new IP address is audited and notified. Staff can sign in as a client (impersonation) for support; it is audited and has a way back.

The **Audit** page (permission `audit.read`) is append-only: the database refuses updates, deletes and truncation. Filters can be combined:

- **Search**: free text over the action, target and details (metadata).
- **Action**: a prefix such as `invoice.` or `auth.`, or one exact action; the picker lists the actions that occur, with their counts.
- **Actor**: e-mail substring, or click an actor in the list. **System only** shows entries without a staff or client actor (background jobs, webhooks, the scheduler).
- **IP**: a full address or a prefix (`10.0.`). **Target**: an exact id. Click either in the list to filter by it.
- **From / To**: whole days in UTC, both inclusive. The API also accepts RFC 3339 timestamps.

Click an entry to see its details. **Export CSV** downloads the filtered list, newest first, up to 50,000 rows; the export itself is audited as `audit.exported`. The same filters work on `GET /api/v1/admin/audit-logs` (`action`, `actor`, `actor_id`, `target`, `ip`, `q`, `from`, `to`, `system`) and on `GET /api/v1/admin/audit-logs/export.csv`.

## Single sign-on

Staff and clients can sign in with OAuth/OIDC providers (addons of kind `sso`) and SAML (Admin → Addons & integrations → **SAML**). PeltyCMS can also act as an OAuth provider for other apps (**OAuth clients**).

### Game-store sign-in: FiveM (Cfx.re) and Steam

Two addons give a game store "Login with FiveM" and "Login with Steam" buttons. They show as large accent buttons at the top of the sign-in page and of the store checkout's account step, above the other providers (Discord, Microsoft, ...). Email and password sign-in stays available.

**This is not Rockstar sign-in.** Rockstar's sign-in is first-party only, and the Cfx.re IDMS that Tebex and txAdmin use is private, so PeltyCMS uses neither. "Login with FiveM" signs players in with their **Cfx.re forum account** (forum.cfx.re), which is the path Cfx.re staff point third-party sites to. Steam uses Valve's official **Steam OpenID** sign-in.

Setup (Admin → Addons & integrations):

1. **FiveM (Cfx.re)** (`sso-cfx`): turn on **Show the login button**. You can set an **Application name**, which the forum shows when it asks the player to approve your store (empty uses your site name). No client registration is needed: PeltyCMS creates its forum client id and an RSA key pair on first use and keeps the private key sealed in the addon's storage. The private key never leaves the server.
2. **Steam** (`sso-steam`): turn on **Show the login button**. You can add a **Steam Web API key** (a secret) to get the player's Steam name through `GetPlayerSummaries`. Without a key the sign-in still works. The return URL and realm come from your public URL (`PELTY_PUBLIC_URL`).
3. Both have the usual switches: **Button label** (empty shows "Login with FiveM" or "Login with Steam" in the visitor's language), **Allow new sign-ups**, **Link existing accounts by verified email** and **Allow staff sign-in**. Neither provider vouches for an email, so the email linking option never applies to them.

How it works:

- **FiveM:** the player approves your store on forum.cfx.re (scope `session_info`). The forum returns an encrypted key. PeltyCMS decrypts it with its private key, checks the one-time nonce, reads `/session/current.json`, and then revokes the key straight away.
- **Steam:** Steam sends the player back with a signed assertion. PeltyCMS checks that the assertion comes back to exactly the URL this attempt issued, and Steam itself confirms the signature (`check_authentication`). The account is the SteamID64.
- Every attempt is one-time, expires after 10 minutes, and is bound to the browser that started it by a short-lived cookie, so a callback link sent to someone else does nothing. Sign-in starts are rate-limited like the password login. A player sent back to the store (`return_to`) only ever lands on a path of your own site.
- **New players:** neither provider shares an email, so a new player is asked for an email to finish signing up. The email is used for receipts and account recovery, and a verification mail is sent. Banned emails are refused. If the email already belongs to an account, the player signs in with their password and links FiveM or Steam under **Account → Linked accounts**. Staff accounts need **Allow staff sign-in**, and two-factor authentication is never skipped.
- **What is stored:** the identity keeps the Cfx.re user id or SteamID64 and the forum username or Steam name.

**Limitations:**

- **No Rockstar licence from Cfx.re:** the forum's `session/current.json` returns the forum user (id, username), not a Rockstar/FiveM `license:` identifier. With **Game profiles** on, Login with FiveM fills the player's Cfx.re id (`fivem`), Login with Steam their SteamID64 (`steamid`) and Login with Discord their Discord id (`discord_id`) on their game accounts (empty fields only). The Rockstar licence is asked once at checkout when a package needs it and then remembered (see Admin → Modules → Game profiles).
- **Forum dependencies:** the forum must allow your callback URL (Discourse `allowed_user_api_auth_redirects`) and the player's forum trust level must allow user API keys. Cfx.re controls both, so if the forum refuses, the player sees an error on forum.cfx.re.

### SAML scope (Service Provider)

PeltyCMS implements a deliberately narrow SAML 2.0 SP: Exclusive C14N 1.0, enveloped XML-DSig with RSA-SHA256 / ECDSA-P256-SHA256 and SHA-256 digests, pinned IdP certificates, HTTP-POST responses and HTTP-Redirect requests. **Not supported:** EncryptedAssertion, SHA-1 signatures, inclusive C14N, XPath transforms, KeyInfo-supplied keys, or artifact binding. Prefer OIDC when the IdP offers it.

## Fraud screening

Admin → Security → **Fraud** scores every new client order. Each rule that matches adds points (the total is capped at 100). At the **review score** the order is held for a decision; at the **reject score** it is cancelled as fraud straight away and the client sees an error. Fraud addons can add their own scores; a provider that fails is ignored so an outage never blocks sales.

Built-in rules:

| Rule | Fires when | Default |
| --- | --- | --- |
| Blocked country | The billing country, or the connection country, is on your block list | 100 |
| IP / client velocity | Too many orders from one IP or one client in 24 hours | 60 / 50 |
| Large first order | A first order is above your limit | 50 |
| Disposable e-mail | The client's e-mail domain (or a parent domain) is a known throwaway provider or on your extra list | 40 |
| Country mismatch | The visitor's connection country differs from the billing country | 30 |
| Tor | The connection comes from the Tor network | 60 |
| Proxy headers | The request carries headers that forward proxies and anonymisers add (`Via`, `Forwarded`, `Proxy-Connection`, a multi-hop `X-Forwarded-For`, …) | 25 |
| VPN / hosting range | The client IP is inside one of your VPN or data-centre ranges | 40 |
| Shared IP | Several other clients ordered from the same IP in the last 30 days | 30 |
| No billing country | The client has not set a country | 10 |

Set any heuristic's score to 0 to switch it off. The connection country comes from your edge proxy. Cloudflare (`CF-IPCountry`, with `T1` for Tor), CloudFront (`CloudFront-Viewer-Country`) and nginx GeoIP (`X-GeoIP-Country` or `X-Country-Code`) are read **only** from a trusted proxy (Admin → Security → **IP rules**, trusted proxies); from anyone else the headers are removed so a visitor cannot fake them. Requests from a private or loopback address are not counted as proxied, so an nginx in front of PeltyCMS that is not yet listed as trusted does not flag every order.

### HTTP fraud network (optional)

Set **HTTP fraud hook URL** (and optional API key) to POST each screened order as JSON to MaxMind minFraud, FraudLabs Pro, or any custom scorer. The response should be `{ "hits": [ { "rule": "…", "score": 0-100 } ] }` or `{ "score": N, "rule": "…" }`. Failures are ignored (fail-open) so an outage never blocks sales.

### Review queue

The **Review queue** filter lists orders held for review that are still pending. Each row shows the matched rules, the connection and billing countries, any proxy headers, and the IP.

- **Approve** clears the fraud hold. The order then follows the other approval rules (for example "awaiting payment"). Tick **Also approve the order** to approve it and provision its services right away.
- **Reject as fraud** cancels the order with status *Fraud*, releases its coupon use and voids its unpaid invoice.

Approving the order anywhere else (the order page or the approval queue) also closes its review. Decisions record who made them, when, and an optional note; they appear in the audit log as `fraud.review_approved` / `fraud.review_rejected` and fire the `order.fraud_approved` / `order.fraud_rejected` webhook events. Reviewing needs `orders.manage`; changing the settings needs `settings.manage`. The API is `GET /api/v1/admin/fraud/checks?open=true` plus `POST /api/v1/admin/fraud/checks/{id}/approve` and `/reject`.
