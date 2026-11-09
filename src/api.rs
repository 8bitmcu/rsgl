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

    /// One specific `kind` ("grids" or "heroes") asset by its SteamGridDB id.
    fn asset(&self, kind: &str, id: u64) -> Res<Option<Asset>> {
        let found: Single<Asset> = self.get(&format!("/{kind}/{id}"))?;
        Ok(found.data)
    }

    /// The pinned grid, or the game's best-scored portrait grid, falling back to any shape.
    fn fetch_cover(&self, pick: Pick, name: &str) -> Res<Option<PathBuf>> {
        let found = match pick {
            Pick::Asset(id) => self.asset("grids", id)?,
            Pick::Best(game) => self.best("grids", game, &[Some("600x900,342x482,660x930"), None])?,
        };
        match found {
            Some(a) => self.download(&a, &config::covers_dir(), name),
            None => Ok(None),
        }
    }

    fn fetch_hero(&self, pick: Pick, name: &str) -> Res<Option<PathBuf>> {
        let found = match pick {
            Pick::Asset(id) => self.asset("heroes", id)?,
            Pick::Best(game) => self.best("heroes", game, &[None])?,
        };
        match found {
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
            // only a pinned asset can get here, the searches ask for these three
            other => return Err(format!("{other} isn't supported, only png, jpeg and webp").into()),
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

/// Which SteamGridDB asset to download.
#[derive(Clone, Copy)]
enum Pick {
    /// This exact grid or hero, from `grid_id` or `hero_id`.
    Asset(u64),
    /// The best-scored one of this SteamGridDB game.
    Best(u64),
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
    /// Configured grid to use as the cover.
    pub grid_id: Option<u64>,
    /// Configured hero to use for the background.
    pub hero_id: Option<u64>,
}

impl Job {
    /// What the configured ids say the cover should come from, if they say anything.
    fn cover_source(&self) -> Option<String> {
        self.grid_id.map(|id| format!("grid:{id}")).or(self.art_id.map(|a| a.to_string()))
    }

    fn hero_source(&self) -> Option<String> {
        self.hero_id.map(|id| format!("hero:{id}")).or(self.art_id.map(|a| a.to_string()))
    }
}

/// Remembers which configured source the cached image in `dir` was fetched from.
fn source_marker(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.id", config::slug(name)))
}

/// True if the image `marker` belongs to was fetched from something other than `source`.
/// Without a configured source any cached image will do.
fn stale(marker: &Path, source: Option<&str>) -> bool {
    source.is_some_and(|s| fs::read_to_string(marker).ok().as_deref() != Some(s))
}

fn mark(marker: &Path, source: Option<&str>) {
    if let Some(s) = source {
        let _ = marker
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(marker, s));
    }
}

/// The game's SteamGridDB id, by its configured id or else by name. Failures are
/// printed and give `None`.
fn lookup(client: &Client, j: &Job) -> Option<u64> {
    let found = match j.art_id {
        Some(art) => client.resolve(art),
        None => client.search(&j.name),
    };
    match found {
        Ok(Some(id)) => Some(id),
        Ok(None) => {
            eprintln!("no SteamGridDB match for {:?}", j.name);
            None
        }
        Err(e) => {
            eprintln!("SteamGridDB lookup failed for {:?}: {e}", j.name);
            None
        }
    }
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
            // cached art from a name search (or an older id or asset) may not be what
            // the config asks for, so fetch it again once. Covers set with
            // `cover = ...` outside the cache are left alone.
            let (cover_src, hero_src) = (j.cover_source(), j.hero_source());
            let cover_marker = source_marker(&config::covers_dir(), name);
            let hero_marker = source_marker(&config::heroes_dir(), name);
            if stale(&cover_marker, cover_src.as_deref()) && j.cover.is_some() && j.cover == config::find_cover(name) {
                j.cover = None;
            }
            if stale(&hero_marker, hero_src.as_deref()) {
                *hero = None;
            }
            if j.cover.is_some() && hero.is_some() {
                continue;
            }

            // the game itself is only needed for art that isn't pinned to an asset
            let unpinned = (j.cover.is_none() && j.grid_id.is_none()) || (hero.is_none() && j.hero_id.is_none());
            let game = if unpinned { lookup(&client, j) } else { None };
            let pick = |pinned: Option<u64>| pinned.map(Pick::Asset).or(game.map(Pick::Best));

            if let Some(p) = pick(j.grid_id).filter(|_| j.cover.is_none()) {
                match client.fetch_cover(p, name) {
                    Ok(found) => {
                        mark(&cover_marker, cover_src.as_deref());
                        match found {
                            Some(path) => {
                                j.cover = Some(path.clone());
                                if !send(Update::Cover(j.index, path)) {
                                    return;
                                }
                            }
                            None => eprintln!("no cover found for {name:?}"),
                        }
                    }
                    Err(e) => eprintln!("cover fetch failed for {name:?}: {e}"),
                }
            }
            if let Some(p) = pick(j.hero_id).filter(|_| hero.is_none()) {
                match client.fetch_hero(p, name) {
                    Ok(found) => {
                        mark(&hero_marker, hero_src.as_deref());
                        match found {
                            Some(path) => *hero = Some(path),
                            None => eprintln!("no hero found for {name:?}"),
                        }
                    }
                    Err(e) => eprintln!("hero fetch failed for {name:?}: {e}"),
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn job(art_id: Option<config::ArtId>, grid_id: Option<u64>, hero_id: Option<u64>) -> Job {
        Job { index: 0, name: "A".into(), cover: None, art_id, grid_id, hero_id }
    }

    #[test]
    fn pinned_assets_win_over_the_game_id() {
        let art = Some(config::ArtId::Steam(367520));
        let j = job(art, Some(664940), None);
        assert_eq!(j.cover_source().as_deref(), Some("grid:664940"));
        assert_eq!(j.hero_source().as_deref(), Some("steam:367520"));

        let j = job(None, None, Some(15208));
        assert_eq!(j.cover_source(), None, "nothing configured for the cover");
        assert_eq!(j.hero_source().as_deref(), Some("hero:15208"));
    }

    #[test]
    fn stale_until_marked_with_the_same_source() {
        let marker = std::env::temp_dir().join(format!("rsgl-test-{}/a.id", std::process::id()));
        let _ = fs::remove_file(&marker);
        assert!(!stale(&marker, None), "no configured source keeps whatever is cached");
        assert!(stale(&marker, Some("grid:1")), "no marker yet");

        mark(&marker, Some("grid:1"));
        assert!(!stale(&marker, Some("grid:1")));
        assert!(stale(&marker, Some("grid:2")), "a changed id fetches again");

        let _ = fs::remove_dir_all(marker.parent().unwrap());
    }
}
