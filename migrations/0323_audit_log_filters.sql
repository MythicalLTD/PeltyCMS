-- Indexes behind the admin audit-log filters (action prefix, target, IP prefix, actor + time).
CREATE INDEX IF NOT EXISTS audit_logs_action_prefix_idx ON audit_logs (action text_pattern_ops);
CREATE INDEX IF NOT EXISTS audit_logs_target_idx ON audit_logs (target) WHERE target IS NOT NULL;
CREATE INDEX IF NOT EXISTS audit_logs_ip_idx ON audit_logs (ip text_pattern_ops) WHERE ip IS NOT NULL;
CREATE INDEX IF NOT EXISTS audit_logs_actor_created_idx ON audit_logs (actor_id, created_at DESC);
