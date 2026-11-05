-- Blesta -> JSON export for `pelty-server import blesta --file export.json`.
-- Read-only. Run:  mysql -N -r blesta < scripts/export-blesta.sql > export.json
-- Needs MySQL >= 5.7.22 / MariaDB >= 10.5 (JSON_ARRAYAGG). Money is exported
-- as strings so it stays exact.
SET SESSION group_concat_max_len = 1073741824;
SELECT JSON_OBJECT(
 'currencies', (SELECT JSON_ARRAYAGG(JSON_OBJECT('code', code)) FROM currencies),
 'client_groups', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'name', name)) FROM client_groups),
 'clients', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'client_group_id', client_group_id, 'status', status)) FROM clients),
 'contacts', (SELECT JSON_ARRAYAGG(JSON_OBJECT('client_id', client_id, 'contact_type', contact_type, 'first_name', first_name, 'last_name', last_name, 'company', company, 'email', email, 'address1', address1, 'address2', address2, 'city', city, 'state', state, 'zip', zip, 'country', country)) FROM contacts),
 'package_groups', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', g.id, 'name', COALESCE((SELECT n.name FROM package_group_names n WHERE n.package_group_id = g.id ORDER BY n.lang = 'en_us' DESC LIMIT 1), CONCAT('Group ', g.id)))) FROM package_groups g),
 'package_group', (SELECT JSON_ARRAYAGG(JSON_OBJECT('package_id', package_id, 'package_group_id', package_group_id)) FROM package_group),
 'packages', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', p.id, 'status', p.status,
      'name', COALESCE((SELECT n.name FROM package_names n WHERE n.package_id = p.id ORDER BY n.lang = 'en_us' DESC LIMIT 1), CONCAT('Package ', p.id)),
      'description', COALESCE((SELECT d.text FROM package_descriptions d WHERE d.package_id = p.id ORDER BY d.lang = 'en_us' DESC LIMIT 1), ''),
      'module', COALESCE((SELECT m.class FROM modules m WHERE m.id = p.module_id), ''))) FROM packages p),
 'package_pricing', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'package_id', package_id, 'pricing_id', pricing_id)) FROM package_pricing),
 'pricings', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'term', term, 'period', period, 'price', CAST(price AS CHAR), 'setup_fee', CAST(setup_fee AS CHAR), 'currency', currency)) FROM pricings),
 'services', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', s.id, 'client_id', s.client_id, 'pricing_id', s.pricing_id, 'status', s.status, 'date_added', s.date_added, 'date_renews', s.date_renews, 'override_price', CAST(s.override_price AS CHAR),
      'name', COALESCE((SELECT f.value FROM service_fields f WHERE f.service_id = s.id AND f.`key` IN ('domain', 'hostname') LIMIT 1), ''))) FROM services s),
 'invoices', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'id_value', id_value, 'client_id', client_id, 'date_billed', date_billed, 'date_due', date_due, 'date_closed', date_closed, 'subtotal', CAST(subtotal AS CHAR), 'total', CAST(total AS CHAR), 'status', status, 'currency', currency)) FROM invoices),
 'invoice_lines', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'invoice_id', invoice_id, 'service_id', service_id, 'description', description, 'qty', CAST(qty AS CHAR), 'amount', CAST(amount AS CHAR))) FROM invoice_lines),
 'transactions', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', t.id, 'client_id', t.client_id, 'amount', CAST(t.amount AS CHAR), 'currency', t.currency, 'transaction_id', t.transaction_id, 'status', t.status, 'date_added', t.date_added,
      'gateway', COALESCE((SELECT g.class FROM gateways g WHERE g.id = t.gateway_id), ''))) FROM transactions t),
 'transaction_applied', (SELECT JSON_ARRAYAGG(JSON_OBJECT('transaction_id', transaction_id, 'invoice_id', invoice_id)) FROM transaction_applied),
 'support_departments', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'name', name, 'description', description)) FROM support_departments),
 'support_tickets', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'code', code, 'department_id', department_id, 'client_id', client_id, 'summary', summary, 'status', status, 'priority', priority, 'date_added', date_added)) FROM support_tickets),
 'support_replies', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'ticket_id', ticket_id, 'staff_id', staff_id, 'details', details, 'date_added', date_added, 'type', type)) FROM support_replies)
);
