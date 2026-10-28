//! Undo journals for reorganising the music folder in place.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub(super) fn journal_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join("neatlibrary-test-journals");
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

pub fn latest_journal() -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(journal_dir()).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.pop()
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
            fs::rename(new, old)?;
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
