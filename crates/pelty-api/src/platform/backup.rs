//! Database backups via `pg_dump -Fc`, run as a background job. Files live in
//! `PELTY_BACKUP_DIR` (default `./backups`); names are generated server-side.
use crate::worker::JobError;
use crate::AppState;
use pelty_database::{admin_ops, jobs::Job};
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

pub const JOB_KIND: &str = "backup.pg_dump";

pub fn dir() -> PathBuf {
    // Where database backups are written (default `backups`).
    std::env::var("PELTY_BACKUP_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("backups"))
}

/// Path for a stored backup; rejects anything that is not a plain file name.
pub fn path_for(name: &str) -> Option<PathBuf> {
    let ok = !name.is_empty() && name.len() <= 120 && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
    ok.then(|| dir().join(name))
}

pub async fn run_job(s: &AppState, job: &Job) -> Result<Value, JobError> {
    let id: Uuid = job.payload.get("backup_id").and_then(Value::as_str).and_then(|x| x.parse().ok())
        .ok_or_else(|| JobError::Permanent("bad payload".into()))?;
    dump(&s.pool, &s.cfg.database_url, id).await
}

/// Take a backup right now (no job queue): used by `pelty-server update`
/// before it replaces the binary. Listed in Admin > Backups like any other.
pub async fn backup_now(pool: &pelty_database::Pool, database_url: &str) -> Result<PathBuf, String> {
    let b = admin_ops::create_backup(pool, None).await.map_err(|e| e.to_string())?;
    dump(pool, database_url, b.id).await.map_err(|e| match e {
        JobError::Retry(e) | JobError::Permanent(e) => e,
    })?;
    path_for(&b.file_name).ok_or_else(|| "bad file name".into())
}

/// Run `pg_dump` for backup `id` and record the outcome on its row.
async fn dump(pool: &pelty_database::Pool, database_url: &str, id: Uuid) -> Result<Value, JobError> {
    let b = admin_ops::backup(pool, id).await.map_err(|e| JobError::Permanent(e.to_string()))?;
    let path = path_for(&b.file_name).ok_or_else(|| JobError::Permanent("bad file name".into()))?;
    if let Err(e) = std::fs::create_dir_all(dir()) {
        let _ = admin_ops::finish_backup(pool, id, None, Some(&e.to_string())).await;
        return Err(JobError::Permanent(e.to_string()));
    }
    let (p2, url) = (path.clone(), database_url.to_string());
    let out = tokio::task::spawn_blocking(move || {
        // `pg_dump` binary used for backups (default `pg_dump` on PATH).
        std::process::Command::new(std::env::var("PELTY_PG_DUMP").unwrap_or_else(|_| "pg_dump".into()))
            .arg("--format=custom").arg("--no-owner").arg("--file").arg(&p2).arg("--dbname").arg(&url)
            .output()
    }).await.map_err(|e| JobError::Retry(e.to_string()))?;
    let err = match out {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(String::from_utf8_lossy(&o.stderr).chars().take(500).collect::<String>()),
        Err(e) => Some(format!("pg_dump not runnable: {e}")),
    };
    if let Some(e) = err {
        let _ = std::fs::remove_file(&path);
        admin_ops::finish_backup(pool, id, None, Some(&e)).await.map_err(|e| JobError::Retry(e.to_string()))?;
        return Err(JobError::Permanent(e));
    }
    let size = std::fs::metadata(&path).map(|m| m.len() as i64).ok();
    admin_ops::finish_backup(pool, id, size, None).await.map_err(|e| JobError::Retry(e.to_string()))?;
    Ok(json!({ "file": b.file_name, "size": size }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn paths_are_confined() {
        assert!(super::path_for("pelty-1-abc.dump").is_some());
        for bad in ["../x", "a/b", ".env", "", "A.dump"] { assert!(super::path_for(bad).is_none(), "{bad}"); }
    }
}
