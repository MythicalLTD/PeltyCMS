-- Store-package checkout fields can stand for a player identity field
-- (Rockstar licence, SteamID64, Discord id, Cfx.re id, player name...).
-- Such a field is filled from the buyer's linked game account, and a value
-- the buyer types is saved to that account when it had none.
ALTER TABLE product_custom_fields ADD COLUMN identity_key TEXT
    CHECK (identity_key IS NULL OR identity_key IN ('username','uuid','xuid','steamid','license','discord_id','fivem'));
