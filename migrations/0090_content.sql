-- Knowledge base and announcements. Content is per-locale; `locale` NULL
-- means "all languages". Only published rows are visible publicly.
CREATE TABLE kb_categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,79}$'),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    description TEXT NOT NULL DEFAULT '',
    sort_order INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE kb_articles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category_id UUID NOT NULL REFERENCES kb_categories(id) ON DELETE RESTRICT,
    slug TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,119}$'),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    body TEXT NOT NULL CHECK (length(body) <= 100000),
    locale TEXT,
    is_published BOOLEAN NOT NULL DEFAULT false,
    views BIGINT NOT NULL DEFAULT 0,
    helpful_yes BIGINT NOT NULL DEFAULT 0,
    helpful_no BIGINT NOT NULL DEFAULT 0,
    search TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple', title || ' ' || body)) STORED,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX kb_articles_cat ON kb_articles (category_id) WHERE is_published;
CREATE INDEX kb_articles_search ON kb_articles USING GIN (search);

CREATE TABLE announcements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    body TEXT NOT NULL CHECK (length(body) <= 50000),
    locale TEXT,
    is_published BOOLEAN NOT NULL DEFAULT false,
    published_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX announcements_pub ON announcements (published_at DESC) WHERE is_published;

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-000000000040', 'content.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-000000000040'),
  ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0001-000000000040');
