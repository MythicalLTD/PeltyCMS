-- Read/write permission levels: every core resource gets a `<res>.read`
-- permission next to `<res>.manage`. `.manage` implies `.read` (resolved in
-- code), and GET/HEAD endpoints guarded by `<res>.manage` also accept
-- `<res>.read`, so custom roles and API keys can be read-only per resource.
INSERT INTO permissions (id, name)
SELECT gen_random_uuid(), r || '.read'
FROM unnest(ARRAY['customers','catalog','orders','invoices','services','tickets','coupons',
                  'affiliates','content','quotes','downloads','client_groups','campaigns']) AS r
ON CONFLICT (name) DO NOTHING;

-- Roles holding `<res>.manage` get `<res>.read` explicitly too, so the role
-- editor shows a consistent picture.
INSERT INTO role_permissions (role_id, permission_id)
SELECT rp.role_id, rd.id
FROM role_permissions rp
JOIN permissions pm ON pm.id = rp.permission_id AND pm.name LIKE '%.manage'
JOIN permissions rd ON rd.name = replace(pm.name, '.manage', '.read')
ON CONFLICT DO NOTHING;
