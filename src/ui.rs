use crate::{
    api_igdb, api_steamgriddb,
    config::{self, Game, MenuAction, MenuEntry, Settings},
    gamepad::Pad,
    keys::{Action, Keymap, Press},
    launch, playtime, spawn,
    status::{self, Part, Status},
    topbar::{Complete, SortBy, Target, Topbar, Widget, WidgetId},
    Launched,
};
use eframe::egui;
use egui::{vec2, Align, Align2, Color32, FontId, RichText, Stroke};
use std::{
    cell::RefCell,
    cmp::Reverse,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::mpsc,
    thread,
};

const ACCENT: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xf7);
const BG: Color32 = Color32::from_rgb(0x0f, 0x11, 0x16);
const SURFACE: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x26);
const GAP: f32 = 22.0;
const LABEL_H: f32 = 40.0;
/// How far the selected/hovered card grows on each side.
const LIFT: f32 = 5.0;
const OUTLINE: f32 = 2.5;
/// Room around the grid so a lifted card and its outline aren't clipped by the scroll area.
const PAD: f32 = LIFT + OUTLINE + 1.0;
/// Background crossfade length in seconds.
const FADE: f32 = 0.4;
/// How bright the background is drawn (0-255), so cards stay readable.
const BG_DIM: u8 = 90;
/// Widest the search bar's text gets, so it doesn't stretch across ultrawide screens.
const SEARCH_MAX_W: f32 = 720.0;
const SEARCH_FONT: f32 = 18.0;
const SEARCH_MARGIN: egui::Margin = egui::Margin::symmetric(14, 10);
/// Between the top bar's widgets, and between its zones.
const WIDGET_GAP: f32 = 16.0;
const TOOL_FONT: f32 = 15.0;
/// Left and right of a top bar button's title.
const TOOL_PAD: f32 = 14.0;
/// Room for a top bar menu's arrow, after its title.
const ARROW_W: f32 = 16.0;
/// A top bar toggle's switch, after its title.
const SWITCH: egui::Vec2 = vec2(30.0, 16.0);
/// Between a toggle's title and its switch.
const SWITCH_GAP: f32 = 10.0;
const TOOL_HOVER: Color32 = Color32::from_rgb(0x26, 0x2a, 0x36);
/// How bright a game that isn't installed is drawn (0-1), see `Game::installed`.
const NOT_INSTALLED_DIM: f32 = 0.35;

/// Opens the window and runs the launcher until it closes. Returns the game that
/// was launched, if the window closed because of a launch.
pub fn run(
    settings: Settings,
    error: Option<String>,
    fullscreen: bool,
    search: String,
) -> eframe::Result<Option<Launched>> {
    let Settings { games, ui: cfg, api, playtime: pt_cfg, keyboard, gamepad, status, topbar, library } = settings;
    let key = api.steamgriddb_key();
    let igdb = api.igdb_credentials();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([520.0, 400.0])
            .with_title("Games")
            .with_app_id("rsgl")
            .with_maximized(cfg.start_maximized && !fullscreen)
            .with_fullscreen(fullscreen),
        ..Default::default()
    };

    let playtime = pt_cfg.enabled.then(playtime::load);
    let launched = Rc::new(RefCell::new(None));
    let out = launched.clone();

    eframe::run_native(
        "Games",
        options,
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let rx = fetch(&cc.egui_ctx, &games, key, cfg.blur);
            let meta = fetch_meta(&cc.egui_ctx, &games, status.uses_igdb || topbar.uses_igdb, igdb);
            Ok(Box::new(App {
                games,
                error,
                query: search.clone(),
                prev_query: search,
                selected: 0,
                selected_game: None,
                prev_sort: topbar.sort(),
                page_rows: 1,
                card_w: cfg.card_width.clamp(80.0, 400.0),
                controls: Controls { keys: keyboard, pad: Pad::start(&cc.egui_ctx, gamepad) },
                rx,
                meta,
                status,
                topbar,
                library,
                bg: None,
                prev_bg: None,
                bg_since: 0.0,
                playtime,
                launched,
                menu: None,
                swallow_click: false,
                focus: None,
                bar: Vec::new(),
                maximize: cfg.start_maximized && !fullscreen,
                fullscreen,
            }))
        }),
    )?;
    Ok(out.take())
}

/// Starts downloading and blurring art for `games` in the background.
/// The updates refer to games by their index in `games`.
fn fetch(ctx: &egui::Context, games: &[Game], key: Option<String>, blur: f32) -> mpsc::Receiver<api_steamgriddb::Update> {
    let jobs: Vec<api_steamgriddb::Job> = games
        .iter()
        .enumerate()
        .map(|(i, g)| api_steamgriddb::Job {
            index: i,
            name: g.name.clone(),
            cover: g.cover.clone().filter(|p| p.exists()),
            art_id: g.art_id,
            grid_id: g.grid_id,
            hero_id: g.hero_id,
        })
        .collect();
    let ctx = ctx.clone();
    api_steamgriddb::spawn_fetcher(jobs, key, blur.clamp(0.0, 50.0), move || ctx.request_repaint())
}

/// Starts looking up the IGDB metadata of games that don't have it yet. With
/// credentials that's every game, so `g:` can search all their genres; without,
/// only the ones that show it (see `Game::needs_meta`), so the fetcher says why
/// they stay blank. `bars_use_igdb`: the status or top bar shows some of it.
fn fetch_meta(
    ctx: &egui::Context,
    games: &[Game],
    bars_use_igdb: bool,
    credentials: Option<(String, String)>,
) -> mpsc::Receiver<(usize, api_igdb::Meta)> {
    let jobs = games
        .iter()
        .enumerate()
        .filter(|(_, g)| match credentials {
            Some(_) => g.meta.is_none(),
            None => g.needs_meta(bars_use_igdb),
        })
        .map(|(i, g)| (i, g.name.clone(), g.igdb_id))
        .collect();
    let ctx = ctx.clone();
    api_igdb::spawn_meta_fetcher(jobs, credentials, move || ctx.request_repaint())
}

/// How a status part or top bar label is drawn.
fn part_text(text: &str, bold: bool) -> RichText {
    let text = RichText::new(text);
    if bold {
        text.strong().size(15.0)
    } else {
        text.color(Color32::from_gray(140))
    }
}

/// One side of the status bar: the parts in order, with the separator between them.
fn status_labels(ui: &mut egui::Ui, separator: &str, parts: &[(String, bool)]) {
    for (k, (text, bold)) in parts.iter().enumerate() {
        if k > 0 {
            ui.label(RichText::new(separator).color(Color32::from_gray(140)));
        }
        ui.label(part_text(text, *bold));
    }
}

/// A top bar widget, ready to draw: a label's text is filled in (and a label
/// that's left out is gone).
enum Item<'a> {
    Label(String, bool),
    Search(Option<&'a str>),
    /// A button, menu, toggle or sort.
    Tool(WidgetId, &'a str, Kind),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Button,
    /// A menu or a sort: a dropdown.
    Menu,
    /// Whether it's on.
    Toggle(bool),
}

impl Kind {
    /// The room after the title: a menu's arrow or a toggle's switch.
    fn extra(self) -> f32 {
        match self {
            Kind::Button => 0.0,
            Kind::Menu => ARROW_W,
            Kind::Toggle(_) => SWITCH_GAP + SWITCH.x,
        }
    }
}

fn tool_text(title: &str) -> RichText {
    RichText::new(title).size(TOOL_FONT)
}

/// A top bar button, menu or toggle: its title on a box like the search box, with an
/// arrow for a menu or a switch for a toggle, and outlined while it has the focus. When
/// the zone is short of room the title is cut off.
fn tool_button(ui: &mut egui::Ui, title: &str, kind: Kind, focused: bool) -> egui::Response {
    let extra = kind.extra();
    let max_text = (ui.available_width() - 2.0 * TOOL_PAD - extra).max(0.0);
    let galley = egui::WidgetText::from(tool_text(title)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_text,
        egui::TextStyle::Body,
    );
    let size = vec2(galley.size().x + 2.0 * TOOL_PAD + extra, bar_height(ui));
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let radius = egui::CornerRadius::same(10);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, if resp.hovered() { TOOL_HOVER } else { SURFACE });
        if focused {
            painter.rect_stroke(rect, radius, Stroke::new(OUTLINE, ACCENT), egui::StrokeKind::Inside);
        }
        let color = if focused || resp.hovered() { Color32::WHITE } else { Color32::from_gray(210) };
        match kind {
            Kind::Button => {}
            Kind::Menu => {
                let c = egui::pos2(rect.right() - TOOL_PAD - ARROW_W / 2.0 + 4.0, rect.center().y);
                let points = vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.0)];
                painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
            }
            Kind::Toggle(on) => {
                let track = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - TOOL_PAD - SWITCH.x / 2.0, rect.center().y),
                    SWITCH,
                );
                let t = ui.ctx().animate_bool_with_time(resp.id, on, 0.1);
                let r = SWITCH.y / 2.0;
                let fill = Color32::from_gray(70).lerp_to_gamma(ACCENT, t);
                painter.rect_filled(track, egui::CornerRadius::same(r as u8), fill);
                let knob = egui::pos2(egui::lerp(track.left() + r..=track.right() - r, t), track.center().y);
                painter.circle_filled(knob, r - 2.5, Color32::WHITE);
            }
        }
        let pos = egui::pos2(rect.left() + TOOL_PAD, rect.center().y - galley.size().y / 2.0);
        painter.galley(pos, galley, color);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl Item<'_> {
    /// How wide it wants to be, before it's squeezed.
    fn width(&self, ui: &egui::Ui) -> f32 {
        match self {
            Item::Label(text, bold) => {
                let galley = egui::WidgetText::from(part_text(text, *bold)).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                );
                galley.size().x
            }
            Item::Search(_) => SEARCH_MAX_W + SEARCH_MARGIN.sum().x,
            Item::Tool(_, title, kind) => {
                let galley = egui::WidgetText::from(tool_text(title)).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                );
                galley.size().x + 2.0 * TOOL_PAD + kind.extra()
            }
        }
    }
}

/// The widgets of top bar zone `zone`, with the labels filled in from `vars` and `env`.
fn items<'a>(zone: usize, widgets: &'a [Widget], vars: &config::Vars, env: &dyn Fn(&str) -> String) -> Vec<Item<'a>> {
    widgets
        .iter()
        .enumerate()
        .filter_map(|(i, w)| match w {
            Widget::Label(part) => Some(Item::Label(part.render(vars, env)?, part.bold)),
            Widget::Search { placeholder, .. } => Some(Item::Search(placeholder.as_deref())),
            Widget::Button(tool) => Some(Item::Tool((zone, i), &tool.title, Kind::Button)),
            Widget::Menu { title, .. } => Some(Item::Tool((zone, i), title, Kind::Menu)),
            Widget::Toggle(t) => Some(Item::Tool((zone, i), &t.title, Kind::Toggle(t.on))),
            // the order it's on: the bar is short of room, and the arrow says it's a dropdown
            Widget::Sort(sort) => Some(Item::Tool((zone, i), sort.options[sort.current].label(), Kind::Menu)),
        })
        .collect()
}

/// What the top bar drew that can be used: the search box, and the buttons, menus,
/// toggles and sort that aren't cut off.
struct Drawn {
    search: Option<egui::Response>,
    tools: Vec<(WidgetId, egui::Response)>,
}

/// The search box, as tall as the top bar.
fn bar_height(ui: &egui::Ui) -> f32 {
    ui.fonts(|f| f.row_height(&FontId::proportional(SEARCH_FONT))) + SEARCH_MARGIN.sum().y
}

/// Draws the top bar: the center zone centered, and the left and right zones in
/// the room that's left on either side, cut off rather than overlapping it. The
/// center gets at most half the width when there's something beside it, so a
/// narrow window still shows some of the sides. `focus` is outlined. `ghost` is
/// autocomplete's suggestion, shown after the search and added by Tab.
fn draw_topbar(
    ui: &mut egui::Ui,
    zones: [Vec<Item>; 3],
    query: &mut String,
    ghost: Option<&str>,
    hint: &str,
    interactive: bool,
    focus: Option<WidgetId>,
) -> Drawn {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), bar_height(ui)), egui::Sense::hover());
    let natural = |items: &[Item]| -> f32 {
        let gaps = items.len().saturating_sub(1) as f32 * WIDGET_GAP;
        items.iter().map(|i| i.width(ui)).sum::<f32>() + gaps
    };
    let [left, center, right] = zones;
    let sides = !left.is_empty() || !right.is_empty();
    let center_w = natural(&center).min(if sides { bar.width() / 2.0 } else { bar.width() });
    let mid = egui::Rect::from_center_size(bar.center(), vec2(center_w, bar.height()));
    // with nothing in the center, the sides meet in the middle
    let gap = if center.is_empty() { WIDGET_GAP / 2.0 } else { WIDGET_GAP };
    let left_rect = egui::Rect::from_x_y_ranges(bar.left()..=mid.left() - gap, bar.y_range());
    let right_rect = egui::Rect::from_x_y_ranges(mid.right() + gap..=bar.right(), bar.y_range());

    let mut drawn = Drawn { search: None, tools: Vec::new() };
    for (rect, items, right_to_left) in [(left_rect, left, false), (mid, center, false), (right_rect, right, true)] {
        if items.is_empty() || rect.width() <= 0.0 {
            continue;
        }
        let layout = match right_to_left {
            true => egui::Layout::right_to_left(Align::Center),
            false => egui::Layout::left_to_right(Align::Center),
        };
        let mut zone = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
        zone.set_clip_rect(rect.intersect(ui.clip_rect()));
        zone.spacing_mut().item_spacing.x = WIDGET_GAP;
        // right to left draws from the edge inwards, so the last widget goes first
        let ordered: Vec<_> = if right_to_left { items.into_iter().rev().collect() } else { items };
        for item in ordered {
            match item {
                Item::Label(text, bold) => {
                    zone.add(egui::Label::new(part_text(&text, bold)).truncate().selectable(false));
                }
                Item::Tool(id, title, kind) => {
                    let resp = tool_button(&mut zone, title, kind, focus == Some(id));
                    // one past the edge of a short zone can't be seen, so it can't have the focus either
                    if zone.clip_rect().expand(0.5).contains_rect(resp.rect) {
                        drawn.tools.push((id, resp));
                    }
                }
                Item::Search(placeholder) => {
                    // It always has the keyboard focus, so egui's focus outline would say
                    // nothing, and would look like a second focus beside the pad's. Hovering
                    // doesn't grow it either: the zone is exactly as wide and would clip it.
                    let v = zone.visuals_mut();
                    v.selection.stroke = Stroke::NONE;
                    v.widgets.hovered.expansion = 0.0;
                    v.widgets.active.expansion = 0.0;
                    // Tab is autocomplete's, so it doesn't move the focus away either
                    let mut out = egui::TextEdit::singleline(query)
                        .hint_text(placeholder.unwrap_or(hint))
                        .font(FontId::proportional(SEARCH_FONT))
                        .margin(SEARCH_MARGIN)
                        .desired_width(SEARCH_MAX_W)
                        .lock_focus(true)
                        .interactive(interactive)
                        .show(&mut zone);
                    let end = egui::text::CCursor::new(query.chars().count());
                    let at_end = out.state.cursor.char_range() == Some(egui::text::CCursorRange::one(end));
                    if let Some(ghost) = ghost.filter(|_| at_end && interactive) {
                        let pos = out.galley_pos + vec2(out.galley.rect.right(), 0.0);
                        let color = zone.visuals().weak_text_color();
                        let font = FontId::proportional(SEARCH_FONT);
                        zone.painter().with_clip_rect(out.text_clip_rect).text(pos, Align2::LEFT_TOP, ghost, font, color);
                        if zone.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                            query.push_str(ghost);
                            let end = egui::text::CCursor::new(query.chars().count());
                            out.state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            out.state.store(zone.ctx(), out.response.id);
                            zone.ctx().request_repaint(); // the grid was drawn for the search before it
                        }
                    }
                    drawn.search = Some(out.response);
                }
            }
        }
    }
    drawn
}

/// Opens `dir` in the file manager.
fn open_folder(dir: &Path) {
    #[cfg(windows)]
    let (program, target) = (
        "explorer.exe",
        std::ffi::OsString::from(dir.to_string_lossy().replace('/', "\\")),
    );
    #[cfg(not(windows))]
    let (program, target) = ("xdg-open", dir.as_os_str().to_owned());

    let child = Command::new(program)
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut c) => {
            thread::spawn(move || c.wait()); // reap it so it doesn't linger as a zombie
        }
        Err(e) => eprintln!("can't run {program}: {e}"),
    }
}

fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.extreme_bg_color = SURFACE; // text box background
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.5_f32, ACCENT);
    let r = egui::CornerRadius::same(10);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    ctx.set_visuals(v);
}

/// Draws one game card and returns its click/hover response.
fn card(ui: &mut egui::Ui, g: &Game, w: f32, selected: bool, scroll: bool) -> egui::Response {
    let cover_h = w * 1.5;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, cover_h + LABEL_H), egui::Sense::click());

    if selected && scroll {
        ui.scroll_to_rect(rect.expand(PAD), None);
    }
    if !ui.is_rect_visible(rect) {
        return resp; // skip drawing off-screen cards
    }

    let lift = ui
        .ctx()
        .animate_bool_with_time(resp.id, selected || resp.hovered(), 0.12);
    let radius = egui::CornerRadius::same(10);
    let cover = egui::Rect::from_min_size(rect.min, vec2(w, cover_h)).expand(lift * LIFT);
    // a game that isn't installed is drawn dimmed
    let dim = if g.installed() { 1.0 } else { NOT_INSTALLED_DIM };

    match g.cover.as_ref().filter(|c| c.exists()) {
        Some(path) => {
            egui::Image::new(format!("file://{}", path.display()))
                .corner_radius(radius)
                .tint(Color32::WHITE.gamma_multiply(dim))
                .paint_at(ui, cover);
        }
        None => {
            // colored placeholder derived from the name
            let hash = g.name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
            let hue = (hash % 360) as f32 / 360.0;
            let fill = Color32::from(egui::ecolor::Hsva::new(hue, 0.45, 0.35 * dim, 1.0));
            ui.painter().rect_filled(cover, radius, fill);
            let initial: String = g.name.chars().next().unwrap_or('?').to_uppercase().collect();
            ui.painter().text(
                cover.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::proportional(w * 0.4),
                Color32::from_white_alpha((190.0 * dim) as u8),
            );
        }
    }

    if lift > 0.0 {
        ui.painter().rect_stroke(
            cover,
            radius,
            Stroke::new(OUTLINE, ACCENT.gamma_multiply(lift)),
            egui::StrokeKind::Outside,
        );
    }

    let label_rect =
        egui::Rect::from_min_size(rect.min + vec2(0.0, cover_h + 8.0), vec2(w, LABEL_H - 8.0));
    let color = if selected { Color32::WHITE } else { Color32::from_gray(170) }.gamma_multiply(dim);
    ui.put(
        label_rect,
        egui::Label::new(RichText::new(&g.title).size(13.0).color(color))
            .wrap_mode(egui::TextWrapMode::Wrap)
            .selectable(false),
    );

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

struct App {
    games: Vec<Game>,
    error: Option<String>,
    query: String,
    prev_query: String,
    selected: usize, // position within the filtered list
    selected_game: Option<usize>, // the game at `selected` last frame, so it stays selected when the order changes under it
    prev_sort: SortBy,
    page_rows: usize, // rows that fit in the grid, for PageUp/PageDown
    card_w: f32,
    controls: Controls,
    rx: mpsc::Receiver<api_steamgriddb::Update>,
    meta: mpsc::Receiver<(usize, api_igdb::Meta)>, // IGDB metadata, by game index
    status: Status,
    topbar: Topbar,
    library: config::Library, // to build the games and top bar again when a toggle switches a variable
    bg: Option<PathBuf>,      // background currently fading in / shown
    prev_bg: Option<PathBuf>, // background fading out
    bg_since: f64,            // when `bg` last changed
    playtime: Option<playtime::Playtime>, // None when playtime is disabled
    launched: Rc<RefCell<Option<Launched>>>,
    menu: Option<Menu>, // the open right-click or top bar menu
    swallow_click: bool, // a click closed the menu, so it must not also launch the card under it
    focus: Option<WidgetId>, // the top bar widget the arrows and pad are on; None while they're on the grid
    bar: Vec<WidgetId>, // the top bar's buttons, menus and toggles as drawn last frame, left to right, for moving the focus
    maximize: bool, // send a maximize request on the first frame
    fullscreen: bool, // same for fullscreen (-f)
}

/// The keyboard and the gamepads, asked together: an action happens when either one
/// triggers it.
struct Controls {
    keys: Keymap,
    pad: Pad,
}

impl Controls {
    fn take(&mut self, ctx: &egui::Context, action: Action) -> bool {
        // `|`, not `||`: both get used up, so neither carries over
        self.keys.take(ctx, action) | self.pad.take(action)
    }

    /// `take`, saying whether it was pressed just now or is repeating because it's held.
    fn take_press(&mut self, ctx: &egui::Context, action: Action) -> Option<Press> {
        Press::or(self.keys.take_press(ctx, action), self.pad.take_press(action))
    }
}

/// An open menu.
struct Menu {
    source: Source,
    pos: egui::Pos2, // top-left corner
    selected: usize, // highlighted entry
}

/// Whose entries a menu shows.
#[derive(Clone, Copy)]
enum Source {
    /// A game's right-click menu, by index into `games`.
    Game(usize),
    /// A top bar button (its one entry) or menu.
    Topbar(WidgetId),
    /// The top bar's sort: its options.
    Sort(WidgetId),
}

impl Source {
    /// The tools it runs. A sort has none.
    fn entries<'a>(self, games: &'a [Game], topbar: &'a Topbar) -> &'a [MenuEntry] {
        match self {
            Source::Game(i) => &games[i].menu,
            Source::Topbar(id) => topbar.widget(id).tools(),
            Source::Sort(_) => &[],
        }
    }

    /// What the menu shows: each entry's title, and whether it can be picked.
    fn items<'a>(self, games: &'a [Game], topbar: &'a Topbar) -> Vec<(&'a str, bool)> {
        if let Source::Sort(id) = self {
            let Widget::Sort(sort) = topbar.widget(id) else { unreachable!("a sort's menu is from a sort") };
            return sort.options.iter().map(|o| (o.label(), true)).collect();
        }
        let entries = self.entries(games, topbar).iter();
        // "open folder" with no workdir to open does nothing
        entries.map(|e| (e.title.as_str(), e.action != MenuAction::OpenFolder || e.workdir.is_some())).collect()
    }
}

/// A search, lowercased: the name text plus its `t:` tag and `g:` genre filters.
/// "t:coop g:shoot quake" is the name "quake", tag "coop" and genre "shoot".
#[derive(Debug, Default, PartialEq)]
struct Query {
    name: String,
    tags: Vec<String>,
    genres: Vec<String>,
}

impl Query {
    fn parse(query: &str) -> Query {
        let mut q = Query::default();
        let mut name = Vec::new();
        for word in query.to_lowercase().split_whitespace() {
            let (list, filter) = match (word.strip_prefix("t:"), word.strip_prefix("g:")) {
                (Some(tag), _) => (&mut q.tags, tag),
                (_, Some(genre)) => (&mut q.genres, genre),
                _ => (&mut name, word),
            };
            if !filter.is_empty() {
                // a bare `t:` or `g:` is still being typed, don't filter yet
                list.push(filter.to_string());
            }
        }
        q.name = name.join(" ");
        q
    }

    fn matches_game(&self, g: &Game) -> bool {
        self.matches(&g.title, &g.tags, genres(g))
    }

    /// Tags match from their start, genres anywhere in them, since IGDB's have
    /// spaces ("Real Time Strategy (RTS)") and the search splits on whitespace.
    fn matches(&self, title: &str, tags: &[String], genres: &[String]) -> bool {
        title.to_lowercase().contains(&self.name)
            && self.tags.iter().all(|t| tags.iter().any(|tag| tag.to_lowercase().starts_with(t)))
            && self.genres.iter().all(|g| genres.iter().any(|genre| genre.to_lowercase().contains(g)))
    }
}

/// IGDB's genres for `g`, none until they're known.
fn genres(g: &Game) -> &[String] {
    g.meta.as_ref().map_or(&[], |m| &m.genres)
}

/// What autocomplete adds to the end of `query`: the rest of the name, `t:` tag or
/// `g:` genre word being typed, the first in alphabetical order that's longer. Only
/// from the games the rest of the search shows, so taking it never empties the grid.
fn suggestion(query: &str, games: &[Game], complete: &[Complete]) -> Option<String> {
    if complete.is_empty() || query.ends_with(char::is_whitespace) {
        return None;
    }
    let word = query.split_whitespace().next_back()?;
    let rest = Query::parse(&query[..query.len() - word.len()]);
    let shown = games.iter().filter(|g| rest.matches_game(g));
    let word = word.to_lowercase();
    let (typed, candidates): (String, Vec<&str>) = match (word.strip_prefix("t:"), word.strip_prefix("g:")) {
        (Some(tag), _) if complete.contains(&Complete::Tags) => {
            (tag.into(), shown.flat_map(|g| &g.tags).map(String::as_str).collect())
        }
        // a word at a time, since the search splits on whitespace; "(RTS)" is "RTS"
        (_, Some(genre)) if complete.contains(&Complete::Genres) => {
            let words = shown.flat_map(genres).flat_map(|g| g.split_whitespace());
            (genre.into(), words.map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).collect())
        }
        // all of the name, which can have spaces, not just this word of it
        (None, None) if complete.contains(&Complete::Names) => {
            (Query::parse(query).name, shown.map(|g| g.title.as_str()).collect())
        }
        _ => return None,
    };
    let rests = candidates.into_iter().filter_map(|c| after_prefix(c, &typed)).filter(|r| !r.is_empty());
    rests.min_by_key(|r| r.to_lowercase()).map(str::to_string)
}

/// What's left of `text` after `prefix` (lowercase), if it starts with it ignoring case.
fn after_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if !text.to_lowercase().starts_with(prefix) {
        return None;
    }
    // lowercasing can change a character's length, so find where the prefix ends in `text`
    let mut ends = text.char_indices().map(|(i, _)| i).chain([text.len()]);
    ends.find(|&i| text[..i].to_lowercase() == prefix).map(|i| &text[i..])
}

impl App {
    /// Indices of the games matching the search, in the top bar's sort order.
    fn filtered(&self) -> Vec<usize> {
        let query = Query::parse(&self.query);
        let mut shown: Vec<usize> = (0..self.games.len())
            .filter(|&i| query.matches_game(&self.games[i]))
            .collect();
        sort(&mut shown, &self.games, self.playtime.as_ref(), self.topbar.sort());
        shown
    }

    /// Builds the games and the top bar again for the toggles as they are now, since
    /// `[vars]` are filled in when the games are built. What was found out about the
    /// games since (art, IGDB data) is kept; a toggle can't change their names or order.
    fn rebuild(&mut self) -> Result<(), String> {
        let (mut games, topbar) = self.library.build(Some(&self.topbar))?;
        assert_eq!(games.len(), self.games.len(), "the same games from the same library");
        for (new, old) in games.iter_mut().zip(&mut self.games) {
            assert_eq!(new.name, old.name, "a toggle can't change a game's name");
            new.cover = old.cover.take();
            new.background = old.background.take();
            if let Some(meta) = old.meta.take() {
                new.set_meta(meta);
            }
        }
        self.games = games;
        self.topbar = topbar;
        Ok(())
    }

    /// Reads the config again and restarts the fetcher for the new game list.
    /// On an error the current games are kept. Window and playtime settings
    /// still need a restart.
    fn reload(&mut self, ctx: &egui::Context) {
        let Settings { mut games, ui, api, keyboard, gamepad, status, topbar, library, .. } = match config::load() {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        // keep showing the current backgrounds until the fetcher sends them again
        for g in &mut games {
            g.background = self.games.iter().find(|o| o.name == g.name).and_then(|o| o.background.clone());
        }
        // replacing rx drops the old receiver, which stops the old fetcher
        self.rx = fetch(ctx, &games, api.steamgriddb_key(), ui.blur);
        self.meta = fetch_meta(ctx, &games, status.uses_igdb || topbar.uses_igdb, api.igdb_credentials());
        self.games = games;
        self.status = status;
        self.topbar = topbar;
        self.library = library;
        // the reloaded sort may be another one, but the selection is found again by name
        self.prev_sort = self.topbar.sort();
        self.menu = None;
        self.focus = None; // the widget it was on may be gone
        self.bar.clear();
        self.card_w = ui.card_width.clamp(80.0, 400.0);
        self.controls.keys = keyboard;
        self.controls.pad.map = gamepad;
        self.error = None;
    }

    /// Opens the right-click menu of game `game` at `pos`. A game with no menu entries
    /// just opens its folder, which is what right-click did before menus existed.
    fn open_menu(&mut self, ctx: &egui::Context, game: usize, pos: egui::Pos2) {
        let g = &self.games[game];
        if g.menu.is_empty() {
            if let Some(dir) = &g.workdir {
                open_folder(dir);
            }
        } else {
            self.menu = Some(Menu { source: Source::Game(game), pos, selected: 0 });
            ctx.request_repaint();
        }
    }

    /// Shows `msg` in red above the grid (until the next F5 or failure) and prints it
    /// to the terminal, so a launch that didn't happen isn't silent.
    fn fail(&mut self, msg: String) {
        eprintln!("rsgl: {msg}");
        self.error = Some(msg);
    }

    /// Uses entry `k` of `source`: picks a sort, or runs a tool, see `run_entry`.
    fn pick(&mut self, ctx: &egui::Context, source: Source, k: usize) {
        match source {
            Source::Sort(id) => self.topbar.set_sort(id, k),
            Source::Game(_) | Source::Topbar(_) => self.run_entry(ctx, source, k),
        }
    }

    /// Runs entry `k` of `source`. A tool runs in the background and rsgl stays open; a
    /// game's entry with `launch = true` is an alternate way to start the game, so rsgl
    /// closes and the playtime is tracked, like a normal launch.
    fn run_entry(&mut self, ctx: &egui::Context, source: Source, k: usize) {
        let toggles = self.topbar.toggles();
        let e = &source.entries(&self.games, &self.topbar)[k];
        match &e.action {
            MenuAction::OpenFolder => {
                if let Some(dir) = &e.workdir {
                    open_folder(dir);
                }
            }
            MenuAction::Exec(cmd) => match spawn(cmd, e.workdir.as_deref(), &e.env.with(&toggles)) {
                Ok(child) if e.launch => {
                    let Source::Game(gi) = source else { unreachable!("only a game's entries launch it") };
                    let g = &self.games[gi];
                    *self.launched.borrow_mut() =
                        Some(Launched { name: g.name.clone(), pgid: child.id(), playtime: g.playtime });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                // rsgl stays open, so reap the tool when it exits instead of leaving a zombie.
                // Repaint then too: an install or uninstall tool changes which games are dimmed
                Ok(mut child) => {
                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        let _ = child.wait();
                        ctx.request_repaint();
                    });
                }
                Err(err) => self.fail(format!("Couldn't run \"{}\": {err}", e.title)),
            },
        }
    }

    /// Draws the open menu on top of everything. Returns the entry picked with the
    /// mouse, and whether this frame's press landed outside the menu.
    fn show_menu(&mut self, ctx: &egui::Context, menu_open: bool) -> (Option<(Source, usize)>, bool) {
        let Some(m) = &mut self.menu else { return (None, false) };
        let entries = m.source.items(&self.games, &self.topbar);
        let pointer_moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let font = FontId::proportional(14.0);
        let mut picked = None;

        let area = egui::Area::new(egui::Id::new("menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(m.pos)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let galleys: Vec<_> = entries
                        .iter()
                        .map(|(title, _)| ui.painter().layout_no_wrap(title.to_string(), font.clone(), Color32::PLACEHOLDER))
                        .collect();
                    let w = galleys.iter().map(|g| g.size().x).fold(160.0, f32::max) + 28.0;

                    for (k, (&(_, usable), galley)) in entries.iter().zip(galleys).enumerate() {
                        let sense = if usable { egui::Sense::click() } else { egui::Sense::hover() };
                        let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), sense);

                        // hovering selects, but only when the mouse actually moves, so it
                        // doesn't fight with the arrow keys
                        if usable && resp.hovered() && pointer_moved {
                            m.selected = k;
                        }
                        let on = k == m.selected;
                        if on {
                            ui.painter().rect_filled(rect, egui::CornerRadius::same(6), ACCENT.gamma_multiply(0.35));
                        }
                        let color = match (usable, on) {
                            (false, _) => Color32::from_gray(90),
                            (true, true) => Color32::WHITE,
                            (true, false) => Color32::from_gray(210),
                        };
                        let pos = egui::pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0);
                        ui.painter().galley(pos, galley, color);

                        if usable && resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            picked = Some((m.source, k));
                        }
                    }
                });
            });

        let rect = area.response.rect;
        let outside = menu_open
            && ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        (picked, outside)
    }

    /// Crossfades to `next` whenever it differs from the current background.
    fn paint_background(&mut self, ctx: &egui::Context, next: Option<&PathBuf>) {
        let now = ctx.input(|i| i.time);
        if self.bg.as_ref() != next {
            self.prev_bg = self.bg.take();
            self.bg = next.cloned();
            self.bg_since = now;
        }

        let t = ((now - self.bg_since) as f32 / FADE).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t); // smoothstep
        let painter = ctx.layer_painter(egui::LayerId::background());
        let screen = ctx.screen_rect();
        painter.rect_filled(screen, 0.0, BG);
        if t < 1.0 {
            // keep the old one fully visible under the new one, unless there's no new one
            let old = if self.bg.is_some() { 1.0 } else { 1.0 - t };
            if let Some(p) = &self.prev_bg {
                paint_cover(ctx, &painter, screen, p, old);
            }
            ctx.request_repaint();
        }
        if let Some(p) = &self.bg {
            paint_cover(ctx, &painter, screen, p, t);
        }
    }
}

/// Draws the image at `path` scaled to cover `screen` (cropping the overflow),
/// dimmed and at the given opacity. Draws nothing until the texture is loaded.
fn paint_cover(ctx: &egui::Context, painter: &egui::Painter, screen: egui::Rect, path: &Path, alpha: f32) {
    let uri = format!("file://{}", path.display());
    let Ok(egui::load::TexturePoll::Ready { texture }) =
        ctx.try_load_texture(&uri, egui::TextureOptions::LINEAR, egui::SizeHint::default())
    else {
        return;
    };
    let scale = (screen.width() / texture.size.x).max(screen.height() / texture.size.y);
    let rect = egui::Rect::from_center_size(screen.center(), texture.size * scale);
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(texture.id, rect, uv, Color32::from_gray(BG_DIM).gamma_multiply(alpha));
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // with_maximized alone is ignored by some X11 window managers (xfwm4),
        // because winit asks before the window is shown. eframe shows it after
        // the first frame, so asking again from here works there too.
        if std::mem::take(&mut self.maximize) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if std::mem::take(&mut self.fullscreen) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }

        while let Ok(update) = self.rx.try_recv() {
            match update {
                api_steamgriddb::Update::Cover(i, path) => self.games[i].cover = Some(path),
                api_steamgriddb::Update::Background(i, path) => self.games[i].background = Some(path),
            }
        }
        while let Ok((i, meta)) = self.meta.try_recv() {
            self.games[i].set_meta(meta);
        }

        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        self.controls.pad.update(now, focused);
        if let Some(t) = self.controls.pad.next_repeat() {
            ctx.request_repaint_after_secs((t - now) as f32);
        }

        // An open menu takes the keys: up/down move, launch runs, back closes.
        let menu_open = self.menu.is_some();
        let mut chosen = None;
        let mut close_menu = false;
        if let Some(m) = &mut self.menu {
            let count = m.source.items(&self.games, &self.topbar).len();
            let input = &mut self.controls;
            if input.take(ctx, Action::Up) {
                m.selected = (m.selected + count - 1) % count;
            }
            if input.take(ctx, Action::Down) {
                m.selected = (m.selected + 1) % count;
            }
            if input.take(ctx, Action::Launch) {
                chosen = Some((m.source, m.selected));
            }
            close_menu = input.take(ctx, Action::Back);
        }
        // a click that closed the menu is over once the button comes up
        let swallow = self.swallow_click;
        if ctx.input(|i| i.pointer.any_released()) {
            self.swallow_click = false;
        }

        // The top bar, once it has the focus: left/right move along it, launch uses the
        // widget, down and back go back to the grid. The search box is skipped, since
        // there's no typing into it with a pad.
        let mut use_widget = None;
        // a narrower window can cut off the widget that has the focus; then it's back on the grid
        self.focus = self.focus.filter(|id| self.bar.contains(id));
        if let Some(id) = self.focus.filter(|_| !menu_open) {
            let bar = &self.bar;
            let input = &mut self.controls;
            let at = bar.iter().position(|&f| f == id).expect("checked above");
            if input.take(ctx, Action::Left) {
                self.focus = Some(bar[at.saturating_sub(1)]);
            }
            if input.take(ctx, Action::Right) {
                self.focus = Some(bar[(at + 1).min(bar.len() - 1)]);
            }
            if input.take(ctx, Action::Launch) {
                use_widget = self.focus;
            }
            // used up so the search box doesn't act on them, but nothing to do here
            for action in [Action::Up, Action::PageUp, Action::PageDown] {
                input.take(ctx, action);
            }
            if input.take(ctx, Action::Down) | input.take(ctx, Action::Back) {
                self.focus = None;
            }
        }

        // back closes the menu or leaves the top bar (both above), else clears the search, else quits
        if !menu_open && self.controls.take(ctx, Action::Back) {
            if self.query.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.query.clear();
            }
        }

        let mut moved = false;
        if self.query != self.prev_query {
            self.prev_query = self.query.clone();
            self.selected = 0;
            self.focus = None; // typing is for the grid
            moved = true;
        }
        // a new order starts from its first game, like a new search
        if self.topbar.sort() != self.prev_sort {
            self.prev_sort = self.topbar.sort();
            self.selected = 0;
            moved = true;
        }

        if self.controls.take(ctx, Action::Reload) {
            let current = self.filtered().get(self.selected).map(|&i| self.games[i].name.clone());
            self.reload(ctx);
            let shown = self.filtered();
            self.selected = shown
                .iter()
                .position(|&i| Some(&self.games[i].name) == current.as_ref())
                .unwrap_or(0);
            moved = true;
        }

        let shown = self.filtered();
        // the order can also change by itself, when IGDB data comes in for a year sort or a
        // g: search; the selection stays on its game then
        if !moved {
            let at = self.selected_game.and_then(|g| shown.iter().position(|&i| i == g));
            if let Some(at) = at.filter(|&at| at != self.selected) {
                self.selected = at;
                moved = true;
            }
        }
        self.selected = self.selected.min(shown.len().saturating_sub(1));

        let next_bg = shown.get(self.selected).and_then(|&i| self.games[i].background.clone());
        self.paint_background(ctx, next_bg.as_ref());

        let gray = Color32::from_gray(140);
        let mut to_launch = None;
        let mut hovered = None;
        let mut open_menu: Option<(usize, egui::Pos2)> = None;
        let mut selected_rect = None;
        // opens the selected game's menu from the keyboard or pad (Shift+F10 / Menu by default)
        let in_grid = !menu_open && self.focus.is_none();
        let open_with_key = in_grid && !shown.is_empty() && self.controls.take(ctx, Action::Menu);

        // Moving and launching are taken before the top bar is drawn, so the search
        // box doesn't also move its cursor or lose focus on Enter. With no games shown
        // they're left to the search box, except up when it can go into the top bar.
        let steps = [Action::Left, Action::Right, Action::Up, Action::Down, Action::PageUp, Action::PageDown];
        let mut pressed = [false; 6];
        let mut up = None;
        if in_grid && (!shown.is_empty() || !self.bar.is_empty()) {
            up = self.controls.take_press(ctx, Action::Up);
        }
        if in_grid && !shown.is_empty() {
            for (p, action) in pressed.iter_mut().zip(steps) {
                *p = match action {
                    Action::Up => up.is_some(),
                    _ => self.controls.take(ctx, action),
                };
            }
            if self.controls.take(ctx, Action::Launch) {
                to_launch = shown.get(self.selected).copied();
            }
        }

        // what the status bar and the top bar's labels show, for the selected game
        let toggles = self.topbar.toggles();
        let (status_left, status_right, zones) = {
            let game = shown.get(self.selected).map(|&i| &self.games[i]);
            let played = game.zip(self.playtime.as_ref()).and_then(|(g, pt)| pt.get(&g.name));
            let vars = status::vars(game.map(|g| (g, played)), shown.len(), self.games.len());
            let env = status::env(game, &toggles);
            let render = |parts: &[Part]| -> Vec<(String, bool)> {
                parts.iter().filter_map(|p| Some((p.render(&vars, &env)?, p.bold))).collect()
            };
            let left = match game {
                Some(_) => render(&self.status.left),
                None => vec![("No games".to_string(), true)],
            };
            // drawn from the right edge, so last part first
            let right: Vec<_> = render(&self.status.right).into_iter().rev().collect();
            let bar = self.topbar.zones();
            let zones: [Vec<Item>; 3] = std::array::from_fn(|z| items(z, bar[z], &vars, &env));
            (left, right, zones)
        };

        let mut tools = Vec::new();
        if !self.topbar.is_empty() {
            let mut hint = "Search games, t:tag or g:genre…   (arrows to move".to_string();
            for (action, what) in [(Action::Launch, "launch"), (Action::Reload, "reload")] {
                if let Some(key) = self.controls.keys.label(action) {
                    hint += &format!(", {key} to {what}");
                }
            }
            hint.push(')');
            let ghost = suggestion(&self.query, &self.games, self.topbar.autocomplete());
            let (query, focus) = (&mut self.query, self.focus);
            // no fill, the background shows through. No bottom margin, the grid's own spacing follows
            tools = egui::TopBottomPanel::top("topbar")
                .frame(egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 24, bottom: 20 }))
                .show_separator_line(false)
                .show(ctx, |ui| {
                    let drawn = draw_topbar(ui, zones, query, ghost.as_deref(), &hint, !menu_open, focus);
                    if let Some(search) = drawn.search.filter(|_| !menu_open) {
                        search.request_focus();
                    }
                    drawn.tools
                })
                .inner;
        }

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(egui::Margin::symmetric(20, 10)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0; // the separator does the spacing
                    status_labels(ui, &self.status.separator, &status_left);
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        status_labels(ui, &self.status.separator, &status_right);
                    });
                });
            });

        let top = if self.topbar.is_empty() { 24 } else { 0 };
        egui::CentralPanel::default()
            // no fill, the background shows through. No bottom margin, so the grid runs down to the status bar
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top, bottom: 0 }))
            .show(ctx, |ui| {
                if let Some(err) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, err);
                    ui.add_space(8.0);
                }

                let w = self.card_w;
                let cols = ((((ui.available_width() - 2.0 * PAD) + GAP) / (w + GAP)) as usize).max(1);

                // a fresh up on the top row goes into the top bar; a held one stops at the
                // top, so holding it to scroll up doesn't overshoot
                if up == Some(Press::Fresh) && (shown.is_empty() || self.selected < cols) {
                    if let Some(&first) = self.bar.first() {
                        self.focus = Some(first);
                        pressed[2] = false;
                        ctx.request_repaint(); // the top bar was drawn before it got the focus
                    }
                }
                // while the top bar has the focus, no card is highlighted
                let highlight = self.focus.is_none();

                if !shown.is_empty() {
                    let page = (cols * self.page_rows) as isize;
                    let cols = cols as isize;
                    let step = [-1, 1, -cols, cols, -page, page];
                    let s = self.selected as isize + pressed.iter().zip(step).filter(|(p, _)| **p).map(|(_, s)| s).sum::<isize>();
                    let s = s.clamp(0, shown.len() as isize - 1) as usize;
                    if s != self.selected {
                        self.selected = s;
                        moved = true;
                    }
                }

                if shown.is_empty() {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No games match your search").size(18.0).color(gray));
                    });
                }

                let selected = self.selected;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                    let row_h = w * 1.5 + LABEL_H + GAP;
                    self.page_rows = ((ui.clip_rect().height() / row_h) as usize).max(1);
                    ui.add_space(PAD);

                    // each row is centered on its own, so short rows line up under the search bar
                    for (r, chunk) in shown.chunks(cols).enumerate() {
                        let n_row = chunk.len() as f32;
                        let total = n_row * w + (n_row - 1.0) * GAP;
                        let margin = ((ui.available_width() - total) / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.add_space(margin);
                            for (c, &i) in chunk.iter().enumerate() {
                                if c > 0 {
                                    ui.add_space(GAP);
                                }
                                let n = r * cols + c;
                                let resp = card(ui, &self.games[i], w, highlight && n == selected, moved);
                                if n == selected {
                                    selected_rect = Some(resp.rect);
                                }
                                // a click while a menu is open, or one that just closed it, is not a launch
                                if resp.clicked() && !menu_open && !swallow {
                                    to_launch = Some(i);
                                }
                                if resp.secondary_clicked() {
                                    open_menu = Some((i, resp.interact_pointer_pos().unwrap_or(resp.rect.center())));
                                    hovered = Some(n);
                                }
                                // hovering selects, but only when the mouse actually moves
                                if resp.hovered()
                                    && !menu_open
                                    && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO)
                                {
                                    hovered = Some(n);
                                }
                            }
                        });
                    }
                    ui.add_space(PAD);
                });
            });

        if let Some(n) = hovered {
            self.selected = n;
            self.focus = None;
        }
        if let Some(i) = to_launch {
            let g = &self.games[i];
            match launch(g, &toggles) {
                Ok(pgid) => {
                    *self.launched.borrow_mut() = Some(Launched { name: g.name.clone(), pgid, playtime: g.playtime });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                // stay open, so the game can be fixed in the config and retried after F5
                Err(err) => {
                    let msg = format!("Couldn't start {}: {err}", g.name);
                    self.fail(msg);
                }
            }
        }

        // The menu is drawn last, on top of everything.
        let (picked, outside) = self.show_menu(ctx, menu_open);
        if outside {
            close_menu = true;
            // if the button is still down, its release must not launch the card under it
            self.swallow_click = ctx.input(|i| i.pointer.primary_down());
        }
        if let Some((source, k)) = picked.or(chosen) {
            self.menu = None;
            self.pick(ctx, source, k);
        } else if close_menu {
            self.menu = None;
        }

        self.bar = tools.iter().map(|&(id, _)| id).collect();
        self.bar.sort(); // the right zone is drawn from the edge in, and ids sort left to right

        // a top bar button runs its tool, a toggle flips, a menu or sort opens under itself
        let clicked = tools.iter().find(|(_, r)| r.clicked() && !menu_open && !swallow).map(|&(id, _)| id);
        if let Some(id) = clicked.or(use_widget) {
            let dropdown = |source, selected| {
                // a menu cut off by a narrow window still opens, at the top
                let pos = tools
                    .iter()
                    .find(|(t, _)| *t == id)
                    .map_or(ctx.screen_rect().center_top(), |(_, r)| r.rect.left_bottom() + vec2(0.0, 6.0));
                Some(Menu { source, pos, selected })
            };
            match self.topbar.widget(id) {
                Widget::Button(_) => self.run_entry(ctx, Source::Topbar(id), 0),
                Widget::Toggle(t) => {
                    let var = matches!(t.target, Target::Var(_));
                    self.topbar.flip(id);
                    if var {
                        if let Err(e) = self.rebuild() {
                            self.topbar.flip(id);
                            self.fail(format!("Couldn't switch the toggle: {e}"));
                        }
                    }
                }
                Widget::Menu { .. } => self.menu = dropdown(Source::Topbar(id), 0),
                // it opens on the order it has now
                Widget::Sort(sort) => self.menu = dropdown(Source::Sort(id), sort.current),
                Widget::Label(_) | Widget::Search { .. } => unreachable!("only buttons, menus, toggles and sorts are tools"),
            }
            // drawn before this: a toggle and the labels using its variable, or a dropdown
            ctx.request_repaint();
        }

        if open_with_key && open_menu.is_none() {
            if let (Some(&i), Some(rect)) = (shown.get(self.selected), selected_rect) {
                open_menu = Some((i, rect.center()));
            }
        }
        if let Some((i, pos)) = open_menu {
            self.open_menu(ctx, i, pos);
        }
        self.selected_game = shown.get(self.selected).copied();
    }
}

/// Puts `shown` (indices into `games`, in name order) in `by` order. `playtime` is None
/// when it's off; then the playtime orders are by name.
fn sort(shown: &mut [usize], games: &[Game], playtime: Option<&playtime::Playtime>, by: SortBy) {
    let played = |i: usize| playtime.and_then(|pt| pt.get(&games[i].name)).copied().unwrap_or_default();
    // stable, so ties stay in name order; Reverse puts the biggest first and the missing last
    match by {
        SortBy::Name => {}
        SortBy::LastPlayed => shown.sort_by_key(|&i| Reverse(played(i).last_played)),
        SortBy::Playtime => shown.sort_by_key(|&i| Reverse(played(i).seconds)),
        SortBy::Year => shown.sort_by_key(|&i| Reverse(games[i].meta.as_ref().and_then(|m| m.year))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_put_the_biggest_first_and_the_missing_last_in_name_order() {
        let cfg = "[[game]]\nname = \"A\"\nexec = \"a\"\n[[game]]\nname = \"B\"\nexec = \"b\"\n\
                   [[game]]\nname = \"C\"\nexec = \"c\"\n[[game]]\nname = \"D\"\nexec = \"d\"\n";
        let mut games = config::parse_games(cfg).unwrap();
        for (g, year) in games.iter_mut().zip([None, Some(1999), None, Some(2004)]) {
            g.set_meta(api_igdb::Meta { year, ..Default::default() });
        }
        let entry = |seconds, last_played| playtime::Entry { seconds, last_played };
        let pt = playtime::Playtime::from([("B".into(), entry(10, 300)), ("C".into(), entry(50, 100))]);
        let sorted = |by, pt: Option<&playtime::Playtime>| {
            let mut shown = vec![0, 1, 2, 3];
            sort(&mut shown, &games, pt, by);
            shown.iter().map(|&i| games[i].name.as_str()).collect::<String>()
        };
        assert_eq!(sorted(SortBy::Name, Some(&pt)), "ABCD");
        assert_eq!(sorted(SortBy::LastPlayed, Some(&pt)), "BCAD");
        assert_eq!(sorted(SortBy::Playtime, Some(&pt)), "CBAD");
        assert_eq!(sorted(SortBy::Playtime, None), "ABCD", "playtime is off");
        assert_eq!(sorted(SortBy::Year, None), "DBAC");
    }

    #[test]
    fn search_splits_into_name_tags_and_genres() {
        let q = Query::parse("  T:Coop quake g:Shoot  arena t: g:");
        let strings = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(q, Query { name: "quake arena".into(), tags: strings(&["coop"]), genres: strings(&["shoot"]) });
    }

    #[test]
    fn genres_match_anywhere_in_a_genre() {
        let genres = ["Real Time Strategy (RTS)".to_string(), "Simulator".to_string()];
        let m = |q: &str| Query::parse(q).matches("Command & Conquer 3", &[], &genres);
        assert!(m("g:Strategy") && m("g:rts") && m("g:sim") && m("g:strat g:sim"));
        assert!(m("g:") && m("command g:rts"), "a bare g: doesn't filter");
        assert!(!m("g:shooter") && !m("g:rts g:shooter") && !m("quake g:rts"));
        assert!(!Query::parse("g:rts").matches("Quake", &[], &[]), "no genres known yet");
    }

    #[test]
    fn autocomplete_suggests_from_the_games_the_rest_of_the_search_shows() {
        let cfg = "[[game]]\nname = \"Quake III Arena\"\nexec = \"a\"\ntags = [\"fps\", \"lan\"]\n\
                   [[game]]\nname = \"Quake\"\nexec = \"b\"\ntags = [\"fps\", \"coop\"]\n\
                   [[game]]\nname = \"Command & Conquer 3\"\nexec = \"c\"\ntags = [\"couch\"]\n";
        let mut games = config::parse_games(cfg).unwrap();
        let genres = ["Real Time Strategy (RTS)".to_string(), "Strategy".to_string()];
        let cnc = games.iter_mut().find(|g| g.name.starts_with("Command")).unwrap();
        cnc.set_meta(api_igdb::Meta { genres: genres.into(), ..Default::default() });
        let all = [Complete::Names, Complete::Tags, Complete::Genres];
        let s = |q: &str| suggestion(q, &games, &all);

        assert_eq!(s("qu").as_deref(), Some("ake"), "the first in alphabetical order");
        assert_eq!(s("QUAKE").as_deref(), Some(" III Arena"), "longer than what's typed, any case");
        assert_eq!(s("t:coop quake i").as_deref(), None, "Quake III isn't tagged coop");
        assert_eq!(s("t:co").as_deref(), Some("op"));
        assert_eq!(s("t:").as_deref(), Some("coop"), "a bare t: suggests too");
        assert_eq!(s("t:lan t:").as_deref(), Some("fps"), "only the tags of the games shown");
        assert_eq!(s("g:r").as_deref(), Some("eal"), "genres a word at a time");
        assert_eq!(s("g:rt").as_deref(), Some("S"), "without the brackets, in its own case");
        assert_eq!(s("g:stra").as_deref(), Some("tegy"));
        assert_eq!(s("t:fps g:").as_deref(), None, "no fps game has genres");
        assert_eq!(s("qu ").as_deref(), None, "nothing while starting a new word");
        assert_eq!(s("").as_deref(), None);
        assert_eq!(s("zork").as_deref(), None);

        assert_eq!(suggestion("qu", &games, &[Complete::Tags]), None, "names are off");
        assert_eq!(suggestion("t:co", &games, &[Complete::Names]), None, "tags are off, and t:co isn't a name");
        assert_eq!(suggestion("qu", &games, &[]), None);
    }

    #[test]
    fn after_prefix_ignores_case_but_keeps_it() {
        assert_eq!(after_prefix("Quake", "qu"), Some("ake"));
        assert_eq!(after_prefix("Quake", ""), Some("Quake"));
        assert_eq!(after_prefix("Quake", "doom"), None);
        // 'İ' lowercases to two characters, so the prefix ends at a different byte in the title
        assert_eq!(after_prefix("İstanbul", "i\u{307}st"), Some("anbul"));
    }
}
