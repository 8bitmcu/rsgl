//! The bar above the grid: widgets from `[topbar]`, in a left, center and right zone.
use crate::{
    config::{self, MenuEntry, Toggles, ToolCfg, Vars},
    search::Query,
    status::{Checker, Part, PartCfg},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct TopbarCfg {
    left: Vec<WidgetCfg>,
    /// Left out, it's the search bar, unless that's in `left` or `right`.
    center: Option<Vec<WidgetCfg>>,
    right: Vec<WidgetCfg>,
}

/// Just the search bar, centered, like before there was a `[topbar]`.
impl Default for TopbarCfg {
    fn default() -> Self {
        Self { left: Vec::new(), center: None, right: Vec::new() }
    }
}

/// Unknown keys are an error: these are inline tables, where a typo would
/// otherwise just be ignored.
#[derive(Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WidgetCfg {
    /// A `[status]` part: text with variables, left out when they're all blank.
    Label {
        text: String,
        #[serde(default)]
        bold: bool,
    },
    /// The search box. `placeholder` replaces the hint with the key bindings.
    Search {
        placeholder: Option<String>,
        #[serde(default)]
        autocomplete: Vec<Complete>,
    },
    /// Runs `exec` like a right-click tool.
    Button(ToolCfg),
    /// A dropdown of tools.
    Menu { title: String, entries: Vec<ToolCfg> },
    /// Sets environment variable `env`, or variable `var` from `[vars]`, to `on` or `off`.
    /// Without `off`, the environment variable is unset and the variable blank.
    Toggle { title: String, env: Option<String>, var: Option<String>, on: String, off: Option<String> },
    /// A dropdown of the orders for the grid. All of them by default, starting with the first.
    Sort { options: Option<Vec<SortBy>>, default: Option<SortBy> },
    /// Searches to narrow the grid to, on top of what's typed, picked by `title`. The
    /// first one by default.
    Filter { options: Vec<FilterCfg>, default: Option<String> },
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct FilterCfg {
    title: String,
    filter: String,
}

pub enum Widget {
    Label(Part),
    Search { placeholder: Option<String>, autocomplete: Vec<Complete> },
    Button(MenuEntry),
    Menu { title: String, entries: Vec<MenuEntry> },
    Toggle(Toggle),
    Sort(Sort),
    Filter(Filter),
}

/// What the search box suggests the rest of, from the games the rest of the search shows.
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Complete {
    /// Game names.
    Names,
    /// Tags, after `t:`.
    Tags,
    /// IGDB genres, after `g:`, a word at a time.
    Genres,
}

/// A toggle. It starts from the config every time rsgl starts or reloads.
pub struct Toggle {
    pub title: String,
    pub target: Target,
    on_value: String,
    off_value: Option<String>,
    pub on: bool,
}

/// What a toggle switches.
#[derive(PartialEq, Debug)]
pub enum Target {
    /// An environment variable, for what's launched.
    Env(String),
    /// A variable from `[vars]`, filled in when the games are built, see `config::Library`.
    Var(String),
}

impl Toggle {
    /// What it sets its target to: a value, or None to unset it (only an environment
    /// variable; a variable's `off_value` is always set).
    fn value(&self) -> Option<&str> {
        match self.on {
            true => Some(&self.on_value),
            false => self.off_value.as_deref(),
        }
    }
}

/// An order for the grid. The ones by a number put the biggest first, and the games
/// without one last; ties stay in name order.
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    Name,
    LastPlayed,
    Playtime,
    /// IGDB's release year.
    Year,
}

impl SortBy {
    const ALL: [SortBy; 4] = [SortBy::Name, SortBy::LastPlayed, SortBy::Playtime, SortBy::Year];

    pub fn label(self) -> &'static str {
        match self {
            SortBy::Name => "Name",
            SortBy::LastPlayed => "Last played",
            SortBy::Playtime => "Playtime",
            SortBy::Year => "Year",
        }
    }
}

/// The sort dropdown. It starts from `default` every time rsgl starts or reloads, and
/// keeps its pick when a toggle builds the top bar again.
pub struct Sort {
    pub options: Vec<SortBy>,
    /// Which of `options` it is now.
    pub current: usize,
}

/// The filter selector. Like the sort, it starts from `default` every time rsgl starts,
/// and keeps its pick when a toggle builds the top bar again or the config is reloaded.
pub struct Filter {
    pub options: Vec<FilterOption>,
    /// Which of `options` it is now.
    pub current: usize,
}

pub struct FilterOption {
    pub title: String,
    /// What it narrows the grid to, ANDed with the search box. An empty one shows everything.
    pub query: Query,
}

impl Widget {
    /// What it runs: a button's one tool or a menu's entries.
    pub fn tools(&self) -> &[MenuEntry] {
        match self {
            Widget::Button(tool) => std::slice::from_ref(tool),
            Widget::Menu { entries, .. } => entries,
            Widget::Label(_) | Widget::Search { .. } | Widget::Toggle(_) | Widget::Sort(_) | Widget::Filter(_) => &[],
        }
    }
}

/// A widget's place: the zone (0 to 2, left to right) and its position in it.
pub type WidgetId = (usize, usize);

pub struct Topbar {
    pub left: Vec<Widget>,
    pub center: Vec<Widget>,
    pub right: Vec<Widget>,
    /// Some label uses `{year}` or `{genres}`, see `Status::uses_igdb`.
    pub uses_igdb: bool,
}

impl Default for Topbar {
    fn default() -> Self {
        let check = Checker::new(&BTreeSet::new());
        Topbar::new(TopbarCfg::default(), &check, &Vars::default(), &BTreeMap::new()).expect("the default top bar is valid")
    }
}

impl Topbar {
    /// `vars` and `env` are the global `[vars]` and `[env]`, for buttons, menus and toggles.
    pub fn new(cfg: TopbarCfg, check: &Checker, vars: &Vars, env: &BTreeMap<String, String>) -> Result<Topbar, String> {
        let mut uses_igdb = false;
        let mut search_at: Option<String> = None;
        let mut sort_at: Option<String> = None;
        let mut filter_at: Option<String> = None;
        let mut toggle_at: BTreeMap<String, String> = BTreeMap::new();
        let mut zone = |side: Vec<WidgetCfg>, which: &str| -> Result<Vec<Widget>, String> {
            let mut out = Vec::new();
            for (i, w) in side.into_iter().enumerate() {
                let at = format!("{which} widget {}", i + 1);
                out.push(match w {
                    WidgetCfg::Label { text, bold } => {
                        let part = check.part(PartCfg { text, bold }).map_err(|e| format!("{at}: {e}"))?;
                        uses_igdb |= part.uses_igdb;
                        Widget::Label(part)
                    }
                    WidgetCfg::Search { placeholder, autocomplete } => {
                        if let Some(first) = &search_at {
                            return Err(format!("{at}: there's already a search bar ({first})"));
                        }
                        search_at = Some(at);
                        Widget::Search { placeholder, autocomplete }
                    }
                    WidgetCfg::Button(t) => Widget::Button(config::tool(t, &at, vars, env)?),
                    WidgetCfg::Menu { title, entries } => {
                        if entries.is_empty() {
                            return Err(format!("{at}: menu {title:?} has no entries"));
                        }
                        let owner = format!("{at} (menu {title:?})");
                        let entries = entries.into_iter().map(|t| config::tool(t, &owner, vars, env)).collect::<Result<_, _>>()?;
                        Widget::Menu { title, entries }
                    }
                    WidgetCfg::Toggle { title, env: env_var, var, on, off } => {
                        let at = format!("{at} (toggle {title:?})");
                        // on when it would be `on` without the toggle
                        let (target, key, on, off, start) = match (env_var, var) {
                            (Some(name), None) => {
                                if !config::is_env_name(&name) {
                                    return Err(format!("{at}: {name:?} isn't an environment variable name"));
                                }
                                // what [env] says, else rsgl's own
                                let start = match env.get(&name) {
                                    Some(v) => {
                                        let v = vars.expand(v).map_err(|e| {
                                            format!("{at}: [env] {name}: {e} (the top bar only has [vars], not a game's own)")
                                        })?;
                                        Some(config::expand(&v))
                                    }
                                    None => std::env::var(&name).ok(),
                                };
                                // a leading ~ is expanded, like in [env]
                                (Target::Env(name.clone()), name, config::expand(&on), off.map(|o| config::expand(&o)), start)
                            }
                            (None, Some(name)) => {
                                if !config::is_var_name(&name) {
                                    return Err(format!("{at}: {name:?} isn't a variable name"));
                                }
                                if !vars.has(&name) {
                                    return Err(format!("{at}: {{{name}}} isn't in [vars], where the toggle starts from"));
                                }
                                if [Some(&on), off.as_ref()].into_iter().flatten().any(|v| v.contains(['{', '}'])) {
                                    return Err(format!(
                                        "{at}: on and off are used as they are, without variables; \
                                         put those in a [vars] value that uses {{{name}}}"
                                    ));
                                }
                                let start = vars.expand(&format!("{{{name}}}")).map_err(|e| {
                                    format!("{at}: [vars] {name}: {e} (the top bar only has [vars], not a game's own)")
                                })?;
                                (Target::Var(name.clone()), format!("{{{name}}}"), on, Some(off.unwrap_or_default()), Some(start))
                            }
                            (Some(_), Some(_)) => return Err(format!("{at}: use env or var, not both")),
                            (None, None) => return Err(format!("{at}: needs an env or a var to switch")),
                        };
                        if off.as_ref() == Some(&on) {
                            return Err(format!("{at}: on and off are both {on:?}"));
                        }
                        if let Some(first) = toggle_at.get(&key) {
                            return Err(format!("{at}: there's already a toggle for {key} ({first})"));
                        }
                        toggle_at.insert(key, at);
                        Widget::Toggle(Toggle { on: start.as_ref() == Some(&on), title, target, on_value: on, off_value: off })
                    }
                    WidgetCfg::Sort { options, default } => {
                        if let Some(first) = &sort_at {
                            return Err(format!("{at}: there's already a sort ({first})"));
                        }
                        let options = options.unwrap_or_else(|| SortBy::ALL.to_vec());
                        if options.is_empty() {
                            return Err(format!("{at}: sort has no options"));
                        }
                        if let Some(twice) = options.iter().enumerate().find(|&(k, o)| options[..k].contains(o)) {
                            return Err(format!("{at}: sort has {:?} twice", twice.1.label()));
                        }
                        let current = match default {
                            Some(d) => options.iter().position(|&o| o == d).ok_or_else(|| {
                                format!("{at}: the default, {:?}, isn't one of the sort's options", d.label())
                            })?,
                            None => 0,
                        };
                        uses_igdb |= options.contains(&SortBy::Year);
                        sort_at = Some(at);
                        Widget::Sort(Sort { options, current })
                    }
                    WidgetCfg::Filter { options, default } => {
                        // one, so prev_filter and next_filter know which to switch
                        if let Some(first) = &filter_at {
                            return Err(format!("{at}: there's already a filter ({first})"));
                        }
                        if options.is_empty() {
                            return Err(format!("{at}: filter has no options"));
                        }
                        if let Some(twice) = options.iter().enumerate().find(|&(k, o)| options[..k].iter().any(|p| p.title == o.title)) {
                            return Err(format!("{at}: filter has {:?} twice", twice.1.title));
                        }
                        let current = match default {
                            Some(d) => options.iter().position(|o| o.title == d).ok_or_else(|| {
                                format!("{at}: the default, {d:?}, isn't one of the filter's titles")
                            })?,
                            None => 0,
                        };
                        filter_at = Some(at);
                        let options = options.into_iter().map(|o| FilterOption { query: Query::parse(&o.filter), title: o.title });
                        Widget::Filter(Filter { options: options.collect(), current })
                    }
                });
            }
            Ok(out)
        };
        let is_search = |w: &WidgetCfg| matches!(w, WidgetCfg::Search { .. });
        let center = cfg.center.unwrap_or_else(|| match cfg.left.iter().chain(&cfg.right).any(is_search) {
            true => Vec::new(),
            false => vec![WidgetCfg::Search { placeholder: None, autocomplete: Vec::new() }],
        });
        let left = zone(cfg.left, "left")?;
        let center = zone(center, "center")?;
        let right = zone(cfg.right, "right")?;
        Ok(Topbar { left, center, right, uses_igdb })
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.center.is_empty() && self.right.is_empty()
    }

    pub fn zones(&self) -> [&[Widget]; 3] {
        [&self.left, &self.center, &self.right]
    }

    pub fn widget(&self, (zone, i): WidgetId) -> &Widget {
        &self.zones()[zone][i]
    }

    fn widget_mut(&mut self, (zone, i): WidgetId) -> &mut Widget {
        let zone = match zone {
            0 => &mut self.left,
            1 => &mut self.center,
            _ => &mut self.right,
        };
        &mut zone[i]
    }

    fn all_toggles(&self) -> impl Iterator<Item = &Toggle> {
        self.zones().into_iter().flatten().filter_map(|w| match w {
            Widget::Toggle(t) => Some(t),
            _ => None,
        })
    }

    /// Switches toggle `id` on or off. Panics if it isn't a toggle.
    pub fn flip(&mut self, id: WidgetId) {
        let Widget::Toggle(t) = self.widget_mut(id) else { panic!("widget {id:?} isn't a toggle") };
        t.on = !t.on;
    }

    /// Switches each toggle on or off like the one in the same place in `other`, a top
    /// bar from the same config, and keeps its sort and filter, see `keep_picks`.
    pub fn copy_state(&mut self, other: &Topbar) {
        let mine = [&mut self.left, &mut self.center, &mut self.right].into_iter().flatten();
        for (mine, theirs) in mine.zip(other.zones().into_iter().flatten()) {
            if let (Widget::Toggle(t), Widget::Toggle(o)) = (mine, theirs) {
                t.on = o.on;
            }
        }
        self.keep_picks(other);
    }

    /// Picks the order and filter `other` is on, where this top bar has them too: by
    /// the order and the filter's title, so it works for a reloaded config. What's
    /// gone stays on its default.
    pub fn keep_picks(&mut self, other: &Topbar) {
        let sort = other.sort();
        let filter = other.filter().map(|f| f.title.as_str());
        for w in [&mut self.left, &mut self.center, &mut self.right].into_iter().flatten() {
            match w {
                Widget::Sort(s) => s.current = s.options.iter().position(|&o| o == sort).unwrap_or(s.current),
                Widget::Filter(f) => {
                    let at = f.options.iter().position(|o| Some(o.title.as_str()) == filter);
                    f.current = at.unwrap_or(f.current);
                }
                _ => {}
            }
        }
    }

    /// What the environment toggles set right now, see `Env::get`.
    pub fn toggles(&self) -> Toggles {
        let env = self.all_toggles().filter_map(|t| match &t.target {
            Target::Env(name) => Some((name.clone(), t.value().map(str::to_string))),
            Target::Var(_) => None,
        });
        env.collect()
    }

    /// What the variable toggles set right now, by variable name.
    pub fn var_values(&self) -> Vec<(&str, &str)> {
        let vars = self.all_toggles().filter_map(|t| match &t.target {
            Target::Var(name) => Some((name.as_str(), t.value().expect("a variable's toggle always has a value"))),
            Target::Env(_) => None,
        });
        vars.collect()
    }

    /// The variables toggles switch, with their on and off values.
    pub fn var_toggles(&self) -> Vec<(String, [String; 2])> {
        let vars = self.all_toggles().filter_map(|t| match &t.target {
            Target::Var(name) => {
                let off = t.off_value.clone().expect("a variable's toggle always has an off value");
                Some((name.clone(), [t.on_value.clone(), off]))
            }
            Target::Env(_) => None,
        });
        vars.collect()
    }

    /// How the grid is ordered right now: by the sort, or by name without one.
    pub fn sort(&self) -> SortBy {
        let sort = self.zones().into_iter().flatten().find_map(|w| match w {
            Widget::Sort(s) => Some(s.options[s.current]),
            _ => None,
        });
        sort.unwrap_or(SortBy::Name)
    }

    /// The filter selector, if there is one.
    pub fn filter_widget(&self) -> Option<&Filter> {
        self.zones().into_iter().flatten().find_map(|w| match w {
            Widget::Filter(f) => Some(f),
            _ => None,
        })
    }

    fn filter_mut(&mut self) -> Option<&mut Filter> {
        [&mut self.left, &mut self.center, &mut self.right].into_iter().flatten().find_map(|w| match w {
            Widget::Filter(f) => Some(f),
            _ => None,
        })
    }

    /// The filter the grid is narrowed to now; None without a filter selector.
    pub fn filter(&self) -> Option<&FilterOption> {
        self.filter_widget().map(|f| &f.options[f.current])
    }

    /// What the search box completes; nothing without one.
    pub fn autocomplete(&self) -> &[Complete] {
        let search = self.zones().into_iter().flatten().find_map(|w| match w {
            Widget::Search { autocomplete, .. } => Some(&autocomplete[..]),
            _ => None,
        });
        search.unwrap_or_default()
    }

    /// Picks option `k` of sort `id`. Panics if it isn't a sort.
    pub fn set_sort(&mut self, id: WidgetId, k: usize) {
        let Widget::Sort(s) = self.widget_mut(id) else { panic!("widget {id:?} isn't a sort") };
        assert!(k < s.options.len(), "sort option {k} of {}", s.options.len());
        s.current = k;
    }

    /// Picks option `k` of filter `id`. Panics if it isn't a filter.
    pub fn set_filter(&mut self, id: WidgetId, k: usize) {
        let Widget::Filter(f) = self.widget_mut(id) else { panic!("widget {id:?} isn't a filter") };
        assert!(k < f.options.len(), "filter option {k} of {}", f.options.len());
        f.current = k;
    }

    /// Picks the filter `--filter` names, by title like `--show` finds a game (see
    /// `config::pick`). The error says why there's none to pick.
    pub fn pick_filter(&mut self, query: &str) -> Result<(), String> {
        let Some(f) = self.filter_mut() else { return Err("--filter: the top bar has no filter".to_string()) };
        let titles: Vec<&str> = f.options.iter().map(|o| o.title.as_str()).collect();
        match config::pick(&titles, query) {
            Ok(k) => {
                f.current = k;
                Ok(())
            }
            Err(none) if none.is_empty() => {
                Err(format!("--filter: no filter matches {query:?}; there's {}", titles.join(", ")))
            }
            Err(many) => Err(format!("--filter: {query:?} matches {} filters: {}", many.len(), many.join(", "))),
        }
    }

    /// Moves the filter `by` options along, going around at either end. Does nothing
    /// without a filter selector.
    pub fn step_filter(&mut self, by: isize) {
        if let Some(f) = self.filter_mut() {
            f.current = (f.current as isize + by).rem_euclid(f.options.len() as isize) as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Vars;

    fn topbar(toml: &str) -> Result<Topbar, String> {
        let cfg: TopbarCfg = toml::from_str(toml).map_err(|e| e.to_string())?;
        let vars = Vars::literal([("term", "foot")]);
        let env = BTreeMap::from([("WINEDEBUG".to_string(), "-all".to_string())]);
        Topbar::new(cfg, &Checker::new(&BTreeSet::from(["dir".to_string()])), &vars, &env)
    }

    #[test]
    fn the_default_is_a_centered_search_and_zones_left_out_keep_theirs() {
        let t = Topbar::default();
        assert!(t.left.is_empty() && t.right.is_empty() && !t.uses_igdb);
        assert!(matches!(t.center[..], [Widget::Search { placeholder: None, .. }]));

        let t = topbar(r#"left = [{ type = "label", text = "{genres}" }]"#).unwrap();
        assert!(t.uses_igdb);
        assert!(matches!(t.center[..], [Widget::Search { .. }]), "center keeps its default");
        assert!(topbar("center = []").unwrap().is_empty());

        let t = topbar(r#"right = [{ type = "search" }]"#).unwrap();
        assert!(t.center.is_empty(), "the search moved, so center doesn't get another");
        assert!(t.autocomplete().is_empty(), "autocomplete is off by default");
    }

    #[test]
    fn autocomplete_picks_what_to_complete() {
        let t = topbar(r#"left = [{ type = "search", autocomplete = ["tags", "names"] }]"#).unwrap();
        assert_eq!(t.autocomplete(), [Complete::Tags, Complete::Names]);
        assert!(topbar("center = []").unwrap().autocomplete().is_empty(), "no search, nothing to complete");
        let e = topbar(r#"center = [{ type = "search", autocomplete = ["tag"] }]"#).err().unwrap();
        assert!(e.contains("unknown variant") && e.contains("tags"), "{e}");
    }

    #[test]
    fn labels_are_status_parts_with_env() {
        let t = topbar(
            r#"center = []
               right = [{ type = "label", text = "MangoHud {env:MANGOHUD}", bold = true }, { type = "search", placeholder = "Find…" }]"#,
        )
        .unwrap();
        let Widget::Label(part) = &t.right[0] else { panic!("a label") };
        assert!(part.bold);
        let env = |v: &'static str| move |name: &str| if name == "MANGOHUD" { v.to_string() } else { String::new() };
        assert_eq!(part.render(&Vars::default(), &env("1")).as_deref(), Some("MangoHud 1"));
        assert_eq!(part.render(&Vars::default(), &env("")), None, "blank like any variable");
        assert!(matches!(&t.right[1], Widget::Search { placeholder: Some(p), .. } if p == "Find…"));
    }

    #[test]
    fn mistakes_are_errors() {
        let e = topbar(r#"left = [{ type = "search" }]
                           center = [{ type = "search" }]"#).err().unwrap();
        assert!(e.contains("center widget 1") && e.contains("already a search bar (left widget 1)"), "{e}");
        let e = topbar(r#"right = [{ type = "label", text = "{nope}" }]"#).err().unwrap();
        assert!(e.contains("right widget 1") && e.contains("{nope} isn't defined"), "{e}");
        let e = topbar(r#"right = [{ type = "label", text = "{env:A B}" }]"#).err().unwrap();
        assert!(e.contains("isn't an environment variable name"), "{e}");
        let e = topbar(r#"left = [{ type = "label", txt = "hi" }]"#).err().unwrap();
        assert!(e.contains("unknown field"), "{e}");
        let e = topbar(r#"left = [{ type = "clock" }]"#).err().unwrap();
        assert!(e.contains("unknown variant"), "{e}");
    }

    #[test]
    fn buttons_and_menus_are_tools_with_the_global_vars_and_env() {
        let t = topbar(
            r#"left = [{ type = "button", title = "Shell", exec = "{term} -e htop", env = { A = "{term}" } }]
               right = [
                 { type = "label", text = "hi" },
                 { type = "menu", title = "Tools", entries = [
                     { title = "Lutris", exec = "lutris" },
                     { title = "Here", exec = "ls", workdir = "/tmp" },
                 ] },
               ]"#,
        )
        .unwrap();
        let Widget::Button(b) = &t.left[0] else { panic!("a button") };
        assert_eq!(b.action, config::MenuAction::Exec(vec!["foot -e htop".into()]));
        assert_eq!((b.env.all["A"].as_str(), b.env.all["WINEDEBUG"].as_str()), ("foot", "-all"));
        assert!(!b.launch && b.workdir.is_none());
        let Widget::Menu { title, entries } = &t.right[1] else { panic!("a menu") };
        assert_eq!((title.as_str(), entries.len()), ("Tools", 2));
        assert_eq!(entries[1].workdir.as_deref(), Some(std::path::Path::new("/tmp")));
        assert_eq!(t.widget((2, 1)).tools().len(), 2);
        assert!(t.widget((2, 0)).tools().is_empty() && t.widget((1, 0)).tools().is_empty(), "labels and search run nothing");
    }

    #[test]
    fn tool_mistakes_are_errors() {
        let e = topbar(r#"left = [{ type = "button", title = "X", exec = "{dir}/run" }]"#).err().unwrap();
        assert!(e.contains("left widget 1, entry \"X\": exec: {dir} isn't defined"), "a game's variable: {e}");
        let e = topbar(r#"left = [{ type = "button", title = "X", exec = "x", launch = true }]"#).err().unwrap();
        assert!(e.contains("unknown field `launch`"), "{e}");
        let e = topbar(r#"left = [{ type = "button", title = "X" }]"#).err().unwrap();
        assert!(e.contains("missing field `exec`"), "{e}");
        let e = topbar(r#"right = [{ type = "menu", title = "M", entries = [] }]"#).err().unwrap();
        assert!(e.contains("right widget 1: menu \"M\" has no entries"), "{e}");
        let e = topbar(r#"right = [{ type = "menu", title = "M", entries = [{ title = "Y", exec = "$X {env:X}" }] }]"#).err().unwrap();
        assert!(e.contains("right widget 1 (menu \"M\"), entry \"Y\"") && e.contains("use $X"), "{e}");
    }

    #[test]
    fn toggles_start_from_env_and_flip() {
        let t = topbar(
            r#"left = [
                 { type = "toggle", title = "Quiet wine", env = "WINEDEBUG", on = "-all" },
                 { type = "toggle", title = "MangoHud", env = "MANGOHUD", on = "1", off = "0" },
               ]"#,
        )
        .unwrap();
        let Widget::Toggle(quiet) = &t.left[0] else { panic!("a toggle") };
        assert!(quiet.on, "[env] has its on value");
        let Widget::Toggle(mango) = &t.left[1] else { panic!("a toggle") };
        assert!(!mango.on && mango.target == Target::Env("MANGOHUD".into()), "not set anywhere");
        assert!(t.widget((0, 0)).tools().is_empty(), "a toggle runs nothing");
        let set = |pairs: &[(&str, Option<&str>)]| -> Toggles {
            pairs.iter().map(|(k, v)| (k.to_string(), v.map(str::to_string))).collect()
        };
        assert_eq!(t.toggles(), set(&[("MANGOHUD", Some("0")), ("WINEDEBUG", Some("-all"))]));

        let mut t = t;
        t.flip((0, 0));
        t.flip((0, 1));
        assert_eq!(t.toggles(), set(&[("MANGOHUD", Some("1")), ("WINEDEBUG", None)]), "no off: unset");
    }

    #[test]
    fn env_toggles_expand_a_leading_tilde_like_env() {
        let cfg: TopbarCfg = toml::from_str(
            r#"left = [{ type = "toggle", title = "Old prefix", env = "WINEPREFIX", on = "~/.games", off = "~/.wine" }]"#,
        )
        .unwrap();
        let env = BTreeMap::from([("WINEPREFIX".to_string(), "~/.games".to_string())]);
        let mut t = Topbar::new(cfg, &Checker::new(&BTreeSet::new()), &Vars::literal([]), &env).unwrap();
        let Widget::Toggle(p) = &t.left[0] else { panic!("a toggle") };
        assert!(p.on, "[env]'s ~ is expanded, so on has to be too to match it");
        let (games, wine) = (config::expand("~/.games"), config::expand("~/.wine"));
        assert!(!games.starts_with('~'), "a home directory to expand to");
        assert_eq!(t.toggles()["WINEPREFIX"].as_deref(), Some(games.as_str()));
        t.flip((0, 0));
        assert_eq!(t.toggles()["WINEPREFIX"].as_deref(), Some(wine.as_str()));
    }

    #[test]
    fn toggles_start_from_rsgls_environment_without_env() {
        // PATH is set wherever the tests run, and isn't in the test [env]
        let path = std::env::var("PATH").unwrap();
        let toml = format!(r#"left = [{{ type = "toggle", title = "P", env = "PATH", on = {path:?} }}]"#);
        let t = topbar(&toml).unwrap();
        assert!(matches!(&t.left[0], Widget::Toggle(p) if p.on));
    }

    #[test]
    fn toggle_mistakes_are_errors() {
        let e = topbar(r#"left = [{ type = "toggle", title = "X", env = "A B", on = "1" }]"#).err().unwrap();
        assert!(e.contains("left widget 1 (toggle \"X\"): \"A B\" isn't an environment variable name"), "{e}");
        let e = topbar(r#"left = [{ type = "toggle", title = "X", env = "A", on = "1", off = "1" }]"#).err().unwrap();
        assert!(e.contains("on and off are both \"1\""), "{e}");
        let e = topbar(
            r#"left = [{ type = "toggle", title = "X", env = "A", on = "1" }]
               right = [{ type = "toggle", title = "Y", env = "A", on = "2" }]"#,
        )
        .err()
        .unwrap();
        assert!(e.contains("right widget 1 (toggle \"Y\"): there's already a toggle for A (left widget 1 (toggle \"X\"))"), "{e}");
        let e = topbar(r#"left = [{ type = "toggle", title = "X", on = "1" }]"#).err().unwrap();
        assert!(e.contains("left widget 1 (toggle \"X\"): needs an env or a var"), "{e}");
        let e = topbar(r#"left = [{ type = "toggle", title = "X", env = "A", on = "1", default = true }]"#).err().unwrap();
        assert!(e.contains("unknown field `default`"), "{e}");

        let cfg: TopbarCfg = toml::from_str(r#"left = [{ type = "toggle", title = "X", env = "W", on = "1" }]"#).unwrap();
        let env = BTreeMap::from([("W".to_string(), "{dir}".to_string())]);
        let e = Topbar::new(cfg, &Checker::new(&BTreeSet::new()), &Vars::default(), &env).err().unwrap();
        assert!(e.contains("left widget 1 (toggle \"X\"): [env] W") && e.contains("not a game's own"), "{e}");
    }

    #[test]
    fn var_toggles_start_from_vars_and_go_blank_without_off() {
        let t = topbar(
            r#"left = [
                 { type = "toggle", title = "Term", var = "term", on = "foot" },
                 { type = "toggle", title = "MangoHud", env = "MANGOHUD", on = "1" },
               ]"#,
        )
        .unwrap();
        assert_eq!(t.var_values(), [("term", "foot")], "on, [vars] says foot");
        assert_eq!(t.var_toggles(), [("term".to_string(), ["foot".to_string(), String::new()])]);
        assert!(!t.toggles().contains_key("term"), "not an environment variable");

        let mut t = t;
        t.flip((0, 0));
        assert_eq!(t.var_values(), [("term", "")]);
        let mut again = topbar(r#"left = [{ type = "toggle", title = "Term", var = "term", on = "foot" },
                                          { type = "toggle", title = "MangoHud", env = "MANGOHUD", on = "1" }]"#)
        .unwrap();
        t.flip((0, 1));
        again.copy_state(&t);
        assert_eq!((again.var_values(), again.toggles()["MANGOHUD"].as_deref()), (vec![("term", "")], Some("1")));
    }

    #[test]
    fn var_toggle_mistakes_are_errors() {
        let e = topbar(r#"left = [{ type = "toggle", title = "X", var = "nope", on = "1" }]"#).err().unwrap();
        assert!(e.contains("left widget 1 (toggle \"X\"): {nope} isn't in [vars]"), "{e}");
        let e = topbar(r#"left = [{ type = "toggle", title = "X", var = "term", on = "{dir}" }]"#).err().unwrap();
        assert!(e.contains("used as they are, without variables"), "{e}");
        let e = topbar(r#"left = [{ type = "toggle", title = "X", var = "term", on = "" }]"#).err().unwrap();
        assert!(e.contains("on and off are both \"\""), "off is blank without it: {e}");
        let e = topbar(r#"left = [{ type = "toggle", title = "X", var = "term", env = "A", on = "1" }]"#).err().unwrap();
        assert!(e.contains("use env or var, not both"), "{e}");
        let e = topbar(
            r#"left = [{ type = "toggle", title = "X", var = "term", on = "1" }, { type = "toggle", title = "Y", var = "term", on = "2" }]"#,
        )
        .err()
        .unwrap();
        assert!(e.contains("there's already a toggle for {term} (left widget 1"), "{e}");
        assert!(
            topbar(r#"left = [{ type = "toggle", title = "X", var = "term", on = "1" }, { type = "toggle", title = "Y", env = "term", on = "2" }]"#).is_ok(),
            "a variable and an environment variable of the same name are different things"
        );
    }

    #[test]
    fn sorts_default_to_every_order_and_their_first() {
        let t = Topbar::default();
        assert_eq!(t.sort(), SortBy::Name, "no sort: by name");
        let mut t = topbar(r#"right = [{ type = "sort" }]"#).unwrap();
        let Widget::Sort(s) = &t.right[0] else { panic!("a sort") };
        assert_eq!((s.options.len(), t.sort()), (4, SortBy::Name));
        assert!(t.uses_igdb, "year is one of them");
        t.set_sort((2, 0), 1);
        assert_eq!(t.sort(), SortBy::LastPlayed);

        let t = topbar(r#"right = [{ type = "sort", options = ["playtime", "name"], default = "name" }]"#).unwrap();
        assert_eq!((t.sort(), t.uses_igdb), (SortBy::Name, false));
        assert!(t.widget((2, 0)).tools().is_empty(), "a sort runs nothing");
    }

    #[test]
    fn sort_mistakes_are_errors() {
        let e = topbar(r#"right = [{ type = "sort", options = ["name", "rating"] }]"#).err().unwrap();
        assert!(e.contains("unknown variant `rating`"), "{e}");
        let e = topbar(r#"right = [{ type = "sort", options = [] }]"#).err().unwrap();
        assert!(e.contains("right widget 1: sort has no options"), "{e}");
        let e = topbar(r#"right = [{ type = "sort", options = ["name", "year", "name"] }]"#).err().unwrap();
        assert!(e.contains("sort has \"Name\" twice"), "{e}");
        let e = topbar(r#"right = [{ type = "sort", options = ["name"], default = "year" }]"#).err().unwrap();
        assert!(e.contains("the default, \"Year\", isn't one of the sort's options"), "{e}");
        let e = topbar(r#"left = [{ type = "sort" }]
                           right = [{ type = "sort" }]"#).err().unwrap();
        assert!(e.contains("right widget 1: there's already a sort (left widget 1)"), "{e}");
    }

    const FILTER: &str = r#"left = [{ type = "filter", options = [
        { title = "All", filter = "" },
        { title = "NES", filter = "t:nes" },
        { title = "LAN", filter = "t:pc t:lan" },
    ] }]"#;

    fn filter(t: &Topbar) -> &str {
        &t.filter().expect("a filter").title
    }

    #[test]
    fn filters_start_on_the_first_or_default_and_go_around() {
        assert!(Topbar::default().filter().is_none(), "no filter widget");
        let mut t = topbar(FILTER).unwrap();
        assert_eq!(filter(&t), "All");
        assert_eq!(t.filter().unwrap().query, Query::default(), "an empty filter shows everything");
        t.step_filter(-1);
        assert_eq!(filter(&t), "LAN", "around from the first to the last");
        assert_eq!(t.filter().unwrap().query.tags, ["pc", "lan"]);
        t.step_filter(1);
        assert_eq!(filter(&t), "All");
        t.set_filter((0, 0), 1);
        assert_eq!(filter(&t), "NES");
        assert!(t.widget((0, 0)).tools().is_empty(), "a filter runs nothing");

        let t = topbar(&FILTER.replace("] }]", "], default = \"LAN\" }]")).unwrap();
        assert_eq!(filter(&t), "LAN");
    }

    #[test]
    fn the_command_line_picks_a_filter_by_title() {
        let mut t = topbar(&FILTER.replace("\"LAN\"", "\"LAN Party\"")).unwrap();
        t.pick_filter("nes").unwrap();
        assert_eq!(filter(&t), "NES", "any case");
        t.pick_filter("party").unwrap();
        assert_eq!(filter(&t), "LAN Party", "part of the only title containing it");
        let e = t.pick_filter("SNES").err().unwrap();
        assert!(e.contains("no filter matches \"SNES\"; there's All, NES, LAN Party"), "{e}");
        let e = t.pick_filter("n").err().unwrap();
        assert!(e.contains("matches 2 filters: NES, LAN Party"), "{e}");
        assert_eq!(filter(&t), "LAN Party", "a failed pick changes nothing");
        let e = Topbar::default().pick_filter("NES").err().unwrap();
        assert!(e.contains("the top bar has no filter"), "{e}");
    }

    #[test]
    fn the_sort_and_filter_are_kept_by_value_across_configs() {
        let mut old = topbar(&format!("{FILTER}\nright = [{{ type = \"sort\" }}]")).unwrap();
        old.set_filter((0, 0), 2);
        old.set_sort((2, 0), 3);
        // reloaded: moved to the other zone, reordered, with fewer orders
        let reloaded = r#"left = [{ type = "sort", options = ["year", "name"], default = "name" }]
                          right = [{ type = "filter", options = [{ title = "LAN", filter = "t:lan" }, { title = "All", filter = "" }], default = "All" }]"#;
        let mut new = topbar(reloaded).unwrap();
        new.keep_picks(&old);
        assert_eq!((new.sort(), filter(&new)), (SortBy::Year, "LAN"));

        old.set_filter((0, 0), 1);
        old.set_sort((2, 0), 2);
        let mut new = topbar(reloaded).unwrap();
        new.keep_picks(&old);
        assert_eq!((new.sort(), filter(&new)), (SortBy::Name, "All"), "gone: the default");

        let mut again = topbar(&format!("{FILTER}\nright = [{{ type = \"sort\" }}]")).unwrap();
        again.copy_state(&old);
        assert_eq!((again.sort(), filter(&again)), (SortBy::Playtime, "NES"), "a toggle's rebuild keeps them too");
    }

    #[test]
    fn filter_mistakes_are_errors() {
        let e = topbar(r#"left = [{ type = "filter", options = [] }]"#).err().unwrap();
        assert!(e.contains("left widget 1: filter has no options"), "{e}");
        let e = topbar(r#"left = [{ type = "filter", options = [{ title = "A", filter = "" }, { title = "A", filter = "t:a" }] }]"#)
            .err()
            .unwrap();
        assert!(e.contains("filter has \"A\" twice"), "{e}");
        let e = topbar(&FILTER.replace("] }]", "], default = \"SNES\" }]")).err().unwrap();
        assert!(e.contains("the default, \"SNES\", isn't one of the filter's titles"), "{e}");
        let e = topbar(r#"left = [{ type = "filter", options = [{ title = "A", search = "t:a" }] }]"#).err().unwrap();
        assert!(e.contains("unknown field `search`"), "{e}");
        let e = topbar(&format!("{FILTER}\nright = [{{ type = \"filter\", options = [{{ title = \"B\", filter = \"\" }}] }}]"))
            .err()
            .unwrap();
        assert!(e.contains("right widget 1: there's already a filter (left widget 1)"), "{e}");
    }

    #[test]
    fn global_env_with_a_games_variable_says_why_it_fails_here() {
        let cfg: TopbarCfg = toml::from_str(r#"left = [{ type = "button", title = "X", exec = "x" }]"#).unwrap();
        let env = BTreeMap::from([("DXVK_CONFIG_FILE".to_string(), "{dir}/dxvk.conf".to_string())]);
        let e = Topbar::new(cfg, &Checker::new(&BTreeSet::new()), &Vars::default(), &env).err().unwrap();
        assert!(e.contains("[env] (for left widget 1), DXVK_CONFIG_FILE") && e.contains("not a game's own"), "{e}");
    }
}
