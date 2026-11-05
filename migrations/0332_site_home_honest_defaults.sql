-- Replace the stock home page when it still ships the fabricated uptime stats.
UPDATE site_pages
SET
  description = 'Billing and client area for your hosting products.',
  blocks = '[
  {"type":"hero","props":{"eyebrow":"","title":"Hosting billing with a client area your customers can use","subtitle":"Sell game servers, web hosting, and VPS. Provision after payment, then let clients manage invoices and services in one place.","primary_label":"View plans","primary_href":"/store","secondary_label":"Client area","secondary_href":"/login"}},
  {"type":"features","props":{"title":"Built for hosting operators","columns":3,"items":[{"icon":"HardDrives","title":"Automatic provisioning","text":"Paid orders hand off to your connected control panels and modules."},{"icon":"ShieldCheck","title":"Staff and client security","text":"2FA, audit logs, and encrypted secrets for API keys and panel credentials."},{"icon":"Lifebuoy","title":"Tickets and knowledgebase","text":"Support stays in the same panel as billing and services."}]}},
  {"type":"pricing","props":{"title":"Plans","subtitle":"Catalog products from Admin appear here.","group":"","highlight":-1}},
  {"type":"faq","props":{"title":"Ordering","items":[{"q":"Can I change plans later?","a":"Yes. Upgrades and downgrades follow the pricing and proration rules you configure."},{"q":"Which payment methods work?","a":"Whatever gateways you enable under Admin → Billing → Gateways."}]}},
  {"type":"cta","props":{"title":"Open a client account","text":"Register, pick a plan, and pay. Provisioning starts after the invoice clears.","label":"Create account","href":"/register"}}
 ]'::jsonb,
  updated_at = now()
WHERE slug = 'home'
  AND blocks::text LIKE '%99.9%';
