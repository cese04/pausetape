//! MusicBrainz lookups on a single worker thread (their API allows ~1 request/second).
//!
//! Album lookups compare a whole local album against a release and emit per-field changes;
//! nothing is applied until the user accepts it.

use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, unbounded};
use deunicode::deunicode;
use serde_json::Value;

use crate::library::grouping::UNKNOWN;
use crate::library::names::{artist_key, clean_album, clean_title, primary_artist, similar_keys, title_key};

const API: &str = "https://musicbrainz.org/ws/2";

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
    pub track: usize,
    pub field: Field,
    pub old: String,
    pub new: String,
    pub accept: bool,
}

#[derive(Clone, Debug)]
pub struct LocalTrack {
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
    pub name: String,
    pub detail: String,
    pub score: u32,
}

pub enum Job {
    Artist { cluster_key: String, name: String },
    Album { album_id: String, artist: String, album: String, tracks: Vec<LocalTrack> },
    Loose { album_id: String, tracks: Vec<LocalTrack> },
}

pub enum Reply {
    Artist { cluster_key: String, result: Result<Vec<ArtistHit>, String> },
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

struct Worker {
    agent: ureq::Agent,
    user_agent: String,
    last: Option<Instant>,
}

impl Worker {
    fn new(user_agent: String) -> Self {
        let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
        Worker { agent, user_agent, last: None }
    }

    fn run(mut self, jobs: Receiver<Job>, replies: Sender<Reply>, wake: impl Fn()) {
        for job in jobs {
            let reply = match job {
                Job::Artist { cluster_key, name } => {
                    Reply::Artist { cluster_key, result: self.artist(&name).map_err(|e| format!("{e:#}")) }
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
            if let Some(last) = self.last {
                let wait = Duration::from_millis(1100).saturating_sub(last.elapsed());
                thread::sleep(wait);
            }
            self.last = Some(Instant::now());
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

    /// Search for the release, check the best candidates against the local songs, and suggest
    /// changes from the first one that is clearly the same album.
    fn album(&mut self, artist: &str, album: &str, local: &[LocalTrack]) -> Result<(String, Vec<FieldChange>)> {
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
                    Some((changes, matched)) => return Ok((release.label(matched, local.len()), changes)),
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
        Release { title: "Unknown Pleasures".into(), artist: "Joy Division".into(), year: Some(1979), country: String::new(), label: String::new(), tracks, multi_disc: false }
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
}
