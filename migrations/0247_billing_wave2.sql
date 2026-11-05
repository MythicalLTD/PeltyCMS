-- Billing wave 2: full ISO 4217 currency list, currency display format,
-- automatic exchange-rate updates (configurable source + markup), invoice
-- merge/split bookkeeping and gift cards / prepaid credit codes.

-- All active ISO 4217 codes with their minor-unit exponent; new ones disabled.
INSERT INTO currencies (code, exponent, enabled) VALUES ('AED',2,FALSE),('AFN',2,FALSE),('ALL',2,FALSE),('AMD',2,FALSE),('ANG',2,FALSE),('AOA',2,FALSE),('ARS',2,FALSE),('AUD',2,FALSE),('AWG',2,FALSE),('AZN',2,FALSE),('BAM',2,FALSE),('BBD',2,FALSE),('BDT',2,FALSE),('BGN',2,FALSE),('BHD',3,FALSE),('BIF',0,FALSE),('BMD',2,FALSE),('BND',2,FALSE),('BOB',2,FALSE),('BRL',2,FALSE),('BSD',2,FALSE),('BTN',2,FALSE),('BWP',2,FALSE),('BYN',2,FALSE),('BZD',2,FALSE),('CAD',2,FALSE),('CDF',2,FALSE),('CHF',2,FALSE),('CLF',4,FALSE),('CLP',0,FALSE),('CNY',2,FALSE),('COP',2,FALSE),('CRC',2,FALSE),('CUP',2,FALSE),('CVE',2,FALSE),('CZK',2,FALSE),('DJF',0,FALSE),('DKK',2,FALSE),('DOP',2,FALSE),('DZD',2,FALSE),('EGP',2,FALSE),('ERN',2,FALSE),('ETB',2,FALSE),('EUR',2,FALSE),('FJD',2,FALSE),('FKP',2,FALSE),('GBP',2,FALSE),('GEL',2,FALSE),('GHS',2,FALSE),('GIP',2,FALSE),('GMD',2,FALSE),('GNF',0,FALSE),('GTQ',2,FALSE),('GYD',2,FALSE),('HKD',2,FALSE),('HNL',2,FALSE),('HTG',2,FALSE),('HUF',2,FALSE),('IDR',2,FALSE),('ILS',2,FALSE),('INR',2,FALSE),('IQD',3,FALSE),('IRR',2,FALSE),('ISK',0,FALSE),('JMD',2,FALSE),('JOD',3,FALSE),('JPY',0,FALSE),('KES',2,FALSE),('KGS',2,FALSE),('KHR',2,FALSE),('KMF',0,FALSE),('KPW',2,FALSE),('KRW',0,FALSE),('KWD',3,FALSE),('KYD',2,FALSE),('KZT',2,FALSE),('LAK',2,FALSE),('LBP',2,FALSE),('LKR',2,FALSE),('LRD',2,FALSE),('LSL',2,FALSE),('LYD',3,FALSE),('MAD',2,FALSE),('MDL',2,FALSE),('MGA',2,FALSE),('MKD',2,FALSE),('MMK',2,FALSE),('MNT',2,FALSE),('MOP',2,FALSE),('MRU',2,FALSE),('MUR',2,FALSE),('MVR',2,FALSE),('MWK',2,FALSE),('MXN',2,FALSE),('MYR',2,FALSE),('MZN',2,FALSE),('NAD',2,FALSE),('NGN',2,FALSE),('NIO',2,FALSE),('NOK',2,FALSE),('NPR',2,FALSE),('NZD',2,FALSE),('OMR',3,FALSE),('PAB',2,FALSE),('PEN',2,FALSE),('PGK',2,FALSE),('PHP',2,FALSE),('PKR',2,FALSE),('PLN',2,FALSE),('PYG',0,FALSE),('QAR',2,FALSE),('RON',2,FALSE),('RSD',2,FALSE),('RUB',2,FALSE),('RWF',0,FALSE),('SAR',2,FALSE),('SBD',2,FALSE),('SCR',2,FALSE),('SDG',2,FALSE),('SEK',2,FALSE),('SGD',2,FALSE),('SHP',2,FALSE),('SLE',2,FALSE),('SOS',2,FALSE),('SRD',2,FALSE),('SSP',2,FALSE),('STN',2,FALSE),('SVC',2,FALSE),('SYP',2,FALSE),('SZL',2,FALSE),('THB',2,FALSE),('TJS',2,FALSE),('TMT',2,FALSE),('TND',3,FALSE),('TOP',2,FALSE),('TRY',2,FALSE),('TTD',2,FALSE),('TWD',2,FALSE),('TZS',2,FALSE),('UAH',2,FALSE),('UGX',0,FALSE),('USD',2,FALSE),('UYU',2,FALSE),('UYW',4,FALSE),('UZS',2,FALSE),('VES',2,FALSE),('VND',0,FALSE),('VUV',0,FALSE),('WST',2,FALSE),('XAF',0,FALSE),('XCD',2,FALSE),('XOF',0,FALSE),('XPF',0,FALSE),('YER',2,FALSE),('ZAR',2,FALSE),('ZMW',2,FALSE),('ZWG',2,FALSE)
ON CONFLICT (code) DO NOTHING;

ALTER TABLE currencies
    ADD COLUMN decimal_sep   TEXT NOT NULL DEFAULT '.' CHECK (decimal_sep IN ('.', ',')),
    ADD COLUMN thousands_sep TEXT NOT NULL DEFAULT ',' CHECK (thousands_sep IN (',', '.', ' ', '''', '')),
    ADD COLUMN auto_update   BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN rate_source   NUMERIC(20,10),            -- last raw rate from the feed (before markup)
    ADD COLUMN rate_updated_at TIMESTAMPTZ;

-- Exchange-rate feed settings (single row).
CREATE TABLE currency_rate_settings (
    id             BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    enabled        BOOLEAN NOT NULL DEFAULT FALSE,
    source         TEXT NOT NULL DEFAULT 'ecb' CHECK (source IN ('ecb', 'json')),
    url            TEXT NOT NULL DEFAULT 'https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml' CHECK (url ~ '^https://' AND length(url) <= 500),
    markup_percent NUMERIC(7,4) NOT NULL DEFAULT 0 CHECK (markup_percent BETWEEN -50 AND 100),
    update_prices  BOOLEAN NOT NULL DEFAULT FALSE,     -- re-run price sync after an update
    last_run_at    TIMESTAMPTZ,
    last_error     TEXT
);
INSERT INTO currency_rate_settings DEFAULT VALUES;

CREATE TABLE currency_rate_history (
    id         BIGSERIAL PRIMARY KEY,
    code       CHAR(3) NOT NULL REFERENCES currencies(code) ON DELETE CASCADE,
    old_rate   NUMERIC(20,10) NOT NULL,
    new_rate   NUMERIC(20,10) NOT NULL,
    source     TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX currency_rate_history_code_idx ON currency_rate_history (code, created_at DESC);

-- Merge/split: finalized items are immutable, so merged/split invoices get
-- *copies* of the lines and the sources are voided. A copy does not claim the
-- service period again (the original line keeps the claim), which replaces
-- the old blanket unique index.
ALTER TABLE invoice_items ADD COLUMN copied_from UUID REFERENCES invoice_items(id) ON DELETE SET NULL;
DROP INDEX invoice_items_service_period_idx;
CREATE UNIQUE INDEX invoice_items_service_period_idx ON invoice_items (service_id, period_start)
    WHERE service_id IS NOT NULL AND copied_from IS NULL;
CREATE INDEX invoice_items_service_idx ON invoice_items (service_id) WHERE service_id IS NOT NULL;

ALTER TABLE invoices
    ADD COLUMN merged_into UUID REFERENCES invoices(id) ON DELETE SET NULL,
    ADD COLUMN split_from  UUID REFERENCES invoices(id) ON DELETE SET NULL,
    ADD COLUMN admin_notes TEXT NOT NULL DEFAULT '' CHECK (length(admin_notes) <= 10000),
    ADD COLUMN client_notes TEXT NOT NULL DEFAULT '' CHECK (length(client_notes) <= 5000);

-- Gift cards / prepaid credit codes: redeeming adds account credit.
CREATE TABLE gift_cards (
    id          UUID PRIMARY KEY,
    code_hash   TEXT NOT NULL UNIQUE,                  -- sha256 hex of the normalized code
    code_last4  TEXT NOT NULL CHECK (length(code_last4) = 4),
    currency    CHAR(3) NOT NULL REFERENCES currencies(code),
    amount      NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    balance     NUMERIC(19,4) NOT NULL CHECK (balance >= 0 AND balance <= amount),
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'redeemed', 'disabled')),
    note        TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    expires_at  TIMESTAMPTZ,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    redeemed_by UUID REFERENCES customers(id) ON DELETE SET NULL,
    redeemed_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX gift_cards_created_idx ON gift_cards (created_at DESC);

ALTER TABLE credit_ledger DROP CONSTRAINT IF EXISTS credit_ledger_kind_check;
ALTER TABLE credit_ledger ADD CONSTRAINT credit_ledger_kind_check CHECK (kind IN ('adjustment','applied','gift_card','add_funds','refund'));

-- EU VAT number verification (VIES) result, frozen per number.
ALTER TABLE customers
    ADD COLUMN vat_status      TEXT NOT NULL DEFAULT 'unchecked' CHECK (vat_status IN ('unchecked','valid','invalid')),
    ADD COLUMN vat_checked_num TEXT NOT NULL DEFAULT '',
    ADD COLUMN vat_checked_at  TIMESTAMPTZ,
    ADD COLUMN vat_name        TEXT NOT NULL DEFAULT '' CHECK (length(vat_name) <= 300);
