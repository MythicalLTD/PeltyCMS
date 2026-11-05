-- Product addons (sold alongside a product) and custom order fields.

CREATE TABLE addons (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','hidden','retired')),
    sort_order  INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Price per currency + cycle. An addon is offered for a product cycle only when
-- a price exists for that cycle; a 'one_time' price applies to any cycle.
CREATE TABLE addon_prices (
    addon_id  UUID NOT NULL REFERENCES addons(id) ON DELETE CASCADE,
    currency  CHAR(3) NOT NULL REFERENCES currencies(code),
    cycle     TEXT NOT NULL CHECK (cycle IN ('one_time','monthly','quarterly','semiannual','annual','biennial','triennial')),
    amount    NUMERIC(19,4) NOT NULL CHECK (amount >= 0),
    setup_fee NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (setup_fee >= 0),
    PRIMARY KEY (addon_id, currency, cycle)
);

CREATE TABLE addon_products (
    addon_id   UUID NOT NULL REFERENCES addons(id) ON DELETE CASCADE,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    PRIMARY KEY (addon_id, product_id)
);
CREATE INDEX addon_products_product_idx ON addon_products (product_id);

CREATE TABLE order_item_addons (
    order_item_id UUID NOT NULL REFERENCES order_items(id) ON DELETE CASCADE,
    addon_id      UUID NOT NULL REFERENCES addons(id) ON DELETE RESTRICT,
    cycle         TEXT NOT NULL,
    amount        NUMERIC(19,4) NOT NULL CHECK (amount >= 0),
    setup_fee     NUMERIC(19,4) NOT NULL CHECK (setup_fee >= 0),
    PRIMARY KEY (order_item_id, addon_id)
);

CREATE TABLE service_addons (
    id               UUID PRIMARY KEY,
    service_id       UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    addon_id         UUID NOT NULL REFERENCES addons(id) ON DELETE RESTRICT,
    status           TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','suspended','cancelled')),
    cycle            TEXT NOT NULL,
    recurring_amount NUMERIC(19,4) NOT NULL CHECK (recurring_amount >= 0),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (service_id, addon_id)
);

CREATE TABLE product_custom_fields (
    id          UUID PRIMARY KEY,
    product_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    kind        TEXT NOT NULL CHECK (kind IN ('text','textarea','number','select','checkbox')),
    choices     JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(choices) = 'array'),
    pattern     TEXT NOT NULL DEFAULT '' CHECK (length(pattern) <= 200),
    required    BOOLEAN NOT NULL DEFAULT FALSE,
    admin_only  BOOLEAN NOT NULL DEFAULT FALSE,
    sort_order  INT NOT NULL DEFAULT 0
);
CREATE INDEX product_custom_fields_product_idx ON product_custom_fields (product_id, sort_order);

ALTER TABLE order_items ADD COLUMN custom_fields JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE services ADD COLUMN custom_fields JSONB NOT NULL DEFAULT '{}'::jsonb;
