// Release builds on Windows are GUI programs, so no console window opens next to the launcher.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api_hasheous;
mod api_igdb;
mod api_steamgriddb;
mod art;
mod blur;
mod config;
mod gamepad;
mod keys;
mod playtime;
mod search;
mod status;
mod topbar;
mod ui;
mod vdf;

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

/// Starts the first command of `exec` and returns a `Launched` with its pid (on Unix also
/// its process group id) and any remaining commands for `Tracker`.
pub fn start_launch(
    name: &str,
    exec: &[String],
    workdir: Option<&Path>,
    env: &config::Env,
    toggles: &Toggles,
    playtime: bool,
) -> Result<Launched, String> {
    let env_changes = env.with(toggles);
    let child = spawn(&exec[0], workdir, &env_changes)?;
    let rest = exec[1..].to_vec();
    let (workdir, env) = match rest.is_empty() {
        true => (None, Vec::new()),
        false => (
            workdir.map(Path::to_path_buf),
            env_changes.into_iter().map(|(k, v)| (k.to_string(), v.map(str::to_string))).collect(),
        ),
    };
    Ok(Launched { name: name.to_string(), pgid: child.id(), exec: rest, workdir, env, playtime })
}

/// Starts the game, with the top bar's `toggles`.
fn launch(g: &Game, toggles: &Toggles) -> Result<Launched, String> {
    start_launch(&g.name, &g.exec, g.workdir.as_deref(), &g.env, toggles, g.playtime)
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
      --filter <NAME>  Start on the top bar's filter with this title
      --installed      Only include installed games
  -h, --help           Show this help
  -V, --version        Show the version

Commands that run without opening the window (use one):
      --launch <GAME>  Start GAME
      --random         Start a random game
      --list           Print the names of all games
      --show <GAME>    Print how GAME will run (exec, workdir, env, menu)
      --fetch          Download missing art and metadata into the cache
      --check          Check the whole config, hidden games included
      --igdb <NAME>    Search IGDB for NAME and print the JSON it returns
      --igdb-matches   Print which IGDB game each game's year and genres come from
";

/// A command that runs without opening the window.
#[derive(Debug, PartialEq)]
enum Action {
    Launch(String),
    Random,
    List,
    Show(String),
    Fetch,
    Check,
    Igdb(String),
    IgdbMatches,
}

struct Cli {
    fullscreen: bool,
    config: Option<PathBuf>,
    search: String,
    filter: Option<String>,
    installed: bool,
    action: Option<Action>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli {
        fullscreen: false,
        config: None,
        search: String::new(),
        filter: None,
        installed: false,
        action: None,
    };
    let set_action = |cli: &mut Cli, action: Action| match cli.action {
        None => {
            cli.action = Some(action);
            Ok(())
        }
        Some(_) => Err(
            "use only one of --launch, --random, --list, --show, --fetch, --check, --igdb and --igdb-matches"
                .to_string(),
        ),
    };
    while let Some(arg) = args.next() {
        // `--flag=value` is the same as `--flag value`
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "-f" | "--fullscreen" if inline.is_none() => cli.fullscreen = true,
            "--installed" if inline.is_none() => cli.installed = true,
            "-h" | "--help" if inline.is_none() => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" if inline.is_none() => {
                println!("rsgl {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--random" if inline.is_none() => set_action(&mut cli, Action::Random)?,
            "--list" if inline.is_none() => set_action(&mut cli, Action::List)?,
            "--fetch" if inline.is_none() => set_action(&mut cli, Action::Fetch)?,
            "--check" if inline.is_none() => set_action(&mut cli, Action::Check)?,
            "--igdb-matches" if inline.is_none() => set_action(&mut cli, Action::IgdbMatches)?,
            "-c" | "--config" | "-s" | "--search" | "--filter" | "--show" | "--launch" | "--igdb" => {
                let value = inline
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{flag} needs an argument"))?;
                match flag {
                    "-c" | "--config" => cli.config = Some(PathBuf::from(config::expand(&value))),
                    "-s" | "--search" => cli.search = value,
                    "--filter" => cli.filter = Some(value),
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
    /// The remaining `exec` commands to run after `pgid` exits, with their `workdir` and `env`.
    pub exec: Vec<String>,
    pub workdir: Option<PathBuf>,
    pub env: Vec<(String, Option<String>)>,
    /// The game's own `playtime` setting.
    pub playtime: bool,
}

/// The job of the background copy of rsgl that stays behind after a launch: wait
/// for each `exec` step to exit and start the next, saving playtime along the way
/// if `playtime` is set, then open the launcher again with the `reopen` arguments
/// if those are set.
#[derive(Debug, PartialEq)]
struct Tracker {
    pgid: u32,
    exec: Vec<String>,
    workdir: Option<PathBuf>,
    env: Vec<(String, Option<String>)>,
    /// Game name, checkpoint interval in seconds and the playtime file. The file is
    /// passed along because the copy doesn't read the config and its [paths].
    playtime: Option<(String, u64, PathBuf)>,
    reopen: Option<Vec<String>>,
}

impl Tracker {
    /// `--track <pgid> [--workdir <dir>] [--env <k> <v> | --unset <k>]... [--exec <cmd>]...
    /// [--playtime <checkpoint secs> <file> <name>] [--reopen <args>...]`
    fn to_args(&self) -> Vec<String> {
        let mut args = vec!["--track".to_string(), self.pgid.to_string()];
        if let Some(dir) = &self.workdir {
            args.extend(["--workdir".to_string(), dir.to_string_lossy().into_owned()]);
        }
        for (k, v) in &self.env {
            match v {
                Some(v) => args.extend(["--env".to_string(), k.clone(), v.clone()]),
                None => args.extend(["--unset".to_string(), k.clone()]),
            }
        }
        for cmd in &self.exec {
            args.extend(["--exec".to_string(), cmd.clone()]);
        }
        if let Some((name, secs, file)) = &self.playtime {
            // arguments are read as UTF-8 anyway (env::args)
            let file = file.to_string_lossy().into_owned();
            args.extend(["--playtime".to_string(), secs.to_string(), file, name.clone()]);
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
        let mut rest = rest;
        let workdir = match rest {
            [f, dir, r @ ..] if f == "--workdir" => {
                rest = r;
                Some(PathBuf::from(dir))
            }
            _ => None,
        };
        let mut env = Vec::new();
        loop {
            match rest {
                [f, k, v, r @ ..] if f == "--env" => {
                    env.push((k.clone(), Some(v.clone())));
                    rest = r;
                }
                [f, k, r @ ..] if f == "--unset" => {
                    env.push((k.clone(), None));
                    rest = r;
                }
                _ => break,
            }
        }
        let mut exec = Vec::new();
        while let [f, cmd, r @ ..] = rest {
            if f != "--exec" {
                break;
            }
            exec.push(cmd.clone());
            rest = r;
        }
        let (playtime, rest) = match rest {
            [f, secs, file, name, rest @ ..] if f == "--playtime" => {
                (Some((name.clone(), secs.parse().ok()?, PathBuf::from(file))), rest)
            }
            _ => (None, rest),
        };
        let reopen = match rest {
            [] => None,
            [f, reopen @ ..] if f == "--reopen" => Some(reopen.to_vec()),
            _ => return None,
        };
        Some(Self { pgid, exec, workdir, env, playtime, reopen })
    }

    fn run(self) {
        let mut session = self
            .playtime
            .as_ref()
            .map(|(name, secs, file)| playtime::Session::new(file, name, Duration::from_secs(*secs)));
        let mut wait = |pgid| match &mut session {
            Some(s) => s.wait(pgid),
            None => playtime::wait(pgid),
        };
        wait(self.pgid);
        let env: Vec<_> = self.env.iter().map(|(k, v)| (k.as_str(), v.as_deref())).collect();
        for cmd in &self.exec {
            match spawn(cmd, self.workdir.as_deref(), &env) {
                Ok(child) => wait(child.id()),
                Err(e) => eprintln!("rsgl: {e}"),
            }
        }
        if let Some(s) = session {
            s.finish();
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

/// The games in `games` that match `-s` (`search`), `--filter` (`filter`) and `--installed`.
fn matching<'a>(
    games: &'a [Game],
    search: &str,
    filter: Option<&search::Query>,
    installed: bool,
) -> Vec<&'a Game> {
    let query = search::Query::parse(search);
    games
        .iter()
        .filter(|g| {
            (!installed || g.installed())
                && filter.is_none_or(|f| f.matches_game(g))
                && query.matches_game(g)
        })
        .collect()
}

/// Picks one item at random from `items`.
fn pick_random<'a, T>(items: &[&'a T]) -> Option<&'a T> {
    if items.is_empty() {
        return None;
    }
    use std::{
        collections::hash_map::RandomState,
        hash::{BuildHasher, Hasher},
        time::{SystemTime, UNIX_EPOCH},
    };
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut h = RandomState::new().build_hasher();
    h.write_u128(nanos);
    Some(items[(h.finish() as usize) % items.len()])
}

/// Starts `game` without a window (so no toggles: `[env]` is what they start from
/// anyway), and keeps a `Tracker` running if playtime is on or there are more `exec`
/// steps to run.
fn launch_headless(game: &Game, pt: &config::PlaytimeCfg) -> Result<(), String> {
    let l = launch(game, &Toggles::new()).map_err(|e| format!("couldn't start {}: {e}", game.name))?;
    let tracker = Tracker {
        pgid: l.pgid,
        exec: l.exec,
        workdir: l.workdir,
        env: l.env,
        playtime: (pt.enabled && l.playtime)
            .then(|| (l.name, pt.checkpoint_minutes * 60, config::playtime_file())),
        reopen: None,
    };
    if !tracker.exec.is_empty() || tracker.playtime.is_some() {
        tracker.start();
    }
    Ok(())
}

/// `--fetch`: identifies ROMs on Hasheous (if enabled), looks up missing IGDB metadata,
/// then downloads missing covers/heroes from SteamGridDB and blurs backgrounds.
fn run_fetch(
    mut settings: Settings,
    search: &str,
    filter: Option<&str>,
    installed: bool,
) -> Result<(), String> {
    if !settings.unidentified_roms.is_empty() {
        let rx = api_hasheous::spawn_identifier(
            settings.api.hasheous_api(),
            settings.unidentified_roms,
            || {},
        );
        let mut identified_any = false;
        for identified in rx {
            identified_any |= identified;
        }
        if identified_any {
            settings = config::load_existing()?;
        }
    }
    if let Some(title) = filter {
        settings.topbar.pick_filter(title)?;
    }
    let active_filter = filter.and_then(|_| settings.topbar.filter().map(|f| &f.query));
    if !search.is_empty() || active_filter.is_some() || installed {
        let query = search::Query::parse(search);
        settings.games.retain(|g| {
            (!installed || g.installed())
                && active_filter.is_none_or(|f| f.matches_game(g))
                && query.matches_game(g)
        });
    }

    let bars_use_igdb = settings.status.uses_igdb || settings.topbar.uses_igdb;
    let igdb = settings.api.igdb_credentials();
    let meta_jobs = ui::meta_jobs(&settings.games, bars_use_igdb, igdb.is_some());
    for (i, meta) in api_igdb::spawn_meta_fetcher(meta_jobs, igdb, || {}) {
        settings.games[i].set_meta(meta);
    }

    let art_jobs = ui::art_jobs(&settings.games);
    let key = settings.api.steamgriddb_key();
    let blur = settings.ui.blur.clamp(0.0, 50.0);
    for _ in api_steamgriddb::spawn_fetcher(art_jobs, key, blur, || {}) {}
    Ok(())
}

/// Runs a command-line action and returns the exit code.
fn run_action(action: Action, search: &str, filter: Option<&str>, installed: bool) -> i32 {
    let fail = |msg: String| {
        eprintln!("rsgl: {msg}");
        1
    };
    if action == Action::Check {
        return run_check();
    }
    let mut settings = match config::load_existing() {
        Ok(loaded) => loaded,
        Err(e) => return fail(e),
    };
    if action == Action::Fetch {
        return match run_fetch(settings, search, filter, installed) {
            Ok(()) => 0,
            Err(e) => fail(e),
        };
    }
    if matches!(action, Action::List | Action::Random) {
        if let Some(title) = filter {
            if let Err(e) = settings.topbar.pick_filter(title) {
                return fail(e);
            }
        }
    }
    let active_filter = filter.and_then(|_| settings.topbar.filter().map(|f| &f.query));
    let Settings { games, playtime: pt, api, .. } = &settings;

    let igdb = || match api.igdb_credentials() {
        Some((id, secret)) => api_igdb::Client::new(id, secret).map_err(|e| format!("IGDB sign-in failed: {e}")),
        None => Err("set igdb_client_id and igdb_client_secret under [api] \
            (or IGDB_CLIENT_ID and IGDB_CLIENT_SECRET)".into()),
    };

    match action {
        Action::Check | Action::Fetch => unreachable!("handled above"),
        Action::Igdb(name) => {
            let mut client = match igdb() {
                Ok(c) => c,
                Err(e) => return fail(e),
            };
            match client.search(&name) {
                Ok(json) => {
                    println!("{}", json.trim_end());
                    0
                }
                Err(e) => fail(format!("IGDB search failed: {e}")),
            }
        }
        Action::IgdbMatches => {
            let mut client = match igdb() {
                Ok(c) => c,
                Err(e) => return fail(e),
            };
            let mut unsure = 0;
            for g in games {
                match client.find(&g.name, g.igdb_id, g.igdb_platform.as_deref()) {
                    Ok(hit) => {
                        let (sure, line) = igdb_match(&g.name, g.igdb_id, hit.as_ref());
                        unsure += usize::from(!sure);
                        println!("{line}");
                    }
                    Err(e) => return fail(format!("IGDB lookup failed for {:?}: {e}", g.name)),
                }
            }
            if unsure > 0 {
                println!(
                    "\n{unsure} of {} games aren't an exact match by name; see the candidates \
                    with --igdb <NAME> and pin the right one with igdb_id",
                    games.len()
                );
            }
            0
        }
        Action::List => {
            for g in matching(games, search, active_filter, installed) {
                println!("{}", g.name);
            }
            0
        }
        Action::Show(query) => match config::find(games, &query) {
            Ok(g) => {
                print!("{}", config::describe(g));
                0
            }
            Err(e) => fail(e),
        },
        Action::Launch(query) => match config::find(games, &query) {
            Ok(game) => match launch_headless(game, pt) {
                Ok(()) => 0,
                Err(e) => fail(e),
            },
            Err(e) => fail(e),
        },
        Action::Random => {
            let pool = matching(games, search, active_filter, installed);
            let Some(game) = pick_random(&pool) else {
                let msg = if games.is_empty() {
                    "no games in the config (hidden games aren't loaded)"
                } else {
                    "no game matches"
                };
                return fail(msg.into());
            };
            match launch_headless(game, pt) {
                Ok(()) => 0,
                Err(e) => fail(e),
            }
        }
    }
}

/// One line of `--igdb-matches`: the game `name` and what IGDB has for it, with whether
/// that's surely the game: IGDB's name is the same, or it was pinned with `igdb_id`.
fn igdb_match(name: &str, igdb_id: Option<u64>, hit: Option<&api_igdb::Hit>) -> (bool, String) {
    let Some(hit) = hit else {
        return match igdb_id {
            Some(id) => (false, format!("?  {name}  ->  IGDB has no game with igdb_id {id}")),
            None => (false, format!("?  {name}  ->  nothing on IGDB")),
        };
    };
    let year = hit.year().map(|y| format!(" ({y})")).unwrap_or_default();
    let found = format!("{}{year}, igdb_id {}", hit.name, hit.id);
    match (igdb_id, hit.is_named(name)) {
        (Some(_), _) => (true, format!("=  {name}  ->  {found}, pinned")),
        (None, true) => (true, format!("=  {name}  ->  {found}")),
        (None, false) => (false, format!("?  {name}  ->  {found}")),
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
    // glibc raises its mmap threshold to the size of each big block freed, after which
    // decoded images come from the heap and stay with the process once freed: RSS grew
    // by hundreds of MB that way. Pinned, they're mapped and go back to the OS on free.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 4 << 20);
    }

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
    let Cli { fullscreen, config: config_path, search, filter, installed, action } = cli;
    if let Some(path) = config_path {
        if !path.is_file() {
            eprintln!("rsgl: {}: config file not found", path.display());
            std::process::exit(1);
        }
        config::set_config_path(path);
    }

    // no window, no single-instance lock, and no example config written
    if let Some(action) = action {
        std::process::exit(run_action(action, &search, filter.as_deref(), installed));
    }

    let (mut settings, mut error) = match config::load() {
        Ok(s) => (s, None),
        Err(e) => (Settings::default(), Some(e)),
    };
    // a title that isn't there shows in the window like a config error, since a
    // launcher started from a hotkey has no terminal to print it to
    if let (Some(title), None) = (&filter, &error) {
        error = settings.topbar.pick_filter(title).err();
    }

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
    let launched = ui::run(settings, error, fullscreen, search, installed)?;

    if let Some(l) = launched {
        let tracker = Tracker {
            pgid: l.pgid,
            exec: l.exec,
            workdir: l.workdir,
            env: l.env,
            playtime: (track && l.playtime).then(|| (l.name, checkpoint, config::playtime_file())),
            // the same arguments, so the reopened window uses the same config, -f, -s and --filter
            reopen: reopen.then_some(args),
        };
        if !tracker.exec.is_empty() || tracker.playtime.is_some() || tracker.reopen.is_some() {
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
        assert_eq!(parse(&["--random"]).unwrap().action, Some(Action::Random));
        assert_eq!(parse(&["--fetch"]).unwrap().action, Some(Action::Fetch));
        assert_eq!(parse(&["--check"]).unwrap().action, Some(Action::Check));
        assert_eq!(parse(&["--show", "Quake"]).unwrap().action, Some(Action::Show("Quake".into())));
        assert_eq!(parse(&["--launch=Left 4 Dead 2"]).unwrap().action, Some(Action::Launch("Left 4 Dead 2".into())));
        assert_eq!(parse(&["--igdb", "Portal 2"]).unwrap().action, Some(Action::Igdb("Portal 2".into())));
        assert_eq!(parse(&["--igdb-matches"]).unwrap().action, Some(Action::IgdbMatches));
        assert_eq!(parse(&[]).unwrap().action, None);
    }

    #[test]
    fn igdb_matches_flag_what_isnt_surely_the_game() {
        let hit: api_igdb::Hit =
            serde_json::from_str(r#"{"id": 333, "name": "Quake", "first_release_date": 835228800}"#).unwrap();
        assert_eq!(igdb_match("quake", None, Some(&hit)), (true, "=  quake  ->  Quake (1996), igdb_id 333".into()));
        assert_eq!(igdb_match("Quake 4", None, Some(&hit)), (false, "?  Quake 4  ->  Quake (1996), igdb_id 333".into()));
        let (sure, line) = igdb_match("Quake 1", Some(333), Some(&hit));
        assert!(sure && line.ends_with(", pinned"), "{line}");
        assert_eq!(igdb_match("Quake", None, None), (false, "?  Quake  ->  nothing on IGDB".into()));
        assert!(!igdb_match("Quake", Some(9), None).0);
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
        let bare = || Tracker { pgid: 42, exec: vec![], workdir: None, env: vec![], playtime: None, reopen: None };
        for t in [
            bare(),
            Tracker { playtime: Some(("".into(), 300, "/p/playtime.toml".into())), ..bare() },
            Tracker { reopen: Some(vec![]), ..bare() },
            Tracker {
                exec: strings(&["--playtime", "--reopen"]),
                workdir: Some("--exec".into()),
                env: vec![("--exec".into(), Some("--reopen".into())), ("UNSET".into(), None)],
                playtime: Some(("--reopen".into(), 0, "--reopen".into())),
                reopen: Some(strings(&["-c", "x", "-f"])),
                ..bare()
            },
        ] {
            assert_eq!(Tracker::from_args(&t.to_args()), Some(t));
        }
        // anything else is left to the normal argument parsing
        assert_eq!(Tracker::from_args(&strings(&["--track", "x"])), None);
        assert_eq!(Tracker::from_args(&strings(&["--track", "1", "--nope"])), None);
        assert_eq!(Tracker::from_args(&strings(&["-f"])), None);
    }

    #[cfg(unix)]
    #[test]
    fn tracker_runs_exec_steps_in_order_and_continues_past_a_failing_step() {
        let dir = std::env::temp_dir().join(format!("rsgl-tracker-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("steps.txt");
        let pt_file = dir.join("playtime.toml");
        let mut env = config::Env::default();
        env.all.insert("STEP_TAG".into(), "ok".into());

        let steps = vec![
            "printf '1:%s\\n' \"$STEP_TAG\" >> steps.txt".to_string(),
            "sed -i 's/x/y/' missing.cfg".to_string(),
            "printf '3\\n' >> steps.txt".to_string(),
            "printf '4\\n' >> steps.txt".to_string(),
        ];
        let l = start_launch("Delta Force", &steps, Some(&dir), &env, &Toggles::new(), true).unwrap();
        Tracker {
            pgid: l.pgid,
            exec: l.exec,
            workdir: l.workdir,
            env: l.env,
            playtime: Some((l.name, 0, pt_file.clone())),
            reopen: None,
        }
        .run();

        assert_eq!(fs::read_to_string(&log).unwrap(), "1:ok\n3\n4\n");
        let pt: playtime::Playtime = toml::from_str(&fs::read_to_string(&pt_file).unwrap()).unwrap();
        assert!(pt["Delta Force"].last_played > 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn options_still_work_alongside_actions() {
        let cli = parse(&["-f", "--installed", "-s", "t:coop", "--filter=NES", "--config=/x.toml", "--list"]).unwrap();
        assert!(cli.fullscreen);
        assert!(cli.installed);
        assert_eq!(cli.search, "t:coop");
        assert_eq!(cli.filter.as_deref(), Some("NES"));
        assert_eq!(cli.config, Some(PathBuf::from("/x.toml")));
        assert_eq!(cli.action, Some(Action::List));
    }

    #[test]
    fn matching_filters_by_search_topbar_filter_and_installed() {
        let dir = std::env::temp_dir();
        let games = config::parse_games(&format!(
            r#"
[[game]]
name = "Quake"
exec = "a"
workdir = {dir:?}
tags = ["fps", "coop"]

[[game]]
name = "Quake III Arena"
exec = "b"
workdir = "/rsgl-test/missing/dir"
tags = ["fps", "lan"]

[[game]]
name = "Left 4 Dead 2"
exec = "c"
tags = ["coop"]
"#
        ))
        .unwrap();
        let names = |search: &str, filter: Option<&str>, installed: bool| -> Vec<&str> {
            let f = filter.map(search::Query::parse);
            matching(&games, search, f.as_ref(), installed).into_iter().map(|g| g.name.as_str()).collect()
        };
        assert_eq!(names("", None, false), ["Left 4 Dead 2", "Quake", "Quake III Arena"]);
        assert_eq!(names("", None, true), ["Left 4 Dead 2", "Quake"], "Quake III's workdir is missing");
        assert_eq!(names("t:coop", None, false), ["Left 4 Dead 2", "Quake"]);
        assert_eq!(names("quake", Some("t:fps"), false), ["Quake", "Quake III Arena"]);
        assert_eq!(names("quake", Some("t:fps"), true), ["Quake"]);
        assert!(pick_random(&matching(&games, "zork", None, false)).is_none());
        let fps = search::Query::parse("t:fps");
        assert_eq!(pick_random(&matching(&games, "quake", Some(&fps), true)).unwrap().name, "Quake");
    }
}
