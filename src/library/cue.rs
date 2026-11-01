//! CUE sheets: the track list of an album stored as one audio file (an "image").
//!
//! A sheet names its audio file(s) and, per track, a title, a performer and where the track
//! starts (`INDEX 01 mm:ss:ff`, in CD frames of 1/75 s). Players like foobar2000 show each track
//! as a song of its own; the scanner does the same with `expand`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::names::clean_album;
use super::track::Track;

/// CD frames per second, the unit of CUE times.
pub const FRAMES_PER_SEC: u64 = 75;

/// Where a song lies inside an album file: CD frames from the start of the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CueSpan {
    /// The track number in the sheet.
    pub number: u32,
    pub start: u64,
    /// `None` for the last track: it runs to the end of the file.
    pub end: Option<u64>,
    pub sheet: PathBuf,
}

impl CueSpan {
    pub fn start_time(&self) -> Duration {
        frames_to_time(self.start)
    }

    pub fn end_time(&self) -> Option<Duration> {
        self.end.map(frames_to_time)
    }
}

pub fn frames_to_time(frames: u64) -> Duration {
    Duration::from_nanos(frames * 1_000_000_000 / FRAMES_PER_SEC)
}

#[derive(Debug, Default)]
pub struct Sheet {
    pub performer: Option<String>,
    pub title: Option<String>,
    pub year: Option<u16>,
    pub disc: Option<u32>,
    pub files: Vec<SheetFile>,
}

#[derive(Debug, Default)]
pub struct SheetFile {
    /// As written in the sheet; see `resolve`.
    pub name: String,
    pub tracks: Vec<SheetTrack>,
}

#[derive(Debug, Default, Clone)]
pub struct SheetTrack {
    pub number: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
    /// `INDEX 01`, in CD frames from the start of its file.
    pub start: u64,
}

/// Decode a sheet: UTF-8 (with or without BOM), UTF-16 with BOM, or a legacy code page guessed
/// from the bytes (rips often use Windows-1251 for Cyrillic, Windows-1252 for Western names).
pub fn decode(bytes: &[u8]) -> String {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc.decode_without_bom_handling(&bytes[bom..]).0.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    det.guess(None, chardetng::Utf8Detection::Allow).decode_without_bom_handling(bytes).0.into_owned()
}

pub fn parse(text: &str) -> Sheet {
    let mut sheet = Sheet::default();
    let mut tracks: Vec<(SheetTrack, Option<usize>)> = Vec::new();
    let mut file: Option<usize> = None;
    for line in text.lines() {
        let line = line.trim();
        let (cmd, rest) = line.split_once(char::is_whitespace).map(|(c, r)| (c, r.trim())).unwrap_or((line, ""));
        match cmd.to_ascii_uppercase().as_str() {
            "FILE" => {
                sheet.files.push(SheetFile { name: file_name(rest), tracks: Vec::new() });
                file = Some(sheet.files.len() - 1);
            }
            "TRACK" => {
                let number = rest.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(tracks.len() as u32 + 1);
                let audio = rest.to_ascii_uppercase().contains("AUDIO");
                // Data tracks never get an INDEX 01 under an audio file; mark them as placed.
                tracks.push((SheetTrack { number, ..Default::default() }, (!audio).then_some(usize::MAX)));
            }
            // INDEX 01 is where the song starts. In per-track rips the TRACK line can come before
            // the FILE line of its own file, so a track belongs to the file current at INDEX 01.
            "INDEX" => {
                let mut parts = rest.split_whitespace();
                if parts.next().and_then(|n| n.parse::<u32>().ok()) == Some(1)
                    && let (Some(t), Some(f)) = (tracks.last_mut(), file)
                    && t.1.is_none()
                {
                    t.0.start = parts.next().and_then(msf).unwrap_or(0);
                    t.1 = Some(f);
                }
            }
            "TITLE" | "PERFORMER" => {
                let v = Some(value(rest)).filter(|v| !v.is_empty());
                let upper = cmd.to_ascii_uppercase();
                match (tracks.last_mut(), upper.as_str()) {
                    (Some(t), "TITLE") => t.0.title = v,
                    (Some(t), _) => t.0.performer = v,
                    (None, "TITLE") => sheet.title = v,
                    (None, _) => sheet.performer = v,
                }
            }
            "REM" => {
                let (key, v) = rest.split_once(char::is_whitespace).map(|(k, v)| (k, value(v))).unwrap_or((rest, String::new()));
                match key.to_ascii_uppercase().as_str() {
                    "DATE" => sheet.year = v.get(..4).and_then(|y| y.parse().ok()).filter(|y| (1900..=2099).contains(y)),
                    "DISCNUMBER" => sheet.disc = v.split('/').next().and_then(|d| d.trim().parse().ok()),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for (t, f) in tracks {
        if let Some(f) = f.filter(|&f| f != usize::MAX) {
            sheet.files[f].tracks.push(t);
        }
    }
    sheet
}

/// `"name with spaces.flac" WAVE` or `name.flac WAVE` → the file name.
fn file_name(rest: &str) -> String {
    if rest.starts_with('"') {
        return value(rest);
    }
    match rest.rsplit_once(char::is_whitespace) {
        Some((name, _kind)) => name.trim().to_string(),
        None => rest.to_string(),
    }
}

/// A quoted value (up to the closing quote) or the rest of the line.
fn value(rest: &str) -> String {
    match rest.strip_prefix('"') {
        Some(r) => r.split_once('"').map(|(v, _)| v).unwrap_or(r).trim().to_string(),
        None => rest.trim().to_string(),
    }
}

/// "03:31:73" → CD frames.
fn msf(s: &str) -> Option<u64> {
    let mut it = s.split(':').map(|p| p.trim().parse::<u64>().ok());
    let (m, sec, f) = (it.next()??, it.next()??, it.next()??);
    Some((m * 60 + sec) * FRAMES_PER_SEC + f)
}

/// Find a sheet's file among the audio files next to it: exact name, else ignoring case
/// (sheets often say `.FLAC`, or were written on Windows), else the same name with another
/// audio extension (the rip was `.wav`, later converted to `.flac`).
pub fn resolve<'a>(name: &str, candidates: &'a [PathBuf]) -> Option<&'a PathBuf> {
    let name = name.replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
    let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    candidates
        .iter()
        .find(|p| lower(p) == name)
        .or_else(|| candidates.iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == stem)))
}

/// Replace album files described by a CUE sheet with one track per song. Only files the sheet
/// splits into two or more tracks are cut; a file holding one song (a per-track rip, or the
/// last side of an LP) keeps its own tags, with the sheet filling only what they leave out.
/// Each file is claimed by the first sheet (by path) that resolves it. Sheets come with their
/// contents, read during a scan or stored in the library database.
pub fn expand(tracks: Vec<Track>, sheets: &[(PathBuf, Vec<u8>)]) -> Vec<Track> {
    let mut by_dir: HashMap<&Path, Vec<PathBuf>> = HashMap::new();
    for t in &tracks {
        if let Some(dir) = t.path.parent() {
            by_dir.entry(dir).or_default().push(t.path.clone());
        }
    }
    let mut songs: HashMap<PathBuf, Vec<Track>> = HashMap::new();
    let mut fill: Vec<(PathBuf, Track)> = Vec::new();
    let mut sheets: Vec<&(PathBuf, Vec<u8>)> = sheets.iter().collect();
    sheets.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, bytes) in sheets {
        let Some(candidates) = path.parent().and_then(|d| by_dir.get(d)) else { continue };
        let sheet = parse(&decode(bytes));
        for f in &sheet.files {
            let Some(audio) = resolve(&f.name, candidates) else { continue };
            if songs.contains_key(audio) || fill.iter().any(|(p, _)| p == audio) {
                continue;
            }
            let Some(image) = tracks.iter().find(|t| &t.path == audio) else { continue };
            match f.tracks.len() {
                0 => {}
                1 => fill.push((audio.clone(), single(image, &sheet, &f.tracks[0]))),
                _ => {
                    songs.insert(audio.clone(), split(image, &sheet, f, path));
                }
            }
        }
    }
    let mut fill: HashMap<PathBuf, Track> = fill.into_iter().collect();
    let mut out = Vec::with_capacity(tracks.len());
    for t in tracks {
        match (songs.remove(&t.path), fill.remove(&t.path)) {
            (Some(s), _) => out.extend(s),
            (None, Some(f)) => out.push(f),
            (None, None) => out.push(t),
        }
    }
    out
}

/// A file holding one song: its tags stay, and the sheet fills the gaps.
fn single(file: &Track, sheet: &Sheet, st: &SheetTrack) -> Track {
    let mut t = file.clone();
    t.title = t.title.or(st.title.clone());
    t.track = t.track.or(Some(st.number));
    t.artist = t.artist.or(st.performer.clone()).or(sheet.performer.clone());
    t.album_artist = t.album_artist.or(sheet.performer.clone());
    t.album = t.album.or(sheet.title.as_deref().map(|a| clean_album(a, sheet.performer.as_deref()).album));
    t.year = t.year.or(sheet.year);
    t.disc = t.disc.or(sheet.disc);
    t
}

/// The songs of one album file. The sheet's names win over the file's own tags, which in album
/// files are often a single song's tags; those only fill what the sheet leaves out.
fn split(image: &Track, sheet: &Sheet, file: &SheetFile, sheet_path: &Path) -> Vec<Track> {
    let total = (image.duration.as_nanos() as u64 * FRAMES_PER_SEC / 1_000_000_000).max(1);
    let album = sheet.title.as_deref().map(|t| clean_album(t, sheet.performer.as_deref()).album).or(image.album.clone());
    let mut seen = HashSet::new();
    let tracks: Vec<&SheetTrack> = file.tracks.iter().filter(|t| seen.insert(t.number)).collect();
    tracks
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let end = tracks.get(i + 1).map(|n| n.start);
            let frames = end.unwrap_or(total).saturating_sub(st.start);
            Track {
                path: image.path.clone(),
                ext: image.ext.clone(),
                codec: image.codec.clone(),
                artist: st.performer.clone().or(sheet.performer.clone()).or(image.artist.clone()),
                album_artist: sheet.performer.clone().or(image.album_artist.clone()),
                album: album.clone(),
                title: Some(st.title.clone().unwrap_or_else(|| format!("Track {}", st.number))),
                track: Some(st.number),
                disc: sheet.disc.or(image.disc),
                year: sheet.year.or(image.year),
                compilation: image.compilation,
                duration: frames_to_time(frames),
                kbps: image.kbps,
                sample_rate: image.sample_rate,
                bit_depth: image.bit_depth,
                // An estimate for space totals: the file's size shared out by length.
                size: image.size * frames / total,
                mtime: image.mtime,
                cue: Some(CueSpan { number: st.number, start: st.start, end, sheet: sheet_path.to_path_buf() }),
                cover: None,
                embedded_cover: image.embedded_cover,
                damaged: image.damaged,
                dirty: false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "REM GENRE Post-Punk\r\nREM DATE 1979\r\nPERFORMER \"JOY DIVISION\"\r\nTITLE \"Unknown Pleasures\"\r\n\
        FILE \"1979 - Unknown Pleasures.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    TITLE \"Disorder\"\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:00:32\r\n\
        \x20 TRACK 02 AUDIO\r\n    TITLE \"Day Of The Lords\"\r\n    INDEX 00 03:28:35\r\n    INDEX 01 03:31:73\r\n\
        \x20 TRACK 03 AUDIO\r\n    TITLE \"Candidate\"\r\n    PERFORMER \"Someone Else\"\r\n    INDEX 01 08:19:05\r\n";

    #[test]
    fn one_file_album() {
        let s = parse(IMAGE);
        assert_eq!((s.performer.as_deref(), s.title.as_deref(), s.year), (Some("JOY DIVISION"), Some("Unknown Pleasures"), Some(1979)));
        assert_eq!(s.files.len(), 1);
        let t = &s.files[0].tracks;
        assert_eq!(t.len(), 3);
        assert_eq!((t[1].number, t[1].title.as_deref(), t[1].start), (2, Some("Day Of The Lords"), (3 * 60 + 31) * 75 + 73));
        assert_eq!(t[2].performer.as_deref(), Some("Someone Else"));
    }

    #[test]
    fn per_track_rip_is_not_split() {
        // EAC "gaps appended": a TRACK and its pregap come before the FILE line of its own file.
        let s = parse(
            "FILE \"01 - Disorder.wav\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 00 03:28:35\n\
             FILE \"02 - Day Of The Lords.wav\" WAVE\n  INDEX 01 00:00:00\n",
        );
        assert_eq!(s.files.iter().map(|f| f.tracks.len()).collect::<Vec<_>>(), [1, 1]);
    }

    #[test]
    fn several_files_each_split() {
        let s = parse(
            "FILE \"Side 1.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 01 05:00:00\n\
             FILE \"Side 2.flac\" WAVE\n TRACK 03 AUDIO\n  INDEX 01 00:00:00\n",
        );
        assert_eq!(s.files.iter().map(|f| f.tracks.len()).collect::<Vec<_>>(), [2, 1]);
    }

    #[test]
    fn legacy_encodings() {
        // "Ночь" in Windows-1251.
        let bytes = b"PERFORMER \"\xcd\xee\xf7\xfc \xcd\xee\xf7\xfc\"\nTITLE \"\xcd\xee\xf7\xfc\"\n";
        assert_eq!(parse(&decode(bytes)).title.as_deref(), Some("Ночь"));
        assert_eq!(decode(b"\xef\xbb\xbfTITLE x"), "TITLE x");
    }

    #[test]
    fn file_names_resolve() {
        let files = [PathBuf::from("/a/Joy Division - Unknown Pleasures.flac"), PathBuf::from("/a/02 - Day.flac")];
        assert_eq!(resolve("Joy Division - Unknown Pleasures.FLAC", &files), Some(&files[0]));
        assert_eq!(resolve("02 - Day.wav", &files), Some(&files[1]));
        assert_eq!(resolve("C:\\rips\\02 - Day.wav", &files), Some(&files[1]));
        assert_eq!(resolve("missing.flac", &files), None);
    }

    #[test]
    fn album_file_becomes_songs() {
        let dir = std::env::temp_dir().join(format!("neatlibrary-test-cue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sheet = dir.join("1979 - Unknown Pleasures.cue");
        std::fs::write(&sheet, IMAGE).unwrap();
        let image = Track {
            path: dir.join("1979 - Unknown Pleasures.flac"),
            ext: "flac".into(),
            // The album file's own tags are one song's, as in the real rip.
            title: Some("Day of the Lords".into()),
            track: Some(2),
            duration: Duration::from_secs(12 * 60),
            size: 1200,
            ..Default::default()
        };
        let other = Track { path: dir.join("bonus.flac"), ..Default::default() };
        let out = expand(vec![image, other], &[(sheet.clone(), IMAGE.into())]);
        assert_eq!(out.len(), 4);
        let titles: Vec<Option<&str>> = out.iter().map(|t| t.title.as_deref()).collect();
        assert_eq!(titles, [Some("Disorder"), Some("Day Of The Lords"), Some("Candidate"), None]);
        assert_eq!(out[1].track, Some(2));
        assert_eq!((out[0].artist.as_deref(), out[2].artist.as_deref()), (Some("JOY DIVISION"), Some("Someone Else")));
        assert_eq!((out[0].album.as_deref(), out[0].year), (Some("Unknown Pleasures"), Some(1979)));
        let span = out[1].cue.as_ref().unwrap();
        assert_eq!((span.start, span.end), ((3 * 60 + 31) * 75 + 73, Some((8 * 60 + 19) * 75 + 5)));
        assert_eq!(out[2].cue.as_ref().unwrap().end, None);
        assert_eq!(out[2].duration, frames_to_time(12 * 60 * 75 - ((8 * 60 + 19) * 75 + 5)));
        assert!(out[3].cue.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_song_file_takes_gaps_from_sheet() {
        let dir = std::env::temp_dir().join(format!("neatlibrary-test-cue-single-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sheet = dir.join("Octavarium.cue");
        let text = "PERFORMER \"Dream Theater\"\nTITLE \"Octavarium\"\nFILE \"Side 4.flac\" WAVE\n TRACK 08 AUDIO\n  TITLE \"Octavarium\"\n  INDEX 01 00:00:00\n";
        std::fs::write(&sheet, text).unwrap();
        let side = Track { path: dir.join("Side 4.flac"), ext: "flac".into(), year: Some(2005), ..Default::default() };
        let out = expand(vec![side], &[(sheet, text.into())]);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].title.as_deref(), out[0].track, out[0].artist.as_deref(), out[0].year), (Some("Octavarium"), Some(8), Some("Dream Theater"), Some(2005)));
        assert!(out[0].cue.is_none(), "a whole file is not cut");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
