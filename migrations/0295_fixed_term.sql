-- Fixed-term products: terminate a service N days after activation (WHMCS "Auto Terminate").
ALTER TABLE products ADD COLUMN auto_terminate_days INT CHECK (auto_terminate_days BETWEEN 1 AND 3650);
