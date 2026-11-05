-- Milestone 2: billing core. All money is NUMERIC(19,4) + ISO currency; the
-- currency's exponent decides rounding. Never floats.

CREATE TABLE currencies (
    code     CHAR(3) PRIMARY KEY CHECK (code ~ '^[A-Z]{3}$'),
    exponent SMALLINT NOT NULL CHECK (exponent BETWEEN 0 AND 4),
    enabled  BOOLEAN NOT NULL DEFAULT TRUE
);
INSERT INTO currencies (code, exponent) VALUES ('EUR',2),('USD',2),('RON',2),('GBP',2),('JPY',0),('KWD',3);

-- ---------------------------------------------------------------- customers
CREATE TABLE customers (
    id            UUID PRIMARY KEY,
    owner_user_id UUID UNIQUE REFERENCES users(id) ON DELETE SET NULL,
    organization  TEXT NOT NULL DEFAULT '' CHECK (length(organization) <= 200),
    first_name    TEXT NOT NULL DEFAULT '' CHECK (length(first_name) <= 100),
    last_name     TEXT NOT NULL DEFAULT '' CHECK (length(last_name) <= 100),
    email         CITEXT NOT NULL CHECK (length(email) <= 254),
    phone         TEXT NOT NULL DEFAULT '' CHECK (length(phone) <= 40),
    address_line1 TEXT NOT NULL DEFAULT '' CHECK (length(address_line1) <= 200),
    address_line2 TEXT NOT NULL DEFAULT '' CHECK (length(address_line2) <= 200),
    city          TEXT NOT NULL DEFAULT '' CHECK (length(city) <= 100),
    region        TEXT NOT NULL DEFAULT '' CHECK (length(region) <= 100),
    postal_code   TEXT NOT NULL DEFAULT '' CHECK (length(postal_code) <= 20),
    country       TEXT NOT NULL DEFAULT '' CHECK (country = '' OR country ~ '^[A-Z]{2}$'),
    tax_id        TEXT NOT NULL DEFAULT '' CHECK (length(tax_id) <= 40),
    currency      CHAR(3) NOT NULL DEFAULT 'EUR' REFERENCES currencies(code),
    status        TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','inactive','closed')),
    notes         TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),  -- staff-only
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX customers_created_idx ON customers (created_at DESC, id);
CREATE INDEX customers_email_idx ON customers (email);

CREATE TABLE customer_contacts (
    id          UUID PRIMARY KEY,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    email       CITEXT NOT NULL CHECK (length(email) <= 254),
    phone       TEXT NOT NULL DEFAULT '' CHECK (length(phone) <= 40),
    is_billing  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX customer_contacts_customer_idx ON customer_contacts (customer_id);

-- ------------------------------------------------------------------ catalog
CREATE TABLE product_groups (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    slug       TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,63}$'),
    sort_order INT NOT NULL DEFAULT 0,
    hidden     BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE products (
    id            UUID PRIMARY KEY,
    group_id      UUID NOT NULL REFERENCES product_groups(id) ON DELETE RESTRICT,
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    slug          TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,63}$'),
    description   TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 5000),
    status        TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','hidden','retired')),
    module        TEXT CHECK (module IS NULL OR module ~ '^[a-z][a-z0-9_-]{1,63}$'),  -- provisioning module id
    module_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    sort_order    INT NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX products_group_idx ON products (group_id, sort_order);

CREATE TABLE product_prices (
    id         UUID PRIMARY KEY,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    currency   CHAR(3) NOT NULL REFERENCES currencies(code),
    cycle      TEXT NOT NULL CHECK (cycle IN ('one_time','monthly','quarterly','semiannual','annual','biennial','triennial')),
    amount     NUMERIC(19,4) NOT NULL CHECK (amount >= 0),
    setup_fee  NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (setup_fee >= 0),
    UNIQUE (product_id, currency, cycle)
);

CREATE TABLE product_config_options (
    id         UUID PRIMARY KEY,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    required   BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INT NOT NULL DEFAULT 0
);
CREATE INDEX product_config_options_product_idx ON product_config_options (product_id);

CREATE TABLE product_config_choices (
    id        UUID PRIMARY KEY,
    option_id UUID NOT NULL REFERENCES product_config_options(id) ON DELETE CASCADE,
    label     TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 120),
    sort_order INT NOT NULL DEFAULT 0
);
CREATE INDEX product_config_choices_option_idx ON product_config_choices (option_id);

-- Price delta of a choice per currency + cycle (missing row = free).
CREATE TABLE product_config_prices (
    choice_id UUID NOT NULL REFERENCES product_config_choices(id) ON DELETE CASCADE,
    currency  CHAR(3) NOT NULL REFERENCES currencies(code),
    cycle     TEXT NOT NULL,
    amount    NUMERIC(19,4) NOT NULL CHECK (amount >= 0),
    PRIMARY KEY (choice_id, currency, cycle)
);

-- ------------------------------------------------------------------- orders
CREATE SEQUENCE order_number_seq START 1000;
CREATE TABLE orders (
    id          UUID PRIMARY KEY,
    number      BIGINT NOT NULL UNIQUE DEFAULT nextval('order_number_seq'),
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','active','cancelled','fraud')),
    currency    CHAR(3) NOT NULL REFERENCES currencies(code),
    total       NUMERIC(19,4) NOT NULL CHECK (total >= 0),
    invoice_id  UUID,
    approved_at TIMESTAMPTZ,
    approved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX orders_customer_idx ON orders (customer_id, created_at DESC);
CREATE INDEX orders_created_idx ON orders (created_at DESC, id);

CREATE TABLE order_items (
    id         UUID PRIMARY KEY,
    order_id   UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    cycle      TEXT NOT NULL,
    amount     NUMERIC(19,4) NOT NULL CHECK (amount >= 0),     -- recurring incl. options
    setup_fee  NUMERIC(19,4) NOT NULL CHECK (setup_fee >= 0),
    config     JSONB NOT NULL DEFAULT '{}'::jsonb              -- option_id -> choice_id
);
CREATE INDEX order_items_order_idx ON order_items (order_id);

-- ----------------------------------------------------------------- invoices
CREATE TABLE invoice_sequences (
    name TEXT PRIMARY KEY,
    next BIGINT NOT NULL CHECK (next > 0)
);
INSERT INTO invoice_sequences VALUES ('invoice', 1), ('credit_note', 1);

CREATE TABLE invoices (
    id           UUID PRIMARY KEY,
    number       TEXT UNIQUE,                       -- assigned at finalization, gapless
    kind         TEXT NOT NULL DEFAULT 'invoice' CHECK (kind IN ('invoice','credit_note')),
    credit_for   UUID REFERENCES invoices(id),
    customer_id  UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    order_id     UUID REFERENCES orders(id) ON DELETE SET NULL,
    currency     CHAR(3) NOT NULL REFERENCES currencies(code),
    status       TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','unpaid','paid','void','refunded')),
    subtotal     NUMERIC(19,4) NOT NULL DEFAULT 0,
    tax_total    NUMERIC(19,4) NOT NULL DEFAULT 0,
    total        NUMERIC(19,4) NOT NULL DEFAULT 0,
    amount_paid  NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (amount_paid >= 0),
    issued_at    TIMESTAMPTZ,
    due_at       TIMESTAMPTZ,
    paid_at      TIMESTAMPTZ,
    finalized_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((finalized_at IS NULL) = (number IS NULL)),
    CHECK (status = 'draft' OR finalized_at IS NOT NULL),
    CHECK (kind = 'invoice' OR credit_for IS NOT NULL)
);
ALTER TABLE orders ADD CONSTRAINT orders_invoice_fk FOREIGN KEY (invoice_id) REFERENCES invoices(id) ON DELETE SET NULL;
CREATE INDEX invoices_customer_idx ON invoices (customer_id, created_at DESC);
CREATE INDEX invoices_created_idx ON invoices (created_at DESC, id);
CREATE INDEX invoices_unpaid_idx ON invoices (due_at) WHERE status = 'unpaid';

CREATE TABLE invoice_items (
    id           UUID PRIMARY KEY,
    invoice_id   UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    description  TEXT NOT NULL CHECK (length(description) BETWEEN 1 AND 500),
    quantity     NUMERIC(19,4) NOT NULL CHECK (quantity > 0),
    unit_price   NUMERIC(19,4) NOT NULL,
    tax_rate     NUMERIC(7,4) NOT NULL DEFAULT 0 CHECK (tax_rate >= 0 AND tax_rate <= 100),
    amount       NUMERIC(19,4) NOT NULL,            -- rounded quantity * unit_price
    tax_amount   NUMERIC(19,4) NOT NULL DEFAULT 0,
    service_id   UUID,
    period_start DATE,
    period_end   DATE,
    sort_order   INT NOT NULL DEFAULT 0
);
CREATE INDEX invoice_items_invoice_idx ON invoice_items (invoice_id);

-- Finalized invoices are immutable: only status/payment fields may change,
-- items cannot be touched, and finalized invoices cannot be deleted.
CREATE FUNCTION invoices_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.finalized_at IS NOT NULL THEN RAISE EXCEPTION 'finalized invoice cannot be deleted'; END IF;
        RETURN OLD;
    END IF;
    IF OLD.finalized_at IS NOT NULL AND (
        NEW.number IS DISTINCT FROM OLD.number OR NEW.kind <> OLD.kind
        OR NEW.credit_for IS DISTINCT FROM OLD.credit_for OR NEW.customer_id <> OLD.customer_id
        OR NEW.currency <> OLD.currency OR NEW.subtotal <> OLD.subtotal OR NEW.tax_total <> OLD.tax_total
        OR NEW.total <> OLD.total OR NEW.issued_at IS DISTINCT FROM OLD.issued_at
        OR NEW.finalized_at IS DISTINCT FROM OLD.finalized_at OR NEW.status = 'draft'
    ) THEN
        RAISE EXCEPTION 'finalized invoice is immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER invoices_guard BEFORE UPDATE OR DELETE ON invoices FOR EACH ROW EXECUTE FUNCTION invoices_guard();

CREATE FUNCTION invoice_items_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE inv UUID; fin TIMESTAMPTZ;
BEGIN
    inv := CASE WHEN TG_OP = 'DELETE' THEN OLD.invoice_id ELSE NEW.invoice_id END;
    SELECT finalized_at INTO fin FROM invoices WHERE id = inv;
    IF fin IS NOT NULL THEN RAISE EXCEPTION 'items of a finalized invoice are immutable'; END IF;
    IF TG_OP = 'UPDATE' AND OLD.invoice_id <> NEW.invoice_id THEN
        SELECT finalized_at INTO fin FROM invoices WHERE id = OLD.invoice_id;
        IF fin IS NOT NULL THEN RAISE EXCEPTION 'items of a finalized invoice are immutable'; END IF;
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END $$;
CREATE TRIGGER invoice_items_guard BEFORE INSERT OR UPDATE OR DELETE ON invoice_items
    FOR EACH ROW EXECUTE FUNCTION invoice_items_guard();

-- Payments link to invoices; one provider reference / idempotency key each.
ALTER TABLE payments ADD COLUMN invoice_id UUID REFERENCES invoices(id) ON DELETE RESTRICT;
ALTER TABLE payments ADD COLUMN customer_id UUID REFERENCES customers(id) ON DELETE RESTRICT;
ALTER TABLE payments ADD COLUMN applied_at TIMESTAMPTZ;   -- set once when credited to the invoice
CREATE INDEX payments_invoice_idx ON payments (invoice_id);

-- ----------------------------------------------------------------- services
CREATE TABLE services (
    id               UUID PRIMARY KEY,
    customer_id      UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    product_id       UUID NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    order_item_id    UUID UNIQUE REFERENCES order_items(id) ON DELETE SET NULL,
    status           TEXT NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending','active','suspended','terminated','cancelled','fraud')),
    cycle            TEXT NOT NULL,
    currency         CHAR(3) NOT NULL REFERENCES currencies(code),
    recurring_amount NUMERIC(19,4) NOT NULL CHECK (recurring_amount >= 0),
    next_due_date    DATE,
    suspend_reason   TEXT NOT NULL DEFAULT '',
    activated_at     TIMESTAMPTZ,
    terminated_at    TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX services_customer_idx ON services (customer_id, created_at DESC);
CREATE INDEX services_created_idx ON services (created_at DESC, id);
CREATE INDEX services_due_idx ON services (next_due_date) WHERE status IN ('active','suspended');

-- Lifecycle is enforced in the DB too (mirrors pelty_billing::lifecycle).
CREATE FUNCTION services_lifecycle_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status = OLD.status THEN RETURN NEW; END IF;
    IF (OLD.status, NEW.status) IN (
        ('pending','active'), ('pending','cancelled'), ('pending','fraud'),
        ('active','suspended'), ('active','terminated'), ('active','cancelled'),
        ('suspended','active'), ('suspended','terminated'), ('suspended','cancelled')
    ) THEN RETURN NEW; END IF;
    RAISE EXCEPTION 'illegal service transition % -> %', OLD.status, NEW.status;
END $$;
CREATE TRIGGER services_lifecycle_guard BEFORE UPDATE OF status ON services
    FOR EACH ROW EXECUTE FUNCTION services_lifecycle_guard();

ALTER TABLE invoice_items ADD CONSTRAINT invoice_items_service_fk FOREIGN KEY (service_id) REFERENCES services(id) ON DELETE SET NULL;
-- A service period is billed at most once (recurring job idempotency).
CREATE UNIQUE INDEX invoice_items_service_period_idx ON invoice_items (service_id, period_start) WHERE service_id IS NOT NULL;

-- --------------------------------------------------------------------- jobs
CREATE TABLE jobs (
    id              BIGSERIAL PRIMARY KEY,
    kind            TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_.]{1,80}$'),
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    status          TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','succeeded','failed','dead')),
    attempts        INT NOT NULL DEFAULT 0,
    max_attempts    INT NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
    run_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_at       TIMESTAMPTZ,
    locked_by       TEXT,
    last_error      TEXT,
    idempotency_key TEXT UNIQUE,
    result          JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ
);
CREATE INDEX jobs_ready_idx ON jobs (run_at, id) WHERE status = 'queued';
CREATE INDEX jobs_running_idx ON jobs (locked_at) WHERE status = 'running';
CREATE INDEX jobs_created_idx ON jobs (created_at DESC, id);

-- -------------------------------------------------------------- permissions
INSERT INTO permissions (id, name) VALUES
  ('00000000-0000-0000-0001-000000000009', 'customers.manage'),
  ('00000000-0000-0000-0001-000000000010', 'catalog.manage'),
  ('00000000-0000-0000-0001-000000000011', 'orders.manage'),
  ('00000000-0000-0000-0001-000000000012', 'invoices.manage'),
  ('00000000-0000-0000-0001-000000000013', 'services.manage'),
  ('00000000-0000-0000-0001-000000000014', 'jobs.manage');
INSERT INTO role_permissions (role_id, permission_id)
  SELECT '00000000-0000-0000-0000-000000000001'::uuid, id FROM permissions
  WHERE name IN ('customers.manage','catalog.manage','orders.manage','invoices.manage','services.manage','jobs.manage');
INSERT INTO role_permissions (role_id, permission_id)
  SELECT '00000000-0000-0000-0000-000000000002'::uuid, id FROM permissions
  WHERE name IN ('customers.manage','orders.manage','invoices.manage','services.manage');
