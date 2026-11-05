//! First-run bootstrap (Minecraft-server style): when `pelty-server` starts in
//! a folder it writes a commented `.env` (with a freshly generated encryption
//! key), creates the folders the server writes to and the default locale
//! files. Idempotent: existing files are never overwritten; later starts only
//! create what is missing.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Name of the generated config file (read on every start).
pub const ENV_FILE: &str = ".env";
const ENV_TEMPLATE: &str = include_str!("env.template");
/// Default translations, written when the locales folder has no `en.json`.
const LOCALES: &[(&str, &str)] = &[
    ("en.json", include_str!("../../../../locales/en.json")),
    ("ro.json", include_str!("../../../../locales/ro.json")),
];

/// What a bootstrap pass did, for the startup messages.
#[derive(Debug, Default)]
pub struct Report {
    pub created: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn print(&self) {
        for c in &self.created {
            println!("[PeltyCMS] created {c}");
        }
        for w in &self.warnings {
            println!("[PeltyCMS] warning: {w}");
        }
    }
}

/// Resolve a configured path against the server folder.
fn at(root: &Path, p: &str) -> PathBuf {
    let p = PathBuf::from(p);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

/// Base64 of 32 random bytes from the OS RNG (`PELTY_ENCRYPTION_KEY` format).
pub fn generate_key() -> String {
    use base64::Engine;
    use rand::Rng;
    let mut k = [0u8; 32];
    rand::rng().fill_bytes(&mut k);
    base64::engine::general_purpose::STANDARD.encode(k)
}

/// The `.env` written on first start.
pub fn env_template(key: &str) -> String {
    ENV_TEMPLATE.replace("{{ENCRYPTION_KEY}}", key)
}

/// Write `content` to `path` only when it does not exist yet (never
/// overwrites, also not in a race with another process).
pub fn write_new(path: &Path, content: &str) -> std::io::Result<bool> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if path.file_name().is_some_and(|n| n == ENV_FILE) {
            opts.mode(0o600);
        }
    }
    match opts.open(path) {
        Ok(mut f) => {
            f.write_all(content.as_bytes())?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// Create `<root>/.env` from the template when missing.
pub fn ensure_env_file(root: &Path, r: &mut Report) {
    let p = root.join(ENV_FILE);
    match write_new(&p, &env_template(&generate_key())) {
        Ok(true) => r.created.push(format!(
            "{ENV_FILE} (configuration with a new PELTY_ENCRYPTION_KEY; edit DATABASE_URL)"
        )),
        Ok(false) => {}
        Err(e) => r
            .warnings
            .push(format!("could not write {}: {e}", p.display())),
    }
}

/// Parse `KEY=value` lines (`#` comments, optional `export `, quotes).
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let v = v.trim();
            let v = if v.len() >= 2
                && ((v.starts_with('"') && v.ends_with('"'))
                    || (v.starts_with('\'') && v.ends_with('\'')))
            {
                &v[1..v.len() - 1]
            } else {
                v
            };
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Values from `<root>/.env` (empty when the file is missing).
pub fn read_env_file(root: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(root.join(ENV_FILE))
        .map(|t| parse_env(&t))
        .unwrap_or_default()
}

/// Folders the server writes to or reads from, with the env var that moves
/// each one and its default (the same defaults the code uses).
pub const DIRS: &[(&str, &str, &str)] = &[
    ("PELTY_ADDONS_DIR", "data/addons", "addon packages"),
    ("PELTY_ADDON_DATA_DIR", "data/addons", "addon data"),
    ("PELTY_THEMES_DIR", "extensions/themes", "themes"),
    ("PELTY_PLUGINS_DIR", "extensions/plugins", "webhook plugins"),
    ("PELTY_LOCALES_DIR", "locales", "translations"),
    ("PELTY_BACKUP_DIR", "backups", "database backups"),
];

/// Create missing folders and default locale files. `get` reads the
/// effective configuration (environment, then `.env`).
pub fn ensure_dirs(root: &Path, get: &dyn Fn(&str) -> Option<String>, r: &mut Report) {
    let mut seen = Vec::new();
    for (var, default, what) in DIRS {
        // The legacy `./addons` folder stays in use when present (see pelty_api::addons::addons_dir).
        if *var == "PELTY_ADDONS_DIR" && get(var).is_none() && root.join("addons").is_dir() {
            continue;
        }
        let rel = get(var).unwrap_or_else(|| default.to_string());
        let dir = at(root, &rel);
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir.clone());
        if dir.is_dir() {
            continue;
        }
        match std::fs::create_dir_all(&dir) {
            Ok(()) => r.created.push(format!("{rel}/ ({what})")),
            Err(e) => r
                .warnings
                .push(format!("could not create {} ({what}): {e}", dir.display())),
        }
    }
    let loc_rel = get("PELTY_LOCALES_DIR").unwrap_or_else(|| "locales".into());
    let loc = at(root, &loc_rel);
    if loc.is_dir() && !loc.join("en.json").exists() {
        for (name, body) in LOCALES {
            match write_new(&loc.join(name), body) {
                Ok(true) => r
                    .created
                    .push(format!("{loc_rel}/{name} (default translations)")),
                Ok(false) => {}
                Err(e) => r
                    .warnings
                    .push(format!("could not write {}: {e}", loc.join(name).display())),
            }
        }
    }
    let th_rel = get("PELTY_THEMES_DIR").unwrap_or_else(|| "extensions/themes".into());
    match crate::embedded::seed_themes(&at(root, &th_rel)) {
        Ok(made) if !made.is_empty() => r
            .created
            .push(format!("{th_rel}/ default themes ({})", made.join(", "))),
        Ok(_) => {}
        Err(e) => r.warnings.push(format!(
            "could not write the default themes to {th_rel}/: {e}"
        )),
    }
    let fe_rel = get("PELTY_FRONTEND_DIR").unwrap_or_else(|| "frontend/dist".into());
    if !at(root, &fe_rel).join("index.html").exists() && !crate::embedded::has_frontend() {
        r.warnings.push(format!(
            "no web UI build in {fe_rel}/ (set PELTY_FRONTEND_DIR); only the API is served"
        ));
    }
}

/// Full bootstrap of `root`: `.env`, then folders (using `env` overlaid on
/// the `.env` values). Returns the `.env` values so the caller can export
/// the ones the environment does not set.
pub fn run(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> (Report, Vec<(String, String)>) {
    let mut r = Report::default();
    ensure_env_file(root, &mut r);
    let file = read_env_file(root);
    let map: HashMap<String, String> = file.iter().cloned().collect();
    let get = |k: &str| {
        env(k)
            .filter(|v| !v.trim().is_empty())
            .or_else(|| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
    };
    ensure_dirs(root, &get, &mut r);
    (r, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("pelty-boot-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn first_run_creates_config_dirs_and_locales() {
        let d = tmp();
        let (r, vars) = run(&d, &|_| None);
        assert!(d.join(".env").is_file());
        for p in [
            "data/addons",
            "extensions/themes",
            "extensions/plugins",
            "locales",
            "backups",
        ] {
            assert!(d.join(p).is_dir(), "{p}");
        }
        assert!(d.join("locales/en.json").is_file());
        let key = vars
            .iter()
            .find(|(k, _)| k == "PELTY_ENCRYPTION_KEY")
            .map(|(_, v)| v.clone())
            .unwrap();
        use base64::Engine;
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&key)
                .unwrap()
                .len(),
            32
        );
        assert!(vars.iter().any(|(k, _)| k == "DATABASE_URL"));
        assert!(r.created.iter().any(|c| c.starts_with(".env")));
        // No UI warning when the binary carries an embedded UI build.
        assert_eq!(
            r.warnings.iter().any(|w| w.contains("frontend/dist")),
            !crate::embedded::has_frontend()
        );
        assert!(!env_template("x").contains("{{"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn idempotent_and_never_overwrites() {
        let d = tmp();
        run(&d, &|_| None);
        std::fs::write(d.join(".env"), "DATABASE_URL=postgres://mine\n").unwrap();
        std::fs::write(d.join("locales/en.json"), "{}").unwrap();
        let (r, vars) = run(&d, &|_| None);
        assert!(r.created.is_empty(), "{:?}", r.created);
        assert_eq!(
            std::fs::read_to_string(d.join(".env")).unwrap(),
            "DATABASE_URL=postgres://mine\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("locales/en.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            vars,
            vec![("DATABASE_URL".to_string(), "postgres://mine".to_string())]
        );
        // A deleted folder is recreated on the next start.
        std::fs::remove_dir_all(d.join("backups")).unwrap();
        let (r, _) = run(&d, &|_| None);
        assert_eq!(r.created.len(), 1);
        assert!(d.join("backups").is_dir());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn env_overrides_folders() {
        let d = tmp();
        let (_, _) = run(&d, &|k| {
            (k == "PELTY_BACKUP_DIR").then(|| "store/bk".to_string())
        });
        assert!(d.join("store/bk").is_dir());
        assert!(!d.join("backups").exists());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn keys_differ() {
        assert_ne!(generate_key(), generate_key());
    }

    #[test]
    fn parses_env_lines() {
        let v = parse_env("# c\nA=1\nexport B = \"two words\"\n\nC='x=y'\nbad line\n#D=4\n");
        assert_eq!(
            v,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two words".into()),
                ("C".into(), "x=y".into())
            ]
        );
    }
}
