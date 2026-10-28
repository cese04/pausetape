//! MusicBrainz lookups on a single worker thread (their API allows ~1 request/second).
//!
//! Album lookups compare a whole local album against a release and emit per-field changes;
//! nothing is applied until the user accepts it.

use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, unbounded};
use serde_json::Value;

use crate::library::names::title_key;

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

    fn album(&mut self, artist: &str, album: &str, local: &[LocalTrack]) -> Result<(String, Vec<FieldChange>)> {
        let query = format!("release:\"{}\" AND artist:\"{}\"", lucene(album), lucene(artist));
        let v = self.get("release", &[("query", &query), ("limit", "10")])?;
        let mut candidates: Vec<&Value> = v["releases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["score"].as_u64().unwrap_or(0) >= 80)
            .collect();
        // Prefer releases with the same number of tracks as we have, then official ones.
        candidates.sort_by_key(|r| {
            let n = r["track-count"].as_u64().unwrap_or(0) as usize;
            (n.abs_diff(local.len()), r["status"].as_str() != Some("Official"), u64::MAX - r["score"].as_u64().unwrap_or(0))
        });
        let best = candidates.first().ok_or_else(|| anyhow!("no confident match for “{album}” by {artist}"))?;
        let id = str_of(&best["id"]);
        let rel = self.get(&format!("release/{id}"), &[("inc", "recordings artist-credits release-groups")])?;

        let album_title = str_of(&rel["title"]);
        let album_artist = credit(&rel["artist-credit"]);
        let year = year_of(&rel["release-group"]["first-release-date"]).or_else(|| year_of(&rel["date"]));
        let mut remote = Vec::new();
        for medium in rel["media"].as_array().into_iter().flatten() {
            let disc = medium["position"].as_u64().unwrap_or(1) as u32;
            for t in medium["tracks"].as_array().into_iter().flatten() {
                remote.push(RemoteTrack {
                    disc,
                    number: t["position"].as_u64().unwrap_or(0) as u32,
                    title: str_of(&t["title"]),
                    artist: credit(&t["artist-credit"]),
                    seconds: t["length"].as_f64().map(|ms| ms / 1000.0),
                });
            }
        }
        let multi_disc = rel["media"].as_array().is_some_and(|m| m.len() > 1);

        let mut changes = Vec::new();
        let mut used = vec![false; remote.len()];
        for lt in local {
            let Some(ri) = match_track(lt, &remote, &used) else { continue };
            used[ri] = true;
            let rt = &remote[ri];
            push_change(&mut changes, lt.idx, Field::Title, &lt.title, &rt.title);
            push_change(&mut changes, lt.idx, Field::Artist, &lt.artist, &rt.artist);
            push_change(&mut changes, lt.idx, Field::AlbumArtist, &lt.album_artist, &album_artist);
            push_change(&mut changes, lt.idx, Field::Album, &lt.album, &album_title);
            push_change(&mut changes, lt.idx, Field::Track, &opt(lt.track), &rt.number.to_string());
            if multi_disc || lt.disc.is_some_and(|d| d != rt.disc) {
                push_change(&mut changes, lt.idx, Field::Disc, &opt(lt.disc), &rt.disc.to_string());
            }
            if let Some(y) = year {
                push_change(&mut changes, lt.idx, Field::Year, &opt(lt.year), &y.to_string());
            }
        }
        let matched = used.iter().filter(|u| **u).count();
        let label = format!(
            "{album_title} — {album_artist}{} · {}/{} tracks matched",
            year.map(|y| format!(", {y}")).unwrap_or_default(),
            matched,
            local.len()
        );
        Ok((label, changes))
    }

    /// Tracks without an album: look each recording up and suggest its earliest album.
    fn loose(&mut self, local: &[LocalTrack]) -> Result<(String, Vec<FieldChange>)> {
        let mut changes = Vec::new();
        let mut found = 0;
        for lt in local {
            let query = format!("recording:\"{}\" AND artist:\"{}\"", lucene(&lt.title), lucene(&lt.artist));
            let v = self.get("recording", &[("query", &query), ("limit", "5")])?;
            let Some(rec) = v["recordings"].as_array().into_iter().flatten().find(|r| r["score"].as_u64().unwrap_or(0) >= 90) else {
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

struct RemoteTrack {
    disc: u32,
    number: u32,
    title: String,
    artist: String,
    seconds: Option<f64>,
}

/// Match by title first, then by disc/track position, then by length.
fn match_track(lt: &LocalTrack, remote: &[RemoteTrack], used: &[bool]) -> Option<usize> {
    let free = || remote.iter().enumerate().filter(|(i, _)| !used[*i]);
    let lk = title_key(&lt.title);
    if let Some((i, score)) = free()
        .map(|(i, r)| (i, strsim::jaro_winkler(&lk, &title_key(&r.title))))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        && score >= 0.88
    {
        return Some(i);
    }
    if let Some(n) = lt.track
        && let Some((i, _)) = free().find(|(_, r)| r.number == n && r.disc == lt.disc.unwrap_or(1))
    {
        return Some(i);
    }
    let mut near = free().filter(|(_, r)| r.seconds.is_some_and(|s| (s - lt.seconds).abs() < 2.0));
    match (near.next(), near.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
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
        let hits = w.artist("Guns and Roses").unwrap();
        println!("{:?}", hits.iter().map(|h| (&h.name, h.score)).collect::<Vec<_>>());
        assert!(hits.iter().any(|h| h.name == "Guns N’ Roses" || h.name == "Guns N' Roses"));
    }
}
