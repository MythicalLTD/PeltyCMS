//! `cargo xtask release <version>`: release archives without CI.
//!
//! Dry run by default: bumps the workspace version, builds the assets
//! (addon zips, docs, web UI; only the UI is embedded, never addons) and `pelty-server` for every target this
//! machine can build, packages `dist/<version>/` (archives with README / EULA
//! / LICENSE, `SHA256SUMS`, its ed25519 signature `SHA256SUMS.sig` when a
//! signing key is configured, `manifest.json` for the updater, and
//! `CHANGELOG.md` from git log since the last tag), then restores the
//! version files. `--publish` keeps the bump, commits it, tags `v<version>`,
//! pushes (never forced) and creates the GitHub Release with `gh`.
//!
//! Targets: `--local` builds only the host target (run it on a Mac or a
//! Windows machine and upload the archive with `gh release upload`);
//! `--targets a,b` picks explicitly; otherwise every known target whose
//! toolchain is present here (Linux gnu always on Linux; musl with
//! `cargo zigbuild` or musl-gcc; Windows with `cargo xwin` or MinGW; macOS
//! only on a Mac).
use crate::util::{self, Ctx, R};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Environment variable with the path of the release signing key (written
/// by `cargo xtask release-keygen`); default `~/.config/peltycms/release-signing.key`.
pub const KEY_ENV: &str = "PELTY_RELEASE_KEY";
/// Public key compiled into `pelty-server` (the updater verifies with it).
pub const PUBKEY_FILE: &str = "crates/pelty-server/release-key.pub";

const KNOWN: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

struct Opts {
    version: String,
    publish: bool,
    local: bool,
    targets: Option<Vec<String>>,
    skip_assets: bool,
    /// Only re-write SHA256SUMS / .sig / manifest / changelog from archives
    /// already in `dist/<version>/` (CI merge step after matrix builds).
    assemble: bool,
}

fn parse(args: &[String]) -> R<Opts> {
    let usage = "usage: cargo xtask release <version> [--local | --targets t1,t2] [--publish] [--skip-assets] [--assemble]";
    let mut o = Opts {
        version: String::new(),
        publish: false,
        local: false,
        targets: None,
        skip_assets: false,
        assemble: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--publish" => o.publish = true,
            "--local" => o.local = true,
            "--skip-assets" => o.skip_assets = true,
            "--assemble" => o.assemble = true,
            "--targets" => {
                o.targets = Some(
                    it.next()
                        .ok_or(usage)?
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect(),
                )
            }
            v if !v.starts_with('-') && o.version.is_empty() => {
                o.version = v.trim_start_matches('v').to_string()
            }
            _ => return Err(usage.into()),
        }
    }
    if !valid_version(&o.version) {
        return Err(format!(
            "{usage}\n(version must look like 1.2.3 or 1.2.3-rc.1)"
        ));
    }
    if o.assemble && (o.publish || o.local || o.targets.is_some()) {
        return Err("--assemble cannot be combined with --publish, --local, or --targets".into());
    }
    Ok(o)
}

/// `MAJOR.MINOR.PATCH` with an optional `-pre` part.
pub fn valid_version(v: &str) -> bool {
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let nums: Vec<&str> = core.split('.').collect();
    nums.len() == 3
        && nums
            .iter()
            .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        && pre.is_none_or(|p| {
            !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        })
}

/// Replace `version = "..."` in `[workspace.package]`.
pub fn bump_manifest(toml: &str, version: &str) -> R<String> {
    let mut in_pkg = false;
    let mut done = false;
    let mut out = String::with_capacity(toml.len());
    for line in toml.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            in_pkg = t == "[workspace.package]";
        }
        if in_pkg && !done && t.starts_with("version") && t.contains('=') {
            out.push_str(&format!("version = \"{version}\"\n"));
            done = true;
        } else {
            out.push_str(line);
        }
    }
    done.then_some(out)
        .ok_or_else(|| "no version in [workspace.package] of Cargo.toml".into())
}

fn host() -> R<String> {
    let out = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("rustc -vV: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        .ok_or_else(|| "rustc -vV printed no host".into())
}

fn installed_targets() -> Vec<String> {
    std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// How to build `target` here: the cargo subcommand (`build`, `zigbuild`,
/// `xwin build`), or why it cannot be built.
fn builder(target: &str, host: &str, installed: &[String]) -> Result<Vec<&'static str>, String> {
    if target == host {
        return Ok(vec!["build"]);
    }
    if !installed.iter().any(|t| t == target) {
        return Err(format!("rustup target add {target}"));
    }
    let has = |p: &str| util::which(p).is_some();
    if target.contains("apple") {
        return if host.contains("apple") {
            Ok(vec!["build"])
        } else {
            Err("needs a Mac (run `cargo xtask release <v> --local` there)".into())
        };
    }
    if target.ends_with("windows-msvc") {
        return if has("cargo-xwin") {
            Ok(vec!["xwin", "build"])
        } else {
            Err("cargo install cargo-xwin".into())
        };
    }
    if target.ends_with("windows-gnu") {
        return if has("x86_64-w64-mingw32-gcc") {
            Ok(vec!["build"])
        } else {
            Err("needs MinGW (x86_64-w64-mingw32-gcc)".into())
        };
    }
    if has("cargo-zigbuild") && has("zig") {
        return Ok(vec!["zigbuild"]);
    }
    if target.ends_with("linux-musl")
        && has("musl-gcc")
        && host.starts_with("x86_64")
        && target.starts_with("x86_64")
    {
        return Ok(vec!["build"]);
    }
    Err("needs cargo-zigbuild + zig (pip install ziglang; cargo install cargo-zigbuild)".into())
}

fn git(ctx: &Ctx, args: &[&str]) -> R<String> {
    let out = ctx
        .cmd("git")
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Markdown changelog: commit subjects since the last tag (all history when
/// there is none yet).
fn changelog(ctx: &Ctx, version: &str) -> R<String> {
    let last = git(ctx, &["describe", "--tags", "--abbrev=0"]).ok();
    let range = last
        .as_ref()
        .map(|t| format!("{t}..HEAD"))
        .unwrap_or_else(|| "HEAD".into());
    let log = git(
        ctx,
        &["log", &range, "--no-merges", "--pretty=format:- %s (%h)"],
    )?;
    let since = last.map(|t| format!(" since {t}")).unwrap_or_default();
    Ok(format!(
        "# PeltyCMS v{version}\n\nChanges{since}:\n\n{log}\n"
    ))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, bytes);
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn key_path() -> Option<PathBuf> {
    std::env::var_os(KEY_ENV)
        .map(PathBuf::from)
        .or_else(|| util::home_dir().map(|h| h.join(".config/peltycms/release-signing.key")))
}

/// Load the signing key (base64 PKCS#8 ed25519) if one is configured.
fn signing_key() -> R<Option<ring::signature::Ed25519KeyPair>> {
    use base64::Engine;
    let Some(p) = key_path() else { return Ok(None) };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Ok(None);
    };
    let der = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|e| format!("{}: {e}", p.display()))?;
    ring::signature::Ed25519KeyPair::from_pkcs8(&der)
        .map(Some)
        .map_err(|e| format!("{}: not an ed25519 key ({e})", p.display()))
}

/// `cargo xtask release-keygen [path]`: new ed25519 signing key (never
/// overwrites); prints the public key for `crates/pelty-server/release-key.pub`.
pub fn keygen(_ctx: &Ctx, args: &[String]) -> R {
    use ring::signature::KeyPair;
    let path = args
        .first()
        .map(PathBuf::from)
        .or_else(key_path)
        .ok_or("give a path for the key")?;
    if path.exists() {
        return Err(format!(
            "{} exists; not overwriting a signing key",
            path.display()
        ));
    }
    let rng = ring::rand::SystemRandom::new();
    let der = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).map_err(|e| e.to_string())?;
    let kp =
        ring::signature::Ed25519KeyPair::from_pkcs8(der.as_ref()).map_err(|e| e.to_string())?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{}", b64(der.as_ref())).map_err(|e| e.to_string())?;
    println!(
        "signing key written to {} (keep it secret and backed up; set {KEY_ENV} if you move it)",
        path.display()
    );
    println!(
        "public key (put this line in {PUBKEY_FILE} and commit it):\n{}",
        b64(kp.public_key().as_ref())
    );
    Ok(())
}

fn archive(
    dir: &Path,
    stem: &str,
    bin: &Path,
    bin_name: &str,
    extras: &[(PathBuf, String)],
    zip: bool,
) -> R<PathBuf> {
    let files: Vec<(PathBuf, String, bool)> =
        std::iter::once((bin.to_path_buf(), bin_name.to_string(), true))
            .chain(
                extras
                    .iter()
                    .filter(|(p, _)| p.is_file())
                    .map(|(p, n)| (p.clone(), n.clone(), false)),
            )
            .collect();
    if zip {
        let out = dir.join(format!("{stem}.zip"));
        let f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        let mut z = zip::ZipWriter::new(f);
        for (p, n, exe) in &files {
            let o = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(if *exe { 0o755 } else { 0o644 });
            z.start_file(format!("{stem}/{n}"), o)
                .map_err(|e| e.to_string())?;
            z.write_all(&std::fs::read(p).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
        Ok(out)
    } else {
        let out = dir.join(format!("{stem}.tar.gz"));
        let f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        let gz = flate2::write::GzEncoder::new(f, flate2::Compression::best());
        let mut t = tar::Builder::new(gz);
        for (p, n, exe) in &files {
            let data = std::fs::read(p).map_err(|e| e.to_string())?;
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(if *exe { 0o755 } else { 0o644 });
            h.set_mtime(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            );
            h.set_cksum();
            t.append_data(&mut h, format!("{stem}/{n}"), data.as_slice())
                .map_err(|e| e.to_string())?;
        }
        t.into_inner()
            .and_then(|gz| gz.finish())
            .map_err(|e| e.to_string())?;
        Ok(out)
    }
}

pub fn release(ctx: &Ctx, args: &[String]) -> R {
    let o = parse(args)?;
    let key = signing_key()?;
    if o.assemble {
        if key.is_none() {
            println!("[release] no signing key ({KEY_ENV}): SHA256SUMS will not be signed");
        }
        let dist = assemble(ctx, &o.version, key.as_ref())?;
        println!("[release] assembled {}", dist.display());
        return Ok(());
    }
    let host = host()?;
    let installed = installed_targets();
    let wanted: Vec<String> = match (&o.targets, o.local) {
        (Some(t), _) => t.clone(),
        (None, true) => vec![host.clone()],
        (None, false) => KNOWN.iter().map(|t| t.to_string()).collect(),
    };
    let mut plan = vec![];
    for t in &wanted {
        match builder(t, &host, &installed) {
            Ok(b) => plan.push((t.clone(), b)),
            Err(why) if o.targets.is_some() => return Err(format!("cannot build {t} here: {why}")),
            Err(why) => println!("[release] skipping {t}: {why}"),
        }
    }
    if plan.is_empty() {
        return Err("no target can be built on this machine".into());
    }
    if o.publish {
        if !git(ctx, &["status", "--porcelain"])?.is_empty() {
            return Err("--publish needs a clean working tree".into());
        }
        if key.is_none() {
            return Err(format!("--publish needs the signing key ({KEY_ENV}; create one with `cargo xtask release-keygen`)"));
        }
        if !util::succeeds(ctx.cmd("gh").args(["auth", "status"])) {
            return Err("--publish needs an authenticated GitHub CLI (`gh auth login`)".into());
        }
        if git(
            ctx,
            &[
                "rev-parse",
                "-q",
                "--verify",
                &format!("refs/tags/v{}", o.version),
            ],
        )
        .is_ok()
        {
            return Err(format!("tag v{} already exists", o.version));
        }
    } else if key.is_none() {
        println!(
            "[release] no signing key ({KEY_ENV}): SHA256SUMS will not be signed (dry run only)"
        );
    }

    // Version bump (restored after a dry run, also when the build fails).
    let manifest = ctx.root.join("Cargo.toml");
    // The addon workspace's lock pins the server crates' versions too.
    let locks: Vec<(PathBuf, Vec<u8>)> = ["Cargo.lock", "extensions/addons/Cargo.lock"]
        .iter()
        .filter_map(|l| {
            let p = ctx.root.join(l);
            std::fs::read(&p).ok().map(|b| (p, b))
        })
        .collect();
    let old_manifest = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest, bump_manifest(&old_manifest, &o.version)?)
        .map_err(|e| e.to_string())?;
    let restore = || {
        let _ = std::fs::write(&manifest, &old_manifest);
        for (p, b) in &locks {
            let _ = std::fs::write(p, b);
        }
    };
    let res = build_and_package(ctx, &o, &host, &plan, key.as_ref());
    if !o.publish || res.is_err() {
        restore();
        if o.publish {
            println!("[release] version files restored after the failure");
        }
    }
    let dist = res?;
    if !o.publish {
        println!(
            "[release] dry run done: {} (version files restored; nothing tagged or uploaded)",
            dist.display()
        );
        return Ok(());
    }
    publish(ctx, &o.version, &dist)
}

fn list_archives(dist: &Path, version: &str) -> R<Vec<(String, String)>> {
    let prefix = format!("pelty-server-{version}-");
    let mut out = vec![];
    for e in std::fs::read_dir(dist)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().to_string();
        let target = name
            .strip_prefix(&prefix)
            .and_then(|t| t.strip_suffix(".tar.gz").or_else(|| t.strip_suffix(".zip")));
        if let Some(t) = target {
            out.push((t.to_string(), name));
        }
    }
    out.sort();
    Ok(out)
}

fn write_sums_manifest(
    dist: &Path,
    version: &str,
    notes: &str,
    archives: &[(String, String)],
    key: Option<&ring::signature::Ed25519KeyPair>,
) -> R {
    let mut assets = vec![];
    for (target, name) in archives {
        let bytes = std::fs::read(dist.join(name)).map_err(|e| e.to_string())?;
        assets.push(serde_json::json!({
            "target": target,
            "name": name,
            "sha256": sha256_hex(&bytes),
            "size": bytes.len()
        }));
        println!("[release] {name}");
    }
    if assets.is_empty() {
        return Err(format!(
            "no pelty-server-{version}-* archives in {}",
            dist.display()
        ));
    }
    let sums: String = assets
        .iter()
        .map(|a| {
            format!(
                "{}  {}\n",
                a["sha256"].as_str().unwrap(),
                a["name"].as_str().unwrap()
            )
        })
        .collect();
    std::fs::write(dist.join("SHA256SUMS"), &sums).map_err(|e| e.to_string())?;
    if let Some(k) = key {
        std::fs::write(
            dist.join("SHA256SUMS.sig"),
            format!("{}\n", b64(k.sign(sums.as_bytes()).as_ref())),
        )
        .map_err(|e| e.to_string())?;
    } else {
        let _ = std::fs::remove_file(dist.join("SHA256SUMS.sig"));
    }
    let manifest = serde_json::json!({
        "version": version,
        "tag": format!("v{version}"),
        "notes": notes,
        "assets": assets
    });
    std::fs::write(
        dist.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Rebuild checksums / signature / manifest for archives already in `dist/<version>/`.
fn assemble(ctx: &Ctx, version: &str, key: Option<&ring::signature::Ed25519KeyPair>) -> R<PathBuf> {
    let dist = ctx.root.join("dist").join(version);
    if !dist.is_dir() {
        return Err(format!("{} is missing", dist.display()));
    }
    let archives = list_archives(&dist, version)?;
    let notes = changelog(ctx, version)?;
    std::fs::write(dist.join("CHANGELOG.md"), &notes).map_err(|e| e.to_string())?;
    write_sums_manifest(&dist, version, &notes, &archives, key)?;
    Ok(dist)
}

fn build_and_package(
    ctx: &Ctx,
    o: &Opts,
    host: &str,
    plan: &[(String, Vec<&'static str>)],
    key: Option<&ring::signature::Ed25519KeyPair>,
) -> R<PathBuf> {
    if !o.skip_assets {
        crate::tasks::build_assets(ctx)?;
    } else if !ctx.root.join("frontend/dist/index.html").is_file() {
        return Err("--skip-assets but frontend/dist has no build".into());
    }
    let dist = ctx.root.join("dist").join(&o.version);
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    // Archives already in dist/<version>/ for targets not built now (e.g. one
    // copied from a Mac `--local` run) are kept and signed along.
    let prefix = format!("pelty-server-{}-", o.version);
    let mut kept = vec![];
    for e in std::fs::read_dir(&dist)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().to_string();
        let target = name
            .strip_prefix(&prefix)
            .and_then(|t| t.strip_suffix(".tar.gz").or_else(|| t.strip_suffix(".zip")));
        match target {
            Some(t) if !plan.iter().any(|(p, _)| p == t) => kept.push((t.to_string(), name)),
            _ => util::remove_path(&e.path())?,
        }
    }
    kept.sort();
    let notes = changelog(ctx, &o.version)?;
    std::fs::write(dist.join("CHANGELOG.md"), &notes).map_err(|e| e.to_string())?;
    let notes_file = dist.join("CHANGELOG.md");
    let mut extras: Vec<(PathBuf, String)> = [
        (ctx.root.join("README.md"), "README.md"),
        (ctx.root.join("EULA.md"), "EULA.md"),
        (ctx.root.join("LICENSE"), "LICENSE"),
        (notes_file, "CHANGELOG.md"),
    ]
    .into_iter()
    .map(|(p, n)| (p, n.to_string()))
    .collect();
    // UI translations next to the binary (same set the binary can seed on first run).
    for name in [
        "de.json", "en.json", "es.json", "fr.json", "nl.json", "ro.json",
    ] {
        extras.push((
            ctx.root.join("locales").join(name),
            format!("locales/{name}"),
        ));
    }
    let mut archives = kept;
    for (_target, name) in &archives {
        println!("[release] keeping {name} (already in {})", dist.display());
    }
    for (target, sub) in plan {
        println!("[release] building pelty-server for {target}");
        let mut c = ctx.cargo();
        c.args(sub)
            .args(["--release", "-p", "pelty-server"])
            .args(RELEASE_FEATURES);
        // The host target builds into target/release (shares the dev cache).
        let out_dir = if target == host && sub == &["build"] {
            ctx.root.join("target/release")
        } else {
            c.args(["--target", target]);
            ctx.root.join("target").join(target).join("release")
        };
        util::run(&mut c)?;
        let windows = target.contains("windows");
        let bin_name = if windows {
            "pelty-server.exe"
        } else {
            "pelty-server"
        };
        let stem = format!("pelty-server-{}-{target}", o.version);
        let a = archive(
            &dist,
            &stem,
            &out_dir.join(bin_name),
            bin_name,
            &extras,
            windows,
        )?;
        let name = a.file_name().unwrap().to_string_lossy().to_string();
        archives.push((target.clone(), name));
    }
    archives.sort();
    write_sums_manifest(&dist, &o.version, &notes, &archives, key)?;
    Ok(dist)
}

fn publish(ctx: &Ctx, version: &str, dist: &Path) -> R {
    let tag = format!("v{version}");
    util::run(
        ctx.cmd("git")
            .args(["commit", "-am", &format!("release: {tag}")]),
    )?;
    util::run(
        ctx.cmd("git")
            .args(["tag", "-a", &tag, "-m", &format!("PeltyCMS {tag}")]),
    )?;
    util::run(ctx.cmd("git").arg("push"))?;
    util::run(ctx.cmd("git").args(["push", "origin", &tag]))?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dist)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n != "CHANGELOG.md"))
        .collect();
    files.sort();
    let mut c = ctx.cmd("gh");
    c.args([
        "release",
        "create",
        &tag,
        "--title",
        &format!("PeltyCMS {tag}"),
        "--notes-file",
    ])
    .arg(dist.join("CHANGELOG.md"))
    .args(&files);
    util::run(&mut c)?;
    println!("[release] published {tag}. Mac/Windows archives: run `cargo xtask release {version} --local` on that machine (with the same signing key), copy its archive into dist/{version}/ here, run `cargo xtask release {version} --skip-assets` to re-sign, and `gh release upload {tag} --clobber dist/{version}/*` (see README).");
    Ok(())
}

/// Release binaries embed the web UI and themes only: never `embed-addons`
/// (addons are installed per site from Admin > Addons).
pub const RELEASE_FEATURES: &[&str] = &["--no-default-features", "--features", "embed-frontend"];

#[cfg(test)]
mod tests {
    #[test]
    fn releases_never_embed_addons() {
        assert!(!super::RELEASE_FEATURES.join(" ").contains("embed-addons"));
        assert!(super::RELEASE_FEATURES.contains(&"--no-default-features"));
    }

    use super::*;

    #[test]
    fn versions_and_bump() {
        assert!(valid_version("1.2.3") && valid_version("0.2.0-rc.1"));
        assert!(!valid_version("1.2") && !valid_version("1.2.x") && !valid_version("1.2.3-"));
        let t = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace.dependencies]\nversion = \"keep\"\n";
        let b = bump_manifest(t, "0.2.0").unwrap();
        assert!(b.contains("[workspace.package]\nversion = \"0.2.0\"\n"));
        assert!(b.contains("version = \"keep\""));
    }

    #[test]
    fn tar_archive_has_the_binary_executable() {
        let d = std::env::temp_dir().join(format!("pelty-rel-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("bin"), b"ELF").unwrap();
        std::fs::write(d.join("README.md"), b"hi").unwrap();
        let a = archive(
            &d,
            "pelty-server-1.0.0-x",
            &d.join("bin"),
            "pelty-server",
            &[
                (d.join("README.md"), "README.md".into()),
                (d.join("nope"), "LICENSE".into()),
            ],
            false,
        )
        .unwrap();
        let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(
            std::fs::File::open(&a).unwrap(),
        ));
        let names: Vec<(String, u32)> = ar
            .entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.path().unwrap().display().to_string(),
                    e.header().mode().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            names,
            vec![
                ("pelty-server-1.0.0-x/pelty-server".to_string(), 0o755),
                ("pelty-server-1.0.0-x/README.md".to_string(), 0o644)
            ]
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
