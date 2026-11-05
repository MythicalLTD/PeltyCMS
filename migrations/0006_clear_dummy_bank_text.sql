-- 0002 seeded a placeholder IBAN into the manual gateway. Bank details now
-- live in system_settings (billing.bank_transfer); drop the placeholder only
-- if the operator never changed it.
UPDATE payment_gateways SET config = config - 'instructions'
WHERE id = 'manual' AND config->>'instructions' = 'Transfer to IBAN XX00 0000 0000, reference = payment id';
