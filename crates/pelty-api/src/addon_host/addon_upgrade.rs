//! Pre-extraction replacement policy. In-place upgrades never change schema or approvals.
use crate::addons::{self, Manifest};
use crate::AppState;
use pelty_core::{AppError, AppResult};
use pelty_database::addon_registry as reg;
use pelty_extensions::addon_pkg::sha256_hex;
use semver::Version;
use sqlparser::ast::{
    ColumnOption, DataType, Expr, Ident, ObjectName, Statement, TableConstraint, UnaryOperator,
    Value,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use sqlx::{Connection, PgConnection, PgPool};

/// Owns a dedicated control connection, never a slot in the application pool.
/// Dropping the guard closes its connection; PostgreSQL then rolls back the
/// transaction and releases the advisory lock. No background task is needed.
#[must_use = "keep the guard alive for the entire addon lifecycle"]
pub struct LifecycleGuard {
    connection: PgConnection,
}

impl LifecycleGuard {
    /// Await rollback before closing, for deterministic release when needed.
    /// Ordinary lifecycle callers may simply let the guard drop.
    pub async fn release(mut self) -> AppResult<()> {
        sqlx::query("ROLLBACK")
            .execute(&mut self.connection)
            .await
            .map_err(pelty_database::db_err)?;
        self.connection
            .close()
            .await
            .map_err(pelty_database::db_err)
    }
}

/// Serialize the entire lifecycle for one addon across host processes.
/// Fail fast with `error.addon_busy` rather than waiting on a held lock.
/// Keep the guard alive through checks, filesystem changes, registry writes and
/// runtime changes; do not acquire it again inside a nested lifecycle helper.
pub async fn lock(pool: &PgPool, id: &str) -> AppResult<LifecycleGuard> {
    // A pooled transaction would starve registry queries when pool size is one.
    // The owned connection is also dropped on errors or cancellation below.
    let options = pool.connect_options();
    let mut connection = PgConnection::connect_with(options.as_ref())
        .await
        .map_err(pelty_database::db_err)?;
    sqlx::query("BEGIN")
        .execute(&mut connection)
        .await
        .map_err(pelty_database::db_err)?;
    let acquired: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("pelty:addon:{id}"))
            .fetch_one(&mut connection)
            .await
            .map_err(pelty_database::db_err)?;
    if !acquired {
        return Err(AppError::Conflict("error.addon_busy"));
    }
    Ok(LifecycleGuard { connection })
}

/// A deliberately small DDL subset, not a general SQL sandbox. Only addon-local
/// tables, keys/references and plain-column indexes may be created. No queries,
/// CHECK expressions, casts, generated columns or arbitrary function calls.
pub fn migration_safe(sql: &str, prefix: &str) -> bool {
    if !prefix.starts_with("addon_") || !prefix.ends_with('_') || !simple_name(prefix) {
        return false;
    }
    let Ok(statements) = Parser::parse_sql(&PostgreSqlDialect {}, sql) else {
        return false;
    };
    !statements.is_empty()
        && statements.iter().all(|statement| {
            let projected = match statement {
                Statement::CreateTable(table) => {
                    if !scoped_name(&table.name, prefix) || table.columns.is_empty() {
                        return false;
                    }
                    let mut definitions = Vec::new();
                    for column in &table.columns {
                        if !safe_ident(&column.name)
                            || !safe_type(&column.data_type)
                            || !column.options.iter().all(|option| {
                                option
                                    .name
                                    .as_ref()
                                    .is_none_or(|name| scoped_ident(name, prefix))
                                    && match &option.option {
                                        ColumnOption::Null | ColumnOption::NotNull => true,
                                        ColumnOption::Default(expr) => safe_default(expr),
                                        // The display comparison also excludes index options,
                                        // characteristics, INCLUDE, and USING on inline keys.
                                        ColumnOption::PrimaryKey(_) => {
                                            option.option.to_string() == "PRIMARY KEY"
                                        }
                                        ColumnOption::Unique(_) => {
                                            option.option.to_string() == "UNIQUE"
                                        }
                                        ColumnOption::ForeignKey(fk) => {
                                            safe_foreign_key(fk, prefix)
                                        }
                                        _ => false,
                                    }
                            })
                        {
                            return false;
                        }
                        definitions.push(column.to_string());
                    }
                    for constraint in &table.constraints {
                        let (name, body) = match constraint {
                            TableConstraint::PrimaryKey(key) => {
                                let Some(columns) = plain_index_columns(&key.columns) else {
                                    return false;
                                };
                                (&key.name, format!("PRIMARY KEY ({columns})"))
                            }
                            TableConstraint::Unique(key) => {
                                let Some(columns) = plain_index_columns(&key.columns) else {
                                    return false;
                                };
                                (&key.name, format!("UNIQUE ({columns})"))
                            }
                            TableConstraint::ForeignKey(fk) if safe_foreign_key(fk, prefix) => {
                                // safe_foreign_key checks every non-identifier field.
                                definitions.push(constraint.to_string());
                                continue;
                            }
                            _ => return false,
                        };
                        if name.as_ref().is_some_and(|n| !scoped_ident(n, prefix)) {
                            return false;
                        }
                        let named = name
                            .as_ref()
                            .map(|n| format!("CONSTRAINT {n} "))
                            .unwrap_or_default();
                        definitions.push(format!("{named}{body}"));
                    }
                    format!(
                        "CREATE TABLE {}{} ({})",
                        if table.if_not_exists {
                            "IF NOT EXISTS "
                        } else {
                            ""
                        },
                        table.name,
                        definitions.join(", ")
                    )
                }
                Statement::CreateIndex(index) => {
                    let Some(name) = &index.name else {
                        return false;
                    };
                    if !scoped_name(name, prefix) || !scoped_name(&index.table_name, prefix) {
                        return false;
                    }
                    let Some(columns) = plain_index_columns(&index.columns) else {
                        return false;
                    };
                    format!(
                        "CREATE {}INDEX {}{name} ON {} ({columns})",
                        if index.unique { "UNIQUE " } else { "" },
                        if index.if_not_exists {
                            "IF NOT EXISTS "
                        } else {
                            ""
                        },
                        index.table_name
                    )
                }
                _ => return false,
            };
            // Compare the complete AST against a projection containing ONLY approved
            // syntax. Extra clauses (including future parser fields) cannot be lost
            // in a partial pattern match and accidentally reach PostgreSQL.
            Parser::parse_sql(&PostgreSqlDialect {}, &projected)
                .is_ok_and(|safe| safe.as_slice() == std::slice::from_ref(statement))
        })
}

fn simple_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && name.as_bytes()[0].is_ascii_lowercase()
}

fn safe_ident(name: &Ident) -> bool {
    // Unquoted identifiers fold to lower case in PostgreSQL. Quoted names must
    // already be lower-case ASCII; no qualification, escapes, or truncation.
    simple_name(&name.value.to_ascii_lowercase())
        && (name.quote_style.is_none()
            || (name.quote_style == Some('"') && simple_name(&name.value)))
}

fn scoped_ident(name: &Ident, prefix: &str) -> bool {
    safe_ident(name)
        && name.value.to_ascii_lowercase().starts_with(prefix)
        && name.value.len() > prefix.len()
}

fn scoped_name(name: &ObjectName, prefix: &str) -> bool {
    name.0.len() == 1
        && name.0[0]
            .as_ident()
            .is_some_and(|n| scoped_ident(n, prefix))
}

fn plain_index_columns(columns: &[sqlparser::ast::IndexColumn]) -> Option<String> {
    if columns.is_empty() {
        return None;
    }
    let mut names = Vec::new();
    for column in columns {
        let Expr::Identifier(name) = &column.column.expr else {
            return None;
        };
        if !safe_ident(name) {
            return None;
        }
        // Only return identifiers; the complete statement comparison rejects
        // operator classes, ordering extensions and all expression indexes.
        names.push(name.to_string());
    }
    Some(names.join(", "))
}

fn safe_foreign_key(fk: &sqlparser::ast::ForeignKeyConstraint, prefix: &str) -> bool {
    fk.name.as_ref().is_none_or(|n| scoped_ident(n, prefix))
        && fk.index_name.is_none()
        && fk.on_delete.is_none()
        && fk.on_update.is_none()
        && fk.match_kind.is_none()
        && fk.characteristics.is_none()
        && scoped_name(&fk.foreign_table, prefix)
        && fk.columns.iter().all(safe_ident)
        && fk.referred_columns.iter().all(safe_ident)
}

fn safe_type(data_type: &DataType) -> bool {
    use DataType::*;
    match data_type {
        SmallInt(None)
        | Int2(None)
        | Int(None)
        | Int4(None)
        | Integer(None)
        | BigInt(None)
        | Int8(None)
        | Text
        | Boolean
        | Bool
        | Uuid
        | JSON
        | JSONB
        | Bytea
        | Date
        | Real
        | Float4
        | Float8
        | DoublePrecision
        | Numeric(_)
        | Decimal(_)
        | Dec(_)
        | Varchar(_)
        | CharacterVarying(_)
        | CharVarying(_)
        | Char(_)
        | Character(_)
        | Timestamp(_, _)
        | Time(_, _) => true,
        // PostgreSQL's SERIAL pseudo-types are represented as Custom by the
        // parser, but no actual custom/schema-qualified type is approved.
        Custom(name, modifiers) => {
            modifiers.is_empty()
                && name.0.len() == 1
                && name.0[0].as_ident().is_some_and(|n| {
                    n.quote_style.is_none()
                        && ["serial", "bigserial", "smallserial"]
                            .contains(&n.value.to_ascii_lowercase().as_str())
                })
        }
        _ => false,
    }
}

fn safe_default(expr: &Expr) -> bool {
    match expr {
        Expr::Value(value) => matches!(
            &value.value,
            Value::Number(_, false)
                | Value::SingleQuotedString(_)
                | Value::Boolean(_)
                | Value::Null
        ),
        Expr::UnaryOp {
            op: UnaryOperator::Minus | UnaryOperator::Plus,
            expr,
        } => {
            matches!(expr.as_ref(), Expr::Value(value) if matches!(value.value, Value::Number(_, false)))
        }
        Expr::Nested(expr) => safe_default(expr),
        // Exact rendering rejects qualification, arguments, FILTER/OVER,
        // function parameters, named arguments, and other call modifiers.
        Expr::Function(function) => matches!(
            function.to_string().to_ascii_lowercase().as_str(),
            "now()" | "current_timestamp" | "current_timestamp()"
        ),
        _ => false,
    }
}

/// Check an installed replacement before extraction or registry writes.
/// `restore` is reserved for trusted bundled startup packages, not HTTP input.
pub async fn check(s: &AppState, incoming: &Manifest, restore: bool) -> AppResult<()> {
    let Some(row) = reg::get(&s.pool, &incoming.id).await? else {
        return Ok(());
    };
    if row.enabled && !restore {
        return Err(AppError::Conflict("error.addon_disable_first"));
    }
    let identity = || AppError::Conflict("error.addon_upgrade_identity");
    if row.kind != incoming.kind || row.engine != incoming.engine_id() {
        return Err(identity());
    }
    let migrations = || AppError::Conflict("error.addon_upgrade_migrations");
    let applied = reg::applied_migration_checksums(&s.pool, &incoming.id).await?;
    // Freeze schema in both directions: even dropping a declaration loses the
    // evidence needed to reason about a later replacement.
    if incoming.migrations.len() != applied.len()
        || incoming.migrations.iter().any(|name| {
            !applied.get(name).is_some_and(|sum| {
                incoming
                    .files
                    .get(name)
                    .is_some_and(|new| new.eq_ignore_ascii_case(sum))
            })
        })
    {
        return Err(migrations());
    }
    // Older host-native rows have no package manifest. Only trusted startup
    // restoration may adopt them; never let an upload claim this exception.
    if row.source == "builtin" {
        if restore
            && row.manifest_hash == "builtin"
            && incoming.id == addons::builtin_id(&row.kind, &row.engine)
            && incoming.is_wasm()
            && applied.is_empty()
        {
            return Ok(());
        }
        return Err(AppError::Conflict("error.addon_upgrade_manifest"));
    }
    let unavailable = || AppError::Conflict("error.addon_upgrade_manifest");
    let raw = std::fs::read(addons::addons_dir().join(&incoming.id).join("addon.json"))
        .map_err(|_| unavailable())?;
    if !sha256_hex(&raw).eq_ignore_ascii_case(&row.manifest_hash) {
        return Err(unavailable());
    }
    let old: Manifest = serde_json::from_slice(&raw).map_err(|_| unavailable())?;
    addons::validate_manifest(s, &old).map_err(|_| unavailable())?;
    if old.id != row.id
        || old.version != row.version
        || old.kind != row.kind
        || old.engine_id() != row.engine
        || old.permissions != row.permissions
    {
        return Err(unavailable());
    }
    if old.kind != incoming.kind
        || old.engine != incoming.engine
        || old.provides != incoming.provides
    {
        return Err(identity());
    }
    if old.migrations != incoming.migrations {
        return Err(migrations());
    }
    if old.permissions != incoming.permissions
        || old.capabilities != incoming.capabilities
        || old.settings != incoming.settings
        || old.ui != incoming.ui
        || old.limits != incoming.limits
        || old.abi != incoming.abi
        || old.wasm != incoming.wasm
    {
        return Err(AppError::Conflict("error.addon_upgrade_contract"));
    }
    let version_error = || AppError::Conflict("error.addon_upgrade_version");
    let previous = Version::parse(&old.version).map_err(|_| version_error())?;
    let next = Version::parse(&incoming.version).map_err(|_| version_error())?;
    // Build metadata is not a version bump. Identical retries (including the
    // bundled startup seeds) remain safe, but may not change any manifest field.
    match next.cmp_precedence(&previous) {
        std::cmp::Ordering::Less => Err(version_error()),
        std::cmp::Ordering::Equal
            if serde_json::to_value(&old).map_err(|_| unavailable())?
                != serde_json::to_value(incoming).map_err(|_| unavailable())? =>
        {
            Err(version_error())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod migration_tests {
    use super::migration_safe;

    const PREFIX: &str = "addon_demo_";

    #[test]
    fn accepts_small_local_ddl_subset() {
        for sql in [
            "CREATE TABLE IF NOT EXISTS addon_demo_log (id BIGSERIAL PRIMARY KEY, message TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());",
            "CREATE TABLE addon_demo_values (id INT PRIMARY KEY, amount NUMERIC(12,2) DEFAULT -1.25, enabled BOOLEAN DEFAULT true, data JSONB DEFAULT '{}', note TEXT DEFAULT 'semi; -- not SQL', missing TEXT DEFAULT NULL);",
            "CREATE TABLE addon_demo_stamp (at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);",
            "CREATE TABLE addon_demo_child (id BIGINT REFERENCES addon_demo_parent(id));",
            "CREATE TABLE addon_demo_child (id INT, parent_id INT, CONSTRAINT addon_demo_pk PRIMARY KEY (id), CONSTRAINT addon_demo_fk FOREIGN KEY (parent_id) REFERENCES addon_demo_parent(id), UNIQUE (parent_id));",
            "CREATE UNIQUE INDEX IF NOT EXISTS addon_demo_unique ON addon_demo_log (id, message);",
            "-- leading comment\nCREATE TABLE addon_demo_a (id INT); /* separator */ CREATE INDEX addon_demo_idx ON addon_demo_a(id);",
            "CREATE TABLE \"addon_demo_quoted\" (\"id\" INTEGER NOT NULL);",
        ] {
            assert!(migration_safe(sql, PREFIX), "refused: {sql}");
        }
        assert!(migration_safe(
            include_str!("../../../../extensions/addons/hello-webhook/migrations/0001_init.sql"),
            "addon_hello_webhook_"
        ));
    }

    #[test]
    fn rejects_queries_functions_and_privileged_ddl() {
        for sql in [
            "CREATE TABLE addon_demo_copy AS SELECT * FROM users;",
            "CREATE TABLE addon_demo_copy AS TABLE users;",
            "CREATE TABLE addon_demo_copy (LIKE users INCLUDING ALL);",
            "CREATE TABLE addon_demo_copy (id INT) INHERITS (users);",
            "CREATE TABLE addon_demo_copy PARTITION OF users FOR VALUES IN (1);",
            "CREATE TABLE addon_demo_copy (id INT) PARTITION BY RANGE (id);",
            "CREATE TABLE addon_demo_copy (id INT) TABLESPACE pg_global;",
            "CREATE TABLE addon_demo_copy (id INT) USING evil;",
            "CREATE TABLE addon_demo_copy (id INT) WITH (autovacuum_enabled = false);",
            "CREATE UNLOGGED TABLE addon_demo_copy (id INT);",
            "CREATE TEMP TABLE addon_demo_copy (id INT);",
            "CREATE FOREIGN TABLE addon_demo_copy (id INT) SERVER hostile;",
            "CREATE TABLE addon_demo_copy (secret TEXT DEFAULT pg_read_file('/etc/passwd'));",
            "CREATE TABLE addon_demo_copy (secret TEXT DEFAULT (SELECT password_hash FROM users LIMIT 1));",
            "CREATE TABLE addon_demo_copy (id INT CHECK (pg_sleep(1) IS NULL));",
            "CREATE TABLE addon_demo_copy (id INT, CHECK (evil(id)));",
            "CREATE TABLE addon_demo_copy (id INT GENERATED ALWAYS AS (evil()) STORED);",
            "CREATE TABLE addon_demo_copy (id INT GENERATED ALWAYS AS IDENTITY);",
            "CREATE TABLE addon_demo_copy (id INT DEFAULT nextval('users_id_seq'));",
            "CREATE TABLE addon_demo_copy (id INT DEFAULT now(1));",
            "CREATE TABLE addon_demo_copy (id TEXT DEFAULT public.now());",
            "CREATE TABLE addon_demo_copy (id TEXT DEFAULT evil()::text);",
            "CREATE TABLE addon_demo_copy (id TEXT DEFAULT 'x'::evil_type);",
            "CREATE TABLE addon_demo_copy (id TEXT DEFAULT 'x' OPERATOR(public.||) 'y');",
            "CREATE TABLE addon_demo_copy (id evil_type);",
            "CREATE TABLE addon_demo_copy (id public.int);",
            "CREATE TABLE addon_demo_copy (id TEXT COLLATE evil);",
            "CREATE TABLE addon_demo_copy (id INT REFERENCES users(id));",
            "CREATE TABLE addon_demo_copy (id INT, FOREIGN KEY (id) REFERENCES public.addon_demo_parent(id));",
            "CREATE TABLE addon_demo_copy (id INT, PRIMARY KEY USING INDEX users_pkey);",
            "CREATE TABLE addon_demo_copy (id INT, EXCLUDE USING gist (id WITH =));",
            "CREATE INDEX addon_demo_idx ON users(id);",
            "CREATE INDEX addon_demo_idx ON addon_demo_log ((evil(id)));",
            "CREATE INDEX addon_demo_idx ON addon_demo_log (id) WHERE evil(id);",
            "CREATE INDEX addon_demo_idx ON addon_demo_log USING evil (id);",
            "CREATE INDEX addon_demo_idx ON addon_demo_log (id evil_ops);",
            "CREATE INDEX addon_demo_idx ON addon_demo_log (id) INCLUDE (other);",
            "CREATE INDEX addon_demo_idx ON addon_demo_log (id) WITH (fillfactor = 80);",
            "CREATE INDEX addon_demo_idx ON addon_demo_log (id) TABLESPACE pg_global;",
            "CREATE INDEX CONCURRENTLY addon_demo_idx ON addon_demo_log (id);",
            "CREATE INDEX public.addon_demo_idx ON addon_demo_log (id);",
            "CREATE TABLE public.addon_demo_copy (id INT);",
            "CREATE TABLE \"addon_demo_copy; SELECT\" (id INT);",
            "CREATE TABLE addon_other_copy (id INT);",
            "CREATE TABLE addon_demo_copy (id INT); SELECT * FROM users;",
            "CREATE TABLE addon_demo_copy (id INT); DO $$ BEGIN PERFORM evil(); END $$;",
            "ALTER TABLE addon_demo_log ADD COLUMN id INT;",
            "DROP TABLE addon_demo_log;",
            "COPY users TO PROGRAM 'evil';",
            "GRANT ALL ON users TO PUBLIC;",
            "", "-- comments only", "CREATE TABLE",
        ] {
            assert!(!migration_safe(sql, PREFIX), "accepted: {sql}");
        }
        assert!(!migration_safe("CREATE TABLE addon_demo_log (id INT)", ""));
        assert!(!migration_safe(
            "CREATE TABLE addon_demo_log (id INT)",
            "addon_demo"
        ));
        let truncated = format!("CREATE TABLE addon_demo_{} (id INT)", "x".repeat(64));
        assert!(!migration_safe(&truncated, PREFIX));
    }
}
