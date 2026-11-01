//! Scanning a music folder into `Track`s, reusing the library database for unchanged files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lofty::prelude::*;
use walkdir::WalkDir;

use super::cue;
use super::db::{Cached, Db};
use super::mp4::{MP4_EXTS, mp4_audio_codec};
use super::names::{artist_key, clean_album, clean_artist_folder, container_folder, disc_folder, parse_file_stem, split_artist_album, split_known_artist};
use super::track::Track;

/// Everything scanned. Opus, APE, WavPack, WMA and DSD have no Rust decoder: listed, organised,
/// played here and converted for the card through ffmpeg.
pub const AUDIO: &[&str] = &[
    "flac", "mp3", "wav", "m4a", "m4b", "mp4", "aac", "alac", "aiff", "aif", "aifc", "ogg", "oga", "caf", "mka", "mp2", "opus", "ape", "wv", "wma", "dsf",
    "dff",
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
    /// The music folder couldn't be read, or held no music where the database knows some (a drive
    /// not connected, an empty mount point): no tracks, and nothing in the database changed.
    pub unreachable: bool,
    /// Folders inside it that couldn't be read. Songs the database knows there are kept.
    pub unreadable: usize,
    /// Taken from the database alone (`load`): the folder as it was read through at this time
    /// (seconds since 1970), unread since.
    pub stored: Option<u64>,
}

pub fn mtime_of(meta: &std::fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// Walk `root` and return its tracks. With a database, a file whose size and modification time
/// match its stored row reuses the stored tags; only new or changed files are opened. `force`
/// re-reads every file. Calls `progress` with the number of files seen.
pub fn scan(root: &Path, mut db: Option<&mut Db>, force: bool, mut progress: impl FnMut(usize)) -> (Vec<Track>, ScanStats) {
    let mut stats = ScanStats::default();
    // A folder that isn't there must never read as an empty library: that would forget every
    // song, and the corrections waiting to be written into them.
    if std::fs::read_dir(root).is_err() {
        stats.unreachable = true;
        return (Vec::new(), stats);
    }
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

    let known = cached.len();
    let mut raw: Vec<Track> = Vec::new();
    let mut fresh: Vec<Track> = Vec::new();
    let mut sheets: Vec<PathBuf> = Vec::new();
    // Linked folders are followed: on Linux especially, music from several drives is often
    // gathered into one folder that way. Songs keep their paths inside the music folder.
    let entries = WalkDir::new(root).follow_links(true).into_iter().filter_entry(|e| {
        // Skip hidden entries, including macOS "._" AppleDouble files on FAT volumes.
        e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
    });
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            // A link back to a folder above it: everything there is found the first time round.
            Err(e) if e.loop_ancestor().is_some() => continue,
            // Unreadable, or a link to a drive that isn't connected: the songs known there are kept.
            Err(_) => {
                stats.unreadable += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else { continue };
        if ext == "cue" {
            sheets.push(path.to_path_buf());
            continue;
        }
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

    // An empty mount point where the drive should be (or the drive gone mid-walk): change nothing.
    if raw.is_empty() && known > 0 {
        stats.unreachable = true;
        return (Vec::new(), stats);
    }
    // What's left in `cached` was not found on disk any more, unless part of the folder couldn't
    // be read: then it may well still be there.
    let gone: Vec<String> = if stats.unreadable == 0 { cached.into_keys().collect() } else { Vec::new() };
    stats.removed = gone.len();
    if let Some(db) = db.as_deref_mut()
        && let Err(e) = db.update(&fresh, &gone).and_then(|()| db.record_scan(root, &sheets, stats.unreadable == 0, now()))
    {
        note(&mut stats, e);
    }
    finish(root, raw, &sheets, &edits, db, stats)
}

/// The library as the last scan of `root` left it, from the database alone: no file is opened
/// and no folder walked, so it's there at once however large the folder. Files added, changed
/// or removed since by other programs show after a rescan. `None` for a folder never scanned
/// (or not since the database was cleared), or a database that can't be read: scan it then.
pub fn load(root: &Path, db: &mut Db) -> Option<(Vec<Track>, ScanStats)> {
    let mut stats = ScanStats::default();
    // As in `scan`: songs that can't be reached aren't shown as if they could.
    if std::fs::read_dir(root).is_err() {
        stats.unreachable = true;
        return Some((Vec::new(), stats));
    }
    stats.stored = Some(db.scanned(root).ok()??);
    let (cached, sheets, edits) = (db.tracks_under(root).ok()?, db.sheets_under(root).ok()?, db.edits_under(root).ok()?);
    let mut raw: Vec<Track> = cached.into_values().map(|c| c.track).collect();
    raw.sort_by(|a, b| a.path.cmp(&b.path));
    stats.cached = raw.len();
    Some(finish(root, raw, &sheets, &edits, Some(db), stats))
}

/// What every scan does after finding the files: songs from album files, names from folders,
/// the user's unsaved corrections.
fn finish(root: &Path, raw: Vec<Track>, sheets: &[PathBuf], edits: &HashMap<String, Cached>, db: Option<&mut Db>, mut stats: ScanStats) -> (Vec<Track>, ScanStats) {
    // Albums stored as one file plus a CUE sheet become one track per song. Like folder-name
    // inference, this runs every scan: sheets are small, and an edited sheet takes effect at once.
    let raw = cue::expand(raw, sheets);

    // Folder-name inference depends on `root`, so it runs every scan rather than being cached.
    let mut stale_edits: HashSet<PathBuf> = HashSet::new();
    let mut tracks: Vec<Track> = raw
        .into_iter()
        .map(|mut t| {
            infer_from_path(&mut t, root);
            match edits.get(t.id().to_string_lossy().as_ref()) {
                Some(e) if e.size == t.size && e.mtime == t.mtime => {
                    stats.edits += 1;
                    // What the file holds now, under the corrections.
                    Track { dirty: true, embedded_cover: t.embedded_cover, ..e.track.clone() }
                }
                Some(_) => {
                    // The file changed since the correction was made; trust the file.
                    stale_edits.insert(t.id());
                    t
                }
                None => t,
            }
        })
        .collect();
    artists_from_known(&mut tracks);
    if let Some(db) = db
        && !stale_edits.is_empty()
        && let Err(e) = db.drop_edits(stale_edits.iter().map(PathBuf::as_path))
    {
        note(&mut stats, e);
    }
    (tracks, stats)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Keep the first database error; scanning carries on without the cache.
fn note(stats: &mut ScanStats, e: anyhow::Error) {
    stats.db_error.get_or_insert_with(|| format!("{e:#}"));
}

fn nonempty(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// A file as a scan would read it now: for the database, after the app changed the file.
pub fn read_file(path: &Path) -> Option<Track> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let meta = std::fs::metadata(path).ok()?;
    let mut t = read_track(path, &ext);
    (t.size, t.mtime) = (meta.len(), mtime_of(&meta));
    Some(t)
}

fn read_track(path: &Path, ext: &str) -> Track {
    let mut t = Track { path: path.to_path_buf(), ext: ext.to_string(), ..Default::default() };
    if MP4_EXTS.contains(&ext) {
        t.codec = mp4_audio_codec(path);
    }
    if super::dsd::is_dsd(ext) {
        return read_dsd(t);
    }
    let Ok(file) = lofty::read_from_path(path) else {
        t.damaged = starts_blank(path);
        return t;
    };
    let props = file.properties();
    t.duration = props.duration();
    (t.kbps, t.sample_rate, t.bit_depth) = (props.audio_bitrate(), props.sample_rate(), props.bit_depth());
    read_tags(&mut t, &file);
    t
}

fn read_tags(t: &mut Track, file: &lofty::file::TaggedFile) {
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
    t.embedded_cover = file.tags().iter().any(|tag| tag.picture_count() > 0);
}

/// A DSD file, from its own headers (lofty doesn't read them). One bit a sample at millions of
/// samples a second: "hi-res", lossless.
fn read_dsd(mut t: Track) -> Track {
    let Some(d) = super::dsd::read(&t.path, &t.ext) else {
        t.damaged = starts_blank(&t.path);
        return t;
    };
    (t.duration, t.sample_rate, t.bit_depth) = (d.duration, Some(d.rate), Some(1));
    t.kbps = Some(d.rate * d.channels as u32 / 1000);
    if let Some(file) = d.id3.as_deref().and_then(super::dsd::tags) {
        read_tags(&mut t, &file);
    }
    t
}

/// The file is empty, or its first 4 KB are zeros: where a header should be, nothing was kept.
fn starts_blank(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::with_capacity(4096);
    std::fs::File::open(path).and_then(|f| f.take(4096).read_to_end(&mut head)).is_ok_and(|_| head.iter().all(|&b| b == 0))
}

/// Fill gaps from the file name and folders: "03 - Title.mp3", "1979 - Album.flac",
/// ".../Artist/1997 - Album/CD 1/track.flac", ".../Artist - collection/1979. Album/edition/".
fn infer_from_path(t: &mut Track, root: &Path) {
    let stem = t.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let p = parse_file_stem(&stem);
    t.track = t.track.or(p.track);
    t.disc = t.disc.or(p.disc);
    t.year = t.year.or(p.year);
    if t.artist.is_none() {
        t.artist = p.artist;
    }
    if t.title.is_none() {
        t.title = p.title;
    }

    let mut dirs: Vec<String> = t
        .path
        .parent()
        .and_then(|p| p.strip_prefix(root).ok())
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    // "CD 1" belongs to the album folder above it.
    if let Some(d) = dirs.last().and_then(|d| disc_folder(d)) {
        t.disc.get_or_insert(d);
        dirs.pop();
    }
    let Some((album_dir, above)) = dirs.split_last() else { return };
    // The artist is the nearest folder above that isn't itself an album, e.g. skipping
    // "1979. Unknown Pleasures {3 editions}" that holds several editions.
    let album_like = |d: &str| disc_folder(d).is_some() || clean_album(d, None).year.is_some();
    // Nor a folder like "Music" or "From the card" that holds albums of many artists.
    let mut folder_artist = above.iter().rev().find(|d| !album_like(d) && !container_folder(d)).map(|d| clean_artist_folder(d));
    let hint = t.album_artist.clone().or(t.artist.clone()).or(folder_artist.clone());
    let mut name = clean_album(album_dir, hint.as_deref());
    // A lone "Artist - Album (1997)" folder names both; a lone "Downloads" isn't an album.
    let mut trusted = !above.is_empty();
    if folder_artist.is_none()
        && let Some((a, n)) = split_artist_album(album_dir)
    {
        (folder_artist, name, trusted) = (Some(a), n, true);
    }
    if !trusted {
        return;
    }
    if t.album.is_none() {
        t.album = Some(name.album);
    }
    t.year = t.year.or(name.year);
    if t.artist.is_none() && t.album_artist.is_none() {
        t.artist = folder_artist;
    }
}

/// Songs left without an artist whose album folder starts with an artist the library knows
/// from other songs: "Guns N' Roses-Appetite For Destruction" when another album is tagged
/// "Guns N' Roses". Folder names alone are too ambiguous to split on a bare dash otherwise.
fn artists_from_known(tracks: &mut [Track]) {
    let known: HashSet<String> = tracks.iter().filter_map(|t| t.album_artist.as_deref().or(t.artist.as_deref())).map(artist_key).collect();
    for t in tracks.iter_mut().filter(|t| t.artist.is_none() && t.album_artist.is_none() && !t.dirty) {
        let mut dirs = t.path.ancestors().skip(1).filter_map(|d| d.file_name()).map(|d| d.to_string_lossy());
        let Some(mut dir) = dirs.next() else { continue };
        if disc_folder(&dir).is_some() {
            let Some(d) = dirs.next() else { continue };
            dir = d;
        }
        let Some((artist, name)) = split_known_artist(&dir, |k| known.contains(k)) else { continue };
        // Replace an album named after the whole folder, not one from the tags.
        if t.album.as_ref().is_none_or(|a| *a == clean_album(&dir, None).album) {
            t.album = Some(name.album);
        }
        t.year = t.year.or(name.year);
        t.artist = Some(artist);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inferred(rel: &str) -> Track {
        let root = Path::new("/music");
        let mut t = Track { path: root.join(rel), ..Default::default() };
        infer_from_path(&mut t, root);
        t
    }

    #[test]
    fn names_from_folders() {
        let t = inferred("Joy Division - коллекция/1979. Unknown Pleasures {3 издания}/1979. Unknown Pleasures {1989, US, Qwest Records, 9 25840-2} v.1/1979 - Unknown Pleasures.flac");
        assert_eq!(t.artist.as_deref(), Some("Joy Division"));
        assert_eq!(t.album.as_deref(), Some("Unknown Pleasures"));
        assert_eq!(t.year, Some(1979));

        let t = inferred("Pink Floyd - The Wall (2007 Remaster) [FLAC] 88/CD 2/03 - Hey You.flac");
        assert_eq!((t.artist.as_deref(), t.album.as_deref(), t.disc, t.track), (Some("Pink Floyd"), Some("The Wall"), Some(2), Some(3)));

        let t = inferred("Radiohead/1997 - OK Computer/02 - Paranoid Android.mp3");
        assert_eq!((t.artist.as_deref(), t.album.as_deref(), t.year, t.title.as_deref()), (Some("Radiohead"), Some("OK Computer"), Some(1997), Some("Paranoid Android")));

        // A folder of many artists' albums isn't an artist.
        let t = inferred("From the card/50 Cent (2003) - Get Rich Or Die Tryin/01. Intro (2015_05_21 08_09_57 UTC).mp3");
        assert_eq!((t.artist.as_deref(), t.album.as_deref(), t.year), (Some("50 Cent"), Some("Get Rich Or Die Tryin"), Some(2003)));
        let t = inferred("From the card/Guns N' Roses-Appetite For Destruction/1. Welcome To The Jungle (2015_05_21 08_09_57 UTC).mp3");
        assert_eq!(t.artist, None);

        let t = inferred("Downloads/03 - Song.mp3");
        assert_eq!((t.artist, t.album), (None, None));

        // Tags win over folders.
        let root = Path::new("/music");
        let mut t = Track { path: root.join("Joy Division - коллекция/1980. Closer {x}/a.flac"), artist: Some("JOY DIVISION".into()), ..Default::default() };
        infer_from_path(&mut t, root);
        assert_eq!((t.artist.as_deref(), t.album.as_deref()), (Some("JOY DIVISION"), Some("Closer")));
    }

    #[cfg(unix)]
    #[test]
    fn linked_folders_are_followed() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("neatlibrary-test-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (music, other) = (dir.join("music"), dir.join("other drive"));
        std::fs::create_dir_all(other.join("Blur/Parklife")).unwrap();
        std::fs::write(other.join("Blur/Parklife/01 - Girls & Boys.mp3"), "x").unwrap();
        std::fs::create_dir_all(&music).unwrap();
        symlink(&other, music.join("Linked")).unwrap();
        // A link back up: found once, not round and round.
        symlink(&music, music.join("Loop")).unwrap();
        let (tracks, stats) = scan(&music, None, false, |_| {});
        assert_eq!(tracks.iter().map(|t| t.path.clone()).collect::<Vec<_>>(), [music.join("Linked/Blur/Parklife/01 - Girls & Boys.mp3")]);
        assert_eq!(stats.unreadable, 0);
        // The linked drive unplugged: its songs count as unreachable, not gone.
        std::fs::remove_dir_all(&other).unwrap();
        let (_, stats) = scan(&music, None, false, |_| {});
        assert_eq!(stats.unreadable, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blank_files_are_damaged() {
        let dir = std::env::temp_dir().join(format!("neatlibrary-test-damaged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // As a failing card leaves them: the size is right, the bytes are zeros.
        std::fs::write(dir.join("blank.ape"), vec![0u8; 100_000]).unwrap();
        std::fs::write(dir.join("empty.flac"), b"").unwrap();
        // Unreadable, but not blank: not called damaged.
        std::fs::write(dir.join("odd.flac"), b"not really audio").unwrap();
        let read = |n: &str| read_track(&dir.join(n), n.rsplit('.').next().unwrap());
        assert!(read("blank.ape").damaged && read("empty.flac").damaged && !read("odd.flac").damaged);
        let t = read("blank.ape");
        assert!(!t.plays_here());
        assert!(crate::card::target(&t, Default::default()).is_err_and(|why| why.contains("damaged")), "left off the card, saying why");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn artist_from_a_folder_starting_with_a_known_artist() {
        let mut tracks = vec![
            inferred("From the card/Guns N' Roses-Appetite For Destruction/1. Welcome To The Jungle (2015_05_21 08_09_57 UTC).mp3"),
            inferred("Jay-Z-The Blueprint/01 - The Ruler's Back.mp3"),
            inferred("Bits-Pieces/01 - Song.mp3"),
            Track { artist: Some("Guns n' Roses".into()), ..inferred("Guns n' Roses/1991 - Use Your Illusion I/01 - Right Next Door to Hell.flac") },
            Track { artist: Some("JAY-Z".into()), ..Default::default() },
        ];
        artists_from_known(&mut tracks);
        assert_eq!((tracks[0].artist.as_deref(), tracks[0].album.as_deref()), (Some("Guns N' Roses"), Some("Appetite For Destruction")));
        assert_eq!((tracks[1].artist.as_deref(), tracks[1].album.as_deref()), (Some("Jay-Z"), Some("The Blueprint")));
        assert_eq!(tracks[2].artist, None, "no artist called Bits in the library");
    }
}
