-- Public website CMS pages built from declarative blocks.
CREATE TABLE site_pages (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug        TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,63}$'),
    title       TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description TEXT NOT NULL DEFAULT '',
    blocks      JSONB NOT NULL DEFAULT '[]'::jsonb,
    published   BOOLEAN NOT NULL DEFAULT false,
    show_in_nav BOOLEAN NOT NULL DEFAULT false,
    sort_order  INT NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO site_pages (slug, title, description, published, blocks) VALUES (
 'home', 'Home', 'Billing and client area for your hosting products.', true,
 '[
  {"type":"hero","props":{"eyebrow":"","title":"Hosting billing with a client area your customers can use","subtitle":"Sell game servers, web hosting, and VPS. Provision after payment, then let clients manage invoices and services in one place.","primary_label":"View plans","primary_href":"/store","secondary_label":"Client area","secondary_href":"/login"}},
  {"type":"features","props":{"title":"Built for hosting operators","columns":3,"items":[{"icon":"HardDrives","title":"Automatic provisioning","text":"Paid orders hand off to your connected control panels and modules."},{"icon":"ShieldCheck","title":"Staff and client security","text":"2FA, audit logs, and encrypted secrets for API keys and panel credentials."},{"icon":"Lifebuoy","title":"Tickets and knowledgebase","text":"Support stays in the same panel as billing and services."}]}},
  {"type":"pricing","props":{"title":"Plans","subtitle":"Catalog products from Admin appear here.","group":"","highlight":-1}},
  {"type":"faq","props":{"title":"Ordering","items":[{"q":"Can I change plans later?","a":"Yes. Upgrades and downgrades follow the pricing and proration rules you configure."},{"q":"Which payment methods work?","a":"Whatever gateways you enable under Admin → Billing → Gateways."}]}},
  {"type":"cta","props":{"title":"Open a client account","text":"Register, pick a plan, and pay. Provisioning starts after the invoice clears.","label":"Create account","href":"/register"}}
 ]'::jsonb)
ON CONFLICT (slug) DO NOTHING;
