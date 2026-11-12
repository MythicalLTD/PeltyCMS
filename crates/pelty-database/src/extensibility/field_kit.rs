//! Custom-field kit: presets staff apply to a product / store package in one
//! click, field keys, and how field values reach provisioning modules and
//! game delivery commands.
//!
//! * Every field has a key (`module_key` for module-declared fields,
//!   otherwise `field_key`). Provisioning modules read
//!   `ServiceCtx.custom_fields[key]`; delivery commands use `{key}`.
//! * `password` values are sealed at rest (`enc:v1:` + hex, AES-GCM with
//!   `PELTY_ENCRYPTION_KEY`) and only opened for the module call or the
//!   delivery command being sent. APIs show them as [`REDACTED`].
use crate::addons::{CustomField, FIELD_COLS, IDENTITY_KEYS};
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::PgPool;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use uuid::Uuid;

/// How a stored password reads in API responses.
pub const REDACTED: &str = "********";
const SEALED_PREFIX: &str = "enc:v1:";

/// Seals password values. Installed once by the API layer from the
/// configured encryption key.
pub trait FieldSealer: Send + Sync {
    fn seal(&self, plain: &[u8]) -> Vec<u8>;
    fn open(&self, sealed: &[u8]) -> Option<Vec<u8>>;
}

static SEALER: OnceLock<Box<dyn FieldSealer>> = OnceLock::new();

/// Install the sealer (first call wins; later calls are ignored).
pub fn install_sealer(s: Box<dyn FieldSealer>) {
    let _ = SEALER.set(s);
}

/// Seal a password for storage. Without an encryption key the value is kept
/// as is (still redacted in every API response).
pub fn seal(plain: &str) -> String {
    match SEALER.get() {
        Some(s) => format!("{SEALED_PREFIX}{}", hex::encode(s.seal(plain.as_bytes()))),
        None => plain.to_string(),
    }
}

/// Open a stored password. A sealed value that cannot be opened reads empty.
pub fn open(stored: &str) -> String {
    let Some(h) = stored.strip_prefix(SEALED_PREFIX) else {
        return stored.to_string();
    };
    hex::decode(h)
        .ok()
        .and_then(|b| SEALER.get()?.open(&b))
        .and_then(|p| String::from_utf8(p).ok())
        .unwrap_or_default()
}

/// Simple e-mail shape check: `local@domain.tld`, no spaces.
pub fn email_ok(v: &str) -> bool {
    let Some((local, domain)) = v.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && v.len() <= 254
        && !v.chars().any(|c| c.is_whitespace() || c.is_control())
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
}

/// Keys the delivery system fills itself; a field cannot take them.
pub const RESERVED_KEYS: &[&str] = &[
    "package",
    "package_id",
    "service_id",
    "order_id",
    "customer_id",
    "amount",
    "currency",
    "server",
    "player",
    "site",
    "uuid_nodash",
];

pub fn key_ok(k: &str) -> bool {
    let mut c = k.chars();
    c.next().is_some_and(|f| f.is_ascii_lowercase())
        && k.len() <= 40
        && c.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == '_')
}

/// A key from a field name: `Player license` → `player_license`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let mut out: String = out.trim_matches('_').chars().take(30).collect();
    out = out.trim_end_matches('_').to_string();
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out = if out.is_empty() {
            "field".into()
        } else {
            format!("field_{out}")
        };
    }
    out
}

/// Pick the key of a staff field: the wanted one (must be valid and free),
/// else the identity field or a slug of the name, made unique per product.
pub(crate) async fn resolve_key(
    pool: &PgPool,
    product: Uuid,
    id: Option<Uuid>,
    wanted: Option<&str>,
    identity: Option<&str>,
    name: &str,
) -> AppResult<String> {
    let taken: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(module_key, field_key) FROM product_custom_fields
          WHERE product_id = $1 AND ($2::uuid IS NULL OR id <> $2) AND COALESCE(module_key, field_key) IS NOT NULL",
    )
    .bind(product)
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if let Some(w) = wanted.map(str::trim).filter(|w| !w.is_empty()) {
        if !key_ok(w) || RESERVED_KEYS.contains(&w) {
            return Err(AppError::Validation("error.invalid_field_key"));
        }
        if taken.iter().any(|t| t == w) {
            return Err(AppError::Validation("error.field_key_taken"));
        }
        return Ok(w.to_string());
    }
    let base = identity.map(str::to_string).unwrap_or_else(|| slug(name));
    let base = if RESERVED_KEYS.contains(&base.as_str()) {
        format!("{base}_field")
    } else {
        base
    };
    Ok(unique(&base, &taken))
}

fn unique(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|k| !taken.contains(k))
        .unwrap_or_default()
}

// presets

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PresetField {
    pub key: &'static str,
    /// Default (English) name; the admin UI sends translated names.
    pub name: &'static str,
    pub kind: &'static str,
    pub identity_key: Option<&'static str>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// Games the preset is meant for (empty: any product).
    pub games: &'static [&'static str],
    pub fields: &'static [PresetField],
}

const fn idf(key: &'static str, name: &'static str, required: bool) -> PresetField {
    PresetField {
        key,
        name,
        kind: "text",
        identity_key: Some(key),
        required,
    }
}

const STEAM_GAMES: &[&str] = &["rust", "gmod", "ark", "cs2", "unturned"];

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "fivem",
        name: "FiveM / RedM player",
        games: &["fivem", "redm"],
        fields: &[
            idf("license", "Rockstar license", true),
            idf("fivem", "Cfx.re ID", false),
            idf("steamid", "SteamID64", false),
            idf("discord_id", "Discord ID", false),
            idf("username", "Player name", false),
        ],
    },
    Preset {
        id: "minecraft_java",
        name: "Minecraft: Java player",
        games: &["minecraft_java"],
        fields: &[
            idf("username", "Minecraft username", true),
            idf("uuid", "Minecraft UUID", false),
            idf("discord_id", "Discord ID", false),
        ],
    },
    Preset {
        id: "minecraft_bedrock",
        name: "Minecraft: Bedrock player",
        games: &["minecraft_bedrock"],
        fields: &[
            idf("username", "Xbox gamertag", true),
            idf("xuid", "XUID", false),
            idf("discord_id", "Discord ID", false),
        ],
    },
    Preset {
        id: "steam",
        name: "Steam game player (Rust, GMod, ARK, CS2, Unturned)",
        games: STEAM_GAMES,
        fields: &[
            idf("username", "Player name", false),
            idf("steamid", "SteamID64", true),
            idf("discord_id", "Discord ID", false),
        ],
    },
    Preset {
        id: "account",
        name: "Account login (username, password, e-mail)",
        games: &[],
        fields: &[
            PresetField {
                key: "account_username",
                name: "Account username",
                kind: "text",
                identity_key: None,
                required: true,
            },
            PresetField {
                key: "account_password",
                name: "Account password",
                kind: "password",
                identity_key: None,
                required: true,
            },
            PresetField {
                key: "account_email",
                name: "Account e-mail",
                kind: "email",
                identity_key: None,
                required: false,
            },
        ],
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Which preset fields are missing from `existing` (same key, or the same
/// identity field already on the product).
pub fn missing<'a>(p: &'a Preset, existing: &[CustomField]) -> Vec<&'a PresetField> {
    p.fields
        .iter()
        .filter(|f| {
            !existing.iter().any(|e| {
                e.key() == Some(f.key)
                    || (f.identity_key.is_some() && e.identity_key.as_deref() == f.identity_key)
            })
        })
        .collect()
}

/// Add the preset's missing fields to a product (after its current fields).
/// `names` overrides field names by key (translated in the admin UI).
pub async fn apply_preset(
    pool: &PgPool,
    product: Uuid,
    preset_id: &str,
    names: &HashMap<String, String>,
) -> AppResult<Vec<CustomField>> {
    let p = preset(preset_id).ok_or(AppError::Validation("error.invalid_field_preset"))?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    // One preset run at a time per product (no double insert on a double click).
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM products WHERE id = $1 FOR UPDATE")
            .bind(product)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    exists.ok_or(AppError::NotFound)?;
    let existing: Vec<CustomField> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {FIELD_COLS} FROM product_custom_fields WHERE product_id = $1"
    )))
    .bind(product)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut order = existing.iter().map(|f| f.sort_order).max().unwrap_or(0);
    let mut added = vec![];
    for f in missing(p, &existing) {
        order += 1;
        let name = names
            .get(f.key)
            .map(|n| n.trim())
            .filter(|n| !n.is_empty() && n.chars().count() <= 120)
            .unwrap_or(f.name);
        let row: CustomField = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO product_custom_fields (id, product_id, name, description, kind, choices, pattern, required, admin_only, sort_order, identity_key, field_key)
             VALUES ($1,$2,$3,'',$4,'[]'::jsonb,'',$5,FALSE,$6,$7,$8) RETURNING {FIELD_COLS}"
        )))
        .bind(Uuid::new_v4())
        .bind(product)
        .bind(name)
        .bind(f.kind)
        .bind(f.required)
        .bind(order)
        .bind(f.identity_key)
        .bind(f.key)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        added.push(row);
    }
    tx.commit().await.map_err(db_err)?;
    Ok(added)
}

/// Set the order of a product's fields (`ids` first, in that order).
pub async fn reorder(pool: &PgPool, product: Uuid, ids: &[Uuid]) -> AppResult<()> {
    if ids.len() > 200 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE product_custom_fields SET sort_order = $3 WHERE id = $1 AND product_id = $2 AND module_key IS NULL")
            .bind(id)
            .bind(product)
            .bind(i as i32 + 1)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

// values

/// (id, key, identity key, kind) of every field of a product.
async fn defs(
    pool: &PgPool,
    product: Uuid,
) -> AppResult<Vec<(Uuid, Option<String>, Option<String>, String)>> {
    sqlx::query_as(
        "SELECT id, COALESCE(module_key, field_key), identity_key, kind FROM product_custom_fields WHERE product_id = $1 ORDER BY sort_order, id",
    )
    .bind(product)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// The custom fields a provisioning module sees: the stored map (field id →
/// value, plus any keys addons set) and every field again under its key,
/// passwords opened. A key nobody else uses also answers for an identity
/// field (`license`, `steamid`, ...).
pub async fn module_view(pool: &PgPool, product: Uuid, stored: &Value) -> AppResult<Value> {
    let defs = defs(pool, product).await?;
    let mut out: Map<String, Value> = stored.as_object().cloned().unwrap_or_default();
    let mut keyed = Map::new();
    for (id, key, identity, kind) in &defs {
        let Some(v) = stored.get(id.to_string()).and_then(Value::as_str) else {
            continue;
        };
        let v = if kind == "password" {
            open(v)
        } else {
            v.to_string()
        };
        out.insert(id.to_string(), Value::String(v.clone()));
        if let Some(k) = key {
            keyed.insert(k.clone(), Value::String(v.clone()));
        }
        if let Some(i) = identity
            .as_ref()
            .filter(|i| IDENTITY_KEYS.contains(&i.as_str()))
        {
            keyed.entry(i.clone()).or_insert(Value::String(v));
        }
    }
    out.extend(keyed);
    Ok(Value::Object(out))
}

/// Field values for delivery commands, by key. Passwords are not included
/// (see [`secret_values`]); their keys come back in the second list.
pub async fn command_values(
    pool: &PgPool,
    service: Uuid,
) -> AppResult<(BTreeMap<String, String>, Vec<String>)> {
    let Some((product, stored)) = stored_values(pool, service).await? else {
        return Ok(Default::default());
    };
    let mut vals = BTreeMap::new();
    let mut secret = vec![];
    for (id, key, _, kind) in defs(pool, product).await? {
        let Some(k) = key else { continue };
        if kind == "password" {
            secret.push(k);
            continue;
        }
        if let Some(v) = stored.get(id.to_string()).and_then(Value::as_str) {
            vals.insert(k, v.to_string());
        }
    }
    Ok((vals, secret))
}

/// Opened password values of a service, by key (delivery send time only).
pub async fn secret_values(pool: &PgPool, service: Uuid) -> AppResult<BTreeMap<String, String>> {
    let Some((product, stored)) = stored_values(pool, service).await? else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for (id, key, _, kind) in defs(pool, product).await? {
        if let (Some(k), "password") = (key, kind.as_str()) {
            if let Some(v) = stored.get(id.to_string()).and_then(Value::as_str) {
                out.insert(k, open(v));
            }
        }
    }
    Ok(out)
}

/// (product, order line values merged with the service's own).
async fn stored_values(pool: &PgPool, service: Uuid) -> AppResult<Option<(Uuid, Value)>> {
    sqlx::query_as(
        "SELECT s.product_id, COALESCE(oi.custom_fields, '{}'::jsonb) || COALESCE(s.custom_fields, '{}'::jsonb)
           FROM services s LEFT JOIN order_items oi ON oi.id = s.order_item_id WHERE s.id = $1",
    )
    .bind(service)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: Option<&str>, identity: Option<&str>) -> CustomField {
        CustomField {
            id: Uuid::new_v4(),
            product_id: Uuid::nil(),
            name: "x".into(),
            description: String::new(),
            kind: "text".into(),
            choices: serde_json::json!([]),
            pattern: String::new(),
            required: false,
            admin_only: false,
            sort_order: 0,
            identity_key: identity.map(str::to_string),
            module_key: None,
            field_key: key.map(str::to_string),
        }
    }

    #[test]
    fn presets_skip_fields_already_there() {
        let p = preset("fivem").unwrap();
        assert_eq!(missing(p, &[]).len(), 5);
        // Same identity under another key, and the same key: both skipped.
        let have = [
            field(Some("rockstar"), Some("license")),
            field(Some("discord_id"), None),
        ];
        let keys: Vec<_> = missing(p, &have).iter().map(|f| f.key).collect();
        assert_eq!(keys, ["fivem", "steamid", "username"]);
        let acc = preset("account").unwrap();
        assert!(acc
            .fields
            .iter()
            .any(|f| f.kind == "password" && f.identity_key.is_none()));
        assert!(acc.fields.iter().any(|f| f.kind == "email"));
        for p in PRESETS {
            for f in p.fields {
                assert!(
                    key_ok(f.key) && !RESERVED_KEYS.contains(&f.key),
                    "{}",
                    f.key
                );
                assert!(crate::addons::FIELD_KINDS.contains(&f.kind));
                assert!(f.identity_key.is_none_or(|i| IDENTITY_KEYS.contains(&i)));
            }
            for g in p.games {
                assert!(crate::games::game(g).is_some(), "{g}");
            }
        }
    }

    #[test]
    fn keys_and_slugs() {
        assert_eq!(slug("Player license"), "player_license");
        assert_eq!(slug("  E-mail (in game)! "), "e_mail_in_game");
        assert_eq!(slug("123 abc"), "field_123_abc");
        assert_eq!(slug("ăîș"), "field");
        assert!(key_ok("steamid") && key_ok("a1_b"));
        assert!(!key_ok("1a") && !key_ok("Bad") && !key_ok("") && !key_ok(&"a".repeat(41)));
        let taken = vec!["x".to_string(), "x_2".to_string()];
        assert_eq!(unique("x", &taken), "x_3");
        assert_eq!(unique("y", &taken), "y");
    }

    #[test]
    fn emails() {
        for ok in ["a@b.co", "first.last+tag@mail.example.com"] {
            assert!(email_ok(ok), "{ok}");
        }
        for bad in [
            "", "a", "a@b", "@b.co", "a@@b.co", "a b@c.de", "a@.co", "a@b..co", "a@b.co.",
        ] {
            assert!(!email_ok(bad), "{bad}");
        }
    }

    #[test]
    fn unsealed_values_pass_through() {
        assert_eq!(open("plain"), "plain");
        assert_eq!(open("enc:v1:zz"), "");
    }
}
