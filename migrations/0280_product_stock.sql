-- Product stock control: optional quantity limit with optional backorders.
ALTER TABLE products
    ADD COLUMN stock_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN stock_qty INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN allow_backorder BOOLEAN NOT NULL DEFAULT FALSE;
