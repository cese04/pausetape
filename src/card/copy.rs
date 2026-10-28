//! Carrying out a plan: copying to a card or moving within the music folder, removing what an
//! earlier write left behind, and cleaning up macOS metadata files.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::journal::save_journal;
use super::plan::{Options, PlanItem};
use crate::library::{Track, write_tags};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Copy into a separate folder (e.g. the SD card). Source untouched; re-runs only copy what changed.
    CopyTo,
    /// Rename files inside the source folder.
    MoveInPlace,
}

pub enum Progress {
    Step { done: usize, total: usize },
    Finished(Report),
}

#[derive(Default)]
pub struct Report {
    pub copied: usize,
    pub moved: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub tagged: usize,
    pub cleaned: usize,
    pub errors: Vec<String>,
    /// (old, new) paths of moved files, oldest first; also saved as an undo journal.
    pub moves: Vec<(PathBuf, PathBuf)>,
    /// Files now present at the destination because of this plan (copy mode).
    pub written: Vec<Written>,
}

/// One file a write put on the card, relative to the card root. Recorded so later writes can
/// tell their own files from anything else on the card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub rel: PathBuf,
    pub size: u64,
}

/// Run the plan. `tracks` must be the same list the plan was built from.
pub fn execute(
    tracks: &[Track],
    plan: &[PlanItem],
    source: &Path,
    dest: &Path,
    mode: Mode,
    opts: &Options,
    mut progress: impl FnMut(Progress),
) -> Report {
    let mut r = Report::default();
    for (n, item) in plan.iter().enumerate() {
        let t = &tracks[item.track];
        progress(Progress::Step { done: n, total: plan.len() });
        if item.skip.is_some() {
            r.skipped += 1;
            continue;
        }
        let dst = dest.join(&item.rel);
        match place(t, &dst, mode, opts, &mut r) {
            Ok(()) if mode == Mode::CopyTo => {
                let size = fs::metadata(&dst).map(|m| m.len()).unwrap_or(t.size);
                r.written.push(Written { rel: item.rel.clone(), size });
            }
            Ok(()) => {}
            Err(e) => r.errors.push(format!("{}: {e:#}", t.path.display())),
        }
    }
    if mode == Mode::MoveInPlace {
        remove_empty_dirs(source);
        if !r.moves.is_empty()
            && let Err(e) = save_journal(&r.moves)
        {
            r.errors.push(format!("saving undo journal: {e:#}"));
        }
    }
    if opts.clean_mac_files {
        r.cleaned = clean_mac_files(dest);
    }
    r
}

fn place(t: &Track, dst: &Path, mode: Mode, opts: &Options, r: &mut Report) -> Result<()> {
    let tag = |r: &mut Report| -> Result<()> {
        if t.dirty && opts.write_tags {
            write_tags(t, dst)?;
            r.tagged += 1;
        }
        Ok(())
    };
    if t.path == dst {
        r.unchanged += 1;
        return tag(r);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    match mode {
        Mode::CopyTo => {
            let same_size = fs::metadata(dst).is_ok_and(|m| m.len() == t.size);
            if same_size && !t.dirty {
                r.unchanged += 1;
                return Ok(());
            }
            plain_copy(&t.path, dst)?;
            r.copied += 1;
        }
        Mode::MoveInPlace => {
            // Case-only renames report the destination as existing on case-insensitive volumes.
            let case_only = t.path.to_string_lossy().to_lowercase() == dst.to_string_lossy().to_lowercase();
            if dst.exists() && !case_only {
                anyhow::bail!("{} already exists; not overwriting", dst.display());
            }
            if fs::rename(&t.path, dst).is_err() {
                plain_copy(&t.path, dst)?;
                fs::remove_file(&t.path)?;
            }
            r.moves.push((t.path.clone(), dst.to_path_buf()));
            r.moved += 1;
        }
    }
    tag(r)
}

/// Byte-for-byte copy. `std::fs::copy` on macOS also copies extended attributes, which on a
/// FAT/exFAT card materialise as "._name" AppleDouble files that clutter the player's browser.
pub fn plain_copy(src: &Path, dst: &Path) -> Result<()> {
    let tmp = dst.with_extension("partial");
    {
        let mut reader = BufReader::with_capacity(1 << 20, File::open(src)?);
        let mut writer = BufWriter::with_capacity(1 << 20, File::create(&tmp)?);
        io::copy(&mut reader, &mut writer)?;
        writer.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    }
    fs::rename(&tmp, dst)?;
    Ok(())
}

/// Delete files an earlier write recorded that are not in `keep` (lowercased rel paths: the card
/// is case-insensitive). A file is only removed if its size still matches the record, so anything
/// the user replaced by hand stays. Returns how many files were removed.
pub fn prune(dest: &Path, previous: &[Written], keep: &HashSet<String>) -> usize {
    let mut removed = 0;
    for w in previous {
        // Records come from our own manifest, but never follow one outside the card.
        if !w.rel.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
            continue;
        }
        if keep.contains(&w.rel.to_string_lossy().to_lowercase()) {
            continue;
        }
        let path = dest.join(&w.rel);
        if fs::metadata(&path).is_ok_and(|m| m.len() == w.size) && fs::remove_file(&path).is_ok() {
            removed += 1;
            // Remove folders this left empty, up to (not including) the card root.
            let mut dir = path.parent();
            while let Some(d) = dir.filter(|d| *d != dest && d.starts_with(dest)) {
                if fs::remove_dir(d).is_err() {
                    break;
                }
                dir = d.parent();
            }
        }
    }
    removed
}

fn remove_empty_dirs(root: &Path) {
    // Deepest first, so parents become empty after their children go. Ignore Finder droppings.
    let dirs: Vec<PathBuf> = WalkDir::new(root)
        .min_depth(1)
        .contents_first(true)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.into_path())
        .collect();
    for d in dirs {
        let only_junk = fs::read_dir(&d).is_ok_and(|entries| {
            entries.flatten().all(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n == ".DS_Store" || n.starts_with("._")
            })
        });
        if only_junk {
            let _ = fs::remove_dir_all(&d);
        }
    }
}

/// Delete macOS metadata files ("._*", ".DS_Store") that show up as junk entries on the player.
pub fn clean_mac_files(root: &Path) -> usize {
    WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let n = e.file_name().to_string_lossy();
            n.starts_with("._") || n == ".DS_Store"
        })
        .filter(|e| fs::remove_file(e.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{AlbumOrder, Custom, build_plan, undo_latest};
    use crate::library::grouping::{ArtistPrefs, build};
    use crate::library::scan;

    /// Untagged dummy files: everything is inferred from "Artist/Album/NN - Title.ext".
    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("neatlibrary-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for f in ["src/Muse/Absolution/02 - Apocalypse Please.mp3", "src/Muse/Absolution/01 - Intro.mp3", "src/Muse/Absolution/._junk.mp3", "src/Downloads/Muse - Hysteria.mp3"] {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"not really audio").unwrap();
        }
        root
    }

    #[test]
    fn copy_is_incremental_and_skips_appledouble() {
        let root = fixture("copy");
        let (src, dst) = (root.join("src"), root.join("dst"));
        let (tracks, _) = scan(&src, None, false, |_| {});
        assert_eq!(tracks.len(), 3, "._ files are not tracks");
        let index = build(&tracks, &ArtistPrefs::default());
        let opts = Options { write_tags: false, ..Default::default() };
        let plan = build_plan(&tracks, &index, &opts, &Custom::default());
        let r = execute(&tracks, &plan, &src, &dst, Mode::CopyTo, &opts, |_| {});
        assert_eq!((r.copied, r.errors.len(), r.written.len()), (3, 0, 3), "{:?}", r.errors);
        assert!(dst.join("Muse/Absolution/01 - Intro.mp3").exists());
        assert!(dst.join("Muse/Singles/Muse - Hysteria.mp3").exists());
        let again = execute(&tracks, &plan, &src, &dst, Mode::CopyTo, &opts, |_| {});
        assert_eq!((again.copied, again.unchanged), (0, 3));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn move_in_place_then_undo() {
        let root = fixture("move");
        let src = root.join("src");
        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let opts = Options { write_tags: false, clean_mac_files: false, ..Default::default() };
        let plan = build_plan(&tracks, &index, &opts, &Custom::default());
        let r = execute(&tracks, &plan, &src, &src, Mode::MoveInPlace, &opts, |_| {});
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!((r.moved, r.unchanged), (1, 2));
        assert!(src.join("Muse/Singles/Muse - Hysteria.mp3").exists());
        assert!(!src.join("Downloads").exists(), "emptied folders are removed");
        let (restored, errors) = undo_latest().unwrap();
        assert_eq!((restored, errors.len()), (1, 0));
        assert!(src.join("Downloads/Muse - Hysteria.mp3").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn custom_layout_excludes_renames_orders_and_prunes() {
        let root = fixture("custom");
        let (src, dst) = (root.join("src"), root.join("dst"));
        fs::create_dir_all(src.join("Muse/Showbiz")).unwrap();
        fs::write(src.join("Muse/Showbiz/01 - Sunburn.mp3"), b"x").unwrap();
        let (tracks, _) = scan(&src, None, false, |_| {});
        let index = build(&tracks, &ArtistPrefs::default());
        let ck = index.clusters[index.artists[0].cluster].key.clone();
        let ids: Vec<String> = index.artists[0].albums.iter().map(|a| a.id(&ck)).collect();
        let showbiz = ids.iter().find(|i| i.ends_with("showbiz")).unwrap().clone();
        let absolution = ids.iter().find(|i| i.ends_with("absolution")).unwrap().clone();

        let opts = Options { album_order: AlbumOrder::Custom, write_tags: false, ..Default::default() };
        let mut custom = Custom::default();
        custom.artist_names.insert(ck.clone(), "MUSE".into());
        custom.album_order.insert(ck.clone(), vec![showbiz.clone(), absolution.clone()]);
        let plan = build_plan(&tracks, &index, &opts, &custom);
        // Compare paths, not strings, so the test holds with Windows separators too.
        let rels: Vec<&Path> = plan.iter().map(|p| p.rel.as_path()).collect();
        let expect = |parts: &[&str]| parts.iter().collect::<PathBuf>();
        assert!(rels.contains(&expect(&["MUSE", "01 - Showbiz", "01 - Sunburn.mp3"]).as_path()), "{rels:?}");
        assert!(rels.contains(&expect(&["MUSE", "02 - Absolution", "01 - Intro.mp3"]).as_path()), "{rels:?}");
        let first = execute(&tracks, &plan, &src, &dst, Mode::CopyTo, &opts, |_| {});

        // Drop Showbiz from the card: its file goes, and so does its now-empty folder.
        custom.excluded_albums.insert(showbiz);
        let plan = build_plan(&tracks, &index, &opts, &custom);
        let keep: HashSet<String> = plan.iter().map(|p| p.rel.to_string_lossy().to_lowercase()).collect();
        fs::write(dst.join("MUSE/notes.txt"), b"mine").unwrap();
        assert_eq!(prune(&dst, &first.written, &keep), 1);
        assert!(!dst.join("MUSE/01 - Showbiz").exists());
        assert!(dst.join("MUSE/02 - Absolution/01 - Intro.mp3").exists());
        assert!(dst.join("MUSE/notes.txt").exists(), "files we didn't write are never touched");
        fs::remove_dir_all(root).unwrap();
    }
}
