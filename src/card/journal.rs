//! Undo journals for reorganising the music folder in place.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(super) fn journal_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join("pausetape-test-journals");
    }
    crate::platform::data_dir().join("journals")
}

pub(super) fn save_journal(moves: &[(PathBuf, PathBuf)]) -> Result<()> {
    let dir = journal_dir();
    fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    fs::write(dir.join(format!("{stamp}.json")), serde_json::to_vec_pretty(moves)?)?;
    Ok(())
}

/// Save the problems of a card operation as a text file people can read or send; its path.
pub fn save_problems(what: &str, errors: &[String]) -> Option<PathBuf> {
    let dir = journal_dir().join("problems");
    fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let path = dir.join(format!("{stamp} {what}.txt"));
    fs::write(&path, format!("{what}: {} problems\n\n{}\n", errors.len(), errors.join("\n"))).ok()?;
    Some(path)
}

pub fn latest_journal() -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(journal_dir()).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.pop()
}

/// Move a file back from `new` to `old`, unless something else has taken `old` since: a plain
/// rename would replace it (on every system, Windows included, as Rust renames).
pub(super) fn move_back(new: &Path, old: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(old).is_ok() && !same_file(new, old) {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{} is taken by another file now; left where it is", old.display())));
    }
    fs::rename(new, old)
}

/// Whether two paths name one file, as two spellings differing in case do on most Mac and
/// Windows disks.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        matches!((fs::metadata(a), fs::metadata(b)), (Ok(x), Ok(y)) if (x.dev(), x.ino()) == (y.dev(), y.ino()))
    }
    #[cfg(not(unix))]
    {
        a.as_os_str().to_ascii_lowercase() == b.as_os_str().to_ascii_lowercase()
    }
}

/// Reverse the most recent in-place move. Returns (restored, errors).
pub fn undo_latest() -> Result<(usize, Vec<String>)> {
    let path = latest_journal().context("nothing to undo")?;
    let moves: Vec<(PathBuf, PathBuf)> = serde_json::from_slice(&fs::read(&path)?)?;
    let mut restored = 0;
    let mut errors = Vec::new();
    for (old, new) in moves.iter().rev() {
        let res = (|| -> Result<()> {
            if let Some(p) = old.parent() {
                fs::create_dir_all(p)?;
            }
            move_back(new, old)?;
            Ok(())
        })();
        match res {
            Ok(()) => restored += 1,
            Err(e) => errors.push(format!("{}: {e:#}", new.display())),
        }
    }
    fs::rename(&path, path.with_extension("undone"))?;
    Ok((restored, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_back_never_replaces_a_file() {
        let dir = std::env::temp_dir().join(format!("pausetape-move-back-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (old, new) = (dir.join("song.flac"), dir.join("moved.flac"));
        fs::write(&new, "moved").unwrap();
        fs::write(&old, "added since").unwrap();
        assert!(move_back(&new, &old).is_err(), "the old place is taken");
        assert_eq!((fs::read_to_string(&old).unwrap(), fs::read_to_string(&new).unwrap()), ("added since".into(), "moved".into()));
        fs::remove_file(&old).unwrap();
        move_back(&new, &old).unwrap();
        // Only the case differs: on a case-insensitive disk both names are this one file.
        let upper = dir.join("Song.flac");
        move_back(&old, &upper).unwrap();
        assert_eq!(fs::read_to_string(&upper).unwrap(), "moved");
        let _ = fs::remove_dir_all(&dir);
    }
}
