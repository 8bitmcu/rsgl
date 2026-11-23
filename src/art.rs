//! Cover and background textures. egui's own image loaders keep the file's bytes, a
//! full-size decoded copy and the texture of every image they ever loaded, and decode
//! on the UI thread; a 4K hero is ~40 MB that way. Here images are decoded on worker
//! threads, shrunk to the size they're drawn at, and only the texture is kept, until
//! the cache is over its budget and the image hasn't been drawn for a frame.

use eframe::egui;
use image::imageops::FilterType;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
};

/// Bytes of textures kept around after they're no longer drawn. What's on screen is
/// never dropped, so this only limits what scrolling back finds already decoded.
const BUDGET: usize = 64 << 20;

/// Decoding threads. Covers are small once shrunk, but a 4K hero takes a while.
const WORKERS: usize = 3;

/// A finished decode: the image, or why it failed.
type Done = (PathBuf, Result<egui::ColorImage, String>);

pub struct Art {
    jobs: mpsc::Sender<(PathBuf, [u32; 2])>,
    done: mpsc::Receiver<Done>,
    entries: HashMap<PathBuf, Entry>,
    frame: u64,
}

struct Entry {
    /// Pixel size asked for: the image covers it, unless it's smaller than that itself.
    want: [u32; 2],
    texture: Option<egui::TextureHandle>, // None while the first decode runs, or if it failed
    pending: bool,
    last_used: u64,
}

impl Art {
    pub fn new(ctx: &egui::Context) -> Self {
        let (jobs, rx) = mpsc::channel::<(PathBuf, [u32; 2])>();
        let (tx, done) = mpsc::channel();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..WORKERS {
            let (rx, tx, ctx) = (rx.clone(), tx.clone(), ctx.clone());
            thread::spawn(move || loop {
                // the lock is only held while waiting, not while decoding
                let Ok((path, want)) = rx.lock().unwrap().recv() else {
                    return; // the cache is gone
                };
                let image = decode(&path, want);
                if tx.send((path, image)).is_err() {
                    return;
                }
                ctx.request_repaint();
            });
        }
        Art { jobs, done, entries: HashMap::new(), frame: 0 }
    }

    /// Call once at the start of every frame: uploads what the workers finished and
    /// drops textures that weren't drawn last frame while over the budget.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        // a bigger size asked for while one was decoding is sent by the next `get`
        for (path, image) in self.done.try_iter() {
            let e = self.entries.get_mut(&path).expect("entries aren't evicted while decoding");
            e.pending = false;
            match image {
                Ok(image) => {
                    let name = path.display().to_string();
                    e.texture = Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
                }
                // not retried: the cover placeholder or no background shows instead
                Err(err) => eprintln!("can't load {}: {err}", path.display()),
            }
        }
        evict(&mut self.entries, self.frame, BUDGET);
    }

    /// The texture for `path`, at least `size` pixels if the image is that big. None
    /// until it's decoded, or if it can't be. A smaller one is returned while a bigger
    /// one is decoding.
    pub fn get(&mut self, path: &Path, size: egui::Vec2) -> Option<&egui::TextureHandle> {
        let want = [size.x.ceil().max(1.0) as u32, size.y.ceil().max(1.0) as u32];
        let frame = self.frame;
        let e = self.entries.entry(path.to_path_buf()).or_insert_with(|| Entry {
            want: [0, 0],
            texture: None,
            pending: false,
            last_used: frame,
        });
        e.last_used = frame;
        if !e.pending && (want[0] > e.want[0] || want[1] > e.want[1]) {
            e.want = [want[0].max(e.want[0]), want[1].max(e.want[1])];
            e.pending = true;
            self.jobs.send((path.to_path_buf(), e.want)).expect("art workers stopped");
        }
        e.texture.as_ref()
    }
}

/// Drops the least recently drawn textures until the rest fit in `budget` bytes.
/// Anything drawn this frame or the one before stays, and so do decodes in flight.
fn evict(entries: &mut HashMap<PathBuf, Entry>, frame: u64, budget: usize) {
    let bytes = |e: &Entry| e.texture.as_ref().map_or(0, |t| t.size()[0] * t.size()[1] * 4);
    let mut total: usize = entries.values().map(bytes).sum();
    if total <= budget {
        return;
    }
    let mut old: Vec<_> = entries
        .iter()
        .filter(|(_, e)| !e.pending && e.last_used + 1 < frame)
        .map(|(p, e)| (e.last_used, p.clone()))
        .collect();
    old.sort();
    for (_, path) in old {
        if total <= budget {
            break;
        }
        total -= bytes(&entries.remove(&path).unwrap());
    }
}

fn decode(path: &Path, want: [u32; 2]) -> Result<egui::ColorImage, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let [w, h] = shrunk([img.width(), img.height()], want);
    let img = if [w, h] == [img.width(), img.height()] {
        img
    } else {
        img.resize_exact(w, h, FilterType::Triangle)
    };
    let rgba = img.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw()))
}

/// The smallest size, keeping the aspect ratio, that still covers `want`. Never larger
/// than the image itself.
fn shrunk(size: [u32; 2], want: [u32; 2]) -> [u32; 2] {
    let scale = (want[0] as f64 / size[0] as f64).max(want[1] as f64 / size[1] as f64);
    if scale >= 1.0 {
        return size;
    }
    let side = |s: u32| ((s as f64 * scale).round() as u32).max(1);
    [side(size[0]), side(size[1])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn shrinks_to_cover_and_never_grows() {
        assert_eq!(shrunk([600, 900], [180, 270]), [180, 270]);
        // a wide hero on a 16:9 screen: the height decides
        assert_eq!(shrunk([3840, 1240], [1920, 1080]), [3345, 1080]);
        assert_eq!(shrunk([640, 206], [1920, 1080]), [640, 206]);
        assert_eq!(shrunk([1000, 1], [10, 10]), [1000, 1]);
    }

    fn wait_for(art: &mut Art, ctx: &egui::Context, path: &Path, size: egui::Vec2) -> [usize; 2] {
        let start = Instant::now();
        loop {
            art.begin_frame(ctx);
            if let Some(t) = art.get(path, size) {
                return t.size();
            }
            assert!(start.elapsed() < Duration::from_secs(10), "never decoded");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn decodes_at_the_drawn_size_and_redoes_it_bigger() {
        let dir = std::env::temp_dir().join(format!("rsgl-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cover.png");
        image::RgbImage::new(600, 900).save(&path).unwrap();

        let ctx = egui::Context::default();
        let mut art = Art::new(&ctx);
        assert_eq!(wait_for(&mut art, &ctx, &path, egui::vec2(100.0, 150.0)), [100, 150]);
        // asking for less keeps the bigger one
        assert_eq!(art.get(&path, egui::vec2(50.0, 75.0)).unwrap().size(), [100, 150]);
        // asking for more decodes again, showing the small one meanwhile
        assert_eq!(art.get(&path, egui::vec2(200.0, 300.0)).unwrap().size(), [100, 150]);
        let start = Instant::now();
        while art.get(&path, egui::vec2(200.0, 300.0)).unwrap().size() != [200, 300] {
            assert!(start.elapsed() < Duration::from_secs(10), "never decoded again");
            thread::sleep(Duration::from_millis(5));
            art.begin_frame(&ctx);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn evicts_least_recently_drawn_but_not_whats_on_screen() {
        let ctx = egui::Context::default();
        let tex = |n| ctx.load_texture(n, egui::ColorImage::new([16, 16], egui::Color32::BLACK), Default::default());
        let entry = |last_used, pending| Entry { want: [16, 16], texture: Some(tex("t")), pending, last_used };
        let mut entries: HashMap<PathBuf, Entry> = [
            ("old".into(), entry(1, false)),
            ("older".into(), entry(0, false)),
            ("decoding".into(), entry(0, true)),
            ("drawn".into(), entry(9, false)),
        ]
        .into_iter()
        .collect();
        let one = 16 * 16 * 4;

        evict(&mut entries, 10, 4 * one);
        assert_eq!(entries.len(), 4, "under budget, nothing goes");
        evict(&mut entries, 10, 3 * one);
        assert!(!entries.contains_key(Path::new("older")));
        evict(&mut entries, 10, 0);
        let mut left: Vec<_> = entries.keys().map(|p| p.to_str().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["decoding", "drawn"]);
    }
}
