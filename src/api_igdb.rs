//! Game metadata from IGDB: the raw name search printed by `--igdb`, and the
//! release year and genres behind `{year}` and `{genres}`.
use crate::{config, playtime::now};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, error::Error, fs, io::Write, path::Path, sync::mpsc, thread, time::Duration};

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const BASE: &str = "https://api.igdb.com/v4";
/// What a search asks for about each game.
const FIELDS: &str = "name, summary, first_release_date, genres.name, \
    involved_companies.company.name, involved_companies.developer, total_rating, url";
/// What a metadata lookup asks for, see `Hit`.
const META_FIELDS: &str = "name, first_release_date, genres.name";
/// A cached token this close to expiring is replaced instead of used.
const EXPIRY_MARGIN: u64 = 60 * 60;

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
        write_file(path, &toml::to_string(self)?, true)
    }
}

/// Writes `text` to `path` through a temporary file, so a half-written file is never
/// read. `owner_only` makes it readable by its owner only (on Unix).
fn write_file(path: &Path, text: &str, owner_only: bool) -> Res<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("part");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if owner_only {
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    }
    opts.open(&tmp)?.write_all(text.as_bytes())?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// What rsgl keeps about a game from IGDB.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Meta {
    pub year: Option<i32>,
    pub genres: Vec<String>,
    /// The game's `igdb_id` when it was looked up, `None` if it was found by name.
    /// Cached data from a different id doesn't count, so changing it looks the game up again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb_id: Option<u64>,
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
    write_file(path, &toml::to_string(&all)?, false)
}

/// One search result, as far as `Meta` goes.
#[derive(Deserialize)]
struct Hit {
    name: String,
    /// Unix time of the earliest release.
    first_release_date: Option<i64>,
    #[serde(default)]
    genres: Vec<Genre>,
}

#[derive(Deserialize)]
struct Genre {
    name: String,
}

/// The hit named exactly `name` (any case), else the best match. Hits with a
/// release date come first: IGDB's search ranks the original release above ports
/// with the same name, and undated entries are usually unreleased or odd ones.
fn pick<'a>(hits: &'a [Hit], name: &str) -> Option<&'a Hit> {
    let exact = |h: &&Hit| h.name.eq_ignore_ascii_case(name);
    let dated = || hits.iter().filter(|h| h.first_release_date.is_some());
    dated().find(exact).or_else(|| dated().next()).or_else(|| hits.iter().find(exact)).or(hits.first())
}

impl From<&Hit> for Meta {
    fn from(h: &Hit) -> Self {
        Meta {
            year: h.first_release_date.map(year_of),
            genres: h.genres.iter().map(|g| g.name.clone()).collect(),
            igdb_id: None,
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

/// Looks up the metadata of each `(index, name, igdb_id)` on a background thread and
/// caches it by name. Sends `(index, meta)` for every game looked up, calling `on_done`
/// after each so the UI can repaint. Without credentials it only says so.
pub fn spawn_meta_fetcher(
    jobs: Vec<(usize, String, Option<u64>)>,
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
        for (index, name, igdb_id) in jobs {
            match client.meta(&name, igdb_id) {
                Ok(found) => {
                    // a game IGDB doesn't know is cached too, so it isn't asked about on every start
                    let meta = found.unwrap_or_else(|| {
                        match igdb_id {
                            Some(id) => eprintln!("IGDB has no game with igdb_id {id} (of {name:?})"),
                            None => eprintln!("no IGDB game matches {name:?}"),
                        }
                        Meta { igdb_id, ..Meta::default() }
                    });
                    if let Err(e) = cache_meta(&path, &name, &meta) {
                        eprintln!("can't cache the IGDB data of {name:?} in {}: {e}", path.display());
                    }
                    if tx.send((index, meta)).is_err() {
                        return; // UI is gone, or reloaded and started a new fetcher
                    }
                    on_done();
                }
                Err(e) => eprintln!("IGDB lookup failed for {name:?}: {e}"),
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
}

impl Client {
    /// Uses the cached app access token, or trades the Twitch app's client id and
    /// secret for a new one.
    pub fn new(client_id: String, client_secret: String) -> Res<Self> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .user_agent("rsgl")
            .build();
        let mut c = Self { client_id, client_secret, token: String::new(), cached: false, agent };
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
    /// rejects (revoked, or the app's secret was reset) is renewed once.
    fn query(&mut self, endpoint: &str, body: &str) -> Res<ureq::Response> {
        let send = |c: &Self| {
            c.agent
                .post(&format!("{BASE}/{endpoint}"))
                .set("Client-ID", &c.client_id)
                .set("Authorization", &format!("Bearer {}", c.token))
                .send_string(body)
                .map_err(Box::new) // ureq::Error is big
        };
        let resp = match send(self) {
            Err(e) if self.cached && matches!(*e, ureq::Error::Status(401, _)) => {
                self.renew()?;
                send(self)
            }
            r => r,
        };
        resp.map_err(|e| explain(*e))
    }

    /// IGDB's JSON for the top 5 games matching `name`.
    pub fn search(&mut self, name: &str) -> Res<String> {
        Ok(self.query("games", &search_query(name, FIELDS, 5))?.into_string()?)
    }

    /// The release year and genres of IGDB game `igdb_id`, or without one, of the
    /// game best matching `name`. `None` if IGDB has no such game.
    pub fn meta(&mut self, name: &str, igdb_id: Option<u64>) -> Res<Option<Meta>> {
        let query = match igdb_id {
            Some(id) => id_query(id, META_FIELDS),
            None => search_query(name, META_FIELDS, 10),
        };
        let hits: Vec<Hit> = self.query("games", &query)?.into_json()?;
        let hit = match igdb_id {
            Some(_) => hits.first(),
            None => pick(&hits, name),
        };
        Ok(hit.map(|h| Meta { igdb_id, ..Meta::from(h) }))
    }
}

/// The Apicalypse query body for a search.
fn search_query(name: &str, fields: &str, limit: u32) -> String {
    let quoted = name.replace('\\', "\\\\").replace('"', "\\\"");
    format!("search \"{quoted}\"; fields {fields}; limit {limit};")
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
        assert_eq!(search_query("Portal 2", "name", 5), r#"search "Portal 2"; fields name; limit 5;"#);
        assert!(search_query(r#"The "Game" \o/"#, "name", 5).starts_with(r#"search "The \"Game\" \\o/";"#));
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
        Hit { name: name.into(), first_release_date: date, genres: genres.iter().map(|g| Genre { name: g.to_string() }).collect() }
    }

    fn year(hits: &[Hit], name: &str) -> Option<i32> {
        pick(hits, name).and_then(|h| Meta::from(h).year)
    }

    #[test]
    fn the_exact_name_wins_then_the_first_dated_hit() {
        let hits = [hit("Quake II", None, &[]), hit("Quake", Some(820454400), &[]), hit("QUAKE II", Some(1206316800), &[])];
        assert_eq!(year(&hits, "quake ii"), Some(2008));
        assert_eq!(year(&hits, "Quake 3"), Some(1996), "no exact match, first hit with a date");
        assert_eq!(pick(&[hit("Quake", None, &["Shooter"])], "Quake").map(Meta::from), Some(Meta { genres: vec!["Shooter".into()], ..Meta::default() }), "undated hits still count");
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
        let cnc = Meta { year: Some(2008), genres: vec!["Strategy".into()], igdb_id: None };
        let doom = Meta { year: Some(1993), genres: vec![], igdb_id: Some(673) };
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
