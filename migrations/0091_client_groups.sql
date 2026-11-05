-- Client groups (WHMCS "Client Groups"): label colour, a percentage discount
-- applied by pricing code that opts in, and an optional "exempt from late fees" flag.
CREATE TABLE client_groups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE CHECK (length(name) BETWEEN 1 AND 80),
    color TEXT NOT NULL DEFAULT 'slate' CHECK (color IN ('slate','red','orange','amber','green','teal','blue','violet','pink')),
    discount_percent NUMERIC(5,2) NOT NULL DEFAULT 0 CHECK (discount_percent >= 0 AND discount_percent <= 100),
    late_fee_exempt BOOLEAN NOT NULL DEFAULT false,
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE customers ADD COLUMN client_group_id UUID REFERENCES client_groups(id) ON DELETE SET NULL;
CREATE INDEX customers_client_group ON customers (client_group_id) WHERE client_group_id IS NOT NULL;

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-000000000091', 'client_groups.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-000000000091');
