-- Optional per-key IP allowlist (addresses or CIDR ranges). Empty = any IP.
ALTER TABLE api_keys ADD COLUMN allowed_ips TEXT[] NOT NULL DEFAULT '{}'
    CHECK (cardinality(allowed_ips) <= 20);
