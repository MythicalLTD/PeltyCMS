-- Downloads area: files stored in the database (no filesystem paths), gated
-- by owning an active service of one of the linked products. No linked
-- products = available to every signed-in customer.
CREATE TABLE downloads (
    id           UUID PRIMARY KEY,
    title        TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description  TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    filename     TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 120),
    content_type TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL CHECK (size_bytes > 0 AND size_bytes <= 26214400),
    sha256       BYTEA NOT NULL,
    data         BYTEA NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE download_products (
    download_id UUID NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    product_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    PRIMARY KEY (download_id, product_id)
);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000a0f03', 'downloads.manage') ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
  SELECT r.id, p.id FROM roles r, permissions p WHERE r.name = 'admin' AND p.name = 'downloads.manage'
  ON CONFLICT DO NOTHING;
