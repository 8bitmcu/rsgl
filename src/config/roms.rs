//! `[[roms]]`: every file in a folder as a game.

use super::{apply_templates, exec_list, expand, literal, opt_exec_list, Config, GameCfg, Origin, Vars};
use crate::api_hasheous;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
};

/// `[[roms]]`: a folder where every file with one of `extensions` becomes a game,
/// see `add_roms`. The other keys are like a game's, and `exec` gets the file as `{rom}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RomsCfg {
    path: String,
    extensions: Vec<String>,
    /// Like a game's, except `[templates.default]` isn't one of them.
    #[serde(default, deserialize_with = "exec_list")]
    template: Vec<String>,
    #[serde(default, deserialize_with = "opt_exec_list")]
    exec: Option<Vec<String>>,
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
    /// The included file this table came from; `None` for the top-level config file.
    #[serde(skip)]
    pub(super) source: Option<PathBuf>,
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
pub(super) struct Scan {
    pub(super) warnings: Vec<String>,
    /// The files `known` has nothing about yet.
    pub(super) unidentified: Vec<PathBuf>,
}

/// Turns every `[[roms]]` folder's files into games in `cfg.games`. A folder that
/// can't be read (a drive that isn't plugged in) is a warning rather than an error,
/// so the rest of the library still loads. With `known` (Hasheous is on), a file it
/// identified gets the game's name and ids from there instead of from its file name.
pub(super) fn add_roms(cfg: &mut Config, known: Option<&api_hasheous::Known>) -> Result<Scan, String> {
    let mut scan = Scan::default();
    let warnings = &mut scan.warnings;
    if cfg.roms.is_empty() {
        return Ok(scan);
    }
    let vars = Vars::default().with(&cfg.vars).map_err(|e| format!("[vars]: {e}"))?;
    for r in std::mem::take(&mut cfg.roms) {
        let who = match &r.source {
            Some(p) => format!("{}: [[roms]] {:?}", p.display(), r.path),
            None => format!("[[roms]] {:?}", r.path),
        };
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
        // what every file gets; the folder is the workdir unless the block or a template sets one
        let own = GameCfg {
            name: String::new(),
            template: Vec::new(),
            exec: r.exec,
            cover: None,
            workdir: r.workdir,
            env: r.env,
            playtime: None,
            hidden: false,
            tags: r.tags,
            steam_appid: None,
            steamgriddb_id: None,
            grid_id: None,
            hero_id: None,
            igdb_id: None,
            vars: r.vars,
            right_click: Vec::new(),
            origin: Origin::Rom,
            pin: None,
            title: r.name,
            igdb_platform: r.igdb_platform.map(|p| p.trim().to_string()),
            source: r.source,
        };
        let mut base = apply_templates(own, &r.template, &cfg.templates).map_err(|e| format!("{who}: {e}"))?;
        base.workdir.get_or_insert_with(|| literal(&dir));
        match &base.exec {
            None => return Err(format!("{who}: needs an exec, here or in a template")),
            Some(e) if e.is_empty() => return Err(format!("{who}: exec is empty")),
            Some(_) => {}
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
            let mut game = base.clone();
            game.name = literal(found.map_or(&rom_name(stem), |f| &f.name));
            game.hidden = pin.hidden;
            game.steamgriddb_id = pin.steamgriddb_id.or(found.and_then(|f| f.steamgriddb_id));
            game.grid_id = pin.grid_id;
            game.hero_id = pin.hero_id;
            game.igdb_id = pin.igdb_id.or(found.and_then(|f| f.igdb_id));
            game.vars.insert(ROM.to_string(), toml::Value::String(literal(full)));
            cfg.games.push(game);
        }
    }
    Ok(scan)
}

/// A ROM's file stem as a game name, without the `(USA)`, `(Rev 1)` and `[!]` groups
/// ROM sets add, so SteamGridDB and IGDB find it by name. A stem that's nothing but
/// groups is kept as it is.
pub(super) fn rom_name(stem: &str) -> String {
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
