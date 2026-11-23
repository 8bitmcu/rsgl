use crate::{
    api_hasheous,
    api_igdb::{self, Meta},
    gamepad::Padmap,
    keys::Keymap,
    status::{Checker, Status, StatusCfg},
    topbar::{Topbar, TopbarCfg},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

mod launcher;
mod lutris;
mod paths;
mod roms;
mod steam;
mod templates;
#[cfg(test)]
mod tests;
mod vars;

pub use paths::{
    backgrounds_dir, covers_dir, expand, find_cover, find_hero, hasheous_file, heroes_dir, igdb_meta_file,
    igdb_token_file, lock_file, playtime_file, slug, write_file,
};
use paths::{config_folder, PathsCfg, PATHS};
use roms::{add_roms, RomsCfg};
use launcher::{add_launchers, LauncherCfg};
use templates::{apply_templates, game_with_templates, TemplateCfg};
pub use vars::{is_env_name, is_var_name, tidy, Vars};
use vars::literal;

/// Config file given with --config. When set, it replaces the default locations,
/// and it also applies to F5 reloads, since those go through `load()` again.
static CONFIG_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_config_path(path: PathBuf) {
    let _ = CONFIG_OVERRIDE.set(path);
}

/// Written to the first config location when no config file exists yet.
const EXAMPLE: &str = include_str!("example.toml");

#[derive(Deserialize)]
struct Config {
    #[serde(default, deserialize_with = "exec_list")]
    include: Vec<String>,
    #[serde(default)]
    ui: UiCfg,
    #[serde(default)]
    api: ApiCfg,
    #[serde(default)]
    paths: PathsCfg,
    #[serde(default)]
    playtime: PlaytimeCfg,
    #[serde(default)]
    keyboard: Keymap,
    #[serde(default)]
    gamepad: Padmap,
    #[serde(default)]
    status: StatusCfg,
    #[serde(default)]
    topbar: TopbarCfg,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Only here to say what replaced it, see `removed_defaults`.
    #[serde(default, rename = "defaults", deserialize_with = "removed_defaults")]
    _defaults: (),
    #[serde(default)]
    templates: BTreeMap<String, TemplateCfg>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
    #[serde(default, rename = "game")]
    games: Vec<GameCfg>,
    #[serde(default)]
    roms: Vec<RomsCfg>,
    #[serde(default)]
    steam: Option<LauncherCfg>,
    #[serde(default)]
    lutris: Option<LauncherCfg>,
    /// Files merged in through `include`, so their errors keep their own file path.
    #[serde(skip)]
    included: Vec<PathBuf>,
    /// The file each template came from, to catch two files defining the same one.
    #[serde(skip)]
    template_files: BTreeMap<String, PathBuf>,
}

/// `[defaults]` became `[templates.default]`. An old config fails to load rather than
/// quietly losing every game's workdir.
fn removed_defaults<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom("[defaults] was removed: rename it to [templates.default], which every [[game]] gets"))
}

/// Everything the window needs from the config file.
#[derive(Default)]
pub struct Settings {
    pub games: Vec<Game>,
    pub ui: UiCfg,
    pub api: ApiCfg,
    pub playtime: PlaytimeCfg,
    pub keyboard: Keymap,
    pub gamepad: Padmap,
    pub status: Status,
    pub topbar: Topbar,
    /// To build `games` and `topbar` again when a toggle switches a variable.
    pub library: Library,
    /// `[[roms]]` files to ask Hasheous about: it's on, and they haven't been looked
    /// up since they last changed.
    pub unidentified_roms: Vec<PathBuf>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ApiCfg {
    steamgriddb_key: Option<String>,
    igdb_client_id: Option<String>,
    igdb_client_secret: Option<String>,
    /// Name `[[roms]]` files by what Hasheous says they are, see `api_hasheous`.
    pub hasheous: bool,
    /// A self-hosted Hasheous to ask instead of hasheous.org.
    #[serde(deserialize_with = "http_url")]
    hasheous_url: Option<String>,
}

impl ApiCfg {
    /// STEAMGRIDDB_API_KEY from the environment wins over the config file.
    pub fn steamgriddb_key(&self) -> Option<String> {
        secret("STEAMGRIDDB_API_KEY", &self.steamgriddb_key)
    }

    /// The Twitch app's client id and secret, if both are set. IGDB_CLIENT_ID and
    /// IGDB_CLIENT_SECRET from the environment win over the config file.
    pub fn igdb_credentials(&self) -> Option<(String, String)> {
        let id = secret("IGDB_CLIENT_ID", &self.igdb_client_id)?;
        Some((id, secret("IGDB_CLIENT_SECRET", &self.igdb_client_secret)?))
    }

    /// Where Hasheous's API is: under `hasheous_url`, else on hasheous.org. The URL
    /// may be given with or without the `/api/v1`.
    pub fn hasheous_api(&self) -> String {
        let root = self.hasheous_url.as_deref().unwrap_or("https://hasheous.org").trim_end_matches('/');
        format!("{}/api/v1", root.strip_suffix("/api/v1").unwrap_or(root))
    }
}

/// An http:// or https:// URL with a host.
fn http_url<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let url = String::deserialize(d)?;
    let host = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    match host {
        Some(h) if !h.trim_start_matches('/').is_empty() => Ok(Some(url)),
        _ => Err(serde::de::Error::custom(format!("{url:?} isn't an http:// or https:// URL"))),
    }
}

/// `var` from the environment, else the config value. Blank counts as unset.
fn secret(var: &str, configured: &Option<String>) -> Option<String> {
    let clean = |s: String| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    std::env::var(var).ok().and_then(clean).or_else(|| configured.clone().and_then(clean))
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
    /// Open the window fullscreen, same as -f.
    pub fullscreen: bool,
    /// Open the launcher again when a game launched from the window exits.
    pub reopen_after_game: bool,
}

impl Default for UiCfg {
    fn default() -> Self {
        Self {
            card_width: 170.0,
            blur: 8.0,
            single_instance: true,
            start_maximized: false,
            fullscreen: false,
            reopen_after_game: false,
        }
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

/// A command or list of commands to run one after another, for `exec`.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum ExecCfg {
    One(String),
    Many(Vec<String>),
}

impl ExecCfg {
    fn into_vec(self) -> Vec<String> {
        match self {
            ExecCfg::One(s) => vec![s],
            ExecCfg::Many(v) => v,
        }
    }
}

fn exec_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(ExecCfg::deserialize(d)?.into_vec())
}

fn opt_exec_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    Ok(Option::<ExecCfg>::deserialize(d)?.map(ExecCfg::into_vec))
}

/// A `[[game]]`, and what `add_roms` and `add_launchers` make of each ROM and imported game. The keys a template
/// can set are optional here, so "not set" can be told from a value; after
/// `apply_templates`, a missing `exec` is an error and a missing `playtime` is true.
#[derive(Deserialize, Clone)]
struct GameCfg {
    name: String,
    /// The `[templates]` it uses, in order; see `apply_templates`.
    #[serde(default, deserialize_with = "exec_list")]
    template: Vec<String>,
    #[serde(default, deserialize_with = "opt_exec_list")]
    exec: Option<Vec<String>>,
    cover: Option<String>,
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    playtime: Option<bool>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    tags: Vec<String>,
    steam_appid: Option<u64>,
    steamgriddb_id: Option<u64>,
    grid_id: Option<u64>,
    hero_id: Option<u64>,
    igdb_id: Option<u64>,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
    /// Where it came from: written as a `[[game]]`, or made by `add_roms` or `add_launchers`.
    #[serde(skip)]
    origin: Origin,
    /// The table that sets an imported game's own keys, like `[steam] games.620`.
    #[serde(skip)]
    pin: Option<String>,
    /// What the window shows instead of `name`: a ROM's `[[roms]] name`.
    #[serde(skip)]
    title: Option<String>,
    /// A ROM's `[[roms]] igdb_platform`.
    #[serde(skip)]
    igdb_platform: Option<String>,
    /// The included file this game came from; `None` for the top-level config file.
    #[serde(skip)]
    source: Option<PathBuf>,
}

/// Where a `GameCfg` came from.
#[derive(Clone, Copy, Default, PartialEq)]
enum Origin {
    #[default]
    Game,
    /// A `[[roms]]` file, see `add_roms`.
    Rom,
    /// An installed Steam game, see `add_steam`.
    Steam,
    /// An installed Lutris game, see `add_lutris`.
    Lutris,
}

impl Origin {
    /// The config table an imported game comes from.
    fn section(self) -> &'static str {
        match self {
            Origin::Game => "[[game]]",
            Origin::Rom => "[[roms]]",
            Origin::Steam => "[steam]",
            Origin::Lutris => "[lutris]",
        }
    }
}

impl GameCfg {
    fn at(&self, msg: String) -> String {
        match &self.source {
            Some(p) => format!("{}: {msg}", p.display()),
            None => msg,
        }
    }
}



/// One right-click menu entry, from `[[game.right_click]]` or the top-level
/// `[[right_click]]`. Unknown keys are an error, so a key that ended up under the
/// wrong header (everything below `[[game.right_click]]` belongs to it) is caught.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct MenuCfg {
    title: String,
    #[serde(default, deserialize_with = "opt_exec_list")]
    exec: Option<Vec<String>>,
    action: Option<String>,
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    /// On top of the game's variables, for this entry only.
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    launch: bool,
    /// Only here to say what replaced it, see `removed_for`.
    #[serde(default, rename = "for", deserialize_with = "removed_for")]
    _for: (),
}

/// `for = "game"` / `"rom"` on a menu entry became templates. An old config fails to
/// load rather than showing an Uninstall entry on every ROM.
fn removed_for<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom(
        "`for` was removed: move this entry to [[templates.default.right_click]] for every [[game]], \
         or to a template your [[roms]] list for ROMs",
    ))
}

/// A top bar button or menu entry. It runs like a right-click tool, but no game is
/// involved, so it only gets the global `[vars]` and `[env]`.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ToolCfg {
    pub title: String,
    #[serde(deserialize_with = "exec_list")]
    exec: Vec<String>,
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

/// A right-click menu entry (or top bar tool) with its paths expanded, and the game's
/// workdir and env already folded in.
pub struct MenuEntry {
    pub title: String,
    pub action: MenuAction,
    pub workdir: Option<PathBuf>,
    pub env: Env,
    /// Behave like launching the game: close rsgl and track playtime.
    pub launch: bool,
}

#[derive(Debug, PartialEq)]
pub enum MenuAction {
    Exec(Vec<String>),
    /// Open `workdir` in the file manager.
    OpenFolder,
}

/// What the top bar's toggles set, by variable name: a value, or None to unset it.
pub type Toggles = BTreeMap<String, Option<String>>;

/// What a game, menu entry or tool adds to rsgl's environment: `[env]`, with its own on top.
#[derive(Clone, Default)]
pub struct Env {
    pub all: HashMap<String, String>,
    /// The names in `all` it sets itself, rather than getting from `[env]`.
    own: BTreeSet<String>,
}

impl Env {
    fn set_own(&mut self, name: &str, value: String) {
        self.all.insert(name.to_string(), value);
        self.own.insert(name.to_string());
    }

    /// What it does to `name` while the top bar's toggles are `toggles`: Some(value) sets
    /// it, Some(None) unsets it, None leaves rsgl's own. A toggle replaces `[env]`'s value,
    /// but not one set more specifically.
    pub fn get<'a>(&'a self, name: &str, toggles: &'a Toggles) -> Option<Option<&'a str>> {
        match toggles.get(name) {
            Some(t) if !self.own.contains(name) => Some(t.as_deref()),
            _ => self.all.get(name).map(|v| Some(v.as_str())),
        }
    }

    /// Every change it makes to rsgl's environment with `toggles`, as `get` says.
    pub fn with<'a>(&'a self, toggles: &'a Toggles) -> Vec<(&'a str, Option<&'a str>)> {
        let names: BTreeSet<&str> = self.all.keys().chain(toggles.keys()).map(String::as_str).collect();
        names.into_iter().filter_map(|k| Some((k, self.get(k, toggles)?))).collect()
    }
}

/// A game with all paths expanded and the cover resolved.
pub struct Game {
    /// Who the game is: playtime, cached art and --launch go by this. Variables are
    /// filled in, except `{year}`, which is left out so the name doesn't change once
    /// the year is known.
    pub name: String,
    /// What the window shows: the name (or a ROM's `[[roms]] name`) with `{year}` and
    /// `{igdb_name}` filled in, once they're known.
    pub title: String,
    /// The title uses IGDB's `{year}` or `{igdb_name}` (the game has no variable of its
    /// own by that name).
    uses_igdb: bool,
    /// What IGDB says about the game, once it's been looked up.
    pub meta: Option<Meta>,
    /// The title as written and the game's variables, to fill in IGDB's data later.
    title_template: String,
    pub vars: Vars,
    pub exec: Vec<String>,
    pub cover: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
    pub env: Env,
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
    /// SteamGridDB grid to use as the cover instead of the best-scored one.
    pub grid_id: Option<u64>,
    /// SteamGridDB hero to use for the background instead of the best-scored one.
    pub hero_id: Option<u64>,
    /// Look the game up on IGDB by this id instead of searching its name.
    pub igdb_id: Option<u64>,
    /// Search IGDB only among games on this platform (a slug). `None` with an
    /// `igdb_id`, since there's no search then.
    pub igdb_platform: Option<String>,
    /// Made from a `[[roms]]` file, see `art_search`.
    rom: bool,
}

impl Game {
    /// Keeps what IGDB found and puts it into the title.
    pub fn set_meta(&mut self, meta: Meta) {
        self.meta = Some(meta);
        self.title = self.shown_title();
    }

    /// The title with what IGDB has said so far: a blank `{year}`, and `{igdb_name}`
    /// is the game's name until IGDB has one.
    fn shown_title(&self) -> String {
        let meta = self.meta.as_ref();
        let year = meta.and_then(|m| m.year).map(|y| y.to_string()).unwrap_or_default();
        let igdb_name = meta.and_then(|m| m.name.as_deref()).unwrap_or(&self.name);
        let title = self
            .vars
            .with_defaults([(YEAR, year.as_str()), (IGDB_NAME, igdb_name)])
            .expand(&self.title_template)
            .expect("the title expanded when the config loaded, and IGDB's values add no braces");
        if self.uses_igdb {
            tidy(&title)
        } else {
            title
        }
    }

    /// Whether something shown needs the game's IGDB data: it hasn't been looked up
    /// yet, and its title uses it or `status_uses_igdb` (see `Status::uses_igdb`).
    pub fn needs_meta(&self, status_uses_igdb: bool) -> bool {
        self.meta.is_none() && (self.uses_igdb || status_uses_igdb)
    }

    /// What to search SteamGridDB for instead of the name. A ROM's name is only its
    /// file's, so IGDB's name for it finds art more often ("1080 Snowboarding" is
    /// "1080° Snowboarding"). A `[[game]]`'s name was written to be found, so it's kept.
    pub fn art_search(&self) -> Option<&str> {
        let igdb = self.meta.as_ref()?.name.as_deref()?;
        (self.rom && !igdb.eq_ignore_ascii_case(&self.name)).then_some(igdb)
    }

    /// False when the game's workdir is set but doesn't exist, which usually means the
    /// game isn't installed (or was uninstalled). A game without a workdir counts as
    /// installed, since there's nothing to check.
    pub fn installed(&self) -> bool {
        self.workdir.as_ref().is_none_or(|d| d.is_dir())
    }
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

/// The variable that holds the game's release year, available in `name` and the status bar.
pub const YEAR: &str = "year";

/// IGDB's name for the game, available in a `[[roms]] name`. Not in a `[[game]]`'s: its
/// name is what IGDB is searched for, and who the game is, so it can't come from IGDB.
const IGDB_NAME: &str = "igdb_name";

/// The variable that holds the game's `exec` as written, available in its menu entries.
const GAME_EXEC: &str = "exec";

/// What a game's menu entries also get: its name (without `{year}`), the cover and hero
/// files it has (blank until there's one) and its workdir, see `build_game`.
const GAME_NAME: &str = "name";
const GAME_GRID: &str = "grid";
const GAME_HERO: &str = "hero";
const GAME_WORKDIR: &str = "workdir";




/// Config locations in priority order:
/// 1. rsgl.toml next to the executable, for a portable folder
/// 2. $XDG_CONFIG_HOME/rsgl/rsgl.toml (usually ~/.config/rsgl/rsgl.toml;
///    on Windows %APPDATA%\rsgl\rsgl.toml)
/// 3. ~/.rsgl.toml
fn candidates() -> Vec<PathBuf> {
    let beside_exe = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("rsgl.toml")));
    beside_exe.into_iter().chain(home_candidates()).collect()
}

/// The default locations in the user's home. The example config is written to the
/// first one, never next to the executable, which may be read-only (/usr/bin).
fn home_candidates() -> Vec<PathBuf> {
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

/// Reads and parses the config at `path` and every file it includes. Keys that nothing
/// reads (a typo like `[evn]`, or a table an older rsgl doesn't know) aren't an error;
/// they come back as warnings.
fn read(path: &Path) -> Result<(Config, Vec<String>), String> {
    let mut chain = Vec::new();
    let (_, cfg, unknown) = read_file(path, true, &mut chain)?;
    Ok((cfg, unknown))
}

/// Reads `path` and recursively merges any files in its `include` list. `chain` holds
/// the `(canonical, as_given)` paths being read right now, outermost first, to catch
/// include loops. Settings in the including file (or a later include) win over earlier
/// ones; `[[game]]`, `[[roms]]` and `[[right_click]]` add up in order.
fn read_file(
    path: &Path,
    is_root: bool,
    chain: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(toml::Table, Config, Vec<String>), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if chain.iter().any(|(c, _)| *c == canon) {
        let names: Vec<_> = chain
            .iter()
            .map(|(_, p)| p.display().to_string())
            .chain([path.display().to_string()])
            .collect();
        return Err(format!("files include each other in a circle: {}", names.join(" -> ")));
    }
    chain.push((canon, path.to_path_buf()));

    let (mut cfg, unknown) = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let unknown: Vec<String> = match is_root {
        true => unknown,
        false => unknown.into_iter().map(|w| format!("{}: {w}", path.display())).collect(),
    };
    let folder = config_folder(path)?;
    let mut own: toml::Table = toml::from_str(&text).expect("already parsed as Config");
    for key in ["include", "game", "roms", "right_click"] {
        own.remove(key);
    }
    if let Some(toml::Value::Table(paths)) = own.get_mut("paths") {
        for key in ["cache", "playtime"] {
            if let Some(toml::Value::String(s)) = paths.get_mut(key) {
                if !s.is_empty() {
                    *s = folder.join(expand(s)).to_string_lossy().into_owned();
                }
            }
        }
    }
    if !is_root {
        for g in &mut cfg.games {
            g.source = Some(path.to_path_buf());
        }
        for r in &mut cfg.roms {
            r.source = Some(path.to_path_buf());
        }
    }
    cfg.template_files = cfg.templates.keys().map(|name| (name.clone(), path.to_path_buf())).collect();
    if cfg.include.is_empty() {
        chain.pop();
        return Ok((own, cfg, unknown));
    }

    let mut table = toml::Table::new();
    let mut right_click = Vec::new();
    let mut games = Vec::new();
    let mut roms = Vec::new();
    let mut included = Vec::new();
    let mut all_unknown = Vec::new();
    // merging two files' [templates.x] key by key would replace a right_click list rather
    // than add to it, so a name is only allowed once
    let mut template_files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut add_templates = |files: BTreeMap<String, PathBuf>| {
        for (name, file) in files {
            if let Some(first) = template_files.get(&name) {
                return Err(format!(
                    "[templates.{name}] is defined in both {} and {}; give one of them another name",
                    first.display(),
                    file.display()
                ));
            }
            template_files.insert(name, file);
        }
        Ok(())
    };
    for inc in &cfg.include {
        if inc.trim().is_empty() {
            return Err(format!("{}: include: path is empty", path.display()));
        }
        let inc_path = folder.join(expand(inc));
        let (inc_table, inc_cfg, inc_unknown) = read_file(&inc_path, false, chain)?;
        add_templates(inc_cfg.template_files)?;
        merge_tables(&mut table, inc_table);
        right_click.extend(inc_cfg.right_click);
        games.extend(inc_cfg.games);
        roms.extend(inc_cfg.roms);
        included.push(inc_path);
        included.extend(inc_cfg.included);
        all_unknown.extend(inc_unknown);
    }
    add_templates(std::mem::take(&mut cfg.template_files))?;
    merge_tables(&mut table, own);
    right_click.extend(cfg.right_click);
    games.extend(cfg.games);
    roms.extend(cfg.roms);
    all_unknown.extend(unknown);

    let mut merged: Config = toml::Value::Table(table.clone())
        .try_into()
        .map_err(|e: toml::de::Error| format!("{}: {e}", path.display()))?;
    merged.right_click = right_click;
    merged.games = games;
    merged.roms = roms;
    merged.included = included;
    merged.template_files = template_files;
    chain.pop();
    Ok((table, merged, all_unknown))
}

fn merge_tables(base: &mut toml::Table, overlay: toml::Table) {
    for (k, v) in overlay {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge_tables(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

fn parse(text: &str) -> Result<(Config, Vec<String>), toml::de::Error> {
    let mut unknown = Vec::new();
    let cfg = serde_ignored::deserialize(toml::Deserializer::parse(text)?, |key| {
        unknown.push(format!("unknown key `{key}`, ignored"));
    })?;
    Ok((cfg, unknown))
}

/// Prefix `msg` with `root` unless it already starts with one of the `included` file paths.
fn at_file(root: &Path, included: &[PathBuf], msg: String) -> String {
    if included.iter().any(|p| msg.starts_with(&format!("{}: ", p.display()))) {
        msg
    } else {
        format!("{}: {msg}", root.display())
    }
}

/// `read`, with the warnings printed to the console.
fn read_and_warn(path: &Path) -> Result<Config, String> {
    let (cfg, unknown) = read(path)?;
    for w in unknown {
        eprintln!("rsgl: {}", at_file(path, &cfg.included, w));
    }
    Ok(cfg)
}

/// Finds and parses the config. If no file exists, writes an example to the
/// first location and returns an error telling the user about it.
pub fn load() -> Result<Settings, String> {
    let Some(path) = locate()? else {
        let candidates = home_candidates();
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
    load_from(&path)
}

/// Like `load`, for commands that run without a window: a missing config is just
/// an error, and nothing is written to disk.
pub fn load_existing() -> Result<Settings, String> {
    let path = locate()?.ok_or_else(no_config)?;
    load_from(&path)
}

/// Reads the config at `path`, switches to its [paths], scans the [[roms]] folders,
/// then builds the settings.
fn load_from(path: &Path) -> Result<Settings, String> {
    let cfg = read_and_warn(path)?;
    let included = cfg.included.clone();
    let at = |e| at_file(path, &included, e);
    *PATHS.write().unwrap() = cfg.paths.resolve(&config_folder(path)?).map_err(at)?;
    // after the switch, so the Hasheous and IGDB data come from the configured cache
    load_cfg(cfg, &at)
}

fn load_cfg(mut cfg: Config, at: &dyn Fn(String) -> String) -> Result<Settings, String> {
    let known = cfg.api.hasheous.then(api_hasheous::cached);
    let scan = add_roms(&mut cfg, known.as_ref()).map_err(at)?;
    let imported = add_launchers(&mut cfg);
    if let Some(e) = imported.errors.into_iter().next() {
        return Err(at(e));
    }
    for w in scan.warnings.into_iter().chain(imported.warnings) {
        eprintln!("rsgl: {}", at(w));
    }
    let settings = build(cfg, api_igdb::cached_meta()).map_err(at)?;
    Ok(Settings { unidentified_roms: scan.unidentified, ..settings })
}

fn no_config() -> String {
    let places: Vec<String> = candidates().iter().map(|p| p.display().to_string()).collect();
    format!("no config file found (looked for {})", places.join(", "))
}


/// Resolves one menu entry for a game. `owner` names where the entry came from, for errors,
/// and `game_exec` is the game's unexpanded `exec` list for `{exec}` (empty if `vars`
/// already defines `exec`, or for a top bar tool).
fn menu_entry(
    m: &MenuCfg,
    owner: &str,
    workdir: Option<&Path>,
    env: &Env,
    vars: &Vars,
    game_exec: &[String],
) -> Result<MenuEntry, String> {
    let err = |msg: String| format!("{owner}, entry {:?}: {msg}", m.title);
    let vars = vars.with(&m.vars).map_err(|e| err(format!("vars: {e}")))?;
    let sub = |field: &str, s: &str| vars.expand(s).map_err(|e| err(format!("{field}: {e}")));

    let action = match (&m.exec, m.action.as_deref()) {
        (Some(cmds), None) => {
            if cmds.is_empty() {
                return Err(err("exec is empty".into()));
            }
            let mut exec = Vec::new();
            for s in cmds {
                if s == "{exec}" && !game_exec.is_empty() && !m.vars.contains_key(GAME_EXEC) {
                    for g_cmd in game_exec {
                        exec.push(expand(&sub("exec", g_cmd)?));
                    }
                } else {
                    exec.push(expand(&sub("exec", s)?));
                }
            }
            MenuAction::Exec(exec)
        }
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
        merged.set_own(k, expand(&sub(&format!("env {k}"), v)?));
    }

    Ok(MenuEntry { title: m.title.clone(), action, workdir, env: merged, launch: m.launch })
}

/// The global `[env]` with `vars` filled in. `who` says what it's for, for errors.
pub fn global_env(env: &BTreeMap<String, String>, vars: &Vars, who: &str) -> Result<Env, String> {
    let mut out = Env::default();
    for (k, v) in env {
        let v = vars.expand(v).map_err(|e| format!("[env] (for {who}), {k}: {e}"))?;
        out.all.insert(k.clone(), expand(&v));
    }
    Ok(out)
}

/// Resolves a top bar button or menu entry with the global `vars` and `env`.
/// `owner` says where it is, for errors.
pub fn tool(t: ToolCfg, owner: &str, vars: &Vars, env: &BTreeMap<String, String>) -> Result<MenuEntry, String> {
    // [env] is fine for every game, so a variable missing here is one only games have
    let env = global_env(env, vars, owner).map_err(|e| format!("{e} (the top bar only has [vars], not a game's own)"))?;
    let m = MenuCfg {
        title: t.title,
        exec: Some(t.exec),
        action: None,
        workdir: t.workdir,
        env: t.env,
        vars: BTreeMap::new(),
        launch: false,
        _for: (),
    };
    menu_entry(&m, owner, None, &env, vars, &[])
}

/// What every game starts from: the global `[vars]` (with the top bar's toggles in
/// them), `[env]` and `[[right_click]]`, and what IGDB has said about games so far.
struct Shared<'a> {
    vars: Vars,
    /// The variables top bar toggles switch, with their on and off values.
    var_toggles: Vec<(String, [String; 2])>,
    env: &'a BTreeMap<String, String>,
    menu: &'a [MenuCfg],
    meta: &'a BTreeMap<String, Meta>,
}

/// What the games and the top bar are built from. It's kept so they can be built again
/// when a top bar toggle switches one of the `[vars]`: those are filled in here, unlike
/// `[env]`, which a toggle changes when something is launched.
#[derive(Default)]
pub struct Library {
    vars: BTreeMap<String, toml::Value>,
    env: BTreeMap<String, String>,
    right_click: Vec<MenuCfg>,
    /// The games that aren't hidden, with their templates applied.
    games: Vec<GameCfg>,
    topbar: TopbarCfg,
    /// See `var_names`.
    names: BTreeSet<String>,
    meta: BTreeMap<String, Meta>,
}

impl Library {
    /// The games and the top bar, with the top bar's toggles and sort like those of
    /// `toggles` (a top bar from this library), or as the config starts them.
    pub fn build(&self, toggles: Option<&Topbar>) -> Result<(Vec<Game>, Topbar), String> {
        let vars = Vars::default().with(&self.vars).map_err(|e| format!("[vars]: {e}"))?;
        let check = Checker::new(&self.names);
        let (topbar, vars) = topbar_and_vars(&self.topbar, &check, &vars, &self.env, toggles)?;
        let shared = Shared {
            vars,
            var_toggles: topbar.var_toggles(),
            env: &self.env,
            menu: &self.right_click,
            meta: &self.meta,
        };
        let mut games = self
            .games
            .iter()
            .map(|g| build_game(g.clone(), &shared).map_err(|e| g.at(e)))
            .collect::<Result<Vec<_>, _>>()?;
        games.sort_by_cached_key(|g| g.name.to_lowercase());
        Ok((games, topbar))
    }
}

/// The top bar, and `vars` with the values its toggles give variables. The toggles and
/// sort are like those of `states`, or as they start.
fn topbar_and_vars(
    cfg: &TopbarCfg,
    check: &Checker,
    vars: &Vars,
    env: &BTreeMap<String, String>,
    states: Option<&Topbar>,
) -> Result<(Topbar, Vars), String> {
    let new = |vars: &Vars| Topbar::new(cfg.clone(), check, vars, env).map_err(|e| format!("[topbar]: {e}"));
    let start;
    let states = match states {
        Some(s) => s,
        None => {
            start = new(vars)?;
            &start
        }
    };
    // built again, so buttons and menus see the toggled values too
    let vars = vars.with_values(states.var_values());
    let mut topbar = new(&vars)?;
    topbar.copy_state(states);
    Ok((topbar, vars))
}

/// Every variable name in `[vars]`, some template's or some game's `vars`, for checking
/// `[status]` and `[topbar]`.
fn var_names(cfg: &Config) -> BTreeSet<String> {
    let templates = cfg.templates.values().flat_map(|t| t.vars.keys());
    cfg.vars.keys().chain(templates).chain(cfg.games.iter().flat_map(|g| g.vars.keys())).cloned().collect()
}

/// Turns the parsed file into games: applies templates, fills in variables, expands
/// paths, finds covers, resolves menus. Hidden games are skipped without being checked.
/// `meta` is the cached IGDB metadata, by game name.
fn build(cfg: Config, meta: BTreeMap<String, Meta>) -> Result<Settings, String> {
    let names = var_names(&cfg);
    let status = Status::new(cfg.status, &Checker::new(&names)).map_err(|e| format!("[status]: {e}"))?;
    let games = cfg
        .games
        .iter()
        .filter(|g| !g.hidden)
        .map(|g| game_with_templates(g, &cfg.templates).map_err(|e| g.at(e)))
        .collect::<Result<_, _>>()?;
    let library = Library {
        vars: cfg.vars,
        env: cfg.env,
        right_click: cfg.right_click,
        games,
        topbar: cfg.topbar,
        names,
        meta,
    };
    let (games, topbar) = library.build(None)?;
    Ok(Settings {
        games,
        ui: cfg.ui,
        api: cfg.api,
        playtime: cfg.playtime,
        keyboard: cfg.keyboard,
        gamepad: cfg.gamepad,
        status,
        topbar,
        library,
        unidentified_roms: Vec::new(),
    })
}

fn build_game(g: GameCfg, shared: &Shared) -> Result<Game, String> {
    let who = format!("game {:?}", g.name);
    let vars = shared.vars.with(&g.vars).map_err(|e| format!("{who}, vars: {e}"))?;
    let sub = |field: &str, s: &str| vars.expand(s).map_err(|e| format!("{who}, {field}: {e}"));

    // {year} only exists in the name, and a game's own `year` variable wins over IGDB's.
    // Expanding with two different years tells whether the name uses it at all.
    let name_with = |year: &str| vars.with_defaults([(YEAR, year)]).expand(&g.name).map_err(|e| format!("{who}, name: {e}"));
    if !vars.has(IGDB_NAME) && name_with("").is_err() && vars.with_defaults([(YEAR, ""), (IGDB_NAME, "")]).expand(&g.name).is_ok() {
        return Err(format!("{who}, name: {{{IGDB_NAME}}} only works in a [[roms]] name"));
    }
    let blank = name_with("")?;
    let uses_year = name_with("0")? != blank;
    let name = if uses_year { tidy(&blank) } else { blank };
    // playtime, art and --launch go by the name, and the games keep their order, so a
    // toggle mustn't change it. A game's own value for the variable wins over the toggle
    for (var, values) in &shared.var_toggles {
        let [on, off] = values.each_ref().map(|v| {
            let vars = shared.vars.with_values([(var.as_str(), v.as_str())]).with(&g.vars).ok()?;
            vars.with_defaults([(YEAR, "")]).expand(&g.name).ok()
        });
        if on != off {
            return Err(format!("{who}, name: can't use {{{var}}}, a top bar toggle switches it"));
        }
    }
    // a ROM's title can come from `[[roms]] name`; a game's is its name. Either may use
    // IGDB's data, and expanding with two different values tells whether it does
    let title_template = g.title.clone().unwrap_or_else(|| g.name.clone());
    let title_with = |year: &str, igdb_name: &str| {
        vars.with_defaults([(YEAR, year), (IGDB_NAME, igdb_name)]).expand(&title_template).map_err(|e| format!("{who}, name: {e}"))
    };
    let uses_igdb = title_with("0", "x")? != title_with("", "")?;

    // data cached under another igdb_id or igdb_platform (or by name, before one was set)
    // is stale
    let igdb_platform = g.igdb_platform.filter(|_| g.igdb_id.is_none());
    let meta = shared
        .meta
        .get(&name)
        .filter(|m| m.igdb_id == g.igdb_id && m.igdb_platform == igdb_platform)
        .cloned();

    let g_exec = match g.exec {
        None => return Err(format!("{who}: needs an exec, here or in a template")),
        Some(e) if e.is_empty() => return Err(format!("{who}: exec is empty")),
        Some(e) => e,
    };
    let exec = g_exec.iter().map(|s| Ok(expand(&sub("exec", s)?))).collect::<Result<Vec<_>, String>>()?;
    // one that comes out empty (workdir = "" over a template's) is none
    let workdir = g
        .workdir
        .as_ref()
        .map(|w| sub("workdir", w))
        .transpose()?
        .filter(|w| !w.is_empty())
        .map(|w| PathBuf::from(expand(&w)));

    // the global [env] first, then the game's own on top
    let mut env = global_env(shared.env, &vars, &who)?;
    for (k, v) in &g.env {
        env.set_own(k, expand(&sub(&format!("env {k}"), v)?));
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
        .or_else(|| find_cover(&name));

    // shared entries first, then the game's own; both see this game's variables, and
    // {exec} is the game's exec as written, so an entry's own vars reach into it.
    // A variable of the game's own with one of these names wins, like for {year}
    let text = |p: Option<&Path>| p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let (grid, hero, wd) = (text(cover.as_deref()), text(find_hero(&name).as_deref()), text(workdir.as_deref()));
    let game_exec = if vars.has(GAME_EXEC) { &[][..] } else { &g_exec };
    let menu_vars = vars.with_template_default(GAME_EXEC, &g_exec.join("; ")).with_defaults([
        (GAME_NAME, name.as_str()),
        (GAME_GRID, &grid),
        (GAME_HERO, &hero),
        (GAME_WORKDIR, &wd),
    ]);
    let mut menu = Vec::new();
    let shared_owner = format!("[[right_click]] (for {who})");
    for m in shared.menu {
        menu.push(menu_entry(m, &shared_owner, workdir.as_deref(), &env, &menu_vars, game_exec)?);
    }
    // its templates' entries, then its own
    for m in &g.right_click {
        menu.push(menu_entry(m, &who, workdir.as_deref(), &env, &menu_vars, game_exec)?);
    }

    let mut game = Game {
        title: String::new(),
        uses_igdb,
        meta: None,
        title_template,
        vars,
        name,
        exec,
        cover,
        workdir,
        env,
        playtime: g.playtime.unwrap_or(true),
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
        grid_id: g.grid_id,
        hero_id: g.hero_id,
        igdb_id: g.igdb_id,
        igdb_platform,
        rom: g.origin == Origin::Rom,
    };
    game.meta = meta;
    game.title = game.shown_title();
    Ok(game)
}

/// The games in config `text`, for tests elsewhere.
#[cfg(test)]
pub fn parse_games(text: &str) -> Result<Vec<Game>, String> {
    let mut cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    add_roms(&mut cfg, None)?;
    if let Some(e) = add_launchers(&mut cfg).errors.into_iter().next() {
        return Err(e);
    }
    build(cfg, BTreeMap::new()).map(|s| s.games)
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
    let (cfg, unknown) = read(&path)?;
    let paths = cfg.paths.resolve(&config_folder(&path)?);
    let mut report = check_config(cfg);
    report.errors.extend(paths.err());
    report.warnings.extend(unknown);
    report.path = path;
    Ok(report)
}

fn check_config(mut cfg: Config) -> Report {
    let (mut errors, mut warnings) = (Vec::new(), Vec::new());
    // without Hasheous's names: what's checked is the config, not what Hasheous said
    match add_roms(&mut cfg, None) {
        Ok(scan) => warnings = scan.warnings,
        Err(e) => errors.push(e),
    }
    let imported = add_launchers(&mut cfg);
    warnings.extend(imported.warnings);
    errors.extend(imported.errors);
    let mut report = Report {
        path: PathBuf::new(),
        games: cfg.games.len(),
        hidden: cfg.games.iter().filter(|g| g.hidden).count(),
        errors,
        warnings,
    };
    let check = Checker::new(&var_names(&cfg));
    let note = |g: &GameCfg| if g.hidden { " (hidden game)" } else { "" };
    let mut games = Vec::new();
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for g in std::mem::take(&mut cfg.games) {
        names.entry(g.name.to_lowercase()).or_default().push(g.name.clone());
        match game_with_templates(&g, &cfg.templates) {
            Ok(g) => games.push(g),
            Err(e) => report.errors.push(g.at(format!("{e}{}", note(&g)))),
        }
    }
    if let Err(e) = Status::new(std::mem::take(&mut cfg.status), &check) {
        report.errors.push(format!("[status]: {e}"));
    }
    let vars = match Vars::default().with(&cfg.vars) {
        Ok(v) => v,
        Err(e) => {
            report.errors.push(format!("[vars]: {e}"));
            return report;
        }
    };
    let (vars, var_toggles) = match topbar_and_vars(&cfg.topbar, &check, &vars, &cfg.env, None) {
        Ok((topbar, vars)) => {
            report.warnings.extend(unmatched_filter_tags(&topbar, &games));
            (vars, topbar.var_toggles())
        }
        Err(e) => {
            report.errors.push(e);
            (vars, Vec::new())
        }
    };
    // IGDB metadata only changes titles and the status bar, which there's nothing to check about
    let meta = BTreeMap::new();
    let shared = Shared { vars, var_toggles, env: &cfg.env, menu: &cfg.right_click, meta: &meta };

    for g in games {
        let note = note(&g);

        let game = match build_game(g.clone(), &shared) {
            Ok(game) => game,
            Err(e) => {
                report.errors.push(g.at(format!("{e}{note}")));
                continue;
            }
        };
        // launching fails with the same message when the folder is missing
        if let Some(wd) = game.workdir.as_ref().filter(|d| !d.is_dir()) {
            report.warnings.push(g.at(format!("game {:?}: the workdir {} doesn't exist{note}", game.name, wd.display())));
        }
        for e in &game.menu {
            // an entry that just uses the game's workdir is covered by the line above
            if let Some(wd) = e.workdir.as_ref().filter(|d| e.workdir != game.workdir && !d.is_dir()) {
                report.warnings.push(g.at(format!(
                    "game {:?}, entry {:?}: the workdir {} doesn't exist{note}",
                    game.name,
                    e.title,
                    wd.display()
                )));
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

/// A warning for each `t:` tag in the top bar's filters that no game that isn't hidden
/// has, which is likely a typo: that filter always shows an empty grid. `g:` can't be
/// checked, since genres come from IGDB.
fn unmatched_filter_tags(topbar: &Topbar, games: &[GameCfg]) -> Vec<String> {
    let Some(filter) = topbar.filter_widget() else { return Vec::new() };
    let tags: Vec<String> = games.iter().filter(|g| !g.hidden).flat_map(|g| &g.tags).map(|t| t.trim().to_lowercase()).collect();
    let mut warnings = Vec::new();
    for o in &filter.options {
        // a tag matches from its start, like in the search box
        for t in o.query.tags.iter().filter(|t| !tags.iter().any(|tag| tag.starts_with(t.as_str()))) {
            warnings.push(format!("[topbar]: filter {:?}: no game has a tag t:{t} matches", o.title));
        }
    }
    warnings
}

/// Which of `names` the command line means by `query`: the one that's exactly it (any
/// case), or the one containing it if only one does. Otherwise the error has every name
/// containing it, so none or several.
pub fn pick<'a>(names: &[&'a str], query: &str) -> Result<usize, Vec<&'a str>> {
    let q = query.to_lowercase();
    if let Some(k) = names.iter().position(|n| n.to_lowercase() == q) {
        return Ok(k);
    }
    let hits: Vec<usize> = (0..names.len()).filter(|&k| names[k].to_lowercase().contains(&q)).collect();
    match hits.as_slice() {
        &[k] => Ok(k),
        many => Err(many.iter().map(|&k| names[k]).collect()),
    }
}

/// The game for `--show` and `--launch`, see `pick`.
pub fn find<'a>(games: &'a [Game], query: &str) -> Result<&'a Game, String> {
    let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
    match pick(&names, query) {
        Ok(k) => Ok(&games[k]),
        Err(none) if none.is_empty() => Err(format!("no game matches {query:?} (hidden games aren't loaded)")),
        Err(many) => Err(format!("{query:?} matches {} games: {}", many.len(), many.join(", "))),
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

    let mut out = format!("{}\n  exec:    {}\n  workdir: {}\n", g.name, g.exec.join("\n           "), dir(&g.workdir));
    if g.env.all.is_empty() {
        out += "  env:     (none)\n";
    } else {
        out += "  env:\n";
        out += &env_lines(&sorted(&g.env.all), "    ");
    }
    if g.menu.is_empty() {
        out += "  menu:    (none, right-click opens the workdir)\n";
        return out;
    }
    out += "  menu:\n";
    for (i, e) in g.menu.iter().enumerate() {
        let what = match &e.action {
            MenuAction::Exec(cmds) => format!("exec: {}", cmds.join("\n             ")),
            MenuAction::OpenFolder => "action: open_folder".to_string(),
        };
        let launch = if e.launch { " (launch)" } else { "" };
        out += &format!("    {}. {}{launch}\n       {what}\n       workdir: {}\n", i + 1, e.title, dir(&e.workdir));
        // only what the entry adds to the game's env
        let extra: HashMap<_, _> = e.env.all.iter().filter(|(k, v)| g.env.all.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect();
        if !extra.is_empty() {
            out += "       env:\n";
            out += &env_lines(&sorted(&extra), "         ");
        }
    }
    out
}
