//! Every event the code emits (and every top-level payload key) must be in
//! `pelty_extensions::events::REGISTRY`, which the generated docs render.
use pelty_extensions::events::{spec, EventKind, REGISTRY};
use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `(event, keys)` for each `<marker>"name", json!({ ... })` call.
fn emissions(src: &str, marker: &str, skip_arg: bool) -> Vec<(String, Vec<String>)> {
    let mut v = vec![];
    let mut rest = src;
    while let Some(i) = rest.find(marker) {
        rest = &rest[i + marker.len()..];
        let mut t = rest.trim_start();
        if skip_arg {
            let Some(c) = t.find(',') else { continue };
            t = t[c + 1..].trim_start();
        }
        let Some(t) = t.strip_prefix('"') else {
            continue;
        };
        let Some(end) = t.find('"') else { continue };
        let name = t[..end].to_string();
        let after = t[end + 1..]
            .trim_start()
            .trim_start_matches(',')
            .trim_start();
        let mut keys = vec![];
        if let Some(body) = after.strip_prefix("json!({") {
            let (mut depth, mut j) = (1i32, 0usize);
            let b = body.as_bytes();
            while depth > 0 && j < b.len() {
                match b[j] {
                    b'{' | b'[' | b'(' => depth += 1,
                    b'}' | b']' | b')' => depth -= 1,
                    b'"' if depth == 1 => {
                        let s = &body[j + 1..];
                        let e = s.find('"').unwrap();
                        let prev = body[..j].trim_end();
                        if (prev.is_empty() || prev.ends_with(','))
                            && s[e + 1..].trim_start().starts_with(':')
                        {
                            keys.push(s[..e].to_string());
                        }
                        j += e + 1;
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        v.push((name, keys));
    }
    v
}

#[test]
fn every_emitted_event_is_registered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = vec![];
    for c in std::fs::read_dir(&root).unwrap().flatten() {
        let src = c.path().join("src");
        if src.is_dir() {
            rs_files(&src, &mut files);
        }
    }
    let (mut seen, mut problems) = (std::collections::BTreeSet::new(), vec![]);
    for f in &files {
        let s = std::fs::read_to_string(f).unwrap();
        // Unit-test modules may emit fixtures.
        let s = s.split("#[cfg(test)]").next().unwrap();
        for (marker, kind, skip) in [
            ("DomainEvent::new(", EventKind::Domain, false),
            ("addon_ext::pre_event(", EventKind::Pre, true),
        ] {
            for (name, keys) in emissions(s, marker, skip) {
                let at = f.strip_prefix(&root).unwrap().display().to_string();
                match spec(&name) {
                    None => problems.push(format!("{at}: `{name}` is not in events::REGISTRY")),
                    Some(e) if e.kind != kind => {
                        problems.push(format!("{at}: `{name}` registered as {:?}", e.kind))
                    }
                    Some(e) => {
                        for k in keys {
                            if !e.payload.iter().any(|p| p.name == k) {
                                problems.push(format!(
                                    "{at}: `{name}` payload key `{k}` is not documented"
                                ));
                            }
                        }
                    }
                }
                seen.insert(name);
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // Registered events nobody emits are stale docs.
    let stale: Vec<_> = REGISTRY
        .iter()
        .filter(|e| !seen.contains(e.name))
        .map(|e| e.name)
        .collect();
    assert!(stale.is_empty(), "registered but never emitted: {stale:?}");
    assert!(seen.len() >= 30, "scanner found only {} events", seen.len());
}
