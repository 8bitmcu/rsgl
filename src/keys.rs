use eframe::egui::{self, Event, Key, Modifiers};
use serde::{Deserialize, Deserializer};
use std::{collections::BTreeMap, fmt::Display};

/// Something the keyboard or a gamepad can do. The grid uses all of them; an open menu uses
/// `Up`/`Down` to move, `Launch` to run the entry and `Back` to close, and the top bar, once
/// it has the focus, `Left`/`Right` to move, `Launch` to use a widget and `Down`/`Back` to leave.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Launch,
    Menu,
    Back,
    Reload,
}

impl Action {
    const ALL: [Action; 10] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::PageUp,
        Action::PageDown,
        Action::Launch,
        Action::Menu,
        Action::Back,
        Action::Reload,
    ];

    /// The action's key in `[keyboard]` and `[gamepad]`.
    fn name(self) -> &'static str {
        match self {
            Action::Up => "up",
            Action::Down => "down",
            Action::Left => "left",
            Action::Right => "right",
            Action::PageUp => "page_up",
            Action::PageDown => "page_down",
            Action::Launch => "launch",
            Action::Menu => "menu",
            Action::Back => "back",
            Action::Reload => "reload",
        }
    }
}

/// How an action was triggered: pressed just now, or repeating because it's held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    Fresh,
    Repeat,
}

impl Press {
    /// Two ways the same action came in this frame: fresh if either is.
    pub fn or(a: Option<Press>, b: Option<Press>) -> Option<Press> {
        match (a, b) {
            (Some(Press::Fresh), _) | (_, Some(Press::Fresh)) => Some(Press::Fresh),
            (a, b) => a.or(b),
        }
    }
}

/// Something `[keyboard]` or `[gamepad]` can bind to an action: a key or a button.
pub trait Bindable: Copy + PartialEq + Display + Sized {
    /// What `action` is bound to when the table doesn't mention it.
    fn defaults(action: Action) -> &'static [&'static str];
    fn parse(s: &str) -> Result<Self, String>;
}

/// A key plus the exact modifiers that must be held with it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Binding {
    mods: Modifiers,
    key: Key,
}

impl Bindable for Binding {
    fn defaults(action: Action) -> &'static [&'static str] {
        match action {
            Action::Up => &["Up"],
            Action::Down => &["Down"],
            Action::Left => &["Left"],
            Action::Right => &["Right"],
            Action::PageUp => &["PageUp"],
            Action::PageDown => &["PageDown"],
            Action::Launch => &["Enter"],
            Action::Menu => &["Shift+F10"],
            Action::Back => &["Escape"],
            Action::Reload => &["F5"],
        }
    }

    /// Parses "Enter", "Shift+F10", "Ctrl+Alt+M", "Ctrl++". Key names are egui's
    /// (`Key::from_name`), with `Key::name` also matched in any case, so "enter" works.
    /// Modifiers are case-insensitive.
    fn parse(s: &str) -> Result<Binding, String> {
        let mut rest = s.trim();
        let mut mods = Modifiers::NONE;
        // an empty part after the `+` means the key itself is `+`
        while let Some((m, after)) = rest.split_once('+').filter(|(_, after)| !after.is_empty()) {
            mods |= match m.to_ascii_lowercase().as_str() {
                "ctrl" => Modifiers::CTRL,
                "alt" => Modifiers::ALT,
                "shift" => Modifiers::SHIFT,
                _ => return Err(format!("{s:?}: unknown modifier {m:?} (use Ctrl, Alt or Shift)")),
            };
            rest = after;
        }
        let key = Key::from_name(rest)
            .or_else(|| Key::ALL.iter().copied().find(|k| k.name().eq_ignore_ascii_case(rest)))
            .ok_or_else(|| format!("{s:?}: unknown key {rest:?}"))?;
        // a key the search box needs would stop working there
        if edits_text(key) && !mods.ctrl && !mods.alt {
            return Err(format!("{s:?} is needed for typing in the search box; add Ctrl or Alt, like \"Ctrl+{rest}\""));
        }
        Ok(Binding { mods, key })
    }
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (on, name) in [(self.mods.ctrl, "Ctrl+"), (self.mods.alt, "Alt+"), (self.mods.shift, "Shift+")] {
            if on {
                f.write_str(name)?;
            }
        }
        f.write_str(self.key.name())
    }
}

/// Keys that type into the search box or edit it: space, punctuation, digits, letters,
/// Backspace and Delete. Punctuation through letters are one run in egui's `Key`.
fn edits_text(key: Key) -> bool {
    matches!(key, Key::Space | Key::Backspace | Key::Delete) || (Key::Colon..=Key::Z).contains(&key)
}

/// What's bound to each action, from `[keyboard]` or `[gamepad]` with the defaults
/// filled in. Each action maps to a list; an empty list unbinds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bindings<B>(Vec<Vec<B>>); // indexed by `Action as usize`

pub type Keymap = Bindings<Binding>;

impl<B: Bindable> Bindings<B> {
    /// The first input bound to `action`, for hints in the UI.
    pub fn label(&self, action: Action) -> Option<String> {
        self.0[action as usize].first().map(B::to_string)
    }

    /// The action `input` is bound to, if any. There's at most one, since an input
    /// bound to two actions is an error.
    pub fn action(&self, input: B) -> Option<Action> {
        Action::ALL.into_iter().find(|&a| self.0[a as usize].contains(&input))
    }
}

impl Keymap {
    /// True if a key bound to `action` was pressed this frame. The press is consumed,
    /// so the search box doesn't also act on it. Modifiers must match exactly, so
    /// "Enter" doesn't also fire on Alt+Enter.
    pub fn take(&self, ctx: &egui::Context, action: Action) -> bool {
        self.take_press(ctx, action).is_some()
    }

    /// `take`, saying whether the key was pressed just now or is repeating.
    pub fn take_press(&self, ctx: &egui::Context, action: Action) -> Option<Press> {
        let bindings = &self.0[action as usize];
        ctx.input_mut(|i| {
            let mut hit = None;
            i.events.retain(|e| match e {
                Event::Key { key, modifiers, pressed: true, repeat, .. }
                    if bindings.iter().any(|b| b.key == *key && modifiers.matches_exact(b.mods)) =>
                {
                    hit = Press::or(hit, Some(if *repeat { Press::Repeat } else { Press::Fresh }));
                    false
                }
                _ => true,
            });
            hit
        })
    }
}

impl<B: Bindable> Default for Bindings<B> {
    fn default() -> Self {
        Bindings::try_from(BTreeMap::new()).expect("the default bindings are valid")
    }
}

impl<'de, B: Bindable> Deserialize<'de> for Bindings<B> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let cfg = BTreeMap::<String, Vec<String>>::deserialize(d)?;
        Bindings::try_from(cfg).map_err(serde::de::Error::custom)
    }
}

/// A `[keyboard]` or `[gamepad]` table: action name to a list of keys or buttons.
/// Unknown actions, bad names and an input bound to two actions are errors, since
/// any of them leaves a control not working.
impl<B: Bindable> TryFrom<BTreeMap<String, Vec<String>>> for Bindings<B> {
    type Error = String;

    fn try_from(mut cfg: BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut map = Vec::new();
        let mut seen: Vec<(B, Action)> = Vec::new();
        for action in Action::ALL {
            let mut bindings = Vec::new();
            let keys = match cfg.remove(action.name()) {
                Some(keys) => keys,
                None => B::defaults(action).iter().map(|s| s.to_string()).collect(),
            };
            for s in &keys {
                let b = B::parse(s).map_err(|e| format!("{}: {e}", action.name()))?;
                if let Some((_, other)) = seen.iter().find(|(o, _)| *o == b) {
                    return Err(format!("{b} is bound to both {} and {}", other.name(), action.name()));
                }
                seen.push((b, action));
                bindings.push(b);
            }
            map.push(bindings);
        }
        if let Some(name) = cfg.keys().next() {
            let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
            return Err(format!("unknown action {name:?} (the actions are {})", names.join(", ")));
        }
        Ok(Bindings(map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keymap(text: &str) -> Result<Keymap, String> {
        let cfg: BTreeMap<String, Vec<String>> = toml::from_str(text).map_err(|e| e.to_string())?;
        Keymap::try_from(cfg)
    }

    #[test]
    fn actions_are_listed_in_index_order() {
        for (i, a) in Action::ALL.iter().enumerate() {
            assert_eq!(*a as usize, i, "{a:?}");
        }
    }

    #[test]
    fn defaults_fill_in_what_isnt_set_and_an_empty_list_unbinds() {
        let k = Keymap::default();
        assert_eq!(k.label(Action::Launch).as_deref(), Some("Enter"));
        assert_eq!(k.label(Action::Menu).as_deref(), Some("Shift+F10"));

        let k = keymap("launch = [\"Ctrl+Enter\", \"F2\"]\nmenu = []\n").unwrap();
        assert_eq!(k.label(Action::Launch).as_deref(), Some("Ctrl+Enter"));
        assert_eq!(k.label(Action::Menu), None);
        assert_eq!(k.label(Action::Reload).as_deref(), Some("F5"), "untouched actions keep their default");
    }

    #[test]
    fn keys_and_modifiers_parse() {
        let b = |s: &str| Binding::parse(s).unwrap();
        assert_eq!(b("Shift+F10"), Binding { mods: Modifiers::SHIFT, key: Key::F10 });
        assert_eq!(b("ctrl+alt+m"), Binding { mods: Modifiers::CTRL | Modifiers::ALT, key: Key::M });
        assert_eq!(b("Ctrl++"), Binding { mods: Modifiers::CTRL, key: Key::Plus });
        assert_eq!(b("escape"), Binding { mods: Modifiers::NONE, key: Key::Escape });
        assert_eq!(b("ArrowUp"), b("Up"));
        assert_eq!(b("Ctrl+Alt+Shift+Home").to_string(), "Ctrl+Alt+Shift+Home");

        for (s, want) in [("Super+X", "unknown modifier"), ("Nope", "unknown key"), ("Ctrl+", "unknown key")] {
            let e = Binding::parse(s).unwrap_err();
            assert!(e.contains(want), "{s:?} gave {e:?}");
        }
    }

    #[test]
    fn keys_the_search_box_needs_require_ctrl_or_alt() {
        for s in ["A", "Shift+A", "5", "/", "Space", "Backspace", "Delete", "Quote"] {
            let e = Binding::parse(s).unwrap_err();
            assert!(e.contains("search box"), "{s:?} gave {e:?}");
        }
        for s in ["Ctrl+A", "Alt+5", "Ctrl+Space", "F1", "Enter", "Home", "Tab", "Shift+Up"] {
            assert!(Binding::parse(s).is_ok(), "{s:?}");
        }
        // the range in `edits_text` relies on egui's key order
        for k in [Key::Colon, Key::Quote, Key::Num0, Key::Num9, Key::A, Key::Z] {
            assert!(edits_text(k), "{k:?}");
        }
        for k in [Key::Paste, Key::PageDown, Key::F1, Key::Enter, Key::Escape] {
            assert!(!edits_text(k), "{k:?}");
        }
    }

    #[test]
    fn a_key_on_two_actions_and_unknown_actions_are_errors() {
        let e = keymap("back = [\"Enter\"]\n").unwrap_err();
        assert!(e.contains("Enter is bound to both launch and back"), "{e}");
        assert!(keymap("back = [\"Enter\"]\nlaunch = [\"Ctrl+Enter\"]\n").is_ok());

        let e = keymap("lanch = [\"F2\"]\n").unwrap_err();
        assert!(e.contains("unknown action \"lanch\"") && e.contains("launch"), "{e}");

        let e = keymap("reload = [\"R\"]\n").unwrap_err();
        assert!(e.starts_with("reload: "), "says which action: {e}");
    }

    fn press(ctx: &egui::Context, key: Key, modifiers: Modifiers) {
        let event = Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        ctx.begin_pass(egui::RawInput { events: vec![event], ..Default::default() });
    }

    #[test]
    fn take_matches_modifiers_exactly_and_consumes_the_press() {
        let k = keymap("menu = [\"Ctrl+M\"]\n").unwrap();
        let ctx = egui::Context::default();

        press(&ctx, Key::Enter, Modifiers::ALT);
        assert!(!k.take(&ctx, Action::Launch), "Alt+Enter isn't Enter");
        let _ = ctx.end_pass();

        press(&ctx, Key::Enter, Modifiers::NONE);
        assert!(k.take(&ctx, Action::Launch));
        assert!(!k.take(&ctx, Action::Launch), "consumed");
        let _ = ctx.end_pass();

        // egui-winit sets both ctrl and command for Ctrl on Linux and Windows
        press(&ctx, Key::M, Modifiers::CTRL | Modifiers::COMMAND);
        assert!(k.take(&ctx, Action::Menu));
        let _ = ctx.end_pass();
    }

    #[test]
    fn take_press_tells_a_held_key_from_a_fresh_press() {
        // egui marks a press of a key that's already down as a repeat
        let k = Keymap::default();
        let ctx = egui::Context::default();
        press(&ctx, Key::ArrowUp, Modifiers::NONE);
        assert_eq!(k.take_press(&ctx, Action::Up), Some(Press::Fresh));
        assert_eq!(k.take_press(&ctx, Action::Up), None, "consumed");
        let _ = ctx.end_pass();
        press(&ctx, Key::ArrowUp, Modifiers::NONE);
        assert_eq!(k.take_press(&ctx, Action::Up), Some(Press::Repeat), "still held");
        let _ = ctx.end_pass();
    }
}
