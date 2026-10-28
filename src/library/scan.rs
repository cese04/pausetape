//! Scanning a music folder into `Track`s, reusing the library database for unchanged files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lofty::prelude::*;
use walkdir::WalkDir;

use super::db::Db;
use super::mp4::{MP4_EXTS, mp4_audio_codec};
use super::track::Track;

/// Everything scanned. Opus, APE, WavPack and WMA have no Rust decoder: listed, organised, not playable.
const AUDIO: &[&str] = &[
    "flac", "mp3", "wav", "m4a", "m4b", "mp4", "aac", "alac", "aiff", "aif", "aifc", "ogg", "oga", "caf", "mka", "mp2", "opus", "ape", "wv", "wma",
];

#[derive(Default)]
pub struct ScanStats {
    /// Tracks taken from the database because the file hadn't changed.
    pub cached: usize,
    /// Tracks whose tags were read from the file.
    pub read: usize,
    /// Database rows for files that no longer exist.
    pub removed: usize,
    /// Unsaved in-app corrections restored from the database.
    pub edits: usize,
    pub db_error: Option<String>,
}

pub fn mtime_of(meta: &std::fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// Walk `root` and return its tracks. With a database, a file whose size and modification time
/// match its stored row reuses the stored tags; only new or changed files are opened. `force`
/// re-reads every file. Calls `progress` with the number of files seen.
pub fn scan(root: &Path, mut db: Option<&mut Db>, force: bool, mut progress: impl FnMut(usize)) -> (Vec<Track>, ScanStats) {
    let mut stats = ScanStats::default();
    let mut cached = match db.as_deref().map(|d| d.tracks_under(root)) {
        Some(Ok(c)) => c,
        Some(Err(e)) => {
            note(&mut stats, e);
            Default::default()
        }
        None => Default::default(),
    };
    let edits = match db.as_deref().map(|d| d.edits_under(root)) {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            note(&mut stats, e);
            Default::default()
        }
        None => Default::default(),
    };

    let mut raw: Vec<Track> = Vec::new();
    let mut fresh: Vec<Track> = Vec::new();
    let entries = WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| {
        // Skip hidden entries, including macOS "._" AppleDouble files on FAT volumes.
        e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
    });
    for entry in entries.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else { continue };
        if !AUDIO.contains(&ext.as_str()) {
            continue;
        }
        let (size, mtime) = entry.metadata().map(|m| (m.len(), mtime_of(&m))).unwrap_or((0, 0));
        let hit = cached.remove(path.to_string_lossy().as_ref()).filter(|c| !force && c.size == size && c.mtime == mtime);
        let t = match hit {
            Some(c) => {
                stats.cached += 1;
                c.track
            }
            None => {
                let mut t = read_track(path, &ext);
                (t.size, t.mtime) = (size, mtime);
                stats.read += 1;
                fresh.push(t.clone());
                t
            }
        };
        raw.push(t);
        if raw.len() % 200 == 0 {
            progress(raw.len());
        }
    }

    // What's left in `cached` was not found on disk any more.
    let gone: Vec<String> = cached.into_keys().collect();
    stats.removed = gone.len();
    if let Some(db) = db.as_deref_mut()
        && let Err(e) = db.update(&fresh, &gone)
    {
        note(&mut stats, e);
    }

    // Folder-name inference depends on `root`, so it runs every scan rather than being cached.
    let mut stale_edits: HashSet<PathBuf> = HashSet::new();
    let tracks = raw
        .into_iter()
        .map(|mut t| {
            infer_from_path(&mut t, root);
            match edits.get(t.path.to_string_lossy().as_ref()) {
                Some(e) if e.size == t.size && e.mtime == t.mtime => {
                    stats.edits += 1;
                    Track { dirty: true, ..e.track.clone() }
                }
                Some(_) => {
                    // The file changed since the correction was made; trust the file.
                    stale_edits.insert(t.path.clone());
                    t
                }
                None => t,
            }
        })
        .collect();
    if let Some(db) = db
        && !stale_edits.is_empty()
        && let Err(e) = db.drop_edits(stale_edits.iter().map(PathBuf::as_path))
    {
        note(&mut stats, e);
    }
    (tracks, stats)
}

/// Keep the first database error; scanning carries on without the cache.
fn note(stats: &mut ScanStats, e: anyhow::Error) {
    stats.db_error.get_or_insert_with(|| format!("{e:#}"));
}

fn nonempty(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_track(path: &Path, ext: &str) -> Track {
    let mut t = Track { path: path.to_path_buf(), ext: ext.to_string(), ..Default::default() };
    if MP4_EXTS.contains(&ext) {
        t.codec = mp4_audio_codec(path);
    }
    let Ok(file) = lofty::read_from_path(path) else { return t };
    t.duration = file.properties().duration();
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        t.artist = nonempty(tag.artist());
        t.title = nonempty(tag.title());
        t.album = nonempty(tag.album());
        t.album_artist = tag.get_string(ItemKey::AlbumArtist).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        t.track = tag.track();
        t.disc = tag.disk();
        t.year = tag.date().map(|d| d.year).filter(|&y| y > 0);
        t.compilation = matches!(tag.get_string(ItemKey::FlagCompilation), Some("1" | "true"));
    }
    t
}

/// Fill gaps from the file name and folders: "03 - Title.mp3", "Artist - Title.mp3",
/// ".../Artist/Album/track.flac".
fn infer_from_path(t: &mut Track, root: &Path) {
    let stem = t.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    let mut rest = stem.as_str();
    if !digits.is_empty() && digits.len() <= 3 {
        rest = stem[digits.len()..].trim_start_matches([' ', '-', '.', '_']);
        if t.track.is_none() {
            t.track = digits.parse().ok();
        }
    }
    if let Some((a, title)) = rest.split_once(" - ") {
        if t.artist.is_none() {
            t.artist = Some(a.trim().to_string());
        }
        if t.title.is_none() {
            t.title = Some(title.trim().to_string());
        }
    } else if t.title.is_none() && !rest.is_empty() {
        t.title = Some(rest.trim().to_string());
    }

    let rel_dirs: Vec<String> = t
        .path
        .parent()
        .and_then(|p| p.strip_prefix(root).ok())
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    // Only trust folder names in an Artist/Album layout; a lone "Downloads" isn't an album.
    if t.album.is_none()
        && rel_dirs.len() >= 2
        && let Some(album) = rel_dirs.last()
    {
        // "1997 - OK Computer" -> "OK Computer"
        let album = album.split_once(" - ").filter(|(y, _)| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()));
        t.album = Some(album.map(|(_, a)| a).unwrap_or(rel_dirs.last().unwrap()).to_string());
    }
    if t.artist.is_none() && t.album_artist.is_none() && rel_dirs.len() >= 2 {
        t.artist = Some(rel_dirs[rel_dirs.len() - 2].clone());
    }
}
