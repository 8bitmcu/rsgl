// Release builds on Windows are GUI programs, so no console window opens next to the launcher.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api_igdb;
mod api_steamgriddb;
mod blur;
mod config;
mod gamepad;
mod keys;
mod playtime;
mod status;
mod topbar;
mod ui;

use config::{Game, Settings, Toggles};
use std::{
    fs::{self, File},
    io::IsTerminal,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The shell command that runs `exec`, in its own process group so the game
/// outlives the launcher. No `exec` prefix: it would replace the shell with the
/// first command of a list like `a && b`, so `b` would never run. Playtime tracks
/// the whole group, so the shell staying around doesn't matter.
#[cfg(unix)]
fn shell_command(exec: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(exec).process_group(0);
    cmd
}

/// On Windows `exec` goes through `cmd /C`. `/S` makes cmd strip exactly one pair of
/// outer quotes, so quoted paths inside `exec` survive.
#[cfg(windows)]
fn shell_command(exec: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.raw_arg(format!("/S /C \"{exec}\""))
        .creation_flags(CREATE_NEW_PROCESS_GROUP);
    cmd
}

/// True when rsgl was started from a terminal (or with its output redirected from
/// one), as opposed to a desktop entry or a hotkey, which have no terminal at all.
fn from_terminal() -> bool {
    std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
}

/// Starts `exec` through the shell, detached from rsgl: in `workdir` and with `env`
/// changing our own environment (None unsets a variable, see `Env::with`). Used for
/// games and for right-click menu entries.
///
/// When rsgl was started from a terminal, the program writes to that terminal too,
/// even after the launcher window has closed. Otherwise its output is discarded.
/// Stdin is never passed on: the program is in the background and the terminal
/// would stop it the moment it tried to read.
///
/// The error says why it couldn't start. Only problems that show up while starting
/// are caught: a command the shell can't find fails after this has already returned.
fn spawn(exec: &str, workdir: Option<&Path>, env: &[(&str, Option<&str>)]) -> Result<Child, String> {
    let mut cmd = shell_command(exec);
    if let Some(dir) = workdir {
        // checked here so the message can name the folder; the OS error alone doesn't
        if !dir.is_dir() {
            return Err(format!("the workdir {} doesn't exist", dir.display()));
        }
        cmd.current_dir(dir);
    }
    for &(name, value) in env {
        match value {
            Some(v) => cmd.env(name, v),
            None => cmd.env_remove(name),
        };
    }
    cmd.stdin(Stdio::null());
    if from_terminal() {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        // no console either, so one doesn't sit behind the game (its own window is unaffected)
        #[cfg(windows)]
        {
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
    }
    cmd.spawn().map_err(|e| e.to_string())
}

/// Starts the game, with the top bar's `toggles`, and returns its pid (on Unix also its
/// process group id).
fn launch(g: &Game, toggles: &Toggles) -> Result<u32, String> {
    spawn(&g.exec, g.workdir.as_deref(), &g.env.with(toggles)).map(|c| c.id())
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
  -s, --search <TEXT>  Start with TEXT in the search bar (t:tag, g:genre filter)
  -h, --help           Show this help

Commands that run without opening the window (use one):
      --launch <GAME>  Start GAME
      --list           Print the names of all games
      --show <GAME>    Print how GAME will run (exec, workdir, env, menu)
      --check          Check the whole config, hidden games included
      --igdb <NAME>    Search IGDB for NAME and print the JSON it returns
";

/// A command that runs without opening the window.
#[derive(Debug, PartialEq)]
enum Action {
    Launch(String),
    List,
    Show(String),
    Check,
    Igdb(String),
}

struct Cli {
    fullscreen: bool,
    config: Option<PathBuf>,
    search: String,
    action: Option<Action>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli { fullscreen: false, config: None, search: String::new(), action: None };
    let set_action = |cli: &mut Cli, action: Action| match cli.action {
        None => {
            cli.action = Some(action);
            Ok(())
        }
        Some(_) => Err("use only one of --launch, --list, --show, --check and --igdb".to_string()),
    };
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
            "--list" if inline.is_none() => set_action(&mut cli, Action::List)?,
            "--check" if inline.is_none() => set_action(&mut cli, Action::Check)?,
            "-c" | "--config" | "-s" | "--search" | "--show" | "--launch" | "--igdb" => {
                let value = inline
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{flag} needs an argument"))?;
                match flag {
                    "-c" | "--config" => cli.config = Some(PathBuf::from(config::expand(&value))),
                    "-s" | "--search" => cli.search = value,
                    "--show" => set_action(&mut cli, Action::Show(value))?,
                    "--igdb" => set_action(&mut cli, Action::Igdb(value))?,
                    _ => set_action(&mut cli, Action::Launch(value))?,
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
    /// The game's own `playtime` setting.
    pub playtime: bool,
}

/// The job of the background copy of rsgl that stays behind after a launch: wait
/// for the game to exit, saving its playtime along the way if `playtime` is set,
/// then open the launcher again with the `reopen` arguments if those are set.
#[derive(Debug, PartialEq)]
struct Tracker {
    pgid: u32,
    /// Game name and checkpoint interval in seconds.
    playtime: Option<(String, u64)>,
    reopen: Option<Vec<String>>,
}

impl Tracker {
    /// `--track <pgid> [--playtime <checkpoint secs> <name>] [--reopen <args>...]`
    fn to_args(&self) -> Vec<String> {
        let mut args = vec!["--track".to_string(), self.pgid.to_string()];
        if let Some((name, secs)) = &self.playtime {
            args.extend(["--playtime".to_string(), secs.to_string(), name.clone()]);
        }
        if let Some(reopen) = &self.reopen {
            args.push("--reopen".to_string());
            args.extend(reopen.iter().cloned());
        }
        args
    }

    /// Reads what `to_args` wrote (without the program name). None for anything else.
    fn from_args(args: &[String]) -> Option<Self> {
        let [flag, pgid, rest @ ..] = args else { return None };
        if flag != "--track" {
            return None;
        }
        let pgid = pgid.parse().ok()?;
        let (playtime, rest) = match rest {
            [f, secs, name, rest @ ..] if f == "--playtime" => (Some((name.clone(), secs.parse().ok()?)), rest),
            _ => (None, rest),
        };
        let reopen = match rest {
            [] => None,
            [f, reopen @ ..] if f == "--reopen" => Some(reopen.to_vec()),
            _ => return None,
        };
        Some(Self { pgid, playtime, reopen })
    }

    fn run(self) {
        match self.playtime {
            Some((name, secs)) => playtime::track(self.pgid, &name, Duration::from_secs(secs)),
            None => playtime::wait(self.pgid),
        }
        if let Some(args) = self.reopen {
            if let Err(e) = respawn(&args) {
                eprintln!("rsgl: can't reopen the launcher: {e}");
            }
        }
    }

    /// Hands the waiting over to a fresh copy of rsgl, see `respawn`.
    fn start(&self) {
        if let Err(e) = respawn(&self.to_args()) {
            eprintln!("rsgl: can't keep watching the game: {e}");
        }
    }
}

/// A GUI-subsystem program has no console of its own, so hook up to the one it was
/// started from. That keeps --help, error messages and the game's output visible in
/// a terminal. Streams the parent redirected (`> file`) are left alone.
#[cfg(windows)]
fn attach_parent_console() {
    use std::{ffi::c_void, fs::OpenOptions, os::windows::io::IntoRawHandle};

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    unsafe {
        if AttachConsole(u32::MAX) == 0 {
            return; // no parent console, e.g. started from Explorer
        }
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(which);
            // attaching doesn't hand a GUI program its std handles, so open them here
            if handle.is_null() || handle as isize == -1 {
                if let Ok(console) = OpenOptions::new().write(true).open("CONOUT$") {
                    SetStdHandle(which, console.into_raw_handle());
                }
            }
        }
    }
}

/// Replaces this process with a fresh copy of rsgl started with `args`. After a
/// launch the game keeps running in its own group, and a copy that only waits for
/// it gives the window's memory (GPU driver, textures, decoded images) back instead
/// of keeping it until the game exits. exec keeps our pid, so the game stays our
/// child and can still be reaped. Only returns on failure.
#[cfg(unix)]
fn respawn(args: &[String]) -> Result<(), std::io::Error> {
    Err(Command::new("/proc/self/exe").arg0("rsgl").args(args).exec())
}

/// Windows has no exec, so start the copy as a detached process; the caller then
/// exits, which frees the window's memory just the same.
#[cfg(windows)]
fn respawn(args: &[String]) -> Result<(), std::io::Error> {
    Command::new(std::env::current_exe()?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(drop)
}

/// Runs a command-line action and returns the exit code.
fn run_action(action: Action) -> i32 {
    let fail = |msg: String| {
        eprintln!("rsgl: {msg}");
        1
    };
    if action == Action::Check {
        return run_check();
    }
    let Settings { games, playtime: pt, api, .. } = match config::load_existing() {
        Ok(loaded) => loaded,
        Err(e) => return fail(e),
    };

    match action {
        Action::Check => unreachable!("handled above"),
        Action::Igdb(name) => {
            let Some((id, secret)) = api.igdb_credentials() else {
                return fail("set igdb_client_id and igdb_client_secret under [api] \
                    (or IGDB_CLIENT_ID and IGDB_CLIENT_SECRET)".into());
            };
            match api_igdb::Client::new(id, secret).and_then(|mut c| c.search(&name)) {
                Ok(json) => {
                    println!("{}", json.trim_end());
                    0
                }
                Err(e) => fail(format!("IGDB search failed: {e}")),
            }
        }
        Action::List => {
            for g in &games {
                println!("{}", g.name);
            }
            0
        }
        Action::Show(query) => match config::find(&games, &query) {
            Ok(g) => {
                print!("{}", config::describe(g));
                0
            }
            Err(e) => fail(e),
        },
        Action::Launch(query) => {
            let game = match config::find(&games, &query) {
                Ok(g) => g,
                Err(e) => return fail(e),
            };
            // no window, so no toggles: [env] is what they start from anyway
            match launch(game, &Toggles::new()) {
                Ok(pgid) => {
                    // like a launch from the window: with playtime on, a small copy of rsgl
                    // stays behind to time the game (on Unix this replaces this process)
                    if pt.enabled && game.playtime {
                        let playtime = Some((game.name.clone(), pt.checkpoint_minutes * 60));
                        Tracker { pgid, playtime, reopen: None }.start();
                    }
                    0
                }
                Err(e) => fail(format!("couldn't start {}: {e}", game.name)),
            }
        }
    }
}

/// `--check`: prints what's wrong with the config. Only errors make the exit code non-zero.
fn run_check() -> i32 {
    const SHOWN: usize = 20;
    let report = match config::check() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rsgl: {e}");
            return 1;
        }
    };
    println!("checked {}: {} games ({} hidden)", report.path.display(), report.games, report.hidden);
    for e in report.errors.iter().take(SHOWN) {
        println!("error: {e}");
    }
    if report.errors.len() > SHOWN {
        println!("... and {} more errors", report.errors.len() - SHOWN);
    }
    for w in &report.warnings {
        println!("warning: {w}");
    }
    let count = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    println!("{}, {}", count(report.errors.len(), "error"), count(report.warnings.len(), "warning"));
    i32::from(!report.errors.is_empty())
}

fn main() -> eframe::Result {
    #[cfg(windows)]
    attach_parent_console();

    let args: Vec<String> = std::env::args().skip(1).collect();
    // internal: the copy that stays behind after a launch, see `Tracker`
    if let Some(tracker) = Tracker::from_args(&args) {
        tracker.run();
        return Ok(());
    }

    let cli = match parse_args(args.iter().cloned()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rsgl: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let Cli { fullscreen, config: config_path, search, action } = cli;
    if let Some(path) = config_path {
        if !path.is_file() {
            eprintln!("rsgl: {}: config file not found", path.display());
            std::process::exit(1);
        }
        config::set_config_path(path);
    }

    // no window, no single-instance lock, and no example config written
    if let Some(action) = action {
        std::process::exit(run_action(action));
    }

    let (settings, error) = match config::load() {
        Ok(s) => (s, None),
        Err(e) => (Settings::default(), Some(e)),
    };

    // kept open while the window is up; std opens files close-on-exec, so the
    // tracker below doesn't hold on to it
    let _instance = if settings.ui.single_instance {
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

    let track = settings.playtime.enabled;
    let checkpoint = settings.playtime.checkpoint_minutes * 60;
    let reopen = settings.ui.reopen_after_game;
    let fullscreen = fullscreen || settings.ui.fullscreen;
    let launched = ui::run(settings, error, fullscreen, search)?;

    if let Some(l) = launched {
        let tracker = Tracker {
            pgid: l.pgid,
            playtime: (track && l.playtime).then_some((l.name, checkpoint)),
            // the same arguments, so the reopened window uses the same config, -f and -s
            reopen: reopen.then_some(args),
        };
        if tracker.playtime.is_some() || tracker.reopen.is_some() {
            tracker.start();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[cfg(unix)]
    #[test]
    fn shell_runs_the_whole_command_list() {
        let out = shell_command("test -n 'x' && echo second").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "second\n");
    }

    #[test]
    fn actions_and_their_arguments() {
        assert_eq!(parse(&["--list"]).unwrap().action, Some(Action::List));
        assert_eq!(parse(&["--check"]).unwrap().action, Some(Action::Check));
        assert_eq!(parse(&["--show", "Quake"]).unwrap().action, Some(Action::Show("Quake".into())));
        assert_eq!(parse(&["--launch=Left 4 Dead 2"]).unwrap().action, Some(Action::Launch("Left 4 Dead 2".into())));
        assert_eq!(parse(&["--igdb", "Portal 2"]).unwrap().action, Some(Action::Igdb("Portal 2".into())));
        assert_eq!(parse(&[]).unwrap().action, None);
    }

    #[test]
    fn only_one_action_at_a_time() {
        let e = parse(&["--list", "--check"]).err().unwrap();
        assert!(e.contains("only one of"), "{e}");
        assert!(parse(&["--show", "A", "--launch", "B"]).is_err());
    }

    #[test]
    fn bad_arguments_are_errors() {
        assert!(parse(&["--launch"]).err().unwrap().contains("needs an argument"));
        assert!(parse(&["--list=1"]).err().unwrap().contains("unknown argument"));
        assert!(parse(&["--nope"]).is_err());
    }

    #[test]
    fn tracker_arguments_round_trip() {
        let strings = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for t in [
            Tracker { pgid: 42, playtime: None, reopen: None },
            Tracker { pgid: 42, playtime: Some(("".into(), 300)), reopen: None },
            Tracker { pgid: 42, playtime: None, reopen: Some(vec![]) },
            Tracker { pgid: 42, playtime: Some(("--reopen".into(), 0)), reopen: Some(strings(&["-c", "x", "-f"])) },
        ] {
            assert_eq!(Tracker::from_args(&t.to_args()), Some(t));
        }
        // anything else is left to the normal argument parsing
        assert_eq!(Tracker::from_args(&strings(&["--track", "x"])), None);
        assert_eq!(Tracker::from_args(&strings(&["--track", "1", "--nope"])), None);
        assert_eq!(Tracker::from_args(&strings(&["-f"])), None);
    }

    #[test]
    fn options_still_work_alongside_actions() {
        let cli = parse(&["-f", "-s", "t:coop", "--config=/x.toml", "--list"]).unwrap();
        assert!(cli.fullscreen);
        assert_eq!(cli.search, "t:coop");
        assert_eq!(cli.config, Some(PathBuf::from("/x.toml")));
        assert_eq!(cli.action, Some(Action::List));
    }
}
