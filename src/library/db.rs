//! The library database: tags read from each file, so rescans only open files that changed,
//! and in-app corrections that haven't been written to the files yet.
//!
//! One SQLite file serves every music folder; rows are keyed by absolute path (corrections by
//! track id, which adds "#03" for a song inside an album file). A cached row is trusted only
//! while the file's size and modification time match what was recorded. A folder read through
//! is recorded with its CUE sheets (contents too), so the app can open with its library as it
//! was without touching the folder.

use std::collections::HashMap;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::library::Track;
use crate::musicbrainz::ArtistInfo;

/// Bump when `Track` gains information older rows lack (2: MP4 codec; 3: bitrate, sample rate
/// and bit depth; 4: embedded cover; 5: damaged files), so files get re-read.
/// 6: tracks hold their genres (read again once).
const SCHEMA_VERSION: i64 = 6;
/// Corrections saved since this version stay valid: the fields they change haven't changed.
const EDITS_SINCE: i64 = 3;

pub struct Db {
    conn: Connection,
}

/// A track as last read from disk, with the size/mtime it was read at.
pub struct Cached {
    pub size: u64,
    pub mtime: u64,
    pub track: Track,
}

pub fn default_path() -> PathBuf {
    crate::platform::data_dir().join("library.sqlite3")
}

/// `[low, high)` bounds selecting every path inside `root`, for an indexed range scan.
fn prefix_range(root: &Path) -> (String, String) {
    let mut low = root.to_string_lossy().into_owned();
    if !low.ends_with(MAIN_SEPARATOR) {
        low.push(MAIN_SEPARATOR);
    }
    let mut high = low.clone();
    let last = high.pop().unwrap();
    high.push(char::from_u32(last as u32 + 1).unwrap());
    (low, high)
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        // WAL lets the scanner thread write while the UI thread reads or saves edits.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS tracks (
                 path  TEXT PRIMARY KEY,
                 size  INTEGER NOT NULL,
                 mtime INTEGER NOT NULL,
                 data  TEXT NOT NULL      -- Track as JSON, tags as read from the file
             );
             CREATE TABLE IF NOT EXISTS edits (
                 path  TEXT PRIMARY KEY,
                 size  INTEGER NOT NULL,  -- the file the edit was made against
                 mtime INTEGER NOT NULL,
                 data  TEXT NOT NULL      -- Track as JSON, with the user's corrections
             );
             CREATE TABLE IF NOT EXISTS folders (
                 root    TEXT PRIMARY KEY,
                 scanned INTEGER NOT NULL -- seconds since 1970 when it was last read through
             );
             CREATE TABLE IF NOT EXISTS artists (
                 key     TEXT PRIMARY KEY, -- the artist as the library groups it: \"flyinglotus\"
                 info    TEXT NOT NULL,    -- ArtistInfo as JSON: the MusicBrainz entry, links
                 fetched INTEGER NOT NULL  -- seconds since 1970
             );
             CREATE TABLE IF NOT EXISTS sheets (
                 path TEXT PRIMARY KEY,
                 data BLOB                -- the sheet's bytes; empty in rows from before they were kept
             );",
        )?;
        let has_data: bool = conn.prepare("SELECT 1 FROM pragma_table_info('sheets') WHERE name = 'data'")?.exists([])?;
        if !has_data {
            conn.execute_batch("ALTER TABLE sheets ADD COLUMN data BLOB;")?;
        }
        let version: Option<i64> = conn.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).ok();
        if version.is_some_and(|v| v != SCHEMA_VERSION) {
            // Older layout: the cache is disposable, so start over rather than migrate. The
            // user's corrections aren't, so they're kept when they still fit.
            conn.execute_batch("DELETE FROM tracks; DELETE FROM folders;")?;
            if version.is_some_and(|v| !(EDITS_SINCE..=SCHEMA_VERSION).contains(&v)) {
                conn.execute_batch("DELETE FROM edits;")?;
            }
        }
        conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)", params![SCHEMA_VERSION])?;
        Ok(Db { conn })
    }

    pub fn open_default() -> Result<Db> {
        Db::open(&default_path())
    }

    /// Every cached track inside `root`, keyed by path string.
    pub fn tracks_under(&self, root: &Path) -> Result<HashMap<String, Cached>> {
        let (low, high) = prefix_range(root);
        let mut stmt = self.conn.prepare("SELECT path, size, mtime, data FROM tracks WHERE path >= ?1 AND path < ?2")?;
        let rows = stmt.query_map(params![low, high], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (path, size, mtime, data) = row?;
            // A row that no longer deserialises is simply re-read from the file.
            if let Ok(track) = serde_json::from_str::<Track>(&data) {
                out.insert(path, Cached { size: size as u64, mtime: mtime as u64, track });
            }
        }
        Ok(out)
    }

    /// Insert or replace freshly read tracks, and forget paths that disappeared, in one transaction.
    pub fn update(&mut self, fresh: &[Track], gone: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut put = tx.prepare("INSERT OR REPLACE INTO tracks (path, size, mtime, data) VALUES (?1, ?2, ?3, ?4)")?;
            for t in fresh {
                put.execute(params![t.path.to_string_lossy(), t.size as i64, t.mtime as i64, serde_json::to_string(t)?])?;
            }
            let mut del = tx.prepare("DELETE FROM tracks WHERE path = ?1")?;
            // Also corrections to the songs of an album file ("<path>#03").
            let mut del_edit = tx.prepare("DELETE FROM edits WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '#'")?;
            for p in gone {
                del.execute(params![p])?;
                del_edit.execute(params![p])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// When `root` was last read through, if it was (with this layout of the database).
    pub fn scanned(&self, root: &Path) -> Result<Option<u64>> {
        let at = self.conn.query_row("SELECT scanned FROM folders WHERE root = ?1", params![root.to_string_lossy()], |r| r.get::<_, i64>(0));
        match at {
            Ok(at) => Ok(Some(at as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// `root` has just been read through and held these CUE sheets. With `complete` false (a
    /// folder in it couldn't be read), sheets known from before are kept too.
    pub fn record_scan(&mut self, root: &Path, sheets: &[(PathBuf, Vec<u8>)], complete: bool, at: u64) -> Result<()> {
        let tx = self.conn.transaction()?;
        if complete {
            let (low, high) = prefix_range(root);
            tx.execute("DELETE FROM sheets WHERE path >= ?1 AND path < ?2", params![low, high])?;
        }
        put_sheets(&tx, sheets)?;
        tx.execute("INSERT OR REPLACE INTO folders (root, scanned) VALUES (?1, ?2)", params![root.to_string_lossy(), at as i64])?;
        tx.commit()?;
        Ok(())
    }

    /// The CUE sheets inside `root` with their contents. A row stored before contents were kept
    /// is read from its file once, and kept from then on.
    pub fn sheets_under(&mut self, root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
        let (low, high) = prefix_range(root);
        let mut stmt = self.conn.prepare("SELECT path, data FROM sheets WHERE path >= ?1 AND path < ?2")?;
        let rows = stmt.query_map(params![low, high], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get::<_, Option<Vec<u8>>>(1)?)))?;
        let (mut sheets, mut unread) = (Vec::new(), Vec::new());
        for row in rows {
            match row? {
                (path, Some(data)) => sheets.push((path, data)),
                (path, None) => unread.push(path),
            }
        }
        drop(stmt);
        let read: Vec<(PathBuf, Vec<u8>)> = unread.into_iter().filter_map(|p| std::fs::read(&p).ok().map(|data| (p, data))).collect();
        if !read.is_empty() {
            let tx = self.conn.transaction()?;
            put_sheets(&tx, &read)?;
            tx.commit()?;
        }
        sheets.extend(read);
        Ok(sheets)
    }

    /// What MusicBrainz said about the artist the library groups under `key`, and when.
    pub fn artist_info(&self, key: &str) -> Result<Option<(ArtistInfo, u64)>> {
        let row = self.conn.query_row("SELECT info, fetched FROM artists WHERE key = ?1", params![key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)));
        match row {
            // A row that no longer deserialises is asked for again.
            Ok((info, at)) => Ok(serde_json::from_str(&info).ok().map(|i| (i, at as u64))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Every artist MusicBrainz was asked about, by key, with when (for the genres view).
    pub fn all_artist_info(&self) -> Result<Vec<(String, ArtistInfo, u64)>> {
        let mut q = self.conn.prepare("SELECT key, info, fetched FROM artists")?;
        let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
        Ok(rows.flatten().filter_map(|(k, info, at)| serde_json::from_str(&info).ok().map(|i| (k, i, at as u64))).collect())
    }

    pub fn save_artist_info(&mut self, key: &str, info: &ArtistInfo, at: u64) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO artists (key, info, fetched) VALUES (?1, ?2, ?3)", params![key, serde_json::to_string(info)?, at as i64])?;
        Ok(())
    }

    pub fn edits_under(&self, root: &Path) -> Result<HashMap<String, Cached>> {
        let (low, high) = prefix_range(root);
        let mut stmt = self.conn.prepare("SELECT path, size, mtime, data FROM edits WHERE path >= ?1 AND path < ?2")?;
        let rows = stmt.query_map(params![low, high], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (path, size, mtime, data) = row?;
            if let Ok(track) = serde_json::from_str::<Track>(&data) {
                out.insert(path, Cached { size: size as u64, mtime: mtime as u64, track });
            }
        }
        Ok(out)
    }

    /// Remember in-app corrections for these tracks (all marked dirty).
    pub fn save_edits<'a>(&mut self, tracks: impl IntoIterator<Item = &'a Track>) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut put = tx.prepare("INSERT OR REPLACE INTO edits (path, size, mtime, data) VALUES (?1, ?2, ?3, ?4)")?;
            for t in tracks {
                put.execute(params![t.id().to_string_lossy(), t.size as i64, t.mtime as i64, serde_json::to_string(t)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The corrections are now in the file itself; the next scan re-reads it. Takes track ids.
    pub fn drop_edits<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut del = tx.prepare("DELETE FROM edits WHERE path = ?1")?;
            for p in paths {
                del.execute(params![p.to_string_lossy()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The app's data folder moved: covers chosen for unsaved corrections, kept in it, follow.
    pub fn data_folder_moved(&mut self, old: &Path, new: &Path) -> Result<()> {
        // As the paths appear inside the stored JSON (escaped backslashes on Windows).
        let json = |p: &Path| serde_json::to_string(&p.join("covers")).map(|s| s.trim_matches('"').to_string());
        self.conn.execute("UPDATE edits SET data = replace(data, ?1, ?2)", params![json(old)?, json(new)?])?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        // Folders are read through again too: what they hold is no longer stored.
        self.conn.execute_batch("DELETE FROM tracks; DELETE FROM folders;")?;
        Ok(())
    }

    /// (cached tracks, pending edits) across all folders.
    pub fn counts(&self) -> (usize, usize) {
        let n = |table: &str| self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
        (n("tracks"), n("edits"))
    }
}

fn put_sheets(tx: &rusqlite::Transaction, sheets: &[(PathBuf, Vec<u8>)]) -> Result<()> {
    let mut put = tx.prepare("INSERT OR REPLACE INTO sheets (path, data) VALUES (?1, ?2)")?;
    for (path, data) in sheets {
        put.execute(params![path.to_string_lossy(), data])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_range_selects_only_children() {
        // Built with the platform's separator, as real paths are ("\\" on Windows).
        let music = Path::new("music");
        let (low, high) = prefix_range(&music.join("rock"));
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        let inside = s(music.join("rock").join("a.mp3"));
        let sibling = s(music.join("rock and roll").join("a.mp3"));
        let sibling2 = s(music.join("rock0").join("a.mp3"));
        assert!(inside >= low && inside < high);
        assert!(!(sibling >= low && sibling < high));
        assert!(!(sibling2 >= low && sibling2 < high));
    }

    #[test]
    fn rescans_reuse_unchanged_files_and_keep_corrections() {
        use crate::library::scan;
        let root = std::env::temp_dir().join(format!("pausetape-test-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let music = root.join("music");
        for f in ["Muse/Absolution/01 - Intro.mp3", "Muse/Absolution/02 - Apocalypse Please.mp3", "Blur/Parklife/01 - Girls and Boys.mp3"] {
            std::fs::create_dir_all(music.join(f).parent().unwrap()).unwrap();
            std::fs::write(music.join(f), f.as_bytes()).unwrap();
        }
        let mut db = Db::open(&root.join("library.sqlite3")).unwrap();

        let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!((tracks.len(), s.cached, s.read), (3, 0, 3));
        let (_, s) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!((s.cached, s.read), (3, 0), "nothing changed, nothing read");

        // A correction made in-app survives a rescan.
        let mut intro = tracks.iter().find(|t| t.display_title() == "Intro").unwrap().clone();
        intro.title = Some("Intro (Remastered)".into());
        intro.dirty = true;
        db.save_edits([&intro]).unwrap();
        let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!(s.edits, 1);
        assert!(tracks.iter().any(|t| t.dirty && t.title.as_deref() == Some("Intro (Remastered)")));

        // Change one file, delete another: one read, one forgotten, and the stale edit dropped.
        std::fs::write(&intro.path, b"re-encoded, different length").unwrap();
        std::fs::remove_file(music.join("Blur/Parklife/01 - Girls and Boys.mp3")).unwrap();
        let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!((tracks.len(), s.cached, s.read, s.removed, s.edits), (2, 1, 1, 1, 0));
        assert!(tracks.iter().all(|t| !t.dirty));
        assert_eq!(db.counts(), (2, 0));

        // `force` ignores the cache.
        let (_, s) = scan(&music, Some(&mut db), true, |_| {});
        assert_eq!((s.cached, s.read), (0, 2));

        // The drive isn't connected: no folder, or an empty mount point where it was. Nothing is
        // forgotten, corrections included.
        let tracks = scan(&music, Some(&mut db), false, |_| {}).0;
        let mut t = tracks[0].clone();
        t.title = Some("Corrected".into());
        t.dirty = true;
        db.save_edits([&t]).unwrap();
        std::fs::rename(&music, root.join("elsewhere")).unwrap();
        let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
        assert!(s.unreachable && tracks.is_empty());
        std::fs::create_dir(&music).unwrap();
        let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
        assert!(s.unreachable && tracks.is_empty(), "an empty folder where songs were known");
        assert_eq!(db.counts(), (2, 1));
        // Back again: everything as it was.
        std::fs::remove_dir(&music).unwrap();
        std::fs::rename(root.join("elsewhere"), &music).unwrap();
        let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!((tracks.len(), s.cached, s.edits, s.unreachable), (2, 2, 1, false));

        // A folder inside that can't be read: its songs aren't forgotten.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let muse = music.join("Muse");
            std::fs::create_dir_all(music.join("Blur/Parklife")).unwrap();
            std::fs::write(music.join("Blur/Parklife/02 - Tracy Jacks.mp3"), b"x").unwrap();
            std::fs::set_permissions(&muse, std::fs::Permissions::from_mode(0o000)).unwrap();
            let (tracks, s) = scan(&music, Some(&mut db), false, |_| {});
            std::fs::set_permissions(&muse, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!((tracks.len(), s.unreadable, s.removed, s.unreachable), (1, 1, 0, false));
            assert_eq!(db.counts(), (3, 1));
        }
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn covers_follow_the_data_folder() {
        let root = std::env::temp_dir().join(format!("pausetape-test-moved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut db = Db::open(&root.join("library.sqlite3")).unwrap();
        let (old, new, music) = (root.join("Old Name"), root.join("New Name"), root.join("music"));
        // Paths as the platform writes them (with backslashes on Windows), as the app's always are.
        let song = music.join("a.flac");
        let t = Track { path: song.clone(), cover: Some(old.join("covers").join("1.jpg")), dirty: true, ..Default::default() };
        db.save_edits([&t]).unwrap();
        db.data_folder_moved(&old, &new).unwrap();
        let edits = db.edits_under(&music).unwrap();
        assert_eq!(edits[song.to_string_lossy().as_ref()].track.cover.as_deref(), Some(new.join("covers").join("1.jpg").as_path()));
        // Windows can't remove a database that's still open.
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artist_info_is_kept() {
        let root = std::env::temp_dir().join(format!("pausetape-test-artists-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut db = Db::open(&root.join("library.sqlite3")).unwrap();
        assert!(db.artist_info("flyinglotus").unwrap().is_none());
        let info = ArtistInfo { mbid: "fc7376fe".into(), name: "Flying Lotus".into(), genres: vec!["downtempo".into()], ..Default::default() };
        db.save_artist_info("flyinglotus", &info, 1_700_000_000).unwrap();
        // Kept across a reopening, and by itself: forgetting stored tags leaves it.
        db.clear().unwrap();
        drop(db);
        let db = Db::open(&root.join("library.sqlite3")).unwrap();
        assert_eq!(db.artist_info("flyinglotus").unwrap(), Some((info, 1_700_000_000)));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_library_opens_as_last_scanned() {
        use crate::library::{load, scan};
        let root = std::env::temp_dir().join(format!("pausetape-test-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let music = root.join("music");
        let album = music.join("Pink Floyd/Animals");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("Animals.flac"), b"one file, five songs").unwrap();
        let sheet = "FILE \"Animals.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Pigs on the Wing 1\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Dogs\"\n    INDEX 01 01:25:00\n";
        std::fs::write(album.join("Animals.cue"), sheet).unwrap();
        std::fs::write(music.join("Pink Floyd/01 - Single.mp3"), b"x").unwrap();
        let mut db = Db::open(&root.join("library.sqlite3")).unwrap();

        // Never scanned: there's nothing to open it from.
        assert!(load(&music, &mut db).is_none());
        let (scanned, _) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!(scanned.len(), 3, "two songs from the album file, and the single");

        // Opened from the database: the same songs, corrections included, without the sheet
        // being read again; files added since by another program wait for a rescan.
        let cue = album.join("Animals.cue");
        std::fs::rename(&cue, album.join("Animals.cue.away")).unwrap();
        let mut dogs = scanned.iter().find(|t| t.display_title() == "Dogs").unwrap().clone();
        dogs.title = Some("Dogs (2018 Remix)".into());
        dogs.dirty = true;
        db.save_edits([&dogs]).unwrap();
        std::fs::write(music.join("Pink Floyd/02 - New.mp3"), b"x").unwrap();
        let (opened, s) = load(&music, &mut db).unwrap();
        assert!(s.stored.is_some() && s.read == 0 && s.edits == 1);
        let ids = |ts: &[Track]| {
            let mut v: Vec<PathBuf> = ts.iter().map(Track::id).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&opened), ids(&scanned));
        assert!(opened.iter().any(|t| t.dirty && t.title.as_deref() == Some("Dogs (2018 Remix)")));
        std::fs::rename(album.join("Animals.cue.away"), &cue).unwrap();
        // A sheet stored before its contents were kept is read once, then kept.
        db.conn.execute("UPDATE sheets SET data = NULL", []).unwrap();
        assert_eq!(load(&music, &mut db).unwrap().0.len(), 3);
        let unread: i64 = db.conn.query_row("SELECT COUNT(*) FROM sheets WHERE data IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(unread, 0);
        let (rescanned, s) = scan(&music, Some(&mut db), false, |_| {});
        assert_eq!((rescanned.len(), s.stored), (4, None));

        // The drive isn't connected: as unreachable as a scan says.
        std::fs::rename(&music, root.join("elsewhere")).unwrap();
        let (none, s) = load(&music, &mut db).unwrap();
        assert!(s.unreachable && none.is_empty());
        std::fs::rename(root.join("elsewhere"), &music).unwrap();

        // Stored tags forgotten: it's read through again.
        db.clear().unwrap();
        assert!(load(&music, &mut db).is_none());
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// `PAUSETAPE_BENCH=~/Music cargo test --release -- --ignored scan_speed --nocapture`
    /// Times a first scan against a rescan, using a throwaway database.
    #[test]
    #[ignore]
    fn scan_speed() {
        use crate::library::scan;
        let Some(dir) = std::env::var_os("PAUSETAPE_BENCH") else { return };
        let db_path = std::env::temp_dir().join(format!("pausetape-bench-{}.sqlite3", std::process::id()));
        let mut db = Db::open(&db_path).unwrap();
        for label in ["first scan", "rescan"] {
            let t = std::time::Instant::now();
            let (tracks, s) = scan(Path::new(&dir), Some(&mut db), false, |_| {});
            println!("{label}: {} songs in {:.2?} ({} read, {} from database)", tracks.len(), t.elapsed(), s.read, s.cached);
        }
        let _ = std::fs::remove_file(db_path);
    }
}
