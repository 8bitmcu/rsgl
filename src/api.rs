use crate::{blur, config};
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

const BASE: &str = "https://www.steamgriddb.com/api/v2";
const MIMES: &str = "image/png,image/jpeg,image/webp";

type Res<T> = Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
struct Response<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

/// Like `Response`, for endpoints that return a single object.
#[derive(Deserialize)]
struct Single<T> {
    data: Option<T>,
}

#[derive(Deserialize)]
struct GameHit {
    id: u64,
    name: String,
}

/// A grid or hero image as returned by SteamGridDB.
#[derive(Deserialize)]
struct Asset {
    url: String,
    mime: String,
}

pub struct Client {
    key: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(key: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .user_agent("rsgl")
            .build();
        Self { key, agent }
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Res<T> {
        Ok(self
            .agent
            .get(&format!("{BASE}{path}"))
            .set("Authorization", &format!("Bearer {}", self.key))
            .call()?
            .into_json()?)
    }

    /// Resolve a game name to a SteamGridDB game id.
    /// Prefers a case-insensitive exact match, otherwise the top result.
    fn search(&self, name: &str) -> Res<Option<u64>> {
        let path = format!("/search/autocomplete/{}", urlencoding::encode(name));
        let hits: Response<GameHit> = self.get(&path)?;
        let exact = hits.data.iter().find(|h| h.name.eq_ignore_ascii_case(name));
        Ok(exact.or(hits.data.first()).map(|h| h.id))
    }

    /// SteamGridDB game id for a configured id, without searching by name.
    fn resolve(&self, id: config::ArtId) -> Res<Option<u64>> {
        match id {
            config::ArtId::SteamGridDb(id) => Ok(Some(id)),
            config::ArtId::Steam(app) => {
                let hit: Single<GameHit> = self.get(&format!("/games/steam/{app}"))?;
                Ok(hit.data.map(|h| h.id))
            }
        }
    }

    /// Best-scored `kind` ("grids" or "heroes") asset for a game, trying each
    /// dimension filter in order (`None` = any size).
    fn best(&self, kind: &str, id: u64, dims: &[Option<&str>]) -> Res<Option<Asset>> {
        for d in dims {
            let mut path =
                format!("/{kind}/game/{id}?mimes={MIMES}&types=static&nsfw=false&humor=false");
            if let Some(d) = d {
                path.push_str(&format!("&dimensions={d}"));
            }
            let found: Response<Asset> = self.get(&path)?;
            if let Some(a) = found.data.into_iter().next() {
                return Ok(Some(a)); // API returns results ordered by score
            }
        }
        Ok(None)
    }

    /// Best-scored portrait grid, falling back to any shape.
    fn fetch_cover(&self, id: u64, name: &str) -> Res<Option<PathBuf>> {
        let dims = [Some("600x900,342x482,660x930"), None];
        match self.best("grids", id, &dims)? {
            Some(a) => self.download(&a, &config::covers_dir(), name),
            None => Ok(None),
        }
    }

    fn fetch_hero(&self, id: u64, name: &str) -> Res<Option<PathBuf>> {
        match self.best("heroes", id, &[None])? {
            Some(a) => self.download(&a, &config::heroes_dir(), name),
            None => Ok(None),
        }
    }

    /// Downloads an asset to `dir/<slug>.<ext>`.
    fn download(&self, asset: &Asset, dir: &Path, name: &str) -> Res<Option<PathBuf>> {
        let ext = match asset.mime.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => return Ok(None),
        };

        fs::create_dir_all(dir)?;
        let dest = dir.join(format!("{}.{ext}", config::slug(name)));
        let tmp = dest.with_extension("part");

        let mut reader = self.agent.get(&asset.url).call()?.into_reader();
        let mut file = fs::File::create(&tmp)?;
        io::copy(&mut reader, &mut file)?;
        fs::rename(&tmp, &dest)?; // rename so a half-download is never picked up
        // a replaced image may have had another extension, which find_image could prefer
        for other in ["png", "jpg", "webp"].into_iter().filter(|&e| e != ext) {
            let _ = fs::remove_file(dest.with_extension(other));
        }
        Ok(Some(dest))
    }
}

pub enum Update {
    Cover(usize, PathBuf),
    Background(usize, PathBuf),
}

/// What the fetcher needs to know about one game.
pub struct Job {
    pub index: usize,
    pub name: String,
    /// Existing cover, if any.
    pub cover: Option<PathBuf>,
    /// Configured id to look the game up by instead of its name.
    pub art_id: Option<config::ArtId>,
}

/// Remembers which configured id the cached cover and hero were fetched with.
fn art_id_marker(name: &str) -> PathBuf {
    config::covers_dir().join(format!("{}.id", config::slug(name)))
}

/// Blurs the hero (or the cover if there's no hero) into the backgrounds cache.
fn background(
    name: &str,
    hero: Option<&PathBuf>,
    cover: Option<&PathBuf>,
    sigma: f32,
) -> Option<PathBuf> {
    let (src, kind) = match (hero, cover) {
        (Some(h), _) => (h, "hero"),
        (None, Some(c)) => (c, "cover"),
        (None, None) => return None,
    };
    let file = |kind| config::backgrounds_dir().join(format!("{}.{kind}.jpg", config::slug(name)));
    let dest = blur::blurred(src, &file(kind), sigma)
        .map_err(|e| eprintln!("blur failed for {name:?}: {e}"))
        .ok()?;
    if kind == "hero" {
        let _ = std::fs::remove_file(file("cover")); // the cover fallback isn't needed anymore
    }
    Some(dest)
}

/// Works through `jobs` on a background thread. First every game gets a
/// background from whatever is already on disk, then missing covers and
/// heroes are downloaded (if there's an API key) and backgrounds upgraded.
/// `on_done` is called after every update so the UI can repaint.
pub fn spawn_fetcher(
    jobs: Vec<Job>,
    key: Option<String>,
    sigma: f32,
    on_done: impl Fn() + Send + 'static,
) -> mpsc::Receiver<Update> {
    let (tx, rx) = mpsc::channel();
    let client = key.map(Client::new);
    if client.is_none() && jobs.iter().any(|j| j.cover.is_none()) {
        eprintln!("no SteamGridDB key set; skipping cover and hero fetch");
    }

    thread::spawn(move || {
        let send = |u| tx.send(u).map(|_| on_done()).is_ok();
        blur::reset_if_changed(&config::backgrounds_dir(), sigma);
        let mut jobs: Vec<_> = jobs
            .into_iter()
            .map(|j| {
                let hero = config::find_hero(&j.name);
                (j, hero)
            })
            .collect();

        // pass 1: local files only, so backgrounds show up right away
        for (j, hero) in &jobs {
            if let Some(bg) = background(&j.name, hero.as_ref(), j.cover.as_ref(), sigma) {
                if !send(Update::Background(j.index, bg)) {
                    return; // UI is gone
                }
            }
        }

        // pass 2: fill in whatever is missing from SteamGridDB
        let Some(client) = client else { return };
        for (j, hero) in &mut jobs {
            let name = &j.name;
            // cached art from a name search (or an older id) may be the wrong game,
            // so fetch it again once with the configured id. Covers set with
            // `cover = ...` outside the cache are left alone.
            let marker = art_id_marker(name);
            if let Some(art) = j.art_id {
                if fs::read_to_string(&marker).ok() != Some(art.to_string()) {
                    if j.cover.is_some() && j.cover == config::find_cover(name) {
                        j.cover = None;
                    }
                    *hero = None;
                }
            }
            if j.cover.is_some() && hero.is_some() {
                continue;
            }
            let found = match j.art_id {
                Some(art) => client.resolve(art),
                None => client.search(name),
            };
            let id = match found {
                Ok(Some(id)) => id,
                Ok(None) => {
                    eprintln!("no SteamGridDB match for {name:?}");
                    continue;
                }
                Err(e) => {
                    eprintln!("SteamGridDB lookup failed for {name:?}: {e}");
                    continue;
                }
            };
            let mut failed = false;

            if j.cover.is_none() {
                match client.fetch_cover(id, name) {
                    Ok(Some(path)) => {
                        j.cover = Some(path.clone());
                        if !send(Update::Cover(j.index, path)) {
                            return;
                        }
                    }
                    Ok(None) => eprintln!("no cover found for {name:?}"),
                    Err(e) => {
                        eprintln!("cover fetch failed for {name:?}: {e}");
                        failed = true;
                    }
                }
            }
            if hero.is_none() {
                match client.fetch_hero(id, name) {
                    Ok(Some(path)) => *hero = Some(path),
                    Ok(None) => eprintln!("no hero found for {name:?}"),
                    Err(e) => {
                        eprintln!("hero fetch failed for {name:?}: {e}");
                        failed = true;
                    }
                }
            }
            if let Some(art) = j.art_id.filter(|_| !failed) {
                let _ = fs::create_dir_all(config::covers_dir())
                    .and_then(|_| fs::write(&marker, art.to_string()));
            }

            // cheap if nothing changed, blurred() reuses the cached file
            if let Some(bg) = background(name, hero.as_ref(), j.cover.as_ref(), sigma) {
                if !send(Update::Background(j.index, bg)) {
                    return;
                }
            }
        }
    });

    rx
}
