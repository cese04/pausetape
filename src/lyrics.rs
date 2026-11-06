//! Lyrics: a song's own (a `.lrc` file beside it, or its tags), else LRCLIB's (lrclib.net, an
//! open database matched by artist, title, album and length). Synced lyrics come as LRC lines,
//! "[01:23.45] words", so the player can follow them. Requests go one at a time, a little apart,
//! as LRCLIB asks, and say who's asking.

use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use lofty::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const API: &str = "https://lrclib.net/api";
/// Between two requests, as LRCLIB asks of clients that send them one after another.
const PAUSE: Duration = Duration::from_millis(400);

/// Where a song's lyrics came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// A `.lrc` file beside the song.
    File,
    /// The song's own tags.
    Tags,
    Lrclib,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lyrics {
    /// Each line with when it's sung, in order; empty when they aren't synced.
    pub synced: Vec<(Duration, String)>,
    /// The words without times (for unsynced lyrics; else made from `synced`).
    pub plain: String,
    pub instrumental: bool,
    pub source: Option<Source>,
}

impl Lyrics {
    /// From LRC text, or plain text when it has no times.
    pub fn from_text(text: &str, source: Source) -> Option<Lyrics> {
        let synced = parse_lrc(text);
        let plain = if synced.is_empty() { text.trim().to_string() } else { synced.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n") };
        (!plain.is_empty()).then_some(Lyrics { synced, plain, instrumental: false, source: Some(source) })
    }

    /// Synced lyrics as the song goes, in a song `length` long: each line, and the breaks where
    /// nothing is sung (before the first line, a solo, after the last), so a song that's in time
    /// shows it before its first words. A break is where an empty line says one starts, or, a
    /// while into a line, when the next is long in coming. Breaks too short to matter are left out.
    pub fn stretches(&self, length: Duration) -> Vec<Stretch> {
        let mut all = vec![Stretch { at: Duration::ZERO, words: None }];
        for (i, (at, words)) in self.synced.iter().enumerate() {
            if words.is_empty() {
                all.push(Stretch { at: *at, words: None });
                continue;
            }
            all.push(Stretch { at: *at, words: Some(words.clone()) });
            let next = self.synced.get(i + 1);
            let until = next.map_or(length, |n| n.0);
            if until > *at + BREAK_AFTER && !next.is_some_and(|n| n.1.is_empty()) {
                all.push(Stretch { at: *at + SUNG_FOR, words: None });
            }
        }
        // One break for breaks in a row, the first; then those long enough.
        all.dedup_by(|b, a| a.words.is_none() && b.words.is_none());
        let ends: Vec<Duration> = all.iter().skip(1).map(|s| s.at).chain([length]).collect();
        all.into_iter().zip(ends).filter(|(s, end)| s.words.is_some() || end.saturating_sub(s.at) >= SHORTEST_BREAK).map(|(s, _)| s).collect()
    }
}

/// Nothing new sung for this long after a line starts: a break is shown (a slow verse, its lines
/// long drawn out, isn't one).
const BREAK_AFTER: Duration = Duration::from_secs(15);
/// How long a line is taken to be sung for, when nothing marks where it ends.
const SUNG_FOR: Duration = Duration::from_secs(8);
/// Breaks shorter than this aren't shown: a breath between lines.
const SHORTEST_BREAK: Duration = Duration::from_secs(4);
/// A break this long is told in words (how long, and how long left); a shorter one is its dots.
pub const LONG_BREAK: Duration = Duration::from_secs(60);

/// A line sung from `at`, or a break where none is (`words` None); it lasts until the next.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretch {
    pub at: Duration,
    pub words: Option<String>,
}

/// The stretch under way at `pos` (the last that has started).
pub fn stretch_at(stretches: &[Stretch], pos: Duration) -> Option<usize> {
    stretches.iter().rposition(|s| s.at <= pos)
}

/// LRC lines into (time, words), sorted by time: "[01:23.45]" or "[1:23]" or "[01:23:45]"; a
/// line may carry several times (a chorus). Tags such as "[ar:Artist]" and "[offset:+200]"
/// (milliseconds, earlier when positive) are understood or skipped.
pub fn parse_lrc(text: &str) -> Vec<(Duration, String)> {
    let mut offset_ms: i64 = 0;
    let mut out: Vec<(i64, String)> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(end) = body.find(']') else { break };
            let tag = &body[..end];
            match time_of(tag) {
                Some(ms) => times.push(ms),
                None => {
                    if let Some(v) = tag.strip_prefix("offset:") {
                        offset_ms = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            rest = body[end + 1..].trim_start();
        }
        for ms in times {
            out.push((ms - offset_ms, rest.trim().to_string()));
        }
    }
    out.sort_by_key(|(ms, _)| *ms);
    out.into_iter().map(|(ms, l)| (Duration::from_millis(ms.max(0) as u64), l)).collect()
}

/// "mm:ss", "mm:ss.xx", "mm:ss.xxx" or "mm:ss:xx", in milliseconds.
fn time_of(tag: &str) -> Option<i64> {
    let (min, rest) = tag.split_once(':')?;
    let min: i64 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((s, f)) => (s, f),
        None => (rest, ""),
    };
    let sec: i64 = sec.parse().ok()?;
    if !(0..60).contains(&sec) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => frac[..3].parse().ok()?,
    };
    Some((min * 60 + sec) * 1000 + frac_ms)
}

/// The song's own lyrics: a `.lrc` with its name beside it, else its tags'. None reads the
/// network.
pub fn local(path: &Path) -> Option<Lyrics> {
    let lrc = path.with_extension("lrc");
    if let Ok(bytes) = std::fs::read(&lrc)
        && let Some(l) = Lyrics::from_text(&String::from_utf8_lossy(&bytes), Source::File)
    {
        return Some(l);
    }
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let text = tag.get_string(ItemKey::Lyrics).or_else(|| tag.get_string(ItemKey::UnsyncLyrics))?;
    Lyrics::from_text(text, Source::Tags)
}

/// What to look for on LRCLIB.
pub struct Query {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub seconds: u64,
}

/// LRCLIB's lyrics for a song: the record for its artist, title, album and length (±2 s);
/// else, searching artist and title, the closest in length within 3 s. `Ok(None)`: none there.
pub fn fetch(agent: &ureq::Agent, user_agent: &str, q: &Query) -> Result<Option<Lyrics>> {
    let secs = q.seconds.to_string();
    let mut params = vec![("track_name", q.title.as_str()), ("artist_name", q.artist.as_str())];
    if !q.album.is_empty() {
        params.push(("album_name", q.album.as_str()));
    }
    if q.seconds > 0 {
        params.push(("duration", secs.as_str()));
    }
    if let Some(v) = get(agent, user_agent, "get", &params)? {
        return Ok(Some(from_record(&v)));
    }
    thread::sleep(PAUSE);
    let found = get(agent, user_agent, "search", &[("track_name", q.title.as_str()), ("artist_name", q.artist.as_str())])?;
    let best = found.as_ref().and_then(Value::as_array).into_iter().flatten()
        .filter(|r| r["syncedLyrics"].is_string() || r["plainLyrics"].is_string() || r["instrumental"].as_bool() == Some(true))
        .map(|r| (r, (r["duration"].as_f64().unwrap_or(0.0) - q.seconds as f64).abs()))
        .filter(|(_, off)| q.seconds == 0 || *off <= 3.0)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    Ok(best.map(|(r, _)| from_record(r)))
}

fn from_record(v: &Value) -> Lyrics {
    let instrumental = v["instrumental"].as_bool().unwrap_or(false);
    let text = v["syncedLyrics"].as_str().or(v["plainLyrics"].as_str()).unwrap_or_default();
    match Lyrics::from_text(text, Source::Lrclib) {
        Some(l) => Lyrics { instrumental, ..l },
        None => Lyrics { instrumental, source: Some(Source::Lrclib), ..Default::default() },
    }
}

/// One request; `Ok(None)` for "not found". Rate-limited, it waits as told and tries again.
fn get(agent: &ureq::Agent, user_agent: &str, endpoint: &str, params: &[(&str, &str)]) -> Result<Option<Value>> {
    for attempt in 0..3 {
        let mut req = agent.get(format!("{API}/{endpoint}")).header("User-Agent", user_agent);
        for (k, v) in params {
            req = req.query(*k, *v);
        }
        match req.call() {
            Ok(mut resp) => return resp.body_mut().read_json::<Value>().map(Some).context("decoding LRCLIB's reply"),
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(ureq::Error::StatusCode(429)) if attempt < 2 => thread::sleep(Duration::from_secs(10 << attempt)),
            Err(e) => return Err(e).context("asking LRCLIB"),
        }
    }
    Err(anyhow!("LRCLIB is busy; try again in a minute"))
}

/// The agent for LRCLIB: a timeout, and that's all.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into()
}

pub fn pause() {
    thread::sleep(PAUSE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_come_with_their_times() {
        let l = parse_lrc("[ar:Air]\n[00:12.30]First\n[00:05]Intro\n[01:02.345][02:10.00]Chorus\n[bad]x\n");
        let got: Vec<(u64, &str)> = l.iter().map(|(t, s)| (t.as_millis() as u64, s.as_str())).collect();
        assert_eq!(got, [(5000, "Intro"), (12300, "First"), (62345, "Chorus"), (130000, "Chorus")]);
    }

    #[test]
    fn an_offset_moves_every_line() {
        let l = parse_lrc("[offset:+500]\n[00:10.00]Line");
        assert_eq!(l[0].0, Duration::from_millis(9500));
    }

    #[test]
    fn plain_text_is_kept_as_it_is() {
        let l = Lyrics::from_text("No times here\nat all", Source::Tags).unwrap();
        assert!(l.synced.is_empty());
        assert_eq!(l.plain, "No times here\nat all");
        assert!(Lyrics::from_text("  \n", Source::Tags).is_none());
    }

    #[test]
    fn the_line_being_sung() {
        let l = Lyrics::from_text("[00:01]a\n[00:05]b\n[00:09]c", Source::Lrclib).unwrap();
        let s = l.stretches(Duration::from_secs(12));
        assert_eq!(s.len(), 3, "no breaks: the first line comes at once, the song ends soon after");
        assert_eq!(stretch_at(&s, Duration::from_millis(500)), None);
        assert_eq!(stretch_at(&s, Duration::from_secs(5)), Some(1));
        assert_eq!(stretch_at(&s, Duration::from_secs(60)), Some(2));
    }

    #[test]
    fn breaks_where_nothing_is_sung() {
        // An intro, a line long before the next, an empty line ending one, a short gap, an outro.
        let l = Lyrics::from_text("[00:20]a\n[00:22]b\n[00:50]c\n[00:53]\n[01:10]d\n[01:12]\n[01:14]e", Source::Lrclib).unwrap();
        let s = l.stretches(Duration::from_secs(120));
        let got: Vec<(u64, Option<&str>)> = s.iter().map(|s| (s.at.as_secs(), s.words.as_deref())).collect();
        assert_eq!(got, [(0, None), (20, Some("a")), (22, Some("b")), (30, None), (50, Some("c")), (53, None), (70, Some("d")), (74, Some("e")), (82, None)]);
        assert_eq!(stretch_at(&s, Duration::from_secs(10)), Some(0), "the intro is under way");
    }

    #[test]
    fn records_read_as_lyrics() {
        let v = serde_json::json!({ "instrumental": false, "plainLyrics": "a\nb", "syncedLyrics": "[00:01.00]a\n[00:02.00]b" });
        let l = from_record(&v);
        assert_eq!((l.synced.len(), l.plain.as_str(), l.source), (2, "a\nb", Some(Source::Lrclib)));
        let i = from_record(&serde_json::json!({ "instrumental": true, "plainLyrics": null, "syncedLyrics": null }));
        assert!(i.instrumental && i.plain.is_empty());
    }

    /// `cargo test -- --ignored lrclib --nocapture` (asks lrclib.net)
    #[test]
    #[ignore]
    fn lrclib() {
        let q = Query { artist: "Radiohead".into(), title: "Karma Police".into(), album: "OK Computer".into(), seconds: 264 };
        let l = fetch(&agent(), "Pausetape tests (https://github.com/cese04/pausetape)", &q).unwrap().expect("found");
        assert!(!l.synced.is_empty(), "synced lyrics for Karma Police");
    }
}
