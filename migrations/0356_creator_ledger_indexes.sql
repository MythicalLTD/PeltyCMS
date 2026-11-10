-- Listing sale counts and refund reversal look up sales by product and invoice.
CREATE INDEX creator_ledger_product ON creator_ledger (product_id) WHERE kind = 'sale';
CREATE INDEX creator_ledger_invoice ON creator_ledger (invoice_id) WHERE kind = 'sale';
