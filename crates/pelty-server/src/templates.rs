//! Ready-made store templates (`pelty-server template list | apply <id>`).
//!
//! A template is a list of admin API calls (see `templates/*.json`) replayed
//! in-process through the normal router with a short-lived session of the
//! first administrator, so every value passes the same validation as the
//! admin panel. Nothing secret is set: game server addresses, RCON passwords
//! and payment gateways stay for the owner to fill in.
use pelty_api::AppState;
use pelty_core::config::Config;
use pelty_core::i18n::I18n;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// Settings key that records the applied template.
pub const MARKER_KEY: &str = "site.template";

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    source: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "minecraft",
        name: "Minecraft server store",
        source: include_str!("../templates/minecraft.json"),
    },
    Template {
        id: "fivem",
        name: "FiveM roleplay store",
        source: include_str!("../templates/fivem.json"),
    },
    Template {
        id: "rust",
        name: "Rust server store",
        source: include_str!("../templates/rust.json"),
    },
];

pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

pub struct Options {
    pub id: String,
    pub force: bool,
    pub site_name: Option<String>,
}

pub fn parse_args(args: &[&str]) -> Result<Options, BoxErr> {
    let usage = "usage: pelty-server template list | apply <minecraft|fivem|rust|blank> [--name <site name>] [--force]";
    let [id, rest @ ..] = args else {
        return Err(usage.into());
    };
    let mut o = Options {
        id: id.to_ascii_lowercase(),
        force: false,
        site_name: None,
    };
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--force" => o.force = true,
            "--name" => o.site_name = Some(it.next().ok_or(usage)?.to_string()),
            _ => return Err(usage.into()),
        }
    }
    Ok(o)
}

/// `${name}` placeholders in every string of `v`.
pub fn substitute(v: &Value, vars: &BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => {
            let mut out = s.clone();
            for (k, val) in vars {
                out = out.replace(&format!("${{{k}}}"), val);
            }
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, vars)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), substitute(x, vars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Objects merge key by key; anything else replaces.
pub fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Expand a `package` step into the calls that create it.
pub fn expand_package(p: &Map<String, Value>) -> Result<Vec<Value>, String> {
    let slug = p
        .get("slug")
        .and_then(Value::as_str)
        .ok_or("package without slug")?;
    let var = p
        .get("save")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("p_{}", slug.replace('-', "_")));
    let mut product = Map::new();
    for k in [
        "group_id",
        "name",
        "slug",
        "description",
        "image_url",
        "sort_order",
    ] {
        if let Some(v) = p.get(k) {
            product.insert(k.into(), v.clone());
        }
    }
    product.insert("status".into(), json!("active"));
    let mut steps =
        vec![json!({"post": "/api/v1/admin/catalog/products", "body": product, "save": var})];
    if let Some(prices) = p.get("prices") {
        steps.push(json!({"put": format!("/api/v1/admin/catalog/products/${{{var}}}/prices"), "body": {"prices": prices}}));
    }
    if let Some(preset) = p.get("preset") {
        steps.push(json!({"post": format!("/api/v1/admin/catalog/products/${{{var}}}/fields/preset"), "body": {"preset": preset}}));
    }
    if let Some(cmds) = p.get("commands") {
        steps.push(json!({"put": format!("/api/v1/admin/game/delivery/products/${{{var}}}/commands"), "body": {"commands": cmds}}));
    }
    Ok(steps)
}

struct Runner {
    app: axum::Router,
    cookie: String,
    csrf: String,
    vars: BTreeMap<String, String>,
}

impl Runner {
    async fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        use tower::ServiceExt;
        let mut b = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", &self.cookie)
            .header("x-csrf-token", &self.csrf)
            .header("user-agent", "pelty-server template");
        let req = match body {
            Some(v) => {
                b = b.header("content-type", "application/json");
                b.body(axum::body::Body::from(v.to_string()))
            }
            None => b.body(axum::body::Body::empty()),
        }
        .map_err(|e| e.to_string())?;
        let res = match self.app.clone().oneshot(req).await {
            Ok(r) => r,
            Err(never) => match never {},
        };
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 8 << 20)
            .await
            .map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(format!("{method} {path}: {status} {v}"));
        }
        Ok(v)
    }

    async fn step(&mut self, raw: &Value) -> Result<(), String> {
        let Some(obj) = raw.as_object() else {
            return Err("step must be an object".into());
        };
        if let Some(p) = obj.get("package").and_then(Value::as_object) {
            for s in expand_package(p)? {
                Box::pin(self.step(&s)).await?;
            }
            return Ok(());
        }
        let s = substitute(raw, &self.vars);
        let optional = s.get("optional").and_then(Value::as_bool) == Some(true);
        let res = self.run_one(&s).await;
        match res {
            Err(e) if optional => {
                println!("[template] skipped: {e}");
                Ok(())
            }
            other => other,
        }
    }

    async fn run_one(&mut self, s: &Value) -> Result<(), String> {
        let body = s.get("body");
        if let Some(path) = s.get("merge").and_then(Value::as_str) {
            let mut current = self.call("GET", path, None).await?;
            let pointer = s.get("pointer").and_then(Value::as_str).unwrap_or("");
            let mut target = if pointer.is_empty() {
                current.clone()
            } else {
                current
                    .pointer(pointer)
                    .cloned()
                    .unwrap_or_else(|| json!({}))
            };
            if let Some(b) = body {
                merge(&mut target, b);
            }
            if !pointer.is_empty() {
                if let Some(slot) = current.pointer_mut(pointer) {
                    *slot = target.clone();
                }
            }
            let method = s.get("method").and_then(Value::as_str).unwrap_or("PUT");
            self.call(method, path, Some(&target)).await?;
            return Ok(());
        }
        let (method, path) = ["post", "put", "patch"]
            .iter()
            .find_map(|m| {
                s.get(*m)
                    .and_then(Value::as_str)
                    .map(|p| (m.to_ascii_uppercase(), p.to_string()))
            })
            .ok_or("step needs post, put, patch, merge or package")?;
        let out = self.call(&method, &path, body).await?;
        if let Some(var) = s.get("save").and_then(Value::as_str) {
            let pointer = s
                .get("save_pointer")
                .and_then(Value::as_str)
                .unwrap_or("/id");
            let id = out
                .pointer(pointer)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{path}: no {pointer} in the answer"))?;
            self.vars.insert(var.to_string(), id.to_string());
        }
        Ok(())
    }
}

pub async fn cmd(
    cfg: Config,
    pool: Option<pelty_database::Pool>,
    args: &[&str],
) -> Result<(), BoxErr> {
    if args.first() == Some(&"list") {
        println!("blank\tBlank site (nothing added)");
        for t in TEMPLATES {
            println!("{}\t{}", t.id, t.name);
        }
        return Ok(());
    }
    let [verb, rest @ ..] = args else {
        return Err(parse_args(&[]).err().unwrap_or_else(|| "usage".into()));
    };
    if *verb != "apply" {
        return Err(parse_args(&[]).err().unwrap_or_else(|| "usage".into()));
    }
    let o = parse_args(rest)?;
    let pool = pool.ok_or("database not reachable")?;
    if o.id == "blank" || o.id == "none" || o.id.is_empty() {
        println!("[template] blank site, nothing to apply");
        return Ok(());
    }
    let t = find(&o.id).ok_or_else(|| {
        format!(
            "unknown template {:?} (minecraft, fivem, rust or blank)",
            o.id
        )
    })?;
    if let Some(done) = pelty_database::settings::get(&pool, MARKER_KEY).await? {
        if !o.force {
            println!(
                "[template] already applied ({}); use --force to apply again",
                done["id"].as_str().unwrap_or("?")
            );
            return Ok(());
        }
    }
    let doc: Value = serde_json::from_str(t.source)?;
    let admin: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM users u JOIN user_roles ur ON ur.user_id = u.id JOIN roles r ON r.id = ur.role_id
         WHERE r.name = 'admin' ORDER BY u.created_at LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?;
    let admin = admin.ok_or("create an administrator first (pelty-server admin create)")?;
    let user = pelty_database::users::find(&pool, admin)
        .await?
        .ok_or("administrator not found")?;

    let i18n = I18n::load(&cfg.locales_dir, &cfg.default_locale)?;
    let state = AppState::new(pool.clone(), cfg, i18n);
    state.init().await.map_err(|e| e.to_string())?;
    let issued = state
        .auth
        .issue_session(user, None, Some("pelty-server template"))
        .await?;
    let session_id = issued.session.id;

    let mut vars = BTreeMap::new();
    let currency = pelty_database::gaming::storefront::default_currency(&pool)
        .await
        .unwrap_or_else(|_| "EUR".into());
    vars.insert("currency".into(), currency);
    let default_name = doc["site_name"]
        .as_str()
        .unwrap_or("Game Store")
        .to_string();
    vars.insert(
        "site_name".into(),
        o.site_name.clone().unwrap_or(default_name),
    );

    let mut runner = Runner {
        app: pelty_api::router(state.clone()),
        cookie: format!("pelty_session={}", issued.token),
        csrf: issued.csrf_token,
        vars,
    };
    let steps = doc["steps"].as_array().cloned().unwrap_or_default();
    let mut result = Ok(());
    for (i, s) in steps.iter().enumerate() {
        if let Err(e) = runner.step(s).await {
            result = Err(format!("step {}: {e}", i + 1));
            break;
        }
    }
    // The session only existed for this run.
    let _ = sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(&pool)
        .await;
    result?;
    pelty_database::settings::set(
        &pool,
        MARKER_KEY,
        json!({ "id": t.id, "applied_at": time::OffsetDateTime::now_utc().unix_timestamp() }),
    )
    .await?;
    println!("[template] {} applied ({} steps)", t.name, steps.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse_and_expand() {
        for t in TEMPLATES {
            let doc: Value =
                serde_json::from_str(t.source).unwrap_or_else(|e| panic!("{}: {e}", t.id));
            let steps = doc["steps"].as_array().expect("steps");
            assert!(steps.len() > 5, "{}", t.id);
            for s in steps {
                if let Some(p) = s.get("package").and_then(Value::as_object) {
                    let ex = expand_package(p).expect("package");
                    assert!(ex.len() >= 2, "{}: {p:?}", t.id);
                } else {
                    assert!(
                        ["post", "put", "patch", "merge"]
                            .iter()
                            .any(|k| s.get(*k).is_some()),
                        "{}: {s}",
                        t.id
                    );
                }
            }
        }
    }

    #[test]
    fn substitute_and_merge() {
        let mut vars = BTreeMap::new();
        vars.insert("a".to_string(), "1".to_string());
        let v = substitute(&json!({"x": ["${a}", "{username} ${a}"], "n": 2}), &vars);
        assert_eq!(v, json!({"x": ["1", "{username} 1"], "n": 2}));
        let mut base = json!({"a": {"b": 1, "c": 2}, "d": [1]});
        merge(&mut base, &json!({"a": {"c": 3}, "d": [2, 3]}));
        assert_eq!(base, json!({"a": {"b": 1, "c": 3}, "d": [2, 3]}));
    }

    #[test]
    fn args() {
        let o = parse_args(&["FiveM", "--name", "My RP", "--force"]).unwrap();
        assert_eq!(
            (o.id.as_str(), o.force, o.site_name.as_deref()),
            ("fivem", true, Some("My RP"))
        );
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["rust", "--bogus"]).is_err());
    }
}
