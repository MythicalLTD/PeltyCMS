-- Sub-account permissions gain "orders" and "domains" (WHMCS Users & Accounts).
ALTER TABLE customer_subaccounts DROP CONSTRAINT IF EXISTS customer_subaccounts_permissions_check;
ALTER TABLE customer_subaccounts ADD CONSTRAINT customer_subaccounts_permissions_check
    CHECK (permissions <@ ARRAY['invoices','tickets','services','orders','domains']::text[]);
ALTER TABLE customer_subaccounts ADD COLUMN invited_at TIMESTAMPTZ;
