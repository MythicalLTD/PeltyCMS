//! `pelty-cli`, developer tooling for PeltyCMS addons.
//!
//! ```text
//! pelty-cli addon scaffold <id> [--kind plugin] [--engine <engine>] [--name "My addon"] [--dir .]
//! pelty-cli addon wasm-scaffold <id> --kind K [--lang rust|c|tinygo] [--name N] [--dir D] [--sdk PATH]
//! pelty-cli addon hash <dir>         # recompute the `files` sha256 map
//! pelty-cli addon validate <dir|zip> # structure, ids, paths, hashes
//! pelty-cli addon pack <dir> [-o out.zip]
//! pelty-cli addon keygen <private.pk8> --key-id ID --public-out trusted-keys.json
//! pelty-cli addon sign <dir> --key <private.pk8> --key-id ID
//! pelty-cli addon verify <dir|zip> --trusted-keys trusted-keys.json [--require-signed]
//! ```
mod wasm_scaffold;

use pelty_extensions::addon_pkg::{self, Limits};
use pelty_extensions::addon_signing::{self, TrustedKey, TrustedKeys, SIGNATURE_FILE};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const KINDS: &[&str] = &[
    "provisioning",
    "gateway",
    "registrar",
    "theme",
    "plugin",
    "captcha",
    "sso",
    "integration",
];
const PERMISSIONS: &[&str] = &[
    "payments.process",
    "provisioning.manage",
    "domains.manage",
    "network.outbound",
    "db.migrate",
    "ui.extend",
];

fn usage() -> ExitCode {
    eprintln!("usage:\n  pelty-cli addon scaffold <id> [--kind K] [--engine E] [--name N] [--dir D]\n  pelty-cli addon wasm-scaffold <id> --kind K [--lang rust|c|tinygo] [--name N] [--dir D] [--sdk PATH]\n  pelty-cli addon hash <dir>\n  pelty-cli addon validate <dir|file.zip>\n  pelty-cli addon pack <dir> [-o out.zip]\n  pelty-cli addon keygen <private.pk8> --key-id ID --public-out trusted-keys.json\n  pelty-cli addon sign <dir> --key <private.pk8> --key-id ID\n  pelty-cli addon verify <dir|file.zip> --trusted-keys trusted-keys.json [--require-signed]");
    ExitCode::from(2)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Checks that need no running server (the server additionally checks the
/// engine is compiled in and migrations only touch `addon_<id>_*`).
pub fn lint_manifest(m: &Value) -> Vec<String> {
    let mut errs = vec![];
    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("");
    if !addon_pkg::id_ok(s("id")) {
        errs.push("id must match [a-z0-9-]{3,64}".into());
    }
    for k in ["name", "version", "engine"] {
        if s(k).trim().is_empty() {
            errs.push(format!("{k} is required"));
        }
    }
    if !KINDS.contains(&s("kind")) {
        errs.push(format!("kind must be one of {}", KINDS.join(", ")));
    }
    for p in m
        .get("permissions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !p.as_str().is_some_and(|p| PERMISSIONS.contains(&p)) {
            errs.push(format!("unknown permission {p}"));
        }
    }
    for st in m
        .get("settings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = st.get("kind").and_then(Value::as_str).unwrap_or("text");
        if st
            .get("key")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
            || !["text", "number", "bool", "select", "textarea", "secret"].contains(&kind)
        {
            errs.push(format!("invalid setting {st}"));
        }
        if kind == "select"
            && st
                .get("choices")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
        {
            errs.push("select setting needs choices".into());
        }
    }
    if let Some(files) = m.get("files").and_then(Value::as_object) {
        for mig in m
            .get("migrations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !mig.as_str().is_some_and(|x| files.contains_key(x)) {
                errs.push(format!("migration {mig} not listed in files"));
            }
        }
    }
    errs
}

fn validate(path: &Path) -> Result<String, String> {
    let bytes = if path.is_dir() {
        addon_pkg::pack(path).map_err(|e| e.to_string())?
    } else {
        std::fs::read(path).map_err(|e| e.to_string())?
    };
    let pkg = addon_pkg::inspect(&bytes, Limits::default()).map_err(|e| e.to_string())?;
    let errs = lint_manifest(&pkg.manifest);
    if !errs.is_empty() {
        return Err(errs.join("\n"));
    }
    Ok(format!(
        "ok: {} ({} files, {} bytes packed, sha256 {})",
        pkg.id,
        pkg.files.len(),
        bytes.len(),
        addon_pkg::sha256_hex(&bytes)
    ))
}

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("input exceeds size limit".into());
    }
    Ok(bytes)
}

/// create_new prevents clobbering existing files or following an existing symlink.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = std::fs::remove_file(path);
        return Err(e.to_string());
    }
    Ok(())
}

fn signing_options(args: &[String], values: &[&str], switches: &[&str]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 3;
    while i < args.len() {
        let name = args[i].as_str();
        if !seen.insert(name) {
            return Err(format!("duplicate option {name}"));
        }
        if values.contains(&name) {
            if args
                .get(i + 1)
                .is_none_or(|v| v.is_empty() || v.starts_with("--"))
            {
                return Err(format!("missing value for {name}"));
            }
            i += 2;
        } else if switches.contains(&name) {
            i += 1;
        } else {
            return Err(format!("unknown signing option {name}"));
        }
    }
    for name in values {
        if !seen.contains(name) {
            return Err(format!("missing option {name}"));
        }
    }
    Ok(())
}

fn run(args: &[String]) -> Result<String, String> {
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
        args.get(2),
    ) {
        (Some("addon"), Some("scaffold"), Some(id)) => {
            let kind = flag(args, "--kind").unwrap_or_else(|| "plugin".into());
            let engine = flag(args, "--engine").unwrap_or_else(|| id.clone());
            let name = flag(args, "--name").unwrap_or_else(|| id.clone());
            let dir = PathBuf::from(flag(args, "--dir").unwrap_or_else(|| ".".into()));
            if !KINDS.contains(&kind.as_str()) {
                return Err(format!("kind must be one of {}", KINDS.join(", ")));
            }
            let d =
                addon_pkg::scaffold(&dir, id, &kind, &engine, &name).map_err(|e| e.to_string())?;
            Ok(format!("created {}", d.display()))
        }
        (Some("addon"), Some("wasm-scaffold"), Some(id)) => {
            let kind = flag(args, "--kind").unwrap_or_else(|| "integration".into());
            if !KINDS.contains(&kind.as_str()) {
                return Err(format!("kind must be one of {}", KINDS.join(", ")));
            }
            let lang = flag(args, "--lang").unwrap_or_else(|| "rust".into());
            let name = flag(args, "--name").unwrap_or_else(|| id.clone());
            let dir = PathBuf::from(flag(args, "--dir").unwrap_or_else(|| ".".into()));
            let sdk = flag(args, "--sdk").unwrap_or_else(|| "../sdk".into());
            let d = wasm_scaffold::scaffold(&dir, id, &kind, &name, &lang, &sdk)?;
            Ok(format!("created {} ({lang})", d.display()))
        }
        (Some("addon"), Some("keygen"), Some(private_path)) => {
            signing_options(args, &["--key-id", "--public-out"], &[])?;
            let id = flag(args, "--key-id").unwrap();
            if !addon_signing::key_id_ok(&id) {
                return Err("invalid signing key id".into());
            }
            let public_path = PathBuf::from(flag(args, "--public-out").unwrap());
            let (private, public_key) =
                addon_signing::generate_keypair().map_err(|e| e.to_string())?;
            let keys = TrustedKeys::from([(
                id.clone(),
                TrustedKey {
                    public_key,
                    revoked: false,
                },
            )]);
            let public = serde_json::to_vec_pretty(&keys).map_err(|e| e.to_string())?;
            write_new(Path::new(private_path), &private)?;
            if let Err(e) = write_new(&public_path, &public) {
                let _ = std::fs::remove_file(private_path);
                return Err(e);
            }
            Ok(format!(
                "created signing key {id}; private file {private_path}; operator trust file {}",
                public_path.display()
            ))
        }
        (Some("addon"), Some("sign"), Some(dir)) => {
            signing_options(args, &["--key", "--key-id"], &[])?;
            let key_path = PathBuf::from(flag(args, "--key").unwrap());
            let dir = Path::new(dir);
            let canonical_dir = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
            let canonical_key = std::fs::canonicalize(&key_path).map_err(|e| e.to_string())?;
            if canonical_key.starts_with(&canonical_dir) {
                return Err("private key must be stored outside the addon directory".into());
            }
            let private = read_bounded(&key_path, 16 * 1024)?;
            let bytes = addon_pkg::pack(dir).map_err(|e| e.to_string())?;
            let pkg = addon_pkg::inspect(&bytes, Limits::default()).map_err(|e| e.to_string())?;
            let errs = lint_manifest(&pkg.manifest);
            if !errs.is_empty() {
                return Err(errs.join("\n"));
            }
            let id = flag(args, "--key-id").unwrap();
            let signature = addon_signing::sign(&pkg, &id, &private).map_err(|e| e.to_string())?;
            let raw = serde_json::to_vec_pretty(&signature).map_err(|e| e.to_string())?;
            write_new(&dir.join(SIGNATURE_FILE), &raw)?;
            Ok(format!("signed {} with key {id}", pkg.id))
        }
        (Some("addon"), Some("verify"), Some(path)) => {
            signing_options(args, &["--trusted-keys"], &["--require-signed"])?;
            let trust_path = PathBuf::from(flag(args, "--trusted-keys").unwrap());
            let keys: TrustedKeys =
                serde_json::from_slice(&read_bounded(&trust_path, 1024 * 1024)?)
                    .map_err(|e| e.to_string())?;
            let path = Path::new(path);
            let bytes = if path.is_dir() {
                addon_pkg::pack(path).map_err(|e| e.to_string())?
            } else {
                read_bounded(path, Limits::default().max_zip_bytes)?
            };
            let pkg = addon_pkg::inspect(&bytes, Limits::default()).map_err(|e| e.to_string())?;
            let signer =
                addon_signing::verify(&pkg, &keys, args.iter().any(|a| a == "--require-signed"))
                    .map_err(|e| e.to_string())?;
            Ok(match signer {
                Some(id) => format!("verified {}: trusted signing key {id}", pkg.id),
                None => format!(
                    "unsigned {}: allowed by permissive policy (not authenticated)",
                    pkg.id
                ),
            })
        }
        (Some("addon"), Some("hash"), Some(dir)) => {
            let m = addon_pkg::rehash(Path::new(dir)).map_err(|e| e.to_string())?;
            Ok(format!(
                "hashed {} files",
                m.get("files")
                    .and_then(Value::as_object)
                    .map_or(0, |f| f.len())
            ))
        }
        (Some("addon"), Some("validate"), Some(p)) => validate(Path::new(p)),
        (Some("addon"), Some("pack"), Some(dir)) => {
            let bytes = addon_pkg::pack(Path::new(dir)).map_err(|e| e.to_string())?;
            let pkg = addon_pkg::inspect(&bytes, Limits::default()).map_err(|e| e.to_string())?;
            let errs = lint_manifest(&pkg.manifest);
            if !errs.is_empty() {
                return Err(errs.join("\n"));
            }
            let version = pkg
                .manifest
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("0");
            let out = flag(args, "-o")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(format!("{}-{version}.zip", pkg.id)));
            std::fs::write(&out, &bytes).map_err(|e| e.to_string())?;
            Ok(format!(
                "wrote {} ({} bytes, sha256 {})",
                out.display(),
                bytes.len(),
                addon_pkg::sha256_hex(&bytes)
            ))
        }
        _ => Err(String::new()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) if e.is_empty() => usage(),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_validate_pack() {
        let tmp = std::env::temp_dir().join(format!("pelty-cli-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let t = tmp.to_string_lossy().to_string();
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        run(&a(&["addon", "scaffold", "cli-demo", "--dir", &t])).unwrap();
        let dir = tmp.join("cli-demo");
        assert!(validate(&dir).unwrap().starts_with("ok: cli-demo"));
        std::fs::write(dir.join("extra.txt"), "x").unwrap();
        assert!(validate(&dir).is_ok(), "pack only includes declared files");
        std::fs::write(dir.join("README.md"), "tampered").unwrap();
        assert!(validate(&dir).is_err());
        run(&a(&["addon", "hash", &dir.to_string_lossy()])).unwrap();
        let out = tmp.join("x.zip").to_string_lossy().to_string();
        run(&a(&["addon", "pack", &dir.to_string_lossy(), "-o", &out])).unwrap();
        assert!(validate(Path::new(&out)).is_ok());
        assert!(run(&a(&["addon", "scaffold", "Bad_ID", "--dir", &t])).is_err());
        assert!(run(&a(&[
            "addon", "scaffold", "okid", "--kind", "nope", "--dir", &t
        ]))
        .is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn signing_cli_roundtrip_and_no_clobber() {
        let tmp = std::env::temp_dir().join(format!("pelty-cli-sign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let private = tmp.join("release.pk8").to_string_lossy().to_string();
        let public = tmp.join("trusted.json").to_string_lossy().to_string();
        let keygen = a(&[
            "addon",
            "keygen",
            &private,
            "--key-id",
            "release",
            "--public-out",
            &public,
        ]);
        run(&keygen).unwrap();
        let original = std::fs::read(&private).unwrap();
        assert!(run(&keygen).is_err());
        assert_eq!(std::fs::read(&private).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&private).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let dir = addon_pkg::scaffold(&tmp, "signed-cli", "plugin", "demo", "Signed").unwrap();
        let dir_s = dir.to_string_lossy().to_string();
        let verify = a(&[
            "addon",
            "verify",
            &dir_s,
            "--trusted-keys",
            &public,
            "--require-signed",
        ]);
        assert!(run(&verify).is_err());
        let sign = a(&[
            "addon", "sign", &dir_s, "--key", &private, "--key-id", "release",
        ]);
        run(&sign).unwrap();
        assert!(run(&sign).is_err(), "must not overwrite existing signature");
        assert!(run(&verify)
            .unwrap()
            .contains("trusted signing key release"));
        std::fs::write(dir.join("README.md"), "tampered").unwrap();
        addon_pkg::rehash(&dir).unwrap();
        assert!(run(&verify).is_err());
        assert!(run(&a(&[
            "addon",
            "verify",
            &dir_s,
            "--trusted-keys",
            &public,
            "--require-sign"
        ]))
        .is_err());
        // A pre-existing public output must also remain untouched.
        let other_private = tmp.join("other.pk8").to_string_lossy().to_string();
        let old_public = std::fs::read(&public).unwrap();
        assert!(run(&a(&[
            "addon",
            "keygen",
            &other_private,
            "--key-id",
            "other",
            "--public-out",
            &public
        ]))
        .is_err());
        assert!(!Path::new(&other_private).exists());
        assert_eq!(std::fs::read(&public).unwrap(), old_public);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn lint_catches_bad_fields() {
        let m = serde_json::json!({"id": "ab", "kind": "x", "permissions": ["root"], "settings": [{"key": "s", "kind": "select"}]});
        assert!(lint_manifest(&m).len() >= 5);
    }
}
