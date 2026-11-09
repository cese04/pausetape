//! Carrying out a plan: copying to a card or moving within the music folder, removing what an
//! earlier write left behind, and cleaning up macOS metadata files.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::journal::save_journal;
use super::plan::{Options, PlanItem};
use crate::library::cue::FRAMES_PER_SEC;
use super::plan::Target;
use crate::library::{Track, carry_cover, write_tags};

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
    /// Asked to stop, it stopped before the end of the plan.
    pub stopped: bool,
    /// The card went away during the copy: what was left of the plan wasn't tried.
    pub card_gone: bool,
}

/// One file a write put on the card, relative to the card root. Recorded so later writes can
/// tell their own files from anything else on the card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub rel: PathBuf,
    pub size: u64,
    /// For a file that isn't a plain copy (converted, cut from an album file, or carrying
    /// corrections): what it was made from (`source_key`). Its size can't be compared with
    /// anything in the music folder, so this says whether it's current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The manifest of a previous write, looked up by card path (lowercased: the card is
/// case-insensitive).
pub fn by_rel(previous: &[Written]) -> HashMap<String, &Written> {
    previous.iter().map(|w| (w.rel.to_string_lossy().to_lowercase(), w)).collect()
}

/// What a card file was made from, when it isn't the library's file as it is: a song made by
/// ffmpeg (converted, or cut out of an album file), or a copy given corrections not yet in the
/// music file. The track, the target format, where it lies in its file, the source's size and
/// date, its tags and any cover being added. `None` for plain copies, which compare by size.
pub fn source_key(t: &Track, target: Target) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let format = match target {
        Target::Copy if !t.dirty => return None,
        Target::Copy => "tagged",
        Target::Flac => "flac",
        Target::FlacCd => "flac16",
        Target::Mp3 => "mp3",
    };
    let span = t.cue.as_ref().map(|c| format!("{}-{}", c.start, c.end.map(|e| e.to_string()).unwrap_or_default())).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&t.title, &t.artist, &t.album_artist, &t.album, t.track, t.disc, t.year, &t.cover).hash(&mut h);
    Some(format!("{}|{format}|{span}|{}|{}|{:016x}", t.id().display(), t.size, t.mtime, h.finish()))
}

/// The card already holds `t` at `dst` as a write would put it there now.
pub fn up_to_date(t: &Track, target: Target, dst: &Path, previous: Option<&Written>) -> bool {
    let size = fs::metadata(dst).ok().map(|m| m.len());
    match source_key(t, target) {
        None => size == Some(t.size),
        Some(key) => previous.is_some_and(|w| w.source.as_deref() == Some(key.as_str()) && size == Some(w.size)),
    }
}

/// Put `t`'s audio at `dst` as `target`: a plain copy, or a file made with ffmpeg, then tagged
/// (ffmpeg's output carries no tags) and given the source's embedded cover.
pub fn materialise(t: &Track, target: Target, dst: &Path) -> Result<()> {
    if target == Target::Copy {
        return plain_copy(&t.path, dst);
    }
    encode(t, target, dst)?;
    write_tags(t, dst)?;
    carry_cover(&t.path, dst)
}

/// Make `dst` from `t` with ffmpeg: the song's part of an album file if it is one (the seek
/// before `-i` is sample-exact when decoding, so consecutive songs join without gaps), as FLAC
/// or as stereo MP3 at 320 kbps.
fn encode(t: &Track, target: Target, dst: &Path) -> Result<()> {
    let ff = crate::audio::ffmpeg().context("ffmpeg is needed to convert or cut songs")?;
    let secs = |f: u64| format!("{}.{:06}", f / FRAMES_PER_SEC, f % FRAMES_PER_SEC * 1_000_000 / FRAMES_PER_SEC);
    let tmp = partial_path(dst);
    let mut cmd = crate::platform::quiet_command(ff);
    cmd.args(["-nostdin", "-v", "error", "-y"]);
    if let Some(c) = t.cue.as_ref().filter(|c| c.start > 0) {
        cmd.args(["-ss", &secs(c.start)]);
    }
    cmd.arg("-i").arg(&t.path);
    if let Some(c) = &t.cue
        && let Some(e) = c.end
    {
        cmd.args(["-t", &secs(e.saturating_sub(c.start))]);
    }
    cmd.args(["-map", "0:a:0", "-map_metadata", "-1"]);
    match target {
        Target::Flac if crate::library::dsd::is_dsd(&t.ext) => {
            // 24-bit (ffmpeg's FLAC takes 32-bit samples as 24).
            let rate = crate::library::dsd::PCM_RATE;
            cmd.args(["-af", &format!("aresample=osr={rate}:osf=s32"), "-c:a", "flac", "-f", "flac"]);
        }
        Target::Flac => {
            cmd.args(["-c:a", "flac", "-f", "flac"]);
        }
        Target::FlacCd => {
            let rate = super::plan::cd_rate(t.sample_rate.unwrap_or(48_000));
            cmd.args(["-af", &format!("aresample=osr={rate}:osf=s16:dither_method=triangular"), "-c:a", "flac", "-f", "flac"]);
        }
        _ => {
            // LAME takes at most 48 kHz; Dolby and other surround mixes fold down to stereo.
            cmd.args(["-c:a", "libmp3lame", "-b:a", "320k", "-ac", "2"]);
            if t.sample_rate.is_some_and(|r| r > 48_000) {
                cmd.args(["-ar", "48000"]);
            }
            cmd.args(["-f", "mp3"]);
        }
    }
    let out = cmd.arg(&tmp).output().context("running ffmpeg")?;
    if !out.status.success() {
        let _ = fs::remove_file(&tmp);
        anyhow::bail!("ffmpeg couldn't convert the song: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    fs::rename(&tmp, dst)?;
    Ok(())
}

/// Run the plan. `tracks` must be the same list the plan was built from. `previous` is the
/// manifest of the last write to `dest` (empty when reorganising). Once `stop` is set, it
/// finishes the file it's on and stops.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    tracks: &[Track],
    plan: &[PlanItem],
    source: &Path,
    dest: &Path,
    mode: Mode,
    opts: &Options,
    previous: &[Written],
    stop: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Report {
    let mut r = Report::default();
    let previous = by_rel(previous);
    if mode == Mode::CopyTo {
        copy_to(tracks, plan, dest, opts, &previous, stop, &mut progress, &mut r);
        if opts.clean_mac_files {
            r.cleaned = clean_mac_files(dest);
        }
        return r;
    }
    for (n, item) in plan.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            r.stopped = true;
            break;
        }
        let t = &tracks[item.track];
        progress(Progress::Step { done: n, total: plan.len() });
        // A song inside an album file can be cut out for the card, but never moved.
        if item.skip.is_some() || t.cue.is_some() {
            r.skipped += 1;
            continue;
        }
        if let Err(e) = move_into_place(t, &dest.join(&item.rel), opts, &mut r) {
            r.errors.push(format!("{}: {e:#}", t.path.display()));
        }
    }
    remove_empty_dirs(source);
    if !r.moves.is_empty()
        && let Err(e) = save_journal(&r.moves)
    {
        r.errors.push(format!("saving undo journal: {e:#}"));
    }
    if opts.clean_mac_files {
        r.cleaned = clean_mac_files(dest);
    }
    r
}

/// Copy mode. Songs that ffmpeg has to make (converted, or cut from an album file) are made by
/// a pool of workers, one per spare core, in a work folder on this computer; one writer puts
/// files on the card: a finished song as soon as there is one, plain copies in between. A card
/// writes one file at a time at best, so the writer never waits on ffmpeg while there's
/// something it could copy.
#[allow(clippy::too_many_arguments)]
fn copy_to(tracks: &[Track], plan: &[PlanItem], dest: &Path, opts: &Options, previous: &HashMap<String, &Written>, stop: &AtomicBool, progress: &mut impl FnMut(Progress), r: &mut Report) {
    use std::sync::{Mutex, mpsc};
    let drive = drive_of(dest);
    let total = plan.len();
    let mut done = 0;
    let mut written: Vec<(usize, Written)> = Vec::new();
    let (mut plain, mut made) = (Vec::new(), Vec::new());
    for (n, item) in plan.iter().enumerate() {
        let t = &tracks[item.track];
        let dst = dest.join(&item.rel);
        if item.skip.is_some() {
            r.skipped += 1;
        } else if t.path == dst || up_to_date(t, item.target, &dst, previous.get(&item.rel.to_string_lossy().to_lowercase()).copied()) {
            r.unchanged += 1;
            written.push((n, Written { rel: item.rel.clone(), size: fs::metadata(&dst).map(|m| m.len()).unwrap_or(t.size), source: source_key(t, item.target) }));
        } else if item.target == Target::Copy {
            plain.push(n);
            continue;
        } else {
            made.push(n);
            continue;
        }
        done += 1;
    }
    progress(Progress::Step { done, total });

    let work = std::env::temp_dir().join(format!("pausetape-card-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    if !made.is_empty()
        && let Err(e) = fs::create_dir_all(&work)
    {
        r.errors.push(format!("making a work folder for conversions: {e}"));
        made.clear();
    }
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).saturating_sub(1).clamp(1, 8).min(made.len().max(1));
    let queue = Mutex::new(made.iter().copied());
    // Bounded, so finished songs can't pile up in the work folder while the card is slow.
    let (tx, rx) = mpsc::sync_channel::<(usize, Result<PathBuf>)>(workers);
    std::thread::scope(|s| {
        for _ in 0..if made.is_empty() { 0 } else { workers } {
            let (tx, queue, work) = (tx.clone(), &queue, &work);
            s.spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Some(n) = queue.lock().unwrap_or_else(|e| e.into_inner()).next() else { break };
                    let item = &plan[n];
                    let ext = item.rel.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                    let out = work.join(format!("{n}.{ext}"));
                    let res = materialise(&tracks[item.track], item.target, &out).map(|()| out.clone());
                    if res.is_err() {
                        let _ = fs::remove_file(&out);
                    }
                    if tx.send((n, res)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut plain = plain.into_iter();
        let mut waiting = made.len();
        loop {
            if stop.load(Ordering::Relaxed) {
                r.stopped = true;
                break;
            }
            // Pulled out: nothing more can go on it, and no folder should be made where it was.
            if !card_there(dest, drive) {
                r.card_gone = true;
                break;
            }
            // A finished song first (its work file goes once it's on the card), else a plain copy,
            // else wait for ffmpeg.
            let (n, res) = match rx.try_recv() {
                Ok(m) => m,
                Err(_) => match plain.next() {
                    Some(n) => (n, Ok(tracks[plan[n].track].path.clone())),
                    None if waiting == 0 => break,
                    None => match rx.recv() {
                        Ok(m) => m,
                        Err(_) => break,
                    },
                },
            };
            let item = &plan[n];
            let t = &tracks[item.track];
            let is_made = item.target != Target::Copy;
            if is_made {
                waiting -= 1;
            }
            let dst = dest.join(&item.rel);
            let placed = res.and_then(|from| {
                let copied = dst
                    .parent()
                    .map_or(Ok(()), |p| fs::create_dir_all(p).with_context(|| format!("creating {}", p.display())))
                    .and_then(|()| plain_copy(&from, &dst));
                if is_made {
                    let _ = fs::remove_file(&from);
                }
                copied
            });
            let tagged = placed.and_then(|()| {
                if t.dirty && opts.write_tags {
                    write_tags(t, &dst)?;
                    r.tagged += 1;
                }
                Ok(())
            });
            match tagged {
                Ok(()) => {
                    r.copied += 1;
                    let size = fs::metadata(&dst).map(|m| m.len()).unwrap_or(t.size);
                    written.push((n, Written { rel: item.rel.clone(), size, source: source_key(t, item.target) }));
                }
                // Pulled out during the copy: the system's answer ("No such file or directory", or
                // "Permission denied" for the folder where the card was) would read as if the song
                // were missing from the library.
                Err(_) if !card_there(dest, drive) => {
                    r.errors.push(format!("{}: the card went away while copying it", t.path.display()));
                    r.card_gone = true;
                    break;
                }
                Err(e) => r.errors.push(format!("{}: {e:#}", t.path.display())),
            }
            done += 1;
            progress(Progress::Step { done, total });
        }
        // Workers waiting to hand over a song give up once nobody listens.
        drop(rx);
    });
    let _ = fs::remove_dir_all(&work);
    // In plan order, as callers (and the manifest) expect.
    written.sort_by_key(|(n, _)| *n);
    r.written = written.into_iter().map(|(_, w)| w).collect();
}

/// Reorganising: move `t` to `dst` within the music folder, then save its corrected tags.
fn move_into_place(t: &Track, dst: &Path, opts: &Options, r: &mut Report) -> Result<()> {
    if t.path != dst {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
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
    } else {
        r.unchanged += 1;
    }
    if t.dirty && opts.write_tags {
        write_tags(t, dst)?;
        r.tagged += 1;
    }
    Ok(())
}

/// Byte-for-byte copy. `std::fs::copy` on macOS also copies extended attributes, which on a
/// FAT/exFAT card materialise as "._name" AppleDouble files that clutter the player's browser.
pub fn plain_copy(src: &Path, dst: &Path) -> Result<()> {
    let tmp = partial_path(dst);
    let copy = || -> Result<()> {
        let mut reader = BufReader::with_capacity(1 << 20, File::open(src)?);
        let mut writer = BufWriter::with_capacity(1 << 20, File::create(&tmp)?);
        io::copy(&mut reader, &mut writer)?;
        writer.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok(())
    };
    // A copy cut short (a full card, say) mustn't leave its half behind taking room.
    if let Err(e) = copy() {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, dst)?;
    Ok(())
}

/// The drive the folder `dest` is on, to tell later whether it's still the card (`card_there`).
pub(super) fn drive_of(dest: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(dest).ok().map(|m| m.dev())
    }
    #[cfg(not(unix))]
    {
        let _ = dest;
        None
    }
}

/// Whether the card at `dest` is still there. On macOS the folder a card was mounted on outlasts
/// it for a moment (about a third of a second, pulled out by force): empty, unreadable, and on the
/// computer's own drive. So it's the drive `dest` was on at the start (`drive_of`) that's looked for.
/// Elsewhere a drive that goes takes its folder with it.
pub(super) fn card_there(dest: &Path, drive: Option<u64>) -> bool {
    dest.is_dir() && (drive.is_none() || drive_of(dest) == drive)
}

/// The end of the name of a file still being made (see `partial_path`).
const PARTIAL: &str = ".pausetape-partial";

/// Where a file for `dst` is made before it takes its name: hidden, beside it. A copy cut short
/// removes its half, but not on a card that has just been pulled out; the name says the half is
/// Pausetape's, so the next Sync can remove it (`remove_partials`) whatever it's choosing then.
pub(super) fn partial_path(dst: &Path) -> PathBuf {
    let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dst.with_file_name(format!(".{name}{PARTIAL}"))
}

/// Remove the halves of files left on the card by a write cut short (the card pulled out
/// mid-copy, a crash), and the folders that held only them. Only files named as `partial_path`
/// names them; hidden folders, such as the system's own, aren't looked in. Returns how many.
pub fn remove_partials(root: &Path) -> usize {
    let halves: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_type().is_dir() || !e.file_name().to_string_lossy().starts_with('.'))
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let n = e.file_name().to_string_lossy();
            n.starts_with('.') && n.ends_with(PARTIAL)
        })
        .map(|e| e.into_path())
        .collect();
    let mut removed = 0;
    for half in halves {
        if fs::remove_file(&half).is_err() {
            continue;
        }
        removed += 1;
        let _ = fs::remove_file(half.with_file_name(format!("._{}", half.file_name().unwrap_or_default().to_string_lossy())));
        let mut dir = half.parent();
        while let Some(d) = dir.filter(|d| *d != root && d.starts_with(root)) {
            if fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    removed
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

/// Remove folders under `root` that hold nothing (or only Finder droppings). Hidden folders,
/// such as the system's own on a card, are left alone.
pub(super) fn remove_empty_dirs(root: &Path) {
    // Deepest first, so parents become empty after their children go.
    let dirs: Vec<PathBuf> = WalkDir::new(root)
        .min_depth(1)
        .contents_first(true)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
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
        let root = std::env::temp_dir().join(format!("pausetape-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for f in ["src/Muse/Absolution/02 - Apocalypse Please.mp3", "src/Muse/Absolution/01 - Intro.mp3", "src/Muse/Absolution/._junk.mp3", "src/Downloads/Muse - Hysteria.mp3"] {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"not really audio").unwrap();
        }
        fs::create_dir_all(root.join("dst")).unwrap();
        root
    }

    #[test]
    fn a_card_is_there_while_its_folder_is_on_its_drive() {
        let root = std::env::temp_dir().join(format!("pausetape-test-card-there-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert!(card_there(&root, drive_of(&root)));
        // The folder a card was mounted on, left behind on another drive.
        #[cfg(unix)]
        assert!(!card_there(&root, drive_of(&root).map(|d| d.wrapping_add(1))));
        fs::remove_dir_all(&root).unwrap();
        assert!(!card_there(&root, None));
    }

    #[test]
    fn only_pausetapes_own_halves_are_removed() {
        let root = std::env::temp_dir().join(format!("pausetape-test-partials-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let ours = partial_path(&root.join("Band/Album/01 - Song.mp3"));
        let kept = ["Band/Album/02 - Song.mp3", "Band/Album/notes.partial", "Other/.hidden.partial", ".Spotlight-V100/.x.mp3.pausetape-partial"];
        for f in kept.iter().map(|f| root.join(f)).chain([ours.clone(), root.join("Gone/Away/.01 - Song.flac.pausetape-partial")]) {
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, b"x").unwrap();
        }
        assert_eq!(ours.file_name().unwrap(), ".01 - Song.mp3.pausetape-partial");
        assert_eq!(remove_partials(&root), 2);
        assert!(!ours.exists() && root.join("Band/Album").is_dir(), "the half goes, its folder stays while it holds songs");
        assert!(!root.join("Gone").exists(), "folders that held only a half go");
        assert!(kept.iter().all(|f| root.join(f).exists()), "anything else stays, and hidden folders aren't looked in");
        // A plain copy leaves no half behind, and takes its name.
        plain_copy(&root.join("Band/Album/02 - Song.mp3"), &root.join("Band/Album/03 - Song.mp3")).unwrap();
        assert_eq!(remove_partials(&root), 0);
        assert!(root.join("Band/Album/03 - Song.mp3").exists());
        let _ = fs::remove_dir_all(&root);
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
        let r = execute(&tracks, &plan, &src, &dst, Mode::CopyTo, &opts, &[], &AtomicBool::new(false), |_| {});
        assert_eq!((r.copied, r.errors.len(), r.written.len()), (3, 0, 3), "{:?}", r.errors);
        assert!(dst.join("Muse/Absolution/01 - Intro.mp3").exists());
        assert!(dst.join("Muse/Singles/Muse - Hysteria.mp3").exists());
        let again = execute(&tracks, &plan, &src, &dst, Mode::CopyTo, &opts, &[], &AtomicBool::new(false), |_| {});
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
        let r = execute(&tracks, &plan, &src, &src, Mode::MoveInPlace, &opts, &[], &AtomicBool::new(false), |_| {});
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
        let first = execute(&tracks, &plan, &src, &dst, Mode::CopyTo, &opts, &[], &AtomicBool::new(false), |_| {});

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
