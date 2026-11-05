-- Configurable option types (WHMCS: dropdown, radio, quantity, yes/no).
-- quantity: the single choice's price is per unit; yesno: the single choice
-- is "yes" (absent = no).
ALTER TABLE product_config_options
    ADD COLUMN option_type TEXT NOT NULL DEFAULT 'dropdown'
        CHECK (option_type IN ('dropdown','radio','quantity','yesno')),
    ADD COLUMN qty_min INT NOT NULL DEFAULT 0 CHECK (qty_min >= 0),
    ADD COLUMN qty_max INT NOT NULL DEFAULT 100 CHECK (qty_max <= 100000),
    ADD CONSTRAINT product_config_options_qty_range CHECK (qty_max >= qty_min);
