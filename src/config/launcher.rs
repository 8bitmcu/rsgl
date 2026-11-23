//! What `[steam]` and `[lutris]` share: their keys, the `games.<key>` tables that set
//! one game's keys, how templates layer under them, and the warning for a game that's
//! shown twice.

use super::{exec_list, expand, literal, opt_exec_list, Config, GameCfg, MenuCfg, Origin, TemplateCfg, Vars};
use super::{lutris::add_lutris, steam::add_steam};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Display,
    path::PathBuf,
};

/// `[steam]` or `[lutris]`: the launcher's installed games become games. The keys are
/// like `[[roms]]`', and each launcher adds its own variables.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LauncherCfg {
    /// The launcher's folder. Left out, it's looked for in the usual places.
    path: Option<String>,
    /// Like a game's, except `[templates.default]` isn't one of them.
    #[serde(default, deserialize_with = "exec_list")]
    template: Vec<String>,
    /// Defaults to the launcher's own command.
    #[serde(default, deserialize_with = "opt_exec_list")]
    exec: Option<Vec<String>>,
    /// Defaults to the game's folder.
    workdir: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    /// Defaults to false: the launcher's command returns before the game does.
    playtime: Option<bool>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
    /// Settings for single games, by the launcher's key for them, see `GamePin`.
    #[serde(default)]
    games: BTreeMap<String, GamePin>,
}

/// `games.<key>`: one game's own keys, over the launcher's the way a `[[game]]`'s are
/// over its templates. Any `[[game]]` key except `steam_appid`, which the launcher sets.
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct GamePin {
    /// Replaces the launcher's name; playtime, art and --launch go by it.
    name: Option<String>,
    /// Applied over the launcher's keys and templates, under these keys.
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
    steamgriddb_id: Option<u64>,
    grid_id: Option<u64>,
    hero_id: Option<u64>,
    igdb_id: Option<u64>,
    #[serde(default)]
    vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    right_click: Vec<MenuCfg>,
}

/// Adds the games of every launcher that's configured, then warns about a game that's
/// shown twice. An error in one launcher doesn't stop the others from being read, so
/// `--check` sees them all.
pub(super) fn add_launchers(cfg: &mut Config) -> Imported {
    let mut imported = Imported::default();
    for add in [add_steam, add_lutris] {
        match add(cfg) {
            Ok(w) => imported.warnings.extend(w),
            Err(e) => imported.errors.push(e),
        }
    }
    imported.warnings.extend(shown_twice(&cfg.games));
    imported
}

/// What `add_launchers` found wrong. Errors stop the library from loading.
#[derive(Default)]
pub(super) struct Imported {
    pub(super) warnings: Vec<String>,
    pub(super) errors: Vec<String>,
}

/// Makes a launcher's games out of its `LauncherCfg`, keyed by `K` in `games.<key>`.
pub(super) struct Importer<'a, K> {
    origin: Origin,
    base: TemplateCfg,
    pins: BTreeMap<K, GamePin>,
    /// The pins a game used, to warn about the rest.
    used: BTreeSet<K>,
    templates: &'a BTreeMap<String, TemplateCfg>,
    /// `path`, with `[vars]` and `~` filled in.
    pub(super) path: Option<PathBuf>,
}

impl<'a, K: Ord + Clone + Display> Importer<'a, K> {
    /// Checks `l`'s templates and `games.<key>` tables, each key read by `key`. Every
    /// error is one, rather than a warning once the launcher is found, so a typo shows
    /// whether or not the launcher is installed.
    pub(super) fn new(
        origin: Origin,
        l: LauncherCfg,
        templates: &'a BTreeMap<String, TemplateCfg>,
        vars: &BTreeMap<String, toml::Value>,
        key: impl Fn(&str) -> Result<K, String>,
    ) -> Result<Self, String> {
        let section = origin.section();
        let mut pins = BTreeMap::new();
        for (k, pin) in l.games {
            let at = format!("{section} games.{k}");
            let k = key(&k).map_err(|e| format!("{at}: {e}"))?;
            // checked up front, since a game that isn't installed is never merged
            if let Some(t) = pin.template.iter().find(|t| !templates.contains_key(*t)) {
                return Err(format!("{at}: no template {t:?}"));
            }
            pins.insert(k, pin);
        }
        let own = TemplateCfg {
            exec: l.exec,
            cover: None,
            workdir: l.workdir,
            env: l.env,
            playtime: l.playtime,
            tags: l.tags,
            vars: l.vars,
            right_click: l.right_click,
        };
        let base = TemplateCfg::default()
            .with_templates(&l.template, templates)
            .map_err(|e| format!("{section}: {e}"))?
            .under(own);
        let path = match &l.path {
            Some(p) => {
                let vars = Vars::default().with(vars).map_err(|e| format!("[vars]: {e}"))?;
                let p = expand(&vars.expand(p).map_err(|e| format!("{section}, path: {e}"))?);
                if p.is_empty() {
                    return Err(format!("{section}: path is empty; leave it out to look in the usual places"));
                }
                Some(PathBuf::from(p))
            }
            None => None,
        };
        Ok(Importer { origin, base, pins, used: BTreeSet::new(), templates, path })
    }

    /// Whether `games.<key>` sets anything for this game.
    pub(super) fn is_pinned(&self, key: &K) -> bool {
        self.pins.contains_key(key)
    }

    /// The game `key`, named `name` by the launcher: the launcher's templates, its keys,
    /// the game's `games.<key>` templates, then its `games.<key>` keys, each over the
    /// ones before. What's still unset gets `exec`, `workdir` and no playtime tracking.
    pub(super) fn game(&mut self, key: &K, name: &str, exec: &str, workdir: &str) -> GameCfg {
        let pin = self.pins.get(key).cloned();
        if pin.is_some() {
            self.used.insert(key.clone());
        }
        let pin = pin.unwrap_or_default();
        let layer = self.base.clone().with_templates(&pin.template, self.templates).expect("its templates were checked in new");
        let mut g = GameCfg {
            name: pin.name.unwrap_or_else(|| literal(name)),
            template: Vec::new(),
            exec: pin.exec,
            cover: pin.cover,
            workdir: pin.workdir,
            env: pin.env,
            playtime: pin.playtime,
            hidden: pin.hidden,
            tags: pin.tags,
            steam_appid: None,
            steamgriddb_id: pin.steamgriddb_id,
            grid_id: pin.grid_id,
            hero_id: pin.hero_id,
            igdb_id: pin.igdb_id,
            vars: pin.vars,
            right_click: pin.right_click,
            origin: self.origin,
            pin: Some(format!("{} games.{key}", self.origin.section())),
            title: None,
            igdb_platform: None,
            source: None,
        }
        .over(layer);
        g.exec.get_or_insert_with(|| vec![exec.to_string()]);
        g.workdir.get_or_insert_with(|| workdir.to_string());
        // the launcher's command returns right away, so tracking would only log a few seconds
        g.playtime.get_or_insert(false);
        g
    }

    /// A warning for each `games.<key>` no installed game used, saying `why`.
    pub(super) fn unused(&self, why: &str) -> Vec<String> {
        let section = self.origin.section();
        let unused = self.pins.keys().filter(|k| !self.used.contains(*k));
        unused.map(|k| format!("{section} games.{k}: {why}")).collect()
    }
}

/// A warning for each game that's shown next to an imported game with the same
/// `steam_appid`: a `[[game]]` next to a Steam or Lutris import, or a Lutris import of a
/// Steam game next to `[steam]`'s. Both are kept, see the README.
fn shown_twice(games: &[GameCfg]) -> Vec<String> {
    let shown = |g: &&GameCfg| !g.hidden && g.steam_appid.is_some();
    let imports: Vec<&GameCfg> = games.iter().filter(shown).filter(|g| g.pin.is_some()).collect();
    let mut warnings = Vec::new();
    for g in games.iter().filter(shown) {
        let appid = g.steam_appid.expect("filtered on it");
        // a [[game]] against either import, Steam first; a Lutris import against Steam's
        let other = |origin: Origin| imports.iter().find(|i| i.origin == origin && i.steam_appid == Some(appid));
        let twin = match g.origin {
            Origin::Game => other(Origin::Steam).or_else(|| other(Origin::Lutris)),
            Origin::Lutris => other(Origin::Steam),
            Origin::Rom | Origin::Steam => None,
        };
        let Some(twin) = twin else { continue };
        let (section, pin) = (twin.origin.section(), twin.pin.as_deref().expect("imports have one"));
        let msg = match &g.pin {
            None => format!(
                "game {:?} has steam_appid {appid}, which {section} imports too (as {:?}), so it's shown twice; \
                 hide one of them, the import with {pin} = {{ hidden = true }}",
                g.name, twin.name
            ),
            Some(own) => format!(
                "{} imports {:?}, Steam app {appid}, which {section} imports too (as {:?}), so it's shown twice; \
                 hide one of them with {own} = {{ hidden = true }} or {pin} = {{ hidden = true }}",
                g.origin.section(),
                g.name,
                twin.name
            ),
        };
        warnings.push(g.at(msg));
    }
    warnings
}
