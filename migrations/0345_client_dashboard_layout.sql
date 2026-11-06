-- Per-user client dashboard layout (drag-and-drop order, hidden widgets, size).
CREATE TABLE client_dashboard_layouts (
    user_id    UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    widgets    JSONB NOT NULL DEFAULT '[]'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
