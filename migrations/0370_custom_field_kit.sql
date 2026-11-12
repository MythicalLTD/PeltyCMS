-- Custom-field kit: e-mail and password field kinds, and a stable key per
-- staff field so provisioning modules (ServiceCtx.custom_fields[key]) and
-- game delivery commands ({key}) can read it.
ALTER TABLE product_custom_fields DROP CONSTRAINT IF EXISTS product_custom_fields_kind_check;
ALTER TABLE product_custom_fields ADD CONSTRAINT product_custom_fields_kind_check
    CHECK (kind IN ('text','textarea','number','select','checkbox','email','password'));

ALTER TABLE product_custom_fields ADD COLUMN field_key TEXT
    CHECK (field_key IS NULL OR field_key ~ '^[a-z][a-z0-9_]{0,39}$');

-- Existing staff fields get a key: the identity field they stand for, else
-- the name as a slug (deduplicated per product).
WITH c AS (
    SELECT id, product_id,
           COALESCE(identity_key, NULLIF(left(trim(both '_' from regexp_replace(lower(name), '[^a-z0-9]+', '_', 'g')), 30), '')) AS base
      FROM product_custom_fields WHERE module_key IS NULL
), c2 AS (
    SELECT id, product_id,
           CASE WHEN base IS NULL THEN 'field' WHEN base !~ '^[a-z]' THEN 'field_' || base ELSE base END AS base
      FROM c
), n AS (
    SELECT id, base, row_number() OVER (PARTITION BY product_id, base ORDER BY id) AS rn FROM c2
)
UPDATE product_custom_fields f
   SET field_key = left(trim(both '_' from CASE WHEN n.rn = 1 THEN n.base ELSE n.base || '_' || n.rn END), 40)
  FROM n WHERE n.id = f.id;
CREATE INDEX product_custom_fields_key_idx ON product_custom_fields (product_id, field_key) WHERE field_key IS NOT NULL;
