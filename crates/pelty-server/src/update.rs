//! `pelty-server update [--check] [--version X] [--yes] [--no-backup]` and
//! the console `update`: install a release over the running binary.
//!
//! 1. Read the release feed (same URL, token and parser as the admin's
//!    "update available" check: see `pelty_api::updates`).
//! 2. Pick the archive for this build's target, download `SHA256SUMS` and,
//!    when a release public key is compiled in (`release-key.pub`), require
//!    a valid ed25519 `SHA256SUMS.sig`; the archive must match its checksum
//!    (and the manifest's, when it lists one).
//! 3. Take a database backup (`pg_dump`, listed in Admin > Backups) unless
//!    `--no-backup`.
//! 4. Replace the binary atomically (`self-replace`, which also handles a
//!    running .exe on Windows). Migrations run on the next start.
use pelty_api::updates::{self, Release};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Target triple this binary was built for (set by build.rs).
pub const TARGET: &str = env!("PELTY_TARGET");
const PUBKEY: &str = include_str!("../release-key.pub");

#[derive(Debug, Default, Clone)]
pub struct Opts {
    pub check: bool,
    pub version: Option<String>,
    pub yes: bool,
    pub no_backup: bool,
    /// Binary to replace (tests); None = the running executable.
    pub exe: Option<PathBuf>,
}

pub fn parse_args(args: &[&str]) -> Result<Opts, String> {
    let usage = "usage: update [--check] [--version <x.y.z>] [--yes] [--no-backup]";
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--check" => o.check = true,
            "--yes" | "-y" => o.yes = true,
            "--no-backup" => o.no_backup = true,
            "--version" => o.version = Some(it.next().ok_or(usage)?.trim_start_matches('v').to_string()),
            _ => return Err(usage.into()),
        }
    }
    if o.version.as_deref().is_some_and(|v| updates::parse_version(v).is_none()) {
        return Err(usage.into());
    }
    Ok(o)
}

/// The compiled-in release public key (32 bytes), if one is configured.
pub fn public_key() -> Option<Vec<u8>> {
    use base64::Engine;
    let line = PUBKEY.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    base64::engine::general_purpose::STANDARD.decode(line).ok().filter(|k| k.len() == 32)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(c: &reqwest::Client, url: &str, binary: bool) -> Result<Vec<u8>, String> {
    let mut rq = c.get(url);
    for (k, v) in updates::feed_headers(binary) {
        rq = rq.header(k, v);
    }
    let r = rq.send().await.map_err(|e| format!("{url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("{url}: HTTP {}", r.status().as_u16()));
    }
    Ok(r.bytes().await.map_err(|e| format!("{url}: {e}"))?.to_vec())
}

/// Fetch the release (latest, or `version`) from `feed`.
pub async fn fetch(feed: &str, version: Option<&str>) -> Result<Release, String> {
    let url = match version {
        Some(v) => updates::feed_for_version(feed, v).ok_or("this feed URL has no per-version form (use .../releases/latest or .../latest/manifest.json)")?,
        None => feed.to_string(),
    };
    let body = get(&client()?, &url, false).await?;
    updates::parse_feed(&String::from_utf8_lossy(&body), &url).map_err(|e| format!("{url}: unreadable release feed ({e})"))
}

fn sha256_hex(b: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, b).as_ref().iter().map(|x| format!("{x:02x}")).collect()
}

/// Checksum for `name` from a `SHA256SUMS` file (`<hex>  <name>` lines).
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (h, n) = l.split_once(char::is_whitespace)?;
        (n.trim().trim_start_matches('*') == name).then(|| h.trim().to_ascii_lowercase())
    })
}

/// Verify `SHA256SUMS` against its base64 ed25519 signature.
pub fn verify_sig(key: &[u8], sums: &[u8], sig_b64: &str) -> bool {
    use base64::Engine;
    let Ok(sig) = base64::engine::general_purpose::STANDARD.decode(sig_b64.trim()) else { return false };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key).verify(sums, &sig).is_ok()
}

/// The `pelty-server` binary inside a release archive.
pub fn extract_binary(name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let want = |p: &str| p.rsplit('/').next().is_some_and(|f| f == "pelty-server" || f == "pelty-server.exe");
    if name.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| e.to_string())?;
            if f.name().is_ok_and(|n| want(&n)) {
                let mut out = vec![];
                f.read_to_end(&mut out).map_err(|e| e.to_string())?;
                return Ok(out);
            }
        }
    } else {
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        for e in t.entries().map_err(|e| e.to_string())? {
            let mut e = e.map_err(|e| e.to_string())?;
            let p = e.path().map_err(|e| e.to_string())?.to_string_lossy().to_string();
            if want(&p) {
                let mut out = vec![];
                e.read_to_end(&mut out).map_err(|e| e.to_string())?;
                return Ok(out);
            }
        }
    }
    Err(format!("{name} has no pelty-server binary"))
}

/// Download and verify the archive for this target; returns the new binary.
pub async fn download_verified(r: &Release, target: &str, key: Option<&[u8]>, say: &mut (dyn FnMut(String) + Send)) -> Result<Vec<u8>, String> {
    let c = client()?;
    let a = r.archive_for(target).ok_or_else(|| format!("release v{} has no build for {target}", r.version))?;
    let sums_a = r.asset("SHA256SUMS").ok_or("release has no SHA256SUMS")?;
    let sums = get(&c, &sums_a.url, true).await?;
    match key {
        Some(k) => {
            let sig_a = r.asset("SHA256SUMS.sig").ok_or("release is not signed (no SHA256SUMS.sig) but this build requires signed releases")?;
            let sig = get(&c, &sig_a.url, true).await?;
            if !verify_sig(k, &sums, &String::from_utf8_lossy(&sig)) {
                return Err("SHA256SUMS signature is INVALID; not installing".into());
            }
            say("signature ok (SHA256SUMS.sig)".into());
        }
        None => say("warning: no release public key compiled in; checking SHA256 only".into()),
    }
    let sums = String::from_utf8_lossy(&sums).to_string();
    let want = sum_for(&sums, &a.name).ok_or_else(|| format!("{} is not listed in SHA256SUMS", a.name))?;
    if a.sha256.as_deref().is_some_and(|m| !m.eq_ignore_ascii_case(&want)) {
        return Err("manifest and SHA256SUMS disagree; not installing".into());
    }
    say(format!("downloading {}", a.name));
    let bytes = get(&c, &a.url, true).await?;
    if sha256_hex(&bytes) != want {
        return Err(format!("{}: SHA256 mismatch; not installing", a.name));
    }
    say("checksum ok".into());
    extract_binary(&a.name, &bytes)
}

/// Write `bin` next to `exe` and swap it in atomically.
pub fn replace(exe: Option<&Path>, bin: &[u8]) -> Result<(), String> {
    let cur = match exe {
        Some(p) => p.to_path_buf(),
        None => std::env::current_exe().map_err(|e| e.to_string())?,
    };
    let dir = cur.parent().ok_or("binary has no folder")?;
    let tmp = dir.join(format!(".pelty-server-update-{}", std::process::id()));
    std::fs::write(&tmp, bin).map_err(|e| format!("{}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let res = if exe.is_some() { std::fs::rename(&tmp, &cur).map_err(|e| e.to_string()) } else { self_replace::self_replace(&tmp).map_err(|e| e.to_string()) };
    let _ = std::fs::remove_file(&tmp);
    res.map_err(|e| format!("could not replace {}: {e}", cur.display()))
}

/// What to tell the user once the new binary is in place.
pub fn restart_hint() -> &'static str {
    if std::env::var_os("INVOCATION_ID").is_some() {
        "Restart the service to run it (e.g. `systemctl restart peltycms`); migrations run on start."
    } else {
        "Restart pelty-server (`stop` in the console, then start it again) to run it; migrations run on start."
    }
}

/// The whole flow. `pool` + `database_url` are for the backup and the saved
/// feed URL; `say` prints progress.
pub async fn run(pool: Option<&pelty_database::Pool>, database_url: &str, o: &Opts, say: &mut (dyn FnMut(String) + Send)) -> Result<(), String> {
    if crate::in_container() && !o.check {
        return Err("running in a container: pull the new image instead (docker compose pull && docker compose up -d)".into());
    }
    let feed = match pool {
        Some(p) => updates::config_in(p).await.map(|c| c.1).unwrap_or_else(|_| updates::env_url().unwrap_or_else(|| updates::DEFAULT_URL.into())),
        None => updates::env_url().unwrap_or_else(|| updates::DEFAULT_URL.into()),
    };
    let r = fetch(&feed, o.version.as_deref()).await?;
    let newer = updates::is_newer(&r.version, updates::CURRENT);
    say(format!("installed {} ({TARGET}); {} v{}{}", updates::CURRENT, if o.version.is_some() { "requested" } else { "latest" }, r.version, if newer { " is newer" } else { "" }));
    if o.check {
        if newer && r.archive_for(TARGET).is_none() {
            say(format!("note: v{} has no build for {TARGET}", r.version));
        }
        return Ok(());
    }
    if !newer && o.version.is_none() {
        say("already up to date".into());
        return Ok(());
    }
    if !o.yes {
        use std::io::Write;
        print!("Install v{} over {}? [y/N] ", r.version, updates::CURRENT);
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        if !matches!(line.trim(), "y" | "Y" | "yes") {
            return Err("cancelled".into());
        }
    }
    let bin = download_verified(&r, TARGET, public_key().as_deref(), say).await?;
    if o.no_backup {
        say("skipping the database backup (--no-backup)".into());
    } else {
        let pool = pool.ok_or("no database connection for the pre-update backup (fix DATABASE_URL or pass --no-backup)")?;
        let p = pelty_api::backup::backup_now(pool, database_url).await.map_err(|e| format!("database backup failed ({e}); not updating (set PELTY_PG_DUMP, or pass --no-backup)"))?;
        say(format!("database backup: {}", p.display()));
    }
    replace(o.exe.as_deref(), &bin)?;
    say(format!("installed v{}. {}", r.version, restart_hint()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get as route;

    fn targz(bin: &[u8]) -> Vec<u8> {
        let mut t = tar::Builder::new(flate2::write::GzEncoder::new(vec![], flate2::Compression::fast()));
        let mut h = tar::Header::new_gnu();
        h.set_size(bin.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        t.append_data(&mut h, "pelty-server-9.9.9-x/pelty-server", bin).unwrap();
        t.into_inner().unwrap().finish().unwrap()
    }

    /// Local fake release server (manifest feed) on 127.0.0.1.
    async fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
        let mut app = axum::Router::new();
        for (name, body) in files {
            app = app.route(&format!("/rel/latest/{name}"), route(move || { let b = body.clone(); async move { b } }));
        }
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}/rel/latest/manifest.json")
    }

    fn signed(sums: &str) -> (Vec<u8>, String) {
        use base64::Engine;
        use ring::signature::KeyPair;
        let rng = ring::rand::SystemRandom::new();
        let der = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = ring::signature::Ed25519KeyPair::from_pkcs8(der.as_ref()).unwrap();
        (kp.public_key().as_ref().to_vec(), base64::engine::general_purpose::STANDARD.encode(kp.sign(sums.as_bytes()).as_ref()))
    }

    #[test]
    fn args() {
        let o = parse_args(&["--version", "v1.2.3", "--yes", "--no-backup"]).unwrap();
        assert_eq!(o.version.as_deref(), Some("1.2.3"));
        assert!(o.yes && o.no_backup && !o.check);
        assert!(parse_args(&["--version", "nope"]).is_err());
        assert!(parse_args(&["--bogus"]).is_err());
    }

    #[tokio::test]
    async fn downloads_verifies_and_replaces_from_a_fake_release_server() {
        let bin = b"#!new pelty-server".to_vec();
        let archive = targz(&bin);
        let name = format!("pelty-server-9.9.9-{TARGET}.tar.gz");
        let sums = format!("{}  {name}\n", sha256_hex(&archive));
        let (key, sig) = signed(&sums);
        let manifest = serde_json::json!({ "version": "9.9.9", "assets": [
            { "target": TARGET, "name": name, "sha256": sha256_hex(&archive) },
            { "name": "SHA256SUMS" }, { "name": "SHA256SUMS.sig" } ] });
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let feed = serve(vec![
            ("manifest.json", manifest.to_string().into_bytes()),
            (leak(name.clone()), archive.clone()),
            ("SHA256SUMS", sums.clone().into_bytes()),
            ("SHA256SUMS.sig", sig.into_bytes()),
        ]).await;
        let r = fetch(&feed, None).await.unwrap();
        assert_eq!(r.version, "9.9.9");
        let mut log = vec![];
        let got = download_verified(&r, TARGET, Some(&key), &mut |m| log.push(m)).await.unwrap();
        assert_eq!(got, bin);
        assert!(log.iter().any(|l| l.contains("signature ok")), "{log:?}");

        // A different key (or a tampered SHA256SUMS) is refused.
        let (other, _) = signed("x");
        let e = download_verified(&r, TARGET, Some(&other), &mut |_| {}).await.unwrap_err();
        assert!(e.contains("INVALID"), "{e}");
        // No build for another target.
        assert!(download_verified(&r, "riscv64-none", None, &mut |_| {}).await.unwrap_err().contains("no build"));

        // Atomic replace of a given binary path.
        let d = std::env::temp_dir().join(format!("pelty-upd-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join("pelty-server");
        std::fs::write(&exe, b"old").unwrap();
        replace(Some(&exe), &got).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), bin);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn checksum_mismatch_is_refused() {
        let archive = targz(b"x");
        let name = format!("pelty-server-9.9.9-{TARGET}.tar.gz");
        let manifest = serde_json::json!({ "version": "9.9.9", "assets": [{ "target": TARGET, "name": name }, { "name": "SHA256SUMS" }] });
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let feed = serve(vec![
            ("manifest.json", manifest.to_string().into_bytes()),
            (leak(name.clone()), archive),
            ("SHA256SUMS", format!("{}  {name}\n", "0".repeat(64)).into_bytes()),
        ]).await;
        let r = fetch(&feed, None).await.unwrap();
        let e = download_verified(&r, TARGET, None, &mut |_| {}).await.unwrap_err();
        assert!(e.contains("SHA256 mismatch"), "{e}");
    }

    #[test]
    fn sums_parsing() {
        assert_eq!(sum_for("AB12  a.tar.gz\ncd34 *b.zip\n", "b.zip").as_deref(), Some("cd34"));
        assert_eq!(sum_for("ab  a.tar.gz\n", "a.zip"), None);
    }
}
