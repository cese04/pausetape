//! Playlists for a player without playlist support.
//!
//! FAT32/exFAT cannot store symbolic links (macOS fakes them as small "XSym" text files the
//! player can't read), so a playlist is exported as a folder of numbered *copies*:
//! `_Playlists/Road Trip/01 - Artist - Title.flac`. Holding Play on that folder in RECHO plays it
//! in order. An `.m3u8` pointing at the organised originals is written alongside, ready for when
//! the firmware exposes its internal m3u support.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::library::Track;
use crate::library::names::sanitize_component;
use super::{Written, plain_copy};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    /// Source file paths, so playlists survive rescans.
    pub tracks: Vec<PathBuf>,
}

impl Playlist {
    pub fn bytes(&self, by_path: &HashMap<&Path, &Track>) -> u64 {
        self.tracks.iter().filter_map(|p| by_path.get(p.as_path())).map(|t| t.size).sum()
    }
}

#[derive(Default)]
pub struct ExportReport {
    pub copied: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub missing: usize,
    pub errors: Vec<String>,
    /// Copies and .m3u8 files now on the card, relative to `dest`.
    pub written: Vec<Written>,
}

/// One planned copy: card path (relative to the card root), source track, label for the m3u8.
pub struct Planned<'a> {
    pub rel: PathBuf,
    pub track: &'a Track,
    pub label: String,
}

/// Where each playlist's copies go: `<folder>/<Name>/NN - Artist - Title.ext`.
/// Returns, per playlist, its folder (relative) and its copies; missing tracks are skipped.
/// With `skip_unplayable`, tracks RECHO can't play are left out (numbering closes the gap).
pub fn planned<'a>(playlists: &[Playlist], by_path: &HashMap<&Path, &'a Track>, folder: &str, skip_unplayable: bool) -> Vec<(PathBuf, Vec<Planned<'a>>, usize)> {
    let root = PathBuf::from(sanitize_component(folder, 80));
    playlists
        .iter()
        .map(|pl| {
            let dir = root.join(sanitize_component(&pl.name, 80));
            let width = pl.tracks.len().to_string().len().max(2);
            let mut missing = 0;
            let copies = pl
                .tracks
                .iter()
                .filter_map(|path| {
                    let t = by_path.get(path.as_path());
                    if t.is_none() {
                        missing += 1;
                    }
                    t.filter(|t| !skip_unplayable || t.on_recho())
                })
                .enumerate()
                .map(|(n, t)| {
                    let label = format!("{} - {}", t.artist.as_deref().unwrap_or("Unknown"), t.display_title());
                    let file = format!("{:0width$} - {}.{}", n + 1, sanitize_component(&label, 110), t.ext);
                    Planned { rel: dir.join(file), track: t, label }
                })
                .collect();
            (dir, copies, missing)
        })
        .collect()
}

/// Export every playlist under `dest/<folder>`. `organised` maps a source path to where the
/// organiser put it (relative to `dest`), used for the m3u8; may be empty.
pub fn export(
    playlists: &[Playlist],
    tracks: &[Track],
    organised: &HashMap<PathBuf, PathBuf>,
    dest: &Path,
    folder: &str,
    skip_unplayable: bool,
    mut progress: impl FnMut(usize, usize),
) -> ExportReport {
    let mut r = ExportReport::default();
    let by_path: HashMap<&Path, &Track> = tracks.iter().map(|t| (t.path.as_path(), t)).collect();
    let plan = planned(playlists, &by_path, folder, skip_unplayable);
    let total: usize = plan.iter().map(|(_, c, _)| c.len()).sum();
    let mut done = 0;
    for (dir_rel, copies, missing) in plan {
        r.missing += missing;
        let dir = dest.join(&dir_rel);
        if let Err(e) = fs::create_dir_all(&dir) {
            r.errors.push(format!("{}: {e}", dir.display()));
            continue;
        }
        let mut keep = HashSet::new();
        let mut m3u = String::from("#EXTM3U\n");
        for c in copies {
            done += 1;
            progress(done, total);
            let dst = dest.join(&c.rel);
            keep.insert(c.rel.file_name().unwrap().to_string_lossy().into_owned());
            if fs::metadata(&dst).is_ok_and(|m| m.len() == c.track.size) {
                r.unchanged += 1;
            } else if let Err(e) = plain_copy(&c.track.path, &dst) {
                r.errors.push(format!("{}: {e:#}", c.track.path.display()));
                continue;
            } else {
                r.copied += 1;
            }
            r.written.push(Written { rel: c.rel.clone(), size: c.track.size });
            let target = organised
                .get(&c.track.path)
                .map(|rel| Path::new("..").join(rel))
                .unwrap_or_else(|| c.rel.strip_prefix(dir_rel.parent().unwrap_or(Path::new(""))).unwrap_or(&c.rel).to_path_buf());
            m3u.push_str(&format!("#EXTINF:{},{}\n{}\n", c.track.duration.as_secs(), c.label, target.to_string_lossy().replace('\\', "/")));
        }
        // The folder belongs to us: drop copies of tracks no longer in the playlist (or renumbered).
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if !keep.contains(&n) && fs::remove_file(e.path()).is_ok() {
                    r.removed += 1;
                }
            }
        }
        // The m3u8 sits beside the folder: `_Playlists/Road Trip.m3u8`.
        let m3u_rel = dir_rel.with_file_name(format!("{}.m3u8", dir_rel.file_name().unwrap().to_string_lossy()));
        match fs::write(dest.join(&m3u_rel), &m3u) {
            Ok(()) => r.written.push(Written { rel: m3u_rel, size: m3u.len() as u64 }),
            Err(e) => r.errors.push(format!("{}: {e}", m3u_rel.display())),
        }
    }
    r
}

/// Follow files moved by the organiser so playlists keep pointing at them.
pub fn remap(playlists: &mut [Playlist], moves: &[(PathBuf, PathBuf)]) {
    let map: HashMap<&PathBuf, &PathBuf> = moves.iter().map(|(a, b)| (a, b)).collect();
    for pl in playlists {
        for p in &mut pl.tracks {
            if let Some(new) = map.get(p) {
                *p = (*new).clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_numbers_copies_and_prunes_removed_tracks() {
        let root = std::env::temp_dir().join(format!("neatlibrary-test-pl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tracks: Vec<Track> = ["a", "b"]
            .iter()
            .map(|n| {
                let path = root.join(format!("{n}.mp3"));
                fs::write(&path, n.as_bytes()).unwrap();
                Track { path, ext: "mp3".into(), artist: Some("X".into()), title: Some(n.to_uppercase()), size: 1, ..Default::default() }
            })
            .collect();
        let mut pl = Playlist { name: "Road: Trip".into(), tracks: vec![tracks[1].path.clone(), tracks[0].path.clone()] };
        let dest = root.join("card");
        let r = export(&[pl.clone()], &tracks, &HashMap::new(), &dest, "_Playlists", true, |_, _| {});
        assert_eq!((r.copied, r.errors.len(), r.written.len()), (2, 0, 3));
        let dir = dest.join("_Playlists/Road - Trip");
        assert!(dir.join("01 - X - B.mp3").exists() && dir.join("02 - X - A.mp3").exists());
        assert!(dest.join("_Playlists/Road - Trip.m3u8").exists());

        pl.tracks.pop();
        let r = export(&[pl], &tracks, &HashMap::new(), &dest, "_Playlists", true, |_, _| {});
        assert_eq!((r.unchanged, r.removed), (1, 1));
        fs::remove_dir_all(root).unwrap();
    }
}
