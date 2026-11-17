//! ROMs identified by their contents on Hasheous (hasheous.org): the SHA-1 of a
//! `[[roms]]` file gives the game's proper name and its IGDB and SteamGridDB ids, so
//! a badly named file still gets the right title, art and metadata.
use crate::config;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::{self, Read},
    path::Path,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, UNIX_EPOCH},
};

type Res<T> = Result<T, Box<dyn Error>>;

/// What Hasheous says a ROM is.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steamgriddb_id: Option<u64>,
}

/// One ROM file that was looked up.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Entry {
    /// The file as it was looked up, see `Stamp`.
    size: u64,
    modified: u64,
    /// `None`: Hasheous doesn't know the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    found: Option<Found>,
}

/// A file's size and modification time (unix seconds), so a file that changed since
/// it was looked up is looked up again.
type Stamp = (u64, u64);

fn stamp(file: &Path) -> io::Result<Stamp> {
    let m = fs::metadata(file)?;
    let modified = m.modified()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    Ok((m.len(), modified))
}

/// The files looked up so far, by full path. A missing or broken file counts as
/// empty; it's only a cache.
#[derive(Default)]
pub struct Known(BTreeMap<String, Entry>);

impl Known {
    /// What Hasheous said about `file`, if it was asked since the file last changed:
    /// `Some(None)` if it doesn't know the file.
    pub fn get(&self, file: &Path) -> Option<Option<&Found>> {
        let e = self.0.get(file.to_str()?)?;
        let now = stamp(file).ok()?;
        (now == (e.size, e.modified)).then_some(e.found.as_ref())
    }
}

#[cfg(test)]
impl Known {
    /// What Hasheous said about each file as it is now, for tests elsewhere.
    pub fn of(files: &[(&Path, Option<Found>)]) -> Known {
        let entry = |f: &Path, found| {
            let (size, modified) = stamp(f).unwrap();
            (f.to_str().unwrap().to_string(), Entry { size, modified, found })
        };
        Known(files.iter().map(|(f, found)| entry(f, found.clone())).collect())
    }
}

pub fn cached() -> Known {
    read(&config::hasheous_file())
}

fn read(path: &Path) -> Known {
    Known(fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default())
}

fn cache(path: &Path, file: &Path, stamp: Stamp, found: Option<Found>) -> Res<()> {
    let mut all = read(path).0;
    let key = file.to_str().ok_or("not valid UTF-8")?.to_string();
    all.insert(key, Entry { size: stamp.0, modified: stamp.1, found });
    Ok(config::write_file(path, &toml::to_string(&all)?, false)?)
}

/// The SHA-1 that ROM sets list for `file`: of the file itself, or for a zip, of the
/// one file in it. `None` for a zip with more than one file, which isn't a single ROM.
fn hash(file: &Path) -> Res<Option<String>> {
    let mut sha = sha1_smol::Sha1::new();
    let is_zip = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip {
        feed(&mut sha, fs::File::open(file)?)?;
        return Ok(Some(sha.digest().to_string()));
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(file)?)?;
    let files: Vec<usize> = (0..zip.len()).filter(|&i| zip.by_index_raw(i).is_ok_and(|f| f.is_file())).collect();
    let [only] = files[..] else { return Ok(None) };
    feed(&mut sha, zip.by_index(only)?)?;
    Ok(Some(sha.digest().to_string()))
}

fn feed(sha: &mut sha1_smol::Sha1, mut from: impl Read) -> io::Result<()> {
    let mut buf = vec![0; 1 << 16];
    loop {
        match from.read(&mut buf)? {
            0 => return Ok(()),
            n => sha.update(&buf[..n]),
        }
    }
}

/// Hasheous's answer to a lookup, as far as `Found` goes.
#[derive(Deserialize)]
struct Hit {
    name: String,
    #[serde(default)]
    metadata: Vec<Source>,
}

/// Where else the game is known: `source` is e.g. "IGDB" or "SteamGridDb", and `id`
/// is blank when it isn't matched there.
#[derive(Deserialize)]
struct Source {
    source: String,
    #[serde(default)]
    id: String,
}

impl From<Hit> for Found {
    fn from(h: Hit) -> Self {
        let id = |source: &str| h.metadata.iter().find(|m| m.source == source).and_then(|m| m.id.parse().ok());
        Found { igdb_id: id("IGDB"), steamgriddb_id: id("SteamGridDb"), name: h.name.trim().to_string() }
    }
}

/// The game with SHA-1 `sha1` on the Hasheous API at `api`, `None` if it doesn't know it.
fn lookup(agent: &ureq::Agent, api: &str, sha1: &str) -> Res<Option<Found>> {
    match agent.get(&format!("{api}/Lookup/ByHash/sha1/{sha1}")).call() {
        Ok(r) => Ok(Some(r.into_json::<Hit>()?.into())),
        Err(ureq::Error::Status(404, r)) if is_hasheous(&r) => Ok(None),
        Err(ureq::Error::Status(404, _)) => Err(format!("no Hasheous API at {api}, check hasheous_url").into()),
        Err(e) => Err(e.into()),
    }
}

/// Whether a 404 came from Hasheous saying it doesn't know the hash, rather than from
/// a server that isn't Hasheous (a wrong `hasheous_url`), which would otherwise get
/// every ROM cached as unknown.
fn is_hasheous(r: &ureq::Response) -> bool {
    r.header("api-supported-versions").is_some()
}

/// Hashes and looks up each of `files` on the Hasheous API at `api` on a background
/// thread, and caches what it finds. Sends whether each was identified, calling
/// `on_done` after each, and once more when it's finished and the channel is closed.
/// A failed lookup (no network, wrong server) stops it; the rest are tried again next
/// time.
pub fn spawn_identifier(api: String, files: Vec<PathBuf>, on_done: impl Fn() + Send + 'static) -> mpsc::Receiver<bool> {
    let (tx, rx) = mpsc::channel();
    if files.is_empty() {
        return rx;
    }
    thread::spawn(move || {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("rsgl").build();
        let path = config::hasheous_file();
        for file in files {
            let shown = file.display();
            // stamped first, so a file that changes while it's hashed is looked up again
            let hashed = || -> Res<_> { Ok((stamp(&file)?, hash(&file)?)) };
            let (stamp, sha1) = match hashed() {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("rsgl: can't hash {shown} for Hasheous: {e}");
                    continue;
                }
            };
            let found = match sha1.map(|h| lookup(&agent, &api, &h)).transpose() {
                Ok(found) => found.flatten(),
                Err(e) => {
                    eprintln!("rsgl: Hasheous lookup failed for {shown}: {e}; the rest wait for the next start");
                    break;
                }
            };
            if found.is_none() {
                eprintln!("rsgl: Hasheous doesn't know {shown}, keeping its file name");
            }
            let identified = found.is_some();
            // a file Hasheous doesn't know is cached too, so it isn't hashed on every start
            if let Err(e) = cache(&path, &file, stamp, found) {
                eprintln!("rsgl: can't cache what Hasheous said about {shown} in {}: {e}", path.display());
            }
            if tx.send(identified).is_err() {
                return; // UI is gone, or reloaded and started a new identifier
            }
            on_done();
        }
        drop(tx);
        on_done();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rsgl-hasheous-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn super_mario_world_from_hasheous() {
        // trimmed from what Lookup/ByHash/sha1/6b47bb75d16514b6a476aa0c73a683a2a4c18765 returned
        let json = r#"{"id": 337, "name": "Super Mario World", "platform": {"name": "Nintendo Super Nintendo Entertainment System"},
            "metadata": [
                {"objectType": "Game", "id": "1070", "status": "Mapped", "source": "IGDB"},
                {"objectType": "Game", "id": "", "status": "NotMapped", "source": "GiantBomb"},
                {"objectType": "Game", "id": "https://en.wikipedia.org/wiki/Super_Mario_World", "status": "Mapped", "source": "Wikipedia"},
                {"objectType": "Game", "id": "36886", "status": "Mapped", "source": "SteamGridDb"}]}"#;
        let found = Found::from(serde_json::from_str::<Hit>(json).unwrap());
        assert_eq!(found, Found { name: "Super Mario World".into(), igdb_id: Some(1070), steamgriddb_id: Some(36886) });
        let bare = Found::from(serde_json::from_str::<Hit>(r#"{"name": "Homebrew", "metadata": [{"source": "IGDB", "id": ""}]}"#).unwrap());
        assert_eq!((bare.igdb_id, bare.steamgriddb_id), (None, None), "blank ids are no ids");
    }

    #[test]
    fn only_hasheous_404s_mean_unknown() {
        // headers as hasheous.org sent them for an unknown hash, and for a path it doesn't have
        let unknown: ureq::Response = "HTTP/1.1 404 Not Found\r\napi-supported-versions: 1.0\r\n\r\nThe provided hash was not found in any signature database.".parse().unwrap();
        let elsewhere: ureq::Response = "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".parse().unwrap();
        assert!(is_hasheous(&unknown));
        assert!(!is_hasheous(&elsewhere), "a wrong hasheous_url isn't every ROM being unknown");
    }

    #[test]
    fn zips_hash_the_rom_inside() {
        let dir = temp("zip");
        let zip_with = |name: &str, files: &[&str]| {
            let path = dir.join(name);
            let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            for f in files {
                z.start_file(*f, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(b"rom").unwrap();
            }
            z.finish().unwrap();
            path
        };
        let plain = dir.join("game.sfc");
        fs::write(&plain, b"rom").unwrap();
        let sha = sha1_smol::Sha1::from(b"rom").digest().to_string();
        assert_eq!(hash(&plain).unwrap(), Some(sha.clone()));
        assert_eq!(hash(&zip_with("one.ZIP", &["game.sfc"])).unwrap(), Some(sha), "the file in it, not the zip");
        assert_eq!(hash(&zip_with("two.zip", &["a.sfc", "b.sfc"])).unwrap(), None, "a set of files isn't one ROM");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lookups_are_cached_until_the_file_changes() {
        let dir = temp("cache");
        let (path, rom) = (dir.join("hasheous.toml"), dir.join("smw.sfc"));
        fs::write(&rom, b"rom").unwrap();
        let smw = Found { name: "Super Mario World".into(), igdb_id: Some(1070), steamgriddb_id: None };
        assert_eq!(read(&path).get(&rom), None, "never looked up");
        cache(&path, &rom, stamp(&rom).unwrap(), Some(smw.clone())).unwrap();
        assert_eq!(read(&path).get(&rom), Some(Some(&smw)));
        let other = dir.join("unknown.sfc");
        fs::write(&other, b"???").unwrap();
        cache(&path, &other, stamp(&other).unwrap(), None).unwrap();
        assert_eq!(read(&path).get(&other), Some(None), "unknown is remembered too");
        assert_eq!(read(&path).get(&rom), Some(Some(&smw)), "and doesn't replace the others");

        fs::write(&rom, b"patched").unwrap();
        assert_eq!(read(&path).get(&rom), None, "a changed file is looked up again");
        fs::write(&path, "not toml [").unwrap();
        assert_eq!(read(&path).get(&other), None, "a broken file is ignored");
        fs::remove_dir_all(&dir).unwrap();
    }
}
