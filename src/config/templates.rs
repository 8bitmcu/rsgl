//! `[templates.<name>]`: keys shared by the games that list them.

use super::{opt_exec_list, GameCfg, MenuCfg, Origin};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

/// `[templates.<name>]`: keys shared by the games that list it in `template`, see
/// `apply_templates`. `default` is used by every `[[game]]`. A game's name and ids are
/// its own, so a template can't set them (nor use another template).
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct TemplateCfg {
    #[serde(default, deserialize_with = "opt_exec_list")]
    pub(super) exec: Option<Vec<String>>,
    pub(super) cover: Option<String>,
    pub(super) workdir: Option<String>,
    #[serde(default)]
    pub(super) env: HashMap<String, String>,
    pub(super) playtime: Option<bool>,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) vars: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub(super) right_click: Vec<MenuCfg>,
}

/// The template every `[[game]]` uses before its own, see `apply_templates`.
const DEFAULT_TEMPLATE: &str = "default";

impl TemplateCfg {
    /// `top` over `self`: its `exec`, `cover`, `workdir` and `playtime` replace these,
    /// its `env` and `vars` win key by key, and its `tags` and `right_click` come after.
    pub(super) fn under(mut self, top: TemplateCfg) -> TemplateCfg {
        self.exec = top.exec.or(self.exec);
        self.cover = top.cover.or(self.cover);
        self.workdir = top.workdir.or(self.workdir);
        self.playtime = top.playtime.or(self.playtime);
        self.env.extend(top.env);
        self.vars.extend(top.vars);
        for t in top.tags {
            if !self.tags.contains(&t) {
                self.tags.push(t);
            }
        }
        self.right_click.extend(top.right_click);
        self
    }
}

impl TemplateCfg {
    /// These keys with `names`' templates on top, in order, so each one wins over those
    /// before it.
    pub(super) fn with_templates(self, names: &[String], templates: &BTreeMap<String, TemplateCfg>) -> Result<TemplateCfg, String> {
        let mut merged = self;
        for name in names {
            let t = templates.get(name).ok_or_else(|| format!("no template {name:?}"))?;
            merged = merged.under(t.clone());
        }
        Ok(merged)
    }
}

/// `g` with `names`' templates under it, in order, so each one wins over those before it
/// and `g`'s own keys win over all of them. Variables aren't filled in yet, so `{dir}`
/// in a template gets each game's own value.
pub(super) fn apply_templates(g: GameCfg, names: &[String], templates: &BTreeMap<String, TemplateCfg>) -> Result<GameCfg, String> {
    Ok(g.over(TemplateCfg::default().with_templates(names, templates)?))
}

impl GameCfg {
    /// The game's own keys over `base`, merged like a template over another.
    pub(super) fn over(mut self, base: TemplateCfg) -> GameCfg {
        let own = TemplateCfg {
            exec: self.exec.take(),
            cover: self.cover.take(),
            workdir: self.workdir.take(),
            env: std::mem::take(&mut self.env),
            playtime: self.playtime,
            tags: std::mem::take(&mut self.tags),
            vars: std::mem::take(&mut self.vars),
            right_click: std::mem::take(&mut self.right_click),
        };
        let t = base.under(own);
        GameCfg {
            exec: t.exec,
            cover: t.cover,
            workdir: t.workdir,
            env: t.env,
            playtime: t.playtime,
            tags: t.tags,
            vars: t.vars,
            right_click: t.right_click,
            ..self
        }
    }
}

/// A `[[game]]` with `[templates.default]` and then its own templates applied. A ROM or
/// Steam game is returned as it is: its templates were applied when it was made, and
/// never `default`.
pub(super) fn game_with_templates(g: &GameCfg, templates: &BTreeMap<String, TemplateCfg>) -> Result<GameCfg, String> {
    if g.origin != Origin::Game {
        return Ok(g.clone());
    }
    let who = format!("game {:?}", g.name);
    if g.template.iter().any(|t| t == DEFAULT_TEMPLATE) {
        return Err(format!("{who}: every [[game]] gets [templates.{DEFAULT_TEMPLATE}] already, leave it out of template"));
    }
    let names: Vec<String> = templates
        .contains_key(DEFAULT_TEMPLATE)
        .then(|| DEFAULT_TEMPLATE.to_string())
        .into_iter()
        .chain(g.template.iter().cloned())
        .collect();
    apply_templates(g.clone(), &names, templates).map_err(|e| format!("{who}: {e}"))
}
