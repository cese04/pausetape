//! The library database: tags read from each file, so rescans only open files that changed,
//! and in-app corrections that haven't been written to the files yet.
//!
//! One SQLite file serves every music folder; rows are keyed by absolute path (corrections by
//! track id, which adds "#03" for a song inside an album file). A cached row is trusted only
//! while the file's size and modification time match what was recorded.

use std::collections::HashMap;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::library::Track;

/// Bump when `Track` gains information older rows lack (2: MP4 codec; 3: bitrate, sample rate
/// and bit depth; 4: embedded cover), so files get re-read.
const SCHEMA_VERSION: i64 = 4;
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
             );",
        )?;
        let version: Option<i64> = conn.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).ok();
        if version.is_some_and(|v| v != SCHEMA_VERSION) {
            // Older layout: the cache is disposable, so start over rather than migrate. The
            // user's corrections aren't, so they're kept when they still fit.
            conn.execute_batch("DELETE FROM tracks;")?;
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

    pub fn clear(&mut self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM tracks;")?;
        Ok(())
    }

    /// (cached tracks, pending edits) across all folders.
    pub fn counts(&self) -> (usize, usize) {
        let n = |table: &str| self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
        (n("tracks"), n("edits"))
    }
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
        let root = std::env::temp_dir().join(format!("neatlibrary-test-db-{}", std::process::id()));
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
        std::fs::remove_dir_all(root).unwrap();
    }

    /// `NEATLIBRARY_BENCH=~/Music cargo test --release -- --ignored scan_speed --nocapture`
    /// Times a first scan against a rescan, using a throwaway database.
    #[test]
    #[ignore]
    fn scan_speed() {
        use crate::library::scan;
        let Some(dir) = std::env::var_os("NEATLIBRARY_BENCH") else { return };
        let db_path = std::env::temp_dir().join(format!("neatlibrary-bench-{}.sqlite3", std::process::id()));
        let mut db = Db::open(&db_path).unwrap();
        for label in ["first scan", "rescan"] {
            let t = std::time::Instant::now();
            let (tracks, s) = scan(Path::new(&dir), Some(&mut db), false, |_| {});
            println!("{label}: {} songs in {:.2?} ({} read, {} from database)", tracks.len(), t.elapsed(), s.read, s.cached);
        }
        let _ = std::fs::remove_file(db_path);
    }
}
