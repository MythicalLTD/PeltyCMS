//! Full site backups: PostgreSQL dump (`pg_dump -Fc`) plus on-disk state
//! (addons, themes, plugins, media, locales, `.env`, `eula.txt`) packed as a
//! zip under `PELTY_BACKUP_DIR` (default `./backups`). Run as a background job.
use crate::worker::JobError;
use crate::AppState;
use pelty_database::{admin_ops, jobs::Job};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub const JOB_KIND: &str = "backup.site";

pub fn dir() -> PathBuf {
    // Where site backups are written (default `backups`).
    std::env::var("PELTY_BACKUP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("backups"))
}

/// Path for a stored backup; rejects anything that is not a plain file name.
pub fn path_for(name: &str) -> Option<PathBuf> {
    let ok = !name.is_empty()
        && name.len() <= 120
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
    ok.then(|| dir().join(name))
}

pub async fn run_job(s: &AppState, job: &Job) -> Result<Value, JobError> {
    let id: Uuid = job
        .payload
        .get("backup_id")
        .and_then(Value::as_str)
        .and_then(|x| x.parse().ok())
        .ok_or_else(|| JobError::Permanent("bad payload".into()))?;
    archive(&s.pool, &s.cfg.database_url, id).await
}

/// Take a backup right now (no job queue): used by `pelty-server update`
/// before it replaces the binary. Listed in Admin > Backups like any other.
pub async fn backup_now(
    pool: &pelty_database::Pool,
    database_url: &str,
) -> Result<PathBuf, String> {
    let b = admin_ops::create_backup(pool, None)
        .await
        .map_err(|e| e.to_string())?;
    archive(pool, database_url, b.id)
        .await
        .map_err(|e| match e {
            JobError::Retry(e) | JobError::Permanent(e) => e,
        })?;
    path_for(&b.file_name).ok_or_else(|| "bad file name".into())
}

/// Run `pg_dump` and pack the site into backup `id`'s zip; record the outcome.
async fn archive(
    pool: &pelty_database::Pool,
    database_url: &str,
    id: Uuid,
) -> Result<Value, JobError> {
    let b = admin_ops::backup(pool, id)
        .await
        .map_err(|e| JobError::Permanent(e.to_string()))?;
    let path = path_for(&b.file_name).ok_or_else(|| JobError::Permanent("bad file name".into()))?;
    if let Err(e) = std::fs::create_dir_all(dir()) {
        let _ = admin_ops::finish_backup(pool, id, None, Some(&e.to_string())).await;
        return Err(JobError::Permanent(e.to_string()));
    }
    let (p2, url) = (path.clone(), database_url.to_string());
    let out = tokio::task::spawn_blocking(move || build_archive(&p2, &url))
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    let err = match out {
        Ok(()) => None,
        Err(e) => Some(e.chars().take(500).collect::<String>()),
    };
    if let Some(e) = err {
        let _ = std::fs::remove_file(&path);
        admin_ops::finish_backup(pool, id, None, Some(&e))
            .await
            .map_err(|e| JobError::Retry(e.to_string()))?;
        return Err(JobError::Permanent(e));
    }
    let size = std::fs::metadata(&path).map(|m| m.len() as i64).ok();
    admin_ops::finish_backup(pool, id, size, None)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    Ok(json!({ "file": b.file_name, "size": size }))
}

fn build_archive(zip_path: &Path, database_url: &str) -> Result<(), String> {
    let staging = zip_path.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("mkdir staging: {e}"))?;
    let dump_path = staging.join("database.dump");
    run_pg_dump(&dump_path, database_url)?;

    let file = std::fs::File::create(zip_path).map_err(|e| format!("create zip: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    add_file(&mut zip, &dump_path, "database.dump", opts)?;

    let mut included = vec!["database.dump".to_string()];
    for (label, src) in site_dirs() {
        if !src.is_dir() {
            continue;
        }
        // Never nest the backup output folder into itself.
        if same_or_under(&src, &dir()) {
            continue;
        }
        let prefix = format!("files/{label}");
        let n = add_tree(&mut zip, &src, &prefix, opts)?;
        if n > 0 {
            included.push(format!("{prefix}/ ({n} files)"));
        }
    }
    for (name, path) in [
        ("config/.env", PathBuf::from(".env")),
        ("config/eula.txt", PathBuf::from("eula.txt")),
    ] {
        if path.is_file() {
            add_file(&mut zip, &path, name, opts)?;
            included.push(name.to_string());
        }
    }

    let manifest = json!({
        "format": 1,
        "kind": "pelty-site",
        "created_at": time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        "version": env!("CARGO_PKG_VERSION"),
        "includes": included,
        "restore": {
            "database": "pg_restore --clean --if-exists --no-owner -d \"$DATABASE_URL\" database.dump",
            "files": "extract files/* over the install folder (addons, themes, …)",
            "config": "restore config/.env (keep PELTY_ENCRYPTION_KEY) and config/eula.txt"
        }
    });
    zip.start_file("manifest.json", opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes())
        .map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;

    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
}

fn run_pg_dump(path: &Path, database_url: &str) -> Result<(), String> {
    let out = std::process::Command::new(
        // `pg_dump` binary used for backups (default `pg_dump` on PATH).
        std::env::var("PELTY_PG_DUMP").unwrap_or_else(|_| "pg_dump".into()),
    )
    .arg("--format=custom")
    .arg("--no-owner")
    .arg("--file")
    .arg(path)
    .arg("--dbname")
    .arg(database_url)
    .output()
    .map_err(|e| format!("pg_dump not runnable: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(500)
            .collect())
    }
}

/// On-disk folders that belong in a full site backup (label → path).
fn site_dirs() -> Vec<(String, PathBuf)> {
    let env_or = |k: &str, default: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(default))
    };
    let addons = crate::addons::addons_dir();
    let addon_data = env_or("PELTY_ADDON_DATA_DIR", "data/addons");
    let mut v = vec![
        ("addons".into(), addons.clone()),
        (
            "themes".into(),
            env_or("PELTY_THEMES_DIR", "extensions/themes"),
        ),
        (
            "plugins".into(),
            env_or("PELTY_PLUGINS_DIR", "extensions/plugins"),
        ),
        ("media".into(), env_or("PELTY_MEDIA_DIR", "data/media")),
        ("locales".into(), env_or("PELTY_LOCALES_DIR", "locales")),
    ];
    // Separate addon data tree when it is not the same as packages.
    if addon_data.canonicalize().ok() != addons.canonicalize().ok() {
        v.push(("addon-data".into(), addon_data));
    }
    v
}

fn same_or_under(path: &Path, root: &Path) -> bool {
    let (Ok(p), Ok(r)) = (path.canonicalize(), root.canonicalize()) else {
        return path == root;
    };
    p == r || p.starts_with(&r)
}

fn add_file(
    zip: &mut ZipWriter<std::fs::File>,
    src: &Path,
    name: &str,
    opts: SimpleFileOptions,
) -> Result<(), String> {
    zip.start_file(name, opts).map_err(|e| e.to_string())?;
    let mut f = std::fs::File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    std::io::copy(&mut f, zip).map_err(|e| e.to_string())?;
    Ok(())
}

fn add_tree(
    zip: &mut ZipWriter<std::fs::File>,
    dir: &Path,
    prefix: &str,
    opts: SimpleFileOptions,
) -> Result<usize, String> {
    let mut n = 0;
    let mut stack = vec![(dir.to_path_buf(), prefix.to_string())];
    while let Some((cur, pref)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&cur)
            .map_err(|e| format!("read {}: {e}", cur.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            // Skip VCS / junk and never pack nested backup staging.
            if name == ".git" || name == "node_modules" || name.ends_with(".staging") {
                continue;
            }
            let path = e.path();
            let rel = format!("{pref}/{name}");
            if path.is_dir() {
                stack.push((path, rel));
            } else if path.is_file() {
                add_file(zip, &path, &rel, opts)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_confined() {
        assert!(path_for("pelty-1-abc.zip").is_some());
        assert!(path_for("pelty-1-abc.dump").is_some());
        for bad in ["../x", "a/b", ".env", "", "A.zip"] {
            assert!(path_for(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn site_dirs_lists_defaults() {
        let labels: Vec<_> = site_dirs().into_iter().map(|(l, _)| l).collect();
        assert!(labels.contains(&"addons".into()));
        assert!(labels.contains(&"themes".into()));
        assert!(labels.contains(&"media".into()));
    }
}
