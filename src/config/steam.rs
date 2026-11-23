//! `[steam]`: every installed Steam game as a game, read from Steam's own files.

use super::launcher::Importer;
use super::{literal, Config, Origin};
use crate::vdf;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// The variables every Steam game gets: its app id, and the full path of its folder.
const APPID: &str = "appid";
const INSTALLDIR: &str = "installdir";

/// Name prefixes of what Steam installs like a game but isn't one: Proton, the Steam
/// Linux Runtime, the redistributables. A `games.<appid>` table brings one back.
const TOOLS: [&str; 3] = ["Proton ", "Steam Linux Runtime", "Steamworks "];

/// The folder name of the Steam flatpak, which needs its own `exec`.
const FLATPAK: &str = "com.valvesoftware.Steam";

/// An installed app, from its `appmanifest_<appid>.acf`.
struct App {
    appid: u64,
    name: String,
    /// The full path of the game's folder.
    installdir: PathBuf,
}

/// Turns every installed Steam game into a game in `cfg.games`, see `Importer::game`
/// for how the keys merge. Returns warnings: Steam not being installed, a library that
/// can't be read (a drive that isn't plugged in) and a `games.<appid>` that isn't installed.
pub(super) fn add_steam(cfg: &mut Config) -> Result<Vec<String>, String> {
    let Some(s) = cfg.steam.take() else { return Ok(Vec::new()) };
    let appid = |k: &str| k.parse::<u64>().map_err(|_| "not a Steam app id (the number in the game's store URL)".to_string());
    let mut import = Importer::new(Origin::Steam, s, &cfg.templates, &cfg.vars, appid)?;
    let mut warnings = Vec::new();
    let root = match import.path.clone() {
        Some(p) => p,
        None => {
            let folders = steam_folders();
            match folders.iter().find(|d| d.join("steamapps").is_dir()) {
                Some(d) => d.clone(),
                None => {
                    let looked: Vec<String> = folders.iter().map(|d| d.display().to_string()).collect();
                    warnings.push(format!(
                        "[steam]: Steam isn't installed in {}; set path if it's somewhere else",
                        looked.join(", ")
                    ));
                    return Ok(warnings);
                }
            }
        }
    };
    let exec = default_exec(&root);

    for app in installed_apps(&root, &mut warnings) {
        if !import.is_pinned(&app.appid) && TOOLS.iter().any(|t| app.name.starts_with(t)) {
            continue;
        }
        let mut g = import.game(&app.appid, &plain_name(&app.name), exec, &format!("{{{INSTALLDIR}}}"));
        g.steam_appid = Some(app.appid);
        g.vars.insert(APPID.to_string(), toml::Value::String(app.appid.to_string()));
        g.vars.insert(INSTALLDIR.to_string(), toml::Value::String(literal(&app.installdir.to_string_lossy())));
        cfg.games.push(g);
    }
    warnings.extend(import.unused("no installed Steam game has this app id"));
    Ok(warnings)
}

/// Where Steam usually is, in the order they're tried: the native install, its
/// ~/.steam link, then the flatpak. On Windows, under Program Files.
fn steam_folders() -> Vec<PathBuf> {
    if cfg!(windows) {
        let programs = std::env::var_os("ProgramFiles(x86)").map_or_else(|| PathBuf::from(r"C:\Program Files (x86)"), PathBuf::from);
        return vec![programs.join("Steam")];
    }
    let Some(dirs) = directories::BaseDirs::new() else { return Vec::new() };
    let home = dirs.home_dir();
    vec![
        dirs.data_dir().join("Steam"),
        home.join(".steam/steam"),
        home.join(".var/app").join(FLATPAK).join(".local/share/Steam"),
    ]
}

/// The command that starts a game through the Steam client at `root`.
fn default_exec(root: &Path) -> &'static str {
    if cfg!(windows) {
        r#"start "" steam://rungameid/{appid}"#
    } else if root.components().any(|c| c.as_os_str() == FLATPAK) {
        "flatpak run com.valvesoftware.Steam steam://rungameid/{appid}"
    } else {
        "steam steam://rungameid/{appid}"
    }
}

/// Steam's name for a game without ™, ® and ©, which searching IGDB and the grid
/// by name doesn't expect.
fn plain_name(name: &str) -> String {
    name.replace(['™', '®', '©'], "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every app installed in the libraries of the Steam folder `root`, by app id. A
/// library that can't be read or a manifest that's broken is a warning.
fn installed_apps(root: &Path, warnings: &mut Vec<String>) -> Vec<App> {
    let mut apps = BTreeMap::new();
    for library in libraries(root, warnings) {
        let steamapps = library.join("steamapps");
        let entries = fs::read_dir(&steamapps).and_then(|entries| entries.map(|e| Ok(e?.path())).collect::<Result<Vec<_>, _>>());
        let mut manifests = match entries {
            Ok(files) => files,
            Err(e) => {
                warnings.push(format!("[steam]: can't read the library {}: {e}", steamapps.display()));
                continue;
            }
        };
        manifests.retain(|f| {
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.starts_with("appmanifest_") && name.ends_with(".acf")
        });
        manifests.sort();
        for m in manifests {
            match read_manifest(&m, &steamapps) {
                // a game in two libraries has a stale manifest in one; the first wins
                Ok(app) => {
                    apps.entry(app.appid).or_insert(app);
                }
                Err(e) => warnings.push(format!("[steam]: {}: {e}, skipped", m.display())),
            }
        }
    }
    apps.into_values().collect()
}

fn read_manifest(path: &Path, steamapps: &Path) -> Result<App, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let top = vdf::parse(&text)?;
    let state = top.block("AppState").ok_or("no AppState")?;
    let field = |key: &str| state.text(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("no {key}"));
    let appid = field("appid")?;
    Ok(App {
        appid: appid.parse().map_err(|_| format!("appid {appid:?} isn't a number"))?,
        name: field("name")?.to_string(),
        installdir: steamapps.join("common").join(field("installdir")?),
    })
}

/// The Steam folder `root` and the library folders its `libraryfolders.vdf` lists,
/// each once.
fn libraries(root: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let mut libraries = vec![root.to_path_buf()];
    let file = root.join("steamapps/libraryfolders.vdf");
    let listed = match fs::read_to_string(&file) {
        Ok(text) => vdf::parse(&text),
        // only the Steam folder, then
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vdf::Block::default()),
        Err(e) => Err(e.to_string()),
    };
    match listed {
        Ok(top) => {
            for (key, value) in top.block("libraryfolders").into_iter().flat_map(vdf::Block::iter) {
                // "0" { "path" "..." }, or "1" "..." from before 2021; other keys are stats
                if key.parse::<u32>().is_err() {
                    continue;
                }
                let path = match value {
                    vdf::Value::Block(b) => b.text("path"),
                    vdf::Value::Text(t) => Some(t.as_str()),
                };
                libraries.extend(path.filter(|p| !p.is_empty()).map(PathBuf::from));
            }
        }
        Err(e) => warnings.push(format!("[steam]: {}: {e}; only reading the games in {}", file.display(), root.display())),
    }
    // the Steam folder is listed too, often under another path (~/.steam/steam is a link)
    let mut seen = BTreeSet::new();
    libraries.retain(|l| seen.insert(fs::canonicalize(l).unwrap_or_else(|_| l.clone())));
    libraries
}

#[cfg(test)]
mod tests {
    use super::super::{check_config, parse_games, ArtId, Game, MenuAction};
    use super::*;

    /// A Steam folder in a new temporary folder `name`, with a second library, both
    /// with these `(library, appid, name)` apps installed. Returns the Steam folder.
    fn fake_steam(name: &str, apps: &[(usize, u64, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rsgl-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let libraries = [dir.join("Steam"), dir.join("Games")];
        for l in &libraries {
            fs::create_dir_all(l.join("steamapps/common")).unwrap();
        }
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t{:?}\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t{:?}\n\t}}\n}}\n",
            libraries[0].display(),
            libraries[1].display()
        );
        fs::write(libraries[0].join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        for &(l, appid, name) in apps {
            let installdir = name.replace(['™', ' '], "");
            let acf = format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"{installdir}\"\n}}\n"
            );
            fs::write(libraries[l].join(format!("steamapps/appmanifest_{appid}.acf")), acf).unwrap();
            fs::create_dir_all(libraries[l].join("steamapps/common").join(installdir)).unwrap();
        }
        libraries[0].clone()
    }

    fn get<'a>(games: &'a [Game], name: &str) -> &'a Game {
        games.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no {name:?}"))
    }

    #[test]
    fn installed_games_in_every_library_become_games() {
        let steam = fake_steam(
            "steam-test",
            &[(0, 620, "Portal 2"), (1, 374320, "DARK SOULS™ III"), (0, 1493710, "Proton Experimental"), (1, 228980, "Steamworks Common Redistributables")],
        );
        fs::write(steam.join("steamapps/appmanifest_1.acf"), "\"AppState\" { \"appid\" ").unwrap();
        let text = format!(
            r#"
[templates.default]
workdir = "/never"
tags = ["never"]

[steam]
path = {:?}
tags = ["steam"]
"#,
            steam.display()
        );
        let games = parse_games(&text).unwrap();
        let names: Vec<_> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["DARK SOULS III", "Portal 2"], "Proton and the redistributables are skipped");

        let ds3 = get(&games, "DARK SOULS III");
        assert_eq!(ds3.exec, ["steam steam://rungameid/374320"]);
        let installdir = steam.parent().unwrap().join("Games/steamapps/common/DARKSOULSIII");
        assert_eq!(ds3.workdir.as_deref(), Some(installdir.as_path()), "the game's folder, in the second library");
        assert!(ds3.installed());
        assert_eq!(ds3.tags, ["steam"], "[templates.default] isn't applied");
        assert!(!ds3.playtime, "steam:// returns right away");
        assert!(ds3.art_id == Some(ArtId::Steam(374320)));

        let report = check_config(toml::from_str(&text).unwrap());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.iter().any(|w| w.contains("appmanifest_1.acf") && w.contains("skipped")), "{:?}", report.warnings);
        fs::remove_dir_all(steam.parent().unwrap()).unwrap();
    }

    #[test]
    fn games_tables_merge_over_steam_and_its_templates() {
        let steam = fake_steam("steam-games-test", &[(0, 620, "Portal 2"), (0, 570, "Dota 2"), (1, 1493710, "Proton Experimental")]);
        let text = format!(
            r#"
[templates.gs]
exec = "gamescope -- {{steam}}"
tags = ["gs"]
vars = {{ steam = "steam -applaunch {{appid}}" }}

[templates.coop]
tags = ["coop"]
env = {{ COOP = "1" }}

[[right_click]]
title = "Folder"
exec = "xdg-open {{installdir}}"

[steam]
path = {:?}
template = "gs"
tags = ["steam"]
env = {{ A = "steam" }}
games.620 = {{ name = "Portal Two", template = "coop", tags = ["puzzle"], playtime = true }}
games.570 = {{ hidden = true }}
games.1493710 = {{ tags = ["tool"] }}
games.42 = {{ hidden = true }}
"#,
            steam.display()
        );
        let games = parse_games(&text).unwrap();
        let names: Vec<_> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Portal Two", "Proton Experimental"], "renamed, hidden, and a tool brought back");

        let portal = get(&games, "Portal Two");
        assert_eq!(portal.exec, ["gamescope -- steam -applaunch 620"]);
        assert_eq!(portal.tags, ["gs", "steam", "coop", "puzzle"], "tags add up");
        assert_eq!((portal.env.all["A"].as_str(), portal.env.all["COOP"].as_str()), ("steam", "1"));
        assert!(portal.playtime);
        let folder = steam.join("steamapps/common/Portal2");
        assert_eq!(portal.menu[0].action, MenuAction::Exec(vec![format!("xdg-open {}", folder.display())]));

        let report = check_config(toml::from_str(&text).unwrap());
        assert_eq!(report.warnings, ["[steam] games.42: no installed Steam game has this app id"]);
        assert_eq!(report.hidden, 1, "dota; 42 isn't installed");

        let err = |from: &str, to: &str| parse_games(&text.replace(from, to)).map(|_| ()).unwrap_err();
        assert_eq!(err("games.42", "games.portal"), "[steam] games.portal: not a Steam app id (the number in the game's store URL)");
        assert_eq!(err("games.42 = {", "games.42 = { template = \"nope\","), "[steam] games.42: no template \"nope\"");
        assert_eq!(err("template = \"gs\"", "template = \"nope\""), "[steam]: no template \"nope\"");
        assert!(err("games.42 = {", "games.42 = { steam_appid = 1,").contains("unknown field `steam_appid`"));
        fs::remove_dir_all(steam.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_game_with_an_imported_steam_appid_is_kept_with_a_warning() {
        let steam = fake_steam("steam-twice-test", &[(0, 548430, "Deep Rock Galactic")]);
        let text = format!(
            r#"
[steam]
path = {:?}

[[game]]
name = "DRG (Wine)"
exec = "wine FSD.exe"
steam_appid = 548430
"#,
            steam.display()
        );
        let games = parse_games(&text).unwrap();
        assert_eq!(games.len(), 2);
        let report = check_config(toml::from_str(&text).unwrap());
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(report.warnings[0].starts_with("game \"DRG (Wine)\" has steam_appid 548430, which [steam] imports too"));

        let hidden = text.replace("path =", "games.548430 = { hidden = true }\npath =");
        assert!(check_config(toml::from_str(&hidden).unwrap()).warnings.is_empty(), "hiding one is the fix");
        fs::remove_dir_all(steam.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_steam_is_a_warning_and_the_rest_still_loads() {
        let text = "[steam]\npath = \"/no/such/steam\"\n\n[[game]]\nname = \"A\"\nexec = \"a\"\n";
        assert_eq!(parse_games(text).unwrap().len(), 1);
        let report = check_config(toml::from_str(text).unwrap());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(report.warnings[0].starts_with("[steam]: can't read the library /no/such/steam/steamapps"), "{:?}", report.warnings);
    }

    #[test]
    fn the_flatpak_gets_its_own_exec_and_names_lose_trademarks() {
        let flatpak = Path::new("/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam");
        if !cfg!(windows) {
            assert!(default_exec(flatpak).starts_with("flatpak run com.valvesoftware.Steam "));
            assert!(default_exec(Path::new("/home/u/.local/share/Steam")).starts_with("steam "));
        }
        assert_eq!(plain_name("Tom Clancy's Rainbow Six® Siege"), "Tom Clancy's Rainbow Six Siege");
        assert_eq!(plain_name("DARK SOULS™ III"), "DARK SOULS III");
    }
}
