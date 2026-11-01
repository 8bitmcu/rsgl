use crate::{config, lock};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

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
fn group_alive(pgid: i32) -> bool {
    let alive = unsafe {
        while libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) > 0 {}
        libc::kill(-pgid, 0) == 0
    };
    alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Waits until the game's process group is empty, saving the time played every
/// `checkpoint` (if non-zero) and once more at the end.
pub fn track(pgid: i32, name: &str, checkpoint: Duration) {
    // Instant doesn't advance while the machine is suspended, so sleep isn't counted
    let mut last = Instant::now();
    loop {
        thread::sleep(POLL);
        let alive = group_alive(pgid);
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

/// "42h 10m · last played 3 days ago"
pub fn describe(e: &Entry) -> String {
    let m = e.seconds / 60;
    let total = if m >= 60 { format!("{}h {}m", m / 60, m % 60) } else { format!("{m}m") };
    let ago = now().saturating_sub(e.last_played);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    let when = match ago {
        0..60 => "just now".to_string(),
        60..3600 => plural(ago / 60, "minute"),
        3600..86400 => plural(ago / 3600, "hour"),
        _ => plural(ago / 86400, "day"),
    };
    format!("{total} · last played {when}")
}
