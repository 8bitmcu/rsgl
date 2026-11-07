use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Config file given with --config. When set, it replaces the default locations,
/// and it also applies to F5 reloads, since those go through `load()` again.
static CONFIG_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_config_path(path: PathBuf) {
    let _ = CONFIG_OVERRIDE.set(path);
}

/// Written to the first config location when no config file exists yet.
const EXAMPLE: &str = r#"# rsgl config

[ui]
card_width = 170
# background blur strength, 0 turns it off
blur = 8
# only allow one launcher window at a time
single_instance = true
# open the window maximized
start_maximized = false

[playtime]
enabled = true
# save the current session every N minutes, 0 only saves when the game exits
checkpoint_minutes = 5

[api]
# steamgriddb_key = "your-key-here"

# Variables: {name} is replaced in exec, workdir, cover and env values (and in
# menu entries). A game's own `vars` override these, and a variable can use
# others, e.g. gs = "gamescope -w {w} -h {h} -f --". Write {{ for a literal brace.
# [vars]
# games = "~/games"

# Environment variables for every game and menu entry. A game's own `env` wins.
# [env]
# WINEDEBUG = "-all"

# Right-click menu entries shown for every game, before the game's own.
# `action = "open_folder"` opens the workdir; or give an exec to run.
# [[right_click]]
# title = "Open game folder"
# action = "open_folder"

[[game]]
name = "Example Game"
exec = "~/games/example/start.sh"
# cover = "~/.cache/rsgl/covers/example.png"
# workdir = "~/games/example"
# env = { SOME_VAR = "value" }
# playtime = false                             # don't track this game
# hidden = true                                # keep the entry but leave it out of the grid
# tags = ["coop", "lan", "fps"]                # filter with t:coop in the search bar
# steam_appid = 367520                         # pick covers by Steam app id instead of by name
# steamgriddb_id = 1234                        # or by SteamGridDB game id
# vars = { dir = "example" }                   # this game's variables, see [vars]

# Menu entries must come after the game's other keys, since every key below a
# [[game.right_click]] header belongs to that entry.
# [[game.right_click]]
# title = "Play (windowed)"
# exec = "./start.sh -windowed"
# launch = true                                # close rsgl and track playtime, like a normal launch
#
# [[game.right_click]]
# title = "Edit settings"
# exec = "xdg-open ~/.config/example/settings.ini"
# workdir = "~"                                # optional, defaults to the game's workdir
# env = { SOME_VAR = "value" }                 # optional, added to the game's env
"#;

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    ui: UiCfg,
    #[serde(default)]
    api: ApiCfg,
    #[serde(default)]
    playtime: PlaytimeCfg,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
    #[serde(default, rename = "game")]
    games: Vec<GameCfg>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ApiCfg {
    steamgriddb_key: Option<String>,
}

impl ApiCfg {
    /// STEAMGRIDDB_API_KEY from the environment wins over the config file.
    pub fn steamgriddb_key(&self) -> Option<String> {
        let clean = |s: String| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        std::env::var("STEAMGRIDDB_API_KEY")
            .ok()
            .and_then(clean)
            .or_else(|| self.steamgriddb_key.clone().and_then(clean))
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct UiCfg {
    pub card_width: f32,
    /// Gaussian sigma for the background blur.
    pub blur: f32,
    /// Refuse to start while another launcher window is open.
    pub single_instance: bool,
    /// Open the window maximized.
    pub start_maximized: bool,
}

impl Default for UiCfg {
    fn default() -> Self {
        Self { card_width: 170.0, blur: 8.0, single_instance: true, start_maximized: false }
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct PlaytimeCfg {
    pub enabled: bool,
    pub checkpoint_minutes: u64,
}

impl Default for PlaytimeCfg {
    fn default() -> Self {
        Self { enabled: true, checkpoint_minutes: 5 }
    }
}

#[derive(Deserialize)]
struct GameCfg {
    name: String,
    exec: String,
    cover: Option<String>,
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "yes")]
    playtime: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    tags: Vec<String>,
    steam_appid: Option<u64>,
    steamgriddb_id: Option<u64>,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
}

fn yes() -> bool {
    true
}

/// One right-click menu entry, from `[[game.right_click]]` or the top-level
/// `[[right_click]]`. Unknown keys are an error, so a key that ended up under the
/// wrong header (everything below `[[game.right_click]]` belongs to it) is caught.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MenuCfg {
    title: String,
    exec: Option<String>,
    action: Option<String>,
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    launch: bool,
}

/// A right-click menu entry with its paths expanded, and the game's workdir and
/// env already folded in.
pub struct MenuEntry {
    pub title: String,
    pub action: MenuAction,
    pub workdir: Option<PathBuf>,
    pub env: HashMap<String, String>,
    /// Behave like launching the game: close rsgl and track playtime.
    pub launch: bool,
}

#[derive(Debug, PartialEq)]
pub enum MenuAction {
    Exec(String),
    /// Open `workdir` in the file manager.
    OpenFolder,
}

/// A game with all paths expanded and the cover resolved.
pub struct Game {
    pub name: String,
    pub exec: String,
    pub cover: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
    pub env: HashMap<String, String>,
    /// Track playtime for this game (only if playtime is enabled globally too).
    pub playtime: bool,
    /// Free-form labels, searched with `t:tag`.
    pub tags: Vec<String>,
    /// Right-click menu: the shared entries, then the game's own. Empty means right-click
    /// just opens the workdir.
    pub menu: Vec<MenuEntry>,
    /// Blurred hero (or cover) drawn behind the grid, filled in by the fetcher.
    pub background: Option<PathBuf>,
    /// Look the game up on SteamGridDB by this id instead of searching its name.
    pub art_id: Option<ArtId>,
}

/// A configured SteamGridDB lookup that skips the name search.
#[derive(Clone, Copy, PartialEq)]
pub enum ArtId {
    SteamGridDb(u64),
    Steam(u64),
}

impl std::fmt::Display for ArtId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ArtId::SteamGridDb(id) => write!(f, "steamgriddb:{id}"),
            ArtId::Steam(id) => write!(f, "steam:{id}"),
        }
    }
}

pub fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect()
}

/// Expands a leading `~` to the home directory.
pub fn expand(s: &str) -> String {
    // on Windows `~\` works as well as `~/`
    let rest = s
        .strip_prefix("~/")
        .or_else(|| if cfg!(windows) { s.strip_prefix("~\\") } else { None });
    if rest.is_none() && s != "~" {
        return s.to_string();
    }
    let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) else {
        return s.to_string();
    };
    match rest {
        Some(rest) => home.join(native_slashes(rest)).to_string_lossy().into_owned(),
        None => home.to_string_lossy().into_owned(),
    }
}

/// On Windows, turns the `/` in the first word (the path) into `\\`, because cmd.exe and
/// explorer.exe don't accept forward slashes there. Later words (arguments) are left alone.
#[cfg(windows)]
fn native_slashes(s: &str) -> String {
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    format!("{}{}", s[..end].replace('/', "\\"), &s[end..])
}

#[cfg(not(windows))]
fn native_slashes(s: &str) -> String {
    s.to_string()
}

/// Everything rsgl downloads or generates lives under ~/.cache/rsgl.
fn cache_dir() -> PathBuf {
    directories::BaseDirs::new().unwrap().cache_dir().join("rsgl")
}

pub fn covers_dir() -> PathBuf {
    cache_dir().join("covers")
}

pub fn heroes_dir() -> PathBuf {
    cache_dir().join("heroes")
}

pub fn backgrounds_dir() -> PathBuf {
    cache_dir().join("backgrounds")
}

/// Playtime is kept under ~/.local/share/rsgl, since unlike the cache it can't be rebuilt.
pub fn playtime_file() -> PathBuf {
    directories::BaseDirs::new().unwrap().data_dir().join("rsgl/playtime.toml")
}

/// Lock file for single instance mode, in $XDG_RUNTIME_DIR if there is one.
pub fn lock_file() -> PathBuf {
    let dirs = directories::BaseDirs::new().unwrap();
    match dirs.runtime_dir() {
        Some(d) => d.join("rsgl.lock"),
        None => cache_dir().join("rsgl.lock"),
    }
}

/// Where covers and heroes were saved before they moved into the cache.
/// Still searched so existing downloads keep working.
fn legacy_dir(name: &str) -> PathBuf {
    directories::BaseDirs::new().unwrap().data_dir().join(name)
}

/// Looks for `dir/<slug>.{png,jpg,webp}`.
fn find_image(dir: &Path, name: &str) -> Option<PathBuf> {
    ["png", "jpg", "webp"]
        .iter()
        .map(|ext| dir.join(format!("{}.{ext}", slug(name))))
        .find(|p| p.exists())
}

pub fn find_cover(name: &str) -> Option<PathBuf> {
    find_image(&covers_dir(), name).or_else(|| find_image(&legacy_dir("game-covers"), name))
}

pub fn find_hero(name: &str) -> Option<PathBuf> {
    find_image(&heroes_dir(), name).or_else(|| find_image(&legacy_dir("game-heroes"), name))
}

/// Config locations in priority order:
/// 1. $XDG_CONFIG_HOME/rsgl/rsgl.toml (usually ~/.config/rsgl/rsgl.toml;
///    on Windows %APPDATA%\rsgl\rsgl.toml)
/// 2. ~/.rsgl.toml
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = directories::BaseDirs::new() {
        v.push(d.config_dir().join("rsgl/rsgl.toml"));
        v.push(d.home_dir().join(".rsgl.toml"));
    }
    v
}

/// Where the config is: the `--config` file, else the first default location that exists.
fn locate() -> Result<Option<PathBuf>, String> {
    if let Some(p) = CONFIG_OVERRIDE.get() {
        // an explicit path is never created for you
        if !p.is_file() {
            return Err(format!("{}: config file not found", p.display()));
        }
        return Ok(Some(p.clone()));
    }
    Ok(candidates().into_iter().find(|p| p.is_file()))
}

fn read(path: &Path) -> Result<Config, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Finds and parses the config. If no file exists, writes an example to the
/// first location and returns an error telling the user about it.
pub fn load() -> Result<(Vec<Game>, UiCfg, ApiCfg, PlaytimeCfg), String> {
    let Some(path) = locate()? else {
        let candidates = candidates();
        let target = candidates
            .first()
            .ok_or("could not determine the home directory")?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(target, EXAMPLE).map_err(|e| format!("{}: {e}", target.display()))?;
        return Err(format!(
            "No config found. Created an example at {}. Edit it and press F5.",
            target.display()
        ));
    };
    build(read(&path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// Like `load`, for commands that run without a window: a missing config is just
/// an error, and nothing is written to disk.
pub fn load_existing() -> Result<(Vec<Game>, UiCfg, ApiCfg, PlaytimeCfg), String> {
    let path = locate()?.ok_or_else(no_config)?;
    build(read(&path)?).map_err(|e| format!("{}: {e}", path.display()))
}

fn no_config() -> String {
    let places: Vec<String> = candidates().iter().map(|p| p.display().to_string()).collect();
    format!("no config file found (looked for {})", places.join(" and "))
}

/// Values for `{name}` placeholders: the global `[vars]` with a game's own `vars` on top.
/// Numbers and booleans are used as their text, and a value may itself contain `{other}`,
/// which is looked up after the two tables are merged, so a game's override reaches it.
#[derive(Clone, Default)]
struct Vars(BTreeMap<String, String>);

impl Vars {
    /// These variables with `table` added on top.
    fn with(&self, table: &BTreeMap<String, toml::Value>) -> Result<Vars, String> {
        let mut map = self.0.clone();
        for (name, value) in table {
            let text = match value {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(n) => n.to_string(),
                toml::Value::Float(f) => f.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                _ => return Err(format!("{name} must be a string, number or boolean")),
            };
            map.insert(name.clone(), text);
        }
        Ok(Vars(map))
    }

    /// Replaces every `{name}` in `s`. `{{` and `}}` are literal braces.
    fn expand(&self, s: &str) -> Result<String, String> {
        self.expand_in(s, &mut Vec::new())
    }

    /// `chain` is the variables being expanded right now, outermost first, to catch
    /// loops and to say where an error is.
    fn expand_in(&self, s: &str, chain: &mut Vec<String>) -> Result<String, String> {
        let at = chain.last().map(|v| format!(" (in variable {v})")).unwrap_or_default();
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(pos) = rest.find(['{', '}']) {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            if rest.as_bytes()[pos] == b'}' {
                let Some(after) = after.strip_prefix('}') else {
                    return Err(format!("stray }} in {s:?}{at} (write }}}} for a literal brace)"));
                };
                out.push('}');
                rest = after;
            } else if let Some(after) = after.strip_prefix('{') {
                out.push('{');
                rest = after;
            } else {
                let Some(end) = after.find('}') else {
                    return Err(format!("unclosed {{ in {s:?}{at} (write {{{{ for a literal brace)"));
                };
                let name = &after[..end];
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                    return Err(format!(
                        "{{{name}}} isn't a variable name{at} (write {{{{ for a literal brace)"
                    ));
                }
                out.push_str(&self.value(name, chain, &at)?);
                rest = &after[end + 1..];
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    fn value(&self, name: &str, chain: &mut Vec<String>, at: &str) -> Result<String, String> {
        chain.push(name.to_string());
        if chain[..chain.len() - 1].iter().any(|n| n == name) {
            return Err(format!("variables refer to each other in a circle: {}", chain.join(" -> ")));
        }
        let Some(raw) = self.0.get(name) else {
            return Err(format!("{{{name}}} isn't defined{at}"));
        };
        let text = self.expand_in(raw, chain)?;
        chain.pop();
        Ok(text)
    }
}

/// Resolves one menu entry for a game. `owner` names where the entry came from, for errors.
fn menu_entry(
    m: &MenuCfg,
    owner: &str,
    workdir: Option<&Path>,
    env: &HashMap<String, String>,
    vars: &Vars,
) -> Result<MenuEntry, String> {
    let err = |msg: String| format!("{owner}, entry {:?}: {msg}", m.title);
    let sub = |field: &str, s: &str| vars.expand(s).map_err(|e| err(format!("{field}: {e}")));

    let action = match (&m.exec, m.action.as_deref()) {
        (Some(exec), None) => MenuAction::Exec(expand(&sub("exec", exec)?)),
        (None, Some("open_folder")) => MenuAction::OpenFolder,
        (None, Some(other)) => {
            return Err(err(format!("unknown action {other:?} (the only one is \"open_folder\")")))
        }
        (Some(_), Some(_)) => return Err(err("use exec or action, not both".into())),
        (None, None) => return Err(err("needs an exec (or an action)".into())),
    };
    if m.launch && action == MenuAction::OpenFolder {
        return Err(err("launch = true needs an exec".into()));
    }

    // the entry's own workdir wins, then the game's; env is the game's plus the entry's
    let workdir = match &m.workdir {
        Some(w) => Some(PathBuf::from(expand(&sub("workdir", w)?))),
        None => workdir.map(Path::to_path_buf),
    };
    let mut merged = env.clone();
    for (k, v) in &m.env {
        merged.insert(k.clone(), expand(&sub(&format!("env {k}"), v)?));
    }

    Ok(MenuEntry { title: m.title.clone(), action, workdir, env: merged, launch: m.launch })
}

/// What every game starts from: the global `[vars]`, `[env]` and `[[right_click]]`.
struct Shared {
    vars: Vars,
    env: BTreeMap<String, String>,
    menu: Vec<MenuCfg>,
}

/// Turns the parsed file into games: fills in variables, expands paths, finds
/// covers, resolves menus. Hidden games are skipped without being checked.
fn build(cfg: Config) -> Result<(Vec<Game>, UiCfg, ApiCfg, PlaytimeCfg), String> {
    let vars = Vars::default().with(&cfg.vars).map_err(|e| format!("[vars]: {e}"))?;
    let shared = Shared { vars, env: cfg.env, menu: cfg.right_click };

    let mut games = Vec::new();
    for g in cfg.games.into_iter().filter(|g| !g.hidden) {
        games.push(build_game(g, &shared)?);
    }
    games.sort_by_cached_key(|g| g.name.to_lowercase());
    Ok((games, cfg.ui, cfg.api, cfg.playtime))
}

fn build_game(g: GameCfg, shared: &Shared) -> Result<Game, String> {
    let who = format!("game {:?}", g.name);
    let vars = shared.vars.with(&g.vars).map_err(|e| format!("{who}, vars: {e}"))?;
    let sub = |field: &str, s: &str| vars.expand(s).map_err(|e| format!("{who}, {field}: {e}"));

    let exec = expand(&sub("exec", &g.exec)?);
    let workdir = g
        .workdir
        .as_ref()
        .map(|w| sub("workdir", w))
        .transpose()?
        .map(|w| PathBuf::from(expand(&w)));

    // the global [env] first, then the game's own on top
    let mut env = HashMap::new();
    for (k, v) in &shared.env {
        let v = vars.expand(v).map_err(|e| format!("[env] (for {who}), {k}: {e}"))?;
        env.insert(k.clone(), expand(&v));
    }
    for (k, v) in &g.env {
        env.insert(k.clone(), expand(&sub(&format!("env {k}"), v)?));
    }

    // a configured cover that doesn't exist falls back to the covers dir,
    // otherwise a downloaded cover would never be picked up again
    let cover = g
        .cover
        .as_ref()
        .map(|c| sub("cover", c))
        .transpose()?
        .map(|c| PathBuf::from(expand(&c)))
        .filter(|p| p.exists())
        .or_else(|| find_cover(&g.name));

    // shared entries first, then the game's own; both see this game's variables
    let mut menu = Vec::new();
    let shared_owner = format!("[[right_click]] (for {who})");
    for m in &shared.menu {
        menu.push(menu_entry(m, &shared_owner, workdir.as_deref(), &env, &vars)?);
    }
    for m in &g.right_click {
        menu.push(menu_entry(m, &who, workdir.as_deref(), &env, &vars)?);
    }

    Ok(Game {
        exec,
        cover,
        workdir,
        env,
        playtime: g.playtime,
        tags: g
            .tags
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        menu,
        background: None,
        // the SteamGridDB id is the more direct one if both are set
        art_id: g
            .steamgriddb_id
            .map(ArtId::SteamGridDb)
            .or(g.steam_appid.map(ArtId::Steam)),
        name: g.name,
    })
}

/// What `--check` found.
pub struct Report {
    pub path: PathBuf,
    pub games: usize,
    pub hidden: usize,
    /// Things that stop a game from loading.
    pub errors: Vec<String>,
    /// Things that load but look wrong.
    pub warnings: Vec<String>,
}

/// Looks over the whole config, hidden games included, and reports every problem
/// instead of stopping at the first. Writes nothing. An `Err` means the file
/// couldn't be found, read or parsed at all.
pub fn check() -> Result<Report, String> {
    let path = locate()?.ok_or_else(no_config)?;
    let mut report = check_config(read(&path)?);
    report.path = path;
    Ok(report)
}

fn check_config(cfg: Config) -> Report {
    let mut report = Report {
        path: PathBuf::new(),
        games: cfg.games.len(),
        hidden: cfg.games.iter().filter(|g| g.hidden).count(),
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let vars = match Vars::default().with(&cfg.vars) {
        Ok(v) => v,
        Err(e) => {
            report.errors.push(format!("[vars]: {e}"));
            return report;
        }
    };
    let shared = Shared { vars, env: cfg.env, menu: cfg.right_click };

    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for g in cfg.games {
        let (name, hidden) = (g.name.clone(), g.hidden);
        names.entry(name.to_lowercase()).or_default().push(name);
        let note = if hidden { " (hidden game)" } else { "" };

        let game = match build_game(g, &shared) {
            Ok(game) => game,
            Err(e) => {
                report.errors.push(format!("{e}{note}"));
                continue;
            }
        };
        // launching fails with the same message when the folder is missing
        if let Some(wd) = game.workdir.as_ref().filter(|d| !d.is_dir()) {
            report.warnings.push(format!("game {:?}: the workdir {} doesn't exist{note}", game.name, wd.display()));
        }
        for e in &game.menu {
            // an entry that just uses the game's workdir is covered by the line above
            if let Some(wd) = e.workdir.as_ref().filter(|d| e.workdir != game.workdir && !d.is_dir()) {
                report.warnings.push(format!(
                    "game {:?}, entry {:?}: the workdir {} doesn't exist{note}",
                    game.name,
                    e.title,
                    wd.display()
                ));
            }
        }
    }
    for same in names.values().filter(|v| v.len() > 1) {
        report.warnings.push(format!(
            "{} games are named {:?}; playtime is kept per name, and --launch and --show can't tell them apart",
            same.len(),
            same[0]
        ));
    }
    report
}

/// The game for `--show`: an exact name (any case), or a name containing `query`
/// if only one does.
pub fn find<'a>(games: &'a [Game], query: &str) -> Result<&'a Game, String> {
    let q = query.to_lowercase();
    if let Some(g) = games.iter().find(|g| g.name.to_lowercase() == q) {
        return Ok(g);
    }
    let hits: Vec<&Game> = games.iter().filter(|g| g.name.to_lowercase().contains(&q)).collect();
    match hits.as_slice() {
        [g] => Ok(g),
        [] => Err(format!("no game matches {query:?} (hidden games aren't loaded)")),
        many => Err(format!(
            "{query:?} matches {} games: {}",
            many.len(),
            many.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// What `--show` prints: the game as rsgl will actually run it, with every variable filled in.
pub fn describe(g: &Game) -> String {
    fn env_lines(env: &[(&String, &String)], indent: &str) -> String {
        env.iter().map(|(k, v)| format!("{indent}{k}={v}\n")).collect()
    }
    fn sorted(env: &HashMap<String, String>) -> Vec<(&String, &String)> {
        let mut v: Vec<_> = env.iter().collect();
        v.sort();
        v
    }
    let dir = |d: &Option<PathBuf>| d.as_ref().map_or("(none)".to_string(), |d| d.display().to_string());

    let mut out = format!("{}\n  exec:    {}\n  workdir: {}\n", g.name, g.exec, dir(&g.workdir));
    if g.env.is_empty() {
        out += "  env:     (none)\n";
    } else {
        out += "  env:\n";
        out += &env_lines(&sorted(&g.env), "    ");
    }
    if g.menu.is_empty() {
        out += "  menu:    (none, right-click opens the workdir)\n";
        return out;
    }
    out += "  menu:\n";
    for (i, e) in g.menu.iter().enumerate() {
        let what = match &e.action {
            MenuAction::Exec(cmd) => format!("exec: {cmd}"),
            MenuAction::OpenFolder => "action: open_folder".to_string(),
        };
        let launch = if e.launch { " (launch)" } else { "" };
        out += &format!("    {}. {}{launch}\n       {what}\n       workdir: {}\n", i + 1, e.title, dir(&e.workdir));
        // only what the entry adds to the game's env
        let extra: HashMap<_, _> = e.env.iter().filter(|(k, v)| g.env.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect();
        if !extra.is_empty() {
            out += "       env:\n";
            out += &env_lines(&sorted(&extra), "         ");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn games(text: &str) -> Result<Vec<Game>, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        build(cfg).map(|(games, ..)| games)
    }

    #[test]
    fn menus_merge_shared_entries_workdir_and_env() {
        let g = games(
            r#"
[[right_click]]
title = "Open folder"
action = "open_folder"

[[game]]
name = "A"
exec = "a"
workdir = "/games/a"
env = { X = "1", Y = "2" }

[[game.right_click]]
title = "Tool"
exec = "tool"
workdir = "/elsewhere"
env = { Y = "3" }

[[game.right_click]]
title = "Windowed"
exec = "./a -windowed"
launch = true

[[game]]
name = "B"
exec = "b"
"#,
        )
        .unwrap();

        let a = &g[0];
        assert_eq!(a.menu.len(), 3, "shared entry first, then the game's own");
        assert_eq!(a.menu[0].action, MenuAction::OpenFolder);
        assert_eq!(a.menu[0].workdir, Some(PathBuf::from("/games/a")));

        let tool = &a.menu[1];
        assert_eq!(tool.action, MenuAction::Exec("tool".into()));
        assert_eq!(tool.workdir, Some(PathBuf::from("/elsewhere")), "own workdir wins");
        assert_eq!(tool.env["X"], "1", "game env is kept");
        assert_eq!(tool.env["Y"], "3", "entry env overrides");
        assert!(!tool.launch);

        let windowed = &a.menu[2];
        assert_eq!(windowed.workdir, Some(PathBuf::from("/games/a")), "defaults to the game's");
        assert!(windowed.launch);

        let b = &g[1];
        assert_eq!(b.menu.len(), 1, "only the shared entry");
        assert_eq!(b.menu[0].workdir, None);
    }

    #[test]
    fn games_without_entries_have_no_menu() {
        let g = games("[[game]]\nname = \"A\"\nexec = \"a\"\n").unwrap();
        assert!(g[0].menu.is_empty());
    }

    #[test]
    fn bad_entries_are_errors() {
        let entry = |body: &str| format!("[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\n{body}\n");
        for (body, want) in [
            ("title = \"T\"", "needs an exec"),
            ("title = \"T\"\nexec = \"x\"\naction = \"open_folder\"", "not both"),
            ("title = \"T\"\naction = \"nope\"", "unknown action"),
            ("title = \"T\"\naction = \"open_folder\"\nlaunch = true", "needs an exec"),
        ] {
            let e = games(&entry(body)).err().unwrap_or_else(|| panic!("{body:?} should fail"));
            assert!(e.contains(want), "{body:?} gave {e:?}");
        }
    }

    #[test]
    fn a_key_below_a_menu_header_is_rejected() {
        // `tags` here belongs to the menu entry, not the game
        let e = games(
            "[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = \"x\"\ntags = [\"x\"]\n",
        )
        .err()
        .expect("should fail");
        assert!(e.contains("unknown field"), "{e}");
    }

    fn game_exec(text: &str) -> String {
        games(text).unwrap().remove(0).exec
    }

    fn game_err(text: &str) -> String {
        games(text).err().expect("should fail")
    }

    #[test]
    fn vars_nest_and_game_overrides_reach_them() {
        let g = games(
            r#"
[vars]
games = "/g"
w = 3440
h = 1440
gs = "gs -w {w} -h {h} --"

[[game]]
name = "A"
exec = "{gs} wine a.exe"
workdir = "{games}/A"

[[game]]
name = "B"
vars = { w = 2560 }
exec = "{gs} wine b.exe"
workdir = "{games}/B"
"#,
        )
        .unwrap();
        assert_eq!(g[0].exec, "gs -w 3440 -h 1440 -- wine a.exe");
        assert_eq!(g[0].workdir, Some(PathBuf::from("/g/A")));
        assert_eq!(g[1].exec, "gs -w 2560 -h 1440 -- wine b.exe", "the game's w flows through {{gs}}");
    }

    #[test]
    fn braces_can_be_escaped() {
        let e = game_exec("[[game]]\nname = \"A\"\nexec = \"awk '{{print $1}}' {{x}}\"\n");
        assert_eq!(e, "awk '{print $1}' {x}");
    }

    #[test]
    fn numbers_and_booleans_are_their_text() {
        let e = game_exec(
            "[vars]\nn = 5\nf = 1.5\nb = true\n[[game]]\nname = \"A\"\nexec = \"{n} {f} {b}\"\n",
        );
        assert_eq!(e, "5 1.5 true");
    }

    #[test]
    fn tilde_is_expanded_after_substitution() {
        let g = games("[vars]\nbase = \"~/g\"\n[[game]]\nname = \"A\"\nexec = \"x\"\nworkdir = \"{base}/A\"\n")
            .unwrap();
        let wd = g[0].workdir.clone().unwrap();
        assert_eq!(wd, PathBuf::from(expand("~/g/A")));
        assert!(!wd.starts_with("~"));
    }

    #[test]
    fn vars_work_in_cover_env_and_menu_entries() {
        let g = games(
            r#"
[vars]
dir = "Quake"

[[right_click]]
title = "Open"
exec = "xdg-open /g/{dir}"
env = { D = "{dir}" }

[[game]]
name = "A"
exec = "x"
env = { WINEPREFIX = "/p/{dir}" }
cover = "/c/{dir}.png"

[[game.right_click]]
title = "Tool"
exec = "tool {dir}"
workdir = "/w/{dir}"
"#,
        )
        .unwrap();
        let a = &g[0];
        assert_eq!(a.env["WINEPREFIX"], "/p/Quake");
        assert_eq!(a.menu[0].action, MenuAction::Exec("xdg-open /g/Quake".into()));
        assert_eq!(a.menu[0].env["D"], "Quake");
        assert_eq!(a.menu[1].action, MenuAction::Exec("tool Quake".into()));
        assert_eq!(a.menu[1].workdir, Some(PathBuf::from("/w/Quake")));
    }

    #[test]
    fn a_shared_menu_entry_sees_each_games_own_vars() {
        let text = |second: &str| {
            format!(
                "[[right_click]]\ntitle = \"T\"\nexec = \"tool {{dir}}\"\n\
                 [[game]]\nname = \"A\"\nexec = \"a\"\nvars = {{ dir = \"one\" }}\n\
                 [[game]]\nname = \"B\"\nexec = \"b\"\n{second}\n"
            )
        };
        let g = games(&text("vars = { dir = \"two\" }")).unwrap();
        assert_eq!(g[0].menu[0].action, MenuAction::Exec("tool one".into()));
        assert_eq!(g[1].menu[0].action, MenuAction::Exec("tool two".into()));

        // a game that doesn't define it is reported, naming both the entry and the game
        let e = game_err(&text(""));
        assert!(e.contains("[[right_click]]") && e.contains("\"B\"") && e.contains("{dir} isn't defined"), "{e}");
    }

    #[test]
    fn errors_name_the_game_and_the_field() {
        let e = game_err("[[game]]\nname = \"A\"\nexec = \"run {nope}\"\n");
        assert!(e.contains("game \"A\"") && e.contains("exec") && e.contains("{nope} isn't defined"), "{e}");

        let e = game_err("[[game]]\nname = \"A\"\nexec = \"x\"\nworkdir = \"{nope}\"\n");
        assert!(e.contains("workdir"), "{e}");

        let e = game_err("[[game]]\nname = \"A\"\nexec = \"x\"\nenv = { K = \"{nope}\" }\n");
        assert!(e.contains("env K"), "{e}");

        // an undefined name inside a variable says which variable
        let e = game_err("[vars]\ngs = \"gamescope {w}\"\n[[game]]\nname = \"A\"\nexec = \"{gs}\"\n");
        assert!(e.contains("{w} isn't defined") && e.contains("in variable gs"), "{e}");
    }

    #[test]
    fn loops_are_errors() {
        let e = game_err("[vars]\na = \"{b}\"\nb = \"{a}\"\n[[game]]\nname = \"A\"\nexec = \"{a}\"\n");
        assert!(e.contains("circle") && e.contains("a -> b -> a"), "{e}");
        let e = game_err("[vars]\na = \"x{a}\"\n[[game]]\nname = \"A\"\nexec = \"{a}\"\n");
        assert!(e.contains("a -> a"), "{e}");
    }

    #[test]
    fn stray_and_odd_braces_are_errors() {
        for (exec, want) in [
            ("run {", "unclosed {"),
            ("run }", "stray }"),
            ("run {}", "isn't a variable name"),
            ("run {a b}", "isn't a variable name"),
            ("awk '{print $1}'", "isn't a variable name"),
        ] {
            let e = game_err(&format!("[[game]]\nname = \"A\"\nexec = \"{}\"\n", exec.replace('"', "\\\"")));
            assert!(e.contains(want), "{exec:?} gave {e:?}");
        }
    }

    #[test]
    fn only_strings_numbers_and_booleans_are_vars() {
        let e = game_err("[vars]\nx = [1, 2]\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
        assert!(e.contains("[vars]") && e.contains("x must be"), "{e}");
        let e = game_err("[[game]]\nname = \"A\"\nexec = \"a\"\nvars = { x = { y = 1 } }\n");
        assert!(e.contains("game \"A\", vars"), "{e}");
    }

    #[test]
    fn hidden_games_are_not_checked() {
        let g = games("[[game]]\nname = \"A\"\nexec = \"{undefined}\"\nhidden = true\n[[game]]\nname = \"B\"\nexec = \"b\"\n")
            .unwrap();
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn show_finds_games_and_describes_them() {
        let g = games(
            r#"
[[game]]
name = "Left 4 Dead 2"
exec = "run l4d2"
workdir = "/g/l4d2"
env = { A = "1" }

[[game.right_click]]
title = "Tool"
exec = "tool"
env = { B = "2" }

[[game]]
name = "Left Alive"
exec = "x"
"#,
        )
        .unwrap();
        assert_eq!(find(&g, "left 4 dead 2").unwrap().name, "Left 4 Dead 2");
        assert_eq!(find(&g, "alive").unwrap().name, "Left Alive");
        assert!(find(&g, "left").err().unwrap().contains("matches 2 games"));
        assert!(find(&g, "zzz").err().unwrap().contains("no game matches"));

        let text = describe(&g[0]);
        assert!(text.contains("exec:    run l4d2") && text.contains("workdir: /g/l4d2"), "{text}");
        assert!(text.contains("A=1") && text.contains("1. Tool") && text.contains("B=2"), "{text}");
    }

    #[test]
    fn global_env_sits_under_each_games_own() {
        let g = games(
            r#"
[vars]
prefix = "/p"

[env]
WINEDEBUG = "-all"
WINEPREFIX = "{prefix}/{dir}"
SHARED = "global"

[[right_click]]
title = "Tool"
exec = "tool"

[[game]]
name = "A"
exec = "a"
vars = { dir = "a" }
env = { SHARED = "mine", ONLY_A = "1" }
"#,
        )
        .unwrap();
        let a = &g[0];
        assert_eq!(a.env["WINEDEBUG"], "-all", "global values apply");
        assert_eq!(a.env["WINEPREFIX"], "/p/a", "and see the game's variables");
        assert_eq!(a.env["SHARED"], "mine", "the game's own value wins");
        assert_eq!(a.env["ONLY_A"], "1");
        assert_eq!(a.menu[0].env["WINEDEBUG"], "-all", "menu entries get it too");
    }

    #[test]
    fn global_env_errors_say_where_they_came_from() {
        let e = game_err("[env]\nK = \"{dir}\"\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
        assert!(e.contains("[env]") && e.contains("game \"A\"") && e.contains("{dir} isn't defined"), "{e}");
        // values must be strings, like a game's env
        assert!(games("[env]\nK = 1\n[[game]]\nname = \"A\"\nexec = \"a\"\n").is_err());
    }

    fn checked(text: &str) -> Report {
        check_config(toml::from_str(text).unwrap())
    }

    #[test]
    fn check_reports_every_problem_hidden_games_included() {
        let dir = std::env::temp_dir();
        let r = checked(&format!(
            r#"
[[game]]
name = "Fine"
exec = "a"
workdir = {dir:?}

[[game]]
name = "Bad one"
exec = "{{nope}}"

[[game]]
name = "Bad hidden"
exec = "{{nope2}}"
hidden = true

[[game]]
name = "Gone"
exec = "a"
workdir = "/definitely/not/here"

[[game.right_click]]
title = "Tool"
exec = "t"
workdir = "/also/not/here"
"#
        ));
        assert_eq!((r.games, r.hidden), (4, 1));
        assert_eq!(r.errors.len(), 2, "both bad games are reported, not just the first: {:?}", r.errors);
        assert!(r.errors[0].contains("Bad one") && r.errors[0].contains("{nope}"));
        assert!(r.errors[1].contains("Bad hidden") && r.errors[1].contains("hidden game"));
        assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
        assert!(r.warnings[0].contains("Gone") && r.warnings[0].contains("/definitely/not/here"));
        assert!(r.warnings[1].contains("entry \"Tool\"") && r.warnings[1].contains("/also/not/here"));
    }

    #[test]
    fn check_warns_about_duplicate_names_and_stops_on_bad_vars() {
        let r = checked("[[game]]\nname = \"Quake\"\nexec = \"a\"\n[[game]]\nname = \"quake\"\nexec = \"b\"\n");
        assert!(r.errors.is_empty());
        assert!(r.warnings.iter().any(|w| w.contains("2 games are named")), "{:?}", r.warnings);

        let r = checked("[vars]\nx = [1]\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].contains("[vars]"));
    }
}
