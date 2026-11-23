//! `[lutris]`: every game installed through Lutris as a game, read from its `pga.db`.
//! Its keys are `LauncherCfg`'s, and each game gets `{id}`, `{slug}`, `{runner}`,
//! `{platform}` and `{directory}`.

use super::launcher::Importer;
use super::{literal, Config, Origin};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// The variables every Lutris game gets: Lutris's own id (it changes on a reinstall),
/// its slug, its runner and platform, and the folder it's installed in.
const ID: &str = "id";
const SLUG: &str = "slug";
const RUNNER: &str = "runner";
const PLATFORM: &str = "platform";
const DIRECTORY: &str = "directory";

/// The flatpak's id, which needs its own `exec`.
const FLATPAK: &str = "net.lutris.Lutris";

/// Lutris's game library, in its data folder.
const DB: &str = "pga.db";

/// The installed games and what rsgl uses of them. Lutris has the column names since
/// 0.5.8; `service` is where an imported game came from, `steam` for Steam's, and
/// `service_id` its id there, the app id for Steam.
const QUERY: &str = "SELECT id, slug, name, runner, platform, directory, service, service_id \
                     FROM games WHERE installed = 1 ORDER BY id";

/// One row of `QUERY`.
struct LutrisGame {
    id: i64,
    slug: String,
    name: String,
    runner: String,
    platform: String,
    directory: String,
    steam_appid: Option<u64>,
}

/// A Lutris install: its data folder, with `pga.db`, and its cache folder, where newer
/// Lutris keeps the covers. `None` when `path` is set to a folder that isn't a usual one.
struct Install {
    data: PathBuf,
    cache: Option<PathBuf>,
}

/// Turns every game Lutris has installed into a game in `cfg.games`, see `Importer::game`
/// for how the keys merge. The `games.<slug>` table goes by slug, since Lutris's id changes
/// on a reinstall; two installs of a game share it. A Lutris import of a Steam game gets
/// `steam_appid`, so its art is found by app id. Returns warnings: Lutris not being
/// installed, a `pga.db` that can't be read and a `games.<slug>` that isn't installed.
pub(super) fn add_lutris(cfg: &mut Config) -> Result<Vec<String>, String> {
    let Some(l) = cfg.lutris.take() else { return Ok(Vec::new()) };
    let mut import = Importer::new(Origin::Lutris, l, &cfg.templates, &cfg.vars, |k| Ok(k.to_string()))?;
    let installs = lutris_folders();
    let install = match import.path.take() {
        // a usual folder written out still finds its covers in the cache
        Some(data) => {
            let cache = installs.into_iter().find(|i| i.data == data).and_then(|i| i.cache);
            Install { data, cache }
        }
        None => match installs.iter().position(|i| i.data.join(DB).is_file()) {
            Some(n) => installs.into_iter().nth(n).expect("found above"),
            None => {
                let looked: Vec<String> = installs.iter().map(|i| i.data.display().to_string()).collect();
                return Ok(vec![format!(
                    "[lutris]: Lutris isn't installed in {}; set path if it's somewhere else",
                    looked.join(", ")
                )]);
            }
        },
    };
    let db = install.data.join(DB);
    let games = match installed_games(&db) {
        Ok(games) => games,
        Err(e) => return Ok(vec![format!("[lutris]: can't read {}: {e}", db.display())]),
    };
    let exec = default_exec(&install.data);
    for lg in games {
        let mut g = import.game(&lg.slug, &lg.name, exec, &format!("{{{DIRECTORY}}}"));
        g.steam_appid = lg.steam_appid;
        // under a cover the config sets, and `cover = ""` drops it for SteamGridDB's
        if g.cover.is_none() {
            g.cover = cover(&install, &lg.slug).map(|c| literal(&c.to_string_lossy()));
        }
        for (var, value) in [
            (ID, lg.id.to_string()),
            (SLUG, lg.slug),
            (RUNNER, lg.runner),
            (PLATFORM, lg.platform),
            (DIRECTORY, lg.directory),
        ] {
            g.vars.insert(var.to_string(), toml::Value::String(literal(&value)));
        }
        cfg.games.push(g);
    }
    Ok(import.unused("no installed Lutris game has this slug"))
}

/// Where Lutris usually is, in the order they're tried: the native install, then the
/// flatpak. Lutris is Linux-only, so nowhere on Windows.
fn lutris_folders() -> Vec<Install> {
    if cfg!(windows) {
        return Vec::new();
    }
    let Some(dirs) = directories::BaseDirs::new() else { return Vec::new() };
    let flatpak = dirs.home_dir().join(".var/app").join(FLATPAK);
    vec![
        Install { data: dirs.data_dir().join("lutris"), cache: Some(dirs.cache_dir().join("lutris")) },
        Install { data: flatpak.join("data/lutris"), cache: Some(flatpak.join("cache/lutris")) },
    ]
}

/// The command that starts a game through the Lutris whose data folder is `data`.
fn default_exec(data: &Path) -> &'static str {
    if data.components().any(|c| c.as_os_str() == FLATPAK) {
        "flatpak run net.lutris.Lutris lutris:rungameid/{id}"
    } else {
        "lutris lutris:rungameid/{id}"
    }
}

/// Lutris's cover for `slug`: in the cache folder for Lutris 0.5.12 and later, in the
/// data folder before that.
fn cover(install: &Install, slug: &str) -> Option<PathBuf> {
    let folders = install.cache.iter().chain([&install.data]).map(|d| d.join("coverart"));
    folders.flat_map(|d| ["jpg", "png"].map(|ext| d.join(format!("{slug}.{ext}")))).find(|p| p.is_file())
}

/// The installed games in the `pga.db` at `db`, opened read-only so a running Lutris
/// isn't disturbed. A row without a slug can't be told apart in `games.<slug>`, and
/// Lutris always gives one, so such a row is skipped.
fn installed_games(db: &Path) -> rusqlite::Result<Vec<LutrisGame>> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let mut stmt = conn.prepare(QUERY)?;
    let rows = stmt.query_map([], |r| {
        let text = |i: usize| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
        let (slug, name, service, service_id) = (text(1)?, text(2)?, text(6)?, text(7)?);
        Ok(LutrisGame {
            id: r.get(0)?,
            name: if name.is_empty() { slug.clone() } else { name },
            slug,
            runner: text(3)?,
            platform: text(4)?,
            directory: text(5)?,
            steam_appid: (service == "steam").then(|| service_id.parse().ok()).flatten(),
        })
    })?;
    let mut games = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    games.retain(|g| !g.slug.is_empty());
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::super::{check_config, parse_games, ArtId, Game, MenuAction};
    use super::*;
    use std::fs;

    /// Lutris 0.5.17's `games` table, so the query is checked against every column it
    /// has, not just the ones rsgl reads.
    const SCHEMA: &str = "CREATE TABLE games (
        id INTEGER PRIMARY KEY, name TEXT, sortname TEXT, slug TEXT, installer_slug TEXT,
        parent_slug TEXT, platform TEXT, runner TEXT, executable TEXT, directory TEXT,
        updated DATETIME, lastplayed INTEGER, installed INTEGER, installed_at INTEGER,
        year INTEGER, configpath TEXT, has_custom_banner INTEGER, has_custom_icon INTEGER,
        has_custom_coverart_big INTEGER, playtime REAL, service TEXT, service_id TEXT,
        discord_id TEXT)";

    /// `(id, slug, name, runner, installed, service, service_id)`
    type Row<'a> = (i64, &'a str, &'a str, &'a str, bool, Option<&'a str>, Option<&'a str>);

    /// A Lutris data folder in a new temporary folder `name`, with a `pga.db` that has
    /// these games.
    fn fake_lutris(name: &str, games: &[Row]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rsgl-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let data = dir.join("lutris");
        fs::create_dir_all(data.join("coverart")).unwrap();
        let conn = Connection::open(data.join(DB)).unwrap();
        conn.execute(SCHEMA, []).unwrap();
        for &(id, slug, name, runner, installed, service, service_id) in games {
            let directory = dir.join("Games").join(slug);
            fs::create_dir_all(&directory).unwrap();
            conn.execute(
                "INSERT INTO games (id, slug, name, runner, platform, directory, installed, service, service_id)
                 VALUES (?1, ?2, ?3, ?4, 'Linux', ?5, ?6, ?7, ?8)",
                rusqlite::params![id, slug, name, runner, directory.to_string_lossy(), installed, service, service_id],
            )
            .unwrap();
        }
        data
    }

    fn get<'a>(games: &'a [Game], name: &str) -> &'a Game {
        games.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no {name:?}"))
    }

    #[test]
    fn installed_games_become_games_with_their_overrides() {
        let data = fake_lutris(
            "lutris-test",
            &[
                (1, "the-witcher-3", "The Witcher 3 {GOTY}", "wine", true, Some("gog"), Some("1207664643")),
                (2, "portal-2", "Portal 2", "steam", true, Some("steam"), Some("620")),
                (3, "doom", "DOOM", "wine", false, None, None),
                (4, "quake", "Quake", "linux", true, None, None),
                (7, "quake", "Quake {modded}", "linux", true, None, None),
            ],
        );
        fs::write(data.join("coverart/portal-2.jpg"), b"").unwrap();
        let text = format!(
            r#"
[templates.default]
tags = ["never"]

[templates.gs]
exec = "gamescope -- {{lutris}}"
tags = ["gs"]
vars = {{ lutris = "lutris lutris:rungameid/{{id}}" }}

[[right_click]]
title = "Folder"
exec = "xdg-open {{directory}}"

[lutris]
path = {:?}
tags = ["lutris"]
games.the-witcher-3 = {{ name = "Witcher 3", template = "gs", tags = ["rpg"], playtime = true }}
games.quake = {{ tags = ["fps"] }}
games.half-life = {{ hidden = true }}
"#,
            data.display()
        );
        let games = parse_games(&text).unwrap();
        let names: Vec<_> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Portal 2", "Quake", "Quake {modded}", "Witcher 3"], "DOOM isn't installed, braces are kept");

        let witcher = get(&games, "Witcher 3");
        assert_eq!(witcher.exec, ["gamescope -- lutris lutris:rungameid/1"]);
        assert_eq!(witcher.tags, ["lutris", "gs", "rpg"], "tags add up, [templates.default] isn't applied");
        assert!(witcher.playtime);
        assert!(witcher.art_id.is_none(), "a GOG game has no Steam app id");
        let folder = data.parent().unwrap().join("Games/the-witcher-3");
        assert_eq!(witcher.workdir.as_deref(), Some(folder.as_path()));
        assert_eq!(witcher.menu[0].action, MenuAction::Exec(vec![format!("xdg-open {}", folder.display())]));

        let portal = get(&games, "Portal 2");
        assert_eq!(portal.exec, ["lutris lutris:rungameid/2"]);
        assert!(!portal.playtime, "lutris: returns right away");
        assert!(portal.art_id == Some(ArtId::Steam(620)));
        assert_eq!(portal.cover.as_deref(), Some(data.join("coverart/portal-2.jpg").as_path()), "Lutris's cover");
        assert_eq!(portal.vars.expand("{slug} {runner} {platform}").unwrap(), "portal-2 steam Linux");

        let quakes: Vec<_> = games.iter().filter(|g| g.name.starts_with("Quake")).collect();
        assert!(quakes.iter().all(|g| g.tags == ["lutris", "fps"]), "both installs share the slug");
        assert_eq!(quakes[1].exec, ["lutris lutris:rungameid/7"]);

        let report = check_config(toml::from_str(&text).unwrap());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.warnings, ["[lutris] games.half-life: no installed Lutris game has this slug"]);

        let err = |from: &str, to: &str| parse_games(&text.replace(from, to)).map(|_| ()).unwrap_err();
        assert_eq!(err("games.quake = {", "games.quake = { template = \"nope\","), "[lutris] games.quake: no template \"nope\"");
        assert!(err("games.quake = {", "games.quake = { steam_appid = 1,").contains("unknown field `steam_appid`"));
        assert_eq!(err("path = ", "path = \"\" #"), "[lutris]: path is empty; leave it out to look in the usual places");
        fs::remove_dir_all(data.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_steam_game_imported_by_lutris_and_steam_is_kept_with_a_warning() {
        let data = fake_lutris("lutris-twice-test", &[(2, "portal-2", "Portal 2", "steam", true, Some("steam"), Some("620"))]);
        let steam = data.parent().unwrap().join("Steam");
        fs::create_dir_all(steam.join("steamapps/common/Portal 2")).unwrap();
        let acf = "\"AppState\" { \"appid\" \"620\" \"name\" \"Portal 2\" \"installdir\" \"Portal 2\" }";
        fs::write(steam.join("steamapps/appmanifest_620.acf"), acf).unwrap();
        let text = format!(
            "[steam]\npath = {:?}\n\n[lutris]\npath = {:?}\n\n[[game]]\nname = \"Portal (native)\"\nexec = \"portal2\"\nsteam_appid = 620\n",
            steam.display(),
            data.display()
        );
        assert_eq!(parse_games(&text).unwrap().len(), 3, "all kept");
        let report = check_config(toml::from_str(&text).unwrap());
        let twice: Vec<_> = report.warnings.iter().filter(|w| w.contains("shown twice")).collect();
        assert_eq!(twice.len(), 2, "{:?}", report.warnings);
        assert!(twice[0].starts_with("game \"Portal (native)\" has steam_appid 620, which [steam] imports too"), "{twice:?}");
        assert!(twice[0].ends_with("[steam] games.620 = { hidden = true }"), "{twice:?}");
        assert!(twice[1].starts_with("[lutris] imports \"Portal 2\", Steam app 620, which [steam] imports too"), "{twice:?}");
        assert!(twice[1].ends_with("[lutris] games.portal-2 = { hidden = true } or [steam] games.620 = { hidden = true }"));

        let hidden = text.replace("[steam]\n", "[steam]\ngames.620 = { hidden = true }\n");
        let report = check_config(toml::from_str(&hidden).unwrap());
        let twice: Vec<_> = report.warnings.iter().filter(|w| w.contains("shown twice")).collect();
        assert_eq!(twice.len(), 1, "the [[game]] against Lutris's, once Steam's is hidden: {twice:?}");
        assert!(twice[0].contains("which [lutris] imports too"), "{twice:?}");
        fs::remove_dir_all(data.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_or_broken_lutris_is_a_warning_and_the_rest_still_loads() {
        let dir = std::env::temp_dir().join(format!("rsgl-lutris-broken-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(DB), "not a database").unwrap();
        for (path, warning) in [(dir.display().to_string(), "[lutris]: can't read "), ("/no/such/lutris".into(), "[lutris]: can't read /no/such/lutris/pga.db")] {
            let text = format!("[lutris]\npath = {path:?}\n\n[[game]]\nname = \"A\"\nexec = \"a\"\n");
            assert_eq!(parse_games(&text).unwrap().len(), 1);
            let report = check_config(toml::from_str(&text).unwrap());
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
            assert!(report.warnings[0].starts_with(warning), "{:?}", report.warnings);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_flatpak_gets_its_own_exec() {
        assert!(default_exec(Path::new("/home/u/.var/app/net.lutris.Lutris/data/lutris")).starts_with("flatpak run net.lutris.Lutris "));
        assert!(default_exec(Path::new("/home/u/.local/share/lutris")).starts_with("lutris "));
    }
}
