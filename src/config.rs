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
    sync::{OnceLock, RwLock},
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
# open the window fullscreen, like -f
fullscreen = false
# open the launcher again when a game launched from it exits
reopen_after_game = false

[playtime]
enabled = true
# save the current session every N minutes, 0 only saves when the game exits
checkpoint_minutes = 5

# Where the cache (covers, heroes, backgrounds, IGDB data) and the playtime file
# go. A relative path is taken from this file's folder, so rsgl, its config and
# its data can live together in one portable folder.
# [paths]
# cache = "./cache"                            # default ~/.cache/rsgl
# playtime = "./playtime.toml"                 # default ~/.local/share/rsgl/playtime.toml

[api]
# steamgriddb_key = "your-key-here"
# IGDB metadata, from a Twitch app (dev.twitch.tv/console); fills in {year} and {genres}
# igdb_client_id = "your-client-id"
# igdb_client_secret = "your-client-secret"
# identify [[roms]] files by their contents on hasheous.org, for proper names and
# IGDB and SteamGridDB ids; reads every ROM once to hash it
# hasheous = false
# hasheous_url = "http://nas:5000"             # a self-hosted Hasheous, default hasheous.org

# The status bar under the grid. Each part is a text with variables: the game's
# {title}, {name}, {year}, {genres}, {playtime}, {last_played}, {not_installed}
# and {tags}, the {shown} and {total} game counts, and your own [vars]. A part
# whose variables are all blank is left out, separator and all. This is the default:
# [status]
# separator = " · "
# left = [
#   { text = "{title}", bold = true },
#   { text = "{playtime} · last played {last_played}" },
#   { text = "{not_installed}" },
#   { text = "{tags}" },
# ]
# right = [{ text = "{shown} games" }]
# {env:NAME} works in parts too: the value the selected game would launch with.

# The bar above the grid: widgets on the left, in the center and on the right.
# A label is a text like a [status] part; the search bar's placeholder replaces
# the key hint, and its autocomplete suggests names, tags and genres (Tab takes it). Buttons and menu entries run a tool, with [vars] and [env] but
# not a game's own. A toggle sets an [env] variable to on or off (no off: unset)
# for whatever is launched next, or with var, one of the [vars] (no off: blank).
# A sort orders the grid. A filter narrows it to a search on top of what's typed,
# switched with its < > arrows, Ctrl+PageUp/PageDown or LB/RB, and "" shows every
# game. Leaving out center puts the search there. This is the default:
# [topbar]
# left = []
# center = [{ type = "search" }]
# right = []
# For example:
# left = [{ type = "label", text = "MangoHud {env:MANGOHUD}", bold = true }]
# center = [{ type = "search", placeholder = "Find something to play…", autocomplete = ["names", "tags", "genres"] }]
# right = [
#   { type = "button", title = "Steam", exec = "steam -bigpicture" },
#   { type = "sort", options = ["name", "last_played", "playtime", "year"], default = "last_played" },
#   { type = "filter", options = [
#     { title = "All", filter = "" },
#     { title = "PC", filter = "t:pc" },
#     { title = "SNES", filter = "t:snes" },
#   ] },
#   { type = "toggle", title = "MangoHud", env = "MANGOHUD", on = "1", off = "0" },
#   { type = "menu", title = "Tools", entries = [{ title = "Lutris", exec = "lutris" }] },
# ]

# Keys for each action. An action you leave out keeps its default (shown here);
# an empty list unbinds it. Modifiers are Ctrl, Alt and Shift. Letters, digits,
# punctuation, Space, Backspace and Delete need Ctrl or Alt, since the search box uses them.
# [keyboard]
# up = ["Up"]                                   # from the top row, into the top bar's buttons
# down = ["Down"]
# left = ["Left"]
# right = ["Right"]
# page_up = ["PageUp"]
# page_down = ["PageDown"]
# launch = ["Enter"]                           # also runs the highlighted menu entry
# menu = ["Shift+F10"]                         # open the selected game's right-click menu
# back = ["Escape"]                            # close the menu, leave the top bar, clear the search, then quit
# reload = ["F5"]
# prev_filter = ["Ctrl+PageUp"]                # switch the top bar's filter
# next_filter = ["Ctrl+PageDown"]

# Gamepad buttons for each action, same rules as [keyboard]. Buttons are named
# after the Xbox layout and matched by position (A is the bottom face button on
# any pad): A B X Y LB RB LT RT View Menu Guide LS RS DPadUp DPadDown DPadLeft
# DPadRight, plus LeftStickUp/Down/Left/Right for tilting the left stick.
# [gamepad]
# up = ["DPadUp", "LeftStickUp"]
# down = ["DPadDown", "LeftStickDown"]
# left = ["DPadLeft", "LeftStickLeft"]
# right = ["DPadRight", "LeftStickRight"]
# page_up = ["LT"]
# page_down = ["RT"]
# launch = ["A"]
# menu = ["Menu"]
# back = ["B"]
# reload = ["View"]
# prev_filter = ["LB"]
# next_filter = ["RB"]

# Variables: {name} is replaced in name, exec, workdir, cover and env values (and
# in menu entries). A game's own `vars` override these, and a variable can use
# others, e.g. gs = "gamescope -w {w} -h {h} -f --". Write {{ for a literal brace.
# [vars]
# games = "~/games"

# Environment variables for every game and menu entry. A game's own `env` wins.
# [env]
# WINEDEBUG = "-all"

# Defaults for every game that leaves the key out. Only workdir so far; it's filled
# in with each game's own vars, and a game's workdir = "" means none.
# [defaults]
# workdir = "{games}/{dir}"

# Right-click menu entries shown for every game, before the game's own.
# `action = "open_folder"` opens the workdir; or give an exec to run.
# [[right_click]]
# title = "Open game folder"
# action = "open_folder"
#
# Entries also get the game's {name}, {grid} and {hero} (the full paths of its
# cover and hero, blank if none) and {workdir}. An entry's own vars win over the
# game's, and {exec} is the game's exec as written, so with
# exec = "{gs} wine game.exe" this runs plain "wine game.exe".
# [[right_click]]
# title = "Launch without gamescope"
# exec = "{exec}"
# vars = { gs = "" }
# launch = true
#
# for = "game" shows an entry only for [[game]] entries, for = "rom" only for
# [[roms]] files, so it can use {rom}.
# [[right_click]]
# title = "Delete ROM"
# for = "rom"
# exec = '''zenity --question --text="Delete {name}?" && rm "{rom}"'''

# Every file in a folder as a game, named after the file without (USA), [!] and
# such. {rom} is the file's full path; workdir defaults to path.
# [[roms]]
# path = "~/roms/snes"
# extensions = ["sfc", "smc", "zip"]
# exec = 'retroarch -L ~/.config/retroarch/cores/snes9x_libretro.so "{rom}"'
# tags = ["snes"]
# name = "{igdb_name} (SNES)"                 # show IGDB's name instead of the file's
# igdb_platform = "snes"                      # only match IGDB games on this platform (its slug)
# files."F-Zero (USA).sfc" = { igdb_id = 1234 } # one file's ids, by file name: igdb_id,
#                                             # steamgriddb_id, grid_id, hero_id
# files."bios.sfc" = { hidden = true }        # a file that isn't a game

[[game]]
name = "Example Game"                          # "Example Game {year}" adds the release year from IGDB
exec = "~/games/example/start.sh"
# cover = "~/.cache/rsgl/covers/example.png"
# workdir = "~/games/example"
# env = { SOME_VAR = "value" }
# playtime = false                             # don't track this game
# hidden = true                                # keep the entry but leave it out of the grid
# tags = ["coop", "lan", "fps"]                # filter with t:coop in the search bar
# steam_appid = 367520                         # pick covers by Steam app id instead of by name
# steamgriddb_id = 1234                        # or by SteamGridDB game id
# grid_id = 664940                             # use this SteamGridDB grid as the cover
# hero_id = 15208                              # and this hero for the background
# igdb_id = 707                                # get {year} and {genres} from this IGDB game instead of by name
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
    #[serde(default)]
    defaults: Defaults,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
    #[serde(default, rename = "game")]
    games: Vec<GameCfg>,
    #[serde(default)]
    roms: Vec<RomsCfg>,
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

#[derive(Deserialize, Clone)]
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
    grid_id: Option<u64>,
    hero_id: Option<u64>,
    igdb_id: Option<u64>,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
    /// Made by `add_roms` from a `[[roms]]` file, not written as a `[[game]]`.
    #[serde(skip)]
    rom: bool,
    /// What the window shows instead of `name`: a ROM's `[[roms]] name`.
    #[serde(skip)]
    title: Option<String>,
    /// A ROM's `[[roms]] igdb_platform`.
    #[serde(skip)]
    igdb_platform: Option<String>,
}

fn yes() -> bool {
    true
}

/// `[[roms]]`: a folder where every file with one of `extensions` becomes a game,
/// see `add_roms`. The other keys are like a game's, and `exec` gets the file as `{rom}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RomsCfg {
    path: String,
    extensions: Vec<String>,
    exec: String,
    /// What the window shows for each file, see `IGDB_NAME`. The game's name (playtime,
    /// art, --launch) stays the file's either way.
    name: Option<String>,
    /// IGDB's slug for the folder's system ("n64"): the IGDB search by name only
    /// finds games on it, so a remake or port with the same name isn't picked.
    igdb_platform: Option<String>,
    /// Defaults to `path`.
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    /// Settings for single files, by file name, see `RomFileCfg`.
    #[serde(default)]
    files: BTreeMap<String, RomFileCfg>,
}

/// `[[roms]] files."name.ext"`: a file's own ids, for when the lookup by name (or
/// Hasheous) picks the wrong game, and `hidden` for files that aren't games (BIOS zips).
/// An id here beats Hasheous's.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RomFileCfg {
    igdb_id: Option<u64>,
    steamgriddb_id: Option<u64>,
    grid_id: Option<u64>,
    hero_id: Option<u64>,
    #[serde(default)]
    hidden: bool,
}

/// The variable that holds a ROM's full path, see `RomsCfg`.
const ROM: &str = "rom";

/// What scanning the `[[roms]]` folders found besides the games, see `add_roms`.
#[derive(Default)]
struct Scan {
    warnings: Vec<String>,
    /// The files `known` has nothing about yet.
    unidentified: Vec<PathBuf>,
}

/// Turns every `[[roms]]` folder's files into games in `cfg.games`. A folder that
/// can't be read (a drive that isn't plugged in) is a warning rather than an error,
/// so the rest of the library still loads. With `known` (Hasheous is on), a file it
/// identified gets the game's name and ids from there instead of from its file name.
fn add_roms(cfg: &mut Config, known: Option<&api_hasheous::Known>) -> Result<Scan, String> {
    let mut scan = Scan::default();
    let warnings = &mut scan.warnings;
    if cfg.roms.is_empty() {
        return Ok(scan);
    }
    let vars = Vars::default().with(&cfg.vars).map_err(|e| format!("[vars]: {e}"))?;
    for r in std::mem::take(&mut cfg.roms) {
        let who = format!("[[roms]] {:?}", r.path);
        if r.extensions.is_empty() {
            return Err(format!("{who}: extensions is empty"));
        }
        if r.igdb_platform.as_ref().is_some_and(|p| p.trim().is_empty()) {
            return Err(format!("{who}: igdb_platform is empty; leave it out to search every platform"));
        }
        let dir = expand(&vars.expand(&r.path).map_err(|e| format!("{who}, path: {e}"))?);
        if dir.is_empty() {
            return Err(format!("{who}: path is empty"));
        }
        let entries = fs::read_dir(&dir).and_then(|entries| entries.map(|e| Ok(e?.path())).collect::<Result<Vec<_>, _>>());
        let mut files = match entries {
            Ok(files) => files,
            Err(e) => {
                warnings.push(format!("{who}: can't read {dir}: {e}"));
                continue;
            }
        };
        files.retain(|f| {
            let ext = f.extension().and_then(|e| e.to_str()).unwrap_or_default();
            f.is_file() && r.extensions.iter().any(|want| want.trim_start_matches('.').eq_ignore_ascii_case(ext))
        });
        files.sort();
        for name in r.files.keys() {
            if !files.iter().any(|f| f.file_name().is_some_and(|n| n == name.as_str())) {
                warnings.push(format!("{who}, files: there's no {name:?} with one of the extensions in {dir}"));
            }
        }
        let none = RomFileCfg::default();
        for file in files {
            let (Some(full), Some(stem)) = (file.to_str(), file.file_stem().and_then(|s| s.to_str())) else {
                warnings.push(format!("{who}: {} isn't valid UTF-8, skipped", file.display()));
                continue;
            };
            let pin = file.file_name().and_then(|n| n.to_str()).and_then(|n| r.files.get(n)).unwrap_or(&none);
            let found = match known.map(|k| k.get(&file)) {
                // a hidden file isn't worth hashing and sending off
                _ if pin.hidden => None,
                Some(None) => {
                    scan.unidentified.push(file.clone());
                    None
                }
                Some(found) => found.flatten(),
                None => None,
            };
            let mut game_vars = r.vars.clone();
            game_vars.insert(ROM.to_string(), toml::Value::String(literal(full)));
            cfg.games.push(GameCfg {
                name: literal(found.map_or(&rom_name(stem), |f| &f.name)),
                exec: r.exec.clone(),
                cover: None,
                workdir: Some(r.workdir.clone().unwrap_or_else(|| literal(&dir))),
                env: r.env.clone(),
                playtime: true,
                hidden: pin.hidden,
                tags: r.tags.clone(),
                steam_appid: None,
                steamgriddb_id: pin.steamgriddb_id.or(found.and_then(|f| f.steamgriddb_id)),
                grid_id: pin.grid_id,
                hero_id: pin.hero_id,
                igdb_id: pin.igdb_id.or(found.and_then(|f| f.igdb_id)),
                vars: game_vars,
                right_click: Vec::new(),
                rom: true,
                title: r.name.clone(),
                igdb_platform: r.igdb_platform.as_ref().map(|p| p.trim().to_string()),
            });
        }
    }
    Ok(scan)
}

/// A ROM's file stem as a game name, without the `(USA)`, `(Rev 1)` and `[!]` groups
/// ROM sets add, so SteamGridDB and IGDB find it by name. A stem that's nothing but
/// groups is kept as it is.
fn rom_name(stem: &str) -> String {
    let mut out = String::new();
    let mut rest = stem;
    while let Some(start) = rest.find(['(', '[']) {
        let close = if rest.as_bytes()[start] == b'(' { ')' } else { ']' };
        let Some(len) = rest[start..].find(close) else { break };
        out.push_str(&rest[..start]);
        out.push(' ');
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    let name = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        stem.to_string()
    } else {
        name
    }
}

/// `[defaults]`: what a game gets for a key it leaves out. Filled in with the game's
/// own variables, so `workdir = "{games}/{path}"` works per game.
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
struct Defaults {
    workdir: Option<String>,
}

/// One right-click menu entry, from `[[game.right_click]]` or the top-level
/// `[[right_click]]`. Unknown keys are an error, so a key that ended up under the
/// wrong header (everything below `[[game.right_click]]` belongs to it) is caught.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct MenuCfg {
    title: String,
    exec: Option<String>,
    action: Option<String>,
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    /// On top of the game's variables, for this entry only.
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    launch: bool,
    /// Which games a top-level entry is shown for; all of them if not set.
    #[serde(rename = "for")]
    only_for: Option<Kind>,
}

/// What a game came from: a `[[game]]`, or a file in a `[[roms]]` folder.
#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Game,
    Rom,
}

/// A top bar button or menu entry. It runs like a right-click tool, but no game is
/// involved, so it only gets the global `[vars]` and `[env]`.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ToolCfg {
    pub title: String,
    exec: String,
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
    Exec(String),
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
    pub exec: String,
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

/// `{env:NAME}` is environment variable NAME, see `Vars::expand_env`.
const ENV_PREFIX: &str = "env:";

/// Letters, digits and `_`: what `{env:NAME}` and a top bar toggle accept.
pub fn is_env_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// What `{name}` accepts: letters, digits, `_` and `-`.
pub fn is_var_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// `text` as a variable's value that's used as it is: its braces are literal.
fn literal(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

/// A name with `{year}` blank or filled in, cleaned up: "Quake ({year})" becomes
/// "Quake" rather than "Quake ()" while the year isn't known.
pub fn tidy(name: &str) -> String {
    name.replace("()", "").replace("[]", "").split_whitespace().collect::<Vec<_>>().join(" ")
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

#[derive(Deserialize, Default)]
#[serde(default)]
struct PathsCfg {
    cache: Option<String>,
    playtime: Option<String>,
}

/// [paths] resolved; `None` is the default location.
#[derive(Debug, PartialEq)]
struct Paths {
    cache: Option<PathBuf>,
    playtime: Option<PathBuf>,
}

/// Set from [paths] by every load, so F5 picks up a change too.
static PATHS: RwLock<Paths> = RwLock::new(Paths { cache: None, playtime: None });

impl PathsCfg {
    /// Expands `~` and takes relative paths from `base`, the config file's folder, so a
    /// portable setup works no matter which folder rsgl is started from.
    fn resolve(&self, base: &Path) -> Result<Paths, String> {
        let path = |key, value: &Option<String>| match value.as_deref() {
            None => Ok(None),
            Some("") => Err(format!("[paths]: {key} is empty")),
            Some(s) => Ok(Some(base.join(expand(s)))),
        };
        Ok(Paths { cache: path("cache", &self.cache)?, playtime: path("playtime", &self.playtime)? })
    }
}

/// The folder of the config file at `path`, which relative [paths] start from.
fn config_folder(path: &Path) -> Result<PathBuf, String> {
    let abs = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(abs.parent().expect("an absolute file path has a parent").to_path_buf())
}

/// Everything rsgl downloads or generates lives under ~/.cache/rsgl, or [paths] cache.
fn cache_dir() -> PathBuf {
    let configured = PATHS.read().unwrap().cache.clone();
    configured.unwrap_or_else(|| directories::BaseDirs::new().unwrap().cache_dir().join("rsgl"))
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

/// The IGDB app access token, so it's only renewed when it expires.
pub fn igdb_token_file() -> PathBuf {
    cache_dir().join("igdb_token.toml")
}

/// Release years and genres from IGDB, by game name.
pub fn igdb_meta_file() -> PathBuf {
    cache_dir().join("igdb_meta.toml")
}

/// Which ROM files Hasheous identified, by path.
pub fn hasheous_file() -> PathBuf {
    cache_dir().join("hasheous.toml")
}

/// Writes `text` to `path` through a temporary file, so a half-written file is never
/// read. `owner_only` makes it readable by its owner only (on Unix).
pub fn write_file(path: &Path, text: &str, owner_only: bool) -> std::io::Result<()> {
    use std::io::Write;
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("part");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if owner_only {
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    }
    opts.open(&tmp)?.write_all(text.as_bytes())?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Playtime is kept under ~/.local/share/rsgl (or [paths] playtime), since unlike the
/// cache it can't be rebuilt.
pub fn playtime_file() -> PathBuf {
    let configured = PATHS.read().unwrap().playtime.clone();
    configured.unwrap_or_else(|| directories::BaseDirs::new().unwrap().data_dir().join("rsgl/playtime.toml"))
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

/// Reads and parses the config. Keys that nothing reads (a typo like `[evn]`, or a
/// table an older rsgl doesn't know) aren't an error; they come back as warnings.
fn read(path: &Path) -> Result<(Config, Vec<String>), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn parse(text: &str) -> Result<(Config, Vec<String>), toml::de::Error> {
    let mut unknown = Vec::new();
    let cfg = serde_ignored::deserialize(toml::Deserializer::parse(text)?, |key| {
        unknown.push(format!("unknown key `{key}`, ignored"));
    })?;
    Ok((cfg, unknown))
}

/// `read`, with the warnings printed to the console.
fn read_and_warn(path: &Path) -> Result<Config, String> {
    let (cfg, unknown) = read(path)?;
    for w in unknown {
        eprintln!("rsgl: {}: {w}", path.display());
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
    let mut cfg = read_and_warn(path)?;
    let at = |e| format!("{}: {e}", path.display());
    *PATHS.write().unwrap() = cfg.paths.resolve(&config_folder(path)?).map_err(at)?;
    // after the switch, so the Hasheous and IGDB data come from the configured cache
    let known = cfg.api.hasheous.then(api_hasheous::cached);
    let scan = add_roms(&mut cfg, known.as_ref()).map_err(at)?;
    for w in scan.warnings {
        eprintln!("rsgl: {}: {w}", path.display());
    }
    let settings = build(cfg, api_igdb::cached_meta()).map_err(at)?;
    Ok(Settings { unidentified_roms: scan.unidentified, ..settings })
}

fn no_config() -> String {
    let places: Vec<String> = candidates().iter().map(|p| p.display().to_string()).collect();
    format!("no config file found (looked for {})", places.join(", "))
}

/// Values for `{name}` placeholders: the global `[vars]` with a game's own `vars` on top.
/// Numbers and booleans are used as their text, and a value may itself contain `{other}`,
/// which is looked up after the two tables are merged, so a game's override reaches it.
#[derive(Clone, Default)]
pub struct Vars(BTreeMap<String, String>);

impl Vars {
    /// Variables with these literal values; braces in them are not variables.
    pub fn literal<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vars {
        Vars::default().with_defaults(pairs)
    }

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

    /// These variables with each `(name, value)` added, unless it's already set.
    /// The values are literal text: braces in them are not variables.
    pub fn with_defaults<'a>(&self, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vars {
        let mut map = self.0.clone();
        for (name, value) in pairs {
            map.entry(name.to_string()).or_insert_with(|| literal(value));
        }
        Vars(map)
    }

    /// These variables with `name` set to `template`, unless it's already set. Unlike
    /// `with_defaults`, `template` is expanded like any other value.
    fn with_template_default(&self, name: &str, template: &str) -> Vars {
        let mut map = self.0.clone();
        map.entry(name.to_string()).or_insert_with(|| template.to_string());
        Vars(map)
    }

    /// These variables with each `(name, value)` set, replacing what was there.
    /// The values are literal text, like in `with_defaults`.
    pub fn with_values<'a>(&self, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vars {
        let mut map = self.0.clone();
        map.extend(pairs.into_iter().map(|(name, value)| (name.to_string(), literal(value))));
        Vars(map)
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Replaces every `{name}` in `s`. `{{` and `}}` are literal braces.
    pub fn expand(&self, s: &str) -> Result<String, String> {
        self.expand_in(s, &mut Vec::new(), None)
    }

    /// `expand`, where `{env:NAME}` is also allowed and is `env("NAME")`, literally.
    /// Only the bars have these: everything else runs through the shell, which has `$NAME`.
    pub fn expand_env(&self, s: &str, env: &dyn Fn(&str) -> String) -> Result<String, String> {
        self.expand_in(s, &mut Vec::new(), Some(env))
    }

    /// `chain` is the variables being expanded right now, outermost first, to catch
    /// loops and to say where an error is.
    fn expand_in(&self, s: &str, chain: &mut Vec<String>, env: Option<&dyn Fn(&str) -> String>) -> Result<String, String> {
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
                if let Some(var) = name.strip_prefix(ENV_PREFIX) {
                    if !is_env_name(var) {
                        return Err(format!("{{{name}}} isn't an environment variable name{at}"));
                    }
                    let Some(env) = env else {
                        return Err(format!("{{{name}}} only works in [status] and [topbar] labels{at}; use ${var} in commands"));
                    };
                    out.push_str(&env(var));
                } else if !is_var_name(name) {
                    return Err(format!(
                        "{{{name}}} isn't a variable name{at} (write {{{{ for a literal brace)"
                    ));
                } else {
                    out.push_str(&self.value(name, chain, &at, env)?);
                }
                rest = &after[end + 1..];
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    fn value(&self, name: &str, chain: &mut Vec<String>, at: &str, env: Option<&dyn Fn(&str) -> String>) -> Result<String, String> {
        chain.push(name.to_string());
        if chain[..chain.len() - 1].iter().any(|n| n == name) {
            return Err(format!("variables refer to each other in a circle: {}", chain.join(" -> ")));
        }
        let Some(raw) = self.0.get(name) else {
            return Err(format!("{{{name}}} isn't defined{at}"));
        };
        let text = self.expand_in(raw, chain, env)?;
        chain.pop();
        Ok(text)
    }
}

/// Resolves one menu entry for a game. `owner` names where the entry came from, for errors.
fn menu_entry(
    m: &MenuCfg,
    owner: &str,
    workdir: Option<&Path>,
    env: &Env,
    vars: &Vars,
) -> Result<MenuEntry, String> {
    let err = |msg: String| format!("{owner}, entry {:?}: {msg}", m.title);
    let vars = vars.with(&m.vars).map_err(|e| err(format!("vars: {e}")))?;
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
        only_for: None,
    };
    menu_entry(&m, owner, None, &env, vars)
}

/// What every game starts from: the global `[vars]` (with the top bar's toggles in
/// them), `[env]` and `[[right_click]]`, and what IGDB has said about games so far.
struct Shared<'a> {
    vars: Vars,
    /// The variables top bar toggles switch, with their on and off values.
    var_toggles: Vec<(String, [String; 2])>,
    env: &'a BTreeMap<String, String>,
    defaults: &'a Defaults,
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
    defaults: Defaults,
    right_click: Vec<MenuCfg>,
    /// The games that aren't hidden.
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
            defaults: &self.defaults,
            menu: &self.right_click,
            meta: &self.meta,
        };
        let mut games = self.games.iter().map(|g| build_game(g.clone(), &shared)).collect::<Result<Vec<_>, _>>()?;
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

/// Every variable name in `[vars]` or some game's `vars`, for checking `[status]` and `[topbar]`.
fn var_names(cfg: &Config) -> BTreeSet<String> {
    cfg.vars.keys().chain(cfg.games.iter().flat_map(|g| g.vars.keys())).cloned().collect()
}

/// Turns the parsed file into games: fills in variables, expands paths, finds
/// covers, resolves menus. Hidden games are skipped without being checked.
/// `meta` is the cached IGDB metadata, by game name.
fn build(cfg: Config, meta: BTreeMap<String, Meta>) -> Result<Settings, String> {
    let names = var_names(&cfg);
    let status = Status::new(cfg.status, &Checker::new(&names)).map_err(|e| format!("[status]: {e}"))?;
    let library = Library {
        vars: cfg.vars,
        env: cfg.env,
        defaults: cfg.defaults,
        right_click: cfg.right_click,
        games: cfg.games.into_iter().filter(|g| !g.hidden).collect(),
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
    // is stale, and so is a ROM's from before IGDB's names were kept, since its title and
    // art may use it
    let igdb_platform = g.igdb_platform.filter(|_| g.igdb_id.is_none());
    let meta = shared
        .meta
        .get(&name)
        .filter(|m| m.igdb_id == g.igdb_id && m.igdb_platform == igdb_platform && (!g.rom || m.name.is_some()))
        .cloned();

    let exec = expand(&sub("exec", &g.exec)?);
    // the game's own workdir wins over [defaults], and one that comes out empty is none
    let workdir = match (&g.workdir, &shared.defaults.workdir) {
        (Some(w), _) => Some(sub("workdir", w)?),
        (None, Some(w)) => Some(vars.expand(w).map_err(|e| {
            format!("{who}, workdir (from [defaults]): {e}; give the game its own workdir, or workdir = \"\" for none")
        })?),
        (None, None) => None,
    }
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
    let menu_vars = vars.with_template_default(GAME_EXEC, &g.exec).with_defaults([
        (GAME_NAME, name.as_str()),
        (GAME_GRID, &grid),
        (GAME_HERO, &hero),
        (GAME_WORKDIR, &wd),
    ]);
    let mut menu = Vec::new();
    let shared_owner = format!("[[right_click]] (for {who})");
    let kind = if g.rom { Kind::Rom } else { Kind::Game };
    // an entry for the other kind isn't built at all, so it may use variables this game lacks
    for m in shared.menu.iter().filter(|m| m.only_for.is_none_or(|k| k == kind)) {
        menu.push(menu_entry(m, &shared_owner, workdir.as_deref(), &env, &menu_vars)?);
    }
    for m in &g.right_click {
        if m.only_for.is_some() {
            return Err(format!("{who}, entry {:?}: `for` only works in a top-level [[right_click]]", m.title));
        }
        menu.push(menu_entry(m, &who, workdir.as_deref(), &env, &menu_vars)?);
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
        grid_id: g.grid_id,
        hero_id: g.hero_id,
        igdb_id: g.igdb_id,
        igdb_platform,
        rom: g.rom,
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
    let mut report = Report {
        path: PathBuf::new(),
        games: cfg.games.len(),
        hidden: cfg.games.iter().filter(|g| g.hidden).count(),
        errors,
        warnings,
    };
    let check = Checker::new(&var_names(&cfg));
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
            report.warnings.extend(unmatched_filter_tags(&topbar, &cfg.games));
            (vars, topbar.var_toggles())
        }
        Err(e) => {
            report.errors.push(e);
            (vars, Vec::new())
        }
    };
    // IGDB metadata only changes titles and the status bar, which there's nothing to check about
    let meta = BTreeMap::new();
    let shared =
        Shared { vars, var_toggles, env: &cfg.env, defaults: &cfg.defaults, menu: &cfg.right_click, meta: &meta };

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

    let mut out = format!("{}\n  exec:    {}\n  workdir: {}\n", g.name, g.exec, dir(&g.workdir));
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
            MenuAction::Exec(cmd) => format!("exec: {cmd}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Action;

    fn games(text: &str) -> Result<Vec<Game>, String> {
        parse_games(text)
    }

    #[test]
    fn config_next_to_exe_comes_first() {
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        assert_eq!(candidates().first(), Some(&exe_dir.join("rsgl.toml")));
        assert!(!home_candidates().iter().any(|p| p.starts_with(&exe_dir)));
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
        assert_eq!(tool.env.all["X"], "1", "game env is kept");
        assert_eq!(tool.env.all["Y"], "3", "entry env overrides");
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
    fn defaults_workdir_uses_each_games_vars_and_a_games_own_wins() {
        let g = games(
            r#"
[vars]
games = "/g"

[defaults]
workdir = "{games}/{path}/"

[[game]]
name = "A"
exec = "a"
vars = { path = "A" }

[[game]]
name = "B"
exec = "b"
workdir = "{games}/B/System/"

[[game]]
name = "C"
exec = "c"
workdir = ""

[[game.right_click]]
title = "Tool"
exec = "t"
"#,
        )
        .unwrap();
        assert_eq!(g[0].workdir, Some(PathBuf::from("/g/A/")));
        assert_eq!(g[1].workdir, Some(PathBuf::from("/g/B/System/")), "own workdir wins");
        assert_eq!(g[2].workdir, None, "an empty workdir opts out");
        assert_eq!(g[2].menu[0].workdir, None);
    }

    #[test]
    fn defaults_workdir_with_a_missing_var_says_where_it_came_from() {
        let e = game_err("[defaults]\nworkdir = \"/g/{path}\"\n[[game]]\nname = \"A\"\nexec = \"x\"\n");
        assert!(e.contains("game \"A\", workdir (from [defaults])") && e.contains("{path}"), "{e}");
        // hidden games aren't checked, so one without the variable is fine
        let g = games("[defaults]\nworkdir = \"/g/{path}\"\n[[game]]\nname = \"A\"\nexec = \"x\"\nhidden = true\n");
        assert!(g.unwrap().is_empty());
    }

    #[test]
    fn defaults_rejects_keys_it_doesnt_have() {
        let e = games("[defaults]\nexec = \"x\"\n").err().expect("should fail");
        assert!(e.contains("exec"), "{e}");
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
        assert_eq!(a.env.all["WINEPREFIX"], "/p/Quake");
        assert_eq!(a.menu[0].action, MenuAction::Exec("xdg-open /g/Quake".into()));
        assert_eq!(a.menu[0].env.all["D"], "Quake");
        assert_eq!(a.menu[1].action, MenuAction::Exec("tool Quake".into()));
        assert_eq!(a.menu[1].workdir, Some(PathBuf::from("/w/Quake")));
    }

    #[test]
    fn a_menu_entry_reruns_the_games_exec_with_its_own_vars() {
        let g = games(
            r#"
[vars]
gs = "gamescope -w {w} --"
w = 3440

[[right_click]]
title = "No gamescope"
exec = "{exec}"
vars = { gs = "" }
launch = true

[[game]]
name = "A"
exec = "{gs} wine a.exe"
vars = { gs = "gs-a" }

[[game.right_click]]
title = "Narrow"
exec = "{exec} -windowed"
vars = { w = 1920 }

[[game]]
name = "B"
exec = "{gs} wine b.exe"
"#,
        )
        .unwrap();
        let (a, b) = (&g[0], &g[1]);
        assert_eq!(a.exec, "gs-a wine a.exe");
        // the entry's vars win over the game's own
        assert_eq!(a.menu[0].action, MenuAction::Exec(" wine a.exe".into()));
        assert_eq!(a.menu[1].action, MenuAction::Exec("gs-a wine a.exe -windowed".into()));
        assert_eq!(b.exec, "gamescope -w 3440 -- wine b.exe");
        assert_eq!(b.menu[0].action, MenuAction::Exec(" wine b.exe".into()));

        // only menu entries have {exec}
        let e = game_err("[[game]]\nname = \"A\"\nexec = \"{exec}\"\n");
        assert!(e.contains("{exec} isn't defined"), "{e}");
        let e = game_err("[[right_click]]\ntitle = \"T\"\nexec = \"t\"\nvars = { v = [1] }\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
        assert!(e.contains("entry \"T\": vars: v must be"), "{e}");
    }

    #[test]
    fn menu_entries_get_the_games_name_art_and_workdir() {
        let cache = std::env::temp_dir().join(format!("rsgl-menu-vars-test-{}", std::process::id()));
        let (cover, hero) = (cache.join("covers/rsgl_menu_test.png"), cache.join("heroes/rsgl_menu_test.jpg"));
        for f in [&cover, &hero] {
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, "").unwrap();
        }
        // only this test's game has art there, so the other tests don't notice
        PATHS.write().unwrap().cache = Some(cache.clone());
        let g = games(
            r#"
[[right_click]]
title = "T"
exec = "t '{name}' {grid} {hero} {workdir}"

[[game]]
name = "Rsgl {x} Test {year}"
exec = "a"
workdir = "/g/{x}"
vars = { x = "Menu" }

[[game]]
name = "B"
exec = "b"
vars = { hero = "mine" }
"#,
        );
        PATHS.write().unwrap().cache = None;
        fs::remove_dir_all(&cache).unwrap();
        let g = g.unwrap();
        let (b, test) = (&g[0], &g[1]);

        // the art is found by the name without {year}, which is also what downloads are saved as
        assert_eq!(test.cover.as_ref(), Some(&cover));
        let want = format!("t 'Rsgl Menu Test' {} {} /g/Menu", cover.display(), hero.display());
        assert_eq!(test.menu[0].action, MenuAction::Exec(want));
        // no art or workdir yet is blank, and the game's own variable wins
        assert_eq!(b.menu[0].action, MenuAction::Exec("t 'B'  mine ".into()));
        // they're only for menu entries, like {exec}
        assert!(game_err("[[game]]\nname = \"A\"\nexec = \"{grid}\"\n").contains("{grid} isn't defined"));
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
        assert_eq!(a.env.all["WINEDEBUG"], "-all", "global values apply");
        assert_eq!(a.env.all["WINEPREFIX"], "/p/a", "and see the game's variables");
        assert_eq!(a.env.all["SHARED"], "mine", "the game's own value wins");
        assert_eq!(a.env.all["ONLY_A"], "1");
        assert_eq!(a.menu[0].env.all["WINEDEBUG"], "-all", "menu entries get it too");
    }

    #[test]
    fn toggles_replace_global_env_but_not_a_games_or_entrys_own() {
        let g = games(
            r#"
[env]
MANGOHUD = "0"
DXVK_HUD = "0"
WINEDEBUG = "-all"

[[game]]
name = "A"
exec = "a"
env = { DXVK_HUD = "fps" }
right_click = [{ title = "T", exec = "t", env = { MANGOHUD = "0" } }]
"#,
        )
        .unwrap();
        let a = &g[0];
        let toggles = Toggles::from([
            ("MANGOHUD".to_string(), Some("1".to_string())),
            ("DXVK_HUD".to_string(), Some("full".to_string())),
            ("WINEDEBUG".to_string(), None),
            ("GAMEMODE".to_string(), Some("1".to_string())),
        ]);
        assert_eq!(a.env.get("MANGOHUD", &toggles), Some(Some("1")), "replaces [env]");
        assert_eq!(a.env.get("DXVK_HUD", &toggles), Some(Some("fps")), "the game's own wins");
        assert_eq!(a.env.get("WINEDEBUG", &toggles), Some(None), "off without a value unsets");
        assert_eq!(a.env.get("PATH", &toggles), None, "left to rsgl's own");
        assert_eq!(
            a.env.with(&toggles),
            [("DXVK_HUD", Some("fps")), ("GAMEMODE", Some("1")), ("MANGOHUD", Some("1")), ("WINEDEBUG", None)],
            "also one nothing else sets"
        );
        assert_eq!(a.menu[0].env.get("MANGOHUD", &toggles), Some(Some("0")), "the entry's own wins");
        assert_eq!(a.menu[0].env.get("DXVK_HUD", &toggles), Some(Some("fps")), "so does the game's");
        assert_eq!(a.env.get("MANGOHUD", &Toggles::new()), Some(Some("0")), "no toggles, just [env]");
    }

    #[test]
    fn global_env_errors_say_where_they_came_from() {
        let e = game_err("[env]\nK = \"{dir}\"\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
        assert!(e.contains("[env]") && e.contains("game \"A\"") && e.contains("{dir} isn't defined"), "{e}");
        // values must be strings, like a game's env
        assert!(games("[env]\nK = 1\n[[game]]\nname = \"A\"\nexec = \"a\"\n").is_err());
    }

    #[test]
    fn keyboard_and_gamepad_tables_are_read_and_checked() {
        let (cfg, unknown) = parse("[keyboard]\nlaunch = [\"F2\"]\n[gamepad]\nlaunch = [\"X\"]\n").unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(cfg.keyboard.label(Action::Launch).as_deref(), Some("F2"));
        assert_eq!(cfg.gamepad.label(Action::Launch).as_deref(), Some("X"));

        let e = parse("[gamepad]\nback = [\"Circle\"]\n").err().unwrap().to_string();
        assert!(e.contains("gamepad") && e.contains("unknown button \"Circle\""), "{e}");
    }

    #[test]
    fn unknown_keys_are_warnings_not_errors() {
        let (cfg, unknown) = parse(
            "[evn]\nK = \"1\"\n[ui]\nblurr = 3\n[[game]]\nname = \"A\"\nexec = \"a\"\ncovr = \"x\"\n",
        )
        .unwrap();
        assert_eq!(cfg.games.len(), 1);
        assert_eq!(unknown.len(), 3, "{unknown:?}");
        for key in ["`evn`", "`ui.blurr`", "`game.0.covr`"] {
            assert!(unknown.iter().any(|w| w.contains(key)), "{key} in {unknown:?}");
        }
        // menu entries still reject unknown keys, see MenuCfg
        let menu = "[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = \"t\"\nhidden = true\n";
        assert!(parse(menu).is_err());
    }

    #[test]
    fn a_game_is_installed_when_its_workdir_exists_or_isnt_set() {
        let dir = std::env::temp_dir();
        let g = games(&format!(
            r#"
[[game]]
name = "Here"
exec = "a"
workdir = {dir:?}

[[game]]
name = "Gone"
exec = "a"
workdir = "/rsgl-test/no/such/dir"

[[game]]
name = "No workdir"
exec = "a"
"#
        ))
        .unwrap();
        let installed = |name: &str| g.iter().find(|g| g.name == name).unwrap().installed();
        assert!(installed("Here"));
        assert!(!installed("Gone"));
        assert!(installed("No workdir"));
    }

    fn checked(text: &str) -> Report {
        check_config(toml::from_str(text).unwrap())
    }

    #[test]
    fn paths_are_taken_from_the_config_folder() {
        let paths = |text: &str| parse(text).unwrap().0.paths.resolve(Path::new("/usb/rsgl"));
        assert_eq!(paths("").unwrap(), Paths { cache: None, playtime: None }, "unset keeps the defaults");
        let p = paths("[paths]\ncache = \"./cache\"\nplaytime = \"/data/playtime.toml\"").unwrap();
        assert_eq!(p.cache, Some(PathBuf::from("/usb/rsgl/./cache")));
        assert_eq!(p.playtime, Some(PathBuf::from("/data/playtime.toml")), "an absolute path is kept");
        let home = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
        assert_eq!(paths("[paths]\ncache = \"~/rsgl\"").unwrap().cache, Some(home.join("rsgl")));
        assert_eq!(paths("[paths]\nplaytime = \"\"").unwrap_err(), "[paths]: playtime is empty");
    }

    #[test]
    fn config_folder_of_a_relative_path_is_absolute() {
        let folder = config_folder(Path::new("rsgl.toml")).unwrap();
        assert_eq!(folder, std::env::current_dir().unwrap());
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

    #[test]
    fn check_warns_about_filter_tags_no_game_has() {
        let r = checked(
            r#"
[topbar]
left = [{ type = "filter", options = [
  { title = "All", filter = "" },
  { title = "LAN", filter = "t:pc t:LAN quake g:shooter" },
  { title = "NES", filter = "t:nes" },
  { title = "Old", filter = "t:old" },
] }]

[[game]]
name = "Quake"
exec = "a"
tags = ["pc", "lan-party"]

[[game]]
name = "Hidden"
exec = "a"
tags = ["old"]
hidden = true
"#,
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(
            r.warnings,
            [
                "[topbar]: filter \"NES\": no game has a tag t:nes matches",
                "[topbar]: filter \"Old\": no game has a tag t:old matches",
            ],
            "tags match from their start, any case; a hidden game isn't shown"
        );
    }

    #[test]
    fn grid_and_hero_ids_are_read() {
        let g = games("[[game]]\nname = \"A\"\nexec = \"a\"\ngrid_id = 664940\nhero_id = 15208\n").unwrap();
        assert_eq!((g[0].grid_id, g[0].hero_id), (Some(664940), Some(15208)));
        let g = games("[[game]]\nname = \"A\"\nexec = \"a\"\n").unwrap();
        assert_eq!((g[0].grid_id, g[0].hero_id), (None, None));
    }

    fn games_with_meta(text: &str, years: &[(&str, i32)]) -> Vec<Game> {
        let cfg: Config = toml::from_str(text).unwrap();
        let meta = years.iter().map(|(n, y)| (n.to_string(), Meta { year: Some(*y), ..Meta::default() })).collect();
        build(cfg, meta).unwrap().games
    }

    #[test]
    fn names_take_variables_and_the_year_stays_out_of_the_name() {
        let text = r#"
[vars]
series = "Command & Conquer"
[[game]]
name = "{series} 3: Kane's Wrath {year}"
exec = "a"
[[game]]
name = "Quake ({year})"
exec = "b"
[[game]]
name = "Doom"
exec = "c"
"#;
        let g = games_with_meta(text, &[]);
        let (cnc, doom, quake) = (&g[0], &g[1], &g[2]);
        assert_eq!(cnc.name, "Command & Conquer 3: Kane's Wrath");
        assert_eq!(cnc.title, cnc.name, "no year known yet");
        assert!(cnc.needs_meta(false));
        assert_eq!(quake.name, "Quake", "empty brackets go too");
        assert_eq!((doom.title.as_str(), doom.needs_meta(false)), ("Doom", false));
        assert!(doom.needs_meta(true), "the status bar wants it");

        // cached metadata goes by the name, and the name stays the same
        let g = games_with_meta(text, &[("Command & Conquer 3: Kane's Wrath", 2008), ("Doom", 1993)]);
        assert_eq!(g[0].name, "Command & Conquer 3: Kane's Wrath");
        assert_eq!(g[0].title, "Command & Conquer 3: Kane's Wrath 2008");
        assert!(!g[0].needs_meta(true));
        assert_eq!(g[1].title, "Doom", "a name without {{year}} ignores the cache");
        assert_eq!(g[1].meta.as_ref().and_then(|m| m.year), Some(1993), "but keeps it for the status bar");

        // metadata found later
        let mut quake = g.into_iter().nth(2).unwrap();
        quake.set_meta(Meta { year: Some(1996), ..Meta::default() });
        assert_eq!((quake.title.as_str(), quake.needs_meta(true)), ("Quake (1996)", false));
    }

    #[test]
    fn igdb_data_cached_under_another_igdb_id_is_stale() {
        let text = |id: &str| format!("[[game]]\nname = \"Doom {{year}}\"\nexec = \"a\"\n{id}");
        let cached = |igdb_id| {
            let meta = Meta { year: Some(1993), igdb_id, ..Meta::default() };
            BTreeMap::from([("Doom".to_string(), meta)])
        };
        let doom = |id: &str, igdb_id| build(toml::from_str(&text(id)).unwrap(), cached(igdb_id)).unwrap().games.remove(0);

        assert_eq!(doom("igdb_id = 673", None).igdb_id, Some(673));
        assert!(doom("igdb_id = 673", None).needs_meta(false), "found by name before the id was set");
        assert!(doom("igdb_id = 673", Some(1)).needs_meta(false), "found by a different id");
        assert_eq!(doom("igdb_id = 673", Some(673)).title, "Doom 1993");
        assert!(doom("", Some(673)).needs_meta(false), "the id was removed, so look it up by name");
        assert_eq!(doom("", None).title, "Doom 1993");
    }

    #[test]
    fn a_games_own_year_wins_and_year_is_only_for_names() {
        let g = games("[[game]]\nname = \"Quake {year}\"\nexec = \"a\"\nvars = { year = 1996 }\n").unwrap();
        assert_eq!((g[0].title.as_str(), g[0].needs_meta(false)), ("Quake 1996", false));
        let e = game_err("[[game]]\nname = \"A\"\nexec = \"run {year}\"\n");
        assert!(e.contains("{year} isn't defined"), "{e}");
        let e = game_err("[[game]]\nname = \"A {nope}\"\nexec = \"a\"\n");
        assert!(e.contains("game \"A {nope}\", name"), "{e}");
    }

    #[test]
    fn status_parts_are_checked_against_all_vars() {
        let text = |part: &str| format!("[status]\nleft = [{{ text = \"{part}\" }}]\n[[game]]\nname = \"A\"\nexec = \"a\"\nvars = {{ dir = \"a\" }}\n");
        assert!(games(&text("{title} {dir}")).is_ok(), "a game's own variable is fine");
        let e = game_err(&text("{nope}"));
        assert!(e.contains("[status]") && e.contains("{nope} isn't defined"), "{e}");
        let r = checked(&text("{nope}"));
        assert!(r.errors.iter().any(|e| e.contains("[status]")), "{:?}", r.errors);
    }

    #[test]
    fn topbar_labels_are_checked_like_status_parts() {
        let text = |label: &str| {
            format!("[topbar]\nright = [{{ type = \"label\", text = \"{label}\" }}]\n[[game]]\nname = \"A\"\nexec = \"a\"\nvars = {{ dir = \"a\" }}\n")
        };
        assert!(games(&text("{dir} {env:HOME}")).is_ok());
        let e = game_err(&text("{nope}"));
        assert!(e.contains("[topbar]: right widget 1") && e.contains("{nope} isn't defined"), "{e}");
        let r = checked(&text("{nope}"));
        assert!(r.errors.iter().any(|e| e.contains("[topbar]")), "{:?}", r.errors);
    }

    #[test]
    fn env_is_only_for_the_bars() {
        // a variable used in exec can't sneak it in either
        let e = game_err("[vars]\nmh = \"{env:MANGOHUD}\"\n[[game]]\nname = \"A\"\nexec = \"run {mh}\"\n");
        assert!(e.contains("{env:MANGOHUD} only works in [status] and [topbar]") && e.contains("$MANGOHUD"), "{e}");
        // but it's fine in a status part, through the variable too
        assert!(games("[vars]\nmh = \"{env:MANGOHUD}\"\n[status]\nleft = [{ text = \"{mh}\" }]\n").is_ok());
    }

    #[test]
    fn bar_env_is_what_the_game_launches_with() {
        let g = games("[env]\nA = \"global\"\n[[game]]\nname = \"G\"\nexec = \"g\"\nenv = { PATH = \"/mine\" }\n").unwrap();
        let no_toggles = Toggles::new();
        let env = crate::status::env(Some(&g[0]), &no_toggles);
        assert_eq!((env("A").as_str(), env("PATH").as_str()), ("global", "/mine"), "[env] and the game's own win over ours");
    }

    const VAR_TOGGLE: &str = r#"
        [vars]
        mango = ""
        gs = "gamescope{mango} -f --"
        [topbar]
        left = [
          { type = "toggle", title = "MangoHud", var = "mango", on = " --mangoapp" },
          { type = "button", title = "Steam", exec = "{gs} steam" },
        ]
        [[game]]
        name = "G"
        exec = "{gs} ./g"
        [[game]]
        name = "Own"
        exec = "{gs} ./own"
        vars = { mango = " --own" }
    "#;

    fn steam(topbar: &Topbar) -> &str {
        let crate::topbar::Widget::Button(b) = topbar.widget((0, 1)) else { panic!("a button") };
        let MenuAction::Exec(exec) = &b.action else { panic!("an exec") };
        exec
    }

    #[test]
    fn var_toggles_switch_vars_for_games_and_buttons_when_built_again() {
        let cfg: Config = toml::from_str(VAR_TOGGLE).unwrap();
        let s = build(cfg, BTreeMap::new()).unwrap();
        let exec = |games: &[Game]| games.iter().map(|g| g.exec.clone()).collect::<Vec<_>>();
        assert_eq!(exec(&s.games), ["gamescope -f -- ./g", "gamescope --own -f -- ./own"], "starts off, like [vars]");
        assert_eq!(steam(&s.topbar), "gamescope -f -- steam");

        let mut on = s.topbar;
        on.flip((0, 0));
        let (games, topbar) = s.library.build(Some(&on)).unwrap();
        assert_eq!(exec(&games), ["gamescope --mangoapp -f -- ./g", "gamescope --own -f -- ./own"], "a game's own wins");
        assert_eq!(steam(&topbar), "gamescope --mangoapp -f -- steam");
        assert_eq!(topbar.var_values(), [("mango", " --mangoapp")], "still on");
    }

    #[test]
    fn building_again_for_a_var_toggle_keeps_the_sort() {
        let text = VAR_TOGGLE.replace("left = [", r#"left = [{ type = "sort", options = ["name", "year"] },"#);
        let s = build(toml::from_str(&text).unwrap(), BTreeMap::new()).unwrap();
        let mut on = s.topbar;
        on.set_sort((0, 0), 1);
        on.flip((0, 1));
        let (_, topbar) = s.library.build(Some(&on)).unwrap();
        assert_eq!(topbar.sort(), crate::topbar::SortBy::Year);
    }

    #[test]
    fn a_var_toggle_starts_from_vars_and_its_off_value_applies_from_the_start() {
        let text = VAR_TOGGLE.replace("mango = \"\"", "mango = \" --mangoapp\"");
        let s = build(toml::from_str(&text).unwrap(), BTreeMap::new()).unwrap();
        assert_eq!(s.topbar.var_values(), [("mango", " --mangoapp")]);
        assert_eq!(s.games[0].exec, "gamescope --mangoapp -f -- ./g");

        // [vars] has neither value: it starts off, and off is what games get
        let text = VAR_TOGGLE.replace("mango = \"\"", "mango = \"x\"");
        let s = build(toml::from_str(&text).unwrap(), BTreeMap::new()).unwrap();
        assert_eq!(s.games[0].exec, "gamescope -f -- ./g");
    }

    #[test]
    fn a_var_toggle_cant_change_a_games_name() {
        let text = VAR_TOGGLE.replace("name = \"G\"", "name = \"G{mango}\"");
        let e = build(toml::from_str(&text).unwrap(), BTreeMap::new()).err().unwrap();
        assert!(e.contains("game \"G{mango}\", name: can't use {mango}, a top bar toggle switches it"), "{e}");
        let text = VAR_TOGGLE.replace("name = \"Own\"", "name = \"Own{mango}\"");
        assert!(build(toml::from_str(&text).unwrap(), BTreeMap::new()).is_ok(), "its own mango, the toggle doesn't reach it");
        let report = check_config(toml::from_str(&VAR_TOGGLE.replace("name = \"G\"", "name = \"G{mango}\"")).unwrap());
        assert!(report.errors.iter().any(|e| e.contains("can't use {mango}")), "{:?}", report.errors);
    }
    #[test]
    fn rom_names_drop_region_and_dump_groups() {
        assert_eq!(rom_name("Super Mario World (USA) [!]"), "Super Mario World");
        assert_eq!(rom_name("Zelda (Europe) (Rev 1) - Extra"), "Zelda - Extra");
        assert_eq!(rom_name("Doom (unclosed"), "Doom (unclosed");
        assert_eq!(rom_name("(USA)"), "(USA)", "nothing but groups is kept");
    }

    #[test]
    fn roms_become_games_with_rom_workdir_vars_and_env() {
        let dir = std::env::temp_dir().join(format!("rsgl-roms-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub.sfc")).unwrap();
        for f in ["Super Mario World (USA).sfc", "F-Zero {x} [!].SMC", "readme.txt", "noext"] {
            fs::write(dir.join(f), "").unwrap();
        }
        let text = format!(
            r#"
[vars]
cores = "/cores"
roms = {:?}
[env]
G = "global"

[[right_click]]
title = "Shared"
exec = "{{exec}} --menu"

[[roms]]
path = "{{roms}}"
extensions = ["sfc", ".smc"]
exec = 'retroarch -L {{cores}}/snes9x.so "{{rom}}"'
tags = ["snes"]
env = {{ R = "rom" }}
"#,
            dir.to_str().unwrap()
        );
        let g = games(&text).unwrap();
        let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["F-Zero {x}", "Super Mario World"], "other extensions and folders are skipped");

        let smw = &g[1];
        let rom = dir.join("Super Mario World (USA).sfc");
        assert_eq!(smw.exec, format!("retroarch -L /cores/snes9x.so \"{}\"", rom.display()));
        assert_eq!(smw.workdir.as_deref(), Some(dir.as_path()), "workdir defaults to the folder");
        assert_eq!(smw.tags, ["snes"]);
        assert_eq!((smw.env.all["G"].as_str(), smw.env.all["R"].as_str()), ("global", "rom"));
        assert_eq!(smw.menu[0].action, MenuAction::Exec(format!("{} --menu", smw.exec)), "[[right_click]] applies");
        assert!(g[0].exec.contains("F-Zero {x} [!].SMC"), "braces in file names are literal");

        let with_workdir = text.replace("tags = ", "workdir = \"/run\"\ntags = ");
        assert_eq!(games(&with_workdir).unwrap()[0].workdir, Some(PathBuf::from("/run")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn right_click_for_picks_games_or_roms() {
        let dir = std::env::temp_dir().join(format!("rsgl-roms-for-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Zelda.nes"), "").unwrap();
        let text = format!(
            r#"
[[right_click]]
title = "Open folder"
action = "open_folder"

[[right_click]]
title = "Uninstall"
for = "game"
exec = "rm -rf {{path}}"

[[right_click]]
title = "Delete ROM"
for = "rom"
exec = 'rm "{{rom}}"'

[[game]]
name = "Quake"
exec = "quake"
vars = {{ path = "quake" }}

[[roms]]
path = {:?}
extensions = ["nes"]
exec = "nestopia"
"#,
            dir.to_str().unwrap()
        );
        let g = games(&text).unwrap();
        let titles = |name: &str| -> Vec<String> {
            g.iter().find(|g| g.name == name).unwrap().menu.iter().map(|m| m.title.clone()).collect()
        };
        assert_eq!(titles("Quake"), ["Open folder", "Uninstall"]);
        assert_eq!(titles("Zelda"), ["Open folder", "Delete ROM"], "and {{path}} isn't needed for ROMs");
        let report = check_config(toml::from_str(&text).unwrap());
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        let e = game_err(&text.replace("for = \"rom\"", "for = \"roms\""));
        assert!(e.contains("unknown variant `roms`"), "{e}");
        let e = game_err("[[game]]\nname = \"A\"\nexec = \"a\"\n[[game.right_click]]\ntitle = \"T\"\nexec = \"t\"\nfor = \"game\"\n");
        assert!(e.contains("`for` only works in a top-level [[right_click]]"), "{e}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hasheous_url_takes_the_server_with_or_without_the_api_path() {
        let api = |text: &str| parse(&format!("[api]\n{text}")).map(|(c, _)| c.api.hasheous_api());
        assert_eq!(api("").unwrap(), "https://hasheous.org/api/v1");
        assert_eq!(api("hasheous_url = \"http://nas:5000\"").unwrap(), "http://nas:5000/api/v1");
        assert_eq!(api("hasheous_url = \"http://nas:5000/\"").unwrap(), "http://nas:5000/api/v1");
        assert_eq!(api("hasheous_url = \"https://h.lan/x/api/v1/\"").unwrap(), "https://h.lan/x/api/v1");
        for bad in ["nas:5000", "ftp://nas", "https://", ""] {
            let e = api(&format!("hasheous_url = \"{bad}\"")).unwrap_err().to_string();
            assert!(e.contains("isn't an http:// or https:// URL"), "{bad}: {e}");
        }
    }

    #[test]
    fn hasheous_names_the_roms_it_identified() {
        let dir = std::env::temp_dir().join(format!("rsgl-roms-hasheous-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let [smw, homebrew, new] = ["smw.sfc", "Homebrew (PD).sfc", "New {x} (USA).sfc"].map(|f| dir.join(f));
        for f in [&smw, &homebrew, &new] {
            fs::write(f, "").unwrap();
        }
        let text = format!("[[roms]]\npath = {:?}\nextensions = [\"sfc\"]\nexec = \"x\"\n", dir.to_str().unwrap());
        let found = api_hasheous::Found { name: "Super Mario World {1}".into(), igdb_id: Some(1070), steamgriddb_id: Some(36886) };
        let known = api_hasheous::Known::of(&[(&smw, Some(found)), (&homebrew, None)]);
        let scan_with = |known| {
            let mut cfg: Config = toml::from_str(&text).unwrap();
            let scan = add_roms(&mut cfg, known).unwrap();
            let games = build(cfg, BTreeMap::new()).unwrap().games;
            (scan.unidentified, games)
        };

        let (unidentified, g) = scan_with(Some(&known));
        assert_eq!(unidentified, [new], "only the one never looked up is left to ask about");
        let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Homebrew", "New {x}", "Super Mario World {1}"], "braces in Hasheous's names are literal");
        assert_eq!((g[2].igdb_id, g[2].art_id.map(|a| a.to_string())), (Some(1070), Some("steamgriddb:36886".into())));

        let (unidentified, g) = scan_with(None);
        assert!(unidentified.is_empty(), "nothing is asked with Hasheous off");
        assert_eq!(g.iter().find(|g| g.name == "smw").map(|g| g.igdb_id), Some(None), "and its names aren't used");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn roms_files_pin_ids_and_hide_files() {
        let dir = std::env::temp_dir().join(format!("rsgl-roms-files-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let [tgm2, neogeo, smw] = ["tgm2.zip", "neogeo.zip", "smw.zip"].map(|f| dir.join(f));
        for f in [&tgm2, &neogeo, &smw] {
            fs::write(f, "").unwrap();
        }
        let text = format!(
            "[[roms]]\npath = {:?}\nextensions = [\"zip\"]\nexec = \"x\"\n\
             files.\"tgm2.zip\" = {{ igdb_id = 1, steamgriddb_id = 2, grid_id = 3, hero_id = 4 }}\n\
             files.\"neogeo.zip\" = {{ hidden = true }}\n\
             files.\"smw.zip\" = {{ igdb_id = 5 }}\n\
             files.\"gone.zip\" = {{ igdb_id = 6 }}\n",
            dir.to_str().unwrap()
        );
        let found = api_hasheous::Found { name: "Super Mario World".into(), igdb_id: Some(1070), steamgriddb_id: Some(36886) };
        let known = api_hasheous::Known::of(&[(&smw, Some(found))]);
        let mut cfg: Config = toml::from_str(&text).unwrap();
        let scan = add_roms(&mut cfg, Some(&known)).unwrap();
        assert_eq!(scan.unidentified, [tgm2], "a hidden file isn't sent to Hasheous");
        assert!(
            matches!(&scan.warnings[..], [w] if w.contains("files") && w.contains("\"gone.zip\"")),
            "a key that matches no file is a warning: {:?}",
            scan.warnings
        );
        let g = build(cfg, BTreeMap::new()).unwrap().games;
        let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Super Mario World", "tgm2"], "neogeo is hidden");
        let ids = |g: &Game| (g.igdb_id, g.art_id.map(|a| a.to_string()), g.grid_id, g.hero_id);
        assert_eq!(ids(&g[1]), (Some(1), Some("steamgriddb:2".into()), Some(3), Some(4)));
        assert_eq!(ids(&g[0]), (Some(5), Some("steamgriddb:36886".into()), None, None), "a pin beats Hasheous's id, key by key");

        let r = checked(&text);
        assert_eq!((r.games, r.hidden), (3, 1));
        assert!(r.warnings.iter().any(|w| w.contains("\"gone.zip\"")), "{:?}", r.warnings);
        let bad = text.replace("hidden = true", "hiden = true");
        assert!(toml::from_str::<Config>(&bad).is_err(), "typos in a file's keys are caught");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn igdb_platform_narrows_the_search_and_changing_it_looks_roms_up_again() {
        let dir = std::env::temp_dir().join(format!("rsgl-roms-platform-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for f in ["1080 Snowboarding (USA).z64", "pinned.z64"] {
            fs::write(dir.join(f), "").unwrap();
        }
        let games_with = |keys: &str, cached: Option<&str>| {
            let text = format!(
                "[[roms]]\npath = {:?}\nextensions = [\"z64\"]\nexec = \"x\"\nfiles.\"pinned.z64\" = {{ igdb_id = 7 }}\n{keys}",
                dir.to_str().unwrap()
            );
            let mut cfg: Config = toml::from_str(&text).map_err(|e| e.to_string())?;
            add_roms(&mut cfg, None)?;
            let found = |igdb_id| Meta { name: Some("1080° Snowboarding".into()), igdb_id, igdb_platform: cached.map(str::to_string), ..Meta::default() };
            let meta = BTreeMap::from([("1080 Snowboarding".to_string(), found(None)), ("pinned".to_string(), found(Some(7)))]);
            build(cfg, meta).map(|s| s.games)
        };

        let g = games_with("igdb_platform = \"n64\"", Some("n64")).unwrap();
        assert_eq!(g[0].igdb_platform.as_deref(), Some("n64"));
        assert!(g[0].meta.is_some(), "found on the same platform");
        assert_eq!(g[1].igdb_platform, None, "a pinned igdb_id skips the search, so the platform doesn't apply");
        assert!(g[1].meta.is_none(), "and data cached with one is stale");

        let g = games_with("igdb_platform = \"n64\"", None).unwrap();
        assert!(g[0].meta.is_none(), "found on any platform, before igdb_platform was set");
        assert!(g[1].meta.is_some(), "a pinned game's data still holds");
        assert!(games_with("igdb_platform = \"snes\"", Some("n64")).unwrap()[0].meta.is_none(), "found on another platform");
        assert!(games_with("", Some("n64")).unwrap()[0].meta.is_none(), "igdb_platform was taken out");

        let e = games_with("igdb_platform = \" \"", None).err().unwrap();
        assert!(e.contains("igdb_platform is empty"), "{e}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_roms_name_sets_the_title_with_igdb_name_and_year() {
        let dir = std::env::temp_dir().join(format!("rsgl-roms-name-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for f in ["tgm2.zip", "1080 Snowboarding (USA).z64"] {
            fs::write(dir.join(f), "").unwrap();
        }
        let text = |name: &str| {
            format!("[[roms]]\npath = {:?}\nextensions = [\"zip\", \"z64\"]\nexec = \"x\"\n{name}", dir.to_str().unwrap())
        };
        let found = |name: &str| Meta { name: Some(name.into()), year: Some(2000), ..Meta::default() };
        let meta = BTreeMap::from([
            ("tgm2".to_string(), found("Tetris: The Grand Master 2 {x}")),
            ("1080 Snowboarding".to_string(), Meta { year: Some(1998), ..Meta::default() }),
        ]);
        let games_with = |name: &str, meta: &BTreeMap<String, Meta>| {
            let mut cfg: Config = toml::from_str(&text(name)).unwrap();
            add_roms(&mut cfg, None).unwrap();
            build(cfg, meta.clone()).unwrap().games
        };

        let g = games_with("name = \"{igdb_name} ({year})\"", &meta);
        let names: Vec<_> = g.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["1080 Snowboarding", "tgm2"], "who a ROM is stays its file's name");
        assert_eq!(g[1].title, "Tetris: The Grand Master 2 {x} (2000)", "braces in IGDB's names are literal");
        assert_eq!(g[1].art_search(), Some("Tetris: The Grand Master 2 {x}"));
        assert!(g[0].meta.is_none(), "cached before IGDB's names were kept, so it's looked up again");
        assert_eq!(g[0].title, "1080 Snowboarding", "{{igdb_name}} is the file's name until IGDB has one");
        assert!(g[0].needs_meta(false));

        let mut snowboarding = games_with("name = \"{igdb_name} (N64)\"", &BTreeMap::new()).remove(0);
        assert_eq!(snowboarding.title, "1080 Snowboarding (N64)");
        snowboarding.set_meta(found("1080° Snowboarding"));
        assert_eq!(snowboarding.title, "1080° Snowboarding (N64)");
        assert_eq!(snowboarding.name, "1080 Snowboarding");
        assert_eq!(snowboarding.art_search(), Some("1080° Snowboarding"));
        snowboarding.set_meta(found("1080 SNOWBOARDING"));
        assert_eq!(snowboarding.art_search(), None, "the same name needs no other search");

        let g = games_with("", &meta);
        assert_eq!(g[1].title, "tgm2", "without a name, the title is the file's");
        assert_eq!(g[1].art_search(), Some("Tetris: The Grand Master 2 {x}"), "art still comes by IGDB's name");
        assert!(!g[1].needs_meta(false));

        let mut cfg: Config = toml::from_str(&text("name = \"{nope}\"")).unwrap();
        add_roms(&mut cfg, None).unwrap();
        let e = build(cfg, BTreeMap::new()).err().unwrap();
        assert!(e.contains("name") && e.contains("nope"), "{e}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_games_name_cant_use_igdb_name_and_isnt_searched_by_it() {
        let e = game_err("[[game]]\nname = \"{igdb_name}\"\nexec = \"a\"\n");
        assert!(e.contains("{igdb_name} only works in a [[roms]] name"), "{e}");
        let text = "[[game]]\nname = \"Quake {year}\"\nexec = \"a\"\n";
        let meta = Meta { name: Some("Quake (1996)".into()), year: Some(1996), ..Meta::default() };
        let g = build(toml::from_str(text).unwrap(), BTreeMap::from([("Quake".to_string(), meta)])).unwrap().games;
        assert_eq!(g[0].title, "Quake 1996");
        assert_eq!(g[0].art_search(), None, "a [[game]] is searched for by the name written");
    }

    #[test]
    fn a_missing_rom_folder_is_a_warning_and_bad_roms_tables_are_errors() {
        let roms = |extra: &str| format!("[[roms]]\npath = \"/definitely/not/here\"\nexec = \"x\"\n{extra}");
        assert!(games(&roms("extensions = [\"sfc\"]")).unwrap().is_empty());
        let r = checked(&roms("extensions = [\"sfc\"]"));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.warnings[0].contains("/definitely/not/here"), "{:?}", r.warnings);

        assert!(game_err(&roms("extensions = []")).contains("extensions is empty"));
        assert!(toml::from_str::<Config>(&roms("extensions = [\"sfc\"]\nextension = 1")).is_err(), "typos are caught");
        let r = checked("[[roms]]\npath = \"{nope}\"\nextensions = [\"sfc\"]\nexec = \"x\"\n");
        assert!(r.errors[0].contains("[[roms]]") && r.errors[0].contains("{nope}"), "{:?}", r.errors);
    }
}
