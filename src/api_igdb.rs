//! Game metadata from IGDB: the raw name search printed by `--igdb`, and the
//! release year and genres behind `{year}` and `{genres}`.
use crate::{config, playtime::now};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fs,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const BASE: &str = "https://api.igdb.com/v4";
/// What a search asks for about each game.
const FIELDS: &str = "name, summary, first_release_date, genres.name, \
    involved_companies.company.name, involved_companies.developer, total_rating, url";
/// What a metadata lookup asks for, see `Hit`.
const META_FIELDS: &str = "id, name, first_release_date, genres.name";
/// A cached token this close to expiring is replaced instead of used.
const EXPIRY_MARGIN: u64 = 60 * 60;
/// IGDB allows 4 requests a second; spacing them out keeps a long run of lookups under it.
const REQUEST_GAP: Duration = Duration::from_millis(250);
/// How long to back off when IGDB answers 429 Too Many Requests anyway.
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(15);
/// 429s in a row before a request gives up, so a stuck limit can't hang forever.
const RATE_LIMIT_RETRIES: u32 = 4;

type Res<T> = Result<T, Box<dyn Error>>;

/// Twitch's answer to a client credentials request.
#[derive(Deserialize)]
struct Grant {
    access_token: String,
    /// Seconds, about 60 days.
    expires_in: u64,
}

/// An app access token, cached on disk until it expires so every request
/// doesn't need a new one.
#[derive(Serialize, Deserialize)]
struct Token {
    /// The Twitch app the token belongs to; a different `igdb_client_id` needs its own.
    client_id: String,
    access_token: String,
    /// Unix time.
    expires_at: u64,
}

impl Token {
    /// The cached token for `client_id`, unless it's missing, broken or about to expire.
    fn load(path: &Path, client_id: &str) -> Option<Self> {
        let t: Self = toml::from_str(&fs::read_to_string(path).ok()?).ok()?;
        (t.client_id == client_id && t.expires_at > now() + EXPIRY_MARGIN).then_some(t)
    }

    /// Owner-only, since the token is a credential.
    fn save(&self, path: &Path) -> Res<()> {
        Ok(config::write_file(path, &toml::to_string(self)?, true)?)
    }
}

/// What rsgl keeps about a game from IGDB.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Meta {
    /// IGDB's name for the game, behind a ROM's `{igdb_name}`. `None` if IGDB doesn't
    /// know the game, or it was cached before names were kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    /// The game's `igdb_id` when it was looked up, `None` if it was found by name.
    /// Cached data from a different id doesn't count, so changing it looks the game up again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb_id: Option<u64>,
    /// The `igdb_platform` the name search was narrowed to, see `igdb_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb_platform: Option<String>,
}

/// Metadata found so far, by game name (see `config::Game::name`).
/// A missing or broken file counts as empty; it's only a cache.
pub fn cached_meta() -> BTreeMap<String, Meta> {
    read_meta(&config::igdb_meta_file())
}

fn read_meta(path: &Path) -> BTreeMap<String, Meta> {
    fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

fn cache_meta(path: &Path, name: &str, meta: &Meta) -> Res<()> {
    let mut all = read_meta(path);
    all.insert(name.to_string(), meta.clone());
    Ok(config::write_file(path, &toml::to_string(&all)?, false)?)
}

/// One search result, as far as `Meta` goes.
#[derive(Deserialize, Clone)]
pub struct Hit {
    pub id: u64,
    pub name: String,
    /// Unix time of the earliest release.
    first_release_date: Option<i64>,
    #[serde(default)]
    genres: Vec<Genre>,
}

#[derive(Deserialize, Clone)]
struct Genre {
    name: String,
}

/// The hit named exactly `name` (any case), else the best match. Hits with a
/// release date come first: IGDB's search ranks the original release above ports
/// with the same name, and undated entries are usually unreleased or odd ones.
fn pick<'a>(hits: &'a [Hit], name: &str) -> Option<&'a Hit> {
    let exact = |h: &&Hit| h.is_named(name);
    let dated = || hits.iter().filter(|h| h.first_release_date.is_some());
    dated().find(exact).or_else(|| dated().next()).or_else(|| hits.iter().find(exact)).or(hits.first())
}

impl Hit {
    /// Whether this is the game named `name`, any case. `pick` prefers these, so a
    /// hit that isn't is IGDB's best guess.
    pub fn is_named(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn year(&self) -> Option<i32> {
        self.first_release_date.map(year_of)
    }
}

impl From<&Hit> for Meta {
    fn from(h: &Hit) -> Self {
        Meta {
            name: Some(h.name.clone()),
            year: h.year(),
            genres: h.genres.iter().map(|g| g.name.clone()).collect(),
            igdb_id: None,
            igdb_platform: None,
        }
    }
}

/// The UTC calendar year of a unix time.
fn year_of(secs: i64) -> i32 {
    // days since 1970-01-01 to a civil date, from Howard Hinnant's `civil_from_days`
    let z = secs.div_euclid(86400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let year = yoe + era * 400 + i64::from(mp >= 10); // March-based years: Jan and Feb belong to the next
    year as i32
}

/// One game for `spawn_meta_fetcher` to look up: its index in the games, and what
/// `Client::meta` takes.
pub struct Job {
    pub index: usize,
    pub name: String,
    pub igdb_id: Option<u64>,
    pub platform: Option<String>,
}

/// Looks up the metadata of each job on a background thread and caches it by name.
/// Sends `(index, meta)` for every game looked up, calling `on_done` after each so
/// the UI can repaint. Without credentials it only says so.
pub fn spawn_meta_fetcher(
    jobs: Vec<Job>,
    credentials: Option<(String, String)>,
    on_done: impl Fn() + Send + 'static,
) -> mpsc::Receiver<(usize, Meta)> {
    let (tx, rx) = mpsc::channel();
    if jobs.is_empty() {
        return rx;
    }
    let Some((id, secret)) = credentials else {
        eprintln!("no IGDB credentials set; {{year}} and {{genres}} stay blank");
        return rx;
    };
    thread::spawn(move || {
        let mut client = match Client::new(id, secret) {
            Ok(c) => c,
            Err(e) => return eprintln!("IGDB sign-in failed: {e}"),
        };
        let path = config::igdb_meta_file();
        // a platform IGDB doesn't have fails every search on it, so it's said once
        let mut bad_platforms = HashSet::new();
        for Job { index, name, igdb_id, platform } in jobs {
            if platform.as_ref().is_some_and(|p| bad_platforms.contains(p)) {
                continue;
            }
            match client.meta(&name, igdb_id, platform.as_deref()) {
                Ok(found) => {
                    // a game IGDB doesn't know is cached too, so it isn't asked about on every start
                    let meta = found.unwrap_or_else(|| {
                        match igdb_id {
                            Some(id) => eprintln!("IGDB has no game with igdb_id {id} (of {name:?})"),
                            None => eprintln!("no IGDB game matches {name:?}"),
                        }
                        Meta { igdb_id, igdb_platform: platform.clone(), ..Meta::default() }
                    });
                    if let Err(e) = cache_meta(&path, &name, &meta) {
                        eprintln!("can't cache the IGDB data of {name:?} in {}: {e}", path.display());
                    }
                    if tx.send((index, meta)).is_err() {
                        return; // UI is gone, or reloaded and started a new fetcher
                    }
                    on_done();
                }
                Err(Lookup::NoPlatform(slug)) => {
                    eprintln!("IGDB has no platform with the slug {slug:?}; check igdb_platform");
                    bad_platforms.insert(slug);
                }
                Err(Lookup::Failed(e)) => eprintln!("IGDB lookup failed for {name:?}: {e}"),
            }
        }
    });
    rx
}

pub struct Client {
    client_id: String,
    client_secret: String,
    token: String,
    /// True if `token` came from the cache, so a rejection may just mean it was revoked.
    cached: bool,
    agent: ureq::Agent,
    /// When the last IGDB request went out, see `REQUEST_GAP`.
    last_request: Option<Instant>,
    /// IGDB's platform ids by slug, see `platform_id`.
    platforms: HashMap<String, Option<u64>>,
}

/// Why `Client::find` found nothing.
#[derive(Debug)]
pub enum Lookup {
    /// IGDB has no platform with this `igdb_platform` slug.
    NoPlatform(String),
    Failed(Box<dyn Error>),
}

impl<E: Into<Box<dyn Error>>> From<E> for Lookup {
    fn from(e: E) -> Self {
        Lookup::Failed(e.into())
    }
}

impl std::fmt::Display for Lookup {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Lookup::NoPlatform(slug) => write!(f, "IGDB has no platform with the slug {slug:?}"),
            Lookup::Failed(e) => e.fmt(f),
        }
    }
}

impl Client {
    /// Uses the cached app access token, or trades the Twitch app's client id and
    /// secret for a new one.
    pub fn new(client_id: String, client_secret: String) -> Res<Self> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .user_agent("rsgl")
            .build();
        let mut c = Self { client_id, client_secret, token: String::new(), cached: false, agent, last_request: None, platforms: HashMap::new() };
        match Token::load(&config::igdb_token_file(), &c.client_id) {
            Some(t) => {
                c.token = t.access_token;
                c.cached = true;
            }
            None => c.renew()?,
        }
        Ok(c)
    }

    /// Gets a new token from Twitch and caches it.
    fn renew(&mut self) -> Res<()> {
        let grant: Grant = self
            .agent
            .post(TOKEN_URL)
            .send_form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .map_err(explain)?
            .into_json()?;
        let token = Token {
            client_id: self.client_id.clone(),
            access_token: grant.access_token,
            expires_at: now() + grant.expires_in,
        };
        let path = config::igdb_token_file();
        if let Err(e) = token.save(&path) {
            // still usable for this run, the next one just asks Twitch again
            eprintln!("rsgl: can't cache the IGDB token in {}: {e}", path.display());
        }
        self.token = token.access_token;
        self.cached = false;
        Ok(())
    }

    /// POSTs an Apicalypse query to an IGDB endpoint. A cached token that IGDB
    /// rejects (revoked, or the app's secret was reset) is renewed once, and a
    /// 429 Too Many Requests waits `RATE_LIMIT_WAIT` and tries again.
    fn query(&mut self, endpoint: &str, body: &str) -> Res<ureq::Response> {
        let mut waits = 0;
        loop {
            if let Some(last) = self.last_request {
                thread::sleep(REQUEST_GAP.saturating_sub(last.elapsed()));
            }
            self.last_request = Some(Instant::now());
            let resp = self
                .agent
                .post(&format!("{BASE}/{endpoint}"))
                .set("Client-ID", &self.client_id)
                .set("Authorization", &format!("Bearer {}", self.token))
                .send_string(body);
            match resp {
                // renew() clears `cached`, so this happens at most once
                Err(ureq::Error::Status(401, _)) if self.cached => self.renew()?,
                Err(ureq::Error::Status(429, _)) if waits < RATE_LIMIT_RETRIES => {
                    waits += 1;
                    eprintln!("rsgl: IGDB says too many requests, waiting {}s", RATE_LIMIT_WAIT.as_secs());
                    thread::sleep(RATE_LIMIT_WAIT);
                }
                r => return r.map_err(explain),
            }
        }
    }

    /// IGDB's JSON for the top 5 games matching `name`.
    pub fn search(&mut self, name: &str) -> Res<String> {
        Ok(self.query("games", &search_query(name, FIELDS, None, 5))?.into_string()?)
    }

    /// The release year and genres of IGDB game `igdb_id`, or without one, of the
    /// game on `platform` (an IGDB slug) best matching `name`. `None` if IGDB has no
    /// such game.
    pub fn meta(&mut self, name: &str, igdb_id: Option<u64>, platform: Option<&str>) -> Result<Option<Meta>, Lookup> {
        let found = self.find(name, igdb_id, platform)?;
        Ok(found.map(|h| Meta { igdb_id, igdb_platform: platform.map(str::to_string), ..Meta::from(&h) }))
    }

    /// The IGDB game `meta` takes the data from, for `--igdb-matches`. `platform`
    /// doesn't matter with an `igdb_id`, since there's no search to narrow.
    pub fn find(&mut self, name: &str, igdb_id: Option<u64>, platform: Option<&str>) -> Result<Option<Hit>, Lookup> {
        let query = match (igdb_id, platform) {
            (Some(id), _) => id_query(id, META_FIELDS),
            (None, Some(slug)) => {
                let platform = self.platform_id(slug)?.ok_or_else(|| Lookup::NoPlatform(slug.to_string()))?;
                search_query(name, META_FIELDS, Some(platform), 10)
            }
            (None, None) => search_query(name, META_FIELDS, None, 10),
        };
        let hits: Vec<Hit> = self.query("games", &query)?.into_json()?;
        let hit = match igdb_id {
            Some(_) => hits.first(),
            None => pick(&hits, name),
        };
        Ok(hit.cloned())
    }

    /// IGDB's id for the platform with `slug` ("n64"), `None` if there's none. Asked
    /// once per slug, since every ROM in a folder has the same one.
    fn platform_id(&mut self, slug: &str) -> Res<Option<u64>> {
        if let Some(&id) = self.platforms.get(slug) {
            return Ok(id);
        }
        #[derive(Deserialize)]
        struct Platform {
            id: u64,
        }
        let found: Vec<Platform> = self.query("platforms", &format!("fields id; where slug = {};", quote(slug)))?.into_json()?;
        let id = found.first().map(|p| p.id);
        self.platforms.insert(slug.to_string(), id);
        Ok(id)
    }
}

/// The Apicalypse query body for a search, only among games on IGDB platform `platform`.
fn search_query(name: &str, fields: &str, platform: Option<u64>, limit: u32) -> String {
    let only = platform.map(|p| format!(" where platforms = ({p});")).unwrap_or_default();
    format!("search {}; fields {fields};{only} limit {limit};", quote(name))
}

/// `s` as an Apicalypse string.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The Apicalypse query body for one game by its IGDB id.
fn id_query(id: u64, fields: &str) -> String {
    format!("fields {fields}; where id = {id};")
}

/// ureq's status errors only give the code; Twitch and IGDB say what's wrong in the body.
fn explain(e: ureq::Error) -> Box<dyn Error> {
    match e {
        ureq::Error::Status(code, r) => {
            let url = r.get_url().to_string();
            let body = r.into_string().unwrap_or_default();
            format!("{url}: status {code}: {}", body.trim()).into()
        }
        e => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_cache_round_trips_and_expires() {
        let dir = std::env::temp_dir().join(format!("rsgl-igdb-test-{}", std::process::id()));
        let path = dir.join("igdb_token.toml");
        let token = |expires_at| Token { client_id: "app".into(), access_token: "tok".into(), expires_at };

        assert!(Token::load(&path, "app").is_none(), "no file, no token");
        token(now() + 86400).save(&path).unwrap();
        assert_eq!(Token::load(&path, "app").unwrap().access_token, "tok");
        assert!(Token::load(&path, "other app").is_none(), "tokens belong to one client id");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }

        token(now() + EXPIRY_MARGIN / 2).save(&path).unwrap();
        assert!(Token::load(&path, "app").is_none(), "about to expire counts as expired");

        fs::write(&path, "not toml [").unwrap();
        assert!(Token::load(&path, "app").is_none(), "a broken file is ignored");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_names_are_quoted() {
        assert_eq!(search_query("Portal 2", "name", None, 5), r#"search "Portal 2"; fields name; limit 5;"#);
        assert!(search_query(r#"The "Game" \o/"#, "name", None, 5).starts_with(r#"search "The \"Game\" \\o/";"#));
        assert_eq!(
            search_query("1080 Snowboarding", "name", Some(4), 10),
            r#"search "1080 Snowboarding"; fields name; where platforms = (4); limit 10;"#
        );
        assert_eq!(id_query(707, "name"), "fields name; where id = 707;");
    }

    #[test]
    fn years_of_unix_times() {
        assert_eq!(year_of(0), 1970);
        assert_eq!(year_of(1206316800), 2008); // 2008-03-24
        assert_eq!(year_of(1230767999), 2008); // 2008-12-31 23:59:59
        assert_eq!(year_of(1230768000), 2009);
        assert_eq!(year_of(951782400), 2000); // 2000-02-29
        assert_eq!(year_of(-1), 1969);
    }

    fn hit(name: &str, date: Option<i64>, genres: &[&str]) -> Hit {
        Hit { id: 1, name: name.into(), first_release_date: date, genres: genres.iter().map(|g| Genre { name: g.to_string() }).collect() }
    }

    fn year(hits: &[Hit], name: &str) -> Option<i32> {
        pick(hits, name).and_then(|h| Meta::from(h).year)
    }

    #[test]
    fn the_exact_name_wins_then_the_first_dated_hit() {
        let hits = [hit("Quake II", None, &[]), hit("Quake", Some(820454400), &[]), hit("QUAKE II", Some(1206316800), &[])];
        assert_eq!(year(&hits, "quake ii"), Some(2008));
        assert_eq!(year(&hits, "Quake 3"), Some(1996), "no exact match, first hit with a date");
        let meta = Meta { name: Some("Quake".into()), genres: vec!["Shooter".into()], ..Meta::default() };
        assert_eq!(pick(&[hit("Quake", None, &["Shooter"])], "Quake").map(Meta::from), Some(meta), "undated hits still count");
        assert!(pick(&[], "Quake").is_none());
    }

    #[test]
    fn kanes_wrath_from_igdb_is_2008_with_its_genres() {
        // the hits `--igdb "Command & Conquer 3: Kane's Wrath"` returned on #14
        let name = "Command & Conquer 3: Kane's Wrath";
        let json = format!(
            r#"[{{"id": 707, "first_release_date": 1206316800, "name": "{name}",
                "genres": [{{"id": 11, "name": "Real Time Strategy (RTS)"}}, {{"id": 13, "name": "Simulator"}}, {{"id": 15, "name": "Strategy"}}]}},
               {{"id": 395863, "first_release_date": 1214179200, "name": "{name}"}}]"#
        );
        let hits: Vec<Hit> = serde_json::from_str(&json).unwrap();
        let meta = Meta::from(pick(&hits, name).unwrap());
        assert_eq!(meta.year, Some(2008));
        assert_eq!(meta.genres, ["Real Time Strategy (RTS)", "Simulator", "Strategy"]);
    }

    #[test]
    fn metadata_is_cached_by_name() {
        let dir = std::env::temp_dir().join(format!("rsgl-igdb-meta-test-{}", std::process::id()));
        let path = dir.join("igdb_meta.toml");
        assert!(read_meta(&path).is_empty());
        let cnc = Meta { name: Some("Command & Conquer 3: Kane's Wrath".into()), year: Some(2008), genres: vec!["Strategy".into()], ..Meta::default() };
        let doom = Meta { year: Some(1993), igdb_id: Some(673), ..Meta::default() };
        cache_meta(&path, "Command & Conquer 3: Kane's Wrath", &cnc).unwrap();
        cache_meta(&path, "Doom", &doom).unwrap();
        cache_meta(&path, "Quake", &Meta::default()).unwrap();
        let all = read_meta(&path);
        assert_eq!(all.get("Command & Conquer 3: Kane's Wrath"), Some(&cnc));
        assert_eq!(all.get("Doom"), Some(&doom), "the igdb_id it was looked up by is kept");
        assert_eq!(all.get("Quake"), Some(&Meta::default()), "a game IGDB has no date or genres for is still cached");
        fs::write(&path, "not toml [").unwrap();
        assert!(read_meta(&path).is_empty(), "a broken file is ignored");
        fs::remove_dir_all(&dir).unwrap();
    }
}
