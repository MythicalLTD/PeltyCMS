//! `eula.txt` gate (Minecraft-server style). The first start writes
//! `eula.txt` with `eula=false`; `serve` refuses to start until it says
//! `eula=true` or the environment sets `PELTY_EULA=true` (Docker / CI).
use std::path::Path;

pub const FILE: &str = "eula.txt";
/// Where the EULA is published (repository copy: `EULA.md`).
pub const URL: &str = "https://peltycms.com/eula";

/// Contents of a new `eula.txt`; `now` is an RFC 3339 timestamp.
pub fn template(now: &str) -> String {
    format!(
        "# PeltyCMS End User License Agreement\n\
         # By changing the setting below to TRUE you agree to the PeltyCMS License (LICENSE),\n\
         # EULA (EULA.md / {URL}) and Free Tier rules (≤50 Active Clients; Soft overage notices).\n\
         # Commercial terms: COMMERCIAL.md. Alternatively set PELTY_EULA=true (Docker / CI).\n\
         # Created by pelty-server {} on {now}\n\
         eula=false\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// `eula=true` (case-insensitive) on a non-comment line.
pub fn accepted_in(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .any(|(k, v)| {
            k.trim().eq_ignore_ascii_case("eula") && v.trim().eq_ignore_ascii_case("true")
        })
}

pub fn env_accepts(v: Option<&str>) -> bool {
    v.is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
}

pub enum Gate {
    Accepted,
    /// Not accepted; `created` when `eula.txt` was written just now.
    Refused {
        created: bool,
    },
}

/// Write `eula.txt` when missing, then check acceptance.
pub fn check(root: &Path, env: Option<&str>) -> Gate {
    let path = root.join(FILE);
    let now = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let created = match crate::bootstrap::write_new(&path, &template(&now)) {
        Ok(c) => c,
        Err(e) => {
            if !env_accepts(env) {
                println!(
                    "[PeltyCMS] warning: could not write {}: {e}",
                    path.display()
                );
            }
            false
        }
    };
    if created {
        println!("[PeltyCMS] created {FILE}");
    }
    if env_accepts(env) || std::fs::read_to_string(&path).is_ok_and(|t| accepted_in(&t)) {
        Gate::Accepted
    } else {
        Gate::Refused { created }
    }
}

pub fn refusal_message(root: &Path) -> String {
    format!(
        "You need to agree to the PeltyCMS EULA in order to run the server.\n\
         Read it at {URL} (or EULA.md), then set eula=true in {}\n\
         (or set the environment variable PELTY_EULA=true) and start pelty-server again.",
        root.join(FILE).display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_acceptance() {
        assert!(!accepted_in(&template("t")));
        assert!(accepted_in(
            &template("t").replace("eula=false", "eula=true")
        ));
        assert!(accepted_in("eula = TRUE\n"));
        assert!(!accepted_in("#eula=true\neula=false\n"));
        assert!(!accepted_in("eula=yes\n"));
        assert!(
            env_accepts(Some("true"))
                && env_accepts(Some("1"))
                && !env_accepts(Some("false"))
                && !env_accepts(None)
        );
    }

    #[test]
    fn gate_writes_file_once_and_respects_edits() {
        let d = std::env::temp_dir().join(format!("pelty-eula-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(matches!(check(&d, None), Gate::Refused { created: true }));
        let first = std::fs::read_to_string(d.join(FILE)).unwrap();
        assert!(first.contains("eula=false") && first.contains(URL));
        assert!(matches!(check(&d, None), Gate::Refused { created: false }));
        assert_eq!(std::fs::read_to_string(d.join(FILE)).unwrap(), first);
        assert!(matches!(check(&d, Some("true")), Gate::Accepted));
        std::fs::write(d.join(FILE), first.replace("eula=false", "eula=true")).unwrap();
        assert!(matches!(check(&d, None), Gate::Accepted));
        std::fs::remove_dir_all(&d).ok();
    }
}
