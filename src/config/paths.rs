//! Where rsgl keeps its cache and playtime (`[paths]`), and `~` in paths.

use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct PathsCfg {
    cache: Option<String>,
    playtime: Option<String>,
}

/// [paths] resolved; `None` is the default location.
#[derive(Debug, PartialEq)]
pub(super) struct Paths {
    pub(super) cache: Option<PathBuf>,
    pub(super) playtime: Option<PathBuf>,
}

/// Set from [paths] by every load, so F5 picks up a change too.
pub(super) static PATHS: RwLock<Paths> = RwLock::new(Paths { cache: None, playtime: None });

impl PathsCfg {
    /// Expands `~` and takes relative paths from `base`, the config file's folder, so a
    /// portable setup works no matter which folder rsgl is started from.
    pub(super) fn resolve(&self, base: &Path) -> Result<Paths, String> {
        let path = |key, value: &Option<String>| match value.as_deref() {
            None => Ok(None),
            Some("") => Err(format!("[paths]: {key} is empty")),
            Some(s) => Ok(Some(base.join(expand(s)))),
        };
        Ok(Paths { cache: path("cache", &self.cache)?, playtime: path("playtime", &self.playtime)? })
    }
}

/// The folder of the config file at `path`, which relative [paths] start from.
pub(super) fn config_folder(path: &Path) -> Result<PathBuf, String> {
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

/// Looks for `dir/<slug>.{png,jpg,webp}`.
fn find_image(dir: &Path, name: &str) -> Option<PathBuf> {
    ["png", "jpg", "webp"]
        .iter()
        .map(|ext| dir.join(format!("{}.{ext}", slug(name))))
        .find(|p| p.exists())
}

pub fn find_cover(name: &str) -> Option<PathBuf> {
    find_image(&covers_dir(), name)
}

pub fn find_hero(name: &str) -> Option<PathBuf> {
    find_image(&heroes_dir(), name)
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
