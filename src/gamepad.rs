use crate::keys::{Action, Bindable, Bindings, Press};
use eframe::egui;
use std::{collections::HashMap, sync::mpsc, thread, time::Duration};

pub type Padmap = Bindings<Button>;

/// A held direction repeats after this many seconds...
const REPEAT_DELAY: f64 = 0.4;
/// ...and then every this many.
const REPEAT_EVERY: f64 = 0.1;
/// How far the stick has to tilt to count as a press...
const STICK_PRESS: f32 = 0.6;
/// ...and how far back it has to come to let go, so it doesn't flicker at the edge.
const STICK_RELEASE: f32 = 0.35;

/// A gamepad button, named after the Xbox layout and matched by position, so `A` is the
/// bottom face button on any pad. The left stick also acts as four direction buttons.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Button {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    LT,
    RT,
    View,
    Menu,
    Guide,
    LS,
    RS,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
}

impl Button {
    const NAMES: [(Button, &'static str); 21] = [
        (Button::A, "A"),
        (Button::B, "B"),
        (Button::X, "X"),
        (Button::Y, "Y"),
        (Button::LB, "LB"),
        (Button::RB, "RB"),
        (Button::LT, "LT"),
        (Button::RT, "RT"),
        (Button::View, "View"),
        (Button::Menu, "Menu"),
        (Button::Guide, "Guide"),
        (Button::LS, "LS"),
        (Button::RS, "RS"),
        (Button::DPadUp, "DPadUp"),
        (Button::DPadDown, "DPadDown"),
        (Button::DPadLeft, "DPadLeft"),
        (Button::DPadRight, "DPadRight"),
        (Button::LeftStickUp, "LeftStickUp"),
        (Button::LeftStickDown, "LeftStickDown"),
        (Button::LeftStickLeft, "LeftStickLeft"),
        (Button::LeftStickRight, "LeftStickRight"),
    ];

    /// gilrs names buttons by position; this maps them onto the Xbox names.
    fn from_gilrs(b: gilrs::Button) -> Option<Button> {
        use gilrs::Button as G;
        Some(match b {
            G::South => Button::A,
            G::East => Button::B,
            G::West => Button::X,
            G::North => Button::Y,
            G::LeftTrigger => Button::LB,
            G::RightTrigger => Button::RB,
            G::LeftTrigger2 => Button::LT,
            G::RightTrigger2 => Button::RT,
            G::Select => Button::View,
            G::Start => Button::Menu,
            G::Mode => Button::Guide,
            G::LeftThumb => Button::LS,
            G::RightThumb => Button::RS,
            G::DPadUp => Button::DPadUp,
            G::DPadDown => Button::DPadDown,
            G::DPadLeft => Button::DPadLeft,
            G::DPadRight => Button::DPadRight,
            _ => return None,
        })
    }
}

impl std::fmt::Display for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let (_, name) = Button::NAMES
            .iter()
            .find(|(b, _)| b == self)
            .expect("every button has a name");
        f.write_str(name)
    }
}

impl Bindable for Button {
    fn defaults(action: Action) -> &'static [&'static str] {
        match action {
            Action::Up => &["DPadUp", "LeftStickUp"],
            Action::Down => &["DPadDown", "LeftStickDown"],
            Action::Left => &["DPadLeft", "LeftStickLeft"],
            Action::Right => &["DPadRight", "LeftStickRight"],
            Action::PageUp => &["LB"],
            Action::PageDown => &["RB"],
            Action::Launch => &["A"],
            Action::Menu => &["Menu"],
            Action::Back => &["B"],
            Action::Reload => &["View"],
        }
    }

    /// Button names are case-insensitive.
    fn parse(s: &str) -> Result<Button, String> {
        let s = s.trim();
        Button::NAMES
            .iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(s))
            .map(|&(b, _)| b)
            .ok_or_else(|| {
                let names: Vec<&str> = Button::NAMES.iter().map(|(_, n)| *n).collect();
                format!(
                    "unknown button {s:?} (the buttons are {})",
                    names.join(", ")
                )
            })
    }
}

/// Moving around the grid repeats while the button is held, like a held arrow key.
/// Nothing else does, so holding B doesn't clear the search and then quit.
fn repeats(action: Action) -> bool {
    matches!(
        action,
        Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::PageUp
            | Action::PageDown
    )
}

/// What the gamepad thread passes on, for the pad with gilrs id `id`.
#[derive(Debug)]
enum Event {
    Button {
        id: usize,
        button: Button,
        pressed: bool,
    },
    /// The left stick moved. `value` runs from -1 to 1, up and right positive.
    Stick {
        id: usize,
        vertical: bool,
        value: f32,
    },
    Disconnected(usize),
}

/// A button being held, so its action can repeat.
struct Held {
    id: usize,
    button: Button,
    action: Action,
    next: f64,
}

/// Turns gamepad input into actions, a frame at a time. The pads are read on a
/// thread of their own, which wakes the UI when something happens.
pub struct Pad {
    pub map: Padmap,
    rx: mpsc::Receiver<Event>,
    pressed: Vec<(Action, Press)>,
    held: Vec<Held>,
    /// Which way each pad's stick is pushed on each axis: -1, 0 or 1.
    sticks: HashMap<(usize, bool), i8>,
}

impl Pad {
    fn new(map: Padmap, rx: mpsc::Receiver<Event>) -> Pad {
        Pad {
            map,
            rx,
            pressed: Vec::new(),
            held: Vec::new(),
            sticks: HashMap::new(),
        }
    }

    /// Starts reading the gamepads. When gamepads can't be read at all, this says so on
    /// the console and the pad simply never presses anything.
    pub fn start(ctx: &egui::Context, map: Padmap) -> Pad {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut gilrs = match gilrs::Gilrs::new() {
                Ok(g) => g,
                Err(e) => return eprintln!("rsgl: can't read gamepads: {e}"),
            };
            loop {
                // the timeout only bounds the wait; nothing happens on it
                let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_secs(1))) else {
                    continue;
                };
                let id = usize::from(ev.id);
                let ev = match ev.event {
                    gilrs::EventType::ButtonPressed(b, _) => {
                        Button::from_gilrs(b).map(|button| Event::Button {
                            id,
                            button,
                            pressed: true,
                        })
                    }
                    gilrs::EventType::ButtonReleased(b, _) => {
                        Button::from_gilrs(b).map(|button| Event::Button {
                            id,
                            button,
                            pressed: false,
                        })
                    }
                    gilrs::EventType::AxisChanged(gilrs::Axis::LeftStickX, value, _) => {
                        Some(Event::Stick {
                            id,
                            vertical: false,
                            value,
                        })
                    }
                    gilrs::EventType::AxisChanged(gilrs::Axis::LeftStickY, value, _) => {
                        Some(Event::Stick {
                            id,
                            vertical: true,
                            value,
                        })
                    }
                    gilrs::EventType::Disconnected => Some(Event::Disconnected(id)),
                    _ => None,
                };
                if let Some(ev) = ev {
                    if tx.send(ev).is_err() {
                        return; // the window is gone
                    }
                    ctx.request_repaint();
                }
            }
        });
        Pad::new(map, rx)
    }

    /// Reads what the pads did since the last frame. `now` is the frame time in seconds.
    /// While the window isn't focused everything is dropped, so a pad used in a game or
    /// a tool started from the menu doesn't also drive rsgl behind it.
    pub fn update(&mut self, now: f64, focused: bool) {
        self.pressed.clear();
        let events: Vec<Event> = self.rx.try_iter().collect();
        if !focused {
            self.held.clear();
            self.sticks.clear();
            return;
        }
        for ev in events {
            match ev {
                Event::Button {
                    id,
                    button,
                    pressed,
                } => self.button(id, button, pressed, now),
                Event::Stick {
                    id,
                    vertical,
                    value,
                } => self.stick(id, vertical, value, now),
                Event::Disconnected(id) => {
                    self.held.retain(|h| h.id != id);
                    self.sticks.retain(|&(i, _), _| i != id);
                }
            }
        }
        for h in self.held.iter_mut().filter(|h| h.next <= now) {
            self.pressed.push((h.action, Press::Repeat));
            h.next = now + REPEAT_EVERY;
        }
    }

    fn button(&mut self, id: usize, button: Button, pressed: bool, now: f64) {
        self.held.retain(|h| (h.id, h.button) != (id, button));
        let Some(action) = self.map.action(button).filter(|_| pressed) else {
            return;
        };
        self.pressed.push((action, Press::Fresh));
        if repeats(action) {
            self.held.push(Held {
                id,
                button,
                action,
                next: now + REPEAT_DELAY,
            });
        }
    }

    /// Treats the stick as four buttons: tilting past `STICK_PRESS` presses one and
    /// coming back under `STICK_RELEASE` lets go of it.
    fn stick(&mut self, id: usize, vertical: bool, value: f32, now: f64) {
        let dir = self.sticks.entry((id, vertical)).or_insert(0);
        let old = *dir;
        let new = if old != 0 && value * f32::from(old) > STICK_RELEASE {
            old
        } else if value.abs() > STICK_PRESS {
            value.signum() as i8
        } else {
            0
        };
        if new == old {
            return;
        }
        *dir = new;
        let button = |d: i8| match (vertical, d) {
            (true, 1) => Button::LeftStickUp,
            (true, _) => Button::LeftStickDown,
            (false, 1) => Button::LeftStickRight,
            (false, _) => Button::LeftStickLeft,
        };
        if old != 0 {
            self.button(id, button(old), false, now);
        }
        if new != 0 {
            self.button(id, button(new), true, now);
        }
    }

    /// True if a button bound to `action` was pressed (or repeated) this frame.
    /// Like `Keymap::take`, the press is used up.
    pub fn take(&mut self, action: Action) -> bool {
        self.take_press(action).is_some()
    }

    /// `take`, saying whether the button was pressed just now or is repeating.
    pub fn take_press(&mut self, action: Action) -> Option<Press> {
        let mut hit = None;
        self.pressed.retain(|&(a, press)| {
            if a == action {
                hit = Press::or(hit, Some(press));
            }
            a != action
        });
        hit
    }

    /// When the next held button repeats, so the UI can wake up for it.
    pub fn next_repeat(&self) -> Option<f64> {
        self.held.iter().map(|h| h.next).min_by(f64::total_cmp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn pad(text: &str) -> (Pad, mpsc::Sender<Event>) {
        let cfg: BTreeMap<String, Vec<String>> = toml::from_str(text).unwrap();
        let (tx, rx) = mpsc::channel();
        (Pad::new(Padmap::try_from(cfg).unwrap(), rx), tx)
    }

    fn press(tx: &mpsc::Sender<Event>, button: Button, pressed: bool) {
        tx.send(Event::Button {
            id: 0,
            button,
            pressed,
        })
        .unwrap();
    }

    #[test]
    fn button_names_parse_both_ways() {
        for &(b, name) in &Button::NAMES {
            assert_eq!(Button::parse(name), Ok(b));
            assert_eq!(b.to_string(), name);
        }
        assert_eq!(Button::parse("dpadup"), Ok(Button::DPadUp));
        let e = Button::parse("Square").unwrap_err();
        assert!(
            e.contains("unknown button \"Square\"") && e.contains("LeftStickUp"),
            "{e}"
        );
    }

    #[test]
    fn defaults_and_config_errors() {
        let (p, _) = pad("");
        assert_eq!(p.map.action(Button::A), Some(Action::Launch));
        assert_eq!(p.map.action(Button::LeftStickLeft), Some(Action::Left));
        assert_eq!(p.map.action(Button::Guide), None);

        let cfg: BTreeMap<String, Vec<String>> = toml::from_str("back = [\"A\"]").unwrap();
        let e = Padmap::try_from(cfg).unwrap_err();
        assert!(e.contains("A is bound to both launch and back"), "{e}");
    }

    #[test]
    fn a_press_is_an_action_for_one_frame() {
        let (mut p, tx) = pad("menu = [\"Y\"]");
        press(&tx, Button::Y, true);
        p.update(0.0, true);
        assert!(p.take(Action::Menu));
        assert!(!p.take(Action::Menu), "used up");

        press(&tx, Button::A, true);
        p.update(0.1, true);
        p.update(0.2, true);
        assert!(
            !p.take(Action::Launch),
            "not taken in its frame, so it's gone"
        );
        assert_eq!(p.next_repeat(), None, "launch doesn't repeat");
    }

    #[test]
    fn held_directions_repeat_until_released() {
        let (mut p, tx) = pad("");
        press(&tx, Button::DPadDown, true);
        p.update(1.0, true);
        assert_eq!(p.take_press(Action::Down), Some(Press::Fresh));
        p.update(1.3, true);
        assert!(!p.take(Action::Down), "not before the delay");
        assert_eq!(p.next_repeat(), Some(1.0 + REPEAT_DELAY));
        p.update(1.45, true);
        assert_eq!(p.take_press(Action::Down), Some(Press::Repeat));
        p.update(1.6, true);
        assert!(p.take(Action::Down));

        press(&tx, Button::DPadDown, false);
        p.update(2.0, true);
        assert!(!p.take(Action::Down));
        assert_eq!(p.next_repeat(), None);
    }

    #[test]
    fn the_stick_is_four_buttons_with_some_slack() {
        let (mut p, tx) = pad("");
        let tilt = |value| {
            tx.send(Event::Stick {
                id: 0,
                vertical: true,
                value,
            })
            .unwrap()
        };
        tilt(0.3);
        p.update(0.0, true);
        assert!(!p.take(Action::Up), "inside the dead zone");
        tilt(0.7);
        p.update(0.0, true);
        assert!(p.take(Action::Up));
        tilt(0.4);
        tilt(0.8);
        p.update(0.01, true);
        assert!(!p.take(Action::Up), "still held, not a second press");
        tilt(-0.9);
        p.update(0.02, true);
        assert!(
            p.take(Action::Down) && !p.take(Action::Up),
            "straight from up to down"
        );
        tilt(0.0);
        p.update(5.0, true);
        assert_eq!(p.next_repeat(), None, "released");
    }

    #[test]
    fn nothing_happens_while_the_window_is_in_the_background() {
        let (mut p, tx) = pad("");
        press(&tx, Button::DPadUp, true);
        press(&tx, Button::B, true);
        p.update(0.0, false);
        assert!(!p.take(Action::Up) && !p.take(Action::Back));
        assert_eq!(p.next_repeat(), None);
    }
}
