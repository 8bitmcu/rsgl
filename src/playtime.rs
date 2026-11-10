use crate::{config, lock};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(unix)]
use std::thread;

/// How often the tracker checks whether the game is still running.
const POLL: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Default, Clone, Copy)]
pub struct Entry {
    /// Total time played.
    pub seconds: u64,
    /// Unix time of the last save while the game was running.
    pub last_played: u64,
}

/// Playtime per game name. A missing or broken file is treated as empty.
pub type Playtime = BTreeMap<String, Entry>;

pub fn load() -> Playtime {
    fs::read_to_string(config::playtime_file())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Adds `played` to the game's total. Locked, so several trackers don't overwrite each other.
fn add(name: &str, played: Duration) -> Result<(), String> {
    let path = config::playtime_file();
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _guard = lock(&dir.join("playtime.lock"), true);

    let mut all = load();
    let e = all.entry(name.to_string()).or_default();
    e.seconds += played.as_secs();
    e.last_played = now();

    let text = toml::to_string(&all).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("part");
    fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// True while any process in the group is alive. Reaps our own exited children
/// first, since a zombie still counts as a member of the group.
#[cfg(unix)]
fn group_alive(pgid: i32) -> bool {
    let alive = unsafe {
        while libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) > 0 {}
        libc::kill(-pgid, 0) == 0
    };
    alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Waits up to `wait`, then reports whether the game has finished.
#[cfg(unix)]
fn wait_finished(pgid: u32, wait: Duration) -> bool {
    thread::sleep(wait);
    !group_alive(pgid as i32)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        pub fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
    }
}

/// Waits up to `wait` for the process to exit and reports whether it has. Windows
/// has no process groups, so this follows the one process we started (the `cmd`
/// wrapper), which itself waits for the game.
#[cfg(windows)]
fn wait_finished(pid: u32, wait: Duration) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    unsafe {
        let handle = win::OpenProcess(SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return true; // already gone
        }
        let result = win::WaitForSingleObject(handle, wait.as_millis() as u32);
        win::CloseHandle(handle);
        result != WAIT_TIMEOUT
    }
}

/// Waits until the game has finished, without recording its playtime.
pub fn wait(pgid: u32) {
    while !wait_finished(pgid, POLL) {}
}

/// Waits until the game has finished (on Unix, until its process group is empty),
/// saving the time played every `checkpoint` (if non-zero) and once more at the end.
pub fn track(pgid: u32, name: &str, checkpoint: Duration) {
    // Instant doesn't advance while the machine is suspended, so sleep isn't counted
    let mut last = Instant::now();
    loop {
        let alive = !wait_finished(pgid, POLL);
        let now = Instant::now();
        if !alive || (!checkpoint.is_zero() && now - last >= checkpoint) {
            // only move `last` forward by whole seconds, so no time is lost to rounding
            let played = Duration::from_secs((now - last).as_secs());
            if let Err(e) = add(name, played) {
                eprintln!("rsgl: saving playtime: {e}");
            }
            last += played;
        }
        if !alive {
            break;
        }
    }
}

/// Total time played: "42h 10m".
pub fn played(e: &Entry) -> String {
    let m = e.seconds / 60;
    if m >= 60 {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// When the game was last played: "3 days ago".
pub fn last_played(e: &Entry) -> String {
    let ago = now().saturating_sub(e.last_played);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match ago {
        0..60 => "just now".to_string(),
        60..3600 => plural(ago / 60, "minute"),
        3600..86400 => plural(ago / 3600, "hour"),
        _ => plural(ago / 86400, "day"),
    }
}
