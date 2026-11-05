//! Runs the real binary in an empty folder: first-run bootstrap and the EULA
//! gate happen before any database access, so no PostgreSQL is needed.
use std::path::Path;
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pelty-server"));
    c.current_dir(dir).args(args).env_remove("PELTY_EULA").env_remove("DATABASE_URL").env_remove("PELTY_ENCRYPTION_KEY");
    for (k, _) in std::env::vars() {
        if k.starts_with("PELTY_") {
            c.env_remove(k);
        }
    }
    c.envs(env.iter().copied()).stdin(std::process::Stdio::null());
    c.output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn first_start_bootstraps_and_stops_at_eula() {
    let d = std::env::temp_dir().join(format!("pelty-first-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();

    let o = run(&d, &[], &[]);
    let out = text(&o);
    assert!(!o.status.success(), "{out}");
    assert!(out.contains("created .env"), "{out}");
    assert!(out.contains("created eula.txt"), "{out}");
    assert!(out.contains("agree to the PeltyCMS EULA"), "{out}");
    for p in [".env", "eula.txt", "locales/en.json", "data/addons", "extensions/themes", "extensions/plugins", "backups"] {
        assert!(d.join(p).exists(), "{p}");
    }
    let env1 = std::fs::read_to_string(d.join(".env")).unwrap();
    assert!(std::fs::read_to_string(d.join("eula.txt")).unwrap().contains("eula=false"));

    // Second start: nothing is recreated, still refused.
    let o = run(&d, &["serve"], &[]);
    let out = text(&o);
    assert!(!o.status.success());
    assert!(!out.contains("created"), "{out}");
    assert_eq!(std::fs::read_to_string(d.join(".env")).unwrap(), env1);

    // Accepted via the environment: the gate passes and config loading runs
    // (an invalid DATABASE_URL stops it before any connection attempt).
    let o = run(&d, &["serve"], &[("PELTY_EULA", "true"), ("DATABASE_URL", "mysql://nope")]);
    let out = text(&o);
    assert!(!out.contains("agree to the PeltyCMS EULA"), "{out}");
    assert!(out.contains("DATABASE_URL"), "{out}");

    // Accepted in eula.txt.
    let e = std::fs::read_to_string(d.join("eula.txt")).unwrap().replace("eula=false", "eula=true");
    std::fs::write(d.join("eula.txt"), e).unwrap();
    let o = run(&d, &[], &[("DATABASE_URL", "mysql://nope")]);
    assert!(!text(&o).contains("agree to the PeltyCMS EULA"));

    // Without a terminal the wizard never runs (the marker stays pending) and
    // `setup` explains instead of hanging on input.
    assert!(std::fs::read_to_string(d.join(".env")).unwrap().contains("PELTY_SETUP=pending"));
    let o = run(&d, &["setup"], &[]);
    assert!(!o.status.success());
    assert!(text(&o).contains("needs an interactive terminal"), "{}", text(&o));

    // Other subcommands are not gated.
    std::fs::remove_file(d.join("eula.txt")).unwrap();
    let o = run(&d, &["bogus"], &[]);
    assert!(text(&o).contains("usage: pelty-server"), "{}", text(&o));
    std::fs::remove_dir_all(&d).ok();
}
