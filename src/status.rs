//! The status bar under the grid: the `[status]` parts, filled in for the selected game.
//! The top bar's labels are parts too, see `topbar`.
use crate::{
    config::{tidy, Env, Game, Toggles, Vars, YEAR},
    playtime,
};
use serde::Deserialize;
use std::collections::BTreeSet;

/// The variable that holds the game's genres from IGDB.
const GENRES: &str = "genres";
/// The variables rsgl fills in, under `[vars]` and the game's own `vars`.
const BUILTINS: &[&str] =
    &["title", "name", YEAR, GENRES, "playtime", "last_played", "not_installed", "tags", "shown", "total"];

#[derive(Deserialize)]
#[serde(default)]
pub struct StatusCfg {
    /// Goes between the parts that are shown.
    separator: String,
    left: Vec<PartCfg>,
    right: Vec<PartCfg>,
}

#[derive(Deserialize)]
pub struct PartCfg {
    pub text: String,
    #[serde(default)]
    pub bold: bool,
}

/// The status bar rsgl always had.
impl Default for StatusCfg {
    fn default() -> Self {
        let part = |text: &str, bold| PartCfg { text: text.into(), bold };
        Self {
            separator: " · ".into(),
            left: vec![
                part("{title}", true),
                part("{playtime} · last played {last_played}", false),
                part("{not_installed}", false),
                part("{tags}", false),
            ],
            right: vec![part("{shown} games", false)],
        }
    }
}

pub struct Status {
    pub separator: String,
    pub left: Vec<Part>,
    pub right: Vec<Part>,
    /// Some part uses `{year}` or `{genres}`, so every game needs its IGDB data.
    pub uses_igdb: bool,
}

pub struct Part {
    text: String,
    pub bold: bool,
    /// `text` with every variable blank, and whether it has variables at all:
    /// a part with variables that are all blank is left out.
    bare: String,
    has_vars: bool,
    /// Uses `{year}` or `{genres}`.
    pub uses_igdb: bool,
}

impl Default for Status {
    fn default() -> Self {
        Status::new(StatusCfg::default(), &Checker::new(&BTreeSet::new())).expect("the default status bar is valid")
    }
}

impl Status {
    pub fn new(cfg: StatusCfg, check: &Checker) -> Result<Status, String> {
        let mut uses_igdb = false;
        let mut parts = |side: Vec<PartCfg>, which: &str| -> Result<Vec<Part>, String> {
            let mut out = Vec::new();
            for (i, p) in side.into_iter().enumerate() {
                let part = check.part(p).map_err(|e| format!("{which} part {}: {e}", i + 1))?;
                uses_igdb |= part.uses_igdb;
                out.push(part);
            }
            Ok(out)
        };
        let left = parts(cfg.left, "left")?;
        let right = parts(cfg.right, "right")?;
        Ok(Status { separator: cfg.separator, left, right, uses_igdb })
    }
}

/// Checks part texts, for the status bar and the top bar's labels. `names` are the
/// variables in `[vars]` and in games' own `vars`: a part may use one that only some
/// games define, and is left out for the others.
pub struct Checker {
    /// Every variable blank, every variable set, and every one set but `{year}` and
    /// `{genres}`. `{env:NAME}` follows along, except it's set without IGDB too.
    blank: Vars,
    full: Vars,
    without_igdb: Vars,
}

impl Checker {
    pub fn new(names: &BTreeSet<String>) -> Checker {
        let all = |value: &'static str, igdb: &'static str| {
            let names = BUILTINS.iter().copied().chain(names.iter().map(String::as_str));
            Vars::literal(names.map(|n| (n, if n == YEAR || n == GENRES { igdb } else { value })))
        };
        Checker { blank: all("", ""), full: all("x", "x"), without_igdb: all("x", "") }
    }

    pub fn part(&self, p: PartCfg) -> Result<Part, String> {
        let (x, none) = (|_: &str| "x".to_string(), |_: &str| String::new());
        let filled = self.full.expand_env(&p.text, &x)?;
        let bare = self.blank.expand_env(&p.text, &none).expect("same variables as `full`");
        let uses_igdb = self.without_igdb.expand_env(&p.text, &x).expect("same variables as `full`") != filled;
        Ok(Part { has_vars: filled != bare, bare, text: p.text, bold: p.bold, uses_igdb })
    }
}

impl Part {
    /// The text to show, or None to leave the part out: its variables are all blank,
    /// or one of them isn't defined for this game. `env` looks up `{env:NAME}`.
    pub fn render(&self, vars: &Vars, env: &dyn Fn(&str) -> String) -> Option<String> {
        let text = vars.expand_env(&self.text, env).ok()?;
        (!self.has_vars || text != self.bare).then(|| tidy(&text))
    }
}

/// `{env:NAME}` for `game`: what it would launch with, its own `env`, the top bar's
/// `toggles` and `[env]` on top of rsgl's environment. Blank if it's not set.
pub fn env<'a>(game: Option<&'a Game>, toggles: &'a Toggles) -> impl Fn(&str) -> String + 'a {
    move |name| {
        let none = Env::default();
        match game.map_or(&none, |g| &g.env).get(name, toggles) {
            Some(value) => value.unwrap_or_default().to_string(),
            None => std::env::var(name).unwrap_or_default(),
        }
    }
}

/// The variables for `game` (the selected one, with its playtime if it has any),
/// when `shown` of `total` games match the search. The game's own variables win.
pub fn vars(game: Option<(&Game, Option<&playtime::Entry>)>, shown: usize, total: usize) -> Vars {
    let (shown, total) = (shown.to_string(), total.to_string());
    let counts = [("shown", shown.as_str()), ("total", total.as_str())];
    let Some((g, played)) = game else { return Vars::literal(counts) };

    let meta = g.meta.clone().unwrap_or_default();
    let year = meta.year.map(|y| y.to_string()).unwrap_or_default();
    let genres = meta.genres.join(", ");
    let tags = g.tags.join(", ");
    let (time, last) = played.map(|e| (playtime::played(e), playtime::last_played(e))).unwrap_or_default();
    let not_installed = if g.installed() { "" } else { "not installed" };
    g.vars.with_defaults(
        [
            ("title", g.title.as_str()),
            ("name", &g.name),
            (YEAR, &year),
            (GENRES, &genres),
            ("playtime", &time),
            ("last_played", &last),
            ("not_installed", not_installed),
            ("tags", &tags),
        ]
        .into_iter()
        .chain(counts),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(toml: &str, names: &[&str]) -> Result<Status, String> {
        let cfg: StatusCfg = toml::from_str(toml).unwrap();
        Status::new(cfg, &Checker::new(&names.iter().map(|n| n.to_string()).collect()))
    }

    fn shown(parts: &[Part], vars: &Vars) -> Vec<String> {
        parts.iter().filter_map(|p| p.render(vars, &|_| String::new())).collect()
    }

    #[test]
    fn parts_with_only_blank_variables_are_left_out() {
        let s = status(
            r#"left = [{ text = "{name} ({year})", bold = true }, { text = "{playtime}, last played {last_played}" },
                        { text = "{genres}" }, { text = "hi" }, { text = "{dir}" }]"#,
            &["dir"],
        )
        .unwrap();
        assert!(s.uses_igdb);
        assert_eq!(s.right.len(), 1, "a side that isn't set keeps its default");
        assert!(s.left[0].bold);

        let vars = Vars::literal([("name", "Quake {1}"), ("year", ""), ("playtime", ""), ("last_played", ""), ("genres", "")]);
        assert_eq!(shown(&s.left, &vars), ["Quake {1}", "hi"], "a blank year leaves no (), and {{dir}} isn't defined here");

        let vars = Vars::literal([("name", "Quake"), ("year", "1996"), ("playtime", "3h"), ("last_played", "now"), ("genres", "Shooter"), ("dir", "q")]);
        assert_eq!(shown(&s.left, &vars), ["Quake (1996)", "3h, last played now", "Shooter", "hi", "q"]);
    }

    #[test]
    fn the_default_bar_is_the_old_one_and_skips_igdb() {
        let s = Status::default();
        assert!(!s.uses_igdb);
        let vars = Vars::literal([("title", "Doom"), ("playtime", "2h 5m"), ("last_played", "3 days ago"), ("not_installed", ""), ("tags", "fps, lan"), ("shown", "4")]);
        assert_eq!(shown(&s.left, &vars), ["Doom", "2h 5m · last played 3 days ago", "fps, lan"]);
        assert_eq!(shown(&s.right, &vars), ["4 games"]);
    }

    #[test]
    fn env_is_the_selected_games_then_ours() {
        let s = status(r#"left = [{ text = "MangoHud {env:MANGOHUD}" }, { text = "{env:RSGL_TEST_UNSET}" }]"#, &[]).unwrap();
        assert!(!s.uses_igdb);
        let env = |name: &str| if name == "MANGOHUD" { "1".to_string() } else { String::new() };
        let shown: Vec<_> = s.left.iter().filter_map(|p| p.render(&Vars::default(), &env)).collect();
        assert_eq!(shown, ["MangoHud 1"], "an unset one is blank, so its part is left out");
        assert_eq!(super::env(None, &Toggles::new())("RSGL_TEST_UNSET"), "");
        assert_eq!(super::env(None, &Toggles::new())("PATH"), std::env::var("PATH").unwrap(), "no game: rsgl's own environment");
        let toggles = Toggles::from([("MANGOHUD".to_string(), Some("1".to_string())), ("PATH".to_string(), None)]);
        assert_eq!(super::env(None, &toggles)("MANGOHUD"), "1", "toggles apply with no game too");
        assert_eq!(super::env(None, &toggles)("PATH"), "", "a toggle that unsets it");
    }

    #[test]
    fn unknown_variables_and_bad_braces_are_errors() {
        let e = status(r#"right = [{ text = "{shown} games" }, { text = "{nope}" }]"#, &[]).err().unwrap();
        assert!(e.contains("right part 2") && e.contains("{nope} isn't defined"), "{e}");
        assert!(status(r#"left = [{ text = "{title" }]"#, &[]).is_err());
        assert!(!status(r#"left = [{ text = "{title} {{year}}" }]"#, &[]).unwrap().uses_igdb, "{{{{year}}}} is just text");
    }
}
