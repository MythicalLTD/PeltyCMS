-- #418: 0220 meant to register the formerly host-native addons only on
-- existing installations, but its `payment_gateways` test is true on every
-- database (0002 seeds that table). A fresh install therefore started with
-- enabled `source = 'builtin'` rows that have no package behind them: the
-- module / gateway lists stayed empty and installing the real addon failed.
-- An installation without products and invoices never used them; drop the
-- placeholders so it starts with nothing installed, as intended. Rows that
-- were adopted by a package (source 'disk' or a real manifest hash) stay.
DELETE FROM installed_addons
WHERE source = 'builtin'
  AND manifest_hash = 'builtin'
  AND id IN (
    'provisioning-pterodactyl', 'provisioning-featherpanel', 'provisioning-pelican',
    'provisioning-webhook', 'provisioning-https-agent',
    'gateway-manual', 'gateway-stripe', 'gateway-paypal', 'gateway-mollie',
    'gateway-razorpay', 'gateway-crypto', 'gateway-paddle', 'gateway-adyen',
    'gateway-custom-http'
  )
  AND NOT EXISTS (SELECT 1 FROM products)
  AND NOT EXISTS (SELECT 1 FROM invoices);
