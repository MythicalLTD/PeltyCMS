//! Build, addons, test, lint, docker and clean (the rest of the old Makefile
//! and scripts/build-addons.sh).
use crate::util::{self, Ctx, R};
use std::path::{Path, PathBuf};

/// `ADDONS_TARGET` (cargo target dir for the wasm build), absolute because
/// cargo runs from extensions/addons. Default `target/addons`.
fn addons_target(ctx: &Ctx) -> PathBuf {
    match ctx.get("ADDONS_TARGET") {
        Some(t) if Path::new(&t).is_absolute() => PathBuf::from(t),
        Some(t) => ctx.root.join(t),
        None => ctx.root.join("target").join("addons"),
    }
}

/// First `name = "..."` in a Cargo.toml.
fn crate_name(manifest: &str) -> Option<String> {
    manifest.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "name").then(|| v.trim().trim_matches('"').to_string())
    })
}

/// A dependency line `pelty-addon-x = { path = "../x", default-features = false ...`:
/// the crate links another addon and is built alone so that addon's `export` feature stays off.
fn reuses_addon(manifest: &str) -> bool {
    manifest.lines().any(|l| {
        let Some((k, v)) = l.split_once('=') else {
            return false;
        };
        let k = k.trim();
        let v: String = v.chars().filter(|c| !c.is_whitespace()).collect();
        k.strip_prefix("pelty-addon-")
            .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
            && v.starts_with("{path=\"../")
            && v.contains("\",default-features=false")
    })
}

/// Builds the WASM addons (all, or the given directory names) for
/// wasm32-wasip2 and packs each into extensions/addons/dist/<id>.zip.
pub fn addons(ctx: &Ctx, only: &[String]) -> R {
    let addons = ctx.root.join("extensions").join("addons");
    let dist = addons.join("dist");
    let target = addons_target(ctx);
    std::fs::create_dir_all(&dist).map_err(|e| format!("mkdir {}: {e}", dist.display()))?;
    let installed = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    if !installed
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim() == "wasm32-wasip2")
        })
        .unwrap_or(false)
    {
        util::run(ctx.cmd("rustup").args(["target", "add", "wasm32-wasip2"]))?;
    }

    let mut dirs: Vec<String> = only.to_vec();
    if dirs.is_empty() {
        let mut all: Vec<String> = std::fs::read_dir(&addons)
            .map_err(|e| e.to_string())?
            .flatten()
            .filter(|e| {
                e.path().join("addon.json").is_file() && e.path().join("Cargo.toml").is_file()
            })
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|d| !d.starts_with("fixture-"))
            .collect();
        all.sort();
        dirs = all;
    }

    let mut batch = vec![];
    let mut reuse = vec![];
    let mut crates = vec![];
    for d in &dirs {
        let manifest = std::fs::read_to_string(addons.join(d).join("Cargo.toml"))
            .map_err(|e| format!("{d}/Cargo.toml: {e}"))?;
        let name = crate_name(&manifest).ok_or(format!("{d}/Cargo.toml: no package name"))?;
        if reuses_addon(&manifest) {
            reuse.push(name.clone())
        } else {
            batch.push(name.clone())
        }
        crates.push((d.clone(), name));
    }
    let wasm_build = |pkgs: &[String]| -> R {
        let mut c = ctx.cargo();
        c.current_dir(&addons)
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-wasip2",
                "--target-dir",
            ])
            .arg(&target);
        for p in pkgs {
            c.args(["-p", p]);
        }
        util::run(&mut c)
    };
    // Reusers first: building them rebuilds the reused crate without `export`,
    // so the main batch must come last to leave its own addon.wasm exported.
    for n in &reuse {
        wasm_build(std::slice::from_ref(n))?;
    }
    if !batch.is_empty() {
        wasm_build(&batch)?;
    }

    let cli = match ctx.get("PELTY_CLI") {
        Some(c) => PathBuf::from(c),
        None => {
            let host = target.join("host");
            util::run(
                ctx.cargo()
                    .args(["build", "-q", "-p", "pelty-cli", "--target-dir"])
                    .arg(&host),
            )?;
            host.join("debug")
                .join(format!("pelty-cli{}", std::env::consts::EXE_SUFFIX))
        }
    };

    let stage = std::env::temp_dir().join(format!("pelty-addons-{}", std::process::id()));
    let res = (|| -> R {
        for (d, name) in &crates {
            let src = addons.join(d);
            let json: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(src.join("addon.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("{d}/addon.json: {e}"))?;
            let id = json
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or(format!("{d}/addon.json: no id"))?;
            let wasm = target
                .join("wasm32-wasip2")
                .join("release")
                .join(format!("{}.wasm", name.replace('-', "_")));
            let dir = stage.join(id);
            util::remove_dir(&dir)?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::copy(src.join("addon.json"), dir.join("addon.json"))
                .map_err(|e| e.to_string())?;
            std::fs::copy(&wasm, dir.join("addon.wasm"))
                .map_err(|e| format!("{}: {e}", wasm.display()))?;
            for sub in ["locales", "assets"] {
                if src.join(sub).is_dir() {
                    util::copy_dir(&src.join(sub), &dir.join(sub))?;
                }
            }
            let zip = dist.join(format!("{id}.zip"));
            util::run(
                std::process::Command::new(&cli)
                    .args(["addon", "hash"])
                    .arg(&dir)
                    .stdout(std::process::Stdio::null()),
            )?;
            util::run(
                std::process::Command::new(&cli)
                    .args(["addon", "pack"])
                    .arg(&dir)
                    .arg("-o")
                    .arg(&zip)
                    .stdout(std::process::Stdio::null()),
            )?;
            let kb = std::fs::metadata(&zip)
                .map(|m| m.len().div_ceil(1024))
                .unwrap_or(0);
            println!("packed {id} ({kb}K)");
        }
        Ok(())
    })();
    let _ = util::remove_dir(&stage);
    res
}

/// Scaffolds a WASM addon: `cargo xtask addon <id> [--kind <kind>]`.
pub fn addon(ctx: &Ctx, args: &[String]) -> R {
    let usage = "usage: cargo xtask addon <id> --kind gateway|provisioning|registrar|captcha|sso|integration";
    let mut id = None;
    let mut kind = "integration".to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--kind" => kind = it.next().ok_or(usage)?.clone(),
            k if k.starts_with("--kind=") => kind = k["--kind=".len()..].to_string(),
            _ if id.is_none() => id = Some(a.clone()),
            _ => return Err(usage.into()),
        }
    }
    let id = id.ok_or(usage)?;
    util::run(ctx.cargo().args([
        "run",
        "-q",
        "-p",
        "pelty-cli",
        "--",
        "addon",
        "wasm-scaffold",
        &id,
        "--kind",
        &kind,
        "--dir",
        "extensions/addons",
        "--sdk",
        "../sdk",
    ]))?;
    println!("add \"{id}\" to members in extensions/addons/Cargo.toml, then: cargo xtask addons");
    Ok(())
}

fn frontend_pnpm(ctx: &Ctx, args: &[&str]) -> R {
    let mut c = ctx.pnpm()?;
    c.current_dir(ctx.root.join("frontend")).args(args);
    util::run(&mut c)
}

/// Local release-ish build: addons, docs, frontend, server binary.
/// Uses `release-fast` (thin LTO). Ship archives with `cargo xtask release`.
pub fn build(ctx: &Ctx) -> R {
    build_assets(ctx)?;
    println!("\n==> Building pelty-server (profile release-fast)");
    util::run(
        ctx.cargo()
            .args(["build", "--profile", "release-fast", "-p", "pelty-server"]),
    )?;
    println!(
        "\n==> Build complete: target/release-fast/pelty-server{}",
        std::env::consts::EXE_SUFFIX
    );
    Ok(())
}

/// Everything `pelty-server`'s `embed-frontend` feature compiles in: addon
/// zips, docs, and the web UI build (frontend/dist).
pub fn build_assets(ctx: &Ctx) -> R {
    println!("==> Building and packing addons");
    addons(ctx, &[])?;
    println!("\n==> Generating documentation");
    crate::docs(ctx)?;
    println!("\n==> Installing frontend dependencies");
    frontend_pnpm(ctx, &["install", "--frozen-lockfile"])?;
    println!("\n==> Building frontend");
    frontend_pnpm(ctx, &["run", "build"])
}

/// Rust workspace tests (extra args go to cargo test) + vitest.
pub fn test(ctx: &Ctx, args: &[String]) -> R {
    util::run(ctx.cargo().args(["test", "--workspace"]).args(args))?;
    frontend_pnpm(ctx, &["exec", "vitest", "run"])
}

pub fn lint(ctx: &Ctx) -> R {
    util::run(ctx.cargo().args(["clippy", "--workspace", "--all-targets"]))?;
    frontend_pnpm(ctx, &["exec", "tsc", "--noEmit"])
}

pub fn docker(ctx: &Ctx) -> R {
    util::run(
        ctx.cmd("docker")
            .args(["build", "-t", "peltycms:latest", "."]),
    )
}

/// Removes build outputs: target/, frontend/dist, addon zips, ADDONS_TARGET.
pub fn clean(ctx: &Ctx) -> R {
    util::remove_dir(&ctx.root.join("frontend").join("dist"))?;
    util::remove_dir(&ctx.root.join("extensions").join("addons").join("dist"))?;
    let at = addons_target(ctx);
    if !at.starts_with(ctx.root.join("target")) {
        util::remove_dir(&at)?;
    }
    if cfg!(windows) {
        // The running xtask.exe cannot be deleted on Windows: remove everything
        // else under target/ and leave the locked files.
        let mut left = 0;
        remove_all_but_locked(&ctx.root.join("target"), &mut left);
        if left > 0 {
            println!("cleaned target/ except {left} file(s) in use (the running xtask); run `cargo clean` to remove them");
        }
        Ok(())
    } else {
        util::run(ctx.cargo().arg("clean"))
    }
}

fn remove_all_but_locked(dir: &Path, left: &mut usize) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            remove_all_but_locked(&p, left);
            let _ = std::fs::remove_dir(&p);
        } else if std::fs::remove_file(&p).is_err() {
            *left += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_helpers() {
        let m = "[package]\nname = \"pelty-addon-x\"\n[dependencies]\npelty-addon-stripe = { path = \"../stripe\", default-features = false }\n";
        assert_eq!(super::crate_name(m).as_deref(), Some("pelty-addon-x"));
        assert!(super::reuses_addon(m));
        assert!(!super::reuses_addon(
            "pelty-addon-sdk = { path = \"../sdk\" }"
        ));
    }
}
