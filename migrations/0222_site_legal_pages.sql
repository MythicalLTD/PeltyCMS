-- Default pages linked from the public footer (Company / Legal). Admins edit them in Website > Pages.
INSERT INTO site_pages (slug, title, description, published, blocks) VALUES
 ('about', 'About us', '', true, '[{"type":"text","props":{"title":"About us","body":"Tell visitors who you are. Edit this page in Admin > Website > Pages."}}]'),
 ('terms', 'Terms of Service', '', true, '[{"type":"text","props":{"title":"Terms of Service","body":"Replace this text with your Terms of Service. Edit it in Admin > Website > Pages."}}]'),
 ('privacy', 'Privacy Policy', '', true, '[{"type":"text","props":{"title":"Privacy Policy","body":"Replace this text with your Privacy Policy. Edit it in Admin > Website > Pages."}}]')
ON CONFLICT (slug) DO NOTHING;
