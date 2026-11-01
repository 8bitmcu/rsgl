use crate::{api, config::{ApiCfg, Game, UiCfg}, launch};
use eframe::egui;
use egui::{vec2, Align, Align2, Color32, FontId, Key, Modifiers, RichText, Stroke};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
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

/// Opens the window and runs the launcher until it closes.
pub fn run(games: Vec<Game>, cfg: UiCfg, api_cfg: ApiCfg, error: Option<String>) -> eframe::Result {
    let key = api_cfg.steamgriddb_key();
    let blur = cfg.blur.clamp(0.0, 50.0);
    let jobs: Vec<api::Job> = games
        .iter()
        .enumerate()
        .map(|(i, g)| api::Job {
            index: i,
            name: g.name.clone(),
            cover: g.cover.clone().filter(|p| p.exists()),
        })
        .collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([520.0, 400.0])
            .with_title("Games")
            .with_app_id("rsgl"),
        ..Default::default()
    };

    eframe::run_native(
        "Games",
        options,
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let rx = api::spawn_fetcher(jobs, key, blur, move || ctx.request_repaint());
            Ok(Box::new(App {
                games,
                error,
                query: String::new(),
                prev_query: String::new(),
                selected: 0,
                card_w: cfg.card_width.clamp(80.0, 400.0),
                rx,
                bg: None,
                prev_bg: None,
                bg_since: 0.0,
            }))
        }),
    )
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

    match g.cover.as_ref().filter(|c| c.exists()) {
        Some(path) => {
            egui::Image::new(format!("file://{}", path.display()))
                .corner_radius(radius)
                .paint_at(ui, cover);
        }
        None => {
            // colored placeholder derived from the name
            let hash = g.name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
            let hue = (hash % 360) as f32 / 360.0;
            let fill = Color32::from(egui::ecolor::Hsva::new(hue, 0.45, 0.35, 1.0));
            ui.painter().rect_filled(cover, radius, fill);
            let initial: String = g.name.chars().next().unwrap_or('?').to_uppercase().collect();
            ui.painter().text(
                cover.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::proportional(w * 0.4),
                Color32::from_white_alpha(190),
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
    let color = if selected { Color32::WHITE } else { Color32::from_gray(170) };
    ui.put(
        label_rect,
        egui::Label::new(RichText::new(&g.name).size(13.0).color(color))
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
    card_w: f32,
    rx: mpsc::Receiver<api::Update>,
    bg: Option<PathBuf>,      // background currently fading in / shown
    prev_bg: Option<PathBuf>, // background fading out
    bg_since: f64,            // when `bg` last changed
}

impl App {
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
        while let Ok(update) = self.rx.try_recv() {
            match update {
                api::Update::Cover(i, path) => self.games[i].cover = Some(path),
                api::Update::Background(i, path) => self.games[i].background = Some(path),
            }
        }

        // Esc clears the search first, then quits
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
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
            moved = true;
        }

        let q = self.query.to_lowercase();
        let shown: Vec<usize> = (0..self.games.len())
            .filter(|&i| self.games[i].name.to_lowercase().contains(&q))
            .collect();
        self.selected = self.selected.min(shown.len().saturating_sub(1));

        let next_bg = shown.get(self.selected).and_then(|&i| self.games[i].background.clone());
        self.paint_background(ctx, next_bg.as_ref());

        let gray = Color32::from_gray(140);
        let mut to_launch = None;
        let mut hovered = None;

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(egui::Margin::symmetric(20, 10)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let title = match shown.get(self.selected) {
                        Some(&i) => self.games[i].name.clone(),
                        None => "No games".to_string(),
                    };
                    ui.label(RichText::new(title).strong().size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} games", shown.len())).color(gray));
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(24))) // no fill, the background shows through
            .show(ctx, |ui| {
                if let Some(err) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, err);
                    ui.add_space(8.0);
                }

                let w = self.card_w;
                let cols = ((((ui.available_width() - 2.0 * PAD) + GAP) / (w + GAP)) as usize).max(1);

                // Arrow keys are consumed so the text box doesn't move its cursor
                if !shown.is_empty() {
                    let (l, r, u, d) = ctx.input_mut(|i| {
                        (
                            i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                            i.consume_key(Modifiers::NONE, Key::ArrowRight),
                            i.consume_key(Modifiers::NONE, Key::ArrowUp),
                            i.consume_key(Modifiers::NONE, Key::ArrowDown),
                        )
                    });
                    let mut s = self.selected as isize;
                    if l {
                        s -= 1;
                    }
                    if r {
                        s += 1;
                    }
                    if u {
                        s -= cols as isize;
                    }
                    if d {
                        s += cols as isize;
                    }
                    let s = s.clamp(0, shown.len() as isize - 1) as usize;
                    if s != self.selected {
                        self.selected = s;
                        moved = true;
                    }
                    if ctx.input(|i| i.key_pressed(Key::Enter)) {
                        to_launch = shown.get(self.selected).copied();
                    }
                }

                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search games…   (arrows to move, Enter to launch)")
                        .font(FontId::proportional(18.0))
                        .margin(egui::Margin::symmetric(14, 10))
                        .desired_width(f32::INFINITY),
                )
                .request_focus();
                ui.add_space(20.0);

                if shown.is_empty() {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No games match your search").size(18.0).color(gray));
                    });
                }

                let selected = self.selected;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                    let total = cols as f32 * w + (cols as f32 - 1.0) * GAP;
                    let margin = ((ui.available_width() - total) / 2.0).max(0.0);
                    ui.add_space(PAD);

                    for (r, chunk) in shown.chunks(cols).enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.add_space(margin);
                            for (c, &i) in chunk.iter().enumerate() {
                                if c > 0 {
                                    ui.add_space(GAP);
                                }
                                let n = r * cols + c;
                                let resp = card(ui, &self.games[i], w, n == selected, moved);
                                if resp.clicked() {
                                    to_launch = Some(i);
                                }
                                // hovering selects, but only when the mouse actually moves
                                if resp.hovered()
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
        }
        if let Some(i) = to_launch {
            launch(&self.games[i]);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
