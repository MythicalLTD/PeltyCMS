-- Paymenter (v1) -> JSON export for `pelty-server import paymenter --file export.json`.
-- Read-only. Run:  mysql -N -r paymenter < scripts/export-paymenter.sql > export.json
-- Needs MySQL >= 5.7.22 / MariaDB >= 10.5. Money exported as strings (exact).
SET SESSION group_concat_max_len = 1073741824;
SELECT JSON_OBJECT(
 'currencies', (SELECT JSON_ARRAYAGG(JSON_OBJECT('code', code)) FROM currencies),
 'users', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'first_name', first_name, 'last_name', last_name, 'email', email, 'role_id', role_id)) FROM users),
 'credits', (SELECT JSON_ARRAYAGG(JSON_OBJECT('user_id', user_id, 'currency_code', currency_code, 'amount', CAST(amount AS CHAR))) FROM credits),
 'categories', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'name', name, 'sort', sort)) FROM categories),
 'products', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'category_id', category_id, 'name', name, 'description', description, 'hidden', hidden, 'sort', sort)) FROM products),
 'plans', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'priceable_id', priceable_id, 'priceable_type', priceable_type, 'type', type, 'billing_period', billing_period, 'billing_unit', billing_unit)) FROM plans),
 'prices', (SELECT JSON_ARRAYAGG(JSON_OBJECT('plan_id', plan_id, 'currency_code', currency_code, 'price', CAST(price AS CHAR), 'setup_fee', CAST(setup_fee AS CHAR))) FROM prices),
 'services', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'user_id', user_id, 'product_id', product_id, 'plan_id', plan_id, 'status', status, 'price', CAST(price AS CHAR), 'currency_code', currency_code, 'expires_at', expires_at, 'created_at', created_at)) FROM services),
 'invoices', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'number', number, 'user_id', user_id, 'status', status, 'currency_code', currency_code, 'due_at', due_at, 'created_at', created_at)) FROM invoices),
 'invoice_items', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'invoice_id', invoice_id, 'description', description, 'price', CAST(price AS CHAR), 'quantity', quantity, 'reference_id', reference_id, 'reference_type', reference_type)) FROM invoice_items),
 'invoice_transactions', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'invoice_id', invoice_id, 'gateway_id', gateway_id, 'amount', CAST(amount AS CHAR), 'fee', CAST(fee AS CHAR), 'transaction_id', transaction_id, 'status', status, 'created_at', created_at)) FROM invoice_transactions),
 'gateways', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'name', name, 'extension', extension)) FROM gateways),
 'tickets', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'user_id', user_id, 'subject', subject, 'status', status, 'priority', priority, 'department', department, 'created_at', created_at)) FROM tickets),
 'ticket_messages', (SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'ticket_id', ticket_id, 'user_id', user_id, 'message', message, 'created_at', created_at)) FROM ticket_messages)
);
