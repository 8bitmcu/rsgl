use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

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

[[game]]
name = "Example Game"
exec = "~/games/example/start.sh"
# cover = "~/.cache/rsgl/covers/example.png"
# workdir = "~/games/example"
# env = { SOME_VAR = "value" }
# playtime = false                             # don't track this game
# hidden = true                                # keep the entry but leave it out of the grid
# steam_appid = 367520                         # pick covers by Steam app id instead of by name
# steamgriddb_id = 1234                        # or by SteamGridDB game id
"#;

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    ui: UiCfg,
    #[serde(default)]
    api: ApiCfg,
    #[serde(default)]
    playtime: PlaytimeCfg,
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
    steam_appid: Option<u64>,
    steamgriddb_id: Option<u64>,
}

fn yes() -> bool {
    true
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
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    match (s.strip_prefix("~/"), s == "~", home) {
        (Some(rest), _, Some(h)) => h.join(rest).to_string_lossy().into_owned(),
        (_, true, Some(h)) => h.to_string_lossy().into_owned(),
        _ => s.to_string(),
    }
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
/// 1. $XDG_CONFIG_HOME/rsgl/rsgl.toml (usually ~/.config/rsgl/rsgl.toml)
/// 2. ~/.rsgl.toml
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = directories::BaseDirs::new() {
        v.push(d.config_dir().join("rsgl/rsgl.toml"));
        v.push(d.home_dir().join(".rsgl.toml"));
    }
    v
}

/// Finds and parses the config. If no file exists, writes an example to the
/// first location and returns an error telling the user about it.
pub fn load() -> Result<(Vec<Game>, UiCfg, ApiCfg, PlaytimeCfg), String> {
    let candidates = candidates();

    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
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

    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let cfg: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let ui = cfg.ui;
    let api = cfg.api;
    let playtime = cfg.playtime;

    let mut games: Vec<Game> = cfg
        .games
        .into_iter()
        .filter(|g| !g.hidden)
        .map(|g| {
            // a configured cover that doesn't exist falls back to the covers dir,
            // otherwise a downloaded cover would never be picked up again
            let cover = g
                .cover
                .map(|c| PathBuf::from(expand(&c)))
                .filter(|p| p.exists())
                .or_else(|| find_cover(&g.name));
            Game {
                name: g.name,
                exec: expand(&g.exec),
                cover,
                workdir: g.workdir.map(|w| PathBuf::from(expand(&w))),
                env: g.env.into_iter().map(|(k, v)| (k, expand(&v))).collect(),
                playtime: g.playtime,
                background: None,
                // the SteamGridDB id is the more direct one if both are set
                art_id: g
                    .steamgriddb_id
                    .map(ArtId::SteamGridDb)
                    .or(g.steam_appid.map(ArtId::Steam)),
            }
        })
        .collect();

    games.sort_by_cached_key(|g| g.name.to_lowercase());
    Ok((games, ui, api, playtime))
}
