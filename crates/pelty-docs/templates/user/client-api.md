# API access

## API keys

**API keys** (`/api-keys`) creates personal tokens to automate your account through the PeltyCMS API, for example to list invoices or order services from your own scripts.

- Give the key a name, choose what it may do (scopes) and optionally an expiry (1 to 3650 days).
- The key is shown **once**. Store it like a password.
- Send it as a header: `Authorization: Bearer pelty_…`.
- A key cannot sign in, change your password or manage other keys.
- Revoke a key at any time; the list shows when and from which IP address each key was last used.

```bash
curl -H "Authorization: Bearer $PELTY_KEY" https://billing.example.com/api/v1/client/invoices
```

## Headless ordering

The headless API lets your own shop or app sell your provider's products with an API key: read the catalog, build a cart, check out, pay the invoice and follow the order.

{{routes:/headless}}

## Connected apps

**Connected apps** (`/connected-apps`) lists apps you allowed to sign in with your account. Revoke access to any you no longer use.

The full API reference is in the [developer docs](developers/api.md).
