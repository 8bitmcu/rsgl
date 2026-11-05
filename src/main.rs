mod api;
mod blur;
mod config;
mod playtime;
mod ui;

use config::{ApiCfg, Game, PlaytimeCfg, UiCfg};
use std::{
    fs::{self, File},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
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

const USAGE: &str = "\
Usage: rsgl [OPTIONS]

Options:
  -c, --config <FILE>  Read this config file instead of the default locations
  -f, --fullscreen     Start fullscreen
  -s, --search <TEXT>  Start with TEXT in the search bar (t:tag filters by tag)
  -h, --help           Show this help
";

struct Cli {
    fullscreen: bool,
    config: Option<PathBuf>,
    search: String,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli { fullscreen: false, config: None, search: String::new() };
    while let Some(arg) = args.next() {
        // `--flag=value` is the same as `--flag value`
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "-f" | "--fullscreen" if inline.is_none() => cli.fullscreen = true,
            "-h" | "--help" if inline.is_none() => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-c" | "--config" | "-s" | "--search" => {
                let value = inline
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{flag} needs an argument"))?;
                if flag == "-c" || flag == "--config" {
                    cli.config = Some(PathBuf::from(config::expand(&value)));
                } else {
                    cli.search = value;
                }
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(cli)
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

    let cli = match parse_args(args.into_iter().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rsgl: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Some(path) = cli.config {
        if !path.is_file() {
            eprintln!("rsgl: {}: config file not found", path.display());
            std::process::exit(1);
        }
        config::set_config_path(path);
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
    let launched = ui::run(games, ui_cfg, api_cfg, pt_cfg, error, cli.fullscreen, cli.search)?;

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
