//! Host side of the addon `power` interface: core reads with fixed column
//! lists, typed audited writes, SQL limited to the addon's own tables, and
//! background jobs. The runtime has already checked the capability; every
//! function here re-checks its own scope (defence in depth).
use pelty_payments::BoxFuture;
use serde_json::{json, Value};
use sqlparser::ast::{visit_expressions, visit_relations, Expr, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlx::PgPool;
use std::ops::ControlFlow;

pub const JOB_KIND: &str = "addon.job";

/// Readable entities: (name, table, columns never returned).
/// Password hashes live in `users`, which is not exposed at all.
pub const ENTITIES: &[(&str, &str, &[&str])] = &[
    ("clients", "customers", &[]),
    ("services", "services", &["module_data"]),
    ("invoices", "invoices", &[]),
    ("invoice_items", "invoice_items", &[]),
    ("orders", "orders", &[]),
    ("tickets", "tickets", &[]),
    ("domains", "domains", &["auth_code_sealed", "contact"]),
    ("products", "products", &["module_config"]),
];

/// Typed write ops (`capabilities.db.write`).
pub const WRITE_OPS: &[&str] = &[
    "client.note",
    "invoice.admin_note",
    "service.custom_field",
    "ticket.status",
];

const TICKET_STATUSES: &[&str] = &[
    "open",
    "answered",
    "customer_reply",
    "on_hold",
    "in_progress",
    "closed",
];

/// SQL functions an addon statement may call.
const SQL_FUNCS: &[&str] = &[
    "count",
    "sum",
    "min",
    "max",
    "avg",
    "coalesce",
    "nullif",
    "lower",
    "upper",
    "length",
    "trim",
    "substr",
    "substring",
    "abs",
    "round",
    "floor",
    "ceil",
    "now",
    "greatest",
    "least",
    "concat",
    "jsonb_build_object",
    "to_jsonb",
    "jsonb_agg",
    "string_agg",
    "date_trunc",
    "extract",
    "gen_random_uuid",
];

pub struct DbPower(pub PgPool);

/// `addon_<id with non-alphanumerics as _>_`.
pub fn table_prefix(addon: &str) -> String {
    let id: String = addon
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("addon_{id}_")
}

/// Accept one statement that only touches `addon_<id>_*` tables and calls
/// only allowlisted functions. Returns true for row-returning statements.
pub fn check_sql(addon: &str, sql: &str) -> Result<bool, String> {
    let stmts = sqlparser::parser::Parser::parse_sql(&PostgreSqlDialect {}, sql)
        .map_err(|e| format!("sql: {e}"))?;
    let [st] = stmts.as_slice() else {
        return Err("exactly one statement".into());
    };
    let rows = match st {
        Statement::Query(q) => {
            if q.with.is_some() {
                return Err("WITH is not allowed".into());
            }
            true
        }
        Statement::Insert(_)
        | Statement::Update { .. }
        | Statement::Delete(_)
        | Statement::CreateTable(_)
        | Statement::CreateIndex(_)
        | Statement::AlterTable { .. }
        | Statement::Drop { .. } => false,
        _ => return Err("statement type not allowed".into()),
    };
    if let Statement::Drop { object_type, .. } = st {
        if !matches!(
            object_type,
            sqlparser::ast::ObjectType::Table | sqlparser::ast::ObjectType::Index
        ) {
            return Err("only DROP TABLE / INDEX".into());
        }
    }
    let prefix = table_prefix(addon);
    // Foreign keys into core tables would let an addon block core deletes.
    if st.to_string().to_ascii_uppercase().contains("REFERENCES") {
        return Err("REFERENCES is not allowed".into());
    }
    if let Statement::AlterTable(at) = st {
        use sqlparser::ast::{AlterTableOperation as Op, RenameTableNameKind};
        for op in &at.operations {
            match op {
                Op::AddColumn { .. }
                | Op::DropColumn { .. }
                | Op::AlterColumn { .. }
                | Op::RenameColumn { .. } => {}
                Op::RenameTable {
                    table_name: RenameTableNameKind::As(n) | RenameTableNameKind::To(n),
                } => {
                    let last =
                        n.0.last()
                            .map(|p| p.to_string().trim_matches('"').to_ascii_lowercase())
                            .unwrap_or_default();
                    if n.0.len() != 1 || !last.starts_with(&prefix) {
                        return Err(format!("table not owned by the addon: {n}"));
                    }
                }
                _ => return Err("ALTER TABLE operation not allowed".into()),
            }
        }
    }
    let mut bad: Option<String> = None;
    let _ = visit_relations(st, |name| {
        let parts: Vec<String> = name
            .0
            .iter()
            .map(|p| p.to_string().trim_matches('"').to_ascii_lowercase())
            .collect();
        let ok = match parts.as_slice() {
            [t] => t.starts_with(&prefix),
            [s, t] => s == "public" && t.starts_with(&prefix),
            _ => false,
        };
        if !ok {
            bad = Some(format!("table not owned by the addon: {name}"));
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    if bad.is_none() {
        if let Statement::Drop { names, .. } = st {
            if let Some(n) = names.iter().find(|n| {
                !n.to_string()
                    .trim_matches('"')
                    .to_ascii_lowercase()
                    .starts_with(&prefix)
            }) {
                bad = Some(format!("table not owned by the addon: {n}"));
            }
        }
        if let Statement::CreateIndex(ci) = st {
            if let Some(n) = &ci.name {
                if !n
                    .to_string()
                    .trim_matches('"')
                    .to_ascii_lowercase()
                    .starts_with(&prefix)
                {
                    bad = Some(format!("index name must start with {prefix}"));
                }
            }
        }
    }
    if bad.is_none() {
        let _ = visit_expressions(st, |e| {
            match e {
                Expr::Function(f) => {
                    let n = f.name.to_string().to_ascii_lowercase();
                    if !SQL_FUNCS.contains(&n.as_str()) {
                        bad = Some(format!("function not allowed: {n}"));
                        return ControlFlow::Break(());
                    }
                }
                Expr::Subquery(_) | Expr::Exists { .. } | Expr::InSubquery { .. } => {}
                _ => {}
            }
            ControlFlow::Continue(())
        });
    }
    match bad {
        Some(b) => Err(b),
        None => Ok(rows),
    }
}

fn bind_all<'q>(
    mut q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    params: &'q [Value],
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    for p in params {
        q = match p {
            Value::Null => q.bind(None::<String>),
            Value::Bool(b) => q.bind(*b),
            Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
            Value::Number(n) => match n.to_string().parse::<rust_decimal::Decimal>() {
                Ok(d) => q.bind(d),
                Err(_) => q.bind(n.as_f64()),
            },
            Value::String(s) => q.bind(s.as_str()),
            v => q.bind(sqlx::types::Json(v)),
        };
    }
    q
}

async fn run_sql(pool: &PgPool, addon: &str, sql: &str, params: &[Value]) -> Result<Value, String> {
    let rows = check_sql(addon, sql)?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let out = if rows {
        let wrapped =
            format!("SELECT COALESCE(jsonb_agg(to_jsonb(q)), '[]'::jsonb) FROM ({sql}) q");
        let mut q = sqlx::query_scalar::<_, Value>(&wrapped);
        for p in params {
            q = match p {
                Value::Null => q.bind(None::<String>),
                Value::Bool(b) => q.bind(*b),
                Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
                Value::Number(n) => q.bind(n.to_string().parse::<rust_decimal::Decimal>().ok()),
                Value::String(s) => q.bind(s.clone()),
                v => q.bind(sqlx::types::Json(v.clone())),
            };
        }
        q.fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("sql: {e}"))?
    } else {
        let r = bind_all(sqlx::query(sql), params)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("sql: {e}"))?;
        json!({ "rows_affected": r.rows_affected() })
    };
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(out)
}

async fn read(pool: &PgPool, entity: &str, q: &Value) -> Result<Value, String> {
    let (_, table, hidden) = ENTITIES
        .iter()
        .find(|(n, ..)| *n == entity)
        .ok_or_else(|| format!("unknown entity {entity}"))?;
    let cols: Vec<String> = sqlx::query_scalar("SELECT column_name::text FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = $1")
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut filters: Vec<(String, String)> = vec![];
    if let Some(id) = q.get("id").and_then(Value::as_str) {
        filters.push(("id".into(), id.to_string()));
    }
    for (k, v) in q
        .get("filter")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if !cols.contains(k) || hidden.contains(&k.as_str()) {
            return Err(format!("unknown filter column {k}"));
        }
        let v = match v {
            Value::String(s) => s.clone(),
            Value::Number(_) | Value::Bool(_) => v.to_string(),
            _ => return Err("filter values must be scalars".into()),
        };
        filters.push((k.clone(), v));
    }
    let limit = q
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(50)
        .clamp(1, 500);
    let offset = q.get("offset").and_then(Value::as_i64).unwrap_or(0).max(0);
    let minus: String = hidden.iter().map(|h| format!(" - '{h}'")).collect();
    let mut sql = format!("SELECT COALESCE(jsonb_agg(r), '[]'::jsonb) FROM (SELECT to_jsonb(t){minus} AS r FROM {table} t");
    for (i, (k, _)) in filters.iter().enumerate() {
        sql.push_str(if i == 0 { " WHERE " } else { " AND " });
        // `k` is a real column name of `table` (checked above).
        sql.push_str(&format!("t.\"{k}\"::text = ${}", i + 1));
    }
    sql.push_str(&format!(
        " ORDER BY t.created_at DESC NULLS LAST LIMIT {limit} OFFSET {offset}) s"
    ));
    let mut qq = sqlx::query_scalar::<_, Value>(&sql);
    for (_, v) in &filters {
        qq = qq.bind(v.clone());
    }
    qq.fetch_one(pool).await.map_err(|e| format!("db: {e}"))
}

fn uuid_of(v: &Value, k: &str) -> Result<uuid::Uuid, String> {
    v.get(k)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("{k} must be a UUID"))
}

fn text_of(v: &Value, k: &str, max: usize) -> Result<String, String> {
    let t = v
        .get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{k} required"))?;
    if t.len() > max {
        return Err(format!("{k} too long"));
    }
    Ok(t.to_string())
}

async fn write(pool: &PgPool, addon: &str, op: &str, i: &Value) -> Result<Value, String> {
    let (target, res) = match op {
        "client.note" => {
            let id = uuid_of(i, "customer_id")?;
            let t = text_of(i, "text", 4000)?;
            let r = sqlx::query("UPDATE customers SET notes = CASE WHEN COALESCE(notes, '') = '' THEN $2 ELSE notes || E'\\n' || $2 END, updated_at = now() WHERE id = $1").bind(id).bind(&t).execute(pool).await;
            (id, r)
        }
        "invoice.admin_note" => {
            let id = uuid_of(i, "invoice_id")?;
            let t = text_of(i, "text", 4000)?;
            (
                id,
                sqlx::query("UPDATE invoices SET admin_notes = $2 WHERE id = $1")
                    .bind(id)
                    .bind(&t)
                    .execute(pool)
                    .await,
            )
        }
        "service.custom_field" => {
            let id = uuid_of(i, "service_id")?;
            let k = text_of(i, "key", 64)?;
            let v = i.get("value").cloned().unwrap_or(Value::Null);
            if v.to_string().len() > 8192 {
                return Err("value too large".into());
            }
            (id, sqlx::query("UPDATE services SET custom_fields = jsonb_set(COALESCE(custom_fields, '{}'::jsonb), ARRAY[$2], $3, true), updated_at = now() WHERE id = $1").bind(id).bind(&k).bind(sqlx::types::Json(&v)).execute(pool).await)
        }
        "ticket.status" => {
            let id = uuid_of(i, "ticket_id")?;
            let st = text_of(i, "status", 32)?;
            if !TICKET_STATUSES.contains(&st.as_str()) {
                return Err("invalid ticket status".into());
            }
            (id, sqlx::query("UPDATE tickets SET status = $2, closed_at = CASE WHEN $2 = 'closed' THEN now() ELSE NULL END WHERE id = $1").bind(id).bind(&st).execute(pool).await)
        }
        _ => return Err(format!("unknown write op {op}")),
    };
    let n = res.map_err(|e| format!("db: {e}"))?.rows_affected();
    if n == 0 {
        return Err("not found".into());
    }
    let _ = pelty_database::audit::record(
        pool,
        None,
        "addon.db_write",
        Some(&target.to_string()),
        None,
        json!({ "addon": addon, "op": op }),
    )
    .await;
    Ok(json!({ "ok": true }))
}

impl pelty_wasm::PowerHost for DbPower {
    fn db_read<'a>(
        &'a self,
        _addon: &'a str,
        entity: &'a str,
        query: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move { read(&self.0, entity, &query).await })
    }
    fn db_write<'a>(
        &'a self,
        addon: &'a str,
        op: &'a str,
        input: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move { write(&self.0, addon, op, &input).await })
    }
    fn sql<'a>(
        &'a self,
        addon: &'a str,
        statement: &'a str,
        params: Vec<Value>,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move { run_sql(&self.0, addon, statement, &params).await })
    }
    fn enqueue<'a>(
        &'a self,
        addon: &'a str,
        op: &'a str,
        input: Value,
        delay: u32,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let at = time::OffsetDateTime::now_utc() + time::Duration::seconds(delay as i64);
            pelty_database::jobs::enqueue(
                &self.0,
                JOB_KIND,
                json!({ "addon": addon, "op": op, "input": input }),
                Some(at),
                None,
                3,
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }
    fn cron_register<'a>(
        &'a self,
        addon: &'a str,
        name: &'a str,
        job: Option<(&'a str, &'a str)>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { crate::addon_ext::cron_register(&self.0, addon, name, job).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_scope() {
        let a = "integration-powerdemo";
        assert_eq!(table_prefix(a), "addon_integration_powerdemo_");
        for ok in [
            "CREATE TABLE addon_integration_powerdemo_items (id bigserial primary key, name text)",
            "INSERT INTO addon_integration_powerdemo_items (name) VALUES ($1)",
            "SELECT id, upper(name) AS n FROM addon_integration_powerdemo_items WHERE id > $1",
            "UPDATE addon_integration_powerdemo_items SET name = $1",
            "DELETE FROM public.addon_integration_powerdemo_items",
            "DROP TABLE addon_integration_powerdemo_items",
            "ALTER TABLE addon_integration_powerdemo_items ADD COLUMN note text",
            "ALTER TABLE addon_integration_powerdemo_items RENAME TO addon_integration_powerdemo_things",
            "SELECT count(*) FROM addon_integration_powerdemo_a a JOIN addon_integration_powerdemo_b b ON a.id = b.id",
        ] {
            assert!(check_sql(a, ok).is_ok(), "{ok}: {:?}", check_sql(a, ok));
        }
        for bad in [
            "SELECT * FROM users",
            "SELECT password_hash FROM addon_integration_powerdemo_x, users",
            "SELECT * FROM addon_integration_powerdemo_x WHERE id IN (SELECT id FROM customers)",
            "SELECT * FROM addon_other_items",
            "SELECT * FROM pg_catalog.pg_user",
            "SELECT pg_read_file('/etc/passwd')",
            "SELECT * FROM addon_integration_powerdemo_x; DROP TABLE customers",
            "WITH x AS (SELECT * FROM users) SELECT * FROM x",
            "COPY addon_integration_powerdemo_x TO '/tmp/x'",
            "SET ROLE postgres",
            "DROP TABLE customers",
            "SELECT * FROM generate_series(1, 10)",
            "SELECT current_setting('server_version')",
            "INSERT INTO addon_integration_powerdemo_x SELECT * FROM invoices",
            "UPDATE addon_integration_powerdemo_x SET a = (SELECT email FROM customers LIMIT 1)",
            // bypass attempts
            "SELECT 1; SELECT * FROM users",
            "SELECT * FROM addon_integration_powerdemo_x /* x */ ; DELETE FROM customers",
            "SELECT * FROM addon_integration_powerdemo_x -- \n UNION SELECT * FROM users",
            "SELECT a FROM addon_integration_powerdemo_x UNION SELECT email FROM customers",
            "SELECT \"pg_read_file\"('/etc/passwd')",
            "SELECT pg_catalog.pg_read_file('/etc/passwd')",
            "SELECT pg_catalog.count(*) FROM addon_integration_powerdemo_x",
            "SELECT * FROM other_schema.addon_integration_powerdemo_x",
            "SELECT * FROM \"public\".\"users\"",
            "SELECT * FROM db.public.addon_integration_powerdemo_x",
            "SELECT pg_sleep(100)",
            "SELECT lo_import('/etc/passwd')",
            "SELECT dblink('host=x', 'select 1')",
            "SELECT set_config('role', 'postgres', false)",
            "WITH RECURSIVE r AS (SELECT 1) SELECT * FROM r",
            "SELECT * FROM (SELECT * FROM users) s",
            "SELECT * FROM addon_integration_powerdemo_x a, LATERAL (SELECT * FROM customers) c",
            "CREATE TABLE addon_integration_powerdemo_y AS SELECT * FROM users",
            "CREATE TABLE addon_integration_powerdemo_y (a text DEFAULT pg_read_file('/etc/passwd'))",
            "DROP TABLE addon_integration_powerdemo_x, customers",
            "DROP FUNCTION addon_integration_powerdemo_f",
            "ALTER TABLE addon_integration_powerdemo_x RENAME TO customers",
            "ALTER TABLE addon_integration_powerdemo_x OWNER TO postgres",
            "ALTER TABLE addon_integration_powerdemo_x ADD CONSTRAINT f FOREIGN KEY (a) REFERENCES customers (id)",
            "CREATE TABLE addon_integration_powerdemo_y (c uuid REFERENCES customers (id))",
            "ALTER TABLE customers ADD COLUMN x text",
            "CREATE INDEX evil ON addon_integration_powerdemo_x (a)",
            "TRUNCATE customers",
            "GRANT ALL ON customers TO public",
            "DO $$ BEGIN PERFORM 1; END $$",
            "CALL p()",
            "SELECT * FROM addon_integration_powerdemo_x WHERE a = current_user",
        ] {
            assert!(check_sql(a, bad).is_err(), "{bad}");
        }
    }
}
