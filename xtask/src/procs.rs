//! Child process supervision without bash: each child runs in its own
//! process group (Unix) so its whole tree can be stopped, Ctrl+C is caught
//! and forwarded, and the PIDs go to `target/xtask-dev.pids` so a later
//! `cargo xtask stop` (or the next `cargo xtask dev`) can clean up leftovers.
use crate::util::{describe, R};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C / SIGTERM set a flag the supervisor loop polls.
pub fn catch_interrupts() {
    let _ = ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::SeqCst));
}

pub fn pid_file(root: &Path) -> PathBuf {
    root.join("target").join("xtask-dev.pids")
}

/// Starts `c` detached from the terminal's stdin, in its own process group on Unix.
pub fn spawn(c: &mut Command) -> R<Child> {
    c.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    c.spawn().map_err(|e| format!("cannot start `{}`: {e}", describe(c)))
}

/// Runs the named children until one exits or Ctrl+C, then stops them all.
pub fn supervise(root: &Path, mut children: Vec<(&'static str, Child)>) -> R {
    record(root, &children);
    let mut result = Ok(());
    'outer: loop {
        if INTERRUPTED.load(Ordering::SeqCst) {
            println!("\nstopping ...");
            break;
        }
        for (name, child) in children.iter_mut() {
            if let Ok(Some(st)) = child.try_wait() {
                // On Windows Ctrl+C also reaches the children, which may exit first.
                if INTERRUPTED.load(Ordering::SeqCst) {
                    println!("\nstopping ...");
                } else if !st.success() {
                    result = Err(format!("{name} exited ({st})"));
                } else {
                    println!("{name} exited");
                }
                break 'outer;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    stop_children(children.iter_mut().map(|(_, c)| c));
    let _ = std::fs::remove_file(pid_file(root));
    result
}

fn record(root: &Path, children: &[(&'static str, Child)]) {
    let mut s = format!("xtask {}\n", std::process::id());
    for (name, c) in children {
        s.push_str(&format!("{name} {}\n", c.id()));
    }
    let f = pid_file(root);
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(f, s);
}

/// Stops everything listed in the pid file (from a previous run that is
/// still going or was killed without cleaning up). Returns the stopped PIDs.
pub fn stop_recorded(root: &Path) -> Vec<u32> {
    let f = pid_file(root);
    let Ok(text) = std::fs::read_to_string(&f) else { return vec![] };
    let mut stopped = vec![];
    for line in text.lines() {
        let Some((name, pid)) = line.split_once(' ') else { continue };
        let Ok(pid) = pid.trim().parse::<u32>() else { continue };
        if pid == std::process::id() || !looks_like_ours(pid) {
            continue;
        }
        if name == "xtask" {
            // The supervisor: ask it to stop so it cleans up its own children.
            terminate(pid);
        } else {
            kill_tree(pid);
        }
        stopped.push(pid);
    }
    let _ = std::fs::remove_file(f);
    stopped
}

/// Guards against PID reuse: only touch processes that look like dev tooling.
fn looks_like_ours(pid: u32) -> bool {
    let desc = process_desc(pid).to_ascii_lowercase();
    ["cargo", "node", "pnpm", "corepack", "pelty", "vite", "xtask", "rustc"].iter().any(|n| desc.contains(n))
}

fn process_desc(pid: u32) -> String {
    #[cfg(windows)]
    let out = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output();
    #[cfg(not(windows))]
    let out = Command::new("ps").args(["-o", "args=", "-p", &pid.to_string()]).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn terminate(pid: u32) {
    #[cfg(windows)]
    kill_tree(pid);
    #[cfg(not(windows))]
    {
        use nix::sys::signal::Signal;
        signal(pid as i32, Some(Signal::SIGTERM));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && signal(pid as i32, None) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Stops our own children and everything they started, all at once: SIGTERM
/// to every process group, then each is reaped as it exits (an unreaped
/// child is a zombie that still counts as a group member, which used to make
/// every stop wait the full 5s), SIGKILL for groups still there after 5s.
/// Windows: `taskkill /T /F`, then wait.
pub fn stop_children<'a>(children: impl IntoIterator<Item = &'a mut Child>) {
    let mut children: Vec<&mut Child> = children.into_iter().collect();
    #[cfg(windows)]
    for c in children.iter_mut() {
        kill_tree(c.id());
        let _ = c.wait();
    }
    #[cfg(not(windows))]
    {
        use nix::sys::signal::Signal;
        for c in children.iter() {
            let pid = c.id() as i32;
            if !signal(-pid, Some(Signal::SIGTERM)) {
                signal(pid, Some(Signal::SIGTERM));
            }
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut alive = 0;
            for c in children.iter_mut() {
                let _ = c.try_wait();
                if signal(-(c.id() as i32), None) {
                    alive += 1;
                }
            }
            if alive == 0 || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        for c in children.iter_mut() {
            signal(-(c.id() as i32), Some(Signal::SIGKILL));
            let _ = c.wait();
        }
    }
}

/// Stops a process that is not our child (from the pid file) and everything
/// it started: the process group on Unix (SIGTERM, then SIGKILL after 5s),
/// `taskkill /T /F` on Windows. For our own children use [`stop_children`].
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = quiet(Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]));
    }
    #[cfg(not(windows))]
    {
        use nix::sys::signal::Signal;
        let group = -(pid as i32);
        if !signal(group, Some(Signal::SIGTERM)) {
            // Not a group leader (e.g. recorded by an older run): the process alone.
            signal(pid as i32, Some(Signal::SIGTERM));
            return;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if !signal(group, None) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        signal(group, Some(Signal::SIGKILL));
    }
}

/// kill(2): a negative `pid` is a process group, `None` only checks it exists.
#[cfg(unix)]
fn signal(pid: i32, sig: Option<nix::sys::signal::Signal>) -> bool {
    pid != 0 && pid != -1 && nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), sig).is_ok()
}

#[cfg(windows)]
fn quiet(c: &mut Command) -> bool {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::{Duration, Instant};

    #[test]
    fn stop_children_stops_whole_groups_fast() {
        // Each `sh` starts a grandchild `sleep`; all are in the child's group.
        let mut a = super::spawn(std::process::Command::new("sh").args(["-c", "sleep 30 & sleep 30"])).unwrap();
        let mut b = super::spawn(std::process::Command::new("sh").args(["-c", "sleep 30 & sleep 30"])).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let groups = [-(a.id() as i32), -(b.id() as i32)];
        assert!(groups.iter().all(|g| super::signal(*g, None)));
        let t = Instant::now();
        super::stop_children([&mut a, &mut b]);
        assert!(t.elapsed() < Duration::from_secs(2), "stop took {:?}", t.elapsed());
        assert!(groups.iter().all(|g| !super::signal(*g, None)), "a group is still alive");
    }
}
