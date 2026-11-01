//! MusicBrainz lookups on a single worker thread (their API allows ~1 request/second).
//!
//! Album lookups compare a whole local album against a release and emit per-field changes;
//! nothing is applied until the user accepts it.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, unbounded};
use deunicode::deunicode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::library::grouping::UNKNOWN;
use crate::library::names::{artist_key, clean_album, clean_title, primary_artist, similar_keys, title_key};

const API: &str = "https://musicbrainz.org/ws/2";
const COVER_ART: &str = "https://coverartarchive.org";
/// When the app last asked MusicBrainz: lookups and cover searches run on different threads
/// but share the one-request-a-second allowance.
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Artist,
    AlbumArtist,
    Album,
    Track,
    Disc,
    Year,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Artist => "artist",
            Field::AlbumArtist => "album artist",
            Field::Album => "album",
            Field::Track => "track",
            Field::Disc => "disc",
            Field::Year => "year",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldChange {
    /// The song: its place in the request (`LocalTrack::idx`); in the app, its place in the library.
    pub track: usize,
    pub field: Field,
    pub old: String,
    pub new: String,
    pub accept: bool,
}

#[derive(Clone, Debug)]
pub struct LocalTrack {
    /// The song's place in the request; suggestions name songs by it.
    pub idx: usize,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<u16>,
    pub seconds: f64,
}

#[derive(Clone, Debug)]
pub struct ArtistHit {
    /// The MusicBrainz id.
    pub id: String,
    pub name: String,
    pub detail: String,
    pub score: u32,
}

/// What the artist panel shows about an artist, from their MusicBrainz entry: who they are, and
/// where to buy, hear or find out more of their music. Kept in the library database.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistInfo {
    pub mbid: String,
    pub name: String,
    pub disambiguation: String,
    /// "US", "GB".
    pub country: String,
    /// The years they were active, as far as MusicBrainz knows.
    pub begin: Option<u16>,
    pub end: Option<u16>,
    pub ended: bool,
    /// Most voted first.
    pub genres: Vec<String>,
    /// In `SITES` order.
    pub links: Vec<Link>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkUse {
    Buy,
    Listen,
    Discover,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    #[serde(rename = "use")]
    pub kind: LinkUse,
    pub site: String,
    pub url: String,
}

/// The sites shown, in the order shown. Buying first, from where most reaches the artist
/// (Bandcamp) and in lossless (Qobuz); records, new and used, on Discogs.
const SITES: &[(LinkUse, &str)] = &[
    (LinkUse::Buy, "Bandcamp"),
    (LinkUse::Buy, "Qobuz"),
    (LinkUse::Buy, "Apple Music"),
    (LinkUse::Buy, "7digital"),
    (LinkUse::Buy, "Beatport"),
    (LinkUse::Buy, "Discogs"),
    (LinkUse::Buy, "shop"),
    (LinkUse::Listen, "Spotify"),
    (LinkUse::Listen, "Apple Music"),
    (LinkUse::Listen, "Tidal"),
    (LinkUse::Listen, "Qobuz"),
    (LinkUse::Listen, "Deezer"),
    (LinkUse::Listen, "YouTube Music"),
    (LinkUse::Listen, "SoundCloud"),
    (LinkUse::Discover, "last.fm"),
    (LinkUse::Discover, "Rate Your Music"),
    (LinkUse::Discover, "Songkick"),
    (LinkUse::Discover, "website"),
];

/// Where a MusicBrainz URL relation goes in the panel, if anywhere: social networks, film and
/// library catalogues and the like are left out.
fn link_site(relation: &str, url: &str) -> Option<(LinkUse, &'static str)> {
    use LinkUse::*;
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or_default().to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let on = |site: &str| host == site || host.ends_with(&format!(".{site}"));
    let buying = relation == "purchase for download";
    Some(match relation {
        "bandcamp" => (Buy, "Bandcamp"),
        "discogs" => (Buy, "Discogs"),
        "purchase for mail-order" => (Buy, "shop"),
        "last.fm" => (Discover, "last.fm"),
        "songkick" => (Discover, "Songkick"),
        "official homepage" => (Discover, "website"),
        "other databases" if on("rateyourmusic.com") => (Discover, "Rate Your Music"),
        "purchase for download" | "streaming" | "free streaming" | "youtube music" | "soundcloud" => {
            // Qobuz's store, and its player.
            if on("open.qobuz.com") {
                (Listen, "Qobuz")
            } else if on("qobuz.com") {
                (Buy, "Qobuz")
            } else if on("apple.com") {
                (if buying { Buy } else { Listen }, "Apple Music")
            } else if buying && on("7digital.com") {
                (Buy, "7digital")
            } else if buying && on("beatport.com") {
                (Buy, "Beatport")
            } else if on("spotify.com") {
                (Listen, "Spotify")
            } else if on("tidal.com") {
                (Listen, "Tidal")
            } else if on("deezer.com") {
                (Listen, "Deezer")
            } else if on("music.youtube.com") {
                (Listen, "YouTube Music")
            } else if on("soundcloud.com") {
                (Listen, "SoundCloud")
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// An artist entry looked up with `inc=url-rels+genres`.
fn parse_artist(v: &Value) -> ArtistInfo {
    let mut links: Vec<Link> = Vec::new();
    for r in v["relations"].as_array().into_iter().flatten() {
        let url = str_of(&r["url"]["resource"]);
        if r["ended"].as_bool() == Some(true) || url.is_empty() {
            continue;
        }
        if let Some((kind, site)) = link_site(&str_of(&r["type"]), &url)
            && !links.iter().any(|l| l.kind == kind && l.site == site)
        {
            links.push(Link { kind, site: site.into(), url });
        }
    }
    links.sort_by_key(|l| SITES.iter().position(|&(k, s)| k == l.kind && s == l.site));
    let mut genres: Vec<(u64, String)> = v["genres"].as_array().into_iter().flatten().map(|g| (g["count"].as_u64().unwrap_or(0), str_of(&g["name"]))).collect();
    genres.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let span = &v["life-span"];
    ArtistInfo {
        mbid: str_of(&v["id"]),
        name: str_of(&v["name"]),
        disambiguation: str_of(&v["disambiguation"]),
        country: str_of(&v["country"]),
        begin: year_of(&span["begin"]),
        end: year_of(&span["end"]),
        ended: span["ended"].as_bool().unwrap_or(false),
        genres: genres.into_iter().map(|(_, g)| g).collect(),
        links,
    }
}

/// An artist found, or the candidates to choose from when the name alone doesn't say which.
pub enum ArtistFound {
    Info(ArtistInfo),
    Choose(Vec<ArtistHit>),
}

pub enum Job {
    Artist { cluster_key: String, name: String },
    /// The panel's facts and links: for the entry `mbid`, or the one artist the name matches.
    ArtistInfo { cluster_key: String, name: String, mbid: Option<String> },
    Album { album_id: String, artist: String, album: String, tracks: Vec<LocalTrack> },
    Loose { album_id: String, tracks: Vec<LocalTrack> },
}

pub enum Reply {
    Artist { cluster_key: String, result: Result<Vec<ArtistHit>, String> },
    ArtistInfo { cluster_key: String, result: Result<ArtistFound, String> },
    Album { album_id: String, result: Result<(String, Vec<FieldChange>), String> },
}

pub struct Client {
    tx: Sender<Job>,
}

impl Client {
    pub fn spawn(user_agent: String, replies: Sender<Reply>, wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = unbounded();
        thread::spawn(move || Worker::new(user_agent).run(rx, replies, wake));
        Client { tx }
    }

    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

/// An album to find a front cover for.
pub struct CoverJob {
    pub artist: String,
    pub album: String,
    pub tracks: Vec<LocalTrack>,
}

/// Look each album up as *look up* does and fetch its front cover from the Cover Art Archive
/// (the release's, else its release group's), reporting each album's JPEG and the release it
/// came from. Stops between albums once `stop` is set.
pub fn find_covers(user_agent: String, jobs: &[CoverJob], stop: &AtomicBool, mut each: impl FnMut(usize, Result<(Vec<u8>, String), String>)) {
    let mut w = Worker::new(user_agent);
    for (i, job) in jobs.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        each(i, w.cover(&job.artist, &job.album, &job.tracks).map_err(|e| format!("{e:#}")));
    }
}

struct Worker {
    agent: ureq::Agent,
    user_agent: String,
}

impl Worker {
    fn new(user_agent: String) -> Self {
        let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
        Worker { agent, user_agent }
    }

    fn run(mut self, jobs: Receiver<Job>, replies: Sender<Reply>, wake: impl Fn()) {
        for job in jobs {
            let reply = match job {
                Job::Artist { cluster_key, name } => {
                    Reply::Artist { cluster_key, result: self.artist(&name).map_err(|e| format!("{e:#}")) }
                }
                Job::ArtistInfo { cluster_key, name, mbid } => {
                    Reply::ArtistInfo { cluster_key, result: self.artist_info(&name, mbid.as_deref()).map_err(|e| format!("{e:#}")) }
                }
                Job::Album { album_id, artist, album, tracks } => {
                    Reply::Album { album_id, result: self.album(&artist, &album, &tracks).map_err(|e| format!("{e:#}")) }
                }
                Job::Loose { album_id, tracks } => {
                    Reply::Album { album_id, result: self.loose(&tracks).map_err(|e| format!("{e:#}")) }
                }
            };
            if replies.send(reply).is_err() {
                return;
            }
            wake();
        }
    }

    fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        for attempt in 0..3 {
            {
                let mut last = LAST_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(at) = *last {
                    thread::sleep(Duration::from_millis(1100).saturating_sub(at.elapsed()));
                }
                *last = Some(Instant::now());
            }
            let mut req = self.agent.get(format!("{API}/{path}")).header("User-Agent", &self.user_agent).query("fmt", "json");
            for (k, v) in params {
                req = req.query(*k, *v);
            }
            match req.call() {
                Ok(mut resp) => return resp.body_mut().read_json::<Value>().context("decoding MusicBrainz response"),
                // 503 = rate limited; back off and retry.
                Err(ureq::Error::StatusCode(503)) if attempt < 2 => thread::sleep(Duration::from_secs(2 << attempt)),
                Err(e) => return Err(e).context("MusicBrainz request"),
            }
        }
        Err(anyhow!("MusicBrainz kept rate-limiting; try again later"))
    }

    fn artist(&mut self, name: &str) -> Result<Vec<ArtistHit>> {
        // Unfielded, so MusicBrainz also searches sort names and aliases ("Guns and Roses").
        let v = self.get("artist", &[("query", &lucene_terms(name)), ("limit", "5")])?;
        Ok(v["artists"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| ArtistHit {
                id: str_of(&a["id"]),
                name: str_of(&a["name"]),
                detail: [str_of(&a["disambiguation"]), str_of(&a["country"]), str_of(&a["type"])]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
                score: a["score"].as_u64().unwrap_or(0) as u32,
            })
            .collect())
    }

    /// The entry `mbid`, or else the one artist going by `name`: several of that name (or none
    /// close) are handed back to choose from.
    fn artist_info(&mut self, name: &str, mbid: Option<&str>) -> Result<ArtistFound> {
        let mbid = match mbid {
            Some(id) => id.to_string(),
            None => {
                let hits = self.artist(name)?;
                let same: Vec<&ArtistHit> = hits.iter().filter(|h| h.score >= 90 && artist_key(&h.name) == artist_key(name)).collect();
                match same[..] {
                    [one] => one.id.clone(),
                    _ => return Ok(ArtistFound::Choose(hits)),
                }
            }
        };
        let v = self.get(&format!("artist/{mbid}"), &[("inc", "url-rels+genres")])?;
        Ok(ArtistFound::Info(parse_artist(&v)))
    }

    /// Search for the release, check the best candidates against the local songs, and suggest
    /// changes from the first one that is clearly the same album.
    fn album(&mut self, artist: &str, album: &str, local: &[LocalTrack]) -> Result<(String, Vec<FieldChange>)> {
        let (release, matched, changes) = self.release(artist, album, local)?;
        Ok((release.label(matched, local.len()), changes))
    }

    /// The front cover of the release that matches the album, as a JPEG, and that release.
    fn cover(&mut self, artist: &str, album: &str, local: &[LocalTrack]) -> Result<(Vec<u8>, String)> {
        let (release, matched, _) = self.release(artist, album, local)?;
        let label = release.label(matched, local.len());
        let mut tried = vec![format!("{COVER_ART}/release/{}/front-500", release.id)];
        if !release.group_id.is_empty() {
            tried.push(format!("{COVER_ART}/release-group/{}/front-500", release.group_id));
        }
        for url in tried {
            match self.agent.get(&url).header("User-Agent", &self.user_agent).call() {
                Ok(mut resp) => {
                    let bytes = resp.body_mut().with_config().limit(20_000_000).read_to_vec().context("downloading the cover")?;
                    return Ok((bytes, label));
                }
                Err(ureq::Error::StatusCode(404)) => continue,
                Err(e) => return Err(e).context("Cover Art Archive request"),
            }
        }
        Err(anyhow!("{label} has no cover in the Cover Art Archive"))
    }

    /// The release that is clearly this album, how many songs matched, and the changes it suggests.
    fn release(&mut self, artist: &str, album: &str, local: &[LocalTrack]) -> Result<(Release, usize, Vec<FieldChange>)> {
        let artist = known_artist(artist);
        let name = clean_album(album, artist).album;
        let mut queries = Vec::new();
        if let Some(a) = artist {
            queries.push(format!("release:\"{}\" AND artist:\"{}\"", lucene(&name), lucene(a)));
        }
        // Without the artist: catches spellings MusicBrainz writes differently ("Guns and Roses").
        queries.push(format!("release:\"{}\"", lucene(&name)));

        let mut tried = HashSet::new();
        let mut rejected = Vec::new();
        for query in queries {
            let v = self.get("release", &[("query", &query), ("limit", "25")])?;
            for id in rank_releases(&v, artist, &name, local.len()) {
                if tried.len() >= MAX_CHECKED {
                    break;
                }
                if !tried.insert(id.clone()) {
                    continue;
                }
                let rel = self.get(&format!("release/{id}"), &[("inc", "recordings artist-credits release-groups labels")])?;
                let release = Release::from_json(&rel);
                match compare(&release, local) {
                    Some((changes, matched)) => return Ok((release, matched, changes)),
                    None => rejected.push(format!("“{}” by {}", release.title, release.artist)),
                }
            }
        }
        let by = artist.map(|a| format!(" by {a}")).unwrap_or_default();
        Err(match rejected.is_empty() {
            true => anyhow!("no release named “{name}”{by} on MusicBrainz"),
            false => anyhow!("no confident match for “{name}”{by}: song titles didn't match {}", rejected.join(", ")),
        })
    }

    /// Tracks without an album: look each recording up and suggest its earliest album.
    fn loose(&mut self, local: &[LocalTrack]) -> Result<(String, Vec<FieldChange>)> {
        let mut changes = Vec::new();
        let mut found = 0;
        for lt in local {
            let artist = known_artist(&lt.artist);
            let title = clean_title(&lt.title, &[&lt.artist, &lt.album_artist]);
            let ms = (lt.seconds * 1000.0) as i64;
            let query = match artist {
                Some(a) => format!("recording:\"{}\" AND artist:\"{}\"", lucene(&title), lucene(a)),
                // Without an artist, a title alone matches too much; the length narrows it down.
                None if ms > 0 => format!("recording:\"{}\" AND dur:[{} TO {}]", lucene(&title), (ms - 3000).max(0), ms + 3000),
                None => continue,
            };
            let v = self.get("recording", &[("query", &query), ("limit", "10")])?;
            let Some(rec) = v["recordings"].as_array().into_iter().flatten().find(|r| {
                let length_ok = match r["length"].as_f64() {
                    Some(len) if lt.seconds > 0.0 => (len / 1000.0 - lt.seconds).abs() <= 5.0,
                    _ => artist.is_some(),
                };
                title_score(lt, &str_of(&r["title"]), &[]) >= TITLE_MATCH && artist.is_none_or(|a| artist_agrees(a, &r["artist-credit"])) && length_ok
            }) else {
                continue;
            };
            found += 1;
            push_change(&mut changes, lt.idx, Field::Title, &lt.title, &str_of(&rec["title"]));
            push_change(&mut changes, lt.idx, Field::Artist, &lt.artist, &credit(&rec["artist-credit"]));
            let album = rec["releases"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| r["status"].as_str() == Some("Official"))
                .filter(|r| r["release-group"]["primary-type"].as_str() == Some("Album"))
                .min_by_key(|r| str_of(&r["date"]).chars().take(4).collect::<String>().parse::<u16>().unwrap_or(u16::MAX));
            if let Some(r) = album {
                push_change(&mut changes, lt.idx, Field::Album, &lt.album, &str_of(&r["title"]));
                if let Some(y) = year_of(&r["date"]) {
                    push_change(&mut changes, lt.idx, Field::Year, &opt(lt.year), &y.to_string());
                }
                if let Some(n) = r["media"][0]["track"][0]["number"].as_str() {
                    push_change(&mut changes, lt.idx, Field::Track, &opt(lt.track), n);
                }
            }
        }
        Ok((format!("{found}/{} recordings identified", local.len()), changes))
    }
}

/// Releases fetched and compared per lookup, at most: each costs a second of rate limit.
const MAX_CHECKED: usize = 3;
/// Jaro-Winkler on `title_key`s for "the same song title".
const TITLE_MATCH: f64 = 0.88;

/// `None` for the placeholder used when files have no artist: searching MusicBrainz for an
/// artist literally called "Unknown Artist" finds thousands of unrelated releases.
fn known_artist(a: &str) -> Option<&str> {
    (!a.trim().is_empty() && artist_key(a) != artist_key(UNKNOWN)).then_some(a)
}

/// Search results worth fetching, best first. A release must have the same artist (when we know
/// it) and the same or a very similar title; only then do track count, status and score count.
fn rank_releases(v: &Value, artist: Option<&str>, album: &str, local_count: usize) -> Vec<String> {
    let mut ranked: Vec<(u8, usize, bool, u64, String)> = v["releases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| artist.is_none_or(|a| artist_agrees(a, &r["artist-credit"])))
        .filter_map(|r| {
            let tier = title_agreement(album, &str_of(&r["title"]))?;
            let count = r["track-count"].as_u64().unwrap_or(0) as usize;
            let score = r["score"].as_u64().unwrap_or(0);
            Some((tier, count.abs_diff(local_count), r["status"].as_str() != Some("Official"), u64::MAX - score, str_of(&r["id"])))
        })
        .collect();
    ranked.sort();
    ranked.into_iter().map(|r| r.4).collect()
}

/// Whether a MusicBrainz artist credit is our artist: same key, any credited artist's key, or a
/// near spelling under the same rules as the artist merging.
fn artist_agrees(local: &str, credit_json: &Value) -> bool {
    let want = artist_key(&primary_artist(local));
    let mut names = vec![credit(credit_json)];
    for c in credit_json.as_array().into_iter().flatten() {
        names.push(str_of(&c["name"]));
        names.push(str_of(&c["artist"]["name"]));
    }
    names.iter().filter(|n| !n.is_empty()).any(|n| {
        let k = artist_key(n);
        k == want || similar_keys(&k, &want).is_some()
    })
}

/// 0 = same title, 1 = close (one starts with the other's words, or a near spelling),
/// `None` = a different album. "Love" vs "Faithless Love" is different; "Love" vs
/// "Love Songs" is close and loses to an exact "Love".
fn title_agreement(local: &str, remote: &str) -> Option<u8> {
    let words = |s: &str| -> Vec<String> {
        deunicode(&clean_album(s, None).album).to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
    };
    let (l, r) = (words(local), words(remote));
    if l.is_empty() || r.is_empty() {
        return None;
    }
    if l == r {
        return Some(0);
    }
    let prefix = l.len().min(r.len());
    if l[..prefix] == r[..prefix] || strsim::jaro_winkler(&l.concat(), &r.concat()) >= 0.92 { Some(1) } else { None }
}

/// The best title similarity between a local song and a remote title, trying the local title
/// both as it is and cleaned ("01 The Beatles - Something" → "Something").
fn title_score(lt: &LocalTrack, remote: &str, artists: &[&str]) -> f64 {
    let rk = title_key(remote);
    let mut all = vec![lt.artist.as_str(), lt.album_artist.as_str()];
    all.extend_from_slice(artists);
    [lt.title.clone(), clean_title(&lt.title, &all)].iter().map(|t| strsim::jaro_winkler(&title_key(t), &rk)).fold(0.0, f64::max)
}

struct Release {
    id: String,
    group_id: String,
    title: String,
    artist: String,
    year: Option<u16>,
    country: String,
    label: String,
    tracks: Vec<RemoteTrack>,
    multi_disc: bool,
}

impl Release {
    fn from_json(rel: &Value) -> Self {
        let mut tracks = Vec::new();
        for medium in rel["media"].as_array().into_iter().flatten() {
            let disc = medium["position"].as_u64().unwrap_or(1) as u32;
            for t in medium["tracks"].as_array().into_iter().flatten() {
                tracks.push(RemoteTrack {
                    disc,
                    number: t["position"].as_u64().unwrap_or(0) as u32,
                    title: str_of(&t["title"]),
                    artist: credit(&t["artist-credit"]),
                    seconds: t["length"].as_f64().map(|ms| ms / 1000.0),
                });
            }
        }
        Release {
            id: str_of(&rel["id"]),
            group_id: str_of(&rel["release-group"]["id"]),
            title: str_of(&rel["title"]),
            artist: credit(&rel["artist-credit"]),
            year: year_of(&rel["release-group"]["first-release-date"]).or_else(|| year_of(&rel["date"])),
            country: str_of(&rel["country"]),
            label: str_of(&rel["label-info"][0]["label"]["name"]),
            tracks,
            multi_disc: rel["media"].as_array().is_some_and(|m| m.len() > 1),
        }
    }

    /// "Love — The Beatles, 2006 · GB, Apple · 26 tracks · 14/14 matched"
    fn label(&self, matched: usize, local: usize) -> String {
        let mut parts = vec![format!("{} — {}{}", self.title, self.artist, self.year.map(|y| format!(", {y}")).unwrap_or_default())];
        let place: Vec<&str> = [self.country.as_str(), self.label.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        if !place.is_empty() {
            parts.push(place.join(", "));
        }
        parts.push(format!("{} tracks", self.tracks.len()));
        parts.push(format!("{matched}/{local} matched"));
        parts.join(" · ")
    }
}

/// Pair local songs with the release's tracks and propose changes, or `None` when the release
/// isn't this album. Titles are matched first; positions and lengths are only trusted once
/// enough titles agree (or every length lines up), so a wrong release can't hand its titles out
/// by track number.
fn compare(release: &Release, local: &[LocalTrack]) -> Option<(Vec<FieldChange>, usize)> {
    let remote = &release.tracks;
    let mut pair: Vec<Option<usize>> = vec![None; local.len()];
    let mut used = vec![false; remote.len()];
    for (li, lt) in local.iter().enumerate() {
        let best = (0..remote.len())
            .filter(|&i| !used[i])
            .map(|i| (i, title_score(lt, &remote[i].title, &[&release.artist])))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, score)| *score >= TITLE_MATCH);
        if let Some((i, _)) = best {
            pair[li] = Some(i);
            used[i] = true;
        }
    }
    let by_title = pair.iter().flatten().count();
    let at = |lt: &LocalTrack| remote.iter().position(|r| Some(r.number) == lt.track && r.disc == lt.disc.unwrap_or(1));
    let lengths_line_up = local.len() >= 2
        && local.iter().all(|lt| at(lt).and_then(|i| remote[i].seconds).is_some_and(|s| lt.seconds > 0.0 && (s - lt.seconds).abs() <= 3.0));
    if by_title * 2 < local.len() && !lengths_line_up {
        return None;
    }
    for (li, lt) in local.iter().enumerate() {
        if pair[li].is_some() {
            continue;
        }
        let by_number = at(lt).filter(|&i| !used[i]);
        let by_length = || {
            let mut near = (0..remote.len()).filter(|&i| !used[i] && remote[i].seconds.is_some_and(|s| (s - lt.seconds).abs() < 2.0));
            match (near.next(), near.next()) {
                (Some(i), None) => Some(i),
                _ => None,
            }
        };
        if let Some(i) = by_number.or_else(by_length) {
            pair[li] = Some(i);
            used[i] = true;
        }
    }

    let mut changes = Vec::new();
    for (lt, ri) in local.iter().zip(&pair) {
        let Some(rt) = ri.map(|i| &remote[i]) else { continue };
        push_change(&mut changes, lt.idx, Field::Title, &lt.title, &rt.title);
        push_change(&mut changes, lt.idx, Field::Artist, &lt.artist, &rt.artist);
        push_change(&mut changes, lt.idx, Field::AlbumArtist, &lt.album_artist, &release.artist);
        push_change(&mut changes, lt.idx, Field::Album, &lt.album, &release.title);
        push_change(&mut changes, lt.idx, Field::Track, &opt(lt.track), &rt.number.to_string());
        if release.multi_disc || lt.disc.is_some_and(|d| d != rt.disc) {
            push_change(&mut changes, lt.idx, Field::Disc, &opt(lt.disc), &rt.disc.to_string());
        }
        if let Some(y) = release.year {
            push_change(&mut changes, lt.idx, Field::Year, &opt(lt.year), &y.to_string());
        }
    }
    Some((changes, pair.iter().flatten().count()))
}

struct RemoteTrack {
    disc: u32,
    number: u32,
    title: String,
    artist: String,
    seconds: Option<f64>,
}

fn push_change(out: &mut Vec<FieldChange>, track: usize, field: Field, old: &str, new: &str) {
    if !new.is_empty() && old.trim() != new.trim() {
        out.push(FieldChange { track, field, old: old.to_string(), new: new.to_string(), accept: true });
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_default()
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn year_of(v: &Value) -> Option<u16> {
    v.as_str().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok())
}

/// "Artist A feat. Artist B" from MusicBrainz's artist-credit array.
fn credit(v: &Value) -> String {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|c| format!("{}{}", str_of(&c["name"]), str_of(&c["joinphrase"])))
        .collect()
}

/// Escape every Lucene operator so a name can be used as free-text terms.
fn lucene_terms(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escape Lucene query syntax inside a quoted phrase.
fn lucene(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_links_sorted_into_buy_listen_discover() {
        // Flying Lotus's entry, cut down: some of each kind, a repeat, an ended link, and links left out.
        let rel = |kind: &str, url: &str| serde_json::json!({ "type": kind, "ended": false, "url": { "resource": url } });
        let v = serde_json::json!({
            "id": "fc7376fe-1a6f-4414-b4a7-83f50ed59c92", "name": "Flying Lotus", "country": "US",
            "life-span": { "begin": "1983-10-07", "ended": false },
            "genres": [{ "name": "downtempo", "count": 3 }, { "name": "experimental hip hop", "count": 9 }],
            "relations": [
                rel("free streaming", "https://open.spotify.com/artist/29XOeO6KIWxGthejQqn793"),
                rel("social network", "https://twitter.com/flyinglotus"),
                rel("purchase for download", "https://music.apple.com/us/artist/120737020"),
                rel("streaming", "https://music.apple.com/gb/artist/120737020"),
                rel("streaming", "https://music.apple.com/us/artist/120737020"),
                rel("purchase for download", "https://www.qobuz.com/us-en/interpreter/flying-lotus/291266"),
                rel("streaming", "https://open.qobuz.com/artist/291266"),
                rel("bandcamp", "https://flyinglotus.bandcamp.com/"),
                rel("youtube music", "https://music.youtube.com/channel/UCJvOvKwxPEL-4RtSvu_nWhQ"),
                rel("youtube", "https://www.youtube.com/channel/UC6An_wEqusDnouBmzQtHheA"),
                rel("discogs", "https://www.discogs.com/artist/365248"),
                rel("other databases", "https://rateyourmusic.com/artist/flying-lotus"),
                rel("other databases", "http://viaf.org/viaf/121197219"),
                rel("last.fm", "https://www.last.fm/music/Flying+Lotus"),
                rel("official homepage", "http://flying-lotus.com/"),
                { "type": "myspace", "ended": true, "url": { "resource": "https://myspace.com/flyinglotus" } },
            ]
        });
        let a = parse_artist(&v);
        assert_eq!((a.name.as_str(), a.country.as_str(), a.begin, a.ended), ("Flying Lotus", "US", Some(1983), false));
        assert_eq!(a.genres, ["experimental hip hop", "downtempo"]);
        let row = |kind: LinkUse| a.links.iter().filter(|l| l.kind == kind).map(|l| l.site.as_str()).collect::<Vec<_>>();
        assert_eq!(row(LinkUse::Buy), ["Bandcamp", "Qobuz", "Apple Music", "Discogs"]);
        assert_eq!(row(LinkUse::Listen), ["Spotify", "Apple Music", "Qobuz", "YouTube Music"]);
        assert!(a.links.iter().any(|l| l.kind == LinkUse::Buy && l.url.starts_with("https://www.qobuz.com/")), "Qobuz's store, not its player");
        assert_eq!(row(LinkUse::Discover), ["last.fm", "Rate Your Music", "website"]);
        // The first of a site's links is kept.
        assert!(a.links.iter().any(|l| l.kind == LinkUse::Listen && l.url.contains("/gb/")));
    }

    fn lt(idx: usize, title: &str, track: Option<u32>) -> LocalTrack {
        LocalTrack { idx, title: title.into(), artist: "radiohead".into(), album_artist: String::new(), album: "Ok Computer".into(), track, disc: None, year: None, seconds: 0.0 }
    }

    fn release(id: &str, title: &str, artist: &str, tracks: usize, score: u64) -> Value {
        serde_json::json!({ "id": id, "title": title, "score": score, "track-count": tracks, "status": "Official",
            "artist-credit": [{ "name": artist, "joinphrase": "", "artist": { "name": artist } }] })
    }

    #[test]
    fn ranking_rejects_other_artists_and_titles() {
        // What `release:"Love" AND artist:"Unknown Artist"` returned: none of these are the album.
        let unknown = serde_json::json!({ "releases": [
            release("a", "Faithless Love", "Unknown Artist", 1, 100),
            release("b", "Im Sorry, I love You EP", "Unknown Artist", 4, 91),
        ]});
        assert!(rank_releases(&unknown, Some("The Beatles"), "Love", 4).is_empty());
        assert_eq!(known_artist("Unknown Artist"), None);
        assert_eq!(known_artist(""), None);

        // The exact title wins over a closer track count.
        let beatles = serde_json::json!({ "releases": [
            release("songs", "Love Songs", "The Beatles", 4, 100),
            release("love", "Love", "The Beatles", 26, 95),
            release("tribute", "Love", "The Beatles Tribute Band", 4, 100),
        ]});
        assert_eq!(rank_releases(&beatles, Some("Beatles"), "Love", 4), ["love", "songs"]);
        // Without an artist, titles still have to agree.
        assert_eq!(rank_releases(&beatles, None, "Love", 4).len(), 3);
        assert!(rank_releases(&beatles, None, "Faithless", 4).is_empty());
    }

    fn remote(titles: &[(&str, f64)]) -> Release {
        let tracks = titles
            .iter()
            .enumerate()
            .map(|(i, (t, s))| RemoteTrack { disc: 1, number: i as u32 + 1, title: t.to_string(), artist: "Joy Division".into(), seconds: Some(*s) })
            .collect();
        Release { id: String::new(), group_id: String::new(), title: "Unknown Pleasures".into(), artist: "Joy Division".into(), year: Some(1979), country: String::new(), label: String::new(), tracks, multi_disc: false }
    }

    fn local(titles: &[(&str, f64)]) -> Vec<LocalTrack> {
        titles
            .iter()
            .enumerate()
            .map(|(i, (t, s))| LocalTrack { seconds: *s, artist: "Joy Division".into(), ..lt(i, t, Some(i as u32 + 1)) })
            .collect()
    }

    #[test]
    fn wrong_release_gets_no_suggestions() {
        let right = remote(&[("Disorder", 209.0), ("Day of the Lords", 288.0), ("Candidate", 185.0)]);
        let wrong = remote(&[("Atmosphere", 250.0), ("Dead Souls", 296.0), ("Glass", 237.0)]);

        // Band name and track number in the title still match; a missing title goes by position.
        let files = local(&[("01 Joy Division - Disorder", 209.5), ("Day Of The Lords", 288.0), ("Track 3", 185.0)]);
        let (changes, matched) = compare(&right, &files).unwrap();
        assert_eq!(matched, 3);
        assert!(changes.iter().any(|c| c.track == 2 && c.field == Field::Title && c.new == "Candidate"));

        // Before: track numbers handed the wrong album's titles out. Now: nothing.
        assert!(compare(&wrong, &files).is_none());
        // Unless every length lines up, which identifies the album without titles.
        let untitled = local(&[("Track 1", 209.0), ("Track 2", 287.0), ("Track 3", 186.0)]);
        assert!(compare(&right, &untitled).is_some());
        assert!(compare(&wrong, &untitled).is_none());
    }

    /// Hits the live API: `cargo test -- --ignored musicbrainz`
    #[test]
    #[ignore]
    fn musicbrainz_album_and_artist() {
        let mut w = Worker::new(format!("NeatLibrary-tests/{}", env!("CARGO_PKG_VERSION")));
        let (label, changes) = w.album("Radiohead", "OK Computer", &[lt(0, "Airbag", Some(1)), lt(1, "Karma Police", None), lt(2, "paranoid android", Some(2))]).unwrap();
        println!("{label}");
        for c in &changes {
            println!("  #{} {:?}: {:?} -> {:?}", c.track, c.field, c.old, c.new);
        }
        assert!(changes.iter().any(|c| c.track == 1 && c.field == Field::Track && c.new == "6"));
        assert!(changes.iter().any(|c| c.field == Field::Year && c.new == "1997"));
        let beatles = |t: &str, n| LocalTrack { artist: "The Beatles".into(), ..lt(n as usize, t, Some(n)) };
        let love = [beatles("Because", 1), beatles("Get Back", 2), beatles("Glass Onion", 3)];
        let (label, _) = w.album("The Beatles", "The Beatles - Love (2006) [Dolby Atmos] {Calderstone Productions Limited}", &love).unwrap();
        println!("{label}");
        assert!(label.starts_with("Love — The Beatles"));
        let unknown = [lt(0, "Track 1", Some(1)), lt(1, "Track 2", Some(2))];
        let err = w.album("Unknown Artist", "Love", &unknown).unwrap_err();
        println!("{err}");

        let hits = w.artist("Guns and Roses").unwrap();
        println!("{:?}", hits.iter().map(|h| (&h.name, h.score)).collect::<Vec<_>>());
        assert!(hits.iter().any(|h| h.name == "Guns N’ Roses" || h.name == "Guns N' Roses"));
    }

    /// Hits the live API and the Cover Art Archive: `cargo test -- --ignored musicbrainz_cover`
    #[test]
    #[ignore]
    fn musicbrainz_cover() {
        let job = CoverJob { artist: "Radiohead".into(), album: "OK Computer".into(), tracks: vec![lt(0, "Airbag", Some(1)), lt(1, "Karma Police", Some(6)), lt(2, "Paranoid Android", Some(2))] };
        let mut got = None;
        find_covers(format!("NeatLibrary-tests/{}", env!("CARGO_PKG_VERSION")), &[job], &AtomicBool::new(false), |i, r| got = Some((i, r)));
        let (i, r) = got.expect("one reply");
        let (bytes, label) = r.unwrap();
        println!("{label}: {} bytes", bytes.len());
        assert_eq!(i, 0);
        assert!(image::load_from_memory(&bytes).is_ok_and(|img| img.width() >= 250), "a real image");
    }
}
