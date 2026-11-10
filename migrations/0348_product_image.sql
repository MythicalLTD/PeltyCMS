-- Optional product image (game-store package art, plan thumbnails).
ALTER TABLE products
    ADD COLUMN image_url TEXT NOT NULL DEFAULT '' CHECK (length(image_url) <= 500);
