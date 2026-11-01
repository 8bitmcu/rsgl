mod api;
mod blur;
mod config;
mod playtime;
mod ui;

use config::{ApiCfg, Game, PlaytimeCfg, UiCfg};
use std::{
    fs::{self, File},
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

/// Starts the game in its own process group and returns its pid (which is also the group id).
fn launch(g: &Game) -> Option<u32> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(format!("exec {}", g.exec));
    if let Some(dir) = &g.workdir {
        cmd.current_dir(dir);
    }
    cmd.envs(&g.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // detach so the game outlives the launcher
        .spawn()
        .ok()
        .map(|c| c.id())
}

/// Takes an exclusive lock on `path`, held until the returned file is dropped
/// (or closed by exec). `wait` blocks until the lock is free instead of giving up.
fn lock(path: &Path, wait: bool) -> Option<File> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let f = File::options().create(true).truncate(false).write(true).open(path).ok()?;
    let locked = if wait { f.lock().is_ok() } else { f.try_lock().is_ok() };
    locked.then_some(f)
}

/// What the window hands back when it closed because a game was launched.
pub struct Launched {
    pub name: String,
    pub pgid: u32,
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    // internal: `rsgl --track <pgid> <checkpoint secs> <name>`, see below
    if let [_, flag, pgid, secs, name] = &args[..] {
        if flag == "--track" {
            if let (Ok(pgid), Ok(secs)) = (pgid.parse(), secs.parse()) {
                playtime::track(pgid, name, Duration::from_secs(secs));
            }
            return Ok(());
        }
    }

    let (games, ui_cfg, api_cfg, pt_cfg, error) = match config::load() {
        Ok((g, u, a, p)) => (g, u, a, p, None),
        Err(e) => (vec![], UiCfg::default(), ApiCfg::default(), PlaytimeCfg::default(), Some(e)),
    };

    // kept open while the window is up; std opens files close-on-exec, so the
    // tracker below doesn't hold on to it
    let _instance = if ui_cfg.single_instance {
        match lock(&config::lock_file(), false) {
            Some(f) => Some(f),
            None => {
                eprintln!("rsgl: already running (single_instance = true)");
                return Ok(());
            }
        }
    } else {
        None
    };

    let track = pt_cfg.enabled;
    let checkpoint = pt_cfg.checkpoint_minutes * 60;
    let launched = ui::run(games, ui_cfg, api_cfg, pt_cfg, error)?;

    // The game keeps running in its own group. Replace this process with a fresh
    // copy that only waits for it, so the window's memory (GPU driver, textures,
    // decoded images) is given back instead of sitting around until the game exits.
    // exec keeps our pid, so the game stays our child and can still be reaped.
    if let Some(l) = launched.filter(|_| track) {
        let err = Command::new("/proc/self/exe")
            .arg0("rsgl")
            .args(["--track", &l.pgid.to_string(), &checkpoint.to_string(), &l.name])
            .exec();
        eprintln!("rsgl: can't start playtime tracking: {err}");
    }
    Ok(())
}
