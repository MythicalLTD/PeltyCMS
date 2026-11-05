-- Checkout fields declared by a product's provisioning module are synced
-- into product_custom_fields and tagged with the module's field key.
ALTER TABLE product_custom_fields ADD COLUMN module_key TEXT CHECK (module_key IS NULL OR module_key ~ '^[a-z0-9_]{1,40}$');
CREATE UNIQUE INDEX product_custom_fields_module_key_idx ON product_custom_fields (product_id, module_key) WHERE module_key IS NOT NULL;
