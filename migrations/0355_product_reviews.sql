-- Product reviews and Q&A (optional `product_reviews` module).
-- One review per customer per product; staff moderate reviews and questions.
CREATE TABLE product_reviews (
    id          UUID PRIMARY KEY,
    product_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    rating      SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
    title       TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 120),
    body        TEXT NOT NULL DEFAULT '' CHECK (length(body) <= 5000),
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    verified    BOOLEAN NOT NULL DEFAULT false,
    reply       TEXT NOT NULL DEFAULT '' CHECK (length(reply) <= 5000),
    replied_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (product_id, customer_id)
);
CREATE INDEX product_reviews_product_idx ON product_reviews (product_id, status, created_at DESC);
CREATE INDEX product_reviews_status_idx ON product_reviews (status, created_at);

CREATE TABLE product_questions (
    id          UUID PRIMARY KEY,
    product_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    body        TEXT NOT NULL CHECK (length(body) BETWEEN 3 AND 2000),
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    answer      TEXT NOT NULL DEFAULT '' CHECK (length(answer) <= 5000),
    answered_by UUID REFERENCES users(id) ON DELETE SET NULL,
    answered_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX product_questions_product_idx ON product_questions (product_id, status, created_at DESC);
CREATE INDEX product_questions_status_idx ON product_questions (status, created_at);
