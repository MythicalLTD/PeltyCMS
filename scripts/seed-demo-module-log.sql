-- Demo-only module log rows, after `cargo xtask seed` (psql -d <demo db> -f this). Idempotent via output.demo.
INSERT INTO module_log (service_id, server_id, module, action, source, ok, error, output, duration_ms, created_at)
SELECT s.id, sv.id, sv.module, a.action, 'queue', a.ok, a.err, a.out::jsonb || '{"demo":true}'::jsonb, a.ms, now() - a.ago
FROM (SELECT id FROM services ORDER BY created_at LIMIT 1) s, (SELECT id, module FROM servers ORDER BY created_at LIMIT 1) sv,
 (VALUES ('create', true, '', '{"remote_id":"1001"}', 812, interval '3 days'),
         ('suspend', true, '', '{}', 240, interval '2 days'),
         ('unsuspend', false, 'panel returned 502', '{}', 5003, interval '1 day'),
         ('unsuspend', true, '', '{}', 198, interval '23 hours')) a(action, ok, err, out, ms, ago)
WHERE NOT EXISTS (SELECT 1 FROM module_log WHERE output ? 'demo');
